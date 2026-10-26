//! Exported games: a folder named after the game with the player as its executable, the project
//! and its files, the licenses and a note for whoever plays, checked by running it before
//! anything at the destination changes. The editor, the MCP server and tests export through
//! here, with the same rules on where a game may go.
use crate::{
    asset_import::{self, normalized, overlaps, real},
    command,
    document::{AssetKind, Id, Project, new_id, validate_project},
    fs_ops::{busy, move_no_replace, retrying},
    headless::{self, SimulationRequest},
    names,
    persistence::{self, PROJECT_FILE},
    script::{self, ScriptSources},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

/// The player's file, next to the editor and the MCP server.
pub const PLAYER_EXE: &str = if cfg!(windows) {
    "oxy_player.exe"
} else {
    "oxy_player"
};
/// The file in an exported game's data folder that says which project it is.
pub const MARKER_FILE: &str = "export.oxy.json";
const MARKER_FORMAT: u32 = 1;
/// Simulated seconds of the check an exported game passes.
pub const VERIFY_SECONDS: f64 = 2.;
/// The longest wait for the exported player's own check.
const PLAYER_TIMEOUT: Duration = Duration::from_secs(120);
/// The longest name of an exported game's folder and executable, in characters.
const MAX_FILE_CHARS: usize = 64;

/// The licenses an exported game carries in its licenses folder: OXY's own and every
/// third-party text the engine ships, embedded so the editor needs no folder next to it.
pub const LICENSES: [(&str, &str); 9] = [
    ("OXY-Engine-MIT.txt", include_str!("../../../LICENSE")),
    ("LEIA-ME.txt", include_str!("../../../licenses/LEIA-ME.txt")),
    (
        "cpal-0.18.2.txt",
        include_str!("../../../licenses/cpal-0.18.2.txt"),
    ),
    (
        "ibm-plex-ofl-1.1.txt",
        include_str!("../../../licenses/ibm-plex-ofl-1.1.txt"),
    ),
    (
        "kenney-skyboxes-cc0.txt",
        include_str!("../../../licenses/kenney-skyboxes-cc0.txt"),
    ),
    (
        "luau-0.740.txt",
        include_str!("../../../licenses/luau-0.740.txt"),
    ),
    (
        "mlua-0.12.2.txt",
        include_str!("../../../licenses/mlua-0.12.2.txt"),
    ),
    (
        "parry-0.30.2.txt",
        include_str!("../../../licenses/parry-0.30.2.txt"),
    ),
    (
        "rapier-0.35.3.txt",
        include_str!("../../../licenses/rapier-0.35.3.txt"),
    ),
];

/// This engine's version as people read it (0.6.0, or 0.3.3.1 for 0.3.3+patch.1).
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").replace("+patch.", ".")
}

/// The name of an exported game's folder and executable: `title` without what Windows refuses
/// in file names (`<>:"/\|?*` and control characters), its spaces collapsed, without dots or
/// spaces at the end, at most 64 characters and never a device name (CON, NUL, COM1…); "Jogo"
/// when nothing is left (see [`names::fit`]).
pub fn file_name(title: &str) -> String {
    names::fit(title, MAX_FILE_CHARS, "Jogo")
}

/// The folder `project` is exported to in `destination`.
pub fn target_folder(destination: &Path, project: &Project) -> PathBuf {
    destination.join(file_name(project.game_title()))
}

/// The player next to `exe` (the editor or the MCP server), or one folder up, where Cargo
/// leaves it for tests.
pub fn player_next_to(exe: &Path) -> Option<PathBuf> {
    let folder = exe.parent()?;
    [Some(folder), folder.parent()]
        .into_iter()
        .flatten()
        .map(|folder| folder.join(PLAYER_EXE))
        .find(|player| player.is_file())
}

/// What stops an export (`errors`) and what the game will take otherwise than it may seem
/// (`warnings`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ExportCheck {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Checks `project` for export: it must pass validation, every image and sound must be at
/// `source_root` and readable, unless it is in `unsaved` (painted and not saved, exported from
/// memory), and its scripts must be there and load (see `script_errors`). The warnings are
/// those of [`command::warnings`].
pub fn check(project: &Project, source_root: Option<&Path>, unsaved: &HashSet<Id>) -> ExportCheck {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if let Err(error) = validate_project(project) {
        errors.push(error);
    }
    for asset in &project.assets {
        if unsaved.contains(&asset.id) {
            if asset.kind != AssetKind::Texture {
                errors.push(format!(
                    "{} não é uma imagem, mas veio com pixels pintados",
                    asset.name
                ));
            }
            continue;
        }
        if asset.kind == AssetKind::Model && asset.model.is_some() {
            continue;
        }
        let Some(root) = source_root else {
            errors.push(format!(
                "{}: o projeto ainda não foi salvo numa pasta; salve antes de exportar",
                asset.name
            ));
            continue;
        };
        if let Some(problem) = persistence::asset_problem(project, root, asset) {
            if persistence::asset_used(project, asset) {
                errors.push(format!(
                    "{}: {problem}; restaure o arquivo ou deixe de usar o recurso",
                    asset.name
                ));
            } else {
                warnings.push(format!(
                    "{}: {problem}; nada usa o recurso, e ele fica fora do jogo",
                    asset.name
                ));
            }
        }
    }
    if unsaved.iter().any(|id| project.asset(id).is_none()) {
        errors.push("Uma imagem pintada não pertence mais ao projeto".into());
    }
    let (script_errors, script_warnings) = script_errors(project, source_root);
    errors.extend(script_errors);
    let mut all: Vec<String> = command::warnings(project)
        .into_iter()
        .map(|warning| warning.message)
        .collect();
    all.extend(warnings);
    all.extend(script_warnings);
    ExportCheck {
        errors,
        warnings: all,
    }
}

/// The project as the game gets it: without the images and sounds nothing uses whose files
/// cannot be read and without the models whose files could not be read and that nothing places
/// (the check warned of both), every model's pieces inside the project file.
fn exported(
    project: &Project,
    source_root: Option<&Path>,
    unsaved: &HashMap<Id, Vec<u8>>,
) -> Project {
    let mut exported = project.clone();
    exported.assets.retain(|asset| {
        unsaved.contains_key(&asset.id)
            || (asset.kind == AssetKind::Model && asset.model.is_some())
            || source_root
                .is_none_or(|root| persistence::asset_problem(project, root, asset).is_none())
            || persistence::asset_used(project, asset)
    });
    exported
}

/// What stops exporting the scripts of `project`, whose folder is `root`, and what is only
/// warned. Every script of the tree that compiles goes to the game, attached or not (a
/// `require` may build the name it loads); the export stops for a script an object attaches
/// that the tree lacks, and for one the game uses (attached, or reached by a literal `require`
/// from one, see [`script::used_scripts`]) that does not compile, that loading refuses before
/// it runs or that cannot be read. One nothing uses only warns, and stays out of the game.
fn script_errors(project: &Project, root: Option<&Path>) -> (Vec<String>, Vec<String>) {
    let attached = script::attached_paths(project);
    let Some(root) = root else {
        if attached.is_empty() {
            return Default::default();
        }
        return (
            vec![
                "O projeto ainda não foi salvo numa pasta, onde ficam os seus scripts; salve \
                 antes de exportar"
                    .into(),
            ],
            Vec::new(),
        );
    };
    let sources = match persistence::load_scripts(root) {
        Ok(sources) => sources,
        Err(error) if attached.is_empty() => {
            return (
                Vec::new(),
                vec![format!("Os scripts não vão para o jogo: {error}")],
            );
        }
        Err(error) => return (vec![error.to_string()], Vec::new()),
    };
    let used = script::used_scripts(&sources, attached.iter().cloned());
    let mut errors: Vec<String> = script::absent_by_file(project, &sources)
        .into_iter()
        .map(|(line, objects)| format!("{line}; {}", persistence::missing_fix(objects)))
        .collect();
    let mut warnings = Vec::new();
    let unused = "nenhum objeto usa este script, e ele fica fora do jogo";
    for (path, file) in &sources.files {
        if let Err(error) = script::check(path, &file.text) {
            if used.contains(path) {
                errors.push(error.to_string());
            } else {
                warnings.push(format!("{error}; {unused}"));
            }
        }
    }
    for (path, reason) in &sources.unreadable {
        let line = format!("O script {path} {reason}");
        if attached.contains(path) {
            // Told above, with the objects.
        } else if used.contains(path) {
            errors.push(line);
        } else {
            warnings.push(format!("{line}; {unused}"));
        }
    }
    (errors, warnings)
}

/// What an exported game's check found: the project loads and its start scene runs without
/// a window for a few seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub ok: bool,
    /// The version of the engine that checked it.
    pub engine: String,
    /// The game's title, when the project loaded.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game: String,
    /// The name of the scene the game starts in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_scene: Option<String>,
    /// The seconds the game ran.
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub errors: Vec<String>,
    /// What the game logged while it ran, with the objects' names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
}

