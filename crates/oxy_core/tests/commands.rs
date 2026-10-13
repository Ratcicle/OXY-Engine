use oxy_core::{
    command::{self, Command},
    document::*,
    editing, graph,
    history::CommandHistory,
    input_actions,
    physics3d::CollisionShape,
    query, surface,
    texture_cache::TextureCache,
};
use serde_json::json;

fn project(kind: SceneKind) -> (Project, Id) {
    let project = editing::blank_project("Teste", kind).unwrap();
    let scene = project.start_scene.clone();
    (project, scene)
}

fn batch(value: serde_json::Value) -> Vec<Command> {
    command::parse(&value).unwrap()
}

#[test]
fn a_batch_builds_objects_components_and_logic_with_placeholders() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands = batch(json!([
        {"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"},
        {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "parent": "$0",
         "position": [0.0, 1.0, 0.0]},
        {"op": "add_component", "entity": "$1", "component": "collider3d"},
        {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
        {"op": "add_node", "entity": "$0", "operation": "debug.message",
         "params": {"message": {"Text": "Olá"}}},
        {"op": "connect", "entity": "$0", "from_node": "$3", "from_port": "exec",
         "to_node": "$4", "to_port": "exec"},
    ]));
    let applied = command::apply(&mut project, &scene, &commands).unwrap();
    assert_eq!(applied.len(), 6);
    let group = &applied[0].created[0];
    let cube = &applied[1].created[0];
    let scene = project.scene(&scene).unwrap();
    assert_eq!(scene.entity(cube).unwrap().parent.as_ref(), Some(group));
    assert!(scene.entity(cube).unwrap().physics3d.is_some());
    let graph = &scene.entity(group).unwrap().graph;
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.nodes[1].text("message"), "Olá");
    assert!(validate_project(&project).is_ok());
}

#[test]
fn a_failing_command_leaves_the_project_untouched() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let before = project.clone();
    let commands = batch(json!([
        {"op": "create_entity", "kind": {"type": "group"}},
        {"op": "rename_entity", "entity": "inexistente", "name": "X"},
    ]));
    let error = command::apply(&mut project, &scene, &commands).unwrap_err();
    assert_eq!(error.index, 1);
    assert_eq!(error.op, "rename_entity");
    // Counted from 0, as "$1" and created name it, and in words for the reader.
    assert!(
        error
            .to_string()
            .starts_with("Comando 1 (rename_entity, o 2º do lote): Objeto inexistente"),
        "{error}"
    );
    assert!(error.to_string().contains("Nada foi alterado"));
    assert_eq!(project, before);
    // Parsing counts the same way.
    let error = command::parse(&json!([
        {"op": "create_entity", "kind": {"type": "group"}},
        {"op": "voar"},
    ]))
    .unwrap_err();
    assert!(
        error.starts_with("Comando 1 (voar, o 2º do lote) não reconhecido: valor `voar`"),
        "{error}"
    );
    // A message that ends a sentence gets no second period.
    let error = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}},
            {"op": "duplicate_entities", "ids": ["$0"], "count": 0},
        ])),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.ends_with("entre 1 e 100. Nada foi alterado.") && !error.contains(".."),
        "{error}"
    );
}

#[test]
fn unknown_fields_and_wrong_scene_shapes_are_refused() {
    assert!(command::parse(&json!({"op": "rename_project", "nome": "X"})).is_err());
    assert!(command::parse(&json!({"op": "fly"})).is_err());
    let (mut project, scene) = project(SceneKind::TwoD);
    let cube = batch(json!({"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}}));
    let error = command::apply(&mut project, &scene, &cube).unwrap_err();
    assert!(error.message.contains("2D"), "{error}");
    let rectangle =
        batch(json!({"op": "create_entity", "kind": {"type": "shape", "shape": "Rectangle"}}));
    command::apply(&mut project, &scene, &rectangle).unwrap();
}

#[test]
fn update_entity_patches_fields_but_never_the_id() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let created = command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "create_entity", "kind": {"type": "shape", "shape": "Sphere"}})),
    )
    .unwrap();
    let id = created[0].created[0].clone();
    let patch = batch(json!({"op": "update_entity", "entity": id,
        "patch": {"name": "Bola", "dimensions": [2.0, 2.0, 2.0], "layer": 3}}));
    command::apply(&mut project, &scene, &patch).unwrap();
    let e = project.scene(&scene).unwrap().entity(&id).unwrap();
    assert_eq!(
        (e.name.as_str(), e.dimensions, e.layer),
        ("Bola", [2.; 3], 3)
    );
    for patch in [json!({"id": "outro"}), json!({"campo_inexistente": 1})] {
        let commands = batch(json!({"op": "update_entity", "entity": id, "patch": patch}));
        assert!(command::apply(&mut project, &scene, &commands).is_err());
    }
}

#[test]
fn components_follow_the_editor_dependency_rules() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands = batch(json!([
        {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}},
        {"op": "add_component", "entity": "$0", "component": "platform"},
    ]));
    let id = command::apply(&mut project, &scene, &commands).unwrap()[0].created[0].clone();
    let remove_shape =
        batch(json!({"op": "remove_component", "entity": id, "component": "collider3d"}));
    let error = command::apply(&mut project, &scene, &remove_shape).unwrap_err();
    assert!(error.message.contains("Plataforma"), "{error}");
    let both = batch(json!([
        {"op": "remove_component", "entity": id, "component": "platform"},
        {"op": "remove_component", "entity": id, "component": "collider3d"},
    ]));
    command::apply(&mut project, &scene, &both).unwrap();
    let e = project.scene(&scene).unwrap().entity(&id).unwrap();
    assert!(e.platform.is_none() && e.physics3d.is_none());
}

#[test]
fn a_batch_is_one_undo_step_in_the_command_history() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let mut images = TextureCache::default();
    let before = project.clone();
    let mut history = CommandHistory::new();
    history.begin("Agente: 2 comandos", &project, &images);
    let commands = batch(json!([
        {"op": "create_entity", "kind": {"type": "camera"}},
        {"op": "set_transform", "entity": "$0", "position": [0.0, 2.0, 5.0],
         "rotation": [-15.0, 0.0, 0.0]},
    ]));
    let id = command::apply(&mut project, &scene, &commands).unwrap()[0].created[0].clone();
    let camera = project.scene(&scene).unwrap().entity(&id).unwrap();
    assert!((camera.transform.rotation[0] - (-15f32).to_radians()).abs() < 1e-6);
    history.commit(&project, &mut images).unwrap();
    assert_eq!(history.undo_label(), Some("Agente: 2 comandos"));
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);
}

#[test]
fn scenes_can_be_created_renamed_started_and_deleted() {
    let (mut project, first) = project(SceneKind::ThreeD);
    let commands = batch(json!([
        {"op": "create_scene", "name": "Fase 2", "kind": "2d"},
        {"op": "rename_scene", "scene": "$0", "name": "Fase dois"},
        {"op": "set_start_scene", "scene": "$0"},
    ]));
    let second = command::apply(&mut project, &first, &commands).unwrap()[0].created[0].clone();
    assert_eq!(project.start_scene, second);
    assert_eq!(project.scene(&second).unwrap().kind, SceneKind::TwoD);
    let delete_start = batch(json!({"op": "delete_scene", "scene": second}));
    assert!(command::apply(&mut project, &first, &delete_start).is_err());
    let delete_first = batch(json!({"op": "delete_scene", "scene": first}));
    command::apply(&mut project, &second, &delete_first).unwrap();
    assert_eq!(project.scenes.len(), 1);
}

#[test]
fn queries_describe_the_project_for_agents() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands = batch(json!([
        {"op": "create_entity", "kind": {"type": "preset", "preset": "third_person"},
         "name": "Herói"},
    ]));
    let hero = command::apply(&mut project, &scene, &commands).unwrap()[0].created[0].clone();
    let summary = query::project_summary(&project);
    assert_eq!(summary["scenes"][0]["kind"], "3d");
    let tree = query::scene_tree(&project, &scene, &query::TreeQuery::default()).unwrap();
    let names: Vec<_> = tree["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"Herói".to_owned()), "{names:?}");
    let entity = query::entity(&project, &scene, &hero).unwrap();
    assert_eq!(entity["id"], hero.as_str());
    assert_eq!(
        query::find(&project, "herói", query::Page::ALL)["objects"][0]["id"],
        hero.as_str()
    );
    let catalog = query::catalog();
    let nodes = catalog["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), oxy_core::graph::registry().len());
    assert!(
        nodes
            .iter()
            .all(|n| !n["description"].as_str().unwrap().is_empty())
    );
}

#[test]
fn headless_simulation_reports_logs_actions_and_watched_objects() {
    use oxy_core::headless::{self, SimulationRequest};
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands = batch(json!([
        {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Caixa",
         "position": [1.0, 2.0, 3.0]},
        {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
        {"op": "add_node", "entity": "$0", "operation": "debug.message",
         "params": {"message": {"Text": "Olá do teste"}}},
        {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
         "to_node": "$2", "to_port": "exec"},
        {"op": "create_input_action", "name": "Pular", "key": "Space"},
    ]));
    let cube = command::apply(&mut project, &scene, &commands).unwrap()[0].created[0].clone();
    let before = project.clone();
    // An action by its name; the project has no other.
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 1.0, "watch": [cube], "samples": 4,
        "inputs": [{"type": "press", "at": 0.2, "action": "Pular"},
                   {"type": "release", "at": 0.4, "action": "pular"}],
    }))
    .unwrap();
    let report = headless::simulate(&project, &request).unwrap();
    assert_eq!(project, before, "the simulation runs on a copy");
    assert!(
        report.logs.iter().any(|l| l.text.contains("Olá do teste")),
        "{:?}",
        report.logs
    );
    let Some(headless::Actions::Summary(actions)) = &report.actions else {
        panic!("actions are summarized by default: {:?}", report.actions);
    };
    assert!(actions.iter().any(|a| a.text.starts_with("Caixa · ")));
    let samples = &report.watched[0].samples;
    assert_eq!(samples.len(), 4);
    assert_eq!(samples.last().unwrap().position, Some([1., 2., 3.]));
    assert_eq!(report.final_scene, scene);
    let too_long = SimulationRequest {
        seconds: 9999.,
        ..request
    };
    assert!(headless::simulate(&project, &too_long).is_err());
}

