use oxy_core::{
    document::*, edit_history::CommandHistory, editing, graph::Node, texture_cache::TextureCache,
};

#[test]
fn scenes_duplicate_remap_and_history_preserve_references() {
    let mut project = editing::blank_project("Oficina", SceneKind::ThreeD).unwrap();
    assert!(project.input_bindings.is_empty());
    assert_eq!(project.scenes[0].kind, SceneKind::ThreeD);
    let original = project.start_scene.clone();
    let mut parent = Entity::new("Grupo", None);
    let mut child = Entity::new("Peça", Some(Primitive::Cube));
    child.parent = Some(parent.id.clone());
    parent
        .attributes
        .insert("Alvo".into(), Value::Object(Some(child.id.clone())));
    let mut node = Node::new("action.scene", [0., 0.]);
    node.params
        .insert("scene".into(), Value::Text(original.clone()));
    parent.graph.nodes.push(node);
    project.scenes[0].entities = vec![parent, child];
    let before = project.clone();
    let mut history = CommandHistory::new();
    let mut textures = TextureCache::default();
    history.begin("Duplicar cena", &project, &textures);
    let id = editing::duplicate_scene(&mut project, &original).unwrap();
    let scene = project.scene(&id).unwrap();
    assert_ne!(scene.entities[0].id, before.scenes[0].entities[0].id);
    assert_eq!(
        scene.entities[1].parent.as_ref(),
        Some(&scene.entities[0].id)
    );
    assert_eq!(
        scene.entities[0].attributes["Alvo"],
        Value::Object(Some(scene.entities[1].id.clone()))
    );
    assert_eq!(
        scene.entities[0].graph.nodes[0].params["scene"],
        Value::Text(id.clone())
    );
    validate_project(&project).unwrap();
    history.commit(&project, &mut textures).unwrap();
    let after = project.clone();
    history.undo(&mut project, &mut textures).unwrap();
    assert_eq!(project, before);
    history.redo(&mut project, &mut textures).unwrap();
    assert_eq!(project, after);
    assert!(
        editing::delete_scene(&mut project, &id)
            .unwrap_err()
            .contains("Mudar de cena")
    );
    assert_eq!(project, after);
    project.scene_mut(&id).unwrap().entities[0]
        .graph
        .nodes
        .clear();
    history.begin("Excluir cena", &project, &textures);
    let before_delete = project.clone();
    editing::delete_scene(&mut project, &id).unwrap();
    history.commit(&project, &mut textures).unwrap();
    history.undo(&mut project, &mut textures).unwrap();
    assert_eq!(project, before_delete);
    assert!(editing::delete_scene(&mut project, &original).is_err());
}

#[test]
fn blank_and_last_scene_are_safe() {
    assert!(editing::blank_project("  ", SceneKind::TwoD).is_err());
    let mut project = editing::blank_project("Novo", SceneKind::TwoD).unwrap();
    let before = project.clone();
    assert!(editing::delete_scene(&mut project, &before.start_scene).is_err());
    assert_eq!(project, before);
}
