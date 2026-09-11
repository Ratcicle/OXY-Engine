use glam::{Quat, Vec2, Vec3};
use oxy_core::{
    character::*,
    document::*,
    physics3d::*,
    runtime::{FIXED_DT, InputFrame, Runtime},
};

fn fixture() -> Project {
    let mut project = Project::new("Contrato de personagem 3D");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.id = "00000000-0000-4000-8000-000000000001".into();
    floor.dimensions = [100., 1., 100.];
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    let mut body = Entity::new("Personagem", None);
    body.id = "00000000-0000-4000-8000-000000000002".into();
    // This fixture isolates input/solver sequencing. Acceleration profiles have
    // separate tests; one tick reaches the requested speed in this fixture.
    body.character3d = Some(CharacterConfig {
        ground_acceleration: 600.,
        ..Default::default()
    });
    body.physics3d = Some(Collider3d {
        shape: CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        center: [0., 0.9, 0.],
        ..Default::default()
    });
    body.transform.position[1] = 0.02;
    let mut camera = Entity::new("Olhos", None);
    camera.id = "00000000-0000-4000-8000-000000000003".into();
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(body.id.clone()),
        ..Default::default()
    });
    // A camera's transform/parent must not be multiplied into an absolute target twice.
    camera.parent = Some(body.id.clone());
    camera.transform.position = [50., 50., 50.];
    scene.entities = vec![floor, body, camera];
    project
}
const BODY: &str = "00000000-0000-4000-8000-000000000002";
fn runtime(project: &Project) -> Runtime {
    Runtime::new(project, &project.start_scene).unwrap()
}
fn settle(rt: &mut Runtime) {
    for _ in 0..30 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
}

#[test]
fn analog_and_diagonal_intent_preserve_direction_and_intensity() {
    let actions = Default::default();
    let small = movement_axis(
        &InputFrame {
            movement: [0.2, 0.],
            ..Default::default()
        },
        &actions,
    );
    assert_eq!(small, Vec2::new(0.2, 0.));
    let diagonal = movement_axis(
        &InputFrame {
            movement: [1., 1.],
            ..Default::default()
        },
        &actions,
    );
    assert!((diagonal.length() - 1.).abs() < 1e-6);
    assert!(wish_direction(Vec2::Y, std::f32::consts::FRAC_PI_2).abs_diff_eq(-Vec3::X, 1e-6));
}

#[test]
fn shared_runtime_moves_capsule_jumps_and_preserves_authored_project() {
    let project = fixture();
    let before = serde_json::to_value(&project).unwrap();
    let mut rt = runtime(&project);
    settle(&mut rt);
    assert!(rt.character_state(BODY).unwrap().grounded);
    let start = rt.character_state(BODY).unwrap().position;
    let input = InputFrame {
        held: ["mover_frente".into()].into(),
        ..Default::default()
    };
    for _ in 0..60 {
        rt.advance(FIXED_DT, &input);
    }
    let state = rt.character_state(BODY).unwrap();
    assert!((state.position.z - start.z + 6.).abs() < 0.01, "{state:?}");
    assert!(state.position.y.abs() < 0.025, "{state:?}");
    rt.advance(
        FIXED_DT,
        &InputFrame {
            pressed: ["pular".into()].into(),
            ..Default::default()
        },
    );
    assert!(
        rt.character_state(BODY).unwrap().velocity.y > 7.
            && !rt.character_state(BODY).unwrap().grounded
    );
    rt.stop();
    assert_eq!(before, serde_json::to_value(&project).unwrap());
}

#[test]
fn camera_relative_movement_and_eyes_ignore_camera_parent_transform() {
    let project = fixture();
    let mut rt = runtime(&project);
    settle(&mut rt);
    // 750 raw units * 0.12 degrees = 90 degrees to the right.
    rt.advance(
        FIXED_DT,
        &InputFrame {
            look: [750., 0.],
            held: ["mover_frente".into()].into(),
            ..Default::default()
        },
    );
    let state = rt.character_state(BODY).unwrap();
    assert!(state.position.x > 0.09 && state.position.z.abs() < 1e-5);
    let pose = rt.game_camera_pose().unwrap();
    assert!(pose.position.x < 1. && (pose.position.y - 1.61).abs() < 0.04);
    assert!((pose.rotation * -Vec3::Z).abs_diff_eq(Vec3::X, 1e-5));
}

