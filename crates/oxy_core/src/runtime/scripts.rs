//! The `Runtime`'s side of scripts (see `crate::script`): the host, kept out of the state as a
//! cache and created only when a scene attaches a script; the scripts stopped on an object by a
//! limit (per scene, in the state); and the errors told once per run with how often they
//! happened (in the run's state, since they decide which lines the log gets).
use super::*;
use crate::script::{ScriptCall, ScriptError, ScriptHost};
use std::collections::btree_map::Entry;

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
    /// objects run without it. A scene without scripts creates no VM.
    pub(super) fn start_scripts(&mut self) {
        let mut paths: Vec<String> = Vec::new();
        for script in self.scene().entities.iter().flat_map(|e| &e.scripts) {
            if !paths.contains(&script.path) {
                paths.push(script.path.clone());
            }
        }
        if paths.is_empty() && self.scripts.is_none() {
            return;
        }
        if !self.script_host_ready() {
            return;
        }
        let host = self.scripts.as_ref().expect("script host");
        // The start logic has a budget of its own, as the graph's has.
        host.begin_step();
        let collisions = host.begin_scene(self.scene().entities.iter().map(|e| e.id.as_str()));
        for (a, b) in collisions {
            let text = crate::script::collision(self, &a, &b);
            self.tell_once(String::new(), ErrorPlace::Text(text.clone()), text, None);
        }
        for path in paths {
            self.load_script(&path);
        }
    }

    /// Loads `path` unless it already was; whether it is ready to run.
    fn load_script(&mut self, path: &str) -> bool {
        let Some(host) = &self.scripts else {
            return false;
        };
        match host.load(path) {
            (Ok(()), _) => true,
            (Err(error), first) => {
                if first {
                    let text = format!(
                        "{} Os objetos com este script ficam sem ele.",
                        sentence(&error)
                    );
                    self.tell_once(
                        String::new(),
                        ErrorPlace::of(&error),
                        text,
                        error.location(),
                    );
                }
                false
            }
        }
    }

    /// Runs `function` of the script `path` that `owner` attaches, as
    /// `Module.function(self, ...args)`. An error stops only that call and is told once, with
    /// how often it happened; past a limit of work or memory the script also stops on `owner`.
    /// Provisional: the entry point the tasks of scripts will use; tests call it directly.
    #[doc(hidden)]
    pub fn call_script(
        &mut self,
        owner: &str,
        path: &str,
        function: &str,
        args: &[Value],
    ) -> ScriptCall {
        let stopped = self
            .disabled_scripts
            .iter()
            .any(|(o, p)| o == owner && p == path);
        if self.stopped || stopped || self.entity(owner).is_none() {
            return ScriptCall::Skipped;
        }
        if !self.script_host_ready() || !self.load_script(path) {
            return ScriptCall::Skipped;
        }
        let host = self.scripts.take().expect("script host");
        let call = host.call(self, owner, path, function, args);
        self.scripts = Some(host);
        if let ScriptCall::Failed(error) = &call {
            self.report_script_error(owner, path, error);
        }
        call
    }

    fn report_script_error(&mut self, owner: &str, path: &str, error: &ScriptError) {
        let stops = error.stops_script();
        if stops {
            self.disabled_scripts
                .insert((owner.to_owned(), path.to_owned()));
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

    /// Logs `text` the first time `owner` gets an error at `place` in the run and counts every
    /// time.
    fn tell_once(
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

    #[cfg(test)]
    pub(crate) fn script_host(&self) -> Option<&ScriptHost> {
        self.scripts.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn replace_script_host(&mut self, host: ScriptHost) {
        self.scripts = Some(host);
    }
}
