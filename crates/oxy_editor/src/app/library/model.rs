//! What the Library lists, made from the project and the last walk of its tree without reading
//! any file: each image, sound, model and script with its folder, its state and how it is used,
//! the folders with their counts, and the files OXY does not use, by folder. Pure, so the rules
//! (tree order, counts, filters, the search, the states, the texts) are tested without a window.
use oxy_core::{
    document::{AssetKind, Id, Layout, Project, Value},
    edit_history::FileOp,
    names,
    persistence::AssetState,
    project_tree::{FileKind, Tree},
};
use std::collections::{BTreeMap, BTreeSet};

/// What an item of the Library is, as its chips filter it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Image,
    Sound,
    Model,
    Script,
}

impl Kind {
    pub(crate) fn of(kind: AssetKind) -> Self {
        match kind {
            AssetKind::Texture => Self::Image,
            AssetKind::Audio => Self::Sound,
            AssetKind::Model => Self::Model,
        }
    }

    /// The item's type, as its row says it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Image => "Imagem",
            Self::Sound => "Som",
            Self::Model => "Modelo",
            Self::Script => "Script",
        }
    }

    /// The chip that shows only items of this kind.
    pub(crate) fn chip(self) -> &'static str {
        match self {
            Self::Image => "Imagens",
            Self::Sound => "Sons",
            Self::Model => "Modelos",
            Self::Script => "Scripts",
        }
    }

    /// How messages name an item of this kind: "o som", "a imagem".
    pub(crate) fn article(self) -> &'static str {
        match self {
            Self::Image => "a imagem",
            Self::Sound => "o som",
            Self::Model => "o modelo",
            Self::Script => "o script",
        }
    }

    pub(crate) fn command(self) -> oxy_core::command::ItemKind {
        use oxy_core::command::ItemKind;
        match self {
            Self::Image => ItemKind::Texture,
            Self::Sound => ItemKind::Audio,
            Self::Model => ItemKind::Model,
            Self::Script => ItemKind::Script,
        }
    }

    fn file(self) -> FileKind {
        match self {
            Self::Image => FileKind::Image,
            Self::Sound => FileKind::Sound,
            Self::Model => FileKind::Model,
            Self::Script => FileKind::Script,
        }
    }
}

/// What a row of the Library is: an image, sound or model by its asset's ID, a script by its
/// path, or a folder of the tree by its path ("" is the project's root).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Key {
    Asset(Id),
    Script(String),
    Folder(String),
}

/// How an item's file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Ok,
    /// It does not compile (a script), or its file holds nothing OXY can use: why.
    Error(String),
    /// Its file is not in the project's folder.
    Missing,
    /// Its file cannot be read now: why.
    Unreadable(String),
    /// Its saved path names no file OXY reads: why.
    InvalidPath(String),
    /// Its file is made at the next save (a model saved from the scene, a copy of an image).
    Unsaved,
}

impl State {
    /// Whether the row shows it as a problem with the file, in yellow.
    pub(crate) fn warns(&self) -> bool {
        matches!(
            self,
            Self::Missing | Self::Unreadable(_) | Self::InvalidPath(_)
        )
    }
}

/// An item of the Library.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) key: Key,
    pub(crate) kind: Kind,
    /// As the Library shows it: the file's name without the extension of its kind.
    pub(crate) name: String,
    /// From the project's folder, with `/`; the path its file has or will have.
    pub(crate) path: String,
    /// The folder of the tree it is listed in ("" is the root).
    pub(crate) folder: String,
    pub(crate) state: State,
    /// Found new in the project's folder since it opened, outside OXY.
    pub(crate) new: bool,
    /// How the project uses it: "6 objetos", "12 colocados", "tocado por checkpoint.luau".
    pub(crate) usage: String,
    /// The line where a script fails, from 1 (0 when it names none), when it does.
    pub(crate) error_line: Option<u32>,
}

impl Item {
    /// The name of its file.
    pub(crate) fn file_name(&self) -> &str {
        file_name(&self.path)
    }
}

/// A folder of the tree as the PASTAS column lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Folder {
    pub(crate) path: String,
    pub(crate) name: String,
    /// From the project's root, which is 0.
    pub(crate) depth: usize,
    /// The items directly in it.
    pub(crate) direct: usize,
    /// The items in it and in its subfolders.
    pub(crate) total: usize,
    /// Whether it has subfolders.
    pub(crate) children: bool,
}

/// What the scripts' check knows of a script's file (see `ScriptStatus`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScriptFacts {
    /// Where it fails to compile or load (its line, 0 when the error names none) and why.
    pub(crate) error: Option<(u32, String)>,
    /// Why the file cannot be a script now, said after its path.
    pub(crate) unreadable: Option<String>,
    /// The names of the sounds it plays with a literal text (`audio.play("Bipe")`).
    pub(crate) sounds: Vec<String>,
}

