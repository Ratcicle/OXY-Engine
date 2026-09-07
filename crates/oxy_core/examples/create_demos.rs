//! One-time project preparation. The player only reads the produced documents.
use oxy_core::{
    animation::{AnimationEvent, Clip, Interpolation, Keyframe},
    document::*,
    graph::{Edge, Graph, Node},
    persistence::{load_project, save_project},
    runtime::{FIXED_DT, InputFrame, Runtime},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::Path,
};

fn configured(op: &str, position: [f32; 2], params: &[(&str, Value)]) -> Node {
    let mut node = Node::new(op, position);
    for (key, value) in params {
        node.params.insert((*key).into(), value.clone());
    }
    node
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn object(id: &str) -> Value {
    Value::Object(Some(id.into()))
}
fn wire(graph: &mut Graph, from: &Node, output: &str, to: &Node, input: &str) {
    graph.edges.push(Edge {
        from_node: from.id.clone(),
        from_port: output.into(),
        to_node: to.id.clone(),
        to_port: input.into(),
    });
}
fn solid(
    name: &str,
    primitive: Primitive,
    position: [f32; 3],
    size: [f32; 3],
    color: [f32; 4],
) -> Entity {
    let mut entity = Entity::new(name, Some(primitive));
    entity.transform.position = position;
    entity.dimensions = size;
    entity.material.color = color;
    entity.collider = Some(Collider {
        size,
        ..Collider::default()
    });
    entity
}
fn ui(name: &str, kind: UiKind, position: [f32; 2], size: [f32; 2], label: &str) -> Entity {
    let mut entity = Entity::new(name, None);
    entity.ui = Some(UiElement {
        kind,
        position,
        size,
        text: label.into(),
        ..UiElement::default()
    });
    entity
}
fn label(scene: &mut Scene, name: &str, position: [f32; 2], label: &str) {
    scene
        .entities
        .push(ui(name, UiKind::Text, position, [700.0, 28.0], label));
}
fn bar(scene: &mut Scene, target: &str, attr: &str, position: [f32; 2], maximum: f64, label: &str) {
    let mut entity = ui(label, UiKind::Bar, position, [220.0, 28.0], label);
    let ui = entity.ui.as_mut().unwrap();
    ui.binding_object = Some(target.into());
    ui.binding_attribute = attr.into();
    ui.max_value = maximum;
    ui.color = [0.22, 0.8, 0.67, 1.0];
    scene.entities.push(entity);
}
fn camera(scene: &mut Scene, position: [f32; 3], rotation: [f32; 3], size: f32) {
    let mut entity = Entity::new("Câmera de jogo", None);
    entity.camera = Some(Camera {
        orthographic_size: size,
        ..Camera::default()
    });
    entity.transform.position = position;
    entity.transform.rotation = rotation;
    scene.entities.push(entity);
}

fn attack_graph(root: &mut Entity, part: &Entity, area: &mut Entity, sound: &str) {
    let mut clip = Clip::new("Ataque");
    clip.duration = 0.5;
    for (time, angle) in [(0.0, -0.65), (0.12, -1.1), (0.22, 0.8), (0.5, -0.65)] {
        let mut transform = part.transform.clone();
        transform.rotation[2] = angle;
        clip.insert_key(
            &part.id,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    clip.events.push(AnimationEvent {
        time: 0.2,
        name: "Impacto".into(),
    });
    let input = configured("event.input", [20.0, 20.0], &[("action", text("atacar"))]);
    let play = configured(
        "action.animation",
        [280.0, 20.0],
        &[("clip", text(&clip.id)), ("restart", Value::Bool(false))],
    );
    wire(&mut root.graph, &input, "exec", &play, "exec");
    let marker = configured(
        "event.animation",
        [20.0, 210.0],
        &[("marker", text("Impacto"))],
    );
    let enable = configured(
        "action.component",
        [280.0, 210.0],
        &[
            ("target", object(&area.id)),
            ("component", text("collider")),
            ("enabled", Value::Bool(true)),
        ],
    );
    let wait = configured(
        "control.wait",
        [560.0, 210.0],
        &[("seconds", Value::Number(0.1))],
    );
    let disable = configured(
        "action.component",
        [830.0, 210.0],
        &[
            ("target", object(&area.id)),
            ("component", text("collider")),
            ("enabled", Value::Bool(false)),
        ],
    );
    wire(&mut root.graph, &marker, "exec", &enable, "exec");
    wire(&mut root.graph, &enable, "exec", &wait, "exec");
    wire(&mut root.graph, &wait, "exec", &disable, "exec");
    root.graph
        .nodes
        .extend([input, play, marker, enable, wait, disable]);
    root.clips.push(clip);
    let entered = Node::new("event.area_enter", [20.0, 20.0]);
    let damage = configured(
        "action.damage",
        [290.0, 20.0],
        &[("attribute", text("Vida")), ("amount", Value::Number(25.0))],
    );
    let audio = configured(
        "action.sound",
        [580.0, 20.0],
        &[("asset", text(sound)), ("volume", Value::Number(0.25))],
    );
    wire(&mut area.graph, &entered, "exec", &damage, "exec");
    wire(&mut area.graph, &entered, "context", &damage, "target");
    wire(&mut area.graph, &damage, "exec", &audio, "exec");
    area.graph.nodes = vec![entered, damage, audio];
}

fn platform_scene(sound: &str, sprite: &str) -> Scene {
    let mut scene = Scene::new("A · Sala de plataforma 2D", SceneKind::TwoD);
    scene.entities.push(solid(
        "Chão",
        Primitive::Rectangle,
        [0.0, -0.5, 0.0],
        [20.0, 1.0, 1.0],
        [0.13, 0.2, 0.24, 1.0],
    ));
    for (x, y, w) in [(-6.0, 1.5, 2.0), (-3.5, 2.7, 2.0), (6.0, 2.8, 2.5)] {
        scene.entities.push(solid(
            "Plataforma editável",
            Primitive::Rectangle,
            [x, y, 0.0],
            [w, 0.25, 1.0],
            [0.24, 0.43, 0.45, 1.0],
        ));
    }
    let mut actor = solid(
        "Personagem · placeholder local",
        Primitive::Sprite,
        [-4.6, 0.55, 0.0],
        [0.65, 1.0, 1.0],
        [1.0; 4],
    );
    actor.material.texture = Some(sprite.into());
    actor.controller = Some(Controller::default());
    actor.attributes = BTreeMap::from([
        ("Vida".into(), Value::Number(100.0)),
        ("Chave".into(), Value::Bool(false)),
    ]);
    let mut hand = Entity::new("Braço articulado", Some(Primitive::Rectangle));
    hand.parent = Some(actor.id.clone());
    hand.transform.position = [0.24, 0.1, 0.05];
    hand.transform.pivot = [-0.3, 0.0, 0.0];
    hand.dimensions = [0.65, 0.15, 1.0];
    hand.layer = 2;
    hand.material.color = [1.0, 0.78, 0.32, 1.0];
    let mut area = Entity::new("Região de acerto · ativação por marcador", None);
    area.parent = Some(actor.id.clone());
    area.transform.position = [0.92, 0.10, 0.0];
    area.collider = Some(Collider {
        size: [1.15, 0.65, 1.0],
        is_trigger: true,
        enabled: false,
        ..Collider::default()
    });
    attack_graph(&mut actor, &hand, &mut area, sound);
    let mut target = solid(
        "Alvo · Vida configurável",
        Primitive::Rectangle,
        [3.0, 0.7, 0.0],
        [0.8, 1.4, 1.0],
        [0.78, 0.31, 0.35, 1.0],
    );
    target
        .attributes
        .insert("Vida".into(), Value::Number(100.0));
    let mut collectible = Entity::new("Coletável · habilita Chave", Some(Primitive::Circle));
    collectible.dimensions = [0.45, 0.45, 1.0];
    collectible.transform.position = [-1.3, 0.7, 0.0];
    collectible.material.color = [1.0, 0.8, 0.28, 1.0];
    collectible.collider = Some(Collider {
        size: [0.6, 0.7, 1.0],
        is_trigger: true,
        ..Collider::default()
    });
    let enter = Node::new("event.area_enter", [20.0, 20.0]);
    let set = configured(
        "attribute.set",
        [290.0, 20.0],
        &[("attribute", text("Chave")), ("value", Value::Bool(true))],
    );
    let message = configured(
        "debug.message",
        [570.0, 20.0],
        &[(
            "message",
            text("Chave coletada: pressione E para abrir a porta."),
        )],
    );
    let remove = Node::new("action.remove", [850.0, 20.0]);
    wire(&mut collectible.graph, &enter, "exec", &set, "exec");
    wire(&mut collectible.graph, &enter, "context", &set, "target");
    wire(&mut collectible.graph, &set, "exec", &message, "exec");
    wire(&mut collectible.graph, &message, "exec", &remove, "exec");
    collectible.graph.nodes = vec![enter, set, message, remove];
    let mut door = solid(
        "Porta · condição por nós",
        Primitive::Rectangle,
        [6.8, 1.2, 0.0],
        [0.6, 2.4, 1.0],
        [0.38, 0.3, 0.52, 1.0],
    );
    let input = configured(
        "event.input",
        [20.0, 20.0],
        &[("action", text("interagir"))],
    );
    let read = configured(
        "attribute.get",
        [20.0, 220.0],
        &[("target", object(&actor.id)), ("attribute", text("Chave"))],
    );
    let branch = Node::new("condition.branch", [290.0, 20.0]);
    let disable = configured(
        "action.component",
        [580.0, 20.0],
        &[
            ("component", text("collider")),
            ("enabled", Value::Bool(false)),
        ],
    );
    let hide = configured(
        "action.component",
        [850.0, 20.0],
        &[
            ("component", text("visible")),
            ("enabled", Value::Bool(false)),
        ],
    );
    let closed = configured(
        "debug.message",
        [570.0, 220.0],
        &[("message", text("A porta precisa da chave amarela."))],
    );
    wire(&mut door.graph, &input, "exec", &branch, "exec");
    wire(&mut door.graph, &read, "value", &branch, "condition");
    wire(&mut door.graph, &branch, "then", &disable, "exec");
    wire(&mut door.graph, &disable, "exec", &hide, "exec");
    wire(&mut door.graph, &branch, "else", &closed, "exec");
    door.graph.nodes = vec![input, read, branch, disable, hide, closed];
    bar(
        &mut scene,
        &actor.id,
        "Vida",
        [24.0, 56.0],
        100.0,
        "Vida do personagem",
    );
    bar(
        &mut scene,
        &target.id,
        "Vida",
        [270.0, 56.0],
        100.0,
        "Vida do alvo",
    );
    label(
        &mut scene,
        "Instruções",
        [24.0, 16.0],
        "SALA 2D · A/D mover · Espaço pular · J atacar à direita · E abrir porta",
    );
    label(
        &mut scene,
        "Objetivo",
        [24.0, 98.0],
        "Colete o círculo amarelo. Aproxime-se do alvo vermelho e ataque com J.",
    );
    scene
        .entities
        .extend([actor, hand, area, target, collectible, door]);
    camera(&mut scene, [0.0, 2.5, 12.0], [0.0; 3], 6.5);
    scene
}

fn three_d_scene(sound: &str, texture: &str) -> Scene {
    let mut scene = Scene::new("B · Oficina 3D e golpe articulado", SceneKind::ThreeD);
    scene.entities.push(solid(
        "Chão 3D",
        Primitive::Cube,
        [0.0, -0.3, 0.0],
        [16.0, 0.6, 14.0],
        [0.16, 0.22, 0.26, 1.0],
    ));
    let mut root = Entity::new("Boneco · modelo por peças", None);
    root.transform.position = [-1.1, 1.05, 0.0];
    root.controller = Some(Controller {
        speed: 3.0,
        ..Controller::default()
    });
    root.collider = Some(Collider {
        size: [0.8, 2.1, 0.6],
        ..Collider::default()
    });
    root.attributes.insert("Vida".into(), Value::Number(100.0));
    let mut pieces = Vec::new();
    for (name, primitive, position, dimensions, color) in [
        (
            "Tronco · textura pintável",
            Primitive::Cube,
            [0.0, 0.2, 0.0],
            [0.7, 0.8, 0.4],
            [1.0; 4],
        ),
        (
            "Cabeça",
            Primitive::Sphere,
            [0.0, 0.92, 0.0],
            [0.52, 0.52, 0.52],
            [0.91, 0.72, 0.48, 1.0],
        ),
        (
            "Braço esquerdo",
            Primitive::Cylinder,
            [-0.48, 0.22, 0.0],
            [0.2, 0.7, 0.2],
            [0.33, 0.62, 0.63, 1.0],
        ),
        (
            "Braço direito · pivô no ombro",
            Primitive::Cylinder,
            [0.48, 0.55, 0.0],
            [0.22, 0.78, 0.22],
            [0.33, 0.62, 0.63, 1.0],
        ),
        (
            "Perna esquerda",
            Primitive::Cube,
            [-0.21, -0.66, 0.0],
            [0.25, 0.78, 0.3],
            [0.25, 0.37, 0.47, 1.0],
        ),
        (
            "Perna direita",
            Primitive::Cube,
            [0.21, -0.66, 0.0],
            [0.25, 0.78, 0.3],
            [0.25, 0.37, 0.47, 1.0],
        ),
    ] {
        let mut piece = Entity::new(name, Some(primitive));
        piece.parent = Some(root.id.clone());
        piece.transform.position = position;
        piece.dimensions = dimensions;
        piece.material.color = color;
        pieces.push(piece);
    }
    pieces[0].material.texture = Some(texture.into());
    pieces[3].transform.pivot = [0.0, 0.35, 0.0];
    let mut sword = Entity::new("Espada · lâmina", Some(Primitive::Cube));
    sword.parent = Some(pieces[3].id.clone());
    sword.transform.position = [0.0, -0.5, 0.0];
    sword.dimensions = [0.14, 0.9, 0.07];
    sword.material.color = [0.82, 0.9, 0.93, 1.0];
    let mut area = Entity::new("Região de acerto 3D", None);
    area.parent = Some(root.id.clone());
    area.transform.position = [1.15, 0.0, 0.0];
    area.collider = Some(Collider {
        size: [1.5, 1.35, 1.2],
        is_trigger: true,
        enabled: false,
        ..Collider::default()
    });
    attack_graph(&mut root, &pieces[3], &mut area, sound);
    let mut target = solid(
        "Alvo 3D · Vida",
        Primitive::Cube,
        [0.65, 0.8, 0.0],
        [0.85, 1.6, 0.85],
        [0.74, 0.32, 0.36, 1.0],
    );
    target
        .attributes
        .insert("Vida".into(), Value::Number(150.0));
    bar(
        &mut scene,
        &target.id,
        "Vida",
        [24.0, 56.0],
        150.0,
        "Vida do alvo 3D",
    );
    label(
        &mut scene,
        "Instruções 3D",
        [24.0, 16.0],
        "OFICINA 3D · A/D/W/S mover · J ataque à direita · peças, UV e clip editáveis",
    );
    scene.entities.extend([root, sword, area, target]);
    scene.entities.extend(pieces);
    camera(&mut scene, [0.0, 4.5, 9.0], [-0.32, 0.0, 0.0], 7.5);
    scene
}

fn cards_scene(texture: &str) -> Scene {
    let mut scene = Scene::new("C · Carta, custo, energia e alvo", SceneKind::TwoD);
    let mut energy = Entity::new("Estado editável da interação", None);
    energy
        .attributes
        .insert("Energia".into(), Value::Number(3.0));
    let mut target = solid(
        "Alvo da carta",
        Primitive::Circle,
        [2.0, 0.0, 0.0],
        [1.6, 1.6, 1.0],
        [0.73, 0.32, 0.38, 1.0],
    );
    target
        .attributes
        .insert("Vida".into(), Value::Number(100.0));
    let mut card = ui(
        "Carta · clique executa o grafo",
        UiKind::Button,
        [32.0, 150.0],
        [240.0, 260.0],
        "FAÍSCA\nCusto: 1 energia\nDano: 25\nClique para jogar",
    );
    card.ui.as_mut().unwrap().color = [0.95, 0.96, 0.98, 1.0];
    card.ui.as_mut().unwrap().texture = Some(texture.into());
    card.attributes.insert("Custo".into(), Value::Number(1.0));
    card.attributes.insert("Dano".into(), Value::Number(25.0));
    let click = Node::new("event.click", [20.0, 20.0]);
    let get_energy = configured(
        "attribute.get",
        [20.0, 220.0],
        &[
            ("target", object(&energy.id)),
            ("attribute", text("Energia")),
        ],
    );
    let cost = configured(
        "attribute.get",
        [20.0, 410.0],
        &[("attribute", text("Custo"))],
    );
    let compare = configured(
        "condition.compare",
        [280.0, 220.0],
        &[("operator", text(">="))],
    );
    let branch = Node::new("condition.branch", [540.0, 20.0]);
    let subtract = configured("math.binary", [540.0, 260.0], &[("operator", text("-"))]);
    let set = configured(
        "attribute.set",
        [820.0, 20.0],
        &[
            ("target", object(&energy.id)),
            ("attribute", text("Energia")),
        ],
    );
    let damage_value = configured(
        "attribute.get",
        [820.0, 260.0],
        &[("attribute", text("Dano"))],
    );
    let damage = configured(
        "action.damage",
        [1110.0, 20.0],
        &[("target", object(&target.id)), ("attribute", text("Vida"))],
    );
    let insufficient = configured(
        "debug.message",
        [820.0, 460.0],
        &[(
            "message",
            text("Energia insuficiente; o custo e o dano são atributos editáveis da carta."),
        )],
    );
    wire(&mut card.graph, &click, "exec", &branch, "exec");
    wire(&mut card.graph, &get_energy, "value", &compare, "a");
    wire(&mut card.graph, &cost, "value", &compare, "b");
    wire(&mut card.graph, &compare, "result", &branch, "condition");
    wire(&mut card.graph, &branch, "then", &set, "exec");
    wire(&mut card.graph, &branch, "else", &insufficient, "exec");
    wire(&mut card.graph, &get_energy, "value", &subtract, "a");
    wire(&mut card.graph, &cost, "value", &subtract, "b");
    wire(&mut card.graph, &subtract, "value", &set, "value");
    wire(&mut card.graph, &set, "exec", &damage, "exec");
    wire(&mut card.graph, &damage_value, "value", &damage, "amount");
    card.graph.nodes = vec![
        click,
        get_energy,
        cost,
        compare,
        branch,
        subtract,
        set,
        damage_value,
        damage,
        insufficient,
    ];
    bar(
        &mut scene,
        &energy.id,
        "Energia",
        [24.0, 56.0],
        3.0,
        "Energia",
    );
    bar(
        &mut scene,
        &target.id,
        "Vida",
        [290.0, 56.0],
        100.0,
        "Vida do alvo",
    );
    label(
        &mut scene,
        "Instruções carta",
        [24.0, 16.0],
        "INTERAÇÃO DE CARTA · três usos; o quarto clique informa energia insuficiente",
    );
    scene.entities.extend([energy, target, card]);
    camera(&mut scene, [0.0, 0.0, 10.0], [0.0; 3], 5.0);
    scene
}

fn prepare_assets(project: &mut Project, folder: &Path) -> Result<(Id, Id, Id), String> {
    fs::create_dir_all(folder.join("assets")).map_err(|error| error.to_string())?;
    let mut painted = image::RgbaImage::from_pixel(256, 256, image::Rgba([41, 112, 122, 255]));
    for (x, y, pixel) in painted.enumerate_pixels_mut() {
        if (x / 32 + y / 32) % 2 == 0 {
            *pixel = image::Rgba([56, 137, 146, 255]);
        }
        if (100..156).contains(&x) && (40..216).contains(&y)
            || (40..216).contains(&x) && (100..156).contains(&y)
        {
            *pixel = image::Rgba([240, 191, 74, 255]);
        }
    }
    painted
        .save(folder.join("assets/pintura_local.png"))
        .map_err(|error| error.to_string())?;
    let mut sprite = image::RgbaImage::new(32, 48);
    for (x, y, pixel) in sprite.enumerate_pixels_mut() {
        if (5..27).contains(&x) && (3..43).contains(&y) {
            *pixel = image::Rgba([70, 188, 183, 255]);
        }
        if (8..24).contains(&x) && (8..19).contains(&y) {
            *pixel = image::Rgba([20, 52, 66, 255]);
        }
        if (10..13).contains(&x) && (11..15).contains(&y)
            || (19..22).contains(&x) && (11..15).contains(&y)
        {
            *pixel = image::Rgba([255, 221, 103, 255]);
        }
    }
    sprite
        .save(folder.join("assets/sprite_placeholder_local.png"))
        .map_err(|error| error.to_string())?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut audio = hound::WavWriter::create(folder.join("assets/impacto_local.wav"), spec)
        .map_err(|error| error.to_string())?;
    for sample in 0..3308 {
        let t = sample as f32 / 22050.0;
        let amplitude = (1.0 - t / 0.15).max(0.0);
        audio
            .write_sample(((t * 280.0 * std::f32::consts::TAU).sin() * amplitude * 11000.0) as i16)
            .map_err(|error| error.to_string())?;
    }
    audio.finalize().map_err(|error| error.to_string())?;
    let mut ids = Vec::new();
    for (name, path, kind) in [
        (
            "Pintura local · pixels editáveis",
            "assets/pintura_local.png",
            AssetKind::Texture,
        ),
        (
            "Sprite placeholder local",
            "assets/sprite_placeholder_local.png",
            AssetKind::Texture,
        ),
        (
            "Impacto sintetizado local",
            "assets/impacto_local.wav",
            AssetKind::Audio,
        ),
    ] {
        let asset = Asset {
            id: new_id(),
            name: name.into(),
            path: path.into(),
            kind,
            model: None,
        };
        ids.push(asset.id.clone());
        project.assets.push(asset);
    }
    Ok((ids[0].clone(), ids[1].clone(), ids[2].clone()))
}

fn validate_demos(project: &Project) -> Result<(), String> {
    let cards = &project.scenes[2];
    let card = cards
        .entities
        .iter()
        .find(|entity| {
            entity
                .ui
                .as_ref()
                .is_some_and(|ui| ui.kind == UiKind::Button)
        })
        .unwrap();
    let energy = cards
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Energia"))
        .unwrap();
    let target = cards
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Vida"))
        .unwrap();
    let mut runtime = Runtime::new(project, &cards.id)?;
    for _ in 0..4 {
        runtime.click(&card.id);
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        runtime.scene().entity(&energy.id).unwrap().attributes["Energia"],
        Value::Number(0.0)
    );
    assert_eq!(
        runtime.scene().entity(&target.id).unwrap().attributes["Vida"],
        Value::Number(25.0)
    );
    assert!(
        runtime
            .logs
            .iter()
            .any(|message| message.contains("Energia insuficiente"))
    );
    let platform = &project.scenes[0];
    let actor = platform
        .entities
        .iter()
        .find(|entity| entity.controller.is_some())
        .unwrap();
    let target = platform
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Vida") && entity.controller.is_none())
        .unwrap();
    let collectible =
        platform
            .entities
            .iter()
            .find(|entity| {
                entity.graph.nodes.iter().any(|node| {
                    node.operation == "attribute.set" && node.text("attribute") == "Chave"
                })
            })
            .unwrap();
    let door =
        platform
            .entities
            .iter()
            .find(|entity| {
                entity.graph.nodes.iter().any(|node| {
                    node.operation == "event.input" && node.text("action") == "interagir"
                })
            })
            .unwrap();
    let mut runtime = Runtime::new(project, &platform.id)?;
    let right = InputFrame {
        held: BTreeSet::from(["mover_direita".into()]),
        pressed: BTreeSet::new(),
    };
    for _ in 0..83 {
        runtime.advance(FIXED_DT, &right);
    }
    assert!(runtime.scene().entity(&collectible.id).is_none());
    assert_eq!(
        runtime.scene().entity(&actor.id).unwrap().attributes["Chave"],
        Value::Bool(true)
    );
    let input = InputFrame {
        pressed: BTreeSet::from(["atacar".into(), "interagir".into()]),
        held: BTreeSet::new(),
    };
    runtime.advance(FIXED_DT, &input);
    for _ in 0..60 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        runtime.scene().entity(&target.id).unwrap().attributes["Vida"],
        Value::Number(75.0)
    );
    assert!(
        !runtime
            .scene()
            .entity(&door.id)
            .unwrap()
            .collider
            .as_ref()
            .unwrap()
            .enabled
    );
    assert!(
        !runtime
            .logs
            .iter()
            .any(|message| message.contains("interrompido")),
        "{:?}",
        runtime.logs
    );
    assert_eq!(actor.attributes["Chave"], Value::Bool(false));
    let three_d = &project.scenes[1];
    let target = three_d
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Vida") && entity.controller.is_none())
        .unwrap();
    let mut runtime = Runtime::new(project, &three_d.id)?;
    runtime.advance(
        FIXED_DT,
        &InputFrame {
            pressed: BTreeSet::from(["atacar".into()]),
            held: BTreeSet::new(),
        },
    );
    for _ in 0..45 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        runtime.scene().entity(&target.id).unwrap().attributes["Vida"],
        Value::Number(125.0)
    );
    assert!(
        !runtime
            .logs
            .iter()
            .any(|message| message.contains("interrompido")),
        "{:?}",
        runtime.logs
    );
    println!(
        "Validação por dados: coleta/porta/ataque 2D, ataque articulado 3D, custo/energia/dano da carta e isolamento passaram."
    );
    Ok(())
}

fn main() -> Result<(), String> {
    let argument = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/validacao".into());
    let folder = Path::new(&argument);
    let mut project = Project::new("OXY Engine · validação v0.1");
    let (texture, sprite, audio) = prepare_assets(&mut project, folder)?;
    project.scenes = vec![
        platform_scene(&audio, &sprite),
        three_d_scene(&audio, &texture),
        cards_scene(&texture),
    ];
    project.start_scene = project.scenes[0].id.clone();
    // The reusable model contains exactly the editable hierarchy, with fresh IDs.
    let model_scene = &project.scenes[1];
    let root = model_scene
        .entities
        .iter()
        .find(|entity| entity.controller.is_some())
        .unwrap();
    let selected = model_scene.descendants(&root.id);
    let mut model: Vec<_> = model_scene
        .entities
        .iter()
        .filter(|entity| selected.contains(&entity.id))
        .cloned()
        .collect();
    let map: HashMap<_, _> = model
        .iter()
        .map(|entity| (entity.id.clone(), new_id()))
        .collect();
    remap_entities(&mut model, &map)?;
    project.assets.push(Asset {
        id: new_id(),
        name: "Boneco editável por peças".into(),
        path: String::new(),
        kind: AssetKind::Model,
        model: Some(model),
    });
    validate_project(&project)?;
    validate_demos(&project)?;
    let path = folder.join("project.oxy.json");
    save_project(&path, &project)?;
    let reopened = load_project(&path)?;
    assert_eq!(reopened, project);
    validate_demos(&reopened)?;
    println!("Projeto editável salvo e reaberto: {}", path.display());
    Ok(())
}
