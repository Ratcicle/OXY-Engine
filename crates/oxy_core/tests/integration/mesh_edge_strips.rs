use glam::Vec3;
use oxy_core::geometry::{
    Corner, EditableMesh, MeshData, operations,
    selection::{Mode, Selection},
};

fn selection(mode: Mode, ids: Vec<u32>) -> Selection {
    Selection {
        mode,
        ids,
        through: false,
    }
}

fn add_edges(mut mesh: EditableMesh, pairs: &[[u32; 2]]) -> (EditableMesh, Vec<u32>) {
    let mut edges = Vec::new();
    for ends in pairs {
        let output = operations::create(&mesh, &selection(Mode::Vertex, ends.to_vec())).unwrap();
        edges.push(output.selection.ids[0]);
        mesh = output.mesh;
    }
    (mesh, edges)
}

fn opposite_half_edges(mesh: &EditableMesh) -> usize {
    let mut shared = 0;
    for uses in &mesh.prepared().incident_faces {
        assert!(uses.len() <= 2);
        if let [(a, ac), (b, bc)] = uses.as_slice() {
            let first = &mesh.data().faces[*a];
            let second = &mesh.data().faces[*b];
            assert_eq!(
                first.corners[*ac].vertex,
                second.corners[(*bc + 1) % second.corners.len()].vertex
            );
            assert_eq!(
                first.corners[(*ac + 1) % first.corners.len()].vertex,
                second.corners[*bc].vertex
            );
            shared += 1;
        }
    }
    shared
}

#[test]
fn reversed_loose_edges_extrude_into_one_consistently_oriented_strip() {
    let mut data = MeshData::default();
    let [a, b, c] = [Vec3::ZERO, Vec3::X, Vec3::X * 2.].map(|p| data.add_vertex(p).unwrap());
    // This is a valid ordered construction through the same API used by the editor.
    let (mesh, edges) = add_edges(EditableMesh::new(data).unwrap(), &[[a, b], [c, b]]);
    let original = mesh.clone();
    let output = operations::extrude(&mesh, &selection(Mode::Edge, edges), Vec3::Y, false).unwrap();
    assert_eq!(mesh, original);
    assert_eq!(output.mesh.data().faces.len(), 2);
    assert_eq!(opposite_half_edges(&output.mesh), 1);
    for normal in &output.mesh.prepared().face_normals {
        assert!(normal.abs_diff_eq(Vec3::Z, 1e-6));
    }
    for normal in &output.mesh.prepared().smooth_normals {
        assert!(normal.abs_diff_eq(Vec3::Z, 1e-6));
    }
}

#[test]
fn reversed_loose_loop_has_opposite_shared_edges_and_outward_walls() {
    let mut data = MeshData::default();
    let [a, b, c, d] =
        [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y].map(|p| data.add_vertex(p).unwrap());
    let (mesh, edges) = add_edges(
        EditableMesh::new(data).unwrap(),
        &[[a, b], [c, b], [c, d], [a, d]],
    );
    let output = operations::extrude(&mesh, &selection(Mode::Edge, edges), Vec3::Z, false).unwrap();
    let mesh = output.mesh;
    assert_eq!(mesh.data().vertices.len(), 8);
    assert_eq!(mesh.data().edges.len(), 12);
    assert_eq!(mesh.data().faces.len(), 4);
    assert_eq!(opposite_half_edges(&mesh), 4);
    for (index, face) in mesh.data().faces.iter().enumerate() {
        let center = face
            .corners
            .iter()
            .map(|c| mesh.position(c.vertex).unwrap())
            .sum::<Vec3>()
            / 4.;
        assert!(mesh.prepared().face_normals[index].dot(center - Vec3::splat(0.5)) > 0.49);
    }
}

fn corner(vertex: u32) -> Corner {
    Corner {
        vertex,
        uv: [0., 0.],
        normal: None,
    }
}

#[test]
fn existing_face_controls_winding_even_when_loose_edge_is_selected_first() {
    let mut data = MeshData::default();
    let [a, b, c, d] =
        [Vec3::ZERO, Vec3::X, Vec3::X * 2., -Vec3::Y].map(|p| data.add_vertex(p).unwrap());
    let face = data.add_face([b, a, d].map(corner).to_vec()).unwrap();
    data.complete_edges().unwrap();
    let mesh = EditableMesh::new(data).unwrap();
    let ab = mesh
        .data()
        .edges
        .iter()
        .find(|e| e.vertices.contains(&a) && e.vertices.contains(&b))
        .unwrap()
        .id;
    let original_face = mesh.face(face).unwrap().clone();
    let (mesh, loose) = add_edges(mesh, &[[c, b]]);
    let output = operations::extrude(
        &mesh,
        &selection(Mode::Edge, vec![loose[0], ab]),
        Vec3::Y,
        false,
    )
    .unwrap();
    assert_eq!(output.mesh.face(face).unwrap(), &original_face);
    assert_eq!(opposite_half_edges(&output.mesh), 2);
    for normal in &output.mesh.prepared().face_normals {
        assert!(normal.abs_diff_eq(Vec3::Z, 1e-6));
    }
}

#[test]
fn conflicting_boundary_winding_is_rejected_without_changing_source() {
    let mut data = MeshData::default();
    let [a, b, c, d, e] = [
        Vec3::ZERO,
        Vec3::X,
        Vec3::X * 2.,
        -Vec3::Y,
        Vec3::X * 2. - Vec3::Y,
    ]
    .map(|p| data.add_vertex(p).unwrap());
    data.add_face([b, a, d].map(corner).to_vec()).unwrap();
    data.add_face([b, c, e].map(corner).to_vec()).unwrap();
    data.complete_edges().unwrap();
    let mesh = EditableMesh::new(data).unwrap();
    let edges = mesh
        .data()
        .edges
        .iter()
        .filter(|edge| {
            edge.vertices.contains(&b) && (edge.vertices.contains(&a) || edge.vertices.contains(&c))
        })
        .map(|e| e.id)
        .collect();
    let original = mesh.clone();
    let error = operations::extrude(&mesh, &selection(Mode::Edge, edges), Vec3::Y, false)
        .err()
        .unwrap();
    assert!(error.contains("sentidos incompatíveis"), "{error}");
    assert_eq!(mesh, original);
}
