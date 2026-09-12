mod body;
use super::*;
use oxy_core::physics3d::{CollisionGeometry, CollisionShape, generate_collider};

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
        use oxy_core::surface::{PlatformMode, VelocitySpace};
        if entity.platform.is_none() {
            return;
        }
        ui.collapsing("Plataforma móvel",|ui|{
            let Some(platform)=&mut entity.platform else{return;};
            ui.checkbox(&mut platform.enabled,"Transporte ativo");
            egui::ComboBox::from_id_salt("platform_mode").selected_text(if platform.mode==PlatformMode::Animation{"Seguir animação"}else{"Velocidade constante"}).show_ui(ui,|ui|{ui.selectable_value(&mut platform.mode,PlatformMode::Animation,"Seguir animação");ui.selectable_value(&mut platform.mode,PlatformMode::Velocity,"Velocidade constante");});
            if platform.mode==PlatformMode::Velocity {
                vector3(ui,"Velocidade (m/s)",&mut platform.velocity,0.1,false);
                egui::ComboBox::from_id_salt("platform_space").selected_text(if platform.space==VelocitySpace::Local{"Eixos locais"}else{"Eixos da cena"}).show_ui(ui,|ui|{ui.selectable_value(&mut platform.space,VelocitySpace::World,"Eixos da cena");ui.selectable_value(&mut platform.space,VelocitySpace::Local,"Eixos locais");});
            }
            ui.add(egui::DragValue::new(&mut platform.max_transport_per_step).speed(0.01).range(0.001..=10.).prefix("Salto máximo entre passos (m) ")).on_hover_text("Mudanças maiores soltam o passageiro; não geram uma velocidade de lançamento.");
            ui.small("Transporte por translação. Rotações, mudança de escala e saltos de animação soltam o apoio com diagnóstico.");
            if ui.button("Remover componente").clicked(){entity.platform=None;}
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
        if entity.camera.is_some() {
            self.camera_properties(ui, entity);
            return;
        }
        if entity.ui.is_some() {
            return;
        }
        self.character_properties(ui, entity);
        self.platform_properties(ui, entity);
        if entity.physics3d.is_none() || entity.camera.is_some() {
            return;
        }
        ui.collapsing("Colisor 3D",|ui| {
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
            ui.add_enabled(entity.character3d.is_none(),egui::Checkbox::new(&mut config.enabled,"Ativo")).on_hover_text("A cápsula é necessária ao Personagem 3D. Use Simular personagem para pausar seu controlador.");
            ui.add_enabled(entity.character3d.is_none() && entity.platform.is_none(),egui::Checkbox::new(&mut config.sensor,"Área de detecção")).on_hover_text("Detecta passagem; não bloqueia o personagem. A aparência visível é independente da colisão.");
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
                ui.checkbox(&mut config.filter.blocks_character,"Bloqueia personagem");ui.checkbox(&mut config.filter.blocks_camera,"Bloqueia câmera");
                crate::graph_ui::collision_filter_picker(ui,&mut config.filter.category,&self.state.project,"Pertence aos grupos…");
                crate::graph_ui::collision_filter_picker(ui,&mut config.filter.mask,&self.state.project,"Interage com grupos…");
                ui.collapsing("Nomes dos grupos no projeto",|ui|{
                    ui.small("Renomear organiza as escolhas; não muda vínculos nem colisões existentes.");
                    egui::ScrollArea::vertical().max_height(160.).show(ui,|ui|{for bit in 0..32_u8 {
                        let mut name=self.state.project.collision_group_label(bit);
                        ui.horizontal(|ui|{ui.label(format!("{}",bit+1)); if ui.text_edit_singleline(&mut name).changed() && !name.trim().is_empty() {self.state.project.collision_groups.insert(bit,name);}});
                    }});
                });
            });
            if let Err(error)=config.validate(){ui.colored_label(Color32::YELLOW,error);}
            if ui.add_enabled(entity.character3d.is_none() && entity.platform.is_none(),egui::Button::new("Remover componente")).on_hover_text("Remova primeiro o Personagem 3D ou a Plataforma móvel que depende desta forma.").clicked(){entity.physics3d=None;}
            if let Some(convex)=refresh {match generate_collider(entity,convex){Ok(next)=>entity.physics3d.as_mut().unwrap().shape=next.shape,Err(e)=>self.warn(e)}}
        });
    }

    fn character_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        use oxy_core::character::MovementReference;
        if entity.character3d.is_none() || entity.camera.is_some() {
            return;
        }
        ui.collapsing("Personagem 3D",|ui| {
            self.movement_body_properties(ui, entity);
            if let Some(config)=&mut entity.character3d {
                ui.collapsing("Básico", |ui| {
                ui.menu_button("Aplicar perfil de movimento",|ui|{
                    use oxy_core::character::MovementProfile;
                    for(profile,label)in[(MovementProfile::Direct,"Direto"),(MovementProfile::Parkour,"Parkour"),(MovementProfile::ChainedJumps,"Saltos encadeados")]{
                        if ui.button(label).on_hover_text("Aplica valores iniciais editáveis. Preserva ações, vínculos e geometria; pode ser desfeito.").clicked(){config.apply_profile(profile);ui.close();}
                    }
                });
                ui.checkbox(&mut config.enabled,"Simular personagem");
                ui.checkbox(&mut config.automatic_input,"Ler ações de movimento automaticamente").on_hover_text("Desative para enviar a intenção e os pedidos de pulo por nós. A simulação continua independente da entrada.");
                egui::ComboBox::from_id_salt("movement_reference").selected_text(match config.reference {MovementReference::World=>"Eixos da cena",MovementReference::Body=>"Direção do corpo",MovementReference::Camera=>"Direção da câmera"}).show_ui(ui,|ui|{ui.selectable_value(&mut config.reference,MovementReference::World,"Eixos da cena");ui.selectable_value(&mut config.reference,MovementReference::Body,"Direção do corpo");ui.selectable_value(&mut config.reference,MovementReference::Camera,"Direção da câmera");});
                ui.add(egui::DragValue::new(&mut config.gravity).speed(0.1).range(0. ..=200.).prefix("Gravidade (m/s²) "));
                });
                ui.collapsing("Movimento no chão",|ui|{
                    ui.add(egui::DragValue::new(&mut config.speed).speed(0.1).range(0. ..=100.).prefix("Velocidade (m/s) "));
                    for (value,label,max) in [(&mut config.ground_acceleration,"Aceleração (m/s²) ",1000.),(&mut config.ground_braking,"Frenagem (m/s²) ",1000.),(&mut config.ground_friction,"Atrito (por segundo) ",100.)] {ui.add(egui::DragValue::new(value).speed(0.1).range(0. ..=max).prefix(label));}
                    ui.small("A velocidade desejada não elimina impulsos externos.");
                });
                ui.collapsing("Pulo",|ui|{
                    ui.add(egui::DragValue::new(&mut config.jump_speed).speed(0.1).range(0. ..=100.).prefix("Impulso do pulo (m/s) "));
                    use oxy_core::character::JumpMode;
                    egui::ComboBox::from_id_salt("jump_mode").selected_text(if config.jump_mode==JumpMode::Manual{"Pulo manual"}else{"Pulo automático ao segurar"}).show_ui(ui,|ui|{ui.selectable_value(&mut config.jump_mode,JumpMode::Manual,"Pulo manual");ui.selectable_value(&mut config.jump_mode,JumpMode::Automatic,"Pulo automático ao segurar");}).response.on_hover_text("Manual exige novo toque. Automático repete ao encontrar chão; não adiciona um passo de simulação ao pousar.");
                    for(value,label) in [(&mut config.coyote_ms,"Tolerância após sair da borda (ms) "),(&mut config.jump_buffer_ms,"Antecipação do pulo (ms) ")] {ui.add(egui::DragValue::new(value).range(0. ..=1000.).prefix(label)).on_hover_text("Zero desativa a tolerância. O pedido é consumido uma única vez.");}
                });
                ui.collapsing("Movimento no ar",|ui|{
                    for(value,label,max)in[(&mut config.air_acceleration,"Aceleração aérea (m/s²) ",1000.),(&mut config.air_projected_limit,"Limite na direção desejada (m/s) ",1000.),(&mut config.air_resistance,"Resistência horizontal do ar ",100.),(&mut config.horizontal_limit,"Limite horizontal absoluto (0 desliga) ",1000.)]{ui.add(egui::DragValue::new(value).speed(0.1).range(0. ..=max).prefix(label));}
                    ui.small("O limite direcional só limita o acréscimo nessa direção. A velocidade perpendicular é preservada.");
                    for(value,label)in[(&mut config.jump_retention,"Conservação ao saltar "),(&mut config.landing_retention,"Conservação ao pousar sem re-salto ")]{ui.add(egui::DragValue::new(value).speed(0.01).range(0. ..=1.).prefix(label)).on_hover_text("1 conserva toda a velocidade própria; 0 elimina o embalo horizontal. Um re-salto já elegível evita a perda do pouso.");}
                    ui.add(egui::DragValue::new(&mut config.absolute_speed_limit).range(1. ..=10000.).prefix("Proteção numérica (m/s) ")).on_hover_text("Limite final separado da velocidade de caminhar e do controle aéreo.");
                });
                ui.collapsing("Corrida",|ui|{
                    ui.add(egui::DragValue::new(&mut config.sprint_speed).speed(0.1).range(0. ..=100.).prefix("Velocidade correndo (m/s) "));
                    let action=&mut config.sprint_action;
                    egui::ComboBox::from_id_salt("sprint_action").selected_text(oxy_core::input_actions::label(&self.state.project,action)).show_ui(ui,|ui|{for id in self.state.project.input_bindings.keys(){ui.selectable_value(action,id.clone(),oxy_core::input_actions::label(&self.state.project,id));}});
                });
                ui.collapsing("Agachamento/deslize",|ui|{
                    ui.add(egui::DragValue::new(&mut config.crouch_speed).speed(0.1).range(0. ..=100.).prefix("Velocidade agachado (m/s) "));

                    ui.checkbox(&mut config.crouch_toggle,"Alternar agachamento a cada toque");
                    let action=&mut config.crouch_action;
                    egui::ComboBox::from_id_salt(("character_extra_action","Agachar")).selected_text(format!("Agachar: {}",oxy_core::input_actions::label(&self.state.project,action))).show_ui(ui,|ui|{for id in self.state.project.input_bindings.keys(){ui.selectable_value(action,id.clone(),oxy_core::input_actions::label(&self.state.project,id));}});

                    ui.checkbox(&mut config.slide_enabled,"Permitir deslize ao agachar com embalo");
                    ui.add_enabled_ui(config.slide_enabled,|ui|{
                        ui.add(egui::DragValue::new(&mut config.slide_min_speed).speed(0.1).range(config.slide_exit_speed..=1000.).prefix("Velocidade mínima para iniciar (m/s) "));
                        ui.add(egui::DragValue::new(&mut config.slide_exit_speed).speed(0.1).range(0. ..=config.slide_min_speed).prefix("Encerrar abaixo de (m/s) "));
                        ui.add(egui::DragValue::new(&mut config.slide_duration).speed(0.05).range(0.01..=60.).prefix("Duração máxima (s) "));
                        for(value,label)in[(&mut config.slide_friction,"Atrito do deslize "),(&mut config.slide_control,"Controle direcional (m/s²) ")]{ui.add(egui::DragValue::new(value).speed(0.1).range(0. ..=100.).prefix(label));}
                    });
                    ui.small("Atrito e tração do piso continuam valendo. Solte o agachamento para encerrar; sob teto permanece agachado.");
                });
                ui.collapsing("Avançado",|ui|{
                    ui.add(egui::DragValue::new(&mut config.recovery_distance).speed(0.01).range(0. ..=10.).prefix("Recuperação máxima (m) ")).on_hover_text("Limita a correção quando um obstáculo entra no personagem. Se não houver espaço seguro, interrompe o movimento e informa o problema.");
                    for (value,label,max) in [(&mut config.slope_degrees,"Rampa máxima (°) ",89.),(&mut config.step_height,"Degrau máximo (m) ",2.),(&mut config.step_width,"Largura mínima (m) ",2.),(&mut config.snap,"Aderência ao chão (m) ",2.)] {ui.add(egui::DragValue::new(value).speed(0.01).range(0.001..=max).prefix(label));}
                    use oxy_core::character::InheritPlatform;
                    egui::ComboBox::from_id_salt("platform_inheritance").selected_text(match config.inherit_platform{InheritPlatform::None=>"Sem herança",InheritPlatform::Horizontal=>"Herdar velocidade horizontal",InheritPlatform::All=>"Herdar toda a velocidade"}).show_ui(ui,|ui|{ui.selectable_value(&mut config.inherit_platform,InheritPlatform::None,"Sem herança");ui.selectable_value(&mut config.inherit_platform,InheritPlatform::Horizontal,"Herdar velocidade horizontal");ui.selectable_value(&mut config.inherit_platform,InheritPlatform::All,"Herdar toda a velocidade");}).response.on_hover_text("Ao pular ou sair da plataforma, soma esta parte da velocidade do apoio uma única vez.");
                });
                ui.collapsing("Orientação do personagem",|ui|{
                    use oxy_core::character::BodyFacing;
                    egui::ComboBox::from_id_salt("character_facing").selected_text(match config.facing {BodyFacing::Movement=>"Virar para o movimento",BodyFacing::Look=>"Virar para o olhar"}).show_ui(ui,|ui|{
                        ui.selectable_value(&mut config.facing,BodyFacing::Movement,"Virar para o movimento");
                        ui.selectable_value(&mut config.facing,BodyFacing::Look,"Virar para o olhar");
                    }).response.on_hover_text("Na primeira pessoa, o corpo acompanha diretamente o olhar horizontal. Na terceira pessoa, esta opção controla o corpo sem limitar a órbita da câmera.");
                    ui.add(egui::DragValue::new(&mut config.angular_speed).speed(5.).range(0. ..=3600.).prefix("Velocidade de giro (°/s) ")).on_hover_text("Zero mantém a orientação do corpo. Não altera a velocidade de olhar com o mouse.");
                });
                if ui.button("Remover componente").clicked(){entity.character3d=None;}
            }
        });
    }
    fn camera_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        if entity.camera.is_none() || entity.camera_rig.is_none() {
            return;
        }
        ui.collapsing("Controle da câmera",|ui|{
            if entity.camera_rig.as_ref().is_some_and(|r|r.mode != oxy_core::character::CameraMode::Fixed) { ui.label("Pose controlada pelo alvo durante o jogo."); }
            if ui.button("Prévia da câmera").clicked() { self.camera_tools.preview = true; }
            let Some(rig)=&mut entity.camera_rig else {return;};
            let targets:Vec<_>=self.scene().entities.iter().filter(|e|e.id!=entity.id).map(|e|(e.id.clone(),e.name.clone())).collect();
            use oxy_core::character::CameraMode;
            ui.collapsing("Básico",|ui|{
            egui::ComboBox::from_id_salt("camera_mode").selected_text(match rig.mode {CameraMode::FirstPerson=>"Primeira pessoa",CameraMode::ThirdPerson=>"Terceira pessoa",CameraMode::Fixed=>"Câmera fixa"}).show_ui(ui,|ui|{
                ui.selectable_value(&mut rig.mode,CameraMode::FirstPerson,"Primeira pessoa");
                ui.selectable_value(&mut rig.mode,CameraMode::ThirdPerson,"Terceira pessoa");
                ui.selectable_value(&mut rig.mode,CameraMode::Fixed,"Câmera fixa");
            });

            ui.label("Alvo").on_hover_text("Objeto acompanhado por esta câmera. O vínculo independe do parentesco na Hierarquia.");
            egui::ComboBox::from_id_salt("rig_target").selected_text(rig.target.as_ref().and_then(|id|targets.iter().find(|(t,_)|t==id).map(|(_,name)|name.as_str())).unwrap_or("Escolha o personagem")).show_ui(ui,|ui|{ui.selectable_value(&mut rig.target,None,"Sem alvo");for (id,name) in &targets{ui.selectable_value(&mut rig.target,Some(id.clone()),name);}});
            });
            ui.collapsing("Primeira pessoa",|ui|{
            ui.add(egui::DragValue::new(&mut rig.eye_height).speed(0.01).range(0. ..=100.).prefix("Altura dos olhos (m) "));
            ui.add(egui::DragValue::new(&mut rig.crouched_eye_height).speed(0.01).range(0. ..=100.).prefix("Olhos agachado (m) "));
            ui.add(egui::DragValue::new(&mut rig.posture_smoothing).speed(0.01).range(0. ..=2.).prefix("Transição de postura (s) "));
            });
            ui.collapsing("Olhar",|ui|{
            ui.add(egui::DragValue::new(&mut rig.sensitivity).speed(0.005).range(0.001..=10.).prefix("Sensibilidade ")).on_hover_text("Graus por unidade relativa do mouse. Não depende da taxa de quadros nem da escala da interface.");
            ui.checkbox(&mut rig.invert_x,"Inverter olhar horizontal");ui.checkbox(&mut rig.invert_y,"Inverter olhar vertical");
            ui.add(egui::Slider::new(&mut rig.pitch_limit,1. ..=89.).text("Limite vertical (°)"));
            });
            ui.collapsing("Terceira pessoa",|ui|{
                ui.add(egui::DragValue::new(&mut rig.min_distance).speed(0.05).range(0. ..=rig.distance).prefix("Distância mínima (m) "));
                ui.add(egui::DragValue::new(&mut rig.max_distance).speed(0.05).range(rig.distance..=1000.).prefix("Distância máxima (m) "));
                ui.add(egui::DragValue::new(&mut rig.distance).speed(0.05).range(rig.min_distance..=rig.max_distance).prefix("Distância desejada (m) ")).on_hover_text("A câmera recolhe diante de paredes. A roda ajusta a distância quando não está vinculada a uma ação do jogo.");
                ui.add(egui::DragValue::new(&mut rig.shoulder).speed(0.02).range(-10. ..=10.).prefix("Deslocamento do ombro (m) "));
                ui.add(egui::DragValue::new(&mut rig.follow_height).speed(0.02).range(0. ..=100.).prefix("Altura acompanhada (m) "));
                ui.add(egui::DragValue::new(&mut rig.zoom_step).speed(0.05).range(0. ..=10.).prefix("Zoom por passo da roda (m) "));
                for(value,label) in [(&mut rig.position_smoothing,"Suavização da posição (s) "),(&mut rig.rotation_smoothing,"Suavização da rotação (s) "),(&mut rig.obstruction_return,"Retorno após obstáculo (s) "),(&mut rig.transition_seconds,"Troca de modo (s) ")] {
                    ui.add(egui::DragValue::new(value).speed(0.01).range(0. ..=5.).prefix(label)).on_hover_text("Zero aplica imediatamente. A proteção contra obstáculos tem prioridade sobre a suavização.");
                }
                for(label,action) in [("Trocar ombro",&mut rig.shoulder_action),("Alternar modo",&mut rig.mode_action)] {
                    egui::ComboBox::from_id_salt(("camera_action",label)).selected_text(format!("{label}: {}",oxy_core::input_actions::label(&self.state.project,action))).show_ui(ui,|ui|{for id in self.state.project.input_bindings.keys(){ui.selectable_value(action,id.clone(),oxy_core::input_actions::label(&self.state.project,id));}});
                }
            });
            ui.collapsing("Proteção contra obstáculos",|ui|{
                ui.add(egui::DragValue::new(&mut rig.collision_radius).speed(0.01).range(rig.collision_margin.max(0.01)+0.001..=5.).prefix("Raio mínimo (m) "));
                ui.add(egui::DragValue::new(&mut rig.collision_margin).speed(0.001).range(0.001..=rig.collision_radius-0.001).prefix("Folga de proteção (m) "));
                ui.small("O volume também protege os cantos próximos da imagem, conforme o campo de visão e a janela. Colisores podem permitir ou impedir a câmera.");
            });
            ui.checkbox(&mut rig.hide_first_person_only,"Ocultar as peças abaixo apenas em primeira pessoa");
            ui.collapsing("Ocultar peças nesta câmera",|ui|{for (id,name) in &targets {let mut hidden=rig.hidden.contains(id);if ui.checkbox(&mut hidden,name).changed(){if hidden{rig.hidden.push(id.clone());}else{rig.hidden.retain(|v|v!=id);}}}});
            ui.small("Jogar/Retomar captura o mouse. Esc ou sair de Jogo libera a entrada.");
            if ui.button("Remover componente").clicked(){entity.camera_rig=None;}
        });
    }
}
