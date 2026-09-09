use glam::{Mat4, Quat, Vec2, Vec3};
use oxy_core::{
    document::{self, Entity, Primitive, Project},
    edit_history::CommandHistory,
    geometry::{
        self, Corner, EditableMesh, atlas, inset, operations,
        primitives::{self, Parameters},
        selection::{Mode, Selection},
    },
    painting::PaintImage,
    persistence, spatial,
    texture_cache::TextureCache,
};
use std::collections::{HashMap, HashSet};

fn shape(primitive: Primitive) -> EditableMesh {
    primitives::generate(primitive, 8, Parameters::default()).unwrap()
}
fn selected(ids: Vec<u32>) -> Selection {
    Selection {
        mode: Mode::Face,
        ids,
        through: false,
    }
}
fn first(mesh: &EditableMesh) -> Selection {
    selected(vec![mesh.data().faces[0].id])
}
fn area(mesh: &EditableMesh, ids: &[u32]) -> f32 {
    mesh.prepared()
        .triangles
        .iter()
        .filter(|t| ids.contains(&t.face))
        .map(|t| {
            let [a, b, c] = mesh.triangle_points(t);
            (b - a).cross(c - a).length() * 0.5
        })
        .sum()
}
fn cap_bounds(output: &operations::Output) -> (Vec3, Vec3) {
    output
        .selection
        .vertices(&output.mesh)
        .iter()
        .map(|&id| output.mesh.position(id).unwrap())
        .fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(a, b), p| (a.min(p), b.max(p)),
        )
}
fn uv_at(mesh: &EditableMesh, p: Vec3, normal: Vec3) -> Vec2 {
    let (index, _, bary) = mesh
        .prepared()
        .acceleration
        .hit(p + normal * 2., -normal, |i| {
            mesh.triangle_points(&mesh.prepared().triangles[i])
        })
        .unwrap();
    let triangle = &mesh.prepared().triangles[index];
    let face = mesh.face(triangle.face).unwrap();
    triangle
        .corners
        .iter()
        .zip(bary)
        .map(|(&i, b)| Vec2::from(face.corners[i].uv) * b)
        .sum()
}

#[test]
fn half_linear_size_keeps_frame_on_original_plane_for_positive_negative_and_zero_distance() {
    let mesh = shape(Primitive::Rectangle);
    for distance in [0., 0.2, -0.2] {
        let out = inset::apply(&mesh, &first(&mesh), 50., Vec3::Z * distance, false).unwrap();
        let (min, max) = cap_bounds(&out);
        assert!((max.x - min.x - 0.5).abs() < 1e-5);
        assert!((max.y - min.y - 0.5).abs() < 1e-5);
        assert!((min.z - distance).abs() < 1e-5 && (max.z - distance).abs() < 1e-5);
        assert!((area(&out.mesh, &out.selection.ids) - 0.25).abs() < 1e-5);
        let frame: Vec<_> = out
            .mesh
            .data()
            .faces
            .iter()
            .filter(|f| !out.selection.ids.contains(&f.id) && !out.new_faces.contains(&f.id))
            .map(|f| f.id)
            .collect();
        assert!((area(&out.mesh, &frame) - 0.75).abs() < 1e-5);
        assert!(
            frame
                .iter()
                .flat_map(|&id| &out.mesh.face(id).unwrap().corners)
                .all(|c| out.mesh.position(c.vertex).unwrap().z.abs() < 1e-6)
        );
        assert_eq!(out.new_faces.is_empty(), distance == 0.);
        assert!(
            out.selection
                .ids
                .iter()
                .all(|id| !out.new_faces.contains(id))
        );
    }
}

