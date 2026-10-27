//! The commands of the project's tree: folders made, renamed and removed, and files moved,
//! renamed and sent to the project's trash, with every reference following (the paths of the
//! assets, the scripts the objects attach in scenes and models). They are planned on a view of
//! the disk ([`TreeView`]) that each planned operation changes, so a later command of the batch
//! finds a file where an earlier one put it; the caller does the operations (journaled, see
//! [`crate::journal::run`]) before the planned project takes the place of the old one. The
//! editor live and the agents' tools on files share them. Nothing ever writes over a file: a
//! name taken is refused, with a free one suggested. A script moved, or a module it requires,
//! keeps being found: the `require` that would no longer find theirs are rewritten in the same
//! step (see [`requires`]).
use super::{Command, ItemKind, asset_kind, find_asset, refuse_used};
use crate::{
    document::{AssetKind, Id, Layout, Project, new_id},
    edit_history::FileOp,
    fs_ops,
    journal::{self, Journal, join, split_extension, under},
    names,
    persistence::{self, PROJECT_FILE, PersistenceError},
    project_tree::{self, FileKind, Tree},
    script::{self, ScriptSources},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod requires;

/// Why the tree's commands wait for a project of the old layout to be organized in folders.
pub(super) const LEGACY: &str = "Este projeto ainda está no formato antigo, sem pastas: o próximo \
    salvamento no editor (ou a próxima gravação de um agente) o organiza em pastas, com uma cópia \
    completa antes; depois disso, pastas e arquivos mudam pela OXY";

/// What an entry of the tree is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Folder,
    File(FileKind),
    /// A link or junction, or a folder that is another project or an exported game: its name is
    /// taken, and the tree does not go in.
    Outside,
}

/// The project's tree as a batch of commands plans it: what the walk found, changed by each
/// file operation planned so far.
#[derive(Clone, Debug)]
pub struct TreeView {
    root: PathBuf,
    /// Every folder and file of the tree as planned, by its path in the disk's case.
    entries: BTreeMap<String, Entry>,
    /// The project's scripts as planned: what attach_script reads.
    pub scripts: ScriptSources,
    /// What the planned operations did, from the disk as the view read it.
    planned: Journal,
    files: Vec<FileOp>,
    /// The step the trash slots of the batch are named after.
    step: String,
    slots: usize,
    /// What following the scripts' `require` told so far in the batch, told once.
    told: BTreeSet<String>,
}

impl TreeView {
    /// The tree and the scripts of the project at `root`, read from the disk.
    pub fn read(root: &Path) -> Result<Self, PersistenceError> {
        let (scripts, tree) = persistence::load_scripts_in_tree(root)?;
        Ok(Self::new(root, &tree, scripts))
    }

