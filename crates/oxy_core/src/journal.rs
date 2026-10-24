//! The journal of the project's files (`.oxy/pendente.json`): what OXY did to the files of the
//! project since its project file was last saved, which goes to the disk at once while the
//! project file changes only when saved. It holds the final state, not the steps: each file or
//! folder moved or renamed by its path in the saved project and its path now, the folders made
//! and removed, the files sent to the project's trash and the files made (imported) since.
//!
//! It is written before OXY touches the disk, with the operations about to be done as pending,
//! and again once they are done; opening the project reads it first, settles a pending step by
//! what the disk shows and puts the files where they are now (the next save writes them), and
//! any save that goes through takes it away. "Descartar alterações" brings the files back to
//! the last save from it ([`discard`]), whatever the undo history kept.
use crate::{
    document::{Asset, AssetKind, Id, Project, new_id},
    fs_ops::{self, BatchError, Op},
    names,
    persistence::{self, FileWrite, KnownFiles},
    project_tree::{self, FileKind},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
    time::SystemTime,
};

#[cfg(test)]
mod tests;

/// The journal, from the project's folder.
pub const JOURNAL: &str = ".oxy/pendente.json";

/// What a file sent to the project's trash since the save was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trashed {
    /// Its path in the saved project.
    pub path: String,
    /// The asset it is, for an image, a sound or a model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
}

/// The operations OXY was about to do when the journal was written, not known to be done.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// The step they are, as the undo history names it.
    pub label: String,
    pub ops: Vec<Op>,
    /// Whether each operation stands alone (the files an import makes), and counts when the
    /// disk shows it done whatever the others did; else they are done in order and undone in
    /// reverse when one fails, so those done are always the first ones.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub apart: bool,
}

/// How the project's files are now, from the project file as last saved. Paths are from the
/// project's folder, with `/`, in the case the disk has.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// The hash of the project file this journal goes from (see [`hash`]).
    #[serde(rename = "manifest_hash")]
    pub manifest: String,
    /// Each file or folder moved or renamed: its path in the saved project → its path now. What
    /// is inside a folder moved with it needs no entry of its own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub moved: BTreeMap<String, String>,
    /// The folders made, by their paths now.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub created_folders: BTreeSet<String>,
    /// The folders removed, by their paths in the saved project.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub removed_folders: BTreeSet<String>,
    /// The files sent to the project's trash, by their slot there.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trash: BTreeMap<String, Trashed>,
    /// The files made, by their paths now, with the asset each one is.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub created: BTreeMap<String, Option<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
}

/// `rest` when `path` is `prefix` ("" then) or inside it, in any case.
pub(crate) fn under<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path);
    }
    let mut chars = path.char_indices();
    for wanted in prefix.chars() {
        let (_, found) = chars.next()?;
        if names::fold(found) != names::fold(wanted) {
            return None;
        }
    }
    match chars.next() {
        None => Some(""),
        Some((at, '/')) => Some(&path[at + 1..]),
        Some(_) => None,
    }
}
/// `rest` inside `prefix` ("" for `prefix` itself).
pub(crate) fn join(prefix: &str, rest: &str) -> String {
    match (prefix.is_empty(), rest.is_empty()) {
        (true, _) => rest.to_owned(),
        (_, true) => prefix.to_owned(),
        _ => format!("{prefix}/{rest}"),
    }
}
/// How deep `path` is: 1 for a name at the root.
fn depth(path: &str) -> usize {
    path.split('/').count()
}

impl Journal {
    /// A journal with nothing done since the project file whose hash is `manifest` was saved.
    pub fn new(manifest: String) -> Self {
        Self {
            manifest,
            ..Self::default()
        }
    }

    /// Whether the files are as the project file was saved, with nothing pending.
    pub fn is_clean(&self) -> bool {
        self.changes() == 0 && self.pending.is_none()
    }

    /// How many files and folders changed since the save.
    pub fn changes(&self) -> usize {
        self.moved.len()
            + self.created_folders.len()
            + self.removed_folders.len()
            + self.trash.len()
            + self.created.len()
    }

    /// Where the file or folder the saved project has at `saved` is now; none when it went to
    /// the project's trash.
    pub fn now_of(&self, saved: &str) -> Option<String> {
        if self
            .trash
            .values()
            .any(|trashed| names::same_name(&trashed.path, saved))
        {
            return None;
        }
        let mut best: Option<(&str, &str, &str)> = None;
        for (from, to) in &self.moved {
            if let Some(rest) = under(saved, from)
                && best.is_none_or(|(longest, _, _)| from.len() > longest.len())
            {
                best = Some((from, to, rest));
            }
        }
        Some(match best {
            Some((_, to, rest)) => join(to, rest),
            None => saved.to_owned(),
        })
    }

