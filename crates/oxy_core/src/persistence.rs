//! Validated documents and staged, recoverable file replacement.
use crate::{
    document::{
        Asset, AssetKind, GameSettings, Id, Layout, Project, new_id, validate_model,
        validate_project,
    },
    fs_ops::{self, Stamp},
    migration::MigrationError,
    painting::PaintImage,
    project_tree::{self, Presence, Tree, TreeFile},
    script::{self, ScriptFile, ScriptSources},
    texture_cache::TextureCache,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

mod files;
mod open;
pub use files::departed;
pub(crate) use open::name_of;
pub use open::{
    Adopted, AssetState, Checked, KnownFiles, OpenReport, adopt, check_assets, open_project, scan,
    shared_names,
};

/// Why a project, asset or save operation failed. Messages are shown to the user as-is.
#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error(transparent)]
    Migration(#[from] MigrationError),
    /// Rejected by document validation.
    #[error("{0}")]
    InvalidProject(String),
    #[error("Não foi possível abrir {}: {source}", .path.display())]
    Open { path: PathBuf, source: io::Error },
    #[error("O documento excede o limite de leitura de 64 MB")]
    DocumentTooLarge,
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        source: io::Error,
    },
    #[error(transparent)]
    Fs(#[from] io::Error),
    #[error(transparent)]
    Serialize(#[from] serde_json::Error),
    /// PNG encoding or decoding.
    #[error("{0}")]
    Image(String),
    #[error("Caminho de asset inválido: {0}")]
    InvalidAssetPath(String),
    /// A part of the path Windows would read otherwise (see [`crate::names::check_part`]).
    #[error("Caminho inválido {path}: {reason}")]
    InvalidPath { path: String, reason: String },
    #[error("Caminho sem pasta existente")]
    NoExistingFolder,
    #[error("O caminho do asset sai da pasta do projeto: {0}")]
    AssetOutsideProject(String),
    #[error("Asset ausente ou inacessível '{name}': {source}")]
    AssetMissing { name: String, source: io::Error },
    #[error("Arquivo de asset inválido ou maior que 300 MB: {0}")]
    AssetTooLarge(String),
    #[error("Falha ao ler '{name}': {source}")]
    AssetRead { name: String, source: io::Error },
    /// The file exists but its PNG/WAV content is not usable.
    #[error("{name}: {reason}")]
    AssetContent { name: String, reason: String },
    #[error("{0}")]
    InvalidAudio(String),
    #[error("Pixels associados a asset não-textura: {0}")]
    PixelsOnNonTexture(String),
    #[error("Textura alterada sem asset correspondente: {0}")]
    DirtyTextureWithoutAsset(Id),
    #[error("Textura alterada não está residente")]
    DirtyTextureNotResident,
    #[error("A pasta escolhida já contém um asset diferente: {0}. Escolha uma pasta vazia.")]
    DestinationConflict(String),
    #[error("Não foi possível copiar {name}: {source}")]
    CopySource { name: String, source: io::Error },
    #[error("Modelos são salvos pelo Estúdio; importe PNG ou WAV")]
    ModelImport,
    #[error("Não foi possível ler origem {}: {source}", .path.display())]
    ReadSource { path: PathBuf, source: io::Error },
    #[error("Textura não encontrada")]
    TextureNotFound,
    #[error("Asset não é textura")]
    NotTexture,
    #[error("Dois assets compartilham o caminho {}", .0.display())]
    DuplicateTarget(PathBuf),
    #[error("O destino é uma pasta: {}", .0.display())]
    TargetIsDirectory(PathBuf),
    /// A file that was only to be created found its name taken (see [`WriteMode::Create`]).
    #[error("{}", taken(.0))]
    TargetExists(PathBuf),
    /// A file to replace no longer holds what OXY expected (see [`WriteMode::Replace`]).
    #[error("{} mudou fora da OXY; nada foi gravado", fs_ops::shown(.0))]
    TargetChanged(PathBuf),
    /// Every file was restored; nothing on disk changed.
    #[error("Salvamento não concluído; versão anterior preservada: {0}")]
    SaveRolledBack(String),
    /// Restoration failed; recovery copies were left next to the files.
    #[error(
        "Falha no salvamento: {error}. Cópias de recuperação foram mantidas na pasta. Falha ao restaurar: {restore}"
    )]
    SaveRecoveryKept { error: String, restore: String },
    /// A folder a new project may not go in (see [`create_project`]).
    #[error("{0}")]
    ProjectFolder(String),
    /// An object attaches a script the project's folder lacks.
    #[error("{object}: o script {path} não existe na pasta do projeto")]
    ScriptMissing { object: String, path: String },
    #[error("O script {0} tem mais de 1 MB")]
    ScriptTooLarge(String),
    #[error("O script {0} não é texto UTF-8")]
    ScriptNotText(String),
    #[error("Falha ao ler o script {path}: {source}")]
    ScriptRead { path: String, source: io::Error },
    /// A copy would replace a different script where it goes.
    #[error("A pasta escolhida já contém um arquivo diferente: {0}. Escolha uma pasta vazia.")]
    ScriptConflict(String),
    /// The project's folder is past what a project holds (see [`crate::project_tree::walk`]).
    #[error("A pasta do projeto é grande demais: {0}; mova o projeto para uma pasta só dele")]
    TreeTooLarge(String),
    /// A model's own file, which the strict load reads, is missing or holds no model of the
    /// project's (see [`crate::model_file`]).
    #[error("O modelo {name}: o arquivo {path} {reason}")]
    ModelFile {
        name: String,
        path: String,
        reason: String,
    },
    /// A model's file would go where a file that is not the model's is.
    #[error(
        "Já existe {path}, e ele não é o arquivo do modelo {name}; a OXY não grava por cima \
         dele: mova ou renomeie esse arquivo e salve de novo"
    )]
    ModelFileTaken { name: String, path: String },
    /// Sending to the project's trash the files whose assets left the project failed; nothing
    /// was saved.
    #[error("Salvamento não concluído: {0}")]
    Trash(String),
}
impl From<PersistenceError> for String {
    fn from(error: PersistenceError) -> Self {
        error.to_string()
    }
}
fn io_context(context: &'static str) -> impl FnOnce(io::Error) -> PersistenceError {
    move |source| PersistenceError::Io { context, source }
}

