mod parameters;
use parameters::{ParameterContext, parameter_editor};
pub use parameters::{collision_filter_picker, object_picker, value_editor};

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use oxy_core::document::{Id, Project, Scene, new_id};
use oxy_core::graph::{Edge, Graph, Node, OperationDef, PortType, registry};
use std::collections::{HashMap, HashSet};

pub use oxy_render::input::text_input_active;

pub struct GraphView {
    pub configure_inputs: bool,
    pub guide_requested: Option<String>,
    pan: Vec2,
    zoom: f32,
    selected: Option<Id>,
    selected_edge: Option<Edge>,
    connecting: Option<(Id, String)>,
    search: String,
    pub error: String,
}

impl Default for GraphView {
    fn default() -> Self {
        Self {
            configure_inputs: false,
            guide_requested: None,
            pan: Vec2::new(35., 45.),
            zoom: 1.,
            selected: None,
            selected_edge: None,
            connecting: None,
            search: String::new(),
            error: String::new(),
        }
    }
}

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

fn operation_description(operation: &str) -> &'static str {
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
        _ => "Operação desconhecida. Verifique a compatibilidade do projeto.",
    }
}

fn port_label<'a>(
    definitions: &'a [OperationDef],
    graph: &Graph,
    node: &str,
    port: &str,
    output: bool,
) -> &'a str {
    graph
        .node(node)
        .and_then(|node| definitions.iter().find(|op| op.id == node.operation))
        .and_then(|op| {
            (if output { &op.outputs } else { &op.inputs })
                .iter()
                .find(|candidate| candidate.id == port)
        })
        .map_or("Porta ausente", |port| port.label)
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

