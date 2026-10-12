//! Project commands: edits described as data, applied atomically and validated. The editor,
//! the MCP server and tests share them, so an agent edits a project through the same rules
//! as a person.
//!
//! A batch is all-or-nothing: commands run in order on a copy of the project, the project is
//! validated after each one, and only a fully valid copy replaces the original. A string
//! such as `"$0"` in a later command stands for the first ID created by command 0 (see
//! [`Placeholder`]).
use crate::{
    animation::{AnimationEvent, Clip, Interpolation, Keyframe, Track},
    authoring::{self, ComponentKind},
    character::MovementProfile,
    document::*,
    editing,
    graph::{self, Edge, Node},
    input_actions::{self, every_entity},
    movement_presets::{self, MovementPreset, PresetCamera, PresetOptions},
    surface::{self, SurfaceMaterial, SurfacePreset, VelocitySpace},
};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json, json};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dimension {
    #[serde(rename = "2d")]
    TwoD,
    #[serde(rename = "3d")]
    ThreeD,
}
impl From<Dimension> for SceneKind {
    fn from(value: Dimension) -> Self {
        match value {
            Dimension::TwoD => SceneKind::TwoD,
            Dimension::ThreeD => SceneKind::ThreeD,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    FirstPerson,
    ThirdPerson,
    Platform,
}

/// What a new object is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntityKind {
    /// Empty object used to group others.
    Group,
    /// Parametric shape. 2D scenes take Rectangle, Circle and Sprite; 3D scenes the rest.
    Shape {
        shape: Primitive,
    },
    Camera,
    /// Game interface element: Text, Image, Button or Bar.
    Ui {
        element: UiKind,
    },
    /// Ready-made assembly with components, pieces and logic (3D scenes only).
    Preset {
        preset: Preset,
        /// Platform: how far it goes from its position, in metres (default [4, 0, 0]).
        #[serde(default)]
        travel: Option<[f32; 3]>,
        /// Platform: seconds for the way there and back (default 4).
        #[serde(default)]
        period: Option<f32>,
        /// Characters: the camera that follows the player (default create).
        #[serde(default)]
        camera: Option<PresetCamera>,
    },
}

/// A keyframe of [`Command::SetClip`]. The channels left out are the target's rest transform,
/// and the target's pivot is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySpec {
    /// Seconds from the start of the clip, 0 to its duration.
    pub time: f32,
    /// Local position.
    #[serde(default)]
    pub position: Option<[f32; 3]>,
    /// Local rotation in degrees.
    #[serde(default)]
    pub rotation: Option<[f32; 3]>,
    #[serde(default)]
    pub scale: Option<[f32; 3]>,
    /// How the clip goes on to the next keyframe (default linear).
    #[serde(default)]
    pub interpolation: Option<KeyInterpolation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyInterpolation {
    Linear,
    /// Keeps this keyframe until the next one.
    Hold,
}
/// The keyframes of one object in [`Command::SetClip`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSpec {
    /// `"self"` (the default) for the clip's object, or the ID of a piece under it.
    #[serde(default)]
    pub target: Option<Id>,
    pub keyframes: Vec<KeySpec>,
}
/// A marker of [`Command::SetClip`]: `event.animation` nodes start at it by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerSpec {
    pub time: f32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    RenameProject {
        name: String,
    },
    SetStartScene {
        scene: Id,
    },
    CreateScene {
        name: String,
        kind: Dimension,
    },
    RenameScene {
        scene: Id,
        name: String,
    },
    DuplicateScene {
        scene: Id,
    },
    DeleteScene {
        scene: Id,
    },
    CreateEntity {
        #[serde(default)]
        scene: Option<Id>,
        kind: EntityKind,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        parent: Option<Id>,
        /// Position relative to the parent.
        #[serde(default)]
        position: Option<[f32; 3]>,
    },
    /// Deletes the objects and everything under them.
    DeleteEntities {
        #[serde(default)]
        scene: Option<Id>,
        ids: Vec<Id>,
    },
    /// Copies the objects `count` times (default 1); copy k moves k × `offset` from the
    /// original, relative to its parent (default [0.5, 0, 0]). Keyframes that animate a copied
    /// object move with it.
    DuplicateEntities {
        #[serde(default)]
        scene: Option<Id>,
        ids: Vec<Id>,
        #[serde(default)]
        offset: Option<[f32; 3]>,
        #[serde(default)]
        count: Option<usize>,
    },
    /// Copies objects of `scene`, with everything under them, to `to_scene` (under `parent`,
    /// or at its root), keeping their place in the world. To another scene, their references
    /// must stay inside the copy.
    CopyEntities {
        #[serde(default)]
        scene: Option<Id>,
        ids: Vec<Id>,
        to_scene: Id,
        #[serde(default)]
        parent: Option<Id>,
    },
    /// Saves an object and everything under it as a model asset that any scene can place.
    SaveModel {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        /// Default: the object's name.
        #[serde(default)]
        name: Option<String>,
    },
    /// Places a copy of a model (its ID or name). Position is relative to the parent.
    PlaceModel {
        #[serde(default)]
        scene: Option<Id>,
        model: Id,
        #[serde(default)]
        parent: Option<Id>,
        #[serde(default)]
        position: Option<[f32; 3]>,
        #[serde(default)]
        name: Option<String>,
    },
    RenameEntity {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        name: String,
    },
    /// Moves objects under `parent` (or to the scene root), keeping their world transform.
    SetParent {
        #[serde(default)]
        scene: Option<Id>,
        ids: Vec<Id>,
        #[serde(default)]
        parent: Option<Id>,
    },
    /// Local transform. Rotation is in degrees. With `move_tracks`, the keyframes that animate
    /// the object follow it ([`editing::carry_tracks`]); by default they do when its own
    /// animations move it, as a moving platform's do.
    SetTransform {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        #[serde(default)]
        position: Option<[f32; 3]>,
        #[serde(default)]
        rotation: Option<[f32; 3]>,
        #[serde(default)]
        scale: Option<[f32; 3]>,
        #[serde(default)]
        move_tracks: Option<bool>,
    },
    /// Creates an animation of `entity`, or replaces its animation `clip` (ID or name) keeping
    /// the ID that logic nodes use.
    SetClip {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        #[serde(default)]
        clip: Option<Id>,
        name: String,
        /// Seconds.
        duration: f32,
        #[serde(default)]
        looping: bool,
        tracks: Vec<TrackSpec>,
        #[serde(default)]
        events: Vec<MarkerSpec>,
    },
    /// Refused while an `action.animation` node plays the clip.
    DeleteClip {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        clip: Id,
    },
    SetVisible {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        visible: bool,
    },
    SetMaterial {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        /// RGBA, 0 to 1.
        #[serde(default)]
        color: Option<[f32; 4]>,
        #[serde(default)]
        texture: Option<Id>,
        #[serde(default)]
        clear_texture: bool,
    },
    /// Sets an attribute, or removes it when `value` is absent.
    SetAttribute {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        name: String,
        #[serde(default)]
        value: Option<Value>,
    },
    AddComponent {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        component: ComponentKind,
    },
    RemoveComponent {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        component: ComponentKind,
    },
    /// Writes a movement profile's values into a 3D character (see
    /// [`crate::character::PROFILE_FIELDS`]); they stay editable.
    ApplyMovementProfile {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        profile: MovementProfile,
    },
    /// JSON merge patch (RFC 7386) over the object's document form, for fields without a
    /// dedicated command. The ID cannot change.
    UpdateEntity {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        patch: Json,
    },
    AddNode {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        operation: String,
        #[serde(default)]
        position: Option<[f32; 2]>,
        #[serde(default)]
        params: BTreeMap<String, Value>,
    },
    /// Removes a node and its connections.
    RemoveNode {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        node: Id,
    },
    SetNodeParams {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        node: Id,
        params: BTreeMap<String, Value>,
    },
    Connect {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        from_node: Id,
        from_port: String,
        to_node: Id,
        to_port: String,
    },
    Disconnect {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        from_node: Id,
        from_port: String,
        to_node: Id,
        to_port: String,
    },
    /// New input action (Ações de entrada) bound to `key`, one of
    /// [`input_actions::keys`]. Its ID is what logic nodes and simulations use.
    CreateInputAction {
        name: String,
        key: String,
    },
    /// `action` is the action's ID or its name.
    RenameInputAction {
        action: Id,
        name: String,
    },
    SetInputKey {
        action: Id,
        key: String,
    },
    /// Refused while a component or logic node uses the action.
    DeleteInputAction {
        action: Id,
    },
    /// New physics surface: the preset's values (common by default), then the ones given.
    CreateSurface {
        #[serde(default)]
        preset: Option<SurfacePreset>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        friction: Option<f32>,
        #[serde(default)]
        traction: Option<f32>,
        #[serde(default)]
        speed_multiplier: Option<f32>,
        /// Conveyor velocity in m/s.
        #[serde(default)]
        conveyor: Option<[f32; 3]>,
        #[serde(default)]
        conveyor_space: Option<VelocitySpace>,
    },
    /// Changes the values given; the others stay.
    UpdateSurface {
        surface: Id,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        friction: Option<f32>,
        #[serde(default)]
        traction: Option<f32>,
        #[serde(default)]
        speed_multiplier: Option<f32>,
        #[serde(default)]
        conveyor: Option<[f32; 3]>,
        #[serde(default)]
        conveyor_space: Option<VelocitySpace>,
    },
    /// Refused while a collider, character, attribute or logic node uses the surface.
    DeleteSurface {
        surface: Id,
    },
}
impl Command {
    /// The command's name as written in JSON.
    pub fn op(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("op").and_then(Json::as_str).map(str::to_owned))
            .unwrap_or_default()
    }
    /// Short Portuguese description, for the undo history and the console.
    pub fn label(&self) -> &'static str {
        match self {
            Self::RenameProject { .. } => "Renomear projeto",
            Self::SetStartScene { .. } => "Escolher cena inicial",
            Self::CreateScene { .. } => "Criar cena",
            Self::RenameScene { .. } => "Renomear cena",
            Self::DuplicateScene { .. } => "Duplicar cena",
            Self::DeleteScene { .. } => "Excluir cena",
            Self::CreateEntity { .. } => "Criar objeto",
            Self::DeleteEntities { .. } => "Excluir objetos",
            Self::DuplicateEntities { .. } => "Duplicar objetos",
            Self::CopyEntities { .. } => "Copiar objetos para outra cena",
            Self::SaveModel { .. } => "Salvar modelo",
            Self::PlaceModel { .. } => "Colocar modelo",
            Self::RenameEntity { .. } => "Renomear objeto",
            Self::SetParent { .. } => "Mudar pai",
            Self::SetTransform { .. } => "Transformar objeto",
            Self::SetClip { .. } => "Definir animação",
            Self::DeleteClip { .. } => "Excluir animação",
            Self::SetVisible { .. } => "Mudar visibilidade",
            Self::SetMaterial { .. } => "Mudar material",
            Self::SetAttribute { .. } => "Mudar atributo",
            Self::AddComponent { .. } => "Adicionar componente",
            Self::RemoveComponent { .. } => "Remover componente",
            Self::ApplyMovementProfile { .. } => "Aplicar perfil de movimento",
            Self::UpdateEntity { .. } => "Editar objeto",
            Self::AddNode { .. } => "Adicionar nó",
            Self::RemoveNode { .. } => "Remover nó",
            Self::SetNodeParams { .. } => "Mudar parâmetros do nó",
            Self::Connect { .. } => "Ligar portas",
            Self::Disconnect { .. } => "Desligar portas",
            Self::CreateInputAction { .. } => "Criar ação de entrada",
            Self::RenameInputAction { .. } => "Renomear ação de entrada",
            Self::SetInputKey { .. } => "Mudar tecla da ação",
            Self::DeleteInputAction { .. } => "Excluir ação de entrada",
            Self::CreateSurface { .. } => "Criar superfície física",
            Self::UpdateSurface { .. } => "Editar superfície física",
            Self::DeleteSurface { .. } => "Excluir superfície física",
        }
    }
}

