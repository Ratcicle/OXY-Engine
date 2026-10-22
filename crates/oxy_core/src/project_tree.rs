//! The project's tree as OXY reads it: one walk of the project's folder that the scripts, the
//! copies of a project, the export and the Library share, so they all see the same files. What
//! starts with a dot is outside (`.oxy`, `.vscode`, `.git`, `.luaurc`, the temporary files of a
//! save), and so are the project's own files at the root, the leftovers of Windows and of text
//! editors, links and junctions (never followed) and the subfolders that are another project or
//! an exported game. The rest is listed in tree order, each file with its kind by its extension
//! and its stamp from the folder's listing. A file that went is no file; one that cannot be read
//! now (another program holds it, no permission) stays, with why, and so does a folder that
//! cannot be listed now.
use crate::{
    export::MARKER_FILE,
    fs_ops::{self, Stamp},
    names,
    persistence::{PROJECT_FILE, PersistenceError},
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Deepest folder of a project, counted from its root.
pub const MAX_DEPTH: usize = 16;
/// Most files and folders a project's tree holds.
pub const MAX_ENTRIES: usize = 20_000;
/// How the reason a file cannot be read now starts (another program holds it, it lacks
/// permission…), as opposed to one that lasts until the file changes.
pub const NOT_NOW: &str = "não pôde ser lido agora";
/// Why a link or junction is outside the tree, said after its path.
const LINK: &str = "é um atalho (link ou junção), e a OXY não segue atalhos";
/// Why a subfolder that is another project or an exported game is outside the tree, said after
/// its path.
const NESTED: &str = "é outro projeto ou um jogo exportado";

/// What a file of the tree is, by its extension: `.png`, `.wav` and `.modelo.json` in any case,
/// `.luau` only in lower case (`require` adds it so, and would never find a `.LUAU`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Sound,
    Model,
    Script,
    /// A file OXY does not use, which goes along when its folder moves.
    Other,
}

impl FileKind {
    pub fn of(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        let ends =
            |text: &str, extension: &str| text.len() > extension.len() && text.ends_with(extension);
        if ends(name, ".luau") {
            Self::Script
        } else if ends(&lower, ".modelo.json") {
            Self::Model
        } else if ends(&lower, ".png") {
            Self::Image
        } else if ends(&lower, ".wav") {
            Self::Sound
        } else {
            Self::Other
        }
    }
}

/// A file of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeFile {
    /// From the project's folder, with `/`, in the case the disk has.
    pub path: String,
    pub kind: FileKind,
    /// Its size and write time as the folder's listing tells them, or why it cannot be read now.
    pub stamp: Result<Stamp, String>,
}

/// What the walk found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    /// The folders, each before what it holds, in tree order.
    pub folders: Vec<String>,
    /// The files, a folder's after its subfolders', in tree order.
    pub files: Vec<TreeFile>,
    /// The folders that could not be listed now, with why after their name: what they hold is
    /// unknown.
    pub unreadable: BTreeMap<String, String>,
    /// The links and junctions and the subfolders that are another project or an exported game,
    /// which the walk did not go into, with why after their path.
    pub skipped: BTreeMap<String, String>,
    /// What the walk left out that someone may count on, one line each, in tree order within a
    /// folder: names that are not UTF-8, links, scripts whose extension is not in lower case,
    /// names with an accent apart from its letter, folders it could not list.
    pub warnings: Vec<String>,
    /// The `.luaurc` and `.config.luau` files of subfolders, which luau-lsp reads before the
    /// root's.
    pub configs: Vec<String>,
}

impl Tree {
    /// The scripts: every `.luau` file.
    pub fn scripts(&self) -> impl Iterator<Item = &TreeFile> {
        self.files.iter().filter(|f| f.kind == FileKind::Script)
    }
}

/// Walks the project's folder `root` (an exported game's `data` folder too), holding the lock
/// the tree's operations share. Fails only when the folder itself cannot be listed or when it is
/// too large to be a project's ([`MAX_DEPTH`], [`MAX_ENTRIES`]): nothing is cut short.
pub fn walk(root: &Path) -> Result<Tree, PersistenceError> {
    walk_with(root, MAX_DEPTH, MAX_ENTRIES)
}

