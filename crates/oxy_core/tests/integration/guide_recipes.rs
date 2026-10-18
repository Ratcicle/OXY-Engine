use oxy_core::{
    document::Value,
    guide_recipes,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    save::{SaveRequest, SaveSlot},
};
fn id(n: u32) -> String {
    format!("02000000-0000-4000-8000-{n:012x}")
}
fn value(runtime: &Runtime, n: u32, attribute: &str) -> Value {
    runtime.scene().entity(&id(n)).unwrap().attributes[attribute].clone()
}
fn ticks(runtime: &mut Runtime, n: usize) {
    for _ in 0..n {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
}
fn walk_right(runtime: &mut Runtime, n: usize) {
    let input = InputFrame {
        held: ["mover_direita".into()].into(),
        ..Default::default()
    };
    for _ in 0..n {
        runtime.advance(FIXED_DT, &input);
    }
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
#[test]
fn all_embedded_recipes_are_valid_editable_documents() {
    assert_eq!(guide_recipes::recipes().len(), 14);
    for i in 0..14 {
        let p = guide_recipes::load(i).unwrap();
        let before = p.clone();
        let mut runtime = Runtime::new(&p, &p.start_scene).unwrap();
        ticks(&mut runtime, 3);
        assert_eq!(p, before);
        assert!(
            !runtime.logs.iter().any(|l| l.text.contains("interrompido")),
            "{:?}",
            runtime.logs
        );
        let json = serde_json::to_vec(&p).unwrap();
        assert_eq!(oxy_core::migration::read(&json).unwrap(), p);
    }
    // The recipes of the game nodes come with every node parameter (in the format of the frozen
    // node registry or later), without warnings.
    for i in [12, 13] {
        let p = guide_recipes::load(i).unwrap();
        let bytes = guide_recipes::recipes()[i].bytes;
        let saved: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let version = saved["schema_version"].as_u64().unwrap();
        assert!(
            version >= oxy_core::migration::NODE_REGISTRY_SCHEMA,
            "{version}"
        );
        let warnings = oxy_core::command::warnings(&p);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
#[test]
fn doorway_reads_the_entrant_key_and_keeps_invalid_visitors_out() {
    for valid in [false, true] {
        let mut p = guide_recipes::load(1).unwrap();
        let e = p.scenes[0].entity_mut(&id(210)).unwrap();
        e.attributes.insert("Chave".into(), Value::Bool(valid));
        e.transform.position = [1.3, 0., 0.];
        let mut r = Runtime::new(&p, &p.start_scene).unwrap();
        ticks(&mut r, 2);
        let door = r.scene().entity(&id(211)).unwrap();
        assert_eq!(door.collider.as_ref().unwrap().enabled, !valid);
        assert_eq!(door.visible, !valid);
        assert_eq!(value(&r, 210, "Chave"), Value::Bool(valid));
    }
}
#[test]
fn card_spends_only_after_the_condition_and_damage_has_no_hidden_rule() {
    let p = guide_recipes::load(2).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    r.click(&id(312));
    ticks(&mut r, 1);
    assert_eq!(value(&r, 310, "Energia"), Value::Number(1.));
    assert_eq!(value(&r, 311, "Vida"), Value::Number(30.));
    r.click(&id(312));
    ticks(&mut r, 1);
    assert_eq!(value(&r, 310, "Energia"), Value::Number(1.));
    assert_eq!(value(&r, 311, "Vida"), Value::Number(30.));
    assert!(
        r.logs
            .iter()
            .any(|l| l.text.contains("Energia insuficiente"))
    );
}
#[test]
fn animation_marker_opens_and_closes_one_activation_per_attack() {
    let p = guide_recipes::load(3).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    let input = InputFrame {
        pressed: [id(402)].into(),
        ..Default::default()
    };
    for life in [85., 70., 55.] {
        r.advance(FIXED_DT, &input);
        ticks(&mut r, 5);
        assert!(
            !r.scene()
                .entity(&id(413))
                .unwrap()
                .collider
                .as_ref()
                .unwrap()
                .enabled
        );
        ticks(&mut r, 10);
        assert_eq!(value(&r, 414, "Vida"), Value::Number(life));
        ticks(&mut r, 35);
        assert_eq!(value(&r, 414, "Vida"), Value::Number(life));
        assert!(
            !r.scene()
                .entity(&id(413))
                .unwrap()
                .collider
                .as_ref()
                .unwrap()
                .enabled
        );
        assert_eq!(r.pending_tasks(), 0);
        assert_eq!(r.retained_counts()[0], 0);
    }
}
#[test]
fn passage_changes_to_the_configured_scene_and_cancels_old_work() {
    let mut p = guide_recipes::load(4).unwrap();
    p.scenes[0].entity_mut(&id(510)).unwrap().transform.position = [3., 0., 0.];
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 2);
    assert_eq!(r.scene_id(), id(502));
    assert_eq!(r.pending_tasks(), 0);
    assert_eq!(r.retained_counts()[0], 0);
}
#[test]
fn pause_menu_opens_with_the_game_pause_and_resumes_restarts_and_quits() {
    let p = guide_recipes::load(12).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    let shown = |r: &Runtime| r.scene().entity(&id(610)).unwrap().visible;
    let x = |r: &Runtime| r.scene().entity(&id(616)).unwrap().transform.position[0];
    assert!(r.has_pause_menu() && !shown(&r));
    walk_right(&mut r, 20);
    let walked = x(&r);
    assert!(walked > -2.5, "{walked}");
    // What Esc does in a scene that listens to Ao pausar ou retomar.
    r.toggle_game_pause();
    ticks(&mut r, 1);
    assert!(r.game_paused() && shown(&r));
    walk_right(&mut r, 20);
    assert_eq!(x(&r), walked);
    r.click(&id(613));
    ticks(&mut r, 1);
    assert!(!r.game_paused() && !shown(&r));
    walk_right(&mut r, 5);
    assert!(x(&r) > walked);
    // Reiniciar opens the scene it is in again, as it began and out of the pause.
    r.toggle_game_pause();
    ticks(&mut r, 1);
    r.click(&id(614));
    ticks(&mut r, 1);
    assert_eq!(r.scene_id(), p.start_scene);
    assert!(!r.game_paused() && !shown(&r));
    assert!(x(&r) < -2.99, "{}", x(&r));
    r.toggle_game_pause();
    ticks(&mut r, 1);
    assert!(!r.quit_requested);
    r.click(&id(615));
    ticks(&mut r, 1);
    assert!(r.quit_requested);
    assert!(
        !r.logs.iter().any(|l| l.text.contains("interrompido")),
        "{:?}",
        r.logs
    );
}
#[test]
fn record_is_kept_by_scene_name_and_saved_only_when_it_improves() {
    let best = id(702);
    let placar = |r: &Runtime, attribute: &str| value(r, 711, attribute);
    // Mm:ss.cc, truncated as Formatar tempo writes it.
    let clock = |seconds: f64| {
        let cents = (seconds * 100.).floor() as u64;
        text(&format!(
            "{:02}:{:02}.{:02}",
            cents / 6000,
            cents / 100 % 60,
            cents % 100
        ))
    };
    let run = |near: bool, save: Option<SaveSlot>| {
        let mut p = guide_recipes::load(13).unwrap();
        if near {
            p.scenes[0].entity_mut(&id(712)).unwrap().transform.position = [2.5, 0., 0.];
        }
        let options = RuntimeOptions {
            save,
            ..Default::default()
        };
        let mut r = Runtime::with_options(&p, &p.start_scene, options).unwrap();
        let before = placar(&r, "Recorde");
        for _ in 0..300 {
            if placar(&r, "Tempo") != text("--:--.--") {
                break;
            }
            walk_right(&mut r, 1);
        }
        assert_ne!(placar(&r, "Tempo"), text("--:--.--"), "{:?}", r.logs);
        (r, before)
    };
    // The first time, the time becomes the record, kept under the scene's name and saved.
    let (r, before) = run(true, None);
    assert_eq!(before, text("--:--.--"));
    let first = r.variable(&best, "Fase 1").and_then(Value::number).unwrap();
    assert!(first > 0. && first < 1., "{first}");
    assert_eq!(r.variable(&best, ""), Some(&Value::Number(0.)));
    let expected = [
        ((best.clone(), String::new()), Value::Number(0.)),
        ((best.clone(), "Fase 1".to_owned()), Value::Number(first)),
    ]
    .into();
    assert_eq!(r.save_requests, [SaveRequest::Write(expected)]);
    assert_eq!(placar(&r, "Recorde"), clock(first));
    assert_eq!(placar(&r, "Tempo"), clock(first));
    // Back with that save, from the start: the record shows at once, and the slower run
    // keeps it and saves nothing.
    let (r, before) = run(false, Some(r.save_slot().clone()));
    assert_eq!(before, clock(first));
    assert!(r.save_requests.is_empty());
    assert_eq!(r.variable(&best, "Fase 1"), Some(&Value::Number(first)));
    assert_eq!(placar(&r, "Recorde"), clock(first));
    let Value::Text(time) = placar(&r, "Tempo") else {
        panic!("Tempo não é texto")
    };
    let line = format!("Recorde mantido, nada foi salvo. Seu tempo: {time}");
    assert!(
        r.logs.iter().any(|l| l.text.ends_with(&line)),
        "{:?}",
        r.logs
    );
    // A slower record falls and is saved again.
    let mut slow = SaveSlot {
        exists: true,
        ..SaveSlot::default()
    };
    slow.values
        .insert((best.clone(), "Fase 1".into()), Value::Number(99.));
    let (r, before) = run(true, Some(slow));
    assert_eq!(before, text("01:39.00"));
    let better = r.variable(&best, "Fase 1").and_then(Value::number).unwrap();
    assert!(better < 1., "{better}");
    assert_eq!(r.save_requests.len(), 1);
    assert_eq!(placar(&r, "Recorde"), clock(better));
    assert!(
        !r.logs.iter().any(|l| l.text.contains("interrompido")),
        "{:?}",
        r.logs
    );
}
