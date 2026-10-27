//! The Ligado of the editor's Scripts card: a script an object attaches can be switched off on
//! it, which keeps it in the object's list (and in the project file, as `"enabled": false`)
//! while the game runs the object without it; set_script_enabled switches it with undo.
use oxy_core::{
    command::{self, Command},
    document::*,
    edit_history::CommandHistory,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{self, ScriptCall, ScriptRef, ScriptSources},
    texture_cache::TextureCache,
};
use std::sync::Arc;

const COUNT: &str = "scripts/conta.luau";
const COUNTER: &str = r#"local M = {}
function M.on_start(self) self.attr.Vezes += 1 end
function M.on_step(self, dt) self.attr.Vezes += 1 end
return M
"#;

fn counter(name: &str, enabled: bool) -> Entity {
    let mut e = Entity::new(name, None);
    e.attributes.insert("Vezes".into(), Value::Number(0.));
    e.scripts = vec![ScriptRef {
        enabled,
        ..ScriptRef::new(COUNT)
    }];
    e
}

fn run(project: &Project, files: &[(&str, &str)]) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(ScriptSources::from_texts(files.iter().copied())),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

fn times(rt: &Runtime, id: &str) -> Value {
    rt.scene().entity(id).unwrap().attributes["Vezes"].clone()
}

fn switch(entity: &str, path: &str, enabled: bool) -> Command {
    Command::SetScriptEnabled {
        scene: None,
        entity: entity.into(),
        path: path.into(),
        enabled,
    }
}

#[test]
fn the_project_file_writes_only_a_script_switched_off() {
    let on = counter("Ligado", true);
    let off = counter("Desligado", false);
    let json = |e: &Entity| serde_json::to_value(e).unwrap()["scripts"].clone();
    assert_eq!(json(&on), serde_json::json!([{"path": COUNT}]));
    assert_eq!(
        json(&off),
        serde_json::json!([{"path": COUNT, "enabled": false}])
    );
    // A reference without the field, as every project before it, is on.
    let read: ScriptRef = serde_json::from_value(serde_json::json!({"path": COUNT})).unwrap();
    assert!(read.enabled);
    let read: ScriptRef =
        serde_json::from_value(serde_json::json!({"path": COUNT, "enabled": false})).unwrap();
    assert!(!read.enabled);
}

#[test]
fn a_script_switched_off_does_not_run_on_that_object() {
    let mut project = Project::new("Desligar");
    let on = counter("Ligado", true);
    let off = counter("Desligado", false);
    let (on_id, off_id) = (on.id.clone(), off.id.clone());
    project.scenes[0].entities.extend([on, off]);
    let mut rt = run(&project, &[(COUNT, COUNTER)]);
    for _ in 0..3 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(times(&rt, &on_id), Value::Number(4.));
    assert_eq!(times(&rt, &off_id), Value::Number(0.));
    // The run has no such script on that object; the document keeps it, in its place.
    assert!(rt.scene().entity(&off_id).unwrap().scripts.is_empty());
    assert!(!project.scenes[0].entity(&off_id).unwrap().scripts[0].enabled);
    // Switched off everywhere, the script never loads: the scene runs without a VM.
    project.scenes[0].entities[0].scripts[0].enabled = false;
    let mut rt = run(&project, &[(COUNT, COUNTER)]);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert!(!rt.has_script_host());
    assert_eq!(times(&rt, &on_id), Value::Number(0.));
}

