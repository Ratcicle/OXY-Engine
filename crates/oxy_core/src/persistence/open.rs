//! Opening a project as the editor and the agents' tools on files open it ([`open_project`]):
//! what is wrong with a file is told on the asset, and the project opens all the same. In the
//! tree layout the project's folder is the Library: each model's pieces come from its file, a
//! model moved or renamed outside OXY is found again by the ID inside it, a path the disk has in
//! another case is taken as the disk has it, and a file of a known kind the project file does not
//! list yet is registered, with an ID made from its path, so it is the same at the next opening.
//! Nothing is written, but to finish or undo an organization in folders that stopped (see
//! [`crate::conversion::resume`]): the next save writes what opening fixed.
use super::{PROJECT_FILE, PersistenceError, read_document, resolve_asset_path, script_warnings};
use crate::{
    document::{Asset, AssetKind, Entity, Id, Layout, Project, validate_model, validate_project},
    editing::asset_uses,
    fs_ops::{self, Op, Stamp},
    journal::{self, Journal},
    model_file, names,
    project_tree::{self, FileKind, Presence, Tree, TreeFile},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs, io,
    path::Path,
    sync::Arc,
};

/// What playing and exporting do while an asset with a problem is used, said after the problem.
const REFUSED: &str = "jogar e exportar ficam recusados enquanto o recurso for usado";

/// What is wrong with an asset of an open project. The project opens all the same; playing and
/// exporting refuse the asset while it is used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetState {
    /// Its file is not in the project's folder.
    Missing,
    /// Its file cannot be read now (another program holds it, no permission…): why.
    Unreadable(String),
    /// Its saved path names no file OXY reads (a name Windows reads otherwise, outside the
    /// project, another asset's path, the extension of another kind): why.
    InvalidPath(String),
    /// Its file is there but holds no image, sound or model OXY can use: why. A model in this
    /// state is unavailable (no pieces).
    Broken(String),
}

/// What OXY last read or wrote of the project's files, which saving compares the disk with:
/// each model's file by model ID (its path and bytes: a model's file is replaced only while it
/// still holds them, and one changed outside OXY is kept first), and each image's file by asset
/// ID as opening found it (its path and stamp: pixels painted on an image the project file does
/// not list yet replace its file only while it is unchanged). Also the stamp of each file OXY
/// moved, made, brought back or wrote since the project opened, which undoing and redoing check
/// (a file changed outside OXY is not moved back), and the journal of what OXY did to the files
/// since the project file was saved (see [`crate::journal`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnownFiles {
    models: BTreeMap<Id, (String, Arc<[u8]>)>,
    images: BTreeMap<Id, (String, Stamp)>,
    stamps: BTreeMap<String, Stamp>,
    journal: Journal,
}
impl KnownFiles {
    /// What OXY did to the files since the project file was saved.
    pub fn journal(&self) -> &Journal {
        &self.journal
    }
    pub(crate) fn journal_mut(&mut self) -> &mut Journal {
        &mut self.journal
    }
    /// The stamp the file `path` had when OXY last moved, made or wrote it.
    pub fn stamp(&self, path: &str) -> Option<Stamp> {
        self.stamps
            .iter()
            .find(|(known, _)| names::same_name(known, path))
            .map(|(_, stamp)| *stamp)
    }
    /// Notes the stamp the file `path` of the project at `root` has now, as OXY left it.
    pub fn note_stamp(&mut self, root: &Path, path: &str) {
        self.stamps
            .retain(|known, _| !names::same_name(known, path));
        if let Some(stamp) = names::resolve(root, path)
            .ok()
            .and_then(|file| Stamp::read(&file).ok())
        {
            self.stamps.insert(path.to_owned(), stamp);
        }
    }
    /// The project file was saved as `manifest`: the journal starts again from it.
    pub(crate) fn saved(&mut self, manifest: &[u8]) {
        self.saved_hash(journal::hash(manifest));
    }
    /// [`Self::saved`] by the project file's hash (see [`journal::hash`]).
    pub(crate) fn saved_hash(&mut self, manifest: String) {
        self.journal = Journal::new(manifest);
    }
    /// Follows `op`, done in the project at `root`: the files read and their stamps take the
    /// paths it gave them, and the journal notes it.
    pub(crate) fn follow(&mut self, root: &Path, op: &Op) {
        let rekey = |path: &mut String, from: &str, to: &str| {
            if let Some(rest) = journal::under(path, from) {
                *path = journal::join(to, rest);
            }
        };
        let (from, to) = match op {
            Op::Move { from, to } => (from, to),
            Op::Trash { from, slot, .. } => (from, slot),
            Op::Untrash { slot, to, .. } => (slot, to),
            Op::CreateDir { .. } | Op::RemoveDir { .. } => {
                self.journal.apply(op);
                return;
            }
            Op::Write { path, .. } => {
                self.note_stamp(root, path);
                self.journal.apply(op);
                return;
            }
        };
        for (path, _) in self.models.values_mut() {
            rekey(path, from, to);
        }
        for (path, _) in self.images.values_mut() {
            rekey(path, from, to);
        }
        let mut moved = false;
        self.stamps = std::mem::take(&mut self.stamps)
            .into_iter()
            .map(|(mut path, stamp)| {
                let before = path.clone();
                rekey(&mut path, from, to);
                moved |= before != path;
                (path, stamp)
            })
            .collect();
        if !moved
            && names::resolve(root, to)
                .ok()
                .and_then(|file| fs::metadata(file).ok())
                .is_some_and(|meta| meta.is_file())
        {
            self.note_stamp(root, to);
        }
        // An image made by an import: pixels painted on it replace its file while it is as made.
        if let (Op::Untrash { id: Some(id), .. }, FileKind::Image, Some(stamp)) =
            (op, FileKind::of(to), self.stamp(to))
            && !self.images.contains_key(id)
        {
            self.images.insert(id.clone(), (to.clone(), stamp));
        }
        self.journal.apply(op);
    }
    /// The path and bytes of the file of the model `id`, as OXY last read or wrote them.
    pub fn get(&self, id: &str) -> Option<(&str, &[u8])> {
        self.models
            .get(id)
            .map(|(path, bytes)| (path.as_str(), bytes.as_ref()))
    }
    /// The path and stamp of the file of the image `id`, as opening found it.
    pub fn image(&self, id: &str) -> Option<(&str, Stamp)> {
        self.images
            .get(id)
            .map(|(path, stamp)| (path.as_str(), *stamp))
    }
    pub(crate) fn note(&mut self, id: Id, path: String, bytes: Arc<[u8]>) {
        self.models.insert(id, (path, bytes));
    }
    pub(crate) fn note_image(&mut self, id: Id, path: String, stamp: Stamp) {
        self.images.insert(id, (path, stamp));
    }
    /// Keeps only the models `keep` says.
    pub(crate) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.models.retain(|id, _| keep(id));
    }
}

