//! Hot reload while testing in the editor (design §4.11, §6.5): a thread walks the project's
//! tree every [`LOOK_EVERY`], only while a test runs, looking at the size and the time of each
//! `.luau` file in any folder, and reads the scripts again only once a change held still for two
//! looks (editors write a temporary file and rename it); a text that came back the same is no
//! change. The test takes the new texts between two steps, paused too
//! (`Runtime::reload_scripts`): the game goes on where it was, and a text that does not load, or
//! a file that cannot be read, leaves the old one running. Each reload is told in text lines of
//! the Console, never in the test's log, whose lines are part of the game's state.
use super::*;
use oxy_core::persistence::ScriptStamp;
use oxy_core::script::ScriptReload;
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

/// How often the tree is looked at (design: every 250–500 ms).
const LOOK_EVERY: Duration = Duration::from_millis(300);

/// What a look at the tree found.
#[derive(Debug, PartialEq)]
pub(crate) enum Change {
    /// The scripts of the tree, after a change that held still.
    Sources(Arc<ScriptSources>),
    /// Why the tree could not be walked; told once until it walks again.
    Unreadable(String),
}

/// The looks at the tree, each against the one before it.
pub(crate) struct Scanner {
    root: PathBuf,
    /// The stamps of the last look.
    last: Option<BTreeMap<String, ScriptStamp>>,
    /// The stamps of the files when the scripts were last read.
    read: Option<BTreeMap<String, ScriptStamp>>,
    /// The texts the test last took.
    known: Arc<ScriptSources>,
    told: Option<String>,
}

impl Scanner {
    pub(crate) fn new(root: PathBuf, known: Arc<ScriptSources>) -> Self {
        Self {
            root,
            last: None,
            read: None,
            known,
            told: None,
        }
    }

    /// One look: the change found, when one held still since the look before.
    pub(crate) fn look(&mut self) -> Option<Change> {
        let stamps = match persistence::script_stamps(&self.root) {
            Ok(stamps) => stamps,
            Err(error) => return self.tell(error.to_string()),
        };
        let still = self.last.as_ref() == Some(&stamps);
        self.last = Some(stamps);
        if !still || self.read == self.last {
            return None;
        }
        self.read = self.last.clone();
        match persistence::load_scripts(&self.root) {
            Ok(sources) => {
                self.told = None;
                // A file another program held is read again at the next look, until it reads:
                // its stamp may stay as it was. One too large or not text waits for its stamp.
                if sources.held() {
                    self.read = None;
                }
                if sources == *self.known {
                    return None;
                }
                self.known = Arc::new(sources);
                Some(Change::Sources(self.known.clone()))
            }
            Err(error) => self.tell(error.to_string()),
        }
    }

    fn tell(&mut self, error: String) -> Option<Change> {
        if self.told.as_ref() == Some(&error) {
            return None;
        }
        self.told = Some(error.clone());
        Some(Change::Unreadable(error))
    }
}

/// The watcher of a test: its thread stops when it drops.
pub(crate) struct ScriptWatch {
    changes: mpsc::Receiver<Change>,
    stop: Arc<AtomicBool>,
    /// The texts the test runs now, to tell a changed file from a new one.
    running: Arc<ScriptSources>,
}

impl ScriptWatch {
    /// Watches the scripts of the tree of the project saved in `root` for a test that started
    /// with `running`; asks `ctx` for a frame when something changed, so a test paused while the
    /// person writes in another window takes it too. None when the thread could not start.
    pub(crate) fn start(
        root: PathBuf,
        running: Arc<ScriptSources>,
        ctx: egui::Context,
    ) -> Option<Self> {
        let (sender, changes) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut scanner = Scanner::new(root, running.clone());
        let stopped = stop.clone();
        std::thread::Builder::new()
            .name("oxy-script-watch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(LOOK_EVERY);
                    if stopped.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(change) = scanner.look() {
                        if sender.send(change).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            })
            .ok()?;
        Some(Self {
            changes,
            stop,
            running,
        })
    }
}

