//! The project's tree as agents reach it: the summary with the layout, the folders, the scripts
//! and how each asset stands on the disk; what a folder holds; the scripts listed with who runs
//! them; imports into a folder (made when missing, a taken name refused, a file of the tree
//! registered where it is); models saved in a folder; moves into a folder that is made; models
//! placed by their path; and the copies an agent keeps of the files a save changes.
use oxy_core::{
    asset_import::{self, ImportBatch, ImportRequest},
    command,
    document::*,
    painting::PaintImage,
    persistence::{self, KnownFiles},
    query,
    script::ScriptRef,
    script_files,
};
use serde_json::{Value as Json, json};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

fn png() -> Vec<u8> {
    PaintImage::new(2, 2, [90, 40, 200, 255])
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

/// A project in folders in a folder of its own: the sound `Pista/Pulo.wav` a node of Caixa plays,
/// the image `Imagens/Pedra.png` whose file went, the scripts `Pista/porta.luau` (attached to
/// Porta, requiring `Comum/dados.luau`) and `Menu/velho.luau` (unused, broken), the model Torre
/// at `Pista/Torre.modelo.json`, and notes OXY does not use in Pista.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oxy-biblioteca-{label}-{}", new_id()));
        put(&root, "Pista/Pulo.wav", &wav());
        put(&root, "Pista/notas.txt", b"notas");
        put(
            &root,
            "Pista/porta.luau",
            b"local dados = require(\"@project/Comum/dados\")\nlocal M = {}\nreturn M\n",
        );
        put(&root, "Comum/dados.luau", b"return { vida = 3 }\n");
        put(&root, "Menu/velho.luau", b"local M = {\nreturn M\n");
        fs::create_dir_all(root.join("Vazia")).unwrap();
        let mut project =
            oxy_core::editing::blank_project("Biblioteca", SceneKind::ThreeD).unwrap();
        project.layout = Layout::Tree;
        let scene = project.start_scene.clone();
        let sound = new_id();
        project.assets.push(Asset {
            id: sound.clone(),
            name: "Pulo".into(),
            path: "Pista/Pulo.wav".into(),
            kind: AssetKind::Audio,
            model: None,
        });
        let image = new_id();
        project.assets.push(Asset {
            id: image.clone(),
            name: "Pedra".into(),
            path: "Imagens/Pedra.png".into(),
            kind: AssetKind::Texture,
            model: None,
        });
        let mut cube = Entity::new("Caixa", Some(Primitive::Cube));
        cube.material.texture = Some(image);
        let mut play = oxy_core::graph::Node::new("action.sound", [0., 0.]);
        play.params.insert("asset".into(), Value::Text(sound));
        cube.graph.nodes.push(play);
        let mut door = Entity::new("Porta", Some(Primitive::Cube));
        door.scripts.push(ScriptRef::new("Pista/porta.luau"));
        let tower = Entity::new("Torre", Some(Primitive::Cube));
        let tower_id = tower.id.clone();
        project.scenes[0].entities.extend([cube, door, tower]);
        let model = project.save_model(&scene, &tower_id, "Torre").unwrap();
        project
            .assets
            .iter_mut()
            .find(|a| a.id == model)
            .unwrap()
            .path = "Pista/Torre.modelo.json".into();
        persistence::save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
        Self { root }
    }

    fn open(&self) -> (Project, KnownFiles) {
        let (project, report) = persistence::open_project(&self.root).unwrap();
        (project, report.known)
    }

    /// Applies `commands` as an agent's batch on files and saves the project.
    fn apply(&self, commands: Json) -> Result<Json, String> {
        let (mut project, mut known) = self.open();
        let parsed = command::parse(&commands)?;
        let start = project.start_scene.clone();
        let done = command::apply_on_files(
            &mut project,
            &start,
            &parsed,
            &self.root,
            &mut known,
            "Agente: teste",
        )?;
        persistence::save_project_with(&self.root, &project, &mut known)
            .map_err(|e| e.to_string())?;
        Ok(done.report)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn entry<'a>(list: &'a Json, key: &str, value: &str) -> &'a Json {
    list.as_array()
        .unwrap()
        .iter()
        .find(|e| e[key] == value)
        .unwrap_or_else(|| panic!("{key} {value} not in {list}"))
}

