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
    #[cfg(test)]
    pub(crate) header_top: f32,
    #[cfg(test)]
    pub(crate) header_bottom: f32,
    pub tab: StudioTab,
    pub owner: Option<Id>,
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
impl Editor {
    pub fn studio_ui(&mut self, ui: &mut egui::Ui, dt: f32) {
        #[cfg(test)]
        {
            self.studio.header_top = ui.cursor().top();
        }
        let previous = self.studio.tab;
        // Decide once from the whole Studio area, before its tabs consume horizontal space.
        // The same decision drives the context menu and the animation properties panel.
        let context_row_height =
            30.0_f32.max(ui.spacing().interact_size.y) + ui.spacing().item_spacing.y;
        let compact_animation =
            ui.available_width() < 850. || ui.available_height() - context_row_height < 450.;
        ui.horizontal(|ui| {
            ui.set_min_height(30.);
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
                && let Some(id)=if self.studio.tab==StudioTab::Animation {self.studio.owner.as_ref().or(self.selected.as_ref())} else {self.selected.as_ref().or(self.studio.owner.as_ref())}
                && let Some(entity)=self.scene().entity(id) {
                ui.add_sized([ui.available_width(),30.],egui::Label::new(&entity.name).truncate()).on_hover_text(format!("Peça em edição: {}",entity.name));
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