#[test]
fn node_choices_are_refused_outside_their_values_and_listed_in_the_catalog() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let before = project.clone();
    for (operation, key, value) in [
        ("event.input", "mode", "segurando"),
        ("action.component", "component", "colisor"),
        ("math.binary", "operator", "^"),
        ("math.unary", "operator", "log"),
        ("text.time", "format", "hh:mm"),
        ("random.number", "mode", "inteiro"),
        ("condition.compare", "operator", "=>"),
        ("event.character", "kind", "pulo"),
        ("event.pause", "when", "pausar"),
        ("game.pause", "mode", "parar"),
        ("transform.write", "channel", "posição"),
        ("transform.write", "mode", "multiplicar"),
        ("transform.write", "space", "pai"),
    ] {
        let commands = batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}},
            {"op": "add_node", "entity": "$0", "operation": operation,
             "params": {key: {"Text": value}}},
        ]));
        let error = command::apply(&mut project, &scene, &commands).unwrap_err();
        assert_eq!(error.index, 1);
        assert!(
            error.message.contains(&format!("recebeu \"{value}\"")),
            "{error}"
        );
        assert_eq!(project, before);
    }
    let error = command::apply(
        &mut project,
        &scene,
        &batch(
            json!({"op": "add_node", "entity": "x", "operation": "event.input",
                      "params": {"mode": {"Text": "segurando"}}}),
        ),
    )
    .unwrap_err();
    assert!(
        error
            .message
            .contains("pressed (Pressionar), held (Manter), released (Soltar)"),
        "the error lists the values: {error}"
    );
    // set_node_params checks them too; the allowed ones pass.
    let created = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}},
            {"op": "add_node", "entity": "$0", "operation": "math.binary",
             "params": {"operator": {"Text": "*"}}},
        ])),
    )
    .unwrap();
    let (entity, node) = (&created[0].created[0], &created[1].created[0]);
    let set = |value: &str| {
        batch(
            json!({"op": "set_node_params", "entity": entity, "node": node,
                     "params": {"operator": {"Text": value}}}),
        )
    };
    assert!(command::apply(&mut project, &scene, &set("**")).is_err());
    for allowed in ["/", "%", "min", "max"] {
        command::apply(&mut project, &scene, &set(allowed)).unwrap();
    }
    // Logic switches moving platforms too.
    command::apply(
        &mut project,
        &scene,
        &batch(
            json!({"op": "add_node", "entity": entity, "operation": "action.component",
                     "params": {"component": {"Text": "platform"}}}),
        ),
    )
    .unwrap();
    // The game pause's nodes take their values, plain ones too.
    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "add_node", "entity": entity, "operation": "event.pause",
             "params": {"when": "resume"}},
            {"op": "add_node", "entity": entity, "operation": "game.pause",
             "params": {"mode": "pause"}},
            {"op": "add_node", "entity": entity, "operation": "game.read"},
            {"op": "add_node", "entity": entity, "operation": "game.quit"},
            {"op": "add_node", "entity": entity, "operation": "transform.write",
             "params": {"channel": "rotation", "mode": "set", "space": "world"}},
        ])),
    )
    .unwrap();

    // The editor reads the same lists from the core.
    assert_eq!(
        graph::choices("event.input", "mode"),
        Some(&graph::INPUT_MODES[..])
    );
    assert_eq!(
        graph::choices("condition.compare", "operator"),
        Some(&graph::COMPARISONS[..])
    );
    assert_eq!(
        graph::choices("math.binary", "operator"),
        Some(&graph::ARITHMETIC[..])
    );
    assert!(graph::choices("character.posture", "command").is_some());
    assert!(graph::choices("debug.message", "message").is_none());

    let catalog = query::catalog();
    let param = |operation: &str, id: &str| {
        let node = catalog["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["operation"] == operation)
            .unwrap();
        node["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        param("event.input", "mode")["choices"][1],
        json!({"value": "held", "label": "Manter"})
    );
    assert_eq!(
        param("action.component", "component")["choices"][0],
        json!({"value": "collider", "label": "Colisão / área (2D e 3D)"})
    );
    assert_eq!(
        param("action.component", "component")["choices"][5],
        json!({"value": "platform", "label": "Plataforma móvel"})
    );
    assert_eq!(
        param("event.pause", "when")["choices"],
        json!([
            {"value": "pause", "label": "Ao pausar"},
            {"value": "resume", "label": "Ao retomar"},
        ])
    );
    assert_eq!(
        param("game.pause", "mode")["choices"],
        json!([
            {"value": "pause", "label": "Pausar"},
            {"value": "resume", "label": "Retomar"},
            {"value": "toggle", "label": "Alternar"},
        ])
    );
    assert_eq!(param("event.input", "action")["ref"], "input_action");
    assert!(param("event.input", "action").get("choices").is_none());
    for (operation, id, reference) in [
        ("input.read", "action", "input_action"),
        ("action.scene", "scene", "scene"),
        ("action.sound", "asset", "audio"),
        ("action.animation", "clip", "clip"),
        ("action.animation", "target", "object"),
        ("attribute.set", "attribute", "attribute"),
        ("event.animation", "marker", "marker"),
        ("surface.apply", "surface", "surface"),
    ] {
        assert_eq!(param(operation, id)["ref"], reference, "{operation}.{id}");
    }
    assert!(param("debug.message", "message").get("ref").is_none());
    let keys = catalog["input_keys"].as_array().unwrap();
    assert!(keys.contains(&json!("Space")) && keys.contains(&json!("MouseLeft")));
    assert!(!keys.contains(&json!("Escape")) && !keys.contains(&json!("F3")));
    assert_eq!(catalog["surface_presets"][1], "ice");
}

#[test]
fn input_actions_are_created_renamed_rebound_and_deleted_by_command() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let created = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_input_action", "name": " Reiniciar ", "key": "r"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Controle"},
            {"op": "add_node", "entity": "$1", "operation": "event.input",
             "params": {"action": {"Text": "$0"}}},
        ])),
    )
    .unwrap();
    let action = created[0].created[0].clone();
    let (entity, node) = (created[1].created[0].clone(), created[2].created[0].clone());
    assert_eq!(
        project.input_bindings[&action], "R",
        "keys are stored as the game names them"
    );
    assert_eq!(input_actions::label(&project, &action), "Reiniciar");

    for (command, expected) in [
        (
            json!({"op": "create_input_action", "name": "reiniciar", "key": "K"}),
            "Já existe",
        ),
        (
            json!({"op": "create_input_action", "name": "Pausar", "key": "Escape"}),
            "reservada",
        ),
        (
            json!({"op": "create_input_action", "name": "Pausar", "key": "F3"}),
            "reservada",
        ),
        (
            json!({"op": "create_input_action", "name": "Pausar", "key": "Ctrl"}),
            "Tecla desconhecida",
        ),
        (
            json!({"op": "create_input_action", "name": " ", "key": "P"}),
            "nome",
        ),
        (
            json!({"op": "set_input_key", "action": "Inexistente", "key": "P"}),
            "não existe",
        ),
        (
            json!({"op": "delete_input_action", "action": action}),
            "nó Ação de entrada",
        ),
    ] {
        let before = project.clone();
        let error = command::apply(&mut project, &scene, &batch(command)).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert_eq!(project, before);
    }

    // By name or by ID; renaming to its own name in other case is fine.
    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "rename_input_action", "action": "reiniciar", "name": "REINICIAR"},
            {"op": "set_input_key", "action": action, "key": "ArrowUp"},
            {"op": "rename_input_action", "action": action, "name": "Recomeçar"},
        ])),
    )
    .unwrap();
    assert_eq!(project.input_bindings[&action], "Up");
    assert_eq!(input_actions::label(&project, &action), "Recomeçar");

    let summary = query::project_summary(&project);
    let listed = summary["input_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == action.as_str())
        .unwrap();
    assert_eq!(listed["name"], "Recomeçar");
    assert_eq!(listed["key"], "Up");
    assert_eq!(
        listed["used_by"],
        json!([{"entity": entity, "name": "Controle", "role": "event.input"}])
    );

    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "remove_node", "entity": entity, "node": node},
            {"op": "delete_input_action", "action": "recomeçar"},
        ])),
    )
    .unwrap();
    assert!(!project.input_bindings.contains_key(&action));
    assert!(!project.input_labels.contains_key(&action));
    assert!(validate_project(&project).is_ok());
}

#[test]
fn an_input_action_name_never_stands_for_another_action() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    // Older projects keep readable IDs, which commands and simulations take before names.
    for (id, key) in [("atacar", "J"), ("interagir", "E")] {
        project.input_bindings.insert(id.into(), key.into());
    }
    command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "rename_input_action", "action": "atacar", "name": "Golpear"})),
    )
    .unwrap();
    for (command, expected) in [
        (
            json!({"op": "create_input_action", "name": "Atacar", "key": "K"}),
            "é o ID da ação de entrada Golpear",
        ),
        (
            json!({"op": "rename_input_action", "action": "interagir", "name": "atacar"}),
            "é o ID da ação de entrada Golpear",
        ),
        (
            json!({"op": "rename_input_action", "action": "interagir", "name": "GOLPEAR"}),
            "Já existe uma ação de entrada chamada \"GOLPEAR\" (atacar)",
        ),
    ] {
        let before = project.clone();
        let error = command::apply(&mut project, &scene, &batch(command)).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert_eq!(project, before);
    }
    // An action may take its own ID back as its name.
    command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "rename_input_action", "action": "Golpear", "name": "Atacar"})),
    )
    .unwrap();
    assert_eq!(input_actions::label(&project, "atacar"), "Atacar");
    assert_eq!(query::validation(&project), json!({"valid": true}));

    // Presets and components add their default actions without that check, so the batch
    // that leaves a name to two actions warns, and by name commands refuse it.
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_input_action", "name": "Pular", "key": "Space"},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "first_person"}},
        ])),
    )
    .unwrap();
    let created = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(input_actions::label(&project, "pular"), "Pular");
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 2, "the name and the key: {warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    assert!(
        warning.contains("2 ações de entrada atendem pelo nome \"pular\"")
            && warning.contains(&format!("{created} (Pular)"))
            && warning.contains("pular (Pular)"),
        "{warning}"
    );
    assert!(
        warnings[1]
            .as_str()
            .unwrap()
            .starts_with("A tecla Space é de 2 ações de entrada"),
        "{warnings:?}"
    );
    let error = command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "set_input_key", "action": "Pular", "key": "K"})),
    )
    .unwrap_err();
    assert!(error.message.contains("Há 2 ações"), "{error}");
    // The game tells them apart, and projects may share a name on purpose (one label for
    // keys 1, 2 and 3), so validation leaves it out and a later batch does not repeat it.
    assert_eq!(query::validation(&project), json!({"valid": true}));
    assert_eq!(
        command::apply_and_report(
            &mut project,
            &scene,
            &batch(json!({"op": "set_input_key", "action": created, "key": "K"})),
        )
        .unwrap(),
        json!({"created": {}})
    );
    command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "rename_input_action", "action": created, "name": "Saltar"})),
    )
    .unwrap();
    assert_eq!(
        command::batch_warnings(&project),
        command::warnings(&project)
    );
}