    /// The view of `tree`, the walk of the project at `root`, whose scripts are `scripts`.
    pub fn new(root: &Path, tree: &Tree, scripts: ScriptSources) -> Self {
        let mut entries = BTreeMap::new();
        for folder in &tree.folders {
            entries.insert(folder.clone(), Entry::Folder);
        }
        for file in &tree.files {
            entries.insert(file.path.clone(), Entry::File(file.kind));
        }
        for outside in tree.skipped.keys() {
            entries.insert(outside.clone(), Entry::Outside);
        }
        Self {
            root: root.to_owned(),
            entries,
            scripts,
            planned: Journal::default(),
            files: Vec::new(),
            step: new_id()[..8].to_owned(),
            slots: 0,
            told: BTreeSet::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file operations planned, in order.
    pub fn files(&self) -> &[FileOp] {
        &self.files
    }

    /// The entry at `path` (in any case), with its path in the disk's case.
    pub fn find(&self, path: &str) -> Option<(String, Entry)> {
        let path = path.trim().replace('\\', "/");
        let path = path.trim_matches('/');
        if path.is_empty() {
            return None;
        }
        if let Some(entry) = self.entries.get(path) {
            return Some((path.to_owned(), *entry));
        }
        let mut alike = self
            .entries
            .iter()
            .filter(|(known, _)| names::same_name(known, path));
        match (alike.next(), alike.next()) {
            (Some((known, entry)), None) => Some((known.clone(), *entry)),
            _ => None,
        }
    }

    /// What the folder `folder` ("" for the root) holds directly, in tree order.
    fn children(&self, folder: &str) -> Vec<(String, Entry)> {
        let mut children: Vec<(String, Entry)> = self
            .entries
            .iter()
            .filter(|(path, _)| names::same_name(parent(path), folder))
            .map(|(path, entry)| (path.clone(), *entry))
            .collect();
        children.sort_by(|a, b| {
            names::tree_order(
                (file_name(&a.0), a.1 == Entry::Folder),
                (file_name(&b.0), b.1 == Entry::Folder),
            )
        });
        children
    }

    /// The names in the folder `folder` that the tree leaves out (those starting with a dot,
    /// the leftovers of Windows and editors, and at the root the project's own files), as the
    /// disk had them when the view was read; none for a folder made by the batch.
    fn hidden(&self, folder: &str) -> Vec<String> {
        let Some(origin) = self.planned.saved_of(folder) else {
            return Vec::new();
        };
        let place = if origin.is_empty() {
            Ok(self.root.clone())
        } else {
            names::resolve(&self.root, &origin)
        };
        let Ok(listing) = place.and_then(|place| fs::read_dir(place).map_err(Into::into)) else {
            return Vec::new();
        };
        let mut hidden: Vec<String> = listing
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let file = entry.file_type().is_ok_and(|kind| kind.is_file());
                let own = origin.is_empty()
                    && (name.eq_ignore_ascii_case(PROJECT_FILE)
                        || name
                            .to_ascii_lowercase()
                            .starts_with(&format!("{PROJECT_FILE}.")));
                (name.starts_with('.') || (file && project_tree::leftover(&name)) || own)
                    .then_some(name)
            })
            .collect();
        hidden.sort_by(|a, b| names::name_order(a, b));
        hidden
    }

    /// Whether `name` is taken in the folder `folder`, by what the tree has or leaves out.
    fn taken(&self, folder: &str, name: &str) -> bool {
        self.find(&join(folder, name)).is_some()
            || self
                .hidden(folder)
                .iter()
                .any(|hidden| names::same_name(hidden, name))
    }

    fn plan(&mut self, op: FileOp) {
        self.planned.apply(&op.forward());
        self.files.push(op);
    }

    fn plan_move(&mut self, from: &str, to: &str) {
        let follow = |path: &str| under(path, from).map(|rest| join(to, rest));
        let moved: Vec<(String, Entry)> = self
            .entries
            .iter()
            .filter(|(path, _)| follow(path).is_some())
            .map(|(path, entry)| (path.clone(), *entry))
            .collect();
        for (path, entry) in moved {
            self.entries.remove(&path);
            self.entries.insert(follow(&path).expect("under"), entry);
        }
        self.scripts.unreadable = std::mem::take(&mut self.scripts.unreadable)
            .into_iter()
            .map(|(path, why)| (follow(&path).unwrap_or(path), why))
            .collect();
        self.scripts.files = std::mem::take(&mut self.scripts.files)
            .into_iter()
            .map(|(path, file)| (follow(&path).unwrap_or(path), file))
            .collect();
        self.plan(FileOp::Move {
            from: from.to_owned(),
            to: to.to_owned(),
        });
    }

    fn plan_create_dir(&mut self, path: &str) {
        self.entries.insert(path.to_owned(), Entry::Folder);
        self.plan(FileOp::CreateDir {
            path: path.to_owned(),
        });
    }

    fn plan_remove_dir(&mut self, path: &str) {
        self.entries.remove(path);
        self.plan(FileOp::RemoveDir {
            path: path.to_owned(),
        });
    }

    fn plan_trash(&mut self, path: &str, id: Option<Id>) {
        self.entries.remove(path);
        self.scripts.files.remove(path);
        self.scripts.unreadable.remove(path);
        self.slots += 1;
        let slot = fs_ops::trash_slot(&self.step, self.slots, path);
        self.plan(FileOp::Trash {
            from: path.to_owned(),
            slot,
            id,
        });
    }
}

/// The folder `path` is in ("" at the root).
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}
/// The last name of `path`.
fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}
/// The folder `folder` as messages place a file in it.
fn place(folder: &str) -> String {
    if folder.is_empty() {
        "na raiz do projeto".into()
    } else {
        format!("em {folder}")
    }
}

