//! Organizing projects of the old layout in folders: the names and the backup, a crash at each
//! step finished or undone by the next opening, folders with other files left as they are,
//! targets with the same bytes taken, pixels not saved yet and an agent's note.
use super::*;
use crate::{
    document::{Entity, Primitive, SceneKind, new_id},
    editing::blank_project,
    painting::PaintImage,
    script::ScriptRef,
};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind, panic_any},
    sync::Once,
};

/// What a test throws to stop a conversion as a crash would.
struct Crash;

/// Keeps the crashes the tests make out of the output; every other panic is told as usual.
fn quiet_crashes() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let told = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !info.payload().is::<Crash>() {
                told(info);
            }
        }));
    });
}

fn temp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-conversao-{label}-{}", new_id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

/// A mono 16-bit PCM WAV of `samples` silent samples.
fn wav(samples: u32) -> Vec<u8> {
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(8000_u32.to_le_bytes());
    bytes.extend(16000_u32.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(vec![0_u8; data as usize]);
    bytes
}

fn png(color: [u8; 4]) -> Vec<u8> {
    PaintImage::new(2, 2, color).unwrap().to_png().unwrap()
}

/// Every file under `root` but the hidden ones (OXY's own folder, a save's temporary files), by
/// its path with `/`, with its bytes.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![(root.to_owned(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = format!("{prefix}{name}");
            if entry.file_type().unwrap().is_dir() {
                pending.push((entry.path(), format!("{path}/")));
            } else {
                found.insert(path, fs::read(entry.path()).unwrap());
            }
        }
    }
    found
}

/// Copies every file of `from`, hidden ones included, to the new folder `to`.
fn copy_folder(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_folder(&entry.path(), &target);
        } else {
            fs::create_dir_all(to).unwrap();
            fs::write(target, fs::read(entry.path()).unwrap()).unwrap();
        }
    }
}

/// A project of the old layout saved in a folder of its own: the sounds "Música", "música" and
/// "Checkpoint", the image "Clique: menu" on a box, the model "Checkpoint" placed once, a sound
/// the 0.6 left behind in `assets/`, a script, and a note that is no asset.
struct Old {
    root: PathBuf,
    project: Project,
    /// Each asset's ID by its name and kind.
    ids: BTreeMap<String, Id>,
}

impl Old {
    fn new(label: &str) -> Self {
        let root = temp(label);
        let mut project = blank_project("Antigo", SceneKind::ThreeD).unwrap();
        let scene = project.start_scene.clone();
        let mut ids = BTreeMap::new();
        for (key, name, kind, bytes) in [
            ("musica", "Música", AssetKind::Audio, wav(10)),
            ("musica-minuscula", "música", AssetKind::Audio, wav(20)),
            ("checkpoint-som", "Checkpoint", AssetKind::Audio, wav(30)),
            (
                "clique",
                "Clique: menu",
                AssetKind::Texture,
                png([200, 40, 40, 255]),
            ),
        ] {
            let id = new_id();
            let extension = if kind == AssetKind::Texture {
                "png"
            } else {
                "wav"
            };
            let path = format!("assets/{id}.{extension}");
            put(&root, &path, &bytes);
            project.assets.push(Asset {
                id: id.clone(),
                name: name.into(),
                path,
                kind,
                model: None,
            });
            ids.insert(key.to_owned(), id);
        }
        put(&root, &format!("assets/{}.wav", new_id()), &wav(40));
        put(&root, "scripts/porta.luau", b"return {}\n");
        put(&root, "LEIA-ME.txt", b"anotacoes");
        let mut cube = Entity::new("Caixa", Some(Primitive::Cube));
        cube.material.texture = Some(ids["clique"].clone());
        cube.scripts.push(ScriptRef::new("scripts/porta.luau"));
        let group = Entity::new("Checkpoint", None);
        let mut block = Entity::new("Bloco", Some(Primitive::Cube));
        block.parent = Some(group.id.clone());
        let group_id = group.id.clone();
        project.scenes[0].entities.extend([cube, group, block]);
        let model = project.save_model(&scene, &group_id, "Checkpoint").unwrap();
        project.instantiate_model(&model, &scene).unwrap();
        ids.insert("checkpoint-modelo".into(), model);
        persistence::save_project(&root, &project).unwrap();
        Self { root, project, ids }
    }

