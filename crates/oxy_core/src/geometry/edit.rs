//! Atomic geometry edits: validate a private candidate before exposing it to editor/runtime.
use super::{
    EditableMesh,
    selection::{Mode, Selection},
};
use glam::{Mat4, Vec3};
use std::collections::HashSet;
pub fn transform(
    mesh: &EditableMesh,
    selection: &Selection,
    delta: Mat4,
) -> Result<EditableMesh, String> {
    if delta == Mat4::IDENTITY {
        return Ok(mesh.clone());
    }
    if !delta.is_finite() || delta.determinant().abs() < 1e-8 {
        return Err("Transformação não invertível; escolha uma escala diferente de zero.".into());
    }
    let ids: HashSet<_> = selection.vertices(mesh).into_iter().collect();
    if ids.is_empty() {
        return Err("Selecione componentes para transformar.".into());
    }
    let mut data = mesh.data().clone();
    for v in &mut data.vertices {
        if ids.contains(&v.id) {
            v.position = delta.transform_point3(Vec3::from(v.position)).to_array();
        }
    }
    for face in &mut data.faces {
        for corner in &mut face.corners {
            corner.normal = None;
        }
    }
    EditableMesh::new(data)
}
pub fn delete(
    mesh: &EditableMesh,
    selection: &Selection,
    keep_edges: bool,
) -> Result<EditableMesh, String> {
    if selection.ids.is_empty() {
        return Err("Selecione componentes para excluir.".into());
    }
    let ids: HashSet<_> = selection.ids.iter().copied().collect();
    let mut data = mesh.data().clone();
    match selection.mode {
        Mode::Object => {
            return Err("Use o modo Face, Aresta ou Vértice para excluir componentes.".into());
        }
        Mode::Face => {
            data.faces.retain(|f| !ids.contains(&f.id));
            if !keep_edges {
                let used: HashSet<_> = data
                    .faces
                    .iter()
                    .flat_map(|f| {
                        f.corners
                            .iter()
                            .zip(f.corners.iter().cycle().skip(1))
                            .take(f.corners.len())
                            .map(|(a, b)| super::pair([a.vertex, b.vertex]))
                    })
                    .collect();
                let removed: HashSet<_> = mesh
                    .data()
                    .faces
                    .iter()
                    .filter(|f| ids.contains(&f.id))
                    .flat_map(|f| {
                        f.corners
                            .iter()
                            .zip(f.corners.iter().cycle().skip(1))
                            .take(f.corners.len())
                            .map(|(a, b)| super::pair([a.vertex, b.vertex]))
                    })
                    .collect();
                data.edges.retain(|e| {
                    !removed.contains(&super::pair(e.vertices))
                        || used.contains(&super::pair(e.vertices))
                });
            }
        }
        Mode::Edge => {
            let faces: HashSet<_> = selection
                .ids
                .iter()
                .filter_map(|id| mesh.prepared().edges.get(id))
                .flat_map(|&i| {
                    mesh.prepared().incident_faces[i]
                        .iter()
                        .map(|&(fi, _)| mesh.data().faces[fi].id)
                })
                .collect();
            data.faces.retain(|f| !faces.contains(&f.id));
            data.edges.retain(|e| !ids.contains(&e.id));
        }
        Mode::Vertex => {
            data.faces
                .retain(|f| !f.corners.iter().any(|c| ids.contains(&c.vertex)));
            data.edges
                .retain(|e| !e.vertices.iter().any(|id| ids.contains(id)));
            data.vertices.retain(|v| !ids.contains(&v.id));
        }
    }
    EditableMesh::new(data)
}
pub fn triangulate(mesh: &EditableMesh, selection: &Selection) -> Result<EditableMesh, String> {
    if selection.mode != Mode::Face || selection.ids.is_empty() {
        return Err("Selecione as faces a triangular.".into());
    }
    let ids: HashSet<_> = selection.ids.iter().copied().collect();
    let mut data = mesh.data().clone();
    data.faces.retain(|f| !ids.contains(&f.id));
    for triangle in &mesh.prepared().triangles {
        if ids.contains(&triangle.face) {
            let face = mesh.face(triangle.face).unwrap();
            data.add_face(triangle.corners.map(|i| face.corners[i].clone()).to_vec())?;
        }
    }
    data.complete_edges()?;
    EditableMesh::new(data)
}
