//! The headless simulation used by agents: inputs across scene changes, the readable report,
//! sounds, music and volumes, movement and camera detail, setup, spawn and stop conditions on
//! the copy, the game interface and the game variables at the end.
use oxy_core::{
    animation::Clip,
    character::{CameraRig, CharacterConfig},
    command,
    document::*,
    graph::{Edge, Node},
    headless::{self, Actions, EventKind, SimulationReport, SimulationRequest},
    physics3d::{Collider3d, CollisionShape},
    random::MAX_SEED,
    runtime::{FIXED_DT, InputFrame, MAX_STEPS, Runtime, RuntimeOptions, StateDifference},
    surface::{PlatformMode, TranslationPlatform},
    variables,
};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};

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

/// An audio asset named `name` with a fixed ID, so reports can be compared.
fn audio(project: &mut Project, name: &str) -> Id {
    let id = format!(
        "00000000-0000-4000-8000-{:012x}",
        project.assets.len() + 0xa0
    );
    project.assets.push(Asset {
        id: id.clone(),
        name: name.into(),
        path: format!("assets/som-{}.wav", project.assets.len()),
        kind: AssetKind::Audio,
        model: None,
    });
    id
}
/// A Tocar em loop of `asset` on `channel`.
fn looping(entity: &mut Entity, asset: &str, channel: &str, volume: f64, fade: f64) -> Id {
    node(
        entity,
        "action.sound_loop",
        &[
            ("asset", text(asset)),
            ("channel", text(channel)),
            ("volume", Value::Number(volume)),
            ("fade", Value::Number(fade)),
        ],
    )
}
/// Logic `entity` runs on input `action`: `nodes` in a row.
fn on_input(entity: &mut Entity, action: &str, nodes: &[Id]) {
    let event = node(entity, "event.input", &[("action", text(action))]);
    let mut previous = event;
    for next in nodes {
        link(entity, &previous, next);
        previous = next.clone();
    }
}
/// The sound entries of a report as JSON, without their times.
fn sound_entries(report: &SimulationReport) -> Vec<serde_json::Value> {
    report
        .sounds
        .iter()
        .map(|sound| {
            let mut entry = serde_json::to_value(sound).unwrap();
            entry.as_object_mut().unwrap().remove("time");
            entry
        })
        .collect()
}

/// A level that plays Trilha as its music (0.8, fading in over 1 s) and Vento as an effect
/// loop (0.5) as it starts, turns Trilha down to 0.4 on "baixar", restarts itself on
/// "reiniciar" and opens the boss scene on "seguir", whose music Chefe takes over in 2 s.
fn soundtrack() -> (Project, Id) {
    let mut project = Project::new("Trilha sonora");
    for (action, key) in [("reiniciar", "R"), ("seguir", "S"), ("baixar", "B")] {
        project.input_bindings.insert(action.into(), key.into());
    }
    let track = audio(&mut project, "Trilha");
    let boss = audio(&mut project, "Chefe");
    let wind = audio(&mut project, "Vento");
    let mut final_scene = Scene::new("Chefão", SceneKind::TwoD);
    let final_id = final_scene.id.clone();
    let mut arena = Entity::new("Arena", None);
    let start = node(&mut arena, "event.scene_start", &[]);
    let music = looping(&mut arena, &boss, "music", 1., 2.);
    link(&mut arena, &start, &music);
    final_scene.entities.push(arena);
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let mut control = Entity::new("Controle", None);
    let start = node(&mut control, "event.scene_start", &[]);
    let music = looping(&mut control, &track, "music", 0.8, 1.);
    let wind = looping(&mut control, &wind, "effects", 0.5, 0.);
    link(&mut control, &start, &music);
    link(&mut control, &music, &wind);
    let restart = node(&mut control, "action.scene", &[]);
    let current = node(&mut control, "scene.current", &[]);
    wire(&mut control, &current, "scene", &restart, "scene");
    on_input(&mut control, "reiniciar", &[restart]);
    let next = node(&mut control, "action.scene", &[("scene", text(&final_id))]);
    on_input(&mut control, "seguir", &[next]);
    let quieter = looping(&mut control, &track, "music", 0.4, 0.);
    on_input(&mut control, "baixar", &[quieter]);
    scene.entities.push(control);
    project.scenes.push(final_scene);
    validate_project(&project).unwrap();
    (project, final_id)
}

#[test]
fn the_music_goes_on_through_restarts_and_another_track_crossfades() {
    let (project, boss_scene) = soundtrack();
    let [track, boss, wind] = [0, 1, 2].map(|i| project.assets[i].id.clone());
    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    let press = |runtime: &mut Runtime, action: &str| {
        let frame = InputFrame {
            pressed: [action.to_owned()].into(),
            ..Default::default()
        };
        runtime.advance(FIXED_DT, &frame);
    };
    let music = runtime.audio().music.clone().unwrap();
    assert_eq!(
        (music.asset.as_str(), music.volume, music.fade),
        (track.as_str(), 0.8, 1.)
    );
    let gust = runtime.audio().loops[&wind].clone();
    // A restart asks for both again: the music goes on as it was, the effect loop starts over.
    press(&mut runtime, "reiniciar");
    assert_eq!(runtime.scene_starts.len(), 1);
    assert_eq!(runtime.audio().music.as_ref(), Some(&music));
    let again = &runtime.audio().loops[&wind];
    assert_ne!(again.generation, gust.generation);
    assert!(runtime.audio().ending.is_empty());
    // Turned down, then restarted, it goes on too, back at the volume the scene asks.
    press(&mut runtime, "baixar");
    let quiet = runtime.audio().music.as_ref().unwrap();
    assert_eq!((quiet.volume, quiet.generation), (0.4, music.generation));
    press(&mut runtime, "reiniciar");
    assert_eq!(runtime.audio().music.as_ref(), Some(&music));
    // Another scene's music takes over, the old one fading out over the new one's 2 s; the
    // effect loop ended with its scene.
    press(&mut runtime, "seguir");
    let audio = runtime.audio();
    assert_eq!(audio.music.as_ref().unwrap().asset, boss);
    assert!(audio.loops.is_empty());
    assert_eq!(audio.ending.len(), 1);
    assert_eq!(
        (
            &audio.ending[0].asset,
            audio.ending[0].generation,
            audio.ending[0].fade
        ),
        (&track, music.generation, 2.)
    );
    // Once faded out, it is forgotten.
    for _ in 0..121 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert!(runtime.audio().ending.is_empty());
    assert_eq!(runtime.scene_id(), boss_scene);

    // The report lists what changed: the music turned down and, at the restart, back up
    // without starting over, and the wind started over.
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": [
            {"type": "press", "at": 0.25, "action": "baixar"},
            {"type": "press", "at": 0.5, "action": "reiniciar"},
            {"type": "press", "at": 1.0, "action": "seguir"},
        ]}),
    );
    assert_eq!(
        sound_entries(&report),
        [
            json!({"text": "Trilha", "kind": "loop", "started": true, "channel": "music", "volume": 0.8, "fade": 1.0}),
            json!({"text": "Vento", "kind": "loop", "started": true, "channel": "effects", "volume": 0.5}),
            json!({"text": "Trilha", "kind": "loop", "channel": "music", "volume": 0.4}),
            json!({"text": "Trilha", "kind": "loop", "channel": "music", "volume": 0.8}),
            json!({"text": "Vento", "kind": "loop", "started": true, "channel": "effects", "volume": 0.5}),
            json!({"text": "Chefe", "kind": "loop", "started": true, "channel": "music", "volume": 1.0, "fade": 2.0}),
        ]
    );
    let times: Vec<_> = report.sounds.iter().map(|s| s.time).collect();
    assert!(
        near(times[1], 0.)
            && near(times[2], 0.25)
            && near(times[3], 0.5)
            && near(times[4], 0.5)
            && near(times[5], 1.0),
        "{times:?}"
    );
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(
        json["audio"],
        json!({
            "music": {"asset": boss, "name": "Chefe", "volume": 1.0},
            "loops": [],
            "volumes": {"master": 1.0, "music": 1.0, "effects": 1.0},
        })
    );
    // Before the change, the wind loops too.
    let before = run(&project, json!({"seconds": 0.5}));
    let loops: Vec<_> = before
        .audio
        .as_ref()
        .unwrap()
        .loops
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(loops, ["Vento"]);

    // Until a sound starts: at the start, or when the boss music takes over.
    let inputs = json!([{"type": "press", "at": 1.0, "action": "seguir"}]);
    let stop = run(
        &project,
        json!({"seconds": 2.0, "inputs": inputs, "until": {"sound": "vento"}}),
    );
    let stopped = stop.stopped_at.unwrap();
    assert_eq!(
        (stopped.reason, stopped.detail.as_str(), stopped.time),
        ("sound", "O som Vento começou a tocar", 0.)
    );
    let stop = run(
        &project,
        json!({"seconds": 2.0, "inputs": inputs, "until": {"sound": boss}}),
    );
    let stopped = stop.stopped_at.unwrap();
    assert!(
        stopped.reason == "sound" && near(stopped.time, 1.0),
        "{stopped:?}"
    );
    let error = headless::simulate(
        &project,
        &request(json!({"seconds": 1.0, "until": {"sound": "Sirene"}})),
    )
    .unwrap_err();
    assert_eq!(error, "until: o som \"Sirene\" não existe no projeto");
}

