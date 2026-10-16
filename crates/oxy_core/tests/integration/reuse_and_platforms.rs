//! Reuse across levels (models, copies between scenes, repeated duplicates), moving platforms
//! and animations authored by command, and presets and movement profiles that describe
//! themselves.
use oxy_core::{
    animation::{Interpolation, sample_clip},
    character::{CharacterConfig, MovementProfile, PROFILE_FIELDS},
    command::{self, Applied, Command},
    document::*,
    editing, graph,
    headless::{self, SimulationRequest},
    query,
};
use serde_json::{Value as Json, json};

fn project(kind: SceneKind) -> (Project, Id) {
    let project = editing::blank_project("Reuso", kind).unwrap();
    let scene = project.start_scene.clone();
    (project, scene)
}

fn batch(value: Json) -> Vec<Command> {
    command::parse(&value).unwrap()
}

fn apply(project: &mut Project, scene: &str, value: Json) -> Vec<Applied> {
    command::apply(project, scene, &batch(value)).unwrap()
}

fn refused(project: &mut Project, scene: &str, value: Json) -> String {
    let before = project.clone();
    let error = command::apply(project, scene, &batch(value)).unwrap_err();
    assert_eq!(*project, before, "a refused batch changes nothing");
    error.message
}

fn entity<'a>(project: &'a Project, scene: &str, id: &str) -> &'a Entity {
    project.scene(scene).unwrap().entity(id).unwrap()
}

/// Positions of the keyframes of the first track of the object's first clip.
fn key_positions(project: &Project, scene: &str, id: &str) -> Vec<[f32; 3]> {
    entity(project, scene, id).clips[0].tracks[0]
        .keyframes
        .iter()
        .map(|k| k.transform.position)
        .collect()
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

#[test]
fn duplicates_repeat_with_an_offset_and_answer_every_copy() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
             "name": "Degrau", "position": [0.0, 1.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Sphere"}, "name": "Moeda",
             "position": [0.0, 2.0, 0.0]},
            {"op": "duplicate_entities", "ids": ["$0"], "offset": [3.0, 0.5, 0.0], "count": 3},
            {"op": "duplicate_entities", "ids": ["$0", "$1"], "count": 2},
            {"op": "duplicate_entities", "ids": ["$1"]},
            {"op": "rename_entity", "entity": "$2.2", "name": "Último degrau"},
        ]),
    );
    let row = &applied[2].created;
    assert_eq!(row.len(), 3, "every copy is answered");
    for (k, id) in row.iter().enumerate() {
        let copy = entity(&project, &scene, id);
        let k = k as f32 + 1.;
        assert_eq!(copy.transform.position, [3. * k, 1. + 0.5 * k, 0.]);
    }
    assert_eq!(entity(&project, &scene, &row[0]).name, "Degrau (cópia 1)");
    assert_eq!(entity(&project, &scene, &row[2]).name, "Último degrau");
    // Two objects twice: copy after copy, the objects in order inside each.
    let pairs = &applied[3].created;
    let names: Vec<&str> = pairs
        .iter()
        .map(|id| entity(&project, &scene, id).name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Degrau (cópia 1)",
            "Moeda (cópia 1)",
            "Degrau (cópia 2)",
            "Moeda (cópia 2)"
        ]
    );
    assert_eq!(
        entity(&project, &scene, &pairs[3]).transform.position,
        [1., 2., 0.],
        "the default offset is half a metre along X per copy"
    );
    // One copy as before: half a metre along X, named (cópia).
    let single = entity(&project, &scene, &applied[4].created[0]);
    assert_eq!(single.transform.position, [0.5, 2., 0.]);
    assert_eq!(single.name, "Moeda (cópia)");

    let cube = applied[0].created[0].clone();
    for (count, expected) in [(0, "entre 1 e 100"), (101, "entre 1 e 100")] {
        let error = refused(
            &mut project,
            &scene,
            json!({"op": "duplicate_entities", "ids": [cube], "count": count}),
        );
        assert!(error.contains(expected), "{error}");
    }
    assert!(
        command::parse(&json!({"op": "duplicate_entities", "ids": [cube], "copies": 2})).is_err()
    );
}

#[test]
fn duplicated_platforms_keep_their_path_around_each_copy() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform"},
             "position": [0.0, 1.0, 0.0]},
            {"op": "duplicate_entities", "ids": ["$0"], "offset": [0.0, 0.0, -6.0], "count": 2},
        ]),
    );
    let second = &applied[1].created[1];
    assert_eq!(
        key_positions(&project, &scene, second),
        [[0., 1., -12.], [4., 1., -12.], [0., 1., -12.]]
    );
    validate_project(&project).unwrap();
}

#[test]
fn moving_an_animated_object_carries_its_keyframes() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform",
             "period": 2.0}, "position": [0.0, 1.0, 0.0]},
            {"op": "set_transform", "entity": "$0", "position": [10.0, 1.0, 0.0]},
        ]),
    );
    let platform = applied[0].created[0].clone();
    assert_eq!(
        key_positions(&project, &scene, &platform),
        [[10., 1., 0.], [14., 1., 0.], [10., 1., 0.]],
        "its own animation moves it, so the path follows by default"
    );
    // The game plays the path around the new place instead of snapping back to the old one.
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 0.5, "watch": [platform],
    }))
    .unwrap();
    let report = headless::simulate(&project, &request).unwrap();
    let at = report.watched[0].samples.last().unwrap().position.unwrap();
    assert!(
        (at[0] - 12.).abs() < 0.2 && (at[1] - 1.).abs() < 1e-3,
        "{at:?}"
    );

    // Opting out keeps the keyframes; opting in reaches tracks of other clips.
    apply(
        &mut project,
        &scene,
        json!({"op": "set_transform", "entity": platform, "position": [20.0, 1.0, 0.0],
               "move_tracks": false}),
    );
    assert_eq!(key_positions(&project, &scene, &platform)[0], [10., 1., 0.]);

    // Turning carries the path around the rest position; scaling resizes the keyframes only.
    apply(
        &mut project,
        &scene,
        json!([
            {"op": "set_transform", "entity": platform, "position": [10.0, 1.0, 0.0],
             "move_tracks": false},
            {"op": "set_transform", "entity": platform, "rotation": [0.0, 60.0, 0.0]},
            {"op": "set_transform", "entity": platform, "scale": [2.0, 1.0, 1.0]},
        ]),
    );
    let keys = &entity(&project, &scene, &platform).clips[0].tracks[0].keyframes;
    assert!(near(keys[0].transform.position, [10., 1., 0.]));
    assert!(
        near(keys[1].transform.position, [12., 1., -12f32.sqrt()]),
        "{:?}",
        keys[1].transform.position
    );
    for key in keys {
        assert!(near(key.transform.rotation, [0., 60f32.to_radians(), 0.]));
        assert_eq!(key.transform.scale, [2., 1., 1.]);
    }

    // A piece animated by its parent's clip keeps its keyframes unless asked.
    let rig = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Folha",
             "parent": "$0"},
            {"op": "set_clip", "entity": "$0", "name": "Abrir", "duration": 1.0,
             "tracks": [{"target": "$1", "keyframes": [{"time": 0.0},
                        {"time": 1.0, "position": [0.0, 0.0, 2.0]}]}]},
            {"op": "set_transform", "entity": "$1", "position": [1.0, 0.0, 0.0]},
        ]),
    );
    let (door, leaf) = (&rig[0].created[0], &rig[1].created[0]);
    assert_eq!(key_positions(&project, &scene, door)[0], [0., 0., 0.]);
    apply(
        &mut project,
        &scene,
        json!({"op": "set_transform", "entity": leaf, "position": [2.0, 0.0, 0.0],
               "move_tracks": true}),
    );
    assert_eq!(
        key_positions(&project, &scene, door),
        [[1., 0., 0.], [1., 0., 2.]]
    );
    validate_project(&project).unwrap();
}

