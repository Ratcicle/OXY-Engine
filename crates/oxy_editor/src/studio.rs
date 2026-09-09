mod animation;
mod paint;
mod uv;
use crate::app::Editor;
pub use animation::AnimationState;

use oxy_core::document::Id;

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
    pub tab: StudioTab,
    pub owner: Option<Id>,
    pub playing: bool,
    pub animation: AnimationState,
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
            tab: StudioTab::Model,
            owner: None,
            playing: false,
            animation: AnimationState::default(),
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
impl Editor {
    pub fn studio_ui(&mut self, ui: &mut egui::Ui, dt: f32) {
        let previous = self.studio.tab;
        ui.horizontal_wrapped(|ui| {
            if self.mesh_operation_active(){ui.disable();}
            if !Self::compact_layout(ui.ctx()) {
                ui.heading("Estúdio");
                ui.separator();
            }
            ui.selectable_value(&mut self.studio.tab, StudioTab::Model, "Modelagem").on_hover_text("Crie formas em + Objeto. Arraste peças na hierarquia para definir suas articulações.");
            ui.selectable_value(&mut self.studio.tab, StudioTab::Paint, "Pintura").on_hover_text("Edite os pixels à esquerda ou pinte diretamente na peça. Ctrl+Z desfaz a pincelada inteira.");
            ui.selectable_value(&mut self.studio.tab, StudioTab::Animation, "Animação");
        });
        if previous != self.studio.tab {
            self.set_spatial_tool(crate::app::Tool::Object);
        }
        if !Self::compact_layout(ui.ctx())
            && self.studio.tab != StudioTab::Animation
            && let Some(id) = self.studio.owner.as_ref().or(self.selected.as_ref())
            && let Some(entity) = self.scene().entity(id)
        {
            ui.label(format!("Modelo: {}", entity.name));
        }
        match self.studio.tab {
            StudioTab::Model => {
                self.viewport(ui, false);
            }
            StudioTab::Paint => self.paint_ui(ui),
            StudioTab::Animation => self.animation_ui(ui, dt),
        }
    }
}