#[test]
fn stops_pick_the_channel_and_volumes_last_the_whole_run() {
    let mut project = Project::new("Mixagem");
    for (action, key) in [
        ("chuva", "C"),
        ("efeitos", "E"),
        ("musica", "M"),
        ("seguir", "S"),
    ] {
        project.input_bindings.insert(action.into(), key.into());
    }
    let track = audio(&mut project, "Trilha");
    let wind = audio(&mut project, "Vento");
    let rain = audio(&mut project, "Chuva");
    let mut next = Scene::new("Opções", SceneKind::TwoD);
    let next_id = next.id.clone();
    // The next scene reads the volumes into attributes.
    let mut panel = Entity::new("Painel", None);
    let start = node(&mut panel, "event.scene_start", &[]);
    let read = node(&mut panel, "audio.read", &[]);
    let mut previous = start;
    for (attribute, output) in [
        ("Geral", "master"),
        ("Música", "music"),
        ("Efeitos", "effects"),
    ] {
        panel
            .attributes
            .insert(attribute.into(), Value::Number(-1.));
        let set = node(
            &mut panel,
            "attribute.set",
            &[("attribute", text(attribute))],
        );
        wire(&mut panel, &read, output, &set, "value");
        link(&mut panel, &previous, &set);
        previous = set;
    }
    let panel_id = panel.id.clone();
    next.entities.push(panel);
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let mut control = Entity::new("Controle", None);
    let start = node(&mut control, "event.scene_start", &[]);
    let music = looping(&mut control, &track, "music", 1., 0.);
    let wind_loop = looping(&mut control, &wind, "effects", 1., 0.);
    let rain_loop = looping(&mut control, &rain, "effects", 1., 0.);
    let music_volume = node(
        &mut control,
        "action.volume",
        &[("channel", text("music")), ("volume", Value::Number(0.25))],
    );
    let master = node(
        &mut control,
        "action.volume",
        &[("volume", Value::Number(0.5))],
    );
    // A volume from the input, out of range, is kept within 0 and 1.
    let effects = node(
        &mut control,
        "action.volume",
        &[("channel", text("effects"))],
    );
    let loud = node(
        &mut control,
        "value.number",
        &[("value", Value::Number(1.5))],
    );
    wire(&mut control, &loud, "value", &effects, "volume");
    for (from, to) in [
        (&start, &music),
        (&music, &wind_loop),
        (&wind_loop, &rain_loop),
        (&rain_loop, &music_volume),
        (&music_volume, &master),
        (&master, &effects),
    ] {
        link(&mut control, from, to);
    }
    let stop_rain = node(
        &mut control,
        "action.sound_stop",
        &[("asset", text(&rain)), ("fade", Value::Number(0.5))],
    );
    on_input(&mut control, "chuva", &[stop_rain]);
    let stop_effects = node(
        &mut control,
        "action.sound_stop",
        &[("channel", text("effects"))],
    );
    on_input(&mut control, "efeitos", &[stop_effects]);
    let stop_music = node(
        &mut control,
        "action.sound_stop",
        &[("channel", text("music"))],
    );
    on_input(&mut control, "musica", &[stop_music]);
    let open = node(&mut control, "action.scene", &[("scene", text(&next_id))]);
    on_input(&mut control, "seguir", &[open]);
    scene.entities.push(control);
    project.scenes.push(next);
    validate_project(&project).unwrap();

    let mut runtime = Runtime::new(&project, &project.start_scene).unwrap();
    let press = |runtime: &mut Runtime, action: &str| {
        let frame = InputFrame {
            pressed: [action.to_owned()].into(),
            ..Default::default()
        };
        runtime.advance(FIXED_DT, &frame);
    };
    let settings = runtime.audio().settings;
    assert_eq!(
        (settings.master, settings.music, settings.effects),
        (0.5, 0.25, 1.)
    );
    assert_eq!(runtime.audio().loops.len(), 2);
    // Chuva, on every channel; then every effect loop, the music playing on.
    press(&mut runtime, "chuva");
    assert_eq!(runtime.audio().loops.keys().collect::<Vec<_>>(), [&wind]);
    assert_eq!(runtime.audio().ending.len(), 1);
    press(&mut runtime, "efeitos");
    assert!(runtime.audio().loops.is_empty());
    assert!(runtime.audio().music.is_some());
    press(&mut runtime, "musica");
    assert!(runtime.audio().music.is_none());
    // The volumes go on in the next scene, where Ler volumes reads them.
    press(&mut runtime, "seguir");
    press(&mut runtime, "nada");
    assert_eq!(runtime.audio().settings, settings);
    let panel = runtime.scene().entity(&panel_id).unwrap();
    for (attribute, value) in [("Geral", 0.5), ("Música", 0.25), ("Efeitos", 1.)] {
        assert_eq!(
            panel.attributes[attribute],
            Value::Number(value),
            "{attribute}"
        );
    }
    assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);

    let report = run(
        &project,
        json!({"seconds": 1.0, "inputs": [
            {"type": "press", "at": 0.2, "action": "chuva"},
            {"type": "press", "at": 0.4, "action": "efeitos"},
            {"type": "press", "at": 0.5, "action": "efeitos"},
            {"type": "press", "at": 0.6, "action": "seguir"},
        ]}),
    );
    // Stopping what does not play is not listed, and neither is a volume already so.
    assert_eq!(
        sound_entries(&report)[3..],
        [
            json!({"text": "Música", "kind": "volume", "channel": "music", "volume": 0.25}),
            json!({"text": "Geral", "kind": "volume", "channel": "master", "volume": 0.5}),
            json!({"text": "Chuva", "kind": "stop", "channel": "all", "fade": 0.5}),
            json!({"text": "Todos os sons do canal", "kind": "stop", "channel": "effects"}),
        ]
    );
    let audio = report.audio.unwrap();
    assert_eq!(audio.music.unwrap().name, "Trilha");
    assert!(audio.loops.is_empty());
    assert_eq!(audio.volumes, settings);
}

#[test]
fn effect_loops_stop_at_32_with_an_error() {
    let mut project = Project::new("Orquestra");
    let scene = &mut project.scenes[0];
    scene.kind = SceneKind::TwoD;
    let assets: Vec<Id> = (0..33)
        .map(|i| audio(&mut project, &format!("Som {i}")))
        .collect();
    let mut band = Entity::new("Banda", None);
    let mut previous = node(&mut band, "event.scene_start", &[]);
    for asset in &assets {
        let next = looping(&mut band, asset, "effects", 1., 0.);
        link(&mut band, &previous, &next);
        previous = next;
    }
    project.scenes[0].entities.push(band);
    let runtime = Runtime::new(&project, &project.start_scene).unwrap();
    assert_eq!(runtime.audio().loops.len(), 32);
    assert!(!runtime.audio().loops.contains_key(&assets[32]));
    assert!(
        runtime
            .logs
            .iter()
            .any(|l| l.contains("Já há 32 sons em loop nos efeitos")),
        "{:?}",
        runtime.logs
    );
}

#[test]
fn a_scene_left_within_the_step_leaves_its_effect_loops_out_of_the_report() {
    // On "jogar" the menu loops a breeze and opens a passage, which loops the wind, starts the
    // music, turns the volume down and opens the level, all within one step: the breeze and
    // the wind ended unheard.
    let mut project = Project::new("Passagem");
    project.input_bindings.insert("jogar".into(), "J".into());
    let track = audio(&mut project, "Trilha");
    let wind = audio(&mut project, "Vento");
    let breeze = audio(&mut project, "Brisa");
    let boss = audio(&mut project, "Chefe");
    let mut level = Scene::new("Fase", SceneKind::TwoD);
    let level_id = level.id.clone();
    level.entities.push(Entity::new("Chão", None));
    let mut passage = Scene::new("Passagem", SceneKind::TwoD);
    let passage_id = passage.id.clone();
    let mut door = Entity::new("Porta", None);
    let start = node(&mut door, "event.scene_start", &[]);
    let gust = looping(&mut door, &wind, "effects", 1., 0.);
    let music = looping(&mut door, &track, "music", 1., 0.);
    let quiet = node(
        &mut door,
        "action.volume",
        &[("volume", Value::Number(0.5))],
    );
    let open = node(&mut door, "action.scene", &[("scene", text(&level_id))]);
    for (from, to) in [
        (&start, &gust),
        (&gust, &music),
        (&music, &quiet),
        (&quiet, &open),
    ] {
        link(&mut door, from, to);
    }
    passage.entities.push(door);
    let mut menu = Entity::new("Menu", None);
    let blow = looping(&mut menu, &breeze, "effects", 1., 0.);
    let go = node(&mut menu, "action.scene", &[("scene", text(&passage_id))]);
    on_input(&mut menu, "jogar", &[blow, go]);
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0].entities.push(menu);
    project.scenes.extend([passage, level]);
    validate_project(&project).unwrap();

    let report = run(
        &project,
        json!({"seconds": 1.0, "inputs": [{"type": "press", "at": 0.5, "action": "jogar"}]}),
    );
    assert_eq!(report.scene_changes.len(), 2);
    assert_eq!(
        sound_entries(&report),
        [
            json!({"text": "Trilha", "kind": "loop", "started": true, "channel": "music", "volume": 1.0}),
            json!({"text": "Geral", "kind": "volume", "channel": "master", "volume": 0.5}),
        ]
    );
    let audio = report.audio.unwrap();
    assert_eq!(audio.music.unwrap().name, "Trilha");
    assert!(audio.loops.is_empty());
    assert_eq!(audio.volumes.master, 0.5);
    let jump = json!([{"type": "press", "at": 0.5, "action": "jogar"}]);
    for unheard in ["Brisa", "Vento"] {
        let report = run(
            &project,
            json!({"seconds": 1.0, "inputs": jump, "until": {"sound": unheard}}),
        );
        assert_eq!(report.stopped_at, None, "{unheard}");
    }
    // A level with music of its own replaces the passage's within the step: only the level's
    // is heard.
    let level = project
        .scenes
        .iter_mut()
        .find(|s| s.id == level_id)
        .unwrap();
    let mut speaker = Entity::new("Alto-falante", None);
    let start = node(&mut speaker, "event.scene_start", &[]);
    let theme = looping(&mut speaker, &boss, "music", 1., 0.);
    link(&mut speaker, &start, &theme);
    level.entities.push(speaker);
    let report = run(
        &project,
        json!({"seconds": 1.0, "inputs": jump, "until": {"sound": "Trilha"}}),
    );
    assert_eq!(report.stopped_at, None);
    assert_eq!(
        sound_entries(&report),
        [
            json!({"text": "Geral", "kind": "volume", "channel": "master", "volume": 0.5}),
            json!({"text": "Chefe", "kind": "loop", "started": true, "channel": "music", "volume": 1.0}),
        ]
    );
    assert_eq!(report.audio.unwrap().music.unwrap().name, "Chefe");
    // A one-shot keeps the shape it had.
    let beeps = run(
        &beep_then_restart(false),
        json!({"seconds": 1.0, "inputs": [{"type": "press", "at": 0.5, "action": "reiniciar"}]}),
    );
    assert_eq!(sound_entries(&beeps), [json!({"text": "Bip"})]);
    assert!(beeps.audio.is_none());
    assert!(serde_json::to_value(&beeps).unwrap().get("audio").is_none());
}