#[test]
fn hundred_percent_matches_existing_extrusion_and_zero_is_identical_storage() {
    let mesh = shape(Primitive::Cube);
    let selection = first(&mesh);
    let delta = mesh.prepared().face_normals[0] * 0.2;
    let expected = operations::extrude(&mesh, &selection, delta, false).unwrap();
    let out = inset::apply(&mesh, &selection, 100., delta, false).unwrap();
    assert_eq!(out.mesh, expected.mesh);
    assert_eq!(out.selection, expected.selection);
    assert_eq!(out.new_faces, expected.new_faces);
    let no_op = inset::apply(&mesh, &selection, 100., Vec3::ZERO, false).unwrap();
    assert!(no_op.mesh.shares_storage(&mesh));
    assert_eq!(no_op.mesh.revision(), mesh.revision());
    assert!(no_op.new_faces.is_empty());
}

#[test]
fn selected_uv_partitioned_caps_can_be_inset_and_extruded_again_as_one_region() {
    let mesh = shape(Primitive::Rectangle);
    let first = inset::apply(&mesh, &first(&mesh), 50., Vec3::Z * 0.1, false).unwrap();
    assert!(first.selection.ids.len() > 1);
    let second = inset::apply(&first.mesh, &first.selection, 70., Vec3::Z * 0.1, false).unwrap();
    let (min, max) = cap_bounds(&second);
    assert!((max.x - min.x - 0.35).abs() < 1e-5);
    assert!((max.y - min.y - 0.35).abs() < 1e-5);
    assert!((min.z - 0.2).abs() < 1e-5 && (max.z - 0.2).abs() < 1e-5);
    let third = inset::apply(&second.mesh, &second.selection, 70., Vec3::ZERO, false).unwrap();
    assert!(third.new_faces.is_empty());
    let (min, max) = cap_bounds(&third);
    assert!((max.x - min.x - 0.245).abs() < 1e-5);
    assert!((min.z - 0.2).abs() < 1e-5);
}

#[test]
fn invalid_percentages_and_swept_intersections_are_rejected_atomically() {
    let mesh = shape(Primitive::Cube);
    let selection = first(&mesh);
    let normal = mesh.prepared().face_normals[0];
    let before = mesh.data().clone();
    let revision = mesh.revision();
    for size in [0., -10., 101., f32::NAN, 0.0000001, 99.99999] {
        assert!(
            inset::apply(&mesh, &selection, size, normal * 0.2, false).is_err(),
            "size {size}"
        );
    }
    assert!(inset::apply(&mesh, &selection, 70., normal * -0.25, false).is_ok());
    for (size, distance) in [(70., -1.2), (100., -1.2), (70., -1.0), (100., -1.0)] {
        let error = inset::apply(&mesh, &selection, size, normal * distance, false)
            .err()
            .unwrap();
        assert!(error.contains("atravessaria"), "{error}");
    }
    assert_eq!(mesh.data(), &before);
    assert_eq!(mesh.revision(), revision);
}

#[test]
fn connected_quad_region_is_welded_with_only_outer_walls_and_each_face_is_independent() {
    let mesh = primitives::generate(
        Primitive::Plane,
        8,
        Parameters {
            plane_divisions: [2, 2],
            ..Default::default()
        },
    )
    .unwrap();
    let selection = selected(mesh.data().faces.iter().map(|f| f.id).collect());
    let normal = mesh.prepared().face_normals[0];
    let out = inset::apply(&mesh, &selection, 50., normal * 0.2, false).unwrap();
    assert!((area(&out.mesh, &out.selection.ids) - 0.25).abs() < 1e-5);
    for &id in &out.new_faces {
        let wall = out.mesh.face(id).unwrap();
        let base: Vec<_> = wall
            .corners
            .iter()
            .map(|c| out.mesh.position(c.vertex).unwrap())
            .filter(|p| p.y.abs() < 1e-5)
            .collect();
        assert_eq!(base.len(), 2);
        assert!(
            base.iter().all(|p| (p.x.abs() - 0.25).abs() < 1e-5)
                || base.iter().all(|p| (p.z.abs() - 0.25).abs() < 1e-5)
        );
    }
    let each = inset::apply(&mesh, &selection, 50., normal * 0.2, true).unwrap();
    assert!((area(&each.mesh, &each.selection.ids) - 0.25).abs() < 1e-5);
    assert!(each.new_faces.len() > out.new_faces.len());
    // Every cap edge is joined to another cap or a wall; none is a crack/T-junction.
    for &id in &each.selection.ids {
        let face = each.mesh.face(id).unwrap();
        for i in 0..face.corners.len() {
            let edge = geometry::pair([
                face.corners[i].vertex,
                face.corners[(i + 1) % face.corners.len()].vertex,
            ]);
            assert_eq!(
                each.mesh.prepared().incident_faces[each.mesh.prepared().edge_pairs[&edge]].len(),
                2
            );
        }
    }
}

