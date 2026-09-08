#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Standalone native host: the same simulation, renderer and UI as the editor.
#[cfg(all(test, target_os = "windows"))]
mod native_qa;
use egui::{Color32, Rect, Sense, Vec2};
use oxy_core::{audio, persistence, runtime::Runtime};
use oxy_render::{CameraState, GameUi, Renderer};
use std::{
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
}
impl Player {
    fn new(cc: &eframe::CreationContext<'_>, path: PathBuf) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
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
        let loaded = persistence::load_project(&self.path)
            .and_then(|project| Runtime::new(&project, &project.start_scene));
        match loaded {
            Ok(runtime) => {
                self.error = None;
                self.diagnostics.clear();
                for (action, key) in &runtime.project().input_bindings {
                    if egui::Key::from_name(key).is_none() {
                        self.diagnostics
                            .push(format!("Ação {action}: tecla desconhecida '{key}'"));
                    }
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
                let input = oxy_render::input::collect_input(
                    ctx,
                    &runtime.project().input_bindings,
                    !runtime.paused && focused,
                );
                runtime.advance(if resized { 0. } else { elapsed }, &input);
                let Some(rs) = frame.wgpu_render_state() else {
                    return;
                };
                let Some(renderer) = &mut self.renderer else {
                    return;
                };
                let camera = CameraState::for_game(runtime.scene());
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
                if !runtime.paused {
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
                        .and_then(|asset| persistence::resolve_asset_path(&self.root, &asset.path))
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
                            ui.label(format!("{action}: {}", oxy_render::labels::key(key)));
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

#[cfg(all(test, target_os = "windows"))]
mod native_perf;
