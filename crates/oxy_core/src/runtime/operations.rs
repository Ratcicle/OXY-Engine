//! The shared operations layer: what logic does to the game and reads from it, as `Runtime`
//! methods with plain arguments (IDs, names, numbers, vectors, values) whose error is the
//! message the person reads. A node is a thin adapter that reads its parameters and inputs and
//! calls one of them, and the script API calls the same ones, so both share the checks, the
//! limits, the messages and the effects. Nothing in their signatures is a node's, so a method
//! that a node needs differently (its parameter names, its activation) leaves that to the
//! node. The layer is:
//! - objects: `attribute`, `set_attribute`, `damage`, `spawn_copy` (and `script_spawn_copy`),
//!   `set_component_enabled`, `play_animation`, `object_surface` (here), `remove_object`
//!   (`runtime.rs`) and `apply_surface` (`movement_nodes/queries.rs`);
//! - transforms (`transforms.rs`): `object_pose`, `world_rotation`, `write_transform`;
//! - the game (`game_nodes.rs`): `change_scene_from_logic`, `variable`, `set_variable`,
//!   `save_progress`, `clear_progress`, `has_saved_progress`, `game_paused`,
//!   `set_game_paused`, `toggle_game_pause`, `set_cursor_shown`, `quit_game`, `game_time`;
//! - sound (`audio_nodes.rs`): `play_sound`, `play_sound_loop`, `stop_sounds`, `set_volume`,
//!   `audio`;
//! - draws and time text (`math_nodes.rs`): `random_number` (and `script_random_number`),
//!   `set_random_seed`, `format_time`;
//! - input (here): `input_pressed`, `input_held`, `input_released`, `input_look`,
//!   `input_wheel`, `input_movement`;
//! - 3D characters (`characters/`, `movement_nodes.rs`, `movement_nodes/actions.rs`), cameras
//!   (`cameras.rs`) and scene queries (`movement_nodes/queries.rs`): their `pub` methods, such
//!   as `inspect_character`, `teleport_character`, `set_camera_mode` and `query_scene`.
use super::*;
use glam::Vec2;

/// A part of an object that logic turns on and off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectComponent {
    /// The 2D box and the 3D collider or area alike.
    Collider,
    /// The 2D movement controller.
    Controller,
    Visible,
    /// The object's own logic graph; its scripts go on.
    Behavior,
    /// The playing of its clip.
    Animation,
    /// The moving platform.
    Platform,
}
impl ObjectComponent {
    /// The component a node names: collider, controller, visible, behavior, animation or
    /// platform.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "collider" => Self::Collider,
            "controller" => Self::Controller,
            "visible" => Self::Visible,
            "behavior" => Self::Behavior,
            "animation" => Self::Animation,
            "platform" => Self::Platform,
            _ => return None,
        })
    }
}

/// A damage `amount` is a number, 0 or more.
pub(super) fn check_damage(amount: f64) -> Result<(), String> {
    if !amount.is_finite() || amount < 0.0 {
        return Err("Dano deve ser um número não negativo".into());
    }
    Ok(())
}

impl Runtime {
    /// The value of `target`'s attribute `name`; none when the object lacks it.
    pub fn attribute(&self, target: &str, name: &str) -> Result<Option<&Value>, String> {
        Ok(self
            .entity(target)
            .ok_or("Objeto de leitura ausente")?
            .attributes
            .get(name))
    }

    /// Writes `value` to `target`'s attribute `name`, which the object must already have with a
    /// value of the same type. The value is finite, and a surface or object it names exists.
    pub fn set_attribute(&mut self, target: &str, name: &str, value: Value) -> Result<(), String> {
        if !value.is_finite() {
            return Err("Novo valor de atributo não finito.".into());
        }
        if let Some(id) = value.surface()
            && !self.project.surfaces.iter().any(|s| s.id == id)
        {
            return Err("Nova referência de superfície não existe.".into());
        }
        if let Value::Object(Some(id)) = &value
            && self.entity(id).is_none()
        {
            return Err("Nova referência de atributo não existe".into());
        }
        let entity = self
            .entity_mut(target)
            .ok_or("Objeto de escrita não existe")?;
        let old = entity
            .attributes
            .get_mut(name)
            .ok_or("Crie primeiro o atributo no painel de propriedades")?;
        if std::mem::discriminant(old) != std::mem::discriminant(&value) {
            return Err("Valor incompatível com o tipo do atributo".into());
        }
        *old = value;
        Ok(())
    }