#[test]
fn original_triangle_uv_mapping_is_exact_on_frame_and_cap_including_non_affine_quad() {
    let shape = shape(Primitive::Rectangle);
    let mut data = shape.data().clone();
    for (corner, uv) in
        data.faces[0]
            .corners
            .iter_mut()
            .zip([[0.1, 0.1], [0.65, 0.15], [0.6, 0.75], [0.2, 0.55]])
    {
        corner.uv = uv;
    }
    let mesh = EditableMesh::new(data).unwrap();
    let delta = Vec3::Z * 0.2;
    let out = inset::apply(&mesh, &first(&mesh), 63., delta, false).unwrap();
    for triangle in &out.mesh.prepared().triangles {
        if out.new_faces.contains(&triangle.face) {
            continue;
        }
        let face = out.mesh.face(triangle.face).unwrap();
        let mut p = out.mesh.triangle_points(triangle).into_iter().sum::<Vec3>() / 3.;
        if out.selection.ids.contains(&triangle.face) {
            p -= delta;
        }
        let expected = uv_at(&mesh, p, Vec3::Z);
        let actual = triangle
            .corners
            .iter()
            .map(|&i| Vec2::from(face.corners[i].uv))
            .sum::<Vec2>()
            / 3.;
        assert!(
            expected.abs_diff_eq(actual, 2e-5),
            "{p:?}: {expected:?} != {actual:?}"
        );
    }
    let mapped = atlas::allocate(&out.mesh, &out.new_faces, [512; 2], 1)
        .unwrap()
        .mesh;
    for original in out
        .mesh
        .data()
        .faces
        .iter()
        .filter(|f| !out.new_faces.contains(&f.id))
    {
        assert_eq!(mapped.face(original.id), Some(original));
    }
}

#[test]
fn convex_ngons_inclined_cone_faces_and_mirrored_global_normal_distance_work() {
    for primitive in [Primitive::Circle, Primitive::Cone, Primitive::Pyramid] {
        let mesh = shape(primitive);
        for (fi, face) in mesh.data().faces.iter().enumerate() {
            let normal = mesh.prepared().face_normals[fi];
            let out = inset::apply(&mesh, &selected(vec![face.id]), 70., normal * 0.03, false)
                .unwrap_or_else(|e| panic!("{primitive:?} {}: {e}", face.id));
            assert!(!out.selection.ids.is_empty());
        }
    }
    let mesh = shape(Primitive::Cone);
    let face = &mesh.data().faces[0];
    let n = mesh.prepared().face_normals[0];
    let world = Mat4::from_scale_rotation_translation(
        Vec3::new(-2., 0.5, 1.4),
        Quat::from_rotation_y(0.7),
        Vec3::new(3., 2., 1.),
    );
    let delta = inset::normal_delta(n, world, 0.2, true).unwrap();
    let actual = world.transform_vector3(delta);
    let expected = world.inverse().transpose().transform_vector3(n).normalize() * 0.2;
    assert!(actual.abs_diff_eq(expected, 1e-5));
    assert!((actual.length() - 0.2).abs() < 1e-5);
    assert!(inset::apply(&mesh, &selected(vec![face.id]), 70., delta, false).is_ok());
    assert!(inset::normal_delta(n, Mat4::from_scale(Vec3::new(0., 1., 1.)), 0.2, true).is_err());
}

