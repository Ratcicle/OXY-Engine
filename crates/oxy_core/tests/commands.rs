use oxy_core::{
    command::{self, Command},
    document::*,
    editing,
    history::CommandHistory,
    query,
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
    let tree = query::scene_tree(&project, &scene).unwrap();
    let names: Vec<_> = tree["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"Herói".to_owned()), "{names:?}");
    let entity = query::entity(&project, &scene, &hero).unwrap();
    assert_eq!(entity["id"], hero.as_str());
    assert_eq!(query::find(&project, "herói")[0]["id"], hero.as_str());
    let catalog = query::catalog();
    let nodes = catalog["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), oxy_core::graph::registry().len());
    assert!(
        nodes
            .iter()
            .all(|n| !n["description"].as_str().unwrap().is_empty())
    );
}
