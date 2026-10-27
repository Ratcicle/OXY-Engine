use super::*;
use oxy_core::document::{Layout, Value};

const DOOR: &str = "scripts/porta.luau";
const BROKEN: &str = "scripts/quebrado.luau";
/// The door of the code tab's flow: it prints at line 3 and fails at line 6 every step.
const VIEWED: &str = "local Porta = {}\nfunction Porta.on_start(self)\n    print(\"Porta pronta\")\nend\n\
                      function Porta.on_step(self)\n    self.attr.Nada = 1\nend\nreturn Porta\n";
/// The same door after an agent fixed it.
const FIXED: &str = "local Porta = {}\n-- Consertado pelo agente.\nfunction Porta.on_start(self)\n    \
                     print(\"Porta consertada\")\nend\nreturn Porta\n";

impl NativeQa {
    pub(super) fn check_scripts(&mut self, label: &str) -> Result<(), String> {
        if label.starts_with("sc7_view") {
            return self.check_code_view(label);
        }
        let door = self
            .editor
            .scene()
            .entities
            .iter()
            .find(|e| e.name == "Porta")
            .ok_or("Porta sumiu")?
            .clone();
        let paths: Vec<&str> = door.scripts.iter().map(|s| s.path.as_str()).collect();
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let undo = self.editor.qa_undo_label();
        let undo_is = |expected: &str| {
            if undo == Some(expected) {
                Ok(())
            } else {
                Err(format!("Desfazer nomeia {undo:?}, não {expected}"))
            }
        };
        match label {
            "sc7_empty" => {
                if !door.scripts.is_empty() || !visible("Nenhum script.") {
                    return Err("O cartão Scripts não diz que a Porta não tem scripts".into());
                }
                if self.surface.texts.iter().any(|t| t.text.starts_with("{ }")) {
                    return Err("A Hierarquia marcou { } num objeto sem scripts".into());
                }
            }
            "sc7_menu" => {
                for text in [
                    "Buscar script…",
                    "scripts",
                    "porta",
                    "quebrado",
                    "Pista",
                    "contagem",
                ] {
                    if !visible(text) {
                        return Err(format!("+ Anexar script não lista {text}"));
                    }
                }
            }
            "sc7_attached" => {
                if paths != [DOOR] || !door.scripts[0].enabled {
                    return Err(format!("Anexar pela lista deixou {paths:?}"));
                }
                if door.attributes.get("Aberta") != Some(&Value::Bool(false)) {
                    return Err("Anexar não criou o atributo declarado Aberta".into());
                }
                undo_is("Anexar script")?;
                for text in [
                    "{ } 1",
                    "porta.luau",
                    "Usado por 1 objeto · ",
                    "sem erros",
                    "pedido por porta.luau",
                ] {
                    if !visible(text) {
                        return Err(format!("O cartão ou a Hierarquia não mostram {text}"));
                    }
                }
            }
            "sc7_off" => {
                if door.scripts.first().is_none_or(|s| s.enabled) {
                    return Err("O interruptor Ligado não desligou o script".into());
                }
                undo_is("Desligar script")?;
            }
            "sc7_on" => {
                if door.scripts.first().is_none_or(|s| !s.enabled) {
                    return Err("Ctrl+Z não religou o script".into());
                }
            }
            "sc7_failing" => {
                if paths != [DOOR, BROKEN] {
                    return Err(format!("O segundo script não foi anexado: {paths:?}"));
                }
                if !visible("{ } 2 !") {
                    return Err("A Hierarquia não marcou o erro do script com !".into());
                }
                if !visible("1 erro na linha 2") {
                    return Err("O cartão não diz a linha do erro de quebrado.luau".into());
                }
            }
            "sc7_moved" => {
                if paths != [BROKEN, DOOR] {
                    return Err(format!("Mover para cima deixou {paths:?}"));
                }
                undo_is("Reordenar scripts")?;
            }
            "sc7_code" => {
                if self.editor.qa_code_view() != Some((BROKEN, 2)) {
                    return Err(format!(
                        "Ver código abriu {:?}, não quebrado.luau na linha 2",
                        self.editor.qa_code_view()
                    ));
                }
                if !visible("Somente leitura") || !visible(BROKEN) {
                    return Err("A aba do código não mostra o caminho somente leitura".into());
                }
            }
            "sc7_code_closed" => {
                if self.editor.qa_code_view().is_some() || visible("Somente leitura") {
                    return Err("Fechar código não fechou a aba".into());
                }
            }
            "sc7_detached" => {
                if paths != [DOOR] {
                    return Err(format!("Desanexar deixou {paths:?}"));
                }
                undo_is("Remover script")?;
                if !visible("{ } 1") || visible("{ } 1 !") {
                    return Err("A Hierarquia não voltou a { } 1 sem erro".into());
                }
                if !door.attributes.contains_key("Aberta") {
                    return Err("Desanexar tirou o atributo declarado".into());
                }
            }
            "sc7_detach_undone" => {
                if paths != [BROKEN, DOOR] {
                    return Err(format!(
                        "Ctrl+Z não devolveu o script desanexado: {paths:?}"
                    ));
                }
            }
            _ => return Err(format!("Verificação de scripts desconhecida: {label}")),
        }
        Ok(())
    }

