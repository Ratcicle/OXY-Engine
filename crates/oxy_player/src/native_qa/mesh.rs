//! Runs the document authored by the final editor workflow, using the actual player host.
use super::*;
use oxy_core::document::Project;

struct MeshPlayerQa {
    player: Player,
    base: Project,
    original_bytes: Vec<u8>,
    output: PathBuf,
    report: Arc<Mutex<Report>>,
    started: Instant,
    animated: bool,
    screenshot_requested: bool,
    screenshot_received: bool,
    finished: bool,
}
impl MeshPlayerQa {
    fn fail(&mut self, ctx: &egui::Context, error: String) {
        self.report.lock().unwrap().error = Some(error);
        self.finished = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn validate_runtime(&mut self) -> Result<(), String> {
        if let Some(error) = &self.player.error {
            return Err(error.clone());
        }
        if !self.player.diagnostics.is_empty() {
            return Err(format!("{:?}", self.player.diagnostics));
        }
        let runtime = self.player.runtime.as_ref().ok_or("Runtime ausente")?;
        let original = self.base.scene(&self.base.start_scene).unwrap();
        for entity in &original.entities {
            let active = runtime
                .scene()
                .entity(&entity.id)
                .ok_or("Objeto desapareceu no player")?;
            if active.mesh != entity.mesh
                || active.material != entity.material
                || active.transform.pivot != entity.transform.pivot
                || active.collider != entity.collider
            {
                return Err(
                    "Runtime alterou a malha, material, pivô ou parâmetros de colisão".into(),
                );
            }
            for clip in &entity.clips {
                for track in &clip.tracks {
                    let before = original.entity(&track.target).ok_or("Trilha sem peça")?;
                    let after = runtime
                        .scene()
                        .entity(&track.target)
                        .ok_or("Peça animada ausente")?;
                    let a = before.transform.matrix().to_cols_array();
                    let b = after.transform.matrix().to_cols_array();
                    if a.iter().zip(b).any(|(a, b)| (a - b).abs() > 0.001) {
                        self.animated = true;
                    }
                }
            }
        }
        if runtime
            .logs
            .iter()
            .any(|line| line.contains("Comportamento interrompido"))
        {
            return Err("Runtime interrompeu o grafo da peça".into());
        }
        if std::fs::read(&self.player.path).map_err(|e| e.to_string())? != self.original_bytes {
            return Err("Player modificou o documento portátil".into());
        }
        Ok(())
    }
}
impl eframe::App for MeshPlayerQa {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        input.focused = true;
        input.modifiers = Modifiers::NONE;
        input.hovered_files.clear();
        input.dropped_files.clear();
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.focused = Some(true);
        }
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.finished {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.started.elapsed() > Duration::from_secs(30) {
            self.fail(ctx, "QA de malha no player excedeu 30 segundos".into());
            return;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = image {
            let colors: HashSet<_> = image.pixels.iter().map(|p| p.to_array()).collect();
            if image.width() < 640 || image.height() < 360 || colors.len() < 30 {
                self.fail(ctx, "Captura do player vazia/sem geometria pintada".into());
                return;
            }
            let png = PaintImage {
                width: image.width() as u32,
                height: image.height() as u32,
                pixels: image.pixels.iter().flat_map(|p| p.to_array()).collect(),
            };
            if let Err(e) = png.save(&self.output.join("final-mesh-player.png")) {
                self.fail(ctx, e);
                return;
            }
            self.screenshot_received = true;
        }
        self.player.update(ctx, frame);
        if let Err(e) = self.validate_runtime() {
            self.fail(ctx, e);
            return;
        }
        let runtime = self.player.runtime.as_ref().unwrap();
        if runtime.time >= 0.3 && self.animated && !self.screenshot_requested {
            let stats = self.player.renderer.as_ref().unwrap().stats();
            if stats.triangles == 0 || stats.textures == 0 || stats.visible_objects < 2 {
                self.fail(
                    ctx,
                    "Player não desenhou as instâncias e sua textura".into(),
                );
                return;
            }
            self.report.lock().unwrap().steps.push(format!("Runtime em {:.3}s; {} objetos, {} triângulos, {} texturas GPU; quadro intermediário moveu a peça sem mudar malha/UV/pivô/colisor",runtime.time,stats.visible_objects,stats.triangles,stats.textures));
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.screenshot_requested = true;
        }
        if self.screenshot_received && runtime.time >= 0.65 {
            let reopened = persistence::load_project(&self.player.path).unwrap();
            if reopened != self.base {
                self.fail(ctx, "Salvar/reabrir após player alterou a peça".into());
                return;
            }
            let mut report = self.report.lock().unwrap();
            report.done = true;
            report.screenshot = true;
            report.steps.push("Cópia fora do repositório; grafo de início reproduziu animação rígida, captura GPU real, arquivo e vínculos intactos após reabrir".into());
            self.finished = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

#[test]
#[ignore = "Run after native_final_mesh_workflow; opens isolated inactive WGPU player"]
fn native_player_final_authored_mesh() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = std::env::var_os("OXY_NATIVE_MESH_PROJECT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("qa/v0.2.1/m7/full-flow/project"));
    assert!(
        source.join(persistence::PROJECT_FILE).is_file(),
        "Execute primeiro o fluxo nativo final do editor"
    );
    let fixture = std::env::temp_dir().join(format!("oxy-final-mesh-player-{}", new_id()));
    copy_directory(&source, &fixture).unwrap();
    let path = fixture.join(persistence::PROJECT_FILE);
    let base = persistence::load_project(&path).unwrap();
    let scene = base.scene(&base.start_scene).unwrap();
    assert!(
        scene.entities.iter().filter(|e| e.mesh.is_some()).count() >= 2,
        "O fluxo final precisa de modelo editado e instância"
    );
    assert!(
        scene.entities.iter().any(|e| !e.clips.is_empty()
            && e.graph
                .nodes
                .iter()
                .any(|n| n.operation == "event.scene_start")
            && e.graph
                .nodes
                .iter()
                .any(|n| n.operation == "action.animation")),
        "A animação precisa iniciar pelo grafo comum do documento"
    );
    for asset in &base.assets {
        if !asset.path.is_empty() {
            assert!(
                persistence::resolve_asset_path(&fixture, &asset.path)
                    .unwrap()
                    .is_file()
            );
        }
    }
    let original_bytes = std::fs::read(&path).unwrap();
    let output = root.join("qa/v0.2.1/m7");
    std::fs::create_dir_all(&output).unwrap();
    let artifacts = output.clone();
    let report = Arc::new(Mutex::new(Report::default()));
    let result = report.clone();
    eframe::run_native(
        "OXY Player — malha editada",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 720.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(MeshPlayerQa {
                player: Player::new(cc, path),
                base,
                original_bytes,
                output: artifacts,
                report: result,
                started: Instant::now(),
                animated: false,
                screenshot_requested: false,
                screenshot_received: false,
                finished: false,
            }))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\nCaptura GPU: {}\n{}\n",
        report.done,
        report.error,
        report.screenshot,
        report.steps.join("\n")
    );
    std::fs::write(output.join("final-mesh-player-qa.txt"), &summary).unwrap();
    assert!(
        report.done && report.screenshot && report.error.is_none(),
        "{summary}"
    );
}