/// What the Library is made from.
pub(crate) struct Sources<'a> {
    pub(crate) project: &'a Project,
    /// The tree as the last walk found it, in a project in folders whose walk worked.
    pub(crate) tree: Option<&'a Tree>,
    /// The project's scripts by path, with what their check found (scripts are in the tree in
    /// both layouts).
    pub(crate) scripts: &'a BTreeMap<String, ScriptFacts>,
    /// The scripts objects attach that the project's folder lacks.
    pub(crate) missing_scripts: &'a BTreeSet<String>,
    /// The scripts objects attach whose `require`s name each script, by path.
    pub(crate) required_by: &'a dyn Fn(&str) -> Vec<String>,
    /// What opening told of each asset with a problem.
    pub(crate) report: &'a BTreeMap<Id, AssetState>,
    /// What was found new in the folder since the project opened.
    pub(crate) new: &'a BTreeSet<Key>,
    /// Whether the image has pixels not saved yet (its file may not exist yet).
    pub(crate) in_memory: &'a dyn Fn(&str) -> bool,
}

/// The Library of a project.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Library {
    pub(crate) items: Vec<Item>,
    /// The tree's folders, each before what it holds, in tree order, the root first; none in
    /// the old layout or without a walk.
    pub(crate) folders: Vec<Folder>,
    /// The names of the files OXY does not use, by folder, in tree order.
    pub(crate) others: BTreeMap<String, Vec<String>>,
}

/// The folder `path` is in ("" at the root).
pub(crate) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The last name of `path`.
pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// `prefix/name`, or `name` at the root.
pub(crate) fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// What is left of `path` under the folder `prefix` ("" for `prefix` itself), in any case.
fn under<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if names::same_name(path, prefix) {
        return Some("");
    }
    let cut = prefix.len();
    (path.len() > cut
        && path.is_char_boundary(cut)
        && path.as_bytes()[cut] == b'/'
        && names::same_name(&path[..cut], prefix))
    .then(|| &path[cut + 1..])
}

/// Where `path` is once `files` are done: moved with each move of it or of a folder it is in;
/// none when one sends it to the project's trash.
pub(crate) fn moved(path: &str, files: &[FileOp]) -> Option<String> {
    let mut now = path.to_owned();
    for file in files {
        match file {
            FileOp::Move { from, to } => {
                if let Some(rest) = under(&now, from) {
                    now = if rest.is_empty() {
                        to.clone()
                    } else {
                        join(to, rest)
                    };
                }
            }
            FileOp::Trash { from, .. } if under(&now, from).is_some() => return None,
            _ => {}
        }
    }
    Some(now)
}

/// The script `path` as the Library names it: its file's name without `.luau`.
pub(crate) fn script_name(path: &str) -> &str {
    let file = file_name(path);
    file.strip_suffix(".luau").unwrap_or(file)
}

/// "1 objeto", "6 objetos": `count` of `one`, plural `many`.
pub(crate) fn count(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// `names` as a sentence: "A", "A e B", "A, B e C", the first two and how many more past three.
pub(crate) fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} e {second}"),
        [first, second, third] => format!("{first}, {second} e {third}"),
        [first, second, rest @ ..] => format!("{first}, {second} e mais {}", rest.len()),
    }
}

/// How the project uses each asset, from one pass over its objects, in scenes and models.
#[derive(Default)]
struct Uses {
    /// The objects whose material or interface shows each image.
    images: BTreeMap<Id, BTreeSet<Id>>,
    /// How many times each model is placed (each placement's root).
    placed: BTreeMap<Id, usize>,
    /// The objects whose logic names each asset (a sound a node plays), by name.
    nodes: BTreeMap<Id, Vec<String>>,
    /// How many objects attach each script.
    scripts: BTreeMap<String, usize>,
}

impl Uses {
    fn of(project: &Project) -> Self {
        let mut uses = Self::default();
        let models = project.assets.iter().filter_map(|a| a.model.as_deref());
        let groups = project
            .scenes
            .iter()
            .map(|scene| scene.entities.as_slice())
            .chain(models);
        for entities in groups {
            let placed: BTreeSet<&str> = entities
                .iter()
                .filter(|e| e.model_source.is_some())
                .map(|e| e.id.as_str())
                .collect();
            for entity in entities {
                for texture in entity
                    .material
                    .texture
                    .iter()
                    .chain(entity.ui.iter().filter_map(|u| u.texture.as_ref()))
                {
                    uses.images
                        .entry(texture.clone())
                        .or_default()
                        .insert(entity.id.clone());
                }
                if let Some(model) = &entity.model_source
                    && entity
                        .parent
                        .as_deref()
                        .is_none_or(|parent| !placed.contains(parent))
                {
                    *uses.placed.entry(model.clone()).or_default() += 1;
                }
                for node in &entity.graph.nodes {
                    for value in node.params.values() {
                        if let Value::Text(id) = value {
                            let names = uses.nodes.entry(id.clone()).or_default();
                            if !names.contains(&entity.name) {
                                names.push(entity.name.clone());
                            }
                        }
                    }
                }
                for script in &entity.scripts {
                    *uses.scripts.entry(script.path.clone()).or_default() += 1;
                }
            }
        }
        uses
    }
}

