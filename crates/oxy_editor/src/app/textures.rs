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
                // The Library opens the folder of the image and selects its row.
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
}
