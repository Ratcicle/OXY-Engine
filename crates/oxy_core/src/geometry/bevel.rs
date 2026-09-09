//! Convex exposed edge/corner bevels via shared half-space clipping. Circular edge profiles;
//! intersections of neighboring profiles form closed miters instead of disconnected strips.
use super::{
    ComponentId as Id, Corner, EditableMesh, Face, MeshData,
    operations::Output,
    pair,
    selection::{Mode, Selection},
};
use glam::{Vec2, Vec3};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    distance: f32,
}
fn relative_scale(mesh: &EditableMesh) -> f32 {
    mesh.prepared()
        .bounds
        .map_or(1., |(a, b)| (b - a).length())
        .max(1e-5)
}
fn face_normal(mesh: &EditableMesh, fi: usize) -> Vec3 {
    mesh.prepared().face_normals[fi]
}
fn plane_through(a: Vec3, b: Vec3, c: Vec3, outward: Vec3) -> Result<Plane, String> {
    let mut normal = (b - a)
        .cross(c - a)
        .try_normalize()
        .ok_or("Perfil degenerado.")?;
    if normal.dot(outward) < 0. {
        normal = -normal;
    }
    Ok(Plane {
        normal,
        distance: normal.dot(a),
    })
}
pub fn apply(
    mesh: &EditableMesh,
    selection: &Selection,
    width: f32,
    segments: u32,
) -> Result<Output, String> {
    if !matches!(selection.mode, Mode::Edge | Mode::Vertex) || selection.ids.is_empty() {
        return Err("Arredondar aceita arestas ou vértices convexos selecionados.".into());
    }
    if !(1..=16).contains(&segments) || !width.is_finite() || width <= 0. {
        return Err("Use largura positiva e de 1 a 16 segmentos.".into());
    }
    if selection.ids.len() > 64 {
        return Err(
            "Arredonde até 64 componentes por operação para manter a prévia responsiva.".into(),
        );
    }
    if mesh.prepared().incident_faces.iter().any(|f| f.len() != 2) {
        return Err("Arredondar requer uma superfície fechada nesta versão. Feche as bordas e separe componentes soltos antes de continuar.".into());
    }
    let scale = relative_scale(mesh);
    let tolerance = scale * 1e-5;
    let mut planes = Vec::new();
    for &id in &selection.ids {
        let (created, corner_ids) = if selection.mode == Mode::Edge {
            let ei = *mesh.prepared().edges.get(&id).ok_or("Aresta ausente.")?;
            let edge = &mesh.data().edges[ei];
            let a = mesh.position(edge.vertices[0]).unwrap();
            let b = mesh.position(edge.vertices[1]).unwrap();
            let uses = &mesh.prepared().incident_faces[ei];
            let n0 = face_normal(mesh, uses[0].0);
            let n1 = face_normal(mesh, uses[1].0);
            let dot = n0.dot(n1).clamp(-1., 1.);
            let angle = dot.acos();
            if !(0.001..=std::f32::consts::PI - 0.001).contains(&angle) {
                return Err("Esta aresta é plana ou degenerada; escolha uma quina convexa.".into());
            }
            // Exposure proof: these clipping planes cannot remove unrelated geometry on another lobe.
            if mesh.data().vertices.iter().any(|v| {
                let p = Vec3::from(v.position) - a;
                n0.dot(p) > tolerance || n1.dot(p) > tolerance
            }) {
                return Err("A quina é côncava ou encoberta por outra região da mesma malha. Este arredondamento aceita quinas convexas expostas.".into());
            }
            let radius = width / (angle * 0.5).tan();
            let center = a - (n0 + n1) * radius / (1. + dot);
            let tangent = (n1 - n0 * dot).normalize();
            let created = (0..segments)
                .map(|i| {
                    let theta = (i as f32 + 0.5) * angle / segments as f32;
                    let normal = n0 * theta.cos() + tangent * theta.sin();
                    Plane {
                        normal,
                        distance: normal.dot(center)
                            + radius * (angle / (2. * segments as f32)).cos(),
                    }
                })
                .collect::<Vec<_>>();
            let _ = b;
            (created, edge.vertices.to_vec())
        } else {
            let p = mesh.position(id).ok_or("Vértice ausente.")?;
            let neighbors = mesh
                .data()
                .edges
                .iter()
                .filter_map(|e| {
                    if e.vertices[0] == id {
                        Some(e.vertices[1])
                    } else if e.vertices[1] == id {
                        Some(e.vertices[0])
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if neighbors.len() != 3 {
                return Err("Arredondar vértice aceita cantos convexos de três arestas, como cubos e prismas. Esta valência precisa de uma operação diferente.".into());
            }
            let ends = neighbors
                .iter()
                .map(|v| p + (mesh.position(*v).unwrap() - p).normalize() * width)
                .collect::<Vec<_>>();
            let [a, b, c] = [ends[0], ends[1], ends[2]];
            let outward = (p - (a + b + c) / 3.).normalize_or_zero();
            let base = plane_through(a, b, c, outward)?;
            let u = b - a;
            let v = c - a;
            let cross = u.cross(v);
            let circumcenter = a
                + (v.cross(cross) * u.length_squared() + cross.cross(u) * v.length_squared())
                    / (2. * cross.length_squared());
            let cr = circumcenter.distance(a);
            let rise = (base.normal.dot(p) - base.distance) * 0.5;
            if !rise.is_finite() || rise < tolerance || cr < rise {
                return Err("O canto não possui um perfil convexo estável.".into());
            }
            let depth = (cr * cr - rise * rise) / (2. * rise);
            let center = circumcenter - base.normal * depth;
            let radius = (cr * cr + depth * depth).sqrt();
            let sample = |i: u32, j: u32| {
                let s = segments as f32;
                let q = a + (b - a) * (i as f32 / s) + (c - a) * (j as f32 / s);
                center + (q - center).normalize() * radius
            };
            let mut created = Vec::new();
            for i in 0..segments {
                for j in 0..segments - i {
                    created.push(plane_through(
                        sample(i, j),
                        sample(i + 1, j),
                        sample(i, j + 1),
                        outward,
                    )?);
                    if i + j + 1 < segments {
                        created.push(plane_through(
                            sample(i + 1, j),
                            sample(i + 1, j + 1),
                            sample(i, j + 1),
                            outward,
                        )?);
                    }
                }
            }
            (created, vec![id])
        };
        // A strict neighborhood bound prevents opposite bevels consuming the intervening face.
        let nearest = mesh
            .data()
            .edges
            .iter()
            .filter(|e| e.vertices.iter().any(|v| corner_ids.contains(v)))
            .map(|e| {
                mesh.position(e.vertices[0])
                    .unwrap()
                    .distance(mesh.position(e.vertices[1]).unwrap())
            })
            .fold(f32::INFINITY, f32::min);
        if width >= nearest * 0.45 {
            return Err(format!(
                "Largura excessiva: use menos de {:.4} unidade(s) nesta seleção para evitar sobreposição.",
                nearest * 0.45
            ));
        }
        for plane in &created {
            if mesh.data().vertices.iter().any(|v| {
                !corner_ids.contains(&v.id)
                    && plane.normal.dot(Vec3::from(v.position)) - plane.distance > tolerance
            }) {
                return Err("O perfil alcançaria outro vértice/região. Reduza a largura ou selecione uma quina convexa isolável.".into());
            }
        }
        planes.extend(created);
    }
    if planes
        .len()
        .saturating_mul(mesh.data().faces.len() + planes.len())
        > 4_000_000
    {
        return Err("Esta prévia excede o limite de trabalho. Reduza a seleção ou os segmentos antes de arredondar.".into());
    }
    let mut data = mesh.data().clone();
    let old_faces = mesh
        .data()
        .faces
        .iter()
        .map(|f| f.id)
        .collect::<HashSet<_>>();
    for plane in planes {
        clip(&mut data, plane, tolerance)?;
    }
    let used = data
        .faces
        .iter()
        .flat_map(|f| f.corners.iter().map(|c| c.vertex))
        .collect::<HashSet<_>>();
    data.vertices.retain(|v| used.contains(&v.id));
    let used_edges = data
        .faces
        .iter()
        .flat_map(|f| {
            f.corners
                .iter()
                .zip(f.corners.iter().cycle().skip(1))
                .take(f.corners.len())
                .map(|(a, b)| pair([a.vertex, b.vertex]))
        })
        .collect::<HashSet<_>>();
    data.edges
        .retain(|e| used_edges.contains(&pair(e.vertices)));
    data.complete_edges()?;
    let mesh = EditableMesh::new(data)?;
    if mesh.prepared().incident_faces.iter().any(|f| f.len() != 2) {
        return Err(
            "A seleção não produziu uma junção fechada; a malha original foi preservada.".into(),
        );
    }
    let new_faces = mesh
        .data()
        .faces
        .iter()
        .filter(|f| !old_faces.contains(&f.id))
        .map(|f| f.id)
        .collect::<Vec<_>>();
    Ok(Output {
        mesh,
        selection: Selection {
            mode: Mode::Face,
            ids: new_faces.clone(),
            through: selection.through,
        },
        new_faces,
    })
}
fn clip(data: &mut MeshData, plane: Plane, tolerance: f32) -> Result<(), String> {
    let mut positions = data
        .vertices
        .iter()
        .map(|v| (v.id, Vec3::from(v.position)))
        .collect::<HashMap<_, _>>();
    let distance = |p: Vec3| plane.normal.dot(p) - plane.distance;
    if !positions.values().any(|&p| distance(p) > tolerance) {
        return Ok(());
    }
    let mut intersections = HashMap::new();
    let mut faces = Vec::new();
    for face in std::mem::take(&mut data.faces) {
        let mut out = Vec::new();
        for (a, b) in face
            .corners
            .iter()
            .zip(face.corners.iter().cycle().skip(1))
            .take(face.corners.len())
        {
            let da = distance(positions[&a.vertex]);
            let db = distance(positions[&b.vertex]);
            let ia = da <= tolerance;
            let ib = db <= tolerance;
            if ia && out.last().is_none_or(|c: &Corner| c.vertex != a.vertex) {
                out.push(a.clone());
            }
            if ia != ib {
                let t = if da.abs() <= tolerance {
                    0.
                } else if db.abs() <= tolerance {
                    1.
                } else {
                    (da / (da - db)).clamp(0., 1.)
                };
                let vertex = if da.abs() <= tolerance {
                    a.vertex
                } else if db.abs() <= tolerance {
                    b.vertex
                } else if let Some(&id) = intersections.get(&pair([a.vertex, b.vertex])) {
                    id
                } else {
                    let p = positions[&a.vertex].lerp(positions[&b.vertex], t);
                    let id = data.add_vertex(p)?;
                    positions.insert(id, p);
                    intersections.insert(pair([a.vertex, b.vertex]), id);
                    id
                };
                let corner = Corner {
                    vertex,
                    uv: Vec2::from(a.uv).lerp(Vec2::from(b.uv), t).to_array(),
                    normal: a.normal.zip(b.normal).map(|(a, b)| {
                        Vec3::from(a)
                            .lerp(Vec3::from(b), t)
                            .normalize_or_zero()
                            .to_array()
                    }),
                };
                if out.last().is_none_or(|c: &Corner| c.vertex != vertex) {
                    out.push(corner);
                }
            }
        }
        if out
            .first()
            .zip(out.last())
            .is_some_and(|(a, b)| a.vertex == b.vertex)
        {
            out.pop();
        }
        if out.len() >= 3 {
            faces.push(Face {
                id: face.id,
                corners: out,
            });
        }
    }
    // Find the actual open contour. Never sort disconnected cuts into an invented cap.
    let mut uses = HashMap::<[Id; 2], (usize, [Id; 2])>::new();
    for f in &faces {
        for (a, b) in f
            .corners
            .iter()
            .zip(f.corners.iter().cycle().skip(1))
            .take(f.corners.len())
        {
            if distance(positions[&a.vertex]).abs() <= tolerance * 2.
                && distance(positions[&b.vertex]).abs() <= tolerance * 2.
            {
                let v = uses
                    .entry(pair([a.vertex, b.vertex]))
                    .or_insert((0, [a.vertex, b.vertex]));
                v.0 += 1;
            }
        }
    }
    let mut border = uses
        .into_iter()
        .filter_map(|(_, (count, edge))| (count == 1).then_some(edge))
        .collect::<Vec<_>>();
    border.sort();
    if border.len() < 3 {
        return Err(
            "O perfil não formou uma borda fechada estável. Reduza os segmentos ou a largura."
                .into(),
        );
    }
    let first = border[0];
    let mut next = HashMap::new();
    for [a, b] in &border {
        if next.insert(*b, *a).is_some() {
            return Err("Junção ambígua neste perfil.".into());
        }
    }
    let mut ids = vec![first[1]];
    let mut current = first[0];
    while current != ids[0] {
        if ids.len() >= border.len() {
            return Err("Contorno interrompido no arredondamento.".into());
        }
        ids.push(current);
        current = *next
            .get(&current)
            .ok_or("Contorno aberto no arredondamento.")?;
    }
    if ids.len() != border.len() {
        return Err("O perfil cruzaria aberturas ou regiões desconectadas. Reduza a largura ou separe a operação.".into());
    }
    data.faces = faces;
    data.add_face(
        ids.iter()
            .enumerate()
            .map(|(i, &vertex)| Corner {
                vertex,
                uv: [i as f32 / ids.len() as f32, 0.],
                normal: None,
            })
            .collect(),
    )?;
    let used = data
        .faces
        .iter()
        .flat_map(|f| f.corners.iter().map(|c| c.vertex))
        .collect::<HashSet<_>>();
    data.vertices.retain(|v| used.contains(&v.id));
    Ok(())
}
