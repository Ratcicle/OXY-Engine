//! A bot that plays one 3D character along a path of points in the headless simulation. Each
//! step it gives the character's movement the direction toward the point it heads for and
//! presses the character's own input actions (jump, sprint, crouch), as a person at the
//! controls would, so the game's bindings and logic run as in play. It walks straight at each
//! point: where it stops coming nearer it gives up and says why, instead of solving the level.
//! The character may be in any scene: the bot takes over once that scene runs.
use crate::{
    character::{CharacterConfig, CharacterState, axis_toward},
    document::{Id, Project, Scene, SceneKind},
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
/// Seconds of game time without support after which a character the bot gives up on fell.
pub const FALL_AFTER: f64 = 1.;
/// How far below where it last stood a character the bot gives up on fell, in metres.
pub const FALL_DROP: f32 = 1.;
/// How fast a character may move across the way to the next point, in m/s, as the bot leaves
/// a jump point: faster, it turns toward that point on the ground first, for at most
/// [`ALIGN_SECONDS`] of game time, and jumps then.
pub const ALIGN_SPEED: f32 = 1.;
pub const ALIGN_SECONDS: f64 = 0.25;

/// A path for one character, as a request gives it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bot {
    /// The 3D character it plays, of any scene.
    pub entity: Id,
    pub path: Vec<Waypoint>,
    /// How near a point counts as reached across the ground, in metres.
    #[serde(default = "default_radius")]
    pub radius: f32,
    /// Seconds after the character's scene starts that it takes over, as an input's `at`: from
    /// the latest start of that scene before it took over (the run's start for the start
    /// scene, unless the game leaves and reopens it first).
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
    /// An object of the character's scene whose world position (its pivot) the point follows
    /// as it moves, as a moving platform's.
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
    /// Refuses what the bot cannot play in `project`, the copy the run starts from, with the
    /// IDs already resolved: a character no scene has, or one the bot cannot drive, and points
    /// following objects its scene lacks.
    pub fn check(&self, project: &Project) -> Result<(), String> {
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
        let (scene, entity) = project
            .scenes
            .iter()
            .find_map(|s| Some((s, s.entity(&self.entity)?)))
            .ok_or_else(|| format!("bot: nenhuma cena do projeto tem o objeto {}", self.entity))?;
        if scene.kind != SceneKind::ThreeD {
            return Err(format!(
                "bot: a cena {} é 2D; o bot só joga com personagens 3D",
                scene.name
            ));
        }
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
                let elsewhere = project.scenes.iter().find_map(|s| Some((s, s.entity(id)?)));
                return refused(match elsewhere {
                    Some((other, object)) => format!(
                        "{} está na cena {}, e o ponto segue um objeto da cena do personagem \
                         ({})",
                        object.name, other.name, scene.name
                    ),
                    None => format!("nenhuma cena do projeto tem o objeto {id}"),
                });
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
    /// go there, the character at `position`, `distance` metres from the point, for `reason`.
    /// `last_support` is the object the character last stood on.
    Stuck {
        index: usize,
        position: Vec3,
        distance: f32,
        reason: StuckReason,
        last_support: Option<Id>,
    },
}

/// Why a bot gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StuckReason {
    /// The character, in the air, went without support for [`FALL_AFTER`] seconds or more, or
    /// is more than [`FALL_DROP`] below where it last stood.
    Fell,
    /// It came no nearer otherwise, as standing against a wall.
    Blocked,
    /// The character, or the object its point follows, was not in the running scene.
    Absent,
}