    /// A copy of this project in a new folder, byte for byte.
    fn copy(&self, label: &str) -> Self {
        let root = temp(label);
        copy_folder(&self.root, &root);
        Self {
            root,
            project: self.project.clone(),
            ids: self.ids.clone(),
        }
    }

    fn convert(&self, who: Who) -> Result<Converted, ConversionError> {
        convert(
            &self.root,
            &self.project,
            None,
            &mut KnownFiles::default(),
            who,
        )
    }

    fn asset<'a>(&self, project: &'a Project, key: &str) -> &'a Asset {
        project.asset(&self.ids[key]).unwrap()
    }
}

impl Drop for Old {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The folder of the copy of the project a conversion of the project at `root` kept.
fn backup_of(root: &Path) -> PathBuf {
    let folder = root.join(".oxy/backup");
    let mut found: Vec<PathBuf> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.pop().unwrap()
}

#[test]
fn converts_in_place_with_a_full_backup_and_the_names_of_the_library() {
    let old = Old::new("lugar");
    // Saved by the 0.6: format 5.
    let file = old.root.join(PROJECT_FILE);
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replacen("\"schema_version\": 6", "\"schema_version\": 5", 1),
    )
    .unwrap();
    let before = files(&old.root);
    let converted = old.convert(Who::Person).unwrap();
    let project = &converted.project;
    assert_eq!(project.layout, Layout::Tree);
    let paths: Vec<(&str, &str)> = project
        .assets
        .iter()
        .map(|asset| (asset.name.as_str(), asset.path.as_str()))
        .collect();
    assert_eq!(
        paths,
        [
            ("Música", "Sons/Música.wav"),
            ("música 2", "Sons/música 2.wav"),
            ("Checkpoint", "Sons/Checkpoint.wav"),
            ("Clique menu", "Imagens/Clique menu.png"),
            ("Checkpoint", "Modelos/Checkpoint.modelo.json"),
        ]
    );
    // No ID changed, and the scenes are as they were.
    let ids: Vec<&Id> = project.assets.iter().map(|asset| &asset.id).collect();
    let original: Vec<&Id> = old.project.assets.iter().map(|asset| &asset.id).collect();
    assert_eq!(ids, original);
    assert_eq!(project.scenes, old.project.scenes);
    let after = files(&old.root);
    for key in ["musica", "musica-minuscula", "checkpoint-som", "clique"] {
        let (was, is) = (old.asset(&old.project, key), old.asset(project, key));
        assert_eq!(after[&is.path], before[&was.path], "{key}");
    }
    assert!(after.keys().all(|path| !path.starts_with("assets/")));
    assert!(!old.root.join("assets").exists(), "the orphan went too");
    assert_eq!(after["scripts/porta.luau"], b"return {}\n");
    assert_eq!(after["LEIA-ME.txt"], b"anotacoes");
    let (id, pieces) = model_file::from_bytes(&after["Modelos/Checkpoint.modelo.json"]).unwrap();
    assert_eq!(id, old.ids["checkpoint-modelo"]);
    assert_eq!(
        Some(&pieces),
        old.asset(&old.project, "checkpoint-modelo").model.as_ref()
    );
    // No backup of the format beside the project file: the copy below keeps it.
    assert!(
        after.keys().all(|path| !path.contains(".schema-")),
        "{:?}",
        after.keys()
    );
    // The strict load reads it whole, and so does opening, without a word.
    let loaded = persistence::load_project(&old.root).unwrap();
    assert_eq!(&loaded, project);
    let (opened, report) = persistence::open_project(&old.root).unwrap();
    assert_eq!(&opened, project);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(!old.root.join(JOURNAL).exists());
    // The copy of the project as it was, which opens as it did.
    let backup = backup_of(&old.root);
    let name = backup.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("antes-das-pastas-2"), "{name}");
    assert_eq!(converted.backup, Some(format!(".oxy/backup/{name}")));
    let kept = files(&backup);
    let expected: BTreeMap<String, Vec<u8>> = before
        .iter()
        .filter(|(path, _)| *path != "LEIA-ME.txt")
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect();
    assert_eq!(kept, expected);
    assert_eq!(persistence::load_project(&backup).unwrap(), old.project);
    // What it tells.
    let summary = converted.summary.as_deref().unwrap();
    assert!(summary.contains("3 sons em Sons/"), "{summary}");
    assert!(summary.contains("1 imagem em Imagens/"), "{summary}");
    assert!(summary.contains("1 modelo em Modelos/"), "{summary}");
    assert!(summary.contains("abre na OXY 0.6"), "{summary}");
    let notes = converted.notes.join("\n");
    assert!(
        notes.contains("\"Clique menu\", um nome que o Windows aceita"),
        "{notes}"
    );
    assert!(
        notes.contains("\"música 2\", porque outro do mesmo tipo"),
        "{notes}"
    );
    assert!(converted.changed_assets);
    let moved = converted.json();
    assert_eq!(moved["moved"].as_array().unwrap().len(), 4);
    assert_eq!(moved["models"], json!(["Modelos/Checkpoint.modelo.json"]));
}

