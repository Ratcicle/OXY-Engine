//! Full native editor CPU measurements. Test input never leaves this eframe host.
use super::super::{Editor, Snapshot, Tab};
use crate::studio::StudioTab;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use glam::Vec3;
use oxy_core::{
    document::*,
    geometry::{EditableMesh, primitives},
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
const ENTITY: &str = "00000000-0000-4000-8000-000000000003";
const PHASES: [&str; 10] = [
    "object_idle_forced_redraw",
    "face_overlay",
    "orbit_raw_input",
    "pan_raw_input",
    "selection_raw_input",
    "operator_preview_raw_input",
    "operator_confirm_raw_input",
    "undo_raw_input",
    "redo_raw_input",
    "restore_raw_input",
];

#[derive(Default)]
struct Report {
    rows: Vec<Value>,
    error: Option<String>,
    done: bool,
    limited: bool,
    gpu: Value,
}
struct MeasuredEditor {
    editor: Editor,
    report: Arc<Mutex<Report>>,
    output: PathBuf,
    divisions: u32,
    base: Snapshot,
    source: EditableMesh,
    faces: [u32; 2],
    centers: [Vec3; 2],
    phase: usize,
    cycle: usize,
    subframe: usize,
    settle_frames: usize,
    values: BTreeMap<&'static str, Vec<u64>>,
    warm_cache_uploads: Option<u64>,
    revision: u64,
    rect: Option<Rect>,
    previous_key: Option<Key>,
    previous_button: Option<PointerButton>,
    pointer: Pos2,
    initial_camera: oxy_render::CameraState,
    started: Instant,
    phase_start: Instant,
    screenshot: bool,
    requested_screenshot: bool,
    complete: bool,
    orbit_verified: bool,
    pan_verified: bool,
    selections_verified: usize,
    operations_verified: usize,
}

fn fixture(divisions: u32) -> Project {
    let mut project = Project::new("Carga de malha nativa");
    project.id = "00000000-0000-4000-8000-000000000001".into();
    project.scenes[0].id = "00000000-0000-4000-8000-000000000002".into();
    project.start_scene = project.scenes[0].id.clone();
    project.scenes[0].kind = SceneKind::ThreeD;
    project.scenes[0].name = "Carga 3D".into();
    let mesh = primitives::generate(
        Primitive::Plane,
        8,
        primitives::Parameters {
            plane_divisions: [divisions; 2],
            ..Default::default()
        },
    )
    .unwrap();
    let mut entity = Entity::new(format!("Plano {divisions} × {divisions}"), None);
    entity.id = ENTITY.into();
    entity.mesh = Some(mesh);
    entity.material.color = [0.25, 0.65, 0.75, 1.];
    project.scenes[0].entities.push(entity);
    project
}

impl MeasuredEditor {
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
        let project = fixture(22);
        let source = project.scenes[0].entities[0].mesh.clone().unwrap();
        editor.scene_id = project.start_scene.clone();
        editor.state.project = project;
        editor.camera = oxy_render::CameraState::for_scene(editor.scene());
        editor.select(Some(ENTITY.into()));
        editor.frame_selection();
        let camera = editor.camera.clone();
        let base = editor.state.clone();
        let mut this = Self {
            editor,
            report,
            output,
            divisions: 22,
            base,
            source: source.clone(),
            faces: [0; 2],
            centers: [Vec3::ZERO; 2],
            phase: 0,
            cycle: 0,
            subframe: 0,
            settle_frames: 3,
            values: BTreeMap::new(),
            warm_cache_uploads: None,
            revision: source.revision(),
            rect: None,
            previous_key: None,
            previous_button: None,
            pointer: Pos2::ZERO,
            initial_camera: camera,
            started: Instant::now(),
            phase_start: Instant::now(),
            screenshot: false,
            requested_screenshot: false,
            complete: false,
            orbit_verified: false,
            pan_verified: false,
            selections_verified: 0,
            operations_verified: 0,
        };
        this.face_targets();
        this
    }
    fn face_targets(&mut self) {
        // Interior cells avoid the selected object's outer outline and transform gizmos.
        for (slot, index) in [
            self.source.data().faces.len() / 3,
            self.source.data().faces.len() / 2 + 2,
        ]
        .into_iter()
        .enumerate()
        {
            let face = &self.source.data().faces[index];
            self.faces[slot] = face.id;
            self.centers[slot] = face
                .corners
                .iter()
                .map(|c| self.source.position(c.vertex).unwrap())
                .sum::<Vec3>()
                / face.corners.len() as f32;
        }
    }
    fn fail(&mut self, ctx: &egui::Context, error: String) {
        self.report.lock().unwrap().error = Some(format!(
            "{} divisions, phase {}, cycle {}: {error}",
            self.divisions, self.phase, self.cycle
        ));
        self.complete = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn next_phase(&mut self) {
        eprintln!(
            "Native mesh {} triangles: phase {} complete ({:.2}s)",
            self.source.prepared().triangles.len(),
            PHASES[self.phase],
            self.phase_start.elapsed().as_secs_f32()
        );
        self.phase += 1;
        self.cycle = 0;
        self.subframe = 0;
        self.settle_frames = 2;
        self.phase_start = Instant::now();
        if self.phase == 1 {
            self.editor.select_mesh_face(Some(self.faces[0]), false);
        }
        if self.phase == 2 || self.phase == 3 {
            self.editor.camera = self.initial_camera.clone();
        }
        if self.phase == 4 || self.phase == 5 {
            self.editor.camera = self.initial_camera.clone();
            self.editor.select_mesh_face(Some(self.faces[0]), false);
        }
    }
    fn finish_load(&mut self) -> Result<(), String> {
        if !self.screenshot
            || !self.orbit_verified
            || !self.pan_verified
            || self.selections_verified < SAMPLES
            || self.operations_verified < SAMPLES
        {
            return Err("A carga não concluiu imagem, navegação, seleção e histórico reais".into());
        }
        if self.editor.state != self.base {
            return Err("O ciclo de operador/desfazer não restaurou o documento".into());
        }
        let measures: BTreeMap<_, _> = self
            .values
            .iter()
            .map(|(key, values)| (*key, summary(values)))
            .collect();
        self.report.lock().unwrap().rows.push(json!({
            "divisions":self.divisions,"entities":1,"vertices":self.source.data().vertices.len(),
            "triangles":self.source.prepared().triangles.len(),"geometry_estimated_bytes":self.source.estimated_bytes(),
            "history_retained_estimated_bytes":self.editor.history.estimated_bytes(),
            "verified_selection_clicks":self.selections_verified,"verified_operator_cycles":self.operations_verified,
            "camera_orbit_verified":self.orbit_verified,"camera_pan_verified":self.pan_verified,
            "mesh_uploads_after_warmup_before_operators":self.warm_cache_uploads,
            "geometry_uploads_during_warm_navigation_and_selection":0,
            "cpu_app_update":measures,"limit_exceeded":false
        }));
        Ok(())
    }
    fn reset_load(&mut self, divisions: u32) {
        self.divisions = divisions;
        let project = fixture(divisions);
        self.source = project.scenes[0].entities[0].mesh.clone().unwrap();
        self.revision = self.source.revision();
        self.editor.state.project = project;
        self.editor.camera = oxy_render::CameraState::for_scene(self.editor.scene());
        self.editor.history = Default::default();
        self.editor.modeling = Default::default();
        self.editor.select(Some(ENTITY.into()));
        self.editor.frame_selection();
        self.initial_camera = self.editor.camera.clone();
        self.base = self.editor.state.clone();
        self.face_targets();
        self.phase = 0;
        self.cycle = 0;
        self.subframe = 0;
        self.settle_frames = 3;
        self.values.clear();
        self.warm_cache_uploads = None;
        self.started = Instant::now();
        self.phase_start = Instant::now();
        self.screenshot = false;
        self.requested_screenshot = false;
        self.orbit_verified = false;
        self.pan_verified = false;
        self.selections_verified = 0;
        self.operations_verified = 0;
    }
}

pub(super) fn summary(values: &[u64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    json!({"samples":n,"median_ns":sorted.get(n/2),"p95_ns":if n>=100{sorted.get(((n-1) as f64*0.95).ceil() as usize)}else{None},"p99_ns":if n>=100{sorted.get(((n-1) as f64*0.99).ceil() as usize)}else{None},"raw_ns":values})
}

pub(super) fn system_environment(root: &Path) -> Value {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    // One bounded, local probe outside every timed sample; no profiler in the product.
    let command = "$ErrorActionPreference='Stop'; $c=Get-CimInstance Win32_Processor | Select-Object -First 1; $s=Get-CimInstance Win32_ComputerSystem; $o=Get-CimInstance Win32_OperatingSystem; @{cpu=$c.Name; logical_cpus=$s.NumberOfLogicalProcessors; ram_capacity_bytes=$s.TotalPhysicalMemory; os=@{Caption=$o.Caption;Version=$o.Version;BuildNumber=$o.BuildNumber};source='local CIM probe outside measurements'} | ConvertTo-Json -Compress";
    let executable =
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:/Windows".into()))
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    if let Ok(mut child) = Command::new(executable)
        .args(["-NoProfile", "-NonInteractive", "-Command", command])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW: never disturb the desktop.
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        let started = Instant::now();
        loop {
            if child.try_wait().ok().flatten().is_some() {
                if let Ok(output) = child.wait_with_output()
                    && let Ok(value) = serde_json::from_slice::<Value>(&output.stdout)
                {
                    return value;
                }
                break;
            }
            if started.elapsed() > Duration::from_secs(6) {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let snapshot = std::fs::read(root.join("benchmarks/results/v020-mesh-final-environment.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    match snapshot {
        Some(old) => {
            json!({"source":"M6 hardware snapshot; live probe unavailable","file":"v020-mesh-final-environment.json","recorded_utc":old["utc"],"cpu":old["cpu"],"logical_cpus":old["logical_cpus"],"ram_capacity_bytes":old["ram_bytes"],"os":old["os"]})
        }
        None => json!({"source":"system probe unavailable; no RAM estimate substituted"}),
    }
}
pub(super) fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    }
}
pub(super) fn viewport(ctx: &egui::Context) -> Option<Rect> {
    let shapes = ctx.graphics(|g| g.clone().drain(&[], &Default::default()));
    fn collect(shape: &egui::Shape, largest: &mut Option<Rect>) {
        match shape {
            egui::Shape::Mesh(mesh) if matches!(mesh.texture_id, egui::TextureId::User(_)) => {
                let rect = mesh.calc_bounds();
                if largest.is_none_or(|old| old.area() < rect.area()) {
                    *largest = Some(rect);
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, largest);
                }
            }
            _ => {}
        }
    }
    let mut largest = None;
    for shape in shapes {
        collect(&shape.shape, &mut largest);
    }
    largest
}

impl eframe::App for MeasuredEditor {
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
        if self.settle_frames > 0 {
            if let Some(button) = self.previous_button.take() {
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
        let mut desired_button = None;
        if self.phase == 2 || self.phase == 3 {
            desired_button = Some(if self.phase == 2 {
                PointerButton::Secondary
            } else {
                PointerButton::Middle
            });
            self.pointer = rect.center()
                + egui::vec2(
                    if self.cycle % 20 < 10 {
                        (self.cycle % 10) as f32
                    } else {
                        (9 - self.cycle % 10) as f32
                    },
                    0.,
                );
        } else if self.phase == 4 {
            self.pointer = oxy_render::collider_debug::project(
                &self.editor.camera,
                rect,
                self.centers[self.cycle % 2],
            )
            .unwrap();
            if self.subframe == 0 {
                desired_button = Some(PointerButton::Primary);
            }
        } else {
            self.pointer = rect.center();
        }
        input.events.push(Event::PointerMoved(self.pointer));
        if self.previous_button != desired_button {
            if let Some(button) = self.previous_button.take() {
                input.events.push(Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                });
            }
            if let Some(button) = desired_button {
                input.events.push(Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                });
            }
            self.previous_button = desired_button;
        }
        if self.phase == 5 && self.subframe < 5 {
            let (key_code, modifiers) = match self.subframe {
                0 => (Key::N, Modifiers::SHIFT),
                1 => (Key::Enter, Modifiers::NONE),
                2 | 4 => (Key::Z, Modifiers::COMMAND),
                3 => (Key::Y, Modifiers::COMMAND),
                _ => unreachable!(),
            };
            input.modifiers = modifiers;
            input.events.push(key(key_code, true, modifiers));
            self.previous_key = Some(key_code);
        }
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.complete {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.started.elapsed() > Duration::from_secs(150)
            || self.phase_start.elapsed() > Duration::from_secs(45)
        {
            let measures: BTreeMap<_, _> = self
                .values
                .iter()
                .map(|(key, values)| (*key, summary(values)))
                .collect();
            let mut report = self.report.lock().unwrap();
            report.rows.push(json!({"divisions":self.divisions,"entities":1,"vertices":self.source.data().vertices.len(),"triangles":self.source.prepared().triangles.len(),"geometry_estimated_bytes":self.source.estimated_bytes(),"history_retained_estimated_bytes":self.editor.history.estimated_bytes(),"phase":PHASES[self.phase],"verified_selection_clicks":self.selections_verified,"verified_operator_cycles":self.operations_verified,"camera_orbit_verified":self.orbit_verified,"camera_pan_verified":self.pan_verified,"samples":self.cycle.saturating_sub(WARMUP),"limit_exceeded":true,"cpu_app_update":measures}));
            report.limited = true;
            eprintln!(
                "LIMIT EXCEEDED: {} triangles, phase {}, {} completed measured cycles; partial samples preserved",
                self.source.prepared().triangles.len(),
                PHASES[self.phase],
                self.cycle.saturating_sub(WARMUP)
            );
            self.complete = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let screenshots = ctx.input(|i| {
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
        });
        for image in screenshots {
            let pixels: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let painted = PaintImage {
                width: image.width() as u32,
                height: image.height() as u32,
                pixels,
            };
            if let Err(e) = painted.save(&self.output.join(format!(
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
        if self.settle_frames > 0 {
            self.settle_frames -= 1;
            ctx.request_repaint();
            return;
        }
        let sample_key = if self.phase == 5 {
            PHASES[5 + self.subframe.min(4)]
        } else {
            PHASES[self.phase]
        };
        let measured = self.cycle >= WARMUP
            && (self.phase != 4 || self.subframe == 1)
            && (self.phase != 5 || self.subframe < 5);
        if measured {
            self.values.entry(sample_key).or_default().push(elapsed);
        }
        if self.phase == 0 && self.cycle == WARMUP {
            self.warm_cache_uploads = Some(self.editor.renderer.stats().mesh_uploads);
        }
        if self.phase < 5
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
                != self.revision
                || self.editor.renderer.stats().mesh_uploads != self.warm_cache_uploads.unwrap())
        {
            self.fail(
                ctx,
                "Navegar/selecionar reconstruiu ou reenviou geometria".into(),
            );
            return;
        }
        if self.phase == 2 && (self.editor.camera.yaw - self.initial_camera.yaw).abs() > 0.01 {
            self.orbit_verified = true;
        }
        if self.phase == 3
            && self
                .editor
                .camera
                .target
                .distance(self.initial_camera.target)
                > 0.001
        {
            self.pan_verified = true;
        }
        if self.phase == 4 && self.subframe == 1 {
            if self.editor.mesh_components().ids != vec![self.faces[self.cycle % 2]] {
                self.fail(
                    ctx,
                    format!("Clique no centro projetado não selecionou a face esperada: {:?}, esperada {}", self.editor.mesh_components().ids,self.faces[self.cycle % 2]),
                );
                return;
            }
            if measured {
                self.selections_verified += 1;
            }
        }
        if self.phase == 5 {
            let current = self
                .editor
                .scene()
                .entity(ENTITY)
                .unwrap()
                .mesh
                .as_ref()
                .unwrap();
            let valid = match self.subframe {
                0 => self.editor.qa_mesh_info().2 && current != &self.source,
                1 => {
                    !self.editor.qa_mesh_info().2
                        && current != &self.source
                        && self.editor.history.undo_len() == 1
                }
                2 | 4 => current == &self.source && self.editor.history.undo_len() == 0,
                3 => current != &self.source && self.editor.history.undo_len() == 1,
                _ => true,
            };
            if !valid {
                self.fail(
                    ctx,
                    format!("Operador/histórico não confirmou etapa {}", self.subframe),
                );
                return;
            }
            if self.subframe == 4 && measured {
                self.operations_verified += 1;
            }
        }
        if self.phase == 1 && self.cycle == 60 && !self.requested_screenshot {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested_screenshot = true;
        }
        let frames_per_cycle = if self.phase == 4 {
            2
        } else if self.phase == 5 {
            6
        } else {
            1
        };
        self.subframe += 1;
        if self.subframe == frames_per_cycle {
            self.subframe = 0;
            self.cycle += 1;
        }
        if self.cycle == WARMUP + SAMPLES {
            if self.phase < 5 {
                self.next_phase();
            } else {
                if let Err(e) = self.finish_load() {
                    self.fail(ctx, e);
                    return;
                }
                match self.divisions {
                    22 => self.reset_load(70),
                    70 => self.reset_load(158),
                    _ => {
                        self.report.lock().unwrap().done = true;
                        self.complete = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        ctx.request_repaint();
    }
}

#[test]
#[ignore = "Opt-in full editor CPU benchmark; inactive WGPU window and isolated RawInput"]
fn native_mesh_editor_measurements() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join("qa/v0.2.0/m7");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    eframe::run_native(
        "OXY — medição nativa de malhas",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(MeasuredEditor::new(cc, output, result)))),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let canonical_root = root.canonicalize().unwrap();
    let canonical_root = canonical_root.to_string_lossy();
    let safe_directory = format!(
        "safe.directory={}",
        canonical_root
            .strip_prefix(r"\\?\")
            .unwrap_or(&canonical_root)
            .replace('\\', "/")
    );
    let commit = std::process::Command::new("git")
        .args(["-c", &safe_directory, "rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    let compiler = std::process::Command::new(
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default())
            .join(".cargo/bin/rustc.exe"),
    )
    .args(["--version", "--verbose"])
    .output()
    .ok()
    .map(|r| String::from_utf8_lossy(&r.stdout).trim().to_owned());
    let json = json!({
        "commit":String::from_utf8_lossy(&commit.stdout).trim(),"working_tree":"M7 implementation; benchmark source stored in repository",
        "version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "cpu_identifier":std::env::var("PROCESSOR_IDENTIFIER").ok(),"gpu":report.gpu,
        "system":system_environment(&root),
        "compiler":compiler,
        "window_logical_points":[1440,900],"interface_scale":1.0,
        "method":"Release --locked opt-level=2. Deterministic Plane grids22/70/158; 20 warmups + 101 samples per phase/cycle. CPU around real Editor::update includes driver submission, excludes eframe tessellation, GPU completion and VSync. Redraw forced only in benchmark. Actual isolated RawInput orbit, pan, alternating face clicks, Shift+N preview, Enter, Undo, Redo, Undo. Fixture creation outside samples. One selected face, default visible-only overlays, all geometry retained. No GPU time, FPS or process RAM inference.",
        "limits":{"phase_seconds":45,"load_seconds":150},"rows":report.rows,"completed_all_samples":report.done,"limit_exceeded":report.limited,"error":report.error,
    });
    std::fs::write(
        root.join("benchmarks/results/v020-native-mesh-editor.json"),
        serde_json::to_vec_pretty(&json).unwrap(),
    )
    .unwrap();
    assert!(
        report.error.is_none(),
        "{}",
        report.error.as_deref().unwrap_or_default()
    );
    // A reported duration limit is a measurement result, never a successful completion claim.
    // Failed picks, lost edits, cache rebuilds or incorrect undo still fail this test above.
    assert!(report.done || report.limited);
}
