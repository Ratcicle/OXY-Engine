use glam::{Vec2, Vec3};
use oxy_core::{
    character::*,
    document::*,
    physics3d::*,
    runtime::{FIXED_DT, InputFrame, Runtime},
    surface::*,
};
const BODY: &str = "00000000-0000-4000-8000-000000000002";
fn project() -> Project {
    let mut project = Project::new("Parkour testável");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", None);
    floor.id = "00000000-0000-4000-8000-000000000001".into();
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [500., 1., 500.],
        },
        ..Default::default()
    });
    let mut body = Entity::new("Corpo", None);
    body.id = BODY.into();
    body.transform.position = [0., 0.02, 0.];
    body.character3d = Some(CharacterConfig::default());
    let mut camera = Entity::new("Olhar", None);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(BODY.into()),
        ..Default::default()
    });
    scene.entities = vec![floor, body, camera];
    project
}
fn runtime(project: &Project) -> Runtime {
    Runtime::new(project, &project.start_scene).unwrap()
}
fn settle(rt: &mut Runtime) {
    for _ in 0..30 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
}

#[test]
fn air_acceleration_preserves_perpendicular_momentum_and_mouse_alone_adds_none() {
    for moving in [false, true] {
        let mut p = project();
        let body = p.scenes[0].entity_mut(BODY).unwrap();
        body.transform.position[1] = 4.;
        let c = body.character3d.as_mut().unwrap();
        c.apply_profile(MovementProfile::ChainedJumps);
        c.gravity = 0.;
        let mut rt = runtime(&p);
        rt.set_character_velocity(BODY, Vec3::X * 6.).unwrap();
        for i in 0..6 {
            rt.advance(
                FIXED_DT,
                &InputFrame {
                    movement: if moving { [0., 1.] } else { [0.; 2] },
                    look: if i == 0 { [-1500., 0.] } else { [0.; 2] },
                    ..Default::default()
                },
            );
        }
        let state = rt.character_state(BODY).unwrap();
        assert!((state.velocity.x - 6.).abs() < 0.001, "{state:?}");
        if moving {
            assert!(
                (state.velocity.z - 6.).abs() < 0.001 && state.velocity.length() > 8.4,
                "{state:?}"
            );
        } else {
            assert!(state.velocity.abs_diff_eq(Vec3::X * 6., 0.001));
        }
    }
}

