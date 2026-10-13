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
fn a_query_right_after_logic_turns_a_collider_off_does_not_hit_it() {
    let (mut p, id) = fixture();
    p.input_bindings.insert("consultar".into(), "Q".into());
    // In one run of the logic: a ray at the floor, the floor off, the ray again, the floor on,
    // the ray once more.
    let input = node(
        "event.input",
        &[("action", Value::Text("consultar".into()))],
    );
    let switch = |enabled| {
        node(
            "action.component",
            &[
                ("target", Value::Object(Some("floor".into()))),
                ("enabled", Value::Bool(enabled)),
            ],
        )
    };
    let (off, on) = (switch(false), switch(true));
    let e = p.scenes[0].entity_mut(&id).unwrap();
    e.graph.nodes = vec![input.clone(), off.clone(), on.clone()];
    let mut previous = input;
    for (attribute, then) in [
        ("Antes", Some(off)),
        ("Desligado", Some(on)),
        ("Religado", None),
    ] {
        let query = node(
            "query.ray",
            &[
                ("origin", Value::Vector3([4., 5., 0.])),
                ("direction", Value::Vector3([0., -1., 0.])),
            ],
        );
        let write = set(attribute);
        e.attributes.insert(attribute.into(), Value::Bool(false));
        wire(&mut e.graph, &previous, "exec", &query, "exec");
        wire(&mut e.graph, &query, "exec", &write, "exec");
        wire(&mut e.graph, &query, "hit", &write, "value");
        previous = write.clone();
        if let Some(then) = then {
            wire(&mut e.graph, &write, "exec", &then, "exec");
            previous = then;
        }
        e.graph.nodes.extend([query, write]);
    }
    validate_project(&p).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 10);
    assert!(r.physics_world().is_some());
    r.advance(
        FIXED_DT,
        &InputFrame {
            pressed: ["consultar".into()].into(),
            ..Default::default()
        },
    );
    let attributes = &r.scene().entity(&id).unwrap().attributes;
    assert_eq!(
        [
            &attributes["Antes"],
            &attributes["Desligado"],
            &attributes["Religado"]
        ],
        [&Value::Bool(true), &Value::Bool(false), &Value::Bool(true)]
    );
    assert!(r.character_state(&id).unwrap().grounded);
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
    let camera = p.scenes[0]
        .entities
        .iter()
        .find(|e| e.camera_rig.is_some())
        .unwrap();
    assert!(camera.parent.is_none());
    assert_eq!(
        camera.camera_rig.as_ref().unwrap().target.as_ref(),
        Some(&id)
    );
    let camera_id = camera.id.clone();
    let asset = p.save_model(&scene, &id, "Personagem").unwrap();
    let copy = p.instantiate_model(&asset, &scene).unwrap();
    let descendants = p.scenes[0].descendants(&copy);
    // Saving a character hierarchy no longer silently saves a camera with it.
    assert!(
        descendants
            .iter()
            .all(|id| p.scenes[0].entity(id).unwrap().camera.is_none())
    );
    assert_eq!(
        p.scenes[0]
            .entity(&camera_id)
            .unwrap()
            .camera_rig
            .as_ref()
            .unwrap()
            .target
            .as_ref(),
        Some(&id)
    );
    // An explicit group containing both objects still serializes/remaps their link.
    let assembly =
        oxy_core::editing::group_selection(&mut p.scenes[0], &[id.clone(), camera_id.clone()])
            .unwrap();
    let asset = p
        .save_model(&scene, &assembly, "Montagem completa")
        .unwrap();
    let copy = p.instantiate_model(&asset, &scene).unwrap();
    let descendants = p.scenes[0].descendants(&copy);
    let rig = descendants
        .iter()
        .find_map(|id| p.scenes[0].entity(id).unwrap().camera_rig.as_ref())
        .unwrap();
    assert!(descendants.contains(rig.target.as_ref().unwrap()));
    assert_ne!(rig.target.as_ref(), Some(&id));
    assert!(rig.hidden.iter().all(|id| descendants.contains(id)));
    validate_project(&p).unwrap();
}

