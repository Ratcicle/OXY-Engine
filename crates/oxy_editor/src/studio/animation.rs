//! Animation authoring. Poses under construction never enter the project document.
use crate::{
    app::{DockTab, Editor},
    graph_ui::text_input_active,
    icons::{self, Icon},
    theme, widgets,
};
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use oxy_core::{
    animation::{AnimationEvent, Clip, Interpolation, Keyframe, sample_clip},
    document::*,
};
use std::collections::{BTreeMap, HashSet};

const EPSILON: f32 = 0.00001;

fn record_drafts(
    state: &mut AnimationState,
    clip: &mut Clip,
    scope: &[Id],
) -> Result<usize, String> {
    let time = state.draft_time.unwrap_or(state.time);
    if (time - state.time).abs() > EPSILON || time > clip.duration {
        return Err("Retorne ao tempo da pose provisória antes de gravar.".into());
    }
    let targets: Vec<_> = state
        .drafts
        .keys()
        .filter(|id| scope.contains(id))
        .cloned()
        .collect();
    let count = targets.len();
    for target in targets {
        let transform = state.drafts.remove(&target).unwrap();
        clip.insert_key(
            &target,
            Keyframe {
                time,
                transform,
                interpolation: Interpolation::Linear,
            },
        );
    }
    if state.drafts.is_empty() {
        state.draft_time = None;
    }
    state.preview = true;
    Ok(count)
}

#[derive(Clone, Debug, PartialEq)]
enum TimelineSelection {
    None,
    Key { target: Id, time: f32 },
    Event(usize),
}
#[derive(Clone, Debug)]
struct Rename {
    id: Id,
    text: String,
    focus: bool,
}

pub struct AnimationState {
    pub selected: Option<Id>,
    pub time: f32,
    pub preview: bool,
    pub drafts: BTreeMap<Id, Transform>,
    pub draft_time: Option<f32>,
    manual_model: Option<Id>,
    selection: TimelineSelection,
    expanded: HashSet<Id>,
    rename: Option<Rename>,
    copied_key: Option<Keyframe>,
    copied_event: Option<AnimationEvent>,
    marker_name: String,
    event_name: Option<(Id, usize, String)>,
    message: String,
    references: Vec<String>,
    /// Bottom area of the Animation: `None` shows the timeline, otherwise a dock tab.
    pub bottom: Option<DockTab>,
}
impl Default for AnimationState {
    fn default() -> Self {
        Self {
            selected: None,
            time: 0.0,
            preview: false,
            drafts: BTreeMap::new(),
            draft_time: None,
            manual_model: None,
            selection: TimelineSelection::None,
            expanded: HashSet::new(),
            rename: None,
            copied_key: None,
            copied_event: None,
            marker_name: "Impacto".into(),
            event_name: None,
            message: String::new(),
            references: Vec::new(),
            bottom: None,
        }
    }
}

/// A selected root/group owns its own animations. Leaves use the nearest animated ancestor.
fn automatic_model(scene: &Scene, selected: Option<&str>) -> Option<Id> {
    let entity = scene.entity(selected?)?;
    if entity.parent.is_none() || !entity.has_geometry() || !entity.clips.is_empty() {
        return Some(entity.id.clone());
    }
    let mut current = entity.parent.as_deref();
    let mut top = entity.id.clone();
    let mut visited = HashSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            break;
        }
        let ancestor = scene.entity(id)?;
        top = ancestor.id.clone();
        if !ancestor.clips.is_empty() {
            return Some(top);
        }
        current = ancestor.parent.as_deref();
    }
    Some(top)
}

fn animation_references(project: &Project, animation: &str) -> Vec<String> {
    let mut references = Vec::new();
    for scene in &project.scenes {
        for entity in &scene.entities {
            for node in &entity.graph.nodes {
                if node.operation == "action.animation" && node.text("clip") == animation {
                    references.push(format!(
                        "Cena {} › {} › Reproduzir animação (nó {})",
                        scene.name, entity.name, node.id
                    ));
                }
            }
        }
    }
    for asset in &project.assets {
        if let Some(model) = &asset.model {
            for entity in model {
                for node in &entity.graph.nodes {
                    if node.operation == "action.animation" && node.text("clip") == animation {
                        references.push(format!(
                            "Modelo {} › {} › Reproduzir animação (nó {})",
                            asset.name, entity.name, node.id
                        ));
                    }
                }
            }
        }
    }
    references
}

fn remove_animation(
    project: &mut Project,
    scene: &str,
    model: &str,
    animation: &str,
) -> Result<(), Vec<String>> {
    let references = animation_references(project, animation);
    if !references.is_empty() {
        return Err(references);
    }
    if let Some(entity) = project
        .scene_mut(scene)
        .and_then(|scene| scene.entity_mut(model))
    {
        entity.clips.retain(|clip| clip.id != animation);
    }
    Ok(())
}

fn preview_scene(scene: &Scene, clip: Option<&Clip>, state: &AnimationState) -> Scene {
    let mut preview = scene.clone();
    if state.preview
        && let Some(clip) = clip
    {
        sample_clip(&mut preview, clip, state.time);
    }
    for (target, transform) in &state.drafts {
        if let Some(entity) = preview.entity_mut(target) {
            entity.transform = transform.clone();
        }
    }
    preview
}

fn unique_name(clips: &[Clip], prefix: &str) -> String {
    if !clips.iter().any(|clip| clip.name == prefix) {
        return prefix.into();
    }
    for number in 2.. {
        let name = format!("{prefix} {number}");
        if !clips.iter().any(|clip| clip.name == name) {
            return name;
        }
    }
    unreachable!()
}

fn duplicate_animation(clips: &[Clip], id: &str) -> Option<Clip> {
    let source = clips.iter().find(|clip| clip.id == id)?;
    let mut copy = source.clone();
    copy.id = new_id();
    copy.name = unique_name(clips, &format!("{} (cópia)", source.name));
    Some(copy)
}