#[test]
fn editor_moves_carry_keyframes_through_the_shared_rule() {
    use glam::{Mat4, Vec3};
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform"}},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Porta"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "parent": "$1"},
            {"op": "set_clip", "entity": "$1", "name": "Abrir", "duration": 1.0,
             "tracks": [{"target": "$2", "keyframes": [{"time": 0.0}]}]},
        ]),
    );
    let platform = applied[0].created[0].clone();
    let (door, leaf) = (applied[1].created[0].clone(), applied[2].created[0].clone());
    let start = key_positions(&project, &scene, &platform);
    let s = project.scene_mut(&scene).unwrap();
    // The gizmo and the multi-selection move through transform_selection.
    editing::transform_selection(
        s,
        &[platform.clone(), leaf.clone()],
        Mat4::from_translation(Vec3::new(0., 0., 5.)),
    )
    .unwrap();
    let moved: Vec<[f32; 3]> = start.iter().map(|p| [p[0], p[1], p[2] + 5.]).collect();
    assert_eq!(key_positions(&project, &scene, &platform), moved);
    assert_eq!(
        key_positions(&project, &scene, &door),
        [[0., 0., 0.]],
        "a piece of a rig keeps its keyframes"
    );
    // The panels write the transform and then let the keyframes follow.
    let s = project.scene_mut(&scene).unwrap();
    let rest = s.entity(&platform).unwrap().transform.clone();
    s.entity_mut(&platform).unwrap().transform.position[1] += 2.;
    editing::follow_tracks(s, &platform, &rest);
    let mut transform = s.entity(&platform).unwrap().transform.clone();
    transform.position[0] -= 1.;
    editing::set_local_transform(s, &platform, transform);
    let expected: Vec<[f32; 3]> = moved.iter().map(|p| [p[0] - 1., p[1] + 2., p[2]]).collect();
    let keys = key_positions(&project, &scene, &platform);
    assert!(
        keys.iter().zip(&expected).all(|(a, b)| near(*a, *b)),
        "{keys:?}"
    );
    // The keyframe at the rest position lands exactly on the new one.
    assert_eq!(
        keys[0],
        entity(&project, &scene, &platform).transform.position
    );
}

#[test]
fn clips_are_authored_replaced_played_and_deleted_by_command() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Elevador",
             "position": [0.0, 1.0, 0.0]},
            {"op": "update_entity", "entity": "$0", "patch": {"transform": {"pivot": [0.0, 0.5, 0.0]},
             "dimensions": [2.0, 0.2, 2.0]}},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Grade",
             "parent": "$0", "position": [0.0, 0.5, 0.0]},
            {"op": "set_clip", "entity": "$0", "name": "Subir", "duration": 2.0, "looping": true,
             "tracks": [
                {"keyframes": [{"time": 1.0, "position": [0.0, 3.0, 0.0]}, {"time": 0.0},
                               {"time": 2.0, "interpolation": "hold"}]},
                {"target": "$2", "keyframes": [{"time": 0.0},
                                               {"time": 2.0, "rotation": [90.0, 0.0, 0.0]}]},
             ],
             "events": [{"time": 1.0, "name": "Topo"}]},
            {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
            {"op": "add_node", "entity": "$0", "operation": "action.animation",
             "params": {"clip": {"Text": "$3"}}},
            {"op": "connect", "entity": "$0", "from_node": "$4", "from_port": "exec",
             "to_node": "$5", "to_port": "exec"},
        ]),
    );
    let lift = applied[0].created[0].clone();
    let clip_id = applied[3].created[0].clone();
    let report = command::report(&project, &applied, &[]);
    assert_eq!(report["created"]["3"][0]["name"], "Subir");
    let owner = entity(&project, &scene, &lift);
    let clip = &owner.clips[0];
    assert_eq!(
        (clip.id.as_str(), clip.duration, clip.looping),
        (clip_id.as_str(), 2., true)
    );
    let keys = &clip.tracks[0].keyframes;
    assert_eq!(
        keys.iter().map(|k| k.time).collect::<Vec<_>>(),
        [0., 1., 2.],
        "sorted by time"
    );
    // Channels left out are the rest transform, pivot included.
    assert_eq!(keys[0].transform, owner.transform);
    assert_eq!(keys[1].transform.position, [0., 3., 0.]);
    assert_eq!(keys[1].transform.pivot, [0., 0.5, 0.]);
    assert_eq!(keys[2].interpolation, Interpolation::Hold);
    assert_eq!(keys[0].interpolation, Interpolation::Linear);
    let turn = &clip.tracks[1].keyframes[1].transform;
    assert!((turn.rotation[0] - 90f32.to_radians()).abs() < 1e-6);
    assert_eq!(turn.position, [0., 0.5, 0.]);
    assert_eq!(clip.events[0].name, "Topo");
    let mut preview = project.scene(&scene).unwrap().clone();
    sample_clip(&mut preview, clip, 0.5);
    assert_eq!(
        preview.entity(&lift).unwrap().transform.position,
        [0., 2., 0.]
    );
    validate_project(&project).unwrap();

    // The game plays it from the logic node.
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 1.0, "watch": [lift],
    }))
    .unwrap();
    let report = headless::simulate(&project, &request).unwrap();
    let at = report.watched[0].samples.last().unwrap().position.unwrap();
    // Its origin, half a metre under the pivot that the keyframe put at 3 m.
    assert!((at[1] - 2.5).abs() < 0.2, "{at:?}");

    // Replacing by name keeps the ID the node uses and creates nothing.
    let replaced = apply(
        &mut project,
        &scene,
        json!({"op": "set_clip", "entity": lift, "clip": "subir", "name": "Subir devagar",
               "duration": 4.0, "tracks": [{"target": "self", "keyframes": [{"time": 4.0}]}]}),
    );
    assert!(replaced[0].created.is_empty());
    let clip = &entity(&project, &scene, &lift).clips[0];
    assert_eq!(
        (clip.id.as_str(), clip.name.as_str(), clip.looping),
        (clip_id.as_str(), "Subir devagar", false)
    );
    assert_eq!(entity(&project, &scene, &lift).clips.len(), 1);

    let set_clip = |tracks: Json, extra: Json| {
        let mut command = json!({"op": "set_clip", "entity": lift, "name": "Outra",
                                 "duration": 1.0, "tracks": tracks});
        command
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        command
    };
    let other = apply(
        &mut project,
        &scene,
        json!({"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}}),
    )[0]
    .created[0]
        .clone();
    for (command, expected) in [
        (
            set_clip(
                json!([{"target": other, "keyframes": [{"time": 0.0}]}]),
                json!({}),
            ),
            "dentro de Elevador",
        ),
        (
            set_clip(
                json!([{"keyframes": [{"time": 0.0}]}, {"target": lift, "keyframes": [{"time": 1.0}]}]),
                json!({}),
            ),
            "Duas trilhas animam Elevador",
        ),
        (
            set_clip(json!([{"keyframes": [{"time": 1.5}]}]), json!({})),
            "fora da animação",
        ),
        (
            set_clip(
                json!([{"keyframes": [{"time": 0.5}, {"time": 0.5}]}]),
                json!({}),
            ),
            "dois quadros-chave no tempo 0.5",
        ),
        (
            set_clip(json!([{"keyframes": []}]), json!({})),
            "não tem quadros-chave",
        ),
        (
            set_clip(json!([]), json!({"name": "subir devagar"})),
            "já tem uma animação chamada",
        ),
        (set_clip(json!([]), json!({"duration": 0.0})), "maior que 0"),
        (
            set_clip(json!([]), json!({"clip": "Pular"})),
            "não tem a animação Pular",
        ),
        (
            set_clip(json!([]), json!({"events": [{"time": 2.0, "name": "Fim"}]})),
            "Marcador em 2 s",
        ),
        (
            set_clip(
                json!([{"keyframes": [{"time": 0.0, "scale": [0.0, 1.0, 1.0]}]}]),
                json!({}),
            ),
            "Escala não pode ser zero",
        ),
    ] {
        let error = refused(&mut project, &scene, command);
        assert!(error.contains(expected), "{expected}: {error}");
    }
    assert!(
        command::parse(&set_clip(
            json!([{"keyframes": [{"time": 0.0, "rotacao": [0, 0, 0]}]}]),
            json!({})
        ))
        .is_err()
    );
    assert!(
        command::parse(&set_clip(
            json!([{"keyframes": [{"time": 0.0, "interpolation": "ease"}]}]),
            json!({})
        ))
        .is_err()
    );

    // Deleting waits until no node plays the clip.
    let play = applied[5].created[0].clone();
    let error = refused(
        &mut project,
        &scene,
        json!({"op": "delete_clip", "entity": lift, "clip": clip_id}),
    );
    assert!(
        error.contains("Reproduzir animação") && error.contains(&play),
        "{error}"
    );
    apply(
        &mut project,
        &scene,
        json!([
            {"op": "remove_node", "entity": lift, "node": play},
            {"op": "delete_clip", "entity": lift, "clip": "Subir devagar"},
        ]),
    );
    assert!(entity(&project, &scene, &lift).clips.is_empty());
}

#[test]
fn copies_go_to_another_level_keeping_their_place_and_their_references() {
    let (mut project, first) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Seção",
             "position": [10.0, 0.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Bloco",
             "parent": "$1", "position": [1.0, 2.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Alavanca",
             "parent": "$1"},
            {"op": "set_attribute", "entity": "$3", "name": "porta", "value": {"Object": "$2"}},
            {"op": "add_node", "entity": "$3", "operation": "action.remove",
             "params": {"target": {"Object": "$2"}}},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform"},
             "position": [0.0, 1.0, 0.0]},
        ]),
    );
    let second = applied[0].created[0].clone();
    let section = applied[1].created[0].clone();
    let block = applied[2].created[0].clone();
    let lever = applied[3].created[0].clone();
    let platform = applied[6].created[0].clone();

    // A piece taken out of its group keeps its place in the world.
    let copied = apply(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [block], "to_scene": second}),
    );
    let copy = entity(&project, &second, &copied[0].created[0]);
    assert_eq!((copy.name.as_str(), copy.parent.as_ref()), ("Bloco", None));
    assert_eq!(copy.transform.position, [11., 2., 0.]);

    // References must stay inside the copy, or the copy is refused.
    let error = refused(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [lever], "to_scene": second}),
    );
    assert!(
        error.contains("fora da cópia") && error.contains("atributo porta → Bloco"),
        "{error}"
    );
    assert!(error.contains("nó action.remove (target)"), "{error}");
    // In the same scene they still exist.
    apply(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [lever], "to_scene": first}),
    );
    // The whole section goes along with its references, remapped to the copies.
    let applied = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_entity", "scene": second, "kind": {"type": "group"}, "name": "Ala B",
             "position": [0.0, 5.0, 0.0]},
            {"op": "copy_entities", "ids": [section], "to_scene": second, "parent": "$0"},
        ]),
    );
    let section_copy = &applied[1].created[0];
    let level = project.scene(&second).unwrap();
    let copy = level.entity(section_copy).unwrap();
    assert!(
        near(copy.transform.position, [10., -5., 0.]),
        "same world place"
    );
    let children = level.descendants(section_copy);
    assert_eq!(children.len(), 3);
    let lever_copy = level
        .entities
        .iter()
        .find(|e| e.name == "Alavanca" && children.contains(&e.id))
        .unwrap();
    let block_copy = lever_copy.attributes["porta"].object().unwrap();
    assert!(children.iter().any(|id| id == block_copy));
    assert_eq!(
        lever_copy.graph.nodes[0].params["target"].object(),
        Some(block_copy)
    );

    // A moving platform keeps playing its own path, the node playing the copied clip.
    let moved = apply(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [platform], "to_scene": second,
               "parent": applied[0].created[0]}),
    );
    let level = project.scene(&second).unwrap();
    let copy = level.entity(&moved[0].created[0]).unwrap();
    assert!(near(copy.transform.position, [0., -4., 0.]));
    let keys = key_positions(&project, &second, &moved[0].created[0]);
    assert!(
        keys.iter()
            .zip([[0., -4., 0.], [4., -4., 0.], [0., -4., 0.]])
            .all(|(a, b)| near(*a, b)),
        "{keys:?}"
    );
    assert_eq!(copy.graph.nodes[1].text("clip"), copy.clips[0].id);
    validate_project(&project).unwrap();

    // Shapes go only to scenes of their kind.
    let flat = apply(
        &mut project,
        &first,
        json!({"op": "create_scene", "name": "Menu", "kind": "2d"}),
    )[0]
    .created[0]
        .clone();
    let error = refused(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [block], "to_scene": flat}),
    );
    assert!(error.contains("forma 3D"), "{error}");
    let error = refused(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [block], "to_scene": second, "parent": block}),
    );
    assert!(error.contains("não existe na cena Fase 2"), "{error}");
}

