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
        let name = format!("{} (cópia)", source.name);
        self.state.project.assets.push(Asset {
            id: copy.clone(),
            name,
            kind: AssetKind::Texture,
            path: format!("assets/{copy}.png"),
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
    pub(super) fn asset_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.asset_ui.delete.clone() else {
            return;
        };
        let Some(asset) = self.state.project.asset(&id).cloned() else {
            self.asset_ui.delete = None;
            return;
        };
        let references = editing::asset_references(&self.state.project, &id);
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
                widgets::hint(
                    ui,
                    "O recurso será retirado da biblioteca. O arquivo original no disco é \
                     preservado para permitir desfazer com segurança.",
                );
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
            match editing::remove_asset(&mut self.state.project, &id) {
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
