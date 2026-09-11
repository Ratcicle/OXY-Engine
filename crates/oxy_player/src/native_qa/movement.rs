use super::*;
use oxy_core::{character::CameraMode, document::Project};
struct MovementPlayerQa {
    player: Player,
    base: Project,
    report: Arc<Mutex<Report>>,
    output: PathBuf,
    started: Instant,
    events: VecDeque<Vec<Event>>,
    phase: u8,
    until: f64,
    wait: usize,
    paused_time: f64,
    shot: bool,
    finished: bool,
}
impl MovementPlayerQa {
    fn key(&mut self, key: Key, pressed: bool) {
        self.events.push_back(vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    fn tap(&mut self, key: Key) {
        self.key(key, true);
        self.key(key, false);
    }
    fn resume(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let button = capture_labels(ctx)
            .into_iter()
            .find(|(text, _)| text == "Retomar")
            .ok_or("Botão Retomar ausente")?
            .1
            .center();
        self.events.push_back(vec![Event::PointerMoved(button)]);
        for pressed in [true, false] {
            self.events.push_back(vec![Event::PointerButton {
                pos: button,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }
        Ok(())
    }
    fn step(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if let Some(e) = &self.player.error {
            return Err(e.clone());
        }
        if !self.player.diagnostics.is_empty() {
            return Err(format!("{:?}", self.player.diagnostics));
        }
        let rt = self.player.runtime.as_ref().ok_or("Runtime ausente")?;
        if !rt.logs.is_empty() {
            return Err(format!("{:?}", rt.logs));
        }
        if !self.events.is_empty() || rt.time < self.until {
            return Ok(());
        }
        if self.wait > 0 {
            self.wait -= 1;
            return Ok(());
        }
        let body = rt
            .scene()
            .entities
            .iter()
            .find(|e| e.character3d.is_some())
            .unwrap();
        let state = rt.character_state(&body.id).unwrap();
        match self.phase {
            0 => {
                if !rt.paused {
                    return Err("Player iniciou capturando sem consentimento".into());
                }
                self.resume(ctx)?;
            }
            1 => {
                if rt.paused {
                    return Ok(());
                }
                self.until = rt.time + 1.;
                self.key(Key::W, true);
                self.events
                    .push_back(vec![Event::MouseMoved(egui::vec2(40., -10.))]);
            }
            2 => {
                if state.position.distance([0., 0.02, 4.].into()) < 2. {
                    return Err("W não movimentou personagem no player".into());
                }
                if state.look_yaw.abs() < 0.03 {
                    return Err("Olhar relativo não aplicado".into());
                }
                self.until = rt.time + 0.4;
                self.key(Key::W, false);
                self.tap(Key::V);
            }
            3 => {
                if rt.camera_mode(rt.active_camera().unwrap()) != Some(CameraMode::ThirdPerson) {
                    return Err("Troca de câmera não chegou ao player".into());
                }
                let stats = self.player.renderer.as_ref().unwrap().stats();
                if stats.triangles < 100 || stats.visible_objects < 20 {
                    return Err("Geometria da pista não foi renderizada".into());
                }
                self.report.lock().unwrap().steps.push(format!(
                    "Player: {} objetos visíveis, {} triângulos; movimento/olhar/TP a {:.3} s",
                    stats.visible_objects, stats.triangles, rt.time
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            4 => {
                if !self.shot {
                    return Ok(());
                }
                self.tap(Key::Escape);
                self.wait = 12;
            }
            5 => {
                if !rt.paused {
                    return Err("Escape não pausou".into());
                }
                self.paused_time = rt.time;
                self.wait = 10;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(920., 600.)));
                ctx.set_zoom_factor(1.2);
            }
            6 => {
                if !rt.paused || rt.time != self.paused_time {
                    return Err("Pausa/redimensionamento avançou simulação".into());
                }
                self.resume(ctx)?;
            }
            7 => {
                if rt.paused {
                    return Ok(());
                }
                self.until = rt.time + 0.2;
                self.tap(Key::R);
            }
            _ => {
                if !self.shot {
                    return Ok(());
                }
                if state.position.distance([0., 0.02, 4.].into()) > 0.15 {
                    return Err("Reinício por nós não restaurou origem".into());
                }
                if persistence::load_project(&self.player.path)? != self.base {
                    return Err("Player alterou projeto".into());
                }
                let mut report = self.report.lock().unwrap();
                report.done = true;
                report.screenshot = true;
                report.steps.push("R restaurou checkpoint por nós; Escape/Retomar, 920×600 a 120%, arquivo reaberto intacto. RawInput isolado, sem certificar mouse físico.".into());
                self.finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.phase += 1;
        Ok(())
    }
}
impl eframe::App for MovementPlayerQa {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        input
            .events
            .extend(self.events.pop_front().unwrap_or_default());
        input.focused = true;
        input.modifiers = Modifiers::NONE;
        input.hovered_files.clear();
        input.dropped_files.clear();
        if let Some(view) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            view.focused = Some(true);
        }
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = image {
            let png = PaintImage {
                width: image.width() as u32,
                height: image.height() as u32,
                pixels: image.pixels.iter().flat_map(|c| c.to_array()).collect(),
            };
            png.save(&self.output.join("standalone-player.png"))
                .unwrap();
            self.shot = true;
        }
        self.player.update(ctx, frame);
        let result = if self.started.elapsed() > Duration::from_secs(30) {
            Err("Tempo limite do QA do player".into())
        } else {
            self.step(ctx)
        };
        if let Err(e) = result {
            self.report.lock().unwrap().error = Some(e);
            self.finished = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}
#[test]
#[ignore = "Real native WGPU standalone player; isolated RawInput"]
fn native_player_movement_v030() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.0/m6");
    std::fs::create_dir_all(&output).unwrap();
    let fixture = std::env::temp_dir()
        .join(format!("oxy-movement-player-{}", new_id()))
        .join(persistence::PROJECT_FILE);
    let base = oxy_core::guide_recipes::movement_laboratory().unwrap();
    persistence::save_project(&fixture, &base).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — player 0.3.0",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 720.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(MovementPlayerQa {
                player: Player::new(cc, fixture),
                base,
                report: shared,
                output: artifacts,
                started: Instant::now(),
                events: VecDeque::new(),
                phase: 0,
                until: 0.,
                wait: 12,
                paused_time: 0.,
                shot: false,
                finished: false,
            }))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\n{}",
        report.done,
        report.error,
        report.steps.join("\n")
    );
    std::fs::write(output.join("native-player.txt"), &summary).unwrap();
    assert!(
        report.done && report.screenshot && report.error.is_none(),
        "{summary}"
    );
}
