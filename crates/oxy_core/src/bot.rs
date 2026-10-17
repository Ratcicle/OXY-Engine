//! A bot that plays one 3D character along a path of points in the headless simulation. Each
//! step it gives the character's movement the direction toward the point it heads for and
//! presses the character's own input actions (jump, sprint, crouch), as a person at the
//! controls would, so the game's bindings and logic run as in play. It walks straight at each
//! point: where it stops coming nearer it gives up and says why, instead of solving the level.
//! The character may be in any scene: the bot takes over once that scene runs. A point may
//! make it stand until an object, such as a moving platform, comes within reach, and logic that
//! holds the character (Bloquear / liberar entrada) is told apart from a wall.
use crate::{
    character::{CharacterConfig, CharacterState, axis_toward},
    document::{Id, Project, Scene, SceneKind},
    runtime::{FIXED_DT, InputFrame, Runtime},
    scene_view::SceneView,
    spatial,
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
/// A `wait_for` without `within`: how far across the ground its object's top may be, in metres.
pub const WAIT_WITHIN: f32 = 1.;
/// A `wait_for` without `timeout`: seconds of game time the bot stands before it gives up.
pub const WAIT_TIMEOUT: f32 = 30.;

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

/// Before the bot heads for a point, it stands where it is until `object` is within reach of
/// where it stands: the point before, as that point is then (the character's feet before the
/// first point), the top of the object (see [`reach`]) at most `within` metres away across the
/// ground and at most `height` above or below the point; or, when the point before follows an
/// object, as a platform it rides, the tops of the two objects as far apart at most (see
/// [`reach_from`]).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WaitFor {
    /// An object of the character's scene, as a moving platform or the place it brings the
    /// character to.
    pub object: Id,
    #[serde(default = "default_within")]
    pub within: f32,
    /// None: the character's step height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    /// Seconds of game time it stands before it gives up.
    #[serde(default = "default_timeout")]
    pub timeout: f32,
}
fn default_within() -> f32 {
    WAIT_WITHIN
}
fn default_timeout() -> f32 {
    WAIT_TIMEOUT
}

/// A point of the path: `[x, y, z]` in the world, or `{at?, object?, jump?, sprint?, crouch?,
/// wait?, radius?, wait_for?}`. The height is the character's feet, its origin.
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
    /// Stands before heading for the point until an object is within reach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<WaitFor>,
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
            #[serde(default)]
            wait_for: Option<WaitFor>,
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
            wait_for: point.wait_for,
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

/// The world box whose top an object is reached at: its 3D collider's, else its shape's and the
/// pieces' under it, else its position. None when `scene` lacks the object.
fn top_box(scene: &Scene, object: &str) -> Option<(Vec3, Vec3)> {
    let entity = scene.entity(object)?;
    let world = scene.world_matrix(object).ok()?;
    match entity
        .physics3d
        .as_ref()
        .and_then(|collider| spatial::collider3d_bounds(collider, world).ok())
        .map(|(bounds, _)| bounds)
        .or_else(|| spatial::shown_bounds(&SceneView::new(scene), object))
    {
        Some(bounds) => Some((bounds.min, bounds.max)),
        None => world_position(scene, object).map(|at| (at, at)),
    }
}