#[test]
fn concave_holed_and_non_coplanar_regions_refuse_inset_without_removing_full_extrusion() {
    let cube = shape(Primitive::Cube);
    let mut adjacent = None;
    for uses in &cube.prepared().incident_faces {
        if uses.len() == 2 {
            adjacent = Some(selected(
                uses.iter()
                    .map(|(fi, _)| cube.data().faces[*fi].id)
                    .collect(),
            ));
            break;
        }
    }
    let selection = adjacent.unwrap();
    let normal = selection
        .ids
        .iter()
        .map(|id| cube.prepared().face_normals[cube.prepared().faces[id]])
        .sum::<Vec3>()
        .normalize();
    assert!(inset::apply(&cube, &selection, 70., normal * 0.1, false).is_err());
    assert!(inset::apply(&cube, &selection, 100., normal * 0.1, false).is_ok());
    assert!(inset::apply(&cube, &selection, 70., normal * 0.1, true).is_ok());
    let grid = primitives::generate(
        Primitive::Plane,
        8,
        Parameters {
            plane_divisions: [3, 3],
            ..Default::default()
        },
    )
    .unwrap();
    let all: Vec<_> = grid.data().faces.iter().map(|f| f.id).collect();
    let holed = selected(
        all.iter()
            .enumerate()
            .filter(|(i, _)| *i != 4)
            .map(|(_, id)| *id)
            .collect(),
    );
    assert!(inset::apply(&grid, &holed, 70., Vec3::Y * 0.1, false).is_err());
    let concave = selected(vec![all[0], all[1], all[3]]);
    assert!(inset::apply(&grid, &concave, 70., Vec3::Y * 0.1, false).is_err());
}

#[test]
fn swept_volume_detects_small_internal_obstacle_even_when_moving_corners_miss_it() {
    let cube = shape(Primitive::Cube);
    let mut data = cube.data().clone();
    let points = [
        Vec3::new(-0.05, 0., -0.05),
        Vec3::new(0.05, 0., -0.05),
        Vec3::new(0., 0., 0.05),
    ];
    let ids: Vec<_> = points
        .iter()
        .map(|&p| data.add_vertex(p).unwrap())
        .collect();
    let obstacle = data
        .add_face(
            ids.iter()
                .enumerate()
                .map(|(i, &vertex)| Corner {
                    vertex,
                    uv: [[0., 0.], [1., 0.], [0., 1.]][i],
                    normal: None,
                })
                .collect(),
        )
        .unwrap();
    data.complete_edges().unwrap();
    let mesh = EditableMesh::new(data).unwrap();
    let fi = mesh
        .prepared()
        .face_normals
        .iter()
        .position(|n| n.y > 0.99)
        .unwrap();
    let error = inset::apply(
        &mesh,
        &selected(vec![mesh.data().faces[fi].id]),
        70.,
        Vec3::Y * -0.8,
        false,
    )
    .err()
    .unwrap();
    assert!(error.contains(&obstacle.to_string()), "{error}");
}

