//! What scripts keep in the engine besides their tasks (see `crate::script`): their memory per
//! object and script and the messages on their way, both per scene and in the state; the
//! reload of their modules, which makes the VM anew from the scripts' texts and keeps every
//! bit of the state, so the oracle of a VM made anew takes the same path; and what each script
//! file costs, measured for the tools and never part of the state.
use super::*;
use crate::script::{
    Callback, Memory, Message, ScriptArg, ScriptError, ScriptHost, ScriptReload, ScriptSources,
    ScriptTask, ScriptValue, Unsent, load_check, messages,
};
use std::borrow::Cow;
use std::time::Duration;

/// The table entries converted between Luau and the engine in one step past which a script
/// file is warned about. Calibrated in release on the development PC (stage 2 of 0.7,
/// `scripts_bench`): 0.05–0.08 µs an entry, so this many cost ~50–80 µs a step, about all the
/// 50 µs the parkour's 8 runners may spend in scripts, while a list of 100 records of 3 fields
/// touched every step (1002 entries, ~55 µs), which the 0.8 budget still takes, is not warned.
pub const SCRIPT_ENTRIES_WARNING: u64 = 1024;

/// What a script file cost in the run, for the tools: the wall time of the steps it ran in,
/// which no rule of the game reads, and the table entries its calls converted between Luau and
/// the engine (memory read and written back, messages), which repeat run after run.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptCost {
    pub path: String,
    pub calls: u64,
    /// Steps in which it ran, a scene start counted as one.
    pub steps: u64,
    /// Microseconds of a step: half of its steps took this long or less (to ~40 %).
    pub p50_us: f64,
    /// Microseconds of its slowest step.
    pub max_us: f64,
    /// The most table entries it converted in one step.
    pub most_entries: u64,
}

impl ScriptCost {
    /// Why the file may cost too much: big tables of memory touched every step.
    pub fn warning(&self) -> Option<String> {
        (self.most_entries > SCRIPT_ENTRIES_WARNING).then(|| {
            format!(
                "{} converteu até {} entradas de tabela num passo entre o script e a engine \
                 (memória e mensagens); listas grandes tocadas todo passo pesam: toque-as só \
                 quando mudam, guarde menos ou use uma função em lote",
                self.path, self.most_entries
            )
        })
    }
}

/// Buckets of step times, each √2 wider than the last: up to ~4 s.
const BUCKETS: usize = 64;

struct FileCost {
    calls: u64,
    steps: u64,
    buckets: [u32; BUCKETS],
    max: Duration,
    step_time: Duration,
    step_entries: u64,
    most_entries: u64,
    ran: bool,
}

impl Default for FileCost {
    fn default() -> Self {
        Self {
            calls: 0,
            steps: 0,
            buckets: [0; BUCKETS],
            max: Duration::ZERO,
            step_time: Duration::ZERO,
            step_entries: 0,
            most_entries: 0,
            ran: false,
        }
    }
}

/// What the run's script files cost (see [`ScriptCost`]).
#[derive(Default)]
pub(super) struct ScriptCosts(BTreeMap<Arc<str>, FileCost>);

impl ScriptCosts {
    fn add(&mut self, path: &Arc<str>, time: Duration, entries: u64) {
        if !self.0.contains_key(path) {
            self.0.insert(path.clone(), FileCost::default());
        }
        let file = self.0.get_mut(path).expect("inserted above");
        file.calls += 1;
        file.step_time += time;
        file.step_entries += entries;
        file.ran = true;
    }

    /// A step ended: the files that ran in it count it.
    pub(super) fn end_step(&mut self) {
        for file in self.0.values_mut().filter(|file| file.ran) {
            let nanos = file.step_time.as_nanos().max(1) as f64;
            let bucket = ((nanos.log2() * 2.) as usize).min(BUCKETS - 1);
            file.buckets[bucket] = file.buckets[bucket].saturating_add(1);
            file.max = file.max.max(file.step_time);
            file.most_entries = file.most_entries.max(file.step_entries);
            file.steps += 1;
            file.step_time = Duration::ZERO;
            file.step_entries = 0;
            file.ran = false;
        }
    }

