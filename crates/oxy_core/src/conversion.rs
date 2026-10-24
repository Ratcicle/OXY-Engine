//! Organizing a project of the old layout in folders: formats 1 to 5 and the development format
//! 6, with images and sounds as `assets/<id>` and models inside the project file. Each image
//! goes to `Imagens/`, each sound to `Sons/` and each model to its own file in `Modelos/`, named
//! as the Library shows it (fitted to what Windows keeps, and numbered while another of its kind
//! has the name: "Música 2"); the scripts stay where they are and no ID changes.
//!
//! In place ([`convert`], the editor's first save and an agent's first write), a full copy of
//! the project as it was goes first to `.oxy/backup/antes-das-pastas-<date>/`, which still opens
//! in the OXY that made it, and a journal (`.oxy/conversao.json`) written before anything else
//! lets the next opening finish or undo a conversion that stopped at any point ([`resume`]):
//! while the project file is still the old one, the new names go and the project stays as it
//! was; once it is the new one, the old names go. To another folder ([`save_copy`]), the project
//! is written there in folders at once. Nothing is ever written over: the new names are only
//! created (a second name for the same file, or a copy), a file already there with the same
//! bytes is taken as it is, and an old name goes only while it holds what the copy of the
//! project holds.
use crate::{
    document::{Asset, AssetKind, Id, Layout, Project, validate_project},
    fs_ops, model_file, names,
    persistence::{
        self, Expected, FileWrite, KnownFiles, PROJECT_FILE, PersistenceError, WriteMode,
    },
    project_lock::{self, ConversionLock},
    project_tree::{self, FileKind},
    texture_cache::TextureCache,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[cfg(test)]
mod tests;

/// The journal of a conversion in place, from the project's folder.
pub const JOURNAL: &str = ".oxy/conversao.json";
/// Where the copy of the project as it was goes, from the project's folder, followed by the date
/// (`.oxy/backup/antes-das-pastas-20261023-213000`, numbered when taken).
pub const BACKUP: &str = ".oxy/backup/antes-das-pastas";
/// What an agent's conversion leaves for the editor to tell when it next opens the project,
/// from the project's folder; the editor's next save takes it away.
pub const AGENT_NOTE: &str = ".oxy/conversao-agente.json";
/// The folder images and sounds had in the old layout.
const OLD_FOLDER: &str = "assets";

/// Who converts, which the conversion's messages say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    /// The person, saving in the editor.
    Person,
    /// An agent, writing the project's files.
    Agent,
}

/// Where the file of an image or a sound goes, both paths from the project's folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
    /// A file with the bytes of `from` was at `to` already: it is taken as it is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reused: bool,
}

/// A model's file a conversion writes, or finds there with the bytes it would write.
#[derive(Clone, Debug)]
struct ModelFile {
    id: Id,
    path: String,
    bytes: Arc<[u8]>,
    reused: bool,
}

/// What converting a project does, planned before anything is written.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The project in the tree layout: each image, sound and model with its new name and path.
    pub project: Project,
    /// The images' and sounds' files that move, each from its old path to its new one.
    pub moved: Vec<Moved>,
    models: Vec<ModelFile>,
    /// The files of `assets/` named like an asset's ID that the project does not name, which
    /// the 0.6 left behind: they go to the copy of the project only.
    pub orphans: Vec<String>,
    /// The scripts of the project's tree, which stay where they are (and go to the copy).
    pub scripts: Vec<String>,
    /// What to tell about the names: fitted, numbered, files that were not there.
    pub notes: Vec<String>,
    /// How many images, sounds and models went to their folders.
    counts: [usize; 3],
}

impl Plan {
    /// The paths of the models' files, as the project has them.
    pub fn model_paths(&self) -> Vec<String> {
        self.models.iter().map(|model| model.path.clone()).collect()
    }

    /// What went where, as the summary says it: "9 sons em Sons/, 2 modelos em Modelos/".
    fn parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        for (kind, count) in [
            (AssetKind::Texture, self.counts[0]),
            (AssetKind::Audio, self.counts[1]),
            (AssetKind::Model, self.counts[2]),
        ] {
            if count == 0 {
                continue;
            }
            let (folder, _, _) = place(kind);
            let what = match (kind, count) {
                (AssetKind::Texture, 1) => "imagem",
                (AssetKind::Texture, _) => "imagens",
                (AssetKind::Audio, 1) => "som",
                (AssetKind::Audio, _) => "sons",
                (AssetKind::Model, 1) => "modelo",
                (AssetKind::Model, _) => "modelos",
            };
            parts.push(format!("{count} {what} em {folder}/"));
        }
        if !self.scripts.is_empty() {
            parts.push("os scripts ficam onde estão".into());
        }
        parts
    }

    /// Whether the conversion changes more of the document than its layout mark, or takes files
    /// away: the editor's undo history, made of the old paths, starts over then.
    fn changes_assets(&self, project: &Project) -> bool {
        !self.orphans.is_empty()
            || project
                .assets
                .iter()
                .zip(&self.project.assets)
                .any(|(old, new)| old.path != new.path || old.name != new.name)
    }
}

/// What a conversion did.
#[derive(Clone, Debug)]
pub struct Converted {
    /// The project as saved, in the tree layout.
    pub project: Project,
    /// One line on what changed; none when no project file was there yet (a project saved for
    /// the first time is simply saved in folders).
    pub summary: Option<String>,
    /// The names fitted or numbered and the files that were not there, one line each.
    pub notes: Vec<String>,
    /// The images' and sounds' files that changed name.
    pub moved: Vec<Moved>,
    /// The models' files, from the project's folder.
    pub models: Vec<String>,
    /// Where the copy of the project as it was went, from the project's folder.
    pub backup: Option<String>,
    /// What the save tells besides: what became of a conversion that had stopped, an old name
    /// that stayed.
    pub warnings: Vec<String>,
    /// Whether the document changed beyond its layout mark (see [`Plan`]).
    pub changed_assets: bool,
}

impl Converted {
    /// Every line to tell: the summary, the notes and the warnings.
    pub fn lines(&self) -> impl Iterator<Item = &String> {
        self.summary.iter().chain(&self.notes).chain(&self.warnings)
    }

    /// What an agent's answer says of it.
    pub fn json(&self) -> Json {
        let mut answer = json!({
            "summary": self.summary,
            "moved": self.moved.iter().map(|m| json!({"from": m.from, "to": m.to})).collect::<Vec<_>>(),
            "models": self.models,
            "backup": self.backup,
        });
        if !self.notes.is_empty() {
            answer["notes"] = json!(self.notes);
        }
        answer
    }
}

