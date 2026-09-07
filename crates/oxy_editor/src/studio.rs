mod animation;
mod paint;
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
        }
    }
}
impl Editor {
    pub fn studio_ui(&mut self, ui: &mut egui::Ui, dt: f32) {
        let previous = self.studio.tab;
        ui.horizontal_wrapped(|ui| {
            ui.heading("Estúdio");
            ui.separator();
            ui.selectable_value(&mut self.studio.tab, StudioTab::Model, "Modelagem");
            ui.selectable_value(&mut self.studio.tab, StudioTab::Paint, "Pintura");
            ui.selectable_value(&mut self.studio.tab, StudioTab::Animation, "Animação");
        });
        if previous != self.studio.tab {
            self.set_spatial_tool(crate::app::Tool::Object);
        }
        if self.selected.is_none() {
            ui.label("Selecione uma peça ou use + Objeto na hierarquia. Todas as peças continuam editáveis.");
        }
        match self.studio.tab {
            StudioTab::Model => {
                ui.label("Adicione formas em + Objeto. No Estúdio, a peça criada recebe a peça selecionada como pai. Dimensões, pivô e parentesco ficam nas propriedades.");
                self.viewport(ui, false);
            }
            StudioTab::Paint => self.paint_ui(ui),
            StudioTab::Animation => self.animation_ui(ui, dt),
        }
    }
}
