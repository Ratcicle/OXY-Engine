use glam::{Mat4, Vec2, Vec3};
use oxy_core::{
    document::*,
    geometry::{
        primitives::{self, Parameters},
        selection::{Mode, Selection},
        *,
    },
};

fn polygon(points: &[[f32; 3]]) -> MeshData {
    let mut d = MeshData::default();
    let mut corners = vec![];
    for &p in points {
        let vertex = d.add_vertex(Vec3::from(p)).unwrap();
        corners.push(Corner {
            vertex,
            uv: Vec2::from([p[0], p[1]]).to_array(),
            normal: None,
        });
    }
    d.add_face(corners).unwrap();
    d.complete_edges().unwrap();
    d
}
#[test]
fn primitives_are_manifold_and_keep_polygons_with_real_tube_hole() {
    for kind in [
        Primitive::Cube,
        Primitive::Sphere,
        Primitive::Cylinder,
        Primitive::Plane,
        Primitive::Pyramid,
        Primitive::Cone,
        Primitive::Tube,
        Primitive::Rectangle,
        Primitive::Circle,
    ] {
        for sides in [3, 4, 8, 24, 64] {
            let p = Parameters {
                latitude: 12,
                height_divisions: 3,
                plane_divisions: [4, 3],
                ..Default::default()
            };
            let mesh = primitives::generate(kind, sides, p)
                .unwrap_or_else(|e| panic!("{kind:?} {sides}: {e}"));
            assert!(!mesh.prepared().triangles.is_empty());
            let closed = matches!(
                kind,
                Primitive::Cube
                    | Primitive::Sphere
                    | Primitive::Cylinder
                    | Primitive::Pyramid
                    | Primitive::Cone
                    | Primitive::Tube
            );
            if closed {
                assert!(
                    mesh.prepared()
                        .incident_faces
                        .iter()
                        .all(|faces| faces.len() == 2),
                    "{kind:?}"
                );
            }
            for t in &mesh.prepared().triangles {
                let [a, b, c] = mesh.triangle_points(t);
                assert!((b - a).cross(c - a).length() > 1e-8);
            }
            if kind == Primitive::Tube {
                // An axial ray cannot hit any triangle: the hole has no hidden cap.
                for t in &mesh.prepared().triangles {
                    let p = mesh.triangle_points(t).map(|p| Vec2::new(p.x, p.z));
                    let cross = |a: Vec2, b: Vec2| a.perp_dot(b);
                    let signs = [cross(p[0], p[1]), cross(p[1], p[2]), cross(p[2], p[0])];
                    assert!(!(signs.iter().all(|v| *v > 1e-7) || signs.iter().all(|v| *v < -1e-7)));
                }
            }
        }
    }
}
#[test]
fn concave_polygon_ear_clipping_preserves_area_and_winding() {
    let d = polygon(&[
        [0., 0., 0.],
        [3., 0., 0.],
        [3., 1., 0.],
        [1., 1., 0.],
        [1., 3., 0.],
        [0., 3., 0.],
    ]);
    let mesh = EditableMesh::new(d).unwrap();
    assert_eq!(mesh.data().faces.len(), 1);
    assert_eq!(mesh.prepared().triangles.len(), 4);
    let area: f32 = mesh
        .prepared()
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = mesh.triangle_points(t);
            let n = (b - a).cross(c - a);
            assert!(n.z > 0.);
            n.length() * 0.5
        })
        .sum();
    assert!((area - 5.).abs() < 1e-5);
}
#[test]
fn invalid_candidates_are_rejected_without_changing_original() {
    let mesh = primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap();
    let original = mesh.clone();
    let mut d = mesh.data().clone();
    d.vertices[0].position[0] = f32::NAN;
    assert!(EditableMesh::new(d).is_err());
    let mut d = mesh.data().clone();
    d.vertices[0].position[1] += 0.1;
    assert!(EditableMesh::new(d).unwrap_err().contains("não plana"));
    let mut d = mesh.data().clone();
    d.edges[0].vertices[1] = u32::MAX;
    assert!(EditableMesh::new(d).is_err());
    let mut d = mesh.data().clone();
    d.vertices[0].id = d.vertices[1].id;
    assert!(EditableMesh::new(d).is_err());
    assert!(
        EditableMesh::new(polygon(&[
            [0., 0., 0.],
            [2., 2., 0.],
            [0., 2., 0.],
            [2., 0., 0.]
        ]))
        .is_err()
    );
    let mut d = mesh.data().clone();
    let corners = d.faces[0].corners.clone();
    d.add_face(corners).unwrap();
    assert!(EditableMesh::new(d).unwrap_err().contains("duplicada"));
    assert_eq!(mesh, original);
    assert!(mesh.shares_storage(&original));
}
#[test]
fn conversion_preserves_world_transform_children_colliders_and_corner_uvs() {
    let mut scene = Scene::new("Modelo", SceneKind::ThreeD);
    let mut e = Entity::new("Braço", Some(Primitive::Cube));
    e.dimensions = [2., 3., 0.4];
    e.transform.position = [3., 5., -2.];
    e.transform.pivot = [0.5, 0.2, 0.];
    e.transform.scale = [-2., 1., 3.];
    e.transform.rotation = [0.2, 0.7, -0.3];
    e.collider = Some(Collider::default());
    let mut child = Entity::new("Filho", None);
    child.parent = Some(e.id.clone());
    scene.entities = vec![e.clone(), child.clone()];
    let before = scene.world_matrix(&child.id).unwrap();
    let bounds = oxy_core::runtime::collider_box(&scene, &e.id).unwrap();
    let source = primitives::for_entity(&e).unwrap();
    primitives::convert(&mut scene.entities[0]).unwrap();
    let converted = &scene.entities[0];
    assert_eq!(converted.transform, e.transform);
    assert_eq!(converted.collider, e.collider);
    assert_eq!(scene.world_matrix(&child.id).unwrap(), before);
    assert_eq!(
        oxy_core::runtime::collider_box(&scene, &converted.id).unwrap(),
        bounds
    );
    let mesh = converted.mesh.as_ref().unwrap();
    let matrix = e.transform.matrix() * Mat4::from_scale(Vec3::from(e.dimensions));
    for (a, b) in source.data().vertices.iter().zip(&mesh.data().vertices) {
        assert!(
            matrix.transform_point3(Vec3::from(a.position)).distance(
                converted
                    .transform
                    .matrix()
                    .transform_point3(Vec3::from(b.position))
            ) < 1e-5
        );
    }
    for (a, b) in source.data().faces.iter().zip(&mesh.data().faces) {
        assert_eq!(
            a.corners.iter().map(|c| c.uv).collect::<Vec<_>>(),
            b.corners.iter().map(|c| c.uv).collect::<Vec<_>>()
        );
    }
}
#[test]
fn serialization_and_same_count_replacement_keep_identity_safe() {
    let mesh = primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap();
    let json = serde_json::to_string(&mesh).unwrap();
    assert!(!json.contains("revision"));
    assert!(!json.contains("triangles"));
    let loaded: EditableMesh = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded, mesh);
    assert_ne!(loaded.revision(), mesh.revision());
    let mut data = mesh.data().clone();
    for v in &mut data.vertices {
        v.position[0] += 1.;
    }
    let changed = EditableMesh::new(data).unwrap();
    assert_ne!(changed.revision(), mesh.revision());
    assert_ne!(changed, mesh);
    let mut s = Selection {
        mode: Mode::Face,
        ..Default::default()
    };
    s.click(Some(mesh.data().faces[0].id), false);
    s.click(Some(mesh.data().faces[1].id), true);
    assert_eq!(s.vertices(&mesh).len(), 6);
    s.invert(&mesh);
    assert_eq!(s.ids.len(), 4);
    s.ids.push(u32::MAX);
    s.sanitize(&mesh);
    assert_eq!(s.ids.len(), 4);
    let mut loose = MeshData::default();
    let a = loose.add_vertex(Vec3::ZERO).unwrap();
    let b = loose.add_vertex(Vec3::X).unwrap();
    let id = loose.allocate_id().unwrap();
    loose.edges.push(Edge {
        id,
        vertices: [a, b],
    });
    assert!(
        EditableMesh::new(loose)
            .unwrap()
            .prepared()
            .triangles
            .is_empty()
    );
}

