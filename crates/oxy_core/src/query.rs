//! Read-only views of a project as JSON, for agents and tools: a summary, the object tree of
//! a scene, objects in full or in part, the catalog of logic nodes and components, and name
//! search. The MCP server on files and the editor's live channel both answer through
//! [`answer`], with the same arguments, so the two read alike.
use crate::{
    authoring::{self, ComponentKind},
    collision::Aabb,
    document::*,
    graph::{self, Graph, Node, PortType},
    input_actions,
    scene_view::SceneView,
    spatial, surface,
};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value as Json, json};
use std::collections::{BTreeSet, HashMap, HashSet};

fn kind_name(kind: SceneKind) -> &'static str {
    match kind {
        SceneKind::TwoD => "2d",
        SceneKind::ThreeD => "3d",
    }
}

/// Components present on an object, by their command names.
pub fn components(e: &Entity) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ComponentKind::ALL
        .iter()
        .filter(|c| c.present(e))
        .map(|c| match c {
            ComponentKind::Character => "character",
            ComponentKind::Collider3d => "collider3d",
            ComponentKind::Sensor3d => "sensor3d",
            ComponentKind::Platform => "platform",
            ComponentKind::CameraControl => "camera_control",
            ComponentKind::Collider2d => "collider2d",
            ComponentKind::Sensor2d => "sensor2d",
            ComponentKind::Controller2d => "controller2d",
        })
        .collect();
    if e.camera.is_some() {
        names.push("camera");
    }
    if e.ui.is_some() {
        names.push("ui");
    }
    names
}

/// What an object is, in one word.
pub fn entity_kind(e: &Entity) -> &'static str {
    if e.camera.is_some() {
        "camera"
    } else if e.ui.is_some() {
        "ui"
    } else if e.character3d.is_some() || e.controller.is_some() {
        "character"
    } else if e.has_geometry() {
        "shape"
    } else {
        "group"
    }
}

/// Answers the read request `request` ("summary", "scene_tree", "entity", "find" or
/// "catalog") with the tool's arguments. `scene` is the scene the request defaults to.
pub fn answer(project: &Project, scene: &str, request: &str, args: &Json) -> Result<Json, String> {
    match request {
        "summary" => Ok(project_summary(project)),
        "scene_tree" => scene_tree(project, scene, &TreeQuery::parse(args)?),
        "entity" => entities(project, scene, &EntityQuery::parse(args)?),
        "find" => {
            let text = args["text"].as_str().ok_or("Informe text")?;
            Ok(find(project, text, Page::parse(args, FIND_LIMIT)?))
        }
        "catalog" => catalog_query(args),
        other => Err(format!("Pedido desconhecido: {other}")),
    }
}

/// An optional argument: absent and null give None.
fn arg<T: DeserializeOwned>(args: &Json, key: &str) -> Result<Option<T>, String> {
    match args.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| format!("{key} inválido: {e}")),
    }
}

/// A window over a long list: the first `offset` entries skipped, at most `limit` kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
}
impl Page {
    /// Everything.
    pub const ALL: Self = Self {
        offset: 0,
        limit: usize::MAX,
    };
    /// From the arguments `offset` and `limit`, `limit` being `default` when absent.
    pub fn parse(args: &Json, default: usize) -> Result<Self, String> {
        Ok(Self {
            offset: arg(args, "offset")?.unwrap_or(0),
            limit: arg(args, "limit")?.unwrap_or(default),
        })
    }
    /// The window of `items`, and what the answer says about the whole list: `total`, and
    /// `next_offset` when more entries follow.
    fn cut<T>(self, items: Vec<T>) -> (Vec<T>, Map<String, Json>) {
        let total = items.len();
        let kept: Vec<T> = items
            .into_iter()
            .skip(self.offset)
            .take(self.limit)
            .collect();
        let mut about = Map::new();
        about.insert("total".into(), json!(total));
        let next = self.offset.saturating_add(kept.len());
        if next < total {
            about.insert("next_offset".into(), json!(next));
        }
        (kept, about)
    }
}

/// Uses of an input action or surface listed in the summary; more are only counted.
const USES_LISTED: usize = 10;

/// `used_by` with the first uses, plus `used_by_total` when some are left out.
fn uses_json(entry: &mut Json, uses: Vec<input_actions::Use>) {
    if uses.len() > USES_LISTED {
        entry["used_by_total"] = json!(uses.len());
    }
    entry["used_by"] = json!(uses.into_iter().take(USES_LISTED).collect::<Vec<_>>());
}

