//! The headless simulation used by agents: inputs across scene changes, the readable report,
//! movement and camera detail, setup, spawn and stop conditions on the copy, and the game
//! interface at the end.
use oxy_core::{
    character::{CameraRig, CharacterConfig},
    document::*,
    graph::{Edge, Node},
    headless::{self, Actions, EventKind, SimulationReport, SimulationRequest},
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, MAX_STEPS, Runtime},
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
fn link(entity: &mut Entity, from: &str, to: &str) {
    entity.graph.edges.push(Edge {
        from_node: from.into(),
        from_port: "exec".into(),
        to_node: to.into(),
        to_port: "exec".into(),
    });
}
/// A data link, e.g. an event's "context" into an action's "target".
fn wire(entity: &mut Entity, from: &str, from_port: &str, to: &str, to_port: &str) {
    entity.graph.edges.push(Edge {
        from_node: from.into(),
        from_port: from_port.into(),
        to_node: to.into(),
        to_port: to_port.into(),
    });
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn request(value: serde_json::Value) -> SimulationRequest {
    serde_json::from_value(value).unwrap()
}
fn run(project: &Project, value: serde_json::Value) -> SimulationReport {
    let before = project.clone();
    let report = headless::simulate(project, &request(value)).unwrap();
    assert_eq!(*project, before, "the simulation runs on a copy");
    report
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.05
}

/// A menu that opens the level after 1 s; the level logs jumps and the release of a key
/// held across the change, and reloads itself on "reiniciar".
fn menu_and_level() -> (Project, Id) {
    let mut project = Project::new("Troca de cena");
    // Simulations press only actions the project has.
    for (action, key) in [("segurar", "H"), ("reiniciar", "R")] {
        project.input_bindings.insert(action.into(), key.into());
    }
    project.scenes[0].kind = SceneKind::TwoD;
    let mut level = Scene::new("Fase", SceneKind::TwoD);
    let level_id = level.id.clone();
    let mut menu = Entity::new("Menu", None);
    let start = node(&mut menu, "event.scene_start", &[]);
    let wait = node(&mut menu, "control.wait", &[("seconds", Value::Number(1.))]);
    let open = node(&mut menu, "action.scene", &[("scene", text(&level_id))]);
    link(&mut menu, &start, &wait);
    link(&mut menu, &wait, &open);
    project.scenes[0].entities.push(menu);
    let mut hero = Entity::new("Herói", None);
    let jump = node(&mut hero, "event.input", &[("action", text("pular"))]);
    let jumped = node(&mut hero, "debug.message", &[("message", text("pulou"))]);
    link(&mut hero, &jump, &jumped);
    let release = node(
        &mut hero,
        "event.input",
        &[("action", text("segurar")), ("mode", text("released"))],
    );
    let released = node(&mut hero, "debug.message", &[("message", text("soltou"))]);
    link(&mut hero, &release, &released);
    let restart = node(&mut hero, "event.input", &[("action", text("reiniciar"))]);
    let reload = node(&mut hero, "action.scene", &[("scene", text(&level_id))]);
    link(&mut hero, &restart, &reload);
    level.entities.push(hero);
    project.scenes.push(level);
    (project, level_id)
}

#[test]
fn inputs_survive_scene_changes_and_reloads_are_reported() {
    let (project, level) = menu_and_level();
    let inputs = json!([
        {"type": "press", "at": 0.5, "action": "segurar"},
        {"type": "release", "at": 1.5, "action": "segurar"},
        {"type": "press", "at": 2.0, "action": "pular"},
        {"type": "release", "at": 2.1, "action": "pular"},
        {"type": "press", "at": 2.5, "action": "reiniciar"},
        {"type": "release", "at": 2.6, "action": "reiniciar"},
        {"type": "press", "at": 3.0, "action": "pular"},
    ]);
    let report = run(&project, json!({"seconds": 4.0, "inputs": inputs}));
    let changes: Vec<_> = report
        .scene_changes
        .iter()
        .map(|c| (c.scene.as_str(), c.name.as_str(), c.reload))
        .collect();
    assert_eq!(
        changes,
        [
            (level.as_str(), "Fase", false),
            (level.as_str(), "Fase", true)
        ]
    );
    assert!(
        near(report.scene_changes[0].time, 1.0),
        "{:?}",
        report.scene_changes
    );
    assert!(
        near(report.scene_changes[1].time, 2.5),
        "{:?}",
        report.scene_changes
    );
    let logs: Vec<_> = report
        .logs
        .iter()
        .map(|l| (l.text.as_str(), l.time))
        .collect();
    assert_eq!(logs.len(), 3, "{logs:?}");
    assert!(logs[0].0 == "soltou" && near(logs[0].1, 1.5), "{logs:?}");
    assert!(logs[1].0 == "pulou" && near(logs[1].1, 2.0), "{logs:?}");
    assert!(logs[2].0 == "pulou" && near(logs[2].1, 3.0), "{logs:?}");
    assert_eq!(report.logs[1].object.as_ref().unwrap().name, "Herói");
    assert_eq!(report.final_scene, level);

    let stop = run(
        &project,
        json!({"seconds": 4.0, "inputs": inputs, "until": {"scene_change": true}}),
    );
    let stopped = stop.stopped_at.unwrap();
    assert_eq!(stopped.reason, "scene_change");
    assert!(near(stopped.time, 1.0) && stopped.detail.contains("Fase"));
    let stop = run(
        &project,
        json!({"seconds": 4.0, "inputs": inputs, "until": {"log_contains": "pulou"}}),
    );
    let stopped = stop.stopped_at.unwrap();
    assert!(stopped.reason == "log_contains" && near(stopped.time, 2.0));
}

/// A 2D scene where "reiniciar" plays "Bip" and then restarts the scene; with `on_start`, the
/// scene does the same as it starts, so it restarts itself until the work budget runs out.
fn beep_then_restart(on_start: bool) -> Project {
    let mut project = Project::new("Som e reinício");
    project
        .input_bindings
        .insert("reiniciar".into(), "R".into());
    project.assets.push(Asset {
        id: "00000000-0000-4000-8000-0000000000b1".into(),
        name: "Bip".into(),
        path: "assets/bip.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let scene_id = scene.id.clone();
    let mut control = Entity::new("Controle", None);
    let event = if on_start {
        node(&mut control, "event.scene_start", &[])
    } else {
        node(
            &mut control,
            "event.input",
            &[("action", text("reiniciar"))],
        )
    };
    let beep = node(
        &mut control,
        "action.sound",
        &[("asset", text("00000000-0000-4000-8000-0000000000b1"))],
    );
    let restart = node(&mut control, "action.scene", &[("scene", text(&scene_id))]);
    link(&mut control, &event, &beep);
    link(&mut control, &beep, &restart);
    scene.entities.push(control);
    project
}

#[test]
fn a_sound_played_right_before_a_scene_change_is_kept() {
    // The hosts (editor, player) take the sounds after the step that changed scene.
    let project = beep_then_restart(false);
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    let press = InputFrame {
        pressed: ["reiniciar".to_owned()].into(),
        ..Default::default()
    };
    runtime.advance(FIXED_DT, &press);
    assert_eq!(
        runtime.scene_starts,
        std::slice::from_ref(&project.start_scene)
    );
    let sounds = std::mem::take(&mut runtime.sounds);
    assert_eq!(sounds.len(), 1, "the beep before the restart");
    assert_eq!(sounds[0].asset, project.assets[0].id);
    // And the report lists it.
    let report = run(
        &project,
        json!({"seconds": 2.0, "inputs": [
            {"type": "press", "at": 0.5, "action": "reiniciar"},
            {"type": "release", "at": 0.6, "action": "reiniciar"},
            {"type": "press", "at": 1.5, "action": "reiniciar"},
        ]}),
    );
    assert_eq!(report.scene_changes.len(), 2);
    let sounds: Vec<_> = report
        .sounds
        .iter()
        .map(|s| (s.text.as_str(), s.time))
        .collect();
    assert_eq!(sounds.len(), 2, "{sounds:?}");
    assert!(sounds.iter().all(|(name, _)| *name == "Bip"));
    assert!(
        near(sounds[0].1, 0.5) && near(sounds[1].1, 1.5),
        "{sounds:?}"
    );

    // A scene that restarts itself as it starts runs until the work budget of the update runs
    // out. The beep that started the loop is heard, and at most the one of the scene running at
    // the end; those of the scenes that started and ended within the step are not.
    let project = beep_then_restart(true);
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    for update in 0..4 {
        // The host takes the sounds once per update (a frame of up to MAX_STEPS steps).
        let starts = std::mem::take(&mut runtime.scene_starts);
        let sounds = std::mem::take(&mut runtime.sounds);
        assert!(
            starts.len() > 2,
            "update {update}: the scene keeps restarting"
        );
        assert!(sounds.len() <= 2, "update {update}: {sounds:?}");
        runtime.advance(FIXED_DT * MAX_STEPS as f32, &InputFrame::default());
    }
}

#[test]
fn every_sound_of_a_step_is_kept() {
    // Without scene changes nothing is dropped, however many sounds a step plays.
    let mut project = beep_then_restart(false);
    let control = &mut project.scenes[0].entities[0];
    // "reiniciar" plays 20 beeps and no longer restarts.
    control.graph.nodes.truncate(1);
    control.graph.edges.clear();
    let mut previous = control.graph.nodes[0].id.clone();
    for _ in 0..20 {
        let beep = node(
            control,
            "action.sound",
            &[("asset", text("00000000-0000-4000-8000-0000000000b1"))],
        );
        link(control, &previous, &beep);
        previous = beep;
    }
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    let press = InputFrame {
        pressed: ["reiniciar".to_owned()].into(),
        ..Default::default()
    };
    runtime.advance(FIXED_DT, &press);
    assert_eq!(runtime.sounds.len(), 20, "{:?}", runtime.logs);
    assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);
    // The report lists the same as the game plays.
    let report = run(
        &project,
        json!({"seconds": 0.5, "inputs": [{"type": "press", "at": 0.25, "action": "reiniciar"}]}),
    );
    assert_eq!(report.sounds.len(), 20, "{:?}", report.logs);
}

/// "Relógio" runs logic every step and greets at the start; "Quebrado" fails at the start.
fn clock() -> (Project, Id, Id) {
    let mut project = Project::new("Relatório");
    project.scenes[0].kind = SceneKind::TwoD;
    let mut clock = Entity::new("Relógio", None);
    clock.attributes.insert("tempo".into(), Value::Number(0.));
    clock.attributes.insert("vida".into(), Value::Number(3.));
    let step = node(&mut clock, "event.step", &[]);
    let tick = node(
        &mut clock,
        "attribute.set",
        &[("attribute", text("tempo")), ("value", Value::Number(1.))],
    );
    link(&mut clock, &step, &tick);
    let start = node(&mut clock, "event.scene_start", &[]);
    let hello = node(&mut clock, "debug.message", &[("message", text("Olá"))]);
    link(&mut clock, &start, &hello);
    let mut broken = Entity::new("Quebrado", None);
    broken.attributes.insert("tempo".into(), Value::Number(0.));
    let start = node(&mut broken, "event.scene_start", &[]);
    let fail = node(
        &mut broken,
        "attribute.set",
        &[
            ("attribute", text("tempo")),
            ("value", text("não é número")),
        ],
    );
    link(&mut broken, &start, &fail);
    let ids = (clock.id.clone(), broken.id.clone());
    project.scenes[0].entities = vec![clock, broken];
    (project, ids.0, ids.1)
}
fn summary(report: &SimulationReport) -> Vec<(String, usize)> {
    match &report.actions {
        Some(Actions::Summary(lines)) => lines.iter().map(|l| (l.text.clone(), l.count)).collect(),
        other => panic!("expected a summary: {other:?}"),
    }
}

#[test]
fn the_report_summarizes_actions_and_names_logs() {
    let (project, clock, broken) = clock();
    let report = run(&project, json!({"seconds": 3.0, "watch": [clock]}));
    let lines = summary(&report);
    assert!(
        lines.contains(&("Relógio · Ao iniciar cena".into(), 1)),
        "{lines:?}"
    );
    assert!(lines.contains(&("Relógio · Mensagem de diagnóstico".into(), 1)));
    assert!(
        !lines
            .iter()
            .any(|(text, _)| text.contains("A cada passo") || text == "Relógio · Alterar atributo"),
        "per-step logic is left out by default: {lines:?}"
    );
    assert!(report.truncated.is_empty(), "{:?}", report.truncated);
    let hello = report.logs.iter().find(|l| l.text == "Olá").unwrap();
    assert_eq!(hello.object.as_ref().unwrap().name, "Relógio");
    assert_eq!(hello.node.as_ref().unwrap().operation, "debug.message");
    assert_eq!(hello.time, 0.);
    let failure = report
        .logs
        .iter()
        .find(|l| l.text.contains("incompatível"))
        .unwrap();
    assert_eq!(failure.object.as_ref().unwrap().id, broken);
    assert_eq!(failure.node.as_ref().unwrap().operation, "attribute.set");
    assert!(!failure.text.contains(&broken), "{}", failure.text);

    let everything = run(
        &project,
        json!({"seconds": 3.0, "actions_filter": {"exclude_operations": []}}),
    );
    let ticks = match &everything.actions {
        Some(Actions::Summary(lines)) => lines
            .iter()
            .find(|l| l.text == "Relógio · Alterar atributo")
            .cloned()
            .unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(ticks.count, 180);
    assert!(
        near(ticks.first, 1. / 60.) && near(ticks.last, 3.0),
        "{ticks:?}"
    );

    let all = run(
        &project,
        json!({"seconds": 3.0, "actions": "all", "actions_filter": {"exclude_operations": []}}),
    );
    let Some(Actions::All(actions)) = &all.actions else {
        panic!("{:?}", all.actions);
    };
    assert_eq!(actions.len(), headless::MAX_ENTRIES);
    assert!(all.truncated["actions"] > 0, "{:?}", all.truncated);
    assert!(near(actions.last().unwrap().time, 3.0));

    let only_broken = run(
        &project,
        json!({"seconds": 0.5, "actions_filter": {"objects": [broken]}}),
    );
    assert!(
        summary(&only_broken)
            .iter()
            .all(|(text, _)| text.starts_with("Quebrado · "))
    );
    let messages = run(
        &project,
        json!({"seconds": 0.5, "actions_filter": {"operations": ["debug.message"]}}),
    );
    assert_eq!(
        summary(&messages),
        [("Relógio · Mensagem de diagnóstico".to_owned(), 1)]
    );
    let none = run(&project, json!({"seconds": 0.5, "actions": "none"}));
    assert!(none.actions.is_none());
    let json = serde_json::to_value(&report).unwrap();
    assert!(json["actions"][0]["count"].is_number(), "{json}");
    assert!(json.get("truncated").is_none() && json.get("stopped_at").is_none());
}

#[test]
fn watch_entries_choose_fields_and_attributes() {
    let (project, clock, _) = clock();
    let report = run(
        &project,
        json!({"seconds": 0.5, "watch": [
            {"id": clock, "attributes": ["vida"]},
            {"id": clock, "attributes": false, "fields": ["position", "rotation", "scale", "visible"]},
        ]}),
    );
    let only_life = report.watched[0].samples[0].attributes.as_ref().unwrap();
    assert_eq!(only_life.keys().collect::<Vec<_>>(), ["vida"]);
    let shape = &report.watched[1].samples[0];
    assert!(shape.attributes.is_none() && shape.character.is_none());
    assert_eq!(
        (shape.rotation, shape.scale, shape.visible),
        (Some([0.; 3]), Some([1.; 3]), Some(true))
    );
    let error = headless::simulate(
        &project,
        &request(json!({"seconds": 0.5, "watch": [{"id": clock, "attributes": ["mana"]}]})),
    )
    .unwrap_err();
    assert!(error.contains("mana"), "{error}");
    assert!(
        serde_json::from_value::<SimulationRequest>(
            json!({"seconds": 1.0, "watch": [{"id": clock, "campo": 1}]})
        )
        .is_err()
    );
}

#[test]
fn watched_rotation_reads_like_the_queries_past_a_quarter_turn() {
    let (mut project, clock, _) = clock();
    let scene = project.start_scene.clone();
    project
        .scene_mut(&scene)
        .unwrap()
        .entity_mut(&clock)
        .unwrap()
        .transform
        .rotation = [0., 120f32.to_radians(), 0.];
    let report = run(
        &project,
        json!({"seconds": 0.2, "watch": [{"id": clock, "fields": ["rotation"]}]}),
    );
    let [x, y, z] = report.watched[0].samples[0].rotation.unwrap();
    assert!(
        near(x.into(), 0.) && near(y.into(), 120.) && near(z.into(), 0.),
        "{:?}",
        [x, y, z]
    );
}

#[test]
fn samples_are_as_many_as_asked_and_end_at_the_final_state() {
    let (project, clock, _) = clock();
    let times = |seconds: f64, samples: u32| -> Vec<f64> {
        run(
            &project,
            json!({"seconds": seconds, "samples": samples, "watch": [clock]}),
        )
        .watched[0]
            .samples
            .iter()
            .map(|s| s.time)
            .collect()
    };
    // 60 and 240 steps are not multiples of 7 and 9: no extra sample before the last.
    for (seconds, samples) in [(1., 7), (4., 9), (2., 120), (1., 60), (3., 4)] {
        let times = times(seconds, samples);
        assert_eq!(times.len(), samples as usize, "{seconds} s: {times:?}");
        assert_eq!(*times.last().unwrap(), seconds);
        let spacing = seconds / f64::from(samples);
        for (k, time) in times.iter().enumerate() {
            assert!(
                // Half a step at most, plus the report's rounding to 0.1 ms.
                (time - spacing * (k + 1) as f64).abs() <= 1. / 120. + 1e-4,
                "sample {k} of {samples} in {seconds} s: {times:?}"
            );
        }
    }
    assert_eq!(times(1., 0), [1.]);
    assert_eq!(times(1., 1), [1.]);
    // Fewer steps than samples: one per step.
    assert_eq!(times(0.05, 7), [0.0167, 0.0333, 0.05]);
}

const BODY: &str = "00000000-0000-4000-8000-000000000002";
const GOAL: &str = "00000000-0000-4000-8000-000000000004";

/// A floor, a 3D character, a third person camera and a sensor around the start.
fn course() -> Project {
    let mut project = Project::new("Pista");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::ThreeD;
    let mut floor = Entity::new("Chão", Some(Primitive::Cube));
    floor.dimensions = [100., 1., 100.];
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: floor.dimensions,
        },
        ..Default::default()
    });
    let mut body = Entity::new("Corredor", None);
    body.id = BODY.into();
    body.character3d = Some(CharacterConfig::default());
    body.transform.position[1] = 0.02;
    let mut camera = Entity::new("Olhos", None);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(BODY.into()),
        ..Default::default()
    });
    let mut goal = Entity::new("Meta", None);
    goal.id = GOAL.into();
    goal.transform.position = [0., 1., 0.];
    goal.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size: [2., 2., 2.] },
        sensor: true,
        ..Default::default()
    });
    scene.entities = vec![floor, body, camera, goal];
    project
}

