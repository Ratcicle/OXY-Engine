//! Project commands: edits described as data, applied atomically and validated. The editor,
//! the MCP server and tests share them, so an agent edits a project through the same rules
//! as a person.
//!
//! A batch is all-or-nothing: commands run in order on a copy of the project, the project is
//! validated after each one, and only a fully valid copy replaces the original. A string
//! such as `"$0"` in a later command stands for the first ID created by command 0, and
//! `"$0@<id>"` for the copy command 0 made of `<id>` (see [`Placeholder`]); commands are
//! counted from 0 in errors and answers too. Nodes added without a position are laid out when
//! the batch ends ([`graph::arrange`]).
use crate::{
    animation::{AnimationEvent, Clip, Interpolation, Keyframe, Track},
    authoring::{self, ComponentKind},
    character::MovementProfile,
    document::*,
    editing,
    graph::{self, Edge, Node, OperationDef, PortType, Reference},
    input_actions::{self, every_entity, every_holder},
    movement_presets::{self, MovementPreset, PresetCamera, PresetOptions},
    physics3d::{CollisionGeometry, CollisionShape},
    script::{self, NameKind, ScriptRef, ScriptSources},
    spatial::{self, FitShape},
    surface::{self, SurfaceMaterial, SurfacePreset, VelocitySpace},
    variables,
};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

mod tree;
pub(crate) use tree::follow_files;
pub use tree::{Entry as TreeEntry, TreeView};

/// What a batch, an undo or a redo whose file operations failed halfway says of those that
/// stayed done.
pub(crate) const KEPT: &str = "Os arquivos que já tinham mudado ficaram onde estão, e o projeto \
    acompanha onde eles estão";

/// What a command of the project's tree names, telling apart items that share a name: an
/// asset's kind, or a script (a `.luau` file, by its path).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Texture,
    Audio,
    Model,
    Script,
}
impl ItemKind {
    /// The kind of asset it is; none for a script.
    pub fn asset(self) -> Option<AssetKind> {
        match self {
            Self::Texture => Some(AssetKind::Texture),
            Self::Audio => Some(AssetKind::Audio),
            Self::Model => Some(AssetKind::Model),
            Self::Script => None,
        }
    }
    /// The kind of file it is in the project's tree.
    pub fn file(self) -> crate::project_tree::FileKind {
        use crate::project_tree::FileKind;
        match self {
            Self::Texture => FileKind::Image,
            Self::Audio => FileKind::Sound,
            Self::Model => FileKind::Model,
            Self::Script => FileKind::Script,
        }
    }
}

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
        /// Platform: seconds for the way there and back, the stops included (default 4).
        #[serde(default)]
        period: Option<f32>,
        /// Platform: seconds it stands still at each end (default 0).
        #[serde(default)]
        pause_at_ends: Option<f32>,
        /// Platform: degrees it turns about X, Y and Z per period while it moves (default
        /// none).
        #[serde(default)]
        spin: Option<[f32; 3]>,
        /// Characters: the camera that follows the player (default create).
        #[serde(default)]
        camera: Option<PresetCamera>,
        /// The collision filter of the character's body or of the platform's collider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<FilterSpec>,
    },
}

/// The group part of a collision filter, as a preset takes it: category (the groups it is in)
/// and mask (those it meets), each bits or collision group names, as update_entity takes them
/// in a filter; what is left out keeps the default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Json>,
}

/// A scene's background in [`Command::SetSceneBackground`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSpec {
    /// Céu da OXY, the engine's sky (3D scenes only).
    Sky,
    /// RGBA from 0 to 1, as the image shows it; opaque, so alpha is 1.
    Color([f32; 4]),
    /// What a new scene of its kind gets: the sky in 3D, the dark color in 2D.
    Default,
}
impl BackgroundSpec {
    /// How reads show `background`, the background a scene shows ([`Scene::shown_background`]).
    pub fn shown(background: Background) -> Self {
        match background {
            Background::Sky => Self::Sky,
            Background::Color(color) => Self::Color(color),
        }
    }
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

/// A logic node parameter or an attribute value as a command gives it: tagged with its type,
/// such as `{"Number": 3}`, or plain. A plain value counts only for a node parameter whose
/// type the catalog knows and only in the obvious reading (a number for Number, a text for
/// Text, true or false for Bool); a parameter that takes any type (its input port is Any)
/// reads a plain number, text or true/false by that form. Anything else is refused with the
/// tagged form expected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Tagged(Value),
    /// Read when the command runs, against the parameter's type.
    Plain(Json),
}
impl ParamValue {
    /// The value, a plain one read as `expected` (the type the catalog gives or, for a
    /// parameter of any type, [`Self::form`]; None for attributes, which have none). `what`
    /// names it in the error, such as "O parâmetro volume do nó action.sound".
    fn value(&self, expected: Option<&Value>, what: &str) -> Result<Value, String> {
        let plain = match self {
            Self::Tagged(value) => return Ok(value.clone()),
            Self::Plain(plain) => plain,
        };
        match (expected, plain) {
            (Some(Value::Number(_)), Json::Number(n)) if n.as_f64().is_some() => {
                return Ok(Value::Number(n.as_f64().unwrap_or_default()));
            }
            (Some(Value::Text(_)), Json::String(text)) => return Ok(Value::Text(text.clone())),
            (Some(Value::Bool(_)), Json::Bool(b)) => return Ok(Value::Bool(*b)),
            _ => {}
        }
        let (example, case) = tagged_example(expected, plain);
        Err(format!(
            "{what} precisa de um valor marcado com o tipo, como {example}; recebeu {plain}{}",
            if case {
                " (o nome do tipo começa com maiúscula)"
            } else {
                ""
            }
        ))
    }
    /// The type a plain number, text or true/false has by its form, for a parameter that takes
    /// any type; None for a tagged value and other forms, which stay refused.
    fn form(&self) -> Option<Value> {
        match self {
            Self::Plain(Json::Number(_)) => Some(Value::Number(0.)),
            Self::Plain(Json::String(_)) => Some(Value::Text(String::new())),
            Self::Plain(Json::Bool(_)) => Some(Value::Bool(false)),
            _ => None,
        }
    }
}

/// The tags of [`Value`], as values are written.
const VALUE_TAGS: [&str; 7] = [
    "Number", "Text", "Bool", "Object", "Vector2", "Vector3", "Surface",
];