#[test]
fn relative_look_survives_zero_steps_is_consumed_once_and_pause_clears_it() {
    let project = fixture();
    let mut rt = runtime(&project);
    settle(&mut rt);
    rt.advance(
        FIXED_DT * 0.25,
        &InputFrame {
            look: [100., 30.],
            ..Default::default()
        },
    );
    let preview = rt.game_camera_pose().unwrap().rotation;
    let expected =
        Quat::from_rotation_y(-12f32.to_radians()) * Quat::from_rotation_x(-3.6f32.to_radians());
    assert!((preview * -Vec3::Z).abs_diff_eq(expected * -Vec3::Z, 1e-5));
    rt.advance(FIXED_DT * 2., &InputFrame::default());
    assert!(preview.abs_diff_eq(rt.game_camera_pose().unwrap().rotation, 1e-5));
    rt.advance(
        0.,
        &InputFrame {
            look: [1000., 0.],
            ..Default::default()
        },
    );
    rt.set_paused(true);
    rt.set_paused(false);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(preview.abs_diff_eq(rt.game_camera_pose().unwrap().rotation, 1e-5));
}

#[test]
fn new_controller_never_runs_legacy_solver_and_invalid_setup_is_rejected() {
    let mut project = fixture();
    project.scenes[0].entity_mut(BODY).unwrap().controller = Some(Controller::default());
    assert!(Runtime::new(&project, &project.start_scene).is_err());
    project.scenes[0].entity_mut(BODY).unwrap().controller = None;
    project.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .physics3d
        .as_mut()
        .unwrap()
        .sensor = true;
    assert!(Runtime::new(&project, &project.start_scene).is_err());
    project.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .physics3d
        .as_mut()
        .unwrap()
        .sensor = false;
    project.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .transform
        .rotation[0] = 0.1;
    assert!(Runtime::new(&project, &project.start_scene).is_err());
}