#[test]
fn model_pivots_collider_pixels_and_unrelated_components_survive_atomic_history_and_reopen() {
    let cube = shape(Primitive::Cube);
    let mut data = cube.data().clone();
    let isolated = data.add_vertex(Vec3::ZERO).unwrap();
    let mesh = EditableMesh::new(data).unwrap();
    let output = inset::apply(
        &mesh,
        &first(&mesh),
        70.,
        mesh.prepared().face_normals[0] * 0.2,
        false,
    )
    .unwrap();
    let mapped = atlas::allocate(&output.mesh, &output.new_faces, [512; 2], 2)
        .unwrap()
        .mesh;
    assert_eq!(mapped.position(isolated), Some(Vec3::ZERO));
    let mut pixels = PaintImage::new(512, 512, [160, 160, 160, 255]).unwrap();
    let original_uv = Vec2::from(mesh.data().faces[1].corners[0].uv);
    pixels.paint_uv(original_uv.to_array(), 2., [220, 10, 20, 255]);
    let old = pixels.clone();
    // Expansion is an explicit caller decision: old pixels retain their exact coordinates,
    // while existing normalized UVs shrink by two. Never stretch or resample the old image.
    pixels = PaintImage::new(1024, 1024, [160, 160, 160, 255]).unwrap();
    for y in 0..old.height {
        let a = (y * old.width * 4) as usize;
        let b = (y * pixels.width * 4) as usize;
        pixels.pixels[b..b + old.width as usize * 4]
            .copy_from_slice(&old.pixels[a..a + old.width as usize * 4]);
    }
    let wall = mapped.face(output.new_faces[0]).unwrap();
    let wall_uv =
        wall.corners.iter().map(|c| Vec2::from(c.uv)).sum::<Vec2>() / wall.corners.len() as f32;
    pixels.paint_uv(wall_uv.to_array(), 1., [20, 50, 240, 255]);
    assert_eq!(
        pixels.sample_uv((original_uv * 0.5).to_array()),
        old.sample_uv(original_uv.to_array())
    );
    let mut project = Project::new("Borda, tinta e articulação");
    let sid = project.start_scene.clone();
    let texture = document::new_id();
    project.assets.push(document::Asset {
        id: texture.clone(),
        name: "Atlas".into(),
        path: "assets/atlas.png".into(),
        kind: document::AssetKind::Texture,
        model: None,
    });
    let mut entity = Entity::new("Peça", None);
    entity.mesh = Some(mesh);
    entity.collider = Some(document::Collider::default());
    entity.transform.pivot = [0.2, -0.1, 0.];
    entity.material.texture = Some(texture.clone());
    let id = entity.id.clone();
    project.scenes[0].entities.push(entity);
    let matrix = project.scenes[0].world_matrix(&id).unwrap();
    let body = spatial::collider_bounds(&project.scenes[0], &id).unwrap();
    let mut images = TextureCache::default();
    images.insert(texture.clone(), old.clone());
    let mut history = CommandHistory::new();
    let before = project.clone();
    history.begin("Borda e extrusão", &project, &images);
    project.scenes[0].entity_mut(&id).unwrap().mesh = Some(mapped.clone());
    images.insert(texture.clone(), pixels.clone());
    history.commit(&project, &mut images).unwrap();
    let after = project.clone();
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);
    assert_eq!(images.get(&texture).unwrap(), &old);
    history.redo(&mut project, &mut images).unwrap();
    assert_eq!(project, after);
    assert_eq!(images.get(&texture).unwrap(), &pixels);
    assert_eq!(project.scenes[0].world_matrix(&id).unwrap(), matrix);
    assert_eq!(
        spatial::collider_bounds(&project.scenes[0], &id).unwrap(),
        body
    );
    let model = project.save_model(&sid, &id, "Modelo com borda").unwrap();
    let instance = project.instantiate_model(&model, &sid).unwrap();
    assert_eq!(
        project.scenes[0].entity(&instance).unwrap().mesh,
        Some(mapped)
    );
    let directory = std::env::temp_dir().join(format!("oxy-inset-{}", document::new_id()));
    let path = directory.join("project.oxy.json");
    persistence::save_bundle(&path, &project, &HashMap::from([(texture, pixels.clone())])).unwrap();
    assert_eq!(persistence::load_project(&path).unwrap(), project);
    assert_eq!(
        PaintImage::load(&directory.join("assets/atlas.png")).unwrap(),
        pixels
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn separate_normals_apply_per_face_and_unaffected_ids_remain_stable() {
    let mesh = shape(Primitive::Cube);
    let selection = selected(mesh.data().faces.iter().map(|f| f.id).collect());
    let deltas: HashMap<_, _> = mesh
        .data()
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id, mesh.prepared().face_normals[i] * 0.05))
        .collect();
    let out = inset::apply_directions(&mesh, &selection, 70., &deltas, true).unwrap();
    for original in &mesh.data().vertices {
        assert_eq!(
            out.mesh.position(original.id),
            Some(Vec3::from(original.position))
        );
    }
    let ids: HashSet<_> = out.mesh.data().vertices.iter().map(|v| v.id).collect();
    assert_eq!(ids.len(), out.mesh.data().vertices.len());
    assert!(
        out.mesh
            .prepared()
            .incident_faces
            .iter()
            .all(|uses| uses.len() == 2)
    );
}
