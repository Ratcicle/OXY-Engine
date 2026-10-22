//! The project's script files as the tools reach them (oxy_scripts, on files and, live, through
//! the editor's channel): read with line numbers, checked as loading checks them, written
//! all-or-nothing with the copy an agent keeps of a file it changes, and the files VS Code with
//! the luau-lsp extension reads (design §6.2): the API's types in [`TYPES_PATH`], `.luaurc` with
//! the project's alias for `require("@project/…")`, and the luau-lsp keys of
//! `.vscode/settings.json`; the person's files only get what they lack. Scripts are the `.luau`
//! files of the whole tree, by their path from the project's folder in the case the disk has.
//! The disk is the truth, live too: a write reaches a test running in the editor through its
//! watcher, and the editor's Ctrl+Z does not undo it.
use crate::document::Project;
use crate::export::engine_version;
use crate::persistence::{self, Expected, FileWrite, PersistenceError, WriteMode};
use crate::project_tree::Tree;
use crate::script::{self, PROJECT_ALIAS, ScriptFile};
use serde_json::{Map, Value as Json, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What oxy_scripts does.
pub const ACTIONS: [&str; 4] = ["read", "write", "check", "vscode"];

/// Where the API's types go, from the project's folder.
pub const TYPES_PATH: &str = ".oxy/tipos/oxy.d.luau";
const LUAURC: &str = ".luaurc";
const LUAURC_TEXT: &str = "{\n    \"languageMode\": \"nonstrict\",\n    \"aliases\": {\n        \
                           \"project\": \"./\"\n    }\n}\n";
/// The `.luaurc` OXY wrote before scripts had the project's alias: OXY's, so written anew.
const LUAURC_BEFORE: &str = "{\n    \"languageMode\": \"nonstrict\"\n}\n";
/// What the project's alias names in `.luaurc`: the folder of the `.luaurc` itself (Luau's own
/// requirer would read `"."` from the folder its process runs in).
const ALIAS_VALUE: &str = "./";
const SETTINGS: &str = ".vscode/settings.json";
/// Where the luau-lsp keys go when `.vscode/settings.json` cannot take them.
const SUGGESTION: &str = ".oxy/vscode-settings.json";

/// The globals the sandbox takes away or replaces, which luau-lsp must not offer (S2 §5).
pub const DISABLED_GLOBALS: [&str; 10] = [
    "os",
    "debug",
    "buffer",
    "math.random",
    "math.randomseed",
    "loadstring",
    "getfenv",
    "setfenv",
    "gcinfo",
    "newproxy",
];

/// Scripts a message lists before it counts the rest.
const LISTED: usize = 20;

/// An oxy_scripts request, its paths written as [`script::normalize_path`] writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Read {
        path: String,
    },
    Write {
        path: String,
        text: String,
    },
    /// One script, a text as that script without saving it, or every script of the tree.
    Check {
        path: Option<String>,
        text: Option<String>,
    },
    Vscode,
}

impl Request {
    /// The request in the tool's arguments, checked before any editor or file is reached.
    pub fn parse(args: &Json) -> Result<Self, String> {
        let action = args["action"].as_str().unwrap_or_default();
        let text = |key: &str| match &args[key] {
            Json::Null => Ok(None),
            Json::String(text) => Ok(Some(text.clone())),
            _ => Err(format!("{key} precisa ser um texto")),
        };
        let path = || -> Result<Option<String>, String> {
            text("path")?
                .map(|path| script::normalize_path(&path))
                .transpose()
        };
        let needed = || {
            path()?.ok_or_else(|| {
                format!(
                    "action {action} precisa de path, o caminho do script a partir da pasta do \
                     projeto, como Pista/porta.luau"
                )
            })
        };
        match action {
            "read" => Ok(Self::Read { path: needed()? }),
            "write" => {
                let path = needed()?;
                let text = text("text")?
                    .ok_or("action write precisa de text, o texto inteiro do arquivo")?;
                Ok(Self::Write { path, text })
            }
            "check" => {
                let (path, text) = (path()?, text("text")?);
                if text.is_some() && path.is_none() {
                    return Err("text no check precisa de path, o script que ele seria".into());
                }
                Ok(Self::Check { path, text })
            }
            "vscode" => Ok(Self::Vscode),
            other => Err(format!(
                "action inválido: `{other}`; use {}",
                ACTIONS.join(", ")
            )),
        }
    }
}

/// What a request did: the tool's answer and the lines the editor's Console tells of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub json: Json,
    pub console: Vec<String>,
}

