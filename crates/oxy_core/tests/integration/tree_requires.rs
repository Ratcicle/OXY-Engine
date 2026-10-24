//! Moving scripts in OXY keeps their `require` finding their modules: a module moved is found by
//! every script that requires it, relative or by the project's alias; a folder of scripts
//! renamed changes only what points into it from outside; a script moved finds its modules from
//! its new folder; deleting a folder moves its scripts up to the root with their `require`
//! following. Each rewrite is part of the command's undo step, refused when the script changed
//! outside OXY, brought back by "Descartar alterações", and what no rewrite can follow (a path
//! the script builds) is told with its file and line.
use oxy_core::{
    command,
    document::*,
    edit_history::CommandHistory,
    journal,
    persistence::{self, KnownFiles},
    script::{self, ScriptRef},
    texture_cache::TextureCache,
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn put(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// Every file under `root` but OXY's own folder, by its path with `/` (folders end in `/`),
/// with its bytes.
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

const DADOS: &str = "return { velocidade = 3 }\n";
const SONS: &str = "return { pulo = 'Pulo' }\n";
/// Requires the module of its own folder, with a comment and a text that name it too.
const CHECKPOINT: &str = "\
-- require('./dados') num comentário não é código
local dados = require(\"./dados\")
local texto = \"require('./dados')\"
local Checkpoint = {}
function Checkpoint.on_start(self)
    local _ = dados.velocidade
end
return Checkpoint
";
const MENU: &str = "local dados = require '../Pista/dados' -- velocidade\nreturn {}\n";
const JOGO: &str = "local dados = require(\"@project/Pista/dados\")\nreturn {}\n";
const LARGADA: &str = "\
local sons = require('../Comum/sons')
local mesmos = require('@project/Comum/sons')
local dados = require([[./dados]])
return {}
";
/// Requires with a path it builds.
const FASES: &str = "\
local Fases = {}
function Fases.carregar(nome)
    return require(\"@project/Pista/\" .. nome)
end
return Fases
";

/// A project in folders saved in a folder of its own: the module `Pista/dados.luau`, which
/// `Pista/checkpoint.luau` (attached to the object Checkpoint), `Menu/menu.luau`, `jogo.luau`
/// and `Pista/largada.luau` (attached to the piece of the model Torre) require, each its own
/// way; `Comum/sons.luau`, which `Pista/largada.luau` requires; and `Menu/fases.luau`, which
/// builds the path it requires.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!("oxy-require-{label}-{}", new_id()));
        for (path, text) in [
            ("Pista/dados.luau", DADOS),
            ("Pista/checkpoint.luau", CHECKPOINT),
            ("Pista/largada.luau", LARGADA),
            ("Comum/sons.luau", SONS),
            ("Menu/menu.luau", MENU),
            ("Menu/fases.luau", FASES),
            ("jogo.luau", JOGO),
        ] {
            put(&root, path, text);
        }
        let mut project = Project::new("Require");
        project.layout = Layout::Tree;
        let scene = project.start_scene.clone();
        let mut checkpoint = Entity::new("Checkpoint", Some(Primitive::Cube));
        checkpoint
            .scripts
            .push(ScriptRef::new("Pista/checkpoint.luau"));
        let group = Entity::new("Torre", None);
        let mut block = Entity::new("Bloco", Some(Primitive::Cube));
        block.parent = Some(group.id.clone());
        block.scripts.push(ScriptRef::new("Pista/largada.luau"));
        let group_id = group.id.clone();
        project.scenes[0]
            .entities
            .extend([checkpoint, group, block]);
        project.save_model(&scene, &group_id, "Torre").unwrap();
        persistence::save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
        Self { root }
    }

    fn text(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).unwrap()
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
        Self {
            root: root.to_owned(),
            project,
            images: TextureCache::default(),
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

    /// The scripts the object `name` attaches, in a scene or (`model`) in the model Torre.
    fn scripts(&self, name: &str, model: bool) -> Vec<String> {
        let entities: Vec<&Entity> = if model {
            self.project
                .assets
                .iter()
                .filter_map(|asset| asset.model.as_ref())
                .flatten()
                .collect()
        } else {
            self.project
                .scenes
                .iter()
                .flat_map(|scene| &scene.entities)
                .collect()
        };
        entities
            .into_iter()
            .find(|entity| entity.name == name)
            .unwrap_or_else(|| panic!("{name}"))
            .scripts
            .iter()
            .map(|script| script.path.clone())
            .collect()
    }
}

/// Why each script of the project at `root` does not load as the runtime loads it: every one
/// should, with its `require` finding their modules.
fn load_errors(root: &Path) -> Vec<String> {
    let sources = persistence::load_scripts(root).unwrap();
    assert!(sources.unreadable.is_empty(), "{:?}", sources.unreadable);
    script::check_loading(&sources, sources.files.keys().map(String::as_str), |_| true)
        .into_iter()
        .filter_map(|report| report.error.map(|error| error.to_string()))
        .collect()
}

/// The warnings of a batch's answer, as one text.
fn warnings(answer: &Json) -> String {
    answer["warnings"].to_string()
}

/// Whether a warning of a batch's answer, after the command that told it, is `line`.
fn tells(answer: &Json, line: &str) -> bool {
    answer["warnings"].as_array().is_some_and(|all| {
        all.iter().filter_map(Json::as_str).any(|warning| {
            warning
                .split_once("): ")
                .is_some_and(|(_, told)| told == line)
        })
    })
}

#[test]
fn a_module_moved_is_found_again_by_the_three_scripts_that_require_it_and_undone_with_it() {
    let fixture = Fixture::new("modulo");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let before = files(root);
    let answer = session
        .run(json!([{"op": "move", "items": ["Pista/dados.luau"], "to": "Comum"}]))
        .unwrap();
    // Each way of naming it, relative from its folder and from another, and by the alias, in
    // the quotes it had; the comment and the text that name it stay.
    let checkpoint = CHECKPOINT.replace(
        "local dados = require(\"./dados\")",
        "local dados = require(\"@project/Comum/dados\")",
    );
    assert_eq!(fixture.text("Pista/checkpoint.luau"), checkpoint);
    assert_eq!(
        fixture.text("Menu/menu.luau"),
        "local dados = require '@project/Comum/dados' -- velocidade\nreturn {}\n"
    );
    assert_eq!(
        fixture.text("jogo.luau"),
        "local dados = require(\"@project/Comum/dados\")\nreturn {}\n"
    );
    assert_eq!(
        fixture.text("Pista/largada.luau"),
        LARGADA.replace("[[./dados]]", "\"@project/Comum/dados\"")
    );
    assert_eq!(fixture.text("Comum/sons.luau"), SONS);
    assert_eq!(
        answer["rewritten"],
        json!([
            "Menu/menu.luau",
            "Pista/checkpoint.luau",
            "Pista/largada.luau",
            "jogo.luau"
        ])
    );
    assert_eq!(
        answer["moved"],
        json!([{"from": "Pista/dados.luau", "to": "Comum/dados.luau"}])
    );
    // The path it builds is told, with its file and line.
    let told = warnings(&answer);
    assert!(
        told.contains("Menu/fases.luau:3: require com caminho montado não é atualizado"),
        "{told}"
    );
    assert!(load_errors(root).is_empty(), "{:?}", load_errors(root));
    // One step, the texts included: undone, every file is as it was; redone, as it was left.
    let after = files(root);
    assert_eq!(session.history.undo_len(), 1);
    session.undo().unwrap().unwrap();
    assert_eq!(files(root), before);
    assert!(session.known.journal().is_clean());
    assert!(!root.join(journal::JOURNAL).exists());
    session.redo().unwrap().unwrap();
    assert_eq!(files(root), after);
    // Saved, the project opens as it was left, with nothing to tell.
    persistence::save_project_with(root, &session.project, &mut session.known).unwrap();
    assert!(!root.join(journal::JOURNAL).exists());
    let (opened, report) = persistence::open_project(root).unwrap();
    assert_eq!(opened, session.project);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
}

#[test]
fn a_folder_of_scripts_renamed_changes_only_what_points_into_it_from_outside() {
    let fixture = Fixture::new("pasta");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let answer = session
        .run(json!([{"op": "rename_folder", "folder": "Pista", "name": "Corrida"}]))
        .unwrap();
    // What moved with the module still finds it, and `../Comum` is as deep as before.
    assert_eq!(fixture.text("Corrida/checkpoint.luau"), CHECKPOINT);
    assert_eq!(fixture.text("Corrida/largada.luau"), LARGADA);
    assert_eq!(
        fixture.text("Menu/menu.luau"),
        "local dados = require '@project/Corrida/dados' -- velocidade\nreturn {}\n"
    );
    assert_eq!(
        fixture.text("jogo.luau"),
        "local dados = require(\"@project/Corrida/dados\")\nreturn {}\n"
    );
    assert_eq!(answer["rewritten"], json!(["Menu/menu.luau", "jogo.luau"]));
    // The objects follow, in the scene and in the model's file.
    assert_eq!(
        session.scripts("Checkpoint", false),
        ["Corrida/checkpoint.luau"]
    );
    assert_eq!(session.scripts("Bloco", true), ["Corrida/largada.luau"]);
    assert!(load_errors(root).is_empty(), "{:?}", load_errors(root));
    persistence::save_project_with(root, &session.project, &mut session.known).unwrap();
    let model = fs::read_to_string(root.join("Torre.modelo.json")).unwrap();
    assert!(model.contains("Corrida/largada.luau"), "{model}");
}

#[test]
fn a_script_moved_finds_its_modules_from_its_new_folder() {
    let fixture = Fixture::new("script");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let answer = session
        .run(json!([{"op": "move", "items": ["Pista/largada.luau"], "to": ""}]))
        .unwrap();
    // Up at the root, `../` would leave the project and `./dados` names another folder; the
    // alias still finds its module.
    assert_eq!(
        fixture.text("largada.luau"),
        "\
local sons = require('@project/Comum/sons')
local mesmos = require('@project/Comum/sons')
local dados = require(\"@project/Pista/dados\")
return {}
"
    );
    assert_eq!(answer["rewritten"], json!(["largada.luau"]));
    for (path, text) in [
        ("Pista/checkpoint.luau", CHECKPOINT),
        ("Menu/menu.luau", MENU),
        ("jogo.luau", JOGO),
    ] {
        assert_eq!(fixture.text(path), text, "{path}");
    }
    assert_eq!(session.scripts("Bloco", true), ["largada.luau"]);
    assert!(load_errors(root).is_empty(), "{:?}", load_errors(root));
    // Renamed in its folder, nothing it requires changes, and nothing requires it.
    let answer = session
        .run(
            json!([{"op": "rename_asset", "asset": "largada.luau", "name": "partida",
                     "kind": "Script"}]),
        )
        .unwrap();
    assert!(answer.get("rewritten").is_none(), "{answer}");
    assert_eq!(session.scripts("Bloco", true), ["partida.luau"]);
}

#[test]
fn deleting_a_folder_moves_its_scripts_up_with_their_requires_following() {
    let fixture = Fixture::new("excluir");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let before = files(root);
    let answer = session
        .run(json!([{"op": "delete_folder", "folder": "Menu"}]))
        .unwrap();
    assert_eq!(
        fixture.text("menu.luau"),
        "local dados = require '@project/Pista/dados' -- velocidade\nreturn {}\n"
    );
    assert_eq!(fixture.text("fases.luau"), FASES);
    assert_eq!(answer["rewritten"], json!(["menu.luau"]));
    let told = warnings(&answer);
    assert!(
        told.contains("fases.luau:3: require com caminho montado"),
        "{told}"
    );
    assert!(!root.join("Menu").exists());
    assert!(load_errors(root).is_empty(), "{:?}", load_errors(root));
    session.undo().unwrap().unwrap();
    assert_eq!(files(root), before);
}

#[test]
fn a_batch_finds_the_texts_an_earlier_command_rewrote() {
    let fixture = Fixture::new("lote");
    let root = &fixture.root;
    let mut session = Session::open(root);
    // Attaching runs the script's top, which requires the module where the batch put it.
    session
        .run(json!([
            {"op": "move", "items": ["Pista/dados.luau"], "to": "Comum"},
            {"op": "attach_script", "entity": "Checkpoint", "path": "Menu/menu.luau"},
            {"op": "rename_folder", "folder": "Comum", "name": "Base"},
        ]))
        .unwrap();
    assert_eq!(
        fixture.text("Menu/menu.luau"),
        "local dados = require '@project/Base/dados' -- velocidade\nreturn {}\n"
    );
    assert_eq!(
        fixture.text("Pista/largada.luau"),
        "\
local sons = require('@project/Base/sons')
local mesmos = require('@project/Base/sons')
local dados = require(\"@project/Base/dados\")
return {}
"
    );
    assert!(load_errors(root).is_empty(), "{:?}", load_errors(root));
    // Undone as one step, though two commands wrote the same files.
    session.undo().unwrap().unwrap();
    assert_eq!(fixture.text("Menu/menu.luau"), MENU);
    assert_eq!(fixture.text("Pista/largada.luau"), LARGADA);
    assert!(root.join("Pista/dados.luau").is_file() && root.join("Comum/sons.luau").is_file());
}

#[test]
fn the_answer_names_each_rewritten_script_where_the_batch_left_it() {
    let fixture = Fixture::new("caminhos");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let before = files(root);
    let answer = session
        .run(json!([
            {"op": "move", "items": ["Pista/dados.luau"], "to": "Comum"},
            {"op": "rename_folder", "folder": "Menu", "name": "Telas"},
            {"op": "delete_asset", "asset": "jogo.luau", "kind": "Script"},
        ]))
        .unwrap();
    // Renamed after it was rewritten, at its new path; sent to the trash, left out.
    assert_eq!(
        answer["rewritten"],
        json!([
            "Telas/menu.luau",
            "Pista/checkpoint.luau",
            "Pista/largada.luau"
        ])
    );
    assert_eq!(
        fixture.text("Telas/menu.luau"),
        "local dados = require '@project/Comum/dados' -- velocidade\nreturn {}\n"
    );
    // The path a script builds is told again where the batch left it.
    let built = "require com caminho montado não é atualizado quando a OXY move arquivos; \
                 confira se ele ainda acha o módulo";
    assert!(
        tells(&answer, &format!("Telas/fases.luau:3: {built}")),
        "{}",
        warnings(&answer)
    );
    session.undo().unwrap().unwrap();
    assert_eq!(files(root), before);
}

#[test]
fn deleting_a_module_tells_each_script_that_required_it_and_rewrites_none() {
    let fixture = Fixture::new("lixeira");
    let root = &fixture.root;
    let mut session = Session::open(root);
    let before = files(root);
    let answer = session
        .run(json!([{"op": "delete_asset", "asset": "Pista/dados.luau", "kind": "Script"}]))
        .unwrap();
    assert_eq!(answer["trashed"], json!(["Pista/dados.luau"]));
    assert!(answer.get("rewritten").is_none(), "{answer}");
    for (path, line, require) in [
        ("Menu/menu.luau", 1, "../Pista/dados"),
        ("Pista/checkpoint.luau", 2, "./dados"),
        ("Pista/largada.luau", 3, "./dados"),
        ("jogo.luau", 1, "@project/Pista/dados"),
    ] {
        let told = format!(
            "{path}:{line}: require(\"{require}\") pede Pista/dados.luau, que foi para a lixeira \
             do projeto"
        );
        assert!(tells(&answer, &told), "{told}: {}", warnings(&answer));
        assert_eq!(fixture.text(path).as_bytes(), &before[path][..], "{path}");
    }
    assert!(!root.join("Pista/dados.luau").exists());
    session.undo().unwrap().unwrap();
    assert_eq!(files(root), before);
}

/// Called through another name, passed on, or calls of a `require` the script declares: none can
/// be followed, each is told with its file and line, and the script stays as it is; and so is a
/// script that cannot be read.
#[test]
fn requires_no_rewrite_can_follow_are_told_and_their_scripts_left_as_they_are() {
    const SAFE: &str = "local ok, dados = pcall(require, \"./dados\")\nreturn ok and dados\n";
    const OWN: &str = "\
local carregar = require
local function usar(require)
    return require('./dados')
end
return { usar = usar, carregar = carregar }
";
    let fixture = Fixture::new("valor");
    let root = &fixture.root;
    put(root, "Pista/seguro.luau", SAFE);
    put(root, "Pista/proprio.luau", OWN);
    let mut session = Session::open(root);
    fs::write(root.join("Menu/quebrado.luau"), [0xff, 0xfe, b'x']).unwrap();
    let answer = session
        .run(json!([{"op": "move", "items": ["Pista/dados.luau"], "to": "Comum"}]))
        .unwrap();
    let passed = "require usado como valor (como em pcall(require, …) ou local r = require) não é \
                  atualizado quando a OXY move arquivos; confira se ele ainda acha o módulo";
    for told in [
        format!("Pista/seguro.luau:1: {passed}"),
        format!("Pista/proprio.luau:1: {passed}"),
        "Pista/proprio.luau:3: require(\"./dados\") não foi atualizado, porque o script também \
         declara um require seu (nome local, função ou parâmetro) e esta chamada pode não ser a \
         da OXY; se for, escreva require(\"@project/Comum/dados\")"
            .to_owned(),
        "Menu/quebrado.luau não é texto UTF-8; a OXY não conferiu os require dele".to_owned(),
    ] {
        assert!(tells(&answer, &told), "{told}: {}", warnings(&answer));
    }
    assert_eq!(fixture.text("Pista/seguro.luau"), SAFE);
    assert_eq!(fixture.text("Pista/proprio.luau"), OWN);
    let rewritten = answer["rewritten"].to_string();
    assert!(
        !rewritten.contains("seguro") && !rewritten.contains("proprio"),
        "{rewritten}"
    );
    // The scripts that call require itself are rewritten all the same.
    assert!(rewritten.contains("Menu/menu.luau"), "{rewritten}");
}

#[test]
fn undo_refuses_a_rewritten_script_changed_outside_oxy_and_discarding_brings_the_texts_back() {
    let fixture = Fixture::new("desfazer");
    let root = &fixture.root;
    let saved = files(root);
    let mut session = Session::open(root);
    session
        .run(json!([{"op": "move", "items": ["Pista/dados.luau"], "to": "Comum"}]))
        .unwrap();
    let rewritten = fixture.text("Menu/menu.luau");
    let project = session.project.clone();
    put(root, "Menu/menu.luau", &format!("-- meu\n{rewritten}"));
    let error = session.undo().unwrap_err();
    assert!(
        error.contains("Menu/menu.luau mudou fora da OXY"),
        "{error}"
    );
    assert_eq!(session.project, project);
    assert!(root.join("Comum/dados.luau").is_file());
    assert_eq!(session.history.undo_len(), 1);
    // As OXY left it again, the undo goes through.
    put(root, "Menu/menu.luau", &rewritten);
    session.undo().unwrap().unwrap();
    assert_eq!(files(root), saved);
    // Moved and renamed again, then discarded: the texts as they were saved.
    session.redo().unwrap().unwrap();
    session
        .run(json!([{"op": "rename_folder", "folder": "Comum", "name": "Base"}]))
        .unwrap();
    assert_ne!(files(root), saved);
    let discarded = journal::discard(root, &mut session.known);
    assert!(discarded.kept.is_empty(), "{discarded:?}");
    assert_eq!(files(root), saved);
    assert!(!root.join(journal::JOURNAL).exists());
}

/// luau-lsp (the extension VS Code uses) still finds every module after OXY rewrote the
/// `require`: each script that requires one reads a key it lacks, which luau-lsp tells only when
/// it found the module. Optional, as the other checks with luau-lsp (`scripts_files`): only
/// where `OXY_LUAU_LSP` names a luau-lsp 1.70 executable.
#[test]
fn luau_lsp_still_finds_the_modules_after_the_rewrite() {
    let Some(lsp) = std::env::var_os("OXY_LUAU_LSP") else {
        return;
    };
    let fixture = Fixture::new("luau-lsp");
    let root = &fixture.root;
    let probes = [
        ("Pista/sonda.luau", "require('./dados')"),
        ("Menu/sonda.luau", "require('../Pista/dados')"),
        ("sonda.luau", "require('@project/Pista/dados')"),
    ];
    for (path, require) in probes {
        put(
            root,
            path,
            &format!("local dados = {require}\nlocal _ = dados.naoexiste\nreturn {{}}\n"),
        );
    }
    let mut session = Session::open(root);
    session
        .run(json!([
            {"op": "move", "items": ["Pista/dados.luau"], "to": "Comum"},
            {"op": "rename_folder", "folder": "Menu", "name": "Telas"},
        ]))
        .unwrap();
    oxy_core::script_files::setup_vscode(root).unwrap();
    let probes = ["Pista/sonda.luau", "Telas/sonda.luau", "sonda.luau"];
    let output = std::process::Command::new(lsp)
        .current_dir(root)
        .args([
            "analyze",
            "--no-flags-enabled",
            "--platform=standard",
            "--formatter=gnu",
        ])
        .args(probes)
        .output()
        .unwrap();
    let told = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!told.contains("Unknown require"), "{told}");
    assert_eq!(told.matches("naoexiste").count(), probes.len(), "{told}");
}