#[test]
fn independent_camera_presets_roundtrip_and_target_motion_is_applied_once() {
    for preset in [MovementPreset::FirstPerson, MovementPreset::ThirdPerson] {
        let mut p = Project::new("Alvo explícito");
        p.scenes[0].kind = SceneKind::ThreeD;
        let scene = p.start_scene.clone();
        let player = create(&mut p, &scene, preset, Vec3::new(4., 0.02, -3.)).unwrap();
        p.scenes[0].entity_mut(&player).unwrap().transform.position[0] += 7.;
        let camera = p.scenes[0]
            .entities
            .iter()
            .find(|e| e.camera.is_some())
            .unwrap();
        let id = camera.id.clone();
        let rig = camera.camera_rig.clone().unwrap();
        assert!(camera.parent.is_none());
        assert!(p.scenes[0].entity(&player).unwrap().camera.is_none());
        let bytes = serde_json::to_vec(&p).unwrap();
        let reopened = oxy_core::migration::read(&bytes).unwrap();
        assert_eq!(p, reopened);
        let mut r = Runtime::new(&reopened, &scene).unwrap();
        ticks(&mut r, 3);
        assert_eq!(r.active_camera(), Some(id.as_str()));
        r.set_camera_mode(&id, CameraMode::FirstPerson, 0.).unwrap();
        ticks(&mut r, 2);
        let pose = r.game_camera_pose().unwrap();
        assert!(
            (pose.position.x - 11.).abs() < 0.0001,
            "camera/parent counted twice: {pose:?}"
        );
        r.advance(
            oxy_core::runtime::FIXED_DT,
            &InputFrame {
                pressed: [rig.mode_action.clone()].into(),
                ..Default::default()
            },
        );
        assert_eq!(r.camera_mode(&id), Some(CameraMode::ThirdPerson));
        ticks(&mut r, 45);
        let before = r.game_camera_pose().unwrap().position;
        r.advance(
            oxy_core::runtime::FIXED_DT,
            &InputFrame {
                pressed: [rig.shoulder_action.clone()].into(),
                ..Default::default()
            },
        );
        ticks(&mut r, 45);
        assert!(
            (r.game_camera_pose().unwrap().position.x - before.x).abs() > 0.1,
            "shoulder did not change"
        );
        let yaw = r.character_state(&player).unwrap().look_yaw;
        r.advance(
            oxy_core::runtime::FIXED_DT,
            &InputFrame {
                look: [120., -20.],
                ..Default::default()
            },
        );
        assert_ne!(r.character_state(&player).unwrap().look_yaw, yaw);
        r.advance(
            oxy_core::runtime::FIXED_DT,
            &InputFrame {
                pressed: [rig.mode_action].into(),
                ..Default::default()
            },
        );
        assert_eq!(r.camera_mode(&id), Some(CameraMode::FirstPerson));
        r.stop();
        assert_eq!(serde_json::to_vec(&p).unwrap(), bytes);
        clean(&r);
    }
}

#[test]
fn published_movement_projects_have_camera_roots_and_explicit_targets() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let paths = (6..=12)
        .map(|i| root.join(format!("guia/{i:02}-movimento-3d")))
        .chain([root.join("laboratorio-3d")]);
    for path in paths {
        let p = oxy_core::persistence::load_project(&path).unwrap();
        for scene in &p.scenes {
            for e in &scene.entities {
                if let Some(rig) = &e.camera_rig {
                    assert!(
                        e.camera.is_some() && e.character3d.is_none() && e.parent.is_none(),
                        "{}",
                        path.display()
                    );
                    assert!(
                        rig.target
                            .as_deref()
                            .and_then(|id| scene.entity(id))
                            .is_some_and(|e| e.character3d.is_some())
                    );
                }
            }
        }
    }
}