/// [`walk`] with other limits, for tests.
pub(crate) fn walk_with(
    root: &Path,
    max_depth: usize,
    max_entries: usize,
) -> Result<Tree, PersistenceError> {
    let _tree = fs_ops::tree_lock();
    let mut walk = Walk {
        tree: Tree::default(),
        entries: 0,
        max_depth,
        max_entries,
    };
    let listing = fs::read_dir(root).map_err(|source| PersistenceError::Io {
        context: "Não foi possível ler a pasta do projeto",
        source,
    })?;
    walk.folder(listing, "", 0)?;
    Ok(walk.tree)
}

struct Walk {
    tree: Tree,
    entries: usize,
    max_depth: usize,
    max_entries: usize,
}

impl Walk {
    fn folder(
        &mut self,
        listing: fs::ReadDir,
        prefix: &str,
        depth: usize,
    ) -> Result<(), PersistenceError> {
        let mut items = Vec::new();
        // Files whose name or kind cannot be read, after the others, and what is warned of, each
        // by its name: put in tree order once the listing ends.
        let mut odd = Vec::new();
        let mut notes = Vec::new();
        for entry in listing {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.cannot_list(prefix, &error);
                    break;
                }
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                let lossy = name.to_string_lossy().into_owned();
                let path = join(prefix, &lossy);
                if FileKind::of(&lossy) == FileKind::Script {
                    let file = TreeFile {
                        path,
                        kind: FileKind::Script,
                        stamp: Err("tem um nome que não é UTF-8; renomeie o arquivo".into()),
                    };
                    odd.push((lossy, file));
                } else {
                    let note = format!(
                        "{path}: o nome não é UTF-8, e o que ele é fica fora do projeto; renomeie-o"
                    );
                    notes.push((lossy, note));
                }
                continue;
            };
            if name.starts_with('.') {
                if depth > 0 && (name == ".luaurc" || name == ".config.luau") {
                    self.tree.configs.push(join(prefix, name));
                }
                continue;
            }
            if depth == 0 && project_file(name) {
                continue;
            }
            let path = join(prefix, name);
            match entry.file_type() {
                Ok(kind) if kind.is_symlink() => {
                    let note = format!(
                        "{path} é um atalho (link ou junção) e fica fora do projeto: a OXY não \
                         segue atalhos"
                    );
                    notes.push((name.to_owned(), note));
                    self.tree.skipped.insert(path, LINK.to_owned());
                }
                // What Windows and editors leave behind is files: a folder of that name is the
                // person's, as `presence` reads it too.
                Ok(kind) if kind.is_file() && leftover(name) => {}
                Ok(kind) if kind.is_dir() || kind.is_file() => {
                    items.push((name.to_owned(), kind.is_dir(), entry));
                }
                Ok(_) => {}
                Err(_) if leftover(name) => {}
                Err(error) => {
                    let file = TreeFile {
                        kind: FileKind::of(name),
                        stamp: Err(now_unreadable(&error)),
                        path,
                    };
                    odd.push((name.to_owned(), file));
                }
            }
        }
        // The system's order is the disk's (creation order on FAT): every decision, and every
        // warning, follows the tree's.
        items.sort_by(|a, b| names::tree_order((&a.0, a.1), (&b.0, b.1)));
        notes.sort_by(|a, b| names::name_order(&a.0, &b.0));
        odd.sort_by(|a, b| names::name_order(&a.0, &b.0));
        self.tree
            .warnings
            .extend(notes.into_iter().map(|(_, note)| note));
        for (name, is_dir, entry) in items {
            self.entries += 1;
            if self.entries > self.max_entries {
                return Err(PersistenceError::TreeTooLarge(format!(
                    "tem mais de {} arquivos e pastas",
                    self.max_entries
                )));
            }
            let path = join(prefix, &name);
            if names::has_loose_marks(&name) {
                self.tree.warnings.push(format!(
                    "{path}: o nome tem um acento separado da letra (como os nomes feitos no \
                     macOS); renomeie-o digitando a letra acentuada"
                ));
            }
            if !is_dir {
                if let Some(extension) = Path::new(&name).extension()
                    && extension.eq_ignore_ascii_case("luau")
                    && extension != "luau"
                {
                    self.tree.warnings.push(format!(
                        "{path}: renomeie para .luau, em minúsculas; só arquivos .luau são scripts"
                    ));
                }
                let stamp = match entry.metadata() {
                    Ok(meta) => Ok(Stamp::of(&meta)),
                    // It went in the meantime.
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => Err(now_unreadable(&error)),
                };
                self.tree.files.push(TreeFile {
                    kind: FileKind::of(&name),
                    path,
                    stamp,
                });
                continue;
            }
            if depth + 1 > self.max_depth {
                return Err(PersistenceError::TreeTooLarge(format!(
                    "tem pastas mais de {} níveis umas dentro das outras ({path})",
                    self.max_depth
                )));
            }
            let folder = entry.path();
            if nested(&folder) {
                self.tree.skipped.insert(path, NESTED.to_owned());
                continue;
            }
            match fs::read_dir(&folder) {
                Ok(listing) => {
                    self.tree.folders.push(path.clone());
                    self.folder(listing, &path, depth + 1)?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    self.tree.folders.push(path.clone());
                    self.cannot_list(&path, &error);
                }
            }
        }
        self.entries += odd.len();
        self.tree
            .files
            .extend(odd.into_iter().map(|(_, file)| file));
        Ok(())
    }

    fn cannot_list(&mut self, folder: &str, error: &io::Error) {
        let reason = format!("não pôde ser lida agora ({error})");
        let shown = if folder.is_empty() {
            "A pasta do projeto".to_owned()
        } else {
            format!("A pasta {folder}")
        };
        self.tree.warnings.push(format!(
            "{shown} {reason}; o que ela tem fica de fora por agora"
        ));
        self.tree.unreadable.insert(folder.to_owned(), reason);
    }
}

