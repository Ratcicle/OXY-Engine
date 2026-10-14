//! Exportar jogo: how the exported game opens, where it goes and what the export will find,
//! then the export on a worker thread while the editor goes on, with its result. Agents export
//! the same way through their channel, answered when the export is over. Both export the
//! project as it is in memory, painted images not saved yet included; the dialog only commits
//! the settings it shows, as one undo step.
use super::*;
use crate::{
    icons::{self, Icon},
    theme, widgets,
};
use egui::{Align, Layout, RichText};
use oxy_core::{
    command::{self, Command},
    export::{self, ExportCheck, ExportError, ExportReport, ExportRequest, ExportStage, Plan},
};
use serde_json::Value as Json;
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc,
    time::Duration,
};

const UNSAVED: &str = "Alterações não salvas entram na exportação; o projeto continua não salvo.";
const KEPT: &str = "Nada foi alterado no destino: a exportação anterior continua intacta.";
const AGENT_EXPORTING: &str =
    "Um agente de IA está exportando este jogo; espere a exportação dele terminar.";
const FAILED: &str = "Não foi possível exportar o jogo";
const IN_USE: &str = "Não foi possível substituir a exportação anterior";
const ERROR_TEXT: Color32 = Color32::from_rgb(244, 180, 180);
const WARNING_TEXT: Color32 = Color32::from_rgb(232, 210, 160);
const OK_TEXT: Color32 = Color32::from_rgb(196, 196, 202);

/// The export dialog and an agent's export, apart from the rest of the editor.
#[derive(Default)]
pub(super) struct ExportUi {
    dialog: Option<ExportDialog>,
    /// An agent's export and where its answer goes.
    agent: Option<(Job, mpsc::Sender<Result<Json, String>>)>,
}
impl ExportUi {
    pub(super) fn is_open(&self) -> bool {
        self.dialog.is_some()
    }
    /// Whether an export runs, the person's or an agent's.
    pub(super) fn running(&self) -> bool {
        self.agent.is_some()
            || self
                .dialog
                .as_ref()
                .is_some_and(|dialog| matches!(dialog.state, State::Running { .. }))
    }
}

struct ExportDialog {
    /// The game's own title as typed; empty for the project's name.
    title: String,
    start_scene: Id,
    window: [u32; 2],
    fullscreen: bool,
    destination: String,
    /// What the export's check found in the project when the dialog opened.
    checked: ExportCheck,
    player: PathBuf,
    /// Whether the player was there when the dialog opened.
    player_found: bool,
    dirty: bool,
    /// Where the game would go for the destination and title last looked at.
    planned: Option<Planned>,
    /// Whether Exportar could be clicked in the last frame.
    ready: bool,
    state: State,
}
struct Planned {
    destination: String,
    title: String,
    /// None without a destination.
    plan: Option<Result<Plan, String>>,
}
enum State {
    Configure,
    Running {
        job: Job,
        /// Whether an earlier export of the game is being replaced.
        replacing: bool,
    },
    Done {
        report: Box<ExportReport>,
        /// What the export found to say that the Conferência did not.
        warnings: Vec<String>,
    },
    Failed {
        title: &'static str,
        message: String,
    },
}

/// What the dialog was asked to do this frame.
enum Step {
    Close,
    Choose,
    Export,
    OpenFolder,
    Play,
    Back,
}

/// What an export of the open project takes from the editor, gathered on the UI thread.
struct Prepared {
    project: Project,
    root: PathBuf,
    unsaved: HashMap<Id, Vec<u8>>,
    player: PathBuf,
    protected: Vec<PathBuf>,
    destination: PathBuf,
    replace: bool,
}

enum Progress {
    Stage(ExportStage),
    Done(Box<Result<ExportReport, ExportError>>),
}
/// An export on its worker thread, which wakes the editor at each stage and when it is over.
pub(super) struct Job {
    messages: mpsc::Receiver<Progress>,
    stage: ExportStage,
}
/// Native QA holds the next export at its start, to see the dialog while it runs.
#[cfg(test)]
static QA_GATE: std::sync::Mutex<Option<mpsc::Receiver<()>>> = std::sync::Mutex::new(None);

