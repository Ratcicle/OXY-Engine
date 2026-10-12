//! Runs a game without a window for a simulated time and reports what happened: scene
//! changes, logs, logic actions, sounds, movement and area events, the game camera and the
//! state of watched objects. Used by the MCP server so an agent can test a change, and by tests.
//!
//! Everything runs on a copy of the project: `setup` commands and `spawn` change only that
//! copy. Timed inputs live here, not in the runtime, so a scene change or reload neither
//! drops the inputs still to come nor releases the keys being held.
use crate::{
    character::{CameraMode, CharacterState, MovementEvent, Posture},
    command::{self, Applied, Command},
    document::*,
    graph::{self, Edge, Graph, PortType},
    input_timeline::{InputChange, InputTimeline, TimedInput},
    runtime::{FIXED_DT, InputFrame, NodeTrace, Runtime},
};
use glam::{EulerRot, Mat4, Quat, Vec3};
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    /// Starts holding an input action (as named in the project's input actions).
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
    /// Clicks an object or its interface button.
    Click {
        at: f64,
        entity: Id,
    },
}
impl InputEvent {
    fn at(&self) -> f64 {
        match self {
            Self::Press { at, .. }
            | Self::Release { at, .. }
            | Self::Movement { at, .. }
            | Self::Look { at, .. }
            | Self::Click { at, .. } => *at,
        }
    }
}

/// How the report lists the logic actions that ran.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionsMode {
    None,
    /// One entry per object and action, with count and first and last time.
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
    /// starts. Absent: `event.step` (per-step logic and timers), unless `operations` is given.
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
    /// `true`: any scene start, reloads included; a scene ID: that scene starting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_change: Option<SceneCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute: Option<AttributeCondition>,
    /// The object no longer exists in the running scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_absent: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventCondition>,
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
    /// A plain number, boolean or text, or a tagged value such as `{"Number": 3}`.
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
    /// Scene to start in; the project's start scene when absent.
    #[serde(default)]
    pub scene: Option<Id>,
    /// Simulated seconds; the upper bound when `until` is set.
    pub seconds: f64,
    #[serde(default)]
    pub inputs: Vec<InputEvent>,
    /// Objects reported in every sample.
    #[serde(default)]
    pub watch: Vec<Watch>,
    /// Evenly spaced samples of the watched objects, the last one being the final state
    /// (0 or 1 report only the final state; at most 120).
    #[serde(default)]
    pub samples: u32,
    /// Commands applied to the copy before the run, as in a batch; commands without a scene
    /// use the start scene. IDs they create are `"$N"` (or `"$N.M"`) in watch, inputs, spawn
    /// and until.
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
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SimulationReport {
    pub seconds: f64,
    pub start_scene: Id,
    pub final_scene: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<StoppedAt>,
    /// What each setup command did, with the IDs it created.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub setup: Vec<Applied>,
    pub scene_changes: Vec<SceneChange>,
    pub logs: Vec<Log>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Actions>,
    pub sounds: Vec<Event>,
    pub events: Vec<GameEvent>,
    /// The game camera at the end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraReport>,
    pub watched: Vec<Watched>,
    /// Entries dropped per list because it reached `MAX_ENTRIES`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub truncated: BTreeMap<&'static str, usize>,
}

/// Simulation time rounded to 0.1 ms, which keeps reports short.
fn rounded(time: f64) -> f64 {
    (time * 10_000.).round() / 10_000.
}

/// `"$N"` stands for the first ID created by setup command N, `"$N.M"` for the M-th.
fn resolve(id: &str, applied: &[Applied]) -> Result<Id, String> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let Some(rest) = id.strip_prefix('$') else {
        return Ok(id.into());
    };
    let (command, nth) = rest.split_once('.').unwrap_or((rest, "0"));
    if !digits(command) || !digits(nth) {
        return Ok(id.into());
    }
    command
        .parse::<usize>()
        .ok()
        .zip(nth.parse::<usize>().ok())
        .and_then(|(command, nth)| applied.get(command)?.created.get(nth))
        .cloned()
        .ok_or_else(|| format!("\"{id}\" não se refere a um ID criado pela preparação (setup)"))
}

