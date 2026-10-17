//! Runs repeat themselves. The replay check runs two copies of a game side by side in one
//! process, where every HashMap orders its keys its own way, and compares their states after
//! every step; the state hash compares whole runs, what plays included.
use oxy_core::{
    animation::{AnimationEvent, Clip, Interpolation, Keyframe},
    character::{CameraRig, CharacterConfig},
    document::*,
    graph::{Edge, Node},
    headless::{self, EventKind, SimulationReport, SimulationRequest},
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, Runtime, StateDifference, StateDigest},
    surface::{PlatformMode, TranslationPlatform},
};
use serde_json::json;

fn node(entity: &mut Entity, operation: &str, params: &[(&str, Value)]) -> Id {
    let mut node = Node::new(operation, [0.; 2]);
    for (key, value) in params {
        node.params.insert((*key).into(), value.clone());
    }
    let id = node.id.clone();
    entity.graph.nodes.push(node);
    id
}
/// A link from `from`'s `from_port` (exec when it is a flow) to `to`'s `to_port`.
fn wire(entity: &mut Entity, from: &str, from_port: &str, to: &str, to_port: &str) {
    entity.graph.edges.push(Edge {
        from_node: from.into(),
        from_port: from_port.into(),
        to_node: to.into(),
        to_port: to_port.into(),
    });
}
fn link(entity: &mut Entity, from: &str, to: &str) {
    wire(entity, from, "exec", to, "exec");
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn object(id: &str) -> Value {
    Value::Object(Some(id.into()))
}
fn request(value: serde_json::Value) -> SimulationRequest {
    serde_json::from_value(value).unwrap()
}
/// A cube with a solid 3D box of its size.
fn block(name: &str, size: [f32; 3], position: [f32; 3]) -> Entity {
    let mut block = Entity::new(name, Some(Primitive::Cube));
    block.dimensions = size;
    block.transform.position = position;
    block.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size },
        ..Default::default()
    });
    block
}
fn character(name: &str, position: [f32; 3]) -> Entity {
    let mut body = Entity::new(name, None);
    body.transform.position = position;
    body.character3d = Some(CharacterConfig::default());
    body
}
/// Logic that writes `value` into `attribute` of `target`, adding one to `count` there too.
fn tally(entity: &mut Entity, event: &str, target: &str, value: Value) {
    let last = node(
        entity,
        "attribute.set",
        &[
            ("target", object(target)),
            ("attribute", text("Último")),
            ("value", value),
        ],
    );
    let read = node(
        entity,
        "attribute.get",
        &[("target", object(target)), ("attribute", text("Contagem"))],
    );
    let plus = node(entity, "math.binary", &[("b", Value::Number(1.))]);
    let count = node(
        entity,
        "attribute.set",
        &[("target", object(target)), ("attribute", text("Contagem"))],
    );
    link(entity, event, &last);
    link(entity, &last, &count);
    wire(entity, &read, "value", &plus, "a");
    wire(entity, &plus, "value", &count, "value");
}
fn scoreboard() -> Entity {
    let mut board = Entity::new("Placar", None);
    board.attributes.insert("Último".into(), text(""));
    board
        .attributes
        .insert("Contagem".into(), Value::Number(0.));
    board
}

/// Runs `value` with the replay check on, which must find both copies alike at every step.
fn replayed(project: &Project, value: serde_json::Value) -> SimulationReport {
    let mut asked = request(value);
    asked.check_replay = true;
    let report = headless::simulate(project, &asked).unwrap();
    let replay = report.replay.as_ref().unwrap();
    assert!(replay.identical, "{replay:?}");
    assert_eq!(replay.first_difference, None);
    if asked.until.is_none() {
        assert_eq!(replay.steps, (asked.seconds * 60.).round() as usize);
    }
    report
}
fn attribute(report: &SimulationReport, watched: usize, name: &str) -> Value {
    report.watched[watched]
        .samples
        .last()
        .unwrap()
        .attributes
        .as_ref()
        .unwrap()[name]
        .clone()
}

