//! Scripts: the Luau files of the project's tree (see [`crate::project_tree`]), in any folder and
//! with human names, that objects attach. The disk is the truth: the tree is the list of
//! scripts, and the project file keeps only each object's references, in the order they run. A
//! script is known, in the project, logs and agents, by its path from the project's folder with
//! `/`, as `Pista/Checkpoint.luau` (or `scripts/porta.luau`). Values per object stay in its
//! attributes.
//!
//! What game scripts reach, by names in English with underscores (decision Q5; objects,
//! attributes, sounds, actions and variables keep the names the person gave them), is the
//! registry's ([`registry::API`], design §5.7): one row per callback, property, method and
//! function, from which the host installs what scripts call and [`generate`] writes the types,
//! the plan-B module, the Guide's pages and docs/api-scripts.md. In short:
//! - the callbacks of [`Callback`], `self` being the object: `on_start(self)`,
//!   `on_area_enter(self, other)`, `on_input(self, action, phase)` (the action's name and
//!   `"pressed"` or `"released"`) and the others;
//! - objects: their properties (`handles`: `obj.name`, `obj.parent`, `obj.attr`, the transform,
//!   `obj.visible`, `self.state`…) and methods (`objects`: `obj:children()`, `obj:clone()`,
//!   `obj:destroy()`…; `obj:send(name, value)` of `messages`);
//! - timers (`timers`): `self:after`, `self:every`, `self:sequence`, `self:schedule` and
//!   `self:cancel`;
//! - the views of objects (`views`): `obj.character` (its state, and its commands of
//!   `character`), `obj.collider`, `obj.platform`, `obj.controller`, `obj.animation` and
//!   `obj.ui`;
//! - the global tables: `scene` (`objects`: finding objects, models, scene changes), `vmath`
//!   and `quat` (`math`), `physics` (`physics`: the 3D queries), `camera` (`camera`), those of
//!   `game`: `game`, `save`, `input`, `time` and `random`, and those of `api`: `audio` (sounds,
//!   loops and volumes), `vars.get(name, key?)`, `vars.set(name, value, key?)`, and
//!   `param.object()`, `param.surface(name?)` and `param.vector2(x, y)` for declarations;
//! - `print` and `warn`, which log the object's name, the text and the script line, and Luau's
//!   own `error` and `assert`;
//! - `require(path)` of `sandbox`, a frozen module of the project: `./` and `../` from the
//!   script's folder, `@project/` from the project's;
//! - the names of 0.8 (`players`, `game.online`, `on_request`…), kept: using one is an error
//!   that says it comes in 0.8.
//!
//! The tools check the names a script gives with literal texts without running it (`names`,
//! design §6.4): those of the API against the globals scripts have, and those of the project
//! (sounds, variables, attributes, scenes…), where the registry marks a parameter with a
//! [`NameKind`], against the project ([`lint`], warnings only); [`references`] lists the latter
//! for the tools that look for where a sound or a scene is used.
//!
//! A module may declare `Module.attributes = { Velocidade = 90, Alvo = param.object() }`, the
//! attributes attaching it gives an object that lacks them, with those defaults (decision Q6;
//! see [`declared_attributes`]), and `Module.state`, its memory, whose fields the same markers
//! may declare empty (an object, a surface) or as a Vetor2. Only an attached script's
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
use crate::document::{Entity, Id, Project, SceneKind, Value};
use crate::input_actions::every_entity;
use crate::surface::SurfaceMaterial;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

mod api;
mod bytecode;
mod camera;
mod character;
mod errors;
mod game;
pub mod generate;
mod handles;
mod host;
#[cfg(test)]
mod host_tests;
pub mod lexer;
mod load;
mod math;
pub(crate) mod memory;
pub(crate) mod messages;
mod names;
mod objects;
mod physics;
pub mod registry;
mod sandbox;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod task_tests;
pub(crate) mod timers;
mod views;

pub(crate) use errors::closest;
pub use errors::{ScriptError, ScriptErrorKind};
pub(crate) use handles::collision;
pub use host::ScriptCall;
pub(crate) use host::{Module, ScriptHost};
pub(crate) use load::check as load_check;
pub(crate) use memory::{Memory, ScriptValue};
pub(crate) use messages::{Message, Unsent};
pub use names::{
    BrokenCitation, Citation, Found, ProjectNames, Reference, broken_citations, citations,
    declared_attribute_names, lint, names_by_id, references,
};
pub use registry::NameKind;
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
    /// The global paths the script reads, as `math.floor`, which the lint of names checks.
    pub imports: Vec<String>,
    /// The names of the project it gives with literal texts (see [`references`]).
    pub references: Vec<Reference>,
}