/// Whether the tree's commands take `command` in `project`: those of folders and moves always,
/// renaming and deleting an asset in the tree layout (in the old one the asset's file never
/// changes).
pub(super) fn handles(command: &Command, project: &Project) -> bool {
    match command {
        Command::CreateFolder { .. }
        | Command::RenameFolder { .. }
        | Command::Move { .. }
        | Command::DeleteFolder { .. } => true,
        Command::RenameAsset { .. } | Command::DeleteAsset { .. } => project.layout == Layout::Tree,
        _ => false,
    }
}

/// Whether `command` needs the project's folder, which a batch without a view of the disk
/// refuses.
pub(super) fn needs_folder(command: &Command) -> bool {
    matches!(
        command,
        Command::CreateFolder { .. }
            | Command::RenameFolder { .. }
            | Command::Move { .. }
            | Command::DeleteFolder { .. }
            | Command::SaveModel {
                folder: Some(_),
                ..
            }
    )
}

/// Why a command of the tree needs the project's folder, when the batch has none.
pub(super) const NO_FOLDER: &str = "Este comando mexe nos arquivos do projeto, e precisa da \
    pasta dele: o projeto ainda não foi salvo, ou o comando veio sem ela";

/// Plans `command` in `project` on `view`: the document changes here, the disk operations go
/// to the view. Answers what the batch's answer tells besides.
pub(super) fn apply(
    project: &mut Project,
    command: &Command,
    view: &mut TreeView,
) -> Result<Vec<String>, String> {
    if project.layout == Layout::Legacy {
        return Err(LEGACY.into());
    }
    let start = view.files.len();
    let mut told = match command {
        Command::CreateFolder { path } => create_folder(view, path).map(|()| Vec::new()),
        Command::RenameFolder { folder, name } => {
            rename_folder(project, view, folder, name).map(|()| Vec::new())
        }
        Command::Move { items, to, kind } => {
            move_items(project, view, items, to, *kind).map(|()| Vec::new())
        }
        Command::DeleteFolder { folder } => delete_folder(project, view, folder),
        Command::RenameAsset { asset, name, kind } => {
            rename_item(project, view, asset, name, *kind).map(|()| Vec::new())
        }
        Command::DeleteAsset {
            asset,
            kind,
            detach,
        } => delete_item(project, view, asset, *kind, detach.unwrap_or(false)).map(|()| Vec::new()),
        _ => unreachable!("only the commands `handles` takes"),
    }?;
    told.extend(requires::follow(view, start));
    Ok(told)
}

/// Every path `project` has at or under `from` now at `to`: the files of its assets (each named
/// after its file) and the scripts its objects attach, in scenes and models.
fn follow(project: &mut Project, from: &str, to: &str) {
    for asset in &mut project.assets {
        if asset.path.is_empty() {
            continue;
        }
        if let Some(rest) = under(&asset.path, from) {
            asset.path = join(to, rest);
            if let Some(name) = persistence::name_of(&asset.path, asset.kind) {
                asset.name = name;
            }
        }
    }
    for entity in journal::entities_mut(project) {
        for script in &mut entity.scripts {
            if let Some(rest) = under(&script.path, from) {
                script.path = join(to, rest);
            }
        }
    }
}

/// Every path `project` has where `files` moved it, as [`follow`] for each move: the document
/// of a step whose files stayed only partly done.
pub(crate) fn follow_files(project: &mut Project, files: &[FileOp]) {
    for file in files {
        if let FileOp::Move { from, to } = file {
            follow(project, from, to);
        }
    }
}

/// The parts of the folder path `text` a person or an agent typed, each a name Windows keeps.
fn folder_parts(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim().replace('\\', "/");
    let parts: Vec<&str> = text.trim_matches('/').split('/').collect();
    if parts.iter().all(|part| part.trim().is_empty()) {
        return Err("Informe o caminho da pasta, como Pista ou Pista/Sons".into());
    }
    parts
        .iter()
        .map(|part| names::check_name(part).map_err(|why| format!("Nome de pasta inválido: {why}")))
        .collect()
}

/// The folder `text` names in `view`, in the disk's case; "" (or nothing) is the root.
fn folder(view: &TreeView, text: &str) -> Result<String, String> {
    let wanted = text.trim().replace('\\', "/");
    let wanted = wanted.trim_matches('/');
    if wanted.is_empty() {
        return Ok(String::new());
    }
    match view.find(wanted) {
        Some((path, Entry::Folder)) => Ok(path),
        Some((path, _)) => Err(format!("{path} não é uma pasta do projeto")),
        None => Err(format!("A pasta {wanted} não existe no projeto")),
    }
}

