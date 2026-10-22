#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Standalone native host: the same simulation, renderer and UI as the editor.
#[cfg(all(test, target_os = "windows"))]
mod native_qa;
use egui::{Color32, Rect, Sense, Vec2};
use oxy_audio::Mixer;
use oxy_core::{
    document::{GameSettings, Project},
    export, persistence, random,
    runtime::{Runtime, RuntimeOptions},
    save::{self, SaveRequest, SaveSlot},
    script,
};
use oxy_render::{CameraState, GameUi, Renderer};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// The window's title when the project cannot be read.
const DEFAULT_TITLE: &str = "OXY Engine — Jogo";
/// Frames the window check (`--verificar --janela`) draws before it reports.
const WINDOW_CHECK_FRAMES: u32 = 60;

fn main() -> ExitCode {
    let launch = match Launch::parse(std::env::args_os().skip(1)) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let (path, check) = match launch {
        Launch::Verify {
            report,
            window: false,
        } => return verify_exported(report.as_deref()),
        Launch::Verify {
            report,
            window: true,
        } => (exported_project(), Some(WindowCheck::new(report))),
        Launch::Play(path) => (path.unwrap_or_else(exported_project), None),
    };
    // The window opens as the game sets it, before the project loads.
    let (title, game) = persistence::read_game_settings(&path)
        .unwrap_or_else(|| (DEFAULT_TITLE.to_owned(), GameSettings::default()));
    let passed = check.as_ref().map(|check| check.passed.clone());
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: viewport(&title, &game),
        ..Default::default()
    };
    let result = eframe::run_native(
        DEFAULT_TITLE,
        options,
        Box::new(move |cc| {
            let mut player = Player::new(cc, path);
            player.check = check;
            Ok(Box::new(player))
        }),
    );
    match (result, passed) {
        (Err(error), _) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
        (Ok(()), Some(passed)) if !passed.load(Ordering::Relaxed) => ExitCode::FAILURE,
        (Ok(()), _) => ExitCode::SUCCESS,
    }
}

/// What the player was asked to do.
#[derive(Debug, PartialEq)]
enum Launch {
    /// Play the project at the path, or the exported game next to the executable.
    Play(Option<PathBuf>),
    /// `--verificar [relatório] [--janela]`: check the exported game next to the executable,
    /// without a window, and write the report (to the output when no file is given); with
    /// `--janela`, open its window, draw some frames and report how the window opened.
    Verify {
        report: Option<PathBuf>,
        window: bool,
    },
}
impl Launch {
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let Some(first) = args.next() else {
            return Ok(Self::Play(None));
        };
        if first != "--verificar" {
            return Ok(Self::Play(Some(first.into())));
        }
        let mut report = None;
        let mut window = false;
        for arg in args {
            if arg == "--janela" {
                window = true;
            } else if report.is_none() && !arg.to_string_lossy().starts_with("--") {
                report = Some(PathBuf::from(arg));
            } else {
                return Err(format!(
                    "Argumento inesperado: {}. Use --verificar [relatório.json] [--janela].",
                    arg.to_string_lossy()
                ));
            }
        }
        Ok(Self::Verify { report, window })
    }
}

/// The project of the exported game: data\project.oxy.json next to the executable.
fn exported_project() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_owned))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
        .join(persistence::PROJECT_FILE)
}

/// The window as the game sets it: its title, size and full screen, with the OXY icon.
fn viewport(title: &str, game: &GameSettings) -> egui::ViewportBuilder {
    let [width, height] = game.window.map(|side| side as f32);
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_icon(oxy_render::branding::oxy_icon())
        .with_inner_size([width, height])
        .with_min_inner_size([width.min(640.), height.min(360.)])
        .with_fullscreen(game.fullscreen)
}