#[test]
fn movement_camera_and_area_events_are_reported() {
    let project = course();
    let report = run(
        &project,
        json!({"seconds": 2.0, "samples": 4,
               "watch": [{"id": BODY, "fields": ["position", "character"]}],
               "inputs": [{"type": "press", "at": 0.5, "action": "pular"},
                          {"type": "release", "at": 0.6, "action": "pular"}]}),
    );
    let kinds: Vec<_> = report
        .events
        .iter()
        .filter(|e| e.object.id == BODY)
        .map(|e| e.kind)
        .collect();
    // Settling on the floor at the start is a landing too.
    assert_eq!(
        kinds[..2],
        [EventKind::Land, EventKind::AreaEnter],
        "{:?}",
        report.events
    );
    let jump = report
        .events
        .iter()
        .position(|e| e.kind == EventKind::Jump)
        .expect("jump");
    let land = report
        .events
        .iter()
        .rposition(|e| e.kind == EventKind::Land)
        .expect("land");
    assert!(jump < land && near(report.events[jump].time, 0.5));
    let landing = &report.events[land];
    assert_eq!(landing.object.name, "Corredor");
    assert_eq!(landing.other.as_ref().unwrap().name, "Chão");
    assert!(landing.speed.unwrap() > 0.);
    let area = report
        .events
        .iter()
        .find(|e| e.kind == EventKind::AreaEnter)
        .unwrap();
    assert_eq!(area.other.as_ref().unwrap().name, "Meta");
    let samples = &report.watched[0].samples;
    assert_eq!(samples.len(), 4);
    let rising = samples
        .iter()
        .find(|s| s.character.as_ref().is_some_and(|c| !c.grounded));
    assert!(rising.is_some(), "{samples:?}");
    let last = samples.last().unwrap();
    assert!(last.attributes.is_none() && last.visible.is_none());
    let character = last.character.as_ref().unwrap();
    assert!(character.grounded && character.posture == "standing" && !character.sliding);
    assert_eq!(character.support.as_ref().unwrap().name, "Chão");
    let camera = report.camera.as_ref().unwrap();
    assert_eq!((camera.active.name.as_str(), camera.fov), ("Olhos", 60.));
    assert!(["first_person", "third_person"].contains(&camera.mode));
    let forward = glam::Vec3::from(camera.forward);
    assert!((forward.length() - 1.).abs() < 1e-3);

    let stop = run(
        &project,
        json!({"seconds": 2.0, "until": {"event": {"kind": "jump", "object": BODY}},
               "inputs": [{"type": "press", "at": 0.5, "action": "pular"}]}),
    );
    let stopped = stop.stopped_at.unwrap();
    assert!(
        stopped.reason == "event" && near(stopped.time, 0.5),
        "{stopped:?}"
    );
    // The detail is in words; the kind stays as `until` and `events` name it.
    assert_eq!(stopped.detail, "Corredor pulou");
    let json = serde_json::to_value(&stopped).unwrap();
    assert_eq!(json["kind"], "jump", "{json}");
    let landed = run(
        &project,
        json!({"seconds": 2.0, "until": {"event": {"kind": "land", "object": BODY}}}),
    )
    .stopped_at
    .unwrap();
    assert_eq!(landed.detail, "Corredor aterrissou em Chão");
    assert_eq!(landed.kind, Some(EventKind::Land));
}