pub const PROJECT_FILE: &str = "project.oxy.json";
pub fn load_project(path: &Path) -> Result<Project, PersistenceError> {
    load_project_with(path, false).map(|(project, _)| project)
}
/// Validates document, relative paths and asset headers without decoding PNG pixels.
/// A damaged pixel stream is diagnosed when the texture is first requested.
pub fn load_project_lazy(path: &Path) -> Result<Project, PersistenceError> {
    load_project_with(path, true).map(|(project, _)| project)
}
pub fn load_project_lazy_report(path: &Path) -> Result<(Project, Option<u32>), PersistenceError> {
    load_project_with(path, true)
}
/// The strict load: the project file as it is, and in the tree layout each model's pieces from
/// the file at its path (an exported game's models are inside its project file). Nothing is
/// looked for elsewhere, and a file missing or unusable fails.
fn load_project_with(path: &Path, lazy: bool) -> Result<(Project, Option<u32>), PersistenceError> {
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let bytes = read_document(&path)?;
    let (mut project, migrated) = crate::migration::read_report(&bytes)?;
    let root = path.parent().unwrap_or(Path::new("."));
    if project.layout == Layout::Tree {
        for asset in &mut project.assets {
            if asset.kind != AssetKind::Model || asset.model.is_some() {
                continue;
            }
            let failed = |reason: String| PersistenceError::ModelFile {
                name: asset.name.clone(),
                path: asset.path.clone(),
                reason,
            };
            let (id, entities, _) = open::read_model_file(root, &asset.path).map_err(&failed)?;
            if id != asset.id {
                return Err(failed("é de outro modelo".into()));
            }
            asset.model = Some(entities);
        }
    }
    validate_project(&project).map_err(PersistenceError::InvalidProject)?;
    if lazy {
        for asset in &project.assets {
            if asset.kind != AssetKind::Model {
                validate_asset_header(asset, &resolve_asset_path(root, &asset.path)?)?;
            }
        }
    } else {
        validate_asset_files(&project, root)?;
    }
    Ok((project, migrated))
}
/// The bytes of the project file `path`, up to 64 MB.
fn read_document(path: &Path) -> Result<Vec<u8>, PersistenceError> {
    let meta = fs::metadata(path).map_err(|source| PersistenceError::Open {
        path: path.to_owned(),
        source,
    })?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err(PersistenceError::DocumentTooLarge);
    }
    fs::read(path).map_err(io_context("Falha ao ler projeto"))
}
/// The game's title (its name, else the project's) and its settings, read from the document at
/// `path` without loading the project: the player opens its window before that. None when the
/// document cannot be read; settings it cannot use read as the defaults. Loading then says what
/// is wrong.
pub fn read_game_settings(path: &Path) -> Option<(String, GameSettings)> {
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    if fs::metadata(&path).ok()?.len() > 64 * 1024 * 1024 {
        return None;
    }
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let game = document
        .get("game")
        .and_then(|game| serde_json::from_value::<GameSettings>(game.clone()).ok())
        .filter(|game| game.validate().is_ok())
        .unwrap_or_default();
    let title = match game.title.trim() {
        "" => document["name"].as_str()?.trim(),
        title => title,
    }
    .to_owned();
    (!title.is_empty()).then_some((title, game))
}
pub fn save_project(path: &Path, project: &Project) -> Result<(), PersistenceError> {
    save_project_with(path, project, &mut KnownFiles::default()).map(drop)
}
/// Saves `project` at `path` (its folder or its project file): the project file and, in the
/// tree layout, the models' files that changed, with `known`, what OXY last read or wrote of
/// them, kept up to date (see [`files`]). A file missing, unreadable or with an invalid path is
/// no reason to refuse: opening tells it. Answers what the save tells besides (a model's file
/// changed outside OXY kept before it was replaced, files sent to the project's trash).
pub fn save_project_with(
    path: &Path,
    project: &Project,
    known: &mut KnownFiles,
) -> Result<Vec<String>, PersistenceError> {
    validate_project(project).map_err(PersistenceError::InvalidProject)?;
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    let plan = files::plan(&path, project, known, None)?;
    files::commit(root, Vec::new(), plan, known)
}
/// The copy of the project file at `path` a save keeps when it migrates the file (see
/// [`crate::migration::backup`]), only created.
fn schema_backup(path: &Path) -> Result<Vec<FileWrite>, PersistenceError> {
    Ok(crate::migration::backup(path)?
        .into_iter()
        .map(|(target, bytes)| FileWrite::create(target, bytes))
        .collect())
}
/// Writes `project` as a new project in `folder`, as the editor's first Salvar of a new project
/// does (the project file alone, in the current format), where an agent may create one: a full
/// path on a disk of this computer, not a disk's root, neither in nor holding a `protected`
/// folder (the engine's), and a folder that is new or empty, named as such (not as its project
/// file). Everything is checked before anything is written, and the folders made for it go
/// again when saving fails. Answers the project file.
pub fn create_project(
    folder: &Path,
    project: &Project,
    protected: &[PathBuf],
) -> Result<PathBuf, PersistenceError> {
    validate_project(project).map_err(PersistenceError::InvalidProject)?;
    let folder = new_project_folder(folder, protected)?;
    // Only what is truly absent: a link that leads nowhere is the person's, not made here.
    let created: Vec<PathBuf> = folder
        .ancestors()
        .take_while(|folder| {
            matches!(fs::symlink_metadata(folder), Err(e) if e.kind() == io::ErrorKind::NotFound)
        })
        .map(Path::to_owned)
        .collect();
    let file = folder.join(PROJECT_FILE);
    // A new folder has none of the files the project's assets name.
    let saved = fs::create_dir_all(&folder)
        .map_err(io_context("Não foi possível criar pasta do projeto"))
        .and_then(|()| validate_asset_files(project, &folder))
        .and_then(|()| save_project(&file, project));
    if let Err(error) = saved {
        // Only empty folders go, the deepest first.
        for folder in &created {
            let _ = fs::remove_dir(folder);
        }
        return Err(error);
    }
    Ok(file)
}
/// `folder` as a new project's folder, with `.` and `..` worked out, refusing what
/// [`create_project`] refuses.
fn new_project_folder(folder: &Path, protected: &[PathBuf]) -> Result<PathBuf, PersistenceError> {
    use crate::asset_import::{local_disk_path, normalized, overlaps, real};
    let refuse = |message: String| Err(PersistenceError::ProjectFolder(message));
    let text = folder.to_string_lossy();
    let folder = normalized(local_disk_path(&text, true).map_err(PersistenceError::ProjectFolder)?);
    if folder.parent().is_none() {
        return refuse(format!(
            "A raiz de um disco ({}) não recebe projetos; escolha uma pasta dentro dele.",
            folder.display()
        ));
    }
    let resolved = real(&folder);
    if protected
        .iter()
        .any(|engine| overlaps(&resolved, &real(engine)))
    {
        return refuse(format!(
            "A pasta do projeto ({}) não pode ficar dentro da pasta da OXY Engine nem contê-la; escolha outra pasta.",
            folder.display()
        ));
    }
    let project_file = |path: &Path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(PROJECT_FILE))
    };
    let meta = match fs::metadata(&folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        meta => Some(meta.map_err(io_context("Não foi possível ler a pasta do projeto"))?),
    };
    // A project already there, named by its folder or by its file.
    let file = if meta.as_ref().is_some_and(fs::Metadata::is_dir) {
        folder.join(PROJECT_FILE)
    } else {
        folder.clone()
    };
    if file.is_file() && project_file(&file) {
        return refuse(format!(
            "Já existe um projeto em {}. Abra-o ou escolha outra pasta.",
            file.parent().unwrap_or(&folder).display()
        ));
    }
    // Else the project would go in a folder named like its file.
    if project_file(&folder) {
        return refuse(format!(
            "Informe a pasta do projeto ({}), não o arquivo {PROJECT_FILE}.",
            folder.parent().unwrap_or(&folder).display()
        ));
    }
    let Some(meta) = meta else {
        return Ok(folder);
    };
    if !meta.is_dir() {
        return refuse(format!(
            "{} não é uma pasta; escolha uma pasta para o projeto.",
            folder.display()
        ));
    }
    let empty = fs::read_dir(&folder)
        .map_err(io_context("Não foi possível ler a pasta do projeto"))?
        .next()
        .is_none();
    if !empty {
        return refuse(format!(
            "A pasta {} já tem outros arquivos; um projeto novo vai numa pasta nova ou vazia.",
            folder.display()
        ));
    }
    Ok(folder)
}
/// Paint buffers stay in memory until the whole save has been staged successfully.
/// The manifest is replaced last; every previous file has a rollback copy.
pub fn save_bundle(
    path: &Path,
    project: &Project,
    images: &HashMap<Id, PaintImage>,
) -> Result<(), PersistenceError> {
    validate_project(project).map_err(PersistenceError::InvalidProject)?;
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(root).map_err(io_context("Não foi possível criar pasta do projeto"))?;
    let mut known = KnownFiles::default();
    let mut plan = files::plan(&path, project, &known, None)?;
    let mut writes = Vec::new();
    for asset in &project.assets {
        if let Some(image) = images.get(&asset.id) {
            if asset.kind != AssetKind::Texture {
                return Err(PersistenceError::PixelsOnNonTexture(asset.name.clone()));
            }
            let bytes = image.to_png().map_err(PersistenceError::Image)?;
            writes.push(plan.painted(root, project, asset, bytes, &known)?);
        }
    }
    files::commit(root, writes, plan, &mut known).map(drop)
}
/// Writes dirty texture buffers and the manifest in the same recoverable transaction.
/// Clean texture files are left as they are: one missing is no reason to refuse (opening tells
/// it). Dirty cache entries stay pinned if any staging/replacement step fails.
pub fn save_cached_bundle(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
) -> Result<(), PersistenceError> {
    save_cached_bundle_with(path, project, images, &mut KnownFiles::default()).map(drop)
}
/// [`save_cached_bundle`] with what OXY last read or wrote of the models' files, kept up to date
/// (see [`save_project_with`]); answers what the save tells besides.
pub fn save_cached_bundle_with(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    known: &mut KnownFiles,
) -> Result<Vec<String>, PersistenceError> {
    save_cached_bundle_from(path, project, images, None, &HashMap::new(), known)
}

/// Save an independent copy, including lazily loaded PNG/WAV resources and every script of the
/// tree, at its path. Source files are only read. All destination writes share one transaction.
pub fn save_cached_copy(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: &Path,
) -> Result<(), PersistenceError> {
    save_cached_copy_with(
        path,
        project,
        images,
        source_root,
        &mut KnownFiles::default(),
    )
    .map(drop)
}
/// [`save_cached_copy`], leaving in `known` what it wrote of the models' files; answers what the
/// copy tells besides (a file the source lacks, left out).
pub fn save_cached_copy_with(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: &Path,
    known: &mut KnownFiles,
) -> Result<Vec<String>, PersistenceError> {
    save_cached_bundle_from(
        path,
        project,
        images,
        Some(source_root),
        &HashMap::new(),
        known,
    )
}
/// [`save_cached_copy_with`] of a project whose images and sounds the source folder has at
/// other paths, `old_paths` by asset ID: a project of the old layout organized in folders on its
/// way to the copy (see [`crate::conversion::save_copy`]).
pub(crate) fn save_cached_copy_from(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: &Path,
    old_paths: &HashMap<Id, String>,
    known: &mut KnownFiles,
) -> Result<Vec<String>, PersistenceError> {
    save_cached_bundle_from(path, project, images, Some(source_root), old_paths, known)
}

