use super::*;

impl Editor {
    pub(super) fn creation_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("+ Objeto", |ui| {
            let primitives: Vec<_> = if self.scene().kind == SceneKind::TwoD {
                vec![
                    (Primitive::Rectangle, "Retângulo"),
                    (Primitive::Circle, "Círculo"),
                    (Primitive::Sprite, "Sprite"),
                ]
            } else {
                vec![
                    (Primitive::Cube, "Cubo"),
                    (Primitive::Sphere, "Esfera"),
                    (Primitive::Cylinder, "Cilindro"),
                    (Primitive::Plane, "Plano"),
                ]
            };
            for (primitive, name) in primitives {
                if ui.button(name).clicked() {
                    self.add_entity(Some(primitive), name);
                    ui.close();
                }
            }
            if ui.button("Grupo vazio").clicked() {
                self.add_entity(None, "Grupo");
                ui.close();
            }
            if ui.button("Câmera").clicked() {
                self.add_entity(None, "Câmera");
                let id = self.selected.clone().unwrap();
                self.scene_mut().entity_mut(&id).unwrap().camera = Some(Camera::default());
                ui.close();
            }
            ui.separator();
            for (kind, name) in [
                (UiKind::Text, "Texto de interface"),
                (UiKind::Image, "Imagem de interface"),
                (UiKind::Button, "Botão de interface"),
                (UiKind::Bar, "Barra de atributo"),
            ] {
                if ui.button(name).clicked() {
                    self.add_entity(None, name);
                    let id = self.selected.clone().unwrap();
                    self.scene_mut().entity_mut(&id).unwrap().ui = Some(UiElement {
                        kind,
                        ..Default::default()
                    });
                    ui.close();
                }
            }
        });
    }
    pub(super) fn hierarchy(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("hierarchy")
            .default_width(220.)
            .width_range(165.0..=370.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("HIERARQUIA");
                    self.creation_menu(ui);
                });
                ui.add_space(4.);
                let name = &mut self.scene_mut().name;
                ui.add(egui::TextEdit::singleline(name).desired_width(ui.available_width()));
                ui.separator();
                let root_drop = ui
                    .add_sized(
                        [ui.available_width(), 27.],
                        egui::Button::new("Raiz da cena · solte aqui"),
                    )
                    .on_hover_text(
                        "Arraste objetos para cá para retirar o pai e manter a posição na cena.",
                    );
                if root_drop.clicked() {
                    self.select(None);
                }
                self.hierarchy_drop(ui, &root_drop, None);
                let scene = self.scene().clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entity in scene.entities.iter().filter(|e| e.parent.is_none()) {
                        self.hierarchy_item(ui, &scene, &entity.id, 0);
                    }
                    let blank =
                        ui.allocate_response(Vec2::new(ui.available_width(), 60.), Sense::click());
                    if blank.clicked() {
                        self.select(None);
                    }
                    self.hierarchy_drop(ui, &blank, None);
                });
            });
    }
    pub(super) fn hierarchy_item(
        &mut self,
        ui: &mut egui::Ui,
        scene: &Scene,
        id: &str,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        let Some(e) = scene.entity(id) else { return };
        ui.push_id(id, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.);
                let icon = if e.camera.is_some() {
                    "◉"
                } else if e.ui.is_some() {
                    "▤"
                } else if e.primitive.is_none() {
                    "▾"
                } else {
                    "◇"
                };
                if self.rename.as_ref().is_some_and(|rename| rename.id == id) {
                    let rename = self.rename.as_mut().unwrap();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut rename.text)
                            .id_salt(("rename_entity", id))
                            .desired_width(ui.available_width()),
                    );
                    if rename.focus {
                        response.request_focus();
                        rename.focus = false;
                    }
                    let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    let confirm =
                        ui.input(|i| i.key_pressed(egui::Key::Enter)) || response.lost_focus();
                    if cancel {
                        self.rename = None;
                        response.surrender_focus();
                    } else if confirm {
                        let name = self.rename.take().unwrap().text;
                        if let Err(error) = editing::rename_entity(self.scene_mut(), id, &name) {
                            self.log(error);
                        }
                        response.surrender_focus();
                    }
                    return;
                }
                let response = ui
                    .add(
                        egui::Button::selectable(
                            self.selection.ids.iter().any(|selected| selected == id),
                            format!("{icon} {}", e.name),
                        )
                        .truncate()
                        .sense(Sense::click_and_drag()),
                    )
                    .on_hover_text(&e.name);
                if response.clicked() {
                    self.select_click(Some(id.into()), ui.input(|i| i.modifiers), true);
                    self.focus_object_click(
                        Some(id.into()),
                        response.double_clicked(),
                        ui.input(|i| i.modifiers),
                    );
                }
                if response.secondary_clicked()
                    && !self.selection.ids.iter().any(|selected| selected == id)
                {
                    self.select(Some(id.into()));
                }
                if response.drag_started()
                    && !self.selection.ids.iter().any(|selected| selected == id)
                {
                    self.select(Some(id.into()));
                }
                response.dnd_set_drag_payload(self.selection.ids.clone());
                self.hierarchy_drop(ui, &response, Some(id.into()));
                response.context_menu(|ui| {
                    self.object_context(ui, id);
                });
            });
        });
        for child in scene
            .entities
            .iter()
            .filter(|c| c.parent.as_deref() == Some(id))
        {
            self.hierarchy_item(ui, scene, &child.id, depth + 1);
        }
    }
    pub(super) fn object_context(&mut self, ui: &mut egui::Ui, id: &str) {
        if !self.selection.ids.iter().any(|selected| selected == id) {
            self.select(Some(id.into()));
        }
        if ui.button("Renomear  F2").clicked() {
            self.selected = Some(id.into());
            self.begin_rename();
            ui.close();
        }
        for (label, tab, sub) in [
            ("Abrir no Estúdio", Tab::Studio, StudioTab::Model),
            ("Editar animações", Tab::Studio, StudioTab::Animation),
            ("Editar lógica", Tab::Logic, StudioTab::Model),
        ] {
            if ui.button(label).clicked() {
                self.select(Some(id.into()));
                self.tab = tab;
                self.studio.tab = sub;
                self.studio.owner = Some(id.into());
                if sub == StudioTab::Animation {
                    self.open_animation_for(id);
                }
                if tab == Tab::Studio {
                    self.frame_selection();
                }
                ui.close();
            }
        }
        if ui.button("Duplicar hierarquia").clicked() {
            self.duplicate();
            ui.close();
        }
        if ui.button("Agrupar").clicked() {
            self.group();
            ui.close();
        }
        if ui.button("Excluir hierarquia").clicked() {
            self.delete();
            ui.close();
        }
    }

    fn hierarchy_drop(&mut self, ui: &egui::Ui, response: &egui::Response, parent: Option<Id>) {
        if let Some(ids) = response.dnd_hover_payload::<Vec<Id>>() {
            let mut validation = self.scene().clone();
            let result = editing::reparent_selection(&mut validation, &ids, parent.clone());
            let color = if result.is_ok() {
                Color32::from_rgb(123, 224, 202)
            } else {
                Color32::from_rgb(238, 113, 113)
            };
            ui.painter().rect_stroke(
                response.rect,
                3.,
                egui::Stroke::new(2., color),
                egui::StrokeKind::Inside,
            );
            if let Err(error) = result {
                response.clone().on_hover_text(error);
            }
        }
        if let Some(ids) = response.dnd_release_payload::<Vec<Id>>()
            && let Err(error) = editing::reparent_selection(self.scene_mut(), &ids, parent)
        {
            self.log(error);
            self.console = true;
        }
    }
}
