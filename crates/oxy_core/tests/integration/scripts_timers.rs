//! Timers and sequences of scripts are data of the engine (decision Q1 of 0.7): tasks that name
//! a function of the module, in game time, which the game pause holds, an object's removal and
//! a scene change drop, and a replay repeats.
use oxy_core::{
    document::*,
    graph::{Edge, Node},
    headless::{self, SimulationRequest},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{ScriptCall, ScriptRef, ScriptSources},
};
use serde_json::json;
use std::sync::Arc;

const CLOCK: &str = r#"local M = {}
function M.on_start(self)
    self:after(0.5, M.mark, "depois")
    self.attr.Repetir = self:every(0.25, M.mark, "sempre")
    self:sequence(0.1, M.mark, { "s1", "s2", "s3" })
    self:schedule({ { 0.2, M.mark, "p1" }, { 0.3, M.mark }, { 0.05, M.mark, "p3" } })
end
function M.on_step(self)
    self.attr.Passo += 1
end
function M.mark(self, label)
    self.attr.Registro ..= string.format("%s@%d ", tostring(label), self.attr.Passo)
end
function M.stop(self)
    self.attr.Registro ..= tostring(self:cancel(self.attr.Repetir)) .. " "
end
return M
"#;

fn clock_object() -> Entity {
    let mut e = Entity::new("Relógio", None);
    e.id = "relogio".into();
    for (name, value) in [
        ("Passo", Value::Number(0.)),
        ("Repetir", Value::Number(0.)),
        ("Registro", Value::Text(String::new())),
    ] {
        e.attributes.insert(name.into(), value);
    }
    e.scripts = vec![ScriptRef::new("scripts/relogio.luau")];
    e
}

fn run(project: &Project, files: &[(&str, &str)]) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(ScriptSources::from_texts(files.iter().copied())),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

