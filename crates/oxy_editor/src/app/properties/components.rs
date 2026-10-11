use super::*;
#[cfg(test)]
use oxy_core::authoring::ComponentKind as Component;
use oxy_core::authoring::{add, available};

impl Editor {
    pub(super) fn object_components(
        &mut self,
        ui: &mut egui::Ui,
        entity: &mut Entity,
        tool: &mut Option<Tool>,
        fit: &mut Option<bool>,
    ) {
        let kind = self.scene().kind;
        if let Some(c) = &mut entity.camera {
            let mut active = c.active;
            crate::widgets::card(ui, "camera", "Câmera", Some(&mut active), None, |ui| {
                if kind == SceneKind::TwoD {
                    crate::widgets::number_row(
                        ui,
                        "Meia altura",
                        "Meia altura visível.",
                        &mut c.orthographic_size,
                        0.1..=500.,
                        0.05,
                        " m",
                    );
                } else {
                    crate::widgets::number_row(
                        ui,
                        "Campo de visão",
                        "Campo de visão vertical.",
                        &mut c.fov,
                        10. ..=150.,
                        0.5,
                        "°",
                    );
                }
                crate::widgets::hint(
                    ui,
                    "Ative apenas a câmera desejada. O alvo é configurado no Controle da câmera.",
                );
            });
            c.active = active;
        }
        if kind == SceneKind::ThreeD {
            self.physics_properties(ui, entity);
        }
        if kind == SceneKind::TwoD && entity.camera.is_none() && entity.ui.is_none() {
            if entity.collider.is_some() {
                let mut enabled = entity.collider.as_ref().is_some_and(|c| c.enabled);
                let has_controller = entity.controller.is_some();
                let mut remove = false;
                let mut menu = |ui: &mut egui::Ui| {
                    if ui
                        .add_enabled(!has_controller, egui::Button::new("Remover componente"))
                        .on_disabled_hover_text(
                            "Remova primeiro o Movimento 2D que depende desta caixa.",
                        )
                        .clicked()
                    {
                        remove = true;
                        ui.close();
                    }
                };
                crate::widgets::card(
                    ui,
                    "collider2d",
                    "Colisor 2D",
                    Some(&mut enabled),
                    Some(&mut menu),
                    |ui| {
                        let c = entity.collider.as_mut().unwrap();
                        ui.add_enabled_ui(!has_controller, |ui| {
                            crate::widgets::property_row(
                                ui,
                                "Tipo",
                                "Sólido bloqueia o movimento. Área apenas detecta entradas.",
                                |ui| {
                                    crate::widgets::segmented(
                                        ui,
                                        &mut c.is_trigger,
                                        &[(false, "Sólido"), (true, "Área")],
                                    );
                                },
                            );
                        });
                        crate::widgets::vec3_row(
                            ui,
                            "Tamanho",
                            "Tamanho da caixa em metros.",
                            &mut c.size,
                            0.05,
                            true,
                        );
                        c.size = c.size.map(|v| v.max(0.0001));
                        crate::widgets::vec3_row(
                            ui,
                            "Deslocamento",
                            "Posição da caixa em relação ao objeto.",
                            &mut c.offset,
                            0.05,
                            false,
                        );
                        if ui.button("Editar colisor (C)").clicked() {
                            *tool = Some(Tool::Collider);
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Ajustar ao objeto").clicked() {
                                *fit = Some(false);
                            }
                            if ui.button("Ajustar ao grupo/filhos").clicked() {
                                *fit = Some(true);
                            }
                        });
                    },
                );
                if let Some(c) = &mut entity.collider {
                    c.enabled = enabled;
                }
                if remove {
                    entity.collider = None;
                }
            }
            if entity.controller.is_some() {
                let mut enabled = entity.controller.as_ref().is_some_and(|c| c.enabled);
                let mut remove = false;
                let mut menu = |ui: &mut egui::Ui| {
                    if ui
                        .add_enabled(true, egui::Button::new("Remover componente"))
                        .on_disabled_hover_text("")
                        .clicked()
                    {
                        remove = true;
                        ui.close();
                    }
                };
                crate::widgets::card(
                    ui,
                    "controller2d",
                    "Movimento 2D",
                    Some(&mut enabled),
                    Some(&mut menu),
                    |ui| {
                        let c = entity.controller.as_mut().unwrap();
                        crate::widgets::number_row(
                            ui,
                            "Velocidade",
                            "",
                            &mut c.speed,
                            0. ..=100.,
                            0.1,
                            " m/s",
                        );
                        crate::widgets::number_row(
                            ui,
                            "Pulo",
                            "Impulso do pulo.",
                            &mut c.jump,
                            0. ..=100.,
                            0.1,
                            " m/s",
                        );
                        crate::widgets::number_row(
                            ui,
                            "Gravidade",
                            "",
                            &mut c.gravity,
                            0. ..=200.,
                            0.1,
                            " m/s²",
                        );
                    },
                );
                if let Some(c) = &mut entity.controller {
                    c.enabled = enabled;
                }
                if remove {
                    entity.controller = None;
                }
            }
        }
        let choices = available(kind, entity);
        let size = Vec2::new(ui.available_width(), 30.);
        let add_button = egui::Button::new("+ Adicionar componente")
            .stroke(egui::Stroke::new(1., crate::theme::BORDER))
            .fill(Color32::TRANSPARENT)
            .min_size(size);
        if choices.is_empty() {
            ui.add_enabled(false, add_button).on_disabled_hover_text(
                "Nenhum componente compatível disponível para este objeto.",
            );
        } else {
            let response = ui.add(add_button);
            egui::Popup::menu(&response).show(|ui| {
                for component in choices {
                    if ui.button(component.label()).clicked() {
                        // Validate an explicit authoring command before committing
                        // input bindings or components (e.g. a tilted physical root).
                        let mut project = self.state.project.clone();
                        let mut next = entity.clone();
                        let result = add(kind, &mut project, &mut next, component).and_then(|()| {
                            *project
                                .scene_mut(&self.scene_id)
                                .unwrap()
                                .entity_mut(&next.id)
                                .unwrap() = next.clone();
                            validate_project(&project)
                        });
                        match result {
                            Ok(()) => {
                                self.state.project = project;
                                *entity = next;
                                crate::widgets::request_card_open(ui.ctx(), component.label());
                            }
                            Err(error) => self.warn(error),
                        }
                        ui.close();
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authoring_menu_is_contextual_and_rejects_incompatible_additions() {
        let mut p = Project::new("Autoria");
        let mut e = Entity::new("Peça", Some(Primitive::Cube));
        assert!(!available(SceneKind::ThreeD, &e).contains(&Component::CameraControl));
        assert!(add(SceneKind::ThreeD, &mut p, &mut e, Component::CameraControl).is_err());
        add(SceneKind::ThreeD, &mut p, &mut e, Component::Character).unwrap();
        assert!(!available(SceneKind::ThreeD, &e).contains(&Component::Character));
        assert!(!available(SceneKind::ThreeD, &e).contains(&Component::Platform));
        assert!(e.camera.is_none());
        let mut camera = Entity::new("Câmera", None);
        camera.camera = Some(Camera::default());
        assert_eq!(
            available(SceneKind::ThreeD, &camera),
            vec![Component::CameraControl]
        );
        assert!(add(SceneKind::ThreeD, &mut p, &mut camera, Component::Character).is_err());
        add(
            SceneKind::ThreeD,
            &mut p,
            &mut camera,
            Component::CameraControl,
        )
        .unwrap();
        assert!(available(SceneKind::ThreeD, &camera).is_empty());
    }
}