fn text(value: &str) -> Value {
    Value::Text(value.into())
}
fn pressing(action: &str) -> InputFrame {
    InputFrame {
        pressed: [action.into()].into(),
        ..Default::default()
    }
}
/// Mover / girar / escalar objeto with these choices and value.
fn write(target: Option<&str>, channel: &str, mode: &str, space: &str, value: [f32; 3]) -> Node {
    node(
        "transform.write",
        &[
            ("target", Value::Object(target.map(str::to_owned))),
            ("channel", text(channel)),
            ("mode", text(mode)),
            ("space", text(space)),
            ("value", Value::Vector3(value)),
        ],
    )
}
/// Peça, under a group with transform `group`, as one write moves it when the scene starts:
/// the group's world matrix and Peça's transform before and after, or the error logged.
fn moved(
    group: &Transform,
    piece: &Transform,
    channel: &str,
    mode: &str,
    space: &str,
    value: [f32; 3],
) -> Result<(glam::Mat4, Transform, Transform), String> {
    let mut p = Project::new("Transformações");
    p.scenes[0].kind = SceneKind::ThreeD;
    let mut parent = Entity::new("Grupo", None);
    parent.transform = group.clone();
    let mut child = Entity::new("Peça", Some(Primitive::Cube));
    child.parent = Some(parent.id.clone());
    child.transform = piece.clone();
    let start = node("event.scene_start", &[]);
    let write = write(Some(&child.id), channel, mode, space, value);
    wire(&mut parent.graph, &start, "exec", &write, "exec");
    parent.graph.nodes = vec![start, write];
    let id = child.id.clone();
    p.scenes[0].entities = vec![parent, child];
    validate_project(&p).unwrap();
    let r = Runtime::new(&p, &p.start_scene).unwrap();
    match r.logs.first() {
        Some(error) => Err(error.clone()),
        None => Ok((
            group.matrix(),
            piece.clone(),
            r.scene().entity(&id).unwrap().transform.clone(),
        )),
    }
}

#[test]
fn transform_write_sets_and_adds_each_part_in_the_world_and_under_the_parent() {
    use glam::{EulerRot, Mat4, Quat};
    let node = Node::new("transform.write", [0.; 2]);
    assert_eq!(
        [node.text("channel"), node.text("mode"), node.text("space")],
        ["position", "add", "local"]
    );
    let degrees = |v: [f32; 3]| v.map(f32::to_radians);
    let piece = Transform {
        position: [1., 0.5, -1.],
        rotation: degrees([0., 20., 0.]),
        scale: [1., 1.5, 1.],
        pivot: [0., 0.25, 0.],
    };
    let turned = Transform {
        position: [1., 2., 3.],
        rotation: degrees([30., 90., 0.]),
        scale: [2.; 3],
        ..Default::default()
    };
    let stretched = Transform {
        position: [0., 1., 0.],
        scale: [2., 1., 0.5],
        ..Default::default()
    };
    // Where the pivot is in the world, and the world rotation.
    let pivot = |parent: Mat4, t: &Transform| parent.transform_point3(Vec3::from(t.position));
    let rotation = |parent: Mat4, t: &Transform| world_pose(parent * t.matrix()).unwrap().1;
    let near = |a: Quat, b: Quat| a.dot(b).abs() > 1. - 1e-6;
    let turn = Quat::from_euler(
        EulerRot::XYZ,
        10f32.to_radians(),
        30f32.to_radians(),
        -20f32.to_radians(),
    );
    for group in [&Transform::default(), &turned, &stretched] {
        for space in ["local", "world"] {
            for mode in ["set", "add"] {
                let v = [0.5, -2., 3.];
                let (parent, before, after) =
                    moved(group, &piece, "position", mode, space, v).unwrap();
                assert_eq!(
                    (after.rotation, after.scale),
                    (before.rotation, before.scale)
                );
                let expected = match (space, mode) {
                    ("local", "set") => Vec3::from(v),
                    ("local", _) => Vec3::from(before.position) + Vec3::from(v),
                    (_, "set") => Vec3::from(v),
                    _ => pivot(parent, &before) + Vec3::from(v),
                };
                let found = if space == "local" {
                    Vec3::from(after.position)
                } else {
                    pivot(parent, &after)
                };
                assert!(found.abs_diff_eq(expected, 1e-4), "{space} {mode}: {found}");

                let v = [10., 30., -20.];
                let result = moved(group, &piece, "rotation", mode, space, v);
                if space == "local" {
                    let (_, before, after) = result.unwrap();
                    assert_eq!(
                        (after.position, after.scale),
                        (before.position, before.scale)
                    );
                    let expected: [f32; 3] = match mode {
                        "set" => degrees(v),
                        _ => std::array::from_fn(|i| before.rotation[i] + degrees(v)[i]),
                    };
                    assert_eq!(after.rotation, expected);
                } else if group == &stretched {
                    // Turned under a group scaled unevenly, Peça would shear.
                    let error = result.unwrap_err();
                    assert!(
                        error.contains("Peça não pode girar no espaço Mundo")
                            && error.contains("escala não uniforme"),
                        "{error}"
                    );
                } else {
                    let (parent, before, after) = result.unwrap();
                    assert_eq!(
                        (after.position, after.scale),
                        (before.position, before.scale)
                    );
                    let expected = match mode {
                        "set" => turn,
                        _ => turn * rotation(parent, &before),
                    };
                    assert!(
                        near(rotation(parent, &after), expected),
                        "{mode}: {after:?}"
                    );
                }

                let v = [2., 0.5, 3.];
                let result = moved(group, &piece, "scale", mode, space, v);
                if space == "world" {
                    let error = result.unwrap_err();
                    assert!(error.contains("A escala de Peça só muda no espaço Local"));
                    continue;
                }
                let (_, before, after) = result.unwrap();
                assert_eq!(
                    (after.position, after.rotation),
                    (before.position, before.rotation)
                );
                let expected = match mode {
                    "set" => Vec3::from(v),
                    _ => Vec3::from(before.scale) * Vec3::from(v),
                };
                assert_eq!(Vec3::from(after.scale), expected);
            }
        }
    }
    // A turn that keeps the axes on the axes is fine under the stretched group.
    let (parent, _, after) = moved(
        &stretched,
        &piece,
        "rotation",
        "set",
        "world",
        [0., 90., 0.],
    )
    .unwrap();
    assert!(near(
        rotation(parent, &after),
        Quat::from_rotation_y(90f32.to_radians())
    ));
    let error = moved(&turned, &piece, "scale", "add", "local", [1., 0., 1.]).unwrap_err();
    assert!(
        error.contains("A escala de Peça chegaria a zero"),
        "{error}"
    );
}

