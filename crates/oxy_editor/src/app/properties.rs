use super::*;
mod components;
mod scripts;

impl Editor {
    /// Header with an icon tile, a title and a muted line under it.
    fn panel_title(ui: &mut egui::Ui, icon: crate::icons::Icon, title: &str, detail: &str) {
        use crate::theme;
        ui.horizontal(|ui| {
            let (badge, _) = ui.allocate_exact_size(Vec2::splat(28.), Sense::hover());
            ui.painter().rect_filled(badge, 6., theme::ACCENT_SOFT);
            crate::icons::paint(
                ui,
                Rect::from_center_size(badge.center(), Vec2::splat(16.)),
                icon,
                theme::ACCENT_BRIGHT,
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                ui.add(
                    egui::Label::new(egui::RichText::new(title).font(theme::medium(13.)))
                        .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(detail)
                            .size(12.)
                            .color(theme::TEXT_MUTED),
                    )
                    .truncate(),
                );
            });
        });
        ui.add_space(6.);
    }
    /// Nothing selected: the project itself, and how to start editing an object.
    fn project_properties(&mut self, ui: &mut egui::Ui) {
        use crate::{theme, widgets};
        let name = self.state.project.name.clone();
        Self::panel_title(
            ui,
            crate::icons::Icon::Folder,
            &name,
            "Projeto · nada selecionado",
        );
        widgets::open_card(ui, "project_properties", "Projeto", |ui| {
            widgets::property_row(ui, "Nome", "", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.state.project.name)
                        .desired_width(ui.available_width()),
                );
            });
            widgets::property_row(ui, "Arquivo", "", |ui| {
                let (short, full) = self.path.as_deref().map_or_else(
                    || ("Ainda não salvo".to_owned(), String::new()),
                    |p| (short_project_path(p), p.display().to_string()),
                );
                let response = ui.add(
                    egui::Label::new(
                        egui::RichText::new(short)
                            .monospace()
                            .size(11.5)
                            .color(theme::TEXT_MUTED),
                    )
                    .truncate(),
                );
                if !full.is_empty() {
                    response.on_hover_text(full);
                }
            });
            let start = self
                .state
                .project
                .scene(&self.state.project.start_scene)
                .map(|s| format!("{} · {}", s.name, scenes::kind_label(s.kind)))
                .unwrap_or_default();
            let options: Vec<_> = self
                .state
                .project
                .scenes
                .iter()
                .map(|s| {
                    (
                        s.id.clone(),
                        format!("{} · {}", s.name, scenes::kind_label(s.kind)),
                    )
                })
                .collect();
            let mut chosen = None;
            widgets::combo_row(
                ui,
                "Cena inicial",
                "Cena em que o jogo começa.",
                "project_start_scene",
                &start,
                |ui| {
                    for (id, label) in &options {
                        if ui
                            .selectable_label(*id == self.state.project.start_scene, label)
                            .clicked()
                        {
                            chosen = Some(id.clone());
                        }
                    }
                },
            );
            if let Some(id) = chosen {
                self.state.project.start_scene = id;
            }
            widgets::property_row(ui, "Conteúdo", "", |ui| {
                ui.label(
                    egui::RichText::new({
                        let count = |n: usize, one: &str, many: &str| {
                            format!("{n} {}", if n == 1 { one } else { many })
                        };
                        format!(
                            "{} · {}",
                            count(self.state.project.scenes.len(), "cena", "cenas"),
                            count(self.state.project.assets.len(), "recurso", "recursos")
                        )
                    })
                    .size(12.5),
                );
            });
        });
        ui.add_space(8.);
        self.open_scene_properties(ui);
        ui.add_space(8.);
        egui::Frame::new()
            .stroke(egui::Stroke::new(1., theme::BORDER))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(16, 20))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let (tile, _) = ui.allocate_exact_size(Vec2::splat(36.), Sense::hover());
                    ui.painter().rect_filled(tile, 8., theme::BG_RAISED);
                    crate::icons::paint(
                        ui,
                        Rect::from_center_size(tile.center(), Vec2::splat(18.)),
                        crate::icons::Icon::Object,
                        theme::TEXT_MUTED,
                    );
                    ui.add_space(4.);
                    ui.label(egui::RichText::new("Nada selecionado").font(theme::medium(13.)));
                    widgets::hint(
                        ui,
                        "Selecione um objeto na Hierarquia ou clique nele na cena para editar \
                         posição, forma e componentes.",
                    );
                });
            });
    }
    /// The scene being edited: its name and kind, and what it shows behind its objects.
    fn open_scene_properties(&mut self, ui: &mut egui::Ui) {
        use crate::widgets;
        let scene = self.scene();
        let (kind, current) = (scene.kind, scene.background);
        let title = format!("{} · {}", scene.name, scenes::kind_label(kind));
        let mut chosen = None;
        widgets::open_card(ui, "open_scene_properties", "Cena aberta", |ui| {
            widgets::property_row(ui, "Cena", "", |ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).size(12.5)).truncate());
            });
            chosen = background_rows(ui, kind, current);
            widgets::hint(
                ui,
                "O que aparece atrás de tudo nesta cena: no editor, no teste, no jogo exportado \
                 e nas capturas. Cada cena tem o seu.",
            );
        });
        if let Some(background) = chosen
            && background != current
        {
            self.scene_mut().background = background;
        }
    }
    fn multiple_properties(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{self, Icon};
        use crate::widgets;
        let ids = self.selection.ids.clone();
        let names: Vec<_> = ids
            .iter()
            .filter_map(|id| self.scene().entity(id).map(|e| e.name.clone()))
            .collect();
        let visible = ids
            .iter()
            .all(|id| self.scene().entity(id).is_some_and(|e| e.visible));
        let mut toggle_visible = false;
        ui.horizontal(|ui| {
            let eye = 26. + ui.spacing().item_spacing.x;
            ui.allocate_ui(Vec2::new(ui.available_width() - eye, 34.), |ui| {
                Self::panel_title(
                    ui,
                    Icon::Group,
                    &format!("{} objetos selecionados", ids.len()),
                    &names.join(", "),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let (icon, label) = if visible {
                    (Icon::Eye, "Ocultar todos")
                } else {
                    (Icon::EyeOff, "Mostrar todos")
                };
                toggle_visible = icons::small(ui, icon, label, 26.).clicked();
            });
        });
        if toggle_visible {
            for id in &ids {
                if let Some(e) = self.scene_mut().entity_mut(id) {
                    e.visible = !visible;
                }
            }
        }
        widgets::open_card(ui, "multiple_transform", "Transformar o conjunto", |ui| {
            let center = editing::selection_center(self.scene(), &ids);
            let mut position = center.to_array();
            widgets::vec3_row(
                ui,
                "Centro",
                "O centro dos objetos é o pivô temporário.",
                &mut position,
                0.05,
                false,
            );
            if Vec3::from(position) != center {
                self.transform_multiple(
                    &ids,
                    glam::Mat4::from_translation(Vec3::from(position) - center),
                );
            }
            ui.separator();
            widgets::vec3_row(
                ui,
                "Girar (graus)",
                "",
                &mut self.transform_ui.selection_rotation,
                0.5,
                false,
            );
            let mut rotate = false;
            widgets::property_row(ui, "", "", |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    rotate = ui.button("Aplicar giro").clicked();
                });
            });
            if rotate {
                let [x, y, z] = self.transform_ui.selection_rotation.map(f32::to_radians);
                let delta = glam::Mat4::from_translation(center)
                    * glam::Mat4::from_quat(glam::Quat::from_euler(glam::EulerRot::XYZ, x, y, z))
                    * glam::Mat4::from_translation(-center);
                self.transform_multiple(&ids, delta);
                self.transform_ui.selection_rotation = [0.; 3];
            }
            let mut scale = false;
            widgets::property_row(ui, "Escala (fator)", "", |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    scale = ui.button("Aplicar escala").clicked();
                    ui.add_sized(
                        [ui.available_width(), crate::theme::CONTROL_HEIGHT],
                        egui::DragValue::new(&mut self.transform_ui.selection_scale)
                            .range(0.01..=100.)
                            .speed(0.02),
                    );
                });
            });
            if scale {
                self.transform_multiple(
                    &ids,
                    glam::Mat4::from_translation(center)
                        * glam::Mat4::from_scale(Vec3::splat(self.transform_ui.selection_scale))
                        * glam::Mat4::from_translation(-center),
                );
                self.transform_ui.selection_scale = 1.;
            }
            widgets::hint(
                ui,
                "O centro dos objetos é o pivô temporário. Filhos de objetos também \
                 selecionados não são transformados duas vezes.",
            );
        });
        if self.scene().kind == SceneKind::TwoD {
            widgets::open_card(ui, "multiple_display", "Exibição", |ui| {
                let mut layer = self
                    .selected
                    .as_deref()
                    .and_then(|id| self.scene().entity(id))
                    .map_or(0, |e| e.layer);
                if widgets::property_row(
                    ui,
                    "Camada",
                    "Aplica a mesma ordem de desenho a todos os objetos selecionados.",
                    |ui| {
                        ui.add_sized(
                            [ui.available_width(), crate::theme::CONTROL_HEIGHT],
                            egui::DragValue::new(&mut layer),
                        )
                    },
                )
                .changed()
                {
                    for id in &ids {
                        if let Some(e) = self.scene_mut().entity_mut(id) {
                            e.layer = layer;
                        }
                    }
                }
            });
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
                    if self.play.driver == Driver::Agent {
                        ui.add_space(8.);
                        Self::test_card(
                            ui,
                            "Um agente está jogando",
                            "O agente iniciou este teste pelo MCP e envia as entradas. O mouse \
                             continua livre. Clique em Assumir para jogar você mesmo.",
                            true,
                        );
                    }
                    if self
                        .play
                        .runtime
                        .as_ref()
                        .is_some_and(|rt| rt.game_paused() && !rt.paused)
                    {
                        ui.add_space(8.);
                        // What the game's own pause means, apart from the test's Pausar.
                        Self::test_card(
                            ui,
                            "O jogo pausou a si mesmo",
                            "O menu é do jogo: física, personagens, animações e efeitos param, e a interface continua. O mouse fica livre. Pausar, na barra, pausa o teste inteiro.",
                            false,
                        );
                    }
                    // The test takes the scripts saved while it runs (see script_watch).
                    if self.play.script_watch.is_some() {
                        ui.add_space(8.);
                        Self::test_card(
                            ui,
                            "Recarga dos scripts",
                            "Quando um script muda no disco, o jogo troca o código e continua de \
                             onde estava. Um erro mantém a versão anterior rodando.",
                            false,
                        );
                    }
                    ui.disable();
                }
                egui::ScrollArea::vertical().show(ui, |ui| self.properties_body(ui));
            });
    }
    /// A note about the running test; `agent` draws it in the colours of the agent's chip.
    fn test_card(ui: &mut egui::Ui, title: &str, text: &str, agent: bool) {
        use crate::theme;
        let (fill, border) = if agent {
            (
                Color32::from_rgb(43, 58, 79),
                Color32::from_rgb(60, 83, 116),
            )
        } else {
            (theme::CARD_FILL, theme::CARD_BORDER)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1., border))
            .corner_radius(theme::CARD_RADIUS)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(title)
                        .font(theme::medium(13.))
                        .color(if agent { Color32::WHITE } else { theme::TEXT }),
                );
                if agent {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .size(12.)
                                .color(Color32::from_rgb(183, 210, 245)),
                        )
                        .wrap(),
                    );
                } else {
                    crate::widgets::hint(ui, text);
                }
            });
    }
    fn properties_body(&mut self, ui: &mut egui::Ui) {
        use crate::widgets::{card, property_row, vec3_row};
        let modeling = self.modeling_active() || self.modeling.preview.is_some();
        if self.modeling.preview.is_some() {
            // An operation in progress owns the inspector until it is applied or cancelled.
            if let Some(mut entity) = self
                .selected
                .as_deref()
                .and_then(|id| self.scene().entity(id))
                .cloned()
            {
                ui.add_enabled_ui(false, |ui| self.object_header(ui, &mut entity));
                ui.add_space(8.);
            }
            self.mesh_preview_panel(ui);
            return;
        }
        if self.mesh_operation_active() {
            ui.disable();
        }
        let Some(id) = self.selected.clone() else {
            self.project_properties(ui);
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
        // Studio blocks come first: what the current Studio tool works on. They edit the
        // document directly, so header edits are stored before them and the remaining
        // blocks continue from their result.
        let painting = self.tab == Tab::Studio && self.studio.tab == StudioTab::Paint;
        if modeling || painting {
            if let Some(original) = self.scene_mut().entity_mut(&id) {
                *original = entity;
            }
            if modeling {
                self.mesh_preview_panel(ui);
                self.mesh_info_card(ui);
            }
            if painting {
                self.paint_texture_card(ui);
            }
            let Some(current) = self.scene().entity(&id).cloned() else {
                return;
            };
            entity = current;
        }
        // The transform before this panel's edits: keyframes follow from it.
        let rest = entity.transform.clone();
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
                                .width(crate::widgets::combo_inner_width(ui, ui.available_width()))
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
                        .width(crate::widgets::combo_inner_width(ui, ui.available_width()))
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
                    use crate::widgets::property_row_with;
                    let element = entity.ui.as_mut().unwrap();
                    let column = ui_label_column(ui);
                    property_row_with(ui, column, "Tipo", "", |ui| {
                        egui::ComboBox::from_id_salt("ui_kind")
                            .width(crate::widgets::combo_inner_width(ui, ui.available_width()))
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
                    property_row_with(ui, column, "Âncora", "", |ui| {
                        egui::ComboBox::from_id_salt("ui_anchor")
                            .width(crate::widgets::combo_inner_width(ui, ui.available_width()))
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
                    property_row_with(
                        ui,
                        column,
                        "Deslocamento",
                        "Em pontos, a partir da âncora.",
                        |ui| {
                            for value in &mut element.position {
                                ui.add(egui::DragValue::new(value));
                            }
                        },
                    );
                    property_row_with(ui, column, "Tamanho", "", |ui| {
                        for value in &mut element.size {
                            ui.add(egui::DragValue::new(value).range(1.0..=4000.));
                        }
                    });
                    ui_text_rows(ui, column, element);
                    ui.label("Texto ({valor} mostra o vínculo)");
                    ui.add(
                        egui::TextEdit::multiline(&mut element.text)
                            .desired_width(ui.available_width()),
                    );
                    property_row_with(ui, column, "Cor", "", |ui| {
                        color_editor(ui, &mut element.color)
                    });
                    ui.label("Objeto que possui o atributo").on_hover_text(
                    "A interface lê o valor deste objeto, por exemplo Vida do personagem. Sem escolha, usa o próprio objeto.",
                );
                    object_picker(ui, &mut element.binding_object, &objects, "ui_binding");
                    ui.add(
                        egui::TextEdit::singleline(&mut element.binding_attribute)
                            .desired_width(ui.available_width()),
                    );
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
                    let hint = if element.kind == UiKind::Image {
                        "Sem imagem, a Imagem é um retângulo na Cor, com a transparência dela, como fundo de um menu. Com imagem, a Cor tinge a imagem."
                    } else {
                        ""
                    };
                    property_row_with(ui, column, "Imagem", hint, |ui| {
                        egui::ComboBox::from_id_salt("ui_image")
                            .width(crate::widgets::combo_inner_width(ui, ui.available_width()))
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
        let script_action = self.scripts_card(ui, &entity);
        let requested = self.requested_attributes(&entity);
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
                    if let Some(note) = requested.get(name) {
                        ui.label(
                            egui::RichText::new(note)
                                .size(11.5)
                                .color(crate::theme::TEXT_MUTED),
                        );
                    }
                });
            }
            if let Some(name) = delete {
                entity.attributes.remove(&name);
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.attribute_name)
                    .hint_text("Nome do atributo")
                    .desired_width(ui.available_width()),
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
        editing::follow_tracks(self.scene_mut(), &id, &rest);
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
        if let Some(action) = script_action {
            self.run_script_action(&id, action);
        }
    }
}

/// Fundo of the Cena aberta card, within its width: Céu da OXY (3D scenes only) or Cor and, for
/// a color, the color, picked as the image shows it (sRGB) with no ready-made swatches, and
/// Padrão, the background of a new scene of `kind`. Answers the background chosen, if any.
fn background_rows(
    ui: &mut egui::Ui,
    kind: SceneKind,
    current: Option<Background>,
) -> Option<Option<Background>> {
    use crate::{theme, widgets};
    let mut chosen = None;
    let sky = current == Some(Background::Sky);
    widgets::combo_row(
        ui,
        "Fundo",
        "O que a cena mostra atrás dos objetos.",
        "open_scene_background",
        if sky { "Céu da OXY" } else { "Cor" },
        |ui| {
            if ui
                .add_enabled(
                    kind == SceneKind::ThreeD,
                    egui::Button::selectable(sky, "Céu da OXY"),
                )
                .on_disabled_hover_text("O Céu da OXY é só para cenas 3D.")
                .clicked()
            {
                chosen = Some(Some(Background::Sky));
            }
            // From the sky, the color starts at its horizon.
            if ui.selectable_label(!sky, "Cor").clicked() && sky {
                chosen = Some(Some(Background::Color(oxy_render::sky::HORIZON)));
            }
        },
    );
    if sky {
        return chosen;
    }
    let color = match current {
        Some(Background::Color(color)) => color,
        _ => Background::DARK,
    };
    widgets::property_row(ui, "Cor", "", |ui| {
        let mut srgb =
            [color[0], color[1], color[2]].map(|c| (c.clamp(0., 1.) * 255.).round() as u8);
        if ui.color_edit_button_srgb(&mut srgb).changed() {
            let [r, g, b] = srgb.map(|c| f32::from(c) / 255.);
            chosen = Some(Some(Background::Color([r, g, b, 1.])));
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!("#{:02X}{:02X}{:02X}", srgb[0], srgb[1], srgb[2]))
                    .monospace()
                    .size(11.5)
                    .color(theme::TEXT_MUTED),
            )
            .truncate(),
        );
    });
    let default = Background::default_for(kind);
    widgets::property_row(ui, "", "", |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reset = ui
                .add_enabled(current != default, egui::Button::new("Padrão"))
                .on_hover_text(if kind == SceneKind::ThreeD {
                    "Volta ao Céu da OXY, o fundo das cenas 3D novas."
                } else {
                    "Volta ao fundo escuro das cenas 2D."
                });
            if reset.clicked() {
                chosen = Some(default);
            }
        });
    });
    chosen
}

