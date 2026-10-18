//! The Luau host seen from outside the crate: scripts read from the project's folder run on the
//! objects that attach them, beside their graph, and a project whose objects attach no script
//! runs without a VM and reaches the same state with or without sources.
use oxy_core::{
    document::*,
    graph::{Edge, Node},
    persistence,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions, ScriptLocation},
    script::{ScriptCall, ScriptRef, ScriptSources},
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-scripts-host-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// An object whose graph adds one to its `Passos` at each step.
fn counter(name: &str) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes.insert("Passos".into(), Value::Number(0.));
    let mut node = |operation: &str, params: &[(&str, &str)]| {
        let mut node = Node::new(operation, [0.; 2]);
        for (key, value) in params {
            node.params
                .insert((*key).into(), Value::Text((*value).into()));
        }
        let id = node.id.clone();
        e.graph.nodes.push(node);
        id
    };
    let step = node("event.step", &[]);
    let read = node("attribute.get", &[("attribute", "Passos")]);
    let add = node("math.binary", &[("operator", "+")]);
    let set = node("attribute.set", &[("attribute", "Passos")]);
    for (from, from_port, to, to_port) in [
        (&step, "exec", &set, "exec"),
        (&read, "value", &add, "a"),
        (&add, "value", &set, "value"),
    ] {
        e.graph.edges.push(Edge {
            from_node: from.clone(),
            from_port: from_port.into(),
            to_node: to.clone(),
            to_port: to_port.into(),
        });
    }
    e
}

fn run(project: &Project, scripts: ScriptSources) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(scripts),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

#[test]
fn scripts_from_the_project_folder_run_beside_the_graph() {
    let root = folder();
    write(
        &root,
        "scripts/porta.luau",
        "local valores = require('./dados/valores')\nlocal M = {}\nfunction M.abrir(self)\n    \
         self.attr.Passos += valores.passo\n    print(self.name, self.attr.Passos)\nend\nreturn M\n",
    );
    write(
        &root,
        "scripts/dados/valores.luau",
        "return { passo = 10 }\n",
    );
    let sources = persistence::load_scripts(&root).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    let mut project = Project::new("Porta");
    let mut door = counter("Porta");
    door.scripts = vec![ScriptRef::new("scripts/porta.luau")];
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    let mut rt = run(&project, sources);
    assert!(rt.has_script_host());
    for _ in 0..3 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        rt.call_script(&id, "scripts/porta.luau", "abrir", &[]),
        ScriptCall::Done
    );
    assert_eq!(
        rt.scene().entity(&id).unwrap().attributes["Passos"],
        Value::Number(13.)
    );
    let line = rt.logs.last().unwrap();
    assert_eq!(line.text, "Porta · Porta 13");
    assert_eq!(
        line.script,
        Some(ScriptLocation {
            path: "scripts/porta.luau".into(),
            line: 5
        })
    );
    assert_eq!(rt.script_errors().count(), 0);
    // The graph goes on after the script.
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        rt.scene().entity(&id).unwrap().attributes["Passos"],
        Value::Number(14.)
    );
}

#[test]
fn a_project_without_scripts_runs_without_a_vm_and_reaches_the_same_state() {
    let mut project = Project::new("Contador");
    project.scenes[0].entities.push(counter("Contador"));
    let hash = |scripts: ScriptSources| {
        let mut rt = run(&project, scripts);
        for _ in 0..30 {
            rt.advance(FIXED_DT, &InputFrame::default());
        }
        assert!(!rt.has_script_host());
        rt.state_hash()
    };
    assert_eq!(
        hash(ScriptSources::default()),
        hash(ScriptSources::from_texts([(
            "scripts/solto.luau",
            "return {}"
        )]))
    );
}
