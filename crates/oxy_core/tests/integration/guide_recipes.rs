use oxy_core::{
    document::Value,
    guide_recipes,
    runtime::{FIXED_DT, InputFrame, Runtime},
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
#[test]
fn all_embedded_recipes_are_valid_editable_documents() {
    assert_eq!(guide_recipes::recipes().len(), 12);
    for i in 0..12 {
        let p = guide_recipes::load(i).unwrap();
        let before = p.clone();
        let mut runtime = Runtime::new(&p, &p.start_scene).unwrap();
        ticks(&mut runtime, 3);
        assert_eq!(p, before);
        assert!(
            !runtime.logs.iter().any(|l| l.contains("Erro")),
            "{:?}",
            runtime.logs
        );
        let json = serde_json::to_vec(&p).unwrap();
        assert_eq!(oxy_core::migration::read(&json).unwrap(), p);
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
    assert!(r.logs.iter().any(|l| l.contains("Energia insuficiente")));
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
