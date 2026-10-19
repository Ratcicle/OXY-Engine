//! An exported game checks itself with its own executable, as people get it: no window, no
//! editor, no Cargo.
use oxy_core::{
    document::{Entity, Project, Value, new_id},
    export::{self, ExportRequest, VerifyReport},
    persistence,
    script::ScriptRef,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

/// Runs the exported `exe` with `--verificar`, answering whether it passed and its report.
fn check(exe: &Path, report: &Path) -> (bool, VerifyReport) {
    let status = Command::new(exe)
        .arg("--verificar")
        .arg(report)
        .status()
        .unwrap();
    let report = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    (status.success(), report)
}

#[test]
fn the_exported_example_checks_itself_and_names_a_missing_file() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let example = workspace.join("examples/validacao");
    let project = persistence::load_project(&example).unwrap();
    let base = std::env::temp_dir().join(format!("oxy-exported-game-{}", new_id()));
    let destination = base.join("Jogos");
    let player = PathBuf::from(env!("CARGO_BIN_EXE_oxy_player"));
    let exported = export::export(&ExportRequest {
        project: &project,
        source_root: Some(&example),
        unsaved_png: &HashMap::new(),
        player: &player,
        destination: &destination,
        replace: false,
        protected: &[],
    })
    .unwrap();
    let name = export::file_name(project.game_title());
    assert_eq!(exported.folder, destination.join(&name));
    assert_eq!(
        exported.executable.file_name().unwrap().to_string_lossy(),
        format!("{name}{}", std::env::consts::EXE_SUFFIX)
    );
    assert!(exported.verify.ok, "{:?}", exported.verify);
    assert_eq!(exported.verify.engine, export::engine_version());

    // Run again by hand, as people get it.
    let report = base.join("conferencia.json");
    let (passed, checked) = check(&exported.executable, &report);
    assert!(passed && checked.ok, "{checked:?}");
    assert_eq!(checked.engine, export::engine_version());
    assert_eq!(checked.game, project.game_title());
    assert_eq!(
        checked.start_scene.as_deref(),
        project.scene(&project.start_scene).map(|s| s.name.as_str())
    );
    assert!(checked.seconds > 0.);

    // A file taken from the game fails the check, by the asset's name.
    let sound = project
        .assets
        .iter()
        .find(|asset| asset.path == "assets/impacto_local.wav")
        .unwrap();
    std::fs::remove_file(exported.folder.join("data").join(&sound.path)).unwrap();
    let (passed, checked) = check(&exported.executable, &report);
    assert!(!passed && !checked.ok, "{checked:?}");
    assert!(
        checked
            .errors
            .iter()
            .any(|error| error.contains(&sound.name)),
        "{checked:?}"
    );
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn an_exported_game_runs_its_scripts_and_names_a_missing_one() {
    let base = std::env::temp_dir().join(format!("oxy-exported-scripts-{}", new_id()));
    let folder = base.join("Projeto");
    std::fs::create_dir_all(folder.join("scripts")).unwrap();
    std::fs::write(
        folder.join("scripts/porta.luau"),
        "local M = {}\nfunction M.on_start(self) print('aberta', self.attr.Vezes) end\nreturn M\n",
    )
    .unwrap();
    let mut project = Project::new("Porta com script");
    let mut door = Entity::new("Porta", None);
    door.attributes.insert("Vezes".into(), Value::Number(1.));
    door.scripts = vec![ScriptRef::new("scripts/porta.luau")];
    project.scenes[0].entities.push(door);
    persistence::save_project(&folder.join(persistence::PROJECT_FILE), &project).unwrap();
    let destination = base.join("Jogos");
    let player = PathBuf::from(env!("CARGO_BIN_EXE_oxy_player"));
    let exported = export::export(&ExportRequest {
        project: &project,
        source_root: Some(&folder),
        unsaved_png: &HashMap::new(),
        player: &player,
        destination: &destination,
        replace: false,
        protected: &[],
    })
    .unwrap();
    assert!(exported.verify.ok, "{:?}", exported.verify);
    assert!(
        exported.folder.join("data/scripts/porta.luau").is_file(),
        "{exported:?}"
    );

    // Its own executable, run by hand, runs the script.
    let report = base.join("conferencia.json");
    let (passed, checked) = check(&exported.executable, &report);
    assert!(passed && checked.ok, "{checked:?}");
    assert!(
        checked
            .logs
            .iter()
            .any(|line| line.ends_with("Porta · aberta 1")),
        "{checked:?}"
    );

    // A script taken from the game fails the check, by the object and the file.
    std::fs::remove_file(exported.folder.join("data/scripts/porta.luau")).unwrap();
    let (passed, checked) = check(&exported.executable, &report);
    assert!(!passed && !checked.ok, "{checked:?}");
    assert!(
        checked
            .errors
            .iter()
            .any(|error| error
                == "Porta: o script scripts/porta.luau não existe na pasta do projeto"),
        "{checked:?}"
    );
    std::fs::remove_dir_all(base).unwrap();
}
