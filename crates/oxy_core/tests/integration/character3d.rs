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
        .character3d
        .as_mut()
        .unwrap()
        .body
        .enabled = false;
    assert!(Runtime::new(&project, &project.start_scene).is_err());
    project.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .body
        .enabled = true;
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
fn landing_on_a_matching_horizontal_platform_preserves_world_velocity() {
    use oxy_core::surface::*;
    for (platform_speed, air_speed) in [(2., 2.), (-3., -3.), (2., 0.), (2., 5.)] {
        let mut project = fixture();
        project.scenes[0].entities[0].platform = Some(TranslationPlatform {
            mode: PlatformMode::Velocity,
            velocity: [platform_speed, 0., 0.],
            ..Default::default()
        });
        let body = project.scenes[0].entity_mut(BODY).unwrap();
        body.transform.position[1] = 2.;
        let config = body.character3d.as_mut().unwrap();
        config.automatic_input = false;
        config.ground_friction = 0.;
        config.ground_braking = 0.;
        config.air_resistance = 0.;
        config.landing_retention = 1.;
        let mut rt = runtime(&project);
        rt.advance(FIXED_DT, &InputFrame::default());
        rt.set_character_velocity(BODY, Vec3::new(air_speed, -1., 0.))
            .unwrap();
        let mut landed = false;
        for _ in 0..120 {
            rt.advance(FIXED_DT, &InputFrame::default());
            let state = rt.character_state(BODY).unwrap();
            if state.grounded {
                assert!(
                    (state.total_velocity().x - air_speed).abs() < 0.002,
                    "acquiring support duplicated velocity: {state:?}"
                );
                assert!(
                    (state.velocity.x - (air_speed - platform_speed)).abs() < 0.002,
                    "velocity must now be relative to support: {state:?}"
                );
                let position = state.position;
                for _ in 0..30 {
                    rt.advance(FIXED_DT, &InputFrame::default());
                }
                let state = rt.character_state(BODY).unwrap();
                assert!(
                    (state.position.x - position.x - air_speed * 0.5).abs() < 0.005,
                    "{state:?}"
                );
                landed = true;
                break;
            }
        }
        assert!(landed);
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
    // The character it dropped reports it, naming the platform.
    assert_eq!(
        rt.logs.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        [format!(
            "Personagem {BODY}: Apoio em Chão liberado sem herdar velocidade, porque a \
             plataforma girou rápido demais, mudou de escala ou saltou de posição."
        )]
    );
    // Turning faster than a platform carries (20° a step) keeps dropping it, unreported.
    for _ in 0..10 {
        rt.scene_mut().entities[0].transform.rotation[1] += 0.5;
        rt.advance(FIXED_DT, &InputFrame::default());
        assert!(!rt.character_state(BODY).unwrap().grounded);
    }
    assert_eq!(rt.logs.len(), 1);
    assert!(rt.character_state(BODY).unwrap().velocity.length() < 10.);
}

#[test]
fn a_platform_that_turns_with_nobody_on_it_reports_nothing() {
    use oxy_core::surface::*;
    let mut project = fixture();
    let mut wheel = Entity::new("Roda", None);
    wheel.id = "wheel".into();
    wheel.transform.position = [10., 0.5, 0.];
    wheel.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [2., 0.2, 2.],
        },
        ..Default::default()
    });
    wheel.platform = Some(TranslationPlatform::default());
    project.scenes[0].entities.push(wheel);
    // "reiniciar" restarts the scene: each start is checked too.
    let mut restart = oxy_core::graph::Node::new("event.input", [0.; 2]);
    restart
        .params
        .insert("action".into(), Value::Text("reiniciar".into()));
    let mut change = oxy_core::graph::Node::new("action.scene", [0.; 2]);
    change
        .params
        .insert("scene".into(), Value::Text(project.start_scene.clone()));
    let body = project.scenes[0].entity_mut(BODY).unwrap();
    body.graph.edges.push(oxy_core::graph::Edge {
        from_node: restart.id.clone(),
        from_port: "exec".into(),
        to_node: change.id.clone(),
        to_port: "exec".into(),
    });
    body.graph.nodes = vec![restart, change];
    let mut rt = runtime(&project);
    for round in 0..2 {
        settle(&mut rt);
        // Too fast to carry anyone, a jump each step.
        for _ in 0..10 {
            let wheel = rt.scene_mut().entity_mut("wheel").unwrap();
            wheel.transform.rotation[1] += 0.5;
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        assert!(rt.logs.is_empty(), "round {round}: {:?}", rt.logs);
        let press = InputFrame {
            pressed: ["reiniciar".to_owned()].into(),
            ..Default::default()
        };
        rt.advance(FIXED_DT, &press);
    }
    assert_eq!(rt.scene_starts.len(), 2);
}

