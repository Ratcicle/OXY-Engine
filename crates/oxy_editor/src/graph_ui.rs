//! Visual logic: the node catalog, the graph canvas and the node inspector.
//! The graph stays the persisted contract of `oxy_core::graph`; this module draws and
//! edits it.
mod parameters;
use parameters::{ParameterContext, param_label, param_summary, parameter_editor};
pub use parameters::{collision_filter_picker, object_picker, value_editor};

use crate::icons::{self, Icon};
use crate::{theme, widgets};
use egui::{
    Align, Color32, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2,
};
use oxy_core::document::{Id, Project, Scene, Value};
use oxy_core::graph::{Edge, Graph, Node, OperationDef, ParamDef, PortDef, PortType, registry};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub use oxy_render::input::text_input_active;

const NODE_WIDTH: f32 = 200.;
const HEADER_HEIGHT: f32 = 30.;
const ROW_HEIGHT: f32 = 22.;
const BODY_TOP: f32 = 6.;
const BODY_BOTTOM: f32 = 8.;
const MIN_ZOOM: f32 = 0.35;
const MAX_ZOOM: f32 = 1.6;
const DEFAULT_PAN: Vec2 = Vec2::new(35., 45.);
const NODE_FILL: Color32 = Color32::from_rgb(47, 47, 51);
const NODE_BORDER: Color32 = Color32::from_rgb(70, 70, 76);
const NODE_HEADER: Color32 = Color32::from_rgb(58, 58, 63);
const EVENT_HEADER: Color32 = Color32::from_rgb(46, 69, 102);
const HEADER_ICON: Color32 = Color32::from_rgb(196, 212, 236);
const CANVAS_DOT: Color32 = Color32::from_rgb(48, 48, 52);
const ROW_TEXT: Color32 = Color32::from_rgb(200, 200, 204);
const PARAM_NAME: Color32 = Color32::from_rgb(142, 142, 148);
const PILL_FILL: Color32 = Color32::from_rgb(37, 37, 40);
const PILL_TEXT: Color32 = Color32::from_rgb(180, 180, 186);
/// Events an empty graph offers to start with.
const STARTERS: [&str; 4] = [
    "event.scene_start",
    "event.click",
    "event.input",
    "event.area_enter",
];
/// Catalog categories open by default; the movement families start collapsed.
const OPEN_CATEGORIES: [&str; 5] = ["Eventos", "Dados", "Condições", "Ações", "Controle"];

fn port_color(kind: &PortType) -> Color32 {
    match kind {
        PortType::Exec => Color32::WHITE,
        PortType::Number => Color32::from_rgb(114, 199, 227),
        PortType::Bool => Color32::from_rgb(230, 135, 130),
        PortType::Text => Color32::from_rgb(205, 172, 230),
        PortType::Object => Color32::from_rgb(229, 192, 115),
        PortType::Any => Color32::from_gray(160),
        PortType::Vector2 => Color32::from_rgb(98, 204, 164),
        PortType::Vector3 => Color32::from_rgb(89, 168, 132),
        PortType::Surface => Color32::from_rgb(193, 148, 101),
    }
}

pub fn operation(id: &str) -> Option<&'static OperationDef> {
    registry().iter().find(|op| op.id == id)
}

/// Operations grouped by category in registry order, each category listed once.
pub fn categories() -> &'static [(&'static str, Vec<&'static OperationDef>)] {
    static CATEGORIES: std::sync::OnceLock<Vec<(&'static str, Vec<&'static OperationDef>)>> =
        std::sync::OnceLock::new();
    CATEGORIES.get_or_init(|| {
        let mut result: Vec<(&'static str, Vec<&'static OperationDef>)> = Vec::new();
        for op in registry() {
            match result.iter_mut().find(|(name, _)| *name == op.category) {
                Some((_, ops)) => ops.push(op),
                None => result.push((op.category, vec![op])),
            }
        }
        result
    })
}

pub fn category_icon(category: &str) -> Icon {
    match category {
        "Eventos" => Icon::Bolt,
        "Dados" => Icon::Data,
        "Condições" => Icon::Branch,
        "Ações" => Icon::Action,
        "Controle" => Icon::Flow,
        "Entrada e tempo" => Icon::Keyboard,
        "Personagem 3D" => Icon::Person,
        "Câmera de jogo" => Icon::Camera,
        "Superfícies físicas" => Icon::Collider,
        "Consultas físicas 3D" => Icon::Sphere,
        "Transformação 3D" | "Vetores" => Icon::Move,
        _ => Icon::Logic,
    }
}

/// What a node does, for the catalog tooltip and the inspector.
pub fn operation_description(operation: &str) -> &'static str {
    match operation {
        "event.scene_start" => "Executa quando a cena começa.",
        "event.input" => {
            "Executa ao pressionar, manter ou soltar uma ação configurada em Ações de entrada."
        }
        "event.click" => "Executa ao clicar neste objeto ou em seu botão de interface.",
        "event.area_enter" => {
            "Executa quando um objeto entra na área e fornece esse objeto na saída."
        }
        "event.animation" => "Executa quando a reprodução atravessa o marcador escolhido.",
        "value.number" => "Fornece um número definido no painel de parâmetros.",
        "value.text" => "Fornece um texto definido no painel de parâmetros.",
        "value.bool" => "Fornece um valor verdadeiro ou falso.",
        "value.object" => "Fornece a referência de um objeto da cena.",
        "attribute.get" => "Lê o atributo escolhido do objeto informado.",
        "attribute.set" => "Altera o atributo escolhido do objeto informado.",
        "math.binary" => "Soma, subtrai, multiplica ou divide os números A e B.",
        "condition.compare" => "Compara A e B e fornece verdadeiro ou falso.",
        "condition.branch" => "Segue a saída Verdadeiro ou Falso conforme a condição.",
        "action.damage" => {
            "Reduz o atributo numérico configurado no alvo. Pode limitar a um acerto por ativação."
        }
        "action.animation" => "Reproduz uma animação do objeto escolhido.",
        "action.sound" => "Reproduz um áudio WAV do projeto no volume configurado.",
        "action.spawn" => "Cria uma cópia da hierarquia do objeto durante o jogo.",
        "action.remove" => "Remove o objeto e seus descendentes durante o jogo.",
        "action.component" => "Ativa ou desativa o componente escolhido do objeto.",
        "action.scene" => "Encerra a cena atual e abre a cena de destino no jogo.",
        "control.sequence" => "Executa as saídas 1, 2 e 3 nessa ordem.",
        "control.wait" => "Continua após o tempo indicado, sem interromper o jogo ou o editor.",
        "debug.message" => "Exibe uma mensagem no diagnóstico durante a execução.",
        // The guide documents every registered operation, movement nodes included.
        _ => oxy_core::guide::topics()
            .iter()
            .find(|topic| topic.operation.id == operation)
            .map_or(
                "Operação desconhecida. Verifique a compatibilidade do projeto.",
                |topic| topic.purpose,
            ),
    }
}

/// The catalog docks beside the canvas when the central area has room for both;
/// otherwise the Lógica toolbar opens it as a menu.
pub fn catalog_docked(width: f32) -> bool {
    width >= 640.
}

/// "N nós · M conexões".
pub fn graph_meta(graph: &Graph) -> String {
    let (nodes, edges) = (graph.nodes.len(), graph.edges.len());
    format!(
        "{nodes} {} · {edges} {}",
        if nodes == 1 { "nó" } else { "nós" },
        if edges == 1 { "conexão" } else { "conexões" }
    )
}

fn node_label(graph: &Graph, node: &str) -> &'static str {
    graph.node(node).map_or("Nó ausente", |node| {
        operation(&node.operation).map_or("Nó desconhecido", |op| op.label)
    })
}

fn port_def(graph: &Graph, node: &str, port: &str, output: bool) -> Option<&'static PortDef> {
    let op = operation(&graph.node(node)?.operation)?;
    (if output { &op.outputs } else { &op.inputs })
        .iter()
        .find(|candidate| candidate.id == port)
}

fn port_label(graph: &Graph, node: &str, port: &str, output: bool) -> &'static str {
    port_def(graph, node, port, output).map_or("Porta ausente", |port| port.label)
}

