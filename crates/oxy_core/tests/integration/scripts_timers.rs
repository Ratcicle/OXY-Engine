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
    // In game time, from when they were asked (time 0): the first item of a sequence on the
    // next step, each step of a schedule after the one before. Calls of one step go in the
    // order their timers were queued.
    assert_eq!(
        attribute(&rt, "relogio", "Registro"),
        Value::Text(
            "s1@1 s2@7 p1@12 s3@13 sempre@15 depois@30 nil@30 sempre@30 p3@33 sempre@45 \
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
        "s1@1 s2@7 p1@12 s3@13 sempre@15 depois@30 nil@30 sempre@30 p3@33 sempre@45 sempre@60 \
         sempre@75 true "
    );
    assert!(rt.has_script_host() && rt.script_errors().count() == 0);
}
