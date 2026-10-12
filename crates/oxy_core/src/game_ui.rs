//! The game interface as the player lays it out: where each element sits, the text it shows
//! with its bound value and how full a bar is. The renderer (editor game tab and player) and
//! the headless reports share it, so an agent reads what a person would see.
use crate::{document::*, scene_view::SceneView, surface::SurfaceMaterial};
use serde::Serialize;

/// Rectangle on screen, y down: pixels, or logical points in the editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}
impl ScreenRect {
    /// A view of `width` × `height` with its top-left corner at the origin.
    pub fn from_size(width: f32, height: f32) -> Self {
        Self {
            min: [0., 0.],
            max: [width, height],
        }
    }
    pub fn width(&self) -> f32 {
        self.max[0] - self.min[0]
    }
    pub fn height(&self) -> f32 {
        self.max[1] - self.min[1]
    }
    /// The two share an area, not only an edge.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.min[0] < other.max[0]
            && other.min[0] < self.max[0]
            && self.min[1] < other.max[1]
            && other.min[1] < self.max[1]
    }
}

/// Where `element` sits inside `viewport`: its offset is measured from the anchored corner,
/// so a right anchor keeps its margin when the view is resized.
pub fn element_rect(element: &UiElement, viewport: ScreenRect) -> ScreenRect {
    let size = [element.size[0].max(1.), element.size[1].max(1.)];
    let [x, y] = element.position;
    let min = match element.anchor {
        UiAnchor::TopLeft => [viewport.min[0] + x, viewport.min[1] + y],
        UiAnchor::TopRight => [viewport.max[0] - size[0] - x, viewport.min[1] + y],
        UiAnchor::BottomLeft => [viewport.min[0] + x, viewport.max[1] - size[1] - y],
        UiAnchor::BottomRight => [viewport.max[0] - size[0] - x, viewport.max[1] - size[1] - y],
        UiAnchor::Center => [
            (viewport.min[0] + viewport.max[0]) / 2. - size[0] * 0.5 + x,
            (viewport.min[1] + viewport.max[1]) / 2. - size[1] * 0.5 + y,
        ],
    };
    ScreenRect {
        min,
        max: [min[0] + size[0], min[1] + size[1]],
    }
}

/// The attribute an element shows: `binding_attribute` of its bound object, or of itself.
pub fn bound_value<'a>(
    view: &SceneView<'a>,
    entity: &Entity,
    element: &UiElement,
) -> Option<&'a Value> {
    view.entity(element.binding_object.as_deref().unwrap_or(&entity.id))
        .and_then(|bound| bound.attributes.get(&element.binding_attribute))
}

/// The element text with the bound value in place of `{value}` (or `{valor}`), after the text
/// when it has no placeholder. References show the object's current name, never its ID.
pub fn display_text(
    element: &UiElement,
    value: Option<&Value>,
    scene: &Scene,
    surfaces: &[SurfaceMaterial],
) -> String {
    let Some(value) = value else {
        return element.text.clone();
    };
    let value = match value {
        Value::Number(number) => {
            if number.fract() == 0. {
                format!("{number:.0}")
            } else {
                format!("{number:.2}")
            }
        }
        Value::Text(text) => text.clone(),
        Value::Bool(value) => {
            if *value {
                "sim".into()
            } else {
                "não".into()
            }
        }
        Value::Object(Some(id)) => scene
            .entity(id)
            .map_or_else(|| "objeto ausente".into(), |entity| entity.name.clone()),
        Value::Object(None) => "nenhum".into(),
        Value::Vector2([x, y]) => format!("({x:.2}; {y:.2})"),
        Value::Vector3([x, y, z]) => format!("({x:.2}; {y:.2}; {z:.2})"),
        Value::Surface(id) => id
            .as_ref()
            .and_then(|id| surfaces.iter().find(|s| &s.id == id))
            .map_or_else(|| "sem superfície".into(), |s| s.name.clone()),
    };
    if element.text.contains("{value}") || element.text.contains("{valor}") {
        element
            .text
            .replace("{value}", &value)
            .replace("{valor}", &value)
    } else if element.text.is_empty() {
        value
    } else {
        format!("{} {value}", element.text)
    }
}

/// How full a bar is, 0 to 1: the bound number over `max_value`.
pub fn bar_fraction(element: &UiElement, value: Option<&Value>) -> f64 {
    let current = value.and_then(Value::number).unwrap_or(0.);
    (current / element.max_value.max(0.0001)).clamp(0., 1.)
}