/// Why a conversion in place did not happen.
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    /// Nothing was written: why the project cannot be organized in folders where it is. Saving
    /// it in the old layout still works.
    #[error("{0}")]
    Refused(String),
    /// It stopped and undid what it had done: nothing changed.
    #[error("{0}")]
    Failed(String),
    /// It stopped, and undoing failed too: its journal stays, and the next opening of the
    /// project finishes or undoes it. Nothing else should write the project meanwhile.
    #[error("{0}")]
    Kept(String),
    /// Nothing was written: another conversion of the project is at work, and saving it now in
    /// either layout could go over what that one writes. Saving again once it ends works.
    #[error("{0}")]
    Busy(String),
}

impl ConversionError {
    /// This error, told after `lines`.
    fn after(self, lines: &[String]) -> Self {
        let told = |why: String| format!("{}; {why}", lines.join("; "));
        match self {
            Self::Refused(why) => Self::Refused(told(why)),
            Self::Failed(why) => Self::Failed(told(why)),
            Self::Kept(why) => Self::Kept(told(why)),
            Self::Busy(why) => Self::Busy(told(why)),
        }
    }
}

/// A point of a conversion in place, where tests stop it as a crash would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The journal is written.
    Journal,
    /// The nth file is about to be copied to the backup (the project file first).
    Backup(usize),
    /// The nth new name is about to be made.
    Name(usize),
    /// The nth file of the project file's transaction (the project file last) is about to be
    /// put in place.
    Install(usize),
    /// The nth old name is about to go.
    Clean(usize),
}

/// How far a conversion in place went, as its journal says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Backup,
    Nomes,
    Manifesto,
    Limpeza,
}

/// A file a conversion creates, which undoing takes away while it still holds what was
/// written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Created {
    path: String,
    hash: String,
}

/// What a conversion in place writes before anything else, enough to finish or undo it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Journal {
    phase: Phase,
    /// The project file's hash before and after (see [`hash`]).
    source: String,
    target: String,
    /// The copy of the project, and the files copied there, from it.
    backup: String,
    copied: Vec<String>,
    moved: Vec<Moved>,
    created: Vec<Created>,
    orphans: Vec<String>,
    /// The folders the conversion made, which undoing removes when they are empty.
    folders: Vec<String>,
}

/// The folder, the extension and the fallback name of the files of `kind` in a converted
/// project.
fn place(kind: AssetKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        AssetKind::Texture => ("Imagens", ".png", "Imagem"),
        AssetKind::Audio => ("Sons", ".wav", "Som"),
        AssetKind::Model => ("Modelos", model_file::EXTENSION, "Modelo"),
    }
}

/// `name` of `kind` as messages name it: `Pulo (som)`.
fn label(kind: AssetKind, name: &str) -> String {
    let kind = match kind {
        AssetKind::Texture => "imagem",
        AssetKind::Audio => "som",
        AssetKind::Model => "modelo",
    };
    format!("{name} ({kind})")
}

/// The project file `path` names: itself, or the one in the folder `path`.
fn project_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    }
}

/// Why the project file `file` cannot be organized in folders by its name, if it cannot: the
/// journal, the copy of the project and the next opening know it as [`PROJECT_FILE`].
fn other_name(file: &Path) -> Option<String> {
    (file.file_name() != Some(PROJECT_FILE.as_ref())).then(|| {
        format!(
            "a OXY só organiza em pastas um projeto cujo arquivo se chama {PROJECT_FILE}; este se \
             chama {}",
            file.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    })
}

/// The hash a journal keeps of a file's bytes.
fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", crate::random::hash(&[b"oxy-conversao", bytes]))
}

/// The `n`th name `base` gives (`base`, `base 2`, …), within [`names::MAX_NAME_CHARS`]
/// characters and `max_bytes` bytes.
fn nth_name(base: &str, n: usize, max_bytes: usize) -> String {
    let number = if n == 1 {
        String::new()
    } else {
        format!(" {n}")
    };
    let mut kept: String = base
        .chars()
        .take(names::MAX_NAME_CHARS - number.chars().count())
        .collect();
    while !kept.is_empty() && kept.len() + number.len() > max_bytes {
        kept.pop();
    }
    format!("{}{number}", kept.trim_end_matches(['.', ' ']))
}

/// What is at a new name.
enum Target {
    Free,
    /// A file with the bytes that would go there.
    Same,
    Taken,
}

/// Whether the file `file` holds `bytes`.
fn holds(file: &Path, bytes: &[u8]) -> io::Result<bool> {
    let meta = fs_ops::retrying(|| fs::metadata(file))?;
    Ok(meta.is_file()
        && meta.len() == bytes.len() as u64
        && fs_ops::retrying(|| fs::read(file))? == bytes)
}

/// Whether the files `a` and `b` hold the same bytes, read a piece at a time; none when one of
/// them is not there.
fn same_files(a: &Path, b: &Path) -> io::Result<Option<bool>> {
    let open = |file: &Path| match fs_ops::retrying(|| File::open(file)) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    let (Some(mut a), Some(mut b)) = (open(a)?, open(b)?) else {
        return Ok(None);
    };
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(Some(false));
    }
    let (mut first, mut second) = (vec![0_u8; 65536], vec![0_u8; 65536]);
    loop {
        let count = a.read(&mut first)?;
        if count == 0 {
            return Ok(Some(true));
        }
        if b.read_exact(&mut second[..count]).is_err() || first[..count] != second[..count] {
            return Ok(Some(false));
        }
    }
}

