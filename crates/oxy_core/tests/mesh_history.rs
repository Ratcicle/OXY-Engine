use oxy_core::{
    document::*,
    edit_history::CommandHistory,
    geometry::{Shading, primitives},
    texture_cache::TextureCache,
};
#[test]
fn affected_mesh_history_restores_structures_without_serializing_unrelated_geometry() {
    let mut p = Project::new("Histórico de malha");
    let sid = p.start_scene.clone();
    let mut e = Entity::new("Peça", Some(Primitive::Cube));
    primitives::convert(&mut e).unwrap();
    let id = e.id.clone();
    let mesh = e.mesh.as_ref().unwrap().clone();
    p.scenes[0].entities.push(e);
    let model = p.save_model(&sid, &id, "Modelo").unwrap();
    p.instantiate_model(&model, &sid).unwrap();
    let mut h = CommandHistory::new();
    let mut images = TextureCache::default();
    let mut versions = vec![p.clone()];
    h.begin("Atributo", &p, &images);
    p.scenes[0].entity_mut(&id).unwrap().name = "Renomeado".into();
    h.commit(&p, &mut images).unwrap();
    assert!(h.estimated_bytes() < 4096);
    versions.push(p.clone());
    h.begin("Geometria", &p, &images);
    p.scenes[0].entity_mut(&id).unwrap().mesh = Some(mesh.with_shading(Shading::Smooth).unwrap());
    h.commit(&p, &mut images).unwrap();
    versions.push(p.clone());
    h.begin("Reordenar", &p, &images);
    p.scenes[0].entities.reverse();
    h.commit(&p, &mut images).unwrap();
    versions.push(p.clone());
    h.begin("Excluir e criar com mesma contagem", &p, &images);
    p.scenes[0].entities.retain(|e| e.id != id);
    let mut e = Entity::new("Nova", None);
    e.mesh = Some(mesh.clone());
    p.scenes[0].entities.push(e);
    h.commit(&p, &mut images).unwrap();
    versions.push(p.clone());
    h.begin("Modelo", &p, &images);
    p.assets[0].model.as_mut().unwrap()[0].mesh = Some(mesh.with_shading(Shading::Smooth).unwrap());
    h.commit(&p, &mut images).unwrap();
    versions.push(p.clone());
    for expected in versions[..versions.len() - 1].iter().rev() {
        h.undo(&mut p, &mut images).unwrap();
        assert_eq!(&p, expected);
        validate_project(&p).unwrap();
    }
    for expected in &versions[1..] {
        h.redo(&mut p, &mut images).unwrap();
        assert_eq!(&p, expected);
        validate_project(&p).unwrap();
    }
    assert!(
        p.scenes[0]
            .entities
            .iter()
            .any(|e| e.mesh.as_ref().is_some_and(|m| m.shares_storage(&mesh)))
    );
    h.set_memory_budget(1);
    assert_eq!(h.undo_len(), 1); // keep latest reversible operation, then trim older ones
}
