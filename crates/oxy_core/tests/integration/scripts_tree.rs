//! Scripts in any folder of the project's tree: the one walk the scripts, copies and exports
//! share (what it leaves out, its order, links and junctions, nested projects and exported games,
//! names that are not UTF-8, files another program holds), scripts that require each other by
//! path and by the project's alias, the stamps of the editor's watcher, oxy_scripts and VS Code's
//! files on that tree, and the checks that look a file up alone.
use oxy_core::{
    document::*,
    persistence::{self, PersistenceError},
    project_tree::{self, FileKind, Presence},
    query,
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions, ScriptLocation},
    script::ScriptRef,
    script_files::{self, Request},
    texture_cache::TextureCache,
};
use serde_json::{Value as Json, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-scripts-tree-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// A project's folder with a bit of everything a person keeps in one, OXY's own files, the
/// leftovers of Windows and of editors, an exported game and another project inside.
fn crowded() -> PathBuf {
    let root = folder();
    for (path, text) in [
        ("Pista/Checkpoints/porta.luau", "return {}\n"),
        ("Pista/dados.luau", "return {}\n"),
        ("Pista/Música.wav", "RIFF"),
        ("Pista/Checkpoint.modelo.json", "{}"),
        ("Pista/notas.txt", "fica de fora"),
        ("Pista/Velho.LUAU", "return {}\n"),
        ("Pista/.luaurc", "{\"aliases\": {\"project\": \"./\"}}"),
        ("Pista/.rascunho.luau", "return {}\n"),
        ("Pista/x.swp", ""),
        ("Pista/~$x.luau", "return {}\n"),
        // A folder of the person's, whose name only looks like a leftover's.
        ("Velho.bak/x.luau", "return {}\n"),
        ("Comum/sons.luau", "return {}\n"),
        ("Imagens/Fundo.PNG", "png"),
        ("Fase 10/a.luau", "return {}\n"),
        ("Fase 2/a.luau", "return {}\n"),
        ("raiz.luau", "return {}\n"),
        (".oxy/tipos/oxy.d.luau", "--"),
        (".oxy/agente/Pista/dados.luau.bak", "return {}\n"),
        (".vscode/settings.json", "{}"),
        (".luaurc", "{}"),
        (".git/HEAD", "ref"),
        (".solucoes/porta.luau", "return {}\n"),
        ("project.oxy.json", "{}"),
        ("project.oxy.json.agente.bak", "{}"),
        ("export.oxy.json", "{}"),
        ("Thumbs.db", ""),
        ("desktop.ini", ""),
        ("~$notas.docx", ""),
        ("porta.luau~", ""),
        ("x.tmp", ""),
        ("scripts/porta.luau.agente.bak", "return {"),
        ("Jogo antigo/data/export.oxy.json", "{}"),
        ("Jogo antigo/data/Pista/dados.luau", "return {}\n"),
        ("Outro/project.oxy.json", "{}"),
        ("Outro/y.luau", "return {}\n"),
    ] {
        write(&root, path, text);
    }
    root
}

#[test]
fn the_walk_lists_the_project_in_tree_order_and_leaves_out_what_is_not_its() {
    let root = crowded();
    let tree = project_tree::walk(&root).unwrap();
    // Folders first, numbers in natural order, each folder before what it holds.
    assert_eq!(
        tree.folders,
        [
            "Comum",
            "Fase 2",
            "Fase 10",
            "Imagens",
            "Pista",
            "Pista/Checkpoints",
            "scripts",
            "Velho.bak"
        ]
    );
    let files: Vec<(&str, FileKind)> = tree
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.kind))
        .collect();
    assert_eq!(
        files,
        [
            ("Comum/sons.luau", FileKind::Script),
            ("Fase 2/a.luau", FileKind::Script),
            ("Fase 10/a.luau", FileKind::Script),
            ("Imagens/Fundo.PNG", FileKind::Image),
            ("Pista/Checkpoints/porta.luau", FileKind::Script),
            ("Pista/Checkpoint.modelo.json", FileKind::Model),
            ("Pista/dados.luau", FileKind::Script),
            ("Pista/Música.wav", FileKind::Sound),
            ("Pista/notas.txt", FileKind::Other),
            ("Pista/Velho.LUAU", FileKind::Other),
            ("Velho.bak/x.luau", FileKind::Script),
            ("raiz.luau", FileKind::Script),
        ]
    );
    assert_eq!(
        tree.skipped.iter().collect::<Vec<_>>(),
        [
            (
                &"Jogo antigo".to_owned(),
                &"é outro projeto ou um jogo exportado".to_owned()
            ),
            (
                &"Outro".to_owned(),
                &"é outro projeto ou um jogo exportado".to_owned()
            ),
        ]
    );
    let stamp = tree.files[0].stamp.clone().unwrap();
    assert_eq!(stamp.len, "return {}\n".len() as u64);
    assert!(stamp.modified.is_some());
    assert_eq!(
        tree.warnings,
        ["Pista/Velho.LUAU: renomeie para .luau, em minúsculas; só arquivos .luau são scripts"]
    );
    assert_eq!(tree.configs, ["Pista/.luaurc"]);
    assert!(tree.unreadable.is_empty());
    // The scripts are those files, read on their own; the stamps the watcher compares too.
    let sources = persistence::load_scripts(&root).unwrap();
    let scripts = [
        "Comum/sons.luau",
        "Fase 10/a.luau",
        "Fase 2/a.luau",
        "Pista/Checkpoints/porta.luau",
        "Pista/dados.luau",
        "Velho.bak/x.luau",
        "raiz.luau",
    ];
    assert_eq!(sources.files.keys().collect::<Vec<_>>(), scripts);
    let stamps = persistence::script_stamps(&root).unwrap();
    assert_eq!(stamps.keys().collect::<Vec<_>>(), scripts);
    write(&root, "Pista/dados.luau", "return { mudou = true }\n");
    assert_ne!(
        persistence::script_stamps(&root).unwrap()["Pista/dados.luau"],
        stamps["Pista/dados.luau"]
    );
    // An exported game's data folder is walked as a project: its own files at the root go.
    let data = root.join("Jogo antigo/data");
    let game = project_tree::walk(&data).unwrap();
    assert_eq!(game.files.len(), 1);
    assert_eq!(game.files[0].path, "Pista/dados.luau");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_file_is_looked_up_alone_as_the_walk_would_find_it() {
    let root = crowded();
    let tree = project_tree::walk(&root).unwrap();
    for path in [
        "Pista/Checkpoints/porta.luau",
        "Pista/dados.luau",
        "pista/dados.luau",
        "Pista/DADOS.luau",
        "Pista/.rascunho.luau",
        ".solucoes/porta.luau",
        "Outro/y.luau",
        "Jogo antigo/data/Pista/dados.luau",
        "Pista/nada.luau",
        "Pista",
        "project.oxy.json",
        "scripts/porta.luau.agente.bak",
        "Pista/x.swp",
        "Pista/~$x.luau",
        "Velho.bak/x.luau",
        "raiz.luau",
        "",
        "Pista//dados.luau",
    ] {
        let listed = tree.files.iter().any(|file| file.path == path);
        let present = project_tree::presence(&root, path) == Presence::Present;
        assert_eq!(present, listed, "{path}");
    }
    assert_eq!(
        project_tree::presence(&root.join("nenhuma"), "Pista/dados.luau"),
        Presence::Missing
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_script_in_a_folder_the_tree_skips_says_why() {
    let root = crowded();
    write(
        &root,
        "Pista/fora.luau",
        "local Y = require('@project/Outro/y')\nreturn {}\n",
    );
    let mut project = Project::new("Fora");
    let mut e = Entity::new("Porta", None);
    e.scripts = vec![ScriptRef::new("Pista/fora.luau")];
    let id = e.id.clone();
    project.scenes[0].entities.push(e);
    let sources = persistence::load_scripts_to_run(&project, &root).unwrap();
    let why = "fica fora do projeto: Outro é outro projeto ou um jogo exportado";
    assert_eq!(
        sources.absent("Outro/y.luau"),
        format!("o script Outro/y.luau {why}")
    );
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    let rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    let expected = format!("Pista/fora.luau:1: require(\"@project/Outro/y\"): Outro/y.luau {why}");
    assert!(
        rt.logs.iter().any(|line| line.text.starts_with(&expected)),
        "{:?}",
        rt.logs
    );
    // Attached there, a run says so too.
    project.scenes[0].entity_mut(&id).unwrap().scripts =
        vec![ScriptRef::new("Jogo antigo/data/Pista/dados.luau")];
    let refused = persistence::load_scripts_to_run(&project, &root).unwrap_err();
    assert!(
        refused.starts_with(
            "Porta: o script Jogo antigo/data/Pista/dados.luau fica fora do projeto: Jogo antigo \
             é outro projeto ou um jogo exportado"
        ),
        "{refused}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_name_with_an_accent_apart_from_its_letter_is_warned_of_and_named_in_hints() {
    let root = folder();
    let decomposed = "Pista/ac\u{327}a\u{303}o.luau";
    write(&root, decomposed, "return {}\n");
    let tree = project_tree::walk(&root).unwrap();
    assert_eq!(
        tree.warnings,
        [format!(
            "{decomposed}: o nome tem um acento separado da letra (como os nomes feitos no \
             macOS); renomeie-o digitando a letra acentuada"
        )]
    );
    let sources = persistence::load_scripts(&root).unwrap();
    assert!(sources.get(decomposed).is_some());
    assert_eq!(
        sources.absent("Pista/ação.luau"),
        format!(
            "o script Pista/ação.luau não existe na pasta do projeto; o nome de {decomposed} tem \
             um acento separado da letra: renomeie o arquivo digitando a letra acentuada"
        )
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn oxy_validate_says_when_the_folder_is_too_large_for_a_project() {
    let root = folder();
    let deep = vec!["a"; project_tree::MAX_DEPTH + 1].join("/");
    write(&root, &format!("{deep}/x.luau"), "return {}\n");
    let validated = query::validation_in(&Project::new("Fundo"), Some(&root));
    assert!(
        validated["warnings"].as_array().unwrap().iter().any(|w| w
            .as_str()
            .unwrap()
            .starts_with("A pasta do projeto é grande demais: tem pastas mais de 16 níveis")),
        "{validated}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A junction at `link` to `target`, which Windows makes without rights a symbolic link asks
/// for.
#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
}

#[cfg(windows)]
#[test]
fn junctions_are_never_followed_nor_written_through() {
    let root = folder();
    let outside = folder();
    write(&root, "Pista/dados.luau", "return {}\n");
    write(&outside, "fora.luau", "return {}\n");
    junction(&root.join("Pista").join("Atalho"), &outside);
    junction(&root.join("Espelho"), &root.join("Pista"));
    let tree = project_tree::walk(&root).unwrap();
    assert_eq!(
        tree.files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["Pista/dados.luau"]
    );
    assert_eq!(tree.folders, ["Pista"]);
    assert_eq!(
        tree.warnings,
        [
            "Espelho é um atalho (link ou junção) e fica fora do projeto: a OXY não segue \
             atalhos",
            "Pista/Atalho é um atalho (link ou junção) e fica fora do projeto: a OXY não segue \
             atalhos",
        ]
    );
    assert_eq!(
        tree.skipped.keys().collect::<Vec<_>>(),
        ["Espelho", "Pista/Atalho"]
    );
    // A script behind one says why it is none.
    let sources = persistence::load_scripts(&root).unwrap();
    assert_eq!(
        sources.absent("Espelho/dados.luau"),
        "o script Espelho/dados.luau fica fora do projeto: Espelho é um atalho (link ou junção), \
         e a OXY não segue atalhos"
    );
    for path in ["Pista/Atalho/fora.luau", "Espelho/dados.luau"] {
        assert_eq!(
            project_tree::presence(&root, path),
            Presence::Missing,
            "{path}"
        );
        assert!(
            persistence::read_script(&root, path).unwrap().is_none(),
            "{path}"
        );
        assert!(
            persistence::write_script(&root, &path.replace("fora", "novo"), "return {}\n", false)
                .is_err(),
            "{path}"
        );
    }
    let error =
        persistence::write_script(&root, "Espelho/novo.luau", "return {}\n", false).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Caminho inválido Espelho/novo.luau: Espelho é um atalho (link ou junção), e a OXY não \
         segue atalhos"
    );
    assert!(!root.join("Pista/novo.luau").exists());
    assert_eq!(
        std::fs::read_dir(&outside).unwrap().count(),
        1,
        "nothing written through the junction"
    );
    // The junctions go, what they point at stays.
    std::fs::remove_dir(root.join("Espelho")).unwrap();
    std::fs::remove_dir(root.join("Pista/Atalho")).unwrap();
    assert!(outside.join("fora.luau").is_file());
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn a_name_that_is_not_utf8_is_left_out_with_a_warning_and_never_stops_the_rest() {
    use std::os::windows::ffi::OsStringExt;
    let root = folder();
    write(&root, "Pista/dados.luau", "return {}\n");
    // A lone surrogate: a name Windows keeps and UTF-8 cannot say.
    let name = |extension: &str| {
        let mut wide = vec![u16::from(b'x'), 0xD800];
        wide.extend(extension.encode_utf16());
        std::ffi::OsString::from_wide(&wide)
    };
    std::fs::write(root.join("Pista").join(name(".luau")), "return {}\n").unwrap();
    std::fs::write(root.join("Pista").join(name(".txt")), "").unwrap();
    let tree = project_tree::walk(&root).unwrap();
    assert_eq!(tree.warnings.len(), 1, "{:?}", tree.warnings);
    assert!(
        tree.warnings[0].starts_with("Pista/x\u{FFFD}.txt: o nome não é UTF-8"),
        "{:?}",
        tree.warnings
    );
    let sources = persistence::load_scripts(&root).unwrap();
    assert_eq!(
        sources.files.keys().collect::<Vec<_>>(),
        ["Pista/dados.luau"]
    );
    assert_eq!(
        sources.unreadable["Pista/x\u{FFFD}.luau"],
        "tem um nome que não é UTF-8; renomeie o arquivo"
    );
    // A copy of the project takes the others: no object or require can name that one.
    let project = Project::new("Cópia");
    let copies = folder();
    let copy = copies.join("Cópia").join(persistence::PROJECT_FILE);
    let mut images = TextureCache::default();
    images.configure(&root, &project);
    persistence::save_cached_copy(&copy, &project, &mut images, &root).unwrap();
    assert_eq!(
        persistence::load_scripts(copy.parent().unwrap())
            .unwrap()
            .files
            .keys()
            .collect::<Vec<_>>(),
        ["Pista/dados.luau"]
    );
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(copies).unwrap();
}

#[cfg(windows)]
#[test]
fn a_file_another_program_holds_is_unreadable_now_not_missing() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = folder();
    write(&root, "Pista/dados.luau", "return {}\n");
    write(&root, "Pista/preso.luau", "return {}\n");
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.join("Pista/preso.luau"))
        .unwrap();
    let sources = persistence::load_scripts(&root).unwrap();
    assert_eq!(
        sources.files.keys().collect::<Vec<_>>(),
        ["Pista/dados.luau"]
    );
    assert!(
        sources.unreadable["Pista/preso.luau"].starts_with("não pôde ser lido agora"),
        "{:?}",
        sources.unreadable
    );
    assert_eq!(
        project_tree::presence(&root, "Pista/preso.luau"),
        Presence::Present
    );
    let mut project = Project::new("Preso");
    let mut e = Entity::new("Porta", None);
    e.scripts = vec![ScriptRef::new("Pista/preso.luau")];
    project.scenes[0].entities.push(e);
    // Opening warns of nothing missing; a run says why the file cannot run.
    assert!(persistence::script_warnings(&project, &root).is_empty());
    let refused = persistence::load_scripts_to_run(&project, &root).unwrap_err();
    assert!(
        refused.starts_with("Porta: o script Pista/preso.luau não pôde ser lido agora"),
        "{refused}"
    );
    drop(held);
    assert!(persistence::load_scripts_to_run(&project, &root).is_ok());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn scripts_in_any_folder_require_each_other_by_path_and_by_the_alias() {
    let root = folder();
    write(
        &root,
        "Pista/Checkpoints/porta.luau",
        "local Dados = require('../dados')\nlocal Sons = require('@project/Comum/sons')\n\
         local Mesmo = require('@project/Pista/dados')\nlocal M = {}\nM.state = { voltas = 0 }\n\
         function M.on_start(self)\n    self.attr.Resultado = Dados.nome .. ' ' .. Sons.pulo .. \
         ' ' .. tostring(rawequal(Dados, Mesmo))\nend\nfunction M.on_step(self)\n    \
         self.state.voltas += 1\n    if self.state.voltas == 3 then print(Sons.pulo) end\nend\n\
         return M\n",
    );
    write(&root, "Pista/dados.luau", "return { nome = 'pista' }\n");
    write(&root, "Comum/sons.luau", "return { pulo = 'Pulo' }\n");
    let mut project = Project::new("Pista");
    let mut door = Entity::new("Porta", None);
    door.attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    door.scripts = vec![ScriptRef::new("Pista/Checkpoints/porta.luau")];
    let id = door.id.clone();
    project.scenes[0].entities.push(door);
    validate_project(&project).unwrap();
    assert!(persistence::script_warnings(&project, &root).is_empty());
    let sources = persistence::load_scripts_to_run(&project, &root).unwrap();
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    let mut rt = Runtime::with_options(&project, &project.start_scene, options.clone()).unwrap();
    for _ in 0..3 {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(rt.script_errors().count(), 0);
    assert_eq!(
        rt.scene().entity(&id).unwrap().attributes["Resultado"],
        Value::Text("pista Pulo true".into())
    );
    let line = rt.logs.last().unwrap();
    assert_eq!(line.text, "Porta · Pulo");
    assert_eq!(
        line.script,
        Some(ScriptLocation {
            path: "Pista/Checkpoints/porta.luau".into(),
            line: 11
        })
    );
    // The memory is the engine's, by the script's path from the project's folder: a VM made
    // anew goes on where the last one stopped, and the same run gives the same state.
    let hash = rt.state_hash();
    rt.recreate_script_host().unwrap();
    assert_eq!(rt.state_hash(), hash);
    let mut again = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    for _ in 0..3 {
        again.advance(FIXED_DT, &InputFrame::default());
    }
    assert_eq!(again.state_hash(), hash);
    // An error in a script of any folder names its file and line.
    write(
        &root,
        "Pista/dados.luau",
        "return { nome = 'pista' }\nlocal x = (\n",
    );
    let sources = persistence::load_scripts_to_run(&project, &root).unwrap();
    let options = RuntimeOptions {
        scripts: Arc::new(sources),
        ..Default::default()
    };
    let rt = Runtime::with_options(&project, &project.start_scene, options).unwrap();
    let error = &rt.logs[0];
    assert!(
        error.text.starts_with(
            "Pista/Checkpoints/porta.luau:1: require(\"../dados\"): Pista/dados.luau:2:"
        ),
        "{}",
        error.text
    );
    assert_eq!(
        error.script,
        Some(ScriptLocation {
            path: "Pista/Checkpoints/porta.luau".into(),
            line: 1
        })
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn call(root: &Path, args: Json) -> Result<(Json, Vec<String>), String> {
    let request = Request::parse(&args)?;
    script_files::answer(root, None, &request, &mut HashSet::new())
        .map(|answer| (answer.json, answer.console))
}

#[test]
fn oxy_scripts_writes_reads_and_checks_scripts_anywhere_in_the_case_the_disk_has() {
    let root = folder();
    write(&root, "Pista/dados.luau", "return { nome = 'pista' }\n");
    // A new script in a new folder, requiring the module of the folder above.
    let text = "local Dados = require('../dados')\nlocal M = {}\nfunction M.on_start(self) \
                print(Dados.nome) end\nreturn M\n";
    let (answer, console) = call(
        &root,
        json!({"action": "write", "path": "Pista/Checkpoints/porta.luau", "text": text}),
    )
    .unwrap();
    assert_eq!(answer["created"], true);
    assert_eq!(answer["path"], "Pista/Checkpoints/porta.luau");
    assert_eq!(
        console[0],
        "Script Pista/Checkpoints/porta.luau criado (4 linhas)"
    );
    // A path in another case is the file the disk has, and the answer says so.
    let (answer, _) = call(
        &root,
        json!({"action": "write", "path": "pista/DADOS.luau", "text": "return { nome = 'Pista' }\n"}),
    )
    .unwrap();
    assert_eq!(
        (answer["path"].as_str(), answer["created"].as_bool()),
        (Some("Pista/dados.luau"), Some(false))
    );
    let (read, _) = call(&root, json!({"action": "read", "path": "PISTA/dados.luau"})).unwrap();
    assert_eq!(read["path"], "Pista/dados.luau");
    // A file the tree lacks is named with the one meant.
    let error = call(&root, json!({"action": "read", "path": "Pista/dado.luau"})).unwrap_err();
    assert!(
        error.starts_with(
            "O script Pista/dado.luau não existe na pasta do projeto; quis dizer \
             Pista/dados.luau?; scripts: "
        ),
        "{error}"
    );
    // Folders outside the project: dots, another project, an exported game.
    write(&root, "Outro/project.oxy.json", "{}");
    write(&root, "Jogo/data/export.oxy.json", "{}");
    for (path, refused) in [
        (".solucoes/porta.luau", "começa com ponto"),
        (
            "Outro/porta.luau",
            "Outro é outro projeto ou um jogo exportado",
        ),
        (
            "Jogo/porta.luau",
            "Jogo é outro projeto ou um jogo exportado",
        ),
        ("Pista/init.luau", "módulo de uma pasta"),
        // The walk skips it: written, it would never be read back.
        ("Pista/~$porta.luau", "deixam para trás"),
    ] {
        let error = call(
            &root,
            json!({"action": "write", "path": path, "text": "return {}\n"}),
        )
        .unwrap_err();
        assert!(error.contains(refused), "{path}: {error}");
    }
    assert!(!root.join("Outro/porta.luau").exists());
    assert!(!root.join("Pista/~$porta.luau").exists());
    // Every script of the tree, with what the tree warns of.
    write(&root, "Menu/Velho.LUAU", "return {}\n");
    write(
        &root,
        "Menu/.config.luau",
        "return { aliases = { project = './Menu' } }\n",
    );
    let (all, _) = call(&root, json!({"action": "check"})).unwrap();
    assert_eq!(all["ok"], true, "{all}");
    assert_eq!(
        all["checked"],
        json!(["Pista/Checkpoints/porta.luau", "Pista/dados.luau"])
    );
    assert_eq!(
        all["warnings"],
        json!([
            "Menu/Velho.LUAU: renomeie para .luau, em minúsculas; só arquivos .luau são scripts",
            "Menu/.config.luau define o apelido project: nos scripts dessa pasta o VS Code lê \
             require(\"@project/…\") a partir dela, e o jogo, a partir da pasta do projeto; tire \
             o apelido desse arquivo",
        ])
    );
    // oxy_validate tells the same, beside the lint.
    let project = Project::new("Pista");
    let validated = query::validation_in(&project, Some(&root));
    let warnings = validated["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().starts_with("Menu/Velho.LUAU: renomeie")),
        "{validated}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_luaurc_gets_the_alias_when_due_even_with_the_types_up_to_date() {
    let root = folder();
    script_files::setup_vscode(&root).unwrap();
    assert!(
        script_files::setup_vscode_when_due(&root)
            .unwrap()
            .is_none()
    );
    // The .luaurc OXY wrote before the alias is OXY's: written anew, the types in their day.
    std::fs::write(
        root.join(".luaurc"),
        "{\n    \"languageMode\": \"nonstrict\"\n}\n",
    )
    .unwrap();
    assert!(!script_files::types_outdated(&root));
    let done = script_files::setup_vscode_when_due(&root).unwrap().unwrap();
    assert_eq!(done.written, [".luaurc"]);
    let written: Json =
        serde_json::from_str(&std::fs::read_to_string(root.join(".luaurc")).unwrap()).unwrap();
    assert_eq!(written["aliases"]["project"], "./");
    assert!(
        script_files::setup_vscode_when_due(&root)
            .unwrap()
            .is_none()
    );
    // The person's .luaurc gets the alias among theirs, the plan B's included.
    let theirs = "{\n    \"languageMode\": \"strict\",\n    \"aliases\": {\n        \"oxy\": \
                  \".oxy/tipos/oxy\"\n    }\n}\n";
    std::fs::write(root.join(".luaurc"), theirs).unwrap();
    let done = script_files::setup_vscode_when_due(&root).unwrap().unwrap();
    assert_eq!(done.written, [".luaurc"]);
    assert_eq!(
        std::fs::read_to_string(root.join(".luaurc")).unwrap(),
        "{\n    \"languageMode\": \"strict\",\n    \"aliases\": {\n        \"oxy\": \
         \".oxy/tipos/oxy\",\n        \"project\": \"./\"\n    }\n}\n"
    );
    assert!(
        script_files::setup_vscode_when_due(&root)
            .unwrap()
            .is_none()
    );
    // One OXY cannot change is not due: it is told when VS Code is set up.
    std::fs::write(root.join(".luaurc"), "{\n  // meu\n}\n").unwrap();
    assert!(
        script_files::setup_vscode_when_due(&root)
            .unwrap()
            .is_none()
    );
    // A subfolder's configuration that gives the alias another folder is noted.
    write(
        &root,
        "Fases/.luaurc",
        "{\"aliases\": {\"PROJECT\": \"./Comum\"}}",
    );
    let done = script_files::setup_vscode(&root).unwrap();
    assert!(
        done.notes
            .iter()
            .any(|note| note.starts_with("Fases/.luaurc define o apelido project")),
        "{:?}",
        done.notes
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn objects_name_their_missing_scripts_by_the_path_each_cites() {
    let root = folder();
    write(&root, "Pista/porta.luau", "return {}\n");
    write(&root, ".solucoes/porta.luau", "return {}\n");
    let mut answer = json!({"objects": [
        {"scripts": [{"path": "Pista/porta.luau"}, {"path": "pista/porta.luau"}]},
        {"scripts": [{"path": ".solucoes/porta.luau"}, {"path": "Pista/nada.luau"}]},
    ]});
    query::mark_missing_scripts(&mut answer, &root);
    assert_eq!(
        answer,
        json!({"objects": [
            {"scripts": [{"path": "Pista/porta.luau"}, {"path": "pista/porta.luau", "missing": true}]},
            {"scripts": [
                {"path": ".solucoes/porta.luau", "missing": true},
                {"path": "Pista/nada.luau", "missing": true}
            ]},
        ]})
    );
    // Opening and saving warn of them by the same rule.
    let mut project = Project::new("Ausentes");
    let mut e = Entity::new("Porta", None);
    e.scripts = vec![ScriptRef::new("pista/porta.luau")];
    project.scenes[0].entities.push(e);
    assert_eq!(
        persistence::missing_scripts(&project, &root).unwrap()[0].path,
        "pista/porta.luau"
    );
    let error = persistence::validate_script_files(&project, &root).unwrap_err();
    assert!(matches!(error, PersistenceError::ScriptMissing { .. }));
    std::fs::remove_dir_all(root).unwrap();
}