impl Job {
    fn start(prepared: Prepared, ctx: &egui::Context) -> Result<Self, String> {
        let (sender, messages) = mpsc::channel();
        let ctx = ctx.clone();
        #[cfg(test)]
        let gate = QA_GATE.lock().ok().and_then(|mut gate| gate.take());
        std::thread::Builder::new()
            .name("oxy-export".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some(gate) = gate {
                    let _ = gate.recv();
                }
                let stages = sender.clone();
                let wake = ctx.clone();
                let request = ExportRequest {
                    project: &prepared.project,
                    source_root: Some(&prepared.root),
                    unsaved_png: &prepared.unsaved,
                    player: &prepared.player,
                    destination: &prepared.destination,
                    replace: prepared.replace,
                    protected: &prepared.protected,
                };
                let result = export::export_reporting(&request, |stage| {
                    let _ = stages.send(Progress::Stage(stage));
                    wake.request_repaint();
                });
                let _ = sender.send(Progress::Done(Box::new(result)));
                ctx.request_repaint();
            })
            .map_err(|e| format!("Não foi possível começar a exportação: {e}"))?;
        Ok(Self {
            messages,
            stage: ExportStage::Copying,
        })
    }
    /// Takes the stages reached; the result once the export is over.
    fn poll(&mut self) -> Option<Result<ExportReport, ExportError>> {
        loop {
            match self.messages.try_recv() {
                Ok(Progress::Stage(stage)) => self.stage = stage,
                Ok(Progress::Done(result)) => return Some(*result),
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Some(Err(ExportError::Io {
                        context: "A exportação parou antes de terminar".into(),
                        source: std::io::Error::other("erro interno da OXY Engine"),
                    }));
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Ok,
    Warn,
    Error,
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The game's title as the export takes it: the one typed, else the project's name.
fn effective_title<'a>(typed: &'a str, project: &'a str) -> &'a str {
    match typed.trim() {
        "" => project.trim(),
        title => title,
    }
}

/// The export refuses a player of another version, so the editor's is the one named.
fn player_found() -> String {
    format!(
        "Player da OXY Engine {} encontrado ao lado do editor.",
        export::engine_version()
    )
}
fn player_missing() -> String {
    format!(
        "Player da OXY ({}) não encontrado ao lado do editor. Extraia o ZIP inteiro da OXY Engine.",
        export::PLAYER_EXE
    )
}

/// The Conferência: whether the project can be exported (its errors, the settings' own),
/// unsaved changes, what the game will take otherwise than it may seem, the player and an
/// agent's export under way.
fn check_rows(
    checked: &ExportCheck,
    settings: Option<&str>,
    contents: (usize, usize),
    dirty: bool,
    player: bool,
    agent: bool,
) -> Vec<(Mark, String)> {
    let errors: Vec<&str> = checked
        .errors
        .iter()
        .map(String::as_str)
        .chain(settings)
        .collect();
    let mut rows = Vec::new();
    if errors.is_empty() {
        let (scenes, assets) = contents;
        rows.push((
            Mark::Ok,
            format!(
                "Projeto válido: {}, {}.",
                count(scenes, "cena", "cenas"),
                count(assets, "recurso", "recursos")
            ),
        ));
    }
    rows.extend(
        errors
            .into_iter()
            .map(|error| (Mark::Error, error.to_owned())),
    );
    if dirty {
        rows.push((Mark::Warn, UNSAVED.into()));
    }
    rows.extend(
        checked
            .warnings
            .iter()
            .map(|warning| (Mark::Warn, warning.clone())),
    );
    rows.push(if player {
        (Mark::Ok, player_found())
    } else {
        (Mark::Error, player_missing())
    });
    if agent {
        rows.push((Mark::Error, AGENT_EXPORTING.into()));
    }
    rows
}

/// The line under the destination, and whether it is an error.
fn destination_note(plan: Option<&Result<Plan, String>>) -> (String, bool) {
    match plan {
        None => ("Escolha a pasta onde o jogo será criado.".into(), false),
        Some(Ok(plan)) if plan.replaces => (
            format!(
                "Substitui a exportação anterior deste jogo em {}. A anterior só é apagada \
                 depois que a nova for conferida.",
                plan.folder.display()
            ),
            false,
        ),
        Some(Ok(plan)) => (
            format!("Será criada a pasta {}.", plan.folder.display()),
            false,
        ),
        Some(Err(error)) => (error.clone(), true),
    }
}

/// Whether the game may go where the line under the destination said (`shown`), as it would go
/// `now`, and whether it then replaces an earlier export there; None when the line is no longer
/// right.
fn confirmed(
    now: Option<&Result<Plan, String>>,
    shown: Option<&Result<Plan, String>>,
) -> Option<bool> {
    match (now, shown) {
        (Some(Ok(now)), Some(Ok(seen))) if now == seen => Some(now.replaces),
        _ => None,
    }
}

/// The commands that give `project` the dialog's settings and start scene: none when they are
/// as they were.
fn settings_commands(
    project: &Project,
    title: &str,
    window: [u32; 2],
    fullscreen: bool,
    start_scene: &Id,
) -> Vec<Command> {
    let game = &project.game;
    let title = title.trim();
    let mut commands = Vec::new();
    if title != game.title || window != game.window || fullscreen != game.fullscreen {
        commands.push(Command::SetGameSettings {
            title: Some(title.into()),
            window: Some(window),
            fullscreen: Some(fullscreen),
        });
    }
    if *start_scene != project.start_scene {
        commands.push(Command::SetStartScene {
            scene: start_scene.clone(),
        });
    }
    commands
}

/// A file size as people read it: kilobytes below one megabyte, else megabytes with a decimal
/// comma.
fn size_text(bytes: u64) -> String {
    const MB: f64 = 1024. * 1024.;
    if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.1} MB", bytes as f64 / MB).replace('.', ",")
    }
}

/// What the success says about the files and the check.
fn verified_text(report: &ExportReport) -> String {
    let seconds = report.verify.seconds;
    let seconds = if seconds.fract() == 0. {
        format!("{seconds:.0}")
    } else {
        format!("{seconds:.1}").replace('.', ",")
    };
    let unit = if report.verify.seconds < 2. {
        "segundo"
    } else {
        "segundos"
    };
    format!(
        "{} · {}. Conferido: o jogo exportado abre a cena {} e roda {seconds} {unit} sem erros.",
        count(report.files, "arquivo", "arquivos"),
        size_text(report.bytes),
        report.verify.start_scene.as_deref().unwrap_or_default()
    )
}

/// `text` ending in a full stop.
fn sentence(text: String) -> String {
    if text.ends_with(['.', '!', '?']) {
        text
    } else {
        format!("{text}.")
    }
}

/// The title and the text of an export that ended in `error`. A game in use is named by its
/// executable; when an earlier export was being replaced and stayed in place, the text says so.
fn failure(error: &ExportError, replacing: bool) -> (&'static str, String) {
    let unchanged = if replacing {
        KEPT
    } else {
        "Nada mudou no destino."
    };
    match error {
        // An empty folder held by another program: there was no game to close.
        ExportError::InUse(folder) if !replacing => (
            FAILED,
            format!(
                "A pasta {} está em uso; feche o que a estiver usando e tente de novo. {unchanged}",
                folder.display()
            ),
        ),
        ExportError::InUse(folder) => {
            let name = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                IN_USE,
                format!(
                    "Feche o jogo exportado ({name}{}) e tente de novo. {KEPT}",
                    std::env::consts::EXE_SUFFIX
                ),
            )
        }
        ExportError::Restore { .. } => (FAILED, error.to_string()),
        // Their own text already says that nothing changed.
        ExportError::Io { context, source } => (
            FAILED,
            format!("{} {unchanged}", sentence(format!("{context}: {source}"))),
        ),
        ExportError::Verify(report) => (
            FAILED,
            format!(
                "{} {unchanged}",
                sentence(format!(
                    "O jogo exportado não passou na conferência: {}",
                    report.errors.join(" ")
                ))
            ),
        ),
        _ if replacing => (FAILED, format!("{} {KEPT}", sentence(error.to_string()))),
        _ => (FAILED, error.to_string()),
    }
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::medium(11.))
            .color(theme::TEXT_MUTED),
    );
}

