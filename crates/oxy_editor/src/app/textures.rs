use super::*;

impl Editor {
    pub(crate) fn ensure_texture(&mut self, id: &str) -> bool {
        if let Err(error) = self.state.images.ensure(id) {
            if self.console.messages.last() != Some(&error) {
                self.log(error);
            }
            false
        } else {
            true
        }
    }
    pub(crate) fn texture_thumbnail(&mut self, ui: &mut egui::Ui, id: &str) {
        self.texture_thumbnail_sized(ui, id, 72.);
    }
    pub(crate) fn texture_thumbnail_sized(&mut self, ui: &mut egui::Ui, id: &str, size: f32) {
        if !self.ensure_texture(id) {
            ui.colored_label(crate::theme::ERROR, "Não foi possível abrir a textura.");
            return;
        }
        if self
            .asset_ui
            .thumbnail
            .as_ref()
            .is_none_or(|(key, _)| key != id)
            && let Some(image) = self.state.images.get(id)
        {
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width as usize, image.height as usize],
                &image.pixels,
            );
            let texture =
                ui.ctx()
                    .load_texture("Miniatura da textura", color, egui::TextureOptions::NEAREST);
            self.asset_ui.thumbnail = Some((id.into(), texture));
        }
        if let Some((_, texture)) = &self.asset_ui.thumbnail {
            ui.add(
                egui::Image::from_texture(texture)
                    .max_size(Vec2::splat(size))
                    .corner_radius(4),
            );
        }
    }
    pub(crate) fn copy_texture_asset(&mut self, id: &str) -> Result<Id, String> {
        if !self.ensure_texture(id) {
            return Err("Não foi possível copiar os pixels.".into());
        }
        let image = self.state.images.get(id).ok_or("Textura ausente")?.clone();
        let source = self.state.project.asset(id).ok_or("Recurso ausente")?;
        let copy = new_id();
        let root = self.path.as_deref().and_then(std::path::Path::parent);
        // Beside the original; its file is made at the next save.
        let folder = source
            .path
            .rsplit_once('/')
            .map_or("", |(folder, _)| folder);
        let (name, path) = persistence::new_asset_file_in(
            &self.state.project,
            root,
            &copy,
            &format!("{} (cópia)", source.name),
            AssetKind::Texture,
            folder,
        );
        self.state.project.assets.push(Asset {
            id: copy.clone(),
            name,
            kind: AssetKind::Texture,
            path,
            model: None,
        });
        self.state.images.insert(copy.clone(), image);
        self.refresh_texture(&copy);
        Ok(copy)
    }
    pub(crate) fn texture_controls(
        &mut self,
        ui: &mut egui::Ui,
        entity: &mut Entity,
        interface: bool,
    ) {
        let current = if interface {
            entity.ui.as_ref().and_then(|u| u.texture.clone())
        } else {
            entity.material.texture.clone()
        };
        let Some(id) = current else {
            return;
        };
        let name = self
            .state
            .project
            .asset(&id)
            .map(|a| a.name.clone())
            .unwrap_or("Textura ausente".into());
        ui.horizontal_wrapped(|ui| {
            self.texture_thumbnail(ui, &id);
            ui.label(name);
        });
        let mut next = Some(id.clone());
        ui.horizontal_wrapped(|ui| {
            if ui.button("Editar no Estúdio").on_hover_text("Abra os pixels desta textura para pintar. Alterar uma textura compartilhada afeta seus vínculos.").clicked() {
                self.set_tab(Tab::Studio);self.studio.tab=StudioTab::Paint;
            }
            if ui.button("Substituir").on_hover_text("Importe uma cópia de outro PNG e aplique somente a este objeto.").clicked()
                && let Some(updated)=self.import_selected(AssetKind::Texture) {
                    next=Some(updated);
                    if !interface && let Some(updated)=self.scene().entity(&entity.id) { entity.dimensions=updated.dimensions; }
            }
            if ui.button("Remover textura").on_hover_text("Remove somente o vínculo deste objeto. O recurso continua na biblioteca.").clicked() { next=None; }
            if editing::asset_references(&self.state.project,&id).len()>1 && ui.button("Criar cópia independente").clicked() {
                match self.copy_texture_asset(&id) { Ok(copy)=>next=Some(copy),Err(e)=>self.log(e) }
            }
            if ui.button("Localizar na biblioteca").clicked() {
                // Filtering by name keeps the card in view without depending on scrolling.
                self.asset_ui.search=self.state.project.asset(&id).map(|a|a.name.clone()).unwrap_or_default();
                self.asset_ui.locate=Some(id.clone()); self.compact_panel=CompactPanel::Library;
                self.show_dock(DockTab::Library);
            }
        });
        if interface {
            if let Some(element) = &mut entity.ui {
                element.texture = next;
            }
        } else {
            entity.material.texture = next;
        }
    }
    /// Deletes the asset `id` of the project in folders at `root` as its own step: the file goes
    /// to the project's trash with it (see `command::apply_on_files`), and undoing brings both
    /// back.
    fn delete_asset_file(&mut self, root: &Path, id: &str) -> Result<(), String> {
        use oxy_core::command::{self, Command};
        self.finish_history(true);
        // What changed outside OXY is told before the walk takes the deletion.
        self.scan_project();
        self.history
            .begin("Excluir recurso", &self.state.project, &self.state.images);
        let delete = [Command::DeleteAsset {
            asset: id.to_owned(),
            kind: None,
            detach: None,
        }];
        match command::apply_on_files(
            &mut self.state.project,
            &self.scene_id,
            &delete,
            root,
            &mut self.known_files,
            "Excluir recurso",
        ) {
            Ok(done) => {
                if let Err(error) = self.history.commit_with_files(
                    &self.state.project,
                    &mut self.state.images,
                    done.files,
                ) {
                    self.log(error);
                }
                self.rebase_scan();
                // A sound or model a script names goes all the same: the Console says where.
                let warnings = done.report["warnings"].as_array().cloned();
                for warning in warnings.iter().flatten().filter_map(|w| w.as_str()) {
                    self.warn(oxy_core::script_files::without_command(warning));
                }
                Ok(())
            }
            Err(command::FilesError { message, kept }) => {
                self.keep_files_or_cancel(kept);
                Err(message)
            }
        }
    }
    pub(super) fn asset_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.asset_ui.delete.clone() else {
            return;
        };
        let Some(asset) = self.state.project.asset(&id).cloned() else {
            self.asset_ui.delete = None;
            return;
        };
        let references = editing::asset_references(&self.state.project, &id);
        // In a project in folders the file goes to the project's trash at once.
        let removed = if self.state.project.layout == Layout::Tree {
            "O recurso será retirado da biblioteca, e o arquivo vai para a lixeira do projeto \
             (.oxy/lixeira). Ctrl+Z traz os dois de volta."
        } else {
            "O recurso será retirado da biblioteca. O arquivo original no disco é preservado \
             para permitir desfazer com segurança."
        };
        use crate::widgets;
        let mut close = false;
        let mut delete = false;
        let response = egui::Modal::new("asset_delete".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 460.));
            let title = if references.is_empty() {
                format!("Excluir “{}”?", asset.name)
            } else {
                format!("Não é possível excluir “{}”", asset.name)
            };
            close = widgets::dialog_header(ui, &title);
            if references.is_empty() {
                widgets::hint(ui, removed);
            } else {
                widgets::hint(
                    ui,
                    "Este recurso ainda está em uso. Remova os vínculos antes de excluí-lo.",
                );
                ui.add_space(4.);
                egui::ScrollArea::vertical()
                    .max_height(240.)
                    .show(ui, |ui| {
                        for reference in &references {
                            // "Scope → Object: use", as listed by editing::asset_references.
                            let (place, usage) = reference
                                .split_once(": ")
                                .unwrap_or((reference.as_str(), ""));
                            let (scope, object) = place.split_once(" → ").unwrap_or(("", place));
                            let detail = [scope, usage]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(" · ");
                            widgets::reference_row(ui, crate::icons::Icon::Object, object, &detail);
                        }
                    });
            }
            widgets::dialog_footer(ui, |ui| {
                if references.is_empty() {
                    delete = widgets::danger_button(ui, "Excluir recurso", true).clicked();
                }
                close |= widgets::secondary_button(ui, "Cancelar").clicked();
            });
        });
        if delete {
            let deleted = match self.project_root() {
                // The test keeps the paths it started with.
                Some(_) if self.state.project.layout == Layout::Tree && self.playing() => Err(
                    "Pare o teste para excluir: o arquivo do recurso vai para a lixeira do \
                     projeto."
                        .into(),
                ),
                Some(root) if self.state.project.layout == Layout::Tree => {
                    self.delete_asset_file(&root, &id)
                }
                _ => editing::remove_asset(&mut self.state.project, &id),
            };
            match deleted {
                Ok(()) => {
                    self.state.images.remove(&id);
                    self.renderer.clear_texture_override(&id);
                    self.play.game_ui.clear_texture_override(&id);
                    self.asset_ui.thumbnail = None;
                    self.asset_ui.delete = None;
                }
                Err(error) => self.log(error),
            }
        } else if close || response.should_close() {
            self.asset_ui.delete = None;
        }
    }
}
