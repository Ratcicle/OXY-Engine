use super::*;

#[test]
#[ignore = "Opens the native home screen; input remains isolated in egui"]
#[cfg(target_os = "windows")]
fn native_portable_home_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Release deliberately has no source-directory fallback. Exercise the same
    // adjacent data layout as the portable ZIP; never overwrite existing data.
    if !cfg!(debug_assertions) {
        let executable = std::env::current_exe().unwrap();
        let data = executable.parent().unwrap().join("data");
        if !data.exists() {
            copy_directory(&workspace.join("examples/validacao"), &data).unwrap();
        }
    }
    let output = workspace.join("qa/v0.2.1/portable");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        persist_window: false,
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA portátil")
            .with_icon(crate::window_icon())
            .with_inner_size([920., 600.])
            .with_active(false),
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "OXY Engine — QA portátil",
        options,
        Box::new(move |cc| {
            let mut qa = NativeQa::new(
                cc,
                workspace.join("examples/validacao/project.oxy.json"),
                artifacts,
                shared,
            );
            qa.editor = Editor::new(cc);
            qa.actions = VecDeque::from([
                Action::Check("home_start"),
                Action::Screenshot("home.png"),
                Action::Idle,
                Action::Click("Novo projeto"),
                Action::Click("Criar projeto"),
                Action::Check("home_new"),
                Action::Click("Projeto"),
                Action::Click("Tela inicial"),
                Action::Click("Projeto de exemplo"),
                Action::Check("home_example"),
                Action::Check("play_baseline"),
                Action::Click("▶ Jogar"),
                Action::Check("playing"),
                Action::Screenshot("example-game.png"),
                Action::Click("■ Parar"),
                Action::Check("stop_isolated"),
                Action::Click("Projeto"),
                Action::Click("Tela inicial"),
                Action::Screenshot("recent-project.png"),
                Action::Click("Minha cópia OXY"),
                Action::Check("home_recent"),
                Action::Check("home_bad_open"),
            ]);
            Ok(Box::new(qa))
        }),
    )
    .unwrap();
    let report = report.lock().unwrap();
    let summary = format!(
        "Concluído: {}\nErro: {:?}\n{}\nCapturas: {:?}\n",
        report.done,
        report.error,
        report.steps.join("\n"),
        report.screenshots
    );
    std::fs::write(output.join("native-home.txt"), &summary).unwrap();
    println!("{summary}");
    assert!(report.done && report.error.is_none(), "{summary}");
    assert_eq!(report.screenshots.len(), 3);
}
