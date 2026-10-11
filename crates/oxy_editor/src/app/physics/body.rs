use super::*;
use crate::widgets::{combo_row, number_row, property_row, sub_section, toggle_row, vec3_row};
use oxy_core::movement_body::MovementBody;

/// Size fields of a movement body shape.
fn dimensions(ui: &mut egui::Ui, shape: &mut CollisionShape) {
    match shape {
        CollisionShape::Capsule { height, radius } => {
            let range = (*radius * 2.)..=100.;
            number_row(ui, "Altura", "", height, range, 0.01, " m");
            let range = 0.01..=(*height * 0.5);
            number_row(ui, "Raio", "", radius, range, 0.01, " m");
        }
        CollisionShape::Box { size } => {
            vec3_row(
                ui,
                "Tamanho",
                "Largura, altura e profundidade em metros.",
                size,
                0.01,
                true,
            );
            *size = size.map(|v| v.clamp(0.01, 100.));
        }
        CollisionShape::Sphere { radius } => {
            number_row(ui, "Raio", "", radius, 0.01..=100., 0.01, " m");
        }
        CollisionShape::Convex { geometry } => {
            property_row(
                ui,
                "Origem",
                "O volume é a envoltória convexa destes pontos. A base é apoiada na origem dos pés.",
                |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Convexo · {} vértices",
                            geometry.vertices.len()
                        ))
                        .size(12.)
                        .color(crate::theme::TEXT_MUTED),
                    );
                },
            );
        }
        CollisionShape::TriMesh { .. } => {
            ui.colored_label(
                crate::theme::WARNING,
                "Malha de triângulos móvel não suportada",
            );
        }
    }
}
impl Editor {
    pub(super) fn movement_body_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        let Some(original) = &entity.character3d else {
            return;
        };
        let mut config = original.clone();
        let mut error = None;
        let mut edit_position = false;
        let pieces: Vec<_> = self
            .scene()
            .entities
            .iter()
            .filter(|e| e.has_geometry())
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();
        let convex_from = |id: &str, scene: &Scene| {
            scene
                .entity(id)
                .ok_or_else(|| "Peça ausente".to_string())
                .and_then(MovementBody::convex_from)
        };
        let scene = self.scene().clone();
        sub_section(ui, "movement_body", "Corpo de movimento", |ui| {
            crate::widgets::hint(
                ui,
                "Colisão de locomoção, independente da aparência. Origem nos pés.",
            );
            ui.add_enabled_ui(!config.enabled, |ui| {
                toggle_row(ui, "Ativo", "Para desativar o corpo, desative primeiro o Personagem 3D. Um personagem ativo precisa de colisão.", &mut config.body.enabled)
            });
            let old = match config.body.standing {
                CollisionShape::Capsule { .. } => 0,
                CollisionShape::Box { .. } => 1,
                CollisionShape::Sphere { .. } => 2,
                _ => 3,
            };
            let mut kind = old;
            combo_row(
                ui,
                "Forma",
                "",
                "movement_body_kind",
                ["Cápsula", "Caixa", "Esfera", "Convexo personalizado"][old],
                |ui| {
                    for (i, label) in ["Cápsula", "Caixa", "Esfera"].iter().enumerate() {
                        ui.selectable_value(&mut kind, i, *label);
                    }
                    ui.separator();
                    ui.menu_button("Convexo de uma peça", |ui| {
                    for (id, name) in &pieces {
                        if ui.button(name).on_hover_text("Gera explicitamente uma envoltória convexa da geometria local. Não copia o material nem altera a peça de origem.").clicked() {
                            match convex_from(id, &scene) {
                                Ok(shape) => {
                                    config.body.standing = shape;
                                    config.body.crouched = None;
                                }
                                Err(e) => error = Some(e),
                            }
                            ui.close();
                        }
                    }
                });
                },
            );
            if kind != old {
                config.body.standing = match kind {
                    1 => CollisionShape::Box {
                        size: [0.6, 1.8, 0.6],
                    },
                    2 => CollisionShape::Sphere { radius: 0.5 },
                    _ => MovementBody::default().standing,
                };
                config.body.crouched = None;
                config.crouch_height = 1.;
            }
            ui.push_id("standing_body", |ui| {
                dimensions(ui, &mut config.body.standing)
            });
            match &config.body.standing {
                CollisionShape::Capsule { height, radius } => {
                    config.crouch_height = config.crouch_height.clamp(radius * 2., *height);
                    let range = radius * 2. ..=*height;
                    number_row(
                        ui,
                        "Agachado",
                        "Altura do corpo agachado.",
                        &mut config.crouch_height,
                        range,
                        0.01,
                        " m",
                    );
                }
                CollisionShape::Box { size } => {
                    config.crouch_height = config.crouch_height.min(size[1]);
                    let range = 0.01..=size[1];
                    number_row(
                        ui,
                        "Agachado",
                        "Altura do corpo agachado.",
                        &mut config.crouch_height,
                        range,
                        0.01,
                        " m",
                    );
                }
                CollisionShape::Sphere { .. } => {
                    let mut alternate = config.body.crouched.is_some();
                    if toggle_row(ui, "Esfera agachada", "Agachar muda o estado e a velocidade. A esfera mantém seu raio, salvo uma forma alternativa explícita.", &mut alternate).changed() {
                        config.body.crouched = alternate.then(|| config.body.standing.clone());
                    }
                }
                CollisionShape::Convex { .. } => {
                    crate::widgets::hint(
                        ui,
                        "Sem convexo alternativo, agachar preserva a geometria.",
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.menu_button("Convexo agachado de…", |ui| {
                            for (id, name) in &pieces {
                                if ui.button(name).clicked() {
                                    match convex_from(id, &scene) {
                                        Ok(shape) => config.body.crouched = Some(shape),
                                        Err(e) => error = Some(e),
                                    }
                                    ui.close();
                                }
                            }
                        });
                        if config.body.crouched.is_some() && ui.button("Remover agachado").clicked()
                        {
                            config.body.crouched = None;
                        }
                    });
                }
                _ => {}
            }
            if let Some(shape) = &mut config.body.crouched {
                ui.push_id("crouched_body", |ui| dimensions(ui, shape));
            }
            let fields = crate::widgets::vec3_fields(
                ui,
                "Deslocamento",
                "Reposiciona somente o corpo físico em relação à origem do personagem. Não move o personagem nem a aparência.",
                &mut config.body.offset,
                0.01,
                false,
            );
            #[cfg(test)]
            for (axis, response) in ["X ", "Y ", "Z "].into_iter().zip(&fields) {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(egui::Id::new(("body_offset_field", axis)), response.rect)
                });
            }
            let _ = fields;
            let editing = self.body_tools.active.as_deref() == Some(&entity.id);
            if ui
                .add_enabled(
                    self.tab == Tab::Scene && self.selection.ids.len() == 1,
                    egui::Button::new(if editing {
                        "Concluir posição do corpo"
                    } else {
                        "Editar posição do corpo"
                    }),
                )
                .clicked()
            {
                edit_position = true;
            }
            number_row(
                ui,
                "Margem",
                "Pequena distância mantida entre o corpo e os obstáculos.",
                &mut config.margin,
                0.0001..=1.,
                0.001,
                " m",
            );
            property_row(
                ui,
                "Bloqueia",
                "O que este corpo impede de atravessar.",
                |ui| {
                    ui.toggle_value(&mut config.body.filter.blocks_character, "Personagens");
                    ui.toggle_value(&mut config.body.filter.blocks_camera, "Câmeras");
                },
            );
            self.surface_row(ui, &mut config.body.surface);
            sub_section(ui, "movement_body_groups", "Grupos de colisão", |ui| {
                number_row(
                    ui,
                    "Categoria",
                    "",
                    &mut config.body.filter.category,
                    0..=u32::MAX,
                    1.,
                    "",
                );
                number_row(
                    ui,
                    "Máscara",
                    "",
                    &mut config.body.filter.mask,
                    0..=u32::MAX,
                    1.,
                    "",
                );
            });
        });
        if edit_position {
            if self.body_tools.active.as_deref() == Some(&entity.id) {
                self.set_spatial_tool(Tool::Object);
            } else {
                self.edit_body_position(&entity.id);
            }
        }
        if let Some(error) = error {
            self.warn(error);
        }
        if entity.character3d.as_ref() != Some(&config) {
            match config.motion(entity, 1.) {
                Ok(_) => entity.character3d = Some(config),
                Err(e) => self.warn(format!("Corpo anterior preservado: {e}")),
            }
        }
    }
}