#[test]
fn a_mirrored_object_or_group_turns_in_the_world_like_any_other() {
    use glam::{EulerRot, Mat3, Mat4, Quat};
    let degrees = |v: [f32; 3]| v.map(f32::to_radians);
    let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4);
    // The object's axes lie on the rotation's, whichever of them a mirror reverses.
    let aligned = |world: Mat4, rotation: Quat| {
        let axes = Mat3::from_quat(rotation).transpose() * Mat3::from_mat4(world);
        let size = axes.to_cols_array().map(f32::abs);
        let largest = size.iter().copied().fold(0., f32::max);
        [1, 2, 3, 5, 6, 7]
            .iter()
            .all(|&i| size[i] <= 1e-4 * largest)
    };
    let v = [10., 30., -20.];
    let [x, y, z] = degrees(v);
    let wanted = Quat::from_euler(EulerRot::XYZ, x, y, z);
    // A root mirrored on any axis: the mirror stays in the scale and the angles are the ones
    // Local writes.
    for scale in [[1., -1., 1.], [1., 1., -1.], [-1., 1., 1.]] {
        let mut p = Project::new("Espelho");
        p.scenes[0].kind = SceneKind::ThreeD;
        let mut mirror = Entity::new("Espelho", Some(Primitive::Cube));
        mirror.transform.rotation = degrees([0., 20., 0.]);
        mirror.transform.scale = scale;
        let start = node("event.scene_start", &[]);
        let turn = write(None, "rotation", "set", "world", v);
        let spin = write(None, "rotation", "add", "world", [0., 30., 0.]);
        wire(&mut mirror.graph, &start, "exec", &turn, "exec");
        wire(&mut mirror.graph, &turn, "exec", &spin, "exec");
        mirror.graph.nodes = vec![start, turn, spin];
        let id = mirror.id.clone();
        p.scenes[0].entities.push(mirror);
        let r = Runtime::new(&p, &p.start_scene).unwrap();
        clean(&r);
        let after = r.scene().entity(&id).unwrap().transform.clone();
        let expected = Quat::from_rotation_y(30f32.to_radians()) * wanted;
        assert!(
            after.quaternion().dot(expected).abs() > 1. - 1e-6,
            "{after:?}"
        );
        assert!(aligned(after.matrix(), expected), "{scale:?}");
        assert_eq!(after.scale, scale);
        let piece = Transform {
            rotation: degrees([0., 20., 0.]),
            scale,
            ..Default::default()
        };
        let (_, _, after) =
            moved(&Transform::default(), &piece, "rotation", "set", "world", v).unwrap();
        assert!(close(after.rotation, degrees(v)), "{scale:?}: {after:?}");
        let (_, _, after) = moved(
            &Transform::default(),
            &piece,
            "rotation",
            "add",
            "world",
            [0., 30., 0.],
        )
        .unwrap();
        assert!(
            close(after.rotation, degrees([0., 50., 0.])),
            "{scale:?}: {after:?}"
        );
    }
    // Under a mirrored group, it reaches the rotation the world reads back.
    let piece = Transform {
        position: [1., 0.5, -1.],
        rotation: degrees([0., 20., 0.]),
        scale: [1., 1.5, 1.],
        pivot: [0., 0.25, 0.],
    };
    for scale in [[1., -1., 1.], [2., 2., -2.]] {
        let group = Transform {
            position: [1., 2., 3.],
            rotation: degrees([30., 40., 10.]),
            scale,
            ..Default::default()
        };
        for mode in ["set", "add"] {
            let (parent, before, after) = moved(&group, &piece, "rotation", mode, "world", v)
                .unwrap_or_else(|error| panic!("{scale:?} {mode}: {error}"));
            let read = |t: &Transform| world_pose(parent * t.matrix()).unwrap().1;
            let expected = match mode {
                "set" => wanted,
                _ => wanted * read(&before),
            };
            assert!(
                aligned(parent * after.matrix(), expected),
                "{scale:?} {mode}"
            );
            assert!(
                read(&after).dot(expected).abs() > 1. - 1e-5,
                "{scale:?} {mode}: {after:?}"
            );
            assert_eq!(
                (after.position, after.scale),
                (before.position, before.scale)
            );
        }
    }
}

