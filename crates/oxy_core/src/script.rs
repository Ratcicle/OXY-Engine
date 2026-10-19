//! Scripts: Luau files in the project's `scripts` folder, with human names and subfolders, that
//! objects attach. The disk is the truth: the folder is the list of scripts, and the project
//! file keeps only each object's references, in the order they run. A script is known, in the
//! project, logs and agents, by its path from the project's folder with `/`, as
//! `scripts/porta.luau`. Values per object stay in its attributes.
//!
//! The host runs them in Luau through mlua (`host`, `sandbox`, `load`, `handles`), one VM per
//! `Runtime`, created only when its scene attaches a script. The VM keeps the compiled modules,
//! frozen, and nothing of the game, which lives in the engine's state: a module's functions
//! with the fixed names of [`Callback`] hear events as tasks of the runtime, its timers
//! (`timers`) are tasks too, which name the function to call, its memory (`memory`) is the
//! engine's per object, and its messages (`messages`) wait in the engine's queue. So a VM made
//! anew, for a reload or to check that no script hides state in it, runs on where the last one
//! stopped. Its `unsafe` code keeps two rules:
//! - Aliasing: a call lends the VM the `Runtime` through a pointer that a guard sets from a
//!   `&mut Runtime` it keeps borrowed until the call ends, so the caller cannot touch the
//!   `Runtime` meanwhile; the host is out of the `Runtime` (`Option::take`) during the call, so
//!   nothing reaches the VM through it; Luau runs on one thread and no function scripts call
//!   calls back into Luau while it holds the `Runtime`, so one reference lives at a time.
//!   Outside a call the pointer is null and the functions that reach the world refuse.
//! - C++ exceptions: the Luau of mlua-sys raises errors as C++ exceptions, and a foreign
//!   exception reaching a `catch_unwind` aborts the process. So no call of Luau's C API that
//!   can raise (an error, an allocation) sits inside a `catch_unwind`: Rust work behind the
//!   panic barriers touches no Lua stack (or only mlua's safe API, which catches the exceptions
//!   itself), raw C code runs inside `exec_raw` or a C function Luau calls, and errors are
//!   raised after the Rust values are dropped. The VM is created with
//!   `catch_rust_panics(false)`, so a script's `pcall` never hides a panic, and each door marks
//!   the host when it catches one (`coroutine.resume` still swallows them).
use crate::document::{Project, Value};
use crate::input_actions::every_entity;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

mod bytecode;
mod errors;
mod handles;
mod host;
#[cfg(test)]
mod host_tests;
mod load;
pub(crate) mod memory;
pub(crate) mod messages;
mod sandbox;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod task_tests;
pub(crate) mod timers;

pub use errors::{ScriptError, ScriptErrorKind};
pub(crate) use handles::collision;
pub use host::ScriptCall;
pub(crate) use host::{Module, ScriptHost};
pub(crate) use load::check as load_check;
pub(crate) use memory::{Memory, ScriptValue};
pub(crate) use messages::{Message, Unsent};
pub(crate) use timers::{Repeat, ScriptTask, Share, Timer, TimerRoom, TimerStep};

/// The fixed names a module's table gives the functions that hear events (design §3.6). The
/// runtime calls them in tasks, on the object's turn in scene order, after its graph and in the
/// order of its list of scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Callback {
    /// The scene starts, or a copy that attaches the script appears.
    Start,
    Step,
    Input,
    Click,
    AreaEnter,
    AreaExit,
    Marker,
    Character,
    Pause,
    /// A message another object (or the object itself) sent.
    Message,
}

