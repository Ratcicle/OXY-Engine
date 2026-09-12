use super::*;
use oxy_core::{
    character::{CameraRig, CharacterConfig},
    physics3d::{Collider3d, CollisionShape},
    surface::TranslationPlatform,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Component {
    Character,
    Collider3d,
    Sensor3d,
    Platform,
    CameraControl,
    Collider2d,
    Sensor2d,
    Controller2d,
}
impl Component {
    fn label(self) -> &'static str {
        match self {
            Self::Character => "Personagem 3D",
            Self::Collider3d => "Colisor 3D",
            Self::Sensor3d => "Área de detecção 3D",
            Self::Platform => "Plataforma móvel",
            Self::CameraControl => "Controle da câmera",
            Self::Collider2d => "Colisor 2D",
            Self::Sensor2d => "Área de detecção 2D",
            Self::Controller2d => "Movimento 2D",
        }
    }
}
// This is an authoring policy, not a migration or a change to old game behavior.
// Existing 3D legacy data remains readable; it never enters the creation menu.
fn available(kind: SceneKind, e: &Entity) -> Vec<Component> {
    if e.camera.is_some() {
        return if kind == SceneKind::ThreeD && e.camera_rig.is_none() {
            vec![Component::CameraControl]
        } else {
            vec![]
        };
    }
    if e.ui.is_some() {
        return vec![];
    }
    if kind == SceneKind::TwoD {
        let mut entries = Vec::new();
        if e.collider.is_none() {
            entries.extend([Component::Collider2d, Component::Sensor2d]);
        }
        if e.controller.is_none() && e.collider.as_ref().is_none_or(|c| !c.is_trigger) {
            entries.push(Component::Controller2d);
        }
        return entries;
    }
    let mut entries = Vec::new();
    if e.controller.is_none() && e.collider.is_none() {
        if e.character3d.is_none() && e.platform.is_none() && e.physics3d.is_none() {
            entries.push(Component::Character);
        }
        if e.physics3d.is_none() && e.character3d.is_none() {
            entries.extend([Component::Collider3d, Component::Sensor3d]);
        }
        if e.platform.is_none()
            && e.character3d.is_none()
            && e.physics3d.as_ref().is_none_or(|c| !c.sensor)
        {
            entries.push(Component::Platform);
        }
    }
    entries
}
fn add(
    kind: SceneKind,
    project: &mut Project,
    e: &mut Entity,
    component: Component,
) -> Result<(), String> {
    if !available(kind, e).contains(&component) {
        return Err("Componente incompatível ou já presente neste objeto.".into());
    }
    match component {
        Component::Character => {
            let config = CharacterConfig::default();
            oxy_core::input_actions::ensure_character(project, &config);
            e.character3d = Some(config);
        }
        Component::CameraControl => {
            let rig = CameraRig::default();
            oxy_core::input_actions::ensure_camera(project, &rig);
            e.camera_rig = Some(rig);
        }
        Component::Collider3d | Component::Sensor3d | Component::Platform => {
            e.physics3d.get_or_insert_with(|| Collider3d {
                shape: CollisionShape::Box { size: e.dimensions },
                sensor: component == Component::Sensor3d,
                ..Default::default()
            });
            if component == Component::Platform {
                e.platform = Some(TranslationPlatform::default());
            }
        }
        Component::Collider2d | Component::Sensor2d => {
            e.collider = Some(Collider {
                size: e.dimensions,
                is_trigger: component == Component::Sensor2d,
                ..Default::default()
            })
        }
        Component::Controller2d => {
            let controller = Controller::default();
            oxy_core::input_actions::ensure_controller(project, &controller.actions, kind);
            e.collider.get_or_insert_with(|| Collider {
                size: e.dimensions,
                ..Default::default()
            });
            e.controller = Some(controller);
        }
    }
    Ok(())
}

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
            ui.collapsing("Câmera", |ui| {
                ui.checkbox(&mut c.active, "Câmera ativa");
                if kind == SceneKind::TwoD {
                    ui.add(
                        egui::DragValue::new(&mut c.orthographic_size)
                            .range(0.1..=500.)
                            .prefix("Meia altura visível (m) "),
                    );
                } else {
                    ui.add(
                        egui::Slider::new(&mut c.fov, 10. ..=150.)
                            .text("Campo de visão vertical (°)"),
                    );
                }
                ui.small(
                    "Ative apenas a câmera desejada. O alvo é configurado no Controle da câmera.",
                );
            });
        }
        if kind == SceneKind::ThreeD {
            self.physics_properties(ui, entity);
        }
        if kind == SceneKind::TwoD && entity.camera.is_none() && entity.ui.is_none() {
            if entity.collider.is_some() {
                ui.collapsing("Colisor 2D", |ui| {
                    let c = entity.collider.as_mut().unwrap();
                    ui.checkbox(&mut c.enabled, "Colisor ativo");
                    ui.add_enabled(
                        entity.controller.is_none(),
                        egui::Checkbox::new(&mut c.is_trigger, "Área de detecção"),
                    )
                    .on_hover_text(
                        "Uma área detecta entradas. Um colisor sólido bloqueia movimento.",
                    );
                    vector3(ui, "Tamanho da caixa", &mut c.size, 0.05, true);
                    c.size = c.size.map(|v| v.max(0.0001));
                    vector3(ui, "Deslocamento", &mut c.offset, 0.05, false);
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
                    if ui
                        .add_enabled(
                            entity.controller.is_none(),
                            egui::Button::new("Remover componente"),
                        )
                        .on_hover_text("Remova primeiro o Movimento 2D que depende desta caixa.")
                        .clicked()
                    {
                        entity.collider = None;
                    }
                });
            }
            if entity.controller.is_some() {
                ui.collapsing("Movimento 2D", |ui| {
                    let c = entity.controller.as_mut().unwrap();
                    ui.checkbox(&mut c.enabled, "Movimento ativo");
                    for (value, label, max) in [
                        (&mut c.speed, "Velocidade ", 100.),
                        (&mut c.jump, "Pulo ", 100.),
                        (&mut c.gravity, "Gravidade ", 200.),
                    ] {
                        ui.add(egui::DragValue::new(value).range(0. ..=max).prefix(label));
                    }
                    if ui.button("Remover componente").clicked() {
                        entity.controller = None;
                    }
                });
            }
        }
        let choices = available(kind, entity);
        if choices.is_empty() {
            ui.add_enabled(false, egui::Button::new("+ Adicionar componente"))
                .on_hover_text("Nenhum componente compatível disponível para este objeto.");
        } else {
            ui.menu_button("+ Adicionar componente", |ui| {
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
