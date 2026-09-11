//! Query-only Rapier adapter. OXY owns velocities, gravity and platform transport.
//! Library handles and prepared shapes are transient; external identity is always OXY ID.
mod scene;
mod shapes;
mod sweep;
pub use scene::*;
pub use shapes::*;
pub use sweep::SweepSpan;

use crate::document::Id;
use glam::{Quat, Vec3};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::{
    control::{CharacterAutostep, CharacterLength, KinematicCharacterController},
    prelude::*,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap, HashSet},
};

fn vector(v: Vec3) -> Vector {
    Vector::from_array(v.to_array())
}
fn oxy(v: Vector) -> Vec3 {
    Vec3::from_array(v.to_array())
}
fn pose(position: Vec3, rotation: Quat) -> Pose {
    Pose::from_parts(
        vector(position),
        rapier3d::math::Rotation::from_array(rotation.to_array()),
    )
}

#[derive(Clone, Debug, Default)]
pub struct QueryHit {
    pub object: Id,
    pub point: Vec3,
    pub normal: Vec3,
    pub fraction: f32,
    pub distance: f32,
    pub surface: Option<Id>,
    pub face: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct QueryOptions {
    pub category: u32,
    pub mask: u32,
    pub include_sensors: bool,
    pub camera: bool,
    pub exclude: HashSet<Id>,
    pub only: Option<Id>,
    pub only_sensors: bool,
}
impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            category: 1,
            mask: u32::MAX,
            include_sensors: false,
            camera: false,
            exclude: HashSet::new(),
            only: None,
            only_sensors: false,
        }
    }
}
impl QueryOptions {
    pub fn excluding(id: &str) -> Self {
        Self {
            exclude: HashSet::from([id.into()]),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CapsuleMotion {
    pub height: f32,
    pub radius: f32,
    pub margin: f32,
    pub climb_degrees: f32,
    pub slide_degrees: f32,
    pub step_height: f32,
    pub step_width: f32,
    pub snap: f32,
}
impl Default for CapsuleMotion {
    fn default() -> Self {
        Self {
            height: 1.8,
            radius: 0.3,
            margin: 0.01,
            climb_degrees: 45.,
            slide_degrees: 46.,
            step_height: 0.25,
            step_width: 0.2,
            snap: 0.15,
        }
    }
}
impl CapsuleMotion {
    pub fn validate(self) -> Result<(), String> {
        let values = [
            self.height,
            self.radius,
            self.margin,
            self.climb_degrees,
            self.slide_degrees,
            self.step_height,
            self.step_width,
            self.snap,
        ];
        if values.iter().any(|v| !v.is_finite()) {
            return Err("Cápsula: valores devem ser finitos.".into());
        }
        if self.radius <= 0. || self.height < self.radius * 2. {
            return Err(
                "Cápsula: altura deve ser pelo menos o diâmetro; raio deve ser positivo.".into(),
            );
        }
        if self.margin <= 0.
            || self.margin >= self.radius
            || self.step_height < 0.
            || self.step_width <= 0.
            || self.snap < 0.
            || !(0. ..89.9).contains(&self.climb_degrees)
            || !(0. ..89.9).contains(&self.slide_degrees)
        {
            return Err("Cápsula: margem, degrau, aderência ou inclinação inválidos.".into());
        }
        Ok(())
    }
    fn shape(self) -> SharedShape {
        SharedShape::capsule_y(self.height * 0.5 - self.radius, self.radius)
    }
}

#[derive(Clone, Debug, Default)]
pub struct MotionResult {
    pub delta: Vec3,
    pub grounded: bool,
    pub sliding: bool,
    pub contacts: Vec<QueryHit>,
    /// Actual swept translation pieces, relative to the initial feet position.
    pub segments: Vec<(Vec3, Vec3)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsCounters {
    pub shapes_prepared: u64,
    pub poses_updated: u64,
    pub tree_updates: u64,
    pub queries: u64,
    pub candidate_tests: u64,
    pub colliders: usize,
}
struct Entry {
    handle: ColliderHandle,
    key: ShapeKey,
    // Keep the immutable payload alive: an Arc address cannot be reused under a cache key.
    source: CollisionShape,
    debug: std::sync::OnceLock<DebugGeometry>,
    position: Vec3,
    rotation: Quat,
    filter: CollisionFilter,
    sensor: bool,
    surface: Option<Id>,
}

pub struct PhysicsWorld {
    colliders: ColliderSet,
    bodies: RigidBodySet,
    broad: BroadPhaseBvh,
    narrow: NarrowPhase,
    islands: IslandManager,
    entries: BTreeMap<Id, Entry>,
    ids: HashMap<ColliderHandle, Id>,
    modified: Vec<ColliderHandle>,
    removed: Vec<ColliderHandle>,
    counters: PhysicsCounters,
    queries: Cell<u64>,
    candidates: Cell<u64>,
}
pub struct DebugGeometry {
    pub vertices: Vec<[f32; 3]>,
    pub edges: Vec<[u32; 2]>,
}
pub struct DebugBody<'a> {
    pub id: &'a str,
    pub geometry: &'a DebugGeometry,
    pub position: Vec3,
    pub rotation: Quat,
}
impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}
impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            colliders: ColliderSet::new(),
            bodies: RigidBodySet::new(),
            broad: BroadPhaseBvh::new(),
            narrow: NarrowPhase::new(),
            islands: IslandManager::new(),
            entries: BTreeMap::new(),
            ids: HashMap::new(),
            modified: vec![],
            removed: vec![],
            counters: PhysicsCounters::default(),
            queries: Cell::new(0),
            candidates: Cell::new(0),
        }
    }
    pub fn counters(&self) -> PhysicsCounters {
        PhysicsCounters {
            queries: self.queries.get(),
            candidate_tests: self.candidates.get(),
            colliders: self.entries.len(),
            ..self.counters
        }
    }
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
    pub fn remove(&mut self, id: &str) {
        if let Some(entry) = self.entries.remove(id) {
            self.ids.remove(&entry.handle);
            self.colliders
                .remove(entry.handle, &mut self.islands, &mut self.bodies, false);
            self.removed.push(entry.handle);
        }
    }
    /// A prepared mesh is immutable; a new geometry payload/scale invalidates preparation.
    /// Translation/rotation only update the collider pose and spatial tree leaf.
    pub fn upsert(
        &mut self,
        id: &str,
        config: &Collider3d,
        position: Vec3,
        rotation: Quat,
        scale: Vec3,
    ) -> Result<(), String> {
        if !config.enabled {
            self.remove(id);
            return Ok(());
        }
        if !position.is_finite()
            || !rotation.is_finite()
            || (rotation.length_squared() - 1.).abs() > 1e-4
            || !scale.is_finite()
            || !Vec3::from(config.center).is_finite()
            || scale.abs().min_element() < 1e-6
        {
            return Err("Transformação física não invertível ou não finita.".into());
        }
        let key = config.shape.key(scale);
        let changed_shape = self.entries.get(id).is_none_or(|e| e.key != key);
        let prepared = if changed_shape {
            Some(config.shape.prepare(scale)?)
        } else {
            None
        };
        let position = position + rotation * (scale * Vec3::from(config.center));
        if !position.is_finite() {
            return Err("Centro físico fora do intervalo numérico suportado.".into());
        }
        if let Some(entry) = self.entries.get_mut(id) {
            let collider = &mut self.colliders[entry.handle];
            let mut modified = false;
            if let Some(shape) = prepared {
                collider.set_shape(shape);
                entry.key = key;
                entry.source = config.shape.clone();
                entry.debug.take();
                self.counters.shapes_prepared += 1;
                modified = true;
            }
            if entry.position != position || entry.rotation != rotation {
                collider.set_position(pose(position, rotation));
                entry.position = position;
                entry.rotation = rotation;
                self.counters.poses_updated += 1;
                modified = true;
            }
            if entry.sensor != config.sensor {
                collider.set_sensor(config.sensor);
                entry.sensor = config.sensor;
                modified = true;
            }
            entry.filter = config.filter.clone();
            entry.surface.clone_from(&config.surface);
            if modified {
                self.modified.push(entry.handle);
            }
        } else {
            let handle = self.colliders.insert(
                ColliderBuilder::new(prepared.unwrap())
                    .position(pose(position, rotation))
                    .sensor(config.sensor)
                    .build(),
            );
            self.entries.insert(
                id.into(),
                Entry {
                    handle,
                    key,
                    source: config.shape.clone(),
                    debug: Default::default(),
                    position,
                    rotation,
                    filter: config.filter.clone(),
                    sensor: config.sensor,
                    surface: config.surface.clone(),
                },
            );
            self.ids.insert(handle, id.into());
            self.modified.push(handle);
            self.counters.shapes_prepared += 1;
        }
        Ok(())
    }
    pub fn flush(&mut self) {
        if self.modified.is_empty() && self.removed.is_empty() {
            return;
        }
        // No physics pipeline is run: only broad-phase leaves are synchronized.
        self.broad.update(
            &IntegrationParameters::default(),
            &self.colliders,
            &self.bodies,
            &self.modified,
            &self.removed,
            &mut Vec::new(),
        );
        self.modified.clear();
        self.removed.clear();
        self.counters.tree_updates += 1;
    }
    fn accepts(&self, handle: ColliderHandle, options: &QueryOptions) -> bool {
        self.candidates.set(self.candidates.get() + 1);
        let Some(id) = self.ids.get(&handle) else {
            return false;
        };
        let Some(e) = self.entries.get(id) else {
            return false;
        };
        !options.exclude.contains(id)
            && options.only.as_ref().is_none_or(|only| only == id)
            && (options.include_sensors || !e.sensor)
            && (!options.only_sensors || e.sensor)
            && (e.sensor || !options.camera || e.filter.blocks_camera)
            && (e.sensor || options.camera || e.filter.blocks_character)
            && e.filter.category & options.mask != 0
            && options.category & e.filter.mask != 0
    }
    fn hit(
        &self,
        handle: ColliderHandle,
        point: Vec3,
        normal: Vec3,
        fraction: f32,
        distance: f32,
    ) -> QueryHit {
        let id = self.ids[&handle].clone();
        QueryHit {
            surface: self.entries[&id].surface.clone(),
            object: id,
            point,
            normal,
            fraction,
            distance,
            face: None,
        }
    }
    pub fn cast(
        &self,
        shape: &CollisionShape,
        position: Vec3,
        direction: Vec3,
        options: &QueryOptions,
    ) -> Result<Option<QueryHit>, String> {
        if !position.is_finite() || !direction.is_finite() {
            return Err("Consulta: posição/deslocamento inválidos.".into());
        }
        let shape = shape.prepare(Vec3::ONE)?;
        let predicate = |h, _: &Collider| self.accepts(h, options);
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        self.queries.set(self.queries.get() + 1);
        Ok(queries
            .cast_shape(
                &pose(position, Quat::IDENTITY),
                vector(direction),
                &*shape,
                ShapeCastOptions {
                    max_time_of_impact: 1.,
                    stop_at_penetration: true,
                    compute_impact_geometry_on_penetration: true,
                    ..Default::default()
                },
            )
            .map(|(h, hit)| {
                self.hit(
                    h,
                    oxy(hit.witness1),
                    oxy(hit.normal1),
                    hit.time_of_impact,
                    direction.length() * hit.time_of_impact,
                )
            }))
    }
    pub fn ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        distance: f32,
        options: &QueryOptions,
    ) -> Result<Option<QueryHit>, String> {
        if !origin.is_finite() || !direction.is_finite() || !distance.is_finite() || distance < 0. {
            return Err("Raio: origem, direção ou distância inválidas.".into());
        }
        if direction.length_squared() < 1e-12 {
            return Ok(None);
        }
        let predicate = |h, _: &Collider| self.accepts(h, options);
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        let direction = direction.normalize();
        self.queries.set(self.queries.get() + 1);
        Ok(queries
            .cast_ray_and_get_normal(&Ray::new(vector(origin), vector(direction)), distance, true)
            .map(|(h, hit)| {
                let mut result = self.hit(
                    h,
                    origin + direction * hit.time_of_impact,
                    oxy(hit.normal),
                    hit.time_of_impact / distance.max(1e-6),
                    hit.time_of_impact,
                );
                if let FeatureId::Face(face) = hit.feature {
                    result.face = Some(face);
                }
                result
            }))
    }
    pub fn overlaps(
        &self,
        shape: &CollisionShape,
        position: Vec3,
        options: &QueryOptions,
    ) -> Result<Vec<Id>, String> {
        if !position.is_finite() {
            return Err("Consulta: posição inválida.".into());
        }
        let shape = shape.prepare(Vec3::ONE)?;
        let predicate = |h, _: &Collider| self.accepts(h, options);
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        self.queries.set(self.queries.get() + 1);
        let mut result: Vec<_> = queries
            .intersect_shape(pose(position, Quat::IDENTITY), &*shape)
            .filter_map(|(h, _)| self.ids.get(&h).cloned())
            .collect();
        result.sort();
        result.dedup();
        Ok(result)
    }
    pub fn move_capsule(
        &self,
        feet: Vec3,
        desired: Vec3,
        dt: f32,
        config: CapsuleMotion,
        options: &QueryOptions,
    ) -> Result<MotionResult, String> {
        config.validate()?;
        if !feet.is_finite() || !desired.is_finite() || !dt.is_finite() || dt <= 0. {
            return Err("Movimento físico inválido.".into());
        }
        let shape = config.shape();
        let center = feet + Vec3::Y * (config.height * 0.5);
        let solver = KinematicCharacterController {
            offset: CharacterLength::Absolute(config.margin),
            max_slope_climb_angle: config.climb_degrees.to_radians(),
            min_slope_slide_angle: config.slide_degrees.to_radians(),
            autostep: (config.step_height > 0.).then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(config.step_height),
                min_width: CharacterLength::Absolute(config.step_width),
                include_dynamic_bodies: false,
            }),
            snap_to_ground: (config.snap > 0. && desired.y <= 0.)
                .then_some(CharacterLength::Absolute(config.snap)),
            ..Default::default()
        };
        let predicate = |h, _: &Collider| self.accepts(h, options);
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        self.queries.set(self.queries.get() + 1);
        let mut result = MotionResult::default();
        let mut previous = Vec3::ZERO;
        let mut previous_wall = false;
        let mut stair_segments = Vec::new();
        let movement = solver.move_shape(
            dt,
            &queries,
            &*shape,
            &pose(center, Quat::IDENTITY),
            vector(desired),
            |collision| {
                let current = oxy(collision.translation_applied);
                if previous.distance_squared(current) > 1e-12 {
                    if previous_wall {
                        stair_segments.push(result.segments.len());
                    }
                    result.segments.push((previous, current));
                    previous = current;
                }
                result.contacts.push(self.hit(
                    collision.handle,
                    oxy(collision.hit.witness1),
                    oxy(collision.hit.normal1),
                    collision.hit.time_of_impact,
                    current.length(),
                ));
                previous_wall =
                    oxy(collision.hit.normal1).y.abs() < config.climb_degrees.to_radians().cos();
            },
        );
        result.delta = oxy(movement.translation);
        result.grounded = movement.grounded;
        result.sliding = movement.is_sliding_down_slope;
        if previous.distance_squared(result.delta) > 1e-12 {
            if previous_wall {
                stair_segments.push(result.segments.len());
            }
            result.segments.push((previous, result.delta));
        }
        // Rapier exposes collision waypoints, but a successful autostep can
        // combine lift and forward nudge in one waypoint. Define an explicit
        // clearance-checked route before exposing that movement to sensors.
        if config.step_height > 0. {
            let capsule = CollisionShape::Capsule {
                height: config.height,
                radius: config.radius,
            };
            let original = std::mem::take(&mut result.segments);
            for (index, (from, to)) in original.into_iter().enumerate() {
                let delta = to - from;
                let blocked = stair_segments.contains(&index)
                    && delta.y > 1e-5
                    && Vec3::new(delta.x, 0., delta.z).length_squared() > 1e-10
                    && self
                        .cast(&capsule, center + from, delta, options)?
                        .is_some_and(|h| h.fraction < 0.9999);
                if blocked {
                    let raised = from + Vec3::Y * (config.step_height + config.margin);
                    let over = Vec3::new(to.x, raised.y, to.z);
                    let route = [(from, raised), (raised, over), (over, to)];
                    for (a, b) in route {
                        if self
                            .cast(&capsule, center + a, b - a, options)?
                            .is_some_and(|h| h.fraction < 0.9999)
                        {
                            return Err("Não foi possível validar o trajeto do degrau; movimento preservado.".into());
                        }
                        result.segments.push((a, b));
                    }
                } else {
                    result.segments.push((from, to));
                }
            }
        }
        if !result.delta.is_finite() {
            return Err("Resolvedor retornou movimento inválido; posição preservada.".into());
        }
        Ok(result)
    }
    /// Unlike `overlaps`, exact tangency is free space. Used for standing up,
    /// safe teleports and bounded recovery, not for sensor overlap events.
    pub fn penetrating(
        &self,
        shape: &CollisionShape,
        position: Vec3,
        options: &QueryOptions,
    ) -> Result<Vec<Id>, String> {
        if !position.is_finite() {
            return Err("Consulta: posição inválida.".into());
        }
        let shape = shape.prepare(Vec3::ONE)?;
        let at = pose(position, Quat::IDENTITY);
        let predicate = |h, _: &Collider| self.accepts(h, options);
        let queries = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        self.queries.set(self.queries.get() + 1);
        let mut result = Vec::new();
        for (handle, collider) in queries.intersect_shape(at, &*shape) {
            let contact = self
                .narrow
                .query_dispatcher()
                .contact(
                    &at.inv_mul(collider.position()),
                    &*shape,
                    collider.shape(),
                    0.,
                )
                .map_err(|_| "Não foi possível verificar a profundidade de contato desta forma.")?;
            if contact.is_some_and(|c| c.dist < -1e-5)
                && let Some(id) = self.ids.get(&handle)
            {
                result.push(id.clone());
            }
        }
        result.sort();
        result.dedup();
        Ok(result)
    }
    /// Same prepared shape and pose used by all queries; rendering must not invent another body.
    pub fn debug_shapes(&self) -> impl Iterator<Item = DebugBody<'_>> {
        self.entries.iter().map(|(id, entry)| {
            let collider = &self.colliders[entry.handle];
            let geometry = entry.debug.get_or_init(|| {
                let (vertices, triangles) = match collider.shape().as_typed_shape() {
                    TypedShape::Cuboid(s) => s.to_trimesh(),
                    TypedShape::Capsule(s) => s.to_trimesh(12, 8),
                    TypedShape::Ball(s) => s.to_trimesh(12, 8),
                    TypedShape::ConvexPolyhedron(s) => s.to_trimesh(),
                    TypedShape::TriMesh(s) => (s.vertices().to_vec(), s.indices().to_vec()),
                    _ => unreachable!("OXY creates only its supported shapes"),
                };
                let vertices = vertices.into_iter().map(|v| v.to_array()).collect();
                let mut edges = std::collections::BTreeSet::new();
                for [a, b, c] in triangles {
                    for [a, b] in [[a, b], [b, c], [c, a]] {
                        edges.insert([a.min(b), a.max(b)]);
                    }
                }
                DebugGeometry {
                    vertices,
                    edges: edges.into_iter().collect(),
                }
            });
            DebugBody {
                id,
                geometry,
                position: entry.position,
                rotation: entry.rotation,
            }
        })
    }
}
