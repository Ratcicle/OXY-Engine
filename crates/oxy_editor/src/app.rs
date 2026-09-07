use crate::{
    graph_ui::{GraphView, object_picker, value_editor},
    studio::{Studio, StudioTab},
};
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use glam::Vec3;
use oxy_core::{
    document::*,
    history::History,
    painting::PaintImage,
    persistence,
    runtime::{InputFrame, Runtime},
};
use oxy_render::{CameraState, Renderer};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, PartialEq)]
pub struct Snapshot {
    pub project: Project,
    pub images: HashMap<Id, PaintImage>,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Scene,
    Game,
    Studio,
    Logic,
}
#[derive(Clone, Copy, PartialEq)]
enum Gizmo {
    Move,
    Rotate,
    Scale,
}
struct GizmoDrag {
    entity: Id,
    base: Transform,
    axis: usize,
    origin: Pos2,
    direction: Vec2,
    pixels_per_unit: f32,
}
enum Transition {
    New,
    Open(PathBuf),
    Close,
}

pub struct Editor {
    pub state: Snapshot,
    saved: Snapshot,
    history: History<Snapshot>,
    pub path: Option<PathBuf>,
    pub scene_id: Id,
    pub selected: Option<Id>,
    pub tab: Tab,
    pub studio: Studio,
    graph: GraphView,
    pub renderer: Renderer,
    render_state: eframe::egui_wgpu::RenderState,
    context: egui::Context,
    camera: CameraState,
    pub(crate) runtime: Option<Runtime>,
    game_ui: oxy_render::game_ui::GameUi,
    pub(crate) capture: bool,
    previous_tab: Tab,
    last_time: Instant,
    pub messages: Vec<String>,
    console: bool,
    debug: bool,
    grid: bool,
    grid_size: f32,
    gizmo: Gizmo,
    gizmo_drag: Option<GizmoDrag>,
    pending: Option<Transition>,
    allow_close: bool,
    scale: f32,
    attribute_name: String,
    attribute_type: u8,
    asset_search: String,
    new_scene_kind: SceneKind,
    view_global: bool,
    last_runtime_scene: Option<Id>,
    context_target: Option<Id>,
    game_size: Option<[u32; 2]>,
}

