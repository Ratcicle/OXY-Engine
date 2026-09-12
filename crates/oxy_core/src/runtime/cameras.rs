//! Evaluated cameras share presentation poses with rendering, never motor authority.
use super::*;
use crate::{
    character::*,
    physics3d::{CollisionShape, PhysicsWorld, QueryOptions},
    scene_view::SceneView,
};
use glam::{Mat4, Quat, Vec2};

#[derive(Clone)]
pub(super) struct CameraControl {
    pub rig: Option<CameraRig>,
    pub yaw: f32,
    pub pitch: f32,
    pub mode: CameraMode,
}
#[derive(Clone)]
struct RigState {
    yaw: f32,
    pitch: f32,
    distance: f32,
    authored_distance: f32,
    shoulder_sign: f32,
    mode_override: Option<CameraMode>,
    look_at: Option<CameraLookAt>,
    pivot: Option<Vec3>,
    shown_distance: Option<f32>,
    rotation: Option<Quat>,
}
struct Transition {
    from: GameCameraPose,
    elapsed: f32,
    duration: f32,
}
#[derive(Default)]
pub(super) struct Cameras {
    debug: Option<CameraDebug>,
    active_override: Option<Id>,
    states: HashMap<Id, RigState>,
    pub(super) worlds: Option<Arc<HashMap<Id, Mat4>>>,
    query_world: Option<PhysicsWorld>,
    pub(super) pose: Option<GameCameraPose>,
    aspect: f32,
    reported: HashMap<Id, String>,
    key: Option<(Id, CameraMode, Option<Id>)>,
    transition: Option<Transition>,
    next_transition: Option<f32>,
}
fn angles(rotation: Quat) -> (f32, f32) {
    let forward = rotation * -Vec3::Z;
    (
        (-forward.x).atan2(-forward.z),
        forward.y.clamp(-1., 1.).asin(),
    )
}
fn blend(dt: f32, tau: f32) -> f32 {
    if tau <= 0. {
        1.
    } else {
        1. - (-dt / tau).exp()
    }
}
fn focus_point(focus: &CameraLookAt, view: &SceneView<'_>) -> Result<Vec3, String> {
    match focus {
        CameraLookAt::Point(p) => Ok(*p),
        CameraLookAt::Object(id) => view
            .world_matrix(id)
            .map(|m| m.w_axis.truncate())
            .map_err(|_| "Alvo do olhar assistido ausente; última vista preservada.".into()),
    }
}
fn look_rotation(from: Vec3, point: Vec3) -> Option<Quat> {
    let d = point - from;
    (d.length_squared() > 1e-8).then(|| {
        Quat::from_rotation_y((-d.x).atan2(-d.z))
            * Quat::from_rotation_x((d.y / d.length()).clamp(-1., 1.).asin())
    })
}
impl Cameras {
    pub(super) fn report(&mut self, runtime: &mut Runtime, id: Id, warning: String) {
        if self.reported.get(&id) != Some(&warning) {
            runtime.log(format!("Câmera {id}: {warning}"));
            self.reported.insert(id, warning);
        }
    }
    pub(super) fn active<'a>(&self, scene: &'a Scene) -> Option<&'a crate::document::Entity> {
        self.active_override
            .as_ref()
            .and_then(|id| scene.entity(id))
            .filter(|e| e.camera.is_some())
            .or_else(|| {
                scene
                    .entities
                    .iter()
                    .find(|e| e.camera.as_ref().is_some_and(|c| c.active))
            })
    }
    fn mode(&self, entity: &crate::document::Entity) -> CameraMode {
        entity
            .camera_rig
            .as_ref()
            .map(|r| {
                self.states
                    .get(&entity.id)
                    .and_then(|s| s.mode_override)
                    .unwrap_or(r.mode)
            })
            .unwrap_or(CameraMode::Fixed)
    }
    pub(super) fn relative(&self, scene: &Scene) -> bool {
        self.active(scene)
            .is_some_and(|e| self.mode(e) != CameraMode::Fixed)
    }
    pub(super) fn accepts_look(&self, scene: &Scene, characters: &characters::Characters) -> bool {
        self.active(scene).is_some_and(|e| {
            self.mode(e) != CameraMode::Fixed
                && self.states.get(&e.id).is_none_or(|s| s.look_at.is_none())
                && e.camera_rig
                    .as_ref()
                    .and_then(|r| r.target.as_ref())
                    .and_then(|id| characters.states.get(id))
                    .is_none_or(|s| s.look_blocks.is_empty())
        })
    }
    pub(super) fn control(
        &mut self,
        scene: &Scene,
        characters: &characters::Characters,
        input: &InputFrame,
    ) -> Result<Option<CameraControl>, String> {
        let Some(entity) = self.active(scene) else {
            return Ok(None);
        };
        let Some(rig) = &entity.camera_rig else {
            let (_, rotation, _) = crate::physics3d::world_pose(scene.world_matrix(&entity.id)?)?;
            let (yaw, pitch) = angles(rotation);
            return Ok(Some(CameraControl {
                rig: None,
                yaw,
                pitch,
                mode: CameraMode::Fixed,
            }));
        };
        rig.validate_settings()?;
        let target = rig.target.as_ref().and_then(|id| characters.states.get(id));
        let initial = target
            .map(|s| (s.look_yaw, s.pitch))
            .or_else(|| {
                rig.target
                    .as_ref()
                    .and_then(|id| scene.world_matrix(id).ok())
                    .and_then(|w| crate::physics3d::world_pose(w).ok())
                    .map(|(_, r, _)| angles(r))
            })
            .unwrap_or((0., 0.));
        let state = self.states.entry(entity.id.clone()).or_insert(RigState {
            yaw: initial.0,
            pitch: initial.1,
            distance: rig.distance,
            authored_distance: rig.distance,
            shoulder_sign: 1.,
            mode_override: None,
            look_at: None,
            pivot: None,
            shown_distance: None,
            rotation: None,
        });
        if state.authored_distance != rig.distance {
            state.distance = rig.distance;
            state.authored_distance = rig.distance;
        }
        if input.pressed(&rig.mode_action) {
            state.mode_override = Some(
                if state.mode_override.unwrap_or(rig.mode) == CameraMode::FirstPerson {
                    CameraMode::ThirdPerson
                } else {
                    CameraMode::FirstPerson
                },
            );
        }
        let mode = state.mode_override.unwrap_or(rig.mode);
        if input.pressed(&rig.shoulder_action) {
            state.shoulder_sign = -state.shoulder_sign;
        }
        if mode == CameraMode::ThirdPerson && input.wheel.is_finite() {
            state.distance = (f64::from(state.distance)
                - f64::from(input.wheel) * f64::from(rig.zoom_step))
            .clamp(f64::from(rig.min_distance), f64::from(rig.max_distance))
                as f32;
        }
        if mode == CameraMode::Fixed {
            let (_, rotation, _) = crate::physics3d::world_pose(scene.world_matrix(&entity.id)?)?;
            let (yaw, pitch) = angles(rotation);
            return Ok(Some(CameraControl {
                rig: Some(rig.clone()),
                yaw,
                pitch,
                mode,
            }));
        }
        if state.look_at.is_none() && target.is_none_or(|s| s.look_blocks.is_empty()) {
            (state.yaw, state.pitch) = rig.look(state.yaw, state.pitch, Vec2::from(input.look));
        }
        if let Some(focus) = &state.look_at {
            let view = SceneView::new(scene);
            let point = focus_point(focus, &view)?;
            let origin = {
                rig.target
                    .as_ref()
                    .and_then(|id| view.world_matrix(id).ok())
                    .map(|m| {
                        m.w_axis.truncate()
                            + Vec3::Y
                                * target
                                    .filter(|s| s.eye_height > 0.)
                                    .map_or(rig.eye_height, |s| s.eye_height)
                    })
            }
            .unwrap_or(Vec3::ZERO);
            if let Some(rotation) = look_rotation(origin, point) {
                (state.yaw, state.pitch) = angles(rotation);
            }
        }
        Ok(Some(CameraControl {
            rig: Some(rig.clone()),
            yaw: state.yaw,
            pitch: state.pitch,
            mode,
        }))
    }
    fn present(
        &mut self,
        scene: &Scene,
        characters: &characters::Characters,
        alpha: f32,
        pending: Vec2,
        dt: f32,
    ) -> Result<(), String> {
        self.states
            .retain(|id, _| scene.entity(id).is_some_and(|e| e.camera_rig.is_some()));
        self.reported.retain(|id, _| scene.entity(id).is_some());
        let worlds = Arc::new(characters.presentation_worlds(alpha));
        self.worlds = Some(worlds.clone());
        let Some(entity) = self.active(scene) else {
            self.pose = None;
            self.transition = None;
            self.key = None;
            return Ok(());
        };
        let mode = self.mode(entity);
        let rig = entity.camera_rig.as_ref();
        let key = (entity.id.clone(), mode, rig.and_then(|r| r.target.clone()));
        let changed = self.key.as_ref() != Some(&key);
        if changed {
            let duration = self
                .next_transition
                .take()
                .unwrap_or_else(|| rig.map_or(0., |r| r.transition_seconds));
            self.transition = self
                .pose
                .clone()
                .filter(|_| duration > 0.)
                .map(|from| Transition {
                    from,
                    elapsed: 0.,
                    duration,
                });
            self.key = Some(key);
            if let Some(state) = self.states.get_mut(&entity.id) {
                state.pivot = None;
                state.shown_distance = None;
                state.rotation = None;
            }
        }
        let view = SceneView::with_worlds(scene, worlds.clone());
        if let Some(source) = &characters.world {
            self.query_world
                .get_or_insert_with(PhysicsWorld::new)
                .sync_presentation(source, scene, worlds)?;
        }
        let camera = entity.camera.as_ref().unwrap();
        let mut desired;
        let mut anchor;
        let mut options = QueryOptions {
            camera: true,
            ..Default::default()
        };
        let settings = rig.cloned().unwrap_or_default();
        if mode == CameraMode::Fixed {
            let (position, rotation, _) =
                crate::physics3d::world_pose(view.world_matrix(&entity.id)?)?;
            desired = GameCameraPose {
                position,
                rotation,
                fov: camera.fov,
                hidden: Vec::new(),
            };
            anchor = position;
        } else {
            let target = rig
                .and_then(|r| r.target.as_ref())
                .ok_or("Escolha o personagem acompanhado pela câmera.")?;
            let (feet, _, scale) = crate::physics3d::world_pose(
                view.world_matrix(target)
                    .map_err(|_| "Alvo da câmera ausente; última vista preservada.")?,
            )?;
            options.exclude.insert(target.clone());
            options.exclude.extend(scene.descendants(target));
            let character = characters.states.get(target);
            let state = self
                .states
                .get_mut(&entity.id)
                .ok_or("Câmera ainda não preparada.")?;
            let (yaw, pitch) =
                if state.look_at.is_none() && character.is_none_or(|s| s.look_blocks.is_empty()) {
                    settings.look(state.yaw, state.pitch, pending)
                } else {
                    (state.yaw, state.pitch)
                };
            let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
            let eye = character
                .filter(|s| s.eye_height > 0.)
                .map_or(settings.eye_height * scale.y, |s| {
                    s.previous_eye_height + (s.eye_height - s.previous_eye_height) * alpha
                });
            let height = character.map_or_else(
                || {
                    view.entity(target)
                        .and_then(|e| e.character3d.as_ref())
                        .and_then(|c| c.body.prepare(false, c.crouch_height, scale.y, 0.).ok())
                        .map_or(1.8 * scale.y, |b| b.height)
                },
                |s| s.height,
            );
            // Sweep from inside the body to the smoothed eye: posture animation
            // cannot leave the camera inside yesterday's ceiling.
            anchor = feet + Vec3::Y * (height * 0.5);
            let mut position = feet + Vec3::Y * eye;
            if mode == CameraMode::ThirdPerson {
                let crouch_ratio = if settings.eye_height > 0. {
                    eye / (settings.eye_height * scale.y)
                } else {
                    1.
                };
                let pivot =
                    feet + Vec3::Y * settings.follow_height * scale.y * crouch_ratio.min(1.);
                let smoothed = state.pivot.map_or(pivot, |old| {
                    old.lerp(pivot, blend(dt, settings.position_smoothing))
                });
                state.pivot = Some(smoothed);
                position = smoothed
                    + rotation * Vec3::Z * state.distance
                    + Quat::from_rotation_y(yaw)
                        * Vec3::X
                        * settings.shoulder
                        * state.shoulder_sign;
                anchor = pivot;
            }
            let mut hidden = Vec::new();
            if mode == CameraMode::FirstPerson || !settings.hide_first_person_only {
                let mut ids: HashSet<_> = settings.hidden.iter().cloned().collect();
                for id in &settings.hidden {
                    ids.extend(scene.descendants(id));
                }
                hidden.extend(
                    scene
                        .entities
                        .iter()
                        .filter(|e| ids.contains(&e.id))
                        .map(|e| e.id.clone()),
                );
            }
            desired = GameCameraPose {
                position,
                rotation,
                fov: camera.fov,
                hidden,
            };
        }
        if let Some(state) = self.states.get_mut(&entity.id) {
            if let Some(focus) = &state.look_at {
                let point = focus_point(focus, &view)?;
                let direction = point - desired.position;
                if direction.length_squared() > 1e-8 {
                    let (yaw, pitch) = (
                        (-direction.x).atan2(-direction.z),
                        (direction.y / direction.length()).clamp(-1., 1.).asin(),
                    );
                    desired.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
                }
            }
            desired.rotation = state.rotation.map_or(desired.rotation, |r| {
                r.slerp(desired.rotation, blend(dt, settings.rotation_smoothing))
            });
            state.rotation = Some(desired.rotation);
        }
        if let Some(t) = &mut self.transition {
            t.elapsed = (t.elapsed + dt).min(t.duration);
            let amount = t.elapsed / t.duration;
            desired.position = t.from.position.lerp(desired.position, amount);
            desired.rotation = t.from.rotation.slerp(desired.rotation, amount);
            desired.fov = t.from.fov + (desired.fov - t.from.fov) * amount;
            if t.elapsed >= t.duration {
                self.transition = None;
            }
        }
        desired.fov = desired.fov.clamp(10., 150.);
        // Sphere encloses near-plane corners, including ultrawide/FOV changes.
        // Rendering imports this same near distance.
        let aspect = if self.aspect > 0. {
            self.aspect
        } else {
            16. / 9.
        };
        let radius = settings.collision_radius.max(
            CAMERA_NEAR
                * (1. + (desired.fov.to_radians() * 0.5).tan().powi(2) * (1. + aspect * aspect))
                    .sqrt(),
        ) + settings.collision_margin;
        let proposed = desired.position;
        let mut contact = None;
        if let Some(world) = &self.query_world {
            anchor = world.recover_sphere(anchor, radius, radius * 2., &options)?;
            (desired.position, contact) =
                sweep_camera_hit(world, anchor, desired.position, radius, &options)?;
            if mode == CameraMode::ThirdPerson
                && self.transition.is_none()
                && let Some(state) = self.states.get_mut(&entity.id)
            {
                let available = desired.position.distance(anchor);
                let distance = state.shown_distance.map_or(available, |old| {
                    if available < old {
                        available
                    } else {
                        old + (available - old) * blend(dt, settings.obstruction_return)
                    }
                });
                state.shown_distance = Some(distance);
                desired.position =
                    anchor + (desired.position - anchor).normalize_or_zero() * distance;
            }
            // Lateral movement and shoulder changes sweep too. A wall moving
            // into the old pose forces recovery from today's safe anchor.
            if let Some(old) = &self.pose
                && !changed
                && world
                    .penetrating(&CollisionShape::Sphere { radius }, old.position, &options)?
                    .is_empty()
            {
                desired.position =
                    sweep_camera(world, old.position, desired.position, radius, &options)?;
                desired.position = sweep_camera(world, anchor, desired.position, radius, &options)?;
            }
            if !world
                .penetrating(
                    &CollisionShape::Sphere { radius },
                    desired.position,
                    &options,
                )?
                .is_empty()
            {
                return Err("Não há espaço livre para a câmera; última vista preservada.".into());
            }
        }
        if !desired.position.is_finite() || !desired.rotation.is_finite() {
            return Err("Pose de câmera inválida; última vista preservada.".into());
        }
        // Obstruction can move the camera after orbit evaluation. Assisted aim
        // uses its final safe origin, rather than aiming from behind the wall.
        if let Some(focus) = self.states.get(&entity.id).and_then(|s| s.look_at.as_ref())
            && let Some(rotation) = look_rotation(desired.position, focus_point(focus, &view)?)
        {
            desired.rotation = rotation;
        }
        self.debug = Some(CameraDebug {
            pose: desired.clone(),
            desired: proposed,
            anchor,
            radius,
            contact,
        });
        self.pose = Some(desired);
        Ok(())
    }
}
fn sweep_camera(
    world: &PhysicsWorld,
    from: Vec3,
    to: Vec3,
    radius: f32,
    options: &QueryOptions,
) -> Result<Vec3, String> {
    sweep_camera_hit(world, from, to, radius, options).map(|(position, _)| position)
}
fn sweep_camera_hit(
    world: &PhysicsWorld,
    from: Vec3,
    to: Vec3,
    radius: f32,
    options: &QueryOptions,
) -> Result<(Vec3, Option<crate::physics3d::QueryHit>), String> {
    let delta = to - from;
    if delta.length_squared() < 1e-12 {
        return Ok((from, None));
    }
    let hit = world.cast(&CollisionShape::Sphere { radius }, from, delta, options)?;
    let fraction = hit
        .as_ref()
        .map_or(1., |hit| (hit.fraction - 0.001 / delta.length()).max(0.));
    Ok((from + delta * fraction, hit))
}