    /// Where the file or folder at `now` was when the project was saved; none when it was made
    /// since (imported, or in a folder made since).
    pub fn saved_of(&self, now: &str) -> Option<String> {
        if let Some((from, _)) = self.moved.iter().find(|(_, to)| names::same_name(to, now)) {
            return Some(from.clone());
        }
        if self.created.keys().any(|made| names::same_name(made, now)) {
            return None;
        }
        // The deepest folder that holds it decides: one moved, or one made since.
        let mut best: Option<(usize, Option<String>)> = None;
        for (from, to) in &self.moved {
            if let Some(rest) = under(now, to)
                && !rest.is_empty()
                && best.as_ref().is_none_or(|(longest, _)| to.len() > *longest)
            {
                best = Some((to.len(), Some(join(from, rest))));
            }
        }
        for made in &self.created_folders {
            if under(now, made).is_some()
                && best
                    .as_ref()
                    .is_none_or(|(longest, _)| made.len() > *longest)
            {
                best = Some((made.len(), None));
            }
        }
        match best {
            Some((_, saved)) => saved,
            None => Some(now.to_owned()),
        }
    }

    /// Notes that `op` was done.
    pub fn apply(&mut self, op: &Op) {
        match op {
            Op::Move { from, to } => self.moved_to(from, to),
            Op::CreateDir { path } => match self.saved_of(path) {
                Some(saved) if self.take_removed(&saved) => {
                    if saved != *path {
                        self.moved.insert(saved, path.clone());
                    }
                }
                _ => {
                    self.created_folders.insert(path.clone());
                }
            },
            Op::RemoveDir { path } => {
                if let Some(made) = self
                    .created_folders
                    .iter()
                    .find(|made| names::same_name(made, path))
                    .cloned()
                {
                    self.created_folders.remove(&made);
                } else if let Some(saved) = self.saved_of(path) {
                    self.moved.retain(|_, to| !names::same_name(to, path));
                    self.removed_folders.insert(saved);
                }
            }
            Op::Trash { from, slot, id } => match self.saved_of(from) {
                None => self.created.retain(|made, _| !names::same_name(made, from)),
                Some(saved) => {
                    self.moved.retain(|_, to| !names::same_name(to, from));
                    self.trash.insert(
                        slot.clone(),
                        Trashed {
                            path: saved,
                            id: id.clone(),
                        },
                    );
                }
            },
            Op::Untrash { slot, to, id } => match self.trash.remove(slot) {
                Some(trashed) => {
                    if trashed.path != *to {
                        self.moved.insert(trashed.path, to.clone());
                    }
                }
                None => {
                    self.created.insert(to.clone(), id.clone());
                }
            },
        }
        self.tidy();
    }

    fn take_removed(&mut self, saved: &str) -> bool {
        let found = self
            .removed_folders
            .iter()
            .find(|removed| names::same_name(removed, saved))
            .cloned();
        found.is_some_and(|removed| self.removed_folders.remove(&removed))
    }

    fn moved_to(&mut self, from: &str, to: &str) {
        let origin = self.saved_of(from);
        let follow = |path: &mut String| {
            if let Some(rest) = under(path, from) {
                *path = join(to, rest);
            }
        };
        for now in self.moved.values_mut() {
            follow(now);
        }
        self.created = std::mem::take(&mut self.created)
            .into_iter()
            .map(|(mut path, id)| {
                follow(&mut path);
                (path, id)
            })
            .collect();
        self.created_folders = std::mem::take(&mut self.created_folders)
            .into_iter()
            .map(|mut path| {
                follow(&mut path);
                path
            })
            .collect();
        if let Some(origin) = origin {
            self.moved
                .retain(|saved, _| !names::same_name(saved, &origin));
            self.moved.insert(origin, to.to_owned());
        }
    }

    /// Takes out the entries that say nothing the others do not: a path moved back where the
    /// folder that holds it puts it.
    fn tidy(&mut self) {
        let redundant: Vec<String> = self
            .moved
            .iter()
            .filter(|(from, to)| {
                let mut best: Option<(&str, &str)> = None;
                for (other, other_to) in &self.moved {
                    if other == *from {
                        continue;
                    }
                    if let Some(rest) = under(from, other)
                        && !rest.is_empty()
                        && best.is_none_or(|(longest, _)| other.len() > longest.len())
                    {
                        best = Some((other, other_to));
                    }
                }
                let alone = match best {
                    Some((other, other_to)) => join(other_to, &from[other.len() + 1..]),
                    None => (*from).clone(),
                };
                alone == **to
            })
            .map(|(from, _)| from.clone())
            .collect();
        for from in redundant {
            self.moved.remove(&from);
        }
    }