fn step(rt: &mut Runtime, steps: usize) {
    for _ in 0..steps {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
}

fn attribute(rt: &Runtime, id: &str, name: &str) -> Value {
    rt.scene().entity(id).unwrap().attributes[name].clone()
}

#[test]
fn timers_sequences_and_schedules_fire_at_their_steps_until_cancelled() {
    let mut project = Project::new("Relógio");
    project.scenes[0].entities.push(clock_object());
    let mut rt = run(&project, &[("scripts/relogio.luau", CLOCK)]);
    assert_eq!(
        rt.script_timers(),
        [
            ("relogio", "scripts/relogio.luau", 1),
            ("relogio", "scripts/relogio.luau", 2),
            ("relogio", "scripts/relogio.luau", 3),
            ("relogio", "scripts/relogio.luau", 4),
        ]
    );
    step(&mut rt, 60);
    // In game time, from when they were asked (time 0): item k of a sequence (k-1) × 0.1 s
    // later, as self:after would call it, so the first on the next step; each step of a
    // schedule after the one before. Calls of one step go in the order their timers were
    // queued.
    assert_eq!(
        attribute(&rt, "relogio", "Registro"),
        Value::Text(
            "s1@1 s2@6 p1@12 s3@12 sempre@15 depois@30 nil@30 sempre@30 p3@33 sempre@45 \
             sempre@60 "
                .into()
        )
    );
    assert_eq!(rt.script_timers(), [("relogio", "scripts/relogio.luau", 2)]);
    assert_eq!(
        rt.call_script("relogio", "scripts/relogio.luau", "stop", &[]),
        ScriptCall::Done
    );
    assert!(rt.script_timers().is_empty());
    step(&mut rt, 30);
    assert_eq!(
        rt.call_script("relogio", "scripts/relogio.luau", "stop", &[]),
        ScriptCall::Done
    );
    let Value::Text(log) = attribute(&rt, "relogio", "Registro") else {
        panic!()
    };
    assert!(log.ends_with("sempre@60 true false "), "{log}");
    assert_eq!(rt.script_errors().count(), 0);
}

const BELL: &str = r#"local M = {}
function M.on_start(self)
    self:after(1, M.ring)
end
function M.ring(self)
    self.attr.Toques += 1
end
return M
"#;

fn bell() -> (Project, Id) {
    let mut project = Project::new("Sino");
    let mut e = Entity::new("Sino", None);
    e.attributes.insert("Toques".into(), Value::Number(0.));
    e.scripts = vec![ScriptRef::new("scripts/sino.luau")];
    let id = e.id.clone();
    project.scenes[0].entities.push(e);
    (project, id)
}

#[test]
fn the_game_pause_holds_timers_and_removal_and_scene_changes_drop_them() {
    let (project, id) = bell();
    let files = [("scripts/sino.luau", BELL)];
    let rings = |rt: &Runtime| attribute(rt, &id, "Toques");
    // Paused for a second halfway, the bell rings a second later.
    let mut rt = run(&project, &files);
    step(&mut rt, 30);
    assert!(rt.set_game_paused(true));
    step(&mut rt, 60);
    assert!(rt.set_game_paused(false));
    step(&mut rt, 29);
    assert_eq!(rings(&rt), Value::Number(0.));
    step(&mut rt, 1);
    assert_eq!(rings(&rt), Value::Number(1.));
    assert!(rt.script_timers().is_empty());
    // Restarting the scene drops the timer it had; the new start asks for its own.
    let mut rt = run(&project, &files);
    step(&mut rt, 30);
    let scene = rt.scene_id().to_owned();
    rt.change_scene(&scene).unwrap();
    assert!(rt.script_timers().is_empty());
    step(&mut rt, 1);
    assert_eq!(rt.script_timers().len(), 1);
    step(&mut rt, 59);
    assert_eq!(rings(&rt), Value::Number(0.));
    step(&mut rt, 1);
    assert_eq!(rings(&rt), Value::Number(1.));
    // A removed object takes its timers with it.
    let mut rt = run(&project, &files);
    step(&mut rt, 10);
    rt.remove_object(&id);
    assert!(rt.script_timers().is_empty());
    step(&mut rt, 60);
    assert_eq!(rt.script_errors().count(), 0);
}

/// A level whose objects run scripts with timers, on_step and on_input, beside a graph that
/// waits and restarts the scene on "reiniciar"; Esc pauses it.
fn level() -> Project {
    let mut project = Project::new("Replay");
    for (action, key) in [("parar", "P"), ("reiniciar", "R")] {
        project.input_bindings.insert(action.into(), key.into());
    }
    let scene_id = project.scenes[0].id.clone();
    let mut clock = clock_object();
    clock.scripts.push(ScriptRef::new("scripts/controle.luau"));
    clock.attributes.insert("Pausas".into(), Value::Number(0.));
    let mut restart = Node::new("event.input", [0.; 2]);
    restart
        .params
        .insert("action".into(), Value::Text("reiniciar".into()));
    let mut reload = Node::new("action.scene", [0.; 2]);
    reload
        .params
        .insert("scene".into(), Value::Text(scene_id.clone()));
    clock.graph.edges.push(Edge {
        from_node: restart.id.clone(),
        from_port: "exec".into(),
        to_node: reload.id.clone(),
        to_port: "exec".into(),
    });
    clock.graph.nodes = vec![restart, reload];
    project.scenes[0].entities.push(clock);
    for i in 0..4 {
        let mut e = Entity::new(format!("Contador {i}"), None);
        e.attributes.insert("Passo".into(), Value::Number(0.));
        e.attributes
            .insert("Registro".into(), Value::Text(String::new()));
        e.attributes.insert("Repetir".into(), Value::Number(0.));
        e.scripts = vec![ScriptRef::new("scripts/relogio.luau")];
        project.scenes[0].entities.push(e);
    }
    project
}

const CONTROL: &str = r#"local M = {}
function M.on_input(self, action, phase)
    if action == "parar" and phase == "pressed" then
        self.attr.Registro ..= tostring(self:cancel(self.attr.Repetir)) .. " "
    end
end
function M.on_pause(self, paused)
    self.attr.Pausas += 1
end
return M
"#;

#[test]
fn a_replay_with_scripts_stays_identical_and_runs_repeat() {
    let project = level();
    let scripts = Arc::new(ScriptSources::from_texts([
        ("scripts/relogio.luau", CLOCK),
        ("scripts/controle.luau", CONTROL),
    ]));
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 4,
        "check_replay": true,
        "actions": "none",
        "ui": false,
        "inputs": [
            { "type": "press", "at": 0.8, "action": "parar" },
            { "type": "release", "at": 0.9, "action": "parar" },
            { "type": "press", "at": 2.0, "action": "reiniciar" },
            { "type": "release", "at": 2.1, "action": "reiniciar" },
            { "type": "escape", "at": 2.4 },
            { "type": "escape", "at": 2.7 },
            { "type": "press", "at": 3.6, "action": "parar" }
        ]
    }))
    .unwrap();
    let simulate = || headless::run_with_scripts(&project, scripts.clone(), &request).unwrap();
    let (report, rt) = simulate();
    let replay = report.replay.as_ref().unwrap();
    assert!(replay.identical, "{replay:?}");
    assert_eq!(replay.steps, 240);
    assert_eq!(report.state_hash, simulate().0.state_hash);
    // The run did what the scripts say: the restart dropped the timers with the scene and its
    // start asked for them again, the pause after it reached on_pause twice and held the
    // timers and on_step alike, and the last press cancelled the new repetition.
    assert_eq!(attribute(&rt, "relogio", "Pausas"), Value::Number(2.));
    let Value::Text(log) = attribute(&rt, "relogio", "Registro") else {
        panic!()
    };
    assert_eq!(
        log,
        "s1@1 s2@6 p1@12 s3@12 sempre@15 depois@30 nil@30 sempre@30 p3@33 sempre@45 sempre@60 \
         sempre@75 true "
    );
    assert!(rt.has_script_host() && rt.script_errors().count() == 0);
}

