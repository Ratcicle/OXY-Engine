//! Read views for agents: world positions and boxes under parent groups, trees read part by
//! part, objects in part with their logic summarized, paged search and a catalog filtered by
//! category and operation.
use oxy_core::{
    command::{self, Applied},
    document::*,
    editing, graph,
    physics3d::{self, Collider3d, CollisionShape},
    query::{self, Detail, EntityQuery, Page, TreeField, TreeQuery},
};
use serde_json::{Value as Json, json};

fn project(kind: SceneKind) -> (Project, Id) {
    let project = editing::blank_project("Consultas", kind).unwrap();
    let scene = project.start_scene.clone();
    (project, scene)
}

fn apply(project: &mut Project, scene: &str, commands: Json) -> Vec<Applied> {
    command::apply(project, scene, &command::parse(&commands).unwrap()).unwrap()
}

fn near(value: &Json, expected: &[f32]) {
    let values: Vec<f64> = value
        .as_array()
        .unwrap_or_else(|| panic!("{value} is not a list"))
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(values.len(), expected.len(), "{value}");
    for (v, e) in values.iter().zip(expected) {
        assert!((v - f64::from(*e)).abs() < 1e-4, "{value} != {expected:?}");
    }
}

fn names(objects: &Json) -> Vec<String> {
    objects
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap().to_owned())
        .collect()
}

/// A section group at (10, 2, 0), turned 90° and doubled, with a platform and an invisible
/// checkpoint area inside: where they are only shows through the parent.
fn section() -> (Project, Id, [Id; 3]) {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let mut group = Entity::new("Seção", None);
    group.transform.position = [10., 2., 0.];
    group.transform.rotation = [0., 90f32.to_radians(), 0.];
    group.transform.scale = [2.; 3];
    let mut platform = Entity::new("Plataforma", Some(Primitive::Cube));
    platform.parent = Some(group.id.clone());
    platform.transform.position = [1., 0., 0.];
    platform.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size: [1.; 3] },
        ..Default::default()
    });
    let mut checkpoint = Entity::new("Ponto de controle", None);
    checkpoint.parent = Some(group.id.clone());
    checkpoint.transform.position = [0., 1., 3.];
    checkpoint.visible = false;
    checkpoint.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size: [2.; 3] },
        sensor: true,
        enabled: false,
        ..Default::default()
    });
    let ids = [group.id.clone(), platform.id.clone(), checkpoint.id.clone()];
    project.scene_mut(&scene).unwrap().entities = vec![group, platform, checkpoint];
    (project, scene, ids)
}

#[test]
fn scene_tree_reports_world_positions_and_boxes_through_parent_groups() {
    let (project, scene, [_, platform, _]) = section();
    let mut tree_query = TreeQuery::parse(&json!({"fields": ["world_position", "bounds",
                                                              "rotation", "scale", "dimensions"]}))
    .unwrap();
    let tree = query::scene_tree(&project, &scene, &tree_query).unwrap();
    let group = &tree["objects"][0];
    assert_eq!(group["name"], "Seção");
    near(&group["world_position"], &[10., 2., 0.]);
    near(&group["rotation"], &[0., 90., 0.]);
    near(&group["scale"], &[2., 2., 2.]);
    assert!(group.get("dimensions").is_none(), "a group has no shape");
    assert!(group.get("collider_bounds").is_none());
    // The group's box is what its children show: the platform, not the empty area.
    near(&group["visual_bounds"]["min"], &[9., 1., -3.]);
    near(&group["visual_bounds"]["max"], &[11., 3., -1.]);
    let platform_node = &group["children"][0];
    assert_eq!(platform_node["id"], platform.as_str());
    near(&platform_node["position"], &[1., 0., 0.]);
    near(&platform_node["world_position"], &[10., 2., -2.]);
    near(&platform_node["dimensions"], &[1., 1., 1.]);
    near(&platform_node["collider_bounds"]["min"], &[9., 1., -3.]);
    near(&platform_node["collider_bounds"]["max"], &[11., 3., -1.]);
    near(&platform_node["collider_bounds"]["center"], &[10., 2., -2.]);
    assert!(platform_node["collider_bounds"].get("sensor").is_none());
    let area = &group["children"][1];
    assert_eq!(area["visible"], false);
    near(&area["world_position"], &[16., 4., 0.]);
    // cos 90° is not 0 in f32: the noise it leaves on the axis the group turned away from
    // reads 0, not 2.6e-7.
    assert_eq!(area["world_position"][2].as_f64(), Some(0.));
    assert_eq!(area["collider_bounds"]["center"][2].as_f64(), Some(0.));
    assert!(area.get("visual_bounds").is_none(), "nothing to show");
    near(&area["collider_bounds"]["min"], &[14., 2., -2.]);
    near(&area["collider_bounds"]["max"], &[18., 6., 2.]);
    assert_eq!(area["collider_bounds"]["sensor"], true);
    assert_eq!(area["collider_bounds"]["enabled"], false);
    // Without fields, only the local position, as before.
    tree_query.fields.clear();
    let plain = query::scene_tree(&project, &scene, &tree_query).unwrap();
    let plain_platform = &plain["objects"][0]["children"][0];
    near(&plain_platform["position"], &[1., 0., 0.]);
    for extra in [
        "world_position",
        "visual_bounds",
        "collider_bounds",
        "dimensions",
    ] {
        assert!(plain_platform.get(extra).is_none(), "{extra}");
    }
    // Each box can be asked for alone.
    let colliders = TreeQuery::parse(&json!({"fields": ["collider_bounds"]})).unwrap();
    let only = query::scene_tree(&project, &scene, &colliders).unwrap();
    assert!(
        only["objects"][0]["children"][0]
            .get("collider_bounds")
            .is_some()
    );
    assert!(only["objects"][0].get("visual_bounds").is_none());
}