/// How `plain` reads tagged as `expected` or, without a type, as its JSON form suggests, for
/// messages; true when `plain` is a tag written in another case, such as {"number": 3}.
fn tagged_example(expected: Option<&Value>, plain: &Json) -> (String, bool) {
    let tagged = |tag: &str, inner: Json| {
        Json::Object(Map::from_iter([(tag.to_owned(), inner)])).to_string()
    };
    // A tag, in any case, around a value: the example shows the tag as written and the value
    // in its form.
    let mut plain = plain;
    let mut known = None;
    let mut case = false;
    if let Json::Object(map) = plain
        && map.len() == 1
        && let Some((key, inner)) = map.iter().next()
        && let Some(tag) = VALUE_TAGS.iter().find(|tag| tag.eq_ignore_ascii_case(key))
    {
        known = Some(*tag);
        case = *tag != key.as_str();
        plain = inner;
    }
    // The catalog's type first, then the tag given, then what the JSON looks like.
    let typed = expected.map(|value| match value {
        Value::Number(_) => "Number",
        Value::Text(_) => "Text",
        Value::Bool(_) => "Bool",
        Value::Object(_) => "Object",
        Value::Vector2(_) => "Vector2",
        Value::Vector3(_) => "Vector3",
        Value::Surface(_) => "Surface",
    });
    let tag = typed.or(known).unwrap_or(match plain {
        Json::Number(_) => "Number",
        Json::Bool(_) => "Bool",
        Json::Array(items) if items.len() == 2 => "Vector2",
        Json::Array(items) if items.len() == 3 => "Vector3",
        _ => "Text",
    });
    let inner = match (tag, plain) {
        ("Number", Json::Number(_))
        | ("Bool", Json::Bool(_))
        | ("Text" | "Object" | "Surface", Json::String(_)) => plain.clone(),
        ("Vector2", Json::Array(items)) if items.len() == 2 => plain.clone(),
        ("Vector3", Json::Array(items)) if items.len() == 3 => plain.clone(),
        ("Number", _) => json!(1),
        ("Bool", _) => json!(true),
        ("Object", _) => json!("<ID do objeto>"),
        ("Surface", _) => json!("<ID da superfície>"),
        ("Vector2", _) => json!([0, 0]),
        ("Vector3", _) => json!([0, 0, 0]),
        _ => json!("texto"),
    };
    (tagged(tag, inner), case)
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
    /// How the exported game opens: its title (empty for the project's name), the size of its
    /// window and whether it opens in full screen. Fields left out stay as they are.
    SetGameSettings {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        window: Option<[u32; 2]>,
        #[serde(default)]
        fullscreen: Option<bool>,
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
    /// What the scene shows behind its objects, in the editor, the game and captures.
    SetSceneBackground {
        #[serde(default)]
        scene: Option<Id>,
        background: BackgroundSpec,
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
    /// or at its root), keeping their place in the world and the world path of the animations
    /// that move them. To another scene, their references must stay inside the copy.
    CopyEntities {
        #[serde(default)]
        scene: Option<Id>,
        ids: Vec<Id>,
        to_scene: Id,
        #[serde(default)]
        parent: Option<Id>,
    },
    /// Saves an object and everything under it as a model asset that any scene can place, or,
    /// with `replace` (a model's ID or name), as the new content of that model, which keeps its
    /// ID. Placements already made are copies and stay as they are. In the tree layout a new
    /// model's file is `<folder>/<name>.modelo.json` (the root by default; a folder missing is
    /// made), written by the next save.
    SaveModel {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        /// Default: the object's name, or the replaced model's own.
        #[serde(default)]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replace: Option<Id>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "folder_or_root"
        )]
        folder: Option<String>,
    },
    /// Places a copy of a model (its ID, name or file's path). Position is relative to the
    /// parent. The root keeps the name it was saved with unless `name` is given.
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
    /// Renames an asset of any kind (its ID, name or path; `kind` tells apart items of
    /// different kinds that share a name). Its ID stays. In the tree layout the name is the
    /// file's: the file is renamed in its folder, and a script (`kind` Script, by its path) too,
    /// with the objects that attach it following; in the old layout only the name changes.
    RenameAsset {
        asset: Id,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<ItemKind>,
    },
    /// Removes an asset from the project, refused while anything uses it (see
    /// [`editing::asset_uses`]). In the tree layout its file goes to the project's trash (undo
    /// brings it back), and a script (`kind` Script, by its path) too, refused while objects
    /// attach it unless `detach` takes it off them; in the old layout the file stays.
    DeleteAsset {
        asset: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<ItemKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detach: Option<bool>,
    },
    /// Makes the folder `path` of the project's tree (`Pista/Sons`), with the folders on its way.
    CreateFolder {
        path: String,
    },
    /// Renames the folder `folder`; what is inside goes with it, and every reference follows.
    RenameFolder {
        folder: String,
        name: String,
    },
    /// Moves files and folders (`items`: paths, or assets by ID or name, `kind` telling apart
    /// those that share a name) into the folder `to` ("" is the project's root); every reference
    /// follows. A name taken there is refused, never written over.
    Move {
        items: Vec<String>,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<ItemKind>,
    },
    /// Deletes the folder `folder`: what it holds (subfolders whole, files of any kind) goes up
    /// to the project's root, a name taken there numbered ("Pulo 2"), the files Windows makes
    /// in folders (Thumbs.db, desktop.ini) go to the project's trash, and the empty folder goes.
    /// No asset is deleted.
    DeleteFolder {
        folder: String,
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
    /// Sets an attribute, or removes it when `value` is absent. Attributes have no type in
    /// the catalog, so the value is always tagged.
    SetAttribute {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        name: String,
        #[serde(default)]
        value: Option<ParamValue>,
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
    /// Fits the object's collider to its shape or, with `children`, to its shape and the pieces
    /// under it, plus `margin` metres on every side; a 3D collider becomes `shape` (by default,
    /// it keeps its own). See [`spatial::fit_collider`].
    FitCollider {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        #[serde(default)]
        children: bool,
        #[serde(default)]
        margin: Option<f32>,
        #[serde(default)]
        shape: Option<FitShape>,
    },
    /// Attaches the script `path` (a `.luau` file of the project's tree, by its path from the
    /// project's folder, as `Pista/porta.luau`, in the case the disk has: `pista/porta.luau`
    /// attaches it so, with a warning) to the object, at `index` of its list of scripts
    /// (default the end), and gives the object, with their defaults, the attributes the script
    /// declares in `Module.attributes` that it lacks (decision Q6 of 0.7). An attribute it has
    /// keeps its value, whatever its type; one of another type than the declared, or declared
    /// with two types by its scripts, is warned about. Attaching a script the object has moves
    /// it to `index`, when given, and only creates the declared attributes still missing. Needs
    /// the project's scripts (see [`apply_with_scripts`]).
    AttachScript {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    /// Takes the script `path` off the object; the attributes it declared stay.
    DetachScript {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        path: String,
    },
    /// Switches the script `path` of the object on or off: off, the object keeps it in its list
    /// but the game runs the object without it.
    SetScriptEnabled {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        path: String,
        enabled: bool,
    },
    /// Takes the object's whole logic graph off (its nodes and their links), as converting an
    /// object from nodes to scripts needs: a command apart from the legacy node commands, whose
    /// contract is frozen since 0.7. The object's scripts, attributes and components stay.
    ClearLogic {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
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
    /// An input action parameter takes the action's ID or its name, and keeps the ID. Without a
    /// `position`, the node is laid out with the others added so in the batch when it ends.
    /// `links` wires its inputs as it is added: each `[from_node, from_port, to_port]` links an
    /// output of a node the object has into the new node's input `to_port`.
    AddNode {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        operation: String,
        #[serde(default)]
        position: Option<[f32; 2]>,
        #[serde(default)]
        params: BTreeMap<String, ParamValue>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        links: Vec<[String; 3]>,
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
        params: BTreeMap<String, ParamValue>,
    },
    /// One link (`from_node`, `from_port`, `to_node`, `to_port`) or many in `links`, each
    /// `[from_node, from_port, to_node, to_port]`, all or none.
    Connect {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_node: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_port: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_node: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_port: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        links: Vec<[String; 4]>,
    },
    /// One link or many in `links`, as [`Command::Connect`] takes them, all or none.
    Disconnect {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_node: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_port: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_node: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_port: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        links: Vec<[String; 4]>,
    },
    /// Lays out the object's logic ([`graph::arrange`]): the `nodes` given, or all of them; the
    /// others stay where they are. With `split_shared_data`, those of them that only give data
    /// and feed several lanes get a copy per lane first ([`graph::layout::split_shared_data`]),
    /// which the command answers as created.
    ArrangeLogic {
        #[serde(default)]
        scene: Option<Id>,
        entity: Id,
        #[serde(default)]
        nodes: Option<Vec<Id>>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        split_shared_data: bool,
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
    /// New game variable (Variáveis do jogo). Its value gives its type: a plain number, text or
    /// true/false by its form, a vector tagged. Its ID is what logic nodes use.
    CreateVariable {
        name: String,
        value: ParamValue,
        #[serde(default)]
        persistent: bool,
    },
    /// `variable` is the variable's ID or its name. A plain value is read as the variable's
    /// type; giving it another type takes a tagged value.
    UpdateVariable {
        variable: Id,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        value: Option<ParamValue>,
        #[serde(default)]
        persistent: Option<bool>,
    },
    /// Refused while a logic node uses the variable.
    DeleteVariable {
        variable: Id,
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
            Self::SetGameSettings { .. } => "Configurar jogo exportado",
            Self::CreateScene { .. } => "Criar cena",
            Self::RenameScene { .. } => "Renomear cena",
            Self::DuplicateScene { .. } => "Duplicar cena",
            Self::DeleteScene { .. } => "Excluir cena",
            Self::SetSceneBackground { .. } => "Mudar fundo da cena",
            Self::CreateEntity { .. } => "Criar objeto",
            Self::DeleteEntities { .. } => "Excluir objetos",
            Self::DuplicateEntities { .. } => "Duplicar objetos",
            Self::CopyEntities { .. } => "Copiar objetos para outra cena",
            Self::SaveModel { replace: None, .. } => "Salvar modelo",
            Self::SaveModel { .. } => "Substituir modelo",
            Self::PlaceModel { .. } => "Colocar modelo",
            Self::RenameAsset { .. } => "Renomear recurso",
            Self::DeleteAsset { .. } => "Excluir recurso",
            Self::CreateFolder { .. } => "Criar pasta",
            Self::RenameFolder { .. } => "Renomear pasta",
            Self::Move { .. } => "Mover para pasta",
            Self::DeleteFolder { .. } => "Excluir pasta",
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
            Self::FitCollider { .. } => "Ajustar colisor",
            Self::AttachScript { .. } => "Anexar script",
            Self::DetachScript { .. } => "Remover script",
            Self::SetScriptEnabled { enabled: true, .. } => "Ligar script",
            Self::SetScriptEnabled { .. } => "Desligar script",
            Self::ClearLogic { .. } => "Tirar a lógica de nós",
            Self::ApplyMovementProfile { .. } => "Aplicar perfil de movimento",
            Self::UpdateEntity { .. } => "Editar objeto",
            Self::AddNode { .. } => "Adicionar nó",
            Self::RemoveNode { .. } => "Remover nó",
            Self::SetNodeParams { .. } => "Mudar parâmetros do nó",
            Self::Connect { .. } => "Ligar portas",
            Self::Disconnect { .. } => "Desligar portas",
            Self::ArrangeLogic { .. } => "Organizar lógica",
            Self::CreateInputAction { .. } => "Criar ação de entrada",
            Self::RenameInputAction { .. } => "Renomear ação de entrada",
            Self::SetInputKey { .. } => "Mudar tecla da ação",
            Self::DeleteInputAction { .. } => "Excluir ação de entrada",
            Self::CreateSurface { .. } => "Criar superfície física",
            Self::UpdateSurface { .. } => "Editar superfície física",
            Self::DeleteSurface { .. } => "Excluir superfície física",
            Self::CreateVariable { .. } => "Criar variável do jogo",
            Self::UpdateVariable { .. } => "Editar variável do jogo",
            Self::DeleteVariable { .. } => "Excluir variável do jogo",
        }
    }
}

/// What one command did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Applied {
    pub op: String,
    /// IDs the command created, in order (objects, scenes or nodes).
    pub created: Vec<Id>,
    /// The name of each created ID right after the command (a node's is its operation), for
    /// the answer to name what a later command of the batch removed.
    #[serde(skip)]
    pub names: Vec<Option<String>>,
    /// What the command left that the game takes otherwise than it may seem, as the batch
    /// ended (see [`Check`]).
    #[serde(skip)]
    pub warnings: Vec<String>,
    /// For a command that copies objects (duplicate_entities, copy_entities, save_model,
    /// place_model and duplicate_scene), one entry per created ID: the IDs of the copy that ID
    /// belongs to, each original object, node and animation (and scene) to its copy (in a
    /// model, for save_model). The copies of one round share it. `"$N.M@<id>"` reads it
    /// ([`Placeholder::Copied`]).
    #[serde(skip)]
    pub copies: Vec<Arc<CopyMap>>,
    /// The attributes attach_script gave the object, by name: those its script declares that
    /// the object lacked.
    #[serde(skip)]
    pub attributes: Vec<String>,
}

/// Which command of a batch, as errors and warnings name it: its index, counted from 0 as
/// `"$N"` and `created` count, its op and its place in words, "Comando 1 (set_node_params, o
/// 2º do lote)".
fn which(index: usize, op: &str) -> String {
    if op.is_empty() {
        format!("Comando {index} (o {}º do lote)", index + 1)
    } else {
        format!("Comando {index} ({op}, o {}º do lote)", index + 1)
    }
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
        // Messages written as sentences end in a period already.
        write!(
            f,
            "{}: {}. Nada foi alterado.",
            which(self.index, &self.op),
            self.message.trim_end().trim_end_matches('.')
        )
    }
}
impl std::error::Error for CommandError {}

/// Applies `commands` in order as one atomic batch. Commands without a scene use
/// `default_scene`. On success the project is replaced; on failure it is unchanged. A project
/// whose scripts a command needs takes [`apply_with_scripts`].
pub fn apply(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
) -> Result<Vec<Applied>, CommandError> {
    apply_with_scripts(project, default_scene, commands, &ScriptSources::default())
}

/// [`apply`] with the project's scripts (see `persistence::load_scripts`), which attach_script
/// attaches, reading what they declare. The commands of the project's tree that move files need
/// the project's folder: see [`plan_with_files`].
pub fn apply_with_scripts(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
    scripts: &ScriptSources,
) -> Result<Vec<Applied>, CommandError> {
    let (draft, applied) = plan(project, default_scene, commands, scripts, None)?;
    *project = draft;
    Ok(applied)
}

/// Plans `commands` as [`apply`] would apply them, on `view`, the project's tree as read from
/// its folder: renaming, moving and deleting files and folders change the view, and the file
/// operations they need are left in it ([`TreeView::files`]), with attach_script reading the
/// scripts where the batch put them. Answers the project as the batch leaves it, which takes
/// the place of `project` only once the operations are done on the disk (see
/// [`crate::journal::run`]); nothing is touched here.
pub fn plan_with_files(
    project: &Project,
    default_scene: &str,
    commands: &[Command],
    view: &mut TreeView,
) -> Result<(Project, Vec<Applied>), CommandError> {
    plan(
        project,
        default_scene,
        commands,
        &ScriptSources::default(),
        Some(view),
    )
}

/// Whether `commands` need the folder of `project` read ([`TreeView`]): those of the tree
/// (renaming and deleting an asset only in the tree layout, where its file changes), and
/// attach_script, which reads the scripts.
pub fn needs_files(project: &Project, commands: &[Command]) -> bool {
    needs_scripts(commands)
        || commands
            .iter()
            .any(|command| tree::needs_folder(command) || tree::handles(command, project))
}

fn plan(
    project: &Project,
    default_scene: &str,
    commands: &[Command],
    scripts: &ScriptSources,
    mut view: Option<&mut TreeView>,
) -> Result<(Project, Vec<Applied>), CommandError> {
    let mut draft = project.clone();
    let mut applied: Vec<Applied> = Vec::with_capacity(commands.len());
    let mut checks: Vec<(usize, Check)> = Vec::new();
    // Nodes added without a position: (scene, object, node).
    let mut unplaced: Vec<(Id, Id, Id)> = Vec::new();
    // The last command that renamed, moved or deleted each item scripts may name.
    let mut took: BTreeMap<(NameKind, Id), usize> = BTreeMap::new();
    for (index, command) in commands.iter().enumerate() {
        let fail = |message: String| CommandError {
            index,
            op: command.op(),
            message,
        };
        let command = resolve_placeholders(command, &applied, &draft).map_err(fail)?;
        let command = resolve_names(&command, &draft, default_scene).map_err(fail)?;
        // Copies take the logic as it is laid out.
        if matches!(
            command,
            Command::DuplicateEntities { .. }
                | Command::CopyEntities { .. }
                | Command::SaveModel { .. }
                | Command::DuplicateScene { .. }
        ) {
            lay_out(&mut draft, &unplaced);
        }
        // A model saved in a folder: the folder as the disk has it, made when missing.
        let command = match (&command, view.as_deref_mut()) {
            (
                Command::SaveModel {
                    folder: Some(_), ..
                },
                Some(view),
            ) => tree::save_model_in(&draft, &command, default_scene, view).map_err(fail)?,
            (
                Command::SaveModel {
                    folder: Some(_), ..
                },
                None,
            ) if draft.layout == Layout::Tree => {
                return Err(fail(tree::NO_FOLDER.into()));
            }
            _ => command,
        };
        let mut raised = Vec::new();
        let mut copies = Vec::new();
        let mut told = Vec::new();
        let had = attach_target(&draft, &command, default_scene);
        let kinds = renaming(&command);
        let named = (!kinds.is_empty()).then(|| script::names_by_id(&draft, kinds));
        let created = match view.as_deref_mut() {
            Some(view) if tree::handles(&command, &draft) => {
                tree::apply(&mut draft, &command, view).map(|lines| {
                    told = lines;
                    Vec::new()
                })
            }
            view => apply_one(
                &mut draft,
                default_scene,
                &command,
                &mut raised,
                &mut copies,
                view.map_or(scripts, |view| &view.scripts),
            ),
        }
        .map_err(|message| fail(placed(&draft, &command, default_scene, message)))?;
        match &command {
            Command::AddNode {
                scene,
                entity,
                position: None,
                ..
            } => {
                let scene = scene_id(&draft, scene, default_scene).map_err(fail)?;
                unplaced.push((scene, entity.clone(), created[0].clone()));
            }
            // Laid out already: the batch's end leaves them where they are.
            Command::ArrangeLogic {
                scene,
                entity,
                nodes,
                ..
            } => {
                let scene = scene_id(&draft, scene, default_scene).map_err(fail)?;
                unplaced.retain(|(s, e, node)| {
                    *s != scene
                        || e != entity
                        || nodes.as_ref().is_some_and(|nodes| !nodes.contains(node))
                });
            }
            _ => {}
        }
        validate_project(&draft).map_err(|e| fail(e.to_string()))?;
        if let Some(named) = named {
            let now = script::names_by_id(&draft, kinds);
            for (item, name) in named {
                if now.get(&item) != Some(&name) {
                    took.insert(item, index);
                }
            }
        }
        for check in raised {
            // The last layout of an object's logic is the one whose wires stay.
            if let Check::LongWires { scene, entity, .. } = &check {
                checks.retain(|(_, earlier)| {
                    !matches!(earlier, Check::LongWires { scene: s, entity: e, .. }
                        if s == scene && e == entity)
                });
            }
            // The last attach to an object read what all its scripts declare.
            if let Check::DeclaredAttributes { scene, entity, .. } = &check {
                checks.retain(|(_, earlier)| {
                    !matches!(earlier, Check::DeclaredAttributes { scene: s, entity: e, .. }
                        if s == scene && e == entity)
                });
            }
            // A script attached again to an object tells its callbacks once.
            if let Check::UnheardCallbacks {
                scene,
                entity,
                path,
                ..
            } = &check
            {
                checks.retain(|(_, earlier)| {
                    !matches!(earlier, Check::UnheardCallbacks { scene: s, entity: e, path: p, .. }
                        if s == scene && e == entity && p == path)
                });
            }
            checks.push((index, check));
        }
        let names = if created.is_empty() {
            Vec::new()
        } else {
            let wanted: HashSet<&str> = created.iter().map(String::as_str).collect();
            let found = names_of(&draft, &wanted);
            created
                .iter()
                .map(|id| found.get(id.as_str()).cloned())
                .collect()
        };
        let op = command.op();
        let warnings = told
            .into_iter()
            .map(|line| format!("{}: {line}", which(index, &op)))
            .collect();
        // The attributes attach_script gave the object.
        let attributes = had
            .and_then(|(scene, entity, keys)| {
                let e = draft.scene(&scene)?.entity(&entity)?;
                Some(
                    e.attributes
                        .keys()
                        .filter(|name| !keys.contains(*name))
                        .cloned()
                        .collect(),
                )
            })
            .unwrap_or_default();
        applied.push(Applied {
            op,
            created,
            names,
            warnings,
            copies,
            attributes,
        });
    }
    lay_out(&mut draft, &unplaced);
    // Checked on the final project: a later command may have settled them.
    for (index, check) in checks {
        if let Some(message) = check.message(&draft) {
            let command = &mut applied[index];
            command
                .warnings
                .push(format!("{}: {message}", which(index, &command.op)));
        }
    }
    let scripts = view.as_deref().map_or(scripts, |view| &view.scripts);
    lint_attached(&draft, commands, scripts, &mut applied);
    warn_broken_citations(project, &draft, commands, scripts, &took, &mut applied);
    Ok((draft, applied))
}

/// The object an attach_script command attaches to, as (scene, object), with the attributes it
/// has before the command.
fn attach_target(
    project: &Project,
    command: &Command,
    default: &str,
) -> Option<(Id, Id, BTreeSet<String>)> {
    let Command::AttachScript { scene, entity, .. } = command else {
        return None;
    };
    let scene = scene_id(project, scene, default).ok()?;
    let e = project.scene(&scene)?.entity(entity)?;
    let keys = e.attributes.keys().cloned().collect();
    Some((scene, e.id.clone(), keys))
}

/// The kinds of names of the project `command` may take away from the scripts that give them
/// as literal texts: renaming or deleting what has them (delete_folder renames a file whose
/// name the root has), or moving a model, which `scene.spawn_model` finds by its file too.
fn renaming(command: &Command) -> &'static [NameKind] {
    use NameKind as K;
    match command {
        Command::RenameAsset { .. }
        | Command::DeleteAsset { .. }
        | Command::DeleteFolder { .. } => &[K::Sound, K::Model],
        Command::Move { .. } | Command::RenameFolder { .. } => &[K::Model],
        Command::RenameScene { .. } | Command::DeleteScene { .. } => &[K::Scene],
        Command::UpdateVariable { name: Some(_), .. } | Command::DeleteVariable { .. } => {
            &[K::Variable]
        }
        Command::RenameInputAction { .. } | Command::DeleteInputAction { .. } => &[K::Action],
        Command::UpdateSurface { name: Some(_), .. } | Command::DeleteSurface { .. } => {
            &[K::Surface]
        }
        _ => &[],
    }
}

/// Whether a command of `commands` renames or deletes a sound, a model, a scene, a game
/// variable, an input action or a surface, which scripts may give by name, or moves a model:
/// hosts pass the project's scripts then, for the batch to warn of the names it takes from
/// them.
pub fn renames_script_names(commands: &[Command]) -> bool {
    commands.iter().any(|command| !renaming(command).is_empty())
}

/// Adds the names the scripts of `scripts` give that `before` had and `after`, the project as
/// the batch left it, no longer has (see [`script::broken_citations`]) to the command that took
/// each away: the last that renamed, moved or deleted what the name found (`took`), else the
/// last that could.
fn warn_broken_citations(
    before: &Project,
    after: &Project,
    commands: &[Command],
    scripts: &ScriptSources,
    took: &BTreeMap<(NameKind, Id), usize>,
    applied: &mut [Applied],
) {
    let mut kinds: Vec<NameKind> = Vec::new();
    for kind in commands.iter().flat_map(|command| renaming(command)) {
        if !kinds.contains(kind) {
            kinds.push(*kind);
        }
    }
    if kinds.is_empty() || scripts.files.is_empty() {
        return;
    }
    for broken in script::broken_citations(before, after, scripts, &kinds) {
        let script::BrokenCitation { kind, id, warning } = broken;
        let by = id.and_then(|id| took.get(&(kind, id)).copied());
        let Some(index) = by.or_else(|| {
            commands
                .iter()
                .rposition(|command| renaming(command).contains(&kind))
        }) else {
            continue;
        };
        let command = &mut applied[index];
        let label = which(index, &command.op);
        command.warnings.push(format!("{label}: {warning}"));
    }
}

/// Adds to the last attach_script of `commands` that attached each script the lint of the
/// names it gives (see [`script::lint`]), against `project` as the batch left it.
fn lint_attached(
    project: &Project,
    commands: &[Command],
    scripts: &ScriptSources,
    applied: &mut [Applied],
) {
    let mut last: Vec<(String, usize)> = Vec::new();
    for (index, command) in commands.iter().enumerate() {
        let Command::AttachScript { path, .. } = command else {
            continue;
        };
        let Some(path) = script::normalize_path(path)
            .ok()
            .and_then(|given| scripts.known(&given).map(str::to_owned))
        else {
            continue;
        };
        last.retain(|(attached, _)| *attached != path);
        last.push((path, index));
    }
    if last.is_empty() {
        return;
    }
    let warnings = script::lint(
        scripts,
        last.iter().map(|(path, _)| path.as_str()),
        Some(project),
    );
    for (path, index) in &last {
        let command = &mut applied[*index];
        let label = which(*index, &command.op);
        command.warnings.extend(
            warnings
                .iter()
                .filter(|warning| warning.path == *path)
                .map(|warning| format!("{label}: {warning}")),
        );
    }
}

/// Lays out the nodes `unplaced` ((scene, object, node)) with [`graph::arrange`], each object's
/// together, around the nodes that stay. Objects and nodes a later command removed are left
/// out.
fn lay_out(project: &mut Project, unplaced: &[(Id, Id, Id)]) {
    let mut objects: Vec<(&Id, &Id, HashSet<Id>)> = Vec::new();
    for (scene, entity, node) in unplaced {
        match objects
            .iter_mut()
            .find(|(s, e, _)| *s == scene && *e == entity)
        {
            Some((_, _, nodes)) => {
                nodes.insert(node.clone());
            }
            None => objects.push((scene, entity, HashSet::from([node.clone()]))),
        }
    }
    for (scene, entity, nodes) in objects {
        if let Some(e) = project.scene_mut(scene).and_then(|s| s.entity_mut(entity)) {
            graph::arrange(&mut e.graph, &nodes);
        }
    }
}

/// Something a command left that the batch checks once it ends, since a later command may
/// settle it.
enum Check {
    /// update_entity gave an object other dimensions while its box collider kept `size`, which
    /// wrapped the old shape.
    StaleCollider {
        scene: Id,
        entity: Id,
        size: [f32; 3],
    },
    /// update_entity changed the shape of an object whose convex or mesh collider was made from
    /// it, and the collider kept the old geometry.
    StaleGeneratedCollider { scene: Id, entity: Id },
    /// copy_entities copied into `scene` the nodes `nodes` ([object, node, parameter]) whose
    /// scene parameter names `from`, the scene the copies came from, such as action.scene
    /// without its Cena input or value.scene (see `opens_scene`).
    SceneBack {
        scene: Id,
        from: Id,
        nodes: Vec<(Id, Id, &'static str)>,
    },
    /// arrange_logic laid out `nodes` (None: all) of an object's logic, which may keep wires too
    /// long to follow ([`graph::layout::long_wires`]); `split` says whether shared data was
    /// copied already.
    LongWires {
        scene: Id,
        entity: Id,
        nodes: Option<Vec<Id>>,
        split: bool,
    },
    /// attach_script attached a script to an object: what each script it attaches declares in
    /// `Module.attributes`, by path, as they loaded, which may give one attribute two types or
    /// another type than the object's.
    DeclaredAttributes {
        scene: Id,
        entity: Id,
        declared: Vec<(String, script::Declared)>,
    },
    /// attach_script attached `path`, whose declared attributes could not be read (`error`: it
    /// does not load, or its `Module.attributes` is no table of defaults).
    UnreadScript {
        scene: Id,
        entity: Id,
        path: String,
        error: String,
    },
    /// attach_script was given `given`, the script `path` in another case, and attached `path`,
    /// in the case the disk has.
    ScriptCase { given: String, path: String },
    /// attach_script attached `path`, which defines `callbacks`, some of which may never run on
    /// the object (see [`script::unheard_callbacks`]).
    UnheardCallbacks {
        scene: Id,
        entity: Id,
        path: String,
        callbacks: Vec<&'static str>,
    },
    /// What a command tells as it is.
    Note(String),
}
impl Check {
    /// The warning, when what was checked is still so in `project`.
    fn message(&self, project: &Project) -> Option<String> {
        match self {
            Self::StaleCollider {
                scene,
                entity,
                size,
            } => {
                let level = project.scene(scene)?;
                if box_fit(level, entity) != Some((*size, false)) {
                    return None;
                }
                let e = level.entity(entity)?;
                let shown = |v: [f32; 3]| format!("[{}, {}, {}]", v[0], v[1], v[2]);
                Some(format!(
                    "{} ({entity}) agora mede {}, mas o colisor de caixa continua com {}; no \
                     jogo, a colisão não acompanha a forma. Para ajustá-lo, use fit_collider \
                     {{\"entity\": \"{entity}\"}}",
                    e.name,
                    shown(e.dimensions),
                    shown(*size)
                ))
            }
            Self::StaleGeneratedCollider { scene, entity } => {
                let e = project.scene(scene)?.entity(entity)?;
                if stale_geometry(e) != Some(true) {
                    return None;
                }
                let (shape, what) = match e.physics3d.as_ref()?.shape {
                    CollisionShape::Convex { .. } => ("convex", "o convexo"),
                    _ => ("mesh", "a malha estática"),
                };
                Some(format!(
                    "A forma de {} ({entity}) mudou, mas o colisor 3D continua com {what} da forma \
                     antiga; no jogo, a colisão não acompanha a forma. Para gerá-lo de novo, use \
                     fit_collider {{\"entity\": \"{entity}\", \"shape\": \"{shape}\"}}",
                    e.name
                ))
            }
            Self::SceneBack { scene, from, nodes } => {
                let to = project.scene(scene)?;
                let mut params: Vec<&str> = Vec::new();
                // The operations listed that take their scene through an input as well, those
                // inputs, and whether every node listed does.
                let (mut wirable, mut inputs): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
                let mut every = true;
                let listed: Vec<String> = nodes
                    .iter()
                    .filter_map(|(entity, node, param)| {
                        let e = to.entity(entity)?;
                        let n = e
                            .graph
                            .nodes
                            .iter()
                            .find(|n| n.id == *node && opens_scene(e, n, param, from))?;
                        if !params.contains(param) {
                            params.push(param);
                        }
                        if graph::definition(&n.operation)
                            .is_some_and(|def| def.inputs.iter().any(|port| port.id == *param))
                        {
                            if !wirable.contains(&n.operation.as_str()) {
                                wirable.push(&n.operation);
                            }
                            if !inputs.contains(param) {
                                inputs.push(param);
                            }
                        } else {
                            every = false;
                        }
                        // The copy's ID, which set_node_params takes: the answer gives only
                        // the copied roots.
                        Some(format!(
                            "{} ({}) → nó {} ({})",
                            e.name, e.id, n.operation, n.id
                        ))
                    })
                    .collect();
                if listed.is_empty() {
                    return None;
                }
                let source = project.scene(from).map_or(from.as_str(), |s| &s.name);
                let wire = if wirable.is_empty() {
                    String::new()
                } else {
                    // Named when only some of the nodes listed have the input.
                    let which = if every {
                        String::new()
                    } else {
                        format!(", nos nós {},", wirable.join("/"))
                    };
                    format!(
                        ", ou{which} ligue a saída scene de um scene.current à entrada {}, para \
                         que abram a cena em que estiverem",
                        inputs.join("/")
                    )
                };
                let params = params.join("/");
                Some(format!(
                    "estes nós copiados para {} ainda nomeiam {source}, a cena de onde vieram; \
                     para que nomeiem a cena nova (reiniciar a fase, por exemplo), use \
                     set_node_params com {params} {{\"Text\": \"{}\"}}{wire}: {}",
                    to.name,
                    to.id,
                    listed.join("; ")
                ))
            }
            Self::LongWires {
                scene,
                entity,
                nodes,
                split,
            } => {
                let level = project.scene(scene)?;
                let e = level.entity(entity)?;
                let wires = graph::layout::long_wires(&e.graph);
                if wires.is_empty() {
                    return None;
                }
                // The nodes as oxy_entity's summary names them.
                let labels = crate::query::node_labels(project, level, &e.graph);
                let label = |id: &str| {
                    e.graph
                        .nodes
                        .iter()
                        .position(|n| n.id == id)
                        .map_or_else(|| id.to_owned(), |i| labels[i].clone())
                };
                let mut listed: Vec<String> = wires
                    .iter()
                    .map(|(i, height)| {
                        let edge = &e.graph.edges[*i];
                        format!(
                            "{}.{} -> {}.{} ({})",
                            label(&edge.from_node),
                            edge.from_port,
                            label(&edge.to_node),
                            edge.to_port,
                            height.round()
                        )
                    })
                    .collect();
                first_ten(&mut listed);
                let limit = graph::layout::LONG_WIRE;
                let counted = match wires.len() {
                    1 => format!(
                        "1 fio longo, que sobe ou desce mais de {limit} unidades entre as pontas \
                         (mais de meia tela do editor em 100%) e cruza a lógica em volta"
                    ),
                    n => format!(
                        "{n} fios longos, que sobem ou descem mais de {limit} unidades entre as \
                         pontas (mais de meia tela do editor em 100%) e cruzam a lógica em volta, \
                         o mais alto primeiro"
                    ),
                };
                // Suggested only when it would copy where one of the wires starts: exec wires,
                // and data tied to an event, stay as they are.
                let mut sources: Vec<String> = Vec::new();
                if !*split {
                    let movable: HashSet<Id> = match nodes {
                        Some(nodes) => nodes.iter().cloned().collect(),
                        None => e.graph.nodes.iter().map(|n| n.id.clone()).collect(),
                    };
                    let shared = graph::layout::shared_data(&e.graph, &movable);
                    for (i, _) in &wires {
                        let from = &e.graph.edges[*i].from_node;
                        let named = label(from);
                        if shared.contains(from) && !sources.contains(&named) {
                            sources.push(named);
                        }
                    }
                    first_ten(&mut sources);
                }
                let hint = if sources.is_empty() {
                    String::new()
                } else {
                    format!(
                        ". Com split_shared_data: true, o arrange_logic dá a cada faixa a sua \
                         cópia dos nós de dados puros que ela usa ({}), e eles deixam de puxar \
                         fios de uma faixa para outra",
                        sources.join(", ")
                    )
                };
                Some(format!(
                    "a lógica de {} ficou com {counted}: {}{hint}",
                    e.name,
                    listed.join("; ")
                ))
            }
            Self::DeclaredAttributes {
                scene,
                entity,
                declared,
            } => {
                let e = project.scene(scene)?.entity(entity)?;
                // The declarations of the scripts the object still attaches.
                let attached: Vec<&(String, script::Declared)> = declared
                    .iter()
                    .filter(|(path, _)| e.scripts.iter().any(|s| s.path == *path))
                    .collect();
                let names: BTreeSet<&str> = attached
                    .iter()
                    .flat_map(|(_, attributes)| attributes.iter().map(|(name, _)| name.as_str()))
                    .collect();
                let same =
                    |a: &Value, b: &Value| std::mem::discriminant(a) == std::mem::discriminant(b);
                let mut found = Vec::new();
                for name in names {
                    let by: Vec<(&str, &Value)> = attached
                        .iter()
                        .filter_map(|(path, attributes)| {
                            let (_, value) = attributes.iter().find(|(n, _)| n == name)?;
                            Some((path.as_str(), value))
                        })
                        .collect();
                    let first = by[0].1;
                    let held = e.attributes.get(name);
                    if by.iter().any(|(_, value)| !same(value, first)) {
                        let listed: Vec<String> = by
                            .iter()
                            .map(|(path, value)| format!("{path}: {}", script::value_kind(value)))
                            .collect();
                        let kept = match held {
                            Some(value) => format!(
                                "o atributo ficou como estava ({})",
                                script::value_kind(value)
                            ),
                            None => "o objeto não tem o atributo".into(),
                        };
                        found.push(format!(
                            "os scripts declaram o atributo {name} com tipos diferentes ({}); \
                             {kept}",
                            listed.join("; ")
                        ));
                    } else if let Some(value) = held
                        && !same(value, first)
                    {
                        let paths: Vec<&str> = by.iter().map(|(path, _)| *path).collect();
                        let verb = if paths.len() == 1 {
                            "declara"
                        } else {
                            "declaram"
                        };
                        found.push(format!(
                            "o atributo {name} é {}, mas {} o {verb} como {}; o atributo ficou \
                             como estava",
                            script::value_kind(value),
                            paths.join(" e "),
                            script::value_kind(first)
                        ));
                    }
                }
                (!found.is_empty()).then(|| format!("{}: {}", e.name, found.join("; ")))
            }
            Self::UnreadScript {
                scene,
                entity,
                path,
                error,
            } => {
                let e = project.scene(scene)?.entity(entity)?;
                e.scripts.iter().any(|s| s.path == *path).then(|| {
                    format!(
                        "{}: os atributos que {path} declara não foram criados ({error}); \
                         corrija o script e anexe-o de novo para criá-los",
                        e.name
                    )
                })
            }
            Self::ScriptCase { given, path } => Some(format!(
                "o script {given} está no disco como {path}, e foi anexado assim"
            )),
            Self::UnheardCallbacks {
                scene,
                entity,
                path,
                callbacks,
            } => {
                let scene = project.scene(scene)?;
                let e = scene.entity(entity)?;
                if !e.scripts.iter().any(|s| s.path == *path) {
                    return None;
                }
                let told = script::unheard_callbacks(e, scene.kind, path, callbacks);
                (!told.is_empty()).then(|| told.join("; "))
            }
            Self::Note(text) => Some(text.clone()),
        }
    }
}

/// Whether the parameter `param` of `n`, a node of `e`, opens `scene`: it names the scene, and
/// no input of the same id gives one in its place.
fn opens_scene(e: &Entity, n: &Node, param: &str, scene: &str) -> bool {
    n.text(param) == scene && !e.graph.input_connected(&n.id, param)
}

/// The size of an object's box collider: its 3D box collider or, without one, its 2D box
/// collider. None for other shapes and for 3D characters, whose body is their collider.
fn box_size(e: &Entity) -> Option<[f32; 3]> {
    if e.character3d.is_some() {
        return None;
    }
    match &e.physics3d {
        Some(collider) => match collider.shape {
            CollisionShape::Box { size } => Some(size),
            _ => None,
        },
        None => e.collider.as_ref().map(|c| c.size),
    }
}

/// The size of the box collider of `entity` and whether it wraps the object's own shape
/// ([`spatial::fits_shape`]); None without a box collider.
fn box_fit(scene: &Scene, entity: &str) -> Option<([f32; 3], bool)> {
    let e = scene.entity(entity)?;
    let size = box_size(e)?;
    let scale = spatial::physical_scale(scene.world_matrix(entity).ok()?);
    Some((size, spatial::fits_shape(e, size, scale)))
}

/// The fingerprint a convex or mesh collider made from `e`'s shape now would keep
/// ([`CollisionGeometry::source_fingerprint`]); None without a shape of its own.
fn source_fingerprint(e: &Entity) -> Option<u64> {
    CollisionGeometry::from_entity(e).ok()?.source_fingerprint
}

/// Whether `e`'s convex or mesh collider, which keeps the geometry it was made from, no longer
/// matches `e`'s shape; None for any other collider.
fn stale_geometry(e: &Entity) -> Option<bool> {
    match &e.physics3d.as_ref()?.shape {
        CollisionShape::Convex { geometry } | CollisionShape::TriMesh { geometry } => {
            Some(source_fingerprint(e) != geometry.source_fingerprint)
        }
        _ => None,
    }
}

/// Whether two sizes are the same but for f32 rounding.
fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1e-4)
}

