use glam::{Vec2, Vec3};
use oxy_core::{
    document::*,
    geometry::{
        primitives::Parameters,
        selection::{Mode, Selection},
        *,
    },
};
fn cube() -> EditableMesh {
    primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap()
}
fn select(mode: Mode, ids: Vec<u32>) -> Selection {
    Selection {
        mode,
        ids,
        through: false,
    }
}
fn closed(mesh: &EditableMesh) {
    for (i, e) in mesh.data().edges.iter().enumerate() {
        let uses = &mesh.prepared().incident_faces[i];
        assert_eq!(uses.len(), 2, "edge {}", e.id);
        let (a, ai) = uses[0];
        let (b, bi) = uses[1];
        assert_ne!(
            mesh.data().faces[a].corners[ai].vertex,
            mesh.data().faces[b].corners[bi].vertex,
            "winding {}",
            e.id
        );
    }
    assert_eq!(
        mesh.data().vertices.len() as isize - mesh.data().edges.len() as isize
            + mesh.data().faces.len() as isize,
        2
    );
}
#[test]
fn adjacent_face_region_has_only_boundary_walls_and_stable_caps() {
    let mesh = cube();
    let selection = select(
        Mode::Face,
        mesh.data().faces[..2].iter().map(|f| f.id).collect(),
    );
    let out = operations::extrude(&mesh, &selection, Vec3::new(0.2, 0., 0.2), false).unwrap();
    assert_eq!(
        (
            out.mesh.data().vertices.len(),
            out.mesh.data().edges.len(),
            out.mesh.data().faces.len()
        ),
        (14, 24, 12)
    );
    assert_eq!(out.new_faces.len(), 6);
    assert_eq!(out.selection, selection);
    closed(&out.mesh);
    for &id in &selection.ids {
        assert_eq!(
            mesh.face(id)
                .unwrap()
                .corners
                .iter()
                .map(|c| c.uv)
                .collect::<Vec<_>>(),
            out.mesh
                .face(id)
                .unwrap()
                .corners
                .iter()
                .map(|c| c.uv)
                .collect::<Vec<_>>()
        );
    }
    let individual = operations::extrude(&mesh, &selection, Vec3::new(0.2, 0., 0.2), true).unwrap();
    assert_eq!(individual.new_faces.len(), 8);
    closed(&individual.mesh);
    assert!(operations::extrude(&mesh, &selection, Vec3::ZERO, false).is_err());
}
#[test]
fn edge_strips_and_vertex_chains_do_not_invent_volumes() {
    let plane = primitives::generate(Primitive::Plane, 8, Parameters::default()).unwrap();
    let edge = plane.data().edges[0].id;
    let out = operations::extrude(&plane, &select(Mode::Edge, vec![edge]), Vec3::Y, false).unwrap();
    assert_eq!(
        (
            out.mesh.data().vertices.len(),
            out.mesh.data().edges.len(),
            out.mesh.data().faces.len()
        ),
        (6, 7, 2)
    );
    assert_eq!(
        out.mesh.prepared().incident_faces[out.mesh.prepared().edges[&edge]].len(),
        2
    );
    assert_eq!(out.selection.mode, Mode::Edge);
    assert_ne!(out.selection.ids[0], edge);
    assert!(
        operations::extrude(&out.mesh, &select(Mode::Edge, vec![edge]), Vec3::Y, false).is_err()
    );
    let mesh = cube();
    let mut selection = select(Mode::Vertex, vec![mesh.data().vertices[0].id]);
    let mut current = mesh.clone();
    for _ in 0..3 {
        let out = operations::extrude(&current, &selection, Vec3::X, false).unwrap();
        current = out.mesh;
        selection = out.selection;
    }
    assert_eq!(
        (
            current.data().vertices.len(),
            current.data().edges.len(),
            current.data().faces.len()
        ),
        (11, 15, 6)
    );
}
#[test]
fn construction_caps_the_boundary_without_duplicate_or_crossed_faces() {
    let mesh = cube();
    let removed = mesh.data().faces[0].id;
    let open = edit::delete(&mesh, &select(Mode::Face, vec![removed]), true).unwrap();
    let border = open
        .data()
        .edges
        .iter()
        .enumerate()
        .filter(|(i, _)| open.prepared().incident_faces[*i].len() == 1)
        .map(|(_, e)| e.id)
        .collect();
    let out = operations::create(&open, &select(Mode::Edge, border)).unwrap();
    closed(&out.mesh);
    assert_eq!(out.mesh.data().faces.len(), 6);
    let by_faces = operations::create(
        &open,
        &select(Mode::Face, open.data().faces.iter().map(|f| f.id).collect()),
    )
    .unwrap();
    closed(&by_faces.mesh);
    assert!(operations::create(&mesh, &select(Mode::Face, vec![removed])).is_err());
    let mut loose = MeshData::default();
    let ids = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]
        .map(|p| loose.add_vertex(Vec3::from(p)).unwrap());
    let loose = EditableMesh::new(loose).unwrap();
    let edge = operations::create(&loose, &select(Mode::Vertex, ids[..2].to_vec())).unwrap();
    assert_eq!(edge.mesh.data().edges.len(), 1);
    assert!(edge.mesh.data().faces.is_empty());
    assert!(operations::create(&edge.mesh, &select(Mode::Vertex, ids[..2].to_vec())).is_err());
    assert!(
        operations::create(
            &loose,
            &select(Mode::Vertex, vec![ids[0], ids[2], ids[1], ids[3]])
        )
        .is_err()
    );
    let face = operations::create(&loose, &select(Mode::Vertex, ids.to_vec())).unwrap();
    assert_eq!(face.mesh.prepared().triangles.len(), 2);
}
#[test]
fn flip_preserves_uv_binding_and_is_its_own_inverse() {
    let mesh = cube();
    let s = select(Mode::Face, vec![mesh.data().faces[0].id]);
    let out = operations::flip(&mesh, &s).unwrap();
    for c in &mesh.face(s.ids[0]).unwrap().corners {
        let new = out
            .mesh
            .face(s.ids[0])
            .unwrap()
            .corners
            .iter()
            .find(|v| v.vertex == c.vertex)
            .unwrap();
        assert_eq!(c.uv, new.uv);
    }
    assert!(
        out.mesh.prepared().face_normals[0].abs_diff_eq(-mesh.prepared().face_normals[0], 1e-6)
    );
    assert_eq!(operations::flip(&out.mesh, &s).unwrap().mesh, mesh);
}
#[test]
fn new_islands_require_space_and_never_overlap_old_paint() {
    let mesh = cube();
    let s = select(Mode::Face, vec![mesh.data().faces[0].id]);
    let out = operations::extrude(&mesh, &s, Vec3::Z * 0.25, false).unwrap();
    assert!(atlas::allocate(&out.mesh, &out.new_faces, [256, 256], 1).is_err());
    let allocated = atlas::allocate(&out.mesh, &out.new_faces, [256, 256], 2).unwrap();
    assert_eq!(allocated.expansion, 2);
    for face in &mesh.data().faces {
        for (old, new) in face
            .corners
            .iter()
            .zip(&allocated.mesh.face(face.id).unwrap().corners)
        {
            assert_eq!(Vec2::from(old.uv) * 0.5, Vec2::from(new.uv));
        }
    }
    let mut regions = Vec::new();
    for id in &out.new_faces {
        let face = allocated.mesh.face(*id).unwrap();
        let min = face
            .corners
            .iter()
            .map(|c| Vec2::from(c.uv))
            .fold(Vec2::splat(f32::INFINITY), Vec2::min);
        let max = face
            .corners
            .iter()
            .map(|c| Vec2::from(c.uv))
            .fold(Vec2::ZERO, Vec2::max);
        assert!((max - min).min_element() > 0.001);
        assert!(min.x > 0.5 || min.y > 0.5);
        for &(a, b) in &regions {
            let a: Vec2 = a;
            let b: Vec2 = b;
            assert!(min.x >= b.x || min.y >= b.y || max.x <= a.x || max.y <= a.y);
        }
        regions.push((min, max));
    }
}
#[test]
fn vertex_alignment_preserves_children_and_rejects_unsolvable_scale() {
    use oxy_core::geometry::snap::{self, Method};
    let mut scene = Scene::new("Snap", SceneKind::ThreeD);
    let mut root = Entity::new("Grupo", None);
    root.transform.position = [1., 0., 0.];
    let mut child = Entity::new("Peça", Some(Primitive::Cube));
    child.parent = Some(root.id.clone());
    child.transform.position = [0., 1., 0.];
    let ids = vec![root.id.clone(), child.id.clone()];
    scene.entities = vec![root, child];
    let before = scene.world_matrix(&ids[1]).unwrap();
    snap::apply(
        &mut scene,
        &ids,
        Vec3::X,
        Vec3::new(3., 2., 0.),
        Method::Move,
    )
    .unwrap();
    assert!(
        (scene
            .world_matrix(&ids[1])
            .unwrap()
            .transform_point3(Vec3::ZERO)
            - before.transform_point3(Vec3::ZERO))
        .abs_diff_eq(Vec3::new(2., 2., 0.), 1e-6)
    );
    let delta = snap::delta(
        Vec3::new(2., 1., 0.),
        Vec3::new(4., 1., 0.),
        Method::Scale {
            axis: 0,
            pivot: Vec3::ZERO,
        },
    )
    .unwrap();
    assert!(
        delta
            .transform_point3(Vec3::new(2., 1., 0.))
            .abs_diff_eq(Vec3::new(4., 1., 0.), 1e-6)
    );
    assert!(
        snap::delta(
            Vec3::Y,
            Vec3::X,
            Method::Scale {
                axis: 0,
                pivot: Vec3::ZERO
            }
        )
        .is_err()
    );
    assert!(
        snap::delta(
            Vec3::X,
            Vec3::ZERO,
            Method::Scale {
                axis: 0,
                pivot: Vec3::ZERO
            }
        )
        .is_err()
    );
    assert!(
        snap::delta(
            Vec3::X,
            Vec3::Y,
            Method::Scale {
                axis: 0,
                pivot: Vec3::ZERO
            }
        )
        .is_err()
    );
}
