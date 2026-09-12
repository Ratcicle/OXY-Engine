use super::*;
use crate::{
    physics3d::{QueryHit, ShapeMotion},
    surface::{SurfaceMaterial, VelocitySpace},
};

pub(super) struct MotorInput {
    pub axis: Vec2,
    pub jump: bool,
    pub sprint: bool,
    pub crouch: bool,
    pub crouch_pressed: bool,
    pub crouch_override: Option<bool>,
    pub jump_held: bool,
    pub slide: bool,
}
pub(super) struct MotorContext<'a> {
    pub world: &'a PhysicsWorld,
    pub config: &'a CharacterConfig,
    pub standing: ShapeMotion,
    pub options: QueryOptions,
    pub surfaces: &'a [SurfaceMaterial],
    pub platforms: &'a HashMap<Id, platforms::PlatformFrame>,
    pub scene: &'a Scene,
    pub evaluation: &'a SceneEvaluation,
    pub time: f64,
    pub dt: f32,
    pub scale: f32,
}
#[derive(Default)]
pub(super) struct MotorStep {
    pub events: Vec<(MovementEvent, Arc<CharacterState>)>,
    pub path: Vec<(Vec3, Vec3)>,
    pub lateral: Vec<QueryHit>,
    pub warning: Option<String>,
}
fn append_path(
    path: &mut Vec<(Vec3, Vec3)>,
    origin: Vec3,
    result: &crate::physics3d::MotionResult,
) {
    path.extend(
        result
            .segments
            .iter()
            .map(|(a, b)| (origin + *a, origin + *b)),
    );
}
pub(super) fn inherited(velocity: Vec3, policy: InheritPlatform) -> Vec3 {
    match policy {
        InheritPlatform::None => Vec3::ZERO,
        InheritPlatform::Horizontal => Vec3::new(velocity.x, 0., velocity.z),
        InheritPlatform::All => velocity,
    }
}
fn record(
    events: &mut Vec<(MovementEvent, Arc<CharacterState>)>,
    event: MovementEvent,
    state: &CharacterState,
) {
    events.push((event, Arc::new(state.clone())));
}
fn jump(
    state: &mut CharacterState,
    config: &CharacterConfig,
    events: &mut Vec<(MovementEvent, Arc<CharacterState>)>,
) {
    state.velocity.x *= config.jump_retention;
    state.velocity.z *= config.jump_retention;
    state.velocity.y = config.jump_speed;
    if let Some(support) = state.support.take() {
        state.velocity += inherited(support.velocity, config.inherit_platform);
    }
    state.grounded = false;
    state.jump_consumed = true;
    state.coyote_until = f64::NEG_INFINITY;
    state.jump_until = None;
    if state.posture == Posture::Sliding {
        state.posture = Posture::Crouched;
        record(
            events,
            MovementEvent::PostureChanged(Posture::Crouched),
            state,
        );
    }
    record(events, MovementEvent::Jumped, state);
}
/// Accelerate along the requested projection, preserving perpendicular momentum.
pub(crate) fn air_accelerate(
    velocity: Vec3,
    axis: Vec3,
    acceleration: f32,
    limit: f32,
    dt: f32,
) -> Vec3 {
    let intensity = axis.length().min(1.);
    if intensity <= 1e-6 {
        return velocity;
    }
    let direction = axis.normalize_or_zero();
    let gain = (limit - velocity.dot(direction))
        .max(0.)
        .min(acceleration * intensity * dt);
    velocity + direction * gain
}
fn approach(value: Vec3, target: Vec3, amount: f32) -> Vec3 {
    let difference = target - value;
    value + difference.clamp_length_max(amount.max(0.))
}

