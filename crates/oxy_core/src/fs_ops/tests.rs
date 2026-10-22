//! The Windows probe of the project tree's design (moves that would replace, case, dots and
//! spaces at the end, device names, NFC and NFD, open files and folders, long paths) and the
//! operations made on it.
use super::*;
use crate::{document::new_id, persistence::PersistenceError};

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oxy-fs-{}", new_id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
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
                pending.push((entry.path(), format!("{name}/")));
            } else {
                found.insert(name, fs::read(entry.path()).unwrap());
            }
        }
    }
    found
}
fn taken(result: io::Result<impl Sized>) -> bool {
    result.is_err_and(|error| error.kind() == io::ErrorKind::AlreadyExists)
}
fn moving(from: &str, to: &str) -> Op {
    Op::Move {
        from: from.into(),
        to: to.into(),
    }
}

#[test]
fn the_tree_modules_never_write_over_files() {
    for (module, source) in [
        ("fs_ops.rs", include_str!("../fs_ops.rs")),
        ("names.rs", include_str!("../names.rs")),
        ("project_tree.rs", include_str!("../project_tree.rs")),
    ] {
        for forbidden in ["fs::write", "File::create", ".create(true)"] {
            assert!(!source.contains(forbidden), "{module} uses {forbidden}");
        }
    }
    let clippy = include_str!("../../../../clippy.toml");
    for method in ["std::fs::rename", "std::fs::copy"] {
        assert!(
            clippy.contains(&format!("path = \"{method}\"")),
            "clippy.toml forbids {method}"
        );
    }
}