/// What opening a project found besides the project.
#[derive(Clone, Debug, Default)]
pub struct OpenReport {
    pub layout: Layout,
    /// The format the project file was converted from in memory, when it was older; the file
    /// stays as it was until saved.
    pub migrated: Option<u32>,
    /// What opening fixed in memory, one line each: a model found by its ID elsewhere, a path
    /// taken in the disk's case, a new file registered, an entry whose file went and that
    /// nothing used taken out. The next save writes them; they leave the project unchanged, and
    /// the next opening makes them again the same way.
    pub fixes: Vec<String>,
    /// What is wrong, one line each: assets with a problem (see [`Self::assets`]), names shared
    /// by two files of a kind, the scripts objects attach that the tree lacks.
    pub warnings: Vec<String>,
    /// Each asset with a problem, by ID.
    pub assets: BTreeMap<Id, AssetState>,
    /// What was read of the models' and images' files.
    pub known: KnownFiles,
}
impl OpenReport {
    /// Every line to tell, the fixes first.
    pub fn lines(&self) -> impl Iterator<Item = &String> {
        self.fixes.iter().chain(&self.warnings)
    }
}

/// Opens the project at `path` (its folder or its project file) as the editor and the agents'
/// tools on files open it. The project file is read and converted in memory; then, in the tree
/// layout, the project's folder is walked: each model's pieces come from its file, a model whose
/// file moved is found by the ID inside it, a path the disk has in another case is taken as the
/// disk has it, every image, sound and model the project file does not list is registered (in the
/// order of the bytes of its path, its ID made from the path until the next save; a model keeps
/// the ID in its file) and each name is its file's. In both layouts every image and sound is
/// looked for: one missing that something uses, one that cannot be read now, one whose saved
/// path is invalid, one whose content is not usable and a model that cannot be read or does not
/// fit the project stay, told in the report (see [`AssetState`]); one missing that nothing uses
/// leaves the project. Only a project file that cannot be read or whose structure is invalid
/// fails. Nothing is written, but first an organization of the project in folders that stopped
/// is finished or undone, told in the report.
pub fn open_project(path: &Path) -> Result<(Project, OpenReport), PersistenceError> {
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    let resumed = crate::conversion::resume(root);
    let bytes = read_document(&path)?;
    let (mut project, migrated) = crate::migration::read_report(&bytes)?;
    let mut report = OpenReport {
        layout: project.layout,
        migrated,
        fixes: resumed.fixes,
        warnings: resumed.warnings,
        ..OpenReport::default()
    };
    // What OXY did to the files since the project file was saved comes first: it is OXY's own
    // record, the best proof of where a file went.
    let opened = journal::opening(root, &journal::hash(&bytes));
    report.fixes.extend(opened.fixes);
    report.warnings.extend(opened.warnings);
    let trashed = journal::relink_assets(&mut project, &opened.journal);
    report.known.journal = opened.journal;
    let tree = if project.layout == Layout::Tree {
        match project_tree::walk(root) {
            Ok(tree) => Some(tree),
            Err(error) => {
                report.warnings.push(format!(
                    "{error}; a OXY não procurou arquivos novos nem movidos na pasta do projeto"
                ));
                None
            }
        }
    } else {
        None
    };
    let disk = Disk::new(root, project.layout, tree.as_ref());
    let mut opening = Opening {
        disk: &disk,
        report: &mut report,
        claimed: HashSet::new(),
        gone: Vec::new(),
        trashed: trashed.into_iter().collect(),
    };
    opening.check_paths(&mut project);
    let models = disk.read_models();
    if project.layout == Layout::Tree {
        opening.load_models(&mut project, &models);
    }
    journal::relink_scripts(&mut project, &opening.report.known.journal);
    opening.check_files(&mut project);
    let gone = std::mem::take(&mut opening.gone);
    project.assets.retain(|asset| !gone.contains(&asset.id));
    if project.layout == Layout::Tree {
        opening.register(&mut project, &models);
        name_by_files(&mut project, opening.report);
        opening.fit_models(&mut project);
    }
    validate_project(&project).map_err(PersistenceError::InvalidProject)?;
    report.warnings.extend(script_warnings(&project, root));
    Ok((project, report))
}

/// Walks again the tree of the project whose project file is `file`, and tells what changed for
/// `project` since the walk `before` (see [`project_tree::reconcile`]): the IDs inside the new
/// model files are read, and the files on their way to the project's trash (see
/// [`super::departed`]) are left out. Answers the new walk with the changes.
pub fn scan(
    file: &Path,
    before: &Tree,
    project: &Project,
    known: &KnownFiles,
) -> Result<(Tree, Vec<project_tree::Change>), PersistenceError> {
    let root = file.parent().unwrap_or(Path::new("."));
    let after = project_tree::walk(root)?;
    let departed = super::files::departed(file, project, known.journal());
    let mut models = BTreeMap::new();
    for found in &after.files {
        let new = !before
            .files
            .iter()
            .any(|old| names::same_name(&old.path, &found.path));
        if found.kind == FileKind::Model
            && new
            && let Ok((id, _, _)) = read_model(root, &found.path)
        {
            models.insert(found.path.clone(), id);
        }
    }
    let changes = project_tree::reconcile(before, &after, project, &departed, &models);
    Ok((after, changes))
}