impl MotorContext<'_> {
    fn probe(
        &self,
        feet: Vec3,
        motion: &ShapeMotion,
        previous: Option<&str>,
    ) -> Result<Option<QueryHit>, String> {
        let lift = motion.margin + 0.025;
        let distance = lift + motion.snap + motion.margin + 0.03;
        let at = motion.at(feet + Vec3::Y * lift);
        let mut options = self.options.clone();
        let mut best = None;
        // Reject walls in the support probe; collision resolution still sees them.
        for _ in 0..8 {
            let Some(hit) = self.world.cast_prepared(
                &motion.geometry,
                motion.rotation,
                at,
                -Vec3::Y * distance,
                &options,
            )?
            else {
                break;
            };
            if hit.normal.y >= motion.climb_degrees.to_radians().cos() {
                best = Some(hit);
                break;
            }
            options.exclude.insert(hit.object);
        }
        if let (Some(id), Some(hit)) = (previous, best.as_ref())
            && id != hit.object
        {
            let mut same = self.options.clone();
            same.only = Some(id.into());
            if let Some(old) = self.world.cast_prepared(
                &motion.geometry,
                motion.rotation,
                at,
                -Vec3::Y * distance,
                &same,
            )? && old.distance <= hit.distance + 0.02
                && old.normal.y >= motion.climb_degrees.to_radians().cos()
            {
                best = Some(old);
            }
        }
        // A short ray at the contact stabilizes exact surface normals near the
        // capsule margin; it must hit the same collider, not a neighbouring wall.
        if let Some(hit) = &mut best {
            let mut same = self.options.clone();
            same.only = Some(hit.object.clone());
            if let Some(face) =
                self.world
                    .ray(hit.point + hit.normal * 0.025, -hit.normal, 0.05, &same)?
                && face.normal.y >= motion.climb_degrees.to_radians().cos()
            {
                hit.normal = face.normal;
                hit.point = face.point;
            }
        }
        // A support query may search as far as snap, but distant ground alone
        // must not grant a jump or suspend gravity. The solver handles snapping.
        Ok(best.filter(|hit| hit.distance <= lift + motion.margin + 0.02))
    }
    fn support(&self, hit: QueryHit) -> Option<Support> {
        let platform = self.platforms.get(&hit.object);
        if platform.is_some_and(|p| p.discontinuous) {
            return None;
        }
        let mut velocity = platform.map_or(Vec3::ZERO, |p| p.velocity);
        if let Some(material) = hit
            .surface
            .as_ref()
            .and_then(|id| self.surfaces.iter().find(|s| s.id == *id))
        {
            let mut belt = Vec3::from(material.conveyor);
            if material.conveyor_space == VelocitySpace::Local
                && let Some(i) = self.evaluation.index.position(&hit.object)
                && let Some(matrix) = self.evaluation.worlds[i]
            {
                belt = crate::physics3d::world_pose(matrix).ok()?.1 * belt;
            }
            velocity += belt - hit.normal * belt.dot(hit.normal);
        }
        Some(Support {
            object: hit.object,
            point: hit.point,
            normal: hit.normal,
            surface: hit.surface,
            velocity,
        })
    }
    pub fn advance(
        &self,
        state: &mut CharacterState,
        input: MotorInput,
    ) -> Result<MotorStep, String> {
        let mut out = MotorStep::default();
        let config = self.config;
        let dt = self.dt;
        let was_grounded = state.grounded;
        let old_surface = state.support.as_ref().and_then(|s| s.surface.clone());
        let previous_support = state.support.as_ref().map(|s| s.object.clone());
        if state.height <= 0. {
            state.height = self.standing.height;
        }
        let mut crouched =
            config
                .body
                .prepare(true, config.crouch_height, self.scale, state.yaw)?;
        crouched.settings = self.standing.settings;
        let crouch_height = crouched.height;
        if let Some(wanted) = input.crouch_override {
            state.want_crouch = wanted;
        } else if config.crouch_toggle {
            if input.crouch_pressed {
                state.want_crouch = !state.want_crouch;
            }
        } else {
            state.want_crouch = input.crouch;
        }
        if input.slide {
            state.want_crouch = true;
            state.slide_latched = false;
        }
        let old_posture = state.posture;
        let mut standing = self.standing.clone();
        standing.set_yaw(state.yaw);
        if !state.want_crouch {
            state.slide_latched = false;
        }
        let horizontal_speed = Vec2::new(state.velocity.x, state.velocity.z).length();
        if state.posture == Posture::Sliding {
            state.slide_elapsed += dt;
            if !config.slide_enabled
                || !state.grounded
                || !state.want_crouch
                || state.slide_elapsed >= config.slide_duration
                || horizontal_speed < config.slide_exit_speed
            {
                state.posture = Posture::Crouched;
            }
        } else if config.slide_enabled
            && state.want_crouch
            && !state.slide_latched
            && state.grounded
            && horizontal_speed >= config.slide_min_speed
        {
            state.posture = Posture::Sliding;
            state.slide_elapsed = 0.;
            state.slide_latched = true;
        }
        if state.posture == Posture::Sliding {
            state.height = crouch_height;
        } else if state.want_crouch {
            state.posture = Posture::Crouched;
            state.height = crouch_height;
        } else if state.posture == Posture::Crouched
            && self
                .world
                .penetrating_prepared(
                    &standing.geometry,
                    standing.rotation,
                    standing.at(state.position),
                    &self.options,
                )?
                .is_empty()
        {
            state.posture = Posture::Standing;
            state.height = self.standing.height;
        }
        let mut motion = if state.posture == Posture::Standing {
            standing
        } else {
            crouched
        };
        if old_posture != state.posture {
            record(
                &mut out.events,
                MovementEvent::PostureChanged(state.posture),
                state,
            );
        }
        state.sprinting = input.sprint && state.posture == Posture::Standing;

        // Carry exactly once, before depenetration. Ignore only the supporting
        // platform during this sweep; walls/ceilings still stop the passenger.
        if let Some(support) = state.support.clone() {
            let exists = self
                .evaluation
                .index
                .position(&support.object)
                .is_some_and(|i| {
                    self.scene.entities[i]
                        .physics3d
                        .as_ref()
                        .is_some_and(|c| c.enabled && !c.sensor)
                        || self.scene.entities[i]
                            .collider
                            .as_ref()
                            .is_some_and(|c| c.enabled && !c.is_trigger)
                        || self.scene.entities[i]
                            .character3d
                            .as_ref()
                            .is_some_and(|c| c.body.enabled)
                });
            if !exists
                || self
                    .platforms
                    .get(&support.object)
                    .is_some_and(|p| p.discontinuous)
            {
                state.support = None;
                state.grounded = false;
            } else {
                let base = self
                    .platforms
                    .get(&support.object)
                    .map_or(Vec3::ZERO, |p| p.delta);
                let platform_velocity = self
                    .platforms
                    .get(&support.object)
                    .map_or(Vec3::ZERO, |p| p.velocity);
                let fresh = self
                    .support(QueryHit {
                        object: support.object.clone(),
                        point: support.point,
                        normal: support.normal,
                        surface: support.surface.clone(),
                        ..Default::default()
                    })
                    .unwrap();
                let carry = base + (fresh.velocity - platform_velocity) * dt;
                if carry.length_squared() > 1e-12 {
                    let mut options = self.options.clone();
                    options.exclude.insert(support.object.clone());
                    let mut carry_motion = motion.clone();
                    carry_motion.snap = 0.;
                    carry_motion.step_height = 0.;
                    let resolved =
                        self.world
                            .move_body(state.position, carry, dt, &carry_motion, &options)?;
                    append_path(&mut out.path, state.position, &resolved);
                    state.position += resolved.delta;
                    state.velocity += (resolved.delta - carry) / dt;
                }
                state.support = Some(fresh);
            }
        }
        // Recover only genuine penetration (touching the floor is allowed).
        let penetrating = self.world.penetrating_prepared(
            &motion.geometry,
            motion.rotation,
            motion.at(state.position),
            &self.options,
        )?;
        if !penetrating.is_empty() {
            let recovered =
                self.world
                    .move_body(state.position, Vec3::ZERO, dt, &motion, &self.options)?;
            let recovery = recovered
                .delta
                .clamp_length_max(config.recovery_distance * self.scale);
            let mut path_options = self.options.clone();
            path_options.exclude.extend(penetrating);
            let obstacle = self.world.cast_prepared(
                &motion.geometry,
                motion.rotation,
                motion.at(state.position),
                recovery,
                &path_options,
            )?;
            let correction =
                obstacle.map_or(recovery, |hit| recovery * (hit.fraction - 0.001).max(0.));
            if correction.length_squared() > 1e-12 {
                out.path.push((state.position, state.position + correction));
                state.position += correction;
            }
            if !self
                .world
                .penetrating_prepared(
                    &motion.geometry,
                    motion.rotation,
                    motion.at(state.position),
                    &self.options,
                )?
                .is_empty()
            {
                state.velocity = Vec3::ZERO;
                state.support = None;
                state.grounded = false;
                out.warning=Some("Personagem sem espaço após recuperação limitada; afaste o obstáculo ou use um ponto livre. Nenhum dano automático foi aplicado.".into());
                return Ok(out);
            }
        }

        let transport_velocity = state.support.as_ref().map_or(Vec3::ZERO, |s| s.velocity);
        if state.grounded {
            let contact = self.probe(state.position, &motion, previous_support.as_deref())?;
            state.grounded = contact.is_some();
            state.support = contact.and_then(|hit| self.support(hit));
        }
        if was_grounded && !state.grounded && !state.jump_consumed {
            state.coyote_until = self.time + f64::from(config.coyote_ms) * 0.001;
        }
        if input.jump || (config.jump_mode == JumpMode::Automatic && input.jump_held) {
            state.jump_until = Some(self.time + f64::from(config.jump_buffer_ms) * 0.001);
        }
        if state.jump_until.is_some_and(|t| t < self.time) {
            state.jump_until = None;
        }
        if !state.movement_blocks.is_empty() {
            state.jump_until = None;
        }
        let eligible = state.grounded || (!state.jump_consumed && self.time <= state.coyote_until);
        if state.jump_until.is_some() && eligible {
            jump(state, config, &mut out.events);
        }

        let material = state
            .support
            .as_ref()
            .and_then(|s| s.surface.as_ref())
            .and_then(|id| self.surfaces.iter().find(|s| s.id == *id));
        let friction = material.map_or(1., |s| s.friction);
        let traction = material.map_or(1., |s| s.traction);
        let modifier = material.map_or(1., |s| s.speed_multiplier);
        let yaw = match config.reference {
            MovementReference::World => 0.,
            MovementReference::Body => state.yaw,
            MovementReference::Camera => state.look_yaw,
        };
        let axis = wish_direction(input.axis, yaw);
        if state.grounded {
            let normal = state.support.as_ref().map_or(Vec3::Y, |s| s.normal);
            let speed = if state.posture != Posture::Standing {
                config.crouch_speed
            } else if state.sprinting {
                config.sprint_speed
            } else {
                config.speed
            };
            if state.posture == Posture::Sliding {
                let length = state.velocity.length();
                let next = (length - config.slide_friction * friction * length * dt).max(0.);
                state.velocity = state.velocity.normalize_or_zero() * next;
                if input.axis.length_squared() > 1e-8 {
                    let direction = (axis - normal * axis.dot(normal)).normalize_or_zero();
                    state.velocity = approach(
                        state.velocity,
                        direction * next,
                        config.slide_control * traction * input.axis.length() * dt,
                    );
                }
            } else if input.axis.length_squared() > 1e-8 {
                let direction = (axis - normal * axis.dot(normal)).normalize_or_zero();
                state.velocity = approach(
                    state.velocity,
                    direction * input.axis.length() * speed * modifier,
                    config.ground_acceleration * traction * dt,
                );
            } else {
                let length = state.velocity.length();
                let drop =
                    (config.ground_braking + config.ground_friction * length) * friction * dt;
                state.velocity *= if length > 0. {
                    (length - drop).max(0.) / length
                } else {
                    0.
                };
            }
        } else {
            let drag = (-config.air_resistance * dt).exp();
            state.velocity.x *= drag;
            state.velocity.z *= drag;
            state.velocity = air_accelerate(
                state.velocity,
                axis,
                config.air_acceleration,
                config.air_projected_limit,
                dt,
            );
        }
        if config.horizontal_limit > 0. {
            let horizontal = Vec2::new(state.velocity.x, state.velocity.z)
                .clamp_length_max(config.horizontal_limit);
            state.velocity.x = horizontal.x;
            state.velocity.z = horizontal.y;
        }
        state.velocity = state.velocity.clamp_length_max(config.absolute_speed_limit);
        let mut desired_velocity = state.velocity;
        if state.grounded {
            desired_velocity.y -= config.gravity * dt;
        } else {
            state.velocity.y -= config.gravity * dt;
            state.velocity = state.velocity.clamp_length_max(config.absolute_speed_limit);
            desired_velocity = state.velocity;
        }
        if !state.grounded && state.velocity.y > 0. {
            motion.snap = 0.;
            motion.step_height = 0.;
        }
        let before_velocity = state.velocity;
        let ascending = !state.grounded && state.velocity.y > 0.;
        let resolved = self.world.move_body(
            state.position,
            desired_velocity * dt,
            dt,
            &motion,
            &self.options,
        )?;
        append_path(&mut out.path, state.position, &resolved);
        state.position += resolved.delta;
        // Rapier's `is_sliding_down_slope` also describes unconstrained tangents
        // on almost-flat contacts. Walkability comes from our support normal.
        state.support = if !ascending {
            self.probe(state.position, &motion, previous_support.as_deref())?
                .and_then(|hit| self.support(hit))
        } else {
            None
        };
        state.grounded = state.support.is_some();
        for hit in &resolved.contacts {
            if hit.normal.y >= motion.climb_degrees.to_radians().cos() {
                continue;
            }
            if hit.normal.y < -0.5 {
                state.velocity.y = state.velocity.y.min(0.);
            } else {
                let into = state.velocity.dot(hit.normal);
                if into < 0. {
                    state.velocity -= hit.normal * into;
                }
                out.lateral.push(hit.clone());
            }
        }
        if state.grounded {
            if !was_grounded {
                // Air velocity is in world space. On acquiring support, store
                // it relative to that support before projection/retention or
                // a buffered jump. total_velocity adds transport exactly once.
                state.velocity -= state.support.as_ref().map_or(Vec3::ZERO, |s| s.velocity);
            }
            let normal = state.support.as_ref().map_or(Vec3::Y, |s| s.normal);
            let speed = Vec3::new(state.velocity.x, 0., state.velocity.z).length();
            let horizontal = Vec3::new(state.velocity.x, 0., state.velocity.z);
            state.velocity = (horizontal - normal * horizontal.dot(normal)).normalize_or_zero()
                * if was_grounded {
                    state.velocity.length()
                } else {
                    speed
                };
            state.jump_consumed = false;
            if !was_grounded {
                let mut snapshot = state.clone();
                snapshot.velocity =
                    before_velocity - snapshot.support.as_ref().map_or(Vec3::ZERO, |s| s.velocity);
                record(
                    &mut out.events,
                    MovementEvent::Landed {
                        impact_speed: (-before_velocity.dot(normal)).max(0.),
                    },
                    &snapshot,
                );
                // A buffered/automatic re-jump avoids landing loss and extra
                // ground friction; ordinary landings apply their own retention.
                if state.jump_until.is_none() {
                    state.velocity *= config.landing_retention;
                }
            }
            // A buffered landing jump prepares velocity only; it does not run a
            // second dt or apply an extra ground-friction update this same tick.
            if state.jump_until.is_some() && state.movement_blocks.is_empty() {
                jump(state, config, &mut out.events);
            }
        } else if was_grounded && !state.jump_consumed {
            state.coyote_until = self.time + f64::from(config.coyote_ms) * 0.001;
            state.velocity += inherited(transport_velocity, config.inherit_platform);
        }
        if was_grounded && !state.grounded {
            record(&mut out.events, MovementEvent::LeftSupport, state);
        }
        let surface = state.support.as_ref().and_then(|s| s.surface.clone());
        if old_surface != surface {
            record(
                &mut out.events,
                MovementEvent::SurfaceChanged {
                    previous: old_surface,
                    current: surface,
                },
                state,
            );
        }
        if !state.position.is_finite() || !state.velocity.is_finite() {
            return Err("Estado de movimento não finito; verifique forças e dimensões.".into());
        }
        Ok(out)
    }
}
