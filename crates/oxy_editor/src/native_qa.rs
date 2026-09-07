//! Opt-in native integration test. Inputs enter only this eframe application's RawInput.
//! It never sends OS keyboard/mouse input and does not require foreground ownership.
use crate::app::{Editor, Snapshot, Tab};
use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, Rect, Vec2};
use oxy_core::document::{Id, SceneKind, new_id};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
enum Action {
    Click(&'static str),
    SelectEntity(&'static str),
    OptionalClick(&'static str),
    Text(&'static str),
    Key(Key, bool),
    Hold(Key, usize),
    Wait(usize),
    Card,
    Resize(Vec2),
    Screenshot(&'static str),
    Check(&'static str),
    DragNode(&'static str),
    ConnectPorts,
    Paint,
    GizmoX,
}

#[derive(Default)]
struct Report {
    done: bool,
    error: Option<String>,
    steps: Vec<String>,
    screenshots: Vec<String>,
}
struct TextTarget {
    text: String,
    rect: Rect,
}
#[derive(Default)]
struct Surface {
    texts: Vec<TextTarget>,
    canvas: Option<Rect>,
    png: Option<Rect>,
    ports: Vec<Pos2>,
    gizmo_x: Option<Pos2>,
}

struct NativeQa {
    editor: Editor,
    actions: VecDeque<Action>,
    events: VecDeque<Vec<Event>>,
    surface: Surface,
    report: Arc<Mutex<Report>>,
    output: PathBuf,
    start: Instant,
    wait: usize,
    missing: usize,
    pending_shot: Option<String>,
    base: Option<Snapshot>,
    before_paint: Option<Snapshot>,
    after_paint: Option<Snapshot>,
    created: Option<Id>,
    initial_entities: usize,
    finished: bool,
}

impl NativeQa {
    fn new(
        cc: &eframe::CreationContext<'_>,
        project: PathBuf,
        output: PathBuf,
        report: Arc<Mutex<Report>>,
    ) -> Self {
        let mut editor = Editor::new(cc);
        editor.open(project);
        let initial_entities = editor.scene().entities.len();
        let actions = VecDeque::from([
            Action::Screenshot("editor-scene.png"),
            Action::Click("+ Objeto"),
            Action::Click("Retângulo"),
            Action::Check("created"),
            Action::Check("gizmo_baseline"),
            Action::GizmoX,
            Action::Check("gizmo_moved"),
            Action::Key(Key::Z, true),
            Action::Check("gizmo_undo"),
            Action::Click("Lógica"),
            Action::Click("Ao iniciar cena"),
            Action::Click("Pesquisar ação…"),
            Action::Text("Mensagem"),
            Action::Click("Mensagem de diagnóstico"),
            Action::DragNode("Mensagem de diagnóstico"),
            Action::ConnectPorts,
            Action::Check("graph_connected"),
            Action::Screenshot("graph.png"),
            Action::Key(Key::Delete, false),
            Action::Check("node_deleted"),
            Action::Key(Key::Z, true),
            Action::Check("node_undo"),
            Action::Click("Cena"),
            Action::Check("play_baseline"),
            Action::Click("▶ Jogar"),
            Action::Check("playing"),
            Action::Hold(Key::D, 24),
            Action::Check("game_moved"),
            Action::Screenshot("game.png"),
            Action::Click("Cena"),
            Action::Check("left_game_paused"),
            Action::Click("Jogo"),
            Action::Check("return_still_paused"),
            Action::Click("▶ Retomar"),
            Action::Check("playing"),
            Action::Key(Key::Escape, false),
            Action::Check("escape_paused"),
            Action::Click("■ Parar"),
            Action::Check("stop_isolated"),
            Action::Resize(Vec2::new(920., 600.)),
            Action::Screenshot("editor-small.png"),
            Action::Resize(Vec2::new(1440., 900.)),
            Action::Click("A · Sala de plataforma 2D"),
            Action::Click("B · Oficina 3D e golpe articulado"),
            Action::Check("scene_3d"),
            Action::SelectEntity("Tronco · textura pintável"),
            Action::Click("Estúdio"),
            Action::Click("Pintura"),
            Action::Click("Enquadrar peça"),
            Action::OptionalClick("Criar cópia independente"),
            Action::Check("paint_baseline"),
            Action::Paint,
            Action::Check("paint_changed"),
            Action::Screenshot("studio-paint.png"),
            Action::Key(Key::Z, true),
            Action::Check("paint_undo"),
            Action::Key(Key::Y, true),
            Action::Check("paint_redo"),
            Action::Key(Key::S, true),
            Action::Check("saved_roundtrip"),
            Action::Click("Modelagem"),
            Action::Screenshot("studio-3d.png"),
            Action::SelectEntity("Boneco · modelo por peças"),
            Action::Click("Animação"),
            Action::Check("animation_baseline"),
            Action::Click("▶ Prévia"),
            Action::Wait(16),
            Action::Check("animation_preview"),
            Action::Screenshot("animation.png"),
            Action::Click("Pose-base"),
            Action::Click("Keyframe da peça"),
            Action::Check("key_created"),
            Action::Click("Copiar keyframe"),
            Action::SelectEntity("Cabeça"),
            Action::Click("Colar keyframe"),
            Action::Check("key_pasted"),
            Action::Key(Key::S, true),
            Action::Check("saved_roundtrip"),
            Action::Click("Cena"),
            Action::Click("B · Oficina 3D e golpe articulado"),
            Action::Click("C · Carta, custo, energia e alvo"),
            Action::Check("play_baseline"),
            Action::Click("▶ Jogar"),
            Action::Check("playing"),
            Action::Card,
            Action::Check("card_one"),
            Action::Card,
            Action::Check("card_two"),
            Action::Card,
            Action::Check("card_three"),
            Action::Card,
            Action::Check("card_four"),
            Action::Screenshot("card.png"),
            Action::Click("■ Parar"),
            Action::Check("stop_isolated"),
        ]);
        Self {
            editor,
            actions,
            events: VecDeque::new(),
            surface: Surface::default(),
            report,
            output,
            start: Instant::now(),
            wait: 15,
            missing: 0,
            pending_shot: None,
            base: None,
            before_paint: None,
            after_paint: None,
            created: None,
            initial_entities,
            finished: false,
        }
    }

    fn fail(&mut self, ctx: &egui::Context, message: String) {
        let labels = self
            .surface
            .texts
            .iter()
            .map(|text| format!("{:?}: {}", text.rect, text.text))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = std::fs::write(self.output.join("native-qa-labels.txt"), labels);
        self.report.lock().unwrap().error = Some(message);
        self.finished = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn find(&self, label: &str, canvas: bool) -> Option<Pos2> {
        self.surface
            .texts
            .iter()
            .filter(|target| target.text == label || target.text.ends_with(&format!(" {label}")))
            .filter(|target| {
                !canvas
                    || self
                        .surface
                        .canvas
                        .is_some_and(|canvas| canvas.contains(target.rect.center()))
            })
            .min_by(|a, b| a.rect.top().total_cmp(&b.rect.top()))
            .map(|target| target.rect.center())
    }

    fn click(&mut self, position: Pos2) {
        self.events.push_back(vec![Event::PointerMoved(position)]);
        self.events.push_back(vec![Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        self.events.push_back(vec![Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        self.events.push_back(vec![]);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.events.push_back(vec![Event::PointerMoved(from)]);
        self.events.push_back(vec![Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        for index in 1..=6 {
            self.events
                .push_back(vec![Event::PointerMoved(from.lerp(to, index as f32 / 6.))]);
        }
        self.events.push_back(vec![Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
    }

    fn check(&mut self, label: &str) -> Result<(), String> {
        let ensure = |condition: bool, message: &str| {
            if condition {
                Ok(())
            } else {
                Err(format!("{label}: {message}"))
            }
        };
        match label {
            "created" => {
                ensure(
                    self.editor.scene().entities.len() == self.initial_entities + 1,
                    "+ Objeto não criou uma entidade",
                )?;
                self.created = self.editor.selected.clone();
                ensure(self.created.is_some(), "Objeto criado não selecionado")
            }
            "graph_connected" | "node_undo" => {
                let entity = self
                    .editor
                    .scene()
                    .entity(self.created.as_deref().ok_or("Objeto de QA ausente")?)
                    .ok_or("Objeto removido")?;
                ensure(
                    entity.graph.nodes.len() == 2 && entity.graph.edges.len() == 1,
                    "Conexão normal de portas deve produzir 2 nós / 1 aresta",
                )
            }
            "node_deleted" => {
                let entity = self
                    .editor
                    .scene()
                    .entity(self.created.as_deref().ok_or("Objeto de QA ausente")?)
                    .ok_or("Delete excluiu o objeto inteiro")?;
                ensure(
                    entity.graph.nodes.len() == 1 && entity.graph.edges.is_empty(),
                    "Delete deve excluir só o nó selecionado",
                )
            }
            "play_baseline" | "gizmo_baseline" | "animation_baseline" => {
                self.base = Some(self.editor.state.clone());
                Ok(())
            }
            "animation_preview" => ensure(
                self.base.as_ref() == Some(&self.editor.state),
                "Prévia do clip existente não pode escrever a pose-base ou alterar o projeto",
            ),
            "key_created" | "key_pasted" => {
                let owner = self
                    .editor
                    .studio
                    .owner
                    .as_deref()
                    .ok_or("Proprietário de clip ausente")?;
                let current = self
                    .editor
                    .scene()
                    .entity(owner)
                    .ok_or("Objeto animado ausente")?;
                let previous = self
                    .base
                    .as_ref()
                    .and_then(|base| base.project.scene(&self.editor.scene_id))
                    .and_then(|scene| scene.entity(owner))
                    .ok_or("Baseline de animação ausente")?;
                let count = |entity: &oxy_core::document::Entity| {
                    entity
                        .clips
                        .iter()
                        .flat_map(|clip| &clip.tracks)
                        .map(|track| track.keyframes.len())
                        .sum::<usize>()
                };
                let added = if label == "key_created" { 1 } else { 2 };
                ensure(
                    count(current) >= count(previous) + added,
                    "Inserir/copiar/colar keyframes deve alterar dados normais do clip",
                )
            }
            "card_one" | "card_two" | "card_three" | "card_four" => {
                let scene = self
                    .editor
                    .runtime
                    .as_ref()
                    .ok_or("Runtime de carta ausente")?
                    .scene();
                let energy = scene.entities.iter().find_map(|entity| {
                    entity
                        .attributes
                        .get("Energia")
                        .and_then(oxy_core::document::Value::number)
                });
                let health = scene.entities.iter().find_map(|entity| {
                    entity
                        .attributes
                        .get("Vida")
                        .and_then(oxy_core::document::Value::number)
                });
                let expected = match label {
                    "card_one" => (2., 75.),
                    "card_two" => (1., 50.),
                    _ => (0., 25.),
                };
                ensure(
                    energy == Some(expected.0) && health == Some(expected.1),
                    &format!(
                        "Clique da carta: energia/vida {energy:?}/{health:?}, esperado {expected:?}"
                    ),
                )
            }
            "gizmo_moved" => {
                let entity = self
                    .editor
                    .scene()
                    .entity(self.created.as_deref().ok_or("Objeto de QA ausente")?)
                    .ok_or("Objeto removido")?;
                ensure(
                    entity.transform.position[0] > 0.1,
                    "Arrasto visual do eixo X deve mover o objeto",
                )
            }
            "gizmo_undo" => ensure(
                self.base.as_ref() == Some(&self.editor.state),
                "Undo deve restaurar o arrasto inteiro de transformação",
            ),
            "playing" => ensure(
                self.editor.tab == Tab::Game
                    && self.editor.capture
                    && self
                        .editor
                        .runtime
                        .as_ref()
                        .is_some_and(|runtime| !runtime.paused),
                "Jogo deveria estar executando e capturando entrada",
            ),
            "game_moved" => {
                let runtime = self.editor.runtime.as_ref().ok_or("Runtime não iniciou")?;
                let original = self
                    .base
                    .as_ref()
                    .and_then(|base| base.project.scene(&self.editor.scene_id))
                    .and_then(|scene| {
                        scene.entities.iter().find(|entity| {
                            entity
                                .controller
                                .as_ref()
                                .is_some_and(|controller| controller.enabled)
                        })
                    })
                    .ok_or("Controlador da cena de validação ausente")?;
                let played = runtime
                    .scene()
                    .entity(&original.id)
                    .ok_or("Controlador removido")?;
                ensure(
                    played.transform.position[0] > original.transform.position[0] + 0.01,
                    "Entrada D remapeada deve mover o controlador no runtime",
                )
            }
            "left_game_paused" | "return_still_paused" | "escape_paused" => ensure(
                !self.editor.capture
                    && self
                        .editor
                        .runtime
                        .as_ref()
                        .is_some_and(|runtime| runtime.paused),
                "Jogo deveria pausar e liberar entrada",
            ),
            "stop_isolated" => ensure(
                self.editor.runtime.is_none() && self.base.as_ref() == Some(&self.editor.state),
                "Parar deve descartar runtime e preservar documento exato",
            ),
            "scene_3d" => ensure(
                self.editor.scene().kind == SceneKind::ThreeD,
                "Seletor de cena não abriu 3D",
            ),
            "paint_baseline" => {
                self.before_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "paint_changed" => {
                ensure(
                    self.before_paint
                        .as_ref()
                        .is_some_and(|before| before.images != self.editor.state.images),
                    "Pincelada não mudou pixels reais",
                )?;
                self.after_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "paint_undo" => ensure(
                self.before_paint.as_ref() == Some(&self.editor.state),
                "Undo deve desfazer o gesto inteiro",
            ),
            "paint_redo" => ensure(
                self.after_paint.as_ref() == Some(&self.editor.state),
                "Redo deve recuperar exatamente os pixels",
            ),
            "saved_roundtrip" => {
                let path = self
                    .editor
                    .path
                    .as_ref()
                    .ok_or("Arquivo de projeto ausente")?;
                let project = oxy_core::persistence::load_project(path)?;
                ensure(
                    project == self.editor.state.project,
                    "Salvar deve preservar os dados editados",
                )?;
                for (id, image) in &self.editor.state.images {
                    let asset = project.asset(id).ok_or("Textura sem referência estável")?;
                    let read = oxy_core::painting::PaintImage::load(
                        &path.parent().unwrap().join(&asset.path),
                    )?;
                    ensure(&read == image, "PNG reaberto diverge dos pixels em memória")?;
                }
                Ok(())
            }
            _ => Err(format!("Verificação desconhecida: {label}")),
        }
    }

    fn step(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        let Some(action) = self.actions.front().cloned() else {
            return Ok(true);
        };
        let description;
        match action {
            Action::SelectEntity(label) => {
                let position = self
                    .surface
                    .texts
                    .iter()
                    .find(|target| {
                        target.text.ends_with(&format!(" {label}"))
                            && target.text.starts_with(['▾', '◇', '▤', '◉'])
                    })
                    .map(|target| target.rect.center())
                    .ok_or_else(|| format!("Objeto da hierarquia não encontrado: {label}"))?;
                self.click(position);
                description = format!("Selecionar hierarquia: {label}");
            }
            Action::Click(label) | Action::OptionalClick(label) => {
                let Some(position) = self.find(label, false) else {
                    if matches!(action, Action::OptionalClick(_)) {
                        self.actions.pop_front();
                        return Ok(false);
                    }
                    self.missing += 1;
                    if self.missing < 70 {
                        return Ok(false);
                    }
                    return Err(format!("Controle visível não encontrado: {label}"));
                };
                self.click(position);
                description = format!("Clique: {label}");
            }
            Action::Text(text) => {
                self.events.push_back(vec![Event::Text(text.into())]);
                description = format!("Digitar: {text}");
            }
            Action::Key(key, control) => {
                let modifiers = Modifiers {
                    ctrl: control,
                    command: control,
                    ..Modifiers::NONE
                };
                for pressed in [true, false] {
                    self.events.push_back(vec![Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers,
                    }]);
                }
                description = format!("Tecla {key:?}, Ctrl={control}");
            }
            Action::Hold(key, frames) => {
                self.events.push_back(vec![Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }]);
                for _ in 0..frames {
                    self.events.push_back(vec![]);
                }
                self.events.push_back(vec![Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }]);
                description = format!("Manter {key:?} por {frames} quadros");
            }
            Action::Wait(frames) => {
                self.wait = frames;
                description = format!("Reproduzir {frames} quadros sem entrada");
            }
            Action::Card => {
                let position = self
                    .surface
                    .texts
                    .iter()
                    .find(|text| text.text.contains("Clique para jogar"))
                    .map(|text| text.rect.center())
                    .ok_or("Texto do botão da carta não encontrado")?;
                self.click(position);
                description = "Clique real no botão da carta".into();
            }
            Action::Resize(size) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                self.wait = 20;
                description = format!("Redimensionar: {size:?}");
            }
            Action::Screenshot(name) => {
                self.pending_shot = Some(name.into());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    name.to_owned(),
                )));
                description = format!("Captura GPU da janela: {name}");
            }
            Action::Check(label) => {
                self.check(label)?;
                description = format!("Verificado: {label}");
            }
            Action::DragNode(label) => {
                let from = self
                    .find(label, true)
                    .ok_or("Título do nó não está visível no canvas")?;
                self.drag(from, from + Vec2::new(290., 120.));
                description = "Arrastar nó pelo título".into();
            }
            Action::ConnectPorts => {
                let mut ports = self.surface.ports.clone();
                ports.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
                if ports.len() != 3 {
                    return Err(format!(
                        "Esperadas 3 portas de execução visíveis; encontradas {}",
                        ports.len()
                    ));
                }
                self.click(ports[0]);
                self.click(ports[1]);
                description = "Conectar saída e entrada pelas portas visuais".into();
            }
            Action::Paint => {
                let rect = self
                    .surface
                    .png
                    .ok_or("Prévia PNG pintável não encontrada")?;
                self.drag(rect.min + rect.size() * 0.33, rect.min + rect.size() * 0.63);
                description = "Pincelada real no PNG".into();
            }
            Action::GizmoX => {
                let from = self
                    .surface
                    .gizmo_x
                    .ok_or("Controle visual vermelho do eixo X não encontrado")?;
                self.drag(from, from + Vec2::new(75., 0.));
                description = "Arrastar eixo X pelo controle visual".into();
            }
        }
        self.report.lock().unwrap().steps.push(description);
        self.actions.pop_front();
        self.missing = 0;
        self.wait = self.wait.max(5);
        Ok(false)
    }
}

impl eframe::App for NativeQa {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        // Preserve native screenshot responses; discard external input so other apps are untouched.
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        let scripted = self.events.pop_front().unwrap_or_default();
        input.modifiers = scripted
            .iter()
            .find_map(|event| {
                if let Event::Key { modifiers, .. } = event {
                    Some(*modifiers)
                } else {
                    None
                }
            })
            .unwrap_or_default();
        input.events.extend(scripted);
        input.focused = true;
        input.hovered_files.clear();
        input.dropped_files.clear();
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.focused = Some(true);
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.start.elapsed() > Duration::from_secs(100) {
            self.fail(ctx, "QA nativo excedeu 100 segundos".into());
            return;
        }
        let screenshots = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let Event::Screenshot {
                        user_data, image, ..
                    } = event
                    {
                        user_data
                            .data
                            .as_ref()
                            .and_then(|data| data.downcast_ref::<String>())
                            .map(|name| (name.clone(), image.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for (name, pixels) in screenshots {
            let rgba: Vec<u8> = pixels
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            let path = self.output.join(&name);
            if let Err(error) = image::save_buffer(
                &path,
                &rgba,
                pixels.width() as u32,
                pixels.height() as u32,
                image::ColorType::Rgba8,
            ) {
                self.fail(ctx, error.to_string());
                return;
            }
            self.report.lock().unwrap().screenshots.push(name);
            self.pending_shot = None;
        }
        self.editor.update(ctx, frame);
        self.surface = capture_surface(ctx);
        if !self.events.is_empty() || self.pending_shot.is_some() {
            ctx.request_repaint();
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            ctx.request_repaint();
            return;
        }
        match self.step(ctx) {
            Ok(true) => {
                let mut report = self.report.lock().unwrap();
                report.done = true;
                self.finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Ok(false) => {}
            Err(error) => self.fail(ctx, error),
        }
        ctx.request_repaint();
    }
}

fn capture_surface(ctx: &egui::Context) -> Surface {
    let shapes = ctx.graphics(|graphics| graphics.clone().drain(&[], &Default::default()));
    let mut surface = Surface::default();
    fn inspect(shape: &egui::Shape, clip: Rect, surface: &mut Surface) {
        match shape {
            egui::Shape::Text(text) => {
                let rect = text
                    .galley
                    .rect
                    .translate(text.pos.to_vec2())
                    .intersect(clip);
                if rect.is_positive() {
                    surface.texts.push(TextTarget {
                        text: text.galley.text().to_owned(),
                        rect,
                    });
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    inspect(shape, clip, surface);
                }
            }
            egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(20, 24, 31) => {
                surface.canvas = Some(rect.rect.intersect(clip))
            }
            egui::Shape::Circle(circle)
                if circle.fill == Color32::WHITE && circle.radius > 2. && circle.radius < 10. =>
            {
                surface.ports.push(circle.center)
            }
            egui::Shape::Circle(circle)
                if circle.fill == Color32::from_rgb(239, 113, 117)
                    && (circle.radius - 6.).abs() < 0.1 =>
            {
                surface.gizmo_x = Some(circle.center)
            }
            egui::Shape::Mesh(mesh) if matches!(mesh.texture_id, egui::TextureId::Managed(_)) => {
                let rect = mesh.calc_bounds().intersect(clip);
                if rect.width() > 100.
                    && rect.height() > 100.
                    && surface.png.is_none_or(|old| rect.area() > old.area())
                {
                    surface.png = Some(rect);
                }
            }
            _ => {}
        }
    }
    for shape in shapes {
        inspect(&shape.shape, shape.clip_rect, &mut surface);
    }
    if let Some(canvas) = surface.canvas {
        surface.ports.retain(|point| canvas.contains(*point));
    }
    surface
}

fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "Opens an inactive native WGPU window; injects input only into this app"]
#[cfg(target_os = "windows")]
fn native_editor_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = std::env::temp_dir().join(format!("oxy-native-qa-{}", new_id()));
    copy_directory(&workspace.join("examples/validacao"), &fixture).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA nativo isolado")
            .with_inner_size([1440., 900.])
            .with_min_inner_size([920., 600.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA nativo isolado",
        options,
        Box::new(move |cc| {
            Ok(Box::new(NativeQa::new(
                cc,
                fixture.join("project.oxy.json"),
                artifacts,
                result,
            )))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\n\n{}\n\nCapturas: {:?}\n",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-qa.txt"), &summary).unwrap();
    println!("{summary}");
    assert!(
        report.done && report.error.is_none(),
        "{}",
        report
            .error
            .as_deref()
            .unwrap_or("Janela fechada antes do fim")
    );
    assert_eq!(report.screenshots.len(), 8);
}
