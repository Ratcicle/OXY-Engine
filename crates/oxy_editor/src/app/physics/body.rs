use super::*;
use oxy_core::movement_body::MovementBody;

fn dimensions(ui: &mut egui::Ui, shape: &mut CollisionShape) {
    match shape {
        CollisionShape::Capsule { height, radius } => {
            ui.add(
                egui::DragValue::new(height)
                    .speed(0.01)
                    .range((*radius * 2.)..=100.)
                    .prefix("Altura (m) "),
            );
            ui.add(
                egui::DragValue::new(radius)
                    .speed(0.01)
                    .range(0.01..=(*height * 0.5))
                    .prefix("Raio (m) "),
            );
        }
        CollisionShape::Box { size } => {
            for (v, label) in
                size.iter_mut()
                    .zip(["Largura (m) ", "Altura (m) ", "Profundidade (m) "])
            {
                ui.add(
                    egui::DragValue::new(v)
                        .speed(0.01)
                        .range(0.01..=100.)
                        .prefix(label),
                );
            }
        }
        CollisionShape::Sphere { radius } => {
            ui.add(
                egui::DragValue::new(radius)
                    .speed(0.01)
                    .range(0.01..=100.)
                    .prefix("Raio (m) "),
            );
        }
        CollisionShape::Convex { geometry } => {
            ui.label(format!("Convexo · {} vértices de origem", geometry.vertices.len())).on_hover_text("O volume é a envoltória convexa destes pontos. A base é apoiada na origem dos pés.");
        }
        CollisionShape::TriMesh { .. } => {
            ui.colored_label(Color32::YELLOW, "Malha de triângulos móvel não suportada");
        }
    }
}
impl Editor {
    pub(super) fn movement_body_properties(&mut self, ui: &mut egui::Ui, entity: &mut Entity) {
        let Some(original) = &entity.character3d else {
            return;
        };
        let mut config = original.clone();
        let mut error = None;
        ui.collapsing("Corpo de movimento", |ui| {
            ui.small("Colisão independente da aparência. Origem: pés.");
            ui.add_enabled(!config.enabled, egui::Checkbox::new(&mut config.body.enabled, "Corpo ativo")).on_hover_text("Para desativar o corpo, desative primeiro o Personagem 3D. Um personagem ativo precisa de colisão.");
            let old = match config.body.standing { CollisionShape::Capsule { .. } => 0, CollisionShape::Box { .. } => 1, CollisionShape::Sphere { .. } => 2, _ => 3 };
            let mut kind = old;
            egui::ComboBox::from_id_salt("movement_body_kind").selected_text(["Cápsula", "Caixa", "Esfera", "Convexo personalizado"][old]).show_ui(ui, |ui| {
                for (i, label) in ["Cápsula", "Caixa", "Esfera"].iter().enumerate() { ui.selectable_value(&mut kind, i, *label); }
            });
            if kind != old {
                config.body.standing = match kind { 1 => CollisionShape::Box { size: [0.6, 1.8, 0.6] }, 2 => CollisionShape::Sphere { radius: 0.5 }, _ => MovementBody::default().standing };
                config.body.crouched = None;
                config.crouch_height = 1.;
            }
            ui.menu_button("Gerar convexo de uma peça…", |ui| {
                for source in self.scene().entities.iter().filter(|e| e.has_geometry()) {
                    if ui.button(&source.name).on_hover_text("Gera explicitamente uma envoltória convexa da geometria local. Não copia o material nem altera a peça de origem.").clicked() {
                        match MovementBody::convex_from(source) { Ok(shape) => { config.body.standing = shape; config.body.crouched = None; }, Err(e) => error = Some(e) }
                        ui.close();
                    }
                }
            });
            ui.label("Em pé");
            ui.push_id("standing_body", |ui| dimensions(ui, &mut config.body.standing));
            match &config.body.standing {
                CollisionShape::Capsule { height, radius } => { config.crouch_height = config.crouch_height.clamp(radius * 2., *height); ui.add(egui::DragValue::new(&mut config.crouch_height).speed(0.01).range(radius * 2. ..=*height).prefix("Altura agachada (m) ")); }
                CollisionShape::Box { size } => { config.crouch_height = config.crouch_height.min(size[1]); ui.add(egui::DragValue::new(&mut config.crouch_height).speed(0.01).range(0.01..=size[1]).prefix("Altura agachada (m) ")); }
                CollisionShape::Sphere { .. } => {
                    ui.small("Agachar muda o estado e a velocidade. A esfera mantém seu raio, salvo uma forma alternativa explícita.");
                    let mut alternate = config.body.crouched.is_some();
                    if ui.checkbox(&mut alternate, "Usar esfera agachada alternativa").changed() { config.body.crouched = alternate.then(|| config.body.standing.clone()); }
                }
                CollisionShape::Convex { .. } => {
                    ui.small("Sem convexo alternativo, agachar preserva a geometria.");
                    ui.menu_button("Gerar convexo agachado de…", |ui| {
                        for source in self.scene().entities.iter().filter(|e| e.has_geometry()) {
                            if ui.button(&source.name).clicked() { match MovementBody::convex_from(source) { Ok(shape) => config.body.crouched = Some(shape), Err(e) => error = Some(e) } ui.close(); }
                        }
                    });
                    if config.body.crouched.is_some() && ui.button("Remover forma agachada alternativa").clicked() { config.body.crouched = None; }
                }
                _ => {}
            }
            if let Some(shape) = &mut config.body.crouched { ui.label("Agachado"); ui.push_id("crouched_body", |ui| dimensions(ui, shape)); }
            ui.add(egui::DragValue::new(&mut config.margin).speed(0.001).range(0.0001..=1.).prefix("Margem de segurança (m) ")).on_hover_text("Pequena distância mantida entre o corpo e os obstáculos.");
            ui.collapsing("Filtros do corpo", |ui| {
                ui.add(egui::DragValue::new(&mut config.body.filter.category).prefix("Categoria "));
                ui.add(egui::DragValue::new(&mut config.body.filter.mask).prefix("Máscara "));
                ui.checkbox(&mut config.body.filter.blocks_character, "Bloqueia outros personagens");
                ui.checkbox(&mut config.body.filter.blocks_camera, "Bloqueia câmeras");
            });
            self.surface_properties(ui, &mut config.body.surface);
        });
        if let Some(error) = error {
            self.warn(error);
        }
        if entity.character3d.as_ref() != Some(&config) {
            match config.motion(entity, 1.) {
                Ok(_) => entity.character3d = Some(config),
                Err(e) => self.warn(format!("Corpo anterior preservado: {e}")),
            }
        }
    }
}