/// `beep_then_restart(false)` with the restart wired: Cena atual gives Mudar de cena the scene
/// to open, and the node names none.
fn wired_restart() -> Project {
    let mut project = beep_then_restart(false);
    let control = &mut project.scenes[0].entities[0];
    let restart = control.graph.nodes.last_mut().unwrap();
    restart.params.insert("scene".into(), text(""));
    let restart = restart.id.clone();
    let current = node(control, "scene.current", &[]);
    wire(control, &current, "scene", &restart, "scene");
    project
}

#[test]
fn a_restart_wired_to_the_current_scene_reloads_it() {
    let project = wired_restart();
    let scene = project.start_scene.clone();
    let report = run(
        &project,
        json!({"seconds": 2.0, "inputs": [
            {"type": "press", "at": 0.5, "action": "reiniciar"},
            {"type": "release", "at": 0.6, "action": "reiniciar"},
            {"type": "press", "at": 1.5, "action": "reiniciar"},
        ]}),
    );
    let changes: Vec<_> = report
        .scene_changes
        .iter()
        .map(|c| (c.scene.as_str(), c.reload))
        .collect();
    assert_eq!(changes, [(scene.as_str(), true), (scene.as_str(), true)]);
    assert!(
        near(report.scene_changes[0].time, 0.5) && near(report.scene_changes[1].time, 1.5),
        "{:?}",
        report.scene_changes
    );
    // The beep right before each restart is still heard, and each change has its action.
    assert_eq!(report.sounds.len(), 2, "{:?}", report.sounds);
    assert!(report.logs.is_empty(), "{:?}", report.logs);
    assert!(
        summary(&report).contains(&("Controle · Mudar de cena".into(), 2)),
        "{:?}",
        summary(&report)
    );
}

#[test]
fn a_wired_restart_copied_or_placed_in_another_scene_restarts_that_scene() {
    let mut project = wired_restart();
    let first = project.start_scene.clone();
    let control = project.scenes[0].entities[0].id.clone();
    let report = command::apply_and_report(
        &mut project,
        &first,
        &command::parse(&json!([
            {"op": "create_scene", "name": "Segunda", "kind": "2d"},
            {"op": "create_scene", "name": "Terceira", "kind": "2d"},
            {"op": "copy_entities", "ids": [control], "to_scene": "$0"},
            {"op": "save_model", "entity": control, "name": "Reinício"},
            {"op": "place_model", "model": "$3", "scene": "$1"},
        ]))
        .unwrap(),
    )
    .unwrap();
    // Nothing copied still opens the scene it came from.
    assert!(report.get("warnings").is_none(), "{report}");
    let created = |index: &str| {
        report["created"][index][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    for scene in [first, created("0"), created("1")] {
        let report = run(
            &project,
            json!({"scene": scene, "seconds": 1.0,
                   "inputs": [{"type": "press", "at": 0.5, "action": "reiniciar"}]}),
        );
        let changes: Vec<_> = report
            .scene_changes
            .iter()
            .map(|c| (c.scene.as_str(), c.reload))
            .collect();
        assert_eq!(changes, [(scene.as_str(), true)]);
        assert_eq!(report.final_scene, scene);
    }
}

/// Two scenes whose "Saída" opens, on "seguir", the scene its Text attribute "Próxima" names:
/// the first scene's leads to the second, and the second's back to the first.
fn next_scene_by_attribute() -> (Project, Id, Id) {
    let mut project = Project::new("Cenas em sequência");
    project.input_bindings.insert("seguir".into(), "N".into());
    project.scenes[0].kind = SceneKind::TwoD;
    let first = project.scenes[0].id.clone();
    let second_scene = Scene::new("Segunda", SceneKind::TwoD);
    let second = second_scene.id.clone();
    project.scenes.push(second_scene);
    for (index, next) in [(0, &second), (1, &first)] {
        let mut exit = Entity::new("Saída", None);
        exit.attributes.insert("Próxima".into(), text(next));
        let go = node(&mut exit, "event.input", &[("action", text("seguir"))]);
        let read = node(
            &mut exit,
            "attribute.get",
            &[("attribute", text("Próxima"))],
        );
        let change = node(&mut exit, "action.scene", &[]);
        link(&mut exit, &go, &change);
        wire(&mut exit, &read, "value", &change, "scene");
        project.scenes[index].entities.push(exit);
    }
    (project, first, second)
}

#[test]
fn a_text_attribute_chooses_the_next_scene() {
    let (project, first, second) = next_scene_by_attribute();
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": [
            {"type": "press", "at": 0.5, "action": "seguir"},
            {"type": "release", "at": 0.6, "action": "seguir"},
            {"type": "press", "at": 1.0, "action": "seguir"},
        ]}),
    );
    let changes: Vec<_> = report
        .scene_changes
        .iter()
        .map(|c| (c.scene.as_str(), c.name.as_str(), c.reload))
        .collect();
    assert_eq!(
        changes,
        [
            (second.as_str(), "Segunda", false),
            (first.as_str(), project.scenes[0].name.as_str(), false)
        ]
    );
    assert!(report.logs.is_empty(), "{:?}", report.logs);
    assert_eq!(report.final_scene, first);
}

#[test]
fn a_scene_the_input_does_not_name_stops_the_node_with_a_clear_error() {
    let (project, first, _) = next_scene_by_attribute();
    let exit = project.scenes[0].entities[0].id.clone();
    let press =
        json!({"seconds": 1.0, "inputs": [{"type": "press", "at": 0.5, "action": "seguir"}]});
    for (value, expected) in [
        (
            text(""),
            "Cena de destino vazia: a entrada Cena recebeu um texto vazio",
        ),
        (
            text("cena-inexistente"),
            "Cena de destino não existe: a entrada Cena recebeu \"cena-inexistente\"",
        ),
        (
            text("Segunda"),
            "a entrada Cena recebeu \"Segunda\", o nome de uma cena; a entrada espera o \
             identificador (para reiniciar, a saída Cena de Cena atual; para outra cena, \
             Referência de cena)",
        ),
        (
            Value::Number(2.),
            "A entrada Cena precisa receber o identificador de uma cena, em texto",
        ),
    ] {
        let mut project = project.clone();
        let saida = project.scenes[0].entity_mut(&exit).unwrap();
        saida.attributes.insert("Próxima".into(), value);
        let report = run(&project, press.clone());
        assert!(
            report.scene_changes.is_empty(),
            "{:?}",
            report.scene_changes
        );
        assert_eq!(report.final_scene, first);
        let [log] = report.logs.as_slice() else {
            panic!("{:?}", report.logs);
        };
        assert!(log.text.contains(expected), "{}", log.text);
        assert_eq!(log.object.as_ref().unwrap().name, "Saída");
        assert_eq!(log.node.as_ref().unwrap().operation, "action.scene");
    }
    // Unconnected, an empty parameter stays an error, as before the input existed.
    let mut project = project;
    let saida = project.scenes[0].entity_mut(&exit).unwrap();
    saida.graph.edges.retain(|edge| edge.to_port != "scene");
    let report = run(&project, press);
    assert!(
        report.scene_changes.is_empty(),
        "{:?}",
        report.scene_changes
    );
    assert!(
        report.logs.len() == 1
            && report.logs[0]
                .text
                .starts_with("Cena de destino não existe."),
        "{:?}",
        report.logs
    );
}

/// Two scenes whose "Saída" opens, on "seguir", a scene picked in a Referência de cena: the
/// first's goes straight into the scene change; the second's is kept in the Text attribute
/// "Próxima" when the scene starts and read back on "seguir".
fn next_scene_by_reference() -> (Project, Id, Id) {
    let mut project = Project::new("Cenas escolhidas");
    project.input_bindings.insert("seguir".into(), "N".into());
    project.scenes[0].kind = SceneKind::TwoD;
    let first = project.scenes[0].id.clone();
    let second_scene = Scene::new("Segunda", SceneKind::TwoD);
    let second = second_scene.id.clone();
    project.scenes.push(second_scene);

    let mut exit = Entity::new("Saída", None);
    let go = node(&mut exit, "event.input", &[("action", text("seguir"))]);
    let next = node(&mut exit, "value.scene", &[("scene", text(&second))]);
    let change = node(&mut exit, "action.scene", &[]);
    link(&mut exit, &go, &change);
    wire(&mut exit, &next, "value", &change, "scene");
    project.scenes[0].entities.push(exit);

    let mut exit = Entity::new("Saída", None);
    exit.attributes.insert("Próxima".into(), text(""));
    let start = node(&mut exit, "event.scene_start", &[]);
    let back = node(&mut exit, "value.scene", &[("scene", text(&first))]);
    let keep = node(
        &mut exit,
        "attribute.set",
        &[("attribute", text("Próxima")), ("value", text(""))],
    );
    link(&mut exit, &start, &keep);
    wire(&mut exit, &back, "value", &keep, "value");
    let go = node(&mut exit, "event.input", &[("action", text("seguir"))]);
    let read = node(
        &mut exit,
        "attribute.get",
        &[("attribute", text("Próxima"))],
    );
    let change = node(&mut exit, "action.scene", &[]);
    link(&mut exit, &go, &change);
    wire(&mut exit, &read, "value", &change, "scene");
    project.scenes[1].entities.push(exit);
    validate_project(&project).unwrap();
    (project, first, second)
}

