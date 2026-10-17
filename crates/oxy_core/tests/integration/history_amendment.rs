use oxy_core::{
    document::*, edit_history::CommandHistory, painting::PaintImage, texture_cache::TextureCache,
};

fn fixture() -> (Project, TextureCache, CommandHistory, Id) {
    let mut project = Project::new("Ajuste");
    let entity = Entity::new("Peça", Some(Primitive::Cube));
    let id = entity.id.clone();
    project.scenes[0].entities.push(entity);
    (project, TextureCache::default(), CommandHistory::new(), id)
}
#[test]
fn adjusting_replaces_one_delta_and_cancel_restores_last_applied_state() {
    let (mut p, mut images, mut h, id) = fixture();
    let original = p.clone();
    h.begin("Mover", &p, &images);
    p.scenes[0].entity_mut(&id).unwrap().transform.position[0] = 1.;
    h.commit(&p, &mut images).unwrap();
    let applied = p.clone();
    let token = h.last_command_id().unwrap();
    assert!(h.begin_amend_last(token, &p, &images).unwrap());
    p.scenes[0].entity_mut(&id).unwrap().transform.position[0] = 2.;
    assert!(h.cancel(&mut p, &mut images));
    assert_eq!(p, applied);
    assert_eq!(h.undo_len(), 1);
    assert!(h.begin_amend_last(token, &p, &images).unwrap());
    p.scenes[0].entity_mut(&id).unwrap().transform.position[0] = 3.;
    h.commit(&p, &mut images).unwrap();
    let adjusted = p.clone();
    assert_eq!(h.undo_len(), 1);
    assert!(h.estimated_bytes() < 4096);
    assert!(!h.begin_amend_last(token, &p, &images).unwrap());
    h.undo(&mut p, &mut images).unwrap();
    assert_eq!(p, original);
    h.redo(&mut p, &mut images).unwrap();
    assert_eq!(p, adjusted);
    let token = h.last_command_id().unwrap();
    h.begin("Nome", &p, &images);
    p.scenes[0].entity_mut(&id).unwrap().name = "Outro".into();
    h.commit(&p, &mut images).unwrap();
    assert!(!h.begin_amend_last(token, &p, &images).unwrap());
}
#[test]
fn zero_effect_amendment_drops_conversion_and_restores_saved_revision() {
    let (mut p, mut images, mut h, id) = fixture();
    let original = p.clone();
    h.begin("Converter e mover", &p, &images);
    oxy_core::geometry::primitives::convert(p.scenes[0].entity_mut(&id).unwrap()).unwrap();
    h.commit(&p, &mut images).unwrap();
    assert!(
        h.begin_amend_last(h.last_command_id().unwrap(), &p, &images)
            .unwrap()
    );
    p = original;
    assert!(!h.commit(&p, &mut images).unwrap());
    assert_eq!(h.undo_len(), 0);
    assert!(!h.is_dirty());
}
#[test]
fn amended_pixels_and_asset_creation_have_one_atomic_undo() {
    let (mut p, mut images, mut h, id) = fixture();
    let original = p.clone();
    let texture = new_id();
    h.begin("Atlas", &p, &images);
    p.assets.push(Asset {
        id: texture.clone(),
        name: "Pintura".into(),
        path: format!("assets/{texture}.png"),
        kind: AssetKind::Texture,
        model: None,
    });
    p.scenes[0].entity_mut(&id).unwrap().material.texture = Some(texture.clone());
    images.insert(
        texture.clone(),
        PaintImage::new(256, 256, [255, 0, 0, 255]).unwrap(),
    );
    h.commit(&p, &mut images).unwrap();
    assert!(
        h.begin_amend_last(h.last_command_id().unwrap(), &p, &images)
            .unwrap()
    );
    images
        .get_mut(&texture)
        .unwrap()
        .set_pixel(1, 1, [0, 255, 0, 255]);
    h.commit(&p, &mut images).unwrap();
    assert_eq!(h.undo_len(), 1);
    h.undo(&mut p, &mut images).unwrap();
    assert_eq!(p, original);
    assert!(images.get(&texture).is_none());
    h.redo(&mut p, &mut images).unwrap();
    assert_eq!(images.get(&texture).unwrap().pixel(1, 1), [0, 255, 0, 255]);
    assert!(h.estimated_bytes() < 400_000); // one image, never two complete project/image snapshots
}