#[test]
fn thin_wall_uses_the_runtime_capsule() {
    let mut project = fixture();
    let scene = &mut project.scenes[0];
    let mut wall = scene.entities[0].clone();
    wall.id = new_id();
    wall.transform.position = [0., 1., -2.];
    wall.physics3d.as_mut().unwrap().shape = CollisionShape::Box {
        size: [100., 4., 0.02],
    };
    scene.entities.push(wall);
    let mut rt = runtime(&project);
    settle(&mut rt);
    for _ in 0..90 {
        rt.advance(
            FIXED_DT,
            &InputFrame {
                held: ["mover_frente".into()].into(),
                ..Default::default()
            },
        );
    }
    let p = rt.character_state(BODY).unwrap().position;
    assert!(p.z > -1.71 && p.z < -1.6, "{p:?}");
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn sequential_characters_see_the_previous_characters_new_position() {
    let mut project = fixture();
    let mut next = project.scenes[0].entity(BODY).unwrap().clone();
    next.id = "00000000-0000-4000-8000-000000000004".into();
    next.transform.position[0] = 0.65;
    next.character3d.as_mut().unwrap().automatic_input = false;
    let next_id = next.id.clone();
    project.scenes[0].entities.push(next);
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.character_state(&next_id).unwrap().position.x;
    rt.set_movement_intent(&next_id, -Vec2::X).unwrap();
    rt.advance(
        FIXED_DT,
        &InputFrame {
            held: ["mover_esquerda".into()].into(),
            ..Default::default()
        },
    );
    let after = rt.character_state(&next_id).unwrap().position.x;
    assert!(
        before - after > 0.08,
        "A stale first body would stop the second near x=0.62: {before} -> {after}"
    );
}

#[test]
fn timestamped_input_matches_at_30_60_144_and_240_hz() {
    use oxy_core::input_timeline::{InputChange, TimedInput};
    let project = fixture();
    let mut reference: Option<(Vec3, Vec3, Quat)> = None;
    for hz in [30, 60, 144, 240] {
        let mut rt = runtime(&project);
        for (at, change) in [
            (0.09, InputChange::Press("mover_frente".into())),
            (0.317, InputChange::Look([233., -50.])),
            (0.553, InputChange::Press("pular".into())),
            (0.56, InputChange::Release("pular".into())),
            (0.895, InputChange::Look([-34., 18.])),
            (1.117, InputChange::Release("mover_frente".into())),
        ] {
            rt.queue_timed_input(TimedInput { at, change }).unwrap();
        }
        for _ in 0..(hz * 2) {
            rt.advance(1. / hz as f32, &InputFrame::default());
        }
        assert!((rt.time - 2.).abs() < 1e-5, "{hz}: {}", rt.time);
        let state = rt.character_state(BODY).unwrap();
        let result = (
            state.position,
            state.velocity,
            rt.game_camera_pose().unwrap().rotation,
        );
        if let Some(expected) = reference {
            assert!(
                result.0.abs_diff_eq(expected.0, 1e-5) && result.1.abs_diff_eq(expected.1, 1e-5)
            );
            assert!((result.2 * -Vec3::Z).abs_diff_eq(expected.2 * -Vec3::Z, 1e-5));
        } else {
            reference = Some(result);
        }
    }
}

#[test]
fn rig_ids_survive_model_duplication_and_deletion_does_not_break_project() {
    let mut project = fixture();
    let scene = &mut project.scenes[0];
    let root = scene.duplicate_subtree(BODY).unwrap();
    let rig = scene
        .entities
        .iter()
        .find(|e| e.parent.as_deref() == Some(&root) && e.camera_rig.is_some())
        .unwrap()
        .camera_rig
        .as_ref()
        .unwrap();
    assert_eq!(rig.target.as_deref(), Some(root.as_str()));
    let camera = scene
        .entities
        .iter_mut()
        .find(|e| e.id == "00000000-0000-4000-8000-000000000003")
        .unwrap();
    camera.parent = None;
    scene.remove_subtree(BODY);
    assert!(
        scene
            .entity("00000000-0000-4000-8000-000000000003")
            .unwrap()
            .camera_rig
            .as_ref()
            .unwrap()
            .target
            .is_none()
    );
    validate_project(&project).unwrap();
    let json = serde_json::to_string(&project).unwrap();
    assert_eq!(project, serde_json::from_str(&json).unwrap());
}

#[test]
fn acceleration_braking_and_external_momentum_are_distinct() {
    let mut project = fixture();
    project.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .ground_acceleration = 40.;
    let mut rt = runtime(&project);
    settle(&mut rt);
    rt.advance(
        FIXED_DT,
        &InputFrame {
            movement: [1., 0.],
            ..Default::default()
        },
    );
    assert!((rt.character_state(BODY).unwrap().velocity.x - 40. * FIXED_DT).abs() < 0.001);
    for _ in 0..60 {
        rt.advance(
            FIXED_DT,
            &InputFrame {
                movement: [1., 0.],
                ..Default::default()
            },
        );
    }
    assert!((rt.character_state(BODY).unwrap().velocity.x - 6.).abs() < 0.001);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.character_state(BODY).unwrap().velocity.x > 4.9);
    rt.add_character_velocity(BODY, Vec3::new(20., 5., 0.))
        .unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.character_state(BODY).unwrap().velocity.x > 24.);
    assert!(
        rt.character_state(BODY).unwrap().velocity.y > 4.,
        "{:?}",
        rt.character_state(BODY)
    );
}

