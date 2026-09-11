use glam::{Mat4, Quat, Vec3};
use oxy_core::{
    character::*,
    document::*,
    physics3d::*,
    runtime::{FIXED_DT, InputFrame, Runtime},
    scene_view::SceneView,
};
use std::{collections::HashMap, sync::Arc};

const BODY: &str = "00000000-0000-4000-8000-000000000011";
const CAMERA: &str = "00000000-0000-4000-8000-000000000012";
fn cube(name: &str, position: [f32; 3], size: [f32; 3]) -> Entity {
    let mut e = Entity::new(name, Some(Primitive::Cube));
    e.transform.position = position;
    e.dimensions = size;
    e.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size },
        ..Default::default()
    });
    e
}
fn fixture(mode: CameraMode) -> Project {
    let mut p = Project::new("Câmeras verificáveis");
    let scene = &mut p.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut body = Entity::new("Personagem", None);
    body.id = BODY.into();
    body.transform.position = [0., 0.02, 0.];
    body.character3d = Some(CharacterConfig::default());
    body.physics3d = Some(Collider3d {
        shape: CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        center: [0., 0.9, 0.],
        ..Default::default()
    });
    let mut camera = Entity::new("Câmera", None);
    camera.id = CAMERA.into();
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        mode,
        target: Some(BODY.into()),
        shoulder: 0.,
        position_smoothing: 0.,
        obstruction_return: 0.15,
        transition_seconds: 0.,
        ..Default::default()
    });
    scene.entities = vec![cube("Chão", [0., -0.5, 0.], [100., 1., 100.]), body, camera];
    p
}
fn runtime(p: &Project) -> Runtime {
    let mut r = Runtime::new(p, &p.start_scene).unwrap();
    r.set_viewport_aspect(16. / 9.);
    tick(&mut r, 3);
    r
}
fn tick(r: &mut Runtime, n: usize) {
    for _ in 0..n {
        r.advance(FIXED_DT, &InputFrame::default());
    }
}
fn pose(r: &Runtime) -> GameCameraPose {
    r.game_camera_pose().unwrap()
}
fn forward(r: &Runtime) -> Vec3 {
    pose(r).rotation * -Vec3::Z
}

