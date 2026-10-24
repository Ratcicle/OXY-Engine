//! The commands of the project's tree as the editor and the agents run them: folders made,
//! renamed and deleted, files moved, renamed and sent to the project's trash, every reference
//! following, each one an undo step with its files; undo refused while the disk changed
//! outside OXY or a file is held, the trash emptied cutting the undo of the steps that need it,
//! "Descartar alterações" bringing everything back, and the scan telling what changed outside.
use oxy_core::{
    command::{self, Command},
    document::*,
    edit_history::CommandHistory,
    fs_ops, journal,
    painting::PaintImage,
    persistence::{self, KnownFiles},
    project_tree,
    script::ScriptRef,
    texture_cache::TextureCache,
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn put(root: &Path, path: &str, bytes: &[u8]) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

fn png(color: u8) -> Vec<u8> {
    PaintImage::new(2, 2, [color, 40, 40, 255])
        .unwrap()
        .to_png()
        .unwrap()
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

/// Every file and folder under `root` but OXY's own folder, by its path with `/` (folders end
/// in `/`), with its bytes.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![(root.to_owned(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if name == ".oxy" {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                found.insert(format!("{name}/"), Vec::new());
                pending.push((entry.path(), format!("{name}/")));
            } else {
                found.insert(name, fs::read(entry.path()).unwrap());
            }
        }
    }
    found
}

/// A project in folders saved in a folder of its own: the image `Imagens/Pedra.png` on the box
/// Caixa, the sound `Sons/Pulo.wav` a node of the box plays, the unused sounds `Sons/Bip.wav`
/// and `Menu/Clique.wav`, `scripts/porta.luau` attached to Porta and to the block of the model
/// Torre (`Torre.modelo.json`, placed once), and in Menu a Thumbs.db, a `.vscode` folder and
/// notes whose name the root has too.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oxy-arvore-{label}-{}", new_id()));
        put(&root, "Imagens/Pedra.png", &png(200));
        put(&root, "Sons/Pulo.wav", &wav(4));
        put(&root, "Sons/Bip.wav", &wav(6));
        put(&root, "Menu/Clique.wav", &wav(8));
        put(&root, "Menu/Thumbs.db", b"miniaturas");
        put(&root, "Menu/notas.txt", b"notas do menu");
        put(&root, "Menu/.vscode/settings.json", b"{}");
        put(&root, "notas.txt", b"notas da raiz");
        put(&root, "scripts/porta.luau", b"return {}\n");
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
        let sound = add("Pulo", "Sons/Pulo.wav", AssetKind::Audio);
        add("Bip", "Sons/Bip.wav", AssetKind::Audio);
        add("Clique", "Menu/Clique.wav", AssetKind::Audio);
        let mut cube = Entity::new("Caixa", Some(Primitive::Cube));
        cube.material.texture = Some(image);
        let mut play = oxy_core::graph::Node::new("action.sound", [0., 0.]);
        play.params.insert("asset".into(), Value::Text(sound));
        cube.graph.nodes.push(play);
        let mut door = Entity::new("Porta", Some(Primitive::Cube));
        door.scripts.push(ScriptRef::new("scripts/porta.luau"));
        let group = Entity::new("Torre", None);
        let mut block = Entity::new("Bloco", Some(Primitive::Cube));
        block.parent = Some(group.id.clone());
        block.scripts.push(ScriptRef::new("scripts/porta.luau"));
        let group_id = group.id.clone();
        project.scenes[0]
            .entities
            .extend([cube, door, group, block]);
        let model = project.save_model(&scene, &group_id, "Torre").unwrap();
        project.instantiate_model(&model, &scene).unwrap();
        persistence::save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The project as the editor holds it: the document, its images, its undo history and what OXY
/// knows of its files.
struct Session {
    root: PathBuf,
    project: Project,
    images: TextureCache,
    history: CommandHistory,
    known: KnownFiles,
}

impl Session {
    fn open(root: &Path) -> Self {
        let (project, report) = persistence::open_project(root).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let mut images = TextureCache::default();
        images.configure(root, &project);
        Self {
            root: root.to_owned(),
            project,
            images,
            history: CommandHistory::new(),
            known: report.known,
        }
    }

