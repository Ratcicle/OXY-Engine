//! Progress saved between runs: a run starts the persistent game variables from the save its
//! host read, Salvar progresso and Apagar progresso queue requests for the host and never touch
//! the disk, and simulations take a save and report the ones made, so runs chain like a player
//! coming back.
use oxy_core::{
    document::*,
    graph::{Edge, Node},
    headless::{self, SaveKind, SimulationReport, SimulationRequest},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    save::{SaveRequest, SaveSlot},
    variables,
};
use serde_json::json;
use std::collections::BTreeMap;

fn node(entity: &mut Entity, operation: &str, params: &[(&str, Value)]) -> Id {
    let mut node = Node::new(operation, [0.; 2]);
    for (key, value) in params {
        node.params.insert((*key).into(), value.clone());
    }
    let id = node.id.clone();
    entity.graph.nodes.push(node);
    id
}
/// An execution link from the output `port` of `from`.
fn flow(entity: &mut Entity, from: &str, port: &str, to: &str) {
    wire(entity, from, port, to, "exec");
}
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
fn run(project: &Project, value: serde_json::Value) -> SimulationReport {
    let request: SimulationRequest = serde_json::from_value(value).unwrap();
    headless::simulate(project, &request).unwrap()
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.05
}

struct Game {
    project: Project,
    level: Id,
    seen: Id,
    menu_coins: Id,
    record: Id,
    coins: Id,
    timer: Id,
}
/// A Menu that keeps whether there is saved progress in Continuar and the coins it starts with
/// in Moedas no menu (neither is saved), then opens the Fase after 0.1 s. In the Fase, "terminar" keeps the time
/// in Tempo (never saved), adds a coin to Moedas and keeps the time as the Fase's Recorde when it
/// is the first or a better one, then saves; "apagar" clears the progress.
fn game() -> Game {
    let mut project = Project::new("Recordes");
    for (action, key) in [("terminar", "T"), ("apagar", "A")] {
        project.input_bindings.insert(action.into(), key.into());
    }
    project.scenes[0].kind = SceneKind::TwoD;
    project.scenes[0].name = "Menu".into();
    let record = variables::create(&mut project, "Recorde", Value::Number(0.), true).unwrap();
    let coins = variables::create(&mut project, "Moedas", Value::Number(0.), true).unwrap();
    let timer = variables::create(&mut project, "Tempo", Value::Number(0.), false).unwrap();
    let mut level = Scene::new("Fase", SceneKind::TwoD);
    let level_id = level.id.clone();

    let seen = variables::create(&mut project, "Continuar", Value::Bool(false), false).unwrap();
    let menu_coins =
        variables::create(&mut project, "Moedas no menu", Value::Number(-1.), false).unwrap();
    let mut menu = Entity::new("Menu", None);
    let start = node(&mut menu, "event.scene_start", &[]);
    let state = node(&mut menu, "save.state", &[]);
    let show = node(&mut menu, "variable.set", &[("variable", text(&seen))]);
    let read_coins = node(&mut menu, "variable.get", &[("variable", text(&coins))]);
    let copy = node(
        &mut menu,
        "variable.set",
        &[("variable", text(&menu_coins))],
    );
    let wait = node(
        &mut menu,
        "control.wait",
        &[("seconds", Value::Number(0.1))],
    );
    let open = node(&mut menu, "action.scene", &[("scene", text(&level_id))]);
    wire(&mut menu, &state, "exists", &show, "value");
    wire(&mut menu, &read_coins, "value", &copy, "value");
    for (from, to) in [
        (&start, &show),
        (&show, &copy),
        (&copy, &wait),
        (&wait, &open),
    ] {
        flow(&mut menu, from, "exec", to);
    }
    project.scenes[0].entities.push(menu);

    let mut finish = Entity::new("Chegada", None);
    let variable = |id: &Id| ("variable", text(id));
    let done = node(&mut finish, "event.input", &[("action", text("terminar"))]);
    let clock = node(&mut finish, "time.read", &[]);
    let keep_time = node(&mut finish, "variable.set", &[variable(&timer)]);
    let coins_now = node(&mut finish, "variable.get", &[variable(&coins)]);
    let plus = node(&mut finish, "math.binary", &[("b", Value::Number(1.))]);
    let add_coin = node(&mut finish, "variable.set", &[variable(&coins)]);
    let here = node(&mut finish, "scene.current", &[]);
    let best = node(&mut finish, "variable.get", &[variable(&record)]);
    let first = node(
        &mut finish,
        "condition.compare",
        &[("operator", text("==")), ("b", Value::Number(0.))],
    );
    let is_first = node(&mut finish, "condition.branch", &[]);
    let faster = node(&mut finish, "condition.compare", &[("operator", text("<"))]);
    let is_faster = node(&mut finish, "condition.branch", &[]);
    let set_best = node(&mut finish, "variable.set", &[variable(&record)]);
    let save = node(&mut finish, "save.write", &[]);
    wire(&mut finish, &clock, "time", &keep_time, "value");
    wire(&mut finish, &coins_now, "value", &plus, "a");
    wire(&mut finish, &plus, "value", &add_coin, "value");
    wire(&mut finish, &here, "name", &best, "key");
    wire(&mut finish, &best, "value", &first, "a");
    wire(&mut finish, &first, "result", &is_first, "condition");
    wire(&mut finish, &clock, "time", &faster, "a");
    wire(&mut finish, &best, "value", &faster, "b");
    wire(&mut finish, &faster, "result", &is_faster, "condition");
    wire(&mut finish, &here, "name", &set_best, "key");
    wire(&mut finish, &clock, "time", &set_best, "value");
    for (from, port, to) in [
        (&done, "exec", &keep_time),
        (&keep_time, "exec", &add_coin),
        (&add_coin, "exec", &is_first),
        (&is_first, "then", &set_best),
        (&is_first, "else", &is_faster),
        (&is_faster, "then", &set_best),
        (&is_faster, "else", &save),
        (&set_best, "exec", &save),
    ] {
        flow(&mut finish, from, port, to);
    }
    let erase = node(&mut finish, "event.input", &[("action", text("apagar"))]);
    let clear = node(&mut finish, "save.clear", &[]);
    flow(&mut finish, &erase, "exec", &clear);
    level.entities.push(finish);
    project.scenes.push(level);
    validate_project(&project).unwrap();
    Game {
        project,
        level: level_id,
        seen,
        menu_coins,
        record,
        coins,
        timer,
    }
}
fn press(action: &str) -> InputFrame {
    InputFrame {
        pressed: [action.into()].into(),
        ..Default::default()
    }
}
fn number(value: f64) -> Value {
    Value::Number(value)
}

