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

#[test]
fn fit_collider_shapes_spheres_capsules_and_the_piece_s_own_geometry() {
    use oxy_core::{command, editing};
    use serde_json::{Value as Json, json};
    let mut project = editing::blank_project("Formas", SceneKind::ThreeD).unwrap();
    let scene = project.start_scene.clone();
    let run = |project: &mut Project, value: Json| {
        command::apply_and_report(project, &scene, &command::parse(&value).unwrap())
    };
    let report = run(
        &mut project,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Sphere"}, "name": "Bola"},
            {"op": "update_entity", "entity": "$0", "patch": {"dimensions": [2.0, 2.0, 2.0]}},
            {"op": "add_component", "entity": "$0", "component": "sensor3d"},
            {"op": "create_surface", "preset": "ice"},
            {"op": "update_entity", "entity": "$0", "patch": {"physics3d": {
                "surface": "$3", "enabled": false,
                "filter": {"category": 2, "blocks_camera": false}}}},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cylinder"},
             "name": "Coluna"},
            {"op": "update_entity", "entity": "$5", "patch": {"dimensions": [1.0, 4.0, 1.0]}},
            {"op": "add_component", "entity": "$5", "component": "collider3d"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Pyramid"},
             "name": "Pirâmide"},
            {"op": "add_component", "entity": "$8", "component": "collider3d"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Tube"}, "name": "Tubo"},
            {"op": "add_component", "entity": "$10", "component": "collider3d"},
        ]),
    )
    .unwrap();
    let id = |index: &str| {
        report["created"][index][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (ball, column, pyramid, tube) = (id("0"), id("5"), id("8"), id("10"));
    let entity =
        |project: &Project, id: &str| project.scene(&scene).unwrap().entity(id).unwrap().clone();
    let collider = |project: &Project, id: &str| entity(project, id).physics3d.unwrap();
    let before = collider(&project, &ball);

    // auto: a sphere for the sphere, keeping what it is besides its shape.
    run(
        &mut project,
        json!({"op": "fit_collider", "entity": ball, "shape": "auto"}),
    )
    .unwrap();
    let fitted = collider(&project, &ball);
    assert_eq!(fitted.shape, CollisionShape::Sphere { radius: 1. });
    assert_eq!(fitted.center, [0.; 3]);
    assert!(fitted.sensor && !fitted.enabled);
    assert_eq!(
        (&fitted.filter, &fitted.surface),
        (&before.filter, &before.surface)
    );
    assert_eq!(fitted.filter.category, 2);
    // Without shape, a sphere stays a sphere; the margin is in metres.
    run(
        &mut project,
        json!({"op": "fit_collider", "entity": ball, "margin": 0.5}),
    )
    .unwrap();
    assert_eq!(
        collider(&project, &ball).shape,
        CollisionShape::Sphere { radius: 1.5 }
    );

    // A capsule stands along Y, as wide as the larger of X and Z.
    run(
        &mut project,
        json!({"op": "fit_collider", "entity": column, "shape": "capsule"}),
    )
    .unwrap();
    assert_eq!(
        collider(&project, &column).shape,
        CollisionShape::Capsule {
            height: 4.,
            radius: 0.5
        }
    );
    // auto: the convex hull for cylinders and pyramids, the triangles for tubes, as the
    // editor's Convexo da peça and Malha estática da peça make them.
    let report = run(
        &mut project,
        json!([
            {"op": "fit_collider", "entity": column, "shape": "auto"},
            {"op": "fit_collider", "entity": pyramid, "shape": "auto"},
            {"op": "fit_collider", "entity": tube, "shape": "auto"},
        ]),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
    for (id, convex) in [(&column, true), (&pyramid, true), (&tube, false)] {
        let e = entity(&project, id);
        let fitted = e.physics3d.clone().unwrap();
        assert_eq!(fitted.shape, generate_collider(&e, convex).unwrap().shape);
        assert_eq!(fitted.center, [0.; 3]);
        assert!(!fitted.sensor && fitted.enabled);
    }
    assert!(matches!(
        collider(&project, &tube).shape,
        CollisionShape::TriMesh { .. }
    ));

    // A shape made from the piece is left behind when the piece changes: warned, with the fit
    // that settles it; a fit later in the batch settles it at once.
    let report = run(
        &mut project,
        json!({"op": "update_entity", "entity": tube, "patch": {"dimensions": [3.0, 1.0, 3.0]}}),
    )
    .unwrap();
    let warning = report["warnings"][0].as_str().unwrap_or_default();
    assert!(
        warning.starts_with("Comando 0 (update_entity, o 1º do lote): A forma de Tubo")
            && warning.contains(&format!(
                "fit_collider {{\"entity\": \"{tube}\", \"shape\": \"mesh\"}}"
            )),
        "{report}"
    );
    let report = run(
        &mut project,
        json!([
            {"op": "update_entity", "entity": pyramid, "patch": {"dimensions": [2.0, 3.0, 2.0]}},
            {"op": "update_entity", "entity": tube, "patch": {"name": "Cano"}},
            {"op": "fit_collider", "entity": pyramid},
        ]),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
    let e = entity(&project, &pyramid);
    assert_eq!(
        e.physics3d.as_ref().unwrap().shape,
        generate_collider(&e, true).unwrap().shape
    );
    // Once left behind, more changes to the shape in the batch add no second warning.
    let report = run(
        &mut project,
        json!([
            {"op": "update_entity", "entity": pyramid, "patch": {"dimensions": [3.0, 3.0, 3.0]}},
            {"op": "update_entity", "entity": pyramid, "patch": {"dimensions": [4.0, 3.0, 4.0]}},
        ]),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{report}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .starts_with("Comando 0 (update_entity, o 1º do lote): A forma de Pirâmide"),
        "{report}"
    );

    // auto: a sphere stretched by its scale or its dimensions takes the convex hull, which
    // follows it, where a sphere would not.
    let report = run(
        &mut project,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Sphere"}, "name": "Ovo"},
            {"op": "set_transform", "entity": "$0", "scale": [1.0, 2.0, 1.0]},
            {"op": "add_component", "entity": "$0", "component": "collider3d"},
            {"op": "fit_collider", "entity": "$0", "shape": "auto"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Sphere"}, "name": "Disco"},
            {"op": "update_entity", "entity": "$4", "patch": {"dimensions": [4.0, 1.0, 4.0]}},
            {"op": "add_component", "entity": "$4", "component": "collider3d"},
            {"op": "fit_collider", "entity": "$4", "shape": "auto"},
        ]),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
    for index in ["0", "4"] {
        let e = entity(
            &project,
            report["created"][index][0]["id"].as_str().unwrap(),
        );
        let fitted = e.physics3d.clone().unwrap();
        assert!(
            matches!(fitted.shape, CollisionShape::Convex { .. }),
            "{}",
            e.name
        );
        assert_eq!(fitted.shape, generate_collider(&e, true).unwrap().shape);
    }

    // box turns any shape back into a box around the piece.
    run(
        &mut project,
        json!({"op": "fit_collider", "entity": tube, "shape": "box"}),
    )
    .unwrap();
    assert_eq!(
        collider(&project, &tube).shape,
        CollisionShape::Box { size: [3., 1., 3.] }
    );

    // What cannot be fitted so is refused, saying why, and nothing changes.
    for (commands, expected) in [
        (
            json!([{"op": "fit_collider", "entity": ball, "shape": "box"},
                   {"op": "set_transform", "entity": ball, "scale": [1.0, 2.0, 1.0]},
                   {"op": "fit_collider", "entity": ball, "shape": "sphere"}]),
            "Bola tem escala não uniforme no mundo (1, 2, 1)",
        ),
        (
            json!({"op": "fit_collider", "entity": column, "shape": "convex", "margin": 0.1}),
            "não leva margin",
        ),
        (
            json!([{"op": "create_entity", "kind": {"type": "group"}, "name": "Grupo"},
                   {"op": "add_component", "entity": "$0", "component": "collider3d"},
                   {"op": "fit_collider", "entity": "$0", "shape": "mesh"}]),
            "Grupo não tem forma própria para o colisor mesh",
        ),
        (
            json!({"op": "fit_collider", "entity": ball, "shape": "cone"}),
            "valor `cone` desconhecido",
        ),
    ] {
        let before = project.clone();
        let error = match command::parse(&commands) {
            Ok(commands) => command::apply(&mut project, &scene, &commands)
                .unwrap_err()
                .to_string(),
            Err(error) => error,
        };
        assert!(error.contains(expected), "{error}");
        assert_eq!(project, before);
    }
}