    /// Runs `commands` as one undo step, as the editor runs an agent's batch.
    fn run(&mut self, commands: Json) -> Result<Json, String> {
        let commands = command::parse(&commands)?;
        let label = format!("Agente: {}", commands[0].label());
        self.history
            .begin(label.clone(), &self.project, &self.images);
        let scene = self.project.start_scene.clone();
        match command::apply_on_files(
            &mut self.project,
            &scene,
            &commands,
            &self.root,
            &mut self.known,
            &label,
        ) {
            Ok(done) => {
                self.history
                    .commit_with_files(&self.project, &mut self.images, done.files)
                    .unwrap();
                Ok(done.report)
            }
            Err(error) => {
                assert!(error.kept.is_empty(), "{error}");
                self.history.cancel(&mut self.project, &mut self.images);
                Err(error.message)
            }
        }
    }

    fn undo(&mut self) -> Result<Option<String>, String> {
        self.history.undo_with(
            &mut self.project,
            &mut self.images,
            Some((&self.root, &mut self.known)),
        )
    }

    fn redo(&mut self) -> Result<Option<String>, String> {
        self.history.redo_with(
            &mut self.project,
            &mut self.images,
            Some((&self.root, &mut self.known)),
        )
    }

    fn save(&mut self) -> Vec<String> {
        let told =
            persistence::save_project_with(&self.root, &self.project, &mut self.known).unwrap();
        self.history.mark_saved();
        told
    }

    fn asset(&self, name: &str) -> &Asset {
        self.project
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .unwrap_or_else(|| panic!("{name}"))
    }

    fn entity(&self, name: &str) -> &Entity {
        self.project
            .scenes
            .iter()
            .flat_map(|scene| &scene.entities)
            .chain(
                self.project
                    .assets
                    .iter()
                    .filter_map(|asset| asset.model.as_ref())
                    .flatten(),
            )
            .find(|entity| entity.name == name)
            .unwrap_or_else(|| panic!("{name}"))
    }

    fn scripts(&self, name: &str) -> Vec<String> {
        self.entity(name)
            .scripts
            .iter()
            .map(|script| script.path.clone())
            .collect()
    }

    /// The pieces of the model Torre attach these scripts.
    fn block_scripts(&self) -> Vec<String> {
        self.asset("Torre").model.as_ref().unwrap()[1]
            .scripts
            .iter()
            .map(|script| script.path.clone())
            .collect()
    }
}