#[test]
fn orbit_is_independent_of_idle_body_and_movement_uses_horizontal_look() {
    let p = fixture(CameraMode::ThirdPerson);
    let mut r = runtime(&p);
    let body = r.character_state(BODY).unwrap().clone();
    r.advance(
        FIXED_DT,
        &InputFrame {
            look: [-750., -200.],
            ..Default::default()
        },
    );
    assert!((r.character_state(BODY).unwrap().yaw - body.yaw).abs() < 1e-5);
    assert!(forward(&r).x < -0.8);
    r.advance(
        FIXED_DT,
        &InputFrame {
            movement: [0., 1.],
            ..Default::default()
        },
    );
    let state = r.character_state(BODY).unwrap();
    assert!(state.velocity.x < -0.1 && state.velocity.z.abs() < 1e-4);
    assert!(
        state.velocity.y.abs() < 0.01,
        "Pitch must not add vertical movement"
    );
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn look_facing_has_bounded_rotation_and_does_not_rotate_capsule_off_vertical() {
    let mut p = fixture(CameraMode::ThirdPerson);
    let c = p.scenes[0]
        .entity_mut(BODY)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap();
    c.facing = BodyFacing::Look;
    c.angular_speed = 90.;
    let mut r = runtime(&p);
    r.advance(
        FIXED_DT,
        &InputFrame {
            look: [-750., 0.],
            ..Default::default()
        },
    );
    assert!((r.character_state(BODY).unwrap().yaw - 90f32.to_radians() * FIXED_DT).abs() < 1e-5);
    let world = r.scene().world_matrix(BODY).unwrap();
    assert!(world.transform_vector3(Vec3::Y).abs_diff_eq(Vec3::Y, 1e-5));
}
#[test]
fn wall_retracts_immediately_and_return_uses_seconds_not_frame_count() {
    let mut p = fixture(CameraMode::ThirdPerson);
    let wall = cube("Parede", [0., 2., 2.], [5., 4., 0.2]);
    let id = wall.id.clone();
    p.scenes[0].entities.push(wall);
    let mut r = runtime(&p);
    assert!(
        pose(&r).position.z < 1.74 && pose(&r).position.z > 1.6,
        "{:?}",
        pose(&r).position
    );
    r.remove_object(&id);
    r.advance(FIXED_DT, &InputFrame::default());
    let returned = pose(&r).position.z;
    assert!(returned > 1.74 && returned < 3.);
    tick(&mut r, 90);
    assert!(pose(&r).position.z > 3.99);
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn near_plane_corners_are_protected_on_ultrawide_high_fov() {
    let mut p = fixture(CameraMode::ThirdPerson);
    p.scenes[0]
        .entity_mut(CAMERA)
        .unwrap()
        .camera
        .as_mut()
        .unwrap()
        .fov = 150.;
    p.scenes[0]
        .entities
        .push(cube("Parede", [0., 2., 2.], [20., 4., 0.2]));
    let mut r = runtime(&p);
    r.set_viewport_aspect(4.);
    let radius = CAMERA_NEAR * (1. + 75f32.to_radians().tan().powi(2) * 17.).sqrt() + 0.02;
    assert!(
        pose(&r).position.z + radius < 1.901,
        "{:?} radius {radius}",
        pose(&r).position
    );
    assert!(pose(&r).position.z > 1.);
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn first_person_eye_cannot_lerp_into_crouch_ceiling() {
    let mut p = fixture(CameraMode::FirstPerson);
    p.scenes[0]
        .entities
        .push(cube("Teto", [0., 2.25, 0.], [5., 0.5, 5.]));
    let mut r = runtime(&p);
    r.request_crouch(BODY, true).unwrap();
    tick(&mut r, 1);
    // Bring a ceiling down after posture changed, while eye smoothing is high.
    let ceiling = r.scene().entities.last().unwrap().id.clone();
    r.scene_mut()
        .entity_mut(&ceiling)
        .unwrap()
        .transform
        .position[1] = 1.35;
    tick(&mut r, 1);
    assert!(
        pose(&r).position.y + 0.17 <= 1.101,
        "{:?}, {:?}",
        pose(&r).position,
        r.logs
    );
    assert_eq!(r.character_state(BODY).unwrap().posture, Posture::Crouched);
}
#[test]
fn shoulder_change_and_initial_overlap_do_not_put_camera_inside_geometry() {
    let mut p = fixture(CameraMode::ThirdPerson);
    p.scenes[0]
        .entity_mut(CAMERA)
        .unwrap()
        .camera_rig
        .as_mut()
        .unwrap()
        .shoulder = 0.7;
    p.scenes[0]
        .entities
        .push(cube("Lado", [-0.7, 1.4, 3.], [0.3, 3., 2.]));
    let mut r = runtime(&p);
    r.advance(
        FIXED_DT,
        &InputFrame {
            pressed: ["trocar_ombro".into()].into(),
            ..Default::default()
        },
    );
    let options = QueryOptions {
        camera: true,
        exclude: [BODY.into()].into(),
        ..Default::default()
    };
    assert!(
        r.physics_world()
            .unwrap()
            .penetrating(
                &CollisionShape::Sphere { radius: 0.17 },
                pose(&r).position,
                &options
            )
            .unwrap()
            .is_empty()
    );
    let mut world = PhysicsWorld::new();
    world
        .upsert(
            "wall",
            &Collider3d {
                shape: CollisionShape::Box { size: [1., 4., 4.] },
                ..Default::default()
            },
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::ONE,
        )
        .unwrap();
    world.flush();
    let recovered = world
        .recover_sphere(Vec3::new(0.55, 0., 0.), 0.2, 0.5, &QueryOptions::default())
        .unwrap();
    assert!(recovered.x >= 0.7, "{recovered:?}");
    assert!(
        world
            .recover_sphere(Vec3::ZERO, 0.2, 0.1, &QueryOptions::default())
            .is_err()
    );
}
#[test]
fn mode_transitions_preserve_physics_and_zero_duration_has_no_residual_blend() {
    let mut p = fixture(CameraMode::FirstPerson);
    let mut fixed = Entity::new("Fixa", None);
    fixed.id = "fixed".into();
    fixed.camera = Some(Camera {
        active: false,
        ..Default::default()
    });
    fixed.transform.position = [5., 4., 5.];
    p.scenes[0].entities.push(fixed);
    let original = serde_json::to_value(&p).unwrap();
    let mut r = runtime(&p);
    r.add_character_velocity(BODY, Vec3::new(2., 5., 0.))
        .unwrap();
    let before = r.character_state(BODY).unwrap().clone();
    let collider = r.scene().entity(BODY).unwrap().physics3d.clone();
    r.set_camera_mode(CAMERA, CameraMode::ThirdPerson, 0.4)
        .unwrap();
    assert_eq!(before.velocity, r.character_state(BODY).unwrap().velocity);
    assert_eq!(before.position, r.character_state(BODY).unwrap().position);
    assert_eq!(collider, r.scene().entity(BODY).unwrap().physics3d);
    r.set_camera_mode(CAMERA, CameraMode::FirstPerson, 0.)
        .unwrap();
    assert!(pose(&r).position.z.abs() < 0.01);
    r.activate_camera("fixed", 0.).unwrap();
    assert!(!r.wants_relative_mouse());
    assert!(pose(&r).position.abs_diff_eq(Vec3::new(5., 4., 5.), 1e-4));
    assert_eq!(before.velocity, r.character_state(BODY).unwrap().velocity);
    assert_eq!(original, serde_json::to_value(&p).unwrap());
    assert!(r.activate_camera("missing", 0.).is_err());
    assert_eq!(r.active_camera(), Some("fixed"));
}
#[test]
fn hidden_groups_include_descendants_and_removed_target_keeps_safe_view_once() {
    let mut p = fixture(CameraMode::FirstPerson);
    let mut group = Entity::new("Cabeça", None);
    group.id = "head".into();
    group.parent = Some(BODY.into());
    let mut child = Entity::new("Olho", Some(Primitive::Sphere));
    child.id = "eye".into();
    child.parent = Some(group.id.clone());
    p.scenes[0].entities.extend([group, child]);
    p.scenes[0]
        .entity_mut(CAMERA)
        .unwrap()
        .camera_rig
        .as_mut()
        .unwrap()
        .hidden
        .push("head".into());
    let mut r = runtime(&p);
    assert_eq!(pose(&r).hidden, vec!["head", "eye"]);
    r.set_camera_mode(CAMERA, CameraMode::ThirdPerson, 0.)
        .unwrap();
    assert!(pose(&r).hidden.is_empty());
    let last = pose(&r);
    r.remove_object(BODY);
    tick(&mut r, 100);
    assert_eq!(last.position, pose(&r).position);
    assert_eq!(r.logs.len(), 1, "{:?}", r.logs);
}
#[test]
fn presentation_bvh_reuses_shapes_and_matches_interpolated_child_without_mutating_physics() {
    let mut p = fixture(CameraMode::ThirdPerson);
    let scene = &mut p.scenes[0];
    let mut group = Entity::new("Plataforma", None);
    group.id = "platform".into();
    group.transform.position = [4., 0., 0.];
    let mut child = cube("Parede filha", [0., 1., 0.], [1., 2., 2.]);
    child.id = "child".into();
    child.parent = Some(group.id.clone());
    scene.entities.extend([group, child]);
    let mut source = PhysicsWorld::new();
    source.sync_scene(scene, false).unwrap();
    let counters = source.counters();
    let overrides = Arc::new(HashMap::from([(
        "platform".into(),
        Mat4::from_translation(Vec3::new(2., 0., 0.)),
    )]));
    let shown = SceneView::with_worlds(scene, overrides.clone());
    assert_eq!(shown.world_matrix("child").unwrap().w_axis.x, 2.);
    let mut camera = PhysicsWorld::new();
    camera
        .sync_presentation(&source, scene, overrides.clone())
        .unwrap();
    camera.sync_presentation(&source, scene, overrides).unwrap();
    assert_eq!(camera.counters().shapes_prepared, 0);
    assert!(camera.counters().shapes_shared >= 2);
    let opts = QueryOptions {
        camera: true,
        exclude: [BODY.into()].into(),
        ..Default::default()
    };
    let a = camera
        .ray(Vec3::new(0., 1., 0.), Vec3::X, 10., &opts)
        .unwrap()
        .unwrap();
    let b = source
        .ray(Vec3::new(0., 1., 0.), Vec3::X, 10., &opts)
        .unwrap()
        .unwrap();
    assert!((a.distance - 1.5).abs() < 1e-4 && (b.distance - 3.5).abs() < 1e-4);
    assert_eq!(source.counters().shapes_prepared, counters.shapes_prepared);
}
#[test]
fn raw_look_is_visible_before_tick_and_blocked_samples_do_not_replay() {
    let p = fixture(CameraMode::FirstPerson);
    let mut r = runtime(&p);
    r.advance(
        FIXED_DT * 0.2,
        &InputFrame {
            look: [100., 0.],
            ..Default::default()
        },
    );
    let shown = forward(&r);
    assert!(shown.x > 0.1);
    r.advance(FIXED_DT * 0.8, &InputFrame::default());
    assert!(shown.abs_diff_eq(forward(&r), 1e-5));
    r.block_character_input(BODY, true, "menu", true).unwrap();
    r.advance(
        FIXED_DT * 0.2,
        &InputFrame {
            look: [100., 0.],
            ..Default::default()
        },
    );
    r.block_character_input(BODY, true, "menu", false).unwrap();
    r.advance(FIXED_DT * 0.8, &InputFrame::default());
    assert!(shown.abs_diff_eq(forward(&r), 1e-5));
}

#[test]
fn assisted_look_uses_safe_origin_overrides_mouse_and_releases_without_jump() {
    let mut p = fixture(CameraMode::ThirdPerson);
    p.scenes[0]
        .entities
        .push(cube("Parede", [0., 2., 2.], [5., 4., 0.2]));
    let mut r = runtime(&p);
    let point = Vec3::new(3., 1., -4.);
    r.set_camera_look_at(CAMERA, Some(CameraLookAt::Point(point)))
        .unwrap();
    r.advance(
        FIXED_DT,
        &InputFrame {
            look: [900., 500.],
            ..Default::default()
        },
    );
    assert!(forward(&r).abs_diff_eq((point - pose(&r).position).normalize(), 1e-5));
    let before = forward(&r);
    r.set_camera_look_at(CAMERA, None).unwrap();
    assert!(before.abs_diff_eq(forward(&r), 1e-5));
    assert!(
        r.set_camera_look_at(CAMERA, Some(CameraLookAt::Point(Vec3::NAN)))
            .is_err()
    );
}
#[test]
fn animated_obstacle_queries_use_the_same_interpolation_as_rendering() {
    use oxy_core::surface::*;
    let mut p = fixture(CameraMode::ThirdPerson);
    let mut wall = cube("Parede móvel", [0., 1.5, 2.], [4., 3., 0.2]);
    let id = wall.id.clone();
    wall.platform = Some(TranslationPlatform {
        mode: PlatformMode::Velocity,
        velocity: [0., 0., -6.],
        ..Default::default()
    });
    p.scenes[0].entities.push(wall);
    let mut r = runtime(&p);
    r.advance(FIXED_DT * 0.5, &InputFrame::default());
    let scene = r.scene();
    let shown = SceneView::with_worlds(scene, r.presentation_worlds().unwrap());
    let rendered = shown.world_matrix(&id).unwrap().w_axis.z;
    let actual = scene.world_matrix(&id).unwrap().w_axis.z;
    assert!((rendered - actual - 0.05).abs() < 1e-5);
    assert!(
        (pose(&r).position.z - (rendered - 0.1 - 0.17 - 0.001)).abs() < 0.003,
        "camera {:?}, shown {rendered}, actual {actual}, {:?}",
        pose(&r).position,
        r.logs
    );
}
#[test]
fn transition_runs_in_seconds_and_competing_active_cameras_warn_once() {
    let mut p = fixture(CameraMode::FirstPerson);
    let mut fixed = Entity::new("Outra ativa", None);
    fixed.camera = Some(Camera::default());
    fixed.transform.position = [5., 4., 5.];
    p.scenes[0].entities.push(fixed);
    let mut r = runtime(&p);
    assert_eq!(r.active_camera(), Some(CAMERA));
    tick(&mut r, 120);
    assert_eq!(r.logs.len(), 1);
    r.set_camera_mode(CAMERA, CameraMode::ThirdPerson, 0.4)
        .unwrap();
    tick(&mut r, 6);
    assert!(
        pose(&r).position.z > 0.5 && pose(&r).position.z < 1.5,
        "{:?}",
        pose(&r).position
    );
    tick(&mut r, 24);
    assert!((pose(&r).position.z - 4.).abs() < 0.01);
}
#[test]
fn high_mouse_values_and_camera_settings_fail_safely_without_nonfinite_pose() {
    let p = fixture(CameraMode::FirstPerson);
    let mut r = runtime(&p);
    r.advance(
        FIXED_DT * 0.1,
        &InputFrame {
            look: [f32::MAX, f32::MAX],
            ..Default::default()
        },
    );
    r.advance(
        FIXED_DT * 0.1,
        &InputFrame {
            look: [f32::MAX, f32::MAX],
            ..Default::default()
        },
    );
    assert!(pose(&r).rotation.is_finite());
    let before = r
        .scene()
        .entity(CAMERA)
        .unwrap()
        .camera_rig
        .clone()
        .unwrap();
    let bad = CameraRig {
        min_distance: 10.,
        distance: 1.,
        ..before.clone()
    };
    assert!(r.configure_camera(CAMERA, bad).is_err());
    assert_eq!(
        r.scene().entity(CAMERA).unwrap().camera_rig.as_ref(),
        Some(&before)
    );
}
