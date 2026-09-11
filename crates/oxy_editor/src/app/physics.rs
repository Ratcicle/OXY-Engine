use super::*;
use oxy_core::physics3d::{Collider3d, CollisionGeometry, CollisionShape, generate_collider};

#[derive(PartialEq)]
struct VisualKey {
    id: Id,
    revision: Option<u64>,
    primitive: Option<Primitive>,
    dimensions: [f32; 3],
    segments: u32,
    parameters: Option<oxy_core::geometry::primitives::Parameters>,
}
#[derive(Default)]
pub(super) struct PhysicsUi {
    fingerprint: Option<(VisualKey, Option<u64>)>,
    surface_edit: Option<Id>,
}
impl PhysicsUi {
    fn fingerprint(&mut self, entity: &Entity) -> Option<u64> {
        let key = VisualKey {
            id: entity.id.clone(),
            revision: entity.mesh.as_ref().map(|m| m.revision()),
            primitive: entity.primitive,
            dimensions: entity.dimensions,
            segments: entity.segments,
            parameters: entity.primitive_parameters,
        };
        if self.fingerprint.as_ref().is_none_or(|(old, _)| old != &key) {
            self.fingerprint = Some((
                key,
                CollisionGeometry::from_entity(entity)
                    .ok()
                    .and_then(|g| g.source_fingerprint),
            ));
        }
        self.fingerprint
            .as_ref()
            .and_then(|(_, fingerprint)| *fingerprint)
    }
}

