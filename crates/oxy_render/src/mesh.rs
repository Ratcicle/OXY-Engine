//! Predictable, editable primitive meshes. Unit dimensions, +Y up, right handed.
//! Texture coordinates use a top-left image origin (V grows down).
use glam::{Vec2, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct MeshVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    fn vertex(&mut self, position: Vec3, normal: Vec3, uv: Vec2) -> u32 {
        let index = self.vertices.len() as u32;
        self.vertices.push(MeshVertex {
            position,
            normal,
            uv,
        });
        index
    }

    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, uv_min: Vec2, uv_max: Vec2) {
        let start = self.vertices.len() as u32;
        for (position, uv) in corners.into_iter().zip([
            Vec2::new(uv_min.x, uv_max.y),
            Vec2::new(uv_max.x, uv_max.y),
            Vec2::new(uv_max.x, uv_min.y),
            Vec2::new(uv_min.x, uv_min.y),
        ]) {
            self.vertex(position, normal, uv);
        }
        self.indices
            .extend([start, start + 1, start + 2, start, start + 2, start + 3]);
    }
}

/// A cube uses a 3-column, 2-row atlas: front, right, back / left, top, bottom.
pub fn cube() -> Mesh {
    let mut mesh = Mesh::default();
    let p = |x, y, z| Vec3::new(x, y, z) * 0.5;
    let faces = [
        (
            [
                p(-1., -1., 1.),
                p(1., -1., 1.),
                p(1., 1., 1.),
                p(-1., 1., 1.),
            ],
            Vec3::Z,
        ),
        (
            [
                p(1., -1., 1.),
                p(1., -1., -1.),
                p(1., 1., -1.),
                p(1., 1., 1.),
            ],
            Vec3::X,
        ),
        (
            [
                p(1., -1., -1.),
                p(-1., -1., -1.),
                p(-1., 1., -1.),
                p(1., 1., -1.),
            ],
            Vec3::NEG_Z,
        ),
        (
            [
                p(-1., -1., -1.),
                p(-1., -1., 1.),
                p(-1., 1., 1.),
                p(-1., 1., -1.),
            ],
            Vec3::NEG_X,
        ),
        (
            [
                p(-1., 1., 1.),
                p(1., 1., 1.),
                p(1., 1., -1.),
                p(-1., 1., -1.),
            ],
            Vec3::Y,
        ),
        (
            [
                p(-1., -1., -1.),
                p(1., -1., -1.),
                p(1., -1., 1.),
                p(-1., -1., 1.),
            ],
            Vec3::NEG_Y,
        ),
    ];
    for (index, (corners, normal)) in faces.into_iter().enumerate() {
        let uv_min = Vec2::new((index % 3) as f32 / 3., (index / 3) as f32 / 2.);
        mesh.quad(corners, normal, uv_min, uv_min + Vec2::new(1. / 3., 0.5));
    }
    mesh
}

pub fn rectangle() -> Mesh {
    let mut mesh = Mesh::default();
    mesh.quad(
        [
            Vec3::new(-0.5, -0.5, 0.),
            Vec3::new(0.5, -0.5, 0.),
            Vec3::new(0.5, 0.5, 0.),
            Vec3::new(-0.5, 0.5, 0.),
        ],
        Vec3::Z,
        Vec2::ZERO,
        Vec2::ONE,
    );
    mesh
}

pub fn plane() -> Mesh {
    let mut mesh = Mesh::default();
    mesh.quad(
        [
            Vec3::new(-0.5, 0., 0.5),
            Vec3::new(0.5, 0., 0.5),
            Vec3::new(0.5, 0., -0.5),
            Vec3::new(-0.5, 0., -0.5),
        ],
        Vec3::Y,
        Vec2::ZERO,
        Vec2::ONE,
    );
    mesh
}

pub fn circle(segments: u32) -> Mesh {
    let count = segments.clamp(3, 256);
    let mut mesh = Mesh::default();
    mesh.vertex(Vec3::ZERO, Vec3::Z, Vec2::splat(0.5));
    for index in 0..=count {
        let angle = index as f32 / count as f32 * std::f32::consts::TAU;
        let position = Vec3::new(angle.cos(), angle.sin(), 0.) * 0.5;
        mesh.vertex(
            position,
            Vec3::Z,
            Vec2::new(position.x + 0.5, 0.5 - position.y),
        );
        if index > 0 {
            mesh.indices.extend([0, index, index + 1]);
        }
    }
    mesh
}

