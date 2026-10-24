//! The `Runtime`'s side of scripts (see `crate::script`): the host, kept out of the state as a
//! cache and created only when a scene attaches a script; the callbacks events call and the
//! timers scripts ask for, which are tasks in the same queues as the graph's (in the state);
//! the scripts stopped on an object by a limit (per scene, in the state); and the errors told
//! once per run with how often they happened (in the run's state, since they decide which
//! lines the log gets).
use super::*;
use crate::script::{
    Callback, Callbacks, Module, Repeat, ScriptArg, ScriptCall, ScriptError, ScriptHost,
    ScriptTask, Share, Timer, TimerStep, timers,
};
use std::borrow::Cow;
use std::collections::btree_map::Entry;
use std::rc::Rc;
use std::sync::LazyLock;

/// What tells a repeat of a script error on an object: the script line it happened at, and
/// whether it stopped the script, whatever values its message carries; the whole text for one
/// without a line (a script that did not load, a function the module lacks).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ErrorPlace {
    Line {
        path: String,
        line: u32,
        stops: bool,
    },
    Text(String),
}

impl ErrorPlace {
    /// Whether the place is in the script `path`.
    pub(super) fn is_in(&self, path: &str) -> bool {
        match self {
            Self::Line { path: p, .. } => p == path,
            Self::Text(text) => text.starts_with(&format!("{path}:")),
        }
    }

    /// The script and line the place is in, the line 0 when the error names none; none for an
    /// error of no script (the VM did not start, two objects' handles collide).
    fn script_line(&self) -> Option<(&str, u32)> {
        match self {
            Self::Line { path, line, .. } => Some((path, *line)),
            Self::Text(text) => {
                let end = text.find(".luau:")? + ".luau".len();
                let digits = text[end + 1..]
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or_default();
                Some((&text[..end], digits.parse().unwrap_or(0)))
            }
        }
    }

    fn of(error: &ScriptError) -> Self {
        match error.location() {
            Some(at) => Self::Line {
                path: at.path,
                line: at.line,
                stops: error.stops_script(),
            },
            None => Self::Text(error.to_string()),
        }
    }
}

/// `error` as a sentence: its text with a full stop unless it ends in one.
fn sentence(error: &ScriptError) -> String {
    if error.message.ends_with(['.', '?', '!']) {
        error.to_string()
    } else {
        format!("{error}.")
    }
}

/// The outputs of a script's task: none, shared.
pub(super) fn no_outputs() -> Arc<BTreeMap<(Id, String), Value>> {
    static NONE: LazyLock<Arc<BTreeMap<(Id, String), Value>>> = LazyLock::new(Arc::default);
    NONE.clone()
}

/// The callback `event` calls on the scripts of `owner`; none when it is not `owner`'s to hear.
fn callback_of(event: &RuntimeEvent, owner: &str) -> Option<Callback> {
    Some(match event {
        RuntimeEvent::SceneStart => Callback::Start,
        RuntimeEvent::FixedStep => Callback::Step,
        RuntimeEvent::Input(_) | RuntimeEvent::InputReleased(_) => Callback::Input,
        RuntimeEvent::Click(id) if id == owner => Callback::Click,
        RuntimeEvent::AreaEnter { area, .. } if area == owner => Callback::AreaEnter,
        RuntimeEvent::AreaExit { area, .. } if area == owner => Callback::AreaExit,
        RuntimeEvent::Animation { object, .. } if object == owner => Callback::Marker,
        RuntimeEvent::Movement(record) if record.object == owner => Callback::Character,
        RuntimeEvent::Pause(_) => Callback::Pause,
        _ => return None,
    })
}

