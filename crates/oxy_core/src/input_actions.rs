//! Stable input identities and editable display names, independent of native keys/UI.
use crate::document::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
/// The action `text` names: its ID, or the name of exactly one action (case-insensitive).
pub fn find(project: &Project, text: &str) -> Result<Id, String> {
    find_named(project, text)?.ok_or_else(|| {
        format!(
            "A ação de entrada \"{text}\" não existe no projeto. Ações: {}",
            summary(project)
        )
    })
}
/// [`find`], with `None` when no action has that ID or name; several actions sharing the
/// name are still an error.
pub fn find_named(project: &Project, text: &str) -> Result<Option<Id>, String> {
    if project.input_bindings.contains_key(text) {
        return Ok(Some(text.into()));
    }
    let wanted = text.trim().to_lowercase();
    let named: Vec<&Id> = project
        .input_bindings
        .keys()
        .filter(|id| label(project, id).to_lowercase() == wanted)
        .collect();
    match named.as_slice() {
        [one] => Ok(Some((*one).clone())),
        [] => Ok(None),
        many => Err(format!(
            "Há {} ações de entrada chamadas \"{text}\"; use o ID: {}",
            many.len(),
            many.iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}
/// Up to 30 actions as "ID (name)", for messages.
fn summary(project: &Project) -> String {
    if project.input_bindings.is_empty() {
        return "nenhuma".into();
    }
    let mut names: Vec<String> = project
        .input_bindings
        .keys()
        .take(30)
        .map(|id| format!("{id} ({})", label(project, id)))
        .collect();
    if project.input_bindings.len() > 30 {
        names.push(format!("e mais {}", project.input_bindings.len() - 30));
    }
    names.join(", ")
}

/// Keys an input action can use, by the names the game reads, separated by spaces: letters,
/// digits, the other keyboard keys, function keys, Shift and the mouse buttons and wheel.
/// Escape and F3 are left out: the game and the player keep them (pause and diagnostics).
const KEY_NAMES: &str = "A B C D E F G H I J K L M N O P Q R S T U V W X Y Z 0 1 2 3 4 5 6 7 8 9 \
Space Shift Enter Tab Backspace Insert Delete Home End PageUp PageDown Up Down Left Right Copy \
Cut Paste Colon Comma Minus Period Plus Equals Semicolon OpenBracket CloseBracket \
OpenCurlyBracket CloseCurlyBracket Backtick Backslash Slash Pipe Questionmark Exclamationmark \
Quote BrowserBack F1 F2 F4 F5 F6 F7 F8 F9 F10 F11 F12 F13 F14 F15 F16 F17 F18 F19 F20 F21 F22 F23 \
F24 F25 F26 F27 F28 F29 F30 F31 F32 F33 F34 F35 MouseLeft MouseRight MouseMiddle Mouse4 Mouse5 \
WheelUp WheelDown";
/// The keys an input action can use, in the order `KEY_NAMES` lists them.
pub fn keys() -> impl Iterator<Item = &'static str> {
    KEY_NAMES.split(' ')
}
/// Keys the game reads but never hands to input actions: Escape releases the game and F3
/// opens the player's diagnostics.
pub const RESERVED_KEYS: [&str; 2] = ["Escape", "F3"];
/// Other spellings the game reads for the same keys, besides lowercase letters and
/// Digit0/Numpad0 for digits.
const KEY_ALIASES: [(&str, &str); 35] = [
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
    ("ArrowUp", "Up"),
    ("⏷", "Down"),
    ("⏴", "Left"),
    ("⏵", "Right"),
    ("⏶", "Up"),
    ("Esc", "Escape"),
    ("Return", "Enter"),
    ("Help", "Insert"),
    (" ", "Space"),
    (":", "Colon"),
    (",", "Comma"),
    ("-", "Minus"),
    ("−", "Minus"),
    (".", "Period"),
    ("+", "Plus"),
    ("=", "Equals"),
    ("Equal", "Equals"),
    ("NumpadEqual", "Equals"),
    (";", "Semicolon"),
    ("\\", "Backslash"),
    ("/", "Slash"),
    ("|", "Pipe"),
    ("?", "Questionmark"),
    ("!", "Exclamationmark"),
    ("[", "OpenBracket"),
    ("]", "CloseBracket"),
    ("{", "OpenCurlyBracket"),
    ("}", "CloseCurlyBracket"),
    ("`", "Backtick"),
    ("Backquote", "Backtick"),
    ("Grave", "Backtick"),
    ("'", "Quote"),
];
/// The name of the key `key` spells, as [`keys`] or [`RESERVED_KEYS`] write it: "a" is "A",
/// "ArrowLeft" is "Left", "Esc" is "Escape". `None` for a name the game does not read.
fn key_name(key: &str) -> Option<&'static str> {
    let known = |name: &str| keys().chain(RESERVED_KEYS).find(|k| *k == name);
    if let Some(name) = known(key) {
        return Some(name);
    }
    if let Some((_, name)) = KEY_ALIASES.iter().find(|(alias, _)| *alias == key) {
        return Some(name);
    }
    if key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase() {
        return known(&key.to_ascii_uppercase());
    }
    let digit = key
        .strip_prefix("Digit")
        .or_else(|| key.strip_prefix("Numpad"))?;
    (digit.len() == 1).then(|| known(digit)).flatten()
}
/// The name an input action stores for `key`, or why it cannot use it.
pub fn check_key(key: &str) -> Result<&'static str, String> {
    match key_name(key) {
        Some("Escape") => Err("Escape é reservada: ela pausa o jogo e libera o mouse".into()),
        Some("F3") => Err("F3 é reservada ao diagnóstico do player".into()),
        Some(name) => Ok(name),
        None => Err(format!("Tecla desconhecida: \"{key}\"")),
    }
}
/// Whether an input action can use `key`: the game reads it and does not keep it.
pub fn valid_binding(key: &str) -> bool {
    check_key(key).is_ok()
}
/// The keys two or more actions are bound to, by the name the game reads ("r" and "R" are one
/// key), with those actions in ID order. In the game, pressing such a key fires them all.
pub fn shared_keys(project: &Project) -> BTreeMap<String, Vec<&Id>> {
    let mut bound: BTreeMap<String, Vec<&Id>> = BTreeMap::new();
    for (id, key) in &project.input_bindings {
        let name = key_name(key).map_or_else(|| key.clone(), str::to_owned);
        bound.entry(name).or_default().push(id);
    }
    bound.retain(|_, ids| ids.len() > 1);
    bound
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
/// One object that uses a project item (an input action, a physics surface or an asset).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Use {
    /// Name of the scene, or of the model, that holds the object.
    #[serde(skip)]
    pub scope: String,
    /// The object's ID and name.
    pub entity: Id,
    pub name: String,
    /// What uses the item: a component (controller2d, character, camera_control,
    /// collider3d), a node operation, "attribute <name>", or for assets material, ui or model.
    pub role: String,
    /// The model asset holding the object, when it exists only inside a model: scene reads
    /// do not reach it, and the scenes get it where the model is placed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
impl Use {
    pub(crate) fn new(scope: Holder, entity: &Entity, role: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            entity: entity.id.clone(),
            name: entity.name.clone(),
            role: role.into(),
            model: scope.model.then(|| scope.name.to_owned()),
        }
    }
}

