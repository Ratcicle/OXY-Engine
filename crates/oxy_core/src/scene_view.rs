//! Explicit immutable read phase. Borrowing prevents stale indices after edits.
//! Derived data never enters JSON, equality or history.
use crate::{document::*, metrics};
use glam::Mat4;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};
#[derive(Debug)]
pub struct SceneIndex {
    positions: HashMap<Id, usize>,
    pub children: Vec<Vec<usize>>,
    pub roots: Vec<usize>,
}
impl SceneIndex {
    pub fn new(scene: &Scene) -> Result<Self, String> {
        metrics::count(|c| c.index_builds += 1);
        let mut positions = HashMap::with_capacity(scene.entities.len());
        for (i, e) in scene.entities.iter().enumerate() {
            if e.id.is_empty() || positions.insert(e.id.clone(), i).is_some() {
                return Err(format!("ID vazio ou duplicado: {}", e.id));
            }
        }
        let mut children = vec![Vec::new(); scene.entities.len()];
        let mut roots = Vec::new();
        for (i, e) in scene.entities.iter().enumerate() {
            if let Some(parent) = &e.parent {
                if let Some(&p) = positions.get(parent) {
                    children[p].push(i);
                }
            } else {
                roots.push(i);
            }
        }
        Ok(Self {
            positions,
            children,
            roots,
        })
    }
    pub fn position(&self, id: &str) -> Option<usize> {
        metrics::count(|c| c.entity_queries += 1);
        self.positions.get(id).copied()
    }
    pub fn descendants(&self, scene: &Scene, id: &str) -> Vec<Id> {
        let Some(root) = self.position(id) else {
            return vec![id.to_owned()];
        };
        let mut queue = vec![root];
        let mut visited = HashSet::from([root]);
        let mut cursor = 0;
        while cursor < queue.len() {
            for &child in &self.children[queue[cursor]] {
                metrics::count(|c| c.hierarchy_visits += 1);
                if visited.insert(child) {
                    queue.push(child);
                }
            }
            cursor += 1;
        }
        queue
            .into_iter()
            .map(|i| scene.entities[i].id.clone())
            .collect()
    }
    pub fn related(&self, scene: &Scene, a: &str, b: &str) -> bool {
        let ancestor = |start: &str, target: &str| {
            let mut current = Some(start);
            for _ in 0..=scene.entities.len() {
                let Some(id) = current else { return false };
                if id == target {
                    return true;
                }
                metrics::count(|c| c.hierarchy_visits += 1);
                current = self
                    .position(id)
                    .and_then(|i| scene.entities[i].parent.as_deref());
            }
            false
        };
        ancestor(a, b) || ancestor(b, a)
    }
}
pub struct SceneView<'a> {
    pub scene: &'a Scene,
    pub index: Result<SceneIndex, String>,
    matrices: RefCell<Vec<Option<Mat4>>>,
}

/// Owned data for the sequential controller phase. Every moved subtree is
/// refreshed before the next controller sees it. No structural edits in this phase.
pub struct SceneEvaluation {
    pub index: SceneIndex,
    pub worlds: Vec<Option<Mat4>>,
    pub boxes: Vec<Option<crate::collision::Aabb>>,
}
impl SceneEvaluation {
    pub fn new(scene: &Scene) -> Result<Self, String> {
        let view = SceneView::new(scene);
        let worlds: Vec<_> = scene
            .entities
            .iter()
            .map(|e| view.world_matrix(&e.id).ok())
            .collect();
        let boxes = scene
            .entities
            .iter()
            .zip(&worlds)
            .map(|(e, w)| Self::bounds(e, *w))
            .collect();
        Ok(Self {
            index: view.index?,
            worlds,
            boxes,
        })
    }
    fn bounds(entity: &Entity, world: Option<Mat4>) -> Option<crate::collision::Aabb> {
        let collider = entity.collider.as_ref().filter(|c| c.enabled)?;
        crate::spatial::bounds_from_world(collider, world?).ok()
    }
    pub fn refresh_subtree(&mut self, scene: &Scene, id: &str) {
        for id in self.index.descendants(scene, id) {
            let Some(i) = self.index.position(&id) else {
                continue;
            };
            let e = &scene.entities[i];
            let parent = match &e.parent {
                Some(id) => self.index.position(id).and_then(|p| self.worlds[p]),
                None => Some(Mat4::IDENTITY),
            };
            crate::metrics::count(|c| c.matrices += 1);
            self.worlds[i] = parent.map(|p| p * e.transform.matrix());
            self.boxes[i] = Self::bounds(e, self.worlds[i]);
        }
    }
}
impl<'a> SceneView<'a> {
    pub fn new(scene: &'a Scene) -> Self {
        Self {
            scene,
            index: SceneIndex::new(scene),
            matrices: RefCell::new(vec![None; scene.entities.len()]),
        }
    }
    pub fn entity(&self, id: &str) -> Option<&'a Entity> {
        self.index
            .as_ref()
            .ok()?
            .position(id)
            .map(|i| &self.scene.entities[i])
    }
    pub fn world_matrix(&self, id: &str) -> Result<Mat4, String> {
        let index = self.index.as_ref().map_err(Clone::clone)?;
        let root = index
            .position(id)
            .ok_or_else(|| format!("Objeto ausente: {id}"))?;
        let mut matrices = self.matrices.borrow_mut();
        let mut current = root;
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut world = loop {
            if let Some(world) = matrices[current] {
                metrics::count(|c| c.matrix_hits += 1);
                break world;
            }
            if !visited.insert(current) {
                return Err("Ciclo na hierarquia".into());
            }
            chain.push(current);
            let Some(parent) = self.scene.entities[current].parent.as_deref() else {
                break Mat4::IDENTITY;
            };
            current = index
                .position(parent)
                .ok_or_else(|| format!("Objeto ausente: {parent}"))?;
        };
        for i in chain.into_iter().rev() {
            metrics::count(|c| c.matrices += 1);
            world *= self.scene.entities[i].transform.matrix();
            matrices[i] = Some(world);
        }
        Ok(world)
    }
    pub fn descendants(&self, id: &str) -> Vec<Id> {
        self.index
            .as_ref()
            .map_or_else(|_| Vec::new(), |i| i.descendants(self.scene, id))
    }
    pub fn visible(&self, entity: &Entity) -> bool {
        let mut current = Some(entity);
        for _ in 0..=self.scene.entities.len() {
            let Some(e) = current else { return true };
            if !e.visible {
                return false;
            }
            current = e.parent.as_deref().and_then(|id| self.entity(id));
        }
        false
    }
    pub fn collider_bounds(&self, id: &str) -> Result<crate::collision::Aabb, String> {
        let collider = self
            .entity(id)
            .and_then(|e| e.collider.as_ref())
            .ok_or("Objeto sem colisor")?;
        crate::spatial::bounds_from_world(collider, self.world_matrix(id)?)
    }
}
