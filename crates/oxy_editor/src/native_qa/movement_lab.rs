use super::*;
use oxy_core::{character::CameraMode, document::Value};
#[path = "../../../oxy_core/examples/support/movement.rs"]
mod scale_fixture;
impl NativeQa {
    pub(super) fn check_movement_lab(&mut self, label: &str) -> Result<(), String> {
        let count = self.editor.state.project.scenes[0]
            .entities
            .iter()
            .filter(|e| e.character3d.is_some())
            .count();
        if label == "lab3d_preset" {
            return if count == 2 {
                Ok(())
            } else {
                Err(format!("Preset não criou personagem: {count}"))
            };
        }
        if label == "lab3d_base" {
            return if count == 1 {
                Ok(())
            } else {
                Err(format!("Histórico não restaurou montagem: {count}"))
            };
        }
        if label == "lab3d_stopped" {
            return if self.editor.runtime.is_none()
                && self.editor.state.project.scenes[0]
                    .entities
                    .iter()
                    .find(|e| e.character3d.is_some())
                    .unwrap()
                    .transform
                    .position
                    == [0., 0.02, 4.]
            {
                Ok(())
            } else {
                Err("Teste alterou pose-base".into())
            };
        }
        let rt = self.editor.runtime.as_ref().ok_or("Runtime ausente")?;
        if !rt.logs.is_empty() {
            return Err(format!("{:?}", rt.logs));
        }
        if label == "lab3d_scale_running" {
            return if count == 50
                && rt.scene().entities.len() == 400
                && rt
                    .physics_world()
                    .is_some_and(|w| w.counters().colliders == 251)
            {
                Ok(())
            } else {
                Err("Carga nativa não manteve 50 personagens / 400 objetos / 251 formas".into())
            };
        }
        let body = rt
            .scene()
            .entities
            .iter()
            .find(|e| e.character3d.is_some())
            .unwrap();
        let state = rt
            .character_state(&body.id)
            .ok_or("Estado de movimento ausente")?;
        match label {
            "lab3d_paused" if self.editor.capture || !rt.paused => {
                Err("Saída não liberou entrada e pausou".into())
            }
            "lab3d_tp"
                if rt.camera_mode(rt.active_camera().unwrap()) != Some(CameraMode::ThirdPerson) =>
            {
                Err("Modo da câmera não mudou".into())
            }
            "lab3d_terrace"
                if state.position.y < 3.2
                    || body.attributes["Checkpoint"] != Value::Text("Terraço".into()) =>
            {
                Err(format!("Não chegou ao terraço: {:?}", state.position))
            }
            "lab3d_reset"
                if state
                    .position
                    .distance(body.attributes["Destino"].vector3().unwrap())
                    > 0.1 =>
            {
                Err("R não restaurou checkpoint".into())
            }
            _ => Ok(()),
        }
    }
}
#[test]
#[ignore = "Native WGPU 400-entity/50-character regression; not a GPU benchmark"]
fn native_movement_scale_v030() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.0/m7-scale");
    let path = output.join("project/project.oxy.json");
    let mut project = scale_fixture::fixture(50, "dense");
    project.scenes[0].name = "50 personagens · 400 objetos".into();
    // Inspection camera only: keep every body in the native QA image. The CPU
    // benchmark retains its original following camera and original workload.
    for e in &mut project.scenes[0].entities {
        e.material.color = if e.character3d.is_some() {
            [0.9, 0.6, 0.15, 1.]
        } else {
            [0.22, 0.29, 0.36, 1.]
        };
        if e.camera.is_some() {
            e.camera.as_mut().unwrap().fov = 75.;
            e.camera_rig.as_mut().unwrap().mode = CameraMode::Fixed;
            e.transform.position = [0., 25., 55.];
            e.transform.rotation = [-(25_f32 / 55.).atan(), 0., 0.];
        }
    }
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine 0.3.0 — 50 personagens / 400 objetos",
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
                Action::Click("▶ Jogar"),
                Action::Wait(30),
                Action::Check("lab3d_scale_running"),
                Action::HoldSeconds(Key::W, 1.5),
                Action::Check("lab3d_scale_running"),
                Action::Screenshot("50-characters.png"),
                Action::Key(Key::Escape, false),
                Action::Click("Cena"),
                Action::Click("■ Parar"),
                Action::Idle,
                Action::Key(Key::S, true),
                Action::ReopenProject,
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let r = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}",
        r.done,
        r.error,
        r.steps.join("\n")
    );
    std::fs::write(output.join("native-scale.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
#[test]
#[ignore = "Real native WGPU editor, isolated RawInput; not a physical mouse capture test"]
fn native_movement_lab_v030() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = std::env::var_os("OXY_MOVEMENT_QA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.0/m6"));
    let path = output.join("project/project.oxy.json");
    oxy_core::persistence::save_project(
        &path,
        &oxy_core::guide_recipes::movement_laboratory().unwrap(),
    )
    .unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — laboratório 0.3.0",
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
                Action::Check("lab3d_base"),
                Action::Click("+ Objeto"),
                Action::Click("Personagem em terceira pessoa"),
                Action::Check("lab3d_preset"),
                Action::Key(Key::Z, true),
                Action::Check("lab3d_base"),
                Action::Key(Key::Y, true),
                Action::Check("lab3d_preset"),
                Action::Key(Key::Z, true),
                Action::Check("lab3d_base"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::Scroll("HIERARQUIA", 10000.),
                Action::SelectEntity("Jogador"),
                Action::Click("Lógica"),
                Action::Screenshot("movement-graph.png"),
                Action::Click("Cena"),
                Action::Click("▶ Jogar"),
                Action::Wait(20),
                Action::Check("lab3d_running"),
                Action::Screenshot("first-person-start.png"),
                Action::Key(Key::V, false),
                Action::Wait(20),
                Action::Check("lab3d_tp"),
                Action::Screenshot("third-person-start.png"),
                Action::HoldSeconds(Key::W, 4.9),
                Action::Wait(15),
                Action::Check("lab3d_terrace"),
                Action::Screenshot("terrace-checkpoint.png"),
                Action::Key(Key::I, false),
                Action::Wait(10),
                Action::Key(Key::R, false),
                Action::Wait(8),
                Action::Check("lab3d_reset"),
                Action::Resize(Vec2::new(920., 600.)),
                Action::InterfaceScale(1.2),
                Action::Screenshot("small-runtime.png"),
                Action::Key(Key::Escape, false),
                Action::Click("Cena"),
                Action::Check("lab3d_paused"),
                Action::Click("■ Parar"),
                Action::Check("lab3d_stopped"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::Idle,
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
    std::fs::write(output.join("native-laboratory.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