/// Label column of the Interface do jogo card: wide enough for Tamanho do texto while the card
/// keeps room for its rows of two numbers; narrower, the usual column, and the longer label
/// takes its own line.
fn ui_label_column(ui: &egui::Ui) -> f32 {
    let wide =
        crate::widgets::label_column(ui, &["Deslocamento", "Alinhamento", "Tamanho do texto"]);
    let numbers = 2. * (ui.spacing().interact_size.x + ui.spacing().item_spacing.x);
    if ui.available_width() >= wide + numbers {
        wide
    } else {
        crate::theme::LABEL_COLUMN
    }
}

/// Alinhamento (texts) and Tamanho do texto (texts and button labels) of the Interface do jogo
/// card, within its width: where the px field does not fit beside the choice, it goes under it.
fn ui_text_rows(ui: &mut egui::Ui, column: f32, element: &mut UiElement) {
    use crate::{icons::Icon, widgets};
    if element.kind == UiKind::Text {
        let aligns = [
            (
                UiTextAlign::Left,
                Icon::AlignLeft,
                "Alinhar à esquerda",
                "Alinhar à esquerda",
            ),
            (
                UiTextAlign::Center,
                Icon::AlignCenter,
                "Centralizar",
                "Centralizar",
            ),
            (
                UiTextAlign::Right,
                Icon::AlignRight,
                "Alinhar à direita",
                "Alinhar à direita",
            ),
        ];
        let width = widgets::segmented_icons_width(ui, &aligns.map(|(_, _, l, _)| l), false);
        widgets::property_row_fit(ui, column, "Alinhamento", "", width, |ui| {
            widgets::segmented_icons(ui, &mut element.text_align, &aligns, false);
        });
    }
    if !matches!(element.kind, UiKind::Text | UiKind::Button) {
        return;
    }
    // Switching to px starts from the size drawn now.
    let drawn =
        oxy_core::game_ui::font_size(element, element.size[1].max(1.), element.texture.is_some())
            .unwrap_or(16.);
    let modes = [(false, "Automático"), (true, "px")];
    let mut fixed = element.text_size.is_some();
    let number = ui.spacing().interact_size.x;
    let mut placed = false;
    let width = widgets::segmented_width(ui, &modes.map(|(_, label)| label));
    let beside = widgets::property_row_fit(ui, column, "Tamanho do texto", "", width, |ui| {
        if widgets::segmented(ui, &mut fixed, &modes) {
            element.text_size = fixed.then_some(drawn.round());
        }
        if ui.available_width() >= number
            && let Some(size) = &mut element.text_size
        {
            text_size_field(ui, size);
            placed = true;
        }
    });
    if !placed && let Some(size) = &mut element.text_size {
        if beside {
            widgets::property_row_with(ui, column, "", "", |ui| text_size_field(ui, size));
        } else {
            text_size_field(ui, size);
        }
    }
    let hint = match (element.text_size, element.kind) {
        (Some(_), UiKind::Text) => {
            "De 10 a 96 px, na escala da janela do jogo. Vale também para botões.".into()
        }
        (Some(_), _) => "De 10 a 96 px, na escala da janela do jogo.".into(),
        (None, UiKind::Text) => "Automático: metade da altura do elemento, de 12 a 32 px.".into(),
        (None, _) => format!("Automático: {drawn} px, o padrão dos botões."),
    };
    widgets::hint(ui, &hint);
}

