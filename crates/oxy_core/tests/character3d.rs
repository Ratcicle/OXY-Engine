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
    body.character3d = Some(CharacterConfig::default());
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
