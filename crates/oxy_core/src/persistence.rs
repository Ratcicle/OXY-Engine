//! Validated documents and staged, recoverable file replacement.
use crate::{
    document::{Asset, AssetKind, GameSettings, Id, Project, new_id, validate_project},
    migration::MigrationError,
    painting::PaintImage,
    script::{self, ScriptFile, ScriptSources},
    texture_cache::TextureCache,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
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
    /// An object attaches a script the project's `scripts` folder lacks.
    #[error("{object}: o script {path} não existe na pasta do projeto")]
    ScriptMissing { object: String, path: String },
    #[error("O script {0} tem mais de 1 MB")]
    ScriptTooLarge(String),
    #[error("O script {0} não é texto UTF-8")]
    ScriptNotText(String),
    #[error("Falha ao ler o script {path}: {source}")]
    ScriptRead { path: String, source: io::Error },
    #[error("Nome de arquivo inválido na pasta scripts: {0}")]
    ScriptName(String),
    /// A copy would replace a different file of the `scripts` folder where it goes.
    #[error("A pasta escolhida já contém um arquivo diferente: {0}. Escolha uma pasta vazia.")]
    ScriptConflict(String),
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
fn load_project_with(path: &Path, lazy: bool) -> Result<(Project, Option<u32>), PersistenceError> {
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let meta = fs::metadata(&path).map_err(|source| PersistenceError::Open {
        path: path.clone(),
        source,
    })?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err(PersistenceError::DocumentTooLarge);
    }
    let bytes = fs::read(&path).map_err(io_context("Falha ao ler projeto"))?;
    let (project, migrated) = crate::migration::read_report(&bytes)?;
    validate_project(&project).map_err(PersistenceError::InvalidProject)?;
    let root = path.parent().unwrap_or(Path::new("."));
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
    validate_project(project).map_err(PersistenceError::InvalidProject)?;
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    validate_asset_files(project, root)?;
    let mut writes: Vec<_> = crate::migration::backup(&path)?.into_iter().collect();
    writes.push((path, serde_json::to_vec_pretty(project)?));
    transaction_write(writes)
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
    let saved = fs::create_dir_all(&folder)
        .map_err(io_context("Não foi possível criar pasta do projeto"))
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
    let mut writes = Vec::new();
    for asset in &project.assets {
        if asset.kind == AssetKind::Model {
            continue;
        }
        let target = resolve_asset_path(root, &asset.path)?;
        if let Some(image) = images.get(&asset.id) {
            if asset.kind != AssetKind::Texture {
                return Err(PersistenceError::PixelsOnNonTexture(asset.name.clone()));
            }
            writes.push((target, image.to_png().map_err(PersistenceError::Image)?));
        } else {
            validate_asset_file(asset, &target)?;
        }
    }
    let bytes = serde_json::to_vec_pretty(project)?;
    writes.extend(crate::migration::backup(&path)?);
    writes.push((path, bytes));
    transaction_write(writes)
}
/// Writes dirty texture buffers and the manifest in the same recoverable transaction.
/// Clean texture files are checked without decoding or copying their pixel buffers.
/// Dirty cache entries stay pinned if any staging/replacement step fails.
pub fn save_cached_bundle(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
) -> Result<(), PersistenceError> {
    save_cached_bundle_from(path, project, images, None)
}

/// Save an independent copy, including lazily loaded PNG/WAV resources and every file of the
/// `scripts` folder. Source files are only read. All destination writes share one transaction.
pub fn save_cached_copy(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: &Path,
) -> Result<(), PersistenceError> {
    save_cached_bundle_from(path, project, images, Some(source_root))
}