/// Checks the game in the data folder `data`: the project loads as the player loads it (every
/// image and sound read in full, every script its objects attach found in the tree of `data`,
/// and every script there, all of which the export checked, checked again) and its start scene
/// runs for `seconds` without a window, its scripts with it.
pub fn verify(data: &Path, seconds: f64) -> VerifyReport {
    let mut report = VerifyReport {
        engine: engine_version(),
        ..Default::default()
    };
    match persistence::load_project(&data.join(PROJECT_FILE)) {
        Err(error) => report.errors.push(error.to_string()),
        Ok(project) => {
            report.game = project.game_title().to_owned();
            report.start_scene = project
                .scene(&project.start_scene)
                .map(|scene| scene.name.clone());
            let scripts = match persistence::load_scripts(data) {
                Ok(scripts) => scripts,
                Err(error) => {
                    report.errors.push(error.to_string());
                    ScriptSources::default()
                }
            };
            report.errors.extend(
                script::absent_by_file(&project, &scripts)
                    .into_iter()
                    .map(|(line, _)| line),
            );
            // The copies, which a script saved after the export's check or edited in the
            // game's folder may have changed.
            for (path, file) in &scripts.files {
                if let Err(error) = script::check(path, &file.text) {
                    report.errors.push(error.to_string());
                }
            }
            let attached = script::attached_paths(&project);
            for (path, reason) in &scripts.unreadable {
                if !attached.contains(path) {
                    report.errors.push(format!("O script {path} {reason}"));
                }
            }
            let ran = serde_json::from_value::<SimulationRequest>(
                serde_json::json!({"seconds": seconds, "ui": false}),
            )
            .map_err(|e| e.to_string())
            .and_then(|request| {
                headless::run_with_scripts(&project, Arc::new(scripts), &request)
                    .map(|(run, _)| run)
            });
            match ran {
                Ok(run) => {
                    report.seconds = run.seconds;
                    report.logs = run
                        .logs
                        .into_iter()
                        .map(|log| match log.object {
                            Some(object) => format!("{}: {}", object.name, log.text),
                            None => log.text,
                        })
                        .collect();
                }
                Err(error) => report.errors.push(error),
            }
        }
    }
    report.ok = report.errors.is_empty();
    report
}

/// What an exported game's [`MARKER_FILE`] holds: the proof that a folder is an export of a
/// project, which only then may be replaced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportMarker {
    pub format: u32,
    pub engine: String,
    pub project_id: Id,
    pub title: String,
    /// The executable's file name.
    pub exe: String,
}

/// An export, as the editor, the MCP server and tests ask for it.
#[derive(Clone, Copy, Debug)]
pub struct ExportRequest<'a> {
    pub project: &'a Project,
    /// The project's folder, where its images and sounds are; None for a project never saved,
    /// whose images must all be in `unsaved_png`.
    pub source_root: Option<&'a Path>,
    /// PNG files of images painted and not saved yet, by asset, exported in place of the files.
    pub unsaved_png: &'a HashMap<Id, Vec<u8>>,
    /// The player copied as the game's executable (see [`player_next_to`]).
    pub player: &'a Path,
    /// The folder the game's folder goes in (see [`target_folder`]).
    pub destination: &'a Path,
    /// Whether an earlier export of the same project in the game's folder may be replaced.
    pub replace: bool,
    /// Folders the game's folder may neither be in nor hold, besides the project's and the
    /// player's: the engine's own.
    pub protected: &'a [PathBuf],
}

/// A finished export.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExportReport {
    /// The game's folder.
    pub folder: PathBuf,
    pub executable: PathBuf,
    /// Files in the game's folder and their size, in bytes.
    pub files: usize,
    pub bytes: u64,
    /// Whether an earlier export of the game was replaced.
    pub replaced: bool,
    pub warnings: Vec<String>,
    /// The check the exported executable ran on itself.
    pub verify: VerifyReport,
}

/// Why an export failed. Nothing at the destination changed, unless [`ExportError::Restore`]
/// says where the earlier export was left. Messages are shown to people as they are.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(
        "O player dos jogos ({}) não foi encontrado em {}. Extraia o ZIP da OXY Engine inteiro: o player fica ao lado do editor.",
        PLAYER_EXE,
        folder_of(.0).display()
    )]
    PlayerMissing(PathBuf),
    #[error("O projeto não pode ser exportado: {}", .0.join("; "))]
    Invalid(Vec<String>),
    #[error("{0}")]
    UnsafeDestination(String),
    #[error(
        "Já existe uma exportação deste jogo em {}. Peça para substituí-la ou escolha outro destino.",
        .0.display()
    )]
    Exists(PathBuf),
    /// The game's folder, an earlier export or an empty folder, could not move aside.
    #[error(
        "A pasta {} está em uso por outro programa, como o jogo exportado aberto: feche-o e tente de novo. Nada mudou no destino.",
        .0.display()
    )]
    InUse(PathBuf),
    #[error("{context}: {source}. Nada mudou no destino.")]
    Io { context: String, source: io::Error },
    #[error(
        "O jogo exportado não passou na conferência, e nada mudou no destino: {}",
        .0.errors.join(" ")
    )]
    Verify(Box<VerifyReport>),
    #[error(
        "Não foi possível pôr o jogo exportado no lugar nem devolver a exportação anterior, que ficou em {}: {source}",
        .kept.display()
    )]
    Restore { kept: PathBuf, source: io::Error },
}
/// The folder of the file at `path`, or `path` when it names no folder.
fn folder_of(path: &Path) -> &Path {
    path.parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(path)
}
fn io_context(context: impl Into<String>) -> impl FnOnce(io::Error) -> ExportError {
    move |source| ExportError::Io {
        context: context.into(),
        source,
    }
}

/// Exports `request.project` to its folder in `request.destination`: checks the project and
/// the destination, writes the game to a staging folder next to it, loads and runs it there,
/// runs the copied executable's own check (`--verificar`), and only then puts it in place,
/// keeping an earlier export until the new one is there.
pub fn export(request: &ExportRequest) -> Result<ExportReport, ExportError> {
    export_with(request, run_player)
}

/// What an export is doing, for a progress line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    /// Writing the game to the staging folder.
    Copying,
    /// Loading and running the written game, then its executable's own check.
    Verifying,
}

/// [`export`], telling `stage` when each [`ExportStage`] starts.
pub fn export_reporting(
    request: &ExportRequest,
    stage: impl FnMut(ExportStage),
) -> Result<ExportReport, ExportError> {
    export_staged(request, run_player, stage)
}

/// Where an export would put the game, as [`plan`] finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The game's folder.
    pub folder: PathBuf,
    /// Whether the folder holds an earlier export of the same project, which an export asked
    /// to replace it takes the place of.
    pub replaces: bool,
}

/// Where an export of the project `id` titled `title` (its [`Project::game_title`]) to
/// `destination` would put the game, by the export's own rules on destinations and without
/// writing anything: what the editor shows before exporting. `source_root` and `protected` are
/// those of the [`ExportRequest`], the player's folder among the protected.
pub fn plan(
    id: &str,
    title: &str,
    destination: &Path,
    source_root: Option<&Path>,
    protected: &[PathBuf],
) -> Result<Plan, ExportError> {
    let (_, folder, previous) = locate(id, title, destination, source_root, protected, true)?;
    Ok(Plan {
        folder,
        replaces: matches!(previous, Previous::Export),
    })
}

/// What is at the game's folder before the export.
enum Previous {
    Nothing,
    Empty,
    Export,
}

fn export_with(
    request: &ExportRequest,
    run: impl FnOnce(&Path) -> Result<VerifyReport, ExportError>,
) -> Result<ExportReport, ExportError> {
    export_staged(request, run, |_| {})
}

/// [`export`], with how the copied executable is checked (tests check it in this process) and
/// the stages told to `stage_reached`.
fn export_staged(
    request: &ExportRequest,
    run: impl FnOnce(&Path) -> Result<VerifyReport, ExportError>,
    mut stage_reached: impl FnMut(ExportStage),
) -> Result<ExportReport, ExportError> {
    let project = request.project;
    let unsaved: HashSet<Id> = request.unsaved_png.keys().cloned().collect();
    let checked = check(project, request.source_root, &unsaved);
    if !checked.errors.is_empty() {
        return Err(ExportError::Invalid(checked.errors));
    }
    if !request.player.is_file() {
        return Err(ExportError::PlayerMissing(request.player.to_owned()));
    }
    let mut protected = request.protected.to_vec();
    protected.extend(request.player.parent().map(Path::to_owned));
    let (destination, target, previous) = locate(
        &project.id,
        project.game_title(),
        request.destination,
        request.source_root,
        &protected,
        request.replace,
    )?;
    let name = file_name(project.game_title());
    let executable = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    stage_reached(ExportStage::Copying);
    let staging = Staging::create(&destination)?;
    stage(request, &staging.path, &executable)?;
    stage_reached(ExportStage::Verifying);
    let verified = verify(&staging.path.join("data"), VERIFY_SECONDS);
    if !verified.ok {
        return Err(ExportError::Verify(Box::new(verified)));
    }
    let mut verified = run(&staging.path.join(&executable))?;
    let version = engine_version();
    if !verified.engine.is_empty() && verified.engine != version {
        verified.ok = false;
        verified.errors.push(format!(
            "O player é da OXY Engine {}, e esta é a {version}: use o {PLAYER_EXE} que veio com ela.",
            verified.engine
        ));
    }
    if !verified.ok {
        return Err(ExportError::Verify(Box::new(verified)));
    }
    let (files, bytes) = measure(&staging.path).map_err(io_context(
        "Não foi possível conferir os arquivos exportados",
    ))?;
    let mut warnings = checked.warnings;
    let replaced = matches!(previous, Previous::Export);
    warnings.extend(swap(staging, &target, &destination, previous)?);
    Ok(ExportReport {
        executable: target.join(&executable),
        folder: target,
        files,
        bytes,
        replaced,
        warnings,
        verify: verified,
    })
}