/// Answers `request` on the project saved in the folder `root`. `project` is that project, as
/// the editor has it live, which tells the checks which scripts objects attach (none when it
/// could not be read). `kept` holds what the session already kept of the scripts it wrote (see
/// [`Request::Write`]): one set per session. A path is taken in the case the disk has, so
/// `pista/porta.luau` reads, checks and writes `Pista/porta.luau`, and the answer says so.
pub fn answer(
    root: &Path,
    project: Option<&Project>,
    request: &Request,
    kept: &mut HashSet<PathBuf>,
) -> Result<Answer, String> {
    let quiet = |json| Answer {
        json,
        console: Vec::new(),
    };
    let attached = project.map(script::attached_paths).unwrap_or_default();
    let on_disk = |path: &str| crate::names::disk_case(root, path).unwrap_or_else(|_| path.into());
    match request {
        Request::Read { path } => read(root, &on_disk(path)).map(quiet),
        Request::Write { path, text } => {
            write(root, project, &on_disk(path), text, &attached, kept)
        }
        Request::Check { path, text } => {
            let path = path.as_deref().map(on_disk);
            check(root, project, path.as_deref(), text.as_deref(), &attached).map(quiet)
        }
        Request::Vscode => {
            let setup = setup_vscode(root)?;
            Ok(Answer {
                json: setup.json(),
                console: setup.console(),
            })
        }
    }
}

/// The text of a script, each line after its number and a tab.
fn read(root: &Path, path: &str) -> Result<Json, String> {
    let text = persistence::read_script(root, path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| missing(root, path))?;
    let numbered: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{}\t{line}", i + 1))
        .collect();
    Ok(json!({"path": path, "lines": numbered.len(), "text": numbered.join("\n")}))
}

/// Why `path` cannot be read: the tree lacks it or cannot read it; with the script it likely
/// is and the scripts it has.
fn missing(root: &Path, path: &str) -> String {
    let sources = persistence::load_scripts(root).unwrap_or_default();
    if let Some(why) = sources.why_none(path) {
        return format!("O script {path} {why}");
    }
    let files: Vec<&str> = sources.files.keys().map(String::as_str).collect();
    let listed = match files.len() {
        0 => "o projeto não tem scripts".to_owned(),
        n if n > LISTED => format!(
            "scripts: {} e mais {}",
            files[..LISTED].join(", "),
            n - LISTED
        ),
        _ => format!("scripts: {}", files.join(", ")),
    };
    let hint = sources.near(path).map_or_else(String::new, |near| {
        script::loose_marks(path, near).unwrap_or_else(|| format!("; quis dizer {near}?"))
    });
    format!("O script {path} não existe na pasta do projeto{hint}; {listed}")
}

/// Checks a script, a text as that script or every script of the tree as loading checks them
/// (see [`script::check_loading`]; `attached`, the scripts objects attach), with the scripts of
/// the tree for `require`, and the names they give against `project` (see [`script::lint`]).
/// Every script also tells the `.luau` files that cannot be scripts (errors where the game uses
/// them, see [`script::used_scripts`], else warnings) and what the tree warns of (see
/// [`tree_warnings`]).
fn check(
    root: &Path,
    project: Option<&Project>,
    path: Option<&str>,
    text: Option<&str>,
    attached: &BTreeSet<String>,
) -> Result<Json, String> {
    let mut told = (Vec::new(), Vec::new());
    let (sources, paths) = match (path, text) {
        (Some(path), Some(text)) => {
            // The file it stands for is not read: it may be what the text comes to fix.
            let mut sources =
                persistence::load_scripts_except(root, path).map_err(|e| e.to_string())?;
            sources
                .files
                .insert(path.to_owned(), ScriptFile::new(text.to_owned()));
            (sources, vec![path.to_owned()])
        }
        (Some(path), None) => {
            let sources = persistence::load_scripts(root).map_err(|e| e.to_string())?;
            if sources.get(path).is_none() {
                return Err(missing(root, path));
            }
            (sources, vec![path.to_owned()])
        }
        _ => {
            let (sources, tree) =
                persistence::load_scripts_in_tree(root).map_err(|e| e.to_string())?;
            // A file that cannot be a script is an error where an object attaches it or a
            // script they run requires it, else a warning.
            let used = script::used_scripts(&sources, attached.iter().cloned());
            for (path, reason) in &sources.unreadable {
                let line = format!("O script {path} {reason}");
                if used.contains(path) {
                    told.0.push(line);
                } else {
                    told.1.push(line);
                }
            }
            told.1.extend(tree_warnings(root, &tree));
            let paths = sources.files.keys().cloned().collect();
            (sources, paths)
        }
    };
    let reports = script::check_loading(&sources, paths.iter().map(String::as_str), |path| {
        attached.contains(path)
    });
    let mut errors: Vec<String> = reports
        .iter()
        .filter_map(|report| report.error.as_ref().map(ToString::to_string))
        .collect();
    errors.extend(told.0);
    let mut warnings = warnings(&sources, &reports, project);
    warnings.extend(told.1);
    let mut answer = json!({
        "ok": errors.is_empty(),
        "checked": paths,
        "checker": {"name": "oxy", "version": engine_version()},
        "types_unavailable": true,
    });
    if !errors.is_empty() {
        answer["errors"] = json!(errors);
    }
    if !warnings.is_empty() {
        answer["warnings"] = json!(warnings);
    }
    Ok(answer)
}