#[test]
fn trigger_areas_report_enter_and_exit_in_2d() {
    let mut project = Project::new("Áreas");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let mut zone = Entity::new("Zona", None);
    zone.collider = Some(Collider {
        size: [2., 2., 1.],
        is_trigger: true,
        ..Default::default()
    });
    let zone_id = zone.id.clone();
    let mut boxed = Entity::new("Caixa", Some(Primitive::Rectangle));
    boxed.controller = Some(Controller::default());
    boxed.collider = Some(Collider::default());
    scene.entities = vec![zone, boxed];
    let report = run(
        &project,
        json!({"seconds": 3.0, "until": {"event": {"kind": "area_exit", "object": zone_id}}}),
    );
    let events: Vec<_> = report
        .events
        .iter()
        .map(|e| {
            (
                e.kind,
                e.object.name.as_str(),
                e.other.as_ref().unwrap().name.as_str(),
            )
        })
        .collect();
    assert_eq!(
        events,
        [
            (EventKind::AreaEnter, "Caixa", "Zona"),
            (EventKind::AreaExit, "Caixa", "Zona")
        ]
    );
    let stopped = report.stopped_at.unwrap();
    assert!(stopped.reason == "event" && stopped.time < 3.0);
}

#[test]
fn setup_and_spawn_change_only_the_copy_and_until_stops_early() {
    let project = course();
    let report = run(
        &project,
        json!({"seconds": 5.0,
               "setup": [
                   {"op": "create_entity", "kind": {"type": "group"}, "name": "Cronômetro"},
                   {"op": "set_attribute", "entity": "$0", "name": "tempo", "value": {"Number": 0}},
                   {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
                   {"op": "add_node", "entity": "$0", "operation": "control.wait",
                    "params": {"seconds": {"Number": 0.5}}},
                   {"op": "add_node", "entity": "$0", "operation": "attribute.set",
                    "params": {"attribute": {"Text": "tempo"}, "value": {"Number": 7}}},
                   {"op": "connect", "entity": "$0", "from_node": "$2", "from_port": "exec",
                    "to_node": "$3", "to_port": "exec"},
                   {"op": "connect", "entity": "$0", "from_node": "$3", "from_port": "exec",
                    "to_node": "$4", "to_port": "exec"},
                   {"op": "rename_entity", "entity": GOAL, "name": "Chegada"},
               ],
               "spawn": {"entity": BODY, "position": [3.0, 0.02, -2.0], "yaw": 90.0},
               "watch": [{"id": "$0", "attributes": ["tempo"]},
                         {"id": BODY, "fields": ["position", "rotation"]}],
               "until": {"attribute": {"entity": "$0", "name": "tempo", "op": ">=", "value": 7}}}),
    );
    assert_eq!(report.setup.len(), 8);
    let timer = &report.watched[0];
    assert_eq!(
        (timer.id.as_str(), timer.name.as_str()),
        (report.setup[0].created[0].as_str(), "Cronômetro")
    );
    assert_eq!(
        timer.samples[0].attributes.as_ref().unwrap()["tempo"],
        Value::Number(7.)
    );
    let stopped = report.stopped_at.as_ref().unwrap();
    assert!(
        stopped.reason == "attribute" && near(stopped.time, 0.5) && stopped.kind.is_none(),
        "{stopped:?}"
    );
    assert!(
        stopped.detail.contains("Cronômetro.tempo >= 7"),
        "{}",
        stopped.detail
    );
    let runner = report.watched[1].samples.last().unwrap();
    let [x, _, z] = runner.position.unwrap();
    assert!((x - 3.).abs() < 0.01 && (z + 2.).abs() < 0.01, "{runner:?}");
    assert!(
        (runner.rotation.unwrap()[1] - 90.).abs() < 0.5,
        "{runner:?}"
    );
    assert!(
        project.scenes[0].entity(GOAL).unwrap().name == "Meta",
        "setup renamed only the copy"
    );
}

#[test]
fn until_entity_absent_and_invalid_options_are_refused() {
    let mut project = Project::new("Coleta");
    project.scenes[0].kind = SceneKind::TwoD;
    let mut coin = Entity::new("Moeda", None);
    let start = node(&mut coin, "event.scene_start", &[]);
    let wait = node(
        &mut coin,
        "control.wait",
        &[("seconds", Value::Number(0.25))],
    );
    let remove = node(&mut coin, "action.remove", &[]);
    link(&mut coin, &start, &wait);
    link(&mut coin, &wait, &remove);
    let coin_id = coin.id.clone();
    project.scenes[0].entities.push(coin);
    let report = run(
        &project,
        json!({"seconds": 2.0, "watch": [coin_id], "until": {"entity_absent": coin_id}}),
    );
    let stopped = report.stopped_at.unwrap();
    assert!(stopped.reason == "entity_absent" && near(stopped.time, 0.25));
    assert!(!report.watched[0].samples.last().unwrap().present);

    let before = project.clone();
    for (value, expected) in [
        (json!({"seconds": 1.0, "until": {}}), "ao menos uma"),
        (
            json!({"seconds": 1.0, "until": {"attribute":
                {"entity": coin_id, "name": "vida", "op": "<=", "value": 0}}}),
            "vida",
        ),
        (
            json!({"seconds": 1.0, "spawn": {"entity": "nada", "position": [0, 0, 0]}}),
            "spawn",
        ),
        (json!({"seconds": 1.0, "watch": ["$3"]}), "$3"),
        (
            json!({"seconds": 1.0, "setup": [{"op": "rename_entity", "entity": "nada", "name": "X"}]}),
            "Preparação",
        ),
    ] {
        let error = headless::simulate(&project, &request(value)).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    assert_eq!(project, before);
}

/// What the report shows of the area events in `report`, as (kind, object, other) names.
fn area_events(report: &SimulationReport) -> Vec<(EventKind, String, String)> {
    report
        .events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::AreaEnter | EventKind::AreaExit))
        .map(|e| {
            (
                e.kind,
                e.object.name.clone(),
                e.other.as_ref().unwrap().name.clone(),
            )
        })
        .collect()
}
fn trigger(name: &str, position: [f32; 3]) -> Entity {
    let mut area = Entity::new(name, None);
    area.transform.position = position;
    area.collider = Some(Collider {
        size: [2., 2., 1.],
        is_trigger: true,
        ..Default::default()
    });
    area
}
fn solid(name: &str, position: [f32; 3]) -> Entity {
    let mut solid = Entity::new(name, Some(Primitive::Rectangle));
    solid.transform.position = position;
    solid.collider = Some(Collider::default());
    solid
}