impl Editor {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(26, 30, 37);
        style.visuals.window_fill = Color32::from_rgb(31, 36, 43);
        style.visuals.extreme_bg_color = Color32::from_rgb(18, 22, 28);
        style.visuals.selection.bg_fill = Color32::from_rgb(43, 103, 100);
        style.visuals.selection.stroke = egui::Stroke::new(1., Color32::from_rgb(140, 229, 214));
        style.spacing.item_spacing = Vec2::new(8., 7.);
        style.spacing.button_padding = Vec2::new(10., 6.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
        cc.egui_ctx.set_style(style);
        let rs = cc
            .wgpu_render_state
            .clone()
            .expect("A OXY Engine requer backend wgpu");
        let project = Project::new("Meu projeto OXY");
        let scene_id = project.start_scene.clone();
        let camera = CameraState::for_scene(project.scene(&scene_id).unwrap());
        let state = Snapshot {
            project,
            images: HashMap::new(),
        };
        let mut this = Self {
            saved: state.clone(),
            history: History::new(state.clone()),
            state,
            path: None,
            scene_id,
            selected: None,
            tab: Tab::Scene,
            studio: Studio::default(),
            graph: GraphView::default(),
            renderer: Renderer::new(&rs),
            render_state: rs,
            context: cc.egui_ctx.clone(),
            camera,
            runtime: None,
            game_ui: Default::default(),
            capture: false,
            previous_tab: Tab::Scene,
            last_time: Instant::now(),
            messages: vec!["OXY Engine · editor nativo · documentos locais".into()],
            console: false,
            debug: false,
            grid: true,
            grid_size: 0.25,
            gizmo: Gizmo::Move,
            gizmo_drag: None,
            pending: None,
            allow_close: false,
            scale: 1.,
            attribute_name: String::new(),
            attribute_type: 0,
            asset_search: String::new(),
            new_scene_kind: SceneKind::TwoD,
            view_global: false,
            last_runtime_scene: None,
            context_target: None,
            game_size: None,
        };
        let arg = if cfg!(test) {
            None
        } else {
            std::env::args_os().nth(1).map(PathBuf::from)
        };
        let mut candidates = vec![PathBuf::from("examples/validacao/project.oxy.json")];
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("data/project.oxy.json"));
            candidates.push(dir.join("../../examples/validacao/project.oxy.json"));
        }
        if let Some(path) = arg.or_else(|| candidates.into_iter().find(|p| p.exists())) {
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
        self.messages.push(message.into());
        if self.messages.len() > 600 {
            self.messages.drain(..100);
        }
    }
    fn dirty(&self) -> bool {
        self.state != self.saved
    }
    fn reset_context(&mut self) {
        self.runtime = None;
        self.capture = false;
        self.selected = None;
        self.tab = Tab::Scene;
        self.studio = Studio::default();
        self.graph = GraphView::default();
        self.scene_id = self.state.project.start_scene.clone();
        self.camera = CameraState::for_scene(self.scene());
        self.history.reset(self.state.clone());
        self.saved = self.state.clone();
        self.sync_textures();
    }
    pub(crate) fn open(&mut self, path: PathBuf) {
        let path = if path.is_dir() {
            path.join(persistence::PROJECT_FILE)
        } else {
            path
        };
        match persistence::load_project(&path) {
            Ok(project) => {
                let root = path.parent().unwrap_or(Path::new("."));
                let images = match persistence::load_paint_images(&project, root) {
                    Ok(images) => images,
                    Err(error) => {
                        self.log(format!("Não foi possível abrir as texturas: {error}"));
                        self.console = true;
                        return;
                    }
                };
                self.state = Snapshot { project, images };
                self.path = Some(path);
                self.reset_context();
                self.log("Projeto aberto e validado.");
            }
            Err(e) => {
                self.log(format!("Não foi possível abrir: {e}"));
                self.console = true;
            }
        }
    }
    pub fn save(&mut self) -> bool {
        self.finish_history(true);
        let previous_path = self.path.clone();
        if self.path.is_none() {
            let Some(folder) = rfd::FileDialog::new()
                .set_title("Escolha a pasta do novo projeto OXY")
                .pick_folder()
            else {
                return false;
            };
            let path = folder.join("project.oxy.json");
            if path.exists() {
                self.log("Já existe project.oxy.json nessa pasta. Abra-o ou escolha outra pasta.");
                self.console = true;
                return false;
            }
            self.path = Some(path);
        }
        let path = self.path.as_ref().unwrap().clone();
        match persistence::save_bundle(&path, &self.state.project, &self.state.images) {
            Ok(()) => {
                self.saved = self.state.clone();
                self.log(format!("Salvo: {}", path.display()));
                true
            }
            Err(e) => {
                self.path = previous_path;
                self.log(format!(
                    "Falha ao salvar; o estado de edição foi preservado: {e}"
                ));
                self.console = true;
                false
            }
        }
    }
    fn transition(&mut self, transition: Transition) {
        if self.dirty() {
            self.pending = Some(transition)
        } else {
            self.apply_transition(transition)
        }
    }
    fn apply_transition(&mut self, t: Transition) {
        match t {
            Transition::New => {
                self.state = Snapshot {
                    project: Project::new("Meu projeto OXY"),
                    images: HashMap::new(),
                };
                self.path = None;
                self.reset_context()
            }
            Transition::Open(path) => self.open(path),
            Transition::Close => self.allow_close = true,
        }
    }
    fn sync_textures(&mut self) {
        self.renderer.clear_textures();
        self.game_ui = oxy_render::GameUi::new();
        for (id, pixels) in &self.state.images {
            if let Err(e) = self.renderer.set_texture_pixels(
                &self.render_state,
                id,
                pixels.width,
                pixels.height,
                &pixels.pixels,
                true,
            ) {
                self.messages.push(e);
            }
            if let Err(e) = self.game_ui.set_texture_pixels(
                &self.context,
                id,
                pixels.width,
                pixels.height,
                &pixels.pixels,
                true,
            ) {
                self.messages.push(e);
            }
        }
    }
    pub fn refresh_texture(&mut self, id: &str) {
        if let Some(p) = self.state.images.get(id) {
            if let Err(e) = self.renderer.set_texture_pixels(
                &self.render_state,
                id,
                p.width,
                p.height,
                &p.pixels,
                true,
            ) {
                self.messages.push(e);
            }
            if let Err(e) = self.game_ui.set_texture_pixels(
                &self.context,
                id,
                p.width,
                p.height,
                &p.pixels,
                true,
            ) {
                self.messages.push(e);
            }
        }
    }
    fn finish_history(&mut self, force: bool) {
        if force {
            if self.history.is_pending() {
                self.history.commit(self.state.clone());
            } else {
                self.history.record("Editar projeto", self.state.clone());
            }
        }
    }
    fn undo(&mut self, redo: bool) {
        self.finish_history(true);
        let snapshot = if redo {
            self.history.redo()
        } else {
            self.history.undo()
        };
        if let Some(snapshot) = snapshot {
            self.state = snapshot;
            if self.state.project.scene(&self.scene_id).is_none() {
                self.scene_id = self.state.project.start_scene.clone()
            };
            if self
                .selected
                .as_ref()
                .is_some_and(|id| self.scene().entity(id).is_none())
            {
                self.selected = None;
            }
            self.sync_textures();
        }
    }
    fn start(&mut self) {
        self.finish_history(true);
        match Runtime::new(&self.state.project, &self.scene_id) {
            Ok(runtime) => {
                self.previous_tab = if self.tab == Tab::Game {
                    Tab::Scene
                } else {
                    self.tab
                };
                self.runtime = Some(runtime);
                self.tab = Tab::Game;
                self.capture = true;
                self.last_runtime_scene = Some(self.scene_id.clone());
                self.last_time = Instant::now();
                self.studio.playing = false;
            }
            Err(e) => {
                self.log(e);
                self.console = true;
            }
        }
    }
    fn stop(&mut self) {
        self.runtime = None;
        self.capture = false;
        self.tab = self.previous_tab;
        self.last_runtime_scene = None;
    }
    fn pause(&mut self) {
        self.capture = false;
        if let Some(rt) = &mut self.runtime {
            rt.set_paused(true);
        }
    }
    pub fn select(&mut self, id: Option<Id>) {
        if self.selected != id {
            self.selected = id;
            self.studio.shared_edit = None;
        }
    }
    pub fn add_entity(&mut self, primitive: Option<Primitive>, name: &str) {
        let mut entity = Entity::new(name, primitive);
        if self.tab == Tab::Studio {
            entity.parent = self.selected.clone();
        }
        let id = entity.id.clone();
        self.scene_mut().entities.push(entity);
        self.select(Some(id));
    }
    fn duplicate(&mut self) {
        if let Some(id) = self.selected.clone() {
            match self.scene_mut().duplicate_subtree(&id) {
                Ok(id) => self.select(Some(id)),
                Err(e) => self.log(e),
            }
        }
    }
    fn delete(&mut self) {
        if let Some(id) = self.selected.take() {
            self.scene_mut().remove_subtree(&id);
        }
    }
    fn group(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(entity) = self.scene().entity(&id).cloned() else {
            return;
        };
        let mut group = Entity::new("Grupo", None);
        group.parent = entity.parent.clone();
        group.transform.position = entity.transform.position;
        let group_id = group.id.clone();
        self.scene_mut().entities.push(group);
        if let Err(e) = self.scene_mut().reparent(&id, Some(group_id.clone()), true) {
            self.log(e)
        }
        self.select(Some(group_id));
    }
    fn set_scene(&mut self, id: Id) {
        self.pause();
        self.scene_id = id;
        self.selected = None;
        self.camera = CameraState::for_scene(self.scene());
        self.studio = Studio::default();
        self.graph = GraphView::default();
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("OXY")
                        .size(26.)
                        .strong()
                        .color(Color32::from_rgb(131, 224, 207)),
                );
                ui.label(egui::RichText::new("ENGINE").size(15.).strong());
                ui.separator();
                ui.menu_button("Projeto", |ui| {
                    if ui.button("Novo projeto").clicked() {
                        self.transition(Transition::New);
                        ui.close();
                    }
                    if ui.button("Abrir projeto…").clicked() {
                        self.pause();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Projeto OXY", &["json"])
                            .pick_file()
                        {
                            self.transition(Transition::Open(path))
                        }
                        ui.close();
                    }
                    if ui.button("Salvar   Ctrl+S").clicked() {
                        self.save();
                        ui.close();
                    }
                    if ui.button("Importar PNG…").clicked() {
                        self.import(AssetKind::Texture);
                        ui.close();
                    }
                    if ui.button("Importar WAV…").clicked() {
                        self.import(AssetKind::Audio);
                        ui.close();
                    }
                });
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new("Desfazer"))
                    .clicked()
                {
                    self.undo(false)
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new("Refazer"))
                    .clicked()
                {
                    self.undo(true)
                }
                ui.separator();
                if self.runtime.is_none() {
                    if ui.button("▶ Jogar").clicked() {
                        self.start();
                    }
                } else {
                    let paused = self.runtime.as_ref().is_some_and(|r| r.paused);
                    if ui
                        .button(if paused { "▶ Retomar" } else { "Ⅱ Pausar" })
                        .clicked()
                    {
                        if paused {
                            self.tab = Tab::Game;
                            self.capture = true;
                            if let Some(r) = &mut self.runtime {
                                r.set_paused(false);
                            }
                            self.last_time = Instant::now();
                        } else {
                            self.pause()
                        }
                    }
                    if ui.button("■ Parar").clicked() {
                        self.stop()
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.scale, 0.8..=1.6)
                            .text("Interface")
                            .show_value(false),
                    );
                    ui.label(if self.dirty() {
                        "● Não salvo"
                    } else if self.path.is_none() {
                        "Sem arquivo"
                    } else {
                        "Salvo"
                    });
                    ui.label(&self.state.project.name);
                });
            });
            ui.horizontal(|ui| {
                let old = self.tab;
                for (tab, label) in [
                    (Tab::Scene, "Cena"),
                    (Tab::Game, "Jogo"),
                    (Tab::Studio, "Estúdio"),
                    (Tab::Logic, "Lógica"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, label);
                }
                if old == Tab::Game && self.tab != Tab::Game {
                    self.pause();
                }
                ui.separator();
                let mut scene_id = self.scene_id.clone();
                egui::ComboBox::from_id_salt("scene")
                    .selected_text(&self.scene().name)
                    .show_ui(ui, |ui| {
                        for scene in &self.state.project.scenes {
                            ui.selectable_value(&mut scene_id, scene.id.clone(), &scene.name);
                        }
                    });
                if scene_id != self.scene_id {
                    self.set_scene(scene_id);
                }
                ui.menu_button("+ Cena", |ui| {
                    ui.selectable_value(&mut self.new_scene_kind, SceneKind::TwoD, "2D");
                    ui.selectable_value(&mut self.new_scene_kind, SceneKind::ThreeD, "3D");
                    if ui.button("Criar cena").clicked() {
                        let scene = Scene::new(
                            if self.new_scene_kind == SceneKind::TwoD {
                                "Nova cena 2D"
                            } else {
                                "Nova cena 3D"
                            },
                            self.new_scene_kind,
                        );
                        let id = scene.id.clone();
                        self.state.project.scenes.push(scene);
                        self.set_scene(id);
                        ui.close();
                    }
                });
                if ui
                    .button("Definir inicial")
                    .on_hover_text("Cena aberta ao iniciar o player")
                    .clicked()
                {
                    self.state.project.start_scene = self.scene_id.clone();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.console, "Console");
                    ui.label(if self.scene().kind == SceneKind::TwoD {
                        "2D · metros · Y ↑"
                    } else {
                        "3D · metros · Y ↑ · RH"
                    });
                });
            });
        });
    }
    pub fn import(&mut self, kind: AssetKind) {
        self.pause();
        if self.path.is_none() && !self.save() {
            return;
        }
        let dialog = rfd::FileDialog::new();
        let file = if kind == AssetKind::Texture {
            dialog.add_filter("Imagem PNG", &["png"]).pick_file()
        } else {
            dialog.add_filter("Áudio WAV", &["wav"]).pick_file()
        };
        if let Some(source) = file {
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
                    self.log("Asset importado como cópia local.");
                }
                Err(e) => {
                    self.log(e);
                    self.console = true;
                }
            }
        }
    }
    fn creation_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("+ Objeto", |ui| {
            let primitives: Vec<_> = if self.scene().kind == SceneKind::TwoD {
                vec![
                    (Primitive::Rectangle, "Retângulo"),
                    (Primitive::Circle, "Círculo"),
                    (Primitive::Sprite, "Sprite"),
                ]
            } else {
                vec![
                    (Primitive::Cube, "Cubo"),
                    (Primitive::Sphere, "Esfera"),
                    (Primitive::Cylinder, "Cilindro"),
                    (Primitive::Plane, "Plano"),
                ]
            };
            for (primitive, name) in primitives {
                if ui.button(name).clicked() {
                    self.add_entity(Some(primitive), name);
                    ui.close();
                }
            }
            if ui.button("Grupo vazio").clicked() {
                self.add_entity(None, "Grupo");
                ui.close();
            }
            if ui.button("Câmera").clicked() {
                self.add_entity(None, "Câmera");
                let id = self.selected.clone().unwrap();
                self.scene_mut().entity_mut(&id).unwrap().camera = Some(Camera::default());
                ui.close();
            }
            ui.separator();
            for (kind, name) in [
                (UiKind::Text, "Texto de interface"),
                (UiKind::Image, "Imagem de interface"),
                (UiKind::Button, "Botão de interface"),
                (UiKind::Bar, "Barra de atributo"),
            ] {
                if ui.button(name).clicked() {
                    self.add_entity(None, name);
                    let id = self.selected.clone().unwrap();
                    self.scene_mut().entity_mut(&id).unwrap().ui = Some(UiElement {
                        kind,
                        ..Default::default()
                    });
                    ui.close();
                }
            }
        });
    }
    fn hierarchy(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("hierarchy")
            .default_width(220.)
            .width_range(165.0..=370.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("HIERARQUIA");
                    self.creation_menu(ui);
                });
                ui.add_space(4.);
                let name = &mut self.scene_mut().name;
                ui.add(egui::TextEdit::singleline(name).desired_width(ui.available_width()));
                ui.separator();
                let scene = self.scene().clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entity in scene.entities.iter().filter(|e| e.parent.is_none()) {
                        self.hierarchy_item(ui, &scene, &entity.id, 0);
                    }
                });
            });
    }
    fn hierarchy_item(&mut self, ui: &mut egui::Ui, scene: &Scene, id: &str, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(e) = scene.entity(id) else { return };
        ui.push_id(id, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.);
                let icon = if e.camera.is_some() {
                    "◉"
                } else if e.ui.is_some() {
                    "▤"
                } else if e.primitive.is_none() {
                    "▾"
                } else {
                    "◇"
                };
                let response = ui
                    .add(
                        egui::Button::selectable(
                            self.selected.as_deref() == Some(id),
                            format!("{icon} {}", e.name),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&e.name);
                if response.clicked() {
                    self.select(Some(id.into()));
                }
                response.context_menu(|ui| {
                    self.object_context(ui, id);
                });
            });
        });
        for child in scene
            .entities
            .iter()
            .filter(|c| c.parent.as_deref() == Some(id))
        {
            self.hierarchy_item(ui, scene, &child.id, depth + 1);
        }
    }
    fn object_context(&mut self, ui: &mut egui::Ui, id: &str) {
        for (label, tab, sub) in [
            ("Abrir no Estúdio", Tab::Studio, StudioTab::Model),
            ("Editar animações", Tab::Studio, StudioTab::Animation),
            ("Editar lógica", Tab::Logic, StudioTab::Model),
        ] {
            if ui.button(label).clicked() {
                self.select(Some(id.into()));
                self.tab = tab;
                self.studio.tab = sub;
                self.studio.owner = Some(id.into());
                if tab == Tab::Studio {
                    self.frame_selection();
                }
                ui.close();
            }
        }
        if ui.button("Duplicar hierarquia").clicked() {
            self.select(Some(id.into()));
            self.duplicate();
            ui.close();
        }
        if ui.button("Agrupar").clicked() {
            self.select(Some(id.into()));
            self.group();
            ui.close();
        }
        if ui.button("Excluir hierarquia").clicked() {
            self.select(Some(id.into()));
            self.delete();
            ui.close();
        }
    }

    fn properties(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties").default_width(290.).width_range(240.0..=460.0).resizable(true).show(ctx,|ui| {
            ui.strong("PROPRIEDADES");ui.separator();
            egui::ScrollArea::vertical().show(ui,|ui| {
                let Some(id)=self.selected.clone() else {
                    ui.heading("Projeto");ui.text_edit_singleline(&mut self.state.project.name);
                    ui.label("Selecione um objeto na cena ou na hierarquia. Use + Objeto para criar sua primeira forma.");
                    ui.separator();ui.strong("Ações de entrada");ui.small("Nomes de teclas: A…Z, Space, ArrowLeft, ArrowRight. Escape libera o jogo.");
                    for (action,key) in &mut self.state.project.input_bindings {ui.horizontal(|ui|{ui.label(action);ui.add(egui::TextEdit::singleline(key).desired_width(70.));});}
                    return
                };
                let Some(mut entity)=self.scene().entity(&id).cloned() else{return};
                let objects:Vec<_>=self.scene().entities.iter().map(|e|(e.id.clone(),e.name.clone())).collect();
                let textures:Vec<_>=self.state.project.assets.iter().filter(|a|a.kind==AssetKind::Texture).map(|a|(a.id.clone(),a.name.clone())).collect();
                ui.text_edit_singleline(&mut entity.name);
                ui.label(egui::RichText::new("INSTÂNCIA NA CENA").small().color(Color32::from_rgb(128,203,192)));
                if entity.model_source.is_some(){ui.small("Esta estrutura é uma cópia editável. Salvar como modelo cria um asset independente.");}
                ui.horizontal(|ui|{ui.checkbox(&mut entity.visible,"Visível");ui.label("Camada");ui.add(egui::DragValue::new(&mut entity.layer));});
                let old_parent=entity.parent.clone();
                ui.label("Pai (preserva posição global)");object_picker(ui,&mut entity.parent,&objects,"entity_parent");
                let new_parent=entity.parent.clone();entity.parent=old_parent.clone();
                ui.collapsing("Transformação",|ui| {
                    ui.horizontal(|ui|{ui.selectable_value(&mut self.view_global,false,"Local");ui.selectable_value(&mut self.view_global,true,"Global");});
                    let mut transform=if self.view_global {Transform::from_matrix(self.scene().world_matrix(&id).unwrap_or_default(),entity.transform.pivot)}else{entity.transform.clone()};
                    let before=transform.clone();
                    vector3(ui,"Posição",&mut transform.position,0.05,false);
                    let mut degrees=transform.rotation.map(f32::to_degrees);let original_degrees=degrees;vector3(ui,"Rotação °",&mut degrees,0.5,false);if degrees!=original_degrees{transform.rotation=degrees.map(f32::to_radians);}
                    vector3(ui,"Escala",&mut transform.scale,0.02,true);
                    if !self.view_global{vector3(ui,"Pivô local",&mut transform.pivot,0.05,false);}
                    if transform!=before {
                        if self.view_global {
                            let parent=entity.parent.as_deref().and_then(|p|self.scene().world_matrix(p).ok()).unwrap_or(glam::Mat4::IDENTITY);
                            let matrix = parent.inverse() * transform.matrix();
                            let candidate = Transform::from_matrix(matrix,entity.transform.pivot);
                            if parent.determinant().abs() < 1e-8 || !candidate.finite() || !candidate.matrix().abs_diff_eq(matrix,0.0001) {
                                self.log("A transformação global exigiria cisalhamento. Edite em Local ou ajuste a escala não uniforme do pai. A peça foi preservada.");
                                self.console = true;
                            } else {
                                entity.transform = candidate;
                            }
                        } else {
                            entity.transform = transform;
                        }
                    }
                    if ui.button("Espelhar X").clicked(){entity.transform.scale[0]*=-1.;}
                });
                if entity.primitive.is_some(){ui.collapsing("Forma e material",|ui| {
                    ui.label(match entity.primitive.unwrap(){Primitive::Rectangle=>"Retângulo",Primitive::Circle=>"Círculo",Primitive::Sprite=>"Sprite",Primitive::Cube=>"Cubo",Primitive::Sphere=>"Esfera",Primitive::Cylinder=>"Cilindro",Primitive::Plane=>"Plano"});
                    vector3(ui,"Dimensões",&mut entity.dimensions,0.05,true);entity.dimensions=entity.dimensions.map(|v|v.max(0.0001));
                    if matches!(entity.primitive,Some(Primitive::Circle|Primitive::Sphere|Primitive::Cylinder)){ui.add(egui::Slider::new(&mut entity.segments,3..=128).text("Segmentos"));if entity.material.texture.is_some(){ui.small("UVs paramétricos preservados. Alterar segmentos mantém o PNG; a superfície pode reamostrar os pixels.");}}
                    ui.label("Cor base");ui.color_edit_button_rgba_unmultiplied(&mut entity.material.color);
                    ui.label("Textura PNG");
                    let selected=entity.material.texture.as_ref().and_then(|id|textures.iter().find(|(i,_)|i==id).map(|(_,n)|n.as_str())).unwrap_or("Sem textura");
                    egui::ComboBox::from_id_salt("material_texture").selected_text(selected).show_ui(ui,|ui|{ui.selectable_value(&mut entity.material.texture,None,"Sem textura");for (id,name) in &textures{ui.selectable_value(&mut entity.material.texture,Some(id.clone()),name);}});
                    ui.checkbox(&mut entity.material.nearest,"Pixel art (vizinho mais próximo)");
                    if ui.button("Abrir pintura").clicked(){self.tab=Tab::Studio;self.studio.tab=StudioTab::Paint;self.studio.owner=Some(id.clone());}
                });}
                ui.collapsing("Componentes",|ui| {
                    component_switch(ui,"Colisão / área",&mut entity.collider,Collider{size:entity.dimensions,..Default::default()});
                    if let Some(c)=&mut entity.collider{ui.checkbox(&mut c.enabled,"Colisor ativo");ui.checkbox(&mut c.is_trigger,"Área de detecção");vector3(ui,"Tamanho AABB",&mut c.size,0.05,true);c.size=c.size.map(|v|v.max(0.0001));vector3(ui,"Deslocamento",&mut c.offset,0.05,false);ui.small("Caixa alinhada aos eixos. Rotacionar a aparência não rotaciona a caixa física.");}
                    ui.separator();component_switch(ui,"Controlador de movimento",&mut entity.controller,Controller::default());
                    if let Some(c)=&mut entity.controller{ui.checkbox(&mut c.enabled,"Controlador ativo");ui.add(egui::DragValue::new(&mut c.speed).range(0.0..=100.0).prefix("Velocidade "));ui.add(egui::DragValue::new(&mut c.jump).range(0.0..=100.0).prefix("Pulo "));ui.add(egui::DragValue::new(&mut c.gravity).range(0.0..=200.0).prefix("Gravidade "));}
                    ui.separator();component_switch(ui,"Câmera de jogo",&mut entity.camera,Camera::default());
                    if let Some(c)=&mut entity.camera{ui.checkbox(&mut c.active,"Câmera ativa");ui.add(egui::DragValue::new(&mut c.orthographic_size).range(0.1..=500.).prefix("Meia altura 2D "));ui.add(egui::Slider::new(&mut c.fov,10.0..=150.).text("FOV 3D"));ui.small("A câmera olha para -Z local. Ative apenas a câmera desejada.");}
                });
                ui.collapsing("Atributos personalizados",|ui| {
                    ui.small("Nenhum nome de atributo impõe regras. Os nós configuram o comportamento.");
                    let mut delete=None;
                    for (name,value) in &mut entity.attributes{ui.push_id(name,|ui|{ui.horizontal(|ui|{ui.strong(name);if ui.small_button("×").clicked(){delete=Some(name.clone());}});value_editor(ui,value,&objects,name);});}
                    if let Some(name)=delete{entity.attributes.remove(&name);}
                    ui.separator();ui.add(egui::TextEdit::singleline(&mut self.attribute_name).hint_text("Nome do atributo"));
                    egui::ComboBox::from_id_salt("attrtype").selected_text(["Número","Texto","Booleano","Objeto"][self.attribute_type as usize]).show_ui(ui,|ui|{for (i,name) in ["Número","Texto","Booleano","Objeto"].into_iter().enumerate(){ui.selectable_value(&mut self.attribute_type,i as u8,name);}});
                    if ui.add_enabled(!self.attribute_name.trim().is_empty(),egui::Button::new("Adicionar atributo")).clicked(){let value=match self.attribute_type{0=>Value::Number(0.),1=>Value::Text(String::new()),2=>Value::Bool(false),_=>Value::Object(None)};entity.attributes.entry(self.attribute_name.trim().into()).or_insert(value);self.attribute_name.clear();}
                });
                if let Some(element)=&mut entity.ui {ui.collapsing("Interface do jogo",|ui| {
                    egui::ComboBox::from_id_salt("ui_kind").selected_text(format!("{:?}",element.kind)).show_ui(ui,|ui|{for (kind,name) in [(UiKind::Text,"Texto"),(UiKind::Image,"Imagem"),(UiKind::Button,"Botão"),(UiKind::Bar,"Barra")]{ui.selectable_value(&mut element.kind,kind,name);}});
                    egui::ComboBox::from_id_salt("ui_anchor").selected_text(format!("{:?}",element.anchor)).show_ui(ui,|ui|{for (anchor,name) in [(UiAnchor::TopLeft,"Superior esquerdo"),(UiAnchor::TopRight,"Superior direito"),(UiAnchor::BottomLeft,"Inferior esquerdo"),(UiAnchor::BottomRight,"Inferior direito"),(UiAnchor::Center,"Centro")]{ui.selectable_value(&mut element.anchor,anchor,name);}});
                    ui.label("Deslocamento em pontos");ui.horizontal(|ui|{for value in &mut element.position{ui.add(egui::DragValue::new(value));}});
                    ui.label("Tamanho");ui.horizontal(|ui|{for value in &mut element.size{ui.add(egui::DragValue::new(value).range(1.0..=4000.));}});
                    ui.label("Texto ({valor} mostra o vínculo)");ui.text_edit_multiline(&mut element.text);ui.color_edit_button_rgba_unmultiplied(&mut element.color);
                    ui.label("Objeto do atributo");object_picker(ui,&mut element.binding_object,&objects,"ui_binding");ui.text_edit_singleline(&mut element.binding_attribute);ui.add(egui::DragValue::new(&mut element.max_value).range(0.01..=1000000.).prefix("Máximo "));
                    let label=element.texture.as_ref().and_then(|id|textures.iter().find(|(i,_)|i==id).map(|(_,n)|n.as_str())).unwrap_or("Sem imagem");
                    egui::ComboBox::from_id_salt("ui_image").selected_text(label).show_ui(ui,|ui|{ui.selectable_value(&mut element.texture,None,"Sem imagem");for (id,name) in &textures{ui.selectable_value(&mut element.texture,Some(id.clone()),name);}});
                });}
                if let Some(original)=self.scene_mut().entity_mut(&id){*original=entity;}
                if new_parent!=old_parent&& let Err(e)=self.scene_mut().reparent(&id,new_parent,true){self.log(e);self.console=true;}
                ui.separator();
                ui.horizontal(|ui|{if ui.button("Lógica").clicked(){self.tab=Tab::Logic;}
if ui.button("Animação").clicked(){self.tab=Tab::Studio;self.studio.tab=StudioTab::Animation;self.studio.owner=Some(id.clone());}});
                if ui.button("Salvar hierarquia como modelo").clicked(){let scene_id=self.scene_id.clone();let name=self.scene().entity(&id).map(|e|e.name.clone()).unwrap_or_default();match self.state.project.save_model(&scene_id,&id,&name){Ok(_)=>self.log("Modelo salvo na biblioteca, com sua estrutura editável. Salve o projeto para gravar em disco."),Err(e)=>{self.log(e);self.console=true;}}}
                ui.horizontal(|ui|{if ui.button("Duplicar").clicked(){self.duplicate();}
if ui.button("Agrupar").clicked(){self.group();}
if ui.button("Excluir").clicked(){self.delete();}});
            });
        });
    }

    fn library(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("library").resizable(true).default_height(145.).height_range(90.0..=360.0).show(ctx,|ui| {
            ui.horizontal(|ui|{ui.strong("BIBLIOTECA DO PROJETO");ui.add(egui::TextEdit::singleline(&mut self.asset_search).hint_text("Filtrar assets…").desired_width(170.));if ui.button("Importar PNG").clicked(){self.import(AssetKind::Texture);}
if ui.button("Importar WAV").clicked(){self.import(AssetKind::Audio);}});
            let assets=self.state.project.assets.clone();let asset_filter=self.asset_search.to_lowercase();
            egui::ScrollArea::vertical().show(ui,|ui|{
                ui.horizontal_wrapped(|ui|{
                    for asset in assets.iter().filter(|a|a.name.to_lowercase().contains(&asset_filter)){
                        ui.group(|ui|{ui.vertical(|ui|{ui.set_width(200.);ui.strong(&asset.name);ui.small(match asset.kind{AssetKind::Texture=>"PNG · textura",AssetKind::Audio=>"WAV · áudio",AssetKind::Model=>"Modelo · peças editáveis"});
                            match asset.kind {
                                AssetKind::Model=>{if ui.button("Colocar na cena").clicked(){let scene_id=self.scene_id.clone();match self.state.project.instantiate_model(&asset.id,&scene_id){Ok(id)=>self.select(Some(id)),Err(e)=>self.log(e)}}},
                                AssetKind::Texture=>{if ui.button("Criar sprite / aplicar").clicked(){if self.selected.is_none(){self.add_entity(Some(Primitive::Sprite),&asset.name);}
if let Some(id)=self.selected.clone(){if let Some(e)=self.scene_mut().entity_mut(&id){e.material.texture=Some(asset.id.clone());}
if let Some(p)=self.state.images.get(&asset.id){let ratio=p.width as f32/p.height as f32;if let Some(e)=self.scene_mut().entity_mut(&id)&& e.primitive==Some(Primitive::Sprite){e.dimensions[0]=e.dimensions[1]*ratio;}}}}},
                                AssetKind::Audio=>{if ui.button("Ouvir").clicked()&& let Err(e)=oxy_core::audio::play_wav(&self.root().join(&asset.path),0.6){self.log(e);self.console=true;}},
                            }
                        });});
                    }
                    if assets.is_empty(){ui.label("Importe um PNG ou WAV, ou salve a hierarquia selecionada como modelo. Os originais importados são preservados.");}
                });
            });
        });
    }

    pub(crate) fn frame_selection(&mut self) {
        let Some(id) = &self.selected else {
            self.camera = CameraState::for_scene(self.scene());
            return;
        };
        let ids = self.scene().descendants(id);
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for entity in self
            .scene()
            .entities
            .iter()
            .filter(|e| ids.contains(&e.id) && e.primitive.is_some())
        {
            if let Ok(world) = self.scene().world_matrix(&entity.id) {
                let half = Vec3::from_array(entity.dimensions) * 0.5;
                for x in [-1., 1.] {
                    for y in [-1., 1.] {
                        for z in [-1., 1.] {
                            let point = world.transform_point3(half * Vec3::new(x, y, z));
                            min = min.min(point);
                            max = max.max(point);
                        }
                    }
                }
            }
        }
        if min.is_finite() && max.is_finite() {
            self.camera.target = (min + max) * 0.5;
            let radius = ((max - min).length() * 0.5).max(0.25);
            self.camera.distance = radius / (self.camera.fov.to_radians() * 0.5).sin() * 1.6;
            self.camera.orthographic_size = radius * 1.5;
        } else if let Ok(world) = self.scene().world_matrix(id) {
            self.camera.target = world.transform_point3(Vec3::ZERO);
        }
    }

    pub fn viewport(&mut self, ui: &mut egui::Ui, painting: bool) {
        if !painting {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.gizmo, Gizmo::Move, "Mover");
                ui.selectable_value(&mut self.gizmo, Gizmo::Rotate, "Girar");
                ui.selectable_value(&mut self.gizmo, Gizmo::Scale, "Escalar");
                ui.separator();
                ui.checkbox(&mut self.grid, "Grade");
                ui.add(
                    egui::DragValue::new(&mut self.grid_size)
                        .range(0.01..=10.)
                        .speed(0.01)
                        .prefix("Encaixe "),
                );
                ui.checkbox(&mut self.debug, "Colisores");
                if ui.button("Enquadrar").clicked() {
                    self.frame_selection();
                }
            });
        }
        let scene = if self.tab == Tab::Studio && self.studio.tab == StudioTab::Animation {
            self.animation_preview()
        } else {
            self.scene().clone()
        };
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::splat(1.)),
            Sense::click_and_drag(),
        );
        let size = [rect.width().max(1.) as u32, rect.height().max(1.) as u32];
        let physical = [
            (rect.width() * ui.ctx().pixels_per_point()).max(1.) as u32,
            (rect.height() * ui.ctx().pixels_per_point()).max(1.) as u32,
        ];
        self.renderer.show_grid = self.grid;
        let texture = self.renderer.render(
            &self.render_state,
            &self.state.project,
            &scene,
            &self.root(),
            &self.camera,
            physical,
            self.selected.clone(),
            self.debug,
        );
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        if response.hovered() {
            self.camera.zoom(ui.input(|i| i.smooth_scroll_delta.y));
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            let d = ui.input(|i| i.pointer.delta());
            self.camera.pan([d.x, d.y], size);
        }
        if self.scene().kind == SceneKind::ThreeD
            && response.dragged_by(egui::PointerButton::Secondary)
        {
            let d = ui.input(|i| i.pointer.delta());
            self.camera.orbit([d.x, d.y]);
        }
        let point = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p));
        let pick = point.and_then(|p| {
            oxy_render::pick(
                &scene,
                &self.camera,
                size,
                [p.x - rect.min.x, p.y - rect.min.y],
            )
        });
        let ui_pick = point.and_then(|p| oxy_render::pick_ui(&scene, rect, p));
        if painting {
            if (response.dragged_by(egui::PointerButton::Primary) || response.clicked())
                && let Some(hit) = pick.as_ref()
            {
                if self.selected.as_ref() == Some(&hit.entity) {
                    self.paint_at_uv(hit.uv);
                } else if response.clicked() {
                    self.select(Some(hit.entity.clone()));
                }
            }
            if let Some(hit) = &pick
                && self.selected.as_ref() == Some(&hit.entity)
            {
                self.studio.hover_uv = Some(hit.uv);
                if let Some(p) = point {
                    ui.painter()
                        .circle_stroke(p, 8., egui::Stroke::new(1., Color32::WHITE));
                }
            }
        } else {
            if response.clicked() {
                self.select(
                    ui_pick
                        .clone()
                        .or_else(|| pick.as_ref().map(|h| h.entity.clone())),
                );
            }
            if response.secondary_clicked() {
                self.context_target = ui_pick.or_else(|| pick.as_ref().map(|h| h.entity.clone()));
            }
            response.context_menu(|ui| {
                if let Some(id) = self.context_target.clone() {
                    self.object_context(ui, &id);
                } else {
                    self.creation_menu(ui);
                }
            });
            self.gizmo_ui(ui, rect, size);
            let clicks = self
                .game_ui
                .draw(ui, &self.state.project, &scene, &self.root(), rect);
            if let Some(id) = clicks.first() {
                self.select(Some(id.clone()));
            }
            if let Some(element) = self
                .selected
                .as_deref()
                .and_then(|id| scene.entity(id))
                .and_then(|e| e.ui.as_ref())
            {
                let outline = oxy_render::game_ui::element_rect(element, rect);
                ui.painter().rect_stroke(
                    outline,
                    0.,
                    egui::Stroke::new(1.5, Color32::GOLD),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let hint = if painting {
            "Pincel: esquerdo · órbita: direito · pan: central · zoom: roda"
        } else if self.scene().kind == SceneKind::TwoD {
            "Selecionar: esquerdo · pan: central · zoom: roda · arraste os eixos para transformar"
        } else {
            "Selecionar: esquerdo · órbita: direito · pan: central · zoom: roda"
        };
        ui.painter().text(
            rect.left_bottom() + Vec2::new(12., -14.),
            egui::Align2::LEFT_BOTTOM,
            hint,
            egui::FontId::proportional(12.),
            Color32::from_gray(170),
        );
    }
    fn gizmo_ui(&mut self, ui: &mut egui::Ui, rect: Rect, size: [u32; 2]) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(entity) = self.scene().entity(&id).cloned() else {
            return;
        };
        if entity.ui.is_some() {
            return;
        }
        let Ok(world) = self.scene().world_matrix(&id) else {
            return;
        };
        let pivot = world.transform_point3(Vec3::from(entity.transform.pivot));
        let Some(origin) = self.camera.world_to_screen(pivot, size) else {
            return;
        };
        let origin = rect.min + Vec2::from(origin);
        for (axis, color) in [
            (0, Color32::from_rgb(239, 113, 117)),
            (1, Color32::from_rgb(127, 215, 153)),
            (2, Color32::from_rgb(122, 169, 240)),
        ] {
            if self.scene().kind == SceneKind::TwoD && axis == 2 && self.gizmo != Gizmo::Rotate {
                continue;
            }
            let vector = [Vec3::X, Vec3::Y, Vec3::Z][axis];
            let parent = entity
                .parent
                .as_deref()
                .and_then(|p| self.scene().world_matrix(p).ok())
                .unwrap_or(glam::Mat4::IDENTITY);
            let world_axis = parent.transform_vector3(vector);
            let projected = self
                .camera
                .world_to_screen(pivot + world_axis, size)
                .map(|p| rect.min + Vec2::from(p) - origin)
                .unwrap_or(Vec2::ZERO);
            let direction = if projected.length() > 0.1 {
                projected.normalized()
            } else {
                Vec2::new(0.7, -0.7)
            };
            let endpoint = origin + direction * (70. + axis as f32 * 8.);
            let painter = ui.painter_at(rect);
            painter.line_segment([origin, endpoint], egui::Stroke::new(2., color));
            painter.circle_filled(endpoint, 6., color);
            painter.text(
                endpoint + Vec2::new(8., -8.),
                egui::Align2::LEFT_BOTTOM,
                ["X", "Y", "Z"][axis],
                egui::FontId::proportional(12.),
                color,
            );
            let _response = ui.interact(
                Rect::from_center_size(endpoint, Vec2::splat(20.)),
                ui.id().with(("gizmo", axis)),
                Sense::drag(),
            );
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } => Some(*pos),
                    _ => None,
                })
            });
            if let Some(pos) =
                pressed.filter(|p| Rect::from_center_size(endpoint, Vec2::splat(22.)).contains(*p))
            {
                self.gizmo_drag = Some(GizmoDrag {
                    entity: id.clone(),
                    base: entity.transform.clone(),
                    axis,
                    origin: pos,
                    direction,
                    pixels_per_unit: projected.length().max(1.),
                });
            }
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos())
                && let Some(drag) = &self.gizmo_drag
            {
                if drag.entity != id || drag.axis != axis {
                    continue;
                }
                let mut next = drag.base.clone();
                let amount = (pos - drag.origin).dot(drag.direction);
                match self.gizmo {
                    Gizmo::Move => {
                        next.position[axis] += amount / drag.pixels_per_unit;
                        if self.grid && !ui.input(|i| i.modifiers.alt) {
                            next.position[axis] =
                                (next.position[axis] / self.grid_size).round() * self.grid_size;
                        }
                    }
                    Gizmo::Rotate => {
                        next.rotation[axis] += amount * 0.012;
                    }
                    Gizmo::Scale => {
                        next.scale[axis] = drag.base.scale[axis] * (amount * 0.01).exp();
                    }
                }
                if let Some(e) = self.scene_mut().entity_mut(&id) {
                    e.transform = next;
                }
            }
        }
        if ui.input(|i| !i.pointer.primary_down()) {
            self.gizmo_drag = None;
        }
    }

    fn game(&mut self, ui: &mut egui::Ui, dt: f32) {
        if self.runtime.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(90.);
                ui.heading("Teste seu projeto");
                ui.label(
                    "Jogar cria uma cópia isolada da cena. Parar descarta o estado de execução.",
                );
                if ui.button("▶ Jogar cena selecionada").clicked() {
                    self.start();
                }
            });
            return;
        }
        if !self.capture {
            ui.horizontal(|ui| {
                ui.label("PAUSADO · entrada liberada");
                if ui.button("Retomar jogo").clicked() {
                    self.capture = true;
                    if let Some(rt) = &mut self.runtime {
                        rt.set_paused(false);
                    }
                    self.last_time = Instant::now();
                }
            });
        } else {
            ui.label("EM EXECUÇÃO · Escape pausa e libera a entrada");
        }
        if !ui.ctx().input(|i| i.focused) || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
            self.pause();
        }
        let input = read_input(ui.ctx(), &self.state.project, self.capture);
        let available = [
            ui.available_width().max(1.) as u32,
            ui.available_height().max(1.) as u32,
        ];
        let resized = self.game_size != Some(available);
        self.game_size = Some(available);
        if let Some(rt) = &mut self.runtime {
            rt.advance(if resized { 0. } else { dt }, &input);
        }
        let scene = self.runtime.as_ref().unwrap().scene().clone();
        let camera = CameraState::for_game(&scene);
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
            self.debug,
        );
        ui.painter().image(
            texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
        let clicks = self
            .game_ui
            .draw(ui, &self.state.project, &scene, &self.root(), rect);
        if self.capture {
            let mut targets = clicks;
            if targets.is_empty()
                && response.clicked()
                && let Some(p) = response.interact_pointer_pos()
                && let Some(hit) =
                    oxy_render::pick(&scene, &camera, size, [p.x - rect.min.x, p.y - rect.min.y])
            {
                targets.push(hit.entity);
            }
            if let Some(rt) = &mut self.runtime {
                for id in targets {
                    rt.click(&id);
                }
            }
        }
        let sounds = if let Some(rt) = &mut self.runtime {
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
    fn console(&mut self, ctx: &egui::Context) {
        if !self.console {
            return;
        }
        egui::TopBottomPanel::bottom("console")
            .resizable(true)
            .default_height(155.)
            .height_range(80.0..=400.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("CONSOLE E EXECUÇÃO");
                    if ui.button("Limpar").clicked() {
                        self.messages.clear();
                        if let Some(rt) = &mut self.runtime {
                            rt.logs.clear();
                        }
                    }
                });
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.messages {
                            ui.monospace(line);
                        }
                        if let Some(rt) = &self.runtime {
                            for line in &rt.logs {
                                ui.monospace(line);
                            }
                            for trace in rt.traces.iter().rev().take(5).rev() {
                                ui.small(format!(
                                    "{:.3}s · objeto {} · nó {} · {}",
                                    trace.time, trace.object, trace.node, trace.operation
                                ));
                            }
                        }
                    });
            });
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = self.last_time.elapsed().as_secs_f32().min(0.1);
        self.last_time = Instant::now();
        ctx.set_zoom_factor(self.scale);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Transition::Close);
            } else {
                self.allow_close = true
            }
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.capture && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save();
        }
        if !self.capture && !crate::graph_ui::text_input_active(ctx) {
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
            if self.tab != Tab::Logic && self.tab != Tab::Game {
                if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                    self.delete();
                }
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D)) {
                    self.duplicate();
                }
            }
        }
        if ctx.input(|i| i.pointer.any_down()) && !self.history.is_pending() {
            self.history.begin("Gesto de edição");
        }
        self.toolbar(ctx);
        self.console(ctx);
        match self.tab {
            Tab::Scene | Tab::Studio => {
                self.library(ctx);
                self.hierarchy(ctx);
                self.properties(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    if self.tab == Tab::Studio {
                        self.studio_ui(ui, dt);
                    } else {
                        self.viewport(ui, false);
                    }
                });
            }
            Tab::Logic => {
                self.hierarchy(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    let objects: Vec<_> = self
                        .scene()
                        .entities
                        .iter()
                        .map(|e| (e.id.clone(), e.name.clone()))
                        .collect();
                    if let Some(id) = self.selected.clone() {
                        let trace: Vec<_> = self
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
                        }
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Selecione um objeto para criar seu comportamento.");
                        });
                    }
                });
            }
            Tab::Game => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.game(ui, dt);
                });
            }
        }
        if self.pending.is_some() {
            self.pause();
            egui::Window::new("Alterações não salvas").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER,Vec2::ZERO).show(ctx,|ui|{
            ui.label("Há alterações no projeto e/ou nas texturas. Salve antes de continuar ou descarte explicitamente.");
            ui.horizontal(|ui|{if ui.button("Salvar e continuar").clicked()&&self.save()&& let Some(t)=self.pending.take(){self.apply_transition(t)}
if ui.button("Descartar alterações").clicked()&& let Some(t)=self.pending.take(){self.apply_transition(t)}
if ui.button("Cancelar").clicked(){self.pending=None;}});
        });
        }
        if !ctx.input(|i| i.pointer.any_down()) && !crate::graph_ui::text_input_active(ctx) {
            self.finish_history(true);
        } else if crate::graph_ui::text_input_active(ctx) && !self.history.is_pending() {
            self.history.begin("Editar valores");
        }
        let errors = self.renderer.take_errors();
        for error in errors {
            self.log(error);
            self.console = true;
        }
        for error in self.game_ui.take_errors() {
            self.log(error);
            self.console = true;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

fn component_switch<T>(ui: &mut egui::Ui, label: &str, value: &mut Option<T>, default: T) {
    let mut enabled = value.is_some();
    if ui.checkbox(&mut enabled, label).changed() {
        *value = if enabled { Some(default) } else { None };
    }
}
pub fn vector3(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64, nonzero: bool) {
    ui.label(label);
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
fn read_input(ctx: &egui::Context, project: &Project, enabled: bool) -> InputFrame {
    oxy_render::input::collect_input(ctx, &project.input_bindings, enabled)
}
