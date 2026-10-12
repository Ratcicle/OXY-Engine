//! Human-readable choices preserve stable identifiers in graph parameters.
use oxy_core::document::{AssetKind, Id, Project, Scene, Value};
use oxy_core::graph::{Node, OperationDef, ParamDef, Reference, reference};
use std::collections::{BTreeMap, BTreeSet};

pub fn value_editor(
    ui: &mut egui::Ui,
    value: &mut Value,
    objects: &[(Id, String)],
    surfaces: &[oxy_core::surface::SurfaceMaterial],
    salt: impl std::hash::Hash,
) {
    match value {
        Value::Number(v) => {
            ui.add(egui::DragValue::new(v).speed(0.1));
        }
        Value::Text(v) => {
            ui.add(egui::TextEdit::singleline(v).desired_width(ui.available_width()));
        }
        Value::Bool(v) => {
            ui.checkbox(v, "Verdadeiro");
        }
        Value::Object(v) => {
            object_picker(ui, v, objects, salt);
        }
        Value::Vector2(v) => {
            ui.horizontal(|ui| {
                for (axis, value) in ["X", "Y"].iter().zip(v) {
                    ui.add(
                        egui::DragValue::new(value)
                            .speed(0.05)
                            .prefix(format!("{axis} ")),
                    );
                }
            });
        }
        Value::Vector3(v) => {
            ui.horizontal_wrapped(|ui| {
                for (axis, value) in ["X", "Y", "Z"].iter().zip(v) {
                    ui.add(
                        egui::DragValue::new(value)
                            .speed(0.05)
                            .prefix(format!("{axis} ")),
                    );
                }
            });
        }
        Value::Surface(value) => {
            egui::ComboBox::from_id_salt(salt)
                .selected_text(
                    value
                        .as_ref()
                        .and_then(|id| surfaces.iter().find(|s| &s.id == id))
                        .map_or("Sem superfície", |s| s.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(value, None, "Sem superfície");
                    for surface in surfaces {
                        ui.selectable_value(value, Some(surface.id.clone()), &surface.name);
                    }
                });
        }
    }
}

pub fn object_picker(
    ui: &mut egui::Ui,
    value: &mut Option<Id>,
    objects: &[(Id, String)],
    salt: impl std::hash::Hash,
) {
    let label = value
        .as_ref()
        .and_then(|id| {
            objects
                .iter()
                .find(|(key, _)| key == id)
                .map(|(_, s)| s.as_str())
        })
        .unwrap_or("Próprio / nenhum");
    egui::ComboBox::from_id_salt(salt)
        .selected_text(label)
        .width(ui.available_width().clamp(80., 220.))
        .truncate()
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Próprio / nenhum");
            for (id, name) in objects {
                ui.selectable_value(value, Some(id.clone()), name);
            }
        });
}

pub(super) struct ParameterContext<'a> {
    pub(super) objects: &'a [(Id, String)],
    pub(super) project: &'a Project,
    pub(super) scene: &'a Scene,
    pub(super) owner: Option<&'a str>,
    pub(super) target_connected: bool,
    pub(super) any_type: bool,
}

fn named_choice(
    ui: &mut egui::Ui,
    value: &mut String,
    choices: &[(String, String)],
    salt: impl std::hash::Hash,
) -> bool {
    let before = value.clone();
    let label = choices
        .iter()
        .find(|(id, _)| id == value)
        .map(|(_, label)| label.as_str())
        .unwrap_or(if value.is_empty() {
            "Escolha…"
        } else {
            "Referência ausente / inválida"
        });
    egui::ComboBox::from_id_salt(salt)
        .width(ui.available_width().clamp(80., 240.))
        .selected_text(label)
        .truncate()
        .show_ui(ui, |ui| {
            if choices.is_empty() {
                ui.label("Nenhuma opção disponível nesta cena/projeto.");
            }
            for (id, label) in choices {
                ui.selectable_value(value, id.clone(), label);
            }
        });
    *value != before
}

fn enum_choice(
    ui: &mut egui::Ui,
    value: &mut String,
    choices: &[(&str, &str)],
    salt: impl std::hash::Hash,
) {
    let owned: Vec<_> = choices
        .iter()
        .map(|(id, label)| ((*id).to_owned(), (*label).to_owned()))
        .collect();
    named_choice(ui, value, &owned, salt);
}