    /// The journal as `pending` would leave it, with what the disk of the project at `root`
    /// shows done. Operations done in order count as the first ones, as many as leave the disk
    /// as it is: each one alone could look undone once a later one moved its file again.
    fn settle(mut self, root: &Path) -> (Self, Option<String>) {
        let Some(pending) = self.pending.take() else {
            return (self, None);
        };
        let ops = &pending.ops;
        if pending.apart {
            for op in ops {
                if done_on_disk(root, op) {
                    self.apply(op);
                }
            }
            return (self, Some(pending.label));
        }
        // The first `done` were done when undoing them from the disk now works; and, unless
        // the step was failing, doing the rest works too.
        let undoable = |done: usize| {
            let back: Vec<Op> = ops[..done].iter().rev().map(Op::inverse).collect();
            blocked(root, None, &back).is_ok()
        };
        let done = (0..=ops.len())
            .rev()
            .find(|&done| undoable(done) && blocked(root, None, &ops[done..]).is_ok())
            .or_else(|| (0..=ops.len()).rev().find(|&done| undoable(done)))
            .unwrap_or_default();
        for op in &ops[..done] {
            self.apply(op);
        }
        (self, Some(pending.label))
    }
}

/// The hash a journal keeps of the project file's bytes.
pub fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", crate::random::hash(&[b"oxy-pendente", bytes]))
}

/// Whether the file or folder `relative` of the project at `root` is there (in any case).
fn exists(root: &Path, relative: &str) -> bool {
    names::resolve(root, relative).is_ok_and(|path| fs::symlink_metadata(path).is_ok())
}

/// Whether the disk of the project at `root` shows `op` done.
fn done_on_disk(root: &Path, op: &Op) -> bool {
    match op {
        Op::Move { from, to } if names::same_name(from, to) => {
            names::disk_case(root, to).is_ok_and(|disk| disk == *to)
        }
        Op::Move { from, to } => exists(root, to) && !exists(root, from),
        Op::CreateDir { path } => exists(root, path),
        Op::RemoveDir { path } => !exists(root, path),
        Op::Trash { from, slot, .. } => exists(root, slot) && !exists(root, from),
        Op::Untrash { slot, to, .. } => exists(root, to) && !exists(root, slot),
    }
}

/// The journal of the project at `root`: none when there is none; why, when it cannot be read
/// now; and a journal that does not decode as an error of its own.
fn read(root: &Path) -> Result<Option<Result<Journal, String>>, String> {
    let file = names::resolve(root, JOURNAL).map_err(|error| error.to_string())?;
    match fs_ops::retrying(|| fs::read(&file)) {
        Ok(bytes) => Ok(Some(
            serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(project_tree::now_unreadable(&error)),
    }
}

/// Writes `journal` as the journal of the project at `root` (OXY's own file), or takes the
/// journal away when the files are as saved.
pub(crate) fn write(root: &Path, journal: &Journal) -> Result<(), String> {
    let file = names::resolve(root, JOURNAL).map_err(|error| error.to_string())?;
    if journal.is_clean() {
        return match fs_ops::retrying(|| fs::remove_file(&file)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(format!("{JOURNAL}: {error}"))
            }
            _ => Ok(()),
        };
    }
    let bytes = serde_json::to_vec_pretty(journal).map_err(|error| error.to_string())?;
    persistence::transaction_write(vec![FileWrite::replace(file, bytes)])
        .map_err(|error| error.to_string())
}

/// Takes away the journal of the project at `root` after a save that went through: the project
/// file now says where every file is.
pub(crate) fn forget(root: &Path) {
    if let Ok(file) = names::resolve(root, JOURNAL)
        && fs::symlink_metadata(&file).is_ok()
    {
        let _ = fs_ops::retrying(|| fs::remove_file(&file));
    }
}

/// Moves the journal of the project at `root` beside it as `pendente.<date>.json`, never over a
/// file; answers where, from the project's folder.
fn set_aside(root: &Path) -> Result<String, String> {
    let date = fs_ops::utc(SystemTime::now()).replace(':', "-");
    let from = names::resolve(root, JOURNAL).map_err(|error| error.to_string())?;
    for name in [
        format!(".oxy/pendente.{date}.json"),
        format!(".oxy/pendente.{date}-{}.json", new_id()),
    ] {
        let to = names::resolve(root, &name).map_err(|error| error.to_string())?;
        match fs_ops::retrying(|| fs_ops::move_no_replace(&from, &to)) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            moved => return moved.map(|()| name).map_err(|error| error.to_string()),
        }
    }
    Err("não há um nome livre para ele".into())
}

