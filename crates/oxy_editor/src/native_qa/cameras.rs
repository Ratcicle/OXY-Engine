use super::*;
use oxy_core::{character::*, document::*, physics3d::*};
impl NativeQa {
    pub(super) fn check_cameras(&mut self, label: &str) -> Result<(), String> {
        let rt = self.editor.runtime.as_ref().ok_or("Runtime ausente")?;
        let camera = rt.active_camera().ok_or("Câmera ausente")?;
        let pose = rt.game_camera_pose().ok_or("Pose ausente")?;
        if !rt.logs.is_empty() {
            return Err(format!("{:?}", rt.logs));
        }
        match label {
            "camera3d_tp" => {
                if rt.camera_mode(camera) != Some(CameraMode::ThirdPerson) || pose.position.z > 1.8
                {
                    return Err(format!(
                        "Câmera não recolheu diante da parede: {:?}",
                        pose.position
                    ));
                }
            }
            "camera3d_fp" => {
                if rt.camera_mode(camera) != Some(CameraMode::FirstPerson) || pose.hidden.len() != 2
                {
                    return Err("Primeira pessoa não ocultou a cabeça".into());
                }
            }
            "camera3d_slide" => {
                let body = rt
                    .scene()
                    .entities
                    .iter()
                    .find(|e| e.character3d.is_some())
                    .unwrap();
                if rt.character_state(&body.id).unwrap().posture != Posture::Sliding {
                    return Err(format!(
                        "Deslize não ativado: {:?}",
                        rt.character_state(&body.id)
                    ));
                }
            }
            "camera3d_safe" => {}
            _ => return Err("Verificação desconhecida".into()),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Real native WGPU, isolated RawInput; physical OS capture is a separate check"]
fn native_cameras_v030() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.0/m5");
    let folder = output.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    let mut p = Project::new("Câmera e percurso");
    let mut config = CharacterConfig::default();
    config.apply_profile(MovementProfile::Parkour);
    config.slide_min_speed = 1.;
    config.slide_exit_speed = 0.;
    config.slide_duration = 2.;
    config.crouch_toggle = true;
    oxy_core::input_actions::ensure_character(&mut p, &config);
    let mut body = Entity::new("Personagem", None);
    body.character3d = Some(config);
    body.transform.position = [0., 0.02, 0.];
    let mut torso = Entity::new("Tronco", Some(Primitive::Cube));
    torso.parent = Some(body.id.clone());
    torso.dimensions = [0.45, 0.9, 0.3];
    torso.transform.position = [0., 0.85, 0.];
    torso.material.color = [0.12, 0.58, 0.67, 1.];
    let mut head = Entity::new("Cabeça", Some(Primitive::Sphere));
    head.parent = Some(body.id.clone());
    head.dimensions = [0.4; 3];
    head.transform.position = [0., 1.5, 0.];
    head.material.color = [0.82, 0.55, 0.29, 1.];
    let rig = CameraRig {
        target: Some(body.id.clone()),
        mode: CameraMode::ThirdPerson,
        hidden: vec![head.id.clone(), torso.id.clone()],
        transition_seconds: 0.12,
        shoulder: 0.45,
        ..Default::default()
    };
    oxy_core::input_actions::ensure_camera(&mut p, &rig);
    let mut camera = Entity::new("Câmera do personagem", None);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(rig);
    let scene = &mut p.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.name = "Órbita, parede e deslize".into();
    scene.entities = vec![body, camera, torso, head];
    for (name, position, size, color) in [
        (
            "Chão",
            [0., -0.5, -12.],
            [24., 1., 44.],
            [0.17, 0.21, 0.26, 1.],
        ),
        (
            "Parede atrás da câmera",
            [0., 2., 2.],
            [8., 4., 0.2],
            [0.35, 0.41, 0.48, 1.],
        ),
        (
            "Referência à esquerda",
            [-3., 1.5, -6.],
            [1., 3., 1.],
            [0.62, 0.35, 0.18, 1.],
        ),
        (
            "Referência à direita",
            [3., 1., -9.],
            [2., 2., 2.],
            [0.17, 0.5, 0.35, 1.],
        ),
    ] {
        let mut e = Entity::new(name, Some(Primitive::Cube));
        e.transform.position = position;
        e.dimensions = size;
        e.material.color = color;
        e.physics3d = Some(Collider3d {
            shape: CollisionShape::Box { size },
            ..Default::default()
        });
        scene.entities.push(e);
    }
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — câmeras 0.3.0",
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
                Action::Wait(24),
                Action::Check("camera3d_tp"),
                Action::Screenshot("third-person-wall.png"),
                Action::Key(Key::Q, false),
                Action::Wait(18),
                Action::Check("camera3d_safe"),
                Action::Screenshot("other-shoulder.png"),
                Action::RelativeLook(Vec2::new(80., -80.)),
                Action::Wait(8),
                Action::Check("camera3d_safe"),
                Action::Screenshot("orbit.png"),
                Action::RelativeLook(Vec2::new(-80., 80.)),
                Action::Key(Key::V, false),
                Action::Wait(24),
                Action::Check("camera3d_fp"),
                Action::Screenshot("first-person.png"),
                Action::HoldSeconds(Key::W, 0.7),
                Action::Key(Key::C, false),
                Action::Wait(3),
                Action::Check("camera3d_slide"),
                Action::Screenshot("slide.png"),
                Action::Key(Key::V, false),
                Action::Wait(24),
                Action::Check("camera3d_safe"),
                Action::Screenshot("third-person-open.png"),
                Action::Key(Key::V, false),
                Action::Wait(24),
                Action::Click("Cena"),
                Action::Check("fp_paused"),
                Action::Click("Jogo"),
                Action::Check("fp_paused"),
                Action::Resize(Vec2::new(920., 600.)),
                Action::InterfaceScale(1.2),
                Action::Screenshot("small-paused.png"),
                Action::Click("■ Parar"),
                Action::Check("fp_stopped"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
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
    std::fs::write(output.join("native-cameras.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
