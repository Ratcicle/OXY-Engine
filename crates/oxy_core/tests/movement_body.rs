use glam::{Quat, Vec3};
use oxy_core::{character::*, document::*, movement_body::MovementBody, physics3d::*, runtime::*};

fn convex(size: [f32; 3]) -> CollisionShape {
    let mut piece = Entity::new("Geometria", Some(Primitive::Cube));
    piece.dimensions = size;
    MovementBody::convex_from(&piece).unwrap()
}
fn shapes() -> Vec<CollisionShape> {
    vec![
        CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        CollisionShape::Box {
            size: [0.6, 1.8, 0.6],
        },
        CollisionShape::Sphere { radius: 0.5 },
        convex([0.6, 1.8, 0.6]),
    ]
}
fn project(shape: CollisionShape, ground: CollisionShape) -> Project {
    let mut p = Project::new("Corpos configuráveis");
    let s = &mut p.scenes[0];
    s.kind = SceneKind::ThreeD;
    let mut ground_entity = Entity::new("Chão", Some(Primitive::Cube));
    ground_entity.id = "floor".into();
    ground_entity.transform.position[1] = -0.5;
    ground_entity.physics3d = Some(Collider3d {
        shape: ground,
        ..Default::default()
    });
    let mut body = Entity::new("Personagem", Some(Primitive::Cube));
    body.id = "body".into();
    body.transform.position[1] = 0.02;
    body.character3d = Some(CharacterConfig {
        body: MovementBody {
            standing: shape,
            ..Default::default()
        },
        ..Default::default()
    });
    s.entities = vec![ground_entity, body];
    p
}
fn step(rt: &mut Runtime, count: usize, input: &InputFrame) {
    for _ in 0..count {
        rt.advance(FIXED_DT, input);
    }
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}
#[test]
fn all_bodies_stand_jump_move_and_use_box_convex_and_static_mesh_worlds() {
    let mut ground_piece = Entity::new("Chão", Some(Primitive::Cube));
    ground_piece.dimensions = [100., 1., 100.];
    let grounds = [
        CollisionShape::Box {
            size: ground_piece.dimensions,
        },
        convex(ground_piece.dimensions),
        generate_collider(&ground_piece, false).unwrap().shape,
    ];
    for shape in shapes() {
        for ground in &grounds {
            let p = project(shape.clone(), ground.clone());
            let original = p.clone();
            validate_project(&p).unwrap();
            assert!(p.scenes[0].entity("body").unwrap().physics3d.is_none());
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            step(&mut rt, 30, &InputFrame::default());
            let s = rt.character_state("body").unwrap();
            assert!(s.grounded, "{shape:?}");
            assert!(
                s.position.y.abs() < 0.04,
                "shape={shape:?} ground={ground:?} state={s:?}"
            );
            rt.request_jump("body").unwrap();
            step(&mut rt, 15, &InputFrame::default());
            assert!(rt.character_state("body").unwrap().position.y > 0.5);
            step(&mut rt, 90, &InputFrame::default());
            assert!(rt.character_state("body").unwrap().grounded);
            step(
                &mut rt,
                30,
                &InputFrame {
                    movement: [1., 0.],
                    ..Default::default()
                },
            );
            assert!(rt.character_state("body").unwrap().position.x > 1.);
            rt.stop();
            assert_eq!(p, original);
        }
    }
}
#[test]
fn crouching_preserves_feet_and_sphere_does_not_flatten() {
    for shape in shapes() {
        let mut body = MovementBody {
            standing: shape.clone(),
            ..Default::default()
        };
        if matches!(shape, CollisionShape::Convex { .. }) {
            body.crouched = Some(convex([0.6, 0.9, 0.6]));
        }
        let p = project(
            shape,
            CollisionShape::Box {
                size: [100., 1., 100.],
            },
        );
        let mut p = p;
        p.scenes[0]
            .entity_mut("body")
            .unwrap()
            .character3d
            .as_mut()
            .unwrap()
            .body = body.clone();
        let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
        step(&mut rt, 30, &InputFrame::default());
        let before = rt.character_state("body").unwrap().position;
        rt.request_crouch("body", true).unwrap();
        step(&mut rt, 1, &InputFrame::default());
        let state = rt.character_state("body").unwrap();
        assert_eq!(state.posture, Posture::Crouched);
        assert!(
            state.position.abs_diff_eq(before, 0.005),
            "shape={:?} before={before:?} after={:?}",
            body.standing,
            state.position
        );
        let used = rt
            .physics_world()
            .unwrap()
            .debug_shapes()
            .find(|b| b.id == "body")
            .unwrap();
        let min = used
            .geometry
            .vertices
            .iter()
            .map(|v| used.position.y + v[1])
            .fold(f32::INFINITY, f32::min);
        assert!((min - state.position.y).abs() < 1e-4);
        if matches!(body.standing, CollisionShape::Sphere { .. }) {
            assert_eq!(body.shape(false, 1.), body.shape(true, 1.));
            assert!((state.height - 1.).abs() < 1e-5);
        }
    }
}
#[test]
fn box_yaw_changes_real_sweep() {
    let mut world = PhysicsWorld::new();
    world
        .upsert(
            "wall",
            &Collider3d {
                shape: CollisionShape::Box {
                    size: [1., 10., 100.],
                },
                ..Default::default()
            },
            Vec3::X * 3.,
            Quat::IDENTITY,
            Vec3::ONE,
        )
        .unwrap();
    world.flush();
    for (yaw, expected) in [(0., 1.49), (std::f32::consts::FRAC_PI_2, 2.24)] {
        let motion = ShapeMotion::new(
            &CollisionShape::Box {
                size: [2., 1., 0.5],
            },
            1.,
            Quat::from_rotation_y(yaw),
        )
        .unwrap();
        let result = world
            .move_body(
                Vec3::ZERO,
                Vec3::X * 5.,
                FIXED_DT,
                &motion,
                &Default::default(),
            )
            .unwrap();
        assert!(
            (result.delta.x - expected).abs() < 0.02,
            "{yaw}: {:?}",
            result.delta
        );
    }
}
#[test]
fn capsule_yaw_keeps_analytic_height_and_unrotated_query_path() {
    let shape = CollisionShape::Capsule {
        height: 1.8,
        radius: 0.3,
    };
    for scale in [0.7, 1., 2.] {
        for yaw in [-3., -0.72, 0., 1.3, 3.] {
            let mut motion = ShapeMotion::new(&shape, scale, Quat::from_rotation_y(yaw)).unwrap();
            assert_eq!(motion.rotation, Quat::IDENTITY);
            assert_eq!(motion.height.to_bits(), (1.8_f32 * scale).to_bits());
            assert_eq!(motion.center.y.to_bits(), (1.8_f32 * scale * 0.5).to_bits());
            motion.set_yaw(yaw + 0.5);
            assert_eq!(motion.rotation, Quat::IDENTITY);
        }
    }
}
#[test]
fn invalid_bodies_are_rejected_and_cache_is_not_serialized_or_logical_state() {
    let mut body = MovementBody::default();
    let saved = serde_json::to_value(&body).unwrap();
    body.prepare(false, 1., 1., 0.).unwrap();
    assert_eq!(saved, serde_json::to_value(&body).unwrap());
    let a = body.prepare(false, 1., 1., 0.).unwrap();
    let b = body.prepare(false, 1., 1., 1.).unwrap();
    assert!(std::ptr::eq(&*a.geometry, &*b.geometry));
    assert!(body.prepare(false, 1., -1., 0.).is_err());
    body.standing = CollisionShape::Box { size: [0., 1., 1.] };
    assert!(body.validate(1.).is_err());
    let mut piece = Entity::new("Plano", Some(Primitive::Plane));
    piece.dimensions = [1.; 3];
    assert!(MovementBody::convex_from(&piece).is_err());
    body.standing = generate_collider(&Entity::new("Cubo", Some(Primitive::Cube)), false)
        .unwrap()
        .shape;
    assert!(body.validate(1.).is_err());
}
#[test]
fn schema_three_migration_preserves_ids_feet_filters_and_requires_no_external_collider() {
    let mut p = project(
        shapes().remove(0),
        CollisionShape::Box {
            size: [100., 1., 100.],
        },
    );
    let surface = oxy_core::surface::SurfaceMaterial::preset(oxy_core::surface::SurfacePreset::Ice);
    let authored = &mut p.scenes[0]
        .entity_mut("body")
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .body;
    authored.filter = CollisionFilter {
        category: 8,
        mask: 42,
        blocks_character: false,
        blocks_camera: false,
    };
    authored.surface = Some(surface.id.clone());
    let source_body = authored.clone();
    p.surfaces.push(surface);
    let mut value = serde_json::to_value(&p).unwrap();
    value["schema_version"] = 3.into();
    let e = &mut value["scenes"][0]["entities"][1];
    e["character3d"].as_object_mut().unwrap().remove("body");
    e["physics3d"] = serde_json::to_value(Collider3d {
        shape: CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        center: [0., 0.9, 0.],
        filter: source_body.filter.clone(),
        surface: source_body.surface.clone(),
        ..Default::default()
    })
    .unwrap();
    let bytes = serde_json::to_vec(&value).unwrap();
    let (converted, version) = oxy_core::migration::read_report(&bytes).unwrap();
    assert_eq!(version, Some(3));
    assert_eq!(converted, p);
    let reopened = oxy_core::migration::read(&serde_json::to_vec(&converted).unwrap()).unwrap();
    assert_eq!(converted, reopened);
    let folder = std::env::temp_dir().join(format!("oxy-body-migration-{}", new_id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    std::fs::write(&path, &bytes).unwrap();
    let loaded = oxy_core::persistence::load_project(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    oxy_core::persistence::save_project(&path, &loaded).unwrap();
    assert_eq!(
        oxy_core::persistence::load_project(&path).unwrap(),
        converted
    );
    let backup = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("backup.json"))
        .unwrap();
    assert_eq!(std::fs::read(backup).unwrap(), bytes);
    std::fs::remove_dir_all(folder).unwrap();
    value["scenes"][0]["entities"][1]["physics3d"]["center"][0] = 0.2.into();
    assert!(
        oxy_core::migration::read(&serde_json::to_vec(&value).unwrap())
            .unwrap_err()
            .contains("origem nos pés")
    );
    assert_eq!(oxy_core::migration::read(&bytes).unwrap(), converted);
    value["scenes"][0]["entities"][1]["physics3d"]["center"][0] = 0.0.into();
    value["scenes"][0]["entities"][1]["physics3d"]["enabled"] = false.into();
    value["scenes"][0]["entities"][1]["character3d"]["enabled"] = false.into();
    let disabled = oxy_core::migration::read(&serde_json::to_vec(&value).unwrap()).unwrap();
    let config = disabled.scenes[0]
        .entity("body")
        .unwrap()
        .character3d
        .as_ref()
        .unwrap();
    assert!(!config.enabled && !config.body.enabled);
    assert_eq!(config.body.filter, source_body.filter);
    assert_eq!(config.body.surface, source_body.surface);
}

#[test]
fn asymmetric_body_teleport_rotation_and_runtime_resize_check_the_actual_volume_atomically() {
    let mut p = project(
        CollisionShape::Box {
            size: [2., 1., 0.5],
        },
        CollisionShape::Box {
            size: [100., 1., 100.],
        },
    );
    let mut wall = Entity::new("Parede", None);
    wall.id = "wall".into();
    wall.transform.position = [3., 2., 0.];
    wall.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [1., 4., 100.],
        },
        ..Default::default()
    });
    p.scenes[0].entities.push(wall);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    let destination = Vec3::new(2., 0.02, 0.);
    let before = rt.scene().clone();
    assert!(
        rt.teleport_character("body", destination, TeleportOptions::default())
            .is_err()
    );
    assert_eq!(rt.scene(), &before);
    rt.teleport_character(
        "body",
        destination,
        TeleportOptions {
            yaw: Some(std::f32::consts::FRAC_PI_2),
            ..Default::default()
        },
    )
    .unwrap();
    let before = rt.scene().clone();
    assert!(rt.set_character_yaw("body", 0.).is_err());
    assert_eq!(rt.scene(), &before);
    let mut config = rt
        .scene()
        .entity("body")
        .unwrap()
        .character3d
        .clone()
        .unwrap();
    config.body.standing = CollisionShape::Box { size: [2., 1., 2.] };
    assert!(
        rt.configure_character("body", config.clone(), false)
            .is_err()
    );
    assert_eq!(rt.scene(), &before);
    config.body.standing = CollisionShape::Sphere { radius: 0.4 };
    rt.configure_character("body", config, false).unwrap();
    assert!((rt.character_state("body").unwrap().height - 0.8).abs() < 1e-5);
    assert_eq!(rt.character_state("body").unwrap().position, destination);
}

