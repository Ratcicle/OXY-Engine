//! Fixed-step, editor-independent execution of ordinary project documents.
//!
//! Runtime state has three lifetimes; a new field picks one and follows its rules:
//! - Per run: survives `change_scene` and dies with the `Runtime` (Parar, a new player run).
//!   The clock (`time`), activation serials, the host pause, logs and traces with their
//!   counters, the host queues (`sounds`, `save_requests`, `scene_starts`, `recording`), the
//!   quit request and `run`, the `RunState` (draws, copy counts, game variables, saved
//!   progress, time spent in the game pause) that new run-wide state goes into. `stop` resets
//!   `run`.
//! - Per scene: cleared in `change_scene` and in `stop`, though `stop` leaves the scene copy
//!   and its lookups readable (`change_scene` re-reads the copy from `source`). Prepared
//!   graphs, input in flight, tasks, controller bodies, animations, characters, cameras,
//!   stopped behaviors, area pairs, activations, hit leases and the game pause.
//! - Per object: the entries of per-scene structures keyed by an object, pruned in
//!   `remove_object` when it goes (the cameras' lazily, at the next presentation), and the
//!   run's counts of an object made during the run (see `forget_copies`).
//!
//! Two pauses are kept apart and may hold together: the host's (`paused`, the editor's Pausar
//! and the player's window) stops everything, while the game's own (`game_paused`, set by
//! logic or by Esc in a scene with a pause menu) freezes only the world. Gameplay clocks read
//! `game_time`, which stands still in the game pause; `time` keeps running for input, traces
//! and hosts.
//!
//! Anything whose order reaches events, tasks, logs, the recording or physics is walked in
//! an order the scene fixes (scene order, as `emit` visits owners), a BTreeMap or a Vec,
//! never in the order of a HashMap or HashSet. The state digest (`state_hash.rs`) reads every
//! field that shapes the steps to come, so a new one goes there too or is left out on purpose.
use crate::scene_view::{SceneEvaluation, SceneIndex, SceneView};
use crate::{
    animation::AnimationPlayer,
    audio::SoundRequest,
    collision::{Aabb, move_and_slide},
    document::{Id, Project, Scene, SceneKind, Value, validate_project},
    graph::Node,
};
use glam::Vec3;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, Weak};
type HitLease = Arc<Mutex<HashSet<Id>>>;
use crate::prepared_graph::PreparedGraph;
mod cameras;
pub use cameras::{CameraDebug, CameraPreview};
mod characters;
mod game_nodes;
mod math_nodes;
mod movement_nodes;
mod state_hash;
pub use characters::SensorCrossing;
pub use movement_nodes::{SceneQuery, SceneQueryShape};
pub use state_hash::{StateDifference, StateDigest, StatePart};

pub const FIXED_DT: f32 = 1.0 / 60.0;
pub const MAX_STEPS: usize = 8;
pub const MAX_NODE_WORK: usize = 1024;
/// Save requests a host may leave queued before they are folded (see `request_save`).
const MAX_SAVE_REQUESTS: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct InputFrame {
    pub held: BTreeSet<String>,
    pub pressed: BTreeSet<String>,
    pub released: BTreeSet<String>,
    /// Optional analog intent: right and forward, clamped only to unit length.
    pub movement: [f32; 2],
    /// Relative device displacement. Not pointer position or points per second.
    pub look: [f32; 2],
    pub wheel: f32,
}
impl InputFrame {
    pub fn held(&self, action: &str) -> bool {
        self.held.contains(action)
    }
    pub fn pressed(&self, action: &str) -> bool {
        self.pressed.contains(action)
    }
}

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    FixedStep,
    Movement(crate::character::MovementRecord),
    SceneStart,
    Input(String),
    InputHeld(String),
    InputReleased(String),
    Click(Id),
    AreaEnter {
        area: Id,
        other: Id,
        activation: u64,
    },
    AreaExit {
        area: Id,
        other: Id,
        activation: u64,
    },
    Animation {
        object: Id,
        marker: String,
    },
    /// The game pause started (true) or ended.
    Pause(bool),
}
#[derive(Clone, Debug)]
pub struct NodeTrace {
    pub object: Id,
    pub node: Id,
    pub operation: String,
    pub time: f64,
}
/// An area crossing as the runtime emits it, for 2D triggers and 3D sensors alike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AreaCrossing {
    pub area: Id,
    pub other: Id,
    pub entered: bool,
}
/// An object created by `action.spawn`, enough to name it and read its logic once it is gone.
#[derive(Clone, Debug)]
pub struct SpawnedObject {
    pub id: Id,
    pub name: String,
    pub graph: crate::graph::Graph,
}
/// Kept only for a host that reports a run (see `Runtime::recorded`). The lists fill as things
/// happen, so logic that later in the same step removes the objects involved or changes scene
/// cannot erase them. The host takes them after each step.
#[derive(Debug, Default)]
pub struct Recording {
    /// The crossings the runtime emits, in order. 2D exits are kept under the same rules even
    /// when no logic listens for them (the runtime then skips emitting them).
    pub crossings: Vec<AreaCrossing>,
    /// Movement records of 3D characters, taken right after they move.
    pub movements: Vec<crate::character::MovementRecord>,
    /// Every object of each spawned copy.
    pub spawned: Vec<SpawnedObject>,
    /// Each change of the game pause, scene changes ending one included: the new state.
    pub pauses: Vec<bool>,
}
#[derive(Clone, Debug)]
struct Context {
    owner: Id,
    other: Option<Id>,
    activation: u64,
    outputs: Arc<BTreeMap<(Id, String), Value>>,
    hits: Option<HitLease>,
    trajectory: Option<(Id, u64)>,
}
#[derive(Clone, Debug)]
struct Task {
    due: f64,
    owner: Id,
    node: Id,
    context: Context,
}
#[derive(Default)]
struct EventCatalog {
    operations: HashSet<String>,
    /// Scene order; invalidated with the existing event/structural revision.
    owners: Vec<usize>,
}
#[derive(Clone, Copy, Debug, Default)]
struct BodyState {
    velocity: Vec3,
    grounded: bool,
}
/// Run-lifetime state: `change_scene` keeps it and `stop` resets it. The home of the seeded
/// draws, the spawn counters, the game variables, the saved progress and the game-clock offset,
/// and later of music and volumes. The counts of an object made during the run go with it (see
/// `Runtime::forget_copies`).
#[derive(Debug, Default)]
pub(crate) struct RunState {
    /// What Sortear número draws from, with the draws so far of each (owner, node): a node's
    /// numbers depend on the seed and on that node alone. Definir semente aleatória sets the
    /// seed and starts the counts over.
    seed: u64,
    rolls: BTreeMap<(Id, Id), u64>,
    /// Copies made so far by each (owner, Criar cópia node), which name the IDs of the next
    /// one (see `spawn_ids`). The seed plays no part.
    spawns: BTreeMap<(Id, Id), u64>,
    /// The game variables' values by (variable, key): the empty key is the variable's own
    /// value, which starts at the declared one; a key never written reads the declared value.
    variables: BTreeMap<(Id, String), Value>,
    /// The saved progress as the host gave it, then as Salvar progresso and Apagar progresso
    /// left it; Ler progresso salvo reads whether there is any.
    save: crate::save::SaveSlot,
    /// Seconds of `time` spent in the game pause, in every scene of the run: the game clock is
    /// `time` minus them (see `Runtime::game_time`).
    paused_for: f64,
}
impl RunState {
    /// A run's start: the draws from `seed`, every variable at its declared value.
    fn new(project: &Project, seed: u64) -> Self {
        Self {
            seed,
            variables: project
                .variables
                .iter()
                .map(|v| ((v.id.clone(), String::new()), v.value.clone()))
                .collect(),
            ..Default::default()
        }
    }
}
/// How a host starts a [`Runtime`]. The default is [`Runtime::new`].
#[derive(Clone, Debug, Default)]
pub struct RuntimeOptions {
    /// Keep a [`Recording`] from the scene start logic on, as [`Runtime::recorded`] does.
    pub record: bool,
    /// What Sortear número draws from: hosts that play pass entropy, a headless run the seed
    /// asked for, 0 by default. The same seed and inputs give the same draws.
    pub seed: u64,
    /// The progress saved by earlier runs, which starts the persistent game variables before
    /// the scene start logic; none is a fresh install. Hosts read it from disk (see `save`).
    pub save: Option<crate::save::SaveSlot>,
}

pub struct Runtime {
    project: Project,
    scene_id: Id,
    index: OnceLock<Result<SceneIndex, String>>,
    graphs: HashMap<Id, Arc<PreparedGraph>>,
    pub paused: bool,
    pub logs: Vec<String>,
    pub traces: Vec<NodeTrace>,
    /// Lines ever added to `logs` and `traces`, those dropped or taken included: a host that
    /// reads them across frames finds the new ones with [`Runtime::logs_written`].
    logs_written: u64,
    traces_written: u64,
    /// Sounds requested since the host last took them, scene changes included: a sound
    /// played right before a restart is heard.
    pub sounds: Vec<SoundRequest>,
    /// How many of `sounds` came before the first `action.scene` of this step. A later one in
    /// the same step drops the rest: they belong to scenes that started and ended within the
    /// step, as in a scene that restarts itself as it starts until the work budget runs out.
    sounds_before_change: Option<usize>,
    /// What Salvar progresso and Apagar progresso asked since the host last took them, oldest
    /// first. Unlike sounds, scene changes and `stop` never drop them: they are the game's state.
    pub save_requests: Vec<crate::save::SaveRequest>,
    /// Scenes started by `change_scene` (reloads included), oldest first, until a host takes them.
    pub scene_starts: Vec<Id>,
    /// Off unless the runtime was created with `recorded`.
    pub recording: Option<Recording>,
    /// Sair do jogo ran: the player closes, the editor test stops, a simulation ends.
    pub quit_requested: bool,
    /// The run clock, which the game pause does not stop (see `game_time`).
    pub time: f64,
    /// The game's own pause (see the module docs); a scene starts without it.
    game_paused: bool,
    source: Project,
    accumulator: f32,
    pending_pressed: BTreeSet<String>,
    pending_released: BTreeSet<String>,
    pending_look: [f32; 2],
    pending_wheel: f32,
    characters: characters::Characters,
    cameras: cameras::Cameras,
    physics_dirty: bool,
    input_timeline: crate::input_timeline::InputTimeline,
    input_modes: OnceLock<BTreeMap<String, u8>>,
    event_operations: OnceLock<EventCatalog>,
    step_input: InputFrame,
    query_work: usize,
    ready: VecDeque<Task>,
    waiting: Vec<Task>,
    bodies: HashMap<Id, BodyState>,
    animations: HashMap<Id, AnimationPlayer>,
    disabled_behaviors: HashSet<Id>,
    overlap_pairs: HashSet<(Id, Id)>,
    area_activations: HashMap<Id, u64>,
    damage_hits: HashMap<(Id, u64), Weak<Mutex<HashSet<Id>>>>,
    area_hits: HashMap<Id, HitLease>,
    serial: u64,
    stopped: bool,
    run: RunState,
}