#[test]
fn world_position_is_where_the_pivot_is_and_characters_and_2d_colliders_have_boxes() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let mut pivoted = Entity::new("Porta", Some(Primitive::Cube));
    pivoted.transform.position = [5., 0., 0.];
    pivoted.transform.pivot = [0.5, 0., 0.];
    let pivoted_id = pivoted.id.clone();
    project.scene_mut(&scene).unwrap().entities.push(pivoted);
    let hero = apply(
        &mut project,
        &scene,
        json!([{"op": "create_entity", "kind": {"type": "preset", "preset": "third_person"},
                "name": "Herói", "position": [1.0, 0.0, 2.0]}]),
    )[0]
    .created[0]
        .clone();
    let pivot = query::entity(&project, &scene, &pivoted_id).unwrap();
    near(&pivot["world"]["position"], &[5., 0., 0.]);
    near(&pivot["visual_bounds"]["min"], &[4., -0.5, -0.5]);
    let hero = query::entity(&project, &scene, &hero).unwrap();
    let body = &hero["collider_bounds"];
    let (min, max) = (&body["min"], &body["max"]);
    let size = |axis: usize| max[axis].as_f64().unwrap() - min[axis].as_f64().unwrap();
    let character = project
        .scene(&scene)
        .unwrap()
        .entity(hero["id"].as_str().unwrap())
        .unwrap()
        .character3d
        .clone()
        .unwrap();
    let CollisionShape::Capsule { height, radius } = character.body.standing else {
        panic!("the preset stands on a capsule");
    };
    assert!((size(1) - f64::from(height)).abs() < 1e-4, "{body}");
    assert!((size(0) - f64::from(2. * radius)).abs() < 1e-4, "{body}");
    assert!(
        (min[1].as_f64().unwrap()).abs() < 1e-3,
        "feet on the floor: {body}"
    );

    let (mut flat, flat_scene) = self::project(SceneKind::TwoD);
    let mut crate_box = Entity::new("Caixote", Some(Primitive::Rectangle));
    crate_box.transform.position = [3., 4., 0.];
    crate_box.collider = Some(Collider {
        size: [2., 1., 1.],
        offset: [0.5, 0., 0.],
        is_trigger: true,
        ..Default::default()
    });
    let id = crate_box.id.clone();
    flat.scene_mut(&flat_scene)
        .unwrap()
        .entities
        .push(crate_box);
    let crate_view = query::entity(&flat, &flat_scene, &id).unwrap();
    near(&crate_view["collider_bounds"]["center"], &[3.5, 4., 0.]);
    near(&crate_view["collider_bounds"]["min"], &[2.5, 3.5, -0.5]);
    assert_eq!(crate_view["collider_bounds"]["sensor"], true);
}

#[test]
fn big_scenes_are_read_part_by_part() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands: Vec<Json> = (1..=5)
        .map(|i| json!({"op": "create_entity", "kind": {"type": "group"}, "name": format!("Peça {i}")}))
        .chain([
            json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Fase"}),
            json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Bloco A", "parent": "$5"}),
            json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Bloco B", "parent": "$5"}),
            json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Detalhe", "parent": "$6"}),
        ])
        .collect();
    let applied = apply(&mut project, &scene, Json::Array(commands));
    let fase = applied[5].created[0].clone();
    let tree = |args: Json| {
        TreeQuery::parse(&args).and_then(|options| query::scene_tree(&project, &scene, &options))
    };

    let all = tree(json!({})).unwrap();
    assert_eq!(all["total"], 6);
    assert!(all.get("next_offset").is_none());
    assert_eq!(
        names(&all["objects"][5]["children"][0]["children"]),
        ["Detalhe"]
    );
    let window = tree(json!({"offset": 1, "limit": 2})).unwrap();
    assert_eq!(names(&window["objects"]), ["Peça 2", "Peça 3"]);
    assert_eq!(window["total"], 6);
    assert_eq!(window["next_offset"], 3);
    assert!(
        tree(json!({"offset": 4, "limit": 2}))
            .unwrap()
            .get("next_offset")
            .is_none()
    );

    let shallow = tree(json!({"depth": 0})).unwrap();
    assert_eq!(shallow["objects"][5]["children_count"], 2);
    assert!(shallow["objects"][5].get("children").is_none());
    let one_level = tree(json!({"depth": 1})).unwrap();
    let blocks = &one_level["objects"][5]["children"];
    assert_eq!(names(blocks), ["Bloco A", "Bloco B"]);
    assert_eq!(blocks[0]["children_count"], 1);
    assert!(
        blocks[1].get("children_count").is_none(),
        "no children to count"
    );

    let inside = tree(json!({"root": fase, "limit": 1})).unwrap();
    assert_eq!(inside["root"], json!({"id": fase, "name": "Fase"}));
    assert_eq!(names(&inside["objects"]), ["Bloco A"]);
    assert_eq!(inside["total"], 2);
    assert_eq!(inside["next_offset"], 1);
    assert_eq!(inside["objects"][0]["children_count"], 1, "no room left");
    assert_eq!(inside["truncated"], true);

    // The limit counts children too, level by level: 6 listed objects, then Fase's 2
    // children, then Detalhe.
    let cut = tree(json!({"limit": 7})).unwrap();
    assert_eq!(cut["objects"][5]["children_count"], 2);
    assert_eq!(cut["truncated"], true);
    assert!(
        cut.get("next_offset").is_none(),
        "every listed object is there"
    );
    let cut = tree(json!({"limit": 8})).unwrap();
    assert_eq!(
        names(&cut["objects"][5]["children"]),
        ["Bloco A", "Bloco B"]
    );
    assert_eq!(cut["objects"][5]["children"][0]["children_count"], 1);
    assert_eq!(cut["truncated"], true);
    assert!(all.get("truncated").is_none());
    assert!(
        shallow.get("truncated").is_none(),
        "depth cuts as asked, not by the limit"
    );
    assert_eq!(
        TreeQuery::default().page,
        Page {
            offset: 0,
            limit: query::TREE_LIMIT
        }
    );

    for (args, expected) in [
        (json!({"root": "fantasma"}), "Objeto fantasma não existe"),
        (json!({"fields": ["altura"]}), "fields inválido"),
        (json!({"depth": -1}), "depth inválido"),
        (json!({"limit": "dez"}), "limit inválido"),
    ] {
        let error = tree(args).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    assert_eq!(
        TreeQuery::parse(&json!({"fields": ["bounds"], "project": "x", "scene": null}))
            .unwrap()
            .fields
            .into_iter()
            .collect::<Vec<_>>(),
        [TreeField::Bounds],
        "other tool arguments are ignored"
    );
}

