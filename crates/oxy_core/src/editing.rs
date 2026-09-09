//! Reversible editing rules, independent of widgets and runtime input.
use crate::document::*;
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};

/// Creates only the requested first scene, without demonstration gameplay.
pub fn blank_project(name: &str, kind: SceneKind) -> Result<Project, String> {
    if name.trim().is_empty() {
        return Err("Informe um nome para o projeto.".into());
    }
    let mut project = Project::new(name.trim());
    let scene = Scene::new(
        if kind == SceneKind::TwoD {
            "Cena 2D"
        } else {
            "Cena 3D"
        },
        kind,
    );
    project.start_scene = scene.id.clone();
    project.scenes = vec![scene];
    project.input_bindings.clear();
    Ok(project)
}

pub fn duplicate_scene(project: &mut Project, id: &str) -> Result<Id, String> {
    let mut scene = project.scene(id).ok_or("Cena não encontrada")?.clone();
    let new_scene = new_id();
    let mut map: HashMap<_, _> = scene
        .entities
        .iter()
        .map(|e| (e.id.clone(), new_id()))
        .collect();
    map.insert(scene.id.clone(), new_scene.clone());
    remap_entities(&mut scene.entities, &map)?;
    scene.id = new_scene.clone();
    scene.name.push_str(" (cópia)");
    project.scenes.push(scene);
    Ok(new_scene)
}

pub fn scene_references(project: &Project, id: &str) -> Vec<String> {
    project
        .scenes
        .iter()
        .map(|s| (s.name.as_str(), s.entities.as_slice()))
        .chain(
            project
                .assets
                .iter()
                .filter_map(|a| a.model.as_deref().map(|m| (a.name.as_str(), m))),
        )
        .flat_map(|(scope, entities)| {
            entities.iter().flat_map(move |e| {
                e.graph.nodes.iter().filter(move |n| {
                    n.operation == "action.scene"
                        && matches!(n.params.get("scene"), Some(Value::Text(target)) if target==id)
                }).map(move |n| format!("{scope} → {} → Mudar de cena ({})", e.name, n.id))
            })
        })
        .collect()
}

pub fn delete_scene(project: &mut Project, id: &str) -> Result<(), String> {
    if project.scene(id).is_none() {
        return Err("Cena não encontrada".into());
    }
    if project.scenes.len() == 1 {
        return Err("Mantenha pelo menos uma cena no projeto.".into());
    }
    let references = scene_references(project, id);
    if !references.is_empty() {
        return Err(format!(
            "Altere os destinos destes nós antes de excluir:\n{}",
            references.join("\n")
        ));
    }
    if project.start_scene == id {
        return Err("Escolha outra cena inicial do jogo antes de excluir esta cena.".into());
    }
    project.scenes.retain(|s| s.id != id);
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub ids: Vec<Id>,
    pub anchor: Option<Id>,
}
impl Selection {
    pub fn single(&mut self, id: Option<Id>) {
        self.ids = id.iter().cloned().collect();
        self.anchor = id;
    }
    pub fn click(&mut self, id: Id, toggle: bool, range: bool, ordered: &[Id]) {
        if range
            && let Some(a) = self
                .anchor
                .as_ref()
                .and_then(|id| ordered.iter().position(|x| x == id))
            && let Some(b) = ordered.iter().position(|x| x == &id)
        {
            if !toggle {
                self.ids.clear();
            }
            for item in &ordered[a.min(b)..=a.max(b)] {
                if !self.ids.contains(item) {
                    self.ids.push(item.clone());
                }
            }
        } else if toggle {
            if self.ids.contains(&id) {
                self.ids.retain(|x| x != &id);
            } else {
                self.ids.push(id.clone());
            }
            self.anchor = Some(id);
        } else {
            self.single(Some(id));
        }
    }
    pub fn retain_scene(&mut self, scene: &Scene) {
        self.ids.retain(|id| scene.entity(id).is_some());
    }
}

pub fn hierarchy_order(scene: &Scene) -> Vec<Id> {
    let Ok(index) = crate::scene_view::SceneIndex::new(scene) else {
        return Vec::new();
    };
    let mut stack: Vec<_> = index.roots.iter().rev().copied().collect();
    let mut out = Vec::with_capacity(scene.entities.len());
    let mut visited = HashSet::new();
    while let Some(i) = stack.pop() {
        if !visited.insert(i) {
            continue;
        }
        out.push(scene.entities[i].id.clone());
        stack.extend(index.children[i].iter().rev().copied());
    }
    out
}
/// Selected descendants inherit their selected ancestor's operation exactly once.
pub fn selection_roots(scene: &Scene, ids: &[Id]) -> Vec<Id> {
    let set: HashSet<_> = ids.iter().collect();
    ids.iter()
        .filter(|id| {
            let Some(entity) = scene.entity(id) else {
                return false;
            };
            let mut parent = entity.parent.as_deref();
            for _ in 0..=scene.entities.len() {
                let Some(id) = parent else {
                    return true;
                };
                if set.iter().any(|selected| selected.as_str() == id) {
                    return false;
                }
                parent = scene.entity(id).and_then(|e| e.parent.as_deref());
            }
            false
        })
        .cloned()
        .collect()
}

