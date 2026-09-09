use super::*;

#[derive(Clone)]
pub(super) struct Preferences {
    pub scale_percent: u16,
    pub tool_names: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            tool_names: false,
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
    fn save(&self) -> Result<(), String> {
        if let Some(path) = Self::path() {
            persistence::safe_write(
                &path,
                &serde_json::to_vec_pretty(&serde_json::json!({"scale_percent":self.scale_percent,"tool_names":self.tool_names})).map_err(|e| e.to_string())?,
            )?;
        }
        Ok(())
    }
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
