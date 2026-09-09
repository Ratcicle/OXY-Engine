//! Geometric screen selection, independent of gestures and GPU readback.
//! Triangles are clipped in homogeneous coordinates; positive surface area is required.
use crate::{CameraState, mesh};
use egui::{Pos2, Rect};
use glam::{DVec3, DVec4, Mat4, Vec3};
use oxy_core::{
    document::{Id, Scene, SceneKind},
    geometry::{EditableMesh, selection::Mode},
    scene_view::SceneView,
};
use std::collections::{HashMap, HashSet};

/// A quarter logical point gives vertices a predictable boundary tolerance at every DPI.
pub const VERTEX_TOLERANCE: f32 = 0.25;
const AREA_EPSILON: f64 = 0.00001;
// Double precision avoids turning a shared silhouette into a selectable hidden sliver.
const DEPTH_EPSILON: f64 = 0.00000000000001;

fn rect_polygon(rect: Rect) -> [DVec3; 4] {
    [
        DVec3::new(rect.min.x as f64, rect.min.y as f64, 0.),
        DVec3::new(rect.max.x as f64, rect.min.y as f64, 0.),
        DVec3::new(rect.max.x as f64, rect.max.y as f64, 0.),
        DVec3::new(rect.min.x as f64, rect.max.y as f64, 0.),
    ]
}
fn cross(a: DVec3, b: DVec3, c: DVec3) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn area(points: &[DVec3]) -> f64 {
    if points.len() < 3 {
        return 0.;
    }
    let origin = points[0];
    points[1..]
        .windows(2)
        .map(|v| cross(origin, v[0], v[1]))
        .sum::<f64>()
        .abs()
        * 0.5
}
fn bounds(points: &[DVec3]) -> Rect {
    points.iter().fold(Rect::NOTHING, |mut r, p| {
        r.extend_with(Pos2::new(p.x as f32, p.y as f32));
        r
    })
}
fn signed_area(points: &[DVec3]) -> f64 {
    if points.len() < 3 {
        return 0.;
    }
    points[1..]
        .windows(2)
        .map(|v| cross(points[0], v[0], v[1]))
        .sum()
}
fn clip_polygon(points: &[DVec3], distance: impl Fn(DVec3) -> f64) -> Vec<DVec3> {
    let Some(&mut_previous) = points.last() else {
        return vec![];
    };
    let mut previous = mut_previous;
    let mut before = distance(previous);
    let mut result = Vec::with_capacity(points.len() + 1);
    for &current in points {
        let after = distance(current);
        if (before >= 0.) != (after >= 0.) {
            result.push(previous.lerp(current, before / (before - after)));
        }
        if after >= 0. {
            result.push(current);
        }
        previous = current;
        before = after;
    }
    result.dedup_by(|a, b| a.distance_squared(*b) < 1e-12);
    if result.len() > 1 && result[0].distance_squared(*result.last().unwrap()) < 1e-12 {
        result.pop();
    }
    result
}
fn intersect_polygon(points: &[DVec3], mask: &[DVec3]) -> Vec<DVec3> {
    let orientation = signed_area(mask).signum();
    let mut result = points.to_vec();
    for (&a, &b) in mask
        .iter()
        .zip(mask.iter().cycle().skip(1))
        .take(mask.len())
    {
        if (a.truncate() - b.truncate()).length_squared() < 1e-12 {
            continue;
        }
        result = clip_polygon(&result, |p| cross(a, b, p) * orientation);
        if result.len() < 3 {
            break;
        }
    }
    result
}
/// Disjoint convex fragments of `subject - mask`, preserving the subject's depth plane.
fn subtract_polygon(subject: &[DVec3], mask: &[DVec3]) -> Vec<Vec<DVec3>> {
    let orientation = signed_area(mask).signum();
    let mut remaining = subject.to_vec();
    let mut fragments = Vec::new();
    for (&a, &b) in mask
        .iter()
        .zip(mask.iter().cycle().skip(1))
        .take(mask.len())
    {
        if (a.truncate() - b.truncate()).length_squared() < 1e-12 {
            continue;
        }
        let outside = clip_polygon(&remaining, |p| -cross(a, b, p) * orientation);
        if area(&outside) > AREA_EPSILON {
            fragments.push(outside);
        }
        remaining = clip_polygon(&remaining, |p| cross(a, b, p) * orientation);
        if area(&remaining) <= AREA_EPSILON {
            break;
        }
    }
    fragments
}
fn clip_homogeneous(points: &[DVec4]) -> Vec<DVec4> {
    let mut result = points.to_vec();
    for plane in [
        DVec4::new(1., 0., 0., 1.),
        DVec4::new(-1., 0., 0., 1.),
        DVec4::new(0., 1., 0., 1.),
        DVec4::new(0., -1., 0., 1.),
        DVec4::Z,
        DVec4::new(0., 0., -1., 1.),
    ] {
        let Some(&last) = result.last() else {
            break;
        };
        let mut previous = last;
        let mut before = plane.dot(last);
        let mut next = Vec::with_capacity(result.len() + 1);
        for current in result {
            let after = plane.dot(current);
            if (before >= 0.) != (after >= 0.) {
                next.push(previous.lerp(current, before / (before - after)));
            }
            if after >= 0. {
                next.push(current);
            }
            previous = current;
            before = after;
        }
        result = next;
    }
    result
}
fn screen(clip: DVec4, rect: Rect) -> Option<DVec3> {
    if clip.w <= 0. || !clip.is_finite() {
        return None;
    }
    let n = clip.truncate() / clip.w;
    Some(DVec3::new(
        rect.min.x as f64 + (n.x * 0.5 + 0.5) * rect.width() as f64,
        rect.min.y as f64 + (0.5 - n.y * 0.5) * rect.height() as f64,
        n.z,
    ))
}
fn clip_segment(mut a: DVec4, mut b: DVec4) -> Option<[DVec4; 2]> {
    for plane in [
        DVec4::new(1., 0., 0., 1.),
        DVec4::new(-1., 0., 0., 1.),
        DVec4::new(0., 1., 0., 1.),
        DVec4::new(0., -1., 0., 1.),
        DVec4::Z,
        DVec4::new(0., 0., -1., 1.),
    ] {
        let da = plane.dot(a);
        let db = plane.dot(b);
        if da < 0. && db < 0. {
            return None;
        }
        if da < 0. {
            a = a.lerp(b, da / (da - db));
        } else if db < 0. {
            b = a.lerp(b, da / (da - db));
        }
    }
    Some([a, b])
}
fn segment_box(points: [DVec3; 2], rect: Rect) -> Option<[DVec3; 2]> {
    let [a, b] = points;
    let delta = b - a;
    let mut lo = 0f64;
    let mut hi = 1f64;
    for (origin, step, min, max) in [
        (a.x, delta.x, rect.min.x as f64, rect.max.x as f64),
        (a.y, delta.y, rect.min.y as f64, rect.max.y as f64),
    ] {
        if step.abs() < 1e-12 {
            if origin < min || origin > max {
                return None;
            }
        } else {
            let x = (min - origin) / step;
            let y = (max - origin) / step;
            lo = lo.max(x.min(y));
            hi = hi.min(x.max(y));
            if hi < lo {
                return None;
            }
        }
    }
    Some([a + delta * lo, a + delta * hi])
}
#[derive(Clone)]
struct Surface {
    face: u32,
    points: Vec<DVec3>,
    rect: Rect,
    /// z = x * plane.x + y * plane.y + plane.z (projected depth is affine).
    plane: DVec3,
}
impl Surface {
    fn new(face: u32, clips: &[DVec4], rect: Rect) -> Option<Self> {
        let points: Option<Vec<_>> = clip_homogeneous(clips)
            .into_iter()
            .map(|p| screen(p, rect))
            .collect();
        let points = points?;
        if area(&points) <= AREA_EPSILON {
            return None;
        }
        let a = points[0];
        let normal = points[1..]
            .windows(2)
            .map(|p| (p[0] - a).cross(p[1] - a))
            .find(|n| n.z.abs() > 1e-12)?;
        let plane = DVec3::new(
            -normal.x / normal.z,
            -normal.y / normal.z,
            normal.dot(a) / normal.z,
        );
        Some(Self {
            face,
            rect: bounds(&points),
            points,
            plane,
        })
    }
    fn depth(&self, p: DVec3) -> f64 {
        self.plane.x * p.x + self.plane.y * p.y + self.plane.z
    }
}
/// One projection, discarded/replaced when its mesh, camera, transform, viewport or DPI changes.
pub struct ProjectedMesh {
    pub mesh: EditableMesh,
    pub rect: Rect,
    pub vertices: Vec<Option<DVec3>>,
    pub edges: Vec<Option<[DVec3; 2]>>,
    faces: Vec<Surface>,
    edge_grid: ScreenGrid,
    vertex_grid: ScreenGrid,
}
impl ProjectedMesh {
    pub fn new(mesh: &EditableMesh, mvp: Mat4, rect: Rect) -> Self {
        let mvp = mvp.as_dmat4();
        let clips: Vec<_> = mesh
            .data()
            .vertices
            .iter()
            .map(|v| mvp * Vec3::from(v.position).as_dvec3().extend(1.))
            .collect();
        let vertices = clips
            .iter()
            .map(|&p| {
                let n = screen(p, rect)?;
                ((0.0..=1.0).contains(&n.z) && rect.contains(Pos2::new(n.x as f32, n.y as f32)))
                    .then_some(n)
            })
            .collect();
        let edges: Vec<_> = mesh
            .data()
            .edges
            .iter()
            .map(|e| {
                let [a, b] = e.vertices.map(|id| clips[mesh.prepared().vertices[&id]]);
                let [a, b] = clip_segment(a, b)?;
                Some([screen(a, rect)?, screen(b, rect)?])
            })
            .collect();
        let faces = mesh
            .prepared()
            .triangles
            .iter()
            .filter_map(|t| {
                let f = mesh.face(t.face).expect("Validated triangle");
                let points = t
                    .corners
                    .map(|c| clips[mesh.prepared().vertices[&f.corners[c].vertex]]);
                Surface::new(t.face, &points, rect)
            })
            .collect();
        let mut result = Self {
            mesh: mesh.clone(),
            rect,
            vertices,
            edges,
            faces,
            edge_grid: ScreenGrid::default(),
            vertex_grid: ScreenGrid::default(),
        };
        for (i, edge) in result.edges.iter().enumerate() {
            if let Some(points) = edge {
                result.edge_grid.add(i, bounds(points));
            }
        }
        for (i, point) in result.vertices.iter().enumerate() {
            if let Some(p) = point {
                result.vertex_grid.add(
                    i,
                    Rect::from_center_size(
                        Pos2::new(p.x as f32, p.y as f32),
                        egui::Vec2::splat(0.5),
                    ),
                );
            }
        }
        result
    }
    pub fn estimated_bytes(&self) -> usize {
        self.vertices.capacity() * std::mem::size_of::<Option<DVec3>>()
            + self.edges.capacity() * std::mem::size_of::<Option<[DVec3; 2]>>()
            + self.faces.capacity() * std::mem::size_of::<Surface>()
            + self
                .faces
                .iter()
                .map(|f| f.points.capacity() * std::mem::size_of::<DVec3>())
                .sum::<usize>()
            + self.edge_grid.estimated_bytes()
            + self.vertex_grid.estimated_bytes()
    }
    /// Through selection never creates or queries an occlusion structure.
    pub fn select(
        &self,
        mode: Mode,
        box_rect: Rect,
        occlusion: Option<(&Occlusion, &str)>,
    ) -> Vec<u32> {
        let rect = box_rect.intersect(self.rect);
        if rect.width() < 0. || rect.height() < 0. {
            return vec![];
        }
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        match mode {
            Mode::Object => {}
            Mode::Vertex => {
                for (vertex, point) in self.mesh.data().vertices.iter().zip(&self.vertices) {
                    if let Some(p) = point
                        && rect
                            .expand(VERTEX_TOLERANCE)
                            .contains(Pos2::new(p.x as f32, p.y as f32))
                        && occlusion.is_none_or(|(o, id)| o.point_visible(*p, id))
                    {
                        result.push(vertex.id);
                    }
                }
            }
            Mode::Edge => {
                for (edge, points) in self.mesh.data().edges.iter().zip(&self.edges) {
                    if let Some(points) = points.and_then(|p| segment_box(p, rect))
                        && occlusion.is_none_or(|(o, id)| o.segment_visible(points, id))
                    {
                        result.push(edge.id);
                    }
                }
            }
            Mode::Face => {
                let mask = rect_polygon(rect);
                for surface in &self.faces {
                    if seen.contains(&surface.face) || !surface.rect.intersects(rect) {
                        continue;
                    }
                    let inside = intersect_polygon(&surface.points, &mask);
                    if area(&inside) > AREA_EPSILON
                        && occlusion.is_none_or(|(o, id)| o.surface_visible(&inside, id))
                    {
                        result.push(surface.face);
                        seen.insert(surface.face);
                    }
                }
            }
        }
        result
    }
    pub fn edge_candidates(&self, point: Pos2, radius: f32) -> Vec<usize> {
        self.edge_grid.query(Rect::from_center_size(
            point,
            egui::Vec2::splat(radius * 2.),
        ))
    }
    pub fn vertex_candidates(&self, point: Pos2, radius: f32) -> Vec<usize> {
        self.vertex_grid.query(Rect::from_center_size(
            point,
            egui::Vec2::splat(radius * 2.),
        ))
    }
}