/// What opening found in the journal of a project.
#[derive(Clone, Debug, Default)]
pub(crate) struct Opened {
    pub journal: Journal,
    pub fixes: Vec<String>,
    pub warnings: Vec<String>,
}

/// Reads the journal of the project at `root`, whose project file has the hash `manifest`, as
/// opening does first: one that goes from that project file is settled (a pending step counts
/// what the disk shows done) and answered; one from another project file (saved another way
/// meanwhile) is set aside, with a warning, and the project opens as its file says.
pub(crate) fn opening(root: &Path, manifest: &str) -> Opened {
    let mut opened = Opened {
        journal: Journal::new(manifest.to_owned()),
        ..Opened::default()
    };
    let journal = match read(root) {
        Ok(None) => return opened,
        Err(why) => {
            opened.warnings.push(format!(
                "O registro do que a OXY fez nos arquivos desde o último salvamento ({JOURNAL}) \
                 {why}; o projeto abriu como o arquivo do projeto diz"
            ));
            return opened;
        }
        Ok(Some(Ok(journal))) if journal.manifest == manifest => journal,
        Ok(Some(_)) => {
            opened.warnings.push(match set_aside(root) {
                Ok(aside) => format!(
                    "O registro do que a OXY fez nos arquivos não é do projeto como ele foi salvo \
                     por último (ele foi gravado de outro jeito depois); nenhum arquivo foi mexido, \
                     e o registro ficou em {aside}"
                ),
                Err(error) => format!(
                    "O registro do que a OXY fez nos arquivos não é do projeto como ele foi salvo \
                     por último, e não pôde ser guardado à parte: {error}"
                ),
            });
            return opened;
        }
    };
    let (journal, interrupted) = journal.settle(root);
    if let Some(label) = interrupted {
        opened.fixes.push(format!(
            "A operação \"{label}\" foi interrompida; o projeto ficou com o que ela chegou a fazer \
             nos arquivos"
        ));
    }
    if journal.changes() > 0 {
        opened.fixes.push(format!(
            "Os arquivos do projeto estão como a OXY os deixou depois do último salvamento ({} \
             {}); salve para gravar o projeto assim, ou descarte as alterações para voltar os \
             arquivos",
            journal.changes(),
            if journal.changes() == 1 {
                "mudança"
            } else {
                "mudanças"
            }
        ));
    }
    opened.journal = journal;
    opened
}

/// Puts the assets of `project`, as its project file has them, where `journal` says their files
/// are now: a path moved follows, and a file made since (an import) comes back as the asset it
/// was. Answers the assets whose files went to the trash, which keep their saved paths: opening
/// tells them missing, or takes them out when nothing uses them, once the models' pieces are
/// read (they may use one).
pub(crate) fn relink_assets(project: &mut Project, journal: &Journal) -> Vec<Id> {
    let mut trashed = Vec::new();
    if journal.changes() == 0 {
        return trashed;
    }
    for asset in &mut project.assets {
        if asset.path.is_empty() {
            continue;
        }
        match journal.now_of(&asset.path) {
            Some(now) => asset.path = now,
            None => trashed.push(asset.id.clone()),
        }
    }
    for (path, id) in &journal.created {
        let Some(id) = id else {
            continue;
        };
        let kind = match FileKind::of(path) {
            FileKind::Image => AssetKind::Texture,
            FileKind::Sound => AssetKind::Audio,
            FileKind::Model => AssetKind::Model,
            FileKind::Script | FileKind::Other => continue,
        };
        if project.asset(id).is_some() {
            continue;
        }
        let Some(name) = persistence::name_of(path, kind) else {
            continue;
        };
        project.assets.push(Asset {
            id: id.clone(),
            name,
            path: path.clone(),
            kind,
            model: None,
        });
    }
    trashed
}

/// Puts the scripts the objects of `project` attach, in scenes and models, where `journal` says
/// they are now; one sent to the trash stays attached, missing.
pub(crate) fn relink_scripts(project: &mut Project, journal: &Journal) {
    if journal.moved.is_empty() {
        return;
    }
    for entity in entities_mut(project) {
        for script in &mut entity.scripts {
            if let Some(now) = journal.now_of(&script.path) {
                script.path = now;
            }
        }
    }
}

/// Every object of `project`: those of its scenes and the pieces of its models.
pub(crate) fn entities_mut(
    project: &mut Project,
) -> impl Iterator<Item = &mut crate::document::Entity> {
    project
        .scenes
        .iter_mut()
        .flat_map(|scene| scene.entities.iter_mut())
        .chain(
            project
                .assets
                .iter_mut()
                .filter_map(|asset| asset.model.as_mut())
                .flatten(),
        )
}

