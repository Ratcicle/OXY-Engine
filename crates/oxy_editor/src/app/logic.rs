//! Lógica: the toolbar, the node inspector, input actions and the visual logic guide.
use super::*;
use crate::graph_ui::{catalog_docked, categories, graph_meta, operation_description};
use crate::icons::{self, Icon};
use oxy_core::graph::Graph;
use oxy_core::input_actions;

#[derive(Default)]
pub(super) struct LogicUi {
    pub inputs: bool,
    pub guide: bool,
    /// Guide page: empty for the first behavior, "recipe:N", "category:Name" or a node.
    pub topic: String,
    search: String,
    action_name: String,
    action_key: String,
    remove: Option<Id>,
}
impl Editor {
    /// The toolbar, then the catalog and canvas of the selected object's graph.
    pub(super) fn logic_view(&mut self, ui: &mut egui::Ui) {
        let objects = self.logic_objects();
        let Some(id) = self
            .selected
            .clone()
            .filter(|id| self.scene().entity(id).is_some())
        else {
            self.logic_toolbar(ui, None, &objects);
            let area = ui.available_rect_before_wrap();
            crate::widgets::empty_state(
                ui,
                area,
                Icon::Logic,
                "Nenhum objeto selecionado",
                "A lógica pertence a um objeto. Escolha um na Hierarquia para criar seu comportamento.",
                |_| {},
            );
            return;
        };
        let trace: Vec<_> = self
            .play
            .runtime
            .as_ref()
            .map(|rt| {
                rt.traces
                    .iter()
                    .filter(|t| t.object == id)
                    .map(|t| t.node.clone())
                    .collect()
            })
            .unwrap_or_default();
        let (name, icon) = self
            .scene()
            .entity(&id)
            .map(|e| (e.name.clone(), entity_icon(e)))
            .expect("Objeto validado acima");
        // The graph leaves the document while it is drawn, so the parameter editors can
        // read the project without cloning it. It returns before the frame ends.
        let mut graph = self.take_graph(&id);
        self.logic_toolbar(ui, Some((&mut graph, &name, icon)), &objects);
        let project = &self.state.project;
        let scene = project
            .scene(&self.scene_id)
            .expect("Cena selecionada validada");
        self.graph.show(
            ui,
            &mut graph,
            &objects,
            project,
            scene,
            Some(&id),
            &name,
            &trace,
        );
        self.restore_graph(&id, graph);
        if std::mem::take(&mut self.graph.configure_inputs) {
            self.pause();
            self.logic_ui.inputs = true;
        }
        if let Some(topic) = self.graph.guide_requested.take() {
            self.open_guide(&topic);
        }
    }
    /// Propriedades in Lógica: the selected node or connection of the object's graph.
    pub(super) fn logic_properties_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties")
            .default_width((ctx.content_rect().width() * 0.23).clamp(140., 290.))
            .width_range(130.0..=(ctx.content_rect().width() * 0.32).clamp(140., 460.))
            .resizable(true)
            .show(ctx, |ui| {
                crate::widgets::panel_header(ui, "Propriedades", |_| {});
                ui.add_space(8.);
                let Some(id) = self
                    .selected
                    .clone()
                    .filter(|id| self.scene().entity(id).is_some())
                else {
                    crate::widgets::hint(
                        ui,
                        "Selecione um objeto na Hierarquia para editar sua lógica.",
                    );
                    return;
                };
                let objects = self.logic_objects();
                let mut graph = self.take_graph(&id);
                let project = &self.state.project;
                let scene = project
                    .scene(&self.scene_id)
                    .expect("Cena selecionada validada");
                let view = &mut self.graph;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.;
                    view.inspector(ui, &mut graph, &objects, project, scene, Some(&id));
                });
                self.restore_graph(&id, graph);
            });
    }
    fn logic_objects(&self) -> Vec<(Id, String)> {
        self.scene()
            .entities
            .iter()
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect()
    }
    fn take_graph(&mut self, id: &str) -> Graph {
        self.scene_mut()
            .entity_mut(id)
            .map(|entity| std::mem::take(&mut entity.graph))
            .unwrap_or_default()
    }
    fn restore_graph(&mut self, id: &str, graph: Graph) {
        if let Some(entity) = self.scene_mut().entity_mut(id) {
            entity.graph = graph;
        }
    }
    /// Whose logic it is, project resources at the right, then checking and the view.
    fn logic_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<(&mut Graph, &str, Icon)>,
        objects: &[(Id, String)],
    ) {
        use crate::{theme, widgets};
        let (rect, mut right) = widgets::toolbar_right(ui, crate::studio::ROW_HEIGHT);
        right.spacing_mut().item_spacing.x = 4.;
        let names = rect.width() >= 780.;
        let (graph, name, icon) = match graph {
            Some((graph, name, icon)) => (Some(graph), name, icon),
            None => (None, "", Icon::Logic),
        };
        let has_nodes = graph.as_ref().is_some_and(|graph| !graph.nodes.is_empty());
        let mut fit = false;
        let mut validate = false;
        let mut zoom = None;
        right.add_enabled_ui(has_nodes, |ui| {
            fit = icons::button(
                ui,
                Icon::Frame,
                "Enquadrar",
                "Mostra o grafo inteiro.",
                false,
                names,
            )
            .clicked();
        });
        right.add_enabled_ui(graph.is_some(), |ui| {
            zoom = zoom_control(ui, self.graph.zoom());
        });
        widgets::divider(&mut right);
        right.add_enabled_ui(graph.is_some(), |ui| {
            validate = icons::button(
                ui,
                Icon::Check,
                "Validar",
                "Confere tipos, parâmetros e referências do grafo.",
                false,
                names,
            )
            .clicked();
        });
        widgets::divider(&mut right);
        let guide = icons::button(
            &mut right,
            Icon::Book,
            "Guia",
            "Guia de lógica visual e receitas (F1).",
            false,
            names,
        );
        icons::register_qa(&right, "Guia de lógica visual", guide.rect);
        if guide.clicked() {
            self.open_guide("");
        }
        if icons::button(
            &mut right,
            Icon::Keyboard,
            "Ações de entrada",
            "Crie ações e escolha teclas para os nós e controladores do projeto.",
            false,
            names,
        )
        .clicked()
        {
            self.pause();
            self.logic_ui.inputs = true;
        }
        let mut left = widgets::toolbar_left(ui, rect, &right);
        left.spacing_mut().item_spacing.x = 6.;
        left.add_space(2.);
        let mut graph = graph;
        if !catalog_docked(rect.width())
            && let Some(graph) = graph.as_deref_mut()
        {
            let names = left.available_width() > 300.;
            let add = icons::button(
                &mut left,
                Icon::Plus,
                "Adicionar nó",
                "Catálogo de nós: o escolhido aparece no centro da vista.",
                false,
                names,
            );
            egui::Popup::from_toggle_button_response(&add)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.graph.catalog_menu(ui, graph));
        }
        match &graph {
            Some(graph) => {
                if left.available_width() > 260. {
                    left.label(
                        egui::RichText::new("Lógica de")
                            .size(12.5)
                            .color(theme::TEXT_MUTED),
                    );
                }
                egui::Frame::new()
                    .fill(theme::CARD_FILL)
                    .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(&mut left, |ui| {
                        ui.spacing_mut().item_spacing.x = 6.;
                        let (tile, _) = ui.allocate_exact_size(Vec2::splat(14.), Sense::hover());
                        icons::paint(ui, tile, icon, theme::ACCENT_BRIGHT);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(name)
                                    .font(theme::medium(12.5))
                                    .color(theme::TEXT),
                            )
                            .truncate(),
                        );
                    });
                if left.available_width() > 110. {
                    left.label(
                        egui::RichText::new(graph_meta(graph))
                            .size(12.)
                            .color(theme::TEXT_MUTED),
                    );
                }
            }
            None => {
                left.add(
                    egui::Label::new(
                        egui::RichText::new("Lógica · nenhum objeto selecionado")
                            .size(12.5)
                            .color(theme::TEXT_MUTED),
                    )
                    .truncate(),
                );
            }
        }
        if let Some(graph) = graph {
            if fit {
                self.graph.fit(graph);
            }
            if validate {
                self.graph.validate(graph, objects);
            }
            if let Some(factor) = zoom {
                self.graph.zoom_by(factor);
            }
        }
    }
    pub(super) fn open_guide(&mut self, topic: &str) {
        self.pause();
        self.studio.playing = false;
        self.logic_ui.guide = true;
        self.logic_ui.topic = topic.into();
    }
    pub(super) fn logic_dialogs(&mut self, ctx: &egui::Context) {
        if self.logic_ui.inputs {
            self.input_actions_dialog(ctx);
        }
        if self.logic_ui.guide {
            self.guide_ui(ctx);
        }
    }
    /// Project input actions as a table: name, key, where each is used and deletion.
    fn input_actions_dialog(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let content = ctx.content_rect();
        let width = (content.width() - 48.).clamp(160., 660.);
        let response = egui::Modal::new("input_actions".into()).show(ctx, |ui| {
            ui.set_width(width);
            dialog_header(
                ui,
                "Ações de entrada",
                "As ações pertencem ao projeto. Nós e controladores usam os mesmos vínculos.",
                &mut close,
            );
            let wide = width >= 520.;
            let name_width = if wide { 190. } else { (width - 160.).max(80.) };
            egui::ScrollArea::vertical()
                .max_height((content.height() - 230.).max(90.))
                .show(ui, |ui| {
                    if wide {
                        // Same widths as the fields below: a text field adds its margins.
                        ui.horizontal(|ui| {
                            column_title(ui, "Ação", name_width + 8.);
                            column_title(ui, "Tecla", 110.);
                            column_title(ui, "Usada por", 160.);
                        });
                    }
                    for (id, key) in self.state.project.input_bindings.clone() {
                        ui.push_id(&id, |ui| {
                            self.input_action_row(ui, &id, &key, name_width, wide);
                        });
                    }
                    ui.add_space(6.);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.logic_ui.action_name)
                                .hint_text("Nome da nova ação")
                                .desired_width(name_width),
                        );
                        if self.logic_ui.action_key.is_empty() {
                            self.logic_ui.action_key = "K".into();
                        }
                        key_choice(ui, &mut self.logic_ui.action_key, "new_action_key");
                        if ui
                            .add_enabled(
                                !self.logic_ui.action_name.trim().is_empty(),
                                egui::Button::new("Criar ação"),
                            )
                            .clicked()
                        {
                            match input_actions::create(
                                &mut self.state.project,
                                &self.logic_ui.action_name,
                                &self.logic_ui.action_key,
                            ) {
                                Ok(_) => self.logic_ui.action_name.clear(),
                                Err(e) => self.warn(e),
                            }
                        }
                    });
                    self.controller_actions(ui);
                });
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let response = ui.button("Fechar");
                    icons::register_qa(ui, "Fechar ações de entrada", response.rect);
                    if response.clicked() {
                        close = true;
                    }
                });
            });
        });
        if close || response.should_close() {
            self.logic_ui.inputs = false;
            self.logic_ui.remove = None;
        }
    }
    fn input_action_row(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        old_key: &str,
        name_width: f32,
        wide: bool,
    ) {
        use crate::theme;
        let uses = input_actions::references(&self.state.project, id);
        let mut name = input_actions::label(&self.state.project, id).into_owned();
        ui.horizontal(|ui| {
            if ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(name_width))
                .changed()
                && let Err(e) = input_actions::rename(&mut self.state.project, id, &name)
            {
                self.warn(e);
            }
            let mut key = old_key.to_owned();
            key_choice(ui, &mut key, id);
            if key != old_key {
                self.state.project.input_bindings.insert(id.into(), key);
            }
            if wide {
                let used = match uses.as_slice() {
                    [] => "—".to_owned(),
                    [one] => one.clone(),
                    many => format!("{} usos", many.len()),
                };
                let cell = Vec2::new((ui.available_width() - 34.).max(20.), 22.);
                ui.allocate_ui_with_layout(
                    cell,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let label = ui.add(
                            egui::Label::new(
                                egui::RichText::new(used).size(12.).color(theme::TEXT_MUTED),
                            )
                            .truncate(),
                        );
                        if !uses.is_empty() {
                            label.on_hover_text(uses.join("\n"));
                        }
                    },
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let delete = icons::small(ui, Icon::Trash, "Excluir ação", 24.);
                icons::register_qa(ui, &format!("Excluir ação {name}"), delete.rect);
                if delete.clicked() {
                    self.logic_ui.remove = Some(id.into());
                }
            });
        });
        if self.logic_ui.remove.as_deref() != Some(id) {
            return;
        }
        // Deleting asks in place, under its own row.
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(46, 39, 24))
            .stroke(egui::Stroke::new(1., egui::Color32::from_rgb(74, 61, 34)))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if uses.is_empty() {
                    ui.label("Excluir a ação sem referências?");
                    ui.horizontal(|ui| {
                        if ui.button("Cancelar exclusão").clicked() {
                            self.logic_ui.remove = None;
                        }
                        if ui
                            .button(
                                egui::RichText::new("Confirmar exclusão da ação")
                                    .color(theme::ERROR),
                            )
                            .clicked()
                        {
                            match input_actions::remove(&mut self.state.project, id) {
                                Ok(()) => self.logic_ui.remove = None,
                                Err(e) => self.warn(e),
                            }
                        }
                    });
                } else {
                    ui.label(
                        egui::RichText::new("Altere estes vínculos antes de excluir:")
                            .color(egui::Color32::from_rgb(232, 210, 160)),
                    );
                    for usage in &uses {
                        ui.label(
                            egui::RichText::new(usage)
                                .size(12.)
                                .color(theme::TEXT_MUTED),
                        );
                    }
                    if ui.button("Cancelar exclusão").clicked() {
                        self.logic_ui.remove = None;
                    }
                }
            });
    }
    /// Movement actions of the selected object's controller, inside Ações de entrada.
    fn controller_actions(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(mut actions) = self.scene().entity(&id).and_then(|e| {
            e.character3d
                .as_ref()
                .map(|c| c.actions.clone())
                .or_else(|| e.controller.as_ref().map(|c| c.actions.clone()))
        }) else {
            return;
        };
        ui.add_space(6.);
        let choices: Vec<_> = self
            .state
            .project
            .input_bindings
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    input_actions::label(&self.state.project, id).into_owned(),
                )
            })
            .collect();
        let kind = self.scene().kind;
        crate::widgets::open_card(
            ui,
            "controller_actions",
            "Controlador do objeto selecionado",
            |ui| {
                for (label, action) in [
                    ("Esquerda", &mut actions.left),
                    ("Direita", &mut actions.right),
                    ("Pular", &mut actions.jump),
                    ("Frente", &mut actions.forward),
                    ("Trás", &mut actions.back),
                ] {
                    if kind == SceneKind::TwoD && matches!(label, "Frente" | "Trás") {
                        continue;
                    }
                    crate::widgets::property_row(ui, label, "", |ui| {
                        egui::ComboBox::from_id_salt(("controller_action", label))
                            .selected_text(
                                choices
                                    .iter()
                                    .find(|(id, _)| id == action)
                                    .map_or("Escolha uma ação", |(_, name)| name.as_str()),
                            )
                            .show_ui(ui, |ui| {
                                for (id, name) in &choices {
                                    ui.selectable_value(action, id.clone(), name);
                                }
                            });
                    });
                }
            },
        );
        if let Some(entity) = self.scene_mut().entity_mut(&id) {
            if let Some(c) = &mut entity.character3d {
                c.actions = actions;
            } else if let Some(c) = &mut entity.controller {
                c.actions = actions;
            }
        }
    }
    fn guide_ui(&mut self, ctx: &egui::Context) {
        use crate::theme;
        let mut close = false;
        let mut open_recipe = None;
        let content = ctx.content_rect();
        let width = (content.width() - 48.).clamp(160., 880.);
        let body_height = (content.height() - 190.).clamp(80., 560.);
        let response = egui::Modal::new("logic_guide".into()).show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Guia de lógica visual")
                        .font(theme::medium(15.))
                        .color(theme::TEXT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::small(ui, Icon::Close, "Fechar guia", 24.).clicked() {
                        close = true;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.logic_ui.search)
                            .hint_text("Buscar operação ou assunto")
                            .desired_width((ui.available_width() - 12.).clamp(80., 260.)),
                    );
                });
            });
            ui.add_space(6.);
            if width >= 560. {
                let nav_width = (width * 0.28).clamp(170., 230.);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(nav_width, body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("guide_nav")
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.guide_nav(ui));
                        },
                    );
                    let (line, _) =
                        ui.allocate_exact_size(Vec2::new(1., body_height), Sense::hover());
                    ui.painter().rect_filled(line, 0., theme::BORDER);
                    ui.add_space(8.);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            // Each page opens at its top.
                            egui::ScrollArea::vertical()
                                .id_salt(("guide_article", self.logic_ui.topic.clone()))
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| open_recipe = self.guide_article(ui));
                        },
                    );
                });
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("guide_all")
                    .max_height(body_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.guide_nav(ui);
                        ui.separator();
                        open_recipe = self.guide_article(ui);
                    });
            }
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Fechar guia").clicked() {
                        close = true;
                    }
                });
            });
        });
        if close || response.should_close() {
            self.logic_ui.guide = false;
        }
        if let Some(recipe) = open_recipe {
            let result = (|| {
                let project = oxy_core::guide_recipes::load(recipe)?;
                let path = std::env::temp_dir()
                    .join(format!("oxy-guide-{}", new_id()))
                    .join(persistence::PROJECT_FILE);
                persistence::save_project(&path, &project)?;
                Ok::<_, String>(path)
            })();
            match result {
                Ok(path) => {
                    self.logic_ui.guide = false;
                    self.transition(Transition::Recipe(path));
                }
                Err(e) => self.warn(e),
            }
        }
    }
    /// Guide topics: first behavior, recipes and nodes by category, or search results.
    fn guide_nav(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 1.;
        let query = self.logic_ui.search.trim().to_lowercase();
        let recipes = oxy_core::guide_recipes::recipes();
        if !query.is_empty() {
            nav_section(ui, "Resultados");
            let mut found = false;
            for (index, recipe) in recipes.iter().enumerate() {
                if format!("{} {}", recipe.title, recipe.setup)
                    .to_lowercase()
                    .contains(&query)
                {
                    found = true;
                    let key = format!("recipe:{index}");
                    if nav_item(
                        ui,
                        recipe.title,
                        Some("Receita"),
                        self.logic_ui.topic == key,
                    )
                    .clicked()
                    {
                        self.logic_ui.topic = key;
                    }
                }
            }
            for topic in oxy_core::guide::topics() {
                let op = topic.operation;
                if format!("{} {} {}", op.label, op.category, topic.purpose)
                    .to_lowercase()
                    .contains(&query)
                {
                    found = true;
                    if nav_item(
                        ui,
                        op.label,
                        Some(op.category),
                        self.logic_ui.topic == op.id,
                    )
                    .clicked()
                    {
                        self.logic_ui.topic = op.id.into();
                    }
                }
            }
            if !found {
                crate::widgets::hint(ui, "Nada encontrado. Tente outro termo.");
            }
            return;
        }
        if nav_item(
            ui,
            "Primeiro comportamento",
            None,
            self.logic_ui.topic.is_empty(),
        )
        .clicked()
        {
            self.logic_ui.topic.clear();
        }
        nav_section(ui, "Receitas");
        for (index, recipe) in recipes.iter().enumerate() {
            let key = format!("recipe:{index}");
            if nav_item(ui, recipe.title, None, self.logic_ui.topic == key).clicked() {
                self.logic_ui.topic = key;
            }
        }
        nav_section(ui, "Nós por categoria");
        for (category, ops) in categories() {
            let key = format!("category:{category}");
            let selected =
                self.logic_ui.topic == key || ops.iter().any(|op| op.id == self.logic_ui.topic);
            if nav_item(ui, category, Some(&ops.len().to_string()), selected).clicked() {
                self.logic_ui.topic = key;
            }
        }
    }
    /// The page chosen in the guide. Returns the recipe to open, if requested.
    fn guide_article(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let mut open = None;
        let topic = self.logic_ui.topic.clone();
        ui.spacing_mut().item_spacing.y = 6.;
        let recipe = topic
            .strip_prefix("recipe:")
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| {
                oxy_core::guide_recipes::recipes()
                    .get(index)
                    .map(|recipe| (index, recipe))
            });
        let category = topic
            .strip_prefix("category:")
            .and_then(|name| categories().iter().find(|(category, _)| *category == name));
        if let Some((index, recipe)) = recipe {
            kicker(ui, "Receita");
            heading(ui, recipe.title);
            // The recipe opens from the top of its page, before the details.
            if ui
                .button(format!(
                    "Abrir cópia da receita: {}",
                    recipe.title.to_lowercase()
                ))
                .clicked()
            {
                open = Some(index);
            }
            ui.add_space(2.);
            section(ui, "Preparação", recipe.setup);
            section(ui, "Conexões e valores", recipe.flow);
            section(ui, "Resultado esperado", recipe.expected);
        } else if let Some((category, ops)) = category {
            kicker(ui, "Nós");
            heading(ui, category);
            paragraph(
                ui,
                "Escolha um nó para ver propósito, entradas e saídas, parâmetros, exemplo e cuidados. O mesmo texto abre por Ajuda deste nó.",
            );
            ui.spacing_mut().item_spacing.y = 1.;
            for op in ops {
                if nav_item(ui, op.label, None, false)
                    .on_hover_text(operation_description(op.id))
                    .clicked()
                {
                    self.logic_ui.topic = op.id.into();
                }
            }
        } else if let Some(topic) = oxy_core::guide::topics()
            .iter()
            .find(|t| t.operation.id == topic)
        {
            let op = topic.operation;
            kicker(ui, op.category);
            heading(ui, op.label);
            paragraph(ui, topic.purpose);
            section(ui, "Preparação e contexto", topic.requirements());
            let ports: Vec<String> = op
                .inputs
                .iter()
                .map(|port| format!("Entrada · {} ({})", port.label, port.kind.label()))
                .chain(
                    op.outputs
                        .iter()
                        .map(|port| format!("Saída · {} ({})", port.label, port.kind.label())),
                )
                .collect();
            if !ports.is_empty() {
                section(ui, "Entradas e saídas", &ports.join("\n"));
            }
            if !op.params.is_empty() {
                let params: Vec<_> = op.params.iter().map(|param| param.label).collect();
                section(ui, "Parâmetros", &params.join("\n"));
            }
            section(ui, "Exemplo", topic.example);
            section(ui, "Cuidados", topic.caution);
        } else {
            heading(ui, "Seu primeiro comportamento");
            for line in oxy_core::guide::FIRST_BEHAVIOR.lines() {
                match line
                    .split_once(". ")
                    .filter(|(number, _)| number.chars().all(|c| c.is_ascii_digit()))
                {
                    Some((number, text)) => step(ui, number, text),
                    None => paragraph(ui, line),
                }
            }
            if ui
                .button("Abrir cópia da receita: primeira mensagem")
                .clicked()
            {
                open = Some(0);
            }
            ui.add_space(4.);
            ui.separator();
            ui.label(
                egui::RichText::new("Execução e dados")
                    .font(crate::theme::medium(14.))
                    .color(crate::theme::TEXT),
            );
            for line in oxy_core::guide::FOUNDATIONS.lines() {
                paragraph(ui, line);
            }
        }
        open
    }
}
fn dialog_header(ui: &mut egui::Ui, title: &str, subtitle: &str, close: &mut bool) {
    use crate::theme;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(theme::medium(15.))
                .color(theme::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::small(ui, Icon::Close, "Fechar", 24.).clicked() {
                *close = true;
            }
        });
    });
    ui.label(
        egui::RichText::new(subtitle)
            .size(12.)
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(6.);
}
fn column_title(ui: &mut egui::Ui, text: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text.to_uppercase(),
        crate::theme::medium(10.5),
        crate::theme::TEXT_MUTED,
    );
}
fn nav_section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .font(crate::theme::medium(10.5))
            .color(crate::theme::TEXT_MUTED),
    );
    ui.add_space(2.);
}
/// Full-width row of the guide navigation, with an optional muted note at the right.
fn nav_item(
    ui: &mut egui::Ui,
    label: &str,
    trailing: Option<&str>,
    selected: bool,
) -> egui::Response {
    use crate::theme;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let fill = if selected {
        theme::ACCENT_SOFT
    } else if response.hovered() {
        theme::BG_RAISED
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5., fill);
    let mut right = rect.right() - 8.;
    if let Some(trailing) = trailing {
        let galley = ui.painter().layout_no_wrap(
            trailing.to_owned(),
            egui::FontId::proportional(11.),
            theme::TEXT_MUTED,
        );
        right -= galley.size().x;
        ui.painter().galley(
            egui::pos2(right, rect.center().y - galley.size().y / 2.),
            galley,
            theme::TEXT_MUTED,
        );
        right -= 8.;
    }
    let color = if selected {
        egui::Color32::WHITE
    } else {
        theme::TEXT
    };
    ui.painter().with_clip_rect(rect.with_max_x(right)).text(
        egui::pos2(rect.left() + 8., rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        color,
    );
    response
}
fn kicker(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(crate::theme::medium(10.5))
            .color(crate::theme::ACCENT_BRIGHT),
    );
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(crate::theme::medium(17.))
            .color(crate::theme::TEXT),
    );
}
fn paragraph(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(12.5)
                .color(egui::Color32::from_rgb(196, 196, 202)),
        )
        .wrap(),
    );
}
fn section(ui: &mut egui::Ui, title: &str, text: &str) {
    ui.add_space(2.);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .font(crate::theme::medium(10.5))
            .color(crate::theme::TEXT_MUTED),
    );
    paragraph(ui, text);
}
fn step(ui: &mut egui::Ui, number: &str, text: &str) {
    ui.horizontal_top(|ui| {
        let (badge, _) = ui.allocate_exact_size(Vec2::splat(20.), Sense::hover());
        ui.painter()
            .circle_filled(badge.center(), 9., crate::theme::ACCENT_SOFT);
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            number,
            crate::theme::medium(11.),
            crate::theme::ACCENT_BRIGHT,
        );
        paragraph(ui, text);
    });
}
/// Zoom out, the current zoom and zoom in. Drawn right to left inside the toolbar.
fn zoom_control(ui: &mut egui::Ui, zoom: f32) -> Option<f32> {
    use crate::theme;
    let mut factor = None;
    egui::Frame::new()
        .fill(theme::CARD_FILL)
        .stroke(egui::Stroke::new(1., theme::CARD_BORDER))
        .corner_radius(6)
        .inner_margin(1)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.;
            if icons::small(ui, Icon::Plus, "Aproximar", 22.).clicked() {
                factor = Some(1.25);
            }
            ui.add_sized(
                [44., 22.],
                egui::Label::new(
                    egui::RichText::new(format!("{:.0}%", zoom * 100.))
                        .size(12.)
                        .color(theme::TEXT),
                )
                .selectable(false),
            );
            if icons::small(ui, Icon::Minus, "Afastar", 22.).clicked() {
                factor = Some(1. / 1.25);
            }
        });
    factor
}
fn key_choice(ui: &mut egui::Ui, key: &mut String, salt: impl std::hash::Hash) {
    egui::ComboBox::from_id_salt(salt).selected_text(crate::labels::key(key)).width(110.).show_ui(ui,|ui| {
        let mut common:Vec<_>=egui::Key::ALL.iter().filter(|k|k.name().len()==1 && k.name().as_bytes()[0].is_ascii_alphanumeric()).collect();
        common.sort_by_key(|k|k.name());
        egui::Grid::new("common_keys").num_columns(8).show(ui,|ui| {
            for (i,candidate) in common.iter().enumerate() {ui.selectable_value(key,candidate.name().into(),candidate.name());if (i+1)%8==0 {ui.end_row();}}
        });
        ui.selectable_value(key,"Space".into(),"Espaço");
        ui.selectable_value(key,"Shift".into(),"Shift (segurar)");
        ui.collapsing("Mouse",|ui|{for candidate in ["MouseLeft","MouseRight","MouseMiddle","Mouse4","Mouse5","WheelUp","WheelDown"] {ui.selectable_value(key,candidate.into(),crate::labels::key(candidate));}});
        ui.collapsing("Outras teclas",|ui| {for candidate in egui::Key::ALL {
            if !matches!(candidate,egui::Key::Escape|egui::Key::F3) && !common.contains(&candidate) {
                ui.selectable_value(key,candidate.name().into(),crate::labels::key(candidate.name()));
            }
        }});
    }).response.on_hover_text("Tecla que aciona este vínculo. Escape libera o jogo; F3 pertence ao diagnóstico do player.");
}