#[test]
fn independent_input_blocks_do_not_freeze_gravity_or_inertia() {
    let project = fixture();
    let mut rt = runtime(&project);
    settle(&mut rt);
    rt.add_character_velocity(BODY, Vec3::new(12., 5., 0.))
        .unwrap();
    rt.block_character_input(BODY, false, "menu", true).unwrap();
    rt.block_character_input(BODY, false, "atordoado", true)
        .unwrap();
    rt.block_character_input(BODY, false, "menu", false)
        .unwrap();
    rt.block_character_input(BODY, true, "menu", true).unwrap();
    let yaw = rt.character_state(BODY).unwrap().yaw;
    rt.advance(
        FIXED_DT,
        &InputFrame {
            movement: [-1., 0.],
            look: [700., 0.],
            pressed: ["pular".into()].into(),
            ..Default::default()
        },
    );
    let state = rt.character_state(BODY).unwrap();
    assert_eq!(state.movement_blocks.len(), 1);
    assert_eq!(state.yaw, yaw);
    assert!((state.velocity.x - 12.).abs() < 1e-5 && state.position.x > 0.19);
    assert!(state.velocity.y < 5. && state.velocity.y > 4.);
}

#[test]
fn crouching_preserves_feet_and_requires_entire_standing_capsule_free() {
    let project = fixture();
    let mut rt = runtime(&project);
    settle(&mut rt);
    let feet = rt.character_state(BODY).unwrap().position;
    rt.request_crouch(BODY, true).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    let state = rt.character_state(BODY).unwrap();
    assert_eq!(state.posture, Posture::Crouched);
    assert!(state.position.abs_diff_eq(feet, 0.002));
    let mut ceiling = Entity::new("Teto", None);
    ceiling.id = "ceiling".into();
    ceiling.transform.position = [0., 1.4, 0.];
    ceiling.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [4., 0.2, 4.],
        },
        ..Default::default()
    });
    rt.scene_mut().entities.push(ceiling);
    rt.request_crouch(BODY, false).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Crouched);
    rt.scene_mut().remove_subtree("ceiling");
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Standing);
    // The actual runtime query body agrees with posture; authored collider remains standing.
    let body = rt
        .physics_world()
        .unwrap()
        .debug_shapes()
        .into_iter()
        .find(|b| b.id == BODY)
        .unwrap();
    assert!((body.position.y - feet.y - 0.9).abs() < 0.002);
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn surfaces_preserve_reference_integrity_and_change_ground_response() {
    use oxy_core::surface::*;
    let mut project = fixture();
    let ice = SurfaceMaterial::preset(SurfacePreset::Ice);
    let id = ice.id.clone();
    project.surfaces.push(ice);
    project.scenes[0].entities[0]
        .physics3d
        .as_mut()
        .unwrap()
        .surface = Some(id.clone());
    assert!(remove(&mut project, &id).is_err());
    let copy = duplicate(&mut project, &id).unwrap();
    assert_ne!(copy, id);
    remove(&mut project, &copy).unwrap();
    let mut rt = runtime(&project);
    settle(&mut rt);
    rt.set_character_velocity(BODY, Vec3::X * 6.).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.character_state(BODY).unwrap().velocity.x > 5.9);
    let json = serde_json::to_string(&project).unwrap();
    assert_eq!(project, serde_json::from_str(&json).unwrap());
}

#[test]
fn platform_carry_is_once_and_jump_inheritance_is_configurable() {
    use oxy_core::surface::*;
    for policy in [
        InheritPlatform::None,
        InheritPlatform::Horizontal,
        InheritPlatform::All,
    ] {
        let mut project = fixture();
        project.scenes[0].entities[0].platform = Some(TranslationPlatform {
            mode: PlatformMode::Velocity,
            velocity: [2., 1., 0.],
            ..Default::default()
        });
        project.scenes[0]
            .entity_mut(BODY)
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .inherit_platform = policy;
        let mut rt = runtime(&project);
        settle(&mut rt);
        assert!(
            rt.character_state(BODY).unwrap().grounded,
            "{:?} {:?}",
            rt.character_state(BODY),
            rt.logs
        );
        let before = rt.character_state(BODY).unwrap().position;
        for _ in 0..30 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        let state = rt.character_state(BODY).unwrap();
        assert!(
            state
                .position
                .abs_diff_eq(before + Vec3::new(1., 0.5, 0.), 0.01),
            "before={before:?} {state:?}"
        );
        assert!(state.velocity.length() < 0.01, "{state:?}");
        assert!(
            state
                .total_velocity()
                .abs_diff_eq(Vec3::new(2., 1., 0.), 0.002)
        );
        rt.request_jump(BODY).unwrap();
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(
            (state.velocity.x
                - if policy == InheritPlatform::None {
                    0.
                } else {
                    2.
                })
            .abs()
                < 0.003,
            "{state:?}"
        );
        assert!(
            (state.velocity.y
                - (8.
                    + if policy == InheritPlatform::All {
                        1.
                    } else {
                        0.
                    }
                    - 22. * FIXED_DT))
                .abs()
                < 0.003,
            "{state:?}"
        );
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    }
}