/// What applying the changes a scan of the project's tree found told.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Adopted {
    /// What changed in the project, one line each: a new file registered, a path taken in the
    /// disk's case, a model found by the ID inside its file.
    pub fixes: Vec<String>,
    /// What is wrong now, one line each: a file missing, a model changed outside OXY.
    pub warnings: Vec<String>,
    /// The images whose files changed outside OXY, whose pixels are read again.
    pub images: Vec<Id>,
}

/// Applies to `project`, saved in the folder `root`, the changes a scan of its tree found (see
/// [`project_tree::reconcile`]), as opening would: a new file is registered (its ID made from its
/// path, a model keeping the one in its file while free), a path takes the disk's case, a model
/// moved outside OXY gets its pieces from the file found by its ID; a file gone and a model
/// changed outside OXY are told. Called between two steps of the undo history, never during
/// one: the next save writes what changed, and the next opening would make it the same way.
pub fn adopt(
    project: &mut Project,
    root: &Path,
    changes: &[project_tree::Change],
    known: &mut KnownFiles,
) -> Adopted {
    use project_tree::Change;
    let mut adopted = Adopted::default();
    let mut taken = identifiers(project);
    for change in changes {
        match change {
            Change::New { path, kind } => {
                let Some(kind) = asset_kind(*kind) else {
                    continue;
                };
                let Some(name) = name_of(path, kind) else {
                    continue;
                };
                let mut asset = Asset {
                    id: String::new(),
                    name,
                    path: path.clone(),
                    kind,
                    model: None,
                };
                let read = (kind == AssetKind::Model).then(|| read_model(root, path));
                asset.id = match &read {
                    Some(Ok((id, _, _))) if !taken.contains(id) => id.clone(),
                    _ => free_id(path, &taken),
                };
                taken.insert(asset.id.clone());
                adopted.fixes.push(format!(
                    "{}: arquivo novo na pasta do projeto, {path}",
                    label(&asset)
                ));
                let problem = match read {
                    Some(Ok((_, entities, bytes))) => {
                        let fits = Asset {
                            model: Some(entities.clone()),
                            ..asset.clone()
                        };
                        match validate_model(project, &fits) {
                            Ok(()) => {
                                known.note(asset.id.clone(), path.clone(), bytes);
                                asset.model = Some(entities);
                                None
                            }
                            Err(error) => Some(format!("não serve neste projeto: {error}")),
                        }
                    }
                    Some(Err(problem)) => Some(match problem {
                        FileProblem::Missing => "não existe mais".to_owned(),
                        FileProblem::Unreadable(why) | FileProblem::Broken(why) => why,
                    }),
                    None => header_state(root, &asset).map(|state| match state {
                        AssetState::Missing => "não existe mais".to_owned(),
                        AssetState::Unreadable(why)
                        | AssetState::Broken(why)
                        | AssetState::InvalidPath(why) => why,
                    }),
                };
                if let Some(why) = problem {
                    adopted.warnings.push(format!(
                        "{}: o arquivo {path} não pode ser usado ({why}); {REFUSED}",
                        label(&asset)
                    ));
                }
                project.assets.push(asset);
            }
            Change::Case { asset, path } => {
                let Some(asset) = project.assets.iter_mut().find(|a| a.id == *asset) else {
                    continue;
                };
                adopted.fixes.push(format!(
                    "{}: o arquivo se chama {path} no disco, com essas maiúsculas e minúsculas (o \
                     projeto o tinha como {})",
                    label(asset),
                    asset.path
                ));
                asset.path = path.clone();
            }
            Change::Found { asset, path } => {
                let Some(index) = project.assets.iter().position(|a| a.id == *asset) else {
                    continue;
                };
                let found = match read_model(root, path) {
                    Ok((id, entities, bytes)) if id == *asset => (entities, bytes),
                    _ => continue,
                };
                let asset = &mut project.assets[index];
                adopted.fixes.push(format!(
                    "{}: o arquivo foi encontrado em {path} pelo ID que tem dentro (o projeto o \
                     tinha em {})",
                    label(asset),
                    asset.path
                ));
                asset.path = path.clone();
                asset.model = Some(found.0);
                known.note(asset.id.clone(), path.clone(), found.1);
            }
            Change::Missing { asset, near } => {
                let Some(asset) = project.asset(asset) else {
                    continue;
                };
                let near = if near.is_empty() {
                    String::new()
                } else {
                    format!(" (talvez seja {})", near.join(" ou "))
                };
                adopted.warnings.push(format!(
                    "{}: o arquivo {} não está mais na pasta do projeto{near}; {REFUSED}",
                    label(asset),
                    asset.path
                ));
            }
            Change::Changed { asset } => {
                let Some(asset) = project.asset(asset) else {
                    continue;
                };
                if asset.kind == AssetKind::Model {
                    adopted.warnings.push(format!(
                        "{}: o arquivo {} mudou fora da OXY; reabra o projeto para carregar a \
                         versão de fora (salvar antes guarda uma cópia dela em .oxy/backup)",
                        label(asset),
                        asset.path
                    ));
                } else {
                    adopted.images.push(asset.id.clone());
                }
            }
        }
    }
    adopted
}