/// A screen tile index with a bounded number of cells for unusually large polygons.
struct ScreenGrid {
    cells: HashMap<(i32, i32), Vec<usize>>,
    large: Vec<usize>,
    references: usize,
    cell_size: f32,
}
impl Default for ScreenGrid {
    fn default() -> Self {
        Self {
            cells: HashMap::new(),
            large: Vec::new(),
            references: 0,
            cell_size: 48.,
        }
    }
}
impl ScreenGrid {
    fn range(&self, rect: Rect) -> (i32, i32, i32, i32) {
        (
            (rect.min.x / self.cell_size).floor() as i32,
            (rect.max.x / self.cell_size).floor() as i32,
            (rect.min.y / self.cell_size).floor() as i32,
            (rect.max.y / self.cell_size).floor() as i32,
        )
    }
    fn adaptive(bounds: Rect, count: usize) -> Self {
        let cell_size = if count > 0 && bounds.is_positive() && bounds.area().is_finite() {
            (bounds.area() / count as f32)
                .sqrt()
                .mul_add(2., 0.)
                .clamp(2., 48.)
        } else {
            48.
        };
        Self {
            cell_size,
            ..Default::default()
        }
    }
    fn add(&mut self, index: usize, rect: Rect) {
        let (x0, x1, y0, y1) = self.range(rect);
        let count = i64::from(x1 - x0 + 1) * i64::from(y1 - y0 + 1);
        // A pathological set of screen-filling triangles must not duplicate its
        // IDs into millions of tiles. Overflow stays in the complete large list.
        if count > 256 || self.references.saturating_add(count as usize) > 1_000_000 {
            self.large.push(index);
            return;
        }
        self.references += count as usize;
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }
    fn query(&self, rect: Rect) -> Vec<usize> {
        let (x0, x1, y0, y1) = self.range(rect);
        let mut result = self.large.clone();
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(indices) = self.cells.get(&(x, y)) {
                    result.extend(indices);
                }
            }
        }
        result.sort_unstable();
        result.dedup();
        result
    }
    fn estimated_bytes(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<((i32, i32), Vec<usize>)>()
            + self
                .cells
                .values()
                .map(|v| v.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + self.large.capacity() * std::mem::size_of::<usize>()
    }
}
#[derive(Clone, PartialEq)]
pub struct OccluderKey {
    id: Id,
    mesh: mesh::MeshKey,
    world: [f32; 16],
    layer: i32,
    alpha: u32,
}
/// Stable full-scene visibility key: insertion/removal/reorder and ancestors are included.
pub fn occluder_keys(scene: &Scene, view: &SceneView<'_>) -> Vec<OccluderKey> {
    scene
        .entities
        .iter()
        .filter(|e| e.has_geometry() && e.ui.is_none() && view.visible(e))
        .filter_map(|e| {
            Some(OccluderKey {
                id: e.id.clone(),
                mesh: mesh::MeshKey::for_entity(e)?,
                world: (view.world_matrix(&e.id).ok()?
                    * Mat4::from_scale(Vec3::from(e.dimensions)))
                .to_cols_array(),
                layer: e.layer,
                alpha: e.material.color[3].to_bits(),
            })
        })
        .collect()
}
struct Occluder {
    surface: Surface,
    layer: (i32, usize),
}
/// Exact polygon/segment occlusion for the portion inside the selection rectangle.
/// Construct only for a normal rectangle; drawing uses the current GPU depth instead.
pub struct Occlusion {
    surfaces: Vec<Occluder>,
    grid: ScreenGrid,
    layers: HashMap<Id, (i32, usize)>,
    kind: SceneKind,
}
impl Occlusion {
    pub fn new(
        scene: &Scene,
        view: &SceneView<'_>,
        camera: &CameraState,
        rect: Rect,
        scale: f32,
    ) -> Self {
        Self::build(scene, view, camera, rect, scale, None)
    }
    /// Reuse the selected mesh's exact projection, including converted parametric dimensions.
    /// Different multiplication orders must not make a surface appear in front of itself.
    pub fn with_target(
        scene: &Scene,
        view: &SceneView<'_>,
        camera: &CameraState,
        rect: Rect,
        scale: f32,
        target: (&str, &ProjectedMesh),
    ) -> Self {
        Self::build(scene, view, camera, rect, scale, Some(target))
    }
    fn build(
        scene: &Scene,
        view: &SceneView<'_>,
        camera: &CameraState,
        rect: Rect,
        scale: f32,
        target: Option<(&str, &ProjectedMesh)>,
    ) -> Self {
        let size = [
            (rect.width() * scale).max(1.) as u32,
            (rect.height() * scale).max(1.) as u32,
        ];
        let matrix = camera.matrix(size);
        let mut result = Self {
            surfaces: Vec::new(),
            grid: ScreenGrid::default(),
            layers: HashMap::new(),
            kind: scene.kind,
        };
        for (order, entity) in scene.entities.iter().enumerate() {
            if !entity.has_geometry()
                || entity.ui.is_some()
                || !view.visible(entity)
                || entity.material.color[3] < 0.01
            {
                continue;
            }
            result
                .layers
                .insert(entity.id.clone(), (entity.layer, order));
            if let Some((id, projected)) = target
                && id == entity.id
            {
                for surface in &projected.faces {
                    result.surfaces.push(Occluder {
                        surface: surface.clone(),
                        layer: (entity.layer, order),
                    });
                }
                continue;
            }
            let Ok(world) = view.world_matrix(&entity.id) else {
                continue;
            };
            let Some(mesh) = mesh::cached_entity(entity) else {
                continue;
            };
            let mvp = (matrix * world * Mat4::from_scale(Vec3::from(entity.dimensions))).as_dmat4();
            let clips: Vec<_> = mesh
                .vertices
                .iter()
                .map(|v| mvp * v.position.as_dvec3().extend(1.))
                .collect();
            for triangle in mesh.indices.chunks_exact(3) {
                let clips = [
                    clips[triangle[0] as usize],
                    clips[triangle[1] as usize],
                    clips[triangle[2] as usize],
                ];
                if let Some(surface) = Surface::new(0, &clips, rect) {
                    result.surfaces.push(Occluder {
                        surface,
                        layer: (entity.layer, order),
                    });
                }
            }
        }
        // Small triangles must not all be compared with hundreds of neighbors
        // sharing a fixed 48-point tile. This changes only the conservative
        // candidate index; exact clipping/depth tests below remain identical.
        let bounds = result
            .surfaces
            .iter()
            .fold(Rect::NOTHING, |bounds, s| bounds.union(s.surface.rect));
        result.grid = ScreenGrid::adaptive(bounds, result.surfaces.len());
        for (i, surface) in result.surfaces.iter().enumerate() {
            result.grid.add(i, surface.surface.rect);
        }
        result
    }
    pub fn estimated_bytes(&self) -> usize {
        self.surfaces.capacity() * std::mem::size_of::<Occluder>()
            + self
                .surfaces
                .iter()
                .map(|s| s.surface.points.capacity() * std::mem::size_of::<DVec3>())
                .sum::<usize>()
            + self.grid.estimated_bytes()
    }
    fn eligible(&self, o: &Occluder, owner: &str) -> bool {
        self.kind != SceneKind::TwoD || self.layers.get(owner).is_some_and(|layer| o.layer > *layer)
    }
    fn front(&self, o: &Occluder, p: DVec3) -> f64 {
        if self.kind == SceneKind::TwoD {
            1.
        } else {
            p.z - o.surface.depth(p) - DEPTH_EPSILON
        }
    }
    fn point_visible(&self, point: DVec3, owner: &str) -> bool {
        let rect = Rect::from_center_size(
            Pos2::new(point.x as f32, point.y as f32),
            egui::Vec2::splat(0.001),
        );
        !self.grid.query(rect).into_iter().any(|i| {
            let o = &self.surfaces[i];
            if !o
                .surface
                .rect
                .contains(Pos2::new(point.x as f32, point.y as f32))
            {
                return false;
            }
            let orientation = signed_area(&o.surface.points).signum();
            self.eligible(o, owner)
                && self.front(o, point) > 0.
                && o.surface
                    .points
                    .iter()
                    .zip(o.surface.points.iter().cycle().skip(1))
                    .take(o.surface.points.len())
                    .all(|(&a, &b)| cross(a, b, point) * orientation >= 0.)
        })
    }
    fn segment_visible(&self, points: [DVec3; 2], owner: &str) -> bool {
        let mut pieces = vec![points];
        let segment_bounds = bounds(&points);
        for index in self.grid.query(segment_bounds) {
            let o = &self.surfaces[index];
            if !self.eligible(o, owner) || !o.surface.rect.intersects(segment_bounds) {
                continue;
            }
            let mut next = Vec::new();
            for [a, b] in pieces {
                let mut lo = 0f64;
                let mut hi = 1f64;
                let orientation = signed_area(&o.surface.points).signum();
                let mut distances: Vec<_> = o
                    .surface
                    .points
                    .iter()
                    .zip(o.surface.points.iter().cycle().skip(1))
                    .take(o.surface.points.len())
                    .map(|(&x, &y)| (cross(x, y, a) * orientation, cross(x, y, b) * orientation))
                    .collect();
                distances.push((self.front(o, a), self.front(o, b)));
                for (da, db) in distances {
                    if da <= 0. && db <= 0. {
                        lo = 1.;
                        hi = 0.;
                        break;
                    }
                    if da < 0. {
                        lo = lo.max(da / (da - db));
                    } else if db < 0. {
                        hi = hi.min(da / (da - db));
                    }
                }
                if hi <= lo {
                    next.push([a, b]);
                    continue;
                }
                if lo > 0.000001 {
                    next.push([a, a.lerp(b, lo)]);
                }
                if hi < 0.999999 {
                    next.push([a.lerp(b, hi), b]);
                }
            }
            if next.is_empty() {
                return false;
            }
            pieces = next;
        }
        !pieces.is_empty()
    }
    fn surface_visible(&self, points: &[DVec3], owner: &str) -> bool {
        let mut pieces = vec![points.to_vec()];
        for index in self.grid.query(bounds(points)) {
            let o = &self.surfaces[index];
            if !self.eligible(o, owner) {
                continue;
            }
            let mut next = Vec::new();
            for piece in pieces {
                if !o.surface.rect.intersects(bounds(&piece))
                    || piece.iter().all(|p| self.front(o, *p) <= 0.)
                {
                    next.push(piece);
                    continue;
                }
                let cover = intersect_polygon(&piece, &o.surface.points);
                let cover = clip_polygon(&cover, |p| self.front(o, p));
                if area(&cover) <= AREA_EPSILON {
                    next.push(piece);
                } else {
                    next.extend(subtract_polygon(&piece, &cover));
                }
            }
            if next.is_empty() {
                return false;
            }
            pieces = next;
        }
        pieces.iter().any(|p| area(p) > AREA_EPSILON)
    }
}

#[cfg(test)]
mod tests;
