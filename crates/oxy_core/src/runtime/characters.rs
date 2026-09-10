use super::*;
use crate::character::*;
use crate::physics3d::{PhysicsWorld, QueryOptions};
use glam::{Mat4, Quat, Vec2};

#[derive(Default)]
pub(super) struct Characters {
    world: Option<PhysicsWorld>,
    states: HashMap<Id, CharacterState>,
    pub(super) intents: HashMap<Id, Vec2>,
    pub(super) jumps: HashSet<Id>,
}
impl Characters {
    fn step(&mut self, scene: &mut Scene, input: &InputFrame, dt: f32) -> Result<(), String> {
        if scene.kind != SceneKind::ThreeD {
            return Ok(());
        }
        let indices: Vec<_> = scene
            .entities
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.character3d.as_ref().filter(|c| c.enabled).map(|_| i))
            .collect();
        if indices.is_empty() {
            self.world = None;
            self.states.clear();
            self.intents.clear();
            self.jumps.clear();
            return Ok(());
        }
        let mut evaluation = SceneEvaluation::new(scene)?;
        self.states.retain(|id, _| {
            evaluation
                .index
                .position(id)
                .is_some_and(|i| scene.entities[i].character3d.is_some())
        });
        let world = self.world.get_or_insert_with(PhysicsWorld::new);
        world.sync_scene(scene, false)?;
        let active_rig = scene
            .entities
            .iter()
            .find(|e| e.camera.as_ref().is_some_and(|c| c.active))
            .and_then(|e| e.camera_rig.clone());
        for i in indices {
            let entity = &scene.entities[i];
            let config = entity.character3d.as_ref().unwrap();
            let (position, rotation, scale) = crate::physics3d::world_pose(
                evaluation.worlds[i].ok_or("Transformação física indisponível")?,
            )?;
            let uniform = character_scale(rotation, scale)?;
            let mut motion = config.motion(entity, uniform)?;
            let forward = rotation * -Vec3::Z;
            let initial_yaw = (-forward.x).atan2(-forward.z);
            let state = self
                .states
                .entry(entity.id.clone())
                .or_insert_with(|| CharacterState::new(position, initial_yaw));
            state.previous_position = position;
            state.position = position;
            if let Some(rig) = active_rig
                .as_ref()
                .filter(|r| r.target.as_deref() == Some(&entity.id))
                && state.look_blocks.is_empty()
            {
                (state.yaw, state.pitch) = rig.look(state.yaw, state.pitch, Vec2::from(input.look));
            }
            let automatic = if config.automatic_input {
                movement_axis(input, &config.actions)
            } else {
                Vec2::ZERO
            };
            let axis = if state.movement_blocks.is_empty() {
                self.intents
                    .remove(&entity.id)
                    .unwrap_or(automatic)
                    .clamp_length_max(1.)
            } else {
                self.intents.remove(&entity.id);
                Vec2::ZERO
            };
            let yaw = match config.reference {
                MovementReference::World => 0.,
                MovementReference::Body => initial_yaw,
                MovementReference::Camera => state.yaw,
            };
            let wish = wish_direction(axis, yaw) * config.speed;
            state.velocity.x = wish.x;
            state.velocity.z = wish.z;
            let jump = self.jumps.remove(&entity.id)
                || (config.automatic_input && input.pressed(&config.actions.jump));
            if jump && state.grounded && state.movement_blocks.is_empty() {
                state.velocity.y = config.jump_speed;
                state.grounded = false;
            }
            state.velocity.y -= config.gravity * dt;
            if state.velocity.y > 0. {
                motion.snap = 0.;
                motion.step_height = 0.;
            }
            let filter = &entity.physics3d.as_ref().unwrap().filter;
            let mut options = QueryOptions {
                category: filter.category,
                mask: filter.mask,
                ..QueryOptions::excluding(&entity.id)
            };
            options
                .exclude
                .extend(evaluation.index.descendants(scene, &entity.id));
            let resolved =
                world.move_capsule(position, state.velocity * dt, dt, motion, &options)?;
            state.position += resolved.delta;
            state.grounded = resolved.grounded;
            if state.grounded && state.velocity.y < 0. {
                state.velocity.y = 0.;
            }
            for hit in &resolved.contacts {
                // Ground support is resolved by the capsule solver. Feeding its
                // tiny contact-normal error back as upward velocity causes drift.
                if state.grounded && hit.normal.y >= motion.climb_degrees.to_radians().cos() {
                    continue;
                }
                let into = state.velocity.dot(hit.normal);
                if into < 0. {
                    state.velocity -= hit.normal * into;
                }
            }
            let next_world = Mat4::from_scale_rotation_translation(
                scale,
                Quat::from_rotation_y(state.yaw),
                state.position,
            );
            let parent = entity
                .parent
                .as_ref()
                .and_then(|id| evaluation.index.position(id))
                .and_then(|p| evaluation.worlds[p])
                .unwrap_or(Mat4::IDENTITY);
            let id = entity.id.clone();
            scene.entities[i].transform = crate::document::Transform::from_matrix(
                parent.inverse() * next_world,
                entity.transform.pivot,
            );
            evaluation.refresh_subtree(scene, &id);
            // The next character observes the body's new location this same tick.
            world.upsert(
                &id,
                scene.entities[i].physics3d.as_ref().unwrap(),
                state.position,
                Quat::from_rotation_y(state.yaw),
                scale,
            )?;
            world.flush();
        }
        self.intents.clear();
        self.jumps.clear();
        Ok(())
    }
}
impl Runtime {
    pub(super) fn move_characters(&mut self, input: &InputFrame) {
        let mut characters = std::mem::take(&mut self.characters);
        if let Err(error) = characters.step(self.scene_mut_internal(), input, FIXED_DT) {
            self.log(format!("Movimento 3D: {error}"));
        }
        self.characters = characters;
    }
    pub fn character_state(&self, id: &str) -> Option<&CharacterState> {
        self.characters.states.get(id)
    }
    /// One-tick intent. Commands submitted after movement apply on the next tick.
    pub fn set_movement_intent(&mut self, id: &str, axis: Vec2) -> Result<(), String> {
        if !axis.is_finite() {
            return Err("Intenção de movimento inválida.".into());
        }
        if self
            .entity(id)
            .and_then(|e| e.character3d.as_ref())
            .is_none()
        {
            return Err("Objeto não possui personagem 3D.".into());
        }
        self.characters
            .intents
            .insert(id.into(), axis.clamp_length_max(1.));
        Ok(())
    }
    pub fn request_jump(&mut self, id: &str) -> Result<(), String> {
        if self
            .entity(id)
            .and_then(|e| e.character3d.as_ref())
            .is_none()
        {
            return Err("Objeto não possui personagem 3D.".into());
        }
        self.characters.jumps.insert(id.into());
        Ok(())
    }
    pub fn wants_relative_mouse(&self) -> bool {
        self.scene()
            .entities
            .iter()
            .find(|e| e.camera.as_ref().is_some_and(|c| c.active))
            .is_some_and(|e| e.camera_rig.is_some())
    }
    /// Presentation-only pose. Unconsumed look is displayed immediately, without
    /// changing collision positions or consuming the same delta a second time.
    pub fn game_camera_pose(&self) -> Option<GameCameraPose> {
        let entity = self
            .scene()
            .entities
            .iter()
            .find(|e| e.camera.as_ref().is_some_and(|c| c.active))?;
        let rig = entity.camera_rig.as_ref()?;
        let target = rig.target.as_deref()?;
        let world = self.scene().world_matrix(target).ok()?;
        let (position, rotation, scale) = crate::physics3d::world_pose(world).ok()?;
        let forward = rotation * -Vec3::Z;
        let initial = CharacterState::new(position, (-forward.x).atan2(-forward.z));
        let state = self.characters.states.get(target).unwrap_or(&initial);
        let pending = if state.look_blocks.is_empty() {
            Vec2::from(self.pending_look)
                + self
                    .input_timeline
                    .unconsumed_look(self.time + f64::from(self.accumulator))
        } else {
            Vec2::ZERO
        };
        let (yaw, pitch) = rig.look(state.yaw, state.pitch, pending);
        let alpha = (self.accumulator / FIXED_DT).clamp(0., 1.);
        Some(GameCameraPose {
            position: state.previous_position.lerp(state.position, alpha)
                + Vec3::Y * rig.eye_height * scale.y,
            rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch),
            fov: entity.camera.as_ref()?.fov,
            hidden: rig.hidden.clone(),
        })
    }
}