/// The folder `text` names in `view`, in the disk's case, made with the folders above it when
/// it is missing (planned as the batch's operations); "" (or nothing) is the root.
fn ensure_folder(view: &mut TreeView, text: &str) -> Result<String, String> {
    let wanted = text.trim().replace('\\', "/");
    if view.find(wanted.trim_matches('/')).is_none() && !wanted.trim_matches('/').is_empty() {
        create_folder(view, &wanted)?;
    }
    folder(view, &wanted)
}

/// save_model with a folder, in `project` on `view`: the folder in the disk's case, made when
/// missing, and a name its folder has refused with a free one suggested. Answers the command
/// with the folder found, which [`super::apply_one`] saves the model's path in.
pub(super) fn save_model_in(
    project: &Project,
    command: &Command,
    default_scene: &str,
    view: &mut TreeView,
) -> Result<Command, String> {
    let Command::SaveModel {
        scene,
        entity,
        name,
        replace,
        folder: Some(text),
    } = command
    else {
        return Ok(command.clone());
    };
    if project.layout == Layout::Legacy {
        return Err(LEGACY.into());
    }
    if replace.is_some() {
        return Err(
            "folder não vale com replace: o modelo continua no arquivo dele; para \
            levá-lo a outra pasta, use move"
                .into(),
        );
    }
    let path = ensure_folder(view, text)?;
    let scene = super::scene_id(project, scene, default_scene)?;
    let named = name.clone().or_else(|| {
        project
            .scene(&scene)
            .and_then(|s| s.entity(entity))
            .map(|e| e.name.clone())
    });
    if let Some(named) = named.filter(|n| !n.trim().is_empty()) {
        let file = format!("{}{}", named.trim(), crate::model_file::EXTENSION);
        if view.taken(&path, &file) {
            let free = free_name(project, view, &path, &file, &[]);
            return Err(format!(
                "Já existe {file} {}; a OXY não grava por cima: escolha outro nome com name ({} \
                 está livre lá)",
                place(&path),
                split_extension(&free).0
            ));
        }
    }
    let mut resolved = command.clone();
    if let Command::SaveModel { folder, .. } = &mut resolved {
        *folder = Some(path);
    }
    Ok(resolved)
}

/// Refuses moving what is at `from` to `to` when a path inside would pass what Windows and Luau
/// keep ([`names::MAX_PATH_BYTES`]).
fn check_lengths(view: &TreeView, from: &str, to: &str) -> Result<(), String> {
    for path in std::iter::once(from).chain(
        view.entries
            .keys()
            .map(String::as_str)
            .filter(|path| under(path, from).is_some()),
    ) {
        let moved = join(to, under(path, from).unwrap_or_default());
        if moved.len() > names::MAX_PATH_BYTES {
            return Err(format!(
                "o caminho {moved} ficaria longo demais: o limite é de {} bytes",
                names::MAX_PATH_BYTES
            ));
        }
    }
    Ok(())
}

/// The first of `name`, `name 2`, `name 3`… (before its extension) the folder `folder` of `view`
/// does not have, nor `extra` names already given, and that no asset of `project` of its kind
/// is called.
fn free_name(
    project: &Project,
    view: &TreeView,
    folder: &str,
    name: &str,
    extra: &[String],
) -> String {
    let (stem, extension) = split_extension(name);
    let kind = match FileKind::of(name) {
        FileKind::Image => Some(AssetKind::Texture),
        FileKind::Sound => Some(AssetKind::Audio),
        FileKind::Model => Some(AssetKind::Model),
        FileKind::Script | FileKind::Other => None,
    };
    (1..)
        .map(|n: usize| {
            if n == 1 {
                name.to_owned()
            } else {
                format!("{stem} {n}{extension}")
            }
        })
        .find(|candidate| {
            let (stem, _) = split_extension(candidate);
            !view.taken(folder, candidate)
                && !extra.iter().any(|other| names::same_name(other, candidate))
                && !project.assets.iter().any(|asset| {
                    Some(asset.kind) == kind
                        && asset.name.trim().to_lowercase() == stem.to_lowercase()
                        && !names::same_name(file_name(&asset.path), candidate)
                })
        })
        .expect("a free name")
}

