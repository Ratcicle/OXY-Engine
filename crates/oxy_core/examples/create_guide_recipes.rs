//! Deterministic preparation only. Recipes are normal saved projects, embedded by the offline guide.
use oxy_core::{
    animation::*,
    document::*,
    graph::{Edge, Graph, Node},
    input_actions, persistence,
};
fn id(n: u32) -> Id {
    format!("02000000-0000-4000-8000-{n:012x}")
}
fn text(s: &str) -> Value {
    Value::Text(s.into())
}
fn object(n: u32) -> Value {
    Value::Object(Some(id(n)))
}
fn entity(n: u32, name: &str, shape: Option<Primitive>, p: [f32; 3], size: [f32; 3]) -> Entity {
    let mut e = Entity::new(name, shape);
    e.id = id(n);
    e.transform.position = p;
    e.dimensions = size;
    e
}
fn node(n: u32, op: &str, p: [f32; 2], params: &[(&str, Value)]) -> Node {
    let mut node = Node::new(op, p);
    node.id = id(n);
    for (k, v) in params {
        node.params.insert((*k).into(), v.clone());
    }
    node
}
fn graph(nodes: Vec<Node>, edges: &[(u32, &str, u32, &str)]) -> Graph {
    Graph {
        nodes,
        edges: edges
            .iter()
            .map(|(a, ap, b, bp)| Edge {
                from_node: id(*a),
                from_port: (*ap).into(),
                to_node: id(*b),
                to_port: (*bp).into(),
            })
            .collect(),
    }
}
fn project(n: u32, name: &str) -> Project {
    let mut p = Project::new(name);
    p.id = id(n);
    p.scenes[0].id = id(n + 1);
    p.start_scene = id(n + 1);
    p.input_bindings.clear();
    p
}
fn hud(n: u32, text: &str, y: f32, binding: Option<(u32, &str)>) -> Entity {
    let mut e = entity(n, "Informação", None, [0.; 3], [1.; 3]);
    e.ui = Some(UiElement {
        text: text.into(),
        position: [24., y],
        size: [560., 36.],
        binding_object: binding.map(|(n, _)| id(n)),
        binding_attribute: binding.map_or("", |(_, a)| a).into(),
        ..Default::default()
    });
    e
}
fn body(n: u32) -> Entity {
    let mut e = entity(
        n,
        "Personagem",
        Some(Primitive::Rectangle),
        [-3., 0., 0.],
        [0.6, 1., 1.],
    );
    e.collider = Some(Collider {
        size: e.dimensions,
        ..Default::default()
    });
    e.controller = Some(Controller::default());
    e.material.color = [0.3, 0.75, 0.68, 1.];
    e
}
fn floor(n: u32) -> Entity {
    let mut e = entity(
        n,
        "Chão",
        Some(Primitive::Rectangle),
        [0., -1., 0.],
        [14., 1., 1.],
    );
    e.collider = Some(Collider {
        size: e.dimensions,
        ..Default::default()
    });
    e.material.color = [0.23, 0.25, 0.3, 1.];
    e
}
fn movement(p: &mut Project, e: &Entity) {
    input_actions::ensure_controller(p, &e.controller.as_ref().unwrap().actions, SceneKind::TwoD);
}
fn door() -> Project {
    let mut p = project(200, "Guia · Porta com chave");
    let mut player = body(210);
    player.attributes.insert("Chave".into(), Value::Bool(true));
    movement(&mut p, &player);
    let mut door = entity(
        211,
        "Porta",
        Some(Primitive::Rectangle),
        [2., 0.5, 0.],
        [0.5, 2., 1.],
    );
    door.collider = Some(Collider {
        size: door.dimensions,
        ..Default::default()
    });
    door.material.color = [0.72, 0.43, 0.22, 1.];
    let mut area = entity(212, "Área da porta", None, [-0.7, -0.1, 0.], [1.; 3]);
    area.parent = Some(door.id.clone());
    area.collider = Some(Collider {
        is_trigger: true,
        size: [1.5, 1.2, 1.],
        ..Default::default()
    });
    area.graph = graph(
        vec![
            node(220, "event.area_enter", [0., 40.], &[]),
            node(
                221,
                "attribute.get",
                [0., 220.],
                &[("attribute", text("Chave"))],
            ),
            node(
                222,
                "condition.compare",
                [300., 220.],
                &[("operator", text("==")), ("b", Value::Bool(true))],
            ),
            node(223, "condition.branch", [600., 40.], &[]),
            node(
                224,
                "action.component",
                [900., 40.],
                &[
                    ("target", object(211)),
                    ("component", text("collider")),
                    ("enabled", Value::Bool(false)),
                ],
            ),
            node(
                225,
                "action.component",
                [1200., 40.],
                &[
                    ("target", object(211)),
                    ("component", text("visible")),
                    ("enabled", Value::Bool(false)),
                ],
            ),
            node(
                226,
                "debug.message",
                [900., 230.],
                &[(
                    "message",
                    text("A porta exige uma chave no objeto que entrou."),
                )],
            ),
        ],
        &[
            (220, "context", 221, "target"),
            (221, "value", 222, "a"),
            (222, "result", 223, "condition"),
            (220, "exec", 223, "exec"),
            (223, "then", 224, "exec"),
            (224, "exec", 225, "exec"),
            (223, "else", 226, "exec"),
        ],
    );
    p.scenes[0].entities = vec![
        player,
        door,
        area,
        floor(213),
        hud(
            214,
            "A/D: mover · Espaço: pular · Chave: {valor}",
            24.,
            Some((210, "Chave")),
        ),
    ];
    p
}
fn card() -> Project {
    let mut p = project(300, "Guia · Carta e energia");
    let mut state = entity(310, "Estado da partida", None, [0.; 3], [1.; 3]);
    state.attributes.insert("Energia".into(), Value::Number(3.));
    let mut target = entity(
        311,
        "Alvo",
        Some(Primitive::Rectangle),
        [2., 0., 0.],
        [1.5, 2., 1.],
    );
    target.attributes.insert("Vida".into(), Value::Number(50.));
    target.material.color = [0.72, 0.3, 0.27, 1.];
    let mut card = hud(
        312,
        "Raio · custo 2 · Clique para causar 20 de dano",
        140.,
        None,
    );
    card.name = "Carta Raio".into();
    card.ui.as_mut().unwrap().kind = UiKind::Button;
    card.ui.as_mut().unwrap().size = [330., 90.];
    card.attributes.insert("Custo".into(), Value::Number(2.));
    card.graph = graph(
        vec![
            node(320, "event.click", [0., 40.], &[]),
            node(
                321,
                "attribute.get",
                [0., 220.],
                &[("target", object(310)), ("attribute", text("Energia"))],
            ),
            node(
                322,
                "attribute.get",
                [0., 380.],
                &[("attribute", text("Custo"))],
            ),
            node(
                323,
                "condition.compare",
                [300., 220.],
                &[("operator", text(">="))],
            ),
            node(324, "condition.branch", [600., 40.], &[]),
            node(325, "math.binary", [600., 300.], &[("operator", text("-"))]),
            node(
                326,
                "attribute.set",
                [900., 40.],
                &[("target", object(310)), ("attribute", text("Energia"))],
            ),
            node(
                327,
                "action.damage",
                [1200., 40.],
                &[("target", object(311)), ("amount", Value::Number(20.))],
            ),
            node(
                328,
                "debug.message",
                [900., 220.],
                &[(
                    "message",
                    text("Energia insuficiente: a carta não alterou energia nem vida."),
                )],
            ),
        ],
        &[
            (320, "exec", 324, "exec"),
            (321, "value", 323, "a"),
            (322, "value", 323, "b"),
            (323, "result", 324, "condition"),
            (321, "value", 325, "a"),
            (322, "value", 325, "b"),
            (325, "value", 326, "value"),
            (324, "then", 326, "exec"),
            (326, "exec", 327, "exec"),
            (324, "else", 328, "exec"),
        ],
    );
    p.scenes[0].entities = vec![
        state,
        target,
        card,
        hud(313, "Energia: {valor}", 24., Some((310, "Energia"))),
        hud(314, "Vida do alvo: {valor}", 65., Some((311, "Vida"))),
    ];
    p
}
fn attack() -> Project {
    let mut p = project(400, "Guia · Ataque por marcador");
    p.input_bindings.insert(id(402), "J".into());
    p.input_labels.insert(id(402), "Atacar".into());
    let mut root = entity(410, "Modelo animável", None, [-0.7, 0., 0.], [1.; 3]);
    let mut torso = entity(
        411,
        "Tronco",
        Some(Primitive::Rectangle),
        [0., 0., 0.],
        [0.6, 1., 1.],
    );
    torso.parent = Some(root.id.clone());
    let mut arm = entity(
        412,
        "Braço",
        Some(Primitive::Rectangle),
        [0.3, 0.4, 0.01],
        [0.9, 0.2, 1.],
    );
    arm.parent = Some(root.id.clone());
    arm.transform.pivot = [-0.45, 0., 0.];
    arm.material.color = [0.85, 0.7, 0.3, 1.];
    let mut clip = Clip::new("Ataque");
    clip.id = id(403);
    clip.duration = 0.6;
    for (time, angle) in [(0., -0.8), (0.15, 0.), (0.3, 0.4), (0.6, -0.8)] {
        let mut transform = arm.transform.clone();
        transform.rotation[2] = angle;
        clip.insert_key(
            &arm.id,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    clip.events.push(AnimationEvent {
        time: 0.15,
        name: "Impacto".into(),
    });
    root.clips.push(clip);
    root.graph = graph(
        vec![
            node(420, "event.input", [0., 40.], &[("action", text(&id(402)))]),
            node(
                421,
                "action.animation",
                [300., 40.],
                &[("clip", text(&id(403)))],
            ),
            node(
                422,
                "event.animation",
                [0., 260.],
                &[("marker", text("Impacto"))],
            ),
            node(
                423,
                "action.component",
                [300., 260.],
                &[
                    ("target", object(413)),
                    ("component", text("collider")),
                    ("enabled", Value::Bool(true)),
                ],
            ),
            node(
                424,
                "control.wait",
                [600., 260.],
                &[("seconds", Value::Number(0.15))],
            ),
            node(
                425,
                "action.component",
                [900., 260.],
                &[
                    ("target", object(413)),
                    ("component", text("collider")),
                    ("enabled", Value::Bool(false)),
                ],
            ),
        ],
        &[
            (420, "exec", 421, "exec"),
            (422, "exec", 423, "exec"),
            (423, "exec", 424, "exec"),
            (424, "exec", 425, "exec"),
        ],
    );
    let mut area = entity(413, "Área de acerto", None, [1.4, 0.4, 0.], [1.; 3]);
    area.parent = Some(root.id.clone());
    area.collider = Some(Collider {
        enabled: false,
        is_trigger: true,
        size: [1.3, 0.8, 1.],
        ..Default::default()
    });
    area.graph = graph(
        vec![
            node(430, "event.area_enter", [0., 40.], &[]),
            node(
                431,
                "action.damage",
                [320., 40.],
                &[("amount", Value::Number(15.)), ("once", Value::Bool(true))],
            ),
        ],
        &[(430, "exec", 431, "exec"), (430, "context", 431, "target")],
    );
    let mut target = entity(
        414,
        "Alvo",
        Some(Primitive::Rectangle),
        [0.9, 0., 0.],
        [0.7, 1., 1.],
    );
    target.collider = Some(Collider {
        size: target.dimensions,
        ..Default::default()
    });
    target.attributes.insert("Vida".into(), Value::Number(100.));
    target.material.color = [0.7, 0.25, 0.2, 1.];
    p.scenes[0].entities = vec![
        root,
        torso,
        arm,
        area,
        target,
        floor(415),
        hud(
            416,
            "J: atacar · cada ativação causa 15 · Vida do alvo: {valor}",
            24.,
            Some((414, "Vida")),
        ),
    ];
    p
}
fn passage() -> Project {
    let mut p = project(500, "Guia · Passagem entre cenas");
    let player = body(510);
    movement(&mut p, &player);
    let mut area = entity(
        511,
        "Passagem",
        Some(Primitive::Rectangle),
        [3., 0.5, 0.],
        [0.8, 1.6, 1.],
    );
    area.collider = Some(Collider {
        is_trigger: true,
        size: [0.8, 1.4, 1.],
        offset: [0., 0.1, 0.],
        ..Default::default()
    });
    area.material.color = [0.35, 0.7, 0.9, 1.];
    area.graph = graph(
        vec![
            node(520, "event.area_enter", [0., 40.], &[]),
            node(
                521,
                "action.scene",
                [300., 40.],
                &[("scene", text(&id(502)))],
            ),
        ],
        &[(520, "exec", 521, "exec")],
    );
    p.scenes[0].name = "Entrada".into();
    p.scenes[0].entities = vec![
        player,
        area,
        floor(512),
        hud(
            513,
            "A/D: vá até a passagem azul para abrir a segunda cena",
            24.,
            None,
        ),
    ];
    let mut destination = Scene::new("Destino", SceneKind::TwoD);
    destination.id = id(502);
    destination.entities.push(hud(
        514,
        "Você chegou à segunda cena. Parar restaura o documento de edição.",
        24.,
        None,
    ));
    p.scenes.push(destination);
    p
}
fn main() -> Result<(), String> {
    for (name, project) in [
        ("02-porta-com-chave", door()),
        ("03-carta-e-energia", card()),
        ("04-ataque-por-marcador", attack()),
        ("05-passagem-entre-cenas", passage()),
    ] {
        validate_project(&project)?;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/guia")
            .join(name)
            .join(persistence::PROJECT_FILE);
        persistence::save_project(&path, &project)?;
        println!("{}", path.display());
    }
    Ok(())
}