pub fn project_summary(project: &Project) -> Json {
    let actions: Vec<Json> = project
        .input_bindings
        .iter()
        .map(|(id, key)| {
            let mut action = json!({
                "id": id,
                "name": input_actions::label(project, id),
                "key": key,
            });
            uses_json(&mut action, input_actions::uses(project, id));
            action
        })
        .collect();
    let surfaces: Vec<Json> = project
        .surfaces
        .iter()
        .map(|s| {
            let mut surface = json!({
                "id": s.id,
                "name": s.name,
                "friction": s.friction,
                "traction": s.traction,
                "speed_multiplier": s.speed_multiplier,
                "conveyor": s.conveyor,
                "conveyor_space": s.conveyor_space,
            });
            uses_json(&mut surface, surface::uses(project, &s.id));
            surface
        })
        .collect();
    // The three groups named by default and the ones the project names; the others read
    // "Grupo N", N being the bit plus 1.
    let groups: Vec<Json> = (0..32_u8)
        .filter(|bit| *bit < 3 || project.collision_groups.contains_key(bit))
        .map(|bit| json!({"bit": bit, "name": project.collision_group_label(bit)}))
        .collect();
    json!({
        "name": project.name,
        "schema_version": project.schema_version,
        "start_scene": project.start_scene,
        "scenes": project.scenes.iter().map(|s| json!({
            "id": s.id,
            "name": s.name,
            "kind": kind_name(s.kind),
            "objects": s.entities.len(),
        })).collect::<Vec<_>>(),
        "assets": project.assets.iter().map(|a| json!({
            "id": a.id,
            "name": a.name,
            "kind": format!("{:?}", a.kind),
        })).collect::<Vec<_>>(),
        "input_actions": actions,
        "surfaces": surfaces,
        "collision_groups": groups,
    })
}

/// The project checked with the rules of saving: `valid`, the `error` that blocks saving when
/// there is one, and `warnings` when the game would ignore something (see
/// [`crate::command::warnings`]).
pub fn validation(project: &Project) -> Json {
    let mut result = match validate_project(project) {
        Ok(()) => json!({"valid": true}),
        Err(error) => json!({"valid": false, "error": error}),
    };
    let warnings: Vec<String> = crate::command::warnings(project)
        .into_iter()
        .map(|w| w.message)
        .collect();
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    result
}

/// What `scene_tree` adds to each object, besides its ID, name, kind, local position, shape,
/// components, logic size, visibility and children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeField {
    /// Where its position (its pivot) is in the world.
    WorldPosition,
    /// Local rotation in degrees, as set_transform takes it.
    Rotation,
    /// Local scale.
    Scale,
    /// Size of its shape.
    Dimensions,
    /// World box of what it and its children show.
    VisualBounds,
    /// World box of the collider the game gives it.
    ColliderBounds,
    /// Both boxes.
    Bounds,
}

/// Objects in a scene tree's answer, its children included, when `limit` is absent.
pub const TREE_LIMIT: usize = 100;
/// Objects found when `limit` is absent.
pub const FIND_LIMIT: usize = 50;

/// Options of `scene_tree`.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeQuery {
    pub fields: BTreeSet<TreeField>,
    /// Lists this object's children instead of the scene's root objects.
    pub root: Option<Id>,
    /// Levels of children shown below the listed objects; deeper children are only counted.
    pub depth: Option<usize>,
    /// The offset is over the listed objects; the limit caps every object in the answer,
    /// children included.
    pub page: Page,
}
impl Default for TreeQuery {
    fn default() -> Self {
        Self {
            fields: BTreeSet::new(),
            root: None,
            depth: None,
            page: Page {
                offset: 0,
                limit: TREE_LIMIT,
            },
        }
    }
}
impl TreeQuery {
    /// From the arguments `fields`, `root`, `depth`, `offset` and `limit`.
    pub fn parse(args: &Json) -> Result<Self, String> {
        Ok(Self {
            fields: arg::<Vec<TreeField>>(args, "fields")?
                .unwrap_or_default()
                .into_iter()
                .collect(),
            root: arg(args, "root")?,
            depth: arg(args, "depth")?,
            page: Page::parse(args, TREE_LIMIT)?,
        })
    }
}

/// A vector worked out through f32 matrices, with the components that are only rounding noise
/// next to the largest one set to 0: under a parent turned 90°, cos 90° is -4.4e-8 in f32, so
/// a child 3 m away would sit 1.3e-7 m off its axis instead of on it.
fn settled(v: Vec3) -> [f32; 3] {
    let noise = v.abs().max_element() * 1e-6;
    v.to_array().map(|c| if c.abs() <= noise { 0. } else { c })
}

/// A world rotation in degrees that reads as set_transform takes it. Two XYZ angle triples
/// give each rotation, such as [0, 120, 0] and [180, 60, 180]; this picks the one nearest to
/// the object's local rotation (`local`, in radians), so a piece turned along with its group
/// does not read as upside down. Angles are in (-180, 180], a half turn reading 180.
fn world_degrees(rotation: Quat, local: [f32; 3]) -> [f32; 3] {
    let wrap = |degrees: f64| {
        let wrapped = degrees - 360. * ((degrees - 180.) / 360.).ceil();
        if wrapped < -180. + 1e-4 {
            180.
        } else {
            wrapped
        }
    };
    let (x, y, z) = rotation.as_dquat().to_euler(EulerRot::XYZ);
    let [x, y, z] = [x, y, z].map(f64::to_degrees);
    let local = local.map(|angle| f64::from(angle).to_degrees());
    let distance = |angles: &[f64; 3]| -> f64 {
        angles
            .iter()
            .zip(&local)
            .map(|(angle, near)| wrap(angle - near).abs())
            .sum()
    };
    let [first, second] =
        [[x, y, z], [x + 180., 180. - y, z + 180.]].map(|angles| angles.map(wrap));
    let nearest = if distance(&second) < distance(&first) {
        second
    } else {
        first
    };
    settled(Vec3::from(nearest.map(|angle| angle as f32)))
}