#[test]
fn a_level_of_many_sections_stays_within_the_limit() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let mut commands = Vec::new();
    for section in 1..=10 {
        let group = commands.len();
        commands.push(json!({"op": "create_entity", "kind": {"type": "group"},
                             "name": format!("Seção {section}")}));
        for platform in 1..=30 {
            commands.push(
                json!({"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
                                 "name": format!("Plataforma {platform}"),
                                 "parent": format!("${group}")}),
            );
        }
    }
    let small = commands.len();
    commands.push(json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Pequena"}));
    for _ in 0..2 {
        commands.push(
            json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Item",
                             "parent": format!("${small}")}),
        );
    }
    let applied = apply(&mut project, &scene, Json::Array(commands));
    let tree = |args: Json| query::answer(&project, &scene, "scene_tree", &args).unwrap();
    let count = |nodes: &Json| -> usize {
        fn walk(node: &Json) -> usize {
            1 + node["children"]
                .as_array()
                .map_or(0, |kids| kids.iter().map(walk).sum())
        }
        nodes.as_array().unwrap().iter().map(walk).sum()
    };

    // 11 listed objects leave room for 89: two whole sections, then the small group, whose
    // children still fit; the other sections only count theirs.
    let default = tree(json!({"fields": ["bounds"]}));
    assert_eq!(default["total"], 11);
    assert_eq!(default["truncated"], true);
    assert!(default.get("next_offset").is_none());
    let objects = default["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 11);
    assert_eq!(objects[0]["children"].as_array().unwrap().len(), 30);
    assert_eq!(objects[1]["children"].as_array().unwrap().len(), 30);
    for section in &objects[2..10] {
        assert_eq!(section["children_count"], 30, "{}", section["name"]);
        assert!(section.get("children").is_none());
    }
    assert_eq!(names(&objects[10]["children"]), ["Item", "Item"]);
    assert_eq!(count(&default["objects"]), 73);
    assert!(count(&default["objects"]) <= query::TREE_LIMIT);

    // A section cut out is read with root, and a higher limit takes the whole level.
    let third = tree(json!({"root": applied[62].created[0]}));
    assert_eq!(third["objects"].as_array().unwrap().len(), 30);
    assert!(third.get("truncated").is_none());
    let whole = tree(json!({"limit": 400}));
    assert_eq!(count(&whole["objects"]), 313);
    assert!(whole.get("truncated").is_none());
}

/// Rotation of the object `id` in the world, from oxy_entity's degrees.
fn world_rotation(project: &Project, scene: &str, id: &str) -> glam::Quat {
    let view = query::entity(project, scene, id).unwrap();
    let degrees: Vec<f32> = view["world"]["rotation_degrees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_f64().unwrap() as f32).to_radians())
        .collect();
    glam::Quat::from_euler(glam::EulerRot::XYZ, degrees[0], degrees[1], degrees[2])
}

#[test]
fn world_rotation_reads_as_set_transform_takes_it_under_turned_groups() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    // Groups turned 120°, 180° and 30° around Y, one pitched 90°, and one not turned, each
    // with a child; the 30° group's child turns 150° more, and the plain group holds a piece
    // turned 200° and one set upside down as [180, 0, 180].
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Curva"},
            {"op": "set_transform", "entity": "$0", "rotation": [0.0, 120.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "A",
             "parent": "$0", "position": [0.0, 0.0, 4.0]},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Volta"},
            {"op": "set_transform", "entity": "$3", "rotation": [0.0, 180.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "B",
             "parent": "$3"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Leve"},
            {"op": "set_transform", "entity": "$6", "rotation": [0.0, 30.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "C",
             "parent": "$6"},
            {"op": "set_transform", "entity": "$8", "rotation": [0.0, 150.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Reta"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "D",
             "parent": "$10"},
            {"op": "set_transform", "entity": "$11", "rotation": [0.0, 200.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "E",
             "parent": "$10"},
            {"op": "set_transform", "entity": "$13", "rotation": [180.0, 0.0, 180.0]},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Parede"},
            {"op": "set_transform", "entity": "$15", "rotation": [90.0, 0.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "F",
             "parent": "$15"},
            {"op": "set_transform", "entity": "$17", "rotation": [0.0, 120.0, 0.0]},
        ]),
    );
    let id = |i: usize| applied[i].created[0].clone();
    let degrees = |i: usize| {
        query::entity(&project, &scene, &id(i)).unwrap()["world"]["rotation_degrees"].clone()
    };
    // Roots read exactly as they were set, and so do their children.
    for (object, expected) in [
        (0, [0., 120., 0.]),
        (2, [0., 120., 0.]),
        (3, [0., 180., 0.]),
        (5, [0., 180., 0.]),
        (8, [0., 180., 0.]),
        (11, [0., -160., 0.]),
        (13, [180., 0., 180.]),
    ] {
        near(&degrees(object), &expected);
        let view = query::entity(&project, &scene, &id(object)).unwrap();
        if view.get("parent").is_none() {
            assert_eq!(view["world"]["rotation_degrees"], view["rotation_degrees"]);
        }
    }
    // A child 4 m ahead of the 120° group sits where the turn puts it.
    let a = query::entity(&project, &scene, &id(2)).unwrap();
    near(
        &a["world"]["position"],
        &[
            4. * 120f32.to_radians().sin(),
            0.,
            4. * 120f32.to_radians().cos(),
        ],
    );
    // Whatever triple is shown, it is the rotation the object has in the world.
    let current = project.scene(&scene).unwrap();
    for object in [2, 5, 8, 11, 13, 17] {
        let entity = current.entity(&id(object)).unwrap();
        let parent = current.entity(entity.parent.as_ref().unwrap()).unwrap();
        let expected = parent.transform.quaternion() * entity.transform.quaternion();
        let shown = world_rotation(&project, &scene, &id(object));
        assert!(
            shown.dot(expected).abs() > 1. - 1e-6,
            "{}: {shown} != {expected}",
            entity.name
        );
    }
    // Every angle is in (-180, 180].
    for object in [2, 5, 8, 11, 13, 17] {
        for angle in degrees(object).as_array().unwrap() {
            let angle = angle.as_f64().unwrap();
            assert!(angle > -180. && angle <= 180., "{angle}");
        }
    }
}

/// An object with logic: an input starts two equal messages, then sets an attribute from a
/// number node.
fn scripted() -> (Project, Id, Id, Vec<Applied>) {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Caixa",
             "position": [0.0, 1.0, 0.0]},
            {"op": "add_node", "entity": "$0", "operation": "event.input",
             "params": {"action": {"Text": "reiniciar"}}},
            {"op": "add_node", "entity": "$0", "operation": "debug.message",
             "params": {"message": {"Text": "oi"}}},
            {"op": "add_node", "entity": "$0", "operation": "debug.message",
             "params": {"message": {"Text": "oi"}}},
            {"op": "add_node", "entity": "$0", "operation": "value.number",
             "params": {"value": {"Number": 5.0}}},
            {"op": "add_node", "entity": "$0", "operation": "attribute.set",
             "params": {"attribute": {"Text": "Pontos"}}},
            {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
             "to_node": "$2", "to_port": "exec"},
            {"op": "connect", "entity": "$0", "from_node": "$2", "from_port": "exec",
             "to_node": "$3", "to_port": "exec"},
            {"op": "connect", "entity": "$0", "from_node": "$3", "from_port": "exec",
             "to_node": "$5", "to_port": "exec"},
            {"op": "connect", "entity": "$0", "from_node": "$4", "from_port": "value",
             "to_node": "$5", "to_port": "value"},
            {"op": "set_attribute", "entity": "$0", "name": "Pontos", "value": {"Number": 0.0}},
        ]),
    );
    let id = applied[0].created[0].clone();
    (project, scene, id, applied)
}