impl Runtime {
    pub fn new(project: &Project, scene_id: &str) -> Result<Self, String> {
        Self::with_options(project, scene_id, RuntimeOptions::default())
    }
    /// `new` with a `Recording`, kept from the scene start logic on.
    pub fn recorded(project: &Project, scene_id: &str) -> Result<Self, String> {
        Self::with_options(
            project,
            scene_id,
            RuntimeOptions {
                record: true,
                ..Default::default()
            },
        )
    }
    pub fn with_options(
        project: &Project,
        scene_id: &str,
        options: RuntimeOptions,
    ) -> Result<Self, String> {
        validate_project(project)?;
        if project.scene(scene_id).is_none() {
            return Err("Cena solicitada não existe.".into());
        }
        let mut runtime = Self {
            index: OnceLock::new(),
            graphs: HashMap::new(),
            project: project.clone(),
            scene_id: scene_id.into(),
            paused: false,
            logs: Vec::new(),
            traces: Vec::new(),
            logs_written: 0,
            traces_written: 0,
            sounds: Vec::new(),
            sounds_before_change: None,
            save_requests: Vec::new(),
            scene_starts: Vec::new(),
            recording: options.record.then(Recording::default),
            quit_requested: false,
            time: 0.0,
            game_paused: false,
            source: project.clone(),
            accumulator: 0.0,
            pending_pressed: BTreeSet::new(),
            pending_released: BTreeSet::new(),
            pending_look: [0.; 2],
            pending_wheel: 0.,
            characters: Default::default(),
            cameras: Default::default(),
            physics_dirty: true,
            input_timeline: Default::default(),
            input_modes: OnceLock::new(),
            event_operations: OnceLock::new(),
            step_input: InputFrame::default(),
            query_work: 0,
            ready: VecDeque::new(),
            waiting: Vec::new(),
            bodies: HashMap::new(),
            animations: HashMap::new(),
            disabled_behaviors: HashSet::new(),
            overlap_pairs: HashSet::new(),
            area_activations: HashMap::new(),
            damage_hits: HashMap::new(),
            area_hits: HashMap::new(),
            serial: 0,
            stopped: false,
            // Before the scene start logic, which may read the variables.
            run: RunState::new(project, options.seed),
        };
        if let Some(slot) = options.save {
            runtime.load_save(slot);
        }
        runtime.initialize_character_states()?;
        runtime.begin_camera_input(&InputFrame::default());
        runtime.emit(RuntimeEvent::SceneStart);
        let mut budget = MAX_NODE_WORK;
        runtime.process_tasks(&mut budget);
        Ok(runtime)
    }
    pub fn scene(&self) -> &Scene {
        self.project
            .scene(&self.scene_id)
            .expect("runtime scene invariant")
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn scene_id(&self) -> &str {
        &self.scene_id
    }
    fn index(&self) -> Option<&SceneIndex> {
        self.index
            .get_or_init(|| SceneIndex::new(self.scene()))
            .as_ref()
            .ok()
    }
    fn entity(&self, id: &str) -> Option<&crate::document::Entity> {
        self.index()?
            .position(id)
            .map(|i| &self.scene().entities[i])
    }
    fn entity_mut(&mut self, id: &str) -> Option<&mut crate::document::Entity> {
        let i = self.index()?.position(id)?;
        Some(&mut self.scene_mut_internal().entities[i])
    }
    pub fn scene_mut(&mut self) -> &mut Scene {
        self.physics_dirty = true;
        self.index.take();
        self.graphs.clear();
        self.input_modes.take();
        self.event_operations.take();
        self.scene_mut_internal()
    }
    fn scene_mut_internal(&mut self) -> &mut Scene {
        self.project
            .scene_mut(&self.scene_id)
            .expect("runtime scene invariant")
    }
    pub fn pending_tasks(&self) -> usize {
        self.ready.len() + self.waiting.len()
    }
    /// Logical live-state counts, not resident RAM or allocator capacity.
    pub fn retained_counts(&self) -> [usize; 4] {
        [
            self.damage_hits
                .values()
                .filter_map(Weak::upgrade)
                .map(|hits| hits.lock().unwrap().len())
                .sum(),
            self.area_activations.len(),
            self.ready.len(),
            self.waiting.len(),
        ]
    }
    /// The run state as text, for the lifetime contract test (tests/runtime_lifetimes.rs).
    #[doc(hidden)]
    pub fn run_state_debug(&self) -> String {
        format!("{:?}", self.run)
    }
    /// The per-scene structures holding anything, for the same test: none right after
    /// `change_scene` into a scene without start logic, characters or cameras, nor after `stop`.
    #[doc(hidden)]
    pub fn scene_state_in_use(&self) -> Vec<&'static str> {
        let InputFrame {
            held,
            pressed,
            released,
            movement,
            look,
            wheel,
        } = &self.step_input;
        [
            ("graphs", self.graphs.is_empty()),
            ("input_timeline", self.input_timeline.is_empty()),
            ("pending_pressed", self.pending_pressed.is_empty()),
            ("pending_released", self.pending_released.is_empty()),
            ("pending_look", self.pending_look == [0.; 2]),
            ("pending_wheel", self.pending_wheel == 0.),
            (
                "step_input",
                held.is_empty()
                    && pressed.is_empty()
                    && released.is_empty()
                    && *movement == [0.; 2]
                    && *look == [0.; 2]
                    && *wheel == 0.,
            ),
            (
                "characters",
                self.characters.states.is_empty() && self.characters.world.is_none(),
            ),
            (
                "cameras",
                self.cameras.worlds.is_none() && self.cameras.pose.is_none(),
            ),
            ("ready", self.ready.is_empty()),
            ("waiting", self.waiting.is_empty()),
            ("bodies", self.bodies.is_empty()),
            ("animations", self.animations.is_empty()),
            ("disabled_behaviors", self.disabled_behaviors.is_empty()),
            ("overlap_pairs", self.overlap_pairs.is_empty()),
            ("area_activations", self.area_activations.is_empty()),
            ("damage_hits", self.damage_hits.is_empty()),
            ("area_hits", self.area_hits.is_empty()),
            ("game_paused", !self.game_paused),
        ]
        .into_iter()
        .filter_map(|(name, empty)| (!empty).then_some(name))
        .collect()
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.input_timeline = Default::default();
        self.paused = paused;
        self.accumulator = 0.0;
        self.pending_pressed.clear();
        self.pending_released.clear();
        self.pending_look = [0.; 2];
        self.pending_wheel = 0.;
        self.characters.release_input();
        self.step_input = InputFrame::default();
        self.update_presentation(0.);
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.set_paused(true);
        self.graphs.clear();
        self.ready.clear();
        self.waiting.clear();
        self.sounds.clear();
        self.animations.clear();
        self.disabled_behaviors.clear();
        self.area_hits.clear();
        self.area_activations.clear();
        self.damage_hits.clear();
        self.overlap_pairs.clear();
        self.bodies.clear();
        self.characters = Default::default();
        self.cameras = Default::default();
        self.game_paused = false;
        self.run = RunState::new(&self.source, 0);
    }
    /// A click on an object or on its interface element. An element hidden, by itself or by a
    /// group above it, takes no click, as a person could not click it: it is logged. In the
    /// game pause only interface elements take clicks.
    pub fn click(&mut self, entity: &str) {
        if self.paused || self.stopped {
            return;
        }
        if let Some(e) = self.entity(entity).filter(|e| e.ui.is_some()) {
            if !self.shown(e) {
                let name = e.name.clone();
                self.log(format!(
                    "Clique ignorado: {name} está oculto (ele mesmo ou um grupo acima dele)."
                ));
                return;
            }
        } else if self.game_paused {
            return;
        }
        self.emit(RuntimeEvent::Click(entity.into()));
    }
    /// Whether `entity` and every group above it are visible.
    fn shown(&self, entity: &crate::document::Entity) -> bool {
        let mut current = Some(entity);
        for _ in 0..=self.scene().entities.len() {
            let Some(e) = current else { return true };
            if !e.visible {
                return false;
            }
            current = e.parent.as_deref().and_then(|id| self.entity(id));
        }
        false
    }
    pub fn advance(&mut self, elapsed: f32, input: &InputFrame) {
        if self.stopped || self.paused {
            self.accumulator = 0.0;
            self.pending_pressed.clear();
            self.pending_released.clear();
            self.pending_look = [0.; 2];
            self.pending_wheel = 0.;
            self.update_presentation(0.);
            return;
        }
        if !elapsed.is_finite() || elapsed < 0.0 {
            return;
        }
        self.pending_pressed.extend(input.pressed.iter().cloned());
        self.pending_released.extend(input.released.iter().cloned());
        // In the game pause the camera stands still; the wheel still reaches logic.
        if !self.game_paused && self.cameras.accepts_look(self.scene(), &self.characters) {
            for axis in 0..2 {
                if input.look[axis].is_finite() {
                    self.pending_look[axis] = (f64::from(self.pending_look[axis])
                        + f64::from(input.look[axis]))
                    .clamp(-f64::from(f32::MAX), f64::from(f32::MAX))
                        as f32;
                }
            }
        }
        if input.wheel.is_finite() {
            self.pending_wheel = (f64::from(self.pending_wheel) + f64::from(input.wheel))
                .clamp(-f64::from(f32::MAX), f64::from(f32::MAX))
                as f32;
        }
        self.accumulator = (self.accumulator + elapsed.min(0.25)).min(FIXED_DT * MAX_STEPS as f32);
        let mut steps = 0;
        let mut budget = MAX_NODE_WORK;
        while self.accumulator + f32::EPSILON >= FIXED_DT && steps < MAX_STEPS {
            let mut frame = InputFrame {
                held: input.held.clone(),
                pressed: std::mem::take(&mut self.pending_pressed),
                released: std::mem::take(&mut self.pending_released),
                movement: input.movement,
                look: std::mem::take(&mut self.pending_look),
                wheel: std::mem::take(&mut self.pending_wheel),
            };
            self.input_timeline
                .sample(self.time + f64::from(FIXED_DT), &mut frame);
            crate::metrics::timed(
                || self.fixed_step(&frame, &mut budget),
                |c, ns| c.fixed_step_ns += ns,
            );
            self.accumulator = (self.accumulator - FIXED_DT).max(0.0);
            steps += 1;
        }
        self.update_presentation(elapsed);
    }
    /// One step. In the game pause only the clock, input events, logic and activations run:
    /// the camera, Ao passo, movement, animations and areas wait, and so do waits, whose due
    /// times are in game time. A pause that logic starts in the first logic phase skips the
    /// world phase of that same step; one it ends takes effect on the next step.
    fn fixed_step(&mut self, input: &InputFrame, budget: &mut usize) {
        self.step_input = input.clone();
        self.query_work = 0;
        self.sounds_before_change = None;
        let paused = self.game_paused;
        if !paused {
            crate::metrics::timed(|| self.begin_camera_input(input), |c, ns| c.input_ns += ns);
        }
        self.collect_activations();
        crate::metrics::count(|c| c.steps += 1);
        self.time += f64::from(FIXED_DT);
        if paused {
            self.run.paused_for += f64::from(FIXED_DT);
        } else if self.event_used("event.step") {
            self.emit(RuntimeEvent::FixedStep);
        }
        for action in &input.pressed {
            self.emit(RuntimeEvent::Input(action.clone()));
        }
        for action in &input.held {
            if self.input_mode_used(action, 1) {
                self.emit(RuntimeEvent::InputHeld(action.clone()));
            }
        }
        for action in &input.released {
            if self.input_mode_used(action, 2) {
                self.emit(RuntimeEvent::InputReleased(action.clone()));
            }
        }
        crate::metrics::timed(|| self.process_tasks(budget), |c, ns| c.tasks_ns += ns);
        if paused || self.game_paused {
            self.collect_activations();
            return;
        }
        let movement_input = self.step_input.clone();
        crate::metrics::timed(
            || self.move_controllers(&movement_input),
            |c, ns| c.movement_ns += ns,
        );
        crate::metrics::timed(|| self.advance_animations(), |c, ns| c.animation_ns += ns);
        crate::metrics::timed(
            || self.move_characters(&movement_input),
            |c, ns| c.movement_ns += ns,
        );
        if let Some(mut recording) = self.recording.take() {
            recording
                .movements
                .extend_from_slice(self.movement_records());
            self.recording = Some(recording);
        }
        if self.event_used("event.character") {
            for record in self.movement_records().to_vec() {
                self.emit(RuntimeEvent::Movement(record));
            }
        }
        crate::metrics::timed(|| self.detect_areas(), |c, ns| c.areas_ns += ns);
        crate::metrics::timed(
            || self.detect_character_sensors(),
            |c, ns| {
                c.areas_ns += ns;
                c.character_sensors_ns += ns;
            },
        );
        crate::metrics::timed(|| self.process_tasks(budget), |c, ns| c.tasks_ns += ns);
        self.collect_activations();
    }
    pub fn queue_timed_input(
        &mut self,
        event: crate::input_timeline::TimedInput,
    ) -> Result<(), String> {
        self.input_timeline.push(event, self.time)
    }
    /// Log lines written since the runtime started; the last `logs.len()` of them are in
    /// `logs`.
    pub fn logs_written(&self) -> u64 {
        self.logs_written
    }
    /// Node traces written since the runtime started; the last `traces.len()` are in `traces`.
    pub fn traces_written(&self) -> u64 {
        self.traces_written
    }
    pub(crate) fn log(&mut self, message: String) {
        self.logs_written += 1;
        self.logs.push(message);
        if self.logs.len() > 500 {
            self.logs.drain(..100);
        }
    }
    fn input_mode_used(&self, action: &str, bit: u8) -> bool {
        let modes = self.input_modes.get_or_init(|| {
            let mut result = BTreeMap::new();
            for entity in &self.scene().entities {
                for node in &entity.graph.nodes {
                    if node.operation == "event.input" {
                        let mode = match node.text("mode") {
                            "held" => 1,
                            "released" => 2,
                            _ => 0,
                        };
                        *result.entry(node.text("action").to_owned()).or_insert(0) |= mode;
                    }
                }
            }
            result
        });
        modes.get(action).is_some_and(|m| m & bit != 0)
    }
    fn next_serial(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }
    fn lease(&mut self, owner: &str, activation: u64) -> HitLease {
        let key = (owner.to_owned(), activation);
        if let Some(hits) = self.damage_hits.get(&key).and_then(Weak::upgrade) {
            return hits;
        }
        let hits = Arc::new(Mutex::new(HashSet::new()));
        self.damage_hits.insert(key, Arc::downgrade(&hits));
        hits
    }
    fn activate_area(&mut self, id: &str) -> u64 {
        let serial = self.next_serial();
        let hits = self.lease(id, serial);
        self.area_activations.insert(id.into(), serial);
        self.area_hits.insert(id.into(), hits);
        serial
    }
    fn collect_activations(&mut self) {
        let ended: Vec<_> = self
            .area_activations
            .keys()
            .filter(|id| {
                !self.entity(id).is_some_and(|e| {
                    e.collider
                        .as_ref()
                        .is_some_and(|c| c.enabled && c.is_trigger)
                        || e.physics3d.as_ref().is_some_and(|c| c.enabled && c.sensor)
                })
            })
            .cloned()
            .collect();
        for id in ended {
            self.area_activations.remove(&id);
            self.area_hits.remove(&id);
        }
        // Once per phase, never per action. Waiting/ready tasks and active areas
        // own strong leases; completed activations cannot accumulate here.
        self.damage_hits.retain(|_, hits| hits.strong_count() > 0);
    }
    pub fn emit(&mut self, event: RuntimeEvent) {
        if self.stopped {
            return;
        }
        let generated_activation = self.next_serial();
        let mut found = Vec::new();
        for &index in &self.event_catalog().owners {
            let entity = &self.scene().entities[index];
            crate::metrics::count(|c| c.event_entity_visits += 1);
            if self.disabled_behaviors.contains(&entity.id) {
                continue;
            }
            for node in &entity.graph.nodes {
                let matches = match &event {
                    RuntimeEvent::FixedStep => node.operation == "event.step",
                    RuntimeEvent::Movement(record) => {
                        node.operation == "event.character"
                            && node
                                .params
                                .get("target")
                                .and_then(Value::object)
                                .unwrap_or(&entity.id)
                                == record.object
                            && node.text("kind") == movement_nodes::event_kind(&record.event)
                    }
                    RuntimeEvent::SceneStart => node.operation == "event.scene_start",
                    RuntimeEvent::Input(action) => {
                        node.operation == "event.input"
                            && node.text("action") == action
                            && matches!(node.text("mode"), "" | "pressed")
                    }
                    RuntimeEvent::InputHeld(action) => {
                        node.operation == "event.input"
                            && node.text("action") == action
                            && node.text("mode") == "held"
                    }
                    RuntimeEvent::InputReleased(action) => {
                        node.operation == "event.input"
                            && node.text("action") == action
                            && node.text("mode") == "released"
                    }
                    RuntimeEvent::Click(id) => entity.id == *id && node.operation == "event.click",
                    RuntimeEvent::AreaEnter { area, .. } => {
                        entity.id == *area && node.operation == "event.area_enter"
                    }
                    RuntimeEvent::AreaExit { area, .. } => {
                        entity.id == *area && node.operation == "event.area_exit"
                    }
                    RuntimeEvent::Animation { object, marker } => {
                        entity.id == *object
                            && node.operation == "event.animation"
                            && node.text("marker") == marker
                    }
                    RuntimeEvent::Pause(paused) => {
                        node.operation == "event.pause"
                            && node.text("when") == if *paused { "pause" } else { "resume" }
                    }
                };
                if matches {
                    let (other, activation) = match &event {
                        RuntimeEvent::Movement(record) => {
                            (Some(record.object.clone()), generated_activation)
                        }
                        RuntimeEvent::AreaEnter {
                            other, activation, ..
                        }
                        | RuntimeEvent::AreaExit {
                            other, activation, ..
                        } => (Some(other.clone()), *activation),
                        RuntimeEvent::Click(id) => (Some(id.clone()), generated_activation),
                        RuntimeEvent::Animation { object, .. } => {
                            (Some(object.clone()), generated_activation)
                        }
                        _ => (Some(entity.id.clone()), generated_activation),
                    };
                    let trajectory = match &event {
                        RuntimeEvent::Movement(record) => {
                            Some((record.object.clone(), record.state.trajectory))
                        }
                        RuntimeEvent::AreaEnter { other, .. }
                        | RuntimeEvent::AreaExit { other, .. } => self
                            .characters
                            .states
                            .get(other)
                            .map(|s| (other.clone(), s.trajectory)),
                        RuntimeEvent::FixedStep
                        | RuntimeEvent::Input(_)
                        | RuntimeEvent::InputHeld(_)
                        | RuntimeEvent::InputReleased(_) => self
                            .characters
                            .states
                            .get(&entity.id)
                            .map(|s| (entity.id.clone(), s.trajectory)),
                        _ => None,
                    };
                    let values = movement_nodes::event_values(&event, self.game_time())
                        .into_iter()
                        .map(|(key, value)| ((node.id.clone(), key.into()), value))
                        .collect();
                    found.push(Task {
                        due: self.game_time(),
                        owner: entity.id.clone(),
                        node: node.id.clone(),
                        context: Context {
                            owner: entity.id.clone(),
                            other,
                            activation,
                            outputs: Arc::new(values),
                            hits: None,
                            trajectory,
                        },
                    });
                }
            }
        }
        for task in &mut found {
            task.context.hits = Some(self.lease(&task.owner, task.context.activation));
        }
        self.ready.extend(found);
        if self.ready.len() + self.waiting.len() > 16384 {
            self.log(
                "Limite global de tarefas excedido; comportamentos pendentes foram cancelados."
                    .into(),
            );
            self.ready.clear();
            self.waiting.clear();
        }
    }
    fn process_tasks(&mut self, budget: &mut usize) {
        if *budget == 0 {
            return;
        }
        let mut future = Vec::new();
        let now = self.game_time();
        for task in std::mem::take(&mut self.waiting) {
            if task.due <= now {
                self.ready.push_back(task);
            } else {
                future.push(task);
            }
        }
        self.waiting = future;
        while let Some(task) = self.ready.pop_front() {
            if task
                .context
                .trajectory
                .as_ref()
                .is_some_and(|(id, generation)| {
                    self.characters
                        .states
                        .get(id)
                        .is_none_or(|s| s.trajectory != *generation)
                })
            {
                continue;
            }
            if self.disabled_behaviors.contains(&task.owner) || self.entity(&task.owner).is_none() {
                continue;
            }
            let owner = task.owner.clone();
            let node = task.node.clone();
            let result = self.execute(task, budget);
            if let Err(error) = result {
                self.log(format!(
                    "Objeto {} / nó {}: {error}. Comportamento interrompido.",
                    owner, node
                ));
                self.disabled_behaviors.insert(owner.clone());
                self.ready.retain(|queued| queued.owner != owner);
                self.waiting.retain(|queued| queued.owner != owner);
            }
            if *budget == 0 {
                if !self.ready.is_empty() {
                    self.log("Limite de trabalho por atualização atingido; demais tarefas retomam no próximo passo.".into());
                }
                break;
            }
        }
    }
    fn spend(budget: &mut usize) -> Result<(), String> {
        if *budget == 0 {
            return Err(format!(
                "Limite de {MAX_NODE_WORK} operações por atualização"
            ));
        }
        *budget -= 1;
        Ok(())
    }
    fn input_value(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        input: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        if depth > 64 {
            return Err("Profundidade de dados excede 64 nós".into());
        }
        if let Some(edge) = graph.input(&node.id, input) {
            return self.output_value(
                graph,
                &edge.from_node,
                &edge.from_port,
                context,
                budget,
                depth + 1,
            );
        }
        node.params
            .get(input)
            .cloned()
            .ok_or_else(|| format!("Entrada {input} sem conexão ou parâmetro"))
    }
    fn target(
        &self,
        graph: &PreparedGraph,
        node: &Node,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Id, String> {
        match self.input_value(graph, node, "target", context, budget, depth)? {
            Value::Object(None) if graph.input(&node.id,"target").is_some() => Err("A porta Objeto recebeu uma referência vazia; o responsável não será usado como substituto.".into()),
            Value::Object(value) => Ok(value.unwrap_or_else(|| context.owner.clone())),
            _ => Err("Entrada objeto precisa de referência de objeto".into()),
        }
    }
    fn output_value(
        &self,
        graph: &PreparedGraph,
        id: &str,
        output: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        Self::spend(budget)?;
        let node = graph.node(id).ok_or("Nó de dados não existe")?;
        if let Some(value) = context.outputs.get(&(id.into(), output.into())) {
            return Ok(value.clone());
        }
        if node.operation.starts_with("event.") {
            return match output {
                "context" => Ok(Value::Object(context.other.clone())),
                "self" => Ok(Value::Object(Some(context.owner.clone()))),
                _ => Err("Saída de evento não contém dados".into()),
            };
        }
        match node.operation.as_str() {
            "value.number" | "value.text" | "value.bool" | "value.object" | "value.vector2"
            | "value.vector3" | "value.surface" => node
                .params
                .get("value")
                .cloned()
                .ok_or("Constante sem valor".into()),
            "attribute.get" => {
                let target = self.target(graph, node, context, budget, depth + 1)?;
                self.entity(&target)
                    .ok_or("Objeto de leitura ausente")?
                    .attributes
                    .get(node.text("attribute"))
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "Atributo {} não existe no objeto {target}",
                            node.text("attribute")
                        )
                    })
            }
            "condition.compare" => {
                let a = self.input_value(graph, node, "a", context, budget, depth + 1)?;
                let b = self.input_value(graph, node, "b", context, budget, depth + 1)?;
                let result = match node.text("operator") {
                    "==" => a == b,
                    "!=" => a != b,
                    operator => {
                        let a = a.number().ok_or("Comparação de ordem precisa de números")?;
                        let b = b.number().ok_or("Comparação de ordem precisa de números")?;
                        match operator {
                            ">" => a > b,
                            ">=" => a >= b,
                            "<" => a < b,
                            "<=" => a <= b,
                            _ => return Err("Comparador inválido".into()),
                        }
                    }
                };
                Ok(Value::Bool(result))
            }
            _ => self.game_data(graph, node, output, context, budget, depth),
        }
    }
    fn outputs(
        &mut self,
        graph: &PreparedGraph,
        node: &Node,
        ports: &[&str],
        context: Context,
        delay: f64,
    ) {
        let mut tasks = Vec::new();
        for port in ports {
            for edge in graph.outputs(&node.id, port) {
                tasks.push(Task {
                    due: self.game_time() + delay,
                    owner: context.owner.clone(),
                    node: edge.to_node.clone(),
                    context: context.clone(),
                });
            }
        }
        if delay > 0.0 {
            self.waiting.extend(tasks);
        } else {
            for task in tasks.into_iter().rev() {
                self.ready.push_front(task);
            }
        }
    }
    fn execute(&mut self, task: Task, budget: &mut usize) -> Result<(), String> {
        crate::metrics::count(|c| c.actions += 1);
        Self::spend(budget)?;
        let graph = if let Some(graph) = self.graphs.get(&task.owner) {
            graph.clone()
        } else {
            let graph = Arc::new(PreparedGraph::new(
                self.entity(&task.owner)
                    .ok_or("Objeto removido")?
                    .graph
                    .clone(),
            ));
            self.graphs.insert(task.owner.clone(), graph.clone());
            graph
        };
        let node = graph.node(&task.node).ok_or("Nó não encontrado")?;
        let mut context = task.context;
        self.traces_written += 1;
        self.traces.push(NodeTrace {
            object: task.owner.clone(),
            node: node.id.clone(),
            operation: node.operation.clone(),
            time: self.time,
        });
        if self.traces.len() > 256 {
            self.traces.drain(..64);
        }
        let mut ports = vec!["exec"];
        let mut delay = 0.0;
        match node.operation.as_str() {
            operation if operation.starts_with("event.") => {}
            "condition.branch" => {
                let condition = self
                    .input_value(&graph, node, "condition", &context, budget, 0)?
                    .boolean()
                    .ok_or("Condição precisa ser booleana")?;
                ports = vec![if condition { "then" } else { "else" }];
            }
            "control.sequence" => ports = vec!["first", "second", "third"],
            "control.wait" => {
                delay = self
                    .input_value(&graph, node, "seconds", &context, budget, 0)?
                    .number()
                    .ok_or("Espera precisa de segundos numéricos")?;
                if !delay.is_finite() || !(0.0..=86400.0).contains(&delay) {
                    return Err("Espera deve estar entre 0 e 86400 segundos".into());
                }
                delay = delay.max(f64::from(FIXED_DT));
            }
            "debug.message" => self.log(format!(
                "[{} / {}] {}",
                task.owner,
                node.id,
                node.text("message")
            )),
            "attribute.set" => {
                let target = self.target(&graph, node, &context, budget, 0)?;
                let value = self.input_value(&graph, node, "value", &context, budget, 0)?;
                if !value.is_finite() {
                    return Err("Novo valor de atributo não finito.".into());
                }
                if let Some(id) = value.surface()
                    && !self.project.surfaces.iter().any(|s| s.id == id)
                {
                    return Err("Nova referência de superfície não existe.".into());
                }
                if let Value::Object(Some(id)) = &value
                    && self.entity(id).is_none()
                {
                    return Err("Nova referência de atributo não existe".into());
                }
                let entity = self
                    .entity_mut(&target)
                    .ok_or("Objeto de escrita não existe")?;
                let old = entity
                    .attributes
                    .get(node.text("attribute"))
                    .ok_or("Crie primeiro o atributo no painel de propriedades")?;
                if std::mem::discriminant(old) != std::mem::discriminant(&value) {
                    return Err("Valor incompatível com o tipo do atributo".into());
                }
                entity
                    .attributes
                    .insert(node.text("attribute").into(), value);
            }
            "action.damage" => {
                let target = self.target(&graph, node, &context, budget, 0)?;
                let amount = self
                    .input_value(&graph, node, "amount", &context, budget, 0)?
                    .number()
                    .ok_or("Dano precisa ser numérico")?;
                if !amount.is_finite() || amount < 0.0 {
                    return Err("Dano deve ser um número não negativo".into());
                }
                let hits = context
                    .hits
                    .as_ref()
                    .ok_or("Ativação sem contexto de acerto")?;
                if !node.boolean("once", true) || !hits.lock().unwrap().contains(&target) {
                    let entity = self
                        .entity_mut(&target)
                        .ok_or("Alvo do dano foi removido")?;
                    let value = entity
                        .attributes
                        .get_mut(node.text("attribute"))
                        .ok_or("Alvo não possui o atributo de dano configurado")?;
                    if let Value::Number(value) = value {
                        *value = (*value - amount).max(0.0);
                    } else {
                        return Err("Atributo de dano precisa ser numérico".into());
                    }
                    hits.lock().unwrap().insert(target);
                }
            }
            "action.animation" => {
                let target = self.target(&graph, node, &context, budget, 0)?;
                let entity = self.entity(&target).ok_or("Objeto animado ausente")?;
                let clip = entity
                    .clips
                    .iter()
                    .find(|clip| clip.id == node.text("clip"))
                    .ok_or("ID de clip não existe no objeto")?;
                let clip_id = clip.id.clone();
                if node.boolean("restart", true)
                    || !self
                        .animations
                        .get(&target)
                        .is_some_and(|player| player.playing && player.clip_id == clip_id)
                {
                    self.animations
                        .insert(target, AnimationPlayer::new(clip_id));
                }
            }
            "action.sound" => {
                let asset = self
                    .project
                    .asset(node.text("asset"))
                    .ok_or("Asset de áudio não existe")?;
                if asset.kind != crate::document::AssetKind::Audio {
                    return Err("Asset selecionado não é áudio".into());
                }
                self.sounds.push(SoundRequest {
                    asset: asset.id.clone(),
                    volume: node.number("volume", 1.0).clamp(0.0, 1.0) as f32,
                });
            }
            "action.spawn" => {
                let template = self.target(&graph, node, &context, budget, 0)?;
                self.physics_dirty = true;
                self.index.take();
                self.input_modes.take();
                self.event_operations.take();
                let key = (context.owner.clone(), node.id.clone());
                let count = self.run.spawns.get(&key).copied().unwrap_or(0);
                let root = self
                    .scene_mut_internal()
                    .duplicate_subtree_with(&template, &mut spawn_ids(&key, count))?;
                self.run.spawns.insert(key, count + 1);
                // The editor duplicate offset is not part of a runtime spawn's explicit offset.
                let delta = Vec3::new(
                    node.number("x", 0.0) as f32 - 0.5,
                    node.number("y", 0.0) as f32,
                    node.number("z", 0.0) as f32,
                );
                let descendants: HashSet<_> = self.scene().descendants(&root).into_iter().collect();
                if let Some(mut recording) = self.recording.take() {
                    recording.spawned.extend(
                        self.scene()
                            .entities
                            .iter()
                            .filter(|entity| descendants.contains(&entity.id))
                            .map(|entity| SpawnedObject {
                                id: entity.id.clone(),
                                name: entity.name.clone(),
                                graph: entity.graph.clone(),
                            }),
                    );
                    self.recording = Some(recording);
                }
                let entity = self.entity_mut(&root).unwrap();
                entity.transform.position =
                    (Vec3::from(entity.transform.position) + delta).to_array();
                for entity in self
                    .scene_mut_internal()
                    .entities
                    .iter_mut()
                    .filter(|entity| descendants.contains(&entity.id))
                {
                    for track in entity
                        .clips
                        .iter_mut()
                        .flat_map(|clip| &mut clip.tracks)
                        .filter(|track| track.target == root)
                    {
                        for key in &mut track.keyframes {
                            key.transform.position =
                                (Vec3::from(key.transform.position) + delta).to_array();
                        }
                    }
                }
                Arc::make_mut(&mut context.outputs).insert(
                    (node.id.clone(), "created".into()),
                    Value::Object(Some(root)),
                );
            }
            "action.remove" => {
                let target = self.target(&graph, node, &context, budget, 0)?;
                self.remove_object(&target);
                if self.entity(&context.owner).is_none() {
                    return Ok(());
                }
            }
            "action.component" => {
                self.physics_dirty = true;
                let target = self.target(&graph, node, &context, budget, 0)?;
                let enabled = node.boolean("enabled", true);
                match node.text("component") {
                    // The 2D box and the 3D collider or area alike; an old document may have both.
                    "collider" => {
                        let entity = self.entity_mut(&target).ok_or("Objeto ausente")?;
                        if entity.collider.is_none() && entity.physics3d.is_none() {
                            return Err("Objeto não tem colisor nem área".into());
                        }
                        let was = entity.collider.as_ref().is_some_and(|c| c.enabled)
                            || entity.physics3d.as_ref().is_some_and(|c| c.enabled);
                        if let Some(collider) = &mut entity.collider {
                            collider.enabled = enabled;
                        }
                        if let Some(collider) = &mut entity.physics3d {
                            collider.enabled = enabled;
                        }
                        // Turned back on, an area starts a new activation that whoever is inside
                        // enters afresh, even when it was off only within this step. Turned off,
                        // its occupancy ends without an exit.
                        if enabled && !was {
                            self.activate_area(&target);
                            self.overlap_pairs.retain(|(area, _)| area != &target);
                            self.characters.forget_area(&target);
                        }
                        if !enabled {
                            self.area_activations.remove(&target); self.area_hits.remove(&target);
                        }
                    }
                    "controller" => self.entity_mut(&target).ok_or("Objeto ausente")?.controller.as_mut().ok_or("Objeto não tem controlador")?.enabled = enabled,
                    "visible" => self.entity_mut(&target).ok_or("Objeto ausente")?.visible = enabled,
                    "behavior" => {
                        if self.entity(&target).is_none() { return Err("Objeto ausente".into()); }
                        if enabled { self.disabled_behaviors.remove(&target); } else { self.disabled_behaviors.insert(target.clone()); self.waiting.retain(|task| task.owner != target); }
                    }
                    "animation" => { if let Some(player) = self.animations.get_mut(&target) { player.playing = enabled; } }
                    "platform" => self.entity_mut(&target).ok_or("Objeto ausente")?.platform.as_mut().ok_or("Objeto não tem plataforma móvel")?.enabled = enabled,
                    _ => return Err("Componente desconhecido; use collider/controller/visible/behavior/animation/platform".into()),
                }
            }
            "action.scene" => {
                let scene = self.scene_destination(&graph, node, &context, budget)?;
                self.change_scene(&scene)?;
                // A sound played right before the first change of the step is heard; those
                // of a scene that starts and ends within the step are not.
                match self.sounds_before_change {
                    Some(kept) => self.sounds.truncate(kept),
                    None => self.sounds_before_change = Some(self.sounds.len()),
                }
                return Ok(());
            }
            _ => {
                if !self.game_action(&graph, node, &mut context, budget)? {
                    return Err(format!("Operação {} não é executável", node.operation));
                }
            }
        }
        self.outputs(&graph, node, &ports, context, delay);
        Ok(())
    }
    pub fn remove_object(&mut self, id: &str) {
        self.physics_dirty = true;
        let ids: HashSet<_> = self.scene().descendants(id).into_iter().collect();
        let mut characters = std::mem::take(&mut self.characters);
        characters.remove_entities(&ids, self.scene(), self.game_time());
        self.characters = characters;
        self.index.take();
        self.input_modes.take();
        self.event_operations.take();
        self.scene_mut_internal().remove_subtree(id);
        self.graphs.retain(|owner, _| !ids.contains(owner));
        self.area_hits.retain(|owner, _| !ids.contains(owner));
        self.area_activations
            .retain(|owner, _| !ids.contains(owner));
        self.ready.retain(|task| !ids.contains(&task.owner));
        self.waiting.retain(|task| !ids.contains(&task.owner));
        self.bodies.retain(|owner, _| !ids.contains(owner));
        self.animations.retain(|owner, _| !ids.contains(owner));
        self.disabled_behaviors.retain(|owner| !ids.contains(owner));
        self.overlap_pairs
            .retain(|(a, b)| !ids.contains(a) && !ids.contains(b));
        self.damage_hits.retain(|(a, _), hits| {
            if let Some(hits) = hits.upgrade() {
                hits.lock().unwrap().retain(|target| !ids.contains(target));
            }
            !ids.contains(a) && hits.strong_count() > 0
        });
        self.forget_copies(|owner| ids.contains(owner));
    }
    /// Drops the draw and copy counts of the objects `gone` names that the project lacks: an
    /// object made during the run never comes back, since copies get new IDs, while a scene
    /// reload brings back the project's own with their IDs, and their counts go on.
    fn forget_copies(&mut self, gone: impl Fn(&str) -> bool) {
        let source = &self.source;
        let mut authored: Option<HashSet<&str>> = None;
        let mut keep = |owner: &Id| {
            !gone(owner)
                || authored
                    .get_or_insert_with(|| {
                        source
                            .scenes
                            .iter()
                            .flat_map(|scene| &scene.entities)
                            .map(|entity| entity.id.as_str())
                            .collect()
                    })
                    .contains(owner.as_str())
        };
        self.run.rolls.retain(|(owner, _), _| keep(owner));
        self.run.spawns.retain(|(owner, _), _| keep(owner));
    }
    pub fn change_scene(&mut self, id: &str) -> Result<(), String> {
        self.index.take();
        self.input_modes.take();
        self.event_operations.take();
        self.graphs.clear();
        let scene = self
            .source
            .scene(id)
            .ok_or("Cena de destino não existe")?
            .clone();
        *self
            .project
            .scene_mut(id)
            .ok_or("Cena de destino não existe")? = scene;
        self.scene_id = id.into();
        self.scene_starts.push(id.into());
        if self.scene_starts.len() > 64 {
            self.scene_starts.drain(..32);
        }
        // Per-scene state only (see the module docs): `run` carries over, but for the counts
        // of the copies left behind.
        self.forget_copies(|_| true);
        self.input_timeline = Default::default();
        self.characters = Default::default();
        self.cameras = Default::default();
        self.pending_look = [0.; 2];
        self.pending_wheel = 0.;
        self.ready.clear();
        self.waiting.clear();
        self.bodies.clear();
        self.animations.clear();
        self.disabled_behaviors.clear();
        self.overlap_pairs.clear();
        self.area_activations.clear();
        self.damage_hits.clear();
        self.area_hits.clear();
        self.pending_pressed.clear();
        self.pending_released.clear();
        // The new scene starts unpaused, without Ao pausar ou retomar: the menu that showed
        // the pause went with the old one.
        if std::mem::take(&mut self.game_paused)
            && let Some(recording) = &mut self.recording
        {
            recording.pauses.push(false);
        }
        // Sounds already requested still play ("play a sound, then restart"): the host takes
        // them after the step. action.scene drops those of scenes left within the step.
        self.physics_dirty = true;
        self.step_input = InputFrame::default();
        self.query_work = 0;
        self.initialize_character_states()?;
        self.begin_camera_input(&InputFrame::default());
        self.emit(RuntimeEvent::SceneStart);
        Ok(())
    }
    pub fn collider_box(&self, id: &str) -> Option<Aabb> {
        collider_box(self.scene(), id)
    }
    fn record_crossing(&mut self, area: &str, other: &str, entered: bool) {
        if let Some(recording) = &mut self.recording {
            recording.crossings.push(AreaCrossing {
                area: area.into(),
                other: other.into(),
                entered,
            });
        }
    }
    fn move_controllers(&mut self, input: &InputFrame) {
        let controllers: Vec<_> = self
            .scene()
            .entities
            .iter()
            .filter_map(|entity| {
                entity
                    .controller
                    .as_ref()
                    .filter(|controller| controller.enabled)
                    .map(|controller| (entity.id.clone(), controller.clone()))
            })
            .collect();
        if controllers.is_empty() {
            return;
        }
        let Ok(mut evaluated) = crate::metrics::timed(
            || SceneEvaluation::new(self.scene()),
            |c, ns| c.physics_prepare_ns += ns,
        ) else {
            return;
        };
        let dimensions = if self.scene().kind == SceneKind::TwoD {
            2
        } else {
            3
        };
        let mut spatial = crate::metrics::timed(
            || {
                crate::broadphase::Candidates::new(
                    evaluated.boxes.iter().enumerate().filter_map(|(i, b)| {
                        self.scene().entities[i]
                            .collider
                            .as_ref()
                            .filter(|c| c.enabled && !c.is_trigger)
                            .and_then(|_| b.map(|b| (i, b)))
                    }),
                    dimensions,
                )
            },
            |c, ns| c.physics_prepare_ns += ns,
        );
        for (id, controller) in controllers {
            let mut body_state = self.bodies.get(&id).copied().unwrap_or_default();
            let horizontal = i32::from(input.held(&controller.actions.right))
                - i32::from(input.held(&controller.actions.left));
            body_state.velocity.x = horizontal as f32 * controller.speed;
            body_state.velocity.z = if dimensions == 3 {
                (i32::from(input.held(&controller.actions.back))
                    - i32::from(input.held(&controller.actions.forward))) as f32
                    * controller.speed
            } else {
                0.0
            };
            body_state.velocity.y -= controller.gravity * FIXED_DT;
            if input.pressed(&controller.actions.jump) && body_state.grounded {
                body_state.velocity.y = controller.jump;
                body_state.grounded = false;
            }
            let displacement = if let Some(body) = evaluated
                .index
                .position(&id)
                .and_then(|i| evaluated.boxes[i])
            {
                let obstacles: Vec<_> = crate::metrics::timed(
                    || {
                        let eligible = |i: usize| {
                            let entity = &self.scene().entities[i];
                            entity.id != id
                                && !evaluated.index.related(self.scene(), &id, &entity.id)
                        };
                        spatial
                            .motion(body, body_state.velocity * FIXED_DT, |i| {
                                evaluated.boxes[i].is_some_and(|b| body.overlaps(b, dimensions))
                                    && eligible(i)
                            })
                            .into_iter()
                            .filter(|i| eligible(*i))
                            .filter_map(|i| evaluated.boxes[i])
                            .inspect(|_| crate::metrics::count(|c| c.candidates += 1))
                            .collect()
                    },
                    |c, ns| c.physics_filter_ns += ns,
                );
                let result = crate::metrics::timed(
                    || move_and_slide(body, body_state.velocity, FIXED_DT, &obstacles, dimensions),
                    |c, ns| c.physics_resolve_ns += ns,
                );
                body_state.velocity = result.velocity;
                body_state.grounded = result.grounded;
                result.delta
            } else {
                body_state.velocity * FIXED_DT
            };
            let local_delta = self
                .entity(&id)
                .and_then(|entity| entity.parent.as_deref())
                .and_then(|parent| {
                    evaluated
                        .index
                        .position(parent)
                        .and_then(|i| evaluated.worlds[i])
                })
                .map_or(displacement, |matrix| {
                    matrix.inverse().transform_vector3(displacement)
                });
            if let Some(entity) = self.entity_mut(&id) {
                entity.transform.position =
                    (Vec3::from(entity.transform.position) + local_delta).to_array();
            }
            evaluated.refresh_subtree(self.scene(), &id);
            for changed in evaluated.index.descendants(self.scene(), &id) {
                if let Some(i) = evaluated.index.position(&changed)
                    && self.scene().entities[i]
                        .collider
                        .as_ref()
                        .is_some_and(|c| c.enabled && !c.is_trigger)
                {
                    spatial.changed(i);
                }
            }
            self.bodies.insert(id, body_state);
        }
    }
    fn advance_animations(&mut self) {
        if !self.animations.is_empty() && self.scene().kind == SceneKind::ThreeD {
            self.physics_dirty = true;
        }
        let mut players = std::mem::take(&mut self.animations);
        // Scene order, as `emit` visits owners: the markers of one step and clips that share
        // a target never follow the map's order.
        let index = self.index();
        let mut ordered: Vec<_> = players.iter_mut().collect();
        ordered.sort_by_key(|(owner, _)| index.and_then(|index| index.position(owner)));
        let mut events = Vec::new();
        for (owner, player) in ordered {
            let clip = self
                .entity(owner)
                .and_then(|entity| entity.clips.iter().find(|clip| clip.id == player.clip_id))
                .cloned();
            if let Some(mut clip) = clip {
                let previous_cycle = (player.time / f64::from(clip.duration)).floor();
                for marker in player.advance(&clip, FIXED_DT) {
                    events.push(RuntimeEvent::Animation {
                        object: owner.clone(),
                        marker,
                    });
                }
                if clip.looping && (player.time / f64::from(clip.duration)).floor() > previous_cycle
                {
                    for track in &clip.tracks {
                        if let (Some(first), Some(last)) =
                            (track.keyframes.first(), track.keyframes.last())
                            && !first
                                .transform
                                .matrix()
                                .abs_diff_eq(last.transform.matrix(), 1e-4)
                        {
                            let descendants = self
                                .index()
                                .map(|i| i.descendants(self.scene(), &track.target))
                                .unwrap_or_default();
                            self.characters.looped.extend(descendants);
                        }
                    }
                }
                // A physical root is owned by its controller; only visual children
                // may be animated. The authored clip is never rewritten here.
                let mut ignored = Vec::new();
                clip.tracks.retain(|track| {
                    let keep = self.entity(&track.target).is_none_or(|e| {
                        e.character3d.is_none()
                            && !e.platform.as_ref().is_some_and(|p| {
                                p.enabled && p.mode == crate::surface::PlatformMode::Velocity
                            })
                    });
                    if !keep {
                        ignored.push(track.target.clone());
                    }
                    keep
                });
                for target in ignored {
                    if self.characters.ignored_tracks.insert((
                        owner.clone(),
                        clip.id.clone(),
                        target.clone(),
                    )) {
                        self.log(format!("Animação {}: trilha de {target} ignorada porque a raiz tem movimento físico próprio; anime as peças filhas.",clip.name));
                    }
                }
                player.sample(self.scene_mut_internal(), &clip);
            }
        }
        self.animations = players;
        for event in events {
            self.emit(event);
        }
    }
    fn detect_areas(&mut self) {
        if !self.scene().entities.iter().any(|e| {
            e.collider
                .as_ref()
                .is_some_and(|c| c.enabled && c.is_trigger)
        }) {
            self.overlap_pairs.clear();
            return;
        }
        let Ok(index) = SceneIndex::new(self.scene()) else {
            return;
        };
        let dimensions = if self.scene().kind == SceneKind::TwoD {
            2
        } else {
            3
        };
        let colliders: Vec<_> = {
            let view = SceneView::new(self.scene());
            self.scene()
                .entities
                .iter()
                .filter_map(|entity| {
                    let collider = entity.collider.as_ref()?;
                    if !collider.enabled {
                        return None;
                    }
                    Some((
                        entity.id.clone(),
                        collider.is_trigger,
                        view.collider_bounds(&entity.id).ok()?,
                    ))
                })
                .collect()
        };
        let spatial = crate::broadphase::Candidates::new(
            colliders.iter().enumerate().map(|(i, (_, _, b))| (i, *b)),
            dimensions,
        );
        let mut pairs = HashSet::new();
        let mut events = Vec::new();
        for (area, is_trigger, volume) in &colliders {
            if !is_trigger {
                continue;
            }
            for i in spatial.query(*volume) {
                let (other, _, target) = &colliders[i];
                crate::metrics::count(|c| c.candidates += 1);
                if area == other
                    || !volume.overlaps(*target, dimensions)
                    || index.related(self.scene(), area, other)
                {
                    continue;
                }
                let pair = (area.clone(), other.clone());
                crate::metrics::count(|c| c.overlaps += 1);
                if !self.overlap_pairs.contains(&pair) {
                    let activation = if let Some(activation) = self.area_activations.get(area) {
                        *activation
                    } else {
                        self.activate_area(area)
                    };
                    events.push(RuntimeEvent::AreaEnter {
                        area: area.clone(),
                        other: other.clone(),
                        activation,
                    });
                }
                pairs.insert(pair);
            }
        }
        // Preserve pair state for future listeners, but do not compute a full
        // difference of dense sets when no graph consumes exit events (or records them).
        let listened = self.event_used("event.area_exit");
        if listened || self.recording.is_some() {
            let mut exited: Vec<_> = self
                .overlap_pairs
                .difference(&pairs)
                .filter(|(area, other)| {
                    index.position(area).is_some_and(|i| {
                        self.scene().entities[i]
                            .collider
                            .as_ref()
                            .is_some_and(|c| c.enabled && c.is_trigger)
                    }) && index.position(other).is_some()
                })
                .cloned()
                .collect();
            exited.sort_by_key(|(a, b)| (index.position(a), index.position(b)));
            for (area, other) in exited {
                if let Some(&activation) = self.area_activations.get(&area) {
                    events.push(RuntimeEvent::AreaExit {
                        area,
                        other,
                        activation,
                    });
                }
            }
        }
        self.overlap_pairs = pairs;
        for event in events {
            match &event {
                RuntimeEvent::AreaEnter { area, other, .. } => {
                    self.record_crossing(area, other, true);
                }
                RuntimeEvent::AreaExit { area, other, .. } => {
                    self.record_crossing(area, other, false);
                    if !listened {
                        continue;
                    }
                }
                _ => {}
            }
            self.emit(event);
        }
    }
}