impl Drop for ScriptWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The Console's lines about a reload, each with whether it is an error: the scripts that
/// reloaded with how many objects of the scene attach them, the texts that did not load with
/// their file and line (a file that existed in `before` goes on with its old text), and the
/// report's warnings.
pub(crate) fn reload_lines(
    report: &ScriptReload,
    before: &ScriptSources,
    scene: &Scene,
) -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    for path in &report.reloaded {
        let objects = scene
            .entities
            .iter()
            .filter(|e| e.scripts.iter().any(|s| s.path == *path))
            .count();
        lines.push((
            match objects {
                0 => format!("Script recarregado: {path}"),
                1 => format!("Script recarregado: {path} (1 objeto)"),
                n => format!("Script recarregado: {path} ({n} objetos)"),
            },
            false,
        ));
    }
    for error in &report.errors {
        let stop = if error.message.ends_with(['.', '?', '!']) {
            ""
        } else {
            "."
        };
        let line = if before.get(&error.path).is_some() {
            format!("Script não recarregado: {error}{stop} O teste segue com a versão anterior.")
        } else {
            format!("Script com erro: {error}{stop}")
        };
        lines.push((line, true));
    }
    lines.extend(report.warnings.iter().map(|w| (w.clone(), false)));
    lines
}

/// The Console's lines about the files of `after` that cannot be scripts now, newly or for
/// another reason than in `before`, that the reload's `report` did not name: a script that ran
/// goes on with its old text, and a new one stays out.
pub(crate) fn unreadable_lines(
    report: &ScriptReload,
    before: &ScriptSources,
    after: &ScriptSources,
) -> Vec<(String, bool)> {
    after
        .unreadable
        .iter()
        .filter(|(path, reason)| {
            before.unreadable.get(*path) != Some(*reason)
                && !report.errors.iter().any(|error| error.path == **path)
        })
        .map(|(path, reason)| {
            let line = if before.get(path).is_some() {
                format!(
                    "Script não recarregado: {path} {reason}. O teste segue com a versão anterior."
                )
            } else {
                format!("Script com erro: {path} {reason}.")
            };
            (line, true)
        })
        .collect()
}

impl Editor {
    /// Watches the scripts of the project's tree during the test that just started with
    /// `running`, when the project has a folder and its objects attach a script (only then can a
    /// reload matter: the document does not change during a test). It stops with the test.
    pub(crate) fn watch_test_scripts(&mut self, running: Arc<ScriptSources>) {
        let root = self.path.as_deref().and_then(Path::parent);
        self.play.script_watch = match root {
            Some(root) if oxy_core::script::any_attached(&self.state.project) => {
                ScriptWatch::start(root.to_owned(), running, self.context.clone())
            }
            _ => None,
        };
    }