#[test]
fn a_character_on_the_seam_of_turning_planks_is_warned_once_per_platform() {
    use oxy_core::surface::*;
    let warning = |platform: &str| {
        format!(
            "Personagem {BODY}: Apoio em {platform} liberado sem herdar velocidade, porque a \
             plataforma girou rápido demais, mudou de escala ou saltou de posição."
        )
    };
    // Two 2 m planks side by side, either the children of one platform ("Ponte") or each a
    // platform of its own, turning too fast to carry anyone (over 20° a step). On the seam,
    // the support probe hits one plank or the other from step to step.
    for grouped in [true, false] {
        for (x, walking) in [(0., false), (0.05, false), (-0.6, true)] {
            let mut project = fixture();
            let scene = &mut project.scenes[0];
            let mut bridge = Entity::new("Ponte", None);
            bridge.id = "bridge".into();
            bridge.transform.position = [0., 0.4, 0.];
            bridge.platform = Some(TranslationPlatform::default());
            if grouped {
                scene.entities.push(bridge);
            }
            for (id, name, side) in [("plank_a", "Tábua A", -1.), ("plank_b", "Tábua B", 1.)] {
                let mut plank = Entity::new(name, None);
                plank.id = id.into();
                plank.physics3d = Some(Collider3d {
                    shape: CollisionShape::Box {
                        size: [2., 0.2, 2.],
                    },
                    ..Default::default()
                });
                if grouped {
                    plank.parent = Some("bridge".into());
                    plank.transform.position = [side, 0., 0.];
                } else {
                    plank.transform.position = [side, 0.4, 0.];
                    plank.platform = Some(TranslationPlatform::default());
                }
                scene.entities.push(plank);
            }
            let body = scene.entity_mut(BODY).unwrap();
            body.transform.position = [x, 0.52, 0.];
            // Slowly, so that walking crosses the seam without leaving the planks.
            body.character3d.as_mut().unwrap().horizontal_limit = 0.5;
            let mut rt = runtime(&project);
            settle(&mut rt);
            assert!(rt.character_state(BODY).unwrap().grounded);
            let turning: &[&str] = if grouped {
                &["bridge"]
            } else {
                &["plank_a", "plank_b"]
            };
            for _ in 0..120 {
                for id in turning {
                    rt.scene_mut().entity_mut(id).unwrap().transform.rotation[1] += 0.4;
                }
                let mut input = InputFrame::default();
                if walking {
                    input.held.insert("mover_direita".into());
                }
                rt.advance(FIXED_DT, &input);
            }
            let case = format!("grouped {grouped}, x {x}, walking {walking}");
            let state = rt.character_state(BODY).unwrap();
            assert!(
                !state.grounded && state.position.y > 0.4,
                "{case}: {state:?}"
            );
            if walking {
                assert!(
                    state.position.x > 0.2,
                    "{case}: crosses the seam, {state:?}"
                );
            }
            // Once per platform that dropped the character: the group, named as such, or
            // each plank.
            if grouped {
                assert_eq!(
                    rt.logs.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
                    [warning("Ponte")],
                    "{case}"
                );
            } else {
                assert!(
                    !rt.logs.is_empty()
                        && rt.logs.len() <= 2
                        && rt
                            .logs
                            .iter()
                            .all(|l| l.text == warning("Tábua A") || l.text == warning("Tábua B")),
                    "{case}: {:?}",
                    rt.logs
                );
            }
        }
    }
}

