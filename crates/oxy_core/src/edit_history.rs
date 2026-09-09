//! Command history containing field/collection patches and changed RGBA ranges.
//! A single transient gesture baseline shares image buffers; committed commands
//! never retain a project snapshot or a map of all project textures.
use crate::{
    document::{AssetKind, Id, Project},
    painting::PaintImage,
    texture_cache::TextureCache,
};
use serde_json::Value;
mod meshes;
use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
};

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
        self.revision = 0;
        self.saved_revision = 0;
        self.next_revision = 1;
        self.last_texture_changes.clear();
    }
    pub fn cancel(&mut self, project: &mut Project, images: &mut TextureCache) -> bool {
        if let Some(before) = self.pending.take() {
            *project = before.project;
            *images = before.images;
            true
        } else {
            false
        }
    }
    pub fn commit(&mut self, project: &Project, images: &mut TextureCache) -> Result<bool, String> {
        self.last_texture_changes.clear();
        let Some(mut before) = self.pending.take() else {
            return Ok(false);
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
        if document.is_empty() && deltas.is_empty() && mesh_deltas.is_empty() {
            return Ok(false);
        }
        let next = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1);
        self.past.push(Command {
            label: before.label,
            before_revision: self.revision,
            after_revision: next,
            document,
            images: deltas,
            meshes: mesh_deltas,
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
        self.commit(project, images)?;
        let Some(command) = self.past.last() else {
            return Ok(None);
        };
        apply_command(command, false, project, images)?;
        self.last_texture_changes = command
            .images
            .iter()
            .map(|delta| delta.id().to_owned())
            .collect();
        let command = self.past.pop().unwrap();
        self.revision = command.before_revision;
        let label = command.label.clone();
        self.future.push(command);
        if !self.is_dirty() {
            images.mark_saved();
        }
        Ok(Some(label))
    }
    pub fn redo(
        &mut self,
        project: &mut Project,
        images: &mut TextureCache,
    ) -> Result<Option<String>, String> {
        self.commit(project, images)?;
        let Some(command) = self.future.last() else {
            return Ok(None);
        };
        apply_command(command, true, project, images)?;
        self.last_texture_changes = command
            .images
            .iter()
            .map(|delta| delta.id().to_owned())
            .collect();
        let command = self.future.pop().unwrap();
        self.revision = command.after_revision;
        let label = command.label.clone();
        self.past.push(command);
        if !self.is_dirty() {
            images.mark_saved();
        }
        Ok(Some(label))
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
fn apply_command(
    command: &Command,
    forward: bool,
    project: &mut Project,
    images: &mut TextureCache,
) -> Result<(), String> {
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
    *project = next_project;
    *images = next_images;
    Ok(())
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
