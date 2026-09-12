//! Camera guides/edit operations in world metres, shared by UI and deterministic tests.
use crate::{character::*, document::*, runtime::CameraDebug};
use glam::Vec3;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraField {
    Eye,
    CrouchedEye,
    Distance,
    Shoulder,
    FollowHeight,
}
impl CameraField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Eye => "Olhos em pé",
            Self::CrouchedEye => "Olhos agachado",
            Self::Distance => "Distância",
            Self::Shoulder => "Ombro",
            Self::FollowHeight => "Altura de acompanhamento",
        }
    }
    pub fn get(self, rig: &CameraRig) -> f32 {
        match self {
            Self::Eye => rig.eye_height,
            Self::CrouchedEye => rig.crouched_eye_height,
            Self::Distance => rig.distance,
            Self::Shoulder => rig.shoulder,
            Self::FollowHeight => rig.follow_height,
        }
    }
    pub fn set(self, rig: &mut CameraRig, value: f32) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Valor da câmera deve ser finito".into());
        }
        let mut next = rig.clone();
        match self {
            Self::Eye => next.eye_height = value.max(0.),
            Self::CrouchedEye => next.crouched_eye_height = value.max(0.),
            Self::Distance => next.distance = value.clamp(rig.min_distance, rig.max_distance),
            Self::Shoulder => next.shoulder = value,
            Self::FollowHeight => next.follow_height = value,
        }
        next.validate_settings()?;
        *rig = next;
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct CameraHandle {
    pub field: CameraField,
    pub position: Vec3,
    pub axis: Vec3,
    pub units: f32,
}
pub fn handles(
    scene: &Scene,
    id: &str,
    evaluated: &CameraDebug,
) -> Result<Vec<CameraHandle>, String> {
    let rig = scene
        .entity(id)
        .and_then(|e| e.camera_rig.as_ref())
        .ok_or("Câmera sem controle")?;
    if rig.mode == CameraMode::Fixed {
        return Ok(vec![]);
    }
    let target = rig.target.as_deref().ok_or("Escolha o alvo da câmera")?;
    let (feet, _, scale) = crate::physics3d::world_pose(scene.world_matrix(target)?)?;
    let mut handles = vec![];
    for field in [CameraField::Eye, CameraField::CrouchedEye] {
        handles.push(CameraHandle {
            field,
            position: feet + Vec3::Y * field.get(rig) * scale.y,
            axis: Vec3::Y,
            units: scale.y,
        });
    }
    if rig.mode == CameraMode::ThirdPerson {
        let right = evaluated.pose.rotation * Vec3::X;
        let back = evaluated.pose.rotation * Vec3::Z;
        handles.push(CameraHandle {
            field: CameraField::FollowHeight,
            position: evaluated.anchor,
            axis: Vec3::Y,
            units: scale.y,
        });
        handles.push(CameraHandle {
            field: CameraField::Distance,
            position: evaluated.desired,
            axis: back,
            units: 1.,
        });
        handles.push(CameraHandle {
            field: CameraField::Shoulder,
            position: evaluated.anchor + right * rig.shoulder,
            axis: right,
            units: 1.,
        });
    }
    Ok(handles)
}
/// Near/guide planes are optical guides, not authored collision geometry.
pub fn frustum(pose: &GameCameraPose, aspect: f32, distance: f32) -> [Vec3; 4] {
    let h = (pose.fov.to_radians() * 0.5).tan() * distance;
    let w = h * aspect;
    [
        [-w, -h, -distance],
        [w, -h, -distance],
        [w, h, -distance],
        [-w, h, -distance],
    ]
    .map(|v| pose.position + pose.rotation * Vec3::from(v))
}
