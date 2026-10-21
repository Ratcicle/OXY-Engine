use super::*;
use glam::{DVec2, DVec3, EulerRot};
impl Runtime {
    pub(in crate::runtime) fn movement_data(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        output: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        let input = |key: &str, budget: &mut usize| {
            self.input_value(graph, node, key, context, budget, depth + 1)
        };
        let number = |key: &str, budget: &mut usize| {
            input(key, budget)?
                .number()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("{key} precisa de número finito"))
        };
        let value = match node.operation.as_str() {
            "input.read" => Value::Bool(match output {
                "pressed" => self.input_pressed(node.text("action")),
                "held" => self.input_held(node.text("action")),
                "released" => self.input_released(node.text("action")),
                _ => return Err("Saída de entrada ausente".into()),
            }),
            "input.axes" => match output {
                "look" => Value::Vector2(self.input_look().to_array()),
                "wheel" => Value::Number(f64::from(self.input_wheel())),
                "movement" => {
                    let target = self.target(graph, node, context, budget, depth + 1)?;
                    Value::Vector2(self.input_movement(&target)?.to_array())
                }
                _ => return Err("Eixo de entrada ausente".into()),
            },
            "time.read" => Value::Number(if output == "dt" {
                f64::from(FIXED_DT)
            } else {
                self.game_time()
            }),
            "character.state" => {
                let id = self.target(graph, node, context, budget, depth + 1)?;
                let state = self.inspect_character(&id)?;
                state_value(&state, output)?
            }
            "transform.read" => {
                let id = self.target(graph, node, context, budget, depth + 1)?;
                let space = match node.text("space") {
                    "local" => TransformSpace::Local,
                    "world" => TransformSpace::World,
                    _ => {
                        self.entity(&id).ok_or("Objeto ausente")?;
                        return Err("Espaço deve ser Local ou Mundo".into());
                    }
                };
                let (position, rotation) = self.object_pose(&id, space)?;
                let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
                match output {
                    "position" => Value::Vector3(position.to_array()),
                    "rotation" => Value::Vector3([x.to_degrees(), y.to_degrees(), z.to_degrees()]),
                    "yaw" => Value::Number(f64::from(yaw_of(rotation * -Vec3::Z))),
                    _ => return Err("Saída de transformação ausente".into()),
                }
            }
            "surface.get" => {
                let id = self.target(graph, node, context, budget, depth + 1)?;
                Value::Surface(self.object_surface(&id)?)
            }
            "camera.read" => {
                let forward = self.camera_orientation()? * -Vec3::Z;
                match output {
                    "camera" => Value::Object(self.active_camera().map(str::to_owned)),
                    "look" => Value::Vector2([
                        (-forward.x).atan2(-forward.z).to_degrees(),
                        forward.y.clamp(-1., 1.).asin().to_degrees(),
                    ]),
                    "forward" => Value::Vector3(forward.to_array()),
                    "mode" => Value::Text(
                        match self.active_camera().and_then(|id| self.camera_mode(id)) {
                            Some(crate::character::CameraMode::FirstPerson) => "Primeira pessoa",
                            Some(crate::character::CameraMode::ThirdPerson) => "Terceira pessoa",
                            _ => "Fixa",
                        }
                        .into(),
                    ),
                    _ => return Err("Saída de câmera ausente".into()),
                }
            }
            "vector2.make" => {
                Value::Vector2([number("x", budget)? as f32, number("y", budget)? as f32])
            }
            "vector3.make" => Value::Vector3([
                number("x", budget)? as f32,
                number("y", budget)? as f32,
                number("z", budget)? as f32,
            ]),
            "vector2.parts" => {
                let v = input("value", budget)?
                    .vector2()
                    .ok_or("Entrada exige Vetor2")?;
                Value::Number(f64::from(match output {
                    "x" => v.x,
                    "y" => v.y,
                    _ => return Err("Componente vetorial ausente".into()),
                }))
            }
            "vector3.parts" => {
                let v = input("value", budget)?
                    .vector3()
                    .ok_or("Entrada exige Vetor3")?;
                Value::Number(f64::from(match output {
                    "x" => v.x,
                    "y" => v.y,
                    "z" => v.z,
                    _ => return Err("Componente vetorial ausente".into()),
                }))
            }
            "vector2.math" => {
                let a = input("a", budget)?
                    .vector2()
                    .ok_or("A exige Vetor2")?
                    .as_dvec2();
                match output {
                    "length" => Value::Number(a.length()),
                    "dot" => Value::Number(
                        a.dot(
                            input("b", budget)?
                                .vector2()
                                .ok_or("B exige Vetor2")?
                                .as_dvec2(),
                        ),
                    ),
                    _ => Value::Vector2(
                        match output {
                            "sum" => {
                                a + input("b", budget)?
                                    .vector2()
                                    .ok_or("B exige Vetor2")?
                                    .as_dvec2()
                            }
                            "difference" => {
                                a - input("b", budget)?
                                    .vector2()
                                    .ok_or("B exige Vetor2")?
                                    .as_dvec2()
                            }
                            "scaled" => a * number("scalar", budget)?,
                            "normalized" => a.try_normalize().unwrap_or(DVec2::ZERO),
                            _ => return Err("Operação vetorial ausente".into()),
                        }
                        .as_vec2()
                        .to_array(),
                    ),
                }
            }
            "vector3.math" => {
                let a = input("a", budget)?
                    .vector3()
                    .ok_or("A exige Vetor3")?
                    .as_dvec3();
                match output {
                    "length" => Value::Number(a.length()),
                    "dot" => Value::Number(
                        a.dot(
                            input("b", budget)?
                                .vector3()
                                .ok_or("B exige Vetor3")?
                                .as_dvec3(),
                        ),
                    ),
                    _ => Value::Vector3(
                        match output {
                            "sum" => {
                                a + input("b", budget)?
                                    .vector3()
                                    .ok_or("B exige Vetor3")?
                                    .as_dvec3()
                            }
                            "difference" => {
                                a - input("b", budget)?
                                    .vector3()
                                    .ok_or("B exige Vetor3")?
                                    .as_dvec3()
                            }
                            "scaled" => a * number("scalar", budget)?,
                            "normalized" => a.try_normalize().unwrap_or(DVec3::ZERO),
                            _ => return Err("Operação vetorial ausente".into()),
                        }
                        .as_vec3()
                        .to_array(),
                    ),
                }
            }
            "vector.direction" => {
                let target = self.target(graph, node, context, budget, depth + 1)?;
                let rotation = self.world_rotation(&target)?;
                let v = input("value", budget)?
                    .vector3()
                    .ok_or("Direção exige Vetor3")?;
                Value::Vector3(
                    match node.text("space") {
                        "local_to_world" => rotation * v,
                        "world_to_local" => rotation.inverse() * v,
                        _ => {
                            return Err(
                                "Conversão deve ser Local para Mundo ou Mundo para Local".into()
                            );
                        }
                    }
                    .to_array(),
                )
            }
            _ => {
                return Err(format!(
                    "Dados da saída {output} indisponíveis; execute a ação produtora antes"
                ));
            }
        };
        if !value.is_finite() {
            return Err(format!("Resultado não finito em {}.{output}", node.id));
        }
        Ok(value)
    }
}