/// Names and places the assets of `project` in the tree layout. With the project's folder
/// `root`, each image's and sound's file is looked for (one not there is planned at its new
/// path, to be put back there, unless `painted` has its pixels, which are written there) and a
/// new name a file has is taken as it is when it holds the same bytes and skipped when it holds
/// others; without it, nothing is looked at (a copy, whose writes only create).
fn assign(project: &Project, root: Option<&Path>, painted: &HashSet<Id>) -> Result<Plan, String> {
    let mut plan = Plan {
        project: project.clone(),
        moved: Vec::new(),
        models: Vec::new(),
        orphans: Vec::new(),
        scripts: Vec::new(),
        notes: Vec::new(),
        counts: [0; 3],
    };
    plan.project.layout = Layout::Tree;
    let mut taken: Vec<(AssetKind, String)> = Vec::new();
    // The old names go once the project is in folders: no new name may be one of them.
    let old: Vec<&str> = project
        .assets
        .iter()
        .filter(|asset| asset.kind != AssetKind::Model)
        .map(|asset| asset.path.as_str())
        .collect();
    for (index, asset) in project.assets.iter().enumerate() {
        let (folder, extension, fallback) = place(asset.kind);
        let model = match (asset.kind, &asset.model) {
            (AssetKind::Model, Some(pieces)) => Some(Arc::<[u8]>::from(
                model_file::to_bytes(&asset.id, pieces).map_err(|e| e.to_string())?,
            )),
            // The old layout keeps every model's pieces in the project file.
            (AssetKind::Model, None) => continue,
            _ => None,
        };
        let source = match (&model, root) {
            (None, Some(root)) => source_file(root, asset)?,
            _ => None,
        };
        let base = names::fitted_name(&asset.name, fallback);
        let max_bytes = names::MAX_PATH_BYTES - folder.len() - 1 - extension.len();
        let mut chosen = None;
        for n in 1.. {
            let name = nth_name(&base, n, max_bytes);
            if taken
                .iter()
                .any(|(kind, other)| *kind == asset.kind && names::same_name(other, &name))
            {
                continue;
            }
            let path = format!("{folder}/{name}{extension}");
            // Its own file at the new name in another case: the same file, which stays.
            if source.is_some() && names::same_name(&asset.path, &path) {
                chosen = Some((name, asset.path.clone(), true));
                break;
            }
            if old.iter().any(|old| names::same_name(old, &path)) {
                continue;
            }
            let target = match root {
                None => Target::Free,
                Some(root) => target(root, &path, model.as_deref(), source.as_deref())?,
            };
            match target {
                Target::Taken => continue,
                Target::Free => chosen = Some((name, path, false)),
                Target::Same => chosen = Some((name, path, true)),
            }
            break;
        }
        let (name, path, reused) = chosen.expect("a free name");
        taken.push((asset.kind, name.clone()));
        let kind_index = match asset.kind {
            AssetKind::Texture => 0,
            AssetKind::Audio => 1,
            AssetKind::Model => 2,
        };
        if let Some(bytes) = model {
            plan.models.push(ModelFile {
                id: asset.id.clone(),
                path: path.clone(),
                bytes,
                reused,
            });
            plan.counts[kind_index] += 1;
        } else if root.is_none() || source.is_some() {
            plan.moved.push(Moved {
                from: asset.path.clone(),
                to: path.clone(),
                reused,
            });
            plan.counts[kind_index] += 1;
        } else if painted.contains(&asset.id) {
            // Pixels not saved yet, written at the new name by the save.
            plan.counts[kind_index] += 1;
        } else {
            plan.notes.push(format!(
                "{}: o arquivo dele não estava na pasta do projeto; agora ele é procurado em \
                 {path}",
                label(asset.kind, &asset.name)
            ));
        }
        if name != asset.name.trim() {
            plan.notes.push(renamed(asset, &base, &name));
        }
        let converted = &mut plan.project.assets[index];
        converted.name = name;
        converted.path = path;
    }
    Ok(plan)
}

/// The note on an asset whose name the conversion changed from `asset.name` to `name`, `base`
/// being the name fitted to what Windows keeps.
fn renamed(asset: &Asset, base: &str, name: &str) -> String {
    let old = asset.name.trim();
    let why = if base != old {
        "um nome que o Windows aceita em arquivos"
    } else {
        "porque outro do mesmo tipo já tinha esse nome"
    };
    format!(
        "{} passou a se chamar \"{name}\", {why}; quem o pede pelo nome antigo precisa do novo",
        label(asset.kind, old)
    )
}

/// The file of the image or sound `asset` in the project at `root`: none when it is not there
/// (or its path names no file of the project). One that cannot be read now refuses the
/// conversion, which needs it.
fn source_file(root: &Path, asset: &Asset) -> Result<Option<PathBuf>, String> {
    let Ok(file) = persistence::resolve_asset_path(root, &asset.path) else {
        return Ok(None);
    };
    match fs_ops::retrying(|| fs::metadata(&file)) {
        Ok(meta) if meta.is_file() => Ok(Some(file)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "o arquivo de {} {}",
            label(asset.kind, &asset.name),
            project_tree::now_unreadable(&error)
        )),
    }
}

/// What the new name `path` of the project at `root` holds, for a model's file of `model`
/// bytes or a copy of the file `source`.
fn target(
    root: &Path,
    path: &str,
    model: Option<&[u8]>,
    source: Option<&Path>,
) -> Result<Target, String> {
    let file = persistence::resolve_asset_path(root, path).map_err(|e| e.to_string())?;
    match fs::symlink_metadata(&file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Target::Free),
        Err(error) => return Err(format!("{path} {}", project_tree::now_unreadable(&error))),
        Ok(meta) if !meta.is_file() => return Ok(Target::Taken),
        Ok(_) => {}
    }
    let same = match (model, source) {
        (Some(bytes), _) => holds(&file, bytes),
        (None, Some(source)) => same_files(&file, source).map(|same| same == Some(true)),
        (None, None) => Ok(false),
    }
    .map_err(|error| format!("{path} {}", project_tree::now_unreadable(&error)))?;
    Ok(if same { Target::Same } else { Target::Taken })
}

/// Whether `path` (from the project's folder) is a file the old layout named after an asset's
/// ID, `assets/<id>.png` or `.wav`, which a project in folders never takes as an asset of its
/// own.
pub fn old_asset_file(path: &str) -> bool {
    let Some((folder, name)) = path.split_once('/') else {
        return false;
    };
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    folder.eq_ignore_ascii_case(OLD_FOLDER)
        && (extension.eq_ignore_ascii_case("png") || extension.eq_ignore_ascii_case("wav"))
        && uuid::Uuid::parse_str(stem).is_ok()
}