pub fn collider_box(scene: &Scene, id: &str) -> Option<Aabb> {
    let collider = scene.entity(id)?.collider.as_ref()?;
    if !collider.enabled {
        return None;
    }
    crate::spatial::collider_bounds(scene, id).ok()
}

/// The IDs of copy number `count` made by the Criar cópia node of `(owner, node)`: each comes
/// from the ID of the original object, clip or node, so a run that makes the same copies gives
/// them the same IDs, in any order and whatever the seed. An ID the scene already has is
/// asked again and salted.
fn spawn_ids((owner, node): &(Id, Id), count: u64) -> impl FnMut(&str) -> Id {
    let (owner, node) = (owner.clone(), node.clone());
    let mut salts: HashMap<Id, u64> = HashMap::new();
    move |original: &str| {
        let salt = salts.entry(original.into()).or_insert(0);
        let (count, extra) = (count.to_le_bytes(), salt.to_le_bytes());
        let mut parts = vec![
            b"oxy.spawn".as_slice(),
            owner.as_bytes(),
            node.as_bytes(),
            &count,
            original.as_bytes(),
        ];
        if *salt > 0 {
            parts.push(&extra);
        }
        *salt += 1;
        crate::random::derived_id(&parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::{
        document::{Collider, Controller, Entity, Primitive},
        graph::Edge,
    };
    fn connect(graph: &mut Graph, from: &Node, port: &str, to: &Node, input: &str) {
        graph.edges.push(Edge {
            from_node: from.id.clone(),
            from_port: port.into(),
            to_node: to.id.clone(),
            to_port: input.into(),
        });
    }
    fn waiting_project() -> (Project, Id) {
        let mut project = Project::new("Isolamento");
        let mut entity = Entity::new("Objeto", Some(Primitive::Rectangle));
        entity
            .attributes
            .insert("contador".into(), Value::Number(0.0));
        let start = Node::new("event.scene_start", [0.0; 2]);
        let mut wait = Node::new("control.wait", [200.0, 0.0]);
        wait.params.insert("seconds".into(), Value::Number(0.1));
        let mut set = Node::new("attribute.set", [400.0, 0.0]);
        set.params
            .insert("attribute".into(), Value::Text("contador".into()));
        set.params.insert("value".into(), Value::Number(42.0));
        connect(&mut entity.graph, &start, "exec", &wait, "exec");
        connect(&mut entity.graph, &wait, "exec", &set, "exec");
        entity.graph.nodes = vec![start, wait, set];
        let id = entity.id.clone();
        project.scenes[0].entities.push(entity);
        (project, id)
    }
    #[test]
    fn waits_nonblocking_play_isolated_and_stop_cancels() {
        let (project, id) = waiting_project();
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        assert_eq!(runtime.pending_tasks(), 1);
        runtime.advance(0.05, &InputFrame::default());
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["contador"],
            Value::Number(0.0)
        );
        runtime.advance(0.1, &InputFrame::default());
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["contador"],
            Value::Number(42.0)
        );
        assert_eq!(
            project.scenes[0].entity(&id).unwrap().attributes["contador"],
            Value::Number(0.0)
        );
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        runtime.stop();
        runtime.advance(10.0, &InputFrame::default());
        assert_eq!(runtime.pending_tasks(), 0);
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["contador"],
            Value::Number(0.0)
        );
    }
    #[test]
    fn deletion_scene_change_and_pause_cancel_or_suspend_waits() {
        let (mut project, id) = waiting_project();
        let next = Scene::new("Outra", SceneKind::TwoD);
        let next_id = next.id.clone();
        project.scenes.push(next);
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        runtime.set_paused(true);
        runtime.advance(1.0, &InputFrame::default());
        assert_eq!(runtime.time, 0.0);
        runtime.set_paused(false);
        runtime.remove_object(&id);
        assert_eq!(runtime.pending_tasks(), 0);
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        runtime.change_scene(&next_id).unwrap();
        assert_eq!(runtime.pending_tasks(), 0);
        // Hosts that never take the scene starts keep a bounded list.
        for _ in 0..100 {
            runtime.change_scene(&next_id).unwrap();
        }
        assert!(runtime.scene_starts.len() <= 64);
        assert!(runtime.scene_starts.iter().all(|id| *id == next_id));
        // The game pause holds the wait for as long as it lasts; it then ends on time.
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        assert!(runtime.set_game_paused(true));
        assert!(!runtime.set_game_paused(true), "already paused");
        runtime.advance(1.0, &InputFrame::default());
        for _ in 0..60 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        assert_eq!(runtime.pending_tasks(), 1);
        assert_eq!(runtime.game_time(), 0.0);
        assert!(runtime.time > 1.0);
        runtime.set_game_paused(false);
        for _ in 0..5 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["contador"],
            Value::Number(0.0)
        );
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["contador"],
            Value::Number(42.0)
        );
    }

    /// Links `nodes` of `entity` in a chain of execution, in order.
    fn chain(entity: &mut Entity, nodes: Vec<Node>) {
        for pair in nodes.windows(2) {
            connect(&mut entity.graph, &pair[0], "exec", &pair[1], "exec");
        }
        entity.graph.nodes.extend(nodes);
    }
    fn node(operation: &str, params: &[(&str, Value)]) -> Node {
        let mut node = Node::new(operation, [0.0; 2]);
        for (key, value) in params {
            node.params.insert((*key).into(), value.clone());
        }
        node
    }
    fn message(text: &str) -> Node {
        node("debug.message", &[("message", Value::Text(text.into()))])
    }
    /// A 2D scene that falls (a controller), counts steps and keeps each one's time, logs
    /// input and keeps the time Ler tempo gives it, waits 0.1 s at the start, pauses on
    /// "pausar", has a world object and a Continuar button that resumes, a button hidden in a
    /// group, and logs each change of the game pause.
    fn pausing_project() -> (Project, [Id; 5]) {
        let mut project = Project::new("Pausa do jogo");
        let mut faller = Entity::new("Móvel", Some(Primitive::Rectangle));
        faller.controller = Some(Controller::default());
        faller.collider = Some(Collider::default());
        faller.transform.position[1] = 50.0;
        let mut clock = Entity::new("Relógio", None);
        for attribute in ["contador", "tempo_passo", "tempo_entrada"] {
            clock
                .attributes
                .insert(attribute.into(), Value::Number(0.0));
        }
        let set = |attribute: &str| {
            node(
                "attribute.set",
                &[("attribute", Value::Text(attribute.into()))],
            )
        };
        chain(
            &mut clock,
            vec![
                node("event.scene_start", &[]),
                node("control.wait", &[("seconds", Value::Number(0.1))]),
                node(
                    "attribute.set",
                    &[
                        ("attribute", Value::Text("contador".into())),
                        ("value", Value::Number(42.0)),
                    ],
                ),
            ],
        );
        let step = node("event.step", &[]);
        let step_time = set("tempo_passo");
        connect(&mut clock.graph, &step, "time", &step_time, "value");
        chain(&mut clock, vec![step, message("passo"), step_time]);
        let action = |name: &str| ("action", Value::Text(name.into()));
        let read = node("time.read", &[]);
        let input_time = set("tempo_entrada");
        connect(&mut clock.graph, &read, "time", &input_time, "value");
        clock.graph.nodes.push(read);
        chain(
            &mut clock,
            vec![
                node("event.input", &[action("atacar")]),
                message("entrada"),
                input_time,
            ],
        );
        chain(
            &mut clock,
            vec![
                node("event.input", &[action("pausar")]),
                node("game.pause", &[("mode", Value::Text("pause".into()))]),
            ],
        );
        for when in ["pause", "resume"] {
            chain(
                &mut clock,
                vec![
                    node("event.pause", &[("when", Value::Text(when.into()))]),
                    message(when),
                ],
            );
        }
        let mut target = Entity::new("Alvo", Some(Primitive::Rectangle));
        target.transform.position[0] = 10.0;
        chain(
            &mut target,
            vec![node("event.click", &[]), message("mundo")],
        );
        let button = || crate::document::UiElement {
            kind: crate::document::UiKind::Button,
            ..Default::default()
        };
        let mut resume = Entity::new("Continuar", None);
        resume.ui = Some(button());
        chain(
            &mut resume,
            vec![
                node("event.click", &[]),
                node("game.pause", &[("mode", Value::Text("resume".into()))]),
            ],
        );
        let mut group = Entity::new("Grupo oculto", None);
        group.visible = false;
        let mut hidden = Entity::new("Escondido", None);
        hidden.parent = Some(group.id.clone());
        hidden.ui = Some(button());
        chain(
            &mut hidden,
            vec![node("event.click", &[]), message("oculto")],
        );
        let ids = [
            faller.id.clone(),
            clock.id.clone(),
            target.id.clone(),
            resume.id.clone(),
            hidden.id.clone(),
        ];
        project.scenes[0].kind = SceneKind::TwoD;
        project.scenes[0]
            .entities
            .extend([faller, clock, target, resume, group, hidden]);
        validate_project(&project).unwrap();
        (project, ids)
    }
    #[test]
    fn game_pause_freezes_world_but_runs_input_and_ui_clicks() {
        let (mut project, [faller, clock, target, resume, hidden]) = pausing_project();
        let other = Scene::new("Outra", SceneKind::TwoD);
        let other_id = other.id.clone();
        project.scenes.push(other);
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        let count = |runtime: &Runtime, text: &str| {
            runtime.logs.iter().filter(|l| l.ends_with(text)).count()
        };
        let height =
            |runtime: &Runtime| runtime.scene().entity(&faller).unwrap().transform.position[1];
        let attribute = |runtime: &Runtime, name: &str| {
            runtime.scene().entity(&clock).unwrap().attributes[name].clone()
        };
        let counter = |runtime: &Runtime| attribute(runtime, "contador");
        let pressed = |action: &str| InputFrame {
            pressed: [action.to_owned()].into(),
            ..Default::default()
        };
        for _ in 0..2 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        assert_eq!(count(&runtime, "passo"), 2);
        let falling = height(&runtime);
        assert!(falling < 50.0);
        assert_eq!(
            runtime.game_time(),
            runtime.time,
            "the same clock until a pause"
        );
        assert!(runtime.has_pause_menu());

        assert!(runtime.set_game_paused(true));
        let (time, game_time) = (runtime.time, runtime.game_time());
        for step in 0..60 {
            let input = if step == 30 {
                pressed("atacar")
            } else {
                InputFrame::default()
            };
            runtime.advance(FIXED_DT, &input);
        }
        assert!(runtime.game_paused());
        assert_eq!(count(&runtime, "pause"), 1, "{:?}", runtime.logs);
        assert_eq!(height(&runtime), falling, "the controller stands still");
        assert_eq!(count(&runtime, "passo"), 2, "Ao passo waits");
        assert_eq!(counter(&runtime), Value::Number(0.0), "the wait waits");
        assert_eq!(count(&runtime, "entrada"), 1, "input still runs");
        assert!((runtime.time - time - 1.0).abs() < 1e-3);
        assert_eq!(runtime.game_time(), game_time);
        // Ler tempo, read half a second into the pause, gave the game clock that stood still.
        assert_eq!(
            attribute(&runtime, "tempo_entrada"),
            Value::Number(game_time)
        );

        // World objects take no click in the pause; hidden elements never do, with a line.
        runtime.click(&target);
        runtime.click(&hidden);
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(count(&runtime, "mundo"), 0);
        assert_eq!(count(&runtime, "oculto"), 0);
        let ignored = "Clique ignorado: Escondido está oculto (ele mesmo ou um grupo acima dele).";
        assert!(
            runtime.logs.iter().any(|l| l == ignored),
            "{:?}",
            runtime.logs
        );
        // The engine's pause over the game's: resuming it goes back to the game pause.
        runtime.set_paused(true);
        runtime.advance(1.0, &InputFrame::default());
        runtime.set_paused(false);
        assert!(runtime.game_paused());
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(height(&runtime), falling);

        // Continuar resumes; the world waits for the next step.
        runtime.click(&resume);
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert!(!runtime.game_paused());
        assert_eq!(count(&runtime, "resume"), 1);
        assert_eq!(height(&runtime), falling);
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert!(height(&runtime) < falling);
        assert_eq!(count(&runtime, "passo"), 3);
        // Ao passo gives the game clock as well, behind the run clock by the pause.
        assert_eq!(
            attribute(&runtime, "tempo_passo"),
            Value::Number(runtime.game_time())
        );
        assert!(runtime.time - runtime.game_time() > 1.0);
        runtime.click(&target);
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(count(&runtime, "mundo"), 1, "world clicks are back");
        // The wait counts game time only: 0.1 s (6 steps) from the start, 2 + 2 already gone.
        assert_eq!(counter(&runtime), Value::Number(0.0));
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(counter(&runtime), Value::Number(0.0), "5 steps");
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(counter(&runtime), Value::Number(42.0), "6 steps");

        // Logic that pauses within a step skips that step's world phase.
        let before = height(&runtime);
        runtime.advance(FIXED_DT, &pressed("pausar"));
        assert!(runtime.game_paused());
        assert_eq!(height(&runtime), before);
        assert_eq!(count(&runtime, "pause"), 2);
        // A scene change ends the pause, without the event.
        runtime.change_scene(&other_id).unwrap();
        assert!(!runtime.game_paused());
        assert!(!runtime.has_pause_menu());
        assert!(runtime.scene_state_in_use().is_empty());
    }
    #[test]
    fn controller_falls_lands_and_jump_does_not_tunnel() {
        let mut project = Project::new("Colisões");
        let mut entity = Entity::new("Móvel", Some(Primitive::Rectangle));
        entity.controller = Some(Controller::default());
        entity.collider = Some(Collider::default());
        entity.transform.position[1] = 3.0;
        let id = entity.id.clone();
        let mut floor = Entity::new("Chão", Some(Primitive::Rectangle));
        floor.transform.position[1] = -0.5;
        floor.collider = Some(Collider {
            size: [30.0, 1.0, 1.0],
            ..Collider::default()
        });
        project.scenes[0].entities = vec![entity, floor];
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        for _ in 0..120 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        assert!((runtime.scene().entity(&id).unwrap().transform.position[1] - 0.5).abs() < 0.0001);
        let input = InputFrame {
            pressed: BTreeSet::from(["pular".into()]),
            held: BTreeSet::new(),
            released: BTreeSet::new(),
            ..Default::default()
        };
        runtime.advance(FIXED_DT, &input);
        assert!(runtime.scene().entity(&id).unwrap().transform.position[1] > 0.5);
        let before = runtime.time;
        runtime.advance(200.0, &InputFrame::default());
        assert!(runtime.time - before <= f64::from(FIXED_DT) * MAX_STEPS as f64 + 1e-6);
    }
    #[test]
    fn area_hit_is_once_per_activation_and_target() {
        let mut project = Project::new("Acerto");
        let mut area = Entity::new("Área", None);
        area.collider = Some(Collider {
            is_trigger: true,
            ..Collider::default()
        });
        let event = Node::new("event.area_enter", [0.0; 2]);
        let damage = Node::new("action.damage", [200.0, 0.0]);
        connect(&mut area.graph, &event, "exec", &damage, "exec");
        connect(&mut area.graph, &event, "context", &damage, "target");
        area.graph.nodes = vec![event, damage];
        let aid = area.id.clone();
        let mut target = Entity::new("Alvo", None);
        target.collider = Some(Collider::default());
        target
            .attributes
            .insert("Vida".into(), Value::Number(100.0));
        let tid = target.id.clone();
        project.scenes[0].entities = vec![area, target];
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        for _ in 0..30 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        assert_eq!(
            runtime.scene().entity(&tid).unwrap().attributes["Vida"],
            Value::Number(90.0)
        );
        let activation = runtime.area_activations[&aid];
        runtime.emit(RuntimeEvent::AreaEnter {
            area: aid.clone(),
            other: tid.clone(),
            activation,
        });
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(
            runtime.scene().entity(&tid).unwrap().attributes["Vida"],
            Value::Number(90.0)
        );
        runtime.emit(RuntimeEvent::AreaEnter {
            area: aid,
            other: tid.clone(),
            activation: activation + 100,
        });
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(
            runtime.scene().entity(&tid).unwrap().attributes["Vida"],
            Value::Number(80.0)
        );
    }
    #[test]
    fn recording_keeps_crossings_whose_objects_logic_removes() {
        let mut project = Project::new("Gravação");
        let mut coin = Entity::new("Moeda", None);
        coin.collider = Some(Collider {
            is_trigger: true,
            ..Collider::default()
        });
        let event = Node::new("event.area_enter", [0.0; 2]);
        let remove = Node::new("action.remove", [200.0, 0.0]);
        connect(&mut coin.graph, &event, "exec", &remove, "exec");
        coin.graph.nodes = vec![event, remove];
        let mut target = Entity::new("Caixa", None);
        target.collider = Some(Collider::default());
        let (coin_id, target_id) = (coin.id.clone(), target.id.clone());
        project.scenes[0].entities = vec![coin, target];
        let mut plain = Runtime::new(&project, &project.start_scene).unwrap();
        plain.advance(FIXED_DT, &InputFrame::default());
        assert!(plain.recording.is_none(), "only hosts that ask record");
        let mut runtime = Runtime::recorded(&project, &project.start_scene).unwrap();
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert!(runtime.scene().entity(&coin_id).is_none());
        assert_eq!(
            runtime.recording.as_ref().unwrap().crossings,
            [AreaCrossing {
                area: coin_id,
                other: target_id,
                entered: true,
            }]
        );
    }
    #[test]
    fn data_type_failure_identifies_owner_and_stops_only_affected_behavior() {
        let (mut project, id) = waiting_project();
        let broken = project.scenes[0].entity_mut(&id).unwrap();
        let set = broken
            .graph
            .nodes
            .iter_mut()
            .find(|node| node.operation == "attribute.set")
            .unwrap();
        set.params
            .insert("value".into(), Value::Text("não é número".into()));
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        runtime.advance(0.2, &InputFrame::default());
        assert!(runtime.disabled_behaviors.contains(&id));
        assert!(
            runtime
                .logs
                .iter()
                .any(|line| line.contains(&id) && line.contains("incompatível"))
        );
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["contador"],
            Value::Number(0.0)
        );
    }
    #[test]
    fn task_budget_bounds_large_graph_without_blocking_fixed_steps() {
        let mut project = Project::new("Orçamento");
        let mut entity = Entity::new("Fluxo longo", None);
        let start = Node::new("event.click", [0.0; 2]);
        let mut previous = start.clone();
        entity.graph.nodes.push(start);
        for index in 0..MAX_NODE_WORK + 20 {
            let next = Node::new("debug.message", [index as f32, 0.0]);
            connect(&mut entity.graph, &previous, "exec", &next, "exec");
            previous = next.clone();
            entity.graph.nodes.push(next);
        }
        let id = entity.id.clone();
        project.scenes[0].entities.push(entity);
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        runtime.click(&id);
        runtime.advance(0.2, &InputFrame::default());
        assert_eq!(runtime.pending_tasks(), 1);
        assert!(runtime.time >= f64::from(FIXED_DT) * 7.9);
        assert!(
            runtime
                .logs
                .iter()
                .any(|line| line.contains("Limite de trabalho"))
        );
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(runtime.pending_tasks(), 0);
    }

    #[test]
    fn spawned_root_translation_animation_retains_configured_spawn_offset() {
        use crate::animation::{Clip, Interpolation, Keyframe, sample_clip};
        let mut project = Project::new("Spawn animado");
        let mut entity = Entity::new("Base", None);
        entity.transform.position = [5.0, 2.0, 3.0];
        let mut clip = Clip::new("Translação da raiz");
        clip.insert_key(
            &entity.id,
            Keyframe {
                time: 0.0,
                transform: entity.transform.clone(),
                interpolation: Interpolation::Linear,
            },
        );
        let mut end = entity.transform.clone();
        end.position = [7.0, 4.0, 3.0];
        clip.insert_key(
            &entity.id,
            Keyframe {
                time: 1.0,
                transform: end,
                interpolation: Interpolation::Linear,
            },
        );
        entity.clips.push(clip);
        let click = Node::new("event.click", [0.0; 2]);
        let mut spawn = Node::new("action.spawn", [230.0, 0.0]);
        spawn.params.insert("x".into(), Value::Number(4.0));
        spawn.params.insert("y".into(), Value::Number(3.0));
        spawn.params.insert("z".into(), Value::Number(-2.0));
        connect(&mut entity.graph, &click, "exec", &spawn, "exec");
        entity.graph.nodes = vec![click, spawn];
        let original = entity.id.clone();
        project.scenes[0].entities.push(entity);
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        runtime.click(&original);
        runtime.advance(FIXED_DT, &InputFrame::default());
        let spawned = runtime
            .scene()
            .entities
            .iter()
            .find(|entity| entity.id != original)
            .unwrap();
        assert_eq!(spawned.transform.position, [9.0, 5.0, 1.0]);
        let id = spawned.id.clone();
        let clip = spawned.clips[0].clone();
        sample_clip(runtime.scene_mut(), &clip, 1.0);
        assert_eq!(
            runtime.scene().entity(&id).unwrap().transform.position,
            [11.0, 7.0, 1.0]
        );
        assert_eq!(
            runtime
                .scene()
                .entity(&original)
                .unwrap()
                .transform
                .position,
            [5.0, 2.0, 3.0]
        );
    }
}
