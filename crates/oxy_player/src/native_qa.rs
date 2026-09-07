//! Opt-in GPU test of the real standalone host. No OS input or foreground request.
use crate::Player;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use oxy_core::{
    document::{Id, UiKind, new_id},
    painting::PaintImage,
    persistence,
};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Report {
    done: bool,
    error: Option<String>,
    screenshot: bool,
    steps: Vec<String>,
}
struct NativePlayerQa {
    player: Player,
    report: Arc<Mutex<Report>>,
    fixture: PathBuf,
    output: PathBuf,
    started: Instant,
    events: VecDeque<Vec<Event>>,
    labels: Vec<(String, Rect)>,
    step: u8,
    wait: usize,
    controller: Option<Id>,
    baseline_x: f32,
    paused_time: f64,
    shot_pending: bool,
    finished: bool,
}
impl NativePlayerQa {
    fn new(
        cc: &eframe::CreationContext<'_>,
        fixture: PathBuf,
        output: PathBuf,
        report: Arc<Mutex<Report>>,
    ) -> Self {
        Self {
            player: Player::new(cc, fixture.join(persistence::PROJECT_FILE)),
            report,
            fixture,
            output,
            started: Instant::now(),
            events: VecDeque::new(),
            labels: Vec::new(),
            step: 0,
            wait: 12,
            controller: None,
            baseline_x: 0.,
            paused_time: 0.,
            shot_pending: false,
            finished: false,
        }
    }
    fn fail(&mut self, ctx: &egui::Context, error: String) {
        self.report.lock().unwrap().error = Some(error);
        self.finished = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn key(&mut self, key: Key, frames: usize) {
        self.events.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        self.events.extend((0..frames).map(|_| Vec::new()));
        self.events.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    fn click(&mut self, position: Pos2) {
        self.events.push_back(vec![Event::PointerMoved(position)]);
        for pressed in [true, false] {
            self.events.push_back(vec![Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }
        self.events.push_back(Vec::new());
    }
    fn controller_x(&self) -> Result<f32, String> {
        self.player
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.scene().entity(self.controller.as_deref()?))
            .map(|entity| entity.transform.position[0])
            .ok_or_else(|| "Controlador do projeto ausente".into())
    }
    fn advance_step(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        let description = match self.step {
            0 => {
                if let Some(error) = &self.player.error {
                    return Err(error.clone());
                }
                let runtime = self.player.runtime.as_ref().ok_or("Runtime não iniciou")?;
                if self.player.root != self.fixture
                    || self.player.path != self.fixture.join(persistence::PROJECT_FILE)
                {
                    return Err("Player não está usando exclusivamente a cópia portátil".into());
                }
                for asset in &runtime.project.assets {
                    if !asset.path.is_empty() {
                        let path = persistence::resolve_asset_path(&self.fixture, &asset.path)?;
                        if !path.is_file() {
                            return Err("Asset portátil não foi copiado".into());
                        }
                    }
                }
                if !oxy_render::CameraState::for_game(runtime.scene())
                    .matrix([1280, 720])
                    .is_finite()
                {
                    return Err("Câmera produziu projeção inválida".into());
                }
                let static_text = runtime
                    .scene()
                    .entities
                    .iter()
                    .filter_map(|entity| entity.ui.as_ref())
                    .find(|ui| {
                        ui.kind == UiKind::Text
                            && ui.binding_attribute.is_empty()
                            && !ui.text.is_empty()
                    })
                    .ok_or("Projeto não contém texto de interface")?;
                if !self
                    .labels
                    .iter()
                    .any(|(text, _)| text == &static_text.text)
                {
                    return Err("GameUi não desenhou o texto do documento".into());
                }
                self.controller = runtime
                    .scene()
                    .entities
                    .iter()
                    .find(|entity| entity.controller.is_some())
                    .map(|entity| entity.id.clone());
                self.baseline_x = self.controller_x()?;
                self.key(Key::D, 18);
                "Projeto e assets carregados da pasta temporária; câmera e interface presentes"
            }
            1 => {
                if self.controller_x()? <= self.baseline_x + 0.05 {
                    return Err("Ação D não movimentou o personagem".into());
                }
                self.key(Key::Escape, 0);
                "Movimento por entrada nativa isolada confirmado"
            }
            2 => {
                let runtime = self.player.runtime.as_ref().ok_or("Runtime ausente")?;
                if !runtime.paused {
                    return Err("Escape não pausou o player".into());
                }
                self.paused_time = runtime.time;
                self.wait = 8;
                "Escape pausou e liberou a entrada"
            }
            3 => {
                let runtime = self.player.runtime.as_ref().ok_or("Runtime ausente")?;
                if runtime.time != self.paused_time {
                    return Err("Simulação avançou enquanto pausada".into());
                }
                let button = self
                    .labels
                    .iter()
                    .find(|(text, _)| text == "Retomar")
                    .map(|(_, rect)| rect.center())
                    .ok_or("Botão real Retomar não foi desenhado")?;
                self.click(button);
                "Clique no Retomar localizado pelo texto efetivamente desenhado"
            }
            4 => {
                if self
                    .player
                    .runtime
                    .as_ref()
                    .is_none_or(|runtime| runtime.paused)
                {
                    return Err("Clique em Retomar não retomou".into());
                }
                self.baseline_x = self.controller_x()?;
                self.key(Key::D, 18);
                "Retomar reativou a simulação"
            }
            5 => {
                if self.controller_x()? <= self.baseline_x + 0.05 {
                    return Err("Botão focado bloqueou controles após Retomar".into());
                }
                self.shot_pending = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    "player.png".to_owned(),
                )));
                "Movimento após Retomar confirmado; captura GPU solicitada"
            }
            6 => {
                if !self.report.lock().unwrap().screenshot {
                    return Err("Captura GPU não foi recebida".into());
                }
                if self.player.error.is_some() || !self.player.diagnostics.is_empty() {
                    return Err(format!(
                        "Player reportou erros: {:?} {:?}",
                        self.player.error, self.player.diagnostics
                    ));
                }
                if self.player.runtime.as_ref().is_some_and(|runtime| {
                    runtime
                        .logs
                        .iter()
                        .any(|line| line.contains("Comportamento interrompido"))
                }) {
                    return Err("Runtime interrompeu um comportamento".into());
                }
                self.report.lock().unwrap().done = true;
                return Ok(true);
            }
            _ => return Err("Passo de QA desconhecido".into()),
        };
        self.report.lock().unwrap().steps.push(description.into());
        self.step += 1;
        self.wait = self.wait.max(5);
        Ok(false)
    }
}
impl eframe::App for NativePlayerQa {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        input
            .events
            .extend(self.events.pop_front().unwrap_or_default());
        input.modifiers = Modifiers::NONE;
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
        if self.started.elapsed() > Duration::from_secs(45) {
            self.fail(ctx, "QA do player excedeu 45 segundos".into());
            return;
        }
        let screenshots = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for screenshot in screenshots {
            let colors: HashSet<_> = screenshot
                .pixels
                .iter()
                .map(|pixel| pixel.to_array())
                .collect();
            if screenshot.width() < 640 || screenshot.height() < 360 || colors.len() < 20 {
                self.fail(ctx, "Captura GPU vazia ou sem conteúdo suficiente".into());
                return;
            }
            let image = PaintImage {
                width: screenshot.width() as u32,
                height: screenshot.height() as u32,
                pixels: screenshot
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect(),
            };
            if let Err(error) = image.save(&self.output.join("player.png")) {
                self.fail(ctx, error);
                return;
            }
            self.report.lock().unwrap().screenshot = true;
            self.shot_pending = false;
        }
        self.player.update(ctx, frame);
        self.labels = capture_labels(ctx);
        if !self.events.is_empty() || self.shot_pending {
            ctx.request_repaint();
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            ctx.request_repaint();
            return;
        }
        match self.advance_step(ctx) {
            Ok(true) => {
                self.finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Ok(false) => {}
            Err(error) => self.fail(ctx, error),
        }
        ctx.request_repaint();
    }
}
fn capture_labels(ctx: &egui::Context) -> Vec<(String, Rect)> {
    let shapes = ctx.graphics(|graphics| graphics.clone().drain(&[], &Default::default()));
    fn inspect(shape: &egui::Shape, clip: Rect, output: &mut Vec<(String, Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                let rect = text
                    .galley
                    .rect
                    .translate(text.pos.to_vec2())
                    .intersect(clip);
                if rect.is_positive() {
                    output.push((text.galley.text().to_owned(), rect));
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    inspect(shape, clip, output);
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for shape in shapes {
        inspect(&shape.shape, shape.clip_rect, &mut result);
    }
    result
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
#[ignore = "Opens an inactive native WGPU player; inputs stay inside its RawInput"]
fn native_player_portable_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.1.1");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = std::env::temp_dir().join(format!("oxy-player-native-{}", new_id()));
    copy_directory(&workspace.join("examples/validacao"), &fixture).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Player — QA nativo isolado")
            .with_inner_size([1280., 720.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Player — QA nativo isolado",
        options,
        Box::new(move |cc| {
            Ok(Box::new(NativePlayerQa::new(
                cc, fixture, artifacts, result,
            )))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\nCaptura GPU: {}\n{}\n",
        report.done,
        report.error,
        report.screenshot,
        report.steps.join("\n")
    );
    std::fs::write(output.join("player-native-qa.txt"), &summary).unwrap();
    assert!(report.error.is_none(), "{summary}");
    assert!(report.done && report.screenshot, "{summary}");
}
