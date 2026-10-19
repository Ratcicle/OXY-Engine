//! Timers as engine data (design §4.4, decision Q1): `self:after`, `self:every`,
//! `self:sequence` and `self:schedule` queue tasks of the runtime that name a function of the
//! script's module and carry the values to pass it, and `self:cancel` drops what is left of
//! one, by the number the run gave it (from any script of the same object). Nothing waits
//! inside the VM, so a timer outlives a VM made anew; a function made on the spot has no name
//! to keep and is refused. Only on the object running, and only the values attributes hold
//! (and nil) as data. The queue has room for so many calls and so much text (see
//! `TimerRoom`): a timer past it is a limit, which stops the script on its object.
//! `self:after(seconds, Module.f, ...)` calls `Module.f(self, ...)` once, as Esperar sem bloquear
//! waits; `self:sequence(interval, Module.f, items)` calls it with the k-th item (k-1) ×
//! `interval` after it was asked, as `self:after` would: the first on the next step, the others
//! on the grid of a graph's waits. The four answer the number `self:cancel` takes.
use super::ScriptArg;
use super::handles::{self, MARK_OBJECT, decode};
use super::host::{Limit, Shared, api, locate};
use crate::document::{Id, Value};
use crate::runtime::{FIXED_DT, MAX_WAIT, Runtime};
use mlua::{Lua, MultiValue, Table, Value as LuaValue};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

/// A task that runs a function of a script for the task's owner.
#[derive(Clone, Debug)]
pub(crate) struct ScriptTask {
    pub script: Arc<str>,
    pub function: Cow<'static, str>,
    pub args: Vec<ScriptArg>,
    /// The timer that queued it; none for an event's.
    pub timer: Option<Timer>,
}

#[derive(Clone)]
pub(crate) struct Timer {
    /// What `self:cancel` takes: numbers given in order in the run.
    pub id: u64,
    pub then: Repeat,
    pub share: Share,
}

// The share is the room's count of what the timer holds, not state of its own.
impl std::fmt::Debug for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timer")
            .field("id", &self.id)
            .field("then", &self.then)
            .finish()
    }
}

/// The text of the data the queued timers of a run may hold at once, in bytes.
pub(crate) const ROOM_TEXT: usize = 4 * 1024 * 1024;

/// What the queued timers of a run hold beyond their own tasks: the calls still to come of
/// sequences and schedules, and the bytes of text in their data. Each timer task keeps its
/// part in a `Share` and gives it back when it drops, whichever way it leaves the queue (it
/// runs, is cancelled, or goes with its object, its script or the scene), so the count never
/// has to follow the queue's code.
#[derive(Debug, Default)]
pub(crate) struct TimerRoom {
    calls: Cell<usize>,
    text: Cell<usize>,
}

impl TimerRoom {
    /// The calls held beyond their tasks.
    pub(crate) fn calls(&self) -> usize {
        self.calls.get()
    }

    /// The bytes of text held.
    pub(crate) fn text(&self) -> usize {
        self.text.get()
    }
}

/// What one queued timer task holds of its run's `TimerRoom`: the calls after its own and the
/// text of its data and theirs.
pub(crate) struct Share {
    room: Rc<TimerRoom>,
    calls: usize,
    text: usize,
}

impl Share {
    pub(crate) fn new(room: &Rc<TimerRoom>, calls: usize, text: usize) -> Self {
        room.calls.set(room.calls.get() + calls);
        room.text.set(room.text.get() + text);
        Self {
            room: room.clone(),
            calls,
            text,
        }
    }

    /// The task's call ran and its next one holds the rest: `calls` fewer calls and `text`
    /// fewer bytes.
    pub(crate) fn give_back(&mut self, calls: usize, text: usize) {
        self.calls -= calls;
        self.text -= text;
        self.room.calls.set(self.room.calls.get() - calls);
        self.room.text.set(self.room.text.get() - text);
    }
}

impl Clone for Share {
    fn clone(&self) -> Self {
        Self::new(&self.room, self.calls, self.text)
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        self.give_back(self.calls, self.text);
    }
}

