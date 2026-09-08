use glam::Vec3;
use oxy_core::{
    broadphase::Candidates,
    collision::{Aabb, move_and_slide},
};
fn box_at(x: f32, y: f32, half: f32) -> Aabb {
    Aabb::from_center_half(Vec3::new(x, y, 0.), Vec3::splat(half))
}
#[test]
fn grid_matches_ordered_brute_force_motion_and_pairs() {
    let mut seed = 721_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((seed >> 32) as u32 as f32 / u32::MAX as f32 - 0.5) * 120.
    };
    for dimensions in [2, 3] {
        for dense in [false, true] {
            let mut boxes: Vec<_> = (0..240)
                .map(|i| {
                    box_at(
                        if dense {
                            (i % 4) as f32 * 0.1
                        } else {
                            random()
                        },
                        if dense { 0. } else { random() },
                        0.3,
                    )
                })
                .collect();
            boxes.push(box_at(-4., 4., 30.)); // Oversized collider: bounded extra list.
            let mut grid = Candidates::new(boxes.iter().copied().enumerate(), dimensions);
            for n in 0..300 {
                let body = box_at(random(), random(), 0.5);
                let velocity = Vec3::new(random() * 30., random() * 30., 0.);
                if n == 100 {
                    boxes[0] = body.translated(Vec3::X);
                    grid.changed(0);
                }
                let ids = grid.motion(body, velocity / 60., |i| {
                    body.overlaps(boxes[i], dimensions)
                });
                assert!(ids.windows(2).all(|w| w[0] < w[1]));
                let candidates: Vec<_> = ids.into_iter().map(|i| boxes[i]).collect();
                let expected = move_and_slide(body, velocity, 1. / 60., &boxes, dimensions);
                let actual = move_and_slide(body, velocity, 1. / 60., &candidates, dimensions);
                assert!((actual.delta - expected.delta).length() < 1e-5);
                assert_eq!(actual.velocity, expected.velocity);
                assert_eq!(actual.grounded, expected.grounded);
                let expected: Vec<_> = boxes
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| body.overlaps(**b, dimensions))
                    .map(|(i, _)| i)
                    .collect();
                let actual: Vec<_> = grid
                    .query(body)
                    .into_iter()
                    .filter(|i| body.overlaps(boxes[*i], dimensions))
                    .collect();
                assert_eq!(actual, expected);
            }
        }
    }
}
#[test]
fn boundaries_small_scenes_and_penetration_recovery_stay_conservative() {
    let mut boxes: Vec<_> = (0..100)
        .map(|i| box_at(i as f32 * 8. - 400., -4., 0.5))
        .collect();
    boxes[0] = box_at(0., 0., 0.5);
    boxes[1] = box_at(0., 1., 0.5);
    let grid = Candidates::new(boxes.iter().copied().enumerate(), 2);
    let body = box_at(0., 0.1, 0.5);
    assert_eq!(
        grid.motion(body, Vec3::ZERO, |i| body.overlaps(boxes[i], 2)),
        (0..100).collect::<Vec<_>>()
    );
    assert_eq!(grid.query(box_at(0., 0., 1e6)).len(), 100);
    assert!(
        grid.query(box_at(-4., -4., 0.001))
            .windows(2)
            .all(|w| w[0] < w[1])
    );
    let small = Candidates::new(boxes[..10].iter().copied().enumerate(), 2);
    assert_eq!(small.query(box_at(500., 500., 0.1)).len(), 10);
}