#[test]
fn an_object_with_a_pivot_reads_and_writes_the_same_world_position() {
    let mut p = Project::new("Porta");
    p.scenes[0].kind = SceneKind::ThreeD;
    let mut group = Entity::new("Grupo", None);
    group.transform = Transform {
        position: [1., 2., 3.],
        rotation: [0.3, 0.5, 0.],
        scale: [2.; 3],
        ..Default::default()
    };
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    door.transform = Transform {
        position: [5., 0., 0.],
        rotation: [0., 0.4, 0.2],
        scale: [1., 2., 1.],
        pivot: [0., 1., 0.],
    };
    let mut hatch = door.clone();
    hatch.id = "alcapao".into();
    hatch.name = "Alçapão".into();
    hatch.parent = Some(group.id.clone());
    // Each one keeps where it reads itself, and the place it reads in both spaces.
    for e in [&mut door, &mut hatch] {
        for space in ["Mundo", "Local"] {
            e.attributes.insert(space.into(), Value::Vector3([0.; 3]));
        }
        let start = node("event.scene_start", &[]);
        let world = node("transform.read", &[("space", text("world"))]);
        let local = node("transform.read", &[("space", text("local"))]);
        let keep_world = set("Mundo");
        let keep_local = set("Local");
        let place = write(None, "position", "set", "world", [0.; 3]);
        wire(&mut e.graph, &start, "exec", &keep_world, "exec");
        wire(&mut e.graph, &keep_world, "exec", &keep_local, "exec");
        wire(&mut e.graph, &keep_local, "exec", &place, "exec");
        wire(&mut e.graph, &world, "position", &keep_world, "value");
        wire(&mut e.graph, &local, "position", &keep_local, "value");
        wire(&mut e.graph, &world, "position", &place, "value");
        e.graph.nodes = vec![start, world, local, keep_world, keep_local, place];
    }
    let parent = group.transform.matrix();
    let (door_id, hatch_id) = (door.id.clone(), hatch.id.clone());
    p.scenes[0].entities = vec![group, door, hatch];
    validate_project(&p).unwrap();
    let r = Runtime::new(&p, &p.start_scene).unwrap();
    clean(&r);
    let read = |id: &str, space: &str| {
        r.scene().entity(id).unwrap().attributes[space]
            .vector3()
            .unwrap()
    };
    // At the root, Mundo and Local are one space.
    assert!(read(&door_id, "Mundo").abs_diff_eq(Vec3::new(5., 0., 0.), 1e-5));
    assert!(read(&door_id, "Local").abs_diff_eq(Vec3::new(5., 0., 0.), 1e-5));
    let pivot = parent.transform_point3(Vec3::new(5., 0., 0.));
    assert!(
        read(&hatch_id, "Mundo").abs_diff_eq(pivot, 1e-4),
        "{}",
        read(&hatch_id, "Mundo")
    );
    for id in [&door_id, &hatch_id] {
        let position = Vec3::from(r.scene().entity(id).unwrap().transform.position);
        assert!(
            position.abs_diff_eq(Vec3::new(5., 0., 0.), 1e-4),
            "{position}"
        );
    }
}