/// The bytes of text in `args`.
pub(crate) fn text_of(args: &[ScriptArg]) -> usize {
    args.iter()
        .map(|arg| match arg {
            ScriptArg::Value(Value::Text(text)) => text.len(),
            _ => 0,
        })
        .sum()
}

/// What follows a timer's call.
#[derive(Clone, Debug)]
pub(crate) enum Repeat {
    Once,
    /// `self:every`: the same call again after these seconds.
    Every(f64),
    /// `self:sequence` and `self:schedule`: the calls still to come.
    Steps(VecDeque<TimerStep>),
}

/// One call of a timer: `function` after `delay` seconds of game time, counted from the call
/// before or, for the first, from when it was asked.
#[derive(Clone, Debug)]
pub(crate) struct TimerStep {
    pub delay: f64,
    /// Shared by the calls of a sequence.
    pub function: Arc<str>,
    pub args: Vec<ScriptArg>,
}

/// The methods objects have for timers.
pub(crate) const METHODS: [&str; 5] = ["after", "every", "sequence", "schedule", "cancel"];

/// Calls of one sequence or schedule.
const MAX_STEPS: usize = 1024;
/// Values passed to one call.
const MAX_ARGS: usize = 16;
/// The longest text a timer carries, in bytes.
const MAX_TEXT: usize = 4096;

/// `seconds` as a timer waits them: at least one step and at most a day, as Esperar sem
/// bloquear.
pub(crate) fn delay(seconds: f64) -> Result<f64, String> {
    if !seconds.is_finite() || !(0.0..=MAX_WAIT).contains(&seconds) {
        return Err(format!(
            "o tempo do temporizador deve estar entre 0 e {MAX_WAIT} segundos"
        ));
    }
    Ok(seconds.max(f64::from(FIXED_DT)))
}

