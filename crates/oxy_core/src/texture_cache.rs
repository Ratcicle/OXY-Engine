//! Lazy CPU texture residency. Dirty buffers are pinned until a successful save.
//! Cache clones share pixels; the first mutation in a gesture uses copy-on-write.
use crate::{
    document::{AssetKind, Id, Project},
    painting::PaintImage,
    persistence::resolve_asset_path,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
struct Entry {
    image: Arc<PaintImage>,
    dirty: bool,
    pins: usize,
    touched: u64,
    // One transient edit seed also covers a texture loaded after history.begin.
    seed: Option<Arc<PaintImage>>,
}
#[derive(Clone, Debug)]
pub struct TextureCache {
    entries: HashMap<Id, Entry>,
    sources: HashMap<Id, String>,
    root: PathBuf,
    budget_bytes: usize,
    clock: u64,
    decode_count: u64,
}
impl Default for TextureCache {
    fn default() -> Self {
        Self::new(128 * 1024 * 1024)
    }
}
impl PartialEq for TextureCache {
    fn eq(&self, other: &Self) -> bool {
        if self.sources != other.sources {
            return false;
        }
        let ids: HashSet<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.dirty)
            .map(|(id, _)| id)
            .chain(
                other
                    .entries
                    .iter()
                    .filter(|(_, e)| e.dirty)
                    .map(|(id, _)| id),
            )
            .collect();
        ids.into_iter()
            .all(|id| match (self.entries.get(id), other.entries.get(id)) {
                (Some(a), Some(b)) => Arc::ptr_eq(&a.image, &b.image) || a.image == b.image,
                (None, None) => true,
                _ => false,
            })
    }
}
impl TextureCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            sources: HashMap::new(),
            root: PathBuf::from("."),
            budget_bytes,
            clock: 0,
            decode_count: 0,
        }
    }
    /// Registers file references only. No PNG is decoded by this operation.
    /// Existing buffers survive edits; call clear() before switching projects.
    pub fn configure(&mut self, root: &Path, project: &Project) {
        self.root = root.to_owned();
        self.sources = project
            .assets
            .iter()
            .filter(|asset| asset.kind == AssetKind::Texture)
            .map(|asset| (asset.id.clone(), asset.path.clone()))
            .collect();
        self.entries
            .retain(|id, entry| self.sources.contains_key(id) || entry.dirty);
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }
    pub fn set_budget(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.evict_clean();
    }
    pub fn ensure(&mut self, id: &str) -> Result<(), String> {
        self.clock = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(id) {
            entry.touched = self.clock;
            return Ok(());
        }
        let relative = self
            .sources
            .get(id)
            .ok_or_else(|| format!("Textura não registrada: {id}"))?;
        let image = PaintImage::load(&resolve_asset_path(&self.root, relative)?)?;
        self.decode_count += 1;
        self.entries.insert(
            id.into(),
            Entry {
                image: Arc::new(image),
                dirty: false,
                pins: 0,
                touched: self.clock,
                seed: None,
            },
        );
        self.evict_except(Some(id));
        Ok(())
    }
    pub fn get(&self, id: &str) -> Option<&PaintImage> {
        self.entries.get(id).map(|entry| entry.image.as_ref())
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut PaintImage> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(id)?;
        if entry.seed.is_none() {
            entry.seed = Some(entry.image.clone());
        }
        entry.dirty = true;
        entry.touched = self.clock;
        Some(Arc::make_mut(&mut entry.image))
    }
    pub fn insert(&mut self, id: Id, image: PaintImage) -> Option<Arc<PaintImage>> {
        self.clock = self.clock.wrapping_add(1);
        self.entries
            .insert(
                id,
                Entry {
                    image: Arc::new(image),
                    dirty: true,
                    pins: 0,
                    touched: self.clock,
                    seed: None,
                },
            )
            .map(|entry| entry.image)
    }
    pub fn remove(&mut self, id: &str) -> Option<Arc<PaintImage>> {
        self.sources.remove(id);
        self.entries.remove(id).map(|entry| entry.image)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.sources.clear();
        self.decode_count = 0;
    }
    pub fn contains_key(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }
    pub fn is_registered(&self, id: &str) -> bool {
        self.sources.contains_key(id) || self.entries.contains_key(id)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn registered_len(&self) -> usize {
        self.sources.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&Id, &PaintImage)> {
        self.entries
            .iter()
            .map(|(id, entry)| (id, entry.image.as_ref()))
    }
    pub fn keys(&self) -> impl Iterator<Item = &Id> {
        self.entries.keys()
    }
    pub fn values(&self) -> impl Iterator<Item = &PaintImage> {
        self.entries.values().map(|entry| entry.image.as_ref())
    }
    pub fn dirty_images(&self) -> impl Iterator<Item = (&Id, &PaintImage)> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.dirty)
            .map(|(id, entry)| (id, entry.image.as_ref()))
    }
    pub fn has_dirty(&self) -> bool {
        self.entries.values().any(|entry| entry.dirty)
    }
    pub fn is_dirty(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.dirty)
    }
    pub fn mark_saved(&mut self) {
        for entry in self.entries.values_mut() {
            entry.dirty = false;
            entry.seed = None;
        }
        self.evict_clean();
    }
    pub fn pin(&mut self, id: &str) -> Result<(), String> {
        self.ensure(id)?;
        self.entries.get_mut(id).unwrap().pins += 1;
        Ok(())
    }
    pub fn unpin(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.pins = entry.pins.saturating_sub(1);
        }
        self.evict_clean();
    }
    pub fn pin_only<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
        for entry in self.entries.values_mut() {
            entry.pins = 0;
        }
        for id in ids {
            self.pin(id)?;
        }
        self.evict_clean();
        Ok(())
    }
    pub fn evict_clean(&mut self) -> usize {
        self.evict_except(None)
    }
    fn evict_except(&mut self, keep: Option<&str>) -> usize {
        let mut removed = 0;
        while self.resident_bytes() > self.budget_bytes {
            let candidate = self
                .entries
                .iter()
                .filter(|(id, entry)| !entry.dirty && entry.pins == 0 && Some(id.as_str()) != keep)
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(id, _)| id.clone());
            let Some(id) = candidate else {
                break;
            };
            self.entries.remove(&id);
            removed += 1;
        }
        removed
    }
    /// Counts actual distinct resident pixel allocations, including transient COW seeds.
    pub fn resident_bytes(&self) -> usize {
        let mut allocations = HashSet::new();
        let mut bytes = 0;
        for entry in self.entries.values() {
            for image in std::iter::once(&entry.image).chain(entry.seed.iter()) {
                if allocations.insert(Arc::as_ptr(image)) {
                    bytes += image.pixels.capacity() + std::mem::size_of::<PaintImage>();
                }
            }
        }
        bytes
    }
    pub fn decode_count(&self) -> u64 {
        self.decode_count
    }
    pub fn buffer_identity(&self, id: &str) -> Option<usize> {
        self.entries
            .get(id)
            .map(|entry| Arc::as_ptr(&entry.image) as usize)
    }
    pub(crate) fn shared(&self, id: &str) -> Option<Arc<PaintImage>> {
        self.entries.get(id).map(|entry| entry.image.clone())
    }
    pub(crate) fn seed(&self, id: &str) -> Option<Arc<PaintImage>> {
        self.entries.get(id).and_then(|entry| entry.seed.clone())
    }
    pub(crate) fn set_shared(&mut self, id: Id, image: Arc<PaintImage>, dirty: bool) {
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            id,
            Entry {
                image,
                dirty,
                pins: 0,
                touched: self.clock,
                seed: None,
            },
        );
    }
    pub(crate) fn set_dirty(&mut self, id: &str, dirty: bool) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.dirty = dirty;
        }
    }
    pub(crate) fn finish_gesture(&mut self) {
        for entry in self.entries.values_mut() {
            entry.seed = None;
        }
        self.evict_clean();
    }
    pub(crate) fn prune_removed(&mut self, project: &Project) {
        let ids: HashSet<_> = project
            .assets
            .iter()
            .filter(|asset| asset.kind == AssetKind::Texture)
            .map(|asset| asset.id.as_str())
            .collect();
        self.entries.retain(|id, _| ids.contains(id.as_str()));
        self.sources.retain(|id, _| ids.contains(id.as_str()));
    }
}

