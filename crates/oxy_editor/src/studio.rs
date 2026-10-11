mod animation;
mod paint;
mod uv;
use crate::app::{Editor, Tab};
pub use animation::AnimationState;

use oxy_core::document::{Entity, Id, Scene};
use oxy_render::CameraState;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq)]
pub enum StudioTab {
    Model,
    Paint,
    Animation,
}
#[derive(Clone, Copy, PartialEq)]
enum PaintTool {
    Brush,
    Fill,
    Sample,
    Select,
}
pub struct Studio {
    #[cfg(test)]
    pub(crate) header_top: f32,
    #[cfg(test)]
    pub(crate) header_bottom: f32,
    pub tab: StudioTab,
    pub owner: Option<Id>,
    /// Object isolated in the Studio, shown with its children and nothing else.
    pub(crate) focus: Option<Id>,
    /// Camera of the mode not on screen: the scene camera while in the Studio, and the
    /// Studio camera elsewhere. Swapped with `Editor::camera` on mode changes.
    pub(crate) parked_camera: Option<CameraState>,
    /// Focus the Studio camera was last framed on.
    framed: Option<Id>,
    pub playing: bool,
    pub animation: AnimationState,
    animation_warning: Option<String>,
    color: [u8; 4],
    radius: f32,
    tool: PaintTool,
    /// Draws the UV islands over the texture pixels.
    show_uv: bool,
    /// The current pointer gesture began while a menu was open, so it does not paint.
    pub(crate) stroke_blocked: bool,
    pub shared_edit: Option<Id>,
    pub hover_uv: Option<[f32; 2]>,
    last_pixel: Option<[f32; 2]>,
    texture: Option<egui::TextureHandle>,
    pub(crate) texture_key: Option<(Id, usize)>,
    pub(crate) canvas_uploads: u64,
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            #[cfg(test)]
            header_top: 0.,
            #[cfg(test)]
            header_bottom: 0.,
            tab: StudioTab::Model,
            owner: None,
            focus: None,
            parked_camera: None,
            framed: None,
            playing: false,
            animation: AnimationState::default(),
            animation_warning: None,
            color: [198, 82, 63, 255],
            radius: 7.,
            tool: PaintTool::Brush,
            show_uv: true,
            stroke_blocked: false,
            shared_edit: None,
            hover_uv: None,
            last_pixel: None,
            texture: None,
            texture_key: None,
            canvas_uploads: 0,
        }
    }
}
/// Display copy of `scene` with only `focus` and its descendants. Ancestors stay as
/// bare transforms, so world matrices, picking and edits by id match the document.
pub(crate) fn isolate(mut scene: Scene, focus: Option<&str>) -> Scene {
    let Some(focus) = focus.filter(|id| scene.entity(id).is_some()) else {
        scene.entities.clear();
        return scene;
    };
    let keep: HashSet<Id> = oxy_core::scene_view::SceneView::new(&scene)
        .descendants(focus)
        .into_iter()
        .collect();
    let mut ancestors = HashSet::new();
    let mut parent = scene.entity(focus).and_then(|e| e.parent.clone());
    while let Some(id) = parent {
        if !ancestors.insert(id.clone()) {
            break;
        }
        parent = scene.entity(&id).and_then(|e| e.parent.clone());
    }
    scene.entities = std::mem::take(&mut scene.entities)
        .into_iter()
        .filter_map(|e| {
            if keep.contains(&e.id) {
                Some(e)
            } else if ancestors.contains(&e.id) {
                let mut bare = Entity::new(e.name, None);
                bare.id = e.id;
                bare.parent = e.parent;
                bare.transform = e.transform;
                bare.visible = e.visible;
                Some(bare)
            } else {
                None
            }
        })
        .collect();
    scene
}