#[test]
fn adding_a_turn_every_step_stays_on_its_axis_and_wraps_past_a_full_turn() {
    let mut p = Project::new("Hélice");
    p.scenes[0].kind = SceneKind::ThreeD;
    let mut fan = Entity::new("Hélice", Some(Primitive::Cube));
    let step = node("event.step", &[]);
    let turn = write(None, "rotation", "add", "local", [0., 7., 0.]);
    wire(&mut fan.graph, &step, "exec", &turn, "exec");
    fan.graph.nodes = vec![step, turn];
    let id = fan.id.clone();
    p.scenes[0].entities.push(fan);
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 60);
    // 420° comes back as 60°, with X and Z untouched.
    let rotation = r.scene().entity(&id).unwrap().transform.rotation;
    assert_eq!([rotation[0], rotation[2]], [0., 0.]);
    assert!(
        (rotation[1] - 60f32.to_radians()).abs() < 1e-4,
        "{}",
        rotation[1].to_degrees()
    );
    clean(&r);
}

/// Hélice, which a looping clip of its own moves up and down from the scene start.
fn bobbing_fan() -> Entity {
    use oxy_core::animation::{Clip, Interpolation, Keyframe};
    let mut fan = Entity::new("Hélice", Some(Primitive::Cube));
    fan.transform.position = [5., 3., 0.];
    let mut clip = Clip {
        duration: 1.,
        looping: true,
        ..Clip::new("Sobe e desce")
    };
    for (time, y) in [(0., 3.), (0.5, 4.), (1., 3.)] {
        let mut transform = fan.transform.clone();
        transform.position[1] = y;
        clip.insert_key(
            &fan.id,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    let start = node("event.scene_start", &[]);
    let play = node("action.animation", &[("clip", text(&clip.id))]);
    wire(&mut fan.graph, &start, "exec", &play, "exec");
    fan.graph.nodes = vec![start, play];
    fan.clips.push(clip);
    fan
}

#[test]
fn transform_write_refuses_a_character_and_what_a_playing_clip_poses() {
    let (mut p, player) = fixture();
    p.input_bindings.insert("mexer".into(), "M".into());
    p.input_bindings.insert("parar".into(), "P".into());
    let fan = bobbing_fan();
    let fan_id = fan.id.clone();
    // One controller per write: a refusal stops the logic of its object.
    let controller = |name: &str, action: &str, nodes: Vec<Node>| {
        let mut e = Entity::new(name, None);
        let input = node("event.input", &[("action", text(action))]);
        let mut previous = input.clone();
        for next in &nodes {
            wire(&mut e.graph, &previous, "exec", next, "exec");
            previous = next.clone();
        }
        e.graph.nodes = [input].into_iter().chain(nodes).collect();
        e
    };
    let entities = [
        controller(
            "Empurra jogador",
            "mexer",
            vec![write(
                Some(&player),
                "position",
                "add",
                "world",
                [1., 0., 0.],
            )],
        ),
        controller(
            "Empurra hélice",
            "mexer",
            vec![write(
                Some(&fan_id),
                "position",
                "add",
                "world",
                [1., 0., 0.],
            )],
        ),
        controller(
            "Para e move hélice",
            "parar",
            vec![
                node(
                    "action.component",
                    &[
                        ("target", Value::Object(Some(fan_id.clone()))),
                        ("component", text("animation")),
                        ("enabled", Value::Bool(false)),
                    ],
                ),
                write(Some(&fan_id), "position", "set", "world", [5., 6., 0.]),
            ],
        ),
    ];
    p.scenes[0].entities.push(fan);
    p.scenes[0].entities.extend(entities);
    validate_project(&p).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 10);
    let player_at = r.character_state(&player).unwrap().position;
    r.advance(FIXED_DT, &pressing("mexer"));
    assert_eq!(r.logs.len(), 2, "{:?}", r.logs);
    assert!(
        r.logs[0].contains(
            "Jogador é um personagem 3D: mova-o com Teleportar com segurança e gire-o com \
             Definir giro horizontal do personagem"
        ),
        "{:?}",
        r.logs
    );
    assert!(
        r.logs[1].contains(
            "Hélice é movido pela animação Sobe e desce, que está tocando: pause-a com Ativar / \
             desativar componente (Reprodução da animação) ou mova um grupo acima dele"
        ),
        "{:?}",
        r.logs
    );
    assert!(
        r.character_state(&player)
            .unwrap()
            .position
            .abs_diff_eq(player_at, 1e-3)
    );
    // Paused, the clip leaves the place logic gives it.
    r.advance(FIXED_DT, &pressing("parar"));
    ticks(&mut r, 30);
    assert_eq!(
        r.scene().entity(&fan_id).unwrap().transform.position,
        [5., 6., 0.]
    );
    assert_eq!(r.logs.len(), 2, "{:?}", r.logs);
}

#[test]
fn a_finished_clip_no_longer_undoes_a_later_write() {
    use oxy_core::animation::{Clip, Interpolation, Keyframe};
    let mut p = Project::new("Porta");
    p.scenes[0].kind = SceneKind::ThreeD;
    p.input_bindings.insert("mexer".into(), "M".into());
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    let mut clip = Clip {
        duration: 0.25,
        ..Clip::new("Abrir")
    };
    for (time, y) in [(0., 0.), (0.25, 2.)] {
        let mut transform = door.transform.clone();
        transform.position[1] = y;
        clip.insert_key(
            &door.id,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    let start = node("event.scene_start", &[]);
    let play = node("action.animation", &[("clip", text(&clip.id))]);
    let input = node("event.input", &[("action", text("mexer"))]);
    let lift = write(None, "position", "set", "local", [0., 5., 0.]);
    wire(&mut door.graph, &start, "exec", &play, "exec");
    wire(&mut door.graph, &input, "exec", &lift, "exec");
    door.graph.nodes = vec![start, play, input, lift];
    door.clips.push(clip);
    let id = door.id.clone();
    p.scenes[0].entities.push(door);
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 30);
    let at = |r: &Runtime| r.scene().entity(&id).unwrap().transform.position;
    assert_eq!(at(&r), [0., 2., 0.]);
    r.advance(FIXED_DT, &pressing("mexer"));
    ticks(&mut r, 30);
    assert_eq!(at(&r), [0., 5., 0.]);
    clean(&r);
}

/// A 3D character standing on a platform that follows its animation, which has none: only
/// its logic moves it. The project, the platform and the character.
fn rider() -> (Project, Id, Id) {
    use oxy_core::surface::{PlatformMode, TranslationPlatform};
    let mut p = Project::new("Plataforma movida pela lógica");
    p.scenes[0].kind = SceneKind::ThreeD;
    let solid = |name: &str, size: [f32; 3], position: [f32; 3]| {
        let mut e = Entity::new(name, Some(Primitive::Cube));
        e.dimensions = size;
        e.transform.position = position;
        e.physics3d = Some(Collider3d {
            shape: CollisionShape::Box { size },
            ..Default::default()
        });
        e
    };
    let mut lift = solid("Plataforma", [6., 0.5, 6.], [0., 1.75, 0.]);
    lift.platform = Some(TranslationPlatform {
        mode: PlatformMode::Animation,
        ..Default::default()
    });
    let mut body = Entity::new("Passageiro", None);
    body.transform.position = [0., 2.02, 0.];
    body.character3d = Some(CharacterConfig::default());
    let ids = (lift.id.clone(), body.id.clone());
    p.scenes[0].entities = vec![solid("Chão", [100., 1., 100.], [0., -0.5, 0.]), lift, body];
    (p, ids.0, ids.1)
}

#[test]
fn a_platform_logic_moves_every_step_carries_its_rider() {
    let (mut p, lift, body) = rider();
    p.input_bindings.insert("empurrar".into(), "E".into());
    let held = node(
        "event.input",
        &[("action", text("empurrar")), ("mode", text("held"))],
    );
    // 2 m/s along X.
    let push = write(None, "position", "add", "world", [2. * FIXED_DT, 0., 0.]);
    let e = p.scenes[0].entity_mut(&lift).unwrap();
    wire(&mut e.graph, &held, "exec", &push, "exec");
    e.graph.nodes = vec![held, push];
    validate_project(&p).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 30);
    let at = |r: &Runtime| {
        (
            r.scene().entity(&lift).unwrap().transform.position[0],
            r.character_state(&body).unwrap().position.x,
        )
    };
    let before = at(&r);
    let pushing = InputFrame {
        held: ["empurrar".into()].into(),
        ..Default::default()
    };
    for _ in 0..60 {
        r.advance(FIXED_DT, &pushing);
        let state = r.character_state(&body).unwrap();
        assert!(state.grounded, "{state:?}");
        assert_eq!(state.support.as_ref().unwrap().object, lift);
    }
    let after = at(&r);
    let moved = (after.0 - before.0, after.1 - before.1);
    assert!((moved.0 - 2.).abs() < 1e-3, "{moved:?}");
    assert!((moved.1 - moved.0).abs() < 1e-3, "{moved:?}");
    clean(&r);
}

#[test]
fn a_platform_logic_turns_every_step_carries_and_turns_its_rider() {
    let (mut p, lift, body) = rider();
    p.input_bindings.insert("girar".into(), "G".into());
    // 2 m from the centre of a platform that turns 90° a second while G is held.
    p.scenes[0].entity_mut(&body).unwrap().transform.position = [2., 2.02, 0.];
    let held = node(
        "event.input",
        &[("action", text("girar")), ("mode", text("held"))],
    );
    let turn = write(None, "rotation", "add", "local", [0., 1.5, 0.]);
    let e = p.scenes[0].entity_mut(&lift).unwrap();
    wire(&mut e.graph, &held, "exec", &turn, "exec");
    e.graph.nodes = vec![held, turn];
    validate_project(&p).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 30);
    let before = r.character_state(&body).unwrap().clone();
    let turning = InputFrame {
        held: ["girar".into()].into(),
        ..Default::default()
    };
    for _ in 0..60 {
        r.advance(FIXED_DT, &turning);
        let state = r.character_state(&body).unwrap();
        assert!(state.grounded, "{state:?}");
        assert_eq!(state.support.as_ref().unwrap().object, lift);
    }
    let quarter = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let state = r.character_state(&body).unwrap();
    let expected = quarter * before.position.with_y(0.);
    assert!(
        state.position.with_y(0.).distance(expected) < 0.02,
        "{:?} != {expected:?}",
        state.position
    );
    assert!(
        glam::Quat::from_rotation_y(state.yaw)
            .angle_between(quarter * glam::Quat::from_rotation_y(before.yaw))
            < 1e-3,
        "{} -> {}",
        before.yaw,
        state.yaw
    );
    clean(&r);
}

