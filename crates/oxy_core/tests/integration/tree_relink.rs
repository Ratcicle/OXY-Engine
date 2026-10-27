//! What the editor's Library does to the project's files besides the tree's commands: importing
//! into the folder it shows, and pointing an asset or a script whose file went at the file the
//! person picks in "Localizar arquivo…".
use oxy_core::{
    document::*,
    graph::Node,
    painting::PaintImage,
    persistence::{self, AssetState, KnownFiles},
    script::ScriptRef,
    texture_cache::TextureCache,
};
use std::{fs, path::Path};

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

fn temp(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("oxy-biblioteca-{label}-{}", new_id()))
}

#[test]
fn the_library_imports_into_the_folder_it_shows() {
    let root = temp("importar");
    let origin = root.with_extension("origem").join("Pulo.png");
    fs::create_dir_all(origin.parent().unwrap()).unwrap();
    PaintImage::new(2, 2, [200, 40, 40, 255])
        .unwrap()
        .save(&origin)
        .unwrap();
    fs::create_dir_all(root.join("Pista")).unwrap();
    let mut project = Project::new("Pastas");
    project.layout = Layout::Tree;
    let mut made = Vec::new();
    let mut import = |project: &mut Project| {
        persistence::import_asset_into(
            project,
            &root,
            &origin,
            AssetKind::Texture,
            "Pista",
            &mut |path, _| {
                made.push(path.to_owned());
                Ok(())
            },
        )
        .unwrap()
    };
    let first = import(&mut project);
    let second = import(&mut project);
    let shown = |id: &str| {
        let asset = project.asset(id).unwrap();
        (asset.name.clone(), asset.path.clone())
    };
    assert_eq!(shown(&first), ("Pulo".into(), "Pista/Pulo.png".into()));
    assert_eq!(shown(&second), ("Pulo 2".into(), "Pista/Pulo 2.png".into()));
    assert_eq!(made, ["Pista/Pulo.png", "Pista/Pulo 2.png"]);
    for path in ["Pista/Pulo.png", "Pista/Pulo 2.png"] {
        assert_eq!(
            fs::read(root.join(path)).unwrap(),
            fs::read(&origin).unwrap()
        );
    }
    // The old layout has no folders: the copy goes where it always went.
    let mut old = Project::new("Antigo");
    let id = persistence::import_asset_into(
        &mut old,
        &root,
        &origin,
        AssetKind::Texture,
        "Pista",
        &mut |_, _| Ok(()),
    )
    .unwrap();
    assert_eq!(old.asset(&id).unwrap().path, format!("assets/{id}.png"));
    // A model is never imported.
    assert!(
        persistence::import_asset_into(
            &mut project,
            &root,
            &origin,
            AssetKind::Model,
            "Pista",
            &mut |_, _| Ok(()),
        )
        .is_err()
    );
    fs::remove_dir_all(&root).unwrap();
    fs::remove_dir_all(origin.parent().unwrap()).unwrap();
}

/// A project in folders saved at `root` whose box plays the sound Largada, saved as
/// `Pista/Largada.wav` and moved outside OXY to `Sons/Largada.wav`, with the unused sound Bip
/// and the image Pedra on the box.
fn moved_sound(root: &Path) -> (Project, Id) {
    put(root, "Sons/Largada.wav", &wav(4));
    put(root, "Sons/Bip.wav", &wav(6));
    put(
        root,
        "Imagens/Pedra.png",
        &PaintImage::new(2, 2, [90, 90, 90, 255])
            .unwrap()
            .to_png()
            .unwrap(),
    );
    let mut project = Project::new("Religar");
    project.layout = Layout::Tree;
    let mut add = |name: &str, path: &str, kind: AssetKind| {
        let id = new_id();
        project.assets.push(Asset {
            id: id.clone(),
            name: name.into(),
            path: path.into(),
            kind,
            model: None,
        });
        id
    };
    let largada = add("Largada", "Pista/Largada.wav", AssetKind::Audio);
    add("Bip", "Sons/Bip.wav", AssetKind::Audio);
    let pedra = add("Pedra", "Imagens/Pedra.png", AssetKind::Texture);
    let mut caixa = Entity::new("Caixa", Some(Primitive::Cube));
    caixa.material.texture = Some(pedra);
    let mut sound = Node::new("action.sound", [0.; 2]);
    sound
        .params
        .insert("asset".into(), Value::Text(largada.clone()));
    caixa.graph.nodes.push(sound);
    project.scenes[0].entities.push(caixa);
    persistence::save_project(&root.join(persistence::PROJECT_FILE), &project).unwrap();
    let (project, report) =
        persistence::open_project(&root.join(persistence::PROJECT_FILE)).unwrap();
    assert_eq!(report.assets.get(&largada), Some(&AssetState::Missing));
    (project, largada)
}

