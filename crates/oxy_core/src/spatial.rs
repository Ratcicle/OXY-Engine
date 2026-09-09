//! The physical AABB convention shared by simulation and authoring tools.
use crate::{collision::Aabb, document::*};
use glam::{Mat4, Vec3};

pub fn invertible_world(scene: &Scene, id: &str) -> Result<Mat4, String> {
    let world = scene.world_matrix(id)?;
    if !world.is_finite() || world.determinant().abs() < 1e-8 {
        return Err(
            "A transformação não pode ser invertida. Ajuste as escalas da peça e dos pais.".into(),
        );
    }
    Ok(world)
}
pub fn physical_scale(world: Mat4) -> Vec3 {
    Vec3::new(
        world.x_axis.truncate().length(),
        world.y_axis.truncate().length(),
        world.z_axis.truncate().length(),
    )
}
/// Size stays axis aligned; only the center follows the complete hierarchy.
/// Visibility, enabled and is_trigger never change the geometric result.
pub fn collider_bounds(scene: &Scene, id: &str) -> Result<Aabb, String> {
    let collider = scene
        .entity(id)
        .and_then(|e| e.collider.as_ref())
        .ok_or("Objeto sem colisor")?;
    let world = scene.world_matrix(id)?;
    bounds_from_world(collider, world)
}
/// Central physical convention, also used with evaluated world transforms.
pub fn bounds_from_world(collider: &Collider, world: Mat4) -> Result<Aabb, String> {
    crate::metrics::count(|c| c.boxes += 1);
    if !world.is_finite() {
        return Err("Transformação inválida na caixa.".into());
    }
    let size = Vec3::from(collider.size);
    let offset = Vec3::from(collider.offset);
    if !size.is_finite() || size.min_element() <= 0. || !offset.is_finite() {
        return Err("Tamanho e deslocamento da caixa inválidos.".into());
    }
    Ok(Aabb::from_center_half(
        world.transform_point3(offset),
        size * physical_scale(world) * 0.5,
    ))
}

/// Converts a world-space physical box back to the existing collider parameters.
pub fn set_collider_bounds(scene: &mut Scene, id: &str, bounds: Aabb) -> Result<(), String> {
    let world = invertible_world(scene, id)?;
    let size = bounds.max - bounds.min;
    if !size.is_finite() || !bounds.min.is_finite() || size.min_element() < 0.0001 {
        return Err("A caixa precisa ter tamanho positivo em todos os eixos.".into());
    }
    let offset = world
        .inverse()
        .transform_point3((bounds.min + bounds.max) * 0.5);
    let size = size / physical_scale(world);
    if !size.is_finite() || !offset.is_finite() {
        return Err("Transformação da caixa inválida.".into());
    }
    let collider = scene
        .entity_mut(id)
        .and_then(|e| e.collider.as_mut())
        .ok_or("Objeto sem colisor")?;
    collider.size = size.to_array();
    collider.offset = offset.to_array();
    Ok(())
}

/// A signed edge mask: -1 moves the minimum, +1 the maximum, 0 keeps that axis.
pub fn resize_box(mut bounds: Aabb, edges: [i8; 3], delta: Vec3, snap: Option<f32>) -> Aabb {
    for axis in 0..3 {
        let round = |v: f32| snap.filter(|s| *s > 0.).map_or(v, |s| (v / s).round() * s);
        if edges[axis] < 0 {
            bounds.min[axis] = round(bounds.min[axis] + delta[axis]).min(bounds.max[axis] - 0.001);
        }
        if edges[axis] > 0 {
            bounds.max[axis] = round(bounds.max[axis] + delta[axis]).max(bounds.min[axis] + 0.001);
        }
    }
    bounds
}