/// Plans organizing `project`, saved in the folder `root` in the old layout, in folders where
/// it is (see [`assign`]; `painted` are the images whose pixels the save writes). Its tree must
/// be read whole and hold no image, sound or model the project does not name, other than the
/// files of `assets/` named like an asset's ID (orphans, which go to the copy only): a project
/// saved in a folder with other files would take them all once in folders. Why not, when it
/// cannot be organized there.
pub fn plan(root: &Path, project: &Project, painted: &HashSet<Id>) -> Result<Plan, String> {
    let tree = project_tree::walk(root).map_err(|error| error.to_string())?;
    if let Some((folder, why)) = tree.unreadable.iter().next() {
        let folder = if folder.is_empty() {
            "a pasta do projeto".to_owned()
        } else {
            format!("a pasta {folder}")
        };
        return Err(format!(
            "{folder} {why}; feche o programa que a usa e salve de novo"
        ));
    }
    let mut plan = assign(project, Some(root), painted)?;
    let named = |path: &str| {
        plan.moved.iter().any(|moved| {
            names::same_name(&moved.from, path)
                || (moved.reused && names::same_name(&moved.to, path))
        }) || plan
            .models
            .iter()
            .any(|model| model.reused && names::same_name(&model.path, path))
    };
    let mut orphans = Vec::new();
    let mut others = Vec::new();
    for file in &tree.files {
        if !matches!(
            file.kind,
            FileKind::Image | FileKind::Sound | FileKind::Model
        ) || named(&file.path)
        {
            continue;
        }
        if old_asset_file(&file.path) {
            orphans.push(file.path.clone());
        } else {
            others.push(file.path.clone());
        }
    }
    if !others.is_empty() {
        let shown: Vec<&str> = others.iter().take(3).map(String::as_str).collect();
        let more = match others.len() - shown.len() {
            0 => String::new(),
            n => format!(" e mais {n}"),
        };
        return Err(format!(
            "a pasta do projeto tem {} que ele não usa ({}{more}); a OXY só organiza em pastas um \
             projeto que está numa pasta só dele: mova esses arquivos para fora dela, ou o \
             projeto para uma pasta nova",
            match others.len() {
                1 => "1 imagem, som ou modelo".to_owned(),
                n => format!("{n} imagens, sons ou modelos"),
            },
            shown.join(", ")
        ));
    }
    plan.orphans = orphans;
    plan.scripts = tree.scripts().map(|file| file.path.clone()).collect();
    Ok(plan)
}

/// What the editor tells when it opens `project`, saved at `path` (its folder or its project
/// file) in the old layout: what its first save will do, or why it will stay in the old layout.
pub fn preview(path: &Path, project: &Project) -> String {
    let file = project_file(path);
    let root = file.parent().unwrap_or(Path::new("."));
    let planned = match other_name(&file) {
        Some(why) => Err(why),
        None => plan(root, project, &HashSet::new()),
    };
    match planned {
        Ok(plan) => {
            let parts = plan.parts();
            let what = if parts.is_empty() {
                String::new()
            } else {
                format!(" ({})", parts.join(", "))
            };
            format!(
                "Este projeto está no formato antigo da OXY. Ao salvar, a OXY o organiza em \
                 pastas{what}, cada arquivo com o nome que aparece na Biblioteca, e antes guarda \
                 uma cópia completa dele em .oxy/backup/. Os objetos, os scripts e os nós \
                 continuam funcionando."
            )
        }
        Err(why) => {
            format!("Este projeto está no formato antigo da OXY e continua nele ao salvar: {why}.")
        }
    }
}

/// The project file of `project` in the tree layout: without the models' pieces, which go in
/// their own files.
fn manifest(project: &Project) -> Result<Vec<u8>, serde_json::Error> {
    let mut manifest = project.clone();
    for asset in &mut manifest.assets {
        if asset.kind == AssetKind::Model {
            asset.model = None;
        }
    }
    serde_json::to_vec_pretty(&manifest)
}

/// The first of `BACKUP-<date>`, `BACKUP-<date>-2`… that the project at `root` lacks, `date`
/// as `AAAAMMDD-HHMMSS` in UTC.
fn free_backup(root: &Path, now: SystemTime) -> String {
    let date: String = fs_ops::utc(now)
        .chars()
        .filter_map(|c| match c {
            '0'..='9' => Some(c),
            'T' => Some('-'),
            _ => None,
        })
        .collect();
    (1..)
        .map(|n: usize| {
            if n == 1 {
                format!("{BACKUP}-{date}")
            } else {
                format!("{BACKUP}-{date}-{n}")
            }
        })
        .find(|backup| fs::symlink_metadata(root.join(backup)).is_err())
        .expect("a free name")
}

/// The file `relative` of the project at `root`, as a conversion's error.
fn file_of(root: &Path, relative: &str) -> Result<PathBuf, String> {
    persistence::resolve_asset_path(root, relative).map_err(|e| e.to_string())
}

/// Writes `journal` as the journal of the project at `root`: created when `new`, else over the
/// one there (OXY's own file).
fn write_journal(root: &Path, journal: &Journal, new: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
    let file = file_of(root, JOURNAL)?;
    let write = if new {
        FileWrite::create(file, bytes)
    } else {
        FileWrite::replace(file, bytes)
    };
    persistence::transaction_write(vec![write]).map_err(|e| e.to_string())
}

/// Organizes `project`, saved at `path` (its folder or its project file) in the old layout, in
/// folders where it is, as its save: `project` is what is saved, the person's changes since the
/// last save included, with the pixels painted that `images` holds. Every image's and sound's
/// file gets its new name, every model its file, the project file becomes the tree layout's and
/// the old names go; a full copy of the project as it was goes first to `BACKUP-<date>` (the
/// project file as it was, every file at its old path, orphans and scripts included). A project
/// never saved (no project file yet) is saved in the tree layout at once. `known` gets the
/// models' files as written, and `images` the new paths. An agent's conversion (`who`) leaves a
/// note the editor tells when it opens the project ([`agent_note`]).
pub fn convert(
    path: &Path,
    project: &Project,
    images: Option<&mut TextureCache>,
    known: &mut KnownFiles,
    who: Who,
) -> Result<Converted, ConversionError> {
    convert_with(path, project, images, known, who, &mut |_| {})
}