/// The px field of Tamanho do texto; whole pixels read "48" at any interface scale.
fn text_size_field(ui: &mut egui::Ui, size: &mut f32) {
    let field = ui.add(
        egui::DragValue::new(size)
            .range(UiElement::TEXT_SIZES)
            .speed(1.)
            .custom_formatter(|value, _| egui::emath::format_with_decimals_in_range(value, 0..=1)),
    );
    crate::icons::register_qa(ui, "Tamanho do texto em px", field.rect);
}

/// End of a project path, from its folder: the part that tells projects apart.
fn short_project_path(path: &Path) -> String {
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    match path.parent().and_then(Path::file_name) {
        Some(folder) => format!(
            "…{}{}{}{file}",
            std::path::MAIN_SEPARATOR,
            folder.to_string_lossy(),
            std::path::MAIN_SEPARATOR
        ),
        None => file.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_rows_keep_to_the_width_of_the_card() {
        use egui::{Pos2, Rect, Shape, vec2};
        use oxy_core::document::{UiElement, UiTextAlign};
        fn find(shape: &Shape, label: &str) -> Option<Rect> {
            match shape {
                Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut element = UiElement {
            size: [520., 56.],
            text_align: UiTextAlign::Center,
            text_size: Some(48.),
            ..Default::default()
        };
        // The rows laid out `width` points wide: the width they take and where the label, the
        // px choice and the px field are drawn.
        let mut rows = |width: f32| {
            let mut used = 0.;
            let mut output = None;
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
                    ..Default::default()
                };
                output = Some(ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(vec2(width, 500.), |ui| {
                            let column = super::ui_label_column(ui);
                            super::ui_text_rows(ui, column, &mut element);
                            used = ui.min_rect().width();
                        });
                    });
                }));
            }
            let output = output.unwrap();
            let at = |label: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| find(&shape.shape, label))
                    .unwrap_or_else(|| panic!("{label} not drawn {width} points wide"))
            };
            (used, at("Tamanho do texto"), at("px"), at("48"))
        };
        let same_line = |a: Rect, b: Rect| (a.center().y - b.center().y).abs() < 4.;
        // Wide, as in the prototype: one row, past the label column.
        let (used, label, choice, field) = rows(360.);
        assert!(used <= 360.5, "{used}");
        assert!(same_line(label, choice) && same_line(label, field));
        // The default panel: the px field goes under the choice, in the field column.
        let (used, label, choice, field) = rows(260.);
        assert!(used <= 260.5, "{used}");
        assert!(
            same_line(label, choice) && field.top() > label.bottom(),
            "{field:?}"
        );
        assert!(field.left() > label.right(), "{field:?}");
        // Narrow: the label over the choice and the field.
        let (used, label, choice, field) = rows(170.);
        assert!(used <= 170.5, "{used}");
        assert!(choice.top() > label.bottom() && same_line(choice, field));
    }

    #[test]
    fn background_rows_keep_to_the_width_of_the_card() {
        use egui::{Pos2, Rect, Shape, vec2};
        use oxy_core::document::{Background, SceneKind};
        fn drawn(shape: &Shape, label: &str) -> bool {
            match shape {
                Shape::Text(text) => text.galley.text() == label,
                Shape::Vec(shapes) => shapes.iter().any(|shape| drawn(shape, label)),
                _ => false,
            }
        }
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        for (kind, background, hex) in [
            (SceneKind::ThreeD, Some(Background::Sky), None),
            (SceneKind::ThreeD, None, Some("#111112")),
            (
                SceneKind::TwoD,
                Some(Background::Color([0.25, 0.5, 0.75, 1.])),
                Some("#4080BF"),
            ),
        ] {
            for width in [360., 260., 170.] {
                let mut used = 0.;
                let mut output = None;
                for _ in 0..2 {
                    let input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
                        ..Default::default()
                    };
                    output = Some(ctx.run(input, |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.allocate_ui(vec2(width, 500.), |ui| {
                                assert_eq!(super::background_rows(ui, kind, background), None);
                                used = ui.min_rect().width();
                            });
                        });
                    }));
                }
                let shapes = output.unwrap().shapes;
                let shows = |label: &str| shapes.iter().any(|s| drawn(&s.shape, label));
                assert!(
                    used <= width + 0.5,
                    "{kind:?} {background:?}: {used} > {width}"
                );
                assert!(shows("Fundo"));
                // The sky is the default of a 3D scene: no color and nothing to reset.
                assert_eq!(shows("Padrão"), hex.is_some(), "{kind:?} {background:?}");
                if let Some(hex) = hex
                    && width >= 260.
                {
                    assert!(shows(hex), "{hex} at {width}");
                }
            }
        }
    }

    #[test]
    fn background_rows_answer_the_background_clicked() {
        use egui::{Pos2, Rect, Shape, vec2};
        use oxy_core::document::{Background, SceneKind};
        // Every place `label` is drawn, top first.
        fn drawn(output: &egui::FullOutput, label: &str) -> Vec<Pos2> {
            fn find(shape: &Shape, label: &str, found: &mut Vec<Pos2>) {
                match shape {
                    Shape::Text(text) if text.galley.text() == label => {
                        found.push(text.pos + text.galley.size() * 0.5)
                    }
                    Shape::Vec(shapes) => shapes.iter().for_each(|s| find(s, label, found)),
                    _ => {}
                }
            }
            let mut found = Vec::new();
            for shape in &output.shapes {
                find(&shape.shape, label, &mut found);
            }
            found.sort_by(|a, b| a.y.total_cmp(&b.y));
            found
        }
        let frame = |ctx: &egui::Context, kind, current, events| {
            let mut chosen = None;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800., 600.))),
                events,
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(vec2(260., 500.), |ui| {
                        chosen = super::background_rows(ui, kind, current);
                    });
                });
            });
            (output, chosen)
        };
        // A press and a release on `at`: what the rows answered on either.
        let click = |ctx: &egui::Context, kind, current, at: Pos2| {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let events = vec![egui::Event::PointerMoved(at), button(true)];
            let (_, pressed) = frame(ctx, kind, current, events);
            let (_, released) = frame(ctx, kind, current, vec![button(false)]);
            pressed.or(released)
        };
        let color = Some(Background::Color([0.25, 0.5, 0.75, 1.]));
        // Padrão: the sky in a 3D scene, none (the dark color) in a 2D one.
        for (kind, expected) in [
            (SceneKind::ThreeD, Some(Background::Sky)),
            (SceneKind::TwoD, None),
        ] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let (output, _) = frame(&ctx, kind, color, vec![]);
            let reset = drawn(&output, "Padrão")[0];
            assert_eq!(click(&ctx, kind, color, reset), Some(expected), "{kind:?}");
        }
        // From the sky, Cor starts at its horizon.
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let sky = Some(Background::Sky);
        let (output, _) = frame(&ctx, SceneKind::ThreeD, sky, vec![]);
        let combo = drawn(&output, "Céu da OXY")[0];
        assert_eq!(click(&ctx, SceneKind::ThreeD, sky, combo), None);
        let (output, _) = frame(&ctx, SceneKind::ThreeD, sky, vec![]);
        let item = drawn(&output, "Cor")[0];
        assert_eq!(
            click(&ctx, SceneKind::ThreeD, sky, item),
            Some(Some(Background::Color(oxy_render::sky::HORIZON)))
        );
        // In 2D the sky is offered but cannot be chosen.
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let (output, _) = frame(&ctx, SceneKind::TwoD, color, vec![]);
        assert!(drawn(&output, "Céu da OXY").is_empty());
        let combo = drawn(&output, "Cor")[0];
        assert_eq!(click(&ctx, SceneKind::TwoD, color, combo), None);
        let (output, _) = frame(&ctx, SceneKind::TwoD, color, vec![]);
        let item = drawn(&output, "Céu da OXY")[0];
        assert_eq!(click(&ctx, SceneKind::TwoD, color, item), None);
    }

    #[test]
    fn project_path_shows_only_its_folder_and_file() {
        let path = std::path::Path::new("C:/Users/nome/Documents/Jogo/project.oxy.json");
        let separator = std::path::MAIN_SEPARATOR;
        assert_eq!(
            super::short_project_path(path),
            format!("…{separator}Jogo{separator}project.oxy.json")
        );
    }
}