#[test]
fn conveyor_is_tangential_and_pause_clears_requested_posture() {
    use oxy_core::surface::*;
    let mut project = fixture();
    let surface = SurfaceMaterial::preset(SurfacePreset::Conveyor);
    project.scenes[0].entities[0]
        .physics3d
        .as_mut()
        .unwrap()
        .surface = Some(surface.id.clone());
    project.surfaces.push(surface);
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.character_state(BODY).unwrap().position;
    for _ in 0..30 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert!(
        (rt.character_state(BODY).unwrap().position.x - before.x - 1.).abs() < 0.003,
        "{before:?} {:?}",
        rt.character_state(BODY)
    );
    rt.request_crouch(BODY, true).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.set_paused(true);
    rt.set_paused(false);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Standing);
}

#[test]
fn landing_buffer_emits_once_without_integrating_an_extra_tick() {
    for buffer in [0., 120.] {
        let mut project = fixture();
        let body = project.scenes[0].entity_mut(BODY).unwrap();
        body.transform.position[1] = 0.15;
        body.character3d.as_mut().unwrap().jump_buffer_ms = buffer;
        let mut rt = runtime(&project);
        rt.set_character_velocity(BODY, -Vec3::Y * 3.).unwrap();
        rt.request_jump(BODY).unwrap();
        let mut jumped = 0;
        let mut landed = 0;
        for _ in 0..8 {
            rt.advance(FIXED_DT, &InputFrame::default());
            for (_, event) in rt.movement_events() {
                match event {
                    MovementEvent::Jumped => jumped += 1,
                    MovementEvent::Landed { impact_speed } => {
                        landed += 1;
                        assert!(
                            *impact_speed > 3.,
                            "impact={impact_speed} {:?}",
                            rt.character_state(BODY)
                        );
                    }
                    _ => {}
                }
            }
            if jumped > 0 {
                let s = rt.character_state(BODY).unwrap();
                assert!(s.position.y < 0.03, "{s:?}");
                assert_eq!(s.velocity.y, 8.);
                break;
            }
        }
        assert_eq!(landed, 1);
        assert_eq!(jumped, if buffer > 0. { 1 } else { 0 });
    }
}

#[test]
fn coyote_time_zero_disables_delayed_edge_jump() {
    for grace in [0., 100.] {
        let mut project = fixture();
        project.scenes[0]
            .entity_mut(BODY)
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .coyote_ms = grace;
        let mut rt = runtime(&project);
        settle(&mut rt);
        let floor = rt.scene().entities[0].id.clone();
        rt.scene_mut().remove_subtree(&floor);
        for _ in 0..3 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        rt.request_jump(BODY).unwrap();
        rt.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(
            rt.character_state(BODY).unwrap().velocity.y > 0.,
            grace > 0.
        );
        if grace > 0. {
            rt.request_jump(BODY).unwrap();
            rt.advance(FIXED_DT, &InputFrame::default());
            assert!(
                rt.movement_events()
                    .iter()
                    .all(|(_, e)| !matches!(e, MovementEvent::Jumped))
            );
        }
    }
}

