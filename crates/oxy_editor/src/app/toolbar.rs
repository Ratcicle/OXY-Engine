use super::*;

impl Editor {
    pub(crate) fn compact_layout(ctx: &egui::Context) -> bool {
        ctx.content_rect().width() < 800. || ctx.content_rect().height() < 490.
    }
    fn history_buttons(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(self.history.can_undo(), egui::Button::new("Desfazer"))
            .on_hover_text("Desfazer a última edição (Ctrl+Z).")
            .clicked()
        {
            self.undo(false);
        }
        if ui
            .add_enabled(self.history.can_redo(), egui::Button::new("Refazer"))
            .on_hover_text("Refazer a edição desfeita (Ctrl+Y).")
            .clicked()
        {
            self.undo(true);
        }
    }
    fn display_menu(&mut self, ui: &mut egui::Ui) {
        let response = ui.menu_button("Exibir", |ui| {
            ui.checkbox(&mut self.console, "Console");
            ui.checkbox(&mut self.diagnostics, "Desempenho");
            self.notices_button(ui);
            self.camera_display_menu(ui);
        });
        if self.notices.has_unread() {
            ui.painter().circle_filled(
                response.response.rect.right_top() + Vec2::new(-3., 3.),
                3.,
                Color32::LIGHT_YELLOW,
            );
        }
    }
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            if self.mesh_operation_active() {
                ui.disable();
            }
            ui.horizontal_wrapped(|ui| {
                if ctx.content_rect().width() < 700. {
                    ui.label(
                        egui::RichText::new(concat!("OXY\n", env!("CARGO_PKG_VERSION")))
                            .strong()
                            .color(Color32::from_rgb(131, 224, 207)),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("OXY")
                            .size(26.)
                            .strong()
                            .color(Color32::from_rgb(131, 224, 207)),
                    );
                    ui.label(
                        egui::RichText::new(concat!("ENGINE\n", env!("CARGO_PKG_VERSION")))
                            .size(12.)
                            .strong(),
                    );
                }
                ui.separator();
                ui.menu_button("Projeto", |ui| {
                    if self.capture {self.pause();oxy_render::input::release_cursor(ctx);}
                    if ui.button("Tela inicial").clicked() {
                        self.pause();
                        self.transition(Transition::Home);
                        ui.close();
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
                    if ui.button("Importar PNG…").clicked() {
                        self.import(AssetKind::Texture);
                        ui.close();
                    }
                    if ui.button("Importar WAV…").clicked() {
                        self.import(AssetKind::Audio);
                        ui.close();
                    }
                });
                if ctx.content_rect().width() < 700. {
                    ui.menu_button("Editar", |ui| self.history_buttons(ui));
                } else {
                    self.history_buttons(ui);
                }
                ui.separator();
                if self.runtime.is_none() {
                    if ui.button("▶ Jogar").clicked() {
                        self.start();
                    }
                } else {
                    let paused = self.runtime.as_ref().is_some_and(|r| r.paused);
                    if ui
                        .button(if paused { "▶ Retomar · Pausado" } else { "Ⅱ Pausar · Em execução" })
                        .on_hover_text("Escape pausa e libera a entrada. Edições não alteram o teste iniciado; use Parar e Jogar novamente.")
                        .clicked()
                    {
                        if paused {
                            self.tab = Tab::Game;
                            self.capture = true;
                            if let Some(rt) = &mut self.runtime {
                                rt.set_paused(false);
                            }
                            self.last_time = Instant::now();
                        } else {
                            self.pause();
                        }
                    }
                    if ui.button("■ Parar").clicked() {
                        self.stop();
                    }
                    self.visualization_button(ui, true);
                }
                self.display_menu(ui);
                self.interface_button(ui);
                ui.menu_button("Ajuda", |ui| {
                    if ui.button("Guia de lógica visual").clicked() {
                        self.open_guide("");
                        ui.close();
                    }
                });
                let dirty = self.dirty();
                let status = if dirty {
                    "● Não salvo"
                } else if self.path.is_none() {
                    "Sem arquivo"
                } else {
                    "Salvo"
                };
                if ctx.content_rect().width() < 700. {
                    // A fixed-size status never adds a toolbar row when the first edit occurs.
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(14.), egui::Sense::hover());
                    if dirty {
                        ui.painter().circle_filled(rect.center(), 4., Color32::GOLD);
                    } else {
                        ui.painter().circle_stroke(
                            rect.center(),
                            4.,
                            egui::Stroke::new(1.5, Color32::from_rgb(131, 224, 207)),
                        );
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), status)
                    });
                    response.on_hover_text(format!("{status} · Salvar: Ctrl+S"));
                } else {
                    ui.label(status);
                }
            });
            ui.horizontal_wrapped(|ui| {
                let old = self.tab;
                for (tab, label) in [
                    (Tab::Scene, "Cena"),
                    (Tab::Game, "Jogo"),
                    (Tab::Studio, "Estúdio"),
                    (Tab::Logic, "Lógica"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
                if old == Tab::Game && self.tab != Tab::Game {
                    self.pause();
                }
                if old != self.tab {
                    self.cancel_camera_drag();
                    self.set_spatial_tool(Tool::Object);
                    self.spatial.fit = None;
                }
                ui.separator();
                self.scene_bar(ui);
                if Self::compact_layout(ctx) && self.tab != Tab::Game {
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
                    .on_hover_text(
                        "Em janelas pequenas, alterne os painéis para preservar espaço de criação.",
                    );
                }
                if ctx.content_rect().width() >= 900. {
                    ui.checkbox(&mut self.console, "Console");
                    self.notices_button(ui);
                    ui.checkbox(&mut self.diagnostics, "Desempenho")
                        .on_hover_text(
                            "Tempos medidos, objetos, texturas, malhas e tarefas do jogo.",
                        );
                }
            });
        });
    }
}