#[test]
fn entities_summarize_logic_as_labelled_edges() {
    let (project, scene, id, applied) = scripted();
    let view = query::entity(&project, &scene, &id).unwrap();
    let summary = &view["graph"];
    assert_eq!(
        summary["edges"],
        json!([
            "event.input(reiniciar).exec -> debug.message(oi)#1.exec",
            "debug.message(oi)#1.exec -> debug.message(oi)#2.exec",
            "debug.message(oi)#2.exec -> attribute.set(Pontos).exec",
            "value.number(5).value -> attribute.set(Pontos).value",
        ])
    );
    let nodes = &summary["nodes"];
    assert_eq!(nodes.as_object().unwrap().len(), 5);
    assert_eq!(
        nodes["event.input(reiniciar)"],
        json!({"id": applied[1].created[0], "params": {"action": {"Text": "reiniciar"}}}),
        "only the parameters that differ from the defaults"
    );
    assert_eq!(
        nodes["debug.message(oi)#2"]["id"],
        applied[3].created[0].as_str()
    );
    assert_eq!(
        nodes["attribute.set(Pontos)"]["params"],
        json!({"attribute": {"Text": "Pontos"}})
    );
    assert_eq!(
        nodes["value.number(5)"]["params"],
        json!({"value": {"Number": 5.0}})
    );
    // Asked in full, the graph is the stored one.
    let full = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"id": id, "graph": "full"})).unwrap(),
    )
    .unwrap();
    let stored = &project.scene(&scene).unwrap().entity(&id).unwrap().graph;
    assert_eq!(full["graph"], serde_json::to_value(stored).unwrap());
    // Scenes and assets a node names show by name, long texts shortened.
    let mut named = graph::Graph::default();
    let mut change = graph::Node::new("action.scene", [0.; 2]);
    change
        .params
        .insert("scene".into(), Value::Text(scene.clone()));
    let mut long = graph::Node::new("debug.message", [0.; 2]);
    long.params.insert(
        "message".into(),
        Value::Text("Uma mensagem bem mais longa que o rótulo".into()),
    );
    named.nodes = vec![change, long];
    let labels = query::graph_summary(&project, project.scene(&scene).unwrap(), &named)["nodes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "action.scene(Cena 3D)",
            "debug.message(Uma mensagem bem mais l…)"
        ]
    );
}

#[test]
fn entities_add_where_they_are_and_can_be_cut_to_the_parts_asked_for() {
    let (project, scene, id, _) = scripted();
    let view = query::entity(&project, &scene, &id).unwrap();
    near(&view["world"]["position"], &[0., 1., 0.]);
    near(&view["world"]["rotation_degrees"], &[0., 0., 0.]);
    near(&view["world"]["scale"], &[1., 1., 1.]);
    near(&view["visual_bounds"]["max"], &[0.5, 1.5, 0.5]);
    near(&view["rotation_degrees"], &[0., 0., 0.]);
    assert!(view.get("collider_bounds").is_none(), "no collider");
    assert!(view["can_add_components"].is_array());
    assert_eq!(view["components"], json!([]));
    for empty in [
        "parent",
        "camera",
        "ui",
        "collider",
        "controller",
        "model_source",
    ] {
        assert!(view.get(empty).is_none(), "{empty} is null and left out");
    }
    for key in view.as_object().unwrap().keys() {
        assert!(query::ENTITY_FIELDS.contains(&key.as_str()), "{key}");
    }

    let cut = |args: Json| {
        let mut args = args;
        args["id"] = json!(id);
        query::entities(&project, &scene, &EntityQuery::parse(&args)?)
    };
    let picked = cut(
        json!({"fields": ["transform.position", "graph.edges", "world",
                                         "character3d", "attributes.Vida", "clips.0"]}),
    )
    .unwrap();
    assert_eq!(
        picked.as_object().unwrap().keys().collect::<Vec<_>>(),
        [
            "attributes",
            "character3d",
            "clips",
            "graph",
            "id",
            "name",
            "transform",
            "world"
        ]
    );
    near(&picked["transform"]["position"], &[0., 1., 0.]);
    assert!(picked["transform"].get("scale").is_none());
    assert_eq!(picked["graph"]["edges"].as_array().unwrap().len(), 4);
    assert!(picked["graph"].get("nodes").is_none());
    assert_eq!(picked["character3d"], Json::Null, "absent parts are null");
    assert_eq!(picked["attributes"], json!({"Vida": null}));
    assert_eq!(picked["clips"], json!({"0": null}));
    // A path inside a part asked whole changes nothing, in either order.
    for fields in [
        json!(["transform", "transform.position"]),
        json!(["transform.position", "transform"]),
    ] {
        let whole = cut(json!({"fields": fields})).unwrap();
        assert!(whole["transform"].get("scale").is_some(), "{whole}");
    }
    let attribute = cut(json!({"fields": ["attributes.Pontos"]})).unwrap();
    assert_eq!(attribute["attributes"]["Pontos"], json!({"Number": 0.0}));

    let error = cut(json!({"fields": ["atributos"]})).unwrap_err();
    assert!(error.contains("Campo desconhecido: atributos"), "{error}");
    let error = cut(json!({"graph": "tudo"})).unwrap_err();
    assert!(error.contains("graph inválido"), "{error}");

    let many = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"ids": [id, id], "fields": ["name"]})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        many,
        json!({"objects": [{"id": id, "name": "Caixa"}, {"id": id, "name": "Caixa"}]})
    );
    let error = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"ids": [id, "fantasma"]})).unwrap(),
    )
    .unwrap_err();
    assert!(error.contains("fantasma"), "{error}");
    assert!(
        EntityQuery::parse(&json!({}))
            .unwrap_err()
            .contains("Informe id ou ids")
    );
    let parsed = EntityQuery::parse(&json!({"id": "a", "ids": ["b"]})).unwrap();
    assert_eq!(parsed.ids, ["a", "b"]);
    assert!(parsed.list);
    assert_eq!(parsed.graph, Detail::Summary);
}

