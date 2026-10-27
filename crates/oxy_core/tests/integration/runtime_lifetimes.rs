use oxy_core::{
    animation::{AnimationEvent, Clip},
    character::{CharacterConfig, MovementEvent},
    document::*,
    graph::{Edge, Graph, Node},
    input_timeline::{InputChange, TimedInput},
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    scene_view::SceneEvaluation,
};
fn edge(g: &mut Graph, a: &Node, port: &str, b: &Node, input: &str) {
    g.edges.push(Edge {
        from_node: a.id.clone(),
        from_port: port.into(),
        to_node: b.id.clone(),
        to_port: input.into(),
    });
}
fn attacks(area: bool) -> (Project, Id) {
    let mut p = Project::new("Lifetimes");
    let mut e = Entity::new("Origin", None);
    e.attributes.insert("Vida".into(), Value::Number(100000.));
    if area {
        e.collider = Some(Collider {
            is_trigger: true,
            ..Default::default()
        });
    }
    let event = Node::new(
        if area {
            "event.area_enter"
        } else {
            "event.input"
        },
        [0.; 2],
    );
    let damage = Node::new("action.damage", [0.; 2]);
    let mut wait = Node::new("control.wait", [0.; 2]);
    wait.params.insert("seconds".into(), Value::Number(0.05));
    let again = Node::new("action.damage", [0.; 2]);
    edge(&mut e.graph, &event, "exec", &damage, "exec");
    edge(&mut e.graph, &damage, "exec", &wait, "exec");
    edge(&mut e.graph, &wait, "exec", &again, "exec");
    e.graph.nodes = vec![event, damage, wait, again];
    let id = e.id.clone();
    p.scenes[0].entities.push(e);
    if area {
        let mut t = Entity::new("Contact", None);
        t.collider = Some(Collider::default());
        p.scenes[0].entities.push(t);
    }
    (p, id)
}
#[test]
fn completed_attacks_release_records_and_waits_keep_same_activation() {
    let (p, id) = attacks(false);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    for _ in 0..300 {
        rt.advance(
            FIXED_DT,
            &InputFrame {
                pressed: ["atacar".into()].into(),
                ..Default::default()
            },
        );
        assert!(rt.retained_counts()[0] <= 5);
    }
    for _ in 0..5 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        rt.scene().entity(&id).unwrap().attributes["Vida"],
        Value::Number(97000.)
    );
    assert_eq!(rt.retained_counts(), [0, 0, 0, 0]);
    assert_eq!(
        p.scenes[0].entities[0].attributes["Vida"],
        Value::Number(100000.)
    );
    rt.stop();
    assert_eq!(rt.retained_counts(), [0, 0, 0, 0]);
}

#[test]
fn external_reorder_and_same_count_replacement_invalidate_runtime_indices() {
    let (mut p, owner) = attacks(false);
    let mut other = Entity::new("Other", None);
    other.attributes.insert("Vida".into(), Value::Number(123.));
    p.scenes[0].entities.push(other);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    let input = InputFrame {
        pressed: ["atacar".into()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &input);
    rt.scene_mut().entities.reverse();
    rt.advance(FIXED_DT, &input);
    assert_eq!(
        rt.scene().entity(&owner).unwrap().attributes["Vida"],
        Value::Number(99980.)
    );
    assert_eq!(
        rt.scene().entities[0].attributes["Vida"],
        Value::Number(123.)
    );
    rt.scene_mut().entities.remove(0);
    rt.scene_mut()
        .entities
        .insert(0, Entity::new("Replacement", None));
    rt.advance(FIXED_DT, &input);
    assert_eq!(
        rt.scene().entity(&owner).unwrap().attributes["Vida"],
        Value::Number(99970.)
    );
    rt.remove_object(&owner);
    for _ in 0..8 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(rt.retained_counts(), [0; 4]);
}

#[test]
fn reactivation_does_not_expire_old_waits_or_share_new_hits() {
    let (p, owner) = attacks(true);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.scene_mut()
        .entity_mut(&owner)
        .unwrap()
        .collider
        .as_mut()
        .unwrap()
        .enabled = false;
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.scene_mut()
        .entity_mut(&owner)
        .unwrap()
        .collider
        .as_mut()
        .unwrap()
        .enabled = true;
    rt.advance(FIXED_DT, &InputFrame::default());
    for _ in 0..8 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        rt.scene().entity(&owner).unwrap().attributes["Vida"],
        Value::Number(99980.)
    );
    assert_eq!(rt.retained_counts(), [1, 1, 0, 0]);
    rt.stop();
    assert_eq!(rt.retained_counts(), [0; 4]);
}

