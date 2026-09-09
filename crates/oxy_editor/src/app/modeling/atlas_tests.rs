//! Runs against the existing native QA editor and its real WGPU texture caches.
use super::*;

impl Editor {
    pub(crate) fn qa_atlas_amendment(&mut self) -> Result<(), String> {
        let mut project = editing::blank_project("Regressão de atlas", SceneKind::ThreeD)?;
        let mut entity = Entity::new("Peça pintada", Some(Primitive::Cube));
        let id = entity.id.clone();
        let texture = new_id();
        entity.material.texture = Some(texture.clone());
        let mesh = primitives::for_entity(&entity)?;
        let prepared = mesh.prepared();
        let face = prepared
            .face_normals
            .iter()
            .position(|n| n.z > 0.9)
            .map(|i| mesh.data().faces[i].id)
            .ok_or("Face frontal ausente")?;
        project.scenes[0].entities.push(entity);
        project.assets.push(Asset {
            id: texture.clone(),
            name: "Pintura original".into(),
            path: format!("assets/{texture}.png"),
            kind: AssetKind::Texture,
            model: None,
        });
        let mut pixels = PaintImage::new(256, 256, [41, 97, 173, 255])?;
        pixels.set_pixel(31, 73, [211, 19, 64, 255]);
        let mut images = TextureCache::default();
        images.configure(&self.root(), &project);
        images.insert(texture.clone(), pixels.clone());
        let previous = std::mem::replace(&mut self.state, Snapshot { project, images });
        let history = std::mem::take(&mut self.history);
        let modeling = std::mem::take(&mut self.modeling);
        let scene = std::mem::replace(&mut self.scene_id, self.state.project.start_scene.clone());
        let selected = self.selected.replace(id.clone());
        let selection = std::mem::take(&mut self.selection);
        self.selection.single(Some(id.clone()));
        self.modeling.selection = Components {
            mode: Mode::Face,
            ids: vec![face],
            through: false,
        };
        self.sync_textures();
        let result = self.check_atlas_amendment(&id, &texture, &pixels);
        self.state = previous;
        self.history = history;
        self.modeling = modeling;
        self.scene_id = scene;
        self.selected = selected;
        self.selection = selection;
        self.sync_textures();
        result
    }