impl Callback {
    pub(crate) const ALL: [Self; 10] = [
        Self::Start,
        Self::Step,
        Self::Input,
        Self::Click,
        Self::AreaEnter,
        Self::AreaExit,
        Self::Marker,
        Self::Character,
        Self::Pause,
        Self::Message,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Start => "on_start",
            Self::Step => "on_step",
            Self::Input => "on_input",
            Self::Click => "on_click",
            Self::AreaEnter => "on_area_enter",
            Self::AreaExit => "on_area_exit",
            Self::Marker => "on_marker",
            Self::Character => "on_character",
            Self::Pause => "on_pause",
            Self::Message => "on_message",
        }
    }

    /// The graph's event for the same happening, by which the runtime knows what its scene
    /// hears (whether Esc pauses the game, whether exits are worked out); none for messages,
    /// which the graph never hears.
    pub(crate) fn event(self) -> Option<&'static str> {
        Some(match self {
            Self::Start => "event.scene_start",
            Self::Step => "event.step",
            Self::Input => "event.input",
            Self::Click => "event.click",
            Self::AreaEnter => "event.area_enter",
            Self::AreaExit => "event.area_exit",
            Self::Marker => "event.animation",
            Self::Character => "event.character",
            Self::Pause => "event.pause",
            Self::Message => return None,
        })
    }
}

/// Callback names a module may already define that the engine calls in a later version
/// (players in 0.8): no warning for them.
pub(crate) const LATER_CALLBACKS: [&str; 3] = ["on_request", "on_player_join", "on_player_leave"];

/// The callbacks a module defines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Callbacks(u16);

impl Callbacks {
    pub(crate) fn insert(&mut self, callback: Callback) {
        self.0 |= 1 << callback as u16;
    }
    pub(crate) fn contains(self, callback: Callback) -> bool {
        self.0 & (1 << callback as u16) != 0
    }
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub(crate) fn iter(self) -> impl Iterator<Item = Callback> {
        Callback::ALL.into_iter().filter(move |c| self.contains(*c))
    }
}

/// A value a script task passes its function after `self`: the values attributes hold, nil,
/// the records of named values the engine builds for an event (`on_character`'s), which reach
/// the script as a table, and a message's value, as memory keeps it. Timers carry only the
/// first two, so their data is what the engine keeps and compares.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScriptArg {
    Nil,
    Value(Value),
    Record(Vec<(&'static str, Value)>),
    Data(ScriptValue),
}

/// What reloading the scripts did (see `Runtime::reload_scripts`), for the editor's Console:
/// never the runtime's log, whose lines are part of the state. Provisional, as the reload.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptReload {
    /// The scripts whose new text runs now, by path.
    pub reloaded: Vec<String>,
    /// Why a new text did not load: the old one goes on running (a new file has none).
    pub errors: Vec<ScriptError>,
    /// Memory fields reset or dropped, functions named like callbacks that are none.
    pub warnings: Vec<String>,
}

/// What [`check`] reads from a script that loads. Provisional, as [`check`].
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptCheck {
    /// The global paths the script reads, as `math.floor`, for the lint of names.
    pub imports: Vec<String>,
}

/// Compiles `source` as the script `path` (`scripts/porta.luau`) and checks it the way loading
/// does before running anything: syntax and top-level locals some function writes.
/// Provisional: the lint of names and the checks of the tools build on it in a later stage;
/// only tests call it now.
#[doc(hidden)]
pub fn check(path: &str, source: &str) -> Result<ScriptCheck, ScriptError> {
    let (_, report) = load::check(path, source)?;
    Ok(ScriptCheck {
        imports: report.imports.into_iter().collect(),
    })
}

/// The scripts' folder, in the project's folder (`data` in an exported game).
pub const FOLDER: &str = "scripts";
/// The extension of script files.
pub const EXTENSION: &str = "luau";
/// Largest script file, in bytes.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// A script an object attaches, by its path in [`normalize_path`]'s form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptRef {
    pub path: String,
}
impl ScriptRef {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// The project's scripts as its folder holds them: each file's text by path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptSources {
    pub files: BTreeMap<String, ScriptFile>,
}
impl ScriptSources {
    pub fn get(&self, path: &str) -> Option<&ScriptFile> {
        self.files.get(path)
    }
    /// Sources given in memory, as tests and tools write them: (path, text).
    pub fn from_texts<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(path, text)| (path.to_owned(), ScriptFile::new(text.to_owned())))
                .collect(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    pub text: String,
    /// The portable hash of the text: the same text has the same one on every machine, which
    /// tells a changed file and a file moved with the same contents.
    pub hash: u64,
}
impl ScriptFile {
    pub fn new(text: String) -> Self {
        let hash = crate::random::hash(&[text.as_bytes()]);
        Self { text, hash }
    }
}