/// Compiles `source` as the script `path` (`Pista/porta.luau`) and checks it the way loading
/// does before running anything: syntax and top-level locals some function writes. Nothing runs,
/// so it needs no other script; the export refuses a script it refuses. [`check_loading`] goes
/// on to run the module's top, and [`lint`] checks the names it gives.
pub fn check(path: &str, source: &str) -> Result<ScriptCheck, ScriptError> {
    let (_, report) = load::check(path, source)?;
    Ok(ScriptCheck {
        references: names::references_in(&report.literals),
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
/// (`near` the one it likely means), or like one the engine calls from 0.8: it never runs by
/// itself.
pub(crate) fn unknown_callback(
    path: &str,
    name: &str,
    line: u32,
    near: Option<&str>,
) -> ScriptError {
    if registry::reserved(name).is_some() {
        return ScriptError::new(path, line, registry::later(name));
    }
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
/// function` only, the one form both of Luau's solvers in luau-lsp 1.70.1 read, made from the
/// registry by [`generate::definitions`] (the repository keeps it in `script/oxy.d.luau`).
/// Without the header line `script_files` writes before it.
pub fn types() -> &'static str {
    static TYPES: std::sync::LazyLock<String> = std::sync::LazyLock::new(generate::definitions);
    &TYPES
}

/// The extension of script files.
pub const EXTENSION: &str = "luau";
/// Largest script file, in bytes.
pub const MAX_BYTES: u64 = 1024 * 1024;
/// The alias `require` knows for the project's folder (`data` in an exported game), as in
/// `require("@project/Pista/dados")`: built into the runtime, since an exported game reads no
/// `.luaurc`, and written in the root's `.luaurc` for luau-lsp (see `script_files`).
pub const PROJECT_ALIAS: &str = "project";

/// A script an object attaches, by its path in [`normalize_path`]'s form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptRef {
    pub path: String,
    /// Off, the object keeps the script but runs without it (the Ligado of the editor's Scripts
    /// card); written only when off.
    #[serde(default = "switched_on_default", skip_serializing_if = "is_on")]
    pub enabled: bool,
}
impl ScriptRef {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            enabled: true,
        }
    }
}
fn switched_on_default() -> bool {
    true
}
fn is_on(enabled: &bool) -> bool {
    *enabled
}

/// The project's scripts as its tree holds them: each file's text by path, and the `.luau`
/// files that are no script now, with why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptSources {
    pub files: BTreeMap<String, ScriptFile>,
    /// Why each file the tree has but that cannot run is none, said after its path ("tem mais
    /// de 1 MB", "não é texto UTF-8", "não pôde ser lido agora…"). Attaching or requiring one
    /// is that error; the others only warn, so one bad file never keeps the rest from running.
    pub unreadable: BTreeMap<String, String>,
    /// The folders of the tree whose files these sources cannot have, with why said after the
    /// path of a script in them: one that could not be listed now ("não pôde ser lido agora: a
    /// pasta Pista não pôde ser lida agora…"), a link, another project or an exported game
    /// ("fica fora do projeto: Outro é outro projeto ou um jogo exportado").
    pub folders: BTreeMap<String, String>,
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
            ..Self::default()
        }
    }
    /// The script `path` names as these sources have it: itself, or the only one whose path
    /// differs from it in case alone (Windows opens it, and OXY keeps the case the disk has).
    pub fn known(&self, path: &str) -> Option<&str> {
        if let Some((known, _)) = self.files.get_key_value(path) {
            return Some(known);
        }
        let mut alike = self
            .files
            .keys()
            .filter(|known| crate::names::same_name(known, path));
        match (alike.next(), alike.next()) {
            (Some(known), None) => Some(known),
            _ => None,
        }
    }
    /// Why these sources lack the script `path`, said after its path, when the tree has the file
    /// but it cannot run (see [`Self::unreadable`]) or it is in a folder they could not look
    /// into (see [`Self::folders`]); none when it is not there.
    pub fn why_none(&self, path: &str) -> Option<&str> {
        if let Some(reason) = self.unreadable.get(path) {
            return Some(reason);
        }
        let parts: Vec<&str> = path.split('/').collect();
        self.folders
            .iter()
            .filter(|(folder, _)| {
                let depth = if folder.is_empty() {
                    0
                } else {
                    folder.split('/').count()
                };
                // A link may be the file itself.
                depth <= parts.len() && crate::names::same_name(&parts[..depth].join("/"), folder)
            })
            .max_by_key(|(folder, _)| folder.len())
            .map(|(_, why)| why.as_str())
    }
    /// Whether a file these sources could not read may read now with nothing about it changed:
    /// another program held it, for one (see [`crate::project_tree::NOT_NOW`]).
    pub fn held(&self) -> bool {
        self.unreadable
            .values()
            .any(|why| why.starts_with(crate::project_tree::NOT_NOW))
    }
    /// The script the person likely means by `path`, which these sources lack: the one whose
    /// path differs only in case (Windows opens it, `require` and the project do not) or in an
    /// accent written apart from its letter, else the nearest.
    pub fn near(&self, path: &str) -> Option<&str> {
        let paths = || {
            self.files
                .keys()
                .chain(self.unreadable.keys())
                .map(String::as_str)
        };
        let composed = crate::names::composed(path);
        paths()
            .find(|known| crate::names::same_name(known, path))
            .or_else(|| paths().find(|known| crate::names::composed(known) == composed))
            .or_else(|| closest(path, paths()))
    }
    /// What these sources say of the script `path` they lack, for a message: why the file
    /// cannot be read, or that it does not exist, with the script it likely is (`o script
    /// pista/porta.luau não existe na pasta do projeto; quis dizer Pista/porta.luau?`).
    pub fn absent(&self, path: &str) -> String {
        if let Some(why) = self.why_none(path) {
            return format!("o script {path} {why}");
        }
        let hint = self.near(path).map_or_else(String::new, |near| {
            loose_marks(path, near).unwrap_or_else(|| format!("; quis dizer {near}?"))
        });
        format!("o script {path} não existe na pasta do projeto{hint}")
    }
}