#[test]
fn a_crash_at_any_point_is_finished_or_undone_at_the_next_opening() {
    quiet_crashes();
    let template = Old::new("modelo");
    let start = files(&template.root);
    let mut steps = Vec::new();
    let whole = template.copy("inteira");
    let converted = convert_with(
        &whole.root,
        &whole.project,
        None,
        &mut KnownFiles::default(),
        Who::Person,
        &mut |step| steps.push(step),
    )
    .unwrap();
    let done = files(&whole.root);
    let named: Vec<(String, String)> = converted
        .project
        .assets
        .iter()
        .map(|asset| (asset.name.clone(), asset.path.clone()))
        .collect();
    // Every phase has its steps.
    for phase in [
        |s: &Step| matches!(s, Step::Journal),
        |s: &Step| matches!(s, Step::Backup(_)),
        |s: &Step| matches!(s, Step::Name(_)),
        |s: &Step| matches!(s, Step::Install(_)),
        |s: &Step| matches!(s, Step::Clean(_)),
    ] {
        assert!(steps.iter().any(phase), "{steps:?}");
    }
    for step in steps {
        let old = template.copy("queda");
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            convert_with(
                &old.root,
                &old.project,
                None,
                &mut KnownFiles::default(),
                Who::Person,
                &mut |at| {
                    if at == step {
                        panic_any(Crash);
                    }
                },
            )
        }));
        assert!(crashed.is_err(), "{step:?}");
        assert!(old.root.join(JOURNAL).exists(), "{step:?}");
        let (project, report) = persistence::open_project(&old.root).unwrap();
        assert!(!old.root.join(JOURNAL).exists(), "{step:?}");
        assert!(!old.root.join(project_lock::CONVERSION_LOCK).exists());
        if matches!(step, Step::Clean(_)) {
            // The project file was the new one: the old names go.
            assert_eq!(project.layout, Layout::Tree, "{step:?}");
            assert_eq!(files(&old.root), done, "{step:?}");
            assert!(report.fixes[0].contains("foi concluída"), "{report:?}");
            assert!(backup_of(&old.root).join(PROJECT_FILE).is_file());
        } else {
            // It was still the old one: the project is as it was, and saving organizes it.
            assert_eq!(project.layout, Layout::Legacy, "{step:?}");
            assert_eq!(files(&old.root), start, "{step:?}");
            assert!(report.fixes[0].contains("foi desfeita"), "{report:?}");
            assert!(!old.root.join(".oxy/backup").exists(), "{step:?}");
            let again = old.convert(Who::Person).unwrap();
            let names: Vec<(String, String)> = again
                .project
                .assets
                .iter()
                .map(|asset| (asset.name.clone(), asset.path.clone()))
                .collect();
            assert_eq!(
                names, named,
                "{step:?}: no \"Música 3\" from what the crash left"
            );
            assert!(
                names
                    .iter()
                    .all(|(name, _)| uuid::Uuid::parse_str(name).is_err())
            );
            assert_eq!(files(&old.root), done, "{step:?}");
        }
    }
}

