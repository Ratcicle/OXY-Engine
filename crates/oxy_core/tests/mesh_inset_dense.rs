use glam::Vec3;
use oxy_core::{
    document::Primitive,
    geometry::{
        inset,
        primitives::{self, Parameters},
        selection::{Mode, Selection},
    },
};

#[test]
fn dense_benchmark_plane_preserves_manifold_inset_with_positive_and_negative_extrusion() {
    let mesh = primitives::generate(
        Primitive::Plane,
        8,
        Parameters {
            plane_divisions: [158; 2],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(mesh.prepared().triangles.len(), 49_928);
    let unchanged = mesh.data().clone();
    // Keep the exact failing benchmark selection, plus cells at other magnitudes
    // and at the authored border. IDs and input geometry are deterministic.
    for face_index in [
        mesh.data().faces.len() / 3,
        0,
        mesh.data().faces.len() / 2,
        mesh.data().faces.len() - 1,
    ] {
        let face = &mesh.data().faces[face_index];
        let selected = Selection {
            mode: Mode::Face,
            ids: vec![face.id],
            through: false,
        };
        let normal = mesh.prepared().face_normals[face_index];
        for distance in [0.02, -0.02, 0.] {
            let result = inset::apply(&mesh, &selected, 70., normal * distance, false)
                .unwrap_or_else(|e| panic!("distance={distance}, face={}: {e}", face.id));
            assert!(
                result
                    .mesh
                    .prepared()
                    .incident_faces
                    .iter()
                    .all(|faces| faces.len() <= 2)
            );
            assert!(!result.selection.ids.is_empty());
            assert_eq!(
                result.new_faces.len(),
                if distance == 0. { 0 } else { 4 },
                "A diagonal das partições UV não deve gerar parede"
            );
            let mut perimeter = 0.;
            for (index, uses) in result.mesh.prepared().incident_faces.iter().enumerate() {
                if uses.len() == 1 {
                    let edge = &result.mesh.data().edges[index];
                    let [a, b] = edge.vertices.map(|id| result.mesh.position(id).unwrap());
                    assert!(
                        [0, 2]
                            .into_iter()
                            .any(|axis| [-0.5, 0.5].into_iter().any(|boundary| (a[axis]
                                - boundary)
                                .abs()
                                < 1e-7
                                && (b[axis] - boundary).abs() < 1e-7)),
                        "Costura aberta dentro da superfície: {a:?}, {b:?}"
                    );
                    perimeter += a.distance(b);
                }
            }
            assert!(
                (perimeter - 4.).abs() < 0.0001,
                "O contorno aberto original deve ser preservado"
            );
            let base = mesh.position(face.corners[0].vertex).unwrap();
            for point in result
                .selection
                .vertices(&result.mesh)
                .iter()
                .map(|&id| result.mesh.position(id).unwrap())
            {
                assert!(((point - base).dot(normal) - distance).abs() < 1e-6);
            }
            let min = result
                .selection
                .vertices(&result.mesh)
                .iter()
                .map(|&id| result.mesh.position(id).unwrap())
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let max = result
                .selection
                .vertices(&result.mesh)
                .iter()
                .map(|&id| result.mesh.position(id).unwrap())
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            assert!((max.x - min.x - 0.7 / 158.).abs() < 1e-7);
            assert!((max.z - min.z - 0.7 / 158.).abs() < 1e-7);
            let area: f32 = result
                .mesh
                .prepared()
                .triangles
                .iter()
                .filter(|t| result.selection.ids.contains(&t.face))
                .map(|t| {
                    let [a, b, c] = result.mesh.triangle_points(t);
                    (b - a).cross(c - a).length() * 0.5
                })
                .sum();
            assert!(
                (area - 0.49 / (158. * 158.)).abs() < 1e-9,
                "Área da tampa foi duplicada ou perdida"
            );
        }
    }
    assert_eq!(mesh.data(), &unchanged);
}
