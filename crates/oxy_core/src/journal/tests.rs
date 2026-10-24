//! The journal of the project's files: paths translated both ways through moves of files and
//! folders, a crash at every point of an operation opened as the disk shows it, a journal of
//! another save set aside, and "Descartar alterações" bringing every file back, a file another
//! program holds told and kept for the next opening.
use super::*;
use crate::{
    document::{Entity, Primitive},
    persistence::{PROJECT_FILE, open_project, save_project_with},
    script::ScriptRef,
};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind, panic_any},
    path::PathBuf,
    sync::Once,
};

/// What a test throws to stop an operation as a crash would.
struct Crash;

/// Keeps the crashes the tests make out of the output; every other panic is told as usual.
fn quiet_crashes() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let told = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !info.payload().is::<Crash>() {
                told(info);
            }
        }));
    });
}

fn moving(from: &str, to: &str) -> Op {
    Op::Move {
        from: from.into(),
        to: to.into(),
    }
}
fn folder(path: &str) -> Op {
    Op::CreateDir { path: path.into() }
}

#[test]
fn paths_follow_moves_of_files_and_folders_both_ways_and_back_to_clean() {
    let mut journal = Journal::new("h".into());
    journal.apply(&folder("Pista"));
    journal.apply(&moving("Sons/Pulo.wav", "Pista/Pulo.wav"));
    assert_eq!(
        journal.now_of("Sons/Pulo.wav").as_deref(),
        Some("Pista/Pulo.wav")
    );
    assert_eq!(
        journal.saved_of("Pista/Pulo.wav").as_deref(),
        Some("Sons/Pulo.wav")
    );
    // Made since: no saved path.
    assert_eq!(journal.saved_of("Pista"), None);
    assert_eq!(journal.saved_of("Pista/Outro.wav"), None);
    // A folder moved takes what it holds; a file moved out of it keeps its own entry.
    journal.apply(&moving("Menu", "Telas"));
    journal.apply(&moving("Telas/fundo.png", "Pista/fundo.png"));
    assert_eq!(
        journal.now_of("Menu/botao.png").as_deref(),
        Some("Telas/botao.png")
    );
    assert_eq!(
        journal.now_of("Menu/fundo.png").as_deref(),
        Some("Pista/fundo.png")
    );
    assert_eq!(
        journal.saved_of("Telas/botao.png").as_deref(),
        Some("Menu/botao.png")
    );
    // A folder moved into a folder made since still knows where it was.
    journal.apply(&moving("Telas", "Pista/Telas"));
    assert_eq!(
        journal.now_of("Menu/botao.png").as_deref(),
        Some("Pista/Telas/botao.png")
    );
    assert_eq!(
        journal.saved_of("Pista/Telas/botao.png").as_deref(),
        Some("Menu/botao.png")
    );
    // The case alone changes too.
    journal.apply(&moving("scripts/porta.luau", "scripts/Porta.luau"));
    assert_eq!(
        journal.now_of("scripts/porta.luau").as_deref(),
        Some("scripts/Porta.luau")
    );
    // A file sent to the trash and a file made since.
    journal.apply(&Op::Trash {
        from: "Sons/Bip.wav".into(),
        slot: ".oxy/lixeira/a-1.wav".into(),
        id: Some("bip".into()),
    });
    assert_eq!(journal.now_of("Sons/Bip.wav"), None);
    journal.apply(&Op::Untrash {
        slot: ".oxy/lixeira/b-1.png".into(),
        to: "Pista/Novo.png".into(),
        id: Some("novo".into()),
    });
    assert_eq!(
        journal.created.get("Pista/Novo.png"),
        Some(&Some("novo".into()))
    );
    assert_eq!(journal.saved_of("Pista/Novo.png"), None);
    let saved: Journal = serde_json::from_slice(&serde_json::to_vec(&journal).unwrap()).unwrap();
    assert_eq!(saved, journal);
    // Undone in reverse, it is clean again.
    journal.apply(&Op::Trash {
        from: "Pista/Novo.png".into(),
        slot: ".oxy/lixeira/b-1.png".into(),
        id: Some("novo".into()),
    });
    journal.apply(&Op::Untrash {
        slot: ".oxy/lixeira/a-1.wav".into(),
        to: "Sons/Bip.wav".into(),
        id: Some("bip".into()),
    });
    journal.apply(&moving("scripts/Porta.luau", "scripts/porta.luau"));
    journal.apply(&moving("Pista/Telas", "Telas"));
    journal.apply(&moving("Pista/fundo.png", "Telas/fundo.png"));
    journal.apply(&moving("Telas", "Menu"));
    journal.apply(&moving("Pista/Pulo.wav", "Sons/Pulo.wav"));
    journal.apply(&Op::RemoveDir {
        path: "Pista".into(),
    });
    assert!(journal.is_clean(), "{journal:?}");
}