/// World matrices of the poses that the keyframes of the object's first track give it.
fn world_keys(project: &Project, scene: &str, id: &str) -> Vec<glam::Mat4> {
    let level = project.scene(scene).unwrap();
    let e = level.entity(id).unwrap();
    let parent = e
        .parent
        .as_deref()
        .map_or(glam::Mat4::IDENTITY, |p| level.world_matrix(p).unwrap());
    e.clips[0].tracks[0]
        .keyframes
        .iter()
        .map(|k| {
            let pose = Transform {
                pivot: e.transform.pivot,
                ..k.transform.clone()
            };
            parent * pose.matrix()
        })
        .collect()
}

fn same_world_path(project: &Project, (from, original): (&str, &str), (to, copy): (&str, &str)) {
    let expected = world_keys(project, from, original);
    let got = world_keys(project, to, copy);
    assert_eq!(expected.len(), got.len());
    for (a, b) in expected.iter().zip(&got) {
        assert!(a.abs_diff_eq(*b, 1e-4), "{a:?}\n{b:?}");
    }
}

#[test]
fn copies_under_scaled_or_turned_parents_keep_the_world_path() {
    let (mut project, first) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Setor",
             "position": [5.0, 0.0, 0.0]},
            {"op": "set_transform", "entity": "$1", "scale": [2.0, 2.0, 2.0],
             "rotation": [0.0, 60.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform"},
             "parent": "$1", "position": [1.0, 0.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cylinder"}, "name": "Roda",
             "parent": "$1", "position": [0.0, 0.0, 2.0]},
            {"op": "set_clip", "entity": "$4", "name": "Girar", "duration": 3.0, "looping": true,
             "tracks": [{"keyframes": [{"time": 0.0},
                                       {"time": 1.0, "rotation": [0.0, 120.0, 0.0]},
                                       {"time": 2.0, "rotation": [0.0, 240.0, 0.0]},
                                       {"time": 3.0, "rotation": [0.0, 360.0, 0.0]}]}]},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform"},
             "position": [0.0, 1.0, 0.0]},
            {"op": "create_entity", "scene": "$0", "kind": {"type": "group"}, "name": "Ala",
             "position": [0.0, 3.0, 0.0]},
            {"op": "set_transform", "scene": "$0", "entity": "$7", "scale": [0.5, 0.5, 0.5],
             "rotation": [0.0, 0.0, 45.0]},
            {"op": "create_entity", "scene": "$0", "kind": {"type": "group"}, "name": "Esticado"},
            {"op": "set_transform", "scene": "$0", "entity": "$9", "scale": [2.0, 1.0, 1.0]},
        ]),
    );
    let second = applied[0].created[0].clone();
    let inner = applied[3].created[0].clone();
    let wheel = applied[4].created[0].clone();
    let outer = applied[6].created[0].clone();
    let wing = applied[7].created[0].clone();
    let stretched = applied[9].created[0].clone();
    let copied = apply(
        &mut project,
        &first,
        json!([
            {"op": "copy_entities", "ids": [inner], "to_scene": second},
            {"op": "copy_entities", "ids": [outer], "to_scene": second, "parent": wing},
            {"op": "copy_entities", "ids": [wheel], "to_scene": second, "parent": wing},
            {"op": "copy_entities", "ids": [wheel], "to_scene": second},
            {"op": "copy_entities", "ids": [outer], "to_scene": second, "parent": stretched},
        ]),
    );
    let copy = |i: usize| copied[i].created[0].clone();
    // Out of a group twice as big: the path is twice as long in the copy's own space, 8 m.
    same_world_path(&project, (&first, &inner), (&second, &copy(0)));
    let keys = key_positions(&project, &second, &copy(0));
    let length = glam::Vec3::from(keys[1]).distance(glam::Vec3::from(keys[0]));
    assert!((length - 8.).abs() < 1e-4, "{keys:?}");
    // Into a group half as big and turned, or stretched along X: the same 4 m in the world.
    same_world_path(&project, (&first, &outer), (&second, &copy(1)));
    same_world_path(&project, (&first, &outer), (&second, &copy(4)));
    // A turntable turns with its parent's space and keeps reading its own turns.
    same_world_path(&project, (&first, &wheel), (&second, &copy(2)));
    same_world_path(&project, (&first, &wheel), (&second, &copy(3)));
    let level = project.scene(&second).unwrap();
    let turned = level.entity(&copy(3)).unwrap();
    let read = |r: [f32; 3]| r.map(f32::to_degrees);
    assert!(near(read(turned.transform.rotation), [0., 60., 0.]));
    for (key, y) in turned.clips[0].tracks[0]
        .keyframes
        .iter()
        .zip([60., 180., 300., 420.])
    {
        let angles = read(key.transform.rotation);
        assert!(
            angles
                .iter()
                .zip([0., y, 0.])
                .all(|(a, b)| (a - b).abs() < 1e-3),
            "{angles:?}"
        );
    }
    validate_project(&project).unwrap();

    // A turning keyframe under a stretched parent would need shear: refused, nothing copied.
    let top = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cylinder"}, "name": "Pião"},
            {"op": "set_clip", "entity": "$0", "name": "Girar", "duration": 1.0,
             "tracks": [{"keyframes": [{"time": 0.0},
                                       {"time": 1.0, "rotation": [0.0, 120.0, 0.0]}]}]},
        ]),
    )[0]
    .created[0]
        .clone();
    let error = refused(
        &mut project,
        &first,
        json!({"op": "copy_entities", "ids": [top], "to_scene": second, "parent": stretched}),
    );
    assert!(
        error.contains("A animação de Pião") && error.contains("cisalhamento"),
        "{error}"
    );
}

