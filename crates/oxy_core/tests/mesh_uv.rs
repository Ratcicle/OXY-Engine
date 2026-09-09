use glam::{Vec2, Vec3};
use oxy_core::{
    document::Primitive,
    geometry::{primitives::Parameters, *},
};

#[test]
fn actual_uv_picking_and_shading_keep_topology_and_pixels_domain() {
    let mesh = primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap();
    for face in &mesh.data().faces {
        let center =
            face.corners.iter().map(|c| Vec2::from(c.uv)).sum::<Vec2>() / face.corners.len() as f32;
        assert_eq!(uv::pick(&mesh, center, &[]), Some(face.id));
    }
    assert_eq!(uv::pick(&mesh, Vec2::new(-1., -1.), &[]), None);
    let smooth = mesh.with_shading(Shading::Smooth).unwrap();
    let flat = smooth.with_shading(Shading::Flat).unwrap();
    assert_eq!(mesh.data().vertices, smooth.data().vertices);
    assert_eq!(mesh.data().edges, smooth.data().edges);
    for (i, face) in flat.data().faces.iter().enumerate() {
        for (a, b) in face.corners.iter().zip(&mesh.data().faces[i].corners) {
            assert_eq!(a.uv, b.uv);
            assert!(
                flat.corner_normal(i, a)
                    .abs_diff_eq(flat.prepared().face_normals[i], 1e-6)
            );
            let n = smooth.corner_normal(i, a);
            assert!((n.length() - 1.).abs() < 1e-6);
            assert!(!n.abs_diff_eq(flat.corner_normal(i, a), 0.1));
        }
    }
    assert_eq!(flat.prepared().bounds.unwrap().0, Vec3::splat(-0.5));
}
