//! The Luau host: one VM per `Runtime`, created only when the scene attaches a script, holding
//! the compiled modules and nothing of the game. Calls lend it the `Runtime` (see the module
//! docs of [`crate::script`] for the borrow and the C++ exception rules this file keeps).
use super::errors::{ScriptError, ScriptErrorKind, describe};
use super::handles::{self, MARK_OBJECT};
use super::memory::{self, Memory};
use super::{Callbacks, FOLDER, ScriptArg, ScriptSources, load, messages, sandbox};
use crate::document::{Id, Value};
use crate::runtime::{Runtime, ScriptLocation};
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, MultiValue, Table, Thread, Value as LuaValue};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::CStr;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

/// What a script call may spend (design §3.11). Units are deterministic: one for each handler
/// called and one for each time a script reaches the world through the engine. Interrupts are
/// Luau's own checkpoints (calls, returns and loop turns), a safety net against endless loops
/// whose last units vary with the collector, so they decide nothing in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Units one call may spend; past them the call fails and the script stops on its object.
    pub call_units: u64,
    /// Units the step's calls may spend, checked between calls: the calls left go to the next
    /// step, and a call is never cut by what ran before it.
    pub step_units: u64,
    /// Interrupts of one call (~20 ms on the development PC).
    pub call_interrupts: u64,
    /// Interrupts of the step's calls (~50 ms): past them the call in progress fails.
    pub step_interrupts: u64,
    /// Bytes the VM may hold.
    pub memory: usize,
}

pub(crate) const LIMITS: Limits = Limits {
    call_units: 20_000,
    step_units: 100_000,
    call_interrupts: 2_000_000,
    step_interrupts: 5_000_000,
    memory: 64 * 1024 * 1024,
};

/// Which limit a call went past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Limit {
    CallUnits,
    CallWork,
    StepWork,
    Memory,
    /// The queue had no room for another timer (see `timers::TimerRoom`).
    Timers,
    /// The memory of the object went past its bytes (see `memory`).
    ScriptMemory,
    /// The script sent too many messages from its object in the step (see `messages`).
    Messages,
}

