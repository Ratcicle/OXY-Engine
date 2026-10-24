use super::*;
use oxy_core::document::Value;

const DOOR: &str = "scripts/porta.luau";
const BROKEN: &str = "scripts/quebrado.luau";

impl NativeQa {
    pub(super) fn check_scripts(&mut self, label: &str) -> Result<(), String> {
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
