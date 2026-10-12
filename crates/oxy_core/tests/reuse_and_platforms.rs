//! Reuse across levels (models, copies between scenes, repeated duplicates), moving platforms
//! and animations authored by command, and presets and movement profiles that describe
//! themselves.
use oxy_core::{
    animation::{Interpolation, sample_clip},
    character::{CharacterConfig, MovementProfile, PROFILE_FIELDS},
    command::{self, Applied, Command},
    document::*,
    editing,
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
    apply(
        &mut project,
        &first,
        json!({"op": "save_model", "entity": tower, "name": "Torre"}),
    );
    let error = refused(
        &mut project,
        &first,
        json!({"op": "place_model", "model": "Torre"}),
    );
    assert!(error.contains("2 modelos se chamam \"Torre\""), "{error}");
    let error = refused(
        &mut project,
        &first,
        json!({"op": "place_model", "model": "Castelo"}),
    );
    assert!(error.contains("Modelo Castelo não existe"), "{error}");
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
            ("parkour", "Parkour"),
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
    // The profile names commands, nodes and the catalog use are the same.
    for profile in MovementProfile::ALL {
        assert_eq!(MovementProfile::from_id(profile.id()), Some(profile));
        assert_eq!(serde_json::to_value(profile).unwrap(), profile.id());
    }

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
    assert!(preset("third_person")["description"].is_string());
}