/// Where a bot is along its path.
#[derive(Clone, Debug)]
pub struct BotState {
    /// When (run time) the character's scene last started, until the bot takes over; None
    /// while another scene runs.
    scene_start: Option<f64>,
    /// When (run time) it took over: `start` after that latest start. None until then.
    started: Option<f64>,
    /// The point it heads for.
    next: usize,
    /// When (game time) it reached `next`, while it waits there.
    arrived: Option<f64>,
    /// The nearest it came to `next`, and when (game time) it last came [`PROGRESS`] nearer.
    nearest: Option<(f32, f64)>,
    /// Where the character last was, and how far from `next`.
    last: (Vec3, f32),
    /// The object the character last stood on and the height of its feet there, and when (game
    /// time) it last stood on something, or the bot took over.
    support: Option<(Id, f32)>,
    supported_at: f64,
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
    /// A bot about to play, in the scene `runtime` starts with.
    pub fn new(runtime: &Runtime, bot: &Bot) -> Self {
        let mut state = Self {
            scene_start: None,
            started: None,
            next: 0,
            arrived: None,
            nearest: None,
            last: (Vec3::ZERO, 0.),
            support: None,
            supported_at: 0.,
            held: BTreeSet::new(),
            tapped: BTreeSet::new(),
            crouch_tap: None,
            done: false,
            stuck: false,
        };
        state.scene_started(runtime, bot);
        state
    }
    /// The running scene started, by a change or a reload: until the bot takes over, its
    /// `start` counts from the latest start of the character's scene.
    pub fn scene_started(&mut self, runtime: &Runtime, bot: &Bot) {
        if self.started.is_none() {
            self.scene_start = runtime
                .scene()
                .entity(&bot.entity)
                .is_some()
                .then_some(runtime.time);
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
    /// When (run time) it took over; None while it waits for its character's scene or `start`.
    pub fn started_at(&self) -> Option<f64> {
        self.started
    }
    /// Starts driving the character where `runtime` has it, `at` (run time).
    fn take_over(&mut self, runtime: &Runtime, bot: &Bot, at: f64) {
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
        self.started = Some(at);
        self.last = (position, distance);
        self.supported_at = runtime.game_time();
    }
    fn last_support(&self) -> Option<Id> {
        self.support.as_ref().map(|(id, _)| id.clone())
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
            reason: StuckReason::Absent,
            last_support: self.last_support(),
        })
    }
    /// Gives up on the character at `position`, `distance` metres from its point: it fell when
    /// in the air long enough or low enough, else something blocks it.
    fn give_up(
        &mut self,
        character: &CharacterState,
        position: Vec3,
        distance: f32,
        now: f64,
    ) -> BotEvent {
        let fell = !character.grounded
            && (now - self.supported_at >= FALL_AFTER
                || self
                    .support
                    .as_ref()
                    .is_some_and(|(_, feet)| position.y < feet - FALL_DROP));
        self.stuck = true;
        BotEvent::Stuck {
            index: self.next,
            position,
            distance,
            reason: if fell {
                StuckReason::Fell
            } else {
                StuckReason::Blocked
            },
            last_support: self.last_support(),
        }
    }
}

/// Plays `bot` in the step `runtime` is about to take: the movement toward its point goes into
/// `frame` in place of the movement inputs and the character's direction actions, and so do
/// the actions it presses. Before it takes over (`start` after its character's scene starts)
/// and once done or stuck it leaves `frame` alone, but for letting go of what it held.
pub fn steer(
    runtime: &Runtime,
    bot: &Bot,
    state: &mut BotState,
    frame: &mut InputFrame,
) -> Option<BotEvent> {
    let mut hold = BTreeSet::new();
    let mut tap = BTreeSet::new();
    // The same rule as timed inputs: it plays the step that ends at or after `start`.
    if state.started.is_none()
        && let Some(at) = state.scene_start.map(|begun| begun + bot.start)
        && at <= runtime.time + f64::from(FIXED_DT)
    {
        state.take_over(runtime, bot, at);
    }
    let event = if state.started.is_some() && !state.finished() {
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
    if character.grounded {
        state.supported_at = now;
        if let Some(support) = &character.support {
            state.support = Some((support.object.clone(), position.y));
        }
    }
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
            // Running across the way, it first turns toward the next point on the ground, for
            // a short while, so the jump goes that way.
            if now - arrived - f64::from(point.wait) < ALIGN_SECONDS
                && let Some(way) = across_the_way(bot, state, scene, character)
            {
                state.last = (position, offset.length());
                actions(config, character, point, state, hold, tap);
                steer_axis(frame, config, axis_toward(way, yaw));
                return event;
            }
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
        return Some(state.give_up(character, position, distance, now));
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

/// The way across the ground to the point after the one the bot leaves, when the character,
/// on the ground, moves across that way faster than [`ALIGN_SPEED`]: a jump now would carry it
/// off the way.
fn across_the_way(
    bot: &Bot,
    state: &BotState,
    scene: &Scene,
    character: &CharacterState,
) -> Option<Vec3> {
    if !character.grounded {
        return None;
    }
    let next = bot.path.get(state.next + 1)?.position(scene)?;
    let way = Vec3::new(
        next.x - character.position.x,
        0.,
        next.z - character.position.z,
    )
    .try_normalize()?;
    let velocity = Vec3::new(character.velocity.x, 0., character.velocity.z);
    let sideways = velocity - way * velocity.dot(way);
    (sideways.length() > ALIGN_SPEED).then_some(way)
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
            scene_start: Some(0.),
            started: Some(0.),
            next: 0,
            arrived: None,
            nearest: None,
            last: (Vec3::ZERO, 0.),
            support: None,
            supported_at: 0.,
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
