//! Real WGPU editor, with input isolated to this application's RawInput.
use super::*;
use eframe::App;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

enum Step {
    Shot(&'static str),
    Run(&'static str, Modifiers, Vec2),
    Input(Vec<Event>),
    Check(&'static str),
    Block(&'static str),
    Clear,
    Resize,
    Scale,
    Idle,
}
struct Probe {
    label: &'static str,
    until: Instant,
    elapsed: f32,
    distance: f32,
    cpu_ms: Vec<f64>,
    origin: Option<Vec3>,
    expected: Vec3,
    factor: f32,
}
#[derive(Default)]
struct Report {
    done: bool,
    error: Option<String>,
    measurements: Vec<serde_json::Value>,
    shots: Vec<String>,
}
struct Qa {
    editor: Editor,
    steps: VecDeque<Step>,
    events: VecDeque<Vec<Event>>,
    modifiers: Modifiers,
    focused: bool,
    wait: usize,
    probe: Option<Probe>,
    before: Project,
    saved: Vec<u8>,
    output: PathBuf,
    report: Arc<Mutex<Report>>,
    start: Instant,
    pending_shot: bool,
    before_block: Option<CameraState>,
    text: Option<String>,
    menu: bool,
    idle: Option<(Instant, bool, usize)>,
    uploads: Option<u64>,
}
fn key(k: Key, pressed: bool) -> Event {
    Event::Key {
        key: k,
        physical_key: Some(k),
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}
impl Qa {
    fn text_field(&mut self, ctx: &egui::Context) {
        if self.menu {
            let id = egui::Id::new("navigation-test-popup");
            egui::Popup::new(
                id,
                ctx.clone(),
                egui::PopupAnchor::Position(Pos2::new(50., 200.)),
                egui::LayerId::background(),
            )
            .open_memory(None)
            .show(|ui| {
                ui.label("Menu do teste");
            });
        }
        if let Some(text) = &mut self.text {
            egui::Window::new("Campo de texto do teste").show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(text).id(egui::Id::new("nav_test_text")))
                    .request_focus();
            });
        }
    }
    fn pointer(&self) -> Pos2 {
        self.editor.navigation.viewport.unwrap().0.center()
    }
    fn button(&self, pressed: bool) -> Event {
        Event::PointerButton {
            pos: self.pointer(),
            button: PointerButton::Secondary,
            pressed,
            modifiers: self.modifiers,
        }
    }
    fn wake(ctx: &egui::Context, ms: u64) {
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            ctx.request_repaint();
        });
    }
    fn fail(&mut self, ctx: &egui::Context, error: String) {
        self.report.lock().unwrap().error = Some(error);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn verify(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.editor.state.project != self.before
            || self.editor.dirty()
            || self.editor.history.undo_len() != 0
            || self.editor.history.redo_len() != 0
            || self.editor.runtime.is_some()
        {
            return Err("Navegação alterou documento, histórico, estado salvo ou runtime".into());
        }
        if self.editor.navigation.active && self.editor.history.is_pending() {
            return Err("Navegação iniciou uma cópia temporária para histórico".into());
        }
        self.text_field(ctx);
        if let Some(probe) = &mut self.probe {
            if probe.origin.is_none() {
                probe.origin = Some(self.editor.camera.eye());
            }
            if self.editor.navigation.active && !self.editor.navigation.started {
                let dt = (self.editor.frame_interval_ms / 1000.).clamp(0., 0.05);
                let c = &self.editor.camera;
                let direction = Vec3::new(
                    -c.pitch.cos() * c.yaw.sin(),
                    -c.pitch.sin(),
                    -c.pitch.cos() * c.yaw.cos(),
                );
                probe.expected +=
                    direction * self.editor.preferences.navigation_speed * probe.factor * dt;
                probe.elapsed += dt;
            }
            probe.distance = (self.editor.camera.eye() - probe.origin.unwrap()).length();
            if Instant::now() >= probe.until {
                let actual = self.editor.camera.eye() - probe.origin.unwrap();
                if !actual.abs_diff_eq(probe.expected, 0.003) || probe.distance < 0.05 {
                    return Err(format!(
                        "{}: deslocamento {actual:?}, esperado {:?}",
                        probe.label, probe.expected
                    ));
                }
                let mut sorted = probe.cpu_ms.clone();
                sorted.sort_by(f64::total_cmp);
                self.report.lock().unwrap().measurements.push(serde_json::json!({"label":probe.label,
                    "elapsed_clamped_seconds":probe.elapsed,"distance_metres":probe.distance,
                    "observed_metres_per_second":probe.distance/probe.elapsed,"expected_vector":probe.expected.to_array(),
                    "actual_vector":actual.to_array(),"samples":sorted.len(),"cpu_app_update_ms":probe.cpu_ms,
                    "cpu_median_ms":sorted[sorted.len()/2],"mesh_uploads":self.editor.renderer.stats().mesh_uploads,
                    "method":"Real elapsed editor dt, clamped at 50ms. CPU App::update includes rendering preparation/submission, not GPU completion or VSync. QA total duration is not a benchmark."}));
                self.probe = None;
                self.modifiers = Modifiers::NONE;
                self.events.push_back(vec![key(Key::W, false)]);
                self.wait = 3;
            }
        }
        Ok(())
    }
    fn step(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(step) = self.steps.pop_front() else {
            if std::fs::read(self.editor.path.as_ref().unwrap()).map_err(|e| e.to_string())?
                != self.saved
            {
                return Err("Arquivo salvo foi alterado".into());
            }
            self.report.lock().unwrap().done = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return Ok(());
        };
        match step {
            Step::Shot(name) => {
                self.pending_shot = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    name.to_owned(),
                )));
            }
            Step::Run(label, mods, look) => {
                self.modifiers = mods;
                let mut events = vec![Event::PointerMoved(self.pointer())];
                if !self.editor.navigation.active {
                    events.push(self.button(true));
                }
                events.extend([Event::MouseMoved(look), key(Key::W, true)]);
                self.events.push_back(events);
                self.probe = Some(Probe {
                    label,
                    until: Instant::now() + Duration::from_millis(350),
                    elapsed: 0.,
                    distance: 0.,
                    cpu_ms: vec![],
                    origin: Some(self.editor.camera.eye()),
                    expected: Vec3::ZERO,
                    factor: multiplier(mods),
                });
            }
            Step::Input(events) => self.events.push_back(events),
            Step::Check(label) => match label {
                "initial" => {
                    self.editor.gizmo = Gizmo::Scale;
                    self.uploads = Some(self.editor.renderer.stats().mesh_uploads);
                    self.report.lock().unwrap().measurements.push(serde_json::json!({"label":"initial",
                        "objects":self.editor.scene().entities.len(),"eye":self.editor.camera.eye().to_array(),
                        "gpu":self.editor.camera_test_metadata()}));
                }
                "wheel" => {
                    if (self.editor.preferences.navigation_speed - 9.6).abs() > 0.0001 {
                        return Err("Roda não alterou velocidade".into());
                    }
                    if (self.editor.camera.distance - 12.).abs() > 0.0001 {
                        return Err("RMB + roda também aplicou zoom".into());
                    }
                    if self.editor.gizmo != Gizmo::Scale {
                        return Err("W com RMB trocou ferramenta".into());
                    }
                    if egui::Popup::is_any_open(ctx) {
                        return Err("RMB abriu menu durante navegação".into());
                    }
                    self.events
                        .push_back(vec![self.button(false), key(Key::W, false)]);
                }
                "move" | "rotate" | "scale" => {
                    let expected = match label {
                        "move" => Gizmo::Move,
                        "rotate" => Gizmo::Rotate,
                        _ => Gizmo::Scale,
                    };
                    if self.editor.gizmo != expected {
                        return Err(format!("Atalho {label} não restaurado"));
                    }
                }
                "zoom" => {
                    if self.editor.camera.distance >= 12. {
                        return Err("Roda sem RMB não aplica zoom".into());
                    }
                }
                "blocked" => {
                    if self.editor.navigation.active || !self.editor.navigation.held.is_empty() {
                        return Err("Contexto bloqueado manteve navegação/teclas".into());
                    }
                    if !self
                        .editor
                        .camera
                        .view()
                        .abs_diff_eq(self.before_block.as_ref().unwrap().view(), 0.00001)
                    {
                        return Err("Câmera moveu em contexto bloqueado".into());
                    }
                }
                "cache" => {
                    if self.uploads != Some(self.editor.renderer.stats().mesh_uploads) {
                        return Err("Navegação enviou novas malhas à GPU".into());
                    }
                }
                "undo" => {
                    let camera = self.editor.camera.view();
                    self.editor.history.begin(
                        "Nome de teste",
                        &self.editor.state.project,
                        &self.editor.state.images,
                    );
                    self.editor.scene_mut().entity_mut("piece-0").unwrap().name =
                        "Peça renomeada".into();
                    self.editor.finish_history(true);
                    self.editor.undo(false);
                    if self.editor.state.project != self.before
                        || self.editor.camera.view() != camera
                    {
                        return Err(
                            "Undo do documento alterou a vista ou não restaurou o objeto".into(),
                        );
                    }
                    self.editor.undo(true);
                    if self.editor.scene().entity("piece-0").unwrap().name != "Peça renomeada"
                        || self.editor.camera.view() != camera
                    {
                        return Err(
                            "Redo do documento alterou a vista ou não restaurou o nome".into()
                        );
                    }
                    self.editor.undo(false);
                    self.editor.history.clear();
                    self.report
                        .lock()
                        .unwrap()
                        .measurements
                        .push(serde_json::json!({"undo_redo_preserved_view":true}));
                }
                _ => return Err(label.into()),
            },
            Step::Block(label) => {
                self.report
                    .lock()
                    .unwrap()
                    .measurements
                    .push(serde_json::json!({"blocked_context":label}));
                self.before_block = Some(self.editor.camera.clone());
                match label {
                    "modal" => {
                        self.editor.pending_preferences = Some(self.editor.preferences.clone())
                    }
                    "text" => {
                        self.text = Some("Texto de teste".into());
                        self.text_field(ctx);
                    }
                    "game" => self.editor.tab = Tab::Game,
                    "logic" => self.editor.tab = Tab::Logic,
                    "capture" => self.editor.capture = true,
                    "focus" => self.focused = false,
                    "outside" => self
                        .events
                        .push_back(vec![Event::PointerMoved(Pos2::new(-10., -10.))]),
                    "tab" => {
                        self.editor.tab = Tab::Studio;
                        self.editor.studio.tab = StudioTab::Model;
                    }
                    "menu" => {
                        self.menu = true;
                        egui::Popup::open_id(ctx, egui::Id::new("navigation-test-popup"));
                        self.text_field(ctx);
                    }
                    "model" => self.editor.edit_primitive_parameters("piece-0"),
                    _ => return Err(label.into()),
                }
                // Text focus must be registered by a real widget before the attempted press.
                if !matches!(label, "focus" | "outside" | "tab") {
                    self.events.push_back(vec![]);
                    self.events
                        .push_back(vec![self.button(true), key(Key::W, true)]);
                }
            }
            Step::Clear => {
                self.editor.pending_preferences = None;
                self.editor.modeling.creation = None;
                self.editor.capture = false;
                self.focused = true;
                self.text = None;
                self.menu = false;
                self.editor.tab = Tab::Scene;
                egui::Popup::close_all(ctx);
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
                self.events
                    .push_back(vec![self.button(false), key(Key::W, false)]);
            }
            Step::Resize => {
                let size = Vec2::new(920., 600.) / ctx.pixels_per_point();
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                self.wait = 12;
            }
            Step::Scale => {
                self.editor.scale = 1.2;
                self.editor.preferences.scale_percent = 120;
                ctx.set_zoom_factor(1.2);
                self.wait = 12;
            }
            Step::Idle => {
                self.events.push_back(vec![
                    self.button(false),
                    key(Key::W, false),
                    Event::PointerGone,
                ]);
                self.idle = Some((Instant::now() + Duration::from_millis(2000), false, 0));
                Self::wake(ctx, 2000);
            }
        }
        self.wait = self.wait.max(3);
        Ok(())
    }
}
impl App for Qa {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        input
            .events
            .extend(self.events.pop_front().unwrap_or_default());
        input.modifiers = self.modifiers;
        input.focused = self.focused;
        if let Some(v) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            v.focused = Some(self.focused);
        }
        self.editor.raw_input_hook(ctx, input);
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let finished = {
            let report = self.report.lock().unwrap();
            report.done || report.error.is_some()
        };
        if finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.start.elapsed() > Duration::from_secs(120) {
            self.fail(ctx, "QA excedeu 120s".into());
            return;
        }
        for (name, pixels) in ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| {
                    if let Event::Screenshot {
                        user_data, image, ..
                    } = e
                    {
                        user_data
                            .data
                            .as_ref()
                            .and_then(|v| v.downcast_ref::<String>())
                            .map(|s| (s.clone(), image.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        }) {
            let bytes: Vec<u8> = pixels.pixels.iter().flat_map(|p| p.to_array()).collect();
            if let Err(e) = image::save_buffer(
                self.output.join(&name),
                &bytes,
                pixels.width() as u32,
                pixels.height() as u32,
                image::ColorType::Rgba8,
            ) {
                self.fail(ctx, e.to_string());
                return;
            }
            if name == "small-120-percent.png" && (pixels.width() != 920 || pixels.height() != 600)
            {
                self.fail(ctx, "Captura pequena não é 920×600".into());
                return;
            }
            self.report.lock().unwrap().shots.push(name);
            self.pending_shot = false;
        }
        let start = Instant::now();
        self.editor.update(ctx, frame);
        if let Some(probe) = &mut self.probe {
            probe.cpu_ms.push(start.elapsed().as_secs_f64() * 1000.);
        }
        if let Err(e) = self.verify(ctx) {
            self.fail(ctx, e);
            return;
        }
        if let Some((deadline, measuring, count)) = &mut self.idle {
            if Instant::now() < *deadline {
                if *measuring {
                    *count += 1;
                }
                return;
            }
            if !*measuring {
                *measuring = true;
                *count = 0;
                *deadline = Instant::now() + Duration::from_millis(500);
                Self::wake(ctx, 500);
                return;
            }
            let count = *count;
            self.idle = None;
            self.report
                .lock()
                .unwrap()
                .measurements
                .push(serde_json::json!({"idle_spontaneous_updates_in_500ms":count}));
            if count > 10 {
                self.fail(ctx, "Repaint contínuo após soltar RMB".into());
                return;
            }
        }
        if self.probe.is_none() && self.events.is_empty() && !self.pending_shot {
            if self.wait > 0 {
                self.wait -= 1;
            } else if let Err(e) = self.step(ctx) {
                self.fail(ctx, e);
                return;
            }
        }
        if self.idle.is_none() {
            ctx.request_repaint();
        }
    }
}

