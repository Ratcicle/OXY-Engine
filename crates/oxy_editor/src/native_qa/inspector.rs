use super::*;
use oxy_core::document::*;

impl NativeQa {
    pub(super) fn check_inspector(&mut self, label: &str) -> Result<(), String> {
        let edge = self
            .surface
            .texts
            .iter()
            .find(|t| t.text == "PROPRIEDADES")
            .ok_or("Inspetor ausente")?
            .rect
            .left();
        let labels: Vec<_> = self
            .surface
            .texts
            .iter()
            .filter(|t| t.rect.left() >= edge - 1.)
            .map(|t| t.text.as_str())
            .collect();
        let has = |text: &str| labels.contains(&text);
        let require = |condition: bool, message: &str| {
            if condition {
                Ok(())
            } else {
                Err(format!("{message}: {labels:?}"))
            }
        };
        for forbidden in [
            "Pai (opção avançada)",
            "Lógica",
            "Animação",
            "Salvar hierarquia como modelo",
            "Duplicar",
            "Agrupar",
            "Excluir",
            "Compatibilidade 0.2.1",
            "Controlador de movimento",
            "Câmera de personagem",
        ] {
            if has(forbidden) && label != "i031_context" {
                return Err(format!("Comando proibido no Inspetor: {forbidden}"));
            }
        }
        let selected = self
            .editor
            .selected
            .as_deref()
            .and_then(|id| self.editor.scene().entity(id));
        match label {
            "i031_camera_empty" => require(
                selected.is_some_and(|e| {
                    e.camera.is_some() && e.camera_rig.is_none() && e.parent.is_none()
                }) && has("Câmera")
                    && !has("Controle da câmera")
                    && !has("Personagem 3D")
                    && !has("Colisor 3D")
                    && !has("Plataforma móvel"),
                "Criar câmera ofereceu componentes alheios ou criou controle implícito",
            ),
            "i031_camera_menu" => require(
                has("Controle da câmera")
                    && !has("Personagem 3D")
                    && !has("Colisor 3D")
                    && !has("Plataforma móvel")
                    && !has("Área de detecção 3D"),
                "Menu da câmera ofereceu física",
            ),
            "i031_empty" => require(
                !has("Personagem 3D")
                    && !has("Plataforma móvel")
                    && !has("Controle da câmera")
                    && !has("Colisor 3D")
                    && has("+ Adicionar componente"),
                "Objeto vazio mostra capacidades ausentes",
            ),
            "i031_menu" => require(
                has("Personagem 3D")
                    && has("Plataforma móvel")
                    && !has("Controle da câmera")
                    && !has("Câmera"),
                "Menu de objeto incompatível",
            ),
            "i031_platform" => require(
                selected.is_some_and(|e| e.platform.is_some() && e.physics3d.is_some())
                    && has("Plataforma móvel")
                    && !has("Controle da câmera")
                    && !has("Personagem 3D"),
                "Plataforma não foi criada contextualizada",
            ),
            "i031_platform_removed" => require(
                selected.is_some_and(|e| e.platform.is_none() && e.physics3d.is_some())
                    && !has("Plataforma móvel"),
                "Remover plataforma afetou outro componente",
            ),
            "i031_preset" => {
                let scene = self.editor.scene();
                let body = scene
                    .entities
                    .iter()
                    .find(|e| e.character3d.is_some())
                    .ok_or("Jogador ausente")?;
                let camera = scene
                    .entities
                    .iter()
                    .find(|e| e.camera_rig.is_some())
                    .ok_or("Câmera ausente")?;
                require(
                    body.camera.is_none()
                        && camera.character3d.is_none()
                        && camera.physics3d.is_none()
                        && camera.parent.is_none()
                        && camera.camera_rig.as_ref().unwrap().target.as_ref() == Some(&body.id),
                    "Preset não separou responsabilidades",
                )
            }
            "i031_no_preset" => require(
                self.editor
                    .scene()
                    .entities
                    .iter()
                    .all(|e| e.character3d.is_none() && e.camera.is_none()),
                "Undo da montagem incompleto",
            ),
            "i031_player" => require(
                has("Personagem 3D")
                    && !has("Colisor 3D")
                    && selected.is_some_and(|e| e.physics3d.is_none())
                    && !has("Controle da câmera")
                    && !has("Câmera")
                    && !has("Plataforma móvel"),
                "Jogador contém seções alheias",
            ),
            "i031_groups" => require(
                [
                    "Básico",
                    "Corpo de movimento",
                    "Movimento no chão",
                    "Movimento no ar",
                    "Pulo",
                    "Corrida",
                    "Agachamento/deslize",
                    "Avançado",
                ]
                .iter()
                .all(|s| has(s)),
                "Agrupamentos de movimento ausentes",
            ),
            "i031_camera" => require(
                has("Câmera")
                    && has("Controle da câmera")
                    && !has("Personagem 3D")
                    && !has("Colisor 3D")
                    && !has("Plataforma móvel"),
                "Câmera mostra componentes incompatíveis",
            ),
            "i031_target" => require(
                has("Alvo")
                    && has("Jogador")
                    && selected
                        .is_some_and(|e| e.camera_rig.as_ref().is_some_and(|r| r.target.is_some())),
                "Alvo explícito ausente",
            ),
            "i031_before_parent" => {
                self.base = Some(self.editor.state.clone());
                Ok(())
            }
            "i031_parent" => {
                let scene = self.editor.scene();
                let piece = scene
                    .entities
                    .iter()
                    .find(|e| e.name == "Peça de teste")
                    .unwrap();
                let parent = scene
                    .entities
                    .iter()
                    .find(|e| e.name == "Grupo de teste")
                    .unwrap();
                let base = self
                    .base
                    .as_ref()
                    .unwrap()
                    .project
                    .scene(&scene.id)
                    .unwrap();
                require(
                    piece.parent.as_ref() == Some(&parent.id)
                        && base
                            .world_matrix(&piece.id)?
                            .abs_diff_eq(scene.world_matrix(&piece.id)?, 0.0001),
                    "Drag-and-drop alterou estrutura/posição incorretamente",
                )
            }
            "i031_context" => {
                let all: Vec<_> = self.surface.texts.iter().map(|t| t.text.as_str()).collect();
                require(
                    [
                        "Renomear  F2",
                        "Duplicar hierarquia",
                        "Agrupar",
                        "Excluir hierarquia",
                        "Salvar hierarquia como modelo",
                    ]
                    .iter()
                    .all(|s| all.contains(s)),
                    "Menu estrutural incompleto",
                )
            }
            "i031_model" => require(
                self.editor.state.project.assets.iter().any(|a| {
                    a.kind == AssetKind::Model && a.model.as_ref().is_some_and(|m| m.len() == 2)
                }),
                "Modelo não preservou subárvore",
            ),
            "i031_no_model" => require(
                self.editor.state.project.assets.is_empty(),
                "Undo não removeu modelo",
            ),
            "i031_renamed" => {
                let original = self.base.as_ref().unwrap().project.scenes[0]
                    .entities
                    .iter()
                    .find(|e| e.name == "Grupo de teste")
                    .unwrap();
                require(
                    self.editor
                        .scene()
                        .entity(&original.id)
                        .is_some_and(|e| e.name == "Articulação renomeada"),
                    "F2 alterou identidade ou não renomeou",
                )
            }
            "i031_original" => require(
                self.editor.scene().entities.len()
                    == self.base.as_ref().unwrap().project.scenes[0].entities.len(),
                "Duplicar/Excluir corrompeu quantidade",
            ),
            _ => Err(format!("Verificação desconhecida: {label}")),
        }
    }
}

