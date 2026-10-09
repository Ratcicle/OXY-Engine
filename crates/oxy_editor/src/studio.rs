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
        let context_row_height = crate::theme::CONTROL_HEIGHT.max(ui.spacing().interact_size.y)
            + ui.spacing().item_spacing.y;
        let compact_animation =
            ui.available_width() < 850. || ui.available_height() - context_row_height < 450.;
        ui.horizontal(|ui| {
            ui.set_min_height(crate::theme::CONTROL_HEIGHT);
            ui.add_enabled_ui(!self.mesh_operation_active(),|ui| {
                if ui.available_width()<310. {
                    let label=match self.studio.tab {StudioTab::Model=>"Modelagem",StudioTab::Paint=>"Pintura",StudioTab::Animation=>"Animação"};
                    ui.menu_button(label,|ui| {
                        for (tab,label) in [(StudioTab::Model,"Modelagem"),(StudioTab::Paint,"Pintura"),(StudioTab::Animation,"Animação")] {
                            if ui.selectable_value(&mut self.studio.tab,tab,label).clicked(){ui.close();}
                        }
                    }).response.on_hover_text("Ferramentas do Estúdio: modelagem, pintura e animação.");
                } else {
                    ui.selectable_value(&mut self.studio.tab,StudioTab::Model,"Modelagem").on_hover_text("Crie e edite a geometria. Arraste peças na hierarquia para definir articulações.");
                    ui.selectable_value(&mut self.studio.tab,StudioTab::Paint,"Pintura").on_hover_text("Edite pixels na imagem ou na peça. Ctrl+Z desfaz o traço inteiro.");
                    ui.selectable_value(&mut self.studio.tab,StudioTab::Animation,"Animação").on_hover_text("Grave poses por peça ou grupo na linha do tempo.");
                }
            });
            if self.studio.tab==StudioTab::Model {
                ui.separator();
                self.model_context_controls(ui);
            } else if self.studio.tab==StudioTab::Animation {
                self.animation_context_controls(ui,compact_animation);
            }
            if ui.available_width()>72.
                && let Some(id)=if self.studio.tab==StudioTab::Animation {self.studio.owner.as_ref().or(self.selected.as_ref())} else {self.selected.as_ref().or(self.studio.focus.as_ref())}
                && let Some(entity)=self.scene().entity(id) {
                ui.add_sized([ui.available_width(),crate::theme::CONTROL_HEIGHT],egui::Label::new(&entity.name).truncate()).on_hover_text(format!("Peça em edição: {}",entity.name));
            }
        });
        if previous != self.studio.tab {
            self.set_spatial_tool(crate::app::Tool::Object);
        }
        match self.studio.tab {
            StudioTab::Model => self.viewport(ui, false),
            StudioTab::Paint => self.paint_ui(ui),
            StudioTab::Animation => self.animation_ui(ui, dt, compact_animation),
        }
    }
}

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