/// A line under a field, aligned with the fields.
fn row_hint(ui: &mut egui::Ui, column: f32, text: impl Into<egui::WidgetText>) {
    ui.horizontal(|ui| {
        ui.add_space(column + ui.spacing().item_spacing.x);
        ui.add(egui::Label::new(text).wrap());
    });
}

/// The mark of a Conferência row, `size` points wide.
fn paint_mark(ui: &mut egui::Ui, mark: Mark, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let inner = Rect::from_center_size(rect.center(), Vec2::splat(size * 0.78));
    match mark {
        Mark::Ok => icons::paint(ui, inner, Icon::Check, theme::SUCCESS),
        Mark::Warn => widgets::warning_icon(ui, inner, theme::WARNING),
        Mark::Error => {
            ui.painter().circle_stroke(
                rect.center(),
                size * 0.36,
                egui::Stroke::new(1.4, theme::ERROR),
            );
            icons::paint(
                ui,
                Rect::from_center_size(rect.center(), Vec2::splat(size * 0.42)),
                Icon::Close,
                theme::ERROR,
            );
        }
    }
}

fn check_row(ui: &mut egui::Ui, mark: Mark, text: &str) {
    ui.horizontal_top(|ui| {
        paint_mark(ui, mark, 18.);
        let color = match mark {
            Mark::Ok => OK_TEXT,
            Mark::Warn => WARNING_TEXT,
            Mark::Error => ERROR_TEXT,
        };
        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
    });
}

/// The heading of the result: its mark in a ring and what happened.
fn result_heading(ui: &mut egui::Ui, mark: Mark, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.), Sense::hover());
        let (icon, color) = match mark {
            Mark::Error => (Icon::Close, theme::ERROR),
            _ => (Icon::Check, theme::SUCCESS),
        };
        ui.painter()
            .circle_stroke(rect.center(), 9., egui::Stroke::new(1.5, color));
        icons::paint(
            ui,
            Rect::from_center_size(rect.center(), Vec2::splat(10.)),
            icon,
            color,
        );
        ui.label(
            RichText::new(text)
                .font(theme::medium(14.))
                .color(theme::TEXT),
        );
    });
}

fn open_folder(folder: &Path) -> std::io::Result<()> {
    let program = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(folder)
        .spawn()
        .map(drop)
}