/// What one command did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Applied {
    pub op: String,
    /// IDs the command created, in order (objects, scenes or nodes).
    pub created: Vec<Id>,
}

/// A command that failed; nothing in its batch was applied.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandError {
    pub index: usize,
    pub op: String,
    pub message: String,
}
impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Comando {} ({}): {}. Nada foi alterado.",
            self.index + 1,
            self.op,
            self.message
        )
    }
}
impl std::error::Error for CommandError {}

/// Applies `commands` in order as one atomic batch. Commands without a scene use
/// `default_scene`. On success the project is replaced; on failure it is unchanged.
pub fn apply(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
) -> Result<Vec<Applied>, CommandError> {
    let mut draft = project.clone();
    let mut applied: Vec<Applied> = Vec::with_capacity(commands.len());
    for (index, command) in commands.iter().enumerate() {
        let fail = |message: String| CommandError {
            index,
            op: command.op(),
            message,
        };
        let command = resolve_placeholders(command, &applied).map_err(fail)?;
        let created = apply_one(&mut draft, default_scene, &command).map_err(fail)?;
        validate_project(&draft).map_err(|e| fail(e.to_string()))?;
        applied.push(Applied {
            op: command.op(),
            created,
        });
    }
    *project = draft;
    Ok(applied)
}

/// Applies `commands` like [`apply`] and answers as an agent reads the result: see [`report`].
pub fn apply_and_report(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
) -> Result<Json, CommandError> {
    let before = batch_warnings(project);
    let applied = apply(project, default_scene, commands)?;
    Ok(report(project, &applied, &before))
}

/// A batch's result for agents: `created` maps the index of each command that created
/// something to the IDs it created, in order (a preset's root first), with their names (a
/// node's is its operation); `warnings`, present when the batch added any, lists what the
/// game will ignore without failing and the action names it left ambiguous (see
/// [`batch_warnings`]); `before` are the project's [`batch_warnings`] before the batch, which
/// are not repeated.
pub fn report(project: &Project, applied: &[Applied], before: &[Warning]) -> Json {
    let wanted: HashSet<&str> = applied
        .iter()
        .flat_map(|a| a.created.iter().map(String::as_str))
        .collect();
    let mut names: HashMap<&str, String> = HashMap::new();
    let mut found = |id: &str, name: &str| {
        if let Some(id) = wanted.get(id) {
            names.insert(id, name.to_owned());
        }
    };
    for scene in &project.scenes {
        found(&scene.id, &scene.name);
    }
    for (_, e) in every_entity(project) {
        found(&e.id, &e.name);
        for node in &e.graph.nodes {
            found(&node.id, &node.operation);
        }
        for clip in &e.clips {
            found(&clip.id, &clip.name);
        }
    }
    for id in project.input_bindings.keys() {
        found(id, &input_actions::label(project, id));
    }
    for surface in &project.surfaces {
        found(&surface.id, &surface.name);
    }
    for asset in &project.assets {
        found(&asset.id, &asset.name);
    }
    let created: Map<String, Json> = applied
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.created.is_empty())
        .map(|(index, a)| {
            let ids: Vec<Json> = a
                .created
                .iter()
                .map(|id| json!({"id": id, "name": names.get(id.as_str())}))
                .collect();
            (index.to_string(), Json::Array(ids))
        })
        .collect();
    let mut result = json!({"created": created});
    let new: Vec<String> = batch_warnings(project)
        .into_iter()
        .filter(|w| !before.iter().any(|old| old.about == w.about))
        .map(|w| w.message)
        .collect();
    if !new.is_empty() {
        result["warnings"] = json!(new);
    }
    result
}

