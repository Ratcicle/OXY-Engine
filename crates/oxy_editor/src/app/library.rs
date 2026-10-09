use super::*;
use crate::icons::{self, Icon};
use crate::theme;
use egui::RichText;

const TILE_WIDTH: f32 = 184.;

impl Editor {
    /// Asset cards shown in the dock's Biblioteca tab.
    pub(super) fn library_body(&mut self, ui: &mut egui::Ui) {
        let filter = self.asset_ui.search.to_lowercase();
        let assets: Vec<_> = self
            .state
            .project
            .assets
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&filter))
            .cloned()
            .collect();
        if assets.is_empty() {
            crate::widgets::hint(
                ui,
                if self.state.project.assets.is_empty() {
                    "Nenhum recurso. Importe PNG ou WAV, ou salve uma hierarquia como modelo."
                } else {
                    "Nenhum recurso com esse nome."
                },
            );
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8., 8.);
                for asset in &assets {
                    self.asset_tile(ui, asset);
                }
            });
        });
    }
    fn asset_tile(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        let (icon, kind) = match asset.kind {
            AssetKind::Texture => (Icon::Sprite, "Textura PNG"),
            AssetKind::Audio => (Icon::Sound, "Áudio WAV"),
            AssetKind::Model => (Icon::Cube, "Modelo · peças editáveis"),
        };
        let located = self.asset_ui.locate.as_ref() == Some(&asset.id);
        let card = egui::Frame::new()
            .fill(theme::CARD_FILL)
            .stroke(egui::Stroke::new(
                if located { 2. } else { 1. },
                if located {
                    theme::HIGHLIGHT
                } else {
                    theme::CARD_BORDER
                },
            ))
            .corner_radius(theme::CARD_RADIUS)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(TILE_WIDTH);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.;
                    ui.horizontal(|ui| {
                        let (thumb, _) = ui.allocate_exact_size(Vec2::splat(32.), Sense::hover());
                        ui.painter().rect_filled(thumb, 5., theme::BG_BASE);
                        icons::paint(
                            ui,
                            Rect::from_center_size(thumb.center(), Vec2::splat(18.)),
                            icon,
                            theme::TEXT_MUTED,
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.;
                            ui.add(
                                egui::Label::new(RichText::new(&asset.name).strong()).truncate(),
                            );
                            ui.label(RichText::new(kind).size(11.).color(theme::TEXT_MUTED));
                        });
                    });
                    ui.horizontal(|ui| {
                        self.asset_primary_action(ui, asset);
                        // Deleting asks for confirmation in its own dialog.
                        let delete = ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Excluir").size(12.).color(theme::TEXT_MUTED),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Excluir recurso do projeto");
                        icons::register_qa(ui, &format!("Excluir {}", asset.name), delete.rect);
                        if delete.clicked() {
                            self.asset_ui.delete = Some(asset.id.clone());
                        }
                    });
                });
            });
        if located {
            card.response.scroll_to_me(Some(egui::Align::Center));
            self.asset_ui.locate = None;
        }
    }
    fn asset_primary_action(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        match asset.kind {
            AssetKind::Model => {
                if ui.button("Colocar na cena").clicked() {
                    let scene_id = self.scene_id.clone();
                    match self.state.project.instantiate_model(&asset.id, &scene_id) {
                        Ok(id) => {
                            if self.tab == Tab::Studio {
                                self.studio.focus = Some(id.clone());
                            }
                            self.select(Some(id));
                        }
                        Err(e) => self.log(e),
                    }
                }
            }
            AssetKind::Texture => {
                if ui
                    .button("Aplicar")
                    .on_hover_text("Aplica na peça selecionada. Sem seleção, cria um sprite.")
                    .clicked()
                {
                    self.apply_texture_asset(asset);
                }
            }
            AssetKind::Audio => {
                if ui.button("Ouvir").clicked()
                    && let Err(e) = oxy_core::audio::play_wav(&self.root().join(&asset.path), 0.6)
                {
                    self.log(e);
                    self.notice_last(false);
                }
            }
        }
    }
    fn apply_texture_asset(&mut self, asset: &Asset) {
        self.ensure_texture(&asset.id);
        if self.selected.is_none() {
            self.add_entity(Some(Primitive::Sprite), &asset.name);
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        if let Some(e) = self.scene_mut().entity_mut(&id) {
            e.material.texture = Some(asset.id.clone());
        }
        if let Some(p) = self.state.images.get(&asset.id) {
            let ratio = p.width as f32 / p.height as f32;
            if let Some(e) = self.scene_mut().entity_mut(&id)
                && e.primitive == Some(Primitive::Sprite)
            {
                e.dimensions[0] = e.dimensions[1] * ratio;
            }
        }
    }
    /// Bottom dock: Biblioteca, Console and Avisos tabs. Collapses to its tab row.
    pub(super) fn dock(&mut self, ctx: &egui::Context) {
        if self.asset_ui.locate.is_some() {
            self.show_dock(DockTab::Library);
        }
        let open = self.dock.open;
        let panel = egui::TopBottomPanel::bottom("dock").frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 0)),
        );
        let panel = if open {
            panel
                .resizable(true)
                .default_height(176.)
                .height_range(110.0..=(ctx.content_rect().height() * 0.45).max(120.))
        } else {
            panel
                .resizable(false)
                .exact_height(theme::PANEL_HEADER_HEIGHT)
        };
        panel.show(ctx, |ui| {
            if self.mesh_operation_active() {
                ui.disable();
            }
            ui.horizontal(|ui| {
                ui.set_height(theme::PANEL_HEADER_HEIGHT);
                let items = [
                    (
                        DockTab::Library,
                        "Biblioteca",
                        Some(self.state.project.assets.len()),
                    ),
                    (DockTab::Console, "Console", None),
                    (DockTab::Notices, "Avisos", Some(self.notices.unread())),
                ];
                if let Some(tab) = crate::widgets::tabs(ui, open.then_some(self.dock.tab), &items) {
                    if open && self.dock.tab == tab {
                        self.dock.open = false;
                    } else {
                        self.show_dock(tab);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, label) = if open {
                        (Icon::ChevronDown, "Recolher painel")
                    } else {
                        (Icon::ChevronRight, "Expandir painel")
                    };
                    if icons::small(ui, icon, label, 26.).clicked() {
                        self.dock.open = !open;
                    }
                    if !open {
                        return;
                    }
                    match self.dock.tab {
                        DockTab::Library => {
                            ui.menu_button("Importar", |ui| {
                                if ui.button("Importar PNG…").clicked() {
                                    self.import(AssetKind::Texture);
                                    ui.close();
                                }
                                if ui.button("Importar WAV…").clicked() {
                                    self.import(AssetKind::Audio);
                                    ui.close();
                                }
                            });
                            // Narrow docks drop the filter instead of overlapping the tabs.
                            let width = (ui.available_width() - 12.).min(170.);
                            if width >= 80. {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.asset_ui.search)
                                        .hint_text("Filtrar recursos…")
                                        .desired_width(width),
                                );
                            }
                        }
                        DockTab::Console => {
                            if ui.button("Limpar").clicked() {
                                self.clear_console();
                            }
                        }
                        DockTab::Notices => {}
                    }
                });
            });
            if open {
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1., theme::BORDER),
                );
                ui.add_space(8.);
                match self.dock.tab {
                    DockTab::Library => self.library_body(ui),
                    DockTab::Console => self.console_body(ui),
                    DockTab::Notices => self.notices_body(ui),
                }
            }
        });
    }
}