pub fn reparent_selection(scene: &mut Scene, ids: &[Id], parent: Option<Id>) -> Result<(), String> {
    let roots = selection_roots(scene, ids);
    if roots.is_empty() {
        return Err("Selecione os objetos que deseja mover.".into());
    }
    let mut next = scene.clone();
    for root in roots {
        next.reparent(&root, parent.clone(), true)?;
    }
    *scene = next;
    Ok(())
}

pub fn rename_entity(scene: &mut Scene, id: &str, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("O nome não pode ficar vazio.".into());
    }
    scene.entity_mut(id).ok_or("Objeto não encontrado")?.name = name.into();
    Ok(())
}

pub fn selection_center(scene: &Scene, ids: &[Id]) -> Vec3 {
    let roots = selection_roots(scene, ids);
    let points: Vec<_> = roots
        .iter()
        .filter_map(|id| {
            scene.entity(id).and_then(|e| {
                scene
                    .world_matrix(id)
                    .ok()
                    .map(|m| m.transform_point3(Vec3::from(e.transform.pivot)))
            })
        })
        .collect();
    if points.is_empty() {
        Vec3::ZERO
    } else {
        points.iter().copied().sum::<Vec3>() / points.len() as f32
    }
}

pub fn group_selection(scene: &mut Scene, ids: &[Id]) -> Result<Id, String> {
    let roots = selection_roots(scene, ids);
    if roots.is_empty() {
        return Err("Selecione ao menos um objeto para agrupar.".into());
    }
    let mut next = scene.clone();
    let mut group = Entity::new("Grupo", None);
    group.transform.position = selection_center(scene, &roots).to_array();
    let id = group.id.clone();
    next.entities.push(group);
    reparent_selection(&mut next, &roots, Some(id.clone()))?;
    *scene = next;
    Ok(id)
}

pub fn duplicate_selection(scene: &mut Scene, ids: &[Id]) -> Result<Vec<Id>, String> {
    let roots = selection_roots(scene, ids);
    let all: HashSet<_> = roots.iter().flat_map(|id| scene.descendants(id)).collect();
    let mut copies: Vec<_> = scene
        .entities
        .iter()
        .filter(|e| all.contains(&e.id))
        .cloned()
        .collect();
    let map: HashMap<_, _> = copies.iter().map(|e| (e.id.clone(), new_id())).collect();
    remap_entities(&mut copies, &map)?;
    let new_roots: Vec<_> = roots.iter().map(|id| map[id].clone()).collect();
    for root in &new_roots {
        for entity in &mut copies {
            if entity.id == *root {
                entity.name.push_str(" (cópia)");
                entity.transform.position[0] += 0.5;
            }
            for clip in &mut entity.clips {
                for track in &mut clip.tracks {
                    if track.target == *root {
                        for key in &mut track.keyframes {
                            key.transform.position[0] += 0.5;
                        }
                    }
                }
            }
        }
    }
    scene.entities.extend(copies);
    Ok(new_roots)
}

/// Apply a world-space transform to selection roots as one atomic operation.
pub fn transform_selection(scene: &mut Scene, ids: &[Id], delta: Mat4) -> Result<(), String> {
    let mut transforms = Vec::new();
    for id in selection_roots(scene, ids) {
        let entity = scene.entity(&id).ok_or("Objeto ausente")?;
        let parent = entity
            .parent
            .as_deref()
            .map(|id| scene.world_matrix(id))
            .transpose()?
            .unwrap_or(Mat4::IDENTITY);
        if parent.determinant().abs() < 1e-8 {
            return Err("A escala do pai não permite esta transformação.".into());
        }
        let local = parent.inverse() * delta * scene.world_matrix(&id)?;
        let transform = Transform::from_matrix(local, entity.transform.pivot);
        if !transform.finite() || !transform.matrix().abs_diff_eq(local, 0.0002) {
            return Err("A operação exigiria deformação por cisalhamento. Use escala uniforme ou transforme as peças separadamente.".into());
        }
        transforms.push((id, transform));
    }
    for (id, transform) in transforms {
        scene.entity_mut(&id).unwrap().transform = transform;
    }
    Ok(())
}

pub fn asset_references(project: &Project, asset: &str) -> Vec<String> {
    let mut found = Vec::new();
    let scopes = project
        .scenes
        .iter()
        .map(|s| (s.name.as_str(), s.entities.as_slice()))
        .chain(
            project
                .assets
                .iter()
                .filter_map(|a| a.model.as_deref().map(|m| (a.name.as_str(), m))),
        );
    for (scope, entities) in scopes {
        for entity in entities {
            if entity.material.texture.as_deref() == Some(asset) {
                found.push(format!("{scope} → {}: material", entity.name));
            }
            if entity.ui.as_ref().and_then(|u| u.texture.as_deref()) == Some(asset) {
                found.push(format!("{scope} → {}: imagem da interface", entity.name));
            }
            if entity.model_source.as_deref() == Some(asset) {
                found.push(format!("{scope} → {}: modelo", entity.name));
            }
            for node in &entity.graph.nodes {
                if node
                    .params
                    .values()
                    .any(|v| matches!(v, Value::Text(id) if id == asset))
                {
                    let definitions = crate::graph::registry();
                    let label = definitions
                        .iter()
                        .find(|d| d.id == node.operation)
                        .map_or("não reconhecida", |d| d.label);
                    found.push(format!("{scope} → {}: ação {label}", entity.name));
                }
            }
        }
    }
    found
}