/// What `event` passes its callback after `self`: an input action by its name in `project`, as
/// the person named it.
fn callback_args(event: &RuntimeEvent, time: f64, project: &Project) -> Vec<ScriptArg> {
    let text = |text: &str| ScriptArg::Value(Value::Text(text.to_owned()));
    let action = |id: &str| text(&crate::input_actions::label(project, id));
    match event {
        RuntimeEvent::FixedStep => vec![ScriptArg::Value(Value::Number(f64::from(FIXED_DT)))],
        RuntimeEvent::Input(id) => vec![action(id), text("pressed")],
        RuntimeEvent::InputReleased(id) => vec![action(id), text("released")],
        RuntimeEvent::AreaEnter { other, .. } | RuntimeEvent::AreaExit { other, .. } => {
            vec![ScriptArg::Value(Value::Object(Some(other.clone())))]
        }
        RuntimeEvent::Animation { marker, clip, .. } => vec![text(marker), text(clip)],
        // Provisional: the kind and the values the graph's Evento de personagem gives.
        RuntimeEvent::Movement(record) => {
            let mut fields = vec![(
                "kind",
                Value::Text(movement_nodes::event_kind(&record.event).into()),
            )];
            fields.extend(movement_nodes::event_values(event, time));
            vec![ScriptArg::Record(fields)]
        }
        RuntimeEvent::Pause(paused) => vec![ScriptArg::Value(Value::Bool(*paused))],
        _ => Vec::new(),
    }
}

impl Runtime {
    /// Whether the host exists, creating it once a scene needs it. A VM that cannot start is
    /// told in the log, once.
    fn script_host_ready(&mut self) -> bool {
        if self.scripts.is_none() {
            match ScriptHost::new(self.script_sources.clone()) {
                Ok(host) => self.scripts = Some(host),
                Err(error) => {
                    self.tell_once(String::new(), ErrorPlace::Text(error.clone()), error, None)
                }
            }
        }
        self.scripts.is_some()
    }

    /// At a scene's start, before its start logic: the objects get their handles and each script
    /// they attach loads, in scene order. A script that does not load is told once and its
    /// objects run without it. A scene without scripts creates no VM. Within a step, the start
    /// logic of the scene spends that step's work; a new runtime's VM starts with all of it.
    pub(super) fn start_scripts(&mut self) {
        let paths = self.attached_scripts();
        if paths.is_empty() && self.scripts.is_none() {
            return;
        }
        if !self.script_host_ready() {
            return;
        }
        let host = self.scripts.as_ref().expect("script host");
        let collisions = host.begin_scene(self.scene().entities.iter().map(|e| e.id.as_str()));
        for (a, b) in collisions {
            let text = crate::script::collision(self, &a, &b);
            self.tell_once(String::new(), ErrorPlace::Text(text.clone()), text, None);
        }
        for path in paths {
            self.load_script(&path);
        }
    }

    /// The scripts the scene's objects attach, each once, in scene order.
    pub(super) fn attached_scripts(&self) -> Vec<String> {
        let mut paths: Vec<String> = Vec::new();
        for script in self.scene().entities.iter().flat_map(|e| &e.scripts) {
            if !paths.contains(&script.path) {
                paths.push(script.path.clone());
            }
        }
        paths
    }

    /// The module `path`, loading it unless it already was; none when it does not load or the
    /// VM is busy with a call. The first time a script reaches the runtime as one an object
    /// attaches, what it hears is registered, or why it did not load is told, whether or not a
    /// `require` loaded it before.
    fn load_script(&mut self, path: &str) -> Option<Rc<Module>> {
        let loaded = self.scripts.as_ref()?.load(path);
        if !self.script_callbacks.contains_key(path) {
            match &loaded {
                Ok(module) => self.register_script(module),
                Err(error) => {
                    self.script_callbacks.insert(path.to_owned(), None);
                    let text = format!(
                        "{} Os objetos com este script ficam sem ele.",
                        sentence(error)
                    );
                    self.tell_once(String::new(), ErrorPlace::of(error), text, error.location());
                }
            }
        }
        loaded.ok()
    }