/// Applies `commands` like [`apply`] and answers as an agent reads the result: see [`report`].
pub fn apply_and_report(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
) -> Result<Json, CommandError> {
    apply_and_report_with_scripts(project, default_scene, commands, &ScriptSources::default())
}

/// [`apply_and_report`] with the project's scripts, as [`apply_with_scripts`] takes them.
pub fn apply_and_report_with_scripts(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
    scripts: &ScriptSources,
) -> Result<Json, CommandError> {
    let before = batch_warnings(project);
    let applied = apply_with_scripts(project, default_scene, commands, scripts)?;
    Ok(report(project, &applied, &before))
}

/// What [`apply_on_files`] did: the batch's answer ([`report`], with `moved`, `trashed` and
/// `rewritten` when files moved; see [`add_files`]) and the file operations, for the undo
/// history.
#[derive(Clone, Debug)]
pub struct OnFiles {
    pub report: Json,
    pub files: Vec<crate::edit_history::FileOp>,
}

/// Why [`apply_on_files`] failed.
#[derive(Clone, Debug)]
pub struct FilesError {
    pub message: String,
    /// The file operations that stayed done (undoing them failed too, see
    /// [`crate::journal::RunError`]), which the project now follows: a step of the undo history
    /// of their own. None, mostly: then nothing changed.
    pub kept: Vec<crate::edit_history::FileOp>,
}
impl std::fmt::Display for FilesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl From<FilesError> for String {
    fn from(error: FilesError) -> Self {
        error.message
    }
}

