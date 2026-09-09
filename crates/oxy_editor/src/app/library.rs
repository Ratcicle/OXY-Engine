use super::*;

impl Editor {
    pub(super) fn library(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("library")
            .resizable(true)
            .default_height(if self.state.project.assets.is_empty() {
                (ctx.content_rect().height() * 0.14).clamp(50., 100.)
            } else {
                145.
            })
            .height_range(45.0..=(ctx.content_rect().height() * 0.4).max(50.))
            .show(ctx, |ui| {
                if self.mesh_operation_active() {
                    ui.disable();
                }
                ui.horizontal_wrapped(|ui| {
                    ui.strong("BIBLIOTECA DO PROJETO");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.asset_search)
                            .hint_text("Filtrar recursos…")
                            .desired_width(170.),
                    );
                    if ui.button("Importar PNG").clicked() {
                        self.import(AssetKind::Texture);
                    }
                    if ui.button("Importar WAV").clicked() {
                        self.import(AssetKind::Audio);
                    }
                });
                let assets = self.state.project.assets.clone();
                let asset_filter = self.asset_search.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for asset in assets
                            .iter()
                            .filter(|a| a.name.to_lowercase().contains(&asset_filter))
                        {
                            let card = ui.group(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(200.);
                                    ui.strong(&asset.name);
                                    ui.small(match asset.kind {
                                        AssetKind::Texture => "PNG · textura",
                                        AssetKind::Audio => "WAV · áudio",
                                        AssetKind::Model => "Modelo · peças editáveis",
                                    });
                                    match asset.kind {
                                        AssetKind::Model => {
                                            if ui.button("Colocar na cena").clicked() {
                                                let scene_id = self.scene_id.clone();
                                                match self
                                                    .state
                                                    .project
                                                    .instantiate_model(&asset.id, &scene_id)
                                                {
                                                    Ok(id) => self.select(Some(id)),
                                                    Err(e) => self.log(e),
                                                }
                                            }
                                        }
                                        AssetKind::Texture => {
                                            if ui.button("Criar sprite / aplicar").clicked() {
                                                self.ensure_texture(&asset.id);
                                                if self.selected.is_none() {
                                                    self.add_entity(
                                                        Some(Primitive::Sprite),
                                                        &asset.name,
                                                    );
                                                }
                                                if let Some(id) = self.selected.clone() {
                                                    if let Some(e) =
                                                        self.scene_mut().entity_mut(&id)
                                                    {
                                                        e.material.texture = Some(asset.id.clone());
                                                    }
                                                    if let Some(p) =
                                                        self.state.images.get(&asset.id)
                                                    {
                                                        let ratio =
                                                            p.width as f32 / p.height as f32;
                                                        if let Some(e) =
                                                            self.scene_mut().entity_mut(&id)
                                                            && e.primitive
                                                                == Some(Primitive::Sprite)
                                                        {
                                                            e.dimensions[0] =
                                                                e.dimensions[1] * ratio;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        AssetKind::Audio => {
                                            if ui.button("Ouvir").clicked()
                                                && let Err(e) = oxy_core::audio::play_wav(
                                                    &self.root().join(&asset.path),
                                                    0.6,
                                                )
                                            {
                                                self.log(e);
                                                self.notice_last(false);
                                            }
                                        }
                                    }
                                    if ui.small_button("Excluir recurso do projeto").clicked() {
                                        self.delete_asset = Some(asset.id.clone());
                                    }
                                });
                            });
                            if self.locate_asset.as_ref() == Some(&asset.id) {
                                card.response.scroll_to_me(Some(egui::Align::Center));
                                ui.painter().rect_stroke(
                                    card.response.rect,
                                    3.,
                                    egui::Stroke::new(2., Color32::GOLD),
                                    egui::StrokeKind::Inside,
                                );
                                self.locate_asset = None;
                            }
                        }
                    });
                });
            });
    }
}
