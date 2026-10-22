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
                let integer = match node.text("mode") {
                    "real" => false,
                    "integer" => true,
                    // Refused after the range, as the draw reads them.
                    _ => {
                        check_range(min, max)?;
                        return Err("Modo de sorteio inválido".into());
                    }
                };
                let value = self.random_number((&context.owner, &node.id), min, max, integer)?;
                Arc::make_mut(&mut context.outputs)
                    .insert((node.id.clone(), "value".into()), Value::Number(value));
            }
            "random.seed" => {
                let seed = number(self, "seed", "Semente", budget)?;
                self.set_random_seed(seed)?;
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
                return Self::format_time(seconds, node.text("format")).map(Value::Text);
            }
            _ => return self.movement_data(graph, node, output, context, budget, depth),
        };
        if !value.is_finite() {
            return Err("Resultado aritmético não finito".into());
        }
        // Adding zero turns -0 (ceil of -0.5, -3 mod 3…) into 0, which the HUD would show as -0.
        Ok(Value::Number(value + 0.0))
    }

    /// Draws a number from `min` to `max`, a real one or, with `integer`, a whole one. The
    /// numbers come from the run's seed and from how many `caller` (the object that draws and
    /// what in it asks, a node's ID) drew before, so the same seed gives the same numbers to
    /// each caller, whatever the others draw.
    pub fn random_number(
        &mut self,
        caller: (&str, &str),
        min: f64,
        max: f64,
        integer: bool,
    ) -> Result<f64, String> {
        let key = (caller.0.to_owned(), caller.1.to_owned());
        let drawn = self.run.rolls.get(&key).copied().unwrap_or(0);
        let seed = self.run.seed.to_le_bytes();
        let parts = [
            &seed,
            key.0.as_bytes(),
            key.1.as_bytes(),
            &drawn.to_le_bytes(),
        ];
        let value = draw(&parts, min, max, integer)?;
        self.run.rolls.insert(key, drawn + 1);
        Ok(value)
    }

    /// `random_number` for the script `script` that `owner` attaches (`random.number`,
    /// `random.integer`), from its sequence `stream` (the empty name is its own): counted apart
    /// from the graph's draws and from every other script's and sequence's.
    pub fn script_random_number(
        &mut self,
        owner: &str,
        script: &str,
        stream: &str,
        min: f64,
        max: f64,
        integer: bool,
    ) -> Result<f64, String> {
        let key = (owner.to_owned(), script.to_owned(), stream.to_owned());
        let drawn = self.run.script_rolls.get(&key).copied().unwrap_or(0);
        let seed = self.run.seed.to_le_bytes();
        let parts = [
            b"oxy.script".as_slice(),
            &seed,
            key.0.as_bytes(),
            key.1.as_bytes(),
            key.2.as_bytes(),
            &drawn.to_le_bytes(),
        ];
        let value = draw(&parts, min, max, integer)?;
        self.run.script_rolls.insert(key, drawn + 1);
        Ok(value)
    }

    /// Draws come from `seed`, a whole number from 0 to [`MAX_SEED`], and every caller's count
    /// starts over, the scripts' too.
    pub fn set_random_seed(&mut self, seed: f64) -> Result<(), String> {
        if seed.fract() != 0.0 || !(0.0..=MAX_SEED as f64).contains(&seed) {
            return Err(format!(
                "A semente precisa ser um inteiro de 0 a {MAX_SEED}; recebeu {seed}"
            ));
        }
        self.run.seed = seed as u64;
        self.run.rolls.clear();
        self.run.script_rolls.clear();
        Ok(())
    }

    /// `seconds` in `format` (mm:ss, mm:ss.cc, m:ss.cc, ss.cc, mm:ss.mmm or h:mm:ss),
    /// truncated to the format's last digit (the 1e-6 keeps 1.15 from reading 00:01.14).
    /// Minutes keep counting past 59 unless the format shows hours.
    pub fn format_time(seconds: f64, format: &str) -> Result<String, String> {
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
}

/// A number from `min` to `max` (whole with `integer`) of the generator seeded with `parts`.
fn draw(parts: &[&[u8]], min: f64, max: f64, integer: bool) -> Result<f64, String> {
    if !min.is_finite() {
        return Err("Mínimo precisa ser um número finito".into());
    }
    if !max.is_finite() {
        return Err("Máximo precisa ser um número finito".into());
    }
    check_range(min, max)?;
    let mut rng = Rng::new(random::hash(parts));
    let value = if integer {
        let (low, high) = (min.ceil(), max.floor());
        if low > high {
            return Err(format!(
                "Nenhum inteiro entre o mínimo {min} e o máximo {max}"
            ));
        }
        let limit = MAX_SEED as f64;
        if low < -limit || high > limit {
            return Err(format!(
                "No modo Inteiro, mínimo e máximo precisam estar entre -{MAX_SEED} e \
                 {MAX_SEED}"
            ));
        }
        rng.int_inclusive(low as i64, high as i64) as f64
    } else {
        rng.range(min, max)
    };
    // Adding zero turns -0 into 0, as the number nodes do.
    Ok(value + 0.0)
}

/// A range from `min` to `max` has its minimum first.
fn check_range(min: f64, max: f64) -> Result<(), String> {
    if min > max {
        return Err(format!("Mínimo {min} maior que o máximo {max}"));
    }
    Ok(())
}