#[test]
fn a_scene_reference_gives_the_scene_change_the_scene_picked() {
    let (project, first, second) = next_scene_by_reference();
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": [
            {"type": "press", "at": 0.5, "action": "seguir"},
            {"type": "release", "at": 0.6, "action": "seguir"},
            {"type": "press", "at": 1.0, "action": "seguir"},
        ]}),
    );
    let changes: Vec<_> = report
        .scene_changes
        .iter()
        .map(|c| (c.scene.as_str(), c.name.as_str(), c.reload))
        .collect();
    assert_eq!(
        changes,
        [
            (second.as_str(), "Segunda", false),
            (first.as_str(), project.scenes[0].name.as_str(), false)
        ]
    );
    assert!(report.logs.is_empty(), "{:?}", report.logs);
    assert_eq!(report.final_scene, first);

    // A reference left without a scene stops what reads it, naming the node.
    let mut project = project;
    let reference = project.scenes[0].entities[0]
        .graph
        .nodes
        .iter_mut()
        .find(|n| n.operation == "value.scene")
        .unwrap();
    reference.params.insert("scene".into(), text(""));
    validate_project(&project).unwrap();
    let report = run(
        &project,
        json!({"seconds": 1.0, "inputs": [{"type": "press", "at": 0.5, "action": "seguir"}]}),
    );
    assert!(
        report.scene_changes.is_empty(),
        "{:?}",
        report.scene_changes
    );
    let [log] = report.logs.as_slice() else {
        panic!("{:?}", report.logs);
    };
    assert!(
        log.text
            .contains("Referência de cena sem cena escolhida: escolha a cena no nó"),
        "{}",
        log.text
    );
    assert_eq!(log.object.as_ref().unwrap().name, "Saída");
    assert_eq!(log.node.as_ref().unwrap().operation, "action.scene");
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

fn bot(report: &SimulationReport) -> &headless::BotReport {
    report
        .bot
        .as_ref()
        .unwrap_or_else(|| panic!("no bot: {report:?}"))
}
/// How far `position` is from `point` across the ground.
fn across(position: [f32; 3], point: [f32; 3]) -> f32 {
    glam::Vec2::new(position[0] - point[0], position[2] - point[2]).length()
}

#[test]
fn a_bot_reaches_its_points_in_order_and_until_stops_when_it_is_done() {
    let project = course();
    let points = [[0., 0., -4.], [4., 0., -4.], [4., 0., 2.]];
    let path = json!({"entity": BODY, "path": points});
    let report = run(
        &project,
        json!({"seconds": 10.0, "bot": path, "until": {"bot": "done"}}),
    );
    let played = bot(&report);
    assert_eq!(
        (played.entity.id.as_str(), played.entity.name.as_str()),
        (BODY, "Corredor")
    );
    let indices: Vec<usize> = played.reached.iter().map(|p| p.index).collect();
    assert_eq!(indices, [0, 1, 2], "{played:?}");
    assert!(
        played.reached.windows(2).all(|w| w[0].time < w[1].time),
        "{played:?}"
    );
    for (reached, point) in played.reached.iter().zip(points) {
        assert!(across(reached.position, point) <= 0.6, "{reached:?}");
        assert!(reached.position[1].abs() < 0.1, "{reached:?}");
    }
    assert!(played.stuck.is_none());
    let done = played.done_at.unwrap();
    assert_eq!(done, played.reached[2].time, "no wait or jump at the end");
    // It stops right after the bot's last step, long before `seconds`.
    let stopped = report.stopped_at.as_ref().unwrap();
    assert!(
        stopped.reason == "bot" && near(stopped.time, done),
        "{stopped:?}"
    );
    assert_eq!(
        stopped.detail,
        "Corredor percorreu o caminho do bot até o fim"
    );
    assert!(done < 5., "{done}");

    // Without until, the run goes on and the character stops where the bot left it.
    let free = run(
        &project,
        json!({"seconds": 6.0, "bot": path, "samples": 1,
               "watch": [{"id": BODY, "fields": ["position", "character"]}]}),
    );
    assert!(free.stopped_at.is_none());
    assert_eq!(bot(&free), played);
    let last = free.watched[0].samples.last().unwrap();
    assert!(across(last.position.unwrap(), points[2]) < 1.5, "{last:?}");
    assert!(last.character.as_ref().unwrap().speed < 0.01, "{last:?}");
}

#[test]
fn a_jump_point_presses_the_characters_jump_action_once() {
    let mut project = course();
    let body = project.scenes[0].entity_mut(BODY).unwrap();
    // The game's logic hears the bot's presses as it hears a person's.
    let pressed = node(body, "event.input", &[("action", text("pular"))]);
    let said = node(body, "debug.message", &[("message", text("pulou"))]);
    link(body, &pressed, &said);
    let report = run(
        &project,
        json!({"seconds": 4.0, "until": {"bot": "any"},
               "bot": {"entity": BODY, "path": [{"at": [0, 0, -3], "jump": true}, [0, 0, -10]]}}),
    );
    let played = bot(&report);
    assert_eq!(played.reached.len(), 2, "{played:?}");
    let take_off = played.reached[0].time;
    let jumps: Vec<f64> = report
        .events
        .iter()
        .filter(|e| e.kind == EventKind::Jump && e.object.id == BODY)
        .map(|e| e.time)
        .collect();
    assert_eq!(jumps.len(), 1, "{:?}", report.events);
    assert!(near(jumps[0], take_off), "{jumps:?} {take_off}");
    let heard: Vec<f64> = report
        .logs
        .iter()
        .filter(|l| l.text == "pulou")
        .map(|l| l.time)
        .collect();
    assert_eq!(heard.len(), 1, "{:?}", report.logs);
    assert!(near(heard[0], take_off));
    // It landed before the last point: reached on the ground.
    let landed = report
        .events
        .iter()
        .any(|e| e.kind == EventKind::Land && e.time > jumps[0]);
    assert!(landed, "{:?}", report.events);
    assert!(played.reached[1].position[1].abs() < 0.1);
}

#[test]
fn a_wall_leaves_the_bot_stuck_with_the_point_it_could_not_reach() {
    let mut project = course();
    let mut wall = Entity::new("Muro", Some(Primitive::Cube));
    wall.dimensions = [10., 3., 0.5];
    wall.transform.position = [0., 1.5, -3.5];
    wall.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: wall.dimensions,
        },
        ..Default::default()
    });
    project.scenes[0].entities.push(wall);
    let path = json!({"entity": BODY, "path": [[0, 0, -2], [0, 0, -6]]});
    let report = run(
        &project,
        json!({"seconds": 6.0, "bot": path, "until": {"bot": "stuck"}}),
    );
    let played = bot(&report);
    assert_eq!(played.reached.len(), 1, "{played:?}");
    let stuck = played.stuck.as_ref().unwrap();
    assert_eq!(stuck.index, 1);
    // Against the wall, which ends 3.25 m away along the path.
    assert!(
        stuck.position[2] > -3.25 && stuck.position[2] < -2.5,
        "{stuck:?}"
    );
    assert!(
        (stuck.distance - (6. + stuck.position[2])).abs() < 0.1,
        "{stuck:?}"
    );
    assert!(stuck.time - played.reached[0].time > 1.5, "{stuck:?}");
    assert!(played.done_at.is_none());
    let stopped = report.stopped_at.as_ref().unwrap();
    assert!(stopped.reason == "bot" && near(stopped.time, stuck.time));
    assert!(
        stopped
            .detail
            .starts_with("Corredor parou de avançar rumo ao ponto 1 do bot, a "),
        "{}",
        stopped.detail
    );
    // until done does not stop on a stuck bot; a shorter patience gives up sooner.
    let patient = run(
        &project,
        json!({"seconds": 4.0, "until": {"bot": "done"},
               "bot": {"entity": BODY, "path": [[0, 0, -2], [0, 0, -6]], "stuck_after": 0.5}}),
    );
    assert!(patient.stopped_at.is_none());
    let sooner = bot(&patient).stuck.as_ref().unwrap();
    assert!(
        sooner.index == 1 && sooner.time < stuck.time - 0.5,
        "{sooner:?}"
    );
}

#[test]
fn the_bot_refuses_what_it_cannot_play() {
    let project = course();
    let refused = |project: &Project, value: serde_json::Value| {
        headless::simulate(project, &request(value)).unwrap_err()
    };
    let with = |bot: serde_json::Value| json!({"seconds": 1.0, "bot": bot});
    let point = json!([[0, 0, -2]]);
    for (bot, expected) in [
        (
            json!({"entity": GOAL, "path": point}),
            "bot: Meta não tem Personagem 3D",
        ),
        (
            json!({"entity": "nada", "path": point}),
            "não está na cena inicial",
        ),
        (json!({"entity": "$2", "path": point}), "$2"),
        (json!({"entity": BODY, "path": []}), "ao menos um ponto"),
        (
            json!({"entity": BODY, "path": [{"jump": true}]}),
            "ponto 0 do caminho (o 1º): informe at ou object",
        ),
        (
            json!({"entity": BODY, "path": [[0, 0, 0], {"object": "nada"}]}),
            "ponto 1 do caminho (o 2º): o objeto nada não está na cena inicial",
        ),
        (
            json!({"entity": BODY, "path": point, "radius": 0}),
            "radius precisa ser",
        ),
        (
            json!({"entity": BODY, "path": [{"at": [0, 0, 0], "wait": -1}]}),
            "wait precisa ser",
        ),
    ] {
        let error = refused(&project, with(bot));
        assert!(error.contains(expected), "{error}");
    }
    let mut logic_driven = project.clone();
    let body = logic_driven.scenes[0].entity_mut(BODY).unwrap();
    body.character3d.as_mut().unwrap().automatic_input = false;
    let error = refused(&logic_driven, with(json!({"entity": BODY, "path": point})));
    assert!(
        error.contains("Corredor está com a entrada automática desligada"),
        "{error}"
    );
    let mut flat = project.clone();
    flat.scenes[0].kind = SceneKind::TwoD;
    let error = refused(&flat, with(json!({"entity": BODY, "path": point})));
    assert!(error.contains("é 2D"), "{error}");
    let error = refused(&project, json!({"seconds": 1.0, "until": {"bot": "done"}}));
    assert!(error.contains("until: bot só vale com um bot"), "{error}");
    // Fields the bot does not know are refused when the request is read.
    for bot in [
        json!({"entity": BODY, "path": point, "speed": 2}),
        json!({"entity": BODY, "path": [{"at": [0, 0, 0], "pular": true}]}),
    ] {
        let read = serde_json::from_value::<SimulationRequest>(with(bot));
        assert!(read.is_err());
    }
}