/// A 2D runner that crosses the coins a Gerador copies, left or right of its path as a draw
/// says, while another draw runs every step. A coin the runner touches goes away.
fn coins() -> (Project, Id, Id) {
    let mut project = Project::new("Moedas");
    project.input_bindings.insert("gerar".into(), "G".into());
    let mut floor = Entity::new("Chão", Some(Primitive::Rectangle));
    floor.transform.position = [0., -0.5, 0.];
    floor.collider = Some(Collider {
        size: [60., 1., 1.],
        ..Default::default()
    });
    let mut runner = Entity::new("Jogador", Some(Primitive::Rectangle));
    runner.transform.position = [-8., 0.5, 0.];
    runner.controller = Some(Controller::default());
    runner.collider = Some(Collider::default());
    let runner_id = runner.id.clone();
    let mut coin = Entity::new("Moeda", Some(Primitive::Circle));
    coin.transform.position = [0., 4., 0.];
    coin.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let touched = node(&mut coin, "event.area_enter", &[]);
    let by_runner = node(
        &mut coin,
        "condition.compare",
        &[("b", object(&runner_id)), ("operator", text("=="))],
    );
    let only = node(&mut coin, "condition.branch", &[]);
    let gone = node(&mut coin, "action.remove", &[]);
    wire(&mut coin, &touched, "context", &by_runner, "a");
    wire(&mut coin, &by_runner, "result", &only, "condition");
    link(&mut coin, &touched, &only);
    wire(&mut coin, &only, "then", &gone, "exec");
    let mut maker = Entity::new("Gerador", None);
    let maker_id = maker.id.clone();
    maker.attributes.insert("Sorteio".into(), Value::Number(0.));
    maker.attributes.insert("Ruído".into(), Value::Number(0.));
    let press = node(&mut maker, "event.input", &[("action", text("gerar"))]);
    let draw = node(
        &mut maker,
        "random.number",
        &[("max", Value::Number(6.)), ("mode", text("integer"))],
    );
    let keep = node(
        &mut maker,
        "attribute.set",
        &[("attribute", text("Sorteio"))],
    );
    let high = node(
        &mut maker,
        "condition.compare",
        &[("b", Value::Number(3.)), ("operator", text(">="))],
    );
    let side = node(&mut maker, "condition.branch", &[]);
    let template = object(&coin.id);
    let mut copy = |x: f64| {
        node(
            &mut maker,
            "action.spawn",
            &[
                ("target", template.clone()),
                ("x", Value::Number(x)),
                ("y", Value::Number(-3.5)),
            ],
        )
    };
    let (right, left) = (copy(4.), copy(-3.));
    link(&mut maker, &press, &draw);
    link(&mut maker, &draw, &keep);
    wire(&mut maker, &draw, "value", &keep, "value");
    link(&mut maker, &keep, &side);
    wire(&mut maker, &draw, "value", &high, "a");
    wire(&mut maker, &high, "result", &side, "condition");
    wire(&mut maker, &side, "then", &right, "exec");
    wire(&mut maker, &side, "else", &left, "exec");
    let step = node(&mut maker, "event.step", &[]);
    let noise = node(&mut maker, "random.number", &[("min", Value::Number(-1.))]);
    let write = node(&mut maker, "attribute.set", &[("attribute", text("Ruído"))]);
    link(&mut maker, &step, &noise);
    link(&mut maker, &noise, &write);
    wire(&mut maker, &noise, "value", &write, "value");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    scene.entities = vec![floor, runner, coin, maker];
    (project, runner_id, maker_id)
}
fn running_and_making_coins(seconds: f64) -> serde_json::Value {
    let mut inputs = vec![json!({"type": "press", "at": 0.2, "action": "mover_direita"})];
    for at in [0.1, 0.5, 0.9, 1.3] {
        inputs.push(json!({"type": "press", "at": at, "action": "gerar"}));
        inputs.push(json!({"type": "release", "at": at + 0.05, "action": "gerar"}));
    }
    json!({"seconds": seconds, "inputs": inputs})
}

#[test]
fn copies_draws_and_area_events_in_2d_replay_alike() {
    let (project, runner, maker) = coins();
    let mut asked = running_and_making_coins(3.);
    asked["watch"] = json!([runner, maker]);
    let report = replayed(&project, asked.clone());
    let taken: Vec<_> = report
        .events
        .iter()
        .filter(|e| e.kind == EventKind::AreaEnter && e.object.id == runner)
        .map(|e| e.other.as_ref().unwrap().name.as_str())
        .collect();
    assert!(!taken.is_empty(), "{:?}", report.events);
    assert!(
        taken.iter().all(|name| *name == "Moeda (cópia)"),
        "{taken:?}"
    );
    assert_ne!(attribute(&report, 1, "Ruído"), Value::Number(0.));
    // The function the tests and agents use answers the same.
    assert_eq!(
        headless::check_replay(&project, &request(asked)).unwrap(),
        *report.replay.as_ref().unwrap()
    );
}

