use super::*;
use oxy_core::document::*;
mod checks;

#[test]
#[ignore = "Native WGPU overlay regression; isolated application input"]
#[cfg(target_os = "windows")]
fn native_spatial_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.3.1/spatial");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = std::env::temp_dir().join(format!("oxy-spatial-{}", new_id()));
    std::fs::create_dir_all(&fixture).unwrap();
    let mut project = Project::new("Articulações editáveis");
    let mut group = Entity::new("Personagem", None);
    group.controller = Some(Controller::default());
    group.collider = Some(Collider {
        size: [1.4, 3.4, 1.],
        ..Default::default()
    });
    let id = group.id.clone();
    let scene = &mut project.scenes[0];
    scene.entities.push(group);
    for (name, position, dimensions) in [
        ("Tronco", [0., 0., 0.], [1., 1.5, 1.]),
        ("Cabeça", [0., 1.2, 0.], [0.8, 0.8, 1.]),
        ("Braço", [0.8, 0., 0.], [0.4, 1.5, 1.]),
        ("Braço esquerdo", [-0.8, 0., 0.], [0.4, 1.5, 1.]),
        ("Perna", [-0.3, -1.3, 0.], [0.4, 1., 1.]),
        ("Perna direita", [0.3, -1.3, 0.], [0.4, 1., 1.]),
    ] {
        let mut piece = Entity::new(name, Some(Primitive::Rectangle));
        piece.parent = Some(id.clone());
        piece.transform.position = position;
        piece.dimensions = dimensions;
        piece.material.color = [0.4, 0.6, 0.9, 1.];
        scene.entities.push(piece);
    }
    let mut floor = Entity::new("Chão", Some(Primitive::Rectangle));
    floor.dimensions = [12., 0.5, 1.];
    floor.transform.position = [0., -3., 0.];
    floor.collider = Some(Collider {
        size: floor.dimensions,
        ..Default::default()
    });
    scene.entities.push(floor);
    std::fs::write(
        fixture.join("project.oxy.json"),
        serde_json::to_vec_pretty(&project).unwrap(),
    )
    .unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    let artifacts = output.clone();
    let saved_fixture = fixture.join("project.oxy.json");
    eframe::run_native(
        "OXY Engine — colisores reais",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_min_inner_size([920., 600.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, fixture.join("project.oxy.json"), artifacts, result);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Personagem"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Wait(3),
                Action::Check("selected_group_collider"),
                Action::Screenshot("selected-empty-group-2d.png"),
                Action::Click("Colisor 2D"),
                Action::Click("Ajustar ao grupo/filhos"),
                Action::Wait(2),
                Action::Screenshot("fit-preview-2d.png"),
                Action::Click("Confirmar ajuste"),
                Action::Check("spatial_fit"),
                Action::Key(Key::C, false),
                Action::Screenshot("handles-2d.png"),
                Action::SpatialDrag {
                    face: Some(2),
                    delta: [0., -0.5, 0.],
                    cancel: false,
                },
                Action::Check("spatial_edge"),
                Action::Key(Key::Z, true),
                Action::Check("spatial_undo"),
                Action::Key(Key::Y, true),
                Action::Check("spatial_redo"),
                Action::SpatialDrag {
                    face: None,
                    delta: [0.5, 0.25, 0.],
                    cancel: false,
                },
                Action::Check("spatial_center"),
                Action::SpatialDrag {
                    face: Some(1),
                    delta: [0.75, 0., 0.],
                    cancel: true,
                },
                Action::Check("spatial_cancel"),
                Action::Click("▶ Jogar"),
                Action::Click("Visualização"),
                Action::Click("Colisores"),
                Action::Click("Visualização"),
                Action::Check("spatial_floor"),
                Action::Screenshot("runtime-floor-2d.png"),
                Action::Click("■ Parar"),
                Action::SelectEntity("Braço"),
                Action::Key(Key::P, false),
                Action::SpatialDrag {
                    face: None,
                    delta: [0., 0.75, 0.],
                    cancel: false,
                },
                Action::Check("spatial_pivot"),
                Action::Screenshot("pivot-shoulder-2d.png"),
                Action::Key(Key::Z, true),
                Action::Check("spatial_undo"),
                Action::Key(Key::Y, true),
                Action::Check("spatial_redo"),
                Action::Key(Key::W, false),
                Action::Key(Key::S, true),
                Action::Check("saved_roundtrip"),
                Action::Resize(Vec2::new(920., 600.)),
                Action::Wait(3),
                Action::Screenshot("selected-empty-group-small.png"),
                Action::InterfaceScale(1.2),
                Action::SelectEntity("Personagem"),
                Action::Key(Key::C, false),
                Action::Screenshot("handles-dpi-small.png"),
                Action::SpatialDrag {
                    face: Some(1),
                    delta: [0.5, 0., 0.],
                    cancel: false,
                },
                Action::Check("spatial_edge"),
                Action::InterfaceScale(1.),
                Action::PanZoom,
                Action::Wait(15),
                Action::SpatialDrag {
                    face: Some(0),
                    delta: [-0.5, 0., 0.],
                    cancel: false,
                },
                Action::Check("spatial_edge"),
                Action::CollapseEntity("Personagem"),
                Action::Check("spatial_collapsed"),
                Action::CollapseEntity("Personagem"),
                Action::Check("spatial_expanded"),
                Action::Resize(Vec2::new(1440., 900.)),
                Action::Spatial3D,
                Action::Wait(3),
                // Loaded legacy 3D boxes retain their overlay/editing behavior,
                // but no longer expose legacy authoring in the Inspector.
                // Fit is exercised above in 2D and by the core 3D spatial tests.
                Action::Key(Key::C, false),
                Action::Screenshot("handles-3d.png"),
                Action::SpatialDrag {
                    face: Some(1),
                    delta: [0.5, 0., 0.],
                    cancel: false,
                },
                Action::Check("spatial_edge"),
                Action::SelectEntity("Braço"),
                Action::Key(Key::P, false),
                Action::SpatialDrag {
                    face: None,
                    delta: [0., 0.5, 0.],
                    cancel: false,
                },
                Action::Check("spatial_pivot"),
                Action::Screenshot("pivot-3d.png"),
                Action::Key(Key::W, false),
                Action::Key(Key::S, true),
                Action::Check("saved_roundtrip"),
                Action::Click("Montagem 3D"),
                Action::Click("Cena 2D"),
                Action::SelectEntity("Personagem"),
                Action::Click("Estúdio"),
                Action::Click("Animação"),
                Action::Click("+ Nova animação"),
                Action::Key(Key::A, true),
                Action::Text("Ataque"),
                Action::Key(Key::Enter, false),
                Action::Click("+ Quadro-chave"),
                Action::SelectEntity("Braço"),
                Action::Click("+ Quadro-chave"),
                Action::EditValue("Cursor", "0.5"),
                Action::Check("spatial_animation_baseline"),
                Action::Key(Key::E, false),
                Action::GizmoZ,
                Action::SelectEntity("Personagem"),
                Action::Key(Key::W, false),
                Action::GizmoDelta(35.),
                Action::Check("spatial_drafts"),
                Action::EditValue("Cursor", "0.8"),
                Action::Check("spatial_drafts"),
                // A disabled cursor does not own Ctrl+A: restore the intended single object
                // before testing the separate structural-edit refusal with drafts pending.
                Action::SelectEntity("Personagem"),
                Action::Key(Key::P, false),
                Action::Check("spatial_draft_structure_blocked"),
                Action::OptionalClick("Dispensar"),
                Action::Click("Gravar poses alteradas"),
                Action::Check("spatial_collective_keys"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Screenshot("animation-overlay-2d.png"),
                Action::Key(Key::Z, true),
                Action::Check("spatial_animation_undo"),
                Action::Key(Key::Y, true),
                Action::SelectEntity("Cabeça"),
                Action::Key(Key::P, false),
                Action::Check("spatial_animation_base_mode"),
                Action::Screenshot("animation-base-pivot.png"),
                Action::Key(Key::W, false),
                Action::Check("spatial_animation_restored"),
                Action::SelectEntity("Braço"),
                Action::Key(Key::P, false),
                Action::Check("spatial_track_blocked"),
                Action::OptionalClick("Dispensar"),
                Action::Click("Cena"),
                Action::Key(Key::S, true),
                Action::Check("saved_roundtrip"),
                Action::Idle,
                Action::ReopenProject,
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    std::fs::write(
        output.join("spatial-qa.txt"),
        format!(
            "Done: {}\nError: {:?}\n{:?}\n{:?}",
            report.done, report.error, report.steps, report.screenshots
        ),
    )
    .unwrap();
    assert!(report.done && report.error.is_none(), "{:?}", report.error);
    std::fs::copy(saved_fixture, output.join("spatial-project.oxy.json")).unwrap();
}