fn bounds_json(bounds: Aabb) -> Json {
    json!({"min": settled(bounds.min), "max": settled(bounds.max)})
}

/// Where an object's position (its pivot) is in the world: for a root object, its position.
fn world_position(view: &SceneView, e: &Entity) -> Option<[f32; 3]> {
    let world = view.world_matrix(&e.id).ok().filter(|m| m.is_finite())?;
    Some(settled(
        world.transform_point3(Vec3::from(e.transform.pivot)),
    ))
}

/// The collider the game gives an object, in the physics world's order: the 3D character's
/// standing body, its 3D collider or its box collider. A world box with its center, marked
/// when it is a sensor or disabled.
fn collider_box(view: &SceneView, e: &Entity) -> Option<Json> {
    let world = view.world_matrix(&e.id).ok()?;
    let body = e
        .character3d
        .as_ref()
        .and_then(|c| c.body.collider(false, c.crouch_height).ok());
    let (bounds, center, sensor, enabled) =
        if let Some(collider) = body.as_ref().or(e.physics3d.as_ref()) {
            let (bounds, center) = spatial::collider3d_bounds(collider, world).ok()?;
            (bounds, center, collider.sensor, collider.enabled)
        } else {
            let collider = e.collider.as_ref()?;
            let bounds = spatial::bounds_from_world(collider, world).ok()?;
            let center = (bounds.min + bounds.max) * 0.5;
            (bounds, center, collider.is_trigger, collider.enabled)
        };
    let mut result = bounds_json(bounds);
    result["center"] = json!(settled(center));
    if sensor {
        result["sensor"] = json!(true);
    }
    if !enabled {
        result["enabled"] = json!(false);
    }
    Some(result)
}

/// Object tree of a scene: each node has ID, name, kind, local position, shape, components,
/// logic size, the fields asked for and its children. The answer holds at most `limit`
/// objects: children that no longer fit are only counted, and then `truncated` is true.
pub fn scene_tree(project: &Project, scene: &str, query: &TreeQuery) -> Result<Json, String> {
    let scene = project
        .scene(scene)
        .ok_or_else(|| format!("Cena {scene} não existe"))?;
    let root = match &query.root {
        Some(id) => Some(
            scene
                .entity(id)
                .ok_or_else(|| format!("Objeto {id} não existe nesta cena"))?,
        ),
        None => None,
    };
    let mut children: HashMap<Option<&str>, Vec<&Entity>> = HashMap::new();
    for e in &scene.entities {
        children.entry(e.parent.as_deref()).or_default().push(e);
    }
    let mut tree = Tree {
        view: SceneView::new(scene),
        children,
        query,
        expanded: HashSet::new(),
    };
    let listed = tree.children_of(root.map(|e| e.id.as_str()));
    let (listed, mut about) = query.page.cut(listed);
    let (expanded, truncated) = tree.expand(&listed);
    tree.expanded = expanded;
    if truncated {
        about.insert("truncated".into(), json!(true));
    }
    let mut result = json!({
        "id": scene.id,
        "name": scene.name,
        "kind": kind_name(scene.kind),
        "objects": listed.into_iter().map(|e| tree.node(e)).collect::<Vec<_>>(),
    });
    if let Some(root) = root {
        result["root"] = json!({"id": root.id, "name": root.name});
    }
    result.as_object_mut().unwrap().extend(about);
    Ok(result)
}