/// Writes the script `path` with `text` when it loads (the other scripts of the tree serve its
/// `require`; the file it replaces is not read, so a text fixes a file that is not UTF-8 or is
/// too large), refusing it otherwise with nothing written; its folders are created. The first
/// time the session changes a file that existed before it, the file as it was goes to its copy
/// under `.oxy/agente` (see [`persistence::script_backup_path`]: out of the tree, so never a
/// script) in the same transaction, and again if that copy was deleted; a file the session
/// created keeps none. The first write of a project without this engine's types or the
/// project's alias in `.luaurc` sets VS Code up too.
fn write(
    root: &Path,
    project: Option<&Project>,
    path: &str,
    text: &str,
    attached: &BTreeSet<String>,
    kept: &mut HashSet<PathBuf>,
) -> Result<Answer, String> {
    if text.len() as u64 > script::MAX_BYTES {
        return Err(PersistenceError::ScriptTooLarge(path.to_owned()).to_string());
    }
    let mut sources = persistence::load_scripts_except(root, path).map_err(|e| e.to_string())?;
    sources
        .files
        .insert(path.to_owned(), ScriptFile::new(text.to_owned()));
    let reports = script::check_loading(&sources, [path], |path| attached.contains(path));
    if let Some(error) = &reports[0].error {
        return Err(format!("{error}; nada foi gravado"));
    }
    let base =
        std::fs::canonicalize(root).map_err(|e| format!("Pasta do projeto indisponível: {e}"))?;
    let backup_path = persistence::script_backup_path(path);
    let (file_key, backup_key) = (base.join(path), base.join(&backup_path));
    // A file the session created keeps no copy; one it kept keeps it while the copy is there.
    let created = kept.contains(&file_key);
    let copied = kept.contains(&backup_key) && backup_key.is_file();
    let backup = !created && !copied;
    let previous =
        persistence::write_script(root, path, text, backup).map_err(|e| e.to_string())?;
    match &previous {
        Some(_) if backup => kept.insert(backup_key.clone()),
        None => kept.insert(file_key),
        Some(_) => false,
    };
    let lines = text.lines().count();
    let mut answer = json!({
        "path": path,
        "saved": shown(root, path),
        "created": previous.is_none(),
        "lines": lines,
    });
    let mut console = vec![match &previous {
        None => format!(
            "Script {path} criado ({lines} {})",
            if lines == 1 { "linha" } else { "linhas" }
        ),
        Some(old) => {
            let (added, removed) = changed_lines(old, text);
            answer["added"] = json!(added);
            answer["removed"] = json!(removed);
            if added + removed == 0 {
                format!("Script {path} gravado sem mudanças")
            } else {
                format!("Script {path} alterado (+{added} -{removed} linhas)")
            }
        }
    }];
    if kept.contains(&backup_key) {
        answer["previous_version"] = json!(shown(root, &backup_path));
    }
    let mut warnings = warnings(&sources, &reports, project);
    match setup_vscode_when_due(root) {
        Ok(Some(setup)) => {
            answer["vscode"] = setup.json();
            console.extend(setup.console());
        }
        Ok(None) => {}
        Err(error) => warnings.push(error),
    }
    if !warnings.is_empty() {
        answer["warnings"] = json!(warnings);
    }
    Ok(Answer {
        json: answer,
        console,
    })
}

/// What the check warns of in the scripts of `reports`, script by script: what loading found
/// (see [`script::check_loading`]), then the lint of the names they give, against `project` when
/// there is one (see [`script::lint`]).
fn warnings(
    sources: &script::ScriptSources,
    reports: &[script::ScriptReport],
    project: Option<&Project>,
) -> Vec<String> {
    let linted = script::lint(
        sources,
        reports
            .iter()
            .filter(|report| report.error.is_none())
            .map(|report| report.path.as_str()),
        project,
    );
    reports
        .iter()
        .flat_map(|report| {
            let named = linted.iter().filter(|w| w.path == report.path);
            report.warnings.iter().chain(named).map(ToString::to_string)
        })
        .collect()
}