const BODY: &str = "00000000-0000-4000-8000-000000000002";

/// A bell that swings and rings at the same moment as any other bell, each ring writing its
/// name on the Placar: the last name depends on the order of the rings.
fn bell(name: &str, x: f32, board: &str) -> Entity {
    let mut bell = Entity::new(name, Some(Primitive::Sphere));
    bell.transform.position = [x, 3., -6.];
    let mut clip = Clip::new("Balanço");
    clip.looping = true;
    clip.duration = 0.5;
    clip.events.push(AnimationEvent {
        time: 0.25,
        name: "Toque".into(),
    });
    for (time, height) in [(0., 3.), (0.5, 3.5)] {
        let mut transform = bell.transform.clone();
        transform.position[1] = height;
        clip.insert_key(
            &bell.id,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    let start = node(&mut bell, "event.scene_start", &[]);
    let play = node(&mut bell, "action.animation", &[("clip", text(&clip.id))]);
    link(&mut bell, &start, &play);
    let ring = node(&mut bell, "event.animation", &[("marker", text("Toque"))]);
    tally(&mut bell, &ring, board, text(name));
    bell.clips.push(clip);
    bell
}

/// A 3D course: a runner on a moving platform that carries it through a sensor, a third
/// person camera, two bells that ring in the same steps and a clock on the screen.
fn course() -> (Project, Id) {
    let mut project = Project::new("Pista");
    let floor = block("Chão", [100., 1., 100.], [0., -0.5, 0.]);
    let mut lift = block("Plataforma", [4., 0.5, 4.], [20., 0.75, 0.]);
    lift.platform = Some(TranslationPlatform {
        mode: PlatformMode::Velocity,
        velocity: [1., 0., 0.],
        ..Default::default()
    });
    let mut body = character("Corredor", [20., 1.02, 0.]);
    body.id = BODY.into();
    let mut camera = Entity::new("Olhos", None);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(BODY.into()),
        ..Default::default()
    });
    let mut goal = Entity::new("Meta", None);
    goal.transform.position = [21.5, 2., 0.];
    goal.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size: [1., 2., 2.] },
        sensor: true,
        ..Default::default()
    });
    let board = scoreboard();
    let board_id = board.id.clone();
    let mut clock = Entity::new("Relógio", None);
    clock.attributes.insert("Tempo".into(), text(""));
    clock.ui = Some(UiElement {
        text: "{value}".into(),
        binding_attribute: "Tempo".into(),
        ..Default::default()
    });
    let step = node(&mut clock, "event.step", &[]);
    let now = node(&mut clock, "time.read", &[]);
    let format = node(&mut clock, "text.time", &[]);
    let show = node(&mut clock, "attribute.set", &[("attribute", text("Tempo"))]);
    link(&mut clock, &step, &show);
    wire(&mut clock, &now, "time", &format, "seconds");
    wire(&mut clock, &format, "text", &show, "value");
    let first = bell("Sino A", -2., &board_id);
    let second = bell("Sino B", 2., &board_id);
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.entities = vec![floor, lift, body, camera, goal, board, clock, first, second];
    (project, board_id)
}

#[test]
fn a_3d_course_with_a_platform_bells_and_a_clock_replays_alike() {
    let (project, board) = course();
    let report = replayed(
        &project,
        json!({"seconds": 2.0, "watch": [board],
               "inputs": [{"type": "press", "at": 0.5, "action": "pular"},
                          {"type": "release", "at": 0.6, "action": "pular"}]}),
    );
    let kinds: Vec<_> = report.events.iter().map(|e| e.kind).collect();
    for kind in [EventKind::Jump, EventKind::Land, EventKind::AreaEnter] {
        assert!(kinds.contains(&kind), "{kind:?}: {:?}", report.events);
    }
    // Both bells ring at 0.25, 0.75, 1.25 and 1.75 s, in scene order.
    assert_eq!(attribute(&report, 0, "Contagem"), Value::Number(8.));
    assert_eq!(attribute(&report, 0, "Último"), text("Sino B"));
    let clock = report.ui.iter().find(|ui| ui.name == "Relógio").unwrap();
    assert_eq!(clock.text.as_deref(), Some("00:02.00"));
}