    /// Gives the test the script changes the watcher found, before this frame's step, and tells
    /// the Console.
    pub(crate) fn reload_test_scripts(&mut self) {
        let play = &mut self.play;
        let (Some(watch), Some(runtime)) = (&mut play.script_watch, &mut play.runtime) else {
            return;
        };
        let mut lines = Vec::new();
        for change in watch.changes.try_iter() {
            match change {
                Change::Sources(sources) => {
                    let before = std::mem::replace(&mut watch.running, sources.clone());
                    let report = runtime.reload_scripts(sources.clone());
                    lines.extend(reload_lines(&report, &before, runtime.scene()));
                    lines.extend(unreadable_lines(&report, &before, &sources));
                }
                Change::Unreadable(error) => {
                    lines.push((format!("Scripts não recarregados: {error}"), true));
                }
            }
        }
        for (line, error) in lines {
            if error {
                self.warn(line);
            } else {
                self.log(line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::runtime::{FIXED_DT, InputFrame};
    use oxy_core::script::ScriptRef;

    const PATH: &str = "scripts/porta.luau";
    const DOOR: &str =
        "local M = {}\nfunction M.on_step(self) self.attr.Passos += 1 end\nreturn M\n";

    fn folder(text: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("oxy-editor-watch-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join(PATH), text).unwrap();
        root
    }

    #[test]
    fn a_change_is_read_once_it_held_still_and_the_same_text_is_none() {
        let root = folder(DOOR);
        let known = Arc::new(persistence::load_scripts(&root).unwrap());
        let mut scanner = Scanner::new(root.clone(), known);
        // The first looks find what the test started with.
        assert_eq!(scanner.look(), None);
        assert_eq!(scanner.look(), None);
        // A new text waits for a second look that sees the same file.
        let second = DOOR.replace("+= 1", "+= 20");
        std::fs::write(root.join(PATH), &second).unwrap();
        assert_eq!(scanner.look(), None);
        let Some(Change::Sources(sources)) = scanner.look() else {
            panic!("the new text");
        };
        assert_eq!(sources.get(PATH).unwrap().text, second);
        assert_eq!(scanner.look(), None, "told once");
        // The same text written again is no change.
        std::fs::write(root.join(PATH), format!("{second}\n")).unwrap();
        std::fs::write(root.join(PATH), &second).unwrap();
        assert_eq!((scanner.look(), scanner.look()), (None, None));
        // A file that cannot be read is no script, with why, and the others are read; it is
        // told once, and a fix reads again.
        std::fs::write(root.join("scripts/binario.luau"), [0xff, 0xfe]).unwrap();
        scanner.look();
        let Some(Change::Sources(sources)) = scanner.look() else {
            panic!("the unreadable file");
        };
        assert_eq!(
            sources.unreadable["scripts/binario.luau"],
            "não é texto UTF-8"
        );
        assert_eq!(sources.get(PATH).unwrap().text, second);
        assert_eq!((scanner.look(), scanner.look()), (None, None));
        // Until its stamp changes, the scripts are not read again at every look.
        assert!(!sources.held() && scanner.read.is_some());
        std::fs::remove_file(root.join("scripts/binario.luau")).unwrap();
        std::fs::write(root.join("scripts/nova.luau"), "return {}\n").unwrap();
        scanner.look();
        let Some(Change::Sources(sources)) = scanner.look() else {
            panic!("the scripts read again");
        };
        assert!(sources.get("scripts/nova.luau").is_some() && sources.unreadable.is_empty());
        // Scripts in any folder of the tree; what starts with a dot is none.
        std::fs::create_dir_all(root.join("Pista/Checkpoints")).unwrap();
        std::fs::write(root.join("Pista/Checkpoints/marco.luau"), "return {}\n").unwrap();
        std::fs::create_dir_all(root.join(".oxy/agente")).unwrap();
        std::fs::write(root.join(".oxy/agente/marco.luau"), "return {}\n").unwrap();
        scanner.look();
        let Some(Change::Sources(sources)) = scanner.look() else {
            panic!("the script of Pista");
        };
        assert_eq!(
            sources.files.keys().collect::<Vec<_>>(),
            ["Pista/Checkpoints/marco.luau", "scripts/nova.luau", PATH]
        );
        // A tree that cannot be walked is told once.
        std::fs::remove_dir_all(&root).unwrap();
        let Some(Change::Unreadable(error)) = scanner.look() else {
            panic!("the tree gone");
        };
        assert!(
            error.starts_with("Não foi possível ler a pasta do projeto"),
            "{error}"
        );
        assert_eq!(scanner.look(), None);
    }

    #[test]
    fn a_file_that_can_no_longer_be_read_is_told_once_with_why() {
        let mut before = ScriptSources::from_texts([(PATH, DOOR)]);
        let mut after = ScriptSources::default();
        after
            .unreadable
            .insert(PATH.into(), "tem mais de 1 MB".into());
        after
            .unreadable
            .insert("Pista/nova.luau".into(), "não é texto UTF-8".into());
        let report = ScriptReload::default();
        assert_eq!(
            unreadable_lines(&report, &before, &after),
            [
                (
                    "Script com erro: Pista/nova.luau não é texto UTF-8.".to_owned(),
                    true
                ),
                (
                    "Script não recarregado: scripts/porta.luau tem mais de 1 MB. O teste segue \
                     com a versão anterior."
                        .to_owned(),
                    true
                ),
            ]
        );
        // Named by the reload already, or as it was before: nothing more.
        let named = ScriptReload {
            errors: vec![oxy_core::script::check(PATH, "local x = (\n").unwrap_err()],
            ..Default::default()
        };
        before.unreadable = after.unreadable.clone();
        before.unreadable.remove(PATH);
        assert!(unreadable_lines(&named, &before, &after).is_empty());
    }

    /// A test of `project` running the folder at `root`, a step in.
    fn test_of(project: &Project, sources: &Arc<ScriptSources>) -> Runtime {
        let options = RuntimeOptions {
            scripts: sources.clone(),
            ..Default::default()
        };
        let mut runtime = Runtime::with_options(project, &project.start_scene, options).unwrap();
        runtime.advance(FIXED_DT, &InputFrame::default());
        runtime
    }

    #[test]
    fn the_console_names_the_reloaded_scripts_and_the_errors_with_their_line() {
        let root = folder(DOOR);
        let mut project = Project::new("Recarga");
        for name in ["Porta", "Porta 2"] {
            let mut door = Entity::new(name, None);
            door.attributes.insert("Passos".into(), Value::Number(0.));
            door.scripts = vec![ScriptRef::new(PATH)];
            project.scenes[0].entities.push(door);
        }
        let before = Arc::new(persistence::load_scripts(&root).unwrap());
        let mut runtime = test_of(&project, &before);
        let id = project.scenes[0].entities[0].id.clone();
        let steps =
            |runtime: &Runtime| runtime.scene().entity(&id).unwrap().attributes["Passos"].clone();
        assert_eq!(steps(&runtime), Value::Number(1.));
        let reload = |runtime: &mut Runtime, text: &str, before: &ScriptSources| {
            let sources = Arc::new(ScriptSources::from_texts([(PATH, text)]));
            let report = runtime.reload_scripts(sources);
            reload_lines(&report, before, runtime.scene())
        };
        let second = DOOR.replace("+= 1", "+= 10");
        assert_eq!(
            reload(&mut runtime, &second, &before),
            [(
                "Script recarregado: scripts/porta.luau (2 objetos)".to_owned(),
                false
            )]
        );
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(
            steps(&runtime),
            Value::Number(11.),
            "the game went on where it was"
        );
        // A text that does not load: its file and line, and the old text goes on.
        let lines = reload(&mut runtime, "local M = {\nreturn M\n", &before);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0]
                .0
                .starts_with("Script não recarregado: scripts/porta.luau:2: erro de sintaxe")
                && lines[0].0.ends_with("O teste segue com a versão anterior.")
                && lines[0].1,
            "{lines:?}"
        );
        runtime.advance(FIXED_DT, &InputFrame::default());
        assert_eq!(steps(&runtime), Value::Number(21.));
        // A new file that does not load has no old text.
        let mut more = (*before).clone();
        more.files.remove(PATH);
        let report = ScriptReload {
            errors: vec![
                oxy_core::script::check("scripts/nova.luau", "local x = (\n").unwrap_err(),
            ],
            ..Default::default()
        };
        let lines = reload_lines(&report, &more, runtime.scene());
        assert!(
            lines[0]
                .0
                .starts_with("Script com erro: scripts/nova.luau:"),
            "{lines:?}"
        );
        // An error while the test runs shows in its log with the file and line too.
        let broken =
            "local M = {}\nfunction M.on_step(self)\n    self.attr.Nada = 1\nend\nreturn M\n";
        reload(&mut runtime, broken, &before);
        runtime.advance(FIXED_DT, &InputFrame::default());
        let line = runtime.logs.last().unwrap();
        assert!(
            line.text
                .contains(" · scripts/porta.luau:3: o atributo Nada não existe"),
            "{}",
            line.text
        );
        assert_eq!(line.script.as_ref().map(|s| s.line), Some(3));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_watcher_hands_a_saved_file_to_the_test() {
        let root = folder(DOOR);
        let running = Arc::new(persistence::load_scripts(&root).unwrap());
        let watch =
            ScriptWatch::start(root.clone(), running.clone(), egui::Context::default()).unwrap();
        let second = DOOR.replace("+= 1", "+= 30");
        std::fs::write(root.join(PATH), &second).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let change = loop {
            if let Ok(change) = watch.changes.try_recv() {
                break change;
            }
            assert!(Instant::now() < deadline, "no change seen");
            std::thread::sleep(Duration::from_millis(50));
        };
        let Change::Sources(sources) = change else {
            panic!("{change:?}");
        };
        assert_eq!(sources.get(PATH).unwrap().text, second);
        drop(watch);
        std::fs::remove_dir_all(root).unwrap();
    }
}
