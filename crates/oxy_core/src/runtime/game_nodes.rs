//! Operations about the game as a whole, exposed by the typed graph adapter: the scene running.
use super::*;
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
            // Logic runs only in the running scene, so it is the owner's scene.
            "scene.current" => match output {
                "scene" => Ok(Value::Text(self.scene_id.clone())),
                "name" => Ok(Value::Text(self.scene().name.clone())),
                _ => Err(format!("Cena atual não tem a saída {output}")),
            },
            _ => self.math_data(graph, node, output, context, budget, depth),
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
                 Cena de Cena atual)"
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
