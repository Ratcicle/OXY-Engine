//! MCP tools. Each call targets either an open editor (live: edits appear at once, are
//! undoable, and the person saves) or, when no editor has the project open, the project files.
use crate::editors::{self, LiveEditor};
use glam::Vec3;
use oxy_core::{
    command::{self, Applied},
    document::*,
    encoding, headless, persistence, query,
    runtime::Runtime,
};
use oxy_render::{CameraState, offscreen::ShotView};
use serde_json::{Value as Json, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// What a tool call returns: JSON text and, optionally, a PNG image.
#[derive(Debug)]
pub struct Output {
    pub text: String,
    pub image: Option<String>,
}
impl Output {
    fn json(value: Json) -> Self {
        Self {
            text: serde_json::to_string_pretty(&value).unwrap_or_default(),
            image: None,
        }
    }
}

enum Target {
    Live(u16),
    File(PathBuf),
}

#[derive(Default)]
pub struct Tools {
    live: HashMap<u16, LiveEditor>,
    gpu: Option<(oxy_render::RenderState, oxy_render::Renderer)>,
}

const PROJECT: &str = "Optional path to the project folder or its project.oxy.json. When omitted, \
the single open editor that allows agents is used.";
const SCENE: &str = "Optional scene ID. Defaults to the scene open in the editor (live) or the \
project's start scene (files).";
/// The game always starts in the start scene, live too, unlike the editing tools.
const START_SCENE: &str = "Optional scene the game starts in. Defaults to the project's start \
scene, also live (not the scene open in the editor); setup commands without a scene use it too.";
const INPUTS: &str = "Timed inputs at simulated second `at`: {type: press|release, at, action} \
with the project's input action names (see oxy_project); {type: movement, at, value: [right, \
forward]}, each -1 to 1, held until the next movement; {type: look, at, value: [x, y]}, a mouse \
displacement applied once: each unit turns the camera by its sensitivity (camera control, 0.12 \
degrees by default), +x turns right and +y looks down unless the camera inverts an axis; {type: \
click, at, entity}. They survive scene changes and reloads; a key held across one stays held.";
const COLLIDERS: &str = "Draws colliders (green) and sensor areas (yellow) over the image, \
invisible objects' too, as the editor's collider view does.";

/// Tool definitions for tools/list.
pub fn definitions() -> Json {
    let project = json!({"type": "string", "description": PROJECT});
    let scene = json!({"type": "string", "description": SCENE});
    let start_scene = json!({"type": "string", "description": START_SCENE});
    let inputs = json!({"type": "array", "items": {"type": "object"}, "description": INPUTS});
    let colliders = json!({"type": "boolean", "description": COLLIDERS});
    let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3});
    let tool = |name: &str, description: &str, properties: Json, required: Json| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": properties, "required": required},
        })
    };
    json!([
        tool(
            "oxy_status",
            "Lists open OXY editors that allow AI agents (project, active scene, unsaved changes, \
             whether the editor is busy) and explains how tools pick a target. Call this first.",
            json!({}),
            json!([])
        ),
        tool(
            "oxy_project",
            "Summary of a project: scenes (ID, name, 2D/3D, object count), assets and input actions.",
            json!({"project": project}),
            json!([])
        ),
        tool(
            "oxy_scene",
            "Object tree of a scene: ID, name, kind, shape, components, logic node count and children.",
            json!({"project": project, "scene": scene}),
            json!([])
        ),
        tool(
            "oxy_entity",
            "One object in full (its document JSON: transform in radians plus rotation_degrees, \
             material, attributes, components, logic graph, animation clips) and the components \
             it can still receive.",
            json!({"project": project, "scene": scene,
                   "id": {"type": "string", "description": "Object ID."}}),
            json!(["id"])
        ),
        tool(
            "oxy_find",
            "Finds objects by name (case-insensitive substring) in every scene.",
            json!({"project": project, "text": {"type": "string"}}),
            json!(["text"])
        ),
        tool(
            "oxy_catalog",
            "Catalog of logic nodes (operation ID, ports with types, parameters with defaults, \
             description), components, shapes, interface elements, presets and the value format. \
             Read it before building logic.",
            json!({}),
            json!([])
        ),
        tool(
            "oxy_apply",
            APPLY_DESCRIPTION,
            json!({"project": project, "scene": scene,
                   "commands": {"type": "array", "items": {"type": "object"},
                                "description": "Commands applied in order as one atomic batch."}}),
            json!(["commands"])
        ),
        tool(
            "oxy_validate",
            "Validates the whole project with the same rules used when saving.",
            json!({"project": project}),
            json!([])
        ),
        tool(
            "oxy_simulate",
            SIMULATE_DESCRIPTION,
            json!({"project": project, "scene": start_scene,
                   "seconds": {"type": "number", "description":
                       "Simulated seconds (max 600); the upper bound when `until` is set."},
                   "inputs": inputs,
                   "watch": {"type": "array", "items": {"anyOf": [
                       {"type": "string"},
                       {"type": "object", "properties": {
                           "id": {"type": "string"},
                           "fields": {"type": "array", "items": {"type": "string", "enum":
                               ["position", "rotation", "scale", "visible", "attributes", "character"]}},
                           "attributes": {"anyOf": [{"type": "boolean"},
                               {"type": "array", "items": {"type": "string"}}]}},
                        "required": ["id"]}]},
                       "description": "Objects to report: an ID, or {id, fields?, attributes?}. \
                        fields defaults to position, visible and attributes (world position, XYZ \
                        rotation in degrees, world scale); character adds velocity, speed, \
                        grounded, posture, sliding, sprinting, support {id, name, surface} and yaw, \
                        look_yaw and pitch in degrees for 3D characters. attributes: false leaves \
                        them out, a list keeps only those names."},
                   "samples": {"type": "integer", "description":
                       "Samples of watched objects (max 120), the last being the final state. They \
                        are spaced evenly across `seconds`, so a run that until stops early returns \
                        fewer."},
                   "actions": {"type": "string", "enum": ["none", "summary", "all"], "description":
                       "Logic actions that ran. summary (default): one line per object name and \
                        action with count, first and last time (objects sharing a name, such as \
                        spawned copies, share a line; the 300 lines most recently active stay). \
                        all: every action with its time, the most recent 300. actions_filter \
                        applies to both."},
                   "actions_filter": {"type": "object", "properties": {
                       "objects": {"type": "array", "items": {"type": "string"}},
                       "operations": {"type": "array", "items": {"type": "string"}},
                       "exclude_operations": {"type": "array", "items": {"type": "string"}}},
                       "description": "Keep only actions of these objects / operations. \
                        exclude_operations defaults to [\"event.step\"] unless operations is \
                        given: an excluded event also hides the actions only it starts (per-step \
                        logic and timers), except action.scene, so every scene change has its \
                        action. Pass [] to see everything."},
                   "setup": {"type": "array", "items": {"type": "object"}, "description":
                       "oxy_apply commands applied to the in-memory copy before the run, never to \
                        the project; commands without a scene use the start scene. The IDs they \
                        create are \"$N\" (\"$N.M\" for the M-th) in watch, inputs, spawn, \
                        until and actions_filter, and the report lists them under setup."},
                   "spawn": {"type": "object", "properties": {
                       "entity": {"type": "string"},
                       "position": {"type": "array", "items": {"type": "number"}},
                       "yaw": {"type": "number", "description": "Degrees around the vertical axis."}},
                       "required": ["entity", "position"],
                       "description": "Places an object of the start scene at a world position \
                        (and yaw) on the copy before the run, e.g. the player at a section."},
                   "until": {"type": "object", "properties": {
                       "scene_change": {"anyOf": [{"type": "boolean"}, {"type": "string"}],
                           "description": "true: any scene start, reloads included; or a scene ID."},
                       "log_contains": {"type": "string"},
                       "attribute": {"type": "object", "properties": {
                           "entity": {"type": "string"}, "name": {"type": "string"},
                           "op": {"type": "string", "enum": ["==", "!=", "<", "<=", ">", ">="]},
                           "value": {"description": "Number, boolean, text or a tagged value, of \
                               the attribute's type. For an object or surface attribute, a text \
                               is its ID (\"$N\" allowed)."}},
                           "required": ["entity", "name", "op", "value"]},
                       "entity_absent": {"type": "string", "description":
                           "Stops once this object of the start scene no longer exists."},
                       "event": {"type": "object", "properties": {
                           "kind": {"type": "string", "enum": ["jump", "land", "leave", "side",
                               "surface", "posture", "area_enter", "area_exit"]},
                           "object": {"type": "string", "description": "Either side of the event."}},
                           "required": ["kind"]}},
                       "description": "Stops at the first moment any condition holds; the report \
                        says which in stopped_at {time, reason, detail}."},
                   "ui": {"type": "boolean", "description":
                       "Reports the game interface at the end (default true): ui, in drawing order \
                        (the topmost last), [{id, name, kind: Text|Image|Button|Bar, visible, rect: \
                        [x, y, width, height] in pixels of the game view from its top-left corner, \
                        text as shown with bound values, fraction (bars, 0-1), clickable (a visible \
                        button inside the view)}]."},
                   "width": {"type": "integer", "description":
                       "Game view width in pixels, 16-4096 (default 960): the screenshot and the ui \
                        rectangles."},
                   "height": {"type": "integer", "description":
                       "Game view height in pixels, 16-4096 (default 540)."},
                   "screenshot": {"type": "boolean", "description":
                       "Also returns the game camera view at the end, as oxy_screenshot view=game."},
                   "colliders": colliders}),
            json!(["seconds"])
        ),
        tool(
            "oxy_screenshot",
            SCREENSHOT_DESCRIPTION,
            json!({"project": project, "scene": start_scene,
                   "view": {"type": "string", "enum": ["editor", "game", "free", "top", "side"],
                       "description": "editor by default live, game on files."},
                   "seconds": {"type": "number", "description":
                       "Simulated seconds before the capture, max 600 (default 0: the first frame). \
                        Every view but editor."},
                   "inputs": inputs,
                   "colliders": colliders,
                   "eye": {"description": "view=free: camera position [x, y, z].", "type": "array",
                       "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                   "target": {"description": "view=free: the point looked at, [x, y, z], or an \
                       object ID (the center of its shape, or its position without one; \"$N\" \
                       from setup too).",
                       "anyOf": [point, {"type": "string"}]},
                   "fov": {"type": "number", "description":
                       "view=free: vertical field of view in degrees, 10-150 (default 60)."},
                   "ids": {"type": "array", "items": {"type": "string"}, "description":
                       "view=top|side: objects framed, with their children (default: every \
                        object with a shape, hidden ones too)."},
                   "width": {"type": "integer", "description": "16-4096 pixels (default 960)."},
                   "height": {"type": "integer", "description": "16-4096 pixels (default 540)."}}),
            json!([])
        ),
        tool(
            "oxy_select",
            "Live only: selects objects in the editor (switching scene if needed) so the person \
             sees what you are talking about.",
            json!({"project": project, "scene": scene,
                   "ids": {"type": "array", "items": {"type": "string"}}}),
            json!(["ids"])
        ),
        tool(
            "oxy_frame",
            "Live only: frames the selection in the editor viewport.",
            json!({"project": project}),
            json!([])
        ),
        tool(
            "oxy_undo",
            "Live only: undoes the last step in the editor history (yours or the person's; check \
             the label in oxy_status first).",
            json!({"project": project}),
            json!([])
        ),
        tool(
            "oxy_redo",
            "Live only: redoes the last undone step.",
            json!({"project": project}),
            json!([])
        ),
    ])
}

