use super::*;
use crate::surface::{PlatformMode, VelocitySpace};
use std::f32::consts::{PI, TAU};

/// The most a platform may turn in one step and still carry whoever stands on it (1200° a
/// second): a faster turn drops them, like a jump.
const MAX_TURN_PER_STEP: f32 = 20. * PI / 180.;

#[derive(Clone, Debug)]
pub(super) struct PlatformFrame {
    pub matrix: Mat4,
    /// `matrix` taken apart: position, rotation and scale.
    pose: (Vec3, Quat, Vec3),
    /// Where the platform was a step before, position and rotation, which presentation moves
    /// it from: where it is on a jump and once a pause settles the shown poses.
    previous: (Vec3, Quat),
    /// The rigid move from the last step's place to `matrix`, which carries whoever stands
    /// on it; none on a jump.
    pub motion: Mat4,
    /// How far that move turned about the world's vertical, in radians (counterclockwise
    /// seen from above): the turn a passenger turns with.
    pub turn: f32,
    pub discontinuous: bool,
}
impl PlatformFrame {
    /// The velocity of the platform's point at `point`, over the last step of `dt` seconds.
    pub fn point_velocity(&self, point: Vec3, dt: f32) -> Vec3 {
        (self.motion.transform_point3(point) - point) / dt
    }
    /// Where the platform is shown `alpha` of the way through the last step, moved and turned
    /// along it; none on a jump, which shows it where it is.
    pub fn shown(&self, alpha: f32) -> Option<Mat4> {
        if self.discontinuous {
            return None;
        }
        let (old_position, old_rotation) = self.previous;
        let (position, rotation, scale) = self.pose;
        let mut shown = if old_rotation == rotation {
            self.matrix
        } else {
            Mat4::from_scale_rotation_translation(
                scale,
                old_rotation.slerp(rotation, alpha),
                Vec3::ZERO,
            )
        };
        shown.w_axis = (position - (position - old_position) * (1. - alpha)).extend(1.);
        Some(shown)
    }
    /// Shows the platform where it is, with no move left to interpolate.
    pub fn settle(&mut self) {
        self.previous = (self.pose.0, self.pose.1);
    }
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
        // Keep intermediate joints too: a parent that jumped drops what stands on its
        // children even when a child collider happens to keep its own place.
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
            || old_rotation.angle_between(rotation) > MAX_TURN_PER_STEP;
        let change = (rotation * old_rotation.inverse()).normalize();
        // The twist about the vertical: a platform that tilts does not turn its passengers.
        let twist = 2. * change.y.atan2(change.w);
        let turn = if twist > PI {
            twist - TAU
        } else if twist <= -PI {
            twist + TAU
        } else {
            twist
        };
        result.insert(
            entity.id.clone(),
            if discontinuous {
                PlatformFrame {
                    matrix,
                    pose: (position, rotation, scale),
                    previous: (position, rotation),
                    motion: Mat4::IDENTITY,
                    turn: 0.,
                    discontinuous,
                }
            } else {
                PlatformFrame {
                    matrix,
                    pose: (position, rotation, scale),
                    previous: (old_position, old_rotation),
                    // Turned about its place a step before, then moved: rigid, whatever the
                    // platform's scale.
                    motion: Mat4::from_translation(position)
                        * Mat4::from_quat(change)
                        * Mat4::from_translation(-old_position),
                    turn,
                    discontinuous,
                }
            },
        );
    }
    Ok(result)
}