/// Latitude-longitude sphere with an explicit seam, stable across tessellation changes.
pub fn sphere(segments: u32) -> Mesh {
    let columns = segments.clamp(3, 256);
    let rows = (columns / 2).max(2);
    let mut mesh = Mesh::default();
    for row in 0..=rows {
        let v = row as f32 / rows as f32;
        let latitude = v * std::f32::consts::PI;
        for column in 0..=columns {
            let u = column as f32 / columns as f32;
            let longitude = u * std::f32::consts::TAU;
            let normal = Vec3::new(
                latitude.sin() * longitude.sin(),
                latitude.cos(),
                latitude.sin() * longitude.cos(),
            );
            mesh.vertex(normal * 0.5, normal, Vec2::new(u, v));
        }
    }
    for row in 0..rows {
        for column in 0..columns {
            let a = row * (columns + 1) + column;
            let b = a + columns + 1;
            mesh.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    mesh
}

/// Cylinder: side on the upper half of the PNG; caps in the lower two squares.
pub fn cylinder(segments: u32) -> Mesh {
    let count = segments.clamp(3, 256);
    let mut mesh = Mesh::default();
    for index in 0..=count {
        let u = index as f32 / count as f32;
        let angle = u * std::f32::consts::TAU;
        let normal = Vec3::new(angle.sin(), 0., angle.cos());
        mesh.vertex(normal * 0.5 + Vec3::Y * 0.5, normal, Vec2::new(u, 0.));
        mesh.vertex(normal * 0.5 - Vec3::Y * 0.5, normal, Vec2::new(u, 0.5));
        if index < count {
            let a = index * 2;
            mesh.indices.extend([a, a + 1, a + 2, a + 2, a + 1, a + 3]);
        }
    }
    for (sign, center_u) in [(1., 0.25), (-1., 0.75)] {
        let center = mesh.vertex(
            Vec3::Y * sign * 0.5,
            Vec3::Y * sign,
            Vec2::new(center_u, 0.75),
        );
        for index in 0..=count {
            let angle = index as f32 / count as f32 * std::f32::consts::TAU;
            let x = angle.sin();
            let z = angle.cos();
            mesh.vertex(
                Vec3::new(x, sign, z) * 0.5,
                Vec3::Y * sign,
                Vec2::new(center_u + x * 0.25, 0.75 + z * 0.25),
            );
            if index > 0 {
                if sign > 0. {
                    mesh.indices
                        .extend([center, center + index + 1, center + index]);
                } else {
                    mesh.indices
                        .extend([center, center + index, center + index + 1]);
                }
            }
        }
    }
    mesh
}

/// Möller-Trumbore with barycentric coordinates for exact UV painting.
pub fn ray_triangle(origin: Vec3, direction: Vec3, points: [Vec3; 3]) -> Option<(f32, [f32; 3])> {
    let edge1 = points[1] - points[0];
    let edge2 = points[2] - points[0];
    let cross = direction.cross(edge2);
    let determinant = edge1.dot(cross);
    if determinant.abs() < 1e-7 {
        return None;
    }
    let inv = determinant.recip();
    let offset = origin - points[0];
    let u = offset.dot(cross) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = offset.cross(edge1);
    let v = direction.dot(q) * inv;
    if v < 0. || u + v > 1. {
        return None;
    }
    let distance = edge2.dot(q) * inv;
    (distance >= 0.).then_some((distance, [1. - u - v, u, v]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_have_valid_meshes_and_uvs() {
        for mesh in [
            cube(),
            rectangle(),
            plane(),
            circle(16),
            sphere(24),
            cylinder(24),
        ] {
            assert!(!mesh.indices.is_empty());
            assert_eq!(mesh.indices.len() % 3, 0);
            for index in &mesh.indices {
                assert!((*index as usize) < mesh.vertices.len());
            }
            for vertex in mesh.vertices {
                assert!(vertex.position.is_finite());
                assert!((vertex.normal.length() - 1.).abs() < 0.001);
                assert!(vertex.uv.cmpge(Vec2::ZERO).all() && vertex.uv.cmple(Vec2::ONE).all());
            }
        }
    }

    #[test]
    fn ray_reports_barycentric_uv() {
        let triangle = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let (distance, barycentric) =
            ray_triangle(Vec3::new(0.25, 0.25, 1.), Vec3::NEG_Z, triangle).unwrap();
        assert!((distance - 1.).abs() < 1e-6);
        assert_eq!(barycentric, [0.5, 0.25, 0.25]);
        assert!(ray_triangle(Vec3::new(2., 2., 1.), Vec3::NEG_Z, triangle).is_none());
    }
}
