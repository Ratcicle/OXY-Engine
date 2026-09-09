use super::*;
use oxy_core::{document::*, painting::PaintImage};
impl NativeQa {
    pub(super) fn check_cuts(&mut self, label: &str) -> Result<(), String> {
        let (_, _, pending, history) = self.editor.qa_mesh_info();
        let source = self
            .editor
            .scene()
            .entities
            .iter()
            .find(|e| e.name == "Cubo de cortes")
            .unwrap();
        let faces = source.mesh.as_ref().map_or(0, |m| m.data().faces.len());
        match label {
            "m5_base" => {
                self.base = Some(self.editor.state.clone());
                self.initial_entities = history;
            }
            "m5_cancel" => {
                if pending
                    || self.base.as_ref() != Some(&self.editor.state)
                    || history != self.initial_entities
                {
                    return Err(format!(
                        "Cancelar corte não restaurou o original: faces={faces}, histórico={history}"
                    ));
                }
            }
            "m5_knife_one" => {
                if !pending || faces != 7 {
                    return Err(format!("Primeiro trecho não cortou uma face: {faces}"));
                }
            }
            "m5_knife_two" => {
                if !pending || faces != 8 {
                    return Err(format!("Percurso não cortou duas faces: {faces}"));
                }
            }
            "m5_loop" => {
                if !pending || faces != 10 {
                    return Err(format!(
                        "Corte em loop não percorreu os quatro quads: {faces}"
                    ));
                }
            }
            "m5_loop_confirmed" => {
                if pending || faces != 14 || history != self.initial_entities + 1 {
                    return Err(format!(
                        "Quantidade/confirmar do loop incorretos: {faces}, {history}"
                    ));
                }
            }
            "m5_bevel" => {
                if !pending || faces != 10 || self.editor.state.project.assets.len() != 2 {
                    return Err(format!("Arredondamento com quatro segmentos: {faces}"));
                }
            }
            "m5_vertex" => {
                if !pending || faces <= 10 {
                    return Err(format!(
                        "Arredondamento do canto não subdividiu a superfície: {faces}"
                    ));
                }
            }
            "m5_committed" => {
                if pending || history != self.initial_entities + 1 {
                    return Err("Corte não formou um comando único".into());
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "m5_redo" => {
                if self.after_paint.as_ref() != Some(&self.editor.state) {
                    return Err("Refazer perdeu a malha ou a pintura".into());
                }
            }
            _ => return Err(format!("Check desconhecido {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native WGPU knife/loop/bevel workflow with input confined to egui"]
#[cfg(target_os = "windows")]
fn native_cut_bevel_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.2.0/m5");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-cuts-qa-{}", new_id()));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    let path = root.join("project.oxy.json");
    let mut project =
        oxy_core::editing::blank_project("Cortes e arredondamento", SceneKind::ThreeD).unwrap();
    let mut cube = Entity::new("Cubo de cortes", Some(Primitive::Cube));
    cube.collider = Some(Collider::default());
    let id = new_id();
    cube.material.texture = Some(id.clone());
    let mut paint = PaintImage::new(256, 256, [110, 180, 210, 255]).unwrap();
    for y in 0..256 {
        for x in 0..256 {
            if (x / 16 + y / 16) % 2 == 0 {
                paint.set_pixel(x, y, [225, 220, 190, 255]);
            }
        }
    }
    paint.save(&root.join("assets/paint.png")).unwrap();
    project.assets.push(Asset {
        id,
        name: "Pintura anterior".into(),
        kind: AssetKind::Texture,
        path: "assets/paint.png".into(),
        model: None,
    });
    project.scenes[0].entities.push(cube);
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA cortes e arredondamento")
            .with_inner_size([1440., 900.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|b| {
            b.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA cortes",
        options,
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Cubo de cortes"),
                Action::Click("Estúdio"),
                Action::Click("Enquadrar"),
                Action::Check("m5_base"),
                Action::Chord(Key::K, Modifiers::SHIFT),
                Action::WorldClick([-0.5, 0., 0.5]),
                Action::WorldClick([0.5, 0., 0.5]),
                Action::Check("m5_knife_one"),
                Action::WorldClick([0.5, 0., -0.5]),
                Action::Check("m5_knife_two"),
                Action::Screenshot("knife-path.png"),
                Action::Chord(Key::Escape, Modifiers::ALT | Modifiers::SHIFT),
                Action::Check("m5_cancel"),
                Action::Chord(Key::K, Modifiers::SHIFT),
                Action::WorldClick([-0.5, 0., 0.5]),
                Action::WorldClick([0.5, 0., 0.5]),
                Action::WorldClick([0.5, 0., -0.5]),
                Action::Click("Confirmar (Enter)"),
                Action::Check("m5_committed"),
                Action::Key(Key::Z, true),
                Action::Check("m5_cancel"),
                Action::Key(Key::Y, true),
                Action::Check("m5_redo"),
                Action::Key(Key::Z, true),
                Action::Key(Key::Num3, false),
                Action::ComponentClick([0.5, 0., 0.5], false),
                Action::Chord(Key::R, Modifiers::SHIFT),
                Action::Check("m5_loop"),
                Action::Screenshot("loop-preview.png"),
                Action::EditValue("Quantidade de cortes", "2"),
                Action::Check("m5_loop_confirmed"),
                Action::Key(Key::Z, true),
                Action::Check("m5_cancel"),
                Action::Key(Key::Num3, false),
                Action::ComponentClick([0.5, 0., 0.5], false),
                Action::Chord(Key::B, Modifiers::SHIFT),
                Action::Click("4"),
                Action::Click("Criar cópia ampliada (2×)"),
                Action::Check("m5_bevel"),
                Action::Screenshot("bevel-edge.png"),
                Action::Click("Confirmar (Enter)"),
                Action::Check("m5_committed"),
                Action::Key(Key::Z, true),
                Action::Check("m5_cancel"),
                Action::Key(Key::Y, true),
                Action::Check("m5_redo"),
                Action::Key(Key::Z, true),
                Action::Key(Key::Num4, false),
                Action::ComponentClick([0.5, 0.5, 0.5], false),
                Action::Chord(Key::B, Modifiers::SHIFT),
                Action::Click("4"),
                Action::Click("Criar cópia ampliada (2×)"),
                Action::Check("m5_vertex"),
                Action::Screenshot("bevel-vertex.png"),
                Action::Click("Confirmar (Enter)"),
                Action::Check("m5_committed"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::Screenshot("reopened-model.png"),
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
    std::fs::write(output.join("native-cuts.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
