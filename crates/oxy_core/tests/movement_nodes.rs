use glam::Vec3;
use oxy_core::{
    character::*,
    document::*,
    graph::*,
    movement_presets::*,
    physics3d::*,
    runtime::{FIXED_DT, InputFrame, Runtime, SceneQuery, SceneQueryShape},
};

fn fixture() -> (Project, Id) {
    let mut p = Project::new("Nós de movimento");
    p.scenes[0].kind = SceneKind::ThreeD;
    let scene = p.start_scene.clone();
    let id = create(
        &mut p,
        &scene,
        MovementPreset::FirstPerson,
        Vec3::new(0., 0.02, 0.),
    )
    .unwrap();
    let mut floor = Entity::new("Piso", Some(Primitive::Cube));
    floor.id = "floor".into();
    floor.transform.position = [0., -0.5, 0.];
    floor.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [50., 1., 50.],
        },
        ..Default::default()
    });
    p.scenes[0].entities.push(floor);
    (p, id)
}
fn node(op: &str, params: &[(&str, Value)]) -> Node {
    let mut n = Node::new(op, [0., 0.]);
    n.params
        .extend(params.iter().map(|(k, v)| ((*k).into(), v.clone())));
    n
}
fn wire(g: &mut Graph, a: &Node, out: &str, b: &Node, input: &str) {
    g.edges.push(Edge {
        from_node: a.id.clone(),
        from_port: out.into(),
        to_node: b.id.clone(),
        to_port: input.into(),
    });
}
fn ticks(r: &mut Runtime, n: usize) {
    for _ in 0..n {
        r.advance(FIXED_DT, &InputFrame::default());
    }
}
fn set(attr: &str) -> Node {
    node("attribute.set", &[("attribute", Value::Text(attr.into()))])
}
fn clean(r: &Runtime) {
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}

#[test]
fn typed_vectors_execute_and_reject_mismatched_ports() {
    let (mut p, id) = fixture();
    let start = node("event.scene_start", &[]);
    let vector = node(
        "vector3.make",
        &[("x", Value::Number(3.)), ("y", Value::Number(4.))],
    );
    let math = node("vector3.math", &[]);
    let write = set("Direção");
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.attributes
        .insert("Direção".into(), Value::Vector3([0.; 3]));
    e.graph.nodes = vec![start.clone(), vector.clone(), math.clone(), write.clone()];
    wire(&mut e.graph, &start, "exec", &write, "exec");
    wire(&mut e.graph, &vector, "value", &math, "a");
    wire(&mut e.graph, &math, "normalized", &write, "value");
    validate_project(&p).unwrap();
    let r = Runtime::new(&p, &p.start_scene).unwrap();
    let actual = r.scene().entity(&id).unwrap().attributes["Direção"]
        .vector3()
        .unwrap();
    assert!(actual.abs_diff_eq(Vec3::new(0.6, 0.8, 0.), 1e-6));
    clean(&r);
    let object_ids = p.scenes[0].entities.iter().map(|e| e.id.clone()).collect();
    let e = p.scenes[0].entity_mut(&id).unwrap();
    let scalar = node("value.number", &[]);
    e.graph.nodes.push(scalar.clone());
    assert!(
        e.graph
            .connect(
                &Edge {
                    from_node: scalar.id,
                    from_port: "value".into(),
                    to_node: math.id,
                    to_port: "b".into()
                },
                &object_ids
            )
            .is_err()
    );
}

#[test]
fn node_intent_is_renewed_per_step_and_native_velocity_is_equivalent() {
    let (mut p, id) = fixture();
    let event = node("event.step", &[]);
    let intent = node("character.intent", &[("axis", Value::Vector2([1., 0.]))]);
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.character3d.as_mut().unwrap().automatic_input = false;
    e.graph.nodes = vec![event.clone(), intent.clone()];
    wire(&mut e.graph, &event, "exec", &intent, "exec");
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 20);
    assert!(r.character_state(&id).unwrap().position.x > 0.5);
    r.scene_mut().entity_mut(&id).unwrap().graph = Graph::default();
    ticks(&mut r, 60);
    assert!(r.character_state(&id).unwrap().velocity.x.abs() < 0.01);
    clean(&r);
    let start = node("event.scene_start", &[]);
    let impulse = node(
        "character.velocity",
        &[("velocity", Value::Vector3([3., 7., 1.]))],
    );
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.graph.nodes = vec![start.clone(), impulse.clone()];
    e.graph.edges.clear();
    wire(&mut e.graph, &start, "exec", &impulse, "exec");
    let mut graph_runtime = Runtime::new(&p, &p.start_scene).unwrap();
    p.scenes[0].entity_mut(&id).unwrap().graph = Graph::default();
    let mut native = Runtime::new(&p, &p.start_scene).unwrap();
    native
        .add_character_velocity(&id, Vec3::new(3., 7., 1.))
        .unwrap();
    ticks(&mut graph_runtime, 20);
    ticks(&mut native, 20);
    assert!(
        graph_runtime
            .character_state(&id)
            .unwrap()
            .position
            .abs_diff_eq(native.character_state(&id).unwrap().position, 1e-6)
    );
    clean(&graph_runtime);
    clean(&native);
}

