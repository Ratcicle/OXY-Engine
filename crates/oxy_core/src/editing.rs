//! Reversible editing rules, independent of widgets and runtime input.
use crate::{
    document::*,
    graph::{self, Reference},
};
use glam::{EulerRot, Mat4, Quat, Vec3};
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

/// The logic nodes of the scenes and models that name the scene `id`, as "Scope → Object →
/// node label".
pub fn scene_references(project: &Project, id: &str) -> Vec<String> {
    crate::input_actions::every_entity(project)
        .flat_map(|(scope, e)| {
            e.graph
                .nodes
                .iter()
                .filter(move |n| {
                    graph::node_references(n).any(|(_, reference, value)| {
                        reference == Reference::Scene
                            && matches!(value, Value::Text(target) if target == id)
                    })
                })
                .filter_map(move |n| {
                    let label = graph::definition(&n.operation)?.label;
                    Some(format!("{scope} → {} → {label}", e.name))
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

/// Where the editor's Duplicate puts a copy: half a metre along X from the original.
pub const DUPLICATE_OFFSET: [f32; 3] = [0.5, 0., 0.];
/// Copies made by one repeated duplicate at most.
pub const MAX_DUPLICATES: usize = 100;

pub fn duplicate_selection(scene: &mut Scene, ids: &[Id]) -> Result<Vec<Id>, String> {
    duplicate_repeated(scene, ids, DUPLICATE_OFFSET, 1)
}

/// Duplicates the selection `count` times, copy k (from 1) moved by k × `offset` from the
/// original in its parent's space, keyframes that animate it included. Answers the new roots,
/// copy after copy: with two roots, the first copy of each, then the second of each…
pub fn duplicate_repeated(
    scene: &mut Scene,
    ids: &[Id],
    offset: [f32; 3],
    count: usize,
) -> Result<Vec<Id>, String> {
    if !offset.iter().all(|v| v.is_finite()) {
        return Err("O deslocamento das cópias precisa de números finitos.".into());
    }
    if !(1..=MAX_DUPLICATES).contains(&count) {
        return Err(format!(
            "O número de cópias deve estar entre 1 e {MAX_DUPLICATES}."
        ));
    }
    let roots = selection_roots(scene, ids);
    let all: HashSet<_> = roots.iter().flat_map(|id| scene.descendants(id)).collect();
    let originals: Vec<_> = scene
        .entities
        .iter()
        .filter(|e| all.contains(&e.id))
        .cloned()
        .collect();
    let mut created = Vec::with_capacity(roots.len() * count);
    for copy in 1..=count {
        let mut copies = originals.clone();
        let map: HashMap<_, _> = copies.iter().map(|e| (e.id.clone(), new_id())).collect();
        remap_entities(&mut copies, &map)?;
        let delta = Vec3::from(offset) * copy as f32;
        for root in roots.iter().map(|id| &map[id]) {
            if let Some(entity) = copies.iter_mut().find(|e| e.id == *root) {
                if count == 1 {
                    entity.name.push_str(" (cópia)");
                } else {
                    entity.name.push_str(&format!(" (cópia {copy})"));
                }
            }
            translate_entity_and_tracks(&mut copies, root, delta.to_array());
            created.push(root.clone());
        }
        scene.entities.extend(copies);
    }
    Ok(created)
}

/// Whether the object's own animations move it, as the moving platform's does. Moving such an
/// object carries its keyframes along (see [`follow_tracks`]), or the game would put it back on
/// the path it was animated on.
pub fn animates_itself(entity: &Entity) -> bool {
    entity
        .clips
        .iter()
        .any(|clip| clip.tracks.iter().any(|track| track.target == entity.id))
}

/// Keyframes are local transforms, like the object's own. When its rest transform goes from
/// `before` to `after`, every track that animates `target` (in any clip of `entities`) moves
/// the same way, so the animation plays where the object now is: positions turn with the
/// change of rotation around the old position and follow the new one, rotations turn with it
/// (read near their own angles, see [`angles_near`]) and scales grow by the same factor.
/// Rotations and scales stay as they were when those of the object did not change, and a
/// keyframe at the old rest pose lands exactly on the new. An edit keeps the path's length;
/// a copy under another parent changes space with [`rebase_tracks`] instead.
pub fn carry_tracks(entities: &mut [Entity], target: &str, before: &Transform, after: &Transform) {
    let turn = (before.rotation != after.rotation)
        .then(|| (after.quaternion() * before.quaternion().inverse()).normalize());
    let change = std::array::from_fn(|i| after.rotation[i] - before.rotation[i]);
    let resized = (before.scale != after.scale)
        .then(|| Vec3::from(after.scale) / Vec3::from(before.scale))
        .filter(|factor| factor.is_finite());
    for entity in entities {
        for clip in &mut entity.clips {
            for track in clip.tracks.iter_mut().filter(|t| t.target == target) {
                for key in &mut track.keyframes {
                    let key = &mut key.transform;
                    let offset = Vec3::from(key.position) - Vec3::from(before.position);
                    let offset = turn.map_or(offset, |turn| turn * offset);
                    key.position = (Vec3::from(after.position) + offset).to_array();
                    if let Some(turn) = turn {
                        key.rotation = if key.rotation == before.rotation {
                            after.rotation
                        } else {
                            angles_near(turn * key.quaternion(), offset_angles(key, change))
                        };
                    }
                    if let Some(factor) = resized {
                        key.scale = if key.scale == before.scale {
                            after.scale
                        } else {
                            (Vec3::from(key.scale) * factor).to_array()
                        };
                    }
                }
            }
        }
    }
}

/// The keyframe's angles moved by `change`, where its turned rotation is expected to read.
fn offset_angles(key: &Transform, change: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| key.rotation[i] + change[i])
}

/// The XYZ angles of `rotation` that read closest to `near`. A rotation has two sets of angles,
/// (x, y, z) and (x + 180°, 180° − y, z + 180°), each axis give or take full turns, and plain
/// Euler angles keep Y within ±90°: a keyframe at 120° turned by 30° would read
/// [180°, 30°, 180°] and one at -360° would read 30°. Near the expected angles they read 150°
/// and -330°, keeping the turns and the winding that the timeline and `oxy_entity` show. The
/// game turns the same either way, interpolation taking the short path between keyframes.
pub fn angles_near(rotation: Quat, near: [f32; 3]) -> [f32; 3] {
    use std::f32::consts::{PI, TAU};
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    let wound = |angles: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|i| angles[i] + TAU * ((near[i] - angles[i]) / TAU).round())
    };
    let distance =
        |angles: &[f32; 3]| -> f32 { angles.iter().zip(near).map(|(a, b)| (a - b).powi(2)).sum() };
    let first = wound([x, y, z]);
    let second = wound([x + PI, PI - y, z + PI]);
    if distance(&second) < distance(&first) {
        second
    } else {
        first
    }
}

/// Keyframes are local to the parent, like the object's own transform. When a copy of
/// `target` goes under another parent, `change` taking the old parent's space to the new one
/// (the new parent's world matrix inverted, times the old one's) and its rest transform going
/// from `before` to `after`, every track that animates it (in any clip of `entities`) changes
/// space the same way and the copy plays the same path in the world: under a parent twice as
/// big the path is half as long. [`carry_tracks`] is for an edit of the object itself, which
/// keeps the path's length. Keyframe parts equal to the old rest pose land exactly on the new
/// one. Refused, with nothing changed, when a keyframe would need shear in the new space.
pub fn rebase_tracks(
    entities: &mut [Entity],
    target: &str,
    change: Mat4,
    before: &Transform,
    after: &Transform,
) -> Result<(), String> {
    let rebased = entities
        .iter()
        .flat_map(|e| &e.clips)
        .flat_map(|c| &c.tracks)
        .filter(|t| t.target == target)
        .flat_map(|t| &t.keyframes)
        .map(|key| rebased(&key.transform, change, before, after))
        .collect::<Result<Vec<_>, _>>()?;
    let keys = entities
        .iter_mut()
        .flat_map(|e| &mut e.clips)
        .flat_map(|c| &mut c.tracks)
        .filter(|t| t.target == target)
        .flat_map(|t| &mut t.keyframes);
    for (key, transform) in keys.zip(rebased) {
        key.transform = transform;
    }
    Ok(())
}

/// One keyframe of [`rebase_tracks`] under the new parent.
fn rebased(
    key: &Transform,
    change: Mat4,
    before: &Transform,
    after: &Transform,
) -> Result<Transform, String> {
    // The game poses the object around its own pivot, whatever the keyframe stored.
    let posed = |t: &Transform| Transform {
        pivot: before.pivot,
        ..t.clone()
    };
    let matrix = change * posed(key).matrix();
    let found = Transform::from_matrix(matrix, before.pivot);
    let mut next = key.clone();
    next.position = if key.position == before.position {
        after.position
    } else {
        found.position
    };
    next.rotation = if key.rotation == before.rotation {
        after.rotation
    } else {
        let turned = std::array::from_fn(|i| after.rotation[i] - before.rotation[i]);
        angles_near(found.quaternion(), offset_angles(key, turned))
    };
    next.scale = if key.scale == before.scale {
        after.scale
    } else {
        found.scale
    };
    let check = posed(&next);
    if !check.finite() || !check.matrix().abs_diff_eq(matrix, 0.0001) {
        return Err("Um quadro-chave ficaria deformado no novo pai (cisalhamento)".into());
    }
    Ok(next)
}

/// After the rest transform of `id` changed from `before`, carries the keyframes along when
/// the object animates itself ([`animates_itself`]). The editor's panels, gizmos and
/// [`transform_selection`] go through here, as `set_transform` does by default.
pub fn follow_tracks(scene: &mut Scene, id: &str, before: &Transform) {
    let Some(entity) = scene.entity(id) else {
        return;
    };
    if entity.transform == *before || !animates_itself(entity) {
        return;
    }
    let after = entity.transform.clone();
    carry_tracks(&mut scene.entities, id, before, &after);
}

/// Sets the local transform of `id`, carrying its keyframes when it animates itself.
pub fn set_local_transform(scene: &mut Scene, id: &str, transform: Transform) {
    let Some(entity) = scene.entity_mut(id) else {
        return;
    };
    let before = std::mem::replace(&mut entity.transform, transform);
    follow_tracks(scene, id, &before);
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
        set_local_transform(scene, &id, transform);
    }
    Ok(())
}

/// Objects of the scenes and models that use an asset, with the part that uses it: role
/// `material`, `ui` (the image of an interface element), `model` (a placed model, by the root
/// of each placement, not every piece) or the operation of a node naming it.
pub fn asset_uses(project: &Project, asset: &str) -> Vec<crate::input_actions::Use> {
    use crate::input_actions::{Use, every_entity};
    // The pieces of the placements: a placement's root is the one whose parent is not one.
    let placed: HashSet<&str> = every_entity(project)
        .filter(|(_, e)| e.model_source.as_deref() == Some(asset))
        .map(|(_, e)| e.id.as_str())
        .collect();
    let mut found = Vec::new();
    for (scope, entity) in every_entity(project) {
        let mut add = |role: &str| found.push(Use::new(scope, entity, role));
        if entity.material.texture.as_deref() == Some(asset) {
            add("material");
        }
        if entity.ui.as_ref().and_then(|u| u.texture.as_deref()) == Some(asset) {
            add("ui");
        }
        if placed.contains(entity.id.as_str())
            && entity
                .parent
                .as_deref()
                .is_none_or(|parent| !placed.contains(parent))
        {
            add("model");
        }
        for node in &entity.graph.nodes {
            if node
                .params
                .values()
                .any(|v| matches!(v, Value::Text(id) if id == asset))
            {
                add(&node.operation);
            }
        }
    }
    found
}

/// [`asset_uses`] as the editor lists them: "Scope → Object: use".
pub fn asset_references(project: &Project, asset: &str) -> Vec<String> {
    let definitions = crate::graph::registry();
    asset_uses(project, asset)
        .into_iter()
        .map(|found| {
            let what = match found.role.as_str() {
                "material" => "material".to_owned(),
                "ui" => "imagem da interface".to_owned(),
                "model" => "modelo".to_owned(),
                operation => format!(
                    "ação {}",
                    definitions
                        .iter()
                        .find(|d| d.id == operation)
                        .map_or("não reconhecida", |d| d.label)
                ),
            };
            format!("{} → {}: {what}", found.scope, found.name)
        })
        .collect()
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
    fn a_turned_spin_keeps_turning_the_same_way() {
        use crate::animation::{Clip, Interpolation, Keyframe, sample_track};
        let mut s = Scene::new("Giro", SceneKind::ThreeD);
        let mut wheel = Entity::new("Roda", Some(Primitive::Cylinder));
        let id = wheel.id.clone();
        let mut clip = Clip::new("Girar");
        // A full turn in thirds.
        for (time, degrees) in [(0., 0f32), (1., 120.), (2., 240.), (3., 360.)] {
            let mut pose = wheel.transform.clone();
            pose.rotation[1] = degrees.to_radians();
            clip.insert_key(
                &id,
                Keyframe {
                    time,
                    transform: pose,
                    interpolation: Interpolation::Linear,
                },
            );
        }
        let original = clip.tracks[0].clone();
        wheel.clips.push(clip);
        s.entities.push(wheel);
        let mut turned = s.entity(&id).unwrap().transform.clone();
        turned.rotation = [0., 30f32.to_radians(), 0.];
        let turn = turned.quaternion();
        set_local_transform(&mut s, &id, turned.clone());
        let track = &s.entity(&id).unwrap().clips[0].tracks[0];
        assert_eq!(track.keyframes[0].transform.rotation, turned.rotation);
        assert!((track.keyframes[3].transform.rotation[1] - 390f32.to_radians()).abs() < 1e-5);
        for time in [0.25, 0.5, 1.25, 1.5, 2.5, 2.75] {
            let expected = turn * sample_track(&original, time).unwrap().quaternion();
            let got = sample_track(track, time).unwrap().quaternion();
            assert!(
                got.dot(expected).abs() > 0.9999,
                "{time}: {got:?} / {expected:?}"
            );
        }
    }
    #[test]
    fn angles_near_keep_the_rotation_and_read_the_expected_set() {
        let degrees = [-200f32, -90., -30., 0., 45., 120., 180., 390.];
        let quaternion = |rotation| {
            Transform {
                rotation,
                ..Default::default()
            }
            .quaternion()
        };
        for x in degrees {
            for y in degrees {
                for z in degrees {
                    let angles = [x, y, z].map(f32::to_radians);
                    let rotation = quaternion(angles);
                    let read = angles_near(rotation, angles);
                    assert!(
                        quaternion(read).dot(rotation).abs() > 0.99999,
                        "{x} {y} {z}"
                    );
                    // Away from Y at ±90°, where X and Z share one axis, the angles come back.
                    if y != -90. {
                        assert!(
                            read.iter().zip(angles).all(|(a, b)| (a - b).abs() < 1e-3),
                            "{x} {y} {z}: {read:?}"
                        );
                    }
                }
            }
        }
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
