//! Authored polygon topology. GPU triangles, indices and cache identity are derived only.
pub mod atlas;
pub mod bevel;
pub mod cuts;
pub mod edit;
pub mod operations;
pub mod primitives;
pub mod ray;
pub mod selection;
pub mod snap;
mod triangulate;

use glam::{Vec2, Vec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub type ComponentId = u32;
pub const MAX_VERTICES: usize = 250_000;
pub const MAX_EDGES: usize = 500_000;
pub const MAX_FACES: usize = 200_000;
pub const MAX_CORNERS: usize = 1_000_000;
pub const MAX_FACE_CORNERS: usize = 512;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vertex {
    pub id: ComponentId,
    pub position: [f32; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: ComponentId,
    pub vertices: [ComponentId; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corner {
    pub vertex: ComponentId,
    pub uv: [f32; 2],
    /// Preserves the exact primitive shading at conversion. Geometry edits invalidate these.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f32; 3]>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Face {
    pub id: ComponentId,
    pub corners: Vec<Corner>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shading {
    #[default]
    Flat,
    Smooth,
}

/// The only persisted mesh data. IDs are local, monotonic and independent of buffer positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshData {
    pub next_id: ComponentId,
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    #[serde(default)]
    pub shading: Shading,
}
impl Default for MeshData {
    fn default() -> Self {
        Self {
            next_id: 1,
            vertices: vec![],
            edges: vec![],
            faces: vec![],
            shading: Shading::Flat,
        }
    }
}
impl MeshData {
    pub fn allocate_id(&mut self) -> Result<ComponentId, String> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or("A malha esgotou seus identificadores locais.")?;
        Ok(id)
    }
    pub fn add_vertex(&mut self, position: Vec3) -> Result<ComponentId, String> {
        if self.vertices.len() >= MAX_VERTICES {
            return Err("Limite de 250.000 vértices por malha.".into());
        }
        let id = self.allocate_id()?;
        self.vertices.push(Vertex {
            id,
            position: position.to_array(),
        });
        Ok(id)
    }
    /// Call once after an operator, preserving existing edges (including deliberately loose ones).
    pub fn complete_edges(&mut self) -> Result<(), String> {
        let mut pairs: HashSet<_> = self.edges.iter().map(|edge| pair(edge.vertices)).collect();
        let mut missing = Vec::new();
        for face in &self.faces {
            for (a, b) in face
                .corners
                .iter()
                .zip(face.corners.iter().cycle().skip(1))
                .take(face.corners.len())
            {
                let vertices = [a.vertex, b.vertex];
                if pairs.insert(pair(vertices)) {
                    missing.push(vertices);
                }
            }
        }
        if self.edges.len() + missing.len() > MAX_EDGES {
            return Err("Limite de 500.000 arestas por malha.".into());
        }
        for vertices in missing {
            let id = self.allocate_id()?;
            self.edges.push(Edge { id, vertices });
        }
        Ok(())
    }
    pub fn add_face(&mut self, corners: Vec<Corner>) -> Result<ComponentId, String> {
        if self.faces.len() >= MAX_FACES {
            return Err("Limite de 200.000 faces por malha.".into());
        }
        let id = self.allocate_id()?;
        self.faces.push(Face { id, corners });
        Ok(id)
    }
}
pub fn pair(vertices: [ComponentId; 2]) -> [ComponentId; 2] {
    [vertices[0].min(vertices[1]), vertices[0].max(vertices[1])]
}

#[derive(Clone, Debug)]
pub struct Triangle {
    pub face: ComponentId,
    pub corners: [usize; 3],
}
#[derive(Debug)]
pub struct PreparedMesh {
    pub acceleration: Arc<ray::Bvh>,
    pub vertices: HashMap<ComponentId, usize>,
    pub edges: HashMap<ComponentId, usize>,
    pub faces: HashMap<ComponentId, usize>,
    pub edge_pairs: HashMap<[ComponentId; 2], usize>,
    /// Stable face order, never HashMap iteration order.
    pub incident_faces: Vec<Vec<(usize, usize)>>,
    pub triangles: Vec<Triangle>,
    pub face_normals: Vec<Vec3>,
    pub smooth_normals: Vec<Vec3>,
    pub bounds: Option<(Vec3, Vec3)>,
}
#[derive(Debug)]
struct Storage {
    data: MeshData,
    prepared: PreparedMesh,
    revision: u64,
}
/// Immutable, shared storage makes scene copies cheap. Every replacement is validated atomically.
/// A fresh revision is generated even when the component counts stay unchanged.
#[derive(Clone, Debug)]
pub struct EditableMesh(Arc<Storage>);
impl PartialEq for EditableMesh {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.data == other.0.data
    }
}
impl Serialize for EditableMesh {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.data().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for EditableMesh {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(MeshData::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl EditableMesh {
    pub fn new(data: MeshData) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let prepared = prepare(&data)?;
        Ok(Self(Arc::new(Storage {
            data,
            prepared,
            revision: NEXT.fetch_add(1, Ordering::Relaxed),
        })))
    }
    pub fn data(&self) -> &MeshData {
        &self.0.data
    }
    pub fn prepared(&self) -> &PreparedMesh {
        &self.0.prepared
    }
    pub fn revision(&self) -> u64 {
        self.0.revision
    }
    pub fn position(&self, id: ComponentId) -> Option<Vec3> {
        self.prepared()
            .vertices
            .get(&id)
            .map(|&i| Vec3::from(self.data().vertices[i].position))
    }
    pub fn face(&self, id: ComponentId) -> Option<&Face> {
        self.prepared()
            .faces
            .get(&id)
            .map(|&i| &self.data().faces[i])
    }
    pub fn edge(&self, id: ComponentId) -> Option<&Edge> {
        self.prepared()
            .edges
            .get(&id)
            .map(|&i| &self.data().edges[i])
    }
    pub fn triangle_points(&self, triangle: &Triangle) -> [Vec3; 3] {
        let face = self.face(triangle.face).expect("Validated face");
        triangle.corners.map(|i| {
            self.position(face.corners[i].vertex)
                .expect("Validated vertex")
        })
    }
    pub fn corner_normal(&self, face_index: usize, corner: &Corner) -> Vec3 {
        corner
            .normal
            .map(Vec3::from)
            .unwrap_or_else(|| match self.data().shading {
                Shading::Flat => self.prepared().face_normals[face_index],
                Shading::Smooth => {
                    self.prepared().smooth_normals[self.prepared().vertices[&corner.vertex]]
                }
            })
    }
    pub fn shares_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// Logical allocation estimate, excludes allocator metadata and GPU storage.
    pub fn estimated_bytes(&self) -> usize {
        self.data().vertices.capacity() * std::mem::size_of::<Vertex>()
            + self.data().edges.capacity() * std::mem::size_of::<Edge>()
            + self.data().faces.capacity() * std::mem::size_of::<Face>()
            + self
                .data()
                .faces
                .iter()
                .map(|f| f.corners.capacity() * std::mem::size_of::<Corner>())
                .sum::<usize>()
            + self.prepared().triangles.capacity() * std::mem::size_of::<Triangle>()
    }
}

fn prepare(data: &MeshData) -> Result<PreparedMesh, String> {
    if data.vertices.len() > MAX_VERTICES
        || data.edges.len() > MAX_EDGES
        || data.faces.len() > MAX_FACES
        || data.faces.iter().map(|f| f.corners.len()).sum::<usize>() > MAX_CORNERS
    {
        return Err("Malha excede os limites: 250.000 vértices, 500.000 arestas, 200.000 faces ou 1.000.000 cantos.".into());
    }
    let mut ids = HashSet::new();
    for id in data
        .vertices
        .iter()
        .map(|v| v.id)
        .chain(data.edges.iter().map(|e| e.id))
        .chain(data.faces.iter().map(|f| f.id))
    {
        if id == 0 || id >= data.next_id || !ids.insert(id) {
            return Err(format!(
                "Identificador de componente inválido ou duplicado: {id}"
            ));
        }
    }
    let vertices: HashMap<_, _> = data
        .vertices
        .iter()
        .enumerate()
        .map(|(i, v)| (v.id, i))
        .collect();
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for v in &data.vertices {
        let p = Vec3::from(v.position);
        if !p.is_finite() || p.abs().max_element() > 1e7 {
            return Err(format!(
                "Vértice {}: posição inválida; limite de 10 milhões de unidades.",
                v.id
            ));
        }
        bounds = Some(bounds.map_or((p, p), |(min, max)| (min.min(p), max.max(p))));
    }
    let mut edge_pairs = HashMap::new();
    for (i, e) in data.edges.iter().enumerate() {
        if e.vertices[0] == e.vertices[1] || e.vertices.iter().any(|id| !vertices.contains_key(id))
        {
            return Err(format!("Aresta {}: vértices ausentes ou iguais.", e.id));
        }
        if edge_pairs.insert(pair(e.vertices), i).is_some() {
            return Err(format!("Aresta {} duplicada.", e.id));
        }
        let a = Vec3::from(data.vertices[vertices[&e.vertices[0]]].position);
        let b = Vec3::from(data.vertices[vertices[&e.vertices[1]]].position);
        if a.distance_squared(b) < 1e-16 {
            return Err(format!("Aresta {} tem comprimento nulo.", e.id));
        }
    }
    let mut incident_faces = vec![Vec::new(); data.edges.len()];
    let mut triangles = Vec::new();
    let mut face_normals = Vec::new();
    let mut smooth_normals = vec![Vec3::ZERO; data.vertices.len()];
    let mut face_sets = HashSet::new();
    for (fi, face) in data.faces.iter().enumerate() {
        if !(3..=MAX_FACE_CORNERS).contains(&face.corners.len()) {
            return Err(format!("Face {}: use de 3 a 512 cantos.", face.id));
        }
        let mut points = Vec::with_capacity(face.corners.len());
        let mut unique = HashSet::new();
        for corner in &face.corners {
            let Some(&vi) = vertices.get(&corner.vertex) else {
                return Err(format!(
                    "Face {}: vértice {} ausente.",
                    face.id, corner.vertex
                ));
            };
            if !unique.insert(corner.vertex)
                || !Vec2::from(corner.uv).is_finite()
                || corner.normal.is_some_and(|n| {
                    !Vec3::from(n).is_finite() || (Vec3::from(n).length() - 1.).abs() > 0.001
                })
            {
                return Err(format!(
                    "Face {}: canto repetido, UV ou normal inválida.",
                    face.id
                ));
            }
            points.push(Vec3::from(data.vertices[vi].position));
        }
        let mut signature: Vec<_> = unique.into_iter().collect();
        signature.sort_unstable();
        if !face_sets.insert(signature) {
            return Err(format!("Face {} duplicada.", face.id));
        }
        let (normal, indices) =
            triangulate::polygon(&points).map_err(|e| format!("Face {}: {e}", face.id))?;
        face_normals.push(normal);
        for triangle in indices {
            triangles.push(Triangle {
                face: face.id,
                corners: triangle,
            });
        }
        for (ci, corner) in face.corners.iter().enumerate() {
            smooth_normals[vertices[&corner.vertex]] += normal;
            let next = &face.corners[(ci + 1) % face.corners.len()];
            let Some(&ei) = edge_pairs.get(&pair([corner.vertex, next.vertex])) else {
                return Err(format!("Face {}: aresta de contorno ausente.", face.id));
            };
            incident_faces[ei].push((fi, ci));
            if incident_faces[ei].len() > 2 {
                return Err(format!(
                    "Aresta {} teria mais de duas faces.",
                    data.edges[ei].id
                ));
            }
        }
    }
    for normal in &mut smooth_normals {
        *normal = normal.try_normalize().unwrap_or(Vec3::Y);
    }
    let faces: HashMap<_, _> = data
        .faces
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id, i))
        .collect();
    let points: Vec<_> = triangles
        .iter()
        .map(|t| {
            let face = &data.faces[faces[&t.face]];
            t.corners
                .map(|ci| Vec3::from(data.vertices[vertices[&face.corners[ci].vertex]].position))
        })
        .collect();
    Ok(PreparedMesh {
        acceleration: Arc::new(ray::Bvh::build(&points)),
        vertices,
        edges: data
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect(),
        faces,
        edge_pairs,
        incident_faces,
        triangles,
        face_normals,
        smooth_normals,
        bounds,
    })
}