/// Validation and connection messages name nodes and objects, never their identifiers.
fn humanize(message: &str, graph: &Graph, objects: &[(Id, String)]) -> String {
    let mut text = message.to_owned();
    for node in &graph.nodes {
        text = text.replace(&node.id, node_label(graph, &node.id));
    }
    for (id, name) in objects {
        text = text.replace(id.as_str(), name);
    }
    text.split(' ')
        .map(|word| {
            if looks_like_id(word) {
                "(referência removida)"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn looks_like_id(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    word.len() >= 16
        && word.chars().filter(char::is_ascii_hexdigit).count() >= 16
        && word.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Distance to the drawn cubic, sampled into short line segments for picking.
fn curve_distance(points: [Pos2; 4], mouse: Pos2) -> f32 {
    let mut closest = f32::INFINITY;
    let mut previous = points[0];
    for step in 1..=32 {
        let t = step as f32 / 32.0;
        let s = 1.0 - t;
        let next = (points[0].to_vec2() * s.powi(3)
            + points[1].to_vec2() * (3.0 * s * s * t)
            + points[2].to_vec2() * (3.0 * s * t * t)
            + points[3].to_vec2() * t.powi(3))
        .to_pos2();
        let segment = next - previous;
        let fraction =
            ((mouse - previous).dot(segment) / segment.length_sq().max(1e-6)).clamp(0.0, 1.0);
        closest = closest.min(mouse.distance(previous + segment * fraction));
        previous = next;
    }
    closest
}

fn wire_points(a: Pos2, b: Pos2, zoom: f32) -> [Pos2; 4] {
    let bend = ((b.x - a.x).abs() * 0.45).max(40. * zoom);
    [a, a + Vec2::new(bend, 0.), b - Vec2::new(bend, 0.), b]
}

fn bezier(points: [Pos2; 4], stroke: Stroke) -> egui::Shape {
    egui::epaint::CubicBezierShape::from_points_stroke(points, false, Color32::TRANSPARENT, stroke)
        .into()
}

/// Parameters without an input port of the same name: rows of the node body.
fn summary_params(op: &OperationDef) -> impl Iterator<Item = &ParamDef> {
    op.params
        .iter()
        .filter(|param| !op.inputs.iter().any(|port| port.id == param.id))
}

fn port_rows(op: &OperationDef) -> usize {
    op.inputs.len().max(op.outputs.len())
}

/// Size of a node in graph units. An unknown operation keeps one row for its identifier.
fn node_size(op: Option<&OperationDef>) -> Vec2 {
    let rows = op
        .map_or(1, |op| port_rows(op) + summary_params(op).count())
        .max(1);
    Vec2::new(
        NODE_WIDTH,
        HEADER_HEIGHT + BODY_TOP + rows as f32 * ROW_HEIGHT + BODY_BOTTOM,
    )
}

fn row_center(row: usize) -> f32 {
    HEADER_HEIGHT + BODY_TOP + (row as f32 + 0.5) * ROW_HEIGHT
}

fn graph_bounds(graph: &Graph) -> Option<Rect> {
    graph
        .nodes
        .iter()
        .map(|node| {
            Rect::from_min_size(
                Pos2::from(node.position),
                node_size(operation(&node.operation)),
            )
        })
        .reduce(|a, b| a.union(b))
}

fn truncated(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(1.));
    painter.layout_job(job)
}

fn exec_points(center: Pos2, scale: f32) -> Vec<Pos2> {
    vec![
        center + Vec2::new(-4., -6.) * scale,
        center + Vec2::new(6., 0.) * scale,
        center + Vec2::new(-4., 6.) * scale,
    ]
}

/// Port marker: an arrow for execution, a dot coloured by data type. Execution arrows
/// are pure white only on the canvas; the native QA finds ports by that fill.
fn draw_port(painter: &egui::Painter, center: Pos2, kind: PortType, zoom: f32) {
    let scale = zoom.clamp(0.6, 1.3);
    if kind == PortType::Exec {
        painter.add(egui::Shape::convex_polygon(
            exec_points(center, scale),
            Color32::WHITE,
            Stroke::new(1.5, NODE_FILL),
        ));
    } else {
        painter.circle_filled(center, 7. * scale, NODE_FILL);
        painter.circle_filled(center, 5. * scale, port_color(&kind));
    }
}

/// The same marker outside the canvas: legends and the inspector.
fn port_marker(painter: &egui::Painter, center: Pos2, kind: PortType) {
    if kind == PortType::Exec {
        painter.add(egui::Shape::convex_polygon(
            exec_points(center, 0.8),
            theme::TEXT,
            Stroke::NONE,
        ));
    } else {
        painter.circle_filled(center, 4., port_color(&kind));
    }
}

/// Value capsule inside a node, from `anchor` rightwards, or leftwards with `right`.
fn pill(painter: &egui::Painter, anchor: Pos2, text: &str, max_width: f32, zoom: f32, right: bool) {
    let pad = 6. * zoom;
    if max_width < 22. * zoom {
        return;
    }
    let galley = truncated(
        painter,
        text,
        FontId::proportional(11. * zoom),
        PILL_TEXT,
        max_width - 2. * pad,
    );
    let size = Vec2::new(galley.size().x + 2. * pad, 17. * zoom);
    let min = if right {
        Pos2::new(anchor.x - size.x, anchor.y - size.y / 2.)
    } else {
        Pos2::new(anchor.x, anchor.y - size.y / 2.)
    };
    let rect = Rect::from_min_size(min, size);
    painter.rect_filled(rect, (4. * zoom).round() as u8, PILL_FILL);
    painter.galley(
        Pos2::new(rect.left() + pad, rect.center().y - galley.size().y / 2.),
        galley,
        PILL_TEXT,
    );
}

/// Body, header and rows of a node: inputs on the left, outputs on the right, then
/// the parameters that have no port. Unconnected data inputs show the value they use.
#[allow(clippy::too_many_arguments)]
fn draw_node(
    ui: &egui::Ui,
    painter: &egui::Painter,
    node: &Node,
    op: Option<&OperationDef>,
    rect: Rect,
    zoom: f32,
    context: &ParameterContext<'_>,
    connected: &HashSet<(String, String)>,
) {
    let radius = (7. * zoom).round() as u8;
    painter.rect_filled(rect, radius, NODE_FILL);
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEADER_HEIGHT * zoom));
    let fill = match op {
        None => Color32::from_rgb(92, 46, 46),
        Some(op) if op.category == "Eventos" => EVENT_HEADER,
        Some(_) => NODE_HEADER,
    };
    painter.rect_filled(
        header,
        CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        fill,
    );
    // Icons draw through the canvas `Ui`, whose clip is the canvas itself.
    icons::paint(
        ui,
        Rect::from_center_size(
            Pos2::new(header.left() + 17. * zoom, header.center().y),
            Vec2::splat(13. * zoom),
        ),
        op.map_or(Icon::Logic, |op| category_icon(op.category)),
        HEADER_ICON,
    );
    let title = truncated(
        painter,
        op.map_or("Nó desconhecido", |op| op.label),
        theme::medium(12.5 * zoom),
        Color32::WHITE,
        header.width() - 40. * zoom,
    );
    painter.galley(
        Pos2::new(
            header.left() + 30. * zoom,
            header.center().y - title.size().y / 2.,
        ),
        title,
        Color32::WHITE,
    );
    let font = FontId::proportional(11.5 * zoom);
    let pad = 12. * zoom;
    let Some(op) = op else {
        // Its identifier is the only description of an operation this version lacks.
        let text = truncated(
            painter,
            &node.operation,
            font,
            ROW_TEXT,
            rect.width() - 2. * pad,
        );
        let y = rect.top() + row_center(0) * zoom;
        painter.galley(
            Pos2::new(rect.left() + pad, y - text.size().y / 2.),
            text,
            ROW_TEXT,
        );
        return;
    };
    for row in 0..port_rows(op) {
        let y = rect.top() + row_center(row) * zoom;
        let mut limit = rect.right() - pad;
        if let Some(output) = op.outputs.get(row) {
            let galley = truncated(
                painter,
                output.label,
                font.clone(),
                ROW_TEXT,
                rect.width() * 0.62,
            );
            limit -= galley.size().x + 8. * zoom;
            painter.galley(
                Pos2::new(
                    rect.right() - pad - galley.size().x,
                    y - galley.size().y / 2.,
                ),
                galley,
                ROW_TEXT,
            );
        }
        let Some(input) = op.inputs.get(row) else {
            continue;
        };
        let left = rect.left() + pad;
        let galley = truncated(painter, input.label, font.clone(), ROW_TEXT, limit - left);
        let label_right = left + galley.size().x;
        painter.galley(Pos2::new(left, y - galley.size().y / 2.), galley, ROW_TEXT);
        if input.kind != PortType::Exec
            && !connected.contains(&(node.id.clone(), input.id.to_owned()))
            && let Some(param) = op.params.iter().find(|param| param.id == input.id)
            && let Some(value) = param_summary(node, param, context)
        {
            pill(
                painter,
                Pos2::new(label_right + 6. * zoom, y),
                &value,
                limit - label_right - 6. * zoom,
                zoom,
                false,
            );
        }
    }
    for (index, param) in summary_params(op).enumerate() {
        let y = rect.top() + row_center(port_rows(op) + index) * zoom;
        let name = truncated(
            painter,
            param_label(&node.operation, param),
            font.clone(),
            PARAM_NAME,
            rect.width() * 0.45,
        );
        let name_right = rect.left() + pad + name.size().x;
        painter.galley(
            Pos2::new(rect.left() + pad, y - name.size().y / 2.),
            name,
            PARAM_NAME,
        );
        if let Some(value) = param_summary(node, param, context) {
            let room = (rect.right() - pad) - (name_right + 8. * zoom);
            pill(
                painter,
                Pos2::new(rect.right() - pad, y),
                &value,
                room.min(130. * zoom),
                zoom,
                true,
            );
        }
    }
}

/// Feedback of Validar or of a refused connection. It disappears once the graph it
/// describes changes.
struct Status {
    ok: bool,
    text: String,
    graph: Graph,
}

enum NodeCommand {
    Help(&'static str),
    Duplicate(Id),
    Delete(Id),
}

pub struct GraphView {
    pub configure_inputs: bool,
    pub guide_requested: Option<String>,
    pan: Vec2,
    zoom: f32,
    selected: Option<Id>,
    selected_edge: Option<Edge>,
    connecting: Option<(Id, String)>,
    search: String,
    collapsed: HashSet<&'static str>,
    status: Option<Status>,
    /// Canvas of the previous frame: new nodes appear at its centre; Enquadrar fits it.
    canvas: Rect,
    /// Object whose graph is shown. Switching objects frames the new graph.
    owner: Option<Id>,
    fit_pending: bool,
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            configure_inputs: false,
            guide_requested: None,
            pan: DEFAULT_PAN,
            zoom: 1.,
            selected: None,
            selected_edge: None,
            connecting: None,
            search: String::new(),
            collapsed: categories()
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| !OPEN_CATEGORIES.contains(name))
                .collect(),
            status: None,
            canvas: Rect::NOTHING,
            owner: None,
            fit_pending: false,
        }
    }
}

