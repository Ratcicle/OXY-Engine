use super::*;
use oxy_core::{
    document::{Collider, Entity, Primitive},
    geometry::selection::Mode,
};
impl NativeQa {
    pub(super) fn check_mesh(&mut self, label: &str) -> Result<(), String> {
        let (mode, count, pending, history) = self.editor.qa_mesh_info();
        let entity = self
            .editor
            .selected
            .as_ref()
            .and_then(|id| self.editor.scene().entity(id))
            .ok_or("Peça não selecionada")?;
        match label {
            "m3_creationbase" => {
                self.base = Some(self.editor.state.clone());
                self.initial_entities = history;
            }
            "m3_creationcancel" => {
                if self.editor.mesh_operation_active()
                    || self.base.as_ref() != Some(&self.editor.state)
                    || history != self.initial_entities
                {
                    return Err(format!(
                        "Cancelar criação: operação={}, projeto igual={}, histórico={}/{}; entidades={:?}",
                        self.editor.mesh_operation_active(),
                        self.base.as_ref() == Some(&self.editor.state),
                        history,
                        self.initial_entities,
                        self.editor
                            .scene()
                            .entities
                            .iter()
                            .map(|e| &e.name)
                            .collect::<Vec<_>>()
                    ));
                }
            }
            "m3_tube" => {
                if self.editor.mesh_operation_active()
                    || entity.primitive != Some(Primitive::Tube)
                    || entity.dimensions != [4., 1., 4.]
                    || entity.segments != 8
                    || entity.mesh.is_some()
                    || history != self.initial_entities + 1
                {
                    return Err(format!(
                        "Painel do tubo: ativo={}, forma={:?}, dimensões={:?}, lados={}, histórico={} (base={})",
                        self.editor.mesh_operation_active(),
                        entity.primitive,
                        entity.dimensions,
                        entity.segments,
                        history,
                        self.initial_entities
                    ));
                }
            }
            "m3_outside" => {
                if self.editor.mesh_operation_active()
                    || entity.primitive != Some(Primitive::Cone)
                    || history != self.initial_entities + 1
                {
                    return Err(
                        "O primeiro clique externo não confirmou a criação preservando a seleção"
                            .into(),
                    );
                }
            }
            "m3_parameters" => {
                if !self.editor.mesh_operation_active() || entity.primitive != Some(Primitive::Cone)
                {
                    return Err("Parâmetros não puderam ser reabertos".into());
                }
            }
            "m3_parameters_cancelled" => {
                if self.editor.mesh_operation_active() || history != self.initial_entities + 1 {
                    return Err(format!(
                        "Cancelar parâmetros: operação={}, histórico={}/{}",
                        self.editor.mesh_operation_active(),
                        history,
                        self.initial_entities + 1
                    ));
                }
            }
            "m3_base" => {
                self.base = Some(self.editor.state.clone());
                self.initial_entities = history;
            }
            "m3_face" => {
                if mode != Mode::Face || count != 1 || entity.mesh.is_some() {
                    return Err(format!(
                        "Modo/seleção/conversão inesperados: {mode:?}, {count}"
                    ));
                }
            }
            "m3_preview" => {
                let original = self
                    .base
                    .as_ref()
                    .unwrap()
                    .project
                    .scene(&self.editor.scene_id)
                    .unwrap()
                    .entity(&entity.id)
                    .unwrap();
                if pending
                    || entity.mesh.is_none()
                    || entity.transform != original.transform
                    || entity.collider != original.collider
                    || history != self.initial_entities + 1
                {
                    return Err(
                        "Gesto alterou transformação/colisor ou não formou um comando único".into(),
                    );
                }
                let mesh = entity.mesh.as_ref().unwrap();
                let source = oxy_core::geometry::primitives::for_entity(original)?;
                if mesh == &source {
                    return Err("Arrasto não modificou a geometria real".into());
                }
            }
            "m3_cancel" => {
                if pending
                    || self.base.as_ref() != Some(&self.editor.state)
                    || history != self.initial_entities
                {
                    return Err("Esc não restaurou geometria, parâmetros e histórico".into());
                }
            }
            "m3_commit" => {
                if pending || entity.mesh.is_none() || history != self.initial_entities + 1 {
                    return Err(format!("Operação não formou um único comando: {history}"));
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "m3_redo" => {
                if self.after_paint.as_ref() != Some(&self.editor.state) {
                    return Err("Refazer não restaurou exatamente a malha".into());
                }
            }
            "m3_deleted" => {
                if self.editor.scene().entities.len() != 2
                    || entity
                        .mesh
                        .as_ref()
                        .is_none_or(|m| m.data().faces.len() != 5)
                {
                    return Err("Delete não excluiu somente a face selecionada".into());
                }
            }
            "m3_vertices" => {
                if mode != Mode::Vertex || count != 8 {
                    return Err(format!("Ctrl+A de vértices: {mode:?}, {count}"));
                }
            }
            "m3_visible_vertices" => {
                if mode != Mode::Vertex || count == 0 || count >= 8 {
                    return Err(format!(
                        "Caixa sem seleção através incluiu vértices ocultos: {count}"
                    ));
                }
            }
            "m3_saved" => {
                let p = self.editor.path.as_ref().unwrap();
                let loaded = oxy_core::persistence::load_project_lazy(p)?;
                if loaded != self.editor.state.project {
                    return Err("Malha/IDs não sobreviveram ao salvamento".into());
                }
            }
            _ => return Err(format!("Verificação desconhecida {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native WGPU mesh selection, conversion, handles and transaction QA; RawInput only"]
#[cfg(target_os = "windows")]
fn native_mesh_foundation_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = workspace.join("qa/v0.2.1/m3");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-mesh-qa-{}", new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("project.oxy.json");
    let mut project =
        oxy_core::editing::blank_project("Malhas editáveis", SceneKind::ThreeD).unwrap();
    let mut entity = Entity::new("Cubo editável", Some(Primitive::Cube));
    entity.collider = Some(Collider::default());
    let mut child = Entity::new("Articulação filha", None);
    child.parent = Some(entity.id.clone());
    child.transform.position = [0., 1., 0.];
    project.scenes[0].entities = vec![entity, child];
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        persist_window: false,
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA malhas editáveis")
            .with_inner_size([1440., 900.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|b| {
            b.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA malhas",
        options,
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Cubo editável"),
                Action::Click("Estúdio"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Check("m3_base"),
                Action::Key(Key::Num2, false),
                Action::ComponentClick([0., 0., 0.5], false),
                Action::Check("m3_face"),
                Action::Screenshot("face-selection.png"),
                Action::GizmoCancel(100.),
                Action::Check("m3_cancel"),
                Action::GizmoDelta(100.),
                Action::Check("m3_preview"),
                Action::Screenshot("mesh-preview.png"),
                Action::Check("m3_commit"),
                Action::Key(Key::Z, true),
                Action::Check("m3_cancel"),
                Action::Key(Key::Y, true),
                Action::Check("m3_redo"),
                Action::Key(Key::Delete, false),
                Action::Check("m3_deleted"),
                Action::Key(Key::Z, true),
                Action::Check("m3_redo"),
                Action::Key(Key::Num4, false),
                Action::Key(Key::A, true),
                Action::Check("m3_vertices"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::ComponentBox,
                Action::Check("m3_visible_vertices"),
                Action::Chord(Key::X, Modifiers::SHIFT),
                Action::ComponentBox,
                Action::Check("m3_vertices"),
                Action::Screenshot("vertex-selection.png"),
                Action::ResizePhysical(Vec2::new(920., 600.)),
                Action::Screenshot("mesh-small.png"),
                Action::Idle,
                Action::Key(Key::S, true),
                Action::Check("m3_saved"),
                Action::Click("Cena"),
                Action::Check("m3_creationbase"),
                Action::Click("+ Objeto"),
                Action::Click("Tubo"),
                Action::Chord(Key::Escape, Modifiers::ALT | Modifiers::SHIFT),
                Action::Check("m3_creationcancel"),
                Action::Click("+ Objeto"),
                Action::Click("Tubo"),
                Action::Click("Enquadrar prévia"),
                Action::Screenshot("tube-parameters.png"),
                Action::EditValue("Raio externo", "2"),
                Action::Check("m3_tube"),
                Action::OptionalClick("Ferramentas"),
                Action::OptionalClick("Pintar"),
                Action::Click("Visualização"),
                Action::Click("Enquadrar seleção"),
                Action::Screenshot("tube-eight-sides.png"),
                Action::Key(Key::Z, true),
                Action::SelectEntity("Cubo editável"),
                Action::Check("m3_creationcancel"),
                Action::ReopenProject,
                Action::SelectEntity("Cubo editável"),
                Action::Check("m3_saved"),
                Action::Check("m3_creationbase"),
                Action::Click("+ Objeto"),
                Action::Click("Cone"),
                Action::Click("PROPRIEDADES"),
                Action::Check("m3_outside"),
                Action::Click("Forma e material"),
                Action::Click("Parâmetros da forma"),
                Action::Check("m3_parameters"),
                Action::Key(Key::Escape, false),
                Action::Check("m3_parameters_cancelled"),
                Action::Key(Key::Z, true),
                Action::SelectEntity("Cubo editável"),
                Action::Check("m3_creationcancel"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-mesh.txt"), &summary).unwrap();
    assert!(report.done && report.error.is_none(), "{summary}");
}
