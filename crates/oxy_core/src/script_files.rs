//! The project's script files as the tools reach them (oxy_scripts, on files and, live, through
//! the editor's channel): read with line numbers, checked as loading checks them, written
//! all-or-nothing with the copy an agent keeps of a file it changes, and the files VS Code with
//! the luau-lsp extension reads (design §6.2): the API's types in [`TYPES_PATH`], `.luaurc` and
//! the luau-lsp keys of `.vscode/settings.json`, which only gets the keys it lacks. The disk is
//! the truth, live too: a write reaches a test running in the editor through its watcher, and
//! the editor's Ctrl+Z does not undo it.
use crate::document::Project;
use crate::export::engine_version;
use crate::persistence::{self, PersistenceError};
use crate::script::{self, ScriptFile};
use serde_json::{Map, Value as Json, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What oxy_scripts does.
pub const ACTIONS: [&str; 4] = ["read", "write", "check", "vscode"];

/// Where the API's types go, from the project's folder.
pub const TYPES_PATH: &str = ".oxy/tipos/oxy.d.luau";
const LUAURC: &str = ".luaurc";
const LUAURC_TEXT: &str = "{\n    \"languageMode\": \"nonstrict\"\n}\n";
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
    /// One script, a text as that script without saving it, or every script of the folder.
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
                    "action {action} precisa de path, como {}/porta.luau",
                    script::FOLDER
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
/// [`Request::Write`]): one set per session.
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
    match request {
        Request::Read { path } => read(root, path).map(quiet),
        Request::Write { path, text } => write(root, path, text, &attached, kept),
        Request::Check { path, text } => {
            check(root, path.as_deref(), text.as_deref(), &attached).map(quiet)
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

/// Why `path` cannot be read: the folder lacks it; with the scripts it has.
fn missing(root: &Path, path: &str) -> String {
    let files: Vec<String> = persistence::script_stamps(root)
        .map(|stamps| stamps.into_keys().collect())
        .unwrap_or_default();
    let listed = match files.len() {
        0 => format!("a pasta {} não tem scripts", script::FOLDER),
        n if n > LISTED => format!(
            "scripts: {} e mais {}",
            files[..LISTED].join(", "),
            n - LISTED
        ),
        _ => format!("scripts: {}", files.join(", ")),
    };
    format!("O script {path} não existe na pasta do projeto; {listed}")
}

/// Checks a script, a text as that script or every script of the folder as loading checks them
/// (see [`script::check_loading`]; `attached`, the scripts objects attach), with the scripts of
/// the folder for `require`.
fn check(
    root: &Path,
    path: Option<&str>,
    text: Option<&str>,
    attached: &BTreeSet<String>,
) -> Result<Json, String> {
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
        _ => {
            let sources = persistence::load_scripts(root).map_err(|e| e.to_string())?;
            let paths: Vec<String> = match path {
                Some(path) if sources.get(path).is_none() => return Err(missing(root, path)),
                Some(path) => vec![path.to_owned()],
                None => sources.files.keys().cloned().collect(),
            };
            (sources, paths)
        }
    };
    let reports = script::check_loading(&sources, paths.iter().map(String::as_str), |path| {
        attached.contains(path)
    });
    let errors: Vec<String> = reports
        .iter()
        .filter_map(|report| report.error.as_ref().map(ToString::to_string))
        .collect();
    let warnings: Vec<String> = reports
        .iter()
        .flat_map(|report| &report.warnings)
        .map(ToString::to_string)
        .collect();
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

/// Writes the script `path` with `text` when it loads (the other scripts of the folder serve its
/// `require`; the file it replaces is not read, so a text fixes a file that is not UTF-8 or is
/// too large), refusing it otherwise with nothing written. The first time the session changes a
/// file that existed before it, the file as it was goes to its copy under `.oxy/agente` (see
/// [`persistence::script_backup_path`]: never in the `scripts` folder, which holds only scripts)
/// in the same transaction, and again if that copy was deleted; a file the session created
/// keeps none. The first write of a project without this engine's types sets VS Code up too.
fn write(
    root: &Path,
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
    let report = script::check_loading(&sources, [path], |path| attached.contains(path)).remove(0);
    if let Some(error) = &report.error {
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
    let mut warnings: Vec<String> = report.warnings.iter().map(ToString::to_string).collect();
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

/// The definitions file as written: a line with the engine's version, then [`script::TYPES`].
pub fn types_text() -> String {
    format!(
        "-- Gerado pela OXY Engine {}: os tipos da API dos scripts para o luau-lsp. Não edite: a\n\
         -- OXY grava este arquivo de novo quando a versão da engine muda.\n{}",
        engine_version(),
        script::TYPES
    )
}

/// Whether the project saved in `root` lacks this engine's types: none yet, another version's
/// or a file someone changed.
pub fn types_outdated(root: &Path) -> bool {
    std::fs::read_to_string(root.join(TYPES_PATH)).map_or(true, |text| text != types_text())
}

/// [`setup_vscode`] when the project saved in `root` lacks this engine's types, as the first
/// write of a script and the first attach_script do, so whoever writes the scripts in VS Code
/// has the API's types without asking for them; none when they are there. Why it could not, as
/// a warning.
pub fn setup_vscode_when_due(root: &Path) -> Result<Option<VsCode>, String> {
    if !types_outdated(root) {
        return Ok(None);
    }
    setup_vscode(root)
        .map(Some)
        .map_err(|error| format!("os arquivos do VS Code não foram gravados: {error}"))
}

/// Writes in the project saved in `root` what VS Code with the luau-lsp extension needs to know
/// the scripts' API (design §6.2): the types (rewritten when they are not this engine's),
/// `.luaurc` when there is none, and the luau-lsp keys `.vscode/settings.json` lacks, added at
/// its end with the rest of the file as it was. A key it has with another value stays, with a
/// note, as does a `.luaurc` other than the one OXY writes; a settings file that is not plain
/// JSON (with comments, for one) stays as it was, and the keys go to `.oxy/vscode-settings.json`
/// for the person to copy.
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
    match std::fs::read_to_string(root.join(LUAURC)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write(LUAURC, LUAURC_TEXT)?;
            done.written.push(LUAURC.into());
        }
        Ok(text) if text == LUAURC_TEXT => {}
        _ => done.notes.push(format!(
            "o {LUAURC} que já existe ficou como estava (a OXY não precisa de nada nele)"
        )),
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
                json!({"action": "write", "path": "a.luau", "text": ""}),
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
        assert!(text.ends_with(script::TYPES));
    }
}
