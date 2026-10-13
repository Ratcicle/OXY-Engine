//! A bot that plays one 3D character along a path of points in the headless simulation. Each
//! step it gives the character's movement the direction toward the point it heads for and
//! presses the character's own input actions (jump, sprint, crouch), as a person at the
//! controls would, so the game's bindings and logic run as in play. It walks straight at each
//! point: where it stops coming nearer it gives up and says so, instead of solving the level.
use crate::{
    character::{CharacterConfig, CharacterState, axis_toward},
    document::{Id, Scene, SceneKind},
    runtime::{FIXED_DT, InputFrame, Runtime},
};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::BTreeSet;

/// Most points one path may have.
pub const MAX_POINTS: usize = 300;
/// How much nearer to its point the bot must come within `stuck_after`, in metres.
pub const PROGRESS: f32 = 0.1;

/// A path for one character, as a request gives it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bot {
    /// The 3D character it plays.
    pub entity: Id,
    pub path: Vec<Waypoint>,
    /// How near a point counts as reached across the ground, in metres.
    #[serde(default = "default_radius")]
    pub radius: f32,
    /// Simulated second it takes over, as an input's `at`.
    #[serde(default)]
    pub start: f64,
    /// Seconds of game time without coming [`PROGRESS`] nearer to its point before it gives up.
    #[serde(default = "default_stuck_after")]
    pub stuck_after: f32,
}
fn default_radius() -> f32 {
    0.6
}
fn default_stuck_after() -> f32 {
    1.5
}

/// A point of the path: `[x, y, z]` in the world, or `{at?, object?, jump?, sprint?, crouch?,
/// wait?, radius?}`. The height is the character's feet, its origin.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(try_from = "Json")]
pub struct Waypoint {
    /// A world point; with `object`, an offset from that object, in world axes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 3]>,
    /// An object of the scene whose world position (its pivot) the point follows as it moves,
    /// as a moving platform's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Id>,
    /// Presses the jump action once on leaving the point, which is then the take-off spot.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub jump: bool,
    /// Holds the sprint action on the way to the point and while waiting there.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sprint: bool,
    /// Holds the crouch action the same way; a crouch that toggles takes one tap per point.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub crouch: bool,
    /// Seconds of game time standing at the point once there.
    #[serde(skip_serializing_if = "is_zero")]
    pub wait: f32,
    /// The point's own reach, instead of the bot's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
}
fn is_zero(value: &f32) -> bool {
    *value == 0.
}
impl TryFrom<Json> for Waypoint {
    type Error = String;
    fn try_from(value: Json) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Detailed {
            #[serde(default)]
            at: Option<[f32; 3]>,
            #[serde(default)]
            object: Option<Id>,
            #[serde(default)]
            jump: bool,
            #[serde(default)]
            sprint: bool,
            #[serde(default)]
            crouch: bool,
            #[serde(default)]
            wait: f32,
            #[serde(default)]
            radius: Option<f32>,
        }
        if value.is_array() {
            return serde_json::from_value(value)
                .map(|at| Self {
                    at: Some(at),
                    ..Self::default()
                })
                .map_err(|e| e.to_string());
        }
        let point: Detailed = serde_json::from_value(value).map_err(|e| e.to_string())?;
        Ok(Self {
            at: point.at,
            object: point.object,
            jump: point.jump,
            sprint: point.sprint,
            crouch: point.crouch,
            wait: point.wait,
            radius: point.radius,
        })
    }
}
impl Waypoint {
    /// Where the point is in `scene` now; None when its object is not there.
    pub fn position(&self, scene: &Scene) -> Option<Vec3> {
        let at = Vec3::from(self.at.unwrap_or_default());
        match &self.object {
            None => Some(at),
            Some(id) => Some(world_position(scene, id)? + at),
        }
    }
}
/// Where an object's position (its pivot) is in the world.
fn world_position(scene: &Scene, id: &str) -> Option<Vec3> {
    let entity = scene.entity(id)?;
    let world = scene.world_matrix(id).ok()?;
    Some(world.transform_point3(Vec3::from(entity.transform.pivot)))
}