impl Limit {
    pub(crate) fn message(self, limits: &Limits) -> String {
        match self {
            Self::CallUnits => format!("Limite de {} operações por chamada", limits.call_units),
            Self::CallWork => "o script trabalhou demais numa chamada (um laço sem fim?)".into(),
            Self::StepWork => "os scripts trabalharam demais neste passo (um laço sem fim?)".into(),
            Self::Memory => format!(
                "os scripts passaram do limite de memória ({} MB)",
                limits.memory / (1024 * 1024)
            ),
            Self::Timers => format!(
                "a fila não tem lugar para mais temporizadores (até {} chamadas e {} MB de texto \
                 guardados)",
                crate::runtime::MAX_TASKS,
                super::timers::ROOM_TEXT / (1024 * 1024)
            ),
            Self::ScriptMemory => format!(
                "a memória do script passou de {} KB neste objeto",
                memory::MAX_BYTES / 1024
            ),
            Self::Messages => format!(
                "o script mandou mais de {} mensagens deste objeto neste passo (objetos \
                 respondendo uns aos outros sem parar?)",
                messages::MAX_MESSAGES
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// No script runs: functions that reach the world refuse.
    Idle,
    /// A module's top runs: it may define, not touch the world (R2).
    Loading,
    /// A handler runs with the `Runtime` lent.
    Running,
}

/// A module as loaded: its frozen table, its functions by name and what of them the engine
/// calls by itself.
pub(crate) struct Module {
    /// The script's path, shared by its tasks.
    pub path: Arc<str>,
    pub table: Table,
    pub functions: BTreeMap<String, Function>,
    /// The name of each function by its address, which a timer keeps in its place (the first
    /// name in order when two hold the same function).
    pub names: HashMap<usize, Arc<str>>,
    pub callbacks: Callbacks,
    /// Functions named `on_*` that hear nothing, with their line and the callback meant.
    pub unknown: Vec<(String, u32, Option<&'static str>)>,
    /// What `Module.state` declares; none when the script keeps no memory.
    pub state: Option<Memory>,
}

impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// What the host's functions share, captured by each of them as an `Rc` (not through
/// `Lua::app_data`, a type map lookup per call).
pub(crate) struct Shared {
    runtime: Cell<*mut Runtime>,
    pub phase: Cell<Phase>,
    /// The handle key of the object running, while a call runs.
    caller: Cell<Option<u64>>,
    /// The module whose function runs, while a call runs.
    module: RefCell<Option<Rc<Module>>>,
    pub limits: Cell<Limits>,
    units: Cell<u64>,
    step_units: Cell<u64>,
    interrupts: Cell<u64>,
    step_interrupts: Cell<u64>,
    /// The first limit the call in progress went past, with where it was.
    overrun: RefCell<Option<(Limit, Option<ScriptLocation>)>>,
    /// A panic an API function's door caught in this call: the engine failed, whatever the
    /// script did with the error.
    poisoned: RefCell<Option<String>>,
    /// Handle keys to object IDs (see `handles`).
    pub handles: RefCell<HashMap<u64, Id>>,
    pub sources: Arc<ScriptSources>,
    pub modules: RefCell<BTreeMap<String, Result<Rc<Module>, ScriptError>>>,
    /// The modules whose top is running, outermost first (a require inside a top).
    pub loading: RefCell<Vec<String>>,
    /// By chunk name, the functions whose closures share a mutable upvalue (§3.5).
    pub stateful: RefCell<HashMap<String, BTreeSet<u32>>>,
    /// The host functions a module may keep: those the base reaches.
    pub known_functions: RefCell<HashSet<usize>>,
    /// Coroutines created in the call (or the top) in progress (R3).
    pub threads: RefCell<Vec<Thread>>,
    /// Whether the call in progress touched a table of script memory (see `handles`).
    pub touched: Cell<bool>,
    /// Table entries converted between Luau and the engine since the host last counted them.
    converted: Cell<u64>,
    /// By script, the scripts its top required, as they loaded.
    pub requires: RefCell<BTreeMap<String, BTreeSet<String>>>,
}

impl Shared {
    fn new(sources: Arc<ScriptSources>) -> Self {
        Self {
            runtime: Cell::new(std::ptr::null_mut()),
            phase: Cell::new(Phase::Idle),
            caller: Cell::new(None),
            module: RefCell::default(),
            limits: Cell::new(LIMITS),
            units: Cell::new(0),
            step_units: Cell::new(0),
            interrupts: Cell::new(0),
            step_interrupts: Cell::new(0),
            overrun: RefCell::default(),
            poisoned: RefCell::default(),
            handles: RefCell::default(),
            sources,
            modules: RefCell::default(),
            loading: RefCell::default(),
            stateful: RefCell::default(),
            known_functions: RefCell::default(),
            threads: RefCell::default(),
            touched: Cell::new(false),
            converted: Cell::new(0),
            requires: RefCell::default(),
        }
    }

    /// Counts table entries converted between Luau and the engine.
    pub(crate) fn count_converted(&self, entries: u64) {
        self.converted.set(self.converted.get() + entries);
    }

    /// Lends `runtime` for one call: the guard keeps the `&mut` borrowed until it drops, so the
    /// caller cannot touch the `Runtime` while the VM may reach it.
    fn enter<'a>(
        &'a self,
        runtime: &'a mut Runtime,
        owner: u64,
        module: &Rc<Module>,
    ) -> CallGuard<'a> {
        assert!(
            self.runtime.get().is_null() && self.phase.get() == Phase::Idle,
            "chamada de script dentro de outra"
        );
        self.runtime.set(runtime);
        self.phase.set(Phase::Running);
        self.caller.set(Some(owner));
        *self.module.borrow_mut() = Some(module.clone());
        self.start_count();
        self.units.set(1);
        CallGuard {
            shared: self,
            _runtime: PhantomData,
        }
    }

    pub(crate) fn start_count(&self) {
        self.units.set(0);
        self.interrupts.set(0);
        self.overrun.take();
        self.threads.borrow_mut().clear();
    }

    /// The object running, while a call runs.
    pub(crate) fn caller(&self) -> Option<Id> {
        self.caller.get().and_then(|key| self.id(key))
    }

    /// The handle key of the object running, while a call runs.
    pub(crate) fn caller_key(&self) -> Option<u64> {
        self.caller.get()
    }

    /// The module whose function runs, while a call runs.
    pub(crate) fn module(&self) -> Option<Rc<Module>> {
        self.module.borrow().clone()
    }

    pub(crate) fn id(&self, key: u64) -> Option<Id> {
        self.handles.borrow().get(&key).cloned()
    }

    /// The handle key of `id`, registering it: an error names the other object with the key.
    pub(crate) fn register(&self, id: &str) -> Result<u64, Id> {
        let key = handles::key(id);
        let mut handles = self.handles.borrow_mut();
        match handles.get(&key) {
            Some(other) if other != id => Err(other.clone()),
            Some(_) => Ok(key),
            None => {
                handles.insert(key, id.to_owned());
                Ok(key)
            }
        }
    }

    pub(crate) fn has_overrun(&self) -> bool {
        self.overrun.borrow().is_some()
    }

    pub(crate) fn overran(&self, limit: Limit, at: Option<ScriptLocation>) {
        let mut overrun = self.overrun.borrow_mut();
        if overrun.is_none() {
            *overrun = Some((limit, at));
        }
    }

    /// Counts an interrupt of the call in progress; an error once past a limit, and at every
    /// interrupt after it, so a `pcall` cannot get past it.
    pub(crate) fn interrupt(&self) -> Option<Limit> {
        if self.phase.get() == Phase::Idle {
            return None;
        }
        let limits = self.limits.get();
        self.interrupts.set(self.interrupts.get() + 1);
        self.step_interrupts.set(self.step_interrupts.get() + 1);
        if self.interrupts.get() > limits.call_interrupts {
            Some(Limit::CallWork)
        } else if self.step_interrupts.get() > limits.step_interrupts {
            Some(Limit::StepWork)
        } else {
            None
        }
    }

    pub(crate) fn poison(&self, place: &str, panic: &(dyn std::any::Any + Send)) {
        let message = panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "pânico".into());
        self.poisoned
            .borrow_mut()
            .get_or_insert_with(|| format!("{place}: {message}"));
    }

    /// The step's work so far, deciding whether another call starts in it.
    fn step_spent(&self) -> bool {
        let limits = self.limits.get();
        self.step_units.get() >= limits.step_units
            || self.step_interrupts.get() >= limits.step_interrupts
    }
}

/// Holds the `Runtime` borrowed while the VM may reach it; clears the pointer on drop, in the
/// unwinding of a panic too.
struct CallGuard<'a> {
    shared: &'a Shared,
    _runtime: PhantomData<&'a mut Runtime>,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        self.shared.runtime.set(std::ptr::null_mut());
        self.shared.phase.set(Phase::Idle);
        self.shared.caller.set(None);
        self.shared.module.take();
    }
}