#[test]
fn sound_nodes_take_their_channels_and_the_catalog_lists_them_under_som() {
    let (mut project, scene) = project(SceneKind::TwoD);
    let music = new_id();
    project.assets.push(Asset {
        id: music.clone(),
        name: "Trilha".into(),
        path: "assets/trilha.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let before = project.clone();
    for (operation, value) in [
        ("action.sound_loop", "musica"),
        ("action.sound_stop", "tudo"),
        ("action.volume", "geral"),
    ] {
        let commands = batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}},
            {"op": "add_node", "entity": "$0", "operation": operation,
             "params": {"channel": value}},
        ]));
        let error = command::apply(&mut project, &scene, &commands).unwrap_err();
        assert!(
            error.message.contains(&format!("recebeu \"{value}\"")),
            "{error}"
        );
        assert_eq!(project, before);
    }
    // Plain values; Parar som's empty audio stops every loop of the channel.
    let applied = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Rádio"},
            {"op": "add_node", "entity": "$0", "operation": "action.sound_loop",
             "params": {"asset": music, "channel": "effects", "volume": 0.5, "fade": 1}},
            {"op": "add_node", "entity": "$0", "operation": "action.sound_stop"},
            {"op": "add_node", "entity": "$0", "operation": "action.volume",
             "params": {"channel": "music", "volume": 0.3}},
            {"op": "add_node", "entity": "$0", "operation": "audio.read"},
        ])),
    )
    .unwrap();
    assert!(applied.iter().all(|a| a.warnings.is_empty()), "{applied:?}");
    let radio = &applied[0].created[0];
    let graph = &project.scene(&scene).unwrap().entity(radio).unwrap().graph;
    let looped = &graph.nodes[0];
    assert_eq!(looped.text("asset"), music);
    assert_eq!(looped.text("channel"), "effects");
    assert_eq!(looped.number("volume", 0.), 0.5);
    assert_eq!(looped.number("fade", 0.), 1.);
    let stop = &graph.nodes[1];
    assert_eq!((stop.text("asset"), stop.text("channel")), ("", "all"));
    assert_eq!(graph.nodes[2].text("channel"), "music");
    validate_project(&project).unwrap();
    assert!(command::warnings(&project).is_empty());
    // The audio is used by the loop, not by the stop that names none.
    let summary = query::project_summary(&project);
    assert_eq!(
        summary["assets"][0]["used_by"],
        json!([{"entity": radio, "name": "Rádio", "role": "action.sound_loop"}])
    );
    assert_eq!(
        graph::empty_choice("action.sound_stop", "asset"),
        Some("Todos os sons do canal")
    );
    assert_eq!(graph::empty_choice("action.sound_loop", "asset"), None);

    let catalog = query::catalog_query(&json!({"category": "Som"})).unwrap();
    let nodes = catalog["nodes"].as_array().unwrap();
    let listed: Vec<_> = nodes
        .iter()
        .map(|n| n["operation"].as_str().unwrap())
        .collect();
    assert_eq!(
        listed,
        [
            "action.sound",
            "action.sound_loop",
            "action.sound_stop",
            "action.volume",
            "audio.read"
        ]
    );
    let param = |operation: &str, id: &str| {
        let node = nodes.iter().find(|n| n["operation"] == operation).unwrap();
        node["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()
            .clone()
    };
    let values = |operation: &str| -> Vec<String> {
        param(operation, "channel")["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|choice| choice["value"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(values("action.sound_loop"), ["music", "effects"]);
    assert_eq!(values("action.sound_stop"), ["music", "effects", "all"]);
    assert_eq!(values("action.volume"), ["master", "music", "effects"]);
    assert_eq!(
        param("action.volume", "channel")["choices"][0],
        json!({"value": "master", "label": "Geral"})
    );
    for operation in ["action.sound", "action.sound_loop", "action.sound_stop"] {
        assert_eq!(param(operation, "asset")["ref"], "audio", "{operation}");
    }
    // Volume takes an input; Ler volumes gives the three.
    let ports = |operation: &str, side: &str| -> Vec<String> {
        let node = nodes.iter().find(|n| n["operation"] == operation).unwrap();
        node[side]
            .as_array()
            .unwrap()
            .iter()
            .map(|port| port["id"].as_str().unwrap().to_owned())
            .collect()
    };
    // Reproduzir WAV moved here as it was.
    let sound = nodes
        .iter()
        .find(|n| n["operation"] == "action.sound")
        .unwrap();
    assert_eq!(sound["outputs"][0]["label"], "Executar");
    assert_eq!(ports("action.sound_loop", "inputs"), ["exec", "volume"]);
    assert_eq!(ports("action.volume", "inputs"), ["exec", "volume"]);
    assert_eq!(
        ports("audio.read", "outputs"),
        ["master", "music", "effects"]
    );
}

#[test]
fn empty_choices_are_warned_but_an_unset_input_mode_reads_as_pressed() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let created = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_input_action", "name": "Pular", "key": "Space"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Lógica"},
            {"op": "add_node", "entity": "$1", "operation": "event.character"},
            {"op": "add_node", "entity": "$1", "operation": "math.binary"},
            {"op": "add_node", "entity": "$1", "operation": "event.input",
             "params": {"action": {"Text": "$0"}}},
        ])),
    )
    .unwrap();
    let entity = created[1].created[0].clone();
    // add_node refuses the empty value, as any other one outside the choices.
    let error = command::apply(
        &mut project,
        &scene,
        &batch(
            json!({"op": "add_node", "entity": entity, "operation": "event.character",
                      "params": {"kind": {"Text": ""}}}),
        ),
    )
    .unwrap_err();
    assert!(error.message.contains("recebeu \"\""), "{error}");

    // update_entity lets it through, so the batch warns: the game never matches it.
    let nodes = &project
        .scene(&scene)
        .unwrap()
        .entity(&entity)
        .unwrap()
        .graph
        .nodes;
    let mut nodes = serde_json::to_value(nodes).unwrap();
    for (index, key) in [(0, "kind"), (1, "operator"), (2, "mode")] {
        nodes[index]["params"][key] = json!({"Text": ""});
    }
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!({"op": "update_entity", "entity": entity,
                      "patch": {"graph": {"nodes": nodes}}})),
    )
    .unwrap();
    let warnings: Vec<&str> = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    for operation in ["event.character", "math.binary"] {
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(&format!("nó {operation} ")) && w.contains("recebeu \"\"")),
            "{operation}: {warnings:?}"
        );
    }
    // The empty mode of an event.input is one never set, which the game reads as pressed.
    assert!(
        !warnings.iter().any(|w| w.contains("event.input")),
        "{warnings:?}"
    );
    let validation = query::validation(&project);
    assert_eq!(validation["valid"], true);
    assert_eq!(validation["warnings"].as_array().unwrap().len(), 2);
}

#[test]
fn an_empty_batch_changes_nothing_and_repeats_no_warning() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}},
            {"op": "add_node", "entity": "$0", "operation": "event.input",
             "params": {"action": {"Text": "nada"}}},
        ])),
    )
    .unwrap();
    // One label for two keys, as some examples do.
    for (id, key) in [("1", "1"), ("2", "2")] {
        project.input_bindings.insert(id.into(), key.into());
        project
            .input_labels
            .insert(id.into(), "Escolher perfil".into());
    }
    assert_eq!(command::warnings(&project).len(), 1);
    assert_eq!(command::batch_warnings(&project).len(), 2);
    let before = project.clone();
    let answer = command::apply_and_report(&mut project, &scene, &[]).unwrap();
    assert_eq!(answer, json!({"created": {}}));
    assert_eq!(project, before);
    // The editor answers an empty batch without an undo step, from the same warnings.
    assert_eq!(
        command::report(&project, &[], &command::batch_warnings(&project)),
        answer
    );
}

#[test]
fn surfaces_are_created_assigned_through_patches_and_deleted_when_unused() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let created = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_surface", "preset": "ice", "name": "Gelo fino", "friction": 0.01},
            {"op": "create_surface"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Pista"},
            {"op": "add_component", "entity": "$2", "component": "collider3d"},
            // "$0" inside a patch is the surface created above.
            {"op": "update_entity", "entity": "$2", "patch": {"physics3d": {"surface": "$0"}}},
        ])),
    )
    .unwrap();
    let (ice, common, track) = (
        created[0].created[0].clone(),
        created[1].created[0].clone(),
        created[2].created[0].clone(),
    );
    fn surface(project: &Project, id: &str) -> surface::SurfaceMaterial {
        project
            .surfaces
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .clone()
    }
    let thin = surface(&project, &ice);
    assert_eq!(
        (thin.name.as_str(), thin.friction, thin.traction),
        ("Gelo fino", 0.01, 0.15),
        "the preset's values, then the ones given"
    );
    assert_eq!(surface(&project, &common).name, "Comum");
    let entity = project.scene(&scene).unwrap().entity(&track).unwrap();
    assert_eq!(
        entity.physics3d.as_ref().unwrap().surface.as_deref(),
        Some(ice.as_str())
    );

    command::apply(
        &mut project,
        &scene,
        &batch(
            json!({"op": "update_surface", "surface": common, "name": "Esteira lenta",
                      "conveyor": [1.0, 0.0, 0.0], "conveyor_space": "World",
                      "speed_multiplier": 0.8}),
        ),
    )
    .unwrap();
    let slow = surface(&project, &common);
    assert_eq!(slow.conveyor, [1., 0., 0.]);
    assert_eq!(slow.conveyor_space, surface::VelocitySpace::World);
    assert_eq!((slow.friction, slow.speed_multiplier), (1., 0.8));

    for (command, expected) in [
        (
            json!({"op": "update_surface", "surface": common, "friction": -1.0}),
            "negativos",
        ),
        (
            json!({"op": "update_surface", "surface": common, "name": ""}),
            "nome",
        ),
        (
            json!({"op": "update_surface", "surface": "nenhuma", "traction": 1.0}),
            "não existe",
        ),
        (json!({"op": "delete_surface", "surface": ice}), "em uso"),
        (json!({"op": "create_surface", "preset": "areia"}), ""),
    ] {
        let before = project.clone();
        let result = command::parse(&command)
            .map_err(|e| e.to_string())
            .and_then(|c| command::apply(&mut project, &scene, &c).map_err(|e| e.to_string()));
        let error = result.unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(project, before);
    }

    let summary = query::project_summary(&project);
    let listed = summary["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == ice.as_str())
        .unwrap()
        .clone();
    assert_eq!(listed["name"], "Gelo fino");
    assert_eq!(listed["conveyor_space"], "Local");
    assert_eq!(
        listed["used_by"],
        json!([{"entity": track, "name": "Pista", "role": "collider3d"}])
    );
    assert_eq!(
        summary["collision_groups"],
        json!([{"bit": 0, "name": "Cenário padrão"}, {"bit": 1, "name": "Personagens"},
               {"bit": 2, "name": "Áreas"}])
    );
    project.collision_groups.insert(5, "Inimigos".into());
    assert_eq!(
        query::project_summary(&project)["collision_groups"][3],
        json!({"bit": 5, "name": "Inimigos"})
    );

    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "update_entity", "entity": track, "patch": {"physics3d": {"surface": null}}},
            {"op": "delete_surface", "surface": ice},
        ])),
    )
    .unwrap();
    assert_eq!(project.surfaces.len(), 1);
}