fn save_cached_bundle_from(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: Option<&Path>,
    old_paths: &HashMap<Id, String>,
    known: &mut KnownFiles,
) -> Result<Vec<String>, PersistenceError> {
    validate_project(project).map_err(PersistenceError::InvalidProject)?;
    for (id, _) in images.dirty_images() {
        if !project
            .asset(id)
            .is_some_and(|asset| asset.kind == AssetKind::Texture)
        {
            return Err(PersistenceError::DirtyTextureWithoutAsset(id.clone()));
        }
    }
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(root).map_err(io_context("Não foi possível criar pasta do projeto"))?;
    let mut plan = files::plan(&path, project, known, source_root)?;
    let mut writes = Vec::new();
    for asset in &project.assets {
        if asset.kind == AssetKind::Model {
            continue;
        }
        if images.is_dirty(&asset.id) {
            if asset.kind != AssetKind::Texture {
                return Err(PersistenceError::PixelsOnNonTexture(asset.name.clone()));
            }
            let image = images
                .get(&asset.id)
                .ok_or(PersistenceError::DirtyTextureNotResident)?;
            let bytes = image.to_png().map_err(PersistenceError::Image)?;
            if source_root.is_none() {
                writes.push(plan.painted(root, project, asset, bytes, known)?);
            } else {
                let target = resolve_asset_path(root, &asset.path)?;
                writes.extend(copy_write(target, bytes, || {
                    PersistenceError::DestinationConflict(asset.path.clone())
                })?);
            }
        } else if let Some(source_root) = source_root {
            let from = old_paths.get(&asset.id).unwrap_or(&asset.path);
            let source = resolve_asset_path(source_root, from)?;
            let bytes = match fs::read(&source) {
                Ok(bytes) => bytes,
                // Opening told it missing; the copy goes without it, as the project does.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    plan.warnings.push(format!(
                        "{}: o arquivo {} não existe na pasta de origem e ficou fora da cópia",
                        asset.name, asset.path
                    ));
                    continue;
                }
                Err(source) => {
                    return Err(PersistenceError::CopySource {
                        name: asset.name.clone(),
                        source,
                    });
                }
            };
            let target = resolve_asset_path(root, &asset.path)?;
            writes.extend(copy_write(target, bytes, || {
                PersistenceError::DestinationConflict(asset.path.clone())
            })?);
        }
    }
    if let Some(source_root) = source_root {
        // Every script of the tree goes as it is, attached or not: a script may require any
        // other.
        for file in project_tree::walk(source_root)?.scripts() {
            let path = file.path.clone();
            let source = resolve_asset_path(source_root, &path)?;
            let bytes = match fs::read(&source) {
                Ok(bytes) => bytes,
                // It went in the meantime, or its name is not UTF-8 (the walk warns) and no
                // object or require can name it.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(PersistenceError::CopySource { name: path, source });
                }
            };
            let target = resolve_asset_path(root, &path)?;
            writes.extend(copy_write(target, bytes, || {
                PersistenceError::ScriptConflict(path.clone())
            })?);
        }
    }
    let warnings = files::commit(root, writes, plan, known)?;
    images.configure(root, project);
    images.mark_saved();
    Ok(warnings)
}
/// The write a copy makes of `bytes` at `target`: only created, nothing when the file there
/// already holds them, and `conflict` when it holds anything else.
fn copy_write(
    target: PathBuf,
    bytes: Vec<u8>,
    conflict: impl FnOnce() -> PersistenceError,
) -> Result<Option<FileWrite>, PersistenceError> {
    match fs::read(&target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Some(FileWrite::create(target, bytes)))
        }
        Ok(there) if there == bytes => Ok(None),
        Ok(_) => Err(conflict()),
        Err(error) => Err(error.into()),
    }
}
pub fn load_paint_images(
    project: &Project,
    root: &Path,
) -> Result<HashMap<Id, PaintImage>, PersistenceError> {
    project
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::Texture)
        .map(|a| {
            Ok((
                a.id.clone(),
                PaintImage::load(&resolve_asset_path(root, &a.path)?)
                    .map_err(PersistenceError::Image)?,
            ))
        })
        .collect()
}
pub fn import_asset(
    project: &mut Project,
    root: &Path,
    source: &Path,
    kind: AssetKind,
) -> Result<Id, PersistenceError> {
    import_asset_with(project, root, source, kind, &mut |_, _| Ok(()))
}
/// [`import_asset`], calling `before` with the path the copy gets and the asset's ID right
/// before the file is written (see [`crate::journal::Creating`]); the file is not written when
/// it fails.
pub fn import_asset_with(
    project: &mut Project,
    root: &Path,
    source: &Path,
    kind: AssetKind,
    before: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Id, PersistenceError> {
    if kind == AssetKind::Model {
        return Err(PersistenceError::ModelImport);
    }
    let bytes = fs::read(source).map_err(|e| PersistenceError::ReadSource {
        path: source.to_owned(),
        source: e,
    })?;
    match kind {
        AssetKind::Texture => {
            PaintImage::from_png(&bytes).map_err(PersistenceError::Image)?;
        }
        AssetKind::Audio => validate_wav(&bytes).map_err(PersistenceError::InvalidAudio)?,
        AssetKind::Model => {}
    }
    let name = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Asset importado".into());
    store_asset_as(project, root, new_id(), &bytes, name, kind, "", before)
}
/// Writes PNG or WAV bytes, already checked, to the project's assets folder under a new name in
/// the old layout, as `<name>.<ext>` in the folder `folder` (which exists; "" is the root) in
/// the tree layout (only created: a file there is never written over), and adds the asset as
/// `name`.
pub(crate) fn store_asset_in(
    project: &mut Project,
    root: &Path,
    bytes: &[u8],
    name: String,
    kind: AssetKind,
    folder: &str,
    before: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Id, PersistenceError> {
    store_asset_as(project, root, new_id(), bytes, name, kind, folder, before)
}
/// [`store_asset_in`] as the asset `id`.
#[expect(
    clippy::too_many_arguments,
    reason = "the folder joins what a store already takes"
)]
fn store_asset_as(
    project: &mut Project,
    root: &Path,
    id: Id,
    bytes: &[u8],
    name: String,
    kind: AssetKind,
    folder: &str,
    before: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Id, PersistenceError> {
    fs::create_dir_all(root)?;
    let (name, relative) = new_asset_file_in(project, Some(root), &id, &name, kind, folder);
    let target = resolve_asset_path(root, &relative)?;
    before(&relative, &id).map_err(PersistenceError::InvalidProject)?;
    transaction_write(vec![FileWrite::create(target, bytes.to_vec())])?;
    project.assets.push(Asset {
        id: id.clone(),
        name,
        path: relative,
        kind,
        model: None,
    });
    Ok(id)
}
/// The name and path a new image or sound called `name`, of ID `id`, gets in `project`: `name`
/// and `assets/<id>.<ext>` in the old layout; in the tree layout `<name>.<ext>` at the project's
/// root, the name made one Windows keeps and numbered ("Pulo 2") while an asset of its kind has
/// it, or an asset's path or a file in the project's folder `root` has its file's name. The file
/// is still only created: one that appears meanwhile stays.
pub fn new_asset_file(
    project: &Project,
    root: Option<&Path>,
    id: &str,
    name: &str,
    kind: AssetKind,
) -> (String, String) {
    new_asset_file_in(project, root, id, name, kind, "")
}
/// [`new_asset_file`] in the folder `folder` of the tree layout ("" is the root), as a copy of a
/// texture gets beside its original.
pub fn new_asset_file_in(
    project: &Project,
    root: Option<&Path>,
    id: &str,
    name: &str,
    kind: AssetKind,
    folder: &str,
) -> (String, String) {
    let extension = if kind == AssetKind::Texture {
        "png"
    } else {
        "wav"
    };
    if project.layout == Layout::Legacy {
        return (name.to_owned(), format!("assets/{id}.{extension}"));
    }
    let fallback = if kind == AssetKind::Texture {
        "Imagem"
    } else {
        "Som"
    };
    let base = crate::names::fitted_name(name, fallback);
    let taken = |name: &str, path: &str| {
        project.assets.iter().any(|asset| {
            (asset.kind == kind && asset.name.trim().to_lowercase() == name.to_lowercase())
                || crate::names::same_name(&asset.path, path)
        }) || root.is_some_and(|root| {
            resolve_asset_path(root, path).is_ok_and(|file| fs::symlink_metadata(file).is_ok())
        })
    };
    (1..)
        .map(|n: usize| {
            let name = if n == 1 {
                base.clone()
            } else {
                // The number always fits: the name is cut before it.
                let number = format!(" {n}");
                let kept: String = base
                    .chars()
                    .take(crate::names::MAX_NAME_CHARS - number.chars().count())
                    .collect();
                format!("{}{number}", kept.trim_end_matches(['.', ' ']))
            };
            let path = crate::journal::join(folder, &format!("{name}.{extension}"));
            (name, path)
        })
        .find(|(name, path)| !taken(name, path))
        .expect("a free name")
}
/// Creates an independent texture; neither the original pixels nor its reference changes.
pub fn copy_texture(
    project: &mut Project,
    root: &Path,
    asset_id: &str,
    image: Option<&PaintImage>,
) -> Result<Id, PersistenceError> {
    copy_texture_as(project, root, asset_id, image, new_id())
}
/// [`copy_texture`] as the asset `id`.
fn copy_texture_as(
    project: &mut Project,
    root: &Path,
    asset_id: &str,
    image: Option<&PaintImage>,
    id: Id,
) -> Result<Id, PersistenceError> {
    let asset = project
        .asset(asset_id)
        .ok_or(PersistenceError::TextureNotFound)?
        .clone();
    if asset.kind != AssetKind::Texture {
        return Err(PersistenceError::NotTexture);
    }
    let bytes = if let Some(image) = image {
        image.to_png().map_err(PersistenceError::Image)?
    } else {
        fs::read(resolve_asset_path(root, &asset.path)?)?
    };
    let (name, path) = new_asset_file(
        project,
        Some(root),
        &id,
        &format!("{} (cópia)", asset.name),
        AssetKind::Texture,
    );
    transaction_write(vec![FileWrite::create(
        resolve_asset_path(root, &path)?,
        bytes,
    )])?;
    project.assets.push(Asset {
        id: id.clone(),
        name,
        path,
        kind: AssetKind::Texture,
        model: None,
    });
    Ok(id)
}
/// The file `relative` of the project at `root`, through the one resolver every reader uses
/// ([`crate::names::resolve`]).
pub fn resolve_asset_path(root: &Path, relative: &str) -> Result<PathBuf, PersistenceError> {
    crate::names::resolve(root, relative)
}
pub fn validate_asset_files(project: &Project, root: &Path) -> Result<(), PersistenceError> {
    for asset in &project.assets {
        if asset.kind != AssetKind::Model {
            validate_asset_file(asset, &resolve_asset_path(root, &asset.path)?)?;
        }
    }
    Ok(())
}
fn validate_asset_file(asset: &Asset, path: &Path) -> Result<(), PersistenceError> {
    let meta = fs::metadata(path).map_err(|source| PersistenceError::AssetMissing {
        name: asset.name.clone(),
        source,
    })?;
    if !meta.is_file() || meta.len() > 300_000_000 {
        return Err(PersistenceError::AssetTooLarge(asset.name.clone()));
    }
    let bytes = fs::read(path).map_err(|source| PersistenceError::AssetRead {
        name: asset.name.clone(),
        source,
    })?;
    let content = |reason| PersistenceError::AssetContent {
        name: asset.name.clone(),
        reason,
    };
    match asset.kind {
        AssetKind::Texture => {
            PaintImage::from_png(&bytes).map_err(content)?;
        }
        AssetKind::Audio => validate_wav(&bytes).map_err(content)?,
        AssetKind::Model => {}
    }
    Ok(())
}
/// The scripts of the project at `root`: every `.luau` file of its tree (see
/// [`project_tree::walk`]), by path. A file that cannot be a script now (over 1 MB, not UTF-8
/// text, held by another program, a name scripts may not have) is left in `unreadable` with
/// why, and the rest are read all the same; so are, in `folders`, the folders the walk could not
/// list or skipped as links, other projects or exported games. Only a tree that cannot be walked
/// fails.
pub fn load_scripts(root: &Path) -> Result<ScriptSources, PersistenceError> {
    Ok(read_scripts(root, None)?.0)
}
/// [`load_scripts`] without the script `path`, whose text a write replaces: neither read nor
/// told unreadable.
pub fn load_scripts_except(root: &Path, path: &str) -> Result<ScriptSources, PersistenceError> {
    Ok(read_scripts(root, Some(path))?.0)
}
/// [`load_scripts`] with the tree they came from, whose warnings and configurations the checks
/// of every script tell.
pub fn load_scripts_in_tree(root: &Path) -> Result<(ScriptSources, Tree), PersistenceError> {
    read_scripts(root, None)
}
fn read_scripts(
    root: &Path,
    skip: Option<&str>,
) -> Result<(ScriptSources, Tree), PersistenceError> {
    let tree = project_tree::walk(root)?;
    let mut sources = ScriptSources::default();
    for file in tree.scripts() {
        if skip == Some(file.path.as_str()) {
            continue;
        }
        match read_script_file(root, file) {
            Ok(Some(text)) => {
                sources
                    .files
                    .insert(file.path.clone(), ScriptFile::new(text));
            }
            // It went in the meantime.
            Ok(None) => {}
            Err(reason) => {
                sources.unreadable.insert(file.path.clone(), reason);
            }
        }
    }
    // A script attached or required in a folder the tree did not look into is no missing one.
    for (folder, reason) in &tree.unreadable {
        let shown = if folder.is_empty() {
            "a pasta do projeto".to_owned()
        } else {
            format!("a pasta {folder}")
        };
        let why = format!("{}: {shown} {reason}", project_tree::NOT_NOW);
        sources.folders.insert(folder.clone(), why);
    }
    for (folder, reason) in &tree.skipped {
        let why = format!("fica fora do projeto: {folder} {reason}");
        sources.folders.insert(folder.clone(), why);
    }
    Ok((sources, tree))
}
/// The text of the script `file` of the tree at `root`, none when it went, or why it is no
/// script now, said after its path.
fn read_script_file(root: &Path, file: &TreeFile) -> Result<Option<String>, String> {
    let stamp = file.stamp.clone()?;
    if let Some(reason) = script::path_problem(&file.path) {
        return Err(format!(
            "tem um caminho que os scripts não aceitam: {reason}"
        ));
    }
    let too_large = || "tem mais de 1 MB".to_owned();
    if stamp.len > script::MAX_BYTES {
        return Err(too_large());
    }
    let target = resolve_asset_path(root, &file.path).map_err(|error| error.to_string())?;
    let bytes = match fs_ops::retrying(|| fs::read(&target)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(project_tree::now_unreadable(&error)),
    };
    if bytes.len() as u64 > script::MAX_BYTES {
        return Err(too_large());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "não é texto UTF-8".into())
}
/// What tells that a script file changed without reading it: its size and when it was written.
pub type ScriptStamp = Stamp;

/// The stamp of each script file of the project at `root` (the files [`load_scripts`] reads),
/// by path, from the tree's listing. A file whose stamp cannot be read now is left out: reading
/// the scripts says why.
pub fn script_stamps(root: &Path) -> Result<BTreeMap<String, ScriptStamp>, PersistenceError> {
    Ok(project_tree::walk(root)?
        .scripts()
        .filter_map(|file| Some((file.path.clone(), file.stamp.clone().ok()?)))
        .collect())
}

/// The text of the script `path` of the project at `root` (`path` as [`script::normalize_path`]
/// writes it), with the checks of [`load_scripts`]; none when the tree lacks the file (see
/// [`project_tree::presence`]: in another case, under a link or in another project, too).
pub fn read_script(root: &Path, path: &str) -> Result<Option<String>, PersistenceError> {
    if script::normalize_path(path).as_deref() != Ok(path) {
        return Err(PersistenceError::InvalidAssetPath(path.to_owned()));
    }
    if project_tree::presence(root, path) == Presence::Missing {
        return Ok(None);
    }
    let file = resolve_asset_path(root, path)?;
    let read = |source| PersistenceError::ScriptRead {
        path: path.to_owned(),
        source,
    };
    let meta = match fs::metadata(&file) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(read(source)),
    };
    if meta.len() > script::MAX_BYTES {
        return Err(PersistenceError::ScriptTooLarge(path.to_owned()));
    }
    let bytes = fs::read(&file).map_err(read)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| PersistenceError::ScriptNotText(path.to_owned()))
}

