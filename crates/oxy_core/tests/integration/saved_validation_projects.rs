//! Regression tests open the shipped JSON and pixels through the same loader as the player.
use oxy_core::{
    document::{Project, SceneKind, UiKind, Value},
    persistence::load_project,
    runtime::{FIXED_DT, InputFrame, Runtime},
};
use std::{collections::BTreeSet, path::Path};

fn project() -> Project {
    load_project(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/validacao/project.oxy.json"),
    )
    .expect("The editable validation project and its local assets must ship with the source")
}

#[test]
fn saved_card_graph_checks_cost_and_preserves_edit_document() {
    let project = project();
    let scene = project
        .scenes
        .iter()
        .find(|scene| {
            scene.entities.iter().any(|entity| {
                entity
                    .ui
                    .as_ref()
                    .is_some_and(|ui| ui.kind == UiKind::Button)
            })
        })
        .unwrap();
    let card = scene
        .entities
        .iter()
        .find(|entity| {
            entity
                .ui
                .as_ref()
                .is_some_and(|ui| ui.kind == UiKind::Button)
        })
        .unwrap();
    let state = scene
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Energia"))
        .unwrap();
    let target = scene
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Vida"))
        .unwrap();
    let mut runtime = Runtime::new(&project, &scene.id).unwrap();
    for _ in 0..4 {
        runtime.click(&card.id);
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        runtime.scene().entity(&state.id).unwrap().attributes["Energia"],
        Value::Number(0.0)
    );
    assert_eq!(
        runtime.scene().entity(&target.id).unwrap().attributes["Vida"],
        Value::Number(25.0)
    );
    assert_eq!(state.attributes["Energia"], Value::Number(3.0));
    assert!(
        runtime
            .logs
            .iter()
            .any(|line| line.contains("Energia insuficiente"))
    );
}

#[test]
fn saved_2d_project_collects_opens_door_and_hits_through_marker() {
    let project = project();
    let scene = project
        .scenes
        .iter()
        .find(|scene| {
            scene.kind == SceneKind::TwoD
                && scene
                    .entities
                    .iter()
                    .any(|entity| entity.controller.is_some())
        })
        .unwrap();
    let actor = scene
        .entities
        .iter()
        .find(|entity| entity.controller.is_some())
        .unwrap();
    let target = scene
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Vida") && entity.controller.is_none())
        .unwrap();
    let door =
        scene
            .entities
            .iter()
            .find(|entity| {
                entity.graph.nodes.iter().any(|node| {
                    node.operation == "event.input" && node.text("action") == "interagir"
                })
            })
            .unwrap();
    let mut runtime = Runtime::new(&project, &scene.id).unwrap();
    for _ in 0..83 {
        runtime.advance(
            FIXED_DT,
            &InputFrame {
                released: Default::default(),
                held: BTreeSet::from(["mover_direita".into()]),
                pressed: BTreeSet::new(),
                ..Default::default()
            },
        );
    }
    assert_eq!(
        runtime.scene().entity(&actor.id).unwrap().attributes["Chave"],
        Value::Bool(true)
    );
    runtime.advance(
        FIXED_DT,
        &InputFrame {
            released: Default::default(),
            held: BTreeSet::new(),
            pressed: BTreeSet::from(["atacar".into(), "interagir".into()]),
            ..Default::default()
        },
    );
    for _ in 0..60 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        runtime.scene().entity(&target.id).unwrap().attributes["Vida"],
        Value::Number(75.0)
    );
    assert!(
        !runtime
            .scene()
            .entity(&door.id)
            .unwrap()
            .collider
            .as_ref()
            .unwrap()
            .enabled
    );
    assert!(
        runtime
            .traces
            .iter()
            .any(|trace| trace.operation == "event.animation")
    );
    assert!(
        !runtime
            .logs
            .iter()
            .any(|line| line.contains("interrompido"))
    );
}

#[test]
fn saved_textured_3d_hierarchy_uses_same_runtime_and_hit_rules() {
    let project = project();
    let scene = project
        .scenes
        .iter()
        .find(|scene| scene.kind == SceneKind::ThreeD)
        .unwrap();
    let target = scene
        .entities
        .iter()
        .find(|entity| entity.attributes.contains_key("Vida") && entity.controller.is_none())
        .unwrap();
    let root = scene
        .entities
        .iter()
        .find(|entity| entity.controller.is_some())
        .unwrap();
    assert!(scene.descendants(&root.id).len() >= 8);
    assert!(
        scene
            .entities
            .iter()
            .any(|entity| entity.material.texture.is_some())
    );
    let mut runtime = Runtime::new(&project, &scene.id).unwrap();
    runtime.advance(
        FIXED_DT,
        &InputFrame {
            released: Default::default(),
            held: BTreeSet::new(),
            pressed: BTreeSet::from(["atacar".into()]),
            ..Default::default()
        },
    );
    for _ in 0..60 {
        runtime.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(
        runtime.scene().entity(&target.id).unwrap().attributes["Vida"],
        Value::Number(125.0)
    );
    assert!(!runtime.sounds.is_empty());
    assert!(
        !runtime
            .logs
            .iter()
            .any(|line| line.contains("interrompido"))
    );
}