impl Editor {
    pub(crate) fn finish_animation_rename(&mut self) -> Result<(), String> {
        let Some(rename) = self.studio.animation.rename.take() else {
            return Ok(());
        };
        let name = rename.text.trim();
        if name.is_empty() {
            self.studio.animation.rename = Some(rename);
            return Err(
                "Informe o nome da animação ou pressione Escape para cancelar a renomeação.".into(),
            );
        }
        if let Some(clip) = self
            .scene_mut()
            .entities
            .iter_mut()
            .flat_map(|e| &mut e.clips)
            .find(|c| c.id == rename.id)
        {
            clip.name = name.into();
        }
        Ok(())
    }
    fn chosen_animation(&self) -> Option<&Clip> {
        self.studio
            .owner
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .and_then(|entity| {
                entity
                    .clips
                    .iter()
                    .find(|clip| Some(&clip.id) == self.studio.animation.selected.as_ref())
            })
    }
    pub fn animation_preview(&self) -> Scene {
        if self.spatial.base_pose {
            return self.scene().clone();
        }
        preview_scene(
            self.scene(),
            self.chosen_animation(),
            &self.studio.animation,
        )
    }
    pub fn animation_draft_transform(&self, id: &str) -> Option<Transform> {
        self.animation_preview()
            .entity(id)
            .map(|entity| entity.transform.clone())
    }
    pub fn set_animation_draft_transform(&mut self, id: &str, transform: Transform) {
        if !transform.finite() || self.scene().entity(id).is_none() {
            return;
        }
        if self.studio.animation.drafts.is_empty() {
            self.studio.animation.draft_time = Some(self.studio.animation.time);
        }
        if self
            .studio
            .animation
            .draft_time
            .is_some_and(|time| (time - self.studio.animation.time).abs() > EPSILON)
        {
            self.studio.animation.message =
                "Retorne ao tempo da pose provisória antes de editá-la.".into();
            return;
        }
        self.studio.animation.drafts.insert(id.into(), transform);
        self.studio.animation.selection = TimelineSelection::None;
        self.studio.animation.preview = true;
        self.studio.playing = false;
    }
    pub fn open_animation_for(&mut self, id: &str) {
        self.studio.animation.manual_model = None;
        if self.studio.animation.drafts.is_empty() {
            self.studio.owner = automatic_model(self.scene(), Some(id));
        }
        self.studio.playing = false;
    }
    fn sync_animation_model(&mut self) {
        let keep_model = self.chosen_animation().is_some()
            && self.studio.owner.as_deref().is_some_and(|owner| {
                self.selected
                    .as_ref()
                    .is_some_and(|selected| self.scene().descendants(owner).contains(selected))
            });
        let automatic = if keep_model {
            self.studio.owner.clone()
        } else {
            automatic_model(self.scene(), self.selected.as_deref())
        };
        if let TimelineSelection::Key { target, .. } = &self.studio.animation.selection
            && self.selected.as_ref() != Some(target)
        {
            self.studio.animation.selection = TimelineSelection::None;
        }
        let wanted = self
            .studio
            .animation
            .manual_model
            .clone()
            .filter(|id| self.scene().entity(id).is_some())
            .or(automatic);
        if wanted != self.studio.owner && wanted.is_some() {
            if !self.studio.animation.drafts.is_empty() {
                self.studio.animation.message =
                    "Grave ou descarte a pose provisória antes de trocar o modelo.".into();
            } else {
                self.studio.owner = wanted;
                self.studio.animation.selected = None;
                self.studio.animation.selection = TimelineSelection::None;
                self.studio.animation.time = 0.0;
                self.studio.playing = false;
            }
        }
        let selected = self.studio.animation.selected.clone();
        let choices = self
            .studio
            .owner
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .map(|entity| entity.clips.as_slice())
            .unwrap_or_default();
        if selected
            .as_ref()
            .is_none_or(|id| !choices.iter().any(|clip| &clip.id == id))
        {
            self.studio.animation.selected = choices.first().map(|clip| clip.id.clone());
            self.studio.animation.selection = TimelineSelection::None;
        }
    }
    fn choose_animation(&mut self, id: &str) {
        self.hierarchy_ui.focus = false;
        if self.studio.animation.selected.as_deref() == Some(id) {
            return;
        }
        if !self.studio.animation.drafts.is_empty() {
            self.studio.animation.message =
                "Grave ou descarte a pose provisória antes de trocar a animação.".into();
            return;
        }
        self.studio.animation.selected = Some(id.into());
        self.studio.animation.time = 0.0;
        self.studio.animation.preview = true;
        self.studio.playing = false;
        self.studio.animation.selection = TimelineSelection::None;
    }
    /// Creates an animation for the current model and opens its name for editing.
    fn new_animation(&mut self) {
        let Some(model) = self.studio.owner.clone() else {
            return;
        };
        if !self.studio.animation.drafts.is_empty() {
            self.studio.animation.message =
                "Grave ou descarte a pose provisória antes de criar outra animação.".into();
            return;
        }
        let clips = self
            .scene()
            .entity(&model)
            .map(|entity| entity.clips.clone())
            .unwrap_or_default();
        let clip = Clip::new(unique_name(&clips, "Nova animação"));
        let id = clip.id.clone();
        let name = clip.name.clone();
        if let Some(entity) = self.scene_mut().entity_mut(&model) {
            entity.clips.push(clip);
        }
        self.choose_animation(&id);
        self.studio.animation.rename = Some(Rename {
            id,
            text: name,
            focus: true,
        });
    }
    fn duplicate_chosen_animation(&mut self, id: &str) {
        let Some(model) = self.studio.owner.clone() else {
            return;
        };
        let clips = self
            .scene()
            .entity(&model)
            .map(|entity| entity.clips.clone())
            .unwrap_or_default();
        if let Some(copy) = duplicate_animation(&clips, id) {
            let id = copy.id.clone();
            if let Some(entity) = self.scene_mut().entity_mut(&model) {
                entity.clips.push(copy);
            }
            self.choose_animation(&id);
        }
    }
    /// Rename, duplicate and delete, shared by the selector menu and its context menu.
    fn animation_actions(&mut self, ui: &mut egui::Ui, id: &str, name: &str) {
        if widgets::menu_item(ui, "Renomear", "F2").clicked() {
            self.studio.animation.rename = Some(Rename {
                id: id.into(),
                text: name.into(),
                focus: true,
            });
            ui.close();
        }
        if ui
            .add_enabled(
                self.studio.animation.drafts.is_empty(),
                egui::Button::new("Duplicar animação"),
            )
            .on_disabled_hover_text("Grave ou descarte a pose provisória antes de duplicar.")
            .clicked()
        {
            self.duplicate_chosen_animation(id);
            ui.close();
        }
        if ui
            .button(egui::RichText::new("Excluir animação").color(theme::ERROR))
            .clicked()
        {
            self.delete_selected_animation(id);
            ui.close();
        }
    }
    /// Animation being edited: choose another one or create, rename, duplicate or delete.
    fn animation_clip_selector(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some(model) = self.studio.owner.clone() else {
            return;
        };
        let clips = self
            .scene()
            .entity(&model)
            .map(|entity| entity.clips.clone())
            .unwrap_or_default();
        let current = self
            .chosen_animation()
            .map(|clip| (clip.id.clone(), clip.name.clone()));
        let label = current
            .as_ref()
            .map_or("Sem animação", |(_, name)| name.as_str())
            .to_owned();
        let mut chosen = None;
        let mut create = false;
        let response = egui::ComboBox::from_id_salt("animation_clip")
            .width(widgets::combo_inner_width(ui, width))
            // Room for the clips and every action, without a scroll bar in the list.
            .height(480.)
            .selected_text(egui::RichText::new(&label).font(theme::medium(13.)))
            .truncate()
            .show_ui(ui, |ui| {
                widgets::menu_label(ui, "Animações do modelo");
                for clip in &clips {
                    let selected = current.as_ref().is_some_and(|(id, _)| id == &clip.id);
                    let item = egui::Button::selectable(selected, clip.name.as_str())
                        .shortcut_text(format!("{:.2} s", clip.duration));
                    if ui.add(item).clicked() {
                        chosen = Some(clip.id.clone());
                    }
                }
                ui.separator();
                if ui.button("+ Nova animação").clicked() {
                    create = true;
                }
                if let Some((id, name)) = &current {
                    self.animation_actions(ui, id, name);
                }
            })
            .response
            .on_hover_text("Animação em edição: troque, crie, renomeie, duplique ou exclua.");
        icons::register_qa(ui, "Animação em edição", response.rect);
        if let Some((id, name)) = &current {
            // `Response::context_menu` shares the combo's popup id and would close the
            // list on every left click: the right-click menu needs an id of its own.
            egui::Popup::context_menu(&response)
                .id(response.id.with("clip_actions"))
                .show(|ui| self.animation_actions(ui, id, name));
        }
        if let Some(id) = chosen {
            self.choose_animation(&id);
        }
        if icons::small(ui, Icon::Plus, "+ Nova animação", 26.)
            .on_hover_text("Nova animação deste modelo.")
            .clicked()
            || create
        {
            self.new_animation();
        }
        if let Some((id, name)) = current
            && !self.hierarchy_ui.focus
            && !text_input_active(ui.ctx())
            && ui.input(|input| input.key_pressed(egui::Key::F2))
        {
            self.studio.animation.rename = Some(Rename {
                id,
                text: name,
                focus: true,
            });
        }
    }
    /// Shown in the timeline area while the model has no animation yet.
    fn animation_empty(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        if self.studio.owner.is_none() {
            widgets::empty_state(
                ui,
                area,
                Icon::Clip,
                "Nenhum modelo escolhido",
                "Selecione uma raiz, grupo ou peça para começar.",
                |_| {},
            );
            return;
        }
        widgets::empty_state(
            ui,
            area,
            Icon::Clip,
            "Este modelo ainda não tem animações",
            "Crie uma animação e grave a primeira pose com + Quadro-chave.",
            |ui| {
                if ui
                    .add_sized(
                        [180., 28.],
                        egui::Button::new(
                            egui::RichText::new("+ Nova animação").color(Color32::WHITE),
                        )
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    self.new_animation();
                }
            },
        );
    }
    fn delete_selected_animation(&mut self, id: &str) {
        if !self.studio.animation.drafts.is_empty() {
            self.studio.animation.message =
                "Grave ou descarte a pose provisória antes de excluir a animação.".into();
            return;
        }
        let Some(model) = self.studio.owner.clone() else {
            return;
        };
        let scene = self.scene_id.clone();
        match remove_animation(&mut self.state.project, &scene, &model, id) {
            Ok(()) => {
                self.studio.animation.selected = None;
                self.studio.animation.selection = TimelineSelection::None;
                self.studio.animation.preview = false;
                self.studio.playing = false;
            }
            Err(references) => {
                self.studio.animation.references = references;
            }
        }
    }
    /// State of the animation preview for the viewport status line.
    pub(crate) fn animation_status(&self) -> (egui::Color32, String, &'static str) {
        use crate::theme;
        if self.chosen_animation().is_none() {
            return (
                theme::TEXT_MUTED,
                "Sem animação".into(),
                "crie uma animação na linha do tempo",
            );
        }
        if !self.studio.animation.preview {
            return (
                theme::TEXT_MUTED,
                "Mostrando a pose-base".into(),
                "a animação fica oculta até desligar Pose-base",
            );
        }
        if !self.studio.animation.drafts.is_empty() {
            let time = self
                .studio
                .animation
                .draft_time
                .map_or_else(String::new, |t| format!(" em {t:.2} s"));
            return (
                theme::HIGHLIGHT,
                format!("Pose provisória{time}"),
                "grave ou descarte na linha do tempo",
            );
        }
        if self.studio.playing {
            return (
                theme::SUCCESS,
                "Reproduzindo".into(),
                "clique na linha do tempo para pausar",
            );
        }
        match self
            .selected
            .as_deref()
            .and_then(|id| self.scene().entity(id))
        {
            Some(entity) => (
                theme::ACCENT_BRIGHT,
                format!("{} pronta para posar", entity.name),
                "mova ou gire a peça para criar uma pose provisória",
            ),
            None => (
                theme::TEXT_MUTED,
                "Nenhuma peça selecionada".into(),
                "escolha uma peça na hierarquia ou na linha do tempo",
            ),
        }
    }
    /// Switches the view between the animated pose and the model's original pose.
    pub(crate) fn pose_base_toggle(&mut self, ui: &mut egui::Ui) {
        let available = self.chosen_animation().is_some();
        let showing = available && !self.studio.animation.preview;
        let response = ui
            .add_enabled_ui(available && self.studio.animation.drafts.is_empty(), |ui| {
                crate::icons::button(
                    ui,
                    crate::icons::Icon::Person,
                    "Pose-base",
                    "Mostra a pose original do modelo, sem a animação. Grave ou descarte a pose provisória primeiro.",
                    showing,
                    true,
                )
            })
            .inner;
        if response.clicked() {
            self.studio.animation.preview = showing;
            self.studio.playing = false;
        }
    }
    /// Model context lives in the first Studio row: whose animations are edited.
    pub(super) fn animation_context_controls(&mut self, ui: &mut egui::Ui, _compact: bool) {
        if !self.spatial.base_pose {
            self.sync_animation_model();
        }
        if ui.available_width() >= 160. {
            ui.label(
                egui::RichText::new("Modelo")
                    .size(12.5)
                    .color(theme::TEXT_MUTED),
            );
        }
        let current = self
            .studio
            .owner
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .map(|entity| entity.name.clone())
            .unwrap_or_else(|| "Selecione uma peça".into());
        // Only the isolated object and its parts can own the animations edited here.
        let candidates: Vec<_> = self
            .studio_subtree()
            .iter()
            .filter_map(|id| self.scene().entity(id))
            .map(|entity| (entity.id.clone(), entity.name.clone()))
            .collect();
        let mut selected = self.studio.animation.manual_model.clone();
        let options = |ui: &mut egui::Ui, selected: &mut Option<Id>| {
            ui.selectable_value(selected, None, "Automático pela seleção");
            ui.separator();
            for (id, name) in &candidates {
                ui.selectable_value(selected, Some(id.clone()), name);
            }
        };
        if ui.available_width() >= 100. {
            let width = (ui.available_width() - 8.).clamp(90., 220.);
            let response = egui::ComboBox::from_id_salt("animation_model")
                .width(widgets::combo_inner_width(ui, width))
                .selected_text(&current)
                .truncate()
                .show_ui(ui, |ui| options(ui, &mut selected))
                .response
                .on_hover_text("Modelo animável: a raiz, grupo ou peça dona das animações.");
            icons::register_qa(ui, "Modelo", response.rect);
        } else {
            icons::menu_button(
                ui,
                Icon::Person,
                "Modelo",
                &format!("Modelo animável: {current}."),
                false,
                |ui| options(ui, &mut selected),
            );
        }
        if selected != self.studio.animation.manual_model {
            if self.studio.animation.drafts.is_empty() {
                self.studio.animation.manual_model = selected;
                self.sync_animation_model();
            } else {
                self.studio.animation.message =
                    "Grave ou descarte a pose provisória antes de trocar o modelo.".into();
            }
        }
    }
    pub(super) fn animation_ui(&mut self, ui: &mut egui::Ui, dt: f32, compact: bool) {
        if self.spatial.base_pose {
            self.viewport(ui, false);
            return;
        }
        if self.studio.animation.drafts.is_empty() {
            self.studio.animation.draft_time = None;
        } else if let Some(time) = self.studio.animation.draft_time {
            self.studio.animation.time = time;
        }
        self.sync_animation_model();
        let warning = self.studio.animation.message.clone();
        if self.studio.animation_warning.as_deref() != Some(warning.as_str()) {
            self.studio.animation_warning = Some(warning.clone());
            if !warning.is_empty() {
                self.warn(warning);
            }
        }
        let mut clip = self.chosen_animation().cloned();
        let old_clip = clip.clone();
        if let Some(clip) = &clip
            && self.studio.playing
            && self.studio.animation.drafts.is_empty()
        {
            self.studio.animation.time += dt.clamp(0.0, 0.1);
            if self.studio.animation.time >= clip.duration {
                if clip.looping {
                    self.studio.animation.time %= clip.duration;
                } else {
                    self.studio.animation.time = clip.duration;
                    self.studio.playing = false;
                }
            }
        }
        // Keep space for the viewport tools and a useful drawing area.
        let timeline_max = if Self::compact_layout(ui.ctx()) {
            (ui.available_height() * 0.5).clamp(120., 200.)
        } else if compact {
            (ui.available_height() * 0.5)
                .max(180.0)
                .min((ui.available_height() - 155.0).max(140.0))
        } else {
            600.0
        };
        egui::TopBottomPanel::bottom(if compact {
            "animation_timeline_compact"
        } else {
            "animation_timeline"
        })
        .frame(
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 0,
                bottom: 4,
            }),
        )
        .resizable(true)
        .default_height(if compact { timeline_max } else { 300.0 })
        .height_range(150.0_f32.min(timeline_max)..=timeline_max)
        .show_inside(ui, |ui| {
            // egui keeps the height of the panel's content: fill the panel so a short
            // tab, such as the empty state, never shrinks the timeline for later.
            ui.set_min_height(ui.available_height());
            self.animation_bottom_tabs(ui);
            match (self.studio.animation.bottom, clip.as_mut()) {
                (Some(DockTab::Console), _) => self.console_body(ui),
                (Some(DockTab::Notices), _) => self.notices_body(ui),
                (_, Some(clip)) => {
                    self.animation_toolbar(ui, clip);
                    self.animation_draft_banner(ui, clip);
                    self.animation_timeline(ui, clip);
                }
                (_, None) => self.animation_empty(ui),
            }
        });
        if let Some(clip) = &mut clip
            && !ui.input(|input| input.pointer.primary_down())
        {
            for track in &mut clip.tracks {
                track.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
        }
        if !self.studio.animation.drafts.is_empty()
            && let Some(time) = self.studio.animation.draft_time
            && (time - self.studio.animation.time).abs() > EPSILON
        {
            self.studio.animation.time = time;
            self.studio.animation.message =
                "Grave ou descarte as poses alteradas antes de mudar o cursor.".into();
        }
        if let Some(clip) = clip
            && Some(&clip) != old_clip.as_ref()
        {
            self.store_animation(clip);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| self.viewport(ui, false));
        self.animation_reference_dialog(ui.ctx());
    }
    /// Writes an edited copy of the chosen animation back to its model.
    fn store_animation(&mut self, clip: Clip) {
        if let Some(owner) = self.studio.owner.clone()
            && let Some(existing) = self.scene_mut().entity_mut(&owner).and_then(|entity| {
                entity
                    .clips
                    .iter_mut()
                    .find(|existing| existing.id == clip.id)
            })
        {
            *existing = clip;
        }
    }
    /// Timeline, Console and notices share the bottom of the Animation.
    fn animation_bottom_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_height(theme::PANEL_HEADER_HEIGHT);
            let items = [
                (None, "Linha do tempo", None),
                (Some(DockTab::Console), "Console", None),
                (
                    Some(DockTab::Notices),
                    "Avisos",
                    Some(self.unread_notices()),
                ),
            ];
            if let Some(tab) = widgets::tabs(ui, Some(self.studio.animation.bottom), &items) {
                self.studio.animation.bottom = tab;
            }
        });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.min_rect().bottom(),
            egui::Stroke::new(1., theme::BORDER),
        );
        ui.add_space(2.);
    }
    fn animation_toolbar(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        let scope = self
            .studio
            .owner
            .as_deref()
            .map(|id| self.scene().descendants(id))
            .unwrap_or_default();
        let selected = self.selected.clone().filter(|id| scope.contains(id));
        // One row needs room for the clip, playback, the cursor and the key actions.
        let two_rows = ui.available_width() < 700.;
        let (rect, mut right) = widgets::toolbar_right(ui, crate::studio::ROW_HEIGHT + 6.);
        icons::menu_button(
            &mut right,
            Icon::More,
            "Mais ações da linha do tempo",
            "Colar quadros e eventos.",
            false,
            |ui| {
                self.animation_paste_key_button(ui, clip, selected.as_deref());
                ui.separator();
                if ui.button("+ Evento no cursor").clicked() {
                    self.add_animation_event(clip);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.studio.animation.copied_event.is_some(),
                        egui::Button::new("Colar evento"),
                    )
                    .clicked()
                    && let Some(mut event) = self.studio.animation.copied_event.clone()
                {
                    event.time = self.studio.animation.time;
                    clip.events.push(event);
                    self.studio.animation.selection =
                        TimelineSelection::Event(clip.events.len() - 1);
                    ui.close();
                }
            },
        );
        self.animation_record_key_button(&mut right, clip, selected.as_deref());
        let mut left = widgets::toolbar_left(ui, rect, &right);
        if !two_rows {
            let width = (left.available_width() * 0.28).clamp(72., 180.);
            self.animation_clip_selector(&mut left, width);
            widgets::divider(&mut left);
            self.animation_playback_controls(&mut left, clip);
            widgets::divider(&mut left);
            self.animation_cursor_control(&mut left, clip);
            return;
        }
        // Narrow timelines keep the clip and its actions on the first row, and move
        // playback and the cursor to a second one instead of letting them overlap.
        let width = (left.available_width() - 34.).clamp(72., 220.);
        self.animation_clip_selector(&mut left, width);
        let (row, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), crate::studio::ROW_HEIGHT),
            Sense::hover(),
        );
        let mut second = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        self.animation_playback_controls(&mut second, clip);
        widgets::divider(&mut second);
        self.animation_cursor_control(&mut second, clip);
    }
    fn add_animation_event(&mut self, clip: &mut Clip) {
        if !self.studio.animation.drafts.is_empty() {
            return;
        }
        let name = self.studio.animation.marker_name.trim();
        clip.events.push(AnimationEvent {
            time: self.studio.animation.time,
            name: if name.is_empty() { "Impacto" } else { name }.into(),
        });
        self.studio.animation.selection = TimelineSelection::Event(clip.events.len() - 1);
    }
    fn animation_playback_controls(&mut self, ui: &mut egui::Ui, clip: &Clip) {
        let free = self.studio.animation.drafts.is_empty();
        let blocked = "Grave ou descarte a pose provisória antes de reproduzir ou mover o cursor.";
        let start = ui
            .add_enabled_ui(free, |ui| icons::small(ui, Icon::SkipStart, "Início", 26.))
            .inner;
        if start.on_disabled_hover_text(blocked).clicked() {
            self.studio.playing = false;
            self.studio.animation.time = 0.0;
            self.studio.animation.preview = true;
        }
        let (icon, label, name) = if self.studio.playing {
            (Icon::Pause, "Pausar", "Ⅱ Pausar")
        } else {
            (Icon::Play, "Reproduzir", "▶ Reproduzir")
        };
        let compact = ui.available_width() < 300.;
        let response = ui
            .add_enabled_ui(free, |ui| {
                widgets::icon_text_button(ui, icon, label, false, compact)
            })
            .inner;
        icons::register_qa(ui, name, response.rect);
        if response.on_disabled_hover_text(blocked).clicked() {
            self.studio.playing = !self.studio.playing;
            self.studio.animation.preview = true;
            if self.studio.animation.time >= clip.duration {
                self.studio.animation.time = 0.0;
            }
        }
    }
    fn animation_cursor_control(&mut self, ui: &mut egui::Ui, clip: &Clip) {
        ui.label(
            egui::RichText::new("Cursor")
                .size(12.5)
                .color(theme::TEXT_MUTED),
        );
        if ui
            .add_enabled(
                self.studio.animation.drafts.is_empty(),
                egui::DragValue::new(&mut self.studio.animation.time)
                    .range(0.0..=clip.duration)
                    .speed(1.0 / 60.0)
                    .max_decimals(2)
                    .suffix(" s"),
            )
            .on_hover_text("Instante da linha do tempo.")
            .on_disabled_hover_text(
                "O cursor fica preso ao tempo da pose provisória até gravar ou descartar.",
            )
            .changed()
        {
            self.studio.playing = false;
            self.studio.animation.preview = true;
        }
        if ui.available_width() > 64. {
            ui.label(
                egui::RichText::new(format!("de {:.2} s", clip.duration))
                    .size(12.)
                    .color(theme::TEXT_MUTED),
            );
        }
    }
    /// Pending poses are visible, with their two ways out.
    fn animation_draft_banner(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        if self.studio.animation.drafts.is_empty() {
            return;
        }
        let scope = self
            .studio
            .owner
            .as_deref()
            .map(|id| self.scene().descendants(id))
            .unwrap_or_default();
        let names: Vec<_> = self
            .studio
            .animation
            .drafts
            .keys()
            .filter_map(|id| self.scene().entity(id).map(|entity| entity.name.clone()))
            .collect();
        let who = match names.as_slice() {
            [one] => one.clone(),
            many => format!("{} peças", many.len()),
        };
        let time = self
            .studio
            .animation
            .draft_time
            .unwrap_or(self.studio.animation.time);
        let text = format!(
            "Pose provisória em {who}, {time:.2} s. Grave para guardar ou descarte para voltar."
        );
        ui.add_space(4.);
        widgets::banner(ui, &text, |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new("Gravar poses alteradas")
                            .color(Color32::from_rgb(240, 212, 154)),
                    )
                    .fill(Color32::from_rgb(58, 46, 26)),
                )
                .on_hover_text("Grava todas as peças e grupos alterados no tempo da pose provisória, em uma operação de desfazer.")
                .clicked()
                && let Err(e) = record_drafts(&mut self.studio.animation, clip, &scope)
            {
                self.studio.animation.message = e;
            }
            if ui.button("Descartar").clicked() {
                self.studio.animation.drafts.clear();
                self.studio.animation.draft_time = None;
                self.studio.animation.message.clear();
            }
        });
        ui.add_space(2.);
    }
    fn animation_record_key_button(
        &mut self,
        ui: &mut egui::Ui,
        clip: &mut Clip,
        selected: Option<&str>,
    ) {
        let button = egui::Button::new(egui::RichText::new("+ Quadro-chave").color(Color32::WHITE))
            .fill(theme::ACCENT);
        if ui.add_enabled(selected.is_some(), button).on_hover_text("Grava posição, rotação e escala da peça selecionada no cursor, incluindo a pose provisória.").on_disabled_hover_text("Selecione uma peça do modelo na hierarquia ou na linha do tempo.").clicked()
                && let Some(target) = selected && let Some(transform) = self.animation_draft_transform(target) {
                clip.insert_key(target, Keyframe { time: self.studio.animation.time, transform, interpolation: Interpolation::Linear });
                self.studio.animation.drafts.remove(target); self.studio.animation.selection = TimelineSelection::Key { target: target.to_owned(), time: self.studio.animation.time };
                self.studio.animation.preview = true; self.studio.animation.message.clear();
            }
    }
    fn animation_paste_key_button(
        &mut self,
        ui: &mut egui::Ui,
        clip: &mut Clip,
        selected: Option<&str>,
    ) {
        if ui
            .add_enabled(
                selected.is_some() && self.studio.animation.copied_key.is_some(),
                egui::Button::new("Colar quadro"),
            )
            .on_hover_text("Cola o quadro copiado no cursor, na peça selecionada.")
            .clicked()
            && let (Some(target), Some(mut key)) =
                (selected, self.studio.animation.copied_key.clone())
        {
            key.time = self.studio.animation.time;
            clip.insert_key(target, key);
            self.studio.animation.selection = TimelineSelection::Key {
                target: target.to_owned(),
                time: self.studio.animation.time,
            };
            self.studio.animation.preview = true;
            ui.close();
        }
    }
    fn animation_timeline(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        let Some(owner) = self.studio.owner.clone() else {
            return;
        };
        let name_width = (ui.available_width() * 0.32).clamp(120.0, 196.0);
        let scope = self.scene().descendants(&owner);
        let ids: Vec<_> = oxy_core::editing::hierarchy_order(self.scene())
            .into_iter()
            .filter(|id| scope.contains(id))
            .collect();
        let entities: Vec<_> = ids
            .iter()
            .filter_map(|id| self.scene().entity(id).cloned())
            .collect();
        let free = self.studio.animation.drafts.is_empty();
        egui::ScrollArea::vertical()
            .id_salt("animation_tracks")
            // The default 64-point minimum can exceed the remaining compact timeline.
            // Let the scroll viewport use its actual height so its last row is reachable.
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                // Ruler: click or drag to move the cursor.
                let ruler = ui
                    .horizontal(|ui| {
                        let (name, _) =
                            ui.allocate_exact_size(Vec2::new(name_width, 26.0), Sense::hover());
                        ui.painter().text(
                            Pos2::new(name.left() + 10.0, name.center().y),
                            egui::Align2::LEFT_CENTER,
                            "Tempo (s)",
                            egui::FontId::proportional(11.5),
                            theme::TEXT_MUTED,
                        );
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new((ui.available_width() - 12.0).max(10.0), 26.0),
                            Sense::click_and_drag(),
                        );
                        let painter = ui.painter();
                        // Label every major tick that leaves room for "0.00"; 10 major
                        // intervals divide evenly by 1, 2, 5 and 10.
                        let spacing = rect.width() / 10.0;
                        let every = [1, 2, 5, 10]
                            .into_iter()
                            .find(|step| spacing * *step as f32 >= 40.0)
                            .unwrap_or(10);
                        for tick in 0..=20 {
                            let x = rect.left() + rect.width() * tick as f32 / 20.0;
                            let major = tick % 2 == 0;
                            painter.line_segment(
                                [
                                    Pos2::new(x, rect.bottom() - if major { 7.0 } else { 4.0 }),
                                    Pos2::new(x, rect.bottom()),
                                ],
                                egui::Stroke::new(
                                    1.0,
                                    Color32::from_gray(if major { 92 } else { 66 }),
                                ),
                            );
                            if major && (tick / 2) % every == 0 {
                                let time = clip.duration * tick as f32 / 20.0;
                                painter.text(
                                    Pos2::new(x, rect.top() + 8.0),
                                    egui::Align2::CENTER_CENTER,
                                    format!("{time:.2}"),
                                    egui::FontId::monospace(10.5),
                                    Color32::from_gray(150),
                                );
                            }
                        }
                        painter.hline(
                            rect.x_range(),
                            rect.bottom(),
                            egui::Stroke::new(1.0, theme::BORDER),
                        );
                        if free
                            && (response.clicked() || response.dragged())
                            && let Some(pointer) = response.interact_pointer_pos()
                        {
                            self.studio.animation.time =
                                timeline_time(pointer.x, rect, clip.duration);
                            self.studio.animation.preview = true;
                            self.studio.playing = false;
                        }
                        rect
                    })
                    .inner;
                // Events lane.
                ui.horizontal(|ui| {
                    let (name, _) =
                        ui.allocate_exact_size(Vec2::new(name_width, 28.0), Sense::hover());
                    icons::paint(
                        ui,
                        Rect::from_center_size(
                            Pos2::new(name.left() + 16.0, name.center().y),
                            Vec2::splat(13.0),
                        ),
                        Icon::Flag,
                        theme::HIGHLIGHT,
                    );
                    ui.painter().text(
                        Pos2::new(name.left() + 30.0, name.center().y),
                        egui::Align2::LEFT_CENTER,
                        "Eventos",
                        egui::FontId::proportional(12.5),
                        Color32::from_rgb(232, 210, 160),
                    );
                    let add = Rect::from_center_size(
                        Pos2::new(name.right() - 16.0, name.center().y),
                        Vec2::splat(22.0),
                    );
                    let response = ui
                        .interact(add, ui.id().with("animation_event_add"), Sense::click())
                        .on_hover_text("Adiciona um evento no cursor.");
                    icons::register_qa(ui, "+ Evento no cursor", add);
                    if response.hovered() {
                        ui.painter().rect_filled(add, 4.0, theme::BG_HEADER);
                    }
                    icons::paint(ui, add.shrink(5.0), Icon::Plus, theme::TEXT_MUTED);
                    if response.clicked() {
                        self.add_animation_event(clip);
                    }
                    let rect = self.timeline_lane(ui, clip.duration, 28.0);
                    for (index, event) in clip.events.iter_mut().enumerate() {
                        let x = rect.left() + rect.width() * event.time / clip.duration;
                        let selected =
                            self.studio.animation.selection == TimelineSelection::Event(index);
                        draw_flag(ui, Pos2::new(x, rect.center().y), &event.name, selected);
                        let response = ui
                            .interact(
                                Rect::from_center_size(
                                    Pos2::new(x + 4.0, rect.center().y),
                                    Vec2::new(18.0, 22.0),
                                ),
                                ui.id().with(("animation_event", index)),
                                if free {
                                    Sense::click_and_drag()
                                } else {
                                    Sense::hover()
                                },
                            )
                            .on_hover_text(format!("{} · {:.3} s", event.name, event.time));
                        if response.clicked() || response.drag_started() {
                            self.studio.animation.selection = TimelineSelection::Event(index);
                            self.studio.animation.time = event.time;
                            self.studio.playing = false;
                            self.studio.animation.preview = true;
                        }
                        if response.dragged_by(egui::PointerButton::Primary)
                            && let Some(pointer) = response.interact_pointer_pos()
                        {
                            event.time = timeline_time(pointer.x, rect, clip.duration);
                            self.studio.animation.time = event.time;
                        }
                    }
                });
                // One track per part of the model.
                let mut last = ruler.bottom();
                for entity in &entities {
                    let expanded = self.studio.animation.expanded.contains(&entity.id);
                    let depth = hierarchy_depth(self.scene(), &owner, &entity.id);
                    let row = ui
                        .horizontal(|ui| {
                            self.track_name(ui, entity, depth, expanded, name_width);
                            let rect = self.timeline_lane(ui, clip.duration, 28.0);
                            if self.selected.as_ref() == Some(&entity.id) {
                                ui.painter().rect_filled(rect, 0.0, theme::ACCENT_FILL);
                            }
                            self.key_lane(ui, clip, &entity.id, rect, "pose");
                            if let Some(time) = self.studio.animation.draft_time
                                && self.studio.animation.drafts.contains_key(&entity.id)
                            {
                                let x = rect.left() + rect.width() * time / clip.duration;
                                draw_diamond_outline(ui.painter(), Pos2::new(x, rect.center().y));
                            }
                            rect
                        })
                        .inner;
                    last = row.bottom();
                    if expanded {
                        for (channel, color) in [
                            ("Posição", Color32::from_rgb(135, 203, 223)),
                            ("Rotação", Color32::from_rgb(203, 158, 222)),
                            ("Escala", Color32::from_rgb(154, 207, 150)),
                        ] {
                            let row = ui
                                .horizontal(|ui| {
                                    let (name, _) = ui.allocate_exact_size(
                                        Vec2::new(name_width, 22.0),
                                        Sense::hover(),
                                    );
                                    let x = name.left() + 30.0 + track_indent(depth, name_width);
                                    ui.painter().rect_filled(
                                        Rect::from_center_size(
                                            Pos2::new(x, name.center().y),
                                            Vec2::splat(7.0),
                                        ),
                                        2.0,
                                        color,
                                    );
                                    ui.painter().text(
                                        Pos2::new(x + 10.0, name.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        channel,
                                        egui::FontId::proportional(12.0),
                                        theme::TEXT_MUTED,
                                    );
                                    let rect = self.timeline_lane(ui, clip.duration, 22.0);
                                    self.key_lane(ui, clip, &entity.id, rect, channel);
                                    rect
                                })
                                .inner;
                            last = row.bottom();
                        }
                    }
                }
                // Cursor across the ruler and every lane, with its time on the ruler.
                let x = ruler.left() + ruler.width() * self.studio.animation.time / clip.duration;
                let painter = ui.painter();
                painter.vline(
                    x,
                    ruler.top()..=last,
                    egui::Stroke::new(1.5, theme::ACCENT_BRIGHT),
                );
                let label =
                    Rect::from_center_size(Pos2::new(x, ruler.top() + 9.0), Vec2::new(40.0, 16.0));
                painter.rect_filled(label, 4.0, theme::ACCENT);
                painter.text(
                    label.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{:.2}", self.studio.animation.time),
                    egui::FontId::monospace(10.5),
                    Color32::WHITE,
                );
            });
        if !ui.input(|input| input.pointer.primary_down()) {
            for track in &mut clip.tracks {
                track.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
        }
    }
    /// Name cell of a track: expand, kind icon, name and a dot for a pending pose.
    fn track_name(
        &mut self,
        ui: &mut egui::Ui,
        entity: &Entity,
        depth: usize,
        expanded: bool,
        width: f32,
    ) {
        // The Hierarchy owns "Objeto: X" and "Expandir X": a track row reuses neither label.
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
        let selected = self.selected.as_ref() == Some(&entity.id);
        let row = rect.shrink2(Vec2::new(2.0, 2.0));
        if selected {
            ui.painter().rect_filled(row, 4.0, theme::ACCENT_SOFT);
        } else if response.hovered() {
            ui.painter().rect_filled(row, 4.0, theme::BG_RAISED);
        }
        let x = rect.left() + 6.0 + track_indent(depth, width);
        let chevron = Rect::from_min_size(Pos2::new(x, rect.center().y - 9.0), Vec2::splat(18.0));
        let toggle = ui.interact(
            chevron,
            ui.id().with(("animation_track_expand", &entity.id)),
            Sense::click(),
        );
        let label = if expanded {
            format!("Recolher trilha {}", entity.name)
        } else {
            format!("Expandir trilha {}", entity.name)
        };
        icons::register_qa(ui, &label, chevron);
        icons::paint(
            ui,
            chevron.shrink(4.0),
            if expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            },
            if toggle.hovered() {
                theme::TEXT
            } else {
                theme::TEXT_MUTED
            },
        );
        if toggle
            .on_hover_text("Mostra os canais de posição, rotação e escala.")
            .clicked()
        {
            if expanded {
                self.studio.animation.expanded.remove(&entity.id);
            } else {
                self.studio.animation.expanded.insert(entity.id.clone());
            }
        }
        icons::paint(
            ui,
            Rect::from_center_size(Pos2::new(x + 27.0, rect.center().y), Vec2::splat(14.0)),
            crate::app::entity_icon(entity),
            if selected {
                theme::ACCENT_BRIGHT
            } else {
                theme::TEXT_MUTED
            },
        );
        let pending = self.studio.animation.drafts.contains_key(&entity.id);
        let text_right = rect.right() - if pending { 18.0 } else { 6.0 };
        let text_rect = Rect::from_min_max(
            Pos2::new(x + 40.0, rect.top()),
            Pos2::new(text_right.max(x + 41.0), rect.bottom()),
        );
        ui.painter_at(text_rect).text(
            Pos2::new(text_rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            &entity.name,
            egui::FontId::proportional(12.5),
            if selected {
                Color32::WHITE
            } else {
                theme::TEXT
            },
        );
        if pending {
            ui.painter().circle_filled(
                Pos2::new(rect.right() - 10.0, rect.center().y),
                3.5,
                theme::HIGHLIGHT,
            );
        }
        if response.on_hover_text(&entity.name).clicked() {
            self.select(Some(entity.id.clone()));
            self.hierarchy_ui.focus = false;
            self.studio.animation.selection = TimelineSelection::None;
        }
    }
    fn timeline_lane(&mut self, ui: &mut egui::Ui, duration: f32, height: f32) -> Rect {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new((ui.available_width() - 12.0).max(10.0), height),
            Sense::click(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), 0.0, theme::BG_BASE);
        for tick in 0..=10 {
            let x = rect.left() + rect.width() * tick as f32 / 10.0;
            painter.line_segment(
                [
                    Pos2::new(x, rect.top() + 1.0),
                    Pos2::new(x, rect.bottom() - 1.0),
                ],
                egui::Stroke::new(1.0, Color32::from_rgb(46, 46, 50)),
            );
        }
        if response.clicked()
            && self.studio.animation.drafts.is_empty()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            self.studio.animation.time = timeline_time(pointer.x, rect, duration);
            self.studio.animation.preview = true;
            self.studio.playing = false;
        }
        rect
    }
    fn key_lane(
        &mut self,
        ui: &mut egui::Ui,
        clip: &mut Clip,
        target: &str,
        rect: Rect,
        channel: &str,
    ) {
        let Some(track) = clip.tracks.iter_mut().find(|track| track.target == target) else {
            return;
        };
        let channel_row = channel != "pose";
        let times: Vec<_> = track.keyframes.iter().map(|key| key.time).collect();
        for (index, key) in track.keyframes.iter_mut().enumerate() {
            let center = Pos2::new(
                rect.left() + rect.width() * key.time / clip.duration,
                rect.center().y,
            );
            let selected = matches!(&self.studio.animation.selection, TimelineSelection::Key { target: selected, time } if selected == target && (*time - key.time).abs() < EPSILON);
            draw_key(ui.painter(), center, selected, channel_row);
            let response = ui
                .interact(
                    Rect::from_center_size(center, Vec2::splat(16.0)),
                    ui.id().with(("animation_key", target, channel, index)),
                    if self.studio.animation.drafts.is_empty() {
                        Sense::click_and_drag()
                    } else {
                        Sense::hover()
                    },
                )
                .on_hover_text(format!("Quadro-chave · {:.3} s", key.time));
            if response.clicked() || response.drag_started() {
                self.hierarchy_ui.focus = false;
                self.studio.animation.selection = TimelineSelection::Key {
                    target: target.into(),
                    time: key.time,
                };
                self.studio.animation.time = key.time;
                self.studio.animation.preview = true;
                self.studio.playing = false;
                self.select(Some(target.into()));
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let next = timeline_time(pointer.x, rect, clip.duration);
                if times
                    .iter()
                    .enumerate()
                    .any(|(other, time)| other != index && (*time - next).abs() < EPSILON)
                {
                    self.studio.animation.message = "Já existe um quadro neste instante. Escolha outro instante; nenhum quadro foi removido.".into();
                } else {
                    key.time = next;
                }
                self.studio.animation.time = key.time;
                self.studio.animation.selection = TimelineSelection::Key {
                    target: target.into(),
                    time: key.time,
                };
            }
        }
    }
    /// The inspector while animating: the selected piece, its pose or key, and the clip.
    pub(crate) fn animation_properties_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties")
            .default_width((ctx.content_rect().width() * 0.23).clamp(140., 290.))
            .width_range(130.0..=(ctx.content_rect().width() * 0.32).clamp(140., 460.))
            .resizable(true)
            .show(ctx, |ui| {
                widgets::panel_header(ui, "Propriedades", |_| {});
                ui.add_space(8.);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.;
                    self.animation_piece_header(ui);
                    let Some(mut clip) = self.chosen_animation().cloned() else {
                        widgets::hint(
                            ui,
                            if self.studio.owner.is_some() {
                                "Este modelo ainda não tem animações. Crie uma na linha do tempo."
                            } else {
                                "Selecione uma raiz, grupo ou peça para começar."
                            },
                        );
                        return;
                    };
                    let old = clip.clone();
                    self.animation_properties(ui, &mut clip);
                    self.animation_clip_card(ui, &mut clip);
                    if clip != old {
                        self.store_animation(clip);
                    }
                });
            });
    }
    fn animation_piece_header(&mut self, ui: &mut egui::Ui) {
        let Some(entity) = self
            .selected
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .cloned()
        else {
            widgets::hint(ui, "Nenhuma peça selecionada.");
            return;
        };
        ui.horizontal(|ui| {
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(26.), Sense::hover());
            ui.painter().rect_filled(tile, 6., theme::ACCENT_SOFT);
            icons::paint(
                ui,
                Rect::from_center_size(tile.center(), Vec2::splat(15.)),
                crate::app::entity_icon(&entity),
                theme::ACCENT_BRIGHT,
            );
            ui.add(
                egui::Label::new(egui::RichText::new(&entity.name).font(theme::medium(13.5)))
                    .truncate(),
            );
        });
        let owner = self
            .studio
            .owner
            .as_deref()
            .and_then(|id| self.scene().entity(id))
            .map(|owner| owner.name.clone());
        let clip = self.chosen_animation().map(|clip| clip.name.clone());
        let line = match (owner, clip) {
            (Some(owner), Some(clip)) => format!("Modelo {owner} · animação {clip}"),
            (Some(owner), None) => format!("Modelo {owner}"),
            _ => String::new(),
        };
        if !line.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(34.);
                ui.add(
                    egui::Label::new(egui::RichText::new(line).size(12.).color(theme::TEXT_MUTED))
                        .truncate(),
                );
            });
        }
        ui.add_space(4.);
    }
    fn animation_properties(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        let selection = self.studio.animation.selection.clone();
        let mut delete = false;
        match selection {
            TimelineSelection::Key { target, time } => {
                let other_times: Vec<_> = clip
                    .tracks
                    .iter()
                    .find(|track| track.target == target)
                    .into_iter()
                    .flat_map(|track| &track.keyframes)
                    .filter(|key| (key.time - time).abs() >= EPSILON)
                    .map(|key| key.time)
                    .collect();
                let duration = clip.duration;
                let key = clip
                    .tracks
                    .iter_mut()
                    .find(|track| track.target == target)
                    .and_then(|track| {
                        track
                            .keyframes
                            .iter_mut()
                            .find(|key| (key.time - time).abs() < EPSILON)
                    });
                widgets::panel_card(
                    ui,
                    Icon::Keyframe,
                    "Quadro-chave",
                    &format!("{time:.2} s"),
                    true,
                    |ui| {
                        let Some(key) = key else {
                            widgets::hint(ui, "Quadro ausente.");
                            return;
                        };
                        let changed = widgets::property_row(
                            ui,
                            "Instante",
                            "Momento do quadro na animação.",
                            |ui| {
                                ui.add_sized(
                                    [ui.available_width(), theme::CONTROL_HEIGHT],
                                    egui::DragValue::new(&mut key.time)
                                        .range(0.0..=duration)
                                        .speed(1.0 / 60.0)
                                        .max_decimals(3)
                                        .suffix(" s"),
                                )
                                .changed()
                            },
                        );
                        if changed {
                            if other_times
                                .iter()
                                .any(|other| (*other - key.time).abs() < EPSILON)
                            {
                                key.time = time;
                                self.studio.animation.message = "Já existe um quadro neste instante. O quadro anterior foi preservado.".into();
                            }
                            self.studio.animation.selection = TimelineSelection::Key {
                                target: target.clone(),
                                time: key.time,
                            };
                            self.studio.animation.time = key.time;
                        }
                        edit_pose(ui, &mut key.transform);
                        widgets::property_row(
                            ui,
                            "Até o próximo",
                            "Linear interpola até o quadro seguinte; Manter segura esta pose até ele.",
                            |ui| {
                                widgets::segmented(
                                    ui,
                                    &mut key.interpolation,
                                    &[
                                        (Interpolation::Linear, "Linear"),
                                        (Interpolation::Hold, "Manter"),
                                    ],
                                );
                            },
                        );
                        ui.add_space(2.);
                        ui.horizontal(|ui| {
                            let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.)
                                .max(40.);
                            if ui
                                .add_sized([width, 26.], egui::Button::new("Copiar quadro"))
                                .clicked()
                            {
                                self.studio.animation.copied_key = Some(key.clone());
                            }
                            if ui
                                .add_sized(
                                    [width, 26.],
                                    egui::Button::new(
                                        egui::RichText::new("Excluir quadro").color(theme::ERROR),
                                    ),
                                )
                                .clicked()
                            {
                                delete = true;
                            }
                        });
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Voltar à pose provisória")
                                        .color(theme::ACCENT_BRIGHT),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.studio.animation.selection = TimelineSelection::None;
                        }
                        if !text_input_active(ui.ctx())
                            && ui.input_mut(|input| {
                                input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                            })
                        {
                            delete = true;
                        }
                    },
                );
                if delete {
                    if let Some(track) = clip.tracks.iter_mut().find(|track| track.target == target)
                    {
                        track
                            .keyframes
                            .retain(|key| (key.time - time).abs() > EPSILON);
                    }
                    self.studio.animation.selection = TimelineSelection::None;
                }
            }
            TimelineSelection::Event(index) => {
                let duration = clip.duration;
                let clip_id = clip.id.clone();
                let event = clip.events.get_mut(index);
                widgets::panel_card(ui, Icon::Flag, "Evento de animação", "", false, |ui| {
                    let Some(event) = event else {
                        widgets::hint(ui, "Evento ausente.");
                        return;
                    };
                    if self
                        .studio
                        .animation
                        .event_name
                        .as_ref()
                        .is_none_or(|(id, selected, _)| id != &clip_id || *selected != index)
                    {
                        self.studio.animation.event_name =
                            Some((clip_id.clone(), index, event.name.clone()));
                    }
                    widgets::property_row(
                        ui,
                        "Nome",
                        "Nome lido pelo nó Marcador de animação.",
                        |ui| {
                            let (_, _, name) = self.studio.animation.event_name.as_mut().unwrap();
                            ui.add(
                                egui::TextEdit::singleline(name)
                                    .desired_width(ui.available_width()),
                            );
                            if !name.trim().is_empty() {
                                event.name = name.trim().to_owned();
                            }
                        },
                    );
                    if self
                        .studio
                        .animation
                        .event_name
                        .as_ref()
                        .is_some_and(|(_, _, name)| name.trim().is_empty())
                    {
                        widgets::hint(
                            ui,
                            "Informe um nome; o nome anterior fica preservado até lá.",
                        );
                    }
                    let moved = widgets::property_row(ui, "Instante", "", |ui| {
                        ui.add_sized(
                            [ui.available_width(), theme::CONTROL_HEIGHT],
                            egui::DragValue::new(&mut event.time)
                                .range(0.0..=duration)
                                .speed(1.0 / 60.0)
                                .max_decimals(3)
                                .suffix(" s"),
                        )
                        .changed()
                    });
                    if moved {
                        self.studio.animation.time = event.time;
                    }
                    widgets::hint(
                        ui,
                        "O jogo emite este evento ao passar pelo instante. Use Marcador de animação na Lógica para reagir a ele.",
                    );
                    ui.horizontal(|ui| {
                        let width =
                            ((ui.available_width() - ui.spacing().item_spacing.x) / 2.).max(40.);
                        if ui
                            .add_sized([width, 26.], egui::Button::new("Copiar evento"))
                            .clicked()
                        {
                            self.studio.animation.copied_event = Some(event.clone());
                        }
                        if ui
                            .add_sized(
                                [width, 26.],
                                egui::Button::new(
                                    egui::RichText::new("Excluir evento").color(theme::ERROR),
                                ),
                            )
                            .clicked()
                        {
                            delete = true;
                        }
                    });
                    if !text_input_active(ui.ctx())
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        })
                    {
                        delete = true;
                    }
                });
                if delete && index < clip.events.len() {
                    clip.events.remove(index);
                    self.studio.animation.selection = TimelineSelection::None;
                }
            }
            TimelineSelection::None => {
                let pending = !self.studio.animation.drafts.is_empty();
                let trailing = format!("{:.2} s", self.studio.animation.time);
                widgets::panel_card(
                    ui,
                    Icon::Person,
                    "Pose provisória",
                    &trailing,
                    pending,
                    |ui| {
                        widgets::hint(
                            ui,
                            "Ajuste a peça na cena ou aqui e grave com + Quadro-chave. A pose-base do modelo fica preservada.",
                        );
                        if let Some(id) = self.selected.clone()
                            && let Some(mut transform) = self.animation_draft_transform(&id)
                        {
                            let before = transform.clone();
                            edit_pose(ui, &mut transform);
                            if transform != before {
                                self.set_animation_draft_transform(&id, transform);
                            }
                            if self.studio.animation.copied_key.is_some() {
                                self.animation_paste_key_button(ui, clip, Some(&id));
                            }
                        } else {
                            widgets::hint(ui, "Selecione uma peça, raiz ou grupo.");
                        }
                    },
                );
            }
        }
    }
    /// Name, length and repetition of the chosen animation.
    fn animation_clip_card(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        let end = clip
            .tracks
            .iter()
            .flat_map(|track| track.keyframes.iter().map(|key| key.time))
            .chain(clip.events.iter().map(|event| event.time))
            .fold(
                self.studio.animation.draft_time.unwrap_or(0.1).max(0.1),
                f32::max,
            );
        widgets::open_card(ui, "animation_clip", "Animação", |ui| {
            widgets::property_row(ui, "Nome", "Enter confirma · Escape cancela.", |ui| {
                let editing = self
                    .studio
                    .animation
                    .rename
                    .as_ref()
                    .filter(|rename| rename.id == clip.id)
                    .map(|rename| (rename.text.clone(), rename.focus));
                let (mut text, focus) = editing.unwrap_or_else(|| (clip.name.clone(), false));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id_salt("animation_name")
                        .desired_width(ui.available_width()),
                );
                if focus {
                    response.request_focus();
                }
                if response.changed() || focus {
                    self.studio.animation.rename = Some(Rename {
                        id: clip.id.clone(),
                        text: text.clone(),
                        focus: false,
                    });
                }
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) && response.lost_focus() {
                    self.studio.animation.rename = None;
                } else if response.lost_focus()
                    && let Some(rename) = self.studio.animation.rename.take()
                {
                    let name = rename.text.trim();
                    if rename.id == clip.id && !name.is_empty() {
                        clip.name = name.to_owned();
                    } else if rename.id == clip.id {
                        self.studio.animation.message =
                            "Informe o nome da animação ou pressione Escape para cancelar a renomeação."
                                .into();
                    }
                }
            });
            widgets::property_row(
                ui,
                "Duração",
                "A duração inclui o último quadro e evento. Mova-os antes de encurtar a animação.",
                |ui| {
                    ui.add_sized(
                        [ui.available_width(), theme::CONTROL_HEIGHT],
                        egui::DragValue::new(&mut clip.duration)
                            .range(end..=3600.0)
                            .speed(0.05)
                            .max_decimals(2)
                            .suffix(" s"),
                    );
                },
            );
            widgets::toggle_row(
                ui,
                "Repetir",
                "Ao chegar ao fim, volta ao início e emite novamente os eventos atravessados.",
                &mut clip.looping,
            );
            widgets::hint(
                ui,
                if clip.looping {
                    "Volta ao início e repete os eventos."
                } else {
                    "Para no último instante."
                },
            );
        });
    }
    fn animation_reference_dialog(&mut self, ctx: &egui::Context) {
        if self.studio.animation.references.is_empty() {
            return;
        }
        egui::Window::new("Animação em uso").collapsible(false).resizable(true).default_width(560.0).show(ctx, |ui| {
            ui.label("A exclusão foi bloqueada para preservar os comportamentos abaixo. Selecione outra animação nesses nós antes de excluir.");
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| { for reference in &self.studio.animation.references { ui.label(reference); } });
            if ui.button("Entendi").clicked() { self.studio.animation.references.clear(); }
        });
    }
}

