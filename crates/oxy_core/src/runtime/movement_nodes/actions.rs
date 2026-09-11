use super::*;
use crate::character::{
    BodyFacing, CameraLookAt, CameraMode, JumpMode, MovementProfile, TeleportOptions,
};
impl Runtime {
    pub(in crate::runtime) fn movement_action(
        &mut self,
        graph: &PreparedGraph,
        node: &Node,
        context: &mut Context,
        budget: &mut usize,
    ) -> Result<bool, String> {
        let input = |rt: &Runtime, key: &str, budget: &mut usize| {
            rt.input_value(graph, node, key, context, budget, 0)
        };
        let number = |rt: &Runtime, key: &str, budget: &mut usize| {
            input(rt, key, budget)?
                .number()
                .filter(|v| v.is_finite() && (*v as f32).is_finite())
                .map(|v| v as f32)
                .ok_or_else(|| format!("{key} precisa de número finito suportado"))
        };
        let boolean = |rt: &Runtime, key: &str, budget: &mut usize| {
            input(rt, key, budget)?
                .boolean()
                .ok_or_else(|| format!("{key} precisa ser booleano"))
        };
        let object = |rt: &Runtime, key: &str, budget: &mut usize| match input(rt, key, budget)? {
            Value::Object(Some(id)) if rt.entity(&id).is_some() => Ok(id),
            _ => Err(format!(
                "{key}: escolha um objeto existente ou conecte uma referência válida"
            )),
        };
        let target = |rt: &Runtime, budget: &mut usize| rt.target(graph, node, context, budget, 0);
        let mut outputs = Vec::new();
        let mut new_trajectory = None;
        match node.operation.as_str() {
            "character.intent" => {
                let id = target(self, budget)?;
                let axis = input(self, "axis", budget)?
                    .vector2()
                    .ok_or("Intenção exige Vetor2")?;
                self.set_movement_intent(&id, axis)?;
            }
            "character.jump" => {
                let id = target(self, budget)?;
                self.request_jump(&id)?;
            }
            "character.posture" => {
                let id = target(self, budget)?;
                let enabled = boolean(self, "enabled", budget)?;
                match node.text("command") {
                    "sprint" => self.request_sprint(&id, enabled)?,
                    "crouch" => self.request_crouch(&id, enabled)?,
                    "slide" if enabled => self.request_slide(&id)?,
                    "slide" => self.request_crouch(&id, false)?,
                    _ => return Err("Comando de postura desconhecido".into()),
                };
            }
            "character.velocity" => {
                let id = target(self, budget)?;
                let velocity = input(self, "velocity", budget)?
                    .vector3()
                    .ok_or("Velocidade exige Vetor3")?;
                match node.text("mode") {
                    "add" => self.add_character_velocity(&id, velocity)?,
                    "set" => self.set_character_velocity(&id, velocity)?,
                    _ => return Err("Operação de velocidade desconhecida".into()),
                };
            }
            "character.block" => {
                let id = target(self, budget)?;
                let blocked = boolean(self, "blocked", budget)?;
                let look = match node.text("channel") {
                    "look" => true,
                    "movement" => false,
                    _ => return Err("Canal deve ser Movimento ou Olhar".into()),
                };
                self.block_character_input(&id, look, node.text("reason"), blocked)?;
            }
            "character.profile" => {
                let id = target(self, budget)?;
                let allowed = boolean(self, "allow_limit", budget)?;
                let profile = match node.text("profile") {
                    "direct" => MovementProfile::Direct,
                    "parkour" => MovementProfile::Parkour,
                    "chained" => MovementProfile::ChainedJumps,
                    _ => return Err("Perfil desconhecido".into()),
                };
                self.apply_movement_profile(&id, profile, allowed)?;
            }
            "character.parameter" => {
                let id = target(self, budget)?;
                let value = number(self, "value", budget)?;
                let allowed = boolean(self, "allow_limit", budget)?;
                self.set_character_parameter(&id, node.text("parameter"), value, allowed)?;
            }
            "character.option" => {
                let id = target(self, budget)?;
                let enabled = boolean(self, "enabled", budget)?;
                self.set_character_option(&id, node.text("option"), enabled)?;
            }
            "character.yaw" => {
                let id = target(self, budget)?;
                let yaw = number(self, "yaw", budget)?.to_radians();
                self.set_character_yaw(&id, yaw)?;
            }
            "character.teleport" => {
                let result = (|| {
                    let id = target(self, budget)?;
                    let position = input(self, "position", budget)?
                        .vector3()
                        .ok_or("Destino exige Vetor3")?;
                    let keep_velocity = boolean(self, "keep_velocity", budget)?;
                    let search_radius = number(self, "search", budget)?;
                    let yaw = if boolean(self, "restore_yaw", budget)? {
                        Some(number(self, "yaw", budget)?.to_radians())
                    } else {
                        None
                    };
                    let look = if boolean(self, "restore_look", budget)? {
                        let v = input(self, "look", budget)?
                            .vector2()
                            .ok_or("Olhar exige Vetor2 em graus")?;
                        Some([v.x.to_radians(), v.y.to_radians()])
                    } else {
                        None
                    };
                    let destination = self.teleport_character(
                        &id,
                        position,
                        TeleportOptions {
                            keep_velocity,
                            yaw,
                            look,
                            search_radius,
                        },
                    )?;
                    new_trajectory = Some((id.clone(), self.characters.states[&id].trajectory));
                    Ok::<_, String>(destination)
                })();
                outputs.extend([
                    ("success", Value::Bool(result.is_ok())),
                    (
                        "position",
                        Value::Vector3(result.as_ref().copied().unwrap_or(Vec3::ZERO).to_array()),
                    ),
                    ("error", Value::Text(result.err().unwrap_or_default())),
                ]);
            }
            "camera.activate" => {
                let id = target(self, budget)?;
                let seconds = number(self, "seconds", budget)?;
                self.activate_camera(&id, seconds)?;
            }
            "camera.mode" => {
                let id = target(self, budget)?;
                let seconds = number(self, "seconds", budget)?;
                let mode = match node.text("mode") {
                    "first_person" => CameraMode::FirstPerson,
                    "third_person" => CameraMode::ThirdPerson,
                    "fixed" => CameraMode::Fixed,
                    _ => return Err("Modo de câmera desconhecido".into()),
                };
                self.set_camera_mode(&id, mode, seconds)?;
            }
            "camera.target" => {
                let id = target(self, budget)?;
                let followed = object(self, "object", budget)?;
                self.set_camera_target(&id, &followed)?;
            }
            "camera.look_at" => {
                let id = target(self, budget)?;
                let focus = match node.text("mode") {
                    "point" => Some(CameraLookAt::Point(
                        input(self, "point", budget)?
                            .vector3()
                            .ok_or("Ponto exige Vetor3")?,
                    )),
                    "object" => Some(CameraLookAt::Object(object(self, "object", budget)?)),
                    "clear" => None,
                    _ => return Err("Modo do olhar assistido desconhecido".into()),
                };
                self.set_camera_look_at(&id, focus)?;
            }
            "camera.setting" => {
                let id = target(self, budget)?;
                let value = number(self, "value", budget)?;
                let seconds = number(self, "seconds", budget)?;
                self.set_camera_setting(&id, node.text("setting"), value, seconds)?;
            }
            "surface.apply" => {
                let id = target(self, budget)?;
                let Value::Surface(surface) = input(self, "surface", budget)? else {
                    return Err("Entrada exige referência de superfície física".into());
                };
                self.apply_surface(&id, surface)?;
            }
            "query.ray" | "query.sphere" | "query.capsule" | "query.space" => {
                let result = (|| {
                    let origin = input(self, "origin", budget)?
                        .vector3()
                        .ok_or("Origem exige Vetor3")?;
                    let direction = input(self, "direction", budget)?
                        .vector3()
                        .ok_or("Direção exige Vetor3")?;
                    let distance = number(self, "distance", budget)?;
                    let radius = number(self, "radius", budget)?;
                    let height = number(self, "height", budget)?;
                    let shape = match node.operation.as_str() {
                        "query.ray" => SceneQueryShape::Ray,
                        "query.sphere" => SceneQueryShape::Sphere { radius },
                        "query.capsule" => SceneQueryShape::Capsule { height, radius },
                        _ => SceneQueryShape::Space { height, radius },
                    };
                    let include_sensors = boolean(self, "sensors", budget)?;
                    let Value::Object(exclude) = input(self, "exclude", budget)? else {
                        return Err("Exclusão exige referência de objeto".into());
                    };
                    let mut bits = |key| {
                        let n = input(self, key, budget)?
                            .number()
                            .ok_or("Filtro exige número inteiro")?;
                        if !n.is_finite()
                            || n.fract() != 0.
                            || !(0. ..=f64::from(u32::MAX)).contains(&n)
                        {
                            return Err("Filtro deve ser inteiro entre 0 e 4294967295".to_string());
                        }
                        Ok(n as u32)
                    };
                    let category = bits("category")?;
                    let mask = bits("mask")?;
                    self.query_scene(&SceneQuery {
                        shape,
                        origin,
                        direction,
                        distance,
                        include_sensors,
                        exclude,
                        category,
                        mask,
                    })
                })();
                let hit = result.as_ref().ok().and_then(|hit| hit.as_ref());
                outputs.extend([
                    ("hit", Value::Bool(hit.is_some())),
                    ("free", Value::Bool(result.is_ok() && hit.is_none())),
                    ("object", Value::Object(hit.map(|h| h.object.clone()))),
                    (
                        "point",
                        Value::Vector3(hit.map_or([0.; 3], |h| h.point.to_array())),
                    ),
                    (
                        "normal",
                        Value::Vector3(hit.map_or([0.; 3], |h| h.normal.to_array())),
                    ),
                    (
                        "distance",
                        Value::Number(hit.map_or(0., |h| f64::from(h.distance))),
                    ),
                    (
                        "fraction",
                        Value::Number(hit.map_or(0., |h| f64::from(h.fraction))),
                    ),
                    (
                        "surface",
                        Value::Surface(hit.and_then(|h| h.surface.clone())),
                    ),
                    ("success", Value::Bool(result.is_ok())),
                    ("error", Value::Text(result.err().unwrap_or_default())),
                ]);
            }
            _ => return Ok(false),
        }
        if let Some(trajectory) = new_trajectory {
            context.trajectory = Some(trajectory);
        }
        if !outputs.is_empty() {
            let stored = Arc::make_mut(&mut context.outputs);
            for (key, value) in outputs {
                stored.insert((node.id.clone(), key.into()), value);
            }
        }
        Ok(true)
    }
    pub fn set_character_parameter(
        &mut self,
        id: &str,
        parameter: &str,
        value: f32,
        allow_limit: bool,
    ) -> Result<(), String> {
        let mut c = self
            .entity(id)
            .and_then(|e| e.character3d.clone())
            .ok_or("Objeto não possui personagem 3D")?;
        let field = match parameter {
            "speed" => &mut c.speed,
            "sprint_speed" => &mut c.sprint_speed,
            "crouch_speed" => &mut c.crouch_speed,
            "gravity" => &mut c.gravity,
            "jump_speed" => &mut c.jump_speed,
            "ground_acceleration" => &mut c.ground_acceleration,
            "ground_braking" => &mut c.ground_braking,
            "ground_friction" => &mut c.ground_friction,
            "air_acceleration" => &mut c.air_acceleration,
            "air_projected_limit" => &mut c.air_projected_limit,
            "air_resistance" => &mut c.air_resistance,
            "horizontal_limit" => &mut c.horizontal_limit,
            "jump_retention" => &mut c.jump_retention,
            "landing_retention" => &mut c.landing_retention,
            "coyote_ms" => &mut c.coyote_ms,
            "jump_buffer_ms" => &mut c.jump_buffer_ms,
            "slide_duration" => &mut c.slide_duration,
            "slide_friction" => &mut c.slide_friction,
            "slide_control" => &mut c.slide_control,
            "angular_speed" => &mut c.angular_speed,
            _ => return Err("Parâmetro não permitido; use uma opção disponível no nó.".into()),
        };
        *field = value;
        self.configure_character(id, c, allow_limit)
    }
    pub fn set_character_option(
        &mut self,
        id: &str,
        option: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let mut c = self
            .entity(id)
            .and_then(|e| e.character3d.clone())
            .ok_or("Objeto não possui personagem 3D")?;
        match option {
            "automatic_input" => c.automatic_input = enabled,
            "slide_enabled" => c.slide_enabled = enabled,
            "crouch_toggle" => c.crouch_toggle = enabled,
            "automatic_jump" => {
                c.jump_mode = if enabled {
                    JumpMode::Automatic
                } else {
                    JumpMode::Manual
                }
            }
            "face_look" => {
                c.facing = if enabled {
                    BodyFacing::Look
                } else {
                    BodyFacing::Movement
                }
            }
            _ => return Err("Opção de personagem não permitida".into()),
        };
        self.configure_character(id, c, false)
    }
}