#[test]
fn a_save_starts_the_persistent_variables_before_the_scene_start_logic() {
    let Game {
        project,
        seen,
        menu_coins,
        record,
        coins,
        timer,
        ..
    } = game();
    let start = project.start_scene.clone();
    let mut slot = SaveSlot {
        exists: true,
        ..SaveSlot::default()
    };
    slot.values
        .insert((coins.clone(), String::new()), number(7.));
    slot.values
        .insert((record.clone(), "Fase".into()), number(3.5));
    // What the run does not take: a variable never saved, another type, one the project lacks.
    slot.values
        .insert((timer.clone(), String::new()), number(9.));
    slot.values
        .insert((record.clone(), String::new()), text("rápido"));
    slot.values
        .insert(("sumiu".into(), String::new()), number(1.));
    let options = RuntimeOptions {
        save: Some(slot.clone()),
        ..Default::default()
    };
    let rt = Runtime::with_options(&project, &start, options).unwrap();
    // The menu's start logic already read them.
    assert_eq!(rt.variable(&seen, ""), Some(&Value::Bool(true)));
    assert_eq!(rt.variable(&menu_coins, ""), Some(&number(7.)));
    assert_eq!(rt.variable(&coins, ""), Some(&number(7.)));
    assert_eq!(rt.variable(&record, "Fase"), Some(&number(3.5)));
    assert_eq!(rt.variable(&record, ""), Some(&number(0.)));
    assert_eq!(rt.variable(&timer, ""), Some(&number(0.)));
    // In the order of the variables' IDs.
    let mut logs: Vec<_> = rt.logs.iter().map(|l| l.text.as_str()).collect();
    logs.sort();
    assert_eq!(
        logs,
        [
            "Progresso salvo ignorado: Recorde é texto, mas a variável Recorde guarda número",
            "Progresso salvo ignorado: a variável Tempo não é salva entre partidas",
            "Progresso salvo ignorado: a variável não existe no projeto",
        ]
    );
    assert_eq!(rt.save_slot(), &slot);
    assert!(rt.save_requests.is_empty());

    // Without a save, the game is as on a fresh install.
    let fresh = Runtime::new(&project, &start).unwrap();
    assert_eq!(fresh.variable(&seen, ""), Some(&Value::Bool(false)));
    assert_eq!(fresh.variable(&menu_coins, ""), Some(&number(0.)));
    assert_eq!(fresh.save_slot(), &SaveSlot::default());
}