#[test]
fn placeholders_reach_every_created_id_patches_included_and_dollars_can_be_escaped() {
    use command::Placeholder;
    assert_eq!(
        Placeholder::parse("$3"),
        Some(Placeholder::Created { command: 3, nth: 0 })
    );
    assert_eq!(
        Placeholder::parse("$0.2"),
        Some(Placeholder::Created { command: 0, nth: 2 })
    );
    assert_eq!(
        Placeholder::parse("$$1.4"),
        Some(Placeholder::Escaped("$1.4"))
    );
    assert_eq!(
        Placeholder::parse("$5.1@abc"),
        Some(Placeholder::Copied {
            command: 5,
            nth: 1,
            original: "abc"
        })
    );
    assert_eq!(
        Placeholder::parse("$2@$0.1"),
        Some(Placeholder::Copied {
            command: 2,
            nth: 0,
            original: "$0.1"
        })
    );
    assert_eq!(
        Placeholder::parse("$$2@x"),
        Some(Placeholder::Escaped("$2@x"))
    );
    for text in [
        "$",
        "$$",
        "$x",
        "$1a",
        "$0.",
        "$.1",
        "$$$1",
        "$10 de bônus",
        "1",
        "a$1",
        "$2@",
        "$@x",
        "$2.@x",
        "a$2@x",
    ] {
        assert_eq!(Placeholder::parse(text), None, "{text}");
    }

    let (mut project, scene) = project(SceneKind::ThreeD);
    let created = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "first_person"},
             "name": "Corredor"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "A"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "B"},
            {"op": "duplicate_entities", "ids": ["$1", "$2"]},
            // The second copy and the preset's camera, by "$N.M".
            {"op": "rename_entity", "entity": "$3.1", "name": "Segunda cópia"},
            {"op": "rename_entity", "entity": "$0.2", "name": "Câmera do corredor"},
            {"op": "update_entity", "entity": "$1",
             "patch": {"attributes": {"alvo": {"Object": "$2"}, "preço": {"Text": "$$5"},
                                      "aviso": {"Text": "$10 de bônus"}}}},
            {"op": "set_attribute", "entity": "$2", "name": "rótulo", "value": {"Text": "$$0.1"}},
        ])),
    )
    .unwrap();
    let pieces = &created[0].created;
    let scene_ref = project.scene(&scene).unwrap();
    assert_eq!(pieces.len(), 9, "root, look, camera and six body parts");
    assert!(
        scene_ref.entity(&pieces[0]).unwrap().character3d.is_some(),
        "root first"
    );
    assert_eq!(scene_ref.entity(&pieces[0]).unwrap().name, "Corredor");
    assert_eq!(
        scene_ref.entity(&pieces[2]).unwrap().name,
        "Câmera do corredor"
    );
    let copies = &created[3].created;
    assert_eq!(copies.len(), 2);
    assert_eq!(scene_ref.entity(&copies[1]).unwrap().name, "Segunda cópia");
    let a = scene_ref.entity(&created[1].created[0]).unwrap();
    assert_eq!(
        a.attributes["alvo"],
        Value::Object(Some(created[2].created[0].clone()))
    );
    assert_eq!(a.attributes["preço"], Value::Text("$5".into()));
    assert_eq!(a.attributes["aviso"], Value::Text("$10 de bônus".into()));
    let b = scene_ref.entity(&created[2].created[0]).unwrap();
    assert_eq!(b.attributes["rótulo"], Value::Text("$0.1".into()));

    let before = project.clone();
    for (commands, wrong) in [
        // A command cannot name itself or a later one, the first one included.
        (
            json!([{"op": "rename_entity", "entity": "$0", "name": "X"}]),
            "\"$0\"",
        ),
        (
            json!([{"op": "create_entity", "kind": {"type": "group"}},
                   {"op": "update_entity", "entity": "$0", "patch": {"name": "$7"}}]),
            "\"$$7\"",
        ),
        (
            json!([{"op": "create_entity", "kind": {"type": "group"}},
                   {"op": "rename_entity", "entity": "$0.1", "name": "X"}]),
            "\"$0.1\"",
        ),
    ] {
        let error = command::apply(&mut project, &scene, &batch(commands)).unwrap_err();
        assert!(error.message.contains(wrong), "{error}");
        assert_eq!(project, before);
    }
}

#[test]
fn the_report_names_what_each_command_created_and_the_warnings_it_added() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"},
            {"op": "set_attribute", "entity": "$0", "name": "aberta", "value": {"Bool": false}},
            {"op": "add_node", "entity": "$0", "operation": "event.input",
             "params": {"action": {"Text": "abrir"}}},
            {"op": "create_input_action", "name": "Abrir", "key": "E"},
            {"op": "create_surface", "preset": "mud"},
        ])),
    )
    .unwrap();
    let created = report["created"].as_object().unwrap();
    assert_eq!(
        created.keys().collect::<Vec<_>>(),
        ["0", "2", "3", "4"],
        "only the commands that created something"
    );
    assert_eq!(report["created"]["0"][0]["name"], "Porta");
    assert_eq!(report["created"]["2"][0]["name"], "event.input");
    assert_eq!(report["created"]["3"][0]["name"], "Abrir");
    assert_eq!(report["created"]["4"][0]["name"], "Lama");
    // Named before the action existed, the node keeps the name, which the game never presses.
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("\"abrir\" é o nome da ação de entrada"),
        "{warnings:?}"
    );
    let door = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let node = report["created"]["2"][0]["id"].as_str().unwrap().to_owned();
    let action = report["created"]["3"][0]["id"].as_str().unwrap().to_owned();

    // The warning stays in the project until fixed, but a later batch does not repeat it.
    let later = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!({"op": "rename_entity", "entity": door, "name": "Portão"})),
    )
    .unwrap();
    assert_eq!(later, json!({"created": {}}));
    assert_eq!(
        query::validation(&project)["warnings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    command::apply(
        &mut project,
        &scene,
        &batch(
            json!({"op": "set_node_params", "entity": door, "node": node,
                      "params": {"action": {"Text": action}}}),
        ),
    )
    .unwrap();
    assert_eq!(query::validation(&project), json!({"valid": true}));

    // Values the game ignores, left by update_entity or by hand, are warned about too.
    project
        .input_bindings
        .insert("pausar".into(), "Escape".into());
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(
            json!({"op": "update_entity", "entity": door, "patch": {"graph": {"nodes": [
            {"id": node, "operation": "event.input", "position": [0.0, 0.0],
             "params": {"action": {"Text": action}, "mode": {"Text": "sempre"}}}]}}}),
        ),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "the key was there before: {warnings:?}");
    assert!(warnings[0].as_str().unwrap().contains("recebeu \"sempre\""));
    let validation = query::validation(&project);
    assert_eq!(validation["valid"], true);
    assert!(
        validation["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("Escape é reservada")),
        "{validation}"
    );
}

#[test]
fn a_component_switch_aimed_at_an_object_without_it_is_warned() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Alavanca"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Ponte"},
            {"op": "add_node", "entity": "$0", "operation": "action.component",
             "params": {"target": {"Object": "$1"}, "component": "collider"}},
        ])),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    assert!(
        warning.contains("Alavanca → nó action.component")
            && warning.contains("Ponte não tem colisor nem área"),
        "{warning}"
    );
    let lever = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let bridge = report["created"]["1"][0]["id"].as_str().unwrap().to_owned();
    let node = report["created"]["2"][0]["id"].as_str().unwrap().to_owned();
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(
            json!({"op": "set_node_params", "entity": lever, "node": node,
                      "params": {"component": "platform"}}),
        ),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("Ponte não tem plataforma móvel"),
        "{warnings:?}"
    );
    // A moving platform brings its 3D collider, so both choices find what they switch.
    command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "add_component", "entity": bridge, "component": "platform"})),
    )
    .unwrap();
    assert_eq!(query::validation(&project), json!({"valid": true}));
    command::apply(
        &mut project,
        &scene,
        &batch(
            json!({"op": "set_node_params", "entity": lever, "node": node,
                      "params": {"component": "collider"}}),
        ),
    )
    .unwrap();
    assert_eq!(query::validation(&project), json!({"valid": true}));

    // A target that comes through the port is only known in the game.
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "set_node_params", "entity": lever, "node": node,
             "params": {"component": "controller"}},
            {"op": "add_node", "entity": lever, "operation": "value.object",
             "params": {"value": {"Object": bridge}}},
            {"op": "connect", "entity": lever, "from_node": "$1", "from_port": "value",
             "to_node": node, "to_port": "target"},
        ])),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
}

