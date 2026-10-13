mod body;
use super::*;
use oxy_core::physics3d::{CollisionGeometry, CollisionShape, generate_collider};

#[derive(PartialEq)]
struct VisualKey {
    id: Id,
    revision: Option<u64>,
    primitive: Option<Primitive>,
    dimensions: [f32; 3],
    segments: u32,
    parameters: Option<oxy_core::geometry::primitives::Parameters>,
}
#[derive(Default)]
pub(super) struct PhysicsUi {
    fingerprint: Option<(VisualKey, Option<u64>)>,
    surface_edit: Option<Id>,
}
impl PhysicsUi {
    fn fingerprint(&mut self, entity: &Entity) -> Option<u64> {
        let key = VisualKey {
            id: entity.id.clone(),
            revision: entity.mesh.as_ref().map(|m| m.revision()),
            primitive: entity.primitive,
            dimensions: entity.dimensions,
            segments: entity.segments,
            parameters: entity.primitive_parameters,
        };
        if self.fingerprint.as_ref().is_none_or(|(old, _)| old != &key) {
            self.fingerprint = Some((
                key,
                CollisionGeometry::from_entity(entity)
                    .ok()
                    .and_then(|g| g.source_fingerprint),
            ));
        }
        self.fingerprint
            .as_ref()
            .and_then(|(_, fingerprint)| *fingerprint)
    }
}

/// "Remover componente" menu for a card; `blocked` explains why it is unavailable.
fn remove_menu<'a>(
    remove: &'a mut bool,
    blocked: Option<&'a str>,
) -> impl FnMut(&mut egui::Ui) + 'a {
    move |ui: &mut egui::Ui| {
        let response = ui.add_enabled(blocked.is_none(), egui::Button::new("Remover componente"));
        if let Some(reason) = blocked {
            response.clone().on_disabled_hover_text(reason);
        }
        if response.clicked() {
            *remove = true;
            ui.close();
        }
    }
}

/// Picks an input action by id, showing its readable label.
fn action_row(
    ui: &mut egui::Ui,
    project: &Project,
    label: &str,
    id_salt: &str,
    action: &mut String,
) {
    crate::widgets::combo_row(
        ui,
        label,
        "Ação de entrada que aciona este comando.",
        id_salt,
        &oxy_core::input_actions::label(project, action),
        |ui| {
            for id in project.input_bindings.keys() {
                ui.selectable_value(
                    action,
                    id.clone(),
                    oxy_core::input_actions::label(project, id),
                );
            }
        },
    );
}