#[test]
fn landing_snapshot_survives_wait_and_attribute_reads_remain_live() {
    let (mut p, id) = fixture();
    let land = node("event.character", &[("kind", Value::Text("land".into()))]);
    let wait = node("control.wait", &[("seconds", Value::Number(0.1))]);
    let write = set("Impacto");
    let write_next = set("Cópia");
    let read = node(
        "attribute.get",
        &[("attribute", Value::Text("Impacto".into()))],
    );
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.transform.position[1] = 4.;
    e.attributes
        .insert("Impacto".into(), Value::Vector3([0.; 3]));
    e.attributes.insert("Cópia".into(), Value::Vector3([0.; 3]));
    e.graph.nodes = vec![
        land.clone(),
        wait.clone(),
        write.clone(),
        write_next.clone(),
        read.clone(),
    ];
    wire(&mut e.graph, &land, "exec", &wait, "exec");
    wire(&mut e.graph, &wait, "exec", &write, "exec");
    wire(&mut e.graph, &land, "velocity", &write, "value");
    wire(&mut e.graph, &write, "exec", &write_next, "exec");
    wire(&mut e.graph, &read, "value", &write_next, "value");
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    let mut landed = false;
    for _ in 0..90 {
        ticks(&mut r, 1);
        if r.movement_records()
            .iter()
            .any(|e| matches!(e.event, MovementEvent::Landed { .. }))
        {
            landed = true;
            break;
        }
    }
    assert!(landed);
    r.request_jump(&id).unwrap();
    ticks(&mut r, 8);
    let e = r.scene().entity(&id).unwrap();
    assert!(e.attributes["Impacto"].vector3().unwrap().y < -10.);
    assert_eq!(e.attributes["Impacto"], e.attributes["Cópia"]);
    assert!(r.character_state(&id).unwrap().velocity.y > 0.);
    clean(&r);
}

#[test]
fn teleport_node_cancels_old_sensor_exit_but_keeps_its_own_continuation() {
    let (mut p, id) = fixture();
    let mut area = Entity::new("Reinício fino", None);
    area.transform.position = [-1., 1., 0.];
    area.physics3d = Some(Collider3d {
        shape: CollisionShape::Box {
            size: [0.02, 3., 3.],
        },
        sensor: true,
        ..Default::default()
    });
    let enter = node("event.area_enter", &[]);
    let exit = node("event.area_exit", &[]);
    let teleport = node(
        "character.teleport",
        &[("position", Value::Vector3([10., 0.02, 0.]))],
    );
    let done = node(
        "debug.message",
        &[("message", Value::Text("teleporte concluído".into()))],
    );
    let stale = node(
        "debug.message",
        &[("message", Value::Text("trajetória antiga".into()))],
    );
    area.graph.nodes = vec![
        enter.clone(),
        exit.clone(),
        teleport.clone(),
        done.clone(),
        stale.clone(),
    ];
    wire(&mut area.graph, &enter, "exec", &teleport, "exec");
    wire(&mut area.graph, &enter, "context", &teleport, "target");
    wire(&mut area.graph, &teleport, "exec", &done, "exec");
    wire(&mut area.graph, &exit, "exec", &stale, "exec");
    p.scenes[0].entities.push(area);
    validate_project(&p).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    r.set_character_velocity(&id, Vec3::new(-120., 0., 0.))
        .unwrap();
    ticks(&mut r, 1);
    assert!(r.character_state(&id).unwrap().position.x > 9.9);
    assert_eq!(r.logs.len(), 1, "{:?}", r.logs);
    assert!(r.logs[0].contains("teleporte concluído"));
}