#[test]
fn a_folder_removed_and_made_again_and_a_name_reused_stay_apart() {
    let mut journal = Journal::new("h".into());
    // Deleting a folder: what it holds goes up to the root, then the empty folder goes.
    journal.apply(&moving("Menu/fundo.png", "fundo.png"));
    journal.apply(&Op::RemoveDir {
        path: "Menu".into(),
    });
    assert_eq!(journal.removed_folders, BTreeSet::from(["Menu".to_owned()]));
    journal.apply(&folder("Menu"));
    assert!(journal.removed_folders.is_empty() && journal.created_folders.is_empty());
    // A folder moved away and a new one made at its name.
    journal.apply(&moving("Sons", "Audio"));
    journal.apply(&folder("Sons"));
    assert_eq!(
        journal.now_of("Sons/Pulo.wav").as_deref(),
        Some("Audio/Pulo.wav")
    );
    assert_eq!(journal.saved_of("Sons/novo.wav"), None);
    journal.apply(&moving("fundo.png", "Sons/fundo.png"));
    assert_eq!(
        journal.saved_of("Sons/fundo.png").as_deref(),
        Some("Menu/fundo.png")
    );
}

fn temp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-diario-{label}-{}", new_id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

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

/// Every file under `root` but OXY's own folder, by its path with `/`, with its bytes.
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

/// A project of the tree layout saved in a folder of its own: the sounds `Sons/Pulo.wav` and
/// `Sons/Bip.wav`, the folder `Menu` with `Menu/Clique.wav`, and `scripts/porta.luau` attached
/// to the object Porta.
fn saved(label: &str) -> (PathBuf, Project) {
    let root = temp(label);
    put(&root, "Sons/Pulo.wav", &wav(4));
    put(&root, "Sons/Bip.wav", &wav(6));
    put(&root, "Menu/Clique.wav", &wav(8));
    put(&root, "scripts/porta.luau", b"return {}\n");
    let mut project = Project::new("Diário");
    project.layout = crate::document::Layout::Tree;
    for (name, path) in [
        ("Pulo", "Sons/Pulo.wav"),
        ("Bip", "Sons/Bip.wav"),
        ("Clique", "Menu/Clique.wav"),
    ] {
        project.assets.push(Asset {
            id: new_id(),
            name: name.into(),
            path: path.into(),
            kind: AssetKind::Audio,
            model: None,
        });
    }
    let mut door = Entity::new("Porta", Some(Primitive::Cube));
    door.scripts.push(ScriptRef::new("scripts/porta.luau"));
    project.scenes[0].entities.push(door);
    save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
    (root, project)
}

/// The path of the sound `name` of `saved` (the project as saved) and of the script of Porta in
/// `project`.
fn paths(project: &Project, saved: &Project, name: &str) -> (String, String) {
    let id = &saved
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .unwrap()
        .id;
    let sound = project.asset(id).unwrap().path.clone();
    let script = project.scenes[0].entities[0].scripts[0].path.clone();
    (sound, script)
}