#[test]
fn geometry_is_counted_unless_asked_in_full() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let mut piece = Entity::new("Rampa", Some(Primitive::Cube));
    piece.physics3d = Some(physics3d::generate_collider(&piece, true).unwrap());
    piece.mesh = Some(oxy_core::geometry::primitives::for_entity(&piece).unwrap());
    let mesh = piece.mesh.as_ref().unwrap().data().clone();
    let CollisionShape::Convex { geometry } = &piece.physics3d.as_ref().unwrap().shape else {
        panic!("convex expected");
    };
    let hull = (geometry.vertices.len(), geometry.triangles.len());
    let id = piece.id.clone();
    project.scene_mut(&scene).unwrap().entities.push(piece);
    let view = query::entity(&project, &scene, &id).unwrap();
    assert_eq!(
        view["mesh"],
        json!({"vertices": mesh.vertices.len(), "edges": mesh.edges.len(),
               "faces": mesh.faces.len()})
    );
    assert_eq!(
        view["physics3d"]["shape"]["Convex"]["geometry"],
        json!({"vertices": hull.0, "triangles": hull.1})
    );
    near(&view["collider_bounds"]["max"], &[0.5, 0.5, 0.5]);
    let full = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"id": id, "geometry": "full"})).unwrap(),
    )
    .unwrap();
    let stored = project.scene(&scene).unwrap().entity(&id).unwrap();
    assert_eq!(full["mesh"], serde_json::to_value(&stored.mesh).unwrap());
    assert_eq!(
        full["physics3d"],
        serde_json::to_value(&stored.physics3d).unwrap()
    );
}