/// What a message adds when the file `near` and the path `path` someone wrote for it differ
/// only in accents written apart from their letters, which look the same: how to rename it.
pub(crate) fn loose_marks(path: &str, near: &str) -> Option<String> {
    (crate::names::composed(path) == crate::names::composed(near)).then(|| {
        format!(
            "; o nome de {near} tem um acento separado da letra: renomeie o arquivo digitando a \
             letra acentuada"
        )
    })
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

/// `path` as a new script is named: a `.luau` file anywhere in the project's tree, by its path
/// from the project's folder with `/`, as `Pista/Checkpoint.luau` or `scripts/porta.luau`. `\`
/// reads as `/`, and `.` and empty parts go; `..`, absolute paths, drives, names Windows would
/// read otherwise or keeps for devices, parts starting with a dot (outside the project),
/// `init.luau` and paths past [`crate::names::MAX_PATH_BYTES`] are refused, with why.
pub fn normalize_path(path: &str) -> Result<String, String> {
    let normal = normal_form(path)?;
    match path_problem(&normal) {
        Some(reason) => Err(invalid_path(path, &reason)),
        None => Ok(normal),
    }
}

/// `path` written as scripts are named, as [`normalize_path`] does but for the rules of names a
/// path already saved may break (a dot, a device name, `init.luau`, the length): an object that
/// attaches such a file keeps it, and the file is told missing or unreadable, never the
/// project.
pub(crate) fn normal_form(path: &str) -> Result<String, String> {
    let unified = path.trim().replace('\\', "/");
    if unified.starts_with('/') {
        return Err(invalid_path(path, "começa na raiz do disco"));
    }
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(invalid_path(path, "sai da pasta do projeto")),
            part if part
                .chars()
                .any(|c| c.is_control() || r#"<>:"|?*"#.contains(c))
                || part.ends_with(['.', ' ']) =>
            {
                let reason = crate::names::check_part(part).err().unwrap_or_default();
                return Err(invalid_path(path, &reason));
            }
            part => parts.push(part),
        }
    }
    let named = parts.last().is_some_and(|file| {
        file.strip_suffix(EXTENSION)
            .and_then(|stem| stem.strip_suffix('.'))
            .is_some_and(|stem| !stem.is_empty())
    });
    if !named {
        return Err(invalid_path(
            path,
            &format!("não é um arquivo .{EXTENSION}"),
        ));
    }
    Ok(parts.join("/"))
}

fn invalid_path(path: &str, reason: &str) -> String {
    format!(
        "caminho de script inválido \"{path}\": {reason}; use o caminho de um arquivo .{EXTENSION} \
         a partir da pasta do projeto, como Pista/porta.{EXTENSION}"
    )
}