#[test]
#[ignore = "Native WGPU inspector, hierarchy and isolated input"]
#[cfg(target_os = "windows")]
fn native_inspector_v031() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa/v0.3.1/inspector");
    let folder = output.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("project.oxy.json");
    let mut p = Project::new("Inspetor e responsabilidades");
    p.scenes[0].kind = SceneKind::ThreeD;
    p.scenes[0].name = "Autoria 3D".into();
    let group = Entity::new("Grupo de teste", None);
    let mut piece = Entity::new("Peça de teste", Some(Primitive::Cube));
    piece.transform.position = [2., 1., 0.];
    p.scenes[0].entities = vec![group, piece];
    oxy_core::persistence::save_project(&path, &p).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    eframe::run_native(
        "OXY Engine 0.3.1 — Inspetor",
        eframe::NativeOptions {
            persist_window: false,
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 900.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut qa = NativeQa::new(cc, path, artifacts, shared);
            qa.actions = VecDeque::from([
                Action::Click("+ Objeto"),
                Action::Click("Câmera"),
                Action::Check("i031_camera_empty"),
                Action::Click("+ Adicionar componente"),
                Action::Check("i031_camera_menu"),
                Action::Click("Controle da câmera"),
                Action::Check("i031_camera"),
                Action::Key(Key::Z, true),
                Action::Check("i031_camera_empty"),
                Action::Key(Key::Y, true),
                Action::Check("i031_camera"),
                Action::Key(Key::Delete, false),
                Action::Check("i031_no_preset"),
                Action::SelectEntity("Peça de teste"),
                Action::Check("i031_empty"),
                Action::Screenshot("object-clean.png"),
                Action::Click("+ Adicionar componente"),
                Action::Check("i031_menu"),
                Action::Click("Plataforma móvel"),
                Action::Check("i031_platform"),
                Action::Click("Plataforma móvel"),
                Action::Click("Remover componente"),
                Action::Check("i031_platform_removed"),
                Action::Key(Key::Z, true),
                Action::Check("i031_platform"),
                Action::Key(Key::Z, true),
                Action::Check("i031_empty"),
                Action::Key(Key::Y, true),
                Action::Check("i031_platform"),
                Action::Key(Key::Z, true),
                Action::Click("+ Objeto"),
                Action::Click("Personagem em primeira pessoa"),
                Action::Check("i031_preset"),
                Action::Key(Key::Z, true),
                Action::Check("i031_no_preset"),
                Action::Key(Key::Y, true),
                Action::Check("i031_preset"),
                Action::SelectEntity("Jogador"),
                Action::Check("i031_player"),
                Action::Click("Personagem 3D"),
                Action::Check("i031_groups"),
                Action::Screenshot("player-inspector.png"),
                Action::SelectEntity("Câmera principal"),
                Action::Check("i031_camera"),
                Action::Click("Controle da câmera"),
                Action::Click("Básico"),
                Action::Check("i031_target"),
                Action::Screenshot("camera-inspector.png"),
                Action::Resize(Vec2::new(920., 600.)),
                Action::Check("i031_camera"),
                Action::Screenshot("camera-small.png"),
                Action::Resize(Vec2::new(1280., 900.)),
                Action::SelectEntity("Jogador"),
                Action::Check("i031_groups"),
                Action::Check("i031_before_parent"),
                Action::Reparent("Peça de teste", "Grupo de teste"),
                Action::Check("i031_parent"),
                Action::SelectEntity("Grupo de teste"),
                Action::ContextEntity("Grupo de teste"),
                Action::Check("i031_context"),
                Action::Click("Salvar hierarquia como modelo"),
                Action::Check("i031_model"),
                Action::Key(Key::Z, true),
                Action::Check("i031_no_model"),
                Action::Key(Key::Y, true),
                Action::Check("i031_model"),
                Action::Key(Key::F2, false),
                Action::Key(Key::A, true),
                Action::Text("Articulação renomeada"),
                Action::Key(Key::Enter, false),
                Action::Check("i031_renamed"),
                Action::Key(Key::D, true),
                Action::Key(Key::Delete, false),
                Action::Check("i031_original"),
                Action::Key(Key::Z, true),
                Action::Key(Key::Y, true),
                Action::Check("i031_original"),
                Action::Key(Key::S, true),
                Action::ReopenProject,
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let text = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-inspector.txt"), &text).unwrap();
    assert!(report.done && report.error.is_none(), "{text}");
}
