//! Scripts hear events through callbacks with fixed names, as tasks of the runtime: on each
//! object's turn in scene order, after its graph and in the order of its list of scripts. A
//! script adds its tasks and nothing else to a graph project's state.
use oxy_core::{
    animation::{AnimationEvent, Clip},
    character::{CharacterConfig, MovementEvent},
    document::*,
    graph::{Edge, Node},
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{ScriptRef, ScriptSources},
};
use std::sync::Arc;

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

fn node(e: &mut Entity, operation: &str, params: &[(&str, Value)]) -> Id {
    let mut node = Node::new(operation, [0.; 2]);
    for (key, value) in params {
        node.params.insert((*key).into(), value.clone());
    }
    let id = node.id.clone();
    e.graph.nodes.push(node);
    id
}

fn link(e: &mut Entity, from: &Id, to: &Id) {
    e.graph.edges.push(Edge {
        from_node: from.clone(),
        from_port: "exec".into(),
        to_node: to.clone(),
        to_port: "exec".into(),
    });
}

fn text(text: &str) -> Value {
    Value::Text(text.into())
}

fn attach(e: &mut Entity, paths: &[&str]) {
    e.scripts = paths.iter().copied().map(ScriptRef::new).collect();
}

/// The log lines written since `from`.
fn lines_since(rt: &Runtime, from: u64) -> Vec<String> {
    let new = (rt.logs_written() - from) as usize;
    rt.logs[rt.logs.len() - new..]
        .iter()
        .map(|l| l.text.clone())
        .collect()
}

fn attribute(rt: &Runtime, id: &str, name: &str) -> Value {
    rt.scene().entity(id).unwrap().attributes[name].clone()
}

#[test]
fn each_object_runs_its_graph_then_its_scripts_in_scene_order() {
    let mut project = Project::new("Ordem");
    let mut ids = Vec::new();
    for name in ["A", "B"] {
        let mut e = Entity::new(name, None);
        let start = node(&mut e, "event.scene_start", &[]);
        let started = node(&mut e, "debug.message", &[("message", text("início"))]);
        link(&mut e, &start, &started);
        let tick = node(&mut e, "event.step", &[]);
        let ticked = node(&mut e, "debug.message", &[("message", text("passo"))]);
        link(&mut e, &tick, &ticked);
        attach(&mut e, &["scripts/um.luau", "scripts/dois.luau"]);
        ids.push((e.id.clone(), started, ticked));
        project.scenes[0].entities.push(e);
    }
    let script = |name: &str| {
        format!(
            "local M = {{}}\nfunction M.on_start(self) print('{name} início') end\n\
             function M.on_step(self) print('{name} passo') end\nreturn M\n"
        )
    };
    let (one, two) = (script("um"), script("dois"));
    let mut rt = run(
        &project,
        &[("scripts/um.luau", &one), ("scripts/dois.luau", &two)],
    );
    let expected = |what: &str, graph_node: fn(&(Id, Id, Id)) -> &Id| -> Vec<String> {
        ids.iter()
            .zip(["A", "B"])
            .flat_map(|(ids, name)| {
                [
                    format!("[{} / {}] {what}", ids.0, graph_node(ids)),
                    format!("{name} · um {what}"),
                    format!("{name} · dois {what}"),
                ]
            })
            .collect()
    };
    assert_eq!(lines_since(&rt, 0), expected("início", |ids| &ids.1));
    let from = rt.logs_written();
    step(&mut rt, 1);
    assert_eq!(lines_since(&rt, from), expected("passo", |ids| &ids.2));
}

const LISTENER: &str = r#"local M = {}
function M.on_start(self) print("start") end
function M.on_step(self, dt)
    self.attr.Passos += 1
    self.attr.Dt = dt
end
function M.on_input(self, action, phase) print("input", action, phase) end
function M.on_click(self) print("click") end
function M.on_pause(self, paused) print("pause", paused) end
function M.on_marker(self, marker, clip) print("marker", marker, clip) end
return M
"#;

const AREA: &str = r#"local M = {}
function M.on_area_enter(self, other) print("enter", other.name) end
function M.on_area_exit(self, other) print("exit", other.name) end
return M
"#;