struct Tree<'a> {
    view: SceneView<'a>,
    children: HashMap<Option<&'a str>, Vec<&'a Entity>>,
    query: &'a TreeQuery,
    /// Objects whose children the answer shows; the others only count them.
    expanded: HashSet<&'a str>,
}
impl<'a> Tree<'a> {
    fn children_of(&self, parent: Option<&str>) -> Vec<&'a Entity> {
        self.children.get(&parent).cloned().unwrap_or_default()
    }
    /// The objects whose children the answer shows, level by level from the listed ones, as
    /// long as all of an object's children fit in the limit with what is already shown and
    /// lie within `depth`. True when the limit, not the depth, left children out.
    fn expand(&self, listed: &[&'a Entity]) -> (HashSet<&'a str>, bool) {
        let mut room = self.query.page.limit.saturating_sub(listed.len());
        let mut expanded = HashSet::new();
        let mut truncated = false;
        let mut level = listed.to_vec();
        let mut depth = 0;
        while !level.is_empty() && self.query.depth.is_none_or(|d| depth < d) {
            let mut next = Vec::new();
            for e in level {
                let kids = self.children_of(Some(&e.id));
                if kids.is_empty() {
                    continue;
                }
                if kids.len() > room {
                    truncated = true;
                    continue;
                }
                room -= kids.len();
                expanded.insert(e.id.as_str());
                next.extend(kids);
            }
            level = next;
            depth += 1;
        }
        (expanded, truncated)
    }
    fn node(&self, e: &Entity) -> Json {
        let mut node = json!({
            "id": e.id,
            "name": e.name,
            "kind": entity_kind(e),
            "position": e.transform.position,
        });
        if let Some(shape) = e.primitive {
            node["shape"] = json!(shape);
        }
        let components = components(e);
        if !components.is_empty() {
            node["components"] = json!(components);
        }
        if !e.graph.nodes.is_empty() {
            node["logic_nodes"] = json!(e.graph.nodes.len());
        }
        if !e.visible {
            node["visible"] = json!(false);
        }
        let fields = &self.query.fields;
        let wants = |field: TreeField| {
            fields.contains(&field)
                || (fields.contains(&TreeField::Bounds)
                    && matches!(field, TreeField::VisualBounds | TreeField::ColliderBounds))
        };
        let extras = [
            (TreeField::WorldPosition, "world_position"),
            (TreeField::Rotation, "rotation"),
            (TreeField::Scale, "scale"),
            (TreeField::Dimensions, "dimensions"),
            (TreeField::VisualBounds, "visual_bounds"),
            (TreeField::ColliderBounds, "collider_bounds"),
        ];
        for (field, key) in extras {
            if !wants(field) {
                continue;
            }
            let value = match field {
                TreeField::WorldPosition => world_position(&self.view, e).map(|p| json!(p)),
                TreeField::Rotation => Some(json!(e.transform.rotation.map(f32::to_degrees))),
                TreeField::Scale => Some(json!(e.transform.scale)),
                TreeField::Dimensions => e.primitive.is_some().then(|| json!(e.dimensions)),
                TreeField::VisualBounds => {
                    spatial::shown_bounds(&self.view, &e.id).map(bounds_json)
                }
                TreeField::ColliderBounds | TreeField::Bounds => collider_box(&self.view, e),
            };
            if let Some(value) = value {
                node[key] = value;
            }
        }
        let kids = self.children_of(Some(&e.id));
        if !kids.is_empty() {
            if self.expanded.contains(e.id.as_str()) {
                node["children"] = json!(
                    kids.into_iter()
                        .map(|kid| self.node(kid))
                        .collect::<Vec<_>>()
                );
            } else {
                node["children_count"] = json!(kids.len());
            }
        }
        node
    }
}

/// How much of a long part of an object `entities` returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    #[default]
    Summary,
    Full,
}

/// Options of `entities`.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityQuery {
    pub ids: Vec<Id>,
    /// Only these parts, besides ID and name: top-level keys or dotted paths such as
    /// "character3d.actions" (numbers index lists).
    pub fields: Vec<String>,
    pub graph: Detail,
    pub geometry: Detail,
    /// Asked for with `ids`: the answer lists objects even when there is one.
    pub list: bool,
}
impl EntityQuery {
    /// From the arguments `id` and/or `ids`, `fields`, `graph` and `geometry`.
    pub fn parse(args: &Json) -> Result<Self, String> {
        let one: Option<Id> = arg(args, "id")?;
        let many: Option<Vec<Id>> = arg(args, "ids")?;
        let list = many.is_some();
        let ids: Vec<Id> = one.into_iter().chain(many.unwrap_or_default()).collect();
        if ids.is_empty() {
            return Err("Informe id ou ids".into());
        }
        let fields: Vec<String> = arg(args, "fields")?.unwrap_or_default();
        for field in &fields {
            let top = field.split('.').next().unwrap_or_default();
            if !ENTITY_FIELDS.contains(&top) {
                return Err(format!(
                    "Campo desconhecido: {field}. Campos: {}",
                    ENTITY_FIELDS.join(", ")
                ));
            }
        }
        Ok(Self {
            ids,
            fields,
            graph: arg(args, "graph")?.unwrap_or_default(),
            geometry: arg(args, "geometry")?.unwrap_or_default(),
            list,
        })
    }
}

/// Top-level parts of `entities`' answer: the object's document form, then what it adds.
pub const ENTITY_FIELDS: [&str; 30] = [
    "id",
    "name",
    "parent",
    "transform",
    "primitive",
    "mesh",
    "primitive_parameters",
    "dimensions",
    "segments",
    "material",
    "visible",
    "layer",
    "attributes",
    "collider",
    "physics3d",
    "character3d",
    "camera_rig",
    "platform",
    "controller",
    "camera",
    "ui",
    "graph",
    "clips",
    "model_source",
    "rotation_degrees",
    "world",
    "visual_bounds",
    "collider_bounds",
    "can_add_components",
    "components",
];