#[cfg(windows)]
#[test]
fn an_undo_that_cannot_finish_now_is_tried_again_at_the_next_opening() {
    use std::os::windows::fs::OpenOptionsExt;
    quiet_crashes();
    let old = Old::new("presa");
    let start = files(&old.root);
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        convert_with(
            &old.root,
            &old.project,
            None,
            &mut KnownFiles::default(),
            Who::Person,
            &mut |at| {
                if at == Step::Install(0) {
                    panic_any(Crash);
                }
            },
        )
    }));
    assert!(crashed.is_err());
    // Another program holds a new name, which keeps it from going.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1)
        .open(old.root.join("Sons/Música.wav"))
        .unwrap();
    let (project, report) = persistence::open_project(&old.root).unwrap();
    assert_eq!(project.layout, Layout::Legacy);
    assert!(
        report.warnings[0].contains("não pôde ser desfeita agora"),
        "{report:?}"
    );
    assert!(old.root.join(JOURNAL).exists());
    // While it stays, saving tries again, and tells why it cannot.
    let error = old.convert(Who::Person).unwrap_err();
    assert!(matches!(error, ConversionError::Kept(_)), "{error}");
    drop(held);
    let (project, report) = persistence::open_project(&old.root).unwrap();
    assert_eq!(project.layout, Layout::Legacy);
    assert!(report.fixes[0].contains("foi desfeita"), "{report:?}");
    assert_eq!(files(&old.root), start);
}

#[test]
fn a_journal_of_a_project_saved_another_way_is_set_aside() {
    quiet_crashes();
    let old = Old::new("outro");
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        convert_with(
            &old.root,
            &old.project,
            None,
            &mut KnownFiles::default(),
            Who::Person,
            &mut |at| {
                if at == Step::Name(1) {
                    panic_any(Crash);
                }
            },
        )
    }));
    assert!(crashed.is_err());
    // Someone saved the project file meanwhile.
    let mut renamed = old.project.clone();
    renamed.name = "Renomeado".into();
    let file = old.root.join(PROJECT_FILE);
    fs::write(&file, serde_json::to_vec_pretty(&renamed).unwrap()).unwrap();
    let before = files(&old.root);
    let (project, report) = persistence::open_project(&old.root).unwrap();
    assert_eq!(project.name, "Renomeado");
    assert!(
        report.warnings[0].contains("gravado de outro jeito"),
        "{report:?}"
    );
    assert_eq!(files(&old.root), before, "no file was touched");
    assert!(!old.root.join(JOURNAL).exists());
    let aside: Vec<String> = fs::read_dir(old.root.join(".oxy"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("conversao.") && name.ends_with(".json"))
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
}

#[test]
fn opening_leaves_a_conversion_at_work_alone() {
    let old = Old::new("ocupada");
    let lock = ConversionLock::hold(&old.root).unwrap();
    put(&old.root, JOURNAL, b"{}");
    let resumed = resume(&old.root);
    assert!(resumed.warnings[0].contains("em andamento"), "{resumed:?}");
    assert!(old.root.join(JOURNAL).exists());
    // Saving refuses in either layout: the other conversion may have the new project file in
    // place already.
    let error = old.convert(Who::Person).unwrap_err();
    assert!(
        matches!(&error, ConversionError::Busy(why) if why.contains("salve de novo")),
        "{error}"
    );
    drop(lock);
    // A journal that says nothing is set aside.
    let resumed = resume(&old.root);
    assert!(
        resumed.warnings[0].contains("diário dela ficou"),
        "{resumed:?}"
    );
    assert!(!old.root.join(JOURNAL).exists());
}