fn edit_pose(ui: &mut egui::Ui, transform: &mut Transform) {
    widgets::vec3_row(
        ui,
        "Posição",
        "Distância da origem nos eixos X, Y e Z.",
        &mut transform.position,
        0.02,
        false,
    );
    let mut degrees = transform.rotation.map(f32::to_degrees);
    let previous = degrees;
    widgets::vec3_row(
        ui,
        "Rotação",
        "Giro em graus nos eixos da peça.",
        &mut degrees,
        0.5,
        false,
    );
    if degrees != previous {
        transform.rotation = degrees.map(f32::to_radians);
    }
    widgets::vec3_row(
        ui,
        "Escala",
        "Multiplica o tamanho sem alterar a forma original.",
        &mut transform.scale,
        0.02,
        true,
    );
}
fn hierarchy_depth(scene: &Scene, root: &str, target: &str) -> usize {
    let mut depth = 0;
    let mut current = target;
    while current != root && depth < scene.entities.len() {
        let Some(parent) = scene
            .entity(current)
            .and_then(|entity| entity.parent.as_deref())
        else {
            break;
        };
        current = parent;
        depth += 1;
    }
    depth
}
/// Indent of a track by depth, bounded so the name keeps room in a narrow column.
fn track_indent(depth: usize, width: f32) -> f32 {
    (depth as f32 * 14.0).min((width - 100.0).clamp(14.0, 56.0))
}
fn timeline_time(x: f32, rect: Rect, duration: f32) -> f32 {
    (((x - rect.left()) / rect.width().max(1.0) * duration * 60.0).round() / 60.0)
        .clamp(0.0, duration)
}
fn diamond(center: Pos2, radius: f32) -> Vec<Pos2> {
    vec![
        center + Vec2::new(0.0, -radius),
        center + Vec2::new(radius, 0.0),
        center + Vec2::new(0.0, radius),
        center + Vec2::new(-radius, 0.0),
    ]
}
/// Keyframe marker; the selected one is white with an accent ring.
fn draw_key(painter: &egui::Painter, center: Pos2, selected: bool, channel: bool) {
    let (radius, fill, stroke) = match (selected, channel) {
        (true, _) => (6.0, Color32::WHITE, egui::Stroke::new(2.0, theme::ACCENT)),
        (false, false) => (5.0, theme::ACCENT_BRIGHT, egui::Stroke::NONE),
        (false, true) => (3.5, Color32::from_rgb(94, 127, 166), egui::Stroke::NONE),
    };
    painter.add(egui::Shape::convex_polygon(
        diamond(center, radius),
        fill,
        stroke,
    ));
}
/// Pose waiting to be recorded: an outlined key at the pending time.
fn draw_diamond_outline(painter: &egui::Painter, center: Pos2) {
    painter.add(egui::Shape::convex_polygon(
        diamond(center, 6.0),
        theme::HIGHLIGHT.gamma_multiply(0.18),
        egui::Stroke::new(1.5, theme::HIGHLIGHT),
    ));
}
/// Event marker: a pennant on a pole with its name beside it.
fn draw_flag(ui: &egui::Ui, at: Pos2, name: &str, selected: bool) {
    let color = if selected {
        Color32::WHITE
    } else {
        theme::HIGHLIGHT
    };
    let painter = ui.painter();
    painter.line_segment(
        [at + Vec2::new(0.0, -10.0), at + Vec2::new(0.0, 10.0)],
        egui::Stroke::new(1.4, color),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            at + Vec2::new(0.0, -10.0),
            at + Vec2::new(10.0, -5.5),
            at + Vec2::new(0.0, -1.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
    painter.text(
        at + Vec2::new(13.0, -5.5),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(11.5),
        if selected {
            Color32::WHITE
        } else {
            Color32::from_rgb(232, 210, 160)
        },
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn collective_drafts_are_time_bound_and_one_reversible_delta() {
        use super::*;
        use oxy_core::{edit_history::CommandHistory, texture_cache::TextureCache};
        let mut project = Project::new("Animação segura");
        let root = Entity::new("Grupo", None);
        let mut child = Entity::new("Peça", Some(Primitive::Rectangle));
        child.parent = Some(root.id.clone());
        let ids = vec![root.id.clone(), child.id.clone()];
        project.scenes[0].entities.extend([root, child]);
        let mut clip = Clip::new("Ataque");
        let id = clip.id.clone();
        project.scenes[0].entities[0].clips.push(clip.clone());
        let base = project.clone();
        let mut state = AnimationState {
            time: 0.5,
            draft_time: Some(0.5),
            ..Default::default()
        };
        for target in &ids {
            let mut t = Transform::default();
            t.rotation[2] = 0.75;
            state.drafts.insert(target.clone(), t);
        }
        state.time = 0.8;
        assert!(record_drafts(&mut state, &mut clip, &ids).is_err());
        assert!(clip.tracks.is_empty());
        assert_eq!(state.drafts.len(), 2);
        state.time = 0.5;
        let mut history = CommandHistory::new();
        let mut images = TextureCache::default();
        history.begin("Gravar poses", &project, &images);
        assert_eq!(record_drafts(&mut state, &mut clip, &ids).unwrap(), 2);
        assert!(state.drafts.is_empty());
        assert_eq!(clip.id, id);
        project.scenes[0].entities[0].clips[0] = clip;
        history.commit(&project, &mut images).unwrap();
        let recorded = project.clone();
        assert_eq!(history.undo_len(), 1);
        history.undo(&mut project, &mut images).unwrap();
        assert_eq!(project, base);
        history.redo(&mut project, &mut images).unwrap();
        assert_eq!(project, recorded);
    }
    use super::*;
    use glam::Vec3;
    use oxy_core::graph::Node;

    fn rig() -> (Scene, Id, Id, Id) {
        let mut scene = Scene::new("Oficina", SceneKind::ThreeD);
        let mut root = Entity::new("Modelo", None);
        root.clips.push(Clip::new("Ataque"));
        let mut joint = Entity::new("Articulação", None);
        joint.parent = Some(root.id.clone());
        let mut piece = Entity::new("Lâmina", Some(Primitive::Cube));
        piece.parent = Some(joint.id.clone());
        piece.transform.position = [0.0, 2.0, 0.0];
        let ids = (root.id.clone(), joint.id.clone(), piece.id.clone());
        scene.entities = vec![root, joint, piece];
        (scene, ids.0, ids.1, ids.2)
    }

    #[test]
    fn automatic_model_resolves_roots_groups_and_animated_ancestors_without_names() {
        let (scene, root, joint, piece) = rig();
        assert_eq!(automatic_model(&scene, Some(&root)), Some(root.clone()));
        assert_eq!(automatic_model(&scene, Some(&joint)), Some(joint));
        assert_eq!(automatic_model(&scene, Some(&piece)), Some(root));
        assert_eq!(automatic_model(&scene, Some("missing")), None);
    }

    #[test]
    fn referenced_animation_deletion_lists_scene_and_asset_uses_and_preserves_document() {
        let mut project = Project::new("Proteção de referências");
        let mut owner = Entity::new("Modelo animado", None);
        let clip = Clip::new("Ataque");
        let id = clip.id.clone();
        owner.clips.push(clip);
        let mut node = Node::new("action.animation", [0.0; 2]);
        node.params.insert("clip".into(), Value::Text(id.clone()));
        owner.graph.nodes.push(node);
        let owner_id = owner.id.clone();
        project.scenes[0].entities.push(owner.clone());
        let scene_id = project.scenes[0].id.clone();
        let mut next = Scene::new("Outra cena", SceneKind::TwoD);
        let mut other = Entity::new("Outra referência", None);
        other.graph = owner.graph.clone();
        next.entities.push(other);
        project.scenes.push(next);
        project.assets.push(Asset {
            id: new_id(),
            name: "Modelo guardado".into(),
            path: String::new(),
            kind: AssetKind::Model,
            model: Some(vec![owner]),
        });
        let before = project.clone();
        let errors = remove_animation(&mut project, &scene_id, &owner_id, &id).unwrap_err();
        assert_eq!(errors.len(), 3);
        assert!(errors.iter().any(|error| error.contains("Outra cena")));
        assert!(errors.iter().any(|error| error.contains("Modelo guardado")));
        assert_eq!(project, before);
        for scene in &mut project.scenes {
            for entity in &mut scene.entities {
                entity.graph.nodes.clear();
            }
        }
        project.assets[0].model.as_mut().unwrap()[0]
            .graph
            .nodes
            .clear();
        remove_animation(&mut project, &scene_id, &owner_id, &id).unwrap();
        assert!(
            project.scenes[0]
                .entity(&owner_id)
                .unwrap()
                .clips
                .is_empty()
        );
    }

    #[test]
    fn duplication_and_rename_keep_targets_events_and_original_references_stable() {
        let (mut scene, root, _, piece) = rig();
        let pose = scene.entity(&piece).unwrap().transform.clone();
        let original = &mut scene.entity_mut(&root).unwrap().clips[0];
        original.insert_key(
            &piece,
            Keyframe {
                time: 0.5,
                transform: pose,
                interpolation: Interpolation::Hold,
            },
        );
        original.events.push(AnimationEvent {
            time: 0.5,
            name: "Impacto".into(),
        });
        let original = original.clone();
        let mut duplicate =
            duplicate_animation(std::slice::from_ref(&original), &original.id).unwrap();
        assert_ne!(duplicate.id, original.id);
        assert_eq!(duplicate.tracks, original.tracks);
        assert_eq!(duplicate.events, original.events);
        duplicate.name = "Novo nome legível".into();
        assert_eq!(duplicate.tracks[0].target, piece);
        assert_eq!(scene.entity(&root).unwrap().clips[0], original);
    }

    #[test]
    fn draft_group_pose_moves_descendants_and_recording_never_overwrites_model_base() {
        let (mut scene, root, joint, piece) = rig();
        let baseline = scene.clone();
        let mut state = AnimationState {
            preview: true,
            time: 0.5,
            ..Default::default()
        };
        let mut transform = scene.entity(&joint).unwrap().transform.clone();
        transform.rotation[2] = std::f32::consts::FRAC_PI_2;
        state.drafts.insert(joint.clone(), transform.clone());
        let mut clip = scene.entity(&root).unwrap().clips[0].clone();
        let preview = preview_scene(&scene, Some(&clip), &state);
        assert!(
            preview
                .world_matrix(&piece)
                .unwrap()
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(-2.0, 0.0, 0.0), 1e-5)
        );
        assert_eq!(scene, baseline);
        clip.insert_key(
            &joint,
            Keyframe {
                time: state.time,
                transform: preview.entity(&joint).unwrap().transform.clone(),
                interpolation: Interpolation::Linear,
            },
        );
        state.drafts.clear();
        scene.entity_mut(&root).unwrap().clips[0] = clip.clone();
        let reopened: Scene =
            serde_json::from_str(&serde_json::to_string(&scene).unwrap()).unwrap();
        assert_eq!(
            reopened.entity(&joint).unwrap().transform,
            baseline.entity(&joint).unwrap().transform
        );
        assert_eq!(
            reopened.entity(&piece).unwrap().transform,
            baseline.entity(&piece).unwrap().transform
        );
        let recorded_preview = preview_scene(&reopened, Some(&clip), &state);
        assert!(
            recorded_preview
                .world_matrix(&piece)
                .unwrap()
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(-2.0, 0.0, 0.0), 1e-5)
        );
        clip.validate(
            &reopened
                .entities
                .iter()
                .map(|entity| entity.id.as_str())
                .collect(),
        )
        .unwrap();
    }
}
