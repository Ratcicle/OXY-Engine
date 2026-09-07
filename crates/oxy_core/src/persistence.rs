//! Validated documents and staged, recoverable file replacement.
use crate::{
    document::{Asset, AssetKind, Id, Project, new_id, validate_project},
    painting::PaintImage,
    texture_cache::TextureCache,
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const PROJECT_FILE: &str = "project.oxy.json";
pub fn load_project(path: &Path) -> Result<Project, String> {
    load_project_with(path, false)
}
/// Validates document, relative paths and asset headers without decoding PNG pixels.
/// A damaged pixel stream is diagnosed when the texture is first requested.
pub fn load_project_lazy(path: &Path) -> Result<Project, String> {
    load_project_with(path, true)
}
fn load_project_with(path: &Path, lazy: bool) -> Result<Project, String> {
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let meta = fs::metadata(&path)
        .map_err(|e| format!("Não foi possível abrir {}: {e}", path.display()))?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err("O documento excede o limite de leitura de 64 MB".into());
    }
    let bytes = fs::read(&path).map_err(|e| format!("Falha ao ler projeto: {e}"))?;
    let project: Project =
        serde_json::from_slice(&bytes).map_err(|e| format!("Documento OXY inválido: {e}"))?;
    validate_project(&project)?;
    let root = path.parent().unwrap_or(Path::new("."));
    if lazy {
        for asset in &project.assets {
            if asset.kind != AssetKind::Model {
                validate_asset_header(asset, &resolve_asset_path(root, &asset.path)?)?;
            }
        }
    } else {
        validate_asset_files(&project, root)?;
    }
    Ok(project)
}
pub fn save_project(path: &Path, project: &Project) -> Result<(), String> {
    validate_project(project)?;
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    validate_asset_files(project, root)?;
    safe_write(
        &path,
        &serde_json::to_vec_pretty(project)
            .map_err(|e| format!("Não foi possível serializar: {e}"))?,
    )
}
/// Paint buffers stay in memory until the whole save has been staged successfully.
/// The manifest is replaced last; every previous file has a rollback copy.
pub fn save_bundle(
    path: &Path,
    project: &Project,
    images: &HashMap<Id, PaintImage>,
) -> Result<(), String> {
    validate_project(project)?;
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(root)
        .map_err(|e| format!("Não foi possível criar pasta do projeto: {e}"))?;
    let mut writes = Vec::new();
    for asset in &project.assets {
        if asset.kind == AssetKind::Model {
            continue;
        }
        let target = resolve_asset_path(root, &asset.path)?;
        if let Some(image) = images.get(&asset.id) {
            if asset.kind != AssetKind::Texture {
                return Err(format!(
                    "Pixels associados a asset não-textura: {}",
                    asset.name
                ));
            }
            writes.push((target, image.to_png()?));
        } else {
            validate_asset_file(asset, &target)?;
        }
    }
    let bytes = serde_json::to_vec_pretty(project).map_err(|e| e.to_string())?;
    writes.push((path, bytes));
    transaction_write(writes)
}
/// Writes dirty texture buffers and the manifest in the same recoverable transaction.
/// Clean texture files are checked without decoding or copying their pixel buffers.
/// Dirty cache entries stay pinned if any staging/replacement step fails.
pub fn save_cached_bundle(
    path: &Path,
    project: &Project,
    images: &mut TextureCache,
) -> Result<(), String> {
    validate_project(project)?;
    for (id, _) in images.dirty_images() {
        if !project
            .asset(id)
            .is_some_and(|asset| asset.kind == AssetKind::Texture)
        {
            return Err(format!("Textura alterada sem asset correspondente: {id}"));
        }
    }
    let path = if path.is_dir() {
        path.join(PROJECT_FILE)
    } else {
        path.to_owned()
    };
    let root = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(root)
        .map_err(|e| format!("Não foi possível criar pasta do projeto: {e}"))?;
    let mut writes = Vec::new();
    for asset in &project.assets {
        if asset.kind == AssetKind::Model {
            continue;
        }
        let target = resolve_asset_path(root, &asset.path)?;
        if images.is_dirty(&asset.id) {
            if asset.kind != AssetKind::Texture {
                return Err(format!(
                    "Pixels associados a asset não-textura: {}",
                    asset.name
                ));
            }
            let image = images
                .get(&asset.id)
                .ok_or("Textura alterada não está residente")?;
            writes.push((target, image.to_png()?));
        } else {
            validate_asset_header(asset, &target)?;
        }
    }
    writes.push((
        path.clone(),
        serde_json::to_vec_pretty(project).map_err(|e| e.to_string())?,
    ));
    transaction_write(writes)?;
    images.configure(root, project);
    images.mark_saved();
    Ok(())
}
pub fn load_paint_images(
    project: &Project,
    root: &Path,
) -> Result<HashMap<Id, PaintImage>, String> {
    project
        .assets
        .iter()
        .filter(|a| a.kind == AssetKind::Texture)
        .map(|a| {
            Ok((
                a.id.clone(),
                PaintImage::load(&resolve_asset_path(root, &a.path)?)?,
            ))
        })
        .collect()
}
pub fn import_asset(
    project: &mut Project,
    root: &Path,
    source: &Path,
    kind: AssetKind,
) -> Result<Id, String> {
    if kind == AssetKind::Model {
        return Err("Modelos são salvos pelo Estúdio; importe PNG ou WAV".into());
    }
    let bytes = fs::read(source)
        .map_err(|e| format!("Não foi possível ler origem {}: {e}", source.display()))?;
    match kind {
        AssetKind::Texture => {
            PaintImage::from_png(&bytes)?;
        }
        AssetKind::Audio => validate_wav(&bytes)?,
        AssetKind::Model => {}
    }
    let id = new_id();
    let extension = if kind == AssetKind::Texture {
        "png"
    } else {
        "wav"
    };
    let relative = format!("assets/{id}.{extension}");
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = resolve_asset_path(root, &relative)?;
    safe_write(&target, &bytes)?;
    let name = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Asset importado".into());
    project.assets.push(Asset {
        id: id.clone(),
        name,
        path: relative,
        kind,
        model: None,
    });
    Ok(id)
}
/// Creates an independent texture; neither the original pixels nor its reference changes.
pub fn copy_texture(
    project: &mut Project,
    root: &Path,
    asset_id: &str,
    image: Option<&PaintImage>,
) -> Result<Id, String> {
    let asset = project
        .asset(asset_id)
        .ok_or("Textura não encontrada")?
        .clone();
    if asset.kind != AssetKind::Texture {
        return Err("Asset não é textura".into());
    }
    let bytes = if let Some(image) = image {
        image.to_png()?
    } else {
        fs::read(resolve_asset_path(root, &asset.path)?).map_err(|e| e.to_string())?
    };
    let id = new_id();
    let path = format!("assets/{id}.png");
    safe_write(&resolve_asset_path(root, &path)?, &bytes)?;
    project.assets.push(Asset {
        id: id.clone(),
        name: format!("{} (cópia)", asset.name),
        path,
        kind: AssetKind::Texture,
        model: None,
    });
    Ok(id)
}
pub fn resolve_asset_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || relative.contains(':')
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(format!("Caminho de asset inválido: {relative}"));
    }
    let canonical_root =
        fs::canonicalize(root).map_err(|e| format!("Pasta do projeto indisponível: {e}"))?;
    let result = canonical_root.join(path);
    let mut ancestor = result.as_path();
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or("Caminho sem pasta existente")?;
    }
    let resolved = fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
    if !resolved.starts_with(&canonical_root) {
        return Err(format!(
            "O caminho do asset sai da pasta do projeto: {relative}"
        ));
    }
    Ok(result)
}
pub fn validate_asset_files(project: &Project, root: &Path) -> Result<(), String> {
    for asset in &project.assets {
        if asset.kind != AssetKind::Model {
            validate_asset_file(asset, &resolve_asset_path(root, &asset.path)?)?;
        }
    }
    Ok(())
}
fn validate_asset_file(asset: &Asset, path: &Path) -> Result<(), String> {
    let meta = fs::metadata(path)
        .map_err(|e| format!("Asset ausente ou inacessível '{}': {e}", asset.name))?;
    if !meta.is_file() || meta.len() > 300_000_000 {
        return Err(format!(
            "Arquivo de asset inválido ou maior que 300 MB: {}",
            asset.name
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("Falha ao ler '{}': {e}", asset.name))?;
    match asset.kind {
        AssetKind::Texture => {
            PaintImage::from_png(&bytes).map_err(|e| format!("{}: {e}", asset.name))?;
        }
        AssetKind::Audio => validate_wav(&bytes).map_err(|e| format!("{}: {e}", asset.name))?,
        AssetKind::Model => {}
    }
    Ok(())
}
fn validate_asset_header(asset: &Asset, path: &Path) -> Result<(), String> {
    let meta = fs::metadata(path)
        .map_err(|e| format!("Asset ausente ou inacessível '{}': {e}", asset.name))?;
    if !meta.is_file() || meta.len() > 300_000_000 {
        return Err(format!(
            "Arquivo de asset inválido ou maior que 300 MB: {}",
            asset.name
        ));
    }
    match asset.kind {
        AssetKind::Texture => {
            let reader = image::ImageReader::with_format(
                std::io::BufReader::new(File::open(path).map_err(|e| e.to_string())?),
                image::ImageFormat::Png,
            );
            let (width, height) = reader
                .into_dimensions()
                .map_err(|e| format!("{}: PNG inválido: {e}", asset.name))?;
            if width == 0
                || height == 0
                || width > 16384
                || height > 16384
                || u64::from(width) * u64::from(height) > 67_108_864
            {
                return Err(format!("{}: dimensões de textura inválidas", asset.name));
            }
        }
        AssetKind::Audio => {
            let reader = hound::WavReader::open(path)
                .map_err(|e| format!("{}: WAV inválido: {e}", asset.name))?;
            let spec = reader.spec();
            if spec.channels == 0 || spec.channels > 2 || spec.sample_rate == 0 {
                return Err(format!(
                    "{}: WAV deve ser mono/estéreo com frequência válida",
                    asset.name
                ));
            }
        }
        AssetKind::Model => {}
    }
    Ok(())
}
fn validate_wav(bytes: &[u8]) -> Result<(), String> {
    let reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("WAV inválido: {e}"))?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.channels > 2 || spec.sample_rate == 0 {
        return Err("WAV deve ser mono/estéreo com frequência válida".into());
    }
    Ok(())
}
pub fn safe_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    transaction_write(vec![(path.to_owned(), bytes.to_vec())])
}