/// `asset` as messages name it: `Pulo (som)`.
fn label(asset: &Asset) -> String {
    let kind = match asset.kind {
        AssetKind::Texture => "imagem",
        AssetKind::Audio => "som",
        AssetKind::Model => "modelo",
    };
    format!("{} ({kind})", asset.name)
}

/// The extension of the files of `kind` in the tree layout.
fn extension(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Texture => ".png",
        AssetKind::Audio => ".wav",
        AssetKind::Model => model_file::EXTENSION,
    }
}

/// The kind of asset files of `kind` are, if any.
fn asset_kind(kind: FileKind) -> Option<AssetKind> {
    match kind {
        FileKind::Image => Some(AssetKind::Texture),
        FileKind::Sound => Some(AssetKind::Audio),
        FileKind::Model => Some(AssetKind::Model),
        FileKind::Script | FileKind::Other => None,
    }
}

/// The name an asset of `kind` whose file is `path` has: the file's name without the extension
/// of its kind.
pub(crate) fn name_of(path: &str, kind: AssetKind) -> Option<String> {
    if kind == AssetKind::Model {
        return model_file::stem(path).map(str::to_owned);
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    let cut = file.len().checked_sub(extension(kind).len())?;
    (cut > 0 && file.is_char_boundary(cut) && file[cut..].eq_ignore_ascii_case(extension(kind)))
        .then(|| file[..cut].to_owned())
}

/// The ID a file registered at opening gets: made from its `path` (with `salt` when that one is
/// taken), so the same file gets the same one at every opening until a save writes it.
fn implicit_id(path: &str, salt: u32) -> Id {
    let salt = salt.to_le_bytes();
    let parts = |part: &[u8]| crate::random::hash(&[b"oxy-registro", part, path.as_bytes(), &salt]);
    let (high, low) = (parts(b"1"), parts(b"2"));
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&high.to_le_bytes());
    bytes[8..].copy_from_slice(&low.to_le_bytes());
    uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string()
}

/// A model's file as opening read it: its ID, pieces and bytes, or why not.
type ModelRead = Result<ModelOfFile, FileProblem>;
/// A model as its file holds it: its ID, its pieces and the file's bytes.
pub(crate) type ModelOfFile = (Id, Vec<Entity>, Arc<[u8]>);

/// Why a file could not be used.
#[derive(Clone, Debug)]
enum FileProblem {
    Missing,
    /// It cannot be read now: why.
    Unreadable(String),
    /// Its content is not what its kind holds: why.
    Broken(String),
}

/// Where an asset's file is.
enum Found {
    /// There, at this path (the disk's case).
    At(String),
    /// It cannot be read now: why, and its path in the tree when the tree lists it.
    Unreadable(String, Option<String>),
    /// In a folder the project leaves out (a link, another project): why.
    Outside(String),
    Missing,
}

/// The project's folder as opening looks into it.
struct Disk<'a> {
    root: &'a Path,
    layout: Layout,
    tree: Option<&'a Tree>,
    /// The tree's files by path, and by path in [`names::fold`]ed case.
    exact: HashMap<&'a str, &'a TreeFile>,
    folded: HashMap<String, Vec<&'a TreeFile>>,
}
impl<'a> Disk<'a> {
    fn new(root: &'a Path, layout: Layout, tree: Option<&'a Tree>) -> Self {
        let mut exact = HashMap::new();
        let mut folded: HashMap<String, Vec<&TreeFile>> = HashMap::new();
        for file in tree.iter().flat_map(|tree| &tree.files) {
            exact.insert(file.path.as_str(), file);
            folded.entry(fold(&file.path)).or_default().push(file);
        }
        Self {
            root,
            layout,
            tree,
            exact,
            folded,
        }
    }

    /// Where the file `path` (from the project's folder) is: in the old layout as Windows opens
    /// it, in the tree layout as the tree has it, in its case or, when only one file differs in
    /// case alone, in the disk's.
    fn find(&self, path: &str) -> Found {
        let Some(tree) = self.tree else {
            return self.find_alone(path);
        };
        let file = self.exact.get(path).copied().or_else(|| {
            match self.folded.get(&fold(path)).map(Vec::as_slice) {
                Some([one]) => Some(*one),
                _ => None,
            }
        });
        if let Some(file) = file {
            return match &file.stamp {
                Ok(_) => Found::At(file.path.clone()),
                Err(why) => Found::Unreadable(why.clone(), Some(file.path.clone())),
            };
        }
        let within = |folder: &str| {
            folder.is_empty()
                || names::same_name(path.get(..folder.len()).unwrap_or_default(), folder)
                    && path.as_bytes().get(folder.len()) == Some(&b'/')
        };
        if let Some((folder, why)) = tree.skipped.iter().find(|(folder, _)| within(folder)) {
            return Found::Outside(format!("fica fora do projeto: {folder} {why}"));
        }
        if let Some((_, why)) = tree.unreadable.iter().find(|(folder, _)| within(folder)) {
            return Found::Unreadable(
                format!("{}: a pasta dele {why}", project_tree::NOT_NOW),
                None,
            );
        }
        Found::Missing
    }

    /// [`Self::find`] without the tree: as Windows opens the file in the old layout, as the tree
    /// would list it in the tree layout.
    fn find_alone(&self, path: &str) -> Found {
        if self.layout == Layout::Tree {
            return match project_tree::presence(self.root, path) {
                Presence::Present => Found::At(path.to_owned()),
                Presence::Unreadable(why) => Found::Unreadable(why, None),
                Presence::Missing => Found::Missing,
            };
        }
        let Ok(file) = resolve_asset_path(self.root, path) else {
            return Found::Missing;
        };
        match fs::metadata(&file) {
            Ok(meta) if meta.is_file() => Found::At(path.to_owned()),
            Ok(_) => Found::Missing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Found::Missing,
            Err(error) => Found::Unreadable(project_tree::now_unreadable(&error), None),
        }
    }