/// Where the agents' copies of the files they change go (scripts, the project file, models'
/// files), from the project's folder: out of the tree, so that no copy is ever a script.
pub const SCRIPT_BACKUPS: &str = ".oxy/agente";

/// The copy of a file an agent keeps, as it was before its session first changed the file, by
/// the file's path: `Pista/porta.luau` keeps `.oxy/agente/Pista/porta.luau.bak`.
pub fn script_backup_path(path: &str) -> String {
    format!("{SCRIPT_BACKUPS}/{path}.bak")
}

/// Writes `text` as the script `path` of the project at `root` (`path` as
/// [`script::normalize_path`] writes it), creating its folders, in one transaction with, when
/// `backup` is set and the file exists, the copy of it as it was at [`script_backup_path`].
/// The file is replaced only while it holds what was read, and created only while absent; never
/// under a link or in another project, where the tree would not find it. Answers the text the
/// file had, if it existed.
pub fn write_script(
    root: &Path,
    path: &str,
    text: &str,
    backup: bool,
) -> Result<Option<String>, PersistenceError> {
    if script::normalize_path(path).as_deref() != Ok(path) {
        return Err(PersistenceError::InvalidAssetPath(path.to_owned()));
    }
    if let Some(reason) = project_tree::outside_reason(root, path) {
        return Err(PersistenceError::InvalidPath {
            path: path.to_owned(),
            reason,
        });
    }
    if text.len() as u64 > script::MAX_BYTES {
        return Err(PersistenceError::ScriptTooLarge(path.to_owned()));
    }
    let target = resolve_asset_path(root, path)?;
    let previous = match fs::read(&target) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(PersistenceError::ScriptRead {
                path: path.to_owned(),
                source,
            });
        }
    };
    let mut writes = Vec::new();
    if let (true, Some(bytes)) = (backup, &previous) {
        writes.push(FileWrite::replace(
            resolve_asset_path(root, &script_backup_path(path))?,
            bytes.clone(),
        ));
    }
    let bytes = text.as_bytes().to_vec();
    writes.push(match &previous {
        Some(read) => FileWrite {
            target,
            bytes,
            mode: WriteMode::Replace {
                expected: Some(Expected::Bytes(read.as_slice().into())),
            },
        },
        None => FileWrite::create(target, bytes),
    });
    transaction_write(writes)?;
    Ok(previous.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

/// Keeps under [`SCRIPT_BACKUPS`] (see [`script_backup_path`]: `.oxy/agente/<path>.bak`, out of
/// the tree) the project file `file` and, in the tree layout, the file of every model whose bytes
/// the save of `project` would change, as they are before an agent's session first changes them:
/// each once per session (`kept` holds the copies the session made) and again when the copy was
/// deleted, all in one transaction. Answers every copy that holds the files the save changes,
/// from the project's folder, the project file's first.
pub fn keep_agent_copies(
    file: &Path,
    project: &Project,
    kept: &mut HashSet<PathBuf>,
) -> Result<Vec<String>, PersistenceError> {
    let root = file.parent().unwrap_or(Path::new("."));
    let mut originals: Vec<(String, Vec<u8>)> = Vec::new();
    match fs::read(file) {
        Ok(bytes) => originals.push((PROJECT_FILE.to_owned(), bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(PersistenceError::Io {
                context: "Não foi possível ler o arquivo do projeto",
                source,
            });
        }
    }
    if project.layout == Layout::Tree {
        for asset in &project.assets {
            let (AssetKind::Model, Some(pieces)) = (asset.kind, &asset.model) else {
                continue;
            };
            let Ok(target) = resolve_asset_path(root, &asset.path) else {
                continue;
            };
            let Ok(disk) = fs::read(&target) else {
                continue;
            };
            if crate::model_file::to_bytes(&asset.id, pieces)? != disk {
                originals.push((asset.path.clone(), disk));
            }
        }
    }
    let mut writes = Vec::new();
    let mut taken = Vec::new();
    let mut copies = Vec::new();
    for (path, bytes) in originals {
        let copy = script_backup_path(&path);
        let target = resolve_asset_path(root, &copy)?;
        if !kept.contains(&target) || !target.is_file() {
            taken.push(target.clone());
            writes.push(FileWrite::replace(target, bytes));
        }
        copies.push(copy);
    }
    if !writes.is_empty() {
        transaction_write(writes)?;
    }
    kept.extend(taken);
    Ok(copies)
}

/// An asset of `kind` as messages name it with its article: "a imagem", "o som", "o modelo".
pub(crate) fn kind_label(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Texture => "a imagem",
        AssetKind::Audio => "o som",
        AssetKind::Model => "o modelo",
    }
}

/// What a missing script asks of the person, to play or export the project again, for a file
/// `objects` objects attach.
pub(crate) fn missing_fix(objects: usize) -> &'static str {
    if objects == 1 {
        "restaure o arquivo ou remova o script do objeto"
    } else {
        "restaure o arquivo ou remova o script dos objetos"
    }
}

