use crate::document::Id;
use glam::Vec3;
use rapier3d::prelude::{SharedShape, Vector};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollisionGeometry {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// Deterministic content fingerprint, never a transient in-memory revision ID.
    pub source_fingerprint: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CollisionShape {
    Box { size: [f32; 3] },
    Capsule { height: f32, radius: f32 },
    Sphere { radius: f32 },
    Convex { geometry: Arc<CollisionGeometry> },
    TriMesh { geometry: Arc<CollisionGeometry> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollisionFilter {
    pub category: u32,
    pub mask: u32,
    pub blocks_character: bool,
    pub blocks_camera: bool,
}
impl Default for CollisionFilter {
    fn default() -> Self {
        Self {
            category: 1,
            mask: u32::MAX,
            blocks_character: true,
            blocks_camera: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collider3d {
    pub shape: CollisionShape,
    pub center: [f32; 3],
    pub enabled: bool,
    pub sensor: bool,
    pub filter: CollisionFilter,
    pub surface: Option<Id>,
}
impl Default for Collider3d {
    fn default() -> Self {
        Self {
            shape: CollisionShape::Box { size: [1.; 3] },
            center: [0.; 3],
            enabled: true,
            sensor: false,
            filter: Default::default(),
            surface: None,
        }
    }
}
impl Collider3d {
    pub fn validate(&self) -> Result<(), String> {
        if self.center.iter().any(|v| !v.is_finite()) {
            return Err("Centro do colisor deve ser finito.".into());
        }
        self.shape.validate()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ShapeKey {
    kind: u8,
    numbers: [u32; 6],
    geometry: usize,
}
impl CollisionShape {
    pub(super) fn key(&self, scale: Vec3) -> ShapeKey {
        let (kind, data, geometry) = match self {
            Self::Box { size } => (0, *size, 0),
            Self::Capsule { height, radius } => (1, [*height, *radius, 0.], 0),
            Self::Sphere { radius } => (2, [*radius, 0., 0.], 0),
            Self::Convex { geometry } => (3, [0.; 3], Arc::as_ptr(geometry) as usize),
            Self::TriMesh { geometry } => (4, [0.; 3], Arc::as_ptr(geometry) as usize),
        };
        ShapeKey {
            kind,
            geometry,
            numbers: [
                data[0].to_bits(),
                data[1].to_bits(),
                data[2].to_bits(),
                scale.x.to_bits(),
                scale.y.to_bits(),
                scale.z.to_bits(),
            ],
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: f32| v.is_finite() && v > 0.;
        match self {
            Self::Box { size } if !size.iter().copied().all(positive) => {
                Err("Caixa: dimensões positivas e finitas necessárias.".into())
            }
            Self::Capsule { height, radius }
                if !positive(*height) || !positive(*radius) || *height < 2. * radius =>
            {
                Err("Cápsula: altura deve ser pelo menos o diâmetro.".into())
            }
            Self::Sphere { radius } if !positive(*radius) => {
                Err("Esfera: raio positivo necessário.".into())
            }
            Self::Convex { geometry } | Self::TriMesh { geometry } => {
                if geometry.vertices.len() < 3
                    || geometry.vertices.len() > 250_000
                    || geometry.triangles.is_empty()
                    || geometry.triangles.len() > 500_000
                {
                    return Err("Geometria física vazia ou acima do limite de preparação.".into());
                }
                if geometry.vertices.iter().flatten().any(|v| !v.is_finite()) {
                    return Err("Geometria física contém posição inválida.".into());
                }
                for t in &geometry.triangles {
                    if t.iter().any(|&i| i as usize >= geometry.vertices.len()) {
                        return Err("Índice físico fora da geometria.".into());
                    }
                    let [a, b, c] = t.map(|i| Vec3::from(geometry.vertices[i as usize]));
                    if (b - a).cross(c - a).length_squared() < 1e-16 {
                        return Err("Geometria física contém triângulo degenerado.".into());
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(super) fn prepare(&self, scale: Vec3) -> Result<SharedShape, String> {
        self.validate()?;
        self.validate_scale(scale)?;
        let abs = scale.abs();
        Ok(match self {
            Self::Box { size } => {
                let half = Vec3::from(*size) * abs * 0.5;
                SharedShape::cuboid(half.x, half.y, half.z)
            }
            Self::Capsule { height, radius } => {
                if abs.max_element() - abs.min_element() > 1e-5 {
                    return Err("Cápsula exige escala uniforme.".into());
                }
                SharedShape::capsule_y((height * 0.5 - radius) * abs.x, radius * abs.x)
            }
            Self::Sphere { radius } => {
                if abs.max_element() - abs.min_element() > 1e-5 {
                    return Err("Esfera exige escala uniforme.".into());
                }
                SharedShape::ball(radius * abs.x)
            }
            Self::Convex { geometry } => {
                let points: Vec<_> = geometry
                    .vertices
                    .iter()
                    .map(|v| Vector::from_array((Vec3::from(*v) * scale).to_array()))
                    .collect();
                SharedShape::convex_hull(&points).ok_or("Não foi possível preparar volume convexo: são necessários pontos não coplanares.")?
            }
            Self::TriMesh { geometry } => {
                let points: Vec<_> = geometry
                    .vertices
                    .iter()
                    .map(|v| Vector::from_array((Vec3::from(*v) * scale).to_array()))
                    .collect();
                let mut triangles = geometry.triangles.clone();
                if scale.element_product() < 0. {
                    for t in &mut triangles {
                        t.swap(1, 2);
                    }
                }
                SharedShape::trimesh(points, triangles)
                    .map_err(|e| format!("Falha ao preparar triângulos físicos: {e}"))?
            }
        })
    }
    pub fn validate_scale(&self, scale: Vec3) -> Result<(), String> {
        let abs = scale.abs();
        if !scale.is_finite() || abs.min_element() < 1e-6 {
            return Err("Escala física não invertível ou não finita.".into());
        }
        let finite = match self {
            Self::Box { size } => (Vec3::from(*size) * abs).is_finite(),
            Self::Capsule { height, radius } => {
                if abs.max_element() - abs.min_element() > 1e-5 {
                    return Err("Cápsula exige escala uniforme.".into());
                }
                (height * abs.x).is_finite() && (radius * abs.x).is_finite()
            }
            Self::Sphere { radius } => {
                if abs.max_element() - abs.min_element() > 1e-5 {
                    return Err("Esfera exige escala uniforme.".into());
                }
                (radius * abs.x).is_finite()
            }
            Self::Convex { geometry } | Self::TriMesh { geometry } => geometry
                .vertices
                .iter()
                .all(|p| (Vec3::from(*p) * scale).is_finite()),
        };
        if !finite {
            return Err("Dimensões físicas fora do intervalo numérico suportado.".into());
        }
        Ok(())
    }
}