/// `--verificar` without a window: checks the exported game as the export does and writes the
/// report. Fails when the game does not pass or the report cannot be written.
fn verify_exported(report: Option<&Path>) -> ExitCode {
    let data = exported_project()
        .parent()
        .map(Path::to_owned)
        .unwrap_or_default();
    let checked = export::verify(&data, export::VERIFY_SECONDS);
    let text = serde_json::to_string_pretty(&checked).unwrap_or_default();
    let written = match report {
        Some(path) => std::fs::write(path, text).is_ok(),
        None => {
            println!("{text}");
            true
        }
    };
    if checked.ok && written {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `--verificar --janela`: the game in its real window for some frames, then a report of how
/// the window opened.
struct WindowCheck {
    report: Option<PathBuf>,
    frames: u32,
    passed: Arc<AtomicBool>,
}
impl WindowCheck {
    fn new(report: Option<PathBuf>) -> Self {
        Self {
            report,
            frames: 0,
            passed: Arc::new(AtomicBool::new(false)),
        }
    }
}

struct Player {
    path: PathBuf,
    root: PathBuf,
    runtime: Option<Runtime>,
    renderer: Option<Renderer>,
    game_ui: GameUi,
    error: Option<String>,
    diagnostics: Vec<String>,
    last_update: Instant,
    last_size: Vec2,
    show_diagnostics: bool,
    /// The folder games keep their progress in (see [`save_folder`]), decided as the player
    /// starts.
    saves: Option<PathBuf>,
    /// Where this game's progress lives and what it holds now; none when it cannot be saved.
    progress: Option<(PathBuf, SaveSlot)>,
    mixer: Mixer,
    check: Option<WindowCheck>,
}
impl Player {
    fn new(cc: &eframe::CreationContext<'_>, path: PathBuf) -> Self {
        oxy_render::theme::apply(&cc.egui_ctx);
        let path = if path.is_dir() {
            path.join(persistence::PROJECT_FILE)
        } else {
            path
        };
        let root = path.parent().unwrap_or(Path::new(".")).to_owned();
        let mut player = Self {
            path,
            root,
            runtime: None,
            renderer: cc.wgpu_render_state.as_ref().map(Renderer::new),
            game_ui: GameUi::new(),
            error: None,
            diagnostics: Vec::new(),
            last_update: Instant::now(),
            last_size: Vec2::ZERO,
            show_diagnostics: false,
            saves: save_folder(
                std::env::var_os("OXY_SAVE_DIR"),
                std::env::var_os("APPDATA"),
            ),
            progress: None,
            mixer: Mixer::new(),
            check: None,
        };
        if let Some(renderer) = &mut player.renderer {
            renderer.show_grid = false;
        } else {
            player.error = Some(
                "O renderizador wgpu não foi inicializado. Verifique o driver gráfico.".into(),
            );
            return player;
        }
        player.reload();
        player
    }
    fn reload(&mut self) {
        // Nothing of the old run goes on playing.
        self.mixer.stop_all();
        // Every run draws differently; the low half of a v4 UUID has its fixed bits above the
        // 53 kept.
        let seed = uuid::Uuid::new_v4().as_u64_pair().1 & random::MAX_SEED;
        let mut notes = Vec::new();
        let saves = self.saves.clone();
        let root = self.root.clone();
        let loaded = persistence::load_project(&self.path)
            .map_err(String::from)
            .and_then(|project| {
                // The game's scripts, the tree of its project's folder (data in an exported
                // game); none is read when no object attaches one.
                let scripts = if script::any_attached(&project) {
                    persistence::load_scripts(&root).map_err(String::from)?
                } else {
                    Default::default()
                };
                // The progress is read before the run, which starts from it.
                let (progress, problems) = load_progress_in(saves, &project);
                notes = problems;
                notes.extend(
                    script::absent_by_file(&project, &scripts)
                        .into_iter()
                        .map(|(line, _)| line),
                );
                let options = RuntimeOptions {
                    seed,
                    save: progress.as_ref().map(|(_, slot)| slot.clone()),
                    scripts: Arc::new(scripts),
                    ..Default::default()
                };
                Runtime::with_options(&project, &project.start_scene, options)
                    .map(|runtime| (runtime, progress))
            });
        match loaded {
            Ok((runtime, progress)) => {
                self.error = None;
                self.diagnostics = notes;
                self.progress = progress;
                for (action, key) in &runtime.project().input_bindings {
                    if let Err(error) = oxy_core::input_actions::check_key(key) {
                        self.diagnostics.push(format!("Ação {action}: {error}"));
                    }
                }
                let mut runtime = runtime;
                if runtime.wants_relative_mouse() {
                    runtime.set_paused(true);
                }
                self.runtime = Some(runtime);
                if let Some(renderer) = &mut self.renderer {
                    renderer.clear_textures();
                }
                self.game_ui = GameUi::new();
                self.last_update = Instant::now();
            }
            Err(error) => {
                self.error = Some(error);
                if let Some(runtime) = &mut self.runtime {
                    runtime.set_paused(true)
                }
            }
        }
    }
    /// Counts the frames of `--verificar --janela` and, after enough of them or once the game
    /// fails to open, reports how the window opened and closes it.
    fn window_check(&mut self, ctx: &egui::Context) {
        let Some(check) = &mut self.check else {
            return;
        };
        check.frames += 1;
        if check.frames < WINDOW_CHECK_FRAMES && self.error.is_none() {
            return;
        }
        let info = ctx.input(|i| i.viewport().clone());
        let errors: Vec<&String> = self.error.iter().chain(&self.diagnostics).collect();
        let report = serde_json::json!({
            "ok": errors.is_empty(),
            "engine": export::engine_version(),
            "title": info.title,
            "inner_size": info.inner_rect.map(|rect| [rect.width(), rect.height()]),
            "fullscreen": info.fullscreen,
            "frames": check.frames,
            "errors": errors,
        });
        let text = serde_json::to_string_pretty(&report).unwrap_or_default();
        let written = match &check.report {
            Some(path) => std::fs::write(path, text).is_ok(),
            None => {
                println!("{text}");
                true
            }
        };
        check
            .passed
            .store(errors.is_empty() && written, Ordering::Relaxed);
        self.check = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn push_diagnostic(&mut self, error: String) {
        if self.diagnostics.last() != Some(&error) {
            self.diagnostics.push(error);
        }
        if self.diagnostics.len() > 100 {
            self.diagnostics.drain(..20);
        }
    }
}
impl eframe::App for Player {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let elapsed = (now - self.last_update).as_secs_f32();
        self.last_update = now;
        let focused = ctx.input(|i| i.focused);
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if ctx.input(|i| i.key_pressed(egui::Key::F3)) {
            self.show_diagnostics = !self.show_diagnostics;
        }
        // Alt+Enter switches between the window and full screen, once per press (held, it would
        // repeat); the game never sees it.
        let toggle = ctx.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if modifiers.alt
                )
            })
        });
        if toggle {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
        }
        self.window_check(ctx);
        // Losing focus always pauses the engine, with its window over whatever the game shows.
        // Esc is the game's own pause when the scene has a pause menu, else the engine's.
        if let Some(runtime) = &mut self.runtime {
            if !focused {
                runtime.set_paused(true);
            } else if escape && !runtime.paused {
                if runtime.has_pause_menu() {
                    runtime.toggle_game_pause();
                } else {
                    runtime.set_paused(true);
                }
            }
        }
        if let Some(error) = self.error.clone() {
            oxy_render::input::release_cursor(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.);
                    ui.heading("Não foi possível abrir o jogo");
                    ui.add_space(20.);
                    ui.colored_label(Color32::from_rgb(255, 155, 140), error);
                    ui.label(self.path.display().to_string());
                    ui.add_space(16.);
                    if ui.button("Tentar novamente").clicked() {
                        self.reload();
                    }
                    if ui.button("Fechar").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            return;
        }
        let mut new_diagnostics = Vec::new();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(16, 19, 25)))
            .show(ctx, |ui| {
                let Some(runtime) = &mut self.runtime else {
                    return;
                };
                let (rect, response) = ui
                    .allocate_exact_size(ui.available_size().max(Vec2::splat(1.)), Sense::click());
                let resized = (self.last_size - rect.size()).length() > 0.5;
                self.last_size = rect.size();
                // A press on one of the game's buttons is the button's, not a mouse action.
                let input = oxy_render::input::collect_game_input_where(
                    ctx,
                    &runtime.project().input_bindings,
                    !runtime.paused && focused,
                    runtime.wants_relative_mouse(),
                    |p| {
                        rect.contains(p)
                            && oxy_render::game_ui::button_at(runtime.scene(), rect, p).is_none()
                    },
                );
                runtime.set_viewport_aspect(rect.width() / rect.height());
                runtime.advance(if resized { 0. } else { elapsed }, &input);
                let Some(rs) = frame.wgpu_render_state() else {
                    return;
                };
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                let camera = CameraState::for_runtime(runtime);
                let scale = ctx.pixels_per_point();
                let size = [
                    (rect.width() * scale).round().max(1.) as u32,
                    (rect.height() * scale).round().max(1.) as u32,
                ];
                let texture = renderer.render(
                    rs,
                    runtime.project(),
                    runtime.scene(),
                    &self.root,
                    &camera,
                    size,
                    None,
                    false,
                );
                ui.painter().image(
                    texture,
                    rect,
                    Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::WHITE,
                );
                let clicks =
                    self.game_ui
                        .draw(ui, runtime.project(), runtime.scene(), &self.root, rect);
                if !runtime.paused && oxy_render::input::accepts_game_click(ctx) {
                    if clicks.is_empty()
                        && response.clicked()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        let local = (pointer - rect.min) * scale;
                        if let Some(hit) =
                            oxy_render::pick(runtime.scene(), &camera, size, [local.x, local.y])
                        {
                            runtime.click(&hit.entity);
                        }
                    }
                    for id in clicks {
                        runtime.click(&id);
                    }
                }
                // Also while paused: the pauses freeze what plays.
                self.mixer.update(runtime, &self.root);
                new_diagnostics.extend(self.mixer.take_errors());
                new_diagnostics.extend(renderer.take_errors());
                new_diagnostics.extend(self.game_ui.take_errors());
            });
        for error in new_diagnostics {
            self.push_diagnostic(error)
        }
        if let Some(runtime) = &mut self.runtime {
            let requests = std::mem::take(&mut runtime.save_requests);
            let quit = runtime.quit_requested;
            if !requests.is_empty()
                && let Err(error) = store_progress(&mut self.progress, runtime.project(), requests)
            {
                self.push_diagnostic(error);
            }
            // Sair do jogo closes the window, after what the same frame saved.
            if quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.runtime.as_ref().is_some_and(|runtime| runtime.paused) {
            egui::Window::new("Jogo pausado")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("A entrada está liberada. Retome para continuar.");
                    if ui.button("Retomar").clicked() {
                        if let Some(runtime) = &mut self.runtime {
                            runtime.set_paused(false)
                        }
                        self.last_update = Instant::now();
                    }
                    ui.separator();
                    ui.label("Controles configurados no projeto:");
                    let mut menu = false;
                    if let Some(runtime) = &self.runtime {
                        for (action, key) in &runtime.project().input_bindings {
                            ui.label(format!(
                                "{}: {}",
                                input_action_label(runtime.project(), action),
                                oxy_render::labels::key(key)
                            ));
                        }
                        menu = runtime.has_pause_menu();
                    }
                    ui.label(if menu {
                        "Esc: menu de pausa do jogo · Alt+Enter: tela cheia · F3: diagnóstico"
                    } else {
                        "Esc: pausar · Alt+Enter: tela cheia · F3: diagnóstico"
                    });
                    if ui.button("Sair").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
        }
        if self.show_diagnostics || !self.diagnostics.is_empty() {
            egui::Window::new("Diagnóstico do jogo")
                .open(&mut self.show_diagnostics)
                .default_size([550., 220.])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for error in &self.diagnostics {
                                ui.colored_label(Color32::from_rgb(255, 165, 140), error);
                            }
                            if let Some(runtime) = &self.runtime {
                                for log in &runtime.logs {
                                    ui.label(&log.text);
                                }
                            }
                        });
                });
            if !self.diagnostics.is_empty() && !self.show_diagnostics {
                egui::Area::new(egui::Id::new("player_errors"))
                    .anchor(egui::Align2::RIGHT_BOTTOM, [-10., -10.])
                    .show(ctx, |ui| {
                        if ui
                            .button(format!("{} avisos · F3", self.diagnostics.len()))
                            .clicked()
                        {
                            self.show_diagnostics = true;
                        }
                    });
            }
        }
        if self.runtime.as_ref().is_some_and(|runtime| !runtime.paused) {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// The folder games keep their progress in: `$OXY_SAVE_DIR` when set (tests and QA point it to
/// a temporary folder), else `%APPDATA%\OXY Engine\Jogos`, which an exported game can write
/// even when its own folder is read-only. Each game has its own folder inside (see
/// [`save::path`]).
fn save_folder(override_dir: Option<OsString>, appdata: Option<OsString>) -> Option<PathBuf> {
    let set = |dir: &OsString| !dir.is_empty();
    override_dir.filter(set).map(PathBuf::from).or_else(|| {
        appdata
            .filter(set)
            .map(|dir| PathBuf::from(dir).join("OXY Engine").join("Jogos"))
    })
}
/// The progress of `project` under `folder`: where it lives and what it holds, with what could
/// not be used, for the diagnostics. None when there is nowhere to save it.
fn load_progress_in(
    folder: Option<PathBuf>,
    project: &Project,
) -> (Option<(PathBuf, SaveSlot)>, Vec<String>) {
    let path = folder
        .ok_or_else(|| {
            "A pasta %APPDATA% não foi encontrada; o progresso não será salvo".to_owned()
        })
        .and_then(|folder| save::path(&folder, &project.id));
    match path {
        Ok(path) => {
            let (slot, problems) = save::load(project, &path);
            (Some((path, slot)), problems)
        }
        Err(error) => (None, vec![error]),
    }
}
/// Carries out the progress requests of a frame, in order, with one disk operation for all of
/// them: the last state wins. A save that may not be written stays in memory, as the
/// diagnostics said when it was read.
fn store_progress(
    progress: &mut Option<(PathBuf, SaveSlot)>,
    project: &Project,
    requests: Vec<SaveRequest>,
) -> Result<(), String> {
    let Some((path, slot)) = progress else {
        return Ok(());
    };
    for request in requests {
        slot.apply(request);
    }
    if !slot.writable {
        return Ok(());
    }
    save::store(path, project, slot)
}

fn input_action_label<'a>(project: &'a oxy_core::document::Project, action: &'a str) -> &'a str {
    project
        .input_labels
        .get(action)
        .map_or(action, String::as_str)
}

#[test]
fn player_controls_show_labels_and_preserve_legacy_actions() {
    let mut project = oxy_core::document::Project::new("Controles");
    let id = "02000000-0000-4000-8000-000000000001";
    project
        .input_labels
        .insert(id.into(), "Ataque principal".into());
    assert_eq!(input_action_label(&project, id), "Ataque principal");
    assert_eq!(input_action_label(&project, "pular"), "pular");
}

#[test]
fn progress_lives_under_oxy_save_dir_or_the_roaming_app_data() {
    let project = Project::new("Progresso");
    let appdata = OsString::from("C:\\Users\\Ana\\AppData\\Roaming");
    let file = |folder: PathBuf| save::path(&folder, &project.id).unwrap();
    // OXY_SAVE_DIR wins, so tests never reach the real folder.
    let temp = std::env::temp_dir().join("oxy-progresso");
    let chosen = save_folder(Some(temp.clone().into()), Some(appdata.clone())).unwrap();
    assert_eq!(file(chosen), temp.join(&project.id).join("progresso.json"));
    let roaming = save_folder(Some(OsString::new()), Some(appdata)).unwrap();
    assert_eq!(
        file(roaming),
        Path::new("C:\\Users\\Ana\\AppData\\Roaming")
            .join("OXY Engine")
            .join("Jogos")
            .join(&project.id)
            .join("progresso.json")
    );
    assert_eq!(save_folder(None, None), None);
}

#[test]
fn progress_is_stored_once_per_frame_and_starts_the_next_run() {
    use oxy_core::{
        document::{Value, new_id},
        variables,
    };
    let mut project = Project::new("Progresso");
    let record = variables::create(&mut project, "Recorde", Value::Number(0.), true).unwrap();
    let write = |value: f64| {
        SaveRequest::Write([((record.clone(), String::new()), Value::Number(value))].into())
    };
    let folder = std::env::temp_dir().join(format!("oxy-player-progress-{}", new_id()));
    // A fresh install: nothing read, nothing to explain.
    let (mut progress, problems) = load_progress_in(Some(folder.clone()), &project);
    assert!(problems.is_empty(), "{problems:?}");
    let path = progress.as_ref().unwrap().0.clone();
    assert_eq!(path, folder.join(&project.id).join("progresso.json"));
    assert!(!path.exists());
    // A frame's requests, in order, in one disk operation: the last state lands, in one file.
    store_progress(
        &mut progress,
        &project,
        vec![write(1.), SaveRequest::Clear, write(5.)],
    )
    .unwrap();
    let files: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, ["progresso.json"]);
    // Relaunched, the game starts from it, as reload does.
    let (relaunched, problems) = load_progress_in(Some(folder.clone()), &project);
    assert!(problems.is_empty(), "{problems:?}");
    let options = RuntimeOptions {
        save: relaunched.map(|(_, slot)| slot),
        ..Default::default()
    };
    let runtime = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    assert_eq!(runtime.variable(&record, ""), Some(&Value::Number(5.)));
    assert!(runtime.save_slot().exists);
    // Apagar progresso removes the file.
    store_progress(&mut progress, &project, vec![SaveRequest::Clear]).unwrap();
    assert!(!path.exists());
    // A newer build's file is never written over nor removed.
    let newer = format!("{{\"oxy_save\": 2, \"game\": \"{}\"}}", project.id);
    std::fs::write(&path, &newer).unwrap();
    let (mut progress, problems) = load_progress_in(Some(folder.clone()), &project);
    assert_eq!(problems.len(), 1, "{problems:?}");
    store_progress(&mut progress, &project, vec![write(9.)]).unwrap();
    store_progress(&mut progress, &project, vec![SaveRequest::Clear]).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
    // Without a folder nothing is saved, and the diagnostics say so.
    let (mut nowhere, problems) = load_progress_in(None, &project);
    assert!(
        nowhere.is_none() && problems[0].contains("%APPDATA%"),
        "{problems:?}"
    );
    store_progress(&mut nowhere, &project, vec![write(2.)]).unwrap();
    std::fs::remove_dir_all(folder).unwrap();
}