    /// The callbacks of an attached script join what the scene hears; its functions named like
    /// callbacks that are none are told, and so is a `Module.attributes` that declares nothing
    /// attaching could create (the script runs all the same).
    fn register_script(&mut self, module: &Module) {
        let path = &*module.path;
        self.script_callbacks.insert(
            path.to_owned(),
            Some((module.path.clone(), module.callbacks)),
        );
        // What the scene hears may grow with it.
        self.event_operations.take();
        for (name, line, near) in &module.unknown {
            let error = crate::script::unknown_callback(path, name, *line, *near);
            self.tell_once(
                String::new(),
                ErrorPlace::of(&error),
                sentence(&error),
                error.location(),
            );
        }
        if let Err(message) = &module.attributes {
            let error = crate::script::undeclared_attributes(path, message);
            self.tell_once(
                String::new(),
                ErrorPlace::of(&error),
                sentence(&error),
                error.location(),
            );
        }
    }

    /// What the script `path` hears, loading it if the VM is free; none when it does not load.
    pub(super) fn script_heard(&mut self, path: &str) -> Option<(Arc<str>, Callbacks)> {
        if !self.script_callbacks.contains_key(path) {
            self.load_script(path);
        }
        self.script_callbacks.get(path).cloned().flatten()
    }

    /// Whether a limit stopped `path` on `owner`.
    pub(super) fn script_stopped(&self, owner: &str, path: &str) -> bool {
        !self.disabled_scripts.is_empty()
            && self
                .disabled_scripts
                .iter()
                .any(|(o, p)| o == owner && p == path)
    }

    /// The tasks `event` gives the scripts `entity` attaches (`scripts`, from the catalog), in
    /// their order, after its graph's.
    pub(super) fn script_event_tasks(
        &self,
        event: &RuntimeEvent,
        entity: &crate::document::Entity,
        scripts: &[(Arc<str>, Callbacks)],
        generated_activation: u64,
        found: &mut Vec<Task>,
    ) {
        let Some(callback) = callback_of(event, &entity.id) else {
            return;
        };
        let mut heard = None;
        for (script, callbacks) in scripts {
            if !callbacks.contains(callback) || self.script_stopped(&entity.id, script) {
                continue;
            }
            let ((other, activation, trajectory), args) = heard.get_or_insert_with(|| {
                (
                    self.event_context(event, &entity.id, generated_activation),
                    callback_args(event, self.game_time(), &self.project),
                )
            });
            found.push(Task {
                due: self.game_time(),
                owner: entity.id.clone(),
                target: TaskTarget::Script(ScriptTask {
                    script: script.clone(),
                    function: Cow::Borrowed(callback.name()),
                    args: args.clone(),
                    timer: None,
                }),
                context: Context {
                    owner: entity.id.clone(),
                    other: other.clone(),
                    activation: *activation,
                    outputs: no_outputs(),
                    hits: None,
                    trajectory: trajectory.clone(),
                },
            });
        }
    }

    /// The objects of a copy just made (`copied`, the copy's IDs) whose scripts hear on_start
    /// get it, in scene order, after what is already queued. A copy a script's call made waits
    /// for the call to end, which gives the VM back to load the copy's scripts.
    pub(super) fn start_copied_scripts(&mut self, copied: &HashSet<Id>) {
        if self.in_script_call {
            self.copies_to_start.push(copied.clone());
            return;
        }
        let attached: Vec<(Id, Vec<String>)> = self
            .scene()
            .entities
            .iter()
            .filter(|e| !e.scripts.is_empty() && copied.contains(&e.id))
            .map(|e| {
                (
                    e.id.clone(),
                    e.scripts.iter().map(|s| s.path.clone()).collect(),
                )
            })
            .collect();
        for (owner, paths) in attached {
            for path in paths {
                let Some((script, callbacks)) = self.script_heard(&path) else {
                    continue;
                };
                if !callbacks.contains(Callback::Start) || self.script_stopped(&owner, &path) {
                    continue;
                }
                self.ready.push_back(Task {
                    due: self.game_time(),
                    owner: owner.clone(),
                    target: TaskTarget::Script(ScriptTask {
                        script,
                        function: Cow::Borrowed(Callback::Start.name()),
                        args: Vec::new(),
                        timer: None,
                    }),
                    context: Context {
                        owner: owner.clone(),
                        other: Some(owner.clone()),
                        activation: 0,
                        outputs: no_outputs(),
                        hits: None,
                        trajectory: None,
                    },
                });
            }
        }
    }