/// What holds an object: a scene or a model asset, by name. It reads as its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Holder<'a> {
    pub name: &'a str,
    pub model: bool,
}
impl std::fmt::Display for Holder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}
impl From<Holder<'_>> for String {
    fn from(holder: Holder<'_>) -> Self {
        holder.name.to_owned()
    }
}

/// Every object of the scenes and models, with the scene or model holding it.
pub(crate) fn every_entity(project: &Project) -> impl Iterator<Item = (Holder<'_>, &Entity)> {
    project
        .scenes
        .iter()
        .map(|s| {
            let scope = Holder {
                name: &s.name,
                model: false,
            };
            (scope, s.entities.as_slice())
        })
        .chain(project.assets.iter().filter_map(|a| {
            let scope = Holder {
                name: &a.name,
                model: true,
            };
            a.model.as_deref().map(|m| (scope, m))
        }))
        .flat_map(|(scope, entities)| entities.iter().map(move |e| (scope, e)))
}

/// Where the action `id` is used, in each object's order: 2D controller, 3D character, logic
/// nodes, character camera.
pub fn uses(project: &Project, id: &str) -> Vec<Use> {
    let mut found = Vec::new();
    for (scope, entity) in every_entity(project) {
        let mut add = |role: &str| found.push(Use::new(scope, entity, role));
        if entity
            .controller
            .as_ref()
            .is_some_and(|c| c.actions.all().iter().any(|(action, _, _)| *action == id))
        {
            add("controller2d");
        }
        if entity.character3d.as_ref().is_some_and(|c| {
            c.sprint_action == id
                || c.crouch_action == id
                || c.actions.all().iter().any(|(action, _, _)| *action == id)
        }) {
            add("character");
        }
        for node in &entity.graph.nodes {
            if matches!(node.operation.as_str(), "event.input" | "input.read")
                && node.text("action") == id
            {
                add(&node.operation);
            }
        }
        if entity
            .camera_rig
            .as_ref()
            .is_some_and(|c| c.shoulder_action == id || c.mode_action == id)
        {
            add("camera_control");
        }
    }
    found
}
pub fn references(project: &Project, id: &str) -> Vec<String> {
    uses(project, id)
        .into_iter()
        .map(|found| {
            let role = match found.role.as_str() {
                "controller2d" => "controlador",
                "character" => "personagem 3D",
                "camera_control" => "câmera de personagem",
                "input.read" => "nó Ler ação de entrada",
                _ => "nó Ação de entrada",
            };
            format!("{} → {} → {role}", found.scope, found.name)
        })
        .collect()
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
