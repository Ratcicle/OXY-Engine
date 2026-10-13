use glam::{Quat, Vec3};
use oxy_core::{
    document::{Entity, Primitive},
    physics3d::*,
};
use std::sync::Arc;

fn box_at(world: &mut PhysicsWorld, id: &str, center: Vec3, size: [f32; 3]) {
    world
        .upsert(
            id,
            &Collider3d {
                shape: CollisionShape::Box { size },
                ..Default::default()
            },
            center,
            Quat::IDENTITY,
            Vec3::ONE,
        )
        .unwrap();
    world.flush();
}
fn floor(world: &mut PhysicsWorld) {
    box_at(world, "floor", Vec3::new(0., -0.5, 0.), [100., 1., 100.]);
}

#[test]
fn invalid_query_pose_or_scaled_geometry_preserves_previous_collider() {
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    let before = world.counters().shapes_prepared;
    let config = Collider3d {
        shape: CollisionShape::Box {
            size: [f32::MAX; 3],
        },
        ..Default::default()
    };
    assert!(
        world
            .upsert(
                "floor",
                &config,
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::splat(2.)
            )
            .is_err()
    );
    assert!(
        world
            .upsert(
                "floor",
                &Collider3d::default(),
                Vec3::ZERO,
                Quat::from_xyzw(0., 0., 0., 2.),
                Vec3::ONE
            )
            .is_err()
    );
    assert_eq!(world.counters().shapes_prepared, before);
    assert!(
        (world
            .ray(Vec3::Y, -Vec3::Y, 10., &QueryOptions::default())
            .unwrap()
            .unwrap()
            .distance
            - 1.)
            .abs()
            < 0.001
    );
}

#[test]
fn capsule_stops_at_thin_wall_without_losing_tangential_motion() {
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    box_at(&mut world, "wall", Vec3::new(2., 2., 0.), [0.02, 4., 100.]);
    let r = world
        .move_capsule(
            Vec3::new(0., 0.02, 0.),
            Vec3::new(12., -0.02, 1.),
            1. / 60.,
            CapsuleMotion::default(),
            &QueryOptions::default(),
        )
        .unwrap();
    assert!(r.delta.x < 1.7 && r.delta.x > 1.5, "{r:?}");
    assert!(r.delta.z > 0.9, "{r:?}");
    assert!(
        r.contacts
            .iter()
            .any(|c| c.object == "wall" && c.normal.x < -0.99)
    );
    assert!(r.grounded);
}