fn save_cached_bundle_from(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
    source_root: Option<&Path>,
) -> Result<(), PersistenceError> {
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
    let mut writes = Vec::new();
    for asset in &project.assets {
        if asset.kind == AssetKind::Model {
            continue;
        }
        let target = resolve_asset_path(root, &asset.path)?;
        if images.is_dirty(&asset.id) {
            if asset.kind != AssetKind::Texture {
                return Err(PersistenceError::PixelsOnNonTexture(asset.name.clone()));
            }
            let image = images
                .get(&asset.id)
                .ok_or(PersistenceError::DirtyTextureNotResident)?;
            let bytes = image.to_png().map_err(PersistenceError::Image)?;
            if source_root.is_some() && target.exists() && fs::read(&target)? != bytes {
                return Err(PersistenceError::DestinationConflict(asset.path.clone()));
            }
            writes.push((target, bytes));
        } else if let Some(source_root) = source_root {
            let source = resolve_asset_path(source_root, &asset.path)?;
            validate_asset_header(asset, &source)?;
            let bytes = fs::read(&source).map_err(|source| PersistenceError::CopySource {
                name: asset.name.clone(),
                source,
            })?;
            if target.exists() && fs::read(&target)? != bytes {
                return Err(PersistenceError::DestinationConflict(asset.path.clone()));
            }
            writes.push((target, bytes));
        } else {
            validate_asset_header(asset, &target)?;
        }
    }
    if let Some(source_root) = source_root {
        // The whole folder goes as it is, attached or not: a script may require any other.
        for (path, source) in scripts_folder_files(source_root)? {
            let bytes = fs::read(&source).map_err(|source| PersistenceError::CopySource {
                name: path.clone(),
                source,
            })?;
            let target = resolve_asset_path(root, &path)?;
            if target.exists() && fs::read(&target)? != bytes {
                return Err(PersistenceError::ScriptConflict(path));
            }
            writes.push((target, bytes));
        }
    }
    writes.extend(crate::migration::backup(&path)?);
    writes.push((path.clone(), serde_json::to_vec_pretty(project)?));
    transaction_write(writes)?;
    images.configure(root, project);
    images.mark_saved();
    Ok(())
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
    store_asset(project, root, &bytes, name, kind)
}
/// Writes PNG or WAV bytes, already checked, to the project's assets folder under a new name
/// and adds the asset as `name`.
pub(crate) fn store_asset(
    project: &mut Project,
    root: &Path,
    bytes: &[u8],
    name: String,
    kind: AssetKind,
) -> Result<Id, PersistenceError> {
    let id = new_id();
    let extension = if kind == AssetKind::Texture {
        "png"
    } else {
        "wav"
    };
    let relative = format!("assets/{id}.{extension}");
    fs::create_dir_all(root)?;
    let target = resolve_asset_path(root, &relative)?;
    safe_write(&target, bytes)?;
    project.assets.push(Asset {
        id: id.clone(),
        name,
        path: relative,
        kind,
        model: None,
    });
    Ok(id)
}
/// Creates an independent texture; neither the original pixels nor its reference changes.
pub fn copy_texture(
    project: &mut Project,
    root: &Path,
    asset_id: &str,
    image: Option<&PaintImage>,
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
    let id = new_id();
    let path = format!("assets/{id}.png");
    safe_write(&resolve_asset_path(root, &path)?, &bytes)?;
    project.assets.push(Asset {
        id: id.clone(),
        name: format!("{} (cópia)", asset.name),
        path,
        kind: AssetKind::Texture,
        model: None,
    });
    Ok(id)
}
pub fn resolve_asset_path(root: &Path, relative: &str) -> Result<PathBuf, PersistenceError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || relative.contains(':')
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(PersistenceError::InvalidAssetPath(relative.to_owned()));
    }
    let canonical_root =
        fs::canonicalize(root).map_err(io_context("Pasta do projeto indisponível"))?;
    let result = canonical_root.join(path);
    let mut ancestor = result.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or(PersistenceError::NoExistingFolder)?;
    }
    let resolved = fs::canonicalize(ancestor)?;
    if !resolved.starts_with(&canonical_root) {
        return Err(PersistenceError::AssetOutsideProject(relative.to_owned()));
    }
    Ok(result)
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
/// Every file in the `scripts` folder of the project at `root` or in the folder's subfolders, by
/// its path from the project's folder with `/`; none without the folder. Links in it are not
/// followed, so nothing outside it is read.
fn scripts_folder_files(root: &Path) -> Result<BTreeMap<String, PathBuf>, PersistenceError> {
    let mut found = BTreeMap::new();
    let folder = resolve_asset_path(root, script::FOLDER)?;
    if !folder.is_dir() {
        return Ok(found);
    }
    let listing = || io_context("Falha ao ler a pasta scripts");
    let mut pending = vec![(folder, script::FOLDER.to_owned())];
    while let Some((folder, name)) = pending.pop() {
        for entry in fs::read_dir(&folder).map_err(listing())? {
            let entry = entry.map_err(listing())?;
            let kind = entry.file_type().map_err(listing())?;
            let file = entry.file_name();
            let Some(file) = file.to_str() else {
                return Err(PersistenceError::ScriptName(format!(
                    "{name}/{}",
                    file.to_string_lossy()
                )));
            };
            let path = format!("{name}/{file}");
            if kind.is_dir() {
                pending.push((entry.path(), path));
            } else if kind.is_file() {
                found.insert(path, entry.path());
            }
        }
    }
    Ok(found)
}
/// The script files of the project at `root`: the `.luau` files of its `scripts` folder, by path
/// (see [`crate::script`]).
fn script_files(root: &Path) -> Result<BTreeMap<String, PathBuf>, PersistenceError> {
    let mut files = scripts_folder_files(root)?;
    files.retain(|path, _| {
        Path::new(path)
            .extension()
            .is_some_and(|extension| extension == script::EXTENSION)
    });
    Ok(files)
}
/// The scripts of the project at `root`, read from its `scripts` folder (none without it): each
/// must be UTF-8 text of up to 1 MB.
pub fn load_scripts(root: &Path) -> Result<ScriptSources, PersistenceError> {
    read_scripts(root, None)
}
/// [`load_scripts`] without the script `path`, whose text a write replaces: a file of it that is
/// not UTF-8 or is over 1 MB does not stop the others from being read.
pub fn load_scripts_except(root: &Path, path: &str) -> Result<ScriptSources, PersistenceError> {
    read_scripts(root, Some(path))
}
fn read_scripts(root: &Path, skip: Option<&str>) -> Result<ScriptSources, PersistenceError> {
    let mut sources = ScriptSources::default();
    for (path, file) in script_files(root)? {
        if skip == Some(path.as_str()) {
            continue;
        }
        let read = |source| PersistenceError::ScriptRead {
            path: path.clone(),
            source,
        };
        if fs::metadata(&file).map_err(read)?.len() > script::MAX_BYTES {
            return Err(PersistenceError::ScriptTooLarge(path));
        }
        let bytes = fs::read(&file).map_err(read)?;
        let text =
            String::from_utf8(bytes).map_err(|_| PersistenceError::ScriptNotText(path.clone()))?;
        sources.files.insert(path, ScriptFile::new(text));
    }
    Ok(sources)
}
/// What tells that a script file changed without reading it: its size and when it was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptStamp {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
}

