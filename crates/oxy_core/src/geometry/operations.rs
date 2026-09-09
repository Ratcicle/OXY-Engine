//! Topology operators build a private, validated result; no document or texture is mutated here.
use super::{
    ComponentId as Id, Corner, Edge, EditableMesh, MeshData, pair,
    selection::{Mode, Selection},
};
use glam::{Vec2, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct Output {
    pub mesh: EditableMesh,
    pub selection: Selection,
    /// Faces requiring independent atlas space. Existing UVs remain untouched.
    pub new_faces: Vec<Id>,
}
fn selected(mesh: &EditableMesh, selection: &Selection) -> Result<HashSet<Id>, String> {
    let mut valid = selection.clone();
    valid.sanitize(mesh);
    if valid.ids.is_empty() || valid.ids != selection.ids {
        return Err("Selecione componentes existentes da malha.".into());
    }
    Ok(selection.ids.iter().copied().collect())
}
fn result(mut data: MeshData, selection: Selection, new_faces: Vec<Id>) -> Result<Output, String> {
    data.complete_edges()?;
    Ok(Output {
        mesh: EditableMesh::new(data)?,
        selection,
        new_faces,
    })
}
fn corners(ids: &[Id]) -> Vec<Corner> {
    ids.iter()
        .enumerate()
        .map(|(i, &vertex)| Corner {
            vertex,
            uv: match i % 4 {
                0 => [0., 0.],
                1 => [1., 0.],
                2 => [1., 1.],
                _ => [0., 1.],
            },
            normal: None,
        })
        .collect()
}
fn edge(data: &mut MeshData, a: Id, b: Id) -> Result<Id, String> {
    let id = data.allocate_id()?;
    data.edges.push(Edge {
        id,
        vertices: [a, b],
    });
    Ok(id)
}

fn boundary_direction(mesh: &EditableMesh, id: Id) -> Option<[Id; 2]> {
    let i = mesh.prepared().edges[&id];
    mesh.prepared().incident_faces[i].first().map(|&(f, c)| {
        let face = &mesh.data().faces[f];
        [
            face.corners[(c + 1) % face.corners.len()].vertex,
            face.corners[c].vertex,
        ]
    })
}

/// A strip must enter/leave each shared vertex in opposite directions. Existing faces anchor
/// the winding; otherwise the first selected edge deterministically orients its whole component.
fn strip_directions(
    mesh: &EditableMesh,
    ids: &[Id],
    neighbors: &HashMap<Id, Vec<Id>>,
) -> Result<HashMap<Id, [Id; 2]>, String> {
    let mut directions = HashMap::new();
    // Process anchors first, so a loose edge cannot incorrectly reject a solvable component.
    for &seed in ids
        .iter()
        .filter(|&&id| boundary_direction(mesh, id).is_some())
        .chain(ids.iter())
    {
        if directions.contains_key(&seed) {
            continue;
        }
        directions.insert(
            seed,
            boundary_direction(mesh, seed).unwrap_or(mesh.edge(seed).unwrap().vertices),
        );
        let mut queue = VecDeque::from([seed]);
        while let Some(id) = queue.pop_front() {
            let ends = directions[&id];
            for vertex in ends {
                for &next in &neighbors[&vertex] {
                    if next == id {
                        continue;
                    }
                    let next_ends = mesh.edge(next).unwrap().vertices;
                    let other = if next_ends[0] == vertex {
                        next_ends[1]
                    } else {
                        next_ends[0]
                    };
                    let proposed = if vertex == ends[0] {
                        [other, vertex]
                    } else {
                        [vertex, other]
                    };
                    if boundary_direction(mesh, next).is_some_and(|v| v != proposed)
                        || directions.get(&next).is_some_and(|&v| v != proposed)
                    {
                        return Err("As faces incidentes impõem sentidos incompatíveis à faixa. Inverta a orientação das faces afetadas ou extruda cadeias separadas.".into());
                    }
                    if let std::collections::hash_map::Entry::Vacant(entry) = directions.entry(next)
                    {
                        entry.insert(proposed);
                        queue.push_back(next);
                    }
                }
            }
        }
    }
    Ok(directions)
}

pub fn extrude(
    mesh: &EditableMesh,
    selection: &Selection,
    delta: Vec3,
    per_face: bool,
) -> Result<Output, String> {
    let ids = selected(mesh, selection)?;
    if !delta.is_finite() || delta.length() < 1e-6 {
        return Err("Escolha uma direção e distância diferentes de zero para extrudir.".into());
    }
    let mut data = mesh.data().clone();
    let mut new_faces = Vec::new();
    let mut next = selection.clone();
    match selection.mode {
        Mode::Object => return Err("Extrudir requer Face, Aresta ou Vértice.".into()),
        Mode::Vertex => {
            next.ids.clear();
            for &id in &selection.ids {
                let v = data.add_vertex(mesh.position(id).unwrap() + delta)?;
                edge(&mut data, id, v)?;
                next.ids.push(v);
            }
        }
        Mode::Edge => {
            let mut neighbors = HashMap::<Id, Vec<Id>>::new();
            for &id in &selection.ids {
                let i = mesh.prepared().edges[&id];
                if mesh.prepared().incident_faces[i].len() > 1 {
                    return Err("Extrusão de aresta aceita bordas abertas ou arestas soltas; esta aresta já possui duas faces.".into());
                }
                for v in mesh.data().edges[i].vertices {
                    neighbors.entry(v).or_default().push(id);
                }
            }
            if neighbors.values().any(|v| v.len() > 2) {
                return Err("Selecione uma cadeia ou laço sem ramificações.".into());
            }
            let directions = strip_directions(mesh, &selection.ids, &neighbors)?;
            let mut copies = HashMap::new();
            // Preserve document order; hash tables only resolve IDs.
            for v in &mesh.data().vertices {
                if neighbors.contains_key(&v.id) {
                    copies.insert(v.id, data.add_vertex(Vec3::from(v.position) + delta)?);
                }
            }
            next.ids.clear();
            for &id in &selection.ids {
                let [a, b] = directions[&id];
                let [aa, bb] = [copies[&a], copies[&b]];
                new_faces.push(data.add_face(corners(&[a, b, bb, aa]))?);
                next.ids.push(edge(&mut data, aa, bb)?);
            }
        }
        Mode::Face => {
            let mut copies = HashMap::new();
            let mut boundaries = Vec::new();
            let mut remove = HashSet::new();
            if !per_face {
                let vertices = selection.vertices(mesh).into_iter().collect::<HashSet<_>>();
                for v in &mesh.data().vertices {
                    if vertices.contains(&v.id) {
                        copies.insert(v.id, data.add_vertex(Vec3::from(v.position) + delta)?);
                    }
                }
                for (i, e) in mesh.data().edges.iter().enumerate() {
                    let uses = mesh.prepared().incident_faces[i]
                        .iter()
                        .filter(|&&(f, _)| ids.contains(&mesh.data().faces[f].id))
                        .count();
                    if uses == 2 {
                        remove.insert(e.id);
                    }
                }
            }
            for face in &mesh.data().faces {
                if !ids.contains(&face.id) {
                    continue;
                }
                if per_face {
                    copies.clear();
                    for c in &face.corners {
                        copies.insert(
                            c.vertex,
                            data.add_vertex(mesh.position(c.vertex).unwrap() + delta)?,
                        );
                    }
                }
                for (a, b) in face
                    .corners
                    .iter()
                    .zip(face.corners.iter().cycle().skip(1))
                    .take(face.corners.len())
                {
                    let i = mesh.prepared().edge_pairs[&pair([a.vertex, b.vertex])];
                    let interior = mesh.prepared().incident_faces[i]
                        .iter()
                        .filter(|&&(f, _)| ids.contains(&mesh.data().faces[f].id))
                        .count()
                        == 2;
                    if per_face || !interior {
                        boundaries.push([a.vertex, b.vertex, copies[&b.vertex], copies[&a.vertex]]);
                    }
                }
                let fi = mesh.prepared().faces[&face.id];
                for c in &mut data.faces[fi].corners {
                    c.vertex = copies[&c.vertex];
                }
            }
            if boundaries.is_empty() {
                return Err("A região precisa de uma borda. Para mover uma superfície fechada inteira, use Mover.".into());
            }
            data.edges.retain(|e| !remove.contains(&e.id));
            for boundary in boundaries {
                new_faces.push(data.add_face(corners(&boundary))?);
            }
            // Drop only original vertices made unused by this surface operation; retain loose inputs.
            let originally_used: HashSet<_> = mesh
                .data()
                .faces
                .iter()
                .flat_map(|f| f.corners.iter().map(|c| c.vertex))
                .collect();
            let used: HashSet<_> = data
                .faces
                .iter()
                .flat_map(|f| f.corners.iter().map(|c| c.vertex))
                .chain(data.edges.iter().flat_map(|e| e.vertices))
                .collect();
            data.vertices
                .retain(|v| !originally_used.contains(&v.id) || used.contains(&v.id));
        }
    }
    result(data, next, new_faces)
}

/// A single unbranched boundary loop, oriented opposite the existing incident faces.
fn boundary_loop(mesh: &EditableMesh, ids: &[Id]) -> Result<Vec<Id>, String> {
    if ids.len() < 3 {
        return Err("Selecione um contorno fechado de pelo menos três arestas de borda.".into());
    }
    let mut neighbors = HashMap::<Id, Vec<Id>>::new();
    for &id in ids {
        let i = *mesh.prepared().edges.get(&id).ok_or("Aresta ausente.")?;
        if mesh.prepared().incident_faces[i].len() > 1 {
            return Err(
                "O contorno contém uma aresta interna. Escolha somente a borda de uma abertura."
                    .into(),
            );
        }
        let [a, b] = mesh.data().edges[i].vertices;
        neighbors.entry(a).or_default().push(b);
        neighbors.entry(b).or_default().push(a);
    }
    if neighbors.values().any(|v| v.len() != 2) {
        return Err("O contorno precisa estar fechado, sem pontas ou ramificações.".into());
    }
    let first = mesh.edge(ids[0]).unwrap().vertices;
    let mut ordered = vec![first[0]];
    let mut previous = first[0];
    let mut current = first[1];
    while current != ordered[0] {
        if ordered.len() >= ids.len() {
            return Err("Contorno ambíguo.".into());
        }
        ordered.push(current);
        let n = &neighbors[&current];
        let next = if n[0] == previous { n[1] } else { n[0] };
        previous = current;
        current = next;
    }
    if ordered.len() != ids.len() {
        return Err("Escolha uma única abertura por operação.".into());
    }
    orient_boundary(mesh, &mut ordered)?;
    Ok(ordered)
}
fn orient_boundary(mesh: &EditableMesh, ordered: &mut [Id]) -> Result<(), String> {
    let mut same = None;
    for (&a, &b) in ordered
        .iter()
        .zip(ordered.iter().cycle().skip(1))
        .take(ordered.len())
    {
        if let Some(&i) = mesh.prepared().edge_pairs.get(&pair([a, b])) {
            if mesh.prepared().incident_faces[i].len() > 1 {
                return Err("Uma aresta do contorno já possui duas faces.".into());
            }
            if let Some(&(f, c)) = mesh.prepared().incident_faces[i].first() {
                let direction = mesh.data().faces[f].corners[c].vertex == a;
                if same.is_some_and(|v| v != direction) {
                    return Err("As faces da borda têm orientações incompatíveis. Inverta a orientação das faces afetadas antes de fechar.".into());
                }
                same = Some(direction);
            }
        }
    }
    if same == Some(true) {
        ordered.reverse();
    }
    Ok(())
}
pub fn create(mesh: &EditableMesh, selection: &Selection) -> Result<Output, String> {
    let ids = selected(mesh, selection)?;
    let mut data = mesh.data().clone();
    let mut next = selection.clone();
    if selection.mode == Mode::Vertex && selection.ids.len() == 2 {
        let [a, b] = [selection.ids[0], selection.ids[1]];
        if mesh.prepared().edge_pairs.contains_key(&pair([a, b])) {
            return Err("Estes vértices já estão ligados por uma aresta.".into());
        }
        next.mode = Mode::Edge;
        next.ids = vec![edge(&mut data, a, b)?];
        return result(data, next, vec![]);
    }
    let mut ordered=match selection.mode {
        Mode::Vertex if selection.ids.len()>=3=>selection.ids.clone(),
        Mode::Edge=>boundary_loop(mesh,&selection.ids)?,
        Mode::Face=>{
            let border:Vec<_>=mesh.data().edges.iter().enumerate().filter(|(i,_)|{
                let uses=&mesh.prepared().incident_faces[*i];uses.len()==1&&ids.contains(&mesh.data().faces[uses[0].0].id)
            }).map(|(_,e)|e.id).collect();boundary_loop(mesh,&border)?
        }
        _=>return Err("Use dois vértices para uma aresta, pontos ordenados para uma face ou as arestas de uma abertura fechada.".into()),
    };
    orient_boundary(mesh, &mut ordered)?;
    let mut face = corners(&ordered);
    // Real planar coordinates; atlas allocation will reserve a separate region before commit.
    let points = ordered
        .iter()
        .map(|id| mesh.position(*id).unwrap())
        .collect::<Vec<_>>();
    let uv = project_polygon(&points)?;
    for (c, uv) in face.iter_mut().zip(uv) {
        c.uv = uv.to_array();
    }
    let id = data.add_face(face)?;
    next.mode = Mode::Face;
    next.ids = vec![id];
    result(data, next, vec![id])
}
pub(crate) fn project_polygon(points: &[Vec3]) -> Result<Vec<Vec2>, String> {
    let normal = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.cross(*b))
        .sum::<Vec3>()
        .normalize_or_zero();
    if normal.length_squared() < 0.5 {
        return Err("Os pontos não formam uma superfície com área.".into());
    }
    let axis = normal.abs().max_element();
    let project = |p: Vec3| {
        if normal.x.abs() == axis {
            Vec2::new(p.z, p.y)
        } else if normal.y.abs() == axis {
            Vec2::new(p.x, p.z)
        } else {
            Vec2::new(p.x, p.y)
        }
    };
    let values = points.iter().map(|&p| project(p)).collect::<Vec<_>>();
    let min = values
        .iter()
        .copied()
        .fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let max = values
        .iter()
        .copied()
        .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    let size = (max - min).max_element();
    if size < 1e-7 {
        return Err("Superfície muito pequena para mapear.".into());
    }
    Ok(values.into_iter().map(|p| (p - min) / size).collect())
}
pub fn flip(mesh: &EditableMesh, selection: &Selection) -> Result<Output, String> {
    let ids = selected(mesh, selection)?;
    if selection.mode != Mode::Face {
        return Err("Selecione faces para inverter a orientação.".into());
    }
    let mut data = mesh.data().clone();
    for face in &mut data.faces {
        if ids.contains(&face.id) {
            face.corners.reverse();
            for c in &mut face.corners {
                c.normal = c.normal.map(|n| (-Vec3::from(n)).to_array());
            }
        }
    }
    result(data, selection.clone(), vec![])
}
