//! The parkour in Luau (examples/parkour) against the parkour of 0.6 in nodes, kept as the
//! fixture tests/fixtures/parkour-0.6 (design §10 of 0.7): the same clicks, inputs and bot on
//! both give the same game. The two documents differ, and so does the state hash, so the runs
//! are compared by what the game does: the bot's points, the runner's Etapa, Tempo, Quedas and
//! Checkpoint atual over time, the movement and area events, the saves, the scene changes,
//! the pauses, the game variables, the sounds and the interface at the end.
use crate::parkour_example::{bot_path, fixture};
use oxy_core::{
    document::Project,
    game_ui::UiState,
    headless::{self, SimulationReport, SimulationRequest, SoundEvent},
    persistence,
    script::ScriptSources,
};
use serde_json::{Value as Json, json};
use std::path::Path;
use std::sync::Arc;

fn graph() -> Project {
    persistence::load_project(&fixture()).unwrap()
}

fn luau() -> (Project, Arc<ScriptSources>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/parkour");
    let project = persistence::load_project(&root.join(persistence::PROJECT_FILE)).unwrap();
    assert!(
        project
            .scenes
            .iter()
            .flat_map(|s| &s.entities)
            .all(|e| e.graph.nodes.is_empty()),
        "o parkour da 0.7 não tem lógica de nós"
    );
    let scripts = persistence::load_scripts(&root).unwrap();
    (project, Arc::new(scripts))
}

/// Runs `request` on both parkours: (nodes, Luau). The replay and VM checks run only on the
/// Luau one: parkour_example.rs replays the graph.
fn both(request: Json) -> (SimulationReport, SimulationReport) {
    let request: SimulationRequest = serde_json::from_value(request).unwrap();
    let plain = SimulationRequest {
        check_replay: false,
        check_vm_reset: false,
        ..request.clone()
    };
    let nodes = headless::simulate(&graph(), &plain).unwrap();
    let (project, scripts) = luau();
    let (scripted, _) = headless::run_with_scripts(&project, scripts, &request).unwrap();
    assert!(
        scripted.script_errors.is_empty(),
        "{:?}",
        scripted.script_errors
    );
    (nodes, scripted)
}

/// The runner, sampled as it goes.
fn runner() -> Json {
    json!([{
        "id": "Jogador",
        "fields": ["attributes"],
        "attributes": ["Etapa", "Tempo", "Quedas", "Checkpoint atual"]
    }])
}

/// The sounds by step, in an order that does not depend on which object asked first within a
/// step. The one change design §5.8 makes on purpose: the countdown plays its beeps and the
/// start's sound itself, as the count changes, where the graph's runner heard the count in its
/// next step; so on the graph each of those comes one step later.
fn heard(sounds: &[SoundEvent], heard_late: bool) -> Vec<(i64, String)> {
    let mut heard: Vec<(i64, String)> = sounds
        .iter()
        .map(|sound| {
            let mut step = (sound.time * 60.).round() as i64;
            if heard_late && sound.kind.is_none() && ["Bipe", "Largada"].contains(&&*sound.text) {
                step -= 1;
            }
            let mut written = serde_json::to_value(sound).unwrap();
            written.as_object_mut().unwrap().remove("time");
            (step, written.to_string())
        })
        .collect();
    heard.sort();
    heard
}

/// The interface at the end, but for the text of the menu's footer, which names the version of
/// OXY each parkour comes from.
fn interface(report: &SimulationReport) -> Vec<UiState> {
    let mut ui = report.ui.clone();
    for element in ui.iter_mut().filter(|u| u.name == "Rodapé") {
        element.text = None;
    }
    ui
}