/// The door of every function that reaches the world: the phase, the lent `Runtime` and one
/// unit. Refuses at a module's top (R2) and outside a call.
#[allow(
    clippy::mut_from_ref,
    reason = "the Runtime is lent by the call's guard, not owned by `shared`"
)]
pub(crate) fn door(shared: &Shared) -> Result<&mut Runtime, String> {
    match shared.phase.get() {
        Phase::Running => {}
        Phase::Loading => {
            return Err("o topo do script não pode mexer no mundo; faça isso em on_start".into());
        }
        Phase::Idle => return Err("função da API chamada fora de uma chamada de script".into()),
    }
    let pointer = shared.runtime.get();
    if pointer.is_null() {
        return Err("função da API chamada fora de uma chamada de script".into());
    }
    // Past a limit the call reaches nothing more, even after a pcall caught the error.
    let limits = shared.limits.get();
    if let Some((limit, _)) = &*shared.overrun.borrow() {
        return Err(limit.message(&limits));
    }
    let units = shared.units.get() + 1;
    shared.units.set(units);
    if units > limits.call_units {
        shared.overran(Limit::CallUnits, None);
        return Err(Limit::CallUnits.message(&limits));
    }
    // SAFETY: the guard set the pointer from a `&mut Runtime` it keeps borrowed until the call
    // ends, the host is out of the `Runtime` meanwhile, and no API function calls back into
    // Luau or holds this reference across another door: it is the only live one.
    Ok(unsafe { &mut *pointer })
}