    /// The stamp the tree listed for its file `path`.
    fn stamp(&self, path: &str) -> Option<Stamp> {
        self.exact.get(path)?.stamp.clone().ok()
    }

    /// Every model's file of the tree, read once, by path, in the tree's order.
    fn read_models(&self) -> Vec<(String, ModelRead)> {
        self.tree
            .iter()
            .flat_map(|tree| &tree.files)
            .filter(|file| file.kind == FileKind::Model)
            .map(|file| {
                let read = match &file.stamp {
                    Err(why) => Err(FileProblem::Unreadable(why.clone())),
                    Ok(_) => read_model(self.root, &file.path),
                };
                (file.path.clone(), read)
            })
            .collect()
    }
}

/// `text` in the case NTFS compares names in.
fn fold(text: &str) -> String {
    text.chars().map(names::fold).collect()
}

/// The model in the file `path` of the project at `root`.
pub(crate) fn read_model_file(root: &Path, path: &str) -> Result<ModelOfFile, String> {
    read_model(root, path).map_err(|problem| match problem {
        FileProblem::Missing => "não existe na pasta do projeto".to_owned(),
        FileProblem::Unreadable(why) | FileProblem::Broken(why) => why,
    })
}

fn read_model(root: &Path, path: &str) -> ModelRead {
    let file =
        resolve_asset_path(root, path).map_err(|error| FileProblem::Broken(error.to_string()))?;
    let bytes = match fs_ops::retrying(|| {
        let meta = fs::metadata(&file)?;
        if meta.len() > model_file::MAX_BYTES {
            return Ok(None);
        }
        fs::read(&file).map(Some)
    }) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(FileProblem::Broken("tem mais de 64 MB".into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(FileProblem::Missing);
        }
        Err(error) => {
            return Err(FileProblem::Unreadable(project_tree::now_unreadable(
                &error,
            )));
        }
    };
    let (id, entities) = model_file::from_bytes(&bytes).map_err(FileProblem::Broken)?;
    Ok((id, entities, bytes.into()))
}

/// One opening at work.
struct Opening<'a, 'b> {
    disk: &'a Disk<'a>,
    report: &'b mut OpenReport,
    /// The tree's files the project's entries have, which are not registered again.
    claimed: HashSet<String>,
    /// The entries whose file went and that nothing uses, which leave the project.
    gone: Vec<Id>,
    /// The entries whose file OXY sent to the project's trash since the save (see
    /// [`journal::relink_assets`]): missing, whatever file their saved path has now.
    trashed: HashSet<Id>,
}
/// The line that tells what is wrong with `asset` (`state`), as opening and the checks say it.
fn problem_line(asset: &Asset, state: &AssetState) -> String {
    let label = label(asset);
    let path = &asset.path;
    match state {
        AssetState::Missing => {
            format!("{label}: o arquivo {path} não existe na pasta do projeto; {REFUSED}")
        }
        AssetState::Unreadable(why) => format!("{label}: o arquivo {path} {why}; {REFUSED}"),
        AssetState::InvalidPath(why) => {
            format!("{label}: o caminho {path} é inválido ({why}); {REFUSED}")
        }
        AssetState::Broken(why) if asset.kind == AssetKind::Model => {
            format!("{label}: o arquivo {path} {why}; o modelo fica indisponível, e {REFUSED}")
        }
        AssetState::Broken(why) => {
            format!("{label}: o arquivo {path} não pode ser usado ({why}); {REFUSED}")
        }
    }
}

/// How an asset of an open project stands on the disk now (see [`check_assets`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checked {
    /// Something is wrong with it, told by the line opening would tell.
    Problem(AssetState, String),
    /// Its file is made by the next save: a model saved in OXY, an image whose pixels wait.
    Unsaved,
}

/// How each asset of `project`, saved in the folder `root`, stands on the disk now, looked for
/// as opening looks (see [`open_project`]): a file missing, one that cannot be read now, a saved
/// path that names no file OXY reads (another asset's included), a content OXY cannot use, a
/// model that cannot be read or does not fit the project; or a file the next save makes.
/// `unsaved` says which images have pixels the next save writes, whose files may not be there
/// yet. Assets that are fine are left out. Nothing is changed or written.
pub fn check_assets(
    project: &Project,
    root: &Path,
    unsaved: &dyn Fn(&str) -> bool,
) -> BTreeMap<Id, Checked> {
    let tree = if project.layout == Layout::Tree {
        project_tree::walk(root).ok()
    } else {
        None
    };
    let disk = Disk::new(root, project.layout, tree.as_ref());
    let mut checked = BTreeMap::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    for asset in &project.assets {
        if asset.path.is_empty() && asset.kind == AssetKind::Model {
            // Inside the project file of the old layout.
            continue;
        }
        let why = path_problem(&disk, asset).or_else(|| {
            seen.iter()
                .find(|(path, _)| names::same_name(path, &asset.path))
                .map(|(_, other)| format!("é o mesmo de {other}"))
        });
        seen.push((asset.path.clone(), label(asset)));
        let state = match why {
            Some(why) => Err(AssetState::InvalidPath(why)),
            None => match disk.find(&asset.path) {
                Found::At(path) if asset.kind == AssetKind::Model => {
                    model_now(&disk, project, asset, &path)
                }
                Found::At(path) => {
                    let mut there = asset.clone();
                    there.path = path;
                    header_state(root, &there).map_or(Ok(false), Err)
                }
                Found::Unreadable(why, _) => Err(AssetState::Unreadable(why)),
                Found::Outside(why) => Err(AssetState::InvalidPath(why)),
                Found::Missing if asset.kind == AssetKind::Model && asset.model.is_some() => {
                    Ok(true)
                }
                Found::Missing if asset.kind == AssetKind::Texture && unsaved(&asset.id) => {
                    Ok(true)
                }
                Found::Missing => Err(AssetState::Missing),
            },
        };
        match state {
            Ok(false) => {}
            Ok(true) => {
                checked.insert(asset.id.clone(), Checked::Unsaved);
            }
            Err(state) => {
                let line = problem_line(asset, &state);
                checked.insert(asset.id.clone(), Checked::Problem(state, line));
            }
        }
    }
    checked
}

