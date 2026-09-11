//! Shared commands for native input, nodes and future scripting. Values live
//! only in the isolated runtime; none of these commands edit project assets.
use super::*;
impl Runtime {
    pub(in crate::runtime) fn initialize_character_states(&mut self) -> Result<(), String> {
        let ids: Vec<_> = self
            .scene()
            .entities
            .iter()
            .filter(|e| e.character3d.is_some())
            .map(|e| e.id.clone())
            .collect();
        for id in ids {
            self.ensure_character_state(&id)?;
        }
        Ok(())
    }
    pub fn request_slide(&mut self, id: &str) -> Result<(), String> {
        self.ensure_character_state(id)?;
        if !self
            .entity(id)
            .unwrap()
            .character3d
            .as_ref()
            .unwrap()
            .slide_enabled
        {
            return Err("Deslize desativado na configuração do personagem.".into());
        }
        self.characters.crouches.insert(id.into(), true);
        self.characters.slides.insert(id.into());
        Ok(())
    }
    pub(in crate::runtime) fn ensure_character_state(&mut self, id: &str) -> Result<(), String> {
        if self.characters.states.contains_key(id) {
            return Ok(());
        }
        let entity = self.entity(id).ok_or("Personagem ausente")?;
        let config = entity
            .character3d
            .as_ref()
            .ok_or("Objeto não possui personagem 3D")?;
        let (position, rotation, scale) =
            crate::physics3d::world_pose(self.scene().world_matrix(id)?)?;
        character_scale(rotation, scale)?;
        let motion = config.motion(entity, scale.x)?;
        let forward = rotation * -Vec3::Z;
        let mut state = CharacterState::new(position, (-forward.x).atan2(-forward.z));
        state.height = motion.height;
        state.uniform_scale = scale.x;
        self.characters.states.insert(id.into(), state);
        Ok(())
    }
    pub fn request_sprint(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        self.ensure_character_state(id)?;
        self.characters.sprints.insert(id.into(), enabled);
        Ok(())
    }
    pub fn request_crouch(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        self.ensure_character_state(id)?;
        self.characters.crouches.insert(id.into(), enabled);
        Ok(())
    }
    /// Independent reasons compose. Removing one reason never clears another.
    pub fn block_character_input(
        &mut self,
        id: &str,
        look: bool,
        reason: &str,
        blocked: bool,
    ) -> Result<(), String> {
        if reason.trim().is_empty() || reason.len() > 256 {
            return Err("Informe um motivo de bloqueio entre 1 e 256 caracteres.".into());
        }
        self.ensure_character_state(id)?;
        let state = self.characters.states.get_mut(id).unwrap();
        let reasons = if look {
            &mut state.look_blocks
        } else {
            &mut state.movement_blocks
        };
        if blocked {
            reasons.insert(reason.into());
        } else {
            reasons.remove(reason);
        }
        if !look && blocked {
            state.jump_until = None;
        }
        Ok(())
    }
    /// Set total world velocity, not a walking speed or a position increment.
    pub fn set_character_velocity(&mut self, id: &str, velocity: Vec3) -> Result<(), String> {
        if !velocity.is_finite() {
            return Err("Velocidade inválida".into());
        }
        self.ensure_character_state(id)?;
        let state = self.characters.states.get_mut(id).unwrap();
        let detach = velocity.y > 0. && state.grounded;
        let next = velocity
            - if detach {
                Vec3::ZERO
            } else {
                state.support.as_ref().map_or(Vec3::ZERO, |s| s.velocity)
            };
        if !next.is_finite() {
            return Err("Velocidade relativa fora do intervalo suportado.".into());
        }
        if detach {
            state.support = None;
            state.grounded = false;
            state.jump_consumed = true;
            state.coyote_until = f64::NEG_INFINITY;
            state.jump_until = None;
        }
        state.velocity = next;
        if detach {
            self.characters.pending_records.push(MovementRecord {
                object: id.into(),
                event: MovementEvent::LeftSupport,
                state: Arc::new(state.clone()),
            });
        }
        Ok(())
    }
    pub fn add_character_velocity(&mut self, id: &str, delta: Vec3) -> Result<(), String> {
        if !delta.is_finite() {
            return Err("Impulso inválido".into());
        }
        self.ensure_character_state(id)?;
        let policy = self
            .entity(id)
            .unwrap()
            .character3d
            .as_ref()
            .unwrap()
            .inherit_platform;
        let state = self.characters.states.get_mut(id).unwrap();
        let mut next = state.velocity + delta;
        let detach = next.y > 0. && state.grounded;
        if detach {
            next += state
                .support
                .as_ref()
                .map_or(Vec3::ZERO, |s| motor::inherited(s.velocity, policy));
        }
        if !next.is_finite() {
            return Err("Impulso excede o intervalo numérico suportado.".into());
        }
        state.velocity = next;
        if detach {
            state.support = None;
            state.grounded = false;
            state.jump_consumed = true;
            state.coyote_until = f64::NEG_INFINITY;
            state.jump_until = None;
            self.characters.pending_records.push(MovementRecord {
                object: id.into(),
                event: MovementEvent::LeftSupport,
                state: Arc::new(state.clone()),
            });
        }
        Ok(())
    }
}