#[test]
fn the_same_bot_run_repeats_itself() {
    let project = course();
    let value = json!({"seconds": 3.0,
        "bot": {"entity": BODY, "path": [
            {"at": [0, 0, -3], "jump": true, "sprint": true}, {"at": [3, 0, -6], "wait": 0.5},
            {"at": [0, 0, -9], "crouch": true}]},
        "watch": [{"id": BODY, "fields": ["position", "character"]}], "samples": 10});
    let first = serde_json::to_value(run(&project, value.clone())).unwrap();
    let second = serde_json::to_value(run(&project, value.clone())).unwrap();
    assert_eq!(first, second);
    assert!(
        first["bot"]["reached"].as_array().unwrap().len() >= 2,
        "{first}"
    );
    let replay = headless::check_replay(&project, &request(value)).unwrap();
    assert!(replay.identical, "{replay:?}");
    assert_eq!(replay.steps, 180);
}

#[test]
fn a_point_on_a_moving_platform_follows_it_and_waits_riding_it() {
    let mut project = course();
    let mut lift = Entity::new("Plataforma", Some(Primitive::Cube));
    lift.dimensions = [3., 0.1, 3.];
    lift.transform.position = [4., 0.05, -6.];
    lift.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: lift.dimensions,
        },
        ..Default::default()
    });
    lift.platform = Some(TranslationPlatform {
        mode: PlatformMode::Velocity,
        velocity: [1.5, 0., 0.],
        ..Default::default()
    });
    let lift_id = lift.id.clone();
    project.scenes[0].entities.push(lift);
    let report = run(
        &project,
        json!({"seconds": 8.0, "until": {"bot": "done"}, "samples": 1,
               "watch": [{"id": BODY, "fields": ["position", "character"]}, lift_id],
               "bot": {"entity": BODY,
                       "path": [{"object": lift_id, "at": [0, 0.05, 0], "wait": 1.0}]}}),
    );
    let played = bot(&report);
    let reached = &played.reached[0];
    // Where the platform was by then, not where it started.
    let platform_x = 4. + 1.5 * reached.time as f32;
    assert!(
        reached.position[0] > 4.5 && (reached.position[0] - platform_x).abs() <= 0.65,
        "{reached:?}"
    );
    assert!(
        near(played.done_at.unwrap(), reached.time + 1.),
        "{played:?}"
    );
    // It waited standing on the platform, which carried it along.
    let last = report.watched[0].samples.last().unwrap();
    let character = last.character.as_ref().unwrap();
    assert_eq!(
        character.support.as_ref().map(|s| s.name.as_str()),
        Some("Plataforma"),
        "{last:?}"
    );
    let lift_at = report.watched[1].samples.last().unwrap().position.unwrap();
    assert!(across(last.position.unwrap(), lift_at) < 1.5, "{last:?}");
    assert!(
        last.position.unwrap()[0] - reached.position[0] > 1.,
        "carried during the wait: {last:?}"
    );
}

#[test]
fn the_bot_plays_and_follows_objects_that_setup_creates() {
    let project = course();
    // "$N" from setup as the character it plays and as the object a point follows.
    let report = run(
        &project,
        json!({"seconds": 6.0, "until": {"bot": "any"},
               "setup": [
                   {"op": "create_entity", "kind": {"type": "group"}, "name": "Segundo",
                    "position": [-3, 0.02, -6]},
                   {"op": "add_component", "entity": "$0", "component": "character"},
                   {"op": "create_entity", "kind": {"type": "group"}, "name": "Marco",
                    "position": [3, 0, -6]},
               ],
               "bot": {"entity": "$0", "path": [{"object": "$2", "at": [0, 0, -1]}]}}),
    );
    let played = bot(&report);
    assert_eq!(
        (played.entity.id.as_str(), played.entity.name.as_str()),
        (report.setup[0].created[0].as_str(), "Segundo")
    );
    assert!(
        played.done_at.is_some() && played.stuck.is_none(),
        "{played:?}"
    );
    let reached = &played.reached[0];
    assert!(
        across(reached.position, [3., 0., -7.]) <= 0.6,
        "{reached:?}"
    );
}

#[test]
fn the_bot_takes_over_at_start_and_holds_crouch_and_sprint_over_the_movement_inputs() {
    let project = course();
    // The inputs push to the right all along, with a key and with the analog movement.
    let report = run(
        &project,
        json!({"seconds": 8.0, "samples": 120, "until": {"bot": "any"},
               "inputs": [{"type": "press", "at": 0.0, "action": "mover_direita"},
                          {"type": "movement", "at": 0.0, "value": [1, 0]}],
               "watch": [{"id": BODY, "fields": ["position", "character"]}],
               "bot": {"entity": BODY, "start": 1.0, "path": [
                   {"at": [2, 0, -3], "crouch": true, "wait": 0.5},
                   {"at": [2, 0, -9], "sprint": true}]}}),
    );
    let played = bot(&report);
    let indices: Vec<usize> = played.reached.iter().map(|p| p.index).collect();
    assert_eq!(indices, [0, 1], "{played:?}");
    let (first, second) = (played.reached[0].time, played.reached[1].time);
    assert!(first > 1., "{played:?}");
    let samples = &report.watched[0].samples;
    let character = |s: &headless::Sample| s.character.clone().unwrap();
    // Until start the inputs move it, to the right and walking.
    let before: Vec<_> = samples.iter().filter(|s| s.time < 1.).collect();
    assert!(
        before.iter().all(|s| s.position.unwrap()[2].abs() < 0.05
            && character(s).posture == "standing"
            && !character(s).sprinting),
        "{before:?}"
    );
    assert!(
        before.last().unwrap().position.unwrap()[0] > 1.,
        "{before:?}"
    );
    // Crouched while it waits at the first point, sprinting on the way to the second.
    let waiting: Vec<_> = samples
        .iter()
        .filter(|s| s.time > first + 0.05 && s.time < first + 0.5)
        .collect();
    assert!(
        !waiting.is_empty() && waiting.iter().all(|s| character(s).posture == "crouched"),
        "{waiting:?}"
    );
    assert!(
        samples
            .iter()
            .any(|s| s.time > first + 0.5 && s.time < second && character(s).sprinting),
        "{samples:?}"
    );
}

