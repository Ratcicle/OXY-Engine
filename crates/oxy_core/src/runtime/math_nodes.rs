//! Number and text operations exposed by the typed graph adapter, random draws included.
use super::*;
use crate::random::{self, MAX_SEED, Rng};
impl Runtime {
    /// Runs `node` when it is one of these actions, else hands it to the movement ones; false
    /// when no one runs it.
    pub(super) fn math_action(
        &mut self,
        graph: &PreparedGraph,
        node: &Node,
        context: &mut Context,
        budget: &mut usize,
    ) -> Result<bool, String> {
        let number = |rt: &Runtime, key: &str, name: &str, budget: &mut usize| {
            rt.input_value(graph, node, key, context, budget, 0)?
                .number()
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("{name} precisa ser um número finito"))
        };
        match node.operation.as_str() {
            // An action, so that each execution draws once and whoever reads Valor afterwards
            // gets that number; a data node would draw again on every read.
            "random.number" => {
                let min = number(self, "min", "Mínimo", budget)?;
                let max = number(self, "max", "Máximo", budget)?;
                if min > max {
                    return Err(format!("Mínimo {min} maior que o máximo {max}"));
                }
                let key = (context.owner.clone(), node.id.clone());
                let drawn = self.run.rolls.get(&key).copied().unwrap_or(0);
                let mut rng = Rng::new(random::hash(&[
                    &self.run.seed.to_le_bytes(),
                    key.0.as_bytes(),
                    key.1.as_bytes(),
                    &drawn.to_le_bytes(),
                ]));
                let value = match node.text("mode") {
                    "real" => rng.range(min, max),
                    "integer" => {
                        let (low, high) = (min.ceil(), max.floor());
                        if low > high {
                            return Err(format!(
                                "Nenhum inteiro entre o mínimo {min} e o máximo {max}"
                            ));
                        }
                        let limit = MAX_SEED as f64;
                        if low < -limit || high > limit {
                            return Err(format!(
                                "No modo Inteiro, mínimo e máximo precisam estar entre -{MAX_SEED} \
                                 e {MAX_SEED}"
                            ));
                        }
                        rng.int_inclusive(low as i64, high as i64) as f64
                    }
                    _ => return Err("Modo de sorteio inválido".into()),
                };
                self.run.rolls.insert(key, drawn + 1);
                // Adding zero turns -0 into 0, as the other number nodes do.
                Arc::make_mut(&mut context.outputs).insert(
                    (node.id.clone(), "value".into()),
                    Value::Number(value + 0.0),
                );
            }
            "random.seed" => {
                let seed = number(self, "seed", "Semente", budget)?;
                if seed.fract() != 0.0 || !(0.0..=MAX_SEED as f64).contains(&seed) {
                    return Err(format!(
                        "A semente precisa ser um inteiro de 0 a {MAX_SEED}; recebeu {seed}"
                    ));
                }
                self.run.seed = seed as u64;
                self.run.rolls.clear();
            }
            _ => return self.movement_action(graph, node, context, budget),
        }
        Ok(true)
    }
    pub(super) fn math_data(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        output: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        let number = |key: &str, name: &str, budget: &mut usize| {
            self.input_value(graph, node, key, context, budget, depth + 1)?
                .number()
                .ok_or_else(|| format!("{name} precisa ser número"))
        };
        let value = match node.operation.as_str() {
            "math.binary" => {
                let a = number("a", "A", budget)?;
                let b = number("b", "B", budget)?;
                match node.text("operator") {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" if b != 0.0 => a / b,
                    "/" => return Err("Divisão por zero".into()),
                    "%" if b != 0.0 => a.rem_euclid(b),
                    "%" => return Err("Resto por zero".into()),
                    "min" => a.min(b),
                    "max" => a.max(b),
                    _ => return Err("Operador aritmético inválido".into()),
                }
            }
            "math.unary" => {
                let value = number("value", "Valor", budget)?;
                match node.text("operator") {
                    "floor" => value.floor(),
                    "ceil" => value.ceil(),
                    "round" => value.round(),
                    "trunc" => value.trunc(),
                    "abs" => value.abs(),
                    "sign" if value == 0.0 => 0.0,
                    "sign" => value.signum(),
                    "sqrt" if value < 0.0 => {
                        return Err(format!("Raiz quadrada de número negativo ({value})"));
                    }
                    "sqrt" => value.sqrt(),
                    _ => return Err("Função matemática inválida".into()),
                }
            }
            "math.clamp" => {
                let value = number("value", "Valor", budget)?;
                let min = number("min", "Mínimo", budget)?;
                let max = number("max", "Máximo", budget)?;
                if min > max {
                    return Err(format!("Mínimo {min} maior que o máximo {max}"));
                }
                // NaN bounds would make clamp panic.
                if min.is_nan() || max.is_nan() {
                    return Err("Mínimo e máximo precisam ser números".into());
                }
                value.clamp(min, max)
            }
            "text.time" => {
                let seconds = number("seconds", "Segundos", budget)?;
                return time_text(seconds, node.text("format")).map(Value::Text);
            }
            _ => return self.movement_data(graph, node, output, context, budget, depth),
        };
        if !value.is_finite() {
            return Err("Resultado aritmético não finito".into());
        }
        // Adding zero turns -0 (ceil of -0.5, -3 mod 3…) into 0, which the HUD would show as -0.
        Ok(Value::Number(value + 0.0))
    }
}

/// `seconds` in `format`, truncated to the format's last digit (the 1e-6 keeps 1.15 from
/// reading 00:01.14). Minutes keep counting past 59 unless the format shows hours.
fn time_text(seconds: f64, format: &str) -> Result<String, String> {
    let decimals = match format {
        "mm:ss" | "h:mm:ss" => 0,
        "mm:ss.cc" | "m:ss.cc" | "ss.cc" => 2,
        "mm:ss.mmm" => 3,
        _ => return Err("Formato de tempo inválido".into()),
    };
    if !seconds.is_finite() {
        return Err("Segundos precisam ser um número finito".into());
    }
    let scale = 10_u64.pow(decimals);
    let units = (seconds.abs() * scale as f64 + 1e-6).floor();
    if units >= 2_f64.powi(53) {
        return Err("Tempo grande demais para formatar".into());
    }
    let units = units as u64;
    let (whole, fraction) = (units / scale, units % scale);
    let sign = if seconds < 0.0 && units > 0 { "-" } else { "" };
    let body = match format {
        "h:mm:ss" => format!("{}:{:02}:{:02}", whole / 3600, whole / 60 % 60, whole % 60),
        "m:ss.cc" => format!("{}:{:02}", whole / 60, whole % 60),
        "ss.cc" => format!("{whole:02}"),
        _ => format!("{:02}:{:02}", whole / 60, whole % 60),
    };
    Ok(match decimals {
        0 => format!("{sign}{body}"),
        _ => format!("{sign}{body}.{fraction:0width$}", width = decimals as usize),
    })
}