#[test]
fn a_write_marked_as_a_jump_drops_the_riders_without_launching_them() {
    let (mut p, lift, body) = rider();
    for (action, key, jump) in [("empurrar", "E", false), ("saltar", "S", true)] {
        p.input_bindings.insert(action.into(), key.into());
        let input = node("event.input", &[("action", text(action))]);
        let mut push = write(None, "position", "add", "world", [0.5, 0., 0.]);
        push.params.insert("jump".into(), Value::Bool(jump));
        let e = p.scenes[0].entity_mut(&lift).unwrap();
        wire(&mut e.graph, &input, "exec", &push, "exec");
        e.graph.nodes.extend([input, push]);
    }
    validate_project(&p).unwrap();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    ticks(&mut r, 30);
    let x = |r: &Runtime| r.character_state(&body).unwrap().position.x;
    // Within the platform's Salto máximo, half a metre in one step carries the rider.
    let before = x(&r);
    r.advance(FIXED_DT, &pressing("empurrar"));
    assert!((x(&r) - before - 0.5).abs() < 1e-3, "{}", x(&r) - before);
    ticks(&mut r, 30);
    clean(&r);
    // Marked as a jump, the same move leaves the rider where it was, without speed.
    let before = x(&r);
    r.advance(FIXED_DT, &pressing("saltar"));
    let state = r.character_state(&body).unwrap();
    assert!(
        !state.grounded && state.velocity.x.abs() < 0.001 && (x(&r) - before).abs() < 0.01,
        "{state:?}"
    );
    assert_eq!(r.logs.len(), 1, "{:?}", r.logs);
    assert!(
        r.logs[0].contains("Apoio em Plataforma liberado sem herdar velocidade"),
        "{:?}",
        r.logs
    );
    // It lands on the platform again.
    ticks(&mut r, 30);
    let state = r.character_state(&body).unwrap();
    assert!(state.grounded, "{state:?}");
    assert_eq!(state.support.as_ref().unwrap().object, lift);
}