#[test]
fn turned_keyframes_read_their_own_angles_turned() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let wheel = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cylinder"}},
            {"op": "set_clip", "entity": "$0", "name": "Girar", "duration": 5.0,
             "tracks": [{"keyframes": [{"time": 0.0},
                                       {"time": 1.0, "rotation": [0.0, 120.0, 0.0]},
                                       {"time": 2.0, "rotation": [0.0, 240.0, 0.0]},
                                       {"time": 3.0, "rotation": [0.0, 360.0, 0.0]},
                                       {"time": 4.0, "rotation": [0.0, -360.0, 0.0]},
                                       {"time": 5.0, "rotation": [0.0, 720.0, 0.0]}]}]},
            {"op": "set_transform", "entity": "$0", "rotation": [0.0, 30.0, 0.0]},
        ]),
    )[0]
    .created[0]
        .clone();
    let keys = &entity(&project, &scene, &wheel).clips[0].tracks[0].keyframes;
    let expected = [30., 150., 270., 390., -330., 750.];
    for (key, y) in keys.iter().zip(expected) {
        let angles = key.transform.rotation.map(f32::to_degrees);
        assert!(
            angles
                .iter()
                .zip([0., y, 0.])
                .all(|(a, b)| (a - b).abs() < 1e-3),
            "{y}: {angles:?}"
        );
    }
    // Ten more degrees on a keyframe now turn it ten degrees more, the way it reads.
    let mut key = keys[1].transform.clone();
    key.rotation[1] += 10f32.to_radians();
    let turned = glam::Quat::from_rotation_y(160f32.to_radians());
    assert!(key.quaternion().dot(turned).abs() > 0.99999);
}

#[test]
fn models_are_saved_and_placed_in_any_level() {
    let (mut project, first) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Torre",
             "position": [7.0, 0.0, 3.0]},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cylinder"},
             "name": "Base", "parent": "$1"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cone"}, "name": "Topo",
             "parent": "$1", "position": [0.0, 2.0, 0.0]},
            {"op": "add_node", "entity": "$1", "operation": "action.remove",
             "params": {"target": {"Object": "$3"}}},
            {"op": "save_model", "entity": "$1"},
        ]),
    );
    let second = applied[0].created[0].clone();
    let tower = applied[1].created[0].clone();
    let model = applied[5].created[0].clone();
    let report = command::report(&project, &applied, &[]);
    assert_eq!(
        report["created"]["5"][0]["name"], "Torre",
        "named after the object"
    );
    assert_eq!(project.asset(&model).unwrap().kind, AssetKind::Model);

    let placed = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_entity", "scene": second, "kind": {"type": "group"}, "name": "Pátio",
             "position": [0.0, 0.0, -5.0]},
            {"op": "place_model", "scene": second, "model": "torre", "parent": "$0",
             "position": [2.0, 0.0, 0.0], "name": "Torre norte"},
            {"op": "set_transform", "scene": second, "entity": "$1.2", "scale": [1.0, 2.0, 1.0]},
        ]),
    );
    let pieces = &placed[1].created;
    assert_eq!(pieces.len(), 3, "every piece, the root first");
    let level = project.scene(&second).unwrap();
    let root = level.entity(&pieces[0]).unwrap();
    assert_eq!(
        (
            root.name.as_str(),
            root.parent.as_ref(),
            root.transform.position
        ),
        ("Torre norte", Some(&placed[0].created[0]), [2., 0., 0.])
    );
    assert!(
        pieces
            .iter()
            .all(|id| level.entity(id).unwrap().model_source.as_ref() == Some(&model))
    );
    let top = level.entity(&pieces[2]).unwrap();
    assert_eq!(
        (top.name.as_str(), top.transform.scale),
        ("Topo", [1., 2., 1.])
    );
    assert_eq!(
        root.graph.nodes[0].params["target"].object(),
        Some(top.id.as_str()),
        "logic points to the placed pieces"
    );
    // The original stays where it was.
    assert_eq!(
        entity(&project, &first, &tower).transform.position,
        [7., 0., 3.]
    );

    // A platform saved as a model keeps its path around where it is placed.
    let platform = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform"},
             "position": [30.0, 1.0, 0.0]},
            {"op": "save_model", "entity": "$0", "name": "Plataforma padrão"},
            {"op": "place_model", "scene": second, "model": "$1", "position": [0.0, 3.0, 0.0]},
        ]),
    );
    assert_eq!(
        key_positions(&project, &second, &platform[2].created[0]),
        [[0., 3., 0.], [4., 3., 0.], [0., 3., 0.]]
    );

    // References out of the model, unknown or repeated names and the wrong kind of scene are
    // refused.
    let outside = apply(
        &mut project,
        &first,
        json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Alvo"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Gatilho"},
            {"op": "set_attribute", "entity": "$1", "name": "alvo", "value": {"Object": "$0"}},
        ]),
    );
    let error = refused(
        &mut project,
        &first,
        json!({"op": "save_model", "entity": outside[1].created[0]}),
    );
    assert!(
        error.contains("fora do modelo") && error.contains("atributo alvo → Alvo"),
        "{error}"
    );
    // place_model takes models by name, so save_model refuses a name another model has.
    let error = refused(
        &mut project,
        &first,
        json!({"op": "save_model", "entity": tower, "name": " torre "}),
    );
    assert_eq!(
        error,
        format!("Já existe um modelo chamado \"Torre\" ({model}); escolha outro nome com name")
    );
    // Every way of saving keeps the names apart: the editor saves under the object's name,
    // with a number when a model has it.
    let error = project.save_model(&first, &tower, "TORRE").unwrap_err();
    assert!(
        error.contains("Já existe um modelo chamado \"Torre\""),
        "{error}"
    );
    assert_eq!(project.free_model_name("Torre"), "Torre 2");
    let numbered = project.save_model(&first, &tower, "Torre 2").unwrap();
    assert_eq!(project.free_model_name(" torre "), "torre 3");
    assert_eq!(project.free_model_name("Castelo"), "Castelo");
    project.assets.retain(|a| a.id != numbered);
    let error = refused(
        &mut project,
        &first,
        json!({"op": "place_model", "model": "Castelo"}),
    );
    assert!(
        error.contains("Modelo Castelo não existe no projeto. Modelos: Torre (")
            && error.contains("Plataforma padrão ("),
        "the error lists the models: {error}"
    );
    // Projects saved before may still have two; by name, place_model refuses both.
    let mut twin = project.asset(&model).unwrap().clone();
    twin.id = new_id();
    project.assets.push(twin);
    let error = refused(
        &mut project,
        &first,
        json!({"op": "place_model", "model": "Torre"}),
    );
    assert!(error.contains("2 modelos se chamam \"Torre\""), "{error}");
    project.assets.pop();
    let error = refused(
        &mut project,
        &first,
        json!({"op": "save_model", "entity": tower, "name": "  "}),
    );
    assert!(error.contains("nome para o modelo"), "{error}");
    let menu = apply(
        &mut project,
        &first,
        json!({"op": "create_scene", "name": "Menu", "kind": "2d"}),
    )[0]
    .created[0]
        .clone();
    let error = refused(
        &mut project,
        &first,
        json!({"op": "place_model", "scene": menu, "model": model}),
    );
    assert!(error.contains("a cena Menu é 2D"), "{error}");
    validate_project(&project).unwrap();
}