/// `prefix/name`, or `name` at the root.
fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Why a file cannot be read now, after its name: another program holds it, it lacks
/// permission, it is a cloud file not on this computer… Starts with [`NOT_NOW`].
pub(crate) fn now_unreadable(error: &io::Error) -> String {
    if fs_ops::busy(error) {
        format!("{NOT_NOW}: está aberto em outro programa ou sem permissão ({error})")
    } else {
        format!("{NOT_NOW} ({error})")
    }
}

/// The project's own files at its root: the project file, its old backups and an exported
/// game's marker.
fn project_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == PROJECT_FILE || lower.starts_with(&format!("{PROJECT_FILE}.")) || lower == MARKER_FILE
}

/// Whether the file `name` is one of those Windows and text editors leave behind: thumbnails,
/// folder settings, Office locks, backups and temporary files.
pub(crate) fn leftover(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(), "thumbs.db" | "desktop.ini")
        || name.starts_with("~$")
        || name.ends_with('~')
        || [".tmp", ".swp", ".bak"]
            .iter()
            .any(|extension| lower.ends_with(extension))
}

/// Whether the folder `folder` is another project or an exported game, which the tree leaves
/// out: copying, exporting or requiring their files would take another game's.
fn nested(folder: &Path) -> bool {
    fs::symlink_metadata(folder.join(PROJECT_FILE)).is_ok()
        || fs::symlink_metadata(folder.join("data").join(MARKER_FILE)).is_ok()
}

/// Whether a file is in the tree, looked at alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    Present,
    /// Not there, or where the tree does not look.
    Missing,
    /// It, or a folder on its way, cannot be read now; why.
    Unreadable(String),
}

/// Whether the file `path` (from the project's folder, with `/`) of the project at `root` is one
/// [`walk`] lists, without walking the whole tree: there in the case given, no part starting
/// with a dot, no link on its way, no folder on its way that is another project or an exported
/// game, and none of the project's own files or of the leftovers the walk skips.
pub fn presence(root: &Path, path: &str) -> Presence {
    let parts: Vec<&str> = path.split('/').collect();
    let name = parts.last().copied().unwrap_or_default();
    if parts
        .iter()
        .any(|part| part.is_empty() || part.starts_with('.') || names::check_part(part).is_err())
        || (parts.len() == 1 && project_file(name))
        || leftover(name)
    {
        return Presence::Missing;
    }
    let unreadable = |error: &io::Error| Presence::Unreadable(now_unreadable(error));
    let canonical_root = match fs::canonicalize(root) {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Presence::Missing,
        Err(error) => return unreadable(&error),
    };
    let mut current = canonical_root.clone();
    for (i, part) in parts.iter().enumerate() {
        current.push(part);
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Presence::Missing,
            Err(error) => return unreadable(&error),
        };
        let last = i + 1 == parts.len();
        if meta.file_type().is_symlink()
            || (last && !meta.is_file())
            || (!last && (!meta.is_dir() || nested(&current)))
        {
            return Presence::Missing;
        }
    }
    // The case the disk has: no part is a link, so the real path only fixes the case.
    match fs::canonicalize(&current) {
        Ok(real) if same_parts(&real, &canonical_root, &parts) => Presence::Present,
        Ok(_) => Presence::Missing,
        Err(error) => unreadable(&error),
    }
}

