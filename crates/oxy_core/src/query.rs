//! Read-only views of a project as JSON, for agents and tools: a summary, the object tree of
//! a scene, objects in full or in part, the catalog of logic nodes and components, and name
//! search. The MCP server on files and the editor's live channel both answer through
//! [`answer`], with the same arguments, so the two read alike.
use crate::{
    authoring::{self, ComponentKind},
    collision::Aabb,
    command,
    document::*,
    graph::{self, Graph, Node, OperationDef, ParamDef, PortType, Reference},
    input_actions,
    scene_view::SceneView,
    spatial, surface, variables,
};
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value as Json, json};
use std::collections::{BTreeSet, HashMap, HashSet};

fn kind_name(kind: SceneKind) -> &'static str {
    match kind {
        SceneKind::TwoD => "2d",
        SceneKind::ThreeD => "3d",
    }
}

/// What a scene shows behind its objects, as set_scene_background takes it: "sky" or
/// {"color": [r, g, b, a]}, the dark color for a scene without a background of its own.
fn background(scene: &Scene) -> Json {
    json!(crate::command::BackgroundSpec::shown(
        scene.shown_background()
    ))
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
        "summary" => match ProjectQuery::parse(args)? {
            ProjectQuery {
                model: Some(model),
                page,
            } => model_parts(project, &model, page),
            _ => Ok(project_summary(project)),
        },
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

/// Uses of an input action, surface, game variable or asset listed in the summary; more are
/// only counted.
const USES_LISTED: usize = 10;

/// `used_by` with the first uses, plus `used_by_total` when some are left out.
fn uses_json(entry: &mut Json, uses: Vec<input_actions::Use>) {
    if uses.len() > USES_LISTED {
        entry["used_by_total"] = json!(uses.len());
    }
    entry["used_by"] = json!(uses.into_iter().take(USES_LISTED).collect::<Vec<_>>());
}

pub fn project_summary(project: &Project) -> Json {
    let shared = input_actions::shared_keys(project);
    let actions: Vec<Json> = project
        .input_bindings
        .iter()
        .map(|(id, key)| {
            let mut action = json!({
                "id": id,
                "name": input_actions::label(project, id),
                "key": key,
            });
            // The other actions on its key: the game fires them together.
            if let Some(others) = shared.values().find(|ids| ids.contains(&id)) {
                action["shares_key_with"] = others
                    .iter()
                    .filter(|other| **other != id)
                    .map(|other| json!({"id": other, "name": input_actions::label(project, other)}))
                    .collect();
            }
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
    let variables: Vec<Json> = project
        .variables
        .iter()
        .map(|v| {
            let mut variable = json!({
                "id": v.id,
                "name": v.name,
                "type": variables::type_name(&v.value),
                "value": v.value,
                "persistent": v.persistent,
            });
            uses_json(&mut variable, variables::uses(project, &v.id));
            variable
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
            "background": background(s),
        })).collect::<Vec<_>>(),
        "assets": project.assets.iter().map(|a| {
            let mut asset = json!({
                "id": a.id,
                "name": a.name,
                "kind": format!("{:?}", a.kind),
            });
            // Models live in the project file; images and sounds in the project folder.
            if !a.path.is_empty() {
                asset["path"] = json!(a.path);
            }
            if let Some(pieces) = &a.model {
                asset["part_count"] = json!(pieces.len());
                // The root a placement gets, by the name place_model gives it.
                if let [root] = model_roots(pieces).as_slice() {
                    asset["root"] = json!({"id": root.id, "name": root.name});
                }
            }
            uses_json(&mut asset, crate::editing::asset_uses(project, &a.id));
            asset
        }).collect::<Vec<_>>(),
        "input_actions": actions,
        "surfaces": surfaces,
        "variables": variables,
        "collision_groups": groups,
        "game": {
            "title": project.game_title(),
            "window": project.game.window,
            "fullscreen": project.game.fullscreen,
        },
    })
}

/// What the summary request asks: the whole summary, or one model's parts (`model`, by its ID
/// or name), `offset` and `limit` paging them (default [`TREE_LIMIT`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectQuery {
    pub model: Option<String>,
    pub page: Page,
}
impl ProjectQuery {
    pub fn parse(args: &Json) -> Result<Self, String> {
        Ok(Self {
            model: arg(args, "model")?,
            page: Page::parse(args, TREE_LIMIT)?,
        })
    }
}