/// The points of [`run_with`] where tests stop it as a crash would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The journal with the operations pending is written.
    Intent,
    /// The nth operation is about to be done.
    Op(usize),
    /// The operations are done; the journal is about to be written again.
    Done,
}

/// What a test does at a [`Step`].
#[cfg(test)]
pub(crate) type Hook = Box<dyn FnMut(Step)>;
#[cfg(test)]
thread_local! {
    /// What the crate's tests do at each [`Step`] of [`run`] on their thread: a crash, or a file
    /// another program holds.
    pub(crate) static HOOK: std::cell::RefCell<Option<Hook>> =
        const { std::cell::RefCell::new(None) };
}

/// Why [`run`] did not do every operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunError {
    pub message: String,
    /// How many operations, the first ones, stayed done: undoing them failed too (see
    /// [`BatchError::RecoveryKept`]), and the journal has them. None, mostly.
    pub kept: usize,
}
impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Does `ops` in the project at `root` as the step `label`: the journal with them pending goes
/// to the disk first, then they are done in order (undone in reverse when one fails, see
/// [`fs_ops::batch`]), and `known` (stamps, files read and the journal, written again) follows
/// what was done. Nothing is touched when the journal cannot be written.
pub fn run(root: &Path, known: &mut KnownFiles, label: &str, ops: &[Op]) -> Result<(), RunError> {
    #[cfg(test)]
    if let Some(mut hook) = HOOK.take() {
        let done = run_with(root, known, label, ops, &mut *hook);
        HOOK.set(Some(hook));
        return done;
    }
    run_with(root, known, label, ops, &mut |_| {})
}

pub(crate) fn run_with(
    root: &Path,
    known: &mut KnownFiles,
    label: &str,
    ops: &[Op],
    hook: &mut dyn FnMut(Step),
) -> Result<(), RunError> {
    if ops.is_empty() {
        return Ok(());
    }
    let mut intent = known.journal().clone();
    intent.pending = Some(Pending {
        label: label.to_owned(),
        ops: ops.to_vec(),
        apart: false,
    });
    write(root, &intent).map_err(|error| RunError {
        message: format!("Não foi possível registrar a operação: {error}"),
        kept: 0,
    })?;
    hook(Step::Intent);
    let done = fs_ops::run_batch(root, ops, |index| hook(Step::Op(index)));
    let kept = match &done {
        Ok(()) => ops.len(),
        Err(BatchError::RolledBack { .. }) => 0,
        Err(BatchError::RecoveryKept { kept, .. }) => *kept,
    };
    for op in &ops[..kept] {
        known.follow(root, op);
    }
    hook(Step::Done);
    // A journal that cannot be written now keeps the operations pending, which the next opening
    // settles by the disk the same way.
    let _ = write(root, known.journal());
    done.map_err(|error| RunError {
        message: error.to_string(),
        kept,
    })
}

/// The files an import makes in the project, journaled before each one is written
/// ([`Self::before`]) and noted as made once the import ends ([`Self::finish`]): a crash in
/// between leaves the journal saying which files may be there, which the next opening checks.
pub struct Creating<'a> {
    root: &'a Path,
    known: &'a mut KnownFiles,
    label: String,
    step: String,
    ops: Vec<Op>,
}
impl<'a> Creating<'a> {
    /// The files the step `label` makes in the project at `root`.
    pub fn new(root: &'a Path, known: &'a mut KnownFiles, label: &str) -> Self {
        Self {
            root,
            known,
            label: label.to_owned(),
            step: new_id()[..8].to_owned(),
            ops: Vec::new(),
        }
    }
    /// Notes in the journal that the file `path` of the asset `id` is about to be made; the file
    /// is not to be made when this fails.
    pub fn before(&mut self, path: &str, id: &str) -> Result<(), String> {
        // Made from nothing: undoing sends it to this slot, and redoing brings it back.
        let slot = fs_ops::trash_slot(&self.step, self.ops.len() + 1, path);
        self.ops.push(Op::Untrash {
            slot,
            to: path.to_owned(),
            id: Some(id.to_owned()),
        });
        let mut intent = self.known.journal().clone();
        intent.pending = Some(Pending {
            label: self.label.clone(),
            ops: self.ops.clone(),
            apart: true,
        });
        write(self.root, &intent)
            .map_err(|error| format!("Não foi possível registrar a importação: {error}"))
    }
    /// The import ended: when `made`, its files are noted as made and answered as the undo
    /// history's operations; else they went with it, and the journal forgets them.
    pub fn finish(self, made: bool) -> Vec<crate::edit_history::FileOp> {
        use crate::edit_history::FileOp;
        if made {
            for op in &self.ops {
                self.known.follow(self.root, op);
            }
        }
        let _ = write(self.root, self.known.journal());
        if !made {
            return Vec::new();
        }
        self.ops
            .into_iter()
            .filter_map(|op| match op {
                Op::Untrash { slot, to, id } => Some(FileOp::Created { path: to, slot, id }),
                _ => None,
            })
            .collect()
    }
}

