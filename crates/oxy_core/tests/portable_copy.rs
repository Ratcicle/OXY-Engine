use oxy_core::{document::*, persistence, texture_cache::TextureCache};
use std::path::Path;

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
