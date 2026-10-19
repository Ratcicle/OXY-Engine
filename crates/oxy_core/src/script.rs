//! Scripts: Luau files in the project's `scripts` folder, with human names and subfolders, that
//! objects attach. The disk is the truth: the folder is the list of scripts, and the project
//! file keeps only each object's references, in the order they run. A script is known, in the
//! project, logs and agents, by its path from the project's folder with `/`, as
//! `scripts/porta.luau`. Values per object stay in its attributes.
//!
//! What game scripts reach, by names in English with underscores (decision Q5; objects,
//! attributes, sounds, actions and variables keep the names the person gave them):
//! - the callbacks of [`Callback`], `self` being the object: `on_start(self)`,
//!   `on_area_enter(self, other)`, `on_input(self, action, phase)` (the action's name and
//!   `"pressed"` or `"released"`) and the others;
//! - objects (`handles`): `obj.name`, `obj.attr`, `obj.position`, `obj.world_position`,
//!   `obj:exists()`, `obj:has_attribute(name)`, `self.state` and `obj:send(name, value)`;
//! - timers (`timers`): `self:after`, `self:every`, `self:sequence`, `self:schedule` and
//!   `self:cancel`;
//! - the global tables of `api`: `audio.play(name, { volume = …, audience = obj })`,
//!   `vars.get(name, key?)` and `vars.set(name, value, key?)`;
//! - `print` and `warn`, which log the object's name, the text and the script line.
//!
//! A module may declare `Module.attributes = { Velocidade = 90 }`, the attributes attaching it
//! gives an object that lacks them, with those defaults (decision Q6; see
//! [`declared_attributes`]), and `Module.state`, its memory. Only an attached script's
//! `Module.attributes` is read, so a table of content that is only required may hold an
//! `attributes` of its own.
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
use crate::document::{Id, Project, Value};
use crate::input_actions::every_entity;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

mod api;
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
/// never the runtime's log, whose lines are part of the state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptReload {
    /// The scripts whose new text runs now, by path.
    pub reloaded: Vec<String>,
    /// Why a new text did not load: the old one goes on running (a new file has none).
    pub errors: Vec<ScriptError>,
    /// Memory fields reset or dropped, functions named like callbacks that are none, attributes
    /// a script now declares that its objects lack.
    pub warnings: Vec<String>,
}

/// What [`check`] reads from a script that compiles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptCheck {
    /// The global paths the script reads, as `math.floor`, for the lint of names.
    pub imports: Vec<String>,
}

/// Compiles `source` as the script `path` (`scripts/porta.luau`) and checks it the way loading
/// does before running anything: syntax and top-level locals some function writes. Nothing runs,
/// so it needs no other script; the export refuses a script it refuses. [`check_loading`] goes
/// on to run the module's top.
pub fn check(path: &str, source: &str) -> Result<ScriptCheck, ScriptError> {
    let (_, report) = load::check(path, source)?;
    Ok(ScriptCheck {
        imports: report.imports.into_iter().collect(),
    })
}

/// What [`check_loading`] found in a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptReport {
    pub path: String,
    /// Why it does not load: the objects that attach it would run without it.
    pub error: Option<ScriptError>,
    /// What loads but is likely a mistake: a function named like a callback that is none,
    /// which never runs by itself, and a `Module.attributes` that declares nothing attaching
    /// could create.
    pub warnings: Vec<ScriptError>,
}

/// Checks the scripts `paths` of `sources` as the runtime loads them, in one VM of their own
/// where nothing of the world is reached: [`check`], then the module's top runs (the scripts it
/// requires load with it) and everything it reaches is frozen, and its memory declaration is
/// read. A script `sources` lacks does not load. The warnings are those the runtime gives where
/// the script is attached, so a script no object attaches (`attached` says which some do) that
/// another script of `sources` requires gets none: it is a module of functions and data, whose
/// `on_*` names and `attributes` are its own.
pub fn check_loading<'a>(
    sources: &ScriptSources,
    paths: impl IntoIterator<Item = &'a str>,
    attached: impl Fn(&str) -> bool,
) -> Vec<ScriptReport> {
    let host = ScriptHost::new(std::sync::Arc::new(sources.clone()));
    let mut reports: Vec<ScriptReport> = paths
        .into_iter()
        .map(|path| {
            let mut report = ScriptReport {
                path: path.to_owned(),
                error: None,
                warnings: Vec::new(),
            };
            let loaded = match &host {
                Ok(host) => host.load(path),
                Err(error) => Err(ScriptError::new(path, 0, error.clone())),
            };
            match loaded {
                Ok(module) => {
                    for (name, line, near) in &module.unknown {
                        report
                            .warnings
                            .push(unknown_callback(path, name, *line, *near));
                    }
                    if let Err(message) = &module.attributes {
                        report.warnings.push(undeclared_attributes(path, message));
                    }
                }
                Err(error) => report.error = Some(error),
            }
            report
        })
        .collect();
    // Which of the scripts no object attaches others require: the tops of all of them tell.
    if let Ok(host) = &host
        && reports
            .iter()
            .any(|report| !report.warnings.is_empty() && !attached(&report.path))
    {
        for path in sources.files.keys() {
            let _ = host.load(path);
        }
        let required = host.required_by_others();
        for report in &mut reports {
            if !attached(&report.path) && required.contains(&report.path) {
                report.warnings.clear();
            }
        }
    }
    reports
}