#[test]
fn a_crash_at_every_point_of_an_operation_opens_as_the_disk_shows() {
    quiet_crashes();
    let ops = [
        folder("Pista"),
        moving("Sons/Pulo.wav", "Pista/Pulo.wav"),
        moving("scripts/porta.luau", "Pista/porta.luau"),
    ];
    let steps = [
        Some(Step::Intent),
        Some(Step::Op(0)),
        Some(Step::Op(1)),
        Some(Step::Op(2)),
        Some(Step::Done),
        None,
    ];
    for at in steps {
        let (root, start) = saved("queda");
        let (_, mut report) = open_project(&root).unwrap();
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            run_with(&root, &mut report.known, "Mover", &ops, &mut |step| {
                if Some(step) == at {
                    panic_any(Crash);
                }
            })
        }));
        assert_eq!(crashed.is_err(), at.is_some(), "{at:?}");
        let (project, report) = open_project(&root).unwrap();
        let (sound, script) = paths(&project, &start, "Pulo");
        let moved = root.join("Pista/Pulo.wav").exists();
        assert_eq!(
            sound,
            if moved {
                "Pista/Pulo.wav"
            } else {
                "Sons/Pulo.wav"
            },
            "{at:?}"
        );
        let moved = root.join("Pista/porta.luau").exists();
        assert_eq!(
            script,
            if moved {
                "Pista/porta.luau"
            } else {
                "scripts/porta.luau"
            },
            "{at:?}"
        );
        assert!(report.warnings.is_empty(), "{at:?}: {:?}", report.warnings);
        let interrupted = report.fixes.iter().any(|fix| fix.contains("interrompida"));
        assert_eq!(interrupted, at.is_some(), "{at:?}");
        // The journal goes from the disk now: a save writes it and takes it away.
        let mut known = report.known.clone();
        save_project_with(&root, &project, &mut known).unwrap();
        assert!(!root.join(JOURNAL).exists(), "{at:?}");
        let (again, report) = open_project(&root).unwrap();
        assert_eq!(again, project, "{at:?}");
        assert_eq!(report.lines().count(), 0, "{at:?}: {:?}", report.warnings);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_crash_in_a_batch_that_moves_a_file_again_opens_as_the_disk_shows_both_ways() {
    quiet_crashes();
    // A sound moved then renamed, and a folder made, filled and renamed: a later operation
    // moves again the file of an earlier one.
    let ops = [
        folder("Pista"),
        moving("Sons/Pulo.wav", "Pista/Pulo.wav"),
        moving("Pista/Pulo.wav", "Pista/Salto.wav"),
        folder("A"),
        moving("Menu/Clique.wav", "A/Clique.wav"),
        moving("A", "B"),
    ];
    let back: Vec<Op> = ops.iter().rev().map(Op::inverse).collect();
    let places = [
        (
            "Pulo",
            ["Sons/Pulo.wav", "Pista/Pulo.wav", "Pista/Salto.wav"],
        ),
        (
            "Clique",
            ["Menu/Clique.wav", "A/Clique.wav", "B/Clique.wav"],
        ),
    ];
    for undoing in [false, true] {
        let batch = if undoing { &back[..] } else { &ops[..] };
        let steps = (0..batch.len())
            .map(Step::Op)
            .chain([Step::Intent, Step::Done]);
        for at in steps {
            let (root, start) = saved("encadeado");
            let saved_files = files(&root);
            let (_, mut report) = open_project(&root).unwrap();
            if undoing {
                run(&root, &mut report.known, "Mover", &ops).unwrap();
            }
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                run_with(&root, &mut report.known, "Mover", batch, &mut |step| {
                    if step == at {
                        panic_any(Crash);
                    }
                })
            }));
            assert!(crashed.is_err(), "{undoing} {at:?}");
            let (project, report) = open_project(&root).unwrap();
            assert!(
                report.warnings.is_empty(),
                "{undoing} {at:?}: {:?}",
                report.warnings
            );
            // Each sound, by the ID the saved project gave it, is where its file is.
            for (name, places) in places {
                let (sound, _) = paths(&project, &start, name);
                let there: Vec<&str> = places
                    .into_iter()
                    .filter(|place| root.join(place).is_file())
                    .collect();
                assert_eq!(there, [sound.as_str()], "{undoing} {at:?}");
            }
            // The journal says how to go back: discarding gets the saved files again.
            let mut known = report.known.clone();
            let discarded = discard(&root, &mut known);
            assert!(discarded.kept.is_empty(), "{undoing} {at:?}: {discarded:?}");
            assert_eq!(files(&root), saved_files, "{undoing} {at:?}");
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn an_import_journaled_before_its_file_opens_with_its_asset_after_a_crash() {
    quiet_crashes();
    for write_file in [false, true] {
        let (root, _) = saved("importar");
        let (_, mut report) = open_project(&root).unwrap();
        let id = new_id();
        let crashed = catch_unwind(AssertUnwindSafe(|| {
            let mut creating = Creating::new(&root, &mut report.known, "Importar recurso");
            creating.before("Sons/Novo.wav", &id).unwrap();
            if write_file {
                put(&root, "Sons/Novo.wav", &wav(10));
            }
            panic_any(Crash);
        }));
        assert!(crashed.is_err());
        let (project, report) = open_project(&root).unwrap();
        let found = project.asset(&id);
        assert_eq!(found.is_some(), write_file);
        if let Some(asset) = found {
            assert_eq!(
                (asset.name.as_str(), asset.path.as_str()),
                ("Novo", "Sons/Novo.wav")
            );
            assert_eq!(
                report.known.journal().created.get("Sons/Novo.wav"),
                Some(&Some(id.clone()))
            );
        }
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_journal_of_another_save_is_set_aside_and_the_project_opens_as_saved() {
    let (root, project) = saved("outro");
    let (_, mut report) = open_project(&root).unwrap();
    run(
        &root,
        &mut report.known,
        "Mover",
        &[moving("Sons/Pulo.wav", "Pulo.wav")],
    )
    .unwrap();
    assert!(root.join(JOURNAL).exists());
    // Saved another way meanwhile (an older OXY, by hand): the journal is not about it.
    let mut other = project.clone();
    other.name = "Gravado por fora".into();
    let bytes = serde_json::to_vec_pretty(&other).unwrap();
    fs::write(root.join(PROJECT_FILE), bytes).unwrap();
    let (opened, report) = open_project(&root).unwrap();
    assert!(!root.join(JOURNAL).exists());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("ficou em .oxy/pendente.")),
        "{:?}",
        report.warnings
    );
    // Its rules hold: the file the project file names went, and nothing used it; the file at
    // the root is a new one.
    let pulo = &project.assets[0];
    assert!(opened.asset(&pulo.id).is_none());
    assert!(opened.assets.iter().any(|asset| asset.path == "Pulo.wav"));
    let aside = fs::read_dir(root.join(".oxy"))
        .unwrap()
        .filter(|entry| {
            let name = entry.as_ref().unwrap().file_name();
            name.to_string_lossy().starts_with("pendente.")
        })
        .count();
    assert_eq!(aside, 1);
    fs::remove_dir_all(root).unwrap();
}