/// The lent `Runtime` for reading names, without a unit; none outside a call.
pub(crate) fn peek(shared: &Shared) -> Option<&Runtime> {
    let pointer = shared.runtime.get();
    // SAFETY: as in `door`; the reference is read and dropped before any other door.
    (shared.phase.get() == Phase::Running && !pointer.is_null()).then(|| unsafe { &*pointer })
}

/// An API function written with mlua: the door, then `f` behind a panic barrier that leaves a
/// mark the host reads after the call. `f` uses only mlua's safe API (which never lets a C++
/// exception through a Rust frame); its error is raised at the script line that called it.
pub(crate) fn api<R>(
    lua: &Lua,
    shared: &Shared,
    name: &str,
    f: impl FnOnce(&mut Runtime) -> Result<R, String>,
) -> mlua::Result<R> {
    let runtime = match door(shared) {
        Ok(runtime) => runtime,
        Err(e) => {
            shared.place_overrun(locate(lua));
            return Err(located(lua, e));
        }
    };
    match catch_unwind(AssertUnwindSafe(|| f(runtime))) {
        Ok(result) => result.map_err(|e| located(lua, e)),
        Err(panic) => {
            shared.poison(name, &*panic);
            Err(located(lua, format!("erro interno da engine em {name}")))
        }
    }
}

/// A Lua error at the script line running, found from an mlua function.
pub(crate) fn located(lua: &Lua, message: String) -> mlua::Error {
    mlua::Error::runtime(match locate(lua) {
        Some(at) => format!("{}:{}: {message}", at.path, at.line),
        None => message,
    })
}

/// The innermost script line on the stack, seen from an mlua function.
pub(crate) fn locate(lua: &Lua) -> Option<ScriptLocation> {
    (0..32)
        .map_while(|level| {
            lua.inspect_stack(level, |d| {
                let path = d.source().source.and_then(|s| script_path(&s));
                path.zip(d.current_line())
            })
        })
        .flatten()
        .find(|(_, line)| *line > 0)
        .map(|(path, line)| ScriptLocation {
            path,
            line: line as u32,
        })
}

/// The innermost script line on the stack of `l`, seen from a C function.
///
/// # Safety
/// `l` is a live state; only `lua_getinfo` is called, which raises nothing.
pub(crate) unsafe fn locate_raw(l: *mut mlua::ffi::lua_State) -> Option<ScriptLocation> {
    for level in 0..32 {
        // SAFETY: zeroed is a valid lua_Debug; getinfo fills it for a live level.
        let mut ar: mlua::ffi::lua_Debug = unsafe { std::mem::zeroed() };
        if unsafe { mlua::ffi::lua_getinfo(l, level, c"sl".as_ptr(), &mut ar) } == 0 {
            break;
        }
        if ar.source.is_null() || ar.currentline <= 0 {
            continue;
        }
        // SAFETY: getinfo points `source` at the function's NUL-terminated chunk name.
        let source = unsafe { CStr::from_ptr(ar.source) }.to_string_lossy();
        if let Some(path) = script_path(&source) {
            return Some(ScriptLocation {
                path,
                line: ar.currentline as u32,
            });
        }
    }
    None
}

/// `@scripts/porta.luau` (a chunk name) → `scripts/porta.luau`.
pub(crate) fn script_path(source: &str) -> Option<String> {
    source
        .strip_prefix('@')
        .filter(|path| path.starts_with(&format!("{FOLDER}/")))
        .map(str::to_owned)
}

