//! The parkour that ships with OXY (examples/parkour), played from the menu to the finish line
//! by the bot of the agents' guide (docs/agentes-mcp.md). Its graphs use most of the runtime's
//! operations, so the state at the end pins down what they do, step by step.
use oxy_core::{
    headless::{self, SimulationRequest},
    persistence,
};
use serde_json::json;
use std::path::Path;

/// The state at the finish line, measured on 0.6.0 (0a4d813), where 0.7 started. A change that
/// alters what the parkour does on purpose updates it and says why; any other change, such as
/// a refactor of the runtime, leaves it as it is.
const FINISH_STATE: &str = "071503f82e92a690";

fn point(at: [f64; 3], sprint: bool, jump: bool) -> serde_json::Value {
    json!({ "at": at, "sprint": sprint, "jump": jump })
}

/// The course from the start line to the finish, as docs/agentes-mcp.md drives it: sprinting
/// and jumping over the gaps, crouching under the bars, riding the raft and the lift.
fn bot_path() -> serde_json::Value {
    let mut path: Vec<serde_json::Value> = [
        [0., 0., -5.4],
        [0., 0., -15.4],
        [0., 0., -23.4],
        [0., 0., -31.9],
        [0., 0.5, -39.9],
        [0., 1., -47.9],
    ]
    .into_iter()
    .map(|at| point(at, true, true))
    .collect();
    path.extend([
        point([1., 1.5, -53.5], true, false),
        point([0.2, 1.5, -56.], true, true),
        point([-1., 1., -64.4], true, true),
        point([0., 0.5, -76.], true, false),
        point([0., 0.5, -79.9], true, true),
        point([0., 0.5, -87.5], true, false),
    ]);
    for (under, past) in [
        (-93., -95.5),
        (-101., -103.5),
        (-109., -111.5),
        (-117., -118.5),
        (-127.5, -129.5),
    ] {
        path.push(json!({ "at": [0., 0.5, under], "sprint": true, "crouch": true }));
        path.push(point([0., 0.5, past], true, false));
    }
    path.extend([
        json!({ "at": [0., 0.5, -131.9] }),
        json!({ "object": "Balsa", "at": [0., 0.2, 0.], "wait_for": { "object": "Balsa" } }),
        json!({ "at": [0., 0.5, -158.], "wait_for": { "object": "Cais da balsa" } }),
        point([0., 0.5, -161.], true, false),
        point([0., 0.5, -165.3], true, true),
        point([0., 0.5, -175.2], true, true),
        point([0., 1., -185.7], true, true),
        point([0., 1., -192.], true, false),
        point([0., 1., -195.4], true, false),
        point([0., 1., -210.3], true, true),
        point([0., 1., -229.], true, false),
        point([0., 1., -232.], true, false),
        point([0., 1., -235.4], true, true),
        point([0.5, 1.75, -241.6], true, true),
        point([0.9, 2.5, -246.], true, false),
        point([0.4, 2.5, -248.1], true, true),
        point([-0.8, 3.25, -252.5], true, false),
        point([-0.8, 3.25, -254.6], true, true),
        point([0., 4., -262.], true, false),
        json!({ "at": [0., 4., -267.] }),
        json!({ "object": "Elevador", "at": [0., 0.2, 0.], "wait_for": { "object": "Elevador" } }),
        json!({ "at": [0., 8., -273.], "sprint": true, "wait_for": { "object": "Esteira" } }),
        point([0., 8., -294.2], true, true),
        point([0., 4., -309.], true, false),
    ]);
    serde_json::Value::Array(path)
}

#[test]
fn shipped_parkour_reaches_the_finish_in_the_same_state_and_replays_identically() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/parkour")
        .join(persistence::PROJECT_FILE);
    let project = persistence::load_project(&file).unwrap();
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 200,
        "seed": 0,
        "check_replay": true,
        "actions": "none",
        "ui": false,
        "inputs": [{ "type": "click", "at": 0.5, "entity": "Jogar" }],
        "bot": { "entity": "Jogador", "start": 3.1, "path": bot_path() },
        "until": {
            "attribute": { "entity": "Jogador", "name": "Etapa", "op": "==", "value": 3 },
            "bot": "stuck"
        }
    }))
    .unwrap();
    let report = headless::simulate(&project, &request).unwrap();
    let bot = report.bot.as_ref().unwrap();
    assert!(bot.stuck.is_none(), "{:?}", bot.stuck);
    assert!(
        bot.done_at.is_some_and(|at| (at - 58.55).abs() < 0.001),
        "{:?}",
        report.stopped_at
    );
    let replay = report.replay.as_ref().unwrap();
    assert!(replay.identical, "{replay:?}");
    assert_eq!(replay.steps, 3513);
    assert_eq!(report.state_hash, FINISH_STATE);
}
