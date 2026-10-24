//! The project's files in format 6: the layout mark, the project file as the register of IDs
//! and paths, models in their own files, opening that tells (never refuses) a file missing,
//! unreadable or with an invalid path, the files found in the tree registered, and playing and
//! exporting that refuse what is used and cannot be read.
use oxy_core::{
    command,
    document::*,
    export, fs_ops,
    graph::Node,
    model_file,
    painting::PaintImage,
    persistence::{self, AssetState, KnownFiles},
    query,
};
use serde_json::{Value as Json, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// A folder of its own under the system's temporary folder, removed by the test.
fn folder(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-files-{label}-{}", new_id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `bytes` as the file `path` of the folder `root`, with the folders on its way.
fn put(root: &Path, path: &str, bytes: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

fn png() -> Vec<u8> {
    PaintImage::new(2, 2, [200, 40, 40, 255])
        .unwrap()
        .to_png()
        .unwrap()
}

/// A mono 16-bit PCM WAV of a few silent samples.
fn wav() -> Vec<u8> {
    let data = 16_u32;
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

/// A project of the tree layout saved in a folder of its own: the image `Imagens/Pedra.png` on
/// a box, the sound `Sons/Música.wav` a node of the box plays, and the model `Torre` (a group
/// with a block), saved and placed once.
struct Fixture {
    root: PathBuf,
    project: Project,
    known: KnownFiles,
    image: Id,
    sound: Id,
    model: Id,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = folder(label);
        put(&root, "Imagens/Pedra.png", &png());
        put(&root, "Sons/Música.wav", &wav());
        let mut project = Project::new("Árvore");
        project.layout = Layout::Tree;
        let scene = project.start_scene.clone();
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
        let image = add("Pedra", "Imagens/Pedra.png", AssetKind::Texture);
        let sound = add("Música", "Sons/Música.wav", AssetKind::Audio);
        let mut cube = Entity::new("Caixa", Some(Primitive::Cube));
        cube.material.texture = Some(image.clone());
        let mut play = Node::new("action.sound", [0., 0.]);
        play.params
            .insert("asset".into(), Value::Text(sound.clone()));
        cube.graph.nodes.push(play);
        let group = Entity::new("Torre", None);
        let mut block = Entity::new("Bloco", Some(Primitive::Cube));
        block.parent = Some(group.id.clone());
        let group_id = group.id.clone();
        project.scenes[0].entities.extend([cube, group, block]);
        let model = project.save_model(&scene, &group_id, "Torre").unwrap();
        project.instantiate_model(&model, &scene).unwrap();
        let mut known = KnownFiles::default();
        let told = persistence::save_project_with(&root, &project, &mut known).unwrap();
        assert!(told.is_empty(), "{told:?}");
        Self {
            root,
            project,
            known,
            image,
            sound,
            model,
        }
    }

    fn manifest(&self) -> Json {
        serde_json::from_slice(&fs::read(self.root.join(persistence::PROJECT_FILE)).unwrap())
            .unwrap()
    }

    fn open(&self) -> (Project, persistence::OpenReport) {
        persistence::open_project(&self.root).unwrap()
    }

    fn asset<'a>(&self, project: &'a Project, id: &str) -> &'a Asset {
        project.asset(id).unwrap()
    }

    fn moving(&self, from: &str, to: &str) {
        let target = self.root.join(to);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs_ops::move_no_replace(&self.root.join(from), &target).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_tree_project_round_trips_its_register_and_its_model_files() {
    let fixture = Fixture::new("ida-e-volta");
    let manifest = fixture.manifest();
    assert_eq!(manifest["layout"], "Tree");
    // The register: ID, name, path and kind; no pieces, not even an empty "model".
    for asset in manifest["assets"].as_array().unwrap() {
        let mut keys: Vec<&String> = asset.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["id", "kind", "name", "path"], "{asset}");
    }
    let model = fixture.asset(&fixture.project, &fixture.model);
    assert_eq!(model.path, "Torre.modelo.json");
    let bytes = fs::read(fixture.root.join("Torre.modelo.json")).unwrap();
    let pieces = model.model.clone().unwrap();
    assert_eq!(
        model_file::from_bytes(&bytes).unwrap(),
        (fixture.model.clone(), pieces)
    );
    assert_eq!(
        fixture.known.get(&fixture.model),
        Some(("Torre.modelo.json", bytes.as_slice()))
    );
    // Opened as the editor and the agents open it, and read strictly as the player would.
    let (opened, report) = fixture.open();
    assert_eq!(opened, fixture.project);
    assert_eq!(report.layout, Layout::Tree);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
    assert_eq!(
        report.known.get(&fixture.model),
        fixture.known.get(&fixture.model)
    );
    assert_eq!(
        persistence::load_project(&fixture.root).unwrap(),
        fixture.project
    );
    assert_eq!(
        persistence::load_project_lazy(&fixture.root).unwrap(),
        fixture.project
    );
    // The summary agents read gives the model its file.
    let summary = query::answer(&opened, &opened.start_scene, "summary", &json!({})).unwrap();
    let listed = summary["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "Model")
        .unwrap()
        .clone();
    assert_eq!(listed["path"], "Torre.modelo.json");
    assert_eq!(listed["name"], "Torre");
    // The strict load wants each model's file.
    fs::remove_file(fixture.root.join("Torre.modelo.json")).unwrap();
    let error = persistence::load_project(&fixture.root)
        .unwrap_err()
        .to_string();
    assert!(
        error.starts_with("O modelo Torre: o arquivo Torre.modelo.json não existe"),
        "{error}"
    );
}

#[cfg(windows)]
#[test]
fn a_model_file_is_written_only_when_its_bytes_change() {
    use std::os::windows::fs::OpenOptionsExt;
    let mut fixture = Fixture::new("sem-mudanca");
    let file = fixture.root.join("Torre.modelo.json");
    let before = fs::read(&file).unwrap();
    {
        // Held so that nobody may write it: a save that wrote it would fail.
        let _held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(&file)
            .unwrap();
        persistence::save_project_with(&fixture.root, &fixture.project, &mut fixture.known)
            .unwrap();
        let mut changed = fixture.project.clone();
        changed
            .assets
            .iter_mut()
            .find(|a| a.id == fixture.model)
            .unwrap()
            .model
            .as_mut()
            .unwrap()[1]
            .name = "Andar".into();
        assert!(
            persistence::save_project_with(&fixture.root, &changed, &mut fixture.known).is_err()
        );
    }
    assert_eq!(fs::read(&file).unwrap(), before);
}

#[test]
fn a_model_changed_outside_oxy_is_kept_before_the_save_replaces_it() {
    let fixture = Fixture::new("mudou-fora");
    let file = fixture.root.join("Torre.modelo.json");
    let (mut opened, mut report) = fixture.open();
    // Someone changes the file after OXY read it.
    let (id, mut pieces) = model_file::from_bytes(&fs::read(&file).unwrap()).unwrap();
    pieces[1].name = "Bloco de fora".into();
    let outside = model_file::to_bytes(&id, &pieces).unwrap();
    fs::write(&file, &outside).unwrap();
    let rename = |project: &mut Project, name: &str| {
        project
            .assets
            .iter_mut()
            .find(|a| a.id == fixture.model)
            .unwrap()
            .model
            .as_mut()
            .unwrap()[1]
            .name = name.into();
    };
    rename(&mut opened, "Andar");
    let told = persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].starts_with(
            "Torre (modelo): o arquivo Torre.modelo.json mudou fora da OXY desde que ela o leu; a \
             versão de fora foi guardada em .oxy/backup/modelos/"
        ),
        "{told:?}"
    );
    let backups: Vec<PathBuf> = fs::read_dir(fixture.root.join(".oxy/backup/modelos"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(&backups[0]).unwrap(), outside);
    // Named after the model, never its ID.
    let backup = backups[0].file_name().unwrap().to_str().unwrap();
    assert!(
        backup.starts_with("Torre-") && backup.ends_with(".modelo.json"),
        "{backup}"
    );
    assert!(!told[0].contains(&fixture.model), "{told:?}");
    let saved = model_file::from_bytes(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved.1[1].name, "Andar");
    // What OXY wrote itself, nobody changed since, is replaced without a copy.
    rename(&mut opened, "Andar de cima");
    let told = persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert!(told.is_empty(), "{told:?}");
    assert_eq!(
        fs::read_dir(fixture.root.join(".oxy/backup/modelos"))
            .unwrap()
            .count(),
        1
    );
    // The backups are outside the project: opening registers none.
    let (_, report) = fixture.open();
    assert_eq!(report.lines().count(), 0, "{:?}", report.fixes);
}

#[test]
fn a_model_changed_outside_oxy_and_not_in_it_keeps_the_outside_version() {
    let fixture = Fixture::new("so-fora");
    let file = fixture.root.join("Torre.modelo.json");
    let (mut opened, mut report) = fixture.open();
    // Someone changes the file after OXY read it; in OXY only something else changes.
    let (id, mut pieces) = model_file::from_bytes(&fs::read(&file).unwrap()).unwrap();
    pieces[1].name = "Bloco de fora".into();
    let outside = model_file::to_bytes(&id, &pieces).unwrap();
    fs::write(&file, &outside).unwrap();
    opened.scenes[0].entities.push(Entity::new("Outro", None));
    for _ in 0..2 {
        let told =
            persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
        assert_eq!(
            told,
            [
                "Torre (modelo): o arquivo Torre.modelo.json mudou fora da OXY desde que ela o \
                 leu; o salvamento o deixou como está, e reabrir o projeto carrega a versão de fora"
            ]
        );
        assert_eq!(fs::read(&file).unwrap(), outside);
    }
    assert!(!fixture.root.join(".oxy").exists(), "no backup");
    // Opened again, the model is the outside version.
    let (reopened, report) = fixture.open();
    let model = fixture.asset(&reopened, &fixture.model);
    assert_eq!(model.model.as_ref().unwrap()[1].name, "Bloco de fora");
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
}

#[test]
fn a_model_saved_again_under_the_name_of_one_that_left_takes_its_file() {
    let mut fixture = Fixture::new("de-novo");
    let scene = fixture.project.start_scene.clone();
    let house = Entity::new("Casa", Some(Primitive::Cube));
    let house_id = house.id.clone();
    fixture.project.scenes[0].entities.push(house);
    let first = fixture
        .project
        .save_model(&scene, &house_id, "Casa")
        .unwrap();
    persistence::save_project_with(&fixture.root, &fixture.project, &mut fixture.known).unwrap();
    let old = fs::read(fixture.root.join("Casa.modelo.json")).unwrap();
    // Taken out, as undoing the save of the model would, and saved again under its name.
    oxy_core::editing::remove_asset(&mut fixture.project, &first).unwrap();
    let second = fixture
        .project
        .save_model(&scene, &house_id, "Casa")
        .unwrap();
    assert_ne!(second, first);
    assert_eq!(
        fixture.asset(&fixture.project, &second).path,
        "Casa.modelo.json"
    );
    let told = persistence::save_project_with(&fixture.root, &fixture.project, &mut fixture.known)
        .unwrap();
    assert_eq!(
        told,
        [
            "A versão de Casa.modelo.json de um modelo que saiu do projeto foi para a lixeira do \
             projeto (.oxy/lixeira); o modelo novo ganhou o arquivo"
        ]
    );
    let written = fs::read(fixture.root.join("Casa.modelo.json")).unwrap();
    assert_eq!(model_file::from_bytes(&written).unwrap().0, second);
    let trash = fs_ops::trash_index(&fixture.root);
    let noted: Vec<(&str, Option<&str>)> = trash
        .values()
        .map(|entry| (entry.path.as_str(), entry.id.as_deref()))
        .collect();
    assert_eq!(noted, [("Casa.modelo.json", Some(first.as_str()))]);
    let kept: Vec<Vec<u8>> = fs::read_dir(fixture.root.join(fs_ops::TRASH))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".modelo.json"))
        .map(|path| fs::read(path).unwrap())
        .collect();
    assert_eq!(kept, [old]);
    // Saved again, and opened, nothing more moves.
    let told = persistence::save_project_with(&fixture.root, &fixture.project, &mut fixture.known)
        .unwrap();
    assert!(told.is_empty(), "{told:?}");
    let (opened, report) = fixture.open();
    assert_eq!(opened, fixture.project);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
}

