use glam::Vec3;
use oxy_core::{
    document::Primitive,
    geometry::{
        cuts::{Point, Segment},
        primitives::Parameters,
        *,
    },
};
fn primitive(kind: Primitive) -> EditableMesh {
    primitives::generate(kind, 8, Parameters::default()).unwrap()
}
fn edge_at(mesh: &EditableMesh, from: [f32; 3], to: [f32; 3]) -> u32 {
    mesh.data()
        .edges
        .iter()
        .find(|e| {
            let a = mesh.position(e.vertices[0]).unwrap();
            let b = mesh.position(e.vertices[1]).unwrap();
            (a.abs_diff_eq(Vec3::from(from), 1e-5) && b.abs_diff_eq(Vec3::from(to), 1e-5))
                || (b.abs_diff_eq(Vec3::from(from), 1e-5) && a.abs_diff_eq(Vec3::from(to), 1e-5))
        })
        .unwrap()
        .id
}
fn closed(mesh: &EditableMesh) {
    for uses in &mesh.prepared().incident_faces {
        assert_eq!(uses.len(), 2);
        let (a, ac) = uses[0];
        let (b, bc) = uses[1];
        assert_ne!(
            mesh.data().faces[a].corners[ac].vertex,
            mesh.data().faces[b].corners[bc].vertex
        );
    }
}
fn area(mesh: &EditableMesh) -> f32 {
    mesh.prepared()
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = mesh.triangle_points(t);
            (b - a).cross(c - a).length() * 0.5
        })
        .sum()
}
#[test]
fn cube_loop_is_a_local_quad_strip_with_consistent_shared_vertices() {
    let mesh = primitive(Primitive::Cube);
    let edge = edge_at(&mesh, [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5]);
    for count in [1, 2, 4, 8] {
        for slide in [-0.8, 0., 0.7] {
            let cut = cuts::loop_cut(&mesh, edge, count, slide).unwrap();
            let m = &cut.output.mesh;
            assert_eq!(m.data().vertices.len(), 8 + 4 * count as usize);
            assert_eq!(m.data().faces.len(), 6 + 4 * count as usize);
            assert_eq!(m.data().edges.len(), 12 + 8 * count as usize);
            closed(m);
            assert!((area(m) - 6.).abs() < 1e-5);
            assert!(cut.notes.is_empty());
            assert_eq!(cut.output.selection.ids.len(), 4 * count as usize);
            // Top and bottom stay authored quads, proving this is not whole-mesh subdivision.
            for (i, n) in mesh.prepared().face_normals.iter().enumerate() {
                if n.y.abs() > 0.9 {
                    assert_eq!(
                        mesh.data().faces[i],
                        *m.face(mesh.data().faces[i].id).unwrap()
                    );
                }
            }
        }
    }
}
#[test]
fn cylinder_and_tube_lateral_loops_preserve_holes_and_stop_at_caps() {
    for kind in [Primitive::Cylinder, Primitive::Tube] {
        let mesh = primitive(kind);
        let vertical = mesh
            .data()
            .edges
            .iter()
            .find(|e| {
                let a = mesh.position(e.vertices[0]).unwrap();
                let b = mesh.position(e.vertices[1]).unwrap();
                (a.y - b.y).abs() > 0.9
            })
            .unwrap()
            .id;
        let cut = cuts::loop_cut(&mesh, vertical, 2, 0.3).unwrap();
        closed(&cut.output.mesh);
        assert!((area(&mesh) - area(&cut.output.mesh)).abs() < 2e-5);
        let horizontal = mesh
            .data()
            .edges
            .iter()
            .find(|e| {
                let a = mesh.position(e.vertices[0]).unwrap();
                let b = mesh.position(e.vertices[1]).unwrap();
                (a.y - b.y).abs() < 1e-5
            })
            .unwrap()
            .id;
        let across = cuts::loop_cut(&mesh, horizontal, 1, 0.).unwrap();
        closed(&across.output.mesh);
        assert!((area(&mesh) - area(&across.output.mesh)).abs() < 2e-5);
        if kind == Primitive::Cylinder {
            assert!(!across.notes.is_empty());
        }
        if kind == Primitive::Tube {
            let m = &cut.output.mesh;
            assert!(
                m.prepared()
                    .acceleration
                    .hit(Vec3::new(0., 2., 0.), -Vec3::Y, |i| m
                        .triangle_points(&m.prepared().triangles[i]))
                    .is_none()
            );
        }
    }
}
#[test]
fn knife_can_cross_two_adjacent_painted_cube_faces_without_cutting_hidden_faces() {
    let mesh = primitive(Primitive::Cube);
    let left = edge_at(&mesh, [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5]);
    let middle = edge_at(&mesh, [0.5, -0.5, 0.5], [0.5, 0.5, 0.5]);
    let right = edge_at(&mesh, [0.5, -0.5, -0.5], [0.5, 0.5, -0.5]);
    let point = |edge| Point { edge, factor: 0.5 };
    let face1 = mesh.data().faces[0].id;
    let face2 = mesh.data().faces[1].id;
    let one = cuts::knife(
        &mesh,
        &[Segment {
            face: face1,
            from: point(left),
            to: point(middle),
        }],
    )
    .unwrap();
    closed(&one.output.mesh);
    assert_eq!(one.output.mesh.data().faces.len(), 7);
    let two = cuts::knife(
        &mesh,
        &[
            Segment {
                face: face1,
                from: point(left),
                to: point(middle),
            },
            Segment {
                face: face2,
                from: point(middle),
                to: point(right),
            },
        ],
    )
    .unwrap();
    let m = &two.output.mesh;
    closed(m);
    assert_eq!(m.data().vertices.len(), 11);
    assert_eq!(m.data().faces.len(), 8);
    assert!((area(m) - 6.).abs() < 1e-5);
    let center = m
        .data()
        .vertices
        .iter()
        .find(|v| Vec3::from(v.position).abs_diff_eq(Vec3::new(0.5, 0., 0.5), 1e-6))
        .unwrap()
        .id;
    let corners = m
        .data()
        .faces
        .iter()
        .flat_map(|f| &f.corners)
        .filter(|c| c.vertex == center)
        .collect::<Vec<_>>();
    assert_eq!(corners.len(), 4);
    // This corner has a UV seam; each face independently interpolates its original edge mapping.
    assert!(corners.iter().all(|c| (c.uv[1] - 0.25).abs() < 1e-6));
    assert!(
        cuts::knife(
            &mesh,
            &[
                Segment {
                    face: face1,
                    from: point(left),
                    to: point(middle)
                },
                Segment {
                    face: face2,
                    from: point(right),
                    to: point(middle)
                }
            ]
        )
        .is_err()
    );
    assert!(
        cuts::knife(
            &mesh,
            &[Segment {
                face: face1,
                from: point(left),
                to: point(right)
            }]
        )
        .is_err()
    );
}
#[test]
fn loop_and_knife_keep_existing_corner_uvs_and_stable_ids_on_save_undo() {
    use oxy_core::{document::*, edit_history::CommandHistory, texture_cache::TextureCache};
    let mesh = primitive(Primitive::Cube);
    let edge = mesh.data().edges[0].id;
    let cut = cuts::loop_cut(&mesh, edge, 1, 0.).unwrap();
    for face in &mesh.data().faces {
        for corner in &face.corners {
            assert!(
                cut.output
                    .mesh
                    .data()
                    .faces
                    .iter()
                    .flat_map(|f| &f.corners)
                    .any(|c| c == corner)
            );
        }
    }
    let serialized = serde_json::to_vec(&cut.output.mesh).unwrap();
    assert_eq!(
        serde_json::from_slice::<EditableMesh>(&serialized).unwrap(),
        cut.output.mesh
    );
    let mut p = Project::new("Cuts");
    let mut e = Entity::new("Mesh", None);
    e.mesh = Some(mesh);
    p.scenes[0].entities.push(e);
    let old = p.clone();
    let mut images = TextureCache::default();
    let mut history = CommandHistory::new();
    history.begin("Cortar", &p, &images);
    p.scenes[0].entities[0].mesh = Some(cut.output.mesh);
    history.commit(&p, &mut images).unwrap();
    let after = p.clone();
    history.undo(&mut p, &mut images).unwrap();
    assert_eq!(p, old);
    history.redo(&mut p, &mut images).unwrap();
    assert_eq!(p, after);
}