#[test]
fn each_tree_command_moves_its_files_and_references_and_undoes_and_redoes() {
    let fixture = Fixture::new("comandos");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let mut states = vec![(session.project.clone(), files(root))];
    let mut step = |session: &mut Session, commands: Json| -> Json {
        let answer = session.run(commands).unwrap();
        assert!(!root.join(journal::JOURNAL).exists() || !session.known.journal().is_clean());
        states.push((session.project.clone(), files(root)));
        answer
    };

    // A folder, with the one on its way: a step of files alone, which leaves the project unsaved.
    let made = step(
        &mut session,
        json!([{"op": "create_folder", "path": "Pista/Sons"}]),
    );
    assert!(made.get("moved").is_none());
    assert!(root.join("Pista/Sons").is_dir());
    assert!(session.history.is_dirty());
    // A sound, a script and a model's file into a folder: every reference follows.
    let moved = step(
        &mut session,
        json!([{"op": "move", "items": ["Sons/Pulo.wav", "scripts/porta.luau", "Torre"],
                "to": "Pista"}]),
    );
    assert_eq!(
        moved["moved"],
        json!([
            {"from": "Sons/Pulo.wav", "to": "Pista/Pulo.wav"},
            {"from": "scripts/porta.luau", "to": "Pista/porta.luau"},
            {"from": "Torre.modelo.json", "to": "Pista/Torre.modelo.json"},
        ])
    );
    assert_eq!(session.asset("Pulo").path, "Pista/Pulo.wav");
    assert_eq!(session.asset("Torre").path, "Pista/Torre.modelo.json");
    assert_eq!(session.scripts("Porta"), ["Pista/porta.luau"]);
    assert_eq!(session.block_scripts(), ["Pista/porta.luau"]);
    assert!(root.join("Pista/Torre.modelo.json").is_file());
    assert!(!root.join("Torre.modelo.json").exists());
    // The folder, by any case, renamed: what is inside goes with it.
    step(
        &mut session,
        json!([{"op": "rename_folder", "folder": "pista", "name": "Corrida"}]),
    );
    assert_eq!(session.asset("Pulo").path, "Corrida/Pulo.wav");
    assert_eq!(session.scripts("Porta"), ["Corrida/porta.luau"]);
    assert!(root.join("Corrida/Sons").is_dir());
    // A sound renamed renames its file; nodes keep its ID.
    let pulo = session.asset("Pulo").id.clone();
    step(
        &mut session,
        json!([{"op": "rename_asset", "asset": "Pulo", "name": "Salto"}]),
    );
    assert_eq!(session.asset("Salto").path, "Corrida/Salto.wav");
    assert_eq!(session.asset("Salto").id, pulo);
    assert!(root.join("Corrida/Salto.wav").is_file());
    // A script renamed by its path: the objects that attach it follow.
    step(
        &mut session,
        json!([{"op": "rename_asset", "asset": "corrida/porta.luau", "name": "portao",
                "kind": "Script"}]),
    );
    assert_eq!(session.scripts("Porta"), ["Corrida/portao.luau"]);
    assert_eq!(session.block_scripts(), ["Corrida/portao.luau"]);
    // A sound nothing uses goes to the project's trash.
    let deleted = step(
        &mut session,
        json!([{"op": "delete_asset", "asset": "Bip"}]),
    );
    assert_eq!(deleted["trashed"], json!(["Sons/Bip.wav"]));
    assert!(
        session
            .project
            .assets
            .iter()
            .all(|asset| asset.name != "Bip")
    );
    assert!(!root.join("Sons/Bip.wav").exists());
    // A script attached is taken off its objects only when asked.
    let error = session
        .run(json!([{"op": "delete_asset", "asset": "Corrida/portao.luau", "kind": "Script"}]))
        .unwrap_err();
    assert!(
        // Porta, and Bloco in the scene, in the model and in its placement.
        error.contains("anexado a 4 objetos (Porta, Bloco)") && error.contains("detach: true"),
        "{error}"
    );
    step(
        &mut session,
        json!([{"op": "delete_asset", "asset": "Corrida/portao.luau", "kind": "Script",
                "detach": true}]),
    );
    assert!(session.scripts("Porta").is_empty() && session.block_scripts().is_empty());
    assert!(!root.join("Corrida/portao.luau").exists());
    // A folder deleted: what it holds goes up to the root, a name taken there numbered,
    // Thumbs.db to the trash; no asset leaves.
    let gone = step(
        &mut session,
        json!([{"op": "delete_folder", "folder": "Menu"}]),
    );
    assert_eq!(session.asset("Clique").path, "Clique.wav");
    assert_eq!(
        fs::read(root.join("notas 2.txt")).unwrap(),
        b"notas do menu"
    );
    assert_eq!(fs::read(root.join("notas.txt")).unwrap(), b"notas da raiz");
    assert!(root.join(".vscode/settings.json").is_file());
    assert!(!root.join("Menu").exists());
    let told = gone["warnings"].to_string();
    assert!(
        told.contains("Menu/notas.txt foi para a raiz do projeto como notas 2.txt"),
        "{told}"
    );
    assert!(told.contains("Menu/.vscode fica fora do projeto"), "{told}");
    assert_eq!(session.history.undo_len(), 8);

    // Undone step by step, each state comes back, files included; then redone.
    for (index, (project, disk)) in states.iter().enumerate().rev().skip(1) {
        session.undo().unwrap().unwrap();
        assert_eq!(&session.project, project, "undo to {index}");
        assert_eq!(&files(root), disk, "undo to {index}");
    }
    assert!(!session.history.is_dirty());
    assert!(session.known.journal().is_clean());
    assert!(!root.join(journal::JOURNAL).exists());
    for (index, (project, disk)) in states.iter().enumerate().skip(1) {
        session.redo().unwrap().unwrap();
        assert_eq!(&session.project, project, "redo to {index}");
        assert_eq!(&files(root), disk, "redo to {index}");
    }
    // Saved, the journal goes and the project opens as it was left.
    assert!(root.join(journal::JOURNAL).exists());
    session.save();
    assert!(!root.join(journal::JOURNAL).exists());
    let (opened, report) = persistence::open_project(root).unwrap();
    assert_eq!(opened, session.project);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
}