/// The scripts the objects of `project` attach, in scenes and models.
pub fn attached_paths(project: &Project) -> BTreeSet<String> {
    every_entity(project)
        .flat_map(|(_, e)| e.scripts.iter().map(|s| s.path.clone()))
        .collect()
}

/// A function of the module `path`, defined at `line`, named like a callback that is none
/// (`near` the one it likely means): it never runs by itself.
pub(crate) fn unknown_callback(
    path: &str,
    name: &str,
    line: u32,
    near: Option<&str>,
) -> ScriptError {
    let hint = near.map_or_else(String::new, |near| format!("; quis dizer {near}?"));
    ScriptError::new(
        path,
        line,
        format!("{name} não é um evento dos scripts e nunca roda sozinha{hint}"),
    )
}

/// A `Module.attributes` of the script `path` that is no table of defaults, why in `message`.
pub(crate) fn undeclared_attributes(path: &str, message: &str) -> ScriptError {
    ScriptError::new(
        path,
        0,
        format!("{message}; anexar o script não cria os atributos que ele declara"),
    )
}

/// The types of the scripts' API for the luau-lsp extension of VS Code (design §5.3, §6.2):
/// globals and a definitions file of `export type`, `declare name: type` and `declare
/// function` only, the one form both of Luau's solvers in luau-lsp 1.70.1 read. The vertical
/// slice of 0.7 has the API its scripts reach; the full API comes with the table it is made
/// from. Without the header line `script_files` writes before it.
pub const TYPES: &str = include_str!("script/oxy.d.luau");

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

/// A script an object attaches that the project's folder lacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingScript {
    /// The object's ID, which tells two objects of one name apart; never shown.
    pub object: Id,
    /// The object as messages name it: a model's piece after its model.
    pub name: String,
    pub path: String,
}

impl std::fmt::Display for MissingScript {
    /// `Porta: o script scripts/porta.luau não existe na pasta do projeto`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: o script {} não existe na pasta do projeto",
            self.name, self.path
        )
    }
}

/// The scripts the objects of `project` attach, in scenes and models, that `exists` does not
/// find, in the order of the scenes, the models and their objects.
pub fn missing(project: &Project, exists: impl Fn(&str) -> bool) -> Vec<MissingScript> {
    every_entity(project)
        .flat_map(|(holder, e)| {
            let name = if holder.model {
                format!("Modelo {holder}, {}", e.name)
            } else {
                e.name.clone()
            };
            let exists = &exists;
            e.scripts
                .iter()
                .filter(move |s| !exists(&s.path))
                .map(move |s| MissingScript {
                    object: e.id.clone(),
                    name: name.clone(),
                    path: s.path.clone(),
                })
        })
        .collect()
}

/// Objects a line about one missing file names before it counts the rest.
const OBJECTS_LISTED: usize = 3;

/// `missing` told once per file, in the order the files first come: the objects that attach
/// it, the first few by name and then how many more, and its path, as `Porta e Porta 2: o
/// script scripts/porta.luau não existe na pasta do projeto`; with how many objects attach it.
/// One file shared by many objects is one line.
pub fn missing_by_file(missing: &[MissingScript]) -> Vec<(String, usize)> {
    let mut files: Vec<(&str, Vec<&str>)> = Vec::new();
    for found in missing {
        match files.iter_mut().find(|(path, _)| *path == found.path) {
            Some((_, names)) => names.push(&found.name),
            None => files.push((&found.path, vec![&found.name])),
        }
    }
    files
        .into_iter()
        .map(|(path, names)| {
            let objects = match names.as_slice() {
                [one] => (*one).to_owned(),
                [first @ .., last] if names.len() <= OBJECTS_LISTED => {
                    format!("{} e {last}", first.join(", "))
                }
                _ => format!(
                    "{} e mais {} objetos",
                    names[..OBJECTS_LISTED].join(", "),
                    names.len() - OBJECTS_LISTED
                ),
            };
            (
                format!("{objects}: o script {path} não existe na pasta do projeto"),
                names.len(),
            )
        })
        .collect()
}