#[test]
#[ignore = "Real Windows WGPU editor navigation, focus, document isolation and portable viewport sizes"]
fn native_navigation_v033() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.3/navigation");
    let path = output.join("project/project.oxy.json");
    let mut project = Project::new("Navegação livre — 602 objetos");
    project.scenes[0].kind = SceneKind::ThreeD;
    project.scenes[0].name = "602 objetos · navegação".into();
    for i in 0..600 {
        let mut e = Entity::new(format!("Peça {i}"), Some(Primitive::Cube));
        e.id = format!("piece-{i}");
        e.transform.position = [(i % 30) as f32 * 2. - 30., 0.3, (i / 30) as f32 * 2. - 20.];
        e.dimensions = [0.8, 0.6 + (i % 5) as f32 * 0.3, 0.8];
        e.material.color = [0.2 + (i % 3) as f32 * 0.1, 0.55, 0.6, 1.];
        project.scenes[0].entities.push(e);
    }
    let mut player = Entity::new("Jogador", None);
    player.id = "player".into();
    player.character3d = Some(Default::default());
    let mut camera = Entity::new("Câmera principal", None);
    camera.id = "game-camera".into();
    camera.camera = Some(Default::default());
    camera.camera_rig = Some(oxy_core::character::CameraRig {
        target: Some(player.id.clone()),
        ..Default::default()
    });
    oxy_core::input_actions::ensure_character(&mut project, player.character3d.as_ref().unwrap());
    oxy_core::input_actions::ensure_camera(&mut project, camera.camera_rig.as_ref().unwrap());
    project.scenes[0].entities.extend([player, camera]);
    persistence::save_project(&path, &project).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let folder = output.clone();
    eframe::run_native(
        "OXY Engine 0.3.3 — navegação",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut editor = Editor::new(cc);
            editor.open(path);
            editor.select(None);
            let mut steps = VecDeque::from([
                Step::Check("initial"),
                Step::Shot("initial.png"),
                Step::Run("base", Modifiers::NONE, Vec2::new(55., -80.)),
                Step::Shot("look-and-move.png"),
                Step::Run("fast", Modifiers::SHIFT, Vec2::ZERO),
                Step::Shot("fast.png"),
                Step::Run("precise", Modifiers::CTRL, Vec2::ZERO),
                Step::Input(vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: Vec2::new(0., 1.),
                    modifiers: Modifiers::NONE,
                }]),
                Step::Shot("speed.png"),
                Step::Check("wheel"),
                Step::Input(vec![key(Key::W, true), key(Key::W, false)]),
                Step::Check("move"),
                Step::Input(vec![key(Key::E, true), key(Key::E, false)]),
                Step::Check("rotate"),
                Step::Input(vec![key(Key::R, true), key(Key::R, false)]),
                Step::Check("scale"),
                Step::Input(vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: Vec2::new(0., 1.),
                    modifiers: Modifiers::NONE,
                }]),
                Step::Check("zoom"),
            ]);
            for label in ["modal", "text", "game", "logic", "capture", "model", "menu"] {
                steps.extend([Step::Block(label), Step::Check("blocked"), Step::Clear]);
            }
            for label in ["focus", "outside", "tab"] {
                steps.extend([
                    Step::Run("before-cancel", Modifiers::NONE, Vec2::ZERO),
                    Step::Block(label),
                    Step::Check("blocked"),
                    Step::Clear,
                ]);
            }
            steps.extend([
                Step::Scale,
                Step::Resize,
                Step::Run("small", Modifiers::NONE, Vec2::new(-15., -10.)),
                Step::Shot("small-120-percent.png"),
                Step::Clear,
                Step::Check("cache"),
                Step::Check("undo"),
                Step::Idle,
            ]);
            let before = editor.state.project.clone();
            Ok(Box::new(Qa {
                editor,
                steps,
                events: VecDeque::new(),
                modifiers: Modifiers::NONE,
                focused: true,
                wait: 8,
                probe: None,
                before,
                saved,
                output: folder,
                report: shared,
                start: Instant::now(),
                pending_shot: false,
                before_block: None,
                text: None,
                menu: false,
                idle: None,
                uploads: None,
            }))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let data = serde_json::json!({"done":report.done,"error":report.error,"measurements":report.measurements,"screenshots":report.shots,
        "input":"Isolated egui RawInput in real native WGPU window; not physical OS mouse/keyboard automation."});
    std::fs::write(
        output.join("native-navigation.json"),
        serde_json::to_vec_pretty(&data).unwrap(),
    )
    .unwrap();
    assert!(report.done && report.error.is_none(), "{data}");
}