impl GraphView {
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
            copy.id = new_id();
            copy.position[0] += 30.0;
            copy.position[1] += 30.0;
            self.selected = Some(copy.id.clone());
            graph.nodes.push(copy);
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Graph,
        objects: &[(Id, String)],
        project: &Project,
        scene: &Scene,
        trace: &[Id],
    ) {
        let definitions = registry();
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
        let owner = scene
            .entities
            .iter()
            .find(|entity| {
                entity
                    .graph
                    .nodes
                    .iter()
                    .any(|node| graph.node(&node.id).is_some())
            })
            .map(|entity| entity.id.as_str());
        ui.horizontal_wrapped(|ui| {
            ui.label("LÓGICA DO OBJETO");
            ui.separator();
            ui.label("Branco: execução · cores: dados");
            ui.add(egui::Slider::new(&mut self.zoom, 0.35..=1.6).text("Zoom"));
            if ui.button("Enquadrar").clicked() {
                let minx = graph.nodes.iter().map(|n| n.position[0]).fold(0., f32::min);
                let miny = graph.nodes.iter().map(|n| n.position[1]).fold(0., f32::min);
                self.pan = Vec2::new(30. - minx * self.zoom, 40. - miny * self.zoom);
            }
            if ui.button("Validar").clicked() {
                let ids = objects.iter().map(|(id, _)| id.clone()).collect();
                self.error = match graph.validate(&ids) {
                    Ok(()) => "Grafo válido".into(),
                    Err(e) => e,
                };
            }
            if ui
                .add_enabled(self.selected.is_some(), egui::Button::new("Duplicar nó"))
                .clicked()
            {
                self.duplicate_selected(graph);
            }
            if ui
                .add_enabled(
                    self.selected.is_some() || self.selected_edge.is_some(),
                    egui::Button::new("Excluir seleção"),
                )
                .clicked()
            {
                self.delete_selected(graph);
            }
        });
        if !self.error.is_empty() {
            ui.colored_label(Color32::from_rgb(225, 181, 113), &self.error);
        }
        ui.label("Arraste títulos e conecte portas. Botão central move a área. Clique na curva para selecionar; Delete exclui. Ctrl+D duplica o nó.");
        ui.separator();
        egui::SidePanel::left("catalogo")
            .resizable(true)
            .default_width(200.)
            .width_range(165.0..=300.0)
            .show_inside(ui, |ui| {
                ui.heading("Blocos");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Pesquisar ação…")
                        .desired_width(f32::INFINITY),
                );
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut category = "";
                    for op in definitions {
                        if !format!("{} {} {}", op.id, op.label, op.category)
                            .to_lowercase()
                            .contains(&self.search.to_lowercase())
                        {
                            continue;
                        }
                        if category != op.category {
                            ui.add_space(10.);
                            ui.strong(op.category);
                            category = op.category;
                        }
                        if ui
                            .button(op.label)
                            .on_hover_text(operation_description(op.id))
                            .clicked()
                        {
                            let offset = graph.nodes.len() as f32 % 5. * 25.;
                            let mut node = Node::new(
                                op.id,
                                [
                                    (-self.pan.x + 80.) / self.zoom + offset,
                                    (-self.pan.y + 80.) / self.zoom + offset,
                                ],
                            );
                            if op.id == "event.input" {
                                node.params.insert(
                                    "action".into(),
                                    oxy_core::document::Value::Text(String::new()),
                                );
                            }
                            self.selected = Some(node.id.clone());
                            self.selected_edge = None;
                            graph.nodes.push(node);
                        }
                    }
                });
            });
        egui::SidePanel::right("node_properties").resizable(true).default_width(235.).width_range(180.0..=350.0).show_inside(ui,|ui| {
            ui.heading("Nó");
            let mut delete=false;
            let mut duplicate=false;
            egui::ScrollArea::vertical().show(ui,|ui| {
                if let Some(node)=graph.nodes.iter_mut().find(|n|Some(&n.id)==self.selected.as_ref()) {
                    if let Some(op)=definitions.iter().find(|op|op.id==node.operation) {
                        ui.strong(op.label);ui.label(egui::RichText::new(operation_description(op.id)).small().weak());
                        if ui.button("Ajuda deste nó").clicked() {self.guide_requested=Some(op.id.into());}
                        if op.id=="event.input" && ui.button("Configurar ações de entrada").clicked() {self.configure_inputs=true;}
                        ui.separator();
                        for param in &op.params {
                            ui.label(match (node.operation.as_str(), param.id) {
                                ("action.animation", "clip") => "Animação",
                                ("action.sound", "asset") => "Áudio",
                                ("action.scene", "scene") => "Cena de destino",
                                ("action.component", "component") => "Componente",
                                ("math.binary", "operator") => "Operação",
                                ("condition.compare", "operator") => "Comparação",
                                _ => param.label,
                            });
                            let context = ParameterContext { objects, project, scene, owner,
                                target_connected: graph.edges.iter().any(|edge| edge.to_node == node.id && edge.to_port == "target"),
                                any_type: op.inputs.iter().any(|port| port.id == param.id && port.kind == PortType::Any) };
                            parameter_editor(ui, node, param, &context);ui.add_space(5.);
                        }
                        ui.separator();
                        ui.small("Parâmetros são valores padrão. Uma entrada conectada recebe o valor do outro nó.");
                    } else {ui.colored_label(Color32::LIGHT_RED,"Operação desconhecida");}
                    if ui.button("Duplicar nó").clicked() { duplicate=true; }
                    if ui.button("Excluir nó").clicked() { delete=true; }
                } else if let Some(edge) = &self.selected_edge {
                    ui.strong("Conexão selecionada");
                    let source = graph.node(&edge.from_node).and_then(|node| definitions.iter().find(|op| op.id == node.operation)).map_or("Nó ausente", |op| op.label);
                    let target = graph.node(&edge.to_node).and_then(|node| definitions.iter().find(|op| op.id == node.operation)).map_or("Nó ausente", |op| op.label);
                    let source_port = port_label(definitions, graph, &edge.from_node, &edge.from_port, true);
                    let target_port = port_label(definitions, graph, &edge.to_node, &edge.to_port, false);
                    ui.label(format!("{source} · {source_port} → {target} · {target_port}"));
                    if ui.button("Excluir conexão").clicked() { delete=true; }
                } else { ui.label("Selecione um nó para configurar seus parâmetros."); }
                ui.separator();
                if let Some((node,port))=&self.connecting {ui.label(format!("Conectando: {}", port_label(definitions, graph, node, port, true)));if ui.button("Cancelar conexão").clicked(){self.connecting=None;}}
            });
            if delete { self.delete_selected(graph); }
            if duplicate { self.duplicate_selected(graph); }
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(
                    ui.available_size().max(Vec2::new(1., 1.)),
                    Sense::click_and_drag(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0., Color32::from_rgb(20, 24, 31));
                if ui.input(|input| {
                    input.pointer.button_down(egui::PointerButton::Middle)
                        && input
                            .pointer
                            .hover_pos()
                            .is_some_and(|pointer| rect.contains(pointer))
                }) {
                    self.pan += ui.input(|i| i.pointer.delta());
                }
                if response.hovered() {
                    let wheel = ui.input(|i| i.smooth_scroll_delta.y);
                    if wheel != 0. {
                        let previous = self.zoom;
                        self.zoom = (self.zoom * (wheel * 0.001).exp()).clamp(0.35, 1.6);
                        if let Some(mouse) = ui.input(|input| input.pointer.hover_pos()) {
                            let relative = mouse - rect.min;
                            self.pan = relative - (relative - self.pan) * (self.zoom / previous);
                        }
                    }
                }
                let origin = rect.min + self.pan;
                let step = 24. * self.zoom;
                let startx = rect.min.x + (self.pan.x % step);
                let starty = rect.min.y + (self.pan.y % step);
                for x in 0..((rect.width() / step) as usize + 2) {
                    for y in 0..((rect.height() / step) as usize + 2) {
                        painter.circle_filled(
                            Pos2::new(startx + x as f32 * step, starty + y as f32 * step),
                            0.8,
                            Color32::from_gray(45),
                        );
                    }
                }
                let mut ports: HashMap<(Id, String, bool), (Pos2, PortType)> = HashMap::new();
                for node in &graph.nodes {
                    if let Some(op) = definitions.iter().find(|o| o.id == node.operation) {
                        let p = origin + Vec2::from(node.position) * self.zoom;
                        for (i, port) in op.inputs.iter().enumerate() {
                            ports.insert(
                                (node.id.clone(), port.id.into(), false),
                                (
                                    p + Vec2::new(0., 48. + i as f32 * 24.) * self.zoom,
                                    port.kind,
                                ),
                            );
                        }
                        for (i, port) in op.outputs.iter().enumerate() {
                            ports.insert(
                                (node.id.clone(), port.id.into(), true),
                                (
                                    p + Vec2::new(205., 48. + i as f32 * 24.) * self.zoom,
                                    port.kind,
                                ),
                            );
                        }
                    }
                }
                let mouse = ui.input(|input| input.pointer.hover_pos());
                let pointer_over_node = mouse.is_some_and(|mouse| {
                    graph.nodes.iter().any(|node| {
                        let Some(op) = definitions.iter().find(|op| op.id == node.operation) else {
                            return false;
                        };
                        let position = origin + Vec2::from(node.position) * self.zoom;
                        Rect::from_min_size(
                            position,
                            Vec2::new(
                                205.0,
                                66.0 + op.inputs.len().max(op.outputs.len()) as f32 * 24.0,
                            ) * self.zoom,
                        )
                        .expand(9.0)
                        .contains(mouse)
                    })
                });
                let mut selected_curve = None;
                for edge in &graph.edges {
                    if let (Some((a, kind)), Some((b, _))) = (
                        ports.get(&(edge.from_node.clone(), edge.from_port.clone(), true)),
                        ports.get(&(edge.to_node.clone(), edge.to_port.clone(), false)),
                    ) {
                        let bend = ((b.x - a.x).abs() * 0.45).max(40.);
                        let points = [*a, *a + Vec2::new(bend, 0.), *b - Vec2::new(bend, 0.), *b];
                        let hovered = !pointer_over_node
                            && mouse.is_some_and(|mouse| {
                                rect.contains(mouse) && curve_distance(points, mouse) < 7.0
                            });
                        let selected = self.selected_edge.as_ref() == Some(edge);
                        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                            points,
                            false,
                            Color32::TRANSPARENT,
                            Stroke::new(
                                if hovered || selected { 3.5 } else { 2.0 },
                                if selected {
                                    Color32::GOLD
                                } else {
                                    port_color(kind)
                                },
                            ),
                        ));
                        if hovered {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            if response.clicked() {
                                selected_curve = Some(edge.clone());
                            }
                        }
                    }
                }
                if let Some(edge) = selected_curve {
                    self.selected = None;
                    self.selected_edge = Some(edge);
                }
                if let Some((id, port)) = &self.connecting
                    && let (Some((a, _)), Some(mouse)) = (
                        ports.get(&(id.clone(), port.clone(), true)),
                        ui.input(|i| i.pointer.hover_pos()),
                    )
                {
                    painter.line_segment([*a, mouse], Stroke::new(2., Color32::WHITE));
                }
                let mut new_edge = None;
                for node in &mut graph.nodes {
                    let Some(op) = definitions.iter().find(|o| o.id == node.operation) else {
                        continue;
                    };
                    let p = origin + Vec2::from(node.position) * self.zoom;
                    let height = 66. + op.inputs.len().max(op.outputs.len()) as f32 * 24.;
                    let body = Rect::from_min_size(p, Vec2::new(205., height) * self.zoom);
                    let selected = self.selected.as_ref() == Some(&node.id);
                    let active = trace.contains(&node.id);
                    painter.rect_filled(body, 5., Color32::from_rgb(38, 44, 54));
                    painter.rect_stroke(
                        body,
                        5.,
                        Stroke::new(
                            if selected { 2. } else { 1. },
                            if active {
                                Color32::GOLD
                            } else if selected {
                                Color32::from_rgb(107, 203, 190)
                            } else {
                                Color32::from_gray(77)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let title = Rect::from_min_size(p, Vec2::new(205., 32.) * self.zoom);
                    painter.rect_filled(title, 4., Color32::from_rgb(47, 58, 69));
                    painter.text(
                        title.left_center() + Vec2::new(9., 0.),
                        egui::Align2::LEFT_CENTER,
                        op.label,
                        egui::FontId::proportional(13. * self.zoom),
                        Color32::WHITE,
                    );
                    let response =
                        ui.interact(title, ui.id().with(&node.id), Sense::click_and_drag());
                    response.context_menu(|ui| {
                        if ui.button("Ajuda deste nó").clicked() {
                            self.guide_requested = Some(op.id.into());
                            ui.close();
                        }
                    });
                    if response.clicked() || response.drag_started_by(egui::PointerButton::Primary)
                    {
                        self.selected = Some(node.id.clone());
                        self.selected_edge = None;
                    }
                    if response.dragged_by(egui::PointerButton::Primary) {
                        let d = ui.input(|i| i.pointer.delta()) / self.zoom;
                        node.position[0] += d.x;
                        node.position[1] += d.y;
                    }
                    for (output, list) in [(false, &op.inputs), (true, &op.outputs)] {
                        for port in list {
                            let Some((pos, _)) =
                                ports.get(&(node.id.clone(), port.id.into(), output))
                            else {
                                continue;
                            };
                            painter.circle_filled(*pos, 5. * self.zoom, port_color(&port.kind));
                            painter.text(
                                *pos + Vec2::new(if output { -10. } else { 10. }, 0.) * self.zoom,
                                if output {
                                    egui::Align2::RIGHT_CENTER
                                } else {
                                    egui::Align2::LEFT_CENTER
                                },
                                port.label,
                                egui::FontId::proportional(11. * self.zoom),
                                Color32::LIGHT_GRAY,
                            );
                            let resp = ui
                                .interact(
                                    Rect::from_center_size(*pos, Vec2::splat(18. * self.zoom)),
                                    ui.id().with((&node.id, port.id, output)),
                                    Sense::click_and_drag(),
                                )
                                .on_hover_text(format!(
                                    "{} · {} de {}",
                                    port.label,
                                    if output { "Saída" } else { "Entrada" },
                                    port.kind.label().to_lowercase()
                                ));
                            if output && (resp.clicked() || resp.drag_started()) {
                                self.connecting = Some((node.id.clone(), port.id.into()));
                            }
                            let released = ui
                                .input(|i| i.pointer.button_released(egui::PointerButton::Primary))
                                && resp.contains_pointer();
                            if !output
                                && (resp.clicked() || released)
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
                if let Some(edge) = new_edge {
                    let ids: HashSet<_> = objects.iter().map(|(id, _)| id.clone()).collect();
                    if let Err(e) = graph.connect(&edge, &ids) {
                        self.error = e;
                    } else {
                        self.error.clear();
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::{
        animation::Clip,
        document::{Asset, AssetKind, Entity, SceneKind, Value},
        graph::ParamDef,
    };

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
            let ctx = egui::Context::default();
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
        let ctx = egui::Context::default();
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
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1000.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        view.show(ui, &mut graph, &[], &project, &project.scenes[0], &[]);
                    });
                },
            );
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
            let ctx = egui::Context::default();
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
                            view.show(ui, &mut graph, &[], &project, &project.scenes[0], &[]);
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