/// Why the path `path`, in the form scripts are named, names no script OXY writes or attaches:
/// what [`crate::names::check_path`] refuses, a name of the files Windows and editors leave
/// behind, which the tree skips, and `init.luau`, which Luau reads as its folder's module.
pub(crate) fn path_problem(path: &str) -> Option<String> {
    if let Err(reason) = crate::names::check_path(path) {
        return Some(reason);
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    if crate::project_tree::leftover(file) {
        return Some(format!(
            "\"{file}\" tem o nome dos arquivos que o Windows e os editores deixam para trás \
             (como ~$…), que ficam fora do projeto; dê outro nome ao arquivo"
        ));
    }
    file.eq_ignore_ascii_case("init.luau").then(|| {
        format!(
            "\"{file}\" é o nome que o Luau dá ao módulo de uma pasta, que os scripts da OXY não \
             usam; dê outro nome ao arquivo"
        )
    })
}

/// Checks the scripts an object attaches: each path written as scripts are named (a name the
/// rules of new paths refuse is told when the file is looked for), and none twice.
pub fn validate(scripts: &[ScriptRef]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for script in scripts {
        let normal = normal_form(&script.path)?;
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

/// How many objects of `project`, in scenes and models, attach the script `path`, switched on
/// or off.
pub fn attached_by(project: &Project, path: &str) -> usize {
    every_entity(project)
        .filter(|(_, e)| e.scripts.iter().any(|s| s.path == path))
        .count()
}

/// `project` as a run plays it: each object without the scripts switched off in it, so a script
/// switched off on every object never loads. A project with none switched off is itself.
pub(crate) fn switched_on(project: &Project) -> std::borrow::Cow<'_, Project> {
    if !every_entity(project).any(|(_, e)| e.scripts.iter().any(|s| !s.enabled)) {
        return std::borrow::Cow::Borrowed(project);
    }
    let mut project = project.clone();
    let models = project
        .assets
        .iter_mut()
        .filter_map(|asset| asset.model.as_mut());
    for e in project
        .scenes
        .iter_mut()
        .flat_map(|scene| scene.entities.iter_mut())
        .chain(models.flatten())
    {
        e.scripts.retain(|s| s.enabled);
    }
    std::borrow::Cow::Owned(project)
}

/// A script an object attaches that the project's tree lacks.
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
    by_file(missing, |path| {
        format!("o script {path} não existe na pasta do projeto")
    })
}

/// What keeps the scripts the objects of `project` attach from running with `sources`, one line
/// per file as [`missing_by_file`] tells them, with how many objects attach it: the files the
/// sources lack, then those they could not read or that are in a folder they could not look
/// into, with why (`Porta: o script Pista/porta.luau tem mais de 1 MB`; see
/// [`ScriptSources::why_none`]).
pub fn absent_by_file(project: &Project, sources: &ScriptSources) -> Vec<(String, usize)> {
    let lacking = missing(project, |path| {
        sources.get(path).is_some() || sources.why_none(path).is_some()
    });
    let unreadable = missing(project, |path| {
        sources.get(path).is_some() || sources.why_none(path).is_none()
    });
    let mut lines = missing_by_file(&lacking);
    lines.extend(by_file(&unreadable, |path| {
        format!(
            "o script {path} {}",
            sources.why_none(path).unwrap_or_default()
        )
    }));
    lines
}

/// `found` told once per file, the objects before what `says` of its path.
fn by_file(found: &[MissingScript], says: impl Fn(&str) -> String) -> Vec<(String, usize)> {
    let mut files: Vec<(&str, Vec<&str>)> = Vec::new();
    for found in found {
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
            (format!("{objects}: {}", says(path)), names.len())
        })
        .collect()
}

/// The scripts the scripts `attached` use: themselves, and every script a `require` with a
/// literal path reaches from them, in any of their functions, as far as `sources` has them. A
/// `require` whose path the script builds is not followed.
pub fn used_scripts(
    sources: &ScriptSources,
    attached: impl IntoIterator<Item = String>,
) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    let mut pending: Vec<String> = attached.into_iter().collect();
    while let Some(path) = pending.pop() {
        if !used.insert(path.clone()) {
            continue;
        }
        let Some(Ok((_, report))) = sources
            .get(&path)
            .map(|file| load::check(&path, &file.text))
        else {
            continue;
        };
        for literal in &report.literals {
            if let bytecode::Site::Call { function, arg: 0 } = &literal.site
                && function == "require"
                && let Ok(required) = sandbox::resolve(Some(&path), &literal.text)
            {
                pending.push(required);
            }
        }
    }
    used
}

/// The path of the module `require(name)` names in the script `from`, as the runtime reads it
/// (`./` and `../` from the script's folder, `@project/` from the project's), whether or not the
/// module exists; why not, for a path the runtime refuses.
pub(crate) fn require_path(from: &str, name: &str) -> Result<String, String> {
    sandbox::resolve(Some(from), name)
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
/// or why it cannot be read: the script does not load, the declaration is not a table of
/// defaults, or it names a surface `surfaces` (the project's) lacks. The scripts load in a VM of
/// their own, as the runtime loads them, nothing of the world reached.
pub fn declared_attributes<'a>(
    sources: &ScriptSources,
    surfaces: &[SurfaceMaterial],
    paths: impl IntoIterator<Item = &'a str>,
) -> Vec<(&'a str, Result<Declared, ScriptError>)> {
    let host = ScriptHost::new(std::sync::Arc::new(sources.clone()));
    paths
        .into_iter()
        .map(|path| {
            let declared = match &host {
                Ok(host) => host.load(path).and_then(|module| {
                    let mut declared = module
                        .attributes
                        .clone()
                        .map_err(|message| ScriptError::new(path, 0, message))?;
                    for (attribute, surface) in &module.surfaces {
                        let what = format!("param.surface(\"{surface}\")");
                        let id = surface_named(surfaces, surface, &what).map_err(|message| {
                            ScriptError::new(
                                path,
                                0,
                                format!("Modulo.attributes.{attribute}: {message}"),
                            )
                        })?;
                        if let Some((_, value)) = declared.iter_mut().find(|(n, _)| n == attribute)
                        {
                            *value = Value::Surface(Some(id));
                        }
                    }
                    Ok(declared)
                }),
                Err(error) => Err(ScriptError::new(path, 0, error.clone())),
            };
            (path, declared)
        })
        .collect()
}