/// Moves, deletes and imports in the project at `root` that "Descartar" undoes: a folder made
/// with a sound moved in, a folder moved, a sound sent to the trash, two sounds whose names
/// were swapped, a sound imported, and the scripts' folder deleted (its script up at the root).
fn change_everything(root: &Path, known: &mut KnownFiles) {
    for op in [
        folder("Pista"),
        moving("Sons/Pulo.wav", "Pista/Pulo.wav"),
        moving("Menu", "Pista/Menu"),
        Op::Trash {
            from: "Sons/Bip.wav".into(),
            slot: ".oxy/lixeira/t-1.wav".into(),
            id: None,
        },
        moving("Pista/Menu/Clique.wav", "Pista/Menu/Troca.wav"),
        moving("Pista/Pulo.wav", "Pista/Menu/Clique.wav"),
        moving("Pista/Menu/Troca.wav", "Pista/Pulo.wav"),
        moving("scripts/porta.luau", "porta.luau"),
        Op::RemoveDir {
            path: "scripts".into(),
        },
    ] {
        run(root, known, "Mudar", std::slice::from_ref(&op)).unwrap();
    }
    let mut creating = Creating::new(root, known, "Importar recurso");
    creating.before("Sons/Novo.wav", &new_id()).unwrap();
    put(root, "Sons/Novo.wav", &wav(12));
    assert_eq!(creating.finish(true).len(), 1);
}