/// Applies `commands` to `project`, saved in the folder `root`, as the editor live and the
/// agents' tools on files do: the batch is planned on the project's tree read from the disk
/// (see [`plan_with_files`]), its file operations are done as the step `label` (journaled in
/// `known`, see [`crate::journal::run`], and undone when one fails), and only then the project
/// changes. Nothing changes when anything fails, but when undoing the operations failed too:
/// then the project follows the files that stayed moved ([`FilesError::kept`]). A batch that
/// needs no file ([`needs_files`]) reads none, but the scripts when it renames or deletes a
/// name they may give ([`renames_script_names`]), to warn of it.
pub fn apply_on_files(
    project: &mut Project,
    default_scene: &str,
    commands: &[Command],
    root: &std::path::Path,
    known: &mut crate::persistence::KnownFiles,
    label: &str,
) -> Result<OnFiles, FilesError> {
    let failed = |message: String| FilesError {
        message,
        kept: Vec::new(),
    };
    let before = batch_warnings(project);
    if !needs_files(project, commands) {
        // The scripts tell the names they give that the batch takes away, when they can be read.
        let scripts = if renames_script_names(commands) {
            crate::persistence::load_scripts(root).unwrap_or_default()
        } else {
            ScriptSources::default()
        };
        let applied = apply_with_scripts(project, default_scene, commands, &scripts)
            .map_err(|e| failed(e.to_string()))?;
        return Ok(OnFiles {
            report: report(project, &applied, &before),
            files: Vec::new(),
        });
    }
    let mut view = TreeView::read(root).map_err(|e| failed(e.to_string()))?;
    let (draft, applied) = plan_with_files(project, default_scene, commands, &mut view)
        .map_err(|e| failed(e.to_string()))?;
    let files = view.files().to_vec();
    let ops: Vec<crate::fs_ops::Op> = files.iter().map(|file| file.forward()).collect();
    if let Err(error) = crate::journal::run(root, known, label, &ops) {
        if error.kept == 0 {
            return Err(failed(format!("{error}. Nada foi alterado.")));
        }
        let kept = files[..error.kept].to_vec();
        follow_files(project, &kept);
        return Err(FilesError {
            message: format!("{error}. {KEPT}."),
            kept,
        });
    }
    *project = draft;
    let mut answer = report(project, &applied, &before);
    add_files(&mut answer, &files);
    Ok(OnFiles {
        report: answer,
        files,
    })
}

/// The scripts whose `require` `files` wrote again to find what moved, each where the last of
/// them left it, once; none for one a later operation sent to the project's trash.
pub fn rewritten(files: &[crate::edit_history::FileOp]) -> Vec<String> {
    use crate::{edit_history::FileOp, journal, names};
    let mut found: Vec<String> = Vec::new();
    for (at, file) in files.iter().enumerate() {
        let FileOp::Write { path, .. } = file else {
            continue;
        };
        let mut now = Some(path.clone());
        for later in &files[at + 1..] {
            now = match (now, later) {
                (Some(path), FileOp::Move { from, to }) => match journal::under(&path, from) {
                    Some(rest) => Some(journal::join(to, rest)),
                    None => Some(path),
                },
                (Some(path), FileOp::Trash { from, .. }) if names::same_name(&path, from) => None,
                (now, _) => now,
            };
        }
        if let Some(path) = now
            && !found.iter().any(|known| names::same_name(known, &path))
        {
            found.push(path);
        }
    }
    found
}

/// Adds to a [`report`] what `files` did on the disk: `created_folders` and `removed_folders`,
/// `moved` ([{from, to}], folders and files moved or renamed), `trashed` (the files sent to the
/// project's trash) and `rewritten` (see [`rewritten`]).
pub fn add_files(report: &mut Json, files: &[crate::edit_history::FileOp]) {
    use crate::edit_history::FileOp;
    let moved: Vec<Json> = files
        .iter()
        .filter_map(|file| match file {
            FileOp::Move { from, to } => Some(json!({"from": from, "to": to})),
            _ => None,
        })
        .collect();
    let trashed: Vec<&str> = files
        .iter()
        .filter_map(|file| match file {
            FileOp::Trash { from, .. } => Some(from.as_str()),
            _ => None,
        })
        .collect();
    let rewritten = rewritten(files);
    let folders = |made: bool| -> Vec<&str> {
        files
            .iter()
            .filter_map(|file| match file {
                FileOp::CreateDir { path } if made => Some(path.as_str()),
                FileOp::RemoveDir { path } if !made => Some(path.as_str()),
                _ => None,
            })
            .collect()
    };
    let (made, removed) = (folders(true), folders(false));
    if !made.is_empty() {
        report["created_folders"] = json!(made);
    }
    if !removed.is_empty() {
        report["removed_folders"] = json!(removed);
    }
    if !moved.is_empty() {
        report["moved"] = json!(moved);
    }
    if !trashed.is_empty() {
        report["trashed"] = json!(trashed);
    }
    if !rewritten.is_empty() {
        report["rewritten"] = json!(rewritten);
    }
}

/// Adds `warnings` to those of a [`report`], which gets the list when it had none.
pub fn add_warnings(report: &mut Json, warnings: Vec<String>) {
    if warnings.is_empty() {
        return;
    }
    match report.get_mut("warnings").and_then(Json::as_array_mut) {
        Some(listed) => listed.extend(warnings.into_iter().map(Json::from)),
        None => report["warnings"] = json!(warnings),
    }
}

/// Why renaming or deleting a script waits: the old layout, until its first save organizes the
/// project in folders, or a project that has no folder yet.
fn script_files_need(project: &Project) -> &'static str {
    if project.layout == Layout::Legacy {
        tree::LEGACY
    } else {
        tree::NO_FOLDER
    }
}

/// Whether `commands` need a project in folders: those of folders, a model saved in a folder,
/// renaming or deleting a script. On files an agent's batch with one organizes a project of the
/// old layout in folders first.
pub fn needs_tree_layout(commands: &[Command]) -> bool {
    let script = |asset: &str, kind: &Option<ItemKind>| {
        *kind == Some(ItemKind::Script) || asset.trim().to_ascii_lowercase().ends_with(".luau")
    };
    commands.iter().any(|command| {
        tree::needs_folder(command)
            || match command {
                Command::RenameAsset { asset, kind, .. }
                | Command::DeleteAsset { asset, kind, .. } => script(asset, kind),
                _ => false,
            }
    })
}

/// Whether a command of `commands` needs the project's scripts (see [`apply_with_scripts`]):
/// hosts read the folder only then.
pub fn needs_scripts(commands: &[Command]) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::AttachScript { .. }))
}

/// The names of the `wanted` IDs in `project`: scenes, objects of scenes and models, nodes
/// (by their operation), animations, input actions, surfaces, game variables and assets.
fn names_of<'a>(project: &Project, wanted: &HashSet<&'a str>) -> HashMap<&'a str, String> {
    let mut names = HashMap::new();
    let mut found = |id: &str, name: &str| {
        if let Some(id) = wanted.get(id) {
            names.insert(*id, name.to_owned());
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
    for variable in &project.variables {
        found(&variable.id, &variable.name);
    }
    for asset in &project.assets {
        found(&asset.id, &asset.name);
    }
    names
}

/// A batch's result for agents: `created` maps the index of each command that created
/// something to the IDs it created, in order (a preset's root first), with their names (a
/// node's is its operation; what a later command removed keeps the name it was created
/// with); `warnings`, present when the batch added any, lists what the game will ignore or
/// stop on (see [`warnings`]), the action names and keys it left shared (see
/// [`batch_warnings`]) and what its commands left that the game takes otherwise than it may
/// seem (see [`Check`]);
/// `before` are the project's [`batch_warnings`] before the batch, which are not repeated (a
/// shared name or key comes back only when the batch added an action to it).
pub fn report(project: &Project, applied: &[Applied], before: &[Warning]) -> Json {
    let wanted: HashSet<&str> = applied
        .iter()
        .flat_map(|a| a.created.iter().map(String::as_str))
        .collect();
    let names = names_of(project, &wanted);
    let created: Map<String, Json> = applied
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.created.is_empty())
        .map(|(index, a)| {
            let ids: Vec<Json> = a
                .created
                .iter()
                .enumerate()
                .map(|(nth, id)| {
                    let name = names
                        .get(id.as_str())
                        .or_else(|| a.names.get(nth).and_then(Option::as_ref));
                    json!({"id": id, "name": name})
                })
                .collect();
            (index.to_string(), Json::Array(ids))
        })
        .collect();
    let mut result = json!({"created": created});
    let attributes: Map<String, Json> = applied
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.attributes.is_empty())
        .map(|(index, a)| (index.to_string(), json!(a.attributes)))
        .collect();
    if !attributes.is_empty() {
        result["attributes_created"] = Json::Object(attributes);
    }
    let known: HashSet<&str> = before
        .iter()
        .flat_map(|w| w.about.iter().map(String::as_str))
        .collect();
    let mut new: Vec<String> = batch_warnings(project)
        .into_iter()
        .filter(|w| w.about.iter().any(|about| !known.contains(about.as_str())))
        .map(|w| w.message)
        .collect();
    new.extend(applied.iter().flat_map(|a| a.warnings.iter().cloned()));
    if !new.is_empty() {
        result["warnings"] = json!(new);
    }
    result
}

/// Something the game will ignore or stop on, though it saves, or that a batch should hear
/// about; see [`batch_warnings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// What it is about, by IDs and values only, so renaming an object does not make it new:
    /// one entry per action of a shared name or key, so a batch makes the warning new only when
    /// it adds an action to them, not when it takes one away.
    pub about: Vec<String>,
    pub message: String,
}

/// What the game will ignore or stop on, though validation lets it through: input actions
/// bound to keys the game does not read (or keeps, Escape and F3), logic nodes that name an
/// input action the project does not have (or name it instead of giving its ID, as nodes
/// stored before commands kept the ID), fixed-choice parameters holding another value (set by
/// update_entity or by hand), and switches of a component aimed at an object that lacks it and
/// game variables set to a constant of another type, which stop with an error in the game.
pub fn warnings(project: &Project) -> Vec<Warning> {
    let mut found = Vec::new();
    for (id, key) in &project.input_bindings {
        if let Err(error) = input_actions::check_key(key) {
            found.push(Warning {
                about: vec![format!("key {id} {key}")],
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
            let actions = graph::node_references(node)
                .filter(|(_, reference, _)| *reference == Reference::InputAction)
                .map(|(param, ..)| node.text(param.id));
            for action in actions {
                if project.input_bindings.contains_key(action) {
                    continue;
                }
                // The game reads only IDs: a name the project knows is still never pressed.
                let message = match input_actions::find_named(project, action) {
                    Ok(Some(id)) => format!(
                        "{}: \"{action}\" é o nome da ação de entrada {id}, e o jogo lê o ID; \
                         set_node_params com o nome guarda o ID",
                        place()
                    ),
                    Ok(None) => format!(
                        "{}: a ação de entrada \"{action}\" não existe no projeto",
                        place()
                    ),
                    // A name several actions share, which only their IDs tell apart.
                    Err(_) => {
                        let wanted = action.trim().to_lowercase();
                        let named: Vec<&str> = project
                            .input_bindings
                            .keys()
                            .filter(|id| input_actions::label(project, id).to_lowercase() == wanted)
                            .map(String::as_str)
                            .collect();
                        format!(
                            "{}: \"{action}\" é o nome de {} ações de entrada ({}), e o jogo lê \
                             o ID; use set_node_params com o ID da ação certa",
                            place(),
                            named.len(),
                            named.join(", ")
                        )
                    }
                };
                found.push(Warning {
                    about: vec![format!("action {} {action}", node.id)],
                    message,
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
                    about: vec![format!("choice {} {error}", node.id)],
                    message: format!("{}: {error}", place()),
                });
            }
            // Ranges live in the registry: an older project keeps what it holds.
            for (param, range, value) in graph::out_of_range(&e.graph, node) {
                let label = graph::definition(&node.operation).map_or("", |def| def.label);
                found.push(Warning {
                    about: vec![format!("range {} {} {value}", node.id, param.id)],
                    message: format!(
                        "{scope} → {} → {label}: {}",
                        e.name,
                        graph::range_problem(&node.operation, param, range, value)
                    ),
                });
            }
            // Only a value in the parameter is known here; one from a port is known in the game.
            if node.operation == "variable.set"
                && !e.graph.input_connected(&node.id, "value")
                && let Some(variable) = variables::get(project, node.text("variable"))
                && let Some(value) = node.params.get("value")
                && std::mem::discriminant(value) != std::mem::discriminant(&variable.value)
            {
                found.push(Warning {
                    about: vec![format!(
                        "variable value {} {} {}",
                        node.id,
                        variable.id,
                        variables::type_name(&variable.value)
                    )],
                    message: format!(
                        "{}: o value é {}, mas a variável do jogo {} guarda {}; no jogo, o nó \
                         para com erro. Dê um value desse tipo com set_node_params",
                        place(),
                        variables::kind(value),
                        variable.name,
                        variables::kind(&variable.value)
                    ),
                });
            }
        }
    }
    // Only a target in the parameter is known here; one from a port is known in the game.
    for (scope, entities) in every_holder(project) {
        for e in entities {
            for node in e
                .graph
                .nodes
                .iter()
                .filter(|n| n.operation == "action.component")
            {
                let Some(Value::Object(Some(target))) = node.params.get("target") else {
                    continue;
                };
                let Some(object) = entities.iter().find(|o| o.id == *target) else {
                    continue;
                };
                if e.graph
                    .edges
                    .iter()
                    .any(|edge| edge.to_node == node.id && edge.to_port == "target")
                {
                    continue;
                }
                let component = node.text("component");
                let lacks = match component {
                    "collider" if object.collider.is_none() && object.physics3d.is_none() => {
                        "colisor nem área"
                    }
                    "controller" if object.controller.is_none() => "controlador",
                    "platform" if object.platform.is_none() => "plataforma móvel",
                    _ => continue,
                };
                found.push(Warning {
                    about: vec![format!("component {} {component} {target}", node.id)],
                    message: format!(
                        "{scope} → {} → nó {} ({}): {} não tem {lacks}, e no jogo o nó para \
                         com erro",
                        e.name, node.operation, node.id, object.name
                    ),
                });
            }
        }
    }
    found
}

/// [`warnings`], plus the input action names that stand for several actions and the keys
/// bound to several actions. By name, commands and simulations refuse a name two actions
/// share and take first the action whose ID it is. create and rename keep names apart, but
/// the default actions of presets and components (Pular, Correr…) arrive without that check.
/// A key fires every action bound to it, which simulations, pressing actions, never show. The
/// game itself tells names apart, and projects may share a name or a key on purpose (one label
/// for keys 1, 2 and 3; jump and confirm on Space), so only the batch that leaves a name or a
/// key shared reports it, not validation.
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
            about: ids
                .iter()
                .map(|id| format!("action name {name} {id}"))
                .collect(),
            message: format!(
                "{} ações de entrada atendem pelo nome \"{name}\": {}. Pelo nome, os comandos e o \
                 oxy_simulate não as distinguem; renomeie uma delas ou use o ID",
                ids.len(),
                listed.join(", ")
            ),
        });
    }
    for (key, ids) in input_actions::shared_keys(project) {
        let listed: Vec<String> = ids
            .iter()
            .map(|id| format!("{id} ({})", input_actions::label(project, id)))
            .collect();
        found.push(Warning {
            about: ids
                .iter()
                .map(|id| format!("shared key {key} {id}"))
                .collect(),
            message: format!(
                "A tecla {key} é de {} ações de entrada: {}. No jogo, apertá-la dispara todas; \
                 o oxy_simulate aperta ações, não teclas, e não mostra isso. Se não for de \
                 propósito, mude a tecla de uma delas com set_input_key",
                ids.len(),
                listed.join(", ")
            ),
        });
    }
    found
}