#[test]
fn simulations_press_only_actions_the_project_has() {
    use oxy_core::headless::{self, SimulationRequest};
    let (mut project, _) = project(SceneKind::ThreeD);
    input_actions::create(&mut project, "Pular alto", "Space").unwrap();
    let run = |inputs: serde_json::Value, setup: serde_json::Value| {
        let request: SimulationRequest = serde_json::from_value(json!({
            "seconds": 0.2, "inputs": inputs, "setup": setup,
        }))
        .unwrap();
        headless::simulate(&project, &request)
    };
    let press = |action: &str| json!([{"type": "press", "at": 0.0, "action": action}]);
    run(press("pular alto"), json!([])).expect("by name, in any case");
    let error = run(press("voar"), json!([])).unwrap_err();
    assert!(
        error.contains("\"voar\" não existe") && error.contains("Pular alto"),
        "the error lists the actions: {error}"
    );
    // An action created by setup, by "$N" or by name.
    let setup = json!([{"op": "create_input_action", "name": "Voar", "key": "V"}]);
    run(press("$0"), setup.clone()).unwrap();
    run(press("Voar"), setup).unwrap();
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

fn box_of(project: &Project, scene: &str, id: &str) -> ([f32; 3], [f32; 3]) {
    let collider = project
        .scene(scene)
        .unwrap()
        .entity(id)
        .unwrap()
        .physics3d
        .clone()
        .unwrap();
    match collider.shape {
        CollisionShape::Box { size } => (size, collider.center),
        other => panic!("box expected: {other:?}"),
    }
}

#[test]
fn fit_collider_wraps_the_shape_or_its_pieces_and_a_box_left_behind_is_warned() {
    use glam::Vec3;
    use oxy_core::spatial;
    let (mut project, scene) = project(SceneKind::ThreeD);
    // The collider was added before the dimensions changed: it keeps the old box.
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
             "name": "Pouso", "position": [0.0, 0.7, -93.5]},
            {"op": "add_component", "entity": "$0", "component": "collider3d"},
            {"op": "update_entity", "entity": "$0", "patch": {"dimensions": [9.0, 0.4, 7.0]}},
        ])),
    )
    .unwrap();
    let pad = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    assert!(
        warning.starts_with("Comando 2 (update_entity, o 3º do lote): Pouso")
            && warning.contains("[9, 0.4, 7]")
            && warning.contains("[1, 1, 1]")
            && warning.contains(&format!("fit_collider {{\"entity\": \"{pad}\"}}")),
        "{warning}"
    );
    let fitted = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!({"op": "fit_collider", "entity": pad})),
    )
    .unwrap();
    assert_eq!(fitted, json!({"created": {}}), "nothing left to warn");
    assert_eq!(box_of(&project, &scene, &pad), ([9., 0.4, 7.], [0.; 3]));

    // A fit later in the batch settles it, and a scale change needs none: the box follows it.
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}},
            {"op": "add_component", "entity": "$0", "component": "collider3d"},
            {"op": "update_entity", "entity": "$0", "patch": {"dimensions": [2.0, 3.0, 4.0]}},
            {"op": "fit_collider", "entity": "$0", "margin": 0.5},
            {"op": "set_transform", "entity": pad, "scale": [2.0, 1.0, 1.0]},
        ])),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
    let cube = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(box_of(&project, &scene, &cube).0, [3., 4., 5.]);

    // A fitted box is followed too: a plane's, as thin as allowed, and one with a margin.
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Plane"}, "name": "Piso"},
            {"op": "add_component", "entity": "$0", "component": "collider3d"},
            {"op": "fit_collider", "entity": "$0"},
        ])),
    )
    .unwrap();
    let floor = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    assert!(near(box_of(&project, &scene, &floor).0, [1., 0.001, 1.]));
    for entity in [&floor, &cube] {
        let report = command::apply_and_report(
            &mut project,
            &scene,
            &batch(json!({"op": "update_entity", "entity": entity,
                          "patch": {"dimensions": [20.0, 1.0, 20.0]}})),
        )
        .unwrap();
        let warning = report["warnings"][0].as_str().unwrap_or_default();
        assert!(
            warning.contains(&format!("({entity}) agora mede [20, 1, 20]"))
                && warning.contains("fit_collider"),
            "{report}"
        );
    }

    // With children: the pieces of a turned and scaled group, in the group's own axes.
    let applied = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Seção",
             "position": [10.0, 0.0, 0.0]},
            {"op": "set_transform", "entity": "$0", "rotation": [0.0, 90.0, 0.0],
             "scale": [2.0, 2.0, 2.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "parent": "$0",
             "position": [-2.0, 0.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "parent": "$0",
             "position": [3.0, 0.0, 0.0]},
            {"op": "add_component", "entity": "$0", "component": "sensor3d"},
            {"op": "fit_collider", "entity": "$0", "children": true, "margin": 0.2},
        ])),
    )
    .unwrap();
    let group = applied[0].created[0].clone();
    let (size, center) = box_of(&project, &scene, &group);
    // From -2.5 to 3.5 along the group's X, plus 0.2 m (0.1 at scale 2) on every side.
    assert!(near(size, [6.2, 1.2, 1.2]), "{size:?}");
    assert!(near(center, [0.5, 0., 0.]), "{center:?}");
    let level = project.scene(&scene).unwrap();
    let collider = level.entity(&group).unwrap().physics3d.as_ref().unwrap();
    assert!(collider.sensor, "a sensor stays a sensor");
    let (bounds, _) =
        spatial::collider3d_bounds(collider, level.world_matrix(&group).unwrap()).unwrap();
    let shown = spatial::visual_bounds(level, &level.descendants(&group)).unwrap();
    assert!(
        bounds.min.abs_diff_eq(shown.min - Vec3::splat(0.2), 1e-4)
            && bounds.max.abs_diff_eq(shown.max + Vec3::splat(0.2), 1e-4),
        "{bounds:?} / {shown:?}"
    );

    // What it cannot fit is refused, saying why.
    let others = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Sphere"}, "name": "Bola"},
            {"op": "add_component", "entity": "$0", "component": "collider3d"},
            {"op": "update_entity", "entity": "$0",
             "patch": {"physics3d": {"shape": {"Box": null, "Sphere": {"radius": 0.5}}}}},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Solta"},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "first_person"},
             "name": "Jogador"},
        ])),
    )
    .unwrap();
    let (ball, loose, player) = (
        others[0].created[0].clone(),
        others[3].created[0].clone(),
        others[4].created[0].clone(),
    );
    for (command, expected) in [
        (
            json!({"op": "fit_collider", "entity": group}),
            "Seção não tem forma própria; com children: true",
        ),
        (
            json!({"op": "fit_collider", "entity": ball, "shape": "convex", "children": true}),
            "O colisor convex usa a forma do próprio Bola, sem as peças dentro dele",
        ),
        (
            json!({"op": "fit_collider", "entity": loose}),
            "Solta não tem colisor",
        ),
        (
            json!({"op": "fit_collider", "entity": player}),
            "Personagem 3D",
        ),
        (
            json!({"op": "fit_collider", "entity": pad, "margin": -1.0}),
            "margin",
        ),
        (
            json!({"op": "fit_collider", "entity": "fantasma"}),
            "Objeto fantasma não existe",
        ),
    ] {
        let before = project.clone();
        let error = command::apply(&mut project, &scene, &batch(command)).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert_eq!(project, before);
    }

    // The 2D box collider takes the world box, as the editor's Ajustar ao objeto does.
    let (mut flat, plane) = self::project(SceneKind::TwoD);
    let report = command::apply_and_report(
        &mut flat,
        &plane,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Rectangle"},
             "name": "Chão", "position": [1.0, 2.0, 0.0]},
            {"op": "add_component", "entity": "$0", "component": "collider2d"},
            {"op": "update_entity", "entity": "$0", "patch": {"dimensions": [4.0, 2.0, 1.0]}},
        ])),
    )
    .unwrap();
    assert!(
        report["warnings"][0].as_str().unwrap().contains("Chão"),
        "{report}"
    );
    let floor = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    command::apply(
        &mut flat,
        &plane,
        &batch(json!({"op": "fit_collider", "entity": floor, "margin": 0.5})),
    )
    .unwrap();
    let level = flat.scene(&plane).unwrap();
    let bounds = spatial::collider_bounds(level, &floor).unwrap();
    let shown = spatial::visual_bounds(level, std::slice::from_ref(&floor)).unwrap();
    for axis in 0..2 {
        assert!(
            (bounds.min[axis] - (shown.min[axis] - 0.5)).abs() < 1e-4,
            "{bounds:?}"
        );
        assert!(
            (bounds.max[axis] - (shown.max[axis] + 0.5)).abs() < 1e-4,
            "{bounds:?}"
        );
    }
    // Fitted, it is still followed.
    let report = command::apply_and_report(
        &mut flat,
        &plane,
        &batch(json!({"op": "update_entity", "entity": floor,
                      "patch": {"dimensions": [8.0, 2.0, 1.0]}})),
    )
    .unwrap();
    assert!(
        report["warnings"][0]
            .as_str()
            .unwrap_or_default()
            .contains("Chão"),
        "{report}"
    );
}

#[test]
fn an_input_parameter_takes_the_action_name_and_keeps_its_id() {
    use oxy_core::headless::{self, SimulationRequest};
    let (mut project, scene) = project(SceneKind::ThreeD);
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_input_action", "name": "Pausar", "key": "P"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Controle"},
            {"op": "add_node", "entity": "$1", "operation": "event.input",
             "params": {"action": {"Text": "pausar"}}},
            {"op": "add_node", "entity": "$1", "operation": "input.read",
             "params": {"action": "Pausar"}},
            {"op": "add_node", "entity": "$1", "operation": "debug.message",
             "params": {"message": {"Text": "Pausado"}}},
            {"op": "connect", "entity": "$1", "from_node": "$2", "from_port": "exec",
             "to_node": "$4", "to_port": "exec"},
        ])),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
    let action = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let control = report["created"]["1"][0]["id"].as_str().unwrap().to_owned();
    let nodes = |project: &Project| {
        project
            .scene(&scene)
            .unwrap()
            .entity(&control)
            .unwrap()
            .graph
            .nodes
            .clone()
    };
    assert_eq!(
        nodes(&project)[0].text("action"),
        action,
        "the ID, as the game reads"
    );
    assert_eq!(nodes(&project)[1].text("action"), action);
    // So the game fires it.
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 0.3, "inputs": [{"type": "press", "at": 0.1, "action": "Pausar"}],
    }))
    .unwrap();
    let simulated = headless::simulate(&project, &request).unwrap();
    assert!(
        simulated.logs.iter().any(|l| l.text.contains("Pausado")),
        "{:?}",
        simulated.logs
    );

    // set_node_params too; an action the project lacks stays, warned, and a name two actions
    // share is refused, as by the input action commands.
    let node = nodes(&project)[0].id.clone();
    let set = |action: &str| {
        batch(
            json!({"op": "set_node_params", "entity": control, "node": node,
                     "params": {"action": {"Text": action}}}),
        )
    };
    command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "create_input_action", "name": "Pular alto", "key": "Space"})),
    )
    .unwrap();
    command::apply(&mut project, &scene, &set("PULAR ALTO")).unwrap();
    let jump = input_actions::find(&project, "Pular alto").unwrap();
    assert_eq!(nodes(&project)[0].text("action"), jump);
    let report = command::apply_and_report(&mut project, &scene, &set("voar")).unwrap();
    assert_eq!(nodes(&project)[0].text("action"), "voar");
    assert!(
        report["warnings"][0]
            .as_str()
            .unwrap()
            .contains("a ação de entrada \"voar\" não existe"),
        "{report}"
    );
    project
        .input_bindings
        .insert("pausa_antiga".into(), "O".into());
    project
        .input_labels
        .insert("pausa_antiga".into(), "Pausar".into());
    let before = project.clone();
    let error = command::apply(&mut project, &scene, &set("Pausar")).unwrap_err();
    assert!(error.message.contains("Há 2 ações de entrada"), "{error}");
    assert_eq!(project, before);

    // A node that keeps a name, as nodes added before did, is warned for what it is.
    let mut old = project.clone();
    old.scene_mut(&scene)
        .unwrap()
        .entity_mut(&control)
        .unwrap()
        .graph
        .nodes[0]
        .params
        .insert("action".into(), Value::Text("Pular alto".into()));
    let warnings = command::warnings(&old);
    assert!(
        warnings.iter().any(|w| w
            .message
            .contains(&format!("é o nome da ação de entrada {jump}"))),
        "{warnings:?}"
    );
    // A name two actions share: they exist, and the warning says so.
    old.scene_mut(&scene)
        .unwrap()
        .entity_mut(&control)
        .unwrap()
        .graph
        .nodes[0]
        .params
        .insert("action".into(), Value::Text("Pausar".into()));
    let warnings: Vec<String> = command::warnings(&old)
        .into_iter()
        .map(|w| w.message)
        .collect();
    let shared = warnings
        .iter()
        .find(|w| w.contains("\"Pausar\""))
        .unwrap_or_else(|| panic!("{warnings:?}"));
    assert!(
        shared.contains("\"Pausar\" é o nome de 2 ações de entrada (")
            && shared.contains(&action)
            && shared.contains("pausa_antiga")
            && !shared.contains("não existe"),
        "{shared}"
    );
}