/// The destination as the export uses it, the game's folder in it and what that folder holds,
/// refusing what the export refuses (see [`destination`], [`guard_target`] and [`previous`]).
fn locate(
    id: &str,
    title: &str,
    destination: &Path,
    source_root: Option<&Path>,
    protected: &[PathBuf],
    replace: bool,
) -> Result<(PathBuf, PathBuf, Previous), ExportError> {
    let destination = self::destination(destination)?;
    let target = destination.join(file_name(title));
    guard_target(&target, source_root, protected)?;
    let previous = previous(&target, id, replace)?;
    Ok((destination, target, previous))
}

/// The destination as the export uses it: a full path on a disk of this computer, not a disk's
/// root nor a file, with `.` and `..` worked out.
fn destination(destination: &Path) -> Result<PathBuf, ExportError> {
    let text = destination.to_string_lossy();
    let checked =
        asset_import::local_disk_path(&text, true).map_err(ExportError::UnsafeDestination)?;
    let normal = normalized(checked);
    if normal.parent().is_none() {
        return Err(ExportError::UnsafeDestination(format!(
            "A raiz de um disco ({}) não recebe jogos exportados; escolha uma pasta dentro dele.",
            normal.display()
        )));
    }
    if normal.exists() && !normal.is_dir() {
        return Err(ExportError::UnsafeDestination(format!(
            "{} não é uma pasta; escolha uma pasta para receber o jogo.",
            normal.display()
        )));
    }
    Ok(normal)
}

/// Refuses a game folder that is, holds or lies in the project's folder or a protected one.
fn guard_target(
    target: &Path,
    project: Option<&Path>,
    protected: &[PathBuf],
) -> Result<(), ExportError> {
    let target = real(target);
    if let Some(project) = project
        && overlaps(&target, &real(project))
    {
        return Err(ExportError::UnsafeDestination(format!(
            "A pasta do jogo ({}) não pode ficar dentro da pasta do projeto nem contê-la; escolha outro destino.",
            target.display()
        )));
    }
    if protected
        .iter()
        .any(|folder| overlaps(&target, &real(folder)))
    {
        return Err(ExportError::UnsafeDestination(format!(
            "A pasta do jogo ({}) não pode ficar dentro da pasta da OXY Engine nem contê-la; escolha outro destino.",
            target.display()
        )));
    }
    Ok(())
}

/// What is at the game's folder: nothing, an empty folder or, when `replace` allows it, an
/// earlier export of the project `id`. Anything else is refused.
fn previous(target: &Path, id: &str, replace: bool) -> Result<Previous, ExportError> {
    let meta = match fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Previous::Nothing),
        Err(error) => return Err(io_context("Não foi possível ler o destino")(error)),
    };
    if !meta.is_dir() {
        return Err(ExportError::UnsafeDestination(format!(
            "{} já existe e não é uma pasta; escolha outro destino.",
            target.display()
        )));
    }
    let empty = fs::read_dir(target)
        .map_err(io_context("Não foi possível ler o destino"))?
        .next()
        .is_none();
    if empty {
        return Ok(Previous::Empty);
    }
    let marker = fs::read(target.join("data").join(MARKER_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ExportMarker>(&bytes).ok());
    match marker {
        Some(marker) if marker.project_id == id => {
            if replace {
                Ok(Previous::Export)
            } else {
                Err(ExportError::Exists(target.to_owned()))
            }
        }
        Some(marker) => Err(ExportError::UnsafeDestination(format!(
            "A pasta {} tem a exportação de outro jogo ({}); escolha outro destino.",
            target.display(),
            marker.title
        ))),
        None => Err(ExportError::UnsafeDestination(format!(
            "A pasta {} já existe e tem outros arquivos; a exportação só usa uma pasta nova, vazia ou com uma exportação anterior deste jogo.",
            target.display()
        ))),
    }
}

/// The folder a game is written to before it replaces anything, removed when the export
/// fails, with the folders created for it.
struct Staging {
    path: PathBuf,
    /// Folders up to the destination that did not exist, the deepest first.
    created: Vec<PathBuf>,
    done: bool,
}
impl Staging {
    fn create(destination: &Path) -> Result<Self, ExportError> {
        let created: Vec<PathBuf> = destination
            .ancestors()
            .take_while(|folder| !folder.exists())
            .map(Path::to_owned)
            .collect();
        let mut staging = Self {
            path: destination.join(format!(".oxy-export-{}", new_id())),
            created,
            done: false,
        };
        fs::create_dir_all(destination)
            .map_err(io_context("Não foi possível criar a pasta de destino"))?;
        if let Err(error) = fs::create_dir(&staging.path) {
            staging.path = PathBuf::new();
            return Err(io_context(
                "Não foi possível criar a pasta de trabalho no destino",
            )(error));
        }
        Ok(staging)
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if !self.path.as_os_str().is_empty() {
            let _ = retrying(|| fs::remove_dir_all(&self.path));
        }
        for folder in &self.created {
            // Only empty folders go.
            let _ = fs::remove_dir(folder);
        }
    }
}

/// Writes the game into `staging`: the player as `executable`, never a DLL lying next to it, the
/// project and its images, sounds and scripts in data, the licenses, the note and the marker.
#[expect(
    clippy::disallowed_methods,
    reason = "the staging folder is new and the export's own: nothing there is anyone's"
)]
fn stage(request: &ExportRequest, staging: &Path, executable: &str) -> Result<(), ExportError> {
    let exported = exported(request.project, request.source_root, request.unsaved_png);
    let project = &exported;
    fs::copy(request.player, staging.join(executable))
        .map_err(io_context("Não foi possível copiar o player do jogo"))?;
    let data = staging.join("data");
    fs::create_dir(&data).map_err(io_context("Não foi possível criar a pasta data"))?;
    let document = serde_json::to_vec_pretty(project)
        .map_err(|e| io_context("Não foi possível gravar o projeto")(e.into()))?;
    fs::write(data.join(PROJECT_FILE), document)
        .map_err(io_context("Não foi possível gravar o projeto"))?;
    for asset in project.assets.iter().filter(|a| a.kind != AssetKind::Model) {
        let invalid =
            |error: persistence::PersistenceError| ExportError::Invalid(vec![error.to_string()]);
        let target = persistence::resolve_asset_path(&data, &asset.path).map_err(invalid)?;
        let failed = format!("Não foi possível exportar {}", asset.name);
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder).map_err(io_context(failed.clone()))?;
        }
        match request.unsaved_png.get(&asset.id) {
            Some(bytes) => fs::write(&target, bytes).map_err(io_context(failed))?,
            None => {
                let root = request
                    .source_root
                    .expect("check refuses files without a project folder");
                let source = persistence::resolve_asset_path(root, &asset.path).map_err(invalid)?;
                fs::copy(source, &target)
                    .map(drop)
                    .map_err(io_context(failed))?;
            }
        }
    }
    // Every script of the tree that compiles, at its path, attached or not: a require may build
    // the name it loads. The check warned of the others; `verify` checks what was copied.
    if let Some(root) = request.source_root {
        let sources = match persistence::load_scripts(root) {
            Ok(sources) => sources,
            Err(_) if !script::any_attached(project) => ScriptSources::default(),
            Err(error) => return Err(ExportError::Invalid(vec![error.to_string()])),
        };
        for (path, file) in &sources.files {
            if script::check(path, &file.text).is_err() {
                continue;
            }
            let target = persistence::resolve_asset_path(&data, path)
                .map_err(|error| ExportError::Invalid(vec![error.to_string()]))?;
            let failed = format!("Não foi possível exportar o script {path}");
            if let Some(folder) = target.parent() {
                fs::create_dir_all(folder).map_err(io_context(failed.clone()))?;
            }
            fs::write(&target, &file.text).map_err(io_context(failed))?;
        }
    }
    let licenses = staging.join("licenses");
    fs::create_dir(&licenses).map_err(io_context("Não foi possível criar a pasta licenses"))?;
    for (name, text) in LICENSES {
        fs::write(licenses.join(name), text)
            .map_err(io_context("Não foi possível gravar as licenças"))?;
    }
    fs::write(staging.join("LEIA-ME.txt"), readme(project, executable))
        .map_err(io_context("Não foi possível gravar o LEIA-ME.txt"))?;
    let marker = ExportMarker {
        format: MARKER_FORMAT,
        engine: engine_version(),
        project_id: project.id.clone(),
        title: project.game_title().to_owned(),
        exe: executable.to_owned(),
    };
    let marker = serde_json::to_vec_pretty(&marker).map_err(|e| {
        io_context("Não foi possível gravar o marcador da exportação")(e.into())
    })?;
    fs::write(data.join(MARKER_FILE), marker).map_err(io_context(
        "Não foi possível gravar o marcador da exportação",
    ))
}