/// How a call went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptCall {
    /// It ran to its end.
    Done,
    /// The step's work for scripts ran out before it: it goes to the next step.
    Deferred,
    /// It did not run: the script did not load, or it stopped on this object.
    Skipped,
    /// It stopped at an error; what it did before stays done.
    Failed(ScriptError),
}

pub(crate) struct ScriptHost {
    // Declared first so the VM closes before `shared` (whose pointer C functions keep) drops.
    lua: Lua,
    shared: Rc<Shared>,
    /// The tables of script memory the call in progress touched, by field (see `handles`).
    touched: Table,
}

/// Adds functions to the base before it turns read-only (tests).
pub(crate) type Extra<'a> = &'a dyn Fn(&Lua, &Table, &Rc<Shared>) -> mlua::Result<()>;

impl ScriptHost {
    pub(crate) fn new(sources: Arc<ScriptSources>) -> Result<Self, String> {
        Self::build(sources, &|_, _, _| Ok(()))
    }

    #[cfg(test)]
    pub(crate) fn with_extra(sources: Arc<ScriptSources>, extra: Extra) -> Result<Self, String> {
        Self::build(sources, extra)
    }

    fn build(sources: Arc<ScriptSources>, extra: Extra) -> Result<Self, String> {
        let shared = Rc::new(Shared::new(sources));
        let (lua, touched) = sandbox::create(&shared, extra)
            .map_err(|e| format!("não foi possível iniciar os scripts: {e}"))?;
        Ok(Self {
            lua,
            shared,
            touched,
        })
    }

    #[cfg(test)]
    pub(crate) fn lua(&self) -> &Lua {
        &self.lua
    }

    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Rc<Shared> {
        &self.shared
    }

    #[cfg(test)]
    pub(crate) fn set_limits(&self, limits: Limits) {
        self.shared.limits.set(limits);
        let _ = self.lua.set_memory_limit(limits.memory);
    }

    /// A step starts: its work budget for scripts is full again.
    pub(crate) fn begin_step(&self) {
        self.shared.step_units.set(0);
        self.shared.step_interrupts.set(0);
    }

    /// Whether the step's work for scripts ran out: no other call starts in it.
    pub(crate) fn step_spent(&self) -> bool {
        self.shared.step_spent()
    }

    /// The module `path`, loading it unless it already was (its error too).
    pub(crate) fn load(&self, path: &str) -> Result<Rc<Module>, ScriptError> {
        load::module(&self.lua, &self.shared, path)
    }

    /// The module `path` as it loaded, without loading it; none if it never did.
    pub(crate) fn loaded(&self, path: &str) -> Option<Result<Rc<Module>, ScriptError>> {
        self.shared.modules.borrow().get(path).cloned()
    }

    /// The paths of the modules loaded so far, those that failed included.
    pub(crate) fn loaded_paths(&self) -> Vec<String> {
        self.shared.modules.borrow().keys().cloned().collect()
    }

    /// The modules that did not load, by path, with why.
    pub(crate) fn failed_modules(&self) -> Vec<(String, ScriptError)> {
        self.shared
            .modules
            .borrow()
            .iter()
            .filter_map(|(path, loaded)| Some((path.clone(), loaded.clone().err()?)))
            .collect()
    }

    /// The scripts `path` requires, those they require and so on, as they loaded.
    pub(crate) fn required_by(&self, path: &str) -> BTreeSet<String> {
        let requires = self.shared.requires.borrow();
        let mut found = BTreeSet::new();
        let mut next = vec![path.to_owned()];
        while let Some(path) = next.pop() {
            for required in requires.get(&path).into_iter().flatten() {
                if found.insert(required.clone()) {
                    next.push(required.clone());
                }
            }
        }
        found
    }

    /// Table entries converted between Luau and the engine since the last time this was asked.
    pub(crate) fn take_converted(&self) -> u64 {
        self.shared.converted.take()
    }