pub fn visual_bounds(scene: &Scene, ids: &[Id]) -> Result<Aabb, String> {
    let view = crate::scene_view::SceneView::new(scene);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for id in ids {
        let e = view.entity(id).ok_or("Peça não encontrada")?;
        if !e.has_geometry() || e.camera.is_some() || e.ui.is_some() {
            continue;
        }
        let world = view.world_matrix(id)?;
        if !world.is_finite() || world.determinant().abs() < 1e-8 {
            return Err(
                "A transformação não pode ser invertida. Ajuste as escalas da peça e dos pais."
                    .into(),
            );
        }
        if let Some(mesh) = &e.mesh {
            for vertex in &mesh.data().vertices {
                let p = world.transform_point3(Vec3::from(vertex.position));
                min = min.min(p);
                max = max.max(p);
            }
            continue;
        }
        let mut half = Vec3::from(e.dimensions) * 0.5;
        match e.primitive {
            Some(Primitive::Plane) => half.y = 0.,
            Some(Primitive::Rectangle | Primitive::Sprite | Primitive::Circle) => half.z = 0.,
            _ => {}
        }
        for x in [-1., 1.] {
            for y in [-1., 1.] {
                for z in [-1., 1.] {
                    let p = world.transform_point3(half * Vec3::new(x, y, z));
                    min = min.min(p);
                    max = max.max(p);
                }
            }
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(
            "Sem geometria própria. Use Ajustar ao grupo/filhos e escolha as peças.".into(),
        );
    }
    // Flat geometry still needs a positive depth for the existing 2D collider schema.
    Ok(Aabb::from_center_half(
        (min + max) * 0.5,
        ((max - min) * 0.5).max(Vec3::splat(0.0005)),
    ))
}

fn check_tracks(scene: &Scene, ids: &std::collections::HashSet<Id>) -> Result<(), String> {
    let affected: Vec<_> = scene
        .entities
        .iter()
        .flat_map(|e| e.clips.iter().map(move |c| (e, c)))
        .filter(|(_, c)| {
            c.tracks
                .iter()
                .any(|t| ids.contains(&t.target) && !t.keyframes.is_empty())
        })
        .map(|(e, c)| format!("{} / {}", e.name, c.name))
        .collect();
    if affected.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Alteração estrutural bloqueada: afetaria {}. A preservação de toda a interpolação ainda não é suportada. Faça uma cópia sem essas trilhas antes de reorganizar articulações.",
            affected.join(", ")
        ))
    }
}

pub fn check_reparent_animation(
    scene: &Scene,
    id: &str,
    parent: Option<&str>,
) -> Result<(), String> {
    if scene.entity(id).and_then(|e| e.parent.as_deref()) == parent {
        return Ok(());
    }
    let mut ids: std::collections::HashSet<_> = scene.descendants(id).into_iter().collect();
    for start in [scene.entity(id).and_then(|e| e.parent.as_deref()), parent] {
        let mut current = start;
        let mut visited = std::collections::HashSet::new();
        while let Some(p) = current {
            if !visited.insert(p) {
                return Err("Ciclo na hierarquia".into());
            }
            ids.insert(p.to_owned());
            current = scene.entity(p).and_then(|e| e.parent.as_deref());
        }
    }
    check_tracks(scene, &ids)
}