#[test]
fn discarding_brings_back_what_was_moved_deleted_and_imported() {
    let (root, project) = saved("descartar");
    let start = files(&root);
    let (_, mut report) = open_project(&root).unwrap();
    change_everything(&root, &mut report.known);
    assert_ne!(files(&root), start);
    // Opened before discarding, the project has every file where it is now.
    let (opened, told) = open_project(&root).unwrap();
    assert!(told.warnings.is_empty(), "{:?}", told.warnings);
    // Pulo and Clique swapped names: each asset is named after its file.
    assert_eq!(
        paths(&opened, &project, "Pulo"),
        ("Pista/Menu/Clique.wav".into(), "porta.luau".into())
    );
    assert_eq!(paths(&opened, &project, "Clique").0, "Pista/Pulo.wav");
    assert!(opened.assets.iter().any(|asset| asset.name == "Novo"));
    assert!(opened.assets.iter().all(|asset| asset.name != "Bip"));
    let discarded = discard(&root, &mut report.known);
    assert!(discarded.kept.is_empty(), "{:?}", discarded.kept);
    assert!(discarded.restored >= 8, "{discarded:?}");
    assert_eq!(files(&root), start);
    assert!(!root.join(JOURNAL).exists());
    let (reopened, report) = open_project(&root).unwrap();
    assert_eq!(reopened, project);
    assert_eq!(report.lines().count(), 0, "{:?}", report.warnings);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn a_file_another_program_holds_stays_told_and_the_next_opening_finds_it() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, project) = saved("descartar-preso");
    let start = files(&root);
    let (_, mut report) = open_project(&root).unwrap();
    run(
        &root,
        &mut report.known,
        "Mover",
        &[folder("Pista"), moving("Sons/Pulo.wav", "Pista/Pulo.wav")],
    )
    .unwrap();
    run(
        &root,
        &mut report.known,
        "Mover",
        &[moving("Sons/Bip.wav", "Pista/Bip.wav")],
    )
    .unwrap();
    // Open with no sharing, as an audio editor holds it.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(root.join("Pista/Pulo.wav"))
        .unwrap();
    let discarded = discard(&root, &mut report.known);
    assert_eq!(discarded.kept.len(), 2, "{discarded:?}");
    assert!(
        discarded.kept[0].starts_with("Pista/Pulo.wav não voltou para Sons/Pulo.wav")
            && discarded.kept[0].contains("aberto em outro programa"),
        "{:?}",
        discarded.kept
    );
    assert!(
        discarded.kept[1].starts_with("a pasta Pista"),
        "{:?}",
        discarded.kept
    );
    assert!(root.join("Sons/Bip.wav").exists());
    assert!(root.join(JOURNAL).exists());
    drop(held);
    // The journal keeps what stayed: the next opening finds the sound where it is.
    let (opened, told) = open_project(&root).unwrap();
    assert_eq!(paths(&opened, &project, "Pulo").0, "Pista/Pulo.wav");
    assert_eq!(paths(&opened, &project, "Bip").0, "Sons/Bip.wav");
    assert!(told.warnings.is_empty(), "{:?}", told.warnings);
    // Let go, discarding again finishes.
    let mut known = told.known.clone();
    let discarded = discard(&root, &mut known);
    assert!(discarded.kept.is_empty(), "{discarded:?}");
    assert_eq!(files(&root), start);
    let (reopened, _) = open_project(&root).unwrap();
    assert_eq!(reopened, project);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn undoing_checks_the_disk_first() {
    let (root, _) = saved("conferir");
    let (_, mut report) = open_project(&root).unwrap();
    let ops = [folder("Pista"), moving("Sons/Pulo.wav", "Pista/Pulo.wav")];
    run(&root, &mut report.known, "Mover", &ops).unwrap();
    let back: Vec<Op> = ops.iter().rev().map(Op::inverse).collect();
    assert_eq!(check(&root, &report.known, &back), Ok(()));
    // A file someone put where the sound goes back.
    put(&root, "Sons/Pulo.wav", b"outro");
    assert_eq!(
        check(&root, &report.known, &back),
        Err(Blocked::Changed("já existe Sons/Pulo.wav".into()))
    );
    fs::remove_file(root.join("Sons/Pulo.wav")).unwrap();
    // The sound changed outside OXY since OXY moved it.
    put(&root, "Pista/Pulo.wav", &wav(40));
    assert_eq!(
        check(&root, &report.known, &back),
        Err(Blocked::Changed("Pista/Pulo.wav mudou fora da OXY".into()))
    );
    // Written by OXY itself, its stamp is the new one.
    report.known.note_stamp(&root, "Pista/Pulo.wav");
    assert_eq!(check(&root, &report.known, &back), Ok(()));
    // A script edited after OXY moved it goes back as it is: OXY keeps nothing of what it says.
    let script = moving("scripts/porta.luau", "Pista/porta.luau");
    run(
        &root,
        &mut report.known,
        "Mover",
        std::slice::from_ref(&script),
    )
    .unwrap();
    put(&root, "Pista/porta.luau", b"return { editado = true }\n");
    assert_eq!(check(&root, &report.known, &[script.inverse()]), Ok(()));
    // A file the trash lost.
    let trash = [Op::Untrash {
        slot: ".oxy/lixeira/x-1.wav".into(),
        to: "Sons/Bip2.wav".into(),
        id: None,
    }];
    assert!(matches!(
        check(&root, &report.known, &trash),
        Err(Blocked::TrashEmptied(_))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn names_swapped_in_a_cycle_go_back() {
    let (root, project) = saved("troca");
    let start = files(&root);
    let (_, mut report) = open_project(&root).unwrap();
    for op in [
        moving("Sons/Pulo.wav", "Sons/Troca.wav"),
        moving("Sons/Bip.wav", "Sons/Pulo.wav"),
        moving("Sons/Troca.wav", "Sons/Bip.wav"),
    ] {
        run(
            &root,
            &mut report.known,
            "Renomear",
            std::slice::from_ref(&op),
        )
        .unwrap();
    }
    assert_eq!(report.known.journal().moved.len(), 2);
    let discarded = discard(&root, &mut report.known);
    assert!(discarded.kept.is_empty(), "{discarded:?}");
    assert_eq!(files(&root), start);
    assert!(!root.join(JOURNAL).exists());
    let (reopened, _) = open_project(&root).unwrap();
    assert_eq!(reopened, project);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discarding_waits_for_the_folder_it_puts_things_back_into_and_always_ends() {
    // The empty folder A/B deleted, A renamed to C and a new A made: B comes back only into the
    // A that was saved, once the new one is out of its way.
    let (root, _) = saved("descartar-pasta");
    fs::create_dir_all(root.join("A/B")).unwrap();
    let start = files(&root);
    let changes = [
        Op::RemoveDir { path: "A/B".into() },
        moving("A", "C"),
        folder("A"),
    ];
    let (_, mut report) = open_project(&root).unwrap();
    for op in &changes {
        run(&root, &mut report.known, "Mudar", std::slice::from_ref(op)).unwrap();
    }
    let discarded = discard(&root, &mut report.known);
    assert!(discarded.kept.is_empty(), "{discarded:?}");
    assert_eq!(files(&root), start);
    assert!(!root.join(JOURNAL).exists());
    // With a file someone put in the new A, nothing can go back: discarding ends all the same,
    // saying what stayed.
    for op in &changes {
        run(&root, &mut report.known, "Mudar", std::slice::from_ref(op)).unwrap();
    }
    put(&root, "A/nota.txt", b"nota");
    let discarded = discard(&root, &mut report.known);
    assert_eq!(discarded.restored, 0, "{discarded:?}");
    assert_eq!(discarded.kept.len(), 3, "{discarded:?}");
    assert!(
        discarded
            .kept
            .iter()
            .any(|line| line.starts_with("a pasta A/B não foi refeita: a pasta A não está como")),
        "{discarded:?}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_file_in_the_trash_that_only_a_models_piece_uses_opens_missing() {
    let root = temp("lixeira-modelo");
    let image = crate::painting::PaintImage::new(2, 2, [200, 40, 40, 255])
        .unwrap()
        .to_png()
        .unwrap();
    put(&root, "Imagens/Pedra.png", &image);
    let mut project = Project::new("Lixeira");
    project.layout = crate::document::Layout::Tree;
    let stone = new_id();
    project.assets.push(Asset {
        id: stone.clone(),
        name: "Pedra".into(),
        path: "Imagens/Pedra.png".into(),
        kind: AssetKind::Texture,
        model: None,
    });
    let scene = project.start_scene.clone();
    let group = Entity::new("Torre", None);
    let mut block = Entity::new("Bloco", Some(Primitive::Cube));
    block.parent = Some(group.id.clone());
    block.material.texture = Some(stone.clone());
    let group_id = group.id.clone();
    project.scenes[0].entities.extend([group, block]);
    let model = project.save_model(&scene, &group_id, "Torre").unwrap();
    // Only the model's piece shows the image.
    project.scenes[0].entities.clear();
    save_project_with(&root, &project, &mut KnownFiles::default()).unwrap();
    let (_, mut report) = open_project(&root).unwrap();
    let trash = Op::Trash {
        from: "Imagens/Pedra.png".into(),
        slot: ".oxy/lixeira/t-1.png".into(),
        id: Some(stone.clone()),
    };
    run(&root, &mut report.known, "Excluir recurso", &[trash]).unwrap();
    // OXY stopped before saving: the image stays, missing, and the model still fits.
    let (opened, report) = open_project(&root).unwrap();
    assert!(opened.asset(&stone).is_some());
    assert_eq!(
        report.assets.get(&stone),
        Some(&persistence::AssetState::Missing)
    );
    assert!(opened.asset(&model).unwrap().model.is_some());
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(
        report.warnings[0].contains("foi para a lixeira do projeto"),
        "{:?}",
        report.warnings
    );
    fs::remove_dir_all(root).unwrap();
}

/// Holds the files `paths` of the project at `root` open with no sharing, as an audio editor
/// would, once the next [`run`] on this thread is about to do its operation `at`; taking
/// [`HOOK`] lets them go.
#[cfg(windows)]
fn hold_at(root: &Path, at: usize, paths: &[&str]) {
    use std::os::windows::fs::OpenOptionsExt;
    let files: Vec<PathBuf> = paths.iter().map(|path| root.join(path)).collect();
    let mut held = Vec::new();
    HOOK.set(Some(Box::new(move |step| {
        if step == Step::Op(at) && held.is_empty() {
            for file in &files {
                let open = fs::OpenOptions::new().read(true).share_mode(0).open(file);
                held.push(open.unwrap());
            }
        }
    })));
}

#[cfg(windows)]
#[test]
fn files_that_stay_moved_when_putting_them_back_fails_too_are_a_step_the_project_follows() {
    use crate::{
        command,
        edit_history::{CommandHistory, FileOp},
        texture_cache::TextureCache,
    };
    let (root, saved_project) = saved("metade");
    let start = files(&root);
    let (mut project, mut report) = open_project(&root).unwrap();
    let mut images = TextureCache::default();
    let mut history = CommandHistory::new();
    let scene = project.start_scene.clone();
    let commands = command::parse(
        &serde_json::json!([{"op": "move", "items": ["Pulo", "Bip"], "to": "Menu"}]),
    )
    .unwrap();
    let where_now = |project: &Project| {
        (
            paths(project, &saved_project, "Pulo").0,
            paths(project, &saved_project, "Bip").0,
        )
    };
    // Bip is held as its turn comes, and so is Pulo where it went: the batch fails, and putting
    // Pulo back too. The project follows Pulo, and the step keeps its move.
    hold_at(&root, 1, &["Sons/Bip.wav", "Menu/Pulo.wav"]);
    history.begin("Mover", &project, &images);
    let error = command::apply_on_files(
        &mut project,
        &scene,
        &commands,
        &root,
        &mut report.known,
        "Mover",
    )
    .unwrap_err();
    drop(HOOK.take());
    assert!(
        !error.message.contains("Nada foi alterado")
            && error.message.contains("o projeto acompanha"),
        "{}",
        error.message
    );
    assert_eq!(
        error.kept,
        [FileOp::Move {
            from: "Sons/Pulo.wav".into(),
            to: "Menu/Pulo.wav".into(),
        }]
    );
    assert_eq!(
        where_now(&project),
        ("Menu/Pulo.wav".into(), "Sons/Bip.wav".into())
    );
    // The journal has it too: opening finds Pulo where it is.
    let (opened, _) = open_project(&root).unwrap();
    assert_eq!(where_now(&opened), where_now(&project));
    history
        .commit_with_files(&project, &mut images, error.kept)
        .unwrap();
    history
        .undo_with(&mut project, &mut images, Some((&root, &mut report.known)))
        .unwrap();
    assert_eq!(files(&root), start);
    // Undoing the whole batch: Bip goes back, Pulo is held, and putting Bip there again fails
    // too. Bip's way back is a step of its own, which undoing puts back.
    history.begin("Mover", &project, &images);
    let done = command::apply_on_files(
        &mut project,
        &scene,
        &commands,
        &root,
        &mut report.known,
        "Mover",
    )
    .unwrap();
    history
        .commit_with_files(&project, &mut images, done.files)
        .unwrap();
    hold_at(&root, 1, &["Menu/Pulo.wav", "Sons/Bip.wav"]);
    let error = history
        .undo_with(&mut project, &mut images, Some((&root, &mut report.known)))
        .unwrap_err();
    drop(HOOK.take());
    assert!(
        error.contains("Desfazer incompleto: Mover") && error.contains("o projeto acompanha"),
        "{error}"
    );
    assert_eq!(history.undo_len(), 2);
    assert_eq!(
        where_now(&project),
        ("Menu/Pulo.wav".into(), "Sons/Bip.wav".into())
    );
    history
        .undo_with(&mut project, &mut images, Some((&root, &mut report.known)))
        .unwrap();
    assert_eq!(
        where_now(&project),
        ("Menu/Pulo.wav".into(), "Menu/Bip.wav".into())
    );
    history
        .undo_with(&mut project, &mut images, Some((&root, &mut report.known)))
        .unwrap();
    assert_eq!(files(&root), start);
    assert_eq!(
        where_now(&project),
        ("Sons/Pulo.wav".into(), "Sons/Bip.wav".into())
    );
    assert!(report.known.journal().is_clean());
    fs::remove_dir_all(root).unwrap();
}