    /// Runs a script's task from the queue; gives it back when the step's work for scripts ran
    /// out before it. A timer's next call is queued before this one runs, so that the call can
    /// cancel it.
    pub(super) fn run_script_task(&mut self, task: Task) -> Option<Task> {
        let Task {
            due,
            owner,
            target: TaskTarget::Script(script),
            context,
        } = task
        else {
            unreachable!("only a script's task reaches run_script_task");
        };
        if self.script_stopped(&owner, &script.script) {
            return None;
        }
        if self.scripts.as_ref().is_some_and(ScriptHost::step_spent) {
            let text = "Limite de trabalho dos scripts no passo atingido; demais tarefas retomam \
                        no próximo passo."
                .to_owned();
            self.tell_once(String::new(), ErrorPlace::Text(text.clone()), text, None);
            return Some(Task {
                due,
                owner,
                target: TaskTarget::Script(script),
                context,
            });
        }
        let ScriptTask {
            script: path,
            function,
            args,
            timer,
        } = script;
        // A timer whose function a reload took away falls, after telling it once.
        let vanished = timer.is_some()
            && self
                .load_script(&path)
                .is_some_and(|module| !module.functions.contains_key(&*function));
        if let Some(Timer {
            id,
            then,
            mut share,
        }) = timer.filter(|_| !vanished)
        {
            let next = match then {
                Repeat::Once => None,
                Repeat::Every(interval) => Some((
                    interval,
                    function.clone(),
                    args.clone(),
                    Repeat::Every(interval),
                )),
                Repeat::Steps(mut rest) => rest.pop_front().map(|step| {
                    // The next call holds the rest; this one's data leaves with it.
                    share.give_back(1, timers::text_of(&args));
                    (
                        step.delay,
                        Cow::Owned(step.function.to_string()),
                        step.args,
                        Repeat::Steps(rest),
                    )
                }),
            };
            if let Some((delay, function, args, then)) = next {
                let task = timer_task(
                    &owner,
                    path.clone(),
                    due + delay,
                    function,
                    args,
                    Timer { id, then, share },
                );
                self.waiting.push(task);
            }
        }
        let hits = context.hits.map(|hits| (owner.clone(), hits));
        self.call_script_with(&owner, &path, &function, &args, hits);
        None
    }

    /// Runs `function` of the script `path` that `owner` attaches, as
    /// `Module.function(self, ...args)`. An error stops only that call and is told once, with
    /// how often it happened; past a limit of work or memory the script also stops on `owner`.
    /// Provisional: the queue's tasks run through it, and tests call it directly.
    #[doc(hidden)]
    pub fn call_script(
        &mut self,
        owner: &str,
        path: &str,
        function: &str,
        args: &[Value],
    ) -> ScriptCall {
        if self.entity(owner).is_none() {
            return ScriptCall::Skipped;
        }
        let args: Vec<ScriptArg> = args.iter().cloned().map(ScriptArg::Value).collect();
        self.call_script_with(owner, path, function, &args, None)
    }

