use super::*;
use glam::Vec3;
use oxy_core::{character::*, document::*, physics3d::*};
impl NativeQa {
    pub(super) fn body_patch_drag(
        &mut self,
        axis: usize,
        delta: f32,
        cancel: bool,
    ) -> Result<(), String> {
        let rect = self.surface.native_viewport.ok_or("Viewport ausente")?;
        let scene = self.editor.scene();
        let e = scene.entity("body").unwrap();
        let c = e.character3d.as_ref().unwrap();
        let matrix = scene.world_matrix("body")?;
        let (_, r, s) = world_pose(matrix)?;
        let center =
            matrix.transform_point3(Vec3::from(c.body.collider(false, c.crouch_height)?.center));
        let axis = r * [Vec3::X, Vec3::Y, Vec3::Z][axis];
        let project = |v| {
            oxy_render::collider_debug::project(&self.editor.camera, rect, v)
                .ok_or("Alça fora do viewport")
        };
        let length = s.x * 65. / project(center + axis * s.x)?.distance(project(center)?);
        let from = project(center + axis * length)?;
        let to = project(center + axis * (length + delta * s.x))?;
        self.base = Some(self.editor.state.clone());
        self.drag(from, to);
        if cancel {
            let release = self.events.pop_back().unwrap();
            self.key(Key::Escape, false);
            self.events.push_back(release);
        }
        Ok(())
    }
    pub(super) fn body_patch_input(&mut self, label: &str) -> Result<(), String> {
        let p = self
            .surface
            .native_viewport
            .ok_or("Viewport ausente")?
            .center();
        self.camera_before = Some(self.editor.camera.clone());
        self.before_paint = Some(self.editor.state.clone());
        let key = match label {
            "up" => Key::E,
            "down" => Key::C,
            _ => return Err("Entrada desconhecida".into()),
        };
        self.events.push_back(vec![
            Event::PointerMoved(p),
            Event::PointerButton {
                pos: p,
                button: PointerButton::Secondary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
        self.events.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        for _ in 0..32 {
            self.events.push_back(vec![]);
        }
        self.events.push_back(vec![
            Event::Key {
                key,
                physical_key: Some(key),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: p,
                button: PointerButton::Secondary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ]);
        Ok(())
    }
    pub(super) fn check_body_patch(&mut self, label: &str) -> Result<(), String> {
        let scene = self.editor.scene();
        let offset = scene
            .entity("body")
            .unwrap()
            .character3d
            .as_ref()
            .unwrap()
            .body
            .offset;
        match label {
            "patch_base" => {
                self.hierarchy_base = Some(self.editor.state.clone());
            }
            "patch_field" => {
                if offset != [0., 0.5, 0.] {
                    return Err(format!("Campo não aplicado: {offset:?}"));
                }
            }
            "patch_zero" => {
                if offset != [0.; 3] {
                    return Err(format!("Undo numérico: {offset:?}"));
                }
            }
            "patch_visual" => {
                let mut actual = self.editor.state.project.clone();
                actual.scenes[0]
                    .entity_mut("body")
                    .unwrap()
                    .character3d
                    .as_mut()
                    .unwrap()
                    .body
                    .offset = [0.; 3];
                if actual != self.hierarchy_base.as_ref().unwrap().project {
                    return Err("Offset moveu origem, aparência, filhos ou câmera".into());
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "patch_gizmo" => {
                if (Vec3::from(offset) - Vec3::new(0.5, 0.5, -0.5)).length() > 0.02 {
                    return Err(format!("Gizmo não moveu X/Z corretamente: {offset:?}"));
                }
            }
            "patch_restored" => {
                if self.editor.state != *self.base.as_ref().unwrap() {
                    return Err("Cancelamento/Undo do gesto incompleto".into());
                }
            }
            "patch_play" => {
                let rt = self.editor.runtime.as_ref().ok_or("Jogo não iniciou")?;
                let state = rt
                    .character_state("body")
                    .ok_or("Personagem não simulado")?;
                if !state.grounded || (state.position.y + 0.5).abs() > 0.04 {
                    return Err(format!(
                        "Chão não usou corpo deslocado: {:?}",
                        state.position
                    ));
                }
                let c = rt
                    .scene()
                    .entity("body")
                    .unwrap()
                    .character3d
                    .as_ref()
                    .unwrap();
                let body =
                    c.body
                        .prepare(false, c.crouch_height, state.uniform_scale, state.yaw)?;
                let world = rt.physics_world().ok_or("Física ausente")?;
                let debug = world
                    .debug_shapes()
                    .find(|d| d.id == "body")
                    .ok_or("Overlay ausente")?;
                if !debug.position.abs_diff_eq(body.at(state.position), 1e-4) {
                    return Err("Overlay difere da consulta física".into());
                }
            }
            "patch_stopped" => {
                if self.editor.state != *self.after_paint.as_ref().unwrap() {
                    return Err("Play/Stop alterou documento".into());
                }
            }
            "patch_up" | "patch_down" => {
                let delta = self.editor.camera.eye() - self.camera_before.as_ref().unwrap().eye();
                let sign = if label == "patch_up" { 1. } else { -1. };
                if delta.y * sign <= 0.1 || delta.x.abs() > 1e-4 || delta.z.abs() > 1e-4 {
                    return Err(format!("Navegação vertical incorreta: {delta:?}"));
                }
                if self.editor.state != *self.before_paint.as_ref().unwrap()
                    || !self.editor.patch_tool_is_move()
                {
                    return Err("E/C com RMB alterou ferramenta/documento".into());
                }
            }
            "patch_rotate" => {
                if !self.editor.patch_tool_is_rotate() {
                    return Err("E sem RMB não voltou a Girar".into());
                }
            }
            "patch_collider" => {
                if !self.editor.patch_tool_is_collider() {
                    return Err("C sem RMB não voltou a Editar colisor".into());
                }
            }
            _ => return Err(format!("Verificação desconhecida {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native WGPU movement-body offset field/gizmo/history, runtime and RMB E/C"]
fn native_body_offset_v0331() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.3.1/patch");
    let folder = output.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    let mut p = Project::new("Deslocamento físico e navegação");
    p.scenes[0].kind = SceneKind::ThreeD;
    p.scenes[0].name = "Corpo deslocado · 3D".into();
    let mut e = Entity::new("Jogador", None);
    e.id = "body".into();
    e.transform.position = [0., 0.02, 0.];
    e.character3d = Some(Default::default());
    oxy_core::input_actions::ensure_character(&mut p, e.character3d.as_ref().unwrap());
    let mut visual = Entity::new("Corpo visual", Some(Primitive::Cube));
    visual.id = "visual".into();
    visual.parent = Some(e.id.clone());
    visual.dimensions = [0.6, 1.8, 0.6];
    visual.transform.position = [0.5, 1.4, -0.5];
    visual.material.color = [0.2, 0.55, 0.8, 1.];
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.id = "floor".into();
    floor.transform.position = [0., -0.5, 0.];
    floor.dimensions = [30., 1., 30.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    let mut camera = Entity::new("Câmera principal", None);
    camera.id = "camera".into();
    camera.camera = Some(Default::default());
    camera.camera_rig = Some(CameraRig {
        target: Some("body".into()),
        mode: CameraMode::ThirdPerson,
        ..Default::default()
    });
    oxy_core::input_actions::ensure_camera(&mut p, camera.camera_rig.as_ref().unwrap());
    let mut legacy = Entity::new("Caixa de teste", Some(Primitive::Cube));
    legacy.id = "legacy".into();
    legacy.transform.position = [3., 0.5, 0.];
    legacy.collider = Some(Default::default());
    p.scenes[0].entities = vec![e, visual, floor, camera, legacy];
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine 0.3.3.1 — corpo e navegação",
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
            qa.editor.camera.target = Vec3::new(0., 1., 0.);
            qa.editor.camera.distance = 8.;
            qa.editor.camera.yaw = 0.7;
            qa.editor.camera.pitch = 0.25;
            qa.actions = VecDeque::from([
                Action::SelectEntity("Jogador"),
                Action::Click("Personagem 3D"),
                Action::Click("Corpo de movimento"),
                Action::Check("patch_base"),
                Action::Screenshot("before.png"),
                Action::BodyField("0.5"),
                Action::Check("patch_field"),
                Action::Screenshot("field-y.png"),
                Action::Key(Key::Z, true),
                Action::Check("patch_zero"),
                Action::Key(Key::Y, true),
                Action::Check("patch_field"),
                Action::Click("Editar posição do corpo"),
                Action::BodyDrag(0, 0.5, false),
                Action::Key(Key::Z, true),
                Action::Check("patch_restored"),
                Action::Key(Key::Y, true),
                Action::BodyDrag(2, -0.5, false),
                Action::Check("patch_gizmo"),
                Action::Check("patch_visual"),
                Action::Screenshot("gizmo-xz.png"),
                Action::BodyDrag(0, 0.5, true),
                Action::Check("patch_restored"),
                Action::Key(Key::Escape, false),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::Click("Visualização"),
                Action::Click("Colisores"),
                Action::Key(Key::Escape, false),
                Action::Click("▶ Jogar"),
                Action::Wait(100),
                Action::Check("patch_play"),
                Action::Screenshot("runtime-offset.png"),
                Action::Click("■ Parar"),
                Action::Check("patch_stopped"),
                Action::SelectEntity("Caixa de teste"),
                Action::Key(Key::W, false),
                Action::BodyPatch("up"),
                Action::Check("patch_up"),
                Action::Screenshot("navigation-up.png"),
                Action::BodyPatch("down"),
                Action::Check("patch_down"),
                Action::Key(Key::E, false),
                Action::Check("patch_rotate"),
                Action::Key(Key::C, false),
                Action::Check("patch_collider"),
                Action::Screenshot("released-collider.png"),
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
    std::fs::write(output.join("native-patch.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