#[test]
fn mesh_history_keeps_deltas_and_failed_deformation_is_atomic() {
    use oxy_core::{edit_history::CommandHistory, texture_cache::TextureCache};
    let mut project = oxy_core::editing::blank_project("Teste", SceneKind::ThreeD).unwrap();
    let mut e = Entity::new("Peça", Some(Primitive::Cube));
    primitives::convert(&mut e).unwrap();
    let id = e.id.clone();
    project.scenes[0].entities.push(e);
    let before = project.clone();
    let original = before.scenes[0].entities[0].mesh.as_ref().unwrap();
    let mut images = TextureCache::default();
    let mut history = CommandHistory::new();
    history.begin("Mover uma face", &project, &images);
    let selection = Selection {
        mode: Mode::Face,
        ids: vec![original.data().faces[0].id],
        ..Default::default()
    };
    let changed =
        edit::transform(original, &selection, Mat4::from_translation(Vec3::X * 0.25)).unwrap();
    project.scenes[0].entity_mut(&id).unwrap().mesh = Some(changed);
    history.commit(&project, &mut images).unwrap();
    assert_eq!(history.undo_len(), 1);
    let after = project.clone();
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);
    history.redo(&mut project, &mut images).unwrap();
    assert_eq!(project, after);
    let bad = Selection {
        mode: Mode::Vertex,
        ids: vec![original.data().vertices[0].id],
        ..Default::default()
    };
    assert!(edit::transform(original, &bad, Mat4::from_translation(Vec3::Y * 0.1)).is_err());
    assert!(
        !project.scenes[0].entities[0]
            .mesh
            .as_ref()
            .unwrap()
            .shares_storage(after.scenes[0].entities[0].mesh.as_ref().unwrap())
    ); // History deserializes a fresh validated revision.
}