impl Editor {
    /// Ids shown in the Studio: the focus and its descendants.
    pub(crate) fn studio_subtree(&self) -> Vec<Id> {
        self.studio.focus.as_deref().map_or_else(Vec::new, |focus| {
            oxy_core::scene_view::SceneView::new(self.scene()).descendants(focus)
        })
    }
    /// Keeps the Studio focus valid, adopts a new selection when nothing is focused and
    /// frames the Studio camera whenever the focus changes.
    pub(crate) fn sync_studio_focus(&mut self) {
        if self
            .studio
            .focus
            .as_deref()
            .is_some_and(|id| self.scene().entity(id).is_none())
        {
            self.studio.focus = None;
        }
        if self.studio.focus.is_none() {
            self.studio.focus = self.selected.clone();
        }
        if self.studio.focus != self.studio.framed {
            self.studio.framed = self.studio.focus.clone();
            if self.studio.focus.is_some() {
                let ids = self.studio.focus.iter().cloned().collect::<Vec<_>>();
                let scene = self.view_scene();
                crate::app::frame_camera(&mut self.camera, &scene, &ids, self.editor_size);
            }
        }
    }
    /// Entering the Studio isolates the selection unless it already belongs to the
    /// focused object, so editing a part keeps its whole model on screen.
    pub(crate) fn focus_studio_on_selection(&mut self) {
        if let Some(selected) = self.selected.clone()
            && !self.studio_subtree().contains(&selected)
        {
            self.studio.focus = Some(selected);
        }
    }
    /// Changes the work mode. The Studio and the other modes keep separate cameras.
    pub(crate) fn set_tab(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        if (tab == Tab::Studio) != (self.tab == Tab::Studio) {
            let parked = self
                .studio
                .parked_camera
                .take()
                .unwrap_or_else(|| self.camera.clone());
            self.studio.parked_camera = Some(std::mem::replace(&mut self.camera, parked));
        }
        self.tab = tab;
        if tab == Tab::Studio {
            self.focus_studio_on_selection();
            self.sync_studio_focus();
        }
    }
    pub fn studio_ui(&mut self, ui: &mut egui::Ui, dt: f32) {
        self.sync_studio_focus();
        #[cfg(test)]
        {
            self.studio.header_top = ui.cursor().top();
        }
        let previous = self.studio.tab;
        // Decide once from the whole Studio area, before its tabs consume horizontal space.
        // The same decision drives the context menu and the animation properties panel.
        let context_row_height = ROW_HEIGHT + ui.spacing().item_spacing.y;
        let compact_animation =
            ui.available_width() < 850. || ui.available_height() - context_row_height < 450.;
        self.studio_context_row(ui, compact_animation);
        if previous != self.studio.tab {
            self.set_spatial_tool(crate::app::Tool::Object);
        }
        match self.studio.tab {
            StudioTab::Model => self.viewport(ui, false),
            StudioTab::Paint => self.paint_ui(ui),
            StudioTab::Animation => self.animation_ui(ui, dt, compact_animation),
        }
    }
    /// First Studio row: the sub-mode, what it works on, and the view options at the right.
    fn studio_context_row(&mut self, ui: &mut egui::Ui, compact_animation: bool) {
        use crate::icons::{self, Icon};
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ROW_HEIGHT),
            egui::Sense::hover(),
        );
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if self.studio.tab == StudioTab::Model
            && icons::small(&mut right, Icon::Help, "Ajuda da modelagem (F1)", 26.).clicked()
        {
            self.open_model_help();
        }
        self.visualization_button(&mut right, false);
        let used = right.min_rect().width();
        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.with_max_x(rect.max.x - used - 8.))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        left.add_enabled_ui(!self.mesh_operation_active(), |ui| self.studio_tabs(ui));
        match self.studio.tab {
            StudioTab::Model => {
                crate::widgets::divider(&mut left);
                self.model_context_controls(&mut left);
            }
            StudioTab::Paint => {
                crate::widgets::divider(&mut left);
                self.paint_tool_choice(&mut left);
            }
            StudioTab::Animation => {
                crate::widgets::divider(&mut left);
                self.animation_context_controls(&mut left, compact_animation);
            }
        }
    }
    fn studio_tabs(&mut self, ui: &mut egui::Ui) {
        const TABS: [(StudioTab, &str); 3] = [
            (StudioTab::Model, "Modelagem"),
            (StudioTab::Paint, "Pintura"),
            (StudioTab::Animation, "Animação"),
        ];
        let width = crate::widgets::segmented_width(ui, &TABS.map(|(_, label)| label));
        if ui.available_width() < width + 60. {
            let label = TABS
                .iter()
                .find(|(tab, _)| *tab == self.studio.tab)
                .map_or("Modelagem", |(_, label)| label);
            ui.menu_button(label, |ui| {
                for (tab, label) in TABS {
                    if ui
                        .selectable_value(&mut self.studio.tab, tab, label)
                        .clicked()
                    {
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Ferramentas do Estúdio: modelagem, pintura e animação.");
        } else {
            crate::widgets::segmented(ui, &mut self.studio.tab, &TABS);
        }
    }
}

/// Height of each Studio toolbar row, in points before interface scale.
pub(crate) const ROW_HEIGHT: f32 = crate::theme::CONTROL_HEIGHT + 6.;

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::document::{Primitive, SceneKind};

    #[test]
    fn isolation_keeps_subtree_and_world_matrices() {
        let mut scene = Scene::new("Cena", SceneKind::ThreeD);
        let mut player = Entity::new("Jogador", Some(Primitive::Cube));
        player.transform.position = [4., 0., -2.];
        player.transform.rotation = [0., 0.7, 0.];
        let mut arm = Entity::new("Braço", Some(Primitive::Cube));
        arm.parent = Some(player.id.clone());
        arm.transform.position = [0.5, 1.2, 0.];
        let mut hand = Entity::new("Mão", Some(Primitive::Sphere));
        hand.parent = Some(arm.id.clone());
        hand.transform.position = [0., -0.6, 0.];
        let floor = Entity::new("Piso", Some(Primitive::Plane));
        let (player_id, arm_id, hand_id, floor_id) = (
            player.id.clone(),
            arm.id.clone(),
            hand.id.clone(),
            floor.id.clone(),
        );
        scene.entities.extend([player, arm, hand, floor]);

        let isolated = isolate(scene.clone(), Some(&arm_id));
        let ids: Vec<_> = isolated.entities.iter().map(|e| e.id.clone()).collect();
        assert!(ids.contains(&arm_id) && ids.contains(&hand_id));
        assert!(!ids.contains(&floor_id), "other objects leave the Studio");
        let parent = isolated
            .entity(&player_id)
            .expect("ancestor kept for its transform");
        assert!(
            !parent.has_geometry(),
            "ancestors are shown without geometry"
        );
        for id in [&arm_id, &hand_id] {
            assert!(
                isolated
                    .world_matrix(id)
                    .unwrap()
                    .abs_diff_eq(scene.world_matrix(id).unwrap(), 1e-6),
                "display positions match the document"
            );
        }
        assert_eq!(
            isolated.entity(&hand_id),
            scene.entity(&hand_id),
            "isolated pieces are unchanged copies"
        );
        assert!(isolate(scene.clone(), None).entities.is_empty());
        assert!(isolate(scene, Some("ausente")).entities.is_empty());
    }
}
