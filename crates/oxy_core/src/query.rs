//! Read-only views of a project as JSON, for agents and tools: a summary, the object tree of
//! a scene, one object in full, the catalog of logic nodes and components, and name search.
use crate::{
    authoring::{self, ComponentKind},
    document::*,
    graph::{self, PortType},
};
use serde_json::{Value as Json, json};

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

pub fn project_summary(project: &Project) -> Json {
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
        "input_actions": project.input_bindings,
    })
}

/// Object tree of a scene: each node has ID, name, kind, components, logic size and children.
pub fn scene_tree(project: &Project, scene: &str) -> Result<Json, String> {
    let scene = project
        .scene(scene)
        .ok_or_else(|| format!("Cena {scene} não existe"))?;
    fn children(scene: &Scene, parent: Option<&str>) -> Vec<Json> {
        scene
            .entities
            .iter()
            .filter(|e| e.parent.as_deref() == parent)
            .map(|e| {
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
                let kids = children(scene, Some(&e.id));
                if !kids.is_empty() {
                    node["children"] = json!(kids);
                }
                node
            })
            .collect()
    }
    Ok(json!({
        "id": scene.id,
        "name": scene.name,
        "kind": kind_name(scene.kind),
        "objects": children(scene, None),
    }))
}

/// One object in its document form, plus the components it could still receive.
pub fn entity(project: &Project, scene: &str, id: &str) -> Result<Json, String> {
    let scene = project
        .scene(scene)
        .ok_or_else(|| format!("Cena {scene} não existe"))?;
    let e = scene
        .entity(id)
        .ok_or_else(|| format!("Objeto {id} não existe nesta cena"))?;
    let mut value = serde_json::to_value(e).map_err(|e| e.to_string())?;
    value["rotation_degrees"] = json!(e.transform.rotation.map(f32::to_degrees));
    value["can_add_components"] = json!(authoring::available(scene.kind, e));
    Ok(value)
}

/// Objects whose name contains `text` (case-insensitive), in every scene.
pub fn find(project: &Project, text: &str) -> Json {
    let needle = text.to_lowercase();
    json!(
        project
            .scenes
            .iter()
            .flat_map(|s| s.entities.iter().map(move |e| (s, e)))
            .filter(|(_, e)| e.name.to_lowercase().contains(&needle))
            .map(|(s, e)| json!({
                "scene": s.id,
                "scene_name": s.name,
                "id": e.id,
                "name": e.name,
                "kind": entity_kind(e),
            }))
            .collect::<Vec<_>>()
    )
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
                "params": op.params.iter().map(|p| json!({
                    "id": p.id, "label": p.label, "default": p.default,
                })).collect::<Vec<_>>(),
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
        "presets": ["first_person", "third_person", "platform"],
        "value_format": "Attribute and node parameter values are tagged: {\"Number\": 1.0}, {\"Text\": \"...\"}, {\"Bool\": true}, {\"Object\": \"<id>\" or null}, {\"Vector2\": [x, y]}, {\"Vector3\": [x, y, z]}, {\"Surface\": \"<id>\" or null}.",
    })
}
