mod body_tools;
mod camera_tools;
mod diagnostics;
mod hierarchy;
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
mod spatial_tools;
mod textures;
mod toolbar;
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
    document::*,
    edit_history::CommandHistory,
    editing::{self, Selection},
    painting::PaintImage,
    persistence,
    runtime::Runtime,
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
}
/// Bottom dock shared by the Library, the Console and the notice history.
pub(crate) struct Dock {
    pub(crate) open: bool,
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
/// The isolated play test. While it runs, the central area shows the game.
pub(crate) struct PlaySession {
    pub(crate) runtime: Option<Runtime>,
    pub(crate) game_ui: oxy_render::game_ui::GameUi,
    pub(crate) capture: bool,
    pub(crate) previous_tab: Tab,
    pub(crate) last_runtime_scene: Option<Id>,
    pub(crate) game_size: Option<[u32; 2]>,
}
impl Default for PlaySession {
    fn default() -> Self {
        Self {
            runtime: None,
            game_ui: Default::default(),
            capture: false,
            previous_tab: Tab::Scene,
            last_runtime_scene: None,
            game_size: None,
        }
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
    pub(crate) search: String,
    pub(crate) thumbnail: Option<(Id, egui::TextureHandle)>,
    pub(crate) locate: Option<Id>,
    pub(crate) delete: Option<Id>,
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
    Laboratory(PathBuf),
    Close,
}

pub struct Editor {
    home: home::Home,
    notices: notices::Notices,
    preferences: preferences::Preferences,
    pending_preferences: Option<preferences::Preferences>,
    navigation: navigation::Navigation,
    body_tools: body_tools::BodyTools,
    new_project: Option<scenes::NewProject>,
    scene_dialog: Option<scenes::SceneDialog>,
    logic_ui: logic::LogicUi,
    modeling: modeling::ModelState,
    physics_ui: physics::PhysicsUi,
    pub(crate) camera_tools: camera_tools::CameraTools,
    compact_panel: CompactPanel,
    pub state: Snapshot,
    history: CommandHistory,
    pub path: Option<PathBuf>,
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
    save_requested: bool,
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
        self.dock.open && self.dock.tab == DockTab::Console
    }
    pub(crate) fn show_dock(&mut self, tab: DockTab) {
        self.dock.open = true;
        self.dock.tab = tab;
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
            navigation: Default::default(),
            body_tools: Default::default(),
            new_project: None,
            scene_dialog: None,
            logic_ui: Default::default(),
            modeling: Default::default(),
            physics_ui: Default::default(),
            camera_tools: Default::default(),
            compact_panel: Default::default(),
            history: CommandHistory::new(),
            state,
            path: None,
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
            },
            dock: Dock {
                open: true,
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
            save_requested: false,
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
        self.console.messages.push(message.into());
        if self.console.messages.len() > 600 {
            self.console.messages.drain(..100);
        }
    }
    fn dirty(&self) -> bool {
        self.history.is_dirty()
            || self
                .history
                .pending_changed(&self.state.project, &self.state.images)
            || self.state.images.has_dirty()
    }
    fn reset_context(&mut self) {
        self.spatial = SpatialTools::default();
        self.transform_ui.gizmo_drag = None;
        self.hierarchy_ui.collapsed.clear();
        self.hierarchy_ui.reveal_scroll = None;
        self.play.runtime = None;
        self.play.capture = false;
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
        self.sync_textures();
    }
    pub(crate) fn open(&mut self, path: PathBuf) {
        self.open_with_recent(path, true);
    }
    fn open_with_recent(&mut self, path: PathBuf, add_recent: bool) {
        let path = if path.is_dir() {
            path.join(persistence::PROJECT_FILE)
        } else {
            path
        };
        match persistence::load_project_lazy_report(&path) {
            Ok((project, migrated)) => {
                let root = path.parent().unwrap_or(Path::new("."));
                let mut images = TextureCache::default();
                images.configure(root, &project);
                self.state = Snapshot { project, images };
                self.path = Some(path);
                self.reset_context();
                self.home.visible = false;
                self.home.example_copy = false;
                if add_recent {
                    self.remember_project();
                }
                self.log("Projeto aberto e validado.");
                if let Some(version) = migrated {
                    self.log(format!("Projeto convertido em memória do formato {version}. O original continua intacto. Ao salvar, será criado um backup identificado; versões antigas da engine não abrirão o novo formato."));
                    self.notice_last(true);
                }
            }
            Err(e) => {
                self.log(format!("Não foi possível abrir: {e}"));
                self.notice_last(false);
            }
        }
    }
    pub fn save(&mut self) -> bool {
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
        let previous_path = self.path.clone();
        if self.path.is_none() || self.home.example_copy {
            let Some(folder) = rfd::FileDialog::new()
                .set_title("Escolha a pasta do novo projeto OXY")
                .pick_folder()
            else {
                return false;
            };
            let path = folder.join("project.oxy.json");
            if path.exists() {
                self.log("Já existe project.oxy.json nessa pasta. Abra-o ou escolha outra pasta.");
                self.notice_last(true);
                return false;
            }
            self.path = Some(path);
        }
        let path = self.path.as_ref().unwrap().clone();
        let result = if self.home.example_copy {
            persistence::save_cached_copy(
                &path,
                &self.state.project,
                &mut self.state.images,
                previous_path
                    .as_deref()
                    .and_then(Path::parent)
                    .expect("Exemplo com pasta de origem"),
            )
        } else {
            persistence::save_cached_bundle(&path, &self.state.project, &mut self.state.images)
        };
        match result {
            Ok(()) => {
                self.home.example_copy = false;
                self.history.mark_saved();
                self.remember_project();
                self.renderer.release_saved_texture_pixels();
                self.play.game_ui.release_saved_texture_pixels();
                self.log(format!("Salvo: {}", path.display()));
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
    fn transition(&mut self, transition: Transition) {
        if self.dirty() || !self.studio.animation.drafts.is_empty() {
            self.pending = Some(transition)
        } else {
            self.apply_transition(transition)
        }
    }
    fn apply_transition(&mut self, t: Transition) {
        match t {
            Transition::Home => {
                self.apply_transition(Transition::Create(
                    "Meu projeto OXY".into(),
                    SceneKind::TwoD,
                ));
                self.home.visible = true;
            }
            Transition::Create(name, kind) => {
                self.home.visible = false;
                self.home.example_copy = false;
                self.state = Snapshot {
                    project: editing::blank_project(&name, kind).expect("Nome validado no diálogo"),
                    images: TextureCache::default(),
                };
                self.path = None;
                self.reset_context()
            }
            Transition::Open(path) => self.open(path),
            Transition::Recipe(path) => {
                self.open_with_recent(path, false);
                if !self.home.visible {
                    self.home.example_copy = true;
                    self.tab = Tab::Logic;
                    self.select(self.scene().entities.first().map(|e| e.id.clone()));
                }
            }
            Transition::Laboratory(path) => {
                self.open_with_recent(path, false);
                if !self.home.visible {
                    self.home.example_copy = true;
                    self.tab = Tab::Scene;
                }
            }
            Transition::Close => self.allow_close = true,
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
        if self.cancel_box_selection() {
            return;
        }
        if self.cancel_mesh_operation() {
            return;
        }
        self.forget_last_mesh_operation();
        self.cancel_body_drag();
        self.cancel_spatial_drag();
        self.finish_history(true);
        let result = if redo {
            self.history
                .redo(&mut self.state.project, &mut self.state.images)
        } else {
            self.history
                .undo(&mut self.state.project, &mut self.state.images)
        };
        if let Err(error) = result {
            self.log(error);
            self.notice_last(false);
        } else {
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
    }
    fn start(&mut self) {
        if self.mesh_operation_active() {
            self.warn("Termine o gesto de modelagem ou pressione Esc antes de jogar.");
            return;
        }
        self.cancel_mesh_operation();
        self.set_spatial_tool(Tool::Object);
        self.finish_history(true);
        match Runtime::new(&self.state.project, &self.scene_id) {
            Ok(runtime) => {
                self.play.previous_tab = self.tab;
                self.play.runtime = Some(runtime);
                self.set_tab(Tab::Scene);
                self.play.capture = true;
                self.play.last_runtime_scene = Some(self.scene_id.clone());
                self.frame.last_frame = Instant::now();
                self.studio.playing = false;
            }
            Err(e) => {
                self.log(e);
                self.notice_last(false);
            }
        }
    }
    fn stop(&mut self) {
        self.play.runtime = None;
        self.play.capture = false;
        self.set_tab(self.play.previous_tab);
        self.play.last_runtime_scene = None;
    }
    fn pause(&mut self) {
        self.play.capture = false;
        if let Some(rt) = &mut self.play.runtime {
            rt.set_paused(true);
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
    pub(super) fn import_selected(&mut self, kind: AssetKind) -> Option<Id> {
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
        if let Some(source) = file {
            if !self.history.is_pending() {
                self.history
                    .begin("Importar recurso", &self.state.project, &self.state.images);
            }
            let root = self.root();
            match persistence::import_asset(&mut self.state.project, &root, &source, kind) {
                Ok(id) => {
                    if kind == AssetKind::Texture {
                        if let Some(asset) = self.state.project.asset(&id)
                            && let Ok(image) = PaintImage::load(&root.join(&asset.path))
                        {
                            self.state.images.insert(id.clone(), image);
                        }
                        let ratio = self
                            .state
                            .images
                            .get(&id)
                            .map(|p| p.width as f32 / p.height as f32);
                        if let Some(entity) = self
                            .selected
                            .clone()
                            .and_then(|id| self.scene_mut().entity_mut(&id))
                        {
                            entity.material.texture = Some(id.clone());
                            if entity.primitive == Some(Primitive::Sprite)
                                && let Some(ratio) = ratio
                            {
                                entity.dimensions[0] = entity.dimensions[1] * ratio;
                            }
                        }
                        self.refresh_texture(&id);
                    }
                    self.log("Recurso importado como cópia local.");
                    Some(id)
                }
                Err(e) => {
                    self.log(e);
                    self.notice_last(false);
                    None
                }
            }
        } else {
            None
        }
    }
    fn game(&mut self, ui: &mut egui::Ui, dt: f32) {
        if self.play.runtime.is_none() {
            return;
        }
        if !ui.ctx().input(|i| i.focused) || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.pause();
        }
        let relative = self
            .play
            .runtime
            .as_ref()
            .is_some_and(Runtime::wants_relative_mouse);
        let input = oxy_render::input::collect_game_input(
            ui.ctx(),
            &self.state.project.input_bindings,
            self.play.capture,
            relative,
        );
        let available = [
            ui.available_width().max(1.) as u32,
            ui.available_height().max(1.) as u32,
        ];
        let resized = self.play.game_size != Some(available);
        self.play.game_size = Some(available);
        if let Some(rt) = &mut self.play.runtime {
            rt.set_viewport_aspect(available[0] as f32 / available[1] as f32);
            rt.advance(if resized { 0. } else { dt }, &input);
        }
        let scene = self.play.runtime.as_ref().unwrap().scene().clone();
        let camera = CameraState::for_runtime(self.play.runtime.as_ref().unwrap());
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size().max(Vec2::splat(1.)), Sense::click());
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
        if self.play.capture && oxy_render::input::accepts_game_click(ui.ctx()) {
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
        let sounds = if let Some(rt) = &mut self.play.runtime {
            std::mem::take(&mut rt.sounds)
        } else {
            Vec::new()
        };
        for request in sounds {
            if let Some(asset) = self.state.project.asset(&request.asset)
                && let Err(e) =
                    oxy_core::audio::play_wav(&self.root().join(&asset.path), request.volume)
            {
                self.log(e);
            }
        }
    }
    /// Console on the home screen, where the editor dock does not exist.
    fn home_console(&mut self, ctx: &egui::Context) {
        if !self.console_visible() {
            return;
        }
        egui::TopBottomPanel::bottom("console")
            .resizable(true)
            .default_height(155.)
            .height_range(80.0..=400.0)
            .show(ctx, |ui| self.console_body(ui));
    }
    pub(super) fn clear_console(&mut self) {
        self.console.messages.clear();
        if let Some(rt) = &mut self.play.runtime {
            rt.logs.clear();
        }
    }
    pub(super) fn console_body(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.console.messages {
                    ui.monospace(line);
                }
                if let Some(rt) = &self.play.runtime {
                    for line in &rt.logs {
                        ui.monospace(line);
                    }
                    let operations = oxy_core::graph::registry();
                    for trace in rt.traces.iter().rev().take(5).rev() {
                        let operation = operations
                            .iter()
                            .find(|op| op.id == trace.operation)
                            .map_or("Ação não reconhecida", |op| op.label);
                        let object = rt
                            .scene()
                            .entity(&trace.object)
                            .map_or(trace.object.as_str(), |e| e.name.as_str());
                        ui.small(format!(
                            "{:.3}s · objeto {} · nó {} · {}",
                            trace.time, object, trace.node, operation
                        ));
                    }
                }
            });
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
        if self.home.visible {
            self.stop_navigation(ctx);
            self.home_console(ctx);
            self.home_ui(ctx);
            self.project_dialogs(ctx);
            self.preferences_ui(ctx);
            self.logic_dialogs(ctx);
            self.notices_ui(ctx);
            return;
        }
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
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
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
            || self.pending.is_some()
            || self.logic_ui.inputs
            || self.logic_ui.guide;
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
            if self.tab != Tab::Logic && !self.playing() && (!animation || self.hierarchy_ui.focus)
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
                if !animation && (!compact || panel == CompactPanel::Properties) {
                    self.properties(ctx);
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
                if dock {
                    self.dock(ctx);
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.logic_toolbar(ui);
                    let objects: Vec<_> = self
                        .scene()
                        .entities
                        .iter()
                        .map(|e| (e.id.clone(), e.name.clone()))
                        .collect();
                    if let Some(id) = self.selected.clone() {
                        let trace: Vec<_> = self
                            .play
                            .runtime
                            .as_ref()
                            .map(|rt| {
                                rt.traces
                                    .iter()
                                    .filter(|t| t.object == id)
                                    .map(|t| t.node.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let scene_id = self.scene_id.clone();
                        let project = self.state.project.clone();
                        let scene = self.scene().clone();
                        if let Some(e) = self
                            .state
                            .project
                            .scene_mut(&scene_id)
                            .and_then(|s| s.entity_mut(&id))
                        {
                            self.graph
                                .show(ui, &mut e.graph, &objects, &project, &scene, &trace);
                            if std::mem::take(&mut self.graph.configure_inputs) {
                                self.logic_ui.inputs = true;
                            }
                            if let Some(topic) = self.graph.guide_requested.take() {
                                self.open_guide(&topic);
                            }
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Selecione um objeto para criar seu comportamento.");
                        });
                    }
                });
            }
        }
        self.camera_preview_window(ctx);
        self.asset_delete_dialog(ctx);
        self.fit_dialog(ctx);
        self.diagnostics_ui(ctx);
        self.project_dialogs(ctx);
        self.preferences_ui(ctx);
        self.logic_dialogs(ctx);
        self.mesh_help(ctx);
        self.mesh_creation_dialog(ctx);
        self.notices_ui(ctx);
        self.finish_navigation(ctx);
        if self.pending.is_some() {
            self.pause();
            egui::Window::new("Alterações não salvas").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER,Vec2::ZERO).show(ctx,|ui|{
            ui.label("Há alterações no projeto, nas texturas ou uma pose provisória. Salve antes de continuar ou descarte explicitamente.");
            if !self.studio.animation.drafts.is_empty() { ui.label("A pose provisória precisa ser gravada com + Quadro-chave na Animação antes de salvar. Cancele esta janela para voltar à edição."); }
            ui.horizontal(|ui|{if ui.button("Salvar e continuar").clicked()&&self.save()&& let Some(t)=self.pending.take(){self.apply_transition(t)}
if ui.button("Descartar alterações").clicked()&& let Some(t)=self.pending.take(){self.apply_transition(t)}
if ui.button("Cancelar").clicked(){self.pending=None;}});
        });
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
        if let Some(delay) = diagnostics::repaint_delay(
            self.play.runtime.as_ref().is_some_and(|r| !r.paused) && self.showing_game(),
            self.studio.playing
                && self.tab == Tab::Studio
                && self.studio.tab == StudioTab::Animation,
            ctx.input(|i| i.pointer.any_down()),
            self.frame.visible,
        ) {
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

#[cfg(all(test, target_os = "windows"))]
mod native_perf;
