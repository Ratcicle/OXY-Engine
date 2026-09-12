//! Full authoring workflow: every document mutation comes from editor RawInput.
use super::*;
use oxy_core::document::*;

impl NativeQa {
    pub(super) fn final_action(
        &mut self,
        _ctx: &egui::Context,
        label: &'static str,
    ) -> Result<(), String> {
        match label {
            "save_location" => {
                // Test file routing only: creation and saving still use the ordinary editor.
                let directory = self.output.join("project");
                std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                self.editor.path = Some(directory.join("project.oxy.json"));
            }
            "cap_edge" => {
                let mesh = self.editor.model_source()?;
                let cap = mesh
                    .face(
                        *self
                            .editor
                            .mesh_components()
                            .ids
                            .first()
                            .ok_or("Extrusão sem face selecionada")?,
                    )
                    .ok_or("Tampa ausente")?;
                let ends = cap
                    .corners
                    .iter()
                    .zip(cap.corners.iter().cycle().skip(1))
                    .take(cap.corners.len())
                    .find(|(a, b)| {
                        mesh.position(a.vertex).unwrap().y > 0.49
                            && mesh.position(b.vertex).unwrap().y > 0.49
                    })
                    .ok_or("Aresta superior da região extrudada ausente")?;
                let center = (mesh.position(ends.0.vertex).unwrap()
                    + mesh.position(ends.1.vertex).unwrap())
                    * 0.5;
                self.actions.insert(1, Action::Key(Key::Num3, false));
                self.actions
                    .insert(2, Action::ComponentClick(center.to_array(), false));
                self.base = Some(self.editor.state.clone());
                self.initial_entities = self.editor.qa_mesh_info().3;
            }
            "paint_faces" => {
                let mesh = self.editor.model_source()?;
                let old = self
                    .base
                    .as_ref()
                    .ok_or("Base de arredondamento ausente")?
                    .project
                    .scenes[0]
                    .entities
                    .iter()
                    .find(|e| e.name == "Tubo criado")
                    .and_then(|e| e.mesh.as_ref())
                    .ok_or("Malha antiga ausente")?;
                let choose = |new: bool| {
                    mesh.data()
                        .faces
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| old.face(f.id).is_none() == new)
                        .max_by(|(a, _), (b, _)| {
                            mesh.prepared().face_normals[*a]
                                .z
                                .total_cmp(&mesh.prepared().face_normals[*b].z)
                        })
                        .map(|(_, f)| f.id)
                        .ok_or("Face pintável ausente")
                };
                self.paint_faces = Some((choose(false)?, choose(true)?));
            }
            "camera_x" | "camera_y" | "camera_z" | "camera_pitch" => {
                let (row, prefix, value) = match label {
                    "camera_x" => ("Posição", "X ", "1"),
                    "camera_y" => ("Posição", "Y ", "1"),
                    "camera_z" => ("Posição", "Z ", "4"),
                    _ => ("Rotação °", "X ", "-14"),
                };
                let anchor = self.find(row, false).ok_or("Campo da câmera não visível")?;
                let point = self
                    .surface
                    .texts
                    .iter()
                    .filter(|t| {
                        t.text.starts_with(prefix)
                            && t.rect.center().y > anchor.y
                            && t.rect.center().x > anchor.x - 80.
                    })
                    .min_by(|a, b| a.rect.center().y.total_cmp(&b.rect.center().y))
                    .ok_or("Valor do eixo da câmera não visível")?
                    .rect
                    .center();
                self.click(point);
                self.click(point);
                self.key(Key::A, true);
                self.events.push_back(vec![Event::Text(value.into())]);
                self.key(Key::Enter, false);
            }
            _ => return Err(format!("Ação final desconhecida: {label}")),
        }
        Ok(())
    }
    pub(super) fn check_final_flow(&mut self, label: &str) -> Result<(), String> {
        let selected = self
            .editor
            .selected
            .as_deref()
            .and_then(|id| self.editor.scene().entity(id));
        match label {
            "f7_new" => {
                if self.editor.scene().kind != SceneKind::ThreeD
                    || !self.editor.state.project.input_bindings.is_empty()
                {
                    return Err("Novo projeto não é 3D vazio".into());
                }
            }
            "f7_tube" => {
                let e = selected.ok_or("Tubo ausente")?;
                if e.primitive != Some(Primitive::Tube) || e.segments != 8 || e.mesh.is_some() {
                    return Err(format!(
                        "Tubo paramétrico incorreto: {:?}, {}",
                        e.primitive, e.segments
                    ));
                }
            }
            "f7_converted" | "f7_loop" | "f7_extruded" | "f7_beveled" => {
                let e = selected.ok_or("Peça ausente")?;
                let mesh = e.mesh.as_ref().ok_or("Malha não convertida")?;
                let faces = mesh.data().faces.len();
                let valid = match label {
                    "f7_converted" => faces == 32,
                    "f7_loop" => faces == 40,
                    "f7_extruded" => faces == 44,
                    _ => faces > 44,
                };
                if !valid
                    || mesh
                        .prepared()
                        .incident_faces
                        .iter()
                        .any(|fs| fs.len() != 2)
                {
                    return Err(format!("Topologia inesperada: {label}, {faces} faces"));
                }
                if self.editor.mesh_operation_active() {
                    return Err("Operação não confirmada".into());
                }
                if label == "f7_beveled" {
                    if self.editor.qa_mesh_info().3 != self.initial_entities + 1 {
                        return Err("Arredondamento não gerou um único comando".into());
                    }
                    self.after_paint = Some(self.editor.state.clone());
                }
            }
            "f7_undo" => {
                if self.base.as_ref() != Some(&self.editor.state) {
                    return Err("Desfazer não restaurou documento, geometria, UVs e pixels".into());
                }
            }
            "f7_redo" => {
                if self.after_paint.as_ref() != Some(&self.editor.state) {
                    return Err("Refazer não restaurou documento, geometria, UVs e pixels".into());
                }
            }
            "f7_paint_base" => {
                self.before_paint = Some(self.editor.state.clone());
            }
            "f7_old_painted" => {
                self.texture_base = Some(self.editor.state.clone());
            }
            "f7_painted" => {
                let e = selected.ok_or("Peça ausente")?;
                let texture = e.material.texture.as_ref().ok_or("Textura ausente")?;
                let pixels = self
                    .editor
                    .state
                    .images
                    .get(texture)
                    .ok_or("Pixels ausentes")?;
                let before = self
                    .before_paint
                    .as_ref()
                    .unwrap()
                    .images
                    .get(texture)
                    .ok_or("Imagem inicial ausente")?;
                let mesh = e.mesh.as_ref().unwrap();
                for id in [self.paint_faces.unwrap().0, self.paint_faces.unwrap().1] {
                    let f = mesh.face(id).unwrap();
                    let uv = f
                        .corners
                        .iter()
                        .map(|c| glam::Vec2::from(c.uv))
                        .sum::<glam::Vec2>()
                        / f.corners.len() as f32;
                    if pixels.sample_uv(uv.to_array()) == before.sample_uv(uv.to_array()) {
                        return Err(format!("Face {id} não recebeu pixels reais"));
                    }
                    if id == self.paint_faces.unwrap().0
                        && pixels.sample_uv(uv.to_array())
                            != self
                                .texture_base
                                .as_ref()
                                .unwrap()
                                .images
                                .get(texture)
                                .unwrap()
                                .sample_uv(uv.to_array())
                    {
                        return Err("Pintura da superfície nova alterou a pintura anterior".into());
                    }
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "f7_paint_undo" => {
                if self.texture_base.as_ref() != Some(&self.editor.state) {
                    return Err("Uma pincelada não desfez como um único gesto".into());
                }
            }
            "f7_model" => {
                let model = self
                    .editor
                    .state
                    .project
                    .assets
                    .iter()
                    .find(|a| a.kind == AssetKind::Model)
                    .and_then(|a| a.model.as_ref())
                    .ok_or("Modelo não salvo")?;
                if model.iter().all(|e| e.mesh.is_none()) {
                    return Err("Modelo perdeu a malha editável".into());
                }
            }
            "f7_instance" => {
                let e = selected.ok_or("Instância ausente")?;
                let original = self
                    .editor
                    .scene()
                    .entities
                    .iter()
                    .find(|e| e.name == "Tubo criado")
                    .ok_or("Original ausente")?;
                if e.id == original.id || e.mesh != original.mesh || e.material != original.material
                {
                    return Err("Instância perdeu independência de IDs/geometria/material".into());
                }
                self.created = Some(e.id.clone());
            }
            "f7_spatial_base" => {
                self.base = Some(self.editor.state.clone());
            }
            "f7_pivot" => {
                let id = self.created.as_deref().unwrap();
                let before = &self.base.as_ref().unwrap().project.scenes[0];
                if !before
                    .world_matrix(id)?
                    .abs_diff_eq(self.editor.scene().world_matrix(id)?, 1e-4)
                    || before.entity(id).unwrap().transform.pivot
                        == selected.unwrap().transform.pivot
                {
                    return Err("Pivô não mudou com preservação da peça".into());
                }
                let mut a = oxy_core::physics3d::PhysicsWorld::new();
                let mut b = oxy_core::physics3d::PhysicsWorld::new();
                a.sync_scene(before, false)?;
                b.sync_scene(self.editor.scene(), false)?;
                let a = a
                    .debug_shapes()
                    .find(|s| s.id == id)
                    .ok_or("Colisor anterior ausente")?;
                let b = b
                    .debug_shapes()
                    .find(|s| s.id == id)
                    .ok_or("Colisor atual ausente")?;
                if !a.position.abs_diff_eq(b.position, 1e-4)
                    || !a.rotation.abs_diff_eq(b.rotation, 1e-4)
                    || a.geometry.vertices != b.geometry.vertices
                {
                    return Err("Pivô deslocou a forma física 3D".into());
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "f7_animation" => {
                let e = selected.ok_or("Instância ausente")?;
                let clip = e.clips.first().ok_or("Animação ausente")?;
                let track = clip
                    .tracks
                    .iter()
                    .find(|t| t.target == e.id)
                    .ok_or("Trilha rígida ausente")?;
                if track.keyframes.len() != 2
                    || track.keyframes[0].transform == track.keyframes[1].transform
                {
                    return Err("Animação não gravou duas poses distintas".into());
                }
                self.base = Some(self.editor.state.clone());
            }
            "f7_runtime" => {
                let runtime = self.editor.runtime.as_ref().ok_or("Runtime ausente")?;
                let id = self.created.as_deref().unwrap();
                let animated = runtime
                    .scene()
                    .entity(id)
                    .ok_or("Instância ausente do runtime")?;
                let base = self
                    .editor
                    .scene()
                    .entity(id)
                    .ok_or("Instância ausente do documento")?;
                if animated.mesh != base.mesh || animated.transform == base.transform {
                    return Err("Runtime não executou a animação rígida configurada".into());
                }
            }
            "f7_camera" => {
                let e = selected.ok_or("Câmera ausente")?;
                if e.camera.is_none()
                    || e.parent.is_some()
                    || e.transform.position != [1., 1., 4.]
                    || (e.transform.rotation[0].to_degrees() + 14.).abs() > 1e-4
                {
                    return Err(format!(
                        "Câmera configurada incorretamente: {:?}",
                        e.transform
                    ));
                }
            }
            _ => return Err(format!("Verificação final desconhecida: {label}")),
        }
        Ok(())
    }
}

#[test]
#[ignore = "Native full authoring workflow with real WGPU and isolated RawInput"]
#[cfg(target_os = "windows")]
fn native_final_creation_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.2.1/m7/full-flow");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA criação completa",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(
                cc,
                workspace.join("examples/validacao/project.oxy.json"),
                artifacts,
                shared,
            );
            qa.editor = Editor::new(cc);
            qa.actions = VecDeque::from([
                Action::Click("Novo projeto"),
                Action::Click("Cena 3D\nComece com modelos, profundidade e uma câmera 3D."),
                Action::Click("Criar projeto"),
                Action::Check("f7_new"),
                Action::Final("save_location"),
                Action::Click("Estúdio"),
                Action::Click("+ Objeto"),
                Action::Click("Tubo"),
                Action::Key(Key::Enter, false),
                Action::Key(Key::F2, false),
                Action::Key(Key::A, true),
                Action::Text("Tubo criado"),
                Action::Key(Key::Enter, false),
                Action::Check("f7_tube"),
                Action::Key(Key::Num2, false),
                Action::OptionalClick("Ferramentas"),
                Action::Click("Malha"),
                Action::Click("Converter em malha editável"),
                Action::Check("f7_converted"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Key(Key::Num3, false),
                Action::ComponentClick([0., 0., 0.5], false),
                Action::Chord(Key::R, Modifiers::SHIFT),
                Action::WorldClick([0., 0., 0.5]),
                Action::Key(Key::Enter, false),
                Action::Check("f7_loop"),
                Action::Screenshot("tube-loop.png"),
                Action::Key(Key::Num2, false),
                Action::ComponentClick([0.1767767, 0.25, 0.4267767], false),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::EditValue("Distância", "0.25"),
                Action::Key(Key::Enter, false),
                Action::Check("f7_extruded"),
                Action::Final("cap_edge"),
                Action::Chord(Key::B, Modifiers::SHIFT),
                Action::EditValue("Segmentos", "2"),
                Action::EditValue("Largura", "0.02"),
                Action::Check("f7_beveled"),
                Action::Key(Key::Z, true),
                Action::Check("f7_undo"),
                Action::Key(Key::Y, true),
                Action::Check("f7_redo"),
                Action::Screenshot("tube-edited.png"),
                Action::Final("paint_faces"),
                Action::Click("Pintura"),
                Action::OptionalClick("Pintar"),
                Action::OptionalClick("Textura"),
                Action::Click("Criar textura"),
                Action::Click("Criar 512×512"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Check("f7_paint_base"),
                Action::PaintFace {
                    new: false,
                    image: true,
                },
                Action::Check("f7_old_painted"),
                Action::PaintFace {
                    new: true,
                    image: false,
                },
                Action::Check("f7_painted"),
                Action::Key(Key::Z, true),
                Action::Check("f7_paint_undo"),
                Action::Key(Key::Y, true),
                Action::Check("f7_redo"),
                Action::Screenshot("tube-painted.png"),
                Action::Click("Modelagem"),
                Action::Key(Key::Num1, false),
                Action::Scroll("PROPRIEDADES", -550.),
                Action::ContextEntity("Tubo criado"),
                Action::Click("Salvar hierarquia como modelo"),
                Action::Check("f7_model"),
                Action::Click("Colocar na cena"),
                Action::Key(Key::F2, false),
                Action::Key(Key::A, true),
                Action::Text("Instância articulada"),
                Action::Key(Key::Enter, false),
                Action::Check("f7_instance"),
                Action::Key(Key::W, false),
                Action::GizmoDelta(440.),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Scroll("PROPRIEDADES", 900.),
                Action::Click("+ Adicionar componente"),
                Action::Click("Colisor 3D"),
                Action::Click("Colisor 3D"),
                Action::Click("Gerar colisor convexo"),
                Action::Check("f7_spatial_base"),
                Action::Key(Key::P, false),
                Action::SpatialDrag {
                    face: None,
                    delta: [0., 0.25, 0.],
                    cancel: false,
                },
                Action::Check("f7_pivot"),
                Action::Key(Key::Z, true),
                Action::Check("f7_undo"),
                Action::Key(Key::Y, true),
                Action::Check("f7_redo"),
                Action::Screenshot("tube-pivot.png"),
                Action::Key(Key::W, false),
                Action::Click("Visualização"),
                Action::Click("Colisores"),
                Action::Screenshot("tube-collider.png"),
                Action::Key(Key::W, false),
                Action::Click("Animação"),
                Action::Click("+ Nova animação"),
                Action::Key(Key::A, true),
                Action::Text("Giro rígido"),
                Action::Key(Key::Enter, false),
                Action::Click("+ Quadro-chave"),
                Action::EditValue("Cursor", "0.5"),
                Action::Key(Key::E, false),
                Action::GizmoZ,
                Action::Click("+ Quadro-chave"),
                Action::Check("f7_animation"),
                Action::Screenshot("tube-rigid-animation.png"),
                Action::Click("Modelo"),
                Action::Click("Pose-base"),
                Action::Key(Key::Escape, false),
                Action::Click("Lógica"),
                Action::Click("Ao iniciar cena"),
                Action::Click("Pesquisar ação…"),
                Action::Text("animação"),
                Action::Click("Reproduzir animação"),
                Action::DragNode("Reproduzir animação"),
                Action::Click("Escolha…"),
                Action::Click("Instância articulada · Giro rígido"),
                Action::ConnectPorts,
                Action::Screenshot("tube-animation-graph.png"),
                Action::Click("Cena"),
                Action::Click("+ Objeto"),
                Action::Click("Câmera"),
                Action::Click("Transformação"),
                Action::Final("camera_x"),
                Action::Final("camera_y"),
                Action::Final("camera_z"),
                Action::Final("camera_pitch"),
                Action::Check("f7_camera"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::SelectEntity("Instância articulada"),
                Action::Click("▶ Jogar"),
                Action::Wait(20),
                Action::Check("f7_runtime"),
                Action::Screenshot("tube-runtime.png"),
                Action::Click("■ Parar"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::SelectEntity("Instância articulada"),
                Action::Click("Estúdio"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Screenshot("tube-final-reopened.png"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let r = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        r.done,
        r.error,
        r.steps.join("\n"),
        r.screenshots
    );
    std::fs::write(output.join("native-final-flow.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
