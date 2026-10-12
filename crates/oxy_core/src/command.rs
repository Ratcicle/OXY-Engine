//! Project commands: edits described as data, applied atomically and validated. The editor,
//! the MCP server and tests share them, so an agent edits a project through the same rules
//! as a person.
//!
//! A batch is all-or-nothing: commands run in order on a copy of the project, the project is
//! validated after each one, and only a fully valid copy replaces the original. A string
//! such as `"$0"` in a later command stands for the first ID created by command 0 (see
//! [`Placeholder`]).
use crate::{
    authoring::{self, ComponentKind},
    document::*,
    editing,
    graph::{self, Edge, Node},
    input_actions::{self, every_entity},
    movement_presets::{self, MovementPreset},
    surface::{self, SurfaceMaterial, SurfacePreset, VelocitySpace},
};
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
    },
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
    DuplicateEntities {
        #[serde(default)]
        scene: Option<Id>,
        ids: Vec<Id>,
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
    /// Local transform. Rotation is in degrees.
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
            Self::RenameEntity { .. } => "Renomear objeto",
            Self::SetParent { .. } => "Mudar pai",
            Self::SetTransform { .. } => "Transformar objeto",
            Self::SetVisible { .. } => "Mudar visibilidade",
            Self::SetMaterial { .. } => "Mudar material",
            Self::SetAttribute { .. } => "Mudar atributo",
            Self::AddComponent { .. } => "Adicionar componente",
            Self::RemoveComponent { .. } => "Remover componente",
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
        C::DuplicateEntities { scene, ids } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            check_entities(scene, ids)?;
            editing::duplicate_selection(scene, ids)
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
        } => {
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
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
        EntityKind::Preset { preset } => {
            let preset = match preset {
                Preset::FirstPerson => MovementPreset::FirstPerson,
                Preset::ThirdPerson => MovementPreset::ThirdPerson,
                Preset::Platform => MovementPreset::Platform,
            };
            let at = position.unwrap_or([0., 0.02, 0.]);
            let before = scene_mut(project, scene).entities.len();
            let id = movement_presets::create(project, scene, preset, glam::Vec3::from(at))?;
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
            let two_d = matches!(
                shape,
                Primitive::Rectangle | Primitive::Circle | Primitive::Sprite
            );
            if two_d != (scene_kind == SceneKind::TwoD) {
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