#[derive(Clone, Debug)]
pub struct CameraDebug {
    pub pose: GameCameraPose,
    pub desired: Vec3,
    pub anchor: Vec3,
    pub radius: f32,
    pub contact: Option<crate::physics3d::QueryHit>,
}
/// Authored scene queries only: no Runtime, input timeline, tasks or simulation.
/// A bounded world cache reuses the same prepared physics shapes between edits.
#[derive(Default)]
pub struct CameraPreview {
    world: Option<PhysicsWorld>,
}
impl CameraPreview {
    pub fn evaluate(
        &mut self,
        scene: &Scene,
        id: &str,
        aspect: f32,
        crouched: bool,
    ) -> Result<CameraDebug, String> {
        let entity = scene
            .entity(id)
            .filter(|e| e.camera.is_some())
            .ok_or("Escolha uma câmera")?;
        let world = self.world.get_or_insert_with(PhysicsWorld::new);
        world.sync_scene(scene, false)?;
        let mut characters = characters::Characters::default();
        if let Some(rig) = &entity.camera_rig
            && let Some(target) = &rig.target
        {
            let (feet, rotation, scale) =
                crate::physics3d::world_pose(scene.world_matrix(target)?)?;
            let mut state = CharacterState::new(feet, angles(rotation).0);
            state.pitch = angles(rotation).1;
            state.height = scene
                .entity(target)
                .and_then(|e| e.character3d.as_ref())
                .map(|c| c.body.prepare(crouched, c.crouch_height, scale.y, 0.))
                .transpose()?
                .map_or(1.8 * scale.y, |b| b.height);
            state.eye_height = if crouched {
                rig.crouched_eye_height
            } else {
                rig.eye_height
            } * scale.y;
            state.previous_eye_height = state.eye_height;
            characters.states.insert(target.clone(), state);
        }
        let mut cameras = Cameras {
            active_override: Some(id.into()),
            aspect,
            query_world: self.world.take(),
            ..Default::default()
        };
        let result = cameras
            .control(scene, &characters, &InputFrame::default())
            .and_then(|_| cameras.present(scene, &characters, 1., Vec2::ZERO, 0.));
        self.world = cameras.query_world.take();
        result?;
        cameras.debug.ok_or("Prévia de câmera indisponível".into())
    }
}
impl Runtime {
    pub fn camera_debug(&self) -> Option<&CameraDebug> {
        self.cameras.debug.as_ref()
    }
    pub(in crate::runtime) fn begin_camera_input(&mut self, input: &InputFrame) {
        if self.scene().kind != SceneKind::ThreeD
            || !self.scene().entities.iter().any(|e| e.camera_rig.is_some())
        {
            return;
        }
        let mut cameras = std::mem::take(&mut self.cameras);
        if let Err(error) = cameras.control(self.scene(), &self.characters, input) {
            let id = cameras
                .active(self.scene())
                .map_or_else(|| self.scene_id.clone(), |e| e.id.clone());
            cameras.report(self, id, error);
        }
        self.cameras = cameras;
    }
    /// Unsmoothened simulation look. Independent of presentation interpolation,
    /// obstruction distance, frame rate and editor navigation.
    pub fn camera_orientation(&self) -> Result<Quat, String> {
        let entity = self
            .cameras
            .active(self.scene())
            .ok_or("Câmera ativa ausente")?;
        if self.cameras.mode(entity) == CameraMode::Fixed {
            return crate::physics3d::world_pose(self.scene().world_matrix(&entity.id)?)
                .map(|(_, r, _)| r);
        }
        let state = self
            .cameras
            .states
            .get(&entity.id)
            .ok_or("Controle de câmera ainda não inicializado")?;
        Ok(Quat::from_rotation_y(state.yaw) * Quat::from_rotation_x(state.pitch))
    }
    pub(in crate::runtime) fn reset_camera_after_teleport(
        &mut self,
        target: &str,
        look: Option<[f32; 2]>,
    ) {
        let ids: Vec<_> = self
            .scene()
            .entities
            .iter()
            .filter(|e| {
                e.camera_rig
                    .as_ref()
                    .is_some_and(|r| r.target.as_deref() == Some(target))
            })
            .map(|e| e.id.clone())
            .collect();
        let active = self.active_camera().map(str::to_owned);
        for id in &ids {
            if let Some(state) = self.cameras.states.get_mut(id) {
                state.pivot = None;
                state.rotation = None;
                state.shown_distance = None;
                if let Some([yaw, pitch]) = look {
                    state.yaw = yaw.rem_euclid(std::f32::consts::TAU);
                    state.pitch = pitch;
                    state.look_at = None;
                }
            }
        }
        if active.as_ref().is_some_and(|id| ids.contains(id)) {
            self.cameras.pose = None;
            self.cameras.key = None;
            self.cameras.transition = None;
            self.cameras.next_transition = None;
            self.pending_look = [0.; 2];
            self.pending_wheel = 0.;
            if let Some(rig) = active
                .as_ref()
                .and_then(|id| self.entity(id))
                .and_then(|e| e.camera_rig.clone())
                && let Some(state) = self.characters.states.get_mut(target)
            {
                state.eye_height = if state.posture == Posture::Standing {
                    rig.eye_height
                } else {
                    rig.crouched_eye_height
                } * state.uniform_scale;
                state.previous_eye_height = state.eye_height;
            }
        }
    }
    pub fn set_viewport_aspect(&mut self, aspect: f32) {
        if aspect.is_finite() && aspect > 0. && self.cameras.aspect != aspect {
            self.cameras.aspect = aspect;
            self.update_presentation(0.);
        }
    }
    pub(super) fn update_presentation(&mut self, dt: f32) {
        crate::metrics::timed(
            || self.update_presentation_inner(dt),
            |c, ns| c.presentation_ns += ns,
        );
    }
    pub fn camera_physics_counters(&self) -> crate::physics3d::PhysicsCounters {
        self.cameras
            .query_world
            .as_ref()
            .map_or_else(Default::default, PhysicsWorld::counters)
    }
    fn update_presentation_inner(&mut self, dt: f32) {
        if self.stopped || self.scene().kind != SceneKind::ThreeD {
            return;
        }
        // Legacy scenes do not pay for a second query world or pose maps.
        if !self
            .scene()
            .entities
            .iter()
            .any(|e| e.character3d.is_some() || e.camera_rig.is_some() || e.platform.is_some())
        {
            return;
        }
        let mut cameras = std::mem::take(&mut self.cameras);
        let result = (|| {
            self.refresh_character_queries()?;
            if cameras
                .active(self.scene())
                .is_some_and(|e| e.camera_rig.is_some() && !cameras.states.contains_key(&e.id))
            {
                cameras.control(self.scene(), &self.characters, &InputFrame::default())?;
            }
            if self.characters.world.is_none() {
                let mut world = PhysicsWorld::new();
                world.sync_scene(self.scene(), false)?;
                self.characters.world = Some(world);
            }
            let pending = if self.paused {
                Vec2::ZERO
            } else {
                Vec2::from(self.pending_look)
                    + self
                        .input_timeline
                        .unconsumed_look(self.time + f64::from(self.accumulator))
            };
            cameras.present(
                self.scene(),
                &self.characters,
                (self.accumulator / FIXED_DT).clamp(0., 1.),
                pending,
                dt.min(0.25),
            )
        })();
        let warning = result.err().or_else(|| {
            (cameras.active_override.is_none()
                && self
                    .scene()
                    .entities
                    .iter()
                    .filter(|e| e.camera.as_ref().is_some_and(|c| c.active))
                    .count()
                    > 1)
            .then(|| "Mais de uma câmera ativa: usando a primeira na ordem da cena.".into())
        });
        if let Some(warning) = warning {
            let id = cameras
                .active(self.scene())
                .map_or_else(|| self.scene_id.clone(), |e| e.id.clone());
            cameras.report(self, id, warning);
        }
        self.cameras = cameras;
    }
    pub fn active_camera(&self) -> Option<&str> {
        self.cameras.active(self.scene()).map(|e| e.id.as_str())
    }
    pub fn camera_mode(&self, id: &str) -> Option<CameraMode> {
        self.entity(id)
            .filter(|e| e.camera.is_some())
            .map(|e| self.cameras.mode(e))
    }
    pub fn activate_camera(&mut self, id: &str, seconds: f32) -> Result<(), String> {
        if self.entity(id).is_none_or(|e| e.camera.is_none())
            || !seconds.is_finite()
            || seconds < 0.
        {
            return Err("Escolha uma câmera existente e uma duração válida.".into());
        }
        self.cameras.active_override = Some(id.into());
        self.cameras.next_transition = Some(seconds);
        self.update_presentation(0.);
        Ok(())
    }
    pub fn set_camera_mode(
        &mut self,
        id: &str,
        mode: CameraMode,
        seconds: f32,
    ) -> Result<(), String> {
        if self.entity(id).is_none_or(|e| e.camera_rig.is_none())
            || !seconds.is_finite()
            || seconds < 0.
        {
            return Err("Escolha uma câmera de personagem e uma duração válida.".into());
        }
        let rig = self.entity_mut(id).unwrap().camera_rig.as_mut().unwrap();
        rig.mode = mode;
        if let Some(state) = self.cameras.states.get_mut(id) {
            state.mode_override = None;
        }
        self.cameras.next_transition = Some(seconds);
        self.update_presentation(0.);
        Ok(())
    }
    pub fn configure_camera(&mut self, id: &str, settings: CameraRig) -> Result<(), String> {
        settings.validate_settings()?;
        if self.entity(id).is_none_or(|e| e.camera.is_none()) {
            return Err("Objeto não possui câmera.".into());
        }
        if settings
            .target
            .as_ref()
            .is_some_and(|target| target == id || self.entity(target).is_none())
            || settings.hidden.iter().any(|id| self.entity(id).is_none())
        {
            return Err("Referência da câmera inválida.".into());
        }
        self.entity_mut(id).unwrap().camera_rig = Some(settings);
        self.update_presentation(0.);
        Ok(())
    }
    pub fn set_camera_target(&mut self, id: &str, target: &str) -> Result<(), String> {
        let mut rig = self
            .entity(id)
            .and_then(|e| e.camera_rig.clone())
            .ok_or("Objeto não possui câmera de personagem.")?;
        rig.target = Some(target.into());
        self.configure_camera(id, rig)
    }
    pub fn set_camera_look_at(
        &mut self,
        id: &str,
        focus: Option<CameraLookAt>,
    ) -> Result<(), String> {
        if self.entity(id).is_none_or(|e| e.camera_rig.is_none()) {
            return Err("Objeto não possui câmera de personagem.".into());
        }
        if let Some(focus) = &focus {
            let point = focus_point(focus, &SceneView::new(self.scene()))?;
            if !point.is_finite() {
                return Err("Alvo do olhar inválido.".into());
            }
        }
        self.update_presentation(0.);
        let state = self
            .cameras
            .states
            .get_mut(id)
            .ok_or("Ative a câmera antes de comandar o olhar.")?;
        if focus.is_none()
            && let Some(pose) = &self.cameras.pose
        {
            (state.yaw, state.pitch) = angles(pose.rotation);
        }
        state.look_at = focus;
        self.update_presentation(0.);
        Ok(())
    }
    pub fn set_camera_orientation(&mut self, id: &str, yaw: f32, pitch: f32) -> Result<(), String> {
        if !yaw.is_finite() || !pitch.is_finite() {
            return Err("Orientação da câmera inválida.".into());
        }
        let rig = self
            .entity(id)
            .and_then(|e| e.camera_rig.as_ref())
            .ok_or("Objeto não possui câmera de personagem.")?;
        let limit = rig.pitch_limit.to_radians();
        self.update_presentation(0.);
        let state = self
            .cameras
            .states
            .get_mut(id)
            .ok_or("Ative a câmera antes de comandar o olhar.")?;
        state.yaw = yaw.rem_euclid(std::f32::consts::TAU);
        state.pitch = pitch.clamp(-limit, limit);
        state.rotation = None;
        self.update_presentation(0.);
        Ok(())
    }
    pub fn set_camera_fov(&mut self, id: &str, degrees: f32) -> Result<(), String> {
        if !degrees.is_finite() || !(10. ..=150.).contains(&degrees) {
            return Err("Campo de visão vertical deve estar entre 10° e 150°.".into());
        }
        self.entity_mut(id)
            .and_then(|e| e.camera.as_mut())
            .ok_or("Objeto não possui câmera.")?
            .fov = degrees;
        self.update_presentation(0.);
        Ok(())
    }
    pub fn set_camera_setting(
        &mut self,
        id: &str,
        setting: &str,
        value: f32,
        seconds: f32,
    ) -> Result<(), String> {
        if !value.is_finite() || !seconds.is_finite() || seconds < 0. {
            return Err("Valor/transição da câmera inválidos.".into());
        }
        let before = self.cameras.pose.clone();
        match setting {
            "fov" => self.set_camera_fov(id, value)?,
            "distance" | "shoulder" => {
                let mut rig = self
                    .entity(id)
                    .and_then(|e| e.camera_rig.clone())
                    .ok_or("Objeto não possui câmera de personagem")?;
                if setting == "distance" {
                    rig.distance = value;
                } else {
                    rig.shoulder = value;
                }
                self.configure_camera(id, rig)?;
            }
            _ => return Err("Ajuste de câmera não permitido.".into()),
        }
        if self.active_camera() == Some(id) {
            self.cameras.transition = before.filter(|_| seconds > 0.).map(|from| Transition {
                from,
                elapsed: 0.,
                duration: seconds,
            });
            self.update_presentation(0.);
        }
        Ok(())
    }
}
