mod agent;
mod agent_channel;
mod body_tools;
mod camera_tools;
mod code_view;
mod console_feed;
mod console_panel;
mod diagnostics;
mod export;
mod hierarchy;
pub(crate) use hierarchy::entity_icon;
mod home;
mod library;
mod logic;
mod modeling;
mod navigation;
mod notices;
mod physics;
mod preferences;
mod properties;
mod scenes;
mod script_status;
mod script_watch;
mod spatial_tools;
mod test_progress;
mod textures;
mod toolbar;
mod tree_files;
mod viewport;
mod viewport_controls;
use spatial_tools::SpatialTools;
pub(crate) use spatial_tools::Tool;
pub(crate) use viewport::frame_camera;

use crate::{
    graph_ui::{GraphView, object_picker, value_editor},
    studio::{Studio, StudioTab},
};
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use glam::Vec3;
use oxy_core::{
    conversion,
    document::*,
    edit_history::CommandHistory,
    editing::{self, Selection},
    painting::PaintImage,
    persistence, random,
    runtime::{InputFrame, Runtime, RuntimeOptions},
    script::ScriptSources,
    texture_cache::TextureCache,
};
use oxy_render::{CameraState, Renderer};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, PartialEq)]
pub struct Snapshot {
    pub project: Project,
    pub images: TextureCache,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Scene,
    Studio,
    Logic,
}
/// Tabs of the bottom dock.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DockTab {
    Library,
    Console,
    Notices,
    /// The script "Ver código" opened, read only.
    Code,
}
/// Bottom dock shared by the Library, the Console and the notice history.
pub(crate) struct Dock {
    /// Open state per work mode (Cena, Estúdio, Lógica). The Scene keeps the Library at
    /// hand; the Studio and Logic start collapsed to give their work area the room.
    open: [bool; 3],
    pub(crate) tab: DockTab,
}
#[derive(Clone, Copy, PartialEq, Default)]
enum CompactPanel {
    #[default]
    Hierarchy,
    Properties,
    Library,
    None,
}
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Gizmo {
    Move,
    Rotate,
    Scale,
}
#[derive(Clone)]
pub(crate) struct GizmoDrag {
    entity: Id,
    base: Transform,
    axis: usize,
    origin: Pos2,
    direction: Vec2,
    pixels_per_unit: f32,
    originals: Vec<(Id, Transform)>,
    center: Vec3,
}
pub(crate) struct Rename {
    id: Id,
    text: String,
    focus: bool,
}
/// Viewport display options. Session state, not part of the project document.
pub(crate) struct ViewOptions {
    pub(crate) grid: bool,
    pub(crate) snap_grid: bool,
    pub(crate) grid_size: f32,
    pub(crate) show_disabled_colliders: bool,
    pub(crate) view_global: bool,
    pub(crate) debug: bool,
}
impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            grid: true,
            snap_grid: true,
            grid_size: 0.25,
            show_disabled_colliders: false,
            view_global: false,
            debug: false,
        }
    }
}
/// Who plays the test: the person, with their mouse and keyboard, or an AI agent through its
/// channel, which never takes the mouse or reads the keyboard and plays on while the window is
/// in the background.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Driver {
    #[default]
    Person,
    Agent,
}
/// The isolated play test. While it runs, the central area shows the game.
pub(crate) struct PlaySession {
    pub(crate) runtime: Option<Runtime>,
    pub(crate) game_ui: oxy_render::game_ui::GameUi,
    pub(crate) capture: bool,
    pub(crate) driver: Driver,
    /// Clicks and Esc an agent sent, by game time, given to the game when that time comes.
    pub(crate) agent_clicks: std::collections::VecDeque<(f64, oxy_core::headless::Direct)>,
    pub(crate) previous_tab: Tab,
    pub(crate) last_runtime_scene: Option<Id>,
    pub(crate) game_size: Option<[u32; 2]>,
    /// The seed the current or last test of the project started with, for agents to repeat
    /// its draws in oxy_simulate after it stops too.
    pub(crate) seed: Option<u64>,
    /// What Salvar progresso recorded in this editor session's tests, which the next test
    /// starts from (see `test_progress`).
    pub(crate) test_save: oxy_core::save::SaveSlot,
    /// The test's sounds and the Library's Ouvir.
    pub(crate) mixer: oxy_audio::Mixer,
    /// The watcher of the tree's scripts while a test runs scripts (see `script_watch`).
    pub(crate) script_watch: Option<script_watch::ScriptWatch>,
    /// The person's test goes on while the window is in the background, as Preferências do
    /// editor › Teste allow: no input of theirs reaches it until the focus comes back.
    pub(crate) unfocused: bool,
    /// The person's test paused because the window lost the focus: the Pausado chip says why.
    pub(crate) focus_paused: bool,
    /// The focus came back to a test going on without it: the press that brought it is the
    /// window's, and its click reaches neither the game's buttons nor its objects.
    pub(crate) refocus_click: bool,
    /// The actions held in the input the game took in the last frame.
    held: std::collections::BTreeSet<String>,
}
impl Default for PlaySession {
    fn default() -> Self {
        Self {
            runtime: None,
            game_ui: Default::default(),
            capture: false,
            driver: Driver::Person,
            agent_clicks: Default::default(),
            previous_tab: Tab::Scene,
            last_runtime_scene: None,
            game_size: None,
            seed: None,
            test_save: Default::default(),
            mixer: Default::default(),
            script_watch: None,
            unfocused: false,
            focus_paused: false,
            refocus_click: false,
            held: Default::default(),
        }
    }
}
impl PlaySession {
    /// What another project must not inherit: the running test, its seed, its sounds and the
    /// test progress.
    pub(crate) fn forget_project(&mut self) {
        self.end();
        self.seed = None;
        self.test_save = Default::default();
    }
    /// Parar: the test ends, and nothing it played goes on.
    pub(crate) fn end(&mut self) {
        self.runtime = None;
        self.script_watch = None;
        self.capture = false;
        self.driver = Driver::Person;
        self.agent_clicks.clear();
        self.last_runtime_scene = None;
        self.unfocused = false;
        self.focus_paused = false;
        self.refocus_click = false;
        self.held.clear();
        self.mixer.stop_all();
    }
    /// Follows the window's focus in a frame of the test, before its step (design §6.2): true
    /// when the test must pause. Losing it pauses the person's test, unless `play_on`
    /// (Preferências do editor › Teste) keeps it going without their input; an agent's plays
    /// on, as its inputs need no window.
    pub(crate) fn follow_focus(&mut self, focused: bool, play_on: bool) -> bool {
        let running = self.runtime.as_ref().is_some_and(|rt| !rt.paused);
        let person = self.driver == Driver::Person;
        // A test seen running forgets why it paused before.
        if running {
            self.focus_paused = false;
        }
        if self.unfocused && focused {
            self.refocus_click = true;
        }
        self.unfocused = !focused && person && play_on && running;
        let pause = !focused && person && !play_on;
        if pause && running {
            self.focus_paused = true;
        }
        pause
    }
    /// Follows the mouse in a frame of the test, before `follow_focus`: `pressing` when a button
    /// is down or let go in it. The press that brought the focus back is over in the first
    /// frame after that one without either, as it can come a frame after the focus.
    pub(crate) fn follow_pointer(&mut self, pressing: bool) {
        self.refocus_click &= pressing;
    }
    /// The input the game takes in this frame: what the person's keyboard and mouse gave or,
    /// while the test goes on without the focus, none; in the frame the focus leaves, what was
    /// held is let go, so no key or button sticks.
    pub(crate) fn game_input(&mut self, input: InputFrame) -> InputFrame {
        let input = if self.unfocused {
            InputFrame {
                released: std::mem::take(&mut self.held),
                ..Default::default()
            }
        } else {
            input
        };
        self.held.clone_from(&input.held);
        input
    }
    /// Plays what the test asked for and follows its pauses; what went wrong, for the Console.
    /// Also called when the test pauses, since the Logic tab does not draw the game.
    pub(crate) fn update_audio(&mut self, root: &Path) -> Vec<String> {
        if let Some(runtime) = &mut self.runtime {
            self.mixer.update(runtime, root);
        }
        self.mixer.take_errors()
    }
}
#[derive(Default)]
pub(crate) struct HierarchyUi {
    pub(crate) search: String,
    pub(crate) rename: Option<Rename>,
    pub(crate) focus: bool,
    pub(crate) last_object_click: Option<Id>,
    pub(crate) collapsed: std::collections::HashSet<Id>,
    pub(crate) reveal_scroll: Option<Id>,
    pub(crate) context_target: Option<Id>,
}
#[derive(Default)]
pub(crate) struct AssetUi {
    pub(crate) thumbnail: Option<(Id, egui::TextureHandle)>,
    pub(crate) locate: Option<Id>,
}
/// Active gizmo and the multi-selection rotation/scale fields.
pub(crate) struct TransformUi {
    pub(crate) gizmo: Gizmo,
    pub(crate) gizmo_drag: Option<GizmoDrag>,
    pub(crate) selection_rotation: [f32; 3],
    pub(crate) selection_scale: f32,
}
impl Default for TransformUi {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::Move,
            gizmo_drag: None,
            selection_rotation: [0.; 3],
            selection_scale: 1.,
        }
    }
}
pub(crate) struct ConsoleLog {
    pub(crate) messages: Vec<String>,
    /// Messages removed from the front (the oldest past the limit, or a cleared console).
    pub(crate) dropped: u64,
}
impl ConsoleLog {
    /// Messages ever written; the last `messages.len()` of them are kept.
    pub(crate) fn written(&self) -> u64 {
        self.dropped + self.messages.len() as u64
    }
    /// Adds a message; past 600 kept, the oldest 100 go.
    pub(crate) fn push(&mut self, message: String) {
        self.messages.push(message);
        if self.messages.len() > 600 {
            self.messages.drain(..100);
            self.dropped += 100;
        }
    }
    /// Empties the console; its messages still count as written.
    pub(crate) fn clear(&mut self) {
        self.dropped += self.messages.len() as u64;
        self.messages.clear();
    }
}
/// Frame timing shown by the performance overlay.
pub(crate) struct FrameStats {
    pub(crate) visible: bool,
    pub(crate) last_frame: Instant,
    pub(crate) interval_ms: f32,
    pub(crate) cpu_ms: f32,
}
enum Transition {
    Home,
    Create(String, SceneKind),
    Open(PathBuf),
    Recipe(PathBuf),
    /// A copy of a recipe in script, opened in the Cena with the first object that runs a script
    /// selected, whose Scripts card shows it.
    ScriptRecipe(PathBuf),
    Laboratory(PathBuf),
    Close,
}

