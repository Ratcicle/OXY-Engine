use oxy_core::{
    command::{self, Command},
    document::*,
    editing, graph,
    history::CommandHistory,
    input_actions, query, surface,
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
    assert!(error.to_string().contains("Nada foi alterado"));
    assert_eq!(project, before);
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
        ("condition.compare", "operator", "=>"),
        ("event.character", "kind", "pulo"),
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
    command::apply(&mut project, &scene, &set("/")).unwrap();

    // The editor reads the same lists from the core.
    assert_eq!(
        graph::choices("event.input", "mode"),
        Some(&graph::INPUT_MODES[..])
    );
    assert_eq!(
        graph::choices("condition.compare", "operator"),
        Some(&graph::COMPARISONS[..])
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
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("\"abrir\" não existe"),
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
