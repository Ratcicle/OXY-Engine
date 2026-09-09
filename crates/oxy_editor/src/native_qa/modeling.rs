use super::*;
use oxy_core::{document::*, painting::PaintImage};
impl NativeQa {
    pub(super) fn check_modeling(&mut self, label: &str) -> Result<(), String> {
        let (_, _, pending, history) = self.editor.qa_mesh_info();
        let source = self
            .editor
            .scene()
            .entities
            .iter()
            .find(|e| e.name == "Peça pintada")
            .unwrap();
        let target = self
            .editor
            .scene()
            .entities
            .iter()
            .find(|e| e.name == "Destino")
            .unwrap();
        match label {
            "m4_base" => {
                self.base = Some(self.editor.state.clone());
                self.initial_entities = history;
            }
            "m4_requires_atlas" => {
                if !pending
                    || source.material.texture != target.material.texture
                    || self.editor.state.project.assets.len() != 1
                    || history != self.initial_entities
                {
                    return Err("Extrusão não esperou a decisão sobre o atlas".into());
                }
            }
            "m4_cancel" => {
                if self.editor.mesh_operation_active()
                    || Some(&self.editor.state) != self.base.as_ref()
                    || history != self.initial_entities
                {
                    return Err(format!(
                        "Cancelar não restaurou a operação completa ({history}/{})",
                        self.initial_entities
                    ));
                }
            }
            "m4_extruded" => {
                let mesh = source.mesh.as_ref().ok_or("Malha não convertida")?;
                if pending
                    || mesh.data().faces.len() != 12
                    || mesh.data().vertices.len() != 14
                    || source.material.texture == target.material.texture
                    || self.editor.state.project.assets.len() != 2
                    || history != self.initial_entities + 1
                {
                    return Err(format!(
                        "Extrusão integrada incorreta: faces={}, vértices={}, histórico={history}",
                        mesh.data().faces.len(),
                        mesh.data().vertices.len()
                    ));
                }
                let original = self
                    .editor
                    .state
                    .images
                    .get(target.material.texture.as_ref().unwrap())
                    .unwrap();
                let expanded = self
                    .editor
                    .state
                    .images
                    .get(source.material.texture.as_ref().unwrap())
                    .unwrap();
                for y in 0..original.height {
                    for x in 0..original.width {
                        if original.pixel(x, y) != expanded.pixel(x, y) {
                            return Err("A ampliação alterou um pixel anterior".into());
                        }
                    }
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "m4_redo" => {
                if Some(&self.editor.state) != self.after_paint.as_ref() {
                    return Err("Refazer perdeu geometria ou pixels".into());
                }
            }
            "m4_snap_preview" => {
                if !self.editor.mesh_operation_active() || source.transform.position != [1., 0., 0.]
                {
                    return Err(format!(
                        "Encaixe não moveu a peça para o vértice: {:?}",
                        source.transform.position
                    ));
                }
            }
            "m4_snap_done" => {
                if self.editor.mesh_operation_active() || source.transform.position != [1., 0., 0.]
                {
                    return Err("Encaixe não foi confirmado".into());
                }
            }
            "m4_snap_scale" => {
                if source.transform.scale != [3., 1., 1.]
                    || source.transform.position != [0., 0., 0.]
                {
                    return Err(format!(
                        "Escala de encaixe alterou outros eixos: {:?}",
                        source.transform
                    ));
                }
            }
            "m4_created_face" => {
                let mesh = source.mesh.as_ref().unwrap();
                if mesh.data().faces.len() != 12 {
                    return Err("Criar não fechou a abertura".into());
                }
            }
            _ => return Err(format!("Check desconhecido {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native topology/atlas/snap QA, real WGPU preview and RawInput only"]
#[cfg(target_os = "windows")]
fn native_basic_modeling_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.2.0/m4");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-modeling-qa-{}", new_id()));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    let path = root.join("project.oxy.json");
    let mut project =
        oxy_core::editing::blank_project("Modelagem básica", SceneKind::ThreeD).unwrap();
    let mut source = Entity::new("Peça pintada", Some(Primitive::Cube));
    let mut target = Entity::new("Destino", Some(Primitive::Cube));
    target.transform.position = [2., 0., 0.];
    let mut joint = Entity::new("Articulação filha", None);
    joint.parent = Some(source.id.clone());
    joint.transform.position = [0., 1., 0.];
    let texture = new_id();
    source.material.texture = Some(texture.clone());
    target.material.texture = Some(texture.clone());
    let mut pixels = PaintImage::new(256, 256, [70, 160, 175, 255]).unwrap();
    for y in 0..256 {
        for x in 0..256 {
            if (x / 16 + y / 16) % 2 == 0 {
                pixels.set_pixel(x, y, [215, 220, 195, 255]);
            }
        }
    }
    pixels.save(&root.join("assets/paint.png")).unwrap();
    project.assets.push(Asset {
        id: texture,
        name: "Atlas original".into(),
        kind: AssetKind::Texture,
        path: "assets/paint.png".into(),
        model: None,
    });
    project.scenes[0].entities = vec![source, target, joint];
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA operações de modelagem")
            .with_inner_size([1440., 900.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|b| {
            b.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA modelagem",
        options,
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Peça pintada"),
                Action::Click("Estúdio"),
                Action::Click("Enquadrar"),
                Action::Check("m4_base"),
                Action::Chord(Key::V, Modifiers::SHIFT),
                Action::WorldClick([0.5, 0.5, 0.5]),
                Action::WorldClick([1.5, 0.5, 0.5]),
                Action::Check("m4_snap_preview"),
                Action::Screenshot("vertex-snap.png"),
                Action::Click("Escalar X"),
                Action::Check("m4_snap_scale"),
                Action::Chord(Key::Escape, Modifiers::ALT),
                Action::Check("m4_cancel"),
                Action::Chord(Key::V, Modifiers::SHIFT),
                Action::WorldClick([0.5, 0.5, 0.5]),
                Action::WorldClick([1.5, 0.5, 0.5]),
                Action::Click("Confirmar encaixe (Enter)"),
                Action::Check("m4_snap_done"),
                Action::Key(Key::Z, true),
                Action::Check("m4_cancel"),
                Action::Key(Key::Num2, false),
                Action::ComponentClick([0., 0., 0.5], false),
                Action::ComponentClick([0.5, 0., 0.], true),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::Check("m4_requires_atlas"),
                Action::Click("Criar cópia ampliada (2×)"),
                Action::Screenshot("extrusion-preview.png"),
                Action::Chord(Key::Escape, Modifiers::ALT | Modifiers::SHIFT),
                Action::Check("m4_cancel"),
                Action::Chord(Key::E, Modifiers::SHIFT),
                Action::Click("Criar cópia ampliada (2×)"),
                Action::Click("Confirmar (Enter)"),
                Action::Check("m4_extruded"),
                Action::Key(Key::Z, true),
                Action::Check("m4_cancel"),
                Action::Key(Key::Y, true),
                Action::Check("m4_redo"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::SelectEntity("Peça pintada"),
                Action::Click("Estúdio"),
                Action::Click("Enquadrar"),
            ]);
            // Reopen clears history. Additional object alignment is tested from the saved mesh below.
            qa.actions.extend([
                Action::Key(Key::Num2, false),
                Action::ComponentClick([0.1767767, 0., 0.6767767], false),
                Action::Key(Key::Delete, false),
                Action::Click("Confirmar (Enter)"),
                Action::Key(Key::A, true),
                Action::Chord(Key::F, Modifiers::SHIFT),
                Action::Click("Confirmar (Enter)"),
                Action::Check("m4_created_face"),
                Action::Key(Key::Z, true),
                Action::Key(Key::Z, true),
                Action::Check("m4_redo"),
                Action::Key(Key::Num1, false),
                Action::Check("m4_base"),
                Action::Chord(Key::V, Modifiers::SHIFT),
                // Source cap after the diagonal region extrusion: choose an untouched bottom-left point.
                Action::Chord(Key::Escape, Modifiers::ALT),
                Action::Check("m4_cancel"),
                Action::Key(Key::S, true),
                Action::Screenshot("modeled-painted-mesh.png"),
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
    std::fs::write(output.join("native-modeling.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
