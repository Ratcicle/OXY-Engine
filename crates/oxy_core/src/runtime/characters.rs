use super::*;
use crate::character::*;
use crate::physics3d::{ColliderOverride, PhysicsWorld, QueryOptions};
use glam::{Mat4, Quat, Vec2};
mod commands;
mod motor;
mod platforms;
mod sensors;
mod teleport;
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
    overrides: HashMap<Id, ColliderOverride>,
    events: Vec<(Id, MovementEvent)>,
    records: Vec<MovementRecord>,
    pending_records: Vec<MovementRecord>,
    paths: HashMap<Id, Vec<(Vec3, Vec3)>>,
    lateral: HashSet<(Id, Id)>,
    warnings: Vec<(Id, String)>,
    reported: HashMap<Id, String>,
    /// (character, platform) pairs already warned about a released support: once per scene
    /// load, however often the support switches between platforms.
    released: HashSet<(Id, Id)>,
    /// How far the platforms under them turned the characters this step, in scene order: the
    /// first person view turns with them.
    turns: Vec<(Id, f32)>,
    sensor_pairs: HashSet<(Id, Id)>,
    sensor_events: Vec<SensorCrossing>,
}
impl Characters {
    /// What the state digest reads of character `id` (see `Runtime::state_digest`): its state
    /// and what logic asked of it for the coming step. None when it is not a character.
    pub(super) fn write_character(
        &self,
        id: &str,
        out: &mut dyn std::fmt::Write,
    ) -> Option<std::fmt::Result> {
        let state = self.states.get(id)?;
        Some(write!(
            out,
            "{state:?};{:?};{};{:?};{:?};{};{:?}",
            self.intents.get(id),
            self.jumps.contains(id),
            self.sprints.get(id),
            self.crouches.get(id),
            self.slides.contains(id),
            self.paths.get(id)
        ))
    }
    /// The frame platform `id` moved by in the last step, which carries whoever stands on it.
    pub(super) fn write_platform(
        &self,
        id: &str,
        out: &mut dyn std::fmt::Write,
    ) -> Option<std::fmt::Result> {
        self.platforms.get(id).map(|frame| write!(out, "{frame:?}"))
    }
    /// The rest the state digest reads, belonging to no one character: pairs in contact,
    /// warnings given, and movement records and sensor crossings still to come.
    pub(super) fn write_shared(&self, out: &mut dyn std::fmt::Write) -> std::fmt::Result {
        use super::state_hash::sorted;
        // Every field, so that a new one is read somewhere or left out on purpose.
        let Self {
            // Synced from the scene every step.
            world: _,
            overrides: _,
            // Handed to the cameras in the step that turned them.
            turns: _,
            // Read per object by write_character and write_platform.
            states: _,
            intents: _,
            jumps: _,
            sprints: _,
            crouches: _,
            slides: _,
            paths: _,
            platforms: _,
            sensor_pairs,
            lateral,
            released,
            ignored_tracks,
            looped,
            reported,
            events,
            records,
            pending_records,
            sensor_events,
            warnings,
        } = self;
        write!(
            out,
            "{:?};{:?};{:?};{:?};{:?};{:?};{events:?};{records:?};{pending_records:?};\
             {sensor_events:?};{warnings:?}",
            sorted(sensor_pairs),
            sorted(lateral),
            sorted(released),
            sorted(ignored_tracks),
            sorted(looped),
            sorted(reported),
        )
    }
    pub(super) fn presentation_worlds(&self, alpha: f32) -> HashMap<Id, Mat4> {
        let mut worlds = HashMap::with_capacity(self.states.len() + self.platforms.len());
        for (id, platform) in &self.platforms {
            if let Some(matrix) = platform.shown(alpha) {
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
        for state in self.states.values_mut() {
            state.jump_until = None;
            state.want_crouch = false;
            state.sprinting = false;
        }
        self.settle();
    }
    /// Shows everyone, and the platforms, where the last step left them, with no movement still
    /// to interpolate, as a pause starts or ends. What logic asked of them stays, a toggled
    /// crouch too.
    pub(super) fn settle(&mut self) {
        self.paths.clear();
        for state in self.states.values_mut() {
            state.previous_position = state.position;
            state.previous_eye_height = state.eye_height;
            state.previous_yaw = state.yaw;
        }
        for frame in self.platforms.values_mut() {
            frame.settle();
        }
    }
    /// Drops the one-step requests logic made (movement, jumps, slides), which the game pause
    /// kept from running.
    pub(super) fn forget_requests(&mut self) {
        self.intents.clear();
        self.jumps.clear();
        self.slides.clear();
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
        #[cfg(feature = "profiling")]
        let prepare_started = std::time::Instant::now();
        let mut evaluation = SceneEvaluation::new(scene)?;
        // A platform that turns too fast, scales or jumps is reported by the characters it
        // drops (motor), not on its own: with nobody on it, nothing goes wrong.
        self.platforms =
            platforms::advance(scene, &mut evaluation, &self.platforms, &self.looped, dt)?;
        self.ignored_tracks.retain(|(owner, clip, target)| {
            evaluation
                .index
                .position(owner)
                .is_some_and(|i| scene.entities[i].clips.iter().any(|c| c.id == *clip))
                && evaluation.index.position(target).is_some()
        });
        self.looped.clear();
        self.records = std::mem::take(&mut self.pending_records);
        self.events.clear();
        self.events.extend(
            self.records
                .iter()
                .map(|r| (r.object.clone(), r.event.clone())),
        );
        self.paths.clear();
        self.turns.clear();
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
            self.overrides.insert(
                entity.id.clone(),
                ColliderOverride {
                    config: actual_collider(entity, state, scale.x),
                    position: state.position,
                    rotation: Quat::from_rotation_y(state.yaw),
                    scale,
                },
            );
        }
        let world = self.world.get_or_insert_with(PhysicsWorld::new);
        world.sync_evaluated(scene, &evaluation, &self.overrides)?;
        #[cfg(feature = "profiling")]
        crate::metrics::count(|c| {
            c.character_prepare_ns += prepare_started.elapsed().as_nanos() as u64
        });
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
                let direction = wish_direction(axis, config.reference.yaw(state));
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
            let filter = &config.body.filter;
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
            let events_before = self.events.len();
            let records_before = self.records.len();
            let turn;
            let result = if config.enabled {
                crate::metrics::timed(
                    || {
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
                    },
                    |c, ns| c.character_motor_ns += ns,
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
                    turn = result.turn;
                    if let Some(warning) = result.warning {
                        self.warnings.push((id.clone(), warning));
                    }
                    if let Some(platform) = result.released
                        && self.released.insert((id.clone(), platform.clone()))
                    {
                        let name = evaluation
                            .index
                            .position(&platform)
                            .map_or(platform.as_str(), |i| scene.entities[i].name.as_str());
                        self.warnings.push((
                            id.clone(),
                            format!(
                                "Apoio em {name} liberado sem herdar velocidade, porque a \
                                 plataforma girou rápido demais, mudou de escala ou saltou de \
                                 posição."
                            ),
                        ));
                    }
                    for (event, snapshot) in result.events {
                        self.events.push((id.clone(), event.clone()));
                        self.records.push(MovementRecord {
                            object: id.clone(),
                            event,
                            state: snapshot,
                        });
                    }
                    self.paths.insert(id.clone(), result.path);
                    for hit in result.lateral {
                        let pair = (id.clone(), hit.object.clone());
                        if lateral.insert(pair.clone()) && !self.lateral.contains(&pair) {
                            self.records.push(MovementRecord {
                                object: id.clone(),
                                event: MovementEvent::SideContact {
                                    object: hit.object.clone(),
                                    point: hit.point,
                                    normal: hit.normal,
                                },
                                state: Arc::new(state.clone()),
                            });
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
                self.events.truncate(events_before);
                self.records.truncate(records_before);
                self.paths.remove(&id);
                lateral.retain(|(owner, _)| owner != &id);
                self.warnings.push((id,"O parentesco animado produziria deformação da raiz física; anime as peças filhas.".into()));
                continue;
            }
            scene.entities[i].transform = transform;
            if turn != 0. {
                self.turns.push((id.clone(), turn));
            }
            self.overrides.insert(
                id.clone(),
                ColliderOverride {
                    config: actual_collider(&scene.entities[i], state, scale.x),
                    position: state.position,
                    rotation: Quat::from_rotation_y(state.yaw),
                    scale,
                },
            );
            evaluation.refresh_subtree(scene, &id);
            for child in evaluation.index.descendants(scene, &id) {
                let index = evaluation.index.position(&child).unwrap();
                if let Some(pose) = self.overrides.get(&child) {
                    world.upsert(
                        &child,
                        &pose.config,
                        pose.position,
                        pose.rotation,
                        pose.scale,
                    )?;
                } else if let Some(matrix) = evaluation.worlds[index] {
                    world.sync_entity(&scene.entities[index], matrix, None)?;
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
    let config = entity.character3d.as_ref().unwrap();
    let _ = scale;
    config
        .body
        .collider(state.posture != Posture::Standing, config.crouch_height)
        .expect("validated movement body")
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
        let control = match cameras.control(self.scene(), &self.characters, &InputFrame::default())
        {
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
        // Timing windows (coyote time, jump buffer) run on the game clock, across the pause.
        let time = self.game_time();
        let scene = self
            .project
            .scenes
            .iter_mut()
            .find(|s| s.id == self.scene_id)
            .unwrap();
        let result = characters.step(
            scene,
            &self.project.surfaces,
            input,
            control.as_ref(),
            time,
            FIXED_DT,
        );
        let failed = result.is_err();
        if let Err(error) = result {
            self.log(format!("Movimento 3D: {error}"));
        }
        let scene = self
            .project
            .scenes
            .iter()
            .find(|s| s.id == self.scene_id)
            .unwrap();
        self.cameras
            .carry(scene, &std::mem::take(&mut characters.turns));
        for (id, error) in std::mem::take(&mut characters.warnings) {
            if characters.reported.get(&id) != Some(&error) {
                self.log(format!("Personagem {id}: {error}"));
                characters.reported.insert(id, error);
            }
        }
        characters
            .reported
            .retain(|id, _| self.entity(id).is_some());
        characters.released.retain(|(character, platform)| {
            self.entity(character).is_some() && self.entity(platform).is_some()
        });
        self.characters = characters;
        self.physics_dirty = failed;
    }
    pub fn character_state(&self, id: &str) -> Option<&CharacterState> {
        self.characters.states.get(id)
    }
    /// The yaw character `id`'s movement axis turns with, as its movement reference picks it in
    /// the step (see [`wish_direction`]): 0 for World, the body's for Body, the camera's for
    /// Camera. None when `id` is not a character of the running scene.
    pub fn movement_yaw(&self, id: &str) -> Option<f32> {
        let config = self.entity(id)?.character3d.as_ref()?;
        Some(config.reference.yaw(self.characters.states.get(id)?))
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
    pub fn movement_records(&self) -> &[MovementRecord] {
        &self.characters.records
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
    /// Whether the host should grab the cursor: a camera that turns with the mouse, outside the
    /// game pause, whose menu needs a free cursor.
    pub fn wants_relative_mouse(&self) -> bool {
        !self.game_paused && self.cameras.relative(self.scene())
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
        let pending = if state.look_blocks.is_empty() && !self.game_paused {
            Vec2::from(self.pending_look)
                + self
                    .input_timeline
                    .unconsumed_look(self.time + f64::from(self.accumulator))
        } else {
            Vec2::ZERO
        };
        let (yaw, pitch) = rig.look(state.yaw, state.pitch, pending);
        let alpha = self.presentation_alpha();
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
