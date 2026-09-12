//! Optional authored assemblies. Runtime sees ordinary components, clips and graphs.
use crate::{
    animation::{Clip, Interpolation, Keyframe, Track},
    character::*,
    document::*,
    graph::{Edge, Node},
    physics3d::*,
    surface::*,
};
use glam::Vec3;
#[derive(Clone, Copy, Debug)]
pub enum MovementPreset {
    FirstPerson,
    ThirdPerson,
    Platform,
}
pub fn create(
    project: &mut Project,
    scene_id: &str,
    preset: MovementPreset,
    position: Vec3,
) -> Result<Id, String> {
    let scene = project.scene(scene_id).ok_or("Cena ausente")?;
    if scene.kind != SceneKind::ThreeD || !position.is_finite() {
        return Err("Esta montagem exige uma cena 3D e posição válida.".into());
    }
    let has_camera = scene
        .entities
        .iter()
        .any(|e| e.camera.as_ref().is_some_and(|c| c.active));
    let mut root = Entity::new(
        if matches!(preset, MovementPreset::Platform) {
            "Plataforma móvel"
        } else {
            "Jogador"
        },
        None,
    );
    root.transform.position = position.to_array();
    let id = root.id.clone();
    if matches!(preset, MovementPreset::Platform) {
        root.primitive = Some(Primitive::Cube);
        root.dimensions = [3., 0.4, 3.];
        root.material.color = [0.23, 0.47, 0.5, 1.];
        root.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: root.dimensions,
            },
            ..Default::default()
        });
        root.platform = Some(TranslationPlatform {
            mode: PlatformMode::Animation,
            ..Default::default()
        });
        let mut clip = Clip::new("Translação de ida e volta");
        clip.duration = 4.;
        clip.looping = true;
        clip.tracks.push(Track {
            target: id.clone(),
            keyframes: [(0., 0.), (2., 4.), (4., 0.)]
                .into_iter()
                .map(|(time, x)| Keyframe {
                    time,
                    transform: Transform {
                        position: (position + Vec3::X * x).to_array(),
                        ..Default::default()
                    },
                    interpolation: Interpolation::Linear,
                })
                .collect(),
        });
        let start = Node::new("event.scene_start", [30., 40.]);
        let mut play = Node::new("action.animation", [340., 40.]);
        play.params
            .insert("clip".into(), Value::Text(clip.id.clone()));
        root.graph.edges.push(Edge {
            from_node: start.id.clone(),
            from_port: "exec".into(),
            to_node: play.id.clone(),
            to_port: "exec".into(),
        });
        root.graph.nodes = vec![start, play];
        root.clips.push(clip);
        project.scene_mut(scene_id).unwrap().entities.push(root);
        return Ok(id);
    }
    let config = CharacterConfig::default();
    crate::input_actions::ensure_character(project, &config);
    root.character3d = Some(config);
    root.physics3d = Some(Collider3d {
        shape: CollisionShape::Capsule {
            height: 1.8,
            radius: 0.3,
        },
        center: [0., 0.9, 0.],
        ..Default::default()
    });
    let mut visual = Entity::new("Aparência editável", None);
    visual.parent = Some(id.clone());
    let visual_id = visual.id.clone();
    let mut camera = Entity::new("Câmera principal", None);
    camera.transform.position = (position + Vec3::Y * 1.6).to_array();
    camera.camera = Some(Camera {
        active: !has_camera,
        ..Default::default()
    });
    let rig = CameraRig {
        target: Some(id.clone()),
        mode: if matches!(preset, MovementPreset::FirstPerson) {
            CameraMode::FirstPerson
        } else {
            CameraMode::ThirdPerson
        },
        hidden: vec![visual_id.clone()],
        ..Default::default()
    };
    crate::input_actions::ensure_camera(project, &rig);
    camera.camera_rig = Some(rig);
    let mut pieces = vec![root, visual, camera];
    for (name, position, dimensions, color, primitive) in [
        (
            "Tronco",
            [0., 1.03, 0.],
            [0.44, 0.62, 0.28],
            [0.13, 0.49, 0.57, 1.],
            Primitive::Cube,
        ),
        (
            "Cabeça",
            [0., 1.56, 0.],
            [0.36, 0.36, 0.34],
            [0.82, 0.6, 0.36, 1.],
            Primitive::Sphere,
        ),
        (
            "Braço esquerdo",
            [-0.28, 1., 0.],
            [0.13, 0.62, 0.17],
            [0.18, 0.59, 0.64, 1.],
            Primitive::Cube,
        ),
        (
            "Braço direito",
            [0.28, 1., 0.],
            [0.13, 0.62, 0.17],
            [0.18, 0.59, 0.64, 1.],
            Primitive::Cube,
        ),
        (
            "Perna esquerda",
            [-0.12, 0.35, 0.],
            [0.18, 0.7, 0.2],
            [0.2, 0.24, 0.32, 1.],
            Primitive::Cube,
        ),
        (
            "Perna direita",
            [0.12, 0.35, 0.],
            [0.18, 0.7, 0.2],
            [0.2, 0.24, 0.32, 1.],
            Primitive::Cube,
        ),
    ] {
        let mut piece = Entity::new(name, Some(primitive));
        piece.parent = Some(visual_id.clone());
        piece.transform.position = position;
        piece.dimensions = dimensions;
        piece.material.color = color;
        pieces.push(piece);
    }
    project.scene_mut(scene_id).unwrap().entities.extend(pieces);
    Ok(id)
}