/// Checks that every script an object attaches, in a scene or a model, is in the tree of the
/// project at `root`: the first one missing is the error. Opening and saving do not ask for it
/// (a script renamed or deleted outside OXY must not keep the project from opening or saving:
/// they warn, see [`script_warnings`]); playing it in the editor and exporting it do. The files
/// are looked for only when some object attaches a script.
pub fn validate_script_files(project: &Project, root: &Path) -> Result<(), PersistenceError> {
    match missing_scripts(project, root)?.into_iter().next() {
        Some(missing) => Err(PersistenceError::ScriptMissing {
            object: missing.name,
            path: missing.path,
        }),
        None => Ok(()),
    }
}
/// The scripts the objects of `project` attach, in scenes and models, that the tree of the
/// project at `root` lacks: each file is looked for alone (see [`project_tree::presence`]), so
/// one that cannot be read now is no missing one. Nothing is looked for when no object attaches
/// a script.
pub fn missing_scripts(
    project: &Project,
    root: &Path,
) -> Result<Vec<script::MissingScript>, PersistenceError> {
    let lacking: HashSet<String> = script::attached_paths(project)
        .into_iter()
        .filter(|path| project_tree::presence(root, path) == Presence::Missing)
        .collect();
    Ok(script::missing(project, |path| !lacking.contains(path)))
}
/// What opening or saving the project at `root` warns about its scripts, one line per file:
/// every script its objects attach that the tree lacks, by its path and the names of the
/// objects (see [`script::missing_by_file`]). The project opens and saves all the same;
/// playing it in the editor and exporting it are refused until the file is back or no object
/// attaches it.
pub fn script_warnings(project: &Project, root: &Path) -> Vec<String> {
    match missing_scripts(project, root) {
        Ok(missing) => missing_warnings(&missing),
        Err(error) => vec![format!("Não foi possível conferir os scripts: {error}")],
    }
}
/// The lines of [`script_warnings`] about `missing`.
fn missing_warnings(missing: &[script::MissingScript]) -> Vec<String> {
    script::missing_by_file(missing)
        .into_iter()
        .map(|(line, objects)| {
            format!(
                "{line}; jogar e exportar ficam recusados até que você {}",
                missing_fix(objects)
            )
        })
        .collect()
}
/// The lines of [`script_warnings`] that an edit adds to the project it left, `after`: the
/// scripts its objects attach that the tree at `root` lacks, but those `known` before the
/// edit (its [`missing_scripts`]); a copy of an object whose script is missing, for one.
pub fn added_script_warnings(
    known: &[script::MissingScript],
    after: &Project,
    root: &Path,
) -> Vec<String> {
    let Ok(mut missing) = missing_scripts(after, root) else {
        return Vec::new();
    };
    missing.retain(|missing| {
        !known
            .iter()
            .any(|old| old.object == missing.object && old.path == missing.path)
    });
    missing_warnings(&missing)
}
/// The scripts of the project at `root` for a run that needs every one its objects attach, as
/// the editor's test: the first file missing from the tree, or that cannot be read, refuses it,
/// with the objects that attach it (see [`script::absent_by_file`]); a file nothing attaches
/// that cannot be read does not. The message says what to do. Without attached scripts nothing
/// is read: none would run.
pub fn load_scripts_to_run(project: &Project, root: &Path) -> Result<ScriptSources, String> {
    if !script::any_attached(project) {
        return Ok(ScriptSources::default());
    }
    let sources = load_scripts(root).map_err(|error| error.to_string())?;
    match script::absent_by_file(project, &sources).into_iter().next() {
        Some((line, objects)) => Err(format!("{line}; {}", missing_fix(objects))),
        None => Ok(sources),
    }
}
/// Whether a run or an export of `project` needs the asset `asset`: something uses it (see
/// [`crate::editing::asset_uses`]), or it is a sound or a model and an object attaches a script,
/// which may play or place it by name (`audio.play`, `scene.spawn_model`).
pub fn asset_used(project: &Project, asset: &Asset) -> bool {
    !crate::editing::asset_uses(project, &asset.id).is_empty()
        || (matches!(asset.kind, AssetKind::Audio | AssetKind::Model)
            && script::any_attached(project))
}
/// What keeps the asset `asset` of `project`, saved in the folder `root`, from being played or
/// exported now, said after its name: its path is invalid, its file is missing, cannot be read
/// or holds nothing usable, or it is a model without pieces whose file cannot be read, is
/// another model's or does not fit the project (see [`validate_model`]). None when it is fine.
pub fn asset_problem(project: &Project, root: &Path, asset: &Asset) -> Option<String> {
    if asset.kind == AssetKind::Model {
        if asset.model.is_some() {
            return None;
        }
        let path = &asset.path;
        return Some(match open::read_model_file(root, path) {
            Err(why) => format!("o arquivo {path} {why}"),
            Ok((id, _, _)) if id != asset.id => format!("o arquivo {path} é de outro modelo"),
            Ok((_, entities, _)) => {
                let read = Asset {
                    model: Some(entities),
                    ..asset.clone()
                };
                match validate_model(project, &read) {
                    Err(error) => format!("o arquivo {path} não serve neste projeto: {error}"),
                    Ok(()) => format!(
                        "o modelo está indisponível; reabra o projeto para ler {path} de novo"
                    ),
                }
            }
        });
    }
    let file = match resolve_asset_path(root, &asset.path) {
        Ok(file) => file,
        Err(error) => return Some(format!("o caminho {} é inválido ({error})", asset.path)),
    };
    match validate_asset_header(asset, &file) {
        Ok(()) => None,
        Err(PersistenceError::AssetMissing { source, .. })
            if source.kind() == io::ErrorKind::NotFound =>
        {
            Some(format!(
                "o arquivo {} não existe na pasta do projeto",
                asset.path
            ))
        }
        Err(PersistenceError::AssetMissing { source, .. }) => Some(format!(
            "o arquivo {} {}",
            asset.path,
            project_tree::now_unreadable(&source)
        )),
        Err(PersistenceError::AssetContent { reason, .. }) => Some(format!(
            "o arquivo {} não pode ser usado ({reason})",
            asset.path
        )),
        Err(error) => Some(format!("o arquivo {}: {error}", asset.path)),
    }
}
/// Checks that every asset a run of `project`, saved in the folder `root`, needs (see
/// [`asset_used`]) can be read: the first one that cannot (see [`asset_problem`]) refuses the
/// run, with what to do. An asset `in_memory` has (pixels painted, not saved yet) needs no file.
pub fn assets_to_run(
    project: &Project,
    root: &Path,
    in_memory: impl Fn(&str) -> bool,
) -> Result<(), String> {
    for asset in &project.assets {
        if in_memory(&asset.id) || !asset_used(project, asset) {
            continue;
        }
        if let Some(problem) = asset_problem(project, root, asset) {
            return Err(format!(
                "{}: {problem}; restaure o arquivo ou deixe de usar o recurso",
                asset.name
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate_asset_header(asset: &Asset, path: &Path) -> Result<(), PersistenceError> {
    let meta = fs::metadata(path).map_err(|source| PersistenceError::AssetMissing {
        name: asset.name.clone(),
        source,
    })?;
    if !meta.is_file() || meta.len() > 300_000_000 {
        return Err(PersistenceError::AssetTooLarge(asset.name.clone()));
    }
    let content = |reason| PersistenceError::AssetContent {
        name: asset.name.clone(),
        reason,
    };
    match asset.kind {
        AssetKind::Texture => {
            let reader = image::ImageReader::with_format(
                std::io::BufReader::new(File::open(path)?),
                image::ImageFormat::Png,
            );
            let (width, height) = reader
                .into_dimensions()
                .map_err(|e| content(format!("PNG inválido: {e}")))?;
            if width == 0
                || height == 0
                || width > 16384
                || height > 16384
                || u64::from(width) * u64::from(height) > 67_108_864
            {
                return Err(content("dimensões de textura inválidas".into()));
            }
        }
        AssetKind::Audio => {
            let reader =
                hound::WavReader::open(path).map_err(|e| content(format!("WAV inválido: {e}")))?;
            let spec = reader.spec();
            if spec.channels == 0 || spec.channels > 2 || spec.sample_rate == 0 {
                return Err(content(
                    "WAV deve ser mono/estéreo com frequência válida".into(),
                ));
            }
        }
        AssetKind::Model => {}
    }
    Ok(())
}
pub(crate) fn validate_wav(bytes: &[u8]) -> Result<(), String> {
    let reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("WAV inválido: {e}"))?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.channels > 2 || spec.sample_rate == 0 {
        return Err("WAV deve ser mono/estéreo com frequência válida".into());
    }
    Ok(())
}
/// Writes `bytes` at `path`, replacing the file there if any: for the project file and OXY's
/// own files (see [`transaction_write`]).
pub fn safe_write(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    transaction_write(vec![FileWrite::replace(path.to_owned(), bytes.to_vec())])
}

/// What a [`transaction_write`] may find at a file's path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Nothing: the file is only created, and one that appears at its name meanwhile (in any
    /// case, or as a short 8.3 name) stays and fails the write.
    Create,
    /// Any file when `expected` is None (the project file and OXY's own files in `.oxy`);
    /// else only the file still as expected, which fails the write when it changed or went.
    Replace { expected: Option<Expected> },
}
/// What a file to replace must still be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    /// The bytes OXY last read or wrote there.
    Bytes(Arc<[u8]>),
    /// The size and write time it had then.
    Stamp(Stamp),
}
/// One file of a [`transaction_write`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileWrite {
    pub target: PathBuf,
    pub bytes: Vec<u8>,
    pub mode: WriteMode,
}
impl FileWrite {
    /// Writes `bytes` as a new file at `target`.
    pub fn create(target: PathBuf, bytes: Vec<u8>) -> Self {
        Self {
            target,
            bytes,
            mode: WriteMode::Create,
        }
    }
    /// Writes `bytes` at `target`, over the file there if any.
    pub fn replace(target: PathBuf, bytes: Vec<u8>) -> Self {
        Self {
            target,
            bytes,
            mode: WriteMode::Replace { expected: None },
        }
    }
}
/// The message of a file only to be created whose name `target` is taken.
fn taken(target: &Path) -> String {
    format!(
        "Já existe um arquivo em {}; a OXY não grava por cima dele",
        fs_ops::shown(target)
    )
}

struct Staged {
    target: PathBuf,
    temp: PathBuf,
    backup: Option<PathBuf>,
    create: bool,
    committed: bool,
}
fn temporary_path(target: &Path, label: &str) -> PathBuf {
    let file = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{file}.{label}.{}", new_id()))
}
/// Writes every file of `writes` or none: each is written and synced next to its target first,
/// as its [`WriteMode`] allows, and only then put in place, in order; a file replaced keeps a
/// copy until all are in place, so a failure puts every one back. When putting one back fails,
/// the copies stay for recovery ([`PersistenceError::SaveRecoveryKept`]).
pub fn transaction_write(writes: Vec<FileWrite>) -> Result<(), PersistenceError> {
    transaction_write_with(writes, install)
}
/// [`transaction_write`], calling `before` with the index of each file before it is put in
/// place: where tests stop it as a crash would.
pub(crate) fn transaction_write_hooked(
    writes: Vec<FileWrite>,
    mut before: impl FnMut(usize),
) -> Result<(), PersistenceError> {
    let mut index = 0;
    transaction_write_with(writes, |source, target, mode| {
        before(index);
        index += 1;
        install(source, target, mode)
    })
}
/// Why putting a written file in place failed.
enum InstallError {
    /// Its target was left as it was: the name was taken, or the file changed outside OXY.
    Refused(PersistenceError),
    /// Anything else, after which the target may have changed.
    Failed(String),
}
fn transaction_write_with(
    writes: Vec<FileWrite>,
    mut replace: impl FnMut(&Path, &Path, &WriteMode) -> Result<(), InstallError>,
) -> Result<(), PersistenceError> {
    let mut stages: Vec<Staged> = Vec::new();
    let mut modes = Vec::new();
    let mut targets = std::collections::HashSet::new();
    let prepared = (|| -> Result<(), PersistenceError> {
        for FileWrite {
            target,
            bytes,
            mode,
        } in writes
        {
            if !targets.insert(target.clone()) {
                return Err(PersistenceError::DuplicateTarget(target));
            }
            if target.is_dir() {
                return Err(PersistenceError::TargetIsDirectory(target));
            }
            // A link that leads nowhere takes the name all the same. The check only gives the
            // message: what never writes over a file is the move that puts it in place.
            let named = fs::symlink_metadata(&target).is_ok();
            match &mode {
                WriteMode::Create if named => {
                    return Err(PersistenceError::TargetExists(target));
                }
                WriteMode::Replace {
                    expected: Some(expected),
                } => unchanged(&target, expected)?,
                _ => {}
            }
            let parent = target.parent().unwrap_or(Path::new("."));
            fs::create_dir_all(parent).map_err(io_context("Não foi possível criar pasta"))?;
            let temp = temporary_path(&target, "tmp");
            let backup = target.exists().then(|| temporary_path(&target, "backup"));
            stages.push(Staged {
                target: target.clone(),
                temp: temp.clone(),
                backup: backup.clone(),
                create: mode == WriteMode::Create,
                committed: false,
            });
            modes.push(mode);
            fs_ops::create_new_file(&temp, &bytes)
                .map_err(io_context("Falha gravando arquivo temporário"))?;
            if let Some(backup) = backup {
                fs_ops::copy_new(&target, &backup)
                    .map_err(io_context("Falha preservando arquivo anterior"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        cleanup(&stages);
        return Err(error);
    }
    for index in 0..stages.len() {
        match replace(&stages[index].temp, &stages[index].target, &modes[index]) {
            Ok(()) => stages[index].committed = true,
            // The failing file was left as it was: only the ones before it go back.
            Err(InstallError::Refused(error)) => {
                return Err(match roll_back(&stages, index) {
                    Ok(()) => error,
                    Err(restore) => PersistenceError::SaveRecoveryKept {
                        error: error.to_string(),
                        restore,
                    },
                });
            }
            // ReplaceFileW can report an error after removing/renaming the original.
            // Restore the failing stage too; its committed flag cannot prove safety.
            Err(InstallError::Failed(error)) => {
                return Err(match roll_back(&stages, index + 1) {
                    Ok(()) => PersistenceError::SaveRolledBack(error),
                    Err(restore) => PersistenceError::SaveRecoveryKept { error, restore },
                });
            }
        }
    }
    cleanup(&stages);
    Ok(())
}
/// Puts back the first `touched` files of `stages`, the last first, then removes the temporary
/// files and copies; when one cannot be put back, they all stay for recovery and the error says
/// which.
fn roll_back(stages: &[Staged], touched: usize) -> Result<(), String> {
    let errors: Vec<String> = stages[..touched]
        .iter()
        .rev()
        .filter_map(|stage| {
            let error = restore_stage(stage).err()?;
            Some(format!("{}: {error}", stage.target.display()))
        })
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    cleanup(stages);
    Ok(())
}
/// Fails with [`PersistenceError::TargetChanged`] unless the file at `path` is still as
/// `expected`.
fn unchanged(path: &Path, expected: &Expected) -> Result<(), PersistenceError> {
    if path.exists() && holds(path, expected)? {
        Ok(())
    } else {
        Err(PersistenceError::TargetChanged(path.to_owned()))
    }
}
/// Whether the file at `path` is still as `expected`.
fn holds(path: &Path, expected: &Expected) -> Result<bool, PersistenceError> {
    let holds = match expected {
        Expected::Stamp(stamp) => Stamp::read(path).map(|now| now == *stamp),
        Expected::Bytes(bytes) => fs::metadata(path)
            .and_then(|meta| Ok(meta.len() == bytes.len() as u64 && *fs::read(path)? == **bytes)),
    };
    holds.map_err(io_context("Falha conferindo destino"))
}
/// Puts the written `source` in place at `target` as `mode` allows. A file to replace as
/// expected is checked again here, since writing the other files took a while.
fn install(source: &Path, target: &Path, mode: &WriteMode) -> Result<(), InstallError> {
    match mode {
        WriteMode::Create => fs_ops::move_no_replace(source, target).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                InstallError::Refused(PersistenceError::TargetExists(target.to_owned()))
            } else {
                InstallError::Failed(error.to_string())
            }
        }),
        WriteMode::Replace { expected } => {
            if let Some(expected) = expected {
                unchanged(target, expected).map_err(InstallError::Refused)?;
            }
            atomic_replace(source, target).map_err(InstallError::Failed)
        }
    }
}
fn same_file_contents(a: &Path, b: &Path) -> Result<bool, String> {
    let mut a = File::open(a).map_err(|e| e.to_string())?;
    let mut b = File::open(b).map_err(|e| e.to_string())?;
    if a.metadata().map_err(|e| e.to_string())?.len()
        != b.metadata().map_err(|e| e.to_string())?.len()
    {
        return Ok(false);
    }
    let mut first = [0_u8; 65536];
    let mut second = [0_u8; 65536];
    loop {
        let count = a.read(&mut first).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(true);
        }
        b.read_exact(&mut second[..count])
            .map_err(|e| e.to_string())?;
        if first[..count] != second[..count] {
            return Ok(false);
        }
    }
}
fn restore_stage(stage: &Staged) -> Result<(), String> {
    if stage.create {
        // Only what this write created goes: a file that took the name meanwhile is another's,
        // and the failed move left it as it was.
        if !stage.committed {
            return Ok(());
        }
        return fs::remove_file(&stage.target).map_err(|e| e.to_string());
    }
    let Some(backup) = &stage.backup else {
        if !stage.target.exists() {
            return Ok(());
        }
        // A failed rename with its temporary file still present did not install
        // our new file. Preserve an unexpected competing destination for review.
        if !stage.committed && stage.temp.exists() {
            return Err("Destino inesperado apareceu durante o salvamento; arquivos preservados para revisão".into());
        }
        return fs::remove_file(&stage.target).map_err(|e| e.to_string());
    };
    if same_file_contents(&stage.target, backup) == Ok(true) {
        return Ok(());
    }
    // Recovery never consumes the sole backup. If recovery itself fails partway,
    // the original bytes remain available and cleanup is deliberately skipped.
    let recovery = temporary_path(&stage.target, "recovery");
    fs_ops::copy_new(backup, &recovery)
        .map_err(|e| format!("Não foi possível preparar restauração: {e}"))?;
    let replaced = atomic_replace(&recovery, &stage.target);
    match same_file_contents(&stage.target, backup) {
        Ok(true) => {
            let _ = fs::remove_file(&recovery);
            Ok(())
        }
        verification => Err(format!(
            "Restauração não confirmada; backup mantido em {}. Substituição: {}. Verificação: {:?}",
            backup.display(),
            replaced.err().unwrap_or_else(|| "concluída".into()),
            verification
        )),
    }
}
fn cleanup(stages: &[Staged]) {
    for stage in stages {
        let _ = fs::remove_file(&stage.temp);
        if let Some(backup) = &stage.backup {
            let _ = fs::remove_file(backup);
        }
    }
}
/// Puts `source` in place of `target`: a move that never replaces when nothing is there, and
/// ReplaceFileW, which keeps the replaced file's identity, when something is (or appears in
/// between).
#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    if !target.exists() {
        match fs_ops::move_no_replace(source, target) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            moved => return moved.map_err(|e| e.to_string()),
        }
    }
    let a = fs_ops::wide(target).map_err(|e| e.to_string())?;
    let b = fs_ops::wide(source).map_err(|e| e.to_string())?;
    // Both paths are owned, terminated UTF-16 buffers alive for this synchronous call.
    let success = unsafe {
        ReplaceFileW(
            a.as_ptr(),
            b.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
#[expect(
    clippy::disallowed_methods,
    reason = "a rename is the atomic replacement outside Windows"
)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Entity, Primitive};
    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxy-test-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn cached_bundle_is_lazy_keeps_dirty_on_failure_and_reopens_real_pixels() {
        let dir = temp();
        let path = dir.join(PROJECT_FILE);
        let mut project = Project::new("Cache");
        let mut cache = TextureCache::new(1);
        for name in ["paint", "clean"] {
            let id = new_id();
            project.assets.push(Asset {
                id: id.clone(),
                name: name.into(),
                path: format!("{name}.png"),
                kind: AssetKind::Texture,
                model: None,
            });
            cache.insert(id, PaintImage::new(256, 256, [255; 4]).unwrap());
        }
        save_cached_bundle(&path, &project, &mut cache).unwrap();
        assert!(!cache.has_dirty());
        assert_eq!(cache.len(), 0);
        let reopened = load_project_lazy(&path).unwrap();
        cache.configure(&dir, &reopened);
        assert_eq!(cache.decode_count(), 0);
        let painted = project.assets[0].id.clone();
        let before_manifest = fs::read(&path).unwrap();
        let before_png = fs::read(dir.join("paint.png")).unwrap();
        let clean_png = fs::read(dir.join("clean.png")).unwrap();
        cache.ensure(&painted).unwrap();
        cache
            .get_mut(&painted)
            .unwrap()
            .brush([50., 50.], 4., [255, 0, 0, 255]);
        // A missing file is no reason to refuse a save; one that cannot be written is.
        fs::create_dir(dir.join("bloqueada.png")).unwrap();
        let blocked = new_id();
        project.assets.push(Asset {
            id: blocked.clone(),
            name: "Bloqueada".into(),
            path: "bloqueada.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        cache.insert(blocked.clone(), PaintImage::new(4, 4, [0; 4]).unwrap());
        assert!(save_cached_bundle(&path, &project, &mut cache).is_err());
        assert!(cache.has_dirty());
        assert_eq!(cache.evict_clean(), 0);
        assert_eq!(fs::read(&path).unwrap(), before_manifest);
        assert_eq!(fs::read(dir.join("paint.png")).unwrap(), before_png);
        project.assets.pop();
        cache.remove(&blocked);
        save_cached_bundle(&path, &project, &mut cache).unwrap();
        assert!(!cache.has_dirty());
        assert!(cache.is_empty());
        assert_eq!(fs::read(dir.join("clean.png")).unwrap(), clean_png);
        assert_eq!(load_project(&path).unwrap(), project);
        cache.ensure(&painted).unwrap();
        assert_eq!(cache[&painted].pixel(50, 50), [255, 0, 0, 255]);
        assert_eq!(cache.decode_count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn cached_save_refuses_to_discard_orphan_dirty_pixels() {
        let dir = temp();
        let mut cache = TextureCache::new(1);
        let id = new_id();
        cache.insert(id.clone(), PaintImage::new(16, 16, [255; 4]).unwrap());
        assert!(
            save_cached_bundle(&dir.join(PROJECT_FILE), &Project::new("Orphan"), &mut cache)
                .is_err()
        );
        assert!(cache.is_dirty(&id));
        assert!(cache.contains_key(&id));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn save_load_and_invalid_version_preserves_document() {
        let dir = temp();
        let path = dir.join(PROJECT_FILE);
        let p = Project::new("Teste");
        save_project(&path, &p).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut bad = p.clone();
        bad.schema_version = 999;
        assert!(save_project(&path, &bad).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(load_project(&path).unwrap(), p);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn game_settings_are_read_without_loading_the_project() {
        let dir = temp();
        let path = dir.join(PROJECT_FILE);
        let mut project = Project::new("Corrida");
        save_project(&path, &project).unwrap();
        assert_eq!(
            read_game_settings(&dir),
            Some(("Corrida".to_owned(), GameSettings::default()))
        );
        project.game = GameSettings {
            title: "Corrida Noturna".into(),
            window: [800, 600],
            fullscreen: true,
        };
        save_project(&path, &project).unwrap();
        assert_eq!(
            read_game_settings(&path),
            Some(("Corrida Noturna".to_owned(), project.game.clone()))
        );
        // A project that would not load still opens its window; settings out of range or a
        // document that cannot be read give the defaults.
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["scenes"] = serde_json::json!([]);
        json["game"]["window"] = serde_json::json!([10, 10]);
        fs::write(&path, json.to_string()).unwrap();
        assert!(load_project(&path).is_err());
        assert_eq!(
            read_game_settings(&path),
            Some(("Corrida".to_owned(), GameSettings::default()))
        );
        fs::write(&path, b"{ quebrado").unwrap();
        assert_eq!(read_game_settings(&path), None);
        assert_eq!(read_game_settings(&dir.join("ausente")), None);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn imported_png_and_paint_link_survive() {
        let dir = temp();
        let origin = dir.join("source.png");
        let mut image = PaintImage::new(32, 32, [255; 4]).unwrap();
        image.save(&origin).unwrap();
        let original = fs::read(&origin).unwrap();
        let mut p = Project::new("paint");
        let id = import_asset(&mut p, &dir, &origin, AssetKind::Texture).unwrap();
        let mut e = Entity::new("piece", Some(Primitive::Cube));
        e.material.texture = Some(id.clone());
        let entity_id = e.id.clone();
        p.scenes[0].entities.push(e);
        image.brush([8., 8.], 3., [255, 0, 0, 255]);
        save_bundle(
            &dir.join(PROJECT_FILE),
            &p,
            &HashMap::from([(id.clone(), image.clone())]),
        )
        .unwrap();
        let q = load_project(&dir).unwrap();
        assert_eq!(
            q.scenes[0].entity(&entity_id).unwrap().material.texture,
            Some(id.clone())
        );
        assert_eq!(load_paint_images(&q, &dir).unwrap()[&id], image);
        assert_eq!(fs::read(origin).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn staging_failure_does_not_replace_previous_files() {
        let dir = temp();
        let existing = dir.join("data.txt");
        safe_write(&existing, b"original").unwrap();
        let invalid = dir.join("folder");
        fs::create_dir(&invalid).unwrap();
        assert!(
            transaction_write(vec![
                FileWrite::replace(existing.clone(), b"new".to_vec()),
                FileWrite::replace(invalid, b"bad".to_vec())
            ])
            .is_err()
        );
        assert_eq!(fs::read(existing).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn replacement_writes_new_content_after_backup_sync() {
        let dir = temp();
        let path = dir.join("replace.txt");
        safe_write(&path, b"old").unwrap();
        safe_write(&path, b"new").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn locked_second_file_rolls_back_first_file() {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let dir = temp();
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        safe_write(&a, b"first original").unwrap();
        safe_write(&b, b"second original").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&b)
            .unwrap();
        let result = transaction_write(vec![
            FileWrite::replace(a.clone(), b"first changed".to_vec()),
            FileWrite::replace(b.clone(), b"second changed".to_vec()),
        ]);
        assert!(result.is_err());
        assert_eq!(fs::read(a).unwrap(), b"first original");
        assert_eq!(fs::read(&b).unwrap(), b"second original");
        drop(lock);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn partially_failed_replace_restores_current_and_previous_files() {
        let dir = temp();
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        safe_write(&first, b"first original").unwrap();
        safe_write(&second, b"second original").unwrap();
        let mut attempt = 0;
        let result = transaction_write_with(
            vec![
                FileWrite::replace(first.clone(), b"first new".to_vec()),
                FileWrite::replace(second.clone(), b"second new".to_vec()),
            ],
            |source, target, mode| {
                attempt += 1;
                if attempt == 2 {
                    // Reproduce the documented ReplaceFileW error 1176 contract:
                    // failure is returned after the original pathname is removed.
                    fs::remove_file(target).unwrap();
                    Err(InstallError::Failed(
                        "Falha parcial simulada após remover o original (1176)".into(),
                    ))
                } else {
                    install(source, target, mode)
                }
            },
        );
        assert!(matches!(
            result.unwrap_err(),
            PersistenceError::SaveRolledBack(_)
        ));
        assert_eq!(fs::read(&first).unwrap(), b"first original");
        assert_eq!(fs::read(&second).unwrap(), b"second original");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn failed_recovery_keeps_the_original_backup() {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        let dir = temp();
        let path = dir.join("locked.txt");
        safe_write(&path, b"irreplaceable original").unwrap();
        let mut lock = None;
        let result = transaction_write_with(
            vec![FileWrite::replace(path.clone(), b"new".to_vec())],
            |_, target, _| {
                fs::write(target, b"partially replaced").unwrap();
                lock = Some(
                    OpenOptions::new()
                        .read(true)
                        .share_mode(0)
                        .open(target)
                        .unwrap(),
                );
                Err(InstallError::Failed(
                    "Falha parcial simulada com bloqueio durante recuperação".into(),
                ))
            },
        );
        let PersistenceError::SaveRecoveryKept { restore, .. } = result.unwrap_err() else {
            panic!("A recuperação falha deve manter as cópias");
        };
        assert!(restore.contains("backup mantido"));
        drop(lock);
        let backup = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".backup.")
            })
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"irreplaceable original");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn created_files_never_replace_one_that_appears_meanwhile() {
        let dir = temp();
        let mine = dir.join("Pulo.wav");
        let theirs = dir.join("Torre.modelo.json");
        // Taken before anything is written: nothing is.
        fs::write(&theirs, b"de um agente").unwrap();
        let error = transaction_write(vec![
            FileWrite::create(mine.clone(), b"meu".to_vec()),
            FileWrite::create(theirs.clone(), b"da OXY".to_vec()),
        ])
        .unwrap_err();
        assert!(matches!(&error, PersistenceError::TargetExists(path) if *path == theirs));
        assert!(error.to_string().contains("não grava por cima"), "{error}");
        assert!(!mine.exists());
        fs::remove_file(&theirs).unwrap();
        // Taken between writing and putting in place: the same error, the file that appeared
        // stays, and the one already created goes with the rollback.
        let result = transaction_write_with(
            vec![
                FileWrite::create(mine.clone(), b"meu".to_vec()),
                FileWrite::create(theirs.clone(), b"da OXY".to_vec()),
            ],
            |source, target, mode| {
                if target == theirs {
                    fs::write(target, b"de um agente").unwrap();
                }
                install(source, target, mode)
            },
        );
        let error = result.unwrap_err();
        assert!(matches!(&error, PersistenceError::TargetExists(path) if *path == theirs));
        assert!(error.to_string().contains("não grava por cima"), "{error}");
        assert_eq!(fs::read(&theirs).unwrap(), b"de um agente");
        assert!(!mine.exists());
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary file stays"
        );
        // A file to replace with nothing expected (the project file, OXY's own) is replaced
        // even when it appears in between.
        let manifest = dir.join(PROJECT_FILE);
        transaction_write_with(
            vec![FileWrite::replace(manifest.clone(), b"novo".to_vec())],
            |source, target, mode| {
                fs::write(target, b"apareceu").unwrap();
                install(source, target, mode)
            },
        )
        .unwrap();
        assert_eq!(fs::read(&manifest).unwrap(), b"novo");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn replacing_an_expected_file_refuses_one_changed_outside() {
        let dir = temp();
        let model = dir.join("Checkpoint.modelo.json");
        safe_write(&model, b"lido pela OXY").unwrap();
        let read: Arc<[u8]> = fs::read(&model).unwrap().into();
        let stamp = Stamp::read(&model).unwrap();
        let expecting = |expected: Expected, bytes: &[u8]| FileWrite {
            target: model.clone(),
            bytes: bytes.to_vec(),
            mode: WriteMode::Replace {
                expected: Some(expected),
            },
        };
        transaction_write(vec![expecting(Expected::Bytes(read.clone()), b"salvo 1")]).unwrap();
        assert_eq!(fs::read(&model).unwrap(), b"salvo 1");
        // Changed since it was read: refused by bytes and by stamp.
        for expected in [Expected::Bytes(read), Expected::Stamp(stamp)] {
            let error = transaction_write(vec![expecting(expected, b"salvo 2")]).unwrap_err();
            assert!(
                matches!(error, PersistenceError::TargetChanged(_)),
                "{error}"
            );
            assert!(error.to_string().contains("mudou fora da OXY"), "{error}");
            assert_eq!(fs::read(&model).unwrap(), b"salvo 1");
        }
        let stamp = Stamp::read(&model).unwrap();
        transaction_write(vec![expecting(Expected::Stamp(stamp), b"salvo 2")]).unwrap();
        assert_eq!(fs::read(&model).unwrap(), b"salvo 2");
        // Gone: nothing is created in its place.
        fs::remove_file(&model).unwrap();
        let error = transaction_write(vec![expecting(
            Expected::Bytes(b"salvo 2".as_slice().into()),
            b"salvo 3",
        )])
        .unwrap_err();
        assert!(
            matches!(error, PersistenceError::TargetChanged(_)),
            "{error}"
        );
        assert!(!model.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_file_changed_outside_while_the_others_were_written_is_not_replaced() {
        let dir = temp();
        let manifest = dir.join(PROJECT_FILE);
        let new = dir.join("Novo.modelo.json");
        let script = dir.join("porta.luau");
        safe_write(&manifest, b"manifesto antigo").unwrap();
        safe_write(&script, b"lido pela OXY").unwrap();
        let result = transaction_write_with(
            vec![
                FileWrite::replace(manifest.clone(), b"manifesto novo".to_vec()),
                FileWrite::create(new.clone(), b"modelo".to_vec()),
                FileWrite {
                    target: script.clone(),
                    bytes: b"salvo pela OXY".to_vec(),
                    mode: WriteMode::Replace {
                        expected: Some(Expected::Bytes(b"lido pela OXY".as_slice().into())),
                    },
                },
            ],
            |source, target, mode| {
                if target == script {
                    // The editor of the person saves it right then.
                    fs::write(target, b"salvo no VS Code").unwrap();
                }
                install(source, target, mode)
            },
        );
        let error = result.unwrap_err();
        assert!(matches!(&error, PersistenceError::TargetChanged(path) if *path == script));
        assert_eq!(fs::read(&script).unwrap(), b"salvo no VS Code");
        assert_eq!(fs::read(&manifest).unwrap(), b"manifesto antigo");
        assert!(!new.exists());
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            2,
            "no temporary file or copy stays"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn imports_and_texture_copies_only_create_their_files() {
        let dir = temp();
        let origin = dir.join("origem.png");
        PaintImage::new(8, 8, [255; 4])
            .unwrap()
            .save(&origin)
            .unwrap();
        let mut project = Project::new("Importar");
        let id = import_asset(&mut project, &dir, &origin, AssetKind::Texture).unwrap();
        let path = project.asset(&id).unwrap().path.clone();
        assert_eq!(path, format!("assets/{id}.png"));
        assert_eq!(
            fs::read(dir.join(&path)).unwrap(),
            fs::read(&origin).unwrap()
        );
        let copy = copy_texture(&mut project, &dir, &id, None).unwrap();
        assert_ne!(copy, id);
        assert_eq!(
            fs::read(dir.join(&project.asset(&copy).unwrap().path)).unwrap(),
            fs::read(&origin).unwrap()
        );
        assert_eq!(fs::read_dir(dir.join("assets")).unwrap().count(), 2);
        // A file already at the name the asset gets stays, and no asset is added.
        let before = project.clone();
        let sound = new_id();
        let taken = dir.join("assets").join(format!("{sound}.wav"));
        fs::write(&taken, b"de outra pessoa").unwrap();
        let error = store_asset_as(
            &mut project,
            &dir,
            sound,
            b"RIFF",
            "Pulo".into(),
            AssetKind::Audio,
            "",
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(
            matches!(&error, PersistenceError::TargetExists(path) if path.ends_with(taken.file_name().unwrap())),
            "{error}"
        );
        assert_eq!(fs::read(&taken).unwrap(), b"de outra pessoa");
        let image = new_id();
        let taken = dir.join("assets").join(format!("{image}.png"));
        fs::write(&taken, b"de outra pessoa").unwrap();
        let error = copy_texture_as(&mut project, &dir, &id, None, image).unwrap_err();
        assert!(
            matches!(error, PersistenceError::TargetExists(_)),
            "{error}"
        );
        assert_eq!(fs::read(&taken).unwrap(), b"de outra pessoa");
        assert_eq!(project, before);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn new_files_of_the_tree_layout_take_the_names_the_library_shows() {
        let dir = temp();
        let origin = dir.join("origem").join("Pulo.png");
        fs::create_dir_all(origin.parent().unwrap()).unwrap();
        PaintImage::new(4, 4, [255; 4])
            .unwrap()
            .save(&origin)
            .unwrap();
        let root = dir.join("Projeto");
        let mut project = Project::new("Árvore");
        project.layout = Layout::Tree;
        let first = import_asset(&mut project, &root, &origin, AssetKind::Texture).unwrap();
        let second = import_asset(&mut project, &root, &origin, AssetKind::Texture).unwrap();
        let copy = copy_texture(&mut project, &root, &first, None).unwrap();
        let shown: Vec<(&str, &str)> = [&first, &second, &copy]
            .iter()
            .map(|id| {
                let asset = project.asset(id).unwrap();
                (asset.name.as_str(), asset.path.as_str())
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("Pulo", "Pulo.png"),
                ("Pulo 2", "Pulo 2.png"),
                ("Pulo (cópia)", "Pulo (cópia).png")
            ]
        );
        for path in ["Pulo.png", "Pulo 2.png", "Pulo (cópia).png"] {
            assert_eq!(
                fs::read(root.join(path)).unwrap(),
                fs::read(&origin).unwrap()
            );
        }
        // A file anyone put at the name is passed over; a sound may share an image's name.
        fs::write(root.join("Pulo 3.png"), b"de outra pessoa").unwrap();
        assert_eq!(
            new_asset_file(&project, Some(&root), "x", "Pulo", AssetKind::Texture),
            ("Pulo 4".into(), "Pulo 4.png".into())
        );
        assert_eq!(
            new_asset_file(&project, Some(&root), "x", "Pulo", AssetKind::Audio),
            ("Pulo".into(), "Pulo.wav".into())
        );
        // Names Windows would not keep are made fit, and the number always fits.
        assert_eq!(
            new_asset_file(&project, None, "x", "Clique: menu?", AssetKind::Audio),
            ("Clique menu".into(), "Clique menu.wav".into())
        );
        let long = "a".repeat(crate::names::MAX_NAME_CHARS);
        project.assets.push(Asset {
            id: new_id(),
            name: long.clone(),
            path: format!("{long}.wav"),
            kind: AssetKind::Audio,
            model: None,
        });
        let (name, _) = new_asset_file(&project, None, "x", &long, AssetKind::Audio);
        assert_eq!(
            name,
            format!("{} 2", "a".repeat(crate::names::MAX_NAME_CHARS - 2))
        );
        // The old layout keeps assets/<id>.
        project.layout = Layout::Legacy;
        assert_eq!(
            new_asset_file(&project, Some(&root), "x", "Pulo", AssetKind::Texture),
            ("Pulo".into(), "assets/x.png".into())
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn copies_skip_identical_files_and_refuse_others() {
        let dir = temp();
        let target = dir.join("assets").join("x.png");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let conflict = || PersistenceError::DestinationConflict("assets/x.png".into());
        assert_eq!(
            copy_write(target.clone(), b"png".to_vec(), conflict).unwrap(),
            Some(FileWrite::create(target.clone(), b"png".to_vec()))
        );
        fs::write(&target, b"png").unwrap();
        assert_eq!(
            copy_write(target.clone(), b"png".to_vec(), conflict).unwrap(),
            None
        );
        assert!(matches!(
            copy_write(target.clone(), b"outro".to_vec(), conflict),
            Err(PersistenceError::DestinationConflict(_))
        ));
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn writes_reach_past_260_characters() {
        let dir = temp();
        let mut deep = dir.clone();
        while deep.as_os_str().len() < 300 {
            deep.push("pasta-com-nome-comprido");
        }
        fs::create_dir_all(&deep).unwrap();
        let file = deep.join("arquivo.txt");
        safe_write(&file, b"um").unwrap();
        safe_write(&file, b"dois").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"dois");
        transaction_write(vec![FileWrite::create(
            deep.join("novo.txt"),
            b"tres".to_vec(),
        )])
        .unwrap();
        assert_eq!(fs::read_dir(&deep).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn broken_asset_rejected_without_replacing_caller_state() {
        let dir = temp();
        let p = Project::new("valid");
        save_project(&dir.join(PROJECT_FILE), &p).unwrap();
        let mut q = p.clone();
        q.assets.push(Asset {
            id: new_id(),
            name: "missing".into(),
            path: "assets/missing.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        safe_write(&dir.join(PROJECT_FILE), &serde_json::to_vec(&q).unwrap()).unwrap();
        assert!(load_project(&dir).is_err());
        assert_eq!(p.name, "valid");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn painted_model_animation_and_instanced_graph_survive_bundle() {
        use crate::{
            animation::{AnimationEvent, Clip, Interpolation, Keyframe},
            document::Value,
            graph::Node,
        };
        let dir = temp();
        let mut project = Project::new("Fluxo integrado");
        let scene_id = project.scenes[0].id.clone();
        let texture = new_id();
        project.assets.push(Asset {
            id: texture.clone(),
            name: "Pintura".into(),
            path: format!("assets/{texture}.png"),
            kind: AssetKind::Texture,
            model: None,
        });
        let mut actor = Entity::new("Montagem", None);
        let mut arm = Entity::new("Braço", Some(Primitive::Cube));
        arm.parent = Some(actor.id.clone());
        arm.transform.pivot = [0., 0.5, 0.];
        arm.material.texture = Some(texture.clone());
        let mut clip = Clip::new("Ataque");
        clip.insert_key(
            &arm.id,
            Keyframe {
                time: 0.,
                transform: arm.transform.clone(),
                interpolation: Interpolation::Linear,
            },
        );
        let mut raised = arm.transform.clone();
        raised.rotation[0] = 1.;
        clip.insert_key(
            &arm.id,
            Keyframe {
                time: 0.5,
                transform: raised,
                interpolation: Interpolation::Linear,
            },
        );
        clip.events.push(AnimationEvent {
            time: 0.4,
            name: "Impacto".into(),
        });
        let mut play = Node::new("action.animation", [200., 100.]);
        play.params
            .insert("clip".into(), Value::Text(clip.id.clone()));
        actor.graph.nodes.push(play);
        actor.clips.push(clip);
        let actor_id = actor.id.clone();
        project.scenes[0].entities = vec![actor, arm];
        let model = project
            .save_model(&scene_id, &actor_id, "Peças editáveis")
            .unwrap();
        let instance = project.instantiate_model(&model, &scene_id).unwrap();
        let mut pixels = PaintImage::new(256, 256, [255; 4]).unwrap();
        pixels.stroke([20., 20.], [200., 200.], 12., [35, 170, 95, 255]);
        let images = HashMap::from([(texture.clone(), pixels.clone())]);
        save_bundle(&dir.join(PROJECT_FILE), &project, &images).unwrap();
        let restored = load_project(&dir).unwrap();
        let restored_images = load_paint_images(&restored, &dir).unwrap();
        assert_eq!(restored_images[&texture], pixels);
        let scene = restored.scene(&scene_id).unwrap();
        let root = scene.entity(&instance).unwrap();
        let clip = &root.clips[0];
        assert_eq!(root.graph.nodes[0].text("clip"), clip.id);
        let child = scene.entity(&clip.tracks[0].target).unwrap();
        assert_eq!(child.parent.as_deref(), Some(instance.as_str()));
        assert_eq!(child.material.texture, Some(texture));
        assert_eq!(child.transform.pivot, [0., 0.5, 0.]);
        assert_eq!(clip.events[0].name, "Impacto");
        assert_eq!(
            restored
                .asset(&model)
                .unwrap()
                .model
                .as_ref()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(restored, project);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn new_projects_go_only_to_new_or_empty_folders_and_leave_nothing_when_refused() {
        use crate::document::SceneKind;
        let dir = temp();
        let engine = dir.join("OXY Engine");
        fs::create_dir_all(&engine).unwrap();
        let protected = [engine.clone()];
        let project = crate::editing::blank_project("Novo", SceneKind::ThreeD).unwrap();
        let occupied = dir.join("Ocupada");
        fs::create_dir_all(&occupied).unwrap();
        fs::write(occupied.join("nota.txt"), b"de outra pessoa").unwrap();
        let existing = dir.join("Existente");
        save_project(&existing.join(PROJECT_FILE), &Project::new("Antigo")).unwrap();
        let listing = || {
            let mut names: Vec<_> = walk(&dir);
            names.sort();
            names
        };
        let before = listing();
        for (folder, expected) in [
            (PathBuf::from("Projetos"), "caminho completo da pasta"),
            (PathBuf::from(r"\\servidor\projetos"), "rede"),
            (PathBuf::from(r"C:\"), "raiz de um disco"),
            (PathBuf::from(r"C:\Projetos\.."), "raiz de um disco"),
            (engine.join("Novo"), "pasta da OXY Engine"),
            (dir.clone(), "pasta da OXY Engine"),
            (occupied.clone(), "outros arquivos"),
            (occupied.join("nota.txt"), "não é uma pasta"),
            (existing.clone(), "Já existe um projeto"),
            (existing.join(PROJECT_FILE), "Já existe um projeto"),
            (
                dir.join("Novo").join(PROJECT_FILE),
                "não o arquivo project.oxy.json",
            ),
            (dir.join("Novo").join("Project.OXY.json"), "não o arquivo"),
        ] {
            let error = create_project(&folder, &project, &protected)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{folder:?}: {error}");
        }
        // An invalid project is refused before its folder exists, and a save that fails takes
        // the folders made for it away.
        let mut invalid = project.clone();
        invalid.start_scene = "nenhuma".into();
        assert!(create_project(&dir.join("Inválido").join("Fundo"), &invalid, &protected).is_err());
        let mut missing = project.clone();
        missing.assets.push(Asset {
            id: new_id(),
            name: "Ausente".into(),
            path: "assets/ausente.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        let error = create_project(&dir.join("Falha").join("Fundo"), &missing, &protected)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Ausente"), "{error}");
        assert_eq!(listing(), before);
        // New folders are made; an empty one is taken, `..` worked out.
        let file = create_project(&dir.join("Jogos").join("Novo"), &project, &protected).unwrap();
        assert_eq!(file, dir.join("Jogos").join("Novo").join(PROJECT_FILE));
        assert_eq!(load_project(&file).unwrap(), project);
        let empty = dir.join("Vazia");
        fs::create_dir(&empty).unwrap();
        let file = create_project(&empty.join("x").join(".."), &project, &protected).unwrap();
        assert_eq!(file, empty.join(PROJECT_FILE));
        assert_eq!(
            fs::read_dir(&empty).unwrap().count(),
            1,
            "the project file alone"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    /// Every file and folder under `root`, relative to it.
    fn walk(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                found.extend(walk(&path));
            }
            found.push(path);
        }
        found
    }
}