#[test]
fn bvh_matches_independent_brute_rays_and_same_count_changes() {
    let mesh = primitives::generate(
        Primitive::Tube,
        24,
        Parameters {
            height_divisions: 8,
            ..Default::default()
        },
    )
    .unwrap();
    for i in 0..250 {
        let a = i as f32 * 0.618;
        let origin = Vec3::new(a.sin() * 2., (a * 1.31).sin() * 2., a.cos() * 2.);
        let direction = (-origin + Vec3::new((a * 2.).sin() * 0.6, 0., 0.)).normalize();
        let accelerated = mesh.prepared().acceleration.hit(origin, direction, |i| {
            mesh.triangle_points(&mesh.prepared().triangles[i])
        });
        let brute = mesh
            .prepared()
            .triangles
            .iter()
            .filter_map(|t| {
                // Independent plane intersection followed by oriented edge half-space tests.
                let [a, b, c] = mesh.triangle_points(t);
                let n = (b - a).cross(c - a);
                let denominator = n.dot(direction);
                if denominator.abs() < 1e-8 {
                    return None;
                }
                let distance = n.dot(a - origin) / denominator;
                if distance < 0. {
                    return None;
                }
                let p = origin + direction * distance;
                ([(a, b), (b, c), (c, a)]
                    .iter()
                    .all(|(a, b)| (*b - *a).cross(p - *a).dot(n) >= -1e-9))
                .then_some(distance)
            })
            .min_by(f32::total_cmp);
        assert_eq!(accelerated.is_some(), brute.is_some(), "ray {i}");
        if let (Some((_, a, _)), Some(b)) = (accelerated, brute) {
            assert!((a - b).abs() < 1e-4);
        }
    }
}