    /// Registers the objects of a new scene, dropping the handles of the last one; the pairs
    /// of objects whose keys meet.
    pub(crate) fn begin_scene<'a>(&self, ids: impl IntoIterator<Item = &'a str>) -> Vec<(Id, Id)> {
        self.shared.handles.borrow_mut().clear();
        ids.into_iter()
            .filter_map(|id| {
                self.shared
                    .register(id)
                    .err()
                    .map(|other| (other, id.to_owned()))
            })
            .collect()
    }

    /// Objects gone: their handles no longer name them.
    pub(crate) fn forget(&self, ids: &HashSet<Id>) {
        self.shared
            .handles
            .borrow_mut()
            .retain(|_, id| !ids.contains(id));
    }

    /// Runs `function` of `module` for `owner`, as `Module.function(self, ...args)`.
    pub(crate) fn call(
        &self,
        runtime: &mut Runtime,
        owner: &str,
        module: &Rc<Module>,
        function: &str,
        args: &[ScriptArg],
    ) -> ScriptCall {
        let s = &*self.shared;
        if s.step_spent() {
            return ScriptCall::Deferred;
        }
        let path = &*module.path;
        let Some(f) = module.functions.get(function) else {
            return ScriptCall::Failed(ScriptError::new(
                path,
                0,
                format!("o script não tem a função {function}"),
            ));
        };
        // Errors without a line of their own point at the function's.
        let defined = || f.info().line_defined.unwrap_or(0) as u32;
        let (key, values) = match self.arguments(runtime, owner, args) {
            Ok(arguments) => arguments,
            Err(e) => return ScriptCall::Failed(s.failure(&e, path, defined())),
        };
        let (result, suspended) = {
            let _guard = s.enter(runtime, key, module);
            let result = f.call::<()>(values);
            (result, s.suspended_coroutine())
        };
        // What the call did to its memory stays, error or not, as what it did to the world.
        let saved = self
            .write_back(runtime, owner, module)
            .map_err(|error| ScriptError {
                path: path.to_owned(),
                line: defined(),
                ..error
            });
        s.step_units.set(s.step_units.get() + s.units.get());
        if let Some(error) = s
            .engine_error(path, defined)
            .or_else(|| s.overrun_error(path, defined))
        {
            return ScriptCall::Failed(error);
        }
        if let Err(e) = result {
            let failed = s.failure(&e, path, defined());
            // A memory past its limit stops the script, whatever stopped the call; a value
            // refused is said along with the call's error.
            return ScriptCall::Failed(match saved {
                Err(refused) if refused.stops_script() => ScriptError {
                    message: format!(
                        "{}; a chamada também parou num erro: {failed}",
                        refused.message
                    ),
                    ..refused
                },
                Err(refused) => ScriptError {
                    message: format!(
                        "{} (a memória também recusou: {})",
                        failed.message, refused.message
                    ),
                    ..failed
                },
                Ok(()) => failed,
            });
        }
        if suspended {
            return ScriptCall::Failed(suspended_error(path, defined()));
        }
        if let Err(error) = saved {
            return ScriptCall::Failed(error);
        }
        ScriptCall::Done
    }

    /// Writes back the tables of script memory the call touched, emptying the table of touched
    /// tables; why one was not kept, without its place in the file.
    fn write_back(
        &self,
        runtime: &mut Runtime,
        owner: &str,
        module: &Module,
    ) -> Result<(), ScriptError> {
        if !self.shared.touched.replace(false) {
            return Ok(());
        }
        // A memory error leaves the table as it was: the next call's first touch empties it.
        let tables = memory::read_touched(&self.lua, &self.shared, &self.touched)
            .map_err(|e| self.shared.failure(&e, &module.path, 0))?;
        let Some(memory) = runtime.script_memory_if_any(owner, &module.path) else {
            // The object went in the call, and its memory with it.
            return Ok(());
        };
        memory.write_back(tables).map_err(|refusal| {
            let error = ScriptError::new(&module.path, 0, refusal.message());
            if refusal.limit {
                error.with_kind(ScriptErrorKind::Limit)
            } else {
                error
            }
        })
    }

    /// The owner's handle key, and `self` and the arguments as Luau values.
    fn arguments(
        &self,
        runtime: &Runtime,
        owner: &str,
        args: &[ScriptArg],
    ) -> mlua::Result<(u64, MultiValue)> {
        let me = self.key(runtime, owner)?;
        let mut values = MultiValue::with_capacity(args.len() + 1);
        values.push_back(object(me));
        for arg in args {
            values.push_back(match arg {
                ScriptArg::Nil => LuaValue::Nil,
                ScriptArg::Value(value) => self.value(runtime, value)?,
                ScriptArg::Data(value) => memory::to_lua(&self.lua, &self.shared, value)?,
                ScriptArg::Record(fields) => {
                    let table = self.lua.create_table_with_capacity(0, fields.len())?;
                    for (name, value) in fields {
                        table.raw_set(*name, self.value(runtime, value)?)?;
                    }
                    LuaValue::Table(table)
                }
            });
        }
        Ok((me, values))
    }

    fn value(&self, runtime: &Runtime, value: &Value) -> mlua::Result<LuaValue> {
        Ok(match value {
            Value::Number(n) => LuaValue::Number(*n),
            Value::Bool(b) => LuaValue::Boolean(*b),
            Value::Text(text) => LuaValue::String(self.lua.create_string(text)?),
            Value::Vector3([x, y, z]) => LuaValue::Vector(mlua::Vector::new(*x, *y, *z)),
            Value::Vector2([x, y]) => LuaValue::Vector(mlua::Vector::new(*x, *y, 0.)),
            Value::Object(Some(id)) => object(self.key(runtime, id)?),
            Value::Object(None) | Value::Surface(_) => LuaValue::Nil,
        })
    }

    fn key(&self, runtime: &Runtime, id: &str) -> mlua::Result<u64> {
        self.shared
            .register(id)
            .map_err(|other| mlua::Error::runtime(handles::collision(runtime, &other, id)))
    }
}