#[test]
fn a_batch_finds_a_file_where_an_earlier_command_put_it_and_a_refused_one_touches_nothing() {
    let fixture = Fixture::new("lote");
    let root = &fixture.root;
    let mut session = Session::open(root);
    session
        .run(json!([
            {"op": "create_folder", "path": "Pista"},
            {"op": "move", "items": ["scripts/porta.luau"], "to": "Pista"},
            {"op": "attach_script", "entity": "Caixa", "path": "Pista/porta.luau"},
        ]))
        .unwrap();
    assert_eq!(session.scripts("Caixa"), ["Pista/porta.luau"]);
    assert_eq!(session.scripts("Porta"), ["Pista/porta.luau"]);
    // A batch refused at its last command moves nothing, and leaves no journal behind.
    let disk = files(root);
    let project = session.project.clone();
    let journal = fs::read(root.join(journal::JOURNAL)).unwrap();
    for commands in [
        json!([
            {"op": "move", "items": ["Sons/Pulo.wav"], "to": "Pista"},
            {"op": "rename_asset", "asset": "Bip", "name": "Pedra", "kind": "Audio"},
            {"op": "rename_asset", "asset": "Clique", "name": "pedra"},
        ]),
        json!([{"op": "move", "items": ["Sons/Pulo.wav"], "to": "Nenhuma"}]),
        json!([{"op": "rename_folder", "folder": "Pista", "name": "Sons"}]),
        json!([{"op": "move", "items": ["Pista"], "to": "Pista"}]),
        json!([{"op": "create_folder", "path": "Sons"}]),
        json!([{"op": "create_folder", "path": ".escondida"}]),
        json!([{"op": "rename_asset", "asset": "Pedra", "name": "con"}]),
    ] {
        let error = session.run(commands.clone()).unwrap_err();
        assert!(error.contains("Nada foi alterado"), "{commands}: {error}");
        assert_eq!(files(root), disk, "{commands}");
        assert_eq!(session.project, project, "{commands}");
        assert_eq!(fs::read(root.join(journal::JOURNAL)).unwrap(), journal);
    }
    assert_eq!(session.history.undo_len(), 1);
    // A name taken where an item goes is refused, with a free one suggested.
    put(root, "Pista/Pulo.wav", &wav(30));
    let error = session
        .run(json!([{"op": "move", "items": ["Pulo"], "to": "Pista"}]))
        .unwrap_err();
    assert!(
        error.contains("Já existe Pulo.wav em Pista") && error.contains("Pulo 2.wav está livre"),
        "{error}"
    );
    // The tree's commands wait for an old project to be organized in folders.
    let mut old = session.project.clone();
    old.layout = Layout::Legacy;
    let error = command::apply_on_files(
        &mut old,
        &session.project.start_scene.clone(),
        &command::parse(&json!([{"op": "create_folder", "path": "X"}])).unwrap(),
        root,
        &mut session.known.clone(),
        "Agente",
    )
    .unwrap_err()
    .message;
    assert!(error.contains("formato antigo"), "{error}");
    // Without the project's folder, they say so.
    let error = command::apply(
        &mut session.project.clone(),
        &session.project.start_scene.clone(),
        &command::parse(&json!([{"op": "create_folder", "path": "X"}])).unwrap(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("precisa da pasta"), "{error}");
}

#[test]
fn an_image_whose_file_the_next_save_makes_moves_and_is_renamed_by_its_path() {
    let fixture = Fixture::new("copia");
    let root = &fixture.root;
    let mut session = Session::open(root);
    // A copy of an image, beside the original, has no file until the save.
    let copy = new_id();
    session.project.assets.push(Asset {
        id: copy.clone(),
        name: "Pedra (cópia)".into(),
        path: "Imagens/Pedra (cópia).png".into(),
        kind: AssetKind::Texture,
        model: None,
    });
    session
        .run(json!([
            {"op": "create_folder", "path": "Pista"},
            {"op": "move", "items": ["Pedra (cópia)"], "to": "Pista"},
            {"op": "rename_asset", "asset": "Pedra (cópia)", "name": "Lasca"},
        ]))
        .unwrap();
    let moved = session.project.asset(&copy).unwrap();
    assert_eq!(
        (moved.name.as_str(), moved.path.as_str()),
        ("Lasca", "Pista/Lasca.png")
    );
    assert!(!root.join("Pista/Lasca.png").exists());
    // A sound whose file is really gone stays where it is, said plainly.
    fs::remove_file(root.join("Sons/Bip.wav")).unwrap();
    let error = session
        .run(json!([{"op": "move", "items": ["Bip"], "to": "Pista"}]))
        .unwrap_err();
    assert!(
        error.contains("O arquivo Sons/Bip.wav (Bip, áudio) não está na pasta do projeto"),
        "{error}"
    );
}

#[test]
fn a_step_of_files_is_never_adjusted_live() {
    let fixture = Fixture::new("ajuste");
    let mut session = Session::open(&fixture.root);
    session
        .run(json!([{"op": "create_folder", "path": "Pista"}]))
        .unwrap();
    let id = session.history.last_command_id().unwrap();
    assert!(
        !session
            .history
            .begin_amend_last(id, &session.project, &session.images)
            .unwrap()
    );
}

#[test]
fn undo_refuses_a_file_changed_outside_oxy_and_takes_one_oxy_wrote_itself() {
    let fixture = Fixture::new("carimbo");
    let root = &fixture.root;
    let mut session = Session::open(root);
    session
        .run(json!([{"op": "move", "items": ["Torre", "Pulo"], "to": "Imagens"}]))
        .unwrap();
    // OXY rewrites the model's file when it saves a change to it: that stamp is OXY's own.
    session
        .run(json!([
            {"op": "create_entity", "kind": {"type": "group"}, "name": "Nova torre"},
            {"op": "save_model", "entity": "$0", "replace": "Torre"},
        ]))
        .unwrap();
    let model = root.join("Imagens/Torre.modelo.json");
    let written = fs::read(&model).unwrap();
    session.save();
    assert_ne!(fs::read(&model).unwrap(), written);
    session.undo().unwrap().unwrap();
    // A file changed outside OXY since it moved it: the undo is refused, and nothing changes.
    let sound = root.join("Imagens/Pulo.wav");
    let modified = fs::metadata(&sound).unwrap().modified().unwrap();
    let disk = files(root);
    let project = session.project.clone();
    put(root, "Imagens/Pulo.wav", &wav(50));
    let error = session.undo().unwrap_err();
    assert!(
        error.contains("Imagens/Pulo.wav mudou fora da OXY"),
        "{error}"
    );
    assert_eq!(session.project, project);
    assert_eq!(session.history.undo_len(), 1);
    assert!(session.known.journal().pending.is_none());
    // Back as OXY left it, size and write time: the undo goes through, the model's file it
    // wrote itself included.
    put(root, "Imagens/Pulo.wav", &wav(4));
    fs::File::options()
        .write(true)
        .open(&sound)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(files(root), disk);
    session.undo().unwrap().unwrap();
    assert_eq!(session.asset("Torre").path, "Torre.modelo.json");
    assert_eq!(session.asset("Pulo").path, "Sons/Pulo.wav");
    assert!(root.join("Torre.modelo.json").is_file() && root.join("Sons/Pulo.wav").is_file());
}

#[cfg(windows)]
#[test]
fn a_file_another_program_holds_refuses_the_undo_and_leaves_everything_as_it_was() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new("preso");
    let root = &fixture.root;
    let mut session = Session::open(root);
    session
        .run(json!([
            {"op": "create_folder", "path": "Pista"},
            {"op": "move", "items": ["Pulo", "Bip"], "to": "Pista"},
        ]))
        .unwrap();
    let disk = files(root);
    let project = session.project.clone();
    let journal = session.known.journal().clone();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.join("Pista/Bip.wav"))
        .unwrap();
    let error = session.undo().unwrap_err();
    assert!(
        error.contains("Pista/Bip.wav está aberto em outro programa"),
        "{error}"
    );
    drop(held);
    assert_eq!(files(root), disk);
    assert_eq!(session.project, project);
    assert_eq!(session.known.journal(), &journal);
    assert_eq!(session.history.undo_len(), 1);
    // Let go, the undo goes through.
    session.undo().unwrap().unwrap();
    assert_eq!(session.asset("Pulo").path, "Sons/Pulo.wav");
    assert!(!root.join("Pista").exists());
}

