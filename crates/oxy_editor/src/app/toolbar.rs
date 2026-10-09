//! Shell bars: the single top bar (project, modes, play test) and the status bar.
use super::*;
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets;
use egui::{Align, Layout, RichText, UiBuilder};

const MODES: [(Tab, &str); 3] = [
    (Tab::Scene, "Cena"),
    (Tab::Studio, "Estúdio"),
    (Tab::Logic, "Lógica"),
];

impl Editor {
    pub(crate) fn compact_layout(ctx: &egui::Context) -> bool {
        ctx.content_rect().width() < 800. || ctx.content_rect().height() < 490.
    }
    /// Changes the work mode. Logic pauses a running test; the Studio waits for Stop.
    pub(crate) fn switch_tab(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        if self.playing() && tab == Tab::Studio {
            self.warn("Pare o teste para editar no Estúdio.");
            return;
        }
        if self.playing() && tab == Tab::Logic {
            self.pause();
        }
        self.tab = tab;
        self.cancel_camera_drag();
        self.set_spatial_tool(Tool::Object);
        self.spatial.fit = None;
    }
    fn mode_switch(&mut self, ui: &mut egui::Ui) {
        let mut tab = self.tab;
        if widgets::segmented(ui, &mut tab, &MODES) {
            self.switch_tab(tab);
        }
    }
    fn mode_switch_width(ui: &egui::Ui) -> f32 {
        let padding = ui.spacing().button_padding.x * 2.;
        let text: f32 = MODES
            .iter()
            .map(|(_, label)| {
                ui.painter()
                    .layout_no_wrap(
                        (*label).into(),
                        egui::TextStyle::Button.resolve(ui.style()),
                        Color32::WHITE,
                    )
                    .size()
                    .x
                    + padding
            })
            .sum();
        text + 2. * (MODES.len() as f32 - 1.) + 10.
    }
    fn project_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("Projeto", |ui| {
            if self.play.capture {
                self.pause();
                oxy_render::input::release_cursor(ctx);
            }
            if ui.button("Novo projeto").clicked() {
                self.new_project_dialog();
                ui.close();
            }
            if ui.button("Abrir projeto…").clicked() {
                self.pause();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Projeto OXY", &["json"])
                    .pick_file()
                {
                    self.transition(Transition::Open(path));
                }
                ui.close();
            }
            if ui.button("Salvar   Ctrl+S").clicked() {
                self.save_requested = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Importar PNG…").clicked() {
                self.import(AssetKind::Texture);
                ui.close();
            }
            if ui.button("Importar WAV…").clicked() {
                self.import(AssetKind::Audio);
                ui.close();
            }
            ui.separator();
            if ui.button("Tela inicial").clicked() {
                self.pause();
                self.transition(Transition::Home);
                ui.close();
            }
        });
    }
    fn save_status(&self) -> (&'static str, Color32) {
        if self.dirty() {
            ("Não salvo", theme::WARNING)
        } else if self.path.is_none() {
            ("Sem arquivo", theme::TEXT_MUTED)
        } else {
            ("Salvo", theme::SUCCESS)
        }
    }
    fn toolbar_left(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, compact: bool) {
        ui.add_space(4.);
        let (mark, _) = ui.allocate_exact_size(Vec2::splat(20.), Sense::hover());
        ui.painter().circle_stroke(
            mark.center(),
            7.,
            egui::Stroke::new(2.2, theme::ACCENT_BRIGHT),
        );
        ui.painter()
            .circle_filled(mark.center(), 2.4, theme::ACCENT_BRIGHT);
        ui.label(RichText::new("OXY").strong().size(14.));
        ui.add_space(6.);
        self.project_menu(ui, ctx);
        if !compact {
            ui.add(
                egui::Label::new(RichText::new(&self.state.project.name).color(theme::TEXT_MUTED))
                    .truncate(),
            );
        }
        ui.add_space(4.);
        ui.add_enabled_ui(self.history.can_undo(), |ui| {
            if icons::small(ui, Icon::Undo, "Desfazer", 28.).clicked() {
                self.undo(false);
            }
        });
        ui.add_enabled_ui(self.history.can_redo(), |ui| {
            if icons::small(ui, Icon::Redo, "Refazer", 28.).clicked() {
                self.undo(true);
            }
        });
        let (status, color) = self.save_status();
        let (dot, response) = ui.allocate_exact_size(Vec2::splat(14.), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.5, color);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), status)
        });
        if compact {
            response.on_hover_text(format!("{status} · Salvar: Ctrl+S"));
        } else {
            ui.label(RichText::new(status).size(12.).color(color))
                .on_hover_text("Salvar: Ctrl+S");
        }
        if compact {
            ui.add_space(6.);
            self.mode_switch(ui);
        }
    }
    fn toolbar_right(&mut self, ui: &mut egui::Ui, compact: bool) {
        ui.add_space(4.);
        icons::menu_button(
            ui,
            Icon::Help,
            "Ajuda",
            "Ajuda e guia de lógica visual (F1)",
            false,
            |ui| {
                if ui.button("Guia de lógica visual").clicked() {
                    self.open_guide("");
                    ui.close();
                }
            },
        );
        ui.add_space(4.);
        let Some(paused) = self.play.runtime.as_ref().map(|r| r.paused) else {
            // No tooltip: after Stop the pointer rests here, and an idle editor must not repaint.
            if widgets::icon_text_button(ui, Icon::Play, "Jogar", true, false).clicked() {
                self.start();
            }
            return;
        };
        if widgets::icon_text_button(ui, Icon::Stop, "Parar", false, compact).clicked() {
            self.stop();
        }
        let (icon, label) = if paused {
            (Icon::Play, "Retomar")
        } else {
            (Icon::Pause, "Pausar")
        };
        if widgets::icon_text_button(ui, icon, label, false, compact).clicked() {
            if paused {
                self.tab = if self.tab == Tab::Logic {
                    Tab::Scene
                } else {
                    self.tab
                };
                self.play.capture = true;
                if let Some(rt) = &mut self.play.runtime {
                    rt.set_paused(false);
                }
                self.frame.last_frame = Instant::now();
            } else {
                self.pause();
            }
        }
        self.visualization_button(ui, true);
        if !compact {
            let text = if paused { "Pausado" } else { "Jogando" };
            egui::Frame::new()
                .fill(Color32::from_rgb(58, 46, 26))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).size(12.).color(theme::WARNING));
                });
        }
    }
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(theme::TOPBAR_HEIGHT)
            .show(ctx, |ui| {
                if self.mesh_operation_active() {
                    ui.disable();
                }
                let compact = Self::compact_layout(ctx);
                let rect = ui.max_rect();
                let mut left = ui.new_child(
                    UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                self.toolbar_left(&mut left, ctx, compact);
                let mut right = ui.new_child(
                    UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::right_to_left(Align::Center)),
                );
                self.toolbar_right(&mut right, compact);
                if !compact {
                    let width = Self::mode_switch_width(ui);
                    let center =
                        Rect::from_center_size(rect.center(), Vec2::new(width, rect.height()));
                    let mut middle = ui.new_child(
                        UiBuilder::new()
                            .max_rect(center)
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    self.mode_switch(&mut middle);
                }
            });
    }
    fn panels_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Painéis", |ui| {
            ui.label("Exibir um painel por vez");
            for (panel, label) in [
                (CompactPanel::Hierarchy, "Hierarquia"),
                (CompactPanel::Properties, "Propriedades"),
                (CompactPanel::Library, "Biblioteca"),
                (CompactPanel::None, "Ampliar viewport"),
            ] {
                if ui
                    .selectable_value(&mut self.compact_panel, panel, label)
                    .clicked()
                {
                    ui.close();
                }
            }
            ui.small("Na Animação, selecione as peças pela linha do tempo.");
        })
        .response
        .on_hover_text("Em janelas pequenas, alterne os painéis para preservar espaço de criação.");
    }
    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(theme::STATUS_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(30, 30, 32))
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ctx, |ui| {
                let muted = |text: String| RichText::new(text).size(12.).color(theme::TEXT_MUTED);
                ui.horizontal_centered(|ui| {
                    let scene = self.scene();
                    ui.label(muted(format!(
                        "{} · {} objetos",
                        scene.name,
                        scene.entities.len()
                    )));
                    if let Some(name) = self
                        .selected
                        .as_deref()
                        .and_then(|id| self.scene().entity(id))
                        .map(|e| e.name.clone())
                    {
                        ui.add_space(8.);
                        ui.add(egui::Label::new(muted(format!("Selecionado: {name}"))).truncate());
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(muted(
                            concat!("OXY Engine ", env!("OXY_APP_VERSION")).into(),
                        ));
                        self.interface_button(ui);
                        let performance = ui
                            .add(egui::Button::selectable(
                                self.frame.visible,
                                muted("Desempenho".into()),
                            ))
                            .on_hover_text(
                                "Tempos medidos, objetos, texturas, malhas e tarefas do jogo.",
                            );
                        if performance.clicked() {
                            self.frame.visible = !self.frame.visible;
                        }
                        let console = ui.add(egui::Button::selectable(
                            self.console_visible(),
                            muted("Console".into()),
                        ));
                        if console.clicked() {
                            if self.console_visible() {
                                self.dock.open = false;
                            } else {
                                self.show_dock(DockTab::Console);
                            }
                        }
                        if Self::compact_layout(ctx) {
                            self.panels_menu(ui);
                        }
                    });
                });
            });
    }
}
