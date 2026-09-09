//! Preparation only. The editor embeds the saved documents; runtime never calls this.
use oxy_core::{
    document::*,
    editing,
    graph::{Edge, Node},
    input_actions, persistence,
};
fn main() -> Result<(), String> {
    let mut project = editing::blank_project("Guia · Primeiro comportamento", SceneKind::TwoD)?;
    let action = input_actions::create(&mut project, "Mostrar mensagem", "K")?;
    let mut object = Entity::new("Objeto responsável", Some(Primitive::Rectangle));
    object.dimensions = [2., 1., 1.];
    let mut event = Node::new("event.input", [60., 80.]);
    event.params.insert("action".into(), Value::Text(action));
    let mut message = Node::new("debug.message", [350., 80.]);
    message.params.insert(
        "message".into(),
        Value::Text("Minha primeira ação funciona!".into()),
    );
    object.graph.edges.push(Edge {
        from_node: event.id.clone(),
        from_port: "exec".into(),
        to_node: message.id.clone(),
        to_port: "exec".into(),
    });
    object.graph.nodes = vec![event, message];
    project.scenes[0].entities.push(object);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/guia/01-primeiro-comportamento/project.oxy.json");
    persistence::save_project(&path, &project)?;
    println!("{}", path.display());
    Ok(())
}