#[test]
fn a_folder_with_other_files_stays_in_the_old_layout() {
    let old = Old::new("misturada");
    put(&old.root, "Fotos/praia.png", &png([1, 2, 3, 255]));
    put(&old.root, "assets/fundo.png", &png([4, 5, 6, 255]));
    let before = files(&old.root);
    let error = old.convert(Who::Person).unwrap_err();
    let ConversionError::Refused(why) = error else {
        panic!("{error}");
    };
    assert!(why.contains("2 imagens, sons ou modelos"), "{why}");
    assert!(why.contains("Fotos/praia.png") && why.contains("assets/fundo.png"));
    assert!(why.contains("pasta nova"), "{why}");
    assert_eq!(files(&old.root), before);
    assert!(!old.root.join(".oxy").exists());
    let told = preview(&old.root, &old.project);
    assert!(told.contains("continua nele ao salvar"), "{told}");
    // The note and the scripts are no reason: without the photos, it says what saving does.
    fs::remove_dir_all(old.root.join("Fotos")).unwrap();
    fs::remove_file(old.root.join("assets/fundo.png")).unwrap();
    let told = preview(&old.root, &old.project);
    assert!(told.contains("Ao salvar, a OXY o organiza em pastas (1 imagem em Imagens/, 3 sons em Sons/, 1 modelo em Modelos/, os scripts ficam onde estão)"), "{told}");
}

#[test]
fn a_new_name_holding_the_same_bytes_is_taken_as_it_is() {
    let old = Old::new("igual");
    let music = old.asset(&old.project, "musica").path.clone();
    let bytes = fs::read(old.root.join(&music)).unwrap();
    put(&old.root, "Sons/Música.wav", &bytes);
    let pieces = old.asset(&old.project, "checkpoint-modelo").model.clone();
    let model = model_file::to_bytes(&old.ids["checkpoint-modelo"], &pieces.unwrap()).unwrap();
    put(&old.root, "Modelos/Checkpoint.modelo.json", &model);
    let converted = old.convert(Who::Person).unwrap();
    assert_eq!(
        old.asset(&converted.project, "musica").path,
        "Sons/Música.wav"
    );
    assert_eq!(
        old.asset(&converted.project, "checkpoint-modelo").path,
        "Modelos/Checkpoint.modelo.json"
    );
    assert_eq!(fs::read(old.root.join("Sons/Música.wav")).unwrap(), bytes);
    assert!(!old.root.join(&music).exists());
    assert!(
        converted
            .notes
            .iter()
            .all(|note| !note.contains("Música 2"))
    );
}

#[test]
fn pixels_not_saved_yet_go_to_the_new_name() {
    let old = Old::new("pintura");
    let image = old.ids["clique"].clone();
    let was = old.asset(&old.project, "clique").path.clone();
    let before = fs::read(old.root.join(&was)).unwrap();
    // A new image painted since the last save, with no file yet.
    let mut project = old.project.clone();
    let fresh = new_id();
    project.assets.push(Asset {
        id: fresh.clone(),
        name: "Pintura 64 · Caixa".into(),
        path: format!("assets/{fresh}.png"),
        kind: AssetKind::Texture,
        model: None,
    });
    let mut images = TextureCache::default();
    images.configure(&old.root, &project);
    images.ensure(&image).unwrap();
    let painted = PaintImage::new(2, 2, [9, 9, 200, 255]).unwrap();
    *images.get_mut(&image).unwrap() = painted.clone();
    images.insert(
        fresh.clone(),
        PaintImage::new(4, 4, [1, 1, 1, 255]).unwrap(),
    );
    assert!(images.is_dirty(&image) && images.is_dirty(&fresh));
    let converted = convert(
        &old.root,
        &project,
        Some(&mut images),
        &mut KnownFiles::default(),
        Who::Person,
    )
    .unwrap();
    assert!(!images.has_dirty());
    assert_eq!(
        fs::read(old.root.join("Imagens/Clique menu.png")).unwrap(),
        painted.to_png().unwrap()
    );
    assert_eq!(
        fs::read(backup_of(&old.root).join(&was)).unwrap(),
        before,
        "the copy keeps the image as it was saved"
    );
    let new = converted.project.asset(&fresh).unwrap();
    assert_eq!(new.path, "Imagens/Pintura 64 · Caixa.png");
    assert!(old.root.join(&new.path).is_file());
    assert!(
        converted
            .notes
            .iter()
            .all(|note| !note.contains("não estava")),
        "{:?}",
        converted.notes
    );
    persistence::load_project(&old.root).unwrap();
}