/// The note next to the game, for whoever plays it.
fn readme(project: &Project, executable: &str) -> String {
    [
        project.game_title().to_owned(),
        String::new(),
        format!("Para jogar, abra {executable}."),
        "Mantenha a pasta data e os outros arquivos desta pasta ao lado dele.".into(),
        String::new(),
        "Esc: pausa (ou o menu de pausa do jogo, quando ele tiver um)".into(),
        "Alt+Enter: alterna entre janela e tela cheia".into(),
        "F3: diagnóstico do jogo".into(),
        String::new(),
        "Se o jogo salvar o progresso, ele fica em %APPDATA%\\OXY Engine\\Jogos, numa pasta \
         deste jogo."
            .into(),
        "Requisitos: Windows 10/11 x64 e um driver gráfico compatível com DirectX 12 ou Vulkan."
            .into(),
        "O executável não tem assinatura digital: o Windows pode pedir confirmação ao abri-lo."
            .into(),
        String::new(),
        format!(
            "Feito com OXY Engine {} (licença MIT). As licenças da OXY Engine e das bibliotecas \
             que ela usa estão na pasta licenses.",
            engine_version()
        ),
        String::new(),
    ]
    .join("\r\n")
}

/// Runs the exported `exe` with `--verificar`, as people get it, and reads its report.
fn run_player(exe: &Path) -> Result<VerifyReport, ExportError> {
    let report = std::env::temp_dir().join(format!("oxy-export-{}.json", new_id()));
    let mut command = Command::new(exe);
    command
        .arg("--verificar")
        .arg(&report)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A console build of the player opens no console window either.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(io_context(
        "Não foi possível abrir o jogo exportado para conferi-lo",
    ))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < PLAYER_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let read = fs::read(&report)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<VerifyReport>(&bytes).ok());
    let _ = fs::remove_file(&report);
    let written = read.is_some();
    let mut verified = read.unwrap_or_default();
    let code = |status: std::process::ExitStatus| {
        status
            .code()
            .map_or_else(|| "sem código".to_owned(), |code| format!("código {code}"))
    };
    match status {
        None => verified.errors.push(format!(
            "O jogo exportado não terminou a conferência em {} segundos.",
            PLAYER_TIMEOUT.as_secs()
        )),
        Some(status) if !written => verified.errors.push(format!(
            "O jogo exportado não escreveu o relatório da conferência ({}); o player pode ser de \
             outra versão da OXY Engine.",
            code(status)
        )),
        Some(status) if !status.success() && verified.errors.is_empty() => verified.errors.push(
            format!("O jogo exportado falhou na conferência ({}).", code(status)),
        ),
        Some(_) => {}
    }
    verified.ok = verified.ok && status.is_some_and(|s| s.success()) && verified.errors.is_empty();
    Ok(verified)
}

/// The files under `folder` and their size, in bytes.
fn measure(folder: &Path) -> io::Result<(usize, u64)> {
    let mut found = (0, 0);
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            let (files, bytes) = measure(&entry.path())?;
            found.0 += files;
            found.1 += bytes;
        } else {
            found.0 += 1;
            found.1 += meta.len();
        }
    }
    Ok(found)
}

