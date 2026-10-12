//! Number and text data operations exposed by the typed graph adapter.
use super::*;
impl Runtime {
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