#[test]
fn the_summary_tells_the_layout_folders_scripts_and_how_each_asset_stands() {
    let fixture = Fixture::new("resumo");
    let (project, _) = fixture.open();
    let summary = query::summary_in(&project, Some(&fixture.root), &json!({}), &|_| false).unwrap();
    assert_eq!(summary["layout"], "tree");
    assert!(summary.get("conversion_pending").is_none());
    // Folders in tree order, each with what the Library shows in it.
    assert_eq!(
        summary["folders"],
        json!([{"path": "Comum", "items": 1}, {"path": "Menu", "items": 1},
               {"path": "Pista", "items": 3}, {"path": "Vazia", "items": 0}])
    );
    assert_eq!(summary["folders_total"], 4);
    let paged = query::summary_in(
        &project,
        Some(&fixture.root),
        &json!({"offset": 1, "limit": 2}),
        &|_| false,
    )
    .unwrap();
    assert_eq!(
        paged["folders"],
        json!([{"path": "Menu", "items": 1}, {"path": "Pista", "items": 3}])
    );
    assert_eq!(paged["folders_next_offset"], 3);
    // Every script, with the objects that attach it.
    let door = project.scenes[0].entities[1].id.clone();
    assert_eq!(
        summary["scripts"],
        json!([{"path": "Comum/dados.luau", "used_by": []},
               {"path": "Menu/velho.luau", "used_by": []},
               {"path": "Pista/porta.luau",
                "used_by": [{"entity": door, "name": "Porta", "role": "script"}]}])
    );
    // The image whose file went is missing; the model has its path; the rest have no state.
    let stone = entry(&summary["assets"], "name", "Pedra");
    assert_eq!(stone["state"], "missing");
    assert!(stone.get("problem").is_none());
    let tower = entry(&summary["assets"], "name", "Torre");
    assert_eq!(tower["path"], "Pista/Torre.modelo.json");
    assert!(tower.get("state").is_none(), "{tower}");
    assert!(
        entry(&summary["assets"], "name", "Pulo")
            .get("state")
            .is_none()
    );

    // A model saved in OXY but not yet in its file waits for the save; an image whose pixels
    // wait too, when the editor says so; a file that is not what its kind holds is an error.
    let mut unsaved = project.clone();
    let caixa = unsaved.scenes[0].entities[0].id.clone();
    let scene = unsaved.start_scene.clone();
    unsaved.save_model(&scene, &caixa, "Caixote").unwrap();
    let stone_id = stone["id"].as_str().unwrap().to_owned();
    put(&fixture.root, "Pista/Pulo.wav", b"nada de som");
    let checked = query::summary_in(&unsaved, Some(&fixture.root), &json!({}), &|id| {
        id == stone_id
    })
    .unwrap();
    assert_eq!(
        entry(&checked["assets"], "name", "Caixote")["state"],
        "unsaved"
    );
    assert_eq!(
        entry(&checked["assets"], "name", "Pedra")["state"],
        "unsaved"
    );
    let jump = entry(&checked["assets"], "name", "Pulo");
    assert_eq!(jump["state"], "error");
    assert!(jump["problem"].is_string(), "{jump}");
    // The validation tells the problems as opening does.
    let validation = query::validation_in(&project, Some(&fixture.root));
    let warnings = validation["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap()
            .starts_with("Pulo (som): o arquivo Pista/Pulo.wav não pode ser usado")),
        "{validation}"
    );
    assert!(
        warnings.iter().any(|w| w
            == "Pedra (imagem): o arquivo Imagens/Pedra.png não existe na pasta do projeto; \
                jogar e exportar ficam recusados enquanto o recurso for usado"),
        "{validation}"
    );

    // Without a folder the summary says only the layout; an old project waits to be organized.
    let alone = query::summary_in(&project, None, &json!({}), &|_| false).unwrap();
    assert_eq!(alone["layout"], "tree");
    assert!(alone.get("folders").is_none() && alone.get("scripts").is_none());
    let mut old = project.clone();
    old.layout = Layout::Legacy;
    let legacy = query::summary_in(&old, Some(&fixture.root), &json!({}), &|_| false).unwrap();
    assert_eq!(
        (&legacy["layout"], &legacy["conversion_pending"]),
        (&json!("legacy"), &json!(true))
    );
    assert!(legacy.get("folders").is_none());
    assert_eq!(legacy["scripts"].as_array().unwrap().len(), 3);
    // Bad arguments are refused before anything is read.
    for (args, refused) in [
        (json!({"model": "Torre", "folder": "Pista"}), "só um deles"),
        (json!({"kind": "Audio"}), "kind só vale com folder"),
        (json!({"folder": "Pista", "kind": "Som"}), "kind inválido"),
    ] {
        let error = query::ProjectQuery::parse(&args).unwrap_err();
        assert!(error.contains(refused), "{args}: {error}");
    }
}