    /// `call_script` for an `owner` known to be in the scene; `hits`, the area and the hits of
    /// the area event the call answers.
    fn call_script_with(
        &mut self,
        owner: &str,
        path: &str,
        function: &str,
        args: &[ScriptArg],
        hits: Option<(Id, HitLease)>,
    ) -> ScriptCall {
        if self.stopped || self.script_stopped(owner, path) {
            return ScriptCall::Skipped;
        }
        if !self.script_host_ready() {
            return ScriptCall::Skipped;
        }
        let Some(module) = self.load_script(path) else {
            return ScriptCall::Skipped;
        };
        let host = self.scripts.take().expect("script host");
        let started = std::time::Instant::now();
        self.in_script_call = true;
        self.script_hits = hits;
        let call = host.call(self, owner, &module, function, args);
        self.in_script_call = false;
        self.script_hits = None;
        let spent = started.elapsed();
        let converted = host.take_converted();
        let scene = host.take_scene_change();
        self.scripts = Some(host);
        self.count_script_cost(&module.path, spent, converted);
        if let ScriptCall::Failed(error) = &call {
            self.report_script_error(owner, path, error);
        }
        // What the call left for its end: the copies' scripts start, then the scene it asked
        // for opens (dropping the tasks of this one, theirs included).
        for copied in std::mem::take(&mut self.copies_to_start) {
            self.start_copied_scripts(&copied);
        }
        if let Some(scene) = scene
            && let Err(error) = self.change_scene_from_logic(&scene)
        {
            self.log(format!("A troca de cena pedida por {path} falhou: {error}"));
        }
        call
    }

    fn report_script_error(&mut self, owner: &str, path: &str, error: &ScriptError) {
        let stops = error.stops_script();
        if stops {
            self.disabled_scripts
                .insert((owner.to_owned(), path.to_owned()));
            // Its tasks go; the graph and the object's other scripts go on.
            let its = |task: &Task| {
                task.owner == owner
                    && matches!(&task.target, TaskTarget::Script(s) if &*s.script == path)
            };
            self.ready.retain(|task| !its(task));
            self.waiting.retain(|task| !its(task));
        }
        let object = self
            .entity(owner)
            .map_or_else(|| "Objeto removido".to_owned(), |e| e.name.clone());
        let mut text = format!("{object} · {}", sentence(error));
        if stops {
            text.push_str(" Script interrompido neste objeto.");
        }
        self.tell_once(
            owner.to_owned(),
            ErrorPlace::of(error),
            text,
            error.location(),
        );
    }

    /// Queues a timer of the script `script` on `owner`: `first` after its delay of game time,
    /// then what `then` says. Its number, for `self:cancel`; with no `first`, a number for
    /// nothing. None when the queue has no room for it: the tasks queued and the calls still to
    /// come of sequences stay within [`MAX_TASKS`], and the text of the timers' data within
    /// [`timers::ROOM_TEXT`].
    pub(crate) fn add_script_timer(
        &mut self,
        owner: &str,
        script: &Arc<str>,
        first: Option<TimerStep>,
        then: Repeat,
    ) -> Option<u64> {
        let rest = match &then {
            Repeat::Steps(steps) => Some(steps),
            Repeat::Once | Repeat::Every(_) => None,
        };
        let (calls, text) = rest.into_iter().flatten().fold(
            (
                0,
                first.as_ref().map_or(0, |step| timers::text_of(&step.args)),
            ),
            |(calls, text), step| (calls + 1, text + timers::text_of(&step.args)),
        );
        let queued = self.ready.len() + self.waiting.len() + self.timer_room.calls();
        if queued + usize::from(first.is_some()) + calls > MAX_TASKS
            || self.timer_room.text() + text > timers::ROOM_TEXT
        {
            return None;
        }
        self.run.timers += 1;
        let id = self.run.timers;
        if let Some(TimerStep {
            delay,
            function,
            args,
        }) = first
        {
            let due = self.game_time() + delay;
            let share = Share::new(&self.timer_room, calls, text);
            let task = timer_task(
                owner,
                script.clone(),
                due,
                Cow::Owned(function.to_string()),
                args,
                Timer { id, then, share },
            );
            self.waiting.push(task);
        }
        Some(id)
    }

    /// Drops what is left of the timer `id` on `owner`, whichever of its scripts asked for it
    /// (the numbers are the run's); whether any of it was still to come.
    pub(crate) fn cancel_script_timer(&mut self, owner: &str, id: u64) -> bool {
        let its = |task: &Task| {
            task.owner == owner
                && matches!(&task.target, TaskTarget::Script(s)
                    if s.timer.as_ref().is_some_and(|t| t.id == id))
        };
        let before = self.ready.len() + self.waiting.len();
        self.ready.retain(|task| !its(task));
        self.waiting.retain(|task| !its(task));
        self.ready.len() + self.waiting.len() != before
    }

