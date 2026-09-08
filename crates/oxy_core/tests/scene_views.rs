use oxy_core::{
    document::*,
    edit_history::CommandHistory,
    scene_view::{SceneIndex, SceneView},
    texture_cache::TextureCache,
};
fn fixture() -> Project {
    let mut p = Project::new("Views");
    for i in 0..80 {
        let mut e = Entity::new(i.to_string(), Some(Primitive::Cube));
        e.id = format!("piece-{i}");
        if i > 0 {
            e.parent = Some(format!("piece-{}", (i - 1) / 2));
        }
        e.transform.position = [0.1, 0.2, 0.];
        e.transform.pivot = [0.2, -0.1, 0.];
        e.transform.scale = [-1., 1.01, 1.];
        e.transform.rotation[2] = 0.15;
        e.collider = Some(Collider::default());
        p.scenes[0].entities.push(e);
    }
    p
}
fn compare(s: &Scene) {
    let view = SceneView::new(s);
    for e in &s.entities {
        assert_eq!(view.entity(&e.id), s.entity(&e.id));
        assert_eq!(view.descendants(&e.id), s.descendants(&e.id));
        assert!(
            view.world_matrix(&e.id)
                .unwrap()
                .abs_diff_eq(s.world_matrix(&e.id).unwrap(), 1e-5)
        );
        let a = view.collider_bounds(&e.id).unwrap();
        let b = oxy_core::spatial::collider_bounds(s, &e.id).unwrap();
        assert!(a.min.abs_diff_eq(b.min, 1e-5) && a.max.abs_diff_eq(b.max, 1e-5));
    }
}
#[test]
fn phase_indices_survive_structure_history_and_equal_count_edits() {
    for mutation in 0..7 {
        let mut p = fixture();
        let before = p.clone();
        let mut h = CommandHistory::new();
        let mut images = TextureCache::default();
        compare(&p.scenes[0]);
        h.begin("structural", &p, &images);
        let s = &mut p.scenes[0];
        match mutation {
            0 => s.entities.reverse(),
            1 => {
                s.remove_subtree("piece-1");
            }
            2 => {
                s.entities.remove(79);
                s.entities
                    .push(Entity::new("replacement", Some(Primitive::Cube)));
                s.entities.last_mut().unwrap().collider = Some(Collider::default());
            }
            3 => s.entities.swap(0, 40),
            4 => {
                s.duplicate_subtree("piece-3").unwrap();
            }
            5 => {
                s.reparent("piece-39", Some("piece-2".into()), false)
                    .unwrap();
            }
            _ => s.entity_mut("piece-0").unwrap().transform.position[0] += 3.,
        }
        compare(s);
        h.commit(&p, &mut images).unwrap();
        let edited = p.clone();
        h.undo(&mut p, &mut images).unwrap();
        assert_eq!(p, before);
        compare(&p.scenes[0]);
        h.redo(&mut p, &mut images).unwrap();
        assert_eq!(p, edited);
        compare(&p.scenes[0]);
        let loaded: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        compare(&loaded.scenes[0]);
    }
}
#[test]
fn duplicate_missing_cycles_and_siblings_are_not_silently_resolved() {
    let mut p = fixture();
    let s = &mut p.scenes[0];
    let index = SceneIndex::new(s).unwrap();
    assert!(index.position("missing").is_none());
    assert!(!index.related(s, "piece-1", "piece-2"));
    assert!(index.related(s, "piece-0", "piece-2"));
    s.entities[0].parent = Some("piece-39".into());
    assert!(SceneView::new(s).world_matrix("piece-0").is_err());
    s.entities[0].parent = Some("missing".into());
    assert!(SceneView::new(s).world_matrix("piece-0").is_err());
    s.entities[1].id = "piece-0".into();
    assert!(SceneIndex::new(s).is_err());
    assert!(SceneView::new(s).entity("piece-0").is_none());
}

#[test]
fn deep_chain_matches_independent_ordered_matrix_product() {
    let mut p = Project::new("Profunda");
    let mut expected = glam::Mat4::IDENTITY;
    for i in 0..256 {
        let mut e = Entity::new("Articulação", None);
        e.id = format!("joint-{i}");
        e.parent = (i > 0).then(|| format!("joint-{}", i - 1));
        e.transform.position = [0.01, 0.02, 0.];
        e.transform.rotation[2] = 0.005;
        e.transform.pivot = [0.2, 0., 0.];
        expected *= e.transform.matrix();
        p.scenes[0].entities.push(e);
    }
    let view = SceneView::new(&p.scenes[0]);
    assert!(
        view.world_matrix("joint-255")
            .unwrap()
            .abs_diff_eq(expected, 1e-5)
    );
    assert_eq!(view.descendants("joint-128").len(), 128);
}

#[test]
#[cfg(feature = "profiling")]
fn shared_phase_calculates_each_matrix_once_without_linear_id_scans() {
    let p = fixture();
    oxy_core::metrics::take();
    let view = SceneView::new(&p.scenes[0]);
    for _ in 0..3 {
        for entity in &p.scenes[0].entities {
            view.world_matrix(&entity.id).unwrap();
        }
    }
    let work = oxy_core::metrics::take();
    assert_eq!(work.matrices, p.scenes[0].entities.len() as u64);
    assert_eq!(work.index_builds, 1);
    assert_eq!(work.entity_scans, 0);
}