    /// Lowers `target`'s number attribute `attribute` by `amount`, 0 or more, never below 0.
    pub fn damage(&mut self, target: &str, attribute: &str, amount: f64) -> Result<(), String> {
        check_damage(amount)?;
        let entity = self.entity_mut(target).ok_or("Alvo do dano foi removido")?;
        let value = entity
            .attributes
            .get_mut(attribute)
            .ok_or("Alvo não possui o atributo de dano configurado")?;
        let Value::Number(value) = value else {
            return Err("Atributo de dano precisa ser numérico".into());
        };
        *value = (*value - amount).max(0.0);
        Ok(())
    }

    /// Plays `target`'s clip `clip` (its ID) from the start; without `restart`, a clip already
    /// playing goes on.
    pub fn play_animation(
        &mut self,
        target: &str,
        clip: &str,
        restart: bool,
    ) -> Result<(), String> {
        let entity = self.entity(target).ok_or("Objeto animado ausente")?;
        let clip_id = entity
            .clips
            .iter()
            .find(|c| c.id == clip)
            .ok_or("ID de clip não existe no objeto")?
            .id
            .clone();
        if restart
            || !self
                .animations
                .get(target)
                .is_some_and(|player| player.playing && player.clip_id == clip_id)
        {
            self.animations
                .insert(target.into(), AnimationPlayer::new(clip_id));
        }
        Ok(())
    }

    /// Copies `template` and everything under it into the running scene, `offset` metres from
    /// it, and gives the copy's ID. The copies' IDs come from `caller` (the object that copies
    /// and what in it asks, a node's ID) and from how many it made before (see `spawn_ids`), so
    /// a run that makes the same copies gives them the same IDs. The offset is not checked: a
    /// node's offset past the `f32` range has always made a copy at infinity, so a caller that
    /// can pass a non-finite one, as the script API, refuses it first. The copy's objects that
    /// attach scripts hear on_start once the tasks already queued ran.
    pub fn spawn_copy(
        &mut self,
        template: &str,
        offset: Vec3,
        caller: (&str, &str),
    ) -> Result<Id, String> {
        self.physics_dirty = true;
        self.index.take();
        self.input_modes.take();
        self.event_operations.take();
        let key = (caller.0.to_owned(), caller.1.to_owned());
        let count = self.run.spawns.get(&key).copied().unwrap_or(0);
        let root = self
            .scene_mut_internal()
            .duplicate_subtree_with(template, &mut spawn_ids(b"oxy.spawn", &key, count))?;
        self.run.spawns.insert(key, count + 1);
        self.place_copy(&root, offset);
        Ok(root)
    }

    /// `spawn_copy` asked by the script `script` that `owner` attaches: its copies are counted
    /// apart from the graph's, by (owner, script), and their IDs never meet a node's. The
    /// offset is finite. Provisional: the script API calls it in a later stage; tests now.
    #[doc(hidden)]
    pub fn script_spawn_copy(
        &mut self,
        template: &str,
        offset: Vec3,
        owner: &str,
        script: &str,
    ) -> Result<Id, String> {
        if !offset.is_finite() {
            return Err("O deslocamento da cópia precisa ser finito".into());
        }
        self.physics_dirty = true;
        self.index.take();
        self.input_modes.take();
        self.event_operations.take();
        let key = (owner.to_owned(), script.to_owned());
        let count = self.run.script_spawns.get(&key).copied().unwrap_or(0);
        let root = self
            .scene_mut_internal()
            .duplicate_subtree_with(template, &mut spawn_ids(b"oxy.script.spawn", &key, count))?;
        self.run.script_spawns.insert(key, count + 1);
        self.place_copy(&root, offset);
        Ok(root)
    }

    /// A copy just made goes `offset` from its original, is recorded, and its scripts start.
    fn place_copy(&mut self, root: &Id, offset: Vec3) {
        // The editor duplicate offset is not part of a runtime spawn's explicit offset.
        let delta = Vec3::new(offset.x - 0.5, offset.y, offset.z);
        let descendants: HashSet<_> = self.scene().descendants(root).into_iter().collect();
        if let Some(mut recording) = self.recording.take() {
            recording.spawned.extend(
                self.scene()
                    .entities
                    .iter()
                    .filter(|entity| descendants.contains(&entity.id))
                    .map(|entity| SpawnedObject {
                        id: entity.id.clone(),
                        name: entity.name.clone(),
                        graph: entity.graph.clone(),
                    }),
            );
            self.recording = Some(recording);
        }
        let entity = self.entity_mut(root).unwrap();
        entity.transform.position = (Vec3::from(entity.transform.position) + delta).to_array();
        for entity in self
            .scene_mut_internal()
            .entities
            .iter_mut()
            .filter(|entity| descendants.contains(&entity.id))
        {
            for track in entity
                .clips
                .iter_mut()
                .flat_map(|clip| &mut clip.tracks)
                .filter(|track| track.target == *root)
            {
                for key in &mut track.keyframes {
                    key.transform.position =
                        (Vec3::from(key.transform.position) + delta).to_array();
                }
            }
        }
        self.start_copied_scripts(&descendants);
    }