/// Why the asset of `kind` saved at `path` names no file OXY reads, by the path alone: what
/// Windows would read otherwise, or the extension of another kind.
fn path_problem(path: &str, kind: Kind) -> Option<String> {
    if let Err(why) = names::check_path(path) {
        return Some(why);
    }
    (FileKind::of(file_name(path)) != kind.file()).then(|| {
        format!(
            "{path} não tem a extensão de {}",
            match kind {
                Kind::Image => "uma imagem (.png)",
                Kind::Sound => "um som (.wav)",
                Kind::Model => "um modelo (.modelo.json)",
                Kind::Script => "um script (.luau)",
            }
        )
    })
}

impl Library {
    /// The Library of `sources`.
    pub(crate) fn new(sources: &Sources<'_>) -> Self {
        let project = sources.project;
        let uses = Uses::of(project);
        let tree = sources.tree.filter(|_| project.layout == Layout::Tree);
        let folders: BTreeSet<&str> = tree
            .iter()
            .flat_map(|tree| tree.folders.iter().map(String::as_str))
            .collect();
        // Listed in the folder their path names, or at the root when the tree lacks it.
        let folder_of = |path: &str| {
            let folder = parent(path);
            if folders.contains(folder) {
                folder.to_owned()
            } else {
                String::new()
            }
        };
        let file_at = |path: &str| {
            tree.and_then(|tree| {
                tree.files
                    .iter()
                    .find(|f| f.path == path)
                    .or_else(|| tree.files.iter().find(|f| names::same_name(&f.path, path)))
            })
        };
        let mut items = Vec::new();
        for asset in &project.assets {
            let kind = Kind::of(asset.kind);
            let shared = project.assets.iter().find(|other| {
                other.id != asset.id
                    && !asset.path.is_empty()
                    && names::same_name(&other.path, &asset.path)
            });
            let state = match sources.report.get(&asset.id) {
                Some(AssetState::Broken(why)) => State::Error(why.clone()),
                _ if tree.is_none() => match sources.report.get(&asset.id) {
                    Some(AssetState::Missing) => State::Missing,
                    Some(AssetState::Unreadable(why)) => State::Unreadable(why.clone()),
                    Some(AssetState::InvalidPath(why)) => State::InvalidPath(why.clone()),
                    _ => State::Ok,
                },
                _ if asset.path.is_empty() => State::Unsaved,
                _ => {
                    if let Some(why) = path_problem(&asset.path, kind) {
                        State::InvalidPath(why)
                    } else if let Some(other) = shared {
                        State::InvalidPath(format!(
                            "{} também é o arquivo de {}",
                            asset.path, other.name
                        ))
                    } else {
                        match file_at(&asset.path) {
                            Some(file) => match &file.stamp {
                                Err(why) => State::Unreadable(why.clone()),
                                Ok(_) if kind == Kind::Model && asset.model.is_none() => {
                                    State::Error("o modelo está indisponível".into())
                                }
                                Ok(_) => State::Ok,
                            },
                            None if (kind == Kind::Model && asset.model.is_some())
                                || (kind == Kind::Image && (sources.in_memory)(&asset.id)) =>
                            {
                                State::Unsaved
                            }
                            None => State::Missing,
                        }
                    }
                }
            };
            let usage = match kind {
                Kind::Image => match uses.images.get(&asset.id).map_or(0, BTreeSet::len) {
                    0 => "não usada".to_owned(),
                    n => count(n, "objeto", "objetos"),
                },
                Kind::Model => match uses.placed.get(&asset.id).copied().unwrap_or(0) {
                    0 => "não colocado".to_owned(),
                    n => count(n, "colocado", "colocados"),
                },
                Kind::Sound => {
                    let mut by: Vec<String> =
                        uses.nodes.get(&asset.id).cloned().unwrap_or_default();
                    let wanted = asset.name.trim().to_lowercase();
                    by.extend(
                        sources
                            .scripts
                            .iter()
                            .filter(|(_, facts)| {
                                facts
                                    .sounds
                                    .iter()
                                    .any(|s| s.trim().to_lowercase() == wanted)
                            })
                            .map(|(path, _)| file_name(path).to_owned()),
                    );
                    if by.is_empty() {
                        "não usado".to_owned()
                    } else {
                        format!("tocado por {}", listed(&by))
                    }
                }
                Kind::Script => unreachable!("assets are no scripts"),
            };
            let path = asset.path.clone();
            items.push(Item {
                key: Key::Asset(asset.id.clone()),
                kind,
                name: asset.name.clone(),
                folder: folder_of(&path),
                path,
                state,
                new: sources.new.contains(&Key::Asset(asset.id.clone())),
                usage,
                error_line: None,
            });
        }
        let script_paths = sources
            .scripts
            .keys()
            .map(|path| (path, false))
            .chain(sources.missing_scripts.iter().map(|path| (path, true)));
        for (path, missing) in script_paths {
            if missing && sources.scripts.contains_key(path) {
                continue;
            }
            let facts = sources.scripts.get(path).cloned().unwrap_or_default();
            let state = if missing {
                // A path saved against the name rules names no file Windows would read.
                path_problem(path, Kind::Script).map_or(State::Missing, State::InvalidPath)
            } else if let Some(why) = facts.unreadable {
                State::Unreadable(why)
            } else if let Some((_, why)) = &facts.error {
                State::Error(why.clone())
            } else {
                State::Ok
            };
            let objects = uses.scripts.get(path).copied().unwrap_or(0);
            let mut usage = if objects > 0 {
                count(objects, "objeto", "objetos")
            } else {
                let requiring: Vec<String> = (sources.required_by)(path)
                    .iter()
                    .map(|path| file_name(path).to_owned())
                    .collect();
                if requiring.is_empty() {
                    "não usado".to_owned()
                } else {
                    format!("usado por {}", listed(&requiring))
                }
            };
            if facts.error.is_some() {
                usage.push_str(" · 1 erro");
            }
            items.push(Item {
                key: Key::Script(path.clone()),
                kind: Kind::Script,
                name: script_name(path).to_owned(),
                folder: folder_of(path),
                path: path.clone(),
                state,
                new: sources.new.contains(&Key::Script(path.clone())),
                usage,
                error_line: facts.error.as_ref().map(|(line, _)| *line),
            });
        }
        items.sort_by(|a, b| {
            names::name_order(&a.folder, &b.folder)
                .then_with(|| names::name_order(a.file_name(), b.file_name()))
                .then_with(|| a.kind.cmp(&b.kind))
        });
        let mut library = Self {
            items,
            folders: Vec::new(),
            others: BTreeMap::new(),
        };
        if let Some(tree) = tree {
            let root = Folder {
                path: String::new(),
                name: project.name.clone(),
                depth: 0,
                direct: 0,
                total: 0,
                children: false,
            };
            library.folders = std::iter::once(root)
                .chain(tree.folders.iter().map(|path| Folder {
                    name: file_name(path).to_owned(),
                    depth: path.split('/').count(),
                    path: path.clone(),
                    direct: 0,
                    total: 0,
                    children: false,
                }))
                .collect();
            for index in 0..library.folders.len() {
                let path = library.folders[index].path.clone();
                let folder = &mut library.folders[index];
                folder.direct = library.items.iter().filter(|i| i.folder == path).count();
                folder.total = library
                    .items
                    .iter()
                    .filter(|i| path.is_empty() || under(&i.folder, &path).is_some())
                    .count();
                folder.children = tree
                    .folders
                    .iter()
                    .any(|other| parent(other) == path && !other.is_empty());
            }
            for file in tree.files.iter().filter(|f| f.kind == FileKind::Other) {
                library
                    .others
                    .entry(parent(&file.path).to_owned())
                    .or_default()
                    .push(file_name(&file.path).to_owned());
            }
        }
        library
    }