impl Bot {
    /// Refuses what the bot cannot play in `scene`, the scene the run starts in, with the IDs
    /// already resolved.
    pub fn check(&self, scene: &Scene) -> Result<(), String> {
        if self.path.is_empty() {
            return Err("bot: informe ao menos um ponto em path".into());
        }
        if self.path.len() > MAX_POINTS {
            return Err(format!("bot: no máximo {MAX_POINTS} pontos em path"));
        }
        if !self.radius.is_finite() || self.radius <= 0. {
            return Err("bot: radius precisa ser um número maior que 0".into());
        }
        if !self.start.is_finite() || self.start < 0. {
            return Err("bot: start precisa ser um número a partir de 0".into());
        }
        if !self.stuck_after.is_finite() || self.stuck_after <= 0. {
            return Err("bot: stuck_after precisa ser um número maior que 0".into());
        }
        if scene.kind != SceneKind::ThreeD {
            return Err(format!(
                "bot: a cena {} é 2D; o bot só joga com personagens 3D",
                scene.name
            ));
        }
        let entity = scene
            .entity(&self.entity)
            .ok_or_else(|| format!("bot: o objeto {} não está na cena inicial", self.entity))?;
        let Some(config) = &entity.character3d else {
            return Err(format!(
                "bot: {} não tem Personagem 3D (character3d)",
                entity.name
            ));
        };
        if !config.automatic_input {
            return Err(format!(
                "bot: {} está com a entrada automática desligada (character3d.automatic_input: \
                 false) e ignora as entradas que o bot aperta",
                entity.name
            ));
        }
        for (index, point) in self.path.iter().enumerate() {
            let refused = |problem: String| {
                Err(format!(
                    "bot: ponto {index} do caminho (o {}º): {problem}",
                    index + 1
                ))
            };
            if point.at.is_none() && point.object.is_none() {
                return refused("informe at ou object".into());
            }
            if point.at.is_some_and(|at| at.iter().any(|v| !v.is_finite())) {
                return refused("at precisa de números finitos".into());
            }
            if !point.wait.is_finite() || point.wait < 0. {
                return refused("wait precisa ser um número a partir de 0".into());
            }
            if point.radius.is_some_and(|r| !r.is_finite() || r <= 0.) {
                return refused("radius precisa ser um número maior que 0".into());
            }
            if let Some(id) = &point.object
                && scene.entity(id).is_none()
            {
                return refused(format!("o objeto {id} não está na cena inicial"));
            }
        }
        Ok(())
    }
}

/// What happened to a bot in a step.
#[derive(Clone, Debug, PartialEq)]
pub enum BotEvent {
    /// The character reached point `index` (counted from 0), standing at `position`.
    Reached { index: usize, position: Vec3 },
    /// It came no [`PROGRESS`] nearer to point `index` for `stuck_after` seconds: the bot let
    /// go there, the character at `position`, `distance` metres from the point.
    Stuck {
        index: usize,
        position: Vec3,
        distance: f32,
    },
}

/// Where a bot is along its path.
#[derive(Clone, Debug)]
pub struct BotState {
    /// The point it heads for.
    next: usize,
    /// When (game time) it reached `next`, while it waits there.
    arrived: Option<f64>,
    /// The nearest it came to `next`, and when (game time) it last came [`PROGRESS`] nearer.
    nearest: Option<(f32, f64)>,
    /// Where the character last was, and how far from `next`.
    last: (Vec3, f32),
    /// The actions it holds down, and those of them only tapped for the last step.
    held: BTreeSet<String>,
    tapped: BTreeSet<String>,
    /// The point it last tapped a crouch that toggles for: once per point, so a crouch that
    /// logic or a block keeps as it is takes no stream of presses.
    crouch_tap: Option<usize>,
    done: bool,
    stuck: bool,
}
impl BotState {
    /// A bot about to play, its character where `runtime` has it.
    pub fn new(runtime: &Runtime, bot: &Bot) -> Self {
        let scene = runtime.scene();
        let position = runtime
            .character_state(&bot.entity)
            .map(|state| state.position)
            .or_else(|| world_position(scene, &bot.entity))
            .unwrap_or_default();
        let distance = bot
            .path
            .first()
            .and_then(|point| point.position(scene))
            .map_or(0., |point| point.distance(position));
        Self {
            next: 0,
            arrived: None,
            nearest: None,
            last: (position, distance),
            held: BTreeSet::new(),
            tapped: BTreeSet::new(),
            crouch_tap: None,
            done: false,
            stuck: false,
        }
    }
    /// It went through the whole path: it reached the last point, waited and jumped there.
    pub fn done(&self) -> bool {
        self.done
    }
    /// It let go of the character, done or stuck.
    pub fn finished(&self) -> bool {
        self.done || self.stuck
    }
    /// A step without the character, or the point's object, in the running scene: no progress,
    /// so after `stuck_after` the bot gives up where it last saw them.
    fn idle(&mut self, bot: &Bot, now: f64) -> Option<BotEvent> {
        let nearest = self.nearest.get_or_insert((self.last.1, now));
        if now - nearest.1 < f64::from(bot.stuck_after) {
            return None;
        }
        self.stuck = true;
        Some(BotEvent::Stuck {
            index: self.next,
            position: self.last.0,
            distance: self.last.1,
        })
    }
}

