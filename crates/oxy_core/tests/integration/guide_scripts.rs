//! The scripts the Guide's pages of Scripts show (`guide_scripts`): each loads without a warning,
//! names only what its project has, is attached as the person attaches it and does what its
//! page says, in a run of its own.
use oxy_core::{
    command::{self, Command},
    document::*,
    guide_scripts::{self, Block},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{self, ScriptCall, ScriptSources},
    variables,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Every code block of the pages, by its file name, as the Guide shows it.
fn shown() -> Vec<(&'static str, &'static str)> {
    guide_scripts::pages()
        .iter()
        .flat_map(|page| page.blocks)
        .filter_map(|block| match block {
            Block::Code { file, code } => Some((*file, *code)),
            _ => None,
        })
        .collect()
}

/// The scripts `files` of the pages, at the root of the project, as the Guide names them.
fn sources(files: &[&str]) -> ScriptSources {
    let shown = shown();
    ScriptSources::from_texts(files.iter().map(|file| {
        shown
            .iter()
            .copied()
            .find(|(name, _)| name == file)
            .unwrap_or_else(|| panic!("o Guia não mostra {file}"))
    }))
}

/// `path` attached to `entity` as "+ Anexar script" attaches it; the warnings.
fn attach(project: &mut Project, entity: &Id, path: &str, sources: &ScriptSources) -> Vec<String> {
    let scene = project.start_scene.clone();
    command::apply_with_scripts(
        project,
        &scene,
        &[Command::AttachScript {
            scene: None,
            entity: entity.clone(),
            path: path.into(),
            index: None,
        }],
        sources,
    )
    .unwrap()
    .into_iter()
    .flat_map(|applied| applied.warnings)
    .collect()
}

fn run(project: &Project, sources: ScriptSources) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

fn steps(runtime: &mut Runtime, n: usize) {
    for _ in 0..n {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
}

fn no_errors(runtime: &Runtime) {
    let errors: Vec<&str> = runtime.script_errors().map(|(_, text, _)| text).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// A project with one object for each name, in a 2D scene; their IDs.
fn project(names: &[&str]) -> (Project, Vec<Id>) {
    let mut project = Project::new("Guia");
    let ids = names
        .iter()
        .map(|name| {
            let entity = Entity::new(*name, Some(Primitive::Cube));
            let id = entity.id.clone();
            project.scenes[0].entities.push(entity);
            id
        })
        .collect();
    (project, ids)
}

fn attribute(runtime: &Runtime, id: &str, name: &str) -> Value {
    runtime.scene().entity(id).unwrap().attributes[name].clone()
}

#[test]
fn every_script_the_guide_shows_loads_names_only_its_project_and_has_a_test() {
    let shown = shown();
    let files: BTreeSet<&str> = shown.iter().map(|(file, _)| *file).collect();
    // A new script on a page needs its test here, as the ones below.
    assert_eq!(
        files,
        BTreeSet::from(["alavanca.luau", "contador.luau", "girar.luau", "porta.luau"])
    );
    assert_eq!(files.len(), shown.len(), "dois blocos com o mesmo arquivo");
    let all: Vec<&str> = files.iter().copied().collect();
    let sources = sources(&all);
    for report in script::check_loading(&sources, all.iter().copied(), |_| true) {
        assert!(
            report.error.is_none() && report.warnings.is_empty(),
            "{report:?}"
        );
    }
    // The names the scripts give: the game variable Recorde, the attributes they declare.
    let (mut project, _) = project(&["Área"]);
    variables::create(&mut project, "Recorde", Value::Number(0.), false).unwrap();
    let warnings = script::lint(&sources, all.iter().copied(), Some(&project));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn girar_turns_the_object_by_the_speed_it_declares() {
    let sources = sources(&["girar.luau"]);
    let (mut project, ids) = project(&["Cubo"]);
    assert!(attach(&mut project, &ids[0], "girar.luau", &sources).is_empty());
    // Attaching created the attribute the script asks for.
    let cube = project.scenes[0].entity(&ids[0]).unwrap();
    assert_eq!(cube.attributes["Velocidade"], Value::Number(90.));
    let mut runtime = run(&project, sources);
    steps(&mut runtime, 120);
    no_errors(&runtime);
    // 90 degrees a second, for two seconds of game.
    let turned = runtime.scene().entity(&ids[0]).unwrap().transform.rotation[1].to_degrees();
    assert!((turned - 180.).abs() < 3., "{turned}");
}

#[test]
fn contador_keeps_the_visits_in_an_attribute_the_record_in_a_variable_and_names_in_memory() {
    let sources = sources(&["contador.luau"]);
    let (mut project, ids) = project(&["Área", "Jogador"]);
    let record = variables::create(&mut project, "Recorde", Value::Number(1.), false).unwrap();
    // The script hears an area: its object's collider is one.
    project.scenes[0].entity_mut(&ids[0]).unwrap().collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let warnings = attach(&mut project, &ids[0], "contador.luau", &sources);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut runtime = run(&project, sources);
    steps(&mut runtime, 1);
    let player = [Value::Object(Some(ids[1].clone()))];
    for _ in 0..2 {
        assert_eq!(
            runtime.call_script(&ids[0], "contador.luau", "on_area_enter", &player),
            ScriptCall::Done
        );
    }
    no_errors(&runtime);
    assert_eq!(attribute(&runtime, &ids[0], "Visitas"), Value::Number(2.));
    assert_eq!(runtime.variable(&record, ""), Some(&Value::Number(2.)));
    let memory = runtime
        .script_memory_text(&ids[0], "contador.luau")
        .unwrap();
    assert_eq!(
        memory,
        [(
            "nomes".to_owned(),
            "{[1] = \"Jogador\", [2] = \"Jogador\"}".to_owned()
        )]
    );
}

#[test]
fn the_lever_opens_the_door_with_a_message_and_a_timer_closes_it() {
    let sources = sources(&["alavanca.luau", "porta.luau"]);
    let (mut project, ids) = project(&["Alavanca", "Porta"]);
    let (lever, door) = (&ids[0], &ids[1]);
    project.scenes[0].entity_mut(door).unwrap().collider = Some(Collider::default());
    assert!(attach(&mut project, lever, "alavanca.luau", &sources).is_empty());
    assert!(attach(&mut project, door, "porta.luau", &sources).is_empty());
    // The person picks the door in the lever's attribute Porta.
    let attributes = &mut project.scenes[0].entity_mut(lever).unwrap().attributes;
    assert_eq!(attributes["Porta"], Value::Object(None));
    attributes.insert("Porta".into(), Value::Object(Some(door.clone())));
    let mut runtime = run(&project, sources);
    let open = |runtime: &Runtime| {
        !runtime
            .scene()
            .entity(door)
            .unwrap()
            .collider
            .as_ref()
            .unwrap()
            .enabled
    };
    steps(&mut runtime, 1);
    assert_eq!(
        runtime.call_script(lever, "alavanca.luau", "on_click", &[]),
        ScriptCall::Done
    );
    // The message arrives in the next phase of the logic.
    steps(&mut runtime, 2);
    assert!(open(&runtime));
    // Three seconds of game, then the timer closes it.
    steps(&mut runtime, 170);
    assert!(open(&runtime));
    steps(&mut runtime, 20);
    assert!(!open(&runtime));
    no_errors(&runtime);
}