/// Why the operations of an undo or redo cannot be done now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// A file it brings back from the project's trash is no longer there: why.
    TrashEmptied(String),
    /// The disk changed outside OXY since: why.
    Changed(String),
}

/// Where what `path` names once `done` is done is before them.
enum Before {
    Disk(String),
    /// One of `done` makes it.
    Made,
    /// One of `done` takes it away.
    Gone,
}
fn before(done: &[Op], path: &str) -> Before {
    let mut path = path.to_owned();
    for op in done.iter().rev() {
        match op {
            Op::Move { from, to } => {
                if let Some(rest) = under(&path, to) {
                    path = join(from, rest);
                } else if under(&path, from).is_some() {
                    return Before::Gone;
                }
            }
            Op::CreateDir { path: made } if names::same_name(&path, made) => return Before::Made,
            Op::RemoveDir { path: removed } if under(&path, removed).is_some() => {
                return Before::Gone;
            }
            Op::Trash { from, slot, .. }
            | Op::Untrash {
                slot: from,
                to: slot,
                ..
            } => {
                if names::same_name(&path, slot) {
                    path = from.clone();
                } else if names::same_name(&path, from) {
                    return Before::Gone;
                }
            }
            _ => {}
        }
    }
    Before::Disk(path)
}

/// Checks that `ops` can be done now in the project at `root`, as undoing or redoing a step
/// does before it touches anything: each source is there, unchanged since OXY last moved or
/// wrote it (its stamp in `known`), and each target is free. A script needs no stamp: OXY
/// keeps nothing of what it says, and one edited after OXY moved it goes back as it is.
pub(crate) fn check(root: &Path, known: &KnownFiles, ops: &[Op]) -> Result<(), Blocked> {
    blocked(root, Some(known), ops)
}

/// [`check`]; without `known`, as settling a pending step reads the disk: no stamps, and a
/// change of case alone only where the disk has the name in the case it goes from.
fn blocked(root: &Path, known: Option<&KnownFiles>, ops: &[Op]) -> Result<(), Blocked> {
    let there = |done: &[Op], path: &str| match before(done, path) {
        Before::Made => true,
        Before::Gone => false,
        Before::Disk(path) => exists(root, &path),
    };
    let unchanged = |done: &[Op], path: &str| match (before(done, path), known) {
        (Before::Disk(disk), Some(known)) if FileKind::of(&disk) != FileKind::Script => {
            match known.stamp(&disk) {
                Some(stamp) => names::resolve(root, &disk)
                    .ok()
                    .and_then(|file| fs_ops::Stamp::read(&file).ok())
                    .is_some_and(|now| now == stamp),
                None => true,
            }
        }
        _ => true,
    };
    let in_case = |done: &[Op], path: &str| match (before(done, path), known) {
        (Before::Disk(disk), None) => {
            let last = |path: &str| path.rsplit('/').next().unwrap_or_default().to_owned();
            names::disk_case(root, &disk).is_ok_and(|found| last(&found) == last(path))
        }
        _ => true,
    };
    for (index, op) in ops.iter().enumerate() {
        let done = &ops[..index];
        let changed = |why: String| Err(Blocked::Changed(why));
        match op {
            Op::Move { from, to } | Op::Trash { from, slot: to, .. } => {
                if !there(done, from) {
                    return changed(format!("{from} não está mais na pasta do projeto"));
                }
                if !unchanged(done, from) {
                    return changed(format!("{from} mudou fora da OXY"));
                }
                if names::same_name(from, to) && !in_case(done, from) {
                    return changed(format!("{from} não está mais na pasta do projeto"));
                }
                if !names::same_name(from, to) && there(done, to) {
                    return changed(if matches!(op, Op::Trash { .. }) {
                        format!("a vaga de {from} na lixeira do projeto está ocupada")
                    } else {
                        format!("já existe {to}")
                    });
                }
            }
            Op::Untrash { slot, to, .. } => {
                if !there(done, slot) {
                    return Err(Blocked::TrashEmptied(format!(
                        "{to} não está mais na lixeira do projeto"
                    )));
                }
                if there(done, to) {
                    return changed(format!("já existe {to}"));
                }
            }
            Op::CreateDir { path } => {
                if there(done, path) {
                    return changed(format!("já existe {path}"));
                }
            }
            Op::RemoveDir { path } => {
                if !there(done, path) {
                    return changed(format!("a pasta {path} não está mais na pasta do projeto"));
                }
            }
        }
    }
    Ok(())
}

