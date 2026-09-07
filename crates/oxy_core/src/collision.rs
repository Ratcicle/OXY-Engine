//! Axis aligned collision. Rotation affects the drawing, never the physical box.
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn from_center_half(center: Vec3, half: Vec3) -> Self {
        let half = half.abs().max(Vec3::splat(0.0001));
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn translated(self, offset: Vec3) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    pub fn overlaps(self, other: Self, dimensions: usize) -> bool {
        (0..dimensions)
            .all(|axis| self.min[axis] < other.max[axis] && self.max[axis] > other.min[axis])
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MotionResult {
    pub delta: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
}

/// Swept slab intersection: the returned normal points away from the obstacle.
pub fn sweep(
    moving: Aabb,
    displacement: Vec3,
    fixed: Aabb,
    dimensions: usize,
) -> Option<(f32, Vec3)> {
    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for axis in 0..dimensions {
        if displacement[axis].abs() < 1e-9 {
            if moving.max[axis] <= fixed.min[axis] || moving.min[axis] >= fixed.max[axis] {
                return None;
            }
            continue;
        }
        let (near, far, sign) = if displacement[axis] > 0.0 {
            (
                (fixed.min[axis] - moving.max[axis]) / displacement[axis],
                (fixed.max[axis] - moving.min[axis]) / displacement[axis],
                -1.0,
            )
        } else {
            (
                (fixed.max[axis] - moving.min[axis]) / displacement[axis],
                (fixed.min[axis] - moving.max[axis]) / displacement[axis],
                1.0,
            )
        };
        if near > entry {
            entry = near;
            normal = Vec3::ZERO;
            normal[axis] = sign;
        }
        exit = exit.min(far);
    }
    if entry <= exit && (0.0..=1.0).contains(&entry) {
        Some((entry, normal))
    } else {
        None
    }
}

/// Resolve one fixed step against static boxes, retaining sliding on other axes.
/// Up is +Y in both scene types. Sweeps, rather than end-point overlap, prevent tunneling.
pub fn move_and_slide(
    mut body: Aabb,
    velocity: Vec3,
    dt: f32,
    obstacles: &[Aabb],
    dimensions: usize,
) -> MotionResult {
    let mut result = MotionResult {
        delta: Vec3::ZERO,
        velocity,
        grounded: false,
    };
    // Recover shallow initial overlaps (for instance after moving an object in the editor).
    for obstacle in obstacles {
        if !body.overlaps(*obstacle, dimensions) {
            continue;
        }
        let mut correction = Vec3::ZERO;
        let mut shortest = f32::INFINITY;
        for axis in 0..dimensions {
            for distance in [
                obstacle.min[axis] - body.max[axis],
                obstacle.max[axis] - body.min[axis],
            ] {
                if distance.abs() < shortest {
                    shortest = distance.abs();
                    correction = Vec3::ZERO;
                    correction[axis] = distance;
                }
            }
        }
        body = body.translated(correction);
        result.delta += correction;
        for axis in 0..dimensions {
            if correction[axis] != 0.0 {
                result.velocity[axis] = 0.0;
            }
        }
        result.grounded |= correction.y > 0.0;
    }
    let mut remaining = result.velocity * dt;
    for _ in 0..dimensions + 1 {
        if remaining.length_squared() < 1e-14 {
            break;
        }
        let mut earliest: Option<(f32, Vec3)> = None;
        for obstacle in obstacles {
            if let Some(contact) = sweep(body, remaining, *obstacle, dimensions)
                && earliest.is_none_or(|best| contact.0 < best.0)
            {
                earliest = Some(contact);
            }
        }
        if let Some((fraction, normal)) = earliest {
            let step = remaining * fraction;
            result.delta += step;
            body = body.translated(step);
            remaining *= 1.0 - fraction;
            remaining -= normal * remaining.dot(normal);
            result.velocity -= normal * result.velocity.dot(normal);
            result.grounded |= normal.y > 0.5;
        } else {
            result.delta += remaining;
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cannot_tunnel_through_floor_or_wall() {
        let body = Aabb::from_center_half(Vec3::new(0.0, 5.0, 0.0), Vec3::splat(0.5));
        let floor = Aabb::from_center_half(Vec3::new(0.0, -0.5, 0.0), Vec3::new(20.0, 0.5, 1.0));
        let motion = move_and_slide(body, Vec3::new(2.0, -1000.0, 0.0), 1.0 / 60.0, &[floor], 2);
        assert!((motion.delta.y + 4.5).abs() < 0.0001);
        assert!(motion.grounded);
        assert!((motion.delta.x - 2.0 / 60.0).abs() < 0.0001);
        let wall = Aabb::from_center_half(Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.05, 20.0, 1.0));
        let motion = move_and_slide(body, Vec3::new(1000.0, 0.0, 0.0), 1.0 / 60.0, &[wall], 2);
        assert!((motion.delta.x - 2.45).abs() < 0.0001);
        assert_eq!(motion.velocity.x, 0.0);
    }

    #[test]
    fn three_dimensions_and_parallel_motion() {
        let body = Aabb::from_center_half(Vec3::ZERO, Vec3::splat(0.5));
        let wall = Aabb::from_center_half(Vec3::new(0.0, 0.0, 3.0), Vec3::new(20.0, 20.0, 0.1));
        let result = move_and_slide(body, Vec3::new(1.0, 0.0, 500.0), 0.1, &[wall], 3);
        assert!((result.delta.z - 2.4).abs() < 0.0001);
        assert!((result.delta.x - 0.1).abs() < 0.0001);
        assert!(sweep(body, Vec3::X, wall, 3).is_none());
    }
}
