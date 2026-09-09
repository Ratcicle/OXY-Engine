//! Mesh editing context. Previews share the normal scene renderer and commit one history delta.
use super::*;
mod creation;
mod viewport;
use glam::Mat4;
use oxy_core::geometry::{
    EditableMesh, primitives,
    selection::{Mode, Selection as Components},
};

#[derive(Clone, PartialEq)]
struct SourceKey {
    id: Id,
    primitive: Option<Primitive>,
    segments: u32,
    dimensions: [f32; 3],
    parameters: Option<primitives::Parameters>,
    revision: u64,
}
#[derive(Default)]
pub(super) struct ModelState {
    pub selection: Components,
    source: Option<(SourceKey, EditableMesh)>,
    pub preview: Option<Preview>,
    pub creation: Option<Creation>,
    box_start: Option<Pos2>,
    pub help: bool,
    global: bool,
}
pub(super) struct Creation {
    candidate: Entity,
    original: Option<Entity>,
    selection: Selection,
    selected: Option<Id>,
    error: Option<String>,
    allow_mapping: bool,
    last_checked: Option<(Primitive, u32, primitives::Parameters)>,
}
#[derive(Clone, Copy, PartialEq)]
enum Operation {
    Transform(Gizmo),
    Delete,
    Triangulate,
}
pub(super) struct Preview {
    entity: Id,
    source: EditableMesh,
    selection: Components,
    operation: Operation,
    values: [f32; 3],
    previous: [f32; 3],
    world: Mat4,
    center: Vec3,
    global: bool,
    keep_edges: bool,
    error: Option<String>,
    drag: Option<Drag>,
}
struct Drag {
    axis: Option<usize>,
    start: Pos2,
    direction: Vec2,
    base: [f32; 3],
    plane_origin: Vec3,
    plane_normal: Vec3,
    world_axis: Vec3,
    start_world: Vec3,
}

