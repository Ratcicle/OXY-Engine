use oxy_core::{
    document::*,
    editing,
    graph::{Edge, Node},
    input_actions,
    runtime::{FIXED_DT, InputFrame, Runtime},
};

fn project() -> Project {
    let mut project = editing::blank_project("Entradas", SceneKind::TwoD).unwrap();
    let action = input_actions::create(&mut project, "Minha ação", "K").unwrap();
    let mut object = Entity::new("Responsável", None);
    for (i, mode) in ["pressed", "held", "released"].into_iter().enumerate() {
        let mut event = Node::new("event.input", [0., i as f32 * 100.]);
        event
            .params
            .insert("action".into(), Value::Text(action.clone()));
        event.params.insert("mode".into(), Value::Text(mode.into()));
        let mut message = Node::new("debug.message", [300., i as f32 * 100.]);
        message
            .params
            .insert("message".into(), Value::Text(mode.into()));
        object.graph.edges.push(Edge {
            from_node: event.id.clone(),
            from_port: "exec".into(),
            to_node: message.id.clone(),
            to_port: "exec".into(),
        });
        object.graph.nodes.extend([event, message]);
    }
    project.scenes[0].entities.push(object);
    project
}
#[test]
fn press_release_once_held_once_per_fixed_step_and_pause_cancels_pending() {
    let project = project();
    let action = project.input_bindings.keys().next().unwrap().clone();
    let mut rt = Runtime::new(&project, &project.start_scene).unwrap();
    let input = InputFrame {
        pressed: [action.clone()].into(),
        held: [action.clone()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT * 0.25, &input);
    assert!(rt.logs.is_empty());
    rt.advance(
        FIXED_DT * 2.75,
        &InputFrame {
            held: input.held.clone(),
            ..Default::default()
        },
    );
    assert_eq!(rt.logs.iter().filter(|l| l.ends_with("pressed")).count(), 1);
    assert_eq!(rt.logs.iter().filter(|l| l.ends_with("held")).count(), 3);
    rt.advance(
        FIXED_DT * 3.,
        &InputFrame {
            released: [action.clone()].into(),
            ..Default::default()
        },
    );
    assert_eq!(
        rt.logs.iter().filter(|l| l.ends_with("released")).count(),
        1
    );
    let before = rt.logs.clone();
    rt.advance(FIXED_DT * 0.25, &input);
    rt.set_paused(true);
    rt.advance(1., &input);
    rt.set_paused(false);
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.logs, before);
    assert_eq!(rt.retained_counts(), [0; 4]);
    assert!(project.scenes[0].entities[0].graph.nodes.iter().all(|n| {
        n.params
            .get("action")
            .is_none_or(|v| v == &Value::Text(action.clone()))
    }));
}
#[test]
fn action_identity_rename_references_and_controller_bindings() {
    let mut project = project();
    let id = project.input_bindings.keys().next().unwrap().clone();
    let graph = project.scenes[0].entities[0].graph.clone();
    input_actions::rename(&mut project, &id, "Confirmar").unwrap();
    assert_eq!(project.scenes[0].entities[0].graph, graph);
    let before = project.clone();
    assert!(input_actions::remove(&mut project, &id).is_err());
    assert_eq!(project, before);
    project.scenes[0].entities[0].graph.nodes.clear();
    project.scenes[0].entities[0].graph.edges.clear();
    let mut controller = Controller::default();
    controller.actions.left = id.clone();
    project.scenes[0].entities[0].controller = Some(controller);
    assert!(
        input_actions::remove(&mut project, &id)
            .unwrap_err()
            .contains("controlador")
    );
    project.scenes[0].entities[0].controller = None;
    input_actions::remove(&mut project, &id).unwrap();
    assert!(project.input_bindings.is_empty());
}