/// Puts the timer methods in the objects' table of methods.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>, methods: &Table) -> mlua::Result<()> {
    let s = shared.clone();
    methods.raw_set(
        "after",
        lua.create_function(
            move |lua, (me, seconds, f, data): (LuaValue, LuaValue, LuaValue, MultiValue)| {
                api(lua, &s, "after", |runtime| {
                    let (owner, script) = mine(&s, &me, "after")?;
                    let first = TimerStep {
                        delay: delay(seconds_of(&seconds, "after")?)?,
                        function: function_name(&s, &f, "after")?,
                        args: values(&s, &data)?,
                    };
                    queue(lua, &s, runtime, &owner, &script, Some(first), Repeat::Once)
                })
            },
        )?,
    )?;
    let s = shared.clone();
    methods.raw_set(
        "every",
        lua.create_function(
            move |lua, (me, seconds, f, data): (LuaValue, LuaValue, LuaValue, MultiValue)| {
                api(lua, &s, "every", |runtime| {
                    let (owner, script) = mine(&s, &me, "every")?;
                    let interval = delay(seconds_of(&seconds, "every")?)?;
                    let first = TimerStep {
                        delay: interval,
                        function: function_name(&s, &f, "every")?,
                        args: values(&s, &data)?,
                    };
                    let then = Repeat::Every(interval);
                    queue(lua, &s, runtime, &owner, &script, Some(first), then)
                })
            },
        )?,
    )?;
    let s = shared.clone();
    methods.raw_set(
        "sequence",
        lua.create_function(
            move |lua, (me, interval, f, items): (LuaValue, LuaValue, LuaValue, LuaValue)| {
                api(lua, &s, "sequence", |runtime| {
                    let (owner, script) = mine(&s, &me, "sequence")?;
                    let interval = delay(seconds_of(&interval, "sequence")?)?;
                    let function = function_name(&s, &f, "sequence")?;
                    let LuaValue::Table(items) = &items else {
                        return Err(
                            "sequence precisa de uma lista de itens, como { 3, 2, 1 }".into()
                        );
                    };
                    // The k-th item (k-1) × interval after now, as self:after would call it: the
                    // first on the next step, the second interval after now (one step less after
                    // the first), every other one interval after the one before.
                    let step = f64::from(FIXED_DT);
                    let mut steps = VecDeque::new();
                    for (k, item) in list(items, "sequence")?.into_iter().enumerate() {
                        steps.push_back(TimerStep {
                            delay: match k {
                                0 => step,
                                1 => interval - step,
                                _ => interval,
                            },
                            function: function.clone(),
                            args: vec![value(&s, &item)?],
                        });
                    }
                    let first = steps.pop_front();
                    let then = Repeat::Steps(steps);
                    queue(lua, &s, runtime, &owner, &script, first, then)
                })
            },
        )?,
    )?;
    let s = shared.clone();
    methods.raw_set(
        "schedule",
        lua.create_function(move |lua, (me, list_of): (LuaValue, LuaValue)| {
            api(lua, &s, "schedule", |runtime| {
                let (owner, script) = mine(&s, &me, "schedule")?;
                let shape = "schedule precisa de uma lista de passos { atraso, função, dado? }, \
                             como { {0.5, Modulo.abrir}, {1, Modulo.fechar} }";
                let LuaValue::Table(given) = &list_of else {
                    return Err(shape.into());
                };
                let mut steps = VecDeque::new();
                for (i, step) in list(given, "schedule")?.into_iter().enumerate() {
                    let LuaValue::Table(step) = step else {
                        return Err(format!("passo {}: {shape}", i + 1));
                    };
                    let get = |n: usize| step.raw_get::<LuaValue>(n).map_err(|e| e.to_string());
                    let data = get(3)?;
                    steps.push_back(TimerStep {
                        delay: delay(seconds_of(&get(1)?, "schedule")?)?,
                        function: function_name(&s, &get(2)?, "schedule")?,
                        args: match data {
                            LuaValue::Nil => Vec::new(),
                            data => vec![value(&s, &data)?],
                        },
                    });
                }
                let first = steps.pop_front();
                let then = Repeat::Steps(steps);
                queue(lua, &s, runtime, &owner, &script, first, then)
            })
        })?,
    )?;
    let s = shared.clone();
    methods.raw_set(
        "cancel",
        lua.create_function(move |lua, (me, id): (LuaValue, LuaValue)| {
            api(lua, &s, "cancel", |runtime| {
                let (owner, _) = mine(&s, &me, "cancel")?;
                let Some(id) = number(&id) else {
                    return Err(
                        "cancel precisa do número que after, every, sequence ou schedule \
                                devolveu"
                            .into(),
                    );
                };
                let given =
                    id.fract() == 0.0 && (1.0..=crate::random::MAX_SEED as f64).contains(&id);
                Ok(given && runtime.cancel_script_timer(&owner, id as u64))
            })
        })?,
    )?;
    Ok(())
}

/// Queues a timer for `owner`; its number, for `self:cancel`. With no room left in the queue,
/// the call went past a limit: the script stops on its object, as past any other, and nothing
/// more of the call reaches the world, whatever a `pcall` does with the error.
fn queue(
    lua: &Lua,
    s: &Shared,
    runtime: &mut Runtime,
    owner: &str,
    script: &Arc<str>,
    first: Option<TimerStep>,
    then: Repeat,
) -> Result<f64, String> {
    match runtime.add_script_timer(owner, script, first, then) {
        Some(id) => Ok(id as f64),
        None => {
            s.overran(Limit::Timers, locate(lua));
            Err(Limit::Timers.message(&s.limits.get()))
        }
    }
}

/// The object running and its script, when `me` is that object.
fn mine(s: &Shared, me: &LuaValue, method: &str) -> Result<(Id, Arc<str>), String> {
    let own = match me {
        LuaValue::LightUserData(handle) => {
            let (mark, key) = decode(handle.0);
            mark == MARK_OBJECT && Some(key) == s.caller_key()
        }
        _ => false,
    };
    if !own {
        return Err(format!(
            "temporizadores são do próprio objeto: use self:{method}(...)"
        ));
    }
    let owner = s.caller().ok_or("o objeto não existe mais")?;
    let script = s
        .module()
        .ok_or("temporizador fora de uma chamada de script")?;
    Ok((owner, script.path.clone()))
}