    fn summary(&self) -> Vec<ScriptCost> {
        self.0
            .iter()
            .filter(|(_, file)| file.steps > 0)
            .map(|(path, file)| {
                let half = file.steps.div_ceil(2);
                let mut seen = 0;
                let bucket = file
                    .buckets
                    .iter()
                    .position(|&count| {
                        seen += u64::from(count);
                        seen >= half
                    })
                    .unwrap_or(BUCKETS - 1);
                // The top of the bucket, in microseconds.
                let p50 = 2f64.powf((bucket + 1) as f64 / 2.) / 1000.;
                ScriptCost {
                    path: path.to_string(),
                    calls: file.calls,
                    steps: file.steps,
                    p50_us: p50.min(file.max.as_secs_f64() * 1e6),
                    max_us: file.max.as_secs_f64() * 1e6,
                    most_entries: file.most_entries,
                }
            })
            .collect()
    }
}

impl Runtime {
    /// The memory of the script `path` on `owner`, made from `schema`'s defaults the first time.
    pub(crate) fn script_memory(
        &mut self,
        owner: &str,
        path: &Arc<str>,
        schema: &Memory,
    ) -> &mut Memory {
        if !self.script_memory.contains_key(owner) {
            self.script_memory.insert(owner.to_owned(), Vec::new());
        }
        let records = self.script_memory.get_mut(owner).expect("inserted above");
        let i = match records.binary_search_by(|(p, _)| (**p).cmp(path)) {
            Ok(i) => i,
            Err(i) => {
                records.insert(i, (path.clone(), schema.clone()));
                i
            }
        };
        &mut records[i].1
    }

    /// The memory of the script `path` on `owner`, if it has any.
    pub(crate) fn script_memory_if_any(&mut self, owner: &str, path: &str) -> Option<&mut Memory> {
        let records = self.script_memory.get_mut(owner)?;
        let i = records.binary_search_by(|(p, _)| (**p).cmp(path)).ok()?;
        Some(&mut records[i].1)
    }

    /// Queues `message`, sent by the script `script`, for the next logic phase; why not past the
    /// step's messages of that script on its object or of the scene. A target none of whose
    /// scripts hears on_message is told once for each line that sends to it.
    pub(crate) fn send_message(
        &mut self,
        message: Message,
        script: &Arc<str>,
        at: Option<ScriptLocation>,
    ) -> Result<(), Unsent> {
        let sender = (message.from.clone(), script.clone());
        let sent = self.messages_by_script.get(&sender).copied().unwrap_or(0);
        if sent >= messages::MAX_MESSAGES {
            return Err(Unsent::Script);
        }
        if self.messages_sent >= messages::MAX_SCENE_MESSAGES {
            return Err(Unsent::Scene);
        }
        self.messages_by_script.insert(sender, sent + 1);
        self.messages_sent += 1;
        let target = self.entity(&message.to).expect("checked by send");
        let heard: Option<Vec<bool>> = target
            .scripts
            .iter()
            .map(|script| {
                self.script_callbacks.get(&script.path).map(|heard| {
                    heard
                        .as_ref()
                        .is_some_and(|(_, c)| c.contains(Callback::Message))
                })
            })
            .collect();
        // Scripts not loaded yet (a copy's) may hear it: nothing is said then.
        if heard.is_some_and(|heard| !heard.contains(&true)) {
            let place = at
                .as_ref()
                .map_or_else(String::new, |at| format!("{}:{}: ", at.path, at.line));
            let text = format!(
                "{place}{} não tem on_message em nenhum script: a mensagem \"{}\" não chega a ele.",
                target.name, message.name
            );
            // Whatever the message's name, so that names made on the fly add no lines.
            let key = format!("{place}sem on_message: {}", message.to);
            self.tell_once(String::new(), scripts::ErrorPlace::Text(key), text, at);
        }
        self.messages.push(message);
        Ok(())
    }

