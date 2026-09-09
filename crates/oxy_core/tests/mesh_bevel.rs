use glam::Vec3;
use oxy_core::{
    document::Primitive,
    geometry::{
        primitives::Parameters,
        selection::{Mode, Selection},
        *,
    },
};
fn cube() -> EditableMesh {
    primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap()
}
fn selection(mode: Mode, ids: Vec<u32>) -> Selection {
    Selection {
        mode,
        ids,
        through: false,
    }
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
    assert_eq!(
        mesh.data().vertices.len() as isize - mesh.data().edges.len() as isize
            + mesh.data().faces.len() as isize,
        2
    );
}
#[test]
fn circular_edge_profile_adds_real_segments_and_preserves_closed_cube() {
    let mesh = cube();
    let edge = mesh
        .data()
        .edges
        .iter()
        .find(|e| {
            e.vertices.iter().all(|&id| {
                let p = mesh.position(id).unwrap();
                p.x == 0.5 && p.z == 0.5
            })
        })
        .unwrap()
        .id;
    for count in [1, 2, 4, 8] {
        let out = bevel::apply(&mesh, &selection(Mode::Edge, vec![edge]), 0.15, count)
            .unwrap_or_else(|e| panic!("{count}: {e}"));
        let m = &out.mesh;
        closed(m);
        assert_eq!(m.data().faces.len(), 6 + count as usize);
        assert_eq!(m.data().vertices.len(), 8 + count as usize * 2);
        for v in m
            .data()
            .vertices
            .iter()
            .filter(|v| !mesh.prepared().vertices.contains_key(&v.id))
        {
            let p = Vec3::from(v.position);
            let r = ((p.x - 0.35).powi(2) + (p.z - 0.35).powi(2)).sqrt();
            assert!((r - 0.15).abs() < 2e-5, "radius {r}");
        }
        let mapped = atlas::allocate(m, &out.new_faces, [512, 512], 2).unwrap();
        assert_eq!(mapped.mesh.data().faces.len(), m.data().faces.len());
    }
}
#[test]
fn adjacent_edge_chains_and_all_cube_junctions_have_no_holes() {
    let mesh = cube();
    let v = mesh.data().vertices[0].id;
    let edges = mesh
        .data()
        .edges
        .iter()
        .filter(|e| e.vertices.contains(&v))
        .map(|e| e.id)
        .collect::<Vec<_>>();
    for ids in [
        edges[..2].to_vec(),
        edges,
        mesh.data().edges.iter().map(|e| e.id).collect(),
    ] {
        for segments in [1, 2, 4, 8] {
            let out = bevel::apply(&mesh, &selection(Mode::Edge, ids.clone()), 0.1, segments)
                .unwrap_or_else(|e| panic!("{} edges / {segments}: {e}", ids.len()));
            closed(&out.mesh);
            assert!(out.mesh.data().faces.len() > mesh.data().faces.len());
        }
    }
}
#[test]
fn convex_vertices_have_real_subdivision_and_multiple_corners_are_safe() {
    let mesh = cube();
    for ids in [
        vec![mesh.data().vertices[0].id],
        vec![mesh.data().vertices[0].id, mesh.data().vertices[7].id],
    ] {
        let mut previous = 0;
        for segments in [1, 2, 4, 8] {
            let out = bevel::apply(&mesh, &selection(Mode::Vertex, ids.clone()), 0.2, segments)
                .unwrap_or_else(|e| panic!("{segments}: {e}"));
            closed(&out.mesh);
            assert!(out.mesh.data().faces.len() > previous);
            previous = out.mesh.data().faces.len();
        }
    }
}
#[test]
fn width_and_invalid_selections_are_rejected_without_changing_source() {
    let mesh = cube();
    let before = serde_json::to_vec(&mesh).unwrap();
    let s = selection(Mode::Edge, vec![mesh.data().edges[0].id]);
    for width in [0., -1., 0.5, 1., f32::NAN] {
        assert!(bevel::apply(&mesh, &s, width, 4).is_err());
    }
    let open = edit::delete(
        &mesh,
        &selection(Mode::Face, vec![mesh.data().faces[0].id]),
        true,
    )
    .unwrap();
    assert!(bevel::apply(&open, &s, 0.1, 4).is_err());
    assert_eq!(serde_json::to_vec(&mesh).unwrap(), before);
}
#[test]
fn exposed_prism_edges_preserve_the_hollow_tube() {
    let mesh = primitives::generate(Primitive::Tube, 8, Parameters::default()).unwrap();
    let edge = mesh
        .data()
        .edges
        .iter()
        .find(|e| {
            let a = mesh.position(e.vertices[0]).unwrap();
            let b = mesh.position(e.vertices[1]).unwrap();
            a.truncate().is_finite() && (a.y - b.y).abs() > 0.9 && a.x * a.x + a.z * a.z > 0.24
        })
        .unwrap()
        .id;
    let out = bevel::apply(&mesh, &selection(Mode::Edge, vec![edge]), 0.02, 4).unwrap();
    let m = &out.mesh;
    assert!(m.prepared().incident_faces.iter().all(|f| f.len() == 2));
    assert!(
        m.prepared()
            .acceleration
            .hit(Vec3::new(0., 2., 0.), -Vec3::Y, |i| m
                .triangle_points(&m.prepared().triangles[i]))
            .is_none()
    );
}
