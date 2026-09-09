use super::*;

impl Editor {
    pub(super) fn reveal_selection(&mut self, id: Option<&str>) {
        let mut parent = id
            .and_then(|id| self.scene().entity(id))
            .and_then(|e| e.parent.clone());
        let mut visited = std::collections::HashSet::new();
        while let Some(id) = parent {
            if !visited.insert(id.clone()) {
                break;
            }
            self.collapsed.remove(&id);
            parent = self.scene().entity(&id).and_then(|e| e.parent.clone());
        }
    }
    pub(super) fn visible_hierarchy_order(&self) -> Vec<Id> {
        editing::hierarchy_order(self.scene())
            .into_iter()
            .filter(|id| {
                let mut parent = self.scene().entity(id).and_then(|e| e.parent.as_deref());
                while let Some(id) = parent {
                    if self.collapsed.contains(id) {
                        return false;
                    }
                    parent = self.scene().entity(id).and_then(|e| e.parent.as_deref());
                }
                true
            })
            .collect()
    }
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
                    (Primitive::Pyramid, "Pirâmide"),
                    (Primitive::Cone, "Cone"),
                    (Primitive::Tube, "Tubo"),
                ]
            };
            for (primitive, name) in primitives {
                if ui.button(name).clicked() {
                    self.create_primitive(primitive, name);
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
            .default_width((ctx.content_rect().width() * 0.17).clamp(120., 220.))
            .width_range(115.0..=(ctx.content_rect().width() * 0.3).clamp(120., 370.))
            .resizable(true)
            .show(ctx, |ui| {
                if self.mesh_operation_active() {
                    ui.disable();
                }
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
                let snapshot = self.scene().clone();
                let scene = oxy_core::scene_view::SceneView::new(&snapshot);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entity in scene.scene.entities.iter().filter(|e| e.parent.is_none()) {
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
        scene: &oxy_core::scene_view::SceneView<'_>,
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
                if scene
                    .index
                    .as_ref()
                    .ok()
                    .and_then(|i| i.position(id).map(|p| !i.children[p].is_empty()))
                    .unwrap_or(false)
                {
                    let collapsed = self.collapsed.contains(id);
                    if ui
                        .small_button(if collapsed { "▸" } else { "▾" })
                        .on_hover_text("Recolher ou expandir os filhos")
                        .clicked()
                    {
                        if collapsed {
                            self.collapsed.remove(id);
                        } else {
                            self.collapsed.insert(id.into());
                        }
                    }
                }
                let icon = if e.camera.is_some() {
                    "◉"
                } else if e.ui.is_some() {
                    "▤"
                } else if !e.has_geometry() {
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
                if self.reveal_scroll.as_deref() == Some(id) {
                    if !ui.clip_rect().contains_rect(response.rect) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    self.reveal_scroll = None;
                }
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
        if self.collapsed.contains(id) {
            return;
        }
        for child in scene
            .index
            .as_ref()
            .ok()
            .and_then(|i| i.position(id).map(|p| &i.children[p]))
            .into_iter()
            .flatten()
            .map(|i| &scene.scene.entities[*i])
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
        if ui.button("Editar pivô (P)").clicked() {
            self.set_spatial_tool(Tool::Pivot);
            ui.close();
        }
        if self
            .scene()
            .entity(id)
            .is_some_and(|e| e.collider.is_some())
            && ui.button("Editar colisor (C)").clicked()
        {
            self.set_spatial_tool(Tool::Collider);
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
            && self.structural_ready()
            && let Err(error) = editing::reparent_selection(self.scene_mut(), &ids, parent)
        {
            self.log(error);
            self.notice_last(false);
        }
    }
}