/// The file `path` of the project's folder as answers show it, with one kind of slash.
fn shown(root: &Path, path: &str) -> String {
    root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
        .display()
        .to_string()
}

/// The lines `new` has that `old` lacks and those `old` has that `new` lacks, each line counted
/// as often as it repeats; a line that only moved counts in neither.
fn changed_lines(old: &str, new: &str) -> (usize, usize) {
    let mut count: HashMap<&str, isize> = HashMap::new();
    for line in old.lines() {
        *count.entry(line).or_default() -= 1;
    }
    for line in new.lines() {
        *count.entry(line).or_default() += 1;
    }
    count.values().fold((0, 0), |(added, removed), n| {
        (
            added + n.max(&0).unsigned_abs(),
            removed + n.min(&0).unsigned_abs(),
        )
    })
}

/// What setting VS Code up wrote and what it left for the person.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VsCode {
    /// The files written, from the project's folder.
    pub written: Vec<String>,
    /// What it left as it was and the person may want to change, in Portuguese.
    pub notes: Vec<String>,
}

impl VsCode {
    /// The tools' answer about it: `{written, notes?}`.
    pub fn json(&self) -> Json {
        let mut answer = json!({"written": self.written});
        if !self.notes.is_empty() {
            answer["notes"] = json!(self.notes);
        }
        answer
    }

    /// The Console's lines about it.
    pub fn console(&self) -> Vec<String> {
        let mut lines = vec![if self.written.is_empty() {
            "Arquivos do VS Code em dia".to_owned()
        } else {
            let written: Vec<&str> = self.written.iter().map(String::as_str).collect();
            format!("Arquivos do VS Code gravados: {}", and_list(&written))
        }];
        lines.extend(self.notes.iter().cloned());
        lines
    }
}

/// `a`, `a e b`, `a, b e c`.
fn and_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first @ .., last] => format!("{} e {last}", first.join(", ")),
    }
}

/// The definitions file as written: a line with the engine's version, then [`script::types`].
pub fn types_text() -> String {
    format!(
        "-- Gerado pela OXY Engine {}: os tipos da API dos scripts para o luau-lsp. Não edite: a\n\
         -- OXY grava este arquivo de novo quando a versão da engine muda.\n{}",
        engine_version(),
        script::types()
    )
}

/// Whether the project saved in `root` lacks this engine's types: none yet, another version's
/// or a file someone changed.
pub fn types_outdated(root: &Path) -> bool {
    std::fs::read_to_string(root.join(TYPES_PATH)).map_or(true, |text| text != types_text())
}

/// [`setup_vscode`] when the project saved in `root` lacks this engine's types, or its `.luaurc`
/// lacks the project's alias and OXY can give it (see [`Luaurc`]), as the first write of a
/// script and the first attach_script do, so whoever writes the scripts in VS Code has the
/// API's types and `require("@project/…")` without asking for them; none when they are there.
/// Why it could not, as a warning.
pub fn setup_vscode_when_due(root: &Path) -> Result<Option<VsCode>, String> {
    if !types_outdated(root) && !luaurc(root).due() {
        return Ok(None);
    }
    setup_vscode(root)
        .map(Some)
        .map_err(|error| format!("os arquivos do VS Code não foram gravados: {error}"))
}

/// Writes in the project saved in `root` what VS Code with the luau-lsp extension needs to know
/// the scripts' API (design §6.2): the types (rewritten when they are not this engine's),
/// `.luaurc` with the project's alias (see [`Luaurc`]), and the luau-lsp keys
/// `.vscode/settings.json` lacks, added at its end with the rest of the file as it was. A key it
/// has with another value stays, with a note; a settings file that is not plain JSON (with
/// comments, for one) stays as it was, and the keys go to `.oxy/vscode-settings.json` for the
/// person to copy. The notes also name the configurations of subfolders that give the alias
/// another folder.
pub fn setup_vscode(root: &Path) -> Result<VsCode, String> {
    let mut done = VsCode::default();
    let write = |path: &str, text: &str| -> Result<(), String> {
        let file = persistence::resolve_asset_path(root, path).map_err(|e| e.to_string())?;
        persistence::safe_write(&file, text.as_bytes()).map_err(|e| e.to_string())
    };
    if types_outdated(root) {
        write(TYPES_PATH, &types_text())?;
        done.written.push(TYPES_PATH.into());
    }
    match luaurc(root) {
        Luaurc::Write { text, before } => {
            // The person's file is replaced only while it holds what was read.
            let target =
                persistence::resolve_asset_path(root, LUAURC).map_err(|e| e.to_string())?;
            let file = match before {
                Some(before) => FileWrite {
                    target,
                    bytes: text.into_bytes(),
                    mode: WriteMode::Replace {
                        expected: Some(Expected::Bytes(before.into_bytes().into())),
                    },
                },
                None => FileWrite::create(target, text.into_bytes()),
            };
            persistence::transaction_write(vec![file]).map_err(|e| e.to_string())?;
            done.written.push(LUAURC.into());
        }
        Luaurc::Done => {}
        Luaurc::Kept(note) => done.notes.push(note),
    }
    if let Ok(tree) = crate::project_tree::walk(root) {
        done.notes.extend(nested_aliases(root, &tree));
    }
    let settings = match std::fs::read_to_string(root.join(SETTINGS)) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Não foi possível ler {SETTINGS}: {error}")),
    };
    match merge_settings(settings.as_deref()) {
        Some((text, notes)) => {
            if let Some(text) = text {
                write(SETTINGS, &text)?;
                done.written.push(SETTINGS.into());
            }
            done.notes.extend(notes);
        }
        None => {
            write(SUGGESTION, &settings_text(&settings_keys()))?;
            done.written.push(SUGGESTION.into());
            done.notes.push(format!(
                "o {SETTINGS} não é JSON puro (tem comentários?) e ficou como estava; junte a \
                 ele as chaves de {SUGGESTION}"
            ));
        }
    }
    Ok(done)
}