#[test]
fn a_model_copy_runs_without_the_scripts_switched_off_in_the_model() {
    let mut project = Project::new("Modelo");
    let scene = project.start_scene.clone();
    let bullet = counter("Bala", false);
    let bullet_id = bullet.id.clone();
    project.scenes[0].entities.push(bullet);
    project.save_model(&scene, &bullet_id, "Bala").unwrap();
    project.scenes[0].entities.clear();
    let mut spawner = Entity::new("Lançador", None);
    spawner.scripts = vec![ScriptRef::new("scripts/lancar.luau")];
    let spawner_id = spawner.id.clone();
    project.scenes[0].entities.push(spawner);
    let launch = "local M = {}\nfunction M.fire(self) scene.spawn_model(\"Bala\") end\nreturn M\n";
    let mut rt = run(
        &project,
        &[(COUNT, COUNTER), ("scripts/lancar.luau", launch)],
    );
    assert_eq!(
        rt.call_script(&spawner_id, "scripts/lancar.luau", "fire", &[]),
        ScriptCall::Done
    );
    rt.advance(FIXED_DT, &InputFrame::default());
    let copy = rt
        .scene()
        .entities
        .iter()
        .find(|e| e.name == "Bala")
        .expect("the copy");
    assert_eq!(copy.attributes["Vezes"], Value::Number(0.));
    assert!(copy.scripts.is_empty());
}

#[test]
fn set_script_enabled_switches_with_undo() {
    let mut project = Project::new("Comando");
    let door = counter("Porta", true);
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    let scene = project.start_scene.clone();
    let before = project.clone();
    let mut images = TextureCache::default();
    let mut history = CommandHistory::new();
    // The object by its name, as agents give it.
    let commands = [switch("porta", COUNT, false)];
    history.begin(commands[0].label(), &project, &images);
    command::apply(&mut project, &scene, &commands).unwrap();
    history.commit(&project, &mut images).unwrap();
    assert!(!project.scenes[0].entity(&id).unwrap().scripts[0].enabled);
    assert_eq!(history.undo_label(), Some("Desligar script"));
    let off = project.clone();
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);
    history.redo(&mut project, &mut images).unwrap();
    assert_eq!(project, off);
    command::apply(&mut project, &scene, &[switch(&id, COUNT, true)]).unwrap();
    assert_eq!(project, before);
    assert_eq!(switch(&id, COUNT, true).label(), "Ligar script");
    // Only a script the object attaches.
    let error = command::apply(
        &mut project,
        &scene,
        &[switch(&id, "scripts/outro.luau", false)],
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Comando 0 (set_script_enabled, o 1º do lote): Porta não tem o script \
         scripts/outro.luau. Nada foi alterado."
    );
    assert_eq!(project, before);
    // As the agents write it.
    let parsed = command::parse(&serde_json::json!([
        {"op": "set_script_enabled", "entity": "Porta", "path": COUNT, "enabled": false}
    ]))
    .unwrap();
    assert_eq!(parsed, [switch("Porta", COUNT, false)]);
}

#[test]
fn the_errors_of_a_run_say_their_object_script_and_line() {
    let mut project = Project::new("Erros");
    let mut door = Entity::new("Porta", None);
    door.scripts = vec![ScriptRef::new("scripts/porta.luau")];
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    let mut other = Entity::new("Janela", None);
    other.scripts = vec![ScriptRef::new("scripts/quebrado.luau")];
    project.scenes[0].entities.push(other);
    let rt = run(
        &project,
        &[
            (
                "scripts/porta.luau",
                "local M = {}\nfunction M.on_start(self)\n    error('quebrou')\nend\nreturn M\n",
            ),
            ("scripts/quebrado.luau", "local M = {\nreturn M\n"),
        ],
    );
    let errors = rt.script_error_counts();
    let lines: Vec<(&str, &str, u32)> = errors
        .iter()
        .filter_map(|e| Some((e.object.as_str(), e.path.as_deref()?, e.line)))
        .collect();
    assert!(
        lines.contains(&(id.as_str(), "scripts/porta.luau", 3)),
        "{lines:?}"
    );
    // A script that does not load fails on no object in particular.
    assert!(
        lines.iter().any(|(object, path, line)| object.is_empty()
            && *path == "scripts/quebrado.luau"
            && *line > 0),
        "{lines:?}"
    );
    let text = &errors
        .iter()
        .find(|e| e.path.as_deref() == Some("scripts/porta.luau"))
        .unwrap()
        .text;
    assert!(text.contains("quebrou"), "{text}");
    assert_eq!(script::attached_by(&project, "scripts/porta.luau"), 1);
    assert_eq!(script::attached_by(&project, "scripts/nada.luau"), 0);
}
