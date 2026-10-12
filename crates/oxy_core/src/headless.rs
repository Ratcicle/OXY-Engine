//! Runs a game without a window for a fixed simulated time and reports what happened:
//! logs, logic actions, sounds, scene changes and the state of watched objects. Used by the
//! MCP server so an agent can test a change, and by tests.
use crate::{
    document::*,
    graph,
    input_timeline::{InputChange, TimedInput},
    runtime::{FIXED_DT, InputFrame, Runtime},
};
use serde::{Deserialize, Serialize};

/// Longest simulation one request may ask for.
pub const MAX_SECONDS: f64 = 600.;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    /// Starts holding an input action (as named in the project's input actions).
    Press {
        at: f64,
        action: String,
    },
    Release {
        at: f64,
        action: String,
    },
    /// Analog movement intent: right and forward, -1 to 1.
    Movement {
        at: f64,
        value: [f32; 2],
    },
    /// Relative look displacement.
    Look {
        at: f64,
        value: [f32; 2],
    },
    /// Clicks an object or its interface button.
    Click {
        at: f64,
        entity: Id,
    },
}
impl InputEvent {
    fn at(&self) -> f64 {
        match self {
            Self::Press { at, .. }
            | Self::Release { at, .. }
            | Self::Movement { at, .. }
            | Self::Look { at, .. }
            | Self::Click { at, .. } => *at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SimulationRequest {
    /// Scene to start in; the project's start scene when absent.
    #[serde(default)]
    pub scene: Option<Id>,
    pub seconds: f64,
    #[serde(default)]
    pub inputs: Vec<InputEvent>,
    /// Objects whose world position, visibility and attributes are reported.
    #[serde(default)]
    pub watch: Vec<Id>,
    /// Evenly spaced samples of the watched objects, the last one being the final state
    /// (0 or 1 report only the final state; at most 120).
    #[serde(default)]
    pub samples: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Sample {
    pub time: f64,
    pub present: bool,
    pub position: Option<[f32; 3]>,
    pub visible: Option<bool>,
    pub attributes: Option<std::collections::BTreeMap<String, Value>>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Watched {
    pub id: Id,
    pub name: String,
    pub samples: Vec<Sample>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Event {
    pub time: f64,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SimulationReport {
    pub seconds: f64,
    pub start_scene: Id,
    pub final_scene: Id,
    pub scene_changes: Vec<Event>,
    pub logs: Vec<Event>,
    /// Logic actions that ran, as "Object · Action", the most recent 300.
    pub actions: Vec<Event>,
    pub sounds: Vec<Event>,
    pub watched: Vec<Watched>,
}

fn sample(runtime: &Runtime, id: &str) -> Sample {
    let scene = runtime.scene();
    match scene.entity(id) {
        Some(e) => Sample {
            time: runtime.time,
            present: true,
            position: scene
                .world_matrix(id)
                .ok()
                .map(|m| m.w_axis.truncate().to_array()),
            visible: Some(e.visible),
            attributes: Some(e.attributes.clone()),
        },
        None => Sample {
            time: runtime.time,
            present: false,
            position: None,
            visible: None,
            attributes: None,
        },
    }
}

/// Runs `request` on a copy of `project`. Nothing in `project` changes.
pub fn simulate(
    project: &Project,
    request: &SimulationRequest,
) -> Result<SimulationReport, String> {
    run(project, request).map(|(report, _)| report)
}

/// `simulate`, also returning the runtime at the end, e.g. to render the game view.
pub fn run(
    project: &Project,
    request: &SimulationRequest,
) -> Result<(SimulationReport, Runtime), String> {
    if !request.seconds.is_finite() || request.seconds <= 0. || request.seconds > MAX_SECONDS {
        return Err(format!(
            "Informe uma duração entre 0 e {MAX_SECONDS} segundos"
        ));
    }
    if request.samples > 120 {
        return Err("No máximo 120 amostras".into());
    }
    let start = request
        .scene
        .clone()
        .unwrap_or_else(|| project.start_scene.clone());
    if project.scene(&start).is_none() {
        return Err(format!("Cena {start} não existe"));
    }
    let mut events = request.inputs.clone();
    events.sort_by(|a, b| a.at().total_cmp(&b.at()));
    if events.iter().any(|e| !e.at().is_finite() || e.at() < 0.) {
        return Err("Os tempos das entradas precisam ser números a partir de 0".into());
    }
    let mut runtime = Runtime::new(project, &start)?;
    let mut clicks = Vec::new();
    for event in events {
        let change = match event {
            InputEvent::Press { at, action } => (at, InputChange::Press(action)),
            InputEvent::Release { at, action } => (at, InputChange::Release(action)),
            InputEvent::Movement { at, value } => (at, InputChange::Movement(value)),
            InputEvent::Look { at, value } => (at, InputChange::Look(value)),
            InputEvent::Click { at, entity } => {
                clicks.push((at, entity));
                continue;
            }
        };
        runtime.queue_timed_input(TimedInput {
            at: change.0,
            change: change.1,
        })?;
    }
    let names: Vec<(Id, String)> = request
        .watch
        .iter()
        .map(|id| {
            let name = project
                .scenes
                .iter()
                .find_map(|s| s.entity(id))
                .map(|e| e.name.clone())
                .ok_or_else(|| format!("Objeto observado {id} não existe no projeto"))?;
            Ok((id.clone(), name))
        })
        .collect::<Result<_, String>>()?;
    let mut report = SimulationReport {
        seconds: request.seconds,
        start_scene: start.clone(),
        final_scene: start,
        scene_changes: Vec::new(),
        logs: Vec::new(),
        actions: Vec::new(),
        sounds: Vec::new(),
        watched: names
            .iter()
            .map(|(id, name)| Watched {
                id: id.clone(),
                name: name.clone(),
                samples: Vec::new(),
            })
            .collect(),
    };
    let steps = (request.seconds / f64::from(FIXED_DT)).round().max(1.) as usize;
    let every = (request.samples > 0).then(|| (steps / request.samples as usize).max(1));
    let operations = graph::registry();
    let mut clicks = clicks.into_iter().peekable();
    let idle = InputFrame::default();
    for step in 1..=steps {
        while clicks.peek().is_some_and(|(at, _)| *at <= runtime.time) {
            let (_, entity) = clicks.next().unwrap();
            runtime.click(&entity);
        }
        let scene_before = runtime.scene_id().to_owned();
        runtime.advance(FIXED_DT, &idle);
        let time = runtime.time;
        if runtime.scene_id() != scene_before {
            let name = runtime.scene().name.clone();
            report.scene_changes.push(Event {
                time,
                text: format!("{} ({})", name, runtime.scene_id()),
            });
        }
        report.logs.extend(
            std::mem::take(&mut runtime.logs)
                .into_iter()
                .map(|text| Event { time, text }),
        );
        for trace in std::mem::take(&mut runtime.traces) {
            let object = runtime
                .scene()
                .entity(&trace.object)
                .map_or(trace.object.clone(), |e| e.name.clone());
            let action = operations
                .iter()
                .find(|op| op.id == trace.operation)
                .map_or(trace.operation.as_str(), |op| op.label);
            report.actions.push(Event {
                time: trace.time,
                text: format!("{object} · {action}"),
            });
        }
        for sound in std::mem::take(&mut runtime.sounds) {
            let name = project
                .asset(&sound.asset)
                .map_or(sound.asset.clone(), |a| a.name.clone());
            report.sounds.push(Event { time, text: name });
        }
        if every.is_some_and(|every| step % every == 0) && step != steps {
            for watched in &mut report.watched {
                watched.samples.push(sample(&runtime, &watched.id));
            }
        }
    }
    for watched in &mut report.watched {
        watched.samples.push(sample(&runtime, &watched.id));
    }
    if report.actions.len() > 300 {
        report.actions.drain(..report.actions.len() - 300);
    }
    report.final_scene = runtime.scene_id().to_owned();
    Ok((report, runtime))
}