/// Plays `bot` in the step `runtime` is about to take: the movement toward its point goes into
/// `frame` in place of the movement inputs and the character's direction actions, and so do
/// the actions it presses. Before `start` and once done or stuck it leaves `frame` alone, but
/// for letting go of what it held.
pub fn steer(
    runtime: &Runtime,
    bot: &Bot,
    state: &mut BotState,
    frame: &mut InputFrame,
) -> Option<BotEvent> {
    let mut hold = BTreeSet::new();
    let mut tap = BTreeSet::new();
    // The same rule as timed inputs: it plays the step that ends at or after `start`.
    let started = bot.start <= runtime.time + f64::from(FIXED_DT);
    let event = if started && !state.finished() {
        drive(runtime, bot, state, frame, &mut hold, &mut tap)
    } else {
        None
    };
    press(state, hold, tap, frame);
    event
}

fn drive(
    runtime: &Runtime,
    bot: &Bot,
    state: &mut BotState,
    frame: &mut InputFrame,
    hold: &mut BTreeSet<String>,
    tap: &mut BTreeSet<String>,
) -> Option<BotEvent> {
    if runtime.game_paused() {
        // The world stands still and so does the game clock: keep holding, take no step.
        hold.extend(state.held.difference(&state.tapped).cloned());
        return None;
    }
    let now = runtime.game_time();
    let scene = runtime.scene();
    let (Some(config), Some(character), Some(yaw)) = (
        scene
            .entity(&bot.entity)
            .and_then(|e| e.character3d.as_ref()),
        runtime.character_state(&bot.entity),
        runtime.movement_yaw(&bot.entity),
    ) else {
        return state.idle(bot, now);
    };
    let position = character.position;
    let Some(mut target) = bot.path[state.next].position(scene) else {
        steer_axis(frame, config, Vec2::ZERO);
        return state.idle(bot, now);
    };
    let mut point = &bot.path[state.next];
    let mut event = None;
    let offset = target - position;
    if state.arrived.is_none()
        && Vec2::new(offset.x, offset.z).length() <= point.radius.unwrap_or(bot.radius)
        && offset.y.abs() <= character.height * 0.5
    {
        state.arrived = Some(now);
        event = Some(BotEvent::Reached {
            index: state.next,
            position,
        });
    }
    if let Some(arrived) = state.arrived {
        if now - arrived < f64::from(point.wait) {
            state.last = (position, offset.length());
            actions(config, character, point, state, hold, tap);
            steer_axis(frame, config, Vec2::ZERO);
            return event;
        }
        // Leaves the point: takes off from it and heads for the next one in the same step.
        if point.jump {
            tap.insert(config.actions.jump.clone());
        }
        state.next += 1;
        state.arrived = None;
        state.nearest = None;
        let Some(next) = bot.path.get(state.next) else {
            state.done = true;
            return event;
        };
        point = next;
        let Some(next) = point.position(scene) else {
            steer_axis(frame, config, Vec2::ZERO);
            return event.or_else(|| state.idle(bot, now));
        };
        target = next;
    }
    let offset = target - position;
    let distance = offset.length();
    state.last = (position, distance);
    let nearest = state.nearest.get_or_insert((distance, now));
    if distance <= nearest.0 - PROGRESS {
        *nearest = (distance, now);
    } else if now - nearest.1 >= f64::from(bot.stuck_after) {
        state.stuck = true;
        return Some(BotEvent::Stuck {
            index: state.next,
            position,
            distance,
        });
    }
    actions(config, character, point, state, hold, tap);
    let across = Vec3::new(offset.x, 0., offset.z);
    // Within reach across the ground but not in height, it stands: on a lift, or stuck.
    let axis = if across.length() > point.radius.unwrap_or(bot.radius) {
        axis_toward(across.normalize(), yaw)
    } else {
        Vec2::ZERO
    };
    steer_axis(frame, config, axis);
    event
}

/// The point's sprint and crouch, held as it asks; a crouch that toggles gets one tap per point.
fn actions(
    config: &CharacterConfig,
    character: &CharacterState,
    point: &Waypoint,
    state: &mut BotState,
    hold: &mut BTreeSet<String>,
    tap: &mut BTreeSet<String>,
) {
    if point.sprint {
        hold.insert(config.sprint_action.clone());
    }
    if !config.crouch_toggle {
        if point.crouch {
            hold.insert(config.crouch_action.clone());
        }
    } else if point.crouch != character.want_crouch && state.crouch_tap != Some(state.next) {
        tap.insert(config.crouch_action.clone());
        state.crouch_tap = Some(state.next);
    }
}

/// The bot's movement, in place of the movement inputs and the character's direction actions,
/// whose presses and releases it drops too.
fn steer_axis(frame: &mut InputFrame, config: &CharacterConfig, axis: Vec2) {
    let actions = &config.actions;
    for action in [
        &actions.left,
        &actions.right,
        &actions.forward,
        &actions.back,
    ] {
        frame.held.remove(action);
        frame.pressed.remove(action);
        frame.released.remove(action);
    }
    frame.movement = axis.to_array();
}

