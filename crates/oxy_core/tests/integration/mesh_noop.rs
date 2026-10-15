use oxy_core::{
    document::Primitive,
    geometry::{
        edit,
        primitives::{self, Parameters},
        selection::{Mode, Selection},
    },
};
fn faces(ids: Vec<u32>) -> Selection {
    Selection {
        mode: Mode::Face,
        ids,
        through: false,
    }
}
#[test]
fn triangulating_existing_triangles_keeps_ids_and_the_same_storage_revision() {
    let mesh = primitives::generate(Primitive::Cone, 8, Parameters::default()).unwrap();
    let selection = faces(
        mesh.data()
            .faces
            .iter()
            .filter(|f| f.corners.len() == 3)
            .map(|f| f.id)
            .collect(),
    );
    assert!(!selection.ids.is_empty());
    let result = edit::triangulate(&mesh, &selection).unwrap();
    assert!(result.shares_storage(&mesh));
    assert_eq!(result.revision(), mesh.revision());
    assert_eq!(result, mesh);
}
#[test]
fn mixed_triangle_and_quad_selection_only_changes_the_quad() {
    let cube = primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap();
    let mesh = edit::triangulate(&cube, &faces(vec![cube.data().faces[0].id])).unwrap();
    let triangle = mesh
        .data()
        .faces
        .iter()
        .find(|f| f.corners.len() == 3)
        .unwrap();
    let quad = mesh
        .data()
        .faces
        .iter()
        .find(|f| f.corners.len() == 4)
        .unwrap();
    let result = edit::triangulate(&mesh, &faces(vec![triangle.id, quad.id])).unwrap();
    assert_eq!(result.face(triangle.id), Some(triangle));
    assert_eq!(result.data().faces.len(), mesh.data().faces.len() + 1);
    for face in mesh.data().faces.iter().filter(|f| f.id != quad.id) {
        assert_eq!(result.face(face.id), Some(face));
    }
    assert!(edit::triangulate(&mesh, &faces(vec![u32::MAX])).is_err());
}