pub(crate) fn convert_with(
    path: &Path,
    project: &Project,
    images: Option<&mut TextureCache>,
    known: &mut KnownFiles,
    who: Who,
    hook: &mut dyn FnMut(Step),
) -> Result<Converted, ConversionError> {
    use ConversionError::{Busy, Failed, Kept, Refused};
    let file = project_file(path);
    let root = file.parent().unwrap_or(Path::new(".")).to_owned();
    if project.layout == Layout::Tree {
        return Err(Failed("o projeto já está organizado em pastas".into()));
    }
    if let Some(why) = other_name(&file) {
        return Err(Refused(why));
    }
    validate_project(project).map_err(Failed)?;
    let mut painted = HashSet::new();
    if let Some(images) = images.as_deref() {
        for (id, _) in images.dirty_images() {
            if !project
                .asset(id)
                .is_some_and(|asset| asset.kind == AssetKind::Texture)
            {
                return Err(Failed(
                    PersistenceError::DirtyTextureWithoutAsset(id.clone()).to_string(),
                ));
            }
            painted.insert(id.clone());
        }
    }
    match fs::symlink_metadata(&file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return first_save(&file, project, images, known, &painted);
        }
        Err(error) => {
            return Err(Failed(format!(
                "{PROJECT_FILE} {}",
                project_tree::now_unreadable(&error)
            )));
        }
        Ok(_) => {}
    }
    let _lock = ConversionLock::hold(&root).map_err(|error| {
        if fs_ops::busy(&error) {
            Busy(
                "outra organização em pastas deste projeto está em andamento; salve de novo \
                 quando ela terminar"
                    .into(),
            )
        } else {
            Failed(format!(
                "não foi possível reservar o projeto para organizá-lo: {error}"
            ))
        }
    })?;
    // A conversion that stopped while the project was open: finished or undone first, and told.
    let mut resumed = Vec::new();
    if root.join(JOURNAL).exists() {
        let told = resume_locked(&root);
        if root.join(JOURNAL).exists() {
            return Err(Kept(told.warnings.join("; ")));
        }
        resumed.extend(told.fixes);
        resumed.extend(told.warnings);
    }
    match organize(&file, project, images, known, who, &painted, hook) {
        Ok(mut converted) => {
            converted.warnings.splice(0..0, resumed);
            Ok(converted)
        }
        Err(error) if resumed.is_empty() => Err(error),
        Err(error) => Err(error.after(&resumed)),
    }
}

