use super::*;
use oxy_core::{animation::Clip, document::*, painting::PaintImage};
impl NativeQa {
    pub(super) fn check_layout(&mut self, label: &str) -> Result<(), String> {
        match label {
            "l7_viewport" => {
                let r = self
                    .surface
                    .native_viewport
                    .ok_or("Viewport não está acessível")?;
                self.report.lock().unwrap().steps.push(format!(
                    "Viewport {:.1} × {:.1} pontos; escala {:.2}",
                    r.width(),
                    r.height(),
                    self.editor.scale
                ));
                if r.width() * self.editor.scale < 240. || r.height() * self.editor.scale < 120. {
                    return Err(format!("Viewport pequeno demais para manipulação: {r:?}"));
                }
            }
            "l7_selection" => {
                if self.editor.qa_mesh_info().1 != 1 {
                    return Err(
                        "Clique após redimensionar/alterar escala não selecionou a face".into(),
                    );
                }
            }
            "l7_guide" => {
                if self.find("Fechar guia", false).is_none() {
                    return Err("Guia não oferece saída visível".into());
                }
            }
            "l7_key" => {
                let group = self
                    .editor
                    .scene()
                    .entities
                    .iter()
                    .find(|e| e.name == "Conjunto")
                    .unwrap();
                if group.clips[0].tracks.len() != 1 || group.clips[0].tracks[0].keyframes.len() != 1
                {
                    return Err("Gravar quadro na tela pequena não alterou a trilha".into());
                }
            }
            "l7_tracks" => {
                if self.find("Tubo", false).is_none() {
                    return Err("Trilha filha não ficou acessível pela rolagem".into());
                }
            }
            "l7_track_selected" => {
                if self
                    .editor
                    .selected
                    .as_deref()
                    .and_then(|id| self.editor.scene().entity(id))
                    .is_none_or(|e| e.name != "Tubo")
                {
                    return Err("Clique na trilha filha não selecionou a peça correta".into());
                }
            }
            "l7_refusal" => {
                if self.editor.qa_console_open()
                    || self.editor.state != *self.base.as_ref().ok_or("Sem referência")?
                    || self.ux_viewport != self.surface.native_viewport
                    || self
                        .editor
                        .messages
                        .last()
                        .is_none_or(|m| !m.contains("não está disponível neste modo"))
                {
                    return Err("Recusa não preservou documento, viewport ou aviso".into());
                }
            }
            _ => return Err(format!("Check desconhecido {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native six-size/scale combinations, modeling, painting, animation and guide"]
#[cfg(target_os = "windows")]
fn native_layout_matrix_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.2.0/m7");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-layout-{}", new_id()));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    let path = root.join("project.oxy.json");
    let mut p = oxy_core::editing::blank_project("Matriz de interface", SceneKind::ThreeD).unwrap();
    let mut joint = Entity::new("Conjunto", None);
    joint.clips.push(Clip::new("Parado"));
    let mut tube = Entity::new("Tubo", Some(Primitive::Tube));
    tube.parent = Some(joint.id.clone());
    tube.segments = 8;
    tube.primitive_parameters = Some(Default::default());
    tube.collider = Some(Collider::default());
    let texture = new_id();
    tube.material.texture = Some(texture.clone());
    p.assets.push(Asset {
        id: texture,
        name: "Pintura do tubo".into(),
        path: "assets/tubo.png".into(),
        kind: AssetKind::Texture,
        model: None,
    });
    PaintImage::new(256, 256, [210, 190, 145, 255])
        .unwrap()
        .save(&root.join("assets/tubo.png"))
        .unwrap();
    p.scenes[0].entities.extend([joint, tube]);
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA escala e espaço",
        eframe::NativeOptions {
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
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::SelectEntity("Tubo"),
                Action::Click("Estúdio"),
                Action::Check("ux_base"),
            ]);
            let mut applied = 1.;
            for (size, scale, model, paint) in [
                ([1440., 900.], 0.8, "1440-80-model.png", "1440-80-paint.png"),
                (
                    [1440., 900.],
                    1.,
                    "1440-100-model.png",
                    "1440-100-paint.png",
                ),
                (
                    [1440., 900.],
                    1.6,
                    "1440-160-model.png",
                    "1440-160-paint.png",
                ),
                ([920., 600.], 0.8, "920-80-model.png", "920-80-paint.png"),
                ([920., 600.], 1., "920-100-model.png", "920-100-paint.png"),
                ([920., 600.], 1.6, "920-160-model.png", "920-160-paint.png"),
            ] {
                let interface = if applied == 0.8 {
                    "Interface: 80%"
                } else if applied == 1.6 {
                    "Interface: 160%"
                } else {
                    "Interface: 100%"
                };
                let check = if applied == 0.8 {
                    "ux_scale_80"
                } else if applied == 1.6 {
                    "ux_scale_160"
                } else {
                    "ux_scale_100"
                };
                let target = if scale == 0.8 {
                    "80%"
                } else if scale == 1.6 {
                    "160%"
                } else {
                    "100%"
                };
                qa.actions.extend([
                    Action::ResizePhysical(Vec2::from(size)),
                    Action::Click(interface),
                    Action::Click("125%"),
                    Action::Check(check),
                    Action::Click("Cancelar"),
                    Action::Check(check),
                    Action::Click(interface),
                    Action::Click(target),
                    Action::Click("Aplicar"),
                    Action::Click("Estúdio"),
                    Action::Click("Modelagem"),
                    Action::Key(Key::Num2, false),
                    Action::Click("Enquadrar"),
                    Action::Screenshot(model),
                    Action::Check("l7_viewport"),
                    Action::ComponentClick([0.1767767, 0.25, 0.4267767], false),
                    Action::Check("l7_selection"),
                    Action::Check("ux_viewport"),
                    Action::Chord(Key::B, Modifiers::SHIFT),
                    Action::Check("l7_refusal"),
                    Action::PanZoom,
                    Action::ComponentClick([0.1767767, 0.25, 0.4267767], false),
                    Action::Check("l7_selection"),
                    Action::OptionalClick("Dispensar"),
                    Action::Click("Pintura"),
                    Action::Click("Enquadrar peça"),
                    Action::Screenshot(paint),
                    Action::Check("l7_viewport"),
                    Action::Click("Animação"),
                    Action::Check("l7_viewport"),
                    Action::Click("Lógica"),
                    Action::Click("Guia de lógica visual"),
                    Action::Check("l7_guide"),
                    Action::Click("Fechar guia"),
                    Action::Click("Projeto"),
                    Action::Click("Novo projeto"),
                    Action::Click("Cancelar"),
                    Action::Check("ux_unchanged"),
                ]);
                applied = scale;
            }
            qa.actions.extend([
                Action::Click("Estúdio"),
                Action::Click("Animação"),
                Action::OptionalClick("Dispensar"),
                Action::Click("+ Quadro-chave"),
                Action::Check("l7_key"),
                Action::ScrollAt("Tempo (s)", -150.),
                Action::Screenshot("920-160-animation.png"),
                Action::Check("l7_tracks"),
                Action::Click("Tubo"),
                Action::Check("l7_track_selected"),
                Action::Check("l7_viewport"),
                Action::Key(Key::Z, true),
                Action::Check("ux_unchanged"),
                Action::Click("Lógica"),
                Action::Click("Guia de lógica visual"),
                Action::Screenshot("920-160-guide.png"),
                Action::Click("Fechar guia"),
                Action::Click("Estúdio"),
                Action::Click("Modelagem"),
                Action::Click("+ Objeto"),
                Action::Click("Tubo"),
                Action::Screenshot("920-160-primitive.png"),
                Action::Chord(Key::Escape, Modifiers::ALT | Modifiers::SHIFT),
                Action::Check("ux_unchanged"),
                Action::Click("Interface: 160%"),
                Action::Click("80%"),
                Action::Screenshot("920-160-pending-preference.png"),
                Action::Click("Cancelar"),
                Action::Check("ux_scale_160"),
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
    std::fs::write(output.join("native-layout-matrix.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