    /// The messages sent become tasks, in the order they were sent, for each script of their
    /// target that hears on_message, in the order of its list. A message to an object gone is
    /// dropped, as are those past the queue's room.
    pub(super) fn deliver_messages(&mut self) {
        for Message {
            from,
            to,
            name,
            value,
        } in std::mem::take(&mut self.messages)
        {
            let Some(entity) = self.entity(&to) else {
                continue;
            };
            let paths: Vec<String> = entity.scripts.iter().map(|s| s.path.clone()).collect();
            for path in paths {
                let Some((script, callbacks)) = self.script_heard(&path) else {
                    continue;
                };
                if !callbacks.contains(Callback::Message) || self.script_stopped(&to, &path) {
                    continue;
                }
                let args = vec![
                    ScriptArg::Value(Value::Text(name.clone())),
                    value.clone().map_or(ScriptArg::Nil, ScriptArg::Data),
                    ScriptArg::Value(Value::Object(Some(from.clone()))),
                ];
                self.ready.push_back(Task {
                    due: self.game_time(),
                    owner: to.clone(),
                    target: TaskTarget::Script(ScriptTask {
                        script,
                        function: Cow::Borrowed(Callback::Message.name()),
                        args,
                        timer: None,
                    }),
                    context: Context {
                        owner: to.clone(),
                        other: Some(from.clone()),
                        activation: 0,
                        outputs: scripts::no_outputs(),
                        hits: None,
                        trajectory: None,
                    },
                });
            }
        }
        if self.ready.len() + self.waiting.len() > MAX_TASKS {
            self.log(
                "Limite global de tarefas excedido; comportamentos pendentes foram cancelados."
                    .into(),
            );
            self.ready.clear();
            self.waiting.clear();
        }
    }

    /// The memory and the messages of the objects in `ids`, which went.
    pub(super) fn forget_script_state(&mut self, ids: &HashSet<Id>) {
        self.script_memory.retain(|owner, _| !ids.contains(owner));
        self.messages.retain(|message| !ids.contains(&message.to));
    }

    /// Takes the project's scripts as they are now (the editor's test after a file changed):
    /// the VM is made anew with them between two steps and the game goes on where it was, its
    /// state untouched. A text that does not load keeps its old version running, and so does a
    /// text that makes a script that ran before fail; the report says which. Memory follows the
    /// new declarations (a new field at its default, one of another kind back to it, one gone
    /// dropped), and the scripts a limit stopped on an object run there again, as do those that
    /// require a new text. Nothing goes to the runtime's log. Provisional: the editor calls it
    /// in a later stage.
    #[doc(hidden)]
    pub fn reload_scripts(&mut self, sources: Arc<ScriptSources>) -> ScriptReload {
        self.rebuild_scripts(sources, &ScriptHost::new)
            .unwrap_or_else(|error| ScriptReload {
                warnings: vec![error],
                ..Default::default()
            })
    }

    /// Makes the VM anew from the scripts that run, keeping the state: a script that kept
    /// anything in the VM between calls now runs without it, which the oracle of
    /// `oxy_simulate`'s `check_vm_reset` looks for. Nothing goes to the log. Why it could not:
    /// the VM did not start (the old one goes on), or scripts that ran do not load in the new
    /// one.
    #[doc(hidden)]
    pub fn recreate_script_host(&mut self) -> Result<(), String> {
        self.recreate_script_host_with(&ScriptHost::new)
    }

    /// `recreate_script_host` with the VMs `make` creates (tests give it doubles).
    pub(crate) fn recreate_script_host_with(
        &mut self,
        make: &dyn Fn(Arc<ScriptSources>) -> Result<ScriptHost, String>,
    ) -> Result<(), String> {
        let report = self.rebuild_scripts(self.script_sources.clone(), make)?;
        // The same texts: only a script that loaded before and fails now is an error.
        if report.errors.is_empty() {
            return Ok(());
        }
        let errors: Vec<String> = report.errors.iter().map(ToString::to_string).collect();
        Err(format!(
            "na VM refeita, scripts que rodavam não carregaram: {}",
            errors.join("; ")
        ))
    }