#[test]
fn actual_convex_ramp_has_sloped_normal_and_can_be_climbed() {
    let mut world = PhysicsWorld::new();
    floor(&mut world);
    let geometry = Arc::new(CollisionGeometry {
        vertices: vec![
            [0., 0., -2.],
            [4., 0., -2.],
            [4., 2., -2.],
            [0., 0., 2.],
            [4., 0., 2.],
            [4., 2., 2.],
        ],
        triangles: vec![
            [0, 2, 1],
            [3, 4, 5],
            [0, 3, 5],
            [0, 5, 2],
            [1, 2, 5],
            [1, 5, 4],
            [0, 1, 4],
            [0, 4, 3],
        ],
        source_fingerprint: None,
    });
    world
        .upsert(
            "ramp",
            &Collider3d {
                shape: CollisionShape::Convex { geometry },
                ..Default::default()
            },
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::ONE,
        )
        .unwrap();
    world.flush();
    let hit = world
        .ray(
            Vec3::new(2., 5., 0.),
            -Vec3::Y,
            10.,
            &QueryOptions::default(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.object, "ramp");
    assert!(
        hit.normal
            .abs_diff_eq(Vec3::new(-0.5, 1., 0.).normalize(), 0.002),
        "{hit:?}"
    );
    let mut feet = Vec3::new(-1., 0.02, 0.);
    for _ in 0..180 {
        feet += world
            .move_capsule(
                feet,
                Vec3::new(0.02, -0.01, 0.),
                1. / 60.,
                CapsuleMotion::default(),
                &QueryOptions::default(),
            )
            .unwrap()
            .delta;
    }
    // Reference support height of the bottom hemisphere above y=x/2, including margin.
    // Desired downward motion projects on the slope; it is not free-space horizontal speed.
    let expected_height = feet.x * 0.5 + 0.3 * (1.25f32.sqrt() - 1.) + 0.01 * 1.25f32.sqrt();
    assert!(
        feet.x > 1. && (feet.y - expected_height).abs() < 0.004,
        "{feet:?}, expected y={expected_height}"
    );
}

#[test]
fn triangle_opening_is_not_filled_and_sphere_protects_camera_volume() {
    let mut world = PhysicsWorld::new();
    let geometry = Arc::new(CollisionGeometry {
        vertices: vec![
            [-3., -3., 0.],
            [-1., -3., 0.],
            [-1., 3., 0.],
            [-3., 3., 0.],
            [1., -3., 0.],
            [3., -3., 0.],
            [3., 3., 0.],
            [1., 3., 0.],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
        source_fingerprint: None,
    });
    world
        .upsert(
            "opening",
            &Collider3d {
                shape: CollisionShape::TriMesh { geometry },
                ..Default::default()
            },
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::ONE,
        )
        .unwrap();
    world.flush();
    assert!(
        world
            .ray(
                Vec3::new(0., 0., 2.),
                -Vec3::Z,
                4.,
                &QueryOptions::default()
            )
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .ray(
                Vec3::new(2., 0., 2.),
                -Vec3::Z,
                4.,
                &QueryOptions::default()
            )
            .unwrap()
            .is_some()
    );
    // A centre ray fits through the opening; a camera volume near its side does not.
    let hit = world
        .cast(
            &CollisionShape::Sphere { radius: 0.4 },
            Vec3::new(0.8, 0., 2.),
            Vec3::new(0., 0., -4.),
            &QueryOptions {
                camera: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.object, "opening");
    assert!(hit.fraction < 0.5);
}

#[test]
fn autostep_climbs_small_step_but_not_wall_or_blocked_headroom() {
    for (height, ceiling, expected) in [
        (0.18, false, true),
        (0.7, false, false),
        (0.18, true, false),
    ] {
        let mut world = PhysicsWorld::new();
        floor(&mut world);
        box_at(
            &mut world,
            "step",
            Vec3::new(1.5, height * 0.5, 0.),
            [1., height, 4.],
        );
        if ceiling {
            box_at(&mut world, "ceiling", Vec3::new(1.5, 2., 0.), [2., 0.2, 4.]);
        }
        let mut feet = Vec3::new(0., 0.02, 0.);
        for _ in 0..100 {
            feet += world
                .move_capsule(
                    feet,
                    Vec3::new(0.02, -0.01, 0.),
                    1. / 60.,
                    CapsuleMotion::default(),
                    &QueryOptions::default(),
                )
                .unwrap()
                .delta;
        }
        assert_eq!(
            feet.x > 1.2 && feet.y > height - 0.02,
            expected,
            "height={height}, ceiling={ceiling}, feet={feet:?}"
        );
    }
}

#[test]
fn cache_filters_removal_and_same_count_replacement_preserve_identity() {
    let mut world = PhysicsWorld::new();
    let mut config = Collider3d::default();
    world
        .upsert("old", &config, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE)
        .unwrap();
    world.flush();
    let before = world.counters();
    for i in 1..20 {
        world
            .upsert(
                "old",
                &config,
                Vec3::new(i as f32 * 0.01, 0., 0.),
                Quat::IDENTITY,
                Vec3::ONE,
            )
            .unwrap();
        world.flush();
    }
    assert_eq!(world.counters().shapes_prepared, before.shapes_prepared);
    assert!(world.counters().poses_updated > 0);
    config.filter.blocks_camera = false;
    world
        .upsert("old", &config, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE)
        .unwrap();
    world.flush();
    assert!(
        world
            .ray(
                Vec3::Z * 2.,
                -Vec3::Z,
                4.,
                &QueryOptions {
                    camera: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .is_none()
    );
    assert!(
        world
            .ray(Vec3::Z * 2., -Vec3::Z, 4., &QueryOptions::default())
            .unwrap()
            .is_some()
    );
    world.remove("old");
    box_at(&mut world, "new", Vec3::ZERO, [1.; 3]);
    assert_eq!(
        world
            .ray(Vec3::Z * 2., -Vec3::Z, 4., &QueryOptions::default())
            .unwrap()
            .unwrap()
            .object,
        "new"
    );
    assert!(
        world
            .ray(Vec3::Z * 2., -Vec3::Z, 4., &QueryOptions::excluding("new"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn invalid_and_mirrored_shapes_do_not_mutate_prior_body() {
    let mut world = PhysicsWorld::new();
    box_at(&mut world, "box", Vec3::ZERO, [2.; 3]);
    let invalid = Collider3d {
        shape: CollisionShape::Capsule {
            height: 0.2,
            radius: 0.3,
        },
        ..Default::default()
    };
    assert!(
        world
            .upsert("box", &invalid, Vec3::ZERO, Quat::IDENTITY, Vec3::ONE)
            .is_err()
    );
    assert_eq!(
        world
            .ray(Vec3::Z * 3., -Vec3::Z, 5., &QueryOptions::default())
            .unwrap()
            .unwrap()
            .distance,
        2.
    );
    let capsule = Collider3d {
        shape: CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        ..Default::default()
    };
    assert!(
        world
            .upsert(
                "capsule",
                &capsule,
                Vec3::ZERO,
                Quat::IDENTITY,
                Vec3::new(1., 2., 1.)
            )
            .is_err()
    );
    world
        .upsert(
            "box",
            &Collider3d {
                shape: CollisionShape::Box { size: [2.; 3] },
                ..Default::default()
            },
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::new(-1., 1., 1.),
        )
        .unwrap();
    world.flush();
    assert_eq!(
        world
            .ray(Vec3::Z * 3., -Vec3::Z, 5., &QueryOptions::default())
            .unwrap()
            .unwrap()
            .distance,
        2.
    );
}

/// A floor as a box, the hull of that box or a plane of two triangles, `size` wide with its top
/// at y = 0 and turned `degrees` about the vertical through its middle.
fn turned_floor(kind: &str, size: f32, degrees: f32) -> PhysicsWorld {
    let mut entity = Entity::new("Chão", Some(Primitive::Cube));
    entity.dimensions = [size, 1., size];
    let mut height = -0.5;
    let config = match kind {
        "box" => Collider3d {
            shape: CollisionShape::Box {
                size: entity.dimensions,
            },
            ..Default::default()
        },
        "hull" => generate_collider(&entity, true).unwrap(),
        _ => {
            entity.primitive = Some(Primitive::Plane);
            height = 0.;
            generate_collider(&entity, false).unwrap()
        }
    };
    let mut world = PhysicsWorld::new();
    world
        .upsert(
            "floor",
            &config,
            Vec3::Y * height,
            Quat::from_rotation_y(degrees.to_radians()),
            Vec3::ONE,
        )
        .unwrap();
    world.flush();
    world
}

#[test]
fn casts_over_the_middle_of_a_large_turned_floor_keep_the_margin() {
    let config = CapsuleMotion::default();
    let capsule = CollisionShape::Capsule {
        height: config.height,
        radius: config.radius,
    };
    let gravity = Vec3::new(0., -22. / 3600., 0.);
    for kind in ["box", "hull", "plane"] {
        for size in [6., 20., 100.] {
            for degrees in [0f32, 13.5, 30., 45., 77.] {
                let world = turned_floor(kind, size, degrees);
                let turn = Quat::from_rotation_y(degrees.to_radians());
                // The middle, a millimetre from it, a point on a diagonal and one off them.
                for spot in [
                    Vec3::ZERO,
                    Vec3::new(0.001, 0., 0.),
                    turn * Vec3::new(0.5, 0., 0.5),
                    turn * Vec3::new(0.7, 0., -0.2),
                ] {
                    // Starting inside the margin, on it or just above it, gravity moves the
                    // capsule no closer than the margin or than it was; it does not slide.
                    for gap in [0.003, 0.0099, 0.01, 0.0101, 0.02] {
                        let case = format!("{kind} {size} m, {degrees}°, at {spot}, gap {gap}");
                        let feet = spot.with_y(gap);
                        let r = world
                            .move_capsule(feet, gravity, 1. / 60., config, &QueryOptions::default())
                            .unwrap();
                        let end = feet + r.delta;
                        assert!(
                            end.y >= gap.min(config.margin) - 2e-4,
                            "{case}: {end} {r:?}"
                        );
                        assert!(end.with_y(0.).distance(spot) < 1e-4, "{case}: {end} {r:?}");
                        assert!(r.grounded, "{case}: {r:?}");
                    }
                    // A cast from 3.5 cm above stops where the capsule touches, on the face (the
                    // edge between the plane's triangles may tilt the normal a little).
                    let case = format!("{kind} {size} m, {degrees}°, at {spot}");
                    let hit = world
                        .cast(
                            &capsule,
                            spot.with_y(config.height * 0.5 + 0.035),
                            Vec3::new(0., -0.1, 0.),
                            &QueryOptions::default(),
                        )
                        .unwrap()
                        .unwrap_or_else(|| panic!("{case}: missed"));
                    assert!((hit.distance - 0.035).abs() < 1e-4, "{case}: {hit:?}");
                    assert!(hit.normal.y > 0.99, "{case}: {hit:?}");
                }
            }
        }
    }
}

#[test]
fn a_capsule_deep_in_a_turned_floor_gets_out_through_the_top() {
    let config = CapsuleMotion::default();
    let capsule = CollisionShape::Capsule {
        height: config.height,
        radius: config.radius,
    };
    for kind in ["box", "hull"] {
        for size in [6., 20., 100.] {
            for degrees in [0f32, 13.5, 45.] {
                let world = turned_floor(kind, size, degrees);
                let turn = Quat::from_rotation_y(degrees.to_radians());
                for spot in [Vec3::ZERO, turn * Vec3::new(0.7, 0., -0.2)] {
                    // The floor is a metre thick: the nearest way out is always up.
                    for depth in [0.003, 0.03, 0.3, 0.8, 0.95] {
                        let case = format!("{kind} {size} m, {degrees}°, at {spot}, {depth} deep");
                        let feet = spot.with_y(-depth);
                        let centre = feet + Vec3::Y * config.height * 0.5;
                        let contacts = world
                            .penetration_contacts(&capsule, centre, &QueryOptions::default())
                            .unwrap();
                        assert_eq!(contacts.len(), 1, "{case}: {contacts:?}");
                        assert!(contacts[0].normal.y > 0.999, "{case}: {contacts:?}");
                        assert!(
                            (contacts[0].distance + depth).abs() < 1e-3,
                            "{case}: {contacts:?}"
                        );
                        let hit = world
                            .cast(
                                &capsule,
                                centre,
                                Vec3::new(0., -0.1, 0.),
                                &QueryOptions::default(),
                            )
                            .unwrap()
                            .unwrap_or_else(|| panic!("{case}: missed"));
                        assert!(hit.normal.y > 0.999, "{case}: {hit:?}");
                        let r = world
                            .move_capsule(
                                feet,
                                Vec3::ZERO,
                                1. / 60.,
                                config,
                                &QueryOptions::default(),
                            )
                            .unwrap();
                        assert!(r.delta.y > 0., "{case}: {r:?}");
                        assert!(r.delta.with_y(0.).length() < 1e-4, "{case}: {r:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn standing_near_the_edges_and_corners_of_a_turned_floor_keeps_the_margin() {
    let config = CapsuleMotion::default();
    let gravity = Vec3::new(0., -22. / 3600., 0.);
    for kind in ["box", "hull", "plane"] {
        for size in [6f32, 20.] {
            for degrees in [0f32, 13.5, 30., 45., 77.] {
                let world = turned_floor(kind, size, degrees);
                let turn = Quat::from_rotation_y(degrees.to_radians());
                let half = size * 0.5;
                // Toward three edges and into a corner, a tenth of a millimetre at a time,
                // where the floor ends within the capsule's reach.
                for (toward, corner) in [
                    (Vec3::X, false),
                    (Vec3::Z, false),
                    (-Vec3::Z, false),
                    (-Vec3::X - Vec3::Z, true),
                ] {
                    for step in 0..=4000 {
                        let edge = 0.25 + step as f32 * 1e-4;
                        let local = if corner {
                            toward * (half - edge)
                        } else {
                            toward * (half - edge) + Vec3::new(toward.z, 0., toward.x) * 0.37
                        };
                        let spot = turn * local;
                        let gap = [0.0099, 0.0101][step % 2];
                        let case = format!("{kind} {size} m, {degrees}°, at {spot}, gap {gap}");
                        let feet = spot.with_y(gap);
                        let r = world
                            .move_capsule(feet, gravity, 1. / 60., config, &QueryOptions::default())
                            .unwrap();
                        let end = feet + r.delta;
                        assert!(
                            end.y >= gap.min(config.margin) - 2e-4,
                            "{case}: {end} {r:?}"
                        );
                        assert!(end.with_y(0.).distance(spot) < 1e-4, "{case}: {end} {r:?}");
                        assert!(r.grounded, "{case}: {r:?}");
                    }
                }
            }
        }
    }
}