/// One object (`id`) or several (`ids`, listed under `objects`), as `query` asks.
pub fn entities(project: &Project, scene: &str, query: &EntityQuery) -> Result<Json, String> {
    let scene = project
        .scene(scene)
        .ok_or_else(|| format!("Cena {scene} não existe"))?;
    let view = SceneView::new(scene);
    let mut objects = query
        .ids
        .iter()
        .map(|id| entity_view(project, scene, &view, id, query))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if query.list {
        json!({"objects": objects})
    } else {
        objects.remove(0)
    })
}

/// One object as `entities` returns it by default.
pub fn entity(project: &Project, scene: &str, id: &str) -> Result<Json, String> {
    entities(
        project,
        scene,
        &EntityQuery {
            ids: vec![id.to_owned()],
            fields: Vec::new(),
            graph: Detail::Summary,
            geometry: Detail::Summary,
            list: false,
        },
    )
}

/// An object's document form, without its empty (null) parts, with its logic and geometry
/// summarized unless asked in full, plus where it is in the world, its boxes and the
/// components it can still receive; cut down to `query.fields` when given.
fn entity_view(
    project: &Project,
    scene: &Scene,
    view: &SceneView,
    id: &str,
    query: &EntityQuery,
) -> Result<Json, String> {
    let e = scene
        .entity(id)
        .ok_or_else(|| format!("Objeto {id} não existe nesta cena"))?;
    let mut value = serde_json::to_value(e).map_err(|e| e.to_string())?;
    let map = value
        .as_object_mut()
        .ok_or("Objeto sem forma de documento")?;
    map.retain(|_, part| !part.is_null());
    if query.graph == Detail::Summary {
        map.insert("graph".into(), graph_summary(project, &e.graph));
    }
    if query.geometry == Detail::Summary {
        for part in map.values_mut() {
            summarize_geometry(part);
        }
    }
    map.insert(
        "rotation_degrees".into(),
        json!(e.transform.rotation.map(f32::to_degrees)),
    );
    if let Ok(world) = view.world_matrix(id)
        && world.is_finite()
    {
        // A root object's world rotation and scale are its own, read as they were set.
        let (degrees, scale) = if e.parent.is_none() {
            (e.transform.rotation.map(f32::to_degrees), e.transform.scale)
        } else {
            let (scale, rotation, _) = world.to_scale_rotation_translation();
            (
                world_degrees(rotation, e.transform.rotation),
                scale.to_array(),
            )
        };
        map.insert(
            "world".into(),
            json!({
                "position": settled(world.transform_point3(Vec3::from(e.transform.pivot))),
                "rotation_degrees": degrees,
                "scale": scale,
            }),
        );
    }
    if let Some(bounds) = spatial::shown_bounds(view, id) {
        map.insert("visual_bounds".into(), bounds_json(bounds));
    }
    if let Some(collider) = collider_box(view, e) {
        map.insert("collider_bounds".into(), collider);
    }
    map.insert("components".into(), json!(components(e)));
    map.insert(
        "can_add_components".into(),
        json!(authoring::available(scene.kind, e)),
    );
    if query.fields.is_empty() {
        return Ok(value);
    }
    let mut picked = json!({"id": e.id, "name": e.name});
    for field in &query.fields {
        let path: Vec<&str> = field.split('.').collect();
        let part = path
            .iter()
            .try_fold(&value, |part, key| match part {
                Json::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)),
                _ => part.get(key),
            })
            .cloned()
            .unwrap_or(Json::Null);
        place(&mut picked, &path, part);
    }
    Ok(picked)
}

/// Puts `part` at `path` inside `into`, creating the objects on the way; a path inside a part
/// already placed whole changes nothing.
fn place(into: &mut Json, path: &[&str], part: Json) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = into;
    for key in parents {
        let Some(map) = current.as_object_mut() else {
            return;
        };
        current = map.entry(*key).or_insert_with(|| json!({}));
        if !current.is_object() {
            return;
        }
    }
    if let Some(map) = current.as_object_mut() {
        map.insert((*last).to_owned(), part);
    }
}

/// Mesh and collision geometry as counts of vertices, edges, faces and triangles: their
/// lists are long and an agent rarely needs them.
fn summarize_geometry(part: &mut Json) {
    match part {
        Json::Object(map) => {
            let count = |key: &str| map.get(key).and_then(Json::as_array).map(Vec::len);
            if count("vertices").is_some() && (count("faces").or(count("triangles"))).is_some() {
                *map = ["vertices", "edges", "faces", "triangles"]
                    .into_iter()
                    .filter_map(|key| count(key).map(|n| (key.to_owned(), json!(n))))
                    .collect();
                return;
            }
            map.values_mut().for_each(summarize_geometry);
        }
        Json::Array(items) => items.iter_mut().for_each(summarize_geometry),
        _ => {}
    }
}

