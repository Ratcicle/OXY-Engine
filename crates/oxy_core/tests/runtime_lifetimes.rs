use oxy_core::{
    document::*,
    graph::{Edge, Graph, Node},
    runtime::{FIXED_DT, InputFrame, Runtime},
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