#[test]
fn a_key_bound_to_two_actions_is_warned_by_the_batch_and_listed_in_the_summary() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let mut run = |command: serde_json::Value| {
        command::apply_and_report(&mut project, &scene, &batch(command)).unwrap()
    };
    let back = run(
        json!({"op": "create_input_action", "name": "Voltar ao checkpoint",
                          "key": "R"}),
    );
    assert!(back.get("warnings").is_none(), "{back}");
    let back = back["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    // "r" is the key R.
    let report = run(json!({"op": "create_input_action", "name": "Reiniciar fase", "key": "r"}));
    let restart = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let warning = report["warnings"][0].as_str().unwrap().to_owned();
    assert!(
        warning.starts_with("A tecla R é de 2 ações de entrada")
            && warning.contains(&format!("{back} (Voltar ao checkpoint)"))
            && warning.contains(&format!("{restart} (Reiniciar fase)"))
            && warning.contains("set_input_key"),
        "{warning}"
    );
    let summary = query::project_summary(&project);
    let listed = |summary: &serde_json::Value, id: &str| {
        summary["input_actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        listed(&summary, &restart)["shares_key_with"],
        json!([{"id": back, "name": "Voltar ao checkpoint"}])
    );
    // Projects may share a key on purpose: validation leaves it out.
    assert_eq!(query::validation(&project), json!({"valid": true}));
    let mut run = |command: serde_json::Value| {
        command::apply_and_report(&mut project, &scene, &batch(command)).unwrap()
    };
    assert_eq!(
        run(json!({"op": "set_input_key", "action": "Reiniciar fase", "key": "K"})),
        json!({"created": {}})
    );
    // set_input_key warns the same way.
    let report = run(json!({"op": "set_input_key", "action": back, "key": "K"}));
    assert!(
        report["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("A tecla K é de 2 ações de entrada"),
        "{report}"
    );
    run(json!({"op": "set_input_key", "action": back, "key": "R"}));
    let summary = query::project_summary(&project);
    assert!(listed(&summary, &restart).get("shares_key_with").is_none());

    // Only a batch that adds an action to a shared key warns, not one that takes one away.
    let mut run = |command: serde_json::Value| {
        command::apply_and_report(&mut project, &scene, &batch(command)).unwrap()
    };
    assert!(
        run(json!({"op": "set_input_key", "action": back, "key": "K"}))
            .get("warnings")
            .is_some()
    );
    let third = run(json!({"op": "create_input_action", "name": "Recarregar", "key": "K"}));
    assert!(
        third["warnings"][0]
            .as_str()
            .unwrap()
            .starts_with("A tecla K é de 3 ações de entrada"),
        "{third}"
    );
    assert_eq!(
        run(json!({"op": "delete_input_action", "action": "Recarregar"})),
        json!({"created": {}})
    );
}

#[test]
fn plain_values_follow_the_catalog_type_and_the_rest_is_refused_in_portuguese() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Armadilha"},
            {"op": "add_node", "entity": "$0", "operation": "action.damage",
             "params": {"attribute": "Vida", "amount": 3, "once": false}},
            {"op": "add_node", "entity": "$0", "operation": "event.input",
             "params": {"mode": "held"}},
        ])),
    )
    .unwrap();
    let trap = applied[0].created[0].clone();
    let damage = applied[1].created[0].clone();
    let nodes = &project
        .scene(&scene)
        .unwrap()
        .entity(&trap)
        .unwrap()
        .graph
        .nodes;
    assert_eq!(nodes[0].params["attribute"], Value::Text("Vida".into()));
    assert_eq!(nodes[0].params["amount"], Value::Number(3.));
    assert_eq!(nodes[0].params["once"], Value::Bool(false));
    assert_eq!(nodes[1].text("mode"), "held");

    // Parameters whose input takes any type (attribute.set's value, condition.compare's a and
    // b) have a number as default, not as type: a plain value is read by its form.
    let applied = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "add_node", "entity": trap, "operation": "attribute.set",
             "params": {"attribute": "Checkpoint", "value": "Outro"}},
            {"op": "add_node", "entity": trap, "operation": "condition.compare",
             "params": {"a": 5, "b": true, "operator": "=="}},
        ])),
    )
    .unwrap();
    let node = |id: &str| {
        project
            .scene(&scene)
            .unwrap()
            .entity(&trap)
            .unwrap()
            .graph
            .node(id)
            .unwrap()
            .clone()
    };
    let set = node(&applied[0].created[0]);
    assert_eq!(set.params["value"], Value::Text("Outro".into()));
    let compare = node(&applied[1].created[0]);
    assert_eq!(compare.params["a"], Value::Number(5.));
    assert_eq!(compare.params["b"], Value::Bool(true));

    let add = |params: serde_json::Value| json!({"op": "add_node", "entity": trap, "operation": "action.damage", "params": params});
    for (command, expected) in [
        // An object is never read from a plain text.
        (
            add(json!({"target": "abc"})),
            "O parâmetro target do nó action.damage precisa de um valor marcado com o tipo, \
             como {\"Object\":\"abc\"}; recebeu \"abc\"",
        ),
        (
            add(json!({"amount": {"number": 3}})),
            "como {\"Number\":3}; recebeu {\"number\":3} (o nome do tipo começa com maiúscula)",
        ),
        (add(json!({"amount": "muito"})), "como {\"Number\":1}"),
        (
            add(json!({"amount": {"Number": "muito"}})),
            "como {\"Number\":1}; recebeu {\"Number\":\"muito\"}",
        ),
        (
            json!({"op": "set_node_params", "entity": trap, "node": damage,
                   "params": {"once": "sim"}}),
            "O parâmetro once do nó action.damage precisa de um valor marcado com o tipo, como \
             {\"Bool\":true}",
        ),
        // Attributes have no type in the catalog: their values are always tagged.
        (
            json!({"op": "set_attribute", "entity": trap, "name": "vida", "value": 3}),
            "O atributo vida precisa de um valor marcado com o tipo, como {\"Number\":3}; \
             recebeu 3",
        ),
        (
            json!({"op": "add_node", "entity": trap, "operation": "event.input",
                   "params": {"mode": "segurando"}}),
            "recebeu \"segurando\"",
        ),
        // A parameter of any type is read only by an obvious form, and the example follows it.
        (
            json!({"op": "add_node", "entity": trap, "operation": "attribute.set",
                   "params": {"value": [1, 2, 3]}}),
            "O parâmetro value do nó attribute.set precisa de um valor marcado com o tipo, como \
             {\"Vector3\":[1,2,3]}",
        ),
    ] {
        let before = project.clone();
        let error = command::apply(&mut project, &scene, &batch(command)).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert_eq!(project, before);
    }
    // Other enums: in Portuguese, with the spelling when only the case differs.
    let error = command::parse(&json!(
        {"op": "apply_movement_profile", "entity": trap, "profile": "Parkour"}
    ))
    .unwrap_err();
    assert!(
        error.starts_with("Comando 0 (apply_movement_profile, o 1º do lote) não reconhecido")
            && error.contains("valor `Parkour` desconhecido")
            && error.contains("escreva `parkour`")
            && !error.contains("unknown variant"),
        "{error}"
    );
    let error =
        command::parse(&json!({"op": "rename_entity", "entity": trap, "nome": "X"})).unwrap_err();
    assert!(error.contains("campo `nome` desconhecido"), "{error}");
    // Wrong types and lengths, too.
    for (command, expected) in [
        (
            json!({"op": "set_transform", "entity": trap, "position": [1, 2]}),
            "não reconhecido: tamanho errado: 2 itens, esperava uma lista de 3 itens",
        ),
        (
            json!({"op": "set_node_params", "entity": trap, "node": damage, "params": [1]}),
            "não reconhecido: tipo errado: recebeu uma lista, esperava um objeto",
        ),
        (
            json!("texto"),
            "não reconhecido: tipo errado: recebeu o texto \"texto\", esperava um comando, um \
             objeto com op",
        ),
        (
            json!({"op": "duplicate_entities", "ids": [trap], "count": -1}),
            "não reconhecido: valor inválido: recebeu o número `-1`, esperava um número inteiro, \
             0 ou mais",
        ),
    ] {
        let error = command::parse(&command).unwrap_err();
        assert!(error.ends_with(expected), "{error}");
    }
}