/// What a command names: a file or folder of the tree (with the asset it is, if any), or an
/// asset whose file the tree lacks (a model not saved yet, a file missing).
enum Item {
    File {
        path: String,
        entry: Entry,
        asset: Option<Id>,
    },
    Registered(Id),
}

/// The item `text` names: a path of the tree (in any case), else an asset by ID or name
/// (`kind` telling apart assets of different kinds that share a name), else a script by its
/// path. Folders only when `folders`.
fn item(
    project: &Project,
    view: &TreeView,
    text: &str,
    kind: Option<ItemKind>,
    folders: bool,
) -> Result<Item, String> {
    let asset_at = |path: &str| {
        project
            .assets
            .iter()
            .find(|asset| !asset.path.is_empty() && names::same_name(&asset.path, path))
            .map(|asset| asset.id.clone())
    };
    if let Some((path, entry)) = view.find(text) {
        let fits = match (entry, kind) {
            (Entry::Folder, _) => folders && kind.is_none(),
            (_, None) => true,
            (Entry::File(found), Some(kind)) => kind.file() == found,
            (Entry::Outside, Some(_)) => false,
        };
        if fits {
            return Ok(Item::File {
                asset: asset_at(&path),
                path,
                entry,
            });
        }
        if entry == Entry::Folder && !folders {
            return Err(format!(
                "{path} é uma pasta: use rename_folder ou delete_folder para pastas"
            ));
        }
    }
    let script =
        kind == Some(ItemKind::Script) || text.trim().to_ascii_lowercase().ends_with(".luau");
    let asset = if kind == Some(ItemKind::Script) {
        None
    } else {
        Some(find_asset(project, text, kind.and_then(ItemKind::asset)))
    };
    match asset {
        Some(Ok(id)) => {
            let found = project.asset(&id).expect("found");
            Ok(
                match view.find(&found.path).filter(|_| !found.path.is_empty()) {
                    Some((path, entry @ Entry::File(_))) => Item::File {
                        path,
                        entry,
                        asset: Some(id),
                    },
                    _ => Item::Registered(id),
                },
            )
        }
        Some(Err(error)) if !script => Err(error),
        _ => {
            let path = script::normalize_path(text)?;
            Err(view.scripts.absent(&path))
        }
    }
}

fn create_folder(view: &mut TreeView, path: &str) -> Result<(), String> {
    let parts = folder_parts(path)?;
    let mut current = String::new();
    let mut made = false;
    for part in parts {
        let next = join(&current, &part);
        match view.find(&next) {
            Some((exact, Entry::Folder)) => current = exact,
            Some((exact, _)) => {
                return Err(format!(
                    "Já existe {exact}, que não é uma pasta; escolha outro nome"
                ));
            }
            None => {
                names::check_path(&next).map_err(|why| format!("Pasta inválida: {why}"))?;
                if view.taken(&current, &part) {
                    return Err(format!(
                        "Já existe {part} {} (fora do projeto); escolha outro nome",
                        place(&current)
                    ));
                }
                view.plan_create_dir(&next);
                current = next;
                made = true;
            }
        }
    }
    if made {
        Ok(())
    } else {
        Err(format!("A pasta {current} já existe"))
    }
}

fn rename_folder(
    project: &mut Project,
    view: &mut TreeView,
    folder_text: &str,
    name: &str,
) -> Result<(), String> {
    let path = folder(view, folder_text)?;
    if path.is_empty() {
        return Err("Informe a pasta a renomear".into());
    }
    let name = names::check_name(name).map_err(|why| format!("Nome de pasta inválido: {why}"))?;
    let to = join(parent(&path), &name);
    if to == path {
        return Ok(());
    }
    if !names::same_name(&to, &path) && view.taken(parent(&path), &name) {
        return Err(format!(
            "Já existe {name} {}; escolha outro nome",
            place(parent(&path))
        ));
    }
    check_lengths(view, &path, &to)?;
    view.plan_move(&path, &to);
    follow(project, &path, &to);
    Ok(())
}

