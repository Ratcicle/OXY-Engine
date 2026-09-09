//! Ordered component selection belongs to the editor, never the project or runtime.
use super::{ComponentId, EditableMesh};
use std::collections::HashSet;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Object,
    Face,
    Edge,
    Vertex,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub mode: Mode,
    pub ids: Vec<ComponentId>,
    pub through: bool,
}
impl Selection {
    pub fn click(&mut self, id: Option<ComponentId>, toggle: bool) {
        if !toggle {
            self.ids.clear();
        }
        if let Some(id) = id {
            if toggle && self.ids.contains(&id) {
                self.ids.retain(|v| *v != id);
            } else {
                self.ids.push(id);
            }
        }
    }
    pub fn available(&self, mesh: &EditableMesh) -> Vec<ComponentId> {
        match self.mode {
            Mode::Object => vec![],
            Mode::Face => mesh.data().faces.iter().map(|v| v.id).collect(),
            Mode::Edge => mesh.data().edges.iter().map(|v| v.id).collect(),
            Mode::Vertex => mesh.data().vertices.iter().map(|v| v.id).collect(),
        }
    }
    pub fn sanitize(&mut self, mesh: &EditableMesh) {
        let all: HashSet<_> = self.available(mesh).into_iter().collect();
        let mut seen = HashSet::new();
        self.ids.retain(|id| all.contains(id) && seen.insert(*id));
    }
    pub fn invert(&mut self, mesh: &EditableMesh) {
        let old: HashSet<_> = self.ids.iter().copied().collect();
        self.ids = self
            .available(mesh)
            .into_iter()
            .filter(|id| !old.contains(id))
            .collect();
    }
    pub fn vertices(&self, mesh: &EditableMesh) -> Vec<ComponentId> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut add = |id| {
            if seen.insert(id) {
                result.push(id);
            }
        };
        for id in &self.ids {
            match self.mode {
                Mode::Object => {}
                Mode::Vertex => {
                    if mesh.position(*id).is_some() {
                        add(*id);
                    }
                }
                Mode::Edge => {
                    if let Some(e) = mesh.edge(*id) {
                        for v in e.vertices {
                            add(v);
                        }
                    }
                }
                Mode::Face => {
                    if let Some(f) = mesh.face(*id) {
                        for c in &f.corners {
                            add(c.vertex);
                        }
                    }
                }
            }
        }
        result
    }
}