#[cfg(windows)]
#[test]
fn moves_never_replace_a_file_another_case_or_an_empty_folder() {
    let dir = temp();
    fs::write(dir.join("a.wav"), b"a").unwrap();
    fs::write(dir.join("b.wav"), b"b").unwrap();
    fs::write(dir.join("Pulo.wav"), b"pulo").unwrap();
    fs::create_dir(dir.join("A")).unwrap();
    fs::create_dir(dir.join("B")).unwrap();
    let before = files(&dir);
    for (from, to) in [
        ("a.wav", "b.wav"),
        ("a.wav", "PULO.WAV"),
        ("a.wav", "pulo.wav"),
        ("A", "B"),
        ("A", "b"),
        ("a.wav", "B"),
        ("A", "a.wav"),
    ] {
        let error = move_no_replace(&dir.join(from), &dir.join(to)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{from} -> {to}");
        assert!(matches!(error.raw_os_error(), Some(80 | 183)), "{error}");
    }
    assert_eq!(files(&dir), before);
    assert_eq!(listing(&dir), ["A", "B", "Pulo.wav", "a.wav", "b.wav"]);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn a_change_of_case_alone_is_one_step_for_files_and_folders() {
    let dir = temp();
    fs::write(dir.join("pulo.wav"), b"pulo").unwrap();
    move_no_replace(&dir.join("pulo.wav"), &dir.join("Pulo.wav")).unwrap();
    assert_eq!(listing(&dir), ["Pulo.wav"]);
    assert_eq!(names::disk_case(&dir, "pulo.wav").unwrap(), "Pulo.wav");
    fs::create_dir(dir.join("pista")).unwrap();
    fs::write(dir.join("pista").join("x.wav"), b"x").unwrap();
    move_no_replace(&dir.join("pista"), &dir.join("Pista")).unwrap();
    assert_eq!(listing(&dir), ["Pista", "Pulo.wav"]);
    assert_eq!(
        names::disk_case(&dir, "PISTA/X.WAV").unwrap(),
        "Pista/x.wav"
    );
    // Through a batch, as the tree renames.
    batch(&dir, &[moving("Pulo.wav", "PULO.wav")]).unwrap();
    assert_eq!(listing(&dir), ["PULO.wav", "Pista"]);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn names_windows_reads_otherwise_are_why_the_rules_refuse_them() {
    let dir = temp();
    // A dot at the end is cut without the \\?\ form: two names, one file.
    create_new_file(&dir.join("Ponto."), b"ponto").unwrap();
    assert_eq!(listing(&dir), ["Ponto"]);
    assert!(taken(create_new_file(&dir.join("Ponto"), b"outro")));
    assert!(taken(create_new_file(&dir.join("Ponto "), b"outro")));
    // With it, `Pista.` is a folder apart from `Pista`.
    fs::create_dir(dir.join("Pista")).unwrap();
    let verbatim = canonical(&dir).unwrap().join("Pista.");
    fs::create_dir(&verbatim).unwrap();
    assert_eq!(listing(&dir), ["Pista", "Pista.", "Ponto"]);
    assert!(matches!(
        names::resolve(&dir, "Pista./x.wav"),
        Err(PersistenceError::InvalidPath { .. })
    ));
    assert!(names::resolve(&dir, "Pista/x.wav").is_ok());
    fs::remove_dir(&verbatim).unwrap();
    // NUL is the null device: what is written there is lost.
    let _ = create_new_file(&dir.join("NUL"), b"perdido");
    assert_eq!(listing(&dir), ["Pista", "Ponto"]);
    for name in [
        "NUL",
        "nul.png",
        "CON",
        "COM0.wav",
        "CONIN$.wav",
        "lpt¹.png",
    ] {
        assert!(names::resolve(&dir, name).is_err(), "{name}");
        assert!(names::check_name(name).is_err(), "{name}");
    }
    // The same accented name typed whole (NFC) and with the accent apart (NFD) are two files
    // that look alike.
    create_new_file(&dir.join("Música.wav"), b"nfc").unwrap();
    create_new_file(&dir.join("Mu\u{301}sica.wav"), b"nfd").unwrap();
    assert_eq!(listing(&dir).len(), 4);
    assert!(!names::same_name("Música", "Mu\u{301}sica"));
    assert!(names::check_name("Mu\u{301}sica").is_err());
    assert_eq!(names::fitted_name("Mu\u{301}sica", "Som"), "Música");
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn only_creating_refuses_another_case_and_a_short_name() {
    let dir = temp();
    create_new_file(&dir.join("σ.wav"), b"sigma").unwrap();
    fs::write(dir.join("x.wav"), b"x").unwrap();
    // σ and Σ are one name on NTFS and the final ς another: the system decides, and the
    // comparison for messages agrees with it.
    assert!(taken(create_new_file(&dir.join("Σ.wav"), b"maiuscula")));
    assert!(taken(move_no_replace(
        &dir.join("x.wav"),
        &dir.join("Σ.WAV")
    )));
    assert!(taken(copy_new(&dir.join("x.wav"), &dir.join("Σ.wav"))));
    assert!(taken(link_or_copy(&dir.join("x.wav"), &dir.join("Σ.wav"))));
    assert!(names::same_name("σ.wav", "Σ.WAV"));
    create_new_file(&dir.join("ς.wav"), b"final").unwrap();
    assert!(!names::same_name("σ.wav", "ς.wav"));
    assert_eq!(fs::read(dir.join("σ.wav")).unwrap(), b"sigma");
    let long = dir.join("Musica do menu principal.wav");
    create_new_file(&long, b"menu").unwrap();
    // Only where the disk makes 8.3 names.
    if let Some(short) = short_name(&long).filter(|short| short.contains('~')) {
        let short = dir.join(short);
        assert!(taken(create_new_file(&short, b"outro")));
        assert!(taken(move_no_replace(&dir.join("x.wav"), &short)));
        assert!(taken(copy_new(&dir.join("x.wav"), &short)));
        assert!(taken(fs::create_dir(&short)));
    }
    assert_eq!(fs::read(&long).unwrap(), b"menu");
    assert_eq!(listing(&dir).len(), 4);
    fs::remove_dir_all(dir).unwrap();
}
#[cfg(windows)]
fn short_name(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, length: u32) -> u32;
    }
    let long = wide(path).ok()?;
    let mut short = vec![0_u16; 2048];
    // The buffer outlives the call and its length is the one passed.
    let length =
        unsafe { GetShortPathNameW(long.as_ptr(), short.as_mut_ptr(), short.len() as u32) };
    let length = usize::try_from(length).ok()?;
    if length == 0 || length >= short.len() {
        return None;
    }
    let short = PathBuf::from(std::ffi::OsString::from_wide(&short[..length]));
    Some(short.file_name()?.to_string_lossy().into_owned())
}

#[cfg(windows)]
#[test]
fn open_files_and_folders_with_open_files_inside_are_busy_and_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = temp();
    let pista = dir.join("Pista");
    fs::create_dir_all(pista.join("Sub")).unwrap();
    fs::write(pista.join("x.wav"), b"x").unwrap();
    fs::write(pista.join("Sub").join("a.wav"), b"a").unwrap();
    fs::write(pista.join("Sub").join("b.wav"), b"b").unwrap();
    let fase = dir.join("Fase");
    // A file another program opened without letting it go away.
    let open = |share: u32| {
        OpenOptions::new()
            .read(true)
            .share_mode(share)
            .open(pista.join("x.wav"))
            .unwrap()
    };
    let handle = open(1);
    let error = move_no_replace(&pista.join("x.wav"), &pista.join("y.wav")).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(32));
    assert!(busy(&error));
    drop(handle);
    // A folder with a file open inside, letting it go away or not: access denied.
    for share in [3, 7] {
        let handle = open(share);
        let error = move_no_replace(&pista, &fase).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5), "share {share}");
        assert!(busy(&error));
        drop(handle);
    }
    // A folder with a subfolder being listed.
    let mut listed = fs::read_dir(pista.join("Sub")).unwrap();
    listed.next();
    let error = move_no_replace(&pista, &fase).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(5));
    drop(listed);
    // Tried again while the other program lets go.
    let handle = open(3);
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        drop(handle);
    });
    let mut tries = 0;
    retrying(|| {
        tries += 1;
        move_no_replace(&pista, &fase)
    })
    .unwrap();
    release.join().unwrap();
    assert!(tries > 1, "{tries}");
    assert_eq!(fs::read(fase.join("x.wav")).unwrap(), b"x");
    // A batch names the cause.
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(fase.join("x.wav"))
        .unwrap();
    let error = batch(&dir, &[moving("Fase", "Pista")]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Não foi possível mover Fase para Pista: um arquivo dentro de Fase está aberto em outro \
         programa (ou um terminal está nessa pasta)"
    );
    drop(handle);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn a_folder_that_is_a_programs_current_folder_is_busy() {
    use std::io::BufRead;
    let dir = temp();
    let pista = dir.join("Pista");
    fs::create_dir(&pista).unwrap();
    let mut terminal = std::process::Command::new("cmd")
        .args(["/d", "/q", "/k", "echo pronto"])
        .current_dir(&pista)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut output = io::BufReader::new(terminal.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "pronto");
    let error = move_no_replace(&pista, &dir.join("Fase")).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(32));
    assert!(busy(&error));
    terminal.kill().unwrap();
    terminal.wait().unwrap();
    drop(output);
    retrying(|| move_no_replace(&pista, &dir.join("Fase"))).unwrap();
    assert_eq!(listing(&dir), ["Fase"]);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn the_busy_rule_is_the_exports_and_gives_up() {
    for code in [5, 32, 33] {
        assert!(busy(&io::Error::from_raw_os_error(code)), "{code}");
    }
    assert!(!busy(&io::Error::from_raw_os_error(2)));
    assert!(!busy(&io::Error::from_raw_os_error(183)));
    let mut tries = 0;
    let result: io::Result<()> = retrying(|| {
        tries += 1;
        Err(io::Error::from_raw_os_error(32))
    });
    assert!(result.is_err());
    assert_eq!(tries, 1 + RETRIES);
    let mut tries = 0;
    let result: io::Result<()> = retrying(|| {
        tries += 1;
        Err(io::ErrorKind::NotFound.into())
    });
    assert!(result.is_err());
    assert_eq!(tries, 1);
}