impl GraphView {
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Zooms around the centre of the view.
    pub fn zoom_by(&mut self, factor: f32) {
        let previous = self.zoom;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if self.canvas.is_positive() {
            let center = self.canvas.size() * 0.5;
            self.pan = center - (center - self.pan) * (self.zoom / previous);
        }
    }

    /// Shows the whole graph, at most at 100%.
    pub fn fit(&mut self, graph: &Graph) {
        if !self.canvas.is_positive() {
            self.fit_pending = true;
            return;
        }
        self.fit_pending = false;
        let Some(bounds) = graph_bounds(graph) else {
            self.zoom = 1.;
            self.pan = DEFAULT_PAN;
            return;
        };
        // Margins, and room for the legend along the bottom edge.
        let room = (self.canvas.size() - Vec2::new(80., 120.)).max(Vec2::splat(40.));
        self.zoom = (room.x / bounds.width())
            .min(room.y / bounds.height())
            .clamp(MIN_ZOOM, 1.);
        let center = Vec2::new(
            self.canvas.width() * 0.5,
            (self.canvas.height() - 40.) * 0.5,
        );
        self.pan = center - bounds.center().to_vec2() * self.zoom;
        // A graph larger than the view at the smallest zoom starts from its top left,
        // where its events usually are, instead of being cut on every side.
        if bounds.width() * self.zoom > room.x {
            self.pan.x = 40. - bounds.min.x * self.zoom;
        }
        if bounds.height() * self.zoom > room.y {
            self.pan.y = 40. - bounds.min.y * self.zoom;
        }
    }

    /// Checks types, parameters and references. A failure selects the node it names.
    pub fn validate(&mut self, graph: &Graph, objects: &[(Id, String)]) {
        let ids = objects.iter().map(|(id, _)| id.clone()).collect();
        let (ok, text) = match graph.validate(&ids) {
            Ok(()) => (true, format!("Lógica válida · {}", graph_meta(graph))),
            Err(error) => {
                if let Some(node) = graph.nodes.iter().find(|node| error.contains(&node.id)) {
                    self.selected = Some(node.id.clone());
                    self.selected_edge = None;
                }
                (false, humanize(&error, graph, objects))
            }
        };
        self.status = Some(Status {
            ok,
            text,
            graph: graph.clone(),
        });
    }

    pub fn delete_selected(&mut self, graph: &mut Graph) {
        if let Some(id) = self.selected.take() {
            graph.nodes.retain(|node| node.id != id);
            graph
                .edges
                .retain(|edge| edge.from_node != id && edge.to_node != id);
            if self
                .connecting
                .as_ref()
                .is_some_and(|(source, _)| source == &id)
            {
                self.connecting = None;
            }
        } else if let Some(selected) = self.selected_edge.take() {
            graph.edges.retain(|edge| edge != &selected);
        }
    }

    pub fn duplicate_selected(&mut self, graph: &mut Graph) {
        if let Some(mut copy) = graph
            .nodes
            .iter()
            .find(|node| Some(&node.id) == self.selected.as_ref())
            .cloned()
        {
            copy.id = oxy_core::document::new_id();
            copy.position[0] += 30.0;
            copy.position[1] += 30.0;
            self.selected = Some(copy.id.clone());
            graph.nodes.push(copy);
        }
    }

    /// New nodes appear at the centre of the view, stepped so repeated additions stay
    /// visible, and become the selection.
    fn add_node(&mut self, graph: &mut Graph, op: &'static OperationDef) {
        let size = node_size(Some(op));
        let view = if self.canvas.is_positive() {
            self.canvas.size()
        } else {
            Vec2::new(600., 400.)
        };
        let center = (view * 0.5 - self.pan) / self.zoom;
        let step = (graph.nodes.len() % 6) as f32 * 26.;
        let mut node = Node::new(
            op.id,
            [
                center.x - size.x * 0.5 + step,
                center.y - size.y * 0.5 + step,
            ],
        );
        if op.id == "event.input" {
            node.params
                .insert("action".into(), Value::Text(String::new()));
        }
        self.selected = Some(node.id.clone());
        self.selected_edge = None;
        graph.nodes.push(node);
    }

    /// Drops state that no longer applies: another object, removed nodes or connections.
    fn prepare(&mut self, graph: &Graph, owner: Option<&str>) {
        if self.owner.as_deref() != owner {
            self.owner = owner.map(str::to_owned);
            self.selected = None;
            self.selected_edge = None;
            self.connecting = None;
            self.status = None;
            self.fit_pending = true;
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| graph.node(id).is_none())
        {
            self.selected = None;
        }
        if self
            .selected_edge
            .as_ref()
            .is_some_and(|edge| !graph.edges.contains(edge))
        {
            self.selected_edge = None;
        }
        if self
            .connecting
            .as_ref()
            .is_some_and(|(id, _)| graph.node(id).is_none())
        {
            self.connecting = None;
        }
        if self
            .status
            .as_ref()
            .is_some_and(|status| status.graph != *graph)
        {
            self.status = None;
        }
    }

