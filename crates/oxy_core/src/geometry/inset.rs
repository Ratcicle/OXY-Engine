//! Linear, area-centroid inset of convex planar surfaces. The original triangulated UV
//! mapping is partitioned (never globally reprojected); only extrusion walls need atlas space.
use super::{
    ComponentId as Id, Corner, EditableMesh, Face, MeshData,
    operations::{self, Output},
    pair,
    selection::{Mode, Selection},
};
use glam::{DVec3, Mat4, Vec2, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

const WORK_LIMIT: usize = 4_000_000;

/// Convert an outward normal into an authored-local displacement. Global distance is
/// measured after the complete parent transform; reflections do not flip outward normals.
pub fn normal_delta(
    normal: Vec3,
    world: Mat4,
    distance: f32,
    global: bool,
) -> Result<Vec3, String> {
    if !normal.is_finite() || !distance.is_finite() || normal.length_squared() < 1e-12 {
        return Err("Normal ou distância inválida para extrudir.".into());
    }
    if !global {
        return Ok(normal.normalize() * distance);
    }
    let inverse = world.inverse();
    if !world.is_finite() || !inverse.is_finite() || world.determinant().abs() < 1e-10 {
        return Err("A transformação do objeto não é invertível; a extrusão foi recusada.".into());
    }
    let world_normal = inverse.transpose().transform_vector3(normal).normalize();
    Ok(inverse.transform_vector3(world_normal * distance))
}

pub fn apply(
    mesh: &EditableMesh,
    selection: &Selection,
    percentage: f32,
    delta: Vec3,
    per_face: bool,
) -> Result<Output, String> {
    let deltas = selection.ids.iter().map(|&id| (id, delta)).collect();
    apply_directions(mesh, selection, percentage, &deltas, per_face)
}

/// `deltas` are already expressed in authored local coordinates; all faces of a united
/// connected region must use the same displacement. Per-face normals can differ.
pub fn apply_directions(
    mesh: &EditableMesh,
    selection: &Selection,
    percentage: f32,
    deltas: &HashMap<Id, Vec3>,
    per_face: bool,
) -> Result<Output, String> {
    if selection.mode != Mode::Face || selection.ids.is_empty() {
        return Err("Criar borda interna requer uma ou mais faces selecionadas.".into());
    }
    let selected: HashSet<_> = selection.ids.iter().copied().collect();
    if selected.len() != selection.ids.len() || selected.iter().any(|id| mesh.face(*id).is_none()) {
        return Err("A seleção contém faces ausentes ou repetidas.".into());
    }
    if !percentage.is_finite() || percentage <= 0. || percentage > 100. {
        return Err(
            "Tamanho interno deve ser maior que 0% e no máximo 100% (proporção linear).".into(),
        );
    }
    if selected
        .iter()
        .any(|id| deltas.get(id).is_none_or(|d| !d.is_finite()))
    {
        return Err("Cada face precisa de um deslocamento finito na direção escolhida.".into());
    }
    if percentage == 100. {
        return full_extrusion(mesh, selection, deltas, per_face);
    }
    if selected.len() > 1024 {
        return Err("Crie a borda em até 1.024 faces por operação para limitar o trabalho de divisão dos UVs.".into());
    }
    let groups = groups(mesh, &selected, per_face);
    let mut data = mesh.data().clone();
    let mut all_pieces = Vec::new();
    let mut pools = Vec::new();
    let mut sweeps = Vec::new();
    let mut work = 0usize;
    for (region, faces) in groups.iter().enumerate() {
        let surface = Surface::new(mesh, faces, percentage / 100.)?;
        let delta = deltas[&mesh.data().faces[faces[0]].id];
        if faces
            .iter()
            .any(|&fi| !deltas[&mesh.data().faces[fi].id].abs_diff_eq(delta, 1e-7))
        {
            return Err(
                "Região unida usa uma direção comum; escolha Cada face para normais individuais."
                    .into(),
            );
        }
        if delta.length() > 0. && surface.normal.dot(delta).abs() < surface.tolerance {
            return Err("Esta direção é paralela à face e produziria paredes sem volume. Escolha a normal ou outro eixo.".into());
        }
        let source_ids: HashSet<_> = faces
            .iter()
            .flat_map(|&i| mesh.data().faces[i].corners.iter().map(|c| c.vertex))
            .collect();
        let mut pool = Pool {
            points: mesh
                .data()
                .vertices
                .iter()
                .filter(|v| source_ids.contains(&v.id))
                .map(|v| (v.id, Vec3::from(v.position).as_dvec3()))
                .collect(),
            tolerance: surface.tolerance,
        };
        let mut pieces = Vec::new();
        let source_faces: HashSet<_> = faces.iter().map(|&fi| mesh.data().faces[fi].id).collect();
        for triangle in &mesh.prepared().triangles {
            if !source_faces.contains(&triangle.face) {
                continue;
            }
            let face = mesh.face(triangle.face).unwrap();
            let mut inside = triangle
                .corners
                .iter()
                .map(|&i| Sample {
                    position: mesh.position(face.corners[i].vertex).unwrap().as_dvec3(),
                    uv: Vec2::from(face.corners[i].uv),
                    normal: face.corners[i].normal.map(Vec3::from),
                })
                .collect::<Vec<_>>();
            for plane in &surface.planes {
                charge(&mut work, inside.len())?;
                let (remain, outside) = split(&inside, *plane, surface.tolerance);
                if has_area(&outside, surface.tolerance) {
                    pieces.push(Piece::new(
                        &mut data,
                        &mut pool,
                        outside,
                        triangle.face,
                        region,
                        false,
                        &mut work,
                    )?);
                }
                inside = remain;
                if inside.len() < 3 {
                    break;
                }
            }
            if has_area(&inside, surface.tolerance) {
                pieces.push(Piece::new(
                    &mut data,
                    &mut pool,
                    inside,
                    triangle.face,
                    region,
                    true,
                    &mut work,
                )?);
            }
        }
        if !pieces.iter().any(|p| p.cap) {
            return Err(
                "O tamanho interno colapsou a região abaixo da tolerância geométrica.".into(),
            );
        }
        // The complement is split sequentially by clipping planes. Insert all shared cut
        // points into its edges so an earlier strip cannot retain a T-junction.
        for piece in &mut pieces {
            densify(&mut piece.corners, &pool.points, pool.tolerance, &mut work)?;
        }
        for piece in pieces.iter().filter(|p| p.cap) {
            if delta.length_squared() > 0. {
                let points: Vec<_> = piece
                    .corners
                    .iter()
                    .map(|c| pool.position(c.vertex))
                    .collect();
                for indices in super::triangulate::polygon(&points)?.1 {
                    sweeps.push(Sweep::new(indices.map(|i| points[i]), delta, region)?);
                }
            }
        }
        all_pieces.extend(pieces);
        pools.push((pool, delta));
    }
    conform_original_edges(mesh, &groups, &mut all_pieces, &mut pools, &mut work)?;
    validate_sweeps(mesh, &selected, &sweeps, &mut work)?;

    let mut top_copies = vec![HashMap::<Id, Id>::new(); pools.len()];
    let mut wall_edges = vec![HashMap::<[Id; 2], (usize, [Id; 2])>::new(); pools.len()];
    for piece in all_pieces.iter().filter(|p| p.cap) {
        for (a, b) in cycle(&piece.corners) {
            let entry = wall_edges[piece.region]
                .entry(pair([a.vertex, b.vertex]))
                .or_insert((0, [a.vertex, b.vertex]));
            entry.0 += 1;
        }
    }
    for piece in all_pieces.iter_mut().filter(|p| p.cap) {
        let (pool, delta) = &pools[piece.region];
        if delta.length_squared() == 0. {
            continue;
        }
        for corner in &mut piece.corners {
            let id = if let Some(&id) = top_copies[piece.region].get(&corner.vertex) {
                id
            } else {
                let id = data.add_vertex(pool.position(corner.vertex) + *delta)?;
                top_copies[piece.region].insert(corner.vertex, id);
                id
            };
            corner.vertex = id;
        }
    }

    let mut by_face = HashMap::<Id, Vec<Piece>>::new();
    for piece in all_pieces {
        by_face.entry(piece.source).or_default().push(piece);
    }
    let mut output_faces = Vec::new();
    let mut cap_ids = Vec::new();
    let all_points: Vec<_> = pools
        .iter()
        .flat_map(|(p, _)| p.points.iter().copied())
        .collect();
    let tolerance = pools
        .iter()
        .map(|(p, _)| p.tolerance)
        .fold(f32::INFINITY, f32::min);
    let touched_edges: HashSet<_> = mesh
        .data()
        .faces
        .iter()
        .filter(|f| selected.contains(&f.id))
        .flat_map(|f| cycle(&f.corners).map(|(a, b)| pair([a.vertex, b.vertex])))
        .collect();
    for original in &mesh.data().faces {
        if let Some(mut pieces) = by_face.remove(&original.id) {
            // Preserve the face ID on a selected cap when possible. Extra UV partitions
            // get stable monotonic IDs; callers select every cap, never its frame/walls.
            if let Some(i) = pieces.iter().position(|p| p.cap) {
                pieces.swap(0, i);
            }
            for (i, piece) in pieces.into_iter().enumerate() {
                let id = if i == 0 {
                    original.id
                } else {
                    data.allocate_id()?
                };
                if piece.cap {
                    cap_ids.push(id);
                }
                output_faces.push(Face {
                    id,
                    corners: piece.corners,
                });
            }
        } else {
            let mut face = original.clone();
            densify_selected_edges(
                &mut face.corners,
                mesh,
                &all_points,
                tolerance,
                &touched_edges,
                &mut work,
            )?;
            output_faces.push(face);
        }
    }
    data.faces = output_faces;
    let mut new_faces = Vec::new();
    for (region, edges) in wall_edges.into_iter().enumerate() {
        if pools[region].1.length_squared() == 0. {
            continue;
        }
        let mut boundary: Vec<_> = edges
            .into_iter()
            .filter_map(|(_, (count, edge))| (count == 1).then_some(edge))
            .collect();
        boundary.sort_unstable();
        for [a, b] in boundary {
            let ids = [a, b, top_copies[region][&b], top_copies[region][&a]];
            new_faces.push(
                data.add_face(
                    ids.iter()
                        .enumerate()
                        .map(|(i, &vertex)| Corner {
                            vertex,
                            uv: [[0., 0.], [1., 0.], [1., 1.], [0., 1.]][i],
                            normal: None,
                        })
                        .collect(),
                )?,
            );
        }
    }
    cleanup(mesh, &mut data);
    data.complete_edges()?;
    let mesh = EditableMesh::new(data)?;
    Ok(Output {
        mesh,
        selection: Selection {
            mode: Mode::Face,
            ids: cap_ids,
            through: selection.through,
        },
        new_faces,
    })
}

fn charge(work: &mut usize, amount: usize) -> Result<(), String> {
    *work = work.saturating_add(amount);
    if *work > WORK_LIMIT {
        Err("A divisão de UVs ou verificação de interseções excedeu o limite de trabalho. Reduza a seleção; a fonte foi preservada.".into())
    } else {
        Ok(())
    }
}

fn full_extrusion(
    mesh: &EditableMesh,
    selection: &Selection,
    deltas: &HashMap<Id, Vec3>,
    per_face: bool,
) -> Result<Output, String> {
    let delta = deltas[&selection.ids[0]];
    let same = selection.ids.iter().all(|id| deltas[id] == delta);
    if !per_face && !same {
        return Err("Região unida usa uma direção comum.".into());
    }
    if deltas.values().all(|d| d.length_squared() == 0.) {
        return Ok(Output {
            mesh: mesh.clone(),
            selection: selection.clone(),
            new_faces: vec![],
        });
    }
    let output = if same {
        operations::extrude(mesh, selection, delta, per_face)?
    } else {
        let mut current = mesh.clone();
        let mut new_faces = Vec::new();
        for &id in &selection.ids {
            if deltas[&id].length_squared() == 0. {
                continue;
            }
            let result = operations::extrude(
                &current,
                &Selection {
                    mode: Mode::Face,
                    ids: vec![id],
                    through: selection.through,
                },
                deltas[&id],
                true,
            )?;
            current = result.mesh;
            new_faces.extend(result.new_faces);
        }
        Output {
            mesh: current,
            selection: selection.clone(),
            new_faces,
        }
    };
    let selected: HashSet<_> = selection.ids.iter().copied().collect();
    let mut sweeps = Vec::new();
    for triangle in &mesh.prepared().triangles {
        if !selected.contains(&triangle.face) {
            continue;
        }
        let delta = deltas[&triangle.face];
        let points = mesh.triangle_points(triangle);
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize();
        if normal.dot(delta).abs() < 1e-7 {
            continue;
        }
        sweeps.push(Sweep::new(
            points,
            delta,
            if per_face { triangle.face as usize } else { 0 },
        )?);
    }
    validate_sweeps(mesh, &selected, &sweeps, &mut 0)?;
    Ok(output)
}

fn groups(mesh: &EditableMesh, selected: &HashSet<Id>, per_face: bool) -> Vec<Vec<usize>> {
    let mut visited = HashSet::new();
    let mut groups = Vec::new();
    for (fi, face) in mesh.data().faces.iter().enumerate() {
        if !selected.contains(&face.id) || !visited.insert(fi) {
            continue;
        }
        let mut group = Vec::new();
        let mut pending = VecDeque::from([fi]);
        while let Some(index) = pending.pop_front() {
            group.push(index);
            if per_face {
                continue;
            }
            for (a, b) in cycle(&mesh.data().faces[index].corners) {
                let edge = mesh.prepared().edge_pairs[&pair([a.vertex, b.vertex])];
                for &(next, _) in &mesh.prepared().incident_faces[edge] {
                    if selected.contains(&mesh.data().faces[next].id) && visited.insert(next) {
                        pending.push_back(next);
                    }
                }
            }
        }
        group.sort_unstable();
        groups.push(group);
    }
    groups
}

#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    distance: f32,
}
/// Region-relative double-precision calculations keep intersections shared by
/// adjacent UV triangles identical even when a tiny face is far from the origin.
#[derive(Clone, Copy)]
struct ClipPlane {
    normal: DVec3,
    origin: DVec3,
}
struct Surface {
    normal: Vec3,
    planes: Vec<ClipPlane>,
    tolerance: f32,
}
impl Surface {
    fn new(mesh: &EditableMesh, faces: &[usize], scale: f32) -> Result<Self, String> {
        let first = &mesh.data().faces[faces[0]];
        let origin = mesh.position(first.corners[0].vertex).unwrap();
        let normal = mesh.prepared().face_normals[faces[0]];
        let extent = faces
            .iter()
            .flat_map(|&i| &mesh.data().faces[i].corners)
            .map(|c| mesh.position(c.vertex).unwrap().distance(origin))
            .fold(0f32, f32::max);
        let tolerance = (extent * 1e-6).max(1e-8);
        let fail = || {
            "A borda interna em Região unida requer faces planas, coplanares e um contorno convexo sem buracos. Para outras seleções, use Cada face ou extrusão a 100%.".to_string()
        };
        let mut edges = HashMap::<[Id; 2], (usize, [Id; 2])>::new();
        for &fi in faces {
            let face = &mesh.data().faces[fi];
            if mesh.prepared().face_normals[fi].dot(normal) < 1. - 1e-5 {
                return Err(fail());
            }
            let points: Vec<_> = face
                .corners
                .iter()
                .map(|c| mesh.position(c.vertex).unwrap())
                .collect();
            if points
                .iter()
                .any(|p| (*p - origin).dot(normal).abs() > tolerance * 4.)
                || !convex(&points, normal, tolerance)
            {
                return Err(fail());
            }
            for (a, b) in cycle(&face.corners) {
                let entry = edges
                    .entry(pair([a.vertex, b.vertex]))
                    .or_insert((0, [a.vertex, b.vertex]));
                entry.0 += 1;
            }
        }
        let mut boundary: Vec<_> = edges
            .into_iter()
            .filter_map(|(_, (count, edge))| (count == 1).then_some(edge))
            .collect();
        boundary.sort_unstable();
        if boundary.len() < 3 || boundary.len() > 512 {
            return Err(fail());
        }
        let next: HashMap<_, _> = boundary.iter().map(|e| (e[0], e[1])).collect();
        if next.len() != boundary.len() {
            return Err(fail());
        }
        let mut ids = vec![boundary[0][0]];
        let mut current = boundary[0][1];
        while current != ids[0] {
            if ids.len() >= boundary.len() {
                return Err(fail());
            }
            ids.push(current);
            current = *next.get(&current).ok_or_else(fail)?;
        }
        if ids.len() != boundary.len() {
            return Err(fail());
        }
        let points: Vec<_> = ids.iter().map(|&id| mesh.position(id).unwrap()).collect();
        if !convex(&points, normal, tolerance) {
            return Err(fail());
        }
        let precise: Vec<_> = points.iter().map(|p| p.as_dvec3()).collect();
        let precise_normal = normal.as_dvec3();
        let origin = precise[0];
        let mut weight = 0.;
        let mut center = DVec3::ZERO;
        for i in 1..precise.len() - 1 {
            let area = (precise[i] - origin)
                .cross(precise[i + 1] - origin)
                .dot(precise_normal);
            center += (origin + precise[i] + precise[i + 1]) / 3. * area;
            weight += area;
        }
        if weight <= f64::from(tolerance).powi(2) {
            return Err(fail());
        }
        center /= weight;
        let inner: Vec<_> = precise
            .iter()
            .map(|p| center + (*p - center) * f64::from(scale))
            .collect();
        if inner
            .iter()
            .zip(inner.iter().cycle().skip(1))
            .take(inner.len())
            .any(|(a, b)| a.distance(*b) < f64::from(tolerance) * 4.)
            || (1. - scale) * extent < tolerance * 4.
        {
            return Err("O tamanho interno está perto demais de 0% ou 100% para produzir uma borda não degenerada nesta face.".into());
        }
        let planes = inner
            .iter()
            .zip(inner.iter().cycle().skip(1))
            .take(inner.len())
            .map(|(&a, &b)| {
                let inward = precise_normal.cross(b - a).normalize();
                ClipPlane {
                    normal: inward,
                    origin: a,
                }
            })
            .collect();
        Ok(Self {
            normal,
            planes,
            tolerance,
        })
    }
}
fn convex(points: &[Vec3], normal: Vec3, tolerance: f32) -> bool {
    (0..points.len()).all(|i| {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let c = points[(i + 2) % points.len()];
        (b - a).cross(c - b).dot(normal) >= -tolerance * tolerance
    })
}
fn cycle(corners: &[Corner]) -> impl Iterator<Item = (&Corner, &Corner)> {
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .take(corners.len())
}
#[derive(Clone)]
struct Sample {
    position: DVec3,
    uv: Vec2,
    normal: Option<Vec3>,
}
impl Sample {
    fn lerp(&self, other: &Self, t: f64) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            uv: self.uv.lerp(other.uv, t as f32),
            normal: self
                .normal
                .zip(other.normal)
                .map(|(a, b)| a.lerp(b, t as f32).normalize_or_zero()),
        }
    }
}
fn split(polygon: &[Sample], plane: ClipPlane, tolerance: f32) -> (Vec<Sample>, Vec<Sample>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        let mut da = plane.normal.dot(a.position - plane.origin);
        let mut db = plane.normal.dot(b.position - plane.origin);
        if da.abs() < f64::from(tolerance) {
            da = 0.;
        }
        if db.abs() < f64::from(tolerance) {
            db = 0.;
        }
        if da >= 0. {
            inside.push(a.clone());
        }
        if da <= 0. {
            outside.push(a.clone());
        }
        if da * db < 0. {
            let intersection = a.lerp(b, da / (da - db));
            inside.push(intersection.clone());
            outside.push(intersection);
        }
    }
    (inside, outside)
}
fn has_area(polygon: &[Sample], tolerance: f32) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let origin = polygon[0].position;
    (1..polygon.len() - 1)
        .map(|i| (polygon[i].position - origin).cross(polygon[i + 1].position - origin))
        .sum::<DVec3>()
        .length()
        > f64::from(tolerance).powi(2) * 4.
}
struct Pool {
    points: Vec<(Id, DVec3)>,
    tolerance: f32,
}
impl Pool {
    fn id(&mut self, data: &mut MeshData, p: DVec3, work: &mut usize) -> Result<Id, String> {
        charge(work, self.points.len())?;
        if let Some((id, _)) = self
            .points
            .iter()
            .find(|(_, q)| p.distance(*q) <= f64::from(self.tolerance))
        {
            return Ok(*id);
        }
        let id = data.add_vertex(p.as_vec3())?;
        self.points.push((id, p));
        Ok(id)
    }
    fn position(&self, id: Id) -> Vec3 {
        self.points
            .iter()
            .find(|(v, _)| *v == id)
            .unwrap()
            .1
            .as_vec3()
    }
}
struct Piece {
    source: Id,
    region: usize,
    cap: bool,
    corners: Vec<Corner>,
}
impl Piece {
    fn new(
        data: &mut MeshData,
        pool: &mut Pool,
        polygon: Vec<Sample>,
        source: Id,
        region: usize,
        cap: bool,
        work: &mut usize,
    ) -> Result<Self, String> {
        let mut corners = Vec::new();
        for sample in polygon {
            let vertex = pool.id(data, sample.position, work)?;
            if corners.last().is_none_or(|c: &Corner| c.vertex != vertex) {
                corners.push(Corner {
                    vertex,
                    uv: sample.uv.to_array(),
                    normal: sample.normal.map(|n| n.to_array()),
                });
            }
        }
        if corners
            .first()
            .zip(corners.last())
            .is_some_and(|(a, b)| a.vertex == b.vertex)
        {
            corners.pop();
        }
        Ok(Self {
            source,
            region,
            cap,
            corners,
        })
    }
}
fn intermediate(
    points: &[(Id, DVec3)],
    a: DVec3,
    b: DVec3,
    tolerance: f32,
    work: &mut usize,
) -> Result<Vec<(f64, Id)>, String> {
    charge(work, points.len())?;
    let tolerance = f64::from(tolerance);
    let direction = b - a;
    let length = direction.length_squared();
    let mut result = Vec::new();
    for &(id, p) in points {
        let t = (p - a).dot(direction) / length;
        if t > tolerance / length.sqrt()
            && t < 1. - tolerance / length.sqrt()
            && p.distance(a + direction * t) <= tolerance
        {
            result.push((t, id));
        }
    }
    result.sort_by(|(a, id), (b, jd)| a.total_cmp(b).then(id.cmp(jd)));
    result.dedup_by(|(a, _), (b, _)| (*a - *b).abs() * length.sqrt() <= tolerance);
    Ok(result)
}
fn interpolate_corner(a: &Corner, b: &Corner, t: f64, vertex: Id) -> Corner {
    Corner {
        vertex,
        uv: Vec2::from(a.uv).lerp(Vec2::from(b.uv), t as f32).to_array(),
        normal: a.normal.zip(b.normal).map(|(a, b)| {
            Vec3::from(a)
                .lerp(Vec3::from(b), t as f32)
                .normalize_or_zero()
                .to_array()
        }),
    }
}
fn densify(
    corners: &mut Vec<Corner>,
    points: &[(Id, DVec3)],
    tolerance: f32,
    work: &mut usize,
) -> Result<(), String> {
    let positions: HashMap<_, _> = points.iter().copied().collect();
    let mut result = Vec::new();
    for (a, b) in cycle(corners) {
        result.push(a.clone());
        for (t, vertex) in intermediate(
            points,
            positions[&a.vertex],
            positions[&b.vertex],
            tolerance,
            work,
        )? {
            result.push(interpolate_corner(a, b, t, vertex));
        }
    }
    *corners = result;
    Ok(())
}
fn densify_selected_edges(
    corners: &mut Vec<Corner>,
    mesh: &EditableMesh,
    points: &[(Id, DVec3)],
    tolerance: f32,
    touched: &HashSet<[Id; 2]>,
    work: &mut usize,
) -> Result<(), String> {
    let mut result = Vec::new();
    for (a, b) in cycle(corners) {
        result.push(a.clone());
        if touched.contains(&pair([a.vertex, b.vertex])) {
            for (t, vertex) in intermediate(
                points,
                mesh.position(a.vertex).unwrap().as_dvec3(),
                mesh.position(b.vertex).unwrap().as_dvec3(),
                tolerance,
                work,
            )? {
                result.push(interpolate_corner(a, b, t, vertex));
            }
        }
    }
    *corners = result;
    Ok(())
}
fn cleanup(source: &EditableMesh, data: &mut MeshData) {
    let loose: HashSet<_> = source
        .data()
        .edges
        .iter()
        .enumerate()
        .filter(|(i, _)| source.prepared().incident_faces[*i].is_empty())
        .map(|(_, e)| e.id)
        .collect();
    let pairs: HashSet<_> = data
        .faces
        .iter()
        .flat_map(|f| cycle(&f.corners).map(|(a, b)| pair([a.vertex, b.vertex])))
        .collect();
    data.edges
        .retain(|e| loose.contains(&e.id) || pairs.contains(&pair(e.vertices)));
    let original_surface: HashSet<_> = source
        .data()
        .faces
        .iter()
        .flat_map(|f| f.corners.iter().map(|c| c.vertex))
        .collect();
    let original: HashSet<_> = source.data().vertices.iter().map(|v| v.id).collect();
    let used: HashSet<_> = data
        .faces
        .iter()
        .flat_map(|f| f.corners.iter().map(|c| c.vertex))
        .chain(data.edges.iter().flat_map(|e| e.vertices))
        .collect();
    data.vertices.retain(|v| {
        used.contains(&v.id) || (original.contains(&v.id) && !original_surface.contains(&v.id))
    });
}