/// Two 3D characters on a platform that the logic removes at 0.5 s, each writing its name on
/// the Placar as it loses its support.
fn falling_pair() -> (Project, Id) {
    let mut project = Project::new("Queda");
    let board = scoreboard();
    let board_id = board.id.clone();
    let mut bodies = Vec::new();
    for (name, x) in [("Primeiro", -1.5), ("Segundo", 1.5)] {
        let mut body = character(name, [x, 2.02, 0.]);
        let left = node(&mut body, "event.character", &[("kind", text("leave"))]);
        tally(&mut body, &left, &board_id, text(name));
        bodies.push(body);
    }
    let platform = block("Plataforma", [6., 0.5, 6.], [0., 1.75, 0.]);
    let mut trigger = Entity::new("Gatilho", None);
    let start = node(&mut trigger, "event.scene_start", &[]);
    let wait = node(
        &mut trigger,
        "control.wait",
        &[("seconds", Value::Number(0.5))],
    );
    let remove = node(
        &mut trigger,
        "action.remove",
        &[("target", object(&platform.id))],
    );
    link(&mut trigger, &start, &wait);
    link(&mut trigger, &wait, &remove);
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.entities = vec![block("Chão", [40., 1., 40.], [0., -0.5, 0.])];
    scene.entities.extend(bodies);
    scene.entities.extend([platform, trigger, board]);
    (project, board_id)
}

#[test]
fn removing_a_platform_under_two_characters_replays_alike() {
    let (project, board) = falling_pair();
    let report = replayed(&project, json!({"seconds": 1.5, "watch": [board]}));
    let left: Vec<_> = report
        .events
        .iter()
        .filter(|e| e.kind == EventKind::Leave)
        .map(|e| (e.object.name.as_str(), e.time))
        .collect();
    assert_eq!(left.len(), 2, "{:?}", report.events);
    assert_eq!(left[0].1, left[1].1, "{left:?}");
    assert_eq!(attribute(&report, 0, "Contagem"), Value::Number(2.));
    assert_eq!(attribute(&report, 0, "Último"), text("Segundo"));
}

/// A lift that its logic moves every step, with a runner riding it, and a fan that its logic
/// turns in the world every step.
fn logic_lift() -> Project {
    let mut project = Project::new("Elevador da lógica");
    let mut lift = block("Elevador", [6., 0.5, 6.], [0., 1.75, 0.]);
    lift.platform = Some(TranslationPlatform {
        mode: PlatformMode::Animation,
        ..Default::default()
    });
    let mut fan = block("Hélice", [2., 0.2, 0.2], [10., 3., 0.]);
    for (entity, channel, value) in [
        (&mut lift, "position", [0.02, 0., 0.01]),
        (&mut fan, "rotation", [0., 3., 1.]),
    ] {
        let step = node(entity, "event.step", &[]);
        let write = node(
            entity,
            "transform.write",
            &[
                ("channel", text(channel)),
                ("mode", text("add")),
                ("space", text("world")),
                ("value", Value::Vector3(value)),
            ],
        );
        link(entity, &step, &write);
    }
    let mut body = character("Corredor", [0., 2.02, 0.]);
    body.id = BODY.into();
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.entities = vec![
        block("Chão", [100., 1., 100.], [0., -0.5, 0.]),
        lift,
        body,
        fan,
    ];
    project
}

#[test]
fn a_lift_and_a_fan_that_logic_moves_replay_alike() {
    let project = logic_lift();
    let report = replayed(
        &project,
        json!({"seconds": 2.0, "watch": [{"id": BODY, "fields": ["position", "character"]}]}),
    );
    // 1.2 m/s along X from the first steps, with the runner on board.
    let last = report.watched[0].samples.last().unwrap();
    let character = last.character.as_ref().unwrap();
    assert!(character.grounded, "{last:?}");
    assert!(last.position.unwrap()[0] > 2.2, "{last:?}");
    assert!(report.logs.is_empty(), "{:?}", report.logs);
}

