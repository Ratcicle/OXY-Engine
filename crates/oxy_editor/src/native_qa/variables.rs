use super::*;
use oxy_core::document::Value;
impl NativeQa {
    pub(super) fn check_variables(&mut self, label: &str) -> Result<(), String> {
        let project = &self.editor.state.project;
        let variable = project.variables.iter().find(|v| v.name == "Melhor tempo");
        let node = project
            .scenes
            .iter()
            .flat_map(|scene| &scene.entities)
            .flat_map(|entity| &entity.graph.nodes)
            .find(|node| node.operation == "variable.set")
            .ok_or("Nó Alterar variável do jogo ausente")?;
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let undo = self.editor.qa_undo_label();
        match label {
            "var_created" => {
                let variable = variable.ok_or("Criar variável não criou Melhor tempo")?;
                if project.variables.len() != 1
                    || variable.value != Value::Number(0.)
                    || variable.persistent
                {
                    return Err("Variável criada com tipo, valor ou persistência errados".into());
                }
                if undo != Some("Criar variável do jogo") {
                    return Err(format!(
                        "Desfazer nomeia {undo:?}, não Criar variável do jogo"
                    ));
                }
                if !visible("não usada") {
                    return Err("Usada por não diz que a variável nova não é usada".into());
                }
            }
            "var_persistent" => {
                if !variable.is_some_and(|v| v.persistent) {
                    return Err("O interruptor não marcou Salva entre partidas".into());
                }
                if undo != Some("Editar variável do jogo") {
                    return Err(format!(
                        "Desfazer nomeia {undo:?}, não Editar variável do jogo"
                    ));
                }
                if !visible("Sim") {
                    return Err("Salva entre partidas não mostra Sim".into());
                }
            }
            "var_picked" => {
                let variable = variable.ok_or("Variável sumiu")?;
                if node.params.get("variable") != Some(&Value::Text(variable.id.clone())) {
                    return Err("O seletor não guardou a variável pelo identificador".into());
                }
                if node.params.get("value") != Some(&Value::Number(0.)) {
                    return Err("Novo valor não segue o tipo da variável".into());
                }
                if !visible("Melhor tempo · Número") || !visible("Melhor tempo") {
                    return Err("Seletor ou corpo do nó não mostram o nome da variável".into());
                }
            }
            "var_delete_refused" => {
                if variable.is_none() {
                    return Err("A variável em uso foi excluída".into());
                }
                if !visible("Altere estes nós antes de excluir a variável:")
                    || !visible("Variáveis → Placar → Alterar variável do jogo")
                {
                    return Err("A exclusão não listou o nó que usa a variável".into());
                }
                if !visible("Variáveis → Placar") {
                    return Err("Usada por não nomeia a cena e o objeto".into());
                }
            }
            "var_second" => {
                if project.variables.len() != 2
                    || !project.variables.iter().any(|v| v.name == "Mortes")
                {
                    return Err("Enter no nome não criou a segunda variável".into());
                }
                if undo != Some("Criar variável do jogo") {
                    return Err(format!(
                        "Desfazer nomeia {undo:?}, não Criar variável do jogo"
                    ));
                }
            }
            "var_renamed" => {
                let names: Vec<_> = project.variables.iter().map(|v| v.name.as_str()).collect();
                if names != ["Melhor tempo", "Quedas"] {
                    return Err(format!("Renomear na tabela deixou {names:?}"));
                }
                if undo != Some("Editar variável do jogo") {
                    return Err(format!(
                        "Desfazer nomeia {undo:?}, não Editar variável do jogo"
                    ));
                }
            }
            "var_unused" => {
                if !visible("Excluir a variável Quedas? Nenhum nó a usa.") {
                    return Err("A exclusão de uma variável sem uso não pediu confirmação".into());
                }
            }
            "var_deleted" => {
                if project.variables.len() != 1 || variable.is_none() {
                    return Err("Excluir variável não excluiu só Quedas".into());
                }
                if undo != Some("Excluir variável do jogo") {
                    return Err(format!(
                        "Desfazer nomeia {undo:?}, não Excluir variável do jogo"
                    ));
                }
            }
            "var_kept" => {
                let variable = variable.ok_or("A variável em uso foi excluída")?;
                if self.surface.modal.is_some() {
                    return Err("Fechar não fechou Variáveis do jogo".into());
                }
                if node.params.get("variable") != Some(&Value::Text(variable.id.clone())) {
                    return Err("Cancelar a exclusão mudou o nó".into());
                }
            }
            _ => return Err(format!("Verificação desconhecida: {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Real native WGPU game variables dialog and picker; RawInput only"]
#[cfg(target_os = "windows")]
fn native_game_variables_workflow() {
    use oxy_core::command;
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.6/variables");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-variables-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("project.oxy.json");
    let mut project = oxy_core::editing::blank_project("Variáveis", SceneKind::TwoD).unwrap();
    project.scenes[0].name = "Variáveis".into();
    let scene = project.start_scene.clone();
    let commands = command::parse(&serde_json::json!([
        {"op": "create_entity", "kind": {"type": "group"}, "name": "Placar"},
        {"op": "add_node", "entity": "$0", "operation": "event.scene_start",
         "position": [40, 60]},
        {"op": "add_node", "entity": "$0", "operation": "variable.set", "position": [320, 60]},
        {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
         "to_node": "$2", "to_port": "exec"},
    ]))
    .unwrap();
    command::apply(&mut project, &scene, &commands).unwrap();
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA variáveis do jogo",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA variáveis do jogo")
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::Click("Lógica"),
                Action::SelectEntity("Placar"),
                Action::Click("Variáveis do jogo"),
                Action::Wait(4),
                Action::Screenshot("variables-empty.png"),
                Action::Click("Nome da nova variável"),
                Action::Text("Melhor tempo"),
                Action::Click("Criar variável"),
                Action::Check("var_created"),
                Action::Click("Salva entre partidas: Melhor tempo"),
                Action::Check("var_persistent"),
                Action::Screenshot("variables.png"),
                Action::Click("Fechar variáveis do jogo"),
                Action::SelectNode("Alterar variável do jogo"),
                Action::Click("Escolha…"),
                Action::Click("Melhor tempo · Número"),
                Action::Wait(4),
                Action::Check("var_picked"),
                Action::Screenshot("variable-picker.png"),
                Action::Click("Variáveis do jogo"),
                Action::Click("Excluir variável Melhor tempo"),
                Action::Wait(4),
                Action::Check("var_delete_refused"),
                Action::Screenshot("variable-delete-refused.png"),
                Action::Click("Cancelar exclusão"),
                Action::Click("Nome da nova variável"),
                Action::Text("Mortes"),
                Action::Key(Key::Enter, false),
                Action::Check("var_second"),
                Action::Click("Mortes"),
                Action::Key(Key::A, true),
                Action::Text("Quedas"),
                Action::Key(Key::Enter, false),
                Action::Check("var_renamed"),
                Action::Click("Excluir variável Quedas"),
                Action::Wait(4),
                Action::Check("var_unused"),
                Action::Screenshot("variable-delete-unused.png"),
                Action::Click("Confirmar exclusão da variável"),
                Action::Check("var_deleted"),
                Action::Click("Fechar variáveis do jogo"),
                Action::Wait(4),
                Action::Check("var_kept"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-variables.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 5, "{text}");
}