#[test]
fn physical_query_node_matches_native_and_space_has_real_contact() {
    let (mut p, id) = fixture();
    let start = node("event.scene_start", &[]);
    let query = node(
        "query.ray",
        &[
            ("origin", Value::Vector3([4., 5., 0.])),
            ("direction", Value::Vector3([0., -1., 0.])),
        ],
    );
    let write = set("Ponto");
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.attributes.insert("Ponto".into(), Value::Vector3([0.; 3]));
    e.graph.nodes = vec![start.clone(), query.clone(), write.clone()];
    wire(&mut e.graph, &start, "exec", &query, "exec");
    wire(&mut e.graph, &query, "exec", &write, "exec");
    wire(&mut e.graph, &query, "point", &write, "value");
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    let mut request = SceneQuery {
        shape: SceneQueryShape::Ray,
        origin: Vec3::new(4., 5., 0.),
        direction: -Vec3::Y,
        distance: 10.,
        include_sensors: false,
        exclude: Some(id.clone()),
        category: 1,
        mask: u32::MAX,
    };
    let hit = r.query_scene(&request).unwrap().unwrap();
    assert_eq!(hit.object, "floor");
    assert!(hit.normal.abs_diff_eq(Vec3::Y, 1e-6));
    assert_eq!(
        r.scene().entity(&id).unwrap().attributes["Ponto"]
            .vector3()
            .unwrap(),
        hit.point
    );
    request.shape = SceneQueryShape::Space {
        height: 1.8,
        radius: 0.3,
    };
    request.origin = Vec3::new(4., 0.8, 0.);
    let hit = r.query_scene(&request).unwrap().unwrap();
    assert!(hit.point.y.abs() < 1e-5);
    assert!(hit.normal.abs_diff_eq(Vec3::Y, 1e-5));
    assert!(hit.distance < 0.);
    r.scene_mut()
        .entity_mut("floor")
        .unwrap()
        .physics3d
        .as_mut()
        .unwrap()
        .enabled = false;
    assert!(r.query_scene(&request).unwrap().is_none());
    for _ in 0..252 {
        r.query_scene(&request).unwrap();
    }
    assert!(r.query_scene(&request).is_err());
    ticks(&mut r, 1);
    assert!(r.query_scene(&request).is_ok());
    clean(&r);
}

#[test]
fn connected_null_object_does_not_fall_back_to_graph_owner() {
    let (mut p, id) = fixture();
    let start = node("event.scene_start", &[]);
    let null = node("value.object", &[]);
    let remove = node("action.remove", &[]);
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.graph.nodes = vec![start.clone(), null.clone(), remove.clone()];
    wire(&mut e.graph, &start, "exec", &remove, "exec");
    wire(&mut e.graph, &null, "value", &remove, "target");
    let r = Runtime::new(&p, &p.start_scene).unwrap();
    assert!(r.scene().entity(&id).is_some());
    assert_eq!(r.logs.len(), 1);
}

#[test]
fn optional_presets_undo_redo_and_model_copy_keep_bindings_and_internal_links() {
    use oxy_core::{history::CommandHistory, texture_cache::TextureCache};
    let mut p = Project::new("Vazio");
    p.scenes[0].kind = SceneKind::ThreeD;
    assert!(p.scenes[0].entities.is_empty());
    p.input_bindings.insert("pular".into(), "J".into());
    let base = p.clone();
    let mut h = CommandHistory::new();
    let mut images = TextureCache::default();
    h.begin("Criar personagem", &p, &images);
    let scene = p.start_scene.clone();
    let id = create(&mut p, &scene, MovementPreset::ThirdPerson, Vec3::ZERO).unwrap();
    h.commit(&p, &mut images).unwrap();
    let after = p.clone();
    assert_eq!(p.input_bindings["pular"], "J");
    h.undo(&mut p, &mut images).unwrap();
    assert_eq!(p, base);
    h.redo(&mut p, &mut images).unwrap();
    assert_eq!(p, after);
    assert_eq!(h.undo_len(), 1);
    let asset = p.save_model(&scene, &id, "Personagem").unwrap();
    let copy = p.instantiate_model(&asset, &scene).unwrap();
    let descendants = p.scenes[0].descendants(&copy);
    let rig = descendants
        .iter()
        .find_map(|id| p.scenes[0].entity(id).unwrap().camera_rig.as_ref())
        .unwrap();
    assert_eq!(rig.target.as_ref(), Some(&copy));
    assert!(rig.hidden.iter().all(|id| descendants.contains(id)));
    validate_project(&p).unwrap();
}
