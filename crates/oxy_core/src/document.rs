//! Versioned editor documents. Units are metres, +Y is up, +Z faces the viewer.
//! Euler angles are radians in XYZ order. Transforms use an explicit local pivot.
use crate::{
    animation::Clip,
    graph::{Graph, Reference, node_references},
};
use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SCHEMA_VERSION: u32 = 5;
pub type Id = String;
/// The IDs a copy took: each original object, node and animation (and scene) to its copy.
pub type CopyMap = HashMap<Id, Id>;
pub fn new_id() -> Id {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Object(Option<Id>),
    Vector2([f32; 2]),
    Vector3([f32; 3]),
    Surface(Option<Id>),
}
impl Value {
    pub fn is_finite(&self) -> bool {
        match self {
            Self::Number(v) => v.is_finite(),
            Self::Vector2(v) => v.iter().all(|v| v.is_finite()),
            Self::Vector3(v) => v.iter().all(|v| v.is_finite()),
            _ => true,
        }
    }
    pub fn vector2(&self) -> Option<glam::Vec2> {
        if let Self::Vector2(v) = self {
            Some((*v).into())
        } else {
            None
        }
    }
    pub fn vector3(&self) -> Option<Vec3> {
        if let Self::Vector3(v) = self {
            Some((*v).into())
        } else {
            None
        }
    }
    pub fn surface(&self) -> Option<&str> {
        if let Self::Surface(Some(id)) = self {
            Some(id)
        } else {
            None
        }
    }
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
    Pyramid,
    Cone,
    Tube,
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
    #[serde(default)]
    pub actions: crate::input_actions::MovementActions,
    pub speed: f32,
    pub jump: f32,
    pub gravity: f32,
    pub enabled: bool,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            actions: Default::default(),
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
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: Id,
    pub name: String,
    pub parent: Option<Id>,
    pub transform: Transform,
    pub primitive: Option<Primitive>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<crate::geometry::EditableMesh>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primitive_parameters: Option<crate::geometry::primitives::Parameters>,
    pub dimensions: [f32; 3],
    pub segments: u32,
    pub material: Material,
    pub visible: bool,
    pub layer: i32,
    pub attributes: BTreeMap<String, Value>,
    pub collider: Option<Collider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physics3d: Option<crate::physics3d::Collider3d>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character3d: Option<crate::character::CharacterConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_rig: Option<crate::character::CameraRig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<crate::surface::TranslationPlatform>,
    pub controller: Option<Controller>,
    pub camera: Option<Camera>,
    pub ui: Option<UiElement>,
    pub graph: Graph,
    pub clips: Vec<Clip>,
    pub model_source: Option<Id>,
}
impl Entity {
    pub fn has_geometry(&self) -> bool {
        self.primitive.is_some() || self.mesh.is_some()
    }
    pub fn new(name: impl Into<String>, primitive: Option<Primitive>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            parent: None,
            transform: Transform::default(),
            primitive,
            mesh: None,
            primitive_parameters: None,
            dimensions: [1.; 3],
            segments: 24,
            material: Material::default(),
            visible: true,
            layer: 0,
            attributes: BTreeMap::new(),
            collider: None,
            physics3d: None,
            character3d: None,
            camera_rig: None,
            platform: None,
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
        crate::metrics::count(|c| c.entity_queries += 1);
        self.entities.iter().find(|e| {
            crate::metrics::count(|c| c.entity_scans += 1);
            e.id == id
        })
    }
    pub fn entity_mut(&mut self, id: &str) -> Option<&mut Entity> {
        crate::metrics::count(|c| c.entity_queries += 1);
        self.entities.iter_mut().find(|e| {
            crate::metrics::count(|c| c.entity_scans += 1);
            e.id == id
        })
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
            crate::metrics::count(|c| c.matrices += 1);
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
                crate::metrics::count(|c| c.hierarchy_visits += 1);
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
            if let Some(rig) = &mut e.camera_rig {
                if rig.target.as_ref().is_some_and(|id| ids.contains(id)) {
                    rig.target = None;
                }
                rig.hidden.retain(|id| !ids.contains(id));
            }
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
        self.duplicate_subtree_with(id, &mut |_| new_id())
    }
    /// [`Scene::duplicate_subtree`] with the copies' IDs from `ids`, which gets the ID of each
    /// original object, clip and node. It is asked again for an object whose ID it gave is
    /// taken, so a generator of repeatable IDs moves on to another one.
    pub fn duplicate_subtree_with(
        &mut self,
        id: &str,
        ids: &mut dyn FnMut(&str) -> Id,
    ) -> Result<Id, String> {
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
        let taken: HashSet<&str> = self.entities.iter().map(|e| e.id.as_str()).collect();
        let mut given = HashSet::new();
        let mut map = HashMap::new();
        for copy in &copies {
            let new = (0..64)
                .map(|_| ids(&copy.id))
                .find(|new| !taken.contains(new.as_str()) && given.insert(new.clone()))
                .ok_or_else(|| format!("Sem ID livre para a cópia de {}", copy.name))?;
            map.insert(copy.id.clone(), new);
        }
        remap_entities_with(&mut copies, &map, ids)?;
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
pub fn translate_entity_and_tracks(entities: &mut [Entity], target: &str, delta: [f32; 3]) {
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
/// Remaps only exact stable identifiers; names and relative paths remain untouched. Answers
/// the whole map: `map` plus the new IDs of the clips and nodes.
pub fn remap_entities(entities: &mut [Entity], map: &HashMap<Id, Id>) -> Result<CopyMap, String> {
    remap_entities_with(entities, map, &mut |_| new_id())
}
/// [`remap_entities`] with the new IDs of the clips and nodes `map` leaves out from `ids`,
/// which gets each original ID.
pub fn remap_entities_with(
    entities: &mut [Entity],
    map: &HashMap<Id, Id>,
    ids: &mut dyn FnMut(&str) -> Id,
) -> Result<CopyMap, String> {
    let mut map = map.clone();
    for e in entities.iter() {
        for clip in &e.clips {
            map.entry(clip.id.clone()).or_insert_with(|| ids(&clip.id));
        }
        for node in &e.graph.nodes {
            map.entry(node.id.clone()).or_insert_with(|| ids(&node.id));
        }
    }
    for e in entities {
        e.id = map.get(&e.id).cloned().unwrap_or_else(|| e.id.clone());
        if let Some(rig) = &mut e.camera_rig {
            if let Some(target) = &mut rig.target
                && let Some(next) = map.get(target)
            {
                *target = next.clone();
            }
            for id in &mut rig.hidden {
                if let Some(next) = map.get(id) {
                    *id = next.clone();
                }
            }
        }
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
    Ok(map)
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
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema_version: u32,
    pub id: Id,
    pub name: String,
    pub start_scene: Id,
    pub scenes: Vec<Scene>,
    pub assets: Vec<Asset>,
    pub input_bindings: BTreeMap<String, String>,
    #[serde(default)]
    pub input_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub collision_groups: BTreeMap<u8, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<crate::surface::SurfaceMaterial>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<crate::variables::GameVariable>,
    #[serde(default, skip_serializing_if = "GameSettings::is_default")]
    pub game: GameSettings,
}
/// How the exported game opens: its name and its window. Left out of the document while every
/// value is the default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameSettings {
    /// The window's title and the name of the exported game; empty for the project's name.
    #[serde(default)]
    pub title: String,
    /// Width and height of the window's inside, in pixels at the system's 100% scale.
    #[serde(default = "GameSettings::default_window")]
    pub window: [u32; 2],
    #[serde(default)]
    pub fullscreen: bool,
}
impl Default for GameSettings {
    fn default() -> Self {
        Self {
            title: String::new(),
            window: Self::DEFAULT_WINDOW,
            fullscreen: false,
        }
    }
}
impl GameSettings {
    pub const DEFAULT_WINDOW: [u32; 2] = [1280, 720];
    pub const MIN_WINDOW: [u32; 2] = [320, 240];
    pub const MAX_WINDOW: [u32; 2] = [7680, 4320];
    pub const MAX_TITLE_CHARS: usize = 80;
    fn default_window() -> [u32; 2] {
        Self::DEFAULT_WINDOW
    }
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.title.chars().count() > Self::MAX_TITLE_CHARS {
            return Err(format!(
                "O título do jogo tem mais de {} caracteres",
                Self::MAX_TITLE_CHARS
            ));
        }
        if self.title.chars().any(char::is_control) {
            return Err(
                "O título do jogo não pode ter quebras de linha nem caracteres de controle".into(),
            );
        }
        let [width, height] = self.window;
        let [min_width, min_height] = Self::MIN_WINDOW;
        let [max_width, max_height] = Self::MAX_WINDOW;
        if !(min_width..=max_width).contains(&width) || !(min_height..=max_height).contains(&height)
        {
            return Err(format!(
                "A janela do jogo deve ter de {min_width}×{min_height} a {max_width}×{max_height} \
                 pixels; recebeu {width}×{height}"
            ));
        }
        Ok(())
    }
}
impl Project {
    /// The exported game's name: its title, or the project's name when it has none.
    pub fn game_title(&self) -> &str {
        let title = self.game.title.trim();
        if title.is_empty() {
            self.name.trim()
        } else {
            title
        }
    }
    pub fn collision_group_label(&self, bit: u8) -> String {
        self.collision_groups
            .get(&bit)
            .cloned()
            .unwrap_or_else(|| match bit {
                0 => "Cenário padrão".into(),
                1 => "Personagens".into(),
                2 => "Áreas".into(),
                _ => format!("Grupo {}", bit + 1),
            })
    }
    pub fn new(name: impl Into<String>) -> Self {
        let scene = Scene::new("Cena 2D", SceneKind::TwoD);
        Self {
            schema_version: SCHEMA_VERSION,
            id: new_id(),
            name: name.into(),
            start_scene: scene.id.clone(),
            scenes: vec![scene],
            assets: Vec::new(),
            input_labels: BTreeMap::new(),
            collision_groups: BTreeMap::new(),
            surfaces: Vec::new(),
            variables: Vec::new(),
            game: GameSettings::default(),
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
    /// The scene `text` names, as agents give it: its ID or, when no scene has that ID, its
    /// name in any case, when no other scene has it. `what` is who gave it ("scene", "O
    /// parâmetro scene do nó action.scene"…): the refusal says what it received and lists the
    /// scenes.
    pub fn find_scene(&self, text: &str, what: &str) -> Result<&Scene, String> {
        if let Some(scene) = self.scene(text) {
            return Ok(scene);
        }
        let wanted = text.trim().to_lowercase();
        let named: Vec<&Scene> = self
            .scenes
            .iter()
            .filter(|s| s.name.trim().to_lowercase() == wanted)
            .collect();
        match named.as_slice() {
            [one] => Ok(one),
            [] => Err(format!(
                "{what} recebeu \"{text}\", que não é o ID nem o nome de uma cena do projeto. \
                 Cenas: {}",
                scene_list(self.scenes.iter())
            )),
            many => Err(format!(
                "{what} recebeu \"{text}\", o nome de {} cenas; use o ID: {}",
                many.len(),
                scene_list(many.iter().copied())
            )),
        }
    }
    pub fn asset(&self, id: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }
    /// The model called `name`, in any case and without the spaces around it. Agents place
    /// models by name, so no two models share one.
    pub fn model_named(&self, name: &str) -> Option<&Asset> {
        let wanted = name.trim().to_lowercase();
        self.assets
            .iter()
            .find(|a| a.kind == AssetKind::Model && a.name.trim().to_lowercase() == wanted)
    }
    /// `name` or, when a model has it, the first of "name 2", "name 3"… no model has.
    pub fn free_model_name(&self, name: &str) -> String {
        let name = name.trim();
        if self.model_named(name).is_none() {
            return name.into();
        }
        (2..)
            .map(|number| format!("{name} {number}"))
            .find(|free| self.model_named(free).is_none())
            .expect("a free number")
    }
    /// Stores the editable hierarchy in a shared model asset. Instances remain explicit. A name
    /// another model has is refused ([`Self::model_named`]).
    pub fn save_model(
        &mut self,
        scene_id: &str,
        entity_id: &str,
        name: impl Into<String>,
    ) -> Result<Id, String> {
        self.save_model_mapped(scene_id, entity_id, name)
            .map(|(model, _)| model)
    }
    /// [`Self::save_model`] with the IDs the model took: each object, node and animation saved
    /// to its copy in the model.
    pub fn save_model_mapped(
        &mut self,
        scene_id: &str,
        entity_id: &str,
        name: impl Into<String>,
    ) -> Result<(Id, CopyMap), String> {
        let name = name.into();
        if let Some(other) = self.model_named(&name) {
            return Err(format!(
                "Já existe um modelo chamado \"{}\" ({})",
                other.name, other.id
            ));
        }
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
        let map = remap_entities(&mut pieces, &map)?;
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
            name,
            path: String::new(),
            kind: AssetKind::Model,
            model: Some(pieces),
        });
        if let Err(error) = validate_project(self) {
            self.assets.pop();
            return Err(error);
        }
        Ok((id, map))
    }
    pub fn instantiate_model(&mut self, asset_id: &str, scene_id: &str) -> Result<Id, String> {
        self.instantiate_model_mapped(asset_id, scene_id)
            .map(|(root, _)| root)
    }
    /// [`Self::instantiate_model`] with the IDs the copy took: each piece, node and animation
    /// of the model to its copy in the scene.
    pub fn instantiate_model_mapped(
        &mut self,
        asset_id: &str,
        scene_id: &str,
    ) -> Result<(Id, CopyMap), String> {
        let mut model = self
            .asset(asset_id)
            .and_then(|a| a.model.clone())
            .ok_or("Modelo não encontrado")?;
        let map: HashMap<_, _> = model.iter().map(|e| (e.id.clone(), new_id())).collect();
        let map = remap_entities(&mut model, &map)?;
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
        Ok((root, map))
    }
}
pub fn validate_project(project: &Project) -> Result<(), String> {
    if project
        .collision_groups
        .iter()
        .any(|(bit, name)| *bit >= 32 || name.trim().is_empty())
    {
        return Err("Grupos de colisão precisam de nome e posição entre 1 e 32.".into());
    }
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
    project.game.validate()?;
    if project.input_bindings.len() > 1024 || project.input_labels.len() > 1024 {
        return Err("Limite de 1024 ações de entrada excedido.".into());
    }
    for (id, name) in &project.input_labels {
        if !project.input_bindings.contains_key(id) || name.trim().is_empty() {
            return Err(format!("Nome de ação sem vínculo válido: {id}"));
        }
    }
    let mut identifiers = HashSet::new();
    identifiers.insert(project.id.clone());
    let mut surfaces = HashSet::new();
    for surface in &project.surfaces {
        surface.validate()?;
        if !identifiers.insert(surface.id.clone()) || !surfaces.insert(surface.id.as_str()) {
            return Err("Identificador de superfície duplicado.".into());
        }
    }
    crate::variables::validate_all(&project.variables, &mut identifiers)?;
    for entity in project.scenes.iter().flat_map(|s| &s.entities).chain(
        project
            .assets
            .iter()
            .filter_map(|a| a.model.as_deref())
            .flatten(),
    ) {
        if let Some(id) = entity
            .character3d
            .as_ref()
            .and_then(|c| c.body.surface.as_deref())
            .or_else(|| entity.physics3d.as_ref().and_then(|c| c.surface.as_deref()))
            && !surfaces.contains(id)
        {
            return Err(format!("{}: superfície física ausente ({id})", entity.name));
        }
        for value in entity
            .attributes
            .values()
            .chain(entity.graph.nodes.iter().flat_map(|n| n.params.values()))
        {
            if let Some(id) = value.surface()
                && !surfaces.contains(id)
            {
                return Err(format!(
                    "{}: referência de superfície ausente ({id})",
                    entity.name
                ));
            }
        }
    }
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
        validate_action_references(project, &scene.entities, None)?;
    }
    for asset in &project.assets {
        if let Some(model) = &asset.model {
            let scene = Scene {
                id: asset.id.clone(),
                name: asset.name.clone(),
                kind: SceneKind::ThreeD,
                entities: model.clone(),
            };
            validate_scene(&scene, &assets)?;
            validate_action_references(project, model, Some(&asset.name))?;
        }
    }
    Ok(())
}
fn validate_scene(scene: &Scene, assets: &HashMap<&str, &Asset>) -> Result<(), String> {
    let view = crate::scene_view::SceneView::new(scene);
    let ids: HashSet<_> = scene.entities.iter().map(|e| e.id.as_str()).collect();
    if ids.len() != scene.entities.len() {
        return Err(format!("IDs duplicados em {}", scene.name));
    }
    let owned_ids: HashSet<_> = ids.iter().map(|id| (*id).to_owned()).collect();
    for e in &scene.entities {
        crate::character::validate(scene, e, &view)
            .map_err(|error| format!("{}: {error}", e.name))?;
        if e.mesh.is_some()
            && (e.primitive.is_some()
                || e.primitive_parameters.is_some()
                || e.dimensions != [1.; 3])
        {
            return Err(format!(
                "{}: malha editável não pode conter parâmetros de primitiva ou dimensões externas; use a transformação para escalar.",
                e.name
            ));
        }
        if let Some(parameters) = e.primitive_parameters {
            parameters
                .validate()
                .map_err(|error| format!("{}: {error}", e.name))?;
        }
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
        view.world_matrix(&e.id)?;
        if let Some(collider) = &e.physics3d {
            if scene.kind != SceneKind::ThreeD {
                return Err(format!("{}: formas físicas 3D exigem uma cena 3D.", e.name));
            }
            collider
                .validate()
                .map_err(|error| format!("{}: {error}", e.name))?;
            let (_, _, scale) = crate::physics3d::world_pose(view.world_matrix(&e.id)?)
                .map_err(|error| format!("{}: {error}", e.name))?;
            collider
                .shape
                .validate_scale(scale)
                .map_err(|error| format!("{}: {error}", e.name))?;
        }
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
            if !value.is_finite() {
                return Err(format!("Valor não finito em {}.{key}", e.name));
            }
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
/// Scenes that messages list; more are only counted.
const SCENES_LISTED: usize = 30;

/// Up to [`SCENES_LISTED`] scenes as `[{"id": …, "name": …}]`, for refusals of what agents
/// give, which may need the ID.
fn scene_list<'a>(scenes: impl Iterator<Item = &'a Scene>) -> String {
    let scenes: Vec<&Scene> = scenes.collect();
    let listed: Vec<serde_json::Value> = scenes
        .iter()
        .take(SCENES_LISTED)
        .map(|s| serde_json::json!({"id": s.id, "name": s.name}))
        .collect();
    let text = serde_json::to_string(&listed).unwrap_or_default();
    with_more(text, scenes.len())
}
/// The names of up to [`SCENES_LISTED`] of the project's scenes, for messages people read.
fn scene_names(project: &Project) -> String {
    let names: Vec<&str> = project
        .scenes
        .iter()
        .take(SCENES_LISTED)
        .map(|s| s.name.as_str())
        .collect();
    with_more(names.join(", "), project.scenes.len())
}
fn with_more(mut text: String, scenes: usize) -> String {
    if scenes > SCENES_LISTED {
        text.push_str(&format!(" e mais {}", scenes - SCENES_LISTED));
    }
    text
}

/// The scenes, audios, clips and game variables named by the logic of `entities`, a scene's
/// objects or the pieces of the model named `model`. Clips are checked in scenes only: a model
/// may name the clip of an object it is placed with, such as the visitor of its area.
fn validate_action_references(
    project: &Project,
    entities: &[Entity],
    model: Option<&str>,
) -> Result<(), String> {
    let clips: HashSet<_> = entities
        .iter()
        .flat_map(|e| e.clips.iter().map(|clip| clip.id.as_str()))
        .collect();
    for e in entities {
        for node in &e.graph.nodes {
            let missing = node_references(node).find(|(param, reference, _)| {
                let id = node.text(param.id);
                !id.is_empty()
                    && match reference {
                        Reference::Scene => project.scene(id).is_none(),
                        Reference::Audio => !project
                            .asset(id)
                            .is_some_and(|a| a.kind == AssetKind::Audio),
                        Reference::Clip => model.is_none() && !clips.contains(id),
                        Reference::Variable => crate::variables::get(project, id).is_none(),
                        _ => false,
                    }
            });
            if let Some((param, reference, _)) = missing {
                // A piece exists only in its model: the scenes do not show it.
                let holder = model.map_or(String::new(), |name| format!("modelo {name}, "));
                let label = crate::graph::definition(&node.operation).map_or("", |d| d.label);
                let scenes = if reference == Reference::Scene {
                    format!(". Cenas: {}", scene_names(project))
                } else {
                    String::new()
                };
                return Err(format!(
                    "Referência quebrada: {holder}objeto {}, nó {label} ({}): o parâmetro {} \
                     recebeu \"{}\"{scenes}",
                    e.name,
                    node.operation,
                    param.id,
                    node.text(param.id)
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
    fn game_variables_are_optional_typed_and_named_apart() {
        use crate::variables::{GameVariable, MAX_TEXT, MAX_VARIABLES};
        let mut p = Project::new("Variáveis");
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("variables").is_none(), "{json}");
        let variable = |name: &str, value: Value| GameVariable {
            id: new_id(),
            name: name.into(),
            value,
            persistent: false,
        };
        p.variables = vec![
            variable("Moedas", Value::Number(0.)),
            variable("Saída", Value::Vector3([0., 1., 0.])),
        ];
        validate_project(&p).unwrap();
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["variables"][0]["persistent"], false);
        let loaded: Project = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(loaded, p);
        // persistent may be left out; unknown fields may not.
        let mut short = json.clone();
        short["variables"][0]
            .as_object_mut()
            .unwrap()
            .remove("persistent");
        let loaded: Project = serde_json::from_value(short).unwrap();
        assert!(!loaded.variables[0].persistent);
        let mut unknown = json;
        unknown["variables"][0]["scope"] = "cena".into();
        assert!(serde_json::from_value::<Project>(unknown).is_err());

        let refused = |change: &dyn Fn(&mut Project)| {
            let mut broken = p.clone();
            change(&mut broken);
            validate_project(&broken).unwrap_err()
        };
        let error = refused(&|p| p.variables[1].value = Value::Object(None));
        assert!(
            error.contains("Saída") && error.contains("objetos"),
            "{error}"
        );
        let error = refused(&|p| p.variables[1].value = Value::Surface(None));
        assert!(error.contains("superfícies"), "{error}");
        let error = refused(&|p| p.variables[1].name = " moedas ".into());
        assert!(
            error.contains("duas variáveis do jogo chamadas moedas"),
            "{error}"
        );
        let error = refused(&|p| p.variables[1].name = "  ".into());
        assert!(error.contains("nome"), "{error}");
        // Reports write a key's value as "name · key".
        let error = refused(&|p| p.variables[1].name = "Moedas · Fase".into());
        assert!(error.contains("não pode ter ·"), "{error}");
        let error = refused(&|p| p.variables[1].id = p.scenes[0].id.clone());
        assert!(error.contains("Identificador"), "{error}");
        let error = refused(&|p| p.variables[0].value = Value::Number(f64::INFINITY));
        assert!(error.contains("finito"), "{error}");
        let error = refused(&|p| p.variables[0].value = Value::Text("a".repeat(MAX_TEXT + 1)));
        assert!(error.contains("4096 bytes"), "{error}");
        let error = refused(&|p| {
            p.variables = (0..=MAX_VARIABLES)
                .map(|i| variable(&format!("V{i}"), Value::Bool(false)))
                .collect();
        });
        assert!(error.contains("256"), "{error}");
    }
    #[test]
    fn game_settings_are_optional_and_limited() {
        let mut p = Project::new("Corrida");
        // Without settings the document is as before, and an older one loads.
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("game").is_none(), "{json}");
        assert_eq!(p.game_title(), "Corrida");
        p.game.title = "  Corrida Noturna ".into();
        p.game.fullscreen = true;
        assert_eq!(p.game_title(), "Corrida Noturna");
        validate_project(&p).unwrap();
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["game"]["window"], serde_json::json!([1280, 720]));
        assert_eq!(serde_json::from_value::<Project>(json.clone()).unwrap(), p);
        // Fields may be left out; unknown ones may not.
        let mut short = json.clone();
        short["game"] = serde_json::json!({"fullscreen": true});
        let loaded: Project = serde_json::from_value(short).unwrap();
        assert_eq!(loaded.game.window, GameSettings::DEFAULT_WINDOW);
        assert!(loaded.game.fullscreen && loaded.game.title.is_empty());
        let mut unknown = json;
        unknown["game"]["save_folder"] = "Corrida".into();
        assert!(serde_json::from_value::<Project>(unknown).is_err());

        let refused = |change: &dyn Fn(&mut GameSettings)| {
            let mut broken = p.clone();
            change(&mut broken.game);
            validate_project(&broken).unwrap_err()
        };
        let error = refused(&|g| g.title = "a".repeat(81));
        assert!(error.contains("80 caracteres"), "{error}");
        validate_project(&Project {
            game: GameSettings {
                title: "é".repeat(80),
                ..p.game.clone()
            },
            ..p.clone()
        })
        .unwrap();
        let error = refused(&|g| g.title = "Linha\nNova".into());
        assert!(error.contains("quebras de linha"), "{error}");
        for window in [[319, 240], [320, 239], [7681, 720], [1280, 4321], [0, 0]] {
            let error = refused(&|g| g.window = window);
            assert!(
                error.contains("320×240 a 7680×4320")
                    && error.contains(&format!("{}×{}", window[0], window[1])),
                "{error}"
            );
        }
        for window in [GameSettings::MIN_WINDOW, GameSettings::MAX_WINDOW] {
            let mut edge = p.clone();
            edge.game.window = window;
            validate_project(&edge).unwrap();
        }
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