#[test]
fn areas_that_remove_themselves_or_the_visitor_still_report_the_crossing() {
    let mut project = Project::new("Coleta");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    // A coin removes itself; lava removes whatever enters it.
    let mut coin = trigger("Moeda", [0.; 3]);
    let enter = node(&mut coin, "event.area_enter", &[]);
    let remove = node(&mut coin, "action.remove", &[]);
    link(&mut coin, &enter, &remove);
    let mut lava = trigger("Lava", [10., 0., 0.]);
    let enter = node(&mut lava, "event.area_enter", &[]);
    let remove = node(&mut lava, "action.remove", &[]);
    link(&mut lava, &enter, &remove);
    wire(&mut lava, &enter, "context", &remove, "target");
    let player = solid("Jogador", [10., 0., 0.]);
    let (coin_id, player_id) = (coin.id.clone(), player.id.clone());
    scene.entities = vec![coin, solid("Caixa", [0.; 3]), lava, player];
    let report = run(&project, json!({"seconds": 1.0}));
    let mut events = area_events(&report);
    events.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        events,
        [
            (EventKind::AreaEnter, "Caixa".into(), "Moeda".into()),
            (EventKind::AreaEnter, "Jogador".into(), "Lava".into()),
        ]
    );
    assert!(report.events.iter().all(|e| near(e.time, 1. / 60.)));
    for (object, detail) in [
        (&coin_id, "Caixa entrou em Moeda"),
        (&player_id, "Jogador entrou em Lava"),
    ] {
        let stop = run(
            &project,
            json!({"seconds": 1.0, "until": {"event": {"kind": "area_enter", "object": object}}}),
        );
        let stopped = stop.stopped_at.expect("the crossing stops the run");
        assert!(near(stopped.time, 1. / 60.), "{stopped:?}");
        assert_eq!(
            (stopped.kind, stopped.detail.as_str()),
            (Some(EventKind::AreaEnter), detail)
        );
    }
}