#[test]
fn a_crouch_that_toggles_gets_one_tap_per_point_even_when_logic_keeps_it() {
    let mut project = course();
    let body = project.scenes[0].entity_mut(BODY).unwrap();
    body.character3d.as_mut().unwrap().crouch_toggle = true;
    // Logic keeps the character standing and logs every press of crouch.
    let start = node(body, "event.scene_start", &[]);
    let stand = node(
        body,
        "character.posture",
        &[("command", text("crouch")), ("enabled", Value::Bool(false))],
    );
    link(body, &start, &stand);
    let pressed = node(body, "event.input", &[("action", text("agachar"))]);
    let said = node(body, "debug.message", &[("message", text("agachou"))]);
    link(body, &pressed, &said);
    let report = run(
        &project,
        json!({"seconds": 4.0, "until": {"bot": "any"},
               "bot": {"entity": BODY, "path": [{"at": [0, 0, -3], "crouch": true}, [0, 0, -5]]}}),
    );
    assert_eq!(bot(&report).reached.len(), 2, "{:?}", report.bot);
    let taps = report.logs.iter().filter(|l| l.text == "agachou").count();
    assert_eq!(taps, 1, "{:?}", report.logs);
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
/// An ID derived for a spawned copy: a version 8 UUID written as the editor writes IDs.
fn derived_uuid(id: &str) {
    let uuid = uuid::Uuid::parse_str(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    assert_eq!((uuid.get_version_num(), uuid.to_string()), (8, id.into()));
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
    // Each shot's copy and its logging node: named in the logs, so read as IDs, and the same
    // on every run, while every shot gets its own.
    let copies = |report: &SimulationReport| -> Vec<(Id, Id)> {
        report
            .logs
            .iter()
            .filter(|l| l.text == "acertou")
            .map(|l| {
                let (object, node) = (l.object.as_ref().unwrap(), l.node.as_ref().unwrap());
                assert_eq!(object.name, "Bala (cópia)");
                (object.id.clone(), node.id.clone())
            })
            .collect()
    };
    let shots = copies(&report);
    assert!(shots.len() > 200, "{}", shots.len());
    assert_eq!(
        shots,
        copies(&run(&project, json!({"seconds": 4.5, "inputs": inputs})))
    );
    let objects: HashSet<_> = shots.iter().map(|(object, _)| object).collect();
    let nodes: HashSet<_> = shots.iter().map(|(_, node)| node).collect();
    assert_eq!((objects.len(), nodes.len()), (shots.len(), shots.len()));
    for (object, node) in &shots {
        derived_uuid(object);
        derived_uuid(node);
    }
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

/// A Fábrica whose two Criar cópia nodes copy the Caixa (its Tampa, logic and animation
/// included) on each press of "gerar".
fn factory() -> Project {
    let mut project = Project::new("Fábrica");
    project.input_bindings.insert("gerar".into(), "G".into());
    project.scenes[0].kind = SceneKind::TwoD;
    let mut boxed = Entity::new("Caixa", None);
    let click = node(&mut boxed, "event.click", &[]);
    let say = node(&mut boxed, "debug.message", &[("message", text("aberta"))]);
    link(&mut boxed, &click, &say);
    boxed.clips.push(Clip::new("Abrir"));
    let mut lid = Entity::new("Tampa", None);
    lid.parent = Some(boxed.id.clone());
    let mut factory = Entity::new("Fábrica", None);
    let press = node(&mut factory, "event.input", &[("action", text("gerar"))]);
    let template = Value::Object(Some(boxed.id.clone()));
    let first = node(
        &mut factory,
        "action.spawn",
        &[("target", template.clone())],
    );
    let second = node(&mut factory, "action.spawn", &[("target", template)]);
    link(&mut factory, &press, &first);
    link(&mut factory, &first, &second);
    project.scenes[0].entities = vec![boxed, lid, factory];
    project
}
/// Each object the runtime copies when "gerar" is pressed twice in a run started with `seed`:
/// its ID, name, clip IDs and node IDs.
fn made(project: &Project, seed: u64) -> Vec<(Id, String, Vec<Id>, Vec<Id>)> {
    let options = RuntimeOptions {
        record: true,
        seed,
        ..Default::default()
    };
    let mut runtime = Runtime::with_options(project, &project.start_scene, options).unwrap();
    let press = InputFrame {
        pressed: ["gerar".into()].into(),
        ..Default::default()
    };
    for input in [&press, &InputFrame::default(), &press] {
        runtime.advance(FIXED_DT, input);
    }
    assert!(runtime.logs.is_empty(), "{:?}", runtime.logs);
    let spawned = runtime.recording.take().unwrap().spawned;
    spawned
        .into_iter()
        .map(|copy| {
            let clips = runtime.scene().entity(&copy.id).unwrap().clips.iter();
            let nodes = copy
                .graph
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect();
            (
                copy.id,
                copy.name,
                clips.map(|clip| clip.id.clone()).collect(),
                nodes,
            )
        })
        .collect()
}

#[test]
fn spawned_copies_get_the_same_ids_on_every_run_and_others_per_node_and_count() {
    let project = factory();
    let copies = made(&project, 0);
    // Two presses, two nodes, the Caixa and its Tampa each time.
    let names: Vec<_> = copies.iter().map(|copy| copy.1.as_str()).collect();
    assert_eq!(names, ["Caixa (cópia)", "Tampa"].repeat(4));
    assert_eq!(copies, made(&project, 0));
    let originals: HashSet<_> = project.scenes[0]
        .entities
        .iter()
        .flat_map(|e| {
            let clips = e.clips.iter().map(|clip| &clip.id);
            let nodes = e.graph.nodes.iter().map(|node| &node.id);
            std::iter::once(&e.id).chain(clips).chain(nodes)
        })
        .collect();
    let mut ids = HashSet::new();
    for (id, name, clips, nodes) in &copies {
        let expected = if name == "Tampa" { (0, 0) } else { (1, 2) };
        assert_eq!((clips.len(), nodes.len()), expected, "{name}");
        for id in std::iter::once(id).chain(clips).chain(nodes) {
            derived_uuid(id);
            assert!(!originals.contains(id) && ids.insert(id), "{id}");
        }
    }

    // A copy whose ID the scene already has gets another one, the same on every run; the rest
    // of the copy keeps its IDs.
    let mut crowded = project.clone();
    let mut squatter = Entity::new("Ocupante", None);
    squatter.id = copies[0].0.clone();
    crowded.scenes[0].entities.push(squatter);
    let moved = made(&crowded, 0);
    assert_eq!(moved, made(&crowded, 0));
    assert_ne!(moved[0].0, copies[0].0);
    derived_uuid(&moved[0].0);
    assert!(!ids.contains(&moved[0].0));
    assert_eq!(
        (&moved[0].1, &moved[0].2, &moved[0].3),
        (&copies[0].1, &copies[0].2, &copies[0].3)
    );
    assert_eq!(moved[1..], copies[1..]);

    // The seed plays no part: neither the one the run starts with nor one the logic sets
    // before each press's copies, which starts the draws over but not the copy counts.
    assert_eq!(made(&project, 99), copies);
    let mut seeding = project.clone();
    let factory = seeding.scenes[0].entities.last_mut().unwrap();
    let (press, first) = (
        factory.graph.nodes[0].id.clone(),
        factory.graph.nodes[1].id.clone(),
    );
    let reseed = node(factory, "random.seed", &[("seed", Value::Number(12345.))]);
    let pressed = factory
        .graph
        .edges
        .iter_mut()
        .find(|e| e.from_node == press)
        .unwrap();
    pressed.to_node = reseed.clone();
    link(factory, &reseed, &first);
    assert_eq!(made(&seeding, 7), copies);
}

#[test]
fn the_seed_asked_for_repeats_the_draws_and_comes_back_in_the_report() {
    let mut project = Project::new("Sorteio");
    project.scenes[0].kind = SceneKind::TwoD;
    let mut dice = Entity::new("Dado", None);
    dice.attributes.insert("valor".into(), Value::Number(0.));
    let step = node(&mut dice, "event.step", &[]);
    let draw = node(
        &mut dice,
        "random.number",
        &[("max", Value::Number(1e6)), ("mode", text("integer"))],
    );
    let write = node(&mut dice, "attribute.set", &[("attribute", text("valor"))]);
    link(&mut dice, &step, &draw);
    link(&mut dice, &draw, &write);
    wire(&mut dice, &draw, "value", &write, "value");
    let id = dice.id.clone();
    project.scenes[0].entities.push(dice);
    let draws = |seed: Option<u64>| -> (u64, Vec<Value>) {
        let mut asked = json!({"seconds": 1.0, "samples": 10, "watch": [id]});
        if let Some(seed) = seed {
            asked["seed"] = json!(seed);
        }
        let report = run(&project, asked);
        let values = report.watched[0]
            .samples
            .iter()
            .map(|sample| sample.attributes.as_ref().unwrap()["valor"].clone())
            .collect();
        (report.seed, values)
    };
    let (seed, zero) = draws(None);
    assert_eq!(seed, 0);
    assert_eq!(draws(Some(0)), (0, zero.clone()));
    let (seed, other) = draws(Some(12345));
    assert_eq!(seed, 12345);
    assert_ne!(other, zero);
    assert_eq!(draws(Some(12345)).1, other);
    let largest = draws(Some(MAX_SEED));
    assert_eq!(largest.0, MAX_SEED);
    let error = headless::simulate(
        &project,
        &request(json!({"seconds": 1.0, "seed": MAX_SEED + 1})),
    )
    .unwrap_err();
    assert!(
        error.contains("A semente (seed) deve ser um inteiro de 0 a 9007199254740991"),
        "{error}"
    );
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

/// A Menu and a Fase that count every scene start in the game variable Visitas, and the starts
/// of each scene in Recorde, keyed by the scene's name, after setting Recorde's own value to
/// 100 (so keys never written must read the initial 0, not it). The menu opens the level after
/// 0.5 s; the level reloads itself on "reiniciar".
fn visits() -> (Project, Id, Id) {
    let mut project = Project::new("Visitas");
    project
        .input_bindings
        .insert("reiniciar".into(), "R".into());
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0].name = "Menu".into();
    let visits = variables::create(&mut project, "Visitas", Value::Number(0.), false).unwrap();
    let record = variables::create(&mut project, "Recorde", Value::Number(0.), false).unwrap();
    let counter = || {
        let mut e = Entity::new("Contador", None);
        let start = node(&mut e, "event.scene_start", &[]);
        let read = node(&mut e, "variable.get", &[("variable", text(&visits))]);
        let add = node(&mut e, "math.binary", &[("b", Value::Number(1.))]);
        let write = node(&mut e, "variable.set", &[("variable", text(&visits))]);
        link(&mut e, &start, &write);
        wire(&mut e, &read, "value", &add, "a");
        wire(&mut e, &add, "value", &write, "value");
        let write_own = node(
            &mut e,
            "variable.set",
            &[("variable", text(&record)), ("value", Value::Number(100.))],
        );
        link(&mut e, &write, &write_own);
        let here = node(&mut e, "scene.current", &[]);
        let read_key = node(&mut e, "variable.get", &[("variable", text(&record))]);
        let add_key = node(&mut e, "math.binary", &[("b", Value::Number(1.))]);
        let write_key = node(&mut e, "variable.set", &[("variable", text(&record))]);
        link(&mut e, &write_own, &write_key);
        wire(&mut e, &here, "name", &read_key, "key");
        wire(&mut e, &here, "name", &write_key, "key");
        wire(&mut e, &read_key, "value", &add_key, "a");
        wire(&mut e, &add_key, "value", &write_key, "value");
        e
    };
    let mut level = Scene::new("Fase", SceneKind::TwoD);
    let level_id = level.id.clone();
    let mut menu = Entity::new("Menu", None);
    let start = node(&mut menu, "event.scene_start", &[]);
    let wait = node(
        &mut menu,
        "control.wait",
        &[("seconds", Value::Number(0.5))],
    );
    let open = node(&mut menu, "action.scene", &[("scene", text(&level_id))]);
    link(&mut menu, &start, &wait);
    link(&mut menu, &wait, &open);
    project.scenes[0].entities.extend([counter(), menu]);
    let mut hero = Entity::new("Herói", None);
    let restart = node(&mut hero, "event.input", &[("action", text("reiniciar"))]);
    let reload = node(&mut hero, "action.scene", &[("scene", text(&level_id))]);
    link(&mut hero, &restart, &reload);
    level.entities.extend([counter(), hero]);
    project.scenes.push(level);
    validate_project(&project).unwrap();
    (project, level_id, visits)
}
fn restart_at_one() -> serde_json::Value {
    json!([
        {"type": "press", "at": 1.0, "action": "reiniciar"},
        {"type": "release", "at": 1.1, "action": "reiniciar"},
    ])
}

#[test]
fn game_variables_survive_scene_changes_and_reloads_and_each_run_starts_over() {
    let (project, level, visits) = visits();
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": restart_at_one()}),
    );
    let changes: Vec<_> = report
        .scene_changes
        .iter()
        .map(|c| (c.scene.as_str(), c.reload))
        .collect();
    assert_eq!(changes, [(level.as_str(), false), (level.as_str(), true)]);
    // Menu, Fase and Fase again, each reading what the scene before wrote.
    assert_eq!(report.variables["Visitas"], Value::Number(3.));
    // Another run starts from the initial value again.
    let again = run(
        &project,
        json!({"seconds": 1.5, "inputs": restart_at_one()}),
    );
    assert_eq!(again.variables, report.variables);

    // A runtime fills the variables before the start logic, keeps them across scene changes,
    // and a new one starts over.
    let mut rt = Runtime::new(&project, &project.start_scene).unwrap();
    assert_eq!(rt.variable(&visits, ""), Some(&Value::Number(1.)));
    for expected in [2., 3.] {
        rt.change_scene(&level).unwrap();
        rt.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(rt.variable(&visits, ""), Some(&Value::Number(expected)));
    }
    let fresh = Runtime::new(&project, &project.start_scene).unwrap();
    assert_eq!(fresh.variable(&visits, ""), Some(&Value::Number(1.)));
    rt.stop();
    assert_eq!(rt.variable(&visits, ""), Some(&Value::Number(0.)));
    assert_eq!(rt.variable("variavel-inexistente", ""), None);
    // The project itself never changes.
    assert_eq!(project.variables[0].value, Value::Number(0.));
}

#[test]
fn a_key_keeps_one_value_per_scene_name_and_unwritten_keys_read_the_initial_value() {
    let (project, _, _) = visits();
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": restart_at_one()}),
    );
    // The keys count from the initial 0, not from the own value of 100.
    let expected: BTreeMap<String, Value> = [
        ("Recorde", 100.),
        ("Recorde · Fase", 2.),
        ("Recorde · Menu", 1.),
        ("Visitas", 3.),
    ]
    .into_iter()
    .map(|(label, value)| (label.to_owned(), Value::Number(value)))
    .collect();
    assert_eq!(report.variables, expected);
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["variables"]["Recorde · Fase"], json!({"Number": 2.0}));
    // Without variables, the report leaves them out.
    let (plain, _) = menu_and_level();
    let json = serde_json::to_value(run(&plain, json!({"seconds": 0.1}))).unwrap();
    assert!(json.get("variables").is_none());
}