#[test]
fn every_callback_hears_its_event() {
    let mut project = Project::new("Eventos");
    project.scenes[0].kind = SceneKind::TwoD;
    let mut listener = Entity::new("Ouvinte", None);
    attach(&mut listener, &["scripts/ouvinte.luau"]);
    listener
        .attributes
        .insert("Passos".into(), Value::Number(0.));
    listener.attributes.insert("Dt".into(), Value::Number(0.));
    let mut clip = Clip::new("Golpe");
    clip.events.push(AnimationEvent {
        time: 0.,
        name: "Impacto".into(),
    });
    let clip_id = clip.id.clone();
    listener.clips.push(clip);
    let mut area = Entity::new("Área", None);
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    attach(&mut area, &["scripts/area.luau"]);
    let mut crate_ = Entity::new("Caixa", None);
    crate_.collider = Some(Collider::default());
    let (listener_id, crate_id) = (listener.id.clone(), crate_.id.clone());
    project.scenes[0].entities.extend([listener, area, crate_]);
    let mut rt = run(
        &project,
        &[
            ("scripts/ouvinte.luau", LISTENER),
            ("scripts/area.luau", AREA),
        ],
    );
    assert_eq!(lines_since(&rt, 0), ["Ouvinte · start"]);
    // A script that hears on_pause gives the scene a pause menu, as the node does.
    assert!(rt.has_pause_menu());
    let from = rt.logs_written();
    rt.play_animation(&listener_id, &clip_id, true).unwrap();
    step(&mut rt, 1);
    assert_eq!(attribute(&rt, &listener_id, "Passos"), Value::Number(1.));
    assert_eq!(
        attribute(&rt, &listener_id, "Dt"),
        Value::Number(f64::from(FIXED_DT))
    );
    assert_eq!(
        lines_since(&rt, from),
        ["Ouvinte · marker Impacto Golpe", "Área · enter Caixa"]
    );
    let from = rt.logs_written();
    let press = InputFrame {
        pressed: ["Pular".into()].into(),
        held: ["Pular".into()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &press);
    // Holding is read in on_step; only the press and the release are events.
    let held = InputFrame {
        held: ["Pular".into()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &held);
    let release = InputFrame {
        released: ["Pular".into()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &release);
    rt.click(&listener_id);
    rt.scene_mut()
        .entity_mut(&crate_id)
        .unwrap()
        .transform
        .position = [10., 0., 0.];
    step(&mut rt, 1);
    assert_eq!(
        lines_since(&rt, from),
        [
            "Ouvinte · input Pular pressed",
            "Ouvinte · input Pular released",
            "Ouvinte · click",
            "Área · exit Caixa",
        ]
    );
    let from = rt.logs_written();
    assert!(rt.set_game_paused(true));
    step(&mut rt, 3);
    assert!(rt.set_game_paused(false));
    step(&mut rt, 1);
    assert_eq!(
        lines_since(&rt, from),
        ["Ouvinte · pause true", "Ouvinte · pause false"]
    );
    // The game pause held on_step back: 5 steps before it and 1 after.
    assert_eq!(attribute(&rt, &listener_id, "Passos"), Value::Number(6.));
    assert_eq!(rt.script_errors().count(), 0);
}

#[test]
fn a_script_another_one_required_first_still_hears_its_events_where_attached() {
    let mut project = Project::new("Require");
    let mut panel = Entity::new("Painel", None);
    attach(&mut panel, &["scripts/painel.luau"]);
    let mut manager = Entity::new("Gerente", None);
    manager
        .attributes
        .insert("Passos".into(), Value::Number(0.));
    attach(&mut manager, &["scripts/corrida.luau"]);
    let mut broken = Entity::new("Quebrado", None);
    attach(&mut broken, &["scripts/quebrado.luau"]);
    let manager_id = manager.id.clone();
    // The panel comes first in the scene, so its top requires the manager's script (and the
    // broken one, under pcall) before the scene loads them as attached.
    project.scenes[0].entities.extend([panel, manager, broken]);
    let panel_source = "local Corrida = require('./corrida')\nlocal ok = \
                        pcall(require, './quebrado')\nlocal M = {}\nfunction M.on_start(self) \
                        print('painel', Corrida.NOME, ok) end\nreturn M\n";
    let race = "local M = {}\nM.NOME = 'corrida'\nfunction M.on_start(self) print('gerente') \
                end\nfunction M.on_step(self) self.attr.Passos += 1 end\nreturn M\n";
    let mut rt = run(
        &project,
        &[
            ("scripts/painel.luau", panel_source),
            ("scripts/corrida.luau", race),
            (
                "scripts/quebrado.luau",
                "local M = {}\nM.x = nada.y\nreturn M\n",
            ),
        ],
    );
    let lines = lines_since(&rt, 0);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].starts_with("scripts/quebrado.luau:2: ")
            && lines[0].ends_with("Os objetos com este script ficam sem ele."),
        "{lines:?}"
    );
    assert_eq!(
        lines[1..],
        ["Painel · painel corrida false", "Gerente · gerente"]
    );
    step(&mut rt, 3);
    assert_eq!(attribute(&rt, &manager_id, "Passos"), Value::Number(3.));
    assert_eq!(rt.script_errors().count(), 1);
}

#[test]
fn characters_tell_their_own_scripts_how_they_move() {
    let mut project = Project::new("Personagem");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.dimensions = [40., 1., 40.];
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    let mut body = Entity::new("Corredor", None);
    body.character3d = Some(CharacterConfig::default());
    body.transform.position[1] = 3.;
    body.attributes.insert("Impacto".into(), Value::Number(0.));
    attach(&mut body, &["scripts/corpo.luau"]);
    let mut watcher = Entity::new("Observador", None);
    watcher
        .attributes
        .insert("Impacto".into(), Value::Number(0.));
    attach(&mut watcher, &["scripts/corpo.luau"]);
    let body_id = body.id.clone();
    scene.entities.extend([floor, body, watcher]);
    let source = "local M = {}\nfunction M.on_character(self, event)\n    print(event.kind, \
                  event.grounded)\n    if event.kind == 'land' then self.attr.Impacto = \
                  event.impact end\nend\nreturn M\n";
    let mut rt = run(&project, &[("scripts/corpo.luau", source)]);
    let mut landed = false;
    for _ in 0..120 {
        step(&mut rt, 1);
        if rt
            .movement_records()
            .iter()
            .any(|r| matches!(r.event, MovementEvent::Landed { .. }))
        {
            landed = true;
            break;
        }
    }
    assert!(landed);
    let impact = attribute(&rt, &body_id, "Impacto").number().unwrap();
    assert!(impact > 1., "{impact}");
    let from = rt.logs_written();
    rt.request_jump(&body_id).unwrap();
    step(&mut rt, 2);
    let lines = lines_since(&rt, from);
    assert!(
        lines.iter().any(|l| l.starts_with("Corredor · jump ")),
        "{lines:?}"
    );
    // Only the character's own scripts hear it.
    let all: Vec<&str> = rt.logs.iter().map(|l| l.text.as_str()).collect();
    assert!(all.iter().all(|l| l.starts_with("Corredor · ")), "{all:?}");
    assert!(all.contains(&"Corredor · land true"), "{all:?}");
    assert!(!rt.has_pause_menu() && rt.script_errors().count() == 0);
}

#[test]
fn copies_hear_on_start_once_the_queue_reaches_them() {
    let mut project = Project::new("Cópias");
    let mut mould = Entity::new("Molde", None);
    attach(&mut mould, &["scripts/molde.luau"]);
    let mut factory = Entity::new("Fábrica", None);
    let start = node(&mut factory, "event.scene_start", &[]);
    let copy = node(
        &mut factory,
        "action.spawn",
        &[("target", Value::Object(Some(mould.id.clone())))],
    );
    let after = node(
        &mut factory,
        "debug.message",
        &[("message", text("copiou"))],
    );
    link(&mut factory, &start, &copy);
    link(&mut factory, &copy, &after);
    let factory_id = factory.id.clone();
    project.scenes[0].entities.extend([factory, mould]);
    let source = "local M = {}\nfunction M.on_start(self) print('nasceu') end\nreturn M\n";
    let rt = run(&project, &[("scripts/molde.luau", source)]);
    let lines = lines_since(&rt, 0);
    // The factory's flow goes on first; then the Molde's own start, then the copy's.
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].starts_with(&format!("[{factory_id} / ")) && lines[0].ends_with("copiou"));
    assert_eq!(lines[1..], ["Molde · nasceu", "Molde (cópia) · nasceu"]);
}

