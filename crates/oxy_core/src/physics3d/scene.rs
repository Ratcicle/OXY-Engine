use super::*;
use crate::{
    document::{Entity, Scene, SceneKind},
    scene_view::SceneView,
};
use glam::Mat4;
use std::sync::Arc;

/// Reject shear rather than displaying one orientation and solving another.
pub fn world_pose(matrix: Mat4) -> Result<(Vec3, Quat, Vec3), String> {
    if !matrix.is_finite() || matrix.determinant().abs() < 1e-8 {
        return Err("Transformação física não invertível.".into());
    }
    let (scale, rotation, position) = matrix.to_scale_rotation_translation();
    if !Mat4::from_scale_rotation_translation(scale, rotation, position).abs_diff_eq(matrix, 1e-4) {
        return Err("Colisor 3D: o parentesco produz cisalhamento. Use uma raiz sem deformação ou incorpore a escala na geometria.".into());
    }
    Ok((position, rotation, scale))
}

impl CollisionGeometry {
    /// Explicit authoring operation: visual topology and UVs are never modified.
    pub fn from_entity(entity: &Entity) -> Result<Self, String> {
        let (mesh, dimensions) = if let Some(mesh) = &entity.mesh {
            (mesh.clone(), Vec3::ONE)
        } else {
            (
                crate::geometry::primitives::for_entity(entity)?,
                Vec3::from(entity.dimensions),
            )
        };
        let vertices = mesh
            .data()
            .vertices
            .iter()
            .map(|v| (Vec3::from(v.position) * dimensions).to_array())
            .collect();
        let triangles = mesh
            .prepared()
            .triangles
            .iter()
            .map(|t| {
                let face = mesh.face(t.face).unwrap();
                t.corners
                    .map(|i| mesh.prepared().vertices[&face.corners[i].vertex] as u32)
            })
            .collect();
        let mut result = Self {
            vertices,
            triangles,
            source_fingerprint: None,
        };
        result.source_fingerprint = Some(result.fingerprint());
        Ok(result)
    }
    pub fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in self
            .vertices
            .iter()
            .flatten()
            .flat_map(|f| f.to_bits().to_le_bytes())
            .chain(
                self.triangles
                    .iter()
                    .flatten()
                    .flat_map(|i| i.to_le_bytes()),
            )
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        hash
    }
}
pub fn generate_collider(entity: &Entity, convex: bool) -> Result<Collider3d, String> {
    let geometry = Arc::new(CollisionGeometry::from_entity(entity)?);
    let shape = if convex {
        CollisionShape::Convex { geometry }
    } else {
        CollisionShape::TriMesh { geometry }
    };
    // Fail before replacing authored data when the geometry cannot form a valid hull.
    shape.prepare(Vec3::ONE)?;
    Ok(Collider3d {
        shape,
        ..Default::default()
    })
}

impl PhysicsWorld {
    /// Once per simulation phase, never per controller/contact. SceneView shares ancestors.
    /// This method does not move characters or create a second simulation authority.
    pub fn sync_scene(&mut self, scene: &Scene, show_disabled: bool) -> Result<(), String> {
        if scene.kind != SceneKind::ThreeD {
            return Err("Consultas 3D não podem ser executadas em uma cena 2D.".into());
        }
        let view = SceneView::new(scene);
        let mut seen = HashSet::new();
        for entity in &scene.entities {
            if let Some(config) = &entity.physics3d {
                seen.insert(entity.id.clone());
                let (position, rotation, scale) = world_pose(view.world_matrix(&entity.id)?)?;
                if show_disabled && !config.enabled {
                    let mut visible = config.clone();
                    visible.enabled = true;
                    self.upsert(&entity.id, &visible, position, rotation, scale)?;
                } else {
                    self.upsert(&entity.id, config, position, rotation, scale)?;
                }
            } else if let Some(legacy) = &entity.collider {
                seen.insert(entity.id.clone());
                let bounds = view.collider_bounds(&entity.id)?;
                let config = Collider3d {
                    shape: CollisionShape::Box {
                        size: (bounds.max - bounds.min).to_array(),
                    },
                    enabled: legacy.enabled || show_disabled,
                    sensor: legacy.is_trigger,
                    ..Default::default()
                };
                self.upsert(
                    &entity.id,
                    &config,
                    (bounds.min + bounds.max) * 0.5,
                    Quat::IDENTITY,
                    Vec3::ONE,
                )?;
            }
        }
        let removed: Vec<_> = self
            .entries
            .keys()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        for id in removed {
            self.remove(&id);
        }
        self.flush();
        Ok(())
    }
}
