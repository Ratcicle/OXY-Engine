use super::*;
fn offset(p: &mut Project, value: Vec3) {
    p.scenes[0]
        .entity_mut("body")
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .body
        .offset = value.to_array();
}
fn obstacle(p: &mut Project, id: &str, at: [f32; 3], size: [f32; 3]) {
    let mut e = Entity::new(id, None);
    e.id = id.into();
    e.transform.position = at;
    e.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size },
        ..Default::default()
    });
    p.scenes[0].entities.push(e);
}
#[test]
fn old_schema_defaults_to_zero_and_offset_roundtrips_without_moving_visuals() {
    let mut p = project(
        shapes().remove(0),
        CollisionShape::Box {
            size: [100., 1., 100.],
        },
    );
    let mut json = serde_json::to_value(&p).unwrap();
    json["scenes"][0]["entities"][1]["character3d"]["body"]
        .as_object_mut()
        .unwrap()
        .remove("offset");
    assert_eq!(
        oxy_core::migration::read(&serde_json::to_vec(&json).unwrap()).unwrap(),
        p
    );
    let transform = p.scenes[0].entity("body").unwrap().transform.clone();
    let id = p.scenes[0].entity("body").unwrap().id.clone();
    offset(&mut p, Vec3::new(1., 0.5, -2.));
    assert_eq!(p.scenes[0].entity(&id).unwrap().transform, transform);
    let path = std::env::temp_dir().join(format!("oxy-offset-{}.json", new_id()));
    oxy_core::persistence::save_project(&path, &p).unwrap();
    assert_eq!(oxy_core::persistence::load_project(&path).unwrap(), p);
    std::fs::remove_file(path).unwrap();
    offset(&mut p, Vec3::new(f32::NAN, 0., 0.));
    assert!(validate_project(&p).is_err());
}
#[test]
fn all_shapes_rotate_scale_offset_and_match_physics_debug_without_moving_origin() {
    for shape in shapes() {
        for yaw in [0., 0.7, -1.5] {
            for scale in [0.5, 2.] {
                let mut b = MovementBody {
                    standing: shape.clone(),
                    ..Default::default()
                };
                let zero = b.prepare(false, 1., scale, yaw).unwrap();
                b.offset = [1., 0.5, -2.];
                for posture in [false, true] {
                    let motion = b.prepare(posture, 1., scale, yaw).unwrap();
                    let base = MovementBody {
                        standing: shape.clone(),
                        ..Default::default()
                    }
                    .prepare(posture, 1., scale, yaw)
                    .unwrap();
                    let feet = Vec3::new(3., 4., 5.);
                    let delta = Quat::from_rotation_y(yaw) * (Vec3::from(b.offset) * scale);
                    assert!((motion.at(feet) - base.at(feet)).abs_diff_eq(delta, 1e-5));
                    let mut world = PhysicsWorld::new();
                    world
                        .upsert(
                            "body",
                            &b.collider(posture, 1.).unwrap(),
                            feet,
                            Quat::from_rotation_y(yaw),
                            Vec3::splat(scale),
                        )
                        .unwrap();
                    assert!(
                        world
                            .debug_shapes()
                            .next()
                            .unwrap()
                            .position
                            .abs_diff_eq(motion.at(feet), 1e-5)
                    );
                }
                let mut later = b.prepare(false, 1., scale, 0.).unwrap();
                later.set_yaw(yaw);
                assert!(later.at(Vec3::ZERO).abs_diff_eq(
                    b.prepare(false, 1., scale, yaw).unwrap().at(Vec3::ZERO),
                    1e-5
                ));
                assert_eq!(
                    zero.height,
                    b.prepare(false, 1., scale, yaw).unwrap().height
                );
            }
        }
    }
}
#[test]
fn offset_solver_is_equivalent_to_translated_feet_for_walls_steps_ramps_ceiling_and_recovery() {
    // Independent reference: the unchanged zero-offset solver positioned at the physical feet.
    let shift = Vec3::new(1., 0.5, -2.);
    for shape in shapes() {
        for kind in 0..3 {
            let ground = match kind {
                0 => CollisionShape::Box {
                    size: [100., 1., 100.],
                },
                1 => convex([100., 1., 100.]),
                _ => {
                    let mut g = Entity::new("mesh", Some(Primitive::Cube));
                    g.dimensions = [100., 1., 100.];
                    generate_collider(&g, false).unwrap().shape
                }
            };
            let mut p = project(shape.clone(), ground);
            p.scenes[0]
                .entity_mut("body")
                .unwrap()
                .character3d
                .as_mut()
                .unwrap()
                .angular_speed = 0.;
            obstacle(&mut p, "step", [1., 0.075, 0.], [0.5, 0.15, 4.]);
            obstacle(&mut p, "ramp", [3., 0.05, 0.], [2., 0.2, 4.]);
            p.scenes[0].entity_mut("ramp").unwrap().transform.rotation = [0., 0., 0.15];
            obstacle(&mut p, "wall", [5., 2., 0.], [0.2, 4., 4.]);
            obstacle(&mut p, "ceiling", [0., 2.5, 0.], [2., 0.2, 4.]);
            let mut world = PhysicsWorld::new();
            world.sync_scene(&p.scenes[0], false).unwrap();
            let mut translated = MovementBody {
                standing: shape.clone(),
                offset: shift.to_array(),
                ..Default::default()
            }
            .prepare(false, 1., 1., 0.)
            .unwrap();
            let mut reference = MovementBody {
                standing: shape.clone(),
                ..Default::default()
            }
            .prepare(false, 1., 1., 0.)
            .unwrap();
            translated.step_height = 0.25;
            reference.step_height = 0.25;
            let options = QueryOptions::excluding("body");
            // Compare the same physical input to the original zero-offset query,
            // avoiding cumulative floating-point drift from subtracting logical origins.
            for (physical, desired) in [
                (Vec3::new(0., 0.125, 0.), Vec3::new(0., -0.5, 0.)),
                (Vec3::new(0.25, 0.01, 0.), Vec3::new(1., -0.02, 0.)),
                (Vec3::new(2., 0.25, 0.), Vec3::new(1., -0.2, 0.)),
                (Vec3::new(4., 0.125, 0.), Vec3::new(2., 0., 0.)),
                (Vec3::new(0., 0.5, 0.), Vec3::new(0., 2., 0.)),
            ] {
                let feet = physical - shift;
                let reference_feet = translated.at(feet) - reference.center;
                let a = world
                    .move_body(feet, desired, FIXED_DT, &translated, &options)
                    .unwrap();
                let b = world
                    .move_body(reference_feet, desired, FIXED_DT, &reference, &options)
                    .unwrap();
                assert!(
                    a.delta.abs_diff_eq(b.delta, 0.0001),
                    "{shape:?} / {kind}: {:?} vs {:?}",
                    a.delta,
                    b.delta
                );
                assert_eq!(a.grounded, b.grounded);
            }
            // Negative offset begins inside ground: recovery follows the same physical origin.
            let motion = MovementBody {
                standing: shape.clone(),
                offset: [0., -0.2, 0.],
                ..Default::default()
            }
            .prepare(false, 1., 1., 0.)
            .unwrap();
            let zero = MovementBody {
                standing: shape.clone(),
                ..Default::default()
            }
            .prepare(false, 1., 1., 0.)
            .unwrap();
            let mut world = PhysicsWorld::new();
            world.sync_scene(&p.scenes[0], false).unwrap();
            let options = QueryOptions::excluding("body");
            let x = world
                .move_body(
                    Vec3::new(0., 0.02, 0.),
                    Vec3::ZERO,
                    FIXED_DT,
                    &motion,
                    &options,
                )
                .unwrap();
            let y = world
                .move_body(
                    Vec3::new(0., -0.18, 0.),
                    Vec3::ZERO,
                    FIXED_DT,
                    &zero,
                    &options,
                )
                .unwrap();
            assert!(x.delta.abs_diff_eq(y.delta, 1e-5));
        }
    }
}
#[test]
fn displaced_postures_keep_logical_feet_and_refuse_blocked_shapes() {
    for shape in shapes() {
        let mut p = project(
            shape.clone(),
            CollisionShape::Box {
                size: [100., 1., 100.],
            },
        );
        offset(&mut p, Vec3::new(3., 0.5, 0.));
        p.scenes[0].entity_mut("body").unwrap().transform.position[1] -= 0.5;
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
        let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
        step(&mut rt, 20, &InputFrame::default());
        let feet = rt.character_state("body").unwrap().position;
        rt.request_crouch("body", true).unwrap();
        step(&mut rt, 1, &InputFrame::default());
        assert_eq!(
            rt.character_state("body").unwrap().posture,
            Posture::Crouched
        );
        assert!(
            rt.character_state("body")
                .unwrap()
                .position
                .abs_diff_eq(feet, 0.015)
        );
        let y = if matches!(shape, CollisionShape::Sphere { .. }) {
            0.85
        } else {
            1.35
        };
        let mut scene_project = p.clone();
        obstacle(&mut scene_project, "ceiling", [3., y, 0.], [1., 0.2, 1.]);
        rt.scene_mut()
            .entities
            .push(scene_project.scenes[0].entity("ceiling").unwrap().clone());
        rt.request_crouch("body", false).unwrap();
        step(&mut rt, 1, &InputFrame::default());
        assert_eq!(
            rt.character_state("body").unwrap().posture,
            Posture::Crouched
        );
    }
    let mut p = project(
        CollisionShape::Box {
            size: [0.4, 1.8, 0.4],
        },
        CollisionShape::Box {
            size: [100., 1., 100.],
        },
    );
    offset(&mut p, Vec3::X * 3.);
    p.scenes[0]
        .entity_mut("body")
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .body
        .crouched = Some(CollisionShape::Box {
        size: [2., 0.8, 0.4],
    });
    obstacle(&mut p, "side", [3.7, 0.5, 0.], [0.2, 1., 2.]);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    step(&mut rt, 20, &InputFrame::default());
    let feet = rt.character_state("body").unwrap().position;
    rt.request_crouch("body", true).unwrap();
    step(&mut rt, 1, &InputFrame::default());
    assert_eq!(
        rt.character_state("body").unwrap().posture,
        Posture::Standing
    );
    assert!(
        rt.character_state("body")
            .unwrap()
            .position
            .abs_diff_eq(feet, 1e-4)
    );
}