/// Something the game will ignore without failing, or that a batch should hear about; see
/// [`batch_warnings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// What it is about, by IDs and values only, so renaming an object does not make it new.
    pub about: String,
    pub message: String,
}

/// What the game will ignore without failing, so validation lets it through: input actions
/// bound to keys the game does not read (or keeps, Escape and F3), logic nodes that name an
/// input action the project does not have, and fixed-choice parameters holding another value
/// (set by update_entity or by hand).
pub fn warnings(project: &Project) -> Vec<Warning> {
    let mut found = Vec::new();
    for (id, key) in &project.input_bindings {
        if let Err(error) = input_actions::check_key(key) {
            found.push(Warning {
                about: format!("key {id} {key}"),
                message: format!(
                    "Ação de entrada {} ({id}): {error}",
                    input_actions::label(project, id)
                ),
            });
        }
    }
    for (scope, e) in every_entity(project) {
        for node in &e.graph.nodes {
            let place = || format!("{scope} → {} → nó {} ({})", e.name, node.operation, node.id);
            let action = node.text("action");
            if matches!(node.operation.as_str(), "event.input" | "input.read")
                && !project.input_bindings.contains_key(action)
            {
                found.push(Warning {
                    about: format!("action {} {action}", node.id),
                    message: format!(
                        "{}: a ação de entrada \"{action}\" não existe no projeto",
                        place()
                    ),
                });
            }
            // An empty mode is an event.input never configured, which the game reads as
            // pressed; any other empty choice never matches, like any other wrong value.
            let set = node.params.iter().filter(|(key, v)| {
                !(node.operation == "event.input"
                    && key.as_str() == "mode"
                    && matches!(v, Value::Text(t) if t.is_empty()))
            });
            if let Err(error) = graph::check_choices(&node.operation, set) {
                found.push(Warning {
                    about: format!("choice {} {error}", node.id),
                    message: format!("{}: {error}", place()),
                });
            }
        }
    }
    found
}

/// [`warnings`], plus the input action names that stand for several actions. By name,
/// commands and simulations refuse a name two actions share and take first the action whose
/// ID it is. create and rename keep names apart, but the default actions of presets and
/// components (Pular, Correr…) arrive without that check. The game itself tells them apart,
/// and projects may share a name on purpose (one label for keys 1, 2 and 3), so only the batch
/// that leaves a name ambiguous reports it, not validation.
pub fn batch_warnings(project: &Project) -> Vec<Warning> {
    let mut found = warnings(project);
    let mut named: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for id in project.input_bindings.keys() {
        let label = input_actions::label(project, id).trim().to_lowercase();
        for name in [label, id.to_lowercase()] {
            let ids = named.entry(name).or_default();
            if !ids.contains(&id.as_str()) {
                ids.push(id);
            }
        }
    }
    for (name, ids) in named.into_iter().filter(|(_, ids)| ids.len() > 1) {
        let listed: Vec<String> = ids
            .iter()
            .map(|id| format!("{id} ({})", input_actions::label(project, id)))
            .collect();
        found.push(Warning {
            about: format!("action name {name} {}", ids.join(" ")),
            message: format!(
                "{} ações de entrada atendem pelo nome \"{name}\": {}. Pelo nome, os comandos e o \
                 oxy_simulate não as distinguem; renomeie uma delas ou use o ID",
                ids.len(),
                listed.join(", ")
            ),
        });
    }
    found
}