pub struct TextureIter<'a>(std::collections::hash_map::Iter<'a, Id, Entry>);
impl<'a> Iterator for TextureIter<'a> {
    type Item = (&'a Id, &'a PaintImage);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(id, entry)| (id, entry.image.as_ref()))
    }
}
impl<'a> IntoIterator for &'a TextureCache {
    type Item = (&'a Id, &'a PaintImage);
    type IntoIter = TextureIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        TextureIter(self.entries.iter())
    }
}
impl std::ops::Index<&str> for TextureCache {
    type Output = PaintImage;
    fn index(&self, id: &str) -> &Self::Output {
        self.get(id)
            .expect("Textura não residente: use ensure antes de ler")
    }
}
impl std::ops::Index<&String> for TextureCache {
    type Output = PaintImage;
    fn index(&self, id: &String) -> &Self::Output {
        &self[id.as_str()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Asset, new_id};
    #[test]
    fn dirty_is_pinned_and_clones_share_until_a_mutation() {
        let mut cache = TextureCache::new(1);
        let id = new_id();
        cache.insert(id.clone(), PaintImage::new(1024, 1024, [255; 4]).unwrap());
        let before = cache.clone();
        assert_eq!(cache.buffer_identity(&id), before.buffer_identity(&id));
        cache.get_mut(&id).unwrap().set_pixel(4, 5, [1, 2, 3, 255]);
        assert_ne!(cache.buffer_identity(&id), before.buffer_identity(&id));
        assert_eq!(before.get(&id).unwrap().pixel(4, 5), [255; 4]);
        assert_eq!(cache.evict_clean(), 0);
        assert!(cache.get(&id).is_some());
        cache.mark_saved();
        assert!(cache.get(&id).is_none());
    }
    #[test]
    fn registration_is_lazy_and_clean_residency_is_bounded() {
        let root = std::env::temp_dir().join(format!("oxy-cache-{}", new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut project = Project::new("lazy");
        for index in 0..3 {
            let id = new_id();
            let path = format!("{index}.png");
            PaintImage::new(256, 256, [index; 4])
                .unwrap()
                .save(&root.join(&path))
                .unwrap();
            project.assets.push(Asset {
                id,
                name: format!("{index}"),
                path,
                kind: AssetKind::Texture,
                model: None,
            });
        }
        let mut cache = TextureCache::new(300_000);
        cache.configure(&root, &project);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.decode_count(), 0);
        let before = cache.clone();
        cache.ensure(&project.assets[0].id).unwrap();
        assert_eq!(cache, before);
        cache.ensure(&project.assets[1].id).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.decode_count(), 2);
        cache.pin(&project.assets[1].id).unwrap();
        cache.ensure(&project.assets[2].id).unwrap();
        assert_eq!(cache.len(), 2);
        cache.unpin(&project.assets[1].id);
        assert!(cache.resident_bytes() <= 300_000);
        std::fs::remove_dir_all(root).unwrap();
    }
}