/// The pieces of a model without a parent in it: one for the models save_model makes.
fn model_roots(pieces: &[Entity]) -> Vec<&Entity> {
    pieces
        .iter()
        .filter(|e| {
            e.parent
                .as_ref()
                .is_none_or(|p| !pieces.iter().any(|o| o.id == *p))
        })
        .collect()
}

/// The model `model` (its ID or name, as place_model takes it) and its parts: {id, name, root?,
/// parts: [{id, name, parent, kind}], total, next_offset?}: a page of the parts, each parent
/// before its children (a root's parent null). Their IDs, as the model keeps them, are the ones
/// `"$N@<id>"` takes after a place_model.
pub fn model_parts(project: &Project, model: &str, page: Page) -> Result<Json, String> {
    let id = command::find_model(project, model)?;
    let asset = project.asset(&id).expect("found");
    let pieces = asset.model.as_deref().unwrap_or_default();
    let mut ordered: Vec<&Entity> = Vec::with_capacity(pieces.len());
    let mut pending = model_roots(pieces);
    pending.reverse();
    while let Some(e) = pending.pop() {
        ordered.push(e);
        let children = pieces
            .iter()
            .rev()
            .filter(|child| child.parent.as_deref() == Some(e.id.as_str()));
        pending.extend(children);
    }
    let parts: Vec<Json> = ordered
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "name": e.name,
                "parent": e.parent.as_ref().filter(|p| pieces.iter().any(|o| o.id == **p)),
                "kind": entity_kind(e),
            })
        })
        .collect();
    let (parts, about) = page.cut(parts);
    let mut answer = json!({"id": asset.id, "name": asset.name});
    if let [root] = model_roots(pieces).as_slice() {
        answer["root"] = json!({"id": root.id, "name": root.name});
    }
    answer["parts"] = json!(parts);
    answer.as_object_mut().unwrap().extend(about);
    Ok(answer)
}

/// The project checked with the rules of saving: `valid`, the `error` that blocks saving when
/// there is one, and `warnings` when the game would ignore or stop on something (see
/// [`crate::command::warnings`]).
pub fn validation(project: &Project) -> Json {
    validation_in(project, None)
}