    /// `reload_scripts` with the VMs `make` creates; why the new VM did not start, the old one
    /// going on.
    pub(crate) fn rebuild_scripts(
        &mut self,
        sources: Arc<ScriptSources>,
        make: &dyn Fn(Arc<ScriptSources>) -> Result<ScriptHost, String>,
    ) -> Result<ScriptReload, String> {
        let mut report = ScriptReload::default();
        let old_sources = self.script_sources.clone();
        let Some(old_host) = self.scripts.take() else {
            // No VM: no scene of the run attached a script yet.
            self.script_sources = sources;
            return Ok(report);
        };
        let changed: BTreeSet<String> = old_sources
            .files
            .keys()
            .chain(sources.files.keys())
            .filter(|path| old_sources.get(path) != sources.get(path))
            .cloned()
            .collect();
        let mut running = (*sources).clone();
        // A changed text goes back to its old one; a new file keeps its text, which fails.
        let restore = |running: &mut ScriptSources, path: &str| {
            if let Some(file) = old_sources.get(path) {
                running.files.insert(path.to_owned(), file.clone());
            }
        };
        let revertible = |running: &ScriptSources, path: &str| {
            old_sources.get(path).is_some() && running.get(path) != old_sources.get(path)
        };
        for path in &changed {
            let Some(file) = sources.get(path) else {
                continue;
            };
            if let Err(error) = load_check(path, &file.text) {
                report.errors.push(error);
                restore(&mut running, path);
            }
        }
        // The scripts that run load again in the new VM, and so do the modules the old one
        // loaded (by a require inside a function too) that are still in the folder.
        let mut known: BTreeSet<String> = self.attached_scripts().into_iter().collect();
        known.extend(self.script_callbacks.keys().cloned());
        let required = old_host.loaded_paths();
        let host = loop {
            let host = match make(Arc::new(running.clone())) {
                Ok(host) => host,
                Err(error) => {
                    // The old VM goes on, with the old texts.
                    self.scripts = Some(old_host);
                    return Err(error);
                }
            };
            let collisions = host.begin_scene(self.scene().entities.iter().map(|e| e.id.as_str()));
            for path in &known {
                let _ = host.load(path);
            }
            for path in required.iter().filter(|path| running.get(path).is_some()) {
                let _ = host.load(path);
            }
            // A changed text that fails goes back to its old version, and so do the changed
            // texts that make fail a script that ran before; then the VM is made again.
            let mut back: Vec<(String, ScriptError)> = Vec::new();
            let mut lasting = Vec::new();
            for (path, error) in host.failed_modules() {
                if revertible(&running, &path) {
                    back.push((path, error));
                } else if matches!(old_host.loaded(&path), Some(Ok(_))) {
                    let causes: Vec<String> = host
                        .required_by(&path)
                        .into_iter()
                        .filter(|required| revertible(&running, required))
                        .collect();
                    if causes.is_empty() {
                        lasting.push(error);
                        continue;
                    }
                    for cause in causes {
                        let told = ScriptError::new(
                            &cause,
                            0,
                            format!("a nova versão faz {path} falhar: {error}"),
                        );
                        back.push((cause, told));
                    }
                } else if changed.contains(&path)
                    && !report.errors.iter().any(|told| told.path == path)
                {
                    // A new file whose top fails.
                    lasting.push(error);
                }
            }
            if back.is_empty() {
                report.errors.extend(lasting);
                for (a, b) in collisions {
                    report.warnings.push(crate::script::collision(self, &a, &b));
                }
                break host;
            }
            for (path, error) in back {
                if revertible(&running, &path) {
                    report.errors.push(error);
                    restore(&mut running, &path);
                }
            }
        };
        report.reloaded = changed
            .iter()
            .filter(|path| {
                running.get(path) == sources.get(path) && matches!(host.loaded(path), Some(Ok(_)))
            })
            .cloned()
            .collect();
        for path in &report.reloaded {
            let Some(new) = host.loaded(path).and_then(Result::ok) else {
                continue;
            };
            for (name, line, near) in &new.unknown {
                let hint = near.map_or_else(String::new, |near| format!("; quis dizer {near}?"));
                report.warnings.push(format!(
                    "{path}:{line}: {name} não é um evento dos scripts e nunca roda sozinha{hint}"
                ));
            }
        }
        // Memory follows what each script declares now, whether its own text changed or that
        // of a module its top reads (`M.state = { n = H.START }`).
        for path in host.loaded_paths() {
            let Some(new) = host.loaded(&path).and_then(Result::ok) else {
                continue;
            };
            let old = old_host.loaded(&path).and_then(Result::ok);
            if old.as_ref().and_then(|m| m.state.as_ref()) != new.state.as_ref() {
                self.reconcile_memory(&path, new.state.as_ref(), &mut report.warnings);
            }
        }
        // The scripts a limit stopped run again where their new text runs, or the new text of a
        // module they require, and their errors are told again.
        let renewed: BTreeSet<String> = host
            .loaded_paths()
            .into_iter()
            .filter(|path| {
                report.reloaded.contains(path)
                    || host
                        .required_by(path)
                        .iter()
                        .any(|required| report.reloaded.contains(required))
            })
            .collect();
        self.disabled_scripts
            .retain(|(_, path)| !renewed.contains(path));
        self.run
            .script_errors
            .retain(|(_, place), _| !renewed.iter().any(|path| place.is_in(path)));
        // What each attached script hears, as the new VM loaded it.
        self.script_callbacks = self
            .script_callbacks
            .keys()
            .map(|path| {
                let loaded = host.loaded(path).and_then(Result::ok);
                (
                    path.clone(),
                    loaded.map(|module| (module.path.clone(), module.callbacks)),
                )
            })
            .collect();
        self.event_operations.take();
        self.scripts = Some(host);
        self.script_sources = Arc::new(running);
        Ok(report)
    }