/// A first person runner 2 m from the centre of a turntable that a clip turns a quarter turn
/// a second, and a railing on the turntable beside it.
fn turntable() -> Project {
    let mut project = Project::new("Carrossel");
    let mut table = block("Carrossel", [10., 0.5, 10.], [0., 0.25, 0.]);
    table.platform = Some(TranslationPlatform {
        mode: PlatformMode::Animation,
        ..Default::default()
    });
    let mut clip = Clip::new("Giro");
    clip.looping = true;
    clip.duration = 4.;
    for quarter in 0..=4 {
        let mut transform = table.transform.clone();
        transform.rotation[1] = (90. * quarter as f32).to_radians();
        clip.insert_key(
            &table.id,
            Keyframe {
                time: quarter as f32,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    let start = node(&mut table, "event.scene_start", &[]);
    let play = node(&mut table, "action.animation", &[("clip", text(&clip.id))]);
    link(&mut table, &start, &play);
    table.clips.push(clip);
    let mut railing = block("Grade", [0.2, 1., 3.], [2.5, 0.75, 0.]);
    railing.parent = Some(table.id.clone());
    let mut body = character("Passageiro", [2., 0.52, 0.]);
    body.id = BODY.into();
    let mut camera = Entity::new("Olhos", None);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(BODY.into()),
        ..Default::default()
    });
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    scene.entities = vec![
        block("Chão", [100., 1., 100.], [0., -0.5, 0.]),
        table,
        railing,
        body,
        camera,
    ];
    project
}

#[test]
fn a_turntable_passenger_replays_alike() {
    let project = turntable();
    let report = replayed(
        &project,
        json!({"seconds": 2.0, "samples": 4,
               "watch": [{"id": BODY, "fields": ["character"]}],
               "inputs": [{"type": "press", "at": 1.0, "action": "pular"},
                          {"type": "release", "at": 1.1, "action": "pular"}]}),
    );
    let kinds: Vec<_> = report.events.iter().map(|e| e.kind).collect();
    for kind in [EventKind::Jump, EventKind::Land] {
        assert!(kinds.contains(&kind), "{kind:?}: {:?}", report.events);
    }
    // Turned with the turntable before the jump, and back on it at the end: on its floor or
    // on its railing.
    let samples = &report.watched[0].samples;
    let character = |i: usize| samples[i].character.as_ref().unwrap();
    assert!(
        character(0).grounded && character(0).yaw > 30.,
        "{samples:?}"
    );
    let support = character(3).support.as_ref().map(|s| s.name.as_str());
    assert!(
        matches!(support, Some("Carrossel" | "Grade")),
        "{samples:?}"
    );
    assert!(report.logs.is_empty(), "{:?}", report.logs);
}

/// A menu that opens the level after 0.3 s; the level draws a number, copies a Bloco and
/// restarts itself through Cena atual every 0.25 s.
fn restarting_level() -> Project {
    let mut project = Project::new("Fases");
    let mut level = Scene::new("Fase", SceneKind::TwoD);
    let level_id = level.id.clone();
    let mut door = Entity::new("Porta", None);
    let start = node(&mut door, "event.scene_start", &[]);
    let wait = node(
        &mut door,
        "control.wait",
        &[("seconds", Value::Number(0.3))],
    );
    let open = node(&mut door, "action.scene", &[("scene", text(&level_id))]);
    link(&mut door, &start, &wait);
    link(&mut door, &wait, &open);
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0].entities.push(door);
    let mut brick = Entity::new("Bloco", Some(Primitive::Rectangle));
    brick.transform.position = [0., 5., 0.];
    brick.collider = Some(Collider::default());
    let mut master = Entity::new("Mestre", None);
    master
        .attributes
        .insert("Sorteio".into(), Value::Number(0.));
    let start = node(&mut master, "event.scene_start", &[]);
    let draw = node(
        &mut master,
        "random.number",
        &[("max", Value::Number(100.))],
    );
    let keep = node(
        &mut master,
        "attribute.set",
        &[("attribute", text("Sorteio"))],
    );
    let copy = node(
        &mut master,
        "action.spawn",
        &[("target", object(&brick.id)), ("y", Value::Number(-2.))],
    );
    let wait = node(
        &mut master,
        "control.wait",
        &[("seconds", Value::Number(0.25))],
    );
    let here = node(&mut master, "scene.current", &[]);
    let again = node(&mut master, "action.scene", &[]);
    link(&mut master, &start, &draw);
    link(&mut master, &draw, &keep);
    wire(&mut master, &draw, "value", &keep, "value");
    link(&mut master, &keep, &copy);
    link(&mut master, &copy, &wait);
    link(&mut master, &wait, &again);
    wire(&mut master, &here, "scene", &again, "scene");
    level.entities = vec![brick, master];
    project.scenes.push(level);
    project
}

