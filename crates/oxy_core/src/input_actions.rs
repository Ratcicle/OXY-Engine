//! Stable input identities and editable display names, independent of native keys/UI.
use crate::document::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovementActions {
    pub left: Id,
    pub right: Id,
    pub forward: Id,
    pub back: Id,
    pub jump: Id,
}
impl Default for MovementActions {
    fn default() -> Self {
        Self {
            left: "mover_esquerda".into(),
            right: "mover_direita".into(),
            forward: "mover_frente".into(),
            back: "mover_tras".into(),
            jump: "pular".into(),
        }
    }
}
impl MovementActions {
    pub fn all(&self) -> [(&str, &str, &str); 5] {
        [
            (&self.left, "Mover à esquerda", "A"),
            (&self.right, "Mover à direita", "D"),
            (&self.forward, "Mover para frente", "W"),
            (&self.back, "Mover para trás", "S"),
            (&self.jump, "Pular", "Space"),
        ]
    }
}
pub fn legacy_label(id: &str) -> String {
    match id {
        "mover_esquerda" => "Mover à esquerda",
        "mover_direita" => "Mover à direita",
        "mover_frente" => "Mover para frente",
        "mover_tras" => "Mover para trás",
        "pular" => "Pular",
        "correr" => "Correr",
        "agachar" => "Agachar",
        "trocar_ombro" => "Trocar ombro da câmera",
        "alternar_camera" => "Alternar primeira/terceira pessoa",
        "atacar" => "Atacar",
        "interagir" => "Interagir",
        _ => return id.replace('_', " "),
    }
    .into()
}
pub fn label<'a>(project: &'a Project, id: &str) -> std::borrow::Cow<'a, str> {
    project.input_labels.get(id).map_or_else(
        || std::borrow::Cow::Owned(legacy_label(id)),
        |s| std::borrow::Cow::Borrowed(s.as_str()),
    )
}
pub fn create(project: &mut Project, name: &str, key: &str) -> Result<Id, String> {
    if name.trim().is_empty() {
        return Err("Informe o nome da ação.".into());
    }
    let id = new_id();
    project.input_bindings.insert(id.clone(), key.into());
    project.input_labels.insert(id.clone(), name.trim().into());
    Ok(id)
}
pub fn rename(project: &mut Project, id: &str, name: &str) -> Result<(), String> {
    if !project.input_bindings.contains_key(id) {
        return Err("Ação não encontrada.".into());
    }
    if name.trim().is_empty() {
        return Err("O nome da ação não pode ficar vazio.".into());
    }
    project.input_labels.insert(id.into(), name.trim().into());
    Ok(())
}
pub fn references(project: &Project, id: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (scope, entities) in project
        .scenes
        .iter()
        .map(|s| (s.name.as_str(), s.entities.as_slice()))
        .chain(
            project
                .assets
                .iter()
                .filter_map(|a| a.model.as_deref().map(|m| (a.name.as_str(), m))),
        )
    {
        for entity in entities {
            if entity
                .controller
                .as_ref()
                .is_some_and(|c| c.actions.all().iter().any(|(action, _, _)| *action == id))
            {
                found.push(format!("{scope} → {} → controlador", entity.name));
            }
            if entity.character3d.as_ref().is_some_and(|c| {
                c.sprint_action == id
                    || c.crouch_action == id
                    || c.actions.all().iter().any(|(action, _, _)| *action == id)
            }) {
                found.push(format!("{scope} → {} → personagem 3D", entity.name));
            }
            for node in &entity.graph.nodes {
                if matches!(node.operation.as_str(), "event.input" | "input.read")
                    && node.text("action") == id
                {
                    found.push(format!(
                        "{scope} → {} → ação de entrada ({})",
                        entity.name, node.id
                    ));
                }
            }
            if entity
                .camera_rig
                .as_ref()
                .is_some_and(|c| c.shoulder_action == id || c.mode_action == id)
            {
                found.push(format!("{scope} → {} → câmera de personagem", entity.name));
            }
        }
    }
    found
}
pub fn remove(project: &mut Project, id: &str) -> Result<(), String> {
    let uses = references(project, id);
    if !uses.is_empty() {
        return Err(format!(
            "Altere estes vínculos antes de excluir a ação:\n{}",
            uses.join("\n")
        ));
    }
    project
        .input_bindings
        .remove(id)
        .ok_or("Ação não encontrada")?;
    project.input_labels.remove(id);
    Ok(())
}
pub fn ensure_controller(project: &mut Project, actions: &MovementActions, kind: SceneKind) {
    for (index, (id, name, key)) in actions.all().iter().enumerate() {
        if kind == SceneKind::TwoD && matches!(index, 2 | 3) {
            continue;
        }
        project
            .input_bindings
            .entry((*id).into())
            .or_insert_with(|| (*key).into());
        project
            .input_labels
            .entry((*id).into())
            .or_insert_with(|| (*name).into());
    }
}
pub fn ensure_character(project: &mut Project, config: &crate::character::CharacterConfig) {
    ensure_controller(project, &config.actions, SceneKind::ThreeD);
    for (id, name, key) in [
        (&config.sprint_action, "Correr", "Shift"),
        (&config.crouch_action, "Agachar", "C"),
    ] {
        project
            .input_bindings
            .entry(id.clone())
            .or_insert_with(|| key.into());
        project
            .input_labels
            .entry(id.clone())
            .or_insert_with(|| name.into());
    }
}
pub fn ensure_camera(project: &mut Project, rig: &crate::character::CameraRig) {
    for (id, name, key) in [
        (&rig.shoulder_action, "Trocar ombro da câmera", "Q"),
        (&rig.mode_action, "Alternar primeira/terceira pessoa", "V"),
    ] {
        project
            .input_bindings
            .entry(id.clone())
            .or_insert_with(|| key.into());
        project
            .input_labels
            .entry(id.clone())
            .or_insert_with(|| name.into());
    }
}