#[test]
fn a_folder_lists_its_folders_then_its_items_with_the_missing_ones_in_place() {
    let fixture = Fixture::new("pasta");
    let (project, _) = fixture.open();
    let read = |args: Json| query::summary_in(&project, Some(&fixture.root), &args, &|_| false);
    let pista = read(json!({"folder": "pista"})).unwrap();
    assert_eq!(pista["folder"], "Pista");
    let items = pista["items"].as_array().unwrap();
    let kinds: Vec<(&str, &str)> = items
        .iter()
        .map(|i| (i["kind"].as_str().unwrap(), i["name"].as_str().unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [("Script", "porta"), ("Audio", "Pulo"), ("Model", "Torre")]
    );
    assert_eq!(pista["total"], 3);
    assert_eq!(pista["other_files"], 1);
    let jump = entry(&pista["items"], "name", "Pulo");
    assert_eq!(jump["path"], "Pista/Pulo.wav");
    assert_eq!(jump["used_by_count"], 1);
    assert!(jump["id"].is_string());
    assert_eq!(entry(&pista["items"], "name", "porta")["used_by_count"], 1);
    // The root: folders first, in tree order.
    let root = read(json!({"folder": ""})).unwrap();
    let names: Vec<&str> = root["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Comum", "Menu", "Pista", "Vazia"]);
    assert_eq!(entry(&root["items"], "name", "Pista")["items"], 3);
    // An asset whose file went is listed in the folder of its path, missing.
    let images = read(json!({"folder": "Imagens"}));
    assert!(
        images
            .unwrap_err()
            .contains("A pasta Imagens não existe no projeto")
    );
    fs::create_dir_all(fixture.root.join("Imagens")).unwrap();
    let images = read(json!({"folder": "Imagens"})).unwrap();
    assert_eq!(images["items"][0]["name"], "Pedra");
    assert_eq!(images["items"][0]["state"], "missing");
    // One kind, paged.
    let scripts = read(json!({"folder": "Pista", "kind": "Script"})).unwrap();
    assert_eq!(scripts["total"], 1);
    let page = read(json!({"folder": "Pista", "offset": 1, "limit": 1})).unwrap();
    assert_eq!(page["items"][0]["name"], "Pulo");
    assert_eq!(page["next_offset"], 2);
    let error = read(json!({"folder": "Nada"})).unwrap_err();
    assert!(
        error.contains("A pasta Nada não existe no projeto; pastas: Comum, Imagens, Menu"),
        "{error}"
    );
    // Without a folder of its own, a project has none to list.
    let error = query::summary_in(&project, None, &json!({"folder": "Pista"}), &|_| false);
    assert!(error.unwrap_err().contains("não foi salvo numa pasta"));
}

#[test]
fn scripts_are_listed_with_who_runs_them_and_the_check_tells_the_unused() {
    let fixture = Fixture::new("scripts");
    let (project, _) = fixture.open();
    let listed = query::scripts_list(Some(&project), &fixture.root).unwrap();
    assert_eq!(listed["total"], 3);
    let scripts = &listed["scripts"];
    let door = entry(scripts, "path", "Pista/porta.luau");
    assert_eq!(door["kind"], "script");
    assert_eq!(door["lines"], 3);
    assert_eq!(door["requires"], json!(["Comum/dados.luau"]));
    assert_eq!(door["used_by"][0]["name"], "Porta");
    let data = entry(scripts, "path", "Comum/dados.luau");
    assert_eq!(data["kind"], "module");
    assert_eq!(data["required_by"], json!(["Pista/porta.luau"]));
    assert_eq!(data["used_by"], json!([]));
    let old = entry(scripts, "path", "Menu/velho.luau");
    assert_eq!(old["kind"], "unused");
    assert!(
        old["error"]
            .as_str()
            .unwrap()
            .starts_with("Menu/velho.luau:"),
        "{old}"
    );
    // Through oxy_scripts: list, and the check of every script separates the unused.
    let mut kept = HashSet::new();
    let answer = |request: Json, kept: &mut HashSet<PathBuf>| {
        let request = script_files::Request::parse(&request).unwrap();
        script_files::answer(&fixture.root, Some(&project), &request, kept).map(|a| a.json)
    };
    assert_eq!(
        answer(json!({"action": "list"}), &mut kept).unwrap(),
        listed
    );
    let checked = answer(json!({"action": "check"}), &mut kept).unwrap();
    assert_eq!(checked["unused"], json!(["Menu/velho.luau"]));
    // Renaming and deleting are the project's commands, never answered here.
    let rename = json!({"action": "rename", "path": "Menu/velho.luau", "to": "velho.luau"});
    assert!(
        answer(rename, &mut kept)
            .unwrap_err()
            .contains("comandos do projeto")
    );
}

