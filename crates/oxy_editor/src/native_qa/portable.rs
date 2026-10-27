use super::*;

#[test]
#[ignore = "Opens the native home screen; input remains isolated in egui"]
#[cfg(target_os = "windows")]
fn native_portable_home_workflow() {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Release deliberately has no source-directory fallback. Exercise the same
    // adjacent data layout as the portable ZIP; never overwrite current data. The whole
    // example folder goes, entries starting with a dot included, which the editor's copy of
    // the example leaves out (see home_parkour).
    if !cfg!(debug_assertions) {
        let executable = std::env::current_exe().unwrap();
        for (folder, example) in [("data", "validacao"), ("parkour", "parkour")] {
            let source = workspace.join("examples").join(example);
            let data = executable.parent().unwrap().join(folder);
            // What an older checkout left there goes (the parkour of 0.6 kept its sounds in
            // assets/; the one of 0.7 is in folders, with scripts): its files would be files
            // the project does not name, and the home screen would open the old example.
            if data.exists() && files_of(&data).ok() != files_of(&source).ok() {
                std::fs::remove_dir_all(&data).unwrap();
            }
            if !data.exists() {
                copy_directory(&source, &data).unwrap();
            }
        }
    }
    let output = oxy_core::qa::output_dir("v0.2.1/portable");
    std::fs::create_dir_all(&output).unwrap();
    let report = Arc::new(Mutex::new(Report::default()));
    let shared = report.clone();
    let artifacts = output.clone();
    let options = eframe::NativeOptions {
        persist_window: false,
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("OXY Engine — QA portátil")
            .with_icon(oxy_render::branding::oxy_icon())
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
                Action::Click("Jogar"),
                Action::Check("playing"),
                Action::Screenshot("example-game.png"),
                Action::Click("Parar"),
                Action::Check("stop_isolated"),
                Action::Click("Projeto"),
                Action::Click("Tela inicial"),
                Action::Screenshot("recent-project.png"),
                Action::Click("Minha cópia OXY"),
                Action::Check("home_recent"),
                Action::Check("home_bad_open"),
                Action::Click("Projeto"),
                Action::Click("Tela inicial"),
                Action::Click("OXY Parkour"),
                Action::Check("home_parkour"),
                Action::Screenshot("parkour-copy.png"),
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
    assert_eq!(report.screenshots.len(), 4);
}

/// Each file under `folder`, by its path from it, with its bytes.
#[cfg(target_os = "windows")]
fn files_of(folder: &Path) -> std::io::Result<std::collections::BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = std::collections::BTreeMap::new();
    let mut folders = vec![folder.to_owned()];
    while let Some(current) = folders.pop() {
        for entry in std::fs::read_dir(&current)? {
            let path = entry?.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let relative = path.strip_prefix(folder).unwrap_or(&path).to_owned();
                files.insert(relative, std::fs::read(&path)?);
            }
        }
    }
    Ok(files)
}

/// The entries of the project copied to `root` that stay out of a project: any whose name starts
/// with a dot, at any depth, and the backups next to the project file. OXY's own `.oxy` at the
/// root is the project's once the editor holds it, with the editor's lock; a folder in it (types,
/// backups, agents' copies, trash) came from elsewhere.
pub(super) fn outside_project(root: &Path) -> std::io::Result<Vec<String>> {
    let backups = format!("{}.", oxy_core::persistence::PROJECT_FILE);
    let mut outside = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(current) = folders.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let at_root = current == root;
            if at_root && name == ".oxy" && entry.file_type()?.is_dir() {
                for inside in std::fs::read_dir(&path)? {
                    let inside = inside?;
                    if inside.file_type()?.is_dir() {
                        let inside = inside.file_name().to_string_lossy().into_owned();
                        outside.push(format!(".oxy/{inside}"));
                    }
                }
            } else if name.starts_with('.') || (at_root && name.starts_with(&backups)) {
                outside.push(relative);
            } else if entry.file_type()?.is_dir() {
                folders.push(path);
            }
        }
    }
    outside.sort();
    Ok(outside)
}