/// [`validation`] of the project saved in the folder `root`, whose warnings also name the
/// scripts its objects attach that the folder lacks (see `persistence::script_warnings`).
pub fn validation_in(project: &Project, root: Option<&std::path::Path>) -> Json {
    let mut result = match validate_project(project) {
        Ok(()) => json!({"valid": true}),
        Err(error) => json!({"valid": false, "error": error}),
    };
    let mut warnings: Vec<String> = crate::command::warnings(project)
        .into_iter()
        .map(|w| w.message)
        .collect();
    if let Some(root) = root {
        warnings.extend(crate::persistence::script_warnings(project, root));
    }
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
    /// Lists this object's children (by ID or name) instead of the scene's root objects.
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
pub(crate) fn world_degrees(rotation: Quat, local: [f32; 3]) -> [f32; 3] {
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
    let scene = project.find_scene(scene, "scene")?;
    let root = match &query.root {
        Some(text) => Some(scene.find_entity(text, "root")?),
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
        "background": background(scene),
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
    /// Objects of the scene, each by ID or name (see [`Scene::find_entity`]).
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
        let mut fields: Vec<String> = arg(args, "fields")?.unwrap_or_default();
        for field in &mut fields {
            let (top, rest) = field.split_once('.').unwrap_or((field.as_str(), ""));
            if let Some((_, part)) = FIELD_ALIASES.iter().find(|(alias, _)| *alias == top) {
                *field = if rest.is_empty() {
                    (*part).to_owned()
                } else {
                    format!("{part}.{rest}")
                };
                continue;
            }
            if !ENTITY_FIELDS.contains(&top) {
                return Err(format!(
                    "Campo desconhecido: {field}. Campos: {}; os nomes dos componentes também \
                     valem, pela parte que os guarda ({})",
                    ENTITY_FIELDS.join(", "),
                    FIELD_ALIASES
                        .iter()
                        .map(|(alias, part)| format!("{alias} → {part}"))
                        .collect::<Vec<_>>()
                        .join(", ")
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

/// Component names, as `oxy_scene` and the catalog give them, that `fields` takes for the part
/// of the document holding the component.
pub const FIELD_ALIASES: [(&str, &str); 7] = [
    ("collider3d", "physics3d"),
    ("sensor3d", "physics3d"),
    ("character", "character3d"),
    ("camera_control", "camera_rig"),
    ("collider2d", "collider"),
    ("sensor2d", "collider"),
    ("controller2d", "controller"),
];

/// Top-level parts of `entities`' answer: the object's document form, then what it adds.
pub const ENTITY_FIELDS: [&str; 33] = [
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
    "scripts",
    "clips",
    "model_source",
    "rotation_degrees",
    "world",
    "visual_bounds",
    "collider_bounds",
    "can_add_components",
    "components",
    "referenced_by",
    "referenced_by_total",
];

/// One object (`id`) or several (`ids`, listed under `objects`), as `query` asks.
pub fn entities(project: &Project, scene: &str, query: &EntityQuery) -> Result<Json, String> {
    let scene = project.find_scene(scene, "scene")?;
    let view = SceneView::new(scene);
    let what = if query.list { "ids" } else { "id" };
    let mut objects = query
        .ids
        .iter()
        .map(|text| {
            let e = scene.find_entity(text, what)?;
            entity_view(project, scene, &view, e, query)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if query.list {
        json!({"objects": objects})
    } else {
        objects.remove(0)
    })
}

/// Marks with `missing: true`, in an answer of [`entities`], each script an object attaches that
/// the scripts folder of the project saved in `root` lacks. Nothing changes when the folder
/// cannot be read.
pub fn mark_missing_scripts(answer: &mut Json, root: &std::path::Path) {
    let objects: Vec<&mut Json> = if answer.get("objects").is_some() {
        match answer.get_mut("objects") {
            Some(Json::Array(objects)) => objects.iter_mut().collect(),
            _ => Vec::new(),
        }
    } else {
        vec![answer]
    };
    let mut files = None;
    for object in objects {
        let Some(Json::Array(scripts)) = object.get_mut("scripts") else {
            continue;
        };
        if files.is_none() {
            match crate::persistence::script_stamps(root) {
                Ok(found) => files = Some(found),
                Err(_) => return,
            }
        }
        let files = files.as_ref().expect("read above");
        for script in scripts {
            if script["path"]
                .as_str()
                .is_some_and(|p| !files.contains_key(p))
            {
                script["missing"] = json!(true);
            }
        }
    }
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
    e: &Entity,
    query: &EntityQuery,
) -> Result<Json, String> {
    let id = e.id.as_str();
    let mut value = serde_json::to_value(e).map_err(|e| e.to_string())?;
    name_groups(project, &mut value);
    let map = value
        .as_object_mut()
        .ok_or("Objeto sem forma de documento")?;
    map.retain(|_, part| !part.is_null());
    if query.graph == Detail::Summary {
        map.insert("graph".into(), graph_summary(project, scene, &e.graph));
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
    // Keyframes in degrees too, as set_clip takes them.
    if let Some(Json::Array(clips)) = map.get_mut("clips") {
        let keyframes = e
            .clips
            .iter()
            .flat_map(|clip| &clip.tracks)
            .flat_map(|track| &track.keyframes);
        let written = clips
            .iter_mut()
            .filter_map(|clip| clip["tracks"].as_array_mut())
            .flatten()
            .filter_map(|track| track["keyframes"].as_array_mut())
            .flatten();
        for (written, keyframe) in written.zip(keyframes) {
            written["rotation_degrees"] = json!(keyframe.transform.rotation.map(f32::to_degrees));
        }
    }
    let references = referenced_by(project, scene, id);
    if !references.is_empty() || query.fields.iter().any(|f| f.starts_with("referenced_by")) {
        if references.len() > REFERENCES_LISTED {
            map.insert("referenced_by_total".into(), json!(references.len()));
        }
        map.insert(
            "referenced_by".into(),
            json!(
                references
                    .into_iter()
                    .take(REFERENCES_LISTED)
                    .collect::<Vec<_>>()
            ),
        );
    }
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

/// Adds to the collision filters of an object's document form the names of their groups:
/// category_groups and mask_groups, "all" when every bit is set, as update_entity takes them.
fn name_groups(project: &Project, value: &mut Json) {
    for path in command::FILTERS {
        let Some(filter) = path
            .iter()
            .try_fold(&mut *value, |part, key| part.get_mut(*key))
            .and_then(Json::as_object_mut)
        else {
            continue;
        };
        for key in ["category", "mask"] {
            let Some(bits) = filter.get(key).and_then(Json::as_u64) else {
                continue;
            };
            let names = if bits == u64::from(u32::MAX) {
                json!("all")
            } else {
                json!(project.collision_group_names(bits as u32))
            };
            filter.insert(format!("{key}_groups"), names);
        }
    }
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

/// The name of what `value`, a parameter that names something of the project (`reference`),
/// names in `scene`: a scene, an audio, a clip, an input action, a game variable, an object or
/// a surface. None when it names nothing there.
fn reference_name(
    project: &Project,
    scene: &Scene,
    reference: Reference,
    value: &Value,
) -> Option<String> {
    let text = match value {
        Value::Object(Some(id)) if reference == Reference::Object => {
            return scene.entity(id).map(|e| e.name.clone());
        }
        Value::Surface(Some(id)) if reference == Reference::Surface => {
            return project
                .surfaces
                .iter()
                .find(|s| s.id == *id)
                .map(|s| s.name.clone());
        }
        Value::Text(text) if !text.is_empty() => text,
        _ => return None,
    };
    match reference {
        Reference::Scene => project.scene(text).map(|s| s.name.clone()),
        Reference::Audio => project.asset(text).map(|a| a.name.clone()),
        Reference::Clip => scene
            .entities
            .iter()
            .flat_map(|e| &e.clips)
            .find(|clip| clip.id == *text)
            .map(|clip| clip.name.clone()),
        Reference::InputAction => project
            .input_bindings
            .contains_key(text)
            .then(|| input_actions::label(project, text).into_owned()),
        Reference::Variable => variables::get(project, text).map(|v| v.name.clone()),
        _ => None,
    }
}

/// A name or text cut to 24 characters, for labels.
fn shortened(text: &str) -> String {
    if text.chars().count() > 24 {
        format!("{}…", text.chars().take(23).collect::<String>())
    } else {
        text.to_owned()
    }
}

/// What a node's label shows in parentheses after its operation: its first non-empty text or
/// surface parameter (by the name of the scene, audio, clip, input action, game variable or
/// surface it names), or its only parameter, then "→" and the object its first object
/// parameter names, as in "action.component(visible → Resultado)". A parameter whose input of
/// the same id is connected is left out: the game uses the input.
fn label_detail(project: &Project, scene: &Scene, graph: &Graph, node: &Node) -> Option<String> {
    let definition = graph::definition(&node.operation)?;
    let used: Vec<(&ParamDef, &Value)> = definition
        .params
        .iter()
        .filter(|param| !graph.input_connected(&node.id, param.id))
        .filter_map(|param| Some((param, node.params.get(param.id)?)))
        .collect();
    let named = |param: &ParamDef, value: &Value| {
        graph::reference(definition.id, param)
            .and_then(|reference| reference_name(project, scene, reference, value))
    };
    // A text that is the ID of a scene, an asset or a game variable, such as a scene kept for
    // a scene change, reads by that name too.
    let identified = |text: &str| {
        project
            .scene(text)
            .map(|s| s.name.clone())
            .or_else(|| project.asset(text).map(|a| a.name.clone()))
            .or_else(|| variables::get(project, text).map(|v| v.name.clone()))
    };
    let detail = used
        .iter()
        .find_map(|(param, value)| match value {
            Value::Text(text) if !text.is_empty() => Some(
                named(param, value)
                    .or_else(|| identified(text))
                    .unwrap_or_else(|| text.clone()),
            ),
            Value::Surface(Some(_)) => named(param, value),
            _ => None,
        })
        .or_else(|| match (definition.params.as_slice(), used.as_slice()) {
            ([_], [(_, Value::Number(n))]) => Some(n.to_string()),
            ([_], [(_, Value::Bool(b))]) => Some(b.to_string()),
            _ => None,
        });
    let object = used.iter().find_map(|(param, value)| {
        value
            .object()
            .map(|_| named(param, value).unwrap_or_else(|| "objeto ausente".into()))
    });
    match (detail, object) {
        (Some(detail), Some(object)) => {
            Some(format!("{} → {}", shortened(&detail), shortened(&object)))
        }
        (None, Some(object)) => Some(format!("→ {}", shortened(&object))),
        (Some(detail), None) => Some(shortened(&detail)),
        (None, None) => None,
    }
}

/// A parameter's value in a label: the name of what it names, a text in quotes.
fn value_text(
    project: &Project,
    scene: &Scene,
    definition: &OperationDef,
    param: &ParamDef,
    value: &Value,
) -> String {
    if let Some(name) = graph::reference(definition.id, param)
        .and_then(|reference| reference_name(project, scene, reference, value))
    {
        return shortened(&name);
    }
    let numbers = |values: &[f32]| {
        let numbers: Vec<String> = values.iter().map(f32::to_string).collect();
        format!("[{}]", numbers.join(", "))
    };
    match value {
        Value::Number(n) => n.to_string(),
        Value::Text(text) => format!("\"{}\"", shortened(text)),
        Value::Bool(b) => b.to_string(),
        Value::Object(None) => "este objeto".into(),
        Value::Object(Some(_)) => "objeto ausente".into(),
        Value::Surface(None) => "nenhuma".into(),
        Value::Surface(Some(_)) => "superfície ausente".into(),
        Value::Vector2(v) => numbers(v),
        Value::Vector3(v) => numbers(v),
    }
}

/// The labels of `graph`'s nodes, in its order: the operation and, in parentheses, the detail
/// [`label_detail`] gives. Nodes that would share a label add the first parameter they use
/// whose value differs among them, as in "attribute.set(Aviso, value = \"3\")"; a label still
/// repeated in the graph ends in #N, N being the node's index in the stored graph.
pub(crate) fn node_labels(project: &Project, scene: &Scene, graph: &Graph) -> Vec<String> {
    let mut details: Vec<Option<String>> = graph
        .nodes
        .iter()
        .map(|node| label_detail(project, scene, graph, node))
        .collect();
    let label = |node: &Node, detail: &Option<String>| match detail {
        Some(detail) => format!("{}({detail})", node.operation),
        None => node.operation.clone(),
    };
    let mut shared: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        shared.entry(label(node, &details[i])).or_default().push(i);
    }
    for nodes in shared.into_values().filter(|nodes| nodes.len() > 1) {
        let Some(definition) = graph::definition(&graph.nodes[nodes[0]].operation) else {
            continue;
        };
        let value = |i: usize, param: &'static ParamDef| {
            graph.nodes[i]
                .params
                .get(param.id)
                .unwrap_or(&param.default)
        };
        let Some(param) = definition.params.iter().find(|&param| {
            nodes
                .iter()
                .all(|&i| !graph.input_connected(&graph.nodes[i].id, param.id))
                && nodes
                    .iter()
                    .any(|&i| value(i, param) != value(nodes[0], param))
        }) else {
            continue;
        };
        for &i in &nodes {
            let differs = format!(
                "{} = {}",
                param.id,
                value_text(project, scene, definition, param, value(i, param))
            );
            details[i] = Some(match details[i].take() {
                Some(detail) => format!("{detail}, {differs}"),
                None => differs,
            });
        }
    }
    let base: Vec<String> = graph
        .nodes
        .iter()
        .zip(&details)
        .map(|(node, detail)| label(node, detail))
        .collect();
    let mut repeated: HashMap<&str, usize> = HashMap::new();
    for label in &base {
        *repeated.entry(label).or_default() += 1;
    }
    base.iter()
        .enumerate()
        .map(|(i, label)| {
            if repeated[label.as_str()] > 1 {
                format!("{label}#{i}")
            } else {
                label.clone()
            }
        })
        .collect()
}

/// A logic graph of an object of `scene` in few lines: `nodes` maps each node's label (its
/// operation and main parameter, with the names of what it names) to its ID and the parameters
/// that differ from the operation's defaults, a value that names something with its name, and
/// `edges` are text, such as "event.input(Reiniciar).exec -> character.teleport.exec". Labels
/// that would repeat tell their nodes apart as [`node_labels`] says.
pub fn graph_summary(project: &Project, scene: &Scene, graph: &Graph) -> Json {
    let labels = node_labels(project, scene, graph);
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
            let definition = graph::definition(&node.operation);
            let defaults = definition.map(|d| d.params.as_slice()).unwrap_or_default();
            let changed: Map<String, Json> = node
                .params
                .iter()
                .filter_map(|(key, value)| {
                    let param = defaults.iter().find(|p| p.id == key.as_str());
                    if param.is_some_and(|p| p.default == *value) {
                        return None;
                    }
                    let mut shown = json!(value);
                    if let (Some(definition), Some(param)) = (definition, param)
                        && let Some(name) = graph::reference(definition.id, param)
                            .and_then(|reference| reference_name(project, scene, reference, value))
                    {
                        shown["name"] = json!(name);
                    }
                    Some((key.clone(), shown))
                })
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

/// References an object's `referenced_by` lists; more are only counted.
const REFERENCES_LISTED: usize = 50;

/// What names the object `id` in `scene`, each as {scene: {id, name}, entity, name, role, …}
/// (entity and name are the object that holds the reference): role is `attribute <name>`, `ui`
/// (the interface element shows its attribute), `camera_control` (with param `target` or
/// `hidden`), `animation` (another object's, with clip, moves it) or the operation of a logic
/// node, with node (its label in the graph summary) and param, unless an input connected to
/// the param replaces it.
fn referenced_by(project: &Project, scene: &Scene, id: &str) -> Vec<Json> {
    let mut found = Vec::new();
    let place = json!({"id": scene.id, "name": scene.name});
    for e in &scene.entities {
        let entry = |role: &str, extra: Json| {
            let mut entry = json!({"scene": place, "entity": e.id, "name": e.name, "role": role});
            if let (Some(entry), Json::Object(extra)) = (entry.as_object_mut(), extra) {
                entry.extend(extra);
            }
            entry
        };
        for (key, value) in &e.attributes {
            if value.object() == Some(id) {
                found.push(entry(&format!("attribute {key}"), Json::Null));
            }
        }
        if e.ui.as_ref().and_then(|ui| ui.binding_object.as_deref()) == Some(id) {
            found.push(entry("ui", Json::Null));
        }
        if let Some(rig) = &e.camera_rig {
            if rig.target.as_deref() == Some(id) {
                found.push(entry("camera_control", json!({"param": "target"})));
            }
            if rig.hidden.iter().any(|hidden| hidden == id) {
                found.push(entry("camera_control", json!({"param": "hidden"})));
            }
        }
        // An object's own animations move it by its own ID.
        if e.id != id {
            for clip in &e.clips {
                if clip.tracks.iter().any(|track| track.target == id) {
                    found.push(entry("animation", json!({"clip": clip.name})));
                }
            }
        }
        let mut labels = None;
        for (index, node) in e.graph.nodes.iter().enumerate() {
            for (key, value) in &node.params {
                if value.object() == Some(id) && !e.graph.input_connected(&node.id, key) {
                    let labels =
                        labels.get_or_insert_with(|| node_labels(project, scene, &e.graph));
                    found.push(entry(
                        &node.operation,
                        json!({"node": labels[index], "param": key}),
                    ));
                }
            }
        }
    }
    found
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
                    if let Some(range) = p.range {
                        param["min"] = json!(range.min);
                        if let Some(max) = range.max {
                            param["max"] = json!(max);
                        }
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
        "ui_elements": {
            "kinds": ["Text", "Image", "Button", "Bar"],
            "fields": "An interface element is the object's ui, which update_entity patches: kind; anchor (TopLeft, TopRight, BottomLeft, BottomRight or Center), position [x, y] (the offset from that corner) and size [width, height], in pixels of the game view; text, where {value} or {valor} shows the bound attribute; color [r, g, b, a] from 0 to 1 (an Image without texture is a panel in it); texture (an image asset ID); binding_object (left out, the object itself) and binding_attribute; max_value (a Bar's full value). text_align (Left, the default, Center or Right) places each line of a Text across its width. text_size is the font size in pixels, 10 to 96, of a Text or a Button label; left out (null in a patch) it is automatic: half the height from 12 to 32 on a Text, 16 on a Button (15 on one with an image). Both stay out of the document while left at the default.",
        },
        "presets": presets(),
        "movement_profiles": movement_profiles(),
        "surface_presets": ["common", "ice", "mud", "conveyor"],
        "input_keys": input_actions::keys().collect::<Vec<_>>(),
        "value_format": "Attribute and node parameter values are tagged: {\"Number\": 1.0}, {\"Text\": \"...\"}, {\"Bool\": true}, {\"Object\": \"<id>\" or null}, {\"Vector2\": [x, y]}, {\"Vector3\": [x, y, z]}, {\"Surface\": \"<id>\" or null}. A node parameter may also be plain when its default above has the type: a number for Number, a text for Text, true or false for Bool. A parameter with an input of the same id of type any (attribute.set value, condition.compare a and b) takes any type: its default is only where it starts, and a plain number, text or true/false keeps that type. Data nodes (no exec input) are evaluated when the node that consumes them runs, each time, not once per event: a data chain wired to two nodes is computed twice, and after an action changes what it reads it gives the new value (attribute.get Tempo plus dt wired to attribute.set Tempo and then to text.time adds dt twice). To use a value right after setting it, read it with a fresh attribute.get or variable.get instead of reusing the expression that computed it. Events and actions store their outputs once per execution (event.step time, random.number value, query.* hit, character.teleport success, action.spawn created): the nodes after them in the same flow read those stored values.",
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
            "Plataforma móvel (não é um personagem): um bloco sólido de 3 × 0,4 × 3 m que vai e volta a partir de onde é posto, sem parar ou parando nas pontas, e pode girar, levando junto o que estiver em cima. O percurso é uma animação comum, editável com set_clip; set_transform move a plataforma com o percurso.",
            json!({
                "travel": {"default": [4., 0., 0.], "description": "Quanto a plataforma anda a partir de onde é posta, em metros, antes de voltar. [0, 0, 0] com spin faz uma plataforma que só gira (a animação se chama Giro)."},
                "period": {"default": 4., "description": "Segundos da ida e da volta, com as paradas."},
                "pause_at_ends": {"default": 0., "description": "Segundos que a plataforma fica parada em cada ponta (posição e giro), dentro do período: menos que metade dele. Ela sai andando no começo da cena, para na ponta de lá e, na volta, no lugar onde foi posta; os quadros-chave das pontas seguram a pose (hold)."},
                "spin": {"default": [0., 0., 0.], "description": "Graus que a plataforma gira em X, Y e Z por período, enquanto anda, levando junto e girando quem está em cima: [0, 360, 0] dá uma volta em Y a cada período, [0, -180, 0] meia volta para o outro lado. Cada ida e cada volta ganham um quadro-chave a cada 90° no máximo (somando os eixos), porque entre dois quadros-chave o giro vai pelo caminho mais curto. Para recomeçar sem salto, a animação dura os períodos que o giro leva para voltar ao começo, com voltas inteiras em todos os eixos: 1 com 360° por período, 2 com 180°, 4 com 90°, 8 com 45°. Um giro que precisaria de mais de 8 períodos, ou que passa de 3600° por período somando os eixos, é recusado."},
            }),
        ),
    ]
    .into_iter()
    .map(|(id, preset, description, mut params)| {
        params["filter"] = json!({
            "default": {"category": ["Cenário padrão"], "mask": "all"},
            "description": "O filtro de colisão do corpo do personagem (jogadores) ou do colisor da plataforma: category (os grupos em que ele está) e mask (os grupos que ele encontra), cada um como número (os bits) ou como lista de nomes dos grupos de colisão do oxy_project, \"all\" para todos. O que ficar de fora mantém o padrão.",
        });
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

/// Where a line of the editor's console comes from, as the live console read names it: the
/// editor, an agent's edits, the game's logs or the logic actions that ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleOrigin {
    Editor,
    Agente,
    Jogo,
    Logica,
}

/// Lines a console read answers when `limit` is absent.
pub const CONSOLE_LIMIT: usize = 50;

/// Options of the live console read: the lines after `since`, of `origins` (every origin when
/// empty), the most recent `limit` of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleQuery {
    pub since: Option<u64>,
    pub origins: Vec<ConsoleOrigin>,
    pub limit: usize,
}
impl ConsoleQuery {
    /// From the arguments `since`, `limit` and `origin` (one origin or a list).
    pub fn parse(args: &Json) -> Result<Self, String> {
        let origins = match args.get("origin") {
            Some(Json::String(_)) => arg(args, "origin")?.into_iter().collect(),
            _ => arg(args, "origin")?.unwrap_or_default(),
        };
        Ok(Self {
            since: arg(args, "since")?,
            origins,
            limit: arg(args, "limit")?.unwrap_or(CONSOLE_LIMIT),
        })
    }
    /// Whether a line of `origin` numbered `seq` is asked for, before the limit.
    pub fn wants(&self, seq: u64, origin: ConsoleOrigin) -> bool {
        self.since.is_none_or(|since| seq > since)
            && (self.origins.is_empty() || self.origins.contains(&origin))
    }
}