#[test]
fn a_graph_that_stops_leaves_the_objects_scripts_running() {
    let mut project = Project::new("Misto");
    let mut broken = Entity::new("Quebrado", None);
    broken.attributes.insert("Passos".into(), Value::Number(0.));
    let tick = node(&mut broken, "event.step", &[]);
    let fail = node(
        &mut broken,
        "attribute.set",
        &[("attribute", text("Nada")), ("value", Value::Number(1.))],
    );
    link(&mut broken, &tick, &fail);
    attach(&mut broken, &["scripts/conta.luau"]);
    // Turning the graph off once the script's timer waits leaves the scripts and their timers.
    let mut off = Entity::new("Desligado", None);
    off.attributes.insert("Passos".into(), Value::Number(0.));
    let start = node(&mut off, "event.scene_start", &[]);
    let wait = node(
        &mut off,
        "control.wait",
        &[("seconds", Value::Number(0.05))],
    );
    let stop = node(
        &mut off,
        "action.component",
        &[
            ("component", text("behavior")),
            ("enabled", Value::Bool(false)),
        ],
    );
    link(&mut off, &start, &wait);
    link(&mut off, &wait, &stop);
    attach(&mut off, &["scripts/conta.luau"]);
    let (broken_id, off_id) = (broken.id.clone(), off.id.clone());
    project.scenes[0].entities.extend([broken, off]);
    let source = "local M = {}\nfunction M.on_start(self) self:every(0.1, M.tick) end\n\
                  function M.on_step(self) self.attr.Passos += 1 end\nfunction M.tick(self) \
                  self.attr.Passos += 100 end\nreturn M\n";
    let mut rt = run(&project, &[("scripts/conta.luau", source)]);
    step(&mut rt, 60);
    assert!(
        rt.logs
            .iter()
            .any(|l| l.text.ends_with("Comportamento interrompido.")),
        "{:?}",
        rt.logs
    );
    for id in [&broken_id, &off_id] {
        assert_eq!(attribute(&rt, id, "Passos"), Value::Number(60. + 1000.));
    }
}