impl Editor {
    fn platform_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use crate::widgets::{combo_row, number_row, vec3_row};
        use oxy_core::surface::{PlatformMode, VelocitySpace};
        if entity.platform.is_none() {
            return;
        }
        let mut enabled = entity.platform.as_ref().is_some_and(|p| p.enabled);
        let mut remove = false;
        let mut menu = remove_menu(&mut remove, None);
        crate::widgets::card(
            ui,
            "platform",
            "Plataforma móvel",
            Some(&mut enabled),
            Some(&mut menu),
            |ui| {
                let Some(platform) = &mut entity.platform else {
                    return;
                };
                let animation = platform.mode == PlatformMode::Animation;
                combo_row(
                    ui,
                    "Movimento",
                    "",
                    "platform_mode",
                    if animation {
                        "Seguir animação ou lógica"
                    } else {
                        "Velocidade constante"
                    },
                    |ui| {
                        ui.selectable_value(
                            &mut platform.mode,
                            PlatformMode::Animation,
                            "Seguir animação ou lógica",
                        );
                        ui.selectable_value(
                            &mut platform.mode,
                            PlatformMode::Velocity,
                            "Velocidade constante",
                        );
                    },
                );
                if platform.mode == PlatformMode::Velocity {
                    vec3_row(
                        ui,
                        "Velocidade",
                        "Metros por segundo.",
                        &mut platform.velocity,
                        0.1,
                        false,
                    );
                    let local = platform.space == VelocitySpace::Local;
                    combo_row(
                        ui,
                        "Eixos",
                        "",
                        "platform_space",
                        if local { "Locais" } else { "Da cena" },
                        |ui| {
                            ui.selectable_value(
                                &mut platform.space,
                                VelocitySpace::World,
                                "Da cena",
                            );
                            ui.selectable_value(
                                &mut platform.space,
                                VelocitySpace::Local,
                                "Locais",
                            );
                        },
                    );
                }
                number_row(
                    ui,
                    "Salto máximo",
                    "Deslocamento máximo entre passos. Mudanças maiores soltam o passageiro, sem gerar velocidade de lançamento.",
                    &mut platform.max_transport_per_step,
                    0.001..=10.,
                    0.01,
                    " m",
                );
                crate::widgets::hint(
                    ui,
                    "Transporta quem está em cima, também quando a plataforma gira: o passageiro gira junto, e em primeira pessoa a câmera acompanha. Giros rápidos demais, mudança de escala e saltos de animação soltam o apoio com diagnóstico.",
                );
            },
        );
        if let Some(platform) = &mut entity.platform {
            platform.enabled = enabled;
        }
        drop(menu);
        if remove {
            entity.platform = None;
        }
    }
    /// Surface link with its actions in a "⋯" menu. Editing a surface opens below it.
    fn surface_row(&mut self, ui: &mut egui::Ui, link: &mut Option<Id>) {
        use crate::icons::{self, Icon};
        use oxy_core::surface::{self, SurfaceMaterial, SurfacePreset};
        let name = link
            .as_ref()
            .and_then(|id| self.state.project.surfaces.iter().find(|s| s.id == *id))
            .map_or("Padrão", |s| s.name.as_str())
            .to_owned();
        let surfaces: Vec<_> = self
            .state
            .project
            .surfaces
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let mut action = None;
        crate::widgets::property_row(
            ui,
            "Superfície",
            "Atrito, tração e esteira do contato. Superfícies são compartilhadas entre objetos.",
            |ui| {
                let width = ui.available_width() - 26. - ui.spacing().item_spacing.x;
                crate::widgets::combo(ui, "surface_link", &name, width, |ui| {
                    ui.selectable_value(link, None, "Padrão");
                    for (id, name) in &surfaces {
                        ui.selectable_value(link, Some(id.clone()), name);
                    }
                });
                let more = icons::small(ui, Icon::More, "Ações da superfície", 26.);
                egui::Popup::menu(&more).show(|ui| {
                ui.menu_button("Nova superfície", |ui| {
                    for (preset, label) in [(SurfacePreset::Common, "Comum"), (SurfacePreset::Ice, "Gelo"), (SurfacePreset::Mud, "Lama"), (SurfacePreset::Conveyor, "Esteira")] {
                        if ui.button(label).clicked() {
                            action = Some(("new", Some(preset)));
                            ui.close();
                        }
                    }
                });
                if link.is_some() {
                    if ui.button("Editar superfície").on_hover_text("Afeta todos os objetos vinculados. Use uma cópia para mudar só este.").clicked() {
                        action = Some(("edit", None));
                        ui.close();
                    }
                    if ui.button("Criar cópia independente").clicked() {
                        action = Some(("copy", None));
                        ui.close();
                    }
                    if ui.button("Remover vínculo").clicked() {
                        action = Some(("unlink", None));
                        ui.close();
                    }
                }
            });
            },
        );
        match action {
            Some(("new", Some(preset))) => {
                let surface = SurfaceMaterial::preset(preset);
                *link = Some(surface.id.clone());
                self.physics_ui.surface_edit = Some(surface.id.clone());
                self.state.project.surfaces.push(surface);
            }
            Some(("edit", _)) => self.physics_ui.surface_edit = link.clone(),
            Some(("copy", _)) => {
                if let Some(id) = link.clone() {
                    match surface::duplicate(&mut self.state.project, &id) {
                        Ok(copy) => {
                            self.physics_ui.surface_edit = Some(copy.clone());
                            *link = Some(copy);
                        }
                        Err(e) => self.warn(e),
                    }
                }
            }
            Some(("unlink", _)) => *link = None,
            _ => {}
        }
        if let Some(id) = link.clone() {
            let uses = surface::references(&self.state.project, &id);
            if uses.len() > 1 {
                crate::widgets::hint(ui, &format!("Compartilhada com {} vínculos.", uses.len()))
            }
        }
        if let Some(id) = self.physics_ui.surface_edit.clone()
            && link.as_deref() == Some(id.as_str())
        {
            self.surface_editor(ui, &id, link);
        }
    }
    fn surface_editor(&mut self, ui: &mut egui::Ui, id: &str, link: &Option<Id>) {
        use crate::widgets::{combo_row, number_row, property_row, vec3_row};
        use oxy_core::surface::{self, VelocitySpace};
        let uses = surface::references(&self.state.project, id);
        egui::Frame::new()
            .fill(crate::theme::BG_RAISED)
            .corner_radius(5)
            .inner_margin(6)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(surface) = self.state.project.surfaces.iter_mut().find(|s| s.id == id) {
                    property_row(ui, "Nome", "", |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut surface.name)
                                .desired_width(ui.available_width()),
                        )
                    });
                    number_row(
                        ui,
                        "Atrito",
                        "Multiplicador do atrito do personagem neste chão.",
                        &mut surface.friction,
                        0. ..=100.,
                        0.01,
                        "×",
                    );
                    number_row(
                        ui,
                        "Tração",
                        "Multiplicador da aceleração neste chão.",
                        &mut surface.traction,
                        0. ..=100.,
                        0.01,
                        "×",
                    );
                    number_row(
                        ui,
                        "Velocidade",
                        "Multiplicador da velocidade desejada neste chão.",
                        &mut surface.speed_multiplier,
                        0. ..=100.,
                        0.01,
                        "×",
                    );
                    vec3_row(
                        ui,
                        "Esteira",
                        "Velocidade transmitida pela superfície, em m/s.",
                        &mut surface.conveyor,
                        0.1,
                        false,
                    );
                    let local = surface.conveyor_space == VelocitySpace::Local;
                    combo_row(
                        ui,
                        "Eixos",
                        "",
                        "conveyor_space",
                        if local { "Locais" } else { "Da cena" },
                        |ui| {
                            ui.selectable_value(
                                &mut surface.conveyor_space,
                                VelocitySpace::World,
                                "Da cena",
                            );
                            ui.selectable_value(
                                &mut surface.conveyor_space,
                                VelocitySpace::Local,
                                "Locais",
                            );
                        },
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Concluir").clicked() {
                        self.physics_ui.surface_edit = None;
                    }
                    let can_delete = uses.is_empty() && link.as_deref() != Some(id);
                    if ui
                        .add_enabled(can_delete, egui::Button::new("Excluir superfície"))
                        .on_disabled_hover_text(format!(
                            "Remova os vínculos antes de excluir:\n{}",
                            uses.join("\n")
                        ))
                        .clicked()
                    {
                        match surface::remove(&mut self.state.project, id) {
                            Ok(()) => self.physics_ui.surface_edit = None,
                            Err(e) => self.warn(e),
                        }
                    }
                });
            });
    }
    /// Groups this shape belongs to and interacts with, and the project's group names.
    fn collision_groups(
        &mut self,
        ui: &mut egui::Ui,
        filter: &mut oxy_core::physics3d::CollisionFilter,
    ) {
        crate::widgets::sub_section(ui, "collision_groups", "Grupos de colisão", |ui| {
            crate::graph_ui::collision_filter_picker(
                ui,
                &mut filter.category,
                &self.state.project,
                "Pertence aos grupos…",
            );
            crate::graph_ui::collision_filter_picker(
                ui,
                &mut filter.mask,
                &self.state.project,
                "Interage com grupos…",
            );
            crate::widgets::sub_section(ui, "collision_group_names", "Nomes dos grupos", |ui| {
                crate::widgets::hint(
                    ui,
                    "Renomear organiza as escolhas; não muda vínculos nem colisões.",
                );
                egui::ScrollArea::vertical()
                    .max_height(160.)
                    .show(ui, |ui| {
                        for bit in 0..32_u8 {
                            let mut name = self.state.project.collision_group_label(bit);
                            crate::widgets::property_row(ui, &format!("{}", bit + 1), "", |ui| {
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut name)
                                            .desired_width(ui.available_width()),
                                    )
                                    .changed()
                                    && !name.trim().is_empty()
                                {
                                    self.state.project.collision_groups.insert(bit, name);
                                }
                            });
                        }
                    });
            });
        });
    }
    pub(super) fn physics_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use crate::widgets::{number_row, property_row, vec3_row};
        if entity.camera.is_some() {
            self.camera_properties(ui, entity);
            return;
        }
        if entity.ui.is_some() {
            return;
        }
        self.character_properties(ui, entity);
        self.platform_properties(ui, entity);
        if entity.physics3d.is_none() {
            return;
        }
        let character = entity.character3d.is_some();
        let blocked = if character || entity.platform.is_some() {
            Some("Remova primeiro o Personagem 3D ou a Plataforma móvel que depende desta forma.")
        } else {
            None
        };
        let generate = entity.has_geometry() && !character;
        let fingerprint = if entity.physics3d.as_ref().is_some_and(|c| {
            matches!(
                c.shape,
                CollisionShape::Convex { .. } | CollisionShape::TriMesh { .. }
            )
        }) {
            self.physics_ui.fingerprint(entity)
        } else {
            None
        };
        let dimensions = entity.dimensions;
        let platform = entity.platform.is_some();
        let mut enabled = entity.physics3d.as_ref().is_some_and(|c| c.enabled);
        let mut remove = false;
        let mut regenerate = None;
        let mut menu = remove_menu(&mut remove, blocked);
        crate::widgets::card(
            ui,
            "collider3d",
            "Colisor 3D",
            (!character).then_some(&mut enabled),
            Some(&mut menu),
            |ui| {
                let Some(config) = &mut entity.physics3d else {
                    return;
                };
                const SHAPES: [&str; 5] = [
                    "Caixa",
                    "Cápsula",
                    "Esfera",
                    "Convexo da peça",
                    "Malha estática da peça",
                ];
                let kind = match config.shape {
                    CollisionShape::Box { .. } => 0,
                    CollisionShape::Capsule { .. } => 1,
                    CollisionShape::Sphere { .. } => 2,
                    CollisionShape::Convex { .. } => 3,
                    CollisionShape::TriMesh { .. } => 4,
                };
                let mut choice = kind;
                ui.add_enabled_ui(!character, |ui| {
                crate::widgets::combo_row(ui, "Forma", "Caixa, cápsula e esfera são volumes simples. Convexo envolve a peça e fecha aberturas; a malha estática preserva triângulos e aberturas.", "physics_shape", SHAPES[kind], |ui| {
                    for (i, label) in SHAPES.iter().enumerate() {
                        if i < 3 || generate {
                            ui.selectable_value(&mut choice, i, *label);
                        }
                    }
                });
            });
                if choice != kind {
                    match choice {
                        0 => config.shape = CollisionShape::Box { size: dimensions },
                        1 => {
                            config.shape = CollisionShape::Capsule {
                                height: 1.8,
                                radius: 0.3,
                            }
                        }
                        2 => config.shape = CollisionShape::Sphere { radius: 0.5 },
                        _ => regenerate = Some(choice == 3),
                    }
                }
                match &mut config.shape {
                    CollisionShape::Box { size } => {
                        vec3_row(
                            ui,
                            "Tamanho",
                            "Dimensões da caixa em metros.",
                            size,
                            0.02,
                            true,
                        );
                    }
                    CollisionShape::Capsule { height, radius } => {
                        let range = (*radius * 2.)..=100.;
                        let changed =
                            number_row(ui, "Altura", "", height, range, 0.01, " m").changed();
                        let range = 0.01..=(*height * 0.5);
                        let radius_changed =
                            number_row(ui, "Raio", "", radius, range, 0.01, " m").changed();
                        if let Some(character) = &mut entity.character3d
                            && (changed || radius_changed)
                        {
                            config.center = [0., *height * 0.5, 0.];
                            character.crouch_height =
                                character.crouch_height.clamp(*radius * 2., *height);
                        }
                    }
                    CollisionShape::Sphere { radius } => {
                        number_row(ui, "Raio", "", radius, 0.01..=100., 0.01, " m");
                    }
                    CollisionShape::Convex { geometry } | CollisionShape::TriMesh { geometry } => {
                        property_row(
                            ui,
                            "Origem",
                            "Geometria preparada a partir da aparência da peça.",
                            |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} vértices · {} triângulos",
                                        geometry.vertices.len(),
                                        geometry.triangles.len()
                                    ))
                                    .size(12.)
                                    .color(crate::theme::TEXT_MUTED),
                                );
                            },
                        );
                        if fingerprint.is_some() && fingerprint != geometry.source_fingerprint {
                            ui.colored_label(
                                crate::theme::WARNING,
                                "A aparência mudou; a colisão preparada foi preservada.",
                            );
                            if ui.button("Atualizar geometria de colisão").on_hover_text("Substitui a forma física pela geometria visual atual. Uma operação de desfazer.").clicked() {
                            regenerate = Some(kind == 3);
                        }
                        }
                    }
                }
                ui.add_enabled_ui(!character, |ui| {
                    vec3_row(
                        ui,
                        "Centro",
                        "Posição do volume em relação ao objeto, em metros.",
                        &mut config.center,
                        0.02,
                        false,
                    )
                });
                if character {
                    crate::widgets::hint(
                        ui,
                        "Forma do Personagem 3D: origem nos pés, centro na metade da altura. Use Simular para pausar o controlador.",
                    );
                } else {
                    let mut kind = config.sensor;
                    ui.add_enabled_ui(!platform, |ui| {
                    property_row(ui, "Tipo", "Sólido bloqueia o movimento. Área apenas detecta a passagem, para lógica e sensores.", |ui| {
                        crate::widgets::segmented(ui, &mut kind, &[(false, "Sólido"), (true, "Área")]);
                    });
                });
                    config.sensor = kind;
                }
                property_row(
                    ui,
                    "Bloqueia",
                    "O que esta forma impede de atravessar.",
                    |ui| {
                        ui.toggle_value(&mut config.filter.blocks_character, "Personagem");
                        ui.toggle_value(&mut config.filter.blocks_camera, "Câmera");
                    },
                );
                self.surface_row(ui, &mut config.surface);
                self.collision_groups(ui, &mut config.filter);
                if let Err(error) = config.validate() {
                    ui.colored_label(crate::theme::WARNING, error);
                }
            },
        );
        if let Some(convex) = regenerate {
            match generate_collider(entity, convex) {
                Ok(generated) => {
                    if let Some(current) = &mut entity.physics3d {
                        current.shape = generated.shape;
                    }
                }
                Err(e) => self.warn(e),
            }
        }
        if !character && let Some(config) = &mut entity.physics3d {
            config.enabled = enabled;
        }
        drop(menu);
        if remove {
            entity.physics3d = None;
        }
    }

    fn character_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use crate::widgets::{combo_row, number_row, sub_section, toggle_row};
        use oxy_core::character::{
            BodyFacing, InheritPlatform, JumpMode, MovementProfile, MovementReference,
        };
        if entity.character3d.is_none() || entity.camera.is_some() {
            return;
        }
        let mut enabled = entity.character3d.as_ref().is_some_and(|c| c.enabled);
        let mut remove = false;
        let mut menu = remove_menu(&mut remove, None);
        crate::widgets::card(
            ui,
            "character3d",
            "Personagem 3D",
            Some(&mut enabled),
            Some(&mut menu),
            |ui| {
                self.movement_body_properties(ui, entity);
                let Some(config) = &mut entity.character3d else {
                    return;
                };
                sub_section(ui, "character_basic", "Básico", |ui| {
                    ui.menu_button("Aplicar perfil de movimento", |ui| {
                    for (profile, label) in [(MovementProfile::Direct, "Direto"), (MovementProfile::Parkour, "Parkour"), (MovementProfile::ChainedJumps, "Saltos encadeados")] {
                        if ui.button(label).on_hover_text("Aplica valores iniciais editáveis. Preserva ações, vínculos e geometria; pode ser desfeito.").clicked() {
                            config.apply_profile(profile);
                            ui.close();
                        }
                    }
                });
                    toggle_row(
                        ui,
                        "Entrada automática",
                        "Lê as ações de movimento sozinho. Desative para enviar intenção e pulos por nós.",
                        &mut config.automatic_input,
                    );
                    let reference = match config.reference {
                        MovementReference::World => "Eixos da cena",
                        MovementReference::Body => "Corpo",
                        MovementReference::Camera => "Câmera",
                    };
                    combo_row(
                        ui,
                        "Direção",
                        "Referência das teclas de movimento.",
                        "movement_reference",
                        reference,
                        |ui| {
                            ui.selectable_value(
                                &mut config.reference,
                                MovementReference::World,
                                "Eixos da cena",
                            );
                            ui.selectable_value(
                                &mut config.reference,
                                MovementReference::Body,
                                "Corpo",
                            );
                            ui.selectable_value(
                                &mut config.reference,
                                MovementReference::Camera,
                                "Câmera",
                            );
                        },
                    );
                    number_row(
                        ui,
                        "Gravidade",
                        "",
                        &mut config.gravity,
                        0. ..=200.,
                        0.1,
                        " m/s²",
                    );
                });
                sub_section(ui, "character_ground", "Movimento no chão", |ui| {
                    number_row(
                        ui,
                        "Velocidade",
                        "Velocidade desejada. Não elimina impulsos externos.",
                        &mut config.speed,
                        0. ..=100.,
                        0.1,
                        " m/s",
                    );
                    number_row(
                        ui,
                        "Aceleração",
                        "",
                        &mut config.ground_acceleration,
                        0. ..=1000.,
                        0.1,
                        " m/s²",
                    );
                    number_row(
                        ui,
                        "Frenagem",
                        "",
                        &mut config.ground_braking,
                        0. ..=1000.,
                        0.1,
                        " m/s²",
                    );
                    number_row(
                        ui,
                        "Atrito",
                        "Perda de velocidade por segundo.",
                        &mut config.ground_friction,
                        0. ..=100.,
                        0.1,
                        " /s",
                    );
                });
                sub_section(ui, "character_jump", "Pulo", |ui| {
                    number_row(
                        ui,
                        "Impulso",
                        "",
                        &mut config.jump_speed,
                        0. ..=100.,
                        0.1,
                        " m/s",
                    );
                    let manual = config.jump_mode == JumpMode::Manual;
                    combo_row(
                        ui,
                        "Modo",
                        "Manual exige novo toque. Automático repete ao encontrar chão.",
                        "jump_mode",
                        if manual {
                            "Manual"
                        } else {
                            "Automático ao segurar"
                        },
                        |ui| {
                            ui.selectable_value(&mut config.jump_mode, JumpMode::Manual, "Manual");
                            ui.selectable_value(
                                &mut config.jump_mode,
                                JumpMode::Automatic,
                                "Automático ao segurar",
                            );
                        },
                    );
                    number_row(
                        ui,
                        "Tolerância",
                        "Tempo para ainda pular depois de sair da borda. Zero desativa.",
                        &mut config.coyote_ms,
                        0. ..=1000.,
                        1.,
                        " ms",
                    );
                    number_row(
                        ui,
                        "Antecipação",
                        "Tempo em que um pulo pedido antes de pousar ainda vale. Zero desativa.",
                        &mut config.jump_buffer_ms,
                        0. ..=1000.,
                        1.,
                        " ms",
                    );
                });
                sub_section(ui, "character_air", "Movimento no ar", |ui| {
                    number_row(
                        ui,
                        "Aceleração",
                        "Aceleração aérea.",
                        &mut config.air_acceleration,
                        0. ..=1000.,
                        0.1,
                        " m/s²",
                    );
                    number_row(
                        ui,
                        "Limite direcional",
                        "Limita só o acréscimo na direção desejada; a velocidade perpendicular é preservada.",
                        &mut config.air_projected_limit,
                        0. ..=1000.,
                        0.1,
                        " m/s",
                    );
                    number_row(
                        ui,
                        "Resistência",
                        "Resistência horizontal do ar.",
                        &mut config.air_resistance,
                        0. ..=100.,
                        0.1,
                        "",
                    );
                    number_row(
                        ui,
                        "Limite horizontal",
                        "Limite absoluto; zero desliga.",
                        &mut config.horizontal_limit,
                        0. ..=1000.,
                        0.1,
                        " m/s",
                    );
                    number_row(
                        ui,
                        "Conservar ao saltar",
                        "1 conserva toda a velocidade; 0 elimina o embalo horizontal.",
                        &mut config.jump_retention,
                        0. ..=1.,
                        0.01,
                        "",
                    );
                    number_row(
                        ui,
                        "Conservar ao pousar",
                        "Sem re-salto. 1 conserva toda a velocidade; 0 elimina o embalo.",
                        &mut config.landing_retention,
                        0. ..=1.,
                        0.01,
                        "",
                    );
                    number_row(
                        ui,
                        "Proteção numérica",
                        "Limite final, separado da velocidade de caminhar e do controle aéreo.",
                        &mut config.absolute_speed_limit,
                        1. ..=10000.,
                        1.,
                        " m/s",
                    );
                });
                sub_section(ui, "character_sprint", "Corrida", |ui| {
                    number_row(
                        ui,
                        "Velocidade",
                        "",
                        &mut config.sprint_speed,
                        0. ..=100.,
                        0.1,
                        " m/s",
                    );
                    action_row(
                        ui,
                        &self.state.project,
                        "Ação",
                        "sprint_action",
                        &mut config.sprint_action,
                    );
                });
                sub_section(ui, "character_crouch", "Agachamento/deslize", |ui| {
                    number_row(
                        ui,
                        "Velocidade",
                        "Velocidade agachado.",
                        &mut config.crouch_speed,
                        0. ..=100.,
                        0.1,
                        " m/s",
                    );
                    toggle_row(
                        ui,
                        "Alternar",
                        "Cada toque alterna entre agachado e em pé.",
                        &mut config.crouch_toggle,
                    );
                    action_row(
                        ui,
                        &self.state.project,
                        "Ação",
                        "crouch_action",
                        &mut config.crouch_action,
                    );
                    toggle_row(
                        ui,
                        "Deslize",
                        "Agachar com embalo inicia um deslize.",
                        &mut config.slide_enabled,
                    );
                    ui.add_enabled_ui(config.slide_enabled, |ui| {
                        let exit = config.slide_exit_speed;
                        number_row(
                            ui,
                            "Iniciar acima de",
                            "",
                            &mut config.slide_min_speed,
                            exit..=1000.,
                            0.1,
                            " m/s",
                        );
                        let min = config.slide_min_speed;
                        number_row(
                            ui,
                            "Encerrar abaixo de",
                            "",
                            &mut config.slide_exit_speed,
                            0. ..=min,
                            0.1,
                            " m/s",
                        );
                        number_row(
                            ui,
                            "Duração máxima",
                            "",
                            &mut config.slide_duration,
                            0.01..=60.,
                            0.05,
                            " s",
                        );
                        number_row(
                            ui,
                            "Atrito",
                            "Atrito do deslize.",
                            &mut config.slide_friction,
                            0. ..=100.,
                            0.1,
                            "",
                        );
                        number_row(
                            ui,
                            "Controle",
                            "Controle direcional durante o deslize.",
                            &mut config.slide_control,
                            0. ..=100.,
                            0.1,
                            " m/s²",
                        );
                    });
                    crate::widgets::hint(
                        ui,
                        "Atrito e tração do piso continuam valendo. Sob teto, o personagem permanece agachado.",
                    );
                });
                sub_section(ui, "character_advanced", "Avançado", |ui| {
                    number_row(
                        ui,
                        "Recuperação",
                        "Correção máxima quando um obstáculo entra no personagem.",
                        &mut config.recovery_distance,
                        0. ..=10.,
                        0.01,
                        " m",
                    );
                    number_row(
                        ui,
                        "Rampa máxima",
                        "",
                        &mut config.slope_degrees,
                        0.001..=89.,
                        0.1,
                        "°",
                    );
                    number_row(
                        ui,
                        "Degrau máximo",
                        "",
                        &mut config.step_height,
                        0.001..=2.,
                        0.01,
                        " m",
                    );
                    number_row(
                        ui,
                        "Largura mínima",
                        "Largura mínima de um degrau.",
                        &mut config.step_width,
                        0.001..=2.,
                        0.01,
                        " m",
                    );
                    number_row(
                        ui,
                        "Aderência",
                        "Distância em que o personagem se mantém colado ao chão.",
                        &mut config.snap,
                        0.001..=2.,
                        0.01,
                        " m",
                    );
                    let inherit = match config.inherit_platform {
                        InheritPlatform::None => "Nenhuma",
                        InheritPlatform::Horizontal => "Horizontal",
                        InheritPlatform::All => "Toda",
                    };
                    combo_row(
                        ui,
                        "Herança",
                        "Velocidade do apoio somada uma vez ao pular ou sair da plataforma.",
                        "platform_inheritance",
                        inherit,
                        |ui| {
                            ui.selectable_value(
                                &mut config.inherit_platform,
                                InheritPlatform::None,
                                "Nenhuma",
                            );
                            ui.selectable_value(
                                &mut config.inherit_platform,
                                InheritPlatform::Horizontal,
                                "Horizontal",
                            );
                            ui.selectable_value(
                                &mut config.inherit_platform,
                                InheritPlatform::All,
                                "Toda",
                            );
                        },
                    );
                });
                sub_section(ui, "character_facing", "Orientação", |ui| {
                    let facing = if config.facing == BodyFacing::Movement {
                        "Movimento"
                    } else {
                        "Olhar"
                    };
                    combo_row(
                        ui,
                        "Virar para",
                        "Na primeira pessoa, o corpo acompanha o olhar horizontal.",
                        "character_facing",
                        facing,
                        |ui| {
                            ui.selectable_value(
                                &mut config.facing,
                                BodyFacing::Movement,
                                "Movimento",
                            );
                            ui.selectable_value(&mut config.facing, BodyFacing::Look, "Olhar");
                        },
                    );
                    number_row(
                        ui,
                        "Giro",
                        "Zero mantém a orientação do corpo.",
                        &mut config.angular_speed,
                        0. ..=3600.,
                        5.,
                        "°/s",
                    );
                });
            },
        );
        if let Some(config) = &mut entity.character3d {
            config.enabled = enabled;
        }
        drop(menu);
        if remove {
            entity.character3d = None;
        }
    }
    fn camera_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use crate::widgets::{combo_row, number_row, sub_section, toggle_row};
        use oxy_core::character::CameraMode;
        if entity.camera.is_none() || entity.camera_rig.is_none() {
            return;
        }
        let mut remove = false;
        let mut menu = remove_menu(&mut remove, None);
        let targets: Vec<_> = self
            .scene()
            .entities
            .iter()
            .filter(|e| e.id != entity.id)
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();
        crate::widgets::card(
            ui,
            "camera_rig",
            "Controle da câmera",
            None,
            Some(&mut menu),
            |ui| {
                if ui.button("Prévia da câmera").clicked() {
                    self.camera_tools.preview = true;
                }
                let Some(rig) = &mut entity.camera_rig else {
                    return;
                };
                if rig.mode != CameraMode::Fixed {
                    crate::widgets::hint(ui, "Durante o jogo, a pose é controlada pelo alvo.");
                }
                sub_section(ui, "camera_basic", "Básico", |ui| {
                    let mode = match rig.mode {
                        CameraMode::FirstPerson => "Primeira pessoa",
                        CameraMode::ThirdPerson => "Terceira pessoa",
                        CameraMode::Fixed => "Câmera fixa",
                    };
                    combo_row(ui, "Modo", "", "camera_mode", mode, |ui| {
                        ui.selectable_value(
                            &mut rig.mode,
                            CameraMode::FirstPerson,
                            "Primeira pessoa",
                        );
                        ui.selectable_value(
                            &mut rig.mode,
                            CameraMode::ThirdPerson,
                            "Terceira pessoa",
                        );
                        ui.selectable_value(&mut rig.mode, CameraMode::Fixed, "Câmera fixa");
                    });
                    let target = rig
                        .target
                        .as_ref()
                        .and_then(|id| {
                            targets
                                .iter()
                                .find(|(t, _)| t == id)
                                .map(|(_, name)| name.as_str())
                        })
                        .unwrap_or("Escolha o personagem");
                    combo_row(
                        ui,
                        "Alvo",
                        "Objeto acompanhado por esta câmera. O vínculo independe do parentesco na Hierarquia.",
                        "rig_target",
                        target,
                        |ui| {
                            ui.selectable_value(&mut rig.target, None, "Sem alvo");
                            for (id, name) in &targets {
                                ui.selectable_value(&mut rig.target, Some(id.clone()), name);
                            }
                        },
                    );
                });
                sub_section(ui, "camera_first", "Primeira pessoa", |ui| {
                    number_row(
                        ui,
                        "Olhos",
                        "Altura dos olhos.",
                        &mut rig.eye_height,
                        0. ..=100.,
                        0.01,
                        " m",
                    );
                    number_row(
                        ui,
                        "Olhos agachado",
                        "",
                        &mut rig.crouched_eye_height,
                        0. ..=100.,
                        0.01,
                        " m",
                    );
                    number_row(
                        ui,
                        "Transição",
                        "Tempo da troca de postura.",
                        &mut rig.posture_smoothing,
                        0. ..=2.,
                        0.01,
                        " s",
                    );
                });
                sub_section(ui, "camera_look", "Olhar", |ui| {
                    number_row(
                        ui,
                        "Sensibilidade",
                        "Graus por unidade relativa do mouse. Não depende da taxa de quadros nem da escala da interface.",
                        &mut rig.sensitivity,
                        0.001..=10.,
                        0.005,
                        "",
                    );
                    toggle_row(
                        ui,
                        "Inverter X",
                        "Inverte o olhar horizontal.",
                        &mut rig.invert_x,
                    );
                    toggle_row(
                        ui,
                        "Inverter Y",
                        "Inverte o olhar vertical.",
                        &mut rig.invert_y,
                    );
                    number_row(
                        ui,
                        "Limite vertical",
                        "",
                        &mut rig.pitch_limit,
                        1. ..=89.,
                        0.5,
                        "°",
                    );
                });
                sub_section(ui, "camera_third", "Terceira pessoa", |ui| {
                    let (min, max) = (rig.min_distance, rig.max_distance);
                    number_row(
                        ui,
                        "Distância",
                        "A câmera recolhe diante de paredes. A roda ajusta a distância quando não está vinculada a uma ação do jogo.",
                        &mut rig.distance,
                        min..=max,
                        0.05,
                        " m",
                    );
                    let distance = rig.distance;
                    number_row(
                        ui,
                        "Mínima",
                        "",
                        &mut rig.min_distance,
                        0. ..=distance,
                        0.05,
                        " m",
                    );
                    number_row(
                        ui,
                        "Máxima",
                        "",
                        &mut rig.max_distance,
                        distance..=1000.,
                        0.05,
                        " m",
                    );
                    number_row(
                        ui,
                        "Ombro",
                        "Deslocamento lateral do ombro.",
                        &mut rig.shoulder,
                        -10. ..=10.,
                        0.02,
                        " m",
                    );
                    number_row(
                        ui,
                        "Altura",
                        "Altura acompanhada.",
                        &mut rig.follow_height,
                        0. ..=100.,
                        0.02,
                        " m",
                    );
                    number_row(
                        ui,
                        "Zoom por passo",
                        "Ajuste da roda do mouse.",
                        &mut rig.zoom_step,
                        0. ..=10.,
                        0.05,
                        " m",
                    );
                    let hint = "Zero aplica imediatamente. A proteção contra obstáculos tem prioridade sobre a suavização.";
                    number_row(
                        ui,
                        "Suavizar posição",
                        hint,
                        &mut rig.position_smoothing,
                        0. ..=5.,
                        0.01,
                        " s",
                    );
                    number_row(
                        ui,
                        "Suavizar rotação",
                        hint,
                        &mut rig.rotation_smoothing,
                        0. ..=5.,
                        0.01,
                        " s",
                    );
                    number_row(
                        ui,
                        "Retorno",
                        "Retorno após obstáculo.",
                        &mut rig.obstruction_return,
                        0. ..=5.,
                        0.01,
                        " s",
                    );
                    number_row(
                        ui,
                        "Troca de modo",
                        "",
                        &mut rig.transition_seconds,
                        0. ..=5.,
                        0.01,
                        " s",
                    );
                    action_row(
                        ui,
                        &self.state.project,
                        "Trocar ombro",
                        "camera_shoulder_action",
                        &mut rig.shoulder_action,
                    );
                    action_row(
                        ui,
                        &self.state.project,
                        "Alternar modo",
                        "camera_mode_action",
                        &mut rig.mode_action,
                    );
                });
                sub_section(
                    ui,
                    "camera_protection",
                    "Proteção contra obstáculos",
                    |ui| {
                        let margin = rig.collision_margin;
                        number_row(
                            ui,
                            "Raio mínimo",
                            "",
                            &mut rig.collision_radius,
                            margin.max(0.01) + 0.001..=5.,
                            0.01,
                            " m",
                        );
                        let radius = rig.collision_radius;
                        number_row(
                            ui,
                            "Folga",
                            "Folga de proteção.",
                            &mut rig.collision_margin,
                            0.001..=radius - 0.001,
                            0.001,
                            " m",
                        );
                        crate::widgets::hint(
                            ui,
                            "O volume também protege os cantos próximos da imagem. Colisores podem permitir ou impedir a câmera.",
                        );
                    },
                );
                sub_section(ui, "camera_hidden", "Ocultar peças nesta câmera", |ui| {
                    toggle_row(
                        ui,
                        "Só em 1ª pessoa",
                        "Oculta as peças abaixo apenas em primeira pessoa.",
                        &mut rig.hide_first_person_only,
                    );
                    for (id, name) in &targets {
                        let mut hidden = rig.hidden.contains(id);
                        if ui.checkbox(&mut hidden, name).changed() {
                            if hidden {
                                rig.hidden.push(id.clone());
                            } else {
                                rig.hidden.retain(|v| v != id);
                            }
                        }
                    }
                });
                crate::widgets::hint(
                    ui,
                    "Jogar/Retomar captura o mouse. Esc ou abrir a Lógica libera a entrada.",
                );
            },
        );
        drop(menu);
        if remove {
            entity.camera_rig = None;
        }
    }
}