#[test]
fn an_import_named_after_a_file_like_an_id_says_so() {
    let fixture = Fixture::new("importar-id");
    let sources = fixture.root.with_extension("origem");
    let id = "1d76fcf6-74c0-40ed-8168-64ef00b254fb";
    put(&sources, &format!("{id}.wav"), &wav());
    put(&sources, "vento.wav", &wav());
    let path = |name: &str| sources.join(name).display().to_string();
    let (mut project, _) = fixture.open();
    let scene = project.start_scene.clone();
    let import = |project: &mut Project, args: Json| {
        let batch = ImportBatch::parse(&args).unwrap();
        let read = batch.read().unwrap();
        asset_import::import_batch(project, &fixture.root, &scene, &batch, &read).unwrap()
    };
    let done = import(&mut project, json!({"path": path(&format!("{id}.wav"))}));
    assert_eq!(done[0].name, id);
    assert_eq!(
        done[0].warning.as_deref(),
        Some(
            "o nome 1d76fcf6-74c0-40ed-8168-64ef00b254fb veio do nome do arquivo e parece um \
             ID; dê um nome que a pessoa reconheça com rename_asset (ou importe com name)"
        )
    );
    let answer = serde_json::to_value(&done[0]).unwrap();
    assert!(answer["warning"].is_string());
    let done = import(&mut project, json!({"path": path("vento.wav")}));
    assert!(done[0].warning.is_none());
    assert!(
        serde_json::to_value(&done[0])
            .unwrap()
            .get("warning")
            .is_none()
    );
    let _ = fs::remove_dir_all(&sources);
}

#[test]
fn imports_go_to_a_folder_made_when_missing_and_never_take_a_name_twice() {
    let fixture = Fixture::new("importar");
    let sources = fixture.root.with_extension("origem");
    put(&sources, "musica.wav", &wav());
    put(&sources, "pulo.wav", &wav());
    put(&sources, "rocha.png", &png());
    let path = |name: &str| sources.join(name).display().to_string();
    let (mut project, _) = fixture.open();
    let scene = project.start_scene.clone();
    let import = |project: &mut Project, args: Json| {
        let batch = ImportBatch::parse(&args)?;
        let read = batch.read()?;
        asset_import::import_batch(project, &fixture.root, &scene, &batch, &read)
    };
    // Into a folder made with the one above it, the file named after the asset.
    let done = import(
        &mut project,
        json!({"path": path("musica.wav"), "name": "Música", "folder": "Sons/Fundo"}),
    )
    .unwrap();
    assert_eq!(done[0].path, "Sons/Fundo/Música.wav");
    assert_eq!(done[0].made, ["Sons", "Sons/Fundo"]);
    assert!(fixture.root.join("Sons/Fundo/Música.wav").is_file());
    // A name a sound has, anywhere, is refused with a free one; nothing is made.
    let before = project.clone();
    let error = import(
        &mut project,
        json!({"path": path("pulo.wav"), "folder": "Efeitos"}),
    )
    .unwrap_err();
    assert!(
        error.contains("Já existe o som \"Pulo\" (Pista/Pulo.wav)")
            && error.contains("passe outro name (pulo 2 está livre)"),
        "{error}"
    );
    assert_eq!(project, before);
    assert!(!fixture.root.join("Efeitos").exists());
    // A name Windows would not keep is refused with why.
    let error = import(
        &mut project,
        json!({"path": path("pulo.wav"), "name": "Pulo: alto"}),
    );
    assert!(error.unwrap_err().contains("Nome inválido"));
    // A batch that fails takes away the copies and the folders it made.
    let error = import(
        &mut project,
        json!({"folder": "Novos", "paths": [
            {"path": path("rocha.png"), "name": "Rocha"},
            {"path": path("pulo.wav"), "name": "Música", "folder": "Novos/Sons"},
        ]}),
    )
    .unwrap_err();
    assert!(
        error.starts_with("paths[1] (o 2º arquivo): Já existe o som"),
        "{error}"
    );
    assert_eq!(project, before);
    assert!(!fixture.root.join("Novos").exists());
    // A file already in the project's tree is registered where it is, nothing copied.
    put(&fixture.root, "Pista/Vento.wav", &wav());
    let (mut project, _) = fixture.open();
    let registered = project
        .assets
        .iter()
        .find(|a| a.path == "Pista/Vento.wav")
        .unwrap()
        .id
        .clone();
    let inside = fixture
        .root
        .join("Pista")
        .join("Vento.wav")
        .display()
        .to_string();
    let done = import(&mut project, json!({"path": inside})).unwrap();
    assert_eq!(
        (&done[0].asset, done[0].path.as_str(), done[0].in_place),
        (&registered, "Pista/Vento.wav", true)
    );
    let again = import(&mut project, json!({"path": inside, "name": "Brisa"})).unwrap_err();
    assert!(again.contains("use oxy_apply rename_asset"), "{again}");
    let moved = import(&mut project, json!({"path": inside, "folder": "Sons"})).unwrap_err();
    assert!(moved.contains("use oxy_apply move"), "{moved}");
    // In the old layout a folder waits for the project to be organized.
    let mut old = project.clone();
    old.layout = Layout::Legacy;
    let error = import(
        &mut old,
        json!({"path": path("rocha.png"), "folder": "Imagens"}),
    );
    assert!(error.unwrap_err().contains("formato antigo"));
    // The root is no folder to wait for: "" and "/" are where a file goes without one.
    for root in ["", "/"] {
        let request =
            ImportRequest::parse(&json!({"path": path("rocha.png"), "folder": root})).unwrap();
        assert_eq!(request.folder, None, "{root:?}");
    }
    let done = import(&mut old, json!({"path": path("rocha.png"), "folder": ""})).unwrap();
    assert!(done[0].path.starts_with("assets/"), "{}", done[0].path);
    let _ = fs::remove_dir_all(&sources);
}