/// Structural edit: exactly preserves the local matrix, hence all descendant world matrices.
pub fn move_pivot(scene: &mut Scene, id: &str, pivot: Vec3) -> Result<(), String> {
    if !pivot.is_finite() {
        return Err("Pivô inválido.".into());
    }
    invertible_world(scene, id)?;
    check_tracks(scene, &std::collections::HashSet::from([id.to_owned()]))?;
    let transform = &mut scene.entity_mut(id).ok_or("Objeto ausente")?.transform;
    let old = transform.matrix();
    let delta = old.transform_vector3(pivot - Vec3::from(transform.pivot));
    let mut next = transform.clone();
    next.position = (Vec3::from(transform.position) + delta).to_array();
    next.pivot = pivot.to_array();
    if !next.finite() || !next.matrix().abs_diff_eq(old, 0.0001) {
        return Err("Não foi possível preservar a montagem com essa transformação.".into());
    }
    *transform = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collider_pivot_and_fit_commands_roundtrip_as_deltas() {
        use crate::{edit_history::CommandHistory, texture_cache::TextureCache};
        let (scene, id, _) = fixture();
        let mut project = Project::new("Histórico");
        project.start_scene = scene.id.clone();
        project.scenes = vec![scene];
        let base = project.clone();
        let mut h = CommandHistory::new();
        let mut images = TextureCache::default();
        h.begin("Ajuste e pivô", &project, &images);
        let b = visual_bounds(&project.scenes[0], &project.scenes[0].descendants(&id)).unwrap();
        set_collider_bounds(&mut project.scenes[0], &id, b).unwrap();
        move_pivot(&mut project.scenes[0], &id, Vec3::new(0., 1., 0.)).unwrap();
        h.commit(&project, &mut images).unwrap();
        let after = project.clone();
        assert_eq!(h.undo_len(), 1);
        h.undo(&mut project, &mut images).unwrap();
        assert_eq!(project, base);
        h.redo(&mut project, &mut images).unwrap();
        assert_eq!(project, after);
    }
    use crate::animation::{Clip, Interpolation, Keyframe, sample_clip};
    fn fixture() -> (Scene, Id, Id) {
        let mut s = Scene::new("Montagem", SceneKind::ThreeD);
        let mut p = Entity::new("Pai", None);
        p.transform.position = [2., -1., 3.];
        p.transform.scale = [-2., 3., 1.];
        p.transform.rotation = [0.2, 0.5, -0.3];
        let mut e = Entity::new("Braço", Some(Primitive::Cube));
        e.parent = Some(p.id.clone());
        e.transform.pivot = [0.2, 0.5, 0.];
        e.transform.rotation = [0., 0., 0.6];
        e.collider = Some(Collider::default());
        let mut child = Entity::new("Mão", Some(Primitive::Sphere));
        child.parent = Some(e.id.clone());
        child.transform.position = [0., -1.2, 0.];
        child.collider = Some(Collider::default());
        let ids = (e.id.clone(), child.id.clone());
        s.entities.extend([p, e, child]);
        (s, ids.0, ids.1)
    }
    #[test]
    fn resize_opposite_edge_and_move_center_preserve_entities_and_flags() {
        let (mut scene, id, child) = fixture();
        let before = scene.clone();
        let b = collider_bounds(&scene, &id).unwrap();
        let next = resize_box(b, [1, 0, 0], Vec3::new(1.7, 9., 9.), Some(0.25));
        assert_eq!(next.min, b.min);
        assert_eq!(next.max.y, b.max.y);
        set_collider_bounds(&mut scene, &id, next).unwrap();
        let actual = collider_bounds(&scene, &id).unwrap();
        assert!(
            actual.min.abs_diff_eq(next.min, 0.0001) && actual.max.abs_diff_eq(next.max, 0.0001)
        );
        let moved = actual.translated(Vec3::new(2., -3., 1.));
        set_collider_bounds(&mut scene, &id, moved).unwrap();
        assert_eq!(
            scene.entity(&id).unwrap().transform,
            before.entity(&id).unwrap().transform
        );
        assert_eq!(
            scene.world_matrix(&child).unwrap(),
            before.world_matrix(&child).unwrap()
        );
        assert!(
            collider_bounds(&scene, &id)
                .unwrap()
                .min
                .abs_diff_eq(moved.min, 0.0001)
        );
        assert_eq!(
            scene
                .entity(&id)
                .unwrap()
                .collider
                .as_ref()
                .unwrap()
                .enabled,
            before
                .entity(&id)
                .unwrap()
                .collider
                .as_ref()
                .unwrap()
                .enabled
        );
        assert_eq!(
            resize_box(b, [-1, 0, 0], Vec3::splat(1000.), None).max,
            b.max
        );
    }
    #[test]
    fn pivot_preserves_matrices_children_colliders_and_static_animation_intervals() {
        let (mut scene, id, child) = fixture();
        let original = scene.clone();
        // Only the child is animated: changing its static ancestor pivot is exactly safe.
        let mut clip = Clip::new("Mão");
        for time in [0., 1.] {
            let mut pose = scene.entity(&child).unwrap().transform.clone();
            pose.rotation[0] = time;
            clip.insert_key(
                &child,
                Keyframe {
                    time,
                    transform: pose,
                    interpolation: Interpolation::Linear,
                },
            );
        }
        move_pivot(&mut scene, &id, Vec3::new(-0.7, 1.3, 0.4)).unwrap();
        for target in [&id, &child] {
            assert!(
                scene
                    .world_matrix(target)
                    .unwrap()
                    .abs_diff_eq(original.world_matrix(target).unwrap(), 0.0001)
            );
            assert!(
                collider_bounds(&scene, target)
                    .unwrap()
                    .min
                    .abs_diff_eq(collider_bounds(&original, target).unwrap().min, 0.0001)
            );
        }
        for time in [0.13, 0.37, 0.81] {
            let mut a = original.clone();
            let mut b = scene.clone();
            sample_clip(&mut a, &clip, time);
            sample_clip(&mut b, &clip, time);
            assert!(
                a.world_matrix(&child)
                    .unwrap()
                    .abs_diff_eq(b.world_matrix(&child).unwrap(), 0.0001)
            );
        }
        let old_world = scene.world_matrix(&id).unwrap();
        scene.entity_mut(&id).unwrap().transform.rotation[2] += 0.4;
        assert!(
            !scene
                .world_matrix(&id)
                .unwrap()
                .abs_diff_eq(old_world, 0.001)
        );
    }
    #[test]
    fn animated_structural_edits_and_singular_transforms_are_atomic() {
        let (mut scene, id, _) = fixture();
        let mut clip = Clip::new("Ataque");
        clip.insert_key(
            &id,
            Keyframe {
                time: 0.,
                transform: Transform::default(),
                interpolation: Interpolation::Linear,
            },
        );
        scene.entities[0].clips.push(clip);
        let original = scene.clone();
        assert!(
            move_pivot(&mut scene, &id, Vec3::X)
                .unwrap_err()
                .contains("Ataque")
        );
        assert_eq!(scene, original);
        assert!(
            scene
                .reparent(&id, None, true)
                .unwrap_err()
                .contains("Ataque")
        );
        assert_eq!(scene, original);
        scene.entities[0].transform.scale = [0., 1., 1.];
        let original = scene.clone();
        assert!(move_pivot(&mut scene, &id, Vec3::Y).is_err());
        assert_eq!(scene, original);
    }
    #[test]
    fn fit_nested_geometry_uses_extents_and_can_exclude_weapon() {
        let (mut scene, id, child) = fixture();
        let b = visual_bounds(&scene, &[id.clone(), child]).unwrap();
        let own = visual_bounds(&scene, std::slice::from_ref(&id)).unwrap();
        assert!(b.min.cmple(own.min).all() && b.max.cmpge(own.max).all());
        let mut sword = Entity::new("Espada", Some(Primitive::Cube));
        sword.parent = Some(id.clone());
        sword.transform.position = [15., 0., 0.];
        sword.dimensions = [4., 1., 1.];
        scene.entities.push(sword);
        let all = visual_bounds(&scene, &scene.descendants(&id)).unwrap();
        assert!((all.max - all.min).length() > (b.max - b.min).length());
        set_collider_bounds(&mut scene, &id, b).unwrap();
        let actual = collider_bounds(&scene, &id).unwrap();
        assert!(actual.min.abs_diff_eq(b.min, 0.0001) && actual.max.abs_diff_eq(b.max, 0.0001));
    }
    #[test]
    fn empty_invisible_group_uses_runtime_box_including_pivot_and_mirror() {
        for kind in [SceneKind::TwoD, SceneKind::ThreeD] {
            let mut scene = Scene::new("Caixas", kind);
            let mut parent = Entity::new("Pai", None);
            parent.transform.position = [3., 2., -1.];
            parent.transform.scale = [-2., 3., 1.5];
            let mut body = Entity::new("Personagem", None);
            body.parent = Some(parent.id.clone());
            body.visible = false;
            body.transform.pivot = [0.7, -0.4, 0.2];
            body.transform.rotation = [0., 0., 0.3];
            body.collider = Some(Collider {
                size: [2., 4., 1.],
                offset: [0.2, 0.5, 0.],
                ..Default::default()
            });
            let id = body.id.clone();
            scene.entities.extend([parent, body]);
            let matrix = scene.world_matrix(&id).unwrap();
            let (scale, _, _) = matrix.to_scale_rotation_translation();
            let expected = Aabb::from_center_half(
                matrix.transform_point3(Vec3::new(0.2, 0.5, 0.)),
                Vec3::new(2., 4., 1.) * scale.abs() * 0.5,
            );
            assert_eq!(collider_bounds(&scene, &id).unwrap(), expected);
            assert_eq!(crate::runtime::collider_box(&scene, &id), Some(expected));
            scene
                .entity_mut(&id)
                .unwrap()
                .collider
                .as_mut()
                .unwrap()
                .enabled = false;
            assert_eq!(collider_bounds(&scene, &id).unwrap(), expected);
            assert!(crate::runtime::collider_box(&scene, &id).is_none());
        }
    }
}
