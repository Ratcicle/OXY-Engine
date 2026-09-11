use super::*;
use crate::surface::{PlatformMode, VelocitySpace};

#[derive(Clone, Debug)]
pub(super) struct PlatformFrame {
    pub matrix: Mat4,
    pub delta: Vec3,
    pub velocity: Vec3,
    pub discontinuous: bool,
}
pub(super) fn advance(
    scene: &mut Scene,
    evaluation: &mut SceneEvaluation,
    previous: &HashMap<Id, PlatformFrame>,
    looped: &HashSet<Id>,
    dt: f32,
) -> Result<HashMap<Id, PlatformFrame>, String> {
    if !scene
        .entities
        .iter()
        .any(|e| e.platform.as_ref().is_some_and(|p| p.enabled))
    {
        return Ok(HashMap::new());
    }
    let initial = evaluation.worlds.clone();
    let mut order = evaluation.index.roots.clone();
    let mut cursor = 0;
    while cursor < order.len() {
        order.extend(evaluation.index.children[order[cursor]].iter().copied());
        cursor += 1;
    }
    let mut inherited: Vec<Option<(f32, bool)>> = vec![None; scene.entities.len()];
    for &i in &order {
        let entity = &scene.entities[i];
        let parent_index = entity
            .parent
            .as_ref()
            .and_then(|id| evaluation.index.position(id));
        inherited[i] = parent_index.and_then(|p| inherited[p]);
        if let Some(config) = entity.platform.as_ref().filter(|p| p.enabled) {
            inherited[i] = Some((
                config.max_transport_per_step,
                inherited[i].is_some_and(|(_, jump)| jump) || looped.contains(&entity.id),
            ));
            if config.mode == PlatformMode::Velocity {
                let before = evaluation.worlds[i].ok_or("Plataforma sem transformação")?;
                let mut velocity = Vec3::from(config.velocity);
                if config.space == VelocitySpace::Local {
                    velocity = crate::physics3d::world_pose(before)?.1 * velocity;
                }
                let parent = parent_index
                    .and_then(|p| evaluation.worlds[p])
                    .unwrap_or(Mat4::IDENTITY);
                let delta = parent.inverse().transform_vector3(velocity * dt);
                if !delta.is_finite() {
                    return Err(format!(
                        "{}: translação de plataforma inválida",
                        entity.name
                    ));
                }
                let id = entity.id.clone();
                scene.entities[i].transform.position =
                    (Vec3::from(entity.transform.position) + delta).to_array();
                evaluation.refresh_subtree(scene, &id);
            }
        } else if let Some((limit, jump)) = inherited[i] {
            inherited[i] = Some((limit, jump || looped.contains(&entity.id)));
        }
    }
    let mut result: HashMap<Id, PlatformFrame> = HashMap::new();
    for i in order {
        let Some((limit, looped)) = inherited[i] else {
            continue;
        };
        let entity = &scene.entities[i];
        // Keep intermediate joints too: a rotated parent invalidates transport
        // even when a child collider happens to retain its own orientation.
        let matrix = evaluation.worlds[i].ok_or("Plataforma sem transformação")?;
        let prior = previous
            .get(&entity.id)
            .map(|f| f.matrix)
            .or(initial[i])
            .unwrap();
        let (old_position, old_rotation, old_scale) = crate::physics3d::world_pose(prior)?;
        let (position, rotation, scale) = crate::physics3d::world_pose(matrix)?;
        let delta = position - old_position;
        let parent_jump = entity
            .parent
            .as_ref()
            .and_then(|id| result.get(id))
            .is_some_and(|p| p.discontinuous);
        let discontinuous = looped
            || parent_jump
            || delta.length() > limit
            || !old_scale.abs_diff_eq(scale, 1e-4)
            || old_rotation.dot(rotation).abs() < 0.999999;
        result.insert(
            entity.id.clone(),
            PlatformFrame {
                matrix,
                delta: if discontinuous { Vec3::ZERO } else { delta },
                velocity: if discontinuous {
                    Vec3::ZERO
                } else {
                    delta / dt
                },
                discontinuous,
            },
        );
    }
    Ok(result)
}
