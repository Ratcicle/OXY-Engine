use oxy_core::{
    document::*,
    persistence,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    texture_cache::TextureCache,
};
use std::path::Path;
use std::sync::Arc;

#[test]
fn example_copy_keeps_lazy_png_audio_and_ids_without_writing_source() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/validacao");
    let project = persistence::load_project_lazy(&source.join(persistence::PROJECT_FILE)).unwrap();
    let original = std::fs::read(source.join(persistence::PROJECT_FILE)).unwrap();
    let output = std::env::temp_dir().join(format!("oxy-copy-{}", new_id()));
    let mut images = TextureCache::default();
    images.configure(&source, &project);
    persistence::save_cached_copy(
        &output.join(persistence::PROJECT_FILE),
        &project,
        &mut images,
        &source,
    )
    .unwrap();
    assert_eq!(
        persistence::load_project(&output.join(persistence::PROJECT_FILE)).unwrap(),
        project
    );
    assert_eq!(
        std::fs::read(source.join(persistence::PROJECT_FILE)).unwrap(),
        original
    );
    for asset in project.assets.iter().filter(|a| a.kind != AssetKind::Model) {
        assert_eq!(
            std::fs::read(source.join(&asset.path)).unwrap(),
            std::fs::read(output.join(&asset.path)).unwrap()
        );
    }
    let texture = project
        .assets
        .iter()
        .find(|a| a.kind == AssetKind::Texture)
        .unwrap();
    images.ensure(&texture.id).unwrap();
    images.get_mut(&texture.id).unwrap().pixels[0] ^= 1;
    let painted = images.get(&texture.id).unwrap().clone();
    let painted_root = output.join("painted-copy");
    persistence::save_cached_copy(
        &painted_root.join(persistence::PROJECT_FILE),
        &project,
        &mut images,
        &source,
    )
    .unwrap();
    assert_eq!(
        oxy_core::painting::PaintImage::load(&painted_root.join(&texture.path)).unwrap(),
        painted
    );
    assert_ne!(
        std::fs::read(source.join(&texture.path)).unwrap(),
        std::fs::read(painted_root.join(&texture.path)).unwrap()
    );
    let asset = project
        .assets
        .iter()
        .find(|a| a.kind != AssetKind::Model)
        .unwrap();
    std::fs::write(output.join(&asset.path), b"preserve existing file").unwrap();
    assert!(
        persistence::save_cached_copy(
            &output.join(persistence::PROJECT_FILE),
            &project,
            &mut images,
            &source
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(output.join(&asset.path)).unwrap(),
        b"preserve existing file"
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn shipped_parkour_opens_as_a_copy_with_its_folders_and_scripts_and_plays() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/parkour");
    let file = source.join(persistence::PROJECT_FILE);
    let original = std::fs::read(&file).unwrap();
    let project = persistence::load_project_lazy(&file).unwrap();
    assert_eq!(project.layout, Layout::Tree);
    // It opens as it is, with nothing to tell: no file missing, no file the register lacks.
    let (opened, report) = persistence::open_project(&file).unwrap();
    assert_eq!(opened, project);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
    let scripts = persistence::load_scripts(&source).unwrap();
    assert!(scripts.unreadable.is_empty(), "{:?}", scripts.unreadable);
    // The project's files, in the folders of the course, of the menu and of what both use: the
    // sounds and the models (each in its own file) and the scripts, which the scenes attach.
    let mut tree: Vec<&str> = project.assets.iter().map(|a| a.path.as_str()).collect();
    tree.extend(scripts.files.keys().map(String::as_str));
    for (kind, count) in [(AssetKind::Audio, 9), (AssetKind::Model, 2)] {
        assert_eq!(
            project.assets.iter().filter(|a| a.kind == kind).count(),
            count
        );
    }
    assert!(tree.contains(&"Comum/Música.wav") && tree.contains(&"Pista/jogador.luau"));
    assert!(
        tree.iter().all(|path| ["Pista/", "Menu/", "Comum/"]
            .iter()
            .any(|f| path.starts_with(f))),
        "{tree:?}"
    );
    let output = std::env::temp_dir().join(format!("oxy-parkour-copy-{}", new_id()));
    let mut images = TextureCache::default();
    images.configure(&source, &project);
    persistence::save_cached_copy(
        &output.join(persistence::PROJECT_FILE),
        &project,
        &mut images,
        &source,
    )
    .unwrap();
    for path in &tree {
        assert_eq!(
            std::fs::read(source.join(path)).unwrap(),
            std::fs::read(output.join(path)).unwrap(),
            "{path}"
        );
    }
    // Nothing else goes: neither OXY's own folder nor the copy of an older agent.
    assert!(!output.join(".oxy").exists());
    assert!(!output.join("project.oxy.json.agente.bak").exists());
    assert_eq!(std::fs::read(&file).unwrap(), original);
    let copy = persistence::load_project(&output.join(persistence::PROJECT_FILE)).unwrap();
    assert_eq!(copy, project);
    let copied = Arc::new(persistence::load_scripts(&output).unwrap());
    assert_eq!(*copied, scripts);
    // Each scene of the copy plays some seconds with its scripts, without a line in the log.
    for scene in &copy.scenes {
        let options = RuntimeOptions {
            scripts: copied.clone(),
            ..Default::default()
        };
        let mut runtime = Runtime::with_options(&copy, &scene.id, options).unwrap();
        assert!(runtime.has_script_host(), "{}", scene.name);
        for _ in 0..240 {
            runtime.advance(FIXED_DT, &InputFrame::default());
        }
        assert!(
            runtime.logs.is_empty(),
            "{}: {:?}",
            scene.name,
            runtime.logs
        );
    }
    std::fs::remove_dir_all(output).unwrap();
}
