//! Command history containing field/collection patches and changed RGBA ranges.
//! A single transient gesture baseline shares image buffers; committed commands
//! never retain a project snapshot or a map of all project textures.
//!
//! A step may also have moved, made or sent to the project's trash files and folders of the
//! project's tree ([`FileOp`]): undoing and redoing it first prepares the document, then checks
//! and does the operations on the disk (journaled, see [`crate::journal`]), and only then swaps
//! the document, so a failure leaves both as they were.
use crate::{
    document::{AssetKind, Id, Project},
    fs_ops::Op,
    journal::{self, Blocked},
    painting::PaintImage,
    persistence::KnownFiles,
    texture_cache::TextureCache,
};
use serde_json::Value;
mod meshes;
use std::{
    collections::{BTreeSet, HashSet},
    path::Path,
    sync::Arc,
};

/// A change an edit made to the project's files, with paths from the project's folder, undone
/// and redone with the edit. Trash slots are fixed when the edit is planned, so undoing and
/// redoing find the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOp {
    /// A file or folder moved or renamed, never over anything.
    Move {
        from: String,
        to: String,
    },
    CreateDir {
        path: String,
    },
    /// An empty folder removed.
    RemoveDir {
        path: String,
    },
    /// A file sent to the project's trash, at `slot`.
    Trash {
        from: String,
        slot: String,
        id: Option<Id>,
    },
    /// A file made by the edit (an import): undoing sends it to `slot`, redoing brings it back.
    Created {
        path: String,
        slot: String,
        id: Option<Id>,
    },
}
impl FileOp {
    /// The change `op` makes, as a step keeps it.
    pub fn of(op: &Op) -> Self {
        match op.clone() {
            Op::Move { from, to } => Self::Move { from, to },
            Op::CreateDir { path } => Self::CreateDir { path },
            Op::RemoveDir { path } => Self::RemoveDir { path },
            Op::Trash { from, slot, id } => Self::Trash { from, slot, id },
            Op::Untrash { slot, to, id } => Self::Created { path: to, slot, id },
        }
    }
    /// The operation that redoes it.
    pub fn forward(&self) -> Op {
        match self.clone() {
            Self::Move { from, to } => Op::Move { from, to },
            Self::CreateDir { path } => Op::CreateDir { path },
            Self::RemoveDir { path } => Op::RemoveDir { path },
            Self::Trash { from, slot, id } => Op::Trash { from, slot, id },
            Self::Created { path, slot, id } => Op::Untrash { slot, to: path, id },
        }
    }
    /// The operation that undoes it.
    pub fn backward(&self) -> Op {
        self.forward().inverse()
    }
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Move { from, to } => from.capacity() + to.capacity(),
                Self::CreateDir { path } | Self::RemoveDir { path } => path.capacity(),
                Self::Trash {
                    from: path,
                    slot,
                    id,
                }
                | Self::Created { path, slot, id } => {
                    path.capacity() + slot.capacity() + id.as_ref().map_or(0, String::capacity)
                }
            }
    }
}

/// Why a step with files cannot be undone or redone without the project's folder.
const NO_FOLDER: &str = "este passo mexe nos arquivos do projeto, e o projeto não tem pasta";