pub struct Editor {
    home: home::Home,
    notices: notices::Notices,
    preferences: preferences::Preferences,
    pending_preferences: Option<preferences::Preferences>,
    /// Local channel for AI agents, open while Permitir agentes de IA is on.
    agents: Option<agent_channel::AgentChannel>,
    navigation: navigation::Navigation,
    body_tools: body_tools::BodyTools,
    new_project: Option<scenes::NewProject>,
    scene_dialog: Option<scenes::SceneDialog>,
    /// Exportar jogo and an agent's export.
    export: export::ExportUi,
    logic_ui: logic::LogicUi,
    modeling: modeling::ModelState,
    physics_ui: physics::PhysicsUi,
    pub(crate) camera_tools: camera_tools::CameraTools,
    compact_panel: CompactPanel,
    pub state: Snapshot,
    history: CommandHistory,
    pub path: Option<PathBuf>,
    /// What OXY last read or wrote of the open project's model and image files, which saving
    /// compares the disk with.
    known_files: persistence::KnownFiles,
    /// The open project's `.oxy/editor.lock`, held while it is open so that agents on files do
    /// not write it.
    pub(crate) lock: Option<oxy_core::project_lock::EditorLock>,
    /// What the last save that organized the project in folders did, for an agent's answer.
    pub(crate) converted: Option<serde_json::Value>,
    /// Why the last save kept the project in the old layout, for an agent's answer.
    pub(crate) kept_layout: Option<String>,
    /// The last save organized the project in folders and the undo history started over.
    pub(crate) undo_restarted: bool,
    pub scene_id: Id,
    pub selected: Option<Id>,
    pub selection: Selection,
    pub(crate) hierarchy_ui: HierarchyUi,
    pub tab: Tab,
    pub studio: Studio,
    graph: GraphView,
    pub renderer: Renderer,
    render_state: eframe::egui_wgpu::RenderState,
    context: egui::Context,
    pub(crate) camera: CameraState,
    pub(crate) editor_size: [u32; 2],
    pub(crate) play: PlaySession,
    pub(crate) console: ConsoleLog,
    /// The console as agents read it, kept beyond a play test.
    pub(crate) console_feed: console_feed::ConsoleFeed,
    pub(crate) dock: Dock,
    pub(crate) frame: FrameStats,
    pub(crate) view: ViewOptions,
    pub(crate) transform_ui: TransformUi,
    pub(crate) spatial: SpatialTools,
    pending: Option<Transition>,
    allow_close: bool,
    pub(crate) scale: f32,
    attribute_name: String,
    attribute_type: u8,
    pub(crate) asset_ui: AssetUi,
    /// The Library's folders, filters, selection and dialogs.
    pub(crate) library: library::LibraryUi,
    save_requested: bool,
    /// What agents' writes kept of the scripts they changed in this editor session (see
    /// `script_files`).
    agent_scripts_kept: std::collections::HashSet<PathBuf>,
    /// The project's tree as the last walk found it, followed by OXY's own operations since:
    /// the next walk tells what changed outside OXY (see `tree_files`).
    tree_scan: Option<oxy_core::project_tree::Tree>,
    /// A walk of the tree waits for the step in progress, or the test, to end.
    scan_requested: bool,
    /// F5 asked for it: a tree the last walk could not take is walked again too.
    scan_forced: bool,
    /// Whether the window had the focus in the last frame.
    was_focused: bool,
    /// What the Scripts card and the Hierarchy show of the attached scripts.
    script_status: script_status::ScriptStatus,
    /// The search of "+ Anexar script".
    script_search: String,
    /// The script open in the dock's code tab.
    code_view: Option<code_view::CodeView>,
    /// Which lines the Console shows.
    console_show: console_panel::Show,
    /// The script errors the test told, which the Console keeps after the runtime forgets them.
    console_errors: console_panel::ScriptErrors,
}

