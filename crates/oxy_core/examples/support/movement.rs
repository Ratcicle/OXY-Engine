//! Deterministic, asset-free validation data. Only examples/tests compile this module.
use oxy_core::{
    character::*,
    document::*,
    physics3d::*,
    surface::{PlatformMode, TranslationPlatform},
};
use std::sync::Arc;
pub fn id(n: usize) -> String {
    format!("03000000-0000-4000-8000-{n:012x}")
}
pub fn fixture(count: usize, mode: &str) -> Project {
    assert!((1..=50).contains(&count));
    let mut p = Project::new("Medição 3D");
    p.id = id(9000);
    let s = &mut p.scenes[0];
    s.id = id(9001);
    p.start_scene = s.id.clone();
    s.kind = SceneKind::ThreeD;
    s.entities.clear();
    let mut floor = solid(1000, [0., -0.5, 0.], [160., 1., 160.]);
    if mode == "triangles" {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for z in 0..=40 {
            for x in 0..=40 {
                vertices.push([x as f32 * 4. - 80., 0.5, z as f32 * 4. - 80.]);
            }
        }
        for z in 0..40 {
            for x in 0..40 {
                let a = z * 41 + x;
                triangles.extend([[a, a + 41, a + 1], [a + 1, a + 41, a + 42]]);
            }
        }
        floor.physics3d.as_mut().unwrap().shape = CollisionShape::TriMesh {
            geometry: Arc::new(CollisionGeometry {
                vertices,
                triangles,
                source_fingerprint: None,
            }),
        };
    }
    s.entities.push(floor);
    for i in 0..count {
        let x = (i as f32 - count as f32 * 0.5) * 1.6;
        let mut body = Entity::new("Cápsula", Some(Primitive::Cube));
        body.id = id(i);
        body.transform.position = [x, if mode == "platforms" { 0.52 } else { 0.02 }, 0.];
        body.dimensions = [0.4, 1.5, 0.4];
        body.character3d = Some(CharacterConfig {
            reference: MovementReference::World,
            ..Default::default()
        });
        s.entities.push(body);
        if mode == "platforms" {
            let mut platform = solid(2000 + i, [x, 0.25, 0.], [1.4, 0.5, 8.]);
            platform.platform = Some(TranslationPlatform {
                mode: PlatformMode::Velocity,
                velocity: [0.1, 0., 0.],
                ..Default::default()
            });
            s.entities.push(platform);
        }
    }
    let mut camera = Entity::new("Câmera", None);
    camera.id = id(3000);
    camera.camera = Some(Camera::default());
    camera.camera_rig = Some(CameraRig {
        target: Some(id(0)),
        mode: CameraMode::ThirdPerson,
        ..Default::default()
    });
    s.entities.push(camera);
    for i in 0..200 {
        let dense = mode == "dense" || mode == "corridor";
        let x = if dense {
            (i % 50) as f32 * 1.6 - count as f32 * 0.8
        } else {
            (i % 20) as f32 * 4. - 40.
        };
        let z = if dense {
            (i / 50) as f32 * 2. - 3.
        } else {
            20. + (i / 20) as f32 * 4.
        };
        let x = x + if mode == "corridor" { 0.8 } else { 0. };
        let mut e = solid(
            4000 + i,
            [x, 1., z],
            if mode == "corridor" {
                [0.2, 2., 1.8]
            } else {
                [0.8, 2., 0.2]
            },
        );
        if i % 10 == 0 {
            e.physics3d.as_mut().unwrap().sensor = true;
        }
        s.entities.push(e);
    }
    while s.entities.len() < 400 {
        let n = s.entities.len();
        let mut e = Entity::new("Decoração", Some(Primitive::Cube));
        e.id = id(5000 + n);
        e.transform.position = [(n % 30) as f32 * 3. - 45., 0., 50. + (n / 30) as f32];
        s.entities.push(e);
    }
    validate_project(&p).unwrap();
    p
}
fn solid(n: usize, position: [f32; 3], size: [f32; 3]) -> Entity {
    let mut e = Entity::new("Obstáculo", Some(Primitive::Cube));
    e.id = id(n);
    e.transform.position = position;
    e.dimensions = size;
    e.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size },
        ..Default::default()
    });
    e
}
