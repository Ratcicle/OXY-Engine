use super::*;
use oxy_core::{
    document::{Entity, Primitive},
    geometry::{Corner, MeshData, primitives},
};

fn polygon(points: &[[f32; 3]]) -> EditableMesh {
    let mut d = MeshData::default();
    let mut corners = Vec::new();
    for &p in points {
        corners.push(Corner {
            vertex: d.add_vertex(Vec3::from(p)).unwrap(),
            uv: [0., 0.],
            normal: None,
        });
    }
    d.add_face(corners).unwrap();
    d.complete_edges().unwrap();
    EditableMesh::new(d).unwrap()
}
fn rect() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(100., 100.))
}
fn identity(mesh: &EditableMesh) -> ProjectedMesh {
    ProjectedMesh::new(mesh, Mat4::IDENTITY, rect())
}
#[test]
fn face_intersection_accepts_partial_surface_and_rectangle_inside_face() {
    let mesh = polygon(&[
        [-0.9, -0.9, 0.5],
        [0.9, -0.9, 0.5],
        [0.9, 0.9, 0.5],
        [-0.9, 0.9, 0.5],
    ]);
    let p = identity(&mesh);
    let face = mesh.data().faces[0].id;
    assert_eq!(
        p.select(
            Mode::Face,
            Rect::from_min_max(Pos2::new(3., 3.), Pos2::new(8., 8.)),
            None
        ),
        vec![face]
    );
    assert_eq!(
        p.select(
            Mode::Face,
            Rect::from_min_max(Pos2::new(40., 40.), Pos2::new(45., 45.)),
            None
        ),
        vec![face]
    );
}
#[test]
fn boundary_contact_has_no_positive_area_and_does_not_add_neighbor() {
    let left = polygon(&[
        [-1., -1., 0.5],
        [0., -1., 0.5],
        [0., 1., 0.5],
        [-1., 1., 0.5],
    ]);
    let p = identity(&left);
    assert!(
        p.select(
            Mode::Face,
            Rect::from_min_max(Pos2::new(50., 20.), Pos2::new(70., 40.)),
            None
        )
        .is_empty()
    );
    assert_eq!(
        p.select(
            Mode::Face,
            Rect::from_min_max(Pos2::new(49.99, 20.), Pos2::new(70., 40.)),
            None
        )
        .len(),
        1
    );
}
#[test]
fn concave_face_uses_derived_triangles_instead_of_bounding_box() {
    let mesh = polygon(&[
        [-0.9, -0.9, 0.5],
        [0.9, -0.9, 0.5],
        [0.9, -0.3, 0.5],
        [-0.3, -0.3, 0.5],
        [-0.3, 0.9, 0.5],
        [-0.9, 0.9, 0.5],
    ]);
    let p = identity(&mesh);
    let hole = Rect::from_min_max(Pos2::new(60., 10.), Pos2::new(80., 30.));
    assert!(p.select(Mode::Face, hole, None).is_empty());
    assert_eq!(
        p.select(
            Mode::Face,
            Rect::from_min_max(Pos2::new(10., 10.), Pos2::new(20., 20.)),
            None
        )
        .len(),
        1
    );
}
#[test]
fn crossing_edge_is_selected_even_when_its_midpoint_is_outside() {
    let mesh = polygon(&[[-0.9, 0., 0.5], [0.9, 0., 0.5], [0., 0.9, 0.5]]);
    let edge = mesh
        .data()
        .edges
        .iter()
        .find(|e| {
            e.vertices
                .iter()
                .all(|id| mesh.position(*id).unwrap().y == 0.)
        })
        .unwrap()
        .id;
    let selected = identity(&mesh).select(
        Mode::Edge,
        Rect::from_min_max(Pos2::new(7., 47.), Pos2::new(10., 53.)),
        None,
    );
    assert!(selected.contains(&edge));
}
#[test]
fn near_plane_is_clipped_not_discarded_and_behind_camera_never_enters() {
    let camera = Mat4::perspective_rh(60f32.to_radians(), 1., 0.02, 100.);
    let behind = polygon(&[[-0.2, -0.2, 1.], [0.2, -0.2, 1.], [0., 0.2, 1.]]);
    assert!(
        ProjectedMesh::new(&behind, camera, rect())
            .select(Mode::Face, rect(), None)
            .is_empty()
    );
    let crossing = polygon(&[[-0.1, -0.1, -0.2], [0.1, -0.1, -0.2], [0., 0.1, 0.01]]);
    let p = ProjectedMesh::new(&crossing, camera, rect());
    assert_eq!(p.select(Mode::Face, rect(), None).len(), 1);
    assert!(p.faces.iter().flat_map(|f| &f.points).all(|p| {
        p.z >= -0.000001
            && p.z <= 1.000001
            && rect()
                .expand(0.001)
                .contains(Pos2::new(p.x as f32, p.y as f32))
    }));
}
fn scene_with_target_and_occluder(width: f32) -> (Scene, Id, CameraState, EditableMesh) {
    let mut scene = Scene::new("Seleção", SceneKind::ThreeD);
    let target_mesh = polygon(&[[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]]);
    let mut target = Entity::new("Peça", None);
    target.mesh = Some(target_mesh.clone());
    let id = target.id.clone();
    let mut cover = Entity::new("Outra peça", None);
    cover.mesh = Some(polygon(&[
        [-width, -1.1, 0.1],
        [width, -1.1, 0.1],
        [width, 1.1, 0.1],
        [-width, 1.1, 0.1],
    ]));
    scene.entities = vec![target, cover];
    let mut camera = CameraState::for_scene(&scene);
    camera.target = Vec3::ZERO;
    camera.distance = 3.;
    camera.yaw = 0.;
    camera.pitch = 0.;
    (scene, id, camera, target_mesh)
}
#[test]
fn visible_box_tests_reached_surface_not_occluded_face_center() {
    let (scene, id, camera, mesh) = scene_with_target_and_occluder(0.9);
    let p = ProjectedMesh::new(&mesh, camera.matrix([100, 100]), rect());
    let o = Occlusion::new(&scene, &SceneView::new(&scene), &camera, rect(), 1.);
    assert_eq!(
        p.select(Mode::Face, rect(), Some((&o, &id))).len(),
        1,
        "The narrow exposed border must count even with a covered center"
    );
    let center = Rect::from_min_max(Pos2::new(45., 45.), Pos2::new(55., 55.));
    assert!(p.select(Mode::Face, center, Some((&o, &id))).is_empty());
    assert_eq!(p.select(Mode::Face, center, None).len(), 1);
}
#[test]
fn full_occlusion_blocks_normal_but_through_never_needs_occlusion() {
    let (scene, id, camera, mesh) = scene_with_target_and_occluder(1.1);
    let p = ProjectedMesh::new(&mesh, camera.matrix([100, 100]), rect());
    let o = Occlusion::new(&scene, &SceneView::new(&scene), &camera, rect(), 1.);
    for mode in [Mode::Face, Mode::Edge, Mode::Vertex] {
        assert!(
            p.select(mode, rect(), Some((&o, &id))).is_empty(),
            "{mode:?}"
        );
        assert!(!p.select(mode, rect(), None).is_empty(), "{mode:?}");
    }
}
#[test]
fn cone_tip_selects_all_eight_sides_when_face_centers_are_outside() {
    let mesh = primitives::generate(
        Primitive::Cone,
        8,
        primitives::Parameters {
            height_divisions: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let scene = Scene::new("Cone", SceneKind::ThreeD);
    let mut camera = CameraState::for_scene(&scene);
    camera.target = Vec3::ZERO;
    camera.distance = 3.;
    camera.pitch = 0.15;
    let p = ProjectedMesh::new(&mesh, camera.matrix([100, 100]), rect());
    let top = camera.world_to_screen(Vec3::Y * 0.5, [100, 100]).unwrap();
    let tip = Rect::from_center_size(Pos2::from(top), egui::Vec2::splat(2.));
    let selected = p.select(Mode::Face, tip, None);
    let mut top_faces: Vec<_> = mesh
        .data()
        .faces
        .iter()
        .filter(|f| {
            f.corners
                .iter()
                .any(|c| mesh.position(c.vertex).unwrap().y > 0.499)
        })
        .map(|f| f.id)
        .collect();
    top_faces.sort_unstable();
    let mut actual = selected.clone();
    actual.sort_unstable();
    assert_eq!(actual, top_faces);
    assert_eq!(actual.len(), 8);
    assert!(top_faces.iter().any(|id| {
        let f = mesh.face(*id).unwrap();
        let center = f
            .corners
            .iter()
            .map(|c| mesh.position(c.vertex).unwrap())
            .sum::<Vec3>()
            / f.corners.len() as f32;
        !tip.contains(Pos2::from(
            camera.world_to_screen(center, [100, 100]).unwrap(),
        ))
    }));
}
#[test]
fn occluder_key_changes_for_transform_mesh_visibility_removal_and_reorder() {
    let (mut scene, _, _, _) = scene_with_target_and_occluder(0.9);
    let key = |s: &Scene| occluder_keys(s, &SceneView::new(s));
    let old = key(&scene);
    scene.entities[1].transform.position[0] = 1.;
    assert!(old != key(&scene));
    let old = key(&scene);
    scene.entities[1].visible = false;
    assert!(old != key(&scene));
    scene.entities[1].visible = true;
    let old = key(&scene);
    scene.entities.swap(0, 1);
    assert!(old != key(&scene));
    let old = key(&scene);
    scene.entities[0].mesh =
        Some(primitives::generate(Primitive::Cube, 8, Default::default()).unwrap());
    assert!(old != key(&scene));
    let old = key(&scene);
    scene.entities.remove(0);
    assert!(old != key(&scene));
}
#[test]
fn polygon_subtraction_preserves_narrow_visible_fragment() {
    let subject = rect_polygon(Rect::from_min_max(Pos2::ZERO, Pos2::new(100., 100.)));
    let mask = rect_polygon(Rect::from_min_max(Pos2::ZERO, Pos2::new(99.99, 100.)));
    let pieces = subtract_polygon(&subject, &mask);
    let remaining: f64 = pieces.iter().map(|p| area(p)).sum();
    assert!((remaining - 1.).abs() < 0.01);
}
#[test]
fn closed_cube_only_exposes_three_sides_from_oblique_view() {
    let mesh = primitives::generate(Primitive::Cube, 8, Default::default()).unwrap();
    let mut scene = Scene::new("Cubo", SceneKind::ThreeD);
    let mut e = Entity::new("Cubo", None);
    e.mesh = Some(mesh.clone());
    let id = e.id.clone();
    scene.entities.push(e);
    let mut camera = CameraState::for_scene(&scene);
    camera.target = Vec3::ZERO;
    camera.distance = 3.;
    let r = Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(600., 400.));
    let p = ProjectedMesh::new(&mesh, camera.matrix([600, 400]), r);
    let o = Occlusion::new(&scene, &SceneView::new(&scene), &camera, r, 1.);
    assert_eq!(p.select(Mode::Face, r, Some((&o, &id))).len(), 3);
}
#[test]
fn parametric_dimensions_do_not_make_a_plane_occlude_itself() {
    for n in 0..12 {
        let mut scene = Scene::new("Plano paramétrico", SceneKind::ThreeD);
        let mut e = Entity::new("Plano", Some(Primitive::Plane));
        e.dimensions = [1.33 + n as f32 * 0.11, 1., 0.71];
        e.transform.rotation = [15.5, 18.9, 29.2];
        let id = e.id.clone();
        let mut converted = e.clone();
        primitives::convert(&mut converted).unwrap();
        let mesh = converted.mesh.unwrap();
        scene.entities.push(e);
        let camera = CameraState::for_scene(&scene);
        let r = Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(600., 400.));
        let view = SceneView::new(&scene);
        let p = ProjectedMesh::new(
            &mesh,
            camera.matrix([600, 400]) * view.world_matrix(&id).unwrap(),
            r,
        );
        let o = Occlusion::with_target(&scene, &view, &camera, r, 1., (&id, &p));
        assert_eq!(
            p.select(Mode::Face, r, Some((&o, &id))).len(),
            mesh.data().faces.len(),
            "dimension case {n}"
        );
        assert_eq!(
            p.select(Mode::Vertex, r, Some((&o, &id))).len(),
            mesh.data().vertices.len(),
            "dimension case {n}"
        );
    }
}
