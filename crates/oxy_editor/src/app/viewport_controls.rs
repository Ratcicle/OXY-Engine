//! Shared, non-wrapping viewport controls. Layout never changes the simulation or document.
use super::*;
use crate::icons::{self, Icon};

impl Editor {
    pub(crate) fn show_tool_names(&self) -> bool {
        self.preferences.tool_names
    }
    fn transform_controls(&mut self, ui: &mut egui::Ui, names: bool) {
        ui.add_enabled_ui(!self.mesh_operation_active(),|ui| {
            for (gizmo,icon,label) in [(Gizmo::Move,Icon::Move,"Mover (W)"),(Gizmo::Rotate,Icon::Rotate,"Girar (E)"),(Gizmo::Scale,Icon::Scale,"Escalar (R)")] {
                if icons::button(ui,icon,label,label,self.spatial.mode==Tool::Object&&self.gizmo==gizmo,names).clicked(){self.set_spatial_tool(Tool::Object);self.gizmo=gizmo;}
            }
            for (tool,icon,label,tip) in [
                (Tool::Collider,Icon::Collider,"Editar colisor (C)","Edite a caixa real, inclusive em grupos vazios. O centro desloca somente o colisor; Alt suspende encaixe e Esc cancela."),
                (Tool::Pivot,Icon::Pivot,"Editar pivô (P)","Pivô — ponto de giro. Reposicione sem mover a peça ou seus filhos; Alt suspende encaixe e Esc cancela."),
            ] {
                if icons::button(ui,icon,label,tip,self.spatial.mode==tool,names).clicked(){self.set_spatial_tool(tool);}
            }
        });
    }
    fn active_tool(&self) -> (Icon, &'static str) {
        match self.spatial.mode {
            Tool::Collider => (Icon::Collider, "Colisor (C)"),
            Tool::Pivot => (Icon::Pivot, "Pivô (P)"),
            Tool::Object => match self.gizmo {
                Gizmo::Move => (Icon::Move, "Mover (W)"),
                Gizmo::Rotate => (Icon::Rotate, "Girar (E)"),
                Gizmo::Scale => (Icon::Scale, "Escalar (R)"),
            },
        }
    }
    pub(super) fn viewport_tools_row(&mut self, ui: &mut egui::Ui) {
        let names = self.preferences.tool_names;
        ui.horizontal(|ui| {
            ui.set_min_height(30.);
            let (icon, label) = self.active_tool();
            let menus_width = icons::width(ui, label, names)
                + icons::width(ui, "Visualização", names)
                + if self.modeling_active() {
                    icons::width(ui, "Malha", names)
                } else {
                    0.
                }
                + 24.;
            if ui.available_width() < menus_width {
                icons::menu_button(
                    ui,
                    icon,
                    "Ferramentas",
                    "Ferramenta ativa, transformação, visualização e operações da peça.",
                    names,
                    |ui| {
                        self.transform_controls(ui, true);
                        self.spatial_value_controls(ui);
                        ui.separator();
                        ui.menu_button("Visualização", |ui| self.view_options(ui, false));
                        if self.modeling_active() {
                            ui.menu_button("Malha", |ui| self.model_secondary_menu(ui));
                        }
                    },
                );
            } else {
                let full_width = [
                    "Mover (W)",
                    "Girar (E)",
                    "Escalar (R)",
                    "Editar colisor (C)",
                    "Editar pivô (P)",
                ]
                .iter()
                .map(|label| icons::width(ui, label, names) + ui.spacing().item_spacing.x)
                .sum::<f32>()
                    + menus_width
                    - icons::width(ui, label, names);
                if ui.available_width() >= full_width {
                    self.transform_controls(ui, names);
                } else {
                    icons::menu_button(
                        ui,
                        icon,
                        label,
                        "Ferramenta ativa. Mover/Girar/Escalar (W/E/R), colisor (C) e pivô (P).",
                        names,
                        |ui| {
                            self.transform_controls(ui, true);
                            self.spatial_value_controls(ui);
                        },
                    );
                }
                ui.separator();
                self.visualization_button(ui, false);
                if self.modeling_active() {
                    self.model_tools_controls(ui);
                    icons::menu_button(
                        ui,
                        Icon::More,
                        "Malha",
                        "Todas as operações da malha e informações da seleção.",
                        names,
                        |ui| self.model_secondary_menu(ui),
                    );
                } else if self.spatial.mode != Tool::Object && ui.available_width() >= 32. {
                    icons::menu_button(
                        ui,
                        Icon::More,
                        "Valores",
                        "Valores numéricos e centralização da ferramenta.",
                        false,
                        |ui| self.spatial_value_controls(ui),
                    );
                }
            }
        });
        #[cfg(test)]
        {
            if self.tab == Tab::Studio {
                self.studio.header_bottom = ui.cursor().top();
            }
        }
    }
    pub(crate) fn visualization_button(&mut self, ui: &mut egui::Ui, runtime: bool) {
        icons::menu_button(
            ui,
            Icon::View,
            "Visualização",
            if runtime {
                "Depuração da instância do jogo, somente leitura. Alterações de edição exigem Parar e Jogar novamente."
            } else {
                "Grade, encaixe, colisores, câmera e seleção através. Mostrar grade e encaixar são independentes."
            },
            self.preferences.tool_names,
            |ui| self.view_options(ui, runtime),
        );
    }
    pub(super) fn view_options(&mut self, ui: &mut egui::Ui, runtime: bool) {
        if !runtime {
            ui.checkbox(&mut self.grid, "Mostrar grade").on_hover_text(
                "A grade é somente uma referência visual; ocultá-la não desativa o encaixe.",
            );
            ui.checkbox(&mut self.snap_grid,"Encaixe na grade").on_hover_text("Arredonda movimento e ferramentas ao intervalo escolhido. Alt suspende o encaixe durante o gesto.");
            ui.add(
                egui::DragValue::new(&mut self.grid_size)
                    .range(0.01..=10.)
                    .speed(0.01)
                    .prefix("Intervalo "),
            );
            ui.separator();
        }
        ui.label("Depuração");
        ui.checkbox(&mut self.debug,"Colisores").on_hover_text("Mostra as caixas reais usadas pela simulação. O colisor selecionado continua visível mesmo com a opção desligada.");
        ui.checkbox(&mut self.show_disabled_colliders, "Mostrar desativados")
            .on_hover_text(
                "Mostra caixas desativadas com traço atenuado, sem ativá-las na física.",
            );
        if !runtime {
            ui.separator();
            if icons::button(ui,Icon::Frame,"Enquadrar seleção","Centraliza a câmera na peça ou hierarquia selecionada. Duplo clique na peça faz o mesmo.",false,true).clicked(){self.frame_selection();ui.close();}
            if ui
                .button("Restaurar vista")
                .on_hover_text("Volta à câmera inicial de edição desta cena.")
                .clicked()
            {
                self.camera = CameraState::for_scene(self.scene());
                ui.close();
            }
            if self.components_active() {
                ui.separator();
                ui.checkbox(&mut self.modeling.selection.through,"Selecionar através (Shift+X)").on_hover_text("Inclui componentes ocultos em toda seleção retangular. Ctrl + arrasto ativa através somente naquele gesto, sem mudar esta preferência.");
            }
        }
    }
    fn spatial_value_controls(&mut self, ui: &mut egui::Ui) {
        if self.spatial.mode == Tool::Pivot {
            ui.label("Mover pivô sem mover a peça")
                .on_hover_text("A compensação preserva a montagem e os colisores na pose-base.");
            if ui.button("Centralizar na peça").clicked() {
                self.center_pivot(false);
            }
            if ui.button("Centralizar no conjunto").clicked() {
                self.center_pivot(true);
            }
        }
        if self.spatial.base_pose {
            ui.label("Editando a pose-base")
                .on_hover_text("W/E/R ou Esc retorna ao contexto da animação.");
        }
        if let Some(id) = self.selected.clone()
            && let Some(entity) = self.scene().entity(&id).cloned()
        {
            if self.spatial.mode == Tool::Pivot {
                let mut pivot = entity.transform.pivot;
                vector3(ui, "Pivô — ponto de giro", &mut pivot, 0.05, false);
                if pivot != entity.transform.pivot
                    && let Err(error) =
                        oxy_core::spatial::move_pivot(self.scene_mut(), &id, Vec3::from(pivot))
                {
                    self.log(error);
                    self.notice_last(false);
                }
            } else if self.spatial.mode == Tool::Collider
                && let Some(mut collider) = entity.collider.clone()
            {
                vector3(ui, "Tamanho da caixa", &mut collider.size, 0.05, true);
                vector3(ui, "Deslocamento", &mut collider.offset, 0.05, false);
                if Some(&collider) != entity.collider.as_ref() {
                    let mut candidate = self.scene().clone();
                    candidate.entity_mut(&id).unwrap().collider = Some(collider);
                    match oxy_core::spatial::collider_bounds(&candidate, &id) {
                        Ok(_) => *self.scene_mut() = candidate,
                        Err(e) => self.log(e),
                    }
                }
            }
        }
    }
}
