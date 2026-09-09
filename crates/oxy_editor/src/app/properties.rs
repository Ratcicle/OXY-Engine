use super::*;

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
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Duplicar").clicked() {
                self.duplicate();
            }
            if ui.button("Agrupar").clicked() {
                self.group();
            }
            if ui.button("Excluir").clicked() {
                self.delete();
            }
        });
    }
    fn transform_multiple(&mut self, ids: &[Id], delta: glam::Mat4) {
        if let Err(error) = editing::transform_selection(self.scene_mut(), ids, delta) {
            self.log(error);
            self.notice_last(false);
        }
    }
    pub(super) fn properties(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties").default_width((ctx.content_rect().width()*0.23).clamp(140.,290.)).width_range(130.0..=(ctx.content_rect().width()*0.32).clamp(140.,460.)).resizable(true).show(ctx,|ui| {
            if self.modeling.preview.is_some(){egui::ScrollArea::vertical().show(ui,|ui|self.mesh_preview_panel(ui));return;}
            if self.snap_panel(ui){return;}
            if self.mesh_operation_active(){ui.disable();}
            ui.strong("PROPRIEDADES");ui.separator();
            egui::ScrollArea::vertical().show(ui,|ui| {
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
                let old_parent=entity.parent.clone();
                ui.label("Pai (opção avançada)").on_hover_text("Objeto cuja transformação será herdada. Também é possível arrastar na Hierarquia; a posição global será preservada.");object_picker(ui,&mut entity.parent,&objects,"entity_parent");
                let new_parent=entity.parent.clone();entity.parent=old_parent.clone();
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
                    ui.label("Cor base");color_editor(ui, &mut entity.material.color);
                    ui.label("Textura PNG");
                    let selected=entity.material.texture.as_ref().and_then(|id|textures.iter().find(|(i,_)|i==id).map(|(_,n)|n.as_str())).unwrap_or("Sem textura");
                    egui::ComboBox::from_id_salt("material_texture").selected_text(selected).show_ui(ui,|ui|{ui.selectable_value(&mut entity.material.texture,None,"Sem textura");for (id,name) in &textures{ui.selectable_value(&mut entity.material.texture,Some(id.clone()),name);}});
                    ui.checkbox(&mut entity.material.nearest,"Pixels nítidos").on_hover_text("Preserva os pixels de imagens pequenas. Desative para suavizar a textura.");
                    self.texture_controls(ui,&mut entity,false);
                });}
                ui.collapsing("Componentes",|ui| {
                    component_switch(ui,"Colisão / área",&mut entity.collider,Collider{size:entity.dimensions,..Default::default()});
                    if let Some(c)=&mut entity.collider{ui.checkbox(&mut c.enabled,"Colisor ativo");ui.checkbox(&mut c.is_trigger,"Área de detecção").on_hover_text("Área detecta entradas sem bloquear movimento. Desativada, esta caixa é um colisor sólido.");vector3(ui,"Tamanho da caixa",&mut c.size,0.05,true);c.size=c.size.map(|v|v.max(0.0001));vector3(ui,"Deslocamento",&mut c.offset,0.05,false);ui.small("Caixa alinhada aos eixos").on_hover_text("Girar a aparência não gira a caixa física. Não é uma colisão precisa da malha.");}
                    if entity.collider.is_some(){
                        if ui.button("Editar colisor (C)").clicked(){requested_tool=Some(Tool::Collider);}
                        ui.horizontal_wrapped(|ui|{
                            if ui.button("Ajustar ao objeto").clicked(){requested_fit=Some(false);}
                            if ui.button("Ajustar ao grupo/filhos").clicked(){requested_fit=Some(true);}
                        });
                        if entity.controller.is_some()&&entity.collider.as_ref().is_some_and(|c|c.is_trigger){ui.colored_label(Color32::YELLOW,"Personagem com área de detecção: esta caixa detecta entradas; um colisor sólido representa bloqueios. A configuração não foi alterada.");}
                    }
                    ui.separator();component_switch(ui,"Controlador de movimento",&mut entity.controller,Controller::default());                    if self.scene().entity(&id).is_some_and(|e|e.controller.is_none()) && let Some(c)=&entity.controller {
                        let kind=self.scene().kind;
                        oxy_core::input_actions::ensure_controller(&mut self.state.project,&c.actions,kind);
                    }
                    if entity.controller.is_some() && ui.button("Configurar ações na Lógica").clicked() {self.pause();self.tab=Tab::Logic;self.logic_ui.inputs=true;}
                    if let Some(c)=&mut entity.controller{ui.checkbox(&mut c.enabled,"Controlador ativo");ui.add(egui::DragValue::new(&mut c.speed).range(0.0..=100.0).prefix("Velocidade "));ui.add(egui::DragValue::new(&mut c.jump).range(0.0..=100.0).prefix("Pulo "));ui.add(egui::DragValue::new(&mut c.gravity).range(0.0..=200.0).prefix("Gravidade "));}
                    ui.separator();component_switch(ui,"Câmera de jogo",&mut entity.camera,Camera::default());
                    if let Some(c)=&mut entity.camera{ui.checkbox(&mut c.active,"Câmera ativa");ui.add(egui::DragValue::new(&mut c.orthographic_size).range(0.1..=500.).prefix("Meia altura 2D ")).on_hover_text("Metade da altura visível em unidades da cena.");ui.add(egui::Slider::new(&mut c.fov,10.0..=150.).text("Campo de visão 3D"));ui.small("A câmera olha para -Z local. Ative apenas a câmera desejada.");}
                });
                ui.collapsing("Atributos personalizados",|ui| {
                    ui.small("Nenhum nome de atributo impõe regras. Os nós configuram o comportamento.");
                    let mut delete=None;
                    for (name,value) in &mut entity.attributes{ui.push_id(name,|ui|{ui.horizontal(|ui|{ui.strong(name);if ui.small_button("×").clicked(){delete=Some(name.clone());}});value_editor(ui,value,&objects,name);});}
                    if let Some(name)=delete{entity.attributes.remove(&name);}
                    ui.separator();ui.add(egui::TextEdit::singleline(&mut self.attribute_name).hint_text("Nome do atributo"));
                    egui::ComboBox::from_id_salt("attrtype").selected_text(["Número","Texto","Booleano","Objeto"][self.attribute_type as usize]).show_ui(ui,|ui|{for (i,name) in ["Número","Texto","Booleano","Objeto"].into_iter().enumerate(){ui.selectable_value(&mut self.attribute_type,i as u8,name);}});
                    if ui.add_enabled(!self.attribute_name.trim().is_empty(),egui::Button::new("Adicionar atributo")).clicked(){let value=match self.attribute_type{0=>Value::Number(0.),1=>Value::Text(String::new()),2=>Value::Bool(false),_=>Value::Object(None)};entity.attributes.entry(self.attribute_name.trim().into()).or_insert(value);self.attribute_name.clear();}
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
                if new_parent!=old_parent&&self.structural_ready()&& let Err(e)=self.scene_mut().reparent(&id,new_parent,true){self.log(e);self.notice_last(false);}
                ui.separator();
                ui.horizontal(|ui|{if ui.button("Lógica").clicked(){self.tab=Tab::Logic;}
if ui.button("Animação").clicked(){self.open_animation_for(&id);}});
                if ui.button("Salvar hierarquia como modelo").clicked(){let scene_id=self.scene_id.clone();let name=self.scene().entity(&id).map(|e|e.name.clone()).unwrap_or_default();match self.state.project.save_model(&scene_id,&id,&name){Ok(_)=>self.log("Modelo salvo na biblioteca, com sua estrutura editável. Salve o projeto para gravar em disco."),Err(e)=>{self.log(e);self.notice_last(false);}}}
                ui.horizontal(|ui|{if ui.button("Duplicar").clicked(){self.duplicate();}
if ui.button("Agrupar").clicked(){self.group();}
if ui.button("Excluir").clicked(){self.delete();}});
            });
        });
    }
}
