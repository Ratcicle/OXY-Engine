//! Physical surface properties are explicit project data, independent of color/UVs.
use crate::{
    document::{Id, Project, new_id},
    input_actions::{Use, every_entity},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VelocitySpace {
    World,
    Local,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlatformMode {
    Velocity,
    Animation,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceMaterial {
    pub id: Id,
    pub name: String,
    /// Ground braking multiplier. Zero permits coasting.
    pub friction: f32,
    /// Control acceleration multiplier, not a hard speed clamp.
    pub traction: f32,
    pub speed_multiplier: f32,
    pub conveyor: [f32; 3],
    pub conveyor_space: VelocitySpace,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfacePreset {
    Common,
    Ice,
    Mud,
    Conveyor,
}
impl SurfaceMaterial {
    pub fn preset(preset: SurfacePreset) -> Self {
        let (name, friction, traction, speed_multiplier, conveyor) = match preset {
            SurfacePreset::Common => ("Comum", 1., 1., 1., [0.; 3]),
            SurfacePreset::Ice => ("Gelo", 0.025, 0.15, 1., [0.; 3]),
            SurfacePreset::Mud => ("Lama", 1.8, 0.65, 0.55, [0.; 3]),
            SurfacePreset::Conveyor => ("Esteira", 1., 1., 1., [2., 0., 0.]),
        };
        Self {
            id: new_id(),
            name: name.into(),
            friction,
            traction,
            speed_multiplier,
            conveyor,
            conveyor_space: VelocitySpace::Local,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.name.trim().is_empty() {
            return Err("Superfície física precisa de identificador e nome.".into());
        }
        if [self.friction, self.traction, self.speed_multiplier]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
            || self.conveyor.iter().any(|v| !v.is_finite())
        {
            return Err(format!(
                "Superfície {}: valores devem ser finitos; atrito, tração e velocidade desejada não podem ser negativos.",
                self.name
            ));
        }
        Ok(())
    }
}
/// Where the surface `id` is used: 3D colliders, 3D character bodies, attributes and logic
/// nodes, in each object's order.
pub fn uses(project: &Project, id: &str) -> Vec<Use> {
    let mut found = Vec::new();
    for (scope, e) in every_entity(project) {
        let mut add = |role: String| {
            found.push(Use {
                scope: scope.into(),
                entity: e.id.clone(),
                name: e.name.clone(),
                role,
            })
        };
        if e.physics3d
            .as_ref()
            .is_some_and(|c| c.surface.as_deref() == Some(id))
        {
            add("collider3d".into());
        }
        if e.character3d
            .as_ref()
            .is_some_and(|c| c.body.surface.as_deref() == Some(id))
        {
            add("character".into());
        }
        for (name, value) in &e.attributes {
            if value.surface() == Some(id) {
                add(format!("attribute {name}"));
            }
        }
        for node in &e.graph.nodes {
            if node.params.values().any(|v| v.surface() == Some(id)) {
                add(node.operation.clone());
            }
        }
    }
    found
}
pub fn references(project: &Project, id: &str) -> Vec<String> {
    let mut found = uses(project, id);
    // One line per object, as the editor lists them.
    found.dedup_by(|a, b| a.entity == b.entity);
    found
        .into_iter()
        .map(|found| format!("{} → {}", found.scope, found.name))
        .collect()
}
pub fn duplicate(project: &mut Project, id: &str) -> Result<Id, String> {
    let mut surface = project
        .surfaces
        .iter()
        .find(|s| s.id == id)
        .ok_or("Superfície ausente")?
        .clone();
    surface.id = new_id();
    surface.name.push_str(" · cópia");
    let id = surface.id.clone();
    project.surfaces.push(surface);
    Ok(id)
}
pub fn remove(project: &mut Project, id: &str) -> Result<(), String> {
    let uses = references(project, id);
    if !uses.is_empty() {
        return Err(format!(
            "A superfície ainda está em uso:\n{}",
            uses.join("\n")
        ));
    }
    let index = project
        .surfaces
        .iter()
        .position(|s| s.id == id)
        .ok_or("Superfície ausente")?;
    project.surfaces.remove(index);
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationPlatform {
    pub enabled: bool,
    pub mode: PlatformMode,
    /// World/local velocity, used only in Velocity mode. Never handed to Rapier.
    pub velocity: [f32; 3],
    pub space: VelocitySpace,
    /// A discontinuity larger than this releases passengers instead of launching them.
    pub max_transport_per_step: f32,
}
impl Default for TranslationPlatform {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: PlatformMode::Animation,
            velocity: [0.; 3],
            space: VelocitySpace::World,
            max_transport_per_step: 1.,
        }
    }
}