/// Parses a JSON array (or a single object) of commands.
pub fn parse(json: &Json) -> Result<Vec<Command>, String> {
    let list = match json {
        Json::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    list.into_iter()
        .enumerate()
        .map(|(i, item)| {
            serde_json::from_value(item)
                .map_err(|e| format!("Comando {} não reconhecido: {e}", i + 1))
        })
        .collect()
}

/// A text that stands for an ID created earlier in a batch. Only a whole text of this form
/// counts, so "$10 de bônus" stays text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholder<'a> {
    /// `"$N"` (`nth` 0) or `"$N.M"`: the M-th ID created by command N, both counted from 0.
    Created { command: usize, nth: usize },
    /// `"$$N"` or `"$$N.M"`: the text itself without its first '$', such as "$10".
    Escaped(&'a str),
}
impl<'a> Placeholder<'a> {
    pub fn parse(text: &'a str) -> Option<Self> {
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let pattern = |rest: &str| match rest.split_once('.') {
            Some((command, nth)) => digits(command) && digits(nth),
            None => digits(rest),
        };
        if let Some(rest) = text.strip_prefix("$$") {
            return pattern(rest).then(|| Self::Escaped(&text[1..]));
        }
        let rest = text.strip_prefix('$').filter(|rest| pattern(rest))?;
        let (command, nth) = rest.split_once('.').unwrap_or((rest, "0"));
        // A number too long for usize names a command that cannot exist.
        let number = |s: &str| s.parse().unwrap_or(usize::MAX);
        Some(Self::Created {
            command: number(command),
            nth: number(nth),
        })
    }
    /// The ID or text it stands for after the commands `applied`; `None` when command N did
    /// not run before or created fewer IDs.
    pub fn resolve(self, applied: &[Applied]) -> Option<String> {
        match self {
            Self::Created { command, nth } => applied.get(command)?.created.get(nth).cloned(),
            Self::Escaped(text) => Some(text.into()),
        }
    }
}

/// Replaces placeholders anywhere in `command`, patches included.
fn resolve_placeholders(command: &Command, applied: &[Applied]) -> Result<Command, String> {
    fn walk(value: &mut Json, applied: &[Applied], changed: &mut bool) -> Result<(), String> {
        match value {
            Json::String(text) => {
                if let Some(placeholder) = Placeholder::parse(text) {
                    *text = placeholder.resolve(applied).ok_or_else(|| {
                        format!(
                            "\"{text}\" não se refere a um ID criado antes no lote (para o \
                             texto \"{text}\" em si, escreva \"${text}\")"
                        )
                    })?;
                    *changed = true;
                }
            }
            Json::Array(items) => {
                for item in items {
                    walk(item, applied, changed)?;
                }
            }
            Json::Object(map) => {
                for item in map.values_mut() {
                    walk(item, applied, changed)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut value = serde_json::to_value(command).map_err(|e| e.to_string())?;
    let mut changed = false;
    walk(&mut value, applied, &mut changed)?;
    if !changed {
        return Ok(command.clone());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn scene_id(project: &Project, scene: &Option<Id>, default: &str) -> Result<Id, String> {
    let id = scene.as_deref().unwrap_or(default);
    project
        .scene(id)
        .map(|s| s.id.clone())
        .ok_or_else(|| format!("Cena {id} não existe"))
}
fn scene_mut<'a>(project: &'a mut Project, id: &str) -> &'a mut Scene {
    project.scene_mut(id).expect("scene checked by scene_id")
}
fn entity_mut<'a>(scene: &'a mut Scene, id: &str) -> Result<&'a mut Entity, String> {
    scene
        .entity_mut(id)
        .ok_or_else(|| format!("Objeto {id} não existe nesta cena"))
}
fn check_entities(scene: &Scene, ids: &[Id]) -> Result<(), String> {
    if ids.is_empty() {
        return Err("Informe ao menos um objeto".into());
    }
    match ids.iter().find(|id| scene.entity(id).is_none()) {
        Some(id) => Err(format!("Objeto {id} não existe nesta cena")),
        None => Ok(()),
    }
}
fn finite(values: &[f32], what: &str) -> Result<(), String> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(format!("{what} precisa de números finitos"))
    }
}

fn apply_one(project: &mut Project, default: &str, command: &Command) -> Result<Vec<Id>, String> {
    use Command as C;
    match command {
        C::RenameProject { name } => {
            if name.trim().is_empty() {
                return Err("Informe um nome para o projeto".into());
            }
            project.name = name.trim().into();
            Ok(vec![])
        }
        C::SetStartScene { scene } => {
            let id = scene_id(project, &Some(scene.clone()), default)?;
            project.start_scene = id;
            Ok(vec![])
        }
        C::CreateScene { name, kind } => {
            if name.trim().is_empty() {
                return Err("Informe um nome para a cena".into());
            }
            let scene = Scene::new(name.trim(), (*kind).into());
            let id = scene.id.clone();
            project.scenes.push(scene);
            Ok(vec![id])
        }
        C::RenameScene { scene, name } => {
            if name.trim().is_empty() {
                return Err("Informe um nome para a cena".into());
            }
            let id = scene_id(project, &Some(scene.clone()), default)?;
            scene_mut(project, &id).name = name.trim().into();
            Ok(vec![])
        }
        C::DuplicateScene { scene } => {
            let id = scene_id(project, &Some(scene.clone()), default)?;
            Ok(vec![editing::duplicate_scene(project, &id)?])
        }
        C::DeleteScene { scene } => {
            let id = scene_id(project, &Some(scene.clone()), default)?;
            editing::delete_scene(project, &id)?;
            Ok(vec![])
        }
        C::CreateEntity {
            scene,
            kind,
            name,
            parent,
            position,
        } => {
            let id = scene_id(project, scene, default)?;
            create_entity(
                project,
                &id,
                kind,
                name.as_deref(),
                parent.as_ref(),
                *position,
            )
        }
        C::DeleteEntities { scene, ids } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            check_entities(scene, ids)?;
            for root in editing::selection_roots(scene, ids) {
                scene.remove_subtree(&root);
            }
            Ok(vec![])
        }
        C::DuplicateEntities {
            scene,
            ids,
            offset,
            count,
        } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            check_entities(scene, ids)?;
            editing::duplicate_repeated(
                scene,
                ids,
                offset.unwrap_or(editing::DUPLICATE_OFFSET),
                count.unwrap_or(1),
            )
        }
        C::CopyEntities {
            scene,
            ids,
            to_scene,
            parent,
        } => {
            let from = scene_id(project, scene, default)?;
            let to = scene_id(project, &Some(to_scene.clone()), default)?;
            copy_entities(project, &from, ids, &to, parent.as_ref())
        }
        C::SaveModel {
            scene,
            entity,
            name,
        } => {
            let id = scene_id(project, scene, default)?;
            let source = project.scene(&id).unwrap();
            check_entities(source, std::slice::from_ref(entity))?;
            let name = match name {
                Some(name) if name.trim().is_empty() => {
                    return Err("Informe um nome para o modelo".into());
                }
                Some(name) => name.trim().to_owned(),
                None => source.entity(entity).unwrap().name.clone(),
            };
            let pieces: HashSet<Id> = source.descendants(entity).into_iter().collect();
            refuse_outside(source, &pieces, "do modelo")?;
            Ok(vec![project.save_model(&id, entity, name)?])
        }
        C::PlaceModel {
            scene,
            model,
            parent,
            position,
            name,
        } => {
            let id = scene_id(project, scene, default)?;
            place_model(
                project,
                &id,
                model,
                parent.as_ref(),
                *position,
                name.as_deref(),
            )
        }
        C::RenameEntity {
            scene,
            entity,
            name,
        } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            check_entities(scene, std::slice::from_ref(entity))?;
            editing::rename_entity(scene, entity, name)?;
            Ok(vec![])
        }
        C::SetParent { scene, ids, parent } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            check_entities(scene, ids)?;
            if let Some(parent) = parent {
                check_entities(scene, std::slice::from_ref(parent))?;
            }
            editing::reparent_selection(scene, ids, parent.clone())?;
            Ok(vec![])
        }
        C::SetTransform {
            scene,
            entity,
            position,
            rotation,
            scale,
            move_tracks,
        } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            let e = entity_mut(scene, entity)?;
            let before = e.transform.clone();
            if let Some(p) = position {
                finite(p, "Posição")?;
                e.transform.position = *p;
            }
            if let Some(r) = rotation {
                finite(r, "Rotação")?;
                e.transform.rotation = r.map(f32::to_radians);
            }
            if let Some(s) = scale {
                finite(s, "Escala")?;
                if s.iter().any(|v| v.abs() < 0.0001) {
                    return Err("Escala não pode ser zero".into());
                }
                e.transform.scale = *s;
            }
            let after = e.transform.clone();
            if move_tracks.unwrap_or_else(|| editing::animates_itself(e)) && after != before {
                editing::carry_tracks(&mut scene.entities, entity, &before, &after);
            }
            Ok(vec![])
        }
        C::SetClip {
            scene,
            entity,
            clip,
            name,
            duration,
            looping,
            tracks,
            events,
        } => {
            let id = scene_id(project, scene, default)?;
            set_clip(
                scene_mut(project, &id),
                entity,
                clip.as_deref(),
                ClipSpec {
                    name,
                    duration: *duration,
                    looping: *looping,
                    tracks,
                    events,
                },
            )
        }
        C::DeleteClip {
            scene,
            entity,
            clip,
        } => {
            let id = scene_id(project, scene, default)?;
            let owner = project
                .scene(&id)
                .unwrap()
                .entity(entity)
                .ok_or_else(|| format!("Objeto {entity} não existe nesta cena"))?;
            let index = find_clip(owner, clip)?;
            let found = &owner.clips[index];
            let uses = clip_uses(project, &found.id);
            if !uses.is_empty() {
                return Err(format!(
                    "Altere estes nós antes de excluir a animação {}:\n{}",
                    found.name,
                    uses.join("\n")
                ));
            }
            entity_mut(scene_mut(project, &id), entity)?
                .clips
                .remove(index);
            Ok(vec![])
        }
        C::SetVisible {
            scene,
            entity,
            visible,
        } => {
            let id = scene_id(project, scene, default)?;
            entity_mut(scene_mut(project, &id), entity)?.visible = *visible;
            Ok(vec![])
        }
        C::SetMaterial {
            scene,
            entity,
            color,
            texture,
            clear_texture,
        } => {
            let id = scene_id(project, scene, default)?;
            if let Some(texture) = texture
                && project
                    .asset(texture)
                    .is_none_or(|a| a.kind != AssetKind::Texture)
            {
                return Err(format!("Textura {texture} não existe no projeto"));
            }
            let e = entity_mut(scene_mut(project, &id), entity)?;
            if let Some(color) = color {
                finite(color, "Cor")?;
                e.material.color = *color;
            }
            if *clear_texture {
                e.material.texture = None;
            } else if let Some(texture) = texture {
                e.material.texture = Some(texture.clone());
            }
            Ok(vec![])
        }
        C::SetAttribute {
            scene,
            entity,
            name,
            value,
        } => {
            if name.trim().is_empty() {
                return Err("Informe o nome do atributo".into());
            }
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            match value {
                Some(value) => {
                    e.attributes.insert(name.trim().into(), value.clone());
                }
                None => {
                    if e.attributes.remove(name.trim()).is_none() {
                        return Err(format!("O objeto não tem o atributo {name}"));
                    }
                }
            }
            Ok(vec![])
        }
        C::AddComponent {
            scene,
            entity,
            component,
        } => {
            let id = scene_id(project, scene, default)?;
            let kind = project.scene(&id).unwrap().kind;
            let mut next = project
                .scene(&id)
                .unwrap()
                .entity(entity)
                .cloned()
                .ok_or_else(|| format!("Objeto {entity} não existe nesta cena"))?;
            authoring::add(kind, project, &mut next, *component)?;
            *entity_mut(scene_mut(project, &id), entity)? = next;
            Ok(vec![])
        }
        C::RemoveComponent {
            scene,
            entity,
            component,
        } => {
            let id = scene_id(project, scene, default)?;
            authoring::remove(entity_mut(scene_mut(project, &id), entity)?, *component)?;
            Ok(vec![])
        }
        C::ApplyMovementProfile {
            scene,
            entity,
            profile,
        } => {
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let name = e.name.clone();
            e.character3d
                .as_mut()
                .ok_or_else(|| {
                    format!("{name} não é um personagem 3D: adicione o componente character antes")
                })?
                .apply_profile(*profile);
            Ok(vec![])
        }
        C::UpdateEntity {
            scene,
            entity,
            patch,
        } => {
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let mut value = serde_json::to_value(&*e).map_err(|e| e.to_string())?;
            merge_patch(&mut value, patch);
            if value.get("id").and_then(Json::as_str) != Some(entity.as_str()) {
                return Err("O patch não pode mudar o ID do objeto".into());
            }
            *e = serde_json::from_value(value).map_err(|e| format!("Patch inválido: {e}"))?;
            Ok(vec![])
        }
        C::AddNode {
            scene,
            entity,
            operation,
            position,
            params,
        } => {
            let id = scene_id(project, scene, default)?;
            let def = graph::registry()
                .iter()
                .find(|op| op.id == operation)
                .ok_or_else(|| format!("Nó {operation} não existe no catálogo"))?;
            if let Some(key) = params
                .keys()
                .find(|k| !def.params.iter().any(|p| p.id == *k))
            {
                return Err(format!("O nó {operation} não tem o parâmetro {key}"));
            }
            graph::check_choices(operation, params)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let position = position.unwrap_or_else(|| {
                // To the right of the rightmost node, so new nodes do not overlap.
                let x = e
                    .graph
                    .nodes
                    .iter()
                    .map(|n| n.position[0])
                    .fold(f32::NEG_INFINITY, f32::max);
                if x.is_finite() {
                    [x + 260., 0.]
                } else {
                    [0., 0.]
                }
            });
            finite(&position, "Posição do nó")?;
            let mut node = Node::new(operation, position);
            node.params.extend(params.clone());
            let node_id = node.id.clone();
            e.graph.nodes.push(node);
            Ok(vec![node_id])
        }
        C::RemoveNode {
            scene,
            entity,
            node,
        } => {
            let id = scene_id(project, scene, default)?;
            let graph = &mut entity_mut(scene_mut(project, &id), entity)?.graph;
            let before = graph.nodes.len();
            graph.nodes.retain(|n| n.id != *node);
            if graph.nodes.len() == before {
                return Err(format!("Nó {node} não existe neste objeto"));
            }
            graph
                .edges
                .retain(|edge| edge.from_node != *node && edge.to_node != *node);
            Ok(vec![])
        }
        C::SetNodeParams {
            scene,
            entity,
            node,
            params,
        } => {
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let target = e
                .graph
                .nodes
                .iter_mut()
                .find(|n| n.id == *node)
                .ok_or_else(|| format!("Nó {node} não existe neste objeto"))?;
            let def = graph::registry()
                .iter()
                .find(|op| op.id == target.operation)
                .ok_or("Nó com operação desconhecida")?;
            if let Some(key) = params
                .keys()
                .find(|k| !def.params.iter().any(|p| p.id == *k))
            {
                return Err(format!(
                    "O nó {} não tem o parâmetro {key}",
                    target.operation
                ));
            }
            graph::check_choices(&target.operation, params)?;
            target.params.extend(params.clone());
            Ok(vec![])
        }
        C::Connect {
            scene,
            entity,
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let id = scene_id(project, scene, default)?;
            let objects: HashSet<_> = project
                .scene(&id)
                .unwrap()
                .entities
                .iter()
                .map(|e| e.id.clone())
                .collect();
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let edge = Edge {
                from_node: from_node.clone(),
                from_port: from_port.clone(),
                to_node: to_node.clone(),
                to_port: to_port.clone(),
            };
            if e.graph.edges.contains(&edge) {
                return Err("Essas portas já estão ligadas".into());
            }
            e.graph.connect(&edge, &objects)?;
            Ok(vec![])
        }
        C::Disconnect {
            scene,
            entity,
            from_node,
            from_port,
            to_node,
            to_port,
        } => {
            let id = scene_id(project, scene, default)?;
            let graph = &mut entity_mut(scene_mut(project, &id), entity)?.graph;
            let before = graph.edges.len();
            graph.edges.retain(|edge| {
                !(edge.from_node == *from_node
                    && edge.from_port == *from_port
                    && edge.to_node == *to_node
                    && edge.to_port == *to_port)
            });
            if graph.edges.len() == before {
                return Err("Essas portas não estão ligadas".into());
            }
            Ok(vec![])
        }
        C::CreateInputAction { name, key } => {
            let key = input_actions::check_key(key)?;
            unique_action_name(project, name, None)?;
            Ok(vec![input_actions::create(project, name, key)?])
        }
        C::RenameInputAction { action, name } => {
            let action = input_actions::find(project, action)?;
            unique_action_name(project, name, Some(&action))?;
            input_actions::rename(project, &action, name)?;
            Ok(vec![])
        }
        C::SetInputKey { action, key } => {
            let action = input_actions::find(project, action)?;
            let key = input_actions::check_key(key)?;
            project.input_bindings.insert(action, key.into());
            Ok(vec![])
        }
        C::DeleteInputAction { action } => {
            let action = input_actions::find(project, action)?;
            input_actions::remove(project, &action)?;
            Ok(vec![])
        }
        C::CreateSurface {
            preset,
            name,
            friction,
            traction,
            speed_multiplier,
            conveyor,
            conveyor_space,
        } => {
            let mut created = SurfaceMaterial::preset(preset.unwrap_or(SurfacePreset::Common));
            SurfaceValues {
                name,
                friction: *friction,
                traction: *traction,
                speed_multiplier: *speed_multiplier,
                conveyor: *conveyor,
                conveyor_space: *conveyor_space,
            }
            .set(&mut created)?;
            let id = created.id.clone();
            project.surfaces.push(created);
            Ok(vec![id])
        }
        C::UpdateSurface {
            surface,
            name,
            friction,
            traction,
            speed_multiplier,
            conveyor,
            conveyor_space,
        } => {
            let target = surface_mut(project, surface)?;
            SurfaceValues {
                name,
                friction: *friction,
                traction: *traction,
                speed_multiplier: *speed_multiplier,
                conveyor: *conveyor,
                conveyor_space: *conveyor_space,
            }
            .set(target)?;
            Ok(vec![])
        }
        C::DeleteSurface { surface } => {
            surface_mut(project, surface)?;
            surface::remove(project, surface)?;
            Ok(vec![])
        }
    }
}