#[test]
fn models_go_to_a_folder_moves_make_theirs_and_models_are_placed_by_path() {
    let fixture = Fixture::new("modelos");
    // save_model with a folder: the file at <folder>/<name>.modelo.json, the folder made now.
    let saved = fixture
        .apply(json!([
            {"op": "save_model", "entity": "Caixa", "name": "Caixote", "folder": "Modelos/Caixas"},
        ]))
        .unwrap();
    assert!(saved["created"]["0"][0]["id"].is_string());
    assert_eq!(
        saved["created_folders"],
        json!(["Modelos", "Modelos/Caixas"])
    );
    assert!(
        fixture
            .root
            .join("Modelos/Caixas/Caixote.modelo.json")
            .is_file()
    );
    let (project, _) = fixture.open();
    assert_eq!(
        project.model_named("Caixote").unwrap().path,
        "Modelos/Caixas/Caixote.modelo.json"
    );
    // A file the folder has is refused with a free name; replace keeps the model's own file.
    put(
        &fixture.root,
        "Pista/Barril.modelo.json",
        b"de outra pessoa",
    );
    for (commands, refused) in [
        (
            json!([{"op": "save_model", "entity": "Caixa", "name": "Barril", "folder": "pista"}]),
            "Já existe Barril.modelo.json em Pista; a OXY não grava por cima: escolha outro nome \
             com name (Barril 2 está livre lá)",
        ),
        (
            json!([{"op": "save_model", "entity": "Caixa", "replace": "Torre", "folder": "Menu"}]),
            "folder não vale com replace",
        ),
    ] {
        let error = fixture.apply(commands.clone()).unwrap_err();
        assert!(error.contains(refused), "{commands}: {error}");
    }
    assert_eq!(
        fs::read(fixture.root.join("Pista/Barril.modelo.json")).unwrap(),
        b"de outra pessoa"
    );
    // move to a folder that does not exist makes it, with the folders above.
    let moved = fixture
        .apply(json!([{"op": "move", "items": ["Pista/Torre.modelo.json"], "to": "Fases/Um"}]))
        .unwrap();
    assert_eq!(
        moved["moved"],
        json!([{"from": "Pista/Torre.modelo.json", "to": "Fases/Um/Torre.modelo.json"}])
    );
    assert_eq!(moved["created_folders"], json!(["Fases", "Fases/Um"]));
    // place_model takes the model's file by its path, with or without the extension.
    let placed = fixture
        .apply(json!([
            {"op": "place_model", "model": "Fases/Um/Torre.modelo.json"},
            {"op": "place_model", "model": "fases/um/torre", "name": "Torre 2"},
        ]))
        .unwrap();
    assert_eq!(placed["created"]["0"][0]["name"], "Torre");
    assert_eq!(placed["created"]["1"][0]["name"], "Torre 2");
    // Assets of different kinds that share a name are told apart by kind or path.
    put(&fixture.root, "Sons/Torre.wav", &wav());
    let error = fixture
        .apply(json!([{"op": "rename_asset", "asset": "torre", "name": "Torreão"}]))
        .unwrap_err();
    assert!(
        error.contains(
            "\"torre\" é o nome de 2 recursos: Torre (modelo, Fases/Um/Torre.modelo.json), \
             Torre (áudio, Sons/Torre.wav); passe kind (Model ou Audio) ou o caminho do arquivo"
        ),
        "{error}"
    );
    fixture
        .apply(json!([{"op": "rename_asset", "asset": "Sons/Torre.wav", "name": "Torreão"}]))
        .unwrap();
    assert!(fixture.root.join("Sons/Torreão.wav").is_file());
    // In the old layout a folder for a model waits for the project to be organized (the other
    // person's file, no model, goes first).
    fs::remove_file(fixture.root.join("Pista/Barril.modelo.json")).unwrap();
    let (mut old, _) = fixture.open();
    old.layout = Layout::Legacy;
    let scene = old.start_scene.clone();
    let caixa = old.scenes[0].entities[0].id.clone();
    let error = command::apply(
        &mut old,
        &scene,
        &command::parse(&json!([
            {"op": "save_model", "entity": caixa, "name": "Velho", "folder": "Modelos"},
        ]))
        .unwrap(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("formato antigo"), "{error}");
    // The root is no folder: the model goes where it would without one.
    let at_root = command::parse(&json!([
        {"op": "save_model", "entity": caixa, "name": "Velho", "folder": " / "},
    ]))
    .unwrap();
    assert!(!command::needs_tree_layout(&at_root));
    command::apply(&mut old, &scene, &at_root).unwrap();
    assert!(command::needs_tree_layout(
        &command::parse(&json!([{"op": "delete_asset", "asset": "a.luau"}])).unwrap()
    ));
}

#[test]
fn an_agent_keeps_the_project_file_and_the_models_it_changes_once_per_session() {
    let fixture = Fixture::new("copias");
    let file = fixture.root.join(persistence::PROJECT_FILE);
    let (mut project, _) = fixture.open();
    let original = fs::read(&file).unwrap();
    let mut kept = HashSet::new();
    // Nothing of the models changes: the project file alone.
    let copies = persistence::keep_agent_copies(&file, &project, &mut kept).unwrap();
    assert_eq!(copies, [".oxy/agente/project.oxy.json.bak"]);
    let copy = fixture.root.join(".oxy/agente/project.oxy.json.bak");
    assert_eq!(fs::read(&copy).unwrap(), original);
    // A model the save would change is kept as its file is.
    let model_file = fs::read(fixture.root.join("Pista/Torre.modelo.json")).unwrap();
    let tower = project
        .assets
        .iter_mut()
        .find(|a| a.name == "Torre")
        .unwrap();
    tower.model.as_mut().unwrap()[0].name = "Torre alta".into();
    let copies = persistence::keep_agent_copies(&file, &project, &mut kept).unwrap();
    assert_eq!(
        copies,
        [
            ".oxy/agente/project.oxy.json.bak",
            ".oxy/agente/Pista/Torre.modelo.json.bak"
        ]
    );
    assert_eq!(
        fs::read(fixture.root.join(".oxy/agente/Pista/Torre.modelo.json.bak")).unwrap(),
        model_file
    );
    // Later writes of the session keep the copies as the session found the files.
    persistence::save_project_with(&fixture.root, &project, &mut KnownFiles::default()).unwrap();
    persistence::keep_agent_copies(&file, &project, &mut kept).unwrap();
    assert_eq!(fs::read(&copy).unwrap(), original);
    // A copy someone deleted is taken again; another session starts from the files as they are.
    fs::remove_file(&copy).unwrap();
    persistence::keep_agent_copies(&file, &project, &mut kept).unwrap();
    assert_eq!(fs::read(&copy).unwrap(), fs::read(&file).unwrap());
    // The copies are out of the project's tree.
    let (reopened, _) = fixture.open();
    assert!(
        reopened.assets.iter().all(|a| !a.path.starts_with(".oxy")),
        "{:?}",
        reopened.assets
    );
}
