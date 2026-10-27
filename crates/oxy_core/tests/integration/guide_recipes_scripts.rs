//! The guide's recipes in Luau (examples/guia-scripts, design §10 of 0.7): Porta com chave,
//! Carta e energia and Ataque por marcador, with the objects and IDs of the node recipes and
//! their logic in scripts, built through the MCP. Each plays as its node recipe does, repeats
//! itself and keeps nothing in its VM.
use oxy_core::{
    command,
    document::{Layout, Project, Value, new_id},
    guide_recipes,
    headless::{self, SimulationRequest},
    persistence,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{self, ScriptSources},
};
use serde_json::json;
use std::{path::Path, sync::Arc};

/// Each script recipe's folder and the node recipe it redoes, by its index in
/// `guide_recipes::recipes`.
const RECIPES: [(&str, usize); 3] = [
    ("02-porta-com-chave", 1),
    ("03-carta-e-energia", 2),
    ("04-ataque-por-marcador", 3),
];

fn id(n: u32) -> String {
    format!("02000000-0000-4000-8000-{n:012x}")
}
fn run(project: &Project, sources: ScriptSources) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}
fn value(runtime: &Runtime, n: u32, attribute: &str) -> Value {
    runtime.scene().entity(&id(n)).unwrap().attributes[attribute].clone()
}
fn ticks(runtime: &mut Runtime, n: usize) {
    for _ in 0..n {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
}
fn collider_on(runtime: &Runtime, n: u32) -> bool {
    let entity = runtime.scene().entity(&id(n)).unwrap();
    entity.collider.as_ref().unwrap().enabled
}

#[test]
fn script_recipes_are_projects_in_folders_with_the_objects_of_their_node_recipes() {
    let recipes = guide_recipes::script_recipes();
    assert_eq!(recipes.len(), RECIPES.len());
    for (index, (folder, nodes)) in RECIPES.into_iter().enumerate() {
        assert_eq!(recipes[index].title, guide_recipes::recipes()[nodes].title);
        let (p, sources) = guide_recipes::load_script_recipe(index).unwrap();
        assert_eq!(p.layout, Layout::Tree, "{folder}");
        // The same objects by ID, name and parent: what reads the node recipe's objects, as
        // the native guide checks do, reads these.
        let objects = |p: &Project| -> Vec<(String, String, Option<String>)> {
            p.scenes
                .iter()
                .flat_map(|s| &s.entities)
                .map(|e| (e.id.clone(), e.name.clone(), e.parent.clone()))
                .collect()
        };
        assert_eq!(objects(&p), objects(&guide_recipes::load(nodes).unwrap()));
        assert!(
            p.scenes
                .iter()
                .flat_map(|s| &s.entities)
                .all(|e| e.graph.nodes.is_empty()),
            "{folder} não tem lógica de nós"
        );
        // The scripts it carries are the files of its folder, each attached, loading without
        // warnings and naming only what the project has.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/guia-scripts")
            .join(folder);
        assert_eq!(
            persistence::load_project(&root.join(persistence::PROJECT_FILE)).unwrap(),
            p
        );
        assert_eq!(persistence::load_scripts(&root).unwrap(), sources);
        let paths: Vec<&str> = sources.files.keys().map(String::as_str).collect();
        assert_eq!(
            script::attached_paths(&p).into_iter().collect::<Vec<_>>(),
            paths
        );
        for report in script::check_loading(&sources, paths.iter().copied(), |_| true) {
            assert!(
                report.error.is_none() && report.warnings.is_empty(),
                "{report:?}"
            );
        }
        let lint = script::lint(&sources, paths.iter().copied(), Some(&p));
        assert!(lint.is_empty(), "{lint:?}");
        assert!(command::warnings(&p).is_empty(), "{folder}");
        let missing = persistence::script_warnings(&p, &root);
        assert!(missing.is_empty(), "{missing:?}");
    }
}

/// What a person does in each recipe, as oxy_simulate takes it: walk into the door's area with
/// the key and without it, click the card twice, attack three times.
fn plays() -> [(usize, serde_json::Value); 4] {
    let walk = json!([
        { "type": "press", "at": 0, "action": "mover_direita" },
        { "type": "release", "at": 2, "action": "mover_direita" }
    ]);
    let door = json!([
        { "id": "Porta", "fields": ["visible"] },
        { "id": "Personagem", "fields": ["position", "attributes"] }
    ]);
    [
        (
            0,
            json!({ "seconds": 2.5, "inputs": walk, "watch": door, "samples_at": [0.5, 1, 1.5, 2] }),
        ),
        (
            0,
            json!({
                "seconds": 2.5,
                "setup": [{
                    "op": "set_attribute",
                    "entity": "Personagem",
                    "name": "Chave",
                    "value": { "Bool": false }
                }],
                "inputs": walk,
                "watch": door,
                "samples_at": [0.5, 1, 1.5, 2]
            }),
        ),
        (
            1,
            json!({
                "seconds": 1.5,
                "inputs": [
                    { "type": "click", "at": 0.5, "entity": "Carta Raio" },
                    { "type": "click", "at": 1, "entity": "Carta Raio" }
                ],
                "watch": [
                    { "id": "Estado da partida", "fields": ["attributes"] },
                    { "id": "Alvo", "fields": ["attributes"] }
                ],
                "samples_at": [0.25, 0.75, 1.25]
            }),
        ),
        (
            2,
            json!({
                "seconds": 3.5,
                "inputs": [
                    { "type": "press", "at": 0.1, "action": "Atacar" },
                    { "type": "release", "at": 1.2, "action": "Atacar" },
                    { "type": "press", "at": 1.5, "action": "Atacar" },
                    { "type": "release", "at": 1.6, "action": "Atacar" },
                    { "type": "press", "at": 2.5, "action": "Atacar" }
                ],
                "watch": [
                    { "id": "Alvo", "fields": ["attributes"] },
                    { "id": "Braço", "fields": ["rotation"] }
                ],
                "sample_every": 0.05
            }),
        ),
    ]
}

#[test]
fn each_script_recipe_plays_as_its_node_recipe_repeats_itself_and_keeps_nothing_in_its_vm() {
    for (index, mut request) in plays() {
        request["actions"] = json!("none");
        request["check_replay"] = json!(true);
        request["check_vm_reset"] = json!(true);
        let request: SimulationRequest = serde_json::from_value(request).unwrap();
        let plain = SimulationRequest {
            check_replay: false,
            check_vm_reset: false,
            ..request.clone()
        };
        let nodes = guide_recipes::load(RECIPES[index].1).unwrap();
        let nodes = headless::simulate(&nodes, &plain).unwrap();
        let (p, sources) = guide_recipes::load_script_recipe(index).unwrap();
        let (scripted, _) = headless::run_with_scripts(&p, Arc::new(sources), &request).unwrap();
        assert!(
            scripted.script_errors.is_empty(),
            "{:?}",
            scripted.script_errors
        );
        let replay = scripted.replay.as_ref().unwrap();
        assert!(replay.identical, "{replay:?}");
        let reset = scripted.vm_reset.as_ref().unwrap();
        assert!(reset.identical, "{reset:?}");
        assert_eq!(replay.steps, (request.seconds * 60.).round() as usize);
        assert_eq!(reset.steps, replay.steps);
        assert_eq!(nodes.watched, scripted.watched);
        assert_eq!(nodes.events, scripted.events);
        assert_eq!(nodes.ui, scripted.ui);
        assert_eq!(nodes.sounds, scripted.sounds);
        assert_eq!(nodes.final_scene, scripted.final_scene);
        // A script's print says, after its object's name, what the node's message said.
        assert_eq!(nodes.logs.len(), scripted.logs.len(), "{:?}", scripted.logs);
        for (node, script) in nodes.logs.iter().zip(&scripted.logs) {
            assert_eq!(node.time, script.time);
            assert!(script.text.ends_with(&node.text), "{script:?}");
            assert!(script.script.is_some(), "{script:?}");
        }
    }
}

#[test]
fn doorway_in_luau_reads_the_entrant_key_and_keeps_invalid_visitors_out() {
    for valid in [false, true] {
        let (mut p, sources) = guide_recipes::load_script_recipe(0).unwrap();
        let e = p.scenes[0].entity_mut(&id(210)).unwrap();
        e.attributes.insert("Chave".into(), Value::Bool(valid));
        e.transform.position = [1.3, 0., 0.];
        let mut r = run(&p, sources);
        ticks(&mut r, 2);
        let door = r.scene().entity(&id(211)).unwrap();
        assert_eq!(door.collider.as_ref().unwrap().enabled, !valid);
        assert_eq!(door.visible, !valid);
        assert_eq!(value(&r, 210, "Chave"), Value::Bool(valid));
        let refused = r.logs.iter().any(|l| l.text.contains("exige uma chave"));
        assert_eq!(refused, !valid, "{:?}", r.logs);
    }
}

#[test]
fn card_in_luau_spends_only_when_the_energy_is_enough() {
    let (p, sources) = guide_recipes::load_script_recipe(1).unwrap();
    let mut r = run(&p, sources);
    r.click(&id(312));
    ticks(&mut r, 1);
    assert_eq!(value(&r, 310, "Energia"), Value::Number(1.));
    assert_eq!(value(&r, 311, "Vida"), Value::Number(30.));
    r.click(&id(312));
    ticks(&mut r, 1);
    assert_eq!(value(&r, 310, "Energia"), Value::Number(1.));
    assert_eq!(value(&r, 311, "Vida"), Value::Number(30.));
    assert!(
        r.logs
            .iter()
            .any(|l| l.text.contains("Energia insuficiente"))
    );
}

#[test]
fn attack_in_luau_opens_and_closes_one_activation_per_marker() {
    let (p, sources) = guide_recipes::load_script_recipe(2).unwrap();
    let mut r = run(&p, sources);
    let input = InputFrame {
        pressed: [id(402)].into(),
        ..Default::default()
    };
    for life in [85., 70., 55.] {
        r.advance(FIXED_DT, &input);
        ticks(&mut r, 5);
        assert!(!collider_on(&r, 413));
        ticks(&mut r, 5);
        assert!(collider_on(&r, 413));
        ticks(&mut r, 5);
        assert_eq!(value(&r, 414, "Vida"), Value::Number(life));
        ticks(&mut r, 35);
        // The timer closed the area, and the target that stayed in it was hit once.
        assert_eq!(value(&r, 414, "Vida"), Value::Number(life));
        assert!(!collider_on(&r, 413));
        assert_eq!(r.pending_tasks(), 0);
        assert_eq!(r.retained_counts()[0], 0);
    }
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}

#[test]
fn a_copy_of_a_script_recipe_opens_with_its_scripts_as_the_guide_writes_it() {
    for index in 0..RECIPES.len() {
        let folder = std::env::temp_dir().join(format!("oxy-guide-scripts-{}", new_id()));
        let file = guide_recipes::write_script_recipe(index, &folder).unwrap();
        assert_eq!(file, folder.join(persistence::PROJECT_FILE));
        let (p, sources) = guide_recipes::load_script_recipe(index).unwrap();
        let (opened, report) = persistence::open_project(&file).unwrap();
        assert_eq!(opened, p);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            persistence::load_scripts_to_run(&opened, &folder).unwrap(),
            sources
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