/// Two actions with one name could not be told apart by name, as simulations allow, and a
/// name that is another action's ID would reach that action, which [`input_actions::find`]
/// takes first.
fn unique_action_name(project: &Project, name: &str, renamed: Option<&str>) -> Result<(), String> {
    let wanted = name.trim().to_lowercase();
    let mut others = project
        .input_bindings
        .keys()
        .filter(|id| Some(id.as_str()) != renamed);
    if let Some(id) = others
        .clone()
        .find(|id| input_actions::label(project, id).to_lowercase() == wanted)
    {
        return Err(format!(
            "Já existe uma ação de entrada chamada \"{}\" ({id})",
            name.trim()
        ));
    }
    match others.find(|id| id.to_lowercase() == wanted) {
        Some(id) => Err(format!(
            "\"{}\" é o ID da ação de entrada {}; por esse nome, os comandos e o oxy_simulate \
             chegariam a ela. Escolha outro nome",
            name.trim(),
            input_actions::label(project, id)
        )),
        None => Ok(()),
    }
}

fn surface_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut SurfaceMaterial, String> {
    project
        .surfaces
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Superfície {id} não existe no projeto"))
}

/// The values a surface command sets; absent ones stay as they are.
struct SurfaceValues<'a> {
    name: &'a Option<String>,
    friction: Option<f32>,
    traction: Option<f32>,
    speed_multiplier: Option<f32>,
    conveyor: Option<[f32; 3]>,
    conveyor_space: Option<VelocitySpace>,
}
impl SurfaceValues<'_> {
    fn set(&self, surface: &mut SurfaceMaterial) -> Result<(), String> {
        if let Some(name) = self.name {
            if name.trim().is_empty() {
                return Err("Informe um nome para a superfície".into());
            }
            surface.name = name.trim().into();
        }
        for (value, field) in [
            (self.friction, &mut surface.friction),
            (self.traction, &mut surface.traction),
            (self.speed_multiplier, &mut surface.speed_multiplier),
        ] {
            if let Some(value) = value {
                *field = value;
            }
        }
        if let Some(conveyor) = self.conveyor {
            surface.conveyor = conveyor;
        }
        if let Some(space) = self.conveyor_space {
            surface.conveyor_space = space;
        }
        surface.validate()
    }
}