    /// Catalog and canvas of `graph`, the logic of `owner`.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Graph,
        objects: &[(Id, String)],
        project: &Project,
        scene: &Scene,
        owner: Option<&str>,
        owner_name: &str,
        trace: &[Id],
    ) {
        self.prepare(graph, owner);
        if !text_input_active(ui.ctx()) && ui.ctx().memory(|m| m.top_modal_layer().is_none()) {
            if ui.input(|input| input.key_pressed(egui::Key::Delete)) {
                self.delete_selected(graph);
            }
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::D)) {
                self.duplicate_selected(graph);
            }
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.connecting = None;
            }
        }
        if !catalog_docked(ui.available_width()) {
            // The toolbar offers the catalog as a menu instead.
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    let context = ParameterContext {
                        objects,
                        project,
                        scene,
                        owner,
                        target_connected: false,
                        any_type: false,
                    };
                    self.canvas_ui(ui, graph, &context, owner_name, trace);
                });
            return;
        }
        egui::SidePanel::left("catalogo")
            .resizable(true)
            .default_width(200.)
            .width_range(165.0..=300.0)
            .frame(
                egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 0,
                    bottom: 6,
                }),
            )
            .show_inside(ui, |ui| self.catalog(ui, graph));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let context = ParameterContext {
                    objects,
                    project,
                    scene,
                    owner,
                    target_connected: false,
                    any_type: false,
                };
                self.canvas_ui(ui, graph, &context, owner_name, trace);
            });
    }

    fn catalog(&mut self, ui: &mut egui::Ui, graph: &mut Graph) {
        widgets::panel_header(ui, "Adicionar nó", |_| {});
        ui.add_space(6.);
        self.catalog_search(ui);
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("catalogo_lista")
            // Room for the hint below, which may take two lines.
            .max_height((ui.available_height() - 48.).max(40.))
            .auto_shrink([false, false])
            .show(ui, |ui| self.catalog_list(ui, &mut chosen));
        if let Some(op) = chosen {
            self.add_node(graph, op);
        }
        ui.add_space(4.);
        widgets::hint(ui, "Clique para adicionar ao centro da vista.");
    }

    /// The catalog as a menu, for a central area too narrow to dock it.
    pub fn catalog_menu(&mut self, ui: &mut egui::Ui, graph: &mut Graph) {
        ui.set_width(250.);
        self.catalog_search(ui);
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("catalogo_menu")
            .max_height((ui.ctx().content_rect().height() * 0.6).max(160.))
            .show(ui, |ui| self.catalog_list(ui, &mut chosen));
        if let Some(op) = chosen {
            self.add_node(graph, op);
            ui.close();
        }
    }

    fn catalog_search(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Buscar nó…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.);
    }

    /// Search results, or every category with its nodes.
    fn catalog_list(&mut self, ui: &mut egui::Ui, chosen: &mut Option<&'static OperationDef>) {
        ui.spacing_mut().item_spacing.y = 1.;
        let query = self.search.trim().to_lowercase();
        if !query.is_empty() {
            let mut found = false;
            for op in registry() {
                let text = format!("{} {} {}", op.label, op.category, op.id).to_lowercase();
                if text.contains(&query) {
                    found = true;
                    if catalog_item(ui, op, Some(op.category), 8.).clicked() {
                        *chosen = Some(op);
                    }
                }
            }
            if !found {
                widgets::hint(ui, "Nenhum nó com esse nome.");
            }
            return;
        }
        for (category, ops) in categories() {
            let open = !self.collapsed.contains(category);
            if category_header(ui, category, ops.len(), open).clicked() {
                if open {
                    self.collapsed.insert(*category);
                } else {
                    self.collapsed.remove(*category);
                }
            }
            if open {
                for op in ops {
                    if catalog_item(ui, op, None, 30.).clicked() {
                        *chosen = Some(*op);
                    }
                }
            }
        }
    }

    fn canvas_ui(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Graph,
        context: &ParameterContext<'_>,
        owner_name: &str,
        trace: &[Id],
    ) {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.)),
            Sense::click_and_drag(),
        );
        self.canvas = rect;
        if self.fit_pending {
            self.fit(graph);
        }
        let before = (self.selected.clone(), self.selected_edge.clone());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., theme::BG_BASE);
        // The middle button pans; the wheel zooms around the pointer.
        let pointer = ui.input(|input| input.pointer.hover_pos());
        // Nodes sense clicks, so the canvas response is not hovered over them: check the
        // pointer against the canvas itself, below any popup or dialog.
        let over_canvas = ui.rect_contains_pointer(rect);
        if over_canvas && ui.input(|input| input.pointer.button_down(egui::PointerButton::Middle)) {
            self.pan += ui.input(|input| input.pointer.delta());
        }
        if over_canvas {
            let wheel = ui.input(|input| input.smooth_scroll_delta.y);
            if wheel != 0. {
                let previous = self.zoom;
                self.zoom = (self.zoom * (wheel * 0.001).exp()).clamp(MIN_ZOOM, MAX_ZOOM);
                if let Some(mouse) = pointer {
                    let relative = mouse - rect.min;
                    self.pan = relative - (relative - self.pan) * (self.zoom / previous);
                }
            }
        }
        let zoom = self.zoom;
        // Dot grid, never denser than one dot per 12 points.
        let mut step = 24. * zoom;
        while step < 12. {
            step *= 2.;
        }
        let radius = (1.1 * zoom).clamp(0.7, 1.3);
        let mut y = rect.top() + self.pan.y.rem_euclid(step);
        while y < rect.bottom() {
            let mut x = rect.left() + self.pan.x.rem_euclid(step);
            while x < rect.right() {
                painter.circle_filled(Pos2::new(x, y), radius, CANVAS_DOT);
                x += step;
            }
            y += step;
        }
        let origin = rect.min + self.pan;
        let boxes: Vec<Rect> = graph
            .nodes
            .iter()
            .map(|node| {
                Rect::from_min_size(
                    origin + Vec2::from(node.position) * zoom,
                    node_size(operation(&node.operation)) * zoom,
                )
            })
            .collect();
        let mut ports: HashMap<(Id, String, bool), (Pos2, PortType)> = HashMap::new();
        for (node, node_rect) in graph.nodes.iter().zip(&boxes) {
            let Some(op) = operation(&node.operation) else {
                continue;
            };
            for (output, list) in [(false, &op.inputs), (true, &op.outputs)] {
                for (row, port) in list.iter().enumerate() {
                    let x = if output {
                        node_rect.right()
                    } else {
                        node_rect.left()
                    };
                    ports.insert(
                        (node.id.clone(), port.id.into(), output),
                        (
                            Pos2::new(x, node_rect.top() + row_center(row) * zoom),
                            port.kind,
                        ),
                    );
                }
            }
        }
        // Connections, under the nodes. The selected one gets a halo.
        let pointer_over_node =
            pointer.is_some_and(|pointer| boxes.iter().any(|b| b.expand(9.).contains(pointer)));
        let wire = 2. * zoom.clamp(0.75, 1.25);
        let mut clicked_edge = None;
        for edge in &graph.edges {
            let (Some(&(a, kind)), Some(&(b, _))) = (
                ports.get(&(edge.from_node.clone(), edge.from_port.clone(), true)),
                ports.get(&(edge.to_node.clone(), edge.to_port.clone(), false)),
            ) else {
                continue;
            };
            let points = wire_points(a, b, zoom);
            let hovered = self.connecting.is_none()
                && !pointer_over_node
                && pointer.is_some_and(|pointer| {
                    rect.contains(pointer) && curve_distance(points, pointer) < 7.0
                });
            if self.selected_edge.as_ref() == Some(edge) {
                painter.add(bezier(
                    points,
                    Stroke::new(wire * 4., theme::ACCENT_BRIGHT.gamma_multiply(0.3)),
                ));
            }
            painter.add(bezier(
                points,
                Stroke::new(if hovered { wire * 1.6 } else { wire }, port_color(&kind)),
            ));
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    clicked_edge = Some(edge.clone());
                }
            }
        }
        if let Some(edge) = clicked_edge {
            self.selected = None;
            self.selected_edge = Some(edge);
        } else if response.clicked() {
            // A click on the empty canvas clears the selection and any pending connection.
            self.selected = None;
            self.selected_edge = None;
            self.connecting = None;
        }
        if let Some((id, port)) = &self.connecting
            && let Some(&(a, kind)) = ports.get(&(id.clone(), port.clone(), true))
            && let Some(mouse) = pointer
        {
            painter.add(bezier(
                wire_points(a, mouse, zoom),
                Stroke::new(wire, port_color(&kind)),
            ));
        }
        // Nodes, in document order: later nodes draw and receive input on top.
        let connected: HashSet<(String, String)> = graph
            .edges
            .iter()
            .map(|edge| (edge.to_node.clone(), edge.to_port.clone()))
            .collect();
        let mut select = None;
        let mut moved = None;
        let mut command = None;
        let mut new_edge = None;
        for (index, node) in graph.nodes.iter().enumerate() {
            let node_rect = boxes[index];
            let op = operation(&node.operation);
            let body = ui.interact(
                node_rect,
                ui.id().with(("logic_node", &node.id)),
                Sense::click(),
            );
            draw_node(ui, &painter, node, op, node_rect, zoom, context, &connected);
            let header = Rect::from_min_size(
                node_rect.min,
                Vec2::new(node_rect.width(), HEADER_HEIGHT * zoom),
            );
            let title = ui.interact(
                header,
                ui.id().with(("logic_title", &node.id)),
                Sense::click_and_drag(),
            );
            for response in [&body, &title] {
                response.context_menu(|ui| {
                    if let Some(op) = op {
                        if ui.button("Ajuda deste nó").clicked() {
                            command = Some(NodeCommand::Help(op.id));
                            ui.close();
                        }
                        ui.separator();
                    }
                    if widgets::menu_item(ui, "Duplicar nó", "Ctrl+D").clicked() {
                        command = Some(NodeCommand::Duplicate(node.id.clone()));
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Excluir nó").color(theme::ERROR))
                                .shortcut_text("Delete"),
                        )
                        .clicked()
                    {
                        command = Some(NodeCommand::Delete(node.id.clone()));
                        ui.close();
                    }
                });
            }
            if body.clicked()
                || title.clicked()
                || title.drag_started_by(egui::PointerButton::Primary)
            {
                select = Some(node.id.clone());
            }
            if title.dragged_by(egui::PointerButton::Primary) {
                moved = Some((index, ui.input(|input| input.pointer.delta()) / zoom));
            } else if title.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            // Outline on top of the header, so the selection reads on every side.
            let radius = (7. * zoom).round() as u8;
            if self.selected.as_ref() == Some(&node.id) {
                painter.rect_stroke(
                    node_rect.expand(2.),
                    radius + 2,
                    Stroke::new(4., theme::ACCENT_BRIGHT.gamma_multiply(0.22)),
                    egui::StrokeKind::Outside,
                );
                painter.rect_stroke(
                    node_rect,
                    radius,
                    Stroke::new(2., theme::ACCENT_BRIGHT),
                    egui::StrokeKind::Outside,
                );
            } else if trace.contains(&node.id) {
                painter.rect_stroke(
                    node_rect,
                    radius,
                    Stroke::new(2., theme::HIGHLIGHT),
                    egui::StrokeKind::Outside,
                );
            } else {
                painter.rect_stroke(
                    node_rect,
                    radius,
                    Stroke::new(1., NODE_BORDER),
                    egui::StrokeKind::Inside,
                );
            }
            let Some(op) = op else {
                continue;
            };
            for (output, list) in [(false, &op.inputs), (true, &op.outputs)] {
                for port in list {
                    let Some(&(position, kind)) =
                        ports.get(&(node.id.clone(), port.id.into(), output))
                    else {
                        continue;
                    };
                    draw_port(&painter, position, kind, zoom);
                    let response = ui
                        .interact(
                            Rect::from_center_size(position, Vec2::splat(18. * zoom.max(0.7))),
                            ui.id().with(("logic_port", &node.id, port.id, output)),
                            Sense::click_and_drag(),
                        )
                        .on_hover_text(format!(
                            "{} · {} de {}",
                            port.label,
                            if output { "Saída" } else { "Entrada" },
                            port.kind.label().to_lowercase()
                        ));
                    if output && (response.clicked() || response.drag_started()) {
                        self.connecting = Some((node.id.clone(), port.id.into()));
                    }
                    let released = ui
                        .input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                        && response.contains_pointer();
                    if !output
                        && (response.clicked() || released)
                        && let Some((from, from_port)) = self.connecting.take()
                    {
                        new_edge = Some(Edge {
                            from_node: from,
                            from_port,
                            to_node: node.id.clone(),
                            to_port: port.id.into(),
                        });
                    }
                }
            }
        }
        if let Some((index, delta)) = moved {
            let position = &mut graph.nodes[index].position;
            position[0] += delta.x;
            position[1] += delta.y;
        }
        if let Some(id) = select {
            self.selected = Some(id);
            self.selected_edge = None;
        }
        match command {
            Some(NodeCommand::Help(topic)) => self.guide_requested = Some(topic.into()),
            Some(NodeCommand::Duplicate(id)) => {
                self.selected = Some(id);
                self.duplicate_selected(graph);
            }
            Some(NodeCommand::Delete(id)) => {
                self.selected = Some(id);
                self.selected_edge = None;
                self.delete_selected(graph);
            }
            None => {}
        }
        if let Some(edge) = new_edge {
            let ids: HashSet<_> = context.objects.iter().map(|(id, _)| id.clone()).collect();
            if let Err(error) = graph.connect(&edge, &ids) {
                self.status = Some(Status {
                    ok: false,
                    text: humanize(&error, graph, context.objects),
                    graph: graph.clone(),
                });
            }
        }
        legend(ui, rect);
        self.status_chip(ui, rect);
        if let Some((id, port)) = &self.connecting {
            connecting_chip(ui, rect, port_label(graph, id, port, true));
        }
        if graph.nodes.is_empty() {
            self.starter_card(ui, rect, graph, owner_name);
        }
        if (self.selected.clone(), self.selected_edge.clone()) != before {
            // The inspector drew before the canvas: show it the new selection now.
            ui.ctx().request_repaint();
        }
    }

    fn status_chip(&mut self, ui: &egui::Ui, canvas: Rect) {
        let Some(status) = &self.status else {
            return;
        };
        let color = if status.ok {
            theme::SUCCESS
        } else {
            theme::WARNING
        };
        let painter = ui.painter_at(canvas);
        let text = truncated(
            &painter,
            &status.text,
            FontId::proportional(12.),
            color,
            (canvas.width() - 90.).max(40.),
        );
        let size = Vec2::new(text.size().x + 42., 28.);
        let chip = Rect::from_min_size(
            Pos2::new(canvas.right() - 12. - size.x, canvas.top() + 12.),
            size,
        );
        painter.rect(
            chip,
            6.,
            theme::BG_PANEL,
            Stroke::new(1., color.gamma_multiply(0.55)),
            egui::StrokeKind::Inside,
        );
        let icon = Rect::from_center_size(
            Pos2::new(chip.left() + 16., chip.center().y),
            Vec2::splat(14.),
        );
        if status.ok {
            icons::paint(ui, icon, Icon::Check, color);
        } else {
            widgets::warning_icon(ui, icon, color);
        }
        painter.galley(
            Pos2::new(chip.left() + 30., chip.center().y - text.size().y / 2.),
            text,
            color,
        );
        if ui
            .interact(chip, ui.id().with("logic_status"), Sense::click())
            .on_hover_text("Clique para dispensar.")
            .clicked()
        {
            self.status = None;
        }
    }

    /// An object without logic starts from an event.
    fn starter_card(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        graph: &mut Graph,
        owner_name: &str,
    ) {
        let width = (canvas.width() - 48.).min(400.);
        if width < 240. || canvas.height() < 230. {
            let painter = ui.painter_at(canvas);
            let galley = painter.layout(
                "Comece por um evento em Adicionar nó.".to_owned(),
                FontId::proportional(12.5),
                theme::TEXT_MUTED,
                (canvas.width() - 24.).max(40.),
            );
            painter.galley(
                canvas.center() - galley.size() / 2.,
                galley,
                theme::TEXT_MUTED,
            );
            return;
        }
        let area =
            Rect::from_center_size(canvas.center() - Vec2::new(0., 20.), Vec2::new(width, 220.));
        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(Layout::top_down(Align::Min)),
        );
        let mut chosen = None;
        egui::Frame::new()
            .fill(theme::BG_PANEL)
            .stroke(Stroke::new(1., theme::BORDER))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(22, 18))
            .shadow(egui::epaint::Shadow {
                offset: [0, 12],
                blur: 32,
                spread: 0,
                color: Color32::from_black_alpha(100),
            })
            .show(&mut card, |ui| {
                ui.set_width(width - 44.);
                ui.spacing_mut().item_spacing.y = 8.;
                ui.label(
                    RichText::new(format!("{owner_name} ainda não tem lógica"))
                        .font(theme::medium(14.))
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new(
                        "Comece por um evento: ele decide quando as ações acontecem. Depois conecte a saída Executar às ações.",
                    )
                    .size(12.5)
                    .color(Color32::from_rgb(168, 168, 174)),
                );
                let column = (ui.available_width() - 6.) / 2.;
                for pair in STARTERS.chunks(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.;
                        for id in pair {
                            if let Some(op) = operation(id)
                                && starter_button(ui, op, column).clicked()
                            {
                                chosen = Some(op);
                            }
                        }
                    });
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Abrir o guia: seu primeiro comportamento")
                                .color(theme::ACCENT_BRIGHT),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.guide_requested = Some(String::new());
                }
            });
        if let Some(op) = chosen {
            self.add_node(graph, op);
        }
    }

    /// Node or connection inspector, drawn in the Propriedades panel.
    #[allow(clippy::too_many_arguments)]
    pub fn inspector(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Graph,
        objects: &[(Id, String)],
        project: &Project,
        scene: &Scene,
        owner: Option<&str>,
    ) {
        self.prepare(graph, owner);
        if let Some((node, port)) = self.connecting.clone() {
            egui::Frame::new()
                .fill(theme::ACCENT_SOFT)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!(
                            "Conectando: {}",
                            port_label(graph, &node, &port, true)
                        ))
                        .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new("Clique numa entrada compatível. Esc cancela.")
                            .size(11.5)
                            .color(theme::TEXT_MUTED),
                    );
                    if ui.button("Cancelar conexão").clicked() {
                        self.connecting = None;
                    }
                });
            ui.add_space(4.);
        }
        if let Some(index) = self
            .selected
            .as_ref()
            .and_then(|id| graph.nodes.iter().position(|node| &node.id == id))
        {
            let context = ParameterContext {
                objects,
                project,
                scene,
                owner,
                target_connected: false,
                any_type: false,
            };
            self.node_inspector(ui, graph, index, &context);
        } else if let Some(edge) = self.selected_edge.clone() {
            self.edge_inspector(ui, graph, &edge);
        } else {
            ui.label(
                RichText::new("Nenhum nó selecionado")
                    .font(theme::medium(13.))
                    .color(theme::TEXT),
            );
            widgets::hint(
                ui,
                "Selecione um nó pelo título para ver seus parâmetros e conexões.",
            );
            ui.add_space(4.);
            for tip in [
                "Conecte arrastando de uma saída até uma entrada.",
                "Botão do meio move a área; a roda aproxima.",
                "Delete exclui; Ctrl+D duplica o nó.",
            ] {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::new(10., 14.), Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 2., theme::TEXT_MUTED);
                    ui.add(
                        egui::Label::new(RichText::new(tip).size(12.).color(theme::TEXT_MUTED))
                            .wrap(),
                    );
                });
            }
        }
    }

    fn node_inspector(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Graph,
        index: usize,
        context: &ParameterContext<'_>,
    ) {
        let node_id = graph.nodes[index].id.clone();
        let op = operation(&graph.nodes[index].operation);
        let mut duplicate = false;
        let mut delete = false;
        ui.horizontal(|ui| {
            let event = op.is_some_and(|op| op.category == "Eventos");
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(26.), Sense::hover());
            ui.painter().rect_filled(
                tile,
                6.,
                if event {
                    EVENT_HEADER
                } else {
                    theme::BG_HEADER
                },
            );
            icons::paint(
                ui,
                Rect::from_center_size(tile.center(), Vec2::splat(15.)),
                op.map_or(Icon::Logic, |op| category_icon(op.category)),
                if event {
                    theme::ACCENT_BRIGHT
                } else {
                    theme::TEXT
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let more = icons::small(ui, Icon::More, "Mais ações do nó", 24.);
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(190.);
                    if widgets::menu_item(ui, "Duplicar nó", "Ctrl+D").clicked() {
                        duplicate = true;
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Excluir nó").color(theme::ERROR))
                                .shortcut_text("Delete"),
                        )
                        .clicked()
                    {
                        delete = true;
                        ui.close();
                    }
                });
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(op.map_or("Nó desconhecido", |op| op.label))
                                .font(theme::medium(14.))
                                .color(theme::TEXT),
                        )
                        .truncate(),
                    );
                });
            });
        });
        let Some(op) = op else {
            widgets::hint(
                ui,
                "Este nó veio de uma versão mais nova ou de um projeto alterado. Exclua-o ou abra o projeto na versão que o criou.",
            );
            if ui.button("Excluir nó").clicked() {
                delete = true;
            }
            if delete {
                self.delete_selected(graph);
            }
            return;
        };
        ui.label(
            RichText::new(op.category)
                .size(11.5)
                .color(theme::TEXT_MUTED),
        );
        ui.label(
            RichText::new(operation_description(op.id))
                .size(12.)
                .color(Color32::from_rgb(176, 176, 182)),
        );
        let help = ui.add(
            egui::Button::new(RichText::new("Ajuda deste nó").color(theme::ACCENT_BRIGHT))
                .frame(false),
        );
        if help.clicked() {
            self.guide_requested = Some(op.id.into());
        }
        if op.id == "event.input" && ui.button("Configurar ações de entrada").clicked() {
            self.configure_inputs = true;
        }
        ui.add_space(4.);
        // What feeds each parameter that also has an input port.
        let sources: Vec<Option<String>> = op
            .params
            .iter()
            .map(|param| {
                graph
                    .edges
                    .iter()
                    .find(|edge| edge.to_node == node_id && edge.to_port == param.id)
                    .map(|edge| {
                        format!(
                            "{} · {}",
                            node_label(graph, &edge.from_node),
                            port_label(graph, &edge.from_node, &edge.from_port, true)
                        )
                    })
            })
            .collect();
        let target_connected = graph
            .edges
            .iter()
            .any(|edge| edge.to_node == node_id && edge.to_port == "target");
        let with_ports = op
            .params
            .iter()
            .any(|param| op.inputs.iter().any(|port| port.id == param.id));
        widgets::open_card(ui, "logic_parameters", "Parâmetros", |ui| {
            if op.params.is_empty() {
                widgets::hint(ui, "Este nó não tem parâmetros.");
                return;
            }
            let labels: Vec<_> = op.params.iter().map(|p| param_label(op.id, p)).collect();
            let column = widgets::label_column(ui, &labels);
            let node = &mut graph.nodes[index];
            for (param, source) in op.params.iter().zip(&sources) {
                widgets::property_row_with(ui, column, param_label(op.id, param), "", |ui| {
                    if let Some(source) = source {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("← {source}"))
                                    .size(12.)
                                    .color(theme::TEXT_MUTED),
                            )
                            .truncate(),
                        )
                        .on_hover_text("Uma entrada conectada recebe o valor do outro nó.");
                    } else {
                        ui.vertical(|ui| {
                            let context = ParameterContext {
                                target_connected,
                                any_type: op
                                    .inputs
                                    .iter()
                                    .any(|port| port.id == param.id && port.kind == PortType::Any),
                                ..*context
                            };
                            parameter_editor(ui, node, param, &context);
                        });
                    }
                });
            }
            if with_ports {
                widgets::hint(
                    ui,
                    "Parâmetros são valores padrão. Uma entrada conectada recebe o valor do outro nó.",
                );
            }
        });
        widgets::open_card(ui, "logic_connections", "Conexões", |ui| {
            let ports: Vec<(bool, &PortDef)> = op
                .inputs
                .iter()
                .map(|port| (false, port))
                .chain(op.outputs.iter().map(|port| (true, port)))
                .collect();
            if ports.is_empty() {
                widgets::hint(ui, "Este nó não tem portas.");
            }
            for (output, port) in ports {
                let links: Vec<String> = graph
                    .edges
                    .iter()
                    .filter(|edge| {
                        if output {
                            edge.from_node == node_id && edge.from_port == port.id
                        } else {
                            edge.to_node == node_id && edge.to_port == port.id
                        }
                    })
                    .map(|edge| {
                        if output {
                            format!(
                                "→ {} · {}",
                                node_label(graph, &edge.to_node),
                                port_label(graph, &edge.to_node, &edge.to_port, false)
                            )
                        } else {
                            format!(
                                "← {} · {}",
                                node_label(graph, &edge.from_node),
                                port_label(graph, &edge.from_node, &edge.from_port, true)
                            )
                        }
                    })
                    .collect();
                ui.horizontal(|ui| {
                    let (marker, _) = ui.allocate_exact_size(Vec2::new(12., 16.), Sense::hover());
                    port_marker(ui.painter(), marker.center(), port.kind);
                    ui.label(RichText::new(port.label).size(12.).color(theme::TEXT));
                    if links.is_empty() {
                        ui.label(
                            RichText::new("sem conexão")
                                .size(11.5)
                                .color(theme::TEXT_MUTED),
                        );
                    }
                });
                for link in links {
                    ui.horizontal(|ui| {
                        ui.add_space(16.);
                        ui.add(
                            egui::Label::new(
                                RichText::new(link).size(11.5).color(theme::TEXT_MUTED),
                            )
                            .truncate(),
                        );
                    });
                }
            }
        });
        if duplicate {
            self.duplicate_selected(graph);
        }
        if delete {
            self.delete_selected(graph);
        }
    }

    fn edge_inspector(&mut self, ui: &mut egui::Ui, graph: &mut Graph, edge: &Edge) {
        let kind = port_def(graph, &edge.from_node, &edge.from_port, true).map(|port| port.kind);
        let mut delete = false;
        ui.horizontal(|ui| {
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(26.), Sense::hover());
            ui.painter().rect_filled(tile, 6., theme::BG_HEADER);
            icons::paint(
                ui,
                Rect::from_center_size(tile.center(), Vec2::splat(15.)),
                Icon::Logic,
                theme::TEXT,
            );
            ui.label(
                RichText::new("Conexão")
                    .font(theme::medium(14.))
                    .color(theme::TEXT),
            );
        });
        ui.label(
            RichText::new(match kind {
                Some(PortType::Exec) => "Execução".to_owned(),
                Some(kind) => format!("Dados · {}", kind.label()),
                None => "Conexão sem porta".to_owned(),
            })
            .size(11.5)
            .color(theme::TEXT_MUTED),
        );
        ui.add_space(4.);
        widgets::open_card(ui, "logic_edge", "Origem e destino", |ui| {
            ui.label(format!(
                "{} · {} → {} · {}",
                node_label(graph, &edge.from_node),
                port_label(graph, &edge.from_node, &edge.from_port, true),
                node_label(graph, &edge.to_node),
                port_label(graph, &edge.to_node, &edge.to_port, false)
            ));
        });
        if ui
            .add(
                egui::Button::new(RichText::new("Excluir conexão").color(theme::ERROR))
                    .shortcut_text("Delete"),
            )
            .clicked()
        {
            delete = true;
        }
        if delete {
            self.delete_selected(graph);
        }
    }
}

