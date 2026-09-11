use super::*;
use crate::character::*;
use crate::physics3d::{PhysicsWorld, QueryOptions};
use glam::{Mat4, Quat, Vec2};
mod commands;
mod motor;
mod platforms;
mod sensors;
pub use sensors::SensorCrossing;

#[derive(Default)]
pub(super) struct Characters {
    pub(super) world: Option<PhysicsWorld>,
    pub(super) states: HashMap<Id, CharacterState>,
    pub(super) intents: HashMap<Id, Vec2>,
    pub(super) jumps: HashSet<Id>,
    pub(super) sprints: HashMap<Id, bool>,
    pub(super) crouches: HashMap<Id, bool>,
    pub(super) slides: HashSet<Id>,
    pub(super) looped: HashSet<Id>,
    pub(super) ignored_tracks: HashSet<(Id, Id, Id)>,
    platforms: HashMap<Id, platforms::PlatformFrame>,
    overrides: HashMap<Id, (crate::physics3d::Collider3d, Mat4)>,
    events: Vec<(Id, MovementEvent)>,
    paths: HashMap<Id, Vec<(Vec3, Vec3)>>,
    lateral: HashSet<(Id, Id)>,
    warnings: Vec<(Id, String)>,
    reported: HashMap<Id, String>,
    sensor_pairs: HashSet<(Id, Id)>,
    sensor_events: Vec<SensorCrossing>,
}
impl Characters {
    pub(super) fn presentation_worlds(&self, alpha: f32) -> HashMap<Id, Mat4> {
        let mut worlds = HashMap::with_capacity(self.states.len() + self.platforms.len());
        for (id, platform) in &self.platforms {
            if !platform.discontinuous {
                let mut matrix = platform.matrix;
                let position = matrix.w_axis.truncate() - platform.delta * (1. - alpha);
                matrix.w_axis = position.extend(1.);
                worlds.insert(id.clone(), matrix);
            }
        }
        for (id, state) in &self.states {
            if state.uniform_scale > 0. {
                let rotation = Quat::from_rotation_y(state.previous_yaw)
                    .slerp(Quat::from_rotation_y(state.yaw), alpha);
                worlds.insert(
                    id.clone(),
                    Mat4::from_scale_rotation_translation(
                        Vec3::splat(state.uniform_scale),
                        rotation,
                        interpolated_path(
                            self.paths.get(id),
                            state.previous_position,
                            state.position,
                            alpha,
                        ),
                    ),
                );
            }
        }
        worlds
    }
    pub(super) fn release_input(&mut self) {
        self.intents.clear();
        self.jumps.clear();
        self.sprints.clear();
        self.crouches.clear();
        self.slides.clear();
        self.paths.clear();
        for state in self.states.values_mut() {
            state.jump_until = None;
            state.want_crouch = false;
            state.sprinting = false;
            state.previous_position = state.position;
            state.previous_eye_height = state.eye_height;
            state.previous_yaw = state.yaw;
        }
    }
    fn step(
        &mut self,
        scene: &mut Scene,
        surfaces: &[crate::surface::SurfaceMaterial],
        input: &InputFrame,
        control: Option<&super::cameras::CameraControl>,
        time: f64,
        dt: f32,
    ) -> Result<(), String> {
        if scene.kind != SceneKind::ThreeD {
            return Ok(());
        }
        let has_motion = scene.entities.iter().any(|e| {
            e.character3d.is_some()
                || e.camera_rig.is_some()
                || e.platform.as_ref().is_some_and(|p| p.enabled)
        });
        if !has_motion {
            *self = Default::default();
            return Ok(());
        }
        let mut evaluation = SceneEvaluation::new(scene)?;
        self.platforms =
            platforms::advance(scene, &mut evaluation, &self.platforms, &self.looped, dt)?;
        for (id, platform) in &self.platforms {
            if platform.discontinuous {
                self.warnings.push((id.clone(),"Plataforma com rotação, escala alterada ou salto de posição: apoio liberado sem herdar velocidade.".into()));
            }
        }
        self.ignored_tracks.retain(|(owner, clip, target)| {
            evaluation
                .index
                .position(owner)
                .is_some_and(|i| scene.entities[i].clips.iter().any(|c| c.id == *clip))
                && evaluation.index.position(target).is_some()
        });
        self.looped.clear();
        self.events.clear();
        self.paths.clear();
        self.states.retain(|id, _| {
            evaluation
                .index
                .position(id)
                .is_some_and(|i| scene.entities[i].character3d.is_some())
        });
        let indices: Vec<_> = scene
            .entities
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.character3d.as_ref().map(|_| i))
            .collect();
        self.overrides.clear();
        for &i in &indices {
            let entity = &scene.entities[i];
            let matrix = evaluation.worlds[i].ok_or("Transformação física ausente")?;
            let (position, rotation, scale) = crate::physics3d::world_pose(matrix)?;
            if !self.states.contains_key(&entity.id) {
                character_scale(rotation, scale)?;
            }
            let forward = rotation * -Vec3::Z;
            let state = self
                .states
                .entry(entity.id.clone())
                .or_insert_with(|| CharacterState::new(position, (-forward.x).atan2(-forward.z)));
            if state.uniform_scale <= 0. {
                state.uniform_scale = scale.x;
            }
            let scale = Vec3::splat(state.uniform_scale);
            let motion = entity
                .character3d
                .as_ref()
                .unwrap()
                .motion(entity, scale.x)?;
            if state.height <= 0. {
                state.height = motion.height;
            }
            state.previous_position = state.position;
            state.previous_yaw = state.yaw;
            let world = Mat4::from_scale_rotation_translation(
                scale,
                Quat::from_rotation_y(state.yaw),
                state.position,
            );
            self.overrides.insert(
                entity.id.clone(),
                (actual_collider(entity, state, scale.x), world),
            );
        }
        let world = self.world.get_or_insert_with(PhysicsWorld::new);
        world.sync_evaluated(scene, &evaluation, &self.overrides)?;
        let active_rig = control.and_then(|c| c.rig.as_ref());
        let mut lateral = HashSet::new();
        for i in indices {
            let entity = &scene.entities[i];
            let id = entity.id.clone();
            let config = entity.character3d.as_ref().unwrap();
            let scale = Vec3::splat(self.states[&id].uniform_scale);
            let standing = config.motion(entity, scale.x)?;
            let state = self.states.get_mut(&id).unwrap();
            let before = state.clone();
            if let Some(control) = control {
                state.look_yaw = control.yaw;
                if active_rig.is_some_and(|r| r.target.as_deref() == Some(&id)) {
                    state.pitch = control.pitch;
                    if config.enabled && control.mode == CameraMode::FirstPerson {
                        state.yaw = control.yaw;
                    }
                }
            }
            let automatic = if config.automatic_input {
                movement_axis(input, &config.actions)
            } else {
                Vec2::ZERO
            };
            let mut axis = self
                .intents
                .remove(&id)
                .unwrap_or(automatic)
                .clamp_length_max(1.);
            let mut jump = self.jumps.remove(&id)
                || (config.automatic_input && input.pressed(&config.actions.jump));
            let mut jump_held = config.automatic_input && input.held(&config.actions.jump);
            let mut slide = self.slides.remove(&id);
            let mut sprint = self
                .sprints
                .get(&id)
                .copied()
                .unwrap_or(config.automatic_input && input.held(&config.sprint_action));
            let mut crouch_override = self.crouches.get(&id).copied();
            let mut crouch = crouch_override
                .unwrap_or(config.automatic_input && input.held(&config.crouch_action));
            let mut crouch_pressed = config.automatic_input && input.pressed(&config.crouch_action);
            if !state.movement_blocks.is_empty() {
                axis = Vec2::ZERO;
                jump = false;
                jump_held = false;
                slide = false;
                sprint = false;
                crouch = false;
                crouch_pressed = false;
                crouch_override = Some(false);
            }
            let first_person = control.is_some_and(|c| {
                c.mode == CameraMode::FirstPerson
                    && c.rig
                        .as_ref()
                        .is_some_and(|r| r.target.as_deref() == Some(&id))
            });
            if config.enabled && !first_person {
                let reference = match config.reference {
                    MovementReference::World => 0.,
                    MovementReference::Body => state.yaw,
                    MovementReference::Camera => state.look_yaw,
                };
                let direction = wish_direction(axis, reference);
                let target = if config.facing == BodyFacing::Look {
                    Some(state.look_yaw)
                } else if direction.length_squared() > 1e-8 {
                    Some((-direction.x).atan2(-direction.z))
                } else {
                    None
                };
                if let Some(target) = target {
                    let delta = (target - state.yaw).sin().atan2((target - state.yaw).cos());
                    state.yaw += delta.clamp(
                        -config.angular_speed.to_radians() * dt,
                        config.angular_speed.to_radians() * dt,
                    );
                }
            }
            let filter = &entity.physics3d.as_ref().unwrap().filter;
            let mut options = QueryOptions {
                category: filter.category,
                mask: filter.mask,
                ..QueryOptions::excluding(&id)
            };
            options
                .exclude
                .extend(evaluation.index.descendants(scene, &id));
            let context = motor::MotorContext {
                world,
                config,
                standing,
                options,
                surfaces,
                platforms: &self.platforms,
                scene,
                evaluation: &evaluation,
                time,
                dt,
                scale: scale.x,
            };
            let result = if config.enabled {
                context.advance(
                    state,
                    motor::MotorInput {
                        axis,
                        jump,
                        sprint,
                        crouch,
                        crouch_pressed,
                        crouch_override,
                        jump_held,
                        slide,
                    },
                )
            } else {
                Ok(motor::MotorStep::default())
            };
            match result {
                Err(error) => {
                    *state = before;
                    self.warnings.push((id, error));
                    continue;
                }
                Ok(result) => {
                    if let Some(warning) = result.warning {
                        self.warnings.push((id.clone(), warning));
                    }
                    self.events
                        .extend(result.events.into_iter().map(|event| (id.clone(), event)));
                    self.paths.insert(id.clone(), result.path);
                    for hit in result.lateral {
                        let pair = (id.clone(), hit.object.clone());
                        if lateral.insert(pair.clone()) && !self.lateral.contains(&pair) {
                            self.events.push((
                                id.clone(),
                                MovementEvent::SideContact {
                                    object: hit.object,
                                    point: hit.point,
                                    normal: hit.normal,
                                },
                            ));
                        }
                    }
                }
            }
            if let Some(rig) = active_rig
                .as_ref()
                .filter(|r| r.target.as_deref() == Some(&id))
            {
                let desired = if state.posture != Posture::Standing {
                    rig.crouched_eye_height
                } else {
                    rig.eye_height
                } * scale.y;
                if state.eye_height == 0. {
                    state.eye_height = desired;
                }
                state.previous_eye_height = state.eye_height;
                let blend = if rig.posture_smoothing <= 0. {
                    1.
                } else {
                    1. - (-dt / rig.posture_smoothing).exp()
                };
                state.eye_height += (desired - state.eye_height) * blend;
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
            let transform = crate::document::Transform::from_matrix(
                parent.inverse() * next_world,
                entity.transform.pivot,
            );
            if !transform.finite()
                || !transform
                    .matrix()
                    .abs_diff_eq(parent.inverse() * next_world, 1e-4)
            {
                *state = before;
                self.warnings.push((id,"O parentesco animado produziria deformação da raiz física; anime as peças filhas.".into()));
                continue;
            }
            scene.entities[i].transform = transform;
            self.overrides.insert(
                id.clone(),
                (
                    actual_collider(&scene.entities[i], state, scale.x),
                    next_world,
                ),
            );
            evaluation.refresh_subtree(scene, &id);
            for child in evaluation.index.descendants(scene, &id) {
                let index = evaluation.index.position(&child).unwrap();
                if let Some(matrix) = self
                    .overrides
                    .get(&child)
                    .map(|(_, matrix)| *matrix)
                    .or(evaluation.worlds[index])
                {
                    world.sync_entity(
                        &scene.entities[index],
                        matrix,
                        self.overrides.get(&child).map(|(c, _)| c),
                    )?;
                }
            }
            world.flush();
        }
        self.lateral = lateral;
        self.intents.clear();
        self.jumps.clear();
        self.slides.clear();
        self.sprints.retain(|id, _| self.states.contains_key(id));
        self.crouches.retain(|id, _| self.states.contains_key(id));
        Ok(())
    }
}
/// Follow the resolved route, including an autostep's lift/over/drop waypoints.
/// Interpolating a straight chord through a step would visibly cross geometry.
fn interpolated_path(
    path: Option<&Vec<(Vec3, Vec3)>>,
    previous: Vec3,
    current: Vec3,
    alpha: f32,
) -> Vec3 {
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return previous.lerp(current, alpha);
    };
    let total: f32 = path.iter().map(|(a, b)| a.distance(*b)).sum();
    if total <= 1e-6 {
        return current;
    }
    let mut remaining = total * alpha;
    for (a, b) in path {
        let length = a.distance(*b);
        if length > 1e-6 && remaining <= length {
            return a.lerp(*b, remaining / length);
        }
        remaining -= length;
    }
    current
}
fn actual_collider(
    entity: &crate::document::Entity,
    state: &CharacterState,
    scale: f32,
) -> crate::physics3d::Collider3d {
    let mut result = entity.physics3d.as_ref().unwrap().clone();
    if let crate::physics3d::CollisionShape::Capsule { height, .. } = &mut result.shape {
        *height = state.height / scale;
        result.center = [0., *height * 0.5, 0.];
    }
    result
}
impl Runtime {
    pub(super) fn move_characters(&mut self, input: &InputFrame) {
        if self.scene().kind != SceneKind::ThreeD {
            return;
        }
        if !self.scene().entities.iter().any(|e| {
            e.character3d.is_some()
                || e.camera_rig.is_some()
                || e.platform.as_ref().is_some_and(|p| p.enabled)
        }) {
            return;
        }
        let mut cameras = std::mem::take(&mut self.cameras);
        let control = match cameras.control(self.scene(), &self.characters, input) {
            Ok(control) => control,
            Err(e) => {
                let id = cameras
                    .active(self.scene())
                    .map_or_else(|| self.scene_id.clone(), |e| e.id.clone());
                cameras.report(self, id, e);
                None
            }
        };
        self.cameras = cameras;
        let mut characters = std::mem::take(&mut self.characters);
        let scene = self
            .project
            .scenes
            .iter_mut()
            .find(|s| s.id == self.scene_id)
            .unwrap();
        if let Err(error) = characters.step(
            scene,
            &self.project.surfaces,
            input,
            control.as_ref(),
            self.time,
            FIXED_DT,
        ) {
            self.log(format!("Movimento 3D: {error}"));
        }
        for (id, error) in std::mem::take(&mut characters.warnings) {
            if characters.reported.get(&id) != Some(&error) {
                self.log(format!("Personagem {id}: {error}"));
                characters.reported.insert(id, error);
            }
        }
        characters
            .reported
            .retain(|id, _| self.entity(id).is_some());
        self.characters = characters;
    }
    pub fn character_state(&self, id: &str) -> Option<&CharacterState> {
        self.characters.states.get(id)
    }
    pub fn physics_world(&self) -> Option<&PhysicsWorld> {
        self.characters.world.as_ref()
    }
    pub fn presentation_worlds(&self) -> Option<Arc<HashMap<Id, Mat4>>> {
        self.cameras.worlds.clone()
    }
    /// Events from the last fixed step, with data captured at the transition.
    pub fn movement_events(&self) -> &[(Id, MovementEvent)] {
        &self.characters.events
    }
    pub fn sensor_events(&self) -> &[SensorCrossing] {
        &self.characters.sensor_events
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
        self.cameras.relative(self.scene())
    }
    /// Presentation-only pose. Unconsumed look is displayed immediately, without
    /// changing collision positions or consuming the same delta a second time.
    pub fn game_camera_pose(&self) -> Option<GameCameraPose> {
        if let Some(pose) = &self.cameras.pose {
            return Some(pose.clone());
        }
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
                + Vec3::Y
                    * if state.eye_height > 0. {
                        state.previous_eye_height
                            + (state.eye_height - state.previous_eye_height) * alpha
                    } else {
                        rig.eye_height * scale.y
                    },
            rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch),
            fov: entity.camera.as_ref()?.fov,
            hidden: rig.hidden.clone(),
        })
    }
}
