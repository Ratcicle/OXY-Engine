use super::*;

pub(super) enum SceneDialog {
    Create { name: String, kind: SceneKind },
    Rename { id: Id, name: String },
    Delete(Id),
}
pub(super) struct NewProject {
    pub name: String,
    pub kind: SceneKind,
}
impl Default for NewProject {
    fn default() -> Self {
        Self {
            name: "Meu projeto OXY".into(),
            kind: SceneKind::TwoD,
        }
    }
}
pub(super) fn kind_label(kind: SceneKind) -> &'static str {
    if kind == SceneKind::TwoD { "2D" } else { "3D" }
}

impl Editor {
    pub(super) fn new_project_dialog(&mut self) {
        self.pause();
        self.new_project = Some(NewProject::default());
    }
    pub(super) fn scene_bar(&mut self, ui: &mut egui::Ui) {
        let compact = ui.ctx().content_rect().width() < 700.;
        if !compact {
            ui.label("Cena:");
        }
        let mut scene_id = self.scene_id.clone();
        egui::ComboBox::from_id_salt("scene")
            .width(if compact { 105. } else { 150. })
            .selected_text(format!(
                "{} · {}",
                self.scene().name,
                kind_label(self.scene().kind)
            ))
            .show_ui(ui, |ui| {
                for scene in &self.state.project.scenes {
                    let start = scene.id == self.state.project.start_scene;
                    ui.selectable_value(
                        &mut scene_id,
                        scene.id.clone(),
                        format!(
                            "{}{} · {}",
                            if start { "• " } else { "" },
                            scene.name,
                            kind_label(scene.kind)
                        ),
                    )
                    .on_hover_text(if start {
                        "Cena inicial aberta pelo jogo independente."
                    } else {
                        "Abrir esta cena para editar."
                    });
                }
            });
        if scene_id != self.scene_id {
            self.set_scene(scene_id);
        }
        if ui.button("+").on_hover_text("Criar cena").clicked() {
            self.scene_dialog = Some(SceneDialog::Create {
                name: "Nova cena".into(),
                kind: self.scene().kind,
            });
        }
        ui.menu_button("...", |ui| {
            if ui.button("Renomear cena").clicked() {
                self.scene_dialog = Some(SceneDialog::Rename {
                    id: self.scene_id.clone(),
                    name: self.scene().name.clone(),
                });
                ui.close();
            }
            if ui.button("Duplicar cena").clicked() {
                if self.structural_ready() {
                    match editing::duplicate_scene(&mut self.state.project, &self.scene_id) {
                        Ok(id) => self.set_scene(id),
                        Err(e) => self.warn(e),
                    }
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    self.state.project.scenes.len() > 1,
                    egui::Button::new("Excluir cena"),
                )
                .on_disabled_hover_text("O projeto precisa manter pelo menos uma cena.")
                .clicked()
            {
                self.scene_dialog = Some(SceneDialog::Delete(self.scene_id.clone()));
                ui.close();
            }
            if ui.button("Usar como cena inicial do jogo").clicked() {
                self.state.project.start_scene = self.scene_id.clone();
                ui.close();
            }
        });
    }
    pub(super) fn project_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut new) = self.new_project.take() {
            let mut create = false;
            let mut cancel = false;
            let response = egui::Modal::new("new_project".into()).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.).clamp(160., 420.));
                ui.heading("Novo projeto");
                ui.label("Nome do projeto");
                ui.text_edit_singleline(&mut new.name);
                for (kind, title, description) in [
                    (
                        SceneKind::TwoD,
                        "Cena 2D",
                        "Comece com imagens, formas e uma câmera 2D.",
                    ),
                    (
                        SceneKind::ThreeD,
                        "Cena 3D",
                        "Comece com modelos, profundidade e uma câmera 3D.",
                    ),
                ] {
                    ui.horizontal(|ui| {
                        let (rect, icon) = ui.allocate_exact_size(Vec2::splat(28.), Sense::click());
                        let p = |x, y| rect.min + Vec2::new(x, y);
                        let stroke = egui::Stroke::new(1.5, ui.visuals().text_color());
                        if kind == SceneKind::TwoD {
                            ui.painter().rect_stroke(
                                Rect::from_min_max(p(3., 6.), p(25., 23.)),
                                1.,
                                stroke,
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().line_segment([p(4., 22.), p(14., 13.)], stroke);
                            ui.painter()
                                .line_segment([p(14., 13.), p(24., 22.)], stroke);
                        } else {
                            for (a, b) in [
                                ((3., 8.), (15., 3.)),
                                ((15., 3.), (26., 9.)),
                                ((26., 9.), (14., 14.)),
                                ((14., 14.), (3., 8.)),
                                ((3., 8.), (3., 22.)),
                                ((3., 22.), (14., 27.)),
                                ((14., 27.), (26., 22.)),
                                ((26., 22.), (26., 9.)),
                                ((14., 14.), (14., 27.)),
                            ] {
                                ui.painter()
                                    .line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
                            }
                        }
                        icon.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                new.kind == kind,
                                title,
                            )
                        });
                        let response = ui.add(
                            egui::Button::new(format!("{title}\n{description}"))
                                .selected(new.kind == kind)
                                .wrap(),
                        );
                        if response.clicked() || icon.clicked() {
                            new.kind = kind;
                        }
                    });
                }
                ui.weak("Você poderá adicionar cenas 2D e 3D depois.");
                ui.horizontal(|ui| {
                    create = ui
                        .add_enabled(
                            !new.name.trim().is_empty(),
                            egui::Button::new("Criar projeto"),
                        )
                        .clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
            cancel |= response.should_close();
            create |= response.is_top_modal
                && ctx.input(|i| i.key_pressed(egui::Key::Enter))
                && !new.name.trim().is_empty();
            if !cancel {
                if create {
                    self.transition(Transition::Create(new.name, new.kind));
                } else {
                    self.new_project = Some(new);
                }
            }
        }
        if let Some(mut dialog) = self.scene_dialog.take() {
            let mut confirm = false;
            let mut cancel = false;
            let response = egui::Modal::new("scene_dialog".into()).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.).clamp(160., 420.));
                match &mut dialog {
                    SceneDialog::Create { name, kind } => {
                        ui.heading("Nova cena");
                        ui.text_edit_singleline(name);
                        ui.horizontal(|ui| {
                            ui.selectable_value(kind, SceneKind::TwoD, "Cena 2D");
                            ui.selectable_value(kind, SceneKind::ThreeD, "Cena 3D");
                        });
                        confirm = ui
                            .add_enabled(!name.trim().is_empty(), egui::Button::new("Criar cena"))
                            .clicked();
                    }
                    SceneDialog::Rename { name, .. } => {
                        ui.heading("Renomear cena");
                        ui.text_edit_singleline(name);
                        confirm = ui
                            .add_enabled(
                                !name.trim().is_empty(),
                                egui::Button::new("Confirmar nome"),
                            )
                            .clicked();
                    }
                    SceneDialog::Delete(id) => {
                        ui.heading("Excluir cena");
                        let references = editing::scene_references(&self.state.project, id);
                        if id == &self.state.project.start_scene {
                            ui.label("Escolha outra cena inicial antes de excluir esta cena.");
                        } else if !references.is_empty() {
                            ui.label("Altere os destinos destes nós antes de excluir:");
                            egui::ScrollArea::vertical()
                                .max_height(180.)
                                .show(ui, |ui| {
                                    for reference in &references {
                                        ui.label(reference);
                                    }
                                });
                        } else {
                            ui.label("Excluir esta cena e seus objetos? Você poderá desfazer.");
                            confirm = ui.button("Excluir cena").clicked();
                        }
                    }
                }
                cancel = ui.button("Cancelar").clicked();
            });
            if cancel || response.should_close() {
                return;
            }
            if confirm {
                if !self.structural_ready() {
                    return;
                }
                match dialog {
                    SceneDialog::Create { name, kind } => {
                        let scene = Scene::new(name.trim(), kind);
                        let id = scene.id.clone();
                        self.state.project.scenes.push(scene);
                        self.set_scene(id);
                    }
                    SceneDialog::Rename { id, name } => {
                        if let Some(scene) = self.state.project.scene_mut(&id) {
                            scene.name = name.trim().into();
                        }
                    }
                    SceneDialog::Delete(id) => {
                        match editing::delete_scene(&mut self.state.project, &id) {
                            Ok(()) => self.set_scene(self.state.project.start_scene.clone()),
                            Err(e) => self.warn(e),
                        }
                    }
                }
            } else {
                self.scene_dialog = Some(dialog);
            }
        }
    }
}