impl Editor {
    /// The player exported games are made of: next to the editor, else where it should be.
    fn export_player() -> PathBuf {
        let exe = std::env::current_exe().unwrap_or_default();
        export::player_next_to(&exe).unwrap_or_else(|| exe.with_file_name(export::PLAYER_EXE))
    }
    /// The engine's own folder, which no game may go in nor hold.
    fn export_protected() -> Vec<PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_owned))
            .into_iter()
            .collect()
    }
    /// The painted images of the project not saved yet.
    fn unsaved_textures(&self) -> HashSet<Id> {
        self.state
            .images
            .dirty_images()
            .map(|(id, _)| id)
            .filter(|id| {
                self.state
                    .project
                    .asset(id)
                    .is_some_and(|asset| asset.kind == AssetKind::Texture)
            })
            .cloned()
            .collect()
    }
    fn export_check(&self) -> ExportCheck {
        export::check(
            &self.state.project,
            Some(&self.root()),
            &self.unsaved_textures(),
        )
    }
    /// Where the game titled `title` goes in `destination`, as the export would find it now
    /// with `player`; None without a destination.
    fn export_plan(
        &self,
        title: &str,
        destination: &str,
        player: &Path,
    ) -> Option<Result<Plan, String>> {
        let mut protected = Self::export_protected();
        protected.extend(player.parent().map(Path::to_owned));
        (!destination.is_empty()).then(|| {
            export::plan(
                &self.state.project.id,
                title,
                Path::new(destination),
                Some(&self.root()),
                &protected,
            )
            .map_err(|error| error.to_string())
        })
    }
    /// Back to the settings, with the Conferência and the destination looked at again.
    fn reconfigure(&self, dialog: &mut ExportDialog) {
        dialog.checked = self.export_check();
        dialog.player_found = dialog.player.is_file();
        dialog.dirty = self.dirty();
        dialog.planned = None;
        dialog.state = State::Configure;
    }
    /// The open project, its unsaved painted images as PNG and the player, for an export to
    /// `destination`.
    fn prepare_export(&self, destination: PathBuf, replace: bool) -> Result<Prepared, String> {
        let mut unsaved = HashMap::new();
        for id in self.unsaved_textures() {
            if let Some(image) = self.state.images.get(&id) {
                unsaved.insert(id, image.to_png()?);
            }
        }
        Ok(Prepared {
            project: self.state.project.clone(),
            root: self.root(),
            unsaved,
            player: Self::export_player(),
            protected: Self::export_protected(),
            destination,
            replace,
        })
    }

    /// Projeto → Exportar jogo…: a project never saved is saved first, as Importar does.
    pub(super) fn open_export(&mut self) {
        self.pause();
        if self.path.is_none() && !self.save() {
            return;
        }
        let project = &self.state.project;
        let player = Self::export_player();
        self.export.dialog = Some(ExportDialog {
            title: project.game.title.clone(),
            start_scene: project.start_scene.clone(),
            window: project.game.window,
            fullscreen: project.game.fullscreen,
            destination: self
                .preferences
                .export_folder
                .as_ref()
                .map(|folder| folder.display().to_string())
                .unwrap_or_default(),
            checked: self.export_check(),
            player_found: player.is_file(),
            player,
            dirty: self.dirty(),
            planned: None,
            ready: false,
            state: State::Configure,
        });
    }

    pub(super) fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.export.dialog.take() else {
            return;
        };
        if let State::Running { job, replacing } = &mut dialog.state
            && let Some(result) = job.poll()
        {
            let replacing = *replacing;
            dialog.state = match result {
                Ok(report) => {
                    self.log(format!("Jogo exportado: {}", report.folder.display()));
                    let warnings = report
                        .warnings
                        .iter()
                        .filter(|warning| !dialog.checked.warnings.contains(warning))
                        .cloned()
                        .collect();
                    State::Done {
                        report: Box::new(report),
                        warnings,
                    }
                }
                Err(error) => {
                    let (title, message) = failure(&error, replacing);
                    State::Failed { title, message }
                }
            };
        }
        let content = ctx.content_rect();
        let mut step = None;
        let response = egui::Modal::new("export_game".into()).show(ctx, |ui| {
            ui.set_width((content.width() - 48.).clamp(160., 560.));
            if widgets::dialog_header(ui, "Exportar jogo") {
                step = Some(Step::Close);
            }
            let chosen = match &dialog.state {
                State::Configure => self.export_settings(ui, &mut dialog, content.height()),
                State::Running { job, .. } => {
                    running_view(ui, job.stage);
                    None
                }
                State::Done { report, warnings } => done_view(ui, report, warnings),
                State::Failed { title, message } => failed_view(ui, title, message),
            };
            if chosen.is_some() {
                step = chosen;
            }
        });
        let running = matches!(dialog.state, State::Running { .. });
        if running {
            // The worker wakes the editor too; this covers a wake that came between frames.
            ctx.request_repaint_after(Duration::from_millis(100));
            self.export.dialog = Some(dialog);
            return;
        }
        if response.should_close() && step.is_none() {
            step = Some(Step::Close);
        }
        match step {
            Some(Step::Close) => return,
            Some(Step::Choose) => {
                let mut picker =
                    rfd::FileDialog::new().set_title("Escolha a pasta onde o jogo será criado");
                let typed = Path::new(dialog.destination.trim());
                if typed.is_absolute() && typed.is_dir() {
                    picker = picker.set_directory(typed);
                }
                if let Some(folder) = picker.pick_folder() {
                    dialog.destination = folder.display().to_string();
                }
            }
            Some(Step::Export) => self.start_export(ctx, &mut dialog),
            Some(Step::Back) => self.reconfigure(&mut dialog),
            Some(Step::OpenFolder) => {
                if let State::Done { report, .. } = &dialog.state
                    && let Err(error) = open_folder(&report.folder)
                {
                    self.warn(format!("Não foi possível abrir a pasta do jogo: {error}"));
                }
            }
            Some(Step::Play) => {
                if let State::Done { report, .. } = &dialog.state {
                    let started = std::process::Command::new(&report.executable)
                        .current_dir(&report.folder)
                        .spawn();
                    if let Err(error) = started {
                        self.warn(format!("Não foi possível abrir o jogo exportado: {error}"));
                    }
                }
            }
            None => {}
        }
        self.export.dialog = Some(dialog);
    }

    /// The settings, the destination and the Conferência, with Cancelar and Exportar.
    fn export_settings(
        &self,
        ui: &mut egui::Ui,
        dialog: &mut ExportDialog,
        height: f32,
    ) -> Option<Step> {
        let project = &self.state.project;
        let mut step = None;
        let title = effective_title(&dialog.title, &project.name).to_owned();
        let destination = dialog.destination.trim().to_owned();
        let stale = dialog
            .planned
            .as_ref()
            .is_none_or(|planned| planned.destination != destination || planned.title != title);
        if stale {
            dialog.planned = Some(Planned {
                plan: self.export_plan(&title, &destination, &dialog.player),
                destination,
                title: title.clone(),
            });
        }
        let plan = dialog
            .planned
            .as_ref()
            .and_then(|planned| planned.plan.as_ref());
        let settings = GameSettings {
            title: dialog.title.trim().to_owned(),
            window: dialog.window,
            fullscreen: dialog.fullscreen,
        }
        .validate()
        .err();
        let rows = check_rows(
            &dialog.checked,
            settings.as_deref(),
            (project.scenes.len(), project.assets.len()),
            dialog.dirty,
            dialog.player_found,
            self.export.agent.is_some(),
        );
        dialog.ready =
            !rows.iter().any(|(mark, _)| *mark == Mark::Error) && plan.is_some_and(Result::is_ok);
        let column =
            widgets::label_column(ui, &["Nome do jogo", "Cena inicial", "Janela", "Pasta"]);
        egui::ScrollArea::vertical()
            .max_height((height - 170.).max(120.))
            .show(ui, |ui| {
                section(ui, "JOGO");
                widgets::property_row_with(ui, column, "Nome do jogo", "", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.title)
                            .hint_text(project.name.as_str())
                            .char_limit(GameSettings::MAX_TITLE_CHARS)
                            .desired_width(f32::INFINITY),
                    );
                });
                let mut executable = egui::text::LayoutJob::default();
                let muted =
                    egui::TextFormat::simple(egui::FontId::proportional(12.), theme::TEXT_MUTED);
                executable.append("Executável: ", 0., muted.clone());
                executable.append(
                    &format!(
                        "{}{}",
                        export::file_name(&title),
                        std::env::consts::EXE_SUFFIX
                    ),
                    0.,
                    egui::TextFormat::simple(egui::FontId::monospace(11.5), OK_TEXT),
                );
                executable.append(" · título da janela", 0., muted);
                row_hint(ui, column, executable);
                let scenes: Vec<(Id, String)> = project
                    .scenes
                    .iter()
                    .map(|s| {
                        (
                            s.id.clone(),
                            format!("{} · {}", s.name, scenes::kind_label(s.kind)),
                        )
                    })
                    .collect();
                let selected = scenes
                    .iter()
                    .find(|(id, _)| *id == dialog.start_scene)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_default();
                widgets::property_row_with(ui, column, "Cena inicial", "", |ui| {
                    let width = ui.available_width();
                    widgets::combo(ui, "export_start_scene", &selected, width, |ui| {
                        for (id, label) in &scenes {
                            if ui
                                .selectable_label(*id == dialog.start_scene, label)
                                .clicked()
                            {
                                dialog.start_scene = id.clone();
                            }
                        }
                    });
                });
                widgets::property_row_with(ui, column, "Janela", "", |ui| {
                    let [min_width, min_height] = GameSettings::MIN_WINDOW;
                    let [max_width, max_height] = GameSettings::MAX_WINDOW;
                    let size = [76., theme::CONTROL_HEIGHT];
                    ui.add_sized(
                        size,
                        egui::DragValue::new(&mut dialog.window[0])
                            .range(min_width..=max_width)
                            .speed(1),
                    )
                    .on_hover_text("Largura da janela, em pixels");
                    ui.label(RichText::new("×").color(theme::TEXT_MUTED));
                    ui.add_sized(
                        size,
                        egui::DragValue::new(&mut dialog.window[1])
                            .range(min_height..=max_height)
                            .speed(1),
                    )
                    .on_hover_text("Altura da janela, em pixels");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("Tela cheia").size(12.5).color(OK_TEXT));
                        widgets::toggle(ui, &mut dialog.fullscreen, "Começar em tela cheia");
                    });
                });
                row_hint(
                    ui,
                    column,
                    RichText::new("No jogo, Alt+Enter alterna a tela cheia.")
                        .size(12.)
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(4.);
                ui.separator();
                section(ui, "DESTINO");
                widgets::property_row_with(ui, column, "Pasta", "", |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::secondary_button(ui, "Escolher…").clicked() {
                            step = Some(Step::Choose);
                        }
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut dialog.destination)
                                .font(egui::TextStyle::Monospace)
                                .hint_text("Escolha uma pasta")
                                .desired_width(f32::INFINITY),
                        );
                        icons::register_qa(ui, "Pasta de destino", field.rect);
                    });
                });
                let (note, error) = destination_note(plan);
                row_hint(
                    ui,
                    column,
                    RichText::new(note).size(12.).color(if error {
                        ERROR_TEXT
                    } else {
                        theme::TEXT_MUTED
                    }),
                );
                ui.add_space(4.);
                ui.separator();
                section(ui, "CONFERÊNCIA");
                for (mark, text) in &rows {
                    check_row(ui, *mark, text);
                }
            });
        widgets::dialog_footer(ui, |ui| {
            if widgets::primary_button(ui, "Exportar", dialog.ready)
                .on_disabled_hover_text(
                    "Resolva os erros da Conferência e escolha um destino válido para exportar.",
                )
                .clicked()
            {
                step = Some(Step::Export);
            }
            if widgets::secondary_button(ui, "Cancelar").clicked() {
                step = Some(Step::Close);
            }
        });
        step
    }

    /// Exportar and Tentar de novo: the settings changed in the dialog become one undo step, the
    /// destination is remembered and the export starts on its thread. It does what the line
    /// under the destination said: when the destination changed since (an agent exported
    /// there, say), the settings come back with the line as it is now.
    fn start_export(&mut self, ctx: &egui::Context, dialog: &mut ExportDialog) {
        if self.export.agent.is_some() {
            // Two exports of one game would race for its folder.
            dialog.state = State::Failed {
                title: FAILED,
                message: AGENT_EXPORTING.into(),
            };
            return;
        }
        let title = effective_title(&dialog.title, &self.state.project.name).to_owned();
        let typed = dialog.destination.trim().to_owned();
        let plan = self.export_plan(&title, &typed, &dialog.player);
        let shown = dialog
            .planned
            .as_ref()
            .filter(|planned| planned.destination == typed && planned.title == title)
            .and_then(|planned| planned.plan.as_ref());
        let Some(replacing) = confirmed(plan.as_ref(), shown) else {
            self.reconfigure(dialog);
            dialog.planned = Some(Planned {
                destination: typed,
                title,
                plan,
            });
            return;
        };
        let commands = settings_commands(
            &self.state.project,
            &dialog.title,
            dialog.window,
            dialog.fullscreen,
            &dialog.start_scene,
        );
        if !commands.is_empty() {
            self.finish_history(true);
            self.history.begin(
                "Configurar exportação",
                &self.state.project,
                &self.state.images,
            );
            if let Err(error) = command::apply(&mut self.state.project, &self.scene_id, &commands) {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                dialog.state = State::Failed {
                    title: FAILED,
                    message: sentence(error.message),
                };
                return;
            }
            self.finish_history(true);
        }
        let destination = PathBuf::from(typed);
        if self.preferences.export_folder.as_ref() != Some(&destination) {
            self.preferences.export_folder = Some(destination.clone());
            if let Err(error) = self.preferences.save() {
                self.warn(format!("Não foi possível guardar a preferência: {error}"));
            }
        }
        // Replacing only the earlier export the person was told about.
        let started = self
            .prepare_export(destination, replacing)
            .and_then(|prepared| Job::start(prepared, ctx));
        dialog.state = match started {
            Ok(job) => State::Running { job, replacing },
            Err(message) => State::Failed {
                title: FAILED,
                message,
            },
        };
    }

    /// An agent's `export` request: the open project as it is in memory, unsaved painted
    /// images included, to `destination`, replacing an earlier export of the game only with
    /// `replace`. It changes nothing in the project and is no undo step. The answer goes to
    /// `reply` when the export is over; it is refused during a test, while the editor is busy
    /// or already exporting, and for a project without a folder.
    pub(crate) fn agent_export(
        &mut self,
        ctx: &egui::Context,
        params: &Json,
        reply: mpsc::Sender<Result<Json, String>>,
    ) {
        let started = (|| {
            if let Some(reason) = self.agent_busy() {
                return Err(reason.to_owned());
            }
            if self.path.is_none() {
                return Err(
                    "O projeto ainda não tem pasta: peça para a pessoa salvá-lo antes de \
                            exportar o jogo."
                        .into(),
                );
            }
            if self.export.running() {
                return Err(
                    "O editor já está exportando o jogo; espere essa exportação terminar.".into(),
                );
            }
            let destination = params["destination"]
                .as_str()
                .ok_or("Informe destination: a pasta onde a pessoa pediu o jogo.")?;
            let replace = params["replace"].as_bool().unwrap_or(false);
            let prepared = self.prepare_export(PathBuf::from(destination.trim()), replace)?;
            Job::start(prepared, ctx)
        })();
        match started {
            Ok(job) => self.export.agent = Some((job, reply)),
            Err(error) => {
                self.log(format!("{}{error}", agent::AGENT_PREFIX));
                let _ = reply.send(Err(error));
            }
        }
    }

    /// Answers an agent's export once it is over. Called once per frame.
    pub(crate) fn poll_agent_export(&mut self, ctx: &egui::Context) {
        let Some((job, _)) = &mut self.export.agent else {
            return;
        };
        let Some(result) = job.poll() else {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        let (_, reply) = self.export.agent.take().expect("Exportação do agente");
        if let Some(dialog) = &mut self.export.dialog {
            // The line under the destination may say otherwise now.
            dialog.planned = None;
        }
        let answer = match result {
            Ok(report) => {
                self.log(format!(
                    "{}Jogo exportado: {}",
                    agent::AGENT_PREFIX,
                    report.folder.display()
                ));
                serde_json::to_value(&report).map_err(|e| e.to_string())
            }
            Err(error) => {
                let message = error.to_string();
                self.log(format!("{}{message}", agent::AGENT_PREFIX));
                Err(message)
            }
        };
        let _ = reply.send(answer);
    }
}