#[test]
fn platform_discontinuity_detaches_without_launch_and_diagnostics_are_bounded() {
    use oxy_core::surface::*;
    let mut project = fixture();
    project.scenes[0].entities[0].platform = Some(TranslationPlatform::default());
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.character_state(BODY).unwrap().position;
    rt.scene_mut().entities[0].transform.position[0] = 5.;
    rt.advance(FIXED_DT, &InputFrame::default());
    let s = rt.character_state(BODY).unwrap();
    assert!(
        !s.grounded && s.velocity.x.abs() < 0.001 && (s.position.x - before.x).abs() < 0.01,
        "{s:?}"
    );
    assert!(!rt.logs.is_empty());
    for _ in 0..10 {
        rt.scene_mut().entities[0].transform.rotation[1] += 0.2;
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(rt.logs.len(), 1);
    assert!(rt.character_state(BODY).unwrap().velocity.length() < 10.);
}

#[test]
fn elevator_cannot_push_character_through_a_ceiling() {
    use oxy_core::surface::*;
    let mut project = fixture();
    project.scenes[0].entities[0].platform = Some(TranslationPlatform {
        mode: PlatformMode::Velocity,
        velocity: [0., 1., 0.],
        ..Default::default()
    });
    let mut ceiling = Entity::new("Teto sólido", None);
    ceiling.id = "ceiling".into();
    ceiling.transform.position = [0., 3., 0.];
    ceiling.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [50., 1., 50.],
        },
        ..Default::default()
    });
    project.scenes[0].entities.push(ceiling);
    let mut rt = runtime(&project);
    for _ in 0..150 {
        rt.advance(FIXED_DT, &InputFrame::default());
        assert!(
            rt.character_state(BODY).unwrap().position.y < 0.72,
            "{:?}",
            rt.character_state(BODY)
        );
    }
    assert!(
        !rt.logs.is_empty(),
        "A capsule trapped between surfaces must report bounded recovery"
    );
}

#[test]
fn moving_obstacle_recovers_an_idle_character_with_finite_bounded_motion() {
    use oxy_core::surface::*;
    let project = fixture();
    let mut rt = runtime(&project);
    settle(&mut rt);
    let mut wall = Entity::new("Empurrador", None);
    wall.id = "pusher".into();
    wall.transform.position = [-0.6, 1., 0.];
    wall.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [0.5, 2., 3.],
        },
        ..Default::default()
    });
    wall.platform = Some(TranslationPlatform {
        mode: PlatformMode::Velocity,
        velocity: [1., 0., 0.],
        ..Default::default()
    });
    rt.scene_mut().entities.push(wall);
    for _ in 0..40 {
        let before = rt.character_state(BODY).unwrap().position;
        rt.advance(FIXED_DT, &InputFrame::default());
        let after = rt.character_state(BODY).unwrap().position;
        assert!(after.distance(before) < 0.1, "{before:?} -> {after:?}");
    }
    assert!(
        rt.character_state(BODY).unwrap().position.x > 0.5,
        "{:?} {:?}",
        rt.character_state(BODY),
        rt.logs
    );
}

#[test]
fn walkable_slopes_project_intent_without_energy_gain_and_steep_slopes_slide() {
    for degrees in [25_f32, 60.] {
        let mut project = fixture();
        let floor = &mut project.scenes[0].entities[0];
        floor.physics3d.as_mut().unwrap().shape = CollisionShape::Box {
            size: [50., 1., 50.],
        };
        floor.transform.rotation[2] = degrees.to_radians();
        project.scenes[0]
            .entity_mut(BODY)
            .unwrap()
            .transform
            .position = [0., 3., 0.];
        let mut rt = runtime(&project);
        for _ in 0..90 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        if degrees < 45. {
            assert!(
                rt.character_state(BODY).unwrap().grounded,
                "{:?}",
                rt.character_state(BODY)
            );
            let start = rt.character_state(BODY).unwrap().position;
            for _ in 0..30 {
                rt.advance(
                    FIXED_DT,
                    &InputFrame {
                        movement: [1., 0.],
                        ..Default::default()
                    },
                );
                let s = rt.character_state(BODY).unwrap();
                assert!(s.velocity.length() < 6.05, "{s:?}");
            }
            assert!(rt.character_state(BODY).unwrap().position.y > start.y + 0.5);
        } else {
            let s = rt.character_state(BODY).unwrap();
            assert!(!s.grounded && s.position.x < -1., "{s:?}");
        }
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    }
}

