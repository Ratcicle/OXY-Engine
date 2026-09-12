use super::*;
use std::{cell::RefCell, collections::VecDeque};

// At most 32 immutable preparations per thread, keeping the source Arc alive.
// This cache is derived, never serialized or compared as document state.
thread_local! { static PREPARED: RefCell<VecDeque<(ShapeKey, CollisionShape, SharedShape)>> = Default::default(); }
#[derive(Clone, Copy, Debug)]
pub struct MotionSettings {
    pub margin: f32,
    pub climb_degrees: f32,
    pub slide_degrees: f32,
    pub step_height: f32,
    pub step_width: f32,
    pub snap: f32,
}
impl From<CapsuleMotion> for MotionSettings {
    fn from(c: CapsuleMotion) -> Self {
        Self {
            margin: c.margin,
            climb_degrees: c.climb_degrees,
            slide_degrees: c.slide_degrees,
            step_height: c.step_height,
            step_width: c.step_width,
            snap: c.snap,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ShapeMotion {
    pub geometry: SharedShape,
    pub center: Vec3,
    pub height: f32,
    pub rotation: Quat,
    pub settings: MotionSettings,
}
impl std::ops::Deref for ShapeMotion {
    type Target = MotionSettings;
    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}
impl std::ops::DerefMut for ShapeMotion {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.settings
    }
}
impl ShapeMotion {
    pub fn new(shape: &CollisionShape, scale: f32, rotation: Quat) -> Result<Self, String> {
        if !scale.is_finite()
            || scale <= 0.
            || !rotation.is_finite()
            || !rotation.is_normalized()
            || matches!(shape, CollisionShape::TriMesh { .. })
        {
            return Err("Corpo de movimento exige escala uniforme positiva e uma forma convexa; malha de triângulos móvel não é suportada.".into());
        }
        let key = shape.key(Vec3::splat(scale));
        let geometry = PREPARED.with(|cache| -> Result<SharedShape, String> {
            let mut cache = cache.borrow_mut();
            if let Some(i) = cache.iter().position(|(k, _, _)| *k == key) {
                let item = cache.remove(i).unwrap();
                let geometry = item.2.clone();
                cache.push_back(item);
                return Ok(geometry);
            }
            let geometry = shape.prepare(Vec3::splat(scale))?;
            if geometry.mass_properties(1.).mass() <= 1e-8 {
                return Err("O corpo precisa de volume convexo válido, não coplanar.".into());
            }
            if cache.len() == 32 {
                cache.pop_front();
            }
            cache.push_back((key, shape.clone(), geometry.clone()));
            Ok(geometry)
        })?;
        let bounds = geometry.compute_local_aabb();
        // Keep analytic dimensions/centre for primitives. Reconstructing capsule
        // height from its segment AABB changes rounding at contacts in old games.
        let (height, center) = match shape {
            CollisionShape::Capsule { height, .. } => {
                (*height * scale, Vec3::Y * (*height * scale * 0.5))
            }
            CollisionShape::Box { size } => (size[1] * scale, Vec3::Y * (size[1] * scale * 0.5)),
            CollisionShape::Sphere { radius } => {
                (*radius * scale * 2., Vec3::Y * (*radius * scale))
            }
            _ => (bounds.maxs.y - bounds.mins.y, Vec3::Y * -bounds.mins.y),
        };
        let rotation = if matches!(shape, CollisionShape::Sphere { .. })
            || (matches!(shape, CollisionShape::Capsule { .. })
                && (rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-6))
        {
            Quat::IDENTITY
        } else {
            rotation
        };
        Ok(Self {
            geometry,
            center,
            height,
            rotation,
            settings: CapsuleMotion::default().into(),
        })
    }
    pub fn at(&self, feet: Vec3) -> Vec3 {
        feet + self.rotation * self.center
    }
    pub fn set_yaw(&mut self, yaw: f32) {
        // Yaw cannot change these axisymmetric bodies. Preserve the original
        // unrotated analytic query path, while boxes/convexes rotate physically.
        self.rotation = if matches!(
            self.geometry.as_typed_shape(),
            TypedShape::Capsule(_) | TypedShape::Ball(_)
        ) {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_y(yaw)
        };
    }
    pub fn validate(&self) -> Result<(), String> {
        let bounds = self.geometry.compute_local_aabb();
        let half_extent = (bounds.maxs - bounds.mins).min_element() * 0.5;
        if [
            self.margin,
            self.climb_degrees,
            self.slide_degrees,
            self.step_height,
            self.step_width,
            self.snap,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || self.margin <= 0.
            || self.margin >= half_extent
            || self.step_height < 0.
            || self.step_width <= 0.
            || self.snap < 0.
            || !(0. ..89.9).contains(&self.climb_degrees)
            || !(0. ..89.9).contains(&self.slide_degrees)
        {
            return Err("Corpo de movimento: margem, degrau, aderência ou inclinação inválidos. A margem deve ser menor que metade da menor dimensão.".into());
        }
        Ok(())
    }
}
