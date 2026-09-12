use glam::{Mat4, Quat, Vec3};
use oxy_core::{
    document::*, edit_history::CommandHistory, migration, physics3d::*, texture_cache::TextureCache,
};

#[test]
fn generated_collision_is_independent_and_survives_history_models_and_roundtrip() {
    let mut p = Project::new("Autoria física");
    p.scenes[0].kind = SceneKind::ThreeD;
    let mut e = Entity::new("Tubo", Some(Primitive::Tube));
    e.dimensions = [4., 1., 4.];
    let id = e.id.clone();
    let original = e.clone();
    e.physics3d = Some(generate_collider(&e, false).unwrap());
    assert_eq!(original.primitive, e.primitive);
    assert_eq!(original.dimensions, e.dimensions);
    assert_eq!(original.mesh, e.mesh);
    p.scenes[0].entities.push(e);
    let base = p.clone();
    let mut images = TextureCache::default();
    let mut h = CommandHistory::new();
    h.begin("Mudar aparência", &p, &images);
    p.scenes[0].entities[0].dimensions[0] = 8.;
    h.commit(&p, &mut images).unwrap();
    assert_eq!(
        p.scenes[0].entities[0].physics3d,
        base.scenes[0].entities[0].physics3d
    );
    h.undo(&mut p, &mut images).unwrap();
    assert_eq!(p, base);
    h.redo(&mut p, &mut images).unwrap();
    let scene = p.start_scene.clone();
    let model = p.save_model(&scene, &id, "Modelo físico").unwrap();
    let instance = p.instantiate_model(&model, &scene).unwrap();
    assert_ne!(id, instance);
    assert_eq!(
        p.scenes[0].entity(&id).unwrap().physics3d,
        p.scenes[0].entity(&instance).unwrap().physics3d
    );
    validate_project(&p).unwrap();
    let bytes = serde_json::to_vec(&p).unwrap();
    let roundtrip = migration::read(&bytes).unwrap();
    assert_eq!(p, roundtrip);
    let mut world = PhysicsWorld::new();
    world.sync_scene(&roundtrip.scenes[0], false).unwrap();
    assert!(
        world
            .ray(Vec3::Y * 4., -Vec3::Y, 10., &QueryOptions::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn schema_two_migration_keeps_legacy_controller_exactly_and_does_not_add_new_body() {
    let mut p = Project::new("Legado");
    p.schema_version = 2;
    p.scenes[0].kind = SceneKind::ThreeD;
    let mut e = Entity::new("Controlado", None);
    e.controller = Some(Controller {
        speed: 7.3,
        jump: 9.1,
        ..Default::default()
    });
    e.collider = Some(Collider::default());
    p.scenes[0].entities.push(e);
    let bytes = serde_json::to_vec(&p).unwrap();
    let (converted, from) = migration::read_report(&bytes).unwrap();
    assert_eq!(from, Some(2));
    assert_eq!(converted.schema_version, SCHEMA_VERSION);
    assert_eq!(converted.scenes, p.scenes);
    assert!(converted.scenes[0].entities[0].physics3d.is_none());
}

#[test]
fn sheared_ancestors_are_rejected_and_physics_preview_uses_the_runtime_shape() {
    let shear = Mat4::from_scale(Vec3::new(2., 1., 1.)) * Mat4::from_rotation_y(0.6);
    assert!(world_pose(shear).is_err());
    let mut world = PhysicsWorld::new();
    let config = Collider3d {
        shape: CollisionShape::Box { size: [2., 4., 6.] },
        ..Default::default()
    };
    world
        .upsert(
            "id",
            &config,
            Vec3::new(3., 2., 1.),
            Quat::IDENTITY,
            Vec3::new(-2., 1., 1.),
        )
        .unwrap();
    world.flush();
    let body = world.debug_shapes().next().unwrap();
    let min = body
        .geometry
        .vertices
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |min, v| {
            min.min(body.position + body.rotation * Vec3::from(*v))
        });
    let max = body
        .geometry
        .vertices
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |max, v| {
            max.max(body.position + body.rotation * Vec3::from(*v))
        });
    assert_eq!(min, Vec3::new(1., 0., -2.));
    assert_eq!(max, Vec3::new(5., 4., 4.));
    let hit = world
        .ray(
            Vec3::new(3., 2., 10.),
            -Vec3::Z,
            20.,
            &QueryOptions::default(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.point.z, max.z);
}