fn catalog_item(
    ui: &mut egui::Ui,
    op: &OperationDef,
    category: Option<&str>,
    indent: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), op.label)
    });
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 5., theme::ACCENT_SOFT);
    }
    let painter = ui.painter_at(rect);
    let mut right = rect.right() - 8.;
    if let Some(category) = category {
        let tag = painter.layout_no_wrap(
            category.to_owned(),
            FontId::proportional(11.),
            theme::TEXT_MUTED,
        );
        right -= tag.size().x;
        painter.galley(
            Pos2::new(right, rect.center().y - tag.size().y / 2.),
            tag,
            theme::TEXT_MUTED,
        );
        right -= 8.;
    }
    let color = if hovered {
        Color32::WHITE
    } else {
        Color32::from_rgb(196, 196, 202)
    };
    let label = truncated(
        &painter,
        op.label,
        FontId::proportional(12.5),
        color,
        right - rect.left() - indent,
    );
    painter.galley(
        Pos2::new(rect.left() + indent, rect.center().y - label.size().y / 2.),
        label,
        color,
    );
    response.on_hover_text(operation_description(op.id))
}

fn category_header(ui: &mut egui::Ui, category: &str, count: usize, open: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::CollapsingHeader,
            ui.is_enabled(),
            open,
            category,
        )
    });
    if response.hovered() {
        ui.painter().rect_filled(rect, 5., theme::BG_RAISED);
    }
    let y = rect.center().y;
    icons::paint(
        ui,
        Rect::from_center_size(Pos2::new(rect.left() + 10., y), Vec2::splat(12.)),
        if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        theme::TEXT_MUTED,
    );
    icons::paint(
        ui,
        Rect::from_center_size(Pos2::new(rect.left() + 26., y), Vec2::splat(13.)),
        category_icon(category),
        theme::ACCENT_BRIGHT,
    );
    let painter = ui.painter_at(rect);
    painter.text(
        Pos2::new(rect.left() + 38., y),
        egui::Align2::LEFT_CENTER,
        category,
        theme::medium(12.5),
        theme::TEXT,
    );
    painter.text(
        Pos2::new(rect.right() - 8., y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        FontId::proportional(11.5),
        theme::TEXT_MUTED,
    );
    response
}