/// Puts the staged game at `target`: an earlier export (or empty folder) moves aside first and
/// comes back when the game cannot take its place; removed once it did. Answers a warning when
/// the earlier export could not be removed.
fn swap(
    mut staging: Staging,
    target: &Path,
    destination: &Path,
    previous: Previous,
) -> Result<Option<String>, ExportError> {
    let kept = match previous {
        Previous::Nothing => None,
        Previous::Empty | Previous::Export => {
            let kept = destination.join(format!(".oxy-export-previous-{}", new_id()));
            retrying(|| move_no_replace(target, &kept)).map_err(|error| {
                if busy(&error) {
                    ExportError::InUse(target.to_owned())
                } else {
                    io_context("Não foi possível afastar a exportação anterior")(error)
                }
            })?;
            Some(kept)
        }
    };
    if let Err(error) = retrying(|| move_no_replace(&staging.path, target)) {
        if let Some(kept) = kept
            && let Err(source) = retrying(|| move_no_replace(&kept, target))
        {
            return Err(ExportError::Restore { kept, source });
        }
        return Err(io_context("Não foi possível pôr o jogo exportado no lugar")(error));
    }
    staging.done = true;
    Ok(kept.and_then(|kept| {
        retrying(|| fs::remove_dir_all(&kept)).err().map(|_| {
            format!(
                "A exportação anterior ficou em {}; apague essa pasta quando puder.",
                kept.display()
            )
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Asset, Entity},
        painting::PaintImage,
    };

    fn png(color: [u8; 4]) -> Vec<u8> {
        PaintImage::new(4, 4, color).unwrap().to_png().unwrap()
    }
    fn wav() -> Vec<u8> {
        let mut bytes = Vec::new();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(io::Cursor::new(&mut bytes), spec).unwrap();
        for _ in 0..800 {
            writer.write_sample(1000i16).unwrap();
        }
        writer.finalize().unwrap();
        bytes
    }

    /// A saved project with an image, a sound, a painted image and a model, with backups and a
    /// file it does not use lying in its folder, and a fake player.
    struct Fixture {
        base: PathBuf,
        folder: PathBuf,
        project: Project,
        painted: Id,
        unsaved: HashMap<Id, Vec<u8>>,
        player: PathBuf,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!("oxy-export-{name}-{}", new_id()));
            let folder = base.join("Projeto");
            fs::create_dir_all(folder.join("assets")).unwrap();
            let mut project = Project::new("Corrida");
            project.game.title = "Corrida: Noturna?".into();
            let mut add = |name: &str, path: &str, kind: AssetKind, bytes: &[u8]| {
                fs::write(folder.join(path), bytes).unwrap();
                let id = new_id();
                project.assets.push(Asset {
                    id: id.clone(),
                    name: name.into(),
                    path: path.into(),
                    kind,
                    model: None,
                });
                id
            };
            add(
                "Placa",
                "assets/placa.png",
                AssetKind::Texture,
                &png([9; 4]),
            );
            add("Bip", "assets/bip.wav", AssetKind::Audio, &wav());
            let painted = add(
                "Pintura",
                "assets/pintura.png",
                AssetKind::Texture,
                &png([255; 4]),
            );
            let piece = Entity::new("Grupo", None);
            let piece_id = piece.id.clone();
            let scene = project.start_scene.clone();
            project.scenes[0].entities.push(piece);
            project.save_model(&scene, &piece_id, "Peça").unwrap();
            persistence::save_project(&folder.join(PROJECT_FILE), &project).unwrap();
            fs::write(folder.join("assets/sobra.png"), png([1; 4])).unwrap();
            fs::write(folder.join("project.oxy.json.agente.bak"), b"{}").unwrap();
            fs::write(
                folder.join("project.oxy.json.schema-4.1.backup.json"),
                b"{}",
            )
            .unwrap();
            let engine = base.join("OXY Engine");
            fs::create_dir_all(&engine).unwrap();
            let player = engine.join(PLAYER_EXE);
            fs::write(&player, b"player de teste").unwrap();
            let unsaved = HashMap::from([(painted.clone(), png([0, 200, 0, 255]))]);
            Self {
                base,
                folder,
                project,
                painted,
                unsaved,
                player,
            }
        }
        fn request<'a>(&'a self, destination: &'a Path, replace: bool) -> ExportRequest<'a> {
            ExportRequest {
                project: &self.project,
                source_root: Some(&self.folder),
                unsaved_png: &self.unsaved,
                player: &self.player,
                destination,
                replace,
                protected: &[],
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }
    /// The exported executable's check, run in this process: what the real one does with
    /// `--verificar`.
    fn checked(exe: &Path) -> Result<VerifyReport, ExportError> {
        Ok(verify(&exe.parent().unwrap().join("data"), VERIFY_SECONDS))
    }
    /// Every file under `folder`, relative to it, with '/'.
    fn files(folder: &Path) -> Vec<String> {
        let mut found = Vec::new();
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                found.extend(files(&path).into_iter().map(|f| format!("{name}/{f}")));
            } else {
                found.push(name);
            }
        }
        found.sort();
        found
    }
    fn names(folder: &Path) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_game_folder_holds_the_player_the_project_its_files_and_the_licenses() {
        let fixture = Fixture::new("pasta");
        let destination = fixture.base.join("Jogos");
        let project_file = fs::read(fixture.folder.join(PROJECT_FILE)).unwrap();
        let report = export_with(&fixture.request(&destination, false), checked).unwrap();
        let folder = destination.join("Corrida Noturna");
        assert_eq!(report.folder, folder);
        assert_eq!(report.executable, folder.join("Corrida Noturna.exe"));
        assert!(!report.replaced);
        let mut expected = vec![
            "Corrida Noturna.exe".to_owned(),
            "LEIA-ME.txt".into(),
            "data/assets/bip.wav".into(),
            "data/assets/pintura.png".into(),
            "data/assets/placa.png".into(),
            "data/export.oxy.json".into(),
            "data/project.oxy.json".into(),
        ];
        expected.extend(LICENSES.iter().map(|(name, _)| format!("licenses/{name}")));
        expected.sort();
        // Backups, the file the project does not use and the model (in the document) stay out.
        assert_eq!(files(&folder), expected);
        assert_eq!(report.files, expected.len());
        let bytes: u64 = expected
            .iter()
            .map(|f| fs::metadata(folder.join(f)).unwrap().len())
            .sum();
        assert_eq!(report.bytes, bytes);
        // Nothing else at the destination, and the project as it was.
        assert_eq!(names(&destination), ["Corrida Noturna"]);
        assert_eq!(
            fs::read(fixture.folder.join(PROJECT_FILE)).unwrap(),
            project_file
        );
        assert_eq!(
            fs::read(&report.executable).unwrap(),
            fs::read(&fixture.player).unwrap()
        );
        for file in ["assets/placa.png", "assets/bip.wav"] {
            assert_eq!(
                fs::read(folder.join("data").join(file)).unwrap(),
                fs::read(fixture.folder.join(file)).unwrap(),
                "{file}"
            );
        }
        // The painted image goes as it is in memory.
        assert_eq!(
            fs::read(folder.join("data/assets/pintura.png")).unwrap(),
            fixture.unsaved[&fixture.painted]
        );
        let exported: Project =
            serde_json::from_slice(&fs::read(folder.join("data").join(PROJECT_FILE)).unwrap())
                .unwrap();
        assert_eq!(exported, fixture.project);
        for (name, text) in LICENSES {
            assert_eq!(
                fs::read(folder.join("licenses").join(name)).unwrap(),
                text.as_bytes()
            );
        }
        let readme = fs::read_to_string(folder.join("LEIA-ME.txt")).unwrap();
        for text in [
            "Corrida: Noturna?",
            "abra Corrida Noturna.exe",
            "Alt+Enter",
            "%APPDATA%\\OXY Engine\\Jogos",
            &format!("OXY Engine {}", engine_version()),
        ] {
            assert!(readme.contains(text), "{text}: {readme}");
        }
        assert!(!readme.contains(&fixture.project.id), "{readme}");
        let marker: ExportMarker =
            serde_json::from_slice(&fs::read(folder.join("data").join(MARKER_FILE)).unwrap())
                .unwrap();
        assert_eq!(
            marker,
            ExportMarker {
                format: 1,
                engine: engine_version(),
                project_id: fixture.project.id.clone(),
                title: "Corrida: Noturna?".into(),
                exe: "Corrida Noturna.exe".into(),
            }
        );
        // The game loads and runs as the player opens it.
        assert!(report.verify.ok, "{:?}", report.verify);
        assert_eq!(report.verify.game, "Corrida: Noturna?");
        assert_eq!(report.verify.start_scene.as_deref(), Some("Cena 2D"));
        assert_eq!(report.verify.seconds, VERIFY_SECONDS);
        let loaded = persistence::load_project(&folder.join("data")).unwrap();
        assert_eq!(loaded, fixture.project);
    }

    #[test]
    fn a_game_never_ships_the_dlls_lying_next_to_the_player() {
        // GCC's runtime, left by an older build: the player links it statically since 0.7, and
        // these would go without their licenses.
        let fixture = Fixture::new("dll");
        let engine = fixture.player.parent().unwrap();
        for dll in [
            "libgcc_s_seh-1.dll",
            "libstdc++-6.dll",
            "libwinpthread-1.dll",
        ] {
            fs::write(engine.join(dll), b"dll").unwrap();
        }
        let destination = fixture.base.join("Jogos");
        let report = export_with(&fixture.request(&destination, false), checked).unwrap();
        assert!(report.verify.ok, "{:?}", report.verify);
        let exported = files(&report.folder);
        let dlls: Vec<&String> = exported
            .iter()
            .filter(|f| f.to_ascii_lowercase().ends_with(".dll"))
            .collect();
        assert!(dlls.is_empty(), "{dlls:?}");
        assert_eq!(report.files, exported.len());
        assert_eq!(
            names(&report.folder),
            ["Corrida Noturna.exe", "LEIA-ME.txt", "data", "licenses"]
        );
    }

    #[test]
    fn a_game_of_the_tree_layout_takes_its_models_inside_its_project_file() {
        let mut fixture = Fixture::new("arvore");
        fixture.project.layout = crate::document::Layout::Tree;
        let model = fixture
            .project
            .assets
            .iter_mut()
            .find(|a| a.kind == AssetKind::Model)
            .unwrap();
        model.path = "Peça.modelo.json".into();
        let (model, pieces) = (model.id.clone(), model.model.clone().unwrap());
        let scene = fixture.project.start_scene.clone();
        fixture.project.instantiate_model(&model, &scene).unwrap();
        persistence::save_project(&fixture.folder.join(PROJECT_FILE), &fixture.project).unwrap();
        assert!(fixture.folder.join("Peça.modelo.json").is_file());
        let destination = fixture.base.join("Jogos");
        let report = export_with(&fixture.request(&destination, false), checked).unwrap();
        assert!(report.verify.ok, "{:?}", report.verify);
        let data = destination.join("Corrida Noturna").join("data");
        // The model's file stays out; its pieces are in the project file the player reads.
        assert!(!data.join("Peça.modelo.json").exists());
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(data.join(PROJECT_FILE)).unwrap()).unwrap();
        assert_eq!(document["layout"], "Tree");
        let loaded = persistence::load_project(&data).unwrap();
        assert_eq!(loaded.asset(&model).unwrap().model.as_ref(), Some(&pieces));
        assert_eq!(loaded, fixture.project);
        // A model whose file cannot be read stops the export while it is placed.
        let mut unavailable = fixture.project.clone();
        unavailable
            .assets
            .iter_mut()
            .find(|a| a.id == model)
            .unwrap()
            .model = None;
        fs::remove_file(fixture.folder.join("Peça.modelo.json")).unwrap();
        let found = check(&unavailable, Some(&fixture.folder), &HashSet::new());
        assert_eq!(
            found.errors,
            [
                "Peça: o arquivo Peça.modelo.json não existe na pasta do projeto; restaure o \
              arquivo ou deixe de usar o recurso"
            ]
        );
        // Without placements it only warns, and stays out of the game.
        unavailable.scenes[0]
            .entities
            .retain(|e| e.model_source.as_deref() != Some(model.as_str()));
        let found = check(&unavailable, Some(&fixture.folder), &HashSet::new());
        assert!(found.errors.is_empty(), "{found:?}");
        assert!(
            found
                .warnings
                .iter()
                .any(|w| w.contains("Peça.modelo.json") && w.contains("fica fora do jogo")),
            "{found:?}"
        );
        let game = exported(&unavailable, Some(&fixture.folder), &HashMap::new());
        assert!(game.asset(&model).is_none());
    }

    #[test]
    fn file_names_are_what_windows_accepts() {
        for (title, expected) in [
            ("Corrida", "Corrida"),
            (
                "Fase: 1/2 <final> \"sim\" | não? *",
                "Fase 1 2 final sim não",
            ),
            ("Pão & Jogo", "Pão & Jogo"),
            ("  Muitos   espaços\tno\nmeio  ", "Muitos espaços no meio"),
            ("Termina com pontos...", "Termina com pontos"),
            ("Ponto . ", "Ponto"),
            ("CON", "Jogo CON"),
            ("nul.txt", "Jogo nul.txt"),
            ("com1", "Jogo com1"),
            ("LPT9 ", "Jogo LPT9"),
            ("COM0", "Jogo COM0"),
            ("lpt0.exe", "Jogo lpt0.exe"),
            ("COM³", "Jogo COM³"),
            ("CONIN$", "Jogo CONIN$"),
            ("conout$", "Jogo conout$"),
            ("COM10", "COM10"),
            ("Console", "Console"),
            ("", "Jogo"),
            ("...", "Jogo"),
            ("?*:", "Jogo"),
        ] {
            assert_eq!(file_name(title), expected, "{title:?}");
        }
        let long = file_name(&"Á".repeat(100));
        assert_eq!(long.chars().count(), 64);
        let cut = file_name(&format!("{}. depois", "a".repeat(63)));
        assert_eq!(cut, "a".repeat(63));
        let project = Project::new("Meu: Jogo");
        assert_eq!(
            target_folder(Path::new("D:\\Jogos"), &project),
            Path::new("D:\\Jogos").join("Meu Jogo")
        );
    }

    #[test]
    fn unsafe_destinations_are_refused_before_anything_is_written() {
        let fixture = Fixture::new("destinos");
        let refused = |destination: &Path, protected: &[PathBuf]| {
            let request = ExportRequest {
                protected,
                ..fixture.request(destination, true)
            };
            let error = export_with(&request, checked).unwrap_err();
            assert!(
                matches!(error, ExportError::UnsafeDestination(_)),
                "{error}"
            );
            error.to_string()
        };
        let base = &fixture.base;
        for (destination, expected) in [
            (PathBuf::from("Jogos"), "caminho completo da pasta"),
            (PathBuf::from("\\\\servidor\\jogos"), "rede"),
            (PathBuf::from("//servidor/jogos"), "rede"),
            (PathBuf::from("\\\\?\\C:\\Jogos"), "dispositivo"),
            (PathBuf::from("C:\\"), "raiz de um disco"),
            (PathBuf::from("C:\\Jogos\\.."), "raiz de um disco"),
            // In the project's folder.
            (fixture.folder.clone(), "pasta do projeto"),
            (fixture.folder.join("assets"), "pasta do projeto"),
            // In the engine's folder, where the player is.
            (
                fixture.player.parent().unwrap().to_owned(),
                "pasta da OXY Engine",
            ),
        ] {
            let error = refused(&destination, &[]);
            assert!(error.contains(expected), "{destination:?}: {error}");
        }
        // A game folder that would be the project's, or hold it.
        let mut named = Fixture::new("destinos-nome");
        named.project.game.title = "Projeto".into();
        let error = export_with(&named.request(&named.base, false), checked).unwrap_err();
        assert!(error.to_string().contains("pasta do projeto"), "{error}");
        named.project.game.title = named.base.file_name().unwrap().to_string_lossy().into();
        let parent = named.base.parent().unwrap().to_owned();
        let error = export_with(&named.request(&parent, false), checked).unwrap_err();
        assert!(error.to_string().contains("pasta do projeto"), "{error}");
        // Protected folders, given in any case, or inside the game's folder.
        let engine = base.join("Motor");
        fs::create_dir_all(&engine).unwrap();
        let shouting = PathBuf::from(engine.to_string_lossy().to_uppercase());
        let error = refused(&engine, std::slice::from_ref(&shouting));
        assert!(error.contains("pasta da OXY Engine"), "{error}");
        let inner = base.join("Corrida Noturna").join("data");
        let error = refused(base, std::slice::from_ref(&inner));
        assert!(error.contains("pasta da OXY Engine"), "{error}");
        // A file, a folder with other files and another game's export.
        let destination = base.join("Jogos");
        fs::create_dir_all(&destination).unwrap();
        let target = destination.join("Corrida Noturna");
        fs::write(&target, b"arquivo").unwrap();
        assert!(refused(&destination, &[]).contains("não é uma pasta"));
        fs::remove_file(&target).unwrap();
        fs::write(destination.join("arquivo.txt"), b"x").unwrap();
        assert!(refused(&destination.join("arquivo.txt"), &[]).contains("não é uma pasta"));
        fs::create_dir_all(target.join("data")).unwrap();
        fs::write(target.join("notas.txt"), b"meu").unwrap();
        assert!(refused(&destination, &[]).contains("já existe e tem outros arquivos"));
        let other = ExportMarker {
            format: 1,
            engine: engine_version(),
            project_id: new_id(),
            title: "Outro Jogo".into(),
            exe: "Outro Jogo.exe".into(),
        };
        fs::write(
            target.join("data").join(MARKER_FILE),
            serde_json::to_vec(&other).unwrap(),
        )
        .unwrap();
        let error = refused(&destination, &[]);
        assert!(
            error.contains("exportação de outro jogo (Outro Jogo)"),
            "{error}"
        );
        // Nothing was written anywhere.
        assert_eq!(fs::read(target.join("notas.txt")).unwrap(), b"meu");
        assert_eq!(names(&destination), ["Corrida Noturna", "arquivo.txt"]);
        assert_eq!(names(base), ["Jogos", "Motor", "OXY Engine", "Projeto"]);
    }

    #[test]
    fn an_earlier_export_is_replaced_only_when_asked_and_kept_while_the_game_runs() {
        let mut fixture = Fixture::new("substituir");
        let destination = fixture.base.join("Jogos");
        let first = export_with(&fixture.request(&destination, false), checked).unwrap();
        // An empty folder takes the game too.
        let empty = fixture.base.join("Vazia");
        fs::create_dir_all(empty.join("Corrida Noturna")).unwrap();
        export_with(&fixture.request(&empty, false), checked).unwrap();
        assert!(empty.join("Corrida Noturna/Corrida Noturna.exe").is_file());
        assert_eq!(names(&empty), ["Corrida Noturna"]);
        // Again without replace: refused, with the way out.
        let error = export_with(&fixture.request(&destination, false), checked).unwrap_err();
        assert!(matches!(error, ExportError::Exists(_)), "{error}");
        assert!(
            error.to_string().contains("Peça para substituí-la"),
            "{error}"
        );
        // With replace, the new game takes its place and the old one goes.
        fixture.unsaved.clear();
        let second = export_with(&fixture.request(&destination, true), checked).unwrap();
        assert!(second.replaced);
        assert_eq!(second.folder, first.folder);
        assert_eq!(names(&destination), ["Corrida Noturna"]);
        assert_eq!(
            fs::read(second.folder.join("data/assets/pintura.png")).unwrap(),
            fs::read(fixture.folder.join("assets/pintura.png")).unwrap()
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // A file of the old game held open, as the running game holds its executable.
            let held = second.folder.join("data").join(PROJECT_FILE);
            let before = fs::read(&held).unwrap();
            let lock = fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&held)
                .unwrap();
            fixture.project.game.fullscreen = true;
            let error = export_with(&fixture.request(&destination, true), checked).unwrap_err();
            assert!(matches!(error, ExportError::InUse(_)), "{error}");
            assert!(
                error.to_string().contains("está em uso por outro programa")
                    && error.to_string().ends_with("Nada mudou no destino."),
                "{error}"
            );
            drop(lock);
            assert_eq!(fs::read(&held).unwrap(), before);
            assert_eq!(names(&destination), ["Corrida Noturna"]);
            let third = export_with(&fixture.request(&destination, true), checked).unwrap();
            let exported = persistence::load_project(&third.folder.join("data")).unwrap();
            assert!(exported.game.fullscreen);
            assert_eq!(names(&destination), ["Corrida Noturna"]);
        }
    }

    #[test]
    fn the_plan_says_where_the_game_goes_without_writing_anything() {
        let fixture = Fixture::new("plano");
        let destination = fixture.base.join("Jogos");
        let (id, title) = (fixture.project.id.as_str(), fixture.project.game_title());
        let planned = |id: &str, title: &str, destination: &Path| {
            plan(id, title, destination, Some(&fixture.folder), &[])
        };
        assert_eq!(
            planned(id, title, &destination).unwrap(),
            Plan {
                folder: destination.join("Corrida Noturna"),
                replaces: false,
            }
        );
        assert!(!destination.exists());
        // The export's refusals, with its messages.
        for (destination, expected) in [
            (PathBuf::from("Jogos"), "caminho completo da pasta"),
            (PathBuf::from("C:\\"), "raiz de um disco"),
            (fixture.folder.join("assets"), "pasta do projeto"),
        ] {
            let error = planned(id, title, &destination).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
        let error = plan(
            id,
            title,
            &destination,
            Some(&fixture.folder),
            std::slice::from_ref(&fixture.base),
        )
        .unwrap_err();
        assert!(error.to_string().contains("pasta da OXY Engine"), "{error}");
        // An export says when it copies and when it checks; then the plan replaces it.
        let mut stages = Vec::new();
        export_staged(&fixture.request(&destination, false), checked, |stage| {
            stages.push(stage)
        })
        .unwrap();
        assert_eq!(stages, [ExportStage::Copying, ExportStage::Verifying]);
        assert!(planned(id, title, &destination).unwrap().replaces);
        // Another title goes to another folder; another project may not replace this one.
        assert_eq!(
            planned(id, "Outra Corrida", &destination).unwrap(),
            Plan {
                folder: destination.join("Outra Corrida"),
                replaces: false,
            }
        );
        let error = planned(&new_id(), title, &destination).unwrap_err();
        assert!(
            error.to_string().contains("exportação de outro jogo"),
            "{error}"
        );
        // A refused export reaches no stage.
        let mut stages = Vec::new();
        let error = export_staged(&fixture.request(&destination, false), checked, |stage| {
            stages.push(stage)
        })
        .unwrap_err();
        assert!(matches!(error, ExportError::Exists(_)), "{error}");
        assert!(stages.is_empty());
        assert_eq!(names(&destination), ["Corrida Noturna"]);
    }

    #[test]
    fn a_failed_check_leaves_the_destination_as_it_was() {
        let fixture = Fixture::new("conferencia");
        let destination = fixture.base.join("Novos").join("Jogos");
        let failing = |_: &Path| {
            Ok(VerifyReport {
                engine: engine_version(),
                errors: vec!["Asset ausente ou inacessível 'Bip'".into()],
                ..Default::default()
            })
        };
        let error = export_with(&fixture.request(&destination, false), failing).unwrap_err();
        assert!(matches!(error, ExportError::Verify(_)), "{error}");
        let message = error.to_string();
        assert!(
            message.contains("nada mudou no destino") && message.contains("'Bip'"),
            "{message}"
        );
        // The folders made for it went too.
        assert!(!fixture.base.join("Novos").exists());
        // A player of another version is refused by name.
        let older = |exe: &Path| {
            checked(exe).map(|report| VerifyReport {
                engine: "0.0.1".into(),
                ..report
            })
        };
        let error = export_with(&fixture.request(&destination, false), older).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("OXY Engine 0.0.1") && message.contains(PLAYER_EXE),
            "{message}"
        );
        assert!(!fixture.base.join("Novos").exists());
        // An earlier export stays when the new one fails.
        export_with(&fixture.request(&destination, false), checked).unwrap();
        let before = files(&destination);
        export_with(&fixture.request(&destination, true), failing).unwrap_err();
        assert_eq!(files(&destination), before);
        // Without the player, nothing starts.
        let missing = fixture.base.join("sem-player").join(PLAYER_EXE);
        let request = ExportRequest {
            player: &missing,
            ..fixture.request(&destination, true)
        };
        let error = export_with(&request, checked).unwrap_err();
        assert!(matches!(error, ExportError::PlayerMissing(_)), "{error}");
        let message = error.to_string();
        assert!(message.contains("Extraia o ZIP"), "{message}");
        // It names the folder where the player should be.
        assert!(
            message.contains(&format!("em {}.", missing.parent().unwrap().display())),
            "{message}"
        );
    }

    #[test]
    fn a_painted_image_that_would_not_load_stops_before_the_player_runs() {
        let fixture = Fixture::new("pintura-invalida");
        let destination = fixture.base.join("Novos").join("Jogos");
        // The check only knows the painted image is an image; loading the staged game reads it.
        let unsaved = HashMap::from([(fixture.painted.clone(), b"texto".to_vec())]);
        let request = ExportRequest {
            unsaved_png: &unsaved,
            ..fixture.request(&destination, false)
        };
        let error = export_with(&request, |_: &Path| -> Result<VerifyReport, ExportError> {
            panic!("the player must not run")
        })
        .unwrap_err();
        assert!(matches!(error, ExportError::Verify(_)), "{error}");
        let message = error.to_string();
        assert!(
            message.contains("Pintura") && message.contains("nada mudou no destino"),
            "{message}"
        );
        assert!(!fixture.base.join("Novos").exists());
    }

    #[test]
    fn the_check_blocks_what_would_not_load_and_lists_warnings() {
        let mut fixture = Fixture::new("checagem");
        // The images are used: a file that cannot be read blocks only then.
        let mut sign = Entity::new("Placa", Some(crate::document::Primitive::Cube));
        sign.material.texture = Some(fixture.project.assets[0].id.clone());
        let mut painting = Entity::new("Quadro", Some(crate::document::Primitive::Cube));
        painting.material.texture = Some(fixture.painted.clone());
        fixture.project.scenes[0].entities.extend([sign, painting]);
        let none = HashSet::new();
        let ok = check(&fixture.project, Some(&fixture.folder), &none);
        assert!(ok.errors.is_empty() && ok.warnings.is_empty(), "{ok:?}");
        // A project that does not validate.
        let mut broken = fixture.project.clone();
        broken.start_scene = new_id();
        let found = check(&broken, Some(&fixture.folder), &none);
        assert_eq!(found.errors, ["A cena inicial não existe"]);
        // A missing or broken image blocks, unless it goes from memory.
        fs::remove_file(fixture.folder.join("assets/pintura.png")).unwrap();
        fs::write(fixture.folder.join("assets/placa.png"), b"texto").unwrap();
        let found = check(&fixture.project, Some(&fixture.folder), &none);
        assert_eq!(found.errors.len(), 2, "{found:?}");
        assert!(
            found.errors[0].contains("Placa") && found.errors[0].contains("PNG inválido"),
            "{found:?}"
        );
        assert!(found.errors[1].contains("Pintura"), "{found:?}");
        let painted = HashSet::from([fixture.painted.clone()]);
        let found = check(&fixture.project, Some(&fixture.folder), &painted);
        assert_eq!(found.errors.len(), 1, "{found:?}");
        // A project never saved has no files to read.
        let found = check(&fixture.project, None, &painted);
        assert_eq!(found.errors.len(), 2, "{found:?}");
        assert!(found.errors[0].contains("ainda não foi salvo"), "{found:?}");
        let error = export_with(
            &fixture.request(&fixture.base.join("Jogos"), false),
            checked,
        )
        .unwrap_err();
        assert!(
            matches!(error, ExportError::Invalid(_)) && error.to_string().contains("Placa"),
            "{error}"
        );
        assert!(!fixture.base.join("Jogos").exists());
        // Warnings do not block.
        let mut warned = fixture.project.clone();
        warned
            .input_bindings
            .insert("pausar".into(), "Escape".into());
        fs::write(fixture.folder.join("assets/placa.png"), png([9; 4])).unwrap();
        let found = check(&warned, Some(&fixture.folder), &painted);
        assert!(found.errors.is_empty(), "{found:?}");
        assert_eq!(found.warnings.len(), 1, "{found:?}");
        assert!(found.warnings[0].contains("pausar"), "{found:?}");
        // An image nothing uses whose file is missing only warns, and stays out of the game.
        let mut unused = fixture.project.clone();
        unused.scenes[0].entities.retain(|e| e.name != "Quadro");
        let found = check(&unused, Some(&fixture.folder), &none);
        assert!(found.errors.is_empty(), "{found:?}");
        assert_eq!(
            found.warnings,
            [
                "Pintura: o arquivo assets/pintura.png não existe na pasta do projeto; nada usa o \
              recurso, e ele fica fora do jogo"
            ]
        );
        let game = exported(&unused, Some(&fixture.folder), &HashMap::new());
        assert!(game.assets.iter().all(|a| a.name != "Pintura"));
        assert_eq!(game.assets.len(), unused.assets.len() - 1);
    }

    /// Gives the fixture a door that runs a script, which requires another one of the folder;
    /// the folder also holds a file that is no script and an agent's copy of a script, where
    /// older sessions left them, and the project another copy, where they go now.
    fn scripted(fixture: &mut Fixture) {
        let mut door = Entity::new("Porta", None);
        door.scripts = vec![crate::script::ScriptRef::new("scripts/porta.luau")];
        fixture.project.scenes[0].entities.push(door);
        let write = |path: &str, text: &str| {
            let file = fixture.folder.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        };
        write(
            "scripts/porta.luau",
            "local Textos = require('./dados/textos')\nlocal M = {}\nfunction M.on_start(self) \
             print(Textos.ABERTA) end\nreturn M\n",
        );
        write(
            "scripts/dados/textos.luau",
            "return { ABERTA = 'aberta' }\n",
        );
        write("scripts/notas.txt", "fica de fora");
        write("scripts/porta.luau.agente.bak", "return {\n");
        write(
            &persistence::script_backup_path("scripts/porta.luau"),
            "return {}\n",
        );
        persistence::save_project(&fixture.folder.join(PROJECT_FILE), &fixture.project).unwrap();
    }

    #[test]
    fn a_game_takes_its_scripts_to_data_and_its_check_runs_them() {
        let mut fixture = Fixture::new("scripts");
        scripted(&mut fixture);
        let destination = fixture.base.join("Jogos");
        let report = export_with(&fixture.request(&destination, false), checked).unwrap();
        let data = report.folder.join("data");
        let scripts: Vec<String> = files(&data)
            .into_iter()
            .filter(|file| file.starts_with("scripts/"))
            .collect();
        // Every script of the folder, the one only required too; the rest stays out, and no
        // copy of a script goes anywhere in the game.
        assert_eq!(scripts, ["scripts/dados/textos.luau", "scripts/porta.luau"]);
        assert!(
            files(&report.folder)
                .iter()
                .all(|file| !file.ends_with(".bak") && !file.contains(".oxy/")),
            "{:?}",
            files(&report.folder)
        );
        for file in &scripts {
            assert_eq!(
                fs::read(data.join(file)).unwrap(),
                fs::read(fixture.folder.join(file)).unwrap()
            );
        }
        assert!(report.verify.ok, "{:?}", report.verify);
        assert!(
            report
                .verify
                .logs
                .iter()
                .any(|line| line.ends_with("Porta · aberta")),
            "{:?}",
            report.verify.logs
        );
        // A script taken from the exported game fails its check, by the object and the file.
        fs::remove_file(data.join("scripts/porta.luau")).unwrap();
        let found = verify(&data, VERIFY_SECONDS);
        assert!(!found.ok);
        assert_eq!(
            found.errors,
            ["Porta: o script scripts/porta.luau não existe na pasta do projeto"]
        );
        // A script edited in the game's folder is checked as the export checks it.
        fs::write(data.join("scripts/dados/textos.luau"), "return {\n").unwrap();
        let found = verify(&data, VERIFY_SECONDS);
        assert_eq!(found.errors.len(), 2, "{found:?}");
        assert!(
            found.errors[1].starts_with("scripts/dados/textos.luau:")
                && found.errors[1].contains("erro de sintaxe"),
            "{found:?}"
        );
    }

    #[test]
    fn a_missing_or_broken_script_the_game_uses_stops_the_export() {
        let mut fixture = Fixture::new("scripts-recusados");
        scripted(&mut fixture);
        let painted = HashSet::from([fixture.painted.clone()]);
        let found = check(&fixture.project, Some(&fixture.folder), &painted);
        assert!(found.errors.is_empty(), "{found:?}");
        fs::remove_file(fixture.folder.join("scripts/porta.luau")).unwrap();
        fs::write(fixture.folder.join("scripts/quebrado.luau"), "return {\n").unwrap();
        let found = check(&fixture.project, Some(&fixture.folder), &painted);
        assert_eq!(
            found.errors,
            [
                "Porta: o script scripts/porta.luau não existe na pasta do projeto; restaure o \
              arquivo ou remova o script do objeto"
            ]
        );
        // A broken script nothing uses only warns, and stays out of the game.
        let broken = found
            .warnings
            .iter()
            .find(|w| w.starts_with("scripts/quebrado.luau:"))
            .unwrap_or_else(|| panic!("{found:?}"));
        assert!(
            broken.contains("erro de sintaxe")
                && broken.ends_with("nenhum objeto usa este script, e ele fica fora do jogo"),
            "{broken}"
        );
        let destination = fixture.base.join("Jogos");
        let error = export_with(&fixture.request(&destination, false), checked).unwrap_err();
        assert!(
            matches!(error, ExportError::Invalid(_)) && error.to_string().contains("Porta"),
            "{error}"
        );
        assert!(!destination.exists());
        // A module the attached script requires is used: broken, it stops the export.
        fs::write(
            fixture.folder.join("scripts/porta.luau"),
            "local Textos = require('./dados/textos')\nreturn {}\n",
        )
        .unwrap();
        fs::write(
            fixture.folder.join("scripts/dados/textos.luau"),
            "return {\n",
        )
        .unwrap();
        let found = check(&fixture.project, Some(&fixture.folder), &painted);
        assert_eq!(found.errors.len(), 1, "{found:?}");
        assert!(
            found.errors[0].starts_with("scripts/dados/textos.luau:2: erro de sintaxe"),
            "{found:?}"
        );
        // A module that cannot be read, too; one nothing uses only warns.
        fs::write(
            fixture.folder.join("scripts/dados/textos.luau"),
            b"return { '\xe1' }\n",
        )
        .unwrap();
        fs::write(
            fixture.folder.join("scripts/latin1.luau"),
            b"-- \xe1rvore\n",
        )
        .unwrap();
        let found = check(&fixture.project, Some(&fixture.folder), &painted);
        assert_eq!(
            found.errors,
            ["O script scripts/dados/textos.luau não é texto UTF-8"]
        );
        assert!(
            found.warnings.contains(
                &"O script scripts/latin1.luau não é texto UTF-8; nenhum objeto usa este script, \
                  e ele fica fora do jogo"
                    .to_owned()
            ),
            "{found:?}"
        );
        // A project never saved has no folder for its scripts.
        let found = check(&fixture.project, None, &painted);
        assert!(
            found
                .errors
                .iter()
                .any(|error| error.contains("onde ficam os seus scripts")),
            "{found:?}"
        );
    }

    #[test]
    fn a_game_mirrors_the_projects_tree_of_scripts() {
        let mut fixture = Fixture::new("arvore");
        let mut door = Entity::new("Porta", None);
        door.scripts = vec![crate::script::ScriptRef::new(
            "Pista/Checkpoints/porta.luau",
        )];
        fixture.project.scenes[0].entities.push(door);
        let write = |path: &str, text: &str| {
            let file = fixture.folder.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        };
        write(
            "Pista/Checkpoints/porta.luau",
            "local Dados = require('../dados')\nlocal Sons = require('@project/Comum/sons')\n\
             local M = {}\nfunction M.on_start(self) print(Dados.nome .. ' ' .. Sons.pulo) \
             end\nreturn M\n",
        );
        write("Pista/dados.luau", "return { nome = 'pista' }\n");
        write("Comum/sons.luau", "return { pulo = 'Pulo' }\n");
        // Attached by nothing: it goes all the same, a require may build its name.
        write("Menu/tela.luau", "return {}\n");
        write("Rascunhos/quebrado.luau", "return {\n");
        write("Pista/notas.txt", "fica de fora");
        write(".solucoes/porta.luau", "return {}\n");
        write(".oxy/agente/Pista/dados.luau.bak", "return {}\n");
        // An earlier export of the game inside the project, and another project.
        write("Jogo antigo/data/export.oxy.json", "{}");
        write("Jogo antigo/data/Pista/dados.luau", "return {}\n");
        write("Outro/project.oxy.json", "{}");
        write("Outro/x.luau", "return {}\n");
        persistence::save_project(&fixture.folder.join(PROJECT_FILE), &fixture.project).unwrap();
        let painted = HashSet::from([fixture.painted.clone()]);
        let found = check(&fixture.project, Some(&fixture.folder), &painted);
        assert!(found.errors.is_empty(), "{found:?}");
        let destination = fixture.base.join("Jogos");
        let report = export_with(&fixture.request(&destination, false), checked).unwrap();
        let data = report.folder.join("data");
        let scripts: Vec<String> = files(&data)
            .into_iter()
            .filter(|file| file.ends_with(".luau"))
            .collect();
        assert_eq!(
            scripts,
            [
                "Comum/sons.luau",
                "Menu/tela.luau",
                "Pista/Checkpoints/porta.luau",
                "Pista/dados.luau"
            ]
        );
        assert!(report.verify.ok, "{:?}", report.verify);
        assert!(
            report
                .verify
                .logs
                .iter()
                .any(|line| line.ends_with("Porta · pista Pulo")),
            "{:?}",
            report.verify.logs
        );
        // The game reads its tree as the project does: the player's sources are these.
        let sources = persistence::load_scripts(&data).unwrap();
        assert_eq!(sources.files.keys().cloned().collect::<Vec<_>>(), scripts);
        assert!(sources.unreadable.is_empty());
    }

    #[test]
    fn every_license_text_of_the_engine_is_exported() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../licenses");
        let mut shipped: Vec<(String, Vec<u8>)> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, fs::read(path).unwrap())
            })
            .collect();
        shipped.push((
            "OXY-Engine-MIT.txt".into(),
            fs::read(folder.join("../LICENSE")).unwrap(),
        ));
        shipped.sort();
        let mut embedded: Vec<(String, Vec<u8>)> = LICENSES
            .iter()
            .map(|(name, text)| ((*name).to_owned(), text.as_bytes().to_vec()))
            .collect();
        embedded.sort();
        assert_eq!(
            embedded.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            shipped.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            "a license in licenses/ is missing from export::LICENSES"
        );
        assert_eq!(embedded, shipped);
    }

    #[test]
    fn the_player_is_found_next_to_the_engine_or_one_folder_up() {
        let base = std::env::temp_dir().join(format!("oxy-export-player-{}", new_id()));
        let deps = base.join("deps");
        fs::create_dir_all(&deps).unwrap();
        let editor = base.join("OXY Engine.exe");
        assert_eq!(player_next_to(&editor), None);
        fs::write(base.join(PLAYER_EXE), b"player").unwrap();
        assert_eq!(player_next_to(&editor), Some(base.join(PLAYER_EXE)));
        assert_eq!(
            player_next_to(&deps.join("teste.exe")),
            Some(base.join(PLAYER_EXE))
        );
        fs::remove_dir_all(base).unwrap();
        assert!(engine_version().starts_with(|c: char| c.is_ascii_digit()));
        assert!(!engine_version().contains('+'));
    }

    /// The README says which version this is and names its ZIP, and the CHANGELOG has its
    /// section, so a version bump carries them along.
    #[test]
    fn the_readme_and_the_changelog_name_this_version() {
        let version = engine_version();
        let readme = include_str!("../../../README.md");
        assert!(readme.contains(&format!("**Versão atual: {version}.**")));
        assert!(readme.contains(&format!("OXY-Engine-{version}-windows-x64.zip")));
        let changelog = include_str!("../../../CHANGELOG.md");
        assert!(changelog.contains(&format!("\n## {version} — ")));
    }
}