fn start_node(e: &mut Entity) -> Id {
    let node = Node::new("event.scene_start", [0.; 2]);
    let id = node.id.clone();
    e.graph.nodes.push(node);
    id
}

/// `attribute.set` of `Aviso` to `n` on the object running the graph.
fn show_node(e: &mut Entity, n: f64) -> Id {
    let mut node = Node::new("attribute.set", [0.; 2]);
    node.params
        .insert("attribute".into(), Value::Text("Aviso".into()));
    node.params.insert("value".into(), Value::Number(n));
    let id = node.id.clone();
    e.graph.nodes.push(node);
    id
}

fn wait_node(e: &mut Entity, seconds: f64) -> Id {
    let mut node = Node::new("control.wait", [0.; 2]);
    node.params.insert("seconds".into(), Value::Number(seconds));
    let id = node.id.clone();
    e.graph.nodes.push(node);
    id
}

/// Links `nodes` one after the other.
fn chain(e: &mut Entity, nodes: &[&Id]) {
    for pair in nodes.windows(2) {
        e.graph.edges.push(Edge {
            from_node: pair[0].clone(),
            from_port: "exec".into(),
            to_node: pair[1].clone(),
            to_port: "exec".into(),
        });
    }
}

fn counter(name: &str) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes.insert("Aviso".into(), Value::Number(-1.));
    e
}

const COUNTDOWN: &str = r#"local M = {}
function M.on_start(self)
    self:sequence(1, M.show, { 3, 2, 1, 0 })
end
function M.show(self, n)
    self.attr.Aviso = n
end
return M
"#;

/// The countdown of the parkour ported from its graph: the 3 right at the start, as the graph
/// sets it, then the sequence (whose first item repeats it on the next step) and the end.
const START_COUNTDOWN: &str = r#"local M = {}
function M.on_start(self)
    M.show(self, 3)
    self:sequence(1, M.show, { 3, 2, 1, 0 })
    self:after(4, M.show, -1)
end
function M.show(self, n)
    self.attr.Aviso = n
end
return M
"#;