/// What a script defines and declares, read by loading it as the runtime does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outline {
    /// The callbacks it defines, by name, in the order of [`Callback::ALL`].
    pub callbacks: Vec<&'static str>,
    /// The attributes `Module.attributes` declares, by name; none when it is no table of
    /// defaults.
    pub attributes: Vec<String>,
    /// The fields `Module.state` declares, each with the type of its value as the tools name
    /// types (`Number`, `Text`, `Bool`, `Vector3`, `Vector2`, `Object`, `Surface`, `Table`).
    pub state: Vec<(String, &'static str)>,
}

/// What each of the scripts `paths` of `sources` defines and declares (see [`Outline`]), or why
/// it does not load. The scripts load in a VM of their own, nothing of the world reached.
pub fn outlines<'a>(
    sources: &ScriptSources,
    paths: impl IntoIterator<Item = &'a str>,
) -> Vec<(&'a str, Result<Outline, ScriptError>)> {
    let host = ScriptHost::new(std::sync::Arc::new(sources.clone()));
    paths
        .into_iter()
        .map(|path| {
            let outline = match &host {
                Ok(host) => host.load(path).map(|module| Outline {
                    callbacks: module.callbacks.iter().map(Callback::name).collect(),
                    attributes: module
                        .attributes
                        .as_ref()
                        .map(|declared| declared.iter().map(|(name, _)| name.clone()).collect())
                        .unwrap_or_default(),
                    state: module
                        .state
                        .as_ref()
                        .map(|memory| {
                            memory
                                .fields()
                                .iter()
                                .map(|(name, value)| (name.to_string(), value.type_name()))
                                .collect()
                        })
                        .unwrap_or_default(),
                }),
                Err(error) => Err(ScriptError::new(path, 0, error.clone())),
            };
            (path, outline)
        })
        .collect()
}

/// Why callbacks of the script `path` that `callbacks` names never run on `e`, an object of a
/// scene of `kind`, when some never do: `on_area_enter` and `on_area_exit` on an object without
/// an area (the engine calls them on the object whose sensor is entered), `on_click` on one that
/// takes no clicks (neither a shape nor an interface button).
pub fn unheard_callbacks(
    e: &Entity,
    kind: SceneKind,
    path: &str,
    callbacks: &[&str],
) -> Vec<String> {
    use crate::authoring::ComponentKind;
    let mut told = Vec::new();
    let areas: Vec<&str> = callbacks
        .iter()
        .copied()
        .filter(|name| matches!(*name, "on_area_enter" | "on_area_exit"))
        .collect();
    if !areas.is_empty()
        && !ComponentKind::Sensor3d.present(e)
        && !ComponentKind::Sensor2d.present(e)
    {
        let (names, run) = match areas.as_slice() {
            [one] => ((*one).to_owned(), "nunca roda"),
            _ => ("on_area_enter e on_area_exit".to_owned(), "nunca rodam"),
        };
        let area = match kind {
            SceneKind::TwoD => "sensor2d",
            SceneKind::ThreeD => "sensor3d",
        };
        told.push(format!(
            "{} não tem área ({area}): {names} de {path} {run} nele; adicione o componente \
             {area} (add_component) ou anexe o script à área",
            e.name
        ));
    }
    let clickable = match &e.ui {
        Some(ui) => ui.kind == crate::document::UiKind::Button,
        None => e.has_geometry(),
    };
    if callbacks.contains(&"on_click") && !clickable {
        told.push(format!(
            "{} não recebe cliques (não tem forma nem é um botão da interface): on_click de \
             {path} nunca roda nele",
            e.name
        ));
    }
    told
}

