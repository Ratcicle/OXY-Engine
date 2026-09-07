//! Versioned editor documents. Units are metres, +Y is up, +Z faces the viewer.
//! Euler angles are radians in XYZ order. Transforms use an explicit local pivot.
use crate::{animation::Clip, graph::Graph};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SCHEMA_VERSION: u32 = 1;
pub type Id = String;
pub fn new_id() -> Id {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Object(Option<Id>),
}
impl Value {
    pub fn number(&self) -> Option<f64> {
        if let Self::Number(v) = self {
            Some(*v)
        } else {
            None
        }
    }
    pub fn boolean(&self) -> Option<bool> {
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }
    pub fn object(&self) -> Option<&str> {
        if let Self::Object(Some(v)) = self {
            Some(v)
        } else {
            None
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneKind {
    TwoD,
    ThreeD,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Primitive {
    Rectangle,
    Circle,
    Sprite,
    Cube,
    Sphere,
    Cylinder,
    Plane,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub pivot: [f32; 3],
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.; 3],
            rotation: [0.; 3],
            scale: [1.; 3],
            pivot: [0.; 3],
        }
    }
}
impl Transform {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::from(self.scale),
            self.quaternion(),
            Vec3::from(self.position),
        ) * Mat4::from_translation(-Vec3::from(self.pivot))
    }
    pub fn quaternion(&self) -> Quat {
        Quat::from_euler(
            EulerRot::XYZ,
            self.rotation[0],
            self.rotation[1],
            self.rotation[2],
        )
    }
    pub fn from_matrix(matrix: Mat4, pivot: [f32; 3]) -> Self {
        let adjusted = matrix * Mat4::from_translation(Vec3::from(pivot));
        let (scale, rot, position) = adjusted.to_scale_rotation_translation();
        let (x, y, z) = rot.to_euler(EulerRot::XYZ);
        Self {
            position: position.to_array(),
            rotation: [x, y, z],
            scale: scale.to_array(),
            pivot,
        }
    }
    pub fn finite(&self) -> bool {
        self.position
            .iter()
            .chain(&self.rotation)
            .chain(&self.scale)
            .chain(&self.pivot)
            .all(|v| v.is_finite())
            && self.scale.iter().all(|v| v.abs() > 0.00001)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub color: [f32; 4],
    pub texture: Option<Id>,
    pub nearest: bool,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            color: [0.45, 0.7, 0.8, 1.],
            texture: None,
            nearest: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collider {
    pub size: [f32; 3],
    pub offset: [f32; 3],
    pub is_trigger: bool,
    pub enabled: bool,
}
impl Default for Collider {
    fn default() -> Self {
        Self {
            size: [1.; 3],
            offset: [0.; 3],
            is_trigger: false,
            enabled: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    pub speed: f32,
    pub jump: f32,
    pub gravity: f32,
    pub enabled: bool,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            speed: 5.,
            jump: 8.,
            gravity: 22.,
            enabled: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub active: bool,
    pub orthographic_size: f32,
    pub fov: f32,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            active: true,
            orthographic_size: 7.5,
            fov: 60.,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiKind {
    Text,
    Image,
    Button,
    Bar,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiElement {
    pub kind: UiKind,
    pub anchor: UiAnchor,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
    pub color: [f32; 4],
    pub texture: Option<Id>,
    pub binding_object: Option<Id>,
    pub binding_attribute: String,
    pub max_value: f64,
}
impl Default for UiElement {
    fn default() -> Self {
        Self {
            kind: UiKind::Text,
            anchor: UiAnchor::TopLeft,
            position: [24., 24.],
            size: [180., 36.],
            text: "Texto".into(),
            color: [1.; 4],
            texture: None,
            binding_object: None,
            binding_attribute: String::new(),
            max_value: 100.,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: Id,
    pub name: String,
    pub parent: Option<Id>,
    pub transform: Transform,
    pub primitive: Option<Primitive>,
    pub dimensions: [f32; 3],
    pub segments: u32,
    pub material: Material,
    pub visible: bool,
    pub layer: i32,
    pub attributes: BTreeMap<String, Value>,
    pub collider: Option<Collider>,
    pub controller: Option<Controller>,
    pub camera: Option<Camera>,
    pub ui: Option<UiElement>,
    pub graph: Graph,
    pub clips: Vec<Clip>,
    pub model_source: Option<Id>,
}
impl Entity {
    pub fn new(name: impl Into<String>, primitive: Option<Primitive>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            parent: None,
            transform: Transform::default(),
            primitive,
            dimensions: [1.; 3],
            segments: 24,
            material: Material::default(),
            visible: true,
            layer: 0,
            attributes: BTreeMap::new(),
            collider: None,
            controller: None,
            camera: None,
            ui: None,
            graph: Graph::default(),
            clips: Vec::new(),
            model_source: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: Id,
    pub name: String,
    pub kind: SceneKind,
    pub entities: Vec<Entity>,
}
impl Scene {
    pub fn new(name: impl Into<String>, kind: SceneKind) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            kind,
            entities: Vec::new(),
        }
    }
    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }
    pub fn entity_mut(&mut self, id: &str) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }
    pub fn world_matrix(&self, id: &str) -> Result<Mat4, String> {
        let mut current = Some(id);
        let mut visited = HashSet::new();
        let mut chain = Vec::new();
        while let Some(key) = current {
            if !visited.insert(key) {
                return Err("Ciclo na hierarquia".into());
            }
            let e = self
                .entity(key)
                .ok_or_else(|| format!("Objeto ausente: {key}"))?;
            chain.push(e.transform.matrix());
            current = e.parent.as_deref();
        }
        Ok(chain.into_iter().rev().fold(Mat4::IDENTITY, |a, b| a * b))
    }
    pub fn descendants(&self, id: &str) -> Vec<Id> {
        let mut result = vec![id.to_owned()];
        let mut index = 0;
        while index < result.len() {
            let parent = result[index].clone();
            for e in &self.entities {
                if e.parent.as_deref() == Some(&parent) && !result.contains(&e.id) {
                    result.push(e.id.clone());
                }
            }
            index += 1;
        }
        result
    }
    pub fn reparent(
        &mut self,
        id: &str,
        parent: Option<Id>,
        preserve_world: bool,
    ) -> Result<(), String> {
        if self.entity(id).is_none() {
            return Err("Objeto não encontrado".into());
        }
        if let Some(p) = &parent {
            if self.entity(p).is_none() {
                return Err("Pai não encontrado".into());
            }
            if self.descendants(id).contains(p) {
                return Err("O parentesco criaria um ciclo".into());
            }
        }
        crate::spatial::check_reparent_animation(self, id, parent.as_deref())?;
        let new_local = if preserve_world {
            let world = self.world_matrix(id)?;
            let pm = match &parent {
                Some(p) => self.world_matrix(p)?,
                None => Mat4::IDENTITY,
            };
            if pm.determinant().abs() < 1e-8 {
                return Err("Transformação do pai não pode ser invertida".into());
            }
            Some(pm.inverse() * world)
        } else {
            None
        };
        let next_transform = if let Some(matrix) = new_local {
            let transform =
                Transform::from_matrix(matrix, self.entity(id).unwrap().transform.pivot);
            if !transform.finite() || !transform.matrix().abs_diff_eq(matrix, 0.0001) {
                return Err("Esse parentesco introduziria cisalhamento; ajuste a escala não uniforme do pai ou use transformação local".into());
            }
            Some(transform)
        } else {
            None
        };
        let e = self.entity_mut(id).unwrap();
        e.parent = parent;
        if let Some(transform) = next_transform {
            e.transform = transform;
        }
        Ok(())
    }
    pub fn remove_subtree(&mut self, id: &str) -> Vec<Entity> {
        let ids: HashSet<_> = self.descendants(id).into_iter().collect();
        let mut removed = Vec::new();
        self.entities.retain(|e| {
            if ids.contains(&e.id) {
                removed.push(e.clone());
                false
            } else {
                true
            }
        });
        for e in &mut self.entities {
            for v in e.attributes.values_mut() {
                if let Value::Object(Some(target)) = v
                    && ids.contains(target)
                {
                    *v = Value::Object(None)
                }
            }
            if let Some(ui) = &mut e.ui
                && ui.binding_object.as_ref().is_some_and(|x| ids.contains(x))
            {
                ui.binding_object = None
            }
            for clip in &mut e.clips {
                clip.tracks.retain(|track| !ids.contains(&track.target));
            }
            // Graph references intentionally retain the missing stable ID so validation
            // identifies the affected node. Clearing an action target would mean "self"
            // and could silently turn damage/removal onto a different object.
        }
        removed
    }
    pub fn duplicate_subtree(&mut self, id: &str) -> Result<Id, String> {
        if self.entity(id).is_none() {
            return Err("Objeto não encontrado".into());
        }
        let selected: HashSet<_> = self.descendants(id).into_iter().collect();
        let mut copies: Vec<_> = self
            .entities
            .iter()
            .filter(|e| selected.contains(&e.id))
            .cloned()
            .collect();
        let map: HashMap<_, _> = copies.iter().map(|e| (e.id.clone(), new_id())).collect();
        remap_entities(&mut copies, &map)?;
        let root = map[id].clone();
        if let Some(e) = copies.iter_mut().find(|e| e.id == root) {
            e.name.push_str(" (cópia)");
        }
        translate_entity_and_tracks(&mut copies, &root, [0.5, 0., 0.]);
        self.entities.extend(copies);
        Ok(root)
    }
}
/// Apply a coordinate-origin change to a copied root and every track targeting it,
/// including clips owned by a descendant. Child-local tracks do not change.
fn translate_entity_and_tracks(entities: &mut [Entity], target: &str, delta: [f32; 3]) {
    let delta = Vec3::from(delta);
    for entity in entities {
        if entity.id == target {
            entity.transform.position = (Vec3::from(entity.transform.position) + delta).to_array();
        }
        for clip in &mut entity.clips {
            for track in &mut clip.tracks {
                if track.target == target {
                    for key in &mut track.keyframes {
                        key.transform.position =
                            (Vec3::from(key.transform.position) + delta).to_array();
                    }
                }
            }
        }
    }
}
/// Remaps only exact stable identifiers; names and relative paths remain untouched.
pub fn remap_entities(entities: &mut [Entity], map: &HashMap<Id, Id>) -> Result<(), String> {
    let mut map = map.clone();
    for e in entities.iter() {
        for clip in &e.clips {
            map.entry(clip.id.clone()).or_insert_with(new_id);
        }
        for node in &e.graph.nodes {
            map.entry(node.id.clone()).or_insert_with(new_id);
        }
    }
    for e in entities {
        e.id = map.get(&e.id).cloned().unwrap_or_else(|| e.id.clone());
        if let Some(p) = &mut e.parent
            && let Some(v) = map.get(p)
        {
            *p = v.clone()
        }
        for value in e.attributes.values_mut() {
            if let Value::Object(Some(id)) = value
                && let Some(v) = map.get(id)
            {
                *id = v.clone()
            }
        }
        if let Some(ui) = &mut e.ui
            && let Some(id) = &mut ui.binding_object
            && let Some(v) = map.get(id)
        {
            *id = v.clone()
        }
        for clip in &mut e.clips {
            clip.id = map[&clip.id].clone();
            for track in &mut clip.tracks {
                if let Some(v) = map.get(&track.target) {
                    track.target = v.clone()
                }
            }
        }
        let mut json = serde_json::to_value(&e.graph).map_err(|e| e.to_string())?;
        remap_json(&mut json, &map);
        e.graph = serde_json::from_value(json).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn remap_json(value: &mut serde_json::Value, map: &HashMap<Id, Id>) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(v) = map.get(s) {
                *s = v.clone()
            }
        }
        serde_json::Value::Array(a) => {
            for v in a {
                remap_json(v, map)
            }
        }
        serde_json::Value::Object(o) => {
            for v in o.values_mut() {
                remap_json(v, map)
            }
        }
        _ => {}
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    Texture,
    Audio,
    Model,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: Id,
    pub name: String,
    pub path: String,
    pub kind: AssetKind,
    pub model: Option<Vec<Entity>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub id: Id,
    pub name: String,
    pub start_scene: Id,
    pub scenes: Vec<Scene>,
    pub assets: Vec<Asset>,
    pub input_bindings: BTreeMap<String, String>,
}
impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        let scene = Scene::new("Cena 2D", SceneKind::TwoD);
        Self {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            name: name.into(),
            start_scene: scene.id.clone(),
            scenes: vec![scene],
            assets: Vec::new(),
            input_bindings: BTreeMap::from([
                ("mover_esquerda".into(), "A".into()),
                ("mover_direita".into(), "D".into()),
                ("pular".into(), "Space".into()),
                ("atacar".into(), "J".into()),
                ("interagir".into(), "E".into()),
                ("mover_frente".into(), "W".into()),
                ("mover_tras".into(), "S".into()),
            ]),
        }
    }
    pub fn scene(&self, id: &str) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.id == id)
    }
    pub fn scene_mut(&mut self, id: &str) -> Option<&mut Scene> {
        self.scenes.iter_mut().find(|s| s.id == id)
    }
    pub fn asset(&self, id: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }
    /// Stores the editable hierarchy in a shared model asset. Instances remain explicit.
    pub fn save_model(
        &mut self,
        scene_id: &str,
        entity_id: &str,
        name: impl Into<String>,
    ) -> Result<Id, String> {
        let scene = self.scene(scene_id).ok_or("Cena não encontrada")?;
        if scene.entity(entity_id).is_none() {
            return Err("Objeto não encontrado".into());
        }
        let selected: HashSet<_> = scene.descendants(entity_id).into_iter().collect();
        let mut pieces: Vec<_> = scene
            .entities
            .iter()
            .filter(|e| selected.contains(&e.id))
            .cloned()
            .collect();
        let map: HashMap<_, _> = pieces.iter().map(|e| (e.id.clone(), new_id())).collect();
        remap_entities(&mut pieces, &map)?;
        let root_id = &map[entity_id];
        let offset = pieces
            .iter()
            .find(|piece| &piece.id == root_id)
            .map(|root| (-Vec3::from(root.transform.position)).to_array())
            .unwrap_or([0.; 3]);
        translate_entity_and_tracks(&mut pieces, root_id, offset);
        if let Some(root) = pieces.iter_mut().find(|e| e.id == map[entity_id]) {
            root.parent = None;
        }
        for piece in &mut pieces {
            piece.model_source = None;
        }
        let id = new_id();
        self.assets.push(Asset {
            id: id.clone(),
            name: name.into(),
            path: String::new(),
            kind: AssetKind::Model,
            model: Some(pieces),
        });
        if let Err(error) = validate_project(self) {
            self.assets.pop();
            return Err(error);
        }
        Ok(id)
    }
    pub fn instantiate_model(&mut self, asset_id: &str, scene_id: &str) -> Result<Id, String> {
        let mut model = self
            .asset(asset_id)
            .and_then(|a| a.model.clone())
            .ok_or("Modelo não encontrado")?;
        let map: HashMap<_, _> = model.iter().map(|e| (e.id.clone(), new_id())).collect();
        remap_entities(&mut model, &map)?;
        let mut roots: Vec<_> = model
            .iter()
            .filter(|e| e.parent.is_none())
            .map(|e| e.id.clone())
            .collect();
        if roots.len() != 1 {
            let group = Entity::new("Modelo", None);
            let group_id = group.id.clone();
            for e in &mut model {
                if e.parent.is_none() {
                    e.parent = Some(group_id.clone())
                }
            }
            model.push(group);
            roots = vec![group_id];
        }
        let root = roots[0].clone();
        for e in &mut model {
            e.model_source = Some(asset_id.to_owned())
        }
        self.scene_mut(scene_id)
            .ok_or("Cena não encontrada")?
            .entities
            .extend(model);
        Ok(root)
    }
}
pub fn validate_project(project: &Project) -> Result<(), String> {
    if project.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "Versão de projeto {} incompatível; esta OXY Engine aceita {}",
            project.schema_version, SCHEMA_VERSION
        ));
    }
    if project.scene(&project.start_scene).is_none() {
        return Err("A cena inicial não existe".into());
    }
    if project.id.is_empty() || project.name.trim().is_empty() {
        return Err("Projeto deve possuir identificador e nome".into());
    }
    let mut identifiers = HashSet::new();
    identifiers.insert(project.id.clone());
    let assets: HashMap<_, _> = project.assets.iter().map(|a| (a.id.as_str(), a)).collect();
    for asset in &project.assets {
        if asset.id.is_empty() || !identifiers.insert(asset.id.clone()) {
            return Err("Identificador de asset vazio ou duplicado".into());
        }
        let path = std::path::Path::new(&asset.path);
        if !asset.path.is_empty()
            && (path.is_absolute()
                || path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::Prefix(_)
                    )
                })
                || asset.path.contains(':'))
        {
            return Err(format!(
                "Asset {} deve usar caminho relativo seguro",
                asset.name
            ));
        }
        if asset.kind == AssetKind::Model && asset.model.is_none() {
            return Err(format!("Modelo {} não contém peças", asset.name));
        }
    }
    for scene in &project.scenes {
        if scene.id.is_empty() || !identifiers.insert(scene.id.clone()) {
            return Err("Identificador de cena vazio ou duplicado".into());
        }
        validate_scene(scene, &assets)?;
        for e in &scene.entities {
            if !identifiers.insert(e.id.clone()) {
                return Err(format!("Identificador de objeto duplicado: {}", e.name));
            }
        }
        validate_action_references(project, scene)?;
    }
    for asset in &project.assets {
        if let Some(model) = &asset.model {
            let scene = Scene {
                id: asset.id.clone(),
                name: asset.name.clone(),
                kind: SceneKind::ThreeD,
                entities: model.clone(),
            };
            validate_scene(&scene, &assets)?
        }
    }
    Ok(())
}
fn validate_scene(scene: &Scene, assets: &HashMap<&str, &Asset>) -> Result<(), String> {
    let ids: HashSet<_> = scene.entities.iter().map(|e| e.id.as_str()).collect();
    if ids.len() != scene.entities.len() {
        return Err(format!("IDs duplicados em {}", scene.name));
    }
    let owned_ids: HashSet<_> = ids.iter().map(|id| (*id).to_owned()).collect();
    for e in &scene.entities {
        if e.id.is_empty()
            || !e.transform.finite()
            || !e.dimensions.iter().all(|v| v.is_finite() && *v > 0.)
        {
            return Err(format!("Transformação ou dimensões inválidas: {}", e.name));
        }
        if !(3..=256).contains(&e.segments) {
            return Err(format!("Segmentos de {} devem estar entre 3 e 256", e.name));
        }
        if !e
            .material
            .color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return Err(format!("Cor inválida: {}", e.name));
        }
        scene.world_matrix(&e.id)?;
        for texture in [
            e.material.texture.as_ref(),
            e.ui.as_ref().and_then(|u| u.texture.as_ref()),
        ]
        .into_iter()
        .flatten()
        {
            if !assets
                .get(texture.as_str())
                .is_some_and(|a| a.kind == AssetKind::Texture)
            {
                return Err(format!("Textura ausente para {}: {texture}", e.name));
            }
        }
        if let Some(model) = &e.model_source
            && !assets
                .get(model.as_str())
                .is_some_and(|a| a.kind == AssetKind::Model)
        {
            return Err(format!("Modelo de origem ausente: {}", e.name));
        }
        for (key, value) in &e.attributes {
            match value {
                Value::Number(v) if !v.is_finite() => {
                    return Err(format!("Número inválido em {}.{key}", e.name));
                }
                Value::Object(Some(id)) if !ids.contains(id.as_str()) => {
                    return Err(format!("Referência quebrada em {}.{key}: {id}", e.name));
                }
                _ => {}
            }
        }
        if let Some(ui) = &e.ui {
            if ui
                .binding_object
                .as_ref()
                .is_some_and(|id| !ids.contains(id.as_str()))
            {
                return Err(format!("Vínculo de interface ausente: {}", e.name));
            }
            if !ui.max_value.is_finite()
                || ui.max_value <= 0.
                || !ui.position.iter().all(|v| v.is_finite())
                || !ui.size.iter().all(|v| v.is_finite() && *v > 0.)
                || !ui
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            {
                return Err(format!("Valores de interface inválidos: {}", e.name));
            }
        }
        if let Some(c) = &e.collider
            && (!c.size.iter().all(|v| v.is_finite() && *v > 0.)
                || !c.offset.iter().all(|v| v.is_finite()))
        {
            return Err(format!("Colisor inválido: {}", e.name));
        }
        if let Some(c) = &e.controller
            && ![c.speed, c.jump, c.gravity]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
        {
            return Err(format!("Controlador inválido: {}", e.name));
        }
        if let Some(c) = &e.camera
            && (!c.orthographic_size.is_finite()
                || c.orthographic_size <= 0.
                || !c.fov.is_finite()
                || !(1.0..179.0).contains(&c.fov))
        {
            return Err(format!("Câmera inválida: {}", e.name));
        }
        for clip in &e.clips {
            clip.validate(&ids)?;
        }
        e.graph
            .validate(&owned_ids)
            .map_err(|error| format!("Lógica de {}: {error}", e.name))?;
    }
    Ok(())
}
fn validate_action_references(project: &Project, scene: &Scene) -> Result<(), String> {
    let clips: HashSet<_> = scene
        .entities
        .iter()
        .flat_map(|e| e.clips.iter().map(|clip| clip.id.as_str()))
        .collect();
    for e in &scene.entities {
        for node in &e.graph.nodes {
            let missing = match node.operation.as_str() {
                "action.scene" => {
                    let id = node.text("scene");
                    !id.is_empty() && project.scene(id).is_none()
                }
                "action.sound" => {
                    let id = node.text("asset");
                    !id.is_empty()
                        && !project
                            .asset(id)
                            .is_some_and(|a| a.kind == AssetKind::Audio)
                }
                "action.animation" => {
                    let id = node.text("clip");
                    !id.is_empty() && !clips.contains(id)
                }
                _ => false,
            };
            if missing {
                return Err(format!(
                    "Referência quebrada: objeto {}, nó {} ({})",
                    e.name, node.id, node.operation
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_stable_ids() {
        let mut p = Project::new("Teste");
        let e = Entity::new("Caixa", Some(Primitive::Cube));
        let id = e.id.clone();
        p.scenes[0].entities.push(e);
        let s = serde_json::to_string(&p).unwrap();
        let q: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(p, q);
        assert_eq!(q.scenes[0].entities[0].id, id);
        validate_project(&q).unwrap();
    }
    #[test]
    fn cycles_are_rejected_and_world_is_preserved() {
        let mut s = Scene::new("s", SceneKind::ThreeD);
        let mut a = Entity::new("a", None);
        a.transform.position = [3., 0., 0.];
        let b = Entity::new("b", None);
        let aid = a.id.clone();
        let bid = b.id.clone();
        s.entities = vec![a, b];
        let before = s.world_matrix(&bid).unwrap();
        s.reparent(&bid, Some(aid.clone()), true).unwrap();
        assert!(s.world_matrix(&bid).unwrap().abs_diff_eq(before, 1e-5));
        assert!(s.reparent(&aid, Some(bid), false).is_err());
    }
    #[test]
    fn duplicate_remaps_children_and_attributes() {
        let mut s = Scene::new("s", SceneKind::TwoD);
        let mut root = Entity::new("root", None);
        let mut child = Entity::new("child", Some(Primitive::Rectangle));
        child.parent = Some(root.id.clone());
        root.attributes
            .insert("child".into(), Value::Object(Some(child.id.clone())));
        let id = root.id.clone();
        s.entities = vec![root, child];
        let copied = s.duplicate_subtree(&id).unwrap();
        assert_eq!(s.entities.len(), 4);
        let ref_id = s.entity(&copied).unwrap().attributes["child"]
            .object()
            .unwrap();
        assert_eq!(
            s.entity(ref_id).unwrap().parent.as_deref(),
            Some(copied.as_str())
        );
    }
    #[test]
    fn unsafe_asset_path_and_missing_reference_rejected() {
        let mut p = Project::new("p");
        p.assets.push(Asset {
            id: new_id(),
            name: "x".into(),
            path: "../outside.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        assert!(validate_project(&p).is_err());
        p.assets.clear();
        let mut e = Entity::new("e", None);
        e.attributes
            .insert("target".into(), Value::Object(Some(new_id())));
        p.scenes[0].entities.push(e);
        assert!(validate_project(&p).is_err());
    }
    #[test]
    fn duplicate_remaps_animation_action_to_duplicate_clip() {
        let mut scene = Scene::new("s", SceneKind::ThreeD);
        let mut e = Entity::new("actor", None);
        let clip = Clip::new("Ataque");
        let mut node = crate::graph::Node::new("action.animation", [0., 0.]);
        node.params
            .insert("clip".into(), Value::Text(clip.id.clone()));
        e.clips.push(clip);
        e.graph.nodes.push(node);
        let id = e.id.clone();
        scene.entities.push(e);
        let duplicate = scene.duplicate_subtree(&id).unwrap();
        let e = scene.entity(&duplicate).unwrap();
        assert_eq!(e.graph.nodes[0].text("clip"), e.clips[0].id);
        assert_ne!(e.clips[0].id, scene.entity(&id).unwrap().clips[0].id);
        assert_ne!(
            e.graph.nodes[0].id,
            scene.entity(&id).unwrap().graph.nodes[0].id
        );
    }
    #[test]
    fn editable_model_roundtrip_and_instantiation() {
        let mut p = Project::new("models");
        let root = Entity::new("Assembly", None);
        let mut child = Entity::new("Arm", Some(Primitive::Cube));
        child.parent = Some(root.id.clone());
        child.transform.pivot = [0., 0.5, 0.];
        let root_id = root.id.clone();
        let scene_id = p.scenes[0].id.clone();
        p.scenes[0].entities = vec![root, child];
        let model = p.save_model(&scene_id, &root_id, "Boneco").unwrap();
        let copy = p.instantiate_model(&model, &scene_id).unwrap();
        assert_eq!(p.scene(&scene_id).unwrap().descendants(&copy).len(), 2);
        validate_project(&p).unwrap();
        let loaded: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(
            loaded.asset(&model).unwrap().model.as_ref().unwrap()[1]
                .transform
                .pivot,
            [0., 0.5, 0.]
        );
    }
    fn translated_animation_scene() -> (Project, Id, Id) {
        use crate::animation::{Interpolation, Keyframe};
        let mut project = Project::new("Animação da raiz");
        let scene_id = project.scenes[0].id.clone();
        let mut root = Entity::new("Raiz", None);
        root.transform.position = [10., 2., -3.];
        let mut child = Entity::new("Peça", Some(Primitive::Cube));
        child.parent = Some(root.id.clone());
        child.transform.position = [0., 1., 0.];
        let mut clip = Clip::new("Deslocamento");
        clip.insert_key(
            &root.id,
            Keyframe {
                time: 0.,
                transform: root.transform.clone(),
                interpolation: Interpolation::Linear,
            },
        );
        let mut moved = root.transform.clone();
        moved.position[0] += 2.;
        clip.insert_key(
            &root.id,
            Keyframe {
                time: 1.,
                transform: moved,
                interpolation: Interpolation::Linear,
            },
        );
        clip.insert_key(
            &child.id,
            Keyframe {
                time: 0.,
                transform: child.transform.clone(),
                interpolation: Interpolation::Hold,
            },
        );
        root.clips.push(clip.clone());
        clip.id = new_id();
        clip.name = "Clip pertencente à peça".into();
        child.clips.push(clip);
        let root_id = root.id.clone();
        project.scenes[0].entities = vec![root, child];
        (project, scene_id, root_id)
    }
    #[test]
    fn duplicate_offsets_root_keyframes_and_preserves_original() {
        let (mut project, scene_id, root_id) = translated_animation_scene();
        let original = project.scene(&scene_id).unwrap().entities.clone();
        let scene = project.scene_mut(&scene_id).unwrap();
        let copy = scene.duplicate_subtree(&root_id).unwrap();
        assert_eq!(&scene.entities[..2], &original);
        for id in scene.descendants(&copy) {
            let entity = scene.entity(&id).unwrap();
            for clip in &entity.clips {
                let root_track = clip
                    .tracks
                    .iter()
                    .find(|track| track.target == copy)
                    .unwrap();
                assert_eq!(root_track.keyframes[0].transform.position, [10.5, 2., -3.]);
                assert_eq!(root_track.keyframes[1].transform.position, [12.5, 2., -3.]);
                let child_track = clip
                    .tracks
                    .iter()
                    .find(|track| track.target != copy)
                    .unwrap();
                assert_eq!(child_track.keyframes[0].transform.position, [0., 1., 0.]);
            }
        }
        let clip = scene.entity(&copy).unwrap().clips[0].clone();
        crate::animation::sample_clip(scene, &clip, 0.5);
        assert_eq!(
            scene.entity(&copy).unwrap().transform.position,
            [11.5, 2., -3.]
        );
        assert_eq!(scene.entity(&root_id).unwrap(), &original[0]);
    }
    #[test]
    fn saved_model_normalizes_all_root_tracks_without_touching_scene() {
        let (mut project, scene_id, root_id) = translated_animation_scene();
        let original = project.scene(&scene_id).unwrap().entities.clone();
        let model_id = project.save_model(&scene_id, &root_id, "Animado").unwrap();
        assert_eq!(project.scene(&scene_id).unwrap().entities, original);
        let copy = project.instantiate_model(&model_id, &scene_id).unwrap();
        let scene = project.scene_mut(&scene_id).unwrap();
        assert_eq!(scene.entity(&copy).unwrap().transform.position, [0.; 3]);
        for id in scene.descendants(&copy) {
            for clip in &scene.entity(&id).unwrap().clips {
                let root_track = clip
                    .tracks
                    .iter()
                    .find(|track| track.target == copy)
                    .unwrap();
                assert_eq!(root_track.keyframes[0].transform.position, [0.; 3]);
                assert_eq!(root_track.keyframes[1].transform.position, [2., 0., 0.]);
            }
        }
        let clip = scene.entity(&copy).unwrap().clips[0].clone();
        crate::animation::sample_clip(scene, &clip, 0.5);
        assert_eq!(
            scene.entity(&copy).unwrap().transform.position,
            [1., 0., 0.]
        );
        assert_eq!(&scene.entities[..2], &original);
    }
}
