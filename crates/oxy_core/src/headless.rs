//! Runs a game without a window for a simulated time and reports what happened: scene
//! changes, logs, logic actions, sounds and changes of what plays, movement and area events,
//! the game pause, the game camera, the game interface, the state of watched objects, the game
//! variables, the progress saved, what plays and a hash of the whole state at the end. Used by
//! the MCP server so an agent can test a change, and by tests. On request a second copy runs
//! alongside on the same inputs, to check that the run repeats itself step by step, and a bot
//! plays one character along a path of points (see [`crate::bot`]).
//!
//! Everything runs on a copy of the project: `setup` commands and `spawn` change only that
//! copy. Timed inputs live here, not in the runtime, so a scene change or reload neither
//! drops the inputs still to come nor releases the keys being held. Area crossings, movement
//! events and spawned objects come from the runtime's `Recording`, taken when they happen, so
//! logic that removes the objects involved in the same step cannot hide them.
use crate::{
    audio::{AudioChange, AudioSettings, ChangeKind},
    bot::{self, Bot, BotEvent, BotState},
    character::{CameraMode, CharacterState, MovementEvent, Posture},
    command::{self, Applied, Command},
    document::*,
    game_ui::{self, UiState},
    graph::{self, Edge, Graph, PortType},
    input_actions,
    input_timeline::{InputChange, InputTimeline, TimedInput},
    random,
    runtime::{FIXED_DT, InputFrame, NodeTrace, Recording, Runtime, RuntimeOptions},
    save::{self, SaveRequest, SaveSlot},
    variables,
};
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
};

/// Longest simulation one request may ask for.
pub const MAX_SECONDS: f64 = 600.;
/// Most entries one report list keeps; older ones are dropped and counted in `truncated`.
pub const MAX_ENTRIES: usize = 300;
/// Game view the interface is laid out in when a request gives no size, in pixels.
pub const VIEW_SIZE: [u32; 2] = [960, 540];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    /// Starts holding an input action of the project: its ID, its name or `"$N"` from setup.
    /// Actions the project does not have are refused.
    Press {
        at: f64,
        action: String,
    },
    Release {
        at: f64,
        action: String,
    },
    /// Analog movement intent: right and forward, -1 to 1.
    Movement {
        at: f64,
        value: [f32; 2],
    },
    /// Relative look displacement.
    Look {
        at: f64,
        value: [f32; 2],
    },
    /// Clicks an object or its interface button, by its ID or name. A hidden element takes no
    /// click, nor does a world object in the game pause, as in the game, nor an object that is
    /// not in the running scene (a log line says so).
    Click {
        at: f64,
        entity: Id,
    },
    /// What Esc does in the game: pauses or resumes it when the scene has a pause menu (logic
    /// listening to Ao pausar ou retomar); otherwise a log line says there is none.
    Escape {
        at: f64,
    },
}
impl InputEvent {
    fn at(&self) -> f64 {
        match self {
            Self::Press { at, .. }
            | Self::Release { at, .. }
            | Self::Movement { at, .. }
            | Self::Look { at, .. }
            | Self::Click { at, .. }
            | Self::Escape { at } => *at,
        }
    }
}

/// How the report lists the logic actions that ran.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionsMode {
    None,
    /// One entry per object name and action, with count and first and last time: objects
    /// that share a name, such as spawned copies, share a line. The `MAX_ENTRIES` lines most
    /// recently active stay.
    #[default]
    Summary,
    /// Every action with its time, the most recent `MAX_ENTRIES`.
    All,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionFilter {
    /// Only actions of these objects.
    #[serde(default)]
    pub objects: Vec<Id>,
    /// Only these operations (e.g. `action.scene`).
    #[serde(default)]
    pub operations: Vec<String>,
    /// Operations left out. An event operation also leaves out the actions that only it
    /// starts, except scene changes (`action.scene`). Absent: `event.step` (per-step logic and
    /// timers), unless `operations` is given.
    #[serde(default)]
    pub exclude_operations: Option<Vec<String>>,
}

/// What a sample of a watched object reports.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Position,
    Rotation,
    Scale,
    Visible,
    Attributes,
    Character,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum AttributeFilter {
    All(bool),
    Names(Vec<String>),
}

/// An object to report: its ID, or `{id, fields?, attributes?}`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(try_from = "Json")]
pub struct Watch {
    pub id: Id,
    /// Position, visible and attributes when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<Field>>,
    /// `false` leaves the attributes out; a list keeps only those names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<AttributeFilter>,
}
impl From<Id> for Watch {
    fn from(id: Id) -> Self {
        Self {
            id,
            fields: None,
            attributes: None,
        }
    }
}
impl TryFrom<Json> for Watch {
    type Error = String;
    fn try_from(value: Json) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Detailed {
            id: Id,
            #[serde(default)]
            fields: Option<Vec<Field>>,
            #[serde(default)]
            attributes: Option<AttributeFilter>,
        }
        match value {
            Json::String(id) => Ok(id.into()),
            other => serde_json::from_value::<Detailed>(other)
                .map(|d| Self {
                    id: d.id,
                    fields: d.fields,
                    attributes: d.attributes,
                })
                .map_err(|e| e.to_string()),
        }
    }
}

/// Places an object before the run, in world space.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spawn {
    pub entity: Id,
    pub position: [f32; 3],
    /// Turn around the vertical axis, degrees; the rotation is kept when absent.
    #[serde(default)]
    pub yaw: Option<f32>,
}