#[test]
fn a_new_model_never_writes_over_a_file_that_is_not_its_own() {
    let mut fixture = Fixture::new("modelo-novo");
    let scene = fixture.project.start_scene.clone();
    let house = Entity::new("Casa", Some(Primitive::Cube));
    let house_id = house.id.clone();
    fixture.project.scenes[0].entities.push(house);
    let model = fixture
        .project
        .save_model(&scene, &house_id, "Casa")
        .unwrap();
    assert_eq!(
        fixture.asset(&fixture.project, &model).path,
        "Casa.modelo.json"
    );
    let manifest = fs::read(fixture.root.join(persistence::PROJECT_FILE)).unwrap();
    for theirs in [
        b"notas de outra pessoa".to_vec(),
        model_file::to_bytes(&new_id(), &[Entity::new("Outra casa", None)]).unwrap(),
    ] {
        put(&fixture.root, "Casa.modelo.json", &theirs);
        let error =
            persistence::save_project_with(&fixture.root, &fixture.project, &mut fixture.known)
                .unwrap_err()
                .to_string();
        assert_eq!(
            error,
            "Já existe Casa.modelo.json, e ele não é o arquivo do modelo Casa; a OXY não grava \
             por cima dele: mova ou renomeie esse arquivo e salve de novo"
        );
        assert_eq!(
            fs::read(fixture.root.join("Casa.modelo.json")).unwrap(),
            theirs
        );
        assert_eq!(
            fs::read(fixture.root.join(persistence::PROJECT_FILE)).unwrap(),
            manifest
        );
    }
    fs::remove_file(fixture.root.join("Casa.modelo.json")).unwrap();
    persistence::save_project_with(&fixture.root, &fixture.project, &mut fixture.known).unwrap();
    assert!(fixture.root.join("Casa.modelo.json").is_file());
    // A model's name is its file's: names Windows would not keep are refused, and in the tree
    // layout the file does not change its name yet.
    fixture.project.scenes[0]
        .entities
        .push(Entity::new("Porta", None));
    let door = fixture.project.scenes[0]
        .entities
        .last()
        .unwrap()
        .id
        .clone();
    let error = fixture
        .project
        .save_model(&scene, &door, "CON")
        .unwrap_err();
    assert!(error.contains("reserva para dispositivos"), "{error}");
    let renamed = command::apply(
        &mut fixture.project.clone(),
        &scene,
        &command::parse(&json!([{"op": "rename_asset", "asset": "Pedra", "name": "Rocha"}]))
            .unwrap(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        renamed.contains("o nome de um recurso é o nome do arquivo dele"),
        "{renamed}"
    );
}

#[test]
fn a_model_moved_or_renamed_outside_oxy_is_found_by_the_id_in_its_file() {
    let fixture = Fixture::new("religar");
    fixture.moving("Torre.modelo.json", "Fase1/Torre alta.modelo.json");
    let (opened, mut report) = fixture.open();
    let model = fixture.asset(&opened, &fixture.model);
    assert_eq!(model.path, "Fase1/Torre alta.modelo.json");
    assert_eq!(model.name, "Torre alta");
    assert_eq!(
        model.model,
        fixture.asset(&fixture.project, &fixture.model).model
    );
    assert_eq!(
        report.fixes,
        [
            "Torre (modelo): o arquivo foi encontrado em Fase1/Torre alta.modelo.json pelo ID que \
          tem dentro (o projeto o tinha em Torre.modelo.json)"
        ]
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Nothing was written: the next save writes it.
    assert_eq!(fixture.manifest()["assets"][2]["path"], "Torre.modelo.json");
    persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert_eq!(
        fixture.manifest()["assets"][2]["path"],
        "Fase1/Torre alta.modelo.json"
    );
    assert_eq!(fixture.open().1.lines().count(), 0);
    // A change of case alone is taken as the disk has it.
    fixture.moving(
        "Fase1/Torre alta.modelo.json",
        "Fase1/TORRE alta.modelo.json",
    );
    let (opened, report) = fixture.open();
    assert_eq!(
        fixture.asset(&opened, &fixture.model).path,
        "Fase1/TORRE alta.modelo.json"
    );
    assert!(report.fixes[0].contains("com essas maiúsculas e minúsculas"));
}

#[test]
fn a_file_of_the_same_name_in_another_folder_is_not_taken_for_a_missing_one() {
    let fixture = Fixture::new("mesmo-nome");
    fixture.moving("Imagens/Pedra.png", "Outras/Pedra.png");
    let (opened, report) = fixture.open();
    let image = fixture.asset(&opened, &fixture.image);
    assert_eq!(image.path, "Imagens/Pedra.png");
    assert_eq!(report.assets[&fixture.image], AssetState::Missing);
    assert!(
        report.warnings.contains(
            &"Pedra (imagem): o arquivo Imagens/Pedra.png não existe na pasta do projeto; jogar e \
          exportar ficam recusados enquanto o recurso for usado"
                .to_owned()
        )
    );
    // The other file is a new image of the project, whose name is now said twice.
    let other = opened
        .assets
        .iter()
        .find(|a| a.path == "Outras/Pedra.png")
        .unwrap();
    assert_eq!(
        (other.name.as_str(), other.kind),
        ("Pedra", AssetKind::Texture)
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("O nome \"Pedra\" é de 2 imagens")),
        "{:?}",
        report.warnings
    );
    // Playing and exporting refuse the image the box uses.
    let refused = persistence::assets_to_run(&opened, &fixture.root, |_| false).unwrap_err();
    assert_eq!(
        refused,
        "Pedra: o arquivo Imagens/Pedra.png não existe na pasta do projeto; restaure o arquivo ou \
         deixe de usar o recurso"
    );
    let found = export::check(&opened, Some(&fixture.root), &Default::default());
    assert!(
        found
            .errors
            .iter()
            .any(|e| e.starts_with("Pedra: o arquivo Imagens/Pedra.png")),
        "{found:?}"
    );
    // An image painted and not saved needs no file.
    persistence::assets_to_run(&opened, &fixture.root, |id| id == fixture.image).unwrap();
}

#[test]
fn files_found_in_the_tree_are_registered_with_the_same_id_until_saved() {
    let fixture = Fixture::new("registro");
    put(&fixture.root, "Sons/Bip.wav", &wav());
    let house = new_id();
    put(
        &fixture.root,
        "Pista/Casa.modelo.json",
        &model_file::to_bytes(&house, &[Entity::new("Casa", None)]).unwrap(),
    );
    // A copy of the tower made in the Explorer: the ID in it is taken.
    put(
        &fixture.root,
        "Torre - Cópia.modelo.json",
        &fs::read(fixture.root.join("Torre.modelo.json")).unwrap(),
    );
    let manifest = fs::read(fixture.root.join(persistence::PROJECT_FILE)).unwrap();
    let (first, mut report) = fixture.open();
    let (again, _) = fixture.open();
    assert_eq!(first, again, "the same IDs at every opening");
    assert_eq!(
        fs::read(fixture.root.join(persistence::PROJECT_FILE)).unwrap(),
        manifest,
        "opening writes nothing"
    );
    let added: Vec<(&str, &str, AssetKind)> = first.assets[fixture.project.assets.len()..]
        .iter()
        .map(|a| (a.path.as_str(), a.name.as_str(), a.kind))
        .collect();
    assert_eq!(
        added,
        [
            ("Pista/Casa.modelo.json", "Casa", AssetKind::Model),
            ("Sons/Bip.wav", "Bip", AssetKind::Audio),
            (
                "Torre - Cópia.modelo.json",
                "Torre - Cópia",
                AssetKind::Model
            ),
        ]
    );
    let by_path = |path: &str| first.assets.iter().find(|a| a.path == path).unwrap();
    assert_eq!(by_path("Pista/Casa.modelo.json").id, house);
    let copy = by_path("Torre - Cópia.modelo.json");
    assert_ne!(copy.id, fixture.model);
    assert_eq!(
        copy.model,
        fixture.asset(&fixture.project, &fixture.model).model
    );
    assert_eq!(
        report.fixes,
        [
            "Casa (modelo): arquivo novo na pasta do projeto, Pista/Casa.modelo.json",
            "Bip (som): arquivo novo na pasta do projeto, Sons/Bip.wav",
            "Torre - Cópia (modelo): arquivo novo na pasta do projeto, Torre - Cópia.modelo.json; \
             ele tem o ID de outro modelo (é uma cópia?) e ganhou um ID próprio",
        ]
    );
    // The save writes them, the copy with its own ID, and the next opening has nothing new.
    persistence::save_project_with(&fixture.root, &first, &mut report.known).unwrap();
    let (saved, report) = fixture.open();
    assert_eq!(saved, first);
    assert_eq!(report.lines().count(), 0, "{:?}", report.fixes);
    assert_eq!(
        model_file::from_bytes(&fs::read(fixture.root.join("Torre - Cópia.modelo.json")).unwrap())
            .unwrap()
            .0,
        copy.id
    );
}

#[test]
fn pixels_painted_on_an_image_found_in_the_tree_replace_it_only_while_it_is_unchanged() {
    use oxy_core::texture_cache::TextureCache;
    let fixture = Fixture::new("pintar");
    put(&fixture.root, "Imagens/Nova.png", &png());
    let (opened, mut report) = fixture.open();
    let new = opened
        .assets
        .iter()
        .find(|a| a.path == "Imagens/Nova.png")
        .unwrap()
        .id
        .clone();
    let painted = PaintImage::new(4, 4, [10, 200, 10, 255]).unwrap();
    let mut cache = TextureCache::new(1);
    cache.configure(&fixture.root, &opened);
    // Changed outside OXY after it was opened: the painting is refused, the file stays.
    let theirs = PaintImage::new(8, 8, [1, 2, 3, 255])
        .unwrap()
        .to_png()
        .unwrap();
    fs::write(fixture.root.join("Imagens/Nova.png"), &theirs).unwrap();
    cache.insert(new.clone(), painted.clone());
    let error = persistence::save_cached_bundle_with(
        &fixture.root.join(persistence::PROJECT_FILE),
        &opened,
        &mut cache,
        &mut report.known,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.ends_with("mudou fora da OXY; nada foi gravado"),
        "{error}"
    );
    assert_eq!(
        fs::read(fixture.root.join("Imagens/Nova.png")).unwrap(),
        theirs
    );
    // As opening found it, it is replaced.
    let (opened, mut report) = fixture.open();
    cache.configure(&fixture.root, &opened);
    cache.insert(new.clone(), painted.clone());
    persistence::save_cached_bundle_with(
        &fixture.root.join(persistence::PROJECT_FILE),
        &opened,
        &mut cache,
        &mut report.known,
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.root.join("Imagens/Nova.png")).unwrap(),
        painted.to_png().unwrap()
    );
}