    /// The checks of the code tab's flow: opened from the card and from a Console error, the
    /// line asked for highlighted, a reload while it shows, the file deleted, and the seal of a
    /// graph of nodes.
    fn check_code_view(&mut self, label: &str) -> Result<(), String> {
        let visible = |text: &str| self.surface.texts.iter().any(|t| t.text == text);
        let starts = |text: &str| self.surface.texts.iter().any(|t| t.text.starts_with(text));
        let root = self
            .editor
            .path
            .as_deref()
            .and_then(Path::parent)
            .ok_or("O projeto não tem pasta")?
            .to_owned();
        let state = self.editor.qa_code_state();
        let text = state.and_then(|(text, _, _)| text).unwrap_or_default();
        let shown = state.map_or((0, 0), |(_, shown, _)| shown);
        match label {
            "sc7_view_card" => {
                if self.editor.qa_code_view() != Some((DOOR, 1)) {
                    return Err(format!(
                        "Ver código abriu {:?}, não porta.luau na linha 1",
                        self.editor.qa_code_view()
                    ));
                }
                for text in [DOOR, "Somente leitura", "Usado por Porta", "Copiar caminho"] {
                    if !visible(text) {
                        return Err(format!("O cabeçalho do código não mostra {text}"));
                    }
                }
                if !text.contains("Porta pronta") || shown.0 != 1 {
                    return Err(format!("A aba não mostra o código do início: {shown:?}"));
                }
                if visible("1 erro") {
                    return Err("O código diz que falha antes do teste".into());
                }
            }
            "sc7_view_console" => {
                for text in [
                    "Script",
                    "Porta · Porta pronta",
                    "porta.luau:3",
                    "Erro",
                    "porta.luau:6 →",
                ] {
                    if !visible(text) {
                        return Err(format!("O Console não mostra {text}"));
                    }
                }
                if !visible("Recarga dos scripts") {
                    return Err("As Propriedades não dizem que o teste recarrega os scripts".into());
                }
                if !starts("Porta · o atributo Nada") {
                    return Err("O Console não mostra o erro sem o lugar repetido".into());
                }
                let repeated = self.surface.texts.iter().any(|t| {
                    t.text
                        .strip_prefix('×')
                        .and_then(|n| n.parse::<u64>().ok())
                        .is_some_and(|n| n >= 2)
                });
                if !repeated {
                    return Err("O erro repetido não mostra quantas vezes aconteceu".into());
                }
            }
            "sc7_view_filtered" => {
                if !visible("Mostrar: Erros e avisos") || !visible("porta.luau:6 →") {
                    return Err("Mostrar: Erros e avisos escondeu o erro".into());
                }
                if visible("porta.luau:3") {
                    return Err("Mostrar: Erros e avisos mostrou o print".into());
                }
            }
            "sc7_view_line" => {
                if self.editor.qa_code_view() != Some((DOOR, 6)) {
                    return Err(format!(
                        "O link do erro abriu {:?}, não porta.luau na linha 6",
                        self.editor.qa_code_view()
                    ));
                }
                if !(shown.0..=shown.1).contains(&6) {
                    return Err(format!("A linha 6 não ficou à vista: {shown:?}"));
                }
                if !visible("1 erro") || !starts("Linha 6: ") {
                    return Err("O erro não aparece embaixo da linha 6".into());
                }
            }
            "sc7_view_rewrite" => {
                std::fs::write(root.join(DOOR), FIXED).map_err(|e| e.to_string())?;
            }
            "sc7_view_reloaded" => {
                let reloaded = "Script recarregado: scripts/porta.luau (1 objeto)";
                if !self.editor.console.messages.iter().any(|m| m == reloaded) {
                    return Err("O Console não disse que o script recarregou".into());
                }
                if !text.contains("Porta consertada") {
                    return Err("A aba não releu o arquivo regravado".into());
                }
                if visible("1 erro") || starts("Linha 6: ") {
                    return Err("O erro corrigido continua marcado".into());
                }
            }
            "sc7_view_fixed_error" => {
                if !visible("Erro") || !visible("porta.luau:6 →") {
                    return Err("O Console perdeu o erro que a recarga corrigiu".into());
                }
                if !starts("Porta · o atributo Nada") || starts("Porta · scripts/porta.luau:6") {
                    return Err("O erro corrigido voltou a mostrar o lugar no texto".into());
                }
            }
            "sc7_view_delete" => {
                std::fs::remove_file(root.join(DOOR)).map_err(|e| e.to_string())?;
            }
            "sc7_view_deleted" => {
                if !state.is_some_and(|(_, _, deleted)| deleted) || !visible("Arquivo excluído") {
                    return Err("A aba não disse que o arquivo foi excluído".into());
                }
            }
            "sc7_view_seal" => {
                if !visible("Nós (legado)") {
                    return Err("A Lógica não mostra o selo Nós (legado) no grafo".into());
                }
            }
            "sc7_view_no_seal" => {
                if visible("Nós (legado)") {
                    return Err("O selo Nós (legado) apareceu num objeto sem nós".into());
                }
            }
            _ => return Err(format!("Verificação do código desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Real native WGPU Scripts card, Hierarchy mark and code tab; RawInput only"]
#[cfg(target_os = "windows")]
fn native_scripts_card() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.7/scripts-card");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-scripts-card-{}", new_id()));
    for (path, text) in [
        (
            DOOR,
            "local Porta = {}\nPorta.attributes = { Aberta = false }\nfunction Porta.on_start(self)\n    \
             self.attr.Aberta = false\nend\nreturn Porta\n",
        ),
        (BROKEN, "local M = {\nreturn M\n"),
        ("Pista/contagem.luau", "return {}\n"),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let path = root.join("project.oxy.json");
    let mut project = oxy_core::editing::blank_project("Scripts", SceneKind::ThreeD).unwrap();
    project.scenes[0]
        .entities
        .push(oxy_core::document::Entity::new("Porta", None));
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA cartão Scripts",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA cartão Scripts")
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
                Action::SelectEntity("Porta"),
                Action::Wait(4),
                Action::Check("sc7_empty"),
                Action::Click("+ Anexar script"),
                Action::Wait(4),
                Action::Check("sc7_menu"),
                Action::Screenshot("scripts-attach-menu.png"),
                Action::Click("porta"),
                Action::Wait(4),
                Action::Check("sc7_attached"),
                Action::Screenshot("scripts-card.png"),
                Action::Click("porta.luau ligado"),
                Action::Wait(2),
                Action::Check("sc7_off"),
                Action::Key(Key::Z, true),
                Action::Wait(2),
                Action::Check("sc7_on"),
                Action::Click("+ Anexar script"),
                Action::Wait(4),
                Action::Click("quebrado"),
                Action::Wait(6),
                Action::Check("sc7_failing"),
                Action::Screenshot("scripts-error.png"),
                Action::Click("Mais ações de quebrado.luau"),
                Action::Click("Mover para cima"),
                Action::Wait(2),
                Action::Check("sc7_moved"),
                Action::Click("Ver código"),
                Action::Wait(4),
                Action::Check("sc7_code"),
                Action::Screenshot("scripts-code.png"),
                Action::Click("Fechar código"),
                Action::Wait(2),
                Action::Check("sc7_code_closed"),
                Action::Click("Mais ações de quebrado.luau"),
                Action::Click("Desanexar"),
                Action::Wait(4),
                Action::Check("sc7_detached"),
                Action::Key(Key::Z, true),
                Action::Wait(2),
                Action::Check("sc7_detach_undone"),
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
    std::fs::write(output.join("native-scripts-card.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 4, "{text}");
}

#[test]
#[ignore = "Real native WGPU code tab, Console script lines and the Nós (legado) seal; RawInput only"]
#[cfg(target_os = "windows")]
fn native_scripts_viewer() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = oxy_core::qa::output_dir("v0.7/scripts-viewer");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-scripts-viewer-{}", new_id()));
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join(DOOR), VIEWED).unwrap();
    let path = root.join("project.oxy.json");
    let mut project = oxy_core::editing::blank_project("Código", SceneKind::ThreeD).unwrap();
    // A 0.7 project, in folders: opening one of the old layout leaves its notice up until it is
    // dismissed, over the right end of the dock, where the Console puts its links.
    project.layout = Layout::Tree;
    let scene = project.start_scene.clone();
    // A tile whose logic is a node that waits for a click, which the test never gives.
    oxy_core::command::apply(
        &mut project,
        &scene,
        &oxy_core::command::parse(&serde_json::json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Ladrilho"},
            {"op": "add_node", "entity": "$0", "operation": "event.click"},
        ]))
        .unwrap(),
    )
    .unwrap();
    let mut door = oxy_core::document::Entity::new("Porta", None);
    door.scripts = vec![oxy_core::script::ScriptRef::new(DOOR)];
    project.scenes[0].entities.push(door);
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA visualizador de código",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_title("OXY Engine — QA visualizador de código")
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
                Action::SelectEntity("Porta"),
                Action::Wait(4),
                Action::Click("Ver código"),
                Action::Wait(4),
                Action::Check("sc7_view_card"),
                Action::Screenshot("viewer-card.png"),
                Action::Click("Jogar"),
                Action::Wait(30),
                Action::Click("Console"),
                Action::Wait(4),
                Action::Check("sc7_view_console"),
                Action::Screenshot("viewer-console.png"),
                Action::Click("Mostrar: Tudo"),
                Action::Click("Erros e avisos"),
                Action::Wait(4),
                Action::Check("sc7_view_filtered"),
                Action::Click("porta.luau:6 →"),
                Action::Wait(4),
                Action::Check("sc7_view_line"),
                Action::Screenshot("viewer-error-line.png"),
                // An agent fixes the file while the test runs: the test reloads it and the tab
                // reads it again.
                Action::Check("sc7_view_rewrite"),
                // The scripts' watcher looks every 300 ms and reloads at two equal looks.
                Action::WaitSeconds(1.5),
                Action::Click("print(\"Porta consertada\")"),
                Action::Wait(2),
                Action::Check("sc7_view_reloaded"),
                Action::Screenshot("viewer-reloaded.png"),
                // The error the reload fixed stays in the Console as it was, its link included.
                Action::Click("Console"),
                Action::Wait(4),
                Action::Check("sc7_view_fixed_error"),
                Action::Click("porta.luau:6 →"),
                Action::Wait(4),
                Action::Click("Parar"),
                Action::Wait(4),
                Action::Check("sc7_view_delete"),
                Action::Wait(60),
                Action::Click("Arquivo excluído"),
                Action::Check("sc7_view_deleted"),
                Action::Screenshot("viewer-deleted.png"),
                Action::SelectEntity("Ladrilho"),
                Action::Click("Lógica"),
                Action::Wait(4),
                Action::Check("sc7_view_seal"),
                Action::Screenshot("viewer-seal.png"),
                Action::SelectEntity("Porta"),
                Action::Wait(4),
                Action::Check("sc7_view_no_seal"),
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
    std::fs::write(output.join("native-scripts-viewer.txt"), &text).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    assert!(report.done && report.error.is_none(), "{text}");
    assert_eq!(report.screenshots.len(), 6, "{text}");
}
