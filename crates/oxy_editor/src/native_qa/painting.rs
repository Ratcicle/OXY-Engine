use super::*;
use oxy_core::{
    document::*,
    geometry::{
        self,
        selection::{Mode, Selection},
    },
    painting::PaintImage,
};
impl NativeQa {
    pub(super) fn check_paint_mesh(&mut self, label: &str) -> Result<(), String> {
        let selected = self.editor.selected.as_ref().ok_or("Seleção ausente")?;
        let entity = self.editor.scene().entity(selected).unwrap();
        let texture = entity.material.texture.as_ref().ok_or("Textura ausente")?;
        let pixels = self
            .editor
            .state
            .images
            .get(texture)
            .ok_or("Pixels ausentes")?;
        let (old, new) = self.paint_faces.unwrap();
        match label {
            "m6_shared" => {
                if !self
                    .surface
                    .texts
                    .iter()
                    .any(|t| t.text.contains("2 peças"))
                {
                    return Err("Escolha de textura compartilhada não apareceu".into());
                }
            }
            "m6_base" => {
                self.base = Some(self.editor.state.clone());
                self.initial_entities = self.editor.qa_mesh_info().3;
                self.paint_uploads = self.editor.studio.canvas_uploads;
            }
            "m6_selected_old" | "m6_selected_new" => {
                let expected = if label.ends_with("old") { old } else { new };
                if self.editor.mesh_components().mode != Mode::Face
                    || self.editor.mesh_components().ids != [expected]
                {
                    return Err(format!(
                        "Face selecionada incorreta: {:?}, esperado {expected}",
                        self.editor.mesh_components()
                    ));
                }
            }
            "m6_cached" => {
                if self.paint_uploads != self.editor.studio.canvas_uploads {
                    return Err("Seleção/repouso reenviou os mesmos pixels à GPU".into());
                }
            }
            "m6_painted" => {
                let before = self.base.as_ref().unwrap();
                let original = before.images.get(texture).unwrap();
                let mesh = entity.mesh.as_ref().unwrap();
                let uv = |id| {
                    let f = mesh.face(id).unwrap();
                    (f.corners
                        .iter()
                        .map(|c| glam::Vec2::from(c.uv))
                        .sum::<glam::Vec2>()
                        / f.corners.len() as f32)
                        .to_array()
                };
                if pixels.sample_uv(uv(new)) == original.sample_uv(uv(new))
                    || pixels.sample_uv(uv(old)) != original.sample_uv(uv(old))
                {
                    return Err(
                        "Pintura da face nova não alterou seus pixels de forma independente".into(),
                    );
                }
                let other = self
                    .editor
                    .scene()
                    .entities
                    .iter()
                    .find(|e| e.name == "Outra instância")
                    .unwrap();
                let other_id = other.material.texture.as_ref().unwrap();
                if other_id == texture
                    || self.editor.state.images.get(other_id) != before.images.get(other_id)
                {
                    return Err("Pintura alterou a outra instância".into());
                }
                if self.editor.qa_mesh_info().3 != self.initial_entities + 1 {
                    return Err("Pincelada não gerou um único Undo".into());
                }
                self.after_paint = Some(self.editor.state.clone());
            }
            "m6_undo" => {
                if self.base.as_ref() != Some(&self.editor.state) {
                    return Err("Undo perdeu dados de malha/pixels".into());
                }
            }
            "m6_redo" => {
                if self.after_paint.as_ref() != Some(&self.editor.state) {
                    return Err("Redo perdeu dados de malha/pixels".into());
                }
            }
            _ => return Err(format!("Check desconhecido: {label}")),
        }
        Ok(())
    }
}
#[test]
#[ignore = "Native WGPU UV islands, face selection, real pixels and upload-cache workflow"]
#[cfg(target_os = "windows")]
fn native_mesh_painting_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.2.0/m6");
    std::fs::create_dir_all(&output).unwrap();
    let root = std::env::temp_dir().join(format!("oxy-paint-mesh-{}", new_id()));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    let path = root.join("project.oxy.json");
    let cube = geometry::primitives::generate(Primitive::Cube, 8, Default::default()).unwrap();
    let edge = cube
        .data()
        .edges
        .iter()
        .find(|e| {
            e.vertices.iter().all(|&id| {
                let p = cube.position(id).unwrap();
                p.x == 0.5 && p.z == 0.5
            })
        })
        .unwrap()
        .id;
    let bevel = geometry::bevel::apply(
        &cube,
        &Selection {
            mode: Mode::Edge,
            ids: vec![edge],
            through: false,
        },
        0.18,
        4,
    )
    .unwrap();
    let mapped = geometry::atlas::allocate(&bevel.mesh, &bevel.new_faces, [256; 2], 2)
        .unwrap()
        .mesh;
    let old = cube
        .data()
        .faces
        .iter()
        .enumerate()
        .find(|(i, _)| cube.prepared().face_normals[*i].z > 0.9)
        .unwrap()
        .1
        .id;
    let new = bevel.new_faces[1];
    let mut project =
        oxy_core::editing::blank_project("Pintura da geometria editada", SceneKind::ThreeD)
            .unwrap();
    let texture = new_id();
    project.assets.push(Asset {
        id: texture.clone(),
        name: "Atlas compartilhado".into(),
        kind: AssetKind::Texture,
        path: "assets/atlas.png".into(),
        model: None,
    });
    let paint = PaintImage::new(512, 512, [210, 205, 185, 255]).unwrap();
    paint.save(&root.join("assets/atlas.png")).unwrap();
    let mut e = Entity::new("Peça pintável", None);
    e.mesh = Some(mapped);
    e.material.texture = Some(texture);
    project.scenes[0].entities.push(e.clone());
    e.id = new_id();
    e.name = "Outra instância".into();
    e.transform.position[0] = -3.;
    project.scenes[0].entities.push(e);
    oxy_core::persistence::save_project(&path, &project).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine — QA UV e pintura",
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
            qa.paint_faces = Some((old, new));
            qa.actions = VecDeque::from([
                Action::SelectEntity("Peça pintável"),
                Action::Click("Estúdio"),
                Action::Click("Pintura"),
                Action::Click("Enquadrar peça"),
                Action::Check("m6_shared"),
                Action::Click("Criar cópia independente"),
                Action::Wait(5),
                Action::Check("m6_base"),
                Action::Click("Selecionar faces"),
                Action::PaintFace {
                    new: false,
                    image: true,
                },
                Action::Check("m6_selected_old"),
                Action::Screenshot("uv-old-face.png"),
                Action::PaintFace {
                    new: true,
                    image: false,
                },
                Action::Check("m6_selected_new"),
                Action::Screenshot("uv-new-face.png"),
                Action::Idle,
                Action::Check("m6_cached"),
                Action::Click("Pincel"),
                Action::PaintFace {
                    new: true,
                    image: false,
                },
                Action::Wait(5),
                Action::Check("m6_painted"),
                Action::Screenshot("new-face-painted.png"),
                Action::Key(Key::Z, true),
                Action::Check("m6_undo"),
                Action::Key(Key::Y, true),
                Action::Check("m6_redo"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
                Action::SelectEntity("Peça pintável"),
                Action::Click("Estúdio"),
                Action::Click("Pintura"),
                Action::Click("Enquadrar peça"),
                Action::Screenshot("painted-reopened.png"),
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
    std::fs::write(output.join("native-mesh-painting.txt"), &text).unwrap();
    assert!(r.done && r.error.is_none(), "{text}");
}
