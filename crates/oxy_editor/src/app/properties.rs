use super::*;
mod components;

impl Editor {
    fn multiple_properties(&mut self, ui: &mut egui::Ui) {
        let ids = self.selection.ids.clone();
        ui.heading(format!("{} objetos selecionados", ids.len()));
        ui.label("Use os eixos para transformar o conjunto.").on_hover_text("O centro dos objetos selecionados é usado como pivô temporário. Filhos de objetos também selecionados não são transformados duas vezes.");
        let mut visible = ids
            .iter()
            .all(|id| self.scene().entity(id).is_some_and(|e| e.visible));
        if ui.checkbox(&mut visible, "Todos visíveis").changed() {
            for id in &ids {
                if let Some(e) = self.scene_mut().entity_mut(id) {
                    e.visible = visible;
                }
            }
        }
        if self.scene().kind == SceneKind::TwoD {
            let mut layer = self
                .selected
                .as_deref()
                .and_then(|id| self.scene().entity(id))
                .map_or(0, |e| e.layer);
            if ui
                .add(egui::DragValue::new(&mut layer).prefix("Camada "))
                .on_hover_text("Aplica a mesma ordem de desenho a todos os objetos selecionados.")
                .changed()
            {
                for id in &ids {
                    if let Some(e) = self.scene_mut().entity_mut(id) {
                        e.layer = layer;
                    }
                }
            }
        }
        let center = editing::selection_center(self.scene(), &ids);
        let mut position = center.to_array();
        vector3(ui, "Centro do conjunto", &mut position, 0.05, false);
        if Vec3::from(position) != center {
            self.transform_multiple(
                &ids,
                glam::Mat4::from_translation(Vec3::from(position) - center),
            );
        }
        ui.separator();
        vector3(
            ui,
            "Giro em graus",
            &mut self.transform_ui.selection_rotation,
            0.5,
            false,
        );
        if ui.button("Aplicar giro").clicked() {
            let [x, y, z] = self.transform_ui.selection_rotation.map(f32::to_radians);
            let delta = glam::Mat4::from_translation(center)
                * glam::Mat4::from_quat(glam::Quat::from_euler(glam::EulerRot::XYZ, x, y, z))
                * glam::Mat4::from_translation(-center);
            self.transform_multiple(&ids, delta);
            self.transform_ui.selection_rotation = [0.; 3];
        }
        ui.add(
            egui::DragValue::new(&mut self.transform_ui.selection_scale)
                .range(0.01..=100.)
                .speed(0.02)
                .prefix("Fator de escala "),
        );
        if ui.button("Aplicar escala").clicked() {
            self.transform_multiple(
                &ids,
                glam::Mat4::from_translation(center)
                    * glam::Mat4::from_scale(Vec3::splat(self.transform_ui.selection_scale))
                    * glam::Mat4::from_translation(-center),
            );
            self.transform_ui.selection_scale = 1.;
        }
    }
    fn transform_multiple(&mut self, ids: &[Id], delta: glam::Mat4) {
        if let Err(error) = editing::transform_selection(self.scene_mut(), ids, delta) {
            self.log(error);
            self.notice_last(false);
        }
    }
    /// Kind of object, shown under its name in the inspector header.
    fn entity_kind_label(entity: &Entity) -> &'static str {
        if entity.camera.is_some() {
            "Câmera"
        } else if entity.ui.is_some() {
            "Interface do jogo"
        } else if entity.character3d.is_some() || entity.controller.is_some() {
            "Personagem"
        } else if entity.has_geometry() {
            "Peça"
        } else {
            "Grupo"
        }
    }
    fn object_header(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use crate::icons::{self, Icon};
        use crate::theme;
        ui.horizontal(|ui| {
            let (badge, _) = ui.allocate_exact_size(Vec2::splat(28.), Sense::hover());
            ui.painter().rect_filled(badge, 6., theme::ACCENT_SOFT);
            icons::paint(
                ui,
                Rect::from_center_size(badge.center(), Vec2::splat(16.)),
                super::hierarchy::entity_icon(entity),
                theme::ACCENT_BRIGHT,
            );
            let eye_width =
                26. + 2. * ui.spacing().item_spacing.x + 2. * ui.spacing().button_padding.x;
            ui.add(
                egui::TextEdit::singleline(&mut entity.name)
                    .desired_width((ui.available_width() - eye_width).max(40.))
                    .font(egui::TextStyle::Body),
            );
            let (icon, label) = if entity.visible {
                (Icon::Eye, "Visível")
            } else {
                (Icon::EyeOff, "Oculto")
            };
            if icons::small(ui, icon, label, 26.)
                .on_hover_text("Mostra ou oculta a aparência deste objeto e de seus filhos.")
                .clicked()
            {
                entity.visible = !entity.visible;
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(34.);
            let muted = |text: &str| {
                egui::RichText::new(text)
                    .size(12.)
                    .color(theme::TEXT_MUTED)
            };
            ui.label(muted(Self::entity_kind_label(entity)));
            if entity.model_source.is_some() {
                ui.label(muted("· cópia de modelo")).on_hover_text(
                    "Cópia editável de um modelo. Salvar como modelo cria um recurso independente na biblioteca.",
                );
            }
            if self.scene().kind == SceneKind::TwoD {
                ui.label(muted("· Camada"))
                    .on_hover_text("No 2D, valores maiores aparecem na frente de valores menores.");
                ui.add(egui::DragValue::new(&mut entity.layer));
            }
        });
    }
    pub(super) fn properties(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties")
            .default_width((ctx.content_rect().width() * 0.23).clamp(140., 290.))
            .width_range(130.0..=(ctx.content_rect().width() * 0.32).clamp(140., 460.))
            .resizable(true)
            .show(ctx, |ui| {
                if self.snap_panel(ui) {
                    return;
                }
                crate::widgets::panel_header(ui, "Propriedades", |_| {});
                ui.add_space(8.);
                if self.playing() {
                    crate::widgets::hint(
                        ui,
                        "Durante o teste, mudanças não vão para o projeto. Pare o jogo para editar.",
                    );
                    ui.disable();
                }
                egui::ScrollArea::vertical().show(ui, |ui| self.properties_body(ui));
            });
    }
    fn properties_body(&mut self, ui: &mut egui::Ui) {
        use crate::widgets::{card, property_row, vec3_row};
        if self.modeling_active() || self.modeling.preview.is_some() {
            self.mesh_preview_panel(ui);
            if self.modeling.preview.is_some() {
                return;
            }
        }
        if self.mesh_operation_active() {
            ui.disable();
        }
        let Some(id) = self.selected.clone() else {
            ui.heading("Projeto");
            ui.text_edit_singleline(&mut self.state.project.name);
            return;
        };
        if self.selection.ids.len() > 1 {
            self.multiple_properties(ui);
            return;
        }
        let Some(mut entity) = self.scene().entity(&id).cloned() else {
            return;
        };
        let mut requested_pivot = None;
        let mut requested_tool = None;
        let mut requested_fit = None;
        let objects: Vec<_> = self
            .scene()
            .entities
            .iter()
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();
        let textures: Vec<_> = self
            .state
            .project
            .assets
            .iter()
            .filter(|a| a.kind == AssetKind::Texture)
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        self.object_header(ui, &mut entity);
        ui.add_space(8.);
        ui.spacing_mut().item_spacing.y = 6.;
        card(ui, ("transform", &id), "Transformação", None, None, |ui| {
            let mut global = self.view.view_global;
            crate::widgets::segmented(ui, &mut global, &[(false, "Local"), (true, "Global")]);
            self.view.view_global = global;
            let mut transform = if global {
                Transform::from_matrix(
                    self.scene().world_matrix(&id).unwrap_or_default(),
                    entity.transform.pivot,
                )
            } else {
                entity.transform.clone()
            };
            let before = transform.clone();
            vec3_row(
                ui,
                "Posição",
                "Distância da origem nos eixos X, Y e Z.",
                &mut transform.position,
                0.05,
                false,
            );
            let mut degrees = transform.rotation.map(f32::to_degrees);
            let original_degrees = degrees;
            vec3_row(
                ui,
                "Rotação °",
                "Giro em graus nos eixos da peça.",
                &mut degrees,
                0.5,
                false,
            );
            if degrees != original_degrees {
                transform.rotation = degrees.map(f32::to_radians);
            }
            vec3_row(
                ui,
                "Escala",
                "Multiplica o tamanho sem alterar a forma original.",
                &mut transform.scale,
                0.02,
                true,
            );
            if !global {
                let mut pivot = transform.pivot;
                vec3_row(
                    ui,
                    "Pivô local",
                    "Ponto em torno do qual a peça gira e escala. Mudar este ponto preserva a montagem na pose-base.",
                    &mut pivot,
                    0.05,
                    false,
                );
                if pivot != transform.pivot {
                    requested_pivot = Some(pivot);
                }
            }
            if transform != before {
                if global {
                    let parent = entity
                        .parent
                        .as_deref()
                        .and_then(|p| self.scene().world_matrix(p).ok())
                        .unwrap_or(glam::Mat4::IDENTITY);
                    let matrix = parent.inverse() * transform.matrix();
                    let candidate = Transform::from_matrix(matrix, entity.transform.pivot);
                    if parent.determinant().abs() < 1e-8
                        || !candidate.finite()
                        || !candidate.matrix().abs_diff_eq(matrix, 0.0001)
                    {
                        self.log("A transformação global exigiria cisalhamento. Edite em Local ou ajuste a escala não uniforme do pai. A peça foi preservada.");
                        self.notice_last(false);
                    } else {
                        entity.transform = candidate;
                    }
                } else {
                    entity.transform = transform;
                }
            }
            ui.horizontal_wrapped(|ui| {
                if !global && ui.button("Editar pivô (P)").clicked() {
                    requested_tool = Some(Tool::Pivot);
                }
                if ui.button("Espelhar X").clicked() {
                    entity.transform.scale[0] *= -1.;
                }
            });
        });
        if entity.has_geometry() {
            card(ui, ("shape", &id), "Forma e material", None, None, |ui| {
                property_row(ui, "Forma", "", |ui| {
                    ui.label(
                        entity
                            .primitive
                            .map(oxy_render::labels::primitive)
                            .unwrap_or("Malha editável"),
                    );
                    if entity.primitive.is_some() && ui.button("Parâmetros da forma").clicked() {
                        self.edit_primitive_parameters(&entity.id);
                    }
                });
                if entity.mesh.is_none() {
                    vec3_row(
                        ui,
                        "Dimensões",
                        "Tamanho da forma em metros.",
                        &mut entity.dimensions,
                        0.05,
                        true,
                    );
                    entity.dimensions = entity.dimensions.map(|v| v.max(0.0001));
                }
                if let Some(mesh) = &entity.mesh {
                    use oxy_core::geometry::Shading;
                    let mut shading = mesh.data().shading;
                    property_row(
                        ui,
                        "Sombreamento",
                        "Sombreamento da iluminação. Suave não arredonda a silhueta nem altera os polígonos.",
                        |ui| {
                            egui::ComboBox::from_id_salt("mesh_shading")
                                .selected_text(match shading {
                                    Shading::Flat => "Plano",
                                    Shading::Smooth => "Suave",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut shading, Shading::Flat, "Plano");
                                    ui.selectable_value(&mut shading, Shading::Smooth, "Suave");
                                });
                        },
                    );
                    if shading != mesh.data().shading {
                        match mesh.with_shading(shading) {
                            Ok(next) => entity.mesh = Some(next),
                            Err(e) => self.warn(e),
                        }
                    }
                }
                property_row(ui, "Cor base", "", |ui| {
                    color_editor(ui, &mut entity.material.color)
                });
                let selected = entity
                    .material
                    .texture
                    .as_ref()
                    .and_then(|id| {
                        textures
                            .iter()
                            .find(|(i, _)| i == id)
                            .map(|(_, n)| n.as_str())
                    })
                    .unwrap_or("Sem textura");
                property_row(ui, "Textura PNG", "", |ui| {
                    egui::ComboBox::from_id_salt("material_texture")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut entity.material.texture, None, "Sem textura");
                            for (id, name) in &textures {
                                ui.selectable_value(
                                    &mut entity.material.texture,
                                    Some(id.clone()),
                                    name,
                                );
                            }
                        });
                });
                ui.checkbox(&mut entity.material.nearest, "Pixels nítidos")
                    .on_hover_text(
                        "Preserva os pixels de imagens pequenas. Desative para suavizar a textura.",
                    );
                self.texture_controls(ui, &mut entity, false);
            });
        }
        ui.push_id(("object_components", &id), |ui| {
            self.object_components(ui, &mut entity, &mut requested_tool, &mut requested_fit);
        });
        if entity.ui.is_some() {
            card(
                ui,
                ("game_ui", &id),
                "Interface do jogo",
                None,
                None,
                |ui| {
                    let element = entity.ui.as_mut().unwrap();
                    property_row(ui, "Tipo", "", |ui| {
                        egui::ComboBox::from_id_salt("ui_kind")
                            .selected_text(crate::labels::ui_kind(element.kind))
                            .show_ui(ui, |ui| {
                                for kind in
                                    [UiKind::Text, UiKind::Image, UiKind::Button, UiKind::Bar]
                                {
                                    ui.selectable_value(
                                        &mut element.kind,
                                        kind,
                                        crate::labels::ui_kind(kind),
                                    );
                                }
                            });
                    });
                    property_row(ui, "Âncora", "", |ui| {
                        egui::ComboBox::from_id_salt("ui_anchor")
                            .selected_text(crate::labels::anchor(element.anchor))
                            .show_ui(ui, |ui| {
                                for anchor in [
                                    UiAnchor::TopLeft,
                                    UiAnchor::TopRight,
                                    UiAnchor::BottomLeft,
                                    UiAnchor::BottomRight,
                                    UiAnchor::Center,
                                ] {
                                    ui.selectable_value(
                                        &mut element.anchor,
                                        anchor,
                                        crate::labels::anchor(anchor),
                                    );
                                }
                            });
                    });
                    property_row(ui, "Deslocamento", "Em pontos, a partir da âncora.", |ui| {
                        for value in &mut element.position {
                            ui.add(egui::DragValue::new(value));
                        }
                    });
                    property_row(ui, "Tamanho", "", |ui| {
                        for value in &mut element.size {
                            ui.add(egui::DragValue::new(value).range(1.0..=4000.));
                        }
                    });
                    ui.label("Texto ({valor} mostra o vínculo)");
                    ui.text_edit_multiline(&mut element.text);
                    property_row(ui, "Cor", "", |ui| color_editor(ui, &mut element.color));
                    ui.label("Objeto que possui o atributo").on_hover_text(
                    "A interface lê o valor deste objeto, por exemplo Vida do personagem. Sem escolha, usa o próprio objeto.",
                );
                    object_picker(ui, &mut element.binding_object, &objects, "ui_binding");
                    ui.text_edit_singleline(&mut element.binding_attribute);
                    ui.add(
                        egui::DragValue::new(&mut element.max_value)
                            .range(0.01..=1000000.)
                            .prefix("Máximo "),
                    );
                    let label = element
                        .texture
                        .as_ref()
                        .and_then(|id| {
                            textures
                                .iter()
                                .find(|(i, _)| i == id)
                                .map(|(_, n)| n.as_str())
                        })
                        .unwrap_or("Sem imagem");
                    property_row(ui, "Imagem", "", |ui| {
                        egui::ComboBox::from_id_salt("ui_image")
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut element.texture, None, "Sem imagem");
                                for (id, name) in &textures {
                                    ui.selectable_value(
                                        &mut element.texture,
                                        Some(id.clone()),
                                        name,
                                    );
                                }
                            });
                    });
                    self.texture_controls(ui, &mut entity, true);
                },
            );
        }
        card(ui, ("attributes", &id), "Atributos", None, None, |ui| {
            crate::widgets::hint(
                ui,
                "Atributos guardam valores como Vida ou Chave. Eles não fazem nada sozinhos: a Lógica decide como usá-los.",
            );
            let mut delete = None;
            for (name, value) in &mut entity.attributes {
                ui.push_id(name, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(name);
                        if ui
                            .small_button("×")
                            .on_hover_text("Remover atributo")
                            .clicked()
                        {
                            delete = Some(name.clone());
                        }
                    });
                    value_editor(ui, value, &objects, &self.state.project.surfaces, name);
                });
            }
            if let Some(name) = delete {
                entity.attributes.remove(&name);
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.attribute_name).hint_text("Nome do atributo"),
            );
            const TYPES: [&str; 7] = [
                "Número",
                "Texto",
                "Booleano",
                "Objeto",
                "Vetor2",
                "Vetor3",
                "Superfície física",
            ];
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("attrtype")
                    .selected_text(TYPES[self.attribute_type as usize])
                    .show_ui(ui, |ui| {
                        for (i, name) in TYPES.into_iter().enumerate() {
                            ui.selectable_value(&mut self.attribute_type, i as u8, name);
                        }
                    });
                if ui
                    .add_enabled(
                        !self.attribute_name.trim().is_empty(),
                        egui::Button::new("Adicionar atributo"),
                    )
                    .clicked()
                {
                    let value = match self.attribute_type {
                        0 => Value::Number(0.),
                        1 => Value::Text(String::new()),
                        2 => Value::Bool(false),
                        4 => Value::Vector2([0.; 2]),
                        5 => Value::Vector3([0.; 3]),
                        6 => Value::Surface(None),
                        _ => Value::Object(None),
                    };
                    entity
                        .attributes
                        .entry(self.attribute_name.trim().into())
                        .or_insert(value);
                    self.attribute_name.clear();
                }
            });
        });
        if self
            .scene()
            .entity(&id)
            .is_some_and(|e| e.collider != entity.collider)
            && entity.collider.is_some()
        {
            let mut candidate = self.scene().clone();
            candidate.entity_mut(&id).unwrap().collider = entity.collider.clone();
            if let Err(error) = oxy_core::spatial::collider_bounds(&candidate, &id) {
                entity.collider = self.scene().entity(&id).unwrap().collider.clone();
                self.log(error);
                self.notice_last(false);
            }
        }
        if let Some(original) = self.scene_mut().entity_mut(&id) {
            *original = entity;
        }
        if let Some(pivot) = requested_pivot
            && self.structural_ready()
            && let Err(e) = oxy_core::spatial::move_pivot(self.scene_mut(), &id, Vec3::from(pivot))
        {
            self.log(e);
            self.notice_last(false);
        }
        if let Some(tool) = requested_tool {
            self.set_spatial_tool(tool);
        }
        if let Some(children) = requested_fit {
            self.start_fit(children);
        }
    }
}