#[test]
fn a_scene_change_and_restarts_through_the_current_scene_replay_alike() {
    let project = restarting_level();
    let report = replayed(&project, json!({"seconds": 1.5}));
    let reloads = report.scene_changes.iter().filter(|c| c.reload).count();
    assert_eq!(
        report.scene_changes.len(),
        reloads + 1,
        "{:?}",
        report.scene_changes
    );
    assert!(reloads >= 3, "{:?}", report.scene_changes);
    assert_eq!(report.final_scene, project.scenes[1].id);
}

#[test]
fn the_state_hash_repeats_with_the_request_and_changes_with_the_draws() {
    let (project, _, _) = coins();
    let asked = running_and_making_coins(1.5);
    let hash = |value: serde_json::Value| {
        let report = headless::simulate(&project, &request(value)).unwrap();
        assert!(report.replay.is_none());
        report.state_hash
    };
    let first = hash(asked.clone());
    assert_eq!(first.len(), 16);
    assert!(first.chars().all(|c| c.is_ascii_hexdigit()), "{first}");
    assert_eq!(hash(asked.clone()), first);
    let mut seeded = asked.clone();
    seeded["seed"] = json!(1);
    assert_ne!(hash(seeded.clone()), first);
    // The digests say where: in the draws, which come before the objects they change, and
    // in what the draws wrote. Without draws, only the seed differs.
    let mut dice = Project::new("Dado");
    let mut die = Entity::new("Dado", None);
    die.attributes.insert("Valor".into(), Value::Number(0.));
    let step = node(&mut die, "event.step", &[]);
    let draw = node(&mut die, "random.number", &[]);
    let keep = node(&mut die, "attribute.set", &[("attribute", text("Valor"))]);
    link(&mut die, &step, &draw);
    link(&mut die, &draw, &keep);
    wire(&mut die, &draw, "value", &keep, "value");
    dice.scenes[0].kind = SceneKind::TwoD;
    dice.scenes[0].entities.push(die);
    let state = |project: &Project, seed: u64| {
        let asked = request(json!({"seconds": 0.5, "seed": seed}));
        headless::run(project, &asked).unwrap().1.state_digest()
    };
    let (zero, one) = (state(&dice, 0), state(&dice, 1));
    assert_eq!(
        zero.first_difference(&one),
        Some(StateDifference {
            object: None,
            part: "random".into()
        })
    );
    let value = |digest: &StateDigest| {
        let part = digest.parts.iter().find(|p| p.name == "attributes.Valor");
        part.unwrap().hash
    };
    assert_ne!(value(&zero), value(&one));
    dice.scenes[0].entities[0].graph = Default::default();
    let (zero, one) = (state(&dice, 0), state(&dice, 1));
    assert_eq!(zero.parts.len(), one.parts.len());
    let differing: Vec<_> = zero
        .parts
        .iter()
        .zip(&one.parts)
        .filter(|(a, b)| a != b)
        .map(|(a, _)| a.name.as_str())
        .collect();
    assert_eq!(differing, ["random"]);
}

