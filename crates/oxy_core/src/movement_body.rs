//! Authored locomotion geometry. Origin is always at the feet, independently of appearance.
use crate::physics3d::{Collider3d, CollisionFilter, CollisionShape, ShapeMotion};
use glam::Quat;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MovementBody {
    pub standing: CollisionShape,
    /// None generates a shorter capsule/box; sphere/convex keep their shape.
    pub crouched: Option<CollisionShape>,
    pub filter: CollisionFilter,
    pub surface: Option<crate::document::Id>,
    pub enabled: bool,
}
impl Default for MovementBody {
    fn default() -> Self {
        Self {
            standing: CollisionShape::Capsule {
                height: 1.8,
                radius: 0.3,
            },
            crouched: None,
            filter: Default::default(),
            surface: None,
            enabled: true,
        }
    }
}
impl MovementBody {
    pub fn shape(&self, crouched: bool, crouch_height: f32) -> CollisionShape {
        if crouched {
            if let Some(shape) = &self.crouched {
                return shape.clone();
            }
            match &self.standing {
                CollisionShape::Capsule { radius, .. } => {
                    return CollisionShape::Capsule {
                        height: crouch_height,
                        radius: *radius,
                    };
                }
                CollisionShape::Box { size } => {
                    return CollisionShape::Box {
                        size: [size[0], crouch_height, size[2]],
                    };
                }
                _ => {}
            }
        }
        self.standing.clone()
    }
    pub fn prepare(
        &self,
        crouched: bool,
        crouch_height: f32,
        scale: f32,
        yaw: f32,
    ) -> Result<ShapeMotion, String> {
        ShapeMotion::new(
            &self.shape(crouched, crouch_height),
            scale,
            Quat::from_rotation_y(yaw),
        )
    }
    pub fn collider(&self, crouched: bool, crouch_height: f32) -> Result<Collider3d, String> {
        let shape = self.shape(crouched, crouch_height);
        let prepared = ShapeMotion::new(&shape, 1., Quat::IDENTITY)?;
        Ok(Collider3d {
            shape,
            center: prepared.center.to_array(),
            enabled: self.enabled,
            sensor: false,
            filter: self.filter.clone(),
            surface: self.surface.clone(),
        })
    }
    pub fn validate(&self, crouch_height: f32) -> Result<(), String> {
        let standing = self.prepare(false, crouch_height, 1., 0.)?;
        let crouched = self.prepare(true, crouch_height, 1., 0.)?;
        if crouched.height > standing.height + 1e-5 {
            return Err("O corpo agachado não pode ser mais alto que o corpo em pé.".into());
        }
        if self
            .crouched
            .as_ref()
            .is_some_and(|s| std::mem::discriminant(s) != std::mem::discriminant(&self.standing))
        {
            return Err("As formas em pé e agachada devem ser do mesmo tipo.".into());
        }
        Ok(())
    }
    /// Explicit convex generation; never implicitly adopts a visual mesh.
    pub fn convex_from(entity: &crate::document::Entity) -> Result<CollisionShape, String> {
        let collider = crate::physics3d::generate_collider(entity, true)?;
        ShapeMotion::new(&collider.shape, 1., Quat::IDENTITY)?;
        Ok(collider.shape)
    }
}
