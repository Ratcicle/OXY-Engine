use super::*;
use crate::icons::{self, Icon};
use crate::theme;
use egui::{Align, Layout, UiBuilder};

const ROW_HEIGHT: f32 = 26.;

/// Icon that tells what kind of object an entity is, in the hierarchy and inspector.
pub(crate) fn entity_icon(e: &Entity) -> Icon {
    if e.camera.is_some() {
        Icon::Camera
    } else if e.ui.is_some() {
        Icon::Interface
    } else if e.character3d.is_some() || e.controller.is_some() {
        Icon::Person
    } else if let Some(primitive) = e.primitive {
        match primitive {
            Primitive::Sphere | Primitive::Circle => Icon::Sphere,
            Primitive::Plane | Primitive::Rectangle => Icon::Plane,
            Primitive::Sprite => Icon::Sprite,
            _ => Icon::Cube,
        }
    } else if e.mesh.is_some() {
        Icon::Cube
    } else {
        Icon::Group
    }
}

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
            self.hierarchy_ui.collapsed.remove(&id);
            parent = self.scene().entity(&id).and_then(|e| e.parent.clone());
        }
    }
    pub(super) fn visible_hierarchy_order(&self) -> Vec<Id> {
        editing::hierarchy_order(self.scene())
            .into_iter()
            .filter(|id| {
                let mut parent = self.scene().entity(id).and_then(|e| e.parent.as_deref());
                while let Some(id) = parent {
                    if self.hierarchy_ui.collapsed.contains(id) {
                        return false;
                    }
                    parent = self.scene().entity(id).and_then(|e| e.parent.as_deref());
                }
                true
            })
            .collect()
    }
    pub(super) fn creation_menu(&mut self, ui: &mut egui::Ui) {
        icons::menu_button(
            ui,
            Icon::Plus,
            "+ Objeto",
            "Adicionar objeto",
            false,
            |ui| {
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
                    let camera = self.scene_mut().entity_mut(&id).unwrap();
                    camera.parent = None;
                    camera.camera = Some(Camera::default());
                    ui.close();
                }
                if self.scene().kind == SceneKind::ThreeD && self.tab == Tab::Scene {
                    ui.separator();
                    use oxy_core::movement_presets::{self, MovementPreset};
                    for (preset, label) in [
                        (MovementPreset::FirstPerson, "Personagem em primeira pessoa"),
                        (MovementPreset::ThirdPerson, "Personagem em terceira pessoa"),
                        (MovementPreset::Platform, "Plataforma móvel"),
                    ] {
                        if ui.button(label).on_hover_text("Cria componentes, peças e referências editáveis. A criação inteira pode ser desfeita.").clicked(){
                        let scene=self.scene().id.clone();
                        match movement_presets::create(&mut self.state.project,&scene,preset,Vec3::new(0.,0.02,0.)) {Ok(id)=>self.select(Some(id)),Err(error)=>self.warn(error)}
                        ui.close();
                    }
                    }
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
            },
        );
    }
    pub(super) fn hierarchy(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("hierarchy")
            .default_width((ctx.content_rect().width() * 0.17).clamp(120., 220.))
            .width_range(115.0..=(ctx.content_rect().width() * 0.3).clamp(120., 370.))
            .resizable(true)
            .show(ctx, |ui| {
                if self.mesh_operation_active() || self.playing() {
                    ui.disable();
                }
                crate::widgets::panel_header(ui, "Hierarquia", |ui| self.creation_menu(ui));
                ui.add_space(8.);
                self.scene_bar(ui);
                ui.add_space(4.);
                ui.add(
                    egui::TextEdit::singleline(&mut self.hierarchy_ui.search)
                        .hint_text("Buscar objeto")
                        .desired_width(ui.available_width()),
                );
                ui.add_space(6.);
                let snapshot = self.scene().clone();
                let scene = oxy_core::scene_view::SceneView::new(&snapshot);
                let search = self.hierarchy_ui.search.trim().to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.;
                    if search.is_empty() {
                        for entity in scene.scene.entities.iter().filter(|e| e.parent.is_none()) {
                            self.hierarchy_item(ui, &scene, &entity.id, 0);
                        }
                    } else {
                        // Matches are listed flat, so every result is visible without expanding.
                        for entity in scene
                            .scene
                            .entities
                            .iter()
                            .filter(|e| e.name.to_lowercase().contains(&search))
                        {
                            self.hierarchy_row(ui, &scene, &entity.id, 0, false);
                        }
                    }
                    self.root_drop_zone(ui);
                });
            });
    }
    /// Empty space below the tree: clicking deselects, dropping moves objects to the root.
    fn root_drop_zone(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height().max(60.);
        let blank = ui.allocate_response(Vec2::new(ui.available_width(), height), Sense::click());
        icons::register_qa(ui, "Raiz da cena", blank.rect);
        if egui::DragAndDrop::has_payload_of_type::<Vec<Id>>(ui.ctx()) {
            let zone = Rect::from_min_size(blank.rect.min, Vec2::new(blank.rect.width(), 44.));
            ui.painter().rect_stroke(
                zone.shrink(2.),
                5.,
                egui::Stroke::new(1., theme::BORDER),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                zone.center(),
                egui::Align2::CENTER_CENTER,
                "Solte aqui para mover para a raiz",
                egui::FontId::proportional(12.),
                theme::TEXT_MUTED,
            );
        }
        if blank.clicked() {
            self.select(None);
        }
        self.hierarchy_drop(ui, &blank, None);
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
        let has_children = scene
            .index
            .as_ref()
            .ok()
            .and_then(|i| i.position(id).map(|p| !i.children[p].is_empty()))
            .unwrap_or(false);
        self.hierarchy_row(ui, scene, id, depth, has_children);
        if self.hierarchy_ui.collapsed.contains(id) {
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
    /// One hierarchy row: expand chevron, kind icon and name. Click selects, drag reparents.
    fn hierarchy_row(
        &mut self,
        ui: &mut egui::Ui,
        scene: &oxy_core::scene_view::SceneView<'_>,
        id: &str,
        depth: usize,
        has_children: bool,
    ) {
        let Some(e) = scene.entity(id) else { return };
        ui.push_id(id, |ui| {
            let (row_rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
            let mut row = ui.new_child(
                UiBuilder::new()
                    .max_rect(row_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            let ui = &mut row;
            ui.spacing_mut().item_spacing.x = 2.;
            ui.add_space(2. + depth as f32 * 14.);
            if has_children {
                let collapsed = self.hierarchy_ui.collapsed.contains(id);
                let (icon, label) = if collapsed {
                    (Icon::ChevronRight, format!("Expandir {}", e.name))
                } else {
                    (Icon::ChevronDown, format!("Recolher {}", e.name))
                };
                if icons::small(ui, icon, &label, 18.).clicked() {
                    if collapsed {
                        self.hierarchy_ui.collapsed.remove(id);
                    } else {
                        self.hierarchy_ui.collapsed.insert(id.into());
                    }
                }
            } else {
                ui.add_space(18.);
            }
            let icon = entity_icon(e);
            {
                if self
                    .hierarchy_ui
                    .rename
                    .as_ref()
                    .is_some_and(|rename| rename.id == id)
                {
                    let rename = self.hierarchy_ui.rename.as_mut().unwrap();
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
                        self.hierarchy_ui.rename = None;
                        response.surrender_focus();
                    } else if confirm {
                        let name = self.hierarchy_ui.rename.take().unwrap().text;
                        if let Err(error) = editing::rename_entity(self.scene_mut(), id, &name) {
                            self.log(error);
                        }
                        response.surrender_focus();
                    }
                    return;
                }
                let selected = self.selection.ids.iter().any(|selected| selected == id);
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), ROW_HEIGHT),
                    Sense::click_and_drag(),
                );
                icons::register_qa(ui, &format!("Objeto: {}", e.name), rect);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        selected,
                        &e.name,
                    )
                });
                let (fill, text) = if selected {
                    (theme::ACCENT_SOFT, Color32::WHITE)
                } else if response.hovered() {
                    (theme::BG_RAISED, theme::TEXT)
                } else {
                    (Color32::TRANSPARENT, theme::TEXT)
                };
                ui.painter().rect_filled(rect, 4., fill);
                let icon_rect = Rect::from_center_size(
                    egui::pos2(rect.left() + 10., rect.center().y),
                    Vec2::splat(14.),
                );
                icons::paint(
                    ui,
                    icon_rect,
                    icon,
                    if selected {
                        theme::ACCENT_BRIGHT
                    } else {
                        theme::TEXT_MUTED
                    },
                );
                let name_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 22., rect.top()),
                    rect.right_bottom(),
                );
                let mut name = ui.new_child(
                    UiBuilder::new()
                        .max_rect(name_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                name.add(
                    egui::Label::new(egui::RichText::new(&e.name).color(text))
                        .truncate()
                        .selectable(false),
                );
                let response = response.on_hover_text(&e.name);
                if self.hierarchy_ui.reveal_scroll.as_deref() == Some(id) {
                    if !ui.clip_rect().contains_rect(response.rect) {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    self.hierarchy_ui.reveal_scroll = None;
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
            }
        });
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
        if ui.button("Salvar hierarquia como modelo").clicked() {
            let scene_id = self.scene_id.clone();
            let name = self
                .scene()
                .entity(id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            match self.state.project.save_model(&scene_id,id,&name) {
                Ok(_)=>self.log("Modelo salvo na biblioteca, com sua estrutura editável. Salve o projeto para gravar em disco."),
                Err(e)=>self.warn(e),
            }
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
                crate::theme::SUCCESS
            } else {
                crate::theme::ERROR
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