#[test]
fn surfaces_capsules_and_platforms_undo_redo_without_image_snapshots() {
    use oxy_core::{history::CommandHistory, surface::*, texture_cache::TextureCache};
    let mut project = fixture();
    let base = project.clone();
    let mut textures = TextureCache::default();
    let mut history = CommandHistory::new();
    history.begin("Material e transporte", &project, &textures);
    let surface = SurfaceMaterial::preset(SurfacePreset::Ice);
    project.scenes[0].entities[0]
        .physics3d
        .as_mut()
        .unwrap()
        .surface = Some(surface.id.clone());
    project.surfaces.push(surface);
    project.scenes[0].entities[0].platform = Some(TranslationPlatform::default());
    history.commit(&project, &mut textures).unwrap();
    let changed = project.clone();
    history.undo(&mut project, &mut textures).unwrap();
    assert_eq!(project, base);
    history.redo(&mut project, &mut textures).unwrap();
    assert_eq!(project, changed);
    validate_project(&project).unwrap();
}

#[test]
fn changing_character_ancestor_does_not_steal_physical_authority_or_move_visual_children_twice() {
    let mut project = fixture();
    let mut parent = Entity::new("Montagem", None);
    parent.id = "assembly".into();
    parent.transform.scale = [2.; 3];
    project.scenes[0].entity_mut(BODY).unwrap().parent = Some(parent.id.clone());
    project.scenes[0].entities.push(parent);
    let mut part = Entity::new("Peça visual", Some(Primitive::Cube));
    part.id = "part".into();
    part.parent = Some(BODY.into());
    part.transform.position = [0., 0.5, 0.];
    project.scenes[0].entities.push(part);
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.scene().world_matrix("part").unwrap();
    let feet = rt.character_state(BODY).unwrap().position;
    rt.scene_mut()
        .entity_mut("assembly")
        .unwrap()
        .transform
        .position = [3., 7., -4.];
    rt.scene_mut()
        .entity_mut("assembly")
        .unwrap()
        .transform
        .rotation = [0.2, 0.6, 0.1];
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(
        rt.scene()
            .world_matrix("part")
            .unwrap()
            .abs_diff_eq(before, 0.002)
    );
    assert!(
        rt.character_state(BODY)
            .unwrap()
            .position
            .abs_diff_eq(feet, 0.002)
    );
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn step_height_and_headroom_are_both_required() {
    for (step_height, tunnel, passes) in [
        (0.15, false, true),
        (0.65, false, false),
        (0.15, true, false),
    ] {
        let mut project = fixture();
        let mut step = Entity::new("Degrau", None);
        step.id = "step".into();
        step.transform.position = [0., step_height * 0.5, -2.];
        step.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: [8., step_height, 2.],
            },
            ..Default::default()
        });
        project.scenes[0].entities.push(step);
        if tunnel {
            let mut roof = Entity::new("Teto baixo", None);
            roof.id = "roof".into();
            roof.transform.position = [0., 2., -2.];
            roof.physics3d = Some(Collider3d {
                shape: CollisionShape::Box {
                    size: [8., 0.2, 2.],
                },
                ..Default::default()
            });
            project.scenes[0].entities.push(roof);
        }
        let mut rt = runtime(&project);
        settle(&mut rt);
        for _ in 0..60 {
            rt.advance(
                FIXED_DT,
                &InputFrame {
                    movement: [0., 1.],
                    ..Default::default()
                },
            );
        }
        let state = rt.character_state(BODY).unwrap();
        assert_eq!(
            state.position.z < -3.2,
            passes,
            "height={step_height}, tunnel={tunnel}, {state:?}"
        );
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    }
}