/// What the project's `.luaurc` needs for luau-lsp to read `require("@project/…")` as the game
/// does (alias `"project": "./"`, checked with luau-lsp 1.70.1).
enum Luaurc {
    /// The text to write: OXY's when there is none or it is OXY's of before, else the person's
    /// with the alias added and the rest as it was; with the text it replaces.
    Write {
        text: String,
        before: Option<String>,
    },
    /// It has the alias.
    Done,
    /// It stays as it was, with why for the person.
    Kept(String),
}

impl Luaurc {
    fn due(&self) -> bool {
        matches!(self, Self::Write { .. })
    }
}

/// What the `.luaurc` of the project saved in `root` needs.
fn luaurc(root: &Path) -> Luaurc {
    let text = match std::fs::read_to_string(root.join(LUAURC)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Luaurc::Write {
                text: LUAURC_TEXT.into(),
                before: None,
            };
        }
        Err(error) => return Luaurc::Kept(format!("Não foi possível ler o {LUAURC}: {error}")),
    };
    if text == LUAURC_BEFORE {
        return Luaurc::Write {
            text: LUAURC_TEXT.into(),
            before: Some(text),
        };
    }
    let without = format!(
        "sem \"aliases\": {{ \"{PROJECT_ALIAS}\": \"{ALIAS_VALUE}\" }} no {LUAURC}, o VS Code não \
         acha os require(\"@{PROJECT_ALIAS}/…\"); o jogo acha"
    );
    let Ok(map) = serde_json::from_str::<Map<String, Json>>(&text) else {
        return Luaurc::Kept(format!(
            "o {LUAURC} não é JSON puro (tem comentários?) e ficou como estava; {without}"
        ));
    };
    let merged = match map.get("aliases") {
        None => insert_entry(
            &text,
            None,
            &format!("\"aliases\": {{\n        \"{PROJECT_ALIAS}\": \"{ALIAS_VALUE}\"\n    }}"),
        ),
        Some(Json::Object(aliases)) => {
            if let Some((name, value)) = aliases
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(PROJECT_ALIAS))
            {
                if value == ALIAS_VALUE {
                    return Luaurc::Done;
                }
                return Luaurc::Kept(format!(
                    "o {LUAURC} dá ao apelido {name} o valor {value}; o jogo lê \
                     require(\"@{PROJECT_ALIAS}/…\") a partir da pasta do projeto, e o VS Code, a \
                     partir de {value}"
                ));
            }
            insert_entry(
                &text,
                Some("aliases"),
                &format!("\"{PROJECT_ALIAS}\": \"{ALIAS_VALUE}\""),
            )
        }
        Some(_) => None,
    };
    // Written only when it reads back as JSON with the alias.
    match merged.filter(|merged| {
        serde_json::from_str::<Json>(merged)
            .is_ok_and(|read| read["aliases"][PROJECT_ALIAS] == ALIAS_VALUE)
    }) {
        Some(merged) => Luaurc::Write {
            text: merged,
            before: Some(text),
        },
        None => Luaurc::Kept(format!("o {LUAURC} ficou como estava; {without}")),
    }
}