#[test]
fn a_value_of_another_type_stops_only_the_object_that_writes_it() {
    let (mut project, _, visits) = visits();
    let start = project.start_scene.clone();
    let mut wrong = Entity::new("Errado", None);
    let step = node(&mut wrong, "event.step", &[]);
    let write = node(
        &mut wrong,
        "variable.set",
        &[("variable", text(&visits)), ("value", text("muitas"))],
    );
    link(&mut wrong, &step, &write);
    project.scene_mut(&start).unwrap().entities.push(wrong);
    let report = run(&project, json!({"seconds": 1.0}));
    let refused: Vec<_> = report
        .logs
        .iter()
        .filter(|l| l.text.contains("Valor incompatível"))
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", report.logs);
    assert_eq!(refused[0].object.as_ref().unwrap().name, "Errado");
    assert!(
        refused[0].text.starts_with(
            "Valor incompatível com o tipo da variável Visitas: ela guarda número, e o Novo \
             valor é texto"
        ),
        "{}",
        refused[0].text
    );
    // The menu still opened the level, and both counters counted.
    assert_eq!(report.scene_changes.len(), 1);
    assert_eq!(report.variables["Visitas"], Value::Number(2.));
}

#[test]
fn a_variable_keeps_up_to_1024_keys_and_refuses_longer_keys() {
    let mut project = Project::new("Chaves");
    project.scenes[0].kind = SceneKind::TwoD;
    let keys = variables::create(&mut project, "Chaves", Value::Number(0.), false).unwrap();
    // A new key every step: the time in hundredths of a second.
    let mut writer = Entity::new("Escritor", None);
    let step = node(&mut writer, "event.step", &[]);
    let time = node(&mut writer, "time.read", &[]);
    let format = node(&mut writer, "text.time", &[("format", text("mm:ss.cc"))]);
    let write = node(
        &mut writer,
        "variable.set",
        &[("variable", text(&keys)), ("value", Value::Number(1.))],
    );
    link(&mut writer, &step, &write);
    wire(&mut writer, &time, "time", &format, "seconds");
    wire(&mut writer, &format, "text", &write, "key");
    // A key one byte too long, once at the start.
    let mut long = Entity::new("Longa", None);
    let start = node(&mut long, "event.scene_start", &[]);
    let write_long = node(
        &mut long,
        "variable.set",
        &[
            ("variable", text(&keys)),
            ("key", text(&"a".repeat(variables::MAX_TEXT + 1))),
            ("value", Value::Number(1.)),
        ],
    );
    link(&mut long, &start, &write_long);
    project.scenes[0].entities.extend([writer, long]);
    validate_project(&project).unwrap();

    let seconds = (variables::MAX_KEYS + 30) as f64 * f64::from(FIXED_DT);
    let (report, runtime) = headless::run(&project, &request(json!({"seconds": seconds}))).unwrap();
    let errors = |text: &str| -> Vec<String> {
        report
            .logs
            .iter()
            .filter(|l| l.text.contains(text))
            .map(|l| l.object.as_ref().unwrap().name.clone())
            .collect()
    };
    assert_eq!(
        errors("A variável Chaves já guarda valores para 1024 chaves, o limite"),
        ["Escritor"],
        "{:?}",
        report.logs
    );
    assert_eq!(
        errors("A chave passa de 4096 bytes"),
        ["Longa"],
        "{:?}",
        report.logs
    );
    // Every key besides the own value, which the report lists first; the rest is counted.
    let (all, left_out) = runtime.variable_values(usize::MAX);
    assert_eq!((all.len(), left_out), (variables::MAX_KEYS + 1, 0));
    assert_eq!(report.variables.len(), headless::MAX_ENTRIES);
    assert_eq!(report.variables["Chaves"], Value::Number(0.));
    assert_eq!(
        report.truncated["variables"],
        variables::MAX_KEYS + 1 - headless::MAX_ENTRIES
    );
}