    /// Logs `text` the first time `owner` gets an error at `place` in the run and counts every
    /// time.
    pub(super) fn tell_once(
        &mut self,
        owner: Id,
        place: ErrorPlace,
        text: String,
        script: Option<ScriptLocation>,
    ) {
        match self.run.script_errors.entry((owner, place)) {
            Entry::Occupied(mut told) => told.get_mut().1 += 1,
            Entry::Vacant(new) => {
                new.insert((text.clone(), 1));
                self.log_at(text, script);
            }
        }
    }

    /// The script errors told in the run: the object (empty for a script that did not load),
    /// the line the log has of the first one at that place and how many times it happened.
    pub fn script_errors(&self) -> impl Iterator<Item = (&str, &str, u64)> {
        self.run
            .script_errors
            .iter()
            .map(|((owner, _), (text, count))| (owner.as_str(), text.as_str(), *count))
    }

    /// [`Self::script_errors`] by where they happened: the object (empty for a script that did
    /// not load), the script, its line (0 when the error names none) and the line the log has.
    /// The errors of a script whose new text reloaded are gone; those of no script are left out.
    pub fn script_error_lines(&self) -> impl Iterator<Item = (&str, &str, u32, &str)> {
        self.run
            .script_errors
            .iter()
            .filter_map(|((owner, place), (text, _))| {
                let (path, line) = place.script_line()?;
                Some((owner.as_str(), path, line, text.as_str()))
            })
    }

    /// Whether the run has a Luau VM: only once a scene attached a script.
    #[doc(hidden)]
    pub fn has_script_host(&self) -> bool {
        self.scripts.is_some()
    }

    /// The scripts stopped on an object by a limit, as (object, script).
    pub fn disabled_scripts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.disabled_scripts
            .iter()
            .map(|(owner, path)| (owner.as_str(), path.as_str()))
    }

    /// The script timers still to come, as (object, script, number), in queue order.
    #[doc(hidden)]
    pub fn script_timers(&self) -> Vec<(&str, &str, u64)> {
        self.ready
            .iter()
            .chain(&self.waiting)
            .filter_map(|task| match &task.target {
                TaskTarget::Script(ScriptTask {
                    script,
                    timer: Some(timer),
                    ..
                }) => Some((task.owner.as_str(), &**script, timer.id)),
                _ => None,
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn script_host(&self) -> Option<&ScriptHost> {
        self.scripts.as_ref()
    }

    /// A VM made anew loads the attached scripts anew, and what they hear with them.
    #[cfg(test)]
    pub(crate) fn replace_script_host(&mut self, host: ScriptHost) {
        self.scripts = Some(host);
        self.script_callbacks.clear();
        self.event_operations.take();
        for path in self.attached_scripts() {
            self.load_script(&path);
        }
    }

    /// What the queued timers hold beyond their tasks: (calls, bytes of text).
    #[cfg(test)]
    pub(crate) fn timer_room(&self) -> (usize, usize) {
        (self.timer_room.calls(), self.timer_room.text())
    }
}

/// A timer's task: no event's context, so no trajectory or activation holds it back.
fn timer_task(
    owner: &str,
    script: Arc<str>,
    due: f64,
    function: Cow<'static, str>,
    args: Vec<ScriptArg>,
    timer: Timer,
) -> Task {
    Task {
        due,
        owner: owner.to_owned(),
        target: TaskTarget::Script(ScriptTask {
            script,
            function,
            args,
            timer: Some(timer),
        }),
        context: Context {
            owner: owner.to_owned(),
            other: None,
            activation: 0,
            outputs: no_outputs(),
            hits: None,
            trajectory: None,
        },
    }
}