#[derive(Clone, Debug)]
enum Part {
    Field(String),
    Index(usize),
    Key { field: String, value: String },
}
type JsonPath = Vec<Part>;
#[derive(Debug)]
enum DocumentDelta {
    Set {
        path: JsonPath,
        before: Option<Value>,
        after: Option<Value>,
    },
    Splice {
        path: JsonPath,
        index: usize,
        before: Vec<Value>,
        after: Vec<Value>,
    },
    Reorder {
        path: JsonPath,
        key: String,
        before: Vec<String>,
        after: Vec<String>,
    },
}
#[derive(Debug)]
struct PixelRun {
    offset: usize,
    before: Vec<u8>,
    after: Vec<u8>,
}
#[derive(Debug)]
enum ImageDelta {
    Pixels {
        id: Id,
        width: u32,
        height: u32,
        runs: Vec<PixelRun>,
    },
    Replace {
        id: Id,
        before: Option<Arc<PaintImage>>,
        after: Option<Arc<PaintImage>>,
    },
}
#[derive(Debug)]
struct Command {
    label: String,
    before_revision: u64,
    after_revision: u64,
    document: Vec<DocumentDelta>,
    images: Vec<ImageDelta>,
    meshes: Vec<meshes::Delta>,
    /// Done while the command is in the past, undone while it is in the future.
    files: Vec<FileOp>,
}
impl Command {
    /// Whether undoing it brings a file back from the project's trash.
    fn undo_needs_trash(&self) -> bool {
        self.files
            .iter()
            .any(|file| matches!(file, FileOp::Trash { .. }))
    }
    /// Whether redoing it brings a file back from the project's trash.
    fn redo_needs_trash(&self) -> bool {
        self.files
            .iter()
            .any(|file| matches!(file, FileOp::Created { .. }))
    }
}
#[derive(Debug)]
struct Baseline {
    label: String,
    project: Project,
    images: TextureCache,
}
#[derive(Debug)]
pub struct CommandHistory {
    past: Vec<Command>,
    future: Vec<Command>,
    pending: Option<Baseline>,
    // Present only during a live adjustment; committed history still contains deltas.
    amendment_restore: Option<Baseline>,
    revision: u64,
    saved_revision: u64,
    next_revision: u64,
    capacity: usize,
    budget_bytes: usize,
    last_texture_changes: Vec<Id>,
}
impl Default for CommandHistory {
    fn default() -> Self {
        Self::new()
    }
}
impl CommandHistory {
    pub fn new() -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            pending: None,
            amendment_restore: None,
            revision: 0,
            saved_revision: 0,
            next_revision: 1,
            capacity: 100,
            budget_bytes: 64 * 1024 * 1024,
            last_texture_changes: Vec::new(),
        }
    }
    pub fn begin(&mut self, label: impl Into<String>, project: &Project, images: &TextureCache) {
        if self.pending.is_none() {
            self.pending = Some(Baseline {
                label: label.into(),
                project: project.clone(),
                images: images.clone(),
            });
        }
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    /// Opaque identity for the last applied command, never serialized with a project.
    pub fn last_command_id(&self) -> Option<u64> {
        self.past.last().map(|command| command.after_revision)
    }
    /// Rebuild the original source from the existing delta, retaining a rollback only
    /// while the user is adjusting. This never stores an extra permanent snapshot.
    pub fn begin_amend_last(
        &mut self,
        command_id: u64,
        project: &Project,
        images: &TextureCache,
    ) -> Result<bool, String> {
        if self.amendment_restore.is_some() {
            return Ok(self.last_command_id() == Some(command_id));
        }
        // A step that moved files is never adjusted live.
        if self.last_command_id() != Some(command_id)
            || !self.future.is_empty()
            || self
                .past
                .last()
                .is_some_and(|command| !command.files.is_empty())
        {
            return Ok(false);
        }
        if self.pending_changed(project, images) {
            return Err("Conclua a edição atual antes de ajustar a última operação.".into());
        }
        let command = self.past.last().unwrap();
        let (origin, original_images) = prepare(command, false, project, images)?;
        self.pending = Some(Baseline {
            label: command.label.clone(),
            project: origin,
            images: original_images,
        });
        self.amendment_restore = Some(Baseline {
            label: command.label.clone(),
            project: project.clone(),
            images: images.clone(),
        });
        Ok(true)
    }
    /// Includes edits that are still part of a drag or focused text field.
    pub fn pending_changed(&self, project: &Project, images: &TextureCache) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|before| before.project != *project || before.images != *images)
    }
    /// Texture IDs affected by the last successful commit/undo/redo only.
    pub fn last_texture_changes(&self) -> &[Id] {
        &self.last_texture_changes
    }
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    /// Whether the next undo (or redo, when `redo`) moves files of the project's tree.
    pub fn next_has_files(&self, redo: bool) -> bool {
        let stack = if redo { &self.future } else { &self.past };
        stack
            .last()
            .is_some_and(|command| !command.files.is_empty())
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|c| c.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|c| c.label.as_str())
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }
    pub fn undo_len(&self) -> usize {
        self.past.len()
    }
    pub fn redo_len(&self) -> usize {
        self.future.len()
    }
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        self.trim();
    }
    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.trim();
    }
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.pending = None;
        self.amendment_restore = None;
        self.revision = 0;
        self.saved_revision = 0;
        self.next_revision = 1;
        self.last_texture_changes.clear();
    }
    pub fn cancel(&mut self, project: &mut Project, images: &mut TextureCache) -> bool {
        if let Some(applied) = self.amendment_restore.take() {
            *project = applied.project;
            *images = applied.images;
            self.pending = None;
            return true;
        }
        if let Some(before) = self.pending.take() {
            *project = before.project;
            *images = before.images;
            true
        } else {
            false
        }
    }
    pub fn commit(&mut self, project: &Project, images: &mut TextureCache) -> Result<bool, String> {
        self.commit_with_files(project, images, Vec::new())
    }
    /// [`Self::commit`] of a step that also did `files` on the disk (see [`FileOp`]): kept even
    /// when the document did not change (a folder made, a script nobody attaches moved), and it
    /// leaves the project unsaved as any step does. The step began with [`Self::begin`].
    pub fn commit_with_files(
        &mut self,
        project: &Project,
        images: &mut TextureCache,
        files: Vec<FileOp>,
    ) -> Result<bool, String> {
        self.last_texture_changes.clear();
        let Some(mut before) = self.pending.take() else {
            if files.is_empty() {
                return Ok(false);
            }
            return Err("Operação de arquivos sem passo de desfazer aberto".into());
        };
        let result = (|| {
            let (a, before_meshes) = meshes::detach(&before.project);
            let (b, after_meshes) = meshes::detach(project);
            let a = serde_json::to_value(a.as_ref()).map_err(|e| e.to_string())?;
            let b = serde_json::to_value(b.as_ref()).map_err(|e| e.to_string())?;
            let mut document = Vec::new();
            diff_json(&a, &b, &mut Vec::new(), &mut document);
            let deltas = diff_images(&before.project, project, &mut before.images, images)?;
            Ok::<_, String>((
                document,
                deltas,
                meshes::diff(&before_meshes, &after_meshes),
            ))
        })();
        let (document, deltas, mesh_deltas) = match result {
            Ok(value) => value,
            Err(error) => {
                self.pending = Some(before);
                return Err(error);
            }
        };
        // A mutable borrow alone is not an edit. Restore clean flags for untouched
        // images; this also prevents eyedropper clicks from pinning clean textures.
        let touched: HashSet<_> = deltas.iter().map(|delta| delta.id().to_owned()).collect();
        self.last_texture_changes = touched.iter().cloned().collect();
        let candidates: Vec<_> = images.keys().cloned().collect();
        for id in candidates {
            if !touched.contains(&id) {
                images.set_dirty(&id, before.images.is_dirty(&id));
            }
        }
        images.prune_removed(project);
        images.finish_gesture();
        let before_revision = if self.amendment_restore.take().is_some() {
            let original = self
                .past
                .pop()
                .expect("amendment requires the last command");
            original.before_revision
        } else {
            self.revision
        };
        if document.is_empty() && deltas.is_empty() && mesh_deltas.is_empty() && files.is_empty() {
            self.revision = before_revision;
            return Ok(false);
        }
        let next = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1);
        self.past.push(Command {
            label: before.label,
            before_revision,
            after_revision: next,
            document,
            images: deltas,
            meshes: mesh_deltas,
            files,
        });
        self.revision = next;
        self.future.clear();
        self.trim();
        Ok(true)
    }
    pub fn undo(
        &mut self,
        project: &mut Project,
        images: &mut TextureCache,
    ) -> Result<Option<String>, String> {
        self.undo_with(project, images, None)
    }
    pub fn redo(
        &mut self,
        project: &mut Project,
        images: &mut TextureCache,
    ) -> Result<Option<String>, String> {
        self.redo_with(project, images, None)
    }
    /// [`Self::undo`] of the project saved in the folder of `files` (its root, and what OXY knows
    /// of its files), which a step that moved files needs: the document is prepared first, then
    /// the files go back (checked first: a file changed outside OXY or a name taken refuses it),
    /// and only then the document changes. When the project's trash lost a file the step brings
    /// back, that step and the older ones go (see [`Self::forget_trash`]), and the error says
    /// how many.
    pub fn undo_with(
        &mut self,
        project: &mut Project,
        images: &mut TextureCache,
        files: Option<(&Path, &mut KnownFiles)>,
    ) -> Result<Option<String>, String> {
        self.step(project, images, files, false)
    }
    /// [`Self::undo_with`] for redo.
    pub fn redo_with(
        &mut self,
        project: &mut Project,
        images: &mut TextureCache,
        files: Option<(&Path, &mut KnownFiles)>,
    ) -> Result<Option<String>, String> {
        self.step(project, images, files, true)
    }
    fn step(
        &mut self,
        project: &mut Project,
        images: &mut TextureCache,
        files: Option<(&Path, &mut KnownFiles)>,
        forward: bool,
    ) -> Result<Option<String>, String> {
        self.commit(project, images)?;
        let stack = if forward { &self.future } else { &self.past };
        let Some(command) = stack.last() else {
            return Ok(None);
        };
        let (next_project, next_images) = prepare(command, forward, project, images)?;
        let label = command.label.clone();
        let ops: Vec<Op> = if forward {
            command.files.iter().map(FileOp::forward).collect()
        } else {
            command.files.iter().rev().map(FileOp::backward).collect()
        };
        let what = if forward { "refazer" } else { "desfazer" };
        if !ops.is_empty() {
            let Some((root, known)) = files else {
                return Err(format!("Não foi possível {what} \"{label}\": {NO_FOLDER}."));
            };
            match journal::check(root, known, &ops) {
                Ok(()) => {}
                Err(Blocked::Changed(why)) => {
                    return Err(format!("Não foi possível {what} \"{label}\": {why}."));
                }
                Err(Blocked::TrashEmptied(why)) => {
                    let gone = self.forget_trash();
                    return Err(format!(
                        "Não foi possível {what} \"{label}\": {why}, que foi esvaziada; {gone} {} \
                         de desfazer e refazer deixaram de valer.",
                        if gone == 1 { "passo" } else { "passos" }
                    ));
                }
            }
            let step = if forward { "Refazer" } else { "Desfazer" };
            if let Err(error) = journal::run(root, known, &format!("{step} {label}"), &ops) {
                let message = format!("Não foi possível {what} \"{label}\": {error}");
                if error.kept == 0 {
                    return Err(message);
                }
                // Undoing the operations failed too: the files that stayed moved are a step of
                // their own, which the document follows, and undoing it puts them back.
                let kept: Vec<FileOp> = ops[..error.kept].iter().map(FileOp::of).collect();
                let partial = format!("{step} incompleto: {label}");
                self.begin(partial.clone(), project, images);
                crate::command::follow_files(project, &kept);
                self.commit_with_files(project, images, kept)?;
                return Err(format!(
                    "{message}. {}; desfazer o passo \"{partial}\" os devolve.",
                    crate::command::KEPT
                ));
            }
        }
        *project = next_project;
        *images = next_images;
        let command = if forward {
            self.future.pop()
        } else {
            self.past.pop()
        }
        .expect("the command just prepared");
        self.last_texture_changes = command
            .images
            .iter()
            .map(|delta| delta.id().to_owned())
            .collect();
        if forward {
            self.revision = command.after_revision;
            self.past.push(command);
        } else {
            self.revision = command.before_revision;
            self.future.push(command);
        }
        if !self.is_dirty() {
            images.mark_saved();
        }
        Ok(Some(label))
    }
    /// How many steps of undo and redo need a file of the project's trash: those emptying it
    /// takes away (see [`Self::forget_trash`]).
    pub fn trash_steps(&self) -> usize {
        let past = self
            .past
            .iter()
            .rposition(Command::undo_needs_trash)
            .map_or(0, |newest| newest + 1);
        let future = self
            .future
            .iter()
            .rposition(Command::redo_needs_trash)
            .map_or(0, |nearest| nearest + 1);
        past + future
    }
    /// After the project's trash was emptied: undo stops at the newest step that brings a file
    /// back from it, which goes with every older one (out of reach now), and redo at the nearest
    /// such step, which goes with every one after it. Answers how many went.
    pub fn forget_trash(&mut self) -> usize {
        let gone = self.trash_steps();
        if let Some(newest) = self.past.iter().rposition(Command::undo_needs_trash) {
            self.past.drain(..=newest);
        }
        if let Some(nearest) = self.future.iter().rposition(Command::redo_needs_trash) {
            self.future.drain(..=nearest);
        }
        gone
    }
    /// Conservative estimate of heap payload retained by commands, including vector capacities and
    /// unique full-image allocations for texture creation/deletion/replacement.
    /// The transient baseline is deliberately reported separately.
    pub fn estimated_bytes(&self) -> usize {
        let mut images = HashSet::new();
        let mut meshes = HashSet::new();
        self.past
            .iter()
            .chain(&self.future)
            .map(|command| {
                std::mem::size_of::<Command>()
                    + command
                        .meshes
                        .iter()
                        .map(|m| m.bytes(&mut meshes))
                        .sum::<usize>()
                    + command.label.capacity()
                    + command.files.iter().map(FileOp::bytes).sum::<usize>()
                    + command
                        .document
                        .iter()
                        .map(DocumentDelta::bytes)
                        .sum::<usize>()
                    + command
                        .images
                        .iter()
                        .map(|delta| delta.bytes(&mut images))
                        .sum::<usize>()
            })
            .sum()
    }
    pub fn paint_delta_bytes(&self) -> usize {
        let mut images = HashSet::new();
        self.past
            .iter()
            .chain(&self.future)
            .flat_map(|c| &c.images)
            .map(|delta| delta.bytes(&mut images))
            .sum()
    }
    pub fn pending_document_bytes(&self) -> usize {
        self.pending
            .as_ref()
            .and_then(|b| serde_json::to_vec(&b.project).ok())
            .map_or(0, |v| v.len())
    }
    fn trim(&mut self) {
        while self.past.len() > 1
            && (self.past.len() > self.capacity || self.estimated_bytes() > self.budget_bytes)
        {
            self.past.remove(0);
        }
    }
}
fn path_bytes(path: &JsonPath) -> usize {
    path.capacity() * std::mem::size_of::<Part>()
        + path
            .iter()
            .map(|part| match part {
                Part::Field(v) => v.capacity(),
                Part::Key { field, value } => field.capacity() + value.capacity(),
                Part::Index(_) => 0,
            })
            .sum::<usize>()
}
fn json_bytes(value: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match value {
            Value::String(s) => s.capacity(),
            Value::Array(a) => {
                a.capacity() * std::mem::size_of::<Value>()
                    + a.iter().map(json_bytes).sum::<usize>()
            }
            Value::Object(o) => o
                .iter()
                .map(|(k, v)| k.capacity() + json_bytes(v) + 3 * std::mem::size_of::<usize>())
                .sum(),
            _ => 0,
        }
}
impl DocumentDelta {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Set {
                    path,
                    before,
                    after,
                } => path_bytes(path) + before.iter().chain(after).map(json_bytes).sum::<usize>(),
                Self::Splice {
                    path,
                    before,
                    after,
                    ..
                } => path_bytes(path) + before.iter().chain(after).map(json_bytes).sum::<usize>(),
                Self::Reorder {
                    path,
                    key,
                    before,
                    after,
                } => {
                    path_bytes(path)
                        + key.capacity()
                        + before
                            .iter()
                            .chain(after)
                            .map(|s| s.capacity() + std::mem::size_of::<String>())
                            .sum::<usize>()
                }
            }
    }
}
impl ImageDelta {
    fn id(&self) -> &str {
        match self {
            Self::Pixels { id, .. } | Self::Replace { id, .. } => id,
        }
    }
    fn bytes(&self, seen: &mut HashSet<usize>) -> usize {
        std::mem::size_of::<Self>()
            + self.id().len()
            + match self {
                Self::Pixels { runs, .. } => {
                    runs.capacity() * std::mem::size_of::<PixelRun>()
                        + runs
                            .iter()
                            .map(|run| run.before.capacity() + run.after.capacity())
                            .sum::<usize>()
                }
                Self::Replace { before, after, .. } => before
                    .iter()
                    .chain(after)
                    .filter(|image| seen.insert(Arc::as_ptr(image) as usize))
                    .map(|image| image.pixels.capacity() + std::mem::size_of::<PaintImage>())
                    .sum(),
            }
    }
}
fn array_key(a: &[Value], b: &[Value]) -> Option<&'static str> {
    for key in ["id", "target"] {
        if !a.is_empty() || !b.is_empty() {
            let valid = |values: &[Value]| {
                let mut seen = HashSet::new();
                values.iter().all(|v| {
                    v.get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|id| seen.insert(id))
                })
            };
            if valid(a) && valid(b) {
                return Some(key);
            }
        }
    }
    None
}
fn diff_json(before: &Value, after: &Value, path: &mut JsonPath, output: &mut Vec<DocumentDelta>) {
    if before == after {
        return;
    }
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for key in keys {
                path.push(Part::Field(key.clone()));
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) => diff_json(a, b, path, output),
                    (a, b) => output.push(DocumentDelta::Set {
                        path: path.clone(),
                        before: a.cloned(),
                        after: b.cloned(),
                    }),
                }
                path.pop();
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if let Some(key) = array_key(a, b) {
                let a_ids: Vec<_> = a
                    .iter()
                    .map(|v| v[key].as_str().unwrap().to_owned())
                    .collect();
                let b_ids: Vec<_> = b
                    .iter()
                    .map(|v| v[key].as_str().unwrap().to_owned())
                    .collect();
                let mut current = a_ids.clone();
                for index in (0..a.len()).rev() {
                    if !b_ids.contains(&a_ids[index]) {
                        output.push(DocumentDelta::Splice {
                            path: path.clone(),
                            index,
                            before: vec![a[index].clone()],
                            after: Vec::new(),
                        });
                        current.remove(index);
                    }
                }
                for (index, id) in b_ids.iter().enumerate() {
                    if !a_ids.contains(id) {
                        let at = index.min(current.len());
                        output.push(DocumentDelta::Splice {
                            path: path.clone(),
                            index: at,
                            before: Vec::new(),
                            after: vec![b[index].clone()],
                        });
                        current.insert(at, id.clone());
                    }
                }
                if current != b_ids {
                    output.push(DocumentDelta::Reorder {
                        path: path.clone(),
                        key: key.into(),
                        before: current,
                        after: b_ids.clone(),
                    });
                }
                for (index, id) in a_ids.iter().enumerate() {
                    if let Some(next) = b_ids.iter().position(|other| other == id) {
                        path.push(Part::Key {
                            field: key.into(),
                            value: id.clone(),
                        });
                        diff_json(&a[index], &b[next], path, output);
                        path.pop();
                    }
                }
            } else if a.len() == b.len() {
                for (index, (a, b)) in a.iter().zip(b).enumerate() {
                    path.push(Part::Index(index));
                    diff_json(a, b, path, output);
                    path.pop();
                }
            } else {
                let prefix = a.iter().zip(b).take_while(|(a, b)| a == b).count();
                let suffix = a[prefix..]
                    .iter()
                    .rev()
                    .zip(b[prefix..].iter().rev())
                    .take_while(|(a, b)| a == b)
                    .count();
                output.push(DocumentDelta::Splice {
                    path: path.clone(),
                    index: prefix,
                    before: a[prefix..a.len() - suffix].to_vec(),
                    after: b[prefix..b.len() - suffix].to_vec(),
                });
            }
        }
        _ => output.push(DocumentDelta::Set {
            path: path.clone(),
            before: Some(before.clone()),
            after: Some(after.clone()),
        }),
    }
}
fn diff_images(
    before_project: &Project,
    after_project: &Project,
    before: &mut TextureCache,
    after: &TextureCache,
) -> Result<Vec<ImageDelta>, String> {
    let ids: HashSet<_> = before
        .dirty_images()
        .map(|(id, _)| id.clone())
        .chain(after.dirty_images().map(|(id, _)| id.clone()))
        .collect();
    let mut result = Vec::new();
    for id in ids {
        let existed = before_project
            .asset(&id)
            .is_some_and(|asset| asset.kind == AssetKind::Texture);
        let exists = after_project
            .asset(&id)
            .is_some_and(|asset| asset.kind == AssetKind::Texture);
        let original = if existed {
            if let Some(image) = before.shared(&id).or_else(|| after.seed(&id)) {
                Some(image)
            } else {
                before.ensure(&id)?;
                before.shared(&id)
            }
        } else {
            None
        };
        let current = if exists { after.shared(&id) } else { None };
        match (&original, &current) {
            (Some(a), Some(b)) if Arc::ptr_eq(a, b) || a == b => {}
            (Some(a), Some(b)) if a.width == b.width && a.height == b.height => {
                let mut runs = Vec::new();
                let mut pixel = 0;
                let count = a.pixels.len() / 4;
                while pixel < count {
                    if a.pixels[pixel * 4..pixel * 4 + 4] == b.pixels[pixel * 4..pixel * 4 + 4] {
                        pixel += 1;
                        continue;
                    }
                    let start = pixel;
                    let mut end = pixel + 1;
                    pixel += 1;
                    while pixel < count {
                        let changed = a.pixels[pixel * 4..pixel * 4 + 4]
                            != b.pixels[pixel * 4..pixel * 4 + 4];
                        if changed {
                            end = pixel + 1;
                        } else if pixel - end > 8 {
                            break;
                        }
                        pixel += 1;
                    }
                    runs.push(PixelRun {
                        offset: start * 4,
                        before: a.pixels[start * 4..end * 4].to_vec(),
                        after: b.pixels[start * 4..end * 4].to_vec(),
                    });
                }
                if !runs.is_empty() {
                    result.push(ImageDelta::Pixels {
                        id,
                        width: a.width,
                        height: a.height,
                        runs,
                    });
                }
            }
            (None, None) => {}
            _ => result.push(ImageDelta::Replace {
                id,
                before: original,
                after: current,
            }),
        }
    }
    Ok(result)
}
fn at_mut<'a>(value: &'a mut Value, path: &[Part]) -> Result<&'a mut Value, String> {
    let mut current = value;
    for part in path {
        current = match part {
            Part::Field(key) => current
                .get_mut(key)
                .ok_or_else(|| format!("Campo de histórico ausente: {key}"))?,
            Part::Index(index) => current
                .get_mut(*index)
                .ok_or("Índice de histórico ausente")?,
            Part::Key { field, value } => current
                .as_array_mut()
                .ok_or("Lista de histórico ausente")?
                .iter_mut()
                .find(|v| v.get(field).and_then(Value::as_str) == Some(value))
                .ok_or_else(|| format!("Identificador de histórico ausente: {value}"))?,
        }
    }
    Ok(current)
}
fn apply_document(delta: &DocumentDelta, forward: bool, value: &mut Value) -> Result<(), String> {
    match delta {
        DocumentDelta::Set {
            path,
            before,
            after,
        } => {
            let next = if forward { after } else { before };
            let Some((last, parent)) = path.split_last() else {
                *value = next.clone().ok_or("Não é possível remover documento")?;
                return Ok(());
            };
            let container = at_mut(value, parent)?;
            if let Part::Field(key) = last {
                let object = container.as_object_mut().ok_or("Campo fora de objeto")?;
                if let Some(next) = next {
                    object.insert(key.clone(), next.clone());
                } else {
                    object.remove(key);
                }
            } else {
                *at_mut(container, std::slice::from_ref(last))? = next
                    .clone()
                    .ok_or("Não é possível remover posição de lista")?;
            }
        }
        DocumentDelta::Splice {
            path,
            index,
            before,
            after,
        } => {
            let (remove, insert) = if forward {
                (before, after)
            } else {
                (after, before)
            };
            let array = at_mut(value, path)?
                .as_array_mut()
                .ok_or("Lista de histórico ausente")?;
            if *index > array.len() || *index + remove.len() > array.len() {
                return Err("Limite de coleção inválido no histórico".into());
            }
            array.splice(*index..*index + remove.len(), insert.iter().cloned());
        }
        DocumentDelta::Reorder {
            path,
            key,
            before,
            after,
        } => {
            let ids = if forward { after } else { before };
            let array = at_mut(value, path)?
                .as_array_mut()
                .ok_or("Lista de histórico ausente")?;
            let mut sorted = Vec::with_capacity(array.len());
            for id in ids {
                let item = array
                    .iter()
                    .find(|v| v.get(key).and_then(Value::as_str) == Some(id))
                    .ok_or("Objeto ausente na reordenação")?;
                sorted.push(item.clone());
            }
            *array = sorted;
        }
    }
    Ok(())
}
/// The document and images `command` leaves (redone when `forward`, else undone), made from
/// `project` and `images`, which stay as they are: nothing changes when this fails.
fn prepare(
    command: &Command,
    forward: bool,
    project: &Project,
    images: &TextureCache,
) -> Result<(Project, TextureCache), String> {
    let (skeleton, meshes) = meshes::detach(project);
    let mut json = serde_json::to_value(skeleton.as_ref()).map_err(|e| e.to_string())?;
    if forward {
        for delta in &command.document {
            apply_document(delta, true, &mut json)?;
        }
    } else {
        for delta in command.document.iter().rev() {
            apply_document(delta, false, &mut json)?;
        }
    }
    let mut next_project: Project = serde_json::from_value(json)
        .map_err(|e| format!("Não foi possível restaurar edição: {e}"))?;
    meshes::apply(&command.meshes, forward, meshes, &mut next_project)?;
    let mut next_images = images.clone();
    let root = images.root().to_owned();
    next_images.configure(&root, &next_project);
    for delta in &command.images {
        match delta {
            ImageDelta::Replace { id, before, after } => {
                if let Some(image) = if forward { after } else { before } {
                    next_images.set_shared(id.clone(), image.clone(), true);
                } else {
                    next_images.remove(id);
                }
            }
            ImageDelta::Pixels {
                id,
                width,
                height,
                runs,
            } => {
                next_images.ensure(id)?;
                let image = next_images
                    .get_mut(id)
                    .ok_or("Textura a restaurar não está residente")?;
                if image.width != *width || image.height != *height {
                    return Err("Dimensões da textura mudaram fora do histórico".into());
                }
                for run in runs {
                    let pixels = if forward { &run.after } else { &run.before };
                    let end = run.offset + pixels.len();
                    if end > image.pixels.len() {
                        return Err("Faixa de pixels fora da textura".into());
                    }
                    image.pixels[run.offset..end].copy_from_slice(pixels);
                }
            }
        }
    }
    next_images.prune_removed(&next_project);
    next_images.finish_gesture();
    Ok((next_project, next_images))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Asset, Entity, Primitive, new_id};
    fn setup() -> (Project, TextureCache, Id) {
        let mut project = Project::new("deltas");
        let entity = Entity::new("Peça", Some(Primitive::Cube));
        let id = entity.id.clone();
        project.scenes[0].entities.push(entity);
        (project, TextureCache::default(), id)
    }
    #[test]
    fn repeated_transform_edits_never_store_project_or_texture_snapshots() {
        let (mut project, mut cache, id) = setup();
        let texture = new_id();
        project.assets.push(Asset {
            id: texture.clone(),
            name: "Grande".into(),
            path: "large.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        cache.insert(
            texture.clone(),
            PaintImage::new(2048, 2048, [255; 4]).unwrap(),
        );
        let identity = cache.buffer_identity(&texture);
        let mut history = CommandHistory::new();
        for n in 1..=40 {
            history.begin("Mover", &project, &cache);
            project.scenes[0]
                .entity_mut(&id)
                .unwrap()
                .transform
                .position[0] = n as f32;
            history.commit(&project, &mut cache).unwrap();
        }
        assert_eq!(history.undo_len(), 40);
        assert_eq!(history.paint_delta_bytes(), 0);
        assert!(
            history.estimated_bytes() < 100_000,
            "{}",
            history.estimated_bytes()
        );
        assert_eq!(cache.buffer_identity(&texture), identity);
        eprintln!(
            "40 transforms: history={} B, image_deltas={} B, resident={} B",
            history.estimated_bytes(),
            history.paint_delta_bytes(),
            cache.resident_bytes()
        );
        for _ in 0..40 {
            history.undo(&mut project, &mut cache).unwrap();
        }
        assert_eq!(
            project.scenes[0].entity(&id).unwrap().transform.position[0],
            0.
        );
    }
    #[test]
    fn whole_paint_gesture_is_compact_and_reversible() {
        let (mut project, mut cache, _) = setup();
        let texture = new_id();
        project.assets.push(Asset {
            id: texture.clone(),
            name: "Grande".into(),
            path: "large.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        cache.insert(
            texture.clone(),
            PaintImage::new(2048, 2048, [255; 4]).unwrap(),
        );
        let mut history = CommandHistory::new();
        history.begin("Pincelada", &project, &cache);
        cache
            .get_mut(&texture)
            .unwrap()
            .stroke([100., 100.], [180., 180.], 3., [250, 0, 0, 255]);
        cache
            .get_mut(&texture)
            .unwrap()
            .stroke([180., 180.], [200., 160.], 3., [250, 0, 0, 255]);
        history.commit(&project, &mut cache).unwrap();
        assert_eq!(history.undo_len(), 1);
        assert!(
            history.paint_delta_bytes() < 30_000,
            "{}",
            history.paint_delta_bytes()
        );
        assert!(cache.resident_bytes() < 17_000_000);
        eprintln!(
            "2048² paint gesture: history={} B, image_deltas={} B, resident={} B",
            history.estimated_bytes(),
            history.paint_delta_bytes(),
            cache.resident_bytes()
        );
        history.undo(&mut project, &mut cache).unwrap();
        assert_eq!(cache.get(&texture).unwrap().pixel(150, 150), [255; 4]);
        history.redo(&mut project, &mut cache).unwrap();
        assert_eq!(
            cache.get(&texture).unwrap().pixel(150, 150),
            [250, 0, 0, 255]
        );
    }
    #[test]
    fn creation_deletion_graph_and_array_order_roundtrip() {
        let (mut project, mut cache, id) = setup();
        let initial = project.clone();
        let mut history = CommandHistory::new();
        history.begin("Criar filho", &project, &cache);
        let mut child = Entity::new("Filho", Some(Primitive::Sphere));
        child.parent = Some(id.clone());
        project.scenes[0].entities.insert(0, child);
        project.scenes[0].entity_mut(&id).unwrap().name = "Pai".into();
        history.commit(&project, &mut cache).unwrap();
        let after = project.clone();
        history.undo(&mut project, &mut cache).unwrap();
        assert_eq!(project, initial);
        history.redo(&mut project, &mut cache).unwrap();
        assert_eq!(project, after);
        history.begin("Reordenar", &project, &cache);
        project.scenes[0].entities.reverse();
        history.commit(&project, &mut cache).unwrap();
        history.undo(&mut project, &mut cache).unwrap();
        assert_eq!(project, after);
    }
    #[test]
    fn immutable_cache_loads_and_unchanged_mutations_are_not_commands() {
        let (mut project, mut cache, _) = setup();
        let texture = new_id();
        project.assets.push(Asset {
            id: texture.clone(),
            name: "t".into(),
            path: "x.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        cache.insert(texture.clone(), PaintImage::new(16, 16, [255; 4]).unwrap());
        cache.mark_saved();
        let mut history = CommandHistory::new();
        history.begin("Conta-gotas", &project, &cache);
        cache.get_mut(&texture).unwrap().pixel(0, 0);
        assert!(!history.pending_changed(&project, &cache));
        assert!(!history.commit(&project, &mut cache).unwrap());
        assert!(!cache.has_dirty());
        assert!(!history.is_dirty());
    }
    #[test]
    fn graph_parameters_tracks_attributes_and_subtree_deletion_store_only_changes() {
        use crate::{
            animation::{Clip, Interpolation, Keyframe, Track},
            document::{Transform, Value as Attribute},
            graph::Node,
        };
        let (mut project, mut cache, id) = setup();
        let mut child = Entity::new("Filho", Some(Primitive::Sphere));
        child.parent = Some(id.clone());
        let child_id = child.id.clone();
        project.scenes[0].entities.push(child);
        let mut clip = Clip::new("Longo");
        clip.duration = 200.;
        clip.tracks.push(Track {
            target: child_id.clone(),
            keyframes: (0..200)
                .map(|n| Keyframe {
                    time: n as f32,
                    transform: Transform::default(),
                    interpolation: Interpolation::Linear,
                })
                .collect(),
        });
        let entity = project.scenes[0].entity_mut(&id).unwrap();
        entity.clips.push(clip);
        for n in 0..100 {
            entity
                .graph
                .nodes
                .push(Node::new("control.message", [n as f32, 0.]));
        }
        let original = project.clone();
        let mut history = CommandHistory::new();
        for n in 1..=20 {
            history.begin("Campos", &project, &cache);
            let entity = project.scenes[0].entity_mut(&id).unwrap();
            entity.graph.nodes[50]
                .params
                .insert("message".into(), Attribute::Text(format!("{n}")));
            entity.clips[0].tracks[0].keyframes[100].transform.rotation[1] = n as f32;
            entity
                .attributes
                .insert("Energia".into(), Attribute::Number(n as f64));
            history.commit(&project, &mut cache).unwrap();
        }
        assert!(
            history.estimated_bytes() < 160_000,
            "{}",
            history.estimated_bytes()
        );
        assert!(history.last_texture_changes().is_empty());
        let edited = project.clone();
        history.begin("Excluir estrutura", &project, &cache);
        project.scenes[0].entities.clear();
        history.commit(&project, &mut cache).unwrap();
        history.undo(&mut project, &mut cache).unwrap();
        assert_eq!(project, edited);
        for _ in 0..20 {
            history.undo(&mut project, &mut cache).unwrap();
        }
        assert_eq!(project, original);
        for _ in 0..20 {
            history.redo(&mut project, &mut cache).unwrap();
        }
        assert_eq!(project, edited);
    }
    #[test]
    fn branch_save_marker_and_pending_cancel_are_consistent() {
        let (mut project, mut cache, id) = setup();
        let mut history = CommandHistory::new();
        history.begin("Nome", &project, &cache);
        assert!(!history.pending_changed(&project, &cache));
        project.scenes[0].entity_mut(&id).unwrap().name = "Salvo".into();
        assert!(history.pending_changed(&project, &cache));
        history.commit(&project, &mut cache).unwrap();
        assert!(!history.pending_changed(&project, &cache));
        history.mark_saved();
        history.begin("Mover", &project, &cache);
        project.scenes[0]
            .entity_mut(&id)
            .unwrap()
            .transform
            .position[0] = 3.;
        assert!(history.is_pending());
        history.undo(&mut project, &mut cache).unwrap();
        assert!(!history.is_dirty());
        assert_eq!(
            project.scenes[0].entity(&id).unwrap().transform.position[0],
            0.
        );
        assert!(history.can_redo());
        history.begin("Cancelar", &project, &cache);
        project.scenes[0].entity_mut(&id).unwrap().name = "Rascunho".into();
        assert!(history.cancel(&mut project, &mut cache));
        assert_eq!(project.scenes[0].entity(&id).unwrap().name, "Salvo");
        history.begin("Ramificar", &project, &cache);
        project.scenes[0].entity_mut(&id).unwrap().name = "Ramo".into();
        history.commit(&project, &mut cache).unwrap();
        assert!(!history.can_redo());
        assert!(history.is_dirty());
        history.undo(&mut project, &mut cache).unwrap();
        assert!(!history.is_dirty());
    }
    #[test]
    fn lazy_loaded_paint_gesture_and_dirty_deletion_remain_undoable() {
        let (mut project, mut cache, _) = setup();
        let dir = std::env::temp_dir().join(format!("oxy-history-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let id = new_id();
        project.assets.push(Asset {
            id: id.clone(),
            name: "Lazy".into(),
            path: "lazy.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        PaintImage::new(1024, 1024, [255; 4])
            .unwrap()
            .save(&dir.join("lazy.png"))
            .unwrap();
        cache.configure(&dir, &project);
        let mut history = CommandHistory::new();
        for n in 0..30 {
            history.begin("Pintar", &project, &cache);
            cache.ensure(&id).unwrap();
            cache
                .get_mut(&id)
                .unwrap()
                .brush([50. + n as f32 * 4., 50.], 2., [255, 0, 0, 255]);
            history.commit(&project, &mut cache).unwrap();
            assert_eq!(history.last_texture_changes(), std::slice::from_ref(&id));
        }
        assert_eq!(cache.decode_count(), 1);
        assert!(cache.resident_bytes() < 4_300_000);
        assert!(history.paint_delta_bytes() < 100_000);
        eprintln!(
            "30 paint gestures: history={} B, image_deltas={} B, resident={} B",
            history.estimated_bytes(),
            history.paint_delta_bytes(),
            cache.resident_bytes()
        );
        history.begin("Excluir textura alterada", &project, &cache);
        project.assets.clear();
        cache.remove(&id);
        history.commit(&project, &mut cache).unwrap();
        history.undo(&mut project, &mut cache).unwrap();
        assert_eq!(cache[&id].pixel(50, 50), [255, 0, 0, 255]);
        for _ in 0..30 {
            history.undo(&mut project, &mut cache).unwrap();
        }
        assert_eq!(cache[&id].pixel(50, 50), [255; 4]);
        assert!(!cache.has_dirty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_step_whose_document_cannot_be_prepared_touches_no_file() {
        let (mut project, mut cache, _) = setup();
        let dir = std::env::temp_dir().join(format!("oxy-history-files-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let id = new_id();
        project.assets.push(Asset {
            id: id.clone(),
            name: "T".into(),
            path: "unavailable.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        cache.insert(id.clone(), PaintImage::new(16, 16, [255; 4]).unwrap());
        let mut history = CommandHistory::new();
        history.begin("Mover e pintar", &project, &cache);
        project.name = "Depois".into();
        cache
            .get_mut(&id)
            .unwrap()
            .set_pixel(0, 0, [255, 0, 0, 255]);
        // The step moved a file: done when committed.
        std::fs::write(dir.join("b.txt"), b"nota").unwrap();
        let moved = vec![FileOp::Move {
            from: "a.txt".into(),
            to: "b.txt".into(),
        }];
        history
            .commit_with_files(&project, &mut cache, moved)
            .unwrap();
        assert!(history.next_has_files(false));
        // The pixels to put back are gone: preparing the document fails before the disk.
        cache.clear();
        let before = project.clone();
        let mut known = KnownFiles::default();
        let error = history
            .undo_with(&mut project, &mut cache, Some((&dir, &mut known)))
            .unwrap_err();
        assert!(!error.contains("a.txt"), "{error}");
        assert_eq!(project, before);
        assert_eq!(history.undo_len(), 1);
        assert!(dir.join("b.txt").exists() && !dir.join("a.txt").exists());
        assert!(!dir.join(crate::journal::JOURNAL).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_step_of_files_alone_is_kept_leaves_the_project_unsaved_and_needs_its_folder() {
        let (mut project, mut cache, _) = setup();
        let mut history = CommandHistory::new();
        let made = || {
            vec![FileOp::CreateDir {
                path: "Pista".into(),
            }]
        };
        // Files need a step begun for them.
        assert!(
            history
                .commit_with_files(&project, &mut cache, made())
                .is_err()
        );
        history.begin("Criar pasta", &project, &cache);
        assert!(
            history
                .commit_with_files(&project, &mut cache, made())
                .unwrap()
        );
        assert!(history.is_dirty());
        assert!(history.estimated_bytes() > std::mem::size_of::<FileOp>());
        let error = history.undo(&mut project, &mut cache).unwrap_err();
        assert!(error.contains("não tem pasta"), "{error}");
        assert_eq!(history.undo_len(), 1);
    }
    #[test]
    fn failed_undo_leaves_document_pixels_and_stacks_unchanged() {
        let (mut project, mut cache, _) = setup();
        let id = new_id();
        project.assets.push(Asset {
            id: id.clone(),
            name: "T".into(),
            path: "unavailable.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        cache.insert(id.clone(), PaintImage::new(16, 16, [255; 4]).unwrap());
        let mut history = CommandHistory::new();
        history.begin("Nome e pintura", &project, &cache);
        project.name = "Depois".into();
        cache
            .get_mut(&id)
            .unwrap()
            .set_pixel(0, 0, [255, 0, 0, 255]);
        history.commit(&project, &mut cache).unwrap();
        cache.clear();
        let before = project.clone();
        assert!(history.undo(&mut project, &mut cache).is_err());
        assert_eq!(project, before);
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.redo_len(), 0);
        assert!(cache.is_empty());
    }
}