#[test]
fn until_stops_when_a_variable_or_a_key_reaches_a_value() {
    let (project, _, _) = visits();
    let report = run(
        &project,
        json!({"seconds": 5.0, "until": {"variable": {"variable": "visitas", "op": ">=",
                                                       "value": 2}}}),
    );
    let stopped = report.stopped_at.unwrap();
    assert_eq!(stopped.reason, "variable");
    assert_eq!(stopped.detail, "Visitas >= 2 (valor 2)");
    assert!(near(stopped.time, 0.5), "{stopped:?}");
    let report = run(
        &project,
        json!({"seconds": 5.0, "inputs": restart_at_one(),
               "until": {"variable": {"variable": "Recorde", "key": "Fase", "op": "==",
                                      "value": {"Number": 2}}}}),
    );
    let stopped = report.stopped_at.unwrap();
    assert_eq!(stopped.detail, "Recorde · Fase == 2 (valor 2)");
    assert!(near(stopped.time, 1.0), "{stopped:?}");
    for (until, expected) in [
        (
            json!({"variable": "Vidas", "op": "==", "value": 1}),
            "until: A variável do jogo \"Vidas\" não existe no projeto. Variáveis: Visitas, \
             Recorde",
        ),
        (
            json!({"variable": "Visitas", "op": "==", "value": "duas"}),
            "until: a variável Visitas é número, mas o valor informado é texto",
        ),
        (
            json!({"variable": "Visitas", "op": ">", "value": true}),
            "until: a comparação > precisa de um número",
        ),
    ] {
        let error = headless::simulate(
            &project,
            &request(json!({"seconds": 1.0, "until": {"variable": until}})),
        )
        .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[test]
fn runs_with_variables_repeat_and_setup_starts_them_elsewhere() {
    let (project, _, _) = visits();
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": restart_at_one(), "check_replay": true}),
    );
    let replay = report.replay.unwrap();
    assert!(replay.identical, "{replay:?}");
    // Only setup gives a variable another initial value, on the copy; the state digests name
    // the variable where two runs part.
    let state = |value: f64| {
        let asked = request(json!({"seconds": 0.1, "setup": [
            {"op": "update_variable", "variable": "Visitas", "value": value},
        ]}));
        let (report, runtime) = headless::run(&project, &asked).unwrap();
        (report.variables["Visitas"].clone(), runtime.state_digest())
    };
    let (zero, five) = (state(0.), state(5.));
    assert_eq!((&zero.0, &five.0), (&Value::Number(1.), &Value::Number(6.)));
    assert_eq!(
        zero.1.first_difference(&five.1),
        Some(StateDifference {
            object: None,
            part: "variables.Visitas".into()
        })
    );
}

/// `menu_and_level()` with a pause menu in the level: a hidden group that Ao pausar shows and
/// Ao retomar hides, holding Continuar (resumes) and Sair (quits), and a clock that logs once
/// half a second of game time has gone by in the level.
fn paused_level() -> (Project, Id, Id, Id) {
    let (mut project, level) = menu_and_level();
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
    let button = |name: &str, operation: &str, params: &[(&str, Value)]| {
        let mut e = Entity::new(name, None);
        e.parent = Some(menu.id.clone());
        e.ui = Some(UiElement {
            kind: UiKind::Button,
            text: name.into(),
            ..Default::default()
        });
        let click = node(&mut e, "event.click", &[]);
        let act = node(&mut e, operation, params);
        link(&mut e, &click, &act);
        e
    };
    let resume = button("Continuar", "game.pause", &[("mode", text("resume"))]);
    let quit = button("Sair", "game.quit", &[]);
    let mut clock = Entity::new("Relógio", None);
    let start = node(&mut clock, "event.scene_start", &[]);
    let wait = node(
        &mut clock,
        "control.wait",
        &[("seconds", Value::Number(0.5))],
    );
    let said = node(
        &mut clock,
        "debug.message",
        &[("message", text("meio segundo"))],
    );
    link(&mut clock, &start, &wait);
    link(&mut clock, &wait, &said);
    let (resume_id, quit_id) = (resume.id.clone(), quit.id.clone());
    project
        .scene_mut(&level)
        .unwrap()
        .entities
        .extend([menu, resume, quit, clock]);
    validate_project(&project).unwrap();
    (project, level, resume_id, quit_id)
}

#[test]
fn escape_opens_the_pause_menu_whose_buttons_resume_and_quit() {
    let (project, level, resume, quit) = paused_level();
    let log_time = |report: &SimulationReport, text: &str| {
        report
            .logs
            .iter()
            .find(|l| l.text == text)
            .map(|l| l.time)
            .unwrap_or_else(|| panic!("{text}: {:?}", report.logs))
    };
    let shown = |report: &SimulationReport, name: &str| {
        let element = report.ui.iter().find(|e| e.name == name).unwrap();
        (element.visible, element.clickable)
    };
    // The level opens at 1 s. Esc pauses it at 1.2, Continuar resumes it at 2, Esc again at 2.5.
    let inputs = json!([
        {"type": "escape", "at": 1.2},
        {"type": "press", "at": 1.5, "action": "pular"},
        {"type": "release", "at": 1.6, "action": "pular"},
        {"type": "click", "at": 2.0, "entity": resume},
        {"type": "escape", "at": 2.5},
    ]);
    let report = run(
        &project,
        json!({"seconds": 3.0, "inputs": inputs, "check_replay": true}),
    );
    assert!(
        report.replay.as_ref().unwrap().identical,
        "{:?}",
        report.replay
    );
    let pauses: Vec<_> = report.pauses.iter().map(|p| (p.time, p.paused)).collect();
    assert_eq!(pauses.len(), 3, "{pauses:?}");
    for ((time, paused), (expected, state)) in
        pauses
            .iter()
            .zip([(1.2167, true), (2.0167, false), (2.5167, true)])
    {
        assert!(near(*time, expected) && *paused == state, "{pauses:?}");
    }
    assert!(report.paused);
    // Input logic runs in the pause, at run time; the wait counts game time only: half a
    // second after 1.0, plus the 0.8 s the game spent paused.
    assert!(near(log_time(&report, "pulou"), 1.5), "{:?}", report.logs);
    assert!(
        near(log_time(&report, "meio segundo"), 2.3167),
        "{:?}",
        report.logs
    );
    assert_eq!(shown(&report, "Continuar"), (true, true));
    assert_eq!(shown(&report, "Sair"), (true, true));
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["pauses"][0], json!({"time": 1.2167, "paused": true}));
    assert_eq!(json["paused"], true);

    // Until the pause starts, and until it ends.
    let stop = run(
        &project,
        json!({"seconds": 3.0, "inputs": inputs, "until": {"paused": true}}),
    );
    let stopped = stop.stopped_at.as_ref().unwrap();
    assert_eq!(
        (stopped.reason, stopped.detail.as_str()),
        ("paused", "O jogo pausou")
    );
    assert!(near(stopped.time, 1.2167) && stop.paused, "{stopped:?}");
    assert_eq!(shown(&stop, "Continuar"), (true, true), "the menu shows");
    let stop = run(
        &project,
        json!({"seconds": 3.0, "inputs": inputs, "until": {"paused": false}}),
    );
    let stopped = stop.stopped_at.as_ref().unwrap();
    assert_eq!(stopped.detail, "O jogo saiu da pausa");
    assert!(near(stopped.time, 2.0167) && !stop.paused, "{stopped:?}");
    assert_eq!(shown(&stop, "Continuar"), (false, false));

    // Sair ends the run, with no `until`.
    let quitting = run(
        &project,
        json!({"seconds": 3.0, "inputs": [
            {"type": "escape", "at": 1.2},
            {"type": "click", "at": 1.5, "entity": quit},
        ]}),
    );
    let stopped = quitting.stopped_at.as_ref().unwrap();
    assert_eq!(stopped.reason, "quit");
    assert_eq!(stopped.detail, "O jogo pediu para sair (Sair do jogo)");
    assert!(near(stopped.time, 1.5167), "{stopped:?}");
    assert_eq!(quitting.final_scene, level);

    // A hidden button takes no click, as for a person, and says so.
    let hidden = run(
        &project,
        json!({"seconds": 1.5, "inputs": [{"type": "click", "at": 1.1, "entity": resume}]}),
    );
    assert!(hidden.pauses.is_empty() && !hidden.paused);
    let json = serde_json::to_value(&hidden).unwrap();
    assert!(json.get("pauses").is_none() && json["paused"] == false);
    let ignored = hidden
        .logs
        .iter()
        .find(|l| l.text.starts_with("Clique ignorado"))
        .unwrap();
    assert_eq!(
        ignored.text,
        "Clique ignorado: Continuar está oculto (ele mesmo ou um grupo acima dele)."
    );
    assert!(ignored.object.is_none());

    // The menu scene has no pause menu: Esc only logs.
    let early = run(
        &project,
        json!({"seconds": 0.5, "inputs": [{"type": "escape", "at": 0.2}]}),
    );
    assert!(early.pauses.is_empty() && !early.paused);
    assert!(
        early.logs[0]
            .text
            .starts_with("Esc: a cena não tem menu de pausa"),
        "{:?}",
        early.logs
    );

    // A scene change ends the pause: the level reloads itself on "reiniciar", which input
    // logic runs in the pause.
    let reload = run(
        &project,
        json!({"seconds": 2.0, "inputs": [
            {"type": "escape", "at": 1.2},
            {"type": "press", "at": 1.5, "action": "reiniciar"},
            {"type": "release", "at": 1.6, "action": "reiniciar"},
        ]}),
    );
    let pauses: Vec<_> = reload.pauses.iter().map(|p| (p.time, p.paused)).collect();
    assert!(
        pauses.len() == 2 && near(pauses[1].0, 1.5) && !pauses[1].1,
        "{pauses:?}"
    );
    assert!(!reload.paused);
    assert!(reload.scene_changes.last().unwrap().reload);
    assert_eq!(shown(&reload, "Continuar"), (false, false));
}

#[test]
fn clicks_and_scenes_go_by_name_and_a_click_elsewhere_says_so() {
    let (mut project, level, resume, _) = paused_level();
    // The menu scene opens the level at 1 s; the pause menu's Continuar is in the level.
    let report = run(
        &project,
        json!({"seconds": 1.5, "inputs": [
            {"type": "click", "at": 0.5, "entity": "continuar"},
            {"type": "escape", "at": 1.2},
            {"type": "click", "at": 1.3, "entity": " CONTINUAR "},
        ]}),
    );
    let ignored: Vec<_> = report
        .logs
        .iter()
        .filter(|l| l.text.starts_with("Clique ignorado"))
        .collect();
    assert_eq!(ignored.len(), 1, "{:?}", report.logs);
    assert_eq!(
        ignored[0].text,
        "Clique ignorado: Continuar não está na cena atual (Cena 2D)."
    );
    assert!(near(ignored[0].time, 0.5), "{:?}", ignored[0]);
    let pauses: Vec<_> = report.pauses.iter().map(|p| p.paused).collect();
    assert_eq!(pauses, [true, false], "the click by name resumed the game");
    // The scene to start in and the scene to stop at, by name.
    let report = run(&project, json!({"seconds": 0.5, "scene": "fase"}));
    assert_eq!(report.start_scene, level);
    let report = run(
        &project,
        json!({"seconds": 3.0, "until": {"scene_change": "FASE"}}),
    );
    let stopped = report.stopped_at.as_ref().unwrap();
    assert_eq!(stopped.reason, "scene_change");
    assert!(near(stopped.time, 1.0), "{stopped:?}");
    // What no object or scene is called is refused before the run.
    let refused = |project: &Project, value: serde_json::Value| {
        headless::simulate(project, &request(value)).unwrap_err()
    };
    assert_eq!(
        refused(
            &project,
            json!({"seconds": 1.0, "inputs": [
            {"type": "click", "at": 0.1, "entity": "Jogar"}]})
        ),
        "click: \"Jogar\" não é o ID nem o nome de um objeto do projeto"
    );
    let stray = new_id();
    assert!(
        refused(
            &project,
            json!({"seconds": 1.0, "inputs": [
            {"type": "click", "at": 0.1, "entity": stray}]})
        )
        .contains(&format!("\"{stray}\" não é o ID nem o nome")),
    );
    let error = refused(&project, json!({"seconds": 1.0, "scene": "Nada"}));
    assert!(
        error.starts_with("scene recebeu \"Nada\", que não é o ID nem o nome de uma cena")
            && error.contains("\"name\":\"Fase\""),
        "{error}"
    );
    let error = refused(
        &project,
        json!({"seconds": 1.0, "until": {"scene_change": "Nada"}}),
    );
    assert!(
        error.starts_with("until: scene_change recebeu \"Nada\""),
        "{error}"
    );
    // A copy action.spawn makes is clicked by its ID, though it does not exist yet; the line
    // does not show the ID.
    let copy = oxy_core::random::derived_id(&[b"copia"]);
    let report = run(
        &project,
        json!({"seconds": 0.2, "inputs": [{"type": "click", "at": 0.1, "entity": copy}]}),
    );
    let ignored: Vec<_> = report
        .logs
        .iter()
        .filter(|l| l.text.starts_with("Clique ignorado"))
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(
        ignored,
        ["Clique ignorado: o objeto clicado não está na cena atual (Cena 2D)."]
    );
    // An object the game removed keeps its name.
    let mut target = Entity::new("Alvo", None);
    let start = node(&mut target, "event.scene_start", &[]);
    let remove = node(&mut target, "action.remove", &[]);
    link(&mut target, &start, &remove);
    let mut removing = project.clone();
    removing.scene_mut(&level).unwrap().entities.push(target);
    let report = run(
        &removing,
        json!({"seconds": 0.2, "scene": "Fase", "inputs": [
            {"type": "click", "at": 0.1, "entity": "alvo"}]}),
    );
    assert!(
        report
            .logs
            .iter()
            .any(|l| l.text == "Clique ignorado: Alvo não está na cena atual (Fase)."),
        "{:?}",
        report.logs
    );
    // A name two objects share needs the ID.
    let mut twin = Entity::new("Continuar", None);
    twin.ui = Some(UiElement::default());
    project.scenes[0].entities.push(twin);
    let error = refused(
        &project,
        json!({"seconds": 1.0, "inputs": [
        {"type": "click", "at": 0.1, "entity": "Continuar"}]}),
    );
    assert!(
        error.starts_with("click: há 2 objetos chamados \"Continuar\"; use o ID: ")
            && error.contains(&format!("{resume} (cena Fase)")),
        "{error}"
    );
    run(
        &project,
        json!({"seconds": 0.2, "inputs": [{"type": "click", "at": 0.1, "entity": resume}]}),
    );
}