fn starter_button(ui: &mut egui::Ui, op: &OperationDef, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), op.label)
    });
    let fill = if response.hovered() {
        Color32::from_rgb(60, 60, 65)
    } else {
        Color32::from_rgb(48, 48, 52)
    };
    ui.painter().rect_filled(rect, 6., fill);
    icons::paint(
        ui,
        Rect::from_center_size(
            Pos2::new(rect.left() + 15., rect.center().y),
            Vec2::splat(13.),
        ),
        Icon::Bolt,
        theme::ACCENT_BRIGHT,
    );
    let painter = ui.painter_at(rect);
    let label = truncated(
        &painter,
        op.label,
        FontId::proportional(12.5),
        theme::TEXT,
        rect.width() - 36.,
    );
    painter.galley(
        Pos2::new(rect.left() + 28., rect.center().y - label.size().y / 2.),
        label,
        theme::TEXT,
    );
    response.on_hover_text(operation_description(op.id))
}

/// Port colours, bottom left of the canvas. Hidden when the canvas is too narrow.
fn legend(ui: &egui::Ui, canvas: Rect) {
    let items = [
        (PortType::Exec, "Execução"),
        (PortType::Object, "Objeto"),
        (PortType::Bool, "Booleano"),
        (PortType::Number, "Número"),
        (PortType::Text, "Texto"),
        (PortType::Any, "Qualquer"),
    ];
    let painter = ui.painter_at(canvas);
    let galleys: Vec<_> = items
        .iter()
        .map(|(_, label)| {
            painter.layout_no_wrap(
                (*label).to_owned(),
                FontId::proportional(11.5),
                theme::TEXT_MUTED,
            )
        })
        .collect();
    let width = 10. + galleys.iter().map(|g| 14. + g.size().x + 12.).sum::<f32>();
    if width > canvas.width() - 24. || canvas.height() < 120. {
        return;
    }
    let chip = Rect::from_min_size(
        Pos2::new(canvas.left() + 12., canvas.bottom() - 40.),
        Vec2::new(width, 28.),
    );
    painter.rect(
        chip,
        6.,
        theme::BG_PANEL,
        Stroke::new(1., theme::BORDER),
        egui::StrokeKind::Inside,
    );
    let mut x = chip.left() + 10.;
    for ((kind, _), galley) in items.iter().zip(galleys) {
        port_marker(&painter, Pos2::new(x + 5., chip.center().y), *kind);
        x += 14.;
        let width = galley.size().x;
        painter.galley(
            Pos2::new(x, chip.center().y - galley.size().y / 2.),
            galley,
            theme::TEXT_MUTED,
        );
        x += width + 12.;
    }
}

