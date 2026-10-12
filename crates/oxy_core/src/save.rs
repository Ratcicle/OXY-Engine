//! Progress saved between runs of a game: the values of the game variables marked Salva entre
//! partidas, in a versioned JSON file. Only hosts read and write it, in the folder they choose
//! (see [`path`]); a run starts from a [`SaveSlot`] and queues [`SaveRequest`]s, so the runtime
//! never touches the disk and a run with the same save and inputs asks the same.
use crate::{
    document::{Id, Project, Value, new_id},
    persistence,
    variables::{self, GameVariable, MAX_KEYS, MAX_TEXT},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

/// The format this build reads and writes; a file of a newer one is never written over.
pub const VERSION: u64 = 1;
/// The file in a game's folder.
pub const FILE: &str = "progresso.json";
/// Largest file read; a bigger one is set aside as unreadable.
pub const MAX_BYTES: u64 = 1 << 20;
/// Most lines one load explains; the rest are counted in a last one.
const MAX_DIAGNOSTICS: usize = 20;

/// The saved progress, as a host read it and as a run keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveSlot {
    /// Values by (variable ID, key); the empty key is the variable's own value.
    pub values: BTreeMap<(Id, String), Value>,
    /// Entries of variables the project does not declare, as the file had them: written back
    /// unchanged, so a build without a variable keeps what another build saved for it.
    pub extras: BTreeMap<Id, Json>,
    /// There is saved progress: the file was read, or the run saved.
    pub exists: bool,
    /// The host may write: false when the file could not be read or comes from a newer build.
    pub writable: bool,
}
impl Default for SaveSlot {
    /// No progress yet, as on a fresh install.
    fn default() -> Self {
        Self {
            values: BTreeMap::new(),
            extras: BTreeMap::new(),
            exists: false,
            writable: true,
        }
    }
}
impl SaveSlot {
    /// A slot nothing may be written from: the host could not tell what the file holds.
    fn read_only() -> Self {
        Self {
            writable: false,
            ..Self::default()
        }
    }
    /// The slot once the host carries out `request`.
    pub fn apply(&mut self, request: SaveRequest) {
        match request {
            SaveRequest::Write(values) => {
                self.values = values;
                self.exists = true;
            }
            SaveRequest::Clear => {
                self.values.clear();
                self.extras.clear();
                self.exists = false;
            }
        }
    }
}

/// What Salvar progresso and Apagar progresso ask the host, oldest first.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveRequest {
    /// Every value of the persistent variables now, keys included.
    Write(BTreeMap<(Id, String), Value>),
    /// No progress any more.
    Clear,
}

/// The persistent variable `id`, when `value` fits it under `key`: declared, saved between runs,
/// of its type, finite, with texts and the key of at most [`MAX_TEXT`] bytes. The error is a
/// clause naming the variable.
pub fn check<'a>(
    project: &'a Project,
    id: &str,
    key: &str,
    value: &Value,
) -> Result<&'a GameVariable, String> {
    let variable =
        variables::get(project, id).ok_or_else(|| "a variável não existe no projeto".to_owned())?;
    if !variable.persistent {
        return Err(format!(
            "a variável {} não é salva entre partidas",
            variable.name
        ));
    }
    if key.len() > MAX_TEXT {
        return Err(format!(
            "a chave de {} passa de {MAX_TEXT} bytes",
            variable.name
        ));
    }
    let label = variables::label(&variable.name, key);
    if std::mem::discriminant(value) != std::mem::discriminant(&variable.value) {
        return Err(format!(
            "{label} é {}, mas a variável {} guarda {}",
            variables::kind(value),
            variable.name,
            variables::kind(&variable.value)
        ));
    }
    variables::check_value(value).map_err(|error| format!("{label}: {error}"))?;
    Ok(variable)
}

/// Where the progress of the project `id` lives under `base`, the folder its host keeps games'
/// progress in: `<base>\<id>\progresso.json`. Players use `%APPDATA%\OXY Engine\Jogos`; tests
/// pass a temporary folder. An ID that is not a plain folder name is refused, so a hand-edited
/// project cannot write elsewhere.
pub fn path(base: &Path, id: &str) -> Result<PathBuf, String> {
    let plain = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !plain {
        return Err(
            "O identificador do projeto não serve de nome de pasta; o progresso não será salvo"
                .into(),
        );
    }
    Ok(base.join(id).join(FILE))
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    oxy_save: u64,
    game: String,
    /// For people reading the file; the game is known by its ID.
    #[serde(default)]
    game_name: String,
    #[serde(default)]
    variables: BTreeMap<String, Json>,
}
#[derive(Serialize)]
struct Entry<'a> {
    name: &'a str,
    values: BTreeMap<&'a str, &'a Value>,
}