#[test]
fn manual_and_automatic_jump_have_distinct_held_rules() {
    for mode in [JumpMode::Manual, JumpMode::Automatic] {
        let mut p = project();
        p.scenes[0]
            .entity_mut(BODY)
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .jump_mode = mode;
        let mut rt = runtime(&p);
        settle(&mut rt);
        let mut jumps = 0;
        let mut lands = 0;
        for i in 0..180 {
            rt.advance(
                FIXED_DT,
                &InputFrame {
                    held: ["pular".into()].into(),
                    pressed: if i == 0 {
                        ["pular".into()].into()
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            );
            let mut step_jumps = 0;
            let mut step_lands = 0;
            for (_, event) in rt.movement_events() {
                match event {
                    MovementEvent::Jumped => step_jumps += 1,
                    MovementEvent::Landed { .. } => step_lands += 1,
                    _ => {}
                }
            }
            assert!(step_jumps <= 1 && step_lands <= 1);
            jumps += step_jumps;
            lands += step_lands;
        }
        if mode == JumpMode::Manual {
            assert_eq!(jumps, 1);
            assert_eq!(lands, 1);
        } else {
            assert!(jumps >= 4 && lands >= 3, "{jumps}/{lands}");
        }
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    }
}

#[test]
fn landing_rejump_keeps_momentum_and_only_prepares_vertical_velocity() {
    for jump in [false, true] {
        let mut p = project();
        let body = p.scenes[0].entity_mut(BODY).unwrap();
        body.transform.position[1] = 0.15;
        let c = body.character3d.as_mut().unwrap();
        c.landing_retention = 0.25;
        c.jump_retention = 0.8;
        let mut rt = runtime(&p);
        rt.set_character_velocity(BODY, Vec3::new(6., -3., 0.))
            .unwrap();
        if jump {
            rt.request_jump(BODY).unwrap();
        }
        let mut observed = false;
        for _ in 0..8 {
            rt.advance(FIXED_DT, &InputFrame::default());
            if rt
                .movement_events()
                .iter()
                .any(|(_, e)| matches!(e, MovementEvent::Landed { .. }))
            {
                let s = rt.character_state(BODY).unwrap();
                assert!(
                    (s.velocity.x - if jump { 4.8 } else { 1.5 }).abs() < 0.001,
                    "{s:?}"
                );
                assert!(s.position.y < 0.03);
                assert_eq!(s.velocity.y, if jump { 8. } else { 0. });
                observed = true;
                break;
            }
        }
        assert!(
            observed,
            "jump={jump}, state={:?}, logs={:?}",
            rt.character_state(BODY),
            rt.logs
        );
    }
}

#[test]
fn slide_uses_surface_friction_and_finishes_crouched_under_ceiling() {
    let mut speeds = Vec::new();
    for preset in [SurfacePreset::Common, SurfacePreset::Ice] {
        let mut p = project();
        let s = SurfaceMaterial::preset(preset);
        p.scenes[0].entities[0].physics3d.as_mut().unwrap().surface = Some(s.id.clone());
        p.surfaces.push(s);
        p.scenes[0]
            .entity_mut(BODY)
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .apply_profile(MovementProfile::Parkour);
        let mut rt = runtime(&p);
        settle(&mut rt);
        rt.set_character_velocity(BODY, Vec3::X * 12.).unwrap();
        rt.request_slide(BODY).unwrap();
        for _ in 0..30 {
            rt.advance(FIXED_DT, &InputFrame::default());
            assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Sliding);
        }
        speeds.push(rt.character_state(BODY).unwrap().velocity.x);
        let mut ceiling = Entity::new("Teto do túnel", None);
        ceiling.id = "ceiling".into();
        ceiling.transform.position = [0., 1.3, 0.];
        ceiling.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: [200., 0.2, 4.],
            },
            ..Default::default()
        });
        rt.scene_mut().entities.push(ceiling);
        for _ in 0..40 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Crouched);
        rt.request_crouch(BODY, false).unwrap();
        rt.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Crouched);
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    }
    assert!(speeds[1] > speeds[0] + 3., "{speeds:?}");
}

#[test]
fn profiles_are_editable_data_and_air_limits_are_independent() {
    let mut p = project();
    let c = p.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap();
    let actions = c.actions.clone();
    c.apply_profile(MovementProfile::Parkour);
    c.apply_profile(MovementProfile::ChainedJumps);
    assert_eq!(c.actions, actions);
    c.horizontal_limit = 9.;
    c.gravity = 0.;
    c.air_resistance = 0.;
    p.scenes[0].entity_mut(BODY).unwrap().transform.position[1] = 5.;
    let mut rt = runtime(&p);
    rt.set_character_velocity(BODY, Vec3::new(12., 2., 0.))
        .unwrap();
    rt.set_movement_intent(BODY, Vec2::X).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    let v = rt.character_state(BODY).unwrap().velocity;
    assert!((v.x - 9.).abs() < 0.001);
    assert_eq!(v.y, 2.);
    let round: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, round);
}