#[test]
fn the_current_scene_wires_into_the_scene_change_and_the_catalog_lists_both() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_input_action", "name": "Reiniciar", "key": "R"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Controle"},
            {"op": "add_node", "entity": "$1", "operation": "event.input",
             "params": {"action": "Reiniciar"}},
            {"op": "add_node", "entity": "$1", "operation": "scene.current"},
            {"op": "add_node", "entity": "$1", "operation": "action.scene"},
            {"op": "connect", "entity": "$1", "from_node": "$2", "from_port": "exec",
             "to_node": "$4", "to_port": "exec"},
            {"op": "connect", "entity": "$1", "from_node": "$3", "from_port": "scene",
             "to_node": "$4", "to_port": "scene"},
        ])),
    )
    .unwrap();
    // An empty scene parameter is no warning: the input gives the scene.
    assert!(report.get("warnings").is_none(), "{report}");
    validate_project(&project).unwrap();
    let id = |index: &str| {
        report["created"][index][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (control, current, change) = (id("1"), id("3"), id("4"));
    let graph = &project
        .scene(&scene)
        .unwrap()
        .entity(&control)
        .unwrap()
        .graph;
    let summary = query::graph_summary(&project, graph);
    assert!(
        summary["edges"]
            .as_array()
            .unwrap()
            .contains(&json!("scene.current.scene -> action.scene.scene")),
        "{summary}"
    );
    // The scene is text, and the input takes one connection.
    for (to_port, refusal) in [
        ("seconds", "Conexão incompatível: Texto → Número"),
        ("scene", "já possui uma conexão de dados"),
    ] {
        let before = project.clone();
        let to_node = if to_port == "scene" {
            change.as_str()
        } else {
            "$0"
        };
        let error = command::apply(
            &mut project,
            &scene,
            &batch(json!([
                {"op": "add_node", "entity": control, "operation": "control.wait"},
                {"op": "connect", "entity": control, "from_node": current, "from_port": "name",
                 "to_node": to_node, "to_port": to_port},
            ])),
        )
        .unwrap_err();
        assert!(error.to_string().contains(refusal), "{error}");
        assert_eq!(project, before);
    }

    let catalog = |args: serde_json::Value| query::catalog_query(&args).unwrap();
    let categories = catalog(json!({}))["categories"].clone();
    let categories: Vec<&str> = categories
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    let game = categories.iter().position(|c| *c == "Jogo").unwrap();
    assert_eq!(
        categories[game - 1..=game + 2],
        ["Ações", "Jogo", "Som", "Controle"],
        "{categories:?}"
    );
    let nodes = catalog(json!({"category": "Jogo"}))["nodes"].clone();
    let current = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["operation"] == "scene.current")
        .unwrap();
    assert_eq!(current["inputs"], json!([]));
    assert_eq!(
        current["outputs"],
        json!([
            {"id": "scene", "label": "Cena", "type": "text"},
            {"id": "name", "label": "Nome", "type": "text"},
        ])
    );
    assert_eq!(current["params"], json!([]));
    let change = catalog(json!({"operations": ["action.scene"]}))["nodes"][0].clone();
    assert_eq!(
        change["inputs"],
        json!([
            {"id": "exec", "label": "Executar", "type": "exec"},
            {"id": "scene", "label": "Cena", "type": "text"},
        ])
    );
    assert_eq!(change["params"][0]["id"], "scene");
    assert_eq!(change["params"][0]["ref"], "scene");
}

#[test]
fn a_copied_scene_change_fed_by_its_input_is_not_warned() {
    let (mut project, first) = project(SceneKind::ThreeD);
    // Both name the scene they are in, but the second takes the scene from Cena atual.
    let report = command::apply_and_report(
        &mut project,
        &first,
        &batch(json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Fixo"},
            {"op": "add_node", "entity": "$1", "operation": "action.scene",
             "params": {"scene": first}},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Ligado"},
            {"op": "add_node", "entity": "$3", "operation": "action.scene",
             "params": {"scene": first}},
            {"op": "add_node", "entity": "$3", "operation": "scene.current"},
            {"op": "connect", "entity": "$3", "from_node": "$5", "from_port": "scene",
             "to_node": "$4", "to_port": "scene"},
            {"op": "copy_entities", "ids": ["$1", "$3"], "to_scene": "$0"},
        ])),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    let copies = &report["created"]["7"];
    let (fixed, wired) = (
        copies[0]["id"].as_str().unwrap(),
        copies[1]["id"].as_str().unwrap(),
    );
    assert!(
        warning.contains(&format!("Fixo ({fixed})"))
            && !warning.contains(wired)
            && warning.contains("ou ligue a saída scene de um scene.current à entrada scene"),
        "{warning}"
    );
}

#[test]
fn a_scene_named_only_where_the_input_decides_can_be_deleted() {
    let (mut project, first) = project(SceneKind::ThreeD);
    // A restart wired to Cena atual whose ignored parameter still names "Fase 2", in a scene
    // and in a model.
    let report = command::apply_and_report(
        &mut project,
        &first,
        &batch(json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Reinício"},
            {"op": "add_node", "entity": "$1", "operation": "action.scene",
             "params": {"scene": "$0"}},
            {"op": "add_node", "entity": "$1", "operation": "scene.current"},
            {"op": "connect", "entity": "$1", "from_node": "$3", "from_port": "scene",
             "to_node": "$2", "to_port": "scene"},
            {"op": "save_model", "entity": "$1", "name": "Reinício salvo"},
        ])),
    )
    .unwrap();
    let id = |index: &str| {
        report["created"][index][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (second, owner) = (id("0"), id("1"));
    assert!(editing::scene_references(&project, &second).is_empty());

    // Unconnected, the same parameter guards the scene.
    let mut unwired = project.clone();
    unwired.scenes[0]
        .entity_mut(&owner)
        .unwrap()
        .graph
        .edges
        .clear();
    assert_eq!(
        editing::scene_references(&unwired, &second),
        ["Cena 3D → Reinício → Mudar de cena"]
    );
    assert!(editing::delete_scene(&mut unwired, &second).is_err());

    command::apply(
        &mut project,
        &first,
        &batch(json!({"op": "delete_scene", "scene": second})),
    )
    .unwrap();
    validate_project(&project).unwrap();
    assert!(project.scene(&second).is_none());
    let model = project
        .assets
        .iter()
        .find(|asset| asset.name == "Reinício salvo")
        .and_then(|asset| asset.model.as_ref())
        .unwrap();
    for e in [project.scenes[0].entity(&owner).unwrap(), &model[0]] {
        let node = e
            .graph
            .nodes
            .iter()
            .find(|n| n.operation == "action.scene")
            .unwrap();
        // The parameter goes back to its default; the input still gives the scene.
        assert_eq!(node.text("scene"), "", "{}", e.name);
        assert_eq!(e.graph.edges.len(), 1);
    }
}

#[test]
fn a_scene_reference_holds_its_scene_like_a_scene_change_and_the_catalog_lists_it() {
    let (mut project, first) = project(SceneKind::ThreeD);
    // The scene change takes its scene from the reference, so only the reference names it.
    let report = command::apply_and_report(
        &mut project,
        &first,
        &batch(json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Portal"},
            {"op": "add_node", "entity": "$1", "operation": "value.scene",
             "params": {"scene": "$0"}},
            {"op": "add_node", "entity": "$1", "operation": "action.scene"},
            {"op": "connect", "entity": "$1", "from_node": "$2", "from_port": "value",
             "to_node": "$3", "to_port": "scene"},
        ])),
    )
    .unwrap();
    assert!(report.get("warnings").is_none(), "{report}");
    validate_project(&project).unwrap();
    let id = |index: &str| {
        report["created"][index][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (second, portal, reference) = (id("0"), id("1"), id("2"));
    assert_eq!(
        editing::scene_references(&project, &second),
        ["Cena 3D → Portal → Referência de cena"]
    );
    let before = project.clone();
    let error = command::apply(
        &mut project,
        &first,
        &batch(json!({"op": "delete_scene", "scene": second})),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Cena 3D → Portal → Referência de cena"),
        "{error}"
    );
    assert_eq!(project, before);

    // Pointed at the scene it is in and copied to Fase 2, it still gives the first scene.
    let report = command::apply_and_report(
        &mut project,
        &first,
        &batch(json!([
            {"op": "set_node_params", "entity": portal, "node": reference,
             "params": {"scene": first}},
            {"op": "copy_entities", "ids": [portal], "to_scene": second},
        ])),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    let copy = report["created"]["1"][0]["id"].as_str().unwrap();
    let copied = project
        .scene(&second)
        .unwrap()
        .entity(copy)
        .unwrap()
        .graph
        .nodes
        .iter()
        .find(|n| n.operation == "value.scene")
        .unwrap();
    assert_eq!(copied.text("scene"), first);
    assert!(
        warning.contains(&format!("Portal ({copy}) → nó value.scene ({})", copied.id))
            && warning.contains(&format!("set_node_params com scene {{\"Text\": \"{second}\"}}"))
            // The reference has no input to wire Cena atual into.
            && !warning.contains("scene.current"),
        "{warning}"
    );

    let catalog = |args: serde_json::Value| query::catalog_query(&args).unwrap();
    let data = catalog(json!({"category": "Dados"}))["nodes"].clone();
    let data = data.as_array().unwrap();
    let at = |operation: &str| {
        data.iter()
            .position(|n| n["operation"] == operation)
            .unwrap()
    };
    assert_eq!(at("value.scene"), at("value.object") + 1);
    let node = &data[at("value.scene")];
    assert_eq!(node["label"], "Referência de cena");
    assert_eq!(node["inputs"], json!([]));
    assert_eq!(
        node["outputs"],
        json!([{"id": "value", "label": "Cena", "type": "text"}])
    );
    assert_eq!(
        node["params"],
        json!([{"id": "scene", "label": "Cena", "default": {"Text": ""}, "ref": "scene"}])
    );
}

#[test]
fn a_copied_scene_reference_leaves_the_wiring_hint_to_the_nodes_with_the_input() {
    let (mut project, first) = project(SceneKind::ThreeD);
    // A restart and a reference to the scene they are in, which only the restart can take from
    // Cena atual.
    let report = command::apply_and_report(
        &mut project,
        &first,
        &batch(json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Reinício"},
            {"op": "add_node", "entity": "$1", "operation": "action.scene",
             "params": {"scene": first}},
            {"op": "add_node", "entity": "$1", "operation": "value.scene",
             "params": {"scene": first}},
            {"op": "copy_entities", "ids": ["$1"], "to_scene": "$0"},
        ])),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    assert!(
        warning.contains("ainda nomeiam Cena 3D, a cena de onde vieram")
            && warning.contains("nó action.scene")
            && warning.contains("nó value.scene")
            && warning.contains(
                ", ou, nos nós action.scene, ligue a saída scene de um scene.current à entrada \
                 scene, para que abram a cena em que estiverem"
            ),
        "{warning}"
    );
}

#[test]
fn game_variables_are_created_updated_and_deleted_by_command() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands = batch(json!([
        {"op": "create_variable", "name": " Moedas ", "value": 0},
        {"op": "create_variable", "name": "Fase atual", "value": "Início", "persistent": true},
        {"op": "create_variable", "name": "Ligado", "value": false},
        {"op": "create_variable", "name": "Saída", "value": {"Vector3": [0, 1, 0]}},
        {"op": "create_entity", "kind": {"type": "group"}, "name": "Contador"},
        {"op": "add_node", "entity": "$4", "operation": "variable.set",
         "params": {"variable": "moedas", "key": "Fase 1", "value": 5}},
        {"op": "add_node", "entity": "$4", "operation": "variable.get",
         "params": {"variable": "$1"}},
    ]));
    assert_eq!(commands[0].label(), "Criar variável do jogo");
    let before = command::batch_warnings(&project);
    let applied = command::apply(&mut project, &scene, &commands).unwrap();
    let id = |index: usize| applied[index].created[0].clone();
    let (coins, stage, entity, set) = (id(0), id(1), id(4), id(5));
    let declared: Vec<_> = project
        .variables
        .iter()
        .map(|v| (v.id.clone(), v.name.as_str(), v.value.clone(), v.persistent))
        .collect();
    assert_eq!(
        declared,
        [
            (coins.clone(), "Moedas", Value::Number(0.), false),
            (
                stage.clone(),
                "Fase atual",
                Value::Text("Início".into()),
                true
            ),
            (id(2), "Ligado", Value::Bool(false), false),
            (id(3), "Saída", Value::Vector3([0., 1., 0.]), false),
        ]
    );
    // Nodes keep the ID, whether given by name or by ID.
    let nodes = &project
        .scene(&scene)
        .unwrap()
        .entity(&entity)
        .unwrap()
        .graph
        .nodes;
    assert_eq!(nodes[0].text("variable"), coins);
    assert_eq!(nodes[0].text("key"), "Fase 1");
    assert_eq!(nodes[1].text("variable"), stage);
    // The answer names what was created.
    let report = command::report(&project, &applied, &before);
    assert_eq!(report["created"]["0"][0]["name"], "Moedas");
    assert!(report.get("warnings").is_none(), "{report}");

    for (command, expected) in [
        (
            json!({"op": "create_variable", "name": "MOEDAS", "value": 1}),
            "Já existe uma variável do jogo chamada \"Moedas\"",
        ),
        (
            json!({"op": "create_variable", "name": " ", "value": 1}),
            "nome",
        ),
        (
            json!({"op": "create_variable", "name": "Moedas · Fase", "value": 1}),
            "não pode ter ·",
        ),
        (
            json!({"op": "create_variable", "name": "Alvo", "value": {"Object": null}}),
            "não guarda objetos",
        ),
        (
            json!({"op": "create_variable", "name": "Ponto", "value": [1, 2, 3]}),
            "como {\"Vector3\":[1,2,3]}",
        ),
        (
            json!({"op": "update_variable", "variable": "Moedas", "value": "dez"}),
            "como {\"Number\":1}",
        ),
        (
            json!({"op": "update_variable", "variable": "Ligado", "name": "fase ATUAL"}),
            "Já existe",
        ),
        (
            json!({"op": "update_variable", "variable": "Vidas", "value": 3}),
            "A variável do jogo \"Vidas\" não existe no projeto. Variáveis: Moedas, Fase atual, \
             Ligado, Saída",
        ),
        (
            json!({"op": "delete_variable", "variable": "moedas"}),
            "Cena 3D → Contador → nó Alterar variável do jogo",
        ),
        (
            json!({"op": "add_node", "entity": entity, "operation": "variable.get",
                   "params": {"variable": "Vidas"}}),
            "não existe no projeto",
        ),
    ] {
        let before = project.clone();
        let error = command::apply(&mut project, &scene, &batch(command)).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert_eq!(project, before);
    }

    // A plain value is read as the variable's type; a tagged one may change the type.
    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "update_variable", "variable": "moedas", "value": 10, "persistent": true},
            {"op": "update_variable", "variable": "Ligado", "name": "Aceso", "value": true},
            {"op": "update_variable", "variable": stage, "value": {"Number": 1}},
        ])),
    )
    .unwrap();
    let coins_now = oxy_core::variables::get(&project, &coins).unwrap();
    assert_eq!(
        (&coins_now.value, coins_now.persistent),
        (&Value::Number(10.), true)
    );
    assert_eq!(project.variables[2].name, "Aceso");
    assert_eq!(project.variables[2].value, Value::Bool(true));
    assert_eq!(project.variables[1].value, Value::Number(1.));

    let summary = query::project_summary(&project);
    assert_eq!(
        summary["variables"][0],
        json!({"id": coins, "name": "Moedas", "type": "Number", "value": {"Number": 10.0},
               "persistent": true,
               "used_by": [{"entity": entity, "name": "Contador", "role": "variable.set"}]})
    );
    assert_eq!(summary["variables"][3]["type"], "Vector3");

    // A constant of another type than the variable's is warned: the game would stop there.
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(
            json!([{"op": "set_node_params", "entity": entity, "node": set,
                        "params": {"value": "cinco"}}]),
        ),
    )
    .unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].as_str().unwrap().contains(
            "o value é texto, mas a variável do jogo Moedas guarda número; no jogo, o nó para \
             com erro"
        ),
        "{warnings:?}"
    );
    // A value from a port is known only in the game.
    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "add_node", "entity": entity, "operation": "value.number"},
            {"op": "connect", "entity": entity, "from_node": "$0", "from_port": "value",
             "to_node": set, "to_port": "value"},
        ])),
    )
    .unwrap();
    assert!(command::warnings(&project).is_empty());

    // Unused, a variable can go.
    command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "remove_node", "entity": entity, "node": set},
            {"op": "delete_variable", "variable": "Moedas"},
        ])),
    )
    .unwrap();
    assert!(oxy_core::variables::get(&project, &coins).is_none());
    assert_eq!(project.variables.len(), 3);
    assert!(validate_project(&project).is_ok());
}