const TURNTABLE: &str = "turntable";
/// A 6 × 0.5 × 6 m turntable at (20, 0.75, 0) that turns a quarter turn a second about its
/// centre, as a clip played from the scene start moves it, with the character on it 2 m
/// from the centre along X.
fn turntable() -> Project {
    use oxy_core::{
        animation::{Clip, Interpolation, Keyframe},
        graph::{Edge, Node},
        surface::{PlatformMode, TranslationPlatform},
    };
    let mut project = fixture();
    let mut table = Entity::new("Carrossel", Some(Primitive::Cube));
    table.id = TURNTABLE.into();
    table.dimensions = [6., 0.5, 6.];
    table.transform.position = [20., 0.75, 0.];
    table.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: table.dimensions,
        },
        ..Default::default()
    });
    table.platform = Some(TranslationPlatform {
        mode: PlatformMode::Animation,
        ..Default::default()
    });
    // A key every quarter turn: in between, the turn takes the shorter way.
    let mut clip = Clip {
        duration: 4.,
        looping: true,
        ..Clip::new("Giro")
    };
    for quarter in 0..=4 {
        let mut transform = table.transform.clone();
        transform.rotation[1] = (90. * quarter as f32).to_radians();
        clip.insert_key(
            TURNTABLE,
            Keyframe {
                time: quarter as f32,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    let start = Node::new("event.scene_start", [0.; 2]);
    let mut play = Node::new("action.animation", [0.; 2]);
    play.params
        .insert("clip".into(), Value::Text(clip.id.clone()));
    table.graph.edges.push(Edge {
        from_node: start.id.clone(),
        from_port: "exec".into(),
        to_node: play.id.clone(),
        to_port: "exec".into(),
    });
    table.graph.nodes = vec![start, play];
    table.clips.push(clip);
    let scene = &mut project.scenes[0];
    scene.entity_mut(BODY).unwrap().transform.position = [22., 1.02, 0.];
    scene.entities.push(table);
    project
}
/// `point` turned `degrees` about the turntable's vertical axis.
fn around_turntable(point: Vec3, degrees: f32) -> Vec3 {
    let centre = Vec3::new(20., point.y, 0.);
    centre + Quat::from_rotation_y(degrees.to_radians()) * (point - centre)
}
/// The angle between two headings, in radians, whatever whole turns separate them.
fn heading_difference(a: f32, b: f32) -> f32 {
    Quat::from_rotation_y(a).angle_between(Quat::from_rotation_y(b))
}

#[test]
fn a_turning_platform_carries_position_and_yaw() {
    let project = turntable();
    let mut rt = runtime(&project);
    // Landing keeps the world speed; braking then matches the turntable's.
    for _ in 0..60 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    let before = rt.character_state(BODY).unwrap().clone();
    assert!(
        before.grounded && before.velocity.length() < 1e-3,
        "{before:?}"
    );
    assert!(
        before.position.with_y(0.).distance(Vec3::new(20., 0., 0.)) > 1.5,
        "{before:?}"
    );
    // One second: a quarter turn of the turntable, the first person view and the body.
    let view = rt.camera_orientation().unwrap();
    for step in 0..60 {
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(state.grounded, "step {step}: {state:?}");
        assert_eq!(state.support.as_ref().unwrap().object, TURNTABLE);
    }
    let state = rt.character_state(BODY).unwrap();
    let expected = around_turntable(before.position, 90.);
    assert!(
        state.position.distance(expected) < 0.02,
        "{:?} != {expected:?}",
        state.position
    );
    assert!(
        (heading_difference(state.yaw, before.yaw) - 90f32.to_radians()).abs() < 1e-3,
        "{} -> {}",
        before.yaw,
        state.yaw
    );
    assert!(
        rt.camera_orientation()
            .unwrap()
            .angle_between(Quat::from_rotation_y(90f32.to_radians()) * view)
            < 1e-3
    );
    // Standing still on it: what moves is the turntable.
    assert!(state.velocity.length() < 1e-3, "{state:?}");
    assert!(
        (state.total_velocity().length() - 2. * std::f32::consts::FRAC_PI_2).abs() < 0.05,
        "{state:?}"
    );
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    // On past the end of the clip, which loops at 4 s from where it started: a whole turn.
    for step in 0..180 {
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(state.grounded, "step {step}: {state:?}");
        assert_eq!(state.support.as_ref().unwrap().object, TURNTABLE);
    }
    let state = rt.character_state(BODY).unwrap();
    assert!(
        state.position.distance(before.position) < 0.05,
        "{:?} != {:?}",
        state.position,
        before.position
    );
    assert!(
        heading_difference(state.yaw, before.yaw) < 1e-3,
        "{} -> {}",
        before.yaw,
        state.yaw
    );
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn a_tilting_platform_carries_its_rider_round_its_pivot_without_turning_it() {
    let mut project = turntable();
    let scene = &mut project.scenes[0];
    // Tilted by hand about X, 1° a step, with the character on it 2 m from the centre along
    // Z, the side that rises.
    let table = scene.entity_mut(TURNTABLE).unwrap();
    table.graph = Default::default();
    table.clips.clear();
    scene.entity_mut(BODY).unwrap().transform.position = [20., 1.02, 2.];
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.character_state(BODY).unwrap().clone();
    assert!(before.grounded, "{before:?}");
    let view = rt.camera_orientation().unwrap();
    let pivot = Vec3::new(20., 0.75, 0.);
    let mut tilted = 0_f32;
    for step in 0..10 {
        tilted += 1.;
        let table = rt.scene_mut().entity_mut(TURNTABLE).unwrap();
        table.transform.rotation[0] = -tilted.to_radians();
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(state.grounded, "step {step}: {state:?}");
        assert_eq!(state.support.as_ref().unwrap().object, TURNTABLE);
        let expected =
            pivot + Quat::from_rotation_x(-tilted.to_radians()) * (before.position - pivot);
        assert!(
            state.position.distance(expected) < 0.02,
            "step {step}: {:?} != {expected:?}",
            state.position
        );
        assert_eq!(state.yaw, before.yaw, "step {step}");
    }
    assert!(rt.character_state(BODY).unwrap().position.y > before.position.y + 0.3);
    assert!(rt.camera_orientation().unwrap().angle_between(view) < 1e-6);
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn jumping_off_a_spinning_platform_inherits_contact_point_velocity() {
    for policy in [
        InheritPlatform::None,
        InheritPlatform::Horizontal,
        InheritPlatform::All,
    ] {
        let mut project = turntable();
        project.scenes[0]
            .entity_mut(BODY)
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .inherit_platform = policy;
        let mut rt = runtime(&project);
        for _ in 0..60 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        let before = rt.character_state(BODY).unwrap().position;
        rt.request_jump(BODY).unwrap();
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(!state.grounded && state.support.is_none(), "{state:?}");
        // The step carries the feet 1.5° round before they leave; the point under them
        // goes on as the turntable does.
        let feet = around_turntable(before, 1.5);
        let point = (around_turntable(feet, 1.5) - feet) / FIXED_DT;
        assert!(
            (point.length() - std::f32::consts::PI).abs() < 0.01,
            "{point}"
        );
        let horizontal = state.velocity.with_y(0.);
        let inherited = if policy == InheritPlatform::None {
            Vec3::ZERO
        } else {
            point
        };
        assert!(
            horizontal.distance(inherited) < 0.01,
            "{policy:?}: {horizontal} != {inherited}"
        );
        assert!(
            (state.velocity.y - (8. - 22. * FIXED_DT)).abs() < 0.003,
            "{state:?}"
        );
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    }
}

#[test]
fn a_box_body_keeps_position_carry_when_turning_would_penetrate() {
    let mut project = turntable();
    let scene = &mut project.scenes[0];
    // Turned by hand, 3° a step.
    let table = scene.entity_mut(TURNTABLE).unwrap();
    table.graph = Default::default();
    table.clips.clear();
    let config = scene
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap();
    config.body.standing = CollisionShape::Box {
        size: [1., 1.8, 1.],
    };
    // A wall hung over the turntable, 12 mm from the body's outer face on the side its
    // corner would turn into: carrying the body moves it away, turning it would not fit.
    let mut wall = Entity::new("Parede", None);
    wall.id = "wall".into();
    wall.transform.position = [22.5 + 0.012 + 0.1, 2., 0.5];
    wall.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [0.2, 1.8, 1.],
        },
        ..Default::default()
    });
    scene.entities.push(wall);
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.character_state(BODY).unwrap().clone();
    assert!(before.grounded, "{before:?}");
    assert!(
        before
            .position
            .with_y(0.)
            .abs_diff_eq(Vec3::new(22., 0., 0.), 1e-3),
        "{before:?}"
    );
    let mut turned = 0.;
    for step in 0..2 {
        turned += 3_f32;
        let table = rt.scene_mut().entity_mut(TURNTABLE).unwrap();
        table.transform.rotation[1] = turned.to_radians();
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(state.grounded, "step {step}: {state:?}");
        assert!(
            state
                .position
                .abs_diff_eq(around_turntable(before.position, turned), 1e-3),
            "step {step}: {state:?}"
        );
        assert_eq!(state.yaw, before.yaw, "step {step}");
    }
    // Clear of the wall, it turns with the turntable again.
    for _ in 0..10 {
        turned += 3_f32;
        let table = rt.scene_mut().entity_mut(TURNTABLE).unwrap();
        table.transform.rotation[1] = turned.to_radians();
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    let state = rt.character_state(BODY).unwrap();
    assert!(state.grounded, "{state:?}");
    assert!(
        heading_difference(state.yaw, before.yaw) > 20_f32.to_radians(),
        "{state:?}"
    );
    let world = rt.physics_world().unwrap();
    let body = rt
        .scene()
        .entity(BODY)
        .unwrap()
        .character3d
        .as_ref()
        .unwrap();
    let motion = body
        .body
        .prepare(false, body.crouch_height, 1., state.yaw)
        .unwrap();
    assert!(
        world
            .penetrating_prepared(
                &motion.geometry,
                motion.rotation,
                motion.at(state.position),
                &QueryOptions::excluding(BODY)
            )
            .unwrap()
            .is_empty()
    );
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn a_railing_of_the_same_moving_group_does_not_block_the_passenger() {
    use oxy_core::surface::{PlatformMode, TranslationPlatform};
    let mut project = fixture();
    // A turntable group: the floor and a railing are its pieces, the passenger a box body
    // that stands 15 mm from the railing. Carried, it would cross the railing, which has
    // already turned; turned with it, it keeps its place.
    let mut group = Entity::new("Carrossel", None);
    group.id = TURNTABLE.into();
    group.transform.position = [20., 0.75, 0.];
    group.platform = Some(TranslationPlatform {
        mode: PlatformMode::Animation,
        ..Default::default()
    });
    let piece = |id: &str, size: [f32; 3], position: [f32; 3]| {
        let mut piece = Entity::new(id, Some(Primitive::Cube));
        piece.id = id.into();
        piece.parent = Some(TURNTABLE.into());
        piece.dimensions = size;
        piece.transform.position = position;
        piece.physics3d = Some(Collider3d {
            shape: CollisionShape::Box { size },
            ..Default::default()
        });
        piece
    };
    let floor = piece("Piso", [6., 0.5, 6.], [0., 0., 0.]);
    let railing = piece("Grade", [0.2, 1., 3.], [2.5 + 0.015 + 0.1, 0.75, 0.]);
    let scene = &mut project.scenes[0];
    let body = scene.entity_mut(BODY).unwrap();
    body.transform.position = [22., 1.02, 0.];
    body.character3d.as_mut().unwrap().body.standing = CollisionShape::Box {
        size: [1., 1.8, 1.],
    };
    scene.entities.extend([group, floor, railing]);
    let mut rt = runtime(&project);
    settle(&mut rt);
    let before = rt.character_state(BODY).unwrap().clone();
    assert!(before.grounded, "{before:?}");
    let mut turned = 0_f32;
    for step in 0..40 {
        turned += 3.;
        let group = rt.scene_mut().entity_mut(TURNTABLE).unwrap();
        group.transform.rotation[1] = turned.to_radians();
        rt.advance(FIXED_DT, &InputFrame::default());
        let state = rt.character_state(BODY).unwrap();
        assert!(state.grounded, "step {step}: {state:?}");
        assert!(
            state
                .position
                .abs_diff_eq(around_turntable(before.position, turned), 1e-3),
            "step {step}: {state:?}"
        );
        assert!(
            (heading_difference(state.yaw, before.yaw) - turned.to_radians()).abs() < 1e-3,
            "step {step}: {state:?}"
        );
    }
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

const FLOOR: &str = "00000000-0000-4000-8000-000000000001";
const ANGLES: [f32; 5] = [0., 13.5, 30., 45., 77.];
/// The fixture's floor as a box, the hull of that box or a plane of two triangles, `size` wide
/// with its top at y = 0 and turned `degrees` about the vertical through its middle, with the
/// character's feet at `feet`.
fn turned_floor(kind: &str, size: f32, degrees: f32, feet: Vec3) -> Project {
    let mut project = fixture();
    let scene = &mut project.scenes[0];
    let floor = scene.entity_mut(FLOOR).unwrap();
    floor.dimensions = [size, 1., size];
    floor.transform.rotation[1] = degrees.to_radians();
    floor.physics3d = Some(match kind {
        "box" => Collider3d {
            shape: CollisionShape::Box {
                size: floor.dimensions,
            },
            ..Default::default()
        },
        "hull" => generate_collider(floor, true).unwrap(),
        _ => {
            floor.primitive = Some(Primitive::Plane);
            floor.transform.position[1] = 0.;
            generate_collider(floor, false).unwrap()
        }
    });
    scene.entity_mut(BODY).unwrap().transform.position = feet.to_array();
    project
}
/// The middle of a floor turned `degrees`, a millimetre from it, a point on one of its
/// diagonals and one off them, at feet height `y`.
fn spots(degrees: f32, y: f32) -> [Vec3; 4] {
    let turn = Quat::from_rotation_y(degrees.to_radians());
    [
        Vec3::ZERO,
        Vec3::new(0.001, 0., 0.),
        turn * Vec3::new(0.5, 0., 0.5),
        turn * Vec3::new(0.7, 0., -0.2),
    ]
    .map(|spot| spot.with_y(y))
}
/// Standing or walking on support, with the capsule its skin above `top` and nothing logged.
fn assert_on_top(rt: &Runtime, top: f32, case: &str) {
    let state = rt.character_state(BODY).unwrap();
    let skin = CharacterConfig::default().margin;
    assert!(state.grounded, "{case}: {state:?}");
    assert!(state.position.y >= top + skin - 5e-4, "{case}: {state:?}");
    assert!(rt.logs.is_empty(), "{case}: {:?}", rt.logs);
}

#[test]
fn a_character_stands_over_the_middle_of_a_turned_floor() {
    // 180 runs of two seconds: each kind and size of floor runs on its own thread.
    std::thread::scope(|threads| {
        for kind in ["box", "hull", "plane"] {
            for size in [6., 20., 100.] {
                threads.spawn(move || stands_over_the_middle(kind, size));
            }
        }
    });
}

fn stands_over_the_middle(kind: &str, size: f32) {
    for degrees in ANGLES {
        for feet in spots(degrees, 0.02) {
            let case = format!("{kind} {size} m, {degrees}°, at {feet}");
            let project = turned_floor(kind, size, degrees, feet);
            let mut rt = runtime(&project);
            rt.advance(FIXED_DT, &InputFrame::default());
            for step in 0..120 {
                rt.advance(FIXED_DT, &InputFrame::default());
                let case = format!("{case}, step {step}");
                assert_on_top(&rt, 0., &case);
                let state = rt.character_state(BODY).unwrap();
                assert_eq!(state.support.as_ref().unwrap().object, FLOOR, "{case}");
                assert!(
                    state.position.with_y(0.).distance(feet.with_y(0.)) < 1e-3,
                    "{case}: {state:?}"
                );
            }
        }
    }
}

#[test]
fn a_floor_turned_under_a_standing_character_keeps_it_on_top() {
    for size in [6., 20.] {
        for feet in spots(0., 0.02) {
            let case = format!("{size} m at {feet}");
            let project = turned_floor("box", size, 0., feet);
            let mut rt = runtime(&project);
            settle(&mut rt);
            // Turned by hand a degree and a half a step, a whole turn: nothing carries it round.
            for step in 1..=240 {
                let floor = rt.scene_mut().entity_mut(FLOOR).unwrap();
                floor.transform.rotation[1] = (1.5 * step as f32).to_radians();
                rt.advance(FIXED_DT, &InputFrame::default());
                let case = format!("{case}, step {step}");
                assert_on_top(&rt, 0., &case);
                let state = rt.character_state(BODY).unwrap();
                assert!(
                    state.position.with_y(0.).distance(feet.with_y(0.)) < 1e-3,
                    "{case}: {state:?}"
                );
            }
        }
    }
}

#[test]
fn a_passenger_on_the_middle_of_a_turntable_turns_on_the_spot() {
    for size in [6., 20.] {
        let mut project = turntable();
        let scene = &mut project.scenes[0];
        let table = scene.entity_mut(TURNTABLE).unwrap();
        table.dimensions = [size, 0.5, size];
        table.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: table.dimensions,
            },
            ..Default::default()
        });
        scene.entity_mut(BODY).unwrap().transform.position = [20., 1.02, 0.];
        let mut rt = runtime(&project);
        for _ in 0..10 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        let before = rt.character_state(BODY).unwrap().clone();
        // Five seconds: a turn and a quarter of the turntable, which turns the body with it.
        for step in 0..300 {
            rt.advance(FIXED_DT, &InputFrame::default());
            let case = format!("{size} m, step {step}");
            assert_on_top(&rt, 1., &case);
            let state = rt.character_state(BODY).unwrap();
            assert_eq!(state.support.as_ref().unwrap().object, TURNTABLE, "{case}");
            assert!(
                state.position.with_y(0.).distance(Vec3::new(20., 0., 0.)) < 1e-3,
                "{case}: {state:?}"
            );
        }
        let state = rt.character_state(BODY).unwrap();
        assert!(
            (heading_difference(state.yaw, before.yaw) - 90f32.to_radians()).abs() < 1e-2,
            "{size} m: {} -> {}",
            before.yaw,
            state.yaw
        );
    }
}

#[test]
fn walking_across_the_middle_of_a_turned_floor_stays_on_top_and_on_course() {
    let input = InputFrame {
        held: ["mover_frente".into()].into(),
        ..Default::default()
    };
    for kind in ["box", "hull", "plane"] {
        for degrees in ANGLES {
            for x in [0., 0.001] {
                let case = format!("{kind}, {degrees}°, x {x}");
                let start = Vec3::new(x, 0.02, 3.);
                let project = turned_floor(kind, 20., degrees, start);
                let mut rt = runtime(&project);
                rt.advance(FIXED_DT, &InputFrame::default());
                // Forward is -Z: one second at 6 m/s crosses the middle halfway through.
                for step in 0..60 {
                    rt.advance(FIXED_DT, &input);
                    let case = format!("{case}, step {step}");
                    assert_on_top(&rt, 0., &case);
                    let state = rt.character_state(BODY).unwrap();
                    assert!((state.position.x - x).abs() < 1e-3, "{case}: {state:?}");
                }
                let state = rt.character_state(BODY).unwrap();
                assert!((state.position.z + 3.).abs() < 0.01, "{case}: {state:?}");
            }
        }
    }
}

#[test]
fn a_character_placed_deep_in_a_floor_climbs_out_through_its_top() {
    for size in [20., 100.] {
        for degrees in [0., 13.5] {
            for feet in [
                Vec3::new(0., -0.8, 0.),
                Vec3::new(0.7, -0.8, -0.2),
                Vec3::new(3., -1., 2.),
                Vec3::new(0., -1.2, 0.),
            ] {
                let case = format!("{size} m, {degrees}°, from {feet}");
                let project = turned_floor("box", size, degrees, feet);
                let mut rt = runtime(&project);
                settle(&mut rt);
                let state = rt.character_state(BODY).unwrap();
                assert!(state.grounded, "{case}: {state:?}");
                assert!(state.position.y > 0.009, "{case}: {state:?}");
                assert!(
                    state.position.with_y(0.).distance(feet.with_y(0.)) < 1e-3,
                    "{case}: {state:?}"
                );
            }
        }
    }
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

#[test]
fn coyote_time_and_the_jump_buffer_run_on_game_time_across_a_pause() {
    let pause = |rt: &mut Runtime| {
        let before = rt.character_state(BODY).unwrap().clone();
        assert!(rt.set_game_paused(true));
        // A second of run time, with a jump pressed that the pause keeps from the body.
        for step in 0..60 {
            let input = InputFrame {
                pressed: if step == 10 {
                    ["pular".into()].into()
                } else {
                    Default::default()
                },
                ..Default::default()
            };
            rt.advance(FIXED_DT, &input);
        }
        let state = rt.character_state(BODY).unwrap();
        assert_eq!(
            (state.position, state.velocity),
            (before.position, before.velocity)
        );
        assert!(rt.set_game_paused(false));
    };
    // Off the edge, a long pause, then a jump still inside the 100 ms of coyote time.
    let mut project = fixture();
    project.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .coyote_ms = 100.;
    let mut rt = runtime(&project);
    settle(&mut rt);
    let floor = rt.scene().entities[0].id.clone();
    rt.scene_mut().remove_subtree(&floor);
    for _ in 0..3 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    pause(&mut rt);
    rt.request_jump(BODY).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.character_state(BODY).unwrap().velocity.y > 0.);
    // Logic removing the support after a pause opens the same 100 ms on the game clock: a jump
    // asked 3 steps later comes, one asked 10 steps later does not.
    let jumps_after = |steps: usize| {
        let mut rt = runtime(&project);
        settle(&mut rt);
        pause(&mut rt);
        rt.remove_object(&floor);
        for _ in 0..steps {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        rt.request_jump(BODY).unwrap();
        rt.advance(FIXED_DT, &InputFrame::default());
        assert!(rt.logs.is_empty(), "{:?}", rt.logs);
        rt.character_state(BODY).unwrap().velocity.y > 0.
    };
    assert!(jumps_after(3));
    assert!(!jumps_after(10));

    // A jump asked in the air, a long pause, then the landing inside the 120 ms buffer.
    let mut project = fixture();
    let body = project.scenes[0].entity_mut(BODY).unwrap();
    body.transform.position[1] = 0.15;
    body.character3d.as_mut().unwrap().jump_buffer_ms = 120.;
    let mut rt = runtime(&project);
    rt.set_character_velocity(BODY, -Vec3::Y * 3.).unwrap();
    rt.request_jump(BODY).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(!rt.character_state(BODY).unwrap().grounded);
    pause(&mut rt);
    let mut jumped = 0;
    for _ in 0..8 {
        rt.advance(FIXED_DT, &InputFrame::default());
        jumped += rt
            .movement_events()
            .iter()
            .filter(|(_, e)| matches!(e, MovementEvent::Jumped))
            .count();
    }
    assert_eq!(jumped, 1);
    // A jump logic asks during the pause is dropped as it ends.
    let mut rt = runtime(&fixture());
    settle(&mut rt);
    rt.set_game_paused(true);
    rt.request_jump(BODY).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.set_game_paused(false);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(rt.character_state(BODY).unwrap().grounded);
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn the_game_pause_holds_the_camera_frees_the_cursor_and_keeps_a_toggled_crouch() {
    use oxy_core::input_timeline::{InputChange, TimedInput};
    let project = fixture();
    let mut rt = runtime(&project);
    settle(&mut rt);
    assert!(rt.wants_relative_mouse());
    rt.request_crouch(BODY, true).unwrap();
    for _ in 0..10 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Crouched);
    let orientation = rt.camera_orientation().unwrap();
    let position = rt.character_state(BODY).unwrap().position;
    rt.set_game_paused(true);
    assert!(!rt.wants_relative_mouse(), "a menu needs the cursor");
    rt.advance(FIXED_DT * 0.5, &InputFrame::default());
    let pose = rt.game_camera_pose().unwrap();
    rt.queue_timed_input(TimedInput {
        at: rt.time + 0.1,
        change: InputChange::Look([300., 40.]),
    })
    .unwrap();
    let looking = InputFrame {
        look: [500., 120.],
        held: ["mover_frente".into()].into(),
        ..Default::default()
    };
    for _ in 0..40 {
        rt.advance(FIXED_DT * 0.7, &looking);
        let shown = rt.game_camera_pose().unwrap();
        assert_eq!(shown.position, pose.position);
        assert!(shown.rotation.abs_diff_eq(pose.rotation, 1e-6));
    }
    assert_eq!(rt.character_state(BODY).unwrap().position, position);
    rt.set_game_paused(false);
    assert!(rt.wants_relative_mouse());
    rt.advance(FIXED_DT, &InputFrame::default());
    // Look given in the pause is not applied afterwards; the crouch asked before it stays.
    assert!(
        rt.camera_orientation()
            .unwrap()
            .abs_diff_eq(orientation, 1e-6)
    );
    assert_eq!(rt.character_state(BODY).unwrap().posture, Posture::Crouched);
    rt.advance(
        FIXED_DT,
        &InputFrame {
            look: [750., 0.],
            ..Default::default()
        },
    );
    assert!(
        !rt.camera_orientation()
            .unwrap()
            .abs_diff_eq(orientation, 1e-3)
    );
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn the_cursor_node_frees_the_mouse_and_turns_no_camera_while_the_game_runs() {
    use oxy_core::graph::{Edge, Node};
    use oxy_core::input_timeline::{InputChange, TimedInput};
    let mut project = fixture();
    let body = project.scenes[0].entity_mut(BODY).unwrap();
    for (action, mode) in [("mostrar", "show"), ("esconder", "hide")] {
        let mut input = Node::new("event.input", [0.; 2]);
        input
            .params
            .insert("action".into(), Value::Text(action.into()));
        let mut cursor = Node::new("game.cursor", [0.; 2]);
        cursor
            .params
            .insert("mode".into(), Value::Text(mode.into()));
        body.graph.edges.push(Edge {
            from_node: input.id.clone(),
            from_port: "exec".into(),
            to_node: cursor.id.clone(),
            to_port: "exec".into(),
        });
        body.graph.nodes.extend([input, cursor]);
    }
    let press = |action: &str| InputFrame {
        pressed: [action.into()].into(),
        ..Default::default()
    };
    let mut rt = runtime(&project);
    settle(&mut rt);
    assert!(rt.wants_relative_mouse() && !rt.cursor_shown());
    rt.advance(FIXED_DT, &press("mostrar"));
    assert!(
        rt.cursor_shown() && !rt.wants_relative_mouse(),
        "the mouse goes free"
    );
    assert!(!rt.game_paused());
    // The state says so.
    let mut twin = runtime(&project);
    let before = twin.state_digest();
    twin.set_cursor_shown(true);
    let difference = before.first_difference(&twin.state_digest());
    assert_eq!(difference.map(|d| d.part).as_deref(), Some("scene"));
    // The world goes on and the character walks, but the mouse turns nothing, timed look too.
    let orientation = rt.camera_orientation().unwrap();
    let start = rt.character_state(BODY).unwrap().position;
    rt.queue_timed_input(TimedInput {
        at: rt.time + 0.05,
        change: InputChange::Look([300., 40.]),
    })
    .unwrap();
    let walking = InputFrame {
        held: ["mover_frente".into()].into(),
        look: [500., 120.],
        ..Default::default()
    };
    for _ in 0..30 {
        rt.advance(FIXED_DT, &walking);
        assert!(
            rt.camera_orientation()
                .unwrap()
                .abs_diff_eq(orientation, 1e-6)
        );
    }
    assert!(rt.character_state(BODY).unwrap().position.distance(start) > 1.);
    // Hidden, the mouse is the camera's again.
    rt.advance(FIXED_DT, &press("esconder"));
    assert!(!rt.cursor_shown() && rt.wants_relative_mouse());
    rt.advance(
        FIXED_DT,
        &InputFrame {
            look: [300., 0.],
            ..Default::default()
        },
    );
    assert!(
        !rt.camera_orientation()
            .unwrap()
            .abs_diff_eq(orientation, 1e-3)
    );
    // Shown before the game pause, it stays free after it.
    rt.advance(FIXED_DT, &press("mostrar"));
    assert!(rt.set_game_paused(true));
    assert!(!rt.wants_relative_mouse());
    assert!(rt.set_game_paused(false));
    assert!(!rt.wants_relative_mouse());
    // A new scene starts with the camera's own capture.
    rt.change_scene(&project.start_scene).unwrap();
    assert!(!rt.cursor_shown() && rt.wants_relative_mouse());
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn an_animated_platform_and_its_passenger_freeze_in_the_game_pause() {
    use oxy_core::{
        animation::{Clip, Interpolation, Keyframe},
        graph::{Edge, Node},
        surface::{PlatformMode, TranslationPlatform},
    };
    let mut project = fixture();
    let mut lift = Entity::new("Elevador", Some(Primitive::Cube));
    lift.dimensions = [4., 0.5, 4.];
    lift.transform.position = [20., 0.75, 0.];
    lift.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: lift.dimensions,
        },
        ..Default::default()
    });
    lift.platform = Some(TranslationPlatform {
        mode: PlatformMode::Animation,
        ..Default::default()
    });
    let mut clip = Clip {
        duration: 2.,
        looping: true,
        ..Clip::new("Vaivém")
    };
    for (time, x) in [(0., 20.), (1., 24.), (2., 20.)] {
        let mut transform = lift.transform.clone();
        transform.position[0] = x;
        clip.insert_key(
            &lift.id,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    let start = Node::new("event.scene_start", [0.; 2]);
    let mut play = Node::new("action.animation", [0.; 2]);
    play.params
        .insert("clip".into(), Value::Text(clip.id.clone()));
    lift.graph.edges.push(Edge {
        from_node: start.id.clone(),
        from_port: "exec".into(),
        to_node: play.id.clone(),
        to_port: "exec".into(),
    });
    lift.graph.nodes = vec![start, play];
    lift.clips.push(clip);
    let lift_id = lift.id.clone();
    let scene = &mut project.scenes[0];
    scene.entity_mut(BODY).unwrap().transform.position = [20., 1.02, 0.];
    scene.entities.push(lift);
    let mut rt = runtime(&project);
    settle(&mut rt);
    let x = |rt: &Runtime| {
        (
            rt.scene().entity(&lift_id).unwrap().transform.position[0],
            rt.character_state(BODY).unwrap().position.x,
        )
    };
    let before = x(&rt);
    assert!(before.0 > 21., "{before:?}");
    let state = rt.character_state(BODY).unwrap();
    assert_eq!(state.support.as_ref().unwrap().object, lift_id);
    rt.set_game_paused(true);
    for _ in 0..30 {
        rt.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(x(&rt), before);
    }
    rt.set_game_paused(false);
    rt.advance(FIXED_DT, &InputFrame::default());
    let after = x(&rt);
    // One step of the clip, 4 m a second, with the passenger carried alike.
    let moved = (after.0 - before.0, after.1 - before.1);
    assert!((moved.0 - 4. * FIXED_DT).abs() < 1e-4, "{moved:?}");
    assert!((moved.1 - moved.0).abs() < 1e-4, "{moved:?}");
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}