/// Independent face insets still share their original boundary. Reconcile only points
/// lying on the same authored edge, never weld unrelated coincident loose components.
fn conform_original_edges(
    mesh: &EditableMesh,
    groups: &[Vec<usize>],
    pieces: &mut [Piece],
    pools: &mut [(Pool, Vec3)],
    work: &mut usize,
) -> Result<(), String> {
    let mut cuts = HashMap::<[Id; 2], Vec<(f64, Id, DVec3)>>::new();
    let mut remap = HashMap::new();
    let mut region_edges = Vec::new();
    for (region, faces) in groups.iter().enumerate() {
        let mut edges: Vec<_> = faces
            .iter()
            .flat_map(|&fi| {
                cycle(&mesh.data().faces[fi].corners).map(|(a, b)| pair([a.vertex, b.vertex]))
            })
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let tolerance = pools[region].0.tolerance;
        for &edge in &edges {
            let a = mesh.position(edge[0]).unwrap().as_dvec3();
            let b = mesh.position(edge[1]).unwrap().as_dvec3();
            let length = a.distance(b);
            for (t, id) in intermediate(&pools[region].0.points, a, b, tolerance, work)? {
                let values = cuts.entry(edge).or_default();
                if let Some((_, other, _)) = values
                    .iter()
                    .find(|(u, _, _)| (*u - t).abs() * length <= f64::from(tolerance))
                {
                    if id != *other {
                        remap.insert(id, *other);
                    }
                } else {
                    values.push((t, id, a.lerp(b, t)));
                }
            }
        }
        region_edges.push(edges);
    }
    let canonical = |mut id: Id| {
        while let Some(&next) = remap.get(&id) {
            if next == id {
                break;
            }
            id = next;
        }
        id
    };
    for piece in pieces.iter_mut() {
        for corner in &mut piece.corners {
            corner.vertex = canonical(corner.vertex);
        }
    }
    for (region, (pool, _)) in pools.iter_mut().enumerate() {
        for (id, _) in &mut pool.points {
            *id = canonical(*id);
        }
        let mut ids: HashSet<_> = pool.points.iter().map(|(id, _)| *id).collect();
        for edge in &region_edges[region] {
            if let Some(points) = cuts.get(edge) {
                for &(_, id, p) in points {
                    let id = canonical(id);
                    if ids.insert(id) {
                        pool.points.push((id, p));
                    }
                }
            }
        }
        pool.points.dedup_by_key(|(id, _)| *id);
    }
    for piece in pieces {
        let pool = &pools[piece.region].0;
        densify(&mut piece.corners, &pool.points, pool.tolerance, work)?;
    }
    Ok(())
}