    fn check_atlas_amendment(
        &mut self,
        id: &str,
        texture: &str,
        pixels: &PaintImage,
    ) -> Result<(), String> {
        let base = self.state.clone();
        self.begin_mesh_operation(Operation::Extrude);
        let p = self
            .modeling
            .preview
            .as_mut()
            .ok_or("Extrusão não armada")?;
        p.values = [0.2, 0., 0.];
        p.inner_size = 70.;
        self.update_mesh_preview();
        if !self
            .modeling
            .preview
            .as_ref()
            .is_some_and(|p| p.needs_space)
        {
            return Err("Fixture não exigiu autorização para ampliar o atlas cheio".into());
        }
        let p = self.modeling.preview.as_mut().unwrap();
        p.allow_expansion = true;
        p.previous = [f32::NAN; 3];
        self.update_mesh_preview();
        self.check_atlas_preview()?;
        self.confirm_mesh_operation();
        let expanded = self
            .scene()
            .entity(id)
            .and_then(|e| e.material.texture.clone())
            .filter(|linked| linked != texture)
            .ok_or("A cópia ampliada não foi vinculada")?;
        self.check_atlas_pixels(texture, &expanded, pixels)?;
        let applied = self.state.clone();
        let cap = self.modeling.selection.clone();
        if cap.ids.len() < 2 {
            return Err("Fixture não produziu tampa com seleção distinta da face de origem".into());
        }
        let command = self.history.last_command_id().ok_or("Comando ausente")?;
        let bytes = self.history.estimated_bytes();
        if self.history.undo_len() != 1 || bytes == 0 {
            return Err("Aplicar não produziu exatamente um delta de histórico".into());
        }
        self.atlas_amend(100., 0.)?;
        if self.state.project != base.project
            || self.state.project.asset(&expanded).is_some()
            || self.renderer.texture_override_bytes() != pixels.pixels.len()
            || self.game_ui.texture_override_bytes() != pixels.pixels.len()
        {
            return Err("Prévia neutra deixou conversão, expansão ou override órfão".into());
        }
        self.cancel_mesh_operation();
        if self.state != applied
            || self.modeling.selection != cap
            || self.history.last_command_id() != Some(command)
            || self.history.estimated_bytes() != bytes
        {
            return Err("Cancelar ajuste não restaurou dados, delta e seleção da tampa".into());
        }
        self.check_atlas_pixels(texture, &expanded, pixels)?;
        self.atlas_amend(100., 0.)?;
        let p = self.modeling.preview.as_mut().unwrap();
        p.inner_size = 60.;
        p.values = [0.3, 0., 0.];
        p.previous = [f32::NAN; 3];
        self.update_mesh_preview();
        self.check_atlas_preview()?;
        self.confirm_mesh_operation();
        if self.history.undo_len() != 1
            || self
                .history
                .last_command_id()
                .is_none_or(|id| id == command)
            || self.state.project.assets.len() != 2
            || self
                .scene()
                .entity(id)
                .and_then(|e| e.material.texture.as_deref())
                != Some(expanded.as_str())
        {
            return Err(
                "Reajuste acumulou comandos, criou asset órfão ou mudou o ID reservado".into(),
            );
        }
        self.check_atlas_pixels(texture, &expanded, pixels)?;
        let adjusted = self.state.clone();
        validate_project(&adjusted.project)?;
        self.undo(false);
        if self.state != base || self.history.undo_len() != 0 {
            return Err("Undo não restaurou primitiva e pintura anteriores à operação".into());
        }
        self.undo(true);
        if self.state != adjusted || self.history.undo_len() != 1 {
            return Err("Redo perdeu a geometria ou o atlas do comando reajustado".into());
        }
        self.check_atlas_pixels(texture, &expanded, pixels)
    }

    fn atlas_amend(&mut self, inner_size: f32, distance: f32) -> Result<(), String> {
        let last = self
            .modeling
            .last_operation
            .as_ref()
            .ok_or("Última operação ausente")?;
        let mut p = last.source.clone();
        p.amendment = Some(last.command);
        p.inner_size = inner_size;
        p.values = [distance, 0., 0.];
        p.previous = [f32::NAN; 3];
        p.numeric_edit = true;
        self.modeling.preview = Some(p);
        self.update_mesh_preview();
        self.check_atlas_preview()
    }

    fn check_atlas_preview(&self) -> Result<(), String> {
        let p = self.modeling.preview.as_ref().ok_or("Prévia desapareceu")?;
        p.error.clone().map_or(Ok(()), Err)
    }

    fn check_atlas_pixels(
        &self,
        texture: &str,
        expanded: &str,
        original: &PaintImage,
    ) -> Result<(), String> {
        let image = self
            .state
            .images
            .get(expanded)
            .ok_or("Pixels ampliados ausentes")?;
        if (image.width, image.height) != (512, 512)
            || self.state.images.get(texture) != Some(original)
            || image.pixel(31, 73) != original.pixel(31, 73)
        {
            return Err("Ampliação acumulada ou pintura original alterada".into());
        }
        let row = original.width as usize * 4;
        for y in 0..original.height as usize {
            if image.pixels[y * row * 2..y * row * 2 + row]
                != original.pixels[y * row..y * row + row]
            {
                return Err("A cópia ampliada não preservou todos os pixels originais".into());
            }
        }
        let expected = original.pixels.len() + image.pixels.len();
        if self.renderer.texture_override_bytes() != expected
            || self.game_ui.texture_override_bytes() != expected
        {
            return Err(
                "Cancelamento/Redo não restaurou os pixels ainda não salvos nos caches gráficos"
                    .into(),
            );
        }
        Ok(())
    }
}