/// `text`, a JSON object, with `entry` added at the end of its top level (`key` none) or of the
/// object its top-level `key` holds, the rest as it was; none when they cannot be found.
fn insert_entry(text: &str, key: Option<&str>, entry: &str) -> Option<String> {
    let (open, close) = object_span(text, key)?;
    let head = text[..close].trim_end();
    let inside = &text[open + 1..head.len()];
    // After the last entry: on a line of its own, indented as that one, when the entries have
    // lines of their own.
    let joined = if inside.trim().is_empty() {
        format!(" {entry}")
    } else if inside.contains('\n') {
        let line = &head[head.rfind('\n').map_or(0, |at| at + 1)..];
        let indent = &line[..line.len() - line.trim_start().len()];
        format!(",\n{indent}{entry}")
    } else {
        format!(", {entry}")
    };
    Some(format!("{head}{joined}{}", &text[head.len()..]))
}

/// Where the object of `key` at the top level of `text` (the top-level object itself, `key`
/// none) opens and closes, as byte offsets of its braces.
fn object_span(text: &str, key: Option<&str>) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    let mut wanted: Option<usize> = None;
    let mut last_key: Option<String> = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                if depth == 1 {
                    last_key = Some(text.get(start..i)?.to_owned());
                }
            }
            b'{' => {
                depth += 1;
                stack.push(i);
                if depth == 2 && key.is_some() && last_key.as_deref() == key {
                    wanted = Some(i);
                }
            }
            b'}' => {
                let open = stack.pop()?;
                if (key.is_none() && depth == 1) || (Some(open) == wanted) {
                    return Some((open, i));
                }
                depth -= 1;
            }
            b',' if depth == 1 => last_key = None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// What the configurations of subfolders the tree found (see [`Tree::configs`]) do to the
/// project's alias: luau-lsp reads the nearest one that has it, the game never reads them.
fn nested_aliases(root: &Path, tree: &Tree) -> Vec<String> {
    tree.configs
        .iter()
        .filter(|path| {
            let Ok(text) = std::fs::read_to_string(root.join(path.as_str())) else {
                return false;
            };
            match serde_json::from_str::<Map<String, Json>>(&text) {
                Ok(map) => map
                    .get("aliases")
                    .and_then(Json::as_object)
                    .is_some_and(|aliases| {
                        aliases
                            .keys()
                            .any(|name| name.eq_ignore_ascii_case(PROJECT_ALIAS))
                    }),
                // A .config.luau, or a .luaurc with comments.
                Err(_) => sets_alias(&text),
            }
        })
        .map(|path| {
            format!(
                "{path} define o apelido {PROJECT_ALIAS}: nos scripts dessa pasta o VS Code lê \
                 require(\"@{PROJECT_ALIAS}/…\") a partir dela, e o jogo, a partir da pasta do \
                 projeto; tire o apelido desse arquivo"
            )
        })
        .collect()
}

