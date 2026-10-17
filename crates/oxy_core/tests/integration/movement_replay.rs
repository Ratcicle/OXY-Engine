#[path = "../../examples/support/movement.rs"]
mod movement;
#[path = "../../examples/support/replay.rs"]
mod replay;
#[test]
fn parkour_camera_teleport_and_platform_replay_preserve_fixed_states_and_events() {
    for mode in ["sparse", "platforms", "turntable"] {
        let reference = replay::run(60, mode);
        assert!(reference.events.len() > 20);
        assert_eq!(
            reference
                .events
                .iter()
                .filter(|(_, node)| *node == movement::id(7101))
                .count(),
            1
        );
        if mode != "sparse" {
            // The character rides the platform, which turns it in "turntable".
            assert!(
                reference
                    .samples
                    .iter()
                    .any(|s| s.support.as_deref() == Some(movement::id(2000).as_str())),
                "{mode}: {:?}",
                reference.samples
            );
        }
        for hz in [30, 144, 240] {
            replay::compare(&reference, &replay::run(hz, mode));
        }
    }
}
#[test]
fn benchmark_data_has_declared_counts_free_starts_and_real_moving_platforms() {
    use oxy_core::{
        physics3d::{CollisionShape, QueryOptions},
        runtime::{FIXED_DT, InputFrame, Runtime},
    };
    for n in [1, 10, 50] {
        for mode in ["sparse", "dense", "triangles", "platforms", "corridor"] {
            let p = movement::fixture(n, mode);
            assert_eq!(p.scenes[0].entities.len(), 400);
            assert_eq!(
                p.scenes[0]
                    .entities
                    .iter()
                    .filter(|e| e.character3d.is_some())
                    .count(),
                n
            );
            let mut r = Runtime::new(&p, &p.start_scene).unwrap();
            r.advance(FIXED_DT, &InputFrame::default());
            for i in 0..n {
                let id = movement::id(i);
                let s = r.character_state(&id).unwrap();
                assert!(
                    r.physics_world()
                        .unwrap()
                        .penetrating(
                            &CollisionShape::Capsule {
                                height: 1.8,
                                radius: 0.3
                            },
                            s.position + glam::Vec3::Y * 0.9,
                            &QueryOptions::excluding(&id)
                        )
                        .unwrap()
                        .is_empty()
                );
            }
            if mode == "platforms" {
                assert!(
                    r.scene()
                        .entity(&movement::id(2000))
                        .unwrap()
                        .transform
                        .position[0]
                        > p.scenes[0]
                            .entity(&movement::id(2000))
                            .unwrap()
                            .transform
                            .position[0]
                );
            }
            assert!(r.logs.is_empty(), "{mode}/{n}: {:?}", r.logs);
        }
    }
}
#[test]
fn turning_a_character_reuses_its_validated_capsule_scale() {
    use oxy_core::runtime::{FIXED_DT, InputFrame, Runtime};
    let mut p = replay::project("sparse");
    p.scenes[0]
        .entity_mut(&movement::id(3000))
        .unwrap()
        .camera_rig
        .as_mut()
        .unwrap()
        .mode = oxy_core::character::CameraMode::FirstPerson;
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    for _ in 0..10 {
        r.advance(FIXED_DT, &InputFrame::default());
    }
    let prepared = r.physics_world().unwrap().counters().shapes_prepared;
    for _ in 0..600 {
        r.advance(
            FIXED_DT,
            &InputFrame {
                look: [2.37, 0.],
                ..Default::default()
            },
        );
    }
    assert_eq!(
        r.physics_world().unwrap().counters().shapes_prepared,
        prepared
    );
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn adding_an_exit_listener_after_entry_preserves_occupancy() {
    use oxy_core::{
        document::*,
        graph::{Edge, Node},
        runtime::{FIXED_DT, InputFrame, Runtime},
    };
    let mut p = Project::new("Saída tardia");
    let mut area = Entity::new("Área", None);
    area.id = "area".into();
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    area.attributes.insert("Saiu".into(), Value::Bool(false));
    let mut other = Entity::new("Visitante", None);
    other.id = "visitor".into();
    other.collider = Some(Collider::default());
    p.scenes[0].entities = vec![area, other];
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    r.advance(FIXED_DT, &InputFrame::default());
    let e = r.scene_mut().entity_mut("area").unwrap();
    let exit = Node::new("event.area_exit", [0.; 2]);
    let mut set = Node::new("attribute.set", [0.; 2]);
    set.params
        .insert("attribute".into(), Value::Text("Saiu".into()));
    set.params.insert("value".into(), Value::Bool(true));
    e.graph.edges.push(Edge {
        from_node: exit.id.clone(),
        from_port: "exec".into(),
        to_node: set.id.clone(),
        to_port: "exec".into(),
    });
    e.graph.nodes = vec![exit, set];
    r.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        r.scene().entity("area").unwrap().attributes["Saiu"],
        Value::Bool(false)
    );
    r.scene_mut()
        .entity_mut("visitor")
        .unwrap()
        .transform
        .position[0] = 10.;
    r.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        r.scene().entity("area").unwrap().attributes["Saiu"],
        Value::Bool(true)
    );
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
#[test]
fn event_owner_index_survives_same_length_reordering_removal_and_insertion() {
    use oxy_core::{
        document::*,
        graph::{Edge, Node},
        metrics,
        runtime::{FIXED_DT, InputFrame, Runtime},
    };
    let mut p = Project::new("Índice de eventos");
    p.scenes[0].entities = (0..100)
        .map(|i| {
            let mut e = Entity::new("Objeto", None);
            e.id = movement::id(i);
            e
        })
        .collect();
    let owner = p.scenes[0].entities.last_mut().unwrap();
    let id = owner.id.clone();
    owner.attributes.insert("Valor".into(), Value::Number(0.));
    let event = Node::new("event.input", [0.; 2]);
    let mut set = Node::new("attribute.set", [0.; 2]);
    set.params
        .insert("attribute".into(), Value::Text("Valor".into()));
    set.params.insert("value".into(), Value::Number(7.));
    owner.graph.edges.push(Edge {
        from_node: event.id.clone(),
        from_port: "exec".into(),
        to_node: set.id.clone(),
        to_port: "exec".into(),
    });
    owner.graph.nodes = vec![event, set];
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    let input = InputFrame {
        pressed: ["atacar".into()].into(),
        ..Default::default()
    };
    for phase in 0..4 {
        if phase == 1 {
            r.scene_mut().entities.swap(0, 99);
        }
        if phase == 2 {
            r.scene_mut().entities.remove(40);
            let mut e = Entity::new("Inserido", None);
            e.id = movement::id(500);
            r.scene_mut().entities.insert(10, e);
        }
        r.scene_mut()
            .entity_mut(&id)
            .unwrap()
            .attributes
            .insert("Valor".into(), Value::Number(0.));
        metrics::take();
        r.advance(FIXED_DT, &input);
        let c = metrics::take();
        assert_eq!(
            r.scene().entity(&id).unwrap().attributes["Valor"],
            Value::Number(7.)
        );
        if metrics::ENABLED {
            assert_eq!(
                c.event_entity_visits, 1,
                "Must not scan decorative entities"
            );
        }
    }
    r.scene_mut().remove_subtree(&id);
    r.advance(FIXED_DT, &input);
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}
/// A pause the game takes holds the world still; after it, the run goes on as the run without
/// it did, later by the length of the pause.
#[test]
fn a_game_pause_delays_the_replay_without_changing_it() {
    use oxy_core::{
        input_timeline::{InputChange, TimedInput},
        runtime::{FIXED_DT, InputFrame, Runtime},
    };
    let dt = f64::from(FIXED_DT);
    // The pause starts after step 75, mid-jump, and lasts 90 steps.
    let (start, length) = (75, 90);
    for mode in ["sparse", "platforms", "turntable"] {
        let p = replay::project(mode);
        let id = movement::id(0);
        let inputs = [
            (0.21, InputChange::Movement([0., 1.])),
            (0.5, InputChange::Press("correr".into())),
            (1.113, InputChange::Press("pular".into())),
            (2.29, InputChange::Release("pular".into())),
            (2.7, InputChange::Press("agachar".into())),
            (3.3, InputChange::Release("agachar".into())),
            (3.9, InputChange::Movement([0.4, -1.])),
        ];
        let run = |paused: bool| {
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            for (at, change) in &inputs {
                let at = if paused && *at > start as f64 * dt {
                    at + length as f64 * dt
                } else {
                    *at
                };
                rt.queue_timed_input(TimedInput {
                    at,
                    change: change.clone(),
                })
                .unwrap();
            }
            let mut states = Vec::new();
            for step in 1..=300 + length {
                if paused && step == start + 1 {
                    assert!(rt.set_game_paused(true));
                }
                if paused && step == start + length + 1 {
                    assert!(rt.set_game_paused(false));
                }
                rt.advance(FIXED_DT, &InputFrame::default());
                let s = rt.character_state(&id).unwrap();
                states.push((
                    s.position.to_array(),
                    s.total_velocity().to_array(),
                    s.grounded,
                    format!("{:?}", s.posture),
                    s.support.as_ref().map(|s| s.object.clone()),
                ));
            }
            assert!(rt.logs.is_empty(), "{mode}: {:?}", rt.logs);
            states
        };
        let (plain, paused) = (run(false), run(true));
        assert!(plain.iter().any(|s| !s.2), "{mode}: the run jumps");
        assert_eq!(paused[..start], plain[..start], "{mode}");
        for (i, state) in paused[start..start + length].iter().enumerate() {
            assert_eq!(*state, plain[start - 1], "{mode}: paused step {i}");
        }
        for i in start..plain.len() - length {
            assert_eq!(paused[i + length], plain[i], "{mode}: step {}", i + 1);
        }
    }
}
