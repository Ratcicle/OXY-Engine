//! Operations about the game as a whole, exposed by the typed graph adapter: the game
//! variables, the scene running and the scenes logic names.
use super::*;
use crate::variables::{self, GameVariable, MAX_KEYS, MAX_TEXT};
impl Runtime {
    pub(super) fn game_data(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        output: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        match node.operation.as_str() {
            "variable.get" => {
                let variable = self.declared_variable(node)?;
                let key = self.variable_key(graph, node, context, budget, depth + 1)?;
                Ok(self
                    .run
                    .variables
                    .get(&(variable.id.clone(), key))
                    .unwrap_or(&variable.value)
                    .clone())
            }
            // Logic runs only in the running scene, so it is the owner's scene.
            "scene.current" => match output {
                "scene" => Ok(Value::Text(self.scene_id.clone())),
                "name" => Ok(Value::Text(self.scene().name.clone())),
                _ => Err(format!("Cena atual não tem a saída {output}")),
            },
            // An empty scene is a node left unset; validation refuses a scene the project lacks
            // before the game runs, and the node refuses it as well.
            "value.scene" => match node.text("scene") {
                "" => Err("Referência de cena sem cena escolhida: escolha a cena no nó".into()),
                scene if self.source.scene(scene).is_none() => {
                    Err("Referência de cena nomeia uma cena que o projeto não tem".into())
                }
                scene => Ok(Value::Text(scene.into())),
            },
            _ => self.math_data(graph, node, output, context, budget, depth),
        }
    }

    /// Runs `node` when it is one of these actions, else hands it to the math ones; false when
    /// no one runs it.
    pub(super) fn game_action(
        &mut self,
        graph: &PreparedGraph,
        node: &Node,
        context: &mut Context,
        budget: &mut usize,
    ) -> Result<bool, String> {
        match node.operation.as_str() {
            "variable.set" => {
                let GameVariable {
                    id,
                    name,
                    value: declared,
                    ..
                } = self.declared_variable(node)?.clone();
                let key = self.variable_key(graph, node, context, budget, 0)?;
                let value = self.input_value(graph, node, "value", context, budget, 0)?;
                if std::mem::discriminant(&value) != std::mem::discriminant(&declared) {
                    return Err(format!(
                        "Valor incompatível com o tipo da variável {name}: ela guarda {}, e o \
                         Novo valor é {}",
                        variables::kind(&declared),
                        variables::kind(&value)
                    ));
                }
                variables::check_value(&value)
                    .map_err(|error| format!("Variável {name}: {error}"))?;
                let entry = (id.clone(), key);
                // The variable's own value is always there; the limit counts the keys.
                if !entry.1.is_empty() && !self.run.variables.contains_key(&entry) {
                    let keys = self
                        .run
                        .variables
                        .range((id.clone(), String::new())..)
                        .take_while(|((owner, _), _)| *owner == id)
                        .filter(|((_, key), _)| !key.is_empty())
                        .count();
                    if keys >= MAX_KEYS {
                        return Err(format!(
                            "A variável {name} já guarda valores para {MAX_KEYS} chaves, o limite"
                        ));
                    }
                }
                self.run.variables.insert(entry, value);
            }
            _ => return self.math_action(graph, node, context, budget),
        }
        Ok(true)
    }

    /// The variable a variable node names, as the project declares it.
    fn declared_variable(&self, node: &Node) -> Result<&GameVariable, String> {
        let label = crate::graph::definition(&node.operation).map_or("", |def| def.label);
        match node.text("variable") {
            "" => Err(format!(
                "{label} sem variável escolhida: escolha a variável no nó"
            )),
            id => variables::get(&self.source, id)
                .ok_or_else(|| format!("{label} nomeia uma variável que o projeto não tem")),
        }
    }

    /// The key a variable node reads or writes: its Chave input when connected, else its
    /// parameter; empty for the variable's own value.
    fn variable_key(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<String, String> {
        match self.input_value(graph, node, "key", context, budget, depth)? {
            Value::Text(key) if key.len() > MAX_TEXT => {
                Err(format!("A chave passa de {MAX_TEXT} bytes"))
            }
            Value::Text(key) => Ok(key),
            _ => Err("A entrada Chave precisa receber um texto".into()),
        }
    }

    /// The value the game variable `id` holds under `key` ("" for its own value): the last one
    /// written, or the declared one. None for a variable the project lacks.
    pub fn variable(&self, id: &str, key: &str) -> Option<&Value> {
        let declared = variables::get(&self.source, id)?;
        Some(
            self.run
                .variables
                .get(&(id.to_owned(), key.to_owned()))
                .unwrap_or(&declared.value),
        )
    }

    /// The values the game variables hold now, by the variable's name, or "name · key" for the
    /// value of a key: at most `limit`, the variables' own values first and then the keys', each
    /// group in label order, with how many were left out.
    pub fn variable_values(&self, limit: usize) -> (BTreeMap<String, Value>, usize) {
        let names: HashMap<&str, &str> = self
            .source
            .variables
            .iter()
            .map(|v| (v.id.as_str(), v.name.as_str()))
            .collect();
        let mut labeled: Vec<(bool, String, &Value)> = self
            .run
            .variables
            .iter()
            .filter_map(|((id, key), value)| {
                let name = names.get(id.as_str())?;
                Some((!key.is_empty(), variables::label(name, key), value))
            })
            .collect();
        labeled.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let left_out = labeled.len().saturating_sub(limit);
        let values = labeled
            .into_iter()
            .take(limit)
            .map(|(_, label, value)| (label, value.clone()))
            .collect();
        (values, left_out)
    }

    /// The scene Mudar de cena opens: the one its Cena input receives when connected, else the
    /// one its parameter names (an empty or unknown one fails as the scene change opens it).
    pub(super) fn scene_destination(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        context: &Context,
        budget: &mut usize,
    ) -> Result<Id, String> {
        if graph.input(&node.id, "scene").is_none() {
            return Ok(node.text("scene").into());
        }
        let Value::Text(scene) = self.input_value(graph, node, "scene", context, budget, 0)? else {
            return Err(
                "A entrada Cena precisa receber o identificador de uma cena, em texto".into(),
            );
        };
        if scene.is_empty() {
            return Err("Cena de destino vazia: a entrada Cena recebeu um texto vazio".into());
        }
        if self.source.scene(&scene).is_none() {
            // The name of a scene is the likely mix-up: Cena atual gives both.
            let hint = if self.source.scenes.iter().any(|s| s.name == scene) {
                ", o nome de uma cena; a entrada espera o identificador (para reiniciar, a saída \
                 Cena de Cena atual; para outra cena, Referência de cena)"
            } else {
                ""
            };
            return Err(format!(
                "Cena de destino não existe: a entrada Cena recebeu \"{scene}\"{hint}"
            ));
        }
        Ok(scene)
    }
}
