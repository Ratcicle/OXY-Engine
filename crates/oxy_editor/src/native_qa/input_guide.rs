use super::*;
impl NativeQa {
    pub(super) fn check_input_guide(&mut self, label: &str) -> Result<(), String> {
        match label {
            "m2_recipe" => {
                let original = oxy_core::guide::first_recipe()?;
                if self.editor.state.project != original {
                    return Err("Receita não abriu os dados originais".into());
                }
                if self
                    .editor
                    .path
                    .as_ref()
                    .is_none_or(|p| !p.starts_with(std::env::temp_dir()))
                {
                    return Err("Receita não foi aberta como cópia".into());
                }
                self.base = Some(self.editor.state.clone());
            }
            "m2_binding" => {
                let before = &self.base.as_ref().ok_or("Sem referência")?.project;
                let id = before.input_bindings.keys().next().unwrap();
                if self
                    .editor
                    .state
                    .project
                    .input_bindings
                    .get(id)
                    .map(String::as_str)
                    != Some("L")
                    || self
                        .editor
                        .state
                        .project
                        .input_labels
                        .get(id)
                        .map(String::as_str)
                        != Some("Mensagem local")
                {
                    return Err("Ação não foi renomeada/remapeada".into());
                }
                if before.scenes != self.editor.state.project.scenes {
                    return Err("Renomear ação alterou os IDs ou grafos".into());
                }
            }
            "m2_refused" => {
                if self.editor.state.project.input_bindings.len() != 1
                    || !self
                        .surface
                        .texts
                        .iter()
                        .any(|t| t.text.contains("Altere estes vínculos"))
                {
                    return Err("Exclusão de ação referenciada não foi bloqueada".into());
                }
            }
            "m2_pressed" | "m2_released" | "m2_held" => {
                let rt = self.editor.runtime.as_ref().ok_or("Jogo não iniciou")?;
                let count = rt
                    .logs
                    .iter()
                    .filter(|l| l.contains("Minha primeira ação funciona!"))
                    .count();
                if (label == "m2_held" && count < 2) || (label != "m2_held" && count != 1) {
                    return Err(format!("{label}: {count} mensagens"));
                }
                let times: Vec<_> = rt
                    .traces
                    .iter()
                    .filter(|t| t.operation == "debug.message")
                    .map(|t| t.time)
                    .collect();
                if times.windows(2).any(|t| t[1] - t[0] < 0.016) {
                    return Err("Evento de manter repetido no mesmo passo".into());
                }
            }
            _ => return Err(format!("Verificação desconhecida: {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Real native WGPU guide, binding and three input modes; RawInput only"]
#[cfg(target_os = "windows")]
fn native_input_guide_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.2.1/m2");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        persist_window: false,
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA lógica e guia")
            .with_inner_size([1440., 900.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|b| {
            b.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA lógica e guia",
        options,
        Box::new(move |cc| {
            let mut qa = NativeQa::new(
                cc,
                workspace.join("examples/validacao/project.oxy.json"),
                artifacts,
                shared,
            );
            qa.editor = Editor::new(cc);
            qa.actions = VecDeque::from([
                Action::Click("Guia de lógica visual"),
                Action::Screenshot("guide-first.png"),
                Action::Idle,
                Action::Scroll("Guia de lógica visual", -650.),
                Action::Click("Abrir cópia da receita: primeira mensagem"),
                Action::Check("m2_recipe"),
                Action::Click("Ações de entrada"),
                Action::Click("Mostrar mensagem"),
                Action::Key(Key::A, true),
                Action::Text("Mensagem local"),
                Action::Key(Key::Enter, false),
                Action::Click("K"),
                Action::Click("L"),
                Action::Check("m2_binding"),
                Action::Click("Excluir ação"),
                Action::Check("m2_refused"),
                Action::Screenshot("input-actions.png"),
                Action::Click("Cancelar exclusão"),
                Action::Click("Fechar ações de entrada"),
                Action::SelectNode("Ação de entrada"),
                Action::Click("▶ Jogar"),
                Action::Hold(Key::L, 40),
                Action::Wait(20),
                Action::Check("m2_pressed"),
                Action::Click("■ Parar"),
                Action::Click("Pressionar"),
                Action::Click("Manter"),
                Action::Click("▶ Jogar"),
                Action::Hold(Key::L, 40),
                Action::Wait(20),
                Action::Check("m2_held"),
                Action::Click("■ Parar"),
                Action::Click("Manter"),
                Action::Click("Soltar"),
                Action::Click("▶ Jogar"),
                Action::Hold(Key::L, 40),
                Action::Wait(20),
                Action::Check("m2_released"),
                Action::Click("■ Parar"),
                Action::Click("Guia de lógica visual"),
                Action::Click("Buscar operação ou assunto"),
                Action::Text("Esperar"),
                Action::Click("Esperar sem bloquear"),
                Action::Screenshot("guide-node.png"),
                Action::Key(Key::Escape, false),
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
    std::fs::write(output.join("native-input-guide.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