#[test]
fn emptying_the_trash_cuts_the_undo_and_redo_of_the_steps_that_need_it() {
    let fixture = Fixture::new("lixeira");
    let root = &fixture.root;
    let mut session = Session::open(root);
    session
        .run(json!([{"op": "move", "items": ["Pulo"], "to": "Imagens"}]))
        .unwrap();
    session
        .run(json!([{"op": "delete_asset", "asset": "Bip"}]))
        .unwrap();
    session
        .run(json!([{"op": "create_folder", "path": "Pista"}]))
        .unwrap();
    // An import that was undone needs the trash to be redone.
    let mut creating = journal::Creating::new(root, &mut session.known, "Importar recurso");
    session
        .history
        .begin("Importar recurso", &session.project, &session.images);
    let id = new_id();
    creating.before("Sons/Novo.wav", &id).unwrap();
    put(root, "Sons/Novo.wav", &wav(9));
    let made = creating.finish(true);
    session.project.assets.push(Asset {
        id,
        name: "Novo".into(),
        path: "Sons/Novo.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    session
        .history
        .commit_with_files(&session.project, &mut session.images, made)
        .unwrap();
    session.undo().unwrap();
    assert!(!root.join("Sons/Novo.wav").exists());
    // Redone, the import's file comes back from the slot its step fixed.
    session.redo().unwrap().unwrap();
    assert_eq!(fs::read(root.join("Sons/Novo.wav")).unwrap(), wav(9));
    assert_eq!(session.asset("Novo").path, "Sons/Novo.wav");
    session.undo().unwrap();
    assert!(
        session
            .project
            .assets
            .iter()
            .all(|asset| asset.name != "Novo")
    );
    // Undo stops at the delete (it and the move before it go), and the import's redo goes.
    assert_eq!(session.history.trash_steps(), 3);
    let emptied = journal::empty_trash(root, &mut session.known).unwrap();
    assert_eq!(emptied.files, 2);
    assert!(session.known.journal().trash.is_empty());
    assert_eq!(session.history.forget_trash(), 3);
    assert_eq!(
        (session.history.undo_len(), session.history.redo_len()),
        (1, 0)
    );
    session.undo().unwrap().unwrap();
    assert!(!root.join("Pista").exists());
    assert_eq!(session.undo().unwrap(), None);
    // A trash emptied outside OXY: the undo that needs it says so and the steps go.
    session
        .run(json!([{"op": "delete_asset", "asset": "Clique"}]))
        .unwrap();
    session
        .run(json!([{"op": "create_folder", "path": "Pista"}]))
        .unwrap();
    fs::remove_dir_all(root.join(fs_ops::TRASH)).unwrap();
    session.undo().unwrap().unwrap();
    let error = session.undo().unwrap_err();
    assert!(
        error.contains("não está mais na lixeira do projeto, que foi esvaziada")
            && error.contains("1 passo"),
        "{error}"
    );
    assert_eq!(session.history.undo_len(), 0);
    assert!(
        session
            .project
            .assets
            .iter()
            .all(|asset| asset.name != "Clique")
    );
}

#[test]
fn discarding_returns_files_and_folders_to_the_last_save() {
    let fixture = Fixture::new("descartar");
    let root = &fixture.root;
    let saved = files(root);
    let (saved_project, _) = persistence::open_project(root).unwrap();
    let mut session = Session::open(root);
    for commands in [
        json!([{"op": "create_folder", "path": "Pista/Sons"}]),
        json!([{"op": "move", "items": ["Pulo", "scripts/porta.luau", "Torre"],
                "to": "Pista/Sons"}]),
        json!([{"op": "rename_folder", "folder": "Pista", "name": "Corrida"}]),
        json!([{"op": "rename_asset", "asset": "Pedra", "name": "Rocha"}]),
        json!([{"op": "delete_asset", "asset": "Bip"}]),
        json!([{"op": "delete_folder", "folder": "Menu"}]),
    ] {
        session.run(commands).unwrap();
    }
    let mut creating = journal::Creating::new(root, &mut session.known, "Importar recurso");
    creating.before("Imagens/Nova.png", &new_id()).unwrap();
    put(root, "Imagens/Nova.png", &png(10));
    creating.finish(true);
    assert_ne!(files(root), saved);
    let discarded = journal::discard(root, &mut session.known);
    assert!(discarded.kept.is_empty(), "{discarded:?}");
    assert_eq!(files(root), saved);
    assert!(!root.join(journal::JOURNAL).exists());
    let (reopened, report) = persistence::open_project(root).unwrap();
    assert_eq!(reopened, saved_project);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
}

#[test]
fn the_scan_tells_what_changed_outside_oxy_and_not_what_oxy_did() {
    let fixture = Fixture::new("varredura");
    let root = &fixture.root;
    let file = root.join(persistence::PROJECT_FILE);
    let mut session = Session::open(root);
    let mut before = project_tree::walk(root).unwrap();
    // What OXY does itself is no change from outside.
    session
        .run(json!([{"op": "move", "items": ["Bip"], "to": "Imagens"}]))
        .unwrap();
    let (_, changes) = persistence::scan(&file, &before, &session.project, &session.known).unwrap();
    // The project already has the file where OXY put it: no file gone, none new.
    assert!(changes.is_empty(), "{changes:?}");
    before = project_tree::walk(root).unwrap();
    let (_, changes) = persistence::scan(&file, &before, &session.project, &session.known).unwrap();
    assert!(changes.is_empty(), "{changes:?}");
    // Outside OXY: a new image, a sound renamed, the model's file moved, an image in another
    // case and changed, and a script that nobody attaches.
    put(root, "Imagens/Nova.png", &png(90));
    fs_ops::move_no_replace(&root.join("Sons/Pulo.wav"), &root.join("Sons/Pulinho.wav")).unwrap();
    fs::create_dir_all(root.join("Modelos")).unwrap();
    fs_ops::move_no_replace(
        &root.join("Torre.modelo.json"),
        &root.join("Modelos/Torre.modelo.json"),
    )
    .unwrap();
    fs_ops::move_no_replace(
        &root.join("Imagens/Pedra.png"),
        &root.join("Imagens/pedra.png"),
    )
    .unwrap();
    put(root, "Menu/extra.luau", b"return {}\n");
    let (after, changes) =
        persistence::scan(&file, &before, &session.project, &session.known).unwrap();
    let ids: BTreeMap<String, Id> = session
        .project
        .assets
        .iter()
        .map(|asset| (asset.name.clone(), asset.id.clone()))
        .collect();
    let id = |name: &str| ids[name].clone();
    assert_eq!(
        changes,
        [
            project_tree::Change::Case {
                asset: id("Pedra"),
                path: "Imagens/pedra.png".into(),
            },
            project_tree::Change::Missing {
                asset: id("Pulo"),
                near: vec!["Sons/Pulinho.wav".into()],
            },
            project_tree::Change::Found {
                asset: id("Torre"),
                path: "Modelos/Torre.modelo.json".into(),
            },
            project_tree::Change::New {
                path: "Imagens/Nova.png".into(),
                kind: project_tree::FileKind::Image,
            },
            // A file renamed outside OXY is a new one too; only the person relinks it.
            project_tree::Change::New {
                path: "Sons/Pulinho.wav".into(),
                kind: project_tree::FileKind::Sound,
            },
        ]
    );
    let adopted = persistence::adopt(&mut session.project, root, &changes, &mut session.known);
    assert_eq!(adopted.fixes.len(), 4, "{:?}", adopted.fixes);
    assert_eq!(adopted.warnings.len(), 1, "{:?}", adopted.warnings);
    assert!(
        adopted.warnings[0].contains(
            "Sons/Pulo.wav não está mais na pasta do projeto (talvez \
                                      seja Sons/Pulinho.wav)"
        ),
        "{:?}",
        adopted.warnings
    );
    assert_eq!(session.asset("Pedra").path, "Imagens/pedra.png");
    assert_eq!(session.asset("Torre").path, "Modelos/Torre.modelo.json");
    assert_eq!(session.asset("Nova").path, "Imagens/Nova.png");
    // An image written again outside OXY is read again.
    let before = after;
    put(root, "Imagens/pedra.png", &png(30));
    let (after, changes) =
        persistence::scan(&file, &before, &session.project, &session.known).unwrap();
    assert_eq!(
        changes,
        [project_tree::Change::Changed { asset: id("Pedra") }]
    );
    // A model whose entry left the project after the save (its save_model undone) is on its
    // way to the trash: the scan does not take it again, and the next save sends it there.
    session
        .run(json!([{"op": "save_model", "entity": "Caixa", "name": "Arco"}]))
        .unwrap();
    session.save();
    assert!(root.join("Arco.modelo.json").is_file());
    session.undo().unwrap();
    let (_, changes) = persistence::scan(&file, &after, &session.project, &session.known).unwrap();
    assert!(
        changes
            .iter()
            .all(|change| !matches!(change, project_tree::Change::New { .. })),
        "{changes:?}"
    );
    let told = session.save();
    assert!(
        told.iter()
            .any(|line| line.contains("Arco.modelo.json não é mais do projeto")),
        "{told:?}"
    );
    assert!(!root.join("Arco.modelo.json").exists());
}

#[test]
fn a_command_without_the_projects_folder_keeps_the_old_rules() {
    // Renaming in the tree layout without the folder is refused; deleting leaves the file for
    // the next save, as before.
    let fixture = Fixture::new("sem-pasta");
    let (mut project, _) = persistence::open_project(&fixture.root).unwrap();
    let scene = project.start_scene.clone();
    let error = command::apply(
        &mut project,
        &scene,
        &[Command::RenameAsset {
            asset: "Bip".into(),
            name: "Bipe".into(),
            kind: None,
        }],
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("só rename_asset renomeia, na pasta do projeto"),
        "{error}"
    );
    command::apply(
        &mut project,
        &scene,
        &[Command::DeleteAsset {
            asset: "Bip".into(),
            kind: None,
            detach: None,
        }],
    )
    .unwrap();
    assert!(fixture.root.join("Sons/Bip.wav").exists());
}

#[test]
fn the_trash_says_what_it_holds_and_each_step_what_it_did_to_the_files() {
    let fixture = Fixture::new("conta");
    let root = &fixture.root;
    let mut session = Session::open(root);
    assert_eq!(
        fs_ops::trash_contents(root).unwrap(),
        fs_ops::Emptied::default()
    );
    session
        .run(json!([{"op": "move", "items": ["Bip"], "to": "Imagens"}]))
        .unwrap();
    assert_eq!(
        session.history.next_files(false),
        [oxy_core::edit_history::FileOp::Move {
            from: "Sons/Bip.wav".into(),
            to: "Imagens/Bip.wav".into()
        }]
    );
    assert!(session.history.next_files(true).is_empty());
    session
        .run(json!([{"op": "delete_asset", "asset": "Bip"}]))
        .unwrap();
    session
        .run(json!([{"op": "delete_asset", "asset": "Clique"}]))
        .unwrap();
    let held = fs_ops::trash_contents(root).unwrap();
    assert_eq!(held.files, 2);
    assert_eq!(held.bytes, (wav(6).len() + wav(8).len()) as u64);
    // Undone, the delete is the next redo, and the trash holds one file less.
    session.undo().unwrap().unwrap();
    assert!(matches!(
        session.history.next_files(true),
        [oxy_core::edit_history::FileOp::Trash { from, .. }] if from == "Menu/Clique.wav"
    ));
    assert_eq!(fs_ops::trash_contents(root).unwrap().files, 1);
    let emptied = journal::empty_trash(root, &mut session.known).unwrap();
    assert_eq!(
        emptied,
        fs_ops::Emptied {
            files: 1,
            bytes: wav(6).len() as u64
        }
    );
    assert_eq!(
        fs_ops::trash_contents(root).unwrap(),
        fs_ops::Emptied::default()
    );
}
