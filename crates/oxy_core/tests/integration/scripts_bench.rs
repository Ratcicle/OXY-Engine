//! The parkour's logic of checkpoints and runners in Luau, with 8 runners, on the runtime's
//! real path (events queued by `emit`, tasks, the function looked up by name, the interrupt net
//! on), against the ceiling of 50 µs of scripts per step of the 0.7 design (§2.4, §11); also
//! with the runners' logic run by a timer of each instead of on_step (the waiting queue swept,
//! the next call queued before each one); and what script memory costs by the size of what a
//! step converts, which calibrates the warning of `script_costs`. The measures run in release:
//! `./scripts/cargo.ps1 test -p oxy_core --release --test integration scripts_bench:: '--'
//! --ignored --nocapture`.
use oxy_core::{
    document::*,
    runtime::{
        FIXED_DT, InputFrame, Runtime, RuntimeEvent, RuntimeOptions, SCRIPT_ENTRIES_WARNING,
    },
    script::{ScriptRef, ScriptSources},
};
use std::sync::Arc;
use std::time::Instant;

/// The runner of the 0.6 parkour: held until the start, then its clock and the clock's text.
/// Past the last checkpoint the course starts again, so that every step takes the full path.
const RUNNER: &str = r#"--!strict
local Runner = {}