    /// Brings the memories of `path` to `schema` after a reload, warning once per field.
    fn reconcile_memory(
        &mut self,
        path: &str,
        schema: Option<&Memory>,
        warnings: &mut Vec<String>,
    ) {
        let mut reset = BTreeSet::new();
        let mut dropped = BTreeSet::new();
        let mut cleared = false;
        for records in self.script_memory.values_mut() {
            let Ok(i) = records.binary_search_by(|(p, _)| (**p).cmp(path)) else {
                continue;
            };
            match schema {
                Some(schema) => {
                    let (r, d) = records[i].1.reconcile(schema);
                    reset.extend(r);
                    dropped.extend(d);
                }
                None => {
                    records.remove(i);
                    cleared = true;
                }
            }
        }
        self.script_memory.retain(|_, records| !records.is_empty());
        if cleared {
            warnings.push(format!(
                "{path}: o script não declara mais memória (Modulo.state); a memória dos objetos \
                 foi descartada"
            ));
        }
        for name in reset {
            warnings.push(format!(
                "{path}: o campo {name} da memória mudou de tipo e voltou ao valor declarado"
            ));
        }
        for name in dropped {
            warnings.push(format!(
                "{path}: o campo {name} saiu de Modulo.state e da memória dos objetos"
            ));
        }
    }

    /// Counts a call of `path` in the step's cost.
    pub(super) fn count_script_cost(&mut self, path: &Arc<str>, time: Duration, entries: u64) {
        self.script_costs.add(path, time, entries);
    }

    /// What each script file cost in the run so far, by path: the steps it ran in, how long
    /// they took and the table entries it converted. Wall time is measured, never part of the
    /// state.
    pub fn script_costs(&self) -> Vec<ScriptCost> {
        self.script_costs.summary()
    }

    /// The memory of the script `path` on `owner`, field by field as Luau would show it; none
    /// when it has none. For tests and, later, the tools.
    #[doc(hidden)]
    pub fn script_memory_text(&self, owner: &str, path: &str) -> Option<Vec<(String, String)>> {
        let records = self.script_memory.get(owner)?;
        let i = records.binary_search_by(|(p, _)| (**p).cmp(path)).ok()?;
        Some(
            records[i]
                .1
                .fields()
                .iter()
                .map(|(name, value)| (name.to_string(), show(value)))
                .collect(),
        )
    }
}

/// A value of memory as text: tables in the canonical order of their keys.
fn show(value: &ScriptValue) -> String {
    match value {
        ScriptValue::Bool(b) => b.to_string(),
        ScriptValue::Number(n) => n.to_string(),
        ScriptValue::Text(text) => format!("{text:?}"),
        ScriptValue::Vector([x, y, z]) => format!("vector({x}, {y}, {z})"),
        ScriptValue::Object(id) => format!("objeto {id}"),
        ScriptValue::Table(entries) => {
            let items: Vec<String> = entries
                .iter()
                .map(|(key, value)| match key {
                    crate::script::memory::Key::Int(i) => format!("[{i}] = {}", show(value)),
                    crate::script::memory::Key::Text(text) => format!("{text} = {}", show(value)),
                })
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}