fn create_entity(
    project: &mut Project,
    scene: &str,
    kind: &EntityKind,
    name: Option<&str>,
    parent: Option<&Id>,
    position: Option<[f32; 3]>,
) -> Result<Vec<Id>, String> {
    let scene_kind = project.scene(scene).unwrap().kind;
    if let Some(parent) = parent {
        check_entities(project.scene(scene).unwrap(), std::slice::from_ref(parent))?;
    }
    if let Some(p) = position {
        finite(&p, "Posição")?;
    }
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let mut entity = match kind {
        EntityKind::Preset {
            preset,
            travel,
            period,
            camera,
        } => {
            let platform = *preset == Preset::Platform;
            if !platform && (travel.is_some() || period.is_some()) {
                return Err("travel e period valem só para o preset platform".into());
            }
            if platform && camera.is_some() {
                return Err("camera vale só para os presets first_person e third_person".into());
            }
            if let Some(travel) = travel {
                finite(travel, "travel")?;
            }
            let defaults = PresetOptions::default();
            let options = PresetOptions {
                travel: travel.map_or(defaults.travel, Vec3::from),
                period: period.unwrap_or(defaults.period),
                camera: camera.unwrap_or_default(),
            };
            let preset = match preset {
                Preset::FirstPerson => MovementPreset::FirstPerson,
                Preset::ThirdPerson => MovementPreset::ThirdPerson,
                Preset::Platform => MovementPreset::Platform,
            };
            let at = position.unwrap_or([0., 0.02, 0.]);
            let before = scene_mut(project, scene).entities.len();
            let id =
                movement_presets::create_with(project, scene, preset, Vec3::from(at), options)?;
            let e = entity_mut(scene_mut(project, scene), &id)?;
            if let Some(name) = name {
                e.name = name.into();
            }
            e.parent = parent.cloned();
            // Every piece of the assembly, its root first.
            let mut created: Vec<Id> = scene_mut(project, scene).entities[before..]
                .iter()
                .map(|e| e.id.clone())
                .collect();
            created.sort_by_key(|piece| *piece != id);
            return Ok(created);
        }
        EntityKind::Group => Entity::new(name.unwrap_or("Grupo"), None),
        EntityKind::Shape { shape } => {
            if flat(*shape) != (scene_kind == SceneKind::TwoD) {
                return Err(format!(
                    "A forma {shape:?} não pertence a uma cena {}",
                    if scene_kind == SceneKind::TwoD {
                        "2D"
                    } else {
                        "3D"
                    }
                ));
            }
            let mut e = Entity::new(name.unwrap_or(shape_label(*shape)), Some(*shape));
            e.segments = match shape {
                Primitive::Pyramid => 4,
                Primitive::Sphere | Primitive::Circle => 16,
                _ => 8,
            };
            e.primitive_parameters = Some(crate::geometry::primitives::Parameters::default());
            e
        }
        EntityKind::Camera => {
            let mut e = Entity::new(name.unwrap_or("Câmera"), None);
            e.camera = Some(Camera::default());
            e
        }
        EntityKind::Ui { element } => {
            let mut e = Entity::new(name.unwrap_or(ui_label(*element)), None);
            e.ui = Some(UiElement {
                kind: *element,
                ..Default::default()
            });
            e
        }
    };
    entity.parent = parent.cloned();
    if let Some(p) = position {
        entity.transform.position = p;
    }
    let id = entity.id.clone();
    scene_mut(project, scene).entities.push(entity);
    Ok(vec![id])
}

/// Whether the shape belongs to 2D scenes.
fn flat(shape: Primitive) -> bool {
    matches!(
        shape,
        Primitive::Rectangle | Primitive::Circle | Primitive::Sprite
    )
}

/// Shapes go only to scenes of their kind, as create_entity requires; validation checks the
/// components.
fn check_kind(entities: &[Entity], kind: SceneKind, scene: &str) -> Result<(), String> {
    let wanted = kind == SceneKind::TwoD;
    for e in entities {
        let two_d = match (e.primitive, &e.mesh) {
            (Some(shape), _) => flat(shape),
            (None, Some(_)) => false,
            (None, None) => continue,
        };
        if two_d != wanted {
            let [is, scene_is] = if two_d { ["2D", "3D"] } else { ["3D", "2D"] };
            return Err(format!(
                "{} é uma forma {is}, e a cena {scene} é {scene_is}",
                e.name
            ));
        }
    }
    Ok(())
}