#[test]
fn a_3d_sensor_that_removes_itself_still_reports_the_entry() {
    let mut project = course();
    let goal = project.scenes[0].entity_mut(GOAL).unwrap();
    let enter = node(goal, "event.area_enter", &[]);
    let remove = node(goal, "action.remove", &[]);
    link(goal, &enter, &remove);
    let report = run(&project, json!({"seconds": 1.0}));
    assert_eq!(
        area_events(&report),
        [(EventKind::AreaEnter, "Corredor".into(), "Meta".into())],
        "{:?}",
        report.events
    );
    let stop = run(
        &project,
        json!({"seconds": 1.0, "until": {"event": {"kind": "area_enter", "object": GOAL}}}),
    );
    let stopped = stop.stopped_at.expect("the entry stops the run");
    assert!(stopped.time < 0.5, "{stopped:?}");
}

#[test]
fn an_area_that_turns_off_its_collider_reports_no_exit() {
    let mut project = Project::new("Interruptor");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    // Turns itself off on entry and back on 0.5 s later; it logs every exit the game fires.
    let mut zone = trigger("Zona", [0.; 3]);
    let enter = node(&mut zone, "event.area_enter", &[]);
    let off = node(
        &mut zone,
        "action.component",
        &[
            ("component", text("collider")),
            ("enabled", Value::Bool(false)),
        ],
    );
    let wait = node(
        &mut zone,
        "control.wait",
        &[("seconds", Value::Number(0.5))],
    );
    let on = node(
        &mut zone,
        "action.component",
        &[
            ("component", text("collider")),
            ("enabled", Value::Bool(true)),
        ],
    );
    link(&mut zone, &enter, &off);
    link(&mut zone, &off, &wait);
    link(&mut zone, &wait, &on);
    let exit = node(&mut zone, "event.area_exit", &[]);
    let left = node(&mut zone, "debug.message", &[("message", text("saiu"))]);
    link(&mut zone, &exit, &left);
    let zone_id = zone.id.clone();
    scene.entities = vec![zone, solid("Caixa", [0.; 3])];
    let report = run(
        &project,
        json!({"seconds": 1.2, "until": {"event": {"kind": "area_exit", "object": zone_id}}}),
    );
    assert!(report.stopped_at.is_none(), "{:?}", report.stopped_at);
    assert!(report.logs.is_empty(), "{:?}", report.logs);
    // Turning the collider back on is a new entry, as the game fires it.
    let events = area_events(&report);
    assert_eq!(
        events,
        vec![(EventKind::AreaEnter, "Caixa".to_owned(), "Zona".to_owned()); 3],
        "{:?}",
        report.events
    );
    assert!(
        near(report.events[1].time, 0.5 + 1. / 60.),
        "{:?}",
        report.events
    );
}