/// The first script an object of `project` attaches, in a scene or a model, that `exists` does
/// not find: the object as messages name it (a model's piece after its model) and the path.
pub fn first_missing(project: &Project, exists: impl Fn(&str) -> bool) -> Option<(String, String)> {
    missing(project, exists)
        .into_iter()
        .next()
        .map(|found| (found.name, found.path))
}

/// The attributes a script declares in `Module.attributes`: each one's name and default value,
/// by name.
pub type Declared = Vec<(String, Value)>;

/// What `Module.attributes` declares in each of the scripts `paths` of `sources` (decision Q6),
/// or why it cannot be read: the script does not load, or the declaration is not a table of
/// defaults. The scripts load in a VM of their own, as the runtime loads them, nothing of the
/// world reached.
pub fn declared_attributes<'a>(
    sources: &ScriptSources,
    paths: impl IntoIterator<Item = &'a str>,
) -> Vec<(&'a str, Result<Declared, ScriptError>)> {
    let host = ScriptHost::new(std::sync::Arc::new(sources.clone()));
    paths
        .into_iter()
        .map(|path| {
            let declared = match &host {
                Ok(host) => host.load(path).and_then(|module| {
                    module
                        .attributes
                        .clone()
                        .map_err(|message| ScriptError::new(path, 0, message))
                }),
                Err(error) => Err(ScriptError::new(path, 0, error.clone())),
            };
            (path, declared)
        })
        .collect()
}

