//! Projects of the old layout organized in folders: the parkour as 0.6 shipped it (the fixture
//! tests/fixtures/parkour-0.6) with every file's bytes, a strict load that decodes them and the
//! same finish; the validation demos with names Windows keeps as they are; the development
//! format 6 with models inside and scripts; and a copy to a new folder.
use crate::parkour_example::assert_reaches_the_finish;
use oxy_core::{
    conversion::{self, Who},
    document::*,
    editing,
    persistence::{self, KnownFiles, PROJECT_FILE},
    script::ScriptRef,
    texture_cache::TextureCache,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// A folder of its own under the system's temporary folder, removed by the test.
fn folder(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-conversion-{label}-{}", new_id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every file under `root` but the hidden ones (OXY's own folder among them), by its path with
/// `/`, with its bytes.
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

/// A copy of the folder `from` in a new folder.
fn copy_of(from: &Path, label: &str) -> PathBuf {
    let root = folder(label);
    for (path, bytes) in files(from) {
        let file = root.join(&path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }
    root
}

fn repository(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// The one copy of the project as it was a conversion of the project at `root` kept.
fn backup_of(root: &Path) -> PathBuf {
    let mut found: Vec<PathBuf> = fs::read_dir(root.join(".oxy/backup"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.pop().unwrap()
}

/// Opens the project at `root` as the editor does and converts it as its first save does.
fn convert(root: &Path) -> conversion::Converted {
    let (project, report) = persistence::open_project(root).unwrap();
    assert_eq!(report.layout, Layout::Legacy);
    conversion::convert(
        &root.join(PROJECT_FILE),
        &project,
        None,
        &mut KnownFiles::default(),
        Who::Person,
    )
    .unwrap()
}

#[test]
fn the_parkour_of_0_6_converts_with_every_file_identical_and_the_same_finish() {
    let fixture = repository("tests/fixtures/parkour-0.6");
    let original = persistence::load_project(&fixture).unwrap();
    let before = files(&fixture);
    let root = copy_of(&fixture, "parkour");
    let converted = convert(&root);
    let project = &converted.project;

    // Each sound in Sons/ and each model in Modelos/, named as the Library shows it, no ID
    // changed and the scenes untouched.
    let named: Vec<(&str, &str)> = project
        .assets
        .iter()
        .map(|asset| (asset.name.as_str(), asset.path.as_str()))
        .collect();
    assert_eq!(
        named,
        [
            ("Música", "Sons/Música.wav"),
            ("Clique", "Sons/Clique.wav"),
            ("Pulo", "Sons/Pulo.wav"),
            ("Pouso", "Sons/Pouso.wav"),
            ("Checkpoint", "Sons/Checkpoint.wav"),
            ("Queda", "Sons/Queda.wav"),
            ("Chegada", "Sons/Chegada.wav"),
            ("Bipe", "Sons/Bipe.wav"),
            ("Largada", "Sons/Largada.wav"),
            ("Ladrilho que cai", "Modelos/Ladrilho que cai.modelo.json"),
            ("Checkpoint", "Modelos/Checkpoint.modelo.json"),
        ]
    );
    assert_eq!(project.layout, Layout::Tree);
    assert_eq!(project.scenes, original.scenes);
    for (was, is) in original.assets.iter().zip(&project.assets) {
        assert_eq!(
            (&was.id, was.kind, &was.model),
            (&is.id, is.kind, &is.model)
        );
    }
    assert!(converted.notes.is_empty(), "{:?}", converted.notes);

    // Every sound's bytes are the 0.6 file's, and nothing else of it is left.
    let after = files(&root);
    for (was, is) in original.assets.iter().zip(&project.assets) {
        if was.kind == AssetKind::Audio {
            assert_eq!(after[&is.path], before[&was.path], "{}", was.name);
        }
    }
    // Nor a backup of the format beside the project file: the copy below keeps it.
    assert!(!root.join("assets").exists());
    let kept: Vec<&str> = after
        .keys()
        .map(String::as_str)
        .filter(|path| !path.starts_with("Sons/") && !path.starts_with("Modelos/"))
        .collect();
    assert_eq!(kept, [PROJECT_FILE]);
    let manifest: serde_json::Value = serde_json::from_slice(&after[PROJECT_FILE]).unwrap();
    assert_eq!(manifest["layout"], "Tree");
    assert!(
        manifest["assets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|asset| asset.get("model").is_none())
    );

    // The strict load decodes every file and reads the models' files; opening tells nothing.
    persistence::validate_asset_files(project, &root).unwrap();
    let loaded = persistence::load_project(&root).unwrap();
    assert_eq!(&loaded, project);
    let (opened, report) = persistence::open_project(&root).unwrap();
    assert_eq!(&opened, project);
    assert!(report.lines().next().is_none(), "{report:?}");

    // The copy of the project as it was is the fixture, byte for byte, and opens as it did.
    let backup = backup_of(&root);
    assert_eq!(files(&backup), before);
    assert_eq!(persistence::load_project(&backup).unwrap(), original);
    let summary = converted.summary.as_deref().unwrap();
    assert!(
        summary.starts_with(
            "Projeto organizado em pastas ao salvar: 9 sons em Sons/, 2 modelos em Modelos/."
        ),
        "{summary}"
    );
    assert!(summary.contains("abre na OXY 0.6"), "{summary}");

    // The parkour plays to the finish as 0.6 did.
    assert_reaches_the_finish(&loaded);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_validation_demos_convert_with_their_names_as_they_are() {
    let root = copy_of(&repository("../../examples/validacao"), "validacao");
    let before = files(&root);
    let converted = convert(&root);
    let paths: Vec<&str> = converted
        .project
        .assets
        .iter()
        .map(|asset| asset.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "Imagens/Pintura local · pixels editáveis.png",
            "Imagens/Sprite placeholder local.png",
            "Sons/Impacto sintetizado local.wav",
            "Modelos/Boneco editável por peças.modelo.json",
        ]
    );
    assert!(converted.notes.is_empty(), "{:?}", converted.notes);
    let after = files(&root);
    assert_eq!(
        after["Imagens/Pintura local · pixels editáveis.png"],
        before["assets/pintura_local.png"]
    );
    assert_eq!(
        after["Sons/Impacto sintetizado local.wav"],
        before["assets/impacto_local.wav"]
    );
    assert_eq!(persistence::load_project(&root).unwrap(), converted.project);
    assert_eq!(files(&backup_of(&root)), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_development_format_6_converts_with_its_models_and_scripts_in_the_backup() {
    let root = folder("formato-6");
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("scripts/porta.luau"), "return {}\n").unwrap();
    fs::create_dir_all(root.join(".solucoes")).unwrap();
    fs::write(root.join(".solucoes/porta.luau"), "-- fora do projeto\n").unwrap();
    let mut project = editing::blank_project("Teste", SceneKind::ThreeD).unwrap();
    let scene = project.start_scene.clone();
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    door.scripts.push(ScriptRef::new("scripts/porta.luau"));
    let door_id = door.id.clone();
    project.scenes[0].entities.push(door);
    let model = project.save_model(&scene, &door_id, "Porta").unwrap();
    persistence::save_project(&root, &project).unwrap();
    let text = fs::read_to_string(root.join(PROJECT_FILE)).unwrap();
    assert!(text.contains("\"schema_version\": 6") && !text.contains("\"layout\""));
    let before = files(&root);

    let converted = convert(&root);
    let after = files(&root);
    assert_eq!(
        converted.project.asset(&model).unwrap().path,
        "Modelos/Porta.modelo.json"
    );
    assert_eq!(after["scripts/porta.luau"], before["scripts/porta.luau"]);
    assert!(
        after.keys().all(|path| !path.contains(".schema-")),
        "a format 6 file has no format backup: the full copy is it"
    );
    let backup = backup_of(&root);
    assert_eq!(files(&backup), before);
    assert!(!backup.join(".solucoes").exists());
    let summary = converted.summary.as_deref().unwrap();
    assert!(summary.contains("os scripts ficam onde estão"), "{summary}");
    assert!(!summary.contains("0.6"), "{summary}");
    let loaded = persistence::load_project(&root).unwrap();
    assert_eq!(
        loaded.asset(&model).unwrap().model,
        project.asset(&model).unwrap().model
    );
    assert_eq!(loaded.scenes, project.scenes);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_copy_of_the_parkour_of_0_6_goes_to_a_new_folder_in_folders() {
    let fixture = repository("tests/fixtures/parkour-0.6");
    let source = copy_of(&fixture, "origem");
    let (project, _) = persistence::open_project(&source).unwrap();
    let before = files(&source);
    let target = folder("destino");
    let mut images = TextureCache::default();
    images.configure(&source, &project);
    let copied = conversion::save_copy(
        &target.join(PROJECT_FILE),
        &project,
        &mut images,
        &source,
        &mut KnownFiles::default(),
    )
    .unwrap();
    assert_eq!(files(&source), before);
    assert!(!source.join(".oxy").exists() && !target.join(".oxy").exists());
    let loaded = persistence::load_project(&target).unwrap();
    assert_eq!(loaded, copied.project);
    assert_eq!(loaded.layout, Layout::Tree);
    let copy = files(&target);
    assert_eq!(
        copy["Sons/Música.wav"],
        before["assets/29cc0804-3ee0-473f-a533-999b0476a1df.wav"]
    );
    assert!(copy.contains_key("Modelos/Checkpoint.modelo.json"));
    assert!(copy.keys().all(|path| !path.starts_with("assets/")));
    fs::remove_dir_all(source).unwrap();
    fs::remove_dir_all(target).unwrap();
}
