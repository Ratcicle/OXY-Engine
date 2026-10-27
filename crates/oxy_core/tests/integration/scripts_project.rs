//! What the tools tell of scripts beyond their files (stage 5h): the names of the project a
//! script gives counted as uses and warned of when a batch renames or deletes them, the
//! attributes attach_script creates and the callbacks that never run on an object, clear_logic,
//! the scene tree and objects with their scripts, the errors a run tells, the scripts' status,
//! the names a conversion changed, the scripts' API in the catalog and unknown options.
use oxy_core::{
    command,
    conversion::{self, Who},
    document::*,
    editing,
    headless::{self, SimulationRequest},
    persistence::{self, KnownFiles, PROJECT_FILE},
    query,
    script::{self, Citation, NameKind, ScriptRef, ScriptSources},
};
use serde_json::{Value as Json, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A folder of its own under the system's temporary folder, removed by the test.
fn folder(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-scripts-projeto-{label}-{}", new_id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

/// A mono 16-bit PCM WAV of a few silent samples.
fn wav() -> Vec<u8> {
    let data = 16_u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(8000_u32.to_le_bytes());
    bytes.extend(16000_u32.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(vec![0_u8; data as usize]);
    bytes
}

fn commands(value: Json) -> Vec<command::Command> {
    command::parse(&value).unwrap()
}

/// A script that gives a sound, a game variable, an input action and a scene by name.
const NAMES: &str = r#"local M = {}
function M.on_start(self)
    audio.play("Checkpoint")
    print(vars.get("Contagem"))
    if input.pressed("Pular") then
        scene.change("Pista")
    end
end
return M
"#;

/// A project with the sound Checkpoint (at `path`), the game variable Contagem, the input action
/// Pular and the scene Pista, which nothing but a script names.
fn named_project(path: &str, layout: Layout) -> Project {
    let mut project = editing::blank_project("Nomes", SceneKind::ThreeD).unwrap();
    project.layout = layout;
    project.scenes[0].name = "Pista".into();
    project.assets.push(Asset {
        id: new_id(),
        name: "Checkpoint".into(),
        path: path.into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let scene = project.start_scene.clone();
    command::apply(
        &mut project,
        &scene,
        &commands(json!([
            {"op": "create_variable", "name": "Contagem", "value": 0},
            {"op": "create_input_action", "name": "Pular", "key": "Space"},
        ])),
    )
    .unwrap();
    project
}

#[test]
fn the_names_scripts_give_count_as_uses_of_the_sound_and_the_variable() {
    let root = folder("usos");
    put(&root, "Pista/Checkpoint.wav", &wav());
    put(&root, "Pista/nomes.luau", NAMES.as_bytes());
    let project = named_project("Pista/Checkpoint.wav", Layout::Tree);
    persistence::save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
    let summary = query::summary_in(&project, Some(&root), &json!({}), &|_| false).unwrap();
    let entry = |list: &str, name: &str| {
        summary[list]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        entry("assets", "Checkpoint")["used_by"],
        json!([{"script": "Pista/nomes.luau", "line": 3, "role": "audio.play"}])
    );
    assert_eq!(
        entry("variables", "Contagem")["used_by"],
        json!([{"script": "Pista/nomes.luau", "line": 4, "role": "vars.get"}])
    );
    assert_eq!(
        entry("input_actions", "Pular")["used_by"],
        json!([{"script": "Pista/nomes.luau", "line": 5, "role": "input.pressed"}])
    );
    // The Library's list counts them too.
    let pista = query::summary_in(&project, Some(&root), &json!({"folder": "Pista"}), &|_| {
        false
    })
    .unwrap();
    let sound = pista["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Checkpoint")
        .unwrap();
    assert_eq!(sound["used_by_count"], 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn renaming_or_deleting_what_a_script_names_warns_with_the_line() {
    let project = named_project("assets/checkpoint.wav", Layout::Legacy);
    let scene = project.start_scene.clone();
    let sources = ScriptSources::from_texts([("scripts/nomes.luau", NAMES)]);
    let warnings = |batch: Json| -> Vec<String> {
        let mut project = project.clone();
        command::apply_with_scripts(&mut project, &scene, &commands(batch), &sources)
            .unwrap()
            .into_iter()
            .flat_map(|applied| applied.warnings)
            .collect()
    };
    assert_eq!(
        warnings(json!([{"op": "rename_asset", "asset": "Checkpoint", "name": "Bandeira"}])),
        [
            "Comando 0 (rename_asset, o 1º do lote): scripts/nomes.luau:3: audio.play: o som \
          \"Checkpoint\" agora se chama Bandeira; atualize o script, que no jogo para com erro \
          nessa linha"
        ]
    );
    assert_eq!(
        warnings(json!([{"op": "delete_variable", "variable": "Contagem"}])),
        [
            "Comando 0 (delete_variable, o 1º do lote): scripts/nomes.luau:4: vars.get: a variável \
          do jogo \"Contagem\" não existe mais no projeto; atualize o script, que no jogo para \
          com erro nessa linha"
        ]
    );
    let renamed = warnings(json!([
        {"op": "rename_input_action", "action": "Pular", "name": "Saltar"},
        {"op": "rename_scene", "scene": "Pista", "name": "Fase 1"},
        {"op": "update_variable", "variable": "Contagem", "value": 3},
    ]));
    assert_eq!(renamed.len(), 2, "{renamed:?}");
    assert!(
        renamed[0].starts_with("Comando 0 (rename_input_action, o 1º do lote): scripts/nomes.luau:5: input.pressed: a ação de entrada \"Pular\" agora se chama Saltar"),
        "{renamed:?}"
    );
    assert!(
        renamed[1].starts_with("Comando 1 (rename_scene, o 2º do lote): scripts/nomes.luau:6: scene.change: a cena \"Pista\" agora se chama Fase 1"),
        "{renamed:?}"
    );
    // A rename back within the batch, or one only the case changes, takes nothing away.
    assert!(
        warnings(json!([
            {"op": "rename_asset", "asset": "Checkpoint", "name": "Bandeira"},
            {"op": "rename_asset", "asset": "Bandeira", "name": "Checkpoint"},
        ]))
        .is_empty()
    );
    assert!(
        warnings(json!([{"op": "rename_scene", "scene": "Pista", "name": "PISTA"}])).is_empty()
    );
    // Each warning goes with the command that took the name, not with the batch's last command
    // of its kind.
    let mut two = project.clone();
    two.assets.push(Asset {
        id: new_id(),
        name: "Clique".into(),
        path: "assets/clique.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let told: Vec<String> = command::apply_with_scripts(
        &mut two,
        &scene,
        &commands(json!([
            {"op": "rename_asset", "asset": "Checkpoint", "name": "Bandeira"},
            {"op": "rename_asset", "asset": "Clique", "name": "Toque"},
        ])),
        &sources,
    )
    .unwrap()
    .into_iter()
    .flat_map(|applied| applied.warnings)
    .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].starts_with(
            "Comando 0 (rename_asset, o 1º do lote): scripts/nomes.luau:3: audio.play: o som \
             \"Checkpoint\" agora se chama Bandeira"
        ),
        "{told:?}"
    );
    assert!(command::renames_script_names(&commands(json!([
        {"op": "delete_asset", "asset": "Checkpoint"}
    ]))));
    assert!(!command::renames_script_names(&commands(json!([
        {"op": "update_variable", "variable": "Contagem", "value": 3}
    ]))));

    // Renamed, the text may find another sound with a name alike: said so.
    let mut twin = project.clone();
    let sound = twin.assets[0].id.clone();
    twin.assets.push(Asset {
        id: new_id(),
        name: "checkpoint".into(),
        path: "assets/outro.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let told: Vec<String> = command::apply_with_scripts(
        &mut twin,
        &scene,
        &commands(json!([{"op": "rename_asset", "asset": sound, "name": "Bandeira"}])),
        &sources,
    )
    .unwrap()
    .into_iter()
    .flat_map(|applied| applied.warnings)
    .collect();
    assert_eq!(
        told,
        [
            "Comando 0 (rename_asset, o 1º do lote): scripts/nomes.luau:3: audio.play: o som \
          \"Checkpoint\" agora se chama Bandeira; o texto passou a achar outro com nome \
          parecido; atualize o script"
        ]
    );

    // On files, the batch reads the project's scripts to tell it.
    let root = folder("renomear");
    put(&root, "assets/checkpoint.wav", &wav());
    put(&root, "scripts/nomes.luau", NAMES.as_bytes());
    let mut saved = project.clone();
    persistence::save_project(&root, &saved).unwrap();
    let done = command::apply_on_files(
        &mut saved,
        &scene,
        &commands(json!([{"op": "delete_asset", "asset": "Checkpoint"}])),
        &root,
        &mut KnownFiles::default(),
        "Agente: Excluir recurso",
    )
    .unwrap();
    let told = done.report["warnings"].to_string();
    assert!(
        told.contains(
            "scripts/nomes.luau:3: audio.play: o som \\\"Checkpoint\\\" não existe mais no projeto"
        ),
        "{told}"
    );
    assert!(saved.assets.iter().all(|asset| asset.name != "Checkpoint"));
    fs::remove_dir_all(root).unwrap();
}

/// A script that declares Ordem and Aviso and hears areas and clicks.
const AREA: &str = r#"local M = {}
M.attributes = { Ordem = 1, Aviso = "" }
M.state = { vezes = 0 }
function M.on_area_enter(self, other) end
function M.on_area_exit(self, other) end
function M.on_click(self) end
return M
"#;

#[test]
fn attach_script_tells_the_attributes_it_created_and_the_callbacks_that_never_run() {
    let mut project = editing::blank_project("Anexar", SceneKind::ThreeD).unwrap();
    let scene = project.start_scene.clone();
    let mut control = Entity::new("Controle", None);
    control
        .attributes
        .insert("Aviso".into(), Value::Text("oi".into()));
    let cube = Entity::new("Caixa", Some(Primitive::Cube));
    project.scenes[0].entities.extend([control, cube]);
    let sources = ScriptSources::from_texts([("Pista/area.luau", AREA)]);
    let report = command::apply_and_report_with_scripts(
        &mut project,
        &scene,
        &commands(json!([
            {"op": "attach_script", "entity": "Controle", "path": "Pista/area.luau"},
        ])),
        &sources,
    )
    .unwrap();
    assert_eq!(report["attributes_created"], json!({"0": ["Ordem"]}));
    let warnings: Vec<&str> = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert_eq!(
        warnings,
        [
            "Comando 0 (attach_script, o 1º do lote): Controle não tem área (sensor3d): \
             on_area_enter e on_area_exit de Pista/area.luau nunca rodam nele; adicione o \
             componente sensor3d (add_component) ou anexe o script à área; Controle não recebe \
             cliques (não tem forma nem é um botão da interface): on_click de Pista/area.luau \
             nunca roda nele"
        ]
    );
    // A shape takes clicks, and an area added later in the batch settles the rest.
    let report = command::apply_and_report_with_scripts(
        &mut project,
        &scene,
        &commands(json!([
            {"op": "attach_script", "entity": "Caixa", "path": "Pista/area.luau"},
            {"op": "add_component", "entity": "Caixa", "component": "sensor3d"},
        ])),
        &sources,
    )
    .unwrap();
    assert_eq!(
        report["attributes_created"],
        json!({"0": ["Aviso", "Ordem"]})
    );
    assert!(report.get("warnings").is_none(), "{report}");
    // Attached again: nothing new is created, and nothing is told again.
    let again = command::apply_and_report_with_scripts(
        &mut project,
        &scene,
        &commands(json!([{"op": "attach_script", "entity": "Caixa", "path": "Pista/area.luau"}])),
        &sources,
    )
    .unwrap();
    assert!(again.get("attributes_created").is_none(), "{again}");
    // In a 2D scene the area is a sensor2d.
    let mut flat = editing::blank_project("Plano", SceneKind::TwoD).unwrap();
    let scene = flat.start_scene.clone();
    flat.scenes[0].entities.push(Entity::new("Gatilho", None));
    let report = command::apply_and_report_with_scripts(
        &mut flat,
        &scene,
        &commands(json!([{"op": "attach_script", "entity": "Gatilho", "path": "Pista/area.luau"}])),
        &sources,
    )
    .unwrap();
    let told = report["warnings"][0].as_str().unwrap();
    assert!(
        told.contains(
            "Gatilho não tem área (sensor2d): on_area_enter e on_area_exit de Pista/area.luau \
             nunca rodam nele; adicione o componente sensor2d (add_component)"
        ),
        "{told}"
    );
}

#[test]
fn clear_logic_takes_the_whole_graph_off_and_keeps_the_rest() {
    let mut project = editing::blank_project("Limpar", SceneKind::ThreeD).unwrap();
    let scene = project.start_scene.clone();
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    door.scripts.push(ScriptRef::new("Pista/porta.luau"));
    door.attributes.insert("Vida".into(), Value::Number(3.));
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    command::apply(
        &mut project,
        &scene,
        &commands(json!([
            {"op": "add_node", "entity": &id, "operation": "event.scene_start"},
            {"op": "add_node", "entity": &id, "operation": "debug.message",
             "links": [["$0", "exec", "exec"]]},
        ])),
    )
    .unwrap();
    assert_eq!(project.scenes[0].entities[0].graph.nodes.len(), 2);
    let parsed = commands(json!([{"op": "clear_logic", "entity": "porta"}]));
    assert_eq!(parsed[0].label(), "Tirar a lógica de nós");
    let report = command::apply_and_report(&mut project, &scene, &parsed).unwrap();
    assert_eq!(report, json!({"created": {}}));
    let door = &project.scenes[0].entities[0];
    assert!(door.graph.nodes.is_empty() && door.graph.edges.is_empty());
    assert_eq!(door.scripts, [ScriptRef::new("Pista/porta.luau")]);
    assert_eq!(door.attributes["Vida"], Value::Number(3.));
    let report = command::apply_and_report(&mut project, &scene, &parsed).unwrap();
    assert_eq!(
        report["warnings"],
        json!([
            "Comando 0 (clear_logic, o 1º do lote): Porta não tinha lógica de nós; nada \
                mudou nele"
        ])
    );
    let error = command::apply(
        &mut project,
        &scene,
        &commands(json!([{"op": "clear_logic", "entity": "Janela"}])),
    )
    .unwrap_err();
    assert!(error.to_string().contains("Janela"), "{error}");
}

#[test]
fn the_scene_tree_and_objects_tell_their_scripts() {
    let root = folder("objetos");
    put(&root, "Pista/area.luau", AREA.as_bytes());
    put(&root, "Pista/quebrado.luau", b"local M = {\nreturn M\n");
    let mut project = editing::blank_project("Objetos", SceneKind::ThreeD).unwrap();
    project.layout = Layout::Tree;
    let scene = project.start_scene.clone();
    let mut control = Entity::new("Controle", None);
    control.scripts = vec![
        ScriptRef::new("Pista/area.luau"),
        ScriptRef::new("Pista/quebrado.luau"),
        ScriptRef::new("Pista/sumiu.luau"),
    ];
    let plain = Entity::new("Chão", Some(Primitive::Plane));
    project.scenes[0].entities.extend([control, plain]);
    persistence::save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();

    let tree = query::answer(&project, &scene, "scene_tree", &json!({})).unwrap();
    let objects = tree["objects"].as_array().unwrap();
    assert_eq!(
        objects[0]["scripts"],
        json!(["Pista/area.luau", "Pista/quebrado.luau", "Pista/sumiu.luau"])
    );
    assert!(objects[1].get("scripts").is_none());

    let mut answer = query::answer(&project, &scene, "entity", &json!({"id": "Controle"})).unwrap();
    query::describe_scripts(&mut answer, &project, &scene, &root);
    let scripts = answer["scripts"].as_array().unwrap();
    assert_eq!(scripts[0]["path"], "Pista/area.luau");
    assert_eq!(
        scripts[0]["callbacks"],
        json!(["on_click", "on_area_enter", "on_area_exit"])
    );
    assert_eq!(scripts[0]["attributes"], json!(["Aviso", "Ordem"]));
    assert_eq!(scripts[0]["state"], json!({"vezes": "Number"}));
    let warnings = scripts[0]["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .starts_with("Controle não tem área")
    );
    assert!(
        scripts[1]["error"]
            .as_str()
            .unwrap()
            .starts_with("Pista/quebrado.luau:"),
        "{scripts:?}"
    );
    assert_eq!(
        scripts[2],
        json!({"path": "Pista/sumiu.luau", "missing": true})
    );
    // Cut down to the scripts, as several objects.
    let mut many = query::answer(
        &project,
        &scene,
        "entity",
        &json!({"ids": ["Controle"], "fields": ["scripts"]}),
    )
    .unwrap();
    query::describe_scripts(&mut many, &project, &scene, &root);
    assert_eq!(many["objects"][0]["scripts"], answer["scripts"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_run_reports_each_script_error_once_with_how_often_it_happened() {
    let mut project = editing::blank_project("Erros", SceneKind::ThreeD).unwrap();
    let mut door = Entity::new("Porta", None);
    door.scripts.push(ScriptRef::new("Pista/porta.luau"));
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    let scripts = Arc::new(ScriptSources::from_texts([(
        "Pista/porta.luau",
        "local M = {}\nfunction M.on_step(self)\n    error(\"travou\")\nend\nreturn M\n",
    )]));
    let request: SimulationRequest =
        serde_json::from_value(json!({"seconds": 0.5, "actions": "none", "ui": false})).unwrap();
    let (report, _) = headless::run_with_scripts(&project, scripts, &request).unwrap();
    assert_eq!(report.script_errors.len(), 1, "{:?}", report.script_errors);
    let error = &report.script_errors[0];
    assert_eq!(error.object.as_ref().unwrap().id, id);
    assert_eq!(error.object.as_ref().unwrap().name, "Porta");
    assert_eq!(
        (error.path.as_deref(), error.line),
        (Some("Pista/porta.luau"), 3)
    );
    assert_eq!(error.count, 30);
    assert!(!error.stopped);
    assert!(
        error.text.starts_with("Porta · Pista/porta.luau:3:") && error.text.contains("travou"),
        "{}",
        error.text
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["script_errors"][0]["count"], 30);
    assert!(json["script_errors"][0].get("stopped").is_none());
    // A run without errors has none to report.
    let quiet = Arc::new(ScriptSources::from_texts([(
        "Pista/porta.luau",
        "local M = {}\nfunction M.on_step(self) end\nreturn M\n",
    )]));
    let (report, _) = headless::run_with_scripts(&project, quiet, &request).unwrap();
    assert!(report.script_errors.is_empty());
    assert!(
        serde_json::to_value(&report)
            .unwrap()
            .get("script_errors")
            .is_none()
    );
}

#[test]
fn a_run_reports_the_file_and_line_of_a_message_nobody_hears() {
    let mut project = editing::blank_project("Mensagens", SceneKind::ThreeD).unwrap();
    let mut door = Entity::new("Porta", None);
    door.scripts.push(ScriptRef::new("Pista/porta.luau"));
    project.scenes[0]
        .entities
        .extend([door, Entity::new("Caixa", None)]);
    let scripts = Arc::new(ScriptSources::from_texts([(
        "Pista/porta.luau",
        "local M = {}\nfunction M.on_step(self)\n    scene.find(\"Caixa\"):send(\"abrir\")\nend\n\
         return M\n",
    )]));
    let request: SimulationRequest =
        serde_json::from_value(json!({"seconds": 0.1, "actions": "none", "ui": false})).unwrap();
    let (report, _) = headless::run_with_scripts(&project, scripts, &request).unwrap();
    assert_eq!(report.script_errors.len(), 1, "{:?}", report.script_errors);
    let error = &report.script_errors[0];
    assert_eq!(
        (error.path.as_deref(), error.line, error.object.is_none()),
        (Some("Pista/porta.luau"), 3, true)
    );
    assert!(
        error
            .text
            .contains("Caixa não tem on_message em nenhum script"),
        "{}",
        error.text
    );
    assert_eq!(error.count, 6);
}

/// A script that places the model Torre by its file, in another case.
const BUILDER: &str = r#"local M = {}
function M.on_start(self)
    local torre = scene.spawn_model("pista/torre.modelo.json")
    print(torre.name)
end
return M
"#;

#[test]
fn scene_spawn_model_finds_a_model_by_its_file_and_the_names_check_follows_it() {
    let mut project = editing::blank_project("Torres", SceneKind::ThreeD).unwrap();
    project.layout = Layout::Tree;
    let scene = project.start_scene.clone();
    let tower = Entity::new("Torre", Some(Primitive::Cube));
    let tower_id = tower.id.clone();
    project.scenes[0].entities.push(tower);
    project
        .save_model_mapped_in(&scene, &tower_id, "Torre", Some("Pista"))
        .unwrap();
    project.scenes[0].entities.clear();
    let mut builder = Entity::new("Construtor", None);
    builder
        .scripts
        .push(ScriptRef::new("Pista/construtor.luau"));
    project.scenes[0].entities.push(builder);
    let model = project.model_named("Torre").unwrap().clone();
    assert_eq!(model.path, "Pista/Torre.modelo.json");
    let sources = ScriptSources::from_texts([("Pista/construtor.luau", BUILDER)]);
    let request: SimulationRequest =
        serde_json::from_value(json!({"seconds": 0.1, "actions": "none", "ui": false})).unwrap();
    let (report, _) =
        headless::run_with_scripts(&project, Arc::new(sources.clone()), &request).unwrap();
    assert!(
        report.script_errors.is_empty(),
        "{:?}",
        report.script_errors
    );
    let logs = serde_json::to_value(&report.logs).unwrap().to_string();
    assert!(logs.contains("Construtor · Torre"), "{logs}");
    // The lint and the uses find it as the game does.
    assert!(
        script::lint(&sources, ["Pista/construtor.luau"], Some(&project)).is_empty(),
        "{:?}",
        script::lint(&sources, ["Pista/construtor.luau"], Some(&project))
    );
    assert_eq!(
        script::citations(&project, &sources)[&model.id],
        [Citation {
            path: "Pista/construtor.luau".into(),
            line: 3,
            function: "scene.spawn_model".into(),
        }]
    );
    // Moved to another folder, the file the script gives is gone: the move tells it, with the
    // new file.
    let mut moved = project.clone();
    moved
        .assets
        .iter_mut()
        .find(|asset| asset.id == model.id)
        .unwrap()
        .path = "Modelos/Torre.modelo.json".into();
    let broken = script::broken_citations(&project, &moved, &sources, &[NameKind::Model]);
    assert_eq!(broken.len(), 1, "{broken:?}");
    assert_eq!(broken[0].id.as_deref(), Some(model.id.as_str()));
    let root = folder("torres");
    put(&root, "Pista/construtor.luau", BUILDER.as_bytes());
    persistence::save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
    let mut saved = project.clone();
    let done = command::apply_on_files(
        &mut saved,
        &scene,
        &commands(json!([
            {"op": "set_visible", "entity": "Construtor", "visible": false},
            {"op": "move", "items": ["Pista/Torre.modelo.json"], "to": "Modelos"},
        ])),
        &root,
        &mut KnownFiles::default(),
        "Agente: Mover para pasta",
    )
    .unwrap();
    assert_eq!(
        saved.asset(&model.id).unwrap().path,
        "Modelos/Torre.modelo.json"
    );
    assert_eq!(
        done.report["warnings"],
        json!([
            "Comando 1 (move, o 2º do lote): Pista/construtor.luau:3: scene.spawn_model: o \
             modelo \"pista/torre.modelo.json\" agora está em Modelos/Torre.modelo.json; \
             atualize o script, que no jogo para com erro nessa linha"
        ])
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_status_of_the_scripts_counts_them_and_tells_what_keeps_them_from_running() {
    let root = folder("status");
    put(&root, "Pista/porta.luau", b"local M = {}\nreturn M\n");
    put(&root, "Pista/quebrado.luau", b"local M = {\nreturn M\n");
    put(&root, "Comum/dados.luau", b"return { vida = 3 }\n");
    let mut project = editing::blank_project("Situação", SceneKind::ThreeD).unwrap();
    let mut door = Entity::new("Porta", None);
    door.scripts = vec![
        ScriptRef::new("Pista/porta.luau"),
        ScriptRef::new("Pista/sumiu.luau"),
    ];
    project.scenes[0].entities.push(door);
    let status = query::scripts_status(&project, &root);
    assert_eq!(status["files"], 3);
    assert_eq!(status["objects"], 1);
    assert_eq!(status["checker"]["name"], "oxy");
    let errors: Vec<&str> = status["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(
        errors[0],
        "Porta: o script Pista/sumiu.luau não existe na pasta do projeto"
    );
    assert!(errors[1].starts_with("Pista/quebrado.luau:"), "{errors:?}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn organizing_in_folders_tells_the_names_it_changed_that_scripts_give() {
    let root = folder("conversao");
    let mut project = editing::blank_project("Antigo", SceneKind::ThreeD).unwrap();
    let id = new_id();
    project.assets.push(Asset {
        id: id.clone(),
        name: "Pulo: alto".into(),
        path: format!("assets/{id}.wav"),
        kind: AssetKind::Audio,
        model: None,
    });
    put(&root, &format!("assets/{id}.wav"), &wav());
    put(
        &root,
        "scripts/sons.luau",
        b"local M = {}\nfunction M.on_start(self)\n    audio.play(\"Pulo: alto\")\nend\nreturn M\n",
    );
    persistence::save_project(&root, &project).unwrap();
    let (opened, report) = persistence::open_project(&root).unwrap();
    assert_eq!(report.layout, Layout::Legacy);
    let converted = conversion::convert(
        &root.join(PROJECT_FILE),
        &opened,
        None,
        &mut KnownFiles::default(),
        Who::Agent,
    )
    .unwrap();
    assert_eq!(converted.project.asset(&id).unwrap().name, "Pulo alto");
    assert!(
        converted.warnings.iter().any(|warning| warning
            == "scripts/sons.luau:3: audio.play: o som \"Pulo: alto\" agora se chama Pulo alto; \
                atualize o script, que no jogo para com erro nessa linha"),
        "{:?}",
        converted.warnings
    );
    assert!(
        converted
            .lines()
            .any(|line| line.contains("agora se chama Pulo alto"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_catalog_lists_the_scripts_api_by_default_and_the_nodes_on_request() {
    let catalog = query::catalog_query(&json!({})).unwrap();
    assert!(catalog.get("nodes").is_none() && catalog.get("categories").is_none());
    assert!(
        catalog["legacy_nodes"]
            .as_str()
            .unwrap()
            .contains("nodes: true")
    );
    for section in ["components", "presets", "ui_elements", "input_keys", "api"] {
        assert!(catalog.get(section).is_some(), "{section}");
    }
    let areas = catalog["api"]["areas"].as_array().unwrap();
    let audio = areas.iter().find(|area| area["area"] == "audio").unwrap();
    assert!(
        audio["names"]
            .as_array()
            .unwrap()
            .contains(&json!("audio.play"))
    );
    assert_eq!(catalog["api"]["types_file"], ".oxy/tipos/oxy.d.luau");
    assert!(
        catalog.to_string().len() * 2 < query::catalog().to_string().len(),
        "the default is far shorter than everything"
    );
    let rows = query::catalog_query(&json!({"api": ["audio.", "obj:after"]})).unwrap();
    let names: Vec<&str> = rows["api"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"audio.play") && names.contains(&"Object:after"));
    assert!(rows.get("components").is_none(), "only the rows asked for");
    let character = query::catalog_query(&json!({"area": "character", "limit": 2})).unwrap();
    assert_eq!(character["api"].as_array().unwrap().len(), 2);
    assert_eq!(character["next_offset"], 2);
    let nodes = query::catalog_query(&json!({"nodes": true})).unwrap();
    assert!(nodes["nodes"].as_array().unwrap().len() > 50);
    assert!(nodes.get("api").is_none());
    for (args, expected) in [
        (json!({"api": ["audio.paly"]}), "quis dizer audio.play?"),
        (json!({"area": "som"}), "Área desconhecida"),
        (
            json!({"api": ["audio."], "operations": ["event.input"]}),
            "um de cada vez",
        ),
        (json!({"api": "audio.play"}), "api inválido"),
    ] {
        let error = query::catalog_query(&args).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn unknown_options_are_told_with_the_nearest_known_one() {
    let known = ["id", "ids", "fields", "graph"];
    assert!(query::unknown_options(&json!({"id": "Porta", "fields": []}), &known).is_empty());
    assert_eq!(
        query::unknown_options(&json!({"id": "Porta", "fieldz": ["x"], "modo": 1}), &known),
        [
            "fieldz: opção desconhecida, ignorada; quis dizer fields?",
            "modo: opção desconhecida, ignorada"
        ]
    );
}

#[test]
fn script_lines_are_found_in_console_texts() {
    let at = |text: &str| script::location_in(text).map(|found| (found.path, found.line));
    assert_eq!(
        at("Script não recarregado: Pista/porta.luau:12: erro de sintaxe. O teste segue."),
        Some(("Pista/porta.luau".into(), 12))
    );
    assert_eq!(
        at("Comando 0 (attach_script, o 1º do lote): Meu jogo/a b.luau:5: audio.play: …"),
        Some(("Meu jogo/a b.luau".into(), 5))
    );
    assert_eq!(
        at("Porta · scripts/porta.luau:3: o som não existe"),
        Some(("scripts/porta.luau".into(), 3))
    );
    assert_eq!(at("Script Pista/porta.luau alterado (+2 -1 linhas)"), None);
    assert_eq!(at("Script recarregado: Pista/porta.luau (3 objetos)"), None);
    assert_eq!(at("nada aqui"), None);
}
