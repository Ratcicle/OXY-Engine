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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InheritPlatform {
    None,
    Horizontal,
    All,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Posture {
    Standing,
    Crouched,
    Sliding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JumpMode {
    Manual,
    Automatic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementProfile {
    Direct,
    Parkour,
    ChainedJumps,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraMode {
    FirstPerson,
    ThirdPerson,
    Fixed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyFacing {
    Movement,
    Look,
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
    pub ground_acceleration: f32,
    pub ground_braking: f32,
    pub ground_friction: f32,
    pub air_acceleration: f32,
    pub air_projected_limit: f32,
    pub absolute_speed_limit: f32,
    pub sprint_speed: f32,
    pub crouch_speed: f32,
    pub sprint_action: Id,
    pub crouch_action: Id,
    pub crouch_height: f32,
    pub crouch_toggle: bool,
    pub coyote_ms: f32,
    pub jump_buffer_ms: f32,
    pub inherit_platform: InheritPlatform,
    pub recovery_distance: f32,
    pub jump_mode: JumpMode,
    pub air_resistance: f32,
    /// Zero disables this optional horizontal cap. The numerical guard is separate.
    pub horizontal_limit: f32,
    pub jump_retention: f32,
    pub landing_retention: f32,
    pub slide_enabled: bool,
    pub slide_min_speed: f32,
    pub slide_exit_speed: f32,
    pub slide_duration: f32,
    pub slide_friction: f32,
    pub slide_control: f32,
    pub facing: BodyFacing,
    pub angular_speed: f32,
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
            ground_acceleration: 40.,
            ground_braking: 20.,
            ground_friction: 6.,
            air_acceleration: 8.,
            air_projected_limit: 6.,
            absolute_speed_limit: 150.,
            sprint_speed: 8.,
            crouch_speed: 3.,
            sprint_action: "correr".into(),
            crouch_action: "agachar".into(),
            crouch_height: 1.,
            crouch_toggle: false,
            coyote_ms: 100.,
            jump_buffer_ms: 120.,
            inherit_platform: InheritPlatform::All,
            recovery_distance: 0.5,
            jump_mode: JumpMode::Manual,
            air_resistance: 0.,
            horizontal_limit: 0.,
            jump_retention: 1.,
            landing_retention: 1.,
            slide_enabled: false,
            slide_min_speed: 6.,
            slide_exit_speed: 2.,
            slide_duration: 0.9,
            slide_friction: 0.9,
            slide_control: 3.,
            facing: BodyFacing::Movement,
            angular_speed: 720.,
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
        if self.enabled && (collider.sensor || !collider.enabled) {
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
        if [
            self.speed,
            self.gravity,
            self.jump_speed,
            self.ground_acceleration,
            self.ground_braking,
            self.ground_friction,
            self.air_acceleration,
            self.air_projected_limit,
            self.sprint_speed,
            self.crouch_speed,
            self.coyote_ms,
            self.jump_buffer_ms,
            self.recovery_distance,
            self.air_resistance,
            self.horizontal_limit,
            self.slide_min_speed,
            self.slide_exit_speed,
            self.slide_duration,
            self.slide_friction,
            self.slide_control,
            self.angular_speed,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("Velocidade, gravidade e pulo devem ser finitos e não negativos.".into());
        }
        if !self.absolute_speed_limit.is_finite()
            || !(1. ..=10000.).contains(&self.absolute_speed_limit)
            || !self.crouch_height.is_finite()
            || self.crouch_height < 2. * radius
            || self.crouch_height > height
            || self.coyote_ms > 1000.
            || self.jump_buffer_ms > 1000.
            || self.recovery_distance > 10.
            || !self.jump_retention.is_finite()
            || !(0. ..=1.).contains(&self.jump_retention)
            || !self.landing_retention.is_finite()
            || !(0. ..=1.).contains(&self.landing_retention)
            || self.slide_duration <= 0.
            || self.slide_exit_speed > self.slide_min_speed
        {
            return Err("Altura agachada deve caber na cápsula; limite absoluto deve estar entre 1 e 10000 m/s; tolerâncias de pulo entre 0 e 1000 ms.".into());
        }
        Ok(motion)
    }
    /// Presets write ordinary editable values. Runtime never dispatches on a
    /// preset name; action IDs, geometry, filters and references are preserved.
    pub fn apply_profile(&mut self, profile: MovementProfile) {
        let (accel, brake, friction, air, cap, drag, horizontal, landing, slide) = match profile {
            MovementProfile::Direct => (40., 20., 6., 8., 6., 0., 0., 1., false),
            MovementProfile::Parkour => (45., 12., 4., 20., 6., 0.02, 24., 0.92, true),
            MovementProfile::ChainedJumps => (50., 10., 4., 80., 6., 0., 40., 0.8, true),
        };
        self.speed = 6.;
        self.sprint_speed = 8.;
        self.crouch_speed = 3.;
        self.ground_acceleration = accel;
        self.ground_braking = brake;
        self.ground_friction = friction;
        self.air_acceleration = air;
        self.air_projected_limit = cap;
        self.air_resistance = drag;
        self.horizontal_limit = horizontal;
        self.jump_retention = 1.;
        self.landing_retention = landing;
        self.slide_enabled = slide;
        self.jump_mode = JumpMode::Manual;
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
    pub crouched_eye_height: f32,
    pub posture_smoothing: f32,
    pub mode: CameraMode,
    pub distance: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub follow_height: f32,
    pub shoulder: f32,
    pub zoom_step: f32,
    pub position_smoothing: f32,
    pub obstruction_return: f32,
    pub transition_seconds: f32,
    pub collision_radius: f32,
    pub collision_margin: f32,
    pub shoulder_action: Id,
    pub mode_action: Id,
    pub rotation_smoothing: f32,
    pub hide_first_person_only: bool,
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
            crouched_eye_height: 0.85,
            posture_smoothing: 0.10,
            mode: CameraMode::FirstPerson,
            distance: 4.,
            min_distance: 0.4,
            max_distance: 12.,
            follow_height: 1.4,
            shoulder: 0.45,
            zoom_step: 0.5,
            position_smoothing: 0.06,
            obstruction_return: 0.15,
            transition_seconds: 0.25,
            collision_radius: 0.15,
            collision_margin: 0.02,
            shoulder_action: "trocar_ombro".into(),
            mode_action: "alternar_camera".into(),
            rotation_smoothing: 0.,
            hide_first_person_only: true,
        }
    }
}
impl CameraRig {
    pub fn validate_settings(&self) -> Result<(), String> {
        if [
            self.distance,
            self.min_distance,
            self.max_distance,
            self.zoom_step,
            self.position_smoothing,
            self.obstruction_return,
            self.transition_seconds,
            self.collision_radius,
            self.collision_margin,
            self.rotation_smoothing,
            self.eye_height,
            self.crouched_eye_height,
            self.posture_smoothing,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.)
            || !self.shoulder.is_finite()
            || !self.follow_height.is_finite()
            || self.min_distance > self.max_distance
            || self.distance < self.min_distance
            || self.distance > self.max_distance
            || self.collision_radius < 0.01
            || self.collision_margin <= 0.
            || self.collision_margin >= self.collision_radius
            || !self.sensitivity.is_finite()
            || !(0.001..=10.).contains(&self.sensitivity)
            || !self.pitch_limit.is_finite()
            || !(1. ..89.9).contains(&self.pitch_limit)
        {
            return Err(
                "Câmera: distância, limites, transição e volume de proteção inválidos.".into(),
            );
        }
        Ok(())
    }
    pub fn look(&self, yaw: f32, pitch: f32, delta: Vec2) -> (f32, f32) {
        let radians = f64::from(self.sensitivity).to_radians();
        let yaw =
            f64::from(yaw) - f64::from(delta.x) * radians * if self.invert_x { -1. } else { 1. };
        let pitch = (f64::from(pitch)
            - f64::from(delta.y) * radians * if self.invert_y { -1. } else { 1. })
        .clamp(
            -f64::from(self.pitch_limit).to_radians(),
            f64::from(self.pitch_limit).to_radians(),
        );
        (yaw.rem_euclid(std::f64::consts::TAU) as f32, pitch as f32)
    }
}

#[derive(Clone, Debug)]
pub struct CharacterState {
    pub position: Vec3,
    pub previous_position: Vec3,
    pub velocity: Vec3,
    pub grounded: bool,
    pub yaw: f32,
    pub previous_yaw: f32,
    pub look_yaw: f32,
    pub pitch: f32,
    pub movement_blocks: BTreeSet<String>,
    pub look_blocks: BTreeSet<String>,
    pub posture: Posture,
    pub height: f32,
    pub uniform_scale: f32,
    pub eye_height: f32,
    pub previous_eye_height: f32,
    pub support: Option<Support>,
    pub sprinting: bool,
    pub(crate) want_crouch: bool,
    pub(crate) coyote_until: f64,
    pub(crate) jump_until: Option<f64>,
    pub(crate) jump_consumed: bool,
    pub(crate) slide_elapsed: f32,
    pub(crate) slide_latched: bool,
    pub(crate) trajectory: u64,
}
#[derive(Clone, Debug)]
pub struct Support {
    pub object: Id,
    pub point: Vec3,
    pub normal: Vec3,
    pub surface: Option<Id>,
    pub velocity: Vec3,
}
#[derive(Clone, Debug)]
pub enum MovementEvent {
    Jumped,
    Landed {
        impact_speed: f32,
    },
    LeftSupport,
    SurfaceChanged {
        previous: Option<Id>,
        current: Option<Id>,
    },
    PostureChanged(Posture),
    SideContact {
        object: Id,
        point: Vec3,
        normal: Vec3,
    },
}
#[derive(Clone, Debug)]
pub struct MovementRecord {
    pub object: Id,
    pub event: MovementEvent,
    /// Immutable state at the transition; waits may retain this snapshot.
    pub state: std::sync::Arc<CharacterState>,
}
impl CharacterState {
    pub fn total_velocity(&self) -> Vec3 {
        self.velocity + self.support.as_ref().map_or(Vec3::ZERO, |s| s.velocity)
    }
    pub fn new(position: Vec3, yaw: f32) -> Self {
        Self {
            position,
            previous_position: position,
            velocity: Vec3::ZERO,
            grounded: false,
            yaw,
            previous_yaw: yaw,
            look_yaw: yaw,
            pitch: 0.,
            movement_blocks: BTreeSet::new(),
            look_blocks: BTreeSet::new(),
            posture: Posture::Standing,
            height: 0.,
            uniform_scale: 0.,
            eye_height: 0.,
            previous_eye_height: 0.,
            support: None,
            sprinting: false,
            want_crouch: false,
            coyote_until: f64::NEG_INFINITY,
            jump_until: None,
            jump_consumed: false,
            slide_elapsed: 0.,
            slide_latched: false,
            trajectory: 0,
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
/// Shared optical near plane, in world metres (renderer and camera protection).
pub const CAMERA_NEAR: f32 = 0.02;
#[derive(Clone, Debug)]
pub enum CameraLookAt {
    Point(Vec3),
    Object(Id),
}
#[derive(Clone, Copy, Debug, Default)]
pub struct TeleportOptions {
    pub keep_velocity: bool,
    /// Optional world yaw in radians. Graph/UI adapters expose degrees.
    pub yaw: Option<f32>,
    /// Optional world yaw/pitch in radians, restoring the player's view.
    pub look: Option<[f32; 2]>,
    /// Zero rejects occupied destinations. Maximum supported local search: 2 m.
    pub search_radius: f32,
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
        rig.validate_settings()?;
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
            || !rig.crouched_eye_height.is_finite()
            || rig.crouched_eye_height < 0.
            || !rig.posture_smoothing.is_finite()
            || rig.posture_smoothing < 0.
        {
            return Err("Altura, sensibilidade ou limite de olhar inválidos.".into());
        }
    }
    if let Some(platform) = &entity.platform
        && (scene.kind != SceneKind::ThreeD
            || entity.character3d.is_some()
            || platform.velocity.iter().any(|v| !v.is_finite())
            || !platform.max_transport_per_step.is_finite()
            || !(0.001..=10.).contains(&platform.max_transport_per_step))
    {
        return Err("Plataforma exige cena 3D, velocidade finita e limite de transporte entre 0,001 e 10 m por passo; não pode ser um personagem.".into());
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