/// Name of a parameter in the inspector and in the node, without the option list that
/// some definitions carry in their label.
pub(super) fn param_label(operation: &str, param: &ParamDef) -> &'static str {
    match (operation, param.id) {
        ("action.component", "component") => "Componente",
        ("condition.compare", "operator") => "Comparação",
        _ => param.label,
    }
}

/// What the picker of a parameter chosen by name (a scene, an audio, a clip, an input action)
/// shows while the input of the same id is connected, as Mudar de cena's scene with Cena atual
/// wired in: the input decides the value. `None` for the other parameters.
pub(super) fn connected_choice(operation: &OperationDef, param: &ParamDef) -> Option<String> {
    let input = operation.inputs.iter().find(|port| port.id == param.id)?;
    matches!(
        reference(operation.id, param),
        Some(Reference::Scene | Reference::Audio | Reference::Clip | Reference::InputAction)
    )
    .then(|| format!("Usando a entrada {}", input.label))
}

fn number_text(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Short text of a parameter value for the node body: names instead of identifiers.
/// `None` when there is nothing useful to show, such as an empty object reference.
pub(super) fn param_summary(
    node: &Node,
    param: &ParamDef,
    context: &ParameterContext<'_>,
) -> Option<String> {
    let value = node.params.get(param.id).unwrap_or(&param.default);
    Some(match value {
        Value::Number(number) => number_text(*number),
        Value::Bool(on) => if *on { "Sim" } else { "Não" }.into(),
        Value::Object(None) => return None,
        Value::Object(Some(id)) => context
            .objects
            .iter()
            .find(|(key, _)| key == id)
            .map_or_else(|| "objeto removido".into(), |(_, name)| name.clone()),
        Value::Vector2(v) => format!(
            "{} · {}",
            number_text(v[0].into()),
            number_text(v[1].into())
        ),
        Value::Vector3(v) => format!(
            "{} · {} · {}",
            number_text(v[0].into()),
            number_text(v[1].into()),
            number_text(v[2].into())
        ),
        Value::Surface(None) => "nenhuma".into(),
        Value::Surface(Some(id)) => context
            .project
            .surfaces
            .iter()
            .find(|surface| &surface.id == id)
            .map_or_else(|| "superfície removida".into(), |s| s.name.clone()),
        Value::Text(text) if text.is_empty() => "—".into(),
        Value::Text(text) => {
            if let Some(options) = oxy_core::graph::choices(&node.operation, param.id) {
                return Some(
                    options
                        .iter()
                        .find(|(id, _)| id == text)
                        .map_or(text.as_str(), |(_, label)| label)
                        .to_owned(),
                );
            }
            match reference(&node.operation, param) {
                Some(Reference::Clip) => context
                    .scene
                    .entities
                    .iter()
                    .flat_map(|entity| entity.clips.iter())
                    .find(|clip| &clip.id == text)
                    .map_or_else(|| "animação removida".into(), |clip| clip.name.clone()),
                Some(Reference::Audio) => context
                    .project
                    .assets
                    .iter()
                    .find(|asset| &asset.id == text)
                    .map_or_else(|| "áudio removido".into(), |asset| asset.name.clone()),
                Some(Reference::Scene) => context
                    .project
                    .scenes
                    .iter()
                    .find(|scene| &scene.id == text)
                    .map_or_else(|| "cena removida".into(), |scene| scene.name.clone()),
                // The key goes in parentheses: the action name alone stays the name of
                // its field in Ações de entrada.
                Some(Reference::InputAction) => match context.project.input_bindings.get(text) {
                    Some(key) => format!(
                        "{} ({})",
                        oxy_core::input_actions::label(context.project, text),
                        crate::labels::key(key)
                    ),
                    None => "ação removida".into(),
                },
                _ => text.clone(),
            }
        }
    })
}

pub(super) fn parameter_editor(
    ui: &mut egui::Ui,
    node: &mut Node,
    param: &ParamDef,
    context: &ParameterContext<'_>,
) {
    let mut value = node
        .params
        .get(param.id)
        .cloned()
        .unwrap_or_else(|| param.default.clone());
    let salt = (node.id.clone(), param.id);
    if node.operation.starts_with("query.")
        && matches!(param.id, "category" | "mask")
        && let Value::Number(number) = &mut value
    {
        let mut bits = (*number).clamp(0., f64::from(u32::MAX)) as u32;
        collision_filter_picker(ui, &mut bits, context.project, param.label);
        *number = f64::from(bits);
        node.params.insert(param.id.into(), value);
        return;
    }
    let explicit_target = node
        .params
        .get("target")
        .and_then(Value::object)
        .map(str::to_owned);
    let target = explicit_target.as_deref().or(context.owner);
    let mut specialized = false;
    if let Value::Text(selected) = &mut value {
        // Fixed values (modes, operators, components, movement options) from the core, which
        // also refuses others in agent commands.
        if let Some(choices) = oxy_core::graph::choices(&node.operation, param.id) {
            enum_choice(ui, selected, choices, salt.clone());
            node.params.insert(param.id.into(), value);
            return;
        }
        match reference(&node.operation, param) {
            Some(Reference::Clip) => {
                let choices: Vec<_> = context
                    .scene
                    .entities
                    .iter()
                    .flat_map(|entity| {
                        entity.clips.iter().map(move |clip| {
                            (clip.id.clone(), format!("{} · {}", entity.name, clip.name))
                        })
                    })
                    .collect();
                let changed = named_choice(ui, selected, &choices, salt.clone());
                // On a node that acts on an object, picking a clip also picks its object.
                if node.params.contains_key("target") {
                    if changed
                        && !context.target_connected
                        && let Some(owner) = context
                            .scene
                            .entities
                            .iter()
                            .find(|entity| entity.clips.iter().any(|clip| clip.id == *selected))
                    {
                        node.params
                            .insert("target".into(), Value::Object(Some(owner.id.clone())));
                    }
                    ui.small(if context.target_connected {
                        "O objeto recebido pela porta deve conter a animação escolhida."
                    } else {
                        "Escolher uma animação define também o objeto que a contém."
                    });
                    if let Some(target) = target
                        && !context.target_connected
                        && !changed
                        && !selected.is_empty()
                        && !context.scene.entity(target).is_some_and(|entity| {
                            entity.clips.iter().any(|clip| clip.id == *selected)
                        })
                    {
                        ui.colored_label(
                            crate::theme::ERROR,
                            "A animação selecionada não pertence ao objeto atual.",
                        );
                    }
                }
                specialized = true;
            }
            Some(Reference::Audio) => {
                let choices: Vec<_> = context
                    .project
                    .assets
                    .iter()
                    .filter(|asset| asset.kind == AssetKind::Audio)
                    .map(|asset| (asset.id.clone(), asset.name.clone()))
                    .collect();
                named_choice(ui, selected, &choices, salt.clone());
                ui.small("Importe um WAV pela biblioteca para adicioná-lo aqui.");
                specialized = true;
            }
            Some(Reference::Scene) => {
                let choices: Vec<_> = context
                    .project
                    .scenes
                    .iter()
                    .map(|scene| (scene.id.clone(), scene.name.clone()))
                    .collect();
                named_choice(ui, selected, &choices, salt.clone());
                specialized = true;
            }
            Some(Reference::InputAction) => {
                let choices: Vec<_> = context
                    .project
                    .input_bindings
                    .iter()
                    .map(|(action, key)| {
                        (
                            action.clone(),
                            format!(
                                "{} · {}",
                                oxy_core::input_actions::label(context.project, action),
                                crate::labels::key(key)
                            ),
                        )
                    })
                    .collect();
                named_choice(ui, selected, &choices, salt.clone());
                specialized = true;
            }
            Some(Reference::Marker) => {
                let markers: BTreeSet<_> = context
                    .scene
                    .entities
                    .iter()
                    .filter(|entity| target.is_none_or(|id| entity.id == id))
                    .flat_map(|entity| {
                        entity
                            .clips
                            .iter()
                            .flat_map(|clip| clip.events.iter().map(|event| event.name.clone()))
                    })
                    .collect();
                if !markers.is_empty() {
                    let choices: Vec<_> = markers
                        .into_iter()
                        .map(|marker| (marker.clone(), marker))
                        .collect();
                    named_choice(ui, selected, &choices, (salt.clone(), "markers"));
                }
                ui.add(
                    egui::TextEdit::singleline(selected)
                        .hint_text("Nome do marcador")
                        .desired_width(ui.available_width()),
                );
                specialized = true;
            }
            Some(Reference::Attribute) => {
                let attributes: BTreeMap<_, _> = context
                    .scene
                    .entities
                    .iter()
                    .filter(|entity| {
                        context.target_connected || target.is_none_or(|id| entity.id == id)
                    })
                    .flat_map(|entity| {
                        entity
                            .attributes
                            .iter()
                            .map(|(name, value)| (name.clone(), value.clone()))
                    })
                    .collect();
                if !attributes.is_empty() {
                    let choices: Vec<_> = attributes
                        .keys()
                        .map(|name| (name.clone(), name.clone()))
                        .collect();
                    if named_choice(ui, selected, &choices, (salt.clone(), "attributes"))
                        && node.operation == "attribute.set"
                        && let Some(existing) = attributes.get(selected)
                        && node.params.get("value").is_some_and(|value| {
                            std::mem::discriminant(value) != std::mem::discriminant(existing)
                        })
                    {
                        node.params.insert("value".into(), existing.clone());
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(selected)
                        .hint_text("Nome do atributo")
                        .desired_width(ui.available_width()),
                );
                if context.target_connected {
                    ui.small("Lista da cena; o objeto recebido deve possuir o atributo escolhido.");
                }
                specialized = true;
            }
            _ => {}
        }
    }
    if !specialized {
        if context.any_type {
            let kind = match value {
                Value::Number(_) => 0,
                Value::Text(_) => 1,
                Value::Bool(_) => 2,
                Value::Object(_) => 3,
                Value::Vector2(_) => 4,
                Value::Vector3(_) => 5,
                Value::Surface(_) => 6,
            };
            let mut selected_kind = kind;
            egui::ComboBox::from_id_salt((salt.clone(), "type"))
                .selected_text(
                    [
                        "Número",
                        "Texto",
                        "Booleano",
                        "Objeto",
                        "Vetor2",
                        "Vetor3",
                        "Superfície física",
                    ][kind],
                )
                .show_ui(ui, |ui| {
                    for (kind, label) in [
                        "Número",
                        "Texto",
                        "Booleano",
                        "Objeto",
                        "Vetor2",
                        "Vetor3",
                        "Superfície física",
                    ]
                    .iter()
                    .enumerate()
                    {
                        ui.selectable_value(&mut selected_kind, kind, *label);
                    }
                });
            if selected_kind != kind {
                value = match selected_kind {
                    0 => Value::Number(0.0),
                    1 => Value::Text(String::new()),
                    2 => Value::Bool(false),
                    4 => Value::Vector2([0.; 2]),
                    5 => Value::Vector3([0.; 3]),
                    6 => Value::Surface(None),
                    _ => Value::Object(None),
                };
            }
        }
        value_editor(
            ui,
            &mut value,
            context.objects,
            &context.project.surfaces,
            salt,
        );
    }
    node.params.insert(param.id.into(), value);
}

/// The persisted mask is unchanged; friendly names are project metadata only.
pub fn collision_filter_picker(ui: &mut egui::Ui, bits: &mut u32, project: &Project, label: &str) {
    ui.menu_button(label, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Todos").clicked() {
                *bits = u32::MAX;
            }
            if ui.button("Nenhum").clicked() {
                *bits = 0;
            }
        });
        egui::ScrollArea::vertical()
            .max_height(250.)
            .show(ui, |ui| {
                for bit in 0..32_u8 {
                    let flag = 1_u32 << bit;
                    let mut selected = (*bits & flag) != 0;
                    if ui
                        .checkbox(&mut selected, project.collision_group_label(bit))
                        .changed()
                    {
                        if selected {
                            *bits |= flag;
                        } else {
                            *bits &= !flag;
                        }
                    }
                }
            });
    })
    .response
    .on_hover_text(
        "Escolha grupos físicos. Os nomes não alteram as regras e não são camadas de desenho.",
    );
}