/// Puts the bot's actions in `frame`: a press where it starts holding one or taps it, a release
/// where it lets go, unless the request's own inputs hold that action. A release the inputs give
/// for an action the bot holds waits until the bot lets go.
fn press(
    state: &mut BotState,
    hold: BTreeSet<String>,
    tap: BTreeSet<String>,
    frame: &mut InputFrame,
) {
    for action in hold.iter().chain(&tap) {
        if tap.contains(action) || (!state.held.contains(action) && !frame.held.contains(action)) {
            frame.pressed.insert(action.clone());
        }
        frame.released.remove(action);
        frame.held.insert(action.clone());
    }
    for action in &state.held {
        if !hold.contains(action) && !tap.contains(action) && !frame.held.contains(action) {
            frame.released.insert(action.clone());
        }
    }
    state.held = hold.union(&tap).cloned().collect();
    state.tapped = tap;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::wish_direction;

    #[test]
    fn the_axis_toward_a_direction_undoes_wish_direction() {
        for yaw in [0., 0.7, -2.4, std::f32::consts::PI] {
            for axis in [Vec2::new(0., 1.), Vec2::new(-1., 0.), Vec2::new(0.6, -0.8)] {
                let back = axis_toward(wish_direction(axis, yaw), yaw);
                assert!(back.abs_diff_eq(axis, 1e-5), "{yaw} {axis} {back}");
            }
        }
    }

    #[test]
    fn points_are_plain_or_detailed_and_unknown_fields_are_refused() {
        let point: Waypoint = serde_json::from_value(serde_json::json!([1, 0, -2])).unwrap();
        assert_eq!(point.at, Some([1., 0., -2.]));
        let point: Waypoint =
            serde_json::from_value(serde_json::json!({"object": "a", "jump": true, "wait": 0.5}))
                .unwrap();
        assert_eq!(
            (point.object.as_deref(), point.jump, point.wait),
            (Some("a"), true, 0.5)
        );
        assert!(serde_json::from_value::<Waypoint>(serde_json::json!({"pular": true})).is_err());
        assert!(serde_json::from_value::<Waypoint>(serde_json::json!([1, 0])).is_err());
        // It writes back in the shape it reads.
        assert_eq!(
            serde_json::to_value(&point).unwrap(),
            serde_json::json!({"object": "a", "jump": true, "wait": 0.5})
        );
        let bot: Bot =
            serde_json::from_value(serde_json::json!({"entity": "x", "path": [[0, 0, 0]]}))
                .unwrap();
        assert_eq!((bot.radius, bot.start, bot.stuck_after), (0.6, 0., 1.5));
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn held_actions_press_once_and_release_unless_the_inputs_hold_them() {
        let mut state = BotState {
            next: 0,
            arrived: None,
            nearest: None,
            last: (Vec3::ZERO, 0.),
            held: BTreeSet::new(),
            tapped: BTreeSet::new(),
            crouch_tap: None,
            done: false,
            stuck: false,
        };
        let mut frame = InputFrame::default();
        press(&mut state, set(&["correr"]), set(&["pular"]), &mut frame);
        assert_eq!(frame.pressed, set(&["correr", "pular"]));
        assert_eq!(frame.held, set(&["correr", "pular"]));
        // Still running: no new press; the tap ends with a release.
        let mut frame = InputFrame::default();
        press(&mut state, set(&["correr"]), set(&[]), &mut frame);
        assert_eq!((frame.pressed, frame.released), (set(&[]), set(&["pular"])));
        // The inputs let go of it while the bot still runs: no release while it is held.
        let mut frame = InputFrame {
            released: set(&["correr"]),
            ..Default::default()
        };
        press(&mut state, set(&["correr"]), set(&[]), &mut frame);
        assert_eq!((frame.held, frame.released), (set(&["correr"]), set(&[])));
        // The request's own inputs keep holding it: no release.
        let mut frame = InputFrame {
            held: set(&["correr"]),
            ..Default::default()
        };
        press(&mut state, set(&[]), set(&[]), &mut frame);
        assert!(frame.released.is_empty() && state.held.is_empty());
    }

    #[test]
    fn the_bots_movement_drops_the_direction_actions_of_the_inputs() {
        let config = CharacterConfig::default();
        let mut frame = InputFrame {
            held: set(&["mover_frente", "correr"]),
            pressed: set(&["mover_frente"]),
            released: set(&["mover_tras", "agachar"]),
            movement: [1., 0.],
            ..Default::default()
        };
        steer_axis(&mut frame, &config, Vec2::new(0., -1.));
        assert_eq!(
            (frame.held, frame.pressed, frame.released, frame.movement),
            (set(&["correr"]), set(&[]), set(&["agachar"]), [0., -1.])
        );
    }
}