/// The rectangles of `graph`'s nodes, as the editor draws them, never overlap.
fn assert_nodes_apart(graph: &graph::Graph) {
    let rects: Vec<[f32; 4]> = graph
        .nodes
        .iter()
        .map(|n| {
            let [w, h] = graph::node_extent(graph::definition(&n.operation));
            [
                n.position[0],
                n.position[1],
                n.position[0] + w,
                n.position[1] + h,
            ]
        })
        .collect();
    for (k, a) in rects.iter().enumerate() {
        for b in &rects[k + 1..] {
            assert!(
                a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1],
                "{a:?} overlaps {b:?}"
            );
        }
    }
}

#[test]
fn nodes_added_without_a_position_are_laid_out_when_the_batch_ends() {
    use graph::layout::{BAND_GAP, COLUMN_STEP};
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"},
            {"op": "add_node", "entity": "$0", "operation": "debug.message",
             "position": [900.0, -300.0]},
            {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
            {"op": "add_node", "entity": "$0", "operation": "condition.branch"},
            {"op": "add_node", "entity": "$0", "operation": "value.bool"},
            {"op": "add_node", "entity": "$0", "operation": "debug.message"},
            {"op": "connect", "entity": "$0", "from_node": "$2", "from_port": "exec",
             "to_node": "$3", "to_port": "exec"},
            {"op": "connect", "entity": "$0", "from_node": "$4", "from_port": "value",
             "to_node": "$3", "to_port": "condition"},
            {"op": "connect", "entity": "$0", "from_node": "$3", "from_port": "then",
             "to_node": "$5", "to_port": "exec"},
        ])),
    )
    .unwrap();
    let id = |i: usize| applied[i].created[0].clone();
    let porta = id(0);
    let positions = |project: &Project| -> Vec<[f32; 2]> {
        let graph = &project.scene(&scene).unwrap().entity(&porta).unwrap().graph;
        (1..=5)
            .map(|i| graph.node(&id(i)).unwrap().position)
            .collect()
    };
    let at = positions(&project);
    assert_eq!(at[0], [900., -300.], "a position given stays");
    // The new chain goes below what was there, its event first, its data left of the branch.
    let message = graph::node_extent(graph::definition("debug.message"))[1];
    assert_eq!(at[1], [900., -300. + message + BAND_GAP]);
    assert_eq!(at[2][0], at[1][0] + COLUMN_STEP);
    assert_eq!(at[3][0], at[2][0] - COLUMN_STEP);
    assert_eq!(at[4][0], at[2][0] + COLUMN_STEP);
    assert_nodes_apart(&project.scene(&scene).unwrap().entity(&porta).unwrap().graph);

    // A node added later, fed by one that stays, starts right of it, below what is there.
    let more = command::apply(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "add_node", "entity": porta, "operation": "attribute.set"},
            {"op": "connect", "entity": porta, "from_node": id(3), "from_port": "else",
             "to_node": "$0", "to_port": "exec"},
        ])),
    )
    .unwrap();
    let graph = &project.scene(&scene).unwrap().entity(&porta).unwrap().graph;
    assert_eq!(positions(&project), at, "nodes already placed stay");
    let set = graph.node(&more[0].created[0]).unwrap().position;
    assert_eq!(set[0], at[2][0] + COLUMN_STEP);
    assert!(set[1] > at[4][1], "below the message in its column");
    assert_nodes_apart(graph);

    // arrange_logic lays out every node, as one undo step.
    let mut images = TextureCache::default();
    let before = project.clone();
    let mut history = CommandHistory::new();
    history.begin("Agente: Organizar lógica", &project, &images);
    let arrange = batch(json!({"op": "arrange_logic", "entity": porta}));
    assert_eq!(arrange[0].label(), "Organizar lógica");
    command::apply(&mut project, &scene, &arrange).unwrap();
    history.commit(&project, &mut images).unwrap();
    let after = positions(&project);
    assert_eq!(after[1], [0., 0.], "the event leads column 0");
    assert_eq!(after[0][0], 0., "the loose message goes below, in column 0");
    assert!(after[0][1] > after[3][1]);
    assert_nodes_apart(&project.scene(&scene).unwrap().entity(&porta).unwrap().graph);
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);

    // Only the nodes named move; unknown nodes and objects without logic are refused.
    command::apply(
        &mut project,
        &scene,
        &batch(json!({"op": "arrange_logic", "entity": porta, "nodes": [id(1)]})),
    )
    .unwrap();
    assert_eq!(positions(&project)[1..], at[1..]);
    for (commands, expected) in [
        (
            json!({"op": "arrange_logic", "entity": porta, "nodes": ["fantasma"]}),
            "Nó fantasma não existe neste objeto",
        ),
        (
            json!({"op": "arrange_logic", "entity": porta, "nodes": []}),
            "Informe ao menos um nó",
        ),
        (
            json!([{"op": "create_entity", "kind": {"type": "group"}, "name": "Vazio"},
                   {"op": "arrange_logic", "entity": "$0"}]),
            "Vazio não tem lógica para organizar",
        ),
    ] {
        let before = project.clone();
        let error = command::apply(&mut project, &scene, &batch(commands)).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
        assert_eq!(project, before);
    }
}