/// The file for `slot`: pretty JSON with no timestamps, so equal progress gives equal bytes.
/// Variables the project lacks write nothing, but for the extras, kept as they came.
pub fn encode(project: &Project, slot: &SaveSlot) -> Vec<u8> {
    let mut entries = slot.extras.clone();
    for variable in &project.variables {
        let values: BTreeMap<&str, &Value> = slot
            .values
            .range((variable.id.clone(), String::new())..)
            .take_while(|((id, _), _)| *id == variable.id)
            .map(|((_, key), value)| (key.as_str(), value))
            .collect();
        if values.is_empty() {
            continue;
        }
        let entry = Entry {
            name: &variable.name,
            values,
        };
        // Values and texts always make JSON; a non-finite number, refused before, would read null.
        entries.insert(
            variable.id.clone(),
            serde_json::to_value(entry).unwrap_or_default(),
        );
    }
    let file = SaveFile {
        oxy_save: VERSION,
        game: project.id.clone(),
        game_name: project.name.clone(),
        variables: entries,
    };
    let mut bytes = serde_json::to_vec_pretty(&file).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

/// Lines explaining what a load left out, at most [`MAX_DIAGNOSTICS`] and a count of the rest.
#[derive(Default)]
struct Diagnostics {
    lines: Vec<String>,
    more: usize,
}
impl Diagnostics {
    fn push(&mut self, line: String) {
        if self.lines.len() < MAX_DIAGNOSTICS {
            self.lines.push(line);
        } else {
            self.more += 1;
        }
    }
    fn finish(mut self) -> Vec<String> {
        if self.more > 0 {
            self.lines
                .push(format!("E mais {} valores salvos ignorados.", self.more));
        }
        self.lines
    }
}

/// Reads the progress of `project` at `path`, never failing: what cannot be used is left out
/// and explained in the lines returned, in Portuguese.
/// - No file: no progress.
/// - A file over [`MAX_BYTES`], not JSON or of another shape, or of another game, is moved
///   aside (`progresso.json.ilegivel-<id>.json`, `.outro-jogo-<id>.json`) and the game starts
///   without progress; when it cannot be moved, nothing is written this session.
/// - A newer format, or a file that cannot be read: no progress and nothing written this
///   session, so it is never lost.
/// - Per value: one the project's variables do not take (no longer saved, another type,
///   non-finite, too long, past [`MAX_KEYS`] keys) is skipped, and the variable starts at its
///   initial value. Entries of variables the project lacks are kept in `extras`.
pub fn load(project: &Project, path: &Path) -> (SaveSlot, Vec<String>) {
    let bytes = match read(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return (SaveSlot::default(), Vec::new()),
        Err(Problem::TooLarge) => return set_aside(path, "ilegivel", "passa de 1 MB"),
        Err(Problem::Io(error)) => {
            return (
                SaveSlot::read_only(),
                vec![format!(
                    "Não foi possível ler o progresso salvo ({error}); o jogo começa sem ele e \
                     não grava progresso nesta sessão."
                )],
            );
        }
    };
    let root: Json = match serde_json::from_slice(&bytes) {
        Ok(root) => root,
        Err(_) => return set_aside(path, "ilegivel", "não é um JSON válido"),
    };
    let version = root.get("oxy_save").and_then(Json::as_u64);
    match version {
        Some(version) if version > VERSION => {
            return (
                SaveSlot::read_only(),
                vec![format!(
                    "O progresso salvo é de uma versão mais nova do jogo (formato {version}); o \
                     jogo começa sem ele e não grava progresso nesta sessão, para não perdê-lo."
                )],
            );
        }
        Some(1..) => {}
        _ => return set_aside(path, "ilegivel", "não tem a versão do formato"),
    }
    let file: SaveFile = match serde_json::from_value(root) {
        Ok(file) => file,
        Err(_) => return set_aside(path, "ilegivel", "tem um formato desconhecido"),
    };
    if file.game != project.id {
        return set_aside(path, "outro-jogo", "é de outro jogo");
    }
    let mut slot = SaveSlot {
        exists: true,
        ..SaveSlot::default()
    };
    let mut diagnostics = Diagnostics::default();
    for (id, entry) in file.variables {
        let Some(variable) = variables::get(project, &id) else {
            slot.extras.insert(id, entry);
            continue;
        };
        if !variable.persistent {
            diagnostics.push(format!(
                "Progresso salvo de {} ignorado: a variável não é mais salva entre partidas.",
                variable.name
            ));
            continue;
        }
        let Some(values) = entry.get("values").and_then(Json::as_object) else {
            diagnostics.push(format!(
                "Progresso salvo de {} ignorado: formato desconhecido; vale o valor inicial.",
                variable.name
            ));
            continue;
        };
        let mut keys = 0;
        for (key, value) in values {
            let checked = serde_json::from_value::<Value>(value.clone())
                .map_err(|_| {
                    format!(
                        "o valor de {} não é legível",
                        variables::label(&variable.name, key)
                    )
                })
                .and_then(|value| check(project, &id, key, &value).map(|_| value));
            let value = match checked {
                Ok(value) => value,
                Err(reason) => {
                    diagnostics.push(format!(
                        "Progresso salvo ignorado: {reason}; vale o valor inicial."
                    ));
                    continue;
                }
            };
            if !key.is_empty() {
                if keys == MAX_KEYS {
                    diagnostics.push(format!(
                        "Progresso salvo de {} ignorado além de {MAX_KEYS} chaves.",
                        variable.name
                    ));
                    break;
                }
                keys += 1;
            }
            slot.values.insert((id.clone(), key.clone()), value);
        }
    }
    (slot, diagnostics.finish())
}

enum Problem {
    TooLarge,
    Io(io::Error),
}
/// The file's bytes; None when it does not exist.
fn read(path: &Path) -> Result<Option<Vec<u8>>, Problem> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Problem::Io(error)),
    };
    let size = file.metadata().map_err(Problem::Io)?.len();
    if size > MAX_BYTES {
        return Err(Problem::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(Problem::Io)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Problem::TooLarge);
    }
    Ok(Some(bytes))
}
/// Moves a file the game cannot use out of the way, keeping its bytes, and starts without
/// progress; when it cannot be moved, nothing is written over it this session.
fn set_aside(path: &Path, label: &str, reason: &str) -> (SaveSlot, Vec<String>) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let aside = path.with_file_name(format!("{name}.{label}-{}.json", new_id()));
    match fs::rename(path, &aside) {
        Ok(()) => (
            SaveSlot::default(),
            vec![format!(
                "O progresso salvo {reason}; o arquivo foi guardado como {} e o jogo começa sem \
                 progresso.",
                aside.file_name().unwrap_or_default().to_string_lossy()
            )],
        ),
        Err(error) => (
            SaveSlot::read_only(),
            vec![format!(
                "O progresso salvo {reason} e não pôde ser guardado à parte ({error}); o jogo \
                 começa sem ele e não grava progresso nesta sessão."
            )],
        ),
    }
}