#[test]
fn salvar_progresso_sends_only_persistent_values_and_apagar_resets_them() {
    let Game {
        project,
        level,
        seen,
        record,
        coins,
        timer,
        ..
    } = game();
    let mut rt = Runtime::new(&project, &project.start_scene).unwrap();
    rt.change_scene(&level).unwrap();
    rt.advance(FIXED_DT, &press("terminar"));
    let time = rt.variable(&timer, "").and_then(Value::number).unwrap();
    assert!(time > 0.);
    let expected: BTreeMap<(Id, String), Value> = [
        ((record.clone(), String::new()), number(0.)),
        ((record.clone(), "Fase".into()), number(time)),
        ((coins.clone(), String::new()), number(1.)),
    ]
    .into();
    assert_eq!(rt.save_requests, [SaveRequest::Write(expected.clone())]);
    assert!(rt.save_slot().exists);
    assert_eq!(rt.save_slot().values, expected);
    // The requests stay for the host across scene changes and a stop: they are state. The menu
    // now finds progress.
    rt.change_scene(&project.start_scene).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(rt.variable(&seen, ""), Some(&Value::Bool(true)));
    rt.change_scene(&level).unwrap();
    assert_eq!(rt.save_requests.len(), 1);

    rt.advance(FIXED_DT, &press("apagar"));
    assert_eq!(rt.save_requests.last(), Some(&SaveRequest::Clear));
    assert!(!rt.save_slot().exists && rt.save_slot().values.is_empty());
    // Persistent variables are back at their initial values, keys too; Tempo stays.
    assert_eq!(rt.variable(&coins, ""), Some(&number(0.)));
    assert_eq!(rt.variable(&record, "Fase"), Some(&number(0.)));
    assert_eq!(rt.variable(&timer, ""), Some(&number(time)));
    let (values, _) = rt.variable_values(10);
    assert!(!values.contains_key("Recorde · Fase"), "{values:?}");
    rt.stop();
    assert_eq!(rt.save_requests.len(), 2);
}

#[test]
fn requests_a_host_leaves_queued_fold_into_the_same_outcome() {
    let Game { project, level, .. } = game();
    let mut rt = Runtime::new(&project, &project.start_scene).unwrap();
    rt.change_scene(&level).unwrap();
    for step in 0..150 {
        let action = if step % 7 == 6 { "apagar" } else { "terminar" };
        rt.advance(FIXED_DT, &press(action));
        assert!(rt.save_requests.len() <= 65);
        // Carried out in order, they leave what the run itself saw.
        let mut slot = SaveSlot::default();
        for request in rt.save_requests.clone() {
            slot.apply(request);
        }
        assert_eq!(slot, *rt.save_slot(), "step {step}");
    }
}