pub fn remove_asset(project: &mut Project, asset: &str) -> Result<(), String> {
    let uses = asset_references(project, asset);
    if !uses.is_empty() {
        return Err(format!("O recurso ainda está em uso:\n{}", uses.join("\n")));
    }
    if project.asset(asset).is_none() {
        return Err("Recurso não encontrado.".into());
    }
    project.assets.retain(|a| a.id != asset);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> (Scene, Id, Id, Id) {
        let mut s = Scene::new("Teste", SceneKind::ThreeD);
        let mut parent = Entity::new("Personagem", None);
        parent.transform.position = [3., 2., 0.];
        let mut arm = Entity::new("Braço", Some(Primitive::Cube));
        arm.transform.position = [5., 4., 0.];
        let head = Entity::new("Cabeça", Some(Primitive::Sphere));
        let ids = (parent.id.clone(), arm.id.clone(), head.id.clone());
        s.entities.extend([parent, arm, head]);
        (s, ids.0, ids.1, ids.2)
    }
    #[test]
    fn drop_reparent_preserves_world_and_rejects_cycles_atomically() {
        let (mut s, p, a, h) = scene();
        let before = s.world_matrix(&a).unwrap();
        reparent_selection(&mut s, &[a.clone(), h.clone()], Some(p.clone())).unwrap();
        assert!(s.world_matrix(&a).unwrap().abs_diff_eq(before, 1e-5));
        let snapshot = s.clone();
        assert!(reparent_selection(&mut s, &[p], Some(a.clone())).is_err());
        assert_eq!(s, snapshot);
        reparent_selection(&mut s, std::slice::from_ref(&a), None).unwrap();
        assert!(s.entity(&a).unwrap().parent.is_none());
    }
    #[test]
    fn multi_select_toggle_range_and_clear() {
        let mut s = Selection::default();
        let ids: Vec<Id> = vec!["a".into(), "b".into(), "c".into()];
        s.click(ids[0].clone(), false, false, &ids);
        s.click(ids[2].clone(), false, true, &ids);
        assert_eq!(s.ids, ids);
        s.click("b".into(), true, false, &ids);
        assert_eq!(s.ids, vec!["a", "c"]);
        s.single(None);
        assert!(s.ids.is_empty());
    }
    #[test]
    fn group_duplicate_and_transform_do_not_transform_children_twice() {
        let (mut s, _, a, h) = scene();
        let before = s.world_matrix(&a).unwrap();
        let g = group_selection(&mut s, &[a.clone(), h.clone()]).unwrap();
        assert!(s.world_matrix(&a).unwrap().abs_diff_eq(before, 1e-5));
        transform_selection(
            &mut s,
            &[g.clone(), a.clone()],
            Mat4::from_translation(Vec3::X),
        )
        .unwrap();
        assert!(
            s.world_matrix(&a)
                .unwrap()
                .abs_diff_eq(Mat4::from_translation(Vec3::X) * before, 1e-5)
        );
        let copies = duplicate_selection(&mut s, &[g, a]).unwrap();
        assert_eq!(copies.len(), 1);
        assert_eq!(s.descendants(&copies[0]).len(), 3);
    }
    #[test]
    fn rename_keeps_id_and_texture_unlink_keeps_asset() {
        let mut p = Project::new("Teste");
        let scene = p.start_scene.clone();
        let e = Entity::new("Peça", Some(Primitive::Rectangle));
        let id = e.id.clone();
        p.scene_mut(&scene).unwrap().entities.push(e);
        rename_entity(p.scene_mut(&scene).unwrap(), &id, "Personagem").unwrap();
        assert_eq!(p.scene(&scene).unwrap().entity(&id).unwrap().id, id);
        let asset = Asset {
            id: new_id(),
            name: "Textura".into(),
            kind: AssetKind::Texture,
            path: "assets/paint.png".into(),
            model: None,
        };
        let texture = asset.id.clone();
        p.assets.push(asset);
        p.scene_mut(&scene)
            .unwrap()
            .entity_mut(&id)
            .unwrap()
            .material
            .texture = Some(texture.clone());
        assert!(remove_asset(&mut p, &texture).is_err());
        p.scene_mut(&scene)
            .unwrap()
            .entity_mut(&id)
            .unwrap()
            .material
            .texture = None;
        assert!(p.asset(&texture).is_some());
        remove_asset(&mut p, &texture).unwrap();
    }
}