#[test]
fn copies_links_files_and_folders_are_only_created() {
    let dir = temp();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    create_new_file(&a, b"a").unwrap();
    create_new_file(&b, b"b").unwrap();
    assert!(taken(create_new_file(&a, b"outro")));
    assert!(taken(copy_new(&a, &b)));
    assert!(taken(link_or_copy(&a, &b)));
    assert_eq!(fs::read(&b).unwrap(), b"b");
    assert_eq!(copy_new(&a, &dir.join("c.wav")).unwrap(), 1);
    assert_eq!(fs::read(dir.join("c.wav")).unwrap(), b"a");
    assert_eq!(
        Stamp::read(&a).unwrap(),
        Stamp::read(&dir.join("c.wav")).unwrap(),
        "the copy keeps the write time"
    );
    link_or_copy(&a, &dir.join("d.wav")).unwrap();
    assert_eq!(fs::read(dir.join("d.wav")).unwrap(), b"a");
    assert_eq!(
        link_or_copy(&dir.join("falta.wav"), &dir.join("e.wav"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
    let pista = dir.join("Pista");
    create_dir_new(&pista).unwrap();
    assert!(taken(create_dir_new(&pista)));
    create_new_file(&pista.join("x.wav"), b"x").unwrap();
    assert!(remove_empty_dir(&pista).is_err());
    assert!(pista.join("x.wav").exists());
    fs::remove_file(pista.join("x.wav")).unwrap();
    remove_empty_dir(&pista).unwrap();
    assert_eq!(listing(&dir), ["a.wav", "b.wav", "c.wav", "d.wav"]);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn direct_calls_take_canonical_paths_past_260_characters() {
    for (path, form) in [
        (r"C:\a\b", r"\\?\C:\a\b"),
        ("C:/a/b", r"\\?\C:\a\b"),
        (r"C:\a\..\b\.\c", r"\\?\C:\b\c"),
        (r"C:\", r"\\?\C:\"),
        (r"\\servidor\pasta\x", r"\\?\UNC\servidor\pasta\x"),
        (r"\\servidor\pasta\..\y", r"\\?\UNC\servidor\pasta\y"),
        (r"\\?\C:\Pista.", r"\\?\C:\Pista."),
        // As Windows reads it without the form, which the standard library hands it.
        (r"C:\Projetos\Novo.\x", r"\\?\C:\Projetos\Novo\x"),
        (r"C:\Projetos\Novo.", r"\\?\C:\Projetos\Novo"),
    ] {
        assert_eq!(
            canonical(Path::new(path)).unwrap(),
            Path::new(form),
            "{path}"
        );
    }
    assert_eq!(shown(Path::new(r"\\?\C:\Pista\x.wav")), r"C:\Pista\x.wav");
    assert_eq!(
        shown(Path::new(r"\\?\UNC\servidor\pasta")),
        r"\\servidor\pasta"
    );
    let dir = temp();
    let mut deep = dir.clone();
    let mut relative = Vec::new();
    while deep.as_os_str().len() < 300 {
        deep.push("pasta-com-nome-comprido");
        relative.push("pasta-com-nome-comprido");
    }
    fs::create_dir_all(&deep).unwrap();
    create_new_file(&deep.join("Pulo.wav"), b"pulo").unwrap();
    assert!(deep.join("Pulo.wav").as_os_str().len() > 300);
    move_no_replace(&deep.join("Pulo.wav"), &deep.join("Salto.wav")).unwrap();
    let folder = relative.join("/");
    let file = format!("{folder}/Salto.wav");
    let slot = trash_slot("passo", 1, &file);
    // The deepest folder moves up and, when the next step fails, back down.
    let error = batch(
        &dir,
        &[
            moving(&folder, "Funda"),
            Op::Trash {
                from: "Funda/Falta.wav".into(),
                slot: slot.clone(),
                id: None,
            },
        ],
    )
    .unwrap_err();
    assert!(
        matches!(error, BatchError::RolledBack { step: 1, .. }),
        "{error}"
    );
    assert!(deep.join("Salto.wav").exists());
    assert!(!dir.join("Funda").exists());
    batch(
        &dir,
        &[Op::Trash {
            from: file.clone(),
            slot: slot.clone(),
            id: None,
        }],
    )
    .unwrap();
    assert!(!deep.join("Salto.wav").exists());
    assert_eq!(trash_index(&dir)["passo-1.wav"].path, file);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(windows)]
#[test]
fn direct_calls_and_the_standard_library_name_the_same_file() {
    // A path that does not come from the resolver (a new project's folder, an export's).
    let dir = temp();
    let novo = dir.join("Novo.");
    fs::create_dir_all(&novo).unwrap();
    assert_eq!(listing(&dir), ["Novo"]);
    persistence::safe_write(&novo.join("p.json"), b"um").unwrap();
    persistence::safe_write(&novo.join("p.json"), b"dois").unwrap();
    move_no_replace(&novo.join("p.json"), &novo.join("q.json.")).unwrap();
    assert_eq!(listing(&dir.join("Novo")), ["q.json"]);
    assert_eq!(fs::read(dir.join("Novo").join("q.json")).unwrap(), b"dois");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn trash_slots_are_flat_and_fixed_by_the_step() {
    for (n, from, slot) in [
        (1, "Pista/Bipe.wav", ".oxy/lixeira/passo-1.wav"),
        (
            2,
            "Pista/Torre.modelo.json",
            ".oxy/lixeira/passo-2.modelo.json",
        ),
        (3, "Imagens/Céu.PNG", ".oxy/lixeira/passo-3.png"),
        (4, "scripts/porta.luau", ".oxy/lixeira/passo-4.luau"),
        (5, "LEIA-ME", ".oxy/lixeira/passo-5"),
        (6, r"Pista\Música 1.5.wav", ".oxy/lixeira/passo-6.wav"),
    ] {
        assert_eq!(trash_slot("passo", n, from), slot, "{from}");
    }
    assert_eq!(slot_name(".oxy/lixeira/passo-1.wav"), Some("passo-1.wav"));
    for slot in [
        ".oxy/lixeira/",
        ".oxy/lixeira/a/b.wav",
        "Pista/x.wav",
        ".oxy/lixeiras/x",
    ] {
        assert_eq!(slot_name(slot), None, "{slot}");
    }
}

#[test]
fn batches_undo_what_they_did_when_a_step_fails() {
    let root = temp();
    for name in ["a.wav", "b.wav", "c.wav", "d.wav"] {
        create_new_file(&root.join(name), name.as_bytes()).unwrap();
    }
    let before = files(&root);
    let id = new_id();
    let slot = trash_slot("passo", 1, "b.wav");
    let ops = [
        Op::CreateDir {
            path: "Pista".into(),
        },
        moving("a.wav", "Pista/a.wav"),
        Op::Trash {
            from: "b.wav".into(),
            slot: slot.clone(),
            id: Some(id.clone()),
        },
    ];
    let mut failing = ops.to_vec();
    failing.push(moving("c.wav", "d.wav"));
    assert_eq!(
        batch(&root, &failing).unwrap_err(),
        BatchError::RolledBack {
            step: 3,
            message: "Não foi possível mover c.wav para d.wav: já existe d.wav".into()
        }
    );
    assert_eq!(files(&root), before);
    assert!(!root.join("Pista").exists());
    assert!(trash_index(&root).is_empty());
    // Done, noted in the trash, then undone and redone by the inverses.
    batch(&root, &ops).unwrap();
    assert!(root.join("Pista/a.wav").is_file());
    assert!(!root.join("b.wav").exists());
    assert_eq!(fs::read(root.join(&slot)).unwrap(), b"b.wav");
    let index = trash_index(&root);
    let entry = &index["passo-1.wav"];
    assert_eq!(
        (entry.path.as_str(), entry.id.as_ref()),
        ("b.wav", Some(&id))
    );
    assert!(
        entry.date.len() == 20 && entry.date.ends_with('Z'),
        "{}",
        entry.date
    );
    let undo: Vec<Op> = ops.iter().rev().map(Op::inverse).collect();
    batch(&root, &undo).unwrap();
    assert_eq!(files(&root), before);
    assert!(trash_index(&root).is_empty());
    batch(&root, &ops).unwrap();
    assert!(!root.join("b.wav").exists());
    // A slot is never written over, and only the trash has slots.
    create_new_file(&root.join("b.wav"), b"outro b").unwrap();
    let error = batch(
        &root,
        &[Op::Trash {
            from: "b.wav".into(),
            slot: slot.clone(),
            id: None,
        }],
    )
    .unwrap_err();
    assert!(error.to_string().ends_with("está ocupada"), "{error}");
    let error = batch(
        &root,
        &[Op::Trash {
            from: "b.wav".into(),
            slot: "Pista/b.wav".into(),
            id: None,
        }],
    )
    .unwrap_err();
    assert!(error.to_string().contains("não é uma vaga"), "{error}");
    assert_eq!(fs::read(root.join("b.wav")).unwrap(), b"outro b");
    assert_eq!(fs::read(root.join(&slot)).unwrap(), b"b.wav");
    // Brought back to a folder that went meanwhile: the trash still holds the file.
    let error = batch(
        &root,
        &[Op::Untrash {
            slot: slot.clone(),
            to: "Nenhuma/b.wav".into(),
            id: None,
        }],
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Não foi possível trazer Nenhuma/b.wav de volta da lixeira do projeto: a pasta onde \
         Nenhuma/b.wav ficaria não existe"
    );
    assert_eq!(fs::read(root.join(&slot)).unwrap(), b"b.wav");
    assert!(trash_index(&root).contains_key("passo-1.wav"));
    // Brought back from an emptied trash.
    fs::remove_file(root.join(&slot)).unwrap();
    let error = batch(
        &root,
        &[Op::Untrash {
            slot,
            to: "Pista/b.wav".into(),
            id: None,
        }],
    )
    .unwrap_err();
    assert!(error.to_string().contains("foi esvaziada"), "{error}");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn a_batch_that_cannot_undo_keeps_what_it_did_and_says_so() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = temp();
    create_new_file(&root.join("x.txt"), b"x").unwrap();
    let ops = [moving("x.txt", "y.txt"), moving("falta.txt", "z.txt")];
    let mut lock = None;
    let error = run_batch(&root, &ops, |step| {
        if step == 1 {
            lock = Some(
                OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(root.join("y.txt"))
                    .unwrap(),
            );
        }
    })
    .unwrap_err();
    drop(lock);
    assert_eq!(
        error,
        BatchError::RecoveryKept {
            step: 1,
            message: "Não foi possível mover falta.txt para z.txt: falta.txt não existe".into(),
            restore: "mover y.txt para x.txt: y.txt está aberto em outro programa".into(),
            kept: 1,
        }
    );
    assert!(
        error
            .to_string()
            .contains("também não foi possível desfazer")
    );
    assert_eq!(listing(&root), ["y.txt"]);
    assert_eq!(fs::read(root.join("y.txt")).unwrap(), b"x");
    // What stays in the trash is noted there all the same.
    let slot = trash_slot("passo", 1, "y.txt");
    let ops = [
        Op::Trash {
            from: "y.txt".into(),
            slot: slot.clone(),
            id: None,
        },
        moving("falta.txt", "z.txt"),
    ];
    let mut lock = None;
    let error = run_batch(&root, &ops, |step| {
        if step == 1 {
            lock = Some(
                OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(root.join(&slot))
                    .unwrap(),
            );
        }
    })
    .unwrap_err();
    drop(lock);
    assert!(
        matches!(error, BatchError::RecoveryKept { kept: 1, .. }),
        "{error}"
    );
    assert_eq!(trash_index(&root)["passo-1.txt"].path, "y.txt");
    assert_eq!(fs::read(root.join(&slot)).unwrap(), b"x");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn batches_wait_for_the_scan() {
    let root = temp();
    create_new_file(&root.join("a.wav"), b"a").unwrap();
    let scan = tree_lock();
    let moved = {
        let root = root.clone();
        std::thread::spawn(move || batch(&root, &[moving("a.wav", "b.wav")]))
    };
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        root.join("a.wav").exists(),
        "the batch waits while the scan lists"
    );
    drop(scan);
    moved.join().unwrap().unwrap();
    assert_eq!(listing(&root), ["b.wav"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn batches_say_why_in_the_projects_words() {
    let root = temp();
    create_new_file(&root.join("a.wav"), b"a").unwrap();
    fs::create_dir(root.join("Cheia")).unwrap();
    create_new_file(&root.join("Cheia").join("x.wav"), b"x").unwrap();
    for (op, message) in [
        (
            moving("falta.wav", "b.wav"),
            "Não foi possível mover falta.wav para b.wav: falta.wav não existe",
        ),
        (
            moving("a.wav", "Nenhuma/a.wav"),
            "Não foi possível mover a.wav para Nenhuma/a.wav: a pasta onde Nenhuma/a.wav \
             ficaria não existe",
        ),
        (
            Op::CreateDir {
                path: "Nenhuma/Funda".into(),
            },
            "Não foi possível criar a pasta Nenhuma/Funda: a pasta onde Nenhuma/Funda ficaria \
             não existe",
        ),
        (
            Op::CreateDir {
                path: "Cheia".into(),
            },
            "Não foi possível criar a pasta Cheia: já existe Cheia",
        ),
        (
            Op::RemoveDir {
                path: "Cheia".into(),
            },
            "Não foi possível apagar a pasta vazia Cheia: a pasta Cheia não está vazia",
        ),
        (
            moving("a.wav", "Pista./a.wav"),
            "Não foi possível mover a.wav para Pista./a.wav: Caminho inválido Pista./a.wav: \
             \"Pista.\" termina em ponto, que o Windows corta",
        ),
    ] {
        let error = batch(&root, std::slice::from_ref(&op)).unwrap_err();
        assert_eq!(
            error,
            BatchError::RolledBack {
                step: 0,
                message: message.into()
            },
            "{op:?}"
        );
    }
    assert_eq!(listing(&root), ["Cheia", "a.wav"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_trash_index_oxy_cannot_make_sense_of_is_set_aside_not_written_over() {
    let root = temp();
    create_new_file(&root.join("a.wav"), b"a").unwrap();
    create_new_file(&root.join("b.wav"), b"b").unwrap();
    let trash = root.join(TRASH);
    fs::create_dir_all(&trash).unwrap();
    fs::write(root.join(TRASH_INDEX), b"{ \"passo-0.wav\": quebrado").unwrap();
    let trashing = |from: &str, n: usize| Op::Trash {
        from: from.into(),
        slot: trash_slot("passo", n, from),
        id: None,
    };
    batch(&root, &[trashing("a.wav", 1)]).unwrap();
    assert_eq!(
        trash_index(&root).keys().collect::<Vec<_>>(),
        ["passo-1.wav"]
    );
    let aside: Vec<String> = listing(&trash)
        .into_iter()
        .filter(|name| name.starts_with("indice.") && name != "indice.json")
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert!(names::check_part(&aside[0]).is_ok(), "{aside:?}");
    assert_eq!(
        fs::read(trash.join(&aside[0])).unwrap(),
        b"{ \"passo-0.wav\": quebrado"
    );
    // What a newer OXY adds to an entry does not make the index unreadable.
    let mut newer: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(TRASH_INDEX)).unwrap()).unwrap();
    newer["passo-1.wav"]["origem"] = "Biblioteca".into();
    fs::write(root.join(TRASH_INDEX), newer.to_string()).unwrap();
    batch(&root, &[trashing("b.wav", 2)]).unwrap();
    assert_eq!(
        trash_index(&root).keys().collect::<Vec<_>>(),
        ["passo-1.wav", "passo-2.wav"]
    );
    assert_eq!(listing(&trash).len(), 4, "one index set aside");
    fs::remove_dir_all(root).unwrap();
}
