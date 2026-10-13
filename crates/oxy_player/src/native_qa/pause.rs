use super::*;
// The editor's pause QA plays the same game.
#[path = "../../../oxy_core/examples/support/pause_menu.rs"]
mod pause_menu;

struct PausePlayerQa {
    player: Player,
    report: Arc<Mutex<Report>>,
    output: PathBuf,
    started: Instant,
    events: VecDeque<Vec<Event>>,
    labels: Vec<(String, Rect)>,
    phase: u8,
    wait: usize,
    /// The window is out of focus (Alt+Tab) while set.
    unfocused: bool,
    /// Wall time the next step waits for: egui windows fade in over real time, not frames.
    not_before: Option<Instant>,
    shots: Vec<String>,
    pending_shot: bool,
    finished: bool,
}
impl PausePlayerQa {
    fn tap(&mut self, key: Key) {
        for pressed in [true, false] {
            self.events.push_back(vec![Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }
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
    fn label(&self, text: &str) -> Option<Rect> {
        self.labels
            .iter()
            .find(|(label, _)| label == text)
            .map(|(_, rect)| *rect)
    }
    fn click_label(&mut self, text: &str) -> Result<(), String> {
        let rect = self
            .label(text)
            .ok_or_else(|| format!("{text} não foi desenhado"))?;
        self.click(rect.center());
        Ok(())
    }
    fn shot(&mut self, ctx: &egui::Context, name: &str) {
        self.pending_shot = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
            name.to_owned(),
        )));
    }
    fn settle(&mut self) {
        self.not_before = Some(Instant::now() + Duration::from_millis(300));
    }
    fn step(&mut self, ctx: &egui::Context) -> Result<Option<String>, String> {
        if let Some(error) = &self.player.error {
            return Err(error.clone());
        }
        if !self.player.diagnostics.is_empty() {
            return Err(format!(
                "Diagnóstico do player: {:?}",
                self.player.diagnostics
            ));
        }
        let rt = self.player.runtime.as_ref().ok_or("Runtime ausente")?;
        let fired = rt
            .logs
            .iter()
            .filter(|line| line.ends_with("] disparo"))
            .count();
        let captured = oxy_render::input::cursor_captured(ctx);
        let engine_window = self.label("Jogo pausado").is_some();
        let menu = self.label("Continuar").is_some() && self.label("Sair do jogo").is_some();
        let description = match self.phase {
            0 => {
                if !rt.paused || rt.game_paused() || !rt.has_pause_menu() {
                    return Err(
                        "O jogo deveria abrir na janela da engine, com menu de pausa".into(),
                    );
                }
                self.click_label("Retomar")?;
                "Jogo com menu de pausa aberto; Retomar da janela da engine".into()
            }
            1 => {
                if rt.paused || rt.game_paused() || !captured {
                    return Err(format!(
                        "Retomar deveria jogar com o mouse preso: pausa {} / jogo {} / preso {captured}",
                        rt.paused,
                        rt.game_paused()
                    ));
                }
                self.tap(Key::Escape);
                self.settle();
                "Jogando com o mouse preso; Esc".into()
            }
            2 => {
                if rt.paused || !rt.game_paused() {
                    return Err(format!(
                        "Esc deveria pausar o jogo, não a engine: pausa {} / jogo {}",
                        rt.paused,
                        rt.game_paused()
                    ));
                }
                if captured || !menu || engine_window {
                    return Err(format!(
                        "Pausa do jogo: mouse preso {captured}, menu {menu}, janela da engine {engine_window}"
                    ));
                }
                self.shot(ctx, "player-game-menu.png");
                "Esc pausou o jogo: menu do jogo, mouse livre, sem a janela da engine".into()
            }
            3 => {
                self.unfocused = true;
                self.wait = 6;
                self.settle();
                "Janela perde o foco (Alt+Tab)".into()
            }
            4 => {
                if !rt.paused || !rt.game_paused() || !engine_window || !menu {
                    return Err(format!(
                        "Perda de foco: pausa {} / jogo {} / janela {engine_window} / menu {menu}",
                        rt.paused,
                        rt.game_paused()
                    ));
                }
                self.shot(ctx, "player-engine-over-menu.png");
                "Perda de foco: janela Jogo pausado da engine por cima do menu do jogo".into()
            }
            5 => {
                self.unfocused = false;
                self.wait = 6;
                "Janela volta a ter o foco".into()
            }
            6 => {
                self.click_label("Retomar")?;
                "Retomar da janela da engine".into()
            }
            7 => {
                if rt.paused || !rt.game_paused() || captured || !menu || engine_window {
                    return Err(format!(
                        "Retomar a engine deveria voltar ao menu do jogo: pausa {} / jogo {} / preso {captured} / menu {menu}",
                        rt.paused,
                        rt.game_paused()
                    ));
                }
                self.click_label("Continuar")?;
                "Retomar voltou à pausa do jogo; clique no Continuar desenhado pelo jogo".into()
            }
            8 => {
                self.wait = 10;
                "Quadros depois do Continuar".into()
            }
            9 => {
                if rt.game_paused() || rt.paused || !captured || menu {
                    return Err(format!(
                        "Continuar deveria retomar e prender o mouse: jogo {} / preso {captured} / menu {menu}",
                        rt.game_paused()
                    ));
                }
                if fired != 0 {
                    return Err(format!(
                        "Cliques nos botões viraram Disparar: {:?}",
                        rt.logs
                    ));
                }
                let center = ctx.content_rect().center();
                self.click(center);
                "Continuar retomou e prendeu o mouse, sem Disparar; clique no jogo".into()
            }
            10 => {
                if fired != 1 {
                    return Err(format!(
                        "Clique no jogo deveria Disparar uma vez: {:?}",
                        rt.logs
                    ));
                }
                self.tap(Key::Escape);
                "Clique no jogo com o mouse preso disparou; Esc".into()
            }
            11 => {
                if !rt.game_paused() || captured || !menu {
                    return Err("Esc não abriu o menu de novo".into());
                }
                self.click_label("Sair do jogo")?;
                "Clique em Sair do jogo".into()
            }
            _ => return Ok(None),
        };
        self.phase += 1;
        self.wait = self.wait.max(5);
        Ok(Some(description))
    }
}
impl eframe::App for PausePlayerQa {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        input
            .events
            .extend(self.events.pop_front().unwrap_or_default());
        input.modifiers = Modifiers::NONE;
        input.focused = !self.unfocused;
        input.hovered_files.clear();
        input.dropped_files.clear();
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.focused = Some(!self.unfocused);
        }
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.started.elapsed() > Duration::from_secs(45) {
            self.report.lock().unwrap().error = Some("QA da pausa excedeu 45 segundos".into());
            self.finished = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let screenshots = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Screenshot {
                        image, user_data, ..
                    } => user_data
                        .data
                        .as_ref()
                        .and_then(|data| data.downcast_ref::<String>())
                        .map(|name| (name.clone(), image.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        for (name, image) in screenshots {
            let png = PaintImage {
                width: image.width() as u32,
                height: image.height() as u32,
                pixels: image.pixels.iter().flat_map(|c| c.to_array()).collect(),
            };
            if let Err(error) = png.save(&self.output.join(&name)) {
                self.report.lock().unwrap().error = Some(error);
                self.finished = true;
            }
            self.shots.push(name);
            self.pending_shot = false;
        }
        self.player.update(ctx, frame);
        self.labels = capture_labels(ctx);
        // Sair do jogo: the player asks the window to close in the very frame it runs.
        if self.phase == 12 {
            let closing = ctx.viewport(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::Close))
            });
            let quit = self
                .player
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.quit_requested);
            if closing && quit {
                let mut report = self.report.lock().unwrap();
                report
                    .steps
                    .push("Sair do jogo fechou a janela do player".into());
                report.done = true;
                report.screenshot = self.shots.len() == 2;
                self.finished = true;
                return;
            }
        }
        if !self.events.is_empty() || self.pending_shot {
            ctx.request_repaint();
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            ctx.request_repaint();
            return;
        }
        if self.not_before.is_some_and(|time| Instant::now() < time) {
            ctx.request_repaint();
            return;
        }
        match self.step(ctx) {
            Ok(Some(description)) => self.report.lock().unwrap().steps.push(description),
            Ok(None) => {}
            Err(error) => {
                self.report.lock().unwrap().error = Some(error);
                self.finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        ctx.request_repaint();
    }
}
#[test]
#[ignore = "Opens an inactive native WGPU player; inputs and focus stay inside its RawInput"]
fn native_player_game_pause_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    isolate_progress();
    let output = oxy_core::qa::output_dir("v0.6/player-pause");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = std::env::temp_dir()
        .join(format!("oxy-player-pause-{}", new_id()))
        .join(persistence::PROJECT_FILE);
    persistence::save_project(&fixture, &pause_menu::fixture()).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Player — pausa do jogo",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 720.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(PausePlayerQa {
                player: Player::new(cc, fixture),
                report: shared,
                output: artifacts,
                started: Instant::now(),
                events: VecDeque::new(),
                labels: Vec::new(),
                phase: 0,
                wait: 12,
                unfocused: false,
                not_before: None,
                shots: Vec::new(),
                pending_shot: false,
                finished: false,
            }))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\nCapturas: {}\n{}\n",
        report.done,
        report.error,
        report.screenshot,
        report.steps.join("\n")
    );
    std::fs::write(output.join("player-pause-qa.txt"), &summary).unwrap();
    println!("{summary}");
    assert!(report.error.is_none(), "{summary}");
    assert!(report.done && report.screenshot, "{summary}");
}