#[test]
fn each_shape_crosses_sensors_once_and_rides_a_platform_without_double_velocity() {
    use oxy_core::surface::{PlatformMode, TranslationPlatform};
    for shape in shapes() {
        let mut p = project(
            shape.clone(),
            CollisionShape::Box {
                size: [100., 1., 100.],
            },
        );
        let floor = p.scenes[0].entity_mut("floor").unwrap();
        floor.platform = Some(TranslationPlatform {
            mode: PlatformMode::Velocity,
            velocity: [2., 0., 0.],
            ..Default::default()
        });
        let body = p.scenes[0].entity_mut("body").unwrap();
        body.transform.position[1] = 1.;
        let c = body.character3d.as_mut().unwrap();
        c.ground_acceleration = 0.;
        c.ground_braking = 0.;
        c.ground_friction = 0.;
        c.air_acceleration = 0.;
        c.air_resistance = 0.;
        let mut sensor = Entity::new("Área", None);
        sensor.id = "sensor".into();
        sensor.transform.position = [4., 1., 0.];
        sensor.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: [0.05, 4., 4.],
            },
            sensor: true,
            ..Default::default()
        });
        p.scenes[0].entities.push(sensor);
        let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
        rt.set_character_velocity("body", Vec3::X * 2.).unwrap();
        let mut entered = 0;
        let mut exited = 0;
        let mut landed = false;
        for _ in 0..240 {
            step(&mut rt, 1, &InputFrame::default());
            let state = rt.character_state("body").unwrap();
            if state.grounded {
                landed = true;
                assert!(
                    (state.total_velocity().x - 2.).abs() < 0.005,
                    "{shape:?}: {:?}",
                    state.total_velocity()
                );
            }
            for event in rt.sensor_events().iter().filter(|e| e.area == "sensor") {
                if event.entered {
                    entered += 1
                } else {
                    exited += 1
                }
            }
        }
        assert!(landed);
        assert_eq!((entered, exited), (1, 1), "{shape:?}");
    }
}
#[test]
fn standing_shape_clearance_blocks_every_alternate_under_a_ceiling() {
    for shape in shapes() {
        let mut p = project(
            shape.clone(),
            CollisionShape::Box {
                size: [100., 1., 100.],
            },
        );
        let c = p.scenes[0]
            .entity_mut("body")
            .unwrap()
            .character3d
            .as_mut()
            .unwrap();
        match shape {
            CollisionShape::Sphere { .. } => {
                c.body.crouched = Some(CollisionShape::Sphere { radius: 0.25 })
            }
            CollisionShape::Convex { .. } => c.body.crouched = Some(convex([0.6, 0.8, 0.6])),
            _ => {}
        }
        let ceiling_y = if matches!(shape, CollisionShape::Sphere { .. }) {
            0.85
        } else {
            1.35
        };
        let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
        step(&mut rt, 30, &InputFrame::default());
        rt.request_crouch("body", true).unwrap();
        step(&mut rt, 1, &InputFrame::default());
        let mut ceiling = Entity::new("Teto", None);
        ceiling.id = "ceiling".into();
        ceiling.transform.position = [0., ceiling_y, 0.];
        ceiling.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: [4., 0.2, 4.],
            },
            ..Default::default()
        });
        rt.scene_mut().entities.push(ceiling);
        rt.request_crouch("body", false).unwrap();
        step(&mut rt, 3, &InputFrame::default());
        assert_eq!(
            rt.character_state("body").unwrap().posture,
            Posture::Crouched,
            "{shape:?}"
        );
        rt.scene_mut()
            .entity_mut("ceiling")
            .unwrap()
            .transform
            .position[0] = 10.;
        step(&mut rt, 3, &InputFrame::default());
        assert_eq!(
            rt.character_state("body").unwrap().posture,
            Posture::Standing
        );
    }
}
#[test]
fn each_shape_climbs_a_supported_step_and_ramp() {
    for shape in shapes() {
        for ramp in [false, true] {
            let mut p = project(
                shape.clone(),
                CollisionShape::Box {
                    size: [100., 1., 100.],
                },
            );
            let mut obstacle = Entity::new("Subida", None);
            obstacle.id = "obstacle".into();
            if ramp {
                obstacle.transform.position = [3., 0.4, 0.];
                obstacle.transform.rotation = [0., 0., 15_f32.to_radians()];
                obstacle.physics3d = Some(Collider3d {
                    shape: CollisionShape::Box {
                        size: [4., 0.2, 4.],
                    },
                    ..Default::default()
                });
            } else {
                obstacle.transform.position = [2., 0.1, 0.];
                obstacle.physics3d = Some(Collider3d {
                    shape: CollisionShape::Box {
                        size: [2., 0.2, 4.],
                    },
                    ..Default::default()
                });
            }
            p.scenes[0].entities.push(obstacle);
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            step(&mut rt, 30, &InputFrame::default());
            let mut max_y = 0_f32;
            for _ in 0..65 {
                step(
                    &mut rt,
                    1,
                    &InputFrame {
                        movement: [1., 0.],
                        ..Default::default()
                    },
                );
                max_y = max_y.max(rt.character_state("body").unwrap().position.y);
            }
            assert!(
                max_y > if ramp { 0.6 } else { 0.17 },
                "{shape:?}, ramp={ramp}: max_y={max_y}, {:?}",
                rt.character_state("body")
            );
            assert!(
                rt.character_state("body").unwrap().position.x > 3.5,
                "{shape:?}, ramp={ramp}"
            );
        }
    }
}