impl Editor {
    #[cfg(test)]
    pub(crate) fn qa_ux_refusal(&self) -> Result<(), String> {
        if self.console_visible() {
            return Err("Recusa abriu o console automaticamente".into());
        }
        if self.dirty() {
            return Err("Preferência ou aviso entrou no histórico".into());
        }
        if self
            .console
            .messages
            .last()
            .is_none_or(|m| !m.contains("Escolha somente um objeto"))
        {
            return Err("Atalho inválido não informou motivo".into());
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn qa_console_open(&self) -> bool {
        self.console_visible()
    }
    pub(crate) fn console_visible(&self) -> bool {
        if self.animating() {
            return self.studio.animation.bottom == Some(DockTab::Console);
        }
        self.dock_open() && self.dock.tab == DockTab::Console
    }
    pub(crate) fn show_dock(&mut self, tab: DockTab) {
        // In the Animation the timeline's own tabs hold the Console and the notices.
        if self.animating() && !matches!(tab, DockTab::Library | DockTab::Code) {
            self.studio.animation.bottom = Some(tab);
            return;
        }
        self.set_dock_open(true);
        self.dock.tab = tab;
    }
    fn animating(&self) -> bool {
        self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation
    }
    fn dock_mode(&self) -> usize {
        match self.tab {
            Tab::Scene => 0,
            Tab::Studio => 1,
            Tab::Logic => 2,
        }
    }
    /// Whether the bottom dock is expanded in the current work mode.
    pub(crate) fn dock_open(&self) -> bool {
        self.dock.open[self.dock_mode()]
    }
    pub(crate) fn set_dock_open(&mut self, open: bool) {
        if self.animating() && !open {
            self.studio.animation.bottom = None;
            return;
        }
        let mode = self.dock_mode();
        self.dock.open[mode] = open;
    }
    /// A play test exists (running or paused).
    pub(crate) fn playing(&self) -> bool {
        self.play.runtime.is_some()
    }
    /// The central area shows the game: a test exists and Logic is not open.
    pub(crate) fn showing_game(&self) -> bool {
        self.playing() && self.tab != Tab::Logic
    }
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let rs = cc
            .wgpu_render_state
            .clone()
            .expect("A OXY Engine requer backend wgpu");
        let project = Project::new("Meu projeto OXY");
        let scene_id = project.start_scene.clone();
        let camera = CameraState::for_scene(project.scene(&scene_id).unwrap());
        let state = Snapshot {
            project,
            images: TextureCache::default(),
        };
        let (preferences, preference_error) = preferences::Preferences::load();
        let scale = preferences.scale_percent as f32 / 100.;
        cc.egui_ctx.set_zoom_factor(scale);
        let mut this = Self {
            home: home::Home::load(&cc.egui_ctx),
            notices: Default::default(),
            preferences,
            pending_preferences: None,
            agents: None,
            navigation: Default::default(),
            body_tools: Default::default(),
            new_project: None,
            scene_dialog: None,
            export: Default::default(),
            logic_ui: Default::default(),
            modeling: Default::default(),
            physics_ui: Default::default(),
            camera_tools: Default::default(),
            compact_panel: Default::default(),
            history: CommandHistory::new(),
            state,
            path: None,
            known_files: Default::default(),
            lock: None,
            converted: None,
            kept_layout: None,
            undo_restarted: false,
            scene_id,
            selected: None,
            selection: Selection::default(),
            hierarchy_ui: Default::default(),
            tab: Tab::Scene,
            studio: Studio::default(),
            graph: GraphView::default(),
            renderer: Renderer::new(&rs),
            render_state: rs,
            context: cc.egui_ctx.clone(),
            camera,
            editor_size: [1280, 720],
            play: Default::default(),
            console: ConsoleLog {
                messages: vec!["OXY Engine · editor nativo · documentos locais".into()],
                dropped: 0,
            },
            console_feed: Default::default(),
            dock: Dock {
                open: [true, false, false],
                tab: DockTab::Library,
            },
            frame: FrameStats {
                visible: false,
                last_frame: Instant::now(),
                interval_ms: 0.,
                cpu_ms: 0.,
            },
            view: Default::default(),
            transform_ui: Default::default(),
            spatial: SpatialTools::default(),
            pending: None,
            allow_close: false,
            scale,
            attribute_name: String::new(),
            attribute_type: 0,
            asset_ui: Default::default(),
            library: Default::default(),
            save_requested: false,
            agent_scripts_kept: Default::default(),
            tree_scan: None,
            scan_requested: false,
            scan_forced: false,
            was_focused: true,
            script_status: Default::default(),
            script_search: String::new(),
            code_view: None,
            console_show: Default::default(),
            console_errors: Default::default(),
        };
        if let Some(error) = preference_error {
            this.warn(error);
            this.notice_last(true);
        }
        let arg = if cfg!(test) {
            None
        } else {
            std::env::args_os().nth(1).map(PathBuf::from)
        };
        if let Some(path) = arg {
            this.open(path);
        }
        this
    }
    pub fn scene(&self) -> &Scene {
        self.state
            .project
            .scene(&self.scene_id)
            .expect("Cena selecionada validada")
    }
    pub fn scene_mut(&mut self) -> &mut Scene {
        self.state
            .project
            .scene_mut(&self.scene_id)
            .expect("Cena selecionada validada")
    }
    pub fn root(&self) -> PathBuf {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."))
            .to_path_buf()
    }
    pub fn log(&mut self, message: impl Into<String>) {
        self.console.push(message.into());
    }
    fn dirty(&self) -> bool {
        self.history.is_dirty()
            || self
                .history
                .pending_changed(&self.state.project, &self.state.images)
            || self.state.images.has_dirty()
            // Files OXY moved since the last save (opened after a crash, say): saving writes
            // the project as they are, discarding brings them back.
            || !self.known_files.journal().is_clean()
    }
    fn reset_context(&mut self) {
        self.spatial = SpatialTools::default();
        self.transform_ui.gizmo_drag = None;
        self.hierarchy_ui.collapsed.clear();
        self.hierarchy_ui.reveal_scroll = None;
        self.sync_console_feed();
        self.console_feed.new_test();
        self.play.forget_project();
        self.selected = None;
        self.selection.single(None);
        self.hierarchy_ui.rename = None;
        self.hierarchy_ui.last_object_click = None;
        self.tab = Tab::Scene;
        self.studio = Studio::default();
        self.graph = GraphView::default();
        self.scene_id = self.state.project.start_scene.clone();
        self.camera = CameraState::for_scene(self.scene());
        self.history.clear();
        self.history.mark_saved();
        self.asset_ui.thumbnail = None;
        self.library = Default::default();
        self.script_status = Default::default();
        self.script_search.clear();
        self.close_code();
        self.sync_textures();
    }
    pub(crate) fn open(&mut self, path: PathBuf) {
        self.open_with_recent(path, true);
    }
    /// Opens the project at `path`; answers whether it opened.
    fn open_with_recent(&mut self, path: PathBuf, add_recent: bool) -> bool {
        let path = if path.is_dir() {
            path.join(persistence::PROJECT_FILE)
        } else {
            path
        };
        let root = path.parent().unwrap_or(Path::new(".")).to_owned();
        let agent_writing = oxy_core::project_lock::wait_for_agents(&root);
        match persistence::open_project(&path) {
            Ok((project, report)) => {
                let mut images = TextureCache::default();
                images.configure(&root, &project);
                self.state = Snapshot { project, images };
                self.path = Some(path);
                self.known_files = report.known.clone();
                self.reset_context();
                self.rebase_scan();
                self.library.report = report.assets.clone();
                self.home.visible = false;
                self.home.example_copy = false;
                if add_recent {
                    self.remember_project();
                }
                self.log("Projeto aberto e validado.");
                self.hold_lock(&root);
                if let Some(warning) = tree_files::synced_folder(&root) {
                    self.log(warning);
                    self.notice_last(true);
                }
                if agent_writing {
                    self.log(AGENT_WAS_WRITING);
                    self.notice_last(true);
                }
                if let Some(version) = report.migrated {
                    self.log(format!("Projeto convertido em memória do formato {version}. O original continua intacto. Ao salvar, será criado um backup identificado; versões antigas da engine não abrirão o novo formato."));
                    self.notice_last(true);
                }
                // What opening fixed in memory, which the next save writes.
                for fix in &report.fixes {
                    self.log(fix.clone());
                }
                // What an agent's write that organized the project in folders did, under the
                // Agente mark until the person saves it; and what the first save of a project of
                // the old layout does.
                for line in conversion::agent_note(&root) {
                    self.log(library::agent_note_line(&line));
                    self.notice_last(true);
                }
                if add_recent && report.layout == Layout::Legacy {
                    let path = self.path.clone().expect("Projeto aberto");
                    self.log(conversion::preview(&path, &self.state.project));
                    self.notice_last(true);
                }
                // A file renamed or deleted outside OXY: the project opens, the test and the
                // export wait for it.
                for warning in report.warnings {
                    self.log(warning);
                    self.notice_last(true);
                }
                true
            }
            Err(e) => {
                self.log(format!("Não foi possível abrir: {e}"));
                self.notice_last(false);
                false
            }
        }
    }
    /// Holds the lock of the project in `root` (see [`oxy_core::project_lock`]), letting go of
    /// the last one first.
    fn hold_lock(&mut self, root: &Path) {
        self.lock = None;
        match oxy_core::project_lock::EditorLock::hold(root, self.preferences.allow_agents) {
            Ok(lock) => self.lock = Some(lock),
            Err(error) if oxy_core::fs_ops::busy(&error) => {
                self.log(
                    "O projeto já está aberto em outro editor da OXY; salvar nos dois faz um \
                     apagar o que o outro gravou.",
                );
                self.notice_last(true);
            }
            Err(error) => {
                self.log(format!(
                    "Não foi possível reservar o projeto para este editor ({error}); agentes nos \
                     arquivos poderão gravá-lo enquanto ele estiver aberto aqui."
                ));
                self.notice_last(true);
            }
        }
    }
    /// Asks what to do with unsaved changes before a pending transition.
    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        use crate::widgets;
        let mut choice = None;
        let response = egui::Modal::new("unsaved_changes".into()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.).clamp(160., 440.));
            let title = format!("Salvar as alterações em “{}”?", self.state.project.name);
            if widgets::dialog_header(ui, &title) {
                choice = Some(false);
            }
            widgets::hint(
                ui,
                "Há mudanças no projeto ou nas texturas que ainda não foram salvas. Se \
                 descartar, elas serão perdidas.",
            );
            if !self.studio.animation.drafts.is_empty() {
                ui.add_space(6.);
                widgets::callout(
                    ui,
                    true,
                    "Há uma pose provisória na Animação. Grave-a com + Quadro-chave antes de \
                     salvar, ou cancele para voltar à edição.",
                );
            }
            widgets::dialog_footer(ui, |ui| {
                if widgets::primary_button(ui, "Salvar e continuar", true).clicked() {
                    choice = Some(true);
                }
                if widgets::secondary_button(ui, "Cancelar").clicked() {
                    choice = Some(false);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if widgets::danger_button(ui, "Descartar alterações", false).clicked()
                        && let Some(t) = self.pending.take()
                    {
                        // The files go back to the last save too (decision 2 of the tree).
                        self.discard_before(t);
                    }
                });
            });
        });
        match choice {
            Some(true) => {
                if self.organize_first() {
                    // The first save of an old project asks how to organize it first.
                    let then = self.pending.take();
                    self.organize_then(then);
                } else if self.save()
                    && let Some(t) = self.pending.take()
                {
                    self.apply_transition(t);
                }
            }
            Some(false) => self.pending = None,
            None if response.should_close() => self.pending = None,
            None => {}
        }
    }
    /// Discards the changes, the files' included, then does `t`; when `t` leaves this project
    /// open (another one did not open), it is read again as the files are now.
    fn discard_and_go(&mut self, t: Transition) {
        let current = self.path.clone();
        let files = self.discard_files();
        if !self.apply_transition(t)
            && files
            && let Some(current) = current
        {
            // Its paths and undo steps then match the files.
            self.open_with_recent(current, false);
        }
    }
    /// The person's save; the first one of a project of the old layout asks how to organize it
    /// in folders first, and answers false while it asks.
    pub fn save(&mut self) -> bool {
        if self.organize_first() {
            return false;
        }
        self.save_from("")
    }
    /// Saves, writing the Console line "Salvo: …" with `origin` before it (an agent's prefix).
    pub(crate) fn save_from(&mut self, origin: &str) -> bool {
        // What the person chose in "Organizar o projeto em pastas", for this save only.
        let elsewhere = std::mem::take(&mut self.library.save_elsewhere);
        let including = std::mem::take(&mut self.library.save_including);
        if self.mesh_operation_active() {
            self.warn("Termine o gesto de modelagem ou pressione Esc antes de salvar.");
            return false;
        }
        self.cancel_mesh_operation();
        if !self.history.is_pending() {
            self.history
                .begin("Finalizar edição", &self.state.project, &self.state.images);
        }
        if let Some(mut rename) = self.hierarchy_ui.rename.take() {
            if !self.history.is_pending() {
                self.history
                    .begin("Renomear objeto", &self.state.project, &self.state.images);
            }
            if let Err(error) = editing::rename_entity(self.scene_mut(), &rename.id, &rename.text) {
                self.log(error);
                self.notice_last(true);
                rename.focus = true;
                self.hierarchy_ui.rename = Some(rename);
                return false;
            }
        }
        if !self.studio.animation.drafts.is_empty() {
            self.log("Há uma pose provisória na Animação. Grave-a com + Quadro-chave ou use Descartar pose provisória antes de salvar.");
            self.notice_last(true);
            return false;
        }
        if let Err(error) = self.finish_animation_rename() {
            self.log(error);
            self.notice_last(true);
            return false;
        }
        self.finish_history(true);
        // What changed outside OXY reaches the project first, as the save writes it.
        self.scan_project();
        let previous_path = self.path.clone();
        // A copy in a new folder: an example's, or an old project organized there.
        let elsewhere = elsewhere && self.path.is_some();
        let copy = self.home.example_copy || elsewhere;
        if self.path.is_none() || copy {
            let title = if elsewhere {
                "Escolha a pasta nova do projeto organizado em pastas"
            } else {
                "Escolha a pasta do novo projeto OXY"
            };
            let Some(folder) = rfd::FileDialog::new().set_title(title).pick_folder() else {
                return false;
            };
            match new_project_file(&folder) {
                Ok(path) => self.path = Some(path),
                Err(error) => {
                    self.log(error);
                    self.notice_last(true);
                    return false;
                }
            }
        }
        let path = self.path.as_ref().unwrap().clone();
        let root = path.parent().unwrap_or(Path::new(".")).to_owned();
        let agent_writing = oxy_core::project_lock::wait_for_agents(&root);
        self.converted = None;
        self.kept_layout = None;
        self.undo_restarted = false;
        // The project as saved, when the save organized it in folders: what it tells, and
        // whether its assets' names and paths changed.
        let mut organized = None;
        let result = if copy {
            conversion::save_copy(
                &path,
                &self.state.project,
                &mut self.state.images,
                previous_path
                    .as_deref()
                    .and_then(Path::parent)
                    .expect("Cópia com pasta de origem"),
                &mut self.known_files,
            )
            .map(|copied| {
                if copied.project != self.state.project {
                    organized = Some((copied.project, true));
                }
                copied.warnings
            })
            .map_err(|error| error.to_string())
        } else if self.state.project.layout == Layout::Legacy {
            self.save_organizing(&path, &mut organized, origin.is_empty(), &including)
        } else {
            persistence::save_cached_bundle_with(
                &path,
                &self.state.project,
                &mut self.state.images,
                &mut self.known_files,
            )
            .map_err(|error| error.to_string())
        };
        match result {
            Ok(told) => {
                self.home.example_copy = false;
                if let Some((project, changed)) = organized {
                    self.state.project = project;
                    // The edits before it name the files by their old paths.
                    if changed && (self.history.can_undo() || self.history.can_redo()) {
                        self.history.clear();
                        self.undo_restarted = true;
                        self.log(
                            "O desfazer recomeçou: as edições anteriores citavam os arquivos \
                             pelos nomes antigos.",
                        );
                    }
                }
                self.history.mark_saved();
                // What was new in the folder is the project's now.
                self.library.new.clear();
                conversion::forget_agent_note(&root);
                self.rebase_scan();
                self.remember_project();
                self.renderer.release_saved_texture_pixels();
                self.play.game_ui.release_saved_texture_pixels();
                self.log(format!("{origin}Salvo: {}", path.display()));
                if previous_path.as_ref() != Some(&path) {
                    self.hold_lock(&root);
                }
                if agent_writing {
                    self.log(AGENT_WAS_WRITING);
                    self.notice_last(true);
                }
                for line in told {
                    self.log(line);
                    self.notice_last(true);
                }
                for warning in persistence::script_warnings(&self.state.project, &root) {
                    self.log(warning);
                    self.notice_last(false);
                }
                true
            }
            Err(e) => {
                self.path = previous_path;
                self.log(format!(
                    "Falha ao salvar; o estado de edição foi preservado: {e}"
                ));
                self.notice_last(true);
                false
            }
        }
    }
    /// Saves the project of the old layout at `path` organizing it in folders (see
    /// `conversion::convert`), leaving in `organized` the project as saved; one that cannot be
    /// organized now is saved in the old layout, telling why (also in `kept_layout`). An agent's
    /// save (`by_person` false) is told as the agent's. `including` are the other images,
    /// sounds and models of the folder the person chose to take in. Answers the lines to tell.
    fn save_organizing(
        &mut self,
        path: &Path,
        organized: &mut Option<(Project, bool)>,
        by_person: bool,
        including: &[String],
    ) -> Result<Vec<String>, String> {
        use conversion::ConversionError::{Busy, Failed, Kept, Refused};
        let converted = conversion::convert_including(
            path,
            &self.state.project,
            Some(&mut self.state.images),
            &mut self.known_files,
            if by_person {
                conversion::Who::Person
            } else {
                conversion::Who::Agent
            },
            including,
        );
        let why = match converted {
            Ok(done) => {
                let told = done.lines().cloned().collect();
                self.converted = Some(done.json());
                *organized = Some((done.project, done.changed_assets));
                return Ok(told);
            }
            Err(Kept(why) | Busy(why)) => return Err(why),
            Err(Refused(why) | Failed(why)) => why,
        };
        let mut told = persistence::save_cached_bundle_with(
            path,
            &self.state.project,
            &mut self.state.images,
            &mut self.known_files,
        )
        .map_err(|error| error.to_string())?;
        let kept =
            format!("O projeto foi salvo no formato antigo, sem organizar em pastas: {why}.");
        self.kept_layout = Some(kept.clone());
        told.insert(0, kept);
        Ok(told)
    }
    fn transition(&mut self, transition: Transition) {
        if self.export.running() {
            // The export finishes, and answers, in the project that started it.
            self.warn("Espere a exportação do jogo terminar para trocar de projeto.");
        } else if self.dirty() || !self.studio.animation.drafts.is_empty() {
            self.pending = Some(transition)
        } else {
            self.apply_transition(transition);
        }
    }
    /// Applies `t`; answers whether the open project gave way (to another, or to closing).
    fn apply_transition(&mut self, t: Transition) -> bool {
        match t {
            Transition::Home => {
                self.apply_transition(Transition::Create(
                    "Meu projeto OXY".into(),
                    SceneKind::TwoD,
                ));
                self.home.visible = true;
                true
            }
            Transition::Create(name, kind) => {
                self.home.visible = false;
                self.home.example_copy = false;
                self.state = Snapshot {
                    project: editing::blank_project(&name, kind).expect("Nome validado no diálogo"),
                    images: TextureCache::default(),
                };
                self.path = None;
                self.known_files = Default::default();
                self.lock = None;
                self.tree_scan = None;
                self.reset_context();
                true
            }
            Transition::Open(path) => self.open_with_recent(path, true),
            Transition::Recipe(path) => {
                let opened = self.open_with_recent(path, false);
                if !self.home.visible {
                    self.home.example_copy = true;
                    self.tab = Tab::Logic;
                    self.select(self.scene().entities.first().map(|e| e.id.clone()));
                }
                opened
            }
            Transition::ScriptRecipe(path) => {
                let opened = self.open_with_recent(path, false);
                if !self.home.visible {
                    self.home.example_copy = true;
                    self.tab = Tab::Scene;
                    let scripted = self
                        .scene()
                        .entities
                        .iter()
                        .find(|e| !e.scripts.is_empty())
                        .map(|e| e.id.clone());
                    self.select(scripted);
                }
                opened
            }
            Transition::Laboratory(path) => {
                let opened = self.open_with_recent(path, false);
                if !self.home.visible {
                    self.home.example_copy = true;
                    self.tab = Tab::Scene;
                }
                opened
            }
            Transition::Close => {
                self.allow_close = true;
                true
            }
        }
    }
    fn sync_textures(&mut self) {
        self.camera_tools.invalidate_image();
        self.renderer.clear_textures();
        self.play.game_ui = oxy_render::GameUi::new();
        for (id, pixels) in self.state.images.dirty_images() {
            if let Err(e) = self.renderer.set_texture_pixels(
                &self.render_state,
                id,
                pixels.width,
                pixels.height,
                &pixels.pixels,
                true,
            ) {
                self.console.messages.push(e);
            }
            if let Err(e) = self.play.game_ui.set_texture_pixels(
                &self.context,
                id,
                pixels.width,
                pixels.height,
                &pixels.pixels,
                true,
            ) {
                self.console.messages.push(e);
            }
        }
    }
    pub fn refresh_texture(&mut self, id: &str) {
        self.camera_tools.invalidate_image();
        if self
            .studio
            .texture_key
            .as_ref()
            .is_some_and(|(current, _)| current == id)
        {
            self.studio.texture_key = None;
        }
        if self
            .asset_ui
            .thumbnail
            .as_ref()
            .is_some_and(|(key, _)| key == id)
        {
            self.asset_ui.thumbnail = None;
        }
        if let Some(p) = self.state.images.get(id) {
            if let Err(e) = self.renderer.set_texture_pixels(
                &self.render_state,
                id,
                p.width,
                p.height,
                &p.pixels,
                true,
            ) {
                self.console.messages.push(e);
            }
            if let Err(e) = self.play.game_ui.set_texture_pixels(
                &self.context,
                id,
                p.width,
                p.height,
                &p.pixels,
                true,
            ) {
                self.console.messages.push(e);
            }
        }
    }
    fn finish_history(&mut self, force: bool) {
        if self.mesh_operation_active() {
            return;
        }
        if force && self.history.is_pending() {
            if let Err(error) = self
                .history
                .commit(&self.state.project, &mut self.state.images)
            {
                self.log(error);
                self.notice_last(false);
            }
            self.state
                .images
                .configure(&self.root(), &self.state.project);
        }
        self.validate_last_mesh_operation();
    }
    fn undo(&mut self, redo: bool) {
        let _ = self.undo_step(redo);
    }
    /// [`Self::undo`], answering why nothing was undone (told in the Console too).
    pub(crate) fn undo_step(&mut self, redo: bool) -> Result<(), String> {
        if self.cancel_box_selection() {
            return Ok(());
        }
        if self.cancel_mesh_operation() {
            return Ok(());
        }
        self.forget_last_mesh_operation();
        self.cancel_body_drag();
        self.cancel_spatial_drag();
        self.finish_history(true);
        // A step that moved files: what changed outside OXY is told first, and the walk then
        // follows the files OXY moves back. The test keeps the paths it started with.
        let files = self.history.next_has_files(redo);
        if files && self.playing() {
            let refused = if redo {
                "Pare o teste para refazer uma operação nos arquivos do projeto."
            } else {
                "Pare o teste para desfazer uma operação nos arquivos do projeto."
            };
            self.warn(refused);
            return Err(refused.into());
        }
        if files {
            self.scan_project();
        }
        // What the step moves, which the Library and the code tab follow once it is done.
        let moved = self.history.next_files(redo).to_vec();
        let root = self.project_root();
        let disk = root.as_deref().map(|root| (root, &mut self.known_files));
        let result = if redo {
            self.history
                .redo_with(&mut self.state.project, &mut self.state.images, disk)
        } else {
            self.history
                .undo_with(&mut self.state.project, &mut self.state.images, disk)
        };
        if files {
            self.rebase_scan();
            // The files moved back, and the texts whose `require` the step rewrote went back
            // too: the Scripts card, the Hierarchy and the code tab see them.
            self.script_status.look_again();
        }
        if let Err(error) = result {
            // Files that stayed moved halfway are a step of their own, which the document
            // follows: the images are found at their paths.
            if files {
                self.state
                    .images
                    .configure(&self.root(), &self.state.project);
                self.asset_ui.thumbnail = None;
            }
            self.log(error.clone());
            self.notice_last(false);
            return Err(error);
        } else {
            if !moved.is_empty() {
                self.follow_moves(&moved, !redo);
            }
            if self.state.project.scene(&self.scene_id).is_none() {
                self.scene_id = self.state.project.start_scene.clone()
            };
            if self
                .selected
                .as_ref()
                .is_some_and(|id| self.scene().entity(id).is_none())
            {
                self.select(None);
            }
            let scene = self.scene().clone();
            self.selection.retain_scene(&scene);
            self.state
                .images
                .configure(&self.root(), &self.state.project);
            let changed = self.history.last_texture_changes().to_vec();
            for id in changed {
                self.renderer.clear_texture_override(&id);
                self.play.game_ui.clear_texture_override(&id);
                if self.state.images.is_registered(&id) && self.ensure_texture(&id) {
                    self.refresh_texture(&id);
                }
            }
            self.asset_ui.thumbnail = None;
        }
        Ok(())
    }
    /// Jogar: the person tests the open scene.
    fn start(&mut self) {
        let scene = self.scene_id.clone();
        if let Err(error) = self.start_with(&scene, Driver::Person) {
            self.warn(error);
        }
    }
    /// Starts a test in `scene`, shown in the Scene tab. The person's holds the mouse and reads
    /// the keyboard; an agent's takes neither.
    pub(crate) fn start_with(&mut self, scene: &Id, driver: Driver) -> Result<(), String> {
        if self.mesh_operation_active() {
            return Err("Termine o gesto de modelagem ou pressione Esc antes de jogar.".into());
        }
        self.cancel_mesh_operation();
        self.set_spatial_tool(Tool::Object);
        self.finish_history(true);
        // A new test starts in silence, the Library's Ouvir included.
        self.play.mixer.stop_all();
        // Every test draws differently, as the exported game does. The low half of a v4 UUID
        // has its fixed bits above the 53 kept.
        let seed = uuid::Uuid::new_v4().as_u64_pair().1 & random::MAX_SEED;
        let scripts = test_scripts(&self.state.project, self.path.as_deref())
            .map_err(|error| format!("Não é possível jogar: {error}."))?;
        // An image, sound or model the test uses whose file cannot be read now.
        if let Some(root) = self.path.as_deref().and_then(Path::parent) {
            let images = &self.state.images;
            persistence::assets_to_run(&self.state.project, root, |id| images.is_dirty(id))
                .map_err(|error| format!("Não é possível jogar: {error}."))?;
        }
        let scripts = std::sync::Arc::new(scripts);
        let options = RuntimeOptions {
            scripts: scripts.clone(),
            ..self.play.options(seed)
        };
        let runtime = Runtime::with_options(&self.state.project, scene, options)?;
        self.watch_test_scripts(scripts);
        self.script_status.start_test();
        self.console_errors.clear();
        self.play.seed = Some(seed);
        self.play.previous_tab = self.tab;
        // The new test's logs count from zero.
        self.sync_console_feed();
        self.console_feed.new_test();
        self.play.runtime = Some(runtime);
        self.set_tab(Tab::Scene);
        self.play.driver = driver;
        self.play.capture = driver == Driver::Person;
        self.play.agent_clicks.clear();
        self.play.last_runtime_scene = Some(scene.clone());
        self.frame.last_frame = Instant::now();
        self.studio.playing = false;
        Ok(())
    }
    fn stop(&mut self) {
        // What the last frame saved, and its logs, reach the Console and agents after the test.
        self.take_test_saves();
        self.sync_console_feed();
        if let Some(runtime) = &self.play.runtime {
            let root = self.project_root();
            self.script_status
                .end_test(runtime, &self.state.project, root.as_deref());
        }
        self.play.end();
        // Files changed outside OXY during the test reach the project now.
        self.scan_requested = true;
        self.set_tab(self.play.previous_tab);
    }
    /// Pauses the test. What an agent sent and the game has not reached yet is dropped, as the
    /// runtime drops its timed inputs.
    fn pause(&mut self) {
        self.play.capture = false;
        self.play.agent_clicks.clear();
        if let Some(rt) = &mut self.play.runtime {
            rt.set_paused(true);
        }
        self.update_test_audio();
    }
    /// Resumes a paused test, or goes on with a running one, for `driver`: Retomar and Assumir
    /// give it to the person, with their mouse and keyboard; an agent's resume to the agent.
    /// Inputs an agent sent and the game has not reached yet are dropped, and keys held let go.
    pub(crate) fn resume_for(&mut self, driver: Driver) {
        if self.tab == Tab::Logic {
            self.set_tab(Tab::Scene);
        }
        self.play.driver = driver;
        self.play.capture = driver == Driver::Person;
        self.play.agent_clicks.clear();
        if let Some(rt) = &mut self.play.runtime {
            rt.set_paused(false);
        }
        self.frame.last_frame = Instant::now();
    }
    /// The test's sounds follow it (see `PlaySession::update_audio`); problems go to the Console.
    fn update_test_audio(&mut self) {
        let root = self.root();
        for error in self.play.update_audio(&root) {
            self.log(error);
        }
    }
    pub fn select(&mut self, id: Option<Id>) {
        if self.selected != id {
            self.cancel_box_selection();
            self.cancel_mesh_operation();
            self.forget_last_mesh_operation();
        }
        if self.selected != id {
            self.hierarchy_ui.reveal_scroll = id.clone();
        }
        self.reveal_selection(id.as_deref());
        self.transform_ui.selection_rotation = [0.; 3];
        self.transform_ui.selection_scale = 1.;
        self.selection.single(id.clone());
        if self.selected != id {
            self.selected = id;
            self.studio.shared_edit = None;
        }
    }
    pub(crate) fn select_click(
        &mut self,
        id: Option<Id>,
        modifiers: egui::Modifiers,
        hierarchy: bool,
    ) {
        if self.selected != id {
            self.cancel_box_selection();
            self.cancel_mesh_operation();
            self.forget_last_mesh_operation();
        }
        self.transform_ui.selection_rotation = [0.; 3];
        self.transform_ui.selection_scale = 1.;
        self.hierarchy_ui.focus = hierarchy;
        if let Some(id) = id {
            if !hierarchy {
                self.hierarchy_ui.reveal_scroll = Some(id.clone());
            }
            self.reveal_selection(Some(&id));
            let order = self.visible_hierarchy_order();
            self.selection.click(
                id.clone(),
                modifiers.ctrl || modifiers.command,
                hierarchy && modifiers.shift,
                &order,
            );
            self.selected = if self.selection.ids.contains(&id) {
                Some(id)
            } else {
                self.selection.ids.last().cloned()
            };
        } else {
            self.select(None);
        }
        self.studio.shared_edit = None;
    }
    pub(crate) fn begin_rename(&mut self) {
        if let Some(entity) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene().entity(id))
        {
            self.hierarchy_ui.rename = Some(Rename {
                id: entity.id.clone(),
                text: entity.name.clone(),
                focus: true,
            });
        }
    }
    pub fn add_entity(&mut self, primitive: Option<Primitive>, name: &str) {
        let mut entity = Entity::new(name, primitive);
        if self.tab == Tab::Studio {
            // New pieces join the isolated object: under the selection, else its root.
            entity.parent = self.selected.clone().or_else(|| self.studio.focus.clone());
        }
        let id = entity.id.clone();
        self.scene_mut().entities.push(entity);
        self.select(Some(id));
    }
    fn duplicate(&mut self) {
        let ids = self.selection.ids.clone();
        match editing::duplicate_selection(self.scene_mut(), &ids) {
            Ok(copies) => {
                self.selected = copies.last().cloned();
                self.selection.ids = copies;
            }
            Err(e) => self.log(e),
        }
    }
    fn delete(&mut self) {
        let roots = editing::selection_roots(self.scene(), &self.selection.ids);
        for id in roots {
            self.scene_mut().remove_subtree(&id);
        }
        self.select(None);
    }
    fn group(&mut self) {
        let ids = self.selection.ids.clone();
        match editing::group_selection(self.scene_mut(), &ids) {
            Ok(group) => self.select(Some(group)),
            Err(error) => {
                self.log(error);
                self.notice_last(false);
            }
        }
    }
    fn set_scene(&mut self, id: Id) {
        self.cancel_box_selection();
        self.cancel_mesh_operation();
        self.forget_last_mesh_operation();
        self.set_spatial_tool(Tool::Object);
        self.spatial.fit = None;
        if !self.studio.animation.drafts.is_empty() {
            self.log(
                "Grave a pose com + Quadro-chave ou descarte a pose provisória antes de mudar de cena.",
            );
            self.notice_last(false);
            return;
        }
        self.pause();
        self.scene_id = id;
        self.select(None);
        self.camera = CameraState::for_scene(self.scene());
        self.studio = Studio::default();
        self.graph = GraphView::default();
    }

    pub fn import(&mut self, kind: AssetKind) {
        let _ = self.import_selected(kind);
    }
    pub(crate) fn import_selected(&mut self, kind: AssetKind) -> Option<Id> {
        self.pause();
        if self.path.is_none() && !self.save() {
            return None;
        }
        let dialog = rfd::FileDialog::new();
        let file = if kind == AssetKind::Texture {
            dialog.add_filter("Imagem PNG", &["png"]).pick_file()
        } else {
            dialog.add_filter("Áudio WAV", &["wav"]).pick_file()
        };
        let source = file?;
        // Into the folder the Library shows, as its Importar para <pasta>… does.
        let folder = self.open_folder();
        self.import_files(&[(source, kind)], &folder, true)
            .into_iter()
            .next()
    }
    /// Imports `sources` (each file with its kind) into the folder `folder` of the project's
    /// tree ("" is the root); with `apply`, an image goes on the selected object. In a project in
    /// folders it is its own step: the files it makes go with it (undoing sends them to the
    /// project's trash, redoing brings them back), and a file that fails leaves the others. In
    /// the old layout it joins the edit in progress, as before. Answers the assets made.
    pub(crate) fn import_files(
        &mut self,
        sources: &[(PathBuf, AssetKind)],
        folder: &str,
        apply: bool,
    ) -> Vec<Id> {
        if sources.is_empty() {
            return Vec::new();
        }
        let tree = self.state.project.layout == Layout::Tree;
        let label = match sources.len() {
            1 => "Importar recurso".to_owned(),
            count => format!("Importar {count} recursos"),
        };
        if tree {
            self.finish_history(true);
            // What changed outside OXY is told before the walk takes the import's files.
            self.scan_project();
        }
        if !self.history.is_pending() {
            self.history
                .begin(label.clone(), &self.state.project, &self.state.images);
        }
        let root = self.root();
        let mut files = Vec::new();
        let mut imported = Vec::new();
        for (source, kind) in sources {
            let mut creating = tree
                .then(|| oxy_core::journal::Creating::new(&root, &mut self.known_files, &label));
            let result = persistence::import_asset_into(
                &mut self.state.project,
                &root,
                source,
                *kind,
                folder,
                &mut |path, id| {
                    creating.as_mut().map_or(Ok(()), |c| {
                        c.before(oxy_core::asset_import::Making::File { path, id })
                    })
                },
            );
            files.extend(
                creating
                    .map(|creating| creating.finish(result.is_ok()))
                    .unwrap_or_default(),
            );
            match result {
                Ok(id) => {
                    if *kind == AssetKind::Texture {
                        if let Some(asset) = self.state.project.asset(&id)
                            && let Ok(file) = persistence::resolve_asset_path(&root, &asset.path)
                            && let Ok(image) = PaintImage::load(&file)
                        {
                            self.state.images.insert(id.clone(), image);
                        }
                        self.refresh_texture(&id);
                    }
                    imported.push(id);
                }
                Err(e) => {
                    let name = source
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.log(if sources.len() > 1 {
                        format!("{name}: {e}")
                    } else {
                        e.to_string()
                    });
                    self.notice_last(false);
                }
            }
        }
        if apply
            && let Some(id) = imported.first().cloned()
            && self
                .state
                .project
                .asset(&id)
                .is_some_and(|a| a.kind == AssetKind::Texture)
        {
            let ratio = self
                .state
                .images
                .get(&id)
                .map(|p| p.width as f32 / p.height as f32);
            if let Some(entity) = self
                .selected
                .clone()
                .and_then(|selected| self.scene_mut().entity_mut(&selected))
            {
                entity.material.texture = Some(id.clone());
                if entity.primitive == Some(Primitive::Sprite)
                    && let Some(ratio) = ratio
                {
                    entity.dimensions[0] = entity.dimensions[1] * ratio;
                }
            }
        }
        if tree {
            if imported.is_empty() {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
            } else {
                if let Err(error) = self.history.commit_with_files(
                    &self.state.project,
                    &mut self.state.images,
                    files,
                ) {
                    self.log(error);
                }
                self.rebase_scan();
                // What the panel that asked for it changes next is another edit.
                self.history
                    .begin("Editar projeto", &self.state.project, &self.state.images);
            }
        }
        match imported.len() {
            0 => {}
            1 => self.log("Recurso importado como cópia local."),
            count => self.log(format!("{count} recursos importados como cópias locais.")),
        }
        imported
    }
    fn game(&mut self, ui: &mut egui::Ui, dt: f32) {
        if self.play.runtime.is_none() {
            return;
        }
        let (focused, escape, pressing) = ui.input(|i| {
            (
                i.focused,
                i.key_pressed(egui::Key::Escape),
                i.pointer.any_down() || i.pointer.any_released(),
            )
        });
        // Losing focus pauses the person's test, unless Preferências do editor › Teste keep it
        // going without their input; an agent's plays on, as its inputs need no window. Esc
        // opens and closes the game's pause menu when the scene has one, as in the exported
        // game: capture stays on so the menu takes clicks, and the cursor goes free
        // (wants_relative_mouse), so Pausar and Parar stay at hand.
        self.play.follow_pointer(pressing);
        if self
            .play
            .follow_focus(focused, self.preferences.play_unfocused)
        {
            self.pause();
        } else if focused && escape {
            let menu = self.play.capture
                && self
                    .play
                    .runtime
                    .as_ref()
                    .is_some_and(|rt| !rt.paused && rt.has_pause_menu());
            if !menu {
                self.pause();
            } else if let Some(rt) = &mut self.play.runtime {
                rt.toggle_game_pause();
            }
        }
        let relative = self
            .play
            .runtime
            .as_ref()
            .is_some_and(Runtime::wants_relative_mouse);
        let available = [
            ui.available_width().max(1.) as u32,
            ui.available_height().max(1.) as u32,
        ];
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size().max(Vec2::splat(1.)), Sense::click());
        // A press on one of the game's buttons, or on the editor around the game, is not a
        // mouse action of the game.
        let input = oxy_render::input::collect_game_input_where(
            ui.ctx(),
            &self.state.project.input_bindings,
            self.play.capture,
            relative,
            |p| {
                rect.contains(p)
                    && self.play.runtime.as_ref().is_some_and(|rt| {
                        oxy_render::game_ui::button_at(rt.scene(), rect, p).is_none()
                    })
            },
        );
        let input = self.play.game_input(input);
        let resized = self.play.game_size != Some(available);
        self.play.game_size = Some(available);
        let play = &mut self.play;
        if let Some(rt) = &mut play.runtime {
            while play
                .agent_clicks
                .front()
                .is_some_and(|(at, _)| *at <= rt.time)
            {
                let (_, direct) = play.agent_clicks.pop_front().unwrap();
                direct.apply(rt);
            }
            rt.set_viewport_aspect(available[0] as f32 / available[1] as f32);
            rt.advance(if resized { 0. } else { dt }, &input);
            // A scene change or restart drops what the agent sent, as it drops timed inputs.
            if !std::mem::take(&mut rt.scene_starts).is_empty() {
                play.agent_clicks.clear();
            }
        }
        let scene = self.play.runtime.as_ref().unwrap().scene().clone();
        let camera = CameraState::for_runtime(self.play.runtime.as_ref().unwrap());
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        let physical = [
            (rect.width() * ui.ctx().pixels_per_point()).max(1.) as u32,
            (rect.height() * ui.ctx().pixels_per_point()).max(1.) as u32,
        ];
        self.renderer.show_grid = false;
        let texture = self.renderer.render(
            &self.render_state,
            &self.state.project,
            &scene,
            &self.root(),
            &camera,
            physical,
            None,
            self.view.debug,
        );
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        let clicks = self
            .play
            .game_ui
            .draw(ui, &self.state.project, &scene, &self.root(), rect);
        self.renderer.draw_runtime_colliders(
            ui,
            &scene,
            &camera,
            rect,
            &[],
            self.view.show_disabled_colliders,
            self.play.runtime.as_ref().and_then(Runtime::physics_world),
        );
        // The click that brought the focus back is the window's, not the game's.
        if self.play.capture
            && !self.play.refocus_click
            && oxy_render::input::accepts_game_click(ui.ctx())
        {
            let mut targets = clicks;
            if targets.is_empty()
                && response.clicked()
                && let Some(p) = response.interact_pointer_pos()
                && let Some(hit) =
                    oxy_render::pick(&scene, &camera, size, [p.x - rect.min.x, p.y - rect.min.y])
            {
                targets.push(hit.entity);
            }
            if let Some(rt) = &mut self.play.runtime {
                for id in targets {
                    rt.click(&id);
                }
            }
        }
        self.update_test_audio();
        self.take_test_saves();
        // Sair do jogo ends the test as Parar does; the exported game would close.
        if self
            .play
            .runtime
            .as_ref()
            .is_some_and(|rt| rt.quit_requested)
        {
            self.stop();
            self.log("O jogo pediu para sair; o teste foi encerrado.");
        }
    }
    /// Console on the home screen, where the editor dock does not exist.
    pub(super) fn home_console(&mut self, ctx: &egui::Context) {
        if !self.console_visible() {
            return;
        }
        egui::TopBottomPanel::bottom("console")
            .resizable(true)
            .default_height(155.)
            .height_range(80.0..=400.0)
            .show(ctx, |ui| self.console_body(ui));
    }
    pub(crate) fn clear_console(&mut self) {
        self.console.clear();
        if let Some(rt) = &mut self.play.runtime {
            rt.logs.clear();
        }
        self.console_feed.clear();
    }
}

