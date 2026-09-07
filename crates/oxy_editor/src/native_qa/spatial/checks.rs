use super::*;
impl NativeQa {
    pub(in crate::native_qa) fn setup_spatial_3d(&mut self) -> Result<(), String> {
        self.editor.set_spatial_tool(crate::app::Tool::Object);
        let mut scene = self.editor.scene().clone();
        scene.kind = SceneKind::ThreeD;
        scene.id = new_id();
        scene.name = "Montagem 3D".into();
        let ids = scene
            .entities
            .iter()
            .map(|e| (e.id.clone(), new_id()))
            .collect();
        remap_entities(&mut scene.entities, &ids)?;
        for e in &mut scene.entities {
            if e.primitive.is_some() {
                e.primitive = Some(Primitive::Cube);
            }
        }
        let mut parent = Entity::new("Pai transformado", None);
        parent.transform.scale = [1.5, 1.2, 0.8];
        parent.transform.rotation = [0.1, 0.3, 0.];
        parent.transform.position = [1., 0., -1.];
        let group = scene
            .entities
            .iter_mut()
            .find(|e| e.name == "Personagem")
            .ok_or("Grupo ausente")?;
        group.parent = Some(parent.id.clone());
        group.transform.scale = [-1., 1., 1.];
        group.controller = None;
        let id = group.id.clone();
        scene.entities.push(parent);
        self.editor.scene_id = scene.id.clone();
        self.editor.camera = oxy_render::CameraState::for_scene(&scene);
        self.editor.state.project.scenes.push(scene);
        self.editor.select(Some(id));
        self.editor.frame_selection();
        Ok(())
    }
    pub(in crate::native_qa) fn check_spatial(&mut self, label: &str) -> Result<(), String> {
        let require = |yes: bool, message: &str| {
            if yes {
                Ok(())
            } else {
                Err(format!("{label}: {message}"))
            }
        };
        match label {
            "spatial_collapsed" => require(
                self.entity_position("Braço").is_none(),
                "Recolher não ocultou os filhos",
            ),
            "spatial_expanded" => require(
                self.entity_position("Braço").is_some(),
                "Expandir não revelou os filhos",
            ),
            "spatial_animation_baseline" => {
                self.before_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "spatial_drafts" => require(
                self.editor.studio.animation.drafts.len() == 2
                    && (self.editor.studio.animation.time - 0.5).abs() < 0.0001,
                "Poses provisórias perderam seu instante ou peça",
            ),
            "spatial_draft_structure_blocked" => require(
                self.editor.spatial.mode == crate::app::Tool::Object
                    && self.editor.studio.animation.drafts.len() == 2
                    && self
                        .editor
                        .messages
                        .last()
                        .is_some_and(|s| s.contains("grave ou descarte")),
                "Mudança estrutural descartou pose",
            ),
            "spatial_collective_keys" => {
                let before = self
                    .before_paint
                    .as_ref()
                    .unwrap()
                    .project
                    .scene(&self.editor.scene_id)
                    .unwrap();
                for e in &self.editor.scene().entities {
                    require(
                        e.transform == before.entity(&e.id).unwrap().transform,
                        "Gravação alterou pose-base",
                    )?;
                }
                let c = self
                    .editor
                    .scene()
                    .entity(self.editor.studio.owner.as_deref().unwrap())
                    .unwrap()
                    .clips
                    .iter()
                    .find(|c| Some(&c.id) == self.editor.studio.animation.selected.as_ref())
                    .unwrap();
                require(
                    self.editor.studio.animation.drafts.is_empty()
                        && c.tracks
                            .iter()
                            .filter(|t| t.keyframes.iter().any(|k| (k.time - 0.5).abs() < 0.0001))
                            .count()
                            == 2,
                    "Gravação coletiva incompleta",
                )
            }
            "spatial_animation_undo" => require(
                self.before_paint.as_ref() == Some(&self.editor.state),
                "Gravação coletiva não foi um único comando",
            ),
            "spatial_animation_base_mode" => require(
                self.editor.spatial.base_pose
                    && self.editor.animation_preview() == *self.editor.scene(),
                "Ferramenta estrutural não usa pose-base",
            ),
            "spatial_animation_restored" => require(
                !self.editor.spatial.base_pose
                    && (self.editor.studio.animation.time - 0.5).abs() < 0.0001
                    && self.editor.animation_preview() != *self.editor.scene(),
                "Contexto da animação não foi restaurado",
            ),
            "spatial_track_blocked" => require(
                self.editor.spatial.mode == crate::app::Tool::Object
                    && self
                        .editor
                        .messages
                        .last()
                        .is_some_and(|s| s.contains("Ataque") && s.contains("bloqueada")),
                "Pivô animado deveria ser bloqueado e identificar o clip",
            ),
            "spatial_fit" => {
                let id = self.editor.selected.as_deref().ok_or("Seleção ausente")?;
                let scene = self.editor.scene();
                let actual = oxy_core::spatial::collider_bounds(scene, id)?;
                let expected = oxy_core::spatial::visual_bounds(scene, &scene.descendants(id))?;
                require(
                    actual.min.abs_diff_eq(expected.min, 0.0002)
                        && actual.max.abs_diff_eq(expected.max, 0.0002),
                    "Ajuste diverge dos limites dos filhos",
                )
            }
            "spatial_edge" | "spatial_center" => {
                let id = self.editor.selected.as_deref().ok_or("Seleção ausente")?;
                let before = self
                    .base
                    .as_ref()
                    .unwrap()
                    .project
                    .scene(&self.editor.scene_id)
                    .unwrap();
                let after = self.editor.scene();
                let a = oxy_core::spatial::collider_bounds(before, id)?;
                let b = oxy_core::spatial::collider_bounds(after, id)?;
                require(
                    !a.min.abs_diff_eq(b.min, 0.0001) || !a.max.abs_diff_eq(b.max, 0.0001),
                    "Alça não alterou a caixa",
                )?;
                if label == "spatial_edge" {
                    require(
                        (a.max - b.max).abs().min_element() < 0.0001,
                        "Borda oposta mudou",
                    )?;
                } else {
                    require(
                        (a.max - a.min).abs_diff_eq(b.max - b.min, 0.0001),
                        "Centro alterou dimensões",
                    )?;
                }
                for e in &after.entities {
                    require(
                        e.transform == before.entity(&e.id).unwrap().transform,
                        "Colisor moveu uma peça",
                    )?;
                }
                self.after_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "spatial_undo" | "spatial_cancel" => require(
                self.base.as_ref() == Some(&self.editor.state),
                "Gesto não restaurado integralmente",
            ),
            "spatial_redo" => require(
                self.after_paint.as_ref() == Some(&self.editor.state),
                "Refazer divergiu do gesto",
            ),
            "spatial_pivot" => {
                let before = self
                    .base
                    .as_ref()
                    .unwrap()
                    .project
                    .scene(&self.editor.scene_id)
                    .unwrap();
                let scene = self.editor.scene();
                let id = self.editor.selected.as_deref().unwrap();
                require(
                    scene.entity(id).unwrap().transform.pivot
                        != before.entity(id).unwrap().transform.pivot,
                    "Alça não mudou o pivô",
                )?;
                for e in &scene.entities {
                    require(
                        scene
                            .world_matrix(&e.id)?
                            .abs_diff_eq(before.world_matrix(&e.id)?, 0.0002),
                        "Pivô desmontou a hierarquia",
                    )?;
                    if e.collider.is_some() {
                        let a = oxy_core::spatial::collider_bounds(scene, &e.id)?;
                        let b = oxy_core::spatial::collider_bounds(before, &e.id)?;
                        require(
                            a.min.abs_diff_eq(b.min, 0.0002) && a.max.abs_diff_eq(b.max, 0.0002),
                            "Pivô deslocou colisor",
                        )?;
                    }
                }
                self.after_paint = Some(self.editor.state.clone());
                Ok(())
            }
            "spatial_floor" => {
                let rt = self.editor.runtime.as_mut().ok_or("Sem runtime")?;
                for _ in 0..180 {
                    rt.advance(1. / 60., &oxy_core::runtime::InputFrame::default());
                }
                let scene = rt.scene();
                let body = scene
                    .entities
                    .iter()
                    .find(|e| e.name == "Personagem")
                    .unwrap();
                let floor = scene.entities.iter().find(|e| e.name == "Chão").unwrap();
                let b = oxy_core::runtime::collider_box(scene, &body.id).unwrap();
                let f = oxy_core::runtime::collider_box(scene, &floor.id).unwrap();
                require(
                    (b.min.y - f.max.y).abs() < 0.005,
                    "Caixa real não parou no chão",
                )
            }
            _ => Err(format!("Verificação desconhecida: {label}")),
        }
    }
}