/// How the model `asset`, whose file is at `path`, stands: fine with its pieces, else what its
/// file holds now (it could not be used when the project opened).
fn model_now(
    disk: &Disk,
    project: &Project,
    asset: &Asset,
    path: &str,
) -> Result<bool, AssetState> {
    if asset.model.is_some() {
        return Ok(false);
    }
    match read_model(disk.root, path) {
        Ok((id, entities, _)) if id == asset.id => {
            let mut fitted = asset.clone();
            fitted.model = Some(entities);
            Err(AssetState::Broken(match validate_model(project, &fitted) {
                Err(error) => format!("não serve neste projeto: {error}"),
                Ok(()) => "mudou fora da OXY depois da abertura; reabra o projeto para \
                    carregá-lo"
                    .into(),
            }))
        }
        // Another model's file has its name now.
        Ok(_) | Err(FileProblem::Missing) => Err(AssetState::Missing),
        Err(FileProblem::Unreadable(why)) => Err(AssetState::Unreadable(why)),
        Err(FileProblem::Broken(why)) => Err(AssetState::Broken(why)),
    }
}

impl Opening<'_, '_> {
    fn problem(&mut self, asset: &Asset, state: AssetState) {
        self.report.warnings.push(problem_line(asset, &state));
        self.report.assets.insert(asset.id.clone(), state);
    }

    /// Tells the saved paths that name no file OXY reads: refused by the rules of names, out of
    /// the project, the extension of another kind (in the tree layout) or another entry's.
    fn check_paths(&mut self, project: &mut Project) {
        let mut seen: Vec<(String, String)> = Vec::new();
        for index in 0..project.assets.len() {
            let asset = &project.assets[index];
            if asset.path.is_empty() && asset.kind == AssetKind::Model
                || self.trashed.contains(&asset.id)
            {
                // A model inside the project file of the old layout, or a file in the trash.
                continue;
            }
            let why = path_problem(self.disk, asset).or_else(|| {
                seen.iter()
                    .find(|(path, _)| names::same_name(path, &asset.path))
                    .map(|(_, other)| format!("é o mesmo de {other}"))
            });
            seen.push((asset.path.clone(), label(asset)));
            if let Some(why) = why {
                let asset = asset.clone();
                self.problem(&asset, AssetState::InvalidPath(why));
            }
        }
    }

    /// Whether the entry `asset` has a problem already.
    fn has_problem(&self, asset: &Asset) -> bool {
        self.report.assets.contains_key(&asset.id)
    }

    /// Gives each model of the project file its pieces from its file: the one at its path in
    /// its case or the disk's, else the first file in tree order with its ID (moved or renamed
    /// outside OXY). One whose file went stays missing while it is placed in a scene, and leaves
    /// the project when it is not.
    fn load_models(&mut self, project: &mut Project, models: &[(String, ModelRead)]) {
        for index in 0..project.assets.len() {
            let asset = &project.assets[index];
            if asset.kind != AssetKind::Model || self.has_problem(asset) {
                continue;
            }
            if self.trashed.contains(&asset.id) {
                let asset = asset.clone();
                self.trashed_file(project, &asset);
                continue;
            }
            // Pieces inside the project file (an exported game's, one written by hand) are the
            // model's; the next save writes them to its file.
            if asset.model.is_some() {
                if let Found::At(path) = self.disk.find(&asset.path) {
                    self.claimed.insert(path);
                }
                continue;
            }
            let mut state = AssetState::Missing;
            let mut found = None;
            match self.disk.find(&asset.path) {
                // Read again when the tree could not be walked.
                Found::At(path) => match read_of(models, &path)
                    .cloned()
                    .unwrap_or_else(|| read_model(self.disk.root, &path))
                {
                    Ok((id, entities, bytes)) if id == asset.id => {
                        found = Some((path, entities, bytes));
                    }
                    // Another model now has its name: this one's file went.
                    Ok(_) => {}
                    Err(problem) => {
                        self.claimed.insert(path.clone());
                        state = match problem {
                            FileProblem::Missing => AssetState::Missing,
                            FileProblem::Unreadable(why) => AssetState::Unreadable(why),
                            FileProblem::Broken(why) => AssetState::Broken(why),
                        };
                    }
                },
                Found::Unreadable(why, file) => {
                    self.claimed.extend(file);
                    state = AssetState::Unreadable(why);
                }
                Found::Outside(why) => state = AssetState::InvalidPath(why),
                Found::Missing => {}
            }
            if found.is_none() && state == AssetState::Missing {
                found = models.iter().find_map(|(path, read)| match read {
                    Ok((id, entities, bytes))
                        if *id == asset.id && !self.claimed.contains(path) =>
                    {
                        Some((path.clone(), entities.clone(), bytes.clone()))
                    }
                    _ => None,
                });
            }
            let Some((path, entities, bytes)) = found else {
                let asset = asset.clone();
                if state == AssetState::Missing {
                    self.went(project, &asset);
                } else {
                    self.problem(&asset, state);
                }
                continue;
            };
            self.claimed.insert(path.clone());
            self.report
                .known
                .note(asset.id.clone(), path.clone(), bytes);
            if path != asset.path {
                let (label, old) = (label(asset), &asset.path);
                let fix = if names::same_name(&path, old) {
                    format!(
                        "{label}: o arquivo se chama {path} no disco, com essas maiúsculas e \
                         minúsculas (o projeto o tinha como {old})"
                    )
                } else {
                    format!(
                        "{label}: o arquivo foi encontrado em {path} pelo ID que tem dentro (o \
                         projeto o tinha em {old})"
                    )
                };
                self.report.fixes.push(fix);
            }
            let asset = &mut project.assets[index];
            asset.path = path;
            asset.model = Some(entities);
        }
    }

