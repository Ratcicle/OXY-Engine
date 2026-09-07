use super::*;

impl Editor {
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
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
                ui.separator();
                ui.menu_button("Projeto", |ui| {
                    if ui.button("Novo projeto").clicked() {
                        self.transition(Transition::New);
                        ui.close();
                    }
                    if ui.button("Abrir projeto…").clicked() {
                        self.pause();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Projeto OXY", &["json"])
                            .pick_file()
                        {
                            self.transition(Transition::Open(path))
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
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new("Desfazer"))
                    .clicked()
                {
                    self.undo(false)
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new("Refazer"))
                    .clicked()
                {
                    self.undo(true)
                }
                ui.separator();
                if self.runtime.is_none() {
                    if ui.button("▶ Jogar").clicked() {
                        self.start();
                    }
                } else {
                    let paused = self.runtime.as_ref().is_some_and(|r| r.paused);
                    if ui
                        .button(if paused { "▶ Retomar" } else { "Ⅱ Pausar" })
                        .clicked()
                    {
                        if paused {
                            self.tab = Tab::Game;
                            self.capture = true;
                            if let Some(r) = &mut self.runtime {
                                r.set_paused(false);
                            }
                            self.last_time = Instant::now();
                        } else {
                            self.pause()
                        }
                    }
                    if ui.button("■ Parar").clicked() {
                        self.stop()
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.scale, 0.8..=1.6)
                            .text("Interface")
                            .show_value(false),
                    );
                    ui.label(if self.dirty() {
                        "● Não salvo"
                    } else if self.path.is_none() {
                        "Sem arquivo"
                    } else {
                        "Salvo"
                    });
                    if ui.available_width() > 60. {
                        ui.add_sized(
                            [ui.available_width(), 20.],
                            egui::Label::new(&self.state.project.name).truncate(),
                        );
                    }
                });
            });
            ui.horizontal(|ui| {
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
                    self.set_spatial_tool(Tool::Object);
                    self.spatial.fit = None;
                }
                ui.separator();
                let mut scene_id = self.scene_id.clone();
                egui::ComboBox::from_id_salt("scene")
                    .selected_text(&self.scene().name)
                    .show_ui(ui, |ui| {
                        for scene in &self.state.project.scenes {
                            ui.selectable_value(&mut scene_id, scene.id.clone(), &scene.name);
                        }
                    });
                if scene_id != self.scene_id {
                    self.set_scene(scene_id);
                }
                ui.menu_button("+ Cena", |ui| {
                    ui.selectable_value(&mut self.new_scene_kind, SceneKind::TwoD, "2D");
                    ui.selectable_value(&mut self.new_scene_kind, SceneKind::ThreeD, "3D");
                    if ui.button("Criar cena").clicked() {
                        let scene = Scene::new(
                            if self.new_scene_kind == SceneKind::TwoD {
                                "Nova cena 2D"
                            } else {
                                "Nova cena 3D"
                            },
                            self.new_scene_kind,
                        );
                        let id = scene.id.clone();
                        self.state.project.scenes.push(scene);
                        self.set_scene(id);
                        ui.close();
                    }
                });
                if ui
                    .button("Definir inicial")
                    .on_hover_text("Cena aberta pelo executável do jogo")
                    .clicked()
                {
                    self.state.project.start_scene = self.scene_id.clone();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.console, "Console");
                    ui.checkbox(&mut self.diagnostics, "Desempenho")
                        .on_hover_text(
                            "Veja tempos medidos, objetos, texturas, malhas e tarefas do jogo.",
                        );
                    if ui.available_width() > 150. {
                        ui.label(if self.scene().kind == SceneKind::TwoD {
                            "2D · metros · Y ↑"
                        } else {
                            "3D · metros · Y ↑"
                        });
                    }
                });
            });
        });
    }
}