impl Editor {
    #[cfg(test)]
    pub(crate) fn qa_mesh_info(&self) -> (Mode, usize, bool, usize) {
        (
            self.modeling.selection.mode,
            self.modeling.selection.ids.len(),
            self.modeling.preview.is_some(),
            self.history.undo_len(),
        )
    }
    pub(crate) fn mesh_operation_active(&self) -> bool {
        self.modeling.preview.is_some() || self.modeling.creation.is_some()
    }
    pub(super) fn modeling_active(&self) -> bool {
        self.tab == Tab::Studio && self.studio.tab == StudioTab::Model
    }
    pub(super) fn components_active(&self) -> bool {
        self.modeling_active() && self.modeling.selection.mode != Mode::Object
    }
    fn model_source(&mut self) -> Result<EditableMesh, String> {
        if self.selection.ids.len() != 1 {
            return Err("Escolha uma peça para editar seus componentes.".into());
        }
        let id = self.selected.as_ref().ok_or("Selecione uma peça.")?;
        let entity = self.scene().entity(id).ok_or("Peça não encontrada.")?;
        let key = SourceKey {
            id: id.clone(),
            primitive: entity.primitive,
            segments: entity.segments,
            dimensions: entity.dimensions,
            parameters: entity.primitive_parameters,
            revision: entity.mesh.as_ref().map_or(0, EditableMesh::revision),
        };
        if let Some((old, mesh)) = &self.modeling.source
            && old == &key
        {
            return Ok(mesh.clone());
        }
        let mesh = if let Some(mesh) = &entity.mesh {
            mesh.clone()
        } else {
            let mut candidate = entity.clone();
            primitives::convert(&mut candidate)?;
            candidate.mesh.unwrap()
        };
        if self
            .modeling
            .source
            .as_ref()
            .is_some_and(|(old, _)| old.id != key.id)
        {
            self.modeling.selection.ids.clear();
        }
        self.modeling.selection.sanitize(&mesh);
        self.modeling.source = Some((key, mesh.clone()));
        Ok(mesh)
    }
    fn model_mode(&mut self, mode: Mode) {
        if self.modeling.preview.is_some() {
            self.warn("Confirme ou cancele a operação antes de trocar de modo.");
            return;
        }
        if self.modeling.selection.mode != mode {
            self.modeling.selection.ids.clear();
            self.modeling.selection.mode = mode;
        }
        self.set_spatial_tool(Tool::Object);
    }
    pub(super) fn model_toolbar(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{Icon, button};
        ui.horizontal_wrapped(|ui|{
            for (mode,icon,label,tip) in [(Mode::Object,Icon::Object,"Objeto","Objeto (1): transforme a peça inteira."),(Mode::Face,Icon::Face,"Face","Face (2): selecione polígonos da peça."),(Mode::Edge,Icon::Edge,"Aresta","Aresta (3): selecione bordas reais da malha."),(Mode::Vertex,Icon::Vertex,"Vértice","Vértice (4): selecione pontos da malha.")] {
                if button(ui,icon,label,tip,self.modeling.selection.mode==mode,self.preferences.tool_names).clicked(){self.model_mode(mode);}
            }
            if self.components_active(){
                ui.checkbox(&mut self.modeling.selection.through,"Selecionar através").on_hover_text("Shift+X: inclui componentes ocultos. Desativado, respeita as superfícies à frente.");
                egui::ComboBox::from_id_salt("component_space").selected_text(if self.modeling.global{"Global"}else{"Local"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.modeling.global,false,"Local");ui.selectable_value(&mut self.modeling.global,true,"Global");});
                if ui.button("Inverter seleção").on_hover_text("Shift+I: troca selecionados e não selecionados neste modo.").clicked() && let Ok(mesh)=self.model_source(){self.modeling.selection.invert(&mesh);}
            }
            if ui.button("Ajuda (F1)").clicked(){self.modeling.help=true;}
        });
        if let Some(id) = self.selected.as_ref()
            && let Some(entity) = self.scene().entity(id)
            && entity.has_geometry()
        {
            if entity.mesh.is_none() {
                ui.horizontal_wrapped(|ui|{ui.label("Forma paramétrica").on_hover_text("Selecionar componentes preserva os parâmetros. A primeira edição converte a forma em polígonos editáveis.");if ui.button("Converter em malha editável").clicked(){self.convert_selected_mesh();}});
            } else if let Some(mesh) = &entity.mesh {
                ui.small(format!(
                    "{} vértices · {} arestas · {} faces · {} triângulos",
                    mesh.data().vertices.len(),
                    mesh.data().edges.len(),
                    mesh.data().faces.len(),
                    mesh.prepared().triangles.len()
                ));
            }
        }
        if self.components_active() && self.modeling.preview.is_none() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.modeling.selection.ids.is_empty(),
                        egui::Button::new("Transformar seleção"),
                    )
                    .clicked()
                {
                    self.begin_mesh_operation(Operation::Transform(self.gizmo));
                }
                ui.menu_button("Componentes", |ui| {
                    if ui.button("Excluir componentes").clicked() {
                        self.begin_mesh_operation(Operation::Delete);
                        ui.close();
                    }
                    if ui.button("Triangular faces").clicked() {
                        self.begin_mesh_operation(Operation::Triangulate);
                        ui.close();
                    }
                });
            });
        }
        self.mesh_preview_panel(ui);
    }
    fn convert_selected_mesh(&mut self) {
        if !self.studio.animation.drafts.is_empty() || self.studio.playing {
            self.warn("Pause e grave ou descarte a pose provisória antes de editar a geometria na pose-base.");
            return;
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        let Some(mut e) = self.scene().entity(&id).cloned() else {
            return;
        };
        match primitives::convert(&mut e) {
            Ok(()) => {
                self.finish_history(true);
                self.history.begin(
                    "Converter forma em malha",
                    &self.state.project,
                    &self.state.images,
                );
                *self.scene_mut().entity_mut(&id).unwrap() = e;
                self.finish_history(true);
            }
            Err(e) => self.warn(e),
        }
    }
    fn begin_mesh_operation(&mut self, operation: Operation) {
        if self.modeling.preview.is_some() {
            return;
        }
        if !self.studio.animation.drafts.is_empty() || self.studio.playing {
            self.warn("Pause e grave ou descarte a pose provisória antes de editar a geometria na pose-base.");
            return;
        }
        if self.modeling.selection.ids.is_empty() {
            self.warn("Selecione componentes da malha.");
            return;
        }
        let source = match self.model_source() {
            Ok(v) => v,
            Err(e) => {
                self.warn(e);
                return;
            }
        };
        let id = self.selected.clone().unwrap();
        let world = match self.scene().world_matrix(&id) {
            Ok(m) if m.is_finite() && m.determinant().abs() > 1e-8 => m,
            _ => {
                self.warn("A transformação da peça não pode ser invertida.");
                return;
            }
        };
        let vertices = self.modeling.selection.vertices(&source);
        let center = vertices
            .iter()
            .filter_map(|id| source.position(*id))
            .sum::<Vec3>()
            / vertices.len().max(1) as f32;
        self.finish_history(true);
        self.history.begin(
            "Editar componentes da malha",
            &self.state.project,
            &self.state.images,
        );
        if let Err(e) = primitives::convert(self.scene_mut().entity_mut(&id).unwrap()) {
            self.history
                .cancel(&mut self.state.project, &mut self.state.images);
            self.warn(e);
            return;
        }
        let values = if operation == Operation::Transform(Gizmo::Scale) {
            [1.; 3]
        } else {
            [0.; 3]
        };
        self.modeling.preview = Some(Preview {
            entity: id,
            source,
            selection: self.modeling.selection.clone(),
            operation,
            values,
            previous: [f32::NAN; 3],
            world,
            center,
            global: self.modeling.global,
            keep_edges: true,
            error: None,
            drag: None,
        });
        self.update_mesh_preview();
    }
    fn update_mesh_preview(&mut self) {
        let Some(preview) = self.modeling.preview.as_mut() else {
            return;
        };
        if preview.values == preview.previous {
            return;
        }
        preview.previous = preview.values;
        let center = if preview.global {
            preview.world.transform_point3(preview.center)
        } else {
            preview.center
        };
        let result = match preview.operation {
            Operation::Transform(tool) => {
                let delta = match tool {
                    Gizmo::Move => Mat4::from_translation(Vec3::from(preview.values)),
                    Gizmo::Rotate => {
                        Mat4::from_translation(center)
                            * Mat4::from_quat(glam::Quat::from_euler(
                                glam::EulerRot::XYZ,
                                preview.values[0].to_radians(),
                                preview.values[1].to_radians(),
                                preview.values[2].to_radians(),
                            ))
                            * Mat4::from_translation(-center)
                    }
                    Gizmo::Scale => {
                        Mat4::from_translation(center)
                            * Mat4::from_scale(Vec3::from(preview.values))
                            * Mat4::from_translation(-center)
                    }
                };
                let delta = if preview.global {
                    preview.world.inverse() * delta * preview.world
                } else {
                    delta
                };
                oxy_core::geometry::edit::transform(&preview.source, &preview.selection, delta)
            }
            Operation::Delete => oxy_core::geometry::edit::delete(
                &preview.source,
                &preview.selection,
                preview.keep_edges,
            ),
            Operation::Triangulate => {
                oxy_core::geometry::edit::triangulate(&preview.source, &preview.selection)
            }
        };
        match result {
            Ok(mesh) => {
                preview.error = None;
                let id = preview.entity.clone();
                self.scene_mut().entity_mut(&id).unwrap().mesh = Some(mesh);
            }
            Err(error) => {
                preview.error = Some(error);
            }
        }
    }
    pub(super) fn cancel_mesh_operation(&mut self) -> bool {
        if self.mesh_operation_active() {
            self.context.memory_mut(|memory| {
                if let Some(id) = memory.focused() {
                    memory.surrender_focus(id);
                }
            });
        }
        if let Some(creation) = self.modeling.creation.take() {
            self.history
                .cancel(&mut self.state.project, &mut self.state.images);
            self.selection = creation.selection;
            self.selected = creation.selected;
            self.modeling.source = None;
            return true;
        }
        if let Some(preview) = self.modeling.preview.take() {
            self.history
                .cancel(&mut self.state.project, &mut self.state.images);
            self.modeling.selection = preview.selection;
            self.modeling.source = None;
            return true;
        }
        false
    }
    fn confirm_mesh_operation(&mut self) {
        if self
            .modeling
            .preview
            .as_ref()
            .is_some_and(|p| p.error.is_some())
        {
            return;
        }
        self.modeling.preview = None;
        self.context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        self.modeling.source = None;
        self.finish_history(true);
        let _ = self.model_source();
    }
    fn mesh_preview_panel(&mut self, ui: &mut egui::Ui) {
        let Some(preview) = self.modeling.preview.as_mut() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        ui.group(|ui| {
            ui.strong(match preview.operation {
                Operation::Transform(Gizmo::Move) => "Prévia: mover componentes",
                Operation::Transform(Gizmo::Rotate) => "Prévia: girar componentes",
                Operation::Transform(Gizmo::Scale) => "Prévia: escalar componentes",
                Operation::Delete => "Prévia: excluir componentes",
                Operation::Triangulate => "Prévia: triangular faces",
            });
            if let Operation::Transform(tool) = preview.operation {
                ui.horizontal_wrapped(|ui| {
                    for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                        ui.add(
                            egui::DragValue::new(&mut preview.values[axis])
                                .speed(if tool == Gizmo::Rotate { 1. } else { 0.02 })
                                .prefix(format!("{label} ")),
                        );
                    }
                });
            }
            if preview.operation == Operation::Delete
                && ui
                    .checkbox(&mut preview.keep_edges, "Preservar bordas ao excluir faces")
                    .changed()
            {
                preview.previous = [f32::NAN; 3];
            }
            if let Some(error) = &preview.error {
                ui.colored_label(Color32::LIGHT_YELLOW, error);
            }
            ui.horizontal(|ui| {
                confirm = ui
                    .add_enabled(
                        preview.error.is_none(),
                        egui::Button::new("Confirmar (Enter)"),
                    )
                    .clicked();
                cancel = ui.button("Cancelar (Esc)").clicked();
            });
        });
        self.update_mesh_preview();
        if cancel {
            self.cancel_mesh_operation();
        } else if confirm || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.confirm_mesh_operation();
        }
    }
    pub(super) fn mesh_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if !self.modeling_active() {
            return false;
        }
        if self.modeling.preview.is_some() {
            return true;
        }
        if ctx.input(|i| i.modifiers.is_none()) {
            for (key, mode) in [
                (egui::Key::Num1, Mode::Object),
                (egui::Key::Num2, Mode::Face),
                (egui::Key::Num3, Mode::Edge),
                (egui::Key::Num4, Mode::Vertex),
            ] {
                if ctx.input(|i| i.key_pressed(key)) {
                    self.model_mode(mode);
                    return true;
                }
            }
        }
        if self.components_active() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::I)) {
                if let Ok(mesh) = self.model_source() {
                    self.modeling.selection.invert(&mesh);
                }
                return true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::X)) {
                self.modeling.selection.through = !self.modeling.selection.through;
                return true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                if let Ok(mesh) = self.model_source() {
                    self.modeling.selection.ids = self.modeling.selection.available(&mesh);
                }
                return true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                self.begin_mesh_operation(Operation::Delete);
                return true;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.modeling.help = true;
            return true;
        }
        false
    }
    pub(super) fn mesh_help(&mut self, ctx: &egui::Context) {
        if !self.modeling.help {
            return;
        }
        egui::Modal::new(egui::Id::new("mesh_help")).show(ctx,|ui|{
            ui.set_max_width(530.);ui.heading("Modelagem por componentes");
            ui.label("1: Objeto · 2: Face · 3: Aresta · 4: Vértice. Clique seleciona; Ctrl+clique alterna; arraste uma caixa para selecionar vários. Shift+X inclui os ocultos e Shift+I inverte a seleção.");
            ui.label("Selecionar um modo preserva a forma paramétrica. Converter em malha editável mantém aparência, pivô, filhos, colisor e animações. A primeira edição também faz essa conversão; cancelar restaura os parâmetros.");
            ui.label("W/E/R escolhem Mover/Girar/Escalar. Transformar seleção abre uma prévia com alças e valores Local/Global. Enter confirma e Esc cancela, inclusive com Alt/Shift. Faces não planas são recusadas; use Triangular faces antes de deformá-las.");
            if ui.button("Fechar ajuda").clicked() || ui.input(|i|i.key_pressed(egui::Key::Escape)){self.modeling.help=false;}
        });
    }
}