#[test]
fn chained_runs_start_where_the_last_left_off_and_beat_the_stored_record() {
    let Game { project, .. } = game();
    let finish_at = |at: f64| {
        json!([
            {"type": "press", "at": at, "action": "terminar"},
            {"type": "release", "at": at + 0.05, "action": "terminar"},
        ])
    };
    // A fresh install: no progress at first, then the first record.
    let first = run(
        &project,
        json!({"seconds": 1.5, "inputs": finish_at(1.0), "ui": false}),
    );
    let menu_saw = |report: &SimulationReport| {
        (
            report.variables["Continuar"].clone(),
            report.variables["Moedas no menu"].clone(),
        )
    };
    assert_eq!(menu_saw(&first), (Value::Bool(false), number(0.)));
    assert_eq!(first.saves.len(), 1, "{:?}", first.saves);
    let write = &first.saves[0];
    assert_eq!(write.kind, SaveKind::Write);
    assert!(near(write.time, 1.0), "{write:?}");
    let values = write.values.as_ref().unwrap();
    let record = values["Recorde · Fase"].number().unwrap();
    assert!(near(record, 1.0), "{values:?}");
    assert_eq!(values["Moedas"], number(1.));
    assert_eq!(values["Recorde"], number(0.));
    assert!(!values.contains_key("Tempo"));
    assert_eq!(first.save.as_ref(), Some(values));

    // Back again with that save, faster: the coin count goes on and the record falls.
    let save = serde_json::to_value(first.save.as_ref().unwrap()).unwrap();
    let second = run(
        &project,
        json!({"seconds": 1.5, "inputs": finish_at(0.5), "ui": false, "save": save}),
    );
    assert_eq!(menu_saw(&second), (Value::Bool(true), number(1.)));
    let saved = second.save.clone().unwrap();
    let better = saved["Recorde · Fase"].number().unwrap();
    assert!(near(better, 0.5) && better < record, "{saved:?}");
    assert_eq!(saved["Moedas"], number(2.));
    assert_eq!(second.variables["Moedas"], number(2.));

    // Slower, from the second run's save: the record stays, and the save still keeps the coin.
    let third = run(
        &project,
        json!({"seconds": 2.5, "inputs": finish_at(2.0), "ui": false,
               "save": serde_json::to_value(&saved).unwrap()}),
    );
    let kept = third.save.clone().unwrap();
    assert_eq!(kept["Recorde · Fase"], number(better));
    assert_eq!(kept["Moedas"], number(3.));
    assert_eq!(
        third.variables["Tempo"].number().map(|t| near(t, 2.0)),
        Some(true)
    );

    // Plain values and names in any case are read too; a run that saves nothing reports the
    // save it started with.
    let plain = run(
        &project,
        json!({"seconds": 0.2, "ui": false,
               "save": {"moedas": 4, "Recorde · Fase": {"Number": 2.5}}}),
    );
    assert!(plain.saves.is_empty());
    let expected: BTreeMap<String, Value> = [
        ("Moedas".to_owned(), number(4.)),
        ("Recorde · Fase".to_owned(), number(2.5)),
    ]
    .into();
    assert_eq!(plain.save, Some(expected));
    assert_eq!(plain.variables["Moedas"], number(4.));
    // Without save and without Salvar progresso, no progress is reported.
    let json = serde_json::to_value(run(&project, json!({"seconds": 0.2}))).unwrap();
    assert!(
        json.get("save").is_none() && json.get("saves").is_none(),
        "{json}"
    );
}

#[test]
fn apagar_progresso_is_reported_and_leaves_no_save() {
    let Game { project, .. } = game();
    let report = run(
        &project,
        json!({"seconds": 1.0, "ui": false, "save": {"Moedas": 5},
        "inputs": [
            {"type": "press", "at": 0.5, "action": "terminar"},
            {"type": "press", "at": 0.8, "action": "apagar"},
        ]}),
    );
    let kinds: Vec<_> = report.saves.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, [SaveKind::Write, SaveKind::Clear]);
    assert_eq!(
        report.saves[0].values.as_ref().unwrap()["Moedas"],
        number(6.)
    );
    assert!(report.saves[1].values.is_none());
    assert_eq!(report.save, None);
    assert_eq!(report.variables["Moedas"], number(0.));
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(
        json["saves"][1],
        json!({"time": report.saves[1].time, "kind": "clear"})
    );
    assert_eq!(json["saves"][0]["kind"], "write");
    assert!(json["saves"][0].get("values_truncated").is_none());
}

