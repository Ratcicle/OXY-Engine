//! Fixed-step, editor-independent execution of ordinary project documents.
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
mod characters;
mod movement_nodes;
pub use characters::SensorCrossing;
pub use movement_nodes::{SceneQuery, SceneQueryShape};

pub const FIXED_DT: f32 = 1.0 / 60.0;
pub const MAX_STEPS: usize = 8;
pub const MAX_NODE_WORK: usize = 1024;

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
}
#[derive(Clone, Debug)]
pub struct NodeTrace {
    pub object: Id,
    pub node: Id,
    pub operation: String,
    pub time: f64,
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

pub struct Runtime {
    project: Project,
    scene_id: Id,
    index: OnceLock<Result<SceneIndex, String>>,
    graphs: HashMap<Id, Arc<PreparedGraph>>,
    pub paused: bool,
    pub logs: Vec<String>,
    pub traces: Vec<NodeTrace>,
    pub sounds: Vec<SoundRequest>,
    pub time: f64,
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
}

impl Runtime {
    pub fn new(project: &Project, scene_id: &str) -> Result<Self, String> {
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
            sounds: Vec::new(),
            time: 0.0,
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
        };
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
        self.ready.clear();
        self.waiting.clear();
        self.sounds.clear();
        self.animations.clear();
        self.area_hits.clear();
        self.area_activations.clear();
        self.damage_hits.clear();
        self.overlap_pairs.clear();
        self.bodies.clear();
        self.characters = Default::default();
        self.cameras = Default::default();
    }
    pub fn click(&mut self, entity: &str) {
        if !self.paused && !self.stopped {
            self.emit(RuntimeEvent::Click(entity.into()));
        }
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
        if self.cameras.accepts_look(self.scene(), &self.characters) {
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
    fn fixed_step(&mut self, input: &InputFrame, budget: &mut usize) {
        self.step_input = input.clone();
        self.query_work = 0;
        crate::metrics::timed(|| self.begin_camera_input(input), |c, ns| c.input_ns += ns);
        self.collect_activations();
        crate::metrics::count(|c| c.steps += 1);
        self.time += f64::from(FIXED_DT);
        if self.event_used("event.step") {
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
    fn log(&mut self, message: String) {
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
                    let values = movement_nodes::event_values(&event, self.time)
                        .into_iter()
                        .map(|(key, value)| ((node.id.clone(), key.into()), value))
                        .collect();
                    found.push(Task {
                        due: self.time,
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
        for task in std::mem::take(&mut self.waiting) {
            if task.due <= self.time {
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
            "math.binary" => {
                let a = self
                    .input_value(graph, node, "a", context, budget, depth + 1)?
                    .number()
                    .ok_or("A precisa ser número")?;
                let b = self
                    .input_value(graph, node, "b", context, budget, depth + 1)?
                    .number()
                    .ok_or("B precisa ser número")?;
                let value = match node.text("operator") {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" if b != 0.0 => a / b,
                    "/" => return Err("Divisão por zero".into()),
                    _ => return Err("Operador aritmético inválido".into()),
                };
                if !value.is_finite() {
                    return Err("Resultado aritmético não finito".into());
                }
                Ok(Value::Number(value))
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
            _ => self.movement_data(graph, node, output, context, budget, depth),
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
                    due: self.time + delay,
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
                let root = self.scene_mut_internal().duplicate_subtree(&template)?;
                // The editor duplicate offset is not part of a runtime spawn's explicit offset.
                let delta = Vec3::new(
                    node.number("x", 0.0) as f32 - 0.5,
                    node.number("y", 0.0) as f32,
                    node.number("z", 0.0) as f32,
                );
                let descendants: HashSet<_> = self.scene().descendants(&root).into_iter().collect();
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
                    "collider" => {
                        let collider = self.entity_mut(&target).ok_or("Objeto ausente")?.collider.as_mut().ok_or("Objeto não tem colisor")?;
                        let activation = enabled && !collider.enabled;
                        collider.enabled = enabled;
                        if activation {
                            self.activate_area(&target);
                            self.overlap_pairs.retain(|(area, _)| area != &target);
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
                    _ => return Err("Componente desconhecido; use collider/controller/visible/behavior/animation".into()),
                }
            }
            "action.scene" => {
                self.change_scene(node.text("scene"))?;
                return Ok(());
            }
            _ => {
                if !self.movement_action(&graph, node, &mut context, budget)? {
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
        characters.remove_entities(&ids, self.scene(), self.time);
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
        self.overlap_pairs
            .retain(|(a, b)| !ids.contains(a) && !ids.contains(b));
        self.damage_hits.retain(|(a, _), hits| {
            if let Some(hits) = hits.upgrade() {
                hits.lock().unwrap().retain(|target| !ids.contains(target));
            }
            !ids.contains(a) && hits.strong_count() > 0
        });
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
        self.sounds.clear();
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
        let mut events = Vec::new();
        for (owner, player) in &mut players {
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
        // difference of dense sets when no graph consumes exit events.
        if self.event_used("event.area_exit") {
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