#[test]
fn a_3d_area_that_turns_off_its_collider_reports_no_exit() {
    // The 2D case above with a 3D area around a 3D character standing still.
    let mut project = course();
    let goal = project.scenes[0].entity_mut(GOAL).unwrap();
    let enter = node(goal, "event.area_enter", &[]);
    let off = node(
        goal,
        "action.component",
        &[
            ("component", text("collider")),
            ("enabled", Value::Bool(false)),
        ],
    );
    let wait = node(goal, "control.wait", &[("seconds", Value::Number(0.5))]);
    let on = node(
        goal,
        "action.component",
        &[
            ("component", text("collider")),
            ("enabled", Value::Bool(true)),
        ],
    );
    link(goal, &enter, &off);
    link(goal, &off, &wait);
    link(goal, &wait, &on);
    let exit = node(goal, "event.area_exit", &[]);
    let left = node(goal, "debug.message", &[("message", text("saiu"))]);
    link(goal, &exit, &left);
    // Another area, away from the character, keeps the detection going pair by pair while
    // Meta is off.
    let mut far = Entity::new("Longe", None);
    far.transform.position = [10., 1., 0.];
    far.physics3d = project.scenes[0].entity(GOAL).unwrap().physics3d.clone();
    project.scenes[0].entities.push(far);
    let report = run(
        &project,
        json!({"seconds": 1.2, "until": {"event": {"kind": "area_exit", "object": GOAL}}}),
    );
    assert!(report.stopped_at.is_none(), "{:?}", report.stopped_at);
    assert!(report.logs.is_empty(), "{:?}", report.logs);
    assert_eq!(
        area_events(&report),
        vec![
            (
                EventKind::AreaEnter,
                "Corredor".to_owned(),
                "Meta".to_owned()
            );
            3
        ],
        "{:?}",
        report.events
    );
    let entries: Vec<f64> = report
        .events
        .iter()
        .filter(|e| e.kind == EventKind::AreaEnter)
        .map(|e| e.time)
        .collect();
    assert!(near(entries[1], 0.5 + 1. / 60.), "{entries:?}");
}

#[test]
fn turning_off_the_3d_collider_under_a_character_drops_it() {
    let mut project = course();
    let floor = &mut project.scenes[0].entities[0];
    let start = node(floor, "event.scene_start", &[]);
    let wait = node(floor, "control.wait", &[("seconds", Value::Number(0.5))]);
    let off = node(
        floor,
        "action.component",
        &[
            ("component", text("collider")),
            ("enabled", Value::Bool(false)),
        ],
    );
    link(floor, &start, &wait);
    link(floor, &wait, &off);
    let report = run(
        &project,
        json!({"seconds": 1.5, "samples": 1,
               "watch": [{"id": BODY, "fields": ["position", "character"]}]}),
    );
    let leave = report
        .events
        .iter()
        .find(|e| e.kind == EventKind::Leave && e.object.id == BODY)
        .unwrap_or_else(|| panic!("{:?}", report.events));
    assert!(near(leave.time, 0.5), "{leave:?}");
    let last = report.watched[0].samples.last().unwrap();
    assert!(last.position.unwrap()[1] < -1., "{last:?}");
    let character = last.character.as_ref().unwrap();
    assert!(
        !character.grounded && character.support.is_none(),
        "{character:?}"
    );
    assert!(report.logs.is_empty(), "{:?}", report.logs);
}

#[test]
fn a_moving_platform_stops_and_resumes_without_launching_its_passenger() {
    let mut project = course();
    for (action, key) in [("parar", "P"), ("seguir", "O")] {
        project.input_bindings.insert(action.into(), key.into());
    }
    let mut lift = Entity::new("Plataforma", Some(Primitive::Cube));
    lift.dimensions = [4., 0.5, 4.];
    lift.transform.position = [20., 0.75, 0.];
    lift.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: lift.dimensions,
        },
        ..Default::default()
    });
    lift.platform = Some(TranslationPlatform {
        mode: PlatformMode::Velocity,
        velocity: [1., 0., 0.],
        ..Default::default()
    });
    for (action, enabled) in [("parar", false), ("seguir", true)] {
        let input = node(&mut lift, "event.input", &[("action", text(action))]);
        let switch = node(
            &mut lift,
            "action.component",
            &[
                ("component", text("platform")),
                ("enabled", Value::Bool(enabled)),
            ],
        );
        link(&mut lift, &input, &switch);
    }
    let lift_id = lift.id.clone();
    let scene = &mut project.scenes[0];
    scene.entities.push(lift);
    scene.entity_mut(BODY).unwrap().transform.position = [20., 1.02, 0.];
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    for _ in 0..60 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    // How far the platform and its passenger went in one step, pressing `action`.
    let mut step = |action: Option<&str>| {
        let x = |runtime: &Runtime| {
            (
                runtime.scene().entity(&lift_id).unwrap().transform.position[0],
                runtime.character_state(BODY).unwrap().position.x,
            )
        };
        let before = x(&runtime);
        let input = InputFrame {
            pressed: action.into_iter().map(str::to_owned).collect(),
            ..Default::default()
        };
        runtime.advance(FIXED_DT, &input);
        let after = x(&runtime);
        let state = runtime.character_state(BODY).unwrap();
        assert!(state.grounded, "{state:?}");
        assert_eq!(state.support.as_ref().unwrap().object, lift_id);
        // No launch: standing still on the platform, whatever it does.
        assert!(state.velocity.length() < 1e-3, "{state:?}");
        (after.0 - before.0, after.1 - before.1)
    };
    let close = |(platform, passenger): (f32, f32), expected: f32| {
        (platform - expected).abs() < 1e-4 && (passenger - expected).abs() < 1e-4
    };
    assert!(close(step(None), FIXED_DT));
    // Stopped in the step the logic runs, before anything moves.
    assert!(close(step(Some("parar")), 0.));
    for _ in 0..10 {
        assert!(close(step(None), 0.));
    }
    // Back on, it carries the passenger one ordinary step at once.
    let resumed = step(Some("seguir"));
    assert!(close(resumed, FIXED_DT), "{resumed:?}");
    assert!(close(step(None), FIXED_DT));
    assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);
}