struct Sweep {
    points: [Vec3; 3],
    delta: Vec3,
    region: usize,
    planes: [Plane; 5],
    min: Vec3,
    max: Vec3,
    tolerance: f32,
}
impl Sweep {
    fn new(points: [Vec3; 3], delta: Vec3, region: usize) -> Result<Self, String> {
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize();
        let outward = normal * normal.dot(delta).signum();
        let center = (points[0] + points[1] + points[2]) / 3. + delta * 0.5;
        let mut planes = [Plane {
            normal: -outward,
            distance: -outward.dot(points[0]),
        }; 5];
        planes[1] = Plane {
            normal: outward,
            distance: outward.dot(points[0] + delta),
        };
        for i in 0..3 {
            let mut n = (points[(i + 1) % 3] - points[i])
                .cross(delta)
                .try_normalize()
                .ok_or("Direção degenerada no contorno interno.")?;
            if n.dot(center - points[i]) > 0. {
                n = -n;
            }
            planes[i + 2] = Plane {
                normal: n,
                distance: n.dot(points[i]),
            };
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in points.iter().flat_map(|&p| [p, p + delta]) {
            min = min.min(p);
            max = max.max(p);
        }
        Ok(Self {
            points,
            delta,
            region,
            planes,
            min,
            max,
            tolerance: ((max - min).length() * 1e-6).max(1e-8),
        })
    }
    fn intersects(&self, triangle: [Vec3; 3]) -> bool {
        let tmin = triangle[0].min(triangle[1]).min(triangle[2]);
        let tmax = triangle[0].max(triangle[1]).max(triangle[2]);
        if !tmin.cmple(self.max).all() || !self.min.cmple(tmax).all() {
            return false;
        }
        let mut polygon = triangle.to_vec();
        for (index, plane) in self.planes.iter().enumerate() {
            let mut clipped = Vec::new();
            for (&a, &b) in polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .take(polygon.len())
            {
                // Side/base contacts are allowed, but an area of the final cap must not
                // finish coincident with another face (e.g. a recess reaching the box floor).
                let padding = if index == 1 {
                    self.tolerance
                } else {
                    -self.tolerance
                };
                let da = plane.distance + padding - plane.normal.dot(a);
                let db = plane.distance + padding - plane.normal.dot(b);
                if da >= 0. {
                    clipped.push(a);
                }
                if da * db < 0. {
                    clipped.push(a.lerp(b, da / (da - db)));
                }
            }
            polygon = clipped;
            if polygon.len() < 3 {
                return false;
            }
        }
        (1..polygon.len() - 1)
            .map(|i| (polygon[i] - polygon[0]).cross(polygon[i + 1] - polygon[0]))
            .sum::<Vec3>()
            .length()
            > self.tolerance * self.tolerance
    }
    fn triangles(&self) -> [[Vec3; 3]; 8] {
        let [a, b, c] = self.points;
        let [d, e, f] = self.points.map(|p| p + self.delta);
        [
            [a, b, c],
            [d, e, f],
            [a, b, e],
            [a, e, d],
            [b, c, f],
            [b, f, e],
            [c, a, d],
            [c, d, f],
        ]
    }
}
fn validate_sweeps(
    mesh: &EditableMesh,
    selected: &HashSet<Id>,
    sweeps: &[Sweep],
    work: &mut usize,
) -> Result<(), String> {
    for sweep in sweeps {
        for triangle in &mesh.prepared().triangles {
            if selected.contains(&triangle.face) {
                continue;
            }
            charge(work, 1)?;
            if sweep.intersects(mesh.triangle_points(triangle)) {
                return Err(format!(
                    "A extrusão atravessaria a face {} da mesma malha. Reduza a distância ou escolha outra direção; a fonte foi preservada.",
                    triangle.face
                ));
            }
        }
    }
    for (i, a) in sweeps.iter().enumerate() {
        for b in &sweeps[i + 1..] {
            if a.region == b.region {
                continue;
            }
            charge(work, 1)?;
            if !a.min.cmple(b.max).all() || !b.min.cmple(a.max).all() {
                continue;
            }
            if b.triangles().into_iter().any(|t| a.intersects(t))
                || a.triangles().into_iter().any(|t| b.intersects(t))
            {
                return Err("As extrusões de regiões distintas se atravessariam. Reduza a distância ou extruda uma face por vez.".into());
            }
        }
    }
    Ok(())
}
