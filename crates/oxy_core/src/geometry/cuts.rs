//! Shared edge subdivision for loop cuts and visible surface knife paths.
use super::{
    ComponentId as Id, Corner, EditableMesh, MeshData,
    operations::Output,
    pair,
    selection::{Mode, Selection},
};
use glam::{Vec2, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub edge: Id,
    pub factor: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub face: Id,
    pub from: Point,
    pub to: Point,
}
pub struct Cut {
    pub output: Output,
    pub notes: Vec<String>,
}

struct Split {
    ends: [Id; 2],
    points: Vec<(f32, Id)>,
}
fn subdivide(
    mesh: &EditableMesh,
    points: &[Point],
) -> Result<(MeshData, HashMap<Id, Split>), String> {
    let mut factors = HashMap::<Id, Vec<f32>>::new();
    for p in points {
        if mesh.edge(p.edge).is_none() || !p.factor.is_finite() || !(0. ..=1.).contains(&p.factor) {
            return Err("Ponto de corte fora de uma aresta válida.".into());
        }
        if p.factor > 1e-6 && p.factor < 1. - 1e-6 {
            factors.entry(p.edge).or_default().push(p.factor);
        }
    }
    let mut data = mesh.data().clone();
    let mut splits = HashMap::new();
    for (ei, e) in mesh.data().edges.iter().enumerate() {
        if let Some(factors) = factors.get_mut(&e.id) {
            factors.sort_by(f32::total_cmp);
            factors.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
            let a = mesh.position(e.vertices[0]).unwrap();
            let b = mesh.position(e.vertices[1]).unwrap();
            let mut points = Vec::new();
            for &t in factors.iter() {
                points.push((t, data.add_vertex(a.lerp(b, t))?));
            }
            // Retain the old edge identity on its first segment; every incident face receives the same split IDs.
            data.edges[ei].vertices = [e.vertices[0], points[0].1];
            splits.insert(
                e.id,
                Split {
                    ends: e.vertices,
                    points,
                },
            );
        }
    }
    for (fi, face) in mesh.data().faces.iter().enumerate() {
        let mut corners = Vec::new();
        for (a, b) in face
            .corners
            .iter()
            .zip(face.corners.iter().cycle().skip(1))
            .take(face.corners.len())
        {
            corners.push(a.clone());
            let edge = mesh.prepared().edge_pairs[&pair([a.vertex, b.vertex])];
            let id = mesh.data().edges[edge].id;
            if let Some(split) = splits.get(&id) {
                let forward = split.ends[0] == a.vertex;
                let ordered: Box<dyn Iterator<Item = &(f32, Id)>> = if forward {
                    Box::new(split.points.iter())
                } else {
                    Box::new(split.points.iter().rev())
                };
                for &(factor, vertex) in ordered {
                    let t = if forward { factor } else { 1. - factor };
                    corners.push(Corner {
                        vertex,
                        uv: Vec2::from(a.uv).lerp(Vec2::from(b.uv), t).to_array(),
                        normal: a.normal.zip(b.normal).map(|(a, b)| {
                            Vec3::from(a)
                                .lerp(Vec3::from(b), t)
                                .normalize_or_zero()
                                .to_array()
                        }),
                    });
                }
            }
        }
        data.faces[fi].corners = corners;
    }
    Ok((data, splits))
}
fn resolve(mesh: &EditableMesh, splits: &HashMap<Id, Split>, p: Point) -> Result<Id, String> {
    let edge = mesh.edge(p.edge).ok_or("Aresta ausente.")?;
    if p.factor <= 1e-6 {
        return Ok(edge.vertices[0]);
    }
    if p.factor >= 1. - 1e-6 {
        return Ok(edge.vertices[1]);
    }
    splits
        .get(&p.edge)
        .and_then(|s| s.points.iter().find(|&&(t, _)| (t - p.factor).abs() < 1e-6))
        .map(|&(_, id)| id)
        .ok_or_else(|| "Ponto de divisão ausente.".into())
}
fn partition(data: &mut MeshData, index: usize, a: Id, b: Id) -> Result<usize, String> {
    let corners = &data.faces[index].corners;
    let ai = corners
        .iter()
        .position(|c| c.vertex == a)
        .ok_or("O corte precisa começar na borda da face.")?;
    let bi = corners
        .iter()
        .position(|c| c.vertex == b)
        .ok_or("O corte precisa terminar na borda da mesma face.")?;
    let (lo, hi) = (ai.min(bi), ai.max(bi));
    if hi - lo < 2 || hi - lo >= corners.len() - 1 {
        return Err("O trecho coincide com a borda ou não divide a face.".into());
    }
    let left = corners[lo..=hi].to_vec();
    let right = corners[hi..]
        .iter()
        .chain(corners[..=lo].iter())
        .cloned()
        .collect();
    data.faces[index].corners = left;
    let next = data.faces.len();
    data.add_face(right)?;
    Ok(next)
}
fn finish(mut data: MeshData, edges: &[[Id; 2]]) -> Result<Output, String> {
    data.complete_edges()?;
    let mesh = EditableMesh::new(data)?;
    let ids = edges
        .iter()
        .filter_map(|e| mesh.prepared().edge_pairs.get(&pair(*e)))
        .map(|&i| mesh.data().edges[i].id)
        .collect();
    Ok(Output {
        mesh,
        selection: Selection {
            mode: Mode::Edge,
            ids,
            through: false,
        },
        new_faces: vec![],
    })
}
pub fn knife(mesh: &EditableMesh, segments: &[Segment]) -> Result<Cut, String> {
    if segments.is_empty() || segments.len() > 256 {
        return Err(
            "O bisturi aceita um percurso de 1 a 256 trechos, entre bordas de faces adjacentes."
                .into(),
        );
    }
    let mut faces = HashSet::new();
    for segment in segments {
        if !faces.insert(segment.face) {
            return Err("Nesta operação, atravesse cada face uma única vez; confirme antes de fazer outro corte na mesma face.".into());
        }
        let f = mesh
            .prepared()
            .faces
            .get(&segment.face)
            .ok_or("Face não encontrada.")?;
        for point in [segment.from, segment.to] {
            let edge = *mesh
                .prepared()
                .edges
                .get(&point.edge)
                .ok_or("Aresta de corte ausente.")?;
            if !mesh.prepared().incident_faces[edge]
                .iter()
                .any(|&(i, _)| i == *f)
            {
                return Err(
                    "O trecho sairia da superfície. Escolha a borda da face apontada.".into(),
                );
            }
        }
    }
    let points = segments
        .iter()
        .flat_map(|s| [s.from, s.to])
        .collect::<Vec<_>>();
    let (mut data, splits) = subdivide(mesh, &points)?;
    let mut previous = None;
    let mut cuts = Vec::new();
    for segment in segments {
        let a = resolve(mesh, &splits, segment.from)?;
        let b = resolve(mesh, &splits, segment.to)?;
        if let Some((last, last_face)) = previous {
            let fi = mesh.prepared().faces[&segment.face];
            let adjacent = mesh.prepared().incident_faces.iter().any(|uses| {
                uses.iter()
                    .any(|&(f, _)| mesh.data().faces[f].id == last_face)
                    && uses.iter().any(|&(f, _)| f == fi)
            });
            if last != a || !adjacent {
                return Err("O traçado precisa continuar no último ponto, atravessando faces adjacentes; não faça pontes pelo vazio.".into());
            }
        }
        partition(&mut data, mesh.prepared().faces[&segment.face], a, b)?;
        cuts.push([a, b]);
        previous = Some((b, segment.face));
    }
    let output = finish(data, &cuts)?;
    // Independent area conservation catches chords outside concave polygons.
    let area = |m: &EditableMesh| {
        m.prepared()
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = m.triangle_points(t);
                (b - a).cross(c - a).length() * 0.5
            })
            .sum::<f32>()
    };
    let old = area(mesh);
    let new = area(&output.mesh);
    if (old - new).abs() > 1e-5 * (1. + old) {
        return Err("O traçado atravessaria a região externa de uma face côncava. Use um caminho contido na superfície.".into());
    }
    Ok(Cut {
        output,
        notes: vec![],
    })
}
pub fn loop_cut(mesh: &EditableMesh, start: Id, count: u32, slide: f32) -> Result<Cut, String> {
    if !(1..=64).contains(&count) || !slide.is_finite() || !(-1. ..=1.).contains(&slide) {
        return Err("Use de 1 a 64 cortes e deslizamento entre -1 e 1.".into());
    }
    let edge = mesh
        .edge(start)
        .ok_or("Aponte ou selecione a aresta inicial do corte.")?;
    let mut oriented = HashMap::from([(start, edge.vertices)]);
    let mut queue = VecDeque::from([start]);
    let mut faces = HashMap::<usize, (Id, Id)>::new();
    let mut order = Vec::new();
    let mut stops = HashSet::new();
    while let Some(id) = queue.pop_front() {
        order.push(id);
        let ei = mesh.prepared().edges[&id];
        let direction = oriented[&id];
        for &(fi, ci) in &mesh.prepared().incident_faces[ei] {
            let face = &mesh.data().faces[fi];
            if face.corners.len() != 4 {
                stops.insert(face.id);
                continue;
            }
            let mut ends = [
                face.corners[(ci + 3) % 4].vertex,
                face.corners[(ci + 2) % 4].vertex,
            ];
            if face.corners[ci].vertex != direction[0] {
                ends.reverse();
            }
            let opposite = mesh.data().edges[mesh.prepared().edge_pairs[&pair(ends)]].id;
            if let Some(&(a, b)) = faces.get(&fi) {
                if !([a, b].contains(&id) && [a, b].contains(&opposite)) {
                    return Err("O percurso se cruza num polo ambíguo. Escolha outra faixa de quadriláteros.".into());
                }
            } else {
                faces.insert(fi, (id, opposite));
            }
            if let Some(old) = oriented.get(&opposite) {
                if *old != ends {
                    return Err("A faixa se torce e inverte o percurso. Faça cortes separados nessa região.".into());
                }
            } else {
                oriented.insert(opposite, ends);
                queue.push_back(opposite);
            }
        }
    }
    if faces.is_empty() {
        return Err(
            "O corte em loop precisa começar numa faixa de faces com quatro cantos.".into(),
        );
    }
    let mut points = Vec::new();
    let mut on_edge = HashMap::<Id, Vec<Point>>::new();
    for &id in &order {
        let forward = oriented[&id] == mesh.edge(id).unwrap().vertices;
        let list = (0..count)
            .map(|i| {
                let t = (i + 1) as f32 / (count + 1) as f32 + slide * 0.95 / (count + 1) as f32;
                Point {
                    edge: id,
                    factor: if forward { t } else { 1. - t },
                }
            })
            .collect::<Vec<_>>();
        points.extend(&list);
        on_edge.insert(id, list);
    }
    let (mut data, splits) = subdivide(mesh, &points)?;
    let mut cuts = Vec::new();
    for fi in 0..mesh.data().faces.len() {
        if let Some(&(left, right)) = faces.get(&fi) {
            let mut remaining = fi;
            for i in 0..count as usize {
                let a = resolve(mesh, &splits, on_edge[&left][i])?;
                let b = resolve(mesh, &splits, on_edge[&right][i])?;
                let next = partition(&mut data, remaining, a, b)?;
                cuts.push([a, b]);
                if i + 1 < count as usize {
                    let next_a = resolve(mesh, &splits, on_edge[&left][i + 1])?;
                    if data.faces[next].corners.iter().any(|c| c.vertex == next_a) {
                        remaining = next;
                    }
                }
            }
        }
    }
    let output = finish(data, &cuts)?;
    Ok(Cut {
        output,
        notes: if stops.is_empty() {
            vec![]
        } else {
            vec![format!(
                "A faixa terminou em {} face(s) triangular(es) ou poligonal(is). As bordas compartilhadas foram divididas para manter a superfície conectada.",
                stops.len()
            )]
        },
    })
}