/// `path` as scripts are named: from the project's folder with `/`, inside [`FOLDER`] and ending
/// in `.luau`, as `scripts/fases/porta.luau`. `\` reads as `/`, and `.` and empty parts go;
/// `..`, absolute paths, drives and names Windows would not keep are refused.
pub fn normalize_path(path: &str) -> Result<String, String> {
    let invalid = || {
        format!(
            "caminho de script inválido \"{path}\"; use um arquivo .{EXTENSION} dentro da pasta \
             {FOLDER}, como {FOLDER}/porta.{EXTENSION}"
        )
    };
    let unified = path.trim().replace('\\', "/");
    if unified.starts_with('/') {
        return Err(invalid());
    }
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(invalid()),
            part if part.ends_with(['.', ' '])
                || part
                    .chars()
                    .any(|c| c.is_control() || r#"<>:"|?*"#.contains(c)) =>
            {
                return Err(invalid());
            }
            part => parts.push(part),
        }
    }
    match parts.as_slice() {
        [FOLDER, .., file]
            if file
                .strip_suffix(EXTENSION)
                .and_then(|stem| stem.strip_suffix('.'))
                .is_some_and(|stem| !stem.is_empty()) =>
        {
            Ok(parts.join("/"))
        }
        _ => Err(invalid()),
    }
}

/// Checks the scripts an object attaches: each path written as [`normalize_path`] writes it,
/// and none twice.
pub fn validate(scripts: &[ScriptRef]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for script in scripts {
        let normal = normalize_path(&script.path)?;
        if normal != script.path {
            return Err(format!(
                "o caminho de script \"{}\" deve ser escrito {normal}",
                script.path
            ));
        }
        if !seen.insert(script.path.as_str()) {
            return Err(format!("o script {} está anexado duas vezes", script.path));
        }
    }
    Ok(())
}

/// Whether an object of `project`, in a scene or a model, attaches a script.
pub fn any_attached(project: &Project) -> bool {
    every_entity(project).any(|(_, e)| !e.scripts.is_empty())
}

