//! Shared, non-wrapping viewport controls. Layout never changes the simulation or document.
use super::*;
use crate::icons::{self, Icon};

impl Editor {
    fn transform_controls(&mut self, ui: &mut egui::Ui, names: bool) {
        ui.add_enabled_ui(!self.mesh_operation_active(),|ui| {
            for (gizmo,icon,label) in [(Gizmo::Move,Icon::Move,"Mover (W)"),(Gizmo::Rotate,Icon::Rotate,"Girar (E)"),(Gizmo::Scale,Icon::Scale,"Escalar (R)")] {
                if icons::button(ui,icon,label,label,self.spatial.mode==Tool::Object&&self.transform_ui.gizmo==gizmo,names).clicked(){self.set_spatial_tool(Tool::Object);self.transform_ui.gizmo=gizmo;}
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
            Tool::Object => match self.transform_ui.gizmo {
                Gizmo::Move => (Icon::Move, "Mover (W)"),
                Gizmo::Rotate => (Icon::Rotate, "Girar (E)"),
                Gizmo::Scale => (Icon::Scale, "Escalar (R)"),
            },
        }
    }
    /// Tool row of the Scene and the Studio: transform tools at the left, then the
    /// operations of the current mode; menus and view options sit at the right edge.
    pub(super) fn viewport_tools_row(&mut self, ui: &mut egui::Ui) {
        let names = self.preferences.tool_names;
        let modeling = self.modeling_active();
        let studio = self.tab == Tab::Studio;
        let (rect, mut right) = crate::widgets::toolbar_right(ui, crate::studio::ROW_HEIGHT);
        if modeling {
            icons::menu_button(
                &mut right,
                Icon::More,
                "Malha",
                "Todas as operações da malha, a seleção e a ajuda da modelagem.",
                rect.width() >= 560.,
                |ui| self.model_secondary_menu(ui),
            );
        } else {
            if !studio {
                self.visualization_button(&mut right, false);
            }
            if self.spatial.mode != Tool::Object {
                icons::menu_button(
                    &mut right,
                    Icon::More,
                    "Valores",
                    "Valores numéricos e centralização da ferramenta.",
                    false,
                    |ui| self.spatial_value_controls(ui),
                );
            }
            if studio && self.studio.tab == StudioTab::Animation {
                self.pose_base_toggle(&mut right);
            }
        }
        let mut left = crate::widgets::toolbar_left(ui, rect, &right);
        // Buttons of a group sit close together; dividers separate the groups.
        left.spacing_mut().item_spacing.x = 2.;
        let (icon, label) = self.active_tool();
        let spacing = left.spacing().item_spacing.x;
        let tools_width = |ui: &mut egui::Ui, names: bool| {
            [
                "Mover (W)",
                "Girar (E)",
                "Escalar (R)",
                "Editar colisor (C)",
                "Editar pivô (P)",
            ]
            .iter()
            .map(|label| icons::width(ui, label, names) + spacing)
            .sum::<f32>()
                + 10.
        };
        // Tool names are the first thing to go: operations keep their labels longer.
        let operations = if modeling {
            self.topology_width(&mut left) + 9. + spacing
        } else {
            0.
        };
        let available = left.available_width();
        if names && available >= tools_width(&mut left, true) + operations {
            self.transform_controls(&mut left, true);
        } else if available >= tools_width(&mut left, false) {
            self.transform_controls(&mut left, false);
        } else {
            icons::menu_button(
                &mut left,
                icon,
                "Ferramentas",
                &format!(
                    "Ferramenta ativa: {label}. Mover/Girar/Escalar (W/E/R), colisor (C) e pivô (P)."
                ),
                false,
                |ui| {
                    self.transform_controls(ui, true);
                    self.spatial_value_controls(ui);
                },
            );
        }
        if modeling {
            crate::widgets::divider(&mut left);
            self.model_tools_controls(&mut left);
        }
        #[cfg(test)]
        {
            if self.tab == Tab::Studio {
                self.studio.header_bottom = ui.cursor().top();
            }
        }
    }
    /// View options; always an icon, at the right edge of the work area.
    pub(crate) fn visualization_button(&mut self, ui: &mut egui::Ui, runtime: bool) {
        icons::menu_button(
            ui,
            Icon::View,
            "Visualização",
            if runtime {
                "Visualização: depuração da instância do jogo, somente leitura. Alterações de edição exigem Parar e Jogar novamente."
            } else {
                "Visualização: grade, encaixe, colisores, câmeras e seleção através."
            },
            false,
            |ui| self.view_options(ui, runtime),
        );
    }
    pub(super) fn view_options(&mut self, ui: &mut egui::Ui, runtime: bool) {
        ui.set_min_width(240.);
        if !runtime {
            // Actions first: they stay visible when a short window scrolls the menu.
            ui.horizontal(|ui| {
                if icons::button(ui,Icon::Frame,"Enquadrar seleção","Centraliza a câmera na peça ou hierarquia selecionada. Duplo clique na peça faz o mesmo.",false,true).clicked(){self.frame_selection();ui.close();}
                if ui
                    .button("Restaurar vista")
                    .on_hover_text("Volta à câmera inicial de edição desta cena.")
                    .clicked()
                {
                    self.camera = CameraState::for_scene(self.scene());
                    ui.close();
                }
            });
            crate::widgets::menu_label(ui, "Grade");
            ui.checkbox(&mut self.view.grid, "Mostrar grade")
                .on_hover_text(
                    "A grade é somente uma referência visual; ocultá-la não desativa o encaixe.",
                );
            ui.checkbox(&mut self.view.snap_grid,"Encaixe na grade").on_hover_text("Arredonda movimento e ferramentas ao intervalo escolhido. Alt suspende o encaixe durante o gesto.");
            ui.add(
                egui::DragValue::new(&mut self.view.grid_size)
                    .range(0.01..=10.)
                    .speed(0.01)
                    .prefix("Intervalo ")
                    .suffix(" m"),
            );
        }
        crate::widgets::menu_label(ui, "Depuração");
        ui.checkbox(&mut self.view.debug,"Colisores").on_hover_text("Mostra as caixas reais usadas pela simulação. O colisor selecionado continua visível mesmo com a opção desligada.");
        ui.checkbox(
            &mut self.view.show_disabled_colliders,
            "Mostrar desativados",
        )
        .on_hover_text("Mostra caixas desativadas com traço atenuado, sem ativá-las na física.");
        if !runtime {
            if self.components_active() {
                crate::widgets::menu_label(ui, "Seleção");
                ui.checkbox(&mut self.modeling.selection.through,"Selecionar através (Shift+X)").on_hover_text("Inclui componentes ocultos em toda seleção retangular. Ctrl + arrasto ativa através somente naquele gesto, sem mudar esta preferência.");
            }
            if self.scene().kind == SceneKind::ThreeD {
                self.camera_display_menu(ui);
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