#[test]
fn area_events_survive_a_scene_change_in_the_same_step() {
    let mut project = Project::new("Portal");
    let next = Scene::new("Fase 2", SceneKind::TwoD);
    let next_id = next.id.clone();
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let mut portal = trigger("Portal", [0.; 3]);
    let enter = node(&mut portal, "event.area_enter", &[]);
    let travel = node(&mut portal, "action.scene", &[("scene", text(&next_id))]);
    link(&mut portal, &enter, &travel);
    let portal_id = portal.id.clone();
    scene.entities = vec![portal, solid("Caixa", [0.; 3])];
    project.scenes.push(next);
    let report = run(
        &project,
        json!({"seconds": 1.0, "until": {"event": {"kind": "area_enter", "object": portal_id}}}),
    );
    assert_eq!(
        area_events(&report),
        [(EventKind::AreaEnter, "Caixa".into(), "Portal".into())]
    );
    assert_eq!(report.scene_changes.len(), 1);
    assert_eq!(report.final_scene, next_id);
    let stopped = report.stopped_at.unwrap();
    assert!(
        stopped.reason == "event" && near(stopped.time, 1. / 60.),
        "{stopped:?}"
    );
}

fn looks_like_id(text: &str) -> bool {
    text.split([' ', '(', ')'])
        .any(|word| word.len() == 36 && word.matches('-').count() == 4)
}

#[test]
fn spawned_copies_removed_in_the_same_step_keep_names_and_the_summary_room() {
    let mut project = Project::new("Arma");
    for (action, key) in [("gerar", "G"), ("fim", "F")] {
        project.input_bindings.insert(action.into(), key.into());
    }
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let mut floor = solid("Chão", [0.; 3]);
    floor.collider.as_mut().unwrap().size = [100., 1., 1.];
    // A bullet spawned on the floor logs the hit and removes itself in the same step.
    let mut bullet = trigger("Bala", [0., 10., 0.]);
    bullet.collider.as_mut().unwrap().size = [0.2, 0.2, 1.];
    let enter = node(&mut bullet, "event.area_enter", &[]);
    let hit = node(
        &mut bullet,
        "debug.message",
        &[("message", text("acertou"))],
    );
    let remove = node(&mut bullet, "action.remove", &[]);
    link(&mut bullet, &enter, &hit);
    link(&mut bullet, &hit, &remove);
    // A trail copy removes itself on the next step, through per-step logic; the template
    // turns its own logic off at the start.
    let mut trail = Entity::new("Rastro", None);
    trail.transform.position = [0., 20., 0.];
    let start = node(&mut trail, "event.scene_start", &[]);
    let off = node(
        &mut trail,
        "action.component",
        &[
            ("component", text("behavior")),
            ("enabled", Value::Bool(false)),
        ],
    );
    link(&mut trail, &start, &off);
    let step = node(&mut trail, "event.step", &[]);
    let gone = node(&mut trail, "action.remove", &[]);
    link(&mut trail, &step, &gone);
    let mut gun = Entity::new("Arma", None);
    let fire = node(
        &mut gun,
        "event.input",
        &[("action", text("gerar")), ("mode", text("held"))],
    );
    let shot = node(
        &mut gun,
        "action.spawn",
        &[
            ("target", Value::Object(Some(bullet.id.clone()))),
            ("y", Value::Number(-10.)),
        ],
    );
    let mark = node(
        &mut gun,
        "action.spawn",
        &[("target", Value::Object(Some(trail.id.clone())))],
    );
    link(&mut gun, &fire, &shot);
    link(&mut gun, &shot, &mark);
    let mut end = Entity::new("Fim", None);
    let done = node(&mut end, "event.input", &[("action", text("fim"))]);
    let say = node(&mut end, "debug.message", &[("message", text("fim"))]);
    link(&mut end, &done, &say);
    scene.entities = vec![floor, bullet, trail, gun, end];
    let inputs = json!([
        {"type": "press", "at": 0.0, "action": "gerar"},
        {"type": "release", "at": 3.5, "action": "gerar"},
        {"type": "press", "at": 4.0, "action": "fim"},
    ]);
    let report = run(&project, json!({"seconds": 4.5, "inputs": inputs}));
    let lines = summary(&report);
    assert!(
        !lines.iter().any(|(text, _)| looks_like_id(text)),
        "{lines:?}"
    );
    let hits = lines
        .iter()
        .find(|(text, _)| text == "Bala (cópia) · Ao entrar na área")
        .unwrap_or_else(|| panic!("{lines:?}"));
    assert!(hits.1 > 200, "{hits:?}");
    assert!(
        lines.contains(&("Fim · Mensagem de diagnóstico".into(), 1)),
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|(text, _)| text.starts_with("Rastro (cópia)")),
        "per-step logic of copies is left out too: {lines:?}"
    );
    assert!(report.truncated.is_empty(), "{:?}", report.truncated);
    let hit = report.logs.iter().find(|l| l.text == "acertou").unwrap();
    assert_eq!(hit.object.as_ref().unwrap().name, "Bala (cópia)");
    assert_eq!(hit.node.as_ref().unwrap().operation, "debug.message");
    assert!(
        report.events.iter().all(|e| !looks_like_id(&e.object.name)
            && e.other.as_ref().is_none_or(|o| !looks_like_id(&o.name))),
        "{:?}",
        report.events.first()
    );

    let all = run(
        &project,
        json!({"seconds": 1.0, "actions": "all", "inputs": inputs}),
    );
    let Some(Actions::All(actions)) = &all.actions else {
        panic!("{:?}", all.actions);
    };
    assert!(actions.iter().all(|a| !looks_like_id(&a.text)));
}

#[test]
fn a_scene_change_started_every_step_still_shows_in_the_summary() {
    let mut project = Project::new("Menu");
    project.scenes[0].kind = SceneKind::TwoD;
    let level = Scene::new("Fase", SceneKind::TwoD);
    let level_id = level.id.clone();
    let mut menu = Entity::new("Menu", None);
    let step = node(&mut menu, "event.step", &[]);
    let wait = node(&mut menu, "control.wait", &[("seconds", Value::Number(1.))]);
    let open = node(&mut menu, "action.scene", &[("scene", text(&level_id))]);
    link(&mut menu, &step, &wait);
    link(&mut menu, &wait, &open);
    project.scenes[0].entities.push(menu);
    project.scenes.push(level);
    let report = run(&project, json!({"seconds": 2.0}));
    assert_eq!(report.scene_changes.len(), 1);
    let lines = summary(&report);
    assert_eq!(lines, [("Menu · Mudar de cena".to_owned(), 1)]);
    let all = run(&project, json!({"seconds": 2.0, "actions": "all"}));
    let Some(Actions::All(actions)) = &all.actions else {
        panic!("{:?}", all.actions);
    };
    assert_eq!(actions.len(), 1);
    assert!(
        actions[0].text == "Menu · Mudar de cena" && near(actions[0].time, 1. + 1. / 60.),
        "{actions:?}"
    );
    // Listing the operation itself still leaves it out.
    let hidden = run(
        &project,
        json!({"seconds": 2.0, "actions_filter": {"exclude_operations": ["action.scene"]}}),
    );
    assert!(
        summary(&hidden)
            .iter()
            .all(|(text, _)| !text.contains("Mudar de cena"))
    );
}

