//! Operations about the game as a whole, exposed by the typed graph adapter: the game
//! variables, the saved progress, the scene running and the scenes logic names, the game pause
//! and quitting.
use super::*;
use crate::{
    save::{self, SaveRequest, SaveSlot},
    variables::{self, GameVariable, MAX_KEYS, MAX_TEXT},
};
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
            "save.state" => Ok(Value::Bool(self.run.save.exists)),
            "game.read" => Ok(Value::Bool(self.game_paused)),
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
            // The values as they are now, to the host; the run reads them back only in a new run.
            "save.write" => {
                let persistent: HashSet<&str> = self
                    .source
                    .variables
                    .iter()
                    .filter(|v| v.persistent)
                    .map(|v| v.id.as_str())
                    .collect();
                let values: BTreeMap<(Id, String), Value> = self
                    .run
                    .variables
                    .iter()
                    .filter(|((id, _), _)| persistent.contains(id.as_str()))
                    .map(|(entry, value)| (entry.clone(), value.clone()))
                    .collect();
                self.run.save.apply(SaveRequest::Write(values.clone()));
                self.request_save(SaveRequest::Write(values));
            }
            "save.clear" => {
                for variable in self.source.variables.iter().filter(|v| v.persistent) {
                    let id = &variable.id;
                    self.run.variables.retain(|(owner, _), _| owner != id);
                    self.run
                        .variables
                        .insert((id.clone(), String::new()), variable.value.clone());
                }
                self.run.save.apply(SaveRequest::Clear);
                self.request_save(SaveRequest::Clear);
            }
            "game.pause" => match node.text("mode") {
                "pause" => {
                    self.set_game_paused(true);
                }
                "resume" => {
                    self.set_game_paused(false);
                }
                "toggle" => self.toggle_game_pause(),
                _ => return Err("Ação de pausa desconhecida; use pause/resume/toggle".into()),
            },
            "game.quit" => self.quit_requested = true,
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
        variables::labeled(&self.source, &self.run.variables, limit)
    }

    /// The saved progress as the run sees it: what the host gave it, then what Salvar progresso
    /// and Apagar progresso made of it.
    pub fn save_slot(&self) -> &SaveSlot {
        &self.run.save
    }

    /// Whether the game paused itself (Pausar / retomar jogo, or Esc in a scene with a pause
    /// menu): the world stands still while input events, interface clicks and logic go on. The
    /// host's own pause is `paused`; the two are kept apart, and either may come first.
    pub fn game_paused(&self) -> bool {
        self.game_paused
    }

    /// The game clock: `time` less the seconds spent in the game pause. Waits, Ler tempo, Ao
    /// passo and the characters' timing windows run on it, so it stands still in the pause.
    pub fn game_time(&self) -> f64 {
        self.time - self.run.paused_for
    }

    /// Starts or ends the game pause; false when it already was so, or the run stopped. A
    /// change fires Ao pausar ou retomar in the running scene, whatever caused it. The shown
    /// poses settle on the last step, and what logic asked of the characters during the pause
    /// (movement, jumps, slides) is dropped as it ends.
    pub fn set_game_paused(&mut self, paused: bool) -> bool {
        if self.stopped || self.game_paused == paused {
            return false;
        }
        self.game_paused = paused;
        self.characters.settle();
        if !paused {
            self.characters.forget_requests();
        }
        if let Some(recording) = &mut self.recording {
            recording.pauses.push(paused);
        }
        if self.event_used("event.pause") {
            self.emit(RuntimeEvent::Pause(paused));
        }
        true
    }

    pub fn toggle_game_pause(&mut self) {
        self.set_game_paused(!self.game_paused);
    }

    /// Whether the running scene has a pause menu, logic that listens to Ao pausar ou retomar:
    /// Esc then pauses the game rather than the host.
    pub fn has_pause_menu(&self) -> bool {
        self.event_used("event.pause")
    }

    /// Where between the last two steps the world is shown: in the game pause, at the last.
    pub(super) fn presentation_alpha(&self) -> f32 {
        if self.game_paused {
            1.
        } else {
            (self.accumulator / FIXED_DT).clamp(0., 1.)
        }
    }

    /// Starts the persistent variables at the values `slot` holds, before the scene start
    /// logic. A value the project's variables do not take is skipped with a log line, and the
    /// variable keeps its initial value.
    pub(super) fn load_save(&mut self, slot: SaveSlot) {
        let mut keys: HashMap<Id, usize> = HashMap::new();
        for ((id, key), value) in &slot.values {
            let checked = save::check(&self.source, id, key, value).and_then(|variable| {
                let count = keys.entry(id.clone()).or_default();
                if key.is_empty() {
                    return Ok(());
                }
                if *count == MAX_KEYS {
                    return Err(format!(
                        "a variável {} já guarda valores para {MAX_KEYS} chaves, o limite",
                        variable.name
                    ));
                }
                *count += 1;
                Ok(())
            });
            match checked {
                Ok(()) => {
                    self.run
                        .variables
                        .insert((id.clone(), key.clone()), value.clone());
                }
                Err(reason) => self.log(format!("Progresso salvo ignorado: {reason}")),
            }
        }
        self.run.save = slot;
    }

    /// Queues `request` for the host. A host that leaves many queued gets them folded into the
    /// same outcome: the last Apagar progresso and the last Salvar progresso after it.
    fn request_save(&mut self, request: SaveRequest) {
        self.save_requests.push(request);
        if self.save_requests.len() > MAX_SAVE_REQUESTS {
            let clear = self
                .save_requests
                .iter()
                .rposition(|r| *r == SaveRequest::Clear);
            let write = self
                .save_requests
                .iter()
                .rposition(|r| matches!(r, SaveRequest::Write(_)))
                .filter(|&write| clear.is_none_or(|clear| write > clear));
            let kept: Vec<usize> = [clear, write].into_iter().flatten().collect();
            let mut index = 0;
            self.save_requests.retain(|_| {
                index += 1;
                kept.contains(&(index - 1))
            });
        }
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