/// A node in a few characters: its operation and, in parentheses, its first non-empty text
/// parameter (the name of the scene or asset it names, shortened), or its only parameter.
fn node_label(project: &Project, node: &Node) -> String {
    let Some(definition) = graph::registry().iter().find(|d| d.id == node.operation) else {
        return node.operation.clone();
    };
    let shown = |value: &Value| match value {
        Value::Text(text) if !text.is_empty() => {
            let named = project
                .scene(text)
                .map(|s| s.name.as_str())
                .or_else(|| project.asset(text).map(|a| a.name.as_str()))
                .unwrap_or(text.as_str());
            Some(if named.chars().count() > 24 {
                format!("{}…", named.chars().take(23).collect::<String>())
            } else {
                named.to_owned()
            })
        }
        _ => None,
    };
    let detail = definition
        .params
        .iter()
        .find_map(|p| node.params.get(p.id).and_then(&shown))
        .or_else(|| match definition.params.as_slice() {
            [only] => match node.params.get(only.id) {
                Some(Value::Number(n)) => Some(n.to_string()),
                Some(Value::Bool(b)) => Some(b.to_string()),
                _ => None,
            },
            _ => None,
        });
    match detail {
        Some(detail) => format!("{}({detail})", node.operation),
        None => node.operation.clone(),
    }
}

/// A logic graph in few lines: `nodes` maps each node's label (its operation and main
/// parameter) to its ID and the parameters that differ from the operation's defaults, and
/// `edges` are text, such as "event.input(reiniciar).exec -> character.teleport.exec". A
/// label repeated in the graph ends in #N, N being the node's index in the stored graph.
pub fn graph_summary(project: &Project, graph: &Graph) -> Json {
    let base: Vec<String> = graph
        .nodes
        .iter()
        .map(|node| node_label(project, node))
        .collect();
    let mut repeated: HashMap<&str, usize> = HashMap::new();
    for label in &base {
        *repeated.entry(label).or_default() += 1;
    }
    let labels: Vec<String> = base
        .iter()
        .enumerate()
        .map(|(i, label)| {
            if repeated[label.as_str()] > 1 {
                format!("{label}#{i}")
            } else {
                label.clone()
            }
        })
        .collect();
    // An edge to a node that is not there shows its raw ID.
    let label_of = |id: &str| {
        graph
            .nodes
            .iter()
            .position(|node| node.id == id)
            .map_or_else(|| id.to_owned(), |i| labels[i].clone())
    };
    let nodes: Map<String, Json> = graph
        .nodes
        .iter()
        .zip(&labels)
        .map(|(node, label)| {
            let defaults = graph::registry()
                .iter()
                .find(|d| d.id == node.operation)
                .map(|d| d.params.as_slice())
                .unwrap_or_default();
            let changed: Map<String, Json> = node
                .params
                .iter()
                .filter(|(key, value)| {
                    defaults
                        .iter()
                        .find(|p| p.id == key.as_str())
                        .is_none_or(|p| p.default != **value)
                })
                .map(|(key, value)| (key.clone(), json!(value)))
                .collect();
            let mut entry = json!({"id": node.id});
            if !changed.is_empty() {
                entry["params"] = Json::Object(changed);
            }
            (label.clone(), entry)
        })
        .collect();
    let edges: Vec<String> = graph
        .edges
        .iter()
        .map(|edge| {
            format!(
                "{}.{} -> {}.{}",
                label_of(&edge.from_node),
                edge.from_port,
                label_of(&edge.to_node),
                edge.to_port
            )
        })
        .collect();
    json!({"nodes": nodes, "edges": edges})
}

/// Objects whose name contains `text` (case-insensitive), in every scene, as `objects`, with
/// `total` and, when more follow, `next_offset`.
pub fn find(project: &Project, text: &str, page: Page) -> Json {
    let needle = text.to_lowercase();
    let found: Vec<Json> = project
        .scenes
        .iter()
        .flat_map(|s| s.entities.iter().map(move |e| (s, e)))
        .filter(|(_, e)| e.name.to_lowercase().contains(&needle))
        .map(|(s, e)| {
            json!({
                "scene": s.id,
                "scene_name": s.name,
                "id": e.id,
                "name": e.name,
                "kind": entity_kind(e),
            })
        })
        .collect();
    let (objects, about) = page.cut(found);
    let mut result = Map::new();
    result.insert("objects".into(), json!(objects));
    result.extend(about);
    Json::Object(result)
}

fn port_type(kind: PortType) -> &'static str {
    match kind {
        PortType::Exec => "exec",
        PortType::Number => "number",
        PortType::Text => "text",
        PortType::Bool => "bool",
        PortType::Object => "object",
        PortType::Vector2 => "vector2",
        PortType::Vector3 => "vector3",
        PortType::Surface => "surface",
        PortType::Any => "any",
    }
}