/// A command's folder, none for the project's root ("" or "/"), where it goes without one.
fn folder_or_root<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let folder = Option::<String>::deserialize(deserializer)?;
    Ok(folder.filter(|folder| !folder.trim().trim_matches(['/', '\\']).trim().is_empty()))
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
            let op = item["op"].as_str().unwrap_or_default().to_owned();
            serde_json::from_value(item).map_err(|e| {
                format!(
                    "{} não reconhecido: {}",
                    which(i, &op),
                    explain(&e.to_string())
                )
            })
        })
        .collect()
}

/// A serde message about an unknown value, an unknown field or a missing one, in Portuguese,
/// with the accepted spelling when only the case differs ("Parkour" for `parkour`), and one
/// about a wrong type or length (see [`wrong_shape`]); other messages as they are.
fn explain(message: &str) -> String {
    if let Some(text) = wrong_shape(message) {
        return text;
    }
    // The names serde quotes between backticks: the one given, then the ones accepted.
    let quoted: Vec<&str> = message.split('`').skip(1).step_by(2).collect();
    let listed = |names: &[&str]| {
        names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (what, given, accepted) = match quoted.as_slice() {
        [given, accepted @ ..] if message.starts_with("unknown variant") => {
            ("valor", *given, accepted)
        }
        [given, accepted @ ..] if message.starts_with("unknown field") => {
            ("campo", *given, accepted)
        }
        [missing] if message.starts_with("missing field") => {
            return format!("falta o campo `{missing}`");
        }
        _ => return message.to_owned(),
    };
    let mut text = format!("{what} `{given}` desconhecido");
    if !accepted.is_empty() {
        text.push_str(&format!("; os aceitos são {}", listed(accepted)));
    }
    if let Some(right) = accepted.iter().find(|a| a.eq_ignore_ascii_case(given)) {
        text.push_str(&format!(
            " (maiúsculas e minúsculas contam: escreva `{right}`)"
        ));
    }
    text
}

/// A serde message about a value of the wrong type or length, in Portuguese: "invalid type:
/// sequence, expected a map" is "tipo errado: recebeu uma lista, esperava um objeto". Phrases
/// it does not know stay as serde wrote them; None for other messages.
fn wrong_shape(message: &str) -> Option<String> {
    let items = |n: &str| format!("{n} {}", if n == "1" { "item" } else { "itens" });
    let expected = |what: &str| -> String {
        let fixed = match what {
            "a map" => "um objeto",
            "a sequence" => "uma lista",
            "a string" => "um texto",
            "a boolean" => "true ou false",
            "f32" | "f64" => "um número",
            "u8" | "u16" | "u32" | "u64" | "usize" => "um número inteiro, 0 ou mais",
            "i8" | "i16" | "i32" | "i64" | "isize" => "um número inteiro",
            "fewer elements in array" => "menos itens",
            "internally tagged enum Command" => "um comando, um objeto com op",
            _ => "",
        };
        if !fixed.is_empty() {
            return fixed.into();
        }
        if let Some(n) = what
            .strip_prefix("an array of length ")
            .or_else(|| what.strip_prefix("a tuple of size "))
        {
            return format!("uma lista de {}", items(n));
        }
        if let Some(name) = what
            .strip_prefix("internally tagged enum ")
            .or_else(|| what.strip_prefix("struct "))
        {
            return format!("um objeto ({name})");
        }
        match what.strip_prefix("enum ") {
            Some(name) => format!("um dos valores de {name}"),
            None => what.into(),
        }
    };
    if let Some(rest) = message.strip_prefix("invalid length ") {
        let (length, what) = rest.split_once(", expected ")?;
        return Some(format!(
            "tamanho errado: {}, esperava {}",
            items(length),
            expected(what)
        ));
    }
    let (start, rest) = match message.strip_prefix("invalid type: ") {
        Some(rest) => ("tipo errado", rest),
        None => ("valor inválido", message.strip_prefix("invalid value: ")?),
    };
    // A text given may hold ", expected " itself; what serde expects never does.
    let (given, what) = rest.rsplit_once(", expected ")?;
    let given = [
        ("string ", "o texto "),
        ("integer ", "o número "),
        ("floating point ", "o número "),
        ("boolean ", "o valor "),
    ]
    .iter()
    .find_map(|(en, pt)| given.strip_prefix(en).map(|value| format!("{pt}{value}")))
    .unwrap_or_else(|| {
        match given {
            "sequence" => "uma lista",
            "map" => "um objeto",
            "unit value" | "null" => "null",
            other => other,
        }
        .into()
    });
    Some(format!(
        "{start}: recebeu {given}, esperava {}",
        expected(what)
    ))
}

/// A text that stands for an ID created earlier in a batch. Only a whole text of this form
/// counts, so "$10 de bônus" stays text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholder<'a> {
    /// `"$N"` (`nth` 0) or `"$N.M"`: the M-th ID created by command N, both counted from 0.
    Created { command: usize, nth: usize },
    /// `"$N@<id>"` (`nth` 0) or `"$N.M@<id>"`: the copy of the object, node or animation
    /// `original` in the copy command N made with its M-th created ID ([`Applied::copies`]).
    /// The original may itself be `"$K"` or `"$K.L"`.
    Copied {
        command: usize,
        nth: usize,
        original: &'a str,
    },
    /// `"$$N"`, `"$$N.M"` or `"$$N@<id>"`: the text itself without its first '$', such as "$10".
    Escaped(&'a str),
}
impl<'a> Placeholder<'a> {
    pub fn parse(text: &'a str) -> Option<Self> {
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let index = |rest: &str| match rest.split_once('.') {
            Some((command, nth)) => digits(command) && digits(nth),
            None => digits(rest),
        };
        let pattern = |rest: &str| match rest.split_once('@') {
            Some((head, original)) => index(head) && !original.is_empty(),
            None => index(rest),
        };
        if let Some(rest) = text.strip_prefix("$$") {
            return pattern(rest).then(|| Self::Escaped(&text[1..]));
        }
        let rest = text.strip_prefix('$').filter(|rest| pattern(rest))?;
        let (head, original) = match rest.split_once('@') {
            Some((head, original)) => (head, Some(original)),
            None => (rest, None),
        };
        let (command, nth) = head.split_once('.').unwrap_or((head, "0"));
        // A number too long for usize names a command that cannot exist.
        let number = |s: &str| s.parse().unwrap_or(usize::MAX);
        let (command, nth) = (number(command), number(nth));
        Some(match original {
            Some(original) => Self::Copied {
                command,
                nth,
                original,
            },
            None => Self::Created { command, nth },
        })
    }
    /// The ID or text it stands for after the commands `applied`; `None` when command N did
    /// not run before, created fewer IDs or did not copy the original.
    pub fn resolve(self, applied: &[Applied]) -> Option<String> {
        match self {
            Self::Created { command, nth } => applied.get(command)?.created.get(nth).cloned(),
            Self::Copied {
                command,
                nth,
                original,
            } => {
                let original = match Placeholder::parse(original) {
                    Some(inner) => inner.resolve(applied)?,
                    None => original.to_owned(),
                };
                applied
                    .get(command)?
                    .copies
                    .get(nth)?
                    .get(&original)
                    .cloned()
            }
            Self::Escaped(text) => Some(text.into()),
        }
    }
    /// Why `text`, this placeholder, stands for nothing after `applied`; `project` names the
    /// original of a copy.
    fn missing(self, text: &str, applied: &[Applied], project: &Project) -> String {
        let generic = format!(
            "\"{text}\" não se refere a um ID criado antes no lote (para o texto \"{text}\" em \
             si, escreva \"${text}\")"
        );
        let Self::Copied {
            command,
            nth,
            original,
        } = self
        else {
            return generic;
        };
        let Some(copy) = applied.get(command) else {
            return generic;
        };
        let named = lowercase_first(&which(command, &copy.op));
        if copy.copies.is_empty() {
            return format!(
                "\"{text}\": o {named} não copia objetos; \"$N@<ID>\" vale para os comandos \
                 duplicate_entities, copy_entities, save_model, place_model e duplicate_scene"
            );
        }
        if nth >= copy.copies.len() {
            return match copy.created.len() {
                1 => format!("\"{text}\": o {named} criou só 1 ID, \"${command}.0\""),
                count => format!(
                    "\"{text}\": o {named} criou {count} IDs, de \"${command}.0\" a \
                     \"${command}.{}\"",
                    count - 1
                ),
            };
        }
        let original = match Placeholder::parse(original) {
            Some(inner) => match inner.resolve(applied) {
                Some(id) => id,
                None => return inner.missing(original, applied, project),
            },
            None => original.to_owned(),
        };
        format!(
            "\"{text}\": {} não está entre o que o {named} copiou; depois do @ vai o ID de um \
             objeto, nó ou animação do original",
            described(project, &original)
        )
    }
}

/// "Comando 2 (…)" as "comando 2 (…)", inside a sentence.
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}
/// `listed` cut to its first ten, the rest counted: "… e mais 3".
fn first_ten(listed: &mut Vec<String>) {
    let more = listed.len().saturating_sub(10);
    listed.truncate(10);
    if more > 0 {
        listed.push(format!("… e mais {more}"));
    }
}
/// "modelos" as "Modelos", to start a sentence.
fn uppercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// `id` as an error names it: the object, node or animation of the project that has it, with
/// the ID; the ID alone when nothing has it.
fn described(project: &Project, id: &str) -> String {
    for (_, e) in every_entity(project) {
        if e.id == id {
            return format!("o objeto {} ({id})", e.name);
        }
        if let Some(node) = e.graph.node(id) {
            return format!("o nó {} de {} ({id})", node.operation, e.name);
        }
        if let Some(clip) = e.clips.iter().find(|clip| clip.id == id) {
            return format!("a animação {} de {} ({id})", clip.name, e.name);
        }
    }
    match project.scene(id) {
        Some(scene) => format!("a cena {} ({id})", scene.name),
        None => format!("{id} (nenhum objeto, nó ou animação do projeto tem esse ID)"),
    }
}

