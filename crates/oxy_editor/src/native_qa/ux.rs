use super::*;
impl NativeQa {
    pub(super) fn check_ux(&mut self, label: &str) -> Result<(), String> {
        match label {
            "ux_base" => {
                self.base = Some(self.editor.state.clone());
            }
            "ux_unchanged" => {
                if self.editor.state != *self.base.as_ref().ok_or("Sem referência")? {
                    return Err("Modal alterou o documento sem confirmação".into());
                }
            }
            "ux_new_3d" => {
                if self.editor.scene().kind != SceneKind::ThreeD
                    || !self.editor.state.project.input_bindings.is_empty()
                {
                    return Err("Novo projeto 3D ou entradas incorretas".into());
                }
                self.base = Some(self.editor.state.clone());
            }
            "ux_scale_100" | "ux_scale_160" | "ux_scale_80" => {
                let expected = if label == "ux_scale_160" {
                    1.6
                } else if label == "ux_scale_80" {
                    0.8
                } else {
                    1.
                };
                if (self.editor.scale - expected).abs() > 0.0001 {
                    return Err(format!(
                        "Escala aplicada indevidamente: {}",
                        self.editor.scale
                    ));
                }
                if self.editor.state != *self.base.as_ref().ok_or("Sem referência")? {
                    return Err("Preferência alterou projeto".into());
                }
            }
            "ux_viewport" => {
                self.ux_viewport = self.surface.native_viewport;
                if self.ux_viewport.is_none() {
                    return Err("Viewport não renderizado".into());
                }
            }
            "ux_refusal" => {
                self.editor.qa_ux_refusal()?;
                if self.ux_viewport != self.surface.native_viewport {
                    return Err("Notificação deslocou o viewport".into());
                }
                if self.editor.state != *self.base.as_ref().ok_or("Sem referência")? {
                    return Err("Atalho inválido alterou documento".into());
                }
            }
            "ux_duplicate" => {
                if self.editor.state.project.scenes.len() != 2 {
                    return Err("Cena não duplicada".into());
                }
            }
            "ux_undo_scene" => {
                if self.editor.state.project != self.base.as_ref().ok_or("Sem referência")?.project
                {
                    return Err("Desfazer cena não restaurou documento".into());
                }
            }
            _ => return Err(format!("Verificação UX desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Opens real native WGPU editor; scripted RawInput only"]
#[cfg(target_os = "windows")]
fn native_ux_m1_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.2.0/m1");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA interface")
            .with_inner_size([1440., 900.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|b| {
            b.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA interface",
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
                Action::Check("ux_base"),
                Action::Click("Novo projeto"),
                Action::Screenshot("new-project.png"),
                Action::Click("Cancelar"),
                Action::Check("ux_unchanged"),
                Action::Click("Novo projeto"),
                Action::Click("Cena 3D\nComece com modelos, profundidade e uma câmera 3D."),
                Action::Click("Criar projeto"),
                Action::Check("ux_new_3d"),
                Action::Click("Interface: 100%"),
                Action::Click("160%"),
                Action::Check("ux_scale_100"),
                Action::Screenshot("pending-scale.png"),
                Action::Click("Cancelar"),
                Action::Check("ux_scale_100"),
                Action::Click("Interface: 100%"),
                Action::Click("160%"),
                Action::Click("Aplicar"),
                Action::Check("ux_scale_160"),
                Action::Screenshot("1440-scale160.png"),
                Action::ResizePhysical(Vec2::new(920., 600.)),
                Action::Click("Interface: 160%"),
                Action::Screenshot("920-scale160.png"),
                Action::Click("80%"),
                Action::Click("Aplicar"),
                Action::Check("ux_scale_80"),
                Action::Click("Interface: 80%"),
                Action::Click("Restaurar 100%"),
                Action::Click("Aplicar"),
                Action::Check("ux_scale_100"),
                Action::Check("ux_viewport"),
                Action::Key(Key::C, false),
                Action::Check("ux_refusal"),
                Action::Screenshot("refusal-920.png"),
                Action::Idle,
                Action::Click("..."),
                Action::Click("Duplicar cena"),
                Action::Check("ux_duplicate"),
                Action::Click("Desfazer"),
                Action::Check("ux_undo_scene"),
                Action::Click("Refazer"),
                Action::Check("ux_duplicate"),
                Action::Click("Projeto"),
                Action::Click("Novo projeto"),
                Action::Click("Cancelar"),
                Action::Check("ux_duplicate"),
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
    std::fs::write(output.join("native-ux.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