function Runner.on_step(self: Object, dt: number)
    local stage = self.attr.Etapa
    if stage == 0 then
        if self.attr["Largada liberada"] then
            self.attr.Etapa = 1
        end
    elseif stage == 1 then
        local t = self.attr.Tempo + dt
        self.attr.Tempo = t
        self.attr["Tempo texto"] = string.format("%02d:%05.2f", t // 60, t % 60)
        if self.attr["Checkpoint atual"] >= 6 then
            self.attr["Checkpoint atual"] = 0
        end
    elseif stage == 2 then
        Runner.finish(self)
    end
end

function Runner.finish(self: Object)
    self.attr.Etapa = 3
end

return Runner
"#;

/// The checkpoint of the 0.6 parkour (design §5.8): a runner that reaches a farther one keeps
/// its order and its point of return.
const CHECKPOINT: &str = r#"--!strict
local Checkpoint = {}

function Checkpoint.on_area_enter(self: Object, other: Object)
    if not other:has_attribute("Checkpoint atual") then
        return
    end
    local order = self.attr.Ordem
    if other.attr["Checkpoint atual"] < order then
        other.attr["Ponto de retorno"] = self.attr["Ponto de retorno"]
        other.attr["Checkpoint atual"] = order
    end
end

return Checkpoint
"#;

/// The runner with its logic run by a timer every step instead of on_step.
fn timed_runner() -> String {
    RUNNER.replace(
        "function Runner.on_step(self: Object, dt: number)\n",
        "function Runner.on_start(self: Object)\n    self:every(1 / 60, Runner.tick)\nend\n\n\
         function Runner.tick(self: Object)\n    local dt = 1 / 60\n",
    )
}

const RUNNERS: usize = 8;
const CHECKPOINTS: usize = 6;

/// How the runners run their logic.
#[derive(Clone, Copy, PartialEq)]
enum Logic {
    /// No scripts, on the checkpoints either.
    None,
    Step,
    Timer,
}

/// 8 runners and 6 checkpoints, with their scripts or without any.
fn course(logic: Logic) -> (Project, Vec<Id>, Vec<Id>) {
    let scripted = logic != Logic::None;
    let mut project = Project::new("Parkour em Luau");
    let scene = &mut project.scenes[0];
    let mut runners = Vec::new();
    for i in 1..=RUNNERS {
        let mut e = Entity::new(format!("Jogador {i}"), None);
        e.id = format!("corredor-{i}");
        for (name, value) in [
            ("Etapa", Value::Number(0.)),
            ("Largada liberada", Value::Bool(true)),
            ("Tempo", Value::Number(0.)),
            ("Tempo texto", Value::Text(String::new())),
            ("Checkpoint atual", Value::Number(0.)),
            ("Ponto de retorno", Value::Vector3([0.; 3])),
        ] {
            e.attributes.insert(name.into(), value);
        }
        match logic {
            Logic::None => {}
            Logic::Step => e.scripts = vec![ScriptRef::new("scripts/corredor.luau")],
            Logic::Timer => e.scripts = vec![ScriptRef::new("scripts/corredor_temporizado.luau")],
        }
        runners.push(e.id.clone());
        scene.entities.push(e);
    }
    let mut checkpoints = Vec::new();
    for i in 1..=CHECKPOINTS {
        let mut e = Entity::new(format!("Checkpoint {i}"), None);
        e.id = format!("checkpoint-{i}");
        e.attributes.insert("Ordem".into(), Value::Number(i as f64));
        e.attributes.insert(
            "Ponto de retorno".into(),
            Value::Vector3([0., 0., -10. * i as f32]),
        );
        if scripted {
            e.scripts = vec![ScriptRef::new("scripts/checkpoint.luau")];
        }
        checkpoints.push(e.id.clone());
        scene.entities.push(e);
    }
    (project, runners, checkpoints)
}

struct Course {
    rt: Runtime,
    runners: Vec<Id>,
    checkpoints: Vec<Id>,
    steps: usize,
}

impl Course {
    fn new(logic: Logic) -> Self {
        let (project, runners, checkpoints) = course(logic);
        let timed = timed_runner();
        let options = RuntimeOptions {
            scripts: Arc::new(ScriptSources::from_texts([
                ("scripts/corredor.luau", RUNNER),
                ("scripts/corredor_temporizado.luau", &timed),
                ("scripts/checkpoint.luau", CHECKPOINT),
            ])),
            ..Default::default()
        };
        let rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
        assert_eq!(rt.has_script_host(), logic != Logic::None);
        Self {
            rt,
            runners,
            checkpoints,
            steps: 0,
        }
    }

    /// One step; with `entering`, every runner first enters the next checkpoint, as the area
    /// detection would queue it.
    fn step(&mut self, entering: bool) {
        if entering {
            let area = &self.checkpoints[self.steps % CHECKPOINTS];
            for runner in &self.runners {
                self.rt.emit(RuntimeEvent::AreaEnter {
                    area: area.clone(),
                    other: runner.clone(),
                    activation: 0,
                });
            }
        }
        self.rt.advance(FIXED_DT, &InputFrame::default());
        self.steps += 1;
    }

    fn attribute(&self, runner: usize, name: &str) -> Value {
        self.rt
            .scene()
            .entity(&self.runners[runner])
            .unwrap()
            .attributes[name]
            .clone()
    }
}

/// The median time of a step, in µs, over `samples` steps after a warm-up.
fn median_step(course: &mut Course, entering: bool, samples: usize) -> f64 {
    for _ in 0..samples / 10 {
        course.step(entering);
    }
    let mut times: Vec<f64> = (0..samples)
        .map(|_| {
            let started = Instant::now();
            course.step(entering);
            started.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

#[test]
fn the_parkour_logic_in_luau_takes_the_full_path_every_step() {
    for logic in [Logic::Step, Logic::Timer] {
        let mut course = Course::new(logic);
        for _ in 0..12 {
            course.step(true);
        }
        assert_eq!(course.rt.script_errors().count(), 0, "{:?}", course.rt.logs);
        for runner in 0..RUNNERS {
            assert_eq!(course.attribute(runner, "Etapa"), Value::Number(1.));
            // Each step entered the next checkpoint (the twelfth is the sixth, and the course
            // started again after it).
            assert_eq!(
                course.attribute(runner, "Checkpoint atual"),
                Value::Number(0.)
            );
            assert_eq!(
                course.attribute(runner, "Ponto de retorno"),
                Value::Vector3([0., 0., -60.])
            );
            // The timer ran the logic once a step, as on_step does.
            assert_eq!(
                course.attribute(runner, "Tempo texto"),
                Value::Text("00:00.18".into())
            );
        }
        course.step(true);
        assert_eq!(course.attribute(0, "Checkpoint atual"), Value::Number(1.));
        let timers = if logic == Logic::Timer { RUNNERS } else { 0 };
        assert_eq!(course.rt.pending_tasks(), timers);
    }
}

#[test]
#[ignore = "medida de desempenho: rode em release (veja o topo do arquivo)"]
fn the_parkour_logic_in_luau_with_8_runners_fits_in_50_us_per_step() {
    const SAMPLES: usize = 3000;
    // The same steps without scripts: what the runtime spends besides them.
    let idle = median_step(&mut Course::new(Logic::None), false, SAMPLES);
    let idle_entering = median_step(&mut Course::new(Logic::None), true, SAMPLES);
    let running = median_step(&mut Course::new(Logic::Step), false, SAMPLES) - idle;
    let entering = median_step(&mut Course::new(Logic::Step), true, SAMPLES) - idle_entering;
    let timed = median_step(&mut Course::new(Logic::Timer), false, SAMPLES) - idle;
    let timed_entering = median_step(&mut Course::new(Logic::Timer), true, SAMPLES) - idle_entering;
    println!(
        "passo sem scripts: {idle:.2} µs ({idle_entering:.2} µs com as entradas); scripts por \
         passo, 8 corredores: on_step {running:.2} µs, on_step + on_area_enter com o caminho \
         cheio {entering:.2} µs; com um temporizador every por corredor no lugar do on_step: \
         {timed:.2} µs, {timed_entering:.2} µs com as entradas (teto 50 µs)"
    );
    assert!(entering < 50., "{entering:.2} µs por passo");
    assert!(timed_entering < 50., "{timed_entering:.2} µs por passo");
}

/// A list of `Quantos` records of 3 fields in memory; each step reads a scalar (`n`) or
/// touches one record of the list, which takes the whole list in and back out.
const LIST: &str = r#"local M = {}
M.state = { n = 0, items = {} }
function M.on_start(self)
    local items = {}
    for i = 1, self.attr.Quantos do
        items[i] = { hp = 100, name = "inimigo " .. i, pos = vector.create(i, 0, 0) }
    end
    self.state.items = items
end
function M.on_step(self)
    if self.attr.Tocar then
        self.state.items[5].hp -= 1
    else
        self.state.n += 1
    end
end
return M
"#;

/// The median µs of scripts in a step and the table entries they converted in it.
fn list_step(records: usize, touch: bool) -> (f64, u64) {
    let mut e = Entity::new("Ondas", None);
    e.attributes
        .insert("Quantos".into(), Value::Number(records as f64));
    e.attributes.insert("Tocar".into(), Value::Bool(touch));
    e.scripts = vec![ScriptRef::new("scripts/lista.luau")];
    let mut project = Project::new("Memória");
    project.scenes[0].entities.push(e);
    let options = RuntimeOptions {
        scripts: Arc::new(ScriptSources::from_texts([("scripts/lista.luau", LIST)])),
        ..Default::default()
    };
    let rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    let mut course = Course {
        rt,
        runners: Vec::new(),
        checkpoints: Vec::new(),
        steps: 0,
    };
    let time = median_step(&mut course, false, 300);
    assert_eq!(course.rt.script_errors().count(), 0, "{:?}", course.rt.logs);
    let cost = &course.rt.script_costs()[0];
    (time, cost.most_entries)
}

#[test]
#[ignore = "medida de desempenho: rode em release (veja o topo do arquivo)"]
fn script_memory_costs_what_a_step_converts_which_calibrates_the_warning() {
    let mut per_entry = Vec::new();
    for records in [10, 100, 1000] {
        // The scene start builds the list once; a step touching it converts it twice.
        let (scalar, _) = list_step(records, false);
        let (touched, entries) = list_step(records, true);
        let each = (touched - scalar) / entries as f64;
        per_entry.push(each);
        println!(
            "{records} registros: escalar {scalar:.2} µs por passo; um registro tocado \
             {touched:.2} µs, {entries} entradas convertidas, {:.3} µs por entrada",
            each
        );
    }
    // At the warning, the conversions alone cost about the 50 µs of the parkour's 8 runners.
    let each = per_entry.iter().copied().fold(0., f64::max);
    let warned = each * SCRIPT_ENTRIES_WARNING as f64;
    println!(
        "aviso a partir de {SCRIPT_ENTRIES_WARNING} entradas por passo: ~{warned:.1} µs (teto do \
         parkour com 8 corredores: 50 µs)"
    );
    assert!((25. ..100.).contains(&warned), "{warned:.1} µs");
}