/// A quarter of a ring going round its track.
fn ring_spinner(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.), Sense::hover());
    let (center, radius) = (rect.center(), 11.);
    ui.painter()
        .circle_stroke(center, radius, egui::Stroke::new(3., theme::BORDER));
    let start = ui.input(|i| i.time) as f32 * std::f32::consts::TAU * 0.8;
    let arc: Vec<Pos2> = (0..=16)
        .map(|i| {
            let angle = start + i as f32 / 16. * std::f32::consts::FRAC_PI_2;
            center + radius * Vec2::angled(angle)
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        arc,
        egui::Stroke::new(3., theme::ACCENT_BRIGHT),
    ));
    ui.ctx().request_repaint();
}

fn running_view(ui: &mut egui::Ui, stage: ExportStage) {
    ui.vertical_centered(|ui| {
        ui.add_space(36.);
        ring_spinner(ui);
        ui.add_space(8.);
        ui.label(
            RichText::new(match stage {
                ExportStage::Copying => "Copiando arquivos…",
                ExportStage::Verifying => "Conferindo o jogo exportado…",
            })
            .font(theme::medium(13.))
            .color(theme::TEXT),
        );
        widgets::hint(
            ui,
            "A exportação roda em segundo plano; o editor continua respondendo.",
        );
        ui.add_space(36.);
    });
}

