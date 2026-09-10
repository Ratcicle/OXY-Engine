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
    pub(super) fn physics_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        ui.collapsing("Colisor 3D · formas reais",|ui| {
            if entity.physics3d.is_none() && ui.button("Adicionar colisor 3D").on_hover_text("Cria uma forma física independente da aparência. O controlador legado continua usando sua caixa antiga.").clicked() {
                entity.physics3d=Some(Collider3d{shape:CollisionShape::Box{size:entity.dimensions},..Default::default()});
            }
            if entity.has_geometry() {
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
            egui::ComboBox::from_id_salt("physics_shape").selected_text(labels[kind]).show_ui(ui,|ui|{for (i,label) in labels[..3].iter().enumerate(){ui.selectable_value(&mut kind,i,*label);}});
            if kind!=original_kind {config.shape=match kind {1=>CollisionShape::Capsule{height:1.8,radius:0.3},2=>CollisionShape::Sphere{radius:0.5},_=>CollisionShape::Box{size:entity.dimensions}};}
            let mut refresh=None;
            match &mut config.shape {
                CollisionShape::Box{size}=>vector3(ui,"Dimensões (m)",size,0.02,true),
                CollisionShape::Capsule{height,radius}=>{ui.add(egui::DragValue::new(height).range(0.02..=100.).prefix("Altura (m) "));ui.add(egui::DragValue::new(radius).range(0.01..=(*height*0.5)).prefix("Raio (m) "));},
                CollisionShape::Sphere{radius}=>{ui.add(egui::DragValue::new(radius).range(0.01..=100.).prefix("Raio (m) "));},
                CollisionShape::Convex{geometry}|CollisionShape::TriMesh{geometry}=>{
                    ui.small(format!("{} vértices · {} triângulos de origem",geometry.vertices.len(),geometry.triangles.len()));
                    if fingerprint.is_some() && fingerprint!=geometry.source_fingerprint {ui.colored_label(Color32::YELLOW,"A aparência mudou. A colisão preparada foi preservada.");}
                    if ui.button("Atualizar geometria de colisão").on_hover_text("Substitui a forma física pela geometria visual atual. Uma operação de desfazer.").clicked(){refresh=Some(original_kind==3);}
                }
            }
            vector3(ui,"Centro local (m)",&mut config.center,0.02,false);
            ui.collapsing("Filtros de colisão",|ui| {
                ui.checkbox(&mut config.filter.blocks_character,"Bloqueia personagem novo");ui.checkbox(&mut config.filter.blocks_camera,"Bloqueia câmera");
                ui.add(egui::DragValue::new(&mut config.filter.category).prefix("Categoria (bits) ")).on_hover_text("Grupo físico por bits; não é a camada de desenho 2D.");
                ui.add(egui::DragValue::new(&mut config.filter.mask).prefix("Máscara (bits) ")).on_hover_text("Categorias com que esta forma pode interagir. 1 = cenário padrão; todos os bits aceitam todas as categorias.");
            });
            if let Err(error)=config.validate(){ui.colored_label(Color32::YELLOW,error);}
            if ui.button("Remover colisor 3D").clicked(){entity.physics3d=None;}
            if let Some(convex)=refresh {match generate_collider(entity,convex){Ok(next)=>entity.physics3d.as_mut().unwrap().shape=next.shape,Err(e)=>self.warn(e)}}
        });
    }
}