/// How far apart two boxes are across the ground: 0 where they meet or overlap.
fn gap(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> f32 {
    let apart =
        |a_min: f32, a_max: f32, b_min: f32, b_max: f32| (a_min - b_max).max(b_min - a_max).max(0.);
    Vec2::new(
        apart(a.0.x, a.1.x, b.0.x, b.1.x),
        apart(a.0.z, a.1.z, b.0.z, b.1.z),
    )
    .length()
}

/// How far the top of `object` is from feet at `feet` (a point of a path, or a character's
/// position): the gap across the ground to its top face (0 over it) and how high that face is
/// above the feet (negative below them). The top is that of the object's 3D collider, else of
/// its shape and the pieces under it, else its position. None when `scene` lacks the object.
pub fn reach(scene: &Scene, object: &str, feet: Vec3) -> Option<(f32, f32)> {
    let top = top_box(scene, object)?;
    Some((gap(top, (feet, feet)), top.1.y - feet.y))
}

/// As [`reach`], from the top of `from`, an object the character rides, instead of feet: the
/// gap across the ground between the two tops (0 where they meet or overlap) and how high the
/// top of `object` is above that of `from`.
pub fn reach_from(scene: &Scene, object: &str, from: &str) -> Option<(f32, f32)> {
    let top = top_box(scene, object)?;
    let under = top_box(scene, from)?;
    Some((gap(top, under), top.1.y - under.1.y))
}

/// Where the bot stands while it waits for a `wait_for`: at a point (the point before, or the
/// character's feet before the first point), or on the object the point before follows, which
/// it rides.
#[derive(Clone, Copy)]
enum Stand<'a> {
    At(Vec3),
    On(&'a str),
}

impl Bot {
    /// Refuses what the bot cannot play in `project`, the copy the run starts from, with the
    /// IDs already resolved: a character no scene has, or one the bot cannot drive, and points
    /// following, or waiting for, objects its scene lacks.
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
            if let Some(wait) = &point.wait_for {
                if !wait.within.is_finite() || wait.within < 0. {
                    return refused("wait_for.within precisa ser um número a partir de 0".into());
                }
                if wait.height.is_some_and(|h| !h.is_finite() || h < 0.) {
                    return refused("wait_for.height precisa ser um número a partir de 0".into());
                }
                if !wait.timeout.is_finite() || wait.timeout <= 0. {
                    return refused("wait_for.timeout precisa ser um número maior que 0".into());
                }
            }
            let objects = [
                (point.object.as_ref(), "o ponto segue"),
                (
                    point.wait_for.as_ref().map(|wait| &wait.object),
                    "o wait_for espera",
                ),
            ];
            for (id, what) in objects {
                let Some(id) = id.filter(|id| scene.entity(id).is_none()) else {
                    continue;
                };
                let elsewhere = project.scenes.iter().find_map(|s| Some((s, s.entity(id)?)));
                return refused(match elsewhere {
                    Some((other, object)) => format!(
                        "{} está na cena {}, e {what} um objeto da cena do personagem ({})",
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
    /// It came no [`PROGRESS`] nearer to point `index` for `stuck_after` seconds, or waited for
    /// its `wait_for` past the timeout: the bot let go there, the character at `position`,
    /// `distance` metres from the point, for `reason`. `last_support` is the object the
    /// character last stood on; `held_by`, the reasons logic held its movement by, when held.
    Stuck {
        index: usize,
        position: Vec3,
        distance: f32,
        reason: StuckReason,
        last_support: Option<Id>,
        held_by: Vec<String>,
    },
    /// Logic held the character's movement on its way to the last point, since after it reached
    /// the point before (or took over), for `stuck_after` seconds: the game took over at the end
    /// of the path, which counts as done. `held_by` are the reasons.
    Released { held_by: Vec<String> },
}

/// Why a bot gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StuckReason {
    /// The character, in the air, went without support for [`FALL_AFTER`] seconds or more, or
    /// is more than [`FALL_DROP`] below where it last stood.
    Fell,
    /// Logic held the character's movement (Bloquear / liberar entrada, on the movement).
    Held,
    /// It came no nearer otherwise, as standing against a wall.
    Blocked,
    /// The object of the point's `wait_for` did not come within reach before its timeout.
    Waiting,
    /// The character, or the object its point follows or waits for, was not in the running
    /// scene.
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
    /// When (game time) logic began holding the character's movement, while it holds it.
    held_since: Option<f64>,
    /// When (game time) it reached its last point, or took over.
    last_reach: f64,
    /// The point whose `wait_for` it stands for, and since when (game time).
    waiting: Option<(usize, f64)>,
    /// When (game time) the object of that `wait_for` went missing from the running scene,
    /// while it is missing.
    missing: Option<f64>,
    /// The point whose `wait_for` came true, and when (game time): it heads for it.
    cleared: Option<(usize, f64)>,
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
            held_since: None,
            last_reach: 0.,
            waiting: None,
            missing: None,
            cleared: None,
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
        self.last_reach = runtime.game_time();
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
            held_by: Vec::new(),
        })
    }
    /// Whether the character at `position`, in the air long enough or low enough, fell.
    fn fell(&self, character: &CharacterState, position: Vec3, now: f64) -> bool {
        !character.grounded
            && (now - self.supported_at >= FALL_AFTER
                || self
                    .support
                    .as_ref()
                    .is_some_and(|(_, feet)| position.y < feet - FALL_DROP))
    }
    /// Gives up on the character at `position`, `distance` metres from its point: it fell when
    /// in the air long enough or low enough, logic holds it when it blocks its movement (on the
    /// way to the last point, since after the point before: the path is done), else something
    /// blocks it.
    fn give_up(
        &mut self,
        bot: &Bot,
        character: &CharacterState,
        position: Vec3,
        distance: f32,
        now: f64,
    ) -> BotEvent {
        let fell = self.fell(character, position, now);
        let held_by: Vec<String> = if fell {
            Vec::new()
        } else {
            character.movement_blocks.iter().cloned().collect()
        };
        if !held_by.is_empty()
            && self.next + 1 == bot.path.len()
            && self.held_since.is_some_and(|since| since > self.last_reach)
        {
            self.done = true;
            return BotEvent::Released { held_by };
        }
        self.stuck = true;
        BotEvent::Stuck {
            index: self.next,
            position,
            distance,
            reason: if fell {
                StuckReason::Fell
            } else if !held_by.is_empty() {
                StuckReason::Held
            } else {
                StuckReason::Blocked
            },
            last_support: self.last_support(),
            held_by,
        }
    }
    /// Stands for the `wait_for` of point `index`: Ok(true) when the point has none or its
    /// object came within reach of where the bot stands (`from`), and the bot then heads for
    /// it; Ok(false) while it waits; the event when it gives up: the character fell, the object
    /// left the running scene `stuck_after` seconds ago, or the timeout passed.
    #[allow(clippy::too_many_arguments)]
    fn wait_for(
        &mut self,
        bot: &Bot,
        index: usize,
        from: Stand,
        scene: &Scene,
        config: &CharacterConfig,
        character: &CharacterState,
        now: f64,
    ) -> Result<bool, BotEvent> {
        let Some(wait) = bot.path.get(index).and_then(|p| p.wait_for.as_ref()) else {
            return Ok(true);
        };
        if self.cleared.is_some_and(|(cleared, _)| cleared == index) {
            return Ok(true);
        }
        let since = match self.waiting {
            Some((waiting, since)) if waiting == index => since,
            _ => {
                self.waiting = Some((index, now));
                self.missing = None;
                now
            }
        };
        let position = character.position;
        let height = wait.height.unwrap_or(config.step_height);
        if scene.entity(&wait.object).is_some() {
            self.missing = None;
        } else {
            self.missing.get_or_insert(now);
        }
        let near = match from {
            Stand::At(feet) => reach(scene, &wait.object, feet),
            Stand::On(ridden) => reach_from(scene, &wait.object, ridden),
        };
        if near.is_some_and(|(gap, rise)| gap <= wait.within && rise.abs() <= height) {
            self.waiting = None;
            self.cleared = Some((index, now));
            return Ok(true);
        }
        let fell = self.fell(character, position, now);
        // As a point whose object left the scene, it gives up on one gone for stuck_after.
        let gone = self
            .missing
            .is_some_and(|missing| now - missing >= f64::from(bot.stuck_after));
        if !fell && !gone && now - since < f64::from(wait.timeout) {
            return Ok(false);
        }
        self.stuck = true;
        Err(BotEvent::Stuck {
            index,
            position,
            distance: bot.path[index]
                .position(scene)
                .map_or(0., |point| point.distance(position)),
            reason: if fell {
                StuckReason::Fell
            } else if self.missing.is_some() {
                StuckReason::Absent
            } else {
                StuckReason::Waiting
            },
            last_support: self.last_support(),
            held_by: Vec::new(),
        })
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
    if character.movement_blocks.is_empty() {
        state.held_since = None;
    } else {
        state.held_since.get_or_insert(now);
    }
    let Some(mut target) = bot.path[state.next].position(scene) else {
        steer_axis(frame, config, Vec2::ZERO);
        return state.idle(bot, now);
    };
    let mut point = &bot.path[state.next];
    // Its wait_for, before it heads for the point (for the points after the first, it stood
    // for it at the point before).
    if state.arrived.is_none() {
        let from = Stand::At(position);
        match state.wait_for(bot, state.next, from, scene, config, character, now) {
            Ok(true) => {}
            Ok(false) => {
                state.last = (position, target.distance(position));
                steer_axis(frame, config, Vec2::ZERO);
                return None;
            }
            Err(stuck) => {
                steer_axis(frame, config, Vec2::ZERO);
                return Some(stuck);
            }
        }
    }
    let mut event = None;
    let offset = target - position;
    if state.arrived.is_none()
        && Vec2::new(offset.x, offset.z).length() <= point.radius.unwrap_or(bot.radius)
        && offset.y.abs() <= character.height * 0.5
    {
        state.arrived = Some(now);
        state.last_reach = now;
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
        // The next point's wait_for: it stands here until its object is within reach of this
        // point, or of the object this point follows, which it rides, from the step after it
        // arrived (that step reports the arrival).
        let waits = bot
            .path
            .get(state.next + 1)
            .is_some_and(|next| next.wait_for.is_some());
        let ready = if waits && event.is_some() {
            Ok(false)
        } else {
            let from = point.object.as_deref().map_or(Stand::At(target), Stand::On);
            state.wait_for(bot, state.next + 1, from, scene, config, character, now)
        };
        match ready {
            Ok(true) => {}
            Ok(false) => {
                state.last = (position, offset.length());
                actions(config, character, point, state, hold, tap);
                steer_axis(frame, config, Vec2::ZERO);
                return event;
            }
            Err(stuck) => {
                steer_axis(frame, config, Vec2::ZERO);
                return Some(stuck);
            }
        }
        // How long it has been free to leave: since its wait ended and, with a wait_for, since
        // that came true.
        let waited = now - arrived - f64::from(point.wait);
        let free = state
            .cleared
            .filter(|(index, _)| *index == state.next + 1)
            .map_or(waited, |(_, at)| waited.min(now - at));
        // Leaves the point: takes off from it and heads for the next one in the same step.
        if point.jump {
            // Running across the way, it first turns toward the next point on the ground, for
            // a short while, so the jump goes that way.
            if free < ALIGN_SECONDS
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
        return Some(state.give_up(bot, character, position, distance, now));
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
        // A wait_for takes its object and defaults; it writes back what it read.
        let point: Waypoint =
            serde_json::from_value(serde_json::json!({"object": "a", "wait_for": {"object": "b"}}))
                .unwrap();
        let wait = point.wait_for.clone().unwrap();
        assert_eq!(
            (wait.object.as_str(), wait.within, wait.height, wait.timeout),
            ("b", WAIT_WITHIN, None, WAIT_TIMEOUT)
        );
        assert_eq!(
            serde_json::to_value(&point).unwrap()["wait_for"],
            serde_json::json!({"object": "b", "within": 1.0, "timeout": 30.0})
        );
        assert!(
            serde_json::from_value::<Waypoint>(
                serde_json::json!({"at": [0, 0, 0], "wait_for": {"object": "b", "near": 2}})
            )
            .is_err()
        );
    }

    #[test]
    fn reach_measures_the_gap_to_an_objects_top_and_how_high_it_is() {
        use crate::document::{Entity, Primitive};
        use crate::physics3d::{Collider3d, CollisionShape};
        let mut scene = Scene::new("Rio", SceneKind::ThreeD);
        let mut raft = Entity::new("Balsa", Some(Primitive::Cube));
        raft.dimensions = [3., 0.4, 3.];
        raft.transform.position = [0., 0.3, -4.];
        raft.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: [3., 0.4, 3.],
            },
            ..Default::default()
        });
        let raft_id = raft.id.clone();
        let mut post = Entity::new("Marco", None);
        post.transform.position = [2., 1., 0.];
        let post_id = post.id.clone();
        scene.entities = vec![raft, post];
        // 1 m short of its near edge (z = -2.5), level with its top (y = 0.5).
        let (gap, rise) = reach(&scene, &raft_id, Vec3::new(0., 0.5, -1.5)).unwrap();
        assert!((gap - 1.).abs() < 1e-5 && rise.abs() < 1e-5, "{gap} {rise}");
        // Over it, lower: no gap, its top above the feet.
        let (gap, rise) = reach(&scene, &raft_id, Vec3::new(1., 0., -4.)).unwrap();
        assert!(gap == 0. && (rise - 0.5).abs() < 1e-5, "{gap} {rise}");
        // Off a corner, across both axes.
        let (gap, _) = reach(&scene, &raft_id, Vec3::new(4.5, 0.5, -1.5)).unwrap();
        assert!((gap - 3f32.hypot(1.)).abs() < 1e-5, "{gap}");
        // Without a shape or collider, its position.
        let (gap, rise) = reach(&scene, &post_id, Vec3::new(0., 0., 0.)).unwrap();
        assert!(
            (gap - 2.).abs() < 1e-5 && (rise - 1.).abs() < 1e-5,
            "{gap} {rise}"
        );
        assert!(reach(&scene, "nada", Vec3::ZERO).is_none());
        // From an object it rides: between the two tops, a dock's shape and the raft's collider.
        let mut dock = Entity::new("Cais", Some(Primitive::Cube));
        dock.dimensions = [4., 1., 4.];
        dock.transform.position = [0., 0., 1.];
        let dock_id = dock.id.clone();
        scene.entities.push(dock);
        let (gap, rise) = reach_from(&scene, &dock_id, &raft_id).unwrap();
        assert!(
            (gap - 1.5).abs() < 1e-5 && rise.abs() < 1e-5,
            "{gap} {rise}"
        );
        let (gap, rise) = reach_from(&scene, &raft_id, &post_id).unwrap();
        assert!(
            (gap - 0.5f32.hypot(2.5)).abs() < 1e-5 && (rise + 0.5).abs() < 1e-5,
            "{gap} {rise}"
        );
        assert!(reach_from(&scene, &dock_id, "nada").is_none());
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
            held_since: None,
            last_reach: 0.,
            waiting: None,
            missing: None,
            cleared: None,
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
