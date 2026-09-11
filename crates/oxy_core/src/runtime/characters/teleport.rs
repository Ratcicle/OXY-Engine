//! Atomic, bounded relocation shared by graphs and native callers.
use super::*;
impl Runtime {
    pub(in crate::runtime) fn refresh_character_queries(&mut self) -> Result<(), String> {
        if self.scene().kind != SceneKind::ThreeD {
            return Err("Esta consulta exige cena 3D.".into());
        }
        if !self.physics_dirty && self.characters.world.is_some() {
            return Ok(());
        }
        let evaluation = SceneEvaluation::new(self.scene())?;
        let mut overrides = HashMap::new();
        for (id, state) in &self.characters.states {
            if let Some(e) = self.entity(id) {
                let matrix = evaluation
                    .index
                    .position(id)
                    .and_then(|i| evaluation.worlds[i])
                    .ok_or("Transformação do personagem ausente")?;
                // Validate structural edits before reusing the authoritative
                // pose. Never hide a parent that now deforms the physical root.
                let (_, rotation, scale) = crate::physics3d::world_pose(matrix)?;
                character_scale(rotation, scale)?;
                overrides.insert(
                    id.clone(),
                    ColliderOverride {
                        config: actual_collider(e, state, state.uniform_scale),
                        position: state.position,
                        rotation: Quat::from_rotation_y(state.yaw),
                        scale: Vec3::splat(state.uniform_scale),
                    },
                );
            }
        }
        let mut world = self.characters.world.take().unwrap_or_default();
        let result = world.sync_evaluated(self.scene(), &evaluation, &overrides);
        self.characters.world = Some(world);
        result?;
        self.physics_dirty = false;
        Ok(())
    }
    fn character_transform(
        &self,
        id: &str,
        position: Vec3,
        yaw: f32,
        scale: f32,
    ) -> Result<crate::document::Transform, String> {
        let e = self.entity(id).ok_or("Personagem ausente")?;
        let parent = e
            .parent
            .as_ref()
            .map(|p| self.scene().world_matrix(p))
            .transpose()?
            .unwrap_or(Mat4::IDENTITY);
        crate::physics3d::world_pose(parent)?;
        let local = parent.inverse()
            * Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                Quat::from_rotation_y(yaw),
                position,
            );
        let transform = crate::document::Transform::from_matrix(local, e.transform.pivot);
        if !transform.finite() || !transform.matrix().abs_diff_eq(local, 1e-4) {
            return Err(
                "Parentesco produziria uma deformação da raiz física; operação recusada.".into(),
            );
        }
        Ok(transform)
    }
    pub fn teleport_character(
        &mut self,
        id: &str,
        destination: Vec3,
        mut options: TeleportOptions,
    ) -> Result<Vec3, String> {
        if !destination.is_finite()
            || options.yaw.is_some_and(|v| !v.is_finite())
            || options
                .look
                .is_some_and(|v| v.iter().any(|v| !v.is_finite()))
            || !options.search_radius.is_finite()
            || !(0. ..=2.).contains(&options.search_radius)
        {
            return Err(
                "Destino/orientação inválidos; busca local deve estar entre 0 e 2 m.".into(),
            );
        }
        self.ensure_character_state(id)?;
        self.refresh_character_queries()?;
        if let Some([yaw, pitch]) = options.look {
            let limit = self
                .scene()
                .entities
                .iter()
                .filter_map(|e| e.camera_rig.as_ref())
                .filter(|rig| rig.target.as_deref() == Some(id))
                .map(|rig| rig.pitch_limit.to_radians())
                .fold(89_f32.to_radians(), f32::min);
            options.look = Some([
                yaw.rem_euclid(std::f32::consts::TAU),
                pitch.clamp(-limit, limit),
            ]);
        }
        let old = &self.characters.states[id];
        let entity = self.entity(id).unwrap();
        let config = entity.character3d.as_ref().unwrap();
        let motion = config.motion(entity, old.uniform_scale)?;
        let shape = crate::physics3d::CollisionShape::Capsule {
            height: old.height,
            radius: motion.radius,
        };
        let mut filter = QueryOptions {
            category: entity.physics3d.as_ref().unwrap().filter.category,
            mask: entity.physics3d.as_ref().unwrap().filter.mask,
            ..QueryOptions::excluding(id)
        };
        filter.exclude.extend(self.scene().descendants(id));
        let world = self.characters.world.as_ref().unwrap();
        let free = |feet: Vec3| {
            world
                .penetrating(&shape, feet + Vec3::Y * (old.height * 0.5), &filter)
                .map(|ids| ids.is_empty())
        };
        let mut selected = free(destination)?.then_some(destination);
        if selected.is_none() && options.search_radius > 0. {
            'search: for step in 1..=4 {
                for axis in [Vec3::Y, Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z] {
                    let candidate = destination + axis * (options.search_radius * step as f32 / 4.);
                    if free(candidate)? {
                        selected = Some(candidate);
                        break 'search;
                    }
                }
            }
        }
        let destination =
            selected.ok_or("Destino ocupado; personagem mantido na posição anterior.")?;
        let yaw = options
            .yaw
            .unwrap_or(old.yaw)
            .rem_euclid(std::f32::consts::TAU);
        let transform = self.character_transform(id, destination, yaw, old.uniform_scale)?;
        let mut next = old.clone();
        next.position = destination;
        next.previous_position = destination;
        next.yaw = yaw;
        next.previous_yaw = yaw;
        next.velocity = if options.keep_velocity {
            old.total_velocity()
        } else {
            Vec3::ZERO
        };
        if !next.velocity.is_finite() {
            return Err(
                "Velocidade acumulada fora do intervalo suportado; teleporte recusado.".into(),
            );
        }
        next.support = None;
        next.grounded = false;
        next.jump_until = None;
        next.coyote_until = f64::NEG_INFINITY;
        next.jump_consumed = false;
        next.want_crouch = next.posture != Posture::Standing;
        next.slide_latched = false;
        next.slide_elapsed = 0.;
        next.sprinting = false;
        if next.posture == Posture::Sliding {
            next.posture = Posture::Crouched;
        }
        next.eye_height = 0.;
        next.previous_eye_height = 0.;
        next.trajectory = next.trajectory.wrapping_add(1);
        if let Some([look_yaw, pitch]) = options.look {
            next.look_yaw = look_yaw;
            next.pitch = pitch;
        }
        // Transaction: a transformed descendant can still invalidate a physical
        // shape. Validate synchronization before consuming tasks/input or camera
        // state, and roll back the small pose/state delta on failure.
        let old_state = old.clone();
        let old_transform = self.entity(id).unwrap().transform.clone();
        self.entity_mut(id).unwrap().transform = transform;
        self.characters.states.insert(id.into(), next);
        self.physics_dirty = true;
        if let Err(error) = self.refresh_character_queries() {
            self.entity_mut(id).unwrap().transform = old_transform;
            self.characters.states.insert(id.into(), old_state);
            self.physics_dirty = true;
            return match self.refresh_character_queries() {
                Ok(()) => Err(format!(
                    "Teleporte recusado, estado anterior restaurado: {error}"
                )),
                Err(restore) => Err(format!(
                    "Teleporte recusado: {error}. Documento restaurado; consulta física precisa de revisão: {restore}"
                )),
            };
        }
        self.step_input = InputFrame::default();
        self.ready.retain(|task| {
            task.context
                .trajectory
                .as_ref()
                .is_none_or(|(target, _)| target != id)
        });
        self.waiting.retain(|task| {
            task.context
                .trajectory
                .as_ref()
                .is_none_or(|(target, _)| target != id)
        });
        self.characters.intents.remove(id);
        self.characters.jumps.remove(id);
        self.characters.sprints.remove(id);
        self.characters.crouches.remove(id);
        self.characters.slides.remove(id);
        self.characters.paths.remove(id);
        self.characters.platforms.remove(id);
        self.characters
            .sensor_pairs
            .retain(|(a, b)| a != id && b != id);
        self.characters.sensor_events.retain(|e| e.object != id);
        self.characters.lateral.retain(|(a, b)| a != id && b != id);
        self.characters.events.retain(|(owner, _)| owner != id);
        self.characters.records.retain(|r| r.object != id);
        self.characters.pending_records.retain(|r| r.object != id);
        self.reset_camera_after_teleport(id, options.look);
        self.update_presentation(0.);
        Ok(destination)
    }
    pub fn set_character_yaw(&mut self, id: &str, yaw: f32) -> Result<(), String> {
        if !yaw.is_finite() {
            return Err("Orientação inválida.".into());
        }
        self.ensure_character_state(id)?;
        let state = &self.characters.states[id];
        let transform = self.character_transform(id, state.position, yaw, state.uniform_scale)?;
        self.entity_mut(id).unwrap().transform = transform;
        let state = self.characters.states.get_mut(id).unwrap();
        state.yaw = yaw;
        state.previous_yaw = yaw;
        self.physics_dirty = true;
        if let Some(camera) = self.active_camera().map(str::to_owned)
            && self.camera_mode(&camera) == Some(crate::character::CameraMode::FirstPerson)
            && self
                .entity(&camera)
                .and_then(|e| e.camera_rig.as_ref())
                .is_some_and(|r| r.target.as_deref() == Some(id))
        {
            let pitch = self.characters.states[id].pitch;
            self.set_camera_orientation(&camera, yaw, pitch)?;
        }
        Ok(())
    }
    /// Configuration changes preserve momentum; callers must explicitly choose
    /// whether a newly lowered horizontal limit may clamp existing velocity.
    pub fn configure_character(
        &mut self,
        id: &str,
        config: CharacterConfig,
        allow_lower_limit: bool,
    ) -> Result<(), String> {
        self.ensure_character_state(id)?;
        let e = self.entity(id).unwrap();
        let state = &self.characters.states[id];
        config.motion(e, state.uniform_scale)?;
        let speed = Vec3::new(state.velocity.x, 0., state.velocity.z).length();
        if !allow_lower_limit && config.horizontal_limit > 0. && config.horizontal_limit < speed {
            return Err(
                "O novo limite reduziria o embalo atual. Autorize a redução explicitamente.".into(),
            );
        }
        self.entity_mut(id).unwrap().character3d = Some(config);
        Ok(())
    }
    pub fn apply_movement_profile(
        &mut self,
        id: &str,
        profile: MovementProfile,
        allow_lower_limit: bool,
    ) -> Result<(), String> {
        let mut config = self
            .entity(id)
            .and_then(|e| e.character3d.clone())
            .ok_or("Objeto não possui personagem 3D")?;
        config.apply_profile(profile);
        self.configure_character(id, config, allow_lower_limit)
    }
}
