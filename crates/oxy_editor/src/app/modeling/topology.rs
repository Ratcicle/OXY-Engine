use super::*;
use oxy_core::{
    geometry::{atlas, bevel, cuts, operations},
    painting::PaintImage,
};
impl Editor {
    pub(super) fn topology_menu(&mut self, ui: &mut egui::Ui) {
        let active = self.modeling.preview.is_none() && !self.modeling.selection.ids.is_empty();
        for (label, tip, op) in [
            (
                "Corte em loop (Shift+R)",
                "Divide uma faixa de quads a partir da aresta apontada.",
                Operation::Loop,
            ),
            (
                "Bisturi (Shift+K)",
                "Trace entre bordas de faces visíveis e adjacentes.",
                Operation::Knife,
            ),
            (
                "Arredondar (Shift+B)",
                "Chanfre ou arredonde quinas convexas com largura e segmentos.",
                Operation::Bevel,
            ),
            (
                "Extrudir (Shift+E)",
                "Prolonga a região, borda ou pontos selecionados.",
                Operation::Extrude,
            ),
            (
                "Criar face ou aresta (Shift+F)",
                "Use pontos ordenados ou as bordas de uma abertura fechada.",
                Operation::Create,
            ),
            (
                "Inverter orientação (Shift+N)",
                "Inverte o lado da face sem trocar seus pixels.",
                Operation::Flip,
            ),
        ] {
            let enabled = self.operation_available(op)
                && (active || matches!(op, Operation::Loop | Operation::Knife));
            if ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_text(tip)
                .on_disabled_hover_text(
                    "Escolha componentes no modo compatível e conclua a operação em andamento.",
                )
                .clicked()
            {
                self.begin_mesh_operation(op);
                ui.close();
            }
        }
    }
    pub(super) fn topology_toolbar(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{Icon, button};
        ui.horizontal_wrapped(|ui|{
            let available=!self.mesh_operation_active();
            if self.components_active(){
                for (icon,label,tip,op) in [
                    (Icon::Loop,"Corte em loop","Corte em loop (Shift+R): divide uma faixa de quads; aponte a aresta inicial.",Operation::Loop),
                    (Icon::Bevel,"Arredondar","Arredondar (Shift+B): quinas convexas expostas, com largura e segmentos.",Operation::Bevel),
                    (Icon::Extrude,"Extrudir","Extrudir (Shift+E): prolonga faces, bordas ou pontos.",Operation::Extrude),
                    (Icon::Create,"Criar face/aresta","Criar (Shift+F): une pontos ou fecha uma borda plana.",Operation::Create),
                    (Icon::Flip,"Inverter orientação","Inverter orientação (Shift+N): troca o lado das faces mantendo seus UVs.",Operation::Flip),
                ] {
                    let enabled=available&&self.operation_available(op)&&(!self.modeling.selection.ids.is_empty()||op==Operation::Loop);
                    let response=ui.add_enabled_ui(enabled,|ui|button(ui,icon,label,tip,false,self.preferences.tool_names)).inner;
                    if response.on_disabled_hover_text("Selecione componentes no modo compatível e conclua a prévia atual.").clicked(){self.begin_mesh_operation(op);}
                }
            }else if ui.add_enabled_ui(available&&!self.selection.ids.is_empty(),|ui|button(ui,Icon::Snap,"Encaixar vértices","Encaixar vértices (Shift+V): escolha origem e destino para mover a seleção inteira ou escalar pelo eixo.",false,self.preferences.tool_names)).inner.clicked(){self.begin_snap();
            }
            if ui.add_enabled_ui(available&&self.selection.ids.len()==1,|ui|button(ui,Icon::Knife,"Bisturi","Bisturi (Shift+K): corte a superfície clicando em suas bordas.",false,self.preferences.tool_names)).inner.clicked(){self.begin_mesh_operation(Operation::Knife);}
        });
    }
    pub(super) fn operation_available(&self, op: Operation) -> bool {
        match op {
            Operation::Flip => self.modeling.selection.mode == Mode::Face,
            Operation::Bevel => matches!(self.modeling.selection.mode, Mode::Edge | Mode::Vertex),
            Operation::Loop => matches!(self.modeling.selection.mode, Mode::Edge | Mode::Face),
            _ => true,
        }
    }
    pub(super) fn update_topology_preview(&mut self) {
        let Some(mut preview) = self.modeling.preview.take() else {
            return;
        };
        let result = (|| {
            let output = match preview.operation {
                Operation::Loop => cuts::loop_cut(
                    &preview.source,
                    preview.cut_edge.ok_or("Aresta inicial ausente.")?,
                    preview.count,
                    preview.values[1],
                )
                .map(|c| {
                    preview.notes = c.notes;
                    c.output
                }),
                Operation::Knife => {
                    cuts::knife(&preview.source, &preview.path.segments).map(|c| c.output)
                }
                Operation::Bevel => bevel::apply(
                    &preview.source,
                    &preview.selection,
                    preview.values[0],
                    preview.count,
                ),
                Operation::Extrude => operations::extrude(
                    &preview.source,
                    &preview.selection,
                    Vec3::from(preview.values),
                    preview.per_face,
                ),
                Operation::Create => operations::create(&preview.source, &preview.selection),
                Operation::Flip => operations::flip(&preview.source, &preview.selection),
                _ => unreachable!(),
            }?;
            let size = preview
                .texture
                .as_ref()
                .map_or([512, 512], |(_, size)| *size);
            let expansion = if preview.allow_expansion && preview.texture.is_some() {
                2
            } else {
                1
            };
            let mut allocation = atlas::allocate(&output.mesh, &output.new_faces, size, expansion);
            preview.needs_space = false;
            if allocation.is_err() && !output.new_faces.is_empty() {
                if preview.texture.is_none() {
                    allocation = atlas::allocate(&output.mesh, &output.new_faces, size, 2);
                } else {
                    preview.needs_space = true;
                }
            }
            let allocation = allocation?;
            let texture = if let Some((original, size)) = &preview.texture {
                if allocation.expansion == 2 {
                    if preview.expanded_texture.is_none() {
                        let source = self.state.images.get(original).ok_or(
                            "Textura não carregada; cancele e abra a pintura para recarregar.",
                        )?;
                        let image = expand_image(source)?;
                        let id = new_id();
                        self.state.project.assets.push(Asset {
                            id: id.clone(),
                            name: format!(
                                "{} — atlas ampliado",
                                self.state
                                    .project
                                    .asset(original)
                                    .map_or("Pintura", |a| a.name.as_str())
                            ),
                            path: format!("assets/{id}.png"),
                            kind: AssetKind::Texture,
                            model: None,
                        });
                        self.state.images.insert(id.clone(), image);
                        self.refresh_texture(&id);
                        preview.expanded_texture = Some(id);
                    }
                    let _ = size;
                    preview.expanded_texture.clone()
                } else {
                    self.remove_preview_texture(&mut preview.expanded_texture);
                    Some(original.clone())
                }
            } else {
                None
            };
            let entity = self
                .scene_mut()
                .entity_mut(&preview.entity)
                .ok_or("A peça deixou de existir.")?;
            entity.mesh = Some(allocation.mesh);
            entity.material.texture = texture;
            self.modeling.selection = output.selection;
            Ok::<(), String>(())
        })();
        preview.error = result.err();
        self.modeling.preview = Some(preview);
    }
    fn remove_preview_texture(&mut self, id: &mut Option<Id>) {
        if let Some(id) = id.take() {
            self.state.project.assets.retain(|a| a.id != id);
            self.state.images.remove(&id);
            self.renderer.clear_texture_override(&id);
            self.game_ui.clear_texture_override(&id);
        }
    }
}
pub(super) fn expand_image(source: &PaintImage) -> Result<PaintImage, String> {
    if source.width > 4096 || source.height > 4096 {
        return Err("A ampliação excederia o limite de 8192 pixels.".into());
    }
    let mut image = PaintImage::new(source.width * 2, source.height * 2, [235, 231, 218, 255])?;
    let stride = source.width as usize * 4;
    for y in 0..source.height as usize {
        image.pixels[y * stride * 2..y * stride * 2 + stride]
            .copy_from_slice(&source.pixels[y * stride..y * stride + stride]);
        let last = source.pixel(source.width - 1, y as u32);
        image.set_pixel(source.width, y as u32, last);
    }
    for x in 0..=source.width {
        image.set_pixel(
            x,
            source.height,
            source.pixel(x.min(source.width - 1), source.height - 1),
        );
    }
    Ok(image)
}
#[test]
fn expanded_atlas_keeps_pixel_centers_and_clamped_edges() {
    let mut image = PaintImage::new(32, 32, [0, 0, 0, 255]).unwrap();
    for y in 0..32 {
        for x in 0..32 {
            image.set_pixel(x, y, [x as u8, y as u8, 100, 255]);
        }
    }
    let expanded = expand_image(&image).unwrap();
    for y in 0..=32 {
        for x in 0..=32 {
            let uv = [x as f32 / 32., y as f32 / 32.];
            assert_eq!(image.sample_uv(uv), expanded.sample_uv(uv.map(|v| v * 0.5)));
        }
    }
    assert_eq!(
        PaintImage::from_png(&expanded.to_png().unwrap()).unwrap(),
        expanded
    );
}