impl eframe::App for Editor {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        self.navigation_input(ctx, input);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_started = Instant::now();
        if !self.play.capture || !self.showing_game() {
            oxy_render::input::release_cursor(ctx);
        }
        self.frame.interval_ms = self.frame.last_frame.elapsed().as_secs_f32() * 1000.;
        let dt = (self.frame.interval_ms / 1000.).min(0.1);
        self.frame.last_frame = Instant::now();
        if (ctx.zoom_factor() - self.scale).abs() > 0.0001 {
            ctx.set_zoom_factor(self.scale);
        }
        self.agent_frame(ctx);
        // Between two steps: the scripts saved since the last frame reach the test.
        self.reload_test_scripts();
        let mut close_requested =
            ctx.input(|i| i.viewport().close_requested()) && !self.allow_close;
        if close_requested && self.export.running() {
            // The game's folder is put in place at the end; closing now would leave the work
            // folder behind.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.warn("Espere a exportação do jogo terminar para fechar o editor.");
            close_requested = false;
        }
        if self.home.visible {
            self.stop_navigation(ctx);
            self.home_ui(ctx);
            self.project_dialogs(ctx);
            self.preferences_ui(ctx);
            self.logic_dialogs(ctx);
            self.notices_ui(ctx);
            return;
        }
        // The window got the focus back, or F5: the tree is walked again between two steps.
        let focused = ctx.input(|i| i.focused);
        if focused && !self.was_focused {
            self.scan_requested = true;
            self.script_status.look_again();
        }
        self.was_focused = focused;
        if self.scan_requested && !self.history.is_pending() && !self.playing() {
            self.scan_project();
        }
        self.refresh_scripts();
        let edit_event = ctx.input(|i| {
            i.events.iter().any(|e| match e {
                egui::Event::PointerButton {
                    button,
                    pressed: true,
                    ..
                } => !self.navigation.owns_rmb || *button != egui::PointerButton::Secondary,
                egui::Event::Key { pressed: true, .. } | egui::Event::Text(_) => true,
                _ => false,
            })
        });
        if !self.play.capture && !self.navigation.active && edit_event && !self.history.is_pending()
        {
            self.history
                .begin("Editar projeto", &self.state.project, &self.state.images);
        }
        if close_requested {
            if self.dirty() || !self.studio.animation.drafts.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Transition::Close);
            } else {
                self.allow_close = true
            }
        }
        if self.allow_close {
            self.stop_navigation(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !ctx.input(|i| i.focused) {
            self.cancel_box_selection();
            if self.mesh_input_gesture_active() {
                self.cancel_mesh_operation();
            }
        }
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let body_cancelled = (escape
            || !ctx.input(|i| i.focused)
            || self.tab != Tab::Scene
            || self.body_tools.active != self.selected
            || self.pending.is_some()
            || self.pending_preferences.is_some())
            && self.cancel_body_drag();
        let camera_cancelled = (escape || !ctx.input(|i| i.focused)) && self.cancel_camera_drag();
        let cancel_undo = self.mesh_operation_active()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
        let mesh_cancelled = (escape || cancel_undo)
            && (self.cancel_box_selection() || self.cancel_mesh_operation());
        let dialog_open = body_cancelled
            || self.body_dragging()
            || camera_cancelled
            || self.camera_dragging()
            || self.modeling.creation.is_some()
            || mesh_cancelled
            || self.modeling.help
            || self.pending_preferences.is_some()
            || self.new_project.is_some()
            || self.scene_dialog.is_some()
            || self.export.is_open()
            || self.pending.is_some()
            || self.logic_ui.inputs
            || self.logic_ui.variables_open()
            || self.logic_ui.guide
            || self.library.dialog_open();
        if dialog_open && self.play.capture {
            self.pause();
            oxy_render::input::release_cursor(ctx);
        }
        if !dialog_open
            && !self.navigation.active
            && !self.mesh_operation_active()
            && !self.play.capture
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save_requested = true;
        }
        self.follow_library_focus(ctx);
        if !dialog_open
            && !self.navigation.active
            && !self.play.capture
            && ctx.input(|i| i.focused)
            && !crate::graph_ui::text_input_active(ctx)
            && !self.mesh_shortcuts(ctx)
        {
            if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
                self.open_guide("");
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F5)) && !self.playing() {
                self.scan_requested = true;
                self.scan_forced = true;
                self.script_status.look_again();
            }
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                )
            }) {
                self.undo(true);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                self.undo(false);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)) {
                self.undo(true);
            }
            let animation = self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation;
            if self.library_focused() {
                // The last click was in the Library: its items take the keys.
                self.library_keys(ctx);
            } else if self.tab != Tab::Logic
                && !self.playing()
                && (!animation || self.hierarchy_ui.focus)
            {
                if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                    self.delete();
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D)) {
                    self.duplicate();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                    self.begin_rename();
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                    self.selection.ids = if self.tab == Tab::Studio {
                        self.studio_subtree()
                    } else {
                        self.scene().entities.iter().map(|e| e.id.clone()).collect()
                    };
                    self.selected = self.selection.ids.last().cloned();
                }
            }
            if matches!(self.tab, Tab::Scene | Tab::Studio)
                && ctx.input(|i| i.key_pressed(egui::Key::Escape))
                && !self.cancel_spatial_drag()
            {
                self.set_spatial_tool(Tool::Object);
            }
            if matches!(self.tab, Tab::Scene | Tab::Studio) && ctx.input(|i| i.modifiers.is_none())
            {
                if ctx.input(|i| i.key_pressed(egui::Key::C)) {
                    self.set_spatial_tool(Tool::Collider);
                }
                if ctx.input(|i| i.key_pressed(egui::Key::P)) {
                    self.set_spatial_tool(Tool::Pivot);
                }
                if ctx.input(|i| i.key_pressed(egui::Key::W)) {
                    self.set_spatial_tool(Tool::Object);
                    self.transform_ui.gizmo = Gizmo::Move;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::E)) {
                    self.set_spatial_tool(Tool::Object);
                    self.transform_ui.gizmo = Gizmo::Rotate;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::R)) {
                    self.set_spatial_tool(Tool::Object);
                    self.transform_ui.gizmo = Gizmo::Scale;
                }
            }
        }
        if !self.navigation.active
            && (!self.navigation.owns_rmb || ctx.input(|i| i.pointer.primary_down()))
            && ctx.input(|i| i.pointer.any_down())
            && !self.history.is_pending()
        {
            self.history
                .begin("Gesto de edição", &self.state.project, &self.state.images);
        }
        self.toolbar(ctx);
        if !self.play.capture || !self.showing_game() {
            oxy_render::input::release_cursor(ctx);
        }
        self.status_bar(ctx);
        let compact = Self::compact_layout(ctx);
        let panel = if self.mesh_operation_active() {
            CompactPanel::Properties
        } else {
            self.compact_panel
        };
        let animation = self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation;
        let dock = !animation && (!compact || panel == CompactPanel::Library);
        match self.tab {
            Tab::Scene | Tab::Studio => {
                if !compact || (panel == CompactPanel::Hierarchy && !animation) {
                    self.hierarchy(ctx);
                }
                if !compact || panel == CompactPanel::Properties {
                    if animation {
                        self.animation_properties_panel(ctx);
                    } else {
                        self.properties(ctx);
                    }
                }
                // Side panels first: the dock then spans only the central column.
                if dock {
                    self.dock(ctx);
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    if self.showing_game() {
                        self.game(ui, dt);
                    } else if self.tab == Tab::Studio {
                        self.studio_ui(ui, dt);
                    } else {
                        self.viewport(ui, false);
                    }
                });
            }
            Tab::Logic => {
                if !compact || panel == CompactPanel::Hierarchy {
                    self.hierarchy(ctx);
                }
                if !compact || panel == CompactPanel::Properties {
                    self.logic_properties_panel(ctx);
                }
                if dock {
                    self.dock(ctx);
                }
                egui::CentralPanel::default().show(ctx, |ui| self.logic_view(ui));
            }
        }
        self.camera_preview_window(ctx);
        self.library_dialogs(ctx);
        self.fit_dialog(ctx);
        self.diagnostics_ui(ctx);
        self.project_dialogs(ctx);
        self.export_dialog(ctx);
        self.preferences_ui(ctx);
        self.logic_dialogs(ctx);
        self.mesh_help(ctx);
        self.mesh_creation_dialog(ctx);
        self.notices_ui(ctx);
        self.finish_navigation(ctx);
        if self.pending.is_some() {
            self.pause();
            self.unsaved_dialog(ctx);
        }
        if self.save_requested {
            self.save_requested = false;
            self.save();
        }
        if !ctx.input(|i| i.pointer.any_down()) && !crate::graph_ui::text_input_active(ctx) {
            self.finish_history(true);
        } else if crate::graph_ui::text_input_active(ctx) && !self.history.is_pending() {
            self.history
                .begin("Editar valores", &self.state.project, &self.state.images);
        }
        let errors = self.renderer.take_errors();
        for error in errors {
            self.log(error);
            self.notice_last(false);
        }
        for error in self.play.game_ui.take_errors() {
            self.log(error);
            self.notice_last(false);
        }
        let active_textures: Vec<Id> = if matches!(self.tab, Tab::Scene | Tab::Studio) {
            self.selected
                .as_deref()
                .and_then(|id| self.scene().entity(id))
                .map(|e| {
                    e.material
                        .texture
                        .iter()
                        .chain(e.ui.iter().filter_map(|u| u.texture.as_ref()))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if let Err(error) = self
            .state
            .images
            .pin_only(active_textures.iter().map(String::as_str))
            && self.console.messages.last() != Some(&error)
        {
            self.log(error);
        }
        self.frame.cpu_ms = frame_started.elapsed().as_secs_f32() * 1000.;
        // The sound device rests a second after the last sound, the Library's Ouvir and a
        // stopped test included; frames come back to let it.
        let sounding = self.play.mixer.rest_when_quiet();
        if let Some(delay) = diagnostics::repaint_delay(
            self.play.runtime.as_ref().is_some_and(|r| !r.paused) && self.showing_game(),
            self.studio.playing
                && self.tab == Tab::Studio
                && self.studio.tab == StudioTab::Animation,
            ctx.input(|i| i.pointer.any_down()),
            self.frame.visible,
        )
        .or(sounding.then_some(std::time::Duration::from_millis(500)))
        {
            ctx.request_repaint_after(delay);
        }
    }
}

// The HSV conversion inside egui can round untouched RGB values. Only author a color on input.
pub fn color_editor(ui: &mut egui::Ui, value: &mut [f32; 4]) {
    let mut candidate = *value;
    if ui
        .color_edit_button_rgba_unmultiplied(&mut candidate)
        .changed()
    {
        *value = candidate;
    }
}
pub fn vector3(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64, nonzero: bool) {
    ui.label(label).on_hover_text(match label {
        "Pivô local" => "Ponto em torno do qual a peça gira e escala.",
        "Posição" => "Distância da origem nos eixos X, Y e Z.",
        "Rotação °" => "Giro em graus nos eixos da peça.",
        "Escala" => "Multiplica o tamanho sem alterar a forma original.",
        _ => "Ajuste os valores de cada eixo. X é horizontal e Y aponta para cima.",
    });
    ui.horizontal(|ui| {
        for (i, value) in values.iter_mut().enumerate() {
            ui.add(
                egui::DragValue::new(value)
                    .speed(speed)
                    .prefix(["X ", "Y ", "Z "][i])
                    .max_decimals(3),
            );
            if nonzero && value.abs() < 0.0001 {
                *value = 0.0001;
            }
        }
    });
}

/// The project file of a new project in `folder`, as the person chose it: a folder with nothing
/// in it, since the project's tree is its Library and every file in it would be the project's;
/// why not, with what to do.
fn new_project_file(folder: &Path) -> Result<PathBuf, String> {
    let path = folder.join(persistence::PROJECT_FILE);
    if path.exists() {
        return Err(
            "Já existe project.oxy.json nessa pasta. Abra-o ou escolha outra pasta.".into(),
        );
    }
    let empty = std::fs::read_dir(folder)
        .map_err(|error| format!("Não foi possível ler a pasta escolhida: {error}"))?
        .next()
        .is_none();
    if !empty {
        return Err(format!(
            "A pasta {} já tem outros arquivos; um projeto novo vai numa pasta vazia, só dele. \
             Crie dentro dela uma pasta com o nome do projeto e escolha essa.",
            folder.display()
        ));
    }
    Ok(path)
}

/// What the Console says when an agent's tool was still writing the project after the editor
/// waited for it to open or save it.
const AGENT_WAS_WRITING: &str = "Um agente estava gravando este projeto nos arquivos; se algo \
    parecer faltar, reabra o projeto.";

/// The scripts a test of `project`, saved at `path`, runs: those of its tree, every one its
/// objects attach among them (see `persistence::load_scripts_to_run`). A project never saved
/// has no folder for them.
fn test_scripts(project: &Project, path: Option<&Path>) -> Result<ScriptSources, String> {
    match path.and_then(Path::parent) {
        Some(root) => persistence::load_scripts_to_run(project, root),
        None if oxy_core::script::any_attached(project) => Err(
            "o projeto ainda não foi salvo numa pasta, onde ficam os seus scripts; salve-o antes"
                .into(),
        ),
        None => Ok(ScriptSources::default()),
    }
}

#[cfg(all(test, target_os = "windows"))]
mod native_perf;

#[cfg(test)]
mod play_tests {
    use super::*;
    use oxy_core::graph::{Edge, Node};

    /// A project in a new folder whose scene plays Bip, 50 ms long, as it starts.
    fn beeping() -> (Project, PathBuf) {
        let root = std::env::temp_dir().join(format!("oxy-editor-sound-{}", new_id()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = hound::WavWriter::create(root.join("assets/bip.wav"), spec).unwrap();
        for _ in 0..400 {
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
        (project, root)
    }

    #[test]
    fn parar_and_another_project_silence_the_test() {
        let (project, root) = beeping();
        let scene = project.start_scene.clone();
        let mut play = PlaySession {
            mixer: oxy_audio::Mixer::silent(),
            ..Default::default()
        };
        let jogar = |play: &mut PlaySession| {
            play.runtime = Some(Runtime::with_options(&project, &scene, play.options(7)).unwrap());
            assert!(play.update_audio(&root).is_empty());
            assert_eq!(play.mixer.voice_count(), 1);
        };
        let paused = |play: &mut PlaySession, paused| {
            play.runtime.as_mut().unwrap().set_paused(paused);
            assert!(play.update_audio(&root).is_empty());
        };
        let later = |play: &mut PlaySession| {
            std::thread::sleep(std::time::Duration::from_millis(120));
            assert!(play.update_audio(&root).is_empty());
        };
        jogar(&mut play);
        // Pausar freezes the sound, which holds its place past its length; Retomar lets it end.
        paused(&mut play, true);
        later(&mut play);
        assert_eq!(play.mixer.voice_count(), 1);
        paused(&mut play, false);
        assert_eq!(play.mixer.voice_count(), 1);
        later(&mut play);
        assert_eq!(play.mixer.voice_count(), 0);
        // Parar silences what plays.
        jogar(&mut play);
        play.end();
        assert!(play.runtime.is_none());
        assert_eq!(play.mixer.voice_count(), 0);
        jogar(&mut play);
        play.forget_project();
        assert_eq!(play.mixer.voice_count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_test_runs_the_scripts_of_the_folder_and_waits_for_a_missing_one() {
        let root = std::env::temp_dir().join(format!("oxy-editor-scripts-{}", new_id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(
            root.join("scripts/contador.luau"),
            "local M = {}\nfunction M.on_start(self) self.attr.Vezes += 1 end\nreturn M\n",
        )
        .unwrap();
        let mut project = Project::new("Scripts");
        let mut counter = Entity::new("Contador", None);
        counter.attributes.insert("Vezes".into(), Value::Number(0.));
        counter.scripts = vec![oxy_core::script::ScriptRef::new("scripts/contador.luau")];
        let id = counter.id.clone();
        project.scenes[0].entities.push(counter);
        let path = root.join(persistence::PROJECT_FILE);
        let play = PlaySession::default();
        // Jogar: the test runs what the folder holds.
        let scripts = test_scripts(&project, Some(&path)).unwrap();
        let options = RuntimeOptions {
            scripts: std::sync::Arc::new(scripts),
            ..play.options(7)
        };
        let runtime = Runtime::with_options(&project, &project.start_scene, options).unwrap();
        assert_eq!(
            runtime.scene().entity(&id).unwrap().attributes["Vezes"],
            Value::Number(1.)
        );
        // A script taken away outside OXY stops the next test until it is back.
        std::fs::remove_file(root.join("scripts/contador.luau")).unwrap();
        assert_eq!(
            test_scripts(&project, Some(&path)).unwrap_err(),
            "Contador: o script scripts/contador.luau não existe na pasta do projeto; restaure o \
             arquivo ou remova o script do objeto"
        );
        // A project never saved has no folder for scripts; without any, it plays as always.
        assert!(
            test_scripts(&project, None)
                .unwrap_err()
                .contains("ainda não foi salvo")
        );
        project.scenes[0].entities[0].scripts.clear();
        assert!(test_scripts(&project, None).unwrap().files.is_empty());
        assert!(
            test_scripts(&project, Some(&path))
                .unwrap()
                .files
                .is_empty()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A person's test of a scene whose Contador counts the presses and releases of its input
    /// actions, and its steps, through a script.
    fn counting() -> (PlaySession, Id) {
        const PATH: &str = "scripts/contador.luau";
        let mut project = Project::new("Foco");
        let mut counter = Entity::new("Contador", None);
        for name in ["Apertou", "Soltou", "Passos"] {
            counter.attributes.insert(name.into(), Value::Number(0.));
        }
        counter.scripts = vec![oxy_core::script::ScriptRef::new(PATH)];
        let id = counter.id.clone();
        project.scenes[0].entities.push(counter);
        let scripts = ScriptSources::from_texts([(
            PATH,
            "local M = {}\nfunction M.on_input(self, action, phase)\n    if phase == \
             \"pressed\" then\n        self.attr.Apertou += 1\n    else\n        \
             self.attr.Soltou += 1\n    end\nend\nfunction M.on_step(self)\n    \
             self.attr.Passos += 1\nend\nreturn M\n",
        )]);
        let mut play = PlaySession {
            mixer: oxy_audio::Mixer::silent(),
            ..Default::default()
        };
        let options = RuntimeOptions {
            scripts: std::sync::Arc::new(scripts),
            ..play.options(7)
        };
        play.runtime =
            Some(Runtime::with_options(&project, &project.start_scene, options).unwrap());
        (play, id)
    }

    /// One frame of the test with what the person's keyboard and mouse `gave`; the counts of
    /// Contador after it: presses, releases and steps.
    fn frame(play: &mut PlaySession, id: &str, gave: InputFrame) -> [f64; 3] {
        let input = play.game_input(gave);
        let runtime = play.runtime.as_mut().unwrap();
        runtime.advance(oxy_core::runtime::FIXED_DT, &input);
        let entity = runtime.scene().entity(id).unwrap();
        ["Apertou", "Soltou", "Passos"].map(|name| match entity.attributes[name] {
            Value::Number(n) => n,
            ref other => panic!("{other:?}"),
        })
    }

    fn holding(actions: &[&str], pressed: &[&str]) -> InputFrame {
        InputFrame {
            held: actions.iter().map(|a| (*a).to_owned()).collect(),
            pressed: pressed.iter().map(|a| (*a).to_owned()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn losing_the_focus_pauses_the_persons_test_unless_the_preference_keeps_it_going() {
        let (mut play, _) = counting();
        // With the focus, nothing changes.
        assert!(!play.follow_focus(true, false));
        assert!(!play.unfocused && !play.focus_paused);
        // Off (the default): the person's test pauses, and the Pausado chip can say why.
        assert!(play.follow_focus(false, false));
        assert!(play.focus_paused && !play.unfocused);
        play.runtime.as_mut().unwrap().set_paused(true);
        assert!(play.follow_focus(false, false), "it stays paused");
        assert!(play.focus_paused);
        // The focus back keeps the pause and why, until Retomar runs the test again.
        assert!(!play.follow_focus(true, false) && play.focus_paused);
        play.runtime.as_mut().unwrap().set_paused(false);
        assert!(!play.follow_focus(true, false) && !play.focus_paused);
        // A test paused for another reason, Esc or Pausar, keeps today's chip.
        play.runtime.as_mut().unwrap().set_paused(true);
        assert!(play.follow_focus(false, false) && !play.focus_paused);
        play.runtime.as_mut().unwrap().set_paused(false);
        // On: the person's test goes on, without their input, until the focus comes back.
        assert!(!play.follow_focus(false, true));
        assert!(play.unfocused && !play.focus_paused);
        assert!(!play.follow_focus(true, true) && !play.unfocused);
        // A paused test stays paused, as it is, with the preference on too.
        play.runtime.as_mut().unwrap().set_paused(true);
        assert!(!play.follow_focus(false, true) && !play.unfocused);
        play.runtime.as_mut().unwrap().set_paused(false);
        // An agent's test plays on, either way.
        play.driver = Driver::Agent;
        for play_on in [false, true] {
            assert!(!play.follow_focus(false, play_on));
            assert!(!play.unfocused && !play.focus_paused);
        }
        // Parar forgets it all.
        play.driver = Driver::Person;
        play.follow_focus(false, true);
        play.end();
        assert!(!play.unfocused && !play.focus_paused && play.held.is_empty());
    }

    #[test]
    fn the_press_that_brings_the_focus_back_is_the_windows_not_a_click_of_the_game() {
        let (mut play, _) = counting();
        // The test goes on without the focus; the person clicks the window to come back.
        assert!(!play.follow_focus(false, true));
        play.follow_pointer(false);
        assert!(!play.follow_focus(true, true));
        assert!(play.refocus_click && !play.unfocused);
        // The press comes in the next frame, is held and let go: still the window's.
        for _ in 0..3 {
            play.follow_pointer(true);
            assert!(!play.follow_focus(true, true) && play.refocus_click);
        }
        // The first frame without a button ends it: the next clicks are the game's.
        play.follow_pointer(false);
        assert!(!play.follow_focus(true, true) && !play.refocus_click);
        // Back with the keyboard instead, the frame after the focus ends it.
        play.follow_focus(false, true);
        play.follow_pointer(false);
        play.follow_focus(true, true);
        assert!(play.refocus_click);
        play.follow_pointer(false);
        assert!(!play.refocus_click);
        // A test that was not going on without the focus takes the clicks as before: one that
        // paused (the preference off, or Pausar before leaving), and an agent's.
        assert!(play.follow_focus(false, false));
        play.runtime.as_mut().unwrap().set_paused(true);
        play.follow_focus(true, false);
        assert!(!play.refocus_click);
        play.follow_focus(false, true);
        play.follow_focus(true, true);
        assert!(!play.refocus_click);
        play.runtime.as_mut().unwrap().set_paused(false);
        play.driver = Driver::Agent;
        play.follow_focus(false, true);
        play.follow_focus(true, true);
        assert!(!play.refocus_click);
        // Parar forgets it.
        play.driver = Driver::Person;
        play.follow_focus(false, true);
        play.follow_focus(true, true);
        play.end();
        assert!(!play.refocus_click);
    }

    #[test]
    fn a_test_going_on_without_the_focus_lets_go_of_what_was_held_and_takes_no_input() {
        let (mut play, id) = counting();
        // The person holds Andar and the mouse's Atirar.
        assert_eq!(
            frame(
                &mut play,
                &id,
                holding(&["Andar", "Atirar"], &["Andar", "Atirar"])
            ),
            [2., 0., 1.]
        );
        assert_eq!(
            frame(&mut play, &id, holding(&["Andar", "Atirar"], &[])),
            [2., 0., 2.]
        );
        // The focus leaves: both are let go in that frame, and the game goes on.
        assert!(!play.follow_focus(false, true));
        assert_eq!(
            frame(&mut play, &id, holding(&["Andar"], &["Pular"])),
            [2., 2., 3.]
        );
        // Without the focus no key or button reaches the game, and nothing is let go twice.
        assert!(!play.follow_focus(false, true));
        assert_eq!(
            frame(&mut play, &id, holding(&["Andar"], &["Pular"])),
            [2., 2., 4.]
        );
        // The focus back: the person's input reaches the game again.
        assert!(!play.follow_focus(true, true));
        assert_eq!(
            frame(&mut play, &id, holding(&["Pular"], &["Pular"])),
            [3., 2., 5.]
        );
        // Off, the test pauses instead: no step, and no input.
        assert!(play.follow_focus(false, false));
        play.runtime.as_mut().unwrap().set_paused(true);
        assert_eq!(frame(&mut play, &id, InputFrame::default()), [3., 2., 5.]);
    }

    #[test]
    fn a_new_project_goes_in_an_empty_folder() {
        let root = std::env::temp_dir().join(format!("oxy-editor-new-{}", new_id()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            new_project_file(&root),
            Ok(root.join(persistence::PROJECT_FILE))
        );
        // A folder with anything in it, the person's notes or a folder starting with a dot.
        std::fs::create_dir(root.join(".git")).unwrap();
        let refused = new_project_file(&root).unwrap_err();
        assert!(
            refused.starts_with(&format!(
                "A pasta {} já tem outros arquivos",
                root.display()
            )) && refused
                .ends_with("Crie dentro dela uma pasta com o nome do projeto e escolha essa."),
            "{refused}"
        );
        std::fs::write(root.join(persistence::PROJECT_FILE), "{}").unwrap();
        assert_eq!(
            new_project_file(&root).unwrap_err(),
            "Já existe project.oxy.json nessa pasta. Abra-o ou escolha outra pasta."
        );
        assert!(new_project_file(&root.join("nenhuma")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
