//! Fixed-step, editor-independent execution of ordinary project documents.
use crate::{
    animation::AnimationPlayer,
    audio::SoundRequest,
    collision::{Aabb, move_and_slide},
    document::{Id, Project, Scene, SceneKind, Value, validate_project},
    graph::{Graph, Node},
};
use glam::Vec3;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

pub const FIXED_DT: f32 = 1.0 / 60.0;
pub const MAX_STEPS: usize = 8;
pub const MAX_NODE_WORK: usize = 1024;

#[derive(Clone, Debug, Default)]
pub struct InputFrame {
    pub held: BTreeSet<String>,
    pub pressed: BTreeSet<String>,
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
    SceneStart,
    Input(String),
    Click(Id),
    AreaEnter {
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
    outputs: BTreeMap<(Id, String), Value>,
}
#[derive(Clone, Debug)]
struct Task {
    due: f64,
    owner: Id,
    node: Id,
    context: Context,
}
#[derive(Clone, Copy, Debug, Default)]
struct BodyState {
    velocity: Vec3,
    grounded: bool,
}

pub struct Runtime {
    pub project: Project,
    pub scene_id: Id,
    pub paused: bool,
    pub logs: Vec<String>,
    pub traces: Vec<NodeTrace>,
    pub sounds: Vec<SoundRequest>,
    pub time: f64,
    source: Project,
    accumulator: f32,
    pending_pressed: BTreeSet<String>,
    ready: VecDeque<Task>,
    waiting: Vec<Task>,
    bodies: HashMap<Id, BodyState>,
    animations: HashMap<Id, AnimationPlayer>,
    disabled_behaviors: HashSet<Id>,
    overlap_pairs: HashSet<(Id, Id)>,
    area_activations: HashMap<Id, u64>,
    damage_hits: HashSet<(Id, u64, Id)>,
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
            ready: VecDeque::new(),
            waiting: Vec::new(),
            bodies: HashMap::new(),
            animations: HashMap::new(),
            disabled_behaviors: HashSet::new(),
            overlap_pairs: HashSet::new(),
            area_activations: HashMap::new(),
            damage_hits: HashSet::new(),
            serial: 0,
            stopped: false,
        };
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
    pub fn scene_mut(&mut self) -> &mut Scene {
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
            self.damage_hits.len(),
            self.area_activations.len(),
            self.ready.len(),
            self.waiting.len(),
        ]
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.accumulator = 0.0;
        self.pending_pressed.clear();
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.set_paused(true);
        self.ready.clear();
        self.waiting.clear();
        self.sounds.clear();
        self.animations.clear();
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
            return;
        }
        if !elapsed.is_finite() || elapsed < 0.0 {
            return;
        }
        self.pending_pressed.extend(input.pressed.iter().cloned());
        self.accumulator = (self.accumulator + elapsed.min(0.25)).min(FIXED_DT * MAX_STEPS as f32);
        let mut steps = 0;
        let mut budget = MAX_NODE_WORK;
        while self.accumulator + f32::EPSILON >= FIXED_DT && steps < MAX_STEPS {
            let frame = InputFrame {
                held: input.held.clone(),
                pressed: std::mem::take(&mut self.pending_pressed),
            };
            self.fixed_step(&frame, &mut budget);
            self.accumulator = (self.accumulator - FIXED_DT).max(0.0);
            steps += 1;
        }
    }
    fn fixed_step(&mut self, input: &InputFrame, budget: &mut usize) {
        crate::metrics::count(|c| c.steps += 1);
        self.time += f64::from(FIXED_DT);
        for action in &input.pressed {
            self.emit(RuntimeEvent::Input(action.clone()));
        }
        crate::metrics::timed(|| self.process_tasks(budget), |c, ns| c.tasks_ns += ns);
        crate::metrics::timed(|| self.move_controllers(input), |c, ns| c.movement_ns += ns);
        crate::metrics::timed(|| self.advance_animations(), |c, ns| c.animation_ns += ns);
        crate::metrics::timed(|| self.detect_areas(), |c, ns| c.areas_ns += ns);
        crate::metrics::timed(|| self.process_tasks(budget), |c, ns| c.tasks_ns += ns);
    }
    fn log(&mut self, message: String) {
        self.logs.push(message);
        if self.logs.len() > 500 {
            self.logs.drain(..100);
        }
    }
    fn next_serial(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }
    pub fn emit(&mut self, event: RuntimeEvent) {
        if self.stopped {
            return;
        }
        let generated_activation = self.next_serial();
        let mut found = Vec::new();
        for entity in &self.scene().entities {
            if self.disabled_behaviors.contains(&entity.id) {
                continue;
            }
            for node in &entity.graph.nodes {
                let matches = match &event {
                    RuntimeEvent::SceneStart => node.operation == "event.scene_start",
                    RuntimeEvent::Input(action) => {
                        node.operation == "event.input" && node.text("action") == action
                    }
                    RuntimeEvent::Click(id) => entity.id == *id && node.operation == "event.click",
                    RuntimeEvent::AreaEnter { area, .. } => {
                        entity.id == *area && node.operation == "event.area_enter"
                    }
                    RuntimeEvent::Animation { object, marker } => {
                        entity.id == *object
                            && node.operation == "event.animation"
                            && node.text("marker") == marker
                    }
                };
                if matches {
                    let (other, activation) = match &event {
                        RuntimeEvent::AreaEnter {
                            other, activation, ..
                        } => (Some(other.clone()), *activation),
                        RuntimeEvent::Click(id) => (Some(id.clone()), generated_activation),
                        RuntimeEvent::Animation { object, .. } => {
                            (Some(object.clone()), generated_activation)
                        }
                        _ => (Some(entity.id.clone()), generated_activation),
                    };
                    found.push(Task {
                        due: self.time,
                        owner: entity.id.clone(),
                        node: node.id.clone(),
                        context: Context {
                            owner: entity.id.clone(),
                            other,
                            activation,
                            outputs: BTreeMap::new(),
                        },
                    });
                }
            }
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
            if self.disabled_behaviors.contains(&task.owner)
                || self.scene().entity(&task.owner).is_none()
            {
                continue;
            }
            let result = self.execute(task.clone(), budget);
            if let Err(error) = result {
                self.log(format!(
                    "Objeto {} / nó {}: {error}. Comportamento interrompido.",
                    task.owner, task.node
                ));
                self.disabled_behaviors.insert(task.owner.clone());
                self.ready.retain(|queued| queued.owner != task.owner);
                self.waiting.retain(|queued| queued.owner != task.owner);
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
        graph: &Graph,
        node: &Node,
        input: &str,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Value, String> {
        if depth > 64 {
            return Err("Profundidade de dados excede 64 nós".into());
        }
        if let Some(edge) = graph
            .edges
            .iter()
            .find(|edge| edge.to_node == node.id && edge.to_port == input)
        {
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
        graph: &Graph,
        node: &Node,
        context: &Context,
        budget: &mut usize,
        depth: usize,
    ) -> Result<Id, String> {
        match self.input_value(graph, node, "target", context, budget, depth)? {
            Value::Object(value) => Ok(value.unwrap_or_else(|| context.owner.clone())),
            _ => Err("Entrada objeto precisa de referência de objeto".into()),
        }
    }
    fn output_value(
        &self,
        graph: &Graph,
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
            "value.number" | "value.text" | "value.bool" | "value.object" => node
                .params
                .get("value")
                .cloned()
                .ok_or("Constante sem valor".into()),
            "attribute.get" => {
                let target = self.target(graph, node, context, budget, depth + 1)?;
                self.scene()
                    .entity(&target)
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
            _ => Err(format!(
                "Dados da saída {output} indisponíveis; execute a ação produtora antes"
            )),
        }
    }
    fn outputs(
        &mut self,
        graph: &Graph,
        node: &Node,
        ports: &[&str],
        context: Context,
        delay: f64,
    ) {
        let mut tasks = Vec::new();
        for port in ports {
            for edge in graph
                .edges
                .iter()
                .filter(|edge| edge.from_node == node.id && edge.from_port == *port)
            {
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
        let graph = self
            .scene()
            .entity(&task.owner)
            .ok_or("Objeto removido")?
            .graph
            .clone();
        let node = graph.node(&task.node).ok_or("Nó não encontrado")?.clone();
        crate::metrics::count(|c| c.graph_nodes_copied += graph.nodes.len() as u64);
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
                    .input_value(&graph, &node, "condition", &context, budget, 0)?
                    .boolean()
                    .ok_or("Condição precisa ser booleana")?;
                ports = vec![if condition { "then" } else { "else" }];
            }
            "control.sequence" => ports = vec!["first", "second", "third"],
            "control.wait" => {
                delay = self
                    .input_value(&graph, &node, "seconds", &context, budget, 0)?
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
                let target = self.target(&graph, &node, &context, budget, 0)?;
                let value = self.input_value(&graph, &node, "value", &context, budget, 0)?;
                if let Value::Object(Some(id)) = &value
                    && self.scene().entity(id).is_none()
                {
                    return Err("Nova referência de atributo não existe".into());
                }
                let entity = self
                    .scene_mut()
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
                let target = self.target(&graph, &node, &context, budget, 0)?;
                let amount = self
                    .input_value(&graph, &node, "amount", &context, budget, 0)?
                    .number()
                    .ok_or("Dano precisa ser numérico")?;
                if !amount.is_finite() || amount < 0.0 {
                    return Err("Dano deve ser um número não negativo".into());
                }
                let key = (context.owner.clone(), context.activation, target.clone());
                if !node.boolean("once", true) || !self.damage_hits.contains(&key) {
                    let entity = self
                        .scene_mut()
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
                    self.damage_hits.insert(key);
                }
            }
            "action.animation" => {
                let target = self.target(&graph, &node, &context, budget, 0)?;
                let entity = self
                    .scene()
                    .entity(&target)
                    .ok_or("Objeto animado ausente")?;
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
                let template = self.target(&graph, &node, &context, budget, 0)?;
                let root = self.scene_mut().duplicate_subtree(&template)?;
                // The editor duplicate offset is not part of a runtime spawn's explicit offset.
                let delta = Vec3::new(
                    node.number("x", 0.0) as f32 - 0.5,
                    node.number("y", 0.0) as f32,
                    node.number("z", 0.0) as f32,
                );
                let descendants: HashSet<_> = self.scene().descendants(&root).into_iter().collect();
                let entity = self.scene_mut().entity_mut(&root).unwrap();
                entity.transform.position =
                    (Vec3::from(entity.transform.position) + delta).to_array();
                for entity in self
                    .scene_mut()
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
                context.outputs.insert(
                    (node.id.clone(), "created".into()),
                    Value::Object(Some(root)),
                );
            }
            "action.remove" => {
                let target = self.target(&graph, &node, &context, budget, 0)?;
                self.remove_object(&target);
                if self.scene().entity(&context.owner).is_none() {
                    return Ok(());
                }
            }
            "action.component" => {
                let target = self.target(&graph, &node, &context, budget, 0)?;
                let enabled = node.boolean("enabled", true);
                match node.text("component") {
                    "collider" => {
                        let collider = self.scene_mut().entity_mut(&target).ok_or("Objeto ausente")?.collider.as_mut().ok_or("Objeto não tem colisor")?;
                        let activation = enabled && !collider.enabled;
                        collider.enabled = enabled;
                        if activation {
                            let serial = self.next_serial(); self.area_activations.insert(target.clone(), serial);
                            self.overlap_pairs.retain(|(area, _)| area != &target);
                            self.damage_hits.retain(|(owner, _, _)| owner != &target);
                        }
                    }
                    "controller" => self.scene_mut().entity_mut(&target).ok_or("Objeto ausente")?.controller.as_mut().ok_or("Objeto não tem controlador")?.enabled = enabled,
                    "visible" => self.scene_mut().entity_mut(&target).ok_or("Objeto ausente")?.visible = enabled,
                    "behavior" => {
                        if self.scene().entity(&target).is_none() { return Err("Objeto ausente".into()); }
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
            _ => return Err(format!("Operação {} não é executável", node.operation)),
        }
        self.outputs(&graph, &node, &ports, context, delay);
        Ok(())
    }
    pub fn remove_object(&mut self, id: &str) {
        let ids: HashSet<_> = self.scene().descendants(id).into_iter().collect();
        self.scene_mut().remove_subtree(id);
        self.ready.retain(|task| !ids.contains(&task.owner));
        self.waiting.retain(|task| !ids.contains(&task.owner));
        self.bodies.retain(|owner, _| !ids.contains(owner));
        self.animations.retain(|owner, _| !ids.contains(owner));
        self.overlap_pairs
            .retain(|(a, b)| !ids.contains(a) && !ids.contains(b));
        self.damage_hits
            .retain(|(a, _, b)| !ids.contains(a) && !ids.contains(b));
    }
    pub fn change_scene(&mut self, id: &str) -> Result<(), String> {
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
        self.ready.clear();
        self.waiting.clear();
        self.bodies.clear();
        self.animations.clear();
        self.disabled_behaviors.clear();
        self.overlap_pairs.clear();
        self.area_activations.clear();
        self.damage_hits.clear();
        self.pending_pressed.clear();
        self.sounds.clear();
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
        let dimensions = if self.scene().kind == SceneKind::TwoD {
            2
        } else {
            3
        };
        for (id, controller) in controllers {
            let mut body_state = self.bodies.get(&id).copied().unwrap_or_default();
            let horizontal =
                i32::from(input.held("mover_direita")) - i32::from(input.held("mover_esquerda"));
            body_state.velocity.x = horizontal as f32 * controller.speed;
            body_state.velocity.z = if dimensions == 3 {
                (i32::from(input.held("mover_tras")) - i32::from(input.held("mover_frente"))) as f32
                    * controller.speed
            } else {
                0.0
            };
            body_state.velocity.y -= controller.gravity * FIXED_DT;
            if input.pressed("pular") && body_state.grounded {
                body_state.velocity.y = controller.jump;
                body_state.grounded = false;
            }
            let displacement = if let Some(body) = self.collider_box(&id) {
                let obstacles: Vec<_> = self
                    .scene()
                    .entities
                    .iter()
                    .filter(|entity| entity.id != id && !is_related(self.scene(), &id, &entity.id))
                    .filter(|entity| {
                        entity
                            .collider
                            .as_ref()
                            .is_some_and(|collider| collider.enabled && !collider.is_trigger)
                    })
                    .filter_map(|entity| self.collider_box(&entity.id))
                    .inspect(|_| crate::metrics::count(|c| c.candidates += 1))
                    .collect();
                let result =
                    move_and_slide(body, body_state.velocity, FIXED_DT, &obstacles, dimensions);
                body_state.velocity = result.velocity;
                body_state.grounded = result.grounded;
                result.delta
            } else {
                body_state.velocity * FIXED_DT
            };
            let local_delta = self
                .scene()
                .entity(&id)
                .and_then(|entity| entity.parent.as_deref())
                .and_then(|parent| self.scene().world_matrix(parent).ok())
                .map_or(displacement, |matrix| {
                    matrix.inverse().transform_vector3(displacement)
                });
            if let Some(entity) = self.scene_mut().entity_mut(&id) {
                entity.transform.position =
                    (Vec3::from(entity.transform.position) + local_delta).to_array();
            }
            self.bodies.insert(id, body_state);
        }
    }
    fn advance_animations(&mut self) {
        let mut players = std::mem::take(&mut self.animations);
        let mut events = Vec::new();
        for (owner, player) in &mut players {
            let clip = self
                .scene()
                .entity(owner)
                .and_then(|entity| entity.clips.iter().find(|clip| clip.id == player.clip_id))
                .cloned();
            if let Some(clip) = clip {
                for marker in player.advance(&clip, FIXED_DT) {
                    events.push(RuntimeEvent::Animation {
                        object: owner.clone(),
                        marker,
                    });
                }
                player.sample(self.scene_mut(), &clip);
            }
        }
        self.animations = players;
        for event in events {
            self.emit(event);
        }
    }
    fn detect_areas(&mut self) {
        let dimensions = if self.scene().kind == SceneKind::TwoD {
            2
        } else {
            3
        };
        let colliders: Vec<_> = self
            .scene()
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
                    self.collider_box(&entity.id)?,
                ))
            })
            .collect();
        let mut pairs = HashSet::new();
        let mut events = Vec::new();
        for (area, is_trigger, volume) in &colliders {
            if !is_trigger {
                continue;
            }
            for (other, _, target) in &colliders {
                crate::metrics::count(|c| c.candidates += 1);
                if area == other
                    || is_related(self.scene(), area, other)
                    || !volume.overlaps(*target, dimensions)
                {
                    continue;
                }
                let pair = (area.clone(), other.clone());
                crate::metrics::count(|c| c.overlaps += 1);
                if !self.overlap_pairs.contains(&pair) {
                    let activation = if let Some(activation) = self.area_activations.get(area) {
                        *activation
                    } else {
                        let serial = self.next_serial();
                        self.area_activations.insert(area.clone(), serial);
                        serial
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

fn is_related(scene: &Scene, a: &str, b: &str) -> bool {
    scene.descendants(a).iter().any(|id| id == b) || scene.descendants(b).iter().any(|id| id == a)
}

#[cfg(test)]
mod tests {
    use super::*;
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