/// A player of the project saved in `folder`, without a window or sound, keeping its progress
/// there.
#[cfg(test)]
fn windowless(folder: &Path) -> Player {
    Player {
        path: folder.join(persistence::PROJECT_FILE),
        root: folder.to_owned(),
        runtime: None,
        renderer: None,
        game_ui: GameUi::new(),
        error: None,
        diagnostics: Vec::new(),
        last_update: Instant::now(),
        last_size: Vec2::ZERO,
        show_diagnostics: false,
        saves: Some(folder.join("progresso")),
        progress: None,
        mixer: Mixer::silent(),
        check: None,
    }
}

#[test]
fn a_reload_runs_the_scripts_next_to_the_project() {
    use oxy_core::{
        document::{Entity, Value, new_id},
        script::ScriptRef,
    };
    let folder = std::env::temp_dir().join(format!("oxy-player-scripts-{}", new_id()));
    std::fs::create_dir_all(folder.join("scripts")).unwrap();
    std::fs::write(
        folder.join("scripts/contador.luau"),
        "local M = {}\nfunction M.on_start(self) self.attr.Vezes += 1 end\nreturn M\n",
    )
    .unwrap();
    let mut project = Project::new("Scripts");
    let mut counter = Entity::new("Contador", None);
    counter.attributes.insert("Vezes".into(), Value::Number(0.));
    counter.scripts = vec![ScriptRef::new("scripts/contador.luau")];
    let id = counter.id.clone();
    project.scenes[0].entities.push(counter);
    persistence::save_project(&folder.join(persistence::PROJECT_FILE), &project).unwrap();
    let mut player = windowless(&folder);
    let times = |player: &Player| {
        player
            .runtime
            .as_ref()
            .unwrap()
            .scene()
            .entity(&id)
            .unwrap()
            .attributes["Vezes"]
            .clone()
    };
    player.reload();
    assert_eq!(player.error, None);
    assert!(player.diagnostics.is_empty(), "{:?}", player.diagnostics);
    assert_eq!(times(&player), Value::Number(1.));
    // A script taken away: the game opens and runs without it, and the diagnostics name it.
    std::fs::remove_file(folder.join("scripts/contador.luau")).unwrap();
    player.reload();
    assert_eq!(player.error, None);
    assert_eq!(
        player.diagnostics,
        ["Contador: o script scripts/contador.luau não existe na pasta do projeto"]
    );
    assert_eq!(times(&player), Value::Number(0.));
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_reload_silences_the_old_run() {
    use oxy_core::{
        document::{Asset, AssetKind, Entity, Value, new_id},
        graph::{Edge, Node},
    };
    let folder = std::env::temp_dir().join(format!("oxy-player-sound-{}", new_id()));
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    // Five seconds of sound, which the scene plays as it starts.
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(folder.join("assets/bip.wav"), spec).unwrap();
    for _ in 0..40_000 {
        wav.write_sample(16384i16).unwrap();
    }
    wav.finalize().unwrap();
    let mut project = Project::new("Som");
    let asset = new_id();
    project.assets.push(Asset {
        id: asset.clone(),
        name: "Bip".into(),
        path: "assets/bip.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let mut speaker = Entity::new("Alto-falante", None);
    let start = Node::new("event.scene_start", [0.; 2]);
    let mut sound = Node::new("action.sound", [200., 0.]);
    sound.params.insert("asset".into(), Value::Text(asset));
    speaker.graph.edges.push(Edge {
        from_node: start.id.clone(),
        from_port: "exec".into(),
        to_node: sound.id.clone(),
        to_port: "exec".into(),
    });
    speaker.graph.nodes.extend([start, sound]);
    project.scenes[0].entities.push(speaker);
    let path = folder.join(persistence::PROJECT_FILE);
    persistence::save_project(&path, &project).unwrap();
    let mut player = windowless(&folder);
    player.reload();
    assert_eq!(player.error, None);
    // A frame plays it.
    let runtime = player.runtime.as_mut().unwrap();
    player.mixer.update(runtime, &player.root);
    assert_eq!(player.mixer.voice_count(), 1);
    // Starting over leaves nothing of the old run playing.
    player.reload();
    assert_eq!(player.mixer.voice_count(), 0);
    assert!(player.mixer.take_errors().is_empty());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_player_plays_a_project_or_checks_the_exported_game() {
    let parse = |args: &[&str]| Launch::parse(args.iter().map(OsString::from));
    assert_eq!(parse(&[]), Ok(Launch::Play(None)));
    assert_eq!(
        parse(&["C:\\Jogos\\Corrida\\project.oxy.json"]),
        Ok(Launch::Play(Some(
            "C:\\Jogos\\Corrida\\project.oxy.json".into()
        )))
    );
    assert_eq!(
        parse(&["--verificar"]),
        Ok(Launch::Verify {
            report: None,
            window: false
        })
    );
    assert_eq!(
        parse(&["--verificar", "relatorio.json"]),
        Ok(Launch::Verify {
            report: Some("relatorio.json".into()),
            window: false
        })
    );
    assert_eq!(
        parse(&["--verificar", "--janela", "relatorio.json"]),
        Ok(Launch::Verify {
            report: Some("relatorio.json".into()),
            window: true
        })
    );
    for args in [
        &["--verificar", "a.json", "b.json"][..],
        &["--verificar", "--outro"],
    ] {
        let error = parse(args).unwrap_err();
        assert!(error.contains("Argumento inesperado"), "{error}");
    }
}

#[test]
fn the_window_opens_as_the_game_sets_it() {
    let game = GameSettings {
        title: "Corrida".into(),
        window: [800, 300],
        fullscreen: true,
    };
    let built = viewport("Corrida Noturna", &game);
    assert_eq!(built.title.as_deref(), Some("Corrida Noturna"));
    assert_eq!(built.inner_size, Some(Vec2::new(800., 300.)));
    assert_eq!(built.min_inner_size, Some(Vec2::new(640., 300.)));
    assert_eq!(built.fullscreen, Some(true));
    assert!(built.icon.is_some_and(|icon| icon.width > 0));
    let built = viewport(DEFAULT_TITLE, &GameSettings::default());
    assert_eq!(built.inner_size, Some(Vec2::new(1280., 720.)));
    assert_eq!(built.min_inner_size, Some(Vec2::new(640., 360.)));
    assert_eq!(built.fullscreen, Some(false));
}

#[cfg(all(test, target_os = "windows"))]
mod native_perf;