fn done_view(ui: &mut egui::Ui, report: &ExportReport, warnings: &[String]) -> Option<Step> {
    let mut step = None;
    result_heading(ui, Mark::Ok, "Jogo exportado");
    ui.add_space(4.);
    ui.label(
        RichText::new(format!(
            "{}{}",
            report.folder.display(),
            std::path::MAIN_SEPARATOR
        ))
        .monospace()
        .size(12.)
        .color(OK_TEXT),
    );
    ui.add(
        egui::Label::new(
            RichText::new(verified_text(report))
                .size(12.)
                .color(Color32::from_rgb(168, 168, 174)),
        )
        .wrap(),
    );
    for warning in warnings {
        check_row(ui, Mark::Warn, warning);
    }
    widgets::hint(
        ui,
        "Para jogar em outro computador, copie a pasta inteira. O progresso salvo fica em \
         %APPDATA%\\OXY Engine\\Jogos.",
    );
    widgets::dialog_footer(ui, |ui| {
        if widgets::primary_button(ui, "Fechar", true).clicked() {
            step = Some(Step::Close);
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if widgets::secondary_button(ui, "Abrir pasta").clicked() {
                step = Some(Step::OpenFolder);
            }
            if widgets::secondary_button(ui, "Jogar agora").clicked() {
                step = Some(Step::Play);
            }
        });
    });
    step
}