fn object(key: u64) -> LuaValue {
    LuaValue::LightUserData(mlua::LightUserData(handles::handle(MARK_OBJECT, key)))
}

impl Shared {
    /// A units overrun found where it happened (the door does not see the script line).
    pub(crate) fn place_overrun(&self, at: Option<ScriptLocation>) {
        if let Some((_, place @ None)) = &mut *self.overrun.borrow_mut() {
            *place = at;
        }
    }

    /// The limit the call went past, as its error, forgetting it; `line` when it has none.
    pub(crate) fn overrun_error(&self, path: &str, line: impl Fn() -> u32) -> Option<ScriptError> {
        let (limit, at) = self.overrun.take()?;
        let (path, line) = at.map_or_else(|| (path.to_owned(), line()), |at| (at.path, at.line));
        Some(
            ScriptError::new(&path, line, limit.message(&self.limits.get()))
                .with_kind(ScriptErrorKind::Limit),
        )
    }

    /// A panic caught in the call, as its error, forgetting it.
    pub(crate) fn engine_error(&self, path: &str, line: impl Fn() -> u32) -> Option<ScriptError> {
        let panic = self.poisoned.take()?;
        Some(
            ScriptError::new(path, line(), format!("erro interno da engine em {panic}"))
                .with_kind(ScriptErrorKind::Engine),
        )
    }

    /// The error a call or a top ended with.
    pub(crate) fn failure(&self, error: &mlua::Error, path: &str, line: u32) -> ScriptError {
        match error {
            mlua::Error::MemoryError(_) => {
                ScriptError::new(path, line, Limit::Memory.message(&self.limits.get()))
                    .with_kind(ScriptErrorKind::Limit)
            }
            error => describe(error, path, line),
        }
    }

    /// Whether a coroutine the call created is still suspended (R3), forgetting them.
    pub(crate) fn suspended_coroutine(&self) -> bool {
        self.threads
            .take()
            .iter()
            .any(|thread| thread.status() == ThreadStatus::Resumable)
    }
}

pub(crate) fn suspended_error(path: &str, line: u32) -> ScriptError {
    ScriptError::new(
        path,
        line,
        "uma corrotina criada nesta chamada ficou suspensa no fim dela: geradores servem, espera \
         não (use um temporizador)",
    )
}