/// Stops the run at the first moment any condition holds; `seconds` stays the upper bound.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Until {
    /// `true`: any scene start, reloads included; a scene's ID or name: that scene starting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_change: Option<SceneCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute: Option<AttributeCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable: Option<VariableCondition>,
    /// The object no longer exists in the running scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_absent: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventCondition>,
    /// `true`: the game pause starts; `false`: it ends (a scene change ending it included).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    /// A sound, by the audio's ID or name, starts playing: once or in a loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<String>,
    /// The request's bot is done with its path, stuck, or either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot: Option<BotEnd>,
}
/// How a bot ends: at the end of its path or stuck; `Any` stands for either in `until`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BotEnd {
    Done,
    Stuck,
    Any,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum SceneCondition {
    Any(bool),
    Scene(Id),
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttributeCondition {
    pub entity: Id,
    pub name: String,
    pub op: Comparison,
    /// A plain number, boolean or text, or a tagged value such as `{"Number": 3}`, of the
    /// attribute's type. For an object or surface attribute, a text is the ID (`"$N"` too).
    #[serde(deserialize_with = "plain_or_tagged")]
    pub value: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VariableCondition {
    /// The game variable's ID or name (`"$N"` too).
    pub variable: String,
    /// The key whose value is compared; empty or absent, the variable's own value.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    pub op: Comparison,
    /// A plain number, boolean or text, or a tagged value such as `{"Vector3": [0, 1, 0]}`, of
    /// the variable's type.
    #[serde(deserialize_with = "plain_or_tagged")]
    pub value: Value,
}
fn plain_or_tagged<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    match Json::deserialize(deserializer)? {
        Json::Number(n) => n
            .as_f64()
            .map(Value::Number)
            .ok_or_else(|| serde::de::Error::custom("número inválido")),
        Json::Bool(b) => Ok(Value::Bool(b)),
        Json::String(s) => Ok(Value::Text(s)),
        other => serde_json::from_value(other).map_err(serde::de::Error::custom),
    }
}
fn plain_or_tagged_values<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<String, Value>>, D::Error> {
    let Some(values) = Option::<BTreeMap<String, Json>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    values
        .into_iter()
        .map(|(label, value)| {
            let value = plain_or_tagged(value)
                .map_err(|error| serde::de::Error::custom(format!("{label}: {error}")))?;
            Ok((label, value))
        })
        .collect::<Result<_, D::Error>>()
        .map(Some)
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Comparison {
    #[serde(rename = "==")]
    Equal,
    #[serde(rename = "!=")]
    NotEqual,
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    LessOrEqual,
    #[serde(rename = ">")]
    Greater,
    #[serde(rename = ">=")]
    GreaterOrEqual,
}
impl Comparison {
    fn symbol(self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessOrEqual => "<=",
            Self::Greater => ">",
            Self::GreaterOrEqual => ">=",
        }
    }
    fn ordered(self) -> bool {
        !matches!(self, Self::Equal | Self::NotEqual)
    }
    fn holds(self, a: &Value, b: &Value) -> bool {
        match self {
            Self::Equal => a == b,
            Self::NotEqual => a != b,
            _ => {
                let (Some(a), Some(b)) = (a.number(), b.number()) else {
                    return false;
                };
                match self {
                    Self::Less => a < b,
                    Self::LessOrEqual => a <= b,
                    Self::Greater => a > b,
                    _ => a >= b,
                }
            }
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EventCondition {
    pub kind: EventKind,
    /// Only events involving this object, on either side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<Id>,
}

/// Movement and area events, named as in the logic node "Evento de personagem".
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Jump,
    Land,
    Leave,
    Side,
    Surface,
    Posture,
    AreaEnter,
    AreaExit,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SimulationRequest {
    /// Scene to start in, by its ID or name; the project's start scene when absent.
    #[serde(default)]
    pub scene: Option<Id>,
    /// Simulated seconds; the upper bound when `until` is set.
    pub seconds: f64,
    #[serde(default)]
    pub inputs: Vec<InputEvent>,
    /// Objects reported in every sample.
    #[serde(default)]
    pub watch: Vec<Watch>,
    /// Evenly spaced samples of the watched objects, exactly this many, the last one being the
    /// final state (0 or 1 report only the final state; at most 120, and at most one per step).
    /// They are spaced across `seconds`, so a run that `until` stops early reports fewer.
    #[serde(default)]
    pub samples: u32,
    /// Commands applied to the copy before the run, as in a batch; commands without a scene
    /// use the start scene. IDs they create are `"$N"` (or `"$N.M"`) in watch, inputs, spawn,
    /// bot and until. update_variable here gives a game variable another initial value.
    #[serde(default)]
    pub setup: Vec<Command>,
    #[serde(default)]
    pub spawn: Option<Spawn>,
    #[serde(default)]
    pub until: Option<Until>,
    #[serde(default)]
    pub actions: ActionsMode,
    #[serde(default)]
    pub actions_filter: ActionFilter,
    /// Size in pixels of the game view that lays out `ui` (16 to 4096 a side); `VIEW_SIZE`
    /// when absent.
    #[serde(default)]
    pub view_size: Option<[u32; 2]>,
    /// Reports the game interface at the end; on unless `false`.
    #[serde(default = "on")]
    pub ui: bool,
    /// What Sortear número draws from, 0 to [`random::MAX_SEED`]: the same seed and inputs
    /// repeat the draws.
    #[serde(default)]
    pub seed: u64,
    /// Runs a second copy of the game alongside, with the same inputs, and compares the two
    /// states after every step: the report's `replay` says whether they stayed the same.
    #[serde(default)]
    pub check_replay: bool,
    /// The progress a returning player has, which starts the persistent game variables: each
    /// one by ID or name (`"$N"` too), `"name · key"` for the value of a key, with a plain or
    /// tagged value of its type. Absent, the game runs as on a fresh install; the report's
    /// `save` has this shape, so a run can start where another left off.
    #[serde(default, deserialize_with = "plain_or_tagged_values")]
    pub save: Option<BTreeMap<String, Value>>,
    /// Plays one 3D character of the start scene along a path of points, in place of the
    /// movement inputs; its IDs may be `"$N"` from setup. The report's `bot` says how it went.
    #[serde(default)]
    pub bot: Option<Bot>,
}
impl SimulationRequest {
    /// Reads a `save` given apart from the rest of a request, as the MCP tools take it.
    pub fn read_save(value: Json) -> Result<BTreeMap<String, Value>, serde_json::Error> {
        plain_or_tagged_values(value).map(Option::unwrap_or_default)
    }
}
fn on() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Named {
    pub id: Id,
    pub name: String,
}
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Sample {
    pub time: f64,
    pub present: bool,
    /// World position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    /// World rotation, XYZ Euler degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 3]>,
    /// World scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<BTreeMap<String, Value>>,
    /// Only for 3D characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character: Option<CharacterSample>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CharacterSample {
    /// Total velocity, a moving support's included.
    pub velocity: [f32; 3],
    /// Horizontal speed.
    pub speed: f32,
    pub grounded: bool,
    /// standing, crouched or sliding.
    pub posture: &'static str,
    pub sliding: bool,
    pub sprinting: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<SupportSample>,
    /// Body facing, camera turn and camera pitch, degrees.
    pub yaw: f32,
    pub look_yaw: f32,
    pub pitch: f32,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SupportSample {
    pub id: Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<Named>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Watched {
    pub id: Id,
    pub name: String,
    pub samples: Vec<Sample>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Event {
    pub time: f64,
    pub text: String,
}
/// A sound played once (no `kind`), or a change logic made to what plays.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SoundEvent {
    pub time: f64,
    /// The audio's name; for a stop of every loop, "Todos os sons do canal"; for a volume, the
    /// channel's label.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ChangeKind>,
    /// A loop that starts (or starts over), not one already playing that only takes another
    /// volume, as a restarted scene asking for its music at another volume.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub started: bool,
    /// music or effects; all for a stop of both, master for the general volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<&'static str>,
    /// What a loop or a volume was set to; a one-shot's when below 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    /// Seconds a loop fades in, or loops stopped fade out, when not at once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade: Option<f64>,
}
/// A sound the game keeps playing at the end.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PlayingSound {
    pub asset: Id,
    pub name: String,
    pub volume: f64,
}
/// What the game keeps playing at the end, and the run's volumes.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AudioReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music: Option<PlayingSound>,
    /// The effect loops.
    pub loops: Vec<PlayingSound>,
    pub volumes: AudioSettings,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SceneChange {
    pub time: f64,
    pub scene: Id,
    pub name: String,
    /// The scene that was already running started again.
    pub reload: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Log {
    pub time: f64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Named>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeRef>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NodeRef {
    pub id: Id,
    pub operation: String,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ActionSummary {
    /// "Object · Action".
    pub text: String,
    pub count: usize,
    pub first: f64,
    pub last: f64,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Actions {
    Summary(Vec<ActionSummary>),
    All(Vec<Event>),
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GameEvent {
    pub time: f64,
    pub kind: EventKind,
    /// The character, or the object that entered or left an area.
    pub object: Named,
    /// The area, the wall touched or the support landed on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<Named>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posture: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<Named>,
    /// Impact speed of a landing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CameraReport {
    pub active: Named,
    /// first_person, third_person or fixed.
    pub mode: &'static str,
    pub position: [f32; 3],
    pub forward: [f32; 3],
    /// Vertical field of view, degrees.
    pub fov: f32,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StoppedAt {
    pub time: f64,
    /// The `until` condition that held.
    pub reason: &'static str,
    /// The event that met an `event` condition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<EventKind>,
    /// What happened, in words.
    pub detail: String,
}
/// Two copies of the game run side by side on the same inputs, compared after every step.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplayCheck {
    /// The two stayed the same to the end.
    pub identical: bool,
    /// Steps compared: all of the run's, or up to the one where the two first differed. The
    /// start, before the first step, is compared too.
    pub steps: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_difference: Option<ReplayDifference>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplayDifference {
    pub time: f64,
    /// The object that differed; none for the run's own state (scene, time, input, tasks,
    /// areas, characters, random, variables.<name>, save, audio, camera).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Named>,
    /// What differed, a part of [`crate::runtime::StateDigest`]: transform, attributes.<name>,
    /// document, character… for an object; exists when only one copy has it, order when both
    /// have it in another scene order.
    pub part: String,
}

/// A request of Salvar progresso (write, with the values saved) or Apagar progresso (clear).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SaveEvent {
    pub time: f64,
    pub kind: SaveKind,
    /// What a write saved, as in `variables`: up to `MAX_ENTRIES`, the variables' own values
    /// first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<BTreeMap<String, Value>>,
    /// Values of a write left out of `values`.
    #[serde(skip_serializing_if = "is_zero")]
    pub values_truncated: usize,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaveKind {
    Write,
    Clear,
}
fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// A change of the game pause: `paused` is the new state.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PauseEvent {
    pub time: f64,
    pub paused: bool,
}

/// How the request's bot went.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BotReport {
    /// The character it played.
    pub entity: Named,
    /// The points it reached, in order.
    pub reached: Vec<BotPoint>,
    /// When it left the last point, its wait and jump there done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_at: Option<f64>,
    /// Where it gave up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stuck: Option<BotStuck>,
}
/// A point the bot reached: its index in the path, from 0, and where the character stood.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BotPoint {
    pub index: usize,
    pub time: f64,
    pub position: [f32; 3],
}
/// The point the bot came no nearer to, where the character stood and how far from it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BotStuck {
    pub index: usize,
    pub time: f64,
    pub position: [f32; 3],
    pub distance: f32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SimulationReport {
    pub seconds: f64,
    pub start_scene: Id,
    pub final_scene: Id,
    /// The seed the draws came from, the one asked for.
    pub seed: u64,
    /// [`Runtime::state_hash`] at the end, 16 hex digits: runs that ended in the same state
    /// have the same one. The seed is part of the state, draws or not.
    pub state_hash: String,
    /// The replay check, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replay: Option<ReplayCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<StoppedAt>,
    /// Whether the game was in its own pause at the end.
    pub paused: bool,
    /// Each change of the game pause, oldest first (the most recent `MAX_ENTRIES`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pauses: Vec<PauseEvent>,
    /// What each setup command did, with the IDs it created.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub setup: Vec<Applied>,
    pub scene_changes: Vec<SceneChange>,
    pub logs: Vec<Log>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Actions>,
    /// Sounds played once and changes of what plays, oldest first (the most recent
    /// `MAX_ENTRIES`). The one-shots and effect loops of a scene that started and ended within
    /// a step are left out, as the game never plays them, and so are the loops a step that
    /// changed scene started and no longer plays at its end (see `Recording::audio`).
    pub sounds: Vec<SoundEvent>,
    pub events: Vec<GameEvent>,
    /// The bot, when the request has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot: Option<BotReport>,
    /// The game camera at the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraReport>,
    /// The game interface at the end, in drawing order (the topmost last).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ui: Vec<UiState>,
    pub watched: Vec<Watched>,
    /// The game variables at the end, by name, or "name · key" for the value of a key: the
    /// first `MAX_ENTRIES` of [`Runtime::variable_values`].
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Value>,
    /// What Salvar progresso and Apagar progresso asked, oldest first (the most recent
    /// `MAX_ENTRIES`). The run never touches the disk.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub saves: Vec<SaveEvent>,
    /// The saved progress at the end, in the shape [`SimulationRequest::save`] takes (up to
    /// `MAX_ENTRIES` values); absent when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save: Option<BTreeMap<String, Value>>,
    /// What plays at the end and the volumes; absent when nothing loops and every volume is 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioReport>,
    /// Entries dropped per list because it reached `MAX_ENTRIES`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub truncated: BTreeMap<&'static str, usize>,
}

/// Simulation time rounded to 0.1 ms, which keeps reports short.
fn rounded(time: f64) -> f64 {
    (time * 10_000.).round() / 10_000.
}

/// `"$N"` stands for the first ID created by setup command N, `"$N.M"` for the M-th,
/// `"$N@<id>"` for the copy of `<id>` it made, and `"$$N"` for the text "$N" (see
/// [`command::Placeholder`]); other text is an ID as it is.
pub fn resolve(id: &str, applied: &[Applied]) -> Result<Id, String> {
    match command::Placeholder::parse(id) {
        None => Ok(id.into()),
        Some(placeholder) => placeholder.resolve(applied).ok_or_else(|| {
            format!("\"{id}\" não se refere a um ID criado pela preparação (setup)")
        }),
    }
}

/// Finds objects by ID: in the running scene, in another scene of the run, or in the copy the
/// run started from (for objects removed since). Copies spawned during the run and already
/// removed keep only their name.
#[derive(Clone, Copy)]
struct Lookup<'a> {
    runtime: &'a Runtime,
    source: &'a Project,
    spawned: &'a HashMap<Id, String>,
}
impl<'a> Lookup<'a> {
    fn entity(self, id: &str) -> Option<&'a Entity> {
        self.runtime
            .scene()
            .entity(id)
            .or_else(|| {
                self.runtime
                    .project()
                    .scenes
                    .iter()
                    .find_map(|s| s.entity(id))
            })
            .or_else(|| self.source.scenes.iter().find_map(|s| s.entity(id)))
    }
    /// Names do not change during a run, so a copy's is found without a scan.
    fn name(self, id: &str) -> Option<&'a str> {
        self.spawned
            .get(id)
            .map(String::as_str)
            .or_else(|| self.entity(id).map(|e| e.name.as_str()))
    }
    fn named(self, id: &str) -> Named {
        Named {
            id: id.into(),
            name: self.name(id).unwrap_or(id).into(),
        }
    }
}
fn surface(project: &Project, id: &str) -> Named {
    Named {
        id: id.into(),
        name: project
            .surfaces
            .iter()
            .find(|s| s.id == id)
            .map_or_else(|| id.into(), |s| s.name.clone()),
    }
}
fn posture_name(posture: Posture) -> &'static str {
    match posture {
        Posture::Standing => "standing",
        Posture::Crouched => "crouched",
        Posture::Sliding => "sliding",
    }
}
fn value_text(value: &Value) -> String {
    match value {
        Value::Number(n) => n.to_string(),
        Value::Text(t) => format!("\"{t}\""),
        Value::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

/// Splits the runtime's log lines that name an object and a logic node: "[object / node]
/// message" from a diagnostic message and "Objeto object / nó node: error" from a failure.
/// The editor's console feed for agents splits them the same way.
pub fn node_line(line: &str) -> Option<(&str, &str, &str)> {
    if let Some(rest) = line.strip_prefix('[') {
        let (head, message) = rest.split_once("] ")?;
        let (object, node) = head.split_once(" / ")?;
        return Some((object, node, message));
    }
    let (object, rest) = line.strip_prefix("Objeto ")?.split_once(" / nó ")?;
    let (node, error) = rest.split_once(": ")?;
    Some((object, node, error))
}
fn is_id(bytes: &[u8]) -> bool {
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// A runtime log line as an entry with names: object and node IDs become fields, and IDs of
/// objects and scenes left in the text become their names. `traces` are the step's own: they
/// name the node of a spawned copy that is already gone.
fn log_entry(lookup: Lookup, traces: &[NodeTrace], time: f64, line: &str) -> Log {
    let mut entry = Log {
        time: rounded(time),
        text: line.into(),
        object: None,
        node: None,
    };
    if let Some((object, node, message)) = node_line(line)
        && let Some(name) = lookup.name(object)
    {
        entry.object = Some(Named {
            id: object.into(),
            name: name.into(),
        });
        entry.node = Some(NodeRef {
            id: node.into(),
            operation: lookup
                .entity(object)
                .and_then(|e| e.graph.nodes.iter().find(|n| n.id == node))
                .map(|n| n.operation.clone())
                .or_else(|| {
                    traces
                        .iter()
                        .rev()
                        .find(|t| t.node == node)
                        .map(|t| t.operation.clone())
                })
                .unwrap_or_default(),
        });
        entry.text = message.into();
    }
    let text = std::mem::take(&mut entry.text);
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i + 36 <= bytes.len() {
        if (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) && is_id(&bytes[i..i + 36]) {
            let id = &text[i..i + 36];
            let name = if let Some(name) = lookup.name(id) {
                if entry.object.is_none() {
                    entry.object = Some(Named {
                        id: id.into(),
                        name: name.into(),
                    });
                }
                Some(name)
            } else {
                lookup.runtime.project().scene(id).map(|s| s.name.as_str())
            };
            if let Some(name) = name {
                entry.text.push_str(&text[start..i]);
                entry.text.push_str(name);
                start = i + 36;
            }
            i += 36;
        } else {
            i += 1;
        }
    }
    entry.text.push_str(&text[start..]);
    entry
}

/// Whether running a node of `operation` opens another scene: an action that names the scene
/// to open, as Mudar de cena does.
fn changes_scene(operation: &str) -> bool {
    graph::definition(operation).is_some_and(|def| {
        def.inputs.iter().any(|port| port.kind == PortType::Exec)
            && def
                .params
                .iter()
                .any(|param| graph::reference(def.id, param) == Some(graph::Reference::Scene))
    })
}

/// Nodes of `graph` that run only because of the given event operations: those events and
/// every node their execution flow reaches that no other event reaches.
fn reached_only_from(graph: &Graph, events: &[String]) -> HashSet<Id> {
    let registry = graph::registry();
    let flow: Vec<&Edge> = graph
        .edges
        .iter()
        .filter(|edge| {
            graph
                .nodes
                .iter()
                .find(|n| n.id == edge.from_node)
                .and_then(|n| registry.iter().find(|op| op.id == n.operation))
                .and_then(|op| op.outputs.iter().find(|port| port.id == edge.from_port))
                .is_some_and(|port| port.kind == PortType::Exec)
        })
        .collect();
    let reach = |listed: bool| {
        let mut seen: HashSet<Id> = graph
            .nodes
            .iter()
            .filter(|n| {
                n.operation.starts_with("event.") && events.contains(&n.operation) == listed
            })
            .map(|n| n.id.clone())
            .collect();
        let mut queue: Vec<Id> = seen.iter().cloned().collect();
        while let Some(id) = queue.pop() {
            for edge in flow.iter().filter(|e| e.from_node == id) {
                if seen.insert(edge.to_node.clone()) {
                    queue.push(edge.to_node.clone());
                }
            }
        }
        seen
    };
    let only = reach(true);
    let others = reach(false);
    only.difference(&others).cloned().collect()
}

/// Keeps the most recent `MAX_ENTRIES` items and counts the ones dropped.
struct Recent<T> {
    items: VecDeque<T>,
    dropped: usize,
}
impl<T> Default for Recent<T> {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            dropped: 0,
        }
    }
}
impl<T> Recent<T> {
    fn push(&mut self, item: T) {
        if self.items.len() == MAX_ENTRIES {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(item);
    }
    fn finish(self, name: &'static str, truncated: &mut BTreeMap<&'static str, usize>) -> Vec<T> {
        if self.dropped > 0 {
            truncated.insert(name, self.dropped);
        }
        self.items.into()
    }
}

/// What one step (or the start) produced, for the stop conditions.
#[derive(Default)]
struct Happened {
    scenes: Vec<Id>,
    logs: Vec<String>,
    events: Vec<GameEvent>,
    /// The game pause's changes, each its new state.
    pauses: Vec<bool>,
    /// The audios that started playing, once or in a loop.
    sounds: Vec<Id>,
    /// How the bot ended in the step (done or stuck), in words.
    bot: Option<(BotEnd, String)>,
}

/// "Object · Action", with the action's label.
fn action_text(lookup: Lookup, object: &str, operation: &str) -> String {
    let action = graph::registry()
        .iter()
        .find(|op| op.id == operation)
        .map_or(operation, |op| op.label);
    format!("{} · {action}", lookup.named(object).name)
}

/// The action summary, one line per visible text, so objects that share a name (spawned copies
/// among them) read as one line. It keeps at most `MAX_ENTRIES` lines: a new line takes the
/// place of the one idle the longest, which counts as dropped.
#[derive(Default)]
struct Summary {
    /// Each line with the order it first appeared in.
    lines: Vec<(u64, ActionSummary)>,
    by_text: HashMap<String, usize>,
    /// The line text of each (object, operation) already seen.
    texts: HashMap<(Id, String), String>,
    started: u64,
    dropped: usize,
}
impl Summary {
    fn count(
        &mut self,
        object: Id,
        operation: String,
        time: f64,
        text: impl FnOnce(&str, &str) -> String,
    ) {
        let text = self
            .texts
            .entry((object, operation))
            .or_insert_with_key(|(object, operation)| text(object, operation));
        if let Some(&i) = self.by_text.get(text.as_str()) {
            let line = &mut self.lines[i].1;
            line.count += 1;
            line.last = time;
            return;
        }
        self.started += 1;
        let line = (
            self.started,
            ActionSummary {
                text: text.clone(),
                count: 1,
                first: time,
                last: time,
            },
        );
        let i = if self.lines.len() < MAX_ENTRIES {
            self.lines.push(line);
            self.lines.len() - 1
        } else {
            let i = (0..self.lines.len())
                .min_by(|&a, &b| self.lines[a].1.last.total_cmp(&self.lines[b].1.last))
                .unwrap_or_default();
            self.by_text.remove(&self.lines[i].1.text);
            self.lines[i] = line;
            self.dropped += 1;
            i
        };
        self.by_text.insert(text.clone(), i);
    }
    /// The lines in the order they first appeared, and how many were dropped.
    fn finish(mut self) -> (Vec<ActionSummary>, usize) {
        self.lines.sort_by_key(|(order, _)| *order);
        (
            self.lines.into_iter().map(|(_, line)| line).collect(),
            self.dropped,
        )
    }
}

/// Collects what the runtime did, step by step, into the report lists.
struct Recorder<'a> {
    source: &'a Project,
    mode: ActionsMode,
    objects: HashSet<Id>,
    operations: HashSet<String>,
    exclude: Vec<String>,
    exclude_events: Vec<String>,
    excluded_nodes: HashMap<Id, HashSet<Id>>,
    /// Names of the copies spawned during the run, which may be gone when they are reported.
    spawned: HashMap<Id, String>,
    summary: Summary,
    actions: Recent<Event>,
    scene: Id,
    scene_changes: Recent<SceneChange>,
    logs: Recent<Log>,
    sounds: Recent<SoundEvent>,
    events: Recent<GameEvent>,
    saves: Recent<SaveEvent>,
    pauses: Recent<PauseEvent>,
}
impl Recorder<'_> {
    fn lookup<'b>(&'b self, runtime: &'b Runtime) -> Lookup<'b> {
        Lookup {
            runtime,
            source: self.source,
            spawned: &self.spawned,
        }
    }

    fn collect(&mut self, runtime: &mut Runtime) -> Happened {
        let time = runtime.time;
        let mut happened = Happened::default();
        let recording = runtime
            .recording
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        let scenes = std::mem::take(&mut runtime.scene_starts);
        let logs = std::mem::take(&mut runtime.logs);
        let traces = std::mem::take(&mut runtime.traces);
        let sounds = std::mem::take(&mut runtime.sounds);
        let saves = std::mem::take(&mut runtime.save_requests);
        let runtime = &*runtime;
        let Recording {
            crossings,
            movements,
            spawned,
            pauses,
            audio,
        } = recording;
        for &paused in &pauses {
            self.pauses.push(PauseEvent {
                time: rounded(time),
                paused,
            });
        }
        happened.pauses = pauses;
        for object in spawned {
            // A copy's nodes have IDs of their own, so its exclusions come from its own graph.
            if self.mode != ActionsMode::None
                && !self.exclude_events.is_empty()
                && !object.graph.nodes.is_empty()
            {
                self.excluded_nodes.insert(
                    object.id.clone(),
                    reached_only_from(&object.graph, &self.exclude_events),
                );
            }
            self.spawned.insert(object.id, object.name);
        }
        for scene in scenes {
            self.scene_changes.push(SceneChange {
                time: rounded(time),
                name: runtime
                    .project()
                    .scene(&scene)
                    .map_or_else(|| scene.clone(), |s| s.name.clone()),
                reload: scene == self.scene,
                scene: scene.clone(),
            });
            self.scene = scene.clone();
            happened.scenes.push(scene);
        }
        for line in logs {
            let entry = log_entry(self.lookup(runtime), &traces, time, &line);
            happened.logs.push(entry.text.clone());
            happened.logs.push(line);
            self.logs.push(entry);
        }
        for trace in traces {
            self.action(runtime, trace);
        }
        let name = |asset: &str| {
            runtime
                .project()
                .asset(asset)
                .map_or_else(|| asset.to_owned(), |a| a.name.clone())
        };
        for sound in sounds {
            self.sounds.push(SoundEvent {
                time: rounded(time),
                text: name(&sound.asset),
                kind: None,
                started: false,
                channel: None,
                volume: (sound.volume < 1.).then_some(f64::from(sound.volume)),
                fade: None,
            });
            happened.sounds.push(sound.asset);
        }
        for change in audio {
            self.sounds.push(sound_change(rounded(time), &change, name));
            if change.started
                && let Some(asset) = change.asset
            {
                happened.sounds.push(asset);
            }
        }
        for request in saves {
            let (kind, values) = match &request {
                SaveRequest::Write(values) => (SaveKind::Write, Some(values)),
                SaveRequest::Clear => (SaveKind::Clear, None),
            };
            let values = values.map(|values| variables::labeled(self.source, values, MAX_ENTRIES));
            self.saves.push(SaveEvent {
                time: rounded(time),
                kind,
                values_truncated: values.as_ref().map_or(0, |(_, left_out)| *left_out),
                values: values.map(|(values, _)| values),
            });
        }
        let lookup = Lookup {
            runtime,
            source: self.source,
            spawned: &self.spawned,
        };
        let mut events = Vec::new();
        let event = |kind, object: &str, other: Option<&str>| GameEvent {
            time: rounded(time),
            kind,
            object: lookup.named(object),
            other: other.map(|id| lookup.named(id)),
            posture: None,
            surface: None,
            speed: None,
        };
        for record in &movements {
            let id = &record.object;
            events.push(match &record.event {
                MovementEvent::Jumped => event(EventKind::Jump, id, None),
                MovementEvent::Landed { impact_speed } => GameEvent {
                    speed: Some(*impact_speed),
                    ..event(
                        EventKind::Land,
                        id,
                        record.state.support.as_ref().map(|s| s.object.as_str()),
                    )
                },
                MovementEvent::LeftSupport => event(EventKind::Leave, id, None),
                MovementEvent::SurfaceChanged { current, .. } => GameEvent {
                    surface: current.as_deref().map(|s| surface(runtime.project(), s)),
                    ..event(EventKind::Surface, id, None)
                },
                MovementEvent::PostureChanged(posture) => GameEvent {
                    posture: Some(posture_name(*posture)),
                    ..event(EventKind::Posture, id, None)
                },
                MovementEvent::SideContact { object, .. } => {
                    event(EventKind::Side, id, Some(object.as_str()))
                }
            });
        }
        for crossing in &crossings {
            let kind = if crossing.entered {
                EventKind::AreaEnter
            } else {
                EventKind::AreaExit
            };
            events.push(event(kind, &crossing.other, Some(crossing.area.as_str())));
        }
        // A trigger seen both as a 2D area and as a 3D sensor reads as one event.
        let mut seen = HashSet::new();
        events.retain(|e| {
            seen.insert((
                e.kind,
                e.object.id.clone(),
                e.other.as_ref().map(|o| o.id.clone()),
            ))
        });
        for event in &events {
            self.events.push(event.clone());
        }
        happened.events = events;
        happened
    }

    fn action(&mut self, runtime: &Runtime, trace: NodeTrace) {
        if self.mode == ActionsMode::None
            || (!self.objects.is_empty() && !self.objects.contains(&trace.object))
            || (!self.operations.is_empty() && !self.operations.contains(&trace.operation))
            || self.exclude.contains(&trace.operation)
        {
            return;
        }
        // A scene change always shows, even from a chain an excluded event starts (a reset
        // on falling, checked every step), so each scene start has an action that explains it.
        if !self.exclude_events.is_empty() && !changes_scene(&trace.operation) {
            if !self.excluded_nodes.contains_key(&trace.object) {
                let nodes = self
                    .lookup(runtime)
                    .entity(&trace.object)
                    .map(|e| reached_only_from(&e.graph, &self.exclude_events))
                    .unwrap_or_default();
                self.excluded_nodes.insert(trace.object.clone(), nodes);
            }
            if self.excluded_nodes[&trace.object].contains(&trace.node) {
                return;
            }
        }
        let time = rounded(trace.time);
        let lookup = Lookup {
            runtime,
            source: self.source,
            spawned: &self.spawned,
        };
        if self.mode == ActionsMode::All {
            self.actions.push(Event {
                time,
                text: action_text(lookup, &trace.object, &trace.operation),
            });
            return;
        }
        self.summary
            .count(trace.object, trace.operation, time, |object, operation| {
                action_text(lookup, object, operation)
            });
    }

    fn finish(self, report: &mut SimulationReport) {
        let truncated = &mut report.truncated;
        report.scene_changes = self.scene_changes.finish("scene_changes", truncated);
        report.logs = self.logs.finish("logs", truncated);
        report.sounds = self.sounds.finish("sounds", truncated);
        report.events = self.events.finish("events", truncated);
        report.saves = self.saves.finish("saves", truncated);
        report.pauses = self.pauses.finish("pauses", truncated);
        report.actions = match self.mode {
            ActionsMode::None => None,
            ActionsMode::All => Some(Actions::All(self.actions.finish("actions", truncated))),
            ActionsMode::Summary => {
                let (lines, dropped) = self.summary.finish();
                if dropped > 0 {
                    truncated.insert("actions", dropped);
                }
                Some(Actions::Summary(lines))
            }
        };
    }
}

/// A change of what plays as the report lists it.
fn sound_change(time: f64, change: &AudioChange, name: impl Fn(&str) -> String) -> SoundEvent {
    let text = match (change.kind, &change.asset) {
        (_, Some(asset)) => name(asset),
        (ChangeKind::Volume, None) => graph::VOLUME_CHANNELS
            .iter()
            .find(|(id, _)| *id == change.channel)
            .map_or(change.channel, |(_, label)| label)
            .to_owned(),
        _ => graph::empty_choice("action.sound_stop", "asset")
            .unwrap_or_default()
            .to_owned(),
    };
    SoundEvent {
        time,
        text,
        kind: Some(change.kind),
        started: change.started,
        channel: Some(change.channel),
        volume: (change.kind != ChangeKind::Stop).then_some(change.volume),
        fade: (change.fade > 0.).then_some(change.fade),
    }
}

/// What plays at the end, by name; none when nothing loops and every volume is 1.
fn audio_report(runtime: &Runtime) -> Option<AudioReport> {
    let audio = runtime.audio();
    let playing = |voice: &crate::audio::LoopVoice| PlayingSound {
        asset: voice.asset.clone(),
        name: runtime
            .project()
            .asset(&voice.asset)
            .map_or_else(|| voice.asset.clone(), |a| a.name.clone()),
        volume: voice.volume,
    };
    let report = AudioReport {
        music: audio.music.as_ref().map(playing),
        loops: audio.loops.values().map(playing).collect(),
        volumes: audio.settings,
    };
    let quiet = report.music.is_none()
        && report.loops.is_empty()
        && report.volumes == AudioSettings::default();
    (!quiet).then_some(report)
}

struct WatchPlan {
    id: Id,
    fields: BTreeSet<Field>,
    /// None: every attribute.
    attributes: Option<BTreeSet<String>>,
}

fn sample(lookup: Lookup, watch: &WatchPlan) -> Sample {
    let runtime = lookup.runtime;
    let time = rounded(runtime.time);
    let scene = runtime.scene();
    let Some(entity) = scene.entity(&watch.id) else {
        return Sample {
            time,
            ..Default::default()
        };
    };
    let has = |field| watch.fields.contains(&field);
    let world = scene
        .world_matrix(&watch.id)
        .ok()
        .map(|m| m.to_scale_rotation_translation());
    Sample {
        time,
        present: true,
        position: world
            .filter(|_| has(Field::Position))
            .map(|(_, _, position)| position.to_array()),
        rotation: world
            .filter(|_| has(Field::Rotation))
            // The same reading as oxy_scene and oxy_entity: nearest to the local angles, so a
            // character yawed past 90 degrees does not read as upside down.
            .map(|(_, rotation, _)| {
                crate::query::world_degrees(rotation, entity.transform.rotation)
            }),
        scale: world
            .filter(|_| has(Field::Scale))
            .map(|(scale, _, _)| scale.to_array()),
        visible: has(Field::Visible).then_some(entity.visible),
        attributes: has(Field::Attributes).then(|| {
            entity
                .attributes
                .iter()
                .filter(|(name, _)| watch.attributes.as_ref().is_none_or(|n| n.contains(*name)))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        }),
        character: has(Field::Character)
            .then(|| runtime.character_state(&watch.id))
            .flatten()
            .map(|state| character(lookup, state)),
    }
}
fn character(lookup: Lookup, state: &CharacterState) -> CharacterSample {
    let velocity = state.total_velocity();
    CharacterSample {
        velocity: velocity.to_array(),
        speed: Vec3::new(velocity.x, 0., velocity.z).length(),
        grounded: state.grounded,
        posture: posture_name(state.posture),
        sliding: state.posture == Posture::Sliding,
        sprinting: state.sprinting,
        support: state.support.as_ref().map(|support| {
            let object = lookup.named(&support.object);
            SupportSample {
                id: object.id,
                name: object.name,
                surface: support
                    .surface
                    .as_deref()
                    .map(|id| surface(lookup.runtime.project(), id)),
            }
        }),
        yaw: state.yaw.to_degrees(),
        look_yaw: state.look_yaw.to_degrees(),
        pitch: state.pitch.to_degrees(),
    }
}
fn camera(runtime: &Runtime) -> Option<CameraReport> {
    let id = runtime.active_camera()?.to_owned();
    let entity = runtime.scene().entity(&id)?;
    let (position, rotation, fov) = match runtime.game_camera_pose() {
        Some(pose) => (pose.position, pose.rotation, pose.fov),
        None => {
            let (_, rotation, position) = runtime
                .scene()
                .world_matrix(&id)
                .ok()?
                .to_scale_rotation_translation();
            (position, rotation, entity.camera.as_ref()?.fov)
        }
    };
    Some(CameraReport {
        mode: match runtime.camera_mode(&id)? {
            CameraMode::FirstPerson => "first_person",
            CameraMode::ThirdPerson => "third_person",
            CameraMode::Fixed => "fixed",
        },
        active: Named {
            name: entity.name.clone(),
            id,
        },
        position: position.to_array(),
        forward: (rotation * Vec3::NEG_Z).to_array(),
        fov,
    })
}

/// Moves `spawn.entity` so its world position (and yaw, when given) is the requested one.
fn place(project: &mut Project, scene: &str, spawn: &Spawn, entity: &str) -> Result<(), String> {
    if !spawn.position.iter().all(|v| v.is_finite()) || spawn.yaw.is_some_and(|v| !v.is_finite()) {
        return Err("spawn precisa de números finitos".into());
    }
    let scene = project.scene_mut(scene).ok_or("Cena inicial não existe")?;
    if scene.entity(entity).is_none() {
        return Err(format!(
            "Objeto {entity} de spawn não existe na cena inicial"
        ));
    }
    let current = scene.world_matrix(entity)?;
    let (scale, rotation, _) = current.to_scale_rotation_translation();
    let rotation = spawn
        .yaw
        .map_or(rotation, |yaw| Quat::from_rotation_y(yaw.to_radians()));
    let target = Mat4::from_scale_rotation_translation(scale, rotation, Vec3::from(spawn.position));
    crate::editing::transform_selection(scene, &[entity.to_owned()], target * current.inverse())
}

/// `until` with IDs resolved and checked against the copy.
struct Stop {
    scene: Option<Option<Id>>,
    log: Option<String>,
    attribute: Option<(AttributeCondition, String)>,
    /// With the variable's ID, and its label in reports.
    variable: Option<(VariableCondition, String)>,
    absent: Option<(Id, String)>,
    event: Option<(EventKind, Option<Id>)>,
    paused: Option<bool>,
    /// The audio's ID and name.
    sound: Option<(Id, String)>,
    bot: Option<BotEnd>,
}
impl Stop {
    fn new(
        until: &Until,
        project: &Project,
        start: &str,
        applied: &[Applied],
        bot: Option<&Bot>,
    ) -> Result<Self, String> {
        let entity = |id: &str| {
            project
                .scenes
                .iter()
                .find_map(|s| s.entity(id))
                .ok_or_else(|| format!("until: objeto {id} não existe no projeto"))
        };
        let scene = match &until.scene_change {
            None | Some(SceneCondition::Any(false)) => None,
            Some(SceneCondition::Any(true)) => Some(None),
            Some(SceneCondition::Scene(text)) => {
                let text = resolve(text, applied)?;
                let id = project.find_scene(&text, "until: scene_change")?.id.clone();
                Some(Some(id))
            }
        };
        if until.log_contains.as_deref().is_some_and(str::is_empty) {
            return Err("until: log_contains precisa de um texto".into());
        }
        let attribute = match &until.attribute {
            None => None,
            Some(condition) => {
                let id = resolve(&condition.entity, applied)?;
                let target = entity(&id)?;
                let Some(current) = target.attributes.get(&condition.name) else {
                    return Err(format!(
                        "until: atributo {} não existe em {}",
                        condition.name, target.name
                    ));
                };
                if condition.op.ordered() && condition.value.number().is_none() {
                    return Err(format!(
                        "until: a comparação {} precisa de um número",
                        condition.op.symbol()
                    ));
                }
                // A text compared with an object or surface reference is its ID.
                let value = match (current, &condition.value) {
                    (Value::Object(_), Value::Text(id)) if id.is_empty() => Value::Object(None),
                    (Value::Object(_), Value::Text(id) | Value::Object(Some(id))) => {
                        let id = resolve(id, applied)?;
                        entity(&id)?;
                        Value::Object(Some(id))
                    }
                    (Value::Surface(_), Value::Text(id)) if id.is_empty() => Value::Surface(None),
                    (Value::Surface(_), Value::Text(id) | Value::Surface(Some(id))) => {
                        if !project.surfaces.iter().any(|s| s.id == *id) {
                            return Err(format!("until: superfície {id} não existe no projeto"));
                        }
                        Value::Surface(Some(id.clone()))
                    }
                    (_, value) => value.clone(),
                };
                if std::mem::discriminant(current) != std::mem::discriminant(&value) {
                    let kind = |value: &Value| graph::value_type(value).label().to_lowercase();
                    return Err(format!(
                        "until: o atributo {} de {} é {}, mas o valor informado é {}",
                        condition.name,
                        target.name,
                        kind(current),
                        kind(&value)
                    ));
                }
                Some((
                    AttributeCondition {
                        entity: id,
                        value,
                        ..condition.clone()
                    },
                    target.name.clone(),
                ))
            }
        };
        let variable = match &until.variable {
            None => None,
            Some(condition) => {
                let id = crate::variables::find(project, &resolve(&condition.variable, applied)?)
                    .map_err(|error| format!("until: {error}"))?;
                let declared = crate::variables::get(project, &id).expect("found above");
                if condition.op.ordered() && condition.value.number().is_none() {
                    return Err(format!(
                        "until: a comparação {} precisa de um número",
                        condition.op.symbol()
                    ));
                }
                if std::mem::discriminant(&declared.value)
                    != std::mem::discriminant(&condition.value)
                {
                    return Err(format!(
                        "until: a variável {} é {}, mas o valor informado é {}",
                        declared.name,
                        crate::variables::kind(&declared.value),
                        crate::variables::kind(&condition.value)
                    ));
                }
                Some((
                    VariableCondition {
                        variable: id,
                        ..condition.clone()
                    },
                    crate::variables::label(&declared.name, &condition.key),
                ))
            }
        };
        let absent = match &until.entity_absent {
            None => None,
            Some(id) => {
                let id = resolve(id, applied)?;
                let name = project
                    .scene(start)
                    .and_then(|s| s.entity(&id))
                    .ok_or_else(|| format!("until: objeto {id} não está na cena inicial"))?
                    .name
                    .clone();
                Some((id, name))
            }
        };
        let event = match &until.event {
            None => None,
            Some(condition) => {
                let object = match &condition.object {
                    Some(id) => {
                        let id = resolve(id, applied)?;
                        entity(&id)?;
                        Some(id)
                    }
                    None => None,
                };
                Some((condition.kind, object))
            }
        };
        let sound = match &until.sound {
            None => None,
            Some(text) => {
                let text = resolve(text, applied)?;
                let audios = || project.assets.iter().filter(|a| a.kind == AssetKind::Audio);
                let named: Vec<&Asset> = match audios().find(|a| a.id == text) {
                    Some(asset) => vec![asset],
                    None => audios()
                        .filter(|a| a.name.trim().to_lowercase() == text.trim().to_lowercase())
                        .collect(),
                };
                match named.as_slice() {
                    [asset] => Some((asset.id.clone(), asset.name.clone())),
                    [] => return Err(format!("until: o som \"{text}\" não existe no projeto")),
                    many => {
                        return Err(format!(
                            "until: há {} sons chamados \"{text}\"; use o ID: {}",
                            many.len(),
                            many.iter()
                                .map(|a| a.id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
            }
        };
        if until.bot.is_some() && bot.is_none() {
            return Err("until: bot só vale com um bot no pedido (informe bot)".into());
        }
        if scene.is_none()
            && until.log_contains.is_none()
            && attribute.is_none()
            && variable.is_none()
            && absent.is_none()
            && event.is_none()
            && until.paused.is_none()
            && sound.is_none()
            && until.bot.is_none()
        {
            return Err("until: informe ao menos uma condição".into());
        }
        Ok(Self {
            scene,
            log: until.log_contains.clone(),
            attribute,
            variable,
            absent,
            event,
            paused: until.paused,
            sound,
            bot: until.bot,
        })
    }

    /// The first condition that holds now.
    fn check(&self, runtime: &Runtime, happened: &Happened) -> Option<StoppedAt> {
        let stopped = |reason, detail| StoppedAt {
            time: rounded(runtime.time),
            reason,
            kind: None,
            detail,
        };
        if let Some(target) = &self.scene
            && let Some(scene) = happened
                .scenes
                .iter()
                .find(|s| target.as_ref().is_none_or(|t| t == *s))
        {
            let name = runtime
                .project()
                .scene(scene)
                .map_or(scene.as_str(), |s| &s.name);
            return Some(stopped("scene_change", format!("Cena {name} iniciou")));
        }
        if let Some(text) = &self.log
            && let Some(line) = happened.logs.iter().find(|l| l.contains(text.as_str()))
        {
            return Some(stopped("log_contains", line.clone()));
        }
        if let Some((condition, name)) = &self.attribute
            && let Some(value) = runtime
                .scene()
                .entity(&condition.entity)
                .and_then(|e| e.attributes.get(&condition.name))
            && condition.op.holds(value, &condition.value)
        {
            return Some(stopped(
                "attribute",
                format!(
                    "{name}.{} {} {} (valor {})",
                    condition.name,
                    condition.op.symbol(),
                    value_text(&condition.value),
                    value_text(value)
                ),
            ));
        }
        if let Some((condition, label)) = &self.variable
            && let Some(value) = runtime.variable(&condition.variable, &condition.key)
            && condition.op.holds(value, &condition.value)
        {
            return Some(stopped(
                "variable",
                format!(
                    "{label} {} {} (valor {})",
                    condition.op.symbol(),
                    value_text(&condition.value),
                    value_text(value)
                ),
            ));
        }
        if let Some((id, name)) = &self.absent
            && runtime.scene().entity(id).is_none()
        {
            return Some(stopped(
                "entity_absent",
                format!("{name} não existe mais na cena"),
            ));
        }
        if let Some((kind, object)) = &self.event
            && let Some(event) = happened.events.iter().find(|e| {
                e.kind == *kind
                    && object.as_ref().is_none_or(|id| {
                        e.object.id == *id || e.other.as_ref().is_some_and(|o| o.id == *id)
                    })
            })
        {
            return Some(StoppedAt {
                kind: Some(event.kind),
                ..stopped("event", event_detail(event))
            });
        }
        if let Some(paused) = self.paused
            && happened.pauses.contains(&paused)
        {
            let detail = if paused {
                "O jogo pausou"
            } else {
                "O jogo saiu da pausa"
            };
            return Some(stopped("paused", detail.into()));
        }
        if let Some((id, name)) = &self.sound
            && happened.sounds.contains(id)
        {
            return Some(stopped("sound", format!("O som {name} começou a tocar")));
        }
        if let Some(wanted) = self.bot
            && let Some((end, detail)) = &happened.bot
            && (wanted == BotEnd::Any || wanted == *end)
        {
            return Some(stopped("bot", detail.clone()));
        }
        None
    }
}

/// The request's bot as it plays, and what it did.
struct BotRun {
    bot: Bot,
    state: BotState,
    report: BotReport,
}
impl BotRun {
    /// Plays the coming step and records what happened; how the bot ended, if it did now.
    fn steer(&mut self, runtime: &Runtime, frame: &mut InputFrame) -> Option<(BotEnd, String)> {
        let time = rounded(runtime.time);
        let name = &self.report.entity.name;
        match bot::steer(runtime, &self.bot, &mut self.state, frame) {
            Some(BotEvent::Reached { index, position }) => self.report.reached.push(BotPoint {
                index,
                time,
                position: position.to_array(),
            }),
            Some(BotEvent::Stuck {
                index,
                position,
                distance,
            }) => {
                self.report.stuck = Some(BotStuck {
                    index,
                    time,
                    position: position.to_array(),
                    distance,
                });
                return Some((
                    BotEnd::Stuck,
                    format!(
                        "{name} parou de avançar rumo ao ponto {index} do bot, a {distance:.2} m \
                         dele"
                    ),
                ));
            }
            None => {}
        }
        if self.state.done() && self.report.done_at.is_none() {
            self.report.done_at = Some(time);
            return Some((
                BotEnd::Done,
                format!("{name} percorreu o caminho do bot até o fim"),
            ));
        }
        None
    }
}

/// A movement or area event in words, e.g. "Jogador aterrissou em Pouso".
fn event_detail(event: &GameEvent) -> String {
    let object = &event.object.name;
    let other = event.other.as_ref().map(|o| o.name.as_str());
    match (event.kind, other) {
        (EventKind::Jump, _) => format!("{object} pulou"),
        (EventKind::Land, Some(other)) => format!("{object} aterrissou em {other}"),
        (EventKind::Land, None) => format!("{object} aterrissou"),
        (EventKind::Leave, _) => format!("{object} perdeu o apoio"),
        (EventKind::Side, Some(other)) => format!("{object} encostou de lado em {other}"),
        (EventKind::Side, None) => format!("{object} encostou de lado"),
        (EventKind::Surface, _) => match &event.surface {
            Some(surface) => format!("{object} passou para a superfície {}", surface.name),
            None => format!("{object} ficou sem superfície"),
        },
        (EventKind::Posture, _) => match event.posture {
            Some("crouched") => format!("{object} agachou"),
            Some("sliding") => format!("{object} começou a deslizar"),
            _ => format!("{object} ficou em pé"),
        },
        (EventKind::AreaEnter, _) => format!("{object} entrou em {}", other.unwrap_or("uma área")),
        (EventKind::AreaExit, _) => format!("{object} saiu de {}", other.unwrap_or("uma área")),
    }
}

/// An input given to the game itself rather than through its input actions, as a window does.
#[derive(Clone, Debug, PartialEq)]
pub enum Direct {
    Click(Id),
    Escape,
}
impl Direct {
    pub fn apply(&self, runtime: &mut Runtime) {
        match self {
            Self::Click(entity) => runtime.click(entity),
            Self::Escape if runtime.has_pause_menu() => runtime.toggle_game_pause(),
            Self::Escape => runtime.log(
                "Esc: a cena não tem menu de pausa (nenhum objeto usa Ao pausar ou retomar); no \
                 jogo, Esc abriria a pausa da engine."
                    .into(),
            ),
        }
    }
}

/// Inputs planned for a running game: the timed ones, and the clicks and Esc by game time.
pub type PlannedInputs = (Vec<TimedInput>, Vec<(f64, Direct)>);

/// The object a click names: the ID of an object of any scene of `project`, or the name (any
/// case) of exactly one. An ID no object has is refused, but for the copies `action.spawn`
/// makes, which exist only while the game runs ([`random::is_derived_id`]).
fn clicked(project: &Project, text: &str) -> Result<Id, String> {
    let objects = || {
        project
            .scenes
            .iter()
            .flat_map(|s| s.entities.iter().map(move |e| (s, e)))
    };
    if objects().any(|(_, e)| e.id == text) || random::is_derived_id(text) {
        return Ok(text.into());
    }
    let wanted = text.trim().to_lowercase();
    let named: Vec<_> = objects()
        .filter(|(_, e)| e.name.trim().to_lowercase() == wanted)
        .collect();
    match named.as_slice() {
        [(_, e)] => Ok(e.id.clone()),
        [] => Err(format!(
            "click: \"{text}\" não é o ID nem o nome de um objeto do projeto"
        )),
        many => Err(format!(
            "click: há {} objetos chamados \"{text}\"; use o ID: {}",
            many.len(),
            many.iter()
                .map(|(s, e)| format!("{} (cena {})", e.id, s.name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// `inputs` for a game whose clock reads `start`, each `at` counted from it: presses, releases,
/// movement and look as timed inputs in time order, for an [`InputTimeline`] or
/// [`Runtime::queue_timed_inputs`], and clicks and Esc, which the host gives the game once its
/// time comes ([`Direct::apply`]). Actions are found by ID or name, and `"$N"` in `applied`.
pub fn plan_inputs(
    project: &Project,
    inputs: &[InputEvent],
    applied: &[Applied],
    start: f64,
) -> Result<PlannedInputs, String> {
    let mut events = inputs.to_vec();
    for event in &mut events {
        match event {
            InputEvent::Click { entity, .. } => {
                *entity = clicked(project, &resolve(entity, applied)?)?;
            }
            // A name the project does not have would be pressed in vain.
            InputEvent::Press { action, .. } | InputEvent::Release { action, .. } => {
                *action = input_actions::find(project, &resolve(action, applied)?)?;
            }
            _ => {}
        }
    }
    events.sort_by(|a, b| a.at().total_cmp(&b.at()));
    if events.iter().any(|e| !e.at().is_finite() || e.at() < 0.) {
        return Err("Os tempos das entradas precisam ser números a partir de 0".into());
    }
    // A number too large for the game, such as 1e39, reads as infinite.
    if events.iter().any(|e| match e {
        InputEvent::Movement { value, .. } | InputEvent::Look { value, .. } => {
            value.iter().any(|v| !v.is_finite())
        }
        _ => false,
    }) {
        return Err("Os valores de movement e look precisam ser números finitos".into());
    }
    let mut timed = Vec::new();
    let mut directs = Vec::new();
    for event in events {
        let (at, change) = match event {
            InputEvent::Press { at, action } => (at, InputChange::Press(action)),
            InputEvent::Release { at, action } => (at, InputChange::Release(action)),
            InputEvent::Movement { at, value } => (at, InputChange::Movement(value)),
            InputEvent::Look { at, value } => (at, InputChange::Look(value)),
            InputEvent::Click { at, entity } => {
                directs.push((start + at, Direct::Click(entity)));
                continue;
            }
            InputEvent::Escape { at } => {
                directs.push((start + at, Direct::Escape));
                continue;
            }
        };
        timed.push(TimedInput {
            at: start + at,
            change,
        });
    }
    Ok((timed, directs))
}

/// The replay check: a second copy of the game, fed the same clicks and inputs in lockstep and
/// compared after every step until the two differ.
struct Twin {
    runtime: Option<Runtime>,
    check: ReplayCheck,
}
impl Twin {
    fn direct(&mut self, direct: &Direct) {
        if let Some(twin) = &mut self.runtime {
            direct.apply(twin);
        }
    }
    fn advance(&mut self, frame: &InputFrame) {
        if let Some(twin) = &mut self.runtime {
            twin.advance(FIXED_DT, frame);
        }
    }
    /// Compares the copy with the first, both stepped `steps` times. `lookup` names the
    /// object that differs.
    fn compare(&mut self, lookup: Lookup, steps: usize) {
        let Some(twin) = &mut self.runtime else {
            return;
        };
        // The host's queues, taken as the recorder takes the first copy's.
        if let Some(recording) = &mut twin.recording {
            *recording = Recording::default();
        }
        twin.scene_starts.clear();
        twin.logs.clear();
        twin.traces.clear();
        twin.sounds.clear();
        twin.save_requests.clear();
        self.check.steps = steps;
        let runtime = lookup.runtime;
        if runtime.state_hash() == twin.state_hash() {
            return;
        }
        let difference = runtime
            .state_digest()
            .first_difference(&twin.state_digest());
        self.check.identical = false;
        self.check.first_difference = difference.map(|difference| ReplayDifference {
            time: rounded(runtime.time),
            object: difference.object.map(|id| {
                // An object only the second copy has is named by it.
                let name = lookup
                    .name(&id)
                    .or_else(|| twin.scene().entity(&id).map(|e| e.name.as_str()))
                    .unwrap_or(&id)
                    .to_owned();
                Named { id, name }
            }),
            part: difference.part,
        });
        self.runtime = None;
    }
}

/// The progress a request's `save` describes: each label a persistent variable of `project` by
/// ID or name (`"$N"` from setup too) and its key, with a value the variable takes.
fn saved_progress(
    project: &Project,
    values: &BTreeMap<String, Value>,
    applied: &[Applied],
) -> Result<SaveSlot, String> {
    let mut slot = SaveSlot {
        exists: true,
        ..SaveSlot::default()
    };
    let mut keys: HashMap<Id, usize> = HashMap::new();
    for (label, value) in values {
        let (variable, key) = variables::split_label(label);
        let id = variables::find(project, &resolve(variable, applied)?)
            .map_err(|error| format!("save: {error}"))?;
        let declared = save::check(project, &id, key, value).map_err(|error| {
            let hint = if error.contains("não é salva entre partidas") {
                "; marque-a com update_variable e persistent: true no setup, ou use \
                 update_variable no setup para mudar só o valor inicial"
            } else {
                ""
            };
            format!("save: {error}{hint}")
        })?;
        if !key.is_empty() {
            let count = keys.entry(id.clone()).or_default();
            *count += 1;
            if *count > variables::MAX_KEYS {
                return Err(format!(
                    "save: a variável {} passa de {} chaves",
                    declared.name,
                    variables::MAX_KEYS
                ));
            }
        }
        if slot
            .values
            .insert((id, key.to_owned()), value.clone())
            .is_some()
        {
            return Err(format!(
                "save: {} aparece duas vezes",
                variables::label(&declared.name, key)
            ));
        }
    }
    Ok(slot)
}

/// Runs `request` on a copy of `project`. Nothing in `project` changes.
pub fn simulate(
    project: &Project,
    request: &SimulationRequest,
) -> Result<SimulationReport, String> {
    run(project, request).map(|(report, _)| report)
}

/// Runs `request` with the replay check on (see [`SimulationRequest::check_replay`]).
pub fn check_replay(project: &Project, request: &SimulationRequest) -> Result<ReplayCheck, String> {
    let request = SimulationRequest {
        check_replay: true,
        ..request.clone()
    };
    let (report, _) = run(project, &request)?;
    Ok(report.replay.expect("the replay check was asked for"))
}

/// `simulate`, also returning the runtime at the end, e.g. to render the game view.
pub fn run(
    project: &Project,
    request: &SimulationRequest,
) -> Result<(SimulationReport, Runtime), String> {
    if !request.seconds.is_finite() || request.seconds <= 0. || request.seconds > MAX_SECONDS {
        return Err(format!(
            "Informe uma duração entre 0 e {MAX_SECONDS} segundos"
        ));
    }
    if request.samples > 120 {
        return Err("No máximo 120 amostras".into());
    }
    if request.seed > random::MAX_SEED {
        return Err(format!(
            "A semente (seed) deve ser um inteiro de 0 a {}",
            random::MAX_SEED
        ));
    }
    let view_size = request.view_size.unwrap_or(VIEW_SIZE);
    if view_size.iter().any(|v| !(16..=4096).contains(v)) {
        return Err("A vista do jogo deve ter entre 16 e 4096 pixels de lado".into());
    }
    // Setup and spawn change only this copy.
    let mut copy = Cow::Borrowed(project);
    let asked = request
        .scene
        .as_deref()
        .map(|scene| project.find_scene(scene, "scene").map(|s| s.id.clone()))
        .transpose()?;
    let default_scene = asked.clone().unwrap_or_else(|| project.start_scene.clone());
    let applied = if request.setup.is_empty() {
        Vec::new()
    } else {
        command::apply(copy.to_mut(), &default_scene, &request.setup)
            .map_err(|e| format!("Preparação (setup): {e}"))?
    };
    let start = asked.unwrap_or_else(|| copy.start_scene.clone());
    if copy.scene(&start).is_none() {
        return Err(format!("Cena {start} não existe"));
    }
    if let Some(spawn) = &request.spawn {
        let entity = resolve(&spawn.entity, &applied)?;
        place(copy.to_mut(), &start, spawn, &entity)?;
    }
    let source: &Project = &copy;
    let bot = request
        .bot
        .as_ref()
        .map(|bot| {
            let mut bot = bot.clone();
            bot.entity = resolve(&bot.entity, &applied)?;
            for point in &mut bot.path {
                if let Some(object) = &mut point.object {
                    *object = resolve(object, &applied)?;
                }
            }
            bot.check(source.scene(&start).expect("checked above"))?;
            Ok::<_, String>(bot)
        })
        .transpose()?;
    let (timed, directs) = plan_inputs(source, &request.inputs, &applied, 0.)?;
    let mut timeline = InputTimeline::default();
    timeline.push_all(timed, 0.)?;
    let watches: Vec<(WatchPlan, String)> = request
        .watch
        .iter()
        .map(|watch| {
            let id = resolve(&watch.id, &applied)?;
            let entity = source
                .scenes
                .iter()
                .find_map(|s| s.entity(&id))
                .ok_or_else(|| format!("Objeto observado {id} não existe no projeto"))?;
            let mut fields: BTreeSet<Field> = watch.fields.as_ref().map_or_else(
                || [Field::Position, Field::Visible, Field::Attributes].into(),
                |fields| fields.iter().copied().collect(),
            );
            let mut attributes = None;
            match &watch.attributes {
                None => {}
                Some(AttributeFilter::All(false)) => {
                    fields.remove(&Field::Attributes);
                }
                Some(AttributeFilter::All(true)) => {
                    fields.insert(Field::Attributes);
                }
                Some(AttributeFilter::Names(names)) => {
                    if let Some(missing) =
                        names.iter().find(|n| !entity.attributes.contains_key(*n))
                    {
                        return Err(format!("Atributo {missing} não existe em {}", entity.name));
                    }
                    fields.insert(Field::Attributes);
                    attributes = Some(names.iter().cloned().collect());
                }
            }
            Ok((
                WatchPlan {
                    id,
                    fields,
                    attributes,
                },
                entity.name.clone(),
            ))
        })
        .collect::<Result<_, String>>()?;
    let stop = request
        .until
        .as_ref()
        .map(|until| Stop::new(until, source, &start, &applied, bot.as_ref()))
        .transpose()?;
    let filter = &request.actions_filter;
    let objects = filter
        .objects
        .iter()
        .map(|id| resolve(id, &applied))
        .collect::<Result<_, String>>()?;
    let exclude = filter.exclude_operations.clone().unwrap_or_else(|| {
        if filter.operations.is_empty() {
            vec!["event.step".into()]
        } else {
            Vec::new()
        }
    });
    let mut recorder = Recorder {
        source,
        mode: request.actions,
        objects,
        operations: filter.operations.iter().cloned().collect(),
        exclude_events: exclude
            .iter()
            .filter(|op| op.starts_with("event."))
            .cloned()
            .collect(),
        exclude,
        excluded_nodes: HashMap::new(),
        spawned: HashMap::new(),
        summary: Summary::default(),
        actions: Recent::default(),
        scene: start.clone(),
        scene_changes: Recent::default(),
        logs: Recent::default(),
        sounds: Recent::default(),
        events: Recent::default(),
        saves: Recent::default(),
        pauses: Recent::default(),
    };
    let save = request
        .save
        .as_ref()
        .map(|values| saved_progress(source, values, &applied))
        .transpose()?;
    let options = RuntimeOptions {
        record: true,
        seed: request.seed,
        save,
    };
    let mut runtime = Runtime::with_options(source, &start, options.clone())?;
    let mut bot = bot.map(|bot| BotRun {
        state: BotState::new(&runtime, &bot),
        report: BotReport {
            entity: Named {
                name: source
                    .scene(&start)
                    .and_then(|s| s.entity(&bot.entity))
                    .map_or_else(|| bot.entity.clone(), |e| e.name.clone()),
                id: bot.entity.clone(),
            },
            reached: Vec::new(),
            done_at: None,
            stuck: None,
        },
        bot,
    });
    let mut twin = Twin {
        runtime: request
            .check_replay
            .then(|| Runtime::with_options(source, &start, options))
            .transpose()?,
        check: ReplayCheck {
            identical: true,
            steps: 0,
            first_difference: None,
        },
    };
    let mut report = SimulationReport {
        seconds: request.seconds,
        start_scene: start.clone(),
        final_scene: start,
        seed: request.seed,
        state_hash: String::new(),
        replay: None,
        stopped_at: None,
        paused: false,
        pauses: Vec::new(),
        setup: applied,
        scene_changes: Vec::new(),
        logs: Vec::new(),
        actions: None,
        sounds: Vec::new(),
        events: Vec::new(),
        bot: None,
        camera: None,
        ui: Vec::new(),
        watched: watches
            .iter()
            .map(|(watch, name)| Watched {
                id: watch.id.clone(),
                name: name.clone(),
                samples: Vec::new(),
            })
            .collect(),
        variables: BTreeMap::new(),
        saves: Vec::new(),
        save: None,
        audio: None,
        truncated: BTreeMap::new(),
    };
    let steps = (request.seconds / f64::from(FIXED_DT)).round().max(1.) as usize;
    // Sample k of n is taken at the step nearest k/n of the run, the last one being the final
    // state; with fewer steps than samples, every step has one.
    let samples = request.samples.max(1) as usize;
    let due = |k: usize| (k * steps + samples / 2) / samples;
    let mut next_sample = 1;
    let mut directs = directs.into_iter().peekable();
    // Sair do jogo ends the run whatever `until` asks.
    let check = |runtime: &Runtime, happened: &Happened| {
        if runtime.quit_requested {
            return Some(StoppedAt {
                time: rounded(runtime.time),
                reason: "quit",
                kind: None,
                detail: "O jogo pediu para sair (Sair do jogo)".into(),
            });
        }
        stop.as_ref().and_then(|s| s.check(runtime, happened))
    };
    // What the scene start logic did happens at time 0.
    let started = recorder.collect(&mut runtime);
    twin.compare(recorder.lookup(&runtime), 0);
    let mut stopped = check(&runtime, &started);
    if stopped.is_none() {
        for step in 1..=steps {
            while directs.peek().is_some_and(|(at, _)| *at <= runtime.time) {
                let (_, direct) = directs.next().unwrap();
                direct.apply(&mut runtime);
                twin.direct(&direct);
            }
            let mut frame = InputFrame::default();
            timeline.sample(runtime.time + f64::from(FIXED_DT), &mut frame);
            let ended = bot.as_mut().and_then(|bot| bot.steer(&runtime, &mut frame));
            runtime.advance(FIXED_DT, &frame);
            twin.advance(&frame);
            let mut happened = recorder.collect(&mut runtime);
            happened.bot = ended;
            twin.compare(recorder.lookup(&runtime), step);
            stopped = check(&runtime, &happened);
            if stopped.is_some() {
                break;
            }
            while next_sample < samples && due(next_sample) < step {
                next_sample += 1;
            }
            if next_sample < samples && due(next_sample) == step && step != steps {
                for ((watch, _), watched) in watches.iter().zip(&mut report.watched) {
                    watched
                        .samples
                        .push(sample(recorder.lookup(&runtime), watch));
                }
            }
        }
    }
    report.stopped_at = stopped;
    report.bot = bot.map(|bot| bot.report);
    report.paused = runtime.game_paused();
    for ((watch, _), watched) in watches.iter().zip(&mut report.watched) {
        watched
            .samples
            .push(sample(recorder.lookup(&runtime), watch));
    }
    report.camera = camera(&runtime);
    if request.ui {
        report.ui = game_ui::ui_state(
            runtime.project(),
            runtime.scene(),
            view_size.map(|side| side as f32),
        );
        // The topmost elements stay.
        let extra = report.ui.len().saturating_sub(MAX_ENTRIES);
        if extra > 0 {
            report.ui.drain(..extra);
            report.truncated.insert("ui", extra);
        }
    }
    recorder.finish(&mut report);
    let (variables, left_out) = runtime.variable_values(MAX_ENTRIES);
    report.variables = variables;
    if left_out > 0 {
        report.truncated.insert("variables", left_out);
    }
    let slot = runtime.save_slot();
    if slot.exists {
        let (save, left_out) = variables::labeled(source, &slot.values, MAX_ENTRIES);
        report.save = Some(save);
        if left_out > 0 {
            report.truncated.insert("save", left_out);
        }
    }
    report.audio = audio_report(&runtime);
    report.final_scene = runtime.scene_id().to_owned();
    report.state_hash = format!("{:016x}", runtime.state_hash());
    report.replay = request.check_replay.then_some(twin.check);
    Ok((report, runtime))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, Id, Id) {
        let mut project = Project::new("Registros");
        let mut entity = Entity::new("Porta", None);
        let node = crate::graph::Node::new("debug.message", [0.; 2]);
        let node_id = node.id.clone();
        entity.graph.nodes.push(node);
        let id = entity.id.clone();
        project.scenes[0].entities.push(entity);
        (project, id, node_id)
    }

    #[test]
    fn log_lines_name_their_object_and_node() {
        let (project, id, node) = project();
        let runtime = Runtime::new(&project, &project.start_scene).unwrap();
        let spawned = HashMap::new();
        let lookup = Lookup {
            runtime: &runtime,
            source: &project,
            spawned: &spawned,
        };
        let debug = log_entry(lookup, &[], 0.5, &format!("[{id} / {node}] Abriu"));
        assert_eq!(debug.text, "Abriu");
        assert_eq!(debug.object.as_ref().unwrap().name, "Porta");
        assert_eq!(debug.node.as_ref().unwrap().operation, "debug.message");
        let failure = log_entry(
            lookup,
            &[],
            0.5,
            &format!("Objeto {id} / nó {node}: Divisão por zero. Comportamento interrompido."),
        );
        assert_eq!(
            failure.text,
            "Divisão por zero. Comportamento interrompido."
        );
        assert_eq!(failure.node.unwrap().id, node);
        let character = log_entry(lookup, &[], 0.5, &format!("Personagem {id}: sem chão"));
        assert_eq!(character.text, "Personagem Porta: sem chão");
        assert_eq!(character.object.unwrap().id, id);
        let scene = &project.start_scene;
        let camera = log_entry(lookup, &[], 0., &format!("Câmera {scene}: aviso"));
        assert_eq!(
            camera.text,
            format!("Câmera {}: aviso", project.scenes[0].name)
        );
        assert!(camera.object.is_none());
        let plain = log_entry(lookup, &[], 0., "Limite de tarefas");
        assert_eq!(
            (plain.text.as_str(), plain.object),
            ("Limite de tarefas", None)
        );
    }

    #[test]
    fn inputs_for_a_running_game_count_from_its_clock() {
        let (mut project, door, _) = project();
        let open = input_actions::create(&mut project, "Abrir porta", "O").unwrap();
        let inputs: Vec<InputEvent> = serde_json::from_value(serde_json::json!([
            {"type": "release", "at": 0.5, "action": "abrir porta"},
            {"type": "click", "at": 0.25, "entity": door},
            {"type": "press", "at": 0.0, "action": open},
            {"type": "escape", "at": 0.0},
            {"type": "movement", "at": 0.5, "value": [0.0, 1.0]},
        ]))
        .unwrap();
        let (timed, directs) = plan_inputs(&project, &inputs, &[], 10.).unwrap();
        let timed: Vec<_> = timed
            .iter()
            .map(|input| (input.at, format!("{:?}", input.change)))
            .collect();
        assert_eq!(
            timed,
            [
                (10., format!("Press({open:?})")),
                (10.5, format!("Release({open:?})")),
                (10.5, "Movement([0.0, 1.0])".to_owned()),
            ],
            "by name or ID, in time order, equal times as given"
        );
        assert_eq!(
            directs,
            [(10., Direct::Escape), (10.25, Direct::Click(door))]
        );
        let unknown: Vec<InputEvent> = serde_json::from_value(
            serde_json::json!([{"type": "press", "at": 0.0, "action": "voar"}]),
        )
        .unwrap();
        let error = plan_inputs(&project, &unknown, &[], 0.).unwrap_err();
        assert!(error.contains("\"voar\" não existe"), "{error}");
        let early: Vec<InputEvent> =
            serde_json::from_value(serde_json::json!([{"type": "escape", "at": -1.0}])).unwrap();
        assert!(plan_inputs(&project, &early, &[], 5.).is_err());
        let huge: Vec<InputEvent> = serde_json::from_value(
            serde_json::json!([{"type": "look", "at": 0.0, "value": [1e39, 0.0]}]),
        )
        .unwrap();
        let error = plan_inputs(&project, &huge, &[], 0.).unwrap_err();
        assert!(error.contains("números finitos"), "{error}");
    }

    #[test]
    fn copies_already_removed_keep_their_name_and_node() {
        let (project, _, _) = project();
        let runtime = Runtime::new(&project, &project.start_scene).unwrap();
        let copy = new_id();
        let node = new_id();
        let spawned = HashMap::from([(copy.clone(), "Bala (cópia)".to_owned())]);
        let lookup = Lookup {
            runtime: &runtime,
            source: &project,
            spawned: &spawned,
        };
        let traces = [NodeTrace {
            object: copy.clone(),
            node: node.clone(),
            operation: "debug.message".into(),
            time: 0.5,
        }];
        let entry = log_entry(lookup, &traces, 0.5, &format!("[{copy} / {node}] acertou"));
        assert_eq!(entry.text, "acertou");
        assert_eq!(entry.object.unwrap().name, "Bala (cópia)");
        assert_eq!(entry.node.unwrap().operation, "debug.message");
        assert_eq!(
            action_text(lookup, &copy, "action.remove"),
            "Bala (cópia) · Remover objeto"
        );
    }

    #[test]
    fn the_summary_keeps_the_lines_most_recently_active() {
        let mut summary = Summary::default();
        // The text names the object by what comes before '#'.
        let text = |object: &str, operation: &str| {
            format!("{} · {operation}", object.split('#').next().unwrap())
        };
        // Two objects with the same name share a line.
        summary.count("Moeda#1".into(), "x".into(), 0., text);
        summary.count("Moeda#2".into(), "x".into(), 0.5, text);
        for i in 1..MAX_ENTRIES {
            summary.count(format!("Objeto {i}"), "x".into(), 1. + i as f64, text);
        }
        // The first line stays active; "Objeto 1" is now the one idle the longest.
        summary.count("Moeda#1".into(), "x".into(), 1000., text);
        summary.count("Novo".into(), "x".into(), 1001., text);
        let (lines, dropped) = summary.finish();
        assert_eq!((lines.len(), dropped), (MAX_ENTRIES, 1));
        assert_eq!(
            (lines[0].text.as_str(), lines[0].count, lines[0].first),
            ("Moeda · x", 3, 0.)
        );
        assert!(!lines.iter().any(|l| l.text == "Objeto 1 · x"));
        assert_eq!(lines.last().unwrap().text, "Novo · x");
    }

    #[test]
    fn the_replay_check_names_where_the_copies_part_and_stops_there() {
        use crate::graph::Node;
        // A Dado that writes a draw into its Valor every step.
        let mut project = Project::new("Dados");
        let mut die = Entity::new("Dado", None);
        die.attributes.insert("Valor".into(), Value::Number(0.));
        die.attributes.insert("Lado".into(), Value::Number(0.));
        let step = Node::new("event.step", [0.; 2]);
        let draw = Node::new("random.number", [0.; 2]);
        let mut keep = Node::new("attribute.set", [0.; 2]);
        keep.params
            .insert("attribute".into(), Value::Text("Valor".into()));
        let edge = |from: &Node, from_port: &str, to: &Node, to_port: &str| Edge {
            from_node: from.id.clone(),
            from_port: from_port.into(),
            to_node: to.id.clone(),
            to_port: to_port.into(),
        };
        die.graph.edges = vec![
            edge(&step, "exec", &draw, "exec"),
            edge(&draw, "exec", &keep, "exec"),
            edge(&draw, "value", &keep, "value"),
        ];
        die.graph.nodes = vec![step, draw, keep];
        let die_id = die.id.clone();
        project.scenes[0].entities.push(die);
        let start = |seed| {
            let options = RuntimeOptions {
                record: true,
                seed,
                ..Default::default()
            };
            Runtime::with_options(&project, &project.start_scene, options).unwrap()
        };
        let twin = |seed| Twin {
            runtime: Some(start(seed)),
            check: ReplayCheck {
                identical: true,
                steps: 0,
                first_difference: None,
            },
        };
        let spawned = HashMap::new();
        let compare = |twin: &mut Twin, runtime: &Runtime, steps| {
            let lookup = Lookup {
                runtime,
                source: &project,
                spawned: &spawned,
            };
            twin.compare(lookup, steps);
        };
        let frame = InputFrame::default();

        // Another seed: the draws part first, before the attribute they write.
        let mut main = start(0);
        let mut other = twin(1);
        main.advance(FIXED_DT, &frame);
        other.advance(&frame);
        compare(&mut other, &main, 1);
        let random = ReplayDifference {
            time: 0.0167,
            object: None,
            part: "random".into(),
        };
        assert_eq!(
            other.check,
            ReplayCheck {
                identical: false,
                steps: 1,
                first_difference: Some(random),
            }
        );
        assert!(other.runtime.is_none());

        // The same seed, with an attribute the logic leaves alone changed in the copy only:
        // the object by name, and the part.
        let mut main = start(0);
        let mut copy = twin(0);
        main.advance(FIXED_DT, &frame);
        copy.advance(&frame);
        compare(&mut copy, &main, 1);
        assert_eq!((copy.check.identical, copy.check.steps), (true, 1));
        main.advance(FIXED_DT, &frame);
        copy.advance(&frame);
        let changed = copy.runtime.as_mut().unwrap().scene_mut();
        changed
            .entity_mut(&die_id)
            .unwrap()
            .attributes
            .insert("Lado".into(), Value::Number(1.));
        compare(&mut copy, &main, 2);
        let parted = ReplayCheck {
            identical: false,
            steps: 2,
            first_difference: Some(ReplayDifference {
                time: 0.0333,
                object: Some(Named {
                    id: die_id,
                    name: "Dado".into(),
                }),
                part: "attributes.Lado".into(),
            }),
        };
        assert_eq!(copy.check, parted);
        assert!(copy.runtime.is_none());
        // Nothing more is compared after the first difference.
        main.advance(FIXED_DT, &frame);
        copy.advance(&frame);
        compare(&mut copy, &main, 3);
        assert_eq!(copy.check, parted);
    }

    #[test]
    fn placeholders_name_setup_ids_and_leave_other_text_alone() {
        let applied = vec![Applied {
            op: "create_entity".into(),
            created: vec!["a".into(), "b".into()],
            ..Default::default()
        }];
        assert_eq!(resolve("$0", &applied).unwrap(), "a");
        assert_eq!(resolve("$0.1", &applied).unwrap(), "b");
        assert!(resolve("$1", &applied).is_err());
        assert!(resolve("$0.2", &applied).is_err());
        for literal in ["$", "$x", "$+1", "$0.", "id-comum"] {
            assert_eq!(resolve(literal, &applied).unwrap(), literal);
        }
    }

    #[test]
    fn step_only_nodes_exclude_chains_shared_with_other_events() {
        use crate::graph::Node;
        let mut graph = Graph::default();
        let step = Node::new("event.step", [0.; 2]);
        let click = Node::new("event.click", [0.; 2]);
        let timer = Node::new("attribute.set", [0.; 2]);
        let shared = Node::new("debug.message", [0.; 2]);
        let edge = |from: &Node, to: &Node| Edge {
            from_node: from.id.clone(),
            from_port: "exec".into(),
            to_node: to.id.clone(),
            to_port: "exec".into(),
        };
        graph.edges = vec![
            edge(&step, &timer),
            edge(&timer, &shared),
            edge(&click, &shared),
        ];
        graph.nodes = vec![step.clone(), click, timer.clone(), shared.clone()];
        let only = reached_only_from(&graph, &["event.step".into()]);
        assert!(only.contains(&step.id) && only.contains(&timer.id));
        assert!(!only.contains(&shared.id), "reached by a click too");
    }
}