    /// The item of `key`.
    pub(crate) fn item(&self, key: &Key) -> Option<&Item> {
        self.items.iter().find(|item| item.key == *key)
    }

    /// The folder `path` ("" is the root).
    pub(crate) fn folder(&self, path: &str) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.path == path)
    }

    /// The subfolders right inside `folder`, in tree order.
    pub(crate) fn subfolders(&self, folder: &str) -> Vec<&Folder> {
        self.folders
            .iter()
            .filter(|f| !f.path.is_empty() && parent(&f.path) == folder)
            .collect()
    }

    /// The items a view shows: those of `folder` (every item when none), or with `search` those
    /// of the whole project whose name has it, in any case; of `kind` only when given.
    pub(crate) fn shown(
        &self,
        folder: Option<&str>,
        search: &str,
        kind: Option<Kind>,
    ) -> Vec<&Item> {
        let search = search.trim().to_lowercase();
        self.items
            .iter()
            .filter(|item| {
                if !search.is_empty() {
                    item.name.to_lowercase().contains(&search)
                } else {
                    folder.is_none_or(|folder| item.folder == folder)
                }
            })
            .filter(|item| kind.is_none_or(|kind| item.kind == kind))
            .collect()
    }

    /// How many of `items` each chip counts: all of them, then each kind.
    pub(crate) fn chips(items: &[&Item]) -> [(Option<Kind>, usize); 5] {
        let of = |kind: Kind| items.iter().filter(|i| i.kind == kind).count();
        [
            (None, items.len()),
            (Some(Kind::Image), of(Kind::Image)),
            (Some(Kind::Sound), of(Kind::Sound)),
            (Some(Kind::Model), of(Kind::Model)),
            (Some(Kind::Script), of(Kind::Script)),
        ]
    }

    /// The files the person may mean for the item `item`, whose file went: the new ones a scan
    /// found that may be it (`near`), then those of its kind with its file's name in another
    /// folder; none that is the file of another asset something uses (`used`), at most five.
    pub(crate) fn suggestions(
        &self,
        item: &Item,
        near: &[String],
        tree: Option<&Tree>,
        used: &dyn Fn(&Key) -> bool,
    ) -> Vec<String> {
        let Some(tree) = tree else {
            return Vec::new();
        };
        let taken = |path: &str| {
            self.items.iter().any(|other| {
                other.key != item.key
                    && other.kind != Kind::Script
                    && names::same_name(&other.path, path)
                    && used(&other.key)
            })
        };
        let present = |path: &str| tree.files.iter().any(|f| f.path == path);
        let same_name = tree
            .files
            .iter()
            .filter(|f| {
                f.kind == item.kind.file()
                    && names::same_name(file_name(&f.path), item.file_name())
                    && !names::same_name(&f.path, &item.path)
            })
            .map(|f| &f.path);
        let mut found: Vec<String> = Vec::new();
        for path in near.iter().filter(|p| present(p)).chain(same_name) {
            if !taken(path) && !found.iter().any(|known| names::same_name(known, path)) {
                found.push(path.clone());
            }
        }
        found.truncate(5);
        found
    }
}

