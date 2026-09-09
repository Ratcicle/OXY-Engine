//! Animation authoring. Poses under construction never enter the project document.
use crate::{
    app::{Editor, vector3},
    graph_ui::text_input_active,
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
    properties_open: bool,
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
            properties_open: false,
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
        self.hierarchy_focus = false;
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
    fn animation_list(&mut self, ui: &mut egui::Ui) {
        ui.strong("ANIMAÇÕES");
        let Some(model) = self.studio.owner.clone() else {
            ui.label("Selecione uma raiz, grupo ou peça para começar.");
            return;
        };
        let clips = self
            .scene()
            .entity(&model)
            .map(|entity| entity.clips.clone())
            .unwrap_or_default();
        if ui
            .add_enabled(
                self.studio.animation.drafts.is_empty(),
                egui::Button::new("+ Nova animação"),
            )
            .clicked()
        {
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
        ui.add_space(6.0);
        let mut delete = None;
        let mut duplicate = None;
        let mut rename = None;
        let mut rename_commit = None;
        let mut rename_cancel = false;
        egui::ScrollArea::vertical()
            .id_salt("animation_names")
            .show(ui, |ui| {
                for clip in &clips {
                    ui.push_id(&clip.id, |ui| {
                        if self
                            .studio
                            .animation
                            .rename
                            .as_ref()
                            .is_some_and(|rename| rename.id == clip.id)
                        {
                            let edit = self.studio.animation.rename.as_mut().unwrap();
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut edit.text)
                                    .desired_width(ui.available_width()),
                            );
                            if edit.focus {
                                response.request_focus();
                                edit.focus = false;
                            }
                            if ui.input(|input| input.key_pressed(egui::Key::Enter))
                                && !edit.text.trim().is_empty()
                            {
                                rename_commit =
                                    Some((clip.id.clone(), edit.text.trim().to_owned()));
                            }
                            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                                rename_cancel = true;
                            }
                            ui.small("Enter confirma · Escape cancela");
                        } else {
                            let response = ui.add_sized(
                                [ui.available_width(), 30.0],
                                egui::Button::selectable(
                                    self.studio.animation.selected.as_ref() == Some(&clip.id),
                                    &clip.name,
                                ),
                            );
                            if response.clicked() {
                                self.choose_animation(&clip.id);
                            }
                            if response.double_clicked() {
                                rename = Some((clip.id.clone(), clip.name.clone()));
                            }
                            response.context_menu(|ui| {
                                if ui.button("Renomear · F2").clicked() {
                                    rename = Some((clip.id.clone(), clip.name.clone()));
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        self.studio.animation.drafts.is_empty(),
                                        egui::Button::new("Duplicar animação"),
                                    )
                                    .on_hover_text(
                                        "Grave ou descarte a pose provisória antes de duplicar.",
                                    )
                                    .clicked()
                                {
                                    duplicate = Some(clip.id.clone());
                                    ui.close();
                                }
                                if ui.button("Excluir animação").clicked() {
                                    delete = Some(clip.id.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
                }
                if clips.is_empty() {
                    ui.small("Crie uma animação e grave a primeira pose com + Quadro-chave.");
                }
            });
        if let Some((id, name)) = rename_commit {
            if let Some(clip) = self
                .scene_mut()
                .entity_mut(&model)
                .and_then(|entity| entity.clips.iter_mut().find(|clip| clip.id == id))
            {
                clip.name = name;
            }
            self.studio.animation.rename = None;
        }
        if rename_cancel {
            self.studio.animation.rename = None;
        }
        if let Some((id, text)) = rename {
            self.studio.animation.rename = Some(Rename {
                id,
                text,
                focus: true,
            });
        }
        if let Some(id) = duplicate
            && let Some(copy) = duplicate_animation(&clips, &id)
        {
            let id = copy.id.clone();
            if let Some(entity) = self.scene_mut().entity_mut(&model) {
                entity.clips.push(copy);
            }
            self.choose_animation(&id);
        }
        if let Some(id) = delete {
            self.delete_selected_animation(&id);
        }
        if !self.hierarchy_focus
            && !text_input_active(ui.ctx())
            && ui.input(|input| input.key_pressed(egui::Key::F2))
            && let Some(clip) = self.chosen_animation()
        {
            self.studio.animation.rename = Some(Rename {
                id: clip.id.clone(),
                text: clip.name.clone(),
                focus: true,
            });
        }
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
    /// Model context lives in the first Studio row; timeline and clip data stay unchanged.
    pub(super) fn animation_context_controls(&mut self, ui: &mut egui::Ui, compact: bool) {
        if !self.spatial.base_pose {
            self.sync_animation_model();
        }
        crate::icons::menu_button(
            ui,
            crate::icons::Icon::Object,
            "Modelo",
            "Modelo animável, pose-base e propriedades da animação.",
            true,
            |ui| {
                ui.label(egui::RichText::new("Modelo:").small().weak());
                let current = self
                    .studio
                    .owner
                    .as_deref()
                    .and_then(|id| self.scene().entity(id))
                    .map(|entity| entity.name.clone())
                    .unwrap_or_else(|| "Selecione uma peça".into());
                let mut selected = self.studio.animation.manual_model.clone();
                egui::ComboBox::from_id_salt("animation_model")
                    .selected_text(current)
                    .width(210.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, None, "Automático pela seleção");
                        for entity in &self.scene().entities {
                            ui.selectable_value(
                                &mut selected,
                                Some(entity.id.clone()),
                                &entity.name,
                            );
                        }
                    });
                if selected != self.studio.animation.manual_model {
                    if self.studio.animation.drafts.is_empty() {
                        self.studio.animation.manual_model = selected;
                        self.sync_animation_model();
                    } else {
                        self.studio.animation.message =
                            "Grave ou descarte a pose provisória antes de trocar o modelo.".into();
                    }
                }
                if ui
                    .add_enabled(
                        self.studio.animation.drafts.is_empty(),
                        egui::Button::new("Pose-base"),
                    )
                    .on_hover_text(
                        "Mostra a pose original. Grave ou descarte a pose provisória primeiro.",
                    )
                    .clicked()
                {
                    self.studio.animation.preview = false;
                    self.studio.playing = false;
                }
                if compact
                    && ui
                        .add_enabled(
                            self.chosen_animation().is_some(),
                            egui::Button::new("Propriedades"),
                        )
                        .on_hover_text("Editar a pose provisória ou o quadro/evento selecionado.")
                        .clicked()
                {
                    self.studio.animation.properties_open = true;
                }
                if !self.studio.animation.drafts.is_empty() {
                    ui.colored_label(
                        Color32::GOLD,
                        format!(
                            "{} pose(s) provisória(s)",
                            self.studio.animation.drafts.len()
                        ),
                    );
                    if ui.button("Descartar pose provisória").clicked() {
                        self.studio.animation.drafts.clear();
                        self.studio.animation.draft_time = None;
                        self.studio.animation.message.clear();
                    }
                }
            },
        );
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
        egui::SidePanel::left(if compact {
            "animation_list_compact"
        } else {
            "animation_list"
        })
        .resizable(!compact)
        .default_width(if compact { 160.0 } else { 210.0 })
        .width_range(160.0..=if compact { 160.0 } else { 320.0 })
        .show_inside(ui, |ui| self.animation_list(ui));
        let Some(mut clip) = self.chosen_animation().cloned() else {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(ui, |ui| {
                    self.viewport(ui, false);
                });
            self.animation_reference_dialog(ui.ctx());
            return;
        };
        let old_clip = clip.clone();
        if self.studio.playing && self.studio.animation.drafts.is_empty() {
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
        // Keep space for the shared viewport toolbar and a useful drawing area.
        let timeline_max = if Self::compact_layout(ui.ctx()) {
            (ui.available_height() * 0.35).clamp(80., 120.)
        } else if compact {
            (ui.available_height() * 0.45)
                .max(180.0)
                .min((ui.available_height() - 155.0).max(120.0))
        } else {
            600.0
        };
        egui::TopBottomPanel::bottom(if compact {
            "animation_timeline_compact"
        } else {
            "animation_timeline"
        })
        .resizable(true)
        .default_height(if compact { timeline_max } else { 285.0 })
        .height_range(180.0_f32.min(timeline_max)..=timeline_max)
        .show_inside(ui, |ui| {
            self.animation_toolbar(ui, &mut clip, compact);
            self.animation_timeline(ui, &mut clip, compact);
        });
        if compact {
            let mut open = self.studio.animation.properties_open;
            egui::Window::new("Propriedades da animação")
                .open(&mut open)
                .default_width(285.0)
                .max_width((ui.ctx().content_rect().width() - 40.0).max(210.0))
                .max_height((ui.ctx().content_rect().height() - 90.0).max(180.0))
                .vscroll(true)
                .show(ui.ctx(), |ui| self.animation_properties(ui, &mut clip));
            self.studio.animation.properties_open = open;
        } else {
            egui::SidePanel::right("animation_selection_properties")
                .resizable(true)
                .default_width(255.0)
                .width_range(210.0..=370.0)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.animation_properties(ui, &mut clip));
                });
        }
        if !ui.input(|input| input.pointer.primary_down()) {
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
        if clip != old_clip
            && let Some(owner) = self.studio.owner.clone()
            && let Some(existing) = self.scene_mut().entity_mut(&owner).and_then(|entity| {
                entity
                    .clips
                    .iter_mut()
                    .find(|existing| existing.id == clip.id)
            })
        {
            *existing = clip;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| self.viewport(ui, false));
        self.animation_reference_dialog(ui.ctx());
    }
    fn animation_toolbar(&mut self, ui: &mut egui::Ui, clip: &mut Clip, compact: bool) {
        let scope = self
            .studio
            .owner
            .as_deref()
            .map(|id| self.scene().descendants(id))
            .unwrap_or_default();
        let selected = self.selected.clone().filter(|id| scope.contains(id));
        if Self::compact_layout(ui.ctx()) {
            // Keep a full, scrollable strip of tracks below this one-line toolbar.
            // The menu retains every action without making the viewport progressively smaller.
            ui.horizontal(|ui| {
                ui.menu_button("Controles", |ui| {
                    ui.set_width(300.0_f32.min(ui.ctx().content_rect().width() - 40.0));
                    egui::ScrollArea::vertical()
                        .max_height((ui.ctx().content_rect().height() * 0.7).max(120.0))
                        .show(ui, |ui| {
                            ui.strong("LINHA DO TEMPO");
                            ui.label(&clip.name);
                            ui.horizontal_wrapped(|ui| self.animation_duration_controls(ui, clip));
                            ui.horizontal_wrapped(|ui| {
                                self.animation_playback_controls(ui, clip, false)
                            });
                            ui.separator();
                            ui.horizontal_wrapped(|ui| {
                                self.animation_record_all_button(ui, clip, &scope);
                                self.animation_paste_key_button(ui, clip, selected.as_deref());
                            });
                            ui.separator();
                            ui.horizontal_wrapped(|ui| self.animation_event_controls(ui, clip));
                        });
                })
                .response
                .on_hover_text("Reprodução, duração, repetição, poses alteradas e eventos.");
                self.animation_cursor_control(ui, clip);
                self.animation_record_key_button(ui, clip, selected.as_deref());
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong("LINHA DO TEMPO");
            ui.separator();
            if !compact {
                ui.label(&clip.name);
            }
            self.animation_duration_controls(ui, clip);
        });
        ui.horizontal_wrapped(|ui| {
            self.animation_playback_controls(ui, clip, compact);
            self.animation_cursor_control(ui, clip);
            self.animation_record_all_button(ui, clip, &scope);
            self.animation_record_key_button(ui, clip, selected.as_deref());
            self.animation_paste_key_button(ui, clip, selected.as_deref());
        });
    }
    fn animation_duration_controls(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        let end = clip
            .tracks
            .iter()
            .flat_map(|track| track.keyframes.iter().map(|key| key.time))
            .chain(clip.events.iter().map(|event| event.time))
            .fold(
                self.studio.animation.draft_time.unwrap_or(0.1).max(0.1),
                f32::max,
            );
        ui.label("Duração");
        ui.add(
            egui::DragValue::new(&mut clip.duration)
                .range(end..=3600.0)
                .speed(0.05)
                .suffix(" s"),
        )
        .on_hover_text(
            "A duração inclui o último quadro e evento. Mova-os antes de encurtar a animação.",
        );
        ui.checkbox(&mut clip.looping, "Repetir").on_hover_text(
            "Ao chegar ao fim, volta ao início e emite novamente os eventos atravessados.",
        );
    }
    fn animation_playback_controls(&mut self, ui: &mut egui::Ui, clip: &mut Clip, compact: bool) {
        if ui
            .add_enabled(
                self.studio.animation.drafts.is_empty(),
                egui::Button::new(if self.studio.playing {
                    "Ⅱ Pausar"
                } else {
                    "▶ Reproduzir"
                }),
            )
            .on_hover_text("Grave ou descarte poses provisórias antes de reproduzir.")
            .clicked()
        {
            self.studio.playing = !self.studio.playing;
            self.studio.animation.preview = true;
            if self.studio.animation.time >= clip.duration {
                self.studio.animation.time = 0.0;
            }
        }
        if ui
            .add_enabled(
                self.studio.animation.drafts.is_empty(),
                egui::Button::new(if compact { "■" } else { "■ Início" }),
            )
            .on_hover_text("Grave ou descarte as poses alteradas antes de mudar o cursor.")
            .clicked()
        {
            self.studio.playing = false;
            self.studio.animation.time = 0.0;
            self.studio.animation.preview = true;
        }
    }
    fn animation_cursor_control(&mut self, ui: &mut egui::Ui, clip: &Clip) {
        ui.label("Cursor");
        if ui
            .add_enabled(
                self.studio.animation.drafts.is_empty(),
                egui::DragValue::new(&mut self.studio.animation.time)
                    .range(0.0..=clip.duration)
                    .speed(1.0 / 60.0)
                    .suffix(" s"),
            )
            .on_hover_text(
                "O cursor fica preso ao tempo da pose provisória até gravar ou descartar.",
            )
            .changed()
        {
            self.studio.playing = false;
            self.studio.animation.preview = true;
        }
    }
    fn animation_record_all_button(&mut self, ui: &mut egui::Ui, clip: &mut Clip, scope: &[Id]) {
        if ui.add_enabled(!self.studio.animation.drafts.is_empty(),egui::Button::new("Gravar poses alteradas")).on_hover_text("Grava todas as peças e grupos alterados no tempo da pose provisória, em uma operação de desfazer.").clicked() && let Err(e)=record_drafts(&mut self.studio.animation,clip,scope){self.studio.animation.message=e;}
    }
    fn animation_record_key_button(
        &mut self,
        ui: &mut egui::Ui,
        clip: &mut Clip,
        selected: Option<&str>,
    ) {
        if ui.add_enabled(selected.is_some(), egui::Button::new("+ Quadro-chave")).on_hover_text("Grava posição, rotação e escala da peça selecionada no cursor, incluindo a pose provisória.").clicked()
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
        }
    }
    fn animation_event_controls(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        ui.label("EVENTOS").on_hover_text("Grupos também podem ser articulações. Cada quadro reúne posição, rotação e escala; expandir mostra os três canais.");
        ui.add(
            egui::TextEdit::singleline(&mut self.studio.animation.marker_name)
                .desired_width(110.0)
                .hint_text("Nome do evento"),
        );
        if ui
            .add_enabled(
                !self.studio.animation.marker_name.trim().is_empty(),
                egui::Button::new("+ Evento no cursor"),
            )
            .clicked()
        {
            clip.events.push(AnimationEvent {
                time: self.studio.animation.time,
                name: self.studio.animation.marker_name.trim().into(),
            });
            self.studio.animation.selection = TimelineSelection::Event(clip.events.len() - 1);
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
            self.studio.animation.selection = TimelineSelection::Event(clip.events.len() - 1);
        }
    }
    fn animation_timeline(&mut self, ui: &mut egui::Ui, clip: &mut Clip, compact: bool) {
        if !Self::compact_layout(ui.ctx()) {
            ui.horizontal_wrapped(|ui| self.animation_event_controls(ui, clip));
        }
        if !compact {
            ui.small("Grupos também podem ser articulações. Cada quadro reúne posição, rotação e escala; expandir mostra os três canais.");
        }
        let Some(owner) = self.studio.owner.clone() else {
            return;
        };
        let scope = self.scene().descendants(&owner);
        let ids: Vec<_> = oxy_core::editing::hierarchy_order(self.scene())
            .into_iter()
            .filter(|id| scope.contains(id))
            .collect();
        let entities: Vec<_> = ids
            .iter()
            .filter_map(|id| self.scene().entity(id).cloned())
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("animation_tracks")
            // The default 64-point minimum can exceed the remaining compact timeline.
            // Let the scroll viewport use its actual height so its last row is reachable.
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized([160.0, 24.0], egui::Label::new("Tempo (s)"));
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width().max(10.0), 24.0),
                        Sense::hover(),
                    );
                    for tick in 0..=10 {
                        let time = clip.duration * tick as f32 / 10.0;
                        let x = rect.left() + rect.width() * tick as f32 / 10.0;
                        ui.painter().text(
                            Pos2::new(x, rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            format!("{time:.2}"),
                            egui::FontId::proportional(11.0),
                            Color32::GRAY,
                        );
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [160.0, 28.0],
                        egui::Label::new(egui::RichText::new("EVENTOS").color(Color32::GOLD)),
                    );
                    let rect = self.timeline_lane(ui, clip.duration, 30.0);
                    for (index, event) in clip.events.iter_mut().enumerate() {
                        let x = rect.left() + rect.width() * event.time / clip.duration;
                        let center = Pos2::new(x, rect.center().y);
                        let selected =
                            self.studio.animation.selection == TimelineSelection::Event(index);
                        draw_diamond(
                            ui.painter(),
                            center,
                            if selected { 7.0 } else { 5.0 },
                            if selected {
                                Color32::WHITE
                            } else {
                                Color32::GOLD
                            },
                        );
                        let response = ui
                            .interact(
                                Rect::from_center_size(center, Vec2::splat(17.0)),
                                ui.id().with(("animation_event", index)),
                                if self.studio.animation.drafts.is_empty() {
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
                for entity in &entities {
                    let expanded = self.studio.animation.expanded.contains(&entity.id);
                    ui.horizontal(|ui| {
                        let depth = hierarchy_depth(self.scene(), &owner, &entity.id);
                        ui.allocate_ui_with_layout(
                            Vec2::new(160.0, 28.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add_space((depth as f32 * 10.0).min(40.0));
                                if ui.small_button(if expanded { "▾" } else { "▸" }).clicked() {
                                    if expanded {
                                        self.studio.animation.expanded.remove(&entity.id);
                                    } else {
                                        self.studio.animation.expanded.insert(entity.id.clone());
                                    }
                                }
                                let label = if !entity.has_geometry() {
                                    format!("▧ {}", entity.name)
                                } else {
                                    entity.name.clone()
                                };
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 22.0],
                                        egui::Button::selectable(
                                            self.selected.as_ref() == Some(&entity.id),
                                            label,
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&entity.name)
                                    .clicked()
                                {
                                    self.select(Some(entity.id.clone()));
                                    self.hierarchy_focus = false;
                                    self.studio.animation.selection = TimelineSelection::None;
                                }
                            },
                        );
                        let rect = self.timeline_lane(ui, clip.duration, 28.0);
                        self.key_lane(ui, clip, &entity.id, rect, "pose");
                    });
                    if expanded {
                        for (channel, color) in [
                            ("Posição", Color32::from_rgb(135, 203, 223)),
                            ("Rotação", Color32::from_rgb(203, 158, 222)),
                            ("Escala", Color32::from_rgb(154, 207, 150)),
                        ] {
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [160.0, 22.0],
                                    egui::Label::new(
                                        egui::RichText::new(format!("       {channel}"))
                                            .color(color),
                                    ),
                                );
                                let rect = self.timeline_lane(ui, clip.duration, 22.0);
                                self.key_lane(ui, clip, &entity.id, rect, channel);
                            });
                        }
                    }
                }
            });
        if !ui.input(|input| input.pointer.primary_down()) {
            for track in &mut clip.tracks {
                track.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
        }
    }
    fn timeline_lane(&mut self, ui: &mut egui::Ui, duration: f32, height: f32) -> Rect {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width().max(10.0), height),
            Sense::click(),
        );
        ui.painter()
            .rect_filled(rect, 2.0, Color32::from_rgb(30, 37, 45));
        for tick in 0..=10 {
            let x = rect.left() + rect.width() * tick as f32 / 10.0;
            ui.painter().line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                egui::Stroke::new(1.0, Color32::from_gray(56)),
            );
        }
        let x = rect.left() + rect.width() * self.studio.animation.time / duration;
        ui.painter().line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            egui::Stroke::new(1.5, Color32::from_rgb(104, 220, 194)),
        );
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
        let times: Vec<_> = track.keyframes.iter().map(|key| key.time).collect();
        for (index, key) in track.keyframes.iter_mut().enumerate() {
            let center = Pos2::new(
                rect.left() + rect.width() * key.time / clip.duration,
                rect.center().y,
            );
            let selected = matches!(&self.studio.animation.selection, TimelineSelection::Key { target: selected, time } if selected == target && (*time - key.time).abs() < EPSILON);
            draw_diamond(
                ui.painter(),
                center,
                if selected { 6.0 } else { 4.5 },
                if selected {
                    Color32::GOLD
                } else {
                    Color32::from_rgb(125, 195, 218)
                },
            );
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
                .on_hover_text(format!("Quadro completo · {:.3} s", key.time));
            if response.clicked() || response.drag_started() {
                self.hierarchy_focus = false;
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
    fn animation_properties(&mut self, ui: &mut egui::Ui, clip: &mut Clip) {
        let selection = self.studio.animation.selection.clone();
        let mut delete = false;
        match selection {
            TimelineSelection::Key { target, time } => {
                ui.strong("QUADRO-CHAVE");
                let name = self
                    .scene()
                    .entity(&target)
                    .map(|entity| entity.name.as_str())
                    .unwrap_or("Peça ausente");
                ui.label(name);
                ui.separator();
                let other_times: Vec<_> = clip
                    .tracks
                    .iter()
                    .find(|track| track.target == target)
                    .into_iter()
                    .flat_map(|track| &track.keyframes)
                    .filter(|key| (key.time - time).abs() >= EPSILON)
                    .map(|key| key.time)
                    .collect();
                if let Some(key) = clip
                    .tracks
                    .iter_mut()
                    .find(|track| track.target == target)
                    .and_then(|track| {
                        track
                            .keyframes
                            .iter_mut()
                            .find(|key| (key.time - time).abs() < EPSILON)
                    })
                {
                    ui.label("Instante");
                    if ui
                        .add(
                            egui::DragValue::new(&mut key.time)
                                .range(0.0..=clip.duration)
                                .speed(1.0 / 60.0)
                                .suffix(" s"),
                        )
                        .changed()
                    {
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
                    ui.label("Até o próximo quadro");
                    ui.selectable_value(
                        &mut key.interpolation,
                        Interpolation::Linear,
                        "Transição linear",
                    )
                    .on_hover_text(
                        "Rotações seguem o menor arco espacial, sem torções artificiais.",
                    );
                    ui.selectable_value(
                        &mut key.interpolation,
                        Interpolation::Hold,
                        "Manter esta pose",
                    );
                    if ui.button("Copiar quadro").clicked() {
                        self.studio.animation.copied_key = Some(key.clone());
                    }
                    if ui.button("Excluir quadro").clicked() {
                        delete = true;
                    }
                    if ui.button("Voltar à pose provisória").clicked() {
                        self.studio.animation.selection = TimelineSelection::None;
                    }
                    if !text_input_active(ui.ctx())
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        })
                    {
                        delete = true;
                    }
                }
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
                ui.strong("EVENTO DE ANIMAÇÃO");
                ui.separator();
                if let Some(event) = clip.events.get_mut(index) {
                    ui.label("Nome");
                    if self
                        .studio
                        .animation
                        .event_name
                        .as_ref()
                        .is_none_or(|(id, selected, _)| id != &clip.id || *selected != index)
                    {
                        self.studio.animation.event_name =
                            Some((clip.id.clone(), index, event.name.clone()));
                    }
                    let (_, _, name) = self.studio.animation.event_name.as_mut().unwrap();
                    ui.text_edit_singleline(name);
                    if !name.trim().is_empty() {
                        event.name = name.trim().to_owned();
                    } else {
                        ui.small("Informe um nome; o nome anterior fica preservado até lá.");
                    }
                    ui.label("Instante");
                    if ui
                        .add(
                            egui::DragValue::new(&mut event.time)
                                .range(0.0..=clip.duration)
                                .speed(1.0 / 60.0)
                                .suffix(" s"),
                        )
                        .changed()
                    {
                        self.studio.animation.time = event.time;
                    }
                    ui.small("O jogo emite este evento ao atravessar o instante. Em repetição, ele volta a ocorrer em cada passagem.");
                    if ui.button("Copiar evento").clicked() {
                        self.studio.animation.copied_event = Some(event.clone());
                    }
                    if ui.button("Excluir evento").clicked() {
                        delete = true;
                    }
                    if !text_input_active(ui.ctx())
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        })
                    {
                        delete = true;
                    }
                }
                if delete && index < clip.events.len() {
                    clip.events.remove(index);
                    self.studio.animation.selection = TimelineSelection::None;
                }
            }
            TimelineSelection::None => {
                ui.strong("POSE PROVISÓRIA");
                ui.small("Ajuste a peça e grave com + Quadro-chave. A pose-base do modelo permanece preservada.");
                ui.separator();
                if let Some(id) = self.selected.clone()
                    && let Some(mut transform) = self.animation_draft_transform(&id)
                {
                    let name = self
                        .scene()
                        .entity(&id)
                        .map(|entity| entity.name.as_str())
                        .unwrap_or("Peça");
                    ui.label(name);
                    let before = transform.clone();
                    edit_pose(ui, &mut transform);
                    if transform != before {
                        self.set_animation_draft_transform(&id, transform);
                    }
                } else {
                    ui.label("Selecione uma peça, raiz ou grupo.");
                }
            }
        }
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
    vector3(ui, "Posição", &mut transform.position, 0.02, false);
    let mut degrees = transform.rotation.map(f32::to_degrees);
    let previous = degrees;
    vector3(ui, "Rotação °", &mut degrees, 0.5, false);
    if degrees != previous {
        transform.rotation = degrees.map(f32::to_radians);
    }
    vector3(ui, "Escala", &mut transform.scale, 0.02, true);
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
fn timeline_time(x: f32, rect: Rect, duration: f32) -> f32 {
    (((x - rect.left()) / rect.width().max(1.0) * duration * 60.0).round() / 60.0)
        .clamp(0.0, duration)
}
fn draw_diamond(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + Vec2::new(0.0, -radius),
            center + Vec2::new(radius, 0.0),
            center + Vec2::new(0.0, radius),
            center + Vec2::new(-radius, 0.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
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