#[test]
fn a_missing_sound_takes_the_file_the_person_picks() {
    let root = temp("religar");
    let (mut project, largada) = moved_sound(&root);
    // Opening registered the moved file as a new sound, which nothing uses.
    let registered = project
        .assets
        .iter()
        .find(|a| a.path == "Sons/Largada.wav")
        .unwrap()
        .id
        .clone();
    assert_ne!(registered, largada);
    let count = project.assets.len();
    let mut known = KnownFiles::default();
    // What is no sound of the tree, or the file of a sound in use, is refused.
    let mut refused = |project: &mut Project, path: &str| {
        let before = project.clone();
        let error =
            persistence::relink_asset(project, &root, &largada, path, &mut known).unwrap_err();
        assert_eq!(*project, before);
        error
    };
    assert_eq!(
        refused(&mut project, "Imagens/Pedra.png"),
        "Imagens/Pedra.png não é um som WAV"
    );
    assert_eq!(
        refused(&mut project, "Sons/Sumido.wav"),
        "Sons/Sumido.wav não está na pasta do projeto"
    );
    let mut used = project.clone();
    let bip = used
        .assets
        .iter()
        .find(|a| a.name == "Bip")
        .unwrap()
        .id
        .clone();
    let mut node = Node::new("action.sound", [0.; 2]);
    node.params.insert("asset".into(), Value::Text(bip));
    used.scenes[0].entities[0].graph.nodes.push(node);
    assert_eq!(
        refused(&mut used, "Sons/Bip.wav"),
        "Sons/Bip.wav já é o arquivo de Bip (som), que está em uso"
    );
    // Religar: the sound the box plays keeps its ID, takes the file and its name, and the entry
    // opening registered for the file leaves.
    let told = persistence::relink_asset(
        &mut project,
        &root,
        &largada,
        "sons/largada.wav",
        &mut known,
    )
    .unwrap();
    assert_eq!(
        told,
        "Largada (som): o arquivo agora é Sons/Largada.wav (era Pista/Largada.wav)"
    );
    let relinked = project.asset(&largada).unwrap();
    assert_eq!(
        (relinked.name.as_str(), relinked.path.as_str()),
        ("Largada", "Sons/Largada.wav")
    );
    assert!(project.asset(&registered).is_none());
    assert_eq!(project.assets.len(), count - 1);
    assert!(
        persistence::asset_problem(&project, &root, project.asset(&largada).unwrap()).is_none()
    );
    // A sound has no pixels to paint: nothing is noted of it.
    assert_eq!(known, KnownFiles::default());
    // A model is found by the ID inside its file, never picked.
    let model = Asset {
        id: new_id(),
        name: "Torre".into(),
        path: "Torre.modelo.json".into(),
        kind: AssetKind::Model,
        model: None,
    };
    project.assets.push(model.clone());
    assert!(
        persistence::relink_asset(&mut project, &root, &model.id, "Sons/Bip.wav", &mut known)
            .unwrap_err()
            .contains("um modelo é achado pelo ID")
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_missing_image_takes_the_file_and_the_pixels_painted_on_it_after() {
    let root = temp("religar-imagem");
    let png = |color: [u8; 4]| PaintImage::new(2, 2, color).unwrap();
    put(
        &root,
        "Imagens/Pedra.png",
        &png([90, 90, 90, 255]).to_png().unwrap(),
    );
    let mut project = Project::new("Pintar");
    project.layout = Layout::Tree;
    let pedra = new_id();
    project.assets.push(Asset {
        id: pedra.clone(),
        name: "Pedra".into(),
        path: "Pista/Pedra.png".into(),
        kind: AssetKind::Texture,
        model: None,
    });
    let mut caixa = Entity::new("Caixa", Some(Primitive::Cube));
    caixa.material.texture = Some(pedra.clone());
    project.scenes[0].entities.push(caixa);
    let file = root.join(persistence::PROJECT_FILE);
    persistence::save_project(&file, &project).unwrap();
    let (mut project, mut report) = persistence::open_project(&file).unwrap();
    assert_eq!(report.assets.get(&pedra), Some(&AssetState::Missing));
    persistence::relink_asset(
        &mut project,
        &root,
        &pedra,
        "Imagens/Pedra.png",
        &mut report.known,
    )
    .unwrap();
    assert_eq!(
        report.known.image(&pedra).map(|(path, _)| path),
        Some("Imagens/Pedra.png")
    );
    // Painted after Religar, the save replaces the file the person picked.
    let painted = png([10, 200, 10, 255]);
    let mut cache = TextureCache::new(1);
    cache.configure(&root, &project);
    cache.insert(pedra.clone(), painted.clone());
    persistence::save_cached_bundle_with(&file, &project, &mut cache, &mut report.known).unwrap();
    assert_eq!(
        fs::read(root.join("Imagens/Pedra.png")).unwrap(),
        painted.to_png().unwrap()
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_missing_script_takes_the_file_the_person_picks() {
    let root = temp("religar-script");
    put(&root, "Pista/porta.luau", b"return {}\n");
    put(&root, "Pista/outra.luau", b"return {}\n");
    let mut project = Project::new("Scripts");
    project.layout = Layout::Tree;
    let mut porta = Entity::new("Porta", None);
    porta.scripts = vec![ScriptRef::new("scripts/porta.luau")];
    let mut dupla = Entity::new("Dupla", None);
    dupla.scripts = vec![
        ScriptRef::new("scripts/porta.luau"),
        ScriptRef::new("Pista/porta.luau"),
    ];
    let mut outra = Entity::new("Outra", None);
    outra.scripts = vec![ScriptRef::new("Pista/outra.luau")];
    project.scenes[0].entities.extend([porta, dupla, outra]);
    let before = project.clone();
    assert_eq!(
        persistence::relink_script(
            &mut project,
            &root,
            "scripts/porta.luau",
            "Pista/sumiu.luau"
        )
        .unwrap_err(),
        "Pista/sumiu.luau não está na pasta do projeto"
    );
    assert_eq!(
        persistence::relink_script(&mut project, &root, "scripts/nada.luau", "Pista/porta.luau")
            .unwrap_err(),
        "Nenhum objeto anexa o script scripts/nada.luau"
    );
    assert_eq!(project, before);
    let told = persistence::relink_script(
        &mut project,
        &root,
        "scripts/porta.luau",
        "pista/PORTA.luau",
    )
    .unwrap();
    assert_eq!(
        told,
        "Script scripts/porta.luau: o arquivo agora é Pista/porta.luau (2 objetos)"
    );
    let paths = |name: &str| -> Vec<String> {
        project.scenes[0]
            .entities
            .iter()
            .find(|e| e.name == name)
            .unwrap()
            .scripts
            .iter()
            .map(|s| s.path.clone())
            .collect()
    };
    assert_eq!(paths("Porta"), ["Pista/porta.luau"]);
    // An object that attached both keeps one.
    assert_eq!(paths("Dupla"), ["Pista/porta.luau"]);
    assert_eq!(paths("Outra"), ["Pista/outra.luau"]);
    fs::remove_dir_all(&root).unwrap();
}