#[test]
fn an_agent_conversion_leaves_a_note_for_the_editor() {
    let old = Old::new("agente");
    let converted = old.convert(Who::Agent).unwrap();
    let summary = converted.summary.clone().unwrap();
    assert!(
        summary.starts_with("O agente organizou o projeto em pastas"),
        "{summary}"
    );
    let note = agent_note(&old.root);
    assert_eq!(note[0], summary);
    assert!(note.iter().any(|line| line.contains("Clique menu")));
    forget_agent_note(&old.root);
    assert!(agent_note(&old.root).is_empty());
    assert!(!old.root.join(AGENT_NOTE).exists());
    // A project already in folders is not converted again.
    let error = convert(
        &old.root,
        &converted.project,
        None,
        &mut KnownFiles::default(),
        Who::Agent,
    )
    .unwrap_err();
    assert!(error.to_string().contains("já está organizado"), "{error}");
}

#[test]
fn a_document_of_the_old_layout_never_goes_over_a_project_organized_meanwhile() {
    let old = Old::new("depois");
    old.convert(Who::Agent).unwrap();
    let saved = fs::read(old.root.join(PROJECT_FILE)).unwrap();
    // The editor still has the document it opened before the agent's write.
    let error = old.convert(Who::Person).unwrap_err();
    assert!(
        matches!(&error, ConversionError::Kept(why) if why.contains("reabra")),
        "{error}"
    );
    assert_eq!(fs::read(old.root.join(PROJECT_FILE)).unwrap(), saved);
}

/// Converts `old` in place, stopping at `step` as a crash would.
fn crash_at(old: &Old, step: Step) {
    quiet_crashes();
    let crashed = catch_unwind(AssertUnwindSafe(|| {
        convert_with(
            &old.root,
            &old.project,
            None,
            &mut KnownFiles::default(),
            Who::Person,
            &mut |at| {
                if at == step {
                    panic_any(Crash);
                }
            },
        )
    }));
    assert!(crashed.is_err(), "{step:?}");
    assert!(old.root.join(JOURNAL).exists(), "{step:?}");
}

#[test]
fn saving_over_a_conversion_that_stopped_tells_what_became_of_it() {
    let old = Old::new("retomada");
    crash_at(&old, Step::Name(1));
    // A refusal says it too.
    put(&old.root, "Fotos/praia.png", &png([1, 2, 3, 255]));
    let error = old.convert(Who::Person).unwrap_err();
    assert!(
        matches!(&error, ConversionError::Refused(why)
            if why.starts_with("Uma organização do projeto em pastas foi interrompida")
                && why.contains("foi desfeita")
                && why.contains("Fotos/praia.png")),
        "{error}"
    );
    assert!(!old.root.join(JOURNAL).exists());
    fs::remove_dir_all(old.root.join("Fotos")).unwrap();
    crash_at(&old, Step::Install(0));
    let converted = old.convert(Who::Person).unwrap();
    assert!(
        converted.warnings[0].contains("foi desfeita"),
        "{:?}",
        converted.warnings
    );
    assert_eq!(converted.project.layout, Layout::Tree);
    assert!(!old.root.join(JOURNAL).exists());
    backup_of(&old.root);
    assert_eq!(
        persistence::load_project(&old.root).unwrap(),
        converted.project
    );
}

#[cfg(windows)]
#[test]
fn a_file_another_program_holds_keeps_the_journal_for_the_next_opening() {
    use std::os::windows::fs::OpenOptionsExt;
    let old = Old::new("diario-preso");
    let start = files(&old.root);
    crash_at(&old, Step::Name(1));
    for held in [JOURNAL, PROJECT_FILE] {
        let hold = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(old.root.join(held))
            .unwrap();
        let resumed = resume(&old.root);
        assert!(
            resumed.warnings[0].contains("não pôde ser concluída agora")
                && resumed.warnings[0].contains(held),
            "{resumed:?}"
        );
        assert!(old.root.join(JOURNAL).exists(), "{held}: not set aside");
        drop(hold);
    }
    let resumed = resume(&old.root);
    assert!(resumed.fixes[0].contains("foi desfeita"), "{resumed:?}");
    assert_eq!(files(&old.root), start);
}