#[test]
fn presets_take_a_path_a_period_and_a_camera_choice() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform",
             "travel": [0.0, 3.0, 0.0], "period": 6.0}, "position": [2.0, 0.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "first_person"}},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "third_person",
             "camera": "none"}, "position": [5.0, 0.0, 0.0]},
        ]),
    );
    let platform = entity(&project, &scene, &applied[0].created[0]);
    let clip = &platform.clips[0];
    assert_eq!(clip.duration, 6.);
    assert_eq!(
        clip.tracks[0]
            .keyframes
            .iter()
            .map(|k| (k.time, k.transform.position))
            .collect::<Vec<_>>(),
        [(0., [2., 0., 0.]), (3., [2., 3., 0.]), (6., [2., 0., 0.])]
    );
    // By default a character gets its own camera, third in its pieces.
    let player = &applied[1].created;
    assert_eq!(player.len(), 9);
    let camera = entity(&project, &scene, &player[2]);
    assert_eq!(camera.name, "Câmera principal");
    assert!(camera.camera.as_ref().unwrap().active);
    // Without a camera, no piece follows the player.
    let other = &applied[2].created;
    assert_eq!(other.len(), 8);
    assert!(
        other
            .iter()
            .all(|id| entity(&project, &scene, id).camera.is_none())
    );

    // Reusing: the scene's camera follows the new player and no second one appears.
    let (mut level, start) = self::project(SceneKind::ThreeD);
    let error = refused(
        &mut level,
        &start,
        json!({"op": "create_entity", "kind": {"type": "preset", "preset": "first_person",
               "camera": "reuse"}}),
    );
    assert!(error.contains("não tem câmera"), "{error}");
    let applied = apply(
        &mut level,
        &start,
        json!([
            {"op": "create_entity", "kind": {"type": "camera"}, "name": "Câmera da fase"},
            {"op": "update_entity", "entity": "$0", "patch": {"camera": {"active": false}}},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "third_person",
             "camera": "reuse"}},
        ]),
    );
    let camera = entity(&level, &start, &applied[0].created[0]);
    let rig = camera.camera_rig.as_ref().unwrap();
    assert_eq!(rig.target.as_ref(), Some(&applied[2].created[0]));
    assert_eq!(rig.hidden, [applied[2].created[1].clone()]);
    assert!(camera.camera.as_ref().unwrap().active);
    assert_eq!(
        level.scenes[0]
            .entities
            .iter()
            .filter(|e| e.camera.is_some())
            .count(),
        1
    );
    assert!(level.input_bindings.contains_key(&rig.mode_action));
    let error = refused(
        &mut level,
        &start,
        json!({"op": "create_entity", "kind": {"type": "preset", "preset": "first_person",
               "camera": "reuse"}}),
    );
    assert!(error.contains("já segue Jogador"), "{error}");
    validate_project(&level).unwrap();

    for (kind, expected) in [
        (
            json!({"type": "preset", "preset": "first_person", "travel": [1.0, 0.0, 0.0]}),
            "só para o preset platform",
        ),
        (
            json!({"type": "preset", "preset": "platform", "camera": "none"}),
            "first_person e third_person",
        ),
        (
            json!({"type": "preset", "preset": "platform", "period": 0.0}),
            "maior que 0",
        ),
    ] {
        let error = refused(
            &mut project,
            &scene,
            json!({"op": "create_entity", "kind": kind}),
        );
        assert!(error.contains(expected), "{error}");
    }
    assert!(
        command::parse(&json!({"op": "create_entity",
            "kind": {"type": "preset", "preset": "first_person", "camera": "both"}}))
        .is_err()
    );
}

