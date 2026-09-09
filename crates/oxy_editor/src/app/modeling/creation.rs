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
        let response=egui::Modal::new(egui::Id::new("primitive_creation"))
            .backdrop_color(Color32::TRANSPARENT)
            .area(egui::Area::new(egui::Id::new("primitive_creation_area")).order(egui::Order::Foreground).anchor(egui::Align2::RIGHT_CENTER,Vec2::new(-14.,0.)))
            .show(ctx,|ui|{
                ui.set_width(280f32.min(ctx.content_rect().width()*0.7));
                let kind=creation.candidate.primitive.unwrap();ui.heading(format!("Forma: {}",oxy_render::labels::primitive(kind)));
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-100.).max(120.)).show(ui,|ui|{
                    primitive_values(ui,&mut creation.candidate);
                    let remapping=creation.original.as_ref().is_some_and(|old|old.material.texture.is_some() && (old.segments!=creation.candidate.segments || primitives::Parameters::for_entity(old)!=primitives::Parameters::for_entity(&creation.candidate)));
                    if remapping {ui.colored_label(Color32::LIGHT_YELLOW,"A nova topologia pode mudar onde a pintura aparece. Os pixels do PNG serão preservados.");ui.checkbox(&mut creation.allow_mapping,"Aceitar mudança do mapeamento");}
                    if let Some(error)=&creation.error{ui.colored_label(Color32::LIGHT_YELLOW,error);}
                    ui.horizontal_wrapped(|ui|{confirm=ui.add_enabled(creation.error.is_none()&&(!remapping||creation.allow_mapping),egui::Button::new("Confirmar forma (Enter)")).clicked();cancel=ui.button("Cancelar forma (Esc)").clicked();frame=ui.button("Enquadrar prévia").clicked();});
                    ui.small("Clique fora confirma a forma válida; Esc cancela.");
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
    let kind = entity.primitive.unwrap();
    let mut parameters = primitives::Parameters::for_entity(entity);
    let previous = parameters;
    let mut number = |label: &str, value: &mut f32| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(
                egui::DragValue::new(value)
                    .speed(0.02)
                    .range(0.001..=100_000.),
            );
        });
    };
    match kind {
        Primitive::Cube => {
            for (i, label) in ["Largura X", "Altura Y", "Profundidade Z"]
                .iter()
                .enumerate()
            {
                number(label, &mut entity.dimensions[i]);
            }
        }
        Primitive::Plane => {
            number("Largura", &mut entity.dimensions[0]);
            number("Profundidade", &mut entity.dimensions[2]);
        }
        Primitive::Rectangle | Primitive::Sprite => {
            number("Largura", &mut entity.dimensions[0]);
            number("Altura", &mut entity.dimensions[1]);
        }
        _ => {
            let mut radius = entity.dimensions[0] * 0.5;
            number(
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
                number("Altura", &mut entity.dimensions[1]);
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
        ui.add(
            egui::DragValue::new(&mut entity.segments)
                .range(3..=256)
                .prefix(if kind == Primitive::Sphere {
                    "Longitudes "
                } else {
                    "Lados "
                }),
        );
    }
    if kind == Primitive::Sphere {
        ui.add(
            egui::DragValue::new(&mut parameters.latitude)
                .range(2..=256)
                .prefix("Latitudes "),
        );
    }
    if matches!(
        kind,
        Primitive::Cylinder | Primitive::Cone | Primitive::Tube
    ) {
        ui.add(
            egui::DragValue::new(&mut parameters.height_divisions)
                .range(1..=256)
                .prefix("Divisões de altura "),
        );
    }
    if kind == Primitive::Plane {
        for (i, label) in ["Divisões X ", "Divisões Z "].iter().enumerate() {
            ui.add(
                egui::DragValue::new(&mut parameters.plane_divisions[i])
                    .range(1..=256)
                    .prefix(*label),
            );
        }
    }
    if kind == Primitive::Tube {
        let radius = entity.dimensions[0] * 0.5;
        let mut wall = parameters.wall_fraction * radius;
        ui.add(
            egui::DragValue::new(&mut wall)
                .speed(0.01)
                .range(radius * 0.001..=radius * 0.999)
                .prefix("Espessura da parede "),
        );
        parameters.wall_fraction = wall / radius;
    }
    if parameters != previous {
        entity.primitive_parameters = Some(parameters);
    }
}