#[test]
fn a_project_file_gone_outside_oxy_is_saved_as_it_was_first() {
    let old = Old::new("sem-arquivo");
    fs::remove_file(old.root.join(PROJECT_FILE)).unwrap();
    let before = files(&old.root);
    let error = old.convert(Who::Person).unwrap_err();
    let ConversionError::Refused(why) = error else {
        panic!("{error}");
    };
    assert!(why.contains("não estava mais na pasta"), "{why}");
    assert_eq!(files(&old.root), before);
    assert!(!old.root.join(".oxy").exists());
    // Saved in the old layout, as the editor does then, it is whole, and the next save
    // organizes it.
    persistence::save_project(&old.root, &old.project).unwrap();
    assert_eq!(persistence::load_project(&old.root).unwrap(), old.project);
    let converted = old.convert(Who::Person).unwrap();
    assert!(converted.backup.is_some());
    let loaded = persistence::load_project(&old.root).unwrap();
    assert_eq!(loaded.layout, Layout::Tree);
    persistence::validate_asset_files(&loaded, &old.root).unwrap();
}

#[test]
fn only_a_project_file_named_as_oxy_names_it_is_organized() {
    let old = Old::new("outro-nome");
    let (file, other) = (old.root.join(PROJECT_FILE), old.root.join("meu.json"));
    fs::write(&other, fs::read(&file).unwrap()).unwrap();
    fs::remove_file(&file).unwrap();
    let before = files(&old.root);
    let error = convert(
        &other,
        &old.project,
        None,
        &mut KnownFiles::default(),
        Who::Person,
    )
    .unwrap_err();
    let ConversionError::Refused(why) = error else {
        panic!("{error}");
    };
    assert!(why.contains("este se chama meu.json"), "{why}");
    assert_eq!(files(&old.root), before);
    assert!(!old.root.join(".oxy").exists());
    let told = preview(&other, &old.project);
    assert!(told.contains("continua nele ao salvar"), "{told}");
}

#[test]
fn a_file_already_at_its_new_path_goes_to_the_copy_too() {
    let mut old = Old::new("ja-no-lugar");
    let id = old.ids["musica"].clone();
    let was = old.asset(&old.project, "musica").path.clone();
    let bytes = fs::read(old.root.join(&was)).unwrap();
    put(&old.root, "Sons/Música.wav", &bytes);
    fs::remove_file(old.root.join(&was)).unwrap();
    let asset = old.project.assets.iter_mut().find(|a| a.id == id).unwrap();
    asset.path = "Sons/Música.wav".into();
    persistence::save_project(&old.root, &old.project).unwrap();
    let converted = old.convert(Who::Person).unwrap();
    assert_eq!(
        converted.project.asset(&id).unwrap().path,
        "Sons/Música.wav"
    );
    assert!(
        converted
            .moved
            .iter()
            .all(|moved| moved.to != "Sons/Música.wav"),
        "it stays where it is"
    );
    assert_eq!(fs::read(old.root.join("Sons/Música.wav")).unwrap(), bytes);
    // The copy of the project holds every file its project file names, and opens whole.
    let backup = backup_of(&old.root);
    assert_eq!(fs::read(backup.join("Sons/Música.wav")).unwrap(), bytes);
    assert_eq!(persistence::load_project(&backup).unwrap(), old.project);
}