#[test]
fn a_platform_preset_stops_at_its_ends_and_spins() {
    use glam::{Quat, Vec3};
    use oxy_core::{
        animation::sample_track,
        runtime::{FIXED_DT, InputFrame, Runtime},
    };
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform",
             "travel": [0.0, 0.0, -6.0], "period": 8.0, "pause_at_ends": 1.5},
             "position": [1.0, 0.5, 0.0]},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform",
             "travel": [0.0, 0.0, 0.0], "period": 12.0, "spin": [0.0, 360.0, 0.0]},
             "position": [10.0, 0.0, 0.0]},
            {"op": "create_entity", "kind": {"type": "preset", "preset": "platform",
             "period": 4.0, "pause_at_ends": 0.5, "spin": [0.0, -180.0, 0.0]}},
        ]),
    );
    let ids: Vec<Id> = applied.iter().map(|a| a.created[0].clone()).collect();
    let clip = |index: usize| entity(&project, &scene, &ids[index]).clips[0].clone();
    let yaw = |transform: &Transform| transform.rotation[1].to_degrees();

    // Out in 2.5 s, 1.5 s at the far end, back in 2.5 s, 1.5 s where it was put.
    let stopping = clip(0);
    assert_eq!(
        (stopping.name.as_str(), stopping.duration),
        ("Translação de ida e volta", 8.)
    );
    let keys: Vec<_> = stopping.tracks[0]
        .keyframes
        .iter()
        .map(|k| (k.time, k.transform.position[2], k.interpolation))
        .collect();
    assert_eq!(
        keys,
        [
            (0., 0., Interpolation::Linear),
            (2.5, -6., Interpolation::Hold),
            (4., -6., Interpolation::Linear),
            (6.5, 0., Interpolation::Hold),
            (8., 0., Interpolation::Linear),
        ]
    );

    // A turntable: a keyframe every 90°, the position kept.
    let turning = clip(1);
    assert_eq!((turning.name.as_str(), turning.duration), ("Giro", 12.));
    let track = &turning.tracks[0];
    let keys: Vec<_> = track
        .keyframes
        .iter()
        .map(|k| (k.time, yaw(&k.transform).round()))
        .collect();
    assert_eq!(
        keys,
        [(0., 0.), (3., 90.), (6., 180.), (9., 270.), (12., 360.)]
    );
    assert!(
        track
            .keyframes
            .iter()
            .all(|k| k.transform.position == [10., 0., 0.])
    );
    // Between keyframes it turns the short way, the same way all along.
    for (time, degrees) in [(1.5f32, 45f32), (7.5, 225.), (10.5, 315.)] {
        let pose = sample_track(track, time).unwrap();
        let wanted = Quat::from_rotation_y(degrees.to_radians());
        assert!(
            pose.quaternion().angle_between(wanted) < 1e-3,
            "{time}: {pose:?}"
        );
    }

    // Half a turn per period takes two periods to come back: the clip lasts both, and the
    // turn stands still at the stops.
    let half = clip(2);
    assert_eq!(
        (half.name.as_str(), half.duration),
        ("Translação de ida e volta com giro", 8.)
    );
    let keys: Vec<_> = half.tracks[0]
        .keyframes
        .iter()
        .map(|k| (k.time, k.transform.position[0], yaw(&k.transform).round()))
        .collect();
    assert_eq!(
        keys,
        [
            (0., 0., 0.),
            (1.5, 4., -90.),
            (2., 4., -90.),
            (3.5, 0., -180.),
            (4., 0., -180.),
            (5.5, 4., -270.),
            (6., 4., -270.),
            (7.5, 0., -360.),
            (8., 0., -360.),
        ]
    );
    // A turn backwards that has not begun reads 0, not -0.
    assert!(
        half.tracks[0]
            .keyframes
            .iter()
            .flat_map(|k| k.transform.rotation)
            .all(|v| v != 0. || v.is_sign_positive()),
        "{:?}",
        half.tracks[0].keyframes[0]
    );

    // The game plays them so.
    let mut runtime = Runtime::new(&project, &scene).unwrap();
    for _ in 0..(3. / FIXED_DT).round() as usize {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    let at = |id: &str| runtime.scene().entity(id).unwrap().transform.clone();
    assert!(
        near(at(&ids[0]).position, [1., 0.5, -6.]),
        "{:?}",
        at(&ids[0])
    );
    let turned = at(&ids[1]).quaternion();
    assert!(
        turned.angle_between(Quat::from_rotation_y(90f32.to_radians())) < 1e-2,
        "{turned:?}"
    );
    assert!(Vec3::from(at(&ids[1]).position).abs_diff_eq(Vec3::new(10., 0., 0.), 1e-4));
    validate_project(&project).unwrap();

    // Periods to come back with whole turns on every axis, up to 8.
    use oxy_core::movement_presets::spin_periods;
    assert_eq!(
        [
            [0., 360., 0.],
            [0., -180., 0.],
            [90., 45., 0.],
            [0., 120., 90.],
            [0., 100., 0.],
        ]
        .map(|spin| spin_periods(Vec3::from_array(spin))),
        [Some(1), Some(2), Some(8), None, None]
    );
    for (kind, expected) in [
        (
            json!({"type": "preset", "preset": "platform", "period": 4.0, "pause_at_ends": 2.0}),
            "menor que metade do período (4 s)",
        ),
        (
            json!({"type": "preset", "preset": "platform", "pause_at_ends": -1.0}),
            "a partir de 0",
        ),
        (
            json!({"type": "preset", "preset": "platform", "spin": [0.0, 100.0, 0.0]}),
            "voltar ao começo em até 8 períodos",
        ),
        (
            json!({"type": "preset", "preset": "platform", "spin": [0.0, 3960.0, 0.0]}),
            "passa de 3600° por período",
        ),
        (
            json!({"type": "preset", "preset": "third_person", "spin": [0.0, 90.0, 0.0]}),
            "travel, period, pause_at_ends e spin valem só para o preset platform",
        ),
    ] {
        let error = refused(
            &mut project,
            &scene,
            json!({"op": "create_entity", "kind": kind}),
        );
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn movement_profiles_are_applied_by_command_and_described_in_the_catalog() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "preset", "preset": "first_person"}},
            {"op": "apply_movement_profile", "entity": "$0", "profile": "chained"},
        ]),
    );
    let player = applied[0].created[0].clone();
    let mut expected = CharacterConfig::default();
    expected.apply_profile(MovementProfile::ChainedJumps);
    assert_eq!(
        entity(&project, &scene, &player).character3d.as_ref(),
        Some(&expected)
    );
    let camera = applied[0].created[2].clone();
    let error = refused(
        &mut project,
        &scene,
        json!({"op": "apply_movement_profile", "entity": camera, "profile": "parkour"}),
    );
    assert!(error.contains("não é um personagem 3D"), "{error}");
    assert!(
        command::parse(
            &json!({"op": "apply_movement_profile", "entity": player, "profile": "voar"})
        )
        .is_err()
    );

    let catalog = query::catalog();
    let profiles = catalog["movement_profiles"].as_array().unwrap();
    assert_eq!(
        profiles
            .iter()
            .map(|p| (p["id"].as_str().unwrap(), p["label"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [
            ("direct", "Direto"),
            ("parkour", "Ágil com embalo"),
            ("chained", "Saltos encadeados")
        ]
    );
    for profile in profiles {
        let values = profile["values"].as_object().unwrap();
        assert_eq!(values.len(), PROFILE_FIELDS.len());
        assert!(values.values().all(|v| !v.is_null()), "{values:?}");
    }
    assert_eq!(profiles[1]["values"]["slide_enabled"], true);
    assert_eq!(profiles[2]["values"]["air_acceleration"], 80.0);
    assert_eq!(profiles[0]["values"]["jump_mode"], "Manual");
    // The profile names commands, nodes and the catalog use are the same, and so are the labels
    // the node's choices show.
    for profile in MovementProfile::ALL {
        assert_eq!(MovementProfile::from_id(profile.id()), Some(profile));
        assert_eq!(serde_json::to_value(profile).unwrap(), profile.id());
    }
    assert_eq!(
        graph::choices("character.profile", "profile").unwrap(),
        MovementProfile::ALL.map(|profile| (profile.id(), profile.label()))
    );
    // Ágil com embalo was called Parkour: its ID is still `parkour`, in commands as in the
    // nodes of older projects.
    apply(
        &mut project,
        &scene,
        json!({"op": "apply_movement_profile", "entity": player, "profile": "parkour"}),
    );
    expected.apply_profile(MovementProfile::Parkour);
    assert_eq!(
        entity(&project, &scene, &player).character3d.as_ref(),
        Some(&expected)
    );
    graph::check_choices(
        "character.profile",
        [(&"profile".to_owned(), &Value::Text("parkour".into()))],
    )
    .unwrap();

    let presets = catalog["presets"].as_array().unwrap();
    let preset = |id: &str| presets.iter().find(|p| p["id"] == id).unwrap();
    let first_person = preset("first_person");
    let creates = first_person["creates"].as_array().unwrap();
    assert_eq!(creates.len(), 9, "{creates:?}");
    assert_eq!(creates[0], "Jogador (character)");
    assert_eq!(creates[2], "Câmera principal (camera_control, camera)");
    assert!(
        creates[3]
            .as_str()
            .unwrap()
            .contains("em Aparência editável")
    );
    assert_eq!(first_person["params"]["camera"]["default"], "create");
    assert_eq!(
        first_person["params"]["camera"]["choices"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let platform = preset("platform");
    assert!(
        platform["description"]
            .as_str()
            .unwrap()
            .contains("não é um personagem")
    );
    let creates = platform["creates"][0].as_str().unwrap();
    assert!(
        creates.contains("platform") && creates.contains("Translação de ida e volta"),
        "{creates}"
    );
    assert_eq!(
        platform["params"]["travel"]["default"],
        json!([4.0, 0.0, 0.0])
    );
    assert_eq!(platform["params"]["pause_at_ends"]["default"], 0.0);
    assert_eq!(
        platform["params"]["spin"]["default"],
        json!([0.0, 0.0, 0.0])
    );
    let spin = platform["params"]["spin"]["description"].as_str().unwrap();
    assert!(
        spin.contains("a cada 90° no máximo") && spin.contains("caminho mais curto"),
        "{spin}"
    );
    assert!(preset("third_person")["description"].is_string());
}

#[test]
fn copies_to_another_level_warn_about_logic_that_opens_the_level_they_left() {
    let (mut project, first) = project(SceneKind::ThreeD);
    let report = command::apply_and_report(
        &mut project,
        &first,
        &batch(json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Controle da fase"},
            {"op": "add_node", "entity": "$1", "operation": "action.scene",
             "params": {"scene": first}},
            // Another level is a choice, not a leftover.
            {"op": "add_node", "entity": "$1", "operation": "action.scene",
             "params": {"scene": "$0"}},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Filho", "parent": "$1"},
            {"op": "add_node", "entity": "$4", "operation": "action.scene",
             "params": {"scene": first}},
            {"op": "copy_entities", "ids": ["$1"], "to_scene": "$0"},
            // Into the same level, the scene it opens is its own.
            {"op": "copy_entities", "ids": ["$1"], "to_scene": first},
        ])),
    )
    .unwrap();
    let second = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let copy = report["created"]["6"][0]["id"].as_str().unwrap().to_owned();
    let warnings = report["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let warning = warnings[0].as_str().unwrap();
    let node = &entity(&project, &second, &copy).graph.nodes[0];
    assert_eq!(
        node.text("scene"),
        first,
        "the copy still opens the old level"
    );
    // A piece inside the copy comes with its ID, which the answer does not give.
    let child = project
        .scene(&second)
        .unwrap()
        .entities
        .iter()
        .find(|e| e.parent.as_deref() == Some(copy.as_str()))
        .unwrap();
    assert!(
        warning.starts_with("Comando 6 (copy_entities, o 7º do lote): estes nós copiados")
            && warning.contains("ainda nomeiam Cena 3D, a cena de onde vieram")
            && warning.contains(&format!("{{\"Text\": \"{second}\"}}"))
            && warning.contains(&format!(
                "Controle da fase ({copy}) → nó action.scene ({})",
                node.id
            ))
            && warning.contains(&format!(
                "Filho ({}) → nó action.scene ({})",
                child.id, child.graph.nodes[0].id
            )),
        "{warning}"
    );
    assert_eq!(warning.matches("nó action.scene").count(), 2, "{warning}");
}

#[test]
fn uses_name_the_model_holding_them_and_a_model_lists_its_placements() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    let report = command::apply_and_report(
        &mut project,
        &scene,
        &batch(json!([
            {"op": "create_input_action", "name": "Agir", "key": "G"},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Rig"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
             "name": "Corpo", "parent": "$1"},
            {"op": "add_node", "entity": "$2", "operation": "event.input",
             "params": {"action": {"Text": "$0"}}},
            {"op": "save_model", "entity": "$1", "name": "Modelo rig"},
            {"op": "delete_entities", "ids": ["$1"]},
        ])),
    )
    .unwrap();
    // What a later command removed keeps the name it was created with.
    assert_eq!(report["created"]["1"][0]["name"], "Rig");
    assert_eq!(report["created"]["2"][0]["name"], "Corpo");
    assert_eq!(report["created"]["3"][0]["name"], "event.input");
    assert_eq!(report["created"]["4"][0]["name"], "Modelo rig");
    let action = report["created"]["0"][0]["id"].as_str().unwrap().to_owned();
    let model = report["created"]["4"][0]["id"].as_str().unwrap().to_owned();
    let summary = |project: &Project| query::project_summary(project);
    let action_uses = |summary: &Json| {
        summary["input_actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == action.as_str())
            .unwrap()["used_by"]
            .clone()
    };
    // Only inside the model: scene reads do not reach it, and the answer says where it is.
    let inside = action_uses(&summary(&project));
    assert_eq!(inside.as_array().unwrap().len(), 1, "{inside}");
    assert_eq!(inside[0]["name"], "Corpo");
    assert_eq!(inside[0]["model"], "Modelo rig");

    let placed = apply(
        &mut project,
        &scene,
        json!([
            {"op": "place_model", "model": "Modelo rig", "position": [0.0, 0.0, 0.0]},
            {"op": "place_model", "model": model, "position": [5.0, 0.0, 0.0], "name": "Rig 2"},
        ]),
    );
    let uses = action_uses(&summary(&project));
    let models: Vec<&Json> = uses
        .as_array()
        .unwrap()
        .iter()
        .map(|u| &u["model"])
        .collect();
    assert_eq!(models, [&Json::Null, &Json::Null, &json!("Modelo rig")]);
    // The model lists each placement by its root, not every piece.
    let listed = summary(&project)["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == model.as_str())
        .unwrap()["used_by"]
        .clone();
    assert_eq!(
        listed,
        json!([
            {"entity": placed[0].created[0], "name": "Rig", "role": "model"},
            {"entity": placed[1].created[0], "name": "Rig 2", "role": "model"},
        ])
    );
}

/// The logic of `id`'s object: each node's operation, message and position, in order.
fn logic(project: &Project, scene: &str, id: &str) -> Vec<(String, String, [f32; 2])> {
    entity(project, scene, id)
        .graph
        .nodes
        .iter()
        .map(|n| {
            (
                n.operation.clone(),
                n.text("message").to_owned(),
                n.position,
            )
        })
        .collect()
}

#[test]
fn copies_are_reached_in_the_same_batch_by_their_original() {
    let (mut project, scene) = project(SceneKind::ThreeD);
    // A group with a child and logic, duplicated three times; the second copy's node and child,
    // and the third copy itself, are edited by their originals in the same batch.
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Marco"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"}, "name": "Base",
             "parent": "$0"},
            {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
            {"op": "add_node", "entity": "$0", "operation": "debug.message",
             "params": {"message": "Original"}},
            {"op": "connect", "entity": "$0", "from_node": "$2", "from_port": "exec",
             "to_node": "$3", "to_port": "exec"},
            {"op": "duplicate_entities", "ids": ["$0"], "offset": [5.0, 0.0, 0.0], "count": 3},
            {"op": "set_node_params", "entity": "$5.1", "node": "$5.1@$3",
             "params": {"message": "Segunda"}},
            {"op": "rename_entity", "entity": "$5.1@$1", "name": "Base da segunda"},
            {"op": "set_attribute", "entity": "$5.2@$0", "name": "ordem",
             "value": {"Number": 3}},
            {"op": "set_attribute", "entity": "$0", "name": "rótulo", "value": {"Text": "$$5@x"}},
        ]),
    );
    let original = applied[0].created[0].clone();
    let copies = applied[5].created.clone();
    assert_eq!(copies.len(), 3);
    let messages: Vec<String> = [&original, &copies[0], &copies[1], &copies[2]]
        .iter()
        .map(|id| logic(&project, &scene, id)[1].1.clone())
        .collect();
    assert_eq!(messages, ["Original", "Original", "Segunda", "Original"]);
    // The copies took the logic laid out, as the original is.
    let laid_out: Vec<[f32; 2]> = logic(&project, &scene, &original)
        .iter()
        .map(|n| n.2)
        .collect();
    assert_eq!(laid_out[0], [0., 0.]);
    for copy in &copies {
        let positions: Vec<[f32; 2]> = logic(&project, &scene, copy).iter().map(|n| n.2).collect();
        assert_eq!(positions, laid_out);
    }
    let child_of = |project: &Project, root: &str| {
        project
            .scene(&scene)
            .unwrap()
            .entities
            .iter()
            .find(|e| e.parent.as_deref() == Some(root))
            .unwrap()
            .name
            .clone()
    };
    assert_eq!(child_of(&project, &copies[1]), "Base da segunda");
    assert_eq!(child_of(&project, &copies[0]), "Base");
    assert_eq!(
        entity(&project, &scene, &copies[2]).attributes["ordem"],
        Value::Number(3.)
    );
    assert!(
        !entity(&project, &scene, &copies[1])
            .attributes
            .contains_key("ordem")
    );
    assert_eq!(
        entity(&project, &scene, &original).attributes["rótulo"],
        Value::Text("$5@x".into()),
        "\"$$\" still escapes"
    );

    // Originals by their own IDs, from a batch before; copies to another scene and copies of
    // the scene itself.
    let child = applied[1].created[0].clone();
    let message = applied[3].created[0].clone();
    let copy_of = |command: &str, id: &str| format!("${command}@{id}");
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_scene", "name": "Fase 2", "kind": "3d"},
            {"op": "copy_entities", "ids": [original], "to_scene": "$0"},
            {"op": "set_node_params", "scene": "$0", "entity": copy_of("1", &original),
             "node": copy_of("1", &message), "params": {"message": "Na fase 2"}},
            {"op": "rename_entity", "scene": "$0", "entity": copy_of("1", &child),
             "name": "Base da fase 2"},
            {"op": "duplicate_scene", "scene": scene},
            {"op": "set_node_params", "scene": "$4", "entity": copy_of("4", &original),
             "node": copy_of("4", &message), "params": {"message": "Na cópia da cena"}},
        ]),
    );
    let (second, copied) = (applied[0].created[0].clone(), applied[1].created[0].clone());
    assert_eq!(logic(&project, &second, &copied)[1].1, "Na fase 2");
    assert!(
        project
            .scene(&second)
            .unwrap()
            .entities
            .iter()
            .any(|e| e.name == "Base da fase 2" && e.parent.as_ref() == Some(&copied))
    );
    let third = applied[4].created[0].clone();
    let in_copy = project
        .scene(&third)
        .unwrap()
        .entities
        .iter()
        .find(|e| e.name == "Marco")
        .unwrap();
    assert_eq!(in_copy.graph.nodes[1].text("message"), "Na cópia da cena");
    assert_eq!(logic(&project, &scene, &original)[1].1, "Original");

    // place_model: the originals are the model's own pieces and nodes.
    let model = apply(
        &mut project,
        &scene,
        json!({"op": "save_model", "entity": original, "name": "Marco"}),
    )[0]
    .created[0]
        .clone();
    let pieces = project.asset(&model).unwrap().model.clone().unwrap();
    let root = pieces.iter().find(|e| e.parent.is_none()).unwrap();
    let model_message = root.graph.nodes[1].id.clone();
    let model_child = pieces
        .iter()
        .find(|e| e.parent.is_some())
        .unwrap()
        .id
        .clone();
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "place_model", "model": model, "position": [0.0, 0.0, 20.0]},
            {"op": "place_model", "model": model, "position": [0.0, 0.0, 30.0]},
            {"op": "set_node_params", "entity": "$1", "node": copy_of("1.1", &model_message),
             "params": {"message": "Colocado"}},
            {"op": "rename_entity", "entity": copy_of("1", &model_child),
             "name": "Base colocada"},
        ]),
    );
    let (first_placed, placed) = (&applied[0].created, &applied[1].created);
    assert_eq!(logic(&project, &scene, &placed[0])[1].1, "Colocado");
    assert_eq!(logic(&project, &scene, &first_placed[0])[1].1, "Original");
    assert_eq!(entity(&project, &scene, &placed[1]).name, "Base colocada");

    // Reads never reach inside a model, but save_model maps what it saved: in the batch that
    // saves it, "$P@$S@<id>" is the placed copy of the scene's own piece or node.
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "save_model", "entity": original, "name": "Marco salvo"},
            {"op": "place_model", "model": "$0", "position": [0.0, 0.0, 40.0]},
            {"op": "set_node_params", "entity": "$1", "node": format!("$1@$0@{message}"),
             "params": {"message": "Salvo e colocado"}},
            {"op": "rename_entity", "entity": format!("$1@$0@{child}"), "name": "Base salva"},
        ]),
    );
    let saved = project
        .asset(&applied[0].created[0])
        .unwrap()
        .model
        .as_ref()
        .unwrap();
    assert!(saved.iter().all(|piece| piece.name != "Base salva"));
    let placed = &applied[1].created;
    assert_eq!(logic(&project, &scene, &placed[0])[1].1, "Salvo e colocado");
    assert_eq!(entity(&project, &scene, &placed[1]).name, "Base salva");
    assert_eq!(logic(&project, &scene, &original)[1].1, "Original");

    // Two objects duplicated twice: the copies come round after round, each root in order
    // ([A1, B1, A2, B2]), and each round reaches what it copied of both.
    let applied = apply(
        &mut project,
        &scene,
        json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Poste"},
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
             "name": "Lâmpada", "parent": "$0"},
            {"op": "duplicate_entities", "ids": [original, "$0"], "count": 2},
            {"op": "rename_entity", "entity": "$2.1@$1", "name": "Lâmpada da primeira"},
            {"op": "set_node_params", "entity": "$2.2", "node": copy_of("2.2", &message),
             "params": {"message": "Segunda rodada"}},
        ]),
    );
    let rounds = &applied[2].created;
    assert_eq!(rounds.len(), 4);
    let names: Vec<&str> = rounds
        .iter()
        .map(|id| entity(&project, &scene, id).name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Marco (cópia 1)",
            "Poste (cópia 1)",
            "Marco (cópia 2)",
            "Poste (cópia 2)"
        ]
    );
    assert_eq!(child_of(&project, &rounds[1]), "Lâmpada da primeira");
    assert_eq!(child_of(&project, &rounds[3]), "Lâmpada");
    assert_eq!(logic(&project, &scene, &rounds[2])[1].1, "Segunda rodada");
    assert_eq!(logic(&project, &scene, &rounds[0])[1].1, "Original");

    // An original the command did not copy, or a command that copies nothing, is refused,
    // saying why, and nothing changes.
    let other = apply(
        &mut project,
        &scene,
        json!({"op": "create_entity", "kind": {"type": "group"}, "name": "Outro"}),
    )[0]
    .created[0]
        .clone();
    for (commands, expected) in [
        (
            json!([{"op": "duplicate_entities", "ids": [original]},
                   {"op": "rename_entity", "entity": copy_of("0", &other), "name": "X"}]),
            format!(
                "\"$0@{other}\": o objeto Outro ({other}) não está entre o que o comando 0 \
                 (duplicate_entities, o 1º do lote) copiou"
            ),
        ),
        (
            json!([{"op": "duplicate_entities", "ids": [original]},
                   {"op": "rename_entity", "entity": "$0@fantasma", "name": "X"}]),
            "fantasma (nenhum objeto, nó ou animação do projeto tem esse ID)".to_owned(),
        ),
        (
            json!([{"op": "duplicate_entities", "ids": [original]},
                   {"op": "rename_entity", "entity": copy_of("0.1", &original), "name": "X"}]),
            "o comando 0 (duplicate_entities, o 1º do lote) criou só 1 ID, \"$0.0\"".to_owned(),
        ),
        (
            json!([{"op": "duplicate_entities", "ids": [original, other], "count": 2},
                   {"op": "rename_entity", "entity": copy_of("0.4", &original), "name": "X"}]),
            "criou 4 IDs, de \"$0.0\" a \"$0.3\"".to_owned(),
        ),
        (
            json!([{"op": "create_entity", "kind": {"type": "group"}},
                   {"op": "rename_entity", "entity": copy_of("0", &original), "name": "X"}]),
            "não copia objetos".to_owned(),
        ),
        (
            json!([{"op": "duplicate_entities", "ids": [original]},
                   {"op": "rename_entity", "entity": "$0@$1", "name": "X"}]),
            "\"$1\" não se refere a um ID criado antes no lote".to_owned(),
        ),
    ] {
        let error = refused(&mut project, &scene, commands);
        assert!(error.contains(&expected), "{error}");
    }
}