/// Brings the file at `path` to `slot`, in one disk operation: written when the slot holds
/// progress (through [`persistence::safe_write`]), removed when it does not. Refused for a slot
/// that may not be written.
pub fn store(path: &Path, project: &Project, slot: &SaveSlot) -> Result<(), String> {
    if !slot.writable {
        return Err(
            "Progresso não gravado: o arquivo salvo não pôde ser usado nesta sessão, e gravar por \
             cima o perderia."
                .into(),
        );
    }
    if slot.exists {
        persistence::safe_write(path, &encode(project, slot))
            .map_err(|error| format!("Não foi possível gravar o progresso: {error}"))
    } else {
        clear(path)
    }
}
/// Removes the progress at `path`; none there is fine.
pub fn clear(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Não foi possível apagar o progresso salvo: {error}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxy-save-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    /// A game with a best time per level (persistent), a name (persistent text) and a run timer
    /// that is never saved.
    fn game() -> (Project, Id, Id, Id) {
        let mut project = Project::new("Parkour");
        let best =
            variables::create(&mut project, "Melhor tempo", Value::Number(0.), true).unwrap();
        let name = variables::create(&mut project, "Nome", Value::Text("".into()), true).unwrap();
        let run =
            variables::create(&mut project, "Tempo da fase", Value::Number(0.), false).unwrap();
        (project, best, name, run)
    }
    fn entry(id: &str, key: &str) -> (Id, String) {
        (id.into(), key.into())
    }
    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn progress_round_trips_with_stable_bytes_and_keeps_unknown_variables() {
        let (project, best, name, _) = game();
        let dir = temp();
        let path = path(&dir, &project.id).unwrap();
        assert_eq!(path, dir.join(&project.id).join("progresso.json"));
        let mut slot = SaveSlot {
            exists: true,
            ..SaveSlot::default()
        };
        slot.values.insert(entry(&best, ""), Value::Number(0.));
        slot.values
            .insert(entry(&best, "Fase 1"), Value::Number(41.2));
        slot.values
            .insert(entry(&best, "Fase 2"), Value::Number(63.05));
        slot.values
            .insert(entry(&name, ""), Value::Text("Ana".into()));
        // What a newer build saved for a variable this one lacks.
        let newer = serde_json::json!({"name": "Moedas", "values": {"": {"Number": 7.0}}});
        slot.extras.insert("moedas-v2".into(), newer.clone());
        store(&path, &project, &slot).unwrap();
        let bytes = fs::read(&path).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            text.starts_with("{\n  \"oxy_save\": 1,\n  \"game\": "),
            "{text}"
        );
        assert!(text.contains("\"game_name\": \"Parkour\""), "{text}");
        assert!(text.contains("\"Fase 1\": {\n"), "{text}");
        assert!(!text.contains("Tempo da fase"));
        let (loaded, diagnostics) = load(&project, &path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(loaded, slot);
        // Written again, the bytes are the same: no timestamps, sorted keys.
        assert_eq!(encode(&project, &loaded), bytes);
        let file: Json = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(file["variables"]["moedas-v2"], newer);
        assert_eq!(
            file["variables"][&best]["values"]["Fase 1"],
            serde_json::json!({"Number": 41.2})
        );
        assert_eq!(file["variables"][&best]["name"], "Melhor tempo");

        // Clearing removes the file, and a missing file is no progress, writable.
        let mut cleared = loaded;
        cleared.apply(SaveRequest::Clear);
        assert!(!cleared.exists && cleared.values.is_empty() && cleared.extras.is_empty());
        store(&path, &project, &cleared).unwrap();
        assert!(!path.exists());
        assert_eq!(load(&project, &path), (SaveSlot::default(), Vec::new()));
        clear(&path).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unreadable_files_are_set_aside_and_the_game_starts_without_progress() {
        let (project, ..) = game();
        let dir = temp();
        let path = path(&dir, &project.id).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let big = vec![b' '; MAX_BYTES as usize + 1];
        for (bytes, reason) in [
            (
                b"{\"oxy_save\": 1, \"game\": ".to_vec(),
                "não é um JSON válido",
            ),
            (b"[1, 2]".to_vec(), "não tem a versão do formato"),
            (
                b"{\"oxy_save\": 1, \"game\": 5, \"variables\": {}}".to_vec(),
                "formato desconhecido",
            ),
            (big, "passa de 1 MB"),
        ] {
            fs::write(&path, &bytes).unwrap();
            let (slot, diagnostics) = load(&project, &path);
            assert_eq!(slot, SaveSlot::default(), "{reason}");
            assert_eq!(diagnostics.len(), 1);
            assert!(diagnostics[0].contains(reason), "{diagnostics:?}");
            assert!(!path.exists());
            let names = files(path.parent().unwrap());
            assert_eq!(names.len(), 1, "{names:?}");
            assert!(
                names[0].starts_with("progresso.json.ilegivel-") && names[0].ends_with(".json")
            );
            let aside = path.with_file_name(&names[0]);
            assert!(diagnostics[0].contains(&names[0]), "{diagnostics:?}");
            assert_eq!(fs::read(&aside).unwrap(), bytes);
            fs::remove_file(aside).unwrap();
        }

        // Another game's progress is set aside too, never written over.
        let other = Project::new("Outro");
        let mut slot = SaveSlot {
            exists: true,
            ..SaveSlot::default()
        };
        slot.extras
            .insert("x".into(), serde_json::json!({"name": "X", "values": {}}));
        fs::write(&path, encode(&other, &slot)).unwrap();
        let (loaded, diagnostics) = load(&project, &path);
        assert_eq!(loaded, SaveSlot::default());
        assert!(
            diagnostics[0].contains("é de outro jogo"),
            "{diagnostics:?}"
        );
        let names = files(path.parent().unwrap());
        assert!(
            names[0].starts_with("progresso.json.outro-jogo-"),
            "{names:?}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_or_unreadable_progress_is_never_written_over() {
        let (project, best, ..) = game();
        let dir = temp();
        let path = path(&dir, &project.id).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let newer = format!(
            "{{\"oxy_save\": 2, \"game\": \"{}\", \"novidade\": true}}",
            project.id
        );
        fs::write(&path, &newer).unwrap();
        let (mut slot, diagnostics) = load(&project, &path);
        assert!(!slot.writable && !slot.exists && slot.values.is_empty());
        assert!(
            diagnostics[0].contains("versão mais nova do jogo (formato 2)"),
            "{diagnostics:?}"
        );
        slot.apply(SaveRequest::Write(
            [(entry(&best, ""), Value::Number(3.))].into(),
        ));
        let error = store(&path, &project, &slot).unwrap_err();
        assert!(error.contains("gravar por cima o perderia"), "{error}");
        slot.apply(SaveRequest::Clear);
        assert!(store(&path, &project, &slot).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), newer);

        // A folder where the file should be cannot be read: nothing is written either.
        let blocked = dir.join("pasta").join(FILE);
        fs::create_dir_all(&blocked).unwrap();
        let (slot, diagnostics) = load(&project, &blocked);
        assert!(!slot.writable, "{diagnostics:?}");
        assert!(diagnostics[0].contains("não grava progresso nesta sessão"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn values_the_variables_no_longer_take_fall_back_to_the_initial_value() {
        let (project, best, name, run) = game();
        let dir = temp();
        let path = path(&dir, &project.id).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let long = "a".repeat(MAX_TEXT + 1);
        let file = serde_json::json!({
            "oxy_save": 1,
            "game": project.id,
            "game_name": "Parkour antigo",
            "variables": {
                best.clone(): {"name": "Recorde", "values": {
                    "": {"Number": 12.5},
                    "Fase 1": {"Text": "rápido"},
                    "Fase 2": {"Number": 30.0},
                    "Fase 3": {"Vector2": [1e39, 0.0]},
                    "Fase 4": "sem tipo",
                }},
                name.clone(): {"name": "Nome", "values": {"": {"Text": long}}},
                run.clone(): {"name": "Tempo da fase", "values": {"": {"Number": 9.0}}},
            },
        });
        fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        let (slot, diagnostics) = load(&project, &path);
        assert!(slot.exists && slot.writable);
        // Only the values that fit are kept; the file is left as it is until the game saves.
        let kept: Vec<_> = slot
            .values
            .keys()
            .map(|(id, key)| (id == &best, key.as_str()))
            .collect();
        assert_eq!(kept, [(true, ""), (true, "Fase 2")]);
        assert!(slot.extras.is_empty());
        // The variables come in the order of their IDs, each one's values in key order.
        let mut diagnostics = diagnostics;
        diagnostics.sort();
        assert_eq!(
            diagnostics,
            [
                "Progresso salvo de Tempo da fase ignorado: a variável não é mais salva entre \
                 partidas.",
                "Progresso salvo ignorado: Melhor tempo · Fase 1 é texto, mas a variável Melhor \
                 tempo guarda número; vale o valor inicial.",
                "Progresso salvo ignorado: Melhor tempo · Fase 3 é vetor2, mas a variável Melhor \
                 tempo guarda número; vale o valor inicial.",
                "Progresso salvo ignorado: Nome: o texto passa de 4096 bytes; vale o valor inicial.",
                "Progresso salvo ignorado: o valor de Melhor tempo · Fase 4 não é legível; vale o \
                 valor inicial.",
            ]
        );
        assert_eq!(fs::read(&path).unwrap(), serde_json::to_vec(&file).unwrap());
        // A vector past f32 reads as infinite and is refused as such.
        let mut vectors = project.clone();
        variables::update(&mut vectors, &best, Some(Value::Vector2([0.; 2])), None).unwrap();
        let (slot, diagnostics) = load(&vectors, &path);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.contains("Melhor tempo · Fase 3: o valor precisa ser finito")),
            "{diagnostics:?}"
        );
        assert!(slot.values.keys().all(|(_, key)| key != "Fase 3"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checks_and_folders_refuse_what_a_game_cannot_save() {
        let (project, best, _, run) = game();
        assert!(check(&project, &best, "Fase 1", &Value::Number(1.)).is_ok());
        for (id, key, value, expected) in [
            (
                run.as_str(),
                "",
                Value::Number(1.),
                "a variável Tempo da fase não é salva entre partidas",
            ),
            (
                best.as_str(),
                "",
                Value::Number(f64::NAN),
                "Melhor tempo: o valor precisa ser finito",
            ),
            (
                "outra",
                "",
                Value::Number(1.),
                "a variável não existe no projeto",
            ),
        ] {
            assert_eq!(check(&project, id, key, &value).unwrap_err(), expected);
        }
        let long = "k".repeat(MAX_TEXT + 1);
        assert_eq!(
            check(&project, &best, &long, &Value::Number(1.)).unwrap_err(),
            "a chave de Melhor tempo passa de 4096 bytes"
        );
        let base = Path::new("C:\\Jogos");
        for id in ["", "..", "a/b", "a\\b", "c:", &"x".repeat(65)] {
            assert!(path(base, id).is_err(), "{id}");
        }
        assert!(path(base, "02000000-0000-4000-8000-00000000000a").is_ok());
    }
}