fn failed_view(ui: &mut egui::Ui, title: &str, message: &str) -> Option<Step> {
    let mut step = None;
    result_heading(ui, Mark::Error, title);
    ui.add_space(4.);
    ui.add(egui::Label::new(RichText::new(message).size(12.5).color(OK_TEXT)).wrap());
    widgets::dialog_footer(ui, |ui| {
        if widgets::primary_button(ui, "Tentar de novo", true).clicked() {
            step = Some(Step::Export);
        }
        if widgets::secondary_button(ui, "Voltar").clicked() {
            step = Some(Step::Back);
        }
    });
    step
}

#[cfg(test)]
pub(crate) struct QaExport {
    /// configure, running, done or failed.
    pub(crate) state: &'static str,
    pub(crate) ready: bool,
    pub(crate) report: Option<ExportReport>,
    pub(crate) failure: Option<String>,
}
#[cfg(test)]
impl Editor {
    /// The export dialog as native QA checks it.
    pub(crate) fn qa_export(&self) -> Option<QaExport> {
        let dialog = self.export.dialog.as_ref()?;
        let (state, report, failure) = match &dialog.state {
            State::Configure => ("configure", None, None),
            State::Running { .. } => ("running", None, None),
            State::Done { report, .. } => ("done", Some((**report).clone()), None),
            State::Failed { message, .. } => ("failed", None, Some(message.clone())),
        };
        Some(QaExport {
            state,
            ready: dialog.ready,
            report,
            failure,
        })
    }
    /// The folder Exportar remembers, as applied.
    pub(crate) fn qa_export_folder(&self) -> Option<&Path> {
        self.preferences.export_folder.as_deref()
    }
    /// Holds the next export at its start until the answer sends or goes away.
    pub(crate) fn qa_hold_next_export(&self) -> mpsc::Sender<()> {
        let (sender, gate) = mpsc::channel();
        *QA_GATE.lock().unwrap() = Some(gate);
        sender
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::export::VerifyReport;

    fn report(files: usize, bytes: u64, seconds: f64) -> ExportReport {
        ExportReport {
            folder: PathBuf::from("D:\\Jogos\\Meu Parkour"),
            executable: PathBuf::from("D:\\Jogos\\Meu Parkour\\Meu Parkour.exe"),
            files,
            bytes,
            replaced: false,
            warnings: Vec::new(),
            verify: VerifyReport {
                ok: true,
                engine: export::engine_version(),
                game: "Meu Parkour".into(),
                start_scene: Some("Menu".into()),
                seconds,
                ..Default::default()
            },
        }
    }

    #[test]
    fn the_check_lists_errors_warnings_unsaved_changes_and_the_player() {
        let clean = ExportCheck::default();
        assert_eq!(
            check_rows(&clean, None, (4, 12), false, true, false),
            [
                (Mark::Ok, "Projeto válido: 4 cenas, 12 recursos.".to_owned()),
                (Mark::Ok, player_found()),
            ]
        );
        assert_eq!(
            check_rows(&clean, None, (1, 1), false, true, false)[0].1,
            "Projeto válido: 1 cena, 1 recurso."
        );
        let found = ExportCheck {
            errors: vec!["Trilha: arquivo ausente".into()],
            warnings: vec!["Correr e Deslizar usam a mesma tecla".into()],
        };
        let rows = check_rows(
            &found,
            Some("O título do jogo tem mais de 80 caracteres"),
            (4, 12),
            true,
            false,
            true,
        );
        assert_eq!(
            rows,
            [
                (Mark::Error, "Trilha: arquivo ausente".to_owned()),
                (
                    Mark::Error,
                    "O título do jogo tem mais de 80 caracteres".to_owned()
                ),
                (Mark::Warn, UNSAVED.to_owned()),
                (
                    Mark::Warn,
                    "Correr e Deslizar usam a mesma tecla".to_owned()
                ),
                (Mark::Error, player_missing()),
                (Mark::Error, AGENT_EXPORTING.to_owned()),
            ]
        );
        assert!(player_missing().contains("Extraia o ZIP inteiro da OXY Engine"));
        assert!(player_found().contains(&export::engine_version()));
        // Warnings alone do not stop the export.
        let warned = ExportCheck {
            warnings: found.warnings.clone(),
            ..Default::default()
        };
        assert!(
            check_rows(&warned, None, (1, 0), true, true, false)
                .iter()
                .all(|(mark, _)| *mark != Mark::Error)
        );
    }

    #[test]
    fn the_destination_note_says_what_happens_there() {
        let folder = PathBuf::from("D:\\Jogos\\Meu Parkour");
        let new = Plan {
            folder: folder.clone(),
            replaces: false,
        };
        assert_eq!(
            destination_note(Some(&Ok(new))),
            (format!("Será criada a pasta {}.", folder.display()), false)
        );
        let earlier = Plan {
            folder: folder.clone(),
            replaces: true,
        };
        let (note, error) = destination_note(Some(&Ok(earlier)));
        assert!(!error);
        assert_eq!(
            note,
            format!(
                "Substitui a exportação anterior deste jogo em {}. A anterior só é apagada \
                 depois que a nova for conferida.",
                folder.display()
            )
        );
        let refused = Err("A raiz de um disco (C:\\) não recebe jogos exportados".to_owned());
        assert_eq!(
            destination_note(Some(&refused)),
            (
                "A raiz de um disco (C:\\) não recebe jogos exportados".to_owned(),
                true
            )
        );
        assert!(!destination_note(None).1);
        // Exportar does what the line said, or shows the line again.
        let fresh = Ok(Plan {
            folder: folder.clone(),
            replaces: false,
        });
        let earlier = Ok(Plan {
            folder: folder.clone(),
            replaces: true,
        });
        assert_eq!(confirmed(Some(&fresh), Some(&fresh)), Some(false));
        assert_eq!(confirmed(Some(&earlier), Some(&earlier)), Some(true));
        // An agent exported there after the line said the folder would be created.
        assert_eq!(confirmed(Some(&earlier), Some(&fresh)), None);
        assert_eq!(confirmed(Some(&fresh), Some(&earlier)), None);
        assert_eq!(confirmed(Some(&refused), Some(&fresh)), None);
        assert_eq!(confirmed(Some(&fresh), None), None);
        assert_eq!(confirmed(None, None), None);
        assert_eq!(effective_title("  ", " Corrida "), "Corrida");
        assert_eq!(effective_title(" Noturna ", "Corrida"), "Noturna");
    }

    #[test]
    fn only_changed_settings_become_commands() {
        let mut project = Project::new("Corrida");
        let start = project.start_scene.clone();
        assert!(settings_commands(&project, "", [1280, 720], false, &start).is_empty());
        // Spaces around the title are not a change, as the command trims it.
        assert!(settings_commands(&project, "  ", [1280, 720], false, &start).is_empty());
        let other = Scene::new("Menu", SceneKind::ThreeD);
        let menu = other.id.clone();
        project.scenes.push(other);
        let commands = settings_commands(&project, " Meu Parkour ", [1920, 1080], true, &menu);
        assert_eq!(
            commands,
            [
                Command::SetGameSettings {
                    title: Some("Meu Parkour".into()),
                    window: Some([1920, 1080]),
                    fullscreen: Some(true),
                },
                Command::SetStartScene {
                    scene: menu.clone()
                },
            ]
        );
        // Both apply as one batch.
        let scene = project.start_scene.clone();
        command::apply(&mut project, &scene, &commands).unwrap();
        assert_eq!(project.game_title(), "Meu Parkour");
        assert_eq!(project.start_scene, menu);
        assert!(settings_commands(&project, "Meu Parkour", [1920, 1080], true, &menu).is_empty());
    }

    #[test]
    fn the_result_reads_as_people_say_it() {
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(412 * 1024), "412 KB");
        assert_eq!(size_text(24_536_678), "23,4 MB");
        assert_eq!(
            verified_text(&report(14, 24_536_678, 2.)),
            "14 arquivos · 23,4 MB. Conferido: o jogo exportado abre a cena Menu e roda 2 \
             segundos sem erros."
        );
        assert!(verified_text(&report(1, 10, 0.5)).starts_with("1 arquivo · 1 KB."));
        assert!(verified_text(&report(1, 10, 0.5)).contains("roda 0,5 segundo sem erros"));
        let folder = PathBuf::from("D:\\Jogos\\Meu Parkour");
        let (title, message) = failure(&ExportError::InUse(folder.clone()), true);
        assert_eq!(title, IN_USE);
        assert_eq!(
            message,
            format!(
                "Feche o jogo exportado (Meu Parkour{}) e tente de novo. {KEPT}",
                std::env::consts::EXE_SUFFIX
            )
        );
        // An empty folder in use held no earlier export, nor a game to close.
        let (title, message) = failure(&ExportError::InUse(folder.clone()), false);
        assert_eq!(title, FAILED);
        assert_eq!(
            message,
            format!(
                "A pasta {} está em uso; feche o que a estiver usando e tente de novo. Nada mudou \
                 no destino.",
                folder.display()
            )
        );
        let invalid = ExportError::Invalid(vec!["A cena inicial não existe".into()]);
        let (title, message) = failure(&invalid, false);
        assert_eq!(title, FAILED);
        assert_eq!(
            message,
            "O projeto não pode ser exportado: A cena inicial não existe"
        );
        let (_, message) = failure(&invalid, true);
        assert_eq!(
            message,
            format!("O projeto não pode ser exportado: A cena inicial não existe. {KEPT}")
        );
        let restore = ExportError::Restore {
            kept: folder,
            source: std::io::Error::other("negado"),
        };
        assert!(!failure(&restore, true).1.contains(KEPT));
        // Said once that nothing changed: the earlier export, when there was one.
        let io = || ExportError::Io {
            context: "Não foi possível copiar o player do jogo".into(),
            source: std::io::Error::other("disco cheio"),
        };
        assert_eq!(
            failure(&io(), false).1,
            "Não foi possível copiar o player do jogo: disco cheio. Nada mudou no destino."
        );
        assert_eq!(
            failure(&io(), true).1,
            format!("Não foi possível copiar o player do jogo: disco cheio. {KEPT}")
        );
        let verify = ExportError::Verify(Box::new(VerifyReport {
            errors: vec!["Asset ausente ou inacessível 'Bip'".into()],
            ..Default::default()
        }));
        assert_eq!(
            failure(&verify, true).1,
            format!(
                "O jogo exportado não passou na conferência: Asset ausente ou inacessível 'Bip'. \
                 {KEPT}"
            )
        );
    }
}
