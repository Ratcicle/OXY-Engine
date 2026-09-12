use super::*;
use oxy_core::{
    camera_authoring::CameraField, character::*, document::*, physics3d::*, runtime::CameraPreview,
};
pub(super) struct Measurement {
    pub label: &'static str,
    pub samples: Vec<f64>,
    pub warm: usize,
    pub draws_after_warmup: Option<u64>,
}
impl NativeQa {
    pub(super) fn camera_drag(
        &mut self,
        field: CameraField,
        delta: f32,
        cancel: bool,
    ) -> Result<(), String> {
        let rect = self.surface.native_viewport.ok_or("Viewport ausente")?;
        let id = self
            .editor
            .selected
            .as_deref()
            .ok_or("Câmera não selecionada")?;
        let scene = self.editor.scene();
        let evaluated = CameraPreview::default().evaluate(scene, id, 16. / 9., false)?;
        let handle = oxy_core::camera_authoring::handles(scene, id, &evaluated)?
            .into_iter()
            .find(|h| h.field == field)
            .ok_or("Alça ausente")?;
        let from = oxy_render::collider_debug::project(&self.editor.camera, rect, handle.position)
            .ok_or("Alça fora da vista")?;
        let to = oxy_render::collider_debug::project(
            &self.editor.camera,
            rect,
            handle.position + handle.axis * delta * handle.units,
        )
        .ok_or("Destino fora da vista")?;
        if !rect.contains(from) || !rect.contains(to) {
            return Err("Alça não cabe no viewport".into());
        }
        self.base = Some(self.editor.state.clone());
        self.camera_expected = Some((field, delta));
        self.drag(from, to);
        if cancel {
            let release = self.events.pop_back().unwrap();
            self.key(Key::Escape, false);
            self.events.push_back(release);
        }
        Ok(())
    }
    pub(super) fn camera_measurement_frame(&mut self, elapsed: f64) {
        let Some(m) = &mut self.camera_measurement else {
            return;
        };
        if m.warm > 0 {
            m.warm -= 1;
            if m.warm == 0 {
                m.draws_after_warmup = Some(self.editor.camera_tools.preview_draws);
            }
            return;
        }
        m.samples.push(elapsed);
        if m.samples.len() < 101 {
            return;
        }
        let mut sorted = m.samples.clone();
        assert_eq!(
            m.draws_after_warmup,
            Some(self.editor.camera_tools.preview_draws),
            "Uma cena ociosa redesenhou a prévia"
        );
        sorted.sort_by(f64::total_cmp);
        let data = serde_json::json!({"label":m.label,"cpu_host_update_ms":m.samples,"median_ms":sorted[50],"p95_ms":sorted[95],"p99_ms":sorted[99],"warmup":20,"samples":101,"objects":self.editor.scene().entities.len(),"environment":self.editor.camera_test_metadata(),"preview_draws":self.editor.camera_tools.preview_draws,"preview_draws_after_warmup":m.draws_after_warmup,"camera_evaluations":self.editor.camera_tools.evaluations,"method":"App::update CPU including UI/render preparation and driver submission. No GPU completion or VSync; no FPS inferred."});
        std::fs::write(
            self.output.join(format!("{}.json", m.label)),
            serde_json::to_vec_pretty(&data).unwrap(),
        )
        .unwrap();
        self.camera_measurement = None;
    }
    pub(super) fn check_camera_authoring(&mut self, label: &str) -> Result<(), String> {
        let scene = self.editor.scene();
        match label {
            "v032_drag" => {
                let before = &self.base.as_ref().ok_or("Base ausente")?.project.scenes[0];
                if scene.entity("body") != before.entity("body") {
                    return Err("Gizmo de câmera moveu jogador".into());
                }
                let a = scene.entity("camera").unwrap();
                let b = before.entity("camera").unwrap();
                if a.transform != b.transform || a.camera_rig == b.camera_rig {
                    return Err("Gizmo não alterou configuração ou moveu câmera".into());
                }
                let (field, delta) = self.camera_expected.ok_or("Campo esperado ausente")?;
                let mut expected = b.camera_rig.clone().unwrap();
                field.set(
                    &mut expected,
                    field.get(b.camera_rig.as_ref().unwrap()) + delta,
                )?;
                let mut actual = a.camera_rig.clone().unwrap();
                if (field.get(&actual) - field.get(&expected)).abs() > 0.02 {
                    return Err(format!(
                        "Alça {field:?}: esperado {}, obtido {}",
                        field.get(&expected),
                        field.get(&actual)
                    ));
                }
                field.set(&mut actual, field.get(&expected))?;
                if actual != expected {
                    return Err(format!("Alça {field:?} alterou outro campo"));
                }
            }
            "v032_restored" => {
                if scene != &self.base.as_ref().ok_or("Base ausente")?.project.scenes[0] {
                    return Err("Esc/Undo não restaurou documento".into());
                }
            }
            "v032_preview" => {
                if self.editor.runtime.is_some() || self.editor.camera_tools.preview_draws == 0 {
                    return Err("Prévia não desenhou ou iniciou runtime".into());
                }
            }
            "v032_body" => {
                let e = scene.entity("body").unwrap();
                if e.physics3d.is_some()
                    || !matches!(
                        e.character3d.as_ref().unwrap().body.standing,
                        CollisionShape::Box { .. }
                    )
                {
                    return Err("Autoria não aplicou caixa interna".into());
                }
            }
            _ => return Err(format!("Verificação desconhecida: {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native WGPU 0.3.2 bodies, camera gizmos, preview and measured host updates"]
fn native_camera_body_v032() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.2/authoring");
    let folder = output.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    let mut p = Project::new("Corpo e câmera");
    p.scenes[0].kind = SceneKind::ThreeD;
    p.scenes[0].name = "Corpo e câmera".into();
    let mut body = Entity::new("Jogador", Some(Primitive::Cube));
    body.id = "body".into();
    body.transform.position = [0., 0.02, 0.];
    body.dimensions = [0.6, 1.8, 0.6];
    body.material.color = [0.15, 0.65, 0.6, 1.];
    let config = CharacterConfig::default();
    oxy_core::input_actions::ensure_character(&mut p, &config);
    body.character3d = Some(config);
    let rig = CameraRig {
        target: Some("body".into()),
        mode: CameraMode::ThirdPerson,
        max_distance: 6.,
        ..Default::default()
    };
    oxy_core::input_actions::ensure_camera(&mut p, &rig);
    let mut camera = Entity::new("Câmera principal", None);
    camera.id = "camera".into();
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(rig);
    let mut fixed = Entity::new("Câmera fixa", None);
    fixed.id = "fixed".into();
    fixed.camera = Some(Camera {
        active: false,
        ..Default::default()
    });
    fixed.transform.position = [-3., 3., 5.];
    fixed.transform.rotation = [0., (-25_f32).to_radians(), 0.];
    p.scenes[0].entities = vec![body, camera, fixed];
    for (name, position, size) in [
        ("Chão", [0., -0.5, 0.], [20., 1., 20.]),
        ("Parede", [0., 2., 2.], [5., 4., 0.2]),
    ] {
        let mut e = Entity::new(name, Some(Primitive::Cube));
        e.transform.position = position;
        e.dimensions = size;
        e.material.color = [0.27, 0.30, 0.36, 1.];
        e.physics3d = Some(Collider3d {
            shape: CollisionShape::Box { size },
            ..Default::default()
        });
        p.scenes[0].entities.push(e);
    }
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine 0.3.2 — corpo e câmera",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.editor.camera.target = glam::Vec3::new(0., 1.5, 1.);
            qa.editor.camera.distance = 13.;
            qa.editor.camera.yaw = -2.4;
            qa.editor.camera.pitch = 0.35;
            qa.actions = VecDeque::from([
                Action::SelectEntity("Jogador"),
                Action::Click("Personagem 3D"),
                Action::Click("Corpo de movimento"),
                Action::Click("Cápsula"),
                Action::Click("Caixa"),
                Action::Check("v032_body"),
                Action::Screenshot("movement-box.png"),
                Action::Click("Personagem 3D"),
                Action::Click("Exibir"),
                Action::Click("Mostrar ícones de câmera"),
                Action::Key(Key::Escape, false),
                Action::MeasureCamera("guides-off"),
                Action::SelectEntity("Câmera principal"),
                Action::MeasureCamera("selected-frustum"),
                Action::Screenshot("third-person-guides.png"),
                Action::CameraDrag(CameraField::Eye, 0.25, false),
                Action::Check("v032_drag"),
                Action::Key(Key::Z, true),
                Action::Check("v032_restored"),
                Action::CameraDrag(CameraField::CrouchedEye, 0.15, true),
                Action::Check("v032_restored"),
                Action::CameraDrag(CameraField::Distance, 0.4, false),
                Action::Check("v032_drag"),
                Action::Key(Key::Z, true),
                Action::Check("v032_restored"),
                Action::CameraDrag(CameraField::Shoulder, -0.9, false),
                Action::Check("v032_drag"),
                Action::Screenshot("shoulder-gizmo.png"),
                Action::Key(Key::Z, true),
                Action::Check("v032_restored"),
                Action::CameraDrag(CameraField::FollowHeight, 0.2, false),
                Action::Check("v032_drag"),
                Action::Key(Key::Z, true),
                Action::Check("v032_restored"),
                Action::Click("Exibir"),
                Action::Click("Mostrar proteção contra obstáculos"),
                Action::Key(Key::Escape, false),
                Action::Screenshot("obstruction.png"),
                Action::Click("Controle da câmera"),
                Action::Click("Prévia da câmera"),
                Action::Check("v032_preview"),
                Action::MeasureCamera("preview-open-idle"),
                Action::Screenshot("preview-third-person.png"),
                Action::Click("Fixar"),
                Action::SelectEntity("Jogador"),
                Action::Check("v032_preview"),
                Action::Click("Média"),
                Action::Screenshot("preview-pinned-medium.png"),
                Action::Click("Média"),
                Action::SelectEntity("Câmera principal"),
                Action::Click("Fixar"),
                Action::Click("Prévia agachada"),
                Action::Screenshot("crouched-eyes.png"),
                Action::Click("Prévia agachada"),
                Action::Click("Exibir"),
                Action::Click("Mostrar prévia da câmera"),
                Action::Key(Key::Escape, false),
                Action::MeasureCamera("preview-closed"),
                Action::SelectEntity("Câmera fixa"),
                Action::Screenshot("fixed-frustum.png"),
                Action::SelectEntity("Câmera principal"),
                Action::Click("Básico"),
                Action::Click("Terceira pessoa"),
                Action::Click("Primeira pessoa"),
                Action::Screenshot("first-person-guides.png"),
                Action::Resize(Vec2::new(920., 600.)),
                Action::InterfaceScale(1.2),
                Action::CameraDrag(CameraField::Eye, 0.1, false),
                Action::Check("v032_drag"),
                Action::Key(Key::Z, true),
                Action::Check("v032_restored"),
                Action::Screenshot("small-120-percent.png"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::Click("▶ Jogar"),
                Action::HoldSeconds(Key::W, 0.4),
                Action::Click("■ Parar"),
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
    std::fs::write(output.join("native-authoring.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
