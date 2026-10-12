//! Imports of PNG and WAV files for agents (asset_import), the assets as oxy_project lists
//! them, and the runtime's log counters that the editor's console feed reads.
use oxy_core::{
    asset_import::{self, ImportKind, ImportRequest, MAX_FILE_BYTES, MAX_INLINE_BYTES},
    command,
    document::*,
    editing, encoding,
    painting::PaintImage,
    query,
    runtime::Runtime,
};
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};

/// A folder of its own under the system's temporary folder, removed by the test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-import-{name}-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png(width: u32, height: u32) -> Vec<u8> {
    PaintImage::new(width, height, [200, 40, 40, 255])
        .unwrap()
        .to_png()
        .unwrap()
}

/// A mono 16-bit PCM WAV of `samples` silent samples.
fn wav(samples: usize, rate: u32) -> Vec<u8> {
    let data = samples * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((data as u32).to_le_bytes());
    bytes.extend(vec![0u8; data]);
    bytes
}

fn request(args: Json) -> Result<ImportRequest, String> {
    ImportRequest::parse(&args)
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

/// A 2D project with a sprite, an interface image and a group, and its folder.
fn project_with_targets(dir: &Path) -> (Project, Id, [Id; 3]) {
    let mut project = editing::blank_project("Importação", SceneKind::TwoD).unwrap();
    let scene = project.start_scene.clone();
    let mut sprite = Entity::new("Placa", Some(Primitive::Sprite));
    sprite.dimensions[..2].copy_from_slice(&[1., 2.]);
    let mut image = Entity::new("Logo do menu", None);
    image.ui = Some(UiElement {
        kind: UiKind::Image,
        ..Default::default()
    });
    let group = Entity::new("Grupo", None);
    let ids = [sprite.id.clone(), image.id.clone(), group.id.clone()];
    project
        .scene_mut(&scene)
        .unwrap()
        .entities
        .extend([sprite, image, group]);
    oxy_core::persistence::save_project(&dir.join("project.oxy.json"), &project).unwrap();
    (project, scene, ids)
}

#[test]
fn a_png_from_a_path_is_copied_into_assets_and_shown_by_the_object_asked() {
    let dir = folder("png");
    let source_dir = folder("png-source");
    let (mut project, scene, [sprite, image, group]) = project_with_targets(&dir);
    let source = source_dir.join("Placa da fase.PNG");
    std::fs::write(&source, png(64, 32)).unwrap();

    let asked = request(json!({"path": path_arg(&source), "apply_to": sprite})).unwrap();
    let read = asked.read().unwrap();
    assert_eq!(read.kind, ImportKind::Texture);
    assert_eq!(read.name, "Placa da fase");
    let imported = asset_import::import(&mut project, &dir, &scene, &asked, &read).unwrap();
    assert_eq!((imported.width, imported.height), (Some(64), Some(32)));
    assert_eq!(imported.path, format!("assets/{}.png", imported.asset));
    assert_eq!(imported.applied_to.as_ref(), Some(&sprite));
    assert_eq!(
        std::fs::read(dir.join(&imported.path)).unwrap(),
        std::fs::read(&source).unwrap(),
        "the file is copied as it is"
    );
    let placed = project.scene(&scene).unwrap().entity(&sprite).unwrap();
    assert_eq!(placed.material.texture.as_ref(), Some(&imported.asset));
    assert_eq!(
        placed.dimensions[..2],
        [4., 2.],
        "the sprite takes the image's proportions"
    );
    let answer = serde_json::to_value(&imported).unwrap();
    assert_eq!(answer["kind"], "texture");
    assert!(answer.get("seconds").is_none());

    // An interface element shows it as its image, and a project scene can be named.
    let ui = request(
        json!({"path": path_arg(&source), "apply_to": image, "scene": scene,
                            "name": "Logo"}),
    )
    .unwrap();
    let second =
        asset_import::import(&mut project, &dir, "outra", &ui, &ui.read().unwrap()).unwrap();
    let element = project.scene(&scene).unwrap().entity(&image).unwrap();
    assert_eq!(
        element.ui.as_ref().unwrap().texture.as_ref(),
        Some(&second.asset)
    );
    assert!(element.material.texture.is_none());
    assert_eq!(second.name, "Logo");

    // A group has nowhere to show it: refused before anything is written.
    let before = project.clone();
    let files = std::fs::read_dir(dir.join("assets")).unwrap().count();
    let refused = request(json!({"path": path_arg(&source), "apply_to": group})).unwrap();
    let error = asset_import::import(
        &mut project,
        &dir,
        &scene,
        &refused,
        &refused.read().unwrap(),
    )
    .unwrap_err();
    assert!(error.contains("Grupo"), "{error}");
    let missing = request(json!({"path": path_arg(&source), "apply_to": "fantasma"})).unwrap();
    let error = asset_import::import(
        &mut project,
        &dir,
        &scene,
        &missing,
        &missing.read().unwrap(),
    )
    .unwrap_err();
    assert!(error.contains("fantasma"), "{error}");
    assert_eq!(project, before);
    assert_eq!(
        std::fs::read_dir(dir.join("assets")).unwrap().count(),
        files
    );
    oxy_core::persistence::save_project(&dir.join("project.oxy.json"), &project).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(source_dir).unwrap();
}

#[test]
fn a_wav_sent_inline_is_found_by_its_header_and_reports_its_length() {
    let dir = folder("wav");
    let (mut project, scene, _) = project_with_targets(&dir);
    let data = encoding::base64(&wav(22_050, 44_100));
    let asked = request(json!({"data_base64": data, "name": "  Pulo\n\tcurto\u{7} "})).unwrap();
    let read = asked.read().unwrap();
    assert_eq!(read.kind, ImportKind::Audio);
    assert_eq!(read.name, "Pulo curto", "one line, no control characters");
    let imported = asset_import::import(&mut project, &dir, &scene, &asked, &read).unwrap();
    assert_eq!(imported.seconds, Some(0.5));
    assert!(imported.path.ends_with(".wav"));
    assert_eq!(
        project.asset(&imported.asset).unwrap().kind,
        AssetKind::Audio
    );
    // A sound plays through action.sound; the node takes the new asset.
    let applied = command::apply(
        &mut project,
        &scene,
        &command::parse(&json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Som"},
            {"op": "add_node", "entity": "$0", "operation": "action.sound",
             "params": {"asset": {"Text": imported.asset}}},
        ]))
        .unwrap(),
    );
    assert!(applied.is_ok(), "{applied:?}");
    let unnamed = request(json!({"data_base64": encoding::base64(&png(2, 2))})).unwrap();
    assert_eq!(unnamed.read().unwrap().name, "Imagem importada");
    let long = request(json!({"data_base64": data, "name": "a".repeat(200)})).unwrap();
    assert_eq!(long.read().unwrap().name.chars().count(), 80);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sources_are_refused_by_place_type_header_and_size_before_anything_is_written() {
    let dir = folder("refused");
    let png_file = dir.join("imagem.png");
    std::fs::write(&png_file, png(4, 4)).unwrap();
    let disguised = dir.join("som.wav");
    std::fs::write(&disguised, png(4, 4)).unwrap();
    let text = dir.join("notas.png");
    std::fs::write(&text, "não é imagem").unwrap();
    let huge = dir.join("enorme.wav");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    for (args, expected) in [
        (json!({}), "Informe path"),
        (
            json!({"path": path_arg(&png_file), "data_base64": "AAAA"}),
            "não os dois",
        ),
        (json!({"path": "imagens/placa.png"}), "caminho completo"),
        (json!({"path": "\\\\servidor\\pasta\\placa.png"}), "rede"),
        (json!({"path": "//servidor/pasta/placa.png"}), "rede"),
        // Windows takes '/' and '\' alike: mixed, they still reach another machine.
        (json!({"path": "\\/servidor/pasta/placa.png"}), "rede"),
        (json!({"path": "/\\servidor\\pasta\\placa.png"}), "rede"),
        (json!({"path": "  \\\\servidor\\pasta\\placa.png"}), "rede"),
        // Device and verbatim paths are not a disk either.
        (json!({"path": "/\\.\\pipe\\placa.png"}), "dispositivo"),
        (json!({"path": "\\\\.\\C:\\placa.png"}), "dispositivo"),
        (json!({"path": "\\\\?\\C:\\placa.png"}), "dispositivo"),
        (
            json!({"path": "\\\\?\\UNC\\servidor\\pasta\\placa.png"}),
            "dispositivo",
        ),
        (
            json!({"path": path_arg(&dir.join("leia.txt"))}),
            ".png e .wav",
        ),
        (
            json!({"path": path_arg(&png_file), "kind": "audio"}),
            "kind pede som WAV",
        ),
        (
            json!({"data_base64": "AAAA", "kind": "audio", "apply_to": "x"}),
            "apply_to só vale para imagens",
        ),
        (json!({"path": path_arg(&png_file), "pasta": "x"}), "pasta"),
        (
            json!({"data_base64": "A".repeat(MAX_INLINE_BYTES * 2 + 1)}),
            "16 MB",
        ),
    ] {
        let error = request(args.clone()).unwrap_err();
        assert!(error.contains(expected), "{args}: {error}");
    }
    for (args, expected) in [
        (
            json!({"path": path_arg(&disguised)}),
            "a extensão pede som WAV",
        ),
        (json!({"path": path_arg(&text)}), "nem um som WAV"),
        (json!({"path": path_arg(&huge)}), "64 MB"),
        (
            json!({"path": path_arg(&dir.join("sumiu.png"))}),
            "sumiu.png",
        ),
        (
            json!({"data_base64": encoding::base64(&wav(10, 8000)), "kind": "texture"}),
            "kind pede imagem PNG",
        ),
        (
            json!({"data_base64": encoding::base64(&wav(10, 8000)), "apply_to": "x"}),
            "apply_to só vale para imagens",
        ),
        (
            json!({"data_base64": "não é base64"}),
            "data_base64 inválido",
        ),
        (
            json!({"data_base64": encoding::base64(b"\x89PNG\r\n\x1a\nquebrado")}),
            "PNG inválido",
        ),
    ] {
        let error = request(args.clone()).unwrap().read().unwrap_err();
        assert!(error.contains(expected), "{args}: {error}");
    }
    // The file read is the path checked: spaces around it do not count.
    let spaced = request(json!({"path": format!("  {}\n", path_arg(&png_file))})).unwrap();
    let read = spaced.read().unwrap();
    assert_eq!(
        (read.kind, read.name.as_str()),
        (ImportKind::Texture, "imagem")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_project_summary_lists_asset_paths_and_who_uses_them() {
    let dir = folder("summary");
    let (mut project, scene, [sprite, image, _]) = project_with_targets(&dir);
    let source = dir.join("placa.png");
    std::fs::write(&source, png(8, 8)).unwrap();
    let asked = request(json!({"path": path_arg(&source), "apply_to": sprite})).unwrap();
    let texture = asset_import::import(&mut project, &dir, &scene, &asked, &asked.read().unwrap())
        .unwrap()
        .asset;
    project
        .scene_mut(&scene)
        .unwrap()
        .entity_mut(&image)
        .unwrap()
        .ui
        .as_mut()
        .unwrap()
        .texture = Some(texture.clone());
    command::apply(
        &mut project,
        &scene,
        &command::parse(&json!([
            {"op": "save_model", "entity": sprite, "name": "Placa pronta"},
        ]))
        .unwrap(),
    )
    .unwrap();
    let summary = query::project_summary(&project);
    let assets = summary["assets"].as_array().unwrap();
    let listed = assets.iter().find(|a| a["id"] == json!(texture)).unwrap();
    assert_eq!(listed["path"], format!("assets/{texture}.png"));
    let roles: Vec<&str> = listed["used_by"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["role"].as_str().unwrap())
        .collect();
    // The sprite, the interface image, and the sprite's copy inside the model.
    assert_eq!(roles, ["material", "ui", "material"]);
    assert_eq!(listed["used_by"][1]["name"], "Logo do menu");
    let model = assets.iter().find(|a| a["kind"] == "Model").unwrap();
    assert!(
        model.get("path").is_none(),
        "a model lives in the project file"
    );
    // The editor's list of references reads as before.
    assert_eq!(
        editing::asset_references(&project, &texture)[..2],
        [
            "Cena 2D → Placa: material".to_owned(),
            "Cena 2D → Logo do menu: imagem da interface".to_owned()
        ]
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_runtime_counts_every_log_and_trace_it_writes() {
    let mut project = editing::blank_project("Contagem", SceneKind::TwoD).unwrap();
    let scene = project.start_scene.clone();
    command::apply(
        &mut project,
        &scene,
        &command::parse(&json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Falante"},
            {"op": "add_node", "entity": "$0", "operation": "event.scene_start"},
            {"op": "add_node", "entity": "$0", "operation": "debug.message",
             "params": {"message": {"Text": "Olá"}}},
            {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
             "to_node": "$2", "to_port": "exec"},
        ]))
        .unwrap(),
    )
    .unwrap();
    let mut runtime = Runtime::new(&project, &scene).unwrap();
    assert_eq!(runtime.logs.len(), 1);
    assert!(runtime.logs[0].ends_with("] Olá"), "{:?}", runtime.logs);
    assert_eq!(runtime.logs_written(), 1);
    assert_eq!(runtime.traces_written(), runtime.traces.len() as u64);
    assert!(runtime.traces_written() >= 2);
    // A host that clears or takes the lines keeps the count.
    runtime.logs.clear();
    runtime.traces.clear();
    assert_eq!(runtime.logs_written(), 1);
    assert!(runtime.traces_written() >= 2);
}
