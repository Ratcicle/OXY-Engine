//! Permanent, bounded, native editor workload. Never sends input to another window.
//! Both baseline and final executables run exactly these fixtures and RawInput sequences.
use super::super::{Editor, Snapshot, Tab};
use super::mesh::{key, summary, system_environment, viewport};
use crate::studio::StudioTab;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use oxy_core::{
    document::*,
    geometry::{
        EditableMesh, operations, primitives,
        selection::{Mode, Selection},
    },
    painting::PaintImage,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const WARMUP: usize = 20;
const SAMPLES: usize = 101;
const PHASE_LIMIT: u64 = 35;
const ENTITY: &str = "00000000-0000-4000-8000-000000000003";

#[derive(Clone, Copy)]
enum Work {
    Draw(Mode),
    Orbit,
    Pan,
    Hover,
    Box(Mode, bool),
    Flip,
}
const PHASES: [(&str, Work); 14] = [
    ("object", Work::Draw(Mode::Object)),
    ("face", Work::Draw(Mode::Face)),
    ("edge", Work::Draw(Mode::Edge)),
    ("vertex", Work::Draw(Mode::Vertex)),
    ("orbit_face", Work::Orbit),
    ("pan_face", Work::Pan),
    ("hover_face", Work::Hover),
    ("box_face_normal", Work::Box(Mode::Face, false)),
    ("box_face_ctrl", Work::Box(Mode::Face, true)),
    ("box_edge_normal", Work::Box(Mode::Edge, false)),
    ("box_edge_ctrl", Work::Box(Mode::Edge, true)),
    ("box_vertex_normal", Work::Box(Mode::Vertex, false)),
    ("box_vertex_ctrl", Work::Box(Mode::Vertex, true)),
    ("flip_history", Work::Flip),
];

fn fixture(load: usize) -> Project {
    let mut project = Project::new("Carga nativa de componentes");
    project.id = "00000000-0000-4000-8000-000000000001".into();
    project.scenes[0].id = "00000000-0000-4000-8000-000000000002".into();
    project.start_scene = project.scenes[0].id.clone();
    project.scenes[0].kind = SceneKind::ThreeD;
    let (primitive, segments, parameters, name) = if load == 3 {
        (
            Primitive::Sphere,
            64,
            primitives::Parameters {
                latitude: 32,
                ..Default::default()
            },
            "Esfera fechada 64 × 32".into(),
        )
    } else {
        let divisions = [22, 70, 158][load];
        (
            Primitive::Plane,
            8,
            primitives::Parameters {
                plane_divisions: [divisions; 2],
                ..Default::default()
            },
            format!("Plano {divisions} × {divisions}"),
        )
    };
    let mut entity = Entity::new(name, None);
    entity.id = ENTITY.into();
    entity.mesh = Some(primitives::generate(primitive, segments, parameters).unwrap());
    entity.material.color = [0.25, 0.65, 0.75, 1.];
    project.scenes[0].entities.push(entity);
    project
}

#[derive(Default)]
struct Report {
    rows: Vec<Value>,
    error: Option<String>,
    done: bool,
    gpu: Value,
}
struct Bench {
    editor: Editor,
    report: Arc<Mutex<Report>>,
    output: PathBuf,
    source: EditableMesh,
    base: Snapshot,
    camera: oxy_render::CameraState,
    load: usize,
    phase: usize,
    cycle: usize,
    sub: usize,
    settle: usize,
    rect: Option<Rect>,
    pointer: Pos2,
    button: Option<PointerButton>,
    previous_key: Option<Key>,
    values: BTreeMap<String, Vec<u64>>,
    current_cycle_ns: u64,
    phase_started: Instant,
    uploads: Option<u64>,
    rows: Vec<Value>,
    last_count: usize,
    verified: usize,
    screenshot: bool,
    screenshot_pending: bool,
    complete: bool,
}
impl Bench {
    fn new(cc: &eframe::CreationContext<'_>, output: PathBuf, report: Arc<Mutex<Report>>) -> Self {
        let mut editor = Editor::new(cc);
        editor.scale = 1.;
        cc.egui_ctx.set_zoom_factor(1.);
        editor.preferences.scale_percent = 100;
        editor.preferences.tool_names = false;
        editor.home.visible = false;
        editor.tab = Tab::Studio;
        editor.studio.tab = StudioTab::Model;
        editor.debug = true;
        let gpu = cc.wgpu_render_state.as_ref().unwrap().adapter.get_info();
        report.lock().unwrap().gpu = json!({"name":gpu.name,"driver":gpu.driver,"driver_info":gpu.driver_info,"backend":format!("{:?}",gpu.backend)});
        let source = fixture(0).scenes[0].entities[0].mesh.clone().unwrap();
        let base = editor.state.clone();
        let camera = editor.camera.clone();
        let mut this = Self {
            editor,
            report,
            output,
            source,
            base,
            camera,
            load: 0,
            phase: 0,
            cycle: 0,
            sub: 0,
            settle: 3,
            rect: None,
            pointer: Pos2::ZERO,
            button: None,
            previous_key: None,
            values: BTreeMap::new(),
            current_cycle_ns: 0,
            phase_started: Instant::now(),
            uploads: None,
            rows: Vec::new(),
            last_count: 0,
            verified: 0,
            screenshot: false,
            screenshot_pending: false,
            complete: false,
        };
        this.set_load(0);
        this
    }
    fn set_load(&mut self, load: usize) {
        self.load = load;
        let project = fixture(load);
        self.editor.scene_id = project.start_scene.clone();
        self.source = project.scenes[0].entities[0].mesh.clone().unwrap();
        self.editor.state.project = project;
        self.editor.history = Default::default();
        self.editor.modeling = Default::default();
        self.editor.camera = oxy_render::CameraState::for_scene(self.editor.scene());
        self.editor.select(Some(ENTITY.into()));
        self.editor.frame_selection();
        self.camera = self.editor.camera.clone();
        self.base = self.editor.state.clone();
        self.rows.clear();
        self.uploads = None;
        self.screenshot = false;
        self.screenshot_pending = false;
        self.set_phase(0);
    }
    fn set_phase(&mut self, phase: usize) {
        self.phase = phase;
        self.cycle = 0;
        self.sub = 0;
        self.settle = 3;
        self.editor.state = self.base.clone();
        self.editor.history = Default::default();
        self.editor.modeling = Default::default();
        self.editor.camera = self.camera.clone();
        self.editor.select(Some(ENTITY.into()));
        self.editor.modeling.selection.mode = match PHASES[phase].1 {
            Work::Draw(mode) | Work::Box(mode, _) => mode,
            _ => Mode::Face,
        };
        if !matches!(PHASES[phase].1, Work::Draw(Mode::Object) | Work::Box(..)) {
            let ids = match self.editor.modeling.selection.mode {
                Mode::Edge => vec![self.source.data().edges[0].id],
                Mode::Vertex => vec![self.source.data().vertices[0].id],
                _ => vec![self.source.data().faces[self.source.data().faces.len() / 3].id],
            };
            self.editor.modeling.selection.ids = ids;
        }
        self.values.clear();
        self.current_cycle_ns = 0;
        self.last_count = 0;
        self.verified = 0;
        self.phase_started = Instant::now();
    }
    fn fail(&mut self, ctx: &egui::Context, error: impl Into<String>) {
        self.report.lock().unwrap().error = Some(format!(
            "load {}, phase {}, cycle {}, sub {}: {}",
            self.load,
            PHASES[self.phase].0,
            self.cycle,
            self.sub,
            error.into()
        ));
        self.complete = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn finish_phase(&mut self, ctx: &egui::Context, limited: bool) {
        let work = PHASES[self.phase].1;
        let stats: BTreeMap<_, _> = self
            .values
            .iter()
            .map(|(key, raw)| (key, summary(raw)))
            .collect();
        let orbit_changed = (self.editor.camera.yaw - self.camera.yaw).abs() > 0.001;
        let pan_changed = self.editor.camera.target.distance(self.camera.target) > 0.0001;
        self.rows.push(json!({"phase":PHASES[self.phase].0,"measurements":stats,"limited":limited,
            "measured_cycles":self.cycle.saturating_sub(WARMUP),"elapsed_seconds":self.phase_started.elapsed().as_secs_f64(),
            "last_selected_components":self.last_count,"verified_undo_redo_cycles":self.verified,
            "orbit_changed":orbit_changed,"pan_changed":pan_changed,"mesh_uploads":self.editor.renderer.stats().mesh_uploads,
            "history_retained_estimated_bytes":self.editor.history.estimated_bytes(),
            "content_logical_points":[ctx.content_rect().width(),ctx.content_rect().height()],
            "pixels_per_point":ctx.pixels_per_point(),
            "viewport_logical_points":self.rect.map(|r|[r.width(),r.height()]),
        }));
        eprintln!(
            "v021 native {} triangles / {}: {} samples, {:.2}s, limited={limited}",
            self.source.prepared().triangles.len(),
            PHASES[self.phase].0,
            self.cycle.saturating_sub(WARMUP),
            self.phase_started.elapsed().as_secs_f64()
        );
        std::fs::write(self.output.join("progress.json"),serde_json::to_vec_pretty(&json!({"fixture":self.load,"phases":self.rows})).unwrap()).unwrap();
        if matches!(work, Work::Orbit) && !orbit_changed
            || matches!(work, Work::Pan) && !pan_changed
        {
            self.fail(ctx, "RawInput não moveu a câmera");
            return;
        }
        if self.phase + 1 < PHASES.len() {
            self.set_phase(self.phase + 1);
        } else {
            self.report.lock().unwrap().rows.push(json!({"fixture":self.base.project.scenes[0].entities[0].name,
                "entities":1,"triangles":self.source.prepared().triangles.len(),"vertices":self.source.data().vertices.len(),"edges":self.source.data().edges.len(),"faces":self.source.data().faces.len(),
                "mesh_estimated_bytes":self.source.estimated_bytes(),"phases":self.rows,"screenshot_received":self.screenshot,
                "core_extrusion":core_extrusion(&self.source),
            }));
            if self.load < 3 {
                self.set_load(self.load + 1);
            } else {
                self.report.lock().unwrap().done = true;
                self.complete = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn core_extrusion(source: &EditableMesh) -> Value {
    let face = &source.data().faces[source.data().faces.len() / 3];
    let a = source.position(face.corners[0].vertex).unwrap();
    let b = source.position(face.corners[1].vertex).unwrap();
    let c = source.position(face.corners[2].vertex).unwrap();
    let delta = (b - a).cross(c - a).normalize() * 0.02;
    let selection = Selection {
        mode: Mode::Face,
        ids: vec![face.id],
        through: false,
    };
    let start = Instant::now();
    let mut values = Vec::new();
    let mut observed_faces = 0;
    for cycle in 0..WARMUP + SAMPLES {
        if start.elapsed() > Duration::from_secs(8) {
            break;
        }
        let t = Instant::now();
        let result =
            operations::extrude(std::hint::black_box(source), &selection, delta, false).unwrap();
        let elapsed = t.elapsed().as_nanos() as u64;
        observed_faces = std::hint::black_box(result.mesh.data().faces.len());
        if cycle >= WARMUP {
            values.push(elapsed);
        }
    }
    json!({"generation_and_validation":summary(&values),"final_face_count":observed_faces,"limited":values.len()<SAMPLES,"limit_seconds":8,"distance_local":0.02,"selected_faces":1})
}

impl eframe::App for Bench {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        input.focused = true;
        input.modifiers = Modifiers::NONE;
        input.hovered_files.clear();
        input.dropped_files.clear();
        if let Some(v) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            v.focused = Some(true);
        }
        if let Some(previous) = self.previous_key.take() {
            input.events.push(key(previous, false, Modifiers::NONE));
        }
        if self.settle > 0 {
            if let Some(button) = self.button.take() {
                input.events.push(Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                });
            }
            input.events.push(Event::PointerGone);
            return;
        }
        let Some(rect) = self.rect else {
            return;
        };
        let mut desired = None;
        let mut modifiers = Modifiers::NONE;
        self.pointer = rect.center();
        match PHASES[self.phase].1 {
            Work::Orbit | Work::Pan => {
                desired = Some(if matches!(PHASES[self.phase].1, Work::Orbit) {
                    PointerButton::Secondary
                } else {
                    PointerButton::Middle
                });
                self.pointer += egui::vec2(
                    if self.cycle % 20 < 10 {
                        (self.cycle % 10) as f32
                    } else {
                        (9 - self.cycle % 10) as f32
                    },
                    0.,
                );
            }
            Work::Hover => {
                self.pointer += egui::vec2(
                    (self.cycle % 41) as f32 - 20.,
                    (self.cycle % 17) as f32 - 8.,
                );
            }
            Work::Box(mode, ctrl) => {
                if self.sub == 0 {
                    self.editor.modeling.selection.ids = if ctrl {
                        vec![match mode {
                            Mode::Face => self.source.data().faces[0].id,
                            Mode::Edge => self.source.data().edges[0].id,
                            _ => self.source.data().vertices[0].id,
                        }]
                    } else {
                        vec![]
                    };
                }
                if ctrl {
                    modifiers = Modifiers::CTRL;
                }
                self.pointer = if self.sub == 0 {
                    rect.min + egui::vec2(35., 45.)
                } else {
                    rect.center() + egui::vec2(60. + (self.cycle % 5) as f32, 40.)
                };
                if self.sub < 2 {
                    desired = Some(PointerButton::Primary);
                }
            }
            Work::Flip => {
                let action = match self.sub {
                    0 => Some((Key::N, Modifiers::SHIFT)),
                    1 => Some((Key::Enter, Modifiers::NONE)),
                    2 | 4 => Some((Key::Z, Modifiers::COMMAND)),
                    3 => Some((Key::Y, Modifiers::COMMAND)),
                    _ => None,
                };
                if let Some((code, mods)) = action {
                    modifiers = mods;
                    input.events.push(key(code, true, mods));
                    self.previous_key = Some(code);
                }
            }
            Work::Draw(_) => {}
        }
        input.modifiers = modifiers;
        input.events.push(Event::PointerMoved(self.pointer));
        if self.button != desired {
            if let Some(button) = self.button.take() {
                input.events.push(Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: false,
                    modifiers,
                });
            }
            if let Some(button) = desired {
                input.events.push(Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: true,
                    modifiers,
                });
            }
            self.button = desired;
        }
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.complete {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        for image in ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| {
                    if let Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        }) {
            let image = PaintImage {
                width: image.width() as u32,
                height: image.height() as u32,
                pixels: image.pixels.iter().flat_map(|p| p.to_array()).collect(),
            };
            if let Err(e) = image.save(&self.output.join(format!(
                "mesh-{}-editor.png",
                self.source.prepared().triangles.len()
            ))) {
                self.fail(ctx, e);
                return;
            }
            self.screenshot = true;
        }
        let t = Instant::now();
        self.editor.update(ctx, frame);
        let elapsed = t.elapsed().as_nanos() as u64;
        self.rect = viewport(ctx);
        if self.settle > 0 {
            self.settle -= 1;
            ctx.request_repaint();
            return;
        }
        if self.phase == 0 && self.cycle == WARMUP {
            self.uploads = Some(self.editor.renderer.stats().mesh_uploads);
        }
        let work = PHASES[self.phase].1;
        if !matches!(work, Work::Flip)
            && self.cycle >= WARMUP
            && (self
                .editor
                .scene()
                .entity(ENTITY)
                .unwrap()
                .mesh
                .as_ref()
                .unwrap()
                .revision()
                != self.source.revision()
                || self.editor.renderer.stats().mesh_uploads != self.uploads.unwrap())
        {
            self.fail(
                ctx,
                "Navegação/seleção alterou revisão ou reenviou malha à GPU",
            );
            return;
        }
        if self.cycle >= WARMUP {
            let key = match work {
                Work::Box(..) => ["press_cpu", "drag_cpu", "release_cpu", "settle_cpu"][self.sub],
                Work::Flip => [
                    "command_cpu",
                    "legacy_enter_cpu",
                    "undo_cpu",
                    "redo_cpu",
                    "restore_cpu",
                    "settle_cpu",
                ][self.sub],
                _ => "app_update_cpu",
            };
            self.values.entry(key.into()).or_default().push(elapsed);
        }
        self.current_cycle_ns += elapsed;
        if matches!(work, Work::Flip) && self.sub < 5 {
            let current = self
                .editor
                .scene()
                .entity(ENTITY)
                .unwrap()
                .mesh
                .as_ref()
                .unwrap();
            let valid = match self.sub {
                0 => current != &self.source,
                1 => current != &self.source && self.editor.history.undo_len() == 1,
                2 | 4 => current == &self.source && self.editor.history.undo_len() == 0,
                3 => current != &self.source && self.editor.history.undo_len() == 1,
                _ => true,
            };
            if !valid {
                self.fail(
                    ctx,
                    "Comando/Undo/Redo não restaurou a malha/histórico esperado",
                );
                return;
            }
            if self.sub == 4 && self.cycle >= WARMUP {
                self.verified += 1;
            }
        }
        if self.phase == 1 && self.cycle == 60 && !self.screenshot_pending {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.screenshot_pending = true;
        }
        let frames = match work {
            Work::Box(..) => 4,
            Work::Flip => 6,
            _ => 1,
        };
        self.sub += 1;
        if self.sub == frames {
            self.last_count = self.editor.mesh_components().ids.len();
            if self.cycle >= WARMUP {
                self.values
                    .entry("complete_cycle_cpu".into())
                    .or_default()
                    .push(self.current_cycle_ns);
            }
            self.current_cycle_ns = 0;
            self.sub = 0;
            self.cycle += 1;
            if self.cycle == WARMUP + SAMPLES
                || self.phase_started.elapsed() > Duration::from_secs(PHASE_LIMIT)
            {
                self.finish_phase(ctx, self.cycle < WARMUP + SAMPLES);
            }
        }
        ctx.request_repaint();
    }
}

#[test]
#[ignore = "Opt-in v0.2.1 native editor benchmark; bounded phases and isolated RawInput"]
fn native_component_v021_measurements() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let root = std::env::var_os("OXY_BENCH_ROOT").map(PathBuf::from).unwrap_or_else(||Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let label = std::env::var("OXY_BENCH_LABEL").unwrap_or_else(|_| "current".into());
    assert!(
        label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    );
    let output = root.join(format!("qa/v0.2.1/{label}"));
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    eframe::run_native(
        "OXY — referência nativa v0.2.1",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_min_inner_size([1440., 900.])
                .with_max_inner_size([1440., 900.])
                .with_resizable(false)
                .with_maximize_button(false)
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(Bench::new(cc, output, result)))),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let safe = format!(
        "safe.directory={}",
        root.canonicalize()
            .unwrap()
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/")
    );
    let commit = std::process::Command::new("git")
        .args(["-c", &safe, "rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    let compiler = std::process::Command::new(
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".cargo/bin/rustc.exe"),
    )
    .args(["--version", "--verbose"])
    .output()
    .unwrap();
    let json = json!({"commit":String::from_utf8_lossy(&commit.stdout).trim(),"label":label,"version":env!("CARGO_PKG_VERSION"),
        "system":system_environment(&root),"gpu":report.gpu,"compiler":String::from_utf8_lossy(&compiler.stdout).trim(),
        "window_logical_points":[1440,900],"interface_scale":1.,"warmup_cycles":WARMUP,"sample_cycles":SAMPLES,"phase_limit_seconds":PHASE_LIMIT,
        "method":"Release --locked opt-level=2. Deterministic Plane22/70/158 and Sphere64x32. CPU around real Editor::update includes driver submission, excludes eframe tessellation/GPU completion/VSync. Creation/reset outside measurements. RawInput orbit/pan/hover; rectangular selection begins at viewport(+35,+45) and ends center(+60+cycle%5,+40); four frames press/drag/release/settle. Ctrl starts with one original component. Six frames flip/Enter/Undo/Redo/Undo/settle; Enter compatibility step remains in final workload. Modes/selection initialization outside measured update. Actual source/revision/history/upload checks. Forced repaint only in harness; no GPU duration, FPS, process RAM or VRAM inferred. Duration limits preserve partial samples and continue subsequent phases.",
        "rows":report.rows,"completed_all_fixtures":report.done,"error":report.error,
        "new_operations":"Inset and last-operation adjustment absent in base; add separately identified final-only measurements, never compare to a different operation."
    });
    std::fs::write(
        root.join(format!("benchmarks/results/v021-native-{label}.json")),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();
    assert!(
        report.error.is_none(),
        "{}",
        report.error.as_deref().unwrap_or_default()
    );
    assert!(report.done);
}