fn sensor(id: &str, position: [f32; 3], size: [f32; 3]) -> Entity {
    let mut area = Entity::new(id, None);
    area.id = id.into();
    area.transform.position = position;
    area.physics3d = Some(Collider3d {
        sensor: true,
        shape: CollisionShape::Box { size },
        filter: CollisionFilter {
            blocks_character: false,
            ..Default::default()
        },
        ..Default::default()
    });
    area
}
fn flying_project() -> Project {
    let mut p = project();
    let body = p.scenes[0].entity_mut(BODY).unwrap();
    body.transform.position[1] = 2.;
    body.character3d.as_mut().unwrap().gravity = 0.;
    p
}
#[test]
fn thin_sensor_crossing_emits_enter_then_exit_and_routes_both_nodes() {
    use oxy_core::graph::{Edge, Node};
    let mut p = flying_project();
    let mut area = sensor("thin", [1., 3., 0.], [0.02, 4., 2.]);
    for (operation, message) in [("event.area_enter", "ENTROU"), ("event.area_exit", "SAIU")] {
        let event = Node::new(operation, [0.; 2]);
        let mut log = Node::new("debug.message", [200., 0.]);
        log.params
            .insert("message".into(), Value::Text(message.into()));
        area.graph.edges.push(Edge {
            from_node: event.id.clone(),
            from_port: "exec".into(),
            to_node: log.id.clone(),
            to_port: "exec".into(),
        });
        area.graph.nodes.extend([event, log]);
    }
    p.scenes[0].entities.push(area);
    let mut rt = runtime(&p);
    rt.set_character_velocity(BODY, Vec3::X * 120.).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!((rt.character_state(BODY).unwrap().position.x - 2.).abs() < 0.001);
    let events = rt.sensor_events();
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(events[0].entered && !events[1].entered);
    assert!(events[0].path_time < events[1].path_time);
    assert_eq!(rt.logs.len(), 2, "{:?}", rt.logs);
    assert!(rt.logs[0].ends_with("ENTROU") && rt.logs[1].ends_with("SAIU"));
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.sensor_events().is_empty());
    rt.scene_mut()
        .entity_mut("thin")
        .unwrap()
        .physics3d
        .as_mut()
        .unwrap()
        .enabled = false;
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.retained_counts(), [0; 4]);
}

