//! Bounded before/after of only affected immutable meshes. Keeping geometry out of the generic
//! JSON patch avoids serializing and validating every unrelated mesh on each undo or paint stroke.
use crate::{
    document::{Entity, Project},
    geometry::EditableMesh,
};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Location {
    Scene(String, String),
    Model(String, String),
}
type Map = BTreeMap<Location, EditableMesh>;
#[derive(Debug)]
pub(super) struct Delta {
    location: Location,
    before: Option<EditableMesh>,
    after: Option<EditableMesh>,
}
impl Delta {
    pub(super) fn bytes(&self, seen: &mut HashSet<u64>) -> usize {
        let (container, entity) = match &self.location {
            Location::Scene(a, b) | Location::Model(a, b) => (a, b),
        };
        std::mem::size_of::<Self>()
            + container.capacity()
            + entity.capacity()
            + [&self.before, &self.after]
                .into_iter()
                .flatten()
                .filter(|m| seen.insert(m.revision()))
                .map(EditableMesh::estimated_bytes)
                .sum::<usize>()
    }
}
fn entities(project: &Project) -> impl Iterator<Item = &Entity> {
    project.scenes.iter().flat_map(|s| s.entities.iter()).chain(
        project
            .assets
            .iter()
            .filter_map(|a| a.model.as_ref())
            .flatten(),
    )
}
pub(super) fn detach(project: &Project) -> (Cow<'_, Project>, Map) {
    if !entities(project).any(|e| e.mesh.is_some()) {
        return (Cow::Borrowed(project), Map::new());
    }
    let mut document = project.clone();
    let mut map = Map::new();
    for scene in &mut document.scenes {
        for entity in &mut scene.entities {
            if let Some(mesh) = entity.mesh.take() {
                map.insert(Location::Scene(scene.id.clone(), entity.id.clone()), mesh);
            }
        }
    }
    for asset in &mut document.assets {
        if let Some(model) = &mut asset.model {
            for entity in model {
                if let Some(mesh) = entity.mesh.take() {
                    map.insert(Location::Model(asset.id.clone(), entity.id.clone()), mesh);
                }
            }
        }
    }
    (Cow::Owned(document), map)
}
pub(super) fn diff(before: &Map, after: &Map) -> Vec<Delta> {
    let mut keys = before.keys().chain(after.keys()).collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter_map(|location| {
            let a = before.get(location);
            let b = after.get(location);
            (a != b).then(|| Delta {
                location: location.clone(),
                before: a.cloned(),
                after: b.cloned(),
            })
        })
        .collect()
}
pub(super) fn apply(
    deltas: &[Delta],
    forward: bool,
    mut map: Map,
    project: &mut Project,
) -> Result<(), String> {
    if deltas.is_empty() && map.is_empty() {
        return Ok(());
    }
    for delta in deltas {
        if let Some(mesh) = if forward { &delta.after } else { &delta.before } {
            map.insert(delta.location.clone(), mesh.clone());
        } else {
            map.remove(&delta.location);
        }
    }
    for scene in &mut project.scenes {
        for entity in &mut scene.entities {
            entity.mesh = map.remove(&Location::Scene(scene.id.clone(), entity.id.clone()));
        }
    }
    for asset in &mut project.assets {
        if let Some(model) = &mut asset.model {
            for entity in model {
                entity.mesh = map.remove(&Location::Model(asset.id.clone(), entity.id.clone()));
            }
        }
    }
    if !map.is_empty() {
        return Err(
            "Não foi possível restaurar a malha: objeto ou modelo ausente no histórico.".into(),
        );
    }
    Ok(())
}