#[test]
fn a_save_must_name_persistent_variables_with_values_of_their_type() {
    let Game { project, .. } = game();
    for (save, expected) in [
        (
            json!({"Vidas": 3}),
            "save: A variável do jogo \"Vidas\" não existe no projeto. Variáveis: Recorde, \
             Moedas, Tempo, Continuar, Moedas no menu",
        ),
        (
            json!({"Tempo": 3}),
            "save: a variável Tempo não é salva entre partidas; marque-a com update_variable e \
             persistent: true no setup, ou use update_variable no setup para mudar só o valor \
             inicial",
        ),
        (
            json!({"Recorde · Fase": "rápido"}),
            "save: Recorde · Fase é texto, mas a variável Recorde guarda número",
        ),
        (
            json!({"Moedas": 1, "moedas": 2}),
            "save: Moedas aparece duas vezes",
        ),
    ] {
        let request: SimulationRequest =
            serde_json::from_value(json!({"seconds": 0.1, "save": save})).unwrap();
        assert_eq!(
            headless::simulate(&project, &request).unwrap_err(),
            expected
        );
    }
    // Setup can make a variable persistent on the copy, and the save then takes it.
    let report = run(
        &project,
        json!({"seconds": 0.1, "ui": false, "save": {"Tempo": 3},
               "setup": [{"op": "update_variable", "variable": "Tempo", "persistent": true}]}),
    );
    assert_eq!(report.variables["Tempo"], number(3.));
    let long = "k".repeat(variables::MAX_TEXT + 1);
    let request: SimulationRequest =
        serde_json::from_value(json!({"seconds": 0.1, "save": {format!("Recorde · {long}"): 1}}))
            .unwrap();
    assert_eq!(
        headless::simulate(&project, &request).unwrap_err(),
        "save: a chave de Recorde passa de 4096 bytes"
    );
}

#[test]
fn the_save_is_part_of_the_state_and_runs_with_one_repeat() {
    let Game { project, level, .. } = game();
    let finish = json!([{"type": "press", "at": 0.5, "action": "terminar"}]);
    let with_save = run(
        &project,
        json!({"seconds": 1.0, "inputs": finish, "save": {"Moedas": 2}, "check_replay": true}),
    );
    assert!(
        with_save.replay.as_ref().unwrap().identical,
        "{:?}",
        with_save.replay
    );
    let again = run(
        &project,
        json!({"seconds": 1.0, "inputs": finish, "save": {"Moedas": 2}}),
    );
    assert_eq!(again.state_hash, with_save.state_hash);
    assert_eq!(again.saves, with_save.saves);
    // Another starting save is another state, and the digest names the variable it starts.
    let (_, a) = headless::run(
        &project,
        &serde_json::from_value(json!({"seconds": 0.05, "save": {"Recorde · Outra": 1}})).unwrap(),
    )
    .unwrap();
    let (_, b) = headless::run(
        &project,
        &serde_json::from_value(json!({"seconds": 0.05})).unwrap(),
    )
    .unwrap();
    let difference = a
        .state_digest()
        .first_difference(&b.state_digest())
        .unwrap();
    assert_eq!(
        (difference.object, difference.part.as_str()),
        (None, "variables.Recorde")
    );
    let parts: Vec<_> = a.state_digest().parts.into_iter().map(|p| p.name).collect();
    assert!(parts.contains(&"save".to_owned()), "{parts:?}");
    assert_ne!(a.state_hash(), b.state_hash());

    // Only the save differs: in the Fase, which reads no progress, a save holding the initial
    // value leaves every variable as on a fresh install, yet a new run would start from it.
    let in_level = |save: Option<serde_json::Value>| {
        let mut request = json!({"seconds": 0.05, "scene": level});
        if let Some(save) = save {
            request["save"] = save;
        }
        headless::run(&project, &serde_json::from_value(request).unwrap())
            .unwrap()
            .1
    };
    let fresh = in_level(None);
    for save in [json!({"Moedas": 0}), json!({})] {
        let returning = in_level(Some(save.clone()));
        let difference = returning
            .state_digest()
            .first_difference(&fresh.state_digest())
            .unwrap();
        assert_eq!(
            (difference.object, difference.part.as_str()),
            (None, "save"),
            "{save}"
        );
        assert_ne!(returning.state_hash(), fresh.state_hash(), "{save}");
    }
}