/// The folder `folder` as the breadcrumb and the folder column of a flat list show it:
/// "parkour-scripts › Pista › Checkpoints".
pub(crate) fn trail(project: &str, folder: &str) -> String {
    if folder.is_empty() {
        project.to_owned()
    } else {
        format!("{project} › {}", folder.replace('/', " › "))
    }
}

/// The line of the files OXY does not use in a folder: "2 outros arquivos (notas.txt,
/// pista.psd)", the first three named.
pub(crate) fn others_line(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(3).map(String::as_str).collect();
    let more = if names.len() > 3 { ", …" } else { "" };
    format!(
        "{} ({}{more})",
        count(names.len(), "outro arquivo", "outros arquivos"),
        shown.join(", ")
    )
}

/// What the row of `item` says of its state instead of its use, when it is not fine.
pub(crate) fn state_line(item: &Item) -> Option<String> {
    Some(match &item.state {
        State::Ok => return None,
        State::Error(why) if item.kind == Kind::Script => return Some(why.clone()),
        State::Error(why) => format!("erro: {why}"),
        State::Missing => format!("ausente: {} não está na pasta", item.path),
        State::Unreadable(why) => format!("ilegível: {why}"),
        State::InvalidPath(why) => format!("caminho inválido: {why}"),
        State::Unsaved => "não salvo: o arquivo nasce ao salvar o projeto".to_owned(),
    })
}