/// The first script an object of `project` attaches, in a scene or a model, that `exists` does
/// not find: the object as messages name it (a model's piece after its model) and the path.
pub fn first_missing(project: &Project, exists: impl Fn(&str) -> bool) -> Option<(String, &str)> {
    every_entity(project).find_map(|(holder, e)| {
        let script = e.scripts.iter().find(|s| !exists(&s.path))?;
        let object = if holder.model {
            format!("Modelo {holder}, {}", e.name)
        } else {
            e.name.clone()
        };
        Some((object, script.path.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Entity, Primitive, validate_project};

    #[test]
    fn paths_are_normalized_into_the_scripts_folder() {
        for (given, normal) in [
            ("scripts/porta.luau", "scripts/porta.luau"),
            (
                r"scripts\fases\Porta com chave.luau",
                "scripts/fases/Porta com chave.luau",
            ),
            (
                "./scripts//dados/./cartas.luau",
                "scripts/dados/cartas.luau",
            ),
            ("  scripts/ação.luau ", "scripts/ação.luau"),
        ] {
            assert_eq!(normalize_path(given).as_deref(), Ok(normal), "{given}");
        }
        for refused in [
            "",
            "scripts",
            "scripts/",
            "porta.luau",
            "outros/porta.luau",
            "Scripts/porta.luau",
            "scripts/porta.lua",
            "scripts/porta.luau.txt",
            "scripts/.luau",
            "scripts/../porta.luau",
            "../scripts/porta.luau",
            "/scripts/porta.luau",
            r"\\servidor\scripts\porta.luau",
            "C:/projeto/scripts/porta.luau",
            "C:scripts/porta.luau",
            "scripts/por?ta.luau",
            "scripts/fases./porta.luau",
            "scripts/fase /porta.luau",
            "scripts/linha\n.luau",
        ] {
            let error = normalize_path(refused).unwrap_err();
            assert!(
                error.starts_with("caminho de script inválido") && error.contains("scripts/"),
                "{refused}: {error}"
            );
        }
    }

    #[test]
    fn objects_keep_their_scripts_in_order_and_leave_the_field_out_without_any() {
        let mut e = Entity::new("Porta", Some(Primitive::Cube));
        let json = serde_json::to_value(&e).unwrap();
        assert!(json.get("scripts").is_none(), "{json}");
        e.scripts = vec![
            ScriptRef::new("scripts/porta.luau"),
            ScriptRef::new("scripts/comum/som.luau"),
        ];
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(
            json["scripts"],
            serde_json::json!([{"path": "scripts/porta.luau"}, {"path": "scripts/comum/som.luau"}])
        );
        assert_eq!(serde_json::from_value::<Entity>(json.clone()).unwrap(), e);
        let mut unknown = json;
        unknown["scripts"][0]["enabled"] = true.into();
        assert!(serde_json::from_value::<Entity>(unknown).is_err());
    }

    #[test]
    fn projects_refuse_scripts_outside_the_folder_unnormalized_or_twice() {
        let attach = |paths: &[&str]| {
            let mut project = Project::new("Scripts");
            let mut e = Entity::new("Porta", Some(Primitive::Cube));
            e.scripts = paths.iter().copied().map(ScriptRef::new).collect();
            project.scenes[0].entities.push(e);
            validate_project(&project)
        };
        attach(&[]).unwrap();
        attach(&["scripts/porta.luau", "scripts/comum/som.luau"]).unwrap();
        assert_eq!(
            attach(&["../porta.luau"]).unwrap_err(),
            "Porta: caminho de script inválido \"../porta.luau\"; use um arquivo .luau dentro \
             da pasta scripts, como scripts/porta.luau"
        );
        assert_eq!(
            attach(&[r"scripts\porta.luau"]).unwrap_err(),
            "Porta: o caminho de script \"scripts\\porta.luau\" deve ser escrito \
             scripts/porta.luau"
        );
        assert_eq!(
            attach(&["scripts/porta.luau", "scripts/porta.luau"]).unwrap_err(),
            "Porta: o script scripts/porta.luau está anexado duas vezes"
        );
        // A model's pieces follow the same rules.
        let mut project = Project::new("Modelo");
        let scene = project.scenes[0].id.clone();
        let mut piece = Entity::new("Peça", Some(Primitive::Cube));
        piece.scripts = vec![ScriptRef::new("scripts/peça.luau")];
        let id = piece.id.clone();
        project.scenes[0].entities.push(piece);
        project.save_model(&scene, &id, "Bloco").unwrap();
        project.scenes[0].entities.clear();
        validate_project(&project).unwrap();
        project.assets[0].model.as_mut().unwrap()[0].scripts = vec![ScriptRef::new("peça.luau")];
        assert!(
            validate_project(&project)
                .unwrap_err()
                .contains("caminho de script inválido \"peça.luau\"")
        );
    }

    #[test]
    fn the_first_missing_script_names_its_object_and_model() {
        let mut project = Project::new("Ausentes");
        let scene = project.scenes[0].id.clone();
        let mut piece = Entity::new("Peça", Some(Primitive::Cube));
        piece.scripts = vec![ScriptRef::new("scripts/peça.luau")];
        let id = piece.id.clone();
        project.scenes[0].entities.push(piece);
        project.save_model(&scene, &id, "Bloco").unwrap();
        assert!(any_attached(&project));
        assert_eq!(first_missing(&project, |_| true), None);
        assert_eq!(
            first_missing(&project, |_| false),
            Some(("Peça".to_owned(), "scripts/peça.luau"))
        );
        project.scenes[0].entities.clear();
        assert_eq!(
            first_missing(&project, |path| path != "scripts/peça.luau"),
            Some(("Modelo Bloco, Peça".to_owned(), "scripts/peça.luau"))
        );
        project.assets.clear();
        assert!(!any_attached(&project));
    }

    #[test]
    fn a_text_hashes_the_same_wherever_it_is_read() {
        let file = ScriptFile::new("print(\"oi\")\n".into());
        assert_eq!(file, ScriptFile::new("print(\"oi\")\n".into()));
        assert_ne!(file.hash, ScriptFile::new("print(\"olá\")\n".into()).hash);
        assert_eq!(file.hash, crate::random::hash(&[b"print(\"oi\")\n"]));
    }
}