fn move_items(
    project: &mut Project,
    view: &mut TreeView,
    items: &[String],
    to: &str,
    kind: Option<ItemKind>,
) -> Result<(), String> {
    if items.is_empty() {
        return Err("Informe items: os arquivos e pastas a mover".into());
    }
    let destination = ensure_folder(view, to)?;
    for text in items {
        match item(project, view, text, kind, true)? {
            Item::Registered(id) => {
                let asset = project.asset(&id).expect("found");
                if !made_at_save(asset) {
                    return Err(missing(asset));
                }
                if names::same_name(parent(&asset.path), &destination) {
                    continue;
                }
                let to_path = join(&destination, file_name(&asset.path));
                if view.taken(&destination, file_name(&asset.path)) {
                    return Err(format!(
                        "Já existe {} {}; renomeie um dos dois antes",
                        file_name(&to_path),
                        place(&destination)
                    ));
                }
                names::check_path(&to_path).map_err(|why| format!("Caminho inválido: {why}"))?;
                let asset = project
                    .assets
                    .iter_mut()
                    .find(|asset| asset.id == id)
                    .expect("found");
                asset.path = to_path;
            }
            Item::File { path, entry, .. } => {
                let name = file_name(&path).to_owned();
                let to_path = join(&destination, &name);
                if names::same_name(parent(&path), &destination) {
                    continue;
                }
                if entry == Entry::Folder && under(&destination, &path).is_some() {
                    return Err(format!("A pasta {path} não pode ir para dentro dela mesma"));
                }
                if view.taken(&destination, &name) {
                    let free = free_name(project, view, &destination, &name, &[]);
                    return Err(format!(
                        "Já existe {name} {}; a OXY não grava por cima: renomeie um dos dois \
                         antes ({free} está livre lá)",
                        place(&destination)
                    ));
                }
                check_lengths(view, &path, &to_path)?;
                view.plan_move(&path, &to_path);
                follow(project, &path, &to_path);
            }
        }
    }
    Ok(())
}

/// Whether the asset `asset`, whose file the tree lacks, moves or is renamed by its path alone:
/// a model not saved yet, or an image whose file the next save makes (a copy of another); an
/// image whose file is really missing stays missing at its new path.
fn made_at_save(asset: &crate::document::Asset) -> bool {
    match asset.kind {
        AssetKind::Model => asset.model.is_some(),
        AssetKind::Texture => true,
        AssetKind::Audio => false,
    }
}

/// Why the asset `asset`, whose file the tree lacks, cannot move or be renamed.
fn missing(asset: &crate::document::Asset) -> String {
    format!(
        "O arquivo {} ({}, {}) não está na pasta do projeto; restaure-o ou tire o recurso do \
         projeto com delete_asset",
        asset.path,
        asset.name,
        asset_kind(asset.kind).0
    )
}

/// The files Windows makes in folders by itself, which deleting a folder sends to the trash.
fn system_file(name: &str) -> bool {
    name.eq_ignore_ascii_case("thumbs.db") || name.eq_ignore_ascii_case("desktop.ini")
}

fn delete_folder(
    project: &mut Project,
    view: &mut TreeView,
    folder_text: &str,
) -> Result<Vec<String>, String> {
    let path = folder(view, folder_text)?;
    if path.is_empty() {
        return Err("Informe a pasta a excluir; a raiz do projeto não sai".into());
    }
    let mut told = Vec::new();
    let mut inside: Vec<(String, Option<Entry>)> = view
        .children(&path)
        .into_iter()
        .map(|(child, entry)| (child, Some(entry)))
        .collect();
    inside.extend(
        view.hidden(&path)
            .into_iter()
            .map(|name| (join(&path, &name), None)),
    );
    // Names the hidden entries take at the root, which the view does not list.
    let mut given: Vec<String> = Vec::new();
    for (child, entry) in inside {
        let name = file_name(&child).to_owned();
        if entry.is_none() && system_file(&name) {
            view.plan_trash(&child, None);
            continue;
        }
        let target = free_name(project, view, "", &name, &given);
        if target != name {
            told.push(format!(
                "{child} foi para a raiz do projeto como {target}, porque já havia {name} lá"
            ));
        } else if entry.is_none() {
            told.push(format!(
                "{child} fica fora do projeto e foi para a raiz do projeto"
            ));
        }
        check_lengths(view, &child, &target)?;
        if entry.is_none() {
            given.push(target.clone());
        }
        view.plan_move(&child, &target);
        follow(project, &child, &target);
    }
    view.plan_remove_dir(&path);
    Ok(told)
}