/// A click on the row `key` of the list `order`: alone it selects only it; with Ctrl it joins or
/// leaves the selection; with Shift the rows from the `anchor` to it are selected. Answers the
/// selection and the next anchor.
pub(crate) fn click(
    selection: &[Key],
    anchor: Option<&Key>,
    order: &[Key],
    key: &Key,
    ctrl: bool,
    shift: bool,
) -> (Vec<Key>, Key) {
    if shift
        && let Some(from) = anchor.and_then(|anchor| order.iter().position(|k| k == anchor))
        && let Some(to) = order.iter().position(|k| k == key)
    {
        let (start, end) = (from.min(to), from.max(to));
        return (
            order[start..=end].to_vec(),
            anchor.cloned().expect("anchor"),
        );
    }
    if ctrl {
        let mut next: Vec<Key> = selection
            .iter()
            .filter(|k| !matches!(k, Key::Folder(_)))
            .cloned()
            .collect();
        if let Some(at) = next.iter().position(|k| k == key) {
            next.remove(at);
        } else {
            next.push(key.clone());
        }
        return (next, key.clone());
    }
    (vec![key.clone()], key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::{
        document::{Asset, Entity, Primitive},
        fs_ops::Stamp,
        graph::Node,
        project_tree::TreeFile,
        script::ScriptRef,
    };

    fn file(path: &str, unreadable: bool) -> TreeFile {
        TreeFile {
            path: path.into(),
            kind: FileKind::of(file_name(path)),
            stamp: if unreadable {
                Err("não pôde ser lido agora: está aberto em outro programa".into())
            } else {
                Ok(Stamp {
                    len: 10,
                    modified: None,
                })
            },
        }
    }

    /// The parkour as the prototype shows it: Pista with the script contagem, the model
    /// Ladrilho que cai, the sound Bipe and the missing sound Largada; Pista/Checkpoints with
    /// checkpoint.luau; Sons, Menu and an empty scripts folder; notes OXY does not use.
    fn parkour() -> (Project, Tree) {
        let mut project = Project::new("parkour-scripts");
        project.layout = Layout::Tree;
        let mut add = |id: &str, name: &str, path: &str, kind: AssetKind| {
            project.assets.push(Asset {
                id: id.into(),
                name: name.into(),
                path: path.into(),
                kind,
                model: (kind == AssetKind::Model).then(|| vec![Entity::new(name, None)]),
            });
        };
        add("bipe", "Bipe", "Pista/Bipe.wav", AssetKind::Audio);
        add("largada", "Largada", "Pista/Largada.wav", AssetKind::Audio);
        add(
            "ladrilho",
            "Ladrilho que cai",
            "Pista/Ladrilho que cai.modelo.json",
            AssetKind::Model,
        );
        add("pedra", "Pedra", "Imagens/Pedra.png", AssetKind::Texture);
        add("clique", "Clique", "Sons/Clique.wav", AssetKind::Audio);
        let scene = &mut project.scenes[0];
        let mut jogador = Entity::new("Jogador", Some(Primitive::Cube));
        jogador.material.texture = Some("pedra".into());
        let mut beep = Node::new("action.sound", [0.; 2]);
        beep.params
            .insert("asset".into(), Value::Text("bipe".into()));
        jogador.graph.nodes.push(beep);
        jogador.scripts = vec![ScriptRef::new("Pista/contagem.luau")];
        scene.entities.push(jogador);
        for n in 0..12 {
            let mut tile = Entity::new(format!("Ladrilho {n}"), None);
            tile.model_source = Some("ladrilho".into());
            let mut piece = Entity::new("Peça", None);
            piece.model_source = Some("ladrilho".into());
            piece.parent = Some(tile.id.clone());
            scene.entities.extend([tile, piece]);
        }
        let tree = Tree {
            folders: [
                "Imagens",
                "Menu",
                "Pista",
                "Pista/Checkpoints",
                "scripts",
                "Sons",
            ]
            .map(String::from)
            .to_vec(),
            files: [
                ("Imagens/Pedra.png", false),
                ("Menu/menu.luau", false),
                ("Pista/Checkpoints/checkpoint.luau", false),
                ("Pista/Bipe.wav", false),
                ("Pista/contagem.luau", false),
                ("Pista/Ladrilho que cai.modelo.json", false),
                ("Pista/pista.psd", false),
                ("Sons/Clique.wav", true),
                ("Sons/Largada.wav", false),
                ("notas.txt", false),
            ]
            .map(|(path, unreadable)| file(path, unreadable))
            .to_vec(),
            ..Tree::default()
        };
        (project, tree)
    }

    fn facts() -> BTreeMap<String, ScriptFacts> {
        BTreeMap::from([
            (
                "Pista/contagem.luau".to_owned(),
                ScriptFacts {
                    error: Some((7, "contagem.luau:7: esperava end".into())),
                    ..ScriptFacts::default()
                },
            ),
            (
                "Pista/Checkpoints/checkpoint.luau".to_owned(),
                ScriptFacts {
                    sounds: vec!["bipe".into(), "Clique".into()],
                    ..ScriptFacts::default()
                },
            ),
            ("Menu/menu.luau".to_owned(), ScriptFacts::default()),
        ])
    }

    fn library(project: &Project, tree: Option<&Tree>, new: &BTreeSet<Key>) -> Library {
        let scripts = facts();
        let missing = BTreeSet::from(["scripts/sumiu.luau".to_owned()]);
        let required = |path: &str| {
            if path == "Menu/menu.luau" {
                vec!["Pista/contagem.luau".to_owned()]
            } else {
                Vec::new()
            }
        };
        Library::new(&Sources {
            project,
            tree,
            scripts: &scripts,
            missing_scripts: &missing,
            required_by: &required,
            report: &BTreeMap::new(),
            new,
            in_memory: &|_| false,
        })
    }

    fn rows<'a>(items: &[&'a Item]) -> Vec<(&'a str, &'a str)> {
        items
            .iter()
            .map(|i| (i.name.as_str(), i.usage.as_str()))
            .collect()
    }

    #[test]
    fn folders_go_in_tree_order_with_their_counts() {
        let (project, tree) = parkour();
        let library = library(&project, Some(&tree), &BTreeSet::new());
        let folders: Vec<(&str, usize, usize, usize, bool)> = library
            .folders
            .iter()
            .map(|f| (f.name.as_str(), f.depth, f.direct, f.total, f.children))
            .collect();
        assert_eq!(
            folders,
            [
                ("parkour-scripts", 0, 0, 9, true),
                ("Imagens", 1, 1, 1, false),
                ("Menu", 1, 1, 1, false),
                ("Pista", 1, 4, 5, true),
                ("Checkpoints", 2, 1, 1, false),
                ("scripts", 1, 1, 1, false),
                ("Sons", 1, 1, 1, false),
            ]
        );
        assert_eq!(
            library
                .subfolders("Pista")
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            ["Pista/Checkpoints"]
        );
        assert_eq!(library.others["Pista"], ["pista.psd"]);
        assert_eq!(library.others[""], ["notas.txt"]);
        assert_eq!(
            others_line(&library.others["Pista"]),
            "1 outro arquivo (pista.psd)"
        );
        let many: Vec<String> = ["a.txt", "b.psd", "c.md", "d.zip"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            others_line(&many),
            "4 outros arquivos (a.txt, b.psd, c.md, …)"
        );
        assert_eq!(trail("parkour-scripts", ""), "parkour-scripts");
        assert_eq!(
            trail("parkour-scripts", "Pista/Checkpoints"),
            "parkour-scripts › Pista › Checkpoints"
        );
    }

    #[test]
    fn each_item_says_its_state_and_how_it_is_used() {
        let (project, tree) = parkour();
        let new = BTreeSet::from([Key::Script("Menu/menu.luau".into())]);
        let library = library(&project, Some(&tree), &new);
        let pista = library.shown(Some("Pista"), "", None);
        assert_eq!(
            rows(&pista),
            [
                ("Bipe", "tocado por Jogador e checkpoint.luau"),
                ("contagem", "1 objeto · 1 erro"),
                ("Ladrilho que cai", "12 colocados"),
                ("Largada", "não usado"),
            ]
        );
        let state = |name: &str| {
            library
                .items
                .iter()
                .find(|i| i.name == name)
                .map(|i| i.state.clone())
                .unwrap()
        };
        assert_eq!(state("Bipe"), State::Ok);
        assert_eq!(
            state("contagem"),
            State::Error("contagem.luau:7: esperava end".into())
        );
        assert_eq!(state("Largada"), State::Missing);
        assert!(matches!(state("Clique"), State::Unreadable(_)));
        assert_eq!(state("sumiu"), State::Missing);
        let largada = library.item(&Key::Asset("largada".into())).unwrap();
        assert_eq!(
            state_line(largada).as_deref(),
            Some("ausente: Pista/Largada.wav não está na pasta")
        );
        let contagem = library
            .item(&Key::Script("Pista/contagem.luau".into()))
            .unwrap();
        assert_eq!(contagem.error_line, Some(7));
        // A module no object attaches says who requires it; a new file is marked.
        let menu = library.item(&Key::Script("Menu/menu.luau".into())).unwrap();
        assert_eq!(menu.usage, "usado por contagem.luau");
        assert!(menu.new && !contagem.new);
        assert_eq!(
            library.item(&Key::Asset("pedra".into())).unwrap().usage,
            "1 objeto"
        );
        assert_eq!(
            library.item(&Key::Asset("clique".into())).unwrap().usage,
            "tocado por checkpoint.luau"
        );
        // A missing script is listed where its path says, or at the root without that folder.
        assert_eq!(
            library
                .item(&Key::Script("scripts/sumiu.luau".into()))
                .unwrap()
                .folder,
            "scripts"
        );
    }

    #[test]
    fn paths_saved_wrong_and_files_not_saved_yet_are_told() {
        let (mut project, tree) = parkour();
        project.assets.push(Asset {
            id: "torre".into(),
            name: "Torre".into(),
            path: "Torre.modelo.json".into(),
            kind: AssetKind::Model,
            model: Some(vec![Entity::new("Torre", None)]),
        });
        project.assets.push(Asset {
            id: "con".into(),
            name: "con".into(),
            path: "Sons/con.wav".into(),
            kind: AssetKind::Audio,
            model: None,
        });
        project.assets.push(Asset {
            id: "errado".into(),
            name: "Errado".into(),
            path: "Sons/Errado.png".into(),
            kind: AssetKind::Audio,
            model: None,
        });
        let library = library(&project, Some(&tree), &BTreeSet::new());
        let state = |id: &str| library.item(&Key::Asset(id.into())).unwrap().state.clone();
        assert_eq!(state("torre"), State::Unsaved);
        assert!(matches!(state("con"), State::InvalidPath(why) if why.contains("reserva")));
        assert_eq!(
            state("errado"),
            State::InvalidPath("Sons/Errado.png não tem a extensão de um som (.wav)".into())
        );
        // Without a walk (the old layout), what opening told is the state.
        let mut old = project.clone();
        old.layout = Layout::Legacy;
        let report = BTreeMap::from([("bipe".to_owned(), AssetState::Missing)]);
        let scripts = BTreeMap::new();
        let flat = Library::new(&Sources {
            project: &old,
            tree: Some(&tree),
            scripts: &scripts,
            missing_scripts: &BTreeSet::new(),
            required_by: &|_| Vec::new(),
            report: &report,
            new: &BTreeSet::new(),
            in_memory: &|_| false,
        });
        assert!(flat.folders.is_empty() && flat.others.is_empty());
        let state = |id: &str| flat.item(&Key::Asset(id.into())).unwrap().state.clone();
        assert_eq!(state("bipe"), State::Missing);
        assert_eq!(state("largada"), State::Ok);
        assert_eq!(
            flat.items.iter().filter(|i| i.folder.is_empty()).count(),
            flat.items.len()
        );
        // An attached script saved against the name rules, and one in a folder that cannot be
        // read, are listed with why.
        let scripts = BTreeMap::from([(
            "Trancada/porta.luau".to_owned(),
            ScriptFacts {
                unreadable: Some("não pôde ser lido agora: a pasta Trancada não abre".into()),
                ..ScriptFacts::default()
            },
        )]);
        let missing = BTreeSet::from(["scripts/con.luau".to_owned()]);
        let attached = Library::new(&Sources {
            project: &project,
            tree: Some(&tree),
            scripts: &scripts,
            missing_scripts: &missing,
            required_by: &|_| Vec::new(),
            report: &BTreeMap::new(),
            new: &BTreeSet::new(),
            in_memory: &|_| false,
        });
        let state = |path: &str| {
            attached
                .item(&Key::Script(path.into()))
                .unwrap()
                .state
                .clone()
        };
        assert!(
            matches!(state("scripts/con.luau"), State::InvalidPath(why) if why.contains("reserva"))
        );
        assert!(matches!(state("Trancada/porta.luau"), State::Unreadable(_)));
    }

    #[test]
    fn chips_filter_the_folder_and_the_search_looks_everywhere() {
        let (project, tree) = parkour();
        let library = library(&project, Some(&tree), &BTreeSet::new());
        let pista = library.shown(Some("Pista"), "", None);
        assert_eq!(
            Library::chips(&pista),
            [
                (None, 4),
                (Some(Kind::Image), 0),
                (Some(Kind::Sound), 2),
                (Some(Kind::Model), 1),
                (Some(Kind::Script), 1),
            ]
        );
        let sounds = library.shown(Some("Pista"), "", Some(Kind::Sound));
        assert_eq!(
            sounds.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["Bipe", "Largada"]
        );
        // Every item of the project, flat.
        assert_eq!(library.shown(None, "", None).len(), 9);
        // The search looks in the whole project, whatever the folder.
        let found = library.shown(Some("Menu"), " CHECK", None);
        assert_eq!(
            found
                .iter()
                .map(|i| (i.name.as_str(), i.folder.as_str()))
                .collect::<Vec<_>>(),
            [("checkpoint", "Pista/Checkpoints")]
        );
        assert!(library.shown(Some("Pista"), "e", Some(Kind::Image)).len() == 1);
    }

    #[test]
    fn a_missing_file_gets_the_files_that_may_be_it() {
        let (project, tree) = parkour();
        let library = library(&project, Some(&tree), &BTreeSet::new());
        let largada = library.item(&Key::Asset("largada".into())).unwrap();
        // Sons/Largada.wav has its name in another folder.
        assert_eq!(
            library.suggestions(largada, &[], Some(&tree), &|_| false),
            ["Sons/Largada.wav"]
        );
        // A new file a scan found comes first; one gone since does not.
        assert_eq!(
            library.suggestions(
                largada,
                &["Sons/Clique.wav".into(), "Sons/Sumido.wav".into()],
                Some(&tree),
                &|_| false
            ),
            ["Sons/Clique.wav", "Sons/Largada.wav"]
        );
        // The file of another sound in use is no suggestion.
        assert_eq!(
            library.suggestions(
                largada,
                &["Sons/Clique.wav".into()],
                Some(&tree),
                &|key| *key == Key::Asset("clique".into())
            ),
            ["Sons/Largada.wav"]
        );
        assert!(
            library
                .suggestions(largada, &[], None, &|_| false)
                .is_empty()
        );
    }

    #[test]
    fn clicks_select_one_toggle_with_ctrl_and_range_with_shift() {
        let order: Vec<Key> = ["a", "b", "c", "d"].map(|k| Key::Script(k.into())).to_vec();
        let key = |k: &str| Key::Script(k.into());
        let (selection, anchor) = click(&[], None, &order, &key("b"), false, false);
        assert_eq!(
            (selection.clone(), anchor.clone()),
            (vec![key("b")], key("b"))
        );
        let (selection, anchor) = click(&selection, Some(&anchor), &order, &key("d"), true, false);
        assert_eq!(selection, [key("b"), key("d")]);
        let (selection, _) = click(&selection, Some(&anchor), &order, &key("b"), true, false);
        assert_eq!(selection, [key("d")]);
        let (selection, anchor) =
            click(&selection, Some(&key("a")), &order, &key("c"), false, true);
        assert_eq!(
            (selection, anchor),
            (vec![key("a"), key("b"), key("c")], key("a"))
        );
        // A folder selected leaves when Ctrl adds an item.
        let (selection, _) = click(
            &[Key::Folder("Pista".into())],
            None,
            &order,
            &key("a"),
            true,
            false,
        );
        assert_eq!(selection, [key("a")]);
    }

    #[test]
    fn paths_follow_the_moves_of_a_step() {
        let files = vec![
            FileOp::Move {
                from: "Pista".into(),
                to: "Fases/Pista".into(),
            },
            FileOp::Move {
                from: "Fases/Pista/porta.luau".into(),
                to: "Fases/Pista/Porta.luau".into(),
            },
            FileOp::Trash {
                from: "Sons/Bip.wav".into(),
                slot: ".oxy/lixeira/1-1.wav".into(),
                id: None,
            },
        ];
        assert_eq!(
            moved("pista/porta.luau", &files).as_deref(),
            Some("Fases/Pista/Porta.luau")
        );
        assert_eq!(moved("Pista", &files).as_deref(), Some("Fases/Pista"));
        assert_eq!(
            moved("Pistas/x.luau", &files).as_deref(),
            Some("Pistas/x.luau")
        );
        assert_eq!(moved("Sons/Bip.wav", &files), None);
        assert_eq!(count(1, "objeto", "objetos"), "1 objeto");
        assert_eq!(count(6, "colocado", "colocados"), "6 colocados");
        let names: Vec<String> = ["A", "B", "C", "D", "E"].map(String::from).to_vec();
        assert_eq!(listed(&names[..1]), "A");
        assert_eq!(listed(&names[..3]), "A, B e C");
        assert_eq!(listed(&names), "A, B e mais 3");
        assert_eq!(script_name("Pista/contagem.luau"), "contagem");
    }
}