    /// The entry `asset`, whose file is not in the project's folder: told missing while
    /// something uses it, and taken out of the project when nothing does.
    fn went(&mut self, project: &Project, asset: &Asset) {
        if asset_uses(project, &asset.id).is_empty() {
            self.report.fixes.push(format!(
                "{}: o arquivo {} não existe mais, e nada no projeto usava o recurso; ele saiu \
                 da Biblioteca",
                label(asset),
                asset.path
            ));
            self.gone.push(asset.id.clone());
        } else {
            self.problem(asset, AssetState::Missing);
        }
    }

    /// The entry `asset`, whose file OXY sent to the project's trash since the save: missing
    /// while something uses it, and out of the project, as when it was deleted, when nothing
    /// does. Its saved path is not looked at: another file may have it now.
    fn trashed_file(&mut self, project: &Project, asset: &Asset) {
        if asset_uses(project, &asset.id).is_empty() {
            self.gone.push(asset.id.clone());
        } else {
            self.report.warnings.push(format!(
                "{}: o arquivo {} foi para a lixeira do projeto (.oxy/lixeira) depois do último \
                 salvamento, e o recurso ainda é usado; {REFUSED}",
                label(asset),
                asset.path
            ));
            self.report
                .assets
                .insert(asset.id.clone(), AssetState::Missing);
        }
    }

    /// Looks for the file of each image and sound and reads its header: a path the tree has in
    /// another case alone is taken as the disk has it.
    fn check_files(&mut self, project: &mut Project) {
        for index in 0..project.assets.len() {
            let asset = &project.assets[index];
            if asset.kind == AssetKind::Model || self.has_problem(asset) {
                continue;
            }
            if self.trashed.contains(&asset.id) {
                let asset = asset.clone();
                self.trashed_file(project, &asset);
                continue;
            }
            let path = match self.disk.find(&asset.path) {
                Found::At(path) => path,
                Found::Unreadable(why, file) => {
                    self.claimed
                        .insert(file.unwrap_or_else(|| asset.path.clone()));
                    let asset = asset.clone();
                    self.problem(&asset, AssetState::Unreadable(why));
                    continue;
                }
                Found::Outside(why) => {
                    let asset = asset.clone();
                    self.problem(&asset, AssetState::InvalidPath(why));
                    continue;
                }
                Found::Missing => {
                    let asset = asset.clone();
                    self.went(project, &asset);
                    continue;
                }
            };
            self.claimed.insert(path.clone());
            if let (AssetKind::Texture, Some(stamp)) = (asset.kind, self.disk.stamp(&path)) {
                self.report
                    .known
                    .note_image(asset.id.clone(), path.clone(), stamp);
            }
            if path != asset.path {
                self.report.fixes.push(format!(
                    "{}: o arquivo se chama {path} no disco, com essas maiúsculas e minúsculas \
                     (o projeto o tinha como {})",
                    label(asset),
                    asset.path
                ));
                project.assets[index].path = path;
            }
            let asset = project.assets[index].clone();
            if let Some(state) = header_state(self.disk.root, &asset) {
                self.problem(&asset, state);
            }
        }
    }

    /// Registers every image, sound and model of the tree no entry has, in the order of the
    /// bytes of their paths, at the end of the project's assets: each one's ID made from its
    /// path (a model keeps the one in its file while no other asset or object has it), its name
    /// its file's. The files the old layout named after an asset's ID (`assets/<id>.wav`) are
    /// never registered: no asset is named like an ID.
    fn register(&mut self, project: &mut Project, models: &[(String, ModelRead)]) {
        let Some(tree) = self.disk.tree else {
            return;
        };
        let mut new: Vec<&TreeFile> = tree
            .files
            .iter()
            .filter(|file| {
                asset_kind(file.kind).is_some()
                    && !self.claimed.contains(&file.path)
                    && !crate::conversion::old_asset_file(&file.path)
            })
            .collect();
        new.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        let mut taken = identifiers(project);
        for file in new {
            let kind = asset_kind(file.kind).expect("filtered");
            let Some(name) = name_of(&file.path, kind) else {
                continue;
            };
            let read = read_of(models, &file.path);
            let mut asset = Asset {
                id: String::new(),
                name,
                path: file.path.clone(),
                kind,
                model: None,
            };
            let mut copy = false;
            asset.id = match read {
                Some(Ok((id, _, _))) if !taken.contains(id) => id.clone(),
                Some(Ok(_)) => {
                    copy = true;
                    free_id(&file.path, &taken)
                }
                _ => free_id(&file.path, &taken),
            };
            taken.insert(asset.id.clone());
            self.report.fixes.push(if copy {
                format!(
                    "{}: arquivo novo na pasta do projeto, {}; ele tem o ID de outro modelo (é uma \
                     cópia?) e ganhou um ID próprio",
                    label(&asset),
                    file.path
                )
            } else {
                format!(
                    "{}: arquivo novo na pasta do projeto, {}",
                    label(&asset),
                    file.path
                )
            });
            let state = if let Some(why) = path_problem(self.disk, &asset) {
                Some(AssetState::InvalidPath(why))
            } else if let Err(why) = &file.stamp {
                Some(AssetState::Unreadable(why.clone()))
            } else {
                if let (AssetKind::Texture, Ok(stamp)) = (kind, &file.stamp) {
                    self.report
                        .known
                        .note_image(asset.id.clone(), asset.path.clone(), *stamp);
                }
                match read {
                    Some(Ok((_, entities, bytes))) => {
                        asset.model = Some(entities.clone());
                        self.report
                            .known
                            .note(asset.id.clone(), asset.path.clone(), bytes.clone());
                        None
                    }
                    Some(Err(FileProblem::Unreadable(why))) => {
                        Some(AssetState::Unreadable(why.clone()))
                    }
                    Some(Err(FileProblem::Broken(why))) => Some(AssetState::Broken(why.clone())),
                    Some(Err(FileProblem::Missing)) => Some(AssetState::Missing),
                    None => header_state(self.disk.root, &asset),
                }
            };
            if let Some(state) = state {
                self.problem(&asset, state);
            }
            project.assets.push(asset);
        }
    }