/// Refuses to take `pieces` out of `scene`, into a model or another scene, while they point to
/// objects or animations left behind, which would not exist where they go. `what` completes
/// "fora": "do modelo", "da cópia".
fn refuse_outside(scene: &Scene, pieces: &HashSet<Id>, what: &str) -> Result<(), String> {
    let named = |id: &str| {
        scene
            .entity(id)
            .map_or_else(|| id.to_owned(), |e| format!("{} ({id})", e.name))
    };
    let taken: Vec<&Entity> = scene
        .entities
        .iter()
        .filter(|e| pieces.contains(&e.id))
        .collect();
    let clips: HashSet<&str> = taken
        .iter()
        .flat_map(|e| e.clips.iter().map(|clip| clip.id.as_str()))
        .collect();
    let mut found = Vec::new();
    for e in taken {
        // What the object points to: (where, target).
        let mut links: Vec<(String, &str)> = Vec::new();
        for (key, value) in &e.attributes {
            if let Some(target) = value.object() {
                links.push((format!("atributo {key}"), target));
            }
        }
        if let Some(target) = e.ui.as_ref().and_then(|ui| ui.binding_object.as_deref()) {
            links.push(("vínculo da interface".into(), target));
        }
        if let Some(rig) = &e.camera_rig {
            if let Some(target) = rig.target.as_deref() {
                links.push(("alvo da câmera".into(), target));
            }
            for hidden in &rig.hidden {
                links.push(("peça oculta pela câmera".into(), hidden));
            }
        }
        for node in &e.graph.nodes {
            for (key, value) in &node.params {
                if let Some(target) = value.object() {
                    links.push((format!("nó {} ({key})", node.operation), target));
                }
            }
            let clip = node.text("clip");
            if node.operation == "action.animation" && !clip.is_empty() && !clips.contains(clip) {
                found.push(format!(
                    "{} → nó action.animation → animação de outro objeto ({clip})",
                    e.name
                ));
            }
        }
        for clip in &e.clips {
            for track in &clip.tracks {
                links.push((format!("animação {}", clip.name), &track.target));
            }
        }
        for (link, target) in links {
            if !pieces.contains(target) {
                found.push(format!("{} → {link} → {}", e.name, named(target)));
            }
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    let more = found.len().saturating_sub(10);
    found.truncate(10);
    if more > 0 {
        found.push(format!("… e mais {more}"));
    }
    Err(format!(
        "Estas referências apontam para fora {what}; leve junto os objetos e animações \
         que elas usam ou tire as referências antes:\n{}",
        found.join("\n")
    ))
}

/// Copies `ids` of the scene `from`, with everything under them, to the scene `to`. The copied
/// roots go under `parent` (or to the root of `to`) keeping their world transform, and the
/// keyframes that animate them follow. Answers the new roots, in order.
fn copy_entities(
    project: &mut Project,
    from: &str,
    ids: &[Id],
    to: &str,
    parent: Option<&Id>,
) -> Result<Vec<Id>, String> {
    let source = project.scene(from).unwrap();
    let destination = project.scene(to).unwrap();
    check_entities(source, ids)?;
    if let Some(parent) = parent
        && destination.entity(parent).is_none()
    {
        return Err(format!(
            "Objeto {parent} não existe na cena {}",
            destination.name
        ));
    }
    let roots = editing::selection_roots(source, ids);
    let copied: HashSet<Id> = roots.iter().flat_map(|id| source.descendants(id)).collect();
    if from != to {
        refuse_outside(source, &copied, "da cópia")?;
    }
    let mut copies: Vec<Entity> = source
        .entities
        .iter()
        .filter(|e| copied.contains(&e.id))
        .cloned()
        .collect();
    check_kind(&copies, destination.kind, &destination.name)?;
    let parent_world = match parent {
        Some(parent) => destination.world_matrix(parent)?,
        None => Mat4::IDENTITY,
    };
    if parent_world.determinant().abs() < 1e-8 {
        return Err("A transformação do pai de destino não pode ser invertida".into());
    }
    let worlds = roots
        .iter()
        .map(|id| source.world_matrix(id))
        .collect::<Result<Vec<_>, _>>()?;
    let map: HashMap<_, _> = copies.iter().map(|e| (e.id.clone(), new_id())).collect();
    remap_entities(&mut copies, &map)?;
    for (root, world) in roots.iter().zip(worlds) {
        let id = &map[root];
        let entity = copies.iter_mut().find(|e| e.id == *id).unwrap();
        let before = entity.transform.clone();
        let local = parent_world.inverse() * world;
        // An unchanged local transform stays exact, without the rounding of the matrices.
        let after = if before.matrix().abs_diff_eq(local, 1e-6) {
            before.clone()
        } else {
            let transform = Transform::from_matrix(local, before.pivot);
            if !transform.finite() || !transform.matrix().abs_diff_eq(local, 0.0001) {
                return Err(format!(
                    "{} ficaria deformado sob o pai de destino (cisalhamento); escolha outro \
                     pai ou ajuste a escala não uniforme dele",
                    entity.name
                ));
            }
            transform
        };
        entity.parent = parent.cloned();
        entity.transform = after.clone();
        editing::carry_tracks(&mut copies, id, &before, &after);
    }
    let created = roots.iter().map(|root| map[root].clone()).collect();
    scene_mut(project, to).entities.extend(copies);
    Ok(created)
}

/// A model asset by ID or, when no asset has that ID, by name (ignoring case).
fn find_model(project: &Project, model: &str) -> Result<Id, String> {
    if let Some(asset) = project.asset(model) {
        return if asset.kind == AssetKind::Model && asset.model.is_some() {
            Ok(asset.id.clone())
        } else {
            Err(format!("O recurso {} não é um modelo", asset.name))
        };
    }
    let wanted = model.trim().to_lowercase();
    let named: Vec<&Asset> = project
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::Model && a.name.trim().to_lowercase() == wanted)
        .collect();
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(format!("Modelo {model} não existe no projeto")),
        many => Err(format!(
            "{} modelos se chamam \"{}\"; use o ID",
            many.len(),
            model.trim()
        )),
    }
}

/// Places a copy of `model` (ID or name) in `scene`, its root under `parent` at `position`.
/// Answers every piece, the root first.
fn place_model(
    project: &mut Project,
    scene: &str,
    model: &str,
    parent: Option<&Id>,
    position: Option<[f32; 3]>,
    name: Option<&str>,
) -> Result<Vec<Id>, String> {
    let asset = find_model(project, model)?;
    let target = project.scene(scene).unwrap();
    if let Some(parent) = parent {
        check_entities(target, std::slice::from_ref(parent))?;
    }
    if let Some(p) = position {
        finite(&p, "Posição")?;
    }
    if name.is_some_and(|n| n.trim().is_empty()) {
        return Err("Informe um nome para o objeto".into());
    }
    let pieces = project
        .asset(&asset)
        .and_then(|a| a.model.as_deref())
        .unwrap_or_default();
    check_kind(pieces, target.kind, &target.name)?;
    let before = target.entities.len();
    let root = project.instantiate_model(&asset, scene)?;
    let placed = &mut scene_mut(project, scene).entities[before..];
    let index = placed
        .iter()
        .position(|e| e.id == root)
        .ok_or("Modelo sem raiz")?;
    if let Some(p) = position {
        let rest = placed[index].transform.clone();
        let mut moved = rest.clone();
        moved.position = p;
        placed[index].transform = moved.clone();
        editing::carry_tracks(placed, &root, &rest, &moved);
    }
    let e = &mut placed[index];
    e.parent = parent.cloned();
    if let Some(name) = name {
        e.name = name.trim().into();
    }
    let mut created: Vec<Id> = placed.iter().map(|e| e.id.clone()).collect();
    created.sort_by_key(|piece| *piece != root);
    Ok(created)
}

