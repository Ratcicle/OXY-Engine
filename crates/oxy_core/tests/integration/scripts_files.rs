//! The project's script files as the tools reach them (oxy_scripts): read with line numbers,
//! checked as loading checks them, written all-or-nothing with the agent's copy of a changed
//! file, and the files VS Code with luau-lsp reads; with them, what a reload in the editor's
//! test warns about and the scripts an object attaches that the folder lacks. When
//! `OXY_LUAU_LSP` names a luau-lsp 1.70.1 executable, its check of the generated types, of the
//! API's examples, of objects typed by their scripts and of the plan-B module runs too, in both
//! of Luau's solvers (design §5.7).
use oxy_core::{
    document::*,
    persistence::{self, PersistenceError},
    query,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{self, ScriptRef, ScriptSources, registry},
    script_files::{self, Request},
};
use serde_json::{Value as Json, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A project folder of its own, with the scripts `files` in it.
fn folder(files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("oxy-scripts-files-{}", new_id()));
    for (path, text) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// The files under `folder`, by their path from it with `/`.
fn files_under(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![(folder.to_owned(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().unwrap().is_dir() {
                pending.push((entry.path(), format!("{name}/")));
            } else {
                found.push(name);
            }
        }
    }
    found.sort();
    found
}

fn call(
    root: &Path,
    kept: &mut HashSet<PathBuf>,
    args: Json,
) -> Result<(Json, Vec<String>), String> {
    let request = Request::parse(&args)?;
    script_files::answer(root, None, &request, kept).map(|answer| (answer.json, answer.console))
}

const DOOR: &str = "local M = {}\nfunction M.on_start(self)\n    print('aberta')\nend\nreturn M\n";

#[test]
fn scripts_are_read_with_line_numbers_and_checked_as_loading_checks_them() {
    let root = folder(&[
        ("scripts/porta.luau", DOOR),
        (
            "scripts/fases/chave.luau",
            "local M = {}\nfunction M.on_aera_enter(self) end\nreturn M\n",
        ),
        ("scripts/quebrado.luau", "local M = {\nreturn M\n"),
        ("scripts/notas.txt", "não é script"),
    ]);
    let mut kept = HashSet::new();
    let (read, console) = call(
        &root,
        &mut kept,
        json!({"action": "read", "path": r"scripts\porta.luau"}),
    )
    .unwrap();
    assert!(console.is_empty());
    assert_eq!(
        read,
        json!({"path": "scripts/porta.luau", "lines": 5,
               "text": "1\tlocal M = {}\n2\tfunction M.on_start(self)\n3\t    print('aberta')\n4\tend\n5\treturn M"})
    );
    // A file the folder lacks is named with those it has.
    let error = call(
        &root,
        &mut kept,
        json!({"action": "read", "path": "scripts/janela.luau"}),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "O script scripts/janela.luau não existe na pasta do projeto; scripts: \
         scripts/fases/chave.luau, scripts/porta.luau, scripts/quebrado.luau"
    );
    // Every script of the folder, with each error and warning at its line.
    let (all, _) = call(&root, &mut kept, json!({"action": "check"})).unwrap();
    assert_eq!(all["ok"], false);
    assert_eq!(
        all["checked"],
        json!([
            "scripts/fases/chave.luau",
            "scripts/porta.luau",
            "scripts/quebrado.luau"
        ])
    );
    let errors = all["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{all}");
    assert!(
        errors[0]
            .as_str()
            .unwrap()
            .starts_with("scripts/quebrado.luau:2: erro de sintaxe"),
        "{all}"
    );
    assert_eq!(
        all["warnings"],
        json!([
            "scripts/fases/chave.luau:2: on_aera_enter não é um evento dos scripts e nunca \
                roda sozinha; quis dizer on_area_enter?"
        ])
    );
    assert_eq!(all["checker"]["name"], "oxy");
    assert_eq!(all["types_unavailable"], true);
    // One script, or a text as that script, without saving it.
    let (one, _) = call(
        &root,
        &mut kept,
        json!({"action": "check", "path": "scripts/porta.luau"}),
    )
    .unwrap();
    assert_eq!(one["ok"], true);
    assert!(one.get("errors").is_none() && one.get("warnings").is_none());
    let (text, _) = call(
        &root,
        &mut kept,
        json!({"action": "check", "path": "scripts/porta.luau",
               "text": "local n = 0\nlocal M = {}\nfunction M.on_step(self) n += 1 end\nreturn M\n"}),
    )
    .unwrap();
    assert!(
        text["errors"][0]
            .as_str()
            .unwrap()
            .starts_with("scripts/porta.luau:3: `n` é um local do topo do script"),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("scripts/porta.luau")).unwrap(),
        DOOR
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn writes_are_all_or_nothing_and_keep_the_file_as_the_session_found_it() {
    let root = folder(&[("scripts/porta.luau", DOOR)]);
    let mut kept = HashSet::new();
    let write = |kept: &mut HashSet<PathBuf>, path: &str, text: &str| {
        call(
            &root,
            kept,
            json!({"action": "write", "path": path, "text": text}),
        )
    };
    // A text that does not load is refused with its line, and nothing is written.
    let error = write(&mut kept, "scripts/porta.luau", "local M = {\nreturn M\n").unwrap_err();
    assert!(
        error.starts_with("scripts/porta.luau:2: erro de sintaxe")
            && error.ends_with("nada foi gravado"),
        "{error}"
    );
    let error = write(
        &mut kept,
        "scripts/porta.luau",
        "audio.play('Bipe')\nreturn {}\n",
    )
    .unwrap_err();
    assert!(
        error.contains("o topo do script não pode mexer no mundo"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("scripts/porta.luau")).unwrap(),
        DOOR
    );
    assert!(!root.join(".oxy").exists());
    // The first change keeps the file as it was, out of the scripts folder; the answer and the
    // Console say what changed.
    let second = DOOR.replace("aberta", "abriu");
    let (answer, console) = write(&mut kept, "scripts/porta.luau", &second).unwrap();
    let agent = root.join(".oxy").join("agente").join("scripts");
    let backup = agent.join("porta.luau.bak");
    assert_eq!(answer["created"], false);
    assert_eq!(
        (answer["added"].as_u64(), answer["removed"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(answer["lines"], 5);
    assert_eq!(
        answer["previous_version"],
        json!(backup.display().to_string())
    );
    assert_eq!(
        answer["saved"],
        json!(
            root.join("scripts")
                .join("porta.luau")
                .display()
                .to_string()
        )
    );
    assert_eq!(
        console[0],
        "Script scripts/porta.luau alterado (+1 -1 linhas)"
    );
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), DOOR);
    // Later writes of the session keep that copy.
    let third = format!("{second}-- fim\n");
    let (answer, console) = write(&mut kept, "scripts/porta.luau", &third).unwrap();
    assert_eq!(
        console[0],
        "Script scripts/porta.luau alterado (+1 -0 linhas)"
    );
    assert_eq!(
        answer["previous_version"],
        json!(backup.display().to_string())
    );
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), DOOR);
    let (_, console) = write(&mut kept, "scripts/porta.luau", &third).unwrap();
    assert_eq!(console[0], "Script scripts/porta.luau gravado sem mudanças");
    // Deleted, it is kept again from the file as it is then; another session keeps its own.
    std::fs::remove_file(&backup).unwrap();
    write(&mut kept, "scripts/porta.luau", DOOR).unwrap();
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), third);
    write(&mut HashSet::new(), "scripts/porta.luau", &second).unwrap();
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), DOOR);
    // A new file in a new subfolder, which requires another: no copy for a file the session
    // created, not even on its next write.
    let (answer, console) = write(
        &mut kept,
        "scripts/fases/chave.luau",
        "local Porta = require('../porta')\nlocal M = {}\nM.attributes = { Cor = 'azul' }\nreturn M\n",
    )
    .unwrap();
    assert_eq!(answer["created"], true);
    assert!(answer.get("previous_version").is_none());
    assert_eq!(
        console[0],
        "Script scripts/fases/chave.luau criado (4 linhas)"
    );
    write(&mut kept, "scripts/fases/chave.luau", "return {}\n").unwrap();
    assert!(!agent.join("fases").exists());
    // The scripts folder holds the scripts and nothing else, and loading reads only them.
    assert_eq!(
        files_under(&root.join("scripts")),
        ["fases/chave.luau", "porta.luau"]
    );
    assert_eq!(files_under(&agent), ["porta.luau.bak"]);
    std::fs::write(root.join("scripts/notas.txt"), "não é script").unwrap();
    std::fs::write(root.join("scripts/porta.luau.agente.bak"), "return {").unwrap();
    let sources = persistence::load_scripts(&root).unwrap();
    assert_eq!(
        sources.files.keys().collect::<Vec<_>>(),
        ["scripts/fases/chave.luau", "scripts/porta.luau"]
    );
    let (all, _) = call(&root, &mut kept, json!({"action": "check"})).unwrap();
    assert_eq!(
        all["checked"],
        json!(["scripts/fases/chave.luau", "scripts/porta.luau"])
    );
    std::fs::remove_file(root.join("scripts/notas.txt")).unwrap();
    std::fs::remove_file(root.join("scripts/porta.luau.agente.bak")).unwrap();
    // Paths out of the folder and texts over 1 MB are refused.
    assert!(
        Request::parse(&json!({"action": "write", "path": "../fora.luau", "text": ""}))
            .unwrap_err()
            .starts_with("caminho de script inválido")
    );
    assert!(matches!(
        persistence::write_script(&root, "scripts/../fora.luau", "", false),
        Err(PersistenceError::InvalidAssetPath(_))
    ));
    let huge = "-".repeat(1024 * 1024 + 1);
    assert_eq!(
        write(&mut kept, "scripts/grande.luau", &huge).unwrap_err(),
        "O script scripts/grande.luau tem mais de 1 MB"
    );
    // A file saved in Latin-1 is a problem of its own: the others are read and checked, it is
    // an error once something uses it (here, nothing does) and a text replaces it.
    std::fs::write(root.join("scripts/latin.luau"), b"-- n\xe3o\nreturn {}\n").unwrap();
    let (told, _) = call(&root, &mut kept, json!({"action": "check"})).unwrap();
    assert_eq!(told["ok"], true);
    assert_eq!(
        told["checked"],
        json!(["scripts/fases/chave.luau", "scripts/porta.luau"])
    );
    assert_eq!(
        told["warnings"],
        json!(["O script scripts/latin.luau não é texto UTF-8"])
    );
    std::fs::write(
        root.join("scripts/porta.luau"),
        "local Texto = require('./latin')\nreturn {}\n",
    )
    .unwrap();
    let (refused, _) = call(&root, &mut kept, json!({"action": "check"})).unwrap();
    assert_eq!(refused["ok"], false);
    assert!(
        refused["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error
                .as_str()
                .unwrap()
                .contains("scripts/latin.luau não é texto UTF-8")),
        "{refused}"
    );
    assert_eq!(
        call(
            &root,
            &mut kept,
            json!({"action": "read", "path": "scripts/latin.luau"})
        )
        .unwrap_err(),
        "O script scripts/latin.luau não é texto UTF-8"
    );
    let (checked, _) = call(
        &root,
        &mut kept,
        json!({"action": "check", "path": "scripts/latin.luau", "text": "return {}\n"}),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked}");
    let (answer, _) = write(&mut kept, "scripts/latin.luau", "-- não\nreturn {}\n").unwrap();
    assert_eq!(answer["created"], false);
    assert_eq!(
        std::fs::read(agent.join("latin.luau.bak")).unwrap(),
        b"-- n\xe3o\nreturn {}\n"
    );
    let (all, _) = call(&root, &mut kept, json!({"action": "check"})).unwrap();
    assert_eq!(all["ok"], true, "{all}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_first_write_sets_vscode_up_and_settings_keep_what_the_person_wrote() {
    let root = folder(&[]);
    let mut kept = HashSet::new();
    let (answer, console) = call(
        &root,
        &mut kept,
        json!({"action": "write", "path": "scripts/porta.luau", "text": DOOR}),
    )
    .unwrap();
    assert_eq!(
        answer["vscode"],
        json!({"written": [".oxy/tipos/oxy.d.luau", ".luaurc", ".vscode/settings.json"]})
    );
    assert_eq!(
        console[1],
        "Arquivos do VS Code gravados: .oxy/tipos/oxy.d.luau, .luaurc e .vscode/settings.json"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(script_files::TYPES_PATH)).unwrap(),
        script_files::types_text()
    );
    let settings: serde_json::Map<String, Json> =
        serde_json::from_str(&std::fs::read_to_string(root.join(".vscode/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>(),
        script_files::settings()
    );
    // The project's alias, which the runtime knows by itself, for luau-lsp.
    let luaurc: Json =
        serde_json::from_str(&std::fs::read_to_string(root.join(".luaurc")).unwrap()).unwrap();
    assert_eq!(
        luaurc,
        json!({"languageMode": "nonstrict", "aliases": {"project": "./"}})
    );
    // Once set up, writes leave the files alone.
    let (answer, console) = call(
        &root,
        &mut kept,
        json!({"action": "write", "path": "scripts/porta.luau", "text": "return {}\n"}),
    )
    .unwrap();
    assert!(
        answer.get("vscode").is_none() && console.len() == 1,
        "{answer}"
    );
    // Asked again, nothing is due, and nothing more is said.
    let (answer, console) = call(&root, &mut kept, json!({"action": "vscode"})).unwrap();
    assert_eq!(answer, json!({"written": []}));
    assert_eq!(console, ["Arquivos do VS Code em dia"]);
    // A .luaurc of the person's gets the alias, the rest as it was; one with another value for
    // it, or with comments, stays as it was, with a note.
    std::fs::write(root.join(".luaurc"), "{ \"languageMode\": \"strict\" }\n").unwrap();
    let (answer, _) = call(&root, &mut kept, json!({"action": "vscode"})).unwrap();
    assert_eq!(answer, json!({"written": [".luaurc"]}));
    assert_eq!(
        std::fs::read_to_string(root.join(".luaurc")).unwrap(),
        "{ \"languageMode\": \"strict\", \"aliases\": {\n        \"project\": \"./\"\n    } }\n"
    );
    std::fs::write(
        root.join(".luaurc"),
        "{\"aliases\": {\"Project\": \"./Pista\"}}",
    )
    .unwrap();
    let (answer, console) = call(&root, &mut kept, json!({"action": "vscode"})).unwrap();
    assert_eq!(answer["written"], json!([]));
    assert!(
        console[1].starts_with("o .luaurc dá ao apelido Project o valor \"./Pista\""),
        "{console:?}"
    );
    std::fs::write(root.join(".luaurc"), "{\n  // meu\n}\n").unwrap();
    let (answer, console) = call(&root, &mut kept, json!({"action": "vscode"})).unwrap();
    assert_eq!(answer["written"], json!([]));
    assert!(
        console[1].starts_with("o .luaurc não é JSON puro (tem comentários?) e ficou como estava"),
        "{console:?}"
    );
    // Types of another version are written again; a settings file with comments is left as
    // it was, the keys going to a file of their own.
    std::fs::write(root.join(script_files::TYPES_PATH), "-- OXY Engine 0.6.9\n").unwrap();
    let theirs = "{\n  // a fonte\n  \"editor.fontSize\": 15\n}\n";
    std::fs::write(root.join(".vscode/settings.json"), theirs).unwrap();
    let (answer, _) = call(&root, &mut kept, json!({"action": "vscode"})).unwrap();
    assert_eq!(
        answer["written"],
        json!([".oxy/tipos/oxy.d.luau", ".oxy/vscode-settings.json"])
    );
    assert!(
        answer["notes"].as_array().unwrap().iter().any(|note| note
            .as_str()
            .unwrap()
            .contains("ficou como estava; junte a ele")),
        "{answer}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".vscode/settings.json")).unwrap(),
        theirs
    );
    let suggestion: serde_json::Map<String, Json> = serde_json::from_str(
        &std::fs::read_to_string(root.join(".oxy/vscode-settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(suggestion.len(), script_files::settings().len());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_reload_warns_of_attributes_a_new_text_declares_that_objects_lack() {
    const PATH: &str = "scripts/vida.luau";
    let mut project = Project::new("Recarga");
    for name in ["Porta", "Porta 2", "Caixa"] {
        let mut e = Entity::new(name, None);
        e.attributes.insert("Vida".into(), Value::Number(3.));
        if name != "Caixa" {
            e.scripts = vec![ScriptRef::new(PATH)];
        }
        project.scenes[0].entities.push(e);
    }
    let first =
        "local M = {}\nM.attributes = { Vida = 3 }\nfunction M.on_step(self) end\nreturn M\n";
    let options = RuntimeOptions {
        scripts: Arc::new(ScriptSources::from_texts([(PATH, first)])),
        ..Default::default()
    };
    let mut rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    rt.advance(FIXED_DT, &InputFrame::default());
    let (logs, hash) = (rt.logs_written(), rt.state_hash());
    let second = first.replace("{ Vida = 3 }", "{ Vida = 3, Escudo = 1, Nome = 'x' }");
    let report = rt.reload_scripts(Arc::new(ScriptSources::from_texts([(PATH, &*second)])));
    assert_eq!(report.reloaded, [PATH]);
    assert_eq!(
        report.warnings,
        [
            "scripts/vida.luau declara agora Escudo e Nome, que Porta e Porta 2 não têm: ler dá nil \
          até você parar o teste e anexar o script de novo, o que cria os atributos que faltam"
        ]
    );
    // The reload wrote nothing in the game's log nor changed its state.
    assert_eq!((rt.logs_written(), rt.state_hash()), (logs, hash));
    // A text that declares only what the objects have warns of nothing.
    let third = first.replace("on_step(self) end", "on_step(self) local _ = 1 end");
    let report = rt.reload_scripts(Arc::new(ScriptSources::from_texts([(PATH, &*third)])));
    assert_eq!(report.reloaded, [PATH]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn entity_answers_mark_the_scripts_the_folder_lacks() {
    let root = folder(&[("scripts/porta.luau", DOOR)]);
    let mut project = Project::new("Ausente");
    let mut door = Entity::new("Porta", None);
    door.scripts = vec![
        ScriptRef::new("scripts/porta.luau"),
        ScriptRef::new("scripts/sumiu.luau"),
    ];
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    let scene = project.start_scene.clone();
    let mut one = query::entity(&project, &scene, &id).unwrap();
    assert_eq!(
        one["scripts"],
        json!([{"path": "scripts/porta.luau"}, {"path": "scripts/sumiu.luau"}])
    );
    query::mark_missing_scripts(&mut one, &root);
    assert_eq!(
        one["scripts"],
        json!([{"path": "scripts/porta.luau"}, {"path": "scripts/sumiu.luau", "missing": true}])
    );
    let mut many = query::answer(
        &project,
        &scene,
        "entity",
        &json!({"ids": [id], "fields": ["scripts"]}),
    )
    .unwrap();
    query::mark_missing_scripts(&mut many, &root);
    assert_eq!(many["objects"][0]["scripts"][1]["missing"], true);
    std::fs::remove_dir_all(root).unwrap();
}

/// The checkpoint and the countdown of design §5.8 as the slice runs them.
const CHECKPOINT: &str = r#"--!strict
local Checkpoint = {}
Checkpoint.attributes = { Ordem = 1 }
local FEET = vector.create(0, 1.5, 0)

function Checkpoint.on_area_enter(self: Object, other: Object)
    if not other:has_attribute("Checkpoint atual") then
        return
    end
    local order = self.attr.Ordem
    if other.attr["Checkpoint atual"] < order then
        other.attr["Ponto de retorno"] = self.world_position - FEET
        other.attr["Checkpoint atual"] = order
        if other.attr["Jogador local"] then
            audio.play("Checkpoint", { volume = 0.9, audience = other })
        end
    end
end

return Checkpoint
"#;

const RACE: &str = r#"--!strict
local Race = {}
Race.attributes = { Aviso = "" }

function Race.on_start(self: Object)
    vars.set("Largada liberada", false)
    self:sequence(1, Race.show, { 3, 2, 1, 0 })
    self:after(4, Race.hide_banner)
end

function Race.show(self: Object, n: number)
    vars.set("Contagem da largada", n)
    self.attr.Aviso = if n > 0 then tostring(n) else "Já!"
    audio.play(if n > 0 then "Bipe" else "Largada")
    print("contagem", n)
    if n == 0 then vars.set("Largada liberada", true) end
end

function Race.hide_banner(self: Object)
    self.attr.Aviso = ""
    warn("faixa escondida")
end

return Race
"#;

/// `luau-lsp analyze` as design §6.3 runs it, in `root`, on `files`, in the old solver or the
/// new one: whether it found nothing, and what it said.
fn analyze(lsp: &Path, root: &Path, files: &[&str], new_solver: bool) -> (bool, String) {
    analyze_with(lsp, root, files, new_solver, true)
}

/// The settings of the plan-B form: the globals the sandbox takes away, without the definitions
/// file that the settings OXY writes name.
const MODULE_SETTINGS: &str = "modulo-settings.json";

/// `analyze`, with the definitions file OXY writes or, for the plan-B module, without it.
fn analyze_with(
    lsp: &Path,
    root: &Path,
    files: &[&str],
    new_solver: bool,
    definitions: bool,
) -> (bool, String) {
    let mut command = std::process::Command::new(lsp);
    command
        .current_dir(root)
        .arg("analyze")
        .arg("--no-flags-enabled")
        .arg("--platform=standard")
        .arg("--formatter=gnu")
        .arg(format!(
            "--settings={}",
            root.join(if definitions {
                ".vscode/settings.json"
            } else {
                MODULE_SETTINGS
            })
            .display()
        ));
    if definitions {
        command.arg(format!(
            "--definitions=@oxy={}",
            root.join(script_files::TYPES_PATH).display()
        ));
    }
    if new_solver {
        command.arg("--flag:LuauSolverV2=true");
    }
    let output = command.args(files).output().unwrap();
    let said = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), said)
}

/// What luau-lsp said beyond its notes ([INFO], and [WARN] for the module form without
/// definitions): the diagnostics, errors and lint alike.
fn diagnostics(said: &str) -> Vec<&str> {
    said.lines()
        .filter(|line| {
            !line.trim().is_empty() && !line.starts_with("[INFO]") && !line.starts_with("[WARN]")
        })
        .collect()
}

/// `text` as a script of the plan-B form (S2 §3): the module required, the types and the
/// global tables it uses taken from it.
fn module_form(text: &str) -> String {
    let uses = |name: &str| {
        text.match_indices(name).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + name.len()..].chars().next();
            !before.is_some_and(|c| c.is_alphanumeric() || "_.:".contains(c))
                && after.is_some_and(|c| c == '.' || c == '(' || c == '<' || c == ',' || c == ')')
        })
    };
    let mut header = vec!["local oxy = require(\"@oxy\")".to_owned()];
    for ty in ["Object", "CharacterEvent"] {
        if uses(ty) {
            header.push(format!("type {ty} = oxy.{ty}"));
        }
    }
    if uses("ScriptObject") {
        header.push("type ScriptObject<A, S> = oxy.ScriptObject<A, S>".to_owned());
    }
    let mut globals: Vec<&str> = registry::API
        .iter()
        .filter(|f| f.kind == registry::ApiKind::Function && f.in_types() && f.name != "print")
        .map(|f| {
            if f.holder().is_empty() {
                f.name
            } else {
                f.holder()
            }
        })
        .collect();
    globals.dedup();
    let used: Vec<&str> = globals.into_iter().filter(|g| uses(g)).collect();
    if !used.is_empty() {
        let taken: Vec<String> = used.iter().map(|g| format!("oxy.{g}")).collect();
        header.push(format!("local {} = {}", used.join(", "), taken.join(", ")));
    }
    let (first, rest) = text.split_once('\n').unwrap();
    if header.len() == 1 {
        return text.to_owned();
    }
    format!("{first}\n{}\n{rest}", header.join("\n"))
}

/// Typed by its declarations (`ScriptObject`), as the Guide shows: a marker typed as the value
/// the attribute holds, the memory with its fields.
const TYPED: &str = r#"--!strict
local Race = {}
Race.attributes = { Aviso = "", Painel = param.object(), Tamanho = param.vector2(1, 1),
    Piso = param.surface("Gelo") }
Race.state = { mostrados = 0, alvo = param.object() }
type Self = ScriptObject<typeof(Race.attributes), typeof(Race.state)>

function Race.on_start(self: Self)
    self:sequence(1, Race.show, { 3, 2, 1, 0 })
    self:after(4, Race.hide_banner)
end

function Race.show(self: Self, n: number)
    self.attr.Aviso = if n > 0 then tostring(n) else "Já!"
    self.state.mostrados += 1
end

function Race.hide_banner(self: Self)
    local painel = self.attr.Painel
    if painel then
        painel.position = self.attr.Tamanho
        painel.visible = false
    end
    local piso = self.attr.Piso
    if piso then
        print(piso.name)
    end
    local alvo = self.state.alvo
    if alvo then
        alvo.orientation = quat.multiply(alvo.orientation, quat.from_euler(0, 90, 0))
    end
    self.attr.Aviso = ""
end

return Race
"#;

#[test]
fn luau_lsp_reads_the_types_without_false_errors_and_catches_wrong_names() {
    // Optional: only where a luau-lsp executable is named (design §5.7); it is never shipped.
    let Some(lsp) = std::env::var_os("OXY_LUAU_LSP").map(PathBuf::from) else {
        return;
    };
    let root = folder(&[
        ("scripts/checkpoint.luau", CHECKPOINT),
        ("scripts/controle_da_corrida.luau", RACE),
        (
            "scripts/errado.luau",
            &CHECKPOINT.replace("audio.play(", "audio.paly("),
        ),
        (
            "scripts/sem_strict.luau",
            &RACE
                .replace("--!strict\n", "")
                .replace("vars.set(\"Contagem", "vars.sett(\"Contagem"),
        ),
    ]);
    script_files::setup_vscode(&root).unwrap();
    let good = [
        "scripts/checkpoint.luau",
        "scripts/controle_da_corrida.luau",
    ];
    for new_solver in [false, true] {
        let (clean, said) = analyze(&lsp, &root, &good, new_solver);
        assert!(clean, "solver novo {new_solver}: {said}");
        assert!(!said.contains("Error"), "{said}");
        let (clean, said) = analyze(&lsp, &root, &["scripts/errado.luau"], new_solver);
        assert!(!clean, "{said}");
        assert!(said.contains("Key 'paly' not found"), "{said}");
    }
    let (clean, said) = analyze(&lsp, &root, &["scripts/sem_strict.luau"], false);
    assert!(!clean && said.contains("Key 'sett' not found"), "{said}");
    // The engine refuses math.random: luau-lsp does too, with the settings OXY writes.
    std::fs::write(
        root.join("scripts/sorteio.luau"),
        "local M = {}\nfunction M.on_start(self) print(math.random(1, 6)) end\nreturn M\n",
    )
    .unwrap();
    let (clean, said) = analyze(&lsp, &root, &["scripts/sorteio.luau"], false);
    assert!(!clean && said.contains("Key 'random' not found"), "{said}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn luau_lsp_resolves_the_requires_the_engine_takes_with_the_luaurc_oxy_writes() {
    // Optional, as the test above: the parity of `require` in the host's tests, on disk.
    let Some(lsp) = std::env::var_os("OXY_LUAU_LSP").map(PathBuf::from) else {
        return;
    };
    let root = folder(&[
        ("Pista/dados.luau", "return { velocidade = 3 }\n"),
        ("Pista/Dados da pista.luau", "return { volta = 1 }\n"),
        ("Comum/sons.luau", "return { pulo = 'Pulo' }\n"),
        ("Fases/Fase ação/fase.luau", "return { numero = 1 }\n"),
        ("raiz.luau", "return { raiz = true }\n"),
    ]);
    script_files::setup_vscode(&root).unwrap();
    let check = |name: &str| {
        // A key the module lacks: luau-lsp names it only when the path resolved.
        std::fs::write(
            root.join("Pista/Checkpoint.luau"),
            format!("--!strict\nlocal Modulo = require(\"{name}\")\nlocal _ = Modulo.naoexiste\nreturn {{}}\n"),
        )
        .unwrap();
        analyze(&lsp, &root, &["Pista/Checkpoint.luau"], false).1
    };
    for name in [
        "./dados",
        "../Comum/sons",
        "./Dados da pista",
        "../Fases/Fase ação/fase",
        "../raiz",
        "@project/Pista/dados",
        "@Project/Comum/sons",
    ] {
        let said = check(name);
        assert!(said.contains("Key 'naoexiste' not found"), "{name}: {said}");
    }
    let said = check("./naoexiste");
    assert!(said.contains("Unknown require"), "{said}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn luau_lsp_reads_every_example_typed_objects_and_the_module_in_both_solvers() {
    // Optional, as the test above.
    let Some(lsp) = std::env::var_os("OXY_LUAU_LSP").map(PathBuf::from) else {
        return;
    };
    // The names of 0.8 are not in the types: their examples only show the error of using them.
    let examples: Vec<(String, String)> = registry::API
        .iter()
        .enumerate()
        .filter(|(_, f)| f.kind != registry::ApiKind::Reserved)
        .map(|(i, f)| (format!("scripts/api/{i:03}.luau"), f.example_script()))
        .collect();
    let mut files: Vec<(String, String)> = examples.clone();
    files.push(("scripts/tipado.luau".into(), TYPED.into()));
    // The module require's example loads by the project's alias, which the .luaurc OXY writes
    // gives luau-lsp, for the examples in both forms.
    files.push(("Pista/dados.luau".into(), "return { custo = 2 }\n".into()));
    // Names the script does not declare, and an empty object attribute read unchecked.
    for (name, from, to) in [
        (
            "avisoo",
            "self.attr.Aviso = \"\"\n",
            "self.attr.Avisoo = \"\"\n",
        ),
        (
            "memoria",
            "self.state.mostrados += 1",
            "self.state.mostrado += 1",
        ),
        (
            "painel",
            "local painel = self.attr.Painel\n    if painel then",
            "self.attr.Painel.position = self.attr.Tamanho\n    if true then\n        local \
             painel = self.attr.Painel :: Object",
        ),
        // A memory field declared empty is an Object? too.
        (
            "alvo",
            "local alvo = self.state.alvo\n    if alvo then",
            "self.state.alvo.visible = false\n    if true then\n        local alvo = \
             self.state.alvo :: Object",
        ),
    ] {
        assert!(TYPED.contains(from), "{name}");
        files.push((format!("scripts/{name}.luau"), TYPED.replacen(from, to, 1)));
    }
    // The plan-B form: the module and the same scripts requiring it.
    files.push((".oxy/tipos/oxy.luau".into(), script::generate::module()));
    files.push((
        "modulo/.luaurc".into(),
        "{ \"aliases\": { \"oxy\": \"../.oxy/tipos/oxy\" } }\n".into(),
    ));
    let mut scripts = examples.clone();
    scripts.push(("scripts/tipado.luau".into(), TYPED.into()));
    for (path, text) in &scripts {
        files.push((path.replace("scripts/", "modulo/"), module_form(text)));
    }
    files.push((
        "modulo/errado.luau".into(),
        module_form(&registry::find("audio.play").unwrap().example_script())
            .replace("audio.play(", "audio.paly("),
    ));
    files.push((
        MODULE_SETTINGS.into(),
        json!({ "luau-lsp.types.disabledGlobals": script_files::DISABLED_GLOBALS }).to_string(),
    ));
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    let root = folder(&borrowed);
    script_files::setup_vscode(&root).unwrap();
    let mut good: Vec<&str> = examples.iter().map(|(path, _)| path.as_str()).collect();
    good.push("scripts/tipado.luau");
    let module: Vec<String> = good
        .iter()
        .map(|path| path.replace("scripts/", "modulo/"))
        .collect();
    let mut module: Vec<&str> = module.iter().map(String::as_str).collect();
    module.push(".oxy/tipos/oxy.luau");
    for new_solver in [false, true] {
        let (clean, said) = analyze(&lsp, &root, &good, new_solver);
        assert!(
            clean && diagnostics(&said).is_empty(),
            "solver novo {new_solver}: {said}"
        );
        let (clean, said) = analyze_with(&lsp, &root, &module, new_solver, false);
        assert!(
            clean && diagnostics(&said).is_empty(),
            "módulo, solver novo {new_solver}: {said}"
        );
        for (name, found) in [
            ("avisoo", "Cannot add property 'Avisoo'"),
            ("memoria", "key 'mostrado'"),
            ("painel", "could be nil"),
            ("alvo", "could be nil"),
        ] {
            let (clean, said) =
                analyze(&lsp, &root, &[&format!("scripts/{name}.luau")], new_solver);
            assert!(
                !clean && said.contains(found),
                "{name}, solver novo {new_solver}: {said}"
            );
        }
        let (clean, said) = analyze_with(&lsp, &root, &["modulo/errado.luau"], new_solver, false);
        assert!(!clean && said.contains("Key 'paly' not found"), "{said}");
    }
    std::fs::remove_dir_all(root).unwrap();
}
