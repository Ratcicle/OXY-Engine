//! The game with a pause menu that the player's and the editor's native QA both play; only
//! they compile this module.
use oxy_core::{
    document::{Entity, Project, UiAnchor, UiElement, UiKind, Value},
    graph::{Edge, Node},
};

/// The guide's first-person recipe (the camera holds the mouse) with a pause menu: a hidden
/// group that Ao pausar shows and Ao retomar hides, holding a dimmed screen, a panel (an image
/// without an image), a title, Continuar and Sair do jogo; and a see-through frame in a
/// corner. Disparar, on the left mouse button, logs "disparo": a click that reaches the game
/// as a mouse action shows in the logs.
pub fn fixture() -> Project {
    let mut project = oxy_core::guide_recipes::load(5).unwrap();
    project
        .input_bindings
        .insert("disparar".into(), "MouseLeft".into());
    project
        .input_labels
        .insert("disparar".into(), "Disparar".into());
    fn node(entity: &mut Entity, operation: &str, params: &[(&str, Value)]) -> String {
        let mut node = Node::new(operation, [0.; 2]);
        for (key, value) in params {
            node.params.insert((*key).into(), value.clone());
        }
        let id = node.id.clone();
        entity.graph.nodes.push(node);
        id
    }
    fn link(entity: &mut Entity, from: &str, to: &str) {
        entity.graph.edges.push(Edge {
            from_node: from.into(),
            from_port: "exec".into(),
            to_node: to.into(),
            to_port: "exec".into(),
        });
    }
    let text = |value: &str| Value::Text(value.into());
    let mut shooter = Entity::new("Disparo", None);
    let input = node(&mut shooter, "event.input", &[("action", text("disparar"))]);
    let said = node(
        &mut shooter,
        "debug.message",
        &[("message", text("disparo"))],
    );
    link(&mut shooter, &input, &said);
    let mut menu = Entity::new("Menu de pausa", None);
    menu.visible = false;
    for (when, shown) in [("pause", true), ("resume", false)] {
        let event = node(&mut menu, "event.pause", &[("when", text(when))]);
        let show = node(
            &mut menu,
            "action.component",
            &[
                ("component", text("visible")),
                ("enabled", Value::Bool(shown)),
            ],
        );
        link(&mut menu, &event, &show);
    }
    // Named apart from what they show, so the Hierarquia never repeats a button's text.
    let element =
        |name: &str, shown: &str, kind, size: [f32; 2], y: f32, color: [f32; 4], layer| {
            let mut entity = Entity::new(name, None);
            entity.parent = Some(menu.id.clone());
            entity.layer = layer;
            entity.ui = Some(UiElement {
                kind,
                anchor: UiAnchor::Center,
                position: [0., y],
                size,
                text: shown.into(),
                color,
                ..Default::default()
            });
            entity
        };
    let white = [1.; 4];
    let dim = element(
        "Escurecer",
        "",
        UiKind::Image,
        [4000., 4000.],
        0.,
        [0.03, 0.04, 0.05, 0.55],
        0,
    );
    let panel = element(
        "Fundo do menu",
        "",
        UiKind::Image,
        [360., 260.],
        0.,
        [0.06, 0.08, 0.11, 0.86],
        1,
    );
    let title = element(
        "Título",
        "Pausado",
        UiKind::Text,
        [104., 56.],
        -82.,
        white,
        2,
    );
    let mut resume = element(
        "Botão de continuar",
        "Continuar",
        UiKind::Button,
        [240., 42.],
        -8.,
        white,
        2,
    );
    let click = node(&mut resume, "event.click", &[]);
    let act = node(&mut resume, "game.pause", &[("mode", text("resume"))]);
    link(&mut resume, &click, &act);
    let mut quit = element(
        "Botão de sair",
        "Sair do jogo",
        UiKind::Button,
        [240., 42.],
        50.,
        white,
        2,
    );
    let click = node(&mut quit, "event.click", &[]);
    let act = node(&mut quit, "game.quit", &[]);
    link(&mut quit, &click, &act);
    // A see-through frame in the corner, always shown: an image without an image as well.
    let mut frame = Entity::new("Moldura", None);
    // Under the menu's dimmed screen.
    frame.layer = -1;
    frame.ui = Some(UiElement {
        kind: UiKind::Image,
        anchor: UiAnchor::TopRight,
        position: [16., 16.],
        size: [220., 64.],
        color: [0.24, 0.79, 0.65, 0.35],
        ..Default::default()
    });
    let start = project.start_scene.clone();
    project
        .scene_mut(&start)
        .unwrap()
        .entities
        .extend([shooter, menu, dim, panel, title, resume, quit, frame]);
    oxy_core::document::validate_project(&project).unwrap();
    project
}