/// A 3-2-1 countdown with self:sequence keeps the steps of the waits of a graph on the same
/// project: item k waits (k-1) s from the call, the first as Esperar sem bloquear with 0 s
/// waits (the next step). The countdown of the parkour, which sets the 3 at the start and then
/// waits a second between items, ports to a script step for step; a replay repeats it all.
#[test]
fn a_sequence_keeps_the_steps_of_the_waits_of_an_equivalent_graph() {
    let mut project = Project::new("Contagem");
    // As the sequence: Esperar 0 s → 3; and from the start, 1 s → 2 → 1 s → 1 → 1 s → 0.
    let mut graph = counter("Grafo");
    let start = start_node(&mut graph);
    let (first, three) = (wait_node(&mut graph, 0.), show_node(&mut graph, 3.));
    chain(&mut graph, &[&start, &first, &three]);
    let mut waits = vec![start];
    for n in [2., 1., 0.] {
        waits.push(wait_node(&mut graph, 1.));
        waits.push(show_node(&mut graph, n));
    }
    chain(&mut graph, &waits.iter().collect::<Vec<_>>());
    let mut script = counter("Script");
    script.scripts = vec![ScriptRef::new("scripts/contagem.luau")];
    // The parkour's: the 3 at the start, then 1 s between items and 1 s more to clear it.
    let mut parkour = counter("Grafo do parkour");
    let mut flow = vec![start_node(&mut parkour)];
    flow.push(show_node(&mut parkour, 3.));
    for n in [2., 1., 0., -1.] {
        flow.push(wait_node(&mut parkour, 1.));
        flow.push(show_node(&mut parkour, n));
    }
    chain(&mut parkour, &flow.iter().collect::<Vec<_>>());
    let mut ported = counter("Script do parkour");
    ported.scripts = vec![ScriptRef::new("scripts/largada.luau")];
    let ids: Vec<Id> = [&graph, &script, &parkour, &ported]
        .iter()
        .map(|e| e.id.clone())
        .collect();
    project.scenes[0]
        .entities
        .extend([graph, script, parkour, ported]);
    let files = [
        ("scripts/contagem.luau", COUNTDOWN),
        ("scripts/largada.luau", START_COUNTDOWN),
    ];
    let mut rt = run(&project, &files);
    // Each object's value after the start and after each step, and the steps it changed at.
    let mut seen: Vec<Vec<Value>> = vec![Vec::new(); 4];
    for n in 0..=250 {
        if n > 0 {
            step(&mut rt, 1);
        }
        for (values, id) in seen.iter_mut().zip(&ids) {
            values.push(attribute(&rt, id, "Aviso"));
        }
    }
    let changes = |values: &[Value]| -> Vec<(usize, Value)> {
        let mut last = Value::Number(-1.);
        let mut found = Vec::new();
        for (n, value) in values.iter().enumerate() {
            if *value != last {
                found.push((n, value.clone()));
                last = value.clone();
            }
        }
        found
    };
    assert_eq!(seen[1], seen[0], "the sequence and its graph");
    assert_eq!(
        changes(&seen[1]),
        [1, 60, 120, 180]
            .into_iter()
            .zip([3., 2., 1., 0.].map(Value::Number))
            .collect::<Vec<_>>()
    );
    assert_eq!(seen[3], seen[2], "the parkour's countdown and its port");
    assert_eq!(
        changes(&seen[3]),
        [0, 60, 120, 180, 240]
            .into_iter()
            .zip([3., 2., 1., 0., -1.].map(Value::Number))
            .collect::<Vec<_>>()
    );
    assert_eq!(rt.script_errors().count(), 0);
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 5, "check_replay": true, "actions": "none", "ui": false
    }))
    .unwrap();
    let scripts = Arc::new(ScriptSources::from_texts(files));
    let (report, rt) = headless::run_with_scripts(&project, scripts, &request).unwrap();
    let replay = report.replay.as_ref().unwrap();
    assert!(replay.identical, "{replay:?}");
    assert_eq!(replay.steps, 300);
    for (id, values) in ids.iter().zip(&seen) {
        assert_eq!(attribute(&rt, id, "Aviso"), values[250], "{id}");
    }
}