/// The index of `entity`'s animation `clip`, by ID or, failing that, by name (ignoring case).
fn find_clip(entity: &Entity, clip: &str) -> Result<usize, String> {
    let wanted = clip.trim().to_lowercase();
    entity
        .clips
        .iter()
        .position(|c| c.id == clip)
        .or_else(|| {
            entity
                .clips
                .iter()
                .position(|c| c.name.trim().to_lowercase() == wanted)
        })
        .ok_or_else(|| format!("{} não tem a animação {clip}", entity.name))
}

/// The logic nodes that play the animation `id`, in scenes and models.
fn clip_uses(project: &Project, id: &str) -> Vec<String> {
    every_entity(project)
        .flat_map(|(scope, e)| {
            e.graph
                .nodes
                .iter()
                .filter(|n| n.operation == "action.animation" && n.text("clip") == id)
                .map(move |n| format!("{scope} → {} → Reproduzir animação ({})", e.name, n.id))
        })
        .collect()
}

/// What [`Command::SetClip`] asks for, besides where.
struct ClipSpec<'a> {
    name: &'a str,
    duration: f32,
    looping: bool,
    tracks: &'a [TrackSpec],
    events: &'a [MarkerSpec],
}

/// Builds the animation `spec` of `owner`, or replaces its animation `replaced` (ID or name)
/// keeping the ID. Answers the new animation's ID, or nothing when it replaced one.
fn set_clip(
    scene: &mut Scene,
    owner: &str,
    replaced: Option<&str>,
    spec: ClipSpec,
) -> Result<Vec<Id>, String> {
    let entity = scene
        .entity(owner)
        .ok_or_else(|| format!("Objeto {owner} não existe nesta cena"))?;
    let name = spec.name.trim();
    if name.is_empty() {
        return Err("Informe um nome para a animação".into());
    }
    if !spec.duration.is_finite() || spec.duration <= 0. {
        return Err("A duração da animação precisa ser um número de segundos maior que 0".into());
    }
    let index = replaced.map(|clip| find_clip(entity, clip)).transpose()?;
    if let Some((_, other)) = entity.clips.iter().enumerate().find(|(i, clip)| {
        Some(*i) != index && clip.name.trim().to_lowercase() == name.to_lowercase()
    }) {
        return Err(format!(
            "{} já tem uma animação chamada \"{name}\" ({}); para substituí-la, informe clip",
            entity.name, other.id
        ));
    }
    let pieces: HashSet<Id> = scene.descendants(owner).into_iter().collect();
    let mut clip = Clip {
        id: index.map_or_else(new_id, |i| entity.clips[i].id.clone()),
        name: name.into(),
        duration: spec.duration,
        looping: spec.looping,
        tracks: Vec::new(),
        events: Vec::new(),
    };
    for track in spec.tracks {
        let target = match track.target.as_deref() {
            None | Some("self") => owner,
            Some(id) => id,
        };
        let piece = scene
            .entity(target)
            .filter(|_| pieces.contains(target))
            .ok_or_else(|| {
                format!(
                    "Uma trilha anima o próprio objeto (\"self\") ou uma peça dentro de {}; \
                     {target} não é",
                    entity.name
                )
            })?;
        if clip.tracks.iter().any(|t| t.target == target) {
            return Err(format!(
                "Duas trilhas animam {}; junte os quadros-chave numa trilha só",
                piece.name
            ));
        }
        if track.keyframes.is_empty() {
            return Err(format!("A trilha de {} não tem quadros-chave", piece.name));
        }
        let mut keyframes = Vec::with_capacity(track.keyframes.len());
        for key in &track.keyframes {
            check_time(key.time, spec.duration, "Quadro-chave")?;
            // Channels left out keep the rest pose, and the pivot stays the piece's.
            let mut transform = piece.transform.clone();
            if let Some(p) = key.position {
                finite(&p, "Posição do quadro-chave")?;
                transform.position = p;
            }
            if let Some(r) = key.rotation {
                finite(&r, "Rotação do quadro-chave")?;
                transform.rotation = r.map(f32::to_radians);
            }
            if let Some(s) = key.scale {
                finite(&s, "Escala do quadro-chave")?;
                if s.iter().any(|v| v.abs() < 0.0001) {
                    return Err("Escala não pode ser zero".into());
                }
                transform.scale = s;
            }
            keyframes.push(Keyframe {
                time: key.time,
                transform,
                interpolation: match key.interpolation {
                    Some(KeyInterpolation::Hold) => Interpolation::Hold,
                    _ => Interpolation::Linear,
                },
            });
        }
        keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
        if let Some(pair) = keyframes
            .windows(2)
            .find(|pair| pair[0].time >= pair[1].time)
        {
            return Err(format!(
                "A trilha de {} tem dois quadros-chave no tempo {} s",
                piece.name, pair[1].time
            ));
        }
        clip.tracks.push(Track {
            target: target.into(),
            keyframes,
        });
    }
    for event in spec.events {
        check_time(event.time, spec.duration, "Marcador")?;
        if event.name.trim().is_empty() {
            return Err("Informe o nome do marcador".into());
        }
        clip.events.push(AnimationEvent {
            time: event.time,
            name: event.name.trim().into(),
        });
    }
    clip.events.sort_by(|a, b| a.time.total_cmp(&b.time));
    let id = clip.id.clone();
    let clips = &mut scene.entity_mut(owner).unwrap().clips;
    match index {
        Some(i) => {
            clips[i] = clip;
            Ok(vec![])
        }
        None => {
            clips.push(clip);
            Ok(vec![id])
        }
    }
}

fn check_time(time: f32, duration: f32, what: &str) -> Result<(), String> {
    if time.is_finite() && (0. ..=duration).contains(&time) {
        Ok(())
    } else {
        Err(format!(
            "{what} em {time} s fica fora da animação, que dura {duration} s"
        ))
    }
}

fn shape_label(shape: Primitive) -> &'static str {
    match shape {
        Primitive::Rectangle => "Retângulo",
        Primitive::Circle => "Círculo",
        Primitive::Sprite => "Sprite",
        Primitive::Cube => "Cubo",
        Primitive::Sphere => "Esfera",
        Primitive::Cylinder => "Cilindro",
        Primitive::Plane => "Plano",
        Primitive::Pyramid => "Pirâmide",
        Primitive::Cone => "Cone",
        Primitive::Tube => "Tubo",
    }
}
fn ui_label(kind: UiKind) -> &'static str {
    match kind {
        UiKind::Text => "Texto de interface",
        UiKind::Image => "Imagem de interface",
        UiKind::Button => "Botão de interface",
        UiKind::Bar => "Barra de atributo",
    }
}

/// RFC 7386: objects merge recursively, `null` removes a key, anything else replaces.
fn merge_patch(target: &mut Json, patch: &Json) {
    match patch {
        Json::Object(changes) => {
            if !target.is_object() {
                *target = Json::Object(Default::default());
            }
            let map = target.as_object_mut().unwrap();
            for (key, value) in changes {
                if value.is_null() {
                    map.remove(key);
                } else {
                    merge_patch(map.entry(key.clone()).or_insert(Json::Null), value);
                }
            }
        }
        other => *target = other.clone(),
    }
}
