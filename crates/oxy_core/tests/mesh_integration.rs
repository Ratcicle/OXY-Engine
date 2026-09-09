use glam::{Vec2, Vec3};
use oxy_core::{
    animation::*,
    document::*,
    geometry::{
        primitives::Parameters,
        selection::{Mode, Selection},
        *,
    },
    painting::PaintImage,
    persistence, spatial,
};
use std::collections::HashMap;
fn selected(mode: Mode, ids: Vec<u32>) -> Selection {
    Selection {
        mode,
        ids,
        through: false,
    }
}
#[test]
fn tube_model_paint_instances_pivots_collision_and_rigid_animation_roundtrip() {
    let tube = primitives::generate(Primitive::Tube, 8, Parameters::default()).unwrap();
    let edge = tube
        .data()
        .edges
        .iter()
        .find(|e| {
            let [a, b] = e.vertices.map(|v| tube.position(v).unwrap());
            (a.y - b.y).abs() > 0.9 && a.x.hypot(a.z) > 0.49
        })
        .unwrap()
        .id;
    let cut = cuts::loop_cut(&tube, edge, 1, 0.).unwrap().output.mesh;
    assert_eq!(cut.data().faces.len(), 40); // outer eight quads split; hole and inner wall preserved
    let (fi, face) = cut
        .data()
        .faces
        .iter()
        .enumerate()
        .find(|(i, f)| {
            cut.prepared().face_normals[*i].z > 0.8
                && f.corners
                    .iter()
                    .all(|c| cut.position(c.vertex).unwrap().y >= -1e-5)
        })
        .unwrap();
    let extruded = operations::extrude(
        &cut,
        &selected(Mode::Face, vec![face.id]),
        cut.prepared().face_normals[fi] * 0.15,
        false,
    )
    .unwrap();
    let mapped = atlas::allocate(&extruded.mesh, &extruded.new_faces, [512; 2], 2)
        .unwrap()
        .mesh;
    let cap = mapped.face(extruded.selection.ids[0]).unwrap();
    let ends = cap
        .corners
        .iter()
        .zip(cap.corners.iter().cycle().skip(1))
        .take(cap.corners.len())
        .find(|(a, b)| {
            mapped.position(a.vertex).unwrap().y > 0.49
                && mapped.position(b.vertex).unwrap().y > 0.49
        })
        .map(|(a, b)| [a.vertex, b.vertex])
        .unwrap();
    let cap_edge = mapped
        .data()
        .edges
        .iter()
        .find(|e| pair(e.vertices) == pair(ends))
        .unwrap()
        .id;
    let bevel = bevel::apply(&mapped, &selected(Mode::Edge, vec![cap_edge]), 0.02, 2).unwrap();
    let result = atlas::allocate(&bevel.mesh, &bevel.new_faces, [1024; 2], 1)
        .unwrap()
        .mesh;
    assert!(
        result
            .prepared()
            .incident_faces
            .iter()
            .all(|f| f.len() == 2)
    );
    assert_eq!(
        result.data().vertices.len() as isize - result.data().edges.len() as isize
            + result.data().faces.len() as isize,
        0
    ); // genus one, not a capped cylinder
    let mut pixels = PaintImage::new(1024, 1024, [230, 230, 230, 255]).unwrap();
    let old = &result
        .data()
        .faces
        .iter()
        .find(|f| tube.face(f.id).is_some() && !bevel.new_faces.contains(&f.id))
        .unwrap();
    let center = |f: &Face| {
        f.corners.iter().map(|c| Vec2::from(c.uv)).sum::<Vec2>() / f.corners.len() as f32
    };
    let old_uv = center(old);
    pixels.paint_uv(old_uv.to_array(), 2., [200, 30, 20, 255]);
    let before = pixels.clone();
    let new = result.face(bevel.new_faces[0]).unwrap();
    let new_uv = center(new);
    assert_eq!(uv::pick(&result, new_uv, &[]), Some(new.id));
    pixels.paint_uv(new_uv.to_array(), 1.5, [20, 80, 220, 255]);
    assert_eq!(
        pixels.sample_uv(old_uv.to_array()),
        before.sample_uv(old_uv.to_array())
    );
    assert_eq!(pixels.sample_uv(new_uv.to_array()), [20, 80, 220, 255]);
    let mut p = Project::new("Integração tubo");
    let sid = p.start_scene.clone();
    p.scenes[0].kind = SceneKind::ThreeD;
    let texture = new_id();
    p.assets.push(Asset {
        id: texture.clone(),
        name: "Atlas próprio".into(),
        path: "assets/atlas.png".into(),
        kind: AssetKind::Texture,
        model: None,
    });
    let mut root = Entity::new("Articulação", None);
    root.transform.scale = [-1., 1.5, 1.];
    root.transform.rotation = [0., 0.2, 0.];
    root.collider = Some(Collider::default());
    let rid = root.id.clone();
    let mut piece = Entity::new("Tubo editado", None);
    piece.parent = Some(rid.clone());
    piece.mesh = Some(result.clone());
    piece.material.texture = Some(texture.clone());
    let pid = piece.id.clone();
    p.scenes[0].entities.extend([root, piece]);
    let old_box = spatial::collider_bounds(&p.scenes[0], &rid).unwrap();
    let bounds = spatial::visual_bounds(&p.scenes[0], std::slice::from_ref(&pid)).unwrap();
    assert_ne!(old_box, bounds);
    spatial::set_collider_bounds(&mut p.scenes[0], &rid, bounds).unwrap();
    let base = p.scenes[0].world_matrix(&pid).unwrap();
    spatial::move_pivot(&mut p.scenes[0], &rid, Vec3::new(0., 0.5, 0.)).unwrap();
    assert!(base.abs_diff_eq(p.scenes[0].world_matrix(&pid).unwrap(), 1e-5));
    assert!(bounds.min.abs_diff_eq(
        spatial::collider_bounds(&p.scenes[0], &rid).unwrap().min,
        1e-5
    ));
    let t = p.scenes[0].entity(&rid).unwrap().transform.clone();
    let mut t1 = t.clone();
    t1.rotation[2] = 0.6;
    let mut clip = Clip::new("Giro rígido");
    clip.tracks.push(Track {
        target: rid.clone(),
        keyframes: vec![
            Keyframe {
                time: 0.,
                transform: t,
                interpolation: Interpolation::Linear,
            },
            Keyframe {
                time: 1.,
                transform: t1,
                interpolation: Interpolation::Linear,
            },
        ],
    });
    p.scenes[0].entity_mut(&rid).unwrap().clips.push(clip);
    let model = p.save_model(&sid, &rid, "Tubo articulado").unwrap();
    let instance = p.instantiate_model(&model, &sid).unwrap();
    let iid = p.scenes[0]
        .entities
        .iter()
        .find(|e| e.parent.as_deref() == Some(&instance))
        .unwrap()
        .id
        .clone();
    assert!(
        p.scenes[0]
            .entity(&iid)
            .unwrap()
            .mesh
            .as_ref()
            .unwrap()
            .shares_storage(&result)
    );
    p.scenes[0].entity_mut(&iid).unwrap().mesh =
        Some(result.with_shading(Shading::Smooth).unwrap());
    assert_eq!(
        p.scenes[0].entity(&pid).unwrap().mesh.as_ref().unwrap(),
        &result
    );
    let root = p.scenes[0].entity(&instance).unwrap();
    assert_eq!(root.clips[0].tracks[0].target, instance);
    let mut preview = p.scenes[0].clone();
    sample_clip(&mut preview, &root.clips[0], 0.5);
    assert!(
        !preview
            .world_matrix(&iid)
            .unwrap()
            .abs_diff_eq(p.scenes[0].world_matrix(&iid).unwrap(), 0.01)
    );
    assert_eq!(
        preview.entity(&iid).unwrap().mesh,
        p.scenes[0].entity(&iid).unwrap().mesh
    );
    assert!(spatial::move_pivot(&mut p.scenes[0], &rid, Vec3::ZERO).is_err());
    let dir = std::env::temp_dir().join(format!("oxy-mesh-integration-{}", new_id()));
    let path = dir.join("project.oxy.json");
    let images = HashMap::from([(texture, pixels.clone())]);
    persistence::save_bundle(&path, &p, &images).unwrap();
    let reopened = persistence::load_project(&path).unwrap();
    assert_eq!(reopened, p);
    assert_eq!(
        PaintImage::load(&dir.join("assets/atlas.png")).unwrap(),
        pixels
    );
    let runtime = oxy_core::runtime::Runtime::new(&reopened, &sid).unwrap();
    assert_eq!(runtime.scene(), &reopened.scenes[0]);
    std::fs::remove_dir_all(dir).unwrap();
}