/// What "Descartar alterações" did to the files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discarded {
    /// How many files and folders went back to how the project was saved.
    pub restored: usize,
    /// What stayed as it is, one line each, with why (a file open in another program, a name
    /// someone took meanwhile): the journal keeps it, so the next opening finds it.
    pub kept: Vec<String>,
}

/// What [`discard`] tries, in the order it tries it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Back {
    /// A file made since goes to the trash.
    Made(String, Option<Id>),
    /// A folder removed since comes back.
    Folder(String),
    /// A file or folder moved goes back: from its path now to its saved one.
    Move(String, String),
    /// A file sent to the trash comes back.
    Untrash(String, String, Option<Id>),
    /// A folder made since goes, when it is empty.
    Remove(String),
}

/// Brings the files of the project at `root` back to how its project file was saved, from the
/// journal in `known`: what was imported goes to the project's trash, what was moved or renamed
/// goes back, what was sent to the trash comes back, the folders removed come back and those
/// made go while empty. Each step is journaled as it is done; what cannot be done now (a file
/// open in another program) stays, said in the answer, and the journal keeps it.
pub fn discard(root: &Path, known: &mut KnownFiles) -> Discarded {
    let step = format!("descartar-{}", &new_id()[..8]);
    let mut discarded = Discarded::default();
    let mut slots = 0;
    // Why each step that failed did: a name taken may free up after another step, the rest is
    // not tried again.
    let mut failed: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let mut broken_cycles = 0;
    // Steps that undo each other could take turns for ever: a few tries for each change at most.
    let mut rounds = 4 * known.journal().changes() + 16;
    loop {
        if rounds == 0 {
            break;
        }
        rounds -= 1;
        let journal = known.journal();
        let mut backs: Vec<(usize, u8, Back)> = Vec::new();
        for (path, id) in &journal.created {
            backs.push((0, 0, Back::Made(path.clone(), id.clone())));
        }
        for folder in &journal.removed_folders {
            backs.push((depth(folder), 1, Back::Folder(folder.clone())));
        }
        for (from, to) in &journal.moved {
            backs.push((depth(from), 2, Back::Move(to.clone(), from.clone())));
        }
        for (slot, trashed) in &journal.trash {
            backs.push((
                depth(&trashed.path),
                3,
                Back::Untrash(slot.clone(), trashed.path.clone(), trashed.id.clone()),
            ));
        }
        for folder in &journal.created_folders {
            backs.push((usize::MAX - depth(folder), 4, Back::Remove(folder.clone())));
        }
        backs.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let next = backs.into_iter().map(|(.., back)| back).find(|back| {
            !failed.contains_key(&format!("{back:?}")) && waits_for(journal, back).is_none()
        });
        let Some(back) = next else {
            // Names swapped in a cycle (a → b, b → a): one moves aside to free the other.
            if broken_cycles < 8
                && let Some((occupant, aside)) = cycle(root, known.journal(), &failed)
            {
                broken_cycles += 1;
                let op = Op::Move {
                    from: occupant,
                    to: aside,
                };
                if run(root, known, "Descartar alterações", &[op]).is_ok() {
                    failed.retain(|_, (_, taken)| !*taken);
                    continue;
                }
            }
            break;
        };
        let op = match &back {
            Back::Made(path, id) => {
                slots += 1;
                Op::Trash {
                    from: path.clone(),
                    slot: fs_ops::trash_slot(&step, slots, path),
                    id: id.clone(),
                }
            }
            Back::Folder(path) => Op::CreateDir { path: path.clone() },
            Back::Move(from, to) => Op::Move {
                from: from.clone(),
                to: to.clone(),
            },
            Back::Untrash(slot, to, id) => Op::Untrash {
                slot: slot.clone(),
                to: to.clone(),
                id: id.clone(),
            },
            Back::Remove(path) => Op::RemoveDir { path: path.clone() },
        };
        let target = match &op {
            Op::Move { to, .. } | Op::Untrash { to, .. } | Op::CreateDir { path: to } => {
                Some(to.clone())
            }
            _ => None,
        };
        match run(
            root,
            known,
            "Descartar alterações",
            std::slice::from_ref(&op),
        ) {
            Ok(()) => {
                discarded.restored += 1;
                // Something moved: a name taken before may be free now.
                failed.retain(|_, (_, taken)| !*taken);
            }
            Err(error) => {
                let taken = target.is_some_and(|to| exists(root, &to))
                    && !error.message.contains("aberto em outro programa");
                failed.insert(format!("{back:?}"), (error.message, taken));
            }
        }
    }
    let journal = known.journal();
    let why = |back: Back| match failed.get(&format!("{back:?}")) {
        Some((why, _)) => format!(": {why}"),
        None => waits_for(journal, &back).map_or_else(String::new, |folder| {
            format!(": a pasta {folder} não está como no último salvamento")
        }),
    };
    for (from, to) in &journal.moved {
        let why = why(Back::Move(to.clone(), from.clone()));
        discarded
            .kept
            .push(format!("{to} não voltou para {from}{why}"));
    }
    for (slot, trashed) in &journal.trash {
        let why = why(Back::Untrash(
            slot.clone(),
            trashed.path.clone(),
            trashed.id.clone(),
        ));
        discarded.kept.push(format!(
            "{} não saiu da lixeira do projeto{why}",
            trashed.path
        ));
    }
    for (path, id) in &journal.created {
        let why = why(Back::Made(path.clone(), id.clone()));
        discarded.kept.push(format!(
            "{path}, importado depois do salvamento, ficou{why}"
        ));
    }
    for folder in &journal.removed_folders {
        let why = why(Back::Folder(folder.clone()));
        discarded
            .kept
            .push(format!("a pasta {folder} não foi refeita{why}"));
    }
    for folder in &journal.created_folders {
        let why = why(Back::Remove(folder.clone()));
        discarded.kept.push(format!(
            "a pasta {folder}, criada depois do salvamento, ficou{why}"
        ));
    }
    discarded
}