#[test]
fn the_old_layout_registers_nothing_it_finds() {
    let root = folder("antigo");
    let mut project = Project::new("Antigo");
    let image = new_id();
    put(&root, &format!("assets/{image}.png"), &png());
    project.assets.push(Asset {
        id: image.clone(),
        name: "Pedra".into(),
        path: format!("assets/{image}.png"),
        kind: AssetKind::Texture,
        model: None,
    });
    persistence::save_project(&root, &project).unwrap();
    // What 0.6 left behind, and files anyone put there.
    put(&root, &format!("assets/{}.png", new_id()), &png());
    put(&root, "Sons/Bip.wav", &wav());
    let (opened, report) = persistence::open_project(&root).unwrap();
    assert_eq!(report.layout, Layout::Legacy);
    assert_eq!(opened, project);
    assert_eq!(report.lines().count(), 0);
    assert!(
        !fs::read_to_string(root.join(persistence::PROJECT_FILE))
            .unwrap()
            .contains("layout")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_tree_project_stays_in_the_tree_layout_with_a_missing_old_file() {
    let fixture = Fixture::new("marca");
    let mut manifest = fixture.manifest();
    let old = new_id();
    manifest["assets"][0]["path"] = json!(format!("assets/{old}.png"));
    put(
        &fixture.root,
        persistence::PROJECT_FILE,
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    let (opened, mut report) = fixture.open();
    assert_eq!(opened.layout, Layout::Tree);
    assert_eq!(report.assets[&fixture.image], AssetState::Missing);
    persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert_eq!(fixture.manifest()["layout"], "Tree");
}

#[test]
fn saved_paths_the_rules_refuse_open_with_a_warning_on_the_asset() {
    let fixture = Fixture::new("caminho");
    put(&fixture.root, "Imagens/Fora.png", &png());
    let mut manifest = fixture.manifest();
    manifest["assets"][1]["path"] = json!("Sons/con.wav");
    let assets = manifest["assets"].as_array_mut().unwrap();
    assets.push(json!({"id": new_id(), "name": "Fora", "path": "../Fora.png", "kind": "Texture"}));
    assets.push(
        json!({"id": new_id(), "name": "Pedra 2", "path": "imagens/pedra.png", "kind": "Texture"}),
    );
    assets
        .push(json!({"id": new_id(), "name": "Bip", "path": "Imagens/Fora.png", "kind": "Audio"}));
    put(
        &fixture.root,
        persistence::PROJECT_FILE,
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    let (opened, report) = fixture.open();
    let states: Vec<String> = opened.assets[3..6]
        .iter()
        .chain(std::iter::once(fixture.asset(&opened, &fixture.sound)))
        .map(|a| match &report.assets[&a.id] {
            AssetState::InvalidPath(why) => why.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        states,
        [
            "sai da pasta do projeto",
            "é o mesmo de Pedra (imagem)",
            "não termina em .wav",
            "\"con.wav\" é um nome que o Windows reserva para dispositivos",
        ]
    );
    assert!(
        report.warnings.contains(
            &"Música (som): o caminho Sons/con.wav é inválido (\"con.wav\" é um nome que o \
              Windows reserva para dispositivos); jogar e exportar ficam recusados enquanto o \
              recurso for usado"
                .to_owned()
        ),
        "{:?}",
        report.warnings
    );
    // The sound a node plays refuses the test; the strict load refuses the project.
    let refused = persistence::assets_to_run(&opened, &fixture.root, |_| false).unwrap_err();
    assert!(
        refused.starts_with("Música: o caminho Sons/con.wav é inválido"),
        "{refused}"
    );
    assert!(persistence::load_project(&fixture.root).is_err());
}

#[test]
fn an_entry_whose_file_went_and_that_nothing_uses_leaves_the_project() {
    let fixture = Fixture::new("entrada-morta");
    let mut manifest = fixture.manifest();
    let gone = new_id();
    manifest["assets"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": gone, "name": "Sobra", "path": "Sons/Sobra.wav", "kind": "Audio"}));
    put(
        &fixture.root,
        persistence::PROJECT_FILE,
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    let (opened, report) = fixture.open();
    assert!(opened.asset(&gone).is_none());
    assert_eq!(opened, fixture.project);
    assert_eq!(
        report.fixes,
        [
            "Sobra (som): o arquivo Sons/Sobra.wav não existe mais, e nada no projeto usava o \
          recurso; ele saiu da Biblioteca"
        ]
    );
    assert!(report.warnings.is_empty());
}

#[test]
fn a_model_file_that_cannot_be_used_leaves_the_model_unavailable_not_the_project() {
    let fixture = Fixture::new("modelo-ruim");
    put(&fixture.root, "Torre.modelo.json", b"{ quebrado");
    let (opened, mut report) = fixture.open();
    let model = fixture.asset(&opened, &fixture.model);
    assert_eq!(model.model, None);
    assert!(
        matches!(&report.assets[&fixture.model], AssetState::Broken(why) if why.starts_with("não é JSON válido")),
        "{:?}",
        report.assets
    );
    assert!(
        report.warnings[0].starts_with("Torre (modelo): o arquivo Torre.modelo.json não é JSON")
    );
    assert!(report.warnings[0].contains("o modelo fica indisponível"));
    // Its placement is a copy and stays; playing and exporting refuse while it is placed.
    assert_eq!(
        opened.scenes[0].entities.len(),
        fixture.project.scenes[0].entities.len()
    );
    let refused = persistence::assets_to_run(&opened, &fixture.root, |_| false).unwrap_err();
    assert!(
        refused.starts_with("Torre: o arquivo Torre.modelo.json não é JSON válido"),
        "{refused}"
    );
    let found = export::check(&opened, Some(&fixture.root), &Default::default());
    assert!(
        found.errors.iter().any(|e| e.starts_with("Torre: ")),
        "{found:?}"
    );
    // Saving keeps the entry and leaves the file as it is.
    persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert_eq!(
        fs::read(fixture.root.join("Torre.modelo.json")).unwrap(),
        b"{ quebrado"
    );
    assert_eq!(fixture.manifest()["assets"][2]["id"], json!(fixture.model));
    // Not placed, it counts only while an object attaches a script, which may place it by name.
    let mut unplaced = opened.clone();
    unplaced.scenes[0]
        .entities
        .retain(|e| e.model_source.as_deref() != Some(fixture.model.as_str()));
    persistence::assets_to_run(&unplaced, &fixture.root, |_| false).unwrap();
    unplaced.scenes[0].entities[0]
        .scripts
        .push(oxy_core::script::ScriptRef::new("scripts/porta.luau"));
    let refused = persistence::assets_to_run(&unplaced, &fixture.root, |_| false).unwrap_err();
    assert!(
        refused.starts_with("Torre: o arquivo Torre.modelo.json não é JSON válido"),
        "{refused}"
    );
    // A model whose pieces name what the project lacks does not fit it.
    let mut block = Entity::new("Gelo", Some(Primitive::Cube));
    block
        .attributes
        .insert("Chão".into(), Value::Surface(Some(new_id())));
    put(
        &fixture.root,
        "Torre.modelo.json",
        &model_file::to_bytes(&fixture.model, &[block]).unwrap(),
    );
    let (opened, report) = fixture.open();
    assert_eq!(fixture.asset(&opened, &fixture.model).model, None);
    assert!(
        matches!(&report.assets[&fixture.model], AssetState::Broken(why) if why.starts_with("não serve neste projeto: Gelo: referência de superfície ausente")),
        "{:?}",
        report.assets
    );
    // Playing and exporting say why, which reopening does not change.
    let refused = persistence::assets_to_run(&opened, &fixture.root, |_| false).unwrap_err();
    assert!(
        refused.starts_with(
            "Torre: o arquivo Torre.modelo.json não serve neste projeto: Gelo: referência de \
             superfície ausente"
        ),
        "{refused}"
    );
}

#[test]
fn a_missing_model_file_opens_with_a_warning_while_the_model_is_placed() {
    let fixture = Fixture::new("modelo-sumido");
    fs::remove_file(fixture.root.join("Torre.modelo.json")).unwrap();
    let (mut opened, mut report) = fixture.open();
    let model = fixture.asset(&opened, &fixture.model);
    assert_eq!(model.model, None);
    assert_eq!(model.path, "Torre.modelo.json");
    assert_eq!(report.assets[&fixture.model], AssetState::Missing);
    assert_eq!(
        report.warnings,
        [
            "Torre (modelo): o arquivo Torre.modelo.json não existe na pasta do projeto; jogar e \
             exportar ficam recusados enquanto o recurso for usado"
        ]
    );
    assert!(report.fixes.is_empty(), "{:?}", report.fixes);
    // Its placement is a copy and stays; playing and exporting refuse while it is placed.
    assert_eq!(opened.scenes, fixture.project.scenes);
    let refused = persistence::assets_to_run(&opened, &fixture.root, |_| false).unwrap_err();
    assert_eq!(
        refused,
        "Torre: o arquivo Torre.modelo.json não existe na pasta do projeto; restaure o arquivo ou \
         deixe de usar o recurso"
    );
    let found = export::check(&opened, Some(&fixture.root), &Default::default());
    assert!(found.errors.contains(&refused), "{found:?}");
    // Saved without its placement, the entry leaves at the next opening.
    opened.scenes[0]
        .entities
        .retain(|e| e.model_source.as_deref() != Some(fixture.model.as_str()));
    persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert!(!fixture.root.join("Torre.modelo.json").exists());
    let (reopened, report) = fixture.open();
    assert!(reopened.asset(&fixture.model).is_none());
    assert_eq!(
        report.fixes,
        [
            "Torre (modelo): o arquivo Torre.modelo.json não existe mais, e nada no projeto usava \
             o recurso; ele saiu da Biblioteca"
        ]
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_copy_takes_the_file_of_an_unavailable_model_as_it_is() {
    use oxy_core::texture_cache::TextureCache;
    let fixture = Fixture::new("copia");
    put(&fixture.root, "Torre.modelo.json", b"{ quebrado");
    let (opened, _) = fixture.open();
    let mut cache = TextureCache::new(1);
    cache.configure(&fixture.root, &opened);
    let mut known = KnownFiles::default();
    let copy = folder("copia-destino");
    let told = persistence::save_cached_copy_with(
        &copy.join(persistence::PROJECT_FILE),
        &opened,
        &mut cache,
        &fixture.root,
        &mut known,
    )
    .unwrap();
    assert!(told.is_empty(), "{told:?}");
    assert_eq!(
        fs::read(copy.join("Torre.modelo.json")).unwrap(),
        b"{ quebrado"
    );
    assert_eq!(fs::read(copy.join("Imagens/Pedra.png")).unwrap(), png());
    // One the source lacks is left out, with a warning, as an image is.
    fs::remove_file(fixture.root.join("Torre.modelo.json")).unwrap();
    let other = folder("copia-sem-modelo");
    let told = persistence::save_cached_copy_with(
        &other.join(persistence::PROJECT_FILE),
        &opened,
        &mut cache,
        &fixture.root,
        &mut known,
    )
    .unwrap();
    assert_eq!(
        told,
        ["Torre: o arquivo Torre.modelo.json não existe na pasta de origem e ficou fora da cópia"]
    );
    assert!(!other.join("Torre.modelo.json").exists());
    fs::remove_dir_all(copy).unwrap();
    fs::remove_dir_all(other).unwrap();
}

#[test]
fn files_whose_assets_left_the_project_go_to_its_trash_when_it_is_saved() {
    let fixture = Fixture::new("lixeira");
    put(&fixture.root, "Imagens/Sobra.png", &png());
    let (mut opened, mut report) = fixture.open();
    persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    let spare = opened
        .assets
        .iter()
        .find(|a| a.path == "Imagens/Sobra.png")
        .unwrap()
        .id
        .clone();
    oxy_core::editing::remove_asset(&mut opened, &spare).unwrap();
    let told = persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert_eq!(
        told,
        ["Imagens/Sobra.png não é mais do projeto e foi para a lixeira do projeto (.oxy/lixeira)"]
    );
    assert!(!fixture.root.join("Imagens/Sobra.png").exists());
    let trash = fs_ops::trash_index(&fixture.root);
    let noted: Vec<(&str, Option<&str>)> = trash
        .values()
        .map(|entry| (entry.path.as_str(), entry.id.as_deref()))
        .collect();
    assert_eq!(noted, [("Imagens/Sobra.png", Some(spare.as_str()))]);
    // The next opening does not take it back.
    let (reopened, report) = fixture.open();
    assert_eq!(reopened, opened);
    assert_eq!(report.lines().count(), 0);
}

#[test]
fn the_trash_takes_each_file_once_and_only_a_file_of_the_entrys_kind() {
    let fixture = Fixture::new("lixeira-regras");
    put(&fixture.root, "Sons/Bip.wav", &wav());
    put(&fixture.root, "notas.txt", b"minhas notas");
    let mut manifest = fixture.manifest();
    let assets = manifest["assets"].as_array_mut().unwrap();
    // Two entries of one file, and a sound whose saved path is someone's notes.
    for (name, path) in [
        ("Bip", "Sons/Bip.wav"),
        ("Bip 2", "Sons/Bip.wav"),
        ("Notas", "notas.txt"),
    ] {
        assets.push(json!({"id": new_id(), "name": name, "path": path, "kind": "Audio"}));
    }
    put(
        &fixture.root,
        persistence::PROJECT_FILE,
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    let (mut opened, mut report) = fixture.open();
    assert_eq!(opened.assets.len(), fixture.project.assets.len() + 3);
    opened.assets.truncate(fixture.project.assets.len());
    let told = persistence::save_project_with(&fixture.root, &opened, &mut report.known).unwrap();
    assert_eq!(
        told,
        ["Sons/Bip.wav não é mais do projeto e foi para a lixeira do projeto (.oxy/lixeira)"]
    );
    assert!(!fixture.root.join("Sons/Bip.wav").exists());
    assert_eq!(
        fs::read(fixture.root.join("notas.txt")).unwrap(),
        b"minhas notas"
    );
    assert_eq!(fs_ops::trash_index(&fixture.root).len(), 1);
    let (reopened, report) = fixture.open();
    assert_eq!(reopened, opened);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
}

#[test]
fn format_5_projects_still_open_as_they_were() {
    for (example, file) in [
        ("parkour-0.6", crate::parkour_example::fixture()),
        (
            "validacao",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/validacao")
                .join(persistence::PROJECT_FILE),
        ),
    ] {
        let saved: Json = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        let (opened, report) = persistence::open_project(&file).unwrap();
        assert_eq!(report.layout, Layout::Legacy, "{example}");
        assert_eq!(
            report.migrated.map(u64::from),
            saved["schema_version"].as_u64(),
            "{example}"
        );
        assert_eq!(
            report.lines().count(),
            0,
            "{example}: {:?}",
            report.warnings
        );
        assert_eq!(opened, persistence::load_project_lazy(&file).unwrap());
        assert!(
            opened
                .assets
                .iter()
                .filter(|a| a.kind == AssetKind::Model)
                .all(|a| a.path.is_empty() && a.model.is_some()),
            "{example}"
        );
    }
}