#[test]
fn search_is_paged() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let commands: Vec<Json> = (1..=5)
        .map(|i| json!({"op": "create_entity", "kind": {"type": "group"}, "name": format!("Moeda {i}")}))
        .chain([json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"})])
        .collect();
    apply(&mut project, &scene, Json::Array(commands));
    let found = query::find(&project, "MOEDA", Page::ALL);
    assert_eq!(found["total"], 5);
    assert_eq!(found["objects"][0]["scene_name"], "Cena 3D");
    assert_eq!(found["objects"][0]["kind"], "group");
    let page = query::find(
        &project,
        "moeda",
        Page {
            offset: 2,
            limit: 2,
        },
    );
    assert_eq!(names(&page["objects"]), ["Moeda 3", "Moeda 4"]);
    assert_eq!(page["next_offset"], 4);
    let answered = query::answer(
        &project,
        &scene,
        "find",
        &json!({"text": "moeda", "limit": 1}),
    )
    .unwrap();
    assert_eq!(names(&answered["objects"]), ["Moeda 1"]);
    assert_eq!(answered["next_offset"], 1);
    assert!(
        query::answer(&project, &scene, "find", &json!({}))
            .unwrap_err()
            .contains("text")
    );
    assert!(
        query::answer(&project, &scene, "voar", &json!({}))
            .unwrap_err()
            .contains("voar")
    );
    assert_eq!(
        query::answer(&project, &scene, "summary", &json!({})).unwrap(),
        query::project_summary(&project)
    );
}

#[test]
fn the_catalog_is_brief_by_default_and_full_for_the_nodes_asked_for() {
    let registry = graph::registry();
    let brief = query::catalog_query(&json!({})).unwrap();
    let nodes = brief["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), registry.len());
    assert_eq!(brief["total"], registry.len());
    for node in nodes {
        assert_eq!(
            node.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["category", "description", "label", "operation"]
        );
    }
    let categories = brief["categories"].as_array().unwrap();
    assert!(categories.contains(&json!("Ações")) && categories.contains(&json!("Eventos")));
    for section in [
        "components",
        "shapes",
        "ui_elements",
        "presets",
        "value_format",
    ] {
        assert!(brief.get(section).is_some(), "{section}");
    }
    assert!(
        brief.to_string().len() * 2 < query::catalog().to_string().len(),
        "brief is far shorter"
    );

    let actions = query::catalog_query(&json!({"category": "ações"})).unwrap();
    let action_nodes = actions["nodes"].as_array().unwrap();
    assert!(!action_nodes.is_empty());
    assert!(
        action_nodes
            .iter()
            .all(|n| n["category"] == "Ações" && n["params"].is_array())
    );
    assert!(
        actions.get("components").is_none(),
        "only the nodes asked for"
    );

    let picked =
        query::catalog_query(&json!({"operations": ["event.input", "character."]})).unwrap();
    let operations: Vec<&str> = picked["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["operation"].as_str().unwrap())
        .collect();
    let characters = registry
        .iter()
        .filter(|op| op.id.starts_with("character."))
        .count();
    assert!(characters > 0);
    assert_eq!(operations.len(), characters + 1);
    assert!(operations.contains(&"event.input"));
    assert!(picked["nodes"][0]["inputs"].is_array(), "in full");
    let star = query::catalog_query(&json!({"operations": ["character.*"]})).unwrap();
    assert_eq!(star["total"], characters);
    let brief_picked =
        query::catalog_query(&json!({"operations": ["event.input"], "brief": true})).unwrap();
    assert!(brief_picked["nodes"][0].get("params").is_none());
    let window = query::catalog_query(&json!({"limit": 3, "offset": 3})).unwrap();
    assert_eq!(window["nodes"][0]["operation"], registry[3].id);
    assert_eq!(window["next_offset"], 6);
    let everything = query::catalog_query(&json!({"brief": false})).unwrap();
    assert_eq!(everything["nodes"], query::catalog()["nodes"]);
    // Category and operations together: the operations of that category.
    let both = query::catalog_query(&json!({"category": "Eventos", "operations": ["event.input"]}))
        .unwrap();
    assert_eq!(both["total"], 1);
    assert_eq!(both["nodes"][0]["operation"], "event.input");

    for (args, expected) in [
        (
            json!({"category": "Mágica"}),
            "Categoria desconhecida: Mágica",
        ),
        (
            json!({"operations": ["voar"]}),
            "Operação desconhecida: voar",
        ),
        (
            json!({"operations": ["voo."]}),
            "Nenhuma operação começa com voo.",
        ),
        // An operation of another category is not unknown.
        (
            json!({"category": "Ações", "operations": ["event.input"]}),
            "event.input não é da categoria Ações, e sim de Eventos",
        ),
        (
            json!({"category": "ações", "operations": ["event."]}),
            "Nenhuma operação que começa com event. é da categoria Ações",
        ),
        (
            json!({"category": "Ações", "operations": ["voar"]}),
            "Operação desconhecida: voar",
        ),
        (json!({"operations": "event.input"}), "operations inválido"),
        (json!({"brief": "sim"}), "brief inválido"),
    ] {
        let error = query::catalog_query(&args).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn reading_never_changes_the_project() {
    let (project, scene, id, _) = scripted();
    let before = project.clone();
    for (request, args) in [
        ("summary", json!({})),
        (
            "scene_tree",
            json!({"fields": ["bounds", "world_position"], "depth": 0}),
        ),
        (
            "entity",
            json!({"id": id, "graph": "full", "geometry": "full"}),
        ),
        ("entity", json!({"ids": [id], "fields": ["graph"]})),
        ("find", json!({"text": "caixa"})),
        ("catalog", json!({"category": "Eventos"})),
    ] {
        query::answer(&project, &scene, request, &args).unwrap();
    }
    assert_eq!(project, before);
}

/// A level whose logic, attributes, interface, camera and animations name things: a menu scene,
/// an input action, a game variable, an animation, a panel, a first-person player and a door
/// whose animation moves its leaf.
fn named_level() -> (Project, Id, Vec<Applied>) {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_scene", "name": "Menu", "kind": "3d"},
            {"op": "create_input_action", "name": "Reiniciar", "key": "R"},
            {"op": "create_variable", "name": "Recorde", "value": 0},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Resultado"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Disco"},
            {"op": "set_clip", "entity": "$4", "name": "Giro", "duration": 4.0, "tracks": [
                {"keyframes": [{"time": 0.0}, {"time": 1.0, "rotation": [0.0, 90.0, 0.0]}]}]},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Controle"},
            {"op": "add_node", "entity": "$6", "operation": "event.input",
             "params": {"action": "reiniciar"}},
            {"op": "add_node", "entity": "$6", "operation": "action.component",
             "params": {"component": "visible", "target": {"Object": "$3"}}},
            {"op": "add_node", "entity": "$6", "operation": "action.scene",
             "params": {"scene": "menu"}},
            {"op": "add_node", "entity": "$6", "operation": "action.animation",
             "params": {"clip": "$5", "target": {"Object": "$4"}}},
            {"op": "add_node", "entity": "$6", "operation": "variable.get",
             "params": {"variable": "Recorde"}},
            {"op": "add_node", "entity": "$6", "operation": "condition.branch"},
            {"op": "add_node", "entity": "$6", "operation": "value.bool"},
            {"op": "connect", "entity": "$6", "from_node": "$13", "from_port": "value",
             "to_node": "$12", "to_port": "condition"},
            {"op": "connect", "entity": "$6", "from_node": "$7", "from_port": "exec",
             "to_node": "$8", "to_port": "exec"},
            {"op": "set_attribute", "entity": "$4", "name": "Painel", "value": {"Object": "$3"}},
            {"op": "create_entity", "kind": {"type": "ui", "element": "Text"}, "name": "Placar"},
            {"op": "update_entity", "entity": "$17",
             "patch": {"ui": {"binding_object": "$3", "binding_attribute": "Pontos"}}},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "first_person"}},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Folha",
             "parent": "$20"},
            {"op": "set_clip", "entity": "$20", "name": "Abrir", "duration": 1.0, "tracks": [
                {"target": "$21", "keyframes": [{"time": 0.0},
                                                {"time": 1.0, "rotation": [0.0, 90.0, 0.0]}]}]},
        ]),
    );
    (project, scene, applied)
}