/// Replaces placeholders anywhere in `command`, patches included; `project` is the batch's
/// draft, for errors.
fn resolve_placeholders(
    command: &Command,
    applied: &[Applied],
    project: &Project,
) -> Result<Command, String> {
    fn walk(
        value: &mut Json,
        applied: &[Applied],
        project: &Project,
        changed: &mut bool,
    ) -> Result<(), String> {
        match value {
            Json::String(text) => {
                if let Some(placeholder) = Placeholder::parse(text) {
                    *text = placeholder
                        .resolve(applied)
                        .ok_or_else(|| placeholder.missing(text, applied, project))?;
                    *changed = true;
                }
            }
            Json::Array(items) => {
                for item in items {
                    walk(item, applied, project, changed)?;
                }
            }
            Json::Object(map) => {
                for item in map.values_mut() {
                    walk(item, applied, project, changed)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut value = serde_json::to_value(command).map_err(|e| e.to_string())?;
    // A command that takes a scene and has none takes the scene of the object a placeholder
    // gives as its entity (else the first of its ids, else its parent, but for copy_entities,
    // whose parent is in the scene it copies to), created or copied earlier in the batch,
    // wherever it is.
    let keys: &[&str] = if matches!(command, Command::CopyEntities { .. }) {
        &["ids"]
    } else {
        &["entity", "ids", "parent"]
    };
    let carried = value
        .get("scene")
        .is_some_and(Json::is_null)
        .then(|| {
            keys.iter().find_map(|key| {
                let text = match &value[*key] {
                    Json::String(text) => text.as_str(),
                    Json::Array(items) => items.first()?.as_str()?,
                    _ => return None,
                };
                match Placeholder::parse(text)? {
                    Placeholder::Escaped(_) => None,
                    placeholder => placeholder.resolve(applied),
                }
            })
        })
        .flatten();
    let mut changed = false;
    walk(&mut value, applied, project, &mut changed)?;
    if let Some(scene) = carried.and_then(|id| {
        project
            .scenes
            .iter()
            .find(|scene| scene.entity(&id).is_some())
    }) {
        value["scene"] = json!(scene.id);
        changed = true;
    }
    if !changed {
        return Ok(command.clone());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Whether `command` takes objects by name: all but the legacy node commands, whose contract is
/// frozen since 0.7 and takes IDs.
fn takes_names(command: &Command) -> bool {
    !matches!(
        command,
        Command::AddNode { .. }
            | Command::RemoveNode { .. }
            | Command::SetNodeParams { .. }
            | Command::Connect { .. }
            | Command::Disconnect { .. }
            | Command::ArrangeLogic { .. }
    )
}

/// Replaces the objects `command` names by name in its `entity`, `ids` and `parent` with their
/// IDs, as agents give them in the other tools: a text that no object of the project has as its
/// ID and that is the name (any case, without the spaces around it) of exactly one object of
/// the scene the command looks in (`to_scene` for copy_entities' parent). A name several objects
/// there share is refused with their IDs and where each is; one no object there has stays, for
/// the command's own refusal, which says where an object of that name is.
fn resolve_names(command: &Command, project: &Project, default: &str) -> Result<Command, String> {
    if !takes_names(command) {
        return Ok(command.clone());
    }
    let mut value = serde_json::to_value(command).map_err(|e| e.to_string())?;
    let own = value["scene"].as_str().unwrap_or(default).to_owned();
    let mut changed = false;
    for key in ["entity", "ids", "parent"] {
        let looked_in = match command {
            Command::CopyEntities { to_scene, .. } if key == "parent" => to_scene.as_str(),
            _ => own.as_str(),
        };
        // A scene that does not exist is the command's own refusal.
        let Ok(scene) = project.find_scene(looked_in, "scene") else {
            continue;
        };
        let texts: Vec<&mut Json> = match value.get_mut(key) {
            Some(Json::Array(items)) => items.iter_mut().collect(),
            Some(text @ Json::String(_)) => vec![text],
            _ => continue,
        };
        for text in texts {
            let Json::String(given) = text else { continue };
            let is_id = project.scenes.iter().any(|s| s.entity(given).is_some());
            if is_id || scene.entities_named(given).is_empty() {
                continue;
            }
            *given = scene.find_entity(given, key)?.id.clone();
            changed = true;
        }
    }
    if !changed {
        return Ok(command.clone());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// The scene of a command: `scene` by its ID or name, `default` when absent.
fn scene_id(project: &Project, scene: &Option<Id>, default: &str) -> Result<Id, String> {
    scene_named(project, scene.as_deref().unwrap_or(default), "scene")
}
/// The ID of the scene `text` names by ID or name; `what` is the field that gave it.
fn scene_named(project: &Project, text: &str, what: &str) -> Result<Id, String> {
    project.find_scene(text, what).map(|s| s.id.clone())
}
fn scene_mut<'a>(project: &'a mut Project, id: &str) -> &'a mut Scene {
    project.scene_mut(id).expect("scene checked by scene_id")
}
fn entity_mut<'a>(scene: &'a mut Scene, id: &str) -> Result<&'a mut Entity, String> {
    scene.entity_mut(id).ok_or_else(|| absent(id))
}
/// The error of a command whose object `id` its scene lacks; [`apply`] tells which scene that
/// was and where the object is ([`absent_object`] reads it back).
fn absent(id: &str) -> String {
    format!("Objeto {id} não existe nesta cena")
}
fn absent_object(message: &str) -> Option<&str> {
    message
        .strip_prefix("Objeto ")?
        .strip_suffix(" não existe nesta cena")
}
/// `message`, the error of `command` in `project`, naming the scene the command used when it is
/// about an object that scene lacks, and the scene that has it, if one does.
fn placed(project: &Project, command: &Command, default: &str, message: String) -> String {
    let Some(id) = absent_object(&message) else {
        return message;
    };
    let asked = serde_json::to_value(command)
        .ok()
        .and_then(|value| value["scene"].as_str().map(str::to_owned));
    let Some(scene) = project
        .find_scene(asked.as_deref().unwrap_or(default), "scene")
        .ok()
    else {
        return message;
    };
    match project
        .scenes
        .iter()
        .find_map(|other| Some((other, other.entity(id)?)))
    {
        // Looked for in another scene than the command's own, as copy_entities' parent.
        Some((other, e)) if other.id == scene.id => {
            format!("{} não existe na cena em que o comando o procura", e.name)
        }
        Some((other, e)) => format!(
            "{} não existe na cena {} (a do comando); ele está na cena {}: passe scene",
            e.name, scene.name, other.name
        ),
        // A name: of objects of other scenes (see `resolve_names`), of one of this scene looked
        // for in another, or given to a legacy node command, which takes IDs.
        None => {
            let legacy = !takes_names(command);
            let (own, others): (Vec<_>, Vec<_>) = project
                .scenes
                .iter()
                .flat_map(|s| s.entities_named(id).into_iter().map(move |e| (s, e)))
                .partition(|(s, _)| s.id == scene.id);
            match (own.as_slice(), others.as_slice()) {
                ([(_, e)], _) if legacy => format!(
                    "{} é o nome de um objeto da cena {}; os comandos de nós (legado) pedem o ID \
                     dele: {}",
                    e.name, scene.name, e.id
                ),
                ([(_, e)], _) => {
                    format!("{} não existe na cena em que o comando o procura", e.name)
                }
                ([_, _, ..], _) => scene.find_entity(id, "entity").err().unwrap_or(message),
                ([], [(other, e)]) if legacy => format!(
                    "{} é o nome de um objeto da cena {}, e não da {} (a do comando); os comandos \
                     de nós (legado) pedem o ID dele ({}) e a cena dele em scene",
                    e.name, other.name, scene.name, e.id
                ),
                ([], [(other, e)]) => format!(
                    "{} não existe na cena {} (a do comando); ele está na cena {}: passe scene",
                    e.name, scene.name, other.name
                ),
                ([], []) => format!(
                    "Objeto {id} não existe na cena {} (a do comando) nem em outra cena do projeto",
                    scene.name
                ),
                ([], many) => format!(
                    "{id} não existe na cena {} (a do comando); outras cenas têm {} objetos com \
                     esse nome: {}{}",
                    scene.name,
                    many.len(),
                    many.iter()
                        .map(|(s, e)| format!("{} (cena {})", e.id, s.name))
                        .collect::<Vec<_>>()
                        .join(", "),
                    if legacy {
                        "; os comandos de nós (legado) pedem o ID e a cena do objeto em scene"
                    } else {
                        ": passe scene"
                    }
                ),
            }
        }
    }
}
fn check_entities(scene: &Scene, ids: &[Id]) -> Result<(), String> {
    if ids.is_empty() {
        return Err("Informe ao menos um objeto".into());
    }
    match ids.iter().find(|id| scene.entity(id).is_none()) {
        Some(id) => Err(absent(id)),
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

/// The links a connect or disconnect gives: the one of its four fields, or those of `links`.
fn edges_given(single: [&Option<String>; 4], links: &[[String; 4]]) -> Result<Vec<Edge>, String> {
    if links.is_empty() {
        let [
            Some(from_node),
            Some(from_port),
            Some(to_node),
            Some(to_port),
        ] = single
        else {
            return Err(
                "Informe from_node, from_port, to_node e to_port, ou links: \
                        [[from_node, from_port, to_node, to_port], …]"
                    .into(),
            );
        };
        return Ok(vec![Edge {
            from_node: from_node.clone(),
            from_port: from_port.clone(),
            to_node: to_node.clone(),
            to_port: to_port.clone(),
        }]);
    }
    if single.iter().any(|field| field.is_some()) {
        return Err("Use links ou from_node, from_port, to_node e to_port, não os dois".into());
    }
    Ok(links
        .iter()
        .map(|[from_node, from_port, to_node, to_port]| Edge {
            from_node: from_node.clone(),
            from_port: from_port.clone(),
            to_node: to_node.clone(),
            to_port: to_port.clone(),
        })
        .collect())
}

/// Removes `edges` from the logic of `entity` in `scene`, in order; the first one that is not
/// there fails them all, by its place in `links` when `numbered`, with the nodes by name.
fn unlink_all(
    project: &mut Project,
    scene: &str,
    entity: &str,
    edges: &[Edge],
    numbered: bool,
) -> Result<(), String> {
    let place = |index: usize, problem: String| {
        if numbered {
            format!("Ligação {index} de links (a {}ª): {problem}", index + 1)
        } else {
            problem
        }
    };
    let owner = project
        .scene(scene)
        .unwrap()
        .entity(entity)
        .ok_or_else(|| absent(entity))?;
    let mut left = owner.graph.edges.clone();
    for (index, edge) in edges.iter().enumerate() {
        let mut named = Vec::new();
        for id in [&edge.from_node, &edge.to_node] {
            match owner.graph.node(id) {
                Some(node) => named.push(node_named(node)),
                None => return Err(place(index, missing_node(project, &owner.name, id))),
            }
        }
        let Some(at) = left.iter().position(|e| e == edge) else {
            let again = edges[..index].iter().position(|e| e == edge);
            return Err(place(
                index,
                match again {
                    Some(first) => format!("a ligação {first} de links já desliga essas portas"),
                    None => format!(
                        "A saída {} de {} não está ligada à entrada {} de {}",
                        edge.from_port, named[0], edge.to_port, named[1]
                    ),
                },
            ));
        };
        left.remove(at);
    }
    entity_mut(scene_mut(project, scene), entity)?.graph.edges = left;
    Ok(())
}

/// How messages name an asset kind: alone, with "um" or "uma", with "o" or "a", and many.
fn asset_kind(kind: AssetKind) -> (&'static str, &'static str, &'static str, &'static str) {
    match kind {
        AssetKind::Texture => ("textura", "uma textura", "a textura", "texturas"),
        AssetKind::Audio => ("áudio", "um áudio", "o áudio", "áudios"),
        AssetKind::Model => ("modelo", "um modelo", "o modelo", "modelos"),
    }
}

/// An asset of any kind by ID or, when no asset has that ID, by name (ignoring case); `kind`
/// keeps only assets of that kind, telling apart a model and a sound that share a name.
fn find_asset(project: &Project, asset: &str, kind: Option<AssetKind>) -> Result<Id, String> {
    // Said as when the ID is of an asset of another kind.
    let other_kind = |found: &[&Asset], kind: AssetKind| {
        let kinds: BTreeSet<&str> = found.iter().map(|a| asset_kind(a.kind).1).collect();
        format!(
            "O recurso {} é {}, não {}",
            found[0].name,
            kinds.into_iter().collect::<Vec<_>>().join(" e "),
            asset_kind(kind).1
        )
    };
    let path = asset.trim().replace('\\', "/");
    if let Some(found) = project.asset(asset).or_else(|| {
        project
            .assets
            .iter()
            .find(|a| !a.path.is_empty() && crate::names::same_name(&a.path, &path))
    }) {
        return match kind {
            Some(kind) if kind != found.kind => Err(other_kind(&[found], kind)),
            _ => Ok(found.id.clone()),
        };
    }
    let wanted = asset.trim().to_lowercase();
    let same_name = |a: &&Asset| a.name.trim().to_lowercase() == wanted;
    let of_kind = |a: &&Asset| kind.is_none_or(|kind| a.kind == kind);
    let named: Vec<&Asset> = project
        .assets
        .iter()
        .filter(of_kind)
        .filter(same_name)
        .collect();
    let shown = |a: &Asset| format!("{} ({})", a.name, asset_kind(a.kind).0);
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => {
            if let Some(kind) = kind {
                let others: Vec<&Asset> = project.assets.iter().filter(same_name).collect();
                if !others.is_empty() {
                    return Err(other_kind(&others, kind));
                }
            }
            let mut listed: Vec<String> =
                project.assets.iter().filter(of_kind).map(shown).collect();
            first_ten(&mut listed);
            let many = kind.map_or("recursos", |kind| asset_kind(kind).3);
            Err(if listed.is_empty() {
                format!("O recurso {asset} não existe no projeto, que não tem {many}")
            } else {
                format!(
                    "O recurso {asset} não existe no projeto. {}: {}",
                    uppercase_first(many),
                    listed.join(", ")
                )
            })
        }
        many => {
            let kinds: BTreeSet<&str> = many.iter().map(|a| asset_kind(a.kind).0).collect();
            // Of different kinds, the kind tells them apart; of one kind, only the ID does,
            // which the message leaves to oxy_project: images and sounds by their files. In a
            // project in folders the path of the file tells any of them apart.
            if project.layout == Layout::Tree {
                let mut listed: Vec<String> = many
                    .iter()
                    .map(|a| format!("{} ({}, {})", a.name, asset_kind(a.kind).0, a.path))
                    .collect();
                first_ten(&mut listed);
                let kinds = if kinds.len() == many.len() {
                    format!(
                        "kind ({}) ou ",
                        many.iter()
                            .map(|a| format!("{:?}", a.kind))
                            .collect::<Vec<_>>()
                            .join(" ou ")
                    )
                } else {
                    String::new()
                };
                return Err(format!(
                    "\"{}\" é o nome de {} recursos: {}; passe {kinds}o caminho do arquivo",
                    asset.trim(),
                    many.len(),
                    listed.join(", ")
                ));
            }
            if kinds.len() == many.len() {
                Err(format!(
                    "\"{}\" é o nome de {} recursos: {}; passe kind ({})",
                    asset.trim(),
                    many.len(),
                    many.iter().map(|a| shown(a)).collect::<Vec<_>>().join(", "),
                    many.iter()
                        .map(|a| format!("{:?}", a.kind))
                        .collect::<Vec<_>>()
                        .join(" ou ")
                ))
            } else {
                let mut listed: Vec<String> = many
                    .iter()
                    .map(|a| match a.path.as_str() {
                        "" => shown(a),
                        path => format!("{} ({}, {path})", a.name, asset_kind(a.kind).0),
                    })
                    .collect();
                first_ten(&mut listed);
                Err(format!(
                    "\"{}\" é o nome de {} recursos: {}; passe o ID que o oxy_project lista",
                    asset.trim(),
                    many.len(),
                    listed.join(", ")
                ))
            }
        }
    }
}

/// Refuses deleting the asset `id` while anything uses it, naming the uses.
fn refuse_used(project: &Project, id: &str) -> Result<(), String> {
    let mut uses = editing::asset_references(project, id);
    if uses.is_empty() {
        return Ok(());
    }
    let removed = project.asset(id).unwrap();
    first_ten(&mut uses);
    // A placement keeps its pieces as they are; deleting its root takes them away.
    let placements = if removed.kind == AssetKind::Model {
        " (as colocações do modelo saem com delete_entities das raízes listadas)"
    } else {
        ""
    };
    Err(format!(
        "Não é possível excluir {} {}, que ainda está em uso; tire estes usos antes{placements}:\n{}",
        asset_kind(removed.kind).2,
        removed.name,
        uses.join("\n")
    ))
}

/// Links `edges` in the logic of `entity` in `scene`, in order; the first one refused fails
/// them all, by its place in `links` when `numbered`.
fn link_all(
    project: &mut Project,
    scene: &str,
    entity: &str,
    edges: &[Edge],
    numbered: bool,
) -> Result<(), String> {
    let place = |index: usize, problem: String| {
        if numbered {
            format!("Ligação {index} de links (a {}ª): {problem}", index + 1)
        } else {
            problem
        }
    };
    let current = project.scene(scene).unwrap();
    let objects: HashSet<Id> = current.entities.iter().map(|e| e.id.clone()).collect();
    let owner = current.entity(entity).ok_or_else(|| absent(entity))?;
    for (index, edge) in edges.iter().enumerate() {
        for id in [&edge.from_node, &edge.to_node] {
            if owner.graph.node(id).is_none() {
                return Err(place(index, missing_node(project, &owner.name, id)));
            }
        }
    }
    let graph = &mut entity_mut(scene_mut(project, scene), entity)?.graph;
    for (index, edge) in edges.iter().enumerate() {
        link_one(graph, edge, &objects).map_err(|problem| place(index, problem))?;
    }
    Ok(())
}

/// Why the logic of the object `owner` lacks the node `id`: it is in another object's logic, or
/// in none.
fn missing_node(project: &Project, owner: &str, id: &str) -> String {
    for e in project.scenes.iter().flat_map(|s| &s.entities) {
        if let Some(node) = e.graph.node(id) {
            return format!(
                "O nó {} é da lógica de {}, não da de {owner}",
                node_named(node),
                e.name
            );
        }
    }
    format!("{owner} não tem o nó {id} na lógica")
}

/// A node as messages name it: its label, then its operation.
fn node_named(node: &Node) -> String {
    let label = graph::definition(&node.operation).map_or("", |def| def.label);
    format!("{label} ({})", node.operation)
}

/// Links `edge`, both of whose nodes `graph` has, saying by name what refuses it: a port the
/// node lacks, a data input already linked, types that do not match; then the graph's own rules
/// (cycles, limits).
fn link_one(graph: &mut graph::Graph, edge: &Edge, objects: &HashSet<Id>) -> Result<(), String> {
    let (Some(from), Some(to)) = (graph.node(&edge.from_node), graph.node(&edge.to_node)) else {
        return Err("Nó ausente na ligação".into());
    };
    let ports = |list: &[graph::PortDef]| {
        list.iter()
            .map(|port| port.id)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let (Some(from_def), Some(to_def)) = (
        graph::definition(&from.operation),
        graph::definition(&to.operation),
    ) {
        let Some(output) = from_def.outputs.iter().find(|p| p.id == edge.from_port) else {
            return Err(format!(
                "O nó {} não tem a saída {}; saídas: {}",
                node_named(from),
                edge.from_port,
                ports(&from_def.outputs)
            ));
        };
        let Some(input) = to_def.inputs.iter().find(|p| p.id == edge.to_port) else {
            return Err(format!(
                "O nó {} não tem a entrada {}; entradas: {}",
                node_named(to),
                edge.to_port,
                ports(&to_def.inputs)
            ));
        };
        if graph.edges.contains(edge) {
            return Err("Essas portas já estão ligadas".into());
        }
        if !graph::compatible(output.kind, input.kind) {
            return Err(format!(
                "Conexão incompatível: {} → {} (a saída {} de {} na entrada {} de {})",
                output.kind.label(),
                input.kind.label(),
                edge.from_port,
                node_named(from),
                edge.to_port,
                node_named(to)
            ));
        }
        if input.kind != PortType::Exec
            && graph
                .edges
                .iter()
                .any(|e| e.to_node == edge.to_node && e.to_port == edge.to_port)
        {
            return Err(format!(
                "A entrada {} de {} já possui uma conexão de dados; desligue-a antes \
                 (disconnect)",
                edge.to_port,
                node_named(to)
            ));
        }
    }
    // A link back to where it starts would close a loop.
    let mut seen = HashSet::new();
    let mut ahead = vec![edge.to_node.as_str()];
    while let Some(id) = ahead.pop() {
        if id == edge.from_node {
            return Err(format!(
                "A ligação de {} para {} fecharia um ciclo, pois {} já leva a {}; use eventos e \
                 Esperar para retomadas finitas",
                node_named(from),
                node_named(to),
                node_named(to),
                node_named(from)
            ));
        }
        if seen.insert(id) {
            ahead.extend(
                graph
                    .edges
                    .iter()
                    .filter(|e| e.from_node == id)
                    .map(|e| e.to_node.as_str()),
            );
        }
    }
    graph.connect(edge, objects)
}

/// Applies one command to `project`, answering the IDs it created; `checks` takes what the
/// batch checks once it ends, and `copies` what a copy command copied ([`Applied::copies`]).
fn apply_one(
    project: &mut Project,
    default: &str,
    command: &Command,
    checks: &mut Vec<Check>,
    copies: &mut Vec<Arc<CopyMap>>,
    scripts: &ScriptSources,
) -> Result<Vec<Id>, String> {
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
        C::SetGameSettings {
            title,
            window,
            fullscreen,
        } => {
            let game = &mut project.game;
            if let Some(title) = title {
                game.title = title.trim().into();
            }
            if let Some(window) = window {
                game.window = *window;
            }
            if let Some(fullscreen) = fullscreen {
                game.fullscreen = *fullscreen;
            }
            game.validate()?;
            Ok(vec![])
        }
        C::CreateScene { name, kind } => {
            if name.trim().is_empty() {
                return Err("Informe um nome para a cena".into());
            }
            let scene = editing::new_scene(name.trim(), (*kind).into());
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
            let (copy, map) = editing::duplicate_scene_mapped(project, &id)?;
            copies.push(Arc::new(map));
            Ok(vec![copy])
        }
        C::DeleteScene { scene } => {
            let id = scene_id(project, &Some(scene.clone()), default)?;
            editing::delete_scene(project, &id)?;
            Ok(vec![])
        }
        C::SetSceneBackground { scene, background } => {
            let id = scene_id(project, scene, default)?;
            let scene = scene_mut(project, &id);
            // Validation refuses the sky in a 2D scene and colors out of range.
            scene.background = match *background {
                BackgroundSpec::Sky => Some(Background::Sky),
                BackgroundSpec::Color(color) => Some(Background::Color(color)),
                BackgroundSpec::Default => Background::default_for(scene.kind),
            };
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
            let (roots, maps) = editing::duplicate_repeated(
                scene,
                ids,
                offset.unwrap_or(editing::DUPLICATE_OFFSET),
                count.unwrap_or(1),
            )?;
            // Each copy answers one root per object duplicated.
            let per_copy = roots.len() / maps.len().max(1);
            for map in maps {
                copies.extend(std::iter::repeat_n(Arc::new(map), per_copy));
            }
            Ok(roots)
        }
        C::CopyEntities {
            scene,
            ids,
            to_scene,
            parent,
        } => {
            let from = scene_id(project, scene, default)?;
            let to = scene_named(project, to_scene, "to_scene")?;
            let (roots, map) = copy_entities(project, &from, ids, &to, parent.as_ref())?;
            copies.extend(std::iter::repeat_n(Arc::new(map), roots.len()));
            // Logic that opens the scene it came from, such as a restart, still opens it.
            let target = project.scene(&to).unwrap();
            let back = from.as_str();
            let nodes: Vec<(Id, Id, &str)> = roots
                .iter()
                .flat_map(|root| target.descendants(root))
                .filter_map(|id| target.entity(&id))
                .flat_map(|e| {
                    e.graph.nodes.iter().flat_map(move |n| {
                        graph::node_references(n)
                            .filter(move |(param, reference, _)| {
                                *reference == Reference::Scene && opens_scene(e, n, param.id, back)
                            })
                            .map(move |(param, ..)| (e.id.clone(), n.id.clone(), param.id))
                    })
                })
                .collect();
            if from != to && !nodes.is_empty() {
                checks.push(Check::SceneBack {
                    scene: to,
                    from,
                    nodes,
                });
            }
            Ok(roots)
        }
        C::SaveModel {
            scene,
            entity,
            name,
            replace,
            folder,
        } => {
            if folder.is_some() && project.layout == Layout::Legacy {
                return Err(tree::LEGACY.into());
            }
            let id = scene_id(project, scene, default)?;
            let replaced = replace
                .as_deref()
                .map(|model| find_model(project, model))
                .transpose()?;
            let source = project.scene(&id).unwrap();
            check_entities(source, std::slice::from_ref(entity))?;
            let name = match name {
                Some(name) if name.trim().is_empty() => {
                    return Err("Informe um nome para o modelo".into());
                }
                Some(name) => name.trim().to_owned(),
                None => match &replaced {
                    Some(model) => project.asset(model).unwrap().name.clone(),
                    None => source.entity(entity).unwrap().name.clone(),
                },
            };
            // place_model takes a model by name, which two models would share.
            if let Some(other) = project
                .model_named(&name)
                .filter(|other| Some(&other.id) != replaced.as_ref())
            {
                let replace = if replaced.is_none() {
                    ", ou troque o conteúdo desse modelo com replace"
                } else {
                    ""
                };
                return Err(format!(
                    "Já existe um modelo chamado \"{}\" ({}); escolha outro nome com \
                     name{replace}",
                    other.name, other.id
                ));
            }
            let pieces: HashSet<Id> = source.descendants(entity).into_iter().collect();
            refuse_outside(source, &pieces, "do modelo")?;
            let (model, map) = match replaced {
                Some(model) => {
                    let map = project.replace_model_mapped(&model, &id, entity, name)?;
                    (model, map)
                }
                None => project.save_model_mapped_in(&id, entity, name, folder.as_deref())?,
            };
            copies.push(Arc::new(map));
            Ok(vec![model])
        }
        C::PlaceModel {
            scene,
            model,
            parent,
            position,
            name,
        } => {
            let id = scene_id(project, scene, default)?;
            let (pieces, map) = place_model(
                project,
                &id,
                model,
                parent.as_ref(),
                *position,
                name.as_deref(),
            )?;
            copies.extend(std::iter::repeat_n(Arc::new(map), pieces.len()));
            Ok(pieces)
        }
        C::RenameAsset { asset, name, kind } => {
            if *kind == Some(ItemKind::Script) {
                return Err(script_files_need(project).into());
            }
            let id = find_asset(project, asset, kind.and_then(ItemKind::asset))?;
            let name = name.trim();
            if name.is_empty() {
                return Err("Informe um nome para o recurso".into());
            }
            let renamed = project.asset(&id).unwrap();
            if project.layout == Layout::Tree && renamed.name != name {
                return Err(crate::document::renaming_files(&renamed.name));
            }
            // Assets are found by name, which two of a kind would share.
            if let Some(other) = project.assets.iter().find(|a| {
                a.id != id
                    && a.kind == renamed.kind
                    && a.name.trim().to_lowercase() == name.to_lowercase()
            }) {
                return Err(format!(
                    "Já existe {} com o nome \"{}\"; escolha outro nome",
                    asset_kind(other.kind).1,
                    other.name
                ));
            }
            let renamed = project.assets.iter_mut().find(|a| a.id == id).unwrap();
            renamed.name = name.to_owned();
            Ok(vec![])
        }
        C::DeleteAsset { asset, kind, .. } => {
            if *kind == Some(ItemKind::Script) {
                return Err(script_files_need(project).into());
            }
            let id = find_asset(project, asset, kind.and_then(ItemKind::asset))?;
            refuse_used(project, &id)?;
            editing::remove_asset(project, &id)?;
            Ok(vec![])
        }
        C::CreateFolder { .. }
        | C::RenameFolder { .. }
        | C::Move { .. }
        | C::DeleteFolder { .. } => Err(tree::NO_FOLDER.into()),
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
                .ok_or_else(|| absent(entity))?;
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
                    let value = value.value(None, &format!("O atributo {}", name.trim()))?;
                    e.attributes.insert(name.trim().into(), value);
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
                .ok_or_else(|| absent(entity))?;
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
        C::AttachScript {
            scene,
            entity,
            path,
            index,
        } => {
            let id = scene_id(project, scene, default)?;
            let given = script::normalize_path(path)?;
            // The path in the case the disk has, as it is kept.
            let Some(path) = scripts.known(&given).map(str::to_owned) else {
                return Err(scripts.absent(&given));
            };
            if path != given {
                checks.push(Check::ScriptCase {
                    given,
                    path: path.clone(),
                });
            }
            // A Superfície declared by name is the project's of that name.
            let surfaces = project.surfaces.clone();
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let at = e.scripts.iter().position(|s| s.path == path);
            let last = e.scripts.len() - usize::from(at.is_some());
            if let Some(index) = *index
                && index > last
            {
                return Err(format!(
                    "index {index} fica fora da lista de scripts de {} (de 0 a {last})",
                    e.name
                ));
            }
            let attached = match at {
                Some(at) => e.scripts.remove(at),
                None => ScriptRef::new(path.clone()),
            };
            e.scripts
                .insert(index.unwrap_or(at.unwrap_or(last)), attached);
            // What every script of the object declares, for the warnings when the batch ends;
            // only the one attached gives the attributes it declares.
            let paths: Vec<String> = e.scripts.iter().map(|s| s.path.clone()).collect();
            let mut declared = Vec::new();
            for (each, result) in
                script::declared_attributes(scripts, &surfaces, paths.iter().map(String::as_str))
            {
                match result {
                    Ok(attributes) => {
                        if each == path {
                            for (name, default) in &attributes {
                                e.attributes
                                    .entry(name.clone())
                                    .or_insert_with(|| default.clone());
                            }
                        }
                        declared.push((each.to_owned(), attributes));
                    }
                    Err(error) if each == path => checks.push(Check::UnreadScript {
                        scene: id.clone(),
                        entity: e.id.clone(),
                        path: path.clone(),
                        error: error.to_string(),
                    }),
                    // Told where it is attached, or by the project's checks when it is missing.
                    Err(_) => {}
                }
            }
            // Callbacks that would never run on it, told when the batch ends: a later command
            // may give it the area.
            if let Some((_, Ok(outline))) = script::outlines(scripts, [path.as_str()]).pop() {
                checks.push(Check::UnheardCallbacks {
                    scene: id.clone(),
                    entity: e.id.clone(),
                    path: path.clone(),
                    callbacks: outline.callbacks,
                });
            }
            checks.push(Check::DeclaredAttributes {
                scene: id,
                entity: e.id.clone(),
                declared,
            });
            Ok(vec![])
        }
        C::ClearLogic { scene, entity } => {
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            if e.graph.nodes.is_empty() {
                checks.push(Check::Note(format!(
                    "{} não tinha lógica de nós; nada mudou nele",
                    e.name
                )));
            }
            e.graph = graph::Graph::default();
            Ok(vec![])
        }
        C::DetachScript {
            scene,
            entity,
            path,
        } => {
            let id = scene_id(project, scene, default)?;
            // A path saved before a rule of names came is still taken off.
            let path = script::normal_form(path)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let Some(at) = e.scripts.iter().position(|s| s.path == path) else {
                return Err(format!("{} não tem o script {path}", e.name));
            };
            e.scripts.remove(at);
            Ok(vec![])
        }
        C::SetScriptEnabled {
            scene,
            entity,
            path,
            enabled,
        } => {
            let id = scene_id(project, scene, default)?;
            let path = script::normal_form(path)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            let name = e.name.clone();
            let script = e
                .scripts
                .iter_mut()
                .find(|s| s.path == path)
                .ok_or_else(|| format!("{name} não tem o script {path}"))?;
            script.enabled = *enabled;
            Ok(vec![])
        }
        C::FitCollider {
            scene,
            entity,
            children,
            margin,
            shape,
        } => {
            let id = scene_id(project, scene, default)?;
            spatial::fit_collider(
                scene_mut(project, &id),
                entity,
                *children,
                margin.unwrap_or(0.),
                *shape,
            )?;
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
            let patch = filter_bits(project, patch)?;
            let scene = scene_mut(project, &id);
            let fitted = box_fit(scene, entity);
            let e = entity_mut(scene, entity)?;
            let mut value = serde_json::to_value(&*e).map_err(|e| e.to_string())?;
            merge_patch(&mut value, &patch);
            if value.get("id").and_then(Json::as_str) != Some(entity.as_str()) {
                return Err("O patch não pode mudar o ID do objeto".into());
            }
            // Reads add it to keyframes, which keep only the rotation in radians.
            let degrees = value["clips"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|clip| clip["tracks"].as_array())
                .flatten()
                .filter_map(|track| track["keyframes"].as_array())
                .flatten()
                .any(|keyframe| keyframe.get("rotation_degrees").is_some());
            if degrees {
                return Err(
                    "Patch inválido: os quadros-chave das animações não guardam \
                            rotation_degrees. A rotação deles fica em transform.rotation, em \
                            radianos; para escrevê-la em graus, use set_clip."
                        .into(),
                );
            }
            let dimensions = e.dimensions;
            // A collider made from the shape keeps the geometry: one that matched the old shape
            // no longer matches the new one. Once it stays behind, later patches add nothing.
            let matched = stale_geometry(e) == Some(false);
            *e = serde_json::from_value(value).map_err(|e| format!("Patch inválido: {e}"))?;
            if matched && stale_geometry(e) == Some(true) {
                checks.push(Check::StaleGeneratedCollider {
                    scene: id.clone(),
                    entity: entity.clone(),
                });
            }
            let resized = !near(e.dimensions, dimensions);
            // A box collider follows the object's scale but not its dimensions: one that fit
            // the old shape (its dimensions or fit_collider's box, margin and all), and that
            // the patch left as it was, no longer fits the new one.
            if let Some((size, true)) = fitted
                && resized
                && box_fit(scene, entity) == Some((size, false))
            {
                checks.push(Check::StaleCollider {
                    scene: id,
                    entity: entity.clone(),
                    size,
                });
            }
            Ok(vec![])
        }
        C::AddNode {
            scene,
            entity,
            operation,
            position,
            params,
            links,
        } => {
            let id = scene_id(project, scene, default)?;
            let def = graph::registry()
                .iter()
                .find(|op| op.id == operation)
                .ok_or_else(|| format!("Nó {operation} não existe no catálogo"))?;
            let params = node_params(project, def, params)?;
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
            node.params.extend(params);
            let node_id = node.id.clone();
            e.graph.nodes.push(node);
            if !links.is_empty() {
                let edges: Vec<Edge> = links
                    .iter()
                    .map(|[from_node, from_port, to_port]| Edge {
                        from_node: from_node.clone(),
                        from_port: from_port.clone(),
                        to_node: node_id.clone(),
                        to_port: to_port.clone(),
                    })
                    .collect();
                link_all(project, &id, entity, &edges, true)?;
            }
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
            let operation = project
                .scene(&id)
                .unwrap()
                .entity(entity)
                .ok_or_else(|| absent(entity))?
                .graph
                .nodes
                .iter()
                .find(|n| n.id == *node)
                .ok_or_else(|| format!("Nó {node} não existe neste objeto"))?
                .operation
                .clone();
            let def = graph::registry()
                .iter()
                .find(|op| op.id == operation)
                .ok_or("Nó com operação desconhecida")?;
            let params = node_params(project, def, params)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            if let Some(target) = e.graph.nodes.iter_mut().find(|n| n.id == *node) {
                target.params.extend(params);
            }
            Ok(vec![])
        }
        C::Connect {
            scene,
            entity,
            from_node,
            from_port,
            to_node,
            to_port,
            links,
        } => {
            let id = scene_id(project, scene, default)?;
            let edges = edges_given([from_node, from_port, to_node, to_port], links)?;
            link_all(project, &id, entity, &edges, !links.is_empty())?;
            Ok(vec![])
        }
        C::Disconnect {
            scene,
            entity,
            from_node,
            from_port,
            to_node,
            to_port,
            links,
        } => {
            let id = scene_id(project, scene, default)?;
            let edges = edges_given([from_node, from_port, to_node, to_port], links)?;
            unlink_all(project, &id, entity, &edges, !links.is_empty())?;
            Ok(vec![])
        }
        C::ArrangeLogic {
            scene,
            entity,
            nodes,
            split_shared_data,
        } => {
            let id = scene_id(project, scene, default)?;
            let e = entity_mut(scene_mut(project, &id), entity)?;
            if e.graph.nodes.is_empty() {
                return Err(format!("{} não tem lógica para organizar", e.name));
            }
            let mut movable: HashSet<Id> = match nodes {
                None => e.graph.nodes.iter().map(|n| n.id.clone()).collect(),
                Some(nodes) if nodes.is_empty() => {
                    return Err(
                        "Informe ao menos um nó, ou deixe nodes de fora para organizar \
                                todos"
                            .into(),
                    );
                }
                Some(nodes) => {
                    if let Some(missing) = nodes.iter().find(|n| e.graph.node(n).is_none()) {
                        return Err(format!("Nó {missing} não existe neste objeto"));
                    }
                    nodes.iter().cloned().collect()
                }
            };
            let copies = if *split_shared_data {
                graph::layout::split_shared_data(&mut e.graph, &movable, entity)
            } else {
                Vec::new()
            };
            movable.extend(copies.iter().cloned());
            graph::arrange(&mut e.graph, &movable);
            checks.push(Check::LongWires {
                scene: id,
                entity: entity.clone(),
                nodes: nodes.clone(),
                split: *split_shared_data,
            });
            Ok(copies)
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
        C::CreateVariable {
            name,
            value,
            persistent,
        } => {
            let what = format!("O valor da variável {}", name.trim());
            let value = value.value(value.form().as_ref(), &what)?;
            Ok(vec![variables::create(project, name, value, *persistent)?])
        }
        C::UpdateVariable {
            variable,
            name,
            value,
            persistent,
        } => {
            let id = variables::find(project, variable)?;
            if let Some(name) = name {
                variables::rename(project, &id, name)?;
            }
            let value = match value {
                Some(value) => {
                    let current = variables::get(project, &id).expect("found above");
                    let what = format!("O valor da variável {}", current.name);
                    Some(value.value(Some(&current.value), &what)?)
                }
                None => None,
            };
            variables::update(project, &id, value, *persistent)?;
            Ok(vec![])
        }
        C::DeleteVariable { variable } => {
            let id = variables::find(project, variable)?;
            variables::remove(project, &id)?;
            Ok(vec![])
        }
    }
}

/// The parameters `given` to a node of `def`, as the node keeps them: plain values read by the
/// catalog's types (by their form where the input port takes any type), fixed choices checked,
/// and an input action, a game variable or a scene given by name kept by its ID, which is what
/// the game reads. An action the project lacks stays as given, and the batch warns about it; a
/// name two actions share is refused, as by input action commands. A variable or a scene the
/// project lacks is refused.
fn node_params(
    project: &Project,
    def: &OperationDef,
    given: &BTreeMap<String, ParamValue>,
) -> Result<BTreeMap<String, Value>, String> {
    let mut params = BTreeMap::new();
    for (key, value) in given {
        let param = def
            .params
            .iter()
            .find(|p| p.id == key)
            .ok_or_else(|| format!("O nó {} não tem o parâmetro {key}", def.id))?;
        // An Any input takes every attribute type (attribute.set's value, condition.compare's
        // a and b), so its default is only where it starts, not its type.
        let any = def
            .inputs
            .iter()
            .any(|port| port.id == key && port.kind == PortType::Any);
        let expected = if any {
            value.form()
        } else {
            Some(param.default.clone())
        };
        let mut value = value.value(
            expected.as_ref(),
            &format!("O parâmetro {key} do nó {}", def.id),
        )?;
        if graph::reference(def.id, param) == Some(Reference::InputAction)
            && let Value::Text(action) = &mut value
            && let Some(id) = input_actions::find_named(project, action)?
        {
            *action = id;
        }
        // A variable the project lacks would be a broken reference: refused with the list.
        if graph::reference(def.id, param) == Some(Reference::Variable)
            && let Value::Text(variable) = &mut value
            && !variable.is_empty()
        {
            *variable = variables::find(project, variable)?;
        }
        // So would a scene: refused with what the parameter received and the scenes.
        if graph::reference(def.id, param) == Some(Reference::Scene)
            && let Value::Text(scene) = &mut value
            && !scene.is_empty()
        {
            *scene = scene_named(
                project,
                scene,
                &format!("O parâmetro {key} do nó {}", def.id),
            )?;
        }
        params.insert(key.clone(), value);
    }
    graph::check_choices(def.id, &params)?;
    graph::check_ranges(def.id, &params)?;
    Ok(params)
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
            pause_at_ends,
            spin,
            camera,
            filter,
        } => {
            let platform = *preset == Preset::Platform;
            if !platform
                && (travel.is_some()
                    || period.is_some()
                    || pause_at_ends.is_some()
                    || spin.is_some())
            {
                return Err(
                    "travel, period, pause_at_ends e spin valem só para o preset platform".into(),
                );
            }
            if platform && camera.is_some() {
                return Err("camera vale só para os presets first_person e third_person".into());
            }
            if let Some(travel) = travel {
                finite(travel, "travel")?;
            }
            if let Some(spin) = spin {
                finite(spin, "spin")?;
            }
            let defaults = PresetOptions::default();
            let options = PresetOptions {
                travel: travel.map_or(defaults.travel, Vec3::from),
                period: period.unwrap_or(defaults.period),
                pause_at_ends: pause_at_ends.unwrap_or(defaults.pause_at_ends),
                spin: spin.map_or(defaults.spin, Vec3::from),
                camera: camera.unwrap_or_default(),
            };
            let preset = match preset {
                Preset::FirstPerson => MovementPreset::FirstPerson,
                Preset::ThirdPerson => MovementPreset::ThirdPerson,
                Preset::Platform => MovementPreset::Platform,
            };
            let bits = |value: &Option<Json>, key: &str| {
                value
                    .as_ref()
                    .map(|value| {
                        group_bits(project, value).map_err(|error| format!("filter.{key}: {error}"))
                    })
                    .transpose()
            };
            let (category, mask) = match filter {
                Some(filter) => (
                    bits(&filter.category, "category")?,
                    bits(&filter.mask, "mask")?,
                ),
                None => (None, None),
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
            let filter = match (&mut e.character3d, &mut e.physics3d) {
                (Some(character), _) => Some(&mut character.body.filter),
                (None, Some(collider)) => Some(&mut collider.filter),
                (None, None) => None,
            };
            if let Some(filter) = filter {
                filter.category = category.unwrap_or(filter.category);
                filter.mask = mask.unwrap_or(filter.mask);
            }
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
pub(crate) fn check_kind(entities: &[Entity], kind: SceneKind, scene: &str) -> Result<(), String> {
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
            for (param, reference, _) in graph::node_references(node) {
                let clip = node.text(param.id);
                if reference == Reference::Clip && !clip.is_empty() && !clips.contains(clip) {
                    found.push(format!(
                        "{} → nó {} → animação de outro objeto ({clip})",
                        e.name, node.operation
                    ));
                }
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
    first_ten(&mut found);
    Err(format!(
        "Estas referências apontam para fora {what}; leve junto os objetos e animações \
         que elas usam ou tire as referências antes:\n{}",
        found.join("\n")
    ))
}

/// Copies `ids` of the scene `from`, with everything under them, to the scene `to`. The copied
/// roots go under `parent` (or to the root of `to`) keeping their world transform, and the
/// keyframes that animate them change parent space with them, so they play the same path in
/// the world ([`editing::rebase_tracks`]). Answers the new roots, in order, and the IDs the
/// copy took (each object, node and animation to its copy).
fn copy_entities(
    project: &mut Project,
    from: &str,
    ids: &[Id],
    to: &str,
    parent: Option<&Id>,
) -> Result<(Vec<Id>, CopyMap), String> {
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
    // Each root's old parent space, which the copy leaves for the destination's.
    let spaces = roots
        .iter()
        .map(|id| {
            let parent = source.entity(id).and_then(|e| e.parent.as_deref());
            Ok(match parent {
                Some(parent) => source.world_matrix(parent)?,
                None => Mat4::IDENTITY,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let map: HashMap<_, _> = copies.iter().map(|e| (e.id.clone(), new_id())).collect();
    let map = remap_entities(&mut copies, &map)?;
    for (root, space) in roots.iter().zip(spaces) {
        let id = &map[root];
        let entity = copies.iter_mut().find(|e| e.id == *id).unwrap();
        let name = entity.name.clone();
        let before = entity.transform.clone();
        let change = parent_world.inverse() * space;
        let local = change * before.matrix();
        // An unchanged local transform stays exact, without the rounding of the matrices, and
        // so do the keyframes that animate it.
        if before.matrix().abs_diff_eq(local, 1e-6) {
            entity.parent = parent.cloned();
            continue;
        }
        let mut after = Transform::from_matrix(local, before.pivot);
        after.rotation = editing::angles_near(after.quaternion(), before.rotation);
        if !after.finite() || !after.matrix().abs_diff_eq(local, 0.0001) {
            return Err(format!(
                "{name} ficaria deformado sob o pai de destino (cisalhamento); escolha outro \
                 pai ou ajuste a escala não uniforme dele"
            ));
        }
        entity.parent = parent.cloned();
        entity.transform = after.clone();
        // A change of parent space, not an edit: the path scales and turns with the space.
        editing::rebase_tracks(&mut copies, id, change, &before, &after).map_err(|_| {
            format!(
                "A animação de {name} ficaria deformada sob o pai de destino (cisalhamento); \
                 escolha outro pai ou ajuste a escala não uniforme dele"
            )
        })?;
    }
    let created = roots.iter().map(|root| map[root].clone()).collect();
    scene_mut(project, to).entities.extend(copies);
    Ok((created, map))
}

/// A model asset by ID, else by the path of its file (`Pista/Checkpoint.modelo.json`, with or
/// without the extension), else by name (ignoring case).
pub(crate) fn find_model(project: &Project, model: &str) -> Result<Id, String> {
    if let Some(asset) = project.asset(model) {
        return if asset.kind == AssetKind::Model && asset.model.is_some() {
            Ok(asset.id.clone())
        } else {
            Err(format!("O recurso {} não é um modelo", asset.name))
        };
    }
    let path = model.trim().replace('\\', "/");
    let with_extension = format!("{path}{}", crate::model_file::EXTENSION);
    if let Some(asset) = project.assets.iter().find(|a| {
        a.kind == AssetKind::Model
            && !a.path.is_empty()
            && (crate::names::same_name(&a.path, &path)
                || path.contains('/') && crate::names::same_name(&a.path, &with_extension))
    }) {
        return Ok(asset.id.clone());
    }
    let wanted = model.trim().to_lowercase();
    let named: Vec<&Asset> = project
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::Model && a.name.trim().to_lowercase() == wanted)
        .collect();
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => {
            let models: Vec<String> = project
                .assets
                .iter()
                .filter(|a| a.kind == AssetKind::Model)
                // A model's file tells it apart in a project in folders, its ID in an old one.
                .map(|a| match a.path.as_str() {
                    "" => format!("{} ({})", a.name, a.id),
                    path => format!("{} ({path})", a.name),
                })
                .collect();
            Err(if models.is_empty() {
                format!(
                    "Modelo {model} não existe no projeto, que não tem modelos; crie um com \
                     save_model"
                )
            } else {
                format!(
                    "Modelo {model} não existe no projeto. Modelos: {}",
                    models.join(", ")
                )
            })
        }
        many if many.iter().all(|a| !a.path.is_empty()) => Err(format!(
            "{} modelos se chamam \"{}\" ({}); passe o caminho do arquivo de um deles",
            many.len(),
            model.trim(),
            many.iter()
                .map(|a| a.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        many => Err(format!(
            "{} modelos se chamam \"{}\"; use o ID",
            many.len(),
            model.trim()
        )),
    }
}

/// Places a copy of `model` (ID or name) in `scene`, its root under `parent` at `position`.
/// Answers every piece, the root first, and the IDs the copy took (each piece, node and
/// animation of the model to its copy).
fn place_model(
    project: &mut Project,
    scene: &str,
    model: &str,
    parent: Option<&Id>,
    position: Option<[f32; 3]>,
    name: Option<&str>,
) -> Result<(Vec<Id>, CopyMap), String> {
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
    let (root, map) = project.instantiate_model_mapped(&asset, scene)?;
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
    Ok((created, map))
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

/// The logic nodes that name the animation `id`, in scenes and models.
fn clip_uses(project: &Project, id: &str) -> Vec<String> {
    every_entity(project)
        .flat_map(|(scope, e)| {
            e.graph
                .nodes
                .iter()
                .filter(|n| {
                    graph::node_references(n).any(|(param, reference, _)| {
                        reference == Reference::Clip && n.text(param.id) == id
                    })
                })
                .filter_map(move |n| {
                    let label = graph::definition(&n.operation)?.label;
                    Some(format!("{scope} → {} → {label} ({})", e.name, n.id))
                })
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
    let entity = scene.entity(owner).ok_or_else(|| absent(owner))?;
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

/// The collision filters' paths in an object's document form: its 3D collider's and its 3D
/// character body's.
pub(crate) const FILTERS: [&[&str]; 2] =
    [&["physics3d", "filter"], &["character3d", "body", "filter"]];

/// `patch` with the category and mask of its collision filters given as group names turned
/// into the bits they stand for, and bits checked.
fn filter_bits(project: &Project, patch: &Json) -> Result<Json, String> {
    let mut patch = patch.clone();
    for path in FILTERS {
        let Some(filter) = path
            .iter()
            .try_fold(&mut patch, |part, key| part.get_mut(*key))
        else {
            continue;
        };
        for key in ["category", "mask"] {
            if let Some(value) = filter.get_mut(key)
                && !value.is_null()
            {
                *value = json!(
                    group_bits(project, value)
                        .map_err(|error| format!("{}.{key}: {error}", path.join(".")))?
                );
            }
        }
    }
    Ok(patch)
}

/// The bits a filter's category or mask gives: the bits themselves, a list of collision group
/// names (or one name), as oxy_project lists them, or "all" for every group.
pub(crate) fn group_bits(project: &Project, value: &Json) -> Result<u32, String> {
    let names: Vec<&str> = match value {
        Json::Number(bits) => {
            return bits
                .as_u64()
                .and_then(|bits| u32::try_from(bits).ok())
                .ok_or_else(|| {
                    format!(
                        "os bits são um número inteiro de 0 a {} (recebeu {bits})",
                        u32::MAX
                    )
                });
        }
        Json::String(all) if all.trim().eq_ignore_ascii_case("all") => return Ok(u32::MAX),
        Json::String(name) => vec![name],
        Json::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .ok_or("a lista leva nomes de grupos de colisão (texto)")
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(
                "informe um número (os bits), uma lista de nomes de grupos de colisão ou \"all\""
                    .into(),
            );
        }
    };
    names.into_iter().try_fold(0, |bits, name| {
        Ok(bits | 1 << project.collision_group_bit(name)?)
    })
}
