//! Common movement services exposed by the typed graph adapter.
use super::*;
mod actions;
mod data;
mod queries;
use crate::character::{CharacterState, MovementEvent, Posture};
pub use queries::{SceneQuery, SceneQueryShape};
pub(super) fn event_kind(event: &MovementEvent) -> &'static str {
    match event {
        MovementEvent::Jumped => "jump",
        MovementEvent::Landed { .. } => "land",
        MovementEvent::LeftSupport => "leave",
        MovementEvent::SurfaceChanged { .. } => "surface",
        MovementEvent::PostureChanged(_) => "posture",
        MovementEvent::SideContact { .. } => "side",
    }
}
pub(super) fn state_value(state: &CharacterState, key: &str) -> Result<Value, String> {
    let velocity = state.total_velocity();
    Ok(match key {
        "position" => Value::Vector3(state.position.to_array()),
        "velocity" => Value::Vector3(velocity.to_array()),
        "relative" => Value::Vector3(state.velocity.to_array()),
        "horizontal" => Value::Vector3([velocity.x, 0., velocity.z]),
        "speed" => Value::Number(f64::from(Vec3::new(velocity.x, 0., velocity.z).length())),
        "grounded" => Value::Bool(state.grounded),
        "rising" => Value::Bool(!state.grounded && velocity.y > 0.),
        "falling" => Value::Bool(!state.grounded && velocity.y < 0.),
        "posture" => Value::Text(
            match state.posture {
                Posture::Standing => "Em pé",
                Posture::Crouched => "Agachado",
                Posture::Sliding => "Deslizando",
            }
            .into(),
        ),
        "support" => Value::Object(state.support.as_ref().map(|s| s.object.clone())),
        "surface" => Value::Surface(state.support.as_ref().and_then(|s| s.surface.clone())),
        "point" => Value::Vector3(
            state
                .support
                .as_ref()
                .map_or([0.; 3], |s| s.point.to_array()),
        ),
        "normal" => Value::Vector3(
            state
                .support
                .as_ref()
                .map_or([0.; 3], |s| s.normal.to_array()),
        ),
        "support_velocity" => Value::Vector3(
            state
                .support
                .as_ref()
                .map_or([0.; 3], |s| s.velocity.to_array()),
        ),
        "movement_blocked" => Value::Bool(!state.movement_blocks.is_empty()),
        "look_blocked" => Value::Bool(!state.look_blocks.is_empty()),
        "movement_reasons" => Value::Text(
            state
                .movement_blocks
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        ),
        "look_reasons" => Value::Text(
            state
                .look_blocks
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => return Err("Estado do personagem ausente".into()),
    })
}
pub(super) fn state_values(state: &CharacterState) -> Vec<(&'static str, Value)> {
    [
        "position",
        "velocity",
        "relative",
        "horizontal",
        "speed",
        "grounded",
        "rising",
        "falling",
        "posture",
        "support",
        "surface",
        "point",
        "normal",
        "support_velocity",
        "movement_blocked",
        "look_blocked",
        "movement_reasons",
        "look_reasons",
    ]
    .into_iter()
    .map(|key| (key, state_value(state, key).expect("declared state field")))
    .collect()
}
pub(super) fn event_values(event: &RuntimeEvent, time: f64) -> Vec<(&'static str, Value)> {
    match event {
        RuntimeEvent::FixedStep => vec![
            ("dt", Value::Number(f64::from(FIXED_DT))),
            ("time", Value::Number(time)),
        ],
        RuntimeEvent::Movement(record) => {
            let mut values = state_values(&record.state);
            values.extend([
                (
                    "impact",
                    Value::Number(
                        if let MovementEvent::Landed { impact_speed } = record.event {
                            f64::from(impact_speed)
                        } else {
                            0.
                        },
                    ),
                ),
                (
                    "previous_surface",
                    Value::Surface(
                        if let MovementEvent::SurfaceChanged { previous, .. } = &record.event {
                            previous.clone()
                        } else {
                            None
                        },
                    ),
                ),
            ]);
            if let MovementEvent::SideContact {
                object,
                point,
                normal,
            } = &record.event
            {
                values.retain(|(k, _)| !matches!(*k, "point" | "normal"));
                values.extend([
                    ("contact", Value::Object(Some(object.clone()))),
                    ("point", Value::Vector3(point.to_array())),
                    ("normal", Value::Vector3(normal.to_array())),
                ]);
            } else {
                values.push(("contact", Value::Object(None)));
            }
            values
        }
        _ => Vec::new(),
    }
}
impl Runtime {
    pub(super) fn inspect_character(
        &self,
        id: &str,
    ) -> Result<std::borrow::Cow<'_, CharacterState>, String> {
        if let Some(state) = self.characters.states.get(id) {
            return Ok(std::borrow::Cow::Borrowed(state));
        }
        let entity = self.entity(id).ok_or("Personagem ausente")?;
        let config = entity
            .character3d
            .as_ref()
            .ok_or("Objeto não possui personagem 3D")?;
        let (position, rotation, scale) =
            crate::physics3d::world_pose(self.scene().world_matrix(id)?)?;
        crate::character::character_scale(rotation, scale)?;
        let motion = config.motion(entity, scale.x)?;
        let forward = rotation * -Vec3::Z;
        let mut state = CharacterState::new(position, (-forward.x).atan2(-forward.z));
        state.height = motion.height;
        state.uniform_scale = scale.x;
        Ok(std::borrow::Cow::Owned(state))
    }
    pub(super) fn event_used(&self, id: &str) -> bool {
        self.event_catalog().operations.contains(id)
    }
    pub(super) fn event_catalog(&self) -> &EventCatalog {
        self.event_operations.get_or_init(|| {
            let mut catalog = EventCatalog::default();
            for (i, e) in self.scene().entities.iter().enumerate() {
                let mut used = false;
                for node in &e.graph.nodes {
                    if node.operation.starts_with("event.") {
                        used = true;
                        catalog.operations.insert(node.operation.clone());
                    }
                }
                if used {
                    catalog.owners.push(i);
                }
            }
            catalog
        })
    }
    pub fn step_input(&self) -> &InputFrame {
        &self.step_input
    }
}
