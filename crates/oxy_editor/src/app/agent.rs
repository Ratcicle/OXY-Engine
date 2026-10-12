//! Edits requested by AI agents. Each batch of project commands, and each imported PNG or WAV,
//! is one undo step labelled "Agente: …", logged in the console with the Agente origin, and
//! refused while a person is in the middle of something the edit could collide with.
use super::*;
use oxy_core::{
    asset_import::{self, ImportKind, ImportRequest},
    command::{self, Command},
};
use serde_json::Value as Json;

/// Console prefix of messages about agent edits; the console shows it as the line's origin.
pub(crate) const AGENT_PREFIX: &str = "Agente · ";

impl Editor {
    /// Why the editor cannot take agent edits right now, if it cannot.
    pub(crate) fn agent_busy(&self) -> Option<&'static str> {
        if self.home.visible {
            Some("Nenhum projeto aberto no editor.")
        } else if self.playing() {
            Some("O jogo está em teste no editor. Pare o teste para editar.")
        } else if self.mesh_operation_active() {
            Some("Há uma operação de modelagem em prévia no editor.")
        } else if self.studio.playing || !self.studio.animation.drafts.is_empty() {
            Some("Há uma animação tocando ou uma pose provisória no Estúdio.")
        } else if self.pending.is_some() {
            Some("O editor está esperando uma decisão sobre alterações não salvas.")
        } else {
            None
        }
    }
    /// A person is dragging, typing or has an edit in progress: agent edits wait for it.
    pub(crate) fn person_editing(&self, ctx: &egui::Context) -> bool {
        self.history.is_pending()
            || ctx.input(|i| i.pointer.any_down())
            || crate::graph_ui::text_input_active(ctx)
    }
    /// Applies an agent's batch as one undo step and answers with [`command::report`], as the
    /// MCP server does on files. On failure nothing changes.
    pub(crate) fn execute_agent(&mut self, commands: &[Command]) -> Result<Json, String> {
        if let Some(reason) = self.agent_busy() {
            return Err(reason.into());
        }
        if commands.is_empty() {
            // No undo step for nothing; the answer leaves out the project's warnings, as on
            // files.
            let before = command::batch_warnings(&self.state.project);
            return Ok(command::report(&self.state.project, &[], &before));
        }
        let label = match commands {
            [one] => format!("Agente: {}", one.label()),
            many => format!("Agente: {} comandos", many.len()),
        };
        self.history
            .begin(label, &self.state.project, &self.state.images);
        match command::apply_and_report(&mut self.state.project, &self.scene_id, commands) {
            Ok(report) => {
                if let Err(error) = self
                    .history
                    .commit(&self.state.project, &mut self.state.images)
                {
                    self.log(error);
                }
                for command in commands {
                    self.log(format!("{AGENT_PREFIX}{}", command.label()));
                }
                self.after_external_edit();
                Ok(report)
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                let message = error.to_string();
                self.log(format!("{AGENT_PREFIX}{message}"));
                Err(message)
            }
        }
    }
    /// Imports a PNG or WAV for an agent, as the editor's Importar does but without the dialog:
    /// one undo step that carries the image, so the texture cache holds its pixels, the viewport
    /// and the game interface show it at once, and undo takes them away again. `scene` is where
    /// `apply_to` is looked for.
    pub(crate) fn import_agent(
        &mut self,
        request: &ImportRequest,
        scene: &str,
    ) -> Result<Json, String> {
        if let Some(reason) = self.agent_busy() {
            return Err(reason.into());
        }
        if self.path.is_none() {
            return Err(
                "O projeto ainda não tem pasta: peça para a pessoa salvá-lo antes de importar \
                 recursos."
                    .into(),
            );
        }
        let source = match request.read() {
            Ok(source) => source,
            Err(error) => {
                self.log(format!("{AGENT_PREFIX}{error}"));
                return Err(error);
            }
        };
        let root = self.root();
        self.history.begin(
            "Agente: Importar recurso",
            &self.state.project,
            &self.state.images,
        );
        match asset_import::import(&mut self.state.project, &root, scene, request, &source) {
            Ok(imported) => {
                if let Some(image) = source.image {
                    self.state.images.insert(imported.asset.clone(), image);
                }
                if let Err(error) = self
                    .history
                    .commit(&self.state.project, &mut self.state.images)
                {
                    self.log(error);
                }
                self.log(format!(
                    "{AGENT_PREFIX}Recurso importado como cópia local: {}",
                    imported.name
                ));
                self.after_external_edit();
                if imported.kind == ImportKind::Texture {
                    self.refresh_texture(&imported.asset);
                }
                serde_json::to_value(&imported).map_err(|e| e.to_string())
            }
            Err(error) => {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                self.log(format!("{AGENT_PREFIX}{error}"));
                Err(error)
            }
        }
    }
    /// Keeps the active scene, selection and textures consistent after an edit that did not
    /// come from the panels.
    fn after_external_edit(&mut self) {
        if self.state.project.scene(&self.scene_id).is_none() {
            let start = self.state.project.start_scene.clone();
            self.set_scene(start);
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.scene().entity(id).is_none())
        {
            self.select(None);
        }
        let scene = self.scene().clone();
        self.selection.retain_scene(&scene);
        self.state
            .images
            .configure(&self.root(), &self.state.project);
    }
    /// Undo or redo on an agent's request, with the same rules as Ctrl+Z.
    pub(crate) fn agent_undo(&mut self, redo: bool) -> Result<Option<String>, String> {
        if let Some(reason) = self.agent_busy() {
            return Err(reason.into());
        }
        let label = if redo {
            self.history.redo_label()
        } else {
            self.history.undo_label()
        }
        .map(str::to_owned);
        if label.is_none() {
            return Ok(None);
        }
        self.undo(redo);
        self.log(format!(
            "{AGENT_PREFIX}{} {}",
            if redo { "Refazer" } else { "Desfazer" },
            label.as_deref().unwrap_or_default()
        ));
        Ok(label)
    }
}

impl Editor {
    /// Status bar mark: agents allowed, or an agent connected right now.
    pub(crate) fn agent_indicator(&self, ui: &mut egui::Ui) {
        if !self.preferences.allow_agents {
            return;
        }
        let connected = self.agent_connected();
        let color = if connected {
            Color32::from_rgb(190, 160, 240)
        } else {
            crate::theme::TEXT_MUTED
        };
        let text = if connected {
            "Agente conectado"
        } else {
            "Agentes ligados"
        };
        ui.add_space(4.);
        ui.label(egui::RichText::new(text).size(12.).color(color))
            .on_hover_text(if connected {
                "Um agente de IA está conectado pelo servidor MCP. As mudanças dele aparecem                  no Console e podem ser desfeitas com Ctrl+Z."
            } else {
                "Agentes de IA podem se conectar pelo servidor MCP. Desligue em Preferências do                  editor."
            });
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.5, color);
    }
}

#[cfg(test)]
impl Editor {
    pub(crate) fn qa_allow_agents(&mut self, on: bool) {
        self.preferences.allow_agents = on;
    }
    pub(crate) fn qa_undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }
}

#[cfg(test)]
mod tests {
    use super::AGENT_PREFIX;
    #[test]
    fn agent_messages_carry_the_console_origin() {
        assert!(format!("{AGENT_PREFIX}Criar objeto").starts_with("Agente · "));
    }
}