#[test]
fn summaries_name_what_nodes_name_and_leave_out_parameters_inputs_replace() {
    let (project, scene, applied) = named_level();
    let id = |command: usize| applied[command].created[0].clone();
    let view = query::entity(&project, &scene, &id(6)).unwrap();
    let nodes = view["graph"]["nodes"].as_object().unwrap();
    let mut labels: Vec<&str> = nodes.keys().map(String::as_str).collect();
    labels.sort_unstable();
    assert_eq!(
        labels,
        [
            "action.animation(Giro → Disco)",
            "action.component(visible → Resultado)",
            "action.scene(Menu)",
            // Its condition comes through the input: the parameter is not what it uses.
            "condition.branch",
            "event.input(Reiniciar)",
            "value.bool(false)",
            "variable.get(Recorde)",
        ]
    );
    assert_eq!(
        view["graph"]["edges"],
        json!([
            "value.bool(false).value -> condition.branch.condition",
            "event.input(Reiniciar).exec -> action.component(visible → Resultado).exec",
        ])
    );
    // Parameters keep the IDs the game reads, with the names beside them.
    assert_eq!(
        nodes["action.component(visible → Resultado)"]["params"],
        json!({"component": {"Text": "visible"},
               "target": {"Object": id(3), "name": "Resultado"}})
    );
    assert_eq!(
        nodes["action.scene(Menu)"]["params"],
        json!({"scene": {"Text": id(0), "name": "Menu"}})
    );
    assert_eq!(
        nodes["event.input(Reiniciar)"]["params"],
        json!({"action": {"Text": id(1), "name": "Reiniciar"}})
    );
    assert_eq!(
        nodes["action.animation(Giro → Disco)"]["params"]["clip"],
        json!({"Text": id(5), "name": "Giro"})
    );
    assert_eq!(
        nodes["variable.get(Recorde)"]["params"],
        json!({"variable": {"Text": id(2), "name": "Recorde"}})
    );
    // No ID in a label or an edge.
    let summary = view["graph"].to_string();
    let labels_and_edges = format!("{:?} {}", labels, view["graph"]["edges"]);
    for command in [0, 1, 2, 3, 4, 5] {
        assert!(!labels_and_edges.contains(&id(command)), "{summary}");
    }
    // In full, the graph is the stored one: IDs only.
    let full = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"id": id(6), "graph": "full"})).unwrap(),
    )
    .unwrap();
    assert!(
        !full["graph"].to_string().contains("\"name\""),
        "{}",
        full["graph"]
    );
}

#[test]
fn objects_list_what_references_them_with_the_scene() {
    let (project, scene, applied) = named_level();
    let id = |command: usize| applied[command].created[0].clone();
    let place = json!({"id": scene, "name": "Cena 3D"});
    let panel = query::entity(&project, &scene, &id(3)).unwrap();
    assert_eq!(
        panel["referenced_by"],
        json!([
            {"scene": place, "entity": id(4), "name": "Disco", "role": "attribute Painel"},
            {"scene": place, "entity": id(6), "name": "Controle", "role": "action.component",
             "node": "action.component(visible → Resultado)", "param": "target"},
            {"scene": place, "entity": id(17), "name": "Placar", "role": "ui"},
        ])
    );
    assert!(panel.get("referenced_by_total").is_none());
    // Another object's logic moves the disc; its own animation is not a reference.
    let disc = query::entity(&project, &scene, &id(4)).unwrap();
    assert_eq!(
        disc["referenced_by"],
        json!([{"scene": place, "entity": id(6), "name": "Controle",
                "role": "action.animation", "node": "action.animation(Giro → Disco)",
                "param": "target"}])
    );
    // The door's animation moves its leaf.
    assert_eq!(
        query::entity(&project, &scene, &id(21)).unwrap()["referenced_by"],
        json!([{"scene": place, "entity": id(20), "name": "Porta", "role": "animation",
                "clip": "Abrir"}])
    );
    // The preset's camera follows the player and hides its look.
    let player = &applied[19].created;
    let camera = json!({"scene": place, "entity": player[2], "name": "Câmera principal",
                        "role": "camera_control"});
    let mut target = camera.clone();
    target["param"] = json!("target");
    let mut hidden = camera;
    hidden["param"] = json!("hidden");
    let read = |object: &str| query::entity(&project, &scene, object).unwrap();
    assert_eq!(read(&player[0])["referenced_by"], json!([target]));
    assert_eq!(read(&player[1])["referenced_by"], json!([hidden]));
    // Nothing names the control: left out unless asked for.
    let control = read(&id(6));
    assert!(control.get("referenced_by").is_none(), "{control}");
    let asked = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"id": id(6), "fields": ["referenced_by"]})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        asked,
        json!({"id": id(6), "name": "Controle", "referenced_by": []})
    );
}

#[test]
fn keyframes_read_in_degrees_as_set_clip_writes_them() {
    let (project, scene, applied) = named_level();
    let disc = query::entity(&project, &scene, &applied[4].created[0]).unwrap();
    let keyframes = &disc["clips"][0]["tracks"][0]["keyframes"];
    near(&keyframes[0]["rotation_degrees"], &[0., 0., 0.]);
    near(&keyframes[1]["rotation_degrees"], &[0., 90., 0.]);
    near(
        &keyframes[1]["transform"]["rotation"],
        &[0., 90f32.to_radians(), 0.],
    );
    let picked = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"id": applied[4].created[0],
                                    "fields": ["clips.0.tracks.0.keyframes.1"]}))
        .unwrap(),
    )
    .unwrap();
    near(
        &picked["clips"]["0"]["tracks"]["0"]["keyframes"]["1"]["rotation_degrees"],
        &[0., 90., 0.],
    );
}

