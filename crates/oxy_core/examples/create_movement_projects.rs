//! Preparation only: the player loads JSON and never calls these builders.
use glam::Vec3;
use oxy_core::{
    character::*,
    document::*,
    graph::{Edge, Graph, Node},
    movement_presets::{self, MovementPreset},
    persistence,
    physics3d::*,
    surface::*,
};
use std::collections::BTreeMap;
fn text(s: &str) -> Value {
    Value::Text(s.into())
}
fn object(s: &str) -> Value {
    Value::Object(Some(s.into()))
}
fn add(g: &mut Graph, op: &str, params: &[(&str, Value)]) -> Id {
    let i = g.nodes.len();
    let mut n = Node::new(op, [(i % 4) as f32 * 320., (i / 4) as f32 * 300.]);
    n.params
        .extend(params.iter().map(|(k, v)| ((*k).into(), v.clone())));
    let id = n.id.clone();
    g.nodes.push(n);
    id
}
fn wire(g: &mut Graph, a: &str, output: &str, b: &str, input: &str) {
    g.edges.push(Edge {
        from_node: a.into(),
        from_port: output.into(),
        to_node: b.into(),
        to_port: input.into(),
    });
}
fn then(g: &mut Graph, a: &str, b: &str) {
    wire(g, a, "exec", b, "exec");
}
fn cube(name: &str, position: [f32; 3], size: [f32; 3], color: [f32; 4]) -> Entity {
    let mut e = Entity::new(name, Some(Primitive::Cube));
    e.transform.position = position;
    e.dimensions = size;
    e.material.color = color;
    e.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size },
        ..Default::default()
    });
    e
}
fn hud(name: &str, label: &str, y: f32, binding: Option<(&str, &str)>) -> Entity {
    let mut e = Entity::new(name, None);
    e.ui = Some(UiElement {
        text: label.into(),
        position: [16., y],
        size: [700., 28.],
        binding_object: binding.map(|(id, _)| id.into()),
        binding_attribute: binding.map_or("", |(_, a)| a).into(),
        ..Default::default()
    });
    e
}
fn action(p: &mut Project, id: &str, name: &str, key: &str) {
    p.input_bindings.insert(id.into(), key.into());
    p.input_labels.insert(id.into(), name.into());
}
fn assign(g: &mut Graph, attr: &str, value: Value) -> Id {
    add(
        g,
        "attribute.set",
        &[("attribute", text(attr)), ("value", value)],
    )
}
fn read(g: &mut Graph, attr: &str) -> Id {
    add(g, "attribute.get", &[("attribute", text(attr))])
}
fn base(title: &str) -> (Project, Id) {
    let mut p = Project::new(title);
    p.scenes[0].kind = SceneKind::ThreeD;
    p.scenes[0].name = title.into();
    let scene = p.start_scene.clone();
    let body = movement_presets::create(
        &mut p,
        &scene,
        MovementPreset::FirstPerson,
        Vec3::new(0., 0.02, 4.),
    )
    .unwrap();
    p.scenes[0]
        .entity_mut(&body)
        .unwrap()
        .character3d
        .as_mut()
        .unwrap()
        .apply_profile(MovementProfile::Parkour);
    p.scenes[0].entities.push(cube(
        "Piso inicial",
        [0., -0.5, 0.],
        [16., 1., 18.],
        [0.22, 0.26, 0.3, 1.],
    ));
    action(&mut p, "reiniciar", "Voltar ao checkpoint", "R");
    let e = p.scenes[0].entity_mut(&body).unwrap();
    e.attributes = BTreeMap::from([
        ("Velocidade".into(), Value::Number(0.)),
        ("Tempo".into(), Value::Number(0.)),
        ("Checkpoint".into(), text("Início")),
        ("Destino".into(), Value::Vector3([0., 0.02, 4.])),
        ("Giro salvo".into(), Value::Number(0.)),
        ("Olhar salvo".into(), Value::Vector2([0.; 2])),
    ]);
    let g = &mut e.graph;
    let step = add(g, "event.step", &[]);
    let state = add(g, "character.state", &[]);
    let time = add(g, "time.read", &[]);
    let speed = assign(g, "Velocidade", Value::Number(0.));
    let clock = assign(g, "Tempo", Value::Number(0.));
    then(g, &step, &speed);
    wire(g, &state, "speed", &speed, "value");
    then(g, &speed, &clock);
    wire(g, &time, "time", &clock, "value");
    let reset = add(g, "event.input", &[("action", text("reiniciar"))]);
    reset_graph(g, &reset, None);
    p.scenes[0].entities.extend([
        hud(
            "Ajuda de movimento",
            "WASD mover · Espaço pular · Shift correr · C agachar/deslizar",
            12.,
            None,
        ),
        hud(
            "Ajuda da câmera",
            "Mouse olhar · V primeira/terceira pessoa · Q ombro · R checkpoint",
            40.,
            None,
        ),
        hud(
            "Velocímetro",
            "Velocidade horizontal: {valor} m/s",
            72.,
            Some((&body, "Velocidade")),
        ),
        hud(
            "Cronômetro",
            "Simulação: {valor} s",
            100.,
            Some((&body, "Tempo")),
        ),
        hud(
            "Checkpoint atual",
            "Checkpoint: {valor}",
            128.,
            Some((&body, "Checkpoint")),
        ),
    ]);
    (p, body)
}
fn reset_graph(g: &mut Graph, event: &str, context: Option<&str>) {
    let dest = read(g, "Destino");
    let yaw = read(g, "Giro salvo");
    let look = read(g, "Olhar salvo");
    let reset = add(g, "character.teleport", &[("search", Value::Number(0.5))]);
    for n in [&dest, &yaw, &look, &reset] {
        if let Some(context) = context {
            wire(g, event, context, n, "target");
        }
    }
    then(g, event, &reset);
    wire(g, &dest, "value", &reset, "position");
    wire(g, &yaw, "value", &reset, "yaw");
    wire(g, &look, "value", &reset, "look");
    let branch = add(g, "condition.branch", &[]);
    then(g, &reset, &branch);
    wire(g, &reset, "success", &branch, "condition");
    let error = add(
        g,
        "debug.message",
        &[(
            "message",
            text(
                "Destino ocupado: o reinício foi recusado; ajuste o checkpoint ou libere o espaço.",
            ),
        )],
    );
    wire(g, &branch, "else", &error, "exec");
}
fn sensor(name: &str, position: [f32; 3], size: [f32; 3]) -> Entity {
    let mut e = cube(name, position, size, [0.22, 0.65, 0.71, 0.2]);
    e.primitive = None;
    e.physics3d.as_mut().unwrap().sensor = true;
    e
}
fn checkpoint(p: &mut Project, name: &str, position: [f32; 3]) {
    let mut area = sensor(name, position, [8., 2.8, 0.12]);
    let g = &mut area.graph;
    let enter = add(g, "event.area_enter", &[]);
    let transform = add(g, "transform.read", &[]);
    let camera = add(g, "camera.read", &[]);
    wire(g, &enter, "context", &transform, "target");
    let mut previous = enter.clone();
    for (attribute, value, source, port) in [
        (
            "Destino",
            Value::Vector3([0.; 3]),
            Some(&transform),
            "position",
        ),
        ("Giro salvo", Value::Number(0.), Some(&transform), "yaw"),
        (
            "Olhar salvo",
            Value::Vector2([0.; 2]),
            Some(&camera),
            "look",
        ),
        ("Checkpoint", text(name), None, ""),
    ] {
        let n = assign(g, attribute, value);
        wire(g, &enter, "context", &n, "target");
        if let Some(source) = source {
            wire(g, source, port, &n, "value");
        }
        then(g, &previous, &n);
        previous = n;
    }
    marker_frame(p, &area);
    p.scenes[0].entities.push(area);
}
fn reset_floor(p: &mut Project) {
    let mut area = sensor(
        "Reinício ao cair · camada fina",
        [0., -5., -20.],
        [130., 0.02, 160.],
    );
    area.visible = false;
    let event = add(&mut area.graph, "event.area_enter", &[]);
    reset_graph(&mut area.graph, &event, Some("context"));
    p.scenes[0].entities.push(area);
}
fn trigger_action(
    p: &mut Project,
    name: &str,
    key: &str,
    body: &str,
    operation: &str,
    params: &[(&str, Value)],
) {
    action(p, key, name, key);
    let g = &mut p.scenes[0].entity_mut(body).unwrap().graph;
    let event = add(g, "event.input", &[("action", text(key))]);
    let n = add(g, operation, params);
    then(g, &event, &n);
}
fn surfaces(p: &mut Project) -> (Id, Id) {
    let common = SurfaceMaterial::preset(SurfacePreset::Common);
    let ice = SurfaceMaterial::preset(SurfacePreset::Ice);
    let ids = (common.id.clone(), ice.id.clone());
    p.surfaces.extend([common, ice]);
    ids
}
fn recipe(index: usize) -> Project {
    let titles = [
        "Primeira pessoa por nós",
        "Trocar câmera por nós",
        "Gelo e piso comum",
        "Pulo e impulso",
        "Checkpoint e reinício",
        "Bloquear entrada durante queda",
        "Saltos manuais e automáticos",
    ];
    let (mut p, body) = base(titles[index]);
    reset_floor(&mut p);
    match index {
        0 => {
            let e = p.scenes[0].entity_mut(&body).unwrap();
            e.character3d.as_mut().unwrap().automatic_input = false;
            let g = &mut e.graph;
            let event = add(g, "event.step", &[]);
            let axes = add(g, "input.axes", &[]);
            let intent = add(g, "character.intent", &[]);
            then(g, &event, &intent);
            wire(g, &axes, "movement", &intent, "axis");
            let jump = add(g, "event.input", &[("action", text("pular"))]);
            let action = add(g, "character.jump", &[]);
            then(g, &jump, &action);
            let mut previous = intent;
            for (binding, command) in [("correr", "sprint"), ("agachar", "crouch")] {
                let input = add(g, "input.read", &[("action", text(binding))]);
                let action = add(g, "character.posture", &[("command", text(command))]);
                then(g, &previous, &action);
                wire(g, &input, "held", &action, "enabled");
                previous = action;
            }
        }
        1 => {
            let camera = p.scenes[0]
                .entities
                .iter()
                .find(|e| e.camera_rig.is_some())
                .unwrap()
                .id
                .clone();
            for (key, mode) in [("1", "first_person"), ("2", "third_person")] {
                trigger_action(
                    &mut p,
                    "Escolher câmera",
                    key,
                    &body,
                    "camera.mode",
                    &[("target", object(&camera)), ("mode", text(mode))],
                );
            }
            p.scenes[0].entities.push(hud(
                "Instrução",
                "1: primeira pessoa · 2: terceira pessoa · nós mantêm velocidade e apoio",
                170.,
                None,
            ));
        }
        2 => {
            let (common, ice) = surfaces(&mut p);
            let floor = p.scenes[0]
                .entities
                .iter_mut()
                .find(|e| e.name == "Piso inicial")
                .unwrap();
            floor.physics3d.as_mut().unwrap().surface = Some(common.clone());
            let floor = floor.id.clone();
            for (key, surface) in [("1", common), ("2", ice)] {
                trigger_action(
                    &mut p,
                    "Trocar superfície",
                    key,
                    &body,
                    "surface.apply",
                    &[
                        ("target", object(&floor)),
                        ("surface", Value::Surface(Some(surface))),
                    ],
                );
            }
            p.scenes[0].entities.push(hud(
                "Instrução",
                "1: piso comum · 2: gelo · cor da peça não define seu atrito",
                170.,
                None,
            ));
        }
        3 => {
            trigger_action(
                &mut p,
                "Impulso",
                "I",
                &body,
                "character.velocity",
                &[("velocity", Value::Vector3([0., 7., -8.]))],
            );
            trigger_action(&mut p, "Solicitar salto", "K", &body, "character.jump", &[]);
            p.scenes[0].entities.push(hud(
                "Instrução",
                "K solicita pulo no chão · I acrescenta impulso, inclusive no ar",
                170.,
                None,
            ));
        }
        4 => {
            checkpoint(&mut p, "Marco azul", [0., 1.4, -4.]);
            p.scenes[0].entities.push(hud(
                "Instrução",
                "Atravesse o marco azul; R restaura posição, corpo e olhar salvos",
                170.,
                None,
            ));
        }
        5 => {
            p.scenes[0].entity_mut(&body).unwrap().transform.position[1] = 5.;
            trigger_action(
                &mut p,
                "Bloquear movimento",
                "B",
                &body,
                "character.block",
                &[("reason", text("teste de queda"))],
            );
            trigger_action(
                &mut p,
                "Liberar movimento",
                "N",
                &body,
                "character.block",
                &[
                    ("reason", text("teste de queda")),
                    ("blocked", Value::Bool(false)),
                ],
            );
            p.scenes[0].entities.push(hud(
                "Instrução",
                "B bloqueia intenção; gravidade continua · N libera o mesmo motivo",
                170.,
                None,
            ));
        }
        _ => {
            p.scenes[0]
                .entity_mut(&body)
                .unwrap()
                .character3d
                .as_mut()
                .unwrap()
                .apply_profile(MovementProfile::ChainedJumps);
            for (key, enabled) in [("1", false), ("2", true)] {
                trigger_action(
                    &mut p,
                    "Modo de pulo",
                    key,
                    &body,
                    "character.option",
                    &[
                        ("option", text("automatic_jump")),
                        ("enabled", Value::Bool(enabled)),
                    ],
                );
            }
            p.scenes[0].entities.push(hud(
                "Instrução",
                "1: pulo manual · 2: automático ao manter Espaço · observe o embalo",
                170.,
                None,
            ));
        }
    }
    p
}
fn moving_platform(
    p: &mut Project,
    name: &str,
    position: [f32; 3],
    delta: [f32; 3],
    duration: f32,
) {
    let scene = p.start_scene.clone();
    let id = movement_presets::create(p, &scene, MovementPreset::Platform, Vec3::from(position))
        .unwrap();
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.name = name.into();
    let clip = &mut e.clips[0];
    clip.duration = duration;
    for (i, k) in clip.tracks[0].keyframes.iter_mut().enumerate() {
        k.time = duration * i as f32 / 2.;
        k.transform.position = (Vec3::from(position)
            + if i == 1 {
                Vec3::from(delta)
            } else {
                Vec3::ZERO
            })
        .to_array();
    }
}
fn lab() -> Project {
    let (mut p, body) = base("Laboratório de movimento 3D");
    let (common, ice) = surfaces(&mut p);
    let mud = SurfaceMaterial::preset(SurfacePreset::Mud);
    let mud_id = mud.id.clone();
    p.surfaces.push(mud);
    let mut conveyor = SurfaceMaterial::preset(SurfacePreset::Conveyor);
    conveyor.conveyor = [0., 0., -2.];
    let conveyor_id = conveyor.id.clone();
    p.surfaces.push(conveyor);
    let mut ramp = cube(
        "Rampa caminhável · 20 graus",
        [0., 1.6, -14.],
        [5., 0.35, 10.],
        [0.35, 0.42, 0.51, 1.],
    );
    ramp.transform.rotation[0] = 20_f32.to_radians();
    p.scenes[0].entities.push(ramp);
    let mut steep = cube(
        "Rampa íngreme · 60 graus",
        [5.5, 2., -12.],
        [3., 0.35, 5.],
        [0.63, 0.32, 0.25, 1.],
    );
    steep.transform.rotation[0] = 60_f32.to_radians();
    p.scenes[0].entities.push(steep);
    for i in 0..16 {
        p.scenes[0].entities.push(cube(
            &format!("Degrau {} · 20 cm", i + 1),
            [-5.5, (i as f32 + 1.) * 0.1, -8.5 - i as f32 * 0.6],
            [2.5, (i as f32 + 1.) * 0.2, 0.6],
            [0.43, 0.5, 0.6, 1.],
        ));
    }
    p.scenes[0].entities.push(cube(
        "Terraço da rampa",
        [0., 2.95, -22.],
        [12., 0.6, 8.],
        [0.28, 0.36, 0.45, 1.],
    ));
    // Lower stretch; an explicit 2 m gap tests jump/border tolerance.
    for (name, z, length) in [
        ("Trecho de salto", -29., 4.),
        ("Recepção após vão", -36., 6.),
        ("Pista de superfície", -47., 16.),
        ("Final", -60., 10.),
    ] {
        p.scenes[0].entities.push(cube(
            name,
            [0., -0.4, z],
            [12., 0.8, length],
            [0.2, 0.27, 0.31, 1.],
        ));
    }
    for (name, x, material, color) in [
        ("Piso comum", -4., common, [0.34, 0.38, 0.41, 1.]),
        ("Gelo", 0., ice, [0.38, 0.72, 0.87, 1.]),
        ("Lama", 4., mud_id, [0.42, 0.27, 0.17, 1.]),
    ] {
        let mut floor = cube(name, [x, 0.05, -46.], [3.6, 0.1, 13.], color);
        floor.physics3d.as_mut().unwrap().surface = Some(material);
        p.scenes[0].entities.push(floor);
    }
    let mut belt = cube(
        "Esteira local",
        [4., 0.05, -58.],
        [3.5, 0.1, 5.],
        [0.6, 0.45, 0.23, 1.],
    );
    belt.physics3d.as_mut().unwrap().surface = Some(conveyor_id);
    p.scenes[0].entities.push(belt);
    // Roof over the ice lane: standing capsule cannot pass; crouch/slide can.
    p.scenes[0].entities.push(cube(
        "Túnel baixo · altura livre 1,15 m",
        [0., 1.6, -46.],
        [3.5, 0.7, 5.],
        [0.2, 0.32, 0.42, 1.],
    ));
    for x in [-1.8, 1.8] {
        p.scenes[0].entities.push(cube(
            "Parede do túnel",
            [x, 0.85, -46.],
            [0.2, 1.7, 5.],
            [0.27, 0.4, 0.48, 1.],
        ));
    }
    // Optional moving supports reachable from the terrace.
    moving_platform(
        &mut p,
        "Plataforma horizontal",
        [-8., 3., -22.],
        [-5., 0., 0.],
        6.,
    );
    p.scenes[0].entities.push(cube(
        "Pouso lateral",
        [-16., 3., -22.],
        [4., 0.4, 6.],
        [0.24, 0.41, 0.46, 1.],
    ));
    moving_platform(&mut p, "Elevador", [7.5, 0., -58.], [0., 3., 0.], 6.);
    // Side corridor shows camera obstruction and shoulder protection.
    p.scenes[0].entities.push(cube(
        "Piso do corredor",
        [-9., -0.3, -56.],
        [4., 0.6, 18.],
        [0.25, 0.28, 0.33, 1.],
    ));
    for x in [-10.5, -7.5] {
        p.scenes[0].entities.push(cube(
            "Corredor estreito da câmera",
            [x, 1.75, -56.],
            [0.3, 3.5, 18.],
            [0.37, 0.31, 0.46, 1.],
        ));
    }
    p.scenes[0].entities.push(cube(
        "Ligação ao corredor",
        [-6., -0.3, -60.],
        [4., 0.6, 4.],
        [0.25, 0.28, 0.33, 1.],
    ));
    checkpoint(&mut p, "Terraço", [0., 4.8, -22.]);
    checkpoint(&mut p, "Após o salto", [0., 1.4, -35.]);
    checkpoint(&mut p, "Final", [0., 1.4, -60.]);
    reset_floor(&mut p);
    for (key, profile) in [("1", "direct"), ("2", "parkour"), ("3", "chained")] {
        trigger_action(
            &mut p,
            "Escolher perfil",
            key,
            &body,
            "character.profile",
            &[
                ("profile", text(profile)),
                ("allow_limit", Value::Bool(true)),
            ],
        );
    }
    trigger_action(
        &mut p,
        "Impulso de teste",
        "I",
        &body,
        "character.velocity",
        &[("velocity", Value::Vector3([0., 6., -8.]))],
    );
    trigger_action(
        &mut p,
        "Pulo automático",
        "T",
        &body,
        "character.option",
        &[("option", text("automatic_jump"))],
    );
    trigger_action(
        &mut p,
        "Pulo manual",
        "Y",
        &body,
        "character.option",
        &[
            ("option", text("automatic_jump")),
            ("enabled", Value::Bool(false)),
        ],
    );
    p.scenes[0].entities.push(hud(
        "Ajuda de perfis",
        "1 Direto · 2 Parkour · 3 Encadeados · T/Y pulo automático/manual · I impulso",
        168.,
        None,
    ));
    p.scenes[0].entities.push(hud(
        "Percurso",
        "Siga -Z: rampas / vão / gelo e túnel / final. R volta ao último marco azul.",
        198.,
        None,
    ));
    // Finish is an ordinary trigger/attribute update. R can restart the same track.
    let mut finish = sensor("Conclusão da pista", [0., 1.4, -63.], [8., 2.8, 0.12]);
    let enter = add(&mut finish.graph, "event.area_enter", &[]);
    let n = assign(&mut finish.graph, "Checkpoint", text("Pista concluída"));
    then(&mut finish.graph, &enter, &n);
    wire(&mut finish.graph, &enter, "context", &n, "target");
    marker_frame(&mut p, &finish);
    p.scenes[0].entities.push(finish);
    p
}
fn stable(mut p: Project, seed: usize) -> Project {
    for scene in &mut p.scenes {
        for entity in &mut scene.entities {
            layout(&mut entity.graph);
        }
    }
    // Random authoring IDs are replaced once during preparation, in document order.
    // UUID text width and stable references remain representative of real projects.
    let mut ids = vec![p.id.clone(), p.start_scene.clone()];
    for s in &p.scenes {
        ids.push(s.id.clone());
        for e in &s.entities {
            ids.push(e.id.clone());
            for c in &e.clips {
                ids.push(c.id.clone());
            }
            for n in &e.graph.nodes {
                ids.push(n.id.clone());
            }
        }
    }
    ids.extend(p.surfaces.iter().map(|s| s.id.clone()));
    let mut map = BTreeMap::new();
    for id in ids {
        let i = map.len();
        map.entry(id)
            .or_insert_with(|| format!("03000000-0000-4000-8000-{:012x}", seed * 10000 + i));
    }
    fn visit(v: &mut serde_json::Value, map: &BTreeMap<String, String>) {
        match v {
            serde_json::Value::String(s) => {
                if let Some(replacement) = map.get(s) {
                    s.clone_from(replacement);
                }
            }
            serde_json::Value::Array(a) => {
                for v in a {
                    visit(v, map);
                }
            }
            serde_json::Value::Object(o) => {
                for v in o.values_mut() {
                    visit(v, map);
                }
            }
            _ => {}
        }
    }
    let mut json = serde_json::to_value(p).unwrap();
    visit(&mut json, &map);
    serde_json::from_value(json).unwrap()
}
fn marker_frame(p: &mut Project, area: &Entity) {
    for (name, position, size) in [
        ("Marco esquerdo", [-4., 0., 0.], [0.12, 2.8, 0.12]),
        ("Marco direito", [4., 0., 0.], [0.12, 2.8, 0.12]),
        ("Travessa do marco", [0., 1.4, 0.], [8., 0.12, 0.12]),
    ] {
        let mut e = cube(name, position, size, [0.18, 0.65, 0.7, 1.]);
        e.physics3d = None;
        e.parent = Some(area.id.clone());
        p.scenes[0].entities.push(e);
    }
}
fn layout(g: &mut Graph) {
    // Allocate real row height, including all typed state/event ports.
    let mut y = 30.;
    for row in g.nodes.chunks_mut(4) {
        let height = row
            .iter()
            .map(|n| {
                oxy_core::graph::registry()
                    .iter()
                    .find(|op| op.id == n.operation)
                    .map_or(180., |op| {
                        66. + op.inputs.len().max(op.outputs.len()) as f32 * 24.
                    })
            })
            .fold(0., f32::max);
        for (i, n) in row.iter_mut().enumerate() {
            n.position = [30. + i as f32 * 300., y];
        }
        y += height + 100.;
    }
}
fn main() -> Result<(), String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for i in 0..7 {
        let p = stable(recipe(i), i + 6);
        let path = root
            .join("guia")
            .join(format!("{:02}-movimento-3d", i + 6))
            .join(persistence::PROJECT_FILE);
        persistence::save_project(&path, &p)?;
        println!("{}", path.display());
    }
    let p = stable(lab(), 30);
    let path = root.join("laboratorio-3d").join(persistence::PROJECT_FILE);
    persistence::save_project(&path, &p)?;
    println!("{}", path.display());
    Ok(())
}