fn same_parts(real: &Path, root: &Path, parts: &[&str]) -> bool {
    real.strip_prefix(root).is_ok_and(|inside| {
        inside
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .eq(parts.iter().copied())
    })
}

/// Why OXY may not write the file `path` (from the project's folder, with `/`) of the project at
/// `root`, though its name is fine: a folder on its way is a link (what is written there would
/// be outside the tree) or another project or an exported game. None when it may.
pub fn outside_reason(root: &Path, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    let mut current: PathBuf = root.to_owned();
    for (i, part) in parts[..parts.len().saturating_sub(1)].iter().enumerate() {
        current.push(part);
        let Ok(meta) = fs::symlink_metadata(&current) else {
            // Folders that do not exist yet are created, real ones.
            return None;
        };
        let folder = parts[..=i].join("/");
        if meta.file_type().is_symlink() {
            return Some(format!("{folder} {LINK}"));
        }
        if nested(&current) {
            return Some(format!("{folder} {NESTED}, e fica fora deste"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_the_extension() {
        for (name, kind) in [
            ("Pulo.wav", FileKind::Sound),
            ("PULO.WAV", FileKind::Sound),
            ("Fundo.png", FileKind::Image),
            ("Fundo.PNG", FileKind::Image),
            ("Checkpoint.modelo.json", FileKind::Model),
            ("Checkpoint.Modelo.JSON", FileKind::Model),
            ("porta.luau", FileKind::Script),
            ("porta.LUAU", FileKind::Other),
            ("porta.Luau", FileKind::Other),
            ("porta.lua", FileKind::Other),
            ("porta.luau.txt", FileKind::Other),
            (".luau", FileKind::Other),
            (".modelo.json", FileKind::Other),
            ("modelo.json", FileKind::Other),
            ("notas.txt", FileKind::Other),
        ] {
            assert_eq!(FileKind::of(name), kind, "{name}");
        }
    }

    #[test]
    fn leftovers_and_the_projects_own_files_are_out() {
        for name in [
            "Thumbs.db",
            "desktop.ini",
            "DESKTOP.INI",
            "~$notas.docx",
            "porta.luau~",
            "x.tmp",
            "x.TMP",
            ".x.swp",
            "porta.luau.bak",
        ] {
            assert!(leftover(name), "{name}");
        }
        for name in ["Thumbs", "notas.txt", "porta.luau", "bakery.wav"] {
            assert!(!leftover(name), "{name}");
        }
        for name in [
            "project.oxy.json",
            "Project.OXY.json",
            "project.oxy.json.agente.bak",
            "project.oxy.json.schema-5.x.backup.json",
            "export.oxy.json",
        ] {
            assert!(project_file(name), "{name}");
        }
        assert!(!project_file("project.oxy"));
    }

    #[test]
    fn a_folder_too_large_for_a_project_is_refused_never_cut_short() {
        let root = std::env::temp_dir().join(format!("oxy-tree-{}", crate::document::new_id()));
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        for name in ["1.luau", "2.luau", "a/3.luau", "a/b/c/4.luau"] {
            crate::fs_ops::create_new_file(&root.join(name), b"return {}").unwrap();
        }
        let tree = walk_with(&root, 3, 7).unwrap();
        assert_eq!(tree.files.len(), 4);
        assert_eq!(tree.folders, ["a", "a/b", "a/b/c"]);
        let error = walk_with(&root, 2, 7).unwrap_err().to_string();
        assert_eq!(
            error,
            "A pasta do projeto é grande demais: tem pastas mais de 2 níveis umas dentro das \
             outras (a/b/c); mova o projeto para uma pasta só dele"
        );
        let error = walk_with(&root, 3, 6).unwrap_err().to_string();
        assert_eq!(
            error,
            "A pasta do projeto é grande demais: tem mais de 6 arquivos e pastas; mova o projeto \
             para uma pasta só dele"
        );
        // What starts with a dot never counts.
        fs::create_dir_all(root.join(".git/objects/x/y/z")).unwrap();
        crate::fs_ops::create_new_file(&root.join(".git/objects/x/y/z/o"), b"").unwrap();
        assert_eq!(walk_with(&root, 3, 7).unwrap(), tree);
        // A folder that is not there is no project.
        assert!(walk(&root.join("nenhuma")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