/// An object by ID: in the running scene, in another scene of the run, or in the copy the
/// run started from (for objects removed since).
fn find_entity<'a>(runtime: &'a Runtime, source: &'a Project, id: &str) -> Option<&'a Entity> {
    runtime
        .scene()
        .entity(id)
        .or_else(|| runtime.project().scenes.iter().find_map(|s| s.entity(id)))
        .or_else(|| source.scenes.iter().find_map(|s| s.entity(id)))
}
fn named(runtime: &Runtime, source: &Project, id: &str) -> Named {
    Named {
        id: id.into(),
        name: find_entity(runtime, source, id).map_or_else(|| id.into(), |e| e.name.clone()),
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
fn node_line(line: &str) -> Option<(&str, &str, &str)> {
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
/// objects and scenes left in the text become their names.
fn log_entry(runtime: &Runtime, source: &Project, time: f64, line: &str) -> Log {
    let mut entry = Log {
        time: rounded(time),
        text: line.into(),
        object: None,
        node: None,
    };
    if let Some((object, node, message)) = node_line(line)
        && let Some(entity) = find_entity(runtime, source, object)
    {
        entry.object = Some(Named {
            id: object.into(),
            name: entity.name.clone(),
        });
        entry.node = Some(NodeRef {
            id: node.into(),
            operation: entity
                .graph
                .nodes
                .iter()
                .find(|n| n.id == node)
                .map_or_else(String::new, |n| n.operation.clone()),
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
            let name = if let Some(entity) = find_entity(runtime, source, id) {
                if entry.object.is_none() {
                    entry.object = Some(Named {
                        id: id.into(),
                        name: entity.name.clone(),
                    });
                }
                Some(entity.name.as_str())
            } else {
                runtime.project().scene(id).map(|s| s.name.as_str())
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
    summary: Vec<ActionSummary>,
    summary_keys: HashMap<(Id, String), usize>,
    summary_dropped: usize,
    actions: Recent<Event>,
    scene: Id,
    overlaps: BTreeSet<(Id, Id)>,
    scene_changes: Recent<SceneChange>,
    logs: Recent<Log>,
    sounds: Recent<Event>,
    events: Recent<GameEvent>,
}
impl Recorder<'_> {
    fn collect(&mut self, runtime: &mut Runtime) -> Happened {
        let time = runtime.time;
        let mut happened = Happened::default();
        let scenes = std::mem::take(&mut runtime.scene_starts);
        let logs = std::mem::take(&mut runtime.logs);
        let traces = std::mem::take(&mut runtime.traces);
        let sounds = std::mem::take(&mut runtime.sounds);
        let runtime = &*runtime;
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
            // The runtime forgets the old scene's overlaps; the new ones are new entries.
            self.overlaps.clear();
            happened.scenes.push(scene);
        }
        for line in logs {
            let entry = log_entry(runtime, self.source, time, &line);
            happened.logs.push(entry.text.clone());
            happened.logs.push(line);
            self.logs.push(entry);
        }
        for trace in traces {
            self.action(runtime, trace);
        }
        for sound in sounds {
            let name = runtime
                .project()
                .asset(&sound.asset)
                .map_or(sound.asset, |a| a.name.clone());
            self.sounds.push(Event {
                time: rounded(time),
                text: name,
            });
        }
        let mut events = Vec::new();
        let event = |kind, object: &str, other: Option<&str>| GameEvent {
            time: rounded(time),
            kind,
            object: named(runtime, self.source, object),
            other: other.map(|id| named(runtime, self.source, id)),
            posture: None,
            surface: None,
            speed: None,
        };
        for record in runtime.movement_records() {
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
        for crossing in runtime.sensor_events() {
            let kind = if crossing.entered {
                EventKind::AreaEnter
            } else {
                EventKind::AreaExit
            };
            events.push(event(kind, &crossing.object, Some(crossing.area.as_str())));
        }
        let overlaps: BTreeSet<(Id, Id)> = runtime
            .area_overlaps()
            .map(|(area, other)| (area.clone(), other.clone()))
            .collect();
        for (area, other) in overlaps.difference(&self.overlaps) {
            events.push(event(EventKind::AreaEnter, other, Some(area.as_str())));
        }
        // Like the runtime, a removed object does not leave an area.
        for (area, other) in self.overlaps.difference(&overlaps) {
            if runtime.scene().entity(area).is_some() && runtime.scene().entity(other).is_some() {
                events.push(event(EventKind::AreaExit, other, Some(area.as_str())));
            }
        }
        self.overlaps = overlaps;
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
        if !self.exclude_events.is_empty() {
            if !self.excluded_nodes.contains_key(&trace.object) {
                let nodes = find_entity(runtime, self.source, &trace.object)
                    .map(|e| reached_only_from(&e.graph, &self.exclude_events))
                    .unwrap_or_default();
                self.excluded_nodes.insert(trace.object.clone(), nodes);
            }
            if self.excluded_nodes[&trace.object].contains(&trace.node) {
                return;
            }
        }
        let time = rounded(trace.time);
        let text = |object: &str, operation: &str| {
            let action = graph::registry()
                .iter()
                .find(|op| op.id == operation)
                .map_or(operation, |op| op.label);
            format!("{} · {action}", named(runtime, self.source, object).name)
        };
        if self.mode == ActionsMode::All {
            self.actions.push(Event {
                time,
                text: text(&trace.object, &trace.operation),
            });
            return;
        }
        let key = (trace.object, trace.operation);
        if let Some(&i) = self.summary_keys.get(&key) {
            self.summary[i].count += 1;
            self.summary[i].last = time;
        } else if self.summary.len() < MAX_ENTRIES {
            self.summary.push(ActionSummary {
                text: text(&key.0, &key.1),
                count: 1,
                first: time,
                last: time,
            });
            self.summary_keys.insert(key, self.summary.len() - 1);
        } else {
            self.summary_dropped += 1;
        }
    }

    fn finish(self, report: &mut SimulationReport) {
        let truncated = &mut report.truncated;
        report.scene_changes = self.scene_changes.finish("scene_changes", truncated);
        report.logs = self.logs.finish("logs", truncated);
        report.sounds = self.sounds.finish("sounds", truncated);
        report.events = self.events.finish("events", truncated);
        report.actions = match self.mode {
            ActionsMode::None => None,
            ActionsMode::All => Some(Actions::All(self.actions.finish("actions", truncated))),
            ActionsMode::Summary => {
                if self.summary_dropped > 0 {
                    truncated.insert("actions", self.summary_dropped);
                }
                // Objects that share a name read as one line.
                let mut merged: Vec<ActionSummary> = Vec::new();
                let mut index: HashMap<String, usize> = HashMap::new();
                for entry in self.summary {
                    if let Some(&i) = index.get(&entry.text) {
                        let line = &mut merged[i];
                        line.count += entry.count;
                        line.first = line.first.min(entry.first);
                        line.last = line.last.max(entry.last);
                    } else {
                        index.insert(entry.text.clone(), merged.len());
                        merged.push(entry);
                    }
                }
                Some(Actions::Summary(merged))
            }
        };
    }
}

struct WatchPlan {
    id: Id,
    fields: BTreeSet<Field>,
    /// None: every attribute.
    attributes: Option<BTreeSet<String>>,
}

fn sample(runtime: &Runtime, source: &Project, watch: &WatchPlan) -> Sample {
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
            .map(|(_, rotation, _)| {
                let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
                [x.to_degrees(), y.to_degrees(), z.to_degrees()]
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
            .map(|state| character(runtime, source, state)),
    }
}
fn character(runtime: &Runtime, source: &Project, state: &CharacterState) -> CharacterSample {
    let velocity = state.total_velocity();
    CharacterSample {
        velocity: velocity.to_array(),
        speed: Vec3::new(velocity.x, 0., velocity.z).length(),
        grounded: state.grounded,
        posture: posture_name(state.posture),
        sliding: state.posture == Posture::Sliding,
        sprinting: state.sprinting,
        support: state.support.as_ref().map(|support| {
            let object = named(runtime, source, &support.object);
            SupportSample {
                id: object.id,
                name: object.name,
                surface: support
                    .surface
                    .as_deref()
                    .map(|id| surface(runtime.project(), id)),
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
    absent: Option<(Id, String)>,
    event: Option<(EventKind, Option<Id>)>,
}
impl Stop {
    fn new(
        until: &Until,
        project: &Project,
        start: &str,
        applied: &[Applied],
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
            Some(SceneCondition::Scene(id)) => {
                let id = resolve(id, applied)?;
                if project.scene(&id).is_none() {
                    return Err(format!("until: cena {id} não existe"));
                }
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
                if !target.attributes.contains_key(&condition.name) {
                    return Err(format!(
                        "until: atributo {} não existe em {}",
                        condition.name, target.name
                    ));
                }
                if condition.op.ordered() && condition.value.number().is_none() {
                    return Err(format!(
                        "until: a comparação {} precisa de um número",
                        condition.op.symbol()
                    ));
                }
                Some((
                    AttributeCondition {
                        entity: id,
                        ..condition.clone()
                    },
                    target.name.clone(),
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
        if scene.is_none()
            && until.log_contains.is_none()
            && attribute.is_none()
            && absent.is_none()
            && event.is_none()
        {
            return Err("until: informe ao menos uma condição".into());
        }
        Ok(Self {
            scene,
            log: until.log_contains.clone(),
            attribute,
            absent,
            event,
        })
    }

    /// The first condition that holds now, as (reason, detail).
    fn check(&self, runtime: &Runtime, happened: &Happened) -> Option<(&'static str, String)> {
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
            return Some(("scene_change", format!("Cena {name} iniciou")));
        }
        if let Some(text) = &self.log
            && let Some(line) = happened.logs.iter().find(|l| l.contains(text.as_str()))
        {
            return Some(("log_contains", line.clone()));
        }
        if let Some((condition, name)) = &self.attribute
            && let Some(value) = runtime
                .scene()
                .entity(&condition.entity)
                .and_then(|e| e.attributes.get(&condition.name))
            && condition.op.holds(value, &condition.value)
        {
            return Some((
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
        if let Some((id, name)) = &self.absent
            && runtime.scene().entity(id).is_none()
        {
            return Some(("entity_absent", format!("{name} não existe mais na cena")));
        }
        if let Some((kind, object)) = &self.event
            && let Some(event) = happened.events.iter().find(|e| {
                e.kind == *kind
                    && object.as_ref().is_none_or(|id| {
                        e.object.id == *id || e.other.as_ref().is_some_and(|o| o.id == *id)
                    })
            })
        {
            let kind = serde_json::to_value(event.kind)
                .ok()
                .and_then(|k| k.as_str().map(str::to_owned))
                .unwrap_or_default();
            let other = event
                .other
                .as_ref()
                .map_or_else(String::new, |o| format!(" ({})", o.name));
            return Some(("event", format!("{kind} de {}{other}", event.object.name)));
        }
        None
    }
}

/// Runs `request` on a copy of `project`. Nothing in `project` changes.
pub fn simulate(
    project: &Project,
    request: &SimulationRequest,
) -> Result<SimulationReport, String> {
    run(project, request).map(|(report, _)| report)
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
    // Setup and spawn change only this copy.
    let mut copy = Cow::Borrowed(project);
    let default_scene = request
        .scene
        .clone()
        .unwrap_or_else(|| project.start_scene.clone());
    let applied = if request.setup.is_empty() {
        Vec::new()
    } else {
        command::apply(copy.to_mut(), &default_scene, &request.setup)
            .map_err(|e| format!("Preparação (setup): {e}"))?
    };
    let start = request
        .scene
        .clone()
        .unwrap_or_else(|| copy.start_scene.clone());
    if copy.scene(&start).is_none() {
        return Err(format!("Cena {start} não existe"));
    }
    if let Some(spawn) = &request.spawn {
        let entity = resolve(&spawn.entity, &applied)?;
        place(copy.to_mut(), &start, spawn, &entity)?;
    }
    let source: &Project = &copy;
    let mut events = request.inputs.clone();
    for event in &mut events {
        if let InputEvent::Click { entity, .. } = event {
            *entity = resolve(entity, &applied)?;
        }
    }
    events.sort_by(|a, b| a.at().total_cmp(&b.at()));
    if events.iter().any(|e| !e.at().is_finite() || e.at() < 0.) {
        return Err("Os tempos das entradas precisam ser números a partir de 0".into());
    }
    let mut timeline = InputTimeline::default();
    let mut clicks = Vec::new();
    for event in events {
        let (at, change) = match event {
            InputEvent::Press { at, action } => (at, InputChange::Press(action)),
            InputEvent::Release { at, action } => (at, InputChange::Release(action)),
            InputEvent::Movement { at, value } => (at, InputChange::Movement(value)),
            InputEvent::Look { at, value } => (at, InputChange::Look(value)),
            InputEvent::Click { at, entity } => {
                clicks.push((at, entity));
                continue;
            }
        };
        timeline.push(TimedInput { at, change }, 0.)?;
    }
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
        .map(|until| Stop::new(until, source, &start, &applied))
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
        summary: Vec::new(),
        summary_keys: HashMap::new(),
        summary_dropped: 0,
        actions: Recent::default(),
        scene: start.clone(),
        overlaps: BTreeSet::new(),
        scene_changes: Recent::default(),
        logs: Recent::default(),
        sounds: Recent::default(),
        events: Recent::default(),
    };
    let mut runtime = Runtime::new(source, &start)?;
    let mut report = SimulationReport {
        seconds: request.seconds,
        start_scene: start.clone(),
        final_scene: start,
        stopped_at: None,
        setup: applied,
        scene_changes: Vec::new(),
        logs: Vec::new(),
        actions: None,
        sounds: Vec::new(),
        events: Vec::new(),
        camera: None,
        watched: watches
            .iter()
            .map(|(watch, name)| Watched {
                id: watch.id.clone(),
                name: name.clone(),
                samples: Vec::new(),
            })
            .collect(),
        truncated: BTreeMap::new(),
    };
    let steps = (request.seconds / f64::from(FIXED_DT)).round().max(1.) as usize;
    let every = (request.samples > 0).then(|| (steps / request.samples as usize).max(1));
    let mut clicks = clicks.into_iter().peekable();
    // What the scene start logic did happens at time 0.
    let started = recorder.collect(&mut runtime);
    let mut stopped = stop.as_ref().and_then(|s| s.check(&runtime, &started));
    if stopped.is_none() {
        for step in 1..=steps {
            while clicks.peek().is_some_and(|(at, _)| *at <= runtime.time) {
                let (_, entity) = clicks.next().unwrap();
                runtime.click(&entity);
            }
            let mut frame = InputFrame::default();
            timeline.sample(runtime.time + f64::from(FIXED_DT), &mut frame);
            runtime.advance(FIXED_DT, &frame);
            let happened = recorder.collect(&mut runtime);
            stopped = stop.as_ref().and_then(|s| s.check(&runtime, &happened));
            if stopped.is_some() {
                break;
            }
            if every.is_some_and(|every| step % every == 0) && step != steps {
                for ((watch, _), watched) in watches.iter().zip(&mut report.watched) {
                    watched.samples.push(sample(&runtime, source, watch));
                }
            }
        }
    }
    report.stopped_at = stopped.map(|(reason, detail)| StoppedAt {
        time: rounded(runtime.time),
        reason,
        detail,
    });
    for ((watch, _), watched) in watches.iter().zip(&mut report.watched) {
        watched.samples.push(sample(&runtime, source, watch));
    }
    report.camera = camera(&runtime);
    recorder.finish(&mut report);
    report.final_scene = runtime.scene_id().to_owned();
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
        let debug = log_entry(&runtime, &project, 0.5, &format!("[{id} / {node}] Abriu"));
        assert_eq!(debug.text, "Abriu");
        assert_eq!(debug.object.as_ref().unwrap().name, "Porta");
        assert_eq!(debug.node.as_ref().unwrap().operation, "debug.message");
        let failure = log_entry(
            &runtime,
            &project,
            0.5,
            &format!("Objeto {id} / nó {node}: Divisão por zero. Comportamento interrompido."),
        );
        assert_eq!(
            failure.text,
            "Divisão por zero. Comportamento interrompido."
        );
        assert_eq!(failure.node.unwrap().id, node);
        let character = log_entry(
            &runtime,
            &project,
            0.5,
            &format!("Personagem {id}: sem chão"),
        );
        assert_eq!(character.text, "Personagem Porta: sem chão");
        assert_eq!(character.object.unwrap().id, id);
        let scene = &project.start_scene;
        let camera = log_entry(&runtime, &project, 0., &format!("Câmera {scene}: aviso"));
        assert_eq!(
            camera.text,
            format!("Câmera {}: aviso", project.scenes[0].name)
        );
        assert!(camera.object.is_none());
        let plain = log_entry(&runtime, &project, 0., "Limite de tarefas");
        assert_eq!(
            (plain.text.as_str(), plain.object),
            ("Limite de tarefas", None)
        );
    }

    #[test]
    fn placeholders_name_setup_ids_and_leave_other_text_alone() {
        let applied = vec![Applied {
            op: "create_entity".into(),
            created: vec!["a".into(), "b".into()],
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