#[test]
fn digests_name_the_object_and_the_part_where_two_runs_part() {
    let (project, runner, maker) = coins();
    let coin = project.scenes[0].entities[2].id.clone();
    let start = || {
        let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
        for _ in 0..5 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        runtime
    };
    let base = start();
    let difference = |other: &Runtime| {
        assert_ne!(base.state_hash(), other.state_hash());
        let found = base.state_digest().first_difference(&other.state_digest());
        found.map(|d| (d.object.unwrap(), d.part))
    };
    assert_eq!(base.state_hash(), start().state_hash());
    let mut moved = start();
    moved
        .scene_mut()
        .entity_mut(&runner)
        .unwrap()
        .transform
        .position[0] += 1.;
    assert_eq!(
        difference(&moved),
        Some((runner.clone(), "transform".into()))
    );
    let mut drawn = start();
    drawn
        .scene_mut()
        .entity_mut(&maker)
        .unwrap()
        .attributes
        .insert("Sorteio".into(), Value::Number(5.));
    assert_eq!(
        difference(&drawn),
        Some((maker, "attributes.Sorteio".into()))
    );
    let mut hidden = start();
    hidden.scene_mut().entity_mut(&runner).unwrap().collider = None;
    assert_eq!(difference(&hidden), Some((runner, "document".into())));
    let mut taken = start();
    taken.remove_object(&coin);
    assert_eq!(difference(&taken), Some((coin, "exists".into())));
}

/// A radio that loops its music and the wind, turns the music down to half, stops the wind
/// with a fade after 0.3 s and restarts the scene 0.3 s later.
fn radio() -> Project {
    let mut project = Project::new("Rádio");
    let mut assets = Vec::new();
    for name in ["Trilha", "Vento"] {
        let asset = Asset {
            id: new_id(),
            name: name.into(),
            path: format!("assets/{name}.wav"),
            kind: AssetKind::Audio,
            model: None,
        };
        assets.push(asset.id.clone());
        project.assets.push(asset);
    }
    let mut radio = Entity::new("Rádio", None);
    let start = node(&mut radio, "event.scene_start", &[]);
    let music = node(
        &mut radio,
        "action.sound_loop",
        &[("asset", text(&assets[0])), ("fade", Value::Number(1.))],
    );
    let wind = node(
        &mut radio,
        "action.sound_loop",
        &[("asset", text(&assets[1])), ("channel", text("effects"))],
    );
    let quieter = node(
        &mut radio,
        "action.volume",
        &[("channel", text("music")), ("volume", Value::Number(0.5))],
    );
    let wait = node(
        &mut radio,
        "control.wait",
        &[("seconds", Value::Number(0.3))],
    );
    let calm = node(
        &mut radio,
        "action.sound_stop",
        &[("channel", text("effects")), ("fade", Value::Number(0.2))],
    );
    let again = node(
        &mut radio,
        "control.wait",
        &[("seconds", Value::Number(0.3))],
    );
    let here = node(&mut radio, "scene.current", &[]);
    let restart = node(&mut radio, "action.scene", &[]);
    for pair in [
        start,
        music,
        wind,
        quieter,
        wait,
        calm,
        again,
        restart.clone(),
    ]
    .windows(2)
    {
        link(&mut radio, &pair[0], &pair[1]);
    }
    wire(&mut radio, &here, "scene", &restart, "scene");
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0].entities.push(radio);
    project
}

#[test]
fn music_loops_and_volumes_replay_alike_and_are_part_of_the_state() {
    let project = radio();
    let report = replayed(&project, json!({"seconds": 2.0}));
    let reloads = report.scene_changes.iter().filter(|c| c.reload).count();
    assert!(reloads >= 2, "{:?}", report.scene_changes);
    // The music started once; the wind starts with each scene and stops within it.
    let kinds: Vec<_> = report
        .sounds
        .iter()
        .map(|s| (s.text.as_str(), s.kind.map(|k| format!("{k:?}"))))
        .collect();
    assert_eq!(
        kinds.iter().filter(|(text, _)| *text == "Trilha").count(),
        1,
        "{kinds:?}"
    );
    assert!(
        kinds.iter().filter(|(text, _)| *text == "Vento").count() > 2,
        "{kinds:?}"
    );
    // Runs apart only in a volume differ first in the audio, before any object.
    let state = |project: &Project| {
        let asked = request(json!({"seconds": 0.1}));
        headless::run(project, &asked).unwrap().1.state_digest()
    };
    let mut quieter = project.clone();
    let volume = quieter.scenes[0].entities[0]
        .graph
        .nodes
        .iter_mut()
        .find(|node| node.operation == "action.volume")
        .unwrap();
    volume.params.insert("volume".into(), Value::Number(0.25));
    let (half, quarter) = (state(&project), state(&quieter));
    assert_eq!(
        half.first_difference(&quarter),
        Some(StateDifference {
            object: None,
            part: "audio".into()
        })
    );
}