#[test]
fn offset_platform_conveyor_and_swept_sensor_use_body_instead_of_logical_origin() {
    use oxy_core::surface::*;
    for shape in shapes() {
        for belt in [false, true] {
            let mut p = project(
                shape.clone(),
                CollisionShape::Box {
                    size: [100., 1., 100.],
                },
            );
            let shift = Vec3::new(0., 0.5, 5.);
            offset(&mut p, shift);
            let e = p.scenes[0].entity_mut("body").unwrap();
            e.transform.position = [0., 0.5, -5.];
            let c = e.character3d.as_mut().unwrap();
            c.ground_acceleration = 0.;
            c.ground_braking = 0.;
            c.ground_friction = 0.;
            c.air_acceleration = 0.;
            if belt {
                let mut surface = SurfaceMaterial::preset(SurfacePreset::Conveyor);
                surface.conveyor = [2., 0., 0.];
                p.scenes[0]
                    .entity_mut("floor")
                    .unwrap()
                    .physics3d
                    .as_mut()
                    .unwrap()
                    .surface = Some(surface.id.clone());
                p.surfaces.push(surface);
            } else {
                p.scenes[0].entity_mut("floor").unwrap().platform = Some(TranslationPlatform {
                    mode: PlatformMode::Velocity,
                    velocity: [2., 0., 0.],
                    ..Default::default()
                });
            }
            obstacle(&mut p, "sensor", [4., 1., 0.], [0.05, 4., 2.]);
            p.scenes[0]
                .entity_mut("sensor")
                .unwrap()
                .physics3d
                .as_mut()
                .unwrap()
                .sensor = true;
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            rt.set_character_velocity("body", Vec3::X * 2.).unwrap();
            let mut counts = [0, 0];
            let mut grounded = false;
            for _ in 0..240 {
                step(&mut rt, 1, &InputFrame::default());
                let state = rt.character_state("body").unwrap();
                if state.grounded {
                    grounded = true;
                    assert!((state.total_velocity().x - 2.).abs() < 0.005);
                }
                assert!((state.position.z + 5.).abs() < 0.001);
                for event in rt.sensor_events().iter().filter(|e| e.area == "sensor") {
                    counts[usize::from(!event.entered)] += 1;
                }
            }
            assert!(grounded);
            assert_eq!(counts, [1, 1], "{shape:?} belt={belt}");
        }
    }
}

#[test]
fn teleport_and_yaw_keep_logical_destination_but_validate_displaced_geometry() {
    let mut p = project(
        shapes().remove(0),
        CollisionShape::Box {
            size: [100., 1., 100.],
        },
    );
    offset(&mut p, Vec3::X * 2.);
    obstacle(&mut p, "wall", [5., 1., 0.], [0.5, 2., 2.]);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    let before = rt.scene().clone();
    assert!(
        rt.teleport_character("body", Vec3::new(3., 0.02, 0.), TeleportOptions::default())
            .is_err()
    );
    assert_eq!(rt.scene(), &before);
    let logical = Vec3::new(3., 0.02, 0.);
    rt.teleport_character(
        "body",
        logical,
        TeleportOptions {
            yaw: Some(std::f32::consts::FRAC_PI_2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rt.character_state("body").unwrap().position, logical);
    let before = rt.scene().clone();
    assert!(rt.set_character_yaw("body", 0.).is_err());
    assert_eq!(rt.scene(), &before);
}