#[test]
#[cfg(feature = "profiling")]
fn steady_actions_do_not_clone_prepared_graphs() {
    let (p, _) = attacks(false);
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    let input = InputFrame {
        pressed: ["atacar".into()].into(),
        ..Default::default()
    };
    rt.advance(FIXED_DT, &input);
    oxy_core::metrics::take();
    for _ in 0..10 {
        rt.advance(FIXED_DT, &input);
    }
    let work = oxy_core::metrics::take();
    assert_eq!(work.graph_nodes_copied, 0);
    assert_eq!(work.entity_scans, 0);
    assert!(work.actions > 0);
}
#[test]
fn active_area_keeps_hits_across_exit_reentry_and_releases_when_disabled() {
    let (p, id) = attacks(true);
    let target = p.scenes[0].entities[1].id.clone();
    let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    rt.scene_mut()
        .entity_mut(&target)
        .unwrap()
        .transform
        .position[0] = 10.;
    for _ in 0..10 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    rt.scene_mut()
        .entity_mut(&target)
        .unwrap()
        .transform
        .position[0] = 0.;
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        rt.scene().entity(&id).unwrap().attributes["Vida"],
        Value::Number(99990.)
    );
    rt.scene_mut()
        .entity_mut(&id)
        .unwrap()
        .collider
        .as_mut()
        .unwrap()
        .enabled = false;
    for _ in 0..5 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(rt.retained_counts(), [0, 0, 0, 0]);
    rt.scene_mut()
        .entity_mut(&id)
        .unwrap()
        .collider
        .as_mut()
        .unwrap()
        .enabled = true;
    rt.advance(FIXED_DT, &InputFrame::default());
    assert_eq!(
        rt.scene().entity(&id).unwrap().attributes["Vida"],
        Value::Number(99980.)
    );
    rt.remove_object(&id);
    assert_eq!(rt.retained_counts(), [0, 0, 0, 0]);
}
#[test]
fn refreshed_subtree_boxes_match_linear_reference_after_sequential_parent_moves() {
    let mut s = Scene::new("Sequential", SceneKind::ThreeD);
    let mut root = Entity::new("Root", None);
    root.transform.scale = [-1., 2., 1.];
    root.transform.rotation = [0.2, 0.3, 0.1];
    let id = root.id.clone();
    s.entities.push(root);
    for _ in 0..8 {
        let mut e = Entity::new("Child", None);
        e.parent = Some(id.clone());
        e.collider = Some(Collider::default());
        e.transform.pivot = [1., 0.2, 0.];
        s.entities.push(e);
    }
    let mut eval = SceneEvaluation::new(&s).unwrap();
    s.entities[0].transform.position = [3., 4., 5.];
    eval.refresh_subtree(&s, &id);
    for (i, e) in s.entities.iter().enumerate().skip(1) {
        let a = eval.boxes[i].unwrap();
        let b = oxy_core::spatial::collider_bounds(&s, &e.id).unwrap();
        assert!(a.min.abs_diff_eq(b.min, 1e-5) && a.max.abs_diff_eq(b.max, 1e-5));
    }
}
/// Both orders: the ids sort one way, so only scene order passes both.
const ORDERS: [[&str; 2]; 2] = [["primeiro", "segundo"], ["segundo", "primeiro"]];
fn marker_project(order: [&str; 2]) -> Project {
    let mut p = Project::new("Marcadores");
    for name in order {
        let mut e = Entity::new(name, None);
        e.id = name.into();
        let mut clip = Clip::new("Golpe");
        clip.events.push(AnimationEvent {
            time: 0.,
            name: "Impacto".into(),
        });
        let start = Node::new("event.scene_start", [0.; 2]);
        let mut play = Node::new("action.animation", [0.; 2]);
        play.params
            .insert("clip".into(), Value::Text(clip.id.clone()));
        let marker = Node::new("event.animation", [0.; 2]);
        let mut message = Node::new("debug.message", [0.; 2]);
        message
            .params
            .insert("message".into(), Value::Text(name.into()));
        edge(&mut e.graph, &start, "exec", &play, "exec");
        edge(&mut e.graph, &marker, "exec", &message, "exec");
        e.graph.nodes = vec![start, play, marker, message];
        e.clips.push(clip);
        p.scenes[0].entities.push(e);
    }
    p
}
#[test]
fn animation_markers_of_one_step_are_emitted_in_scene_order() {
    for order in ORDERS {
        let p = marker_project(order);
        for _ in 0..20 {
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            rt.advance(FIXED_DT, &InputFrame::default());
            let messages: Vec<_> = rt
                .logs
                .iter()
                .filter_map(|line| line.text.rsplit(' ').next())
                .collect();
            assert_eq!(messages, order, "{:?}", rt.logs);
        }
    }
}
fn standing_pair(order: [&str; 2]) -> Project {
    let mut p = Project::new("Apoio");
    let s = &mut p.scenes[0];
    s.kind = SceneKind::ThreeD;
    for name in order {
        let mut body = Entity::new(name, None);
        body.id = name.into();
        body.transform.position = [if name == "primeiro" { -2. } else { 2. }, 0.02, 0.];
        body.character3d = Some(CharacterConfig::default());
        s.entities.push(body);
    }
    let mut platform = Entity::new("Plataforma", Some(Primitive::Cube));
    platform.id = "plataforma".into();
    platform.transform.position = [0., -0.5, 0.];
    platform.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [20., 1., 20.],
        },
        ..Default::default()
    });
    s.entities.push(platform);
    p
}
/// A 3D level that fills the per-scene structures (all but the ready queue and released
/// keys, empty between steps, and pending look, with no camera rig to take it), and an empty
/// 2D menu to change to.
fn busy_level() -> (Project, Id) {
    let mut p = standing_pair(ORDERS[0]);
    let mut clock = Entity::new("Relógio", None);
    let clip = Clip {
        duration: 10.,
        ..Clip::new("Giro")
    };
    let start = Node::new("event.scene_start", [0.; 2]);
    let mut play = Node::new("action.animation", [0.; 2]);
    play.params
        .insert("clip".into(), Value::Text(clip.id.clone()));
    let mut wait = Node::new("control.wait", [0.; 2]);
    wait.params.insert("seconds".into(), Value::Number(100.));
    let late = Node::new("debug.message", [0.; 2]);
    edge(&mut clock.graph, &start, "exec", &play, "exec");
    edge(&mut clock.graph, &play, "exec", &wait, "exec");
    edge(&mut clock.graph, &wait, "exec", &late, "exec");
    clock.graph.nodes = vec![start, play, wait, late];
    clock.clips.push(clip);
    let mut broken = Entity::new("Sem atributo", None);
    let start = Node::new("event.scene_start", [0.; 2]);
    let set = Node::new("attribute.set", [0.; 2]);
    edge(&mut broken.graph, &start, "exec", &set, "exec");
    broken.graph.nodes = vec![start, set];
    let mut walker = Entity::new("Móvel", None);
    walker.controller = Some(Controller::default());
    let mut area = Entity::new("Área", None);
    area.transform.position = [10., 0., 0.];
    area.collider = Some(Collider {
        is_trigger: true,
        ..Default::default()
    });
    let mut contact = Entity::new("Contato", None);
    contact.transform.position = [10., 0., 0.];
    contact.collider = Some(Collider::default());
    // Draws a number and makes a copy on each step "segurar" is held: run state.
    let mould = Entity::new("Molde", None);
    let mut dice = Entity::new("Dado", None);
    let mut held = Node::new("event.input", [0.; 2]);
    held.params
        .insert("action".into(), Value::Text("segurar".into()));
    held.params
        .insert("mode".into(), Value::Text("held".into()));
    let draw = Node::new("random.number", [0.; 2]);
    let mut copy = Node::new("action.spawn", [0.; 2]);
    copy.params
        .insert("target".into(), Value::Object(Some(mould.id.clone())));
    edge(&mut dice.graph, &held, "exec", &draw, "exec");
    edge(&mut dice.graph, &draw, "exec", &copy, "exec");
    dice.graph.nodes = vec![held, draw, copy];
    p.scenes[0]
        .entities
        .extend([clock, broken, walker, area, contact, mould, dice]);
    let menu = Scene::new("Menu", SceneKind::TwoD);
    let id = menu.id.clone();
    p.scenes.push(menu);
    (p, id)
}
#[test]
fn change_scene_keeps_the_run_state_and_clears_every_per_scene_structure() {
    let (p, menu) = busy_level();
    let busy = || {
        let mut rt = Runtime::with_options(
            &p,
            &p.start_scene,
            RuntimeOptions {
                seed: 7,
                ..Default::default()
            },
        )
        .unwrap();
        let held = InputFrame {
            held: ["segurar".into()].into(),
            ..Default::default()
        };
        for _ in 0..3 {
            rt.advance(FIXED_DT, &held);
        }
        // One step in the game pause, which input logic still runs in, with the cursor shown.
        assert!(rt.set_game_paused(true));
        rt.set_cursor_shown(true);
        rt.advance(FIXED_DT, &held);
        rt.queue_timed_input(TimedInput {
            at: rt.time + 10.,
            change: InputChange::Press("pular".into()),
        })
        .unwrap();
        let input = InputFrame {
            pressed: ["pular".into()].into(),
            wheel: 1.,
            ..Default::default()
        };
        rt.advance(FIXED_DT * 0.5, &input);
        assert_eq!(
            rt.scene_state_in_use(),
            [
                "graphs",
                "input_timeline",
                "pending_pressed",
                "pending_wheel",
                "step_input",
                "characters",
                "cameras",
                "waiting",
                "bodies",
                "animations",
                "disabled_behaviors",
                "overlap_pairs",
                "area_activations",
                "damage_hits",
                "area_hits",
                "game_paused",
                "cursor_shown",
            ]
        );
        rt
    };
    let fresh = Runtime::new(&p, &p.start_scene).unwrap().run_state_debug();
    let mut rt = busy();
    let run = rt.run_state_debug();
    // The seed, four draws and four copies counted for the Dado's nodes (one in the pause),
    // and the step spent in the pause.
    assert!(
        run.contains("seed: 7")
            && run.matches("): 4").count() == 2
            && run.contains(&format!("paused_for: {}", f64::from(FIXED_DT))),
        "{run}"
    );
    assert_ne!(run, fresh);
    let (time, logs) = (rt.time, rt.logs.clone());
    assert!(!logs.is_empty());
    rt.change_scene(&menu).unwrap();
    assert_eq!(rt.scene_state_in_use(), Vec::<&str>::new());
    assert_eq!(rt.run_state_debug(), run);
    assert_eq!((rt.time, &rt.logs), (time, &logs));
    assert_eq!(rt.scene_starts, [menu]);
    let mut rt = busy();
    rt.stop();
    assert_eq!(rt.scene_state_in_use(), Vec::<&str>::new());
    assert_eq!(rt.run_state_debug(), fresh);
}
/// The draw and copy counts of copies made during the run go with them, removed or left in a
/// scene that changes; the project's own objects keep theirs, as a reload brings them back.
#[test]
fn copies_made_during_the_run_leave_no_draw_or_copy_counts_behind() {
    let mut p = Project::new("Fábrica");
    // Every Molde draws a number and copies the Faísca on each step; the Fábrica copies the
    // Molde.
    let spark = Entity::new("Faísca", None);
    let mut mould = Entity::new("Molde", None);
    let step = Node::new("event.step", [0.; 2]);
    let draw = Node::new("random.number", [0.; 2]);
    let mut copy = Node::new("action.spawn", [0.; 2]);
    copy.params
        .insert("target".into(), Value::Object(Some(spark.id.clone())));
    edge(&mut mould.graph, &step, "exec", &draw, "exec");
    edge(&mut mould.graph, &draw, "exec", &copy, "exec");
    mould.graph.nodes = vec![step, draw, copy];
    let mut factory = Entity::new("Fábrica", None);
    let step = Node::new("event.step", [0.; 2]);
    let mut copy = Node::new("action.spawn", [0.; 2]);
    copy.params
        .insert("target".into(), Value::Object(Some(mould.id.clone())));
    edge(&mut factory.graph, &step, "exec", &copy, "exec");
    factory.graph.nodes = vec![step, copy];
    let mould_id = mould.id.clone();
    p.scenes[0].entities.extend([spark, mould, factory]);
    let start = p.start_scene.clone();
    let mut rt = Runtime::new(&p, &start).unwrap();
    // One entry per (owner, node) counted.
    let entries = |rt: &Runtime| rt.run_state_debug().matches("): ").count();
    for _ in 0..300 {
        rt.advance(FIXED_DT, &InputFrame::default());
        let copies: Vec<_> = rt
            .scene()
            .entities
            .iter()
            .filter(|e| e.name.ends_with("(cópia)"))
            .map(|e| e.id.clone())
            .collect();
        assert!(!copies.is_empty());
        for id in copies {
            rt.remove_object(&id);
        }
        // The Molde's draws and copies, and the Fábrica's copies.
        assert_eq!(entries(&rt), 3, "{}", rt.run_state_debug());
    }
    assert!(rt.logs.is_empty(), "{:?}", rt.logs);
    for _ in 0..2 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert!(entries(&rt) > 3, "{}", rt.run_state_debug());
    rt.change_scene(&start).unwrap();
    assert_eq!(entries(&rt), 3, "{}", rt.run_state_debug());
    let kept = rt.run_state_debug();
    assert!(kept.contains("): 302"), "{kept}");
    rt.remove_object(&mould_id);
    assert_eq!(rt.run_state_debug(), kept);
}
#[test]
fn removing_a_shared_support_reports_departures_in_scene_order() {
    for order in ORDERS {
        let p = standing_pair(order);
        for _ in 0..20 {
            let mut rt = Runtime::new(&p, &p.start_scene).unwrap();
            for _ in 0..3 {
                rt.advance(FIXED_DT, &InputFrame::default());
            }
            for id in order {
                assert!(rt.character_state(id).unwrap().support.is_some());
            }
            rt.remove_object("plataforma");
            rt.advance(FIXED_DT, &InputFrame::default());
            let left: Vec<_> = rt
                .movement_records()
                .iter()
                .filter(|r| matches!(r.event, MovementEvent::LeftSupport))
                .map(|r| r.object.as_str())
                .collect();
            assert_eq!(left, order);
        }
    }
}
