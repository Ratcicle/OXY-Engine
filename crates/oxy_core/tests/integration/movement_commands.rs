use glam::Vec3;
use oxy_core::{
    character::*,
    document::*,
    physics3d::*,
    runtime::{FIXED_DT, InputFrame, Runtime},
};
fn fixture() -> Project {
    let mut p = Project::new("Comandos comuns");
    let s = &mut p.scenes[0];
    s.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Piso", Some(Primitive::Cube));
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [100., 1., 100.],
        },
        ..Default::default()
    });
    let mut body = Entity::new("Corpo", None);
    body.id = "body".into();
    body.transform.position = [0., 0.02, 0.];
    body.character3d = Some(CharacterConfig::default());
    let mut cam = Entity::new("Câmera", None);
    cam.id = "camera".into();
    cam.camera = Some(Camera::default());
    cam.camera_rig = Some(CameraRig {
        target: Some(body.id.clone()),
        ..Default::default()
    });
    s.entities = vec![body, floor, cam];
    p
}
fn runtime(p: &Project) -> Runtime {
    let mut r = Runtime::new(p, &p.start_scene).unwrap();
    for _ in 0..3 {
        r.advance(FIXED_DT, &InputFrame::default());
    }
    r
}
#[test]
fn teleport_clears_motion_and_never_sweeps_the_old_path_or_camera() {
    let mut p = fixture();
    let mut area = Entity::new("Área intermediária", None);
    area.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [0.02, 5., 5.],
        },
        sensor: true,
        ..Default::default()
    });
    area.transform.position = [-5., 1., 0.];
    p.scenes[0].entities.push(area);
    let mut r = runtime(&p);
    r.add_character_velocity("body", Vec3::new(3., 8., 0.))
        .unwrap();
    r.request_jump("body").unwrap();
    r.teleport_character(
        "body",
        Vec3::new(-10., 0.02, 0.),
        TeleportOptions {
            look: Some([1.2, 0.3]),
            ..Default::default()
        },
    )
    .unwrap();
    let state = r.character_state("body").unwrap();
    assert_eq!(state.velocity, Vec3::ZERO);
    assert_eq!(state.previous_position, state.position);
    assert!(state.support.is_none());
    let camera = r.game_camera_pose().unwrap();
    assert!((camera.position.x + 10.).abs() < 1e-5);
    r.advance(FIXED_DT, &InputFrame::default());
    assert!(r.sensor_events().is_empty());
    assert!(
        !r.movement_events()
            .iter()
            .any(|(_, e)| matches!(e, MovementEvent::Jumped))
    );
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn occupied_teleport_is_atomic_and_local_search_is_bounded() {
    let mut p = fixture();
    let mut obstacle = Entity::new("Destino ocupado", None);
    obstacle.transform.position = [5., 0.5, 0.];
    obstacle.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [0.5, 1., 0.5],
        },
        ..Default::default()
    });
    p.scenes[0].entities.push(obstacle);
    let mut r = runtime(&p);
    let before = serde_json::to_value(r.scene()).unwrap();
    let pos = r.character_state("body").unwrap().position;
    assert!(
        r.teleport_character("body", Vec3::new(5., 0.02, 0.), TeleportOptions::default())
            .is_err()
    );
    assert_eq!(before, serde_json::to_value(r.scene()).unwrap());
    assert_eq!(pos, r.character_state("body").unwrap().position);
    let dest = r
        .teleport_character(
            "body",
            Vec3::new(5., 0.02, 0.),
            TeleportOptions {
                search_radius: 1.,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(dest.distance(Vec3::new(5., 0.02, 0.)) <= 1.001);
    assert!(
        r.teleport_character("body", Vec3::NAN, TeleportOptions::default())
            .is_err()
    );
}
/// The fixture with a second character, "other", 3 m along X, its body in the groups of
/// `filter`.
fn two_characters(filter: CollisionFilter) -> Project {
    let mut p = fixture();
    let mut other = Entity::new("Outro", None);
    other.id = "other".into();
    other.transform.position = [3., 0.02, 0.];
    let mut config = CharacterConfig::default();
    config.body.filter = filter;
    other.character3d = Some(config);
    p.scenes[0].entities.push(other);
    p
}
#[test]
fn a_teleport_never_puts_a_character_inside_another() {
    let p = two_characters(CollisionFilter::default());
    let mut r = runtime(&p);
    let goal = Vec3::new(-4., 0.02, 2.);
    let start = r.character_state("other").unwrap().position;
    assert_eq!(
        r.teleport_character("body", goal, TeleportOptions::default())
            .unwrap(),
        goal
    );
    // On the first one's spot, or a hair off its axis, where the contact of two capsules
    // used to be lost and the second one took the space for free.
    for offset in [Vec3::ZERO, Vec3::new(4e-5, -0.01, 0.)] {
        let error = r
            .teleport_character("other", goal + offset, TeleportOptions::default())
            .unwrap_err();
        assert!(error.contains("Destino ocupado"), "{error}");
        assert_eq!(r.character_state("other").unwrap().position, start);
    }
    let landed = r
        .teleport_character(
            "other",
            goal,
            TeleportOptions {
                search_radius: 1.,
                ..Default::default()
            },
        )
        .unwrap();
    let apart = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0., a.z - b.z).length();
    assert!(apart(landed, goal) >= 0.6, "{landed:?}");
    assert!(apart(landed, goal) <= 1.001, "{landed:?}");
    for _ in 0..30 {
        r.advance(FIXED_DT, &InputFrame::default());
    }
    let (a, b) = (
        r.character_state("body").unwrap().position,
        r.character_state("other").unwrap().position,
    );
    assert!(apart(a, b) >= 0.6 - 1e-3, "{a:?} {b:?}");
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn a_teleport_counts_only_bodies_the_filters_let_collide() {
    // The other body's mask leaves out the first body's group: the movement lets them pass
    // through each other, and so does a teleport, either way.
    let p = {
        let mut p = two_characters(CollisionFilter {
            category: 4,
            mask: !2,
            ..Default::default()
        });
        let body = p.scenes[0].entity_mut("body").unwrap();
        body.character3d.as_mut().unwrap().body.filter.category = 2;
        p
    };
    let mut r = runtime(&p);
    let here = r.character_state("body").unwrap().position;
    assert_eq!(
        r.teleport_character("other", here, TeleportOptions::default())
            .unwrap(),
        here
    );
    let there = Vec3::new(5., 0.02, 0.);
    r.teleport_character("other", there, TeleportOptions::default())
        .unwrap();
    assert_eq!(
        r.teleport_character("body", there, TeleportOptions::default())
            .unwrap(),
        there
    );
}
#[test]
fn a_teleport_sees_the_body_another_character_took_in_the_same_step() {
    // No step runs between them: the queries must not keep the other's old, thinner body.
    let p = two_characters(CollisionFilter::default());
    let mut r = runtime(&p);
    let beside = r.character_state("other").unwrap().position + Vec3::X;
    let mut config = r
        .scene()
        .entity("other")
        .unwrap()
        .character3d
        .clone()
        .unwrap();
    config.body.standing = CollisionShape::Capsule {
        height: 2.4,
        radius: 0.8,
    };
    config.crouch_height = 1.6;
    r.configure_character("other", config, false).unwrap();
    let error = r
        .teleport_character("body", beside, TeleportOptions::default())
        .unwrap_err();
    assert!(error.contains("Destino ocupado"), "{error}");
}
#[test]
fn teleport_rejects_unsupported_hierarchy_and_clamps_restored_pitch() {
    let mut p = fixture();
    let mut parent = Entity::new("Pai transformado", None);
    parent.id = "parent".into();
    p.scenes[0].entity_mut("body").unwrap().parent = Some(parent.id.clone());
    p.scenes[0].entities.push(parent);
    let mut r = runtime(&p);
    r.teleport_character(
        "body",
        Vec3::new(3., 0.02, 0.),
        TeleportOptions {
            look: Some([4., 40.]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.character_state("body").unwrap().pitch <= 85_f32.to_radians());
    r.scene_mut().entity_mut("parent").unwrap().transform.scale = [1., 2., 1.];
    let document = serde_json::to_value(r.scene()).unwrap();
    let position = r.character_state("body").unwrap().position;
    assert!(
        r.teleport_character("body", Vec3::new(4., 0.02, 0.), TeleportOptions::default())
            .is_err()
    );
    assert_eq!(document, serde_json::to_value(r.scene()).unwrap());
    assert_eq!(position, r.character_state("body").unwrap().position);
}
#[test]
fn impact_record_is_immutable_after_jump_and_external_departures_emit_once() {
    let p = fixture();
    let mut r = runtime(&p);
    r.add_character_velocity("body", Vec3::Y * 6.).unwrap();
    r.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        r.movement_records()
            .iter()
            .filter(|r| matches!(r.event, MovementEvent::LeftSupport))
            .count(),
        1
    );
    let mut landing = None;
    for _ in 0..90 {
        r.advance(FIXED_DT, &InputFrame::default());
        if let Some(record) = r
            .movement_records()
            .iter()
            .find(|r| matches!(r.event, MovementEvent::Landed { .. }))
        {
            landing = Some(record.clone());
            break;
        }
    }
    let landing = landing.unwrap();
    assert!(landing.state.total_velocity().y < -4.);
    assert!(landing.state.support.is_some());
    r.request_jump("body").unwrap();
    r.advance(FIXED_DT, &InputFrame::default());
    assert!(r.character_state("body").unwrap().velocity.y > 0.);
    assert!(landing.state.total_velocity().y < -4.);
}
#[test]
fn vectors_and_surface_references_roundtrip_models_without_changing_ids() {
    use oxy_core::surface::*;
    let mut p = fixture();
    let material = SurfaceMaterial::preset(SurfacePreset::Ice);
    let id = material.id.clone();
    p.surfaces.push(material);
    let body = p.scenes[0].entity_mut("body").unwrap();
    body.attributes
        .insert("Destino".into(), Value::Vector3([1., 2., 3.]));
    body.attributes
        .insert("Eixo".into(), Value::Vector2([0.5, -1.]));
    body.attributes
        .insert("Piso".into(), Value::Surface(Some(id.clone())));
    validate_project(&p).unwrap();
    let json = serde_json::to_vec(&p).unwrap();
    let reopened: Project = serde_json::from_slice(&json).unwrap();
    assert_eq!(p, reopened);
    assert!(remove(&mut p, &id).is_err());
    let scene = p.start_scene.clone();
    let asset = p.save_model(&scene, "body", "Base").unwrap();
    let copy = p.instantiate_model(&asset, &scene).unwrap();
    assert_eq!(
        p.scenes[0].entity(&copy).unwrap().attributes["Destino"],
        Value::Vector3([1., 2., 3.])
    );
    validate_project(&p).unwrap();
    p.scenes[0]
        .entity_mut("body")
        .unwrap()
        .attributes
        .insert("Eixo".into(), Value::Vector2([f32::NAN, 0.]));
    assert!(validate_project(&p).is_err());
}