impl Editor {
    fn platform_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use oxy_core::surface::{PlatformMode, TranslationPlatform, VelocitySpace};
        ui.collapsing("Plataforma móvel",|ui|{
            if entity.platform.is_none() && ui.add_enabled(entity.character3d.is_none(),egui::Button::new("Adicionar transporte por plataforma")).clicked(){entity.platform=Some(TranslationPlatform::default());}
            let Some(platform)=&mut entity.platform else{return;};
            ui.checkbox(&mut platform.enabled,"Transporte ativo");
            egui::ComboBox::from_id_salt("platform_mode").selected_text(if platform.mode==PlatformMode::Animation{"Seguir animação"}else{"Velocidade constante"}).show_ui(ui,|ui|{ui.selectable_value(&mut platform.mode,PlatformMode::Animation,"Seguir animação");ui.selectable_value(&mut platform.mode,PlatformMode::Velocity,"Velocidade constante");});
            if platform.mode==PlatformMode::Velocity {
                vector3(ui,"Velocidade (m/s)",&mut platform.velocity,0.1,false);
                egui::ComboBox::from_id_salt("platform_space").selected_text(if platform.space==VelocitySpace::Local{"Eixos locais"}else{"Eixos da cena"}).show_ui(ui,|ui|{ui.selectable_value(&mut platform.space,VelocitySpace::World,"Eixos da cena");ui.selectable_value(&mut platform.space,VelocitySpace::Local,"Eixos locais");});
            }
            ui.add(egui::DragValue::new(&mut platform.max_transport_per_step).speed(0.01).range(0.001..=10.).prefix("Salto máximo entre passos (m) ")).on_hover_text("Mudanças maiores soltam o passageiro; não geram uma velocidade de lançamento.");
            ui.small("Transporte por translação. Rotações, mudança de escala e saltos de animação soltam o apoio com diagnóstico.");
            if ui.button("Remover transporte da plataforma").clicked(){entity.platform=None;}
        });
    }
    fn surface_properties(&mut self, ui: &mut egui::Ui, link: &mut Option<Id>) {
        use oxy_core::surface::{self, SurfaceMaterial, SurfacePreset, VelocitySpace};
        ui.collapsing("Superfície física",|ui|{
            egui::ComboBox::from_id_salt("surface_link").selected_text(link.as_ref().and_then(|id|self.state.project.surfaces.iter().find(|s|s.id==*id)).map_or("Padrão sem vínculo",|s|s.name.as_str())).show_ui(ui,|ui|{
                ui.selectable_value(link,None,"Padrão sem vínculo");
                for surface in &self.state.project.surfaces {ui.selectable_value(link,Some(surface.id.clone()),&surface.name);}
            });
            ui.menu_button("Nova superfície",|ui|{for(preset,label)in[(SurfacePreset::Common,"Comum"),(SurfacePreset::Ice,"Gelo"),(SurfacePreset::Mud,"Lama"),(SurfacePreset::Conveyor,"Esteira")]{if ui.button(label).clicked(){let surface=SurfaceMaterial::preset(preset);*link=Some(surface.id.clone());self.physics_ui.surface_edit=Some(surface.id.clone());self.state.project.surfaces.push(surface);ui.close();}}});
            if let Some(id)=link.clone(){
                let uses=surface::references(&self.state.project,&id);
                ui.small(format!("Material físico compartilhado · {} vínculo(s)",uses.len().max(1))).on_hover_text(uses.join("\n"));
                if ui.button("Criar cópia independente da superfície").clicked(){match surface::duplicate(&mut self.state.project,&id){Ok(copy)=>{self.physics_ui.surface_edit=Some(copy.clone());*link=Some(copy);},Err(e)=>self.warn(e)}}
                if ui.button("Editar superfície original").on_hover_text("Afeta todos os objetos vinculados, listados acima. Use uma cópia para mudar apenas esta peça.").clicked(){self.physics_ui.surface_edit=Some(id);}
                if ui.button("Remover vínculo físico").clicked(){*link=None;}
            }
            if let Some(id)=self.physics_ui.surface_edit.clone(){
                let uses=surface::references(&self.state.project,&id);
                if let Some(surface)=self.state.project.surfaces.iter_mut().find(|s|s.id==id){
                    ui.separator();ui.label("Editando material físico original");
                    ui.text_edit_singleline(&mut surface.name);
                    for(value,label)in[(&mut surface.friction,"Atrito · multiplicador "),(&mut surface.traction,"Tração · multiplicador "),(&mut surface.speed_multiplier,"Velocidade desejada · multiplicador ")]{ui.add(egui::DragValue::new(value).speed(0.01).range(0. ..=100.).prefix(label));}
                    vector3(ui,"Velocidade da esteira (m/s)",&mut surface.conveyor,0.1,false);
                    egui::ComboBox::from_id_salt("conveyor_space").selected_text(if surface.conveyor_space==VelocitySpace::Local{"Esteira nos eixos locais"}else{"Esteira nos eixos da cena"}).show_ui(ui,|ui|{ui.selectable_value(&mut surface.conveyor_space,VelocitySpace::Local,"Esteira nos eixos locais");ui.selectable_value(&mut surface.conveyor_space,VelocitySpace::World,"Esteira nos eixos da cena");});
                }
                let can_delete=uses.is_empty() && link.as_deref()!=Some(id.as_str());
                if ui.add_enabled(can_delete,egui::Button::new("Excluir superfície do projeto")).on_hover_text(if can_delete{"Remove apenas este material físico sem referências.".into()}else{format!("Remova os vínculos antes de excluir:\n{}",uses.join("\n"))}).clicked(){match surface::remove(&mut self.state.project,&id){Ok(())=>self.physics_ui.surface_edit=None,Err(e)=>self.warn(e)}}
                if ui.button("Concluir edição da superfície").clicked(){self.physics_ui.surface_edit=None;}
            }
        });
    }
    pub(super) fn physics_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        self.character_properties(ui, entity);
        self.platform_properties(ui, entity);
        ui.collapsing("Colisor 3D · formas reais",|ui| {
            if entity.physics3d.is_none() && ui.button("Adicionar colisor 3D").on_hover_text("Cria uma forma física independente da aparência. O controlador legado continua usando sua caixa antiga.").clicked() {
                entity.physics3d=Some(Collider3d{shape:CollisionShape::Box{size:entity.dimensions},..Default::default()});
            }
            if entity.has_geometry() && entity.character3d.is_none() {
                ui.horizontal_wrapped(|ui| {
                    for (convex,label) in [(true,"Gerar colisor convexo"),(false,"Usar malha estática de colisão")] {
                        if ui.button(label).on_hover_text(if convex{"Envolve a peça com um volume convexo: aberturas e concavidades serão fechadas. A sobreposição mostra o volume resultante."}else{"Preserva triângulos e aberturas da peça. Não cria um corpo dinâmico."}).clicked() {
                            match generate_collider(entity,convex) {
                                Ok(generated)=>{if let Some(current)=&mut entity.physics3d {current.shape=generated.shape;}else{entity.physics3d=Some(generated);}},
                                Err(error)=>self.warn(error),
                            }
                        }
                    }
                });
            }
            let fingerprint=if entity.physics3d.as_ref().is_some_and(|c|matches!(c.shape,CollisionShape::Convex{..}|CollisionShape::TriMesh{..})) {self.physics_ui.fingerprint(entity)}else{None};
            let Some(config)=&mut entity.physics3d else {return;};
            ui.checkbox(&mut config.enabled,"Ativo");
            ui.checkbox(&mut config.sensor,"Área de detecção").on_hover_text("Detecta passagem; não bloqueia o personagem. A aparência visível é independente da colisão.");
            let original_kind=match config.shape {CollisionShape::Box{..}=>0,CollisionShape::Capsule{..}=>1,CollisionShape::Sphere{..}=>2,CollisionShape::Convex{..}=>3,CollisionShape::TriMesh{..}=>4};
            let mut kind=original_kind;
            let labels=["Caixa orientada","Cápsula vertical","Esfera","Convexo preparado","Malha estática preparada"];
            ui.add_enabled_ui(entity.character3d.is_none(),|ui|{egui::ComboBox::from_id_salt("physics_shape").selected_text(labels[kind]).show_ui(ui,|ui|{for (i,label) in labels[..3].iter().enumerate(){ui.selectable_value(&mut kind,i,*label);}});});
            if kind!=original_kind {config.shape=match kind {1=>CollisionShape::Capsule{height:1.8,radius:0.3},2=>CollisionShape::Sphere{radius:0.5},_=>CollisionShape::Box{size:entity.dimensions}};}
            let mut refresh=None;
            match &mut config.shape {
                CollisionShape::Box{size}=>vector3(ui,"Dimensões (m)",size,0.02,true),
                CollisionShape::Capsule{height,radius}=>{
                    let changed=ui.add(egui::DragValue::new(height).speed(0.01).range((*radius*2.)..=100.).prefix("Altura (m) ")).changed();
                    let radius_changed=ui.add(egui::DragValue::new(radius).speed(0.01).range(0.01..=(*height*0.5)).prefix("Raio (m) ")).changed();
                    if let Some(character)=&mut entity.character3d && (changed||radius_changed) {config.center=[0.,*height*0.5,0.];character.crouch_height=character.crouch_height.clamp(*radius*2.,*height);}
                },
                CollisionShape::Sphere{radius}=>{ui.add(egui::DragValue::new(radius).range(0.01..=100.).prefix("Raio (m) "));},
                CollisionShape::Convex{geometry}|CollisionShape::TriMesh{geometry}=>{
                    ui.small(format!("{} vértices · {} triângulos de origem",geometry.vertices.len(),geometry.triangles.len()));
                    if fingerprint.is_some() && fingerprint!=geometry.source_fingerprint {ui.colored_label(Color32::YELLOW,"A aparência mudou. A colisão preparada foi preservada.");}
                    if ui.button("Atualizar geometria de colisão").on_hover_text("Substitui a forma física pela geometria visual atual. Uma operação de desfazer.").clicked(){refresh=Some(original_kind==3);}
                }
            }
            ui.add_enabled_ui(entity.character3d.is_none(),|ui|vector3(ui,"Centro local (m)",&mut config.center,0.02,false));
            if entity.character3d.is_some(){ui.small("Origem nos pés; o centro acompanha metade da altura.");}
            self.surface_properties(ui,&mut config.surface);
            ui.collapsing("Filtros de colisão",|ui| {
                ui.checkbox(&mut config.filter.blocks_character,"Bloqueia personagem novo");ui.checkbox(&mut config.filter.blocks_camera,"Bloqueia câmera");
                ui.add(egui::DragValue::new(&mut config.filter.category).prefix("Categoria (bits) ")).on_hover_text("Grupo físico por bits; não é a camada de desenho 2D.");
                ui.add(egui::DragValue::new(&mut config.filter.mask).prefix("Máscara (bits) ")).on_hover_text("Categorias com que esta forma pode interagir. 1 = cenário padrão; todos os bits aceitam todas as categorias.");
            });
            if let Err(error)=config.validate(){ui.colored_label(Color32::YELLOW,error);}
            if ui.add_enabled(entity.character3d.is_none(),egui::Button::new("Remover colisor 3D")).on_hover_text("Remova primeiro o controlador que depende desta cápsula.").clicked(){entity.physics3d=None;}
            if let Some(convex)=refresh {match generate_collider(entity,convex){Ok(next)=>entity.physics3d.as_mut().unwrap().shape=next.shape,Err(e)=>self.warn(e)}}
        });
    }

    fn character_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use oxy_core::character::{CameraRig, CharacterConfig, MovementReference};
        ui.collapsing("Personagem 3D",|ui| {
            if entity.character3d.is_none() {
                let compatible=entity.controller.is_none() && entity.collider.is_none() && entity.physics3d.is_none();
                if ui.add_enabled(compatible,egui::Button::new("Adicionar personagem 3D")).on_hover_text("Cria o controlador e sua cápsula com origem nos pés. Para converter um objeto legado, remova explicitamente seus componentes antigos primeiro; nenhuma configuração é convertida silenciosamente.").clicked() {
                    let config=CharacterConfig::default();
                    oxy_core::input_actions::ensure_character(&mut self.state.project,&config);
                    entity.character3d=Some(config);
                    entity.physics3d=Some(Collider3d{shape:CollisionShape::Capsule{height:1.8,radius:0.3},center:[0.,0.9,0.],..Default::default()});
                }
            }
            if let Some(config)=&mut entity.character3d {
                ui.checkbox(&mut config.enabled,"Simular personagem");
                ui.checkbox(&mut config.automatic_input,"Ler ações de movimento automaticamente").on_hover_text("Desative para enviar a intenção e os pedidos de pulo por nós. A simulação continua independente da entrada.");
                egui::ComboBox::from_id_salt("movement_reference").selected_text(match config.reference {MovementReference::World=>"Eixos da cena",MovementReference::Body=>"Direção do corpo",MovementReference::Camera=>"Direção da câmera"}).show_ui(ui,|ui|{ui.selectable_value(&mut config.reference,MovementReference::World,"Eixos da cena");ui.selectable_value(&mut config.reference,MovementReference::Body,"Direção do corpo");ui.selectable_value(&mut config.reference,MovementReference::Camera,"Direção da câmera");});
                for (value,label,max) in [(&mut config.speed,"Velocidade (m/s) ",100.),(&mut config.gravity,"Gravidade (m/s²) ",200.),(&mut config.jump_speed,"Impulso do pulo (m/s) ",100.)] {ui.add(egui::DragValue::new(value).speed(0.1).range(0. ..=max).prefix(label));}
                ui.collapsing("Resposta no chão",|ui|{
                    for (value,label,max) in [(&mut config.ground_acceleration,"Aceleração (m/s²) ",1000.),(&mut config.ground_braking,"Frenagem (m/s²) ",1000.),(&mut config.ground_friction,"Atrito (por segundo) ",100.),(&mut config.sprint_speed,"Velocidade correndo (m/s) ",100.),(&mut config.crouch_speed,"Velocidade agachado (m/s) ",100.)] {ui.add(egui::DragValue::new(value).speed(0.1).range(0. ..=max).prefix(label));}
                    ui.small("A velocidade desejada não elimina impulsos externos.");
                });
                ui.collapsing("Pulo e postura",|ui|{
                    for(value,label) in [(&mut config.coyote_ms,"Tolerância após sair da borda (ms) "),(&mut config.jump_buffer_ms,"Antecipação do pulo (ms) ")] {ui.add(egui::DragValue::new(value).range(0. ..=1000.).prefix(label)).on_hover_text("Zero desativa a tolerância. O pedido é consumido uma única vez.");}
                    let (minimum,maximum)=entity.physics3d.as_ref().and_then(|c|if let CollisionShape::Capsule{height,radius}=c.shape{Some((radius*2.,height))}else{None}).unwrap_or((0.6,1.8));
                    ui.add(egui::DragValue::new(&mut config.crouch_height).speed(0.01).range(minimum..=maximum).prefix("Altura agachado (m) ")).on_hover_text("Os pés ficam no lugar. Só volta a ficar em pé se a cápsula inteira couber.");
                    ui.checkbox(&mut config.crouch_toggle,"Alternar agachamento a cada toque");
                    for (label,action) in [("Correr",&mut config.sprint_action),("Agachar",&mut config.crouch_action)] {
                        egui::ComboBox::from_id_salt(("character_extra_action",label)).selected_text(format!("{label}: {}",oxy_core::input_actions::label(&self.state.project,action))).show_ui(ui,|ui|{for id in self.state.project.input_bindings.keys(){ui.selectable_value(action,id.clone(),oxy_core::input_actions::label(&self.state.project,id));}});
                    }
                });
                ui.collapsing("Contato com o cenário",|ui|{
                    ui.add(egui::DragValue::new(&mut config.recovery_distance).speed(0.01).range(0. ..=10.).prefix("Recuperação máxima (m) ")).on_hover_text("Limita a correção quando um obstáculo entra no personagem. Se não houver espaço seguro, interrompe o movimento e informa o problema.");
                    for (value,label,max) in [(&mut config.slope_degrees,"Rampa máxima (°) ",89.),(&mut config.step_height,"Degrau máximo (m) ",2.),(&mut config.step_width,"Largura mínima (m) ",2.),(&mut config.snap,"Aderência ao chão (m) ",2.)] {ui.add(egui::DragValue::new(value).speed(0.01).range(0.001..=max).prefix(label));}
                    use oxy_core::character::InheritPlatform;
                    egui::ComboBox::from_id_salt("platform_inheritance").selected_text(match config.inherit_platform{InheritPlatform::None=>"Sem herança",InheritPlatform::Horizontal=>"Herdar velocidade horizontal",InheritPlatform::All=>"Herdar toda a velocidade"}).show_ui(ui,|ui|{ui.selectable_value(&mut config.inherit_platform,InheritPlatform::None,"Sem herança");ui.selectable_value(&mut config.inherit_platform,InheritPlatform::Horizontal,"Herdar velocidade horizontal");ui.selectable_value(&mut config.inherit_platform,InheritPlatform::All,"Herdar toda a velocidade");}).response.on_hover_text("Ao pular ou sair da plataforma, soma esta parte da velocidade do apoio uma única vez.");
                });
                if ui.button("Remover controlador 3D").clicked(){entity.character3d=None;}
            }
        });
        ui.collapsing("Câmera de personagem",|ui|{
            if entity.camera_rig.is_none() && ui.button("Adicionar câmera em primeira pessoa").clicked() {
                entity.camera.get_or_insert_with(Camera::default);
                entity.camera_rig=Some(CameraRig::default());
            }
            let Some(rig)=&mut entity.camera_rig else {return;};
            let targets:Vec<_>=self.scene().entities.iter().filter(|e|e.id!=entity.id).map(|e|(e.id.clone(),e.name.clone())).collect();
            egui::ComboBox::from_id_salt("rig_target").selected_text(rig.target.as_ref().and_then(|id|targets.iter().find(|(t,_)|t==id).map(|(_,name)|name.as_str())).unwrap_or("Escolha o personagem")).show_ui(ui,|ui|{ui.selectable_value(&mut rig.target,None,"Sem alvo");for (id,name) in &targets{ui.selectable_value(&mut rig.target,Some(id.clone()),name);}});
            ui.add(egui::DragValue::new(&mut rig.eye_height).speed(0.01).range(0. ..=100.).prefix("Altura dos olhos (m) "));
            ui.add(egui::DragValue::new(&mut rig.crouched_eye_height).speed(0.01).range(0. ..=100.).prefix("Olhos agachado (m) "));
            ui.add(egui::DragValue::new(&mut rig.posture_smoothing).speed(0.01).range(0. ..=2.).prefix("Transição de postura (s) "));
            ui.add(egui::DragValue::new(&mut rig.sensitivity).speed(0.005).range(0.001..=10.).prefix("Sensibilidade ")).on_hover_text("Graus por unidade relativa do mouse. Não depende da taxa de quadros nem da escala da interface.");
            ui.checkbox(&mut rig.invert_x,"Inverter olhar horizontal");ui.checkbox(&mut rig.invert_y,"Inverter olhar vertical");
            ui.add(egui::Slider::new(&mut rig.pitch_limit,1. ..=89.).text("Limite vertical (°)"));
            ui.collapsing("Ocultar peças nesta câmera",|ui|{for (id,name) in &targets {let mut hidden=rig.hidden.contains(id);if ui.checkbox(&mut hidden,name).changed(){if hidden{rig.hidden.push(id.clone());}else{rig.hidden.retain(|v|v!=id);}}}});
            ui.small("Jogar/Retomar captura o mouse. Esc ou sair de Jogo libera a entrada.");
            if ui.button("Remover controle da câmera").clicked(){entity.camera_rig=None;}
        });
    }
}