    /// Turns `component` of `target` on or off.
    pub fn set_component_enabled(
        &mut self,
        target: &str,
        component: ObjectComponent,
        enabled: bool,
    ) -> Result<(), String> {
        self.physics_dirty = true;
        match component {
            // An old document may have both a 2D box and a 3D collider.
            ObjectComponent::Collider => {
                let entity = self.entity_mut(target).ok_or("Objeto ausente")?;
                if entity.collider.is_none() && entity.physics3d.is_none() {
                    return Err("Objeto não tem colisor nem área".into());
                }
                let was = entity.collider.as_ref().is_some_and(|c| c.enabled)
                    || entity.physics3d.as_ref().is_some_and(|c| c.enabled);
                if let Some(collider) = &mut entity.collider {
                    collider.enabled = enabled;
                }
                if let Some(collider) = &mut entity.physics3d {
                    collider.enabled = enabled;
                }
                // Turned back on, an area starts a new activation that whoever is inside
                // enters afresh, even when it was off only within this step. Turned off, its
                // occupancy ends without an exit.
                if enabled && !was {
                    self.activate_area(target);
                    self.overlap_pairs.retain(|(area, _)| area != target);
                    self.characters.forget_area(target);
                }
                if !enabled {
                    self.area_activations.remove(target);
                    self.area_hits.remove(target);
                }
            }
            ObjectComponent::Controller => {
                self.entity_mut(target)
                    .ok_or("Objeto ausente")?
                    .controller
                    .as_mut()
                    .ok_or("Objeto não tem controlador")?
                    .enabled = enabled;
            }
            ObjectComponent::Visible => {
                self.entity_mut(target).ok_or("Objeto ausente")?.visible = enabled;
            }
            ObjectComponent::Behavior => {
                if self.entity(target).is_none() {
                    return Err("Objeto ausente".into());
                }
                if enabled {
                    self.disabled_behaviors.remove(target);
                } else {
                    self.disabled_behaviors.insert(target.into());
                    // The graph's waits: the object's scripts are not its graph.
                    self.waiting
                        .retain(|task| task.owner != target || !task.is_node());
                }
            }
            ObjectComponent::Animation => {
                if let Some(player) = self.animations.get_mut(target) {
                    player.playing = enabled;
                }
            }
            ObjectComponent::Platform => {
                self.entity_mut(target)
                    .ok_or("Objeto ausente")?
                    .platform
                    .as_mut()
                    .ok_or("Objeto não tem plataforma móvel")?
                    .enabled = enabled;
            }
        }
        Ok(())
    }

    /// The physical surface of `target`'s 3D shape, its character's body or its collider; none
    /// is the default one.
    pub fn object_surface(&self, target: &str) -> Result<Option<Id>, String> {
        Ok(self
            .entity(target)
            .and_then(|e| {
                e.character3d
                    .as_ref()
                    .map(|c| &c.body.surface)
                    .or_else(|| e.physics3d.as_ref().map(|c| &c.surface))
            })
            .ok_or("Objeto não possui forma física 3D")?
            .clone())
    }

    /// Whether the input action `action` was pressed in this step.
    pub fn input_pressed(&self, action: &str) -> bool {
        self.step_input.pressed(action)
    }

    /// Whether the input action `action` is held in this step.
    pub fn input_held(&self, action: &str) -> bool {
        self.step_input.held(action)
    }

    /// Whether the input action `action` was released in this step.
    pub fn input_released(&self, action: &str) -> bool {
        self.step_input.released.contains(action)
    }

    /// How far the look moved in this step, as the device moved it.
    pub fn input_look(&self) -> Vec2 {
        Vec2::from(self.step_input.look)
    }

    /// How far the wheel turned in this step.
    pub fn input_wheel(&self) -> f32 {
        self.step_input.wheel
    }

    /// The movement axis this step's input gives through the actions of the 3D character
    /// `character`: right and forward.
    pub fn input_movement(&self, character: &str) -> Result<Vec2, String> {
        let actions = &self
            .entity(character)
            .and_then(|e| e.character3d.as_ref())
            .ok_or("Escolha o personagem cujas ações formam este eixo.")?
            .actions;
        Ok(crate::character::movement_axis(&self.step_input, actions))
    }
}