#[test]
fn until_compares_attributes_with_values_of_their_type() {
    let mut project = Project::new("Mira");
    project.scenes[0].kind = SceneKind::TwoD;
    let enemy = Entity::new("Inimigo", None);
    let mut player = Entity::new("Jogador", None);
    player.attributes.insert("alvo".into(), Value::Object(None));
    player.attributes.insert("vida".into(), Value::Number(3.));
    let start = node(&mut player, "event.scene_start", &[]);
    let wait = node(
        &mut player,
        "control.wait",
        &[("seconds", Value::Number(0.5))],
    );
    let aim = node(
        &mut player,
        "attribute.set",
        &[
            ("attribute", text("alvo")),
            ("value", Value::Object(Some(enemy.id.clone()))),
        ],
    );
    link(&mut player, &start, &wait);
    link(&mut player, &wait, &aim);
    let (enemy_id, player_id) = (enemy.id.clone(), player.id.clone());
    project.scenes[0].entities = vec![enemy, player];
    // An object attribute compared with the object's ID.
    let report = run(
        &project,
        json!({"seconds": 2.0, "until": {"attribute":
            {"entity": player_id, "name": "alvo", "op": "==", "value": enemy_id}}}),
    );
    let stopped = report.stopped_at.expect("the reference matches the ID");
    assert!(
        stopped.reason == "attribute" && near(stopped.time, 0.5),
        "{stopped:?}"
    );
    assert!(
        stopped.detail.contains("Jogador.alvo =="),
        "{}",
        stopped.detail
    );
    // A value of another type would never match, so it is refused.
    for (value, expected) in [
        (
            json!({"entity": player_id, "name": "vida", "op": "==", "value": "0"}),
            "o atributo vida de Jogador é número, mas o valor informado é texto",
        ),
        (
            json!({"entity": player_id, "name": "alvo", "op": "!=", "value": true}),
            "o atributo alvo de Jogador é objeto",
        ),
        (
            json!({"entity": player_id, "name": "alvo", "op": "==", "value": "nada"}),
            "until: objeto nada não existe",
        ),
    ] {
        let error = headless::simulate(
            &project,
            &request(json!({"seconds": 1.0, "until": {"attribute": value}})),
        )
        .unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

/// A health bar anchored bottom right that logic lowers after 0.2 s, and a pause button
/// hidden through its parent.
fn hud() -> Project {
    let mut project = Project::new("Interface");
    let mut player = Entity::new("Jogador", None);
    player.attributes.insert("vida".into(), Value::Number(100.));
    let start = node(&mut player, "event.scene_start", &[]);
    let wait = node(
        &mut player,
        "control.wait",
        &[("seconds", Value::Number(0.2))],
    );
    let hurt = node(
        &mut player,
        "attribute.set",
        &[("attribute", text("vida")), ("value", Value::Number(40.))],
    );
    link(&mut player, &start, &wait);
    link(&mut player, &wait, &hurt);
    let mut bar = Entity::new("Vida", None);
    bar.ui = Some(UiElement {
        kind: UiKind::Bar,
        text: "Vida {value}".into(),
        anchor: UiAnchor::BottomRight,
        binding_object: Some(player.id.clone()),
        binding_attribute: "vida".into(),
        max_value: 100.,
        ..Default::default()
    });
    let mut pause = Entity::new("Pausa", None);
    pause.visible = false;
    let mut resume = Entity::new("Continuar", None);
    resume.parent = Some(pause.id.clone());
    resume.ui = Some(UiElement {
        kind: UiKind::Button,
        text: "Continuar".into(),
        ..Default::default()
    });
    project.scenes[0]
        .entities
        .extend([player, bar, pause, resume]);
    project
}

#[test]
fn the_report_lays_out_the_game_interface_at_the_end() {
    let project = hud();
    let report = run(&project, json!({"seconds": 0.5}));
    let ui = serde_json::to_value(&report.ui).unwrap();
    assert_eq!(ui.as_array().unwrap().len(), 2, "{ui}");
    assert_eq!(ui[0]["name"], "Vida");
    assert_eq!(ui[0]["kind"], "Bar");
    assert_eq!(ui[0]["text"], "Vida 40", "the value the logic set");
    assert_eq!(ui[0]["fraction"], 0.4);
    assert_eq!(
        ui[0]["rect"],
        json!([756., 480., 180., 36.]),
        "960x540 view"
    );
    assert_eq!(ui[0]["visible"], true);
    assert_eq!(ui[1]["name"], "Continuar");
    assert_eq!(ui[1]["visible"], false);
    assert_eq!(ui[1]["clickable"], false);
    assert!(ui[1].get("fraction").is_none());

    let early = run(&project, json!({"seconds": 0.1}));
    assert_eq!(early.ui[0].text.as_deref(), Some("Vida 100"));
    assert_eq!(early.ui[0].fraction, Some(1.));
    let wide = run(&project, json!({"seconds": 0.1, "view_size": [1920, 1080]}));
    assert_eq!(wide.ui[0].rect, [1716., 1020., 180., 36.]);
    let without = run(&project, json!({"seconds": 0.1, "ui": false}));
    assert!(without.ui.is_empty());
    assert!(serde_json::to_value(&without).unwrap().get("ui").is_none());
    let error = headless::simulate(
        &project,
        &request(json!({"seconds": 0.1, "view_size": [8, 540]})),
    )
    .unwrap_err();
    assert!(error.contains("16 e 4096"), "{error}");
}

#[test]
fn the_interface_reported_is_the_final_scene_one() {
    let (mut project, level) = menu_and_level();
    let mut title = Entity::new("Título", None);
    title.ui = Some(UiElement {
        text: "Menu principal".into(),
        ..Default::default()
    });
    project.scenes[0].entities.push(title);
    let menu = run(&project, json!({"seconds": 0.5}));
    assert_eq!(menu.ui[0].text.as_deref(), Some("Menu principal"));
    let played = run(&project, json!({"seconds": 1.5}));
    assert_eq!(played.final_scene, level);
    assert!(played.ui.is_empty(), "the level has no interface");
}