/// [`convert_with`] of the project file `file`, held for the conversion and without a journal
/// left behind.
fn organize(
    file: &Path,
    project: &Project,
    images: Option<&mut TextureCache>,
    known: &mut KnownFiles,
    who: Who,
    painted: &HashSet<Id>,
    hook: &mut dyn FnMut(Step),
) -> Result<Converted, ConversionError> {
    use ConversionError::{Failed, Kept, Refused};
    let root = file.parent().unwrap_or(Path::new(".")).to_owned();
    let source = fs_ops::retrying(|| fs::read(file)).map_err(|error| Failed(error.to_string()))?;
    let source_json = serde_json::from_slice::<Json>(&source).ok();
    // Organized meanwhile (a conversion that stopped and was finished just now, another OXY):
    // saving the old layout over it would lose its paths.
    if source_json
        .as_ref()
        .is_some_and(|value| value["layout"] == "Tree")
    {
        return Err(Kept(
            "o projeto já foi organizado em pastas no disco por outra gravação; reabra-o para \
             continuar"
                .into(),
        ));
    }
    let source_format = source_json.and_then(|value| value["schema_version"].as_u64());
    let plan = plan(&root, project, painted).map_err(Refused)?;
    validate_project(&plan.project).map_err(Failed)?;
    let manifest = manifest(&plan.project).map_err(|e| Failed(e.to_string()))?;
    let moved: Vec<Moved> = plan
        .moved
        .iter()
        .filter(|moved| moved.from != moved.to)
        .cloned()
        .collect();
    // What goes in the project file's transaction besides it, planned now so the journal names
    // every file the conversion creates.
    let mut writes = Vec::new();
    let mut created = Vec::new();
    let file_at = |relative: &str| file_of(&root, relative).map_err(Failed);
    for model in plan.models.iter().filter(|model| !model.reused) {
        writes.push(FileWrite::create(
            file_at(&model.path)?,
            model.bytes.to_vec(),
        ));
        created.push(Created {
            path: model.path.clone(),
            hash: hash(&model.bytes),
        });
    }
    if let Some(images) = images.as_deref() {
        for (id, image) in images.dirty_images() {
            let asset = plan.project.asset(id).expect("checked above");
            let bytes = image.to_png().map_err(Failed)?;
            // Undoing takes away what this writes, but not the image's own file, already at its
            // new path: it stays as painted, and the copy of the project keeps it as it was.
            if !plan.moved.iter().any(|moved| moved.from == asset.path) {
                created.push(Created {
                    path: asset.path.clone(),
                    hash: hash(&bytes),
                });
            }
            let target = file_at(&asset.path)?;
            // Over the new name the conversion gives the old file (or one with its bytes),
            // while it still holds them; else only created.
            let write = match plan.moved.iter().find(|moved| moved.to == asset.path) {
                Some(moved) => {
                    let from = file_at(&moved.from)?;
                    let old = fs_ops::retrying(|| fs::read(&from))
                        .map_err(|error| Failed(error.to_string()))?;
                    FileWrite {
                        target,
                        bytes,
                        mode: WriteMode::Replace {
                            expected: Some(Expected::Bytes(old.into())),
                        },
                    }
                }
                None => FileWrite::create(target, bytes),
            };
            writes.push(write);
        }
    }
    let parts = plan.parts();
    let backup = free_backup(&root, SystemTime::now());
    let summary = summary(who, &parts, &backup, source_format);
    if who == Who::Agent {
        let lines: Vec<&String> = std::iter::once(&summary).chain(&plan.notes).collect();
        let note = serde_json::to_vec_pretty(&json!({
            "date": fs_ops::utc(SystemTime::now()),
            "lines": lines,
        }))
        .map_err(|e| Failed(e.to_string()))?;
        created.push(Created {
            path: AGENT_NOTE.into(),
            hash: hash(&note),
        });
        writes.push(FileWrite::replace(file_at(AGENT_NOTE)?, note));
    }
    // No backup of the format beside the project file: the copy of the project keeps the file
    // as it was, byte for byte.
    writes.push(FileWrite {
        target: file.to_owned(),
        bytes: manifest.clone(),
        mode: WriteMode::Replace {
            expected: Some(Expected::Bytes(source.as_slice().into())),
        },
    });
    // Every file the project names goes to the copy, one already at its new path included.
    let mut copied: Vec<String> = Vec::new();
    for path in plan
        .moved
        .iter()
        .map(|moved| &moved.from)
        .chain(&plan.orphans)
        .chain(&plan.scripts)
    {
        if !copied.iter().any(|other| names::same_name(other, path)) {
            copied.push(path.clone());
        }
    }
    let folders: Vec<String> = moved
        .iter()
        .map(|moved| moved.to.as_str())
        .chain(plan.models.iter().map(|model| model.path.as_str()))
        .chain(created.iter().map(|created| created.path.as_str()))
        .filter(|path| !path.starts_with('.'))
        .filter_map(|path| path.split_once('/').map(|(folder, _)| folder.to_owned()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|folder| fs::symlink_metadata(root.join(folder)).is_err())
        .collect();
    let mut journal = Journal {
        phase: Phase::Backup,
        source: hash(&source),
        target: hash(&manifest),
        backup: backup.clone(),
        copied,
        moved: moved.clone(),
        created,
        orphans: plan.orphans.clone(),
        folders,
    };
    write_journal(&root, &journal, true).map_err(Failed)?;
    hook(Step::Journal);
    // Undoes what was done when a phase fails before the project file changed.
    let undone = |journal: &Journal, error: String| match undo(&root, journal) {
        Ok(()) => Failed(format!("{error}; nada mudou")),
        Err(restore) => Kept(format!(
            "{error}; também não foi possível desfazer o que já tinha sido feito ({restore}): a \
             próxima abertura do projeto termina de desfazer"
        )),
    };
    if let Err(error) = copy_backup(&root, &journal, hook) {
        return Err(undone(&journal, error));
    }
    journal.phase = Phase::Nomes;
    if let Err(error) = write_journal(&root, &journal, false) {
        return Err(undone(&journal, error));
    }
    if let Err(error) = make_names(&root, &journal, hook) {
        return Err(undone(&journal, error));
    }
    journal.phase = Phase::Manifesto;
    if let Err(error) = write_journal(&root, &journal, false) {
        return Err(undone(&journal, error));
    }
    let written = persistence::transaction_write_hooked(writes, |index| {
        hook(Step::Install(index));
    });
    if let Err(error) = written {
        return Err(undone(&journal, error.to_string()));
    }
    let mut warnings = Vec::new();
    journal.phase = Phase::Limpeza;
    let cleaned =
        write_journal(&root, &journal, false).and_then(|()| finish(&root, &journal, hook));
    match cleaned {
        Ok(kept) => warnings.extend(kept),
        Err(error) => warnings.push(format!(
            "O projeto foi organizado em pastas, mas os nomes antigos de alguns arquivos ficaram: \
             {error}; a próxima abertura termina de tirá-los"
        )),
    }
    known.retain(|id| plan.models.iter().any(|model| model.id == id));
    for model in &plan.models {
        known.note(model.id.clone(), model.path.clone(), model.bytes.clone());
    }
    if let Some(images) = images {
        images.configure(&root, &plan.project);
        images.mark_saved();
    }
    let changed_assets = plan.changes_assets(project);
    Ok(Converted {
        models: plan.model_paths(),
        project: plan.project,
        summary: Some(summary),
        notes: plan.notes,
        moved,
        backup: Some(backup),
        warnings,
        changed_assets,
    })
}

/// The line on a conversion by `who` that put `parts` in their folders and the copy of the
/// project, of format `format`, in `backup`.
fn summary(who: Who, parts: &[String], backup: &str, format: Option<u64>) -> String {
    let done = match who {
        Who::Person => "Projeto organizado em pastas ao salvar",
        Who::Agent => "O agente organizou o projeto em pastas ao gravá-lo",
    };
    let what = if parts.is_empty() {
        "; nenhum arquivo mudou de lugar".to_owned()
    } else {
        format!(": {}", parts.join(", "))
    };
    let opens = if format.is_some_and(|format| format <= 5) {
        ", que ainda abre na OXY 0.6"
    } else {
        ""
    };
    format!("{done}{what}. Cópia do projeto como estava em {backup}/{opens}.")
}

/// The first save of a project of the old layout, whose folder has no project file yet: in the
/// tree layout at once, each model in its file. Refused while the folder has the files of the
/// project at their old paths (its project file went away outside OXY): saved in the old layout,
/// the next save organizes it with the copy of the project first.
fn first_save(
    file: &Path,
    project: &Project,
    images: Option<&mut TextureCache>,
    known: &mut KnownFiles,
    painted: &HashSet<Id>,
) -> Result<Converted, ConversionError> {
    let root = file.parent().unwrap_or(Path::new("."));
    // A folder not made yet has nothing to look at.
    let looked = fs::symlink_metadata(root).is_ok().then_some(root);
    let plan = assign(project, looked, painted).map_err(ConversionError::Failed)?;
    if looked.is_some() && plan.moved.iter().any(|moved| moved.from != moved.to) {
        return Err(ConversionError::Refused(format!(
            "{PROJECT_FILE} não estava mais na pasta, e os arquivos do projeto continuam com os \
             nomes antigos; o próximo salvamento o organiza em pastas, com a cópia completa antes"
        )));
    }
    let saved = match images {
        Some(images) => persistence::save_cached_bundle_with(file, &plan.project, images, known),
        None => persistence::save_project_with(file, &plan.project, known),
    }
    .map_err(|error| ConversionError::Failed(error.to_string()))?;
    let changed_assets = plan.changes_assets(project);
    Ok(Converted {
        models: plan.model_paths(),
        project: plan.project,
        summary: None,
        notes: plan.notes,
        moved: Vec::new(),
        backup: None,
        warnings: saved,
        changed_assets,
    })
}

/// Copies the project file and every file `journal.copied` names to the backup, each at its
/// path, only created.
fn copy_backup(root: &Path, journal: &Journal, hook: &mut dyn FnMut(Step)) -> Result<(), String> {
    let backup = root.join(&journal.backup);
    let failed = |path: &str, error: io::Error| {
        format!(
            "Não foi possível copiar {path} para {}: {error}",
            journal.backup
        )
    };
    fs::create_dir_all(backup.parent().unwrap_or(root))
        .map_err(|error| failed(PROJECT_FILE, error))?;
    fs_ops::create_dir_new(&backup).map_err(|error| format!("{}: {error}", journal.backup))?;
    let all = std::iter::once(PROJECT_FILE).chain(journal.copied.iter().map(String::as_str));
    for (index, path) in all.enumerate() {
        hook(Step::Backup(index));
        let from = file_of(root, path)?;
        let to = file_of(root, &format!("{}/{path}", journal.backup))?;
        if let Some(folder) = to.parent() {
            fs::create_dir_all(folder).map_err(|error| failed(path, error))?;
        }
        fs_ops::retrying(|| fs_ops::copy_new(&from, &to)).map_err(|error| failed(path, error))?;
    }
    Ok(())
}

/// Gives each file `journal.moved` names its new name, beside the old one: a second name for
/// the same file, or a copy on a disk without them. A file already at the new name with the
/// same bytes stays.
fn make_names(root: &Path, journal: &Journal, hook: &mut dyn FnMut(Step)) -> Result<(), String> {
    for (index, moved) in journal.moved.iter().enumerate() {
        hook(Step::Name(index));
        if moved.reused {
            continue;
        }
        let (from, to) = (file_of(root, &moved.from)?, file_of(root, &moved.to)?);
        let failed = |error: io::Error| format!("Não foi possível criar {}: {error}", moved.to);
        if let Some(folder) = to.parent() {
            fs::create_dir_all(folder).map_err(failed)?;
        }
        match fs_ops::retrying(|| fs_ops::link_or_copy(&from, &to)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if same_files(&from, &to).map_err(failed)? != Some(true) {
                    return Err(failed(error));
                }
            }
            Err(error) => return Err(failed(error)),
        }
    }
    Ok(())
}

