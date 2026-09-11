use glam::Vec3;
use oxy_core::{
    character::*,
    document::*,
    guide_recipes,
    runtime::{FIXED_DT, InputFrame, Runtime},
};
fn body(p: &Project) -> String {
    p.scenes[0]
        .entities
        .iter()
        .find(|e| e.character3d.is_some())
        .unwrap()
        .id
        .clone()
}
fn open(index: usize) -> (Runtime, String) {
    let p = guide_recipes::load(index).unwrap();
    let id = body(&p);
    (Runtime::new(&p, &p.start_scene).unwrap(), id)
}
fn ticks(r: &mut Runtime, n: usize, input: &InputFrame) {
    for _ in 0..n {
        r.advance(FIXED_DT, input);
    }
}
fn press(r: &mut Runtime, action: &str) {
    r.advance(
        FIXED_DT,
        &InputFrame {
            pressed: [action.into()].into(),
            ..Default::default()
        },
    );
}
fn clean(r: &Runtime) {
    assert!(r.logs.is_empty(), "{:?}", r.logs);
}

#[test]
fn fps_recipe_runs_the_editable_intent_graph_and_jumps() {
    let (mut r, id) = open(5);
    let start = r.character_state(&id).unwrap().position;
    ticks(
        &mut r,
        45,
        &InputFrame {
            held: ["mover_frente".into()].into(),
            ..Default::default()
        },
    );
    assert!(r.character_state(&id).unwrap().position.z < start.z - 2.);
    press(&mut r, "pular");
    assert!(r.character_state(&id).unwrap().velocity.y > 0.);
    clean(&r);
}
#[test]
fn camera_surface_impulse_and_block_recipes_execute_authored_nodes() {
    let (mut r, id) = open(6);
    let camera = r.active_camera().unwrap().to_string();
    press(&mut r, "2");
    assert_eq!(r.camera_mode(&camera), Some(CameraMode::ThirdPerson));
    press(&mut r, "1");
    assert_eq!(r.camera_mode(&camera), Some(CameraMode::FirstPerson));
    assert!(r.character_state(&id).unwrap().position.is_finite());
    clean(&r);
    let (mut r, _) = open(7);
    press(&mut r, "2");
    let surface = r
        .scene()
        .entities
        .iter()
        .find(|e| e.name == "Piso inicial")
        .unwrap()
        .physics3d
        .as_ref()
        .unwrap()
        .surface
        .as_ref()
        .unwrap();
    assert!(
        r.project()
            .surfaces
            .iter()
            .find(|s| &s.id == surface)
            .unwrap()
            .friction
            < 0.03
    );
    press(&mut r, "1");
    clean(&r);
    let (mut r, id) = open(8);
    press(&mut r, "I");
    assert!(r.character_state(&id).unwrap().velocity.y > 6.);
    clean(&r);
    let (mut r, id) = open(10);
    press(&mut r, "B");
    ticks(
        &mut r,
        90,
        &InputFrame {
            held: ["mover_frente".into()].into(),
            ..Default::default()
        },
    );
    let s = r.character_state(&id).unwrap();
    assert!(s.grounded && s.position.y < 0.1);
    assert!((s.position.z - 4.).abs() < 1e-4);
    press(&mut r, "N");
    ticks(
        &mut r,
        30,
        &InputFrame {
            held: ["mover_frente".into()].into(),
            ..Default::default()
        },
    );
    assert!(r.character_state(&id).unwrap().position.z < 3.);
    clean(&r);
}
#[test]
fn checkpoint_recipe_saves_dynamic_transform_and_resets_via_attributes() {
    let (mut r, id) = open(9);
    ticks(
        &mut r,
        95,
        &InputFrame {
            held: ["mover_frente".into()].into(),
            ..Default::default()
        },
    );
    let e = r.scene().entity(&id).unwrap();
    assert_eq!(e.attributes["Checkpoint"], Value::Text("Marco azul".into()));
    let checkpoint = e.attributes["Destino"].vector3().unwrap();
    assert!(checkpoint.z < -3.);
    r.add_character_velocity(&id, Vec3::Y * 6.).unwrap();
    ticks(&mut r, 8, &InputFrame::default());
    press(&mut r, "reiniciar");
    assert!(
        r.character_state(&id)
            .unwrap()
            .position
            .distance(checkpoint)
            < 0.025
    );
    clean(&r);
}
#[test]
fn automatic_recipe_repeats_only_when_enabled() {
    let (mut r, id) = open(11);
    ticks(&mut r, 3, &InputFrame::default());
    press(&mut r, "2");
    let held = InputFrame {
        held: ["pular".into()].into(),
        ..Default::default()
    };
    let mut jumps = 0;
    for _ in 0..150 {
        ticks(&mut r, 1, &held);
        jumps += r
            .movement_records()
            .iter()
            .filter(|e| matches!(e.event, MovementEvent::Jumped))
            .count();
    }
    assert!(jumps >= 3);
    press(&mut r, "1");
    ticks(&mut r, 100, &InputFrame::default());
    for _ in 0..80 {
        ticks(&mut r, 1, &held);
        assert!(
            !r.movement_records()
                .iter()
                .any(|e| matches!(e.event, MovementEvent::Jumped))
        );
    }
    assert!(r.character_state(&id).unwrap().grounded);
    clean(&r);
}
#[test]
fn laboratory_climbs_authored_ramp_and_updates_checkpoint_hud_without_special_code() {
    let p = guide_recipes::movement_laboratory().unwrap();
    let id = body(&p);
    let before = p.clone();
    let mut r = Runtime::new(&p, &p.start_scene).unwrap();
    let forward = InputFrame {
        held: ["mover_frente".into()].into(),
        ..Default::default()
    };
    for _ in 0..360 {
        ticks(&mut r, 1, &forward);
        if r.character_state(&id).unwrap().position.z < -22. {
            break;
        }
    }
    let s = r.character_state(&id).unwrap();
    assert!(s.position.y > 3.2 && s.position.z < -21.5, "{s:?}");
    let e = r.scene().entity(&id).unwrap();
    assert_eq!(e.attributes["Checkpoint"], Value::Text("Terraço".into()));
    assert!(e.attributes["Velocidade"].number().unwrap() > 5.);
    assert!(e.attributes["Tempo"].number().unwrap() > 4.);
    assert_eq!(p, before);
    clean(&r);
    let saved = r.scene().entity(&id).unwrap().attributes["Destino"]
        .vector3()
        .unwrap();
    // Thin reset sensor sees a large downward swept movement, even with no
    // final overlap. Its ordinary graph discards velocity and old path.
    r.teleport_character(&id, Vec3::new(35., 2., 0.), TeleportOptions::default())
        .unwrap();
    r.set_character_velocity(&id, -Vec3::Y * 120.).unwrap();
    ticks(&mut r, 6, &InputFrame::default());
    assert!(r.character_state(&id).unwrap().position.distance(saved) < 0.1);
    clean(&r);
    assert_eq!(
        p,
        oxy_core::migration::read(&serde_json::to_vec(&p).unwrap()).unwrap()
    );
}
#[test]
fn friendly_collision_groups_roundtrip_without_changing_filter_bits() {
    let mut p = guide_recipes::movement_laboratory().unwrap();
    let filters: Vec<_> = p.scenes[0]
        .entities
        .iter()
        .filter_map(|e| e.physics3d.as_ref().map(|c| c.filter.clone()))
        .collect();
    p.collision_groups.insert(0, "Mundo e plataformas".into());
    validate_project(&p).unwrap();
    let reopened: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(reopened.collision_group_label(0), "Mundo e plataformas");
    assert_eq!(
        filters,
        reopened.scenes[0]
            .entities
            .iter()
            .filter_map(|e| e.physics3d.as_ref().map(|c| c.filter.clone()))
            .collect::<Vec<_>>()
    );
    p.collision_groups.insert(32, "Fora do intervalo".into());
    assert!(validate_project(&p).is_err());
}