fn rename_item(
    project: &mut Project,
    view: &mut TreeView,
    text: &str,
    name: &str,
    kind: Option<ItemKind>,
) -> Result<(), String> {
    let found = item(project, view, text, kind, false)?;
    let name = names::check_name(name).map_err(|why| format!("Nome inválido: {why}"))?;
    let (path, file, asset) = match found {
        Item::File {
            path,
            entry: Entry::File(file),
            asset,
        } => (path, Some(file), asset),
        Item::File { path, .. } => {
            return Err(format!("{path} não é um recurso nem um script do projeto"));
        }
        Item::Registered(id) => {
            let asset = project.asset(&id).expect("found");
            if !made_at_save(asset) {
                return Err(missing(asset));
            }
            (asset.path.clone(), None, Some(id))
        }
    };
    if let Some(id) = &asset {
        let renamed = project.asset(id).expect("found");
        // Assets are found by name, which two of a kind would share.
        if let Some(other) = project.assets.iter().find(|a| {
            a.id != *id
                && a.kind == renamed.kind
                && a.name.trim().to_lowercase() == name.to_lowercase()
        }) {
            return Err(format!(
                "Já existe {} com o nome \"{}\"; escolha outro nome",
                asset_kind(other.kind).1,
                other.name
            ));
        }
    } else if file != Some(FileKind::Script) {
        return Err(format!("{path} não é um recurso nem um script do projeto"));
    }
    let (_, extension) = split_extension(file_name(&path));
    let to = join(parent(&path), &format!("{name}{extension}"));
    if to == path {
        return Ok(());
    }
    if file == Some(FileKind::Script) {
        if let Some(why) = script::path_problem(&to) {
            return Err(format!("Nome de script inválido: {why}"));
        }
    } else {
        names::check_path(&to).map_err(|why| format!("Nome inválido: {why}"))?;
    }
    if !names::same_name(&to, &path) && view.taken(parent(&path), file_name(&to)) {
        return Err(format!(
            "Já existe {} {}; escolha outro nome",
            file_name(&to),
            place(parent(&path))
        ));
    }
    if file.is_some() {
        view.plan_move(&path, &to);
    }
    // A model not saved yet, or a copy of an image, only changes its path: its file is made
    // there at the next save.
    follow(project, &path, &to);
    Ok(())
}

fn delete_item(
    project: &mut Project,
    view: &mut TreeView,
    text: &str,
    kind: Option<ItemKind>,
    detach: bool,
) -> Result<(), String> {
    match item(project, view, text, kind, false)? {
        Item::Registered(id) => {
            refuse_used(project, &id)?;
            crate::editing::remove_asset(project, &id)
        }
        Item::File {
            path,
            entry: Entry::File(FileKind::Script),
            ..
        } => {
            let users: Vec<String> = journal::entities_mut(project)
                .filter(|entity| {
                    entity
                        .scripts
                        .iter()
                        .any(|script| names::same_name(&script.path, &path))
                })
                .map(|entity| entity.name.clone())
                .collect();
            if !users.is_empty() && !detach {
                let mut listed: Vec<String> = Vec::new();
                for name in &users {
                    if !listed.contains(name) {
                        listed.push(name.clone());
                    }
                }
                super::first_ten(&mut listed);
                return Err(format!(
                    "O script {path} está anexado a {} {} ({}); passe detach: true para tirá-lo \
                     deles e excluí-lo",
                    users.len(),
                    if users.len() == 1 {
                        "objeto"
                    } else {
                        "objetos"
                    },
                    listed.join(", ")
                ));
            }
            for entity in journal::entities_mut(project) {
                entity
                    .scripts
                    .retain(|script| !names::same_name(&script.path, &path));
            }
            view.plan_trash(&path, None);
            Ok(())
        }
        Item::File {
            path,
            asset: Some(id),
            ..
        } => {
            refuse_used(project, &id)?;
            crate::editing::remove_asset(project, &id)?;
            view.plan_trash(&path, Some(id));
            Ok(())
        }
        Item::File { path, .. } => Err(format!(
            "{path} não é um recurso nem um script do projeto; pastas saem com delete_folder"
        )),
    }
}