const APPLY_DESCRIPTION: &str = "Edits the project with a batch of commands, all-or-nothing: if \
one fails, nothing changes and the error names it. Live, the batch is one undo step labelled \
'Agente: …' in the editor and the person saves; on files, the project file is saved (the \
previous version is kept as project.oxy.json.agente.bak). In later commands, \"$N\" stands for \
the first ID created by command N (0-based). Commands (field `op`):
- rename_project {name}; set_start_scene {scene}; create_scene {name, kind: \"2d\"|\"3d\"}; \
rename_scene {scene, name}; duplicate_scene {scene}; delete_scene {scene}
- create_entity {kind, name?, parent?, position?: [x,y,z]} where kind is {type: group} | \
{type: shape, shape: Cube|Sphere|Cylinder|Plane|Pyramid|Cone|Tube (3D) or Rectangle|Circle|Sprite \
(2D)} | {type: camera} | {type: ui, element: Text|Image|Button|Bar} | {type: preset, preset: \
first_person|third_person|platform} (3D only)
- delete_entities {ids}; duplicate_entities {ids}; rename_entity {entity, name}; set_parent {ids, \
parent?} (keeps world transform)
- set_transform {entity, position?, rotation? (degrees), scale?}; set_visible {entity, visible}; \
set_material {entity, color?: [r,g,b,a] 0-1, texture?: asset ID, clear_texture?}
- set_attribute {entity, name, value?} (no value removes); values are tagged, e.g. {\"Number\": 3}
- add_component / remove_component {entity, component: character|collider3d|sensor3d|platform|\
camera_control|collider2d|sensor2d|controller2d}
- update_entity {entity, patch}: JSON merge patch over the object's document form (see \
oxy_entity) for fields without a dedicated command
- add_node {entity, operation, position?: [x,y], params?}; remove_node {entity, node}; \
set_node_params {entity, node, params}; connect / disconnect {entity, from_node, from_port, \
to_node, to_port}
Every command except the project/scene ones accepts `scene`; the tool's `scene` fills it when absent.";

const SIMULATE_DESCRIPTION: &str = "Runs the game without a window on an in-memory copy of \
the project (nothing is saved or changed) and reports what happened: scene changes (reload: true \
when a scene restarts), logs with object and node names, a summary of the logic actions that ran, \
sounds, movement and area events {time, kind: jump|land|leave|side|surface|posture|area_enter|\
area_exit, object, other?} as the game fires them (also when its logic then removes the objects \
involved or changes scene), the game camera at the end {active, mode, position, forward, fov}, \
the game interface at the end (ui: texts with their values, bars, buttons) and the watched \
objects. Inputs use the project's input action names (see oxy_project). To test one \
section without editing the project, use setup (commands on the copy), spawn (place the player) \
and until (stop when something happens). Lists keep the most recent 300 entries and `truncated` \
counts what was dropped. Times are seconds rounded to 0.1 ms. Set screenshot=true to also get the \
game camera view at the end.";

const SCREENSHOT_DESCRIPTION: &str = "PNG image. view=editor (live only, the default there): \
the editor viewport as the person sees it, with the collider view when colliders is set. Every \
other view first runs the game on an in-memory copy for `seconds`, with inputs (and setup, spawn \
and until as in oxy_simulate), then captures: game (the default on files) through the game \
camera, and lists the game interface as ui like oxy_simulate, since the image leaves out texts, \
buttons and bars; free through a perspective camera at eye looking at target (3D scenes); top \
and side through orthographic views framing ids: top straight down with -Z up and +X right, \
side level along -Z, or along -X when the framed part is deeper (Z) than wide (X), so the long \
side spans the image. In 2D scenes top and side both show the scene plane. The text gives the \
time, the scene and the camera used (position, forward, and fov or the visible width and \
height in metres).";

/// Scene-level commands that do not take a default scene.
const PROJECT_OPS: [&str; 6] = [
    "rename_project",
    "set_start_scene",
    "create_scene",
    "rename_scene",
    "duplicate_scene",
    "delete_scene",
];

fn text_arg(args: &Json, key: &str) -> Option<String> {
    args[key].as_str().map(str::to_owned)
}

/// The game view size asked for, `headless::VIEW_SIZE` when absent.
fn view_size(args: &Json) -> [u32; 2] {
    let side = |key: &str, default: u32| {
        args[key]
            .as_u64()
            .map_or(default, |v| v.min(u64::from(u32::MAX)) as u32)
    };
    [
        side("width", headless::VIEW_SIZE[0]),
        side("height", headless::VIEW_SIZE[1]),
    ]
}

/// A short number for reports: f32 noise such as 0.4000000059604645 would hide the value.
fn short(value: f32) -> f64 {
    (f64::from(value) * 10_000.).round() / 10_000.
}
fn short3(value: Vec3) -> [f64; 3] {
    value.to_array().map(short)
}

/// Where oxy_screenshot looks from, as asked; object IDs wait for the run.
#[derive(Debug, PartialEq)]
enum Shot {
    Game,
    Free {
        eye: [f32; 3],
        target: Aim,
        fov: f32,
    },
    Top(Vec<String>),
    Side(Vec<String>),
}
#[derive(Debug, PartialEq)]
enum Aim {
    Point([f32; 3]),
    Object(String),
}

impl Shot {
    fn parse(view: &str, args: &Json) -> Result<Self, String> {
        let point = |key: &str| -> Result<Option<[f32; 3]>, String> {
            match &args[key] {
                Json::Null => Ok(None),
                value => serde_json::from_value(value.clone())
                    .map(Some)
                    .map_err(|_| format!("{key} precisa ser [x, y, z]")),
            }
        };
        let ids = || -> Result<Vec<String>, String> {
            match &args["ids"] {
                Json::Null => Ok(Vec::new()),
                value => serde_json::from_value(value.clone())
                    .map_err(|_| "ids precisa ser uma lista de IDs".to_owned()),
            }
        };
        match view {
            "game" => Ok(Self::Game),
            "free" => {
                let eye = point("eye")?.ok_or("view=free precisa de eye: [x, y, z]")?;
                let target = match &args["target"] {
                    Json::String(id) => Aim::Object(id.clone()),
                    Json::Null => {
                        return Err(
                            "view=free precisa de target: [x, y, z] ou o ID de um objeto".into(),
                        );
                    }
                    _ => Aim::Point(point("target")?.unwrap_or_default()),
                };
                let fov = match &args["fov"] {
                    Json::Null => 60.,
                    value => value.as_f64().ok_or("fov precisa ser um número")? as f32,
                };
                Ok(Self::Free { eye, target, fov })
            }
            "top" => Ok(Self::Top(ids()?)),
            "side" => Ok(Self::Side(ids()?)),
            other => Err(format!("Vista desconhecida: {other}")),
        }
    }

    /// The camera of the capture, with IDs (`"$N"` from setup too) found in the running scene.
    fn resolve(self, runtime: &Runtime, setup: &[Applied]) -> Result<ShotView, String> {
        let scene = runtime.scene();
        let ids = |ids: Vec<String>| {
            ids.iter()
                .map(|id| headless::resolve(id, setup))
                .collect::<Result<Vec<_>, _>>()
        };
        let framed = |ids: Vec<Id>| oxy_render::offscreen::framed_bounds(scene, &ids);
        Ok(match self {
            Self::Game => ShotView::Game,
            Self::Free { eye, target, fov } => {
                let target = match target {
                    Aim::Point(point) => Vec3::from(point),
                    Aim::Object(id) => {
                        let id = headless::resolve(&id, setup)?;
                        match framed(vec![id.clone()]) {
                            Ok(bounds) => (bounds.min + bounds.max) * 0.5,
                            // Groups and cameras are aimed at by their position.
                            Err(_) => scene
                                .world_matrix(&id)
                                .map_err(|_| {
                                    format!("Objeto {id} não existe na cena {}", scene.name)
                                })?
                                .w_axis
                                .truncate(),
                        }
                    }
                };
                ShotView::Free {
                    eye: Vec3::from(eye),
                    target,
                    fov,
                }
            }
            Self::Top(list) => ShotView::Top(framed(ids(list)?)?),
            Self::Side(list) => ShotView::Side(framed(ids(list)?)?),
        })
    }
}

/// The camera a capture used, for the agent to read the image with.
fn camera_text(camera: &CameraState, size: [u32; 2]) -> Json {
    let mut text = json!({
        "position": short3(camera.eye()),
        "forward": short3(camera.forward()),
    });
    if camera.kind == SceneKind::TwoD {
        let height = camera.orthographic_size * 2.;
        text["projection"] = json!("orthographic");
        text["height"] = json!(short(height));
        text["width"] = json!(short(height * size[0] as f32 / size[1].max(1) as f32));
    } else {
        text["projection"] = json!("perspective");
        text["fov"] = json!(short(camera.fov));
    }
    text
}

impl Tools {
    pub fn call(&mut self, name: &str, args: &Json) -> Result<Output, String> {
        match name {
            "oxy_status" => self.status(),
            "oxy_catalog" => Ok(Output::json(query::catalog())),
            "oxy_project" => self.read(args, "summary", |p, _| Ok(query::project_summary(p))),
            "oxy_scene" => self.read(args, "scene_tree", query::scene_tree),
            "oxy_entity" => {
                let id = text_arg(args, "id").ok_or("Informe id")?;
                self.read(args, "entity", move |p, scene| query::entity(p, scene, &id))
            }
            "oxy_find" => {
                let text = text_arg(args, "text").ok_or("Informe text")?;
                self.read(args, "find", move |p, _| Ok(query::find(p, &text)))
            }
            "oxy_validate" => self.read(args, "validate", |p, _| {
                Ok(match validate_project(p) {
                    Ok(()) => json!({"valid": true}),
                    Err(error) => json!({"valid": false, "error": error.to_string()}),
                })
            }),
            "oxy_apply" => self.apply(args),
            "oxy_simulate" => self.simulate(args),
            "oxy_screenshot" => self.screenshot(args),
            "oxy_select" | "oxy_frame" | "oxy_undo" | "oxy_redo" => {
                let Target::Live(port) = self.target(args)? else {
                    return Err(
                        "Esta ferramenta só funciona com o projeto aberto no editor.".into(),
                    );
                };
                let (method, params) = match name {
                    "oxy_select" => (
                        "select",
                        json!({"ids": args["ids"], "scene": args["scene"]}),
                    ),
                    "oxy_frame" => ("frame", json!({})),
                    "oxy_undo" => ("undo", json!({})),
                    _ => ("redo", json!({})),
                };
                self.live(port, method, params).map(Output::json)
            }
            other => Err(format!("Ferramenta desconhecida: {other}")),
        }
    }

    fn status(&mut self) -> Result<Output, String> {
        let mut editors = Vec::new();
        for info in editors::discover() {
            let status = self.live(info.port, "status", json!({}));
            match status {
                Ok(status) => editors.push(status),
                Err(_) => continue,
            }
        }
        Ok(Output::json(json!({
            "editors": editors,
            "how_targets_work": "With one editor open (agents allowed in Preferências do editor), \
                tools act on it live: edits show at once, are undoable with Ctrl+Z and the person \
                saves. Pass `project` (a folder or project.oxy.json) to pick an editor by project, \
                or to edit the files directly when no editor has that project open.",
        })))
    }

    /// Connected editor on `port`, reconnecting once if the connection dropped.
    fn live(&mut self, port: u16, method: &str, params: Json) -> Result<Json, String> {
        for attempt in 0..2 {
            if let std::collections::hash_map::Entry::Vacant(slot) = self.live.entry(port) {
                let info = editors::discover()
                    .into_iter()
                    .find(|e| e.port == port)
                    .ok_or("O editor foi fechado.")?;
                slot.insert(LiveEditor::connect(&info)?);
            }
            match self
                .live
                .get_mut(&port)
                .unwrap()
                .request(method, params.clone())
            {
                Err(error) if attempt == 0 && error.contains("conexão") => {
                    self.live.remove(&port);
                }
                result => return result,
            }
        }
        unreachable!()
    }

    fn target(&mut self, args: &Json) -> Result<Target, String> {
        let open: Vec<_> = editors::discover()
            .into_iter()
            .filter(|e| e.project_name.is_some())
            .collect();
        if let Some(path) = text_arg(args, "project") {
            let path = PathBuf::from(path);
            if let Some(editor) = open.iter().find(|e| {
                e.project_path
                    .as_deref()
                    .is_some_and(|p| editors::same_project(p, &path))
            }) {
                return Ok(Target::Live(editor.port));
            }
            let file = if path.is_dir() {
                path.join(persistence::PROJECT_FILE)
            } else {
                path
            };
            if !file.is_file() {
                return Err(format!("Projeto não encontrado: {}", file.display()));
            }
            return Ok(Target::File(file));
        }
        // Only editors that still answer count.
        let mut alive = Vec::new();
        for editor in open {
            if self.live(editor.port, "status", json!({})).is_ok() {
                alive.push(editor);
            }
        }
        match alive.as_slice() {
            [one] => Ok(Target::Live(one.port)),
            [] => Err(
                "Nenhum editor da OXY com agentes permitidos está aberto. Abra o projeto no \
                editor e ligue Permitir agentes de IA em Preferências do editor, ou informe \
                `project` com a pasta do projeto para editar os arquivos."
                    .into(),
            ),
            many => Err(format!(
                "Há {} editores abertos; informe `project` com um destes: {}",
                many.len(),
                many.iter()
                    .map(|e| e.project_path.as_ref().map_or_else(
                        || e.project_name.clone().unwrap_or_default(),
                        |p| p.display().to_string()
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        }
    }

    /// The project document: from the editor (unsaved state included) or from disk.
    fn document(&mut self, target: &Target) -> Result<(Project, Option<PathBuf>), String> {
        match target {
            Target::Live(port) => {
                let status = self.live(*port, "status", json!({}))?;
                let project = serde_json::from_value(self.live(*port, "document", json!({}))?)
                    .map_err(|e| e.to_string())?;
                let root = status["project"]["path"]
                    .as_str()
                    .and_then(|p| Path::new(p).parent().map(Path::to_owned));
                Ok((project, root))
            }
            Target::File(file) => {
                let project = persistence::load_project(file).map_err(|e| e.to_string())?;
                Ok((project, file.parent().map(Path::to_owned)))
            }
        }
    }

    fn read(
        &mut self,
        args: &Json,
        method: &str,
        local: impl FnOnce(&Project, &str) -> Result<Json, String>,
    ) -> Result<Output, String> {
        match self.target(args)? {
            Target::Live(port) => {
                let mut params = args.clone();
                if let Some(map) = params.as_object_mut() {
                    map.remove("project");
                }
                self.live(port, method, params).map(Output::json)
            }
            Target::File(file) => {
                let project = persistence::load_project(&file).map_err(|e| e.to_string())?;
                let scene = text_arg(args, "scene").unwrap_or_else(|| project.start_scene.clone());
                local(&project, &scene).map(Output::json)
            }
        }
    }

    fn apply(&mut self, args: &Json) -> Result<Output, String> {
        let mut commands = match &args["commands"] {
            Json::Array(items) => items.clone(),
            _ => return Err("Informe commands como uma lista".into()),
        };
        if let Some(scene) = text_arg(args, "scene") {
            for item in &mut commands {
                let op = item["op"].as_str().unwrap_or_default().to_owned();
                if let Some(map) = item.as_object_mut()
                    && !PROJECT_OPS.contains(&op.as_str())
                    && !map.contains_key("scene")
                {
                    map.insert("scene".into(), json!(scene));
                }
            }
        }
        match self.target(args)? {
            Target::Live(port) => self
                .live(port, "apply", json!({"commands": commands}))
                .map(Output::json),
            Target::File(file) => {
                let parsed = command::parse(&Json::Array(commands))?;
                let mut project = persistence::load_project(&file).map_err(|e| e.to_string())?;
                let start = project.start_scene.clone();
                let applied =
                    command::apply(&mut project, &start, &parsed).map_err(|e| e.to_string())?;
                let backup = file.with_extension("json.agente.bak");
                std::fs::copy(&file, &backup)
                    .map_err(|e| format!("Não foi possível guardar a versão anterior: {e}"))?;
                persistence::save_project(&file, &project).map_err(|e| e.to_string())?;
                Ok(Output::json(json!({
                    "applied": applied,
                    "saved": file.display().to_string(),
                    "previous_version": backup.display().to_string(),
                })))
            }
        }
    }

    fn simulation_request(
        args: &Json,
        seconds: f64,
    ) -> Result<headless::SimulationRequest, String> {
        // Optional arguments: absent or null gives the default.
        let arg = |key: &str| args.get(key).filter(|v| !v.is_null()).cloned();
        let invalid = |what: &'static str| move |e: serde_json::Error| format!("{what}: {e}");
        Ok(headless::SimulationRequest {
            scene: text_arg(args, "scene"),
            seconds,
            inputs: arg("inputs")
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid("Entradas inválidas"))?
                .unwrap_or_default(),
            watch: arg("watch")
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid("Objetos observados inválidos"))?
                .unwrap_or_default(),
            samples: args["samples"]
                .as_u64()
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32,
            setup: arg("setup")
                .map(|commands| command::parse(&commands))
                .transpose()
                .map_err(|e| format!("Preparação (setup): {e}"))?
                .unwrap_or_default(),
            spawn: arg("spawn")
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid("spawn inválido"))?,
            until: arg("until")
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid("Condições de parada (until) inválidas"))?,
            actions: arg("actions")
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid("Modo de ações inválido"))?
                .unwrap_or_default(),
            actions_filter: arg("actions_filter")
                .map(serde_json::from_value)
                .transpose()
                .map_err(invalid("Filtro de ações inválido"))?
                .unwrap_or_default(),
            view_size: Some(view_size(args)),
            ui: args["ui"].as_bool().unwrap_or(true),
        })
    }

    fn simulate(&mut self, args: &Json) -> Result<Output, String> {
        let seconds = args["seconds"].as_f64().ok_or("Informe seconds")?;
        let request = Self::simulation_request(args, seconds)?;
        let target = self.target(args)?;
        let (project, root) = self.document(&target)?;
        let (report, mut runtime) = headless::run(&project, &request)?;
        let image = if args["screenshot"].as_bool() == Some(true) {
            let (png, _) =
                self.render_view(&mut runtime, root.as_deref(), args, &ShotView::Game)?;
            Some(png)
        } else {
            None
        };
        let mut output = Output::json(serde_json::to_value(report).map_err(|e| e.to_string())?);
        output.image = image;
        Ok(output)
    }

    fn screenshot(&mut self, args: &Json) -> Result<Output, String> {
        let target = self.target(args)?;
        let view = text_arg(args, "view").unwrap_or_else(|| {
            if matches!(target, Target::Live(_)) {
                "editor".into()
            } else {
                "game".into()
            }
        });
        let colliders = args["colliders"].as_bool() == Some(true);
        if view == "editor" {
            let Target::Live(port) = target else {
                return Err(
                    "A vista do editor só existe com o projeto aberto no editor; use view=game."
                        .into(),
                );
            };
            let shot = self.live(port, "screenshot", json!({"colliders": colliders}))?;
            return Ok(Output {
                text: format!(
                    "Vista do editor, {}×{} ({}){}.",
                    shot["width"],
                    shot["height"],
                    shot["view"],
                    if shot["colliders"] == true {
                        ", com colisores"
                    } else {
                        ""
                    }
                ),
                image: shot["png_base64"].as_str().map(str::to_owned),
            });
        }
        let shot = Shot::parse(&view, args)?;
        let seconds = args["seconds"].as_f64().unwrap_or(0.).max(1. / 60.);
        let mut request = Self::simulation_request(args, seconds)?;
        // Only the game camera shows the interface.
        request.ui &= shot == Shot::Game;
        let (project, root) = self.document(&target)?;
        let (report, mut runtime) = headless::run(&project, &request)?;
        let shot = shot.resolve(&runtime, &report.setup)?;
        let (image, camera) = self.render_view(&mut runtime, root.as_deref(), args, &shot)?;
        let mut text = json!({
            "view": view,
            "time": (runtime.time * 10_000.).round() / 10_000.,
            "scene": {"id": runtime.scene_id(), "name": runtime.scene().name},
            "camera": match &report.camera {
                Some(game) if shot == ShotView::Game => {
                    serde_json::to_value(game).map_err(|e| e.to_string())?
                }
                _ => camera_text(&camera, view_size(args)),
            },
            "colliders": colliders,
        });
        if let Some(stopped) = &report.stopped_at {
            text["stopped_at"] = serde_json::to_value(stopped).map_err(|e| e.to_string())?;
        }
        if !report.ui.is_empty() {
            text["ui"] = serde_json::to_value(&report.ui).map_err(|e| e.to_string())?;
        }
        let mut output = Output::json(text);
        output.image = Some(image);
        Ok(output)
    }

    /// PNG (base64) of the running game through `view`, and the camera used.
    fn render_view(
        &mut self,
        runtime: &mut Runtime,
        root: Option<&Path>,
        args: &Json,
        view: &ShotView,
    ) -> Result<(String, CameraState), String> {
        if self.gpu.is_none() {
            let rs = oxy_render::offscreen::headless_state()?;
            let renderer = oxy_render::Renderer::new(&rs);
            self.gpu = Some((rs, renderer));
        }
        let (rs, renderer) = self.gpu.as_mut().unwrap();
        let root = root.map_or_else(std::env::temp_dir, Path::to_owned);
        let (png, camera) = oxy_render::offscreen::view_png(
            rs,
            renderer,
            runtime,
            &root,
            view_size(args),
            view,
            args["colliders"].as_bool() == Some(true),
        )?;
        Ok((encoding::base64(&png), camera))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::{editing, persistence};

    fn schema(tool: &str) -> Json {
        definitions()
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == tool)
            .unwrap()["inputSchema"]["properties"]
            .clone()
    }

    #[test]
    fn screenshot_and_simulate_document_views_inputs_and_the_interface() {
        let shot = schema("oxy_screenshot");
        assert_eq!(
            shot["view"]["enum"],
            json!(["editor", "game", "free", "top", "side"])
        );
        for option in [
            "seconds",
            "inputs",
            "colliders",
            "eye",
            "target",
            "fov",
            "ids",
            "width",
            "height",
        ] {
            assert!(shot[option]["description"].is_string(), "{option}");
        }
        let simulate = schema("oxy_simulate");
        for option in ["ui", "width", "height", "screenshot", "colliders"] {
            assert!(simulate[option]["description"].is_string(), "{option}");
        }
        // Look units and signs, and the same inputs in both tools.
        let inputs = shot["inputs"]["description"].as_str().unwrap();
        assert!(inputs.contains("sensitivity") && inputs.contains("+x turns right"));
        assert!(inputs.contains("+y looks down"));
        assert_eq!(shot["inputs"], simulate["inputs"]);
        assert!(SIMULATE_DESCRIPTION.contains("game interface"));
    }

    #[test]
    fn screenshot_views_are_parsed_before_the_run() {
        let parse = |view: &str, args: Json| Shot::parse(view, &args);
        assert_eq!(parse("game", json!({})), Ok(Shot::Game));
        assert_eq!(
            parse("free", json!({"eye": [1, 2, 3], "target": "$0", "fov": 40})),
            Ok(Shot::Free {
                eye: [1., 2., 3.],
                target: Aim::Object("$0".into()),
                fov: 40.,
            })
        );
        assert_eq!(
            parse("free", json!({"eye": [0, 5, 5], "target": [0, 0, 0]})),
            Ok(Shot::Free {
                eye: [0., 5., 5.],
                target: Aim::Point([0.; 3]),
                fov: 60.,
            })
        );
        assert_eq!(
            parse("top", json!({"ids": ["a", "b"]})),
            Ok(Shot::Top(vec!["a".into(), "b".into()]))
        );
        assert_eq!(parse("side", json!({})), Ok(Shot::Side(Vec::new())));
        for (view, args, expected) in [
            ("free", json!({"target": [0, 0, 0]}), "eye"),
            ("free", json!({"eye": [0, 1]}), "eye precisa"),
            ("free", json!({"eye": [0, 1, 2]}), "target"),
            (
                "free",
                json!({"eye": [0, 1, 2], "target": [1, 1, 1], "fov": "largo"}),
                "fov",
            ),
            ("top", json!({"ids": "a"}), "ids"),
            ("perto", json!({}), "Vista desconhecida"),
        ] {
            let error = parse(view, args).unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn screenshot_targets_and_framing_find_objects_created_by_setup() {
        let mut project = editing::blank_project("Vistas", SceneKind::ThreeD).unwrap();
        let start = project.start_scene.clone();
        let setup = command::parse(&json!([
            {"op": "create_entity", "kind": {"type": "shape", "shape": "Cube"},
             "name": "Alvo", "position": [4.0, 1.0, -6.0]},
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Grupo",
             "position": [-3.0, 2.0, 0.0]},
        ]))
        .unwrap();
        let applied = command::apply(&mut project, &start, &setup).unwrap();
        let runtime = Runtime::new(&project, &start).unwrap();
        let free = |target: Aim| {
            Shot::Free {
                eye: [0., 5., 5.],
                target,
                fov: 60.,
            }
            .resolve(&runtime, &applied)
        };
        let ShotView::Free { target, .. } = free(Aim::Object("$0".into())).unwrap() else {
            panic!("free view expected");
        };
        assert!(target.abs_diff_eq(Vec3::new(4., 1., -6.), 1e-4), "{target}");
        let ShotView::Free { target, .. } = free(Aim::Object("$1".into())).unwrap() else {
            panic!("free view expected");
        };
        assert!(
            target.abs_diff_eq(Vec3::new(-3., 2., 0.), 1e-4),
            "a group by its position"
        );
        assert!(
            free(Aim::Object("fantasma".into()))
                .unwrap_err()
                .contains("fantasma")
        );
        assert!(free(Aim::Object("$5".into())).unwrap_err().contains("$5"));
        let ShotView::Top(bounds) = Shot::Top(vec!["$0".into()])
            .resolve(&runtime, &applied)
            .unwrap()
        else {
            panic!("top view expected");
        };
        assert!(
            ((bounds.min + bounds.max) * 0.5).abs_diff_eq(Vec3::new(4., 1., -6.), 1e-4),
            "{bounds:?}"
        );
        assert!(
            Shot::Side(vec!["$1".into()])
                .resolve(&runtime, &applied)
                .unwrap_err()
                .contains("forma"),
            "a group alone has nothing to frame"
        );
    }

    #[test]
    fn simulate_reports_the_interface_in_the_view_size_asked() {
        let dir = std::env::temp_dir().join(format!("oxy-mcp-ui-{}", oxy_core::document::new_id()));
        let file = dir.join(persistence::PROJECT_FILE);
        let mut project = editing::blank_project("Interface", SceneKind::ThreeD).unwrap();
        let mut timer = Entity::new("Cronômetro", None);
        timer.attributes.insert("tempo".into(), Value::Number(3.));
        timer.ui = Some(UiElement {
            text: "Tempo: {value}".into(),
            anchor: UiAnchor::BottomRight,
            binding_attribute: "tempo".into(),
            ..Default::default()
        });
        project.scenes[0].entities.push(timer);
        persistence::save_project(&file, &project).unwrap();
        let project_arg = dir.display().to_string();
        let report = |extra: Json| -> Result<Json, String> {
            let mut args = json!({"project": project_arg, "seconds": 0.1});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            Tools::default()
                .call("oxy_simulate", &args)
                .map(|output| serde_json::from_str(&output.text).unwrap())
        };
        let default = report(json!({})).unwrap();
        assert_eq!(default["ui"][0]["text"], "Tempo: 3");
        assert_eq!(default["ui"][0]["rect"], json!([756.0, 480.0, 180.0, 36.0]));
        let small = report(json!({"width": 640, "height": 360})).unwrap();
        assert_eq!(small["ui"][0]["rect"], json!([436.0, 300.0, 180.0, 36.0]));
        assert!(report(json!({"ui": false})).unwrap().get("ui").is_none());
        let error = report(json!({"width": 5})).unwrap_err();
        assert!(error.contains("16 e 4096"), "{error}");
        // A bad camera fails before anything runs.
        let error = Tools::default()
            .call(
                "oxy_screenshot",
                &json!({"project": project_arg, "view": "free", "eye": [0, 1, 2]}),
            )
            .unwrap_err();
        assert!(error.contains("target"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The whole capture on files, with a real GPU: the game view lists the interface, the
    /// free and top views report their camera, and the collider view changes the image.
    #[test]
    #[ignore = "Requires a native WGPU adapter; run explicitly for graphics validation"]
    fn native_screenshot_views_on_files() {
        use oxy_core::physics3d::{Collider3d, CollisionShape};
        let dir =
            std::env::temp_dir().join(format!("oxy-mcp-shot-{}", oxy_core::document::new_id()));
        let file = dir.join(persistence::PROJECT_FILE);
        let mut project = editing::blank_project("Capturas", SceneKind::ThreeD).unwrap();
        let mut floor = Entity::new("Chão", Some(Primitive::Cube));
        floor.dimensions = [20., 1., 20.];
        floor.transform.position = [0., -0.5, 0.];
        floor.physics3d = Some(Collider3d {
            shape: CollisionShape::Box {
                size: floor.dimensions,
            },
            ..Default::default()
        });
        // Invisible: only the collider view shows it.
        let mut checkpoint = Entity::new("Ponto de controle", None);
        checkpoint.transform.position = [3., 1., 3.];
        checkpoint.physics3d = Some(Collider3d {
            shape: CollisionShape::Box { size: [2.; 3] },
            sensor: true,
            ..Default::default()
        });
        let mut label = Entity::new("Placar", None);
        label.ui = Some(UiElement {
            text: "Pontos".into(),
            ..Default::default()
        });
        project.scenes[0]
            .entities
            .extend([floor, checkpoint, label]);
        persistence::save_project(&file, &project).unwrap();
        let mut tools = Tools::default();
        let project_arg = dir.display().to_string();
        let mut shot = |args: Json| {
            let mut all = json!({"project": project_arg, "width": 320, "height": 240});
            all.as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            let output = tools.call("oxy_screenshot", &all).unwrap();
            let text: Json = serde_json::from_str(&output.text).unwrap();
            (text, output.image.unwrap())
        };
        let (game, _) = shot(json!({}));
        assert_eq!(game["view"], "game");
        assert_eq!(game["ui"][0]["text"], "Pontos");
        assert_eq!(game["colliders"], false);
        let (top, plain) = shot(json!({"view": "top"}));
        assert_eq!(top["camera"]["projection"], "orthographic");
        assert!(
            top.get("ui").is_none(),
            "only the game view lists the interface"
        );
        let (_, with) = shot(json!({"view": "top", "colliders": true}));
        assert_ne!(
            plain, with,
            "the collider view draws the invisible checkpoint"
        );
        let (free, _) = shot(json!({"view": "free", "eye": [10, 8, 10],
                                    "target": [0, 0, 0], "fov": 45}));
        assert_eq!(free["camera"]["projection"], "perspective");
        assert_eq!(free["camera"]["fov"], 45.0);
        assert_eq!(free["camera"]["position"], json!([10.0, 8.0, 10.0]));
        let (side, _) = shot(json!({"view": "side", "seconds": 0.5}));
        assert_eq!(side["time"], 0.5);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