fn assert_same_game(nodes: &SimulationReport, scripted: &SimulationReport) {
    assert_eq!(nodes.bot, scripted.bot);
    assert_eq!(nodes.watched, scripted.watched);
    assert_eq!(nodes.events, scripted.events);
    assert_eq!(nodes.saves, scripted.saves);
    assert_eq!(nodes.save, scripted.save);
    assert_eq!(nodes.scene_changes, scripted.scene_changes);
    assert_eq!(nodes.pauses, scripted.pauses);
    assert_eq!(nodes.variables, scripted.variables);
    assert_eq!(nodes.stopped_at, scripted.stopped_at);
    assert_eq!(nodes.final_scene, scripted.final_scene);
    assert_eq!(nodes.logs, scripted.logs);
    assert_eq!(nodes.audio, scripted.audio);
    assert_eq!(interface(nodes), interface(scripted));
    assert_eq!(heard(&nodes.sounds, true), heard(&scripted.sounds, false));
}

#[test]
fn the_parkour_in_luau_runs_the_course_as_the_graph_did_and_keeps_no_state_in_its_vm() {
    let (nodes, scripted) = both(json!({
        "seconds": 200,
        "seed": 0,
        "actions": "none",
        "check_replay": true,
        "check_vm_reset": true,
        "inputs": [{ "type": "click", "at": 0.5, "entity": "Jogar" }],
        "bot": { "entity": "Jogador", "start": 3.1, "path": bot_path() },
        "watch": runner(),
        "sample_every": 0.5,
        "until": {
            "attribute": { "entity": "Jogador", "name": "Etapa", "op": "==", "value": 3 },
            "bot": "stuck",
            "then_seconds": 1
        }
    }));
    assert_same_game(&nodes, &scripted);
    let bot = scripted.bot.as_ref().unwrap();
    assert_eq!(bot.done_at, Some(58.55), "{:?}", bot.stuck);
    assert_eq!(bot.held_by, ["chegada"]);
    let end = scripted.watched[0].samples.last().unwrap();
    let time = end.attributes.as_ref().unwrap()["Tempo"].number().unwrap();
    // 3300 steps of 1/60 s, summed as the step's length in f32 gives it.
    assert!((time - 55.).abs() < 1e-5, "{time}");
    // The replay and a VM made anew every 30 steps (and at drawn steps) stay the same as the
    // run, step for step: no script of the parkour keeps anything in its VM.
    for check in [&scripted.replay, &scripted.vm_reset] {
        let check = check.as_ref().unwrap();
        assert!(check.identical && check.steps == 3573, "{check:?}");
    }
    // The finish as a player sees it: the time, the record it set and the result panel.
    let shown = |name: &str| {
        let element = scripted.ui.iter().find(|u| u.name == name).unwrap();
        (element.visible, element.text.clone().unwrap_or_default())
    };
    assert_eq!(shown("Tempo"), (true, "Tempo 00:55.00".into()));
    assert_eq!(shown("Seu tempo"), (true, "Seu tempo 00:55.00".into()));
    assert_eq!(
        shown("Recorde do resultado"),
        (true, "Recorde 00:55.00".into())
    );
    assert_eq!(shown("Novo recorde"), (true, "Novo recorde!".into()));
    assert!(!shown("Recorde mantido").0 && !shown("Recorde igualado").0);
}

