//! Authoring policy for components: which ones an object can receive, and what adding or
//! removing one changes. Shared by the editor's Properties panel and project commands.
use crate::{
    character::{CameraRig, CharacterConfig},
    document::*,
    physics3d::{Collider3d, CollisionShape},
    surface::TranslationPlatform,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Character,
    Collider3d,
    Sensor3d,
    Platform,
    CameraControl,
    Collider2d,
    Sensor2d,
    Controller2d,
}
impl ComponentKind {
    pub const ALL: [Self; 8] = [
        Self::Character,
        Self::Collider3d,
        Self::Sensor3d,
        Self::Platform,
        Self::CameraControl,
        Self::Collider2d,
        Self::Sensor2d,
        Self::Controller2d,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Character => "Personagem 3D",
            Self::Collider3d => "Colisor 3D",
            Self::Sensor3d => "Área de detecção 3D",
            Self::Platform => "Plataforma móvel",
            Self::CameraControl => "Controle da câmera",
            Self::Collider2d => "Colisor 2D",
            Self::Sensor2d => "Área de detecção 2D",
            Self::Controller2d => "Movimento 2D",
        }
    }
    /// Scene kind the component belongs to.
    pub fn scene_kind(self) -> SceneKind {
        match self {
            Self::Collider2d | Self::Sensor2d | Self::Controller2d => SceneKind::TwoD,
            _ => SceneKind::ThreeD,
        }
    }
    /// Whether the entity already has this component.
    pub fn present(self, e: &Entity) -> bool {
        match self {
            Self::Character => e.character3d.is_some(),
            Self::Collider3d => e.physics3d.as_ref().is_some_and(|c| !c.sensor),
            Self::Sensor3d => e.physics3d.as_ref().is_some_and(|c| c.sensor),
            Self::Platform => e.platform.is_some(),
            Self::CameraControl => e.camera_rig.is_some(),
            Self::Collider2d => e.collider.as_ref().is_some_and(|c| !c.is_trigger),
            Self::Sensor2d => e.collider.as_ref().is_some_and(|c| c.is_trigger),
            Self::Controller2d => e.controller.is_some(),
        }
    }
}

// This is an authoring policy, not a migration or a change to old game behavior.
// Existing 3D legacy data remains readable; it never enters the creation menu.
/// Components that can be added to `e` in a scene of `kind`.
pub fn available(kind: SceneKind, e: &Entity) -> Vec<ComponentKind> {
    use ComponentKind as C;
    if e.camera.is_some() {
        return if kind == SceneKind::ThreeD && e.camera_rig.is_none() {
            vec![C::CameraControl]
        } else {
            vec![]
        };
    }
    if e.ui.is_some() {
        return vec![];
    }
    if kind == SceneKind::TwoD {
        let mut entries = Vec::new();
        if e.collider.is_none() {
            entries.extend([C::Collider2d, C::Sensor2d]);
        }
        if e.controller.is_none() && e.collider.as_ref().is_none_or(|c| !c.is_trigger) {
            entries.push(C::Controller2d);
        }
        return entries;
    }
    let mut entries = Vec::new();
    if e.controller.is_none() && e.collider.is_none() {
        if e.character3d.is_none() && e.platform.is_none() && e.physics3d.is_none() {
            entries.push(C::Character);
        }
        if e.physics3d.is_none() && e.character3d.is_none() {
            entries.extend([C::Collider3d, C::Sensor3d]);
        }
        if e.platform.is_none()
            && e.character3d.is_none()
            && e.physics3d.as_ref().is_none_or(|c| !c.sensor)
        {
            entries.push(C::Platform);
        }
    }
    entries
}

/// Adds `component` with its defaults, and the input actions it needs to `project`.
pub fn add(
    kind: SceneKind,
    project: &mut Project,
    e: &mut Entity,
    component: ComponentKind,
) -> Result<(), String> {
    use ComponentKind as C;
    if !available(kind, e).contains(&component) {
        return Err(format!(
            "{} é incompatível com este objeto ou já está presente.",
            component.label()
        ));
    }
    match component {
        C::Character => {
            let config = CharacterConfig::default();
            crate::input_actions::ensure_character(project, &config);
            e.character3d = Some(config);
        }
        C::CameraControl => {
            let rig = CameraRig::default();
            crate::input_actions::ensure_camera(project, &rig);
            e.camera_rig = Some(rig);
        }
        C::Collider3d | C::Sensor3d | C::Platform => {
            e.physics3d.get_or_insert_with(|| Collider3d {
                shape: CollisionShape::Box { size: e.dimensions },
                sensor: component == C::Sensor3d,
                ..Default::default()
            });
            if component == C::Platform {
                e.platform = Some(TranslationPlatform::default());
            }
        }
        C::Collider2d | C::Sensor2d => {
            e.collider = Some(Collider {
                size: e.dimensions,
                is_trigger: component == C::Sensor2d,
                ..Default::default()
            })
        }
        C::Controller2d => {
            let controller = Controller::default();
            crate::input_actions::ensure_controller(project, &controller.actions, kind);
            e.collider.get_or_insert_with(|| Collider {
                size: e.dimensions,
                ..Default::default()
            });
            e.controller = Some(controller);
        }
    }
    Ok(())
}

/// Why `component` cannot be removed from `e` yet, if something depends on it.
pub fn removal_blocked(e: &Entity, component: ComponentKind) -> Option<&'static str> {
    use ComponentKind as C;
    match component {
        C::Collider3d | C::Sensor3d if e.character3d.is_some() || e.platform.is_some() => {
            Some("Remova primeiro o Personagem 3D ou a Plataforma móvel que depende desta forma.")
        }
        C::Collider2d if e.controller.is_some() => {
            Some("Remova primeiro o Movimento 2D que depende deste colisor.")
        }
        _ => None,
    }
}

/// Removes `component` from `e`.
pub fn remove(e: &mut Entity, component: ComponentKind) -> Result<(), String> {
    use ComponentKind as C;
    if !component.present(e) {
        return Err(format!("O objeto não tem {}.", component.label()));
    }
    if let Some(reason) = removal_blocked(e, component) {
        return Err(reason.into());
    }
    match component {
        C::Character => e.character3d = None,
        C::Collider3d | C::Sensor3d => e.physics3d = None,
        C::Platform => e.platform = None,
        C::CameraControl => e.camera_rig = None,
        C::Collider2d | C::Sensor2d => e.collider = None,
        C::Controller2d => e.controller = None,
    }
    Ok(())
}
