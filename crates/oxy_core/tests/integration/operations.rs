//! The shared operations layer (runtime/operations.rs): the `Runtime` methods that the nodes
//! call and the script API will call. Called directly they act as the nodes do, with the same
//! draws, the same copies and the same messages; the nodes keep refusing in the order they
//! always did.
use glam::Vec3;
use oxy_core::{
    character::CharacterConfig,
    document::*,
    graph::*,
    random::MAX_SEED,
    runtime::{ObjectComponent, Runtime, TransformPart, TransformSpace},
    save::SaveRequest,
    variables::GameVariable,
};

fn node(op: &str, params: &[(&str, Value)]) -> Node {
    let mut n = Node::new(op, [0., 0.]);
    n.params
        .extend(params.iter().map(|(k, v)| ((*k).into(), v.clone())));
    n
}
fn wire(g: &mut Graph, a: &Node, out: &str, b: &Node, input: &str) {
    g.edges.push(Edge {
        from_node: a.id.clone(),
        from_port: out.into(),
        to_node: b.id.clone(),
        to_port: input.into(),
    });
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn n(value: f64) -> Value {
    Value::Number(value)
}
fn object(id: &str) -> Value {
    Value::Object(Some(id.into()))
}

/// A Gerador that draws a number into Sorteio and copies the Moeda as the scene starts, the
/// copy into Cópia; the Gerador's, the draw's, the copy node's and the Moeda's IDs.
fn generator() -> (Project, [Id; 4]) {
    let mut project = Project::new("Gerador");
    let mut coin = Entity::new("Moeda", Some(Primitive::Rectangle));
    coin.transform.position = [1., 2., 0.];
    let mut owner = Entity::new("Gerador", None);
    owner.attributes.insert("Sorteio".into(), n(0.));
    owner.attributes.insert("Cópia".into(), Value::Object(None));
    let start = node("event.scene_start", &[]);
    let draw = node(
        "random.number",
        &[("min", n(1.)), ("max", n(1000.)), ("mode", text("integer"))],
    );
    let keep = node("attribute.set", &[("attribute", text("Sorteio"))]);
    let copy = node(
        "action.spawn",
        &[("target", object(&coin.id)), ("x", n(3.)), ("y", n(-1.))],
    );
    let name = node("attribute.set", &[("attribute", text("Cópia"))]);
    let g = &mut owner.graph;
    wire(g, &start, "exec", &draw, "exec");
    wire(g, &draw, "exec", &keep, "exec");
    wire(g, &draw, "value", &keep, "value");
    wire(g, &keep, "exec", &copy, "exec");
    wire(g, &copy, "exec", &name, "exec");
    wire(g, &copy, "created", &name, "value");
    let ids = [
        owner.id.clone(),
        draw.id.clone(),
        copy.id.clone(),
        coin.id.clone(),
    ];
    g.nodes = vec![start, draw, keep, copy, name];
    project.scenes[0].entities = vec![coin, owner];
    validate_project(&project).unwrap();
    (project, ids)
}

#[test]
fn direct_calls_draw_and_copy_as_the_nodes_do() {
    let (project, [owner, draw, copy, coin]) = generator();
    let by_nodes = Runtime::new(&project, &project.start_scene).unwrap();
    assert!(by_nodes.logs.is_empty(), "{:?}", by_nodes.logs);
    let attributes = &by_nodes.scene().entity(&owner).unwrap().attributes;
    let Value::Object(Some(made)) = &attributes["Cópia"] else {
        panic!("{attributes:?}");
    };
    // The same project without logic, its operations called in the same order.
    let mut quiet = project.clone();
    quiet.scenes[0].entities[1].graph = Graph::default();
    let mut direct = Runtime::new(&quiet, &quiet.start_scene).unwrap();
    let drawn = direct
        .random_number((&owner, &draw), 1., 1000., true)
        .unwrap();
    assert_eq!(attributes["Sorteio"], n(drawn));
    let copied = direct
        .spawn_copy(&coin, Vec3::new(3., -1., 0.), (&owner, &copy))
        .unwrap();
    assert_eq!(&copied, made);
    assert_eq!(
        direct.scene().entity(&copied).unwrap().transform.position,
        by_nodes.scene().entity(made).unwrap().transform.position
    );
    assert_eq!(
        direct.scene().entity(&copied).unwrap().transform.position,
        [4., 1., 0.]
    );
    // Each caller has its own draws: the next one of the node's stream differs from the first,
    // and another caller starts its own.
    let next = direct
        .random_number((&owner, &draw), 1., 1000., true)
        .unwrap();
    let other = direct
        .random_number((&owner, "outro"), 1., 1000., true)
        .unwrap();
    let mut again = Runtime::new(&quiet, &quiet.start_scene).unwrap();
    assert_eq!(
        again.random_number((&owner, "outro"), 1., 1000., true),
        Ok(other)
    );
    assert_eq!(
        again.random_number((&owner, &draw), 1., 1000., true),
        Ok(drawn)
    );
    assert_eq!(
        again.random_number((&owner, &draw), 1., 1000., true),
        Ok(next)
    );
    again.set_random_seed(7.).unwrap();
    assert_ne!(
        (
            again.random_number((&owner, &draw), 1., 1000., true),
            again.random_number((&owner, &draw), 1., 1000., true)
        ),
        (Ok(drawn), Ok(next))
    );
}

/// A 2D project with a Painel that has a number Vida, a text Nome and an object Alvo, a Caixa
/// without collider, the game variable Pontos, saved between runs, and the WAV Bipe.
fn panel() -> (Project, Id, Id) {
    let mut project = Project::new("Painel");
    project.assets.push(Asset {
        id: new_id(),
        name: "Bipe".into(),
        path: "assets/Bipe.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let mut owner = Entity::new("Painel", Some(Primitive::Rectangle));
    owner.attributes.insert("Vida".into(), n(30.));
    owner.attributes.insert("Nome".into(), text("Ana"));
    owner.attributes.insert("Alvo".into(), Value::Object(None));
    let boxed = Entity::new("Caixa", Some(Primitive::Rectangle));
    let ids = (owner.id.clone(), boxed.id.clone());
    project.scenes[0].entities = vec![owner, boxed];
    project.variables.push(GameVariable {
        id: new_id(),
        name: "Pontos".into(),
        value: n(0.),
        persistent: true,
    });
    validate_project(&project).unwrap();
    (project, ids.0, ids.1)
}

#[test]
fn direct_calls_refuse_with_the_messages_of_the_nodes() {
    let (project, owner, boxed) = panel();
    let variable = project.variables[0].id.clone();
    let mut rt = Runtime::new(&project, &project.start_scene).unwrap();
    let refused = |result: Result<(), String>, message: &str| {
        assert_eq!(result, Err(message.to_owned()));
    };
    refused(
        rt.set_attribute(&owner, "Vida", text("muita")),
        "Valor incompatível com o tipo do atributo",
    );
    refused(
        rt.set_attribute(&owner, "Força", n(1.)),
        "Crie primeiro o atributo no painel de propriedades",
    );
    refused(
        rt.set_attribute(&owner, "Vida", n(f64::INFINITY)),
        "Novo valor de atributo não finito.",
    );
    refused(
        rt.set_attribute(&owner, "Alvo", object("nenhum")),
        "Nova referência de atributo não existe",
    );
    refused(
        rt.set_attribute("nenhum", "Vida", n(1.)),
        "Objeto de escrita não existe",
    );
    assert_eq!(rt.attribute(&owner, "Força"), Ok(None));
    assert_eq!(
        rt.attribute("nenhum", "Vida"),
        Err("Objeto de leitura ausente".into())
    );
    rt.set_attribute(&owner, "Alvo", object(&boxed)).unwrap();
    assert_eq!(rt.attribute(&owner, "Alvo"), Ok(Some(&object(&boxed))));

    refused(
        rt.damage(&owner, "Vida", -1.),
        "Dano deve ser um número não negativo",
    );
    refused(
        rt.damage(&owner, "Nome", 1.),
        "Atributo de dano precisa ser numérico",
    );
    refused(
        rt.damage(&boxed, "Vida", 1.),
        "Alvo não possui o atributo de dano configurado",
    );
    rt.damage(&owner, "Vida", 12.).unwrap();
    assert_eq!(rt.attribute(&owner, "Vida"), Ok(Some(&n(18.))));
    rt.damage(&owner, "Vida", 100.).unwrap();
    assert_eq!(rt.attribute(&owner, "Vida"), Ok(Some(&n(0.))));

    refused(
        rt.set_variable("nenhuma", "", n(1.)),
        "Variável do jogo não encontrada.",
    );
    refused(
        rt.set_variable(&variable, &"k".repeat(5000), n(1.)),
        "A chave passa de 4096 bytes",
    );
    let error = rt.set_variable(&variable, "", text("dez")).unwrap_err();
    assert!(
        error.starts_with("Valor incompatível com o tipo da variável Pontos: ela guarda"),
        "{error}"
    );
    rt.set_variable(&variable, "fase 2", n(5.)).unwrap();
    assert_eq!(rt.variable(&variable, "fase 2"), Some(&n(5.)));
    assert_eq!(rt.variable(&variable, ""), Some(&n(0.)));

    assert_eq!(
        rt.random_number((&owner, "dado"), 5., 1., false),
        Err("Mínimo 5 maior que o máximo 1".into())
    );
    assert_eq!(
        rt.random_number((&owner, "dado"), 1.2, 1.8, true),
        Err("Nenhum inteiro entre o mínimo 1.2 e o máximo 1.8".into())
    );
    assert!(rt.random_number((&owner, "dado"), 0., 1., false).is_ok());
    refused(
        rt.set_random_seed(1.5),
        &format!("A semente precisa ser um inteiro de 0 a {MAX_SEED}; recebeu 1.5"),
    );
    assert_eq!(
        Runtime::format_time(65.25, "mm:ss.cc"),
        Ok("01:05.25".into())
    );
    assert_eq!(
        Runtime::format_time(1., "minutos"),
        Err("Formato de tempo inválido".into())
    );

    refused(rt.play_sound("nenhum", 1.), "Asset de áudio não existe");
    // A volume no node can give: the scripts' calls refuse it as the loops and volumes do.
    let beep = project.assets[0].id.clone();
    refused(
        rt.play_sound(&beep, f64::NAN),
        "Volume precisa ser um número finito",
    );
    refused(
        rt.set_volume("music", f64::INFINITY),
        "Volume precisa ser um número finito",
    );
    assert!(rt.sounds.is_empty());
    rt.play_sound(&beep, 2.).unwrap();
    assert_eq!(rt.sounds.len(), 1);
    assert_eq!(
        (rt.sounds[0].asset.as_str(), rt.sounds[0].volume),
        (beep.as_str(), 1.)
    );
    refused(
        rt.play_animation(&owner, "nenhum", true),
        "ID de clip não existe no objeto",
    );
    refused(
        rt.set_component_enabled(&boxed, ObjectComponent::Collider, false),
        "Objeto não tem colisor nem área",
    );
    refused(
        rt.set_component_enabled(&boxed, ObjectComponent::Controller, false),
        "Objeto não tem controlador",
    );
    refused(
        rt.write_transform(
            &boxed,
            TransformPart::Scale,
            TransformSpace::World,
            Vec3::ONE,
            false,
            false,
        ),
        "A escala de Caixa só muda no espaço Local, relativa ao pai",
    );
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

#[test]
fn direct_calls_reach_the_scene_the_saved_progress_and_the_host() {
    let (project, owner, boxed) = panel();
    let variable = project.variables[0].id.clone();
    let mut rt = Runtime::new(&project, &project.start_scene).unwrap();
    rt.set_component_enabled(&boxed, ObjectComponent::Visible, false)
        .unwrap();
    assert!(!rt.scene().entity(&boxed).unwrap().visible);
    rt.write_transform(
        &boxed,
        TransformPart::Position,
        TransformSpace::Local,
        Vec3::new(2., 0., 0.),
        true,
        false,
    )
    .unwrap();
    let (position, _) = rt.object_pose(&boxed, TransformSpace::World).unwrap();
    assert_eq!(position, Vec3::new(2., 0., 0.));

    assert!(!rt.has_saved_progress());
    rt.set_variable(&variable, "", n(40.)).unwrap();
    rt.save_progress();
    assert!(rt.has_saved_progress());
    assert!(
        matches!(rt.save_requests.last(), Some(SaveRequest::Write(values))
            if values.get(&(variable.clone(), String::new())) == Some(&n(40.))),
        "{:?}",
        rt.save_requests
    );
    rt.clear_progress();
    assert!(!rt.has_saved_progress());
    assert_eq!(rt.save_requests.last(), Some(&SaveRequest::Clear));
    assert_eq!(rt.variable(&variable, ""), Some(&n(0.)));

    assert!(!rt.quit_requested);
    rt.quit_game(&owner);
    assert!(rt.quit_requested);
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
}

/// What logging a scene start that runs `action` on its owner, `owner`, leaves.
fn refusal(mut owner: Entity, kind: SceneKind, action: Node) -> Vec<String> {
    let mut project = Project::new("Recusas");
    project.scenes[0].kind = kind;
    let start = node("event.scene_start", &[]);
    wire(&mut owner.graph, &start, "exec", &action, "exec");
    owner.graph.nodes = vec![start, action];
    project.scenes[0].entities = vec![owner];
    validate_project(&project).unwrap();
    Runtime::new(&project, &project.start_scene).unwrap().logs
}
fn refused_with(logs: &[String], message: &str) {
    assert!(
        logs.len() == 1 && logs[0].contains(message),
        "{message}: {logs:?}"
    );
}

#[test]
fn nodes_refuse_unknown_choices_where_they_always_did() {
    let write = |channel: &str, mode: &str, space: &str| {
        node(
            "transform.write",
            &[
                ("channel", text(channel)),
                ("mode", text(mode)),
                ("space", text(space)),
            ],
        )
    };
    let plain = || Entity::new("Caixa", None);
    let mut body = Entity::new("Corpo", None);
    body.character3d = Some(CharacterConfig::default());
    // The object comes before the choices, and the mode, the space and the part in that order.
    refused_with(
        &refusal(body, SceneKind::ThreeD, write("girar", "?", "?")),
        "Corpo é um personagem 3D",
    );
    refused_with(
        &refusal(plain(), SceneKind::ThreeD, write("girar", "?", "?")),
        "Como deve ser Definir ou Somar",
    );
    refused_with(
        &refusal(plain(), SceneKind::ThreeD, write("girar", "set", "?")),
        "Espaço deve ser Mundo ou Local",
    );
    refused_with(
        &refusal(plain(), SceneKind::ThreeD, write("girar", "set", "world")),
        "O que muda deve ser Posição, Rotação ou Escala",
    );
    // A draw reads its range before its mode.
    let draw = |min: f64, max: f64| {
        node(
            "random.number",
            &[("min", n(min)), ("max", n(max)), ("mode", text("dado"))],
        )
    };
    refused_with(
        &refusal(plain(), SceneKind::TwoD, draw(5., 1.)),
        "Mínimo 5 maior que o máximo 1",
    );
    refused_with(
        &refusal(plain(), SceneKind::TwoD, draw(1., 5.)),
        "Modo de sorteio inválido",
    );
    refused_with(
        &refusal(
            plain(),
            SceneKind::TwoD,
            node("action.component", &[("component", text("luz"))]),
        ),
        "Componente desconhecido; use collider/controller/visible/behavior/animation/platform",
    );
    refused_with(
        &refusal(
            plain(),
            SceneKind::TwoD,
            node("action.sound_loop", &[("channel", text("música"))]),
        ),
        "Tocar em loop sem áudio escolhido: escolha o áudio no nó",
    );
}

#[test]
fn a_character_state_reads_from_outside_the_runtime() {
    let mut project = Project::new("Corpo");
    project.scenes[0].kind = SceneKind::ThreeD;
    let mut body = Entity::new("Corpo", None);
    body.transform.position = [1., 0., -2.];
    body.character3d = Some(CharacterConfig::default());
    let plain = Entity::new("Caixa", None);
    let ids = (body.id.clone(), plain.id.clone());
    project.scenes[0].entities = vec![body, plain];
    validate_project(&project).unwrap();
    let rt = Runtime::new(&project, &project.start_scene).unwrap();
    let state = rt.inspect_character(&ids.0).unwrap();
    assert_eq!((state.position.x, state.position.z), (1., -2.));
    assert_eq!(
        rt.inspect_character(&ids.1).err().as_deref(),
        Some("Objeto não possui personagem 3D")
    );
    assert_eq!(
        rt.inspect_character("nenhum").err().as_deref(),
        Some("Personagem ausente")
    );
}
