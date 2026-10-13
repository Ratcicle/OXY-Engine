//! Decoded sounds kept between plays, converted once to the mixing format. Past the budget the
//! least recently played leave; a voice still playing one keeps its own copy alive.
use crate::mix::Clip;
use oxy_core::audio::read_wav;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

/// Size and modification time: a file that changed is decoded again.
type Stamp = Option<(u64, SystemTime)>;

struct Entry {
    clip: Arc<Clip>,
    stamp: Stamp,
    used: u64,
}

pub(crate) struct Cache {
    clips: HashMap<PathBuf, Entry>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl Cache {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            clips: HashMap::new(),
            bytes: 0,
            budget,
            clock: 0,
        }
    }
    /// The WAV at `path` at `rate`: decoded on its first play, and again when the file changed.
    pub(crate) fn clip(&mut self, path: &Path, rate: u32) -> Result<Arc<Clip>, String> {
        self.clock += 1;
        let stamp = std::fs::metadata(path)
            .and_then(|meta| Ok((meta.len(), meta.modified()?)))
            .ok();
        if let Some(entry) = self.clips.get_mut(path)
            && stamp.is_some()
            && entry.stamp == stamp
        {
            entry.used = self.clock;
            return Ok(Arc::clone(&entry.clip));
        }
        let clip = Arc::new(Clip::new(&read_wav(path)?, rate));
        self.bytes += clip.bytes();
        let entry = Entry {
            clip: Arc::clone(&clip),
            stamp,
            used: self.clock,
        };
        if let Some(old) = self.clips.insert(path.to_owned(), entry) {
            self.bytes -= old.clip.bytes();
        }
        while self.bytes > self.budget {
            let Some(oldest) = self
                .clips
                .iter()
                .filter(|(other, _)| other.as_path() != path)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(other, _)| other.clone())
            else {
                break;
            };
            if let Some(entry) = self.clips.remove(&oldest) {
                self.bytes -= entry.clip.bytes();
            }
        }
        Ok(clip)
    }
    /// Forgets everything, for another output rate.
    pub(crate) fn clear(&mut self) {
        self.clips.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{Folder, wav};

    #[test]
    fn the_least_recently_played_leave_past_the_budget() {
        let folder = Folder::new("cache");
        // 100 mono frames at 100 Hz are 400 bytes at 100 Hz in stereo.
        let [a, b, c] = ["a.wav", "b.wav", "c.wav"].map(|name| wav(&folder.0, name, 100, 1, 100));
        let mut cache = Cache::new(1000);
        let first = cache.clip(&a, 100).unwrap();
        assert_eq!(first.frames(), 100);
        cache.clip(&b, 100).unwrap();
        // Played again, the same decoded clip; a was used last, so c pushes b out.
        assert!(Arc::ptr_eq(&first, &cache.clip(&a, 100).unwrap()));
        cache.clip(&c, 100).unwrap();
        assert_eq!(cache.bytes, 800);
        let kept = |cache: &Cache, path: &PathBuf| cache.clips.contains_key(path);
        assert!(kept(&cache, &a) && !kept(&cache, &b) && kept(&cache, &c));
        // A file that changed is decoded again.
        wav(&folder.0, "a.wav", 100, 1, 50);
        assert_eq!(cache.clip(&a, 100).unwrap().frames(), 50);
        assert_eq!(cache.bytes, 600);
        // A sound bigger than the budget still plays, alone.
        let mut small = Cache::new(100);
        assert_eq!(small.clip(&c, 100).unwrap().frames(), 100);
        assert!(small.clip(&folder.0.join("nenhum.wav"), 100).is_err());
        assert_eq!(small.clips.len(), 1);
    }
}