/// The text drawn on the element: `display_text`, or "current / max" on a bar left empty.
pub fn shown_text(
    element: &UiElement,
    value: Option<&Value>,
    scene: &Scene,
    surfaces: &[SurfaceMaterial],
) -> String {
    let text = display_text(element, value, scene, surfaces);
    if element.kind == UiKind::Bar && text.is_empty() {
        let current = value.and_then(Value::number).unwrap_or(0.);
        format!("{current:.0} / {:.0}", element.max_value)
    } else {
        text
    }
}

/// One interface element as a person would see it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct UiState {
    pub id: Id,
    pub name: String,
    pub kind: UiKind,
    /// The element and every parent are visible.
    pub visible: bool,
    /// x, y, width and height in pixels of the view, from its top-left corner.
    pub rect: [f64; 4],
    /// Text drawn, with the bound value; absent on images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How full a bar is, 0 to 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,
    /// A visible button at least partly inside the view: a person could click it.
    pub clickable: bool,
}

/// Every interface element of `scene` laid out in a view of `size` pixels, in drawing order
/// (the topmost last), hidden ones included with `visible: false`.
pub fn ui_state(project: &Project, scene: &Scene, size: [f32; 2]) -> Vec<UiState> {
    let view = SceneView::new(scene);
    let viewport = ScreenRect::from_size(size[0], size[1]);
    let mut elements: Vec<_> = scene
        .entities
        .iter()
        .filter_map(|entity| entity.ui.as_ref().map(|element| (entity, element)))
        .collect();
    // Stable, as when drawing: equal layers keep the scene order.
    elements.sort_by_key(|(entity, _)| entity.layer);
    // Hundredths of a pixel and of a bar keep reports short.
    let short = |value: f64| (value * 100.).round() / 100.;
    elements
        .into_iter()
        .map(|(entity, element)| {
            let rect = element_rect(element, viewport);
            let value = bound_value(&view, entity, element);
            let visible = view.visible(entity);
            UiState {
                id: entity.id.clone(),
                name: entity.name.clone(),
                kind: element.kind,
                visible,
                rect: [rect.min[0], rect.min[1], rect.width(), rect.height()]
                    .map(|v| short(f64::from(v))),
                text: (element.kind != UiKind::Image)
                    .then(|| shown_text(element, value, scene, &project.surfaces)),
                fraction: (element.kind == UiKind::Bar)
                    .then(|| short(bar_fraction(element, value))),
                clickable: element.kind == UiKind::Button && visible && rect.overlaps(&viewport),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_binding_follows_live_names_and_never_displays_raw_identifiers() {
        let mut project = Project::new("Referência na interface");
        let scene = &mut project.scenes[0];
        let target = Entity::new("Alvo de treino", None);
        let target_id = target.id.clone();
        let value = Value::Object(Some(target_id.clone()));
        scene.entities.push(target);
        let element = UiElement {
            text: "Selecionado: {valor}".into(),
            ..Default::default()
        };
        assert_eq!(
            display_text(&element, Some(&value), scene, &[]),
            "Selecionado: Alvo de treino"
        );
        scene.entity_mut(&target_id).unwrap().name = "Alvo renomeado".into();
        assert_eq!(
            display_text(&element, Some(&value), scene, &[]),
            "Selecionado: Alvo renomeado"
        );
        assert_eq!(
            value,
            Value::Object(Some(target_id.clone())),
            "The persisted reference remains stable after renaming"
        );
        scene.entities.clear();
        let missing = display_text(&element, Some(&value), scene, &[]);
        assert_eq!(missing, "Selecionado: objeto ausente");
        assert!(!missing.contains(&target_id));
        assert_eq!(
            display_text(&element, Some(&Value::Object(None)), scene, &[]),
            "Selecionado: nenhum"
        );
    }

    #[test]
    fn anchors_keep_their_margins_inside_any_view() {
        let element = |anchor| UiElement {
            anchor,
            position: [12., 8.],
            size: [100., 40.],
            ..Default::default()
        };
        let view = ScreenRect {
            min: [10., 20.],
            max: [310., 420.],
        };
        let rect = |anchor| element_rect(&element(anchor), view);
        assert_eq!(rect(UiAnchor::TopLeft).min, [22., 28.]);
        assert_eq!(rect(UiAnchor::TopRight).min, [198., 28.]);
        assert_eq!(rect(UiAnchor::BottomLeft).min, [22., 372.]);
        assert_eq!(rect(UiAnchor::BottomRight).max, [298., 412.]);
        assert_eq!(rect(UiAnchor::Center).min, [122., 208.]);
        assert_eq!(rect(UiAnchor::Center).width(), 100.);
        // A zero size still takes one pixel, as drawn.
        let tiny = UiElement {
            size: [0., -5.],
            ..element(UiAnchor::TopLeft)
        };
        assert_eq!(element_rect(&tiny, view).height(), 1.);
    }

    #[test]
    fn the_state_reports_bound_text_bar_fill_and_what_a_person_could_click() {
        let mut project = Project::new("Interface");
        let mut player = Entity::new("Jogador", None);
        player.attributes.insert("vida".into(), Value::Number(30.));
        player
            .attributes
            .insert("tempo".into(), Value::Number(12.5));
        let player_id = player.id.clone();
        let mut bar = Entity::new("Barra de vida", None);
        bar.layer = 2;
        bar.ui = Some(UiElement {
            kind: UiKind::Bar,
            text: String::new(),
            binding_object: Some(player_id.clone()),
            binding_attribute: "vida".into(),
            max_value: 120.,
            ..Default::default()
        });
        let mut empty_bar = Entity::new("Barra solta", None);
        empty_bar.ui = Some(UiElement {
            kind: UiKind::Bar,
            text: String::new(),
            max_value: 50.,
            ..Default::default()
        });
        let mut timer = Entity::new("Cronômetro", None);
        timer.ui = Some(UiElement {
            text: "Tempo: {value} s".into(),
            anchor: UiAnchor::TopRight,
            binding_object: Some(player_id),
            binding_attribute: "tempo".into(),
            ..Default::default()
        });
        let mut menu = Entity::new("Menu de pausa", None);
        menu.visible = false;
        let menu_id = menu.id.clone();
        let mut resume = Entity::new("Continuar", None);
        resume.parent = Some(menu_id);
        resume.ui = Some(UiElement {
            kind: UiKind::Button,
            text: "Continuar".into(),
            ..Default::default()
        });
        let mut start = Entity::new("Começar", None);
        start.ui = Some(UiElement {
            kind: UiKind::Button,
            text: "Começar".into(),
            anchor: UiAnchor::Center,
            ..Default::default()
        });
        let mut outside = Entity::new("Fora da tela", None);
        outside.ui = Some(UiElement {
            kind: UiKind::Button,
            position: [2000., 0.],
            ..Default::default()
        });
        let mut logo = Entity::new("Logo", None);
        logo.ui = Some(UiElement {
            kind: UiKind::Image,
            ..Default::default()
        });
        project.scenes[0].entities.extend([
            player, bar, empty_bar, timer, menu, resume, start, outside, logo,
        ]);
        let state = ui_state(&project, &project.scenes[0], [800., 600.]);
        let names: Vec<_> = state.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Barra solta",
                "Cronômetro",
                "Continuar",
                "Começar",
                "Fora da tela",
                "Logo",
                "Barra de vida"
            ],
            "drawing order: by layer, then scene order"
        );
        let get = |name: &str| state.iter().find(|s| s.name == name).unwrap();
        let bar = get("Barra de vida");
        assert_eq!(bar.kind, UiKind::Bar);
        assert_eq!(bar.text.as_deref(), Some("30"));
        assert_eq!(bar.fraction, Some(0.25));
        assert!(!bar.clickable);
        let empty = get("Barra solta");
        assert_eq!(empty.text.as_deref(), Some("0 / 50"));
        assert_eq!(empty.fraction, Some(0.));
        let timer = get("Cronômetro");
        assert_eq!(timer.text.as_deref(), Some("Tempo: 12.50 s"));
        assert_eq!(timer.rect, [596., 24., 180., 36.]);
        let resume = get("Continuar");
        assert!(
            !resume.visible && !resume.clickable,
            "hidden through its parent"
        );
        let start = get("Começar");
        assert!(start.visible && start.clickable);
        assert_eq!(start.rect, [334., 306., 180., 36.]);
        assert!(!get("Fora da tela").clickable);
        assert_eq!(get("Logo").text, None);
        let json = serde_json::to_value(bar).unwrap();
        assert_eq!(json["kind"], "Bar");
        assert!(
            serde_json::to_value(get("Logo"))
                .unwrap()
                .get("fraction")
                .is_none()
        );
    }
}