#[test]
fn sensor_uses_resolved_slide_path_and_never_the_desired_line_through_wall() {
    let mut p = flying_project();
    let mut wall = Entity::new("Parede", None);
    wall.id = "wall".into();
    wall.transform.position = [0.8, 3., 0.];
    wall.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [0.02, 6., 10.],
        },
        ..Default::default()
    });
    p.scenes[0].entities.extend([
        wall,
        sensor("behind", [1.2, 3., 1.], [0.02, 4., 2.]),
        sensor("along_slide", [0.4, 3., 1.25], [1., 4., 0.02]),
    ]);
    let mut rt = runtime(&p);
    rt.set_character_velocity(BODY, Vec3::new(120., 0., 120.))
        .unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    let s = rt.character_state(BODY).unwrap();
    assert!(s.position.x < 0.5 && s.position.z > 1.5, "{s:?}");
    assert_eq!(rt.sensor_events().len(), 2, "{:?}", rt.sensor_events());
    assert!(rt.sensor_events().iter().all(|e| e.area == "along_slide"));
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn sensor_activation_survives_waits_but_ended_history_and_removed_handles_do_not_accumulate() {
    use oxy_core::graph::{Edge, Node};
    let mut p = flying_project();
    p.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .attributes
        .insert("Vida".into(), Value::Number(100.));
    let mut area = sensor("damage_area", [1., 3., 0.], [0.02, 4., 2.]);
    let event = Node::new("event.area_enter", [0.; 2]);
    let mut damage = Node::new("action.damage", [200., 0.]);
    damage
        .params
        .insert("attribute".into(), Value::Text("Vida".into()));
    damage.params.insert("amount".into(), Value::Number(1.));
    let mut wait = Node::new("control.wait", [400., 0.]);
    wait.params.insert("seconds".into(), Value::Number(0.1));
    let mut again = damage.clone();
    again.id = new_id();
    for (from, output, to, input) in [
        (&event, "exec", &damage, "exec"),
        (&damage, "exec", &wait, "exec"),
        (&wait, "exec", &again, "exec"),
        (&event, "context", &damage, "target"),
        (&event, "context", &again, "target"),
    ] {
        area.graph.edges.push(Edge {
            from_node: from.id.clone(),
            from_port: output.into(),
            to_node: to.id.clone(),
            to_port: input.into(),
        });
    }
    area.graph.nodes = vec![event, damage, wait, again];
    p.scenes[0].entities.push(area);
    let mut rt = runtime(&p);
    rt.set_character_velocity(BODY, Vec3::X * 120.).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.set_character_velocity(BODY, -Vec3::X * 120.).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.set_character_velocity(BODY, Vec3::ZERO).unwrap();
    assert_eq!(
        rt.scene().entity(BODY).unwrap().attributes["Vida"],
        Value::Number(99.)
    );
    rt.scene_mut()
        .entity_mut("damage_area")
        .unwrap()
        .physics3d
        .as_mut()
        .unwrap()
        .enabled = false;
    for _ in 0..12 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        rt.scene().entity(BODY).unwrap().attributes["Vida"],
        Value::Number(99.)
    );
    assert_eq!(rt.retained_counts(), [0; 4]);
    rt.scene_mut()
        .entity_mut("damage_area")
        .unwrap()
        .physics3d
        .as_mut()
        .unwrap()
        .enabled = true;
    rt.set_character_velocity(BODY, Vec3::X * 120.).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        rt.scene().entity(BODY).unwrap().attributes["Vida"],
        Value::Number(98.)
    );
    rt.remove_object(BODY);
    assert!(rt.character_state(BODY).is_none());
    assert!(rt.physics_world().unwrap().ids().all(|id| id != BODY));
    rt.remove_object("damage_area");
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.retained_counts(), [0; 4]);
    // Removing the followed character now preserves the last safe camera view
    // and reports the missing target once; physics/task diagnostics stay empty.
    assert_eq!(rt.logs.len(), 1, "{:?}", rt.logs);
    assert!(rt.logs[0].starts_with("Câmera "), "{:?}", rt.logs);
}

#[test]
fn exit_node_also_works_for_legacy_2d_without_creating_3d_world() {
    use oxy_core::graph::{Edge, Node};
    let mut p = Project::new("Área 2D");
    let mut area = Entity::new("Área", None);
    area.id = "area".into();
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let event = Node::new("event.area_exit", [0.; 2]);
    let mut message = Node::new("debug.message", [200., 0.]);
    message
        .params
        .insert("message".into(), Value::Text("Saiu no 2D".into()));
    area.graph.edges.push(Edge {
        from_node: event.id.clone(),
        from_port: "exec".into(),
        to_node: message.id.clone(),
        to_port: "exec".into(),
    });
    area.graph.nodes = vec![event, message];
    let mut body = Entity::new("Objeto", None);
    body.id = BODY.into();
    body.collider = Some(Collider::default());
    p.scenes[0].entities = vec![area, body];
    let mut rt = runtime(&p);
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.scene_mut().entity_mut(BODY).unwrap().transform.position[0] = 10.;
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.logs.len(), 1);
    assert!(rt.logs[0].ends_with("Saiu no 2D"));
    assert!(rt.physics_world().is_none());
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.logs.len(), 1);
}

#[test]
fn disabling_movement_does_not_hide_an_active_collider_from_sensors() {
    let mut p = flying_project();
    p.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .enabled = false;
    p.scenes[0]
        .entities
        .push(sensor("occupied", [0., 3., 0.], [2., 4., 2.]));
    let mut rt = runtime(&p);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.sensor_events().len(), 1);
    assert!(rt.sensor_events()[0].entered);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.sensor_events().is_empty());
    rt.scene_mut()
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .body
        .enabled = false;
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.sensor_events().is_empty());
}
