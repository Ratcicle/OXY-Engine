use glam::{Quat, Vec3};
use oxy_core::{
    camera_authoring::*, character::*, document::*, edit_history::CommandHistory, physics3d::*,
    runtime::*, texture_cache::TextureCache,
};
fn fixture(mode: CameraMode) -> Project {
    let mut p = Project::new("Autoria de câmera");
    p.scenes[0].kind = SceneKind::ThreeD;
    let mut body = Entity::new("Jogador", None);
    body.id = "body".into();
    body.character3d = Some(CharacterConfig {
        enabled: false,
        ..Default::default()
    });
    body.transform.position = [3., 1., -2.];
    body.transform.scale = [2.; 3];
    let mut camera = Entity::new("Câmera", None);
    camera.id = "camera".into();
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(body.id.clone()),
        mode,
        ..Default::default()
    });
    camera.transform.position = [25., 8., 20.];
    p.scenes[0].entities = vec![body, camera];
    p
}
#[test]
fn preview_uses_runtime_camera_evaluation_and_never_moves_target_or_document() {
    for mode in [
        CameraMode::Fixed,
        CameraMode::FirstPerson,
        CameraMode::ThirdPerson,
    ] {
        let p = fixture(mode);
        let original = p.clone();
        let evaluated = CameraPreview::default()
            .evaluate(&p.scenes[0], "camera", 16. / 9., false)
            .unwrap();
        let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
        rt.set_viewport_aspect(16. / 9.);
        rt.advance(FIXED_DT, &InputFrame::default());
        let game = rt.game_camera_pose().unwrap();
        assert!(
            game.position.abs_diff_eq(evaluated.pose.position, 1e-5),
            "{mode:?}: {:?} / {:?}",
            game.position,
            evaluated.pose.position
        );
        assert!(game.rotation.abs_diff_eq(evaluated.pose.rotation, 1e-5));
        assert_eq!(p, original);
        let feet = Vec3::from(p.scenes[0].entity("body").unwrap().transform.position);
        if mode == CameraMode::FirstPerson {
            assert!(
                evaluated
                    .pose
                    .position
                    .abs_diff_eq(feet + Vec3::Y * 3.2, 1e-5)
            );
        }
        if mode == CameraMode::Fixed {
            assert_eq!(evaluated.pose.position, Vec3::new(25., 8., 20.));
        }
    }
}
#[test]
fn frustum_projects_fov_and_aspect_and_crouched_eye_is_explicit() {
    let p = fixture(CameraMode::FirstPerson);
    let mut preview = CameraPreview::default();
    let standing = preview
        .evaluate(&p.scenes[0], "camera", 16. / 9., false)
        .unwrap();
    let crouched = preview
        .evaluate(&p.scenes[0], "camera", 16. / 9., true)
        .unwrap();
    assert!((standing.pose.position.y - crouched.pose.position.y - 1.5).abs() < 1e-5);
    for fov in [30_f32, 60., 110.] {
        for aspect in [4. / 3., 16. / 9., 32. / 9.] {
            let pose = GameCameraPose {
                fov,
                position: Vec3::new(3., 5., -8.),
                rotation: Quat::from_rotation_y(0.7),
                hidden: vec![],
            };
            let corners = frustum(&pose, aspect, 2.);
            for corner in corners {
                let local = pose.rotation.inverse() * (corner - pose.position);
                assert!((local.z + 2.).abs() < 1e-5);
                assert!((local.y.abs() - 2. * (fov.to_radians() / 2.).tan()).abs() < 1e-5);
                assert!((local.x.abs() / local.y.abs() - aspect).abs() < 1e-5);
            }
        }
    }
}
#[test]
fn gizmo_fields_are_exact_and_whole_gesture_undo_redo_keeps_player_and_ids() {
    let mut p = fixture(CameraMode::ThirdPerson);
    let player = p.scenes[0].entity("body").unwrap().clone();
    let mut images = TextureCache::default();
    let mut history = CommandHistory::new();
    for field in [
        CameraField::Eye,
        CameraField::CrouchedEye,
        CameraField::Distance,
        CameraField::Shoulder,
        CameraField::FollowHeight,
    ] {
        let old = p.clone();
        history.begin("Ajustar câmera", &p, &images);
        let rig = p.scenes[0]
            .entity_mut("camera")
            .unwrap()
            .camera_rig
            .as_mut()
            .unwrap();
        let initial = field.get(rig);
        for i in 1..=20 {
            field.set(rig, initial + i as f32 * 0.01).unwrap();
        }
        history.commit(&p, &mut images).unwrap();
        let edited = p.clone();
        assert_eq!(p.scenes[0].entity("body").unwrap(), &player);
        history.undo(&mut p, &mut images).unwrap();
        assert_eq!(p, old);
        history.redo(&mut p, &mut images).unwrap();
        assert_eq!(p, edited);
    }
    let evaluated = CameraPreview::default()
        .evaluate(&p.scenes[0], "camera", 16. / 9., false)
        .unwrap();
    let guides = handles(&p.scenes[0], "camera", &evaluated).unwrap();
    assert_eq!(guides.len(), 5);
    assert_eq!(guides[0].units, 2.);
}
#[test]
fn protection_debug_reports_actual_sweep_and_preview_refreshes_after_obstacle_edit() {
    let mut p = fixture(CameraMode::ThirdPerson);
    let mut wall = Entity::new("Parede", None);
    wall.id = "wall".into();
    wall.transform.position = [3., 4., 0.];
    wall.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [10., 10., 0.2],
        },
        ..Default::default()
    });
    p.scenes[0].entities.push(wall);
    let mut preview = CameraPreview::default();
    let blocked = preview
        .evaluate(&p.scenes[0], "camera", 16. / 9., false)
        .unwrap();
    assert!(
        blocked
            .contact
            .as_ref()
            .is_some_and(|hit| hit.object == "wall")
    );
    assert!(blocked.pose.position.z < blocked.desired.z - 0.5);
    p.scenes[0].entity_mut("wall").unwrap().transform.position[0] = 100.;
    let open = preview
        .evaluate(&p.scenes[0], "camera", 16. / 9., false)
        .unwrap();
    assert!(open.contact.is_none());
    assert!(open.pose.position.abs_diff_eq(open.desired, 1e-5));
}
