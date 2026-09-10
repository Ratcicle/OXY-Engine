use super::*;
use glam::Vec3;
use oxy_core::{character::CameraRig, document::*, physics3d::*};
impl NativeQa {
    pub(super) fn check_first_person(&mut self, label: &str) -> Result<(), String> {
        let body = self
            .editor
            .scene()
            .entities
            .iter()
            .find(|e| e.name == "Personagem")
            .ok_or("Personagem ausente")?;
        match label {
            "fp_author" => {
                if body.character3d.is_none()
                    || !matches!(
                        body.physics3d.as_ref().map(|c| &c.shape),
                        Some(CollisionShape::Capsule { .. })
                    )
                {
                    return Err("O botão não criou controlador/cápsula".into());
                }
            }
            "fp_none" => {
                if body.character3d.is_some() || body.physics3d.is_some() {
                    return Err("Undo não restaurou a ausência dos componentes".into());
                }
            }
            "fp_running" => {
                let rt = self.editor.runtime.as_ref().ok_or("Runtime ausente")?;
                let state = rt
                    .character_state(&body.id)
                    .ok_or("Estado de personagem ausente")?;
                if state.position.x < 0.5
                    || state.position.z.abs() > 0.1
                    || !(rt.game_camera_pose().ok_or("Câmera ausente")?.rotation * -Vec3::Z)
                        .abs_diff_eq(Vec3::X, 0.001)
                {
                    return Err(format!(
                        "Entrada/câmera não corresponderam ao movimento: {state:?}"
                    ));
                }
                if !rt.logs.is_empty() {
                    return Err(format!("{:?}", rt.logs));
                }
            }
            "fp_paused" => {
                if self.editor.capture || !self.editor.runtime.as_ref().is_some_and(|rt| rt.paused)
                {
                    return Err("Entrada não foi liberada/pausada".into());
                }
            }
            "fp_stopped" => {
                if self.editor.runtime.is_some() || body.transform.position != [0., 0.02, 0.] {
                    return Err("Play alterou a pose-base".into());
                }
            }
            _ => return Err(format!("Verificação desconhecida: {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native WGPU and isolated RawInput. Does not certify physical mouse capture"]
fn native_first_person_v030() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.0/m2");
    let folder = output.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    let mut p = Project::new("Primeira pessoa");
    let scene = &mut p.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.name = "Primeira pessoa · 3D".into();
    let mut body = Entity::new("Personagem", None);
    body.transform.position = [0., 0.02, 0.];
    let mut camera = Entity::new("Câmera", None);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(body.id.clone()),
        ..Default::default()
    });
    camera.parent = Some(body.id.clone());
    let mut floor = Entity::new("Chão sólido", Some(Primitive::Cube));
    floor.dimensions = [40., 1., 40.];
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    let mut column = Entity::new("Referência visual", Some(Primitive::Cube));
    column.dimensions = [1., 3., 1.];
    column.transform.position = [7., 1.5, 0.];
    column.material.color = [0.1, 0.65, 0.6, 1.];
    scene.entities = vec![body, camera, floor, column];
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — primeira pessoa",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 800.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Personagem"),
                Action::Click("Componentes"),
                Action::Click("Personagem 3D"),
                Action::Click("Adicionar personagem 3D"),
                Action::Check("fp_author"),
                Action::Key(Key::Z, true),
                Action::Check("fp_none"),
                Action::Key(Key::Y, true),
                Action::Check("fp_author"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::Click("▶ Jogar"),
                Action::Wait(8),
                Action::RelativeLook(Vec2::new(750., 0.)),
                Action::HoldSeconds(Key::W, 0.4),
                Action::Check("fp_running"),
                Action::Screenshot("first-person.png"),
                Action::Click("Cena"),
                Action::Check("fp_paused"),
                Action::Click("Jogo"),
                Action::Check("fp_paused"),
                Action::Click("▶ Retomar · Pausado"),
                Action::Key(Key::Escape, false),
                Action::Check("fp_paused"),
                Action::Click("■ Parar"),
                Action::Check("fp_stopped"),
                Action::SelectEntity("Personagem"),
                Action::Screenshot("authored-capsule.png"),
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
    std::fs::write(output.join("native-first-person.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
