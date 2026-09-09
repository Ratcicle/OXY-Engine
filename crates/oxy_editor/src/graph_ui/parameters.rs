//! Human-readable choices preserve stable identifiers in graph parameters.
use egui::Color32;
use oxy_core::document::{AssetKind, Id, Project, Scene, Value};
use oxy_core::graph::{Node, ParamDef};
use std::collections::{BTreeMap, BTreeSet};

pub fn value_editor(
    ui: &mut egui::Ui,
    value: &mut Value,
    objects: &[(Id, String)],
    salt: impl std::hash::Hash,
) {
    match value {
        Value::Number(v) => {
            ui.add(egui::DragValue::new(v).speed(0.1));
        }
        Value::Text(v) => {
            ui.add(egui::TextEdit::singleline(v).desired_width(155.));
        }
        Value::Bool(v) => {
            ui.checkbox(v, "Verdadeiro");
        }
        Value::Object(v) => {
            object_picker(ui, v, objects, salt);
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
        .width(150.)
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
        .width(190.0)
        .selected_text(label)
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
    let explicit_target = node
        .params
        .get("target")
        .and_then(Value::object)
        .map(str::to_owned);
    let target = explicit_target.as_deref().or(context.owner);
    let mut specialized = false;
    if let Value::Text(selected) = &mut value {
        match (node.operation.as_str(), param.id) {
            ("action.animation", "clip") => {
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
                    && !context
                        .scene
                        .entity(target)
                        .is_some_and(|entity| entity.clips.iter().any(|clip| clip.id == *selected))
                {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "A animação selecionada não pertence ao objeto atual.",
                    );
                }
                specialized = true;
            }
            ("action.sound", "asset") => {
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
            ("action.scene", "scene") => {
                let choices: Vec<_> = context
                    .project
                    .scenes
                    .iter()
                    .map(|scene| (scene.id.clone(), scene.name.clone()))
                    .collect();
                named_choice(ui, selected, &choices, salt.clone());
                specialized = true;
            }
            ("event.input", "action") => {
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
            ("event.input", "mode") => {
                enum_choice(
                    ui,
                    selected,
                    &[
                        ("pressed", "Pressionar"),
                        ("held", "Manter"),
                        ("released", "Soltar"),
                    ],
                    salt.clone(),
                );
                specialized = true;
            }
            ("action.component", "component") => {
                enum_choice(
                    ui,
                    selected,
                    &[
                        ("collider", "Colisão / área"),
                        ("controller", "Controlador de movimento"),
                        ("visible", "Visibilidade"),
                        ("behavior", "Comportamento visual"),
                        ("animation", "Reprodução da animação"),
                    ],
                    salt.clone(),
                );
                specialized = true;
            }
            ("math.binary", "operator") => {
                enum_choice(
                    ui,
                    selected,
                    &[
                        ("+", "Somar · A + B"),
                        ("-", "Subtrair · A − B"),
                        ("*", "Multiplicar · A × B"),
                        ("/", "Dividir · A ÷ B"),
                    ],
                    salt.clone(),
                );
                specialized = true;
            }
            ("condition.compare", "operator") => {
                enum_choice(
                    ui,
                    selected,
                    &[
                        ("==", "Igual · A = B"),
                        ("!=", "Diferente · A ≠ B"),
                        (">", "Maior · A > B"),
                        (">=", "Maior ou igual · A ≥ B"),
                        ("<", "Menor · A < B"),
                        ("<=", "Menor ou igual · A ≤ B"),
                    ],
                    salt.clone(),
                );
                specialized = true;
            }
            ("event.animation", "marker") => {
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
                        .desired_width(180.0),
                );
                specialized = true;
            }
            (_, "attribute") => {
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
                        .desired_width(180.0),
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
            };
            let mut selected_kind = kind;
            egui::ComboBox::from_id_salt((salt.clone(), "type"))
                .selected_text(["Número", "Texto", "Booleano", "Objeto"][kind])
                .show_ui(ui, |ui| {
                    for (kind, label) in
                        ["Número", "Texto", "Booleano", "Objeto"].iter().enumerate()
                    {
                        ui.selectable_value(&mut selected_kind, kind, *label);
                    }
                });
            if selected_kind != kind {
                value = match selected_kind {
                    0 => Value::Number(0.0),
                    1 => Value::Text(String::new()),
                    2 => Value::Bool(false),
                    _ => Value::Object(None),
                };
            }
        }
        value_editor(ui, &mut value, context.objects, salt);
    }
    node.params.insert(param.id.into(), value);
}
