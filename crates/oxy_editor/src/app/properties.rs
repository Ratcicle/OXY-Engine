use super::*;
mod components;

impl Editor {
    fn multiple_properties(&mut self, ui: &mut egui::Ui) {
        let ids = self.selection.ids.clone();
        ui.heading(format!("{} objetos selecionados", ids.len()));
        ui.label("Use os eixos para transformar o conjunto.").on_hover_text("O centro dos objetos selecionados é usado como pivô temporário. Filhos de objetos também selecionados não são transformados duas vezes.");
        let mut visible = ids
            .iter()
            .all(|id| self.scene().entity(id).is_some_and(|e| e.visible));
        if ui.checkbox(&mut visible, "Todos visíveis").changed() {
            for id in &ids {
                if let Some(e) = self.scene_mut().entity_mut(id) {
                    e.visible = visible;
                }
            }
        }
        if self.scene().kind == SceneKind::TwoD {
            let mut layer = self
                .selected
                .as_deref()
                .and_then(|id| self.scene().entity(id))
                .map_or(0, |e| e.layer);
            if ui
                .add(egui::DragValue::new(&mut layer).prefix("Camada "))
                .on_hover_text("Aplica a mesma ordem de desenho a todos os objetos selecionados.")
                .changed()
            {
                for id in &ids {
                    if let Some(e) = self.scene_mut().entity_mut(id) {
                        e.layer = layer;
                    }
                }
            }
        }
        let center = editing::selection_center(self.scene(), &ids);
        let mut position = center.to_array();
        vector3(ui, "Centro do conjunto", &mut position, 0.05, false);
        if Vec3::from(position) != center {
            self.transform_multiple(
                &ids,
                glam::Mat4::from_translation(Vec3::from(position) - center),
            );
        }
        ui.separator();
        vector3(
            ui,
            "Giro em graus",
            &mut self.selection_rotation,
            0.5,
            false,
        );
        if ui.button("Aplicar giro").clicked() {
            let [x, y, z] = self.selection_rotation.map(f32::to_radians);
            let delta = glam::Mat4::from_translation(center)
                * glam::Mat4::from_quat(glam::Quat::from_euler(glam::EulerRot::XYZ, x, y, z))
                * glam::Mat4::from_translation(-center);
            self.transform_multiple(&ids, delta);
            self.selection_rotation = [0.; 3];
        }
        ui.add(
            egui::DragValue::new(&mut self.selection_scale)
                .range(0.01..=100.)
                .speed(0.02)
                .prefix("Fator de escala "),
        );
        if ui.button("Aplicar escala").clicked() {
            self.transform_multiple(
                &ids,
                glam::Mat4::from_translation(center)
                    * glam::Mat4::from_scale(Vec3::splat(self.selection_scale))
                    * glam::Mat4::from_translation(-center),
            );
            self.selection_scale = 1.;
        }
    }
    fn transform_multiple(&mut self, ids: &[Id], delta: glam::Mat4) {
        if let Err(error) = editing::transform_selection(self.scene_mut(), ids, delta) {
            self.log(error);
            self.notice_last(false);
        }
    }
    pub(super) fn properties(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties").default_width((ctx.content_rect().width()*0.23).clamp(140.,290.)).width_range(130.0..=(ctx.content_rect().width()*0.32).clamp(140.,460.)).resizable(true).show(ctx,|ui| {
            if self.snap_panel(ui){return;}
            ui.strong("PROPRIEDADES");ui.separator();
            egui::ScrollArea::vertical().show(ui,|ui| {
                if self.modeling_active() || self.modeling.preview.is_some() {
                    self.mesh_preview_panel(ui);
                    if self.modeling.preview.is_some(){return;}
                }
                if self.mesh_operation_active(){ui.disable();}
                let Some(id)=self.selected.clone() else {
                    ui.heading("Projeto");ui.text_edit_singleline(&mut self.state.project.name);

                    return
                };
                if self.selection.ids.len() > 1 { self.multiple_properties(ui); return; }
                let Some(mut entity)=self.scene().entity(&id).cloned() else{return};
                let mut requested_pivot=None;
                let mut requested_tool=None;
                let mut requested_fit=None;
                let objects:Vec<_>=self.scene().entities.iter().map(|e|(e.id.clone(),e.name.clone())).collect();
                let textures:Vec<_>=self.state.project.assets.iter().filter(|a|a.kind==AssetKind::Texture).map(|a|(a.id.clone(),a.name.clone())).collect();
                ui.text_edit_singleline(&mut entity.name);
                ui.label(egui::RichText::new("INSTÂNCIA NA CENA").small().color(Color32::from_rgb(128,203,192)));
                if entity.model_source.is_some(){ui.small("Cópia editável de um modelo").on_hover_text("Salvar como modelo cria um recurso independente na biblioteca.");}
                ui.horizontal(|ui|{ui.checkbox(&mut entity.visible,"Visível").on_hover_text("Mostra ou oculta a aparência deste objeto e de seus filhos.");ui.label("Camada").on_hover_text("No 2D, valores maiores aparecem na frente de valores menores.");ui.add(egui::DragValue::new(&mut entity.layer));});
                ui.collapsing("Transformação",|ui| {
                    ui.horizontal(|ui|{ui.selectable_value(&mut self.view_global,false,"Local");ui.selectable_value(&mut self.view_global,true,"Global");});
                    let mut transform=if self.view_global {Transform::from_matrix(self.scene().world_matrix(&id).unwrap_or_default(),entity.transform.pivot)}else{entity.transform.clone()};
                    let before=transform.clone();
                    vector3(ui,"Posição",&mut transform.position,0.05,false);
                    let mut degrees=transform.rotation.map(f32::to_degrees);let original_degrees=degrees;vector3(ui,"Rotação °",&mut degrees,0.5,false);if degrees!=original_degrees{transform.rotation=degrees.map(f32::to_radians);}
                    vector3(ui,"Escala",&mut transform.scale,0.02,true);
                    if !self.view_global{
                        let mut pivot=transform.pivot;
                        ui.label("Pivô — ponto de giro").on_hover_text("Ponto em torno do qual a peça gira e escala. Mudar este ponto preserva a montagem na pose-base.");
                        vector3(ui,"Pivô local",&mut pivot,0.05,false);
                        if pivot!=transform.pivot {requested_pivot=Some(pivot);}
                        if ui.button("Editar pivô (P)").clicked(){requested_tool=Some(Tool::Pivot);}
                    }
                    if transform!=before {
                        if self.view_global {
                            let parent=entity.parent.as_deref().and_then(|p|self.scene().world_matrix(p).ok()).unwrap_or(glam::Mat4::IDENTITY);
                            let matrix = parent.inverse() * transform.matrix();
                            let candidate = Transform::from_matrix(matrix,entity.transform.pivot);
                            if parent.determinant().abs() < 1e-8 || !candidate.finite() || !candidate.matrix().abs_diff_eq(matrix,0.0001) {
                                self.log("A transformação global exigiria cisalhamento. Edite em Local ou ajuste a escala não uniforme do pai. A peça foi preservada.");
                                self.notice_last(false);
                            } else {
                                entity.transform = candidate;
                            }
                        } else {
                            entity.transform = transform;
                        }
                    }
                    if ui.button("Espelhar X").clicked(){entity.transform.scale[0]*=-1.;}
                });
                if entity.has_geometry(){ui.collapsing("Forma e material",|ui| {
                    ui.label(entity.primitive.map(oxy_render::labels::primitive).unwrap_or("Malha editável"));
                    if entity.mesh.is_none(){vector3(ui,"Dimensões",&mut entity.dimensions,0.05,true);entity.dimensions=entity.dimensions.map(|v|v.max(0.0001));}
                    if entity.primitive.is_some() && ui.button("Parâmetros da forma").clicked(){self.edit_primitive_parameters(&entity.id);}
                    if let Some(mesh)=&entity.mesh {
                        use oxy_core::geometry::Shading;
                        let mut shading=mesh.data().shading;
                        let response=egui::ComboBox::from_id_salt("mesh_shading").selected_text(match shading {Shading::Flat=>"Plano",Shading::Smooth=>"Suave"}).show_ui(ui,|ui|{
                            ui.selectable_value(&mut shading,Shading::Flat,"Plano");
                            ui.selectable_value(&mut shading,Shading::Smooth,"Suave");
                        });
                        response.response.on_hover_text("Sombreamento da iluminação. Suave não arredonda a silhueta nem altera os polígonos.");
                        if shading!=mesh.data().shading {match mesh.with_shading(shading){Ok(next)=>entity.mesh=Some(next),Err(e)=>self.warn(e)}}
                    }
                    ui.label("Cor base");color_editor(ui, &mut entity.material.color);
                    ui.label("Textura PNG");
                    let selected=entity.material.texture.as_ref().and_then(|id|textures.iter().find(|(i,_)|i==id).map(|(_,n)|n.as_str())).unwrap_or("Sem textura");
                    egui::ComboBox::from_id_salt("material_texture").selected_text(selected).show_ui(ui,|ui|{ui.selectable_value(&mut entity.material.texture,None,"Sem textura");for (id,name) in &textures{ui.selectable_value(&mut entity.material.texture,Some(id.clone()),name);}});
                    ui.checkbox(&mut entity.material.nearest,"Pixels nítidos").on_hover_text("Preserva os pixels de imagens pequenas. Desative para suavizar a textura.");
                    self.texture_controls(ui,&mut entity,false);
                });}
                ui.push_id(("object_components", &id), |ui| {
                    self.object_components(ui, &mut entity, &mut requested_tool, &mut requested_fit);
                });
                ui.collapsing("Atributos personalizados",|ui| {
                    ui.small("Nenhum nome de atributo impõe regras. Os nós configuram o comportamento.");
                    let mut delete=None;
                    for (name,value) in &mut entity.attributes{ui.push_id(name,|ui|{ui.horizontal(|ui|{ui.strong(name);if ui.small_button("×").clicked(){delete=Some(name.clone());}});value_editor(ui,value,&objects,&self.state.project.surfaces,name);});}
                    if let Some(name)=delete{entity.attributes.remove(&name);}
                    ui.separator();ui.add(egui::TextEdit::singleline(&mut self.attribute_name).hint_text("Nome do atributo"));
                    egui::ComboBox::from_id_salt("attrtype").selected_text(["Número","Texto","Booleano","Objeto","Vetor2","Vetor3","Superfície física"][self.attribute_type as usize]).show_ui(ui,|ui|{for (i,name) in ["Número","Texto","Booleano","Objeto","Vetor2","Vetor3","Superfície física"].into_iter().enumerate(){ui.selectable_value(&mut self.attribute_type,i as u8,name);}});
                    if ui.add_enabled(!self.attribute_name.trim().is_empty(),egui::Button::new("Adicionar atributo")).clicked(){let value=match self.attribute_type{0=>Value::Number(0.),1=>Value::Text(String::new()),2=>Value::Bool(false),4=>Value::Vector2([0.;2]),5=>Value::Vector3([0.;3]),6=>Value::Surface(None),_=>Value::Object(None)};entity.attributes.entry(self.attribute_name.trim().into()).or_insert(value);self.attribute_name.clear();}
                });
                if let Some(element)=&mut entity.ui {ui.collapsing("Interface do jogo",|ui| {
                    egui::ComboBox::from_id_salt("ui_kind").selected_text(crate::labels::ui_kind(element.kind)).show_ui(ui,|ui|{for kind in [UiKind::Text,UiKind::Image,UiKind::Button,UiKind::Bar]{ui.selectable_value(&mut element.kind,kind,crate::labels::ui_kind(kind));}});
                    egui::ComboBox::from_id_salt("ui_anchor").selected_text(crate::labels::anchor(element.anchor)).show_ui(ui,|ui|{for anchor in [UiAnchor::TopLeft,UiAnchor::TopRight,UiAnchor::BottomLeft,UiAnchor::BottomRight,UiAnchor::Center]{ui.selectable_value(&mut element.anchor,anchor,crate::labels::anchor(anchor));}});
                    ui.label("Deslocamento em pontos");ui.horizontal(|ui|{for value in &mut element.position{ui.add(egui::DragValue::new(value));}});
                    ui.label("Tamanho");ui.horizontal(|ui|{for value in &mut element.size{ui.add(egui::DragValue::new(value).range(1.0..=4000.));}});
                    ui.label("Texto ({valor} mostra o vínculo)");ui.text_edit_multiline(&mut element.text);color_editor(ui,&mut element.color);
                    ui.label("Objeto que possui o atributo").on_hover_text("A interface lê o valor deste objeto, por exemplo Vida do personagem. Sem escolha, usa o próprio objeto.");object_picker(ui,&mut element.binding_object,&objects,"ui_binding");ui.text_edit_singleline(&mut element.binding_attribute);ui.add(egui::DragValue::new(&mut element.max_value).range(0.01..=1000000.).prefix("Máximo "));
                    let label=element.texture.as_ref().and_then(|id|textures.iter().find(|(i,_)|i==id).map(|(_,n)|n.as_str())).unwrap_or("Sem imagem");
                    egui::ComboBox::from_id_salt("ui_image").selected_text(label).show_ui(ui,|ui|{ui.selectable_value(&mut element.texture,None,"Sem imagem");for (id,name) in &textures{ui.selectable_value(&mut element.texture,Some(id.clone()),name);}});
                }); self.texture_controls(ui,&mut entity,true);}
                if self.scene().entity(&id).is_some_and(|e|e.collider!=entity.collider) && entity.collider.is_some(){
                    let mut candidate=self.scene().clone();candidate.entity_mut(&id).unwrap().collider=entity.collider.clone();
                    if let Err(error)=oxy_core::spatial::collider_bounds(&candidate,&id){entity.collider=self.scene().entity(&id).unwrap().collider.clone();self.log(error);self.notice_last(false);}
                }
                if let Some(original)=self.scene_mut().entity_mut(&id){*original=entity;}
                if let Some(pivot)=requested_pivot && self.structural_ready() && let Err(e)=oxy_core::spatial::move_pivot(self.scene_mut(),&id,Vec3::from(pivot)){self.log(e);self.notice_last(false);}
                if let Some(tool)=requested_tool{self.set_spatial_tool(tool);}
                if let Some(children)=requested_fit{self.start_fit(children);}
            });
        });
    }
}
