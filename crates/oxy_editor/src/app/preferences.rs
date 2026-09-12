use super::*;

#[derive(Clone)]
pub(super) struct Preferences {
    pub scale_percent: u16,
    pub tool_names: bool,
    pub navigation_speed: f32,
    pub navigation_sensitivity: f32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            tool_names: false,
            navigation_speed: 8.,
            navigation_sensitivity: 0.003,
        }
    }
}
impl Preferences {
    fn path() -> Option<PathBuf> {
        if cfg!(test) {
            None
        } else {
            std::env::var_os("LOCALAPPDATA")
                .map(|p| PathBuf::from(p).join("OXY Engine/interface.json"))
        }
    }
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let json: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let value = Self {
            scale_percent: json["scale_percent"]
                .as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .ok_or("Percentual inválido")?,
            tool_names: json
                .get("tool_names")
                .map(|v| v.as_bool().ok_or("Preferência de nomes inválida"))
                .transpose()?
                .unwrap_or(false),
            navigation_speed: local_number(
                &json,
                "navigation_speed",
                8.,
                navigation::MIN_SPEED,
                navigation::MAX_SPEED,
            )?,
            navigation_sensitivity: local_number(
                &json,
                "navigation_sensitivity",
                0.003,
                navigation::MIN_SENSITIVITY,
                navigation::MAX_SENSITIVITY,
            )?,
        };
        if !(80..=160).contains(&value.scale_percent) {
            return Err("A escala deve ficar entre 80% e 160%.".into());
        }
        Ok(value)
    }
    pub fn load() -> (Self, Option<String>) {
        let result = Self::path().filter(|p| p.exists()).map(|p| {
            std::fs::read(p)
                .map_err(|e| e.to_string())
                .and_then(|b| Self::decode(&b))
        });
        match result {
            Some(Ok(v)) => (v, None),
            Some(Err(e)) => (
                Self::default(),
                Some(format!(
                    "Preferência de interface inválida; restaurada para 100%. {e}"
                )),
            ),
            None => (Self::default(), None),
        }
    }
    pub(super) fn save(&self) -> Result<(), String> {
        if let Some(path) = Self::path() {
            self.save_to(&path)?;
        }
        Ok(())
    }
    fn save_to(&self, path: &Path) -> Result<(), String> {
        persistence::safe_write(
            path,
            &serde_json::to_vec_pretty(&serde_json::json!({"scale_percent":self.scale_percent,
                "tool_names":self.tool_names,"navigation_speed":self.navigation_speed,
                "navigation_sensitivity":self.navigation_sensitivity}))
            .map_err(|e| e.to_string())?,
        )
    }
}
fn local_number(
    json: &serde_json::Value,
    name: &str,
    default: f32,
    min: f32,
    max: f32,
) -> Result<f32, String> {
    let Some(value) = json.get(name) else {
        return Ok(default);
    };
    let value = value
        .as_f64()
        .ok_or_else(|| format!("Preferência inválida: {name}"))? as f32;
    if !value.is_finite() {
        return Err("Preferência numérica inválida".into());
    }
    Ok(value.clamp(min, max))
}
impl Editor {
    pub(super) fn interface_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(format!("Interface: {:.0}%", self.scale * 100.))
            .on_hover_text("Ajuste o tamanho dos controles e confirme em Aplicar.")
            .clicked()
        {
            self.pause();
            self.pending_preferences = Some(self.preferences.clone());
        }
    }
    pub(super) fn preferences_ui(&mut self, ctx: &egui::Context) {
        let Some(mut pending) = self.pending_preferences.take() else {
            return;
        };
        let mut apply = false;
        let mut cancel = false;
        let response = egui::Modal::new("interface_preferences".into()).show(ctx, |ui| {
            ui.set_max_width((ctx.content_rect().width() - 48.).clamp(160., 400.));
            ui.heading("Escala da interface");
            ui.add(egui::Slider::new(&mut pending.scale_percent, 80..=160).suffix("%"));
            ui.horizontal_wrapped(|ui| {
                for value in [80, 100, 125, 150, 160] {
                    if ui
                        .selectable_label(pending.scale_percent == value, format!("{value}%"))
                        .clicked()
                    {
                        pending.scale_percent = value;
                    }
                }
            });
            ui.checkbox(&mut pending.tool_names, "Mostrar nomes das ferramentas");
            ui.separator();
            ui.label("Navegação 3D do editor");
            ui.add(egui::DragValue::new(&mut pending.navigation_speed)
                .range(navigation::MIN_SPEED..=navigation::MAX_SPEED).speed(0.1)
                .prefix("Velocidade de navegação 3D: ").suffix(" m/s"))
                .on_hover_text("Velocidade local da vista. RMB + roda ajusta em passos de ×1,2; Shift ×4, Ctrl ×0,25; juntos usam a base.");
            ui.add(egui::DragValue::new(&mut pending.navigation_sensitivity)
                .range(navigation::MIN_SENSITIVITY..=navigation::MAX_SENSITIVITY).speed(0.0001)
                .max_decimals(4).prefix("Sensibilidade do olhar: "))
                .on_hover_text("Sensibilidade local do mouse na vista de edição. Não altera a câmera de jogo nem depende da escala da interface.");
            ui.horizontal_wrapped(|ui| {
                apply = ui.button("Aplicar").clicked();
                cancel = ui.button("Cancelar").clicked();
                if ui
                    .button("Restaurar 100%")
                    .on_hover_text("Prepare 100% e confirme em Aplicar.")
                    .clicked()
                {
                    pending.scale_percent = 100;
                }
            });
        });
        cancel |= response.should_close() || ctx.input(|i| i.key_pressed(egui::Key::Escape));
        apply |= response.is_top_modal && ctx.input(|i| i.key_pressed(egui::Key::Enter));
        if cancel {
            return;
        }
        if apply && !ctx.input(|i| i.pointer.any_down()) {
            self.scale = pending.scale_percent as f32 / 100.;
            self.preferences = pending;
            ctx.set_zoom_factor(self.scale);
            if let Err(error) = self.preferences.save() {
                self.warn(format!("Não foi possível guardar a preferência: {error}"));
                self.notice_last(true);
            }
        } else {
            self.pending_preferences = Some(pending);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_preferences_load_older_settings_and_roundtrip_locally() {
        let old = Preferences::decode(br#"{"scale_percent":120,"tool_names":true}"#).unwrap();
        assert_eq!(old.navigation_speed, 8.);
        assert_eq!(old.navigation_sensitivity, 0.003);
        let mut changed = old;
        changed.navigation_speed = 9.6;
        changed.navigation_sensitivity = 0.006;
        let folder = std::env::temp_dir().join(format!("oxy-preferences-{}", new_id()));
        let path = folder.join("interface.json");
        changed.save_to(&path).unwrap();
        let loaded = Preferences::decode(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&folder).unwrap();
        assert_eq!(loaded.navigation_speed, 9.6);
        assert_eq!(loaded.navigation_sensitivity, 0.006);
        assert_eq!(loaded.scale_percent, 120);
        assert!(loaded.tool_names);
        let bounded = Preferences::decode(
            br#"{"scale_percent":100,"navigation_speed":100000,"navigation_sensitivity":-9}"#,
        )
        .unwrap();
        assert_eq!(bounded.navigation_speed, navigation::MAX_SPEED);
        assert_eq!(bounded.navigation_sensitivity, navigation::MIN_SENSITIVITY);
        assert!(Preferences::decode(br#"{"scale_percent":100,"navigation_speed":"bad"}"#).is_err());
    }
    #[test]
    fn preferences_validate_and_draft_is_independent() {
        assert!(Preferences::decode(br#"{"scale_percent":200}"#).is_err());
        assert!(Preferences::decode(b"invalid").is_err());
        let applied = Preferences::default();
        let mut draft = applied.clone();
        draft.scale_percent = 160;
        assert_eq!(applied.scale_percent, 100);
        assert_eq!(
            Preferences::decode(
                format!("{{\"scale_percent\":{}}}", draft.scale_percent).as_bytes()
            )
            .unwrap()
            .scale_percent,
            160
        );
    }
}