/// The folder `back` puts something back into while what is at its path now is not the folder
/// the project was saved with (moved away, or made since): `back` waits for it to come back.
fn waits_for(journal: &Journal, back: &Back) -> Option<String> {
    let target = match back {
        Back::Folder(path) | Back::Move(_, path) | Back::Untrash(_, path, _) => path,
        Back::Made(..) | Back::Remove(_) => return None,
    };
    let (folder, _) = target.rsplit_once('/')?;
    let saved = journal
        .saved_of(folder)
        .is_some_and(|saved| names::same_name(&saved, folder));
    (!saved).then(|| folder.to_owned())
}

/// A file or folder whose path now is where another moved one goes back, when those that wait
/// are only waiting for each other: it and a free name beside it to move it to.
fn cycle(
    root: &Path,
    journal: &Journal,
    failed: &BTreeMap<String, (String, bool)>,
) -> Option<(String, String)> {
    let waiting = journal.moved.iter().find(|(from, to)| {
        failed
            .get(&format!("{:?}", Back::Move((*to).clone(), (*from).clone())))
            .is_some_and(|(_, taken)| *taken)
    })?;
    let occupant = journal
        .moved
        .values()
        .find(|now| names::same_name(now, waiting.0))?
        .clone();
    let (folder, name) = occupant.rsplit_once('/').unwrap_or(("", &occupant));
    let (stem, extension) = split_extension(name);
    (1..100).find_map(|n| {
        let aside = join(folder, &format!("{stem} (descartar {n}){extension}"));
        (!exists(root, &aside)).then(|| (occupant.clone(), aside))
    })
}

/// `name` as its stem and its extension (`.modelo.json` whole), the dot included.
pub(crate) fn split_extension(name: &str) -> (&str, &str) {
    let lower = name.to_ascii_lowercase();
    if lower.len() > crate::model_file::EXTENSION.len()
        && lower.ends_with(crate::model_file::EXTENSION)
    {
        let cut = name.len() - crate::model_file::EXTENSION.len();
        return name.split_at(cut);
    }
    match name.rfind('.') {
        Some(at) if at > 0 => name.split_at(at),
        _ => (name, ""),
    }
}

/// Empties the trash of the project at `root` for good (see [`fs_ops::empty_trash`]), and the
/// journal in `known` forgets the files that were in it: discarding can no longer bring them
/// back. The undo history's steps that need them go too (see
/// [`crate::edit_history::CommandHistory::forget_trash`]).
pub fn empty_trash(root: &Path, known: &mut KnownFiles) -> Result<fs_ops::Emptied, String> {
    let emptied = fs_ops::empty_trash(root);
    let journal = known.journal_mut();
    journal
        .trash
        .retain(|slot, _| names::resolve(root, slot).is_ok_and(|file| file.exists()));
    let _ = write(root, known.journal());
    emptied.map_err(|error| {
        format!(
            "Não foi possível esvaziar a lixeira do projeto: {error}; o que ficou continua nela"
        )
    })
}
