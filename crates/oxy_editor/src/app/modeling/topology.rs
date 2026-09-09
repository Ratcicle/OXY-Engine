use super::*;
use oxy_core::{
    geometry::{atlas, bevel, cuts, inset, operations},
    painting::PaintImage,
};
impl Editor {
    pub(super) fn topology_menu(&mut self, ui: &mut egui::Ui) {
        let available = !self.mesh_operation_active();
        let active = !self.modeling.selection.ids.is_empty();
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
                "Criar borda interna (Shift+U)",
                "Cria uma moldura no plano original e seleciona a região interna.",
                Operation::Inset,
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
            let enabled = available
                && self.operation_available(op)
                && (active || matches!(op, Operation::Loop | Operation::Knife));
            if ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_text(tip)
                .on_disabled_hover_text(
                    "Escolha componentes no modo compatível; termine ou cancele o arrasto atual.",
                )
                .clicked()
            {
                self.begin_mesh_operation(op);
                ui.close();
            }
        }
    }
    /// Inline tools: caller owns the single row and the always-accessible overflow menu.
    pub(super) fn topology_toolbar(&mut self, ui: &mut egui::Ui) {
        use crate::icons::{Icon, button, width};
        let names = self.preferences.tool_names;
        let reserve = width(ui, "Malha", names) + ui.spacing().item_spacing.x;
        let available = !self.mesh_operation_active();
        if self.components_active() {
            for (icon, label, tip, op) in [
                (
                    Icon::Loop,
                    "Corte em loop",
                    "Corte em loop (Shift+R): clique na faixa de quads para cortar.",
                    Operation::Loop,
                ),
                (
                    Icon::Bevel,
                    "Arredondar",
                    "Arredondar (Shift+B): arraste a largura e solte para aplicar.",
                    Operation::Bevel,
                ),
                (
                    Icon::Extrude,
                    "Extrudir",
                    "Extrudir (Shift+E): arraste a distância; tamanho interno menor que 100% deixa uma moldura.",
                    Operation::Extrude,
                ),
                (
                    Icon::Inset,
                    "Criar borda interna",
                    "Criar borda interna (Shift+U): reduz o contorno interno no plano original.",
                    Operation::Inset,
                ),
                (
                    Icon::Create,
                    "Criar face/aresta",
                    "Criar (Shift+F): une pontos ordenados ou fecha uma borda plana.",
                    Operation::Create,
                ),
                (
                    Icon::Flip,
                    "Inverter orientação",
                    "Inverter orientação (Shift+N): troca o lado da face mantendo os pixels.",
                    Operation::Flip,
                ),
                (
                    Icon::Knife,
                    "Bisturi",
                    "Bisturi (Shift+K): trace o caminho; duplo clique ou Enter conclui.",
                    Operation::Knife,
                ),
            ] {
                if !self.operation_available(op) {
                    continue;
                }
                if ui.available_width() < width(ui, label, names) + reserve {
                    continue;
                }
                let enabled = available
                    && (!self.modeling.selection.ids.is_empty()
                        || matches!(op, Operation::Loop | Operation::Knife));
                let selected = self.modeling.preview.as_ref().is_some_and(|p| p.operation == op);
                let response = ui
                    .add_enabled_ui(enabled, |ui| button(ui, icon, label, tip, selected, names))
                    .inner;
                if response
                    .on_disabled_hover_text(
                        "Selecione componentes compatíveis; termine ou cancele o arrasto atual.",
                    )
                    .clicked()
                {
                    self.begin_mesh_operation(op);
                }
            }
        } else if ui.available_width() >= width(ui, "Encaixar vértices", names) + reserve
            && ui.add_enabled_ui(available&&!self.selection.ids.is_empty(),|ui|button(ui,Icon::Snap,"Encaixar vértices","Encaixar vértices (Shift+V): selecione origem e destino para mover a peça ou escalar pelo eixo.",false,names)).inner.clicked() {
            self.begin_snap();
        }
    }
    pub(super) fn operation_available(&self, op: Operation) -> bool {
        match op {
            Operation::Flip | Operation::Inset => self.modeling.selection.mode == Mode::Face,
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
                Operation::Extrude | Operation::Inset => {
                    let delta = preview.displacement(None);
                    if preview.selection.mode != Mode::Face {
                        operations::extrude(&preview.source, &preview.selection, delta, false)
                    } else if preview.per_face {
                        let directions = preview
                            .selection
                            .ids
                            .iter()
                            .map(|&id| (id, preview.displacement(Some(id))))
                            .collect();
                        inset::apply_directions(
                            &preview.source,
                            &preview.selection,
                            preview.inner_size,
                            &directions,
                            true,
                        )
                    } else {
                        inset::apply(
                            &preview.source,
                            &preview.selection,
                            preview.inner_size,
                            delta,
                            false,
                        )
                    }
                }
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
                    if preview
                        .expanded_texture
                        .as_deref()
                        .is_none_or(|id| self.state.project.asset(id).is_none())
                    {
                        let source = self.state.images.get(original).ok_or(
                            "Textura não carregada; cancele e abra a pintura para recarregar.",
                        )?;
                        let image = expand_image(source)?;
                        let id = preview.expanded_texture.clone().unwrap_or_else(new_id);
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
                    self.remove_preview_texture(&mut preview.expanded_texture.clone());
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
            entity.primitive = None;
            entity.primitive_parameters = None;
            entity.dimensions = [1.; 3];
            entity.material.texture = texture;
            self.modeling.selection = output.selection;
            Ok::<(), String>(())
        })();
        preview.error = result.err();
        self.modeling.preview = Some(preview);
    }
    pub(super) fn remove_preview_texture(&mut self, id: &mut Option<Id>) {
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