#[test]
fn the_parkour_in_luau_answers_the_menus_pauses_falls_and_restarts_as_the_graph_did() {
    let clicks = [
        (0.5, "Opções"),
        (0.8, "Música +"),
        (1.0, "Música -"),
        (1.2, "Música -"),
        (1.4, "Efeitos -"),
        (1.6, "Efeitos +"),
        (1.8, "Efeitos +"),
        (2.3, "Opções"),
        (2.5, "Voltar"),
        (2.8, "Jogar"),
        (4.5, "Continuar"),
        (14.3, "Reiniciar corrida"),
        (18.3, "Ir ao menu"),
        (19.0, "Sair"),
    ];
    let mut inputs: Vec<Json> = clicks
        .iter()
        .map(|(at, name)| json!({ "type": "click", "at": at, "entity": name }))
        .collect();
    // Esc in the menu closes the options; in the course it pauses, before each button of the
    // pause menu.
    for at in [2.0, 4.0, 14.0, 18.0] {
        inputs.push(json!({ "type": "escape", "at": at }));
    }
    inputs.push(json!({ "type": "press", "at": 12.0, "action": "Voltar ao checkpoint" }));
    inputs.push(json!({ "type": "release", "at": 12.1, "action": "Voltar ao checkpoint" }));
    let (nodes, scripted) = both(json!({
        "seconds": 30,
        "seed": 0,
        "actions": "none",
        "check_replay": true,
        "check_vm_reset": true,
        "inputs": inputs,
        // Running straight at the first gap without jumping: it falls, comes back and falls
        // again, until the course starts over.
        "bot": {
            "entity": "Jogador",
            "start": 3.2,
            "path": [{ "at": [0., 0., -15.4], "sprint": true }],
            "stuck_after": 30
        },
        "watch": runner(),
        "sample_every": 0.25
    }));
    assert_same_game(&nodes, &scripted);
    assert_eq!(scripted.stopped_at.as_ref().unwrap().reason, "quit");
    assert_eq!(
        scripted
            .scene_changes
            .iter()
            .map(|c| (c.name.as_str(), c.reload))
            .collect::<Vec<_>>(),
        [("Pista", false), ("Pista", true), ("Menu", false)]
    );
    assert_eq!(scripted.saves.len(), 6);
    assert_eq!(scripted.pauses.len(), 8);
    let falls = scripted.sounds.iter().filter(|s| s.text == "Queda").count();
    assert_eq!(falls, 4);
    // A VM made anew in the menus, in the pause and across the restart changes nothing either.
    for check in [&scripted.replay, &scripted.vm_reset] {
        let check = check.as_ref().unwrap();
        assert!(check.identical && check.steps == 1141, "{check:?}");
    }
}

#[test]
fn the_parkour_in_luau_ties_keeps_and_beats_the_record_as_the_graph_did() {
    // From the square before the finish line: a race of one second.
    let race = |save: Json, inputs: Json| {
        json!({
            "scene": "Pista",
            "seconds": 20,
            "seed": 0,
            "actions": "none",
            "save": save,
            "inputs": inputs,
            "spawn": { "entity": "Jogador", "position": [0., 4.02, -300.] },
            "bot": { "entity": "Jogador", "start": 3.1, "path": [{ "at": [0., 4., -309.] }] },
            "watch": runner(),
            "sample_every": 0.25,
            "until": {
                "attribute": { "entity": "Jogador", "name": "Etapa", "op": "==", "value": 3 },
                "then_seconds": 0.5
            }
        })
    };
    let result = |report: &SimulationReport| {
        report
            .ui
            .iter()
            .filter(|u| u.visible)
            .filter_map(|u| u.text.clone())
            .filter(|text| text.starts_with("Recorde") || text.starts_with("Novo"))
            .collect::<Vec<_>>()
    };
    for (best, shown) in [
        (
            json!(1.0),
            vec!["Recorde 00:01.00", "Recorde 00:01.00", "Recorde igualado!"],
        ),
        (
            json!(0.5),
            vec![
                "Recorde 00:00.50",
                "Recorde 00:00.50",
                "Recorde mantido · +00:00.50",
            ],
        ),
        (
            json!(2.0),
            vec!["Recorde 00:01.00", "Recorde 00:01.00", "Novo recorde!"],
        ),
    ] {
        let (nodes, scripted) = both(race(json!({ "Melhor tempo · Pista": best }), json!([])));
        assert_same_game(&nodes, &scripted);
        assert_eq!(result(&scripted), shown, "{best}");
    }
    // The buttons of the result panel: the menu shows the record just set, and the course
    // starts over with it.
    let after = |button: &str| json!([{ "type": "click", "at": 4.3, "entity": button }]);
    let (nodes, scripted) = both(race(json!({}), after("Menu do resultado")));
    assert_same_game(&nodes, &scripted);
    let best = scripted
        .ui
        .iter()
        .find(|u| u.name == "Melhor tempo do menu")
        .unwrap();
    assert_eq!(best.text.as_deref(), Some("Melhor tempo 00:01.00"));
    let (nodes, scripted) = both(race(json!({}), after("Jogar de novo")));
    assert_same_game(&nodes, &scripted);
    assert!(scripted.scene_changes.last().unwrap().reload);
}
