use super::*;
use glam::Vec3;
use oxy_core::{document::*, physics3d::*};
impl NativeQa {
    pub(super) fn check_physics(&mut self, label: &str) -> Result<(), String> {
        let e = self
            .editor
            .scene()
            .entities
            .iter()
            .find(|e| e.name == "Tubo com abertura")
            .ok_or("Tubo ausente")?;
        let shape = e.physics3d.as_ref().map(|c| &c.shape);
        match label {
            "phys_none" if shape.is_none() => Ok(()),
            "phys_box" if matches!(shape, Some(CollisionShape::Box { .. })) => Ok(()),
            "phys_triangles" | "phys_convex" => {
                let convex = label == "phys_convex";
                if !matches!(
                    (convex, shape),
                    (true, Some(CollisionShape::Convex { .. }))
                        | (false, Some(CollisionShape::TriMesh { .. }))
                ) {
                    return Err(format!("Forma inesperada: {shape:?}"));
                }
                let mut world = PhysicsWorld::new();
                world.sync_scene(self.editor.scene(), false)?;
                let hit = world.ray(Vec3::Y * 4., -Vec3::Y, 8., &QueryOptions::default())?;
                if hit.is_some() != convex {
                    return Err("A abertura não corresponde à forma física escolhida.".into());
                }
                Ok(())
            }
            _ => Err(format!("Verificação física falhou: {label}")),
        }
    }
}
#[test]
#[ignore = "Real native WGPU window, isolated RawInput; no OS capture implied"]
#[cfg(target_os = "windows")]
fn native_physics_v030_authoring() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.3.1/physics");
    std::fs::create_dir_all(&output).unwrap();
    let folder = output.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    let mut project = Project::new("Formas físicas 3D");
    project.scenes[0].kind = SceneKind::ThreeD;
    let mut tube = Entity::new("Tubo com abertura", Some(Primitive::Tube));
    tube.dimensions = [4., 1., 4.];
    tube.segments = 16;
    let mut capsule = Entity::new("Cápsula física", None);
    capsule.transform.position = [4., 1., 0.];
    capsule.physics3d = Some(Collider3d {
        shape: CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        ..Default::default()
    });
    project.scenes[0].entities = vec![tube, capsule];
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — formas físicas 3D",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_min_inner_size([1440., 900.])
                .with_max_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Tubo com abertura"),
                Action::Click("+ Adicionar componente"),
                Action::Click("Colisor 3D"),
                Action::Check("phys_box"),
                Action::Click("Colisor 3D"),
                Action::Click("Usar malha estática de colisão"),
                Action::Check("phys_triangles"),
                Action::Screenshot("triangles-opening.png"),
                Action::Key(Key::Z, true),
                Action::Check("phys_box"),
                Action::Key(Key::Z, true),
                Action::Check("phys_none"),
                Action::Key(Key::Y, true),
                Action::Check("phys_box"),
                Action::Key(Key::Y, true),
                Action::Check("phys_triangles"),
                Action::Click("Gerar colisor convexo"),
                Action::Check("phys_convex"),
                Action::Screenshot("convex-closes-opening.png"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::SelectEntity("Tubo com abertura"),
                Action::Check("phys_convex"),
                Action::SelectEntity("Cápsula física"),
                Action::Screenshot("capsule-empty-group.png"),
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
    std::fs::write(output.join("native-physics.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