/// The script line a text names, as `Pista/porta.luau:12` in "Script não recarregado:
/// Pista/porta.luau:12: …": a `.luau` path after the start of the text or after ": " and
/// "· ", then a colon and the line. None when the text names no line of a script.
pub fn location_in(text: &str) -> Option<crate::runtime::ScriptLocation> {
    let mark = format!(".{EXTENSION}:");
    let mut from = 0;
    while let Some(found) = text[from..].find(&mark) {
        let end = from + found + mark.len() - 1;
        let digits: String = text[end + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let start = [": ", "· "]
            .iter()
            .filter_map(|sep| text[..end].rfind(sep).map(|at| at + sep.len()))
            .max()
            .unwrap_or(0);
        if let (Ok(line), Ok(path)) = (digits.parse::<u32>(), normal_form(text[start..end].trim()))
            && line > 0
        {
            return Some(crate::runtime::ScriptLocation { path, line });
        }
        from = end + 1;
    }
    None
}

/// The script a text starts with, as script errors write it: `Pista/porta.luau:12: …` gives the
/// file and the line, `Pista/porta.luau: …` the file and line 0. None when the text starts with
/// anything else (what the engine tells of the scripts as a whole).
pub(crate) fn place_at_start(text: &str) -> Option<(String, u32)> {
    let (head, rest) = text.split_once(':')?;
    if !head.ends_with(&format!(".{EXTENSION}")) {
        return None;
    }
    let path = normal_form(head).ok()?;
    let line = if rest.starts_with(' ') {
        0
    } else {
        rest.split_once(": ")?
            .0
            .parse::<u32>()
            .ok()
            .filter(|line| *line > 0)?
    };
    Some((path, line))
}

/// The surface of `surfaces` that `name` names (in `what`, as `param.surface("Gelo")`): by its
/// name, else ignoring case and the spaces at its ends, as the project lists them.
pub(crate) fn surface_named(
    surfaces: &[SurfaceMaterial],
    name: &str,
    what: &str,
) -> Result<Id, String> {
    let wanted = name.trim().to_lowercase();
    let found = surfaces.iter().find(|s| s.name == name).or_else(|| {
        surfaces
            .iter()
            .find(|s| s.name.trim().to_lowercase() == wanted)
    });
    if let Some(surface) = found {
        return Ok(surface.id.clone());
    }
    if surfaces.is_empty() {
        return Err(format!(
            "{what} nomeia uma superfície, e o projeto não tem superfícies"
        ));
    }
    let names: Vec<&str> = surfaces.iter().map(|s| s.name.as_str()).collect();
    let hint = closest(name, names.iter().copied())
        .map_or_else(|| ".".to_owned(), |near| format!("; quis dizer {near}?"));
    Err(format!(
        "{what} nomeia uma superfície que o projeto não tem{hint} Superfícies: {}",
        names.join(", ")
    ))
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
    fn paths_name_luau_files_anywhere_in_the_project() {
        for (given, normal) in [
            ("scripts/porta.luau", "scripts/porta.luau"),
            ("Pista/Checkpoint.luau", "Pista/Checkpoint.luau"),
            ("porta.luau", "porta.luau"),
            ("outros/porta.luau", "outros/porta.luau"),
            ("Scripts/porta.luau", "Scripts/porta.luau"),
            (
                r"scripts\fases\Porta com chave.luau",
                "scripts/fases/Porta com chave.luau",
            ),
            (
                "./scripts//dados/./cartas.luau",
                "scripts/dados/cartas.luau",
            ),
            ("  Pista/Fase ação/ação.luau ", "Pista/Fase ação/ação.luau"),
            ("Pista/COM10.luau", "Pista/COM10.luau"),
            ("Pista/initial.luau", "Pista/initial.luau"),
            ("Velho.bak/porta.luau", "Velho.bak/porta.luau"),
        ] {
            assert_eq!(normalize_path(given).as_deref(), Ok(normal), "{given}");
        }
        for (refused, reason) in [
            ("", "não é um arquivo .luau"),
            ("scripts", "não é um arquivo .luau"),
            ("scripts/", "não é um arquivo .luau"),
            ("scripts/porta.lua", "não é um arquivo .luau"),
            ("scripts/porta.LUAU", "não é um arquivo .luau"),
            ("scripts/porta.luau.txt", "não é um arquivo .luau"),
            ("scripts/.luau", "não é um arquivo .luau"),
            ("scripts/../porta.luau", "sai da pasta do projeto"),
            ("../scripts/porta.luau", "sai da pasta do projeto"),
            ("/scripts/porta.luau", "começa na raiz do disco"),
            (r"\\servidor\scripts\porta.luau", "começa na raiz do disco"),
            (
                "C:/projeto/scripts/porta.luau",
                ":, que o Windows não aceita",
            ),
            ("C:scripts/porta.luau", ":, que o Windows não aceita"),
            ("scripts/por?ta.luau", "?, que o Windows não aceita"),
            ("scripts/fases./porta.luau", "termina em ponto"),
            ("scripts/fase /porta.luau", "termina em espaço"),
            ("scripts/linha\n.luau", "caractere de controle"),
            (".oxy/porta.luau", "começa com ponto"),
            ("Pista/.rascunho.luau", "começa com ponto"),
            ("Pista/con.luau", "reserva para dispositivos"),
            ("COM1.luau", "reserva para dispositivos"),
            ("Pista/init.luau", "módulo de uma pasta"),
            ("Pista/Init.luau", "módulo de uma pasta"),
            ("Pista/~$porta.luau", "deixam para trás"),
        ] {
            let error = normalize_path(refused).unwrap_err();
            assert!(
                error.starts_with(&format!("caminho de script inválido \"{refused}\": "))
                    && error.contains(reason)
                    && error.ends_with("como Pista/porta.luau"),
                "{refused:?}: {error}"
            );
        }
        // 180 bytes at most: Luau cuts a chunk's name near 255 and the messages would lose it.
        let longest = format!("{}/{}.luau", "a".repeat(80), "b".repeat(94));
        assert_eq!(longest.len(), crate::names::MAX_PATH_BYTES);
        assert_eq!(normalize_path(&longest).as_deref(), Ok(longest.as_str()));
        let longer = format!("{}/{}.luau", "a".repeat(80), "b".repeat(95));
        assert!(
            normalize_path(&longer)
                .unwrap_err()
                .contains("longo demais")
        );
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
        // Switched off, the reference says so; any other field is refused.
        e.scripts[1].enabled = false;
        let off = serde_json::to_value(&e).unwrap();
        assert_eq!(
            off["scripts"][1],
            serde_json::json!({"path": "scripts/comum/som.luau", "enabled": false})
        );
        assert_eq!(serde_json::from_value::<Entity>(off).unwrap(), e);
        let mut unknown = json;
        unknown["scripts"][0]["ligado"] = true.into();
        assert!(serde_json::from_value::<Entity>(unknown).is_err());
    }

    #[test]
    fn projects_refuse_scripts_outside_the_project_unnormalized_or_twice() {
        let attach = |paths: &[&str]| {
            let mut project = Project::new("Scripts");
            let mut e = Entity::new("Porta", Some(Primitive::Cube));
            e.scripts = paths.iter().copied().map(ScriptRef::new).collect();
            project.scenes[0].entities.push(e);
            validate_project(&project)
        };
        attach(&[]).unwrap();
        attach(&["scripts/porta.luau", "scripts/comum/som.luau"]).unwrap();
        attach(&[
            "porta.luau",
            "Pista/Checkpoints/porta.luau",
            "outros/porta.luau",
        ])
        .unwrap();
        // A name the rules of new paths refuse, saved before them, keeps the project opening:
        // the file is told missing or unreadable instead.
        attach(&[
            "scripts/con.luau",
            "Pista/init.luau",
            ".rascunhos/porta.luau",
        ])
        .unwrap();
        assert_eq!(
            attach(&["../porta.luau"]).unwrap_err(),
            "Porta: caminho de script inválido \"../porta.luau\": sai da pasta do projeto; use o \
             caminho de um arquivo .luau a partir da pasta do projeto, como Pista/porta.luau"
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
        piece.scripts = vec![ScriptRef::new("Pista/peça.luau")];
        let id = piece.id.clone();
        project.scenes[0].entities.push(piece);
        project.save_model(&scene, &id, "Bloco").unwrap();
        project.scenes[0].entities.clear();
        validate_project(&project).unwrap();
        project.assets[0].model.as_mut().unwrap()[0].scripts = vec![ScriptRef::new("../peça.luau")];
        assert!(
            validate_project(&project)
                .unwrap_err()
                .contains("caminho de script inválido \"../peça.luau\"")
        );
    }

    #[test]
    fn sources_say_why_a_script_is_none_and_which_one_was_meant() {
        let mut sources = ScriptSources::from_texts([
            ("Pista/Checkpoint.luau", "return {}"),
            ("Pista/dados.luau", "return {}"),
        ]);
        sources
            .unreadable
            .insert("Pista/grande.luau".into(), "tem mais de 1 MB".into());
        assert_eq!(
            sources.absent("pista/checkpoint.luau"),
            "o script pista/checkpoint.luau não existe na pasta do projeto; quis dizer \
             Pista/Checkpoint.luau?"
        );
        assert_eq!(
            sources.absent("Pista/dado.luau"),
            "o script Pista/dado.luau não existe na pasta do projeto; quis dizer Pista/dados.luau?"
        );
        assert_eq!(
            sources.absent("Menu/tela.luau"),
            "o script Menu/tela.luau não existe na pasta do projeto"
        );
        assert_eq!(
            sources.absent("Pista/grande.luau"),
            "o script Pista/grande.luau tem mais de 1 MB"
        );
        // A file whose name has an accent apart from its letter looks like the one written.
        let mut marks = ScriptSources::from_texts([("Pista/ac\u{327}a\u{303}o.luau", "return {}")]);
        assert_eq!(
            marks.absent("Pista/ação.luau"),
            "o script Pista/ação.luau não existe na pasta do projeto; o nome de \
             Pista/ac\u{327}a\u{303}o.luau tem um acento separado da letra: renomeie o arquivo \
             digitando a letra acentuada"
        );
        // In a folder the tree skips or could not list: why, the deepest folder first, in the
        // case written too.
        marks.folders.insert(
            "Outro".into(),
            "fica fora do projeto: Outro é outro projeto ou um jogo exportado".into(),
        );
        marks.folders.insert(
            "Outro/Fase".into(),
            "não pôde ser lido agora: a pasta Outro/Fase não pôde ser lida agora (negado)".into(),
        );
        assert_eq!(
            marks.absent("outro/x.luau"),
            "o script outro/x.luau fica fora do projeto: Outro é outro projeto ou um jogo \
             exportado"
        );
        assert_eq!(
            marks.why_none("Outro/Fase/x.luau"),
            Some("não pôde ser lido agora: a pasta Outro/Fase não pôde ser lida agora (negado)")
        );
        assert_eq!(marks.why_none("Outros/x.luau"), None);
        assert!(!marks.held());
        marks.unreadable.insert(
            "Pista/preso.luau".into(),
            "não pôde ser lido agora (preso)".into(),
        );
        assert!(marks.held());
        // Lines per file for a run: the missing ones, then those that cannot be read.
        let mut project = Project::new("Ausentes");
        for (name, path) in [
            ("Porta", "Pista/porta.luau"),
            ("Bloco", "Pista/grande.luau"),
            ("Bloco 2", "Pista/grande.luau"),
            ("Marco", "Pista/Checkpoint.luau"),
            ("Elo", "Pista/Fase/elo.luau"),
        ] {
            let mut e = Entity::new(name, None);
            e.scripts = vec![ScriptRef::new(path)];
            project.scenes[0].entities.push(e);
        }
        sources.folders.insert(
            "Pista/Fase".into(),
            "não pôde ser lido agora: a pasta Pista/Fase não pôde ser lida agora (negado)".into(),
        );
        assert_eq!(
            absent_by_file(&project, &sources),
            [
                (
                    "Porta: o script Pista/porta.luau não existe na pasta do projeto".to_owned(),
                    1
                ),
                (
                    "Bloco e Bloco 2: o script Pista/grande.luau tem mais de 1 MB".to_owned(),
                    2
                ),
                (
                    "Elo: o script Pista/Fase/elo.luau não pôde ser lido agora: a pasta \
                     Pista/Fase não pôde ser lida agora (negado)"
                        .to_owned(),
                    1
                ),
            ]
        );
    }

    #[test]
    fn the_scripts_used_are_those_attached_and_what_their_literal_requires_reach() {
        let sources = ScriptSources::from_texts([
            (
                "Pista/Checkpoints/porta.luau",
                "local Dados = require('../dados')\nlocal M = {}\nfunction M.on_start(self) \
                 local Sons = require('@project/Comum/sons') end\nreturn M\n",
            ),
            ("Pista/dados.luau", "return require('./tabela')"),
            ("Pista/tabela.luau", "return {}"),
            ("Comum/sons.luau", "return {}"),
            // Built names are not followed; nothing requires the others.
            (
                "Comum/montado.luau",
                "local M = {}\nfunction M.carregar(nome) return require('./' .. nome) end\n\
                 return M",
            ),
            ("Rascunhos/quebrado.luau", "return {"),
            ("Comum/x.luau", "return {}"),
        ]);
        let used = used_scripts(&sources, ["Pista/Checkpoints/porta.luau".to_owned()]);
        assert_eq!(
            used.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "Comum/sons.luau",
                "Pista/Checkpoints/porta.luau",
                "Pista/dados.luau",
                "Pista/tabela.luau"
            ]
        );
        // A script attached that is missing is used all the same.
        let used = used_scripts(&sources, ["Comum/montado.luau".into(), "nada.luau".into()]);
        assert_eq!(
            used.iter().map(String::as_str).collect::<Vec<_>>(),
            ["Comum/montado.luau", "nada.luau"]
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

    #[test]
    fn a_text_starts_with_a_script_place_only_as_script_errors_write_it() {
        assert_eq!(
            place_at_start("Pista/porta.luau:12: sem on_message: 1d76"),
            Some(("Pista/porta.luau".into(), 12))
        );
        assert_eq!(
            place_at_start("Meu jogo/a b.luau: o módulo não devolve uma tabela"),
            Some(("Meu jogo/a b.luau".into(), 0))
        );
        for engine in [
            "Limite de trabalho dos scripts no passo atingido; demais tarefas retomam no próximo \
             passo.",
            "sem on_message: 1d76",
            "os objetos A e B têm a mesma chave nos scripts; renomeie um deles",
            "A VM dos scripts não iniciou: memória",
            "Pista/porta.luau:x: texto",
        ] {
            assert_eq!(place_at_start(engine), None, "{engine}");
        }
    }
}
