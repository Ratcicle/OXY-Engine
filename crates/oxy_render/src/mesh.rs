//! Predictable, editable primitive meshes. Unit dimensions, +Y up, right handed.
//! Texture coordinates use a top-left image origin (V grows down).
use glam::{Vec2, Vec3};
use oxy_core::document::{Entity, Primitive};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock},
};

/// Only topology belongs in this key. Dimensions, pivots and materials remain per object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MeshKey {
    Quad,
    Circle(u32),
    Cube,
    Sphere(u32),
    Cylinder(u32),
    Plane,
}

impl MeshKey {
    pub fn for_entity(entity: &Entity) -> Option<Self> {
        let segments = entity.segments.clamp(3, 256);
        Some(match entity.primitive? {
            Primitive::Rectangle | Primitive::Sprite => Self::Quad,
            Primitive::Circle => Self::Circle(segments),
            Primitive::Cube => Self::Cube,
            Primitive::Sphere => Self::Sphere(segments),
            Primitive::Cylinder => Self::Cylinder(segments),
            Primitive::Plane => Self::Plane,
        })
    }

    fn generate(self) -> Mesh {
        match self {
            Self::Quad => rectangle(),
            Self::Circle(segments) => circle(segments),
            Self::Cube => cube(),
            Self::Sphere(segments) => sphere(segments),
            Self::Cylinder(segments) => cylinder(segments),
            Self::Plane => plane(),
        }
    }
}

#[derive(Default)]
struct CpuMeshCache {
    entries: VecDeque<(MeshKey, Arc<Mesh>)>,
    bytes: usize,
}

impl CpuMeshCache {
    fn get(&mut self, key: MeshKey) -> Arc<Mesh> {
        if let Some(index) = self.entries.iter().position(|(stored, _)| *stored == key) {
            let entry = self.entries.remove(index).expect("Known cache entry");
            let mesh = Arc::clone(&entry.1);
            self.entries.push_back(entry);
            return mesh;
        }
        let mesh = Arc::new(key.generate());
        let bytes = mesh.byte_size();
        while !self.entries.is_empty()
            && (self.entries.len() >= 64 || self.bytes + bytes > 64 * 1024 * 1024)
        {
            self.bytes -= self
                .entries
                .pop_front()
                .expect("Non-empty cache")
                .1
                .byte_size();
        }
        self.entries.push_back((key, Arc::clone(&mesh)));
        self.bytes += bytes;
        mesh
    }
}

/// Shared by GPU upload and CPU ray picking. No entity/document state is held in this cache.
pub(crate) fn cached_primitive(key: MeshKey) -> Arc<Mesh> {
    static CACHE: OnceLock<Mutex<CpuMeshCache>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(key)
}

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
    fn byte_size(&self) -> usize {
        std::mem::size_of_val(self.vertices.as_slice())
            + std::mem::size_of_val(self.indices.as_slice())
    }

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
    fn cache_key_ignores_object_state_but_keeps_curved_topology() {
        let mut first = Entity::new("Original", Some(Primitive::Cube));
        let key = MeshKey::for_entity(&first);
        first.dimensions = [8., 0.4, 3.];
        first.transform.position = [100., -3., 4.];
        first.transform.rotation = [0.2, 0.6, 0.1];
        first.transform.scale = [-1., 2., 3.];
        first.transform.pivot = [0.3, 0.1, 0.];
        first.material.color = [0.2, 0.4, 0.8, 0.5];
        first.material.texture = Some("texture-stable-id".into());
        first.segments = 80;
        assert_eq!(key, MeshKey::for_entity(&first));
        first.primitive = Some(Primitive::Sphere);
        let sphere = MeshKey::for_entity(&first);
        first.segments = 81;
        assert_ne!(sphere, MeshKey::for_entity(&first));
        first.segments = 0;
        assert_eq!(MeshKey::for_entity(&first), Some(MeshKey::Sphere(3)));
        first.segments = 999;
        assert_eq!(MeshKey::for_entity(&first), Some(MeshKey::Sphere(256)));
        first.primitive = Some(Primitive::Sprite);
        let sprite = MeshKey::for_entity(&first);
        first.primitive = Some(Primitive::Rectangle);
        assert_eq!(sprite, MeshKey::for_entity(&first));
    }

    #[test]
    fn cpu_cache_reuses_geometry_and_evicts_without_invalidating_references() {
        let mut cache = CpuMeshCache::default();
        let first = cache.get(MeshKey::Sphere(24));
        assert!(Arc::ptr_eq(&first, &cache.get(MeshKey::Sphere(24))));
        let triangle_count = first.indices.len();
        for segments in 32..100 {
            cache.get(MeshKey::Circle(segments));
        }
        assert!(cache.entries.len() <= 64);
        assert!(cache.bytes <= 64 * 1024 * 1024);
        assert_eq!(first.indices.len(), triangle_count);
        assert!(!Arc::ptr_eq(&first, &cache.get(MeshKey::Sphere(24))));
    }

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