#[test]
fn a_project_never_saved_is_saved_in_folders_at_once() {
    let root = temp("nova");
    let mut project = blank_project("Novo", SceneKind::ThreeD).unwrap();
    let scene = project.start_scene.clone();
    let id = new_id();
    project.assets.push(Asset {
        id: id.clone(),
        name: "Pintura".into(),
        path: format!("assets/{id}.png"),
        kind: AssetKind::Texture,
        model: None,
    });
    let mut cube = Entity::new("Torre", Some(Primitive::Cube));
    cube.material.texture = Some(id.clone());
    let tower = cube.id.clone();
    project.scenes[0].entities.push(cube);
    project.save_model(&scene, &tower, "Torre").unwrap();
    let mut images = TextureCache::default();
    images.insert(id.clone(), PaintImage::new(2, 2, [5, 5, 5, 255]).unwrap());
    let folder = root.join("Jogo");
    let converted = convert(
        &folder.join(PROJECT_FILE),
        &project,
        Some(&mut images),
        &mut KnownFiles::default(),
        Who::Person,
    )
    .unwrap();
    assert!(converted.summary.is_none() && converted.backup.is_none());
    assert!(converted.notes.is_empty(), "{:?}", converted.notes);
    let saved = persistence::load_project(&folder).unwrap();
    assert_eq!(saved.layout, Layout::Tree);
    assert_eq!(saved.asset(&id).unwrap().path, "Imagens/Pintura.png");
    assert!(folder.join("Modelos/Torre.modelo.json").is_file());
    assert!(!folder.join(".oxy").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_copy_of_a_project_of_the_old_layout_is_organized_in_folders() {
    let old = Old::new("origem");
    let before = files(&old.root);
    let target = temp("copia");
    let mut images = TextureCache::default();
    images.configure(&old.root, &old.project);
    let copied = save_copy(
        &target.join("Cópia").join(PROJECT_FILE),
        &old.project,
        &mut images,
        &old.root,
        &mut KnownFiles::default(),
    )
    .unwrap();
    assert_eq!(files(&old.root), before, "the source is only read");
    let copy = target.join("Cópia");
    let loaded = persistence::load_project(&copy).unwrap();
    assert_eq!(loaded, copied.project);
    assert_eq!(loaded.layout, Layout::Tree);
    let kept = files(&copy);
    assert_eq!(
        kept.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "Imagens/Clique menu.png",
            "Modelos/Checkpoint.modelo.json",
            "Sons/Checkpoint.wav",
            "Sons/Música.wav",
            "Sons/música 2.wav",
            PROJECT_FILE,
            "scripts/porta.luau",
        ]
    );
    assert_eq!(
        kept["Sons/Música.wav"],
        before[&old.asset(&old.project, "musica").path]
    );
    assert!(copied.warnings.iter().any(|line| line.contains("música 2")));
    assert!(!copy.join(".oxy").exists());
    fs::remove_dir_all(target).unwrap();
}

#[test]
fn only_the_files_the_old_layout_named_after_ids_are_old_assets() {
    let id = new_id();
    assert!(old_asset_file(&format!("assets/{id}.wav")));
    assert!(old_asset_file(&format!("Assets/{id}.PNG")));
    assert!(!old_asset_file(&format!("assets/{id}.luau")));
    assert!(!old_asset_file(&format!("Sons/{id}.wav")));
    assert!(!old_asset_file(&format!("assets/sub/{id}.wav")));
    assert!(!old_asset_file("assets/pintura_local.png"));
    assert!(!old_asset_file(&format!("{id}.wav")));
}

#[test]
fn the_backup_is_named_by_its_date_and_numbered_when_taken() {
    let root = temp("data");
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_792_713_600 + 3723);
    let first = free_backup(&root, at);
    assert_eq!(first, ".oxy/backup/antes-das-pastas-20261023-010203");
    fs::create_dir_all(root.join(&first)).unwrap();
    assert_eq!(free_backup(&root, at), format!("{first}-2"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn names_are_numbered_within_the_limits() {
    let long: String = "á".repeat(90);
    let fitted = names::fitted_name(&long, "Som");
    let second = nth_name(&fitted, 2, 1000);
    assert_eq!(second.chars().count(), names::MAX_NAME_CHARS);
    assert!(second.ends_with(" 2"));
    assert!(nth_name("Pulo.", 1, 100) == "Pulo");
    assert!(nth_name(&fitted, 12, 30).len() <= 30);
}