#[test]
fn a_script_adds_nothing_but_its_document_to_a_graph_projects_state() {
    let mut project = Project::new("Contador");
    let mut counter = Entity::new("Contador", None);
    counter
        .attributes
        .insert("Passos".into(), Value::Number(0.));
    let tick = node(&mut counter, "event.step", &[]);
    let wait = node(
        &mut counter,
        "control.wait",
        &[("seconds", Value::Number(0.2))],
    );
    let message = node(&mut counter, "debug.message", &[("message", text("tique"))]);
    link(&mut counter, &tick, &wait);
    link(&mut counter, &wait, &message);
    project.scenes[0].entities.push(counter);
    let mut scripted = project.clone();
    attach(
        &mut scripted.scenes[0].entities[0],
        &["scripts/quieto.luau"],
    );
    let source = "local M = {}\nfunction M.on_start(self) local _ = self.attr.Passos end\n\
                  function M.on_step(self, dt) local _ = self.attr.Passos + dt end\nreturn M\n";
    let digest = |with_script: bool| {
        let project = if with_script { &scripted } else { &project };
        let mut rt = run(project, &[("scripts/quieto.luau", source)]);
        assert_eq!(rt.has_script_host(), with_script);
        step(&mut rt, 45);
        rt.state_digest()
            .parts
            .into_iter()
            .filter(|p| p.name != "document")
            .collect::<Vec<_>>()
    };
    assert_eq!(digest(true), digest(false));
}
