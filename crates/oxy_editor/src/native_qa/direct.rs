use super::*;
use glam::{Quat, Vec3};
use oxy_core::{
    document::*,
    geometry::{EditableMesh, primitives},
};

fn tilted_rotation() -> Quat {
    Quat::from_euler(glam::EulerRot::XYZ, -0.22, 0.35, 0.)
}

impl NativeQa {
    pub(super) fn direct_action(&mut self, label: &str) -> Result<(), String> {
        let (_, _, pending, history) = self.editor.qa_mesh_info();
        match label {
            "atlas_amendment" => self.editor.qa_atlas_amendment()?,
            "base" => {
                self.base = Some(self.editor.state.clone());
                self.initial_entities = history;
                self.saved_before_draft =
                    Some(serde_json::to_vec(&self.editor.mesh_components().ids).unwrap());
            }
            "inclined_face" => {
                self.direct_click_local(tilted_rotation() * Vec3::new(0., 0., 0.5))?
            }
            "inset_flat" => self.check_direct_inset(Some(0.), 0)?,
            "inset_positive" => self.check_direct_inset(None, 1)?,
            "inset_adjusted" => self.check_direct_inset(Some(0.25), 1)?,
            "inset_negative" => self.check_direct_inset(None, -1)?,
            "inset_recess" => self.check_direct_inset(Some(-0.15), -1)?,
            "knife_pending" => {
                if !pending || !self.editor.mesh_operation_active() {
                    return Err("Bisturi não manteve o percurso provisório".into());
                }
            }
            "knife_double" => {
                self.direct_click_local(Vec3::new(0.5, 0., 0.5))?;
                self.direct_click_local(Vec3::new(0.5, 0., 0.5))?;
            }
            "knife_finished" => {
                let mesh = self
                    .editor
                    .scene()
                    .entity(self.editor.selected.as_deref().ok_or("Peça ausente")?)
                    .and_then(|e| e.mesh.as_ref())
                    .ok_or("Corte não produziu malha")?;
                if pending || history != self.initial_entities + 1 || mesh.data().faces.len() != 7 {
                    return Err(format!(
                        "Duplo clique não concluiu um único corte: pendente={pending}, histórico={history}, faces={}",
                        mesh.data().faces.len()
                    ));
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "armed" => {
                if !pending
                    || self.editor.mesh_operation_active()
                    || self.base.as_ref() != Some(&self.editor.state)
                    || history != self.initial_entities
                {
                    return Err("Armar alterou geometria, converteu a primitiva ou abriu uma transação de edição".into());
                }
            }
            "cancel" => {
                if pending
                    || self.base.as_ref() != Some(&self.editor.state)
                    || history != self.initial_entities
                {
                    return Err(format!(
                        "Cancelar não restaurou a origem atômica: pendente={pending}, histórico={history}/{}",
                        self.initial_entities
                    ));
                }
            }
            "applied" => {
                if pending
                    || self.base.as_ref() == Some(&self.editor.state)
                    || history != self.initial_entities + 1
                {
                    return Err(format!(
                        "Soltar não aplicou um comando único: pendente={pending}, histórico={history}/{}",
                        self.initial_entities
                    ));
                }
                validate_project(&self.editor.state.project)?;
                self.after_paint = Some(self.editor.state.clone());
            }
            "adjusted" => {
                if pending || history != self.initial_entities + 1 {
                    return Err("Ajustar criou outro comando ou permaneceu em prévia".into());
                }
                let e = self
                    .editor
                    .scene()
                    .entity(self.editor.selected.as_ref().unwrap())
                    .unwrap();
                let z = e
                    .mesh
                    .as_ref()
                    .ok_or("Malha ausente")?
                    .data()
                    .vertices
                    .iter()
                    .map(|v| v.position[2])
                    .fold(f32::NEG_INFINITY, f32::max);
                if (z - 0.9).abs() > 0.0001 {
                    return Err(format!(
                        "Ajuste acumulou extrusões ou perdeu a distância original: z={z}, esperado0.9"
                    ));
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "restored" => {
                if self.after_paint.as_ref() != Some(&self.editor.state) {
                    return Err("Desfazer/refazer perdeu o resultado ajustado".into());
                }
            }
            "cone_box" => {
                let id = self.editor.selected.as_ref().ok_or("Cone ausente")?;
                let world = self.editor.scene().world_matrix(id)?;
                let rect = self.surface.native_viewport.ok_or("Viewport ausente")?;
                let tip = oxy_render::collider_debug::project(
                    &self.editor.camera,
                    rect,
                    world.transform_point3(glam::Vec3::new(0., 0.5, 0.)),
                )
                .ok_or("Topo fora da projeção")?;
                let from = tip - Vec2::splat(9.);
                let to = tip + Vec2::splat(9.);
                self.events.push_back(vec![Event::PointerMoved(from)]);
                self.events.push_back(vec![Event::PointerButton {
                    pos: from,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::CTRL,
                }]);
                self.events.push_back(vec![Event::PointerMoved(to)]);
                self.events.push_back(vec![Event::PointerButton {
                    pos: to,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::CTRL,
                }]);
            }
            "cone_selected" => {
                let e = self
                    .editor
                    .scene()
                    .entity(self.editor.selected.as_ref().unwrap())
                    .unwrap();
                let mesh = primitives::for_entity(e)?;
                let tip = mesh
                    .data()
                    .vertices
                    .iter()
                    .find(|v| {
                        glam::Vec3::from(v.position).distance(glam::Vec3::new(0., 0.5, 0.)) < 1e-5
                    })
                    .ok_or("Cone sem topo")?
                    .id;
                let top: Vec<_> = mesh
                    .data()
                    .faces
                    .iter()
                    .filter(|f| f.corners.iter().any(|c| c.vertex == tip))
                    .map(|f| f.id)
                    .collect();
                if top.len() != 8
                    || top
                        .iter()
                        .any(|id| !self.editor.mesh_components().ids.contains(id))
                {
                    return Err(format!(
                        "CaixaCtrl no topo omitiu faces: esperadas{top:?}, selecionadas{:?}",
                        self.editor.mesh_components().ids
                    ));
                }
                if e.mesh.is_some() {
                    return Err("Selecionar converteu a primitiva".into());
                }
                self.saved_before_draft =
                    Some(serde_json::to_vec(&self.editor.mesh_components().ids).unwrap());
            }
            "selection_stable" => {
                if self.saved_before_draft.as_ref()
                    != Some(&serde_json::to_vec(&self.editor.mesh_components().ids).unwrap())
                {
                    return Err("Navegar alterou IDs da seleção através".into());
                }
            }
            "roundtrip" => {
                validate_project(&self.editor.state.project)?;
                let path = self.editor.path.as_ref().ok_or("Projeto sem caminho")?;
                let loaded = oxy_core::persistence::load_project(path)?;
                if loaded != self.editor.state.project {
                    return Err("Salvar/reabrir perdeu dados".into());
                }
            }
            _ => return Err(format!("Verificação direta desconhecida: {label}")),
        }
        Ok(())
    }

    fn direct_click_local(&mut self, local: Vec3) -> Result<(), String> {
        let world = self
            .editor
            .scene()
            .world_matrix(self.editor.selected.as_deref().ok_or("Peça ausente")?)?;
        let point = oxy_render::collider_debug::project(
            &self.editor.camera,
            self.surface.native_viewport.ok_or("Viewport ausente")?,
            world.transform_point3(local),
        )
        .ok_or("Ponto fora da projeção")?;
        self.click(point);
        Ok(())
    }

    /// Compare the UI result with the source face plane and tangent bounds, not
    /// with another call to the operation under test. This also detects wrong axes.
    fn check_direct_inset(
        &mut self,
        expected_distance: Option<f32>,
        sign: i8,
    ) -> Result<(), String> {
        let (_, _, pending, history) = self.editor.qa_mesh_info();
        if pending || history != self.initial_entities + 1 {
            return Err(
                "Tamanho interno, arrasto e ajuste não permaneceram no mesmo comando".into(),
            );
        }
        let id = self.editor.selected.as_deref().ok_or("Peça ausente")?;
        let before = self.base.as_ref().ok_or("Referência ausente")?;
        let original = before
            .project
            .scene(&self.editor.scene_id)
            .and_then(|s| s.entity(id))
            .ok_or("Origem ausente")?;
        let source = original
            .mesh
            .clone()
            .map_or_else(|| primitives::for_entity(original), Ok)?;
        let selected: Vec<u32> = serde_json::from_slice(
            self.saved_before_draft
                .as_deref()
                .ok_or("Seleção original ausente")?,
        )
        .map_err(|e| e.to_string())?;
        if selected.len() != 1 {
            return Err("Fixture precisa de uma face original selecionada".into());
        }
        let face = source.face(selected[0]).ok_or("Face original ausente")?;
        let points: Vec<_> = face
            .corners
            .iter()
            .map(|c| source.position(c.vertex).unwrap())
            .collect();
        let center = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let u = (points[1] - points[0]).normalize();
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize();
        let v = normal.cross(u);
        let current = self.editor.scene().entity(id).unwrap();
        let result = current
            .mesh
            .as_ref()
            .ok_or("Operação não produziu malha editável")?;
        if current.transform != original.transform || current.collider != original.collider {
            return Err(
                "A operação de componentes alterou transformação ou colisor da peça".into(),
            );
        }
        let cap = self.editor.mesh_components();
        let cap_points: Vec<_> = cap
            .vertices(result)
            .iter()
            .map(|&id| result.position(id).unwrap())
            .collect();
        if cap_points.is_empty() {
            return Err("Tampa não permaneceu selecionada".into());
        }
        let distance = (cap_points[0] - center).dot(normal);
        if expected_distance.is_some_and(|expected| (distance - expected).abs() > 0.0002)
            || sign > 0 && distance <= 0.0001
            || sign < 0 && distance >= -0.0001
        {
            return Err(format!(
                "Distância Normal incorreta: {distance}, esperada {expected_distance:?}, sinal {sign}"
            ));
        }
        for axis in [u, v] {
            let limits = |points: &[Vec3]| {
                points
                    .iter()
                    .map(|&p| (p - center).dot(axis))
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| {
                        (a.min(p), b.max(p))
                    })
            };
            let original_bounds = limits(&points);
            let inner_bounds = limits(&cap_points);
            if (inner_bounds.0 - original_bounds.0 * 0.7).abs() > 0.0002
                || (inner_bounds.1 - original_bounds.1 * 0.7).abs() > 0.0002
            {
                return Err(
                    "70% não preservou a proporção linear e o centro da face inclinada".into(),
                );
            }
        }
        if cap_points
            .iter()
            .any(|&p| ((p - center).dot(normal) - distance).abs() > 0.0002)
        {
            return Err("Tampa não foi deslocada pela normal da face".into());
        }
        let triangle_area = |mesh: &EditableMesh, t: &oxy_core::geometry::Triangle| {
            let [a, b, c] = mesh.triangle_points(t);
            (b - a).cross(c - a).length() * 0.5
        };
        let original_area: f32 = source
            .prepared()
            .triangles
            .iter()
            .filter(|t| t.face == face.id)
            .map(|t| triangle_area(&source, t))
            .sum();
        let mut cap_area = 0.;
        let mut frame_area = 0.;
        for triangle in &result.prepared().triangles {
            if cap.ids.contains(&triangle.face) {
                cap_area += triangle_area(result, triangle);
            } else if result
                .triangle_points(triangle)
                .iter()
                .all(|&p| (p - center).dot(normal).abs() < 0.0002)
            {
                frame_area += triangle_area(result, triangle);
            }
        }
        if (cap_area - original_area * 0.49).abs() > 0.0002
            || (frame_area - original_area * 0.51).abs() > 0.0002
        {
            return Err(format!(
                "Moldura/tampa incorretas: áreas {frame_area}/{cap_area}, área original {original_area}"
            ));
        }
        validate_project(&self.editor.state.project)?;
        self.after_paint = Some(self.editor.state.clone());
        Ok(())
    }
}

#[test]
#[ignore = "Native v0.2.1 gestures, last-command adjustment, inset and Ctrl box; real WGPU with isolated RawInput"]
#[cfg(target_os = "windows")]
fn native_direct_modeling_v021_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.2.1/direct");
    let root = output.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("project.oxy.json");
    let mut project =
        oxy_core::editing::blank_project("Gestos de modelagem 0.2.1", SceneKind::ThreeD).unwrap();
    let mut cube = Entity::new("Cubo direto", Some(Primitive::Cube));
    cube.collider = Some(Collider::default());
    let mut child = Entity::new("Articulação preservada", None);
    child.parent = Some(cube.id.clone());
    child.transform.position = [0., 1., 0.];
    let mut cone = Entity::new("Cone com oito lados", Some(Primitive::Cone));
    cone.segments = 8;
    cone.primitive_parameters = Some(primitives::Parameters {
        height_divisions: 8,
        ..Default::default()
    });
    cone.transform.position = [2., 0., 0.];
    let mut inclined = Entity::new("Face inclinada", Some(Primitive::Cube));
    let mut data = primitives::for_entity(&inclined).unwrap().data().clone();
    for vertex in &mut data.vertices {
        vertex.position = (tilted_rotation() * Vec3::from(vertex.position)).to_array();
    }
    for face in &mut data.faces {
        for corner in &mut face.corners {
            corner.normal = corner
                .normal
                .map(|n| (tilted_rotation() * Vec3::from(n)).to_array());
        }
    }
    inclined.mesh = Some(EditableMesh::new(data).unwrap());
    inclined.primitive = None;
    inclined.transform.position = [4., 0., 0.];
    inclined.material.color = [0.34, 0.69, 0.88, 1.];
    let mut recess = Entity::new("Cubo para rebaixo", Some(Primitive::Cube));
    recess.transform.position = [6., 0., 0.];
    recess.material.color = [0.78, 0.5, 0.27, 1.];
    let mut knife = Entity::new("Cubo do bisturi", Some(Primitive::Cube));
    knife.transform.position = [8., 0., 0.];
    project.scenes[0].entities = vec![cube, child, cone, inclined, recess, knife];
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA 0.2.1",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            persist_window: false,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Cubo direto"),
                Action::Click("Estúdio"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Key(Key::Num2, false),
                Action::ComponentClick([0., 0., 0.5], false),
                Action::Direct("base"),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::Direct("armed"),
                Action::Chord(Key::Escape, Modifiers::ALT),
                Action::Direct("cancel"),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::DirectDrag {
                    pixels: 40.,
                    cancel: true,
                },
                Action::Direct("cancel"),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::DirectDrag {
                    pixels: 50.,
                    cancel: false,
                },
                Action::Direct("applied"),
                Action::Screenshot("extrude-release.png"),
                Action::EditValue("Distância", "0.4"),
                Action::Direct("adjusted"),
                Action::Screenshot("last-operation.png"),
                Action::Key(Key::Z, true),
                Action::Direct("cancel"),
                Action::Key(Key::Y, true),
                Action::Direct("restored"),
                Action::ComponentClick([0., 0., 0.9], false),
                Action::Direct("base"),
                Action::Chord(Key::U, Modifiers::SHIFT),
                Action::Direct("armed"),
                Action::DirectDrag {
                    pixels: 40.,
                    cancel: false,
                },
                Action::Direct("applied"),
                Action::Screenshot("inset-frame.png"),
                Action::Key(Key::Z, true),
                Action::Direct("cancel"),
                Action::Key(Key::Y, true),
                Action::Direct("restored"),
                Action::Key(Key::S, true),
                Action::Direct("roundtrip"),
                Action::SelectEntity("Face inclinada"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Key(Key::Num2, false),
                Action::Direct("inclined_face"),
                Action::Direct("base"),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::Direct("armed"),
                Action::EditValue("Tamanho interno", "70"),
                Action::Direct("inset_flat"),
                Action::DirectDrag {
                    pixels: 45.,
                    cancel: false,
                },
                Action::Direct("inset_positive"),
                Action::Screenshot("inclined-inset-70-normal-release.png"),
                Action::EditValue("Distância", "0.25"),
                Action::Direct("inset_adjusted"),
                Action::Screenshot("inclined-inset-70-last-adjusted.png"),
                Action::EditValue("Tamanho interno", "0"),
                Action::Direct("restored"),
                Action::Key(Key::Z, true),
                Action::Direct("cancel"),
                Action::Key(Key::Y, true),
                Action::Direct("restored"),
                Action::SelectEntity("Cubo para rebaixo"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Key(Key::Num2, false),
                Action::ComponentClick([0., 0., 0.5], false),
                Action::Direct("base"),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::EditValue("Tamanho interno", "70"),
                Action::Direct("inset_flat"),
                Action::DirectDrag {
                    pixels: -35.,
                    cancel: false,
                },
                // At this framing, -35 screen pixels measures -0.1053 world
                // units: less than half the default 0.25 grid. It must stay flat.
                Action::Direct("inset_flat"),
                Action::DirectDrag {
                    pixels: -60.,
                    cancel: false,
                },
                Action::Direct("inset_negative"),
                Action::EditValue("Distância", "-0.15"),
                Action::Direct("inset_recess"),
                Action::Screenshot("inset-70-recess-negative.png"),
                Action::Key(Key::Z, true),
                Action::Direct("cancel"),
                Action::Key(Key::Y, true),
                Action::Direct("restored"),
                Action::SelectEntity("Cubo do bisturi"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Direct("base"),
                Action::Chord(Key::K, Modifiers::SHIFT),
                Action::ComponentClick([-0.5, 0., 0.5], false),
                Action::ComponentClick([0.5, 0., 0.5], false),
                Action::Direct("knife_pending"),
                Action::Screenshot("knife-before-double-click.png"),
                Action::Direct("knife_double"),
                Action::Direct("knife_finished"),
                Action::Screenshot("knife-double-click-committed.png"),
                Action::Key(Key::Z, true),
                Action::Direct("cancel"),
                Action::Key(Key::Y, true),
                Action::Direct("restored"),
                Action::SelectEntity("Cone com oito lados"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Key(Key::Num2, false),
                Action::Direct("cone_box"),
                Action::Direct("cone_selected"),
                Action::Screenshot("cone-through-front.png"),
                Action::PanZoom,
                Action::Direct("selection_stable"),
                Action::Screenshot("cone-through-orbit.png"),
                Action::ResizePhysical(Vec2::new(920., 600.)),
                Action::InterfaceScale(1.6),
                Action::Direct("selection_stable"),
                Action::Screenshot("small-component-editor.png"),
                Action::Key(Key::S, true),
                Action::Direct("roundtrip"),
                Action::Direct("atlas_amendment"),
                Action::Idle,
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
    std::fs::write(output.join("native-direct.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
