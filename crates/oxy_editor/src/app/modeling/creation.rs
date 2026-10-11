use super::*;
impl Editor {
    pub(crate) fn create_primitive(&mut self, primitive: Primitive, name: &str) {
        if self.mesh_operation_active() {
            return;
        }
        if !self.studio.animation.drafts.is_empty() || self.studio.playing {
            self.warn("Pause e grave ou descarte a pose provisória antes de criar uma forma.");
            return;
        }
        self.finish_history(true);
        self.history.begin(
            "Criar forma paramétrica",
            &self.state.project,
            &self.state.images,
        );
        let selection = self.selection.clone();
        let selected = self.selected.clone();
        self.add_entity(Some(primitive), name);
        let id = self.selected.clone().unwrap();
        let entity = self.scene_mut().entity_mut(&id).unwrap();
        entity.segments = match primitive {
            Primitive::Pyramid => 4,
            Primitive::Sphere | Primitive::Circle => 16,
            _ => 8,
        };
        entity.primitive_parameters = Some(primitives::Parameters::default());
        let candidate = entity.clone();
        self.modeling.selection = Components::default();
        self.modeling.creation = Some(Creation {
            candidate,
            original: None,
            selection,
            selected,
            error: None,
            allow_mapping: false,
            last_checked: None,
        });
    }
    pub(crate) fn edit_primitive_parameters(&mut self, id: &str) {
        if self.mesh_operation_active() {
            return;
        }
        let Some(entity) = self.scene().entity(id).cloned() else {
            return;
        };
        if entity.mesh.is_some() || entity.primitive.is_none() {
            self.warn("Esta peça não é uma forma paramétrica. Edite seus componentes.");
            return;
        }
        self.finish_history(true);
        self.history.begin(
            "Ajustar forma paramétrica",
            &self.state.project,
            &self.state.images,
        );
        self.modeling.creation = Some(Creation {
            candidate: entity.clone(),
            original: Some(entity),
            selection: self.selection.clone(),
            selected: self.selected.clone(),
            error: None,
            allow_mapping: false,
            last_checked: None,
        });
    }
    pub(crate) fn mesh_creation_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut creation) = self.modeling.creation.take() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let mut frame = false;
        let remapping = creation.original.as_ref().is_some_and(|old| {
            old.material.texture.is_some()
                && (old.segments != creation.candidate.segments
                    || primitives::Parameters::for_entity(old)
                        != primitives::Parameters::for_entity(&creation.candidate))
        });
        let ready = creation.error.is_none() && (!remapping || creation.allow_mapping);
        // Floats over the Properties panel, where the other operations in preview live;
        // a click outside it applies a valid shape.
        let top = crate::theme::TOPBAR_HEIGHT + crate::theme::PANEL_HEADER_HEIGHT + 8.;
        let response = egui::Modal::new(egui::Id::new("primitive_creation"))
            .backdrop_color(Color32::TRANSPARENT)
            .frame(egui::Frame::new())
            .area(
                egui::Area::new(egui::Id::new("primitive_creation_area"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-10., top)),
            )
            .show(ctx, |ui| {
                ui.set_width(300f32.min(ctx.content_rect().width() * 0.7));
                let kind = creation.candidate.primitive.unwrap();
                let title = format!(
                    "{} · {}",
                    if creation.original.is_some() {
                        "Ajustar forma"
                    } else {
                        "Nova forma"
                    },
                    oxy_render::labels::primitive(kind)
                );
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - top - 40.).max(120.))
                    .show(ui, |ui| {
                        crate::widgets::panel_card(
                            ui,
                            crate::icons::Icon::Create,
                            &title,
                            "Em prévia",
                            true,
                            |ui| {
                                primitive_values(ui, &mut creation.candidate);
                                if remapping {
                                    crate::widgets::callout(
                                        ui,
                                        true,
                                        "A nova topologia pode mudar onde a pintura aparece. \
                                         Os pixels do PNG serão preservados.",
                                    );
                                    ui.checkbox(
                                        &mut creation.allow_mapping,
                                        "Aceitar mudança do mapeamento",
                                    );
                                }
                                if let Some(error) = &creation.error {
                                    ui.colored_label(crate::theme::WARNING, error);
                                }
                                match crate::widgets::preview_footer(
                                    ui,
                                    ready,
                                    "Corrija os valores da forma primeiro.",
                                    |ui| {
                                        frame = crate::icons::small(
                                            ui,
                                            crate::icons::Icon::Frame,
                                            "Enquadrar prévia",
                                            26.,
                                        )
                                        .clicked();
                                    },
                                ) {
                                    Some(true) => confirm = true,
                                    Some(false) => cancel = true,
                                    None => {}
                                }
                            },
                        );
                        crate::widgets::hint(
                            ui,
                            "Clicar fora da prévia também aplica uma forma válida.",
                        );
                    });
            });
        let parameters = primitives::Parameters::for_entity(&creation.candidate);
        let kind = creation.candidate.primitive.unwrap();
        let key = (kind, creation.candidate.segments, parameters);
        if creation.last_checked != Some(key) {
            creation.error =
                primitives::generate(kind, creation.candidate.segments, parameters).err();
            creation.last_checked = Some(key);
        }
        let remapping = creation.original.as_ref().is_some_and(|old| {
            old.material.texture.is_some()
                && (old.segments != creation.candidate.segments
                    || primitives::Parameters::for_entity(old) != parameters)
        });
        let valid = creation.error.is_none() && (!remapping || creation.allow_mapping);
        if valid {
            let id = creation.candidate.id.clone();
            if let Some(entity) = self.scene_mut().entity_mut(&id) {
                *entity = creation.candidate.clone();
            }
        }
        self.modeling.creation = Some(creation);
        if frame {
            self.frame_selection();
        }
        if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_mesh_operation();
        } else if valid
            && (confirm
                || response.backdrop_response.clicked()
                || (!response.any_popup_open && ctx.input(|i| i.key_pressed(egui::Key::Enter))))
        {
            self.modeling.creation = None;
            ctx.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
            self.finish_history(true);
        }
    }
}
fn primitive_values(ui: &mut egui::Ui, entity: &mut Entity) {
    use crate::widgets;
    let kind = entity.primitive.unwrap();
    let mut parameters = primitives::Parameters::for_entity(entity);
    let previous = parameters;
    let column = widgets::label_column(ui, &["Divisões de altura", "Espessura da parede"]);
    let field = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
        widgets::property_row_with(ui, column, label, "", |ui| add(ui));
    };
    let number = |ui: &mut egui::Ui, label: &str, value: &mut f32| {
        field(ui, label, &mut |ui| {
            ui.add_sized(
                [ui.available_width(), crate::theme::CONTROL_HEIGHT],
                egui::DragValue::new(value)
                    .speed(0.02)
                    .range(0.001..=100_000.),
            );
        });
    };
    let count = |ui: &mut egui::Ui, label: &str, value: &mut u32, min: u32| {
        field(ui, label, &mut |ui| {
            ui.add_sized(
                [ui.available_width(), crate::theme::CONTROL_HEIGHT],
                egui::DragValue::new(value).range(min..=256),
            );
        });
    };
    match kind {
        Primitive::Cube => {
            for (i, label) in ["Largura X", "Altura Y", "Profundidade Z"]
                .iter()
                .enumerate()
            {
                number(ui, label, &mut entity.dimensions[i]);
            }
        }
        Primitive::Plane => {
            number(ui, "Largura", &mut entity.dimensions[0]);
            number(ui, "Profundidade", &mut entity.dimensions[2]);
        }
        Primitive::Rectangle | Primitive::Sprite => {
            number(ui, "Largura", &mut entity.dimensions[0]);
            number(ui, "Altura", &mut entity.dimensions[1]);
        }
        _ => {
            let mut radius = entity.dimensions[0] * 0.5;
            number(
                ui,
                if kind == Primitive::Tube {
                    "Raio externo"
                } else {
                    "Raio"
                },
                &mut radius,
            );
            if entity.dimensions[0] != radius * 2. {
                entity.dimensions[0] = radius * 2.;
                if kind == Primitive::Sphere {
                    entity.dimensions = [radius * 2.; 3];
                } else if kind == Primitive::Circle {
                    entity.dimensions[1] = radius * 2.;
                } else {
                    entity.dimensions[2] = radius * 2.;
                }
            }
            if !matches!(kind, Primitive::Sphere | Primitive::Circle) {
                number(ui, "Altura", &mut entity.dimensions[1]);
            }
        }
    }
    if matches!(
        kind,
        Primitive::Sphere
            | Primitive::Circle
            | Primitive::Cylinder
            | Primitive::Cone
            | Primitive::Pyramid
            | Primitive::Tube
    ) {
        let label = if kind == Primitive::Sphere {
            "Longitudes"
        } else {
            "Lados"
        };
        count(ui, label, &mut entity.segments, 3);
    }
    if kind == Primitive::Sphere {
        count(ui, "Latitudes", &mut parameters.latitude, 2);
    }
    if matches!(
        kind,
        Primitive::Cylinder | Primitive::Cone | Primitive::Tube
    ) {
        count(
            ui,
            "Divisões de altura",
            &mut parameters.height_divisions,
            1,
        );
    }
    if kind == Primitive::Plane {
        let [x, z] = &mut parameters.plane_divisions;
        count(ui, "Divisões X", x, 1);
        count(ui, "Divisões Z", z, 1);
    }
    if kind == Primitive::Tube {
        let radius = entity.dimensions[0] * 0.5;
        let mut wall = parameters.wall_fraction * radius;
        field(ui, "Espessura da parede", &mut |ui| {
            ui.add_sized(
                [ui.available_width(), crate::theme::CONTROL_HEIGHT],
                egui::DragValue::new(&mut wall)
                    .speed(0.01)
                    .range(radius * 0.001..=radius * 0.999),
            );
        });
        parameters.wall_fraction = wall / radius;
    }
    if parameters != previous {
        entity.primitive_parameters = Some(parameters);
    }
}
