#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Standalone native host: the same simulation, renderer and UI as the editor.
#[cfg(all(test, target_os = "windows"))]
mod native_qa;
use egui::{Color32, Rect, Sense, Vec2};
use oxy_core::{
    audio,
    document::Project,
    persistence, random,
    runtime::{Runtime, RuntimeOptions},
    save::{self, SaveRequest, SaveSlot},
};
use oxy_render::{CameraState, GameUi, Renderer};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Instant,
};

fn main() -> eframe::Result {
    let project_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_owned))
                .unwrap_or_else(|| PathBuf::from("."))
                .join("data")
                .join(persistence::PROJECT_FILE)
        });
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — Jogo")
            .with_inner_size([1280., 720.])
            .with_min_inner_size([640., 360.]),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — Jogo",
        options,
        Box::new(move |cc| Ok(Box::new(Player::new(cc, project_path)))),
    )
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
    /// Where this game's progress lives and what it holds now; none when it cannot be saved.
    progress: Option<(PathBuf, SaveSlot)>,
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
            progress: None,
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
        // Every run draws differently; the low half of a v4 UUID has its fixed bits above the
        // 53 kept.
        let seed = uuid::Uuid::new_v4().as_u64_pair().1 & random::MAX_SEED;
        let mut notes = Vec::new();
        let loaded = persistence::load_project(&self.path)
            .map_err(String::from)
            .and_then(|project| {
                // The progress is read before the run, which starts from it.
                let (progress, problems) = load_progress(&project);
                notes = problems;
                let options = RuntimeOptions {
                    seed,
                    save: progress.as_ref().map(|(_, slot)| slot.clone()),
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
        if let Some(runtime) = &mut self.runtime
            && (!focused || escape)
        {
            runtime.set_paused(true)
        }
        if let Some(error) = self.error.clone() {
            oxy_render::input::release_cursor(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.);
                    ui.heading("OXY Engine — não foi possível abrir o jogo");
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
                let input = oxy_render::input::collect_game_input(
                    ctx,
                    &runtime.project().input_bindings,
                    !runtime.paused && focused,
                    runtime.wants_relative_mouse(),
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
                for request in std::mem::take(&mut runtime.sounds) {
                    let result = runtime
                        .project()
                        .asset(&request.asset)
                        .ok_or_else(|| format!("Recurso de áudio ausente: {}", request.asset))
                        .and_then(|asset| {
                            persistence::resolve_asset_path(&self.root, &asset.path)
                                .map_err(String::from)
                        })
                        .and_then(|path| audio::play_wav(&path, request.volume));
                    if let Err(error) = result {
                        new_diagnostics.push(error)
                    }
                }
                new_diagnostics.extend(renderer.take_errors());
                new_diagnostics.extend(self.game_ui.take_errors());
            });
        for error in new_diagnostics {
            self.push_diagnostic(error)
        }
        if let Some(runtime) = &mut self.runtime {
            let requests = std::mem::take(&mut runtime.save_requests);
            if !requests.is_empty()
                && let Err(error) = store_progress(&mut self.progress, runtime.project(), requests)
            {
                self.push_diagnostic(error);
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
                    if let Some(runtime) = &self.runtime {
                        for (action, key) in &runtime.project().input_bindings {
                            ui.label(format!(
                                "{}: {}",
                                input_action_label(runtime.project(), action),
                                oxy_render::labels::key(key)
                            ));
                        }
                    }
                    ui.label("Esc: pausar · F3: diagnóstico");
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
                                    ui.label(log);
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
/// The progress of `project` in the folder this process saves games in (see [`save_folder`]).
fn load_progress(project: &Project) -> (Option<(PathBuf, SaveSlot)>, Vec<String>) {
    let folder = save_folder(
        std::env::var_os("OXY_SAVE_DIR"),
        std::env::var_os("APPDATA"),
    );
    load_progress_in(folder, project)
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

#[cfg(all(test, target_os = "windows"))]
mod native_perf;