/// Removes the file `file`, waiting a moment while another program holds it; one already gone
/// is fine.
fn remove(file: &Path) -> io::Result<()> {
    match fs_ops::retrying(|| fs::remove_file(file)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

/// Removes, the deepest first, each folder of `folders` (from `root`) that is empty.
fn remove_empty_folders(root: &Path, folders: impl IntoIterator<Item = String>) {
    let mut folders: Vec<String> = folders.into_iter().collect();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.matches('/').count()));
    folders.dedup();
    for folder in folders {
        let _ = fs_ops::remove_empty_dir(&root.join(folder));
    }
}

/// The folders on the way to each of `paths`, from the project's folder.
fn folders_of<'a>(paths: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut folders = Vec::new();
    for path in paths {
        let mut folder = path.as_str();
        while let Some((parent, _)) = folder.rsplit_once('/') {
            folders.push(parent.to_owned());
            folder = parent;
        }
    }
    folders
}

/// Removes the journal of the project at `root`, and OXY's folders it leaves empty.
fn forget_journal(root: &Path) -> Result<(), String> {
    remove(&root.join(JOURNAL)).map_err(|error| format!("{JOURNAL}: {error}"))?;
    let _ = fs_ops::remove_empty_dir(&root.join(".oxy/backup"));
    let _ = fs_ops::remove_empty_dir(&root.join(".oxy"));
    Ok(())
}

/// Undoes a conversion whose project file did not change: the files it created go while they
/// hold what it wrote, the new names while they hold what the old ones hold, the folders it made
/// while empty, then its copy of the project and its journal. Why not, when a file cannot be
/// removed now (the journal stays, so trying again finishes it).
fn undo(root: &Path, journal: &Journal) -> Result<(), String> {
    let mut errors = Vec::new();
    for created in &journal.created {
        let Ok(file) = file_of(root, &created.path) else {
            continue;
        };
        if let Ok(bytes) = fs::read(&file)
            && hash(&bytes) == created.hash
            && let Err(error) = remove(&file)
        {
            errors.push(format!("{}: {error}", created.path));
        }
    }
    for moved in journal.moved.iter().filter(|moved| !moved.reused) {
        let (Ok(from), Ok(to)) = (file_of(root, &moved.from), file_of(root, &moved.to)) else {
            continue;
        };
        let same = match same_files(&to, &from) {
            Ok(Some(same)) => same,
            // The old name went: the copy of the project says what it held.
            Ok(None) => file_of(root, &format!("{}/{}", journal.backup, moved.from))
                .ok()
                .and_then(|copy| same_files(&to, &copy).ok().flatten())
                .unwrap_or(false),
            Err(error) => {
                errors.push(format!("{}: {error}", moved.to));
                continue;
            }
        };
        if same && let Err(error) = remove(&to) {
            errors.push(format!("{}: {error}", moved.to));
        }
    }
    remove_empty_folders(root, journal.folders.iter().cloned());
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    // The copy of the project as it was: the project is that one again. Only a copy that holds
    // what the project holds goes.
    let backup = root.join(&journal.backup);
    for path in std::iter::once(&PROJECT_FILE.to_owned()).chain(&journal.copied) {
        let (Ok(copy), Ok(original)) = (
            file_of(root, &format!("{}/{path}", journal.backup)),
            file_of(root, path),
        ) else {
            continue;
        };
        if same_files(&copy, &original).ok().flatten() == Some(true)
            && let Err(error) = remove(&copy)
        {
            errors.push(format!("{}/{path}: {error}", journal.backup));
        }
    }
    remove_empty_folders(&backup, folders_of(&journal.copied));
    let _ = fs_ops::remove_empty_dir(&backup);
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    forget_journal(root)
}

/// Ends a conversion whose project file is the new one: each old name and orphan goes while it
/// holds what its copy in the backup holds; one that changed since stays, told. Then the folders
/// left empty and the journal go. Why not, when a file cannot be removed now (the journal stays).
fn finish(
    root: &Path,
    journal: &Journal,
    hook: &mut dyn FnMut(Step),
) -> Result<Vec<String>, String> {
    let mut kept = Vec::new();
    let mut errors = Vec::new();
    let mut old: Vec<&String> = Vec::new();
    for path in journal
        .moved
        .iter()
        .map(|moved| &moved.from)
        .chain(&journal.orphans)
    {
        if !old.iter().any(|other| names::same_name(other, path)) {
            old.push(path);
        }
    }
    for (index, path) in old.iter().enumerate() {
        hook(Step::Clean(index));
        let (Ok(file), Ok(copy)) = (
            file_of(root, path),
            file_of(root, &format!("{}/{path}", journal.backup)),
        ) else {
            continue;
        };
        match same_files(&file, &copy) {
            Ok(Some(true)) => {
                if let Err(error) = remove(&file) {
                    errors.push(format!("{path}: {error}"));
                }
            }
            // Gone already.
            Ok(None) if fs::symlink_metadata(&file).is_err() => {}
            Ok(_) => kept.push(format!(
                "{path} mudou depois da cópia em {} e ficou na pasta do projeto; apague-o se não \
                 precisar dele",
                journal.backup
            )),
            Err(error) => errors.push(format!("{path}: {error}")),
        }
    }
    remove_empty_folders(root, folders_of(old.iter().copied()));
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    forget_journal(root)?;
    Ok(kept)
}

/// What finishing or undoing an interrupted conversion told: what was done, and what is wrong.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resumed {
    pub fixes: Vec<String>,
    pub warnings: Vec<String>,
}