struct Staged {
    target: PathBuf,
    temp: PathBuf,
    backup: Option<PathBuf>,
    committed: bool,
}
fn temporary_path(target: &Path, label: &str) -> PathBuf {
    let file = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{file}.{label}.{}", new_id()))
}
fn transaction_write(writes: Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    transaction_write_with(writes, atomic_replace)
}
fn transaction_write_with(
    writes: Vec<(PathBuf, Vec<u8>)>,
    mut replace: impl FnMut(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let mut stages: Vec<Staged> = Vec::new();
    let mut targets = std::collections::HashSet::new();
    let prepared = (|| -> Result<(), String> {
        for (target, bytes) in writes {
            if !targets.insert(target.clone()) {
                return Err(format!(
                    "Dois assets compartilham o caminho {}",
                    target.display()
                ));
            }
            if target.is_dir() {
                return Err(format!("O destino é uma pasta: {}", target.display()));
            }
            let parent = target.parent().unwrap_or(Path::new("."));
            fs::create_dir_all(parent).map_err(|e| format!("Não foi possível criar pasta: {e}"))?;
            let temp = temporary_path(&target, "tmp");
            let backup = if target.exists() {
                Some(temporary_path(&target, "backup"))
            } else {
                None
            };
            stages.push(Staged {
                target: target.clone(),
                temp: temp.clone(),
                backup: backup.clone(),
                committed: false,
            });
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| format!("Falha criando arquivo temporário: {e}"))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("Falha gravando arquivo temporário: {e}"))?;
            drop(file);
            if let Some(backup) = backup {
                fs::copy(&target, &backup)
                    .map_err(|e| format!("Falha preservando arquivo anterior: {e}"))?;
                OpenOptions::new()
                    .write(true)
                    .open(&backup)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| format!("Falha confirmando cópia anterior: {e}"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = prepared {
        cleanup(&stages);
        return Err(error);
    }
    for index in 0..stages.len() {
        if let Err(error) = replace(&stages[index].temp, &stages[index].target) {
            let mut rollback_errors = Vec::new();
            // ReplaceFileW can report an error after removing/renaming the original.
            // Restore the failing stage too; its committed flag cannot prove safety.
            for previous in stages[..=index].iter().rev() {
                if let Err(e) = restore_stage(previous) {
                    rollback_errors.push(format!("{}: {e}", previous.target.display()));
                }
            }
            if rollback_errors.is_empty() {
                cleanup(&stages);
                return Err(format!(
                    "Salvamento não concluído; versão anterior preservada: {error}"
                ));
            }
            return Err(format!(
                "Falha no salvamento: {error}. Cópias de recuperação foram mantidas na pasta. Falha ao restaurar: {}",
                rollback_errors.join("; ")
            ));
        }
        stages[index].committed = true;
    }
    cleanup(&stages);
    Ok(())
}
fn same_file_contents(a: &Path, b: &Path) -> Result<bool, String> {
    let mut a = File::open(a).map_err(|e| e.to_string())?;
    let mut b = File::open(b).map_err(|e| e.to_string())?;
    if a.metadata().map_err(|e| e.to_string())?.len()
        != b.metadata().map_err(|e| e.to_string())?.len()
    {
        return Ok(false);
    }
    let mut first = [0_u8; 65536];
    let mut second = [0_u8; 65536];
    loop {
        let count = a.read(&mut first).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(true);
        }
        b.read_exact(&mut second[..count])
            .map_err(|e| e.to_string())?;
        if first[..count] != second[..count] {
            return Ok(false);
        }
    }
}
fn restore_stage(stage: &Staged) -> Result<(), String> {
    let Some(backup) = &stage.backup else {
        if !stage.target.exists() {
            return Ok(());
        }
        // A failed rename with its temporary file still present did not install
        // our new file. Preserve an unexpected competing destination for review.
        if !stage.committed && stage.temp.exists() {
            return Err("Destino inesperado apareceu durante o salvamento; arquivos preservados para revisão".into());
        }
        return fs::remove_file(&stage.target).map_err(|e| e.to_string());
    };
    if same_file_contents(&stage.target, backup) == Ok(true) {
        return Ok(());
    }
    // Recovery never consumes the sole backup. If recovery itself fails partway,
    // the original bytes remain available and cleanup is deliberately skipped.
    let recovery = temporary_path(&stage.target, "recovery");
    fs::copy(backup, &recovery)
        .map_err(|e| format!("Não foi possível preparar restauração: {e}"))?;
    OpenOptions::new()
        .write(true)
        .open(&recovery)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("Não foi possível confirmar cópia de restauração: {e}"))?;
    let replaced = atomic_replace(&recovery, &stage.target);
    match same_file_contents(&stage.target, backup) {
        Ok(true) => {
            let _ = fs::remove_file(&recovery);
            Ok(())
        }
        verification => Err(format!(
            "Restauração não confirmada; backup mantido em {}. Substituição: {}. Verificação: {:?}",
            backup.display(),
            replaced.err().unwrap_or_else(|| "concluída".into()),
            verification
        )),
    }
}
fn cleanup(stages: &[Staged]) {
    for stage in stages {
        let _ = fs::remove_file(&stage.temp);
        if let Some(backup) = &stage.backup {
            let _ = fs::remove_file(backup);
        }
    }
}
#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    if !target.exists() {
        return fs::rename(source, target).map_err(|e| e.to_string());
    }
    let a: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both paths are owned, terminated UTF-16 buffers alive for this synchronous call.
    let success = unsafe {
        ReplaceFileW(
            a.as_ptr(),
            b.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Entity, Primitive};
    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxy-test-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn cached_bundle_is_lazy_keeps_dirty_on_failure_and_reopens_real_pixels() {
        let dir = temp();
        let path = dir.join(PROJECT_FILE);
        let mut project = Project::new("Cache");
        let mut cache = TextureCache::new(1);
        for name in ["paint", "clean"] {
            let id = new_id();
            project.assets.push(Asset {
                id: id.clone(),
                name: name.into(),
                path: format!("{name}.png"),
                kind: AssetKind::Texture,
                model: None,
            });
            cache.insert(id, PaintImage::new(256, 256, [255; 4]).unwrap());
        }
        save_cached_bundle(&path, &project, &mut cache).unwrap();
        assert!(!cache.has_dirty());
        assert_eq!(cache.len(), 0);
        let reopened = load_project_lazy(&path).unwrap();
        cache.configure(&dir, &reopened);
        assert_eq!(cache.decode_count(), 0);
        let painted = project.assets[0].id.clone();
        let before_manifest = fs::read(&path).unwrap();
        let before_png = fs::read(dir.join("paint.png")).unwrap();
        let clean_png = fs::read(dir.join("clean.png")).unwrap();
        cache.ensure(&painted).unwrap();
        cache
            .get_mut(&painted)
            .unwrap()
            .brush([50., 50.], 4., [255, 0, 0, 255]);
        project.assets.push(Asset {
            id: new_id(),
            name: "Ausente".into(),
            path: "absent.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        assert!(save_cached_bundle(&path, &project, &mut cache).is_err());
        assert!(cache.has_dirty());
        assert_eq!(cache.evict_clean(), 0);
        assert_eq!(fs::read(&path).unwrap(), before_manifest);
        assert_eq!(fs::read(dir.join("paint.png")).unwrap(), before_png);
        project.assets.pop();
        save_cached_bundle(&path, &project, &mut cache).unwrap();
        assert!(!cache.has_dirty());
        assert!(cache.is_empty());
        assert_eq!(fs::read(dir.join("clean.png")).unwrap(), clean_png);
        assert_eq!(load_project(&path).unwrap(), project);
        cache.ensure(&painted).unwrap();
        assert_eq!(cache[&painted].pixel(50, 50), [255, 0, 0, 255]);
        assert_eq!(cache.decode_count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn cached_save_refuses_to_discard_orphan_dirty_pixels() {
        let dir = temp();
        let mut cache = TextureCache::new(1);
        let id = new_id();
        cache.insert(id.clone(), PaintImage::new(16, 16, [255; 4]).unwrap());
        assert!(
            save_cached_bundle(&dir.join(PROJECT_FILE), &Project::new("Orphan"), &mut cache)
                .is_err()
        );
        assert!(cache.is_dirty(&id));
        assert!(cache.contains_key(&id));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn save_load_and_invalid_version_preserves_document() {
        let dir = temp();
        let path = dir.join(PROJECT_FILE);
        let p = Project::new("Teste");
        save_project(&path, &p).unwrap();
        let bytes = fs::read(&path).unwrap();
        let mut bad = p.clone();
        bad.schema_version = 999;
        assert!(save_project(&path, &bad).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(load_project(&path).unwrap(), p);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn imported_png_and_paint_link_survive() {
        let dir = temp();
        let origin = dir.join("source.png");
        let mut image = PaintImage::new(32, 32, [255; 4]).unwrap();
        image.save(&origin).unwrap();
        let original = fs::read(&origin).unwrap();
        let mut p = Project::new("paint");
        let id = import_asset(&mut p, &dir, &origin, AssetKind::Texture).unwrap();
        let mut e = Entity::new("piece", Some(Primitive::Cube));
        e.material.texture = Some(id.clone());
        let entity_id = e.id.clone();
        p.scenes[0].entities.push(e);
        image.brush([8., 8.], 3., [255, 0, 0, 255]);
        save_bundle(
            &dir.join(PROJECT_FILE),
            &p,
            &HashMap::from([(id.clone(), image.clone())]),
        )
        .unwrap();
        let q = load_project(&dir).unwrap();
        assert_eq!(
            q.scenes[0].entity(&entity_id).unwrap().material.texture,
            Some(id.clone())
        );
        assert_eq!(load_paint_images(&q, &dir).unwrap()[&id], image);
        assert_eq!(fs::read(origin).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn staging_failure_does_not_replace_previous_files() {
        let dir = temp();
        let existing = dir.join("data.txt");
        safe_write(&existing, b"original").unwrap();
        let invalid = dir.join("folder");
        fs::create_dir(&invalid).unwrap();
        assert!(
            transaction_write(vec![
                (existing.clone(), b"new".to_vec()),
                (invalid, b"bad".to_vec())
            ])
            .is_err()
        );
        assert_eq!(fs::read(existing).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn replacement_writes_new_content_after_backup_sync() {
        let dir = temp();
        let path = dir.join("replace.txt");
        safe_write(&path, b"old").unwrap();
        safe_write(&path, b"new").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn locked_second_file_rolls_back_first_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp();
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        safe_write(&a, b"first original").unwrap();
        safe_write(&b, b"second original").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&b)
            .unwrap();
        let result = transaction_write(vec![
            (a.clone(), b"first changed".to_vec()),
            (b.clone(), b"second changed".to_vec()),
        ]);
        assert!(result.is_err());
        assert_eq!(fs::read(a).unwrap(), b"first original");
        assert_eq!(fs::read(&b).unwrap(), b"second original");
        drop(lock);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn partially_failed_replace_restores_current_and_previous_files() {
        let dir = temp();
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        safe_write(&first, b"first original").unwrap();
        safe_write(&second, b"second original").unwrap();
        let mut attempt = 0;
        let result = transaction_write_with(
            vec![
                (first.clone(), b"first new".to_vec()),
                (second.clone(), b"second new".to_vec()),
            ],
            |source, target| {
                attempt += 1;
                if attempt == 2 {
                    // Reproduce the documented ReplaceFileW error 1176 contract:
                    // failure is returned after the original pathname is removed.
                    fs::remove_file(target).unwrap();
                    Err("Falha parcial simulada após remover o original (1176)".into())
                } else {
                    atomic_replace(source, target)
                }
            },
        );
        assert!(result.unwrap_err().contains("versão anterior preservada"));
        assert_eq!(fs::read(&first).unwrap(), b"first original");
        assert_eq!(fs::read(&second).unwrap(), b"second original");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn failed_recovery_keeps_the_original_backup() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp();
        let path = dir.join("locked.txt");
        safe_write(&path, b"irreplaceable original").unwrap();
        let mut lock = None;
        let result = transaction_write_with(vec![(path.clone(), b"new".to_vec())], |_, target| {
            fs::write(target, b"partially replaced").unwrap();
            lock = Some(
                OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(target)
                    .unwrap(),
            );
            Err("Falha parcial simulada com bloqueio durante recuperação".into())
        });
        let error = result.unwrap_err();
        assert!(error.contains("Cópias de recuperação foram mantidas"));
        assert!(error.contains("backup mantido"));
        drop(lock);
        let backup = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".backup.")
            })
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"irreplaceable original");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn broken_asset_rejected_without_replacing_caller_state() {
        let dir = temp();
        let p = Project::new("valid");
        save_project(&dir.join(PROJECT_FILE), &p).unwrap();
        let mut q = p.clone();
        q.assets.push(Asset {
            id: new_id(),
            name: "missing".into(),
            path: "assets/missing.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        safe_write(&dir.join(PROJECT_FILE), &serde_json::to_vec(&q).unwrap()).unwrap();
        assert!(load_project(&dir).is_err());
        assert_eq!(p.name, "valid");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn painted_model_animation_and_instanced_graph_survive_bundle() {
        use crate::{
            animation::{AnimationEvent, Clip, Interpolation, Keyframe},
            document::Value,
            graph::Node,
        };
        let dir = temp();
        let mut project = Project::new("Fluxo integrado");
        let scene_id = project.scenes[0].id.clone();
        let texture = new_id();
        project.assets.push(Asset {
            id: texture.clone(),
            name: "Pintura".into(),
            path: format!("assets/{texture}.png"),
            kind: AssetKind::Texture,
            model: None,
        });
        let mut actor = Entity::new("Montagem", None);
        let mut arm = Entity::new("Braço", Some(Primitive::Cube));
        arm.parent = Some(actor.id.clone());
        arm.transform.pivot = [0., 0.5, 0.];
        arm.material.texture = Some(texture.clone());
        let mut clip = Clip::new("Ataque");
        clip.insert_key(
            &arm.id,
            Keyframe {
                time: 0.,
                transform: arm.transform.clone(),
                interpolation: Interpolation::Linear,
            },
        );
        let mut raised = arm.transform.clone();
        raised.rotation[0] = 1.;
        clip.insert_key(
            &arm.id,
            Keyframe {
                time: 0.5,
                transform: raised,
                interpolation: Interpolation::Linear,
            },
        );
        clip.events.push(AnimationEvent {
            time: 0.4,
            name: "Impacto".into(),
        });
        let mut play = Node::new("action.animation", [200., 100.]);
        play.params
            .insert("clip".into(), Value::Text(clip.id.clone()));
        actor.graph.nodes.push(play);
        actor.clips.push(clip);
        let actor_id = actor.id.clone();
        project.scenes[0].entities = vec![actor, arm];
        let model = project
            .save_model(&scene_id, &actor_id, "Peças editáveis")
            .unwrap();
        let instance = project.instantiate_model(&model, &scene_id).unwrap();
        let mut pixels = PaintImage::new(256, 256, [255; 4]).unwrap();
        pixels.stroke([20., 20.], [200., 200.], 12., [35, 170, 95, 255]);
        let images = HashMap::from([(texture.clone(), pixels.clone())]);
        save_bundle(&dir.join(PROJECT_FILE), &project, &images).unwrap();
        let restored = load_project(&dir).unwrap();
        let restored_images = load_paint_images(&restored, &dir).unwrap();
        assert_eq!(restored_images[&texture], pixels);
        let scene = restored.scene(&scene_id).unwrap();
        let root = scene.entity(&instance).unwrap();
        let clip = &root.clips[0];
        assert_eq!(root.graph.nodes[0].text("clip"), clip.id);
        let child = scene.entity(&clip.tracks[0].target).unwrap();
        assert_eq!(child.parent.as_deref(), Some(instance.as_str()));
        assert_eq!(child.material.texture, Some(texture));
        assert_eq!(child.transform.pivot, [0., 0.5, 0.]);
        assert_eq!(clip.events[0].name, "Impacto");
        assert_eq!(
            restored
                .asset(&model)
                .unwrap()
                .model
                .as_ref()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(restored, project);
        fs::remove_dir_all(dir).unwrap();
    }
}
