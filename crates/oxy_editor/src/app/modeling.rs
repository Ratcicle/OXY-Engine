//! Mesh editing context. Previews share the normal scene renderer and commit one history delta.
use super::*;
#[cfg(test)]
mod atlas_tests;
mod creation;
mod cutting;
mod direct;
#[cfg(test)]
mod measure;
mod numeric;
mod selection;
mod snapping;
mod topology;
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
    last_operation: Option<direct::LastOperation>,
    pub creation: Option<Creation>,
    selection_gesture: Option<selection::BoxGesture>,
    projection_cache: selection::ProjectionCache,
    pub help: bool,
    global: bool,
    snap: Option<snapping::Snap>,
    hovered_edge: Option<u32>,
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
    Extrude,
    Inset,
    Create,
    Flip,
    Loop,
    Knife,
    Bevel,
}
#[derive(Clone)]
pub(super) struct Preview {
    entity: Id,
    original: Entity,
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
    per_face: bool,
    texture: Option<(Id, [u32; 2])>,
    expanded_texture: Option<Id>,
    allow_expansion: bool,
    needs_space: bool,
    count: u32,
    cut_edge: Option<u32>,
    path: cutting::Path,
    notes: Vec<String>,
    started: bool,
    numeric_edit: bool,
    amendment: Option<u64>,
    rollback_selection: Option<Components>,
    inner_size: f32,
    direction: direct::Direction,
}
#[derive(Clone)]
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
    pub(crate) fn mesh_components(&self) -> &Components {
        &self.modeling.selection
    }
    pub(crate) fn select_mesh_face(&mut self, face: Option<u32>, toggle: bool) {
        self.modeling.selection.mode = Mode::Face;
        self.modeling.selection.click(face, toggle);
    }
    #[cfg(test)]
    pub(crate) fn qa_icon_rect(&self, label: &str) -> Option<egui::Rect> {
        crate::icons::qa_control_rect(&self.context, label)
    }
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
        self.modeling
            .preview
            .as_ref()
            .is_some_and(|p| p.started || !p.path.segments.is_empty())
            || self.modeling.creation.is_some()
            || self.modeling.snap.is_some()
    }
    pub(super) fn modeling_active(&self) -> bool {
        self.tab == Tab::Studio && self.studio.tab == StudioTab::Model
    }
    pub(super) fn components_active(&self) -> bool {
        self.modeling_active() && self.modeling.selection.mode != Mode::Object
    }
    pub(crate) fn model_source(&mut self) -> Result<EditableMesh, String> {
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
        self.cancel_box_selection();
        if self.mesh_operation_active() {
            self.warn("Termine o gesto ou pressione Esc antes de trocar de modo.");
            return;
        }
        self.cancel_mesh_operation();
        if self.modeling.selection.mode != mode {
            self.modeling.selection.ids.clear();
            self.modeling.selection.mode = mode;
        }
        self.set_spatial_tool(Tool::Object);
    }
    /// First Studio row: modes and coordinate space, with overflow instead of wrapping.
    pub(super) fn model_toolbar(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{Icon, button, menu_button, width};
        let modes = [
            (Mode::Object, Icon::Object, "Objeto (1)"),
            (Mode::Face, Icon::Face, "Face (2)"),
            (Mode::Edge, Icon::Edge, "Aresta (3)"),
            (Mode::Vertex, Icon::Vertex, "Vértice (4)"),
        ];
        let names = self.preferences.tool_names;
        let required = modes
            .iter()
            .map(|(_, _, label)| width(ui, label, names) + ui.spacing().item_spacing.x)
            .sum::<f32>()
            + 76.;
        ui.add_enabled_ui(!self.mesh_operation_active(), |ui| {
            if ui.available_width() >= required {
                for (mode, icon, label) in modes {
                    if button(
                        ui,
                        icon,
                        label,
                        label,
                        self.modeling.selection.mode == mode,
                        names,
                    )
                    .clicked()
                    {
                        self.model_mode(mode);
                    }
                }
                egui::ComboBox::from_id_salt("component_space")
                    .width(62.)
                    .selected_text(if self.modeling.global {
                        "Global"
                    } else {
                        "Local"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.modeling.global, false, "Local");
                        ui.selectable_value(&mut self.modeling.global, true, "Global");
                    })
                    .response
                    .on_hover_text(
                        "Local acompanha os eixos da peça; Global usa os eixos da cena.",
                    );
            } else {
                let (_, icon, label) = modes
                    .into_iter()
                    .find(|(mode, _, _)| *mode == self.modeling.selection.mode)
                    .unwrap();
                // Names remain available in the popup when the full label cannot fit.
                let names = names && ui.available_width() >= width(ui, label, true);
                menu_button(
                    ui,
                    icon,
                    label,
                    "Modo de seleção (1/2/3/4) e espaço de transformação.",
                    names,
                    |ui| {
                        for (mode, _, label) in modes {
                            if ui
                                .selectable_label(self.modeling.selection.mode == mode, label)
                                .clicked()
                            {
                                self.model_mode(mode);
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.label("Espaço da transformação");
                        ui.selectable_value(&mut self.modeling.global, false, "Local");
                        ui.selectable_value(&mut self.modeling.global, true, "Global");
                    },
                );
            }
        });
    }
    pub(crate) fn model_context_controls(&mut self, ui: &mut egui::Ui) {
        self.model_toolbar(ui);
    }
    pub(super) fn model_tools_controls(&mut self, ui: &mut egui::Ui) {
        self.topology_toolbar(ui);
    }
    pub(super) fn model_secondary_menu(&mut self, ui: &mut egui::Ui) {
        if self.selected.as_deref().and_then(|id|self.scene().entity(id)).is_some_and(|e|e.primitive.is_some())
            && ui.button("Converter em malha editável").on_hover_text("A primeira edição também converte a forma automaticamente, junto de seu comando de desfazer.").clicked() {
            self.convert_selected_mesh();ui.close();
        }
        self.topology_menu(ui);
        if self.components_active() {
            ui.separator();
            if ui.button("Inverter seleção (Shift+I)").clicked()
                && let Ok(mesh) = self.model_source()
            {
                self.modeling.selection.invert(&mesh);
                ui.close();
            }
            if ui.button("Excluir componentes (Delete)").clicked() {
                self.begin_mesh_operation(Operation::Delete);
                ui.close();
            }
            if ui.button("Triangular faces").clicked() {
                self.begin_mesh_operation(Operation::Triangulate);
                ui.close();
            }
        } else if ui.button("Encaixar vértices (Shift+V)").clicked() {
            self.begin_snap();
            ui.close();
        }
        ui.separator();
        ui.collapsing("Informações da malha", |ui| {
            if let Ok(mesh) = self.model_source() {
                ui.label(format!(
                    "{} vértices · {} arestas · {} faces · {} triângulos",
                    mesh.data().vertices.len(),
                    mesh.data().edges.len(),
                    mesh.data().faces.len(),
                    mesh.prepared().triangles.len()
                ));
                ui.label(format!(
                    "{} componentes selecionados",
                    self.modeling.selection.ids.len()
                ));
            }
        });
        if crate::icons::button(
            ui,
            crate::icons::Icon::Help,
            "Ajuda (F1)",
            "Ajuda das ferramentas de modelagem e atalhos.",
            false,
            true,
        )
        .clicked()
        {
            self.modeling.help = true;
            ui.close();
        }
    }
    fn convert_selected_mesh(&mut self) {
        if self.mesh_operation_active() {
            return;
        }
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
            if self.mesh_operation_active() {
                return;
            }
            self.cancel_mesh_operation();
        }
        if !self.operation_available(operation) {
            self.warn("A ferramenta não está disponível neste modo. Arredondar usa Aresta/Vértice; orientação usa Face; corte em loop usa Face/Aresta.");
            return;
        }
        if !self.studio.animation.drafts.is_empty() || self.studio.playing {
            self.warn("Pause e grave ou descarte a pose provisória antes de editar a geometria na pose-base.");
            return;
        }
        if self.modeling.selection.ids.is_empty()
            && !matches!(operation, Operation::Knife | Operation::Loop)
        {
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
        let cut_edge = self.modeling.hovered_edge.or_else(|| {
            (self.modeling.selection.mode == Mode::Edge)
                .then(|| self.modeling.selection.ids.first().copied())
                .flatten()
        });
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
        let texture = self
            .scene()
            .entity(&id)
            .and_then(|e| e.material.texture.clone());
        let texture = if let Some(texture) = texture {
            if !self.ensure_texture(&texture) {
                return;
            }
            let image = self.state.images.get(&texture).unwrap();
            Some((texture, [image.width, image.height]))
        } else {
            None
        };
        self.finish_history(true);
        let original = self.scene().entity(&id).unwrap().clone();
        let values = if operation == Operation::Transform(Gizmo::Scale) {
            [1.; 3]
        } else {
            [0.; 3]
        };
        self.modeling.preview = Some(Preview {
            entity: id,
            original,
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
            per_face: false,
            texture,
            expanded_texture: None,
            allow_expansion: false,
            needs_space: false,
            count: 1,
            cut_edge,
            path: cutting::Path::default(),
            notes: Vec::new(),
            started: false,
            numeric_edit: false,
            amendment: None,
            rollback_selection: None,
            inner_size: if operation == Operation::Inset {
                75.
            } else {
                100.
            },
            direction: direct::Direction::Normal,
        });
        if operation == Operation::Knife {
            self.modeling.selection.mode = Mode::Edge;
        }
        if matches!(
            operation,
            Operation::Delete | Operation::Triangulate | Operation::Create | Operation::Flip
        ) {
            self.update_mesh_preview();
            self.confirm_mesh_operation();
        }
    }
    fn update_mesh_preview(&mut self) {
        if !self.start_mesh_gesture() {
            return;
        }
        if self.restore_neutral_mesh() {
            return;
        }
        let Some(preview) = self.modeling.preview.as_mut() else {
            return;
        };
        if preview.values == preview.previous {
            return;
        }
        preview.previous = preview.values;
        if matches!(
            preview.operation,
            Operation::Extrude
                | Operation::Inset
                | Operation::Create
                | Operation::Flip
                | Operation::Loop
                | Operation::Knife
                | Operation::Bevel
        ) {
            self.update_topology_preview();
            return;
        }
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
            _ => unreachable!(),
        };
        match result {
            Ok(mesh) => {
                preview.error = None;
                let id = preview.entity.clone();
                if mesh.shares_storage(&preview.source) {
                    let original = preview.original.clone();
                    *self.scene_mut().entity_mut(&id).unwrap() = original;
                } else {
                    let entity = self.scene_mut().entity_mut(&id).unwrap();
                    entity.mesh = Some(mesh);
                    entity.primitive = None;
                    entity.primitive_parameters = None;
                    entity.dimensions = [1.; 3];
                }
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
        if let Some(snap) = self.modeling.snap.take() {
            self.history
                .cancel(&mut self.state.project, &mut self.state.images);
            self.selection = snap.selection;
            self.selected = snap.selected;
            return true;
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
            if let Some(id) = &preview.expanded_texture {
                self.renderer.clear_texture_override(id);
                self.game_ui.clear_texture_override(id);
            }
            if preview.started {
                self.history
                    .cancel(&mut self.state.project, &mut self.state.images);
                if let Some(id) = self
                    .scene()
                    .entity(&preview.entity)
                    .and_then(|e| e.material.texture.clone())
                {
                    self.refresh_texture(&id);
                }
            }
            self.modeling.selection = preview.rollback_selection.unwrap_or(preview.selection);
            self.modeling.source = None;
            return true;
        }
        false
    }
    pub(super) fn mesh_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if !self.modeling_active() {
            return false;
        }
        if self.modeling.preview.is_some() {
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.confirm_mesh_operation();
                return true;
            }
            if self.mesh_operation_active() {
                return true;
            }
        }
        if self.modeling.snap.is_some() {
            return true;
        }
        if ctx.input(|i| i.modifiers == egui::Modifiers::SHIFT && i.key_pressed(egui::Key::K)) {
            self.begin_mesh_operation(Operation::Knife);
            return true;
        }
        if ctx.input(|i| i.modifiers == egui::Modifiers::SHIFT && i.key_pressed(egui::Key::V)) {
            self.begin_snap();
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
            for (key, operation) in [
                (egui::Key::E, Operation::Extrude),
                (egui::Key::U, Operation::Inset),
                (egui::Key::F, Operation::Create),
                (egui::Key::N, Operation::Flip),
                (egui::Key::R, Operation::Loop),
                (egui::Key::B, Operation::Bevel),
            ] {
                if ctx.input(|i| i.modifiers == egui::Modifiers::SHIFT && i.key_pressed(key)) {
                    self.begin_mesh_operation(operation);
                    return true;
                }
            }
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
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-130.).max(100.)).show(ui, |ui| {
            ui.label("1: Objeto · 2: Face · 3: Aresta · 4: Vértice. Clique seleciona; Ctrl+clique alterna. Caixa comum substitui os componentes visíveis. Ctrl+caixa soma todos os componentes tocados, inclusive ocultos; a tecla é considerada ao iniciar o gesto. Shift+X mantém seleção através e Shift+I inverte.");
            ui.label("Selecionar um modo preserva a forma paramétrica. Converter em malha editável mantém aparência, pivô, filhos, colisor e animações. A primeira edição também faz essa conversão; cancelar restaura os parâmetros.");
            ui.label("W/E/R escolhem Mover/Girar/Escalar. Armar uma ferramenta não altera a peça. Arraste uma alça e solte para aplicar; valores numéricos aplicam ao soltar, Enter ou sair do campo. Esc cancela o gesto inteiro, inclusive com Alt/Shift. Ctrl+Z durante o gesto apenas cancela. Faces não planas exigem triangulação antes de deformar.");
            ui.label("Shift+E: Extrudir; Shift+U: Criar borda interna. Tamanho interno reduz as dimensões em torno do centro: 50% é metade do tamanho. A moldura mantém o plano original. A distância pode ser positiva ou negativa; Normal da face acompanha sua orientação. Região convexa plana ou cada face independente; buracos, regiões côncavas e interseções sem suporte são recusados.");
            ui.label("Última operação, em Propriedades, recalcula a partir da origem e substitui o mesmo comando. Desfazer retorna ao estado anterior à operação. Outra edição ou troca de peça encerra esse ajuste. UVs antigos mantêm a pintura; se faltar espaço para as faces novas, escolha explicitamente Criar cópia ampliada (2×) ou cancele.");
            ui.label("Corte em loop: clique numa aresta, arraste para posicionar e solte. Bisturi: trace entre bordas; duplo clique ou Enter termina. Inverter orientação, triangular, criar face/aresta e excluir aplicam imediatamente. Visualização reúne grade, encaixe, seleção através e enquadramento; esconder a grade não desliga o encaixe.");
            });
            if ui.button("Fechar ajuda").clicked() || ui.input(|i|i.key_pressed(egui::Key::Escape)){self.modeling.help=false;}
        });
    }
}