/// Finishes or undoes a conversion of the project at `root` that stopped (the computer went
/// off, OXY was closed), as opening the project does: while the project file is the old one,
/// what the conversion made goes and the project stays as it was (its next save organizes it
/// again); once it is the new one, the old names go. A project file that is neither (saved
/// another way meanwhile) is left as it is, and the journal is set aside. Nothing happens while
/// a conversion holds the project, or without a journal.
pub fn resume(root: &Path) -> Resumed {
    if fs::symlink_metadata(root.join(JOURNAL)).is_err() {
        return Resumed::default();
    }
    let busy = || Resumed {
        fixes: Vec::new(),
        warnings: vec![
            "Uma organização em pastas deste projeto está em andamento; reabra-o quando ela \
             terminar."
                .into(),
        ],
    };
    if project_lock::converting(root) {
        return busy();
    }
    match ConversionLock::hold(root) {
        Ok(_lock) => resume_locked(root),
        Err(_) => busy(),
    }
}

fn resume_locked(root: &Path) -> Resumed {
    let mut resumed = Resumed::default();
    // A file another program holds now is no sign of another save: the journal stays for the
    // next opening. A journal that does not decode, or a project file that went, is.
    let read = |relative: &str| match fs_ops::retrying(|| fs::read(root.join(relative))) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Uma organização do projeto em pastas foi interrompida e não pôde ser concluída \
             agora: {relative} {}; a próxima abertura tenta de novo",
            project_tree::now_unreadable(&error)
        )),
    };
    let (journal, now) = match (read(JOURNAL), read(PROJECT_FILE)) {
        (Ok(None), _) => return resumed,
        (Err(why), _) | (_, Err(why)) => {
            resumed.warnings.push(why);
            return resumed;
        }
        (Ok(Some(journal)), Ok(now)) => (
            serde_json::from_slice::<Journal>(&journal).ok(),
            now.map(|bytes| hash(&bytes)),
        ),
    };
    match journal {
        Some(journal) if now.as_ref() == Some(&journal.source) => match undo(root, &journal) {
            Ok(()) => resumed.fixes.push(
                "Uma organização do projeto em pastas foi interrompida antes de gravar o projeto e \
                 foi desfeita: ele continua como estava, e o próximo salvamento o organiza de novo"
                    .into(),
            ),
            Err(error) => resumed.warnings.push(format!(
                "Uma organização do projeto em pastas foi interrompida e não pôde ser desfeita \
                 agora ({error}); a próxima abertura tenta de novo"
            )),
        },
        Some(journal) if now.as_ref() == Some(&journal.target) => {
            match finish(root, &journal, &mut |_| {}) {
                Ok(kept) => {
                    resumed.fixes.push(format!(
                        "Uma organização do projeto em pastas que tinha sido interrompida foi \
                         concluída: os nomes antigos dos arquivos saíram, e a cópia do projeto \
                         como estava ficou em {}/",
                        journal.backup
                    ));
                    resumed.warnings.extend(kept);
                }
                Err(error) => resumed.warnings.push(format!(
                    "Uma organização do projeto em pastas foi interrompida e não pôde ser \
                     concluída agora ({error}); a próxima abertura tenta de novo"
                )),
            }
        }
        _ => match set_aside(root) {
            Ok(aside) => resumed.warnings.push(format!(
                "Uma organização do projeto em pastas foi interrompida, e o projeto foi gravado de \
                 outro jeito depois; nenhum arquivo foi mexido, e o diário dela ficou em {aside}"
            )),
            Err(error) => resumed.warnings.push(format!(
                "Uma organização do projeto em pastas foi interrompida, e o diário dela não pôde \
                 ser guardado à parte: {error}"
            )),
        },
    }
    resumed
}

/// Moves the journal of the project at `root` beside it as `conversao.<date>.json`, never over
/// a file; answers where, from the project's folder.
fn set_aside(root: &Path) -> Result<String, String> {
    let date = fs_ops::utc(SystemTime::now()).replace(':', "-");
    let from = file_of(root, JOURNAL)?;
    for name in [
        format!(".oxy/conversao.{date}.json"),
        format!(".oxy/conversao.{date}-{}.json", crate::document::new_id()),
    ] {
        let to = file_of(root, &name)?;
        match fs_ops::retrying(|| fs_ops::move_no_replace(&from, &to)) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            moved => return moved.map(|()| name).map_err(|error| error.to_string()),
        }
    }
    Err("não há um nome livre para ele".into())
}

/// The lines an agent's conversion left for the editor to tell about the project at `root`, if
/// any (see [`AGENT_NOTE`]).
pub fn agent_note(root: &Path) -> Vec<String> {
    fs::read(root.join(AGENT_NOTE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Json>(&bytes).ok())
        .and_then(|note| {
            note["lines"].as_array().map(|lines| {
                lines
                    .iter()
                    .filter_map(|line| line.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Takes away the note of an agent's conversion of the project at `root`, once the person has
/// seen it and saved.
pub fn forget_agent_note(root: &Path) {
    if fs::symlink_metadata(root.join(AGENT_NOTE)).is_ok() {
        let _ = remove(&root.join(AGENT_NOTE));
        let _ = fs_ops::remove_empty_dir(&root.join(".oxy"));
    }
}

/// `project` in the tree layout as a new folder takes it (see [`assign`]): each image, sound and
/// model under the folder of its kind, named as the Library shows it. Nothing is looked at.
pub fn in_folders(project: &Project) -> Plan {
    assign(project, None, &HashSet::new()).expect("nothing is read without a folder")
}

/// What saving a copy of a project told.
#[derive(Clone, Debug)]
pub struct Copied {
    /// The project as the copy has it: in the tree layout, which a copy of a project of the old
    /// layout is organized in.
    pub project: Project,
    pub warnings: Vec<String>,
}

/// Saves an independent copy of `project`, whose folder is `source_root`, at `path`, as
/// [`persistence::save_cached_copy_with`] does; a project of the old layout is written there
/// organized in folders (see [`in_folders`]), each file read at its old path. The copy is new,
/// so nothing is kept of the old layout.
pub fn save_copy(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: &Path,
    known: &mut KnownFiles,
) -> Result<Copied, PersistenceError> {
    if project.layout == Layout::Tree {
        let warnings =
            persistence::save_cached_copy_with(path, project, images, source_root, known)?;
        return Ok(Copied {
            project: project.clone(),
            warnings,
        });
    }
    let plan = in_folders(project);
    let old_paths: HashMap<Id, String> = project
        .assets
        .iter()
        .filter(|asset| asset.kind != AssetKind::Model)
        .map(|asset| (asset.id.clone(), asset.path.clone()))
        .collect();
    let mut warnings = plan.notes.clone();
    warnings.extend(persistence::save_cached_copy_from(
        path,
        &plan.project,
        images,
        source_root,
        &old_paths,
        known,
    )?);
    Ok(Copied {
        project: plan.project,
        warnings,
    })
}
