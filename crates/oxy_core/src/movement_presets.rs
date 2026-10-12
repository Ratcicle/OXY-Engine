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
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug)]
pub enum MovementPreset {
    FirstPerson,
    ThirdPerson,
    Platform,
}
/// The camera a character assembly gets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetCamera {
    /// A new "Câmera principal" that follows the player, active when the scene has no active
    /// camera.
    #[default]
    Create,
    /// The scene's active camera (or its first one) follows the player instead.
    Reuse,
    /// No camera follows the player.
    None,
}
/// Choices of an assembly beyond its kind and position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresetOptions {
    /// Platform: how far it goes from where it is placed, in metres, before coming back.
    pub travel: Vec3,
    /// Platform: seconds for the way there and back.
    pub period: f32,
    /// Characters.
    pub camera: PresetCamera,
}
impl Default for PresetOptions {
    fn default() -> Self {
        Self {
            travel: Vec3::X * 4.,
            period: 4.,
            camera: PresetCamera::Create,
        }
    }
}
pub fn create(
    project: &mut Project,
    scene_id: &str,
    preset: MovementPreset,
    position: Vec3,
) -> Result<Id, String> {
    create_with(
        project,
        scene_id,
        preset,
        position,
        PresetOptions::default(),
    )
}
pub fn create_with(
    project: &mut Project,
    scene_id: &str,
    preset: MovementPreset,
    position: Vec3,
    options: PresetOptions,
) -> Result<Id, String> {
    let scene = project.scene(scene_id).ok_or("Cena ausente")?;
    if scene.kind != SceneKind::ThreeD || !position.is_finite() {
        return Err("Esta montagem exige uma cena 3D e posição válida.".into());
    }
    if !options.travel.is_finite() {
        return Err("O percurso da plataforma precisa de números finitos.".into());
    }
    if !options.period.is_finite() || options.period <= 0. {
        return Err(
            "O período da plataforma precisa ser um número de segundos maior que 0.".into(),
        );
    }
    // Checked before anything changes: the camera to follow the player, when reused.
    let reused = match options.camera {
        PresetCamera::Reuse if !matches!(preset, MovementPreset::Platform) => {
            let camera = scene
                .entities
                .iter()
                .find(|e| e.camera.as_ref().is_some_and(|c| c.active))
                .or_else(|| scene.entities.iter().find(|e| e.camera.is_some()))
                .ok_or("A cena não tem câmera para reaproveitar; use camera: create")?;
            if let Some(target) = camera.camera_rig.as_ref().and_then(|r| r.target.as_ref()) {
                let name = scene.entity(target).map_or(target.as_str(), |e| &e.name);
                return Err(format!(
                    "A câmera {} já segue {name}; use camera: create ou none",
                    camera.name
                ));
            }
            Some(camera.id.clone())
        }
        _ => None,
    };
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
        let period = options.period;
        clip.duration = period;
        clip.looping = true;
        clip.tracks.push(Track {
            target: id.clone(),
            keyframes: [(0., 0.), (period * 0.5, 1.), (period, 0.)]
                .into_iter()
                .map(|(time, along)| Keyframe {
                    time,
                    transform: Transform {
                        position: (position + options.travel * along).to_array(),
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

    let mut visual = Entity::new("Aparência editável", None);
    visual.parent = Some(id.clone());
    let visual_id = visual.id.clone();
    let mode = if matches!(preset, MovementPreset::FirstPerson) {
        CameraMode::FirstPerson
    } else {
        CameraMode::ThirdPerson
    };
    let mut pieces = vec![root, visual];
    if let Some(camera) = reused {
        let camera = project
            .scene_mut(scene_id)
            .and_then(|scene| scene.entity_mut(&camera))
            .ok_or("Câmera ausente")?;
        let mut rig = camera.camera_rig.clone().unwrap_or_default();
        rig.target = Some(id.clone());
        rig.mode = mode;
        rig.hidden = vec![visual_id.clone()];
        camera.camera_rig = Some(rig.clone());
        // A camera taken because the scene had no active one becomes the active camera.
        if let Some(settings) = &mut camera.camera {
            settings.active = true;
        }
        crate::input_actions::ensure_camera(project, &rig);
    } else if options.camera == PresetCamera::Create {
        let mut camera = Entity::new("Câmera principal", None);
        camera.transform.position = (position + Vec3::Y * 1.6).to_array();
        camera.camera = Some(Camera {
            active: !has_camera,
            ..Default::default()
        });
        let rig = CameraRig {
            target: Some(id.clone()),
            mode,
            hidden: vec![visual_id.clone()],
            ..Default::default()
        };
        crate::input_actions::ensure_camera(project, &rig);
        camera.camera_rig = Some(rig);
        pieces.push(camera);
    }
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