    /// Checks each model that came from its file against the project (see [`validate_model`]):
    /// one that does not fit, a piece naming a surface the project lacks, for one, is
    /// unavailable.
    fn fit_models(&mut self, project: &mut Project) {
        for index in 0..project.assets.len() {
            if let Err(error) = validate_model(project, &project.assets[index]) {
                project.assets[index].model = None;
                let asset = project.assets[index].clone();
                self.problem(
                    &asset,
                    AssetState::Broken(format!("não serve neste projeto: {error}")),
                );
            }
        }
    }
}

/// What the tree's model file `path` held, if it was read.
fn read_of<'a>(models: &'a [(String, ModelRead)], path: &str) -> Option<&'a ModelRead> {
    models
        .iter()
        .find(|(file, _)| file == path)
        .map(|(_, read)| read)
}

/// Why the saved path of `asset` names no file OXY reads, when it does not.
fn path_problem(disk: &Disk, asset: &Asset) -> Option<String> {
    if asset.path.is_empty() {
        return Some("o projeto não diz onde está o arquivo".into());
    }
    if let Err(error) = resolve_asset_path(disk.root, &asset.path) {
        return Some(match error {
            PersistenceError::InvalidPath { reason, .. } => reason,
            PersistenceError::InvalidAssetPath(_) | PersistenceError::AssetOutsideProject(_) => {
                "sai da pasta do projeto".into()
            }
            error => error.to_string(),
        });
    }
    if disk.layout == Layout::Legacy {
        return None;
    }
    if let Err(reason) = names::check_path(&asset.path) {
        return Some(reason);
    }
    name_of(&asset.path, asset.kind)
        .is_none()
        .then(|| format!("não termina em {}", extension(asset.kind)))
}

/// What is wrong with the header of the image or sound `asset`, whose file is there.
fn header_state(root: &Path, asset: &Asset) -> Option<AssetState> {
    let file = resolve_asset_path(root, &asset.path).ok()?;
    match super::validate_asset_header(asset, &file) {
        Ok(()) => None,
        Err(PersistenceError::AssetMissing { source, .. })
            if source.kind() == io::ErrorKind::NotFound =>
        {
            Some(AssetState::Missing)
        }
        Err(PersistenceError::AssetMissing { source, .. }) => Some(AssetState::Unreadable(
            project_tree::now_unreadable(&source),
        )),
        Err(PersistenceError::AssetContent { reason, .. }) => Some(AssetState::Broken(reason)),
        Err(PersistenceError::AssetTooLarge(_)) => Some(AssetState::Broken(
            "tem mais de 300 MB ou não é um arquivo".into(),
        )),
        Err(error) => Some(AssetState::Unreadable(error.to_string())),
    }
}

/// Every ID the project has: its own, its scenes', objects', assets', surfaces' and variables'.
fn identifiers(project: &Project) -> HashSet<Id> {
    let mut taken: HashSet<Id> = HashSet::from([project.id.clone()]);
    taken.extend(project.scenes.iter().map(|s| s.id.clone()));
    taken.extend(
        project
            .scenes
            .iter()
            .flat_map(|s| &s.entities)
            .map(|e| e.id.clone()),
    );
    taken.extend(project.assets.iter().map(|a| a.id.clone()));
    taken.extend(project.surfaces.iter().map(|s| s.id.clone()));
    taken.extend(project.variables.iter().map(|v| v.id.clone()));
    taken
}

/// The ID made from `path` that `taken` lacks.
fn free_id(path: &str, taken: &HashSet<Id>) -> Id {
    (0..)
        .map(|salt| implicit_id(path, salt))
        .find(|id| !taken.contains(id))
        .expect("a free ID")
}

/// Names each asset whose path is fine after its file, and tells the names two files of a kind
/// share (the scripts and the agents find assets by name).
fn name_by_files(project: &mut Project, report: &mut OpenReport) {
    for asset in &mut project.assets {
        if matches!(
            report.assets.get(&asset.id),
            Some(AssetState::InvalidPath(_))
        ) {
            continue;
        }
        if let Some(name) = name_of(&asset.path, asset.kind) {
            asset.name = name;
        }
    }
    report.warnings.extend(shared_names(project));
}

/// The names two files of a kind share, one line each (the scripts and the agents find assets
/// by name).
pub fn shared_names(project: &Project) -> Vec<String> {
    let mut lines = Vec::new();
    let mut shared: BTreeMap<(String, &'static str), Vec<(&str, &str)>> = BTreeMap::new();
    for asset in &project.assets {
        let kind = match asset.kind {
            AssetKind::Texture => "imagens",
            AssetKind::Audio => "sons",
            AssetKind::Model => "modelos",
        };
        shared
            .entry((asset.name.to_lowercase(), kind))
            .or_default()
            .push((&asset.name, &asset.path));
    }
    for ((_, kind), files) in shared {
        if let [(name, _), _, ..] = files.as_slice() {
            let paths: Vec<&str> = files.iter().map(|(_, path)| *path).collect();
            lines.push(format!(
                "O nome \"{name}\" é de {} {kind} ({}); renomeie os arquivos para que os scripts \
                 e os agentes achem cada um pelo nome",
                files.len(),
                paths.join(", ")
            ));
        }
    }
    lines
}