/// Logic nodes (with ports, parameters and defaults), components and object kinds.
pub fn catalog() -> Json {
    let nodes: Vec<Json> = graph::registry()
        .iter()
        .map(|op| {
            json!({
                "operation": op.id,
                "label": op.label,
                "category": op.category,
                "description": graph::description(op.id),
                "inputs": op.inputs.iter().map(|p| json!({
                    "id": p.id, "label": p.label, "type": port_type(p.kind),
                })).collect::<Vec<_>>(),
                "outputs": op.outputs.iter().map(|p| json!({
                    "id": p.id, "label": p.label, "type": port_type(p.kind),
                })).collect::<Vec<_>>(),
                "params": op.params.iter().map(|p| {
                    let mut param = json!({"id": p.id, "label": p.label, "default": p.default});
                    if let Some(choices) = graph::choices(op.id, p.id) {
                        param["choices"] = choices
                            .iter()
                            .map(|(value, label)| json!({"value": value, "label": label}))
                            .collect();
                    }
                    if let Some(reference) = graph::reference(op.id, p) {
                        param["ref"] = json!(reference);
                    }
                    param
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    let components: Vec<Json> = ComponentKind::ALL
        .iter()
        .map(|c| {
            json!({
                "component": c,
                "label": c.label(),
                "scene": kind_name(c.scene_kind()),
            })
        })
        .collect();
    json!({
        "nodes": nodes,
        "components": components,
        "shapes": {
            "2d": ["Rectangle", "Circle", "Sprite"],
            "3d": ["Cube", "Sphere", "Cylinder", "Plane", "Pyramid", "Cone", "Tube"],
        },
        "ui_elements": ["Text", "Image", "Button", "Bar"],
        "presets": presets(),
        "movement_profiles": movement_profiles(),
        "surface_presets": ["common", "ice", "mud", "conveyor"],
        "input_keys": input_actions::keys().collect::<Vec<_>>(),
        "value_format": "Attribute and node parameter values are tagged: {\"Number\": 1.0}, {\"Text\": \"...\"}, {\"Bool\": true}, {\"Object\": \"<id>\" or null}, {\"Vector2\": [x, y]}, {\"Vector3\": [x, y, z]}, {\"Surface\": \"<id>\" or null}.",
    })
}

/// The assemblies of create_entity's `{type: preset}`: what each is for, the pieces it creates
/// (built for real, so the list follows the presets; in the order the command answers them,
/// "$N.M" being piece M) and its options.
fn presets() -> Json {
    use crate::movement_presets::{self, MovementPreset};
    let camera = json!({"camera": {
        "default": "create",
        "choices": [
            {"value": "create", "label": "Cria a Câmera principal, que segue o jogador; ela fica ativa se a cena não tiver outra câmera ativa"},
            {"value": "reuse", "label": "A câmera ativa da cena (ou a primeira) passa a seguir o jogador, e a Câmera principal não é criada; recusado se a cena não tem câmera ou se ela já segue outro objeto"},
            {"value": "none", "label": "Nenhuma câmera segue o jogador, e a Câmera principal não é criada"},
        ],
    }});
    [
        (
            "first_person",
            MovementPreset::FirstPerson,
            "Jogador 3D em primeira pessoa: personagem com corpo de movimento e as ações de andar, pular, correr e agachar, uma aparência editável que a câmera esconde em primeira pessoa e uma câmera na altura dos olhos.",
            camera.clone(),
        ),
        (
            "third_person",
            MovementPreset::ThirdPerson,
            "Jogador 3D em terceira pessoa: o mesmo personagem, com a câmera atrás dele, a uma distância que encurta diante de paredes.",
            camera,
        ),
        (
            "platform",
            MovementPreset::Platform,
            "Plataforma móvel (não é um personagem): um bloco sólido de 3 × 0,4 × 3 m que vai e volta sem parar a partir de onde é posto, levando junto o que estiver em cima. O percurso é uma animação comum, editável com set_clip; set_transform move a plataforma com o percurso.",
            json!({
                "travel": {"default": [4., 0., 0.], "description": "Quanto a plataforma anda a partir de onde é posta, em metros, antes de voltar."},
                "period": {"default": 4., "description": "Segundos da ida e da volta."},
            }),
        ),
    ]
    .into_iter()
    .map(|(id, preset, description, params)| {
        let mut project = crate::editing::blank_project("Catálogo", SceneKind::ThreeD)
            .expect("project name is not empty");
        let scene = project.start_scene.clone();
        let pieces = movement_presets::create(&mut project, &scene, preset, Vec3::ZERO)
            .map(|_| project.scenes[0].entities.as_slice())
            .unwrap_or_default();
        let name = |id: &str| pieces.iter().find(|e| e.id == id).map(|e| e.name.as_str());
        let creates: Vec<String> = pieces
            .iter()
            .map(|e| {
                // The shape and the components; a piece with neither groups others.
                let mut what: Vec<String> =
                    e.primitive.map(|shape| format!("{shape:?}")).into_iter().collect();
                what.extend(components(e).into_iter().map(str::to_owned));
                if what.is_empty() {
                    what.push("group".into());
                }
                let mut text = format!("{} ({})", e.name, what.join(", "));
                if let Some(parent) = e.parent.as_deref().and_then(name) {
                    text.push_str(&format!(", em {parent}"));
                }
                for clip in &e.clips {
                    text.push_str(&format!(", animação {}", clip.name));
                }
                text
            })
            .collect();
        json!({"id": id, "description": description, "creates": creates, "params": params})
    })
    .collect()
}

/// The movement profiles apply_movement_profile and the `character.profile` node write, with
/// the values they set.
fn movement_profiles() -> Json {
    use crate::character::{CharacterConfig, MovementProfile, PROFILE_FIELDS};
    MovementProfile::ALL
        .into_iter()
        .map(|profile| {
            let mut config = CharacterConfig::default();
            config.apply_profile(profile);
            let config = serde_json::to_value(config).unwrap_or_default();
            let values: Map<String, Json> = PROFILE_FIELDS
                .iter()
                .map(|field| ((*field).to_owned(), config[*field].clone()))
                .collect();
            json!({"id": profile.id(), "label": profile.label(), "values": values})
        })
        .collect()
}

/// Options of `catalog_with`.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogQuery {
    /// Only nodes of this category (case-insensitive).
    pub category: Option<String>,
    /// Only these operations; an entry ending in "." (or ".*") takes every operation that
    /// starts with it.
    pub operations: Vec<String>,
    /// Nodes as operation, label, category and description only. When absent, true unless
    /// category or operations is given.
    pub brief: Option<bool>,
    /// Over the nodes.
    pub page: Page,
}
impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            category: None,
            operations: Vec::new(),
            brief: None,
            page: Page::ALL,
        }
    }
}
impl CatalogQuery {
    /// From the arguments `category`, `operations`, `brief`, `offset` and `limit`.
    pub fn parse(args: &Json) -> Result<Self, String> {
        Ok(Self {
            category: arg(args, "category")?,
            operations: arg(args, "operations")?.unwrap_or_default(),
            brief: arg(args, "brief")?,
            page: Page::parse(args, usize::MAX)?,
        })
    }
}

/// The catalog as the tool's arguments ask.
pub fn catalog_query(args: &Json) -> Result<Json, String> {
    catalog_with(&CatalogQuery::parse(args)?)
}

/// The catalog cut down by `query`. Without a filter, everything with brief nodes and the
/// list of categories; with category or operations, only the nodes asked for, in full.
pub fn catalog_with(query: &CatalogQuery) -> Result<Json, String> {
    let Json::Object(mut sections) = catalog() else {
        return Err("Catálogo inválido".into());
    };
    let Some(Json::Array(mut nodes)) = sections.remove("nodes") else {
        return Err("Catálogo sem nós".into());
    };
    let text = |node: &Json, key: &str| node[key].as_str().unwrap_or_default().to_owned();
    let mut categories: Vec<String> = Vec::new();
    for node in &nodes {
        let category = text(node, "category");
        if !categories.contains(&category) {
            categories.push(category);
        }
    }
    let category = match &query.category {
        Some(asked) => Some(
            categories
                .iter()
                .find(|c| c.to_lowercase() == asked.to_lowercase())
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "Categoria desconhecida: {asked}. Categorias: {}",
                        categories.join(", ")
                    )
                })?,
        ),
        None => None,
    };
    let in_category = |node: &Json| {
        category
            .as_ref()
            .is_none_or(|c| text(node, "category") == *c)
    };
    if !query.operations.is_empty() {
        // Looked up in every category first, so an operation of another category is not
        // reported as unknown.
        let mut wanted = Vec::new();
        for asked in &query.operations {
            let prefix = asked.strip_suffix('*').unwrap_or(asked);
            let matches = |node: &&Json| {
                let operation = text(node, "operation");
                if prefix.ends_with('.') {
                    operation.starts_with(prefix)
                } else {
                    operation == *asked
                }
            };
            let found: Vec<&Json> = nodes.iter().filter(matches).collect();
            let Some(first) = found.first() else {
                return Err(if prefix.ends_with('.') {
                    format!("Nenhuma operação começa com {prefix}")
                } else {
                    format!("Operação desconhecida: {asked}")
                });
            };
            let kept: Vec<String> = found
                .iter()
                .filter(|n| in_category(n))
                .map(|n| text(n, "operation"))
                .collect();
            if let (true, Some(category)) = (kept.is_empty(), &category) {
                return Err(if prefix.ends_with('.') {
                    format!("Nenhuma operação que começa com {prefix} é da categoria {category}")
                } else {
                    format!(
                        "{asked} não é da categoria {category}, e sim de {}",
                        text(first, "category")
                    )
                });
            }
            wanted.extend(kept);
        }
        nodes.retain(|node| wanted.contains(&text(node, "operation")));
    }
    nodes.retain(in_category);
    let filtered = query.category.is_some() || !query.operations.is_empty();
    if query.brief.unwrap_or(!filtered) {
        for node in &mut nodes {
            if let Json::Object(map) = node {
                map.retain(|key, _| {
                    matches!(
                        key.as_str(),
                        "operation" | "label" | "category" | "description"
                    )
                });
            }
        }
    }
    let (nodes, about) = query.page.cut(nodes);
    let mut result = if filtered {
        Map::new()
    } else {
        sections.insert("categories".into(), json!(categories));
        sections
    };
    result.insert("nodes".into(), Json::Array(nodes));
    result.extend(about);
    Ok(Json::Object(result))
}