/// How messages name the type of `value`: "número", "texto", "booleano"…
pub(crate) fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "número",
        Value::Text(_) => "texto",
        Value::Bool(_) => "booleano",
        Value::Vector3(_) => "vetor",
        Value::Vector2(_) => "vetor 2D",
        Value::Object(_) => "objeto",
        Value::Surface(_) => "superfície",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Entity, Primitive, new_id, validate_project};

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
            Some(("Peça".to_owned(), "scripts/peça.luau".to_owned()))
        );
        // Every one, the scene's objects before the models' pieces.
        let all = missing(&project, |_| false);
        assert_eq!(
            all.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [
                "Peça: o script scripts/peça.luau não existe na pasta do projeto",
                "Modelo Bloco, Peça: o script scripts/peça.luau não existe na pasta do projeto",
            ]
        );
        assert_eq!(all[0].object, id);
        project.scenes[0].entities.clear();
        assert_eq!(
            first_missing(&project, |path| path != "scripts/peça.luau"),
            Some((
                "Modelo Bloco, Peça".to_owned(),
                "scripts/peça.luau".to_owned()
            ))
        );
        project.assets.clear();
        assert!(!any_attached(&project));
    }

    #[test]
    fn missing_scripts_are_told_once_per_file_naming_a_few_objects() {
        let found = |name: &str, path: &str| MissingScript {
            object: new_id(),
            name: name.into(),
            path: path.into(),
        };
        let mut missing = vec![
            found("Porta", "scripts/porta.luau"),
            found("Gatilho 1", "scripts/gatilho.luau"),
            found("Porta 2", "scripts/porta.luau"),
            found("Chave", "scripts/chave.luau"),
        ];
        missing.extend((2..=12).map(|n| found(&format!("Gatilho {n}"), "scripts/gatilho.luau")));
        assert_eq!(
            missing_by_file(&missing),
            [
                (
                    "Porta e Porta 2: o script scripts/porta.luau não existe na pasta do projeto"
                        .to_owned(),
                    2
                ),
                (
                    "Gatilho 1, Gatilho 2, Gatilho 3 e mais 9 objetos: o script \
                     scripts/gatilho.luau não existe na pasta do projeto"
                        .to_owned(),
                    12
                ),
                (
                    "Chave: o script scripts/chave.luau não existe na pasta do projeto".to_owned(),
                    1
                ),
            ]
        );
        assert_eq!(missing_by_file(&missing[3..4])[0].0, missing[3].to_string());
        let three = [&missing[1], &missing[4], &missing[5]].map(Clone::clone);
        assert_eq!(
            missing_by_file(&three)[0].0,
            "Gatilho 1, Gatilho 2 e Gatilho 3: o script scripts/gatilho.luau não existe na pasta \
             do projeto"
        );
    }

    /// The names a block of the definitions file declares, from the line `start` to its `}`.
    fn declared(start: &str) -> Vec<String> {
        let body = TYPES.split(start).nth(1).expect(start);
        let body = &body[..body.find("\n}").expect("the block's end")];
        let mut names: Vec<String> = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("--"))
            .filter_map(|line| Some(line.split_once(':')?.0.trim().to_owned()))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_types_declare_what_scripts_reach() {
        let mut members: Vec<String> = handles::OBJECT_MEMBERS.map(String::from).to_vec();
        members.sort();
        assert_eq!(declared("export type Object = {"), members);
        assert_eq!(declared("export type ScriptObject<A, S> = {"), members);
        // The global tables as the VM has them, and the base functions they declare.
        let host = ScriptHost::new(std::sync::Arc::default()).unwrap();
        let globals = host.lua().globals();
        for table in ["audio", "vars"] {
            let found: mlua::Table = globals.get(table).unwrap();
            let mut names: Vec<String> = found
                .pairs::<String, mlua::Value>()
                .map(|pair| pair.unwrap().0)
                .collect();
            names.sort();
            assert_eq!(declared(&format!("declare {table}: {{")), names, "{table}");
        }
        assert!(TYPES.contains("declare function warn(...: any): ()"));
        assert!(globals.get::<mlua::Function>("warn").is_ok());
        // Every callback is listed for the person.
        for callback in Callback::ALL {
            assert!(
                TYPES.contains(&format!("--   {}(self", callback.name())),
                "{callback:?}"
            );
        }
        // Only the syntax luau-lsp 1.70.1 reads in both solvers (design §5.3).
        for line in TYPES
            .lines()
            .filter(|l| !l.starts_with(' ') && !l.starts_with("--"))
        {
            assert!(
                line.is_empty()
                    || line == "}"
                    || line.starts_with("export type ")
                    || line.starts_with("declare function ")
                    || (line.starts_with("declare ") && line.ends_with(": {")),
                "{line}"
            );
        }
        assert!(!TYPES.contains("declare class") && !TYPES.contains("declare extern"));
    }

    #[test]
    fn checking_loads_the_scripts_and_tells_what_never_runs() {
        let sources = ScriptSources::from_texts([
            (
                "scripts/porta.luau",
                "local Dados = require('./dados')\nlocal M = {}\nM.attributes = 3\nfunction \
                 M.on_aera_enter(self) end\nreturn M\n",
            ),
            // A module only required: its on_* names and attributes are its own.
            (
                "scripts/dados.luau",
                "local M = { attributes = { 'forca', 'vida' } }\nfunction M.on_dano(self) \
                 end\nreturn M\n",
            ),
            ("scripts/topo.luau", "audio.play('Bipe')\nreturn {}\n"),
            ("scripts/sintaxe.luau", "local M = {\nreturn M\n"),
            ("scripts/sem_tabela.luau", "local x = 1\n"),
        ]);
        let reports = check_loading(
            &sources,
            [
                "scripts/porta.luau",
                "scripts/topo.luau",
                "scripts/sintaxe.luau",
                "scripts/sem_tabela.luau",
                "scripts/nada.luau",
                "scripts/dados.luau",
            ],
            |_| false,
        );
        let texts = |report: &ScriptReport| {
            (
                report.error.as_ref().map(ToString::to_string),
                report
                    .warnings
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            texts(&reports[0]),
            (
                None,
                vec![
                    "scripts/porta.luau:4: on_aera_enter não é um evento dos scripts e nunca roda \
                     sozinha; quis dizer on_area_enter?"
                        .to_owned(),
                    "scripts/porta.luau: Modulo.attributes precisa ser uma tabela com o valor \
                     padrão de cada atributo, como { Velocidade = 90 }; anexar o script não cria \
                     os atributos que ele declara"
                        .to_owned(),
                ]
            )
        );
        let error = |i: usize| reports[i].error.clone().unwrap();
        assert_eq!(
            error(1).to_string(),
            "scripts/topo.luau:1: o topo do script não pode mexer no mundo; faça isso em on_start"
        );
        assert_eq!(
            (error(2).line, error(2).path.as_str()),
            (2, "scripts/sintaxe.luau")
        );
        assert!(
            error(2).message.starts_with("erro de sintaxe"),
            "{}",
            error(2)
        );
        assert!(error(3).message.contains("return Modulo"), "{}", error(3));
        assert_eq!(error(4).message, "o script não existe na pasta do projeto");
        assert_eq!(texts(&reports[5]), (None, Vec::new()));
        // Alone, the other scripts tell that one requires it; attached, it is warned of.
        let alone = check_loading(&sources, ["scripts/dados.luau"], |_| false);
        assert_eq!(texts(&alone[0]), (None, Vec::new()));
        let attached = check_loading(&sources, ["scripts/dados.luau"], |path| {
            path == "scripts/dados.luau"
        });
        assert_eq!(attached[0].warnings.len(), 2, "{:?}", attached[0].warnings);
        assert!(
            attached[0].warnings[0]
                .message
                .starts_with("on_dano não é um evento")
        );
    }

    #[test]
    fn a_text_hashes_the_same_wherever_it_is_read() {
        let file = ScriptFile::new("print(\"oi\")\n".into());
        assert_eq!(file, ScriptFile::new("print(\"oi\")\n".into()));
        assert_ne!(file.hash, ScriptFile::new("print(\"olá\")\n".into()).hash);
        assert_eq!(file.hash, crate::random::hash(&[b"print(\"oi\")\n"]));
    }
}