/// A number as mlua gives it: whole ones come as `Integer`.
fn number(value: &LuaValue) -> Option<f64> {
    match value {
        LuaValue::Number(n) => Some(*n),
        LuaValue::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

fn seconds_of(value: &LuaValue, method: &str) -> Result<f64, String> {
    number(value).ok_or_else(|| format!("{method} precisa do tempo em segundos (um número)"))
}

/// The name of `f` in the running script's module.
fn function_name(s: &Shared, f: &LuaValue, method: &str) -> Result<Arc<str>, String> {
    let hint = "passe uma função da tabela do módulo, como Modulo.contar";
    let LuaValue::Function(f) = f else {
        return Err(format!("{method} precisa de uma função: {hint}"));
    };
    let module = s
        .module()
        .ok_or("temporizador fora de uma chamada de script")?;
    module
        .names
        .get(&(f.to_pointer() as usize))
        .cloned()
        .ok_or_else(|| {
            format!(
                "{hint}, não uma criada aqui: o temporizador guarda só o nome da função, e uma \
                 função criada na hora não tem nome no módulo"
            )
        })
}

/// The items of a list, at most [`MAX_STEPS`].
fn list(table: &Table, method: &str) -> Result<Vec<LuaValue>, String> {
    let n = table.raw_len();
    if n > MAX_STEPS {
        return Err(format!(
            "{method} aceita até {MAX_STEPS} passos; recebeu {n}"
        ));
    }
    (1..=n)
        .map(|i| table.raw_get::<LuaValue>(i).map_err(|e| e.to_string()))
        .collect()
}

fn values(s: &Shared, data: &MultiValue) -> Result<Vec<ScriptArg>, String> {
    if data.len() > MAX_ARGS {
        return Err(format!(
            "um temporizador passa até {MAX_ARGS} valores; recebeu {}",
            data.len()
        ));
    }
    data.iter().map(|v| value(s, v)).collect()
}

/// A value a timer may carry: nil or one an attribute could hold.
fn value(s: &Shared, value: &LuaValue) -> Result<ScriptArg, String> {
    let refused = |kind: &str| {
        Err(format!(
            "{kind} não pode ser dado de temporizador; passe nil, números, textos, booleanos, \
             vetores ou objetos"
        ))
    };
    Ok(ScriptArg::Value(match value {
        LuaValue::Nil => return Ok(ScriptArg::Nil),
        LuaValue::Boolean(b) => Value::Bool(*b),
        LuaValue::Number(_) | LuaValue::Integer(_) => match number(value) {
            Some(n) if n.is_finite() => Value::Number(n),
            _ => return refused("infinito ou NaN"),
        },
        LuaValue::Vector(v) if [v.x(), v.y(), v.z()].iter().all(|c| c.is_finite()) => {
            Value::Vector3([v.x(), v.y(), v.z()])
        }
        LuaValue::Vector(_) => return refused("vetor com infinito ou NaN"),
        LuaValue::String(text) => {
            let text = text
                .to_str()
                .map_err(|_| "texto que não é UTF-8 não pode ser dado de temporizador")?;
            if text.len() > MAX_TEXT {
                return Err(format!(
                    "texto de mais de {MAX_TEXT} bytes não pode ser dado de temporizador"
                ));
            }
            Value::Text(text.to_owned())
        }
        LuaValue::LightUserData(handle) => match decode(handle.0) {
            (MARK_OBJECT, key) => Value::Object(Some(s.id(key).ok_or("o objeto não existe mais")?)),
            (mark, _) => return refused(handles::mark_kind(mark)),
        },
        LuaValue::Table(_) => return refused("tabela"),
        LuaValue::Function(_) => return refused("função"),
        LuaValue::Thread(_) => return refused("corrotina"),
        LuaValue::Buffer(_) => return refused("buffer"),
        _ => return refused("userdata"),
    }))
}
