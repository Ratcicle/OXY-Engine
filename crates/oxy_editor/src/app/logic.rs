use super::*;
use oxy_core::input_actions;

#[derive(Default)]
pub(super) struct LogicUi {
    pub inputs: bool,
    pub guide: bool,
    pub topic: String,
    search: String,
    action_name: String,
    action_key: String,
    remove: Option<Id>,
}
impl Editor {
    pub(super) fn logic_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Ações de entrada")
                .on_hover_text(
                    "Crie ações e escolha teclas para os nós e controladores do projeto.",
                )
                .clicked()
            {
                self.pause();
                self.logic_ui.inputs = true;
            }
            if ui.button("Guia de lógica visual").clicked() {
                self.open_guide("");
            }
        });
    }
    pub(super) fn open_guide(&mut self, topic: &str) {
        self.pause();
        self.studio.playing = false;
        self.logic_ui.guide = true;
        self.logic_ui.topic = topic.into();
    }
    pub(super) fn logic_dialogs(&mut self, ctx: &egui::Context) {
        if self.logic_ui.inputs {
            let mut close = false;
            let response = egui::Modal::new("input_actions".into()).show(ctx, |ui| {
                ui.set_max_width((ctx.content_rect().width() - 48.).clamp(160., 620.));
                ui.heading("Ações de entrada");
                ui.weak(
                    "As ações pertencem ao projeto. Nós e controladores usam os mesmos vínculos.",
                );
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 150.).max(90.))
                    .show(ui, |ui| {
                        for (id, old_key) in self.state.project.input_bindings.clone() {
                            ui.push_id(&id, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    let mut name =
                                        input_actions::label(&self.state.project, &id).into_owned();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut name)
                                                .desired_width(190.),
                                        )
                                        .changed()
                                        && let Err(e) = input_actions::rename(
                                            &mut self.state.project,
                                            &id,
                                            &name,
                                        )
                                    {
                                        self.warn(e);
                                    }
                                    let mut key = old_key.clone();
                                    key_choice(ui, &mut key, &id);
                                    if key != old_key {
                                        self.state.project.input_bindings.insert(id.clone(), key);
                                    }
                                    if ui.small_button("Excluir ação").clicked() {
                                        self.logic_ui.remove = Some(id.clone());
                                    }
                                });
                            });
                        }
                        if let Some(id) = self.logic_ui.remove.clone() {
                            let uses = input_actions::references(&self.state.project, &id);
                            ui.separator();
                            if uses.is_empty() {
                                ui.label("Excluir a ação sem referências?");
                                if ui.button("Confirmar exclusão da ação").clicked() {
                                    match input_actions::remove(&mut self.state.project, &id) {
                                        Ok(()) => self.logic_ui.remove = None,
                                        Err(e) => self.warn(e),
                                    }
                                }
                            } else {
                                ui.label("Altere estes vínculos antes de excluir:");
                                for usage in uses {
                                    ui.label(usage);
                                }
                            }
                            if ui.button("Cancelar exclusão").clicked() {
                                self.logic_ui.remove = None;
                            }
                        }
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.logic_ui.action_name)
                                    .hint_text("Nome da nova ação")
                                    .desired_width(190.),
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
                        if let Some(id) = self.selected.clone()
                            && let Some(mut controller) =
                                self.scene().entity(&id).and_then(|e| e.controller.clone())
                        {
                            ui.separator();
                            ui.strong("Controlador do objeto selecionado");
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
                            for (label, action) in [
                                ("Esquerda", &mut controller.actions.left),
                                ("Direita", &mut controller.actions.right),
                                ("Pular", &mut controller.actions.jump),
                                ("Frente", &mut controller.actions.forward),
                                ("Trás", &mut controller.actions.back),
                            ] {
                                if kind == SceneKind::TwoD && matches!(label, "Frente" | "Trás") {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    egui::ComboBox::from_id_salt(("controller_action", label))
                                        .selected_text(
                                            choices
                                                .iter()
                                                .find(|(id, _)| id == action)
                                                .map_or("Escolha uma ação", |(_, name)| {
                                                    name.as_str()
                                                }),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (id, name) in &choices {
                                                ui.selectable_value(action, id.clone(), name);
                                            }
                                        });
                                });
                            }
                            self.scene_mut().entity_mut(&id).unwrap().controller = Some(controller);
                        }
                    });
                close = ui.button("Fechar ações de entrada").clicked();
            });
            if close || response.should_close() {
                self.logic_ui.inputs = false;
                self.logic_ui.remove = None;
            }
        }
        if self.logic_ui.guide {
            self.guide_ui(ctx);
        }
    }
    fn guide_ui(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut open_recipe = None;
        let response = egui::Modal::new("logic_guide".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 760.));
            ui.heading("Guia de lógica visual");
            ui.add(
                egui::TextEdit::singleline(&mut self.logic_ui.search)
                    .hint_text("Buscar operação ou assunto")
                    .desired_width(ui.available_width()),
            );
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 150.).max(80.))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if ui
                        .selectable_label(self.logic_ui.topic.is_empty(), "Primeiro comportamento")
                        .clicked()
                    {
                        self.logic_ui.topic.clear();
                    }
                    let query = self.logic_ui.search.to_lowercase();
                    ui.horizontal_wrapped(|ui| {
                        for (i, recipe) in oxy_core::guide_recipes::recipes()
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| {
                                query.is_empty()
                                    || format!("{} {}", r.title, r.setup)
                                        .to_lowercase()
                                        .contains(&query)
                            })
                        {
                            let key = format!("recipe:{i}");
                            if ui
                                .selectable_label(self.logic_ui.topic == key, recipe.title)
                                .clicked()
                            {
                                self.logic_ui.topic = key;
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for topic in oxy_core::guide::topics().iter().filter(|t| {
                            query.is_empty()
                                || format!(
                                    "{} {} {}",
                                    t.operation.label, t.operation.category, t.purpose
                                )
                                .to_lowercase()
                                .contains(&query)
                        }) {
                            if ui
                                .selectable_label(
                                    self.logic_ui.topic == topic.operation.id,
                                    topic.operation.label,
                                )
                                .clicked()
                            {
                                self.logic_ui.topic = topic.operation.id.into();
                            }
                        }
                    });
                    ui.separator();
                    if let Some((index, recipe)) = self
                        .logic_ui
                        .topic
                        .strip_prefix("recipe:")
                        .and_then(|v| v.parse::<usize>().ok())
                        .and_then(|i| oxy_core::guide_recipes::recipes().get(i).map(|r| (i, r)))
                    {
                        ui.heading(recipe.title);
                        ui.strong("Preparação");
                        ui.label(recipe.setup);
                        ui.strong("Conexões e valores");
                        ui.label(recipe.flow);
                        ui.strong("Resultado esperado");
                        ui.label(recipe.expected);
                        if ui
                            .button(format!(
                                "Abrir cópia da receita: {}",
                                recipe.title.to_lowercase()
                            ))
                            .clicked()
                        {
                            open_recipe = Some(index);
                        }
                    } else if let Some(topic) = oxy_core::guide::topics()
                        .iter()
                        .find(|t| t.operation.id == self.logic_ui.topic)
                    {
                        ui.heading(topic.operation.label);
                        ui.label(topic.purpose);
                        ui.strong("Preparação e contexto");
                        ui.label(topic.requirements());
                        ui.strong("Entradas e saídas");
                        for (side, ports) in [
                            ("Entrada", &topic.operation.inputs),
                            ("Saída", &topic.operation.outputs),
                        ] {
                            for port in ports {
                                ui.label(format!("{side}: {} · {}", port.label, port.kind.label()));
                            }
                        }
                        ui.strong("Parâmetros");
                        for param in &topic.operation.params {
                            ui.label(param.label);
                        }
                        ui.strong("Exemplo");
                        ui.label(topic.example);
                        ui.strong("Cuidados");
                        ui.label(topic.caution);
                    } else {
                        ui.heading("Seu primeiro comportamento");
                        ui.label(oxy_core::guide::FIRST_BEHAVIOR);
                        if ui
                            .button("Abrir cópia da receita: primeira mensagem")
                            .clicked()
                        {
                            open_recipe = Some(0);
                        }
                        ui.separator();
                        ui.heading("Execução e dados");
                        ui.label(oxy_core::guide::FOUNDATIONS);
                    }
                });
            close = ui.button("Fechar guia").clicked();
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
}
fn key_choice(ui: &mut egui::Ui, key: &mut String, salt: impl std::hash::Hash) {
    egui::ComboBox::from_id_salt(salt).selected_text(crate::labels::key(key)).width(110.).show_ui(ui,|ui| {
        let mut common:Vec<_>=egui::Key::ALL.iter().filter(|k|k.name().len()==1 && k.name().as_bytes()[0].is_ascii_alphanumeric()).collect();
        common.sort_by_key(|k|k.name());
        egui::Grid::new("common_keys").num_columns(8).show(ui,|ui| {
            for (i,candidate) in common.iter().enumerate() {ui.selectable_value(key,candidate.name().into(),candidate.name());if (i+1)%8==0 {ui.end_row();}}
        });
        ui.selectable_value(key,"Space".into(),"Espaço");
        ui.collapsing("Outras teclas",|ui| {for candidate in egui::Key::ALL {
            if !matches!(candidate,egui::Key::Escape|egui::Key::F3) && !common.contains(&candidate) {
                ui.selectable_value(key,candidate.name().into(),crate::labels::key(candidate.name()));
            }
        }});
    }).response.on_hover_text("Tecla que aciona este vínculo. Escape libera o jogo; F3 pertence ao diagnóstico do player.");
}