/// Whether `text`, a `.config.luau` or a `.luaurc` with comments, gives the project's alias a
/// folder: `aliases` and a key `project` followed by `=` or `:`, in any case, line comments
/// (`--`, `//`) aside.
fn sets_alias(text: &str) -> bool {
    let code = text
        .lines()
        .map(|line| {
            let end = [line.find("--"), line.find("//")]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(line.len());
            &line[..end]
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    code.contains("aliases")
        && code.match_indices(PROJECT_ALIAS).any(|(at, _)| {
            let word = !code[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let after = code[at + PROJECT_ALIAS.len()..]
                .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ']'));
            word && after.starts_with(['=', ':'])
        })
}

/// What the tree at `root` warns of about scripts: what its walk left out that someone may count
/// on (see [`Tree::warnings`]) and the configurations of subfolders that change the project's
/// alias for VS Code.
pub fn tree_warnings(root: &Path, tree: &Tree) -> Vec<String> {
    let mut warnings = tree.warnings.clone();
    warnings.extend(nested_aliases(root, tree));
    warnings
}

/// The luau-lsp keys OXY needs (design §6.2, checked in S2): the definitions as a map (since
/// luau-lsp 1.56), the globals the sandbox takes away, and no Roblox flags or sourcemap. The new
/// solver is left as it is: it doubles the time of a check.
fn settings_keys() -> Vec<(&'static str, Json)> {
    vec![
        ("luau-lsp.platform.type", json!("standard")),
        ("luau-lsp.sourcemap.enabled", json!(false)),
        ("luau-lsp.fflags.sync", json!(false)),
        (
            "luau-lsp.types.definitionFiles",
            json!({"@oxy": TYPES_PATH}),
        ),
        ("luau-lsp.types.disabledGlobals", json!(DISABLED_GLOBALS)),
    ]
}

/// A value on one line, with a space after commas and colons.
fn inline(value: &Json) -> String {
    match value {
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Json::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(key, value)| format!("{}: {}", Json::from(key.as_str()), inline(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

fn entries(keys: &[(&str, Json)]) -> String {
    keys.iter()
        .map(|(key, value)| format!("    {}: {}", Json::from(*key), inline(value)))
        .collect::<Vec<_>>()
        .join(",\n")
}

/// A settings file holding only `keys`.
fn settings_text(keys: &[(&str, Json)]) -> String {
    format!("{{\n{}\n}}\n", entries(keys))
}

/// `settings`, the text of a settings file (none when there is no file), with the luau-lsp keys
/// it lacks added before its closing brace, and the notes about the keys it has with other
/// values; no text when it lacks none. None when it is not a plain JSON object.
fn merge_settings(settings: Option<&str>) -> Option<(Option<String>, Vec<String>)> {
    let keys = settings_keys();
    let Some(text) = settings else {
        return Some((Some(settings_text(&keys)), Vec::new()));
    };
    let map: Map<String, Json> = serde_json::from_str(text).ok()?;
    let mut lacking = Vec::new();
    let mut notes = Vec::new();
    for (key, value) in keys {
        let Some(has) = map.get(key) else {
            lacking.push((key, value));
            continue;
        };
        if *has == value {
            continue;
        }
        let note = match key {
            "luau-lsp.types.definitionFiles" if has["@oxy"] == value["@oxy"] => continue,
            "luau-lsp.types.definitionFiles" => format!(
                "{key} no {SETTINGS} não tem \"@oxy\": \"{TYPES_PATH}\"; sem isso o luau-lsp não \
                 conhece a API dos scripts"
            ),
            "luau-lsp.types.disabledGlobals" => {
                let listed: Vec<&str> = has
                    .as_array()
                    .map(|names| names.iter().filter_map(Json::as_str).collect())
                    .unwrap_or_default();
                let absent: Vec<&str> = DISABLED_GLOBALS
                    .into_iter()
                    .filter(|name| !listed.contains(name))
                    .collect();
                if absent.is_empty() {
                    continue;
                }
                format!(
                    "{key} no {SETTINGS} não tem {}, que os scripts não têm",
                    and_list(&absent)
                )
            }
            "luau-lsp.fflags.sync" => format!(
                "{key} vale {has} no {SETTINGS}; a OXY usa false, porque com true a extensão baixa \
                 as flags do Roblox e aceita o que a engine recusa"
            ),
            _ => format!("{key} vale {has} no {SETTINGS}; a OXY usa {value}"),
        };
        notes.push(note);
    }
    if lacking.is_empty() {
        return Some((None, notes));
    }
    let end = text.rfind('}')?;
    let head = text[..end].trim_end();
    let comma = if map.is_empty() { "" } else { "," };
    let merged = format!(
        "{head}{comma}\n{}\n}}{}",
        entries(&lacking),
        &text[end + 1..]
    );
    Some((Some(merged), notes))
}

/// The luau-lsp settings a project's VS Code reads, by key, as `setup_vscode` writes them: for
/// a check outside VS Code (`luau-lsp analyze --settings`), which takes flat keys only.
pub fn settings() -> BTreeMap<String, Json> {
    settings_keys()
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_name_their_action_and_take_paths_as_scripts_are_named() {
        let parse = |args: Json| Request::parse(&args);
        assert_eq!(
            parse(json!({"action": "read", "path": r"scripts\porta.luau"})),
            Ok(Request::Read {
                path: "scripts/porta.luau".into()
            })
        );
        assert_eq!(
            parse(json!({"action": "check"})),
            Ok(Request::Check {
                path: None,
                text: None
            })
        );
        assert_eq!(parse(json!({"action": "vscode"})), Ok(Request::Vscode));
        for (args, refused) in [
            (
                json!({"action": "ler"}),
                "action inválido: `ler`; use read, write",
            ),
            (json!({"action": "read"}), "action read precisa de path"),
            (
                json!({"action": "write", "path": "scripts/a.luau"}),
                "precisa de text",
            ),
            (
                json!({"action": "write", "path": "../a.luau", "text": ""}),
                "caminho de script",
            ),
            (
                json!({"action": "check", "text": "return {}"}),
                "precisa de path",
            ),
            (
                json!({"action": "read", "path": 3}),
                "path precisa ser um texto",
            ),
        ] {
            let error = parse(args.clone()).unwrap_err();
            assert!(error.contains(refused), "{args}: {error}");
        }
    }

    #[test]
    fn changed_lines_count_what_came_and_what_went() {
        assert_eq!(changed_lines("a\nb\nc\n", "a\nb\nc\n"), (0, 0));
        assert_eq!(changed_lines("a\nb\nc\n", "a\nx\nc\ny\n"), (2, 1));
        assert_eq!(changed_lines("a\nb\n", "b\na\n"), (0, 0));
        assert_eq!(changed_lines("", "a\na\n"), (2, 0));
    }

    #[test]
    fn settings_get_only_the_keys_they_lack_and_keep_the_rest_as_it_was() {
        // No file: the keys alone, as VS Code writes settings.
        let (text, notes) = merge_settings(None).unwrap();
        let text = text.unwrap();
        assert!(notes.is_empty());
        let written: Map<String, Json> = serde_json::from_str(&text).unwrap();
        assert_eq!(written["luau-lsp.fflags.sync"], false);
        assert_eq!(
            written["luau-lsp.types.definitionFiles"],
            json!({"@oxy": ".oxy/tipos/oxy.d.luau"})
        );
        assert_eq!(
            written["luau-lsp.types.disabledGlobals"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
        assert!(written.get("luau-lsp.types.enableNewSolver").is_none());
        // All there already: nothing to write.
        assert_eq!(merge_settings(Some(&text)), Some((None, Vec::new())));
        // The person's file: their keys and their order stay, ours come at the end.
        let theirs = "{\n  \"editor.fontSize\": 15,\n  \"luau-lsp.fflags.sync\": true,\n  \
                      \"luau-lsp.types.definitionFiles\": {\"@roblox\": \"x.d.luau\"}\n}\n";
        let (merged, notes) = merge_settings(Some(theirs)).unwrap();
        let merged = merged.unwrap();
        assert!(
            merged.starts_with(
                "{\n  \"editor.fontSize\": 15,\n  \"luau-lsp.fflags.sync\": true,\n  \
                 \"luau-lsp.types.definitionFiles\": {\"@roblox\": \"x.d.luau\"},\n    \
                 \"luau-lsp.platform.type\": \"standard\""
            ),
            "{merged}"
        );
        assert!(
            merged.ends_with("\"setfenv\", \"gcinfo\", \"newproxy\"]\n}\n"),
            "{merged}"
        );
        let read: Map<String, Json> = serde_json::from_str(&merged).unwrap();
        assert_eq!(read["luau-lsp.fflags.sync"], true, "never changed");
        assert_eq!(read.len(), 6);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(
            notes[0].starts_with("luau-lsp.fflags.sync vale true"),
            "{notes:?}"
        );
        assert!(notes[1].contains("não tem \"@oxy\""), "{notes:?}");
        // An empty object takes them without a comma.
        let (merged, _) = merge_settings(Some("{}")).unwrap();
        let read: Map<String, Json> = serde_json::from_str(&merged.unwrap()).unwrap();
        assert_eq!(read.len(), 5);
        // A list of globals that already has ours, among others, and the definitions with
        // another entry beside @oxy are fine as they are.
        let mut globals: Vec<&str> = DISABLED_GLOBALS.to_vec();
        globals.push("print");
        let mut fine = settings();
        fine.insert("luau-lsp.types.disabledGlobals".into(), json!(globals));
        fine.insert(
            "luau-lsp.types.definitionFiles".into(),
            json!({"@oxy": TYPES_PATH, "@outro": "y.d.luau"}),
        );
        let fine = serde_json::to_string_pretty(&fine).unwrap();
        assert_eq!(merge_settings(Some(&fine)), Some((None, Vec::new())));
        // Comments are not plain JSON: the file stays as it was.
        assert_eq!(merge_settings(Some("{\n  // fonte\n  \"a\": 1\n}\n")), None);
        assert_eq!(merge_settings(Some("[1, 2]")), None);
    }

    #[test]
    fn the_types_say_which_engine_wrote_them() {
        let text = types_text();
        assert!(
            text.starts_with(&format!("-- Gerado pela OXY Engine {}", engine_version())),
            "{}",
            &text[..120]
        );
        assert!(text.ends_with(script::types()));
    }

    #[test]
    fn a_configuration_sets_the_alias_only_with_a_key_of_its_name() {
        for text in [
            "return { aliases = { project = './Menu' } }",
            "return {\n  aliases = {\n    [\"Project\"] = \"./Menu\",\n  },\n}",
            "{\n  // meu\n  \"aliases\": { \"project\": \"./Menu\" }\n}",
        ] {
            assert!(sets_alias(text), "{text}");
        }
        for text in [
            "-- project settings\nreturn { aliases = { comum = './Comum' } }",
            "{\n  // the project's own\n  \"aliases\": { \"comum\": \"./Comum\" }\n}",
            "return { aliases = { myproject = './x', project_b = './y' } }",
            "return { languageMode = 'strict' } -- project = './x'",
            "return { project = './x' }",
        ] {
            assert!(!sets_alias(text), "{text}");
        }
    }
}
