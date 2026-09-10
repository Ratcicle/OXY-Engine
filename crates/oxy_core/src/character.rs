//! Authored movement settings and transient state. No host/UI types belong here.
use crate::{
    document::{Entity, Id, Scene, SceneKind},
    input_actions::MovementActions,
    physics3d::{CapsuleMotion, CollisionShape},
};
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovementReference {
    World,
    Body,
    Camera,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterConfig {
    pub enabled: bool,
    pub automatic_input: bool,
    pub actions: MovementActions,
    pub reference: MovementReference,
    pub speed: f32,
    pub gravity: f32,
    pub jump_speed: f32,
    pub margin: f32,
    pub slope_degrees: f32,
    pub step_height: f32,
    pub step_width: f32,
    pub snap: f32,
}
impl Default for CharacterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            automatic_input: true,
            actions: Default::default(),
            reference: MovementReference::Camera,
            speed: 6.,
            gravity: 22.,
            jump_speed: 8.,
            margin: 0.01,
            slope_degrees: 45.,
            step_height: 0.25,
            step_width: 0.2,
            snap: 0.15,
        }
    }
}
impl CharacterConfig {
    pub fn motion(&self, entity: &Entity, scale: f32) -> Result<CapsuleMotion, String> {
        let collider = entity
            .physics3d
            .as_ref()
            .ok_or("Personagem 3D requer um colisor de cápsula.")?;
        let CollisionShape::Capsule { height, radius } = collider.shape else {
            return Err("Personagem 3D requer uma cápsula vertical.".into());
        };
        if collider.sensor || !collider.enabled {
            return Err(
                "Personagem 3D requer uma cápsula sólida ativa, não uma área de detecção.".into(),
            );
        }
        // Feet, not the visual mesh origin, are the physical reference point.
        if !Vec3::from(collider.center).abs_diff_eq(Vec3::Y * (height * 0.5), 1e-5) {
            return Err("O centro da cápsula do personagem deve ficar em (0, metade da altura, 0); a origem representa os pés.".into());
        }
        let motion = CapsuleMotion {
            height: height * scale,
            radius: radius * scale,
            margin: self.margin,
            climb_degrees: self.slope_degrees,
            slide_degrees: (self.slope_degrees + 0.1).min(89.8),
            step_height: self.step_height,
            step_width: self.step_width,
            snap: self.snap,
        };
        motion.validate()?;
        if [self.speed, self.gravity, self.jump_speed]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("Velocidade, gravidade e pulo devem ser finitos e não negativos.".into());
        }
        Ok(motion)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraRig {
    pub target: Option<Id>,
    pub eye_height: f32,
    /// Degrees per raw device unit. Never multiplied by dt or UI scale.
    pub sensitivity: f32,
    pub invert_x: bool,
    pub invert_y: bool,
    pub pitch_limit: f32,
    pub hidden: Vec<Id>,
}
impl Default for CameraRig {
    fn default() -> Self {
        Self {
            target: None,
            eye_height: 1.6,
            sensitivity: 0.12,
            invert_x: false,
            invert_y: false,
            pitch_limit: 85.,
            hidden: Vec::new(),
        }
    }
}
impl CameraRig {
    pub fn look(&self, yaw: f32, pitch: f32, delta: Vec2) -> (f32, f32) {
        let radians = self.sensitivity.to_radians();
        let yaw = yaw - delta.x * radians * if self.invert_x { -1. } else { 1. };
        let pitch = (pitch - delta.y * radians * if self.invert_y { -1. } else { 1. }).clamp(
            -self.pitch_limit.to_radians(),
            self.pitch_limit.to_radians(),
        );
        (yaw.rem_euclid(std::f32::consts::TAU), pitch)
    }
}

#[derive(Clone, Debug)]
pub struct CharacterState {
    pub position: Vec3,
    pub previous_position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    pub yaw: f32,
    pub pitch: f32,
    pub movement_blocks: BTreeSet<String>,
    pub look_blocks: BTreeSet<String>,
}
impl CharacterState {
    pub fn new(position: Vec3, yaw: f32) -> Self {
        Self {
            position,
            previous_position: position,
            velocity: Vec3::ZERO,
            grounded: false,
            yaw,
            pitch: 0.,
            movement_blocks: BTreeSet::new(),
            look_blocks: BTreeSet::new(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct GameCameraPose {
    pub position: Vec3,
    pub rotation: Quat,
    pub fov: f32,
    pub hidden: Vec<Id>,
}

pub fn validate(
    scene: &Scene,
    entity: &Entity,
    view: &crate::scene_view::SceneView<'_>,
) -> Result<(), String> {
    if let Some(config) = &entity.character3d {
        if scene.kind != SceneKind::ThreeD
            || entity.controller.is_some()
            || entity.collider.is_some()
        {
            return Err("O personagem novo exige cena 3D e não pode usar simultaneamente o controlador/colisor legado.".into());
        }
        let (_, rotation, scale) = crate::physics3d::world_pose(view.world_matrix(&entity.id)?)?;
        character_scale(rotation, scale)?;
        config.motion(entity, scale.x)?;
    }
    if let Some(rig) = &entity.camera_rig {
        if scene.kind != SceneKind::ThreeD || entity.camera.is_none() {
            return Err("A câmera de personagem exige um componente de câmera em cena 3D.".into());
        }
        if rig
            .target
            .as_ref()
            .is_some_and(|id| view.entity(id).is_none())
            || rig.hidden.iter().any(|id| view.entity(id).is_none())
        {
            return Err("A câmera referencia um objeto ausente.".into());
        }
        if !rig.eye_height.is_finite()
            || rig.eye_height < 0.
            || !rig.sensitivity.is_finite()
            || !(0.001..=10.).contains(&rig.sensitivity)
            || !rig.pitch_limit.is_finite()
            || !(1. ..89.9).contains(&rig.pitch_limit)
        {
            return Err("Altura, sensibilidade ou limite de olhar inválidos.".into());
        }
    }
    Ok(())
}
pub fn character_scale(rotation: Quat, scale: Vec3) -> Result<f32, String> {
    if scale.min_element() <= 0.
        || !scale.abs_diff_eq(Vec3::splat(scale.x), 1e-4)
        || !(rotation * Vec3::Y).abs_diff_eq(Vec3::Y, 1e-4)
    {
        return Err("A raiz física precisa ficar em pé e usar escala global uniforme positiva; incline/espelhe as peças visuais filhas.".into());
    }
    Ok(scale.x)
}
pub fn movement_axis(input: &crate::runtime::InputFrame, actions: &MovementActions) -> Vec2 {
    let keyboard = Vec2::new(
        i32::from(input.held(&actions.right)) as f32 - i32::from(input.held(&actions.left)) as f32,
        i32::from(input.held(&actions.forward)) as f32
            - i32::from(input.held(&actions.back)) as f32,
    );
    let analog = Vec2::from(input.movement);
    (keyboard
        + if analog.is_finite() {
            analog
        } else {
            Vec2::ZERO
        })
    .clamp_length_max(1.)
}
pub fn wish_direction(axis: Vec2, yaw: f32) -> Vec3 {
    Quat::from_rotation_y(yaw) * Vec3::new(axis.x, 0., -axis.y)
}