/// The stamp of each script file of the project at `root` (the files [`load_scripts`] reads),
/// by path, from the folder's listing; none without the folder. A file that cannot be looked at
/// (one that went in the meantime) is left out: reading the folder says why it fails.
pub fn script_stamps(root: &Path) -> Result<BTreeMap<String, ScriptStamp>, PersistenceError> {
    Ok(script_files(root)?
        .into_iter()
        .filter_map(|(path, file)| {
            let meta = fs::metadata(file).ok()?;
            let stamp = ScriptStamp {
                len: meta.len(),
                modified: meta.modified().ok(),
            };
            Some((path, stamp))
        })
        .collect())
}

/// The text of the script `path` of the project at `root` (`path` as [`script::normalize_path`]
/// writes it), with the checks of [`load_scripts`]; none when the file does not exist.
pub fn read_script(root: &Path, path: &str) -> Result<Option<String>, PersistenceError> {
    if script::normalize_path(path).as_deref() != Ok(path) {
        return Err(PersistenceError::InvalidAssetPath(path.to_owned()));
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

/// Where the agents' copies of script files go, from the project's folder: out of the `scripts`
/// folder, so that it holds only scripts.
pub const SCRIPT_BACKUPS: &str = ".oxy/agente";

/// The copy of a script file an agent keeps, as it was before its session first changed the
/// file, by the script's path: `scripts/porta.luau` keeps `.oxy/agente/scripts/porta.luau.bak`.
pub fn script_backup_path(path: &str) -> String {
    format!("{SCRIPT_BACKUPS}/{path}.bak")
}

/// Writes `text` as the script `path` of the project at `root` (`path` as
/// [`script::normalize_path`] writes it), creating its folders, in one transaction with, when
/// `backup` is set and the file exists, the copy of it as it was at [`script_backup_path`].
/// Answers the text the file had, if it existed.
pub fn write_script(
    root: &Path,
    path: &str,
    text: &str,
    backup: bool,
) -> Result<Option<String>, PersistenceError> {
    if script::normalize_path(path).as_deref() != Ok(path) {
        return Err(PersistenceError::InvalidAssetPath(path.to_owned()));
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
        writes.push((
            resolve_asset_path(root, &script_backup_path(path))?,
            bytes.clone(),
        ));
    }
    writes.push((target, text.as_bytes().to_vec()));
    transaction_write(writes)?;
    Ok(previous.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
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

/// Checks that every script an object attaches, in a scene or a model, is in the `scripts`
/// folder at `root`: the first one missing is the error. Opening and saving do not ask for it
/// (a script renamed or deleted outside OXY must not keep the project from opening or saving:
/// they warn, see [`script_warnings`]); playing it in the editor and exporting it do. The folder
/// is read only when some object attaches a script.
pub fn validate_script_files(project: &Project, root: &Path) -> Result<(), PersistenceError> {
    match missing_scripts(project, root)?.into_iter().next() {
        Some(missing) => Err(PersistenceError::ScriptMissing {
            object: missing.name,
            path: missing.path,
        }),
        None => Ok(()),
    }
}
/// The scripts the objects of `project` attach, in scenes and models, that the `scripts` folder
/// at `root` lacks; the folder is read only when some object attaches a script.
pub fn missing_scripts(
    project: &Project,
    root: &Path,
) -> Result<Vec<script::MissingScript>, PersistenceError> {
    if !script::any_attached(project) {
        return Ok(Vec::new());
    }
    let files = script_files(root)?;
    Ok(script::missing(project, |path| files.contains_key(path)))
}
/// What opening or saving the project at `root` warns about its scripts, one line per file:
/// every script its objects attach that the `scripts` folder lacks, by its path and the names
/// of the objects (see [`script::missing_by_file`]). The project opens and saves all the same;
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
/// scripts its objects attach that the folder at `root` lacks, but those `known` before the
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
/// the editor's test: the first file missing from the folder refuses it, with the objects that
/// attach it, as [`load_scripts`] refuses a file it cannot read. The message says what to do.
/// Without attached scripts nothing is read: none would run.
pub fn load_scripts_to_run(project: &Project, root: &Path) -> Result<ScriptSources, String> {
    if !script::any_attached(project) {
        return Ok(ScriptSources::default());
    }
    let sources = load_scripts(root).map_err(|error| error.to_string())?;
    let missing = script::missing(project, |path| sources.get(path).is_some());
    match script::missing_by_file(&missing).into_iter().next() {
        Some((line, objects)) => Err(format!("{line}; {}", missing_fix(objects))),
        None => Ok(sources),
    }
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
pub fn safe_write(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    transaction_write(vec![(path.to_owned(), bytes.to_vec())])
}

struct Staged {
    target: PathBuf,
    temp: PathBuf,
    backup: Option<PathBuf>,
    committed: bool,
}
fn temporary_path(target: &Path, label: &str) -> PathBuf {
    let file = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{file}.{label}.{}", new_id()))
}
fn transaction_write(writes: Vec<(PathBuf, Vec<u8>)>) -> Result<(), PersistenceError> {
    transaction_write_with(writes, atomic_replace)
}
fn transaction_write_with(
    writes: Vec<(PathBuf, Vec<u8>)>,
    mut replace: impl FnMut(&Path, &Path) -> Result<(), String>,
) -> Result<(), PersistenceError> {
    let mut stages: Vec<Staged> = Vec::new();
    let mut targets = std::collections::HashSet::new();
    let prepared = (|| -> Result<(), PersistenceError> {
        for (target, bytes) in writes {
            if !targets.insert(target.clone()) {
                return Err(PersistenceError::DuplicateTarget(target));
            }
            if target.is_dir() {
                return Err(PersistenceError::TargetIsDirectory(target));
            }
            let parent = target.parent().unwrap_or(Path::new("."));
            fs::create_dir_all(parent).map_err(io_context("Não foi possível criar pasta"))?;
            let temp = temporary_path(&target, "tmp");
            let backup = if target.exists() {
                Some(temporary_path(&target, "backup"))
            } else {
                None
            };
            stages.push(Staged {
                target: target.clone(),
                temp: temp.clone(),
                backup: backup.clone(),
                committed: false,
            });
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(io_context("Falha criando arquivo temporário"))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(io_context("Falha gravando arquivo temporário"))?;
            drop(file);
            if let Some(backup) = backup {
                fs::copy(&target, &backup)
                    .map_err(io_context("Falha preservando arquivo anterior"))?;
                OpenOptions::new()
                    .write(true)
                    .open(&backup)
                    .and_then(|f| f.sync_all())
                    .map_err(io_context("Falha confirmando cópia anterior"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        cleanup(&stages);
        return Err(error);
    }
    for index in 0..stages.len() {
        if let Err(error) = replace(&stages[index].temp, &stages[index].target) {
            let mut rollback_errors = Vec::new();
            // ReplaceFileW can report an error after removing/renaming the original.
            // Restore the failing stage too; its committed flag cannot prove safety.
            for previous in stages[..=index].iter().rev() {
                if let Err(e) = restore_stage(previous) {
                    rollback_errors.push(format!("{}: {e}", previous.target.display()));
                }
            }
            if rollback_errors.is_empty() {
                cleanup(&stages);
                return Err(PersistenceError::SaveRolledBack(error));
            }
            return Err(PersistenceError::SaveRecoveryKept {
                error,
                restore: rollback_errors.join("; "),
            });
        }
        stages[index].committed = true;
    }
    cleanup(&stages);
    Ok(())
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
    fs::copy(backup, &recovery)
        .map_err(|e| format!("Não foi possível preparar restauração: {e}"))?;
    OpenOptions::new()
        .write(true)
        .open(&recovery)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("Não foi possível confirmar cópia de restauração: {e}"))?;
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
#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
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
        return fs::rename(source, target).map_err(|e| e.to_string());
    }
    let a: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
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
        project.assets.push(Asset {
            id: new_id(),
            name: "Ausente".into(),
            path: "absent.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        assert!(save_cached_bundle(&path, &project, &mut cache).is_err());
        assert!(cache.has_dirty());
        assert_eq!(cache.evict_clean(), 0);
        assert_eq!(fs::read(&path).unwrap(), before_manifest);
        assert_eq!(fs::read(dir.join("paint.png")).unwrap(), before_png);
        project.assets.pop();
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
                (existing.clone(), b"new".to_vec()),
                (invalid, b"bad".to_vec())
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
        use std::os::windows::fs::OpenOptionsExt;
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
            (a.clone(), b"first changed".to_vec()),
            (b.clone(), b"second changed".to_vec()),
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
                (first.clone(), b"first new".to_vec()),
                (second.clone(), b"second new".to_vec()),
            ],
            |source, target| {
                attempt += 1;
                if attempt == 2 {
                    // Reproduce the documented ReplaceFileW error 1176 contract:
                    // failure is returned after the original pathname is removed.
                    fs::remove_file(target).unwrap();
                    Err("Falha parcial simulada após remover o original (1176)".into())
                } else {
                    atomic_replace(source, target)
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
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp();
        let path = dir.join("locked.txt");
        safe_write(&path, b"irreplaceable original").unwrap();
        let mut lock = None;
        let result = transaction_write_with(vec![(path.clone(), b"new".to_vec())], |_, target| {
            fs::write(target, b"partially replaced").unwrap();
            lock = Some(
                OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(target)
                    .unwrap(),
            );
            Err("Falha parcial simulada com bloqueio durante recuperação".into())
        });
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
