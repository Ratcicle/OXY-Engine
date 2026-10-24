//! Format 6: objects attach scripts by their path from the project's folder, whose tree holds
//! the files; graph-only projects read, run and save as in format 5 but for the version.
use oxy_core::{
    document::*,
    persistence::{self, PersistenceError},
    script::{ScriptFile, ScriptRef},
    texture_cache::TextureCache,
};
use std::path::{Path, PathBuf};

fn folder(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-scripts-{label}-{}", new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// Every file under `root`, by its path from it with `/`, sorted.
fn files(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy();
                found.push(relative.replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

/// A door that runs two scripts, and a model whose piece runs one.
fn door() -> Project {
    let mut project = Project::new("Porta");
    let scene = project.scenes[0].id.clone();
    let mut piece = Entity::new("Peça", Some(Primitive::Cube));
    piece.scripts = vec![ScriptRef::new("scripts/comum/peça.luau")];
    let id = piece.id.clone();
    project.scenes[0].entities.push(piece);
    project.save_model(&scene, &id, "Bloco").unwrap();
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    door.scripts = vec![
        ScriptRef::new("scripts/porta.luau"),
        ScriptRef::new("scripts/comum/som.luau"),
    ];
    project.scenes[0].entities = vec![door];
    validate_project(&project).unwrap();
    project
}

/// What a missing script warns of, after the object and the file.
const STOPS: &str = "; jogar e exportar ficam recusados até que você restaure o arquivo ou remova \
                     o script do objeto";

#[test]
fn a_missing_attached_script_opens_and_saves_with_a_warning_and_stops_the_test() {
    let root = folder("ausentes");
    let file = root.join(persistence::PROJECT_FILE);
    let project = door();
    let missing = |root: &Path| {
        persistence::validate_script_files(&project, root)
            .err()
            .map(|error| error.to_string())
    };
    // The project saves and opens without its files, normally and lazily, with a warning for
    // each, by the object's name and the file; a test that would run them is refused.
    persistence::save_project(&file, &project).unwrap();
    assert_eq!(files(&root), ["project.oxy.json"]);
    assert_eq!(persistence::load_project(&file).unwrap(), project);
    assert_eq!(persistence::load_project_lazy(&file).unwrap(), project);
    assert_eq!(
        persistence::script_warnings(&project, &root),
        [
            format!("Porta: o script scripts/porta.luau não existe na pasta do projeto{STOPS}"),
            format!("Porta: o script scripts/comum/som.luau não existe na pasta do projeto{STOPS}"),
            format!(
                "Modelo Bloco, Peça: o script scripts/comum/peça.luau não existe na pasta do \
                 projeto{STOPS}"
            ),
        ]
    );
    assert_eq!(
        persistence::load_scripts_to_run(&project, &root).unwrap_err(),
        "Porta: o script scripts/porta.luau não existe na pasta do projeto; restaure o arquivo ou \
         remova o script do objeto"
    );
    let error = persistence::validate_script_files(&project, &root).unwrap_err();
    assert!(matches!(error, PersistenceError::ScriptMissing { .. }));
    assert_eq!(
        error.to_string(),
        "Porta: o script scripts/porta.luau não existe na pasta do projeto"
    );
    write(
        &root,
        "scripts/porta.luau",
        "local Porta = {}\nreturn Porta\n",
    );
    // Another extension is another file.
    write(&root, "scripts/comum/som.lua", "");
    write(&root, "scripts/comum/som.luau.txt", "");
    assert_eq!(
        missing(&root).as_deref(),
        Some("Porta: o script scripts/comum/som.luau não existe na pasta do projeto")
    );
    write(&root, "scripts/comum/som.luau", "return {}\n");
    assert_eq!(
        missing(&root).as_deref(),
        Some("Modelo Bloco, Peça: o script scripts/comum/peça.luau não existe na pasta do projeto")
    );
    write(&root, "scripts/comum/peça.luau", "return {}\n");
    assert_eq!(missing(&root), None);
    assert!(persistence::script_warnings(&project, &root).is_empty());
    let sources = persistence::load_scripts_to_run(&project, &root).unwrap();
    assert_eq!(sources.files.len(), 3);
    // A script renamed outside OXY: the project still opens and saves every way, and the
    // warning says which reference broke.
    oxy_core::fs_ops::move_no_replace(
        &root.join("scripts/porta.luau"),
        &root.join("scripts/Porta da frente.luau"),
    )
    .unwrap();
    assert_eq!(persistence::load_project(&file).unwrap(), project);
    assert_eq!(persistence::load_project_lazy(&file).unwrap(), project);
    persistence::save_project(&file, &project).unwrap();
    persistence::save_bundle(&file, &project, &Default::default()).unwrap();
    persistence::save_cached_bundle(&file, &project, &mut TextureCache::default()).unwrap();
    assert_eq!(persistence::load_project(&file).unwrap(), project);
    assert_eq!(
        missing(&root).as_deref(),
        Some("Porta: o script scripts/porta.luau não existe na pasta do projeto")
    );
    assert_eq!(
        persistence::script_warnings(&project, &root),
        [format!(
            "Porta: o script scripts/porta.luau não existe na pasta do projeto{STOPS}"
        )]
    );
    assert!(persistence::load_scripts_to_run(&project, &root).is_err());
    // An edit warns of what it adds: a copy of the door misses the same script.
    let known = persistence::missing_scripts(&project, &root).unwrap();
    let mut copied = project.clone();
    let door_id = copied.scenes[0].entities[0].id.clone();
    let copy = copied.scenes[0].duplicate_subtree(&door_id).unwrap();
    let copy_name = copied.scenes[0].entity(&copy).unwrap().name.clone();
    assert_eq!(
        persistence::added_script_warnings(&known, &copied, &root),
        [format!(
            "{copy_name}: o script scripts/porta.luau não existe na pasta do projeto{STOPS}"
        )]
    );
    assert!(persistence::added_script_warnings(&known, &project, &root).is_empty());
    // One file many objects attach is one line, naming them.
    let both =
        format!("Porta e {copy_name}: o script scripts/porta.luau não existe na pasta do projeto");
    assert_eq!(
        persistence::script_warnings(&copied, &root),
        [format!(
            "{both}; jogar e exportar ficam recusados até que você restaure o arquivo ou remova \
             o script dos objetos"
        )]
    );
    assert_eq!(
        persistence::load_scripts_to_run(&copied, &root).unwrap_err(),
        format!("{both}; restaure o arquivo ou remova o script dos objetos")
    );
    // Without attached scripts, the folder is not even looked for.
    let nowhere = root.join("sem pasta");
    let mut graph_only = project.clone();
    graph_only.scenes[0].entities[0].scripts.clear();
    graph_only.assets.clear();
    persistence::validate_script_files(&graph_only, &nowhere).unwrap();
    assert!(persistence::script_warnings(&graph_only, &nowhere).is_empty());
    assert!(
        persistence::load_scripts_to_run(&graph_only, &nowhere)
            .unwrap()
            .files
            .is_empty()
    );
    assert!(missing(&nowhere).is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn scripts_are_read_from_the_whole_tree_each_file_on_its_own() {
    let root = folder("fontes");
    assert!(persistence::load_scripts(&root).unwrap().files.is_empty());
    write(&root, "scripts/porta.luau", "return {}\n");
    write(&root, "scripts/dados/cartas.luau", "return { \"Ás\" }\n");
    write(&root, "scripts/notas.txt", "não é script");
    write(&root, "scripts/antigo.lua", "return 1");
    write(&root, "outros/fora.luau", "return 1");
    write(&root, ".solucoes/porta.luau", "return 1");
    let sources = persistence::load_scripts(&root).unwrap();
    assert_eq!(
        sources.files.keys().collect::<Vec<_>>(),
        [
            "outros/fora.luau",
            "scripts/dados/cartas.luau",
            "scripts/porta.luau"
        ]
    );
    assert_eq!(
        sources.get("scripts/dados/cartas.luau"),
        Some(&ScriptFile::new("return { \"Ás\" }\n".into()))
    );
    // A file over 1 MB or not UTF-8 is no script, with why; the others are read all the same.
    let big = format!("--{}\n", "a".repeat(1024 * 1024));
    write(&root, "scripts/grande.luau", &big);
    std::fs::write(root.join("scripts/dados/latin1.luau"), b"-- \xe1rvore\n").unwrap();
    write(&root, "Pista/init.luau", "return {}\n");
    let sources = persistence::load_scripts(&root).unwrap();
    assert_eq!(sources.files.len(), 3);
    assert_eq!(
        sources.unreadable,
        [
            (
                "Pista/init.luau".to_owned(),
                "tem um caminho que os scripts não aceitam: \"init.luau\" é o nome que o Luau dá \
                 ao módulo de uma pasta, que os scripts da OXY não usam; dê outro nome ao arquivo"
                    .to_owned()
            ),
            (
                "scripts/dados/latin1.luau".to_owned(),
                "não é texto UTF-8".to_owned()
            ),
            (
                "scripts/grande.luau".to_owned(),
                "tem mais de 1 MB".to_owned()
            ),
        ]
        .into()
    );
    // Only a run that needs one of them stops, saying why.
    let mut project = Project::new("Grande");
    let mut e = Entity::new("Painel", None);
    e.scripts = vec![ScriptRef::new("scripts/porta.luau")];
    project.scenes[0].entities.push(e);
    assert!(persistence::load_scripts_to_run(&project, &root).is_ok());
    project.scenes[0].entities[0]
        .scripts
        .push(ScriptRef::new("scripts/grande.luau"));
    assert_eq!(
        persistence::load_scripts_to_run(&project, &root).unwrap_err(),
        "Painel: o script scripts/grande.luau tem mais de 1 MB; restaure o arquivo ou remova o \
         script do objeto"
    );
    // A file that cannot be read now is not missing: opening warns of none.
    assert!(persistence::script_warnings(&project, &root).is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_copy_takes_every_script_of_the_tree_as_it_is() {
    let source = folder("origem");
    let project = door();
    for path in [
        "scripts/porta.luau",
        "scripts/comum/som.luau",
        "scripts/comum/peça.luau",
        "scripts/dados/cartas.luau",
        "Pista/Checkpoints/marco.luau",
    ] {
        write(&source, path, &format!("-- {path}\nreturn {{}}\n"));
    }
    write(&source, "scripts/notas.txt", "fica");
    write(&source, "scripts/.luaurc", "{}");
    write(&source, ".solucoes/porta.luau", "return {}\n");
    // Bytes go as they are: reading scripts is for the runtime, not for a copy.
    std::fs::write(source.join("scripts/dados/latin1.luau"), b"-- \xe1rvore\n").unwrap();
    let big = format!("--{}\n", "a".repeat(1024 * 1024));
    write(&source, "scripts/grande.luau", &big);
    let file = source.join(persistence::PROJECT_FILE);
    persistence::save_project(&file, &project).unwrap();
    let before = files(&source);
    let output = folder("copia");
    let copy = output.join("nova").join(persistence::PROJECT_FILE);
    let mut images = TextureCache::default();
    images.configure(&source, &project);
    persistence::save_cached_copy(&copy, &project, &mut images, &source).unwrap();
    let copied = output.join("nova");
    assert_eq!(
        files(&copied),
        [
            "Pista/Checkpoints/marco.luau",
            "project.oxy.json",
            "scripts/comum/peça.luau",
            "scripts/comum/som.luau",
            "scripts/dados/cartas.luau",
            "scripts/dados/latin1.luau",
            "scripts/grande.luau",
            "scripts/porta.luau",
        ]
    );
    for path in files(&copied)
        .iter()
        .filter(|path| *path != persistence::PROJECT_FILE)
    {
        assert_eq!(
            std::fs::read(source.join(path)).unwrap(),
            std::fs::read(copied.join(path)).unwrap()
        );
    }
    assert_eq!(persistence::load_project(&copy).unwrap(), project);
    persistence::validate_script_files(&project, &copied).unwrap();
    assert_eq!(files(&source), before);
    // A different file where the copy goes is kept, and nothing is written.
    write(&copied, "scripts/dados/cartas.luau", "-- minha versão\n");
    let saved = std::fs::read(&copy).unwrap();
    let error = persistence::save_cached_copy(&copy, &project, &mut images, &source).unwrap_err();
    assert_eq!(
        error.to_string(),
        "A pasta escolhida já contém um arquivo diferente: scripts/dados/cartas.luau. Escolha \
         uma pasta vazia."
    );
    assert_eq!(
        std::fs::read_to_string(copied.join("scripts/dados/cartas.luau")).unwrap(),
        "-- minha versão\n"
    );
    assert_eq!(std::fs::read(&copy).unwrap(), saved);
    // A script the source lacks does not stop the copy; the copy's check names it.
    std::fs::remove_file(source.join("scripts/comum/som.luau")).unwrap();
    let elsewhere = output.join("outra").join(persistence::PROJECT_FILE);
    persistence::save_cached_copy(&elsewhere, &project, &mut images, &source).unwrap();
    assert_eq!(persistence::load_project(&elsewhere).unwrap(), project);
    assert_eq!(
        persistence::validate_script_files(&project, &output.join("outra"))
            .unwrap_err()
            .to_string(),
        "Porta: o script scripts/comum/som.luau não existe na pasta do projeto"
    );
    std::fs::remove_dir_all(source).unwrap();
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn placed_models_keep_the_scripts_of_their_pieces() {
    let mut project = door();
    let scene = project.scenes[0].id.clone();
    let model = project.assets[0].id.clone();
    let placed = project.instantiate_model(&model, &scene).unwrap();
    let piece = project.scenes[0].entity(&placed).unwrap();
    assert_eq!(piece.scripts, [ScriptRef::new("scripts/comum/peça.luau")]);
    validate_project(&project).unwrap();
}

#[test]
fn graph_only_projects_keep_their_document_and_state_through_format_6() {
    let file = crate::parkour_example::fixture();
    let source = file.parent().unwrap().to_path_buf();
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(original["schema_version"], 5);
    let (project, from) = persistence::load_project_lazy_report(&file).unwrap();
    assert_eq!(from, Some(5));
    assert!(!oxy_core::script::any_attached(&project));
    let output = folder("parkour");
    let copy = output.join(persistence::PROJECT_FILE);
    let mut images = TextureCache::default();
    images.configure(&source, &project);
    persistence::save_cached_copy(&copy, &project, &mut images, &source).unwrap();
    let mut saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&copy).unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 6);
    saved["schema_version"] = 5.into();
    // Format 6 leaves out the empty pieces of images and sounds, and nothing else changes.
    let mut original = original;
    for asset in original["assets"].as_array_mut().unwrap() {
        if asset["model"].is_null() {
            asset.as_object_mut().unwrap().remove("model");
        }
    }
    assert_eq!(saved, original);
    assert!(!output.join("scripts").exists());
    // The copy, opened in format 6, plays the course as 0.6.0 did.
    let (reopened, from) = persistence::load_project_lazy_report(&copy).unwrap();
    assert_eq!((&reopened, from), (&project, None));
    crate::parkour_example::assert_reaches_the_finish(&reopened);
    std::fs::remove_dir_all(output).unwrap();
}