/// While a connection is pending: from which output, and how to cancel.
fn connecting_chip(ui: &egui::Ui, canvas: Rect, port: &str) {
    let painter = ui.painter_at(canvas);
    let text = painter.layout_no_wrap(
        format!("Conectando: {port}"),
        FontId::proportional(12.),
        Color32::WHITE,
    );
    let hint = painter.layout_no_wrap(
        " · clique numa entrada; Esc cancela".to_owned(),
        FontId::proportional(12.),
        theme::TEXT_MUTED,
    );
    let size = Vec2::new(text.size().x + hint.size().x + 24., 28.);
    let chip = Rect::from_min_size(canvas.left_top() + Vec2::new(12., 12.), size);
    painter.rect(
        chip,
        6.,
        theme::ACCENT_SOFT,
        Stroke::new(1., theme::ACCENT),
        egui::StrokeKind::Inside,
    );
    let y = chip.center().y - text.size().y / 2.;
    let text_width = text.size().x;
    painter.galley(Pos2::new(chip.left() + 12., y), text, Color32::WHITE);
    painter.galley(
        Pos2::new(chip.left() + 12. + text_width, y),
        hint,
        theme::TEXT_MUTED,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::{
        animation::Clip,
        document::{Asset, AssetKind, Entity, SceneKind, new_id},
        graph::ParamDef,
    };

    /// A context with the editor fonts: nodes and headers use the medium family.
    fn themed() -> egui::Context {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        ctx
    }

    fn text_center(output: &egui::FullOutput, label: &str) -> Pos2 {
        fn find(shape: &egui::Shape, label: &str) -> Option<Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
                _ => None,
            }
        }
        output
            .shapes
            .iter()
            .find_map(|shape| find(&shape.shape, label))
            .unwrap_or_else(|| panic!("UI text not rendered: {label}"))
    }

    fn parameter_frame(
        ctx: &egui::Context,
        node: &mut Node,
        param: &ParamDef,
        context: &ParameterContext<'_>,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 500.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| parameter_editor(ui, node, param, context));
            },
        )
    }

    fn click_parameter(
        ctx: &egui::Context,
        node: &mut Node,
        param: &ParamDef,
        context: &ParameterContext<'_>,
        position: Pos2,
    ) -> egui::FullOutput {
        parameter_frame(
            ctx,
            node,
            param,
            context,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        parameter_frame(
            ctx,
            node,
            param,
            context,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        )
    }

    /// One frame with the inspector in a side panel and the catalog and canvas beside it.
    fn logic_frame(
        ctx: &egui::Context,
        view: &mut GraphView,
        graph: &mut Graph,
        project: &Project,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1000.0))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::right("propriedades").show(ctx, |ui| {
                    view.inspector(ui, graph, &[], project, &project.scenes[0], None);
                });
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.show(
                        ui,
                        graph,
                        &[],
                        project,
                        &project.scenes[0],
                        None,
                        "Objeto",
                        &[],
                    );
                });
            },
        )
    }

    #[test]
    fn named_parameters_are_editable_through_real_egui_clicks_and_keep_stable_ids() {
        let mut project = Project::new("Seleção por nomes");
        let mut owner = Entity::new("Peça animada", None);
        let clip = Clip::new("Golpe editável");
        let clip_id = clip.id.clone();
        owner.clips.push(clip);
        let owner_id = owner.id.clone();
        project.scenes[0].entities.push(owner);
        let next_scene = Scene::new("Outra cena", SceneKind::ThreeD);
        let next_id = next_scene.id.clone();
        project.scenes.push(next_scene);
        let audio_id = new_id();
        project.assets.push(Asset {
            id: audio_id.clone(),
            name: "Som de impacto".into(),
            path: "assets/impacto.wav".into(),
            kind: AssetKind::Audio,
            model: None,
        });
        let scene = &project.scenes[0];
        let context = ParameterContext {
            objects: &[],
            project: &project,
            scene,
            owner: Some(&owner_id),
            target_connected: false,
            any_type: false,
        };
        for (operation, key, label, expected) in [
            (
                "action.animation",
                "clip",
                "Peça animada · Golpe editável",
                clip_id,
            ),
            ("action.sound", "asset", "Som de impacto", audio_id),
            ("action.scene", "scene", "Outra cena", next_id),
        ] {
            let ctx = themed();
            let mut node = Node::new(operation, [0.0; 2]);
            let definition = registry().iter().find(|op| op.id == operation).unwrap();
            let param = definition
                .params
                .iter()
                .find(|param| param.id == key)
                .unwrap();
            let output = parameter_frame(&ctx, &mut node, param, &context, vec![]);
            let button = text_center(&output, "Escolha…");
            click_parameter(&ctx, &mut node, param, &context, button);
            let output = parameter_frame(&ctx, &mut node, param, &context, vec![]);
            let choice = text_center(&output, label);
            click_parameter(&ctx, &mut node, param, &context, choice);
            assert_eq!(node.params[key], Value::Text(expected));
            if operation == "action.animation" {
                assert_eq!(node.params["target"], Value::Object(Some(owner_id.clone())));
            }
        }
    }

    #[test]
    fn node_and_connection_panels_show_descriptions_and_port_labels_instead_of_ids() {
        let ctx = themed();
        let project = Project::new("Rótulos de lógica");
        let source = Node::new("event.click", [15.0, 15.0]);
        let target = Node::new("attribute.get", [260.0, 15.0]);
        let edge = Edge {
            from_node: source.id.clone(),
            from_port: "context".into(),
            to_node: target.id.clone(),
            to_port: "target".into(),
        };
        let mut graph = Graph {
            nodes: vec![source.clone(), target],
            edges: vec![edge.clone()],
        };
        let mut view = GraphView {
            selected: Some(source.id.clone()),
            ..Default::default()
        };
        for connection in [false, true] {
            if connection {
                view.selected = None;
                view.selected_edge = Some(edge.clone());
                view.connecting = Some((source.id.clone(), "context".into()));
            }
            let output = logic_frame(&ctx, &mut view, &mut graph, &project, vec![]);
            let expected = if connection {
                "Ao clicar · Objeto do evento → Ler atributo · Objeto (vazio = este)"
            } else {
                operation_description("event.click")
            };
            text_center(&output, expected);
            if connection {
                text_center(&output, "Conectando: Objeto do evento");
            }
        }
        assert_eq!(
            graph.edges[0], edge,
            "The displayed labels must not replace persisted port IDs"
        );
    }

    #[test]
    fn every_operation_is_described_and_listed_once_in_the_catalog() {
        let listed: Vec<_> = categories()
            .iter()
            .flat_map(|(_, ops)| ops.iter().map(|op| op.id))
            .collect();
        assert_eq!(listed.len(), registry().len());
        let names: HashSet<_> = categories().iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names.len(),
            categories().len(),
            "Each category appears once"
        );
        for op in registry() {
            assert!(
                !operation_description(op.id).contains("desconhecida"),
                "{} has no description",
                op.id
            );
        }
    }

    #[test]
    fn messages_name_nodes_and_never_show_identifiers() {
        let node = Node::new("debug.message", [0.0; 2]);
        let graph = Graph {
            nodes: vec![node.clone()],
            edges: Vec::new(),
        };
        let missing = new_id();
        let text = humanize(
            &format!(
                "Nó {}: referência de objeto inexistente em target: {missing}",
                node.id
            ),
            &graph,
            &[],
        );
        assert_eq!(
            text,
            "Nó Mensagem de diagnóstico: referência de objeto inexistente em target: (referência removida)"
        );
    }

    #[test]
    fn new_nodes_appear_at_the_centre_of_the_view_and_are_selected() {
        let ctx = themed();
        let project = Project::new("Centro da vista");
        let mut graph = Graph::default();
        let mut view = GraphView::default();
        logic_frame(&ctx, &mut view, &mut graph, &project, vec![]);
        let canvas = view.canvas;
        let op = operation("debug.message").unwrap();
        view.add_node(&mut graph, op);
        let node = &graph.nodes[0];
        let center = canvas.min
            + view.pan
            + (Vec2::from(node.position) + node_size(Some(op)) * 0.5) * view.zoom;
        assert!(center.distance(canvas.center()) < 1.0);
        assert_eq!(view.selected.as_ref(), Some(&node.id));
    }

    #[test]
    fn framing_shows_small_graphs_whole_and_large_ones_from_their_top_left() {
        let mut view = GraphView {
            canvas: Rect::from_min_size(Pos2::new(300.0, 100.0), Vec2::new(800.0, 600.0)),
            ..GraphView::default()
        };
        let small = Graph {
            nodes: vec![
                Node::new("event.click", [100.0, 50.0]),
                Node::new("debug.message", [400.0, 120.0]),
            ],
            edges: Vec::new(),
        };
        view.fit(&small);
        let bounds = graph_bounds(&small).unwrap();
        let screen = |view: &GraphView, point: Pos2| {
            view.canvas.min + view.pan + point.to_vec2() * view.zoom
        };
        assert_eq!(view.zoom, 1.0);
        assert!(view.canvas.contains(screen(&view, bounds.min)));
        assert!(view.canvas.contains(screen(&view, bounds.max)));
        let large = Graph {
            nodes: (0..60)
                .map(|i| Node::new("debug.message", [i as f32 * 300.0, 20.0 + i as f32 * 120.0]))
                .collect(),
            edges: Vec::new(),
        };
        view.fit(&large);
        let bounds = graph_bounds(&large).unwrap();
        assert_eq!(view.zoom, MIN_ZOOM);
        let corner = screen(&view, bounds.min);
        assert!((corner - view.canvas.min - Vec2::splat(40.0)).length() < 0.01);
    }

    #[test]
    fn curved_connection_can_be_picked_away_from_its_midpoint() {
        let points = [
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            Pos2::new(0.0, 100.0),
            Pos2::new(100.0, 100.0),
        ];
        let at_quarter = Pos2::new(43.75, 15.625);
        assert!(curve_distance(points, at_quarter) < 0.1);
        assert!(at_quarter.distance(Pos2::new(50.0, 50.0)) > 30.0);
        assert!(curve_distance(points, Pos2::new(180.0, 10.0)) > 70.0);
    }

    #[test]
    fn deleting_or_duplicating_graph_selection_never_changes_an_entity() {
        let first = Node::new("event.click", [10.0, 15.0]);
        let second = Node::new("debug.message", [230.0, 15.0]);
        let edge = Edge {
            from_node: first.id.clone(),
            from_port: "exec".into(),
            to_node: second.id.clone(),
            to_port: "exec".into(),
        };
        let mut graph = Graph {
            nodes: vec![first, second.clone()],
            edges: vec![edge.clone()],
        };
        let mut view = GraphView {
            selected: Some(second.id.clone()),
            ..GraphView::default()
        };
        view.duplicate_selected(&mut graph);
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 1);
        let copy = graph.nodes.last().unwrap();
        assert_ne!(copy.id, second.id);
        assert_eq!(copy.params, second.params);
        view.delete_selected(&mut graph);
        assert_eq!(graph.nodes.len(), 2);
        view.selected_edge = Some(edge);
        view.delete_selected(&mut graph);
        assert!(graph.edges.is_empty());
        assert_eq!(graph.nodes.len(), 2);
        view.selected = Some(second.id);
        view.delete_selected(&mut graph);
        assert_eq!(graph.nodes.len(), 1);
    }

    #[test]
    fn focused_buttons_allow_quick_shortcuts_but_focused_text_edits_protect_typing() {
        for focus_text in [false, true] {
            let ctx = themed();
            let project = Project::new("Foco de teclado");
            let node = Node::new("debug.message", [30.0, 30.0]);
            let mut graph = Graph {
                nodes: vec![node.clone()],
                edges: Vec::new(),
            };
            let mut view = GraphView {
                selected: Some(node.id),
                ..GraphView::default()
            };
            let mut typed = "Campo de texto".to_owned();
            let mut blocked = false;
            let mut any_keyboard_focus = false;
            for press_shortcut in [false, true] {
                let events = if press_shortcut {
                    vec![egui::Event::Key {
                        key: egui::Key::D,
                        physical_key: Some(egui::Key::D),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..egui::Modifiers::NONE
                        },
                    }]
                } else {
                    Vec::new()
                };
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1200.0, 900.0),
                        )),
                        // The chord's modifier was released before this frame finished.
                        // Its original modifier remains correctly recorded on the key event.
                        modifiers: egui::Modifiers::NONE,
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let button = ui.button("Botão com foco de teclado");
                            let text = ui.text_edit_singleline(&mut typed);
                            if !press_shortcut {
                                if focus_text {
                                    text.request_focus();
                                } else {
                                    button.request_focus();
                                }
                            }
                            blocked = text_input_active(ctx);
                            any_keyboard_focus = ctx.wants_keyboard_input();
                            view.show(
                                ui,
                                &mut graph,
                                &[],
                                &project,
                                &project.scenes[0],
                                None,
                                "Objeto",
                                &[],
                            );
                        });
                    },
                );
            }
            assert!(
                any_keyboard_focus,
                "Both widgets really hold egui keyboard focus"
            );
            assert_eq!(
                blocked, focus_text,
                "Only a TextEdit should block editor shortcuts"
            );
            assert_eq!(
                graph.nodes.len(),
                if focus_text { 1 } else { 2 },
                "Ctrl+D is consumed using event modifiers, while text editing remains protected"
            );
        }
    }
}