#[test]
fn reads_take_a_scene_by_name_and_fields_by_component_name() {
    let (mut project, scene, applied) = named_level();
    let menu = applied[0].created[0].clone();
    let tree = query::answer(&project, "MENU", "scene_tree", &json!({})).unwrap();
    assert_eq!((&tree["id"], &tree["name"]), (&json!(menu), &json!("Menu")));
    let placed = query::entity(&project, " cena 3d ", &applied[3].created[0]).unwrap();
    assert_eq!(placed["name"], "Resultado");
    let error = query::answer(&project, "Fase 9", "scene_tree", &json!({})).unwrap_err();
    assert_eq!(
        error,
        format!(
            "scene recebeu \"Fase 9\", que não é o ID nem o nome de uma cena do projeto. Cenas: \
             [{{\"id\":\"{scene}\",\"name\":\"Cena 3D\"}},{{\"id\":\"{menu}\",\"name\":\"Menu\"}}]"
        )
    );
    // Component names read the part of the document that holds them.
    let camera = applied[19].created[2].clone();
    let rig = query::entities(
        &project,
        &scene,
        &EntityQuery::parse(&json!({"id": camera, "fields": ["camera_control.target"]})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rig,
        json!({"id": camera, "name": "Câmera principal",
               "camera_rig": {"target": applied[19].created[0]}})
    );
    let parsed =
        EntityQuery::parse(&json!({"id": "a", "fields": ["sensor3d", "character"]})).unwrap();
    assert_eq!(parsed.fields, ["physics3d", "character3d"]);
    let error = EntityQuery::parse(&json!({"id": "a", "fields": ["sensor"]})).unwrap_err();
    assert!(
        error.contains("Campo desconhecido: sensor") && error.contains("sensor3d → physics3d"),
        "{error}"
    );
    // Two scenes with one name: the ID tells them apart.
    apply(
        &mut project,
        &scene,
        json!([{"op": "create_scene", "name": "menu", "kind": "2d"}]),
    );
    let error = query::answer(&project, "Menu", "scene_tree", &json!({})).unwrap_err();
    assert!(
        error.starts_with("scene recebeu \"Menu\", o nome de 2 cenas; use o ID: [{\"id\":"),
        "{error}"
    );
    assert!(query::answer(&project, &menu, "scene_tree", &json!({})).is_ok());
}

#[test]
fn labels_tell_apart_nodes_that_share_one_and_inputs_replace_references() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Placa"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Contagem"},
            {"op": "add_node", "entity": "$1", "operation": "attribute.set",
             "params": {"attribute": "Aviso", "value": {"Text": "3"}}},
            {"op": "add_node", "entity": "$1", "operation": "attribute.set",
             "params": {"attribute": "Aviso", "value": {"Text": "2"}}},
            {"op": "add_node", "entity": "$1", "operation": "attribute.set",
             "params": {"attribute": "Aviso", "value": {"Text": "3"}}},
            {"op": "add_node", "entity": "$1", "operation": "condition.branch"},
            {"op": "add_node", "entity": "$1", "operation": "condition.branch"},
            {"op": "add_node", "entity": "$1", "operation": "value.bool"},
            {"op": "connect", "entity": "$1", "from_node": "$7", "from_port": "value",
             "to_node": "$5", "to_port": "condition"},
            {"op": "connect", "entity": "$1", "from_node": "$7", "from_port": "value",
             "to_node": "$6", "to_port": "condition"},
            {"op": "add_node", "entity": "$1", "operation": "event.scene_start"},
            {"op": "add_node", "entity": "$1", "operation": "action.remove",
             "params": {"target": {"Object": "$0"}}},
            {"op": "connect", "entity": "$1", "from_node": "$10", "from_port": "self",
             "to_node": "$11", "to_port": "target"},
            {"op": "add_node", "entity": "$1", "operation": "action.component",
             "params": {"component": "visible", "target": {"Object": "$0"}}},
            {"op": "add_node", "entity": "$1", "operation": "action.component",
             "params": {"component": "visible", "enabled": false, "target": {"Object": "$0"}}},
        ]),
    );
    let id = |command: usize| applied[command].created[0].clone();
    let view = query::entity(&project, &scene, &id(1)).unwrap();
    let nodes = view["graph"]["nodes"].as_object().unwrap();
    let mut labels: Vec<&str> = nodes.keys().map(String::as_str).collect();
    labels.sort_unstable();
    assert_eq!(
        labels,
        [
            // What tells them apart, else their place in the graph.
            "action.component(visible → Placa, enabled = false)",
            "action.component(visible → Placa, enabled = true)",
            // Its target comes through the input.
            "action.remove",
            "attribute.set(Aviso, value = \"2\")",
            "attribute.set(Aviso, value = \"3\")#0",
            "attribute.set(Aviso, value = \"3\")#2",
            "condition.branch#3",
            "condition.branch#4",
            "event.scene_start",
            "value.bool(false)",
        ]
    );
    assert_eq!(
        nodes["attribute.set(Aviso, value = \"2\")"]["id"],
        json!(id(3))
    );
    // The removal's parameter names the panel, but the input decides: it is not a reference.
    let panel = query::entity(&project, &scene, &id(0)).unwrap();
    let place = json!({"id": scene, "name": "Cena 3D"});
    assert_eq!(
        panel["referenced_by"],
        json!([
            {"scene": place, "entity": id(1), "name": "Contagem", "role": "action.component",
             "node": "action.component(visible → Placa, enabled = true)", "param": "target"},
            {"scene": place, "entity": id(1), "name": "Contagem", "role": "action.component",
             "node": "action.component(visible → Placa, enabled = false)", "param": "target"},
        ])
    );
}

#[test]
fn many_references_and_many_scenes_are_counted_past_the_listed_ones() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let target = Entity::new("Alvo", None);
    let id = target.id.clone();
    let entities = &mut project.scene_mut(&scene).unwrap().entities;
    entities.push(target);
    for n in 0..51 {
        let mut pointer = Entity::new(format!("Seta {n}"), None);
        pointer
            .attributes
            .insert("Alvo".into(), Value::Object(Some(id.clone())));
        entities.push(pointer);
    }
    let read = query::entity(&project, &scene, &id).unwrap();
    let listed = read["referenced_by"].as_array().unwrap();
    assert_eq!(listed.len(), 50);
    assert_eq!(listed[49]["name"], "Seta 49");
    assert_eq!(read["referenced_by_total"], 51);
    // A scene the project lacks lists 30 scenes and counts the others.
    for n in 1..=31 {
        project
            .scenes
            .push(Scene::new(format!("Fase {n}"), SceneKind::ThreeD));
    }
    let error = query::answer(&project, "Fase 99", "scene_tree", &json!({})).unwrap_err();
    assert!(
        error.ends_with(&format!(
            "{{\"id\":\"{}\",\"name\":\"Fase 29\"}}] e mais 2",
            project.scenes[29].id
        )),
        "{error}"
    );
    assert!(!error.contains("Fase 30"), "{error}");
}
