//! Names of the project's files and folders as Windows keeps them: the rules a name a person
//! types must follow (refused with the reason), the adjustment of a name OXY makes from another
//! (a source file's, a game's title), how names compare and in what order the tree lists them,
//! and the one resolver every reader of a project's file goes through (saving, the renderer,
//! the audio and the tree), so no two paths name one file.
use crate::persistence::PersistenceError;
use std::{
    cmp::Ordering,
    fs,
    path::{Component, Path, PathBuf},
};

/// Longest name of a file (without its type's extension) or folder, in characters.
pub const MAX_NAME_CHARS: usize = 80;
/// Longest path of a file from the project's folder, in bytes: Luau names a script's chunk by
/// it and cuts that name near 255 bytes.
pub const MAX_PATH_BYTES: usize = 180;
/// Past this many characters, the project's folder and a path in it are too long for the
/// Explorer, PowerShell 5.1 and zip tools.
pub const LONG_PATH_CHARS: usize = 240;

/// The characters Windows refuses in names, besides control characters; `/` and `\` separate
/// the parts of a path.
const FORBIDDEN: &str = r#"<>:"/\|?*"#;

/// Whether Windows takes `name` for a device, whatever its case and extension: CON, PRN, AUX,
/// NUL, COM0 to COM9, LPT0 to LPT9, COM¹²³, LPT¹²³, CONIN$ and CONOUT$ (`nul.png` and `CON .txt`
/// as well).
pub fn reserved(name: &str) -> bool {
    let stem: String = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .chars()
        .map(fold)
        .collect();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let Some(number) = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut digits = number.chars();
    matches!(
        (digits.next(), digits.next()),
        (Some('0'..='9' | '¹' | '²' | '³'), None)
    )
}

/// Checks one part of a path as Windows reads it: none of the characters it refuses
/// (`< > : " / \ | ? *` and control characters), no dot or space at the end (Windows cuts them,
/// so `Pista.` and `Pista` would be one folder) and no device name. Why not, when it fails.
pub fn check_part(part: &str) -> Result<(), String> {
    if part.is_empty() {
        return Err("o nome está vazio".into());
    }
    if part.chars().any(char::is_control) {
        return Err(format!("{part:?} tem um caractere de controle"));
    }
    if let Some(c) = part.chars().find(|c| FORBIDDEN.contains(*c)) {
        return Err(format!(
            "\"{part}\" tem {c}, que o Windows não aceita em nomes"
        ));
    }
    if part.ends_with('.') {
        return Err(format!("\"{part}\" termina em ponto, que o Windows corta"));
    }
    if part.ends_with(' ') {
        return Err(format!("\"{part}\" termina em espaço, que o Windows corta"));
    }
    if reserved(part) {
        return Err(format!(
            "\"{part}\" é um nome que o Windows reserva para dispositivos"
        ));
    }
    Ok(())
}

/// A name a person typed for a file (without its type's extension) or a folder, with the spaces
/// at its ends trimmed: [`check_part`], and neither a dot at the start (what starts with a dot
/// is outside the project), an accent typed apart from its letter (it would look like another
/// file's name) nor more than [`MAX_NAME_CHARS`] characters. Why not, when it fails.
pub fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    check_part(name)?;
    if name.starts_with('.') {
        return Err(outside(name));
    }
    if has_loose_marks(name) {
        return Err(format!(
            "\"{name}\" tem um acento separado da letra; digite a letra acentuada de novo"
        ));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("\"{name}\" passa de {MAX_NAME_CHARS} caracteres"));
    }
    Ok(name.to_owned())
}
fn outside(name: &str) -> String {
    format!("\"{name}\" começa com ponto, e o que começa com ponto fica fora do projeto")
}

/// Checks `relative` as the path from the project's folder of a file or folder OXY creates:
/// parts split by `/` alone, each passing [`check_part`] and none starting with a dot, and at
/// most [`MAX_PATH_BYTES`] bytes. Why not, when it fails.
pub fn check_path(relative: &str) -> Result<(), String> {
    if relative.len() > MAX_PATH_BYTES {
        return Err(format!(
            "o caminho {relative} é longo demais: o limite é de {MAX_PATH_BYTES} bytes"
        ));
    }
    for part in relative.split('/') {
        if part.is_empty() {
            return Err(format!("o caminho \"{relative}\" tem uma parte vazia"));
        }
        check_part(part)?;
        if part.starts_with('.') {
            return Err(outside(part));
        }
    }
    Ok(())
}

/// Whether `name` has an accent written apart from its letter (U+0300 to U+036F), as names
/// made on macOS have: Windows keeps it as another name than the one typed with the letter.
pub fn has_loose_marks(name: &str) -> bool {
    name.chars().any(is_mark)
}
fn is_mark(c: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&c)
}

/// `text` made a name for Windows, as a game's folder is named after its title: what Windows
/// refuses becomes a space, spaces collapse, at most `max_chars` characters, no dot or space at
/// the end, and a device name gets `fallback` before it; `fallback` alone when nothing is left.
pub fn fit(text: &str, max_chars: usize, fallback: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN.contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut name = cap(
        &cleaned.split_whitespace().collect::<Vec<_>>().join(" "),
        max_chars,
    );
    if reserved(&name) {
        name = cap(&format!("{fallback} {name}"), max_chars);
    }
    if name.is_empty() {
        fallback.into()
    } else {
        name
    }
}
fn cap(name: &str, max_chars: usize) -> String {
    let capped: String = name.chars().take(max_chars).collect();
    capped.trim_end_matches(['.', ' ']).to_owned()
}

/// A file's or folder's name OXY makes from `text` (a source file's name, a converted asset's),
/// following [`check_name`]'s rules: [`fit`] to [`MAX_NAME_CHARS`], without dots at the start
/// and with each accent written apart from its letter joined to it (the other loose accents
/// go). The caller says when it differs from `text`.
pub fn fitted_name(text: &str, fallback: &str) -> String {
    // The dots come off after cleaning, which may leave one first ("?.Pulo"), and the second
    // fit puts back the device name's prefix and the fallback.
    let fitted = fit(&composed(text), MAX_NAME_CHARS, fallback);
    fit(
        fitted.trim_start_matches(['.', ' ']),
        MAX_NAME_CHARS,
        fallback,
    )
}
/// `text` with each letter and the accent written after it joined into one character, for the
/// letters of Portuguese and the rest of Latin-1; the other loose accents go.
fn composed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if !is_mark(c) {
            out.push(c);
        } else if let Some(last) = out.pop() {
            out.push(compose(last, c).unwrap_or(last));
        }
    }
    out
}
fn compose(letter: char, mark: char) -> Option<char> {
    let (bases, joined) = match mark {
        '\u{300}' => ("AEIOUaeiou", "ÀÈÌÒÙàèìòù"),
        '\u{301}' => ("AEIOUYaeiouy", "ÁÉÍÓÚÝáéíóúý"),
        '\u{302}' => ("AEIOUaeiou", "ÂÊÎÔÛâêîôû"),
        '\u{303}' => ("ANOano", "ÃÑÕãñõ"),
        '\u{308}' => ("AEIOUaeiouy", "ÄËÏÖÜäëïöüÿ"),
        '\u{327}' => ("Cc", "Çç"),
        _ => return None,
    };
    let index = bases.chars().position(|base| base == letter)?;
    joined.chars().nth(index)
}

/// `c` as NTFS compares names: in upper case when that is one letter whose lower case is `c`
/// again (`σ` and `Σ`, `ǆ` and `Ǆ`, `ÿ` and `Ÿ` are one name each), else as it is (`ς`, `ß`,
/// `ı`, `ſ`, `µ` and `ǅ` are names apart from `Σ`, `SS`, `I`, `S`, `Μ` and `Ǆ`).
pub fn fold(c: char) -> char {
    fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
        match (chars.next(), chars.next()) {
            (Some(single), None) => Some(single),
            _ => None,
        }
    }
    match single(c.to_uppercase()) {
        Some(upper) if single(upper.to_lowercase()) == Some(c) => upper,
        _ => c,
    }
}
/// Whether `a` and `b` name one file on Windows as far as case goes. Only for messages: what
/// keeps a name from being taken twice is the system's refusal to create or move onto it.
pub fn same_name(a: &str, b: &str) -> bool {
    a.chars().map(fold).eq(b.chars().map(fold))
}

/// The order of names everywhere the tree lists or decides by them: [`fold`]ed, with numbers in
/// natural order ("Pulo 2" before "Pulo 10"), and the exact bytes when they tie ("Pulo" before
/// "pulo"), so it is total.
pub fn name_order(a: &str, b: &str) -> Ordering {
    natural(a, b).then_with(|| a.as_bytes().cmp(b.as_bytes()))
}
/// The order of a folder's entries, each as (name, whether it is a folder): folders first, then
/// [`name_order`].
pub fn tree_order(a: (&str, bool), b: (&str, bool)) -> Ordering {
    b.1.cmp(&a.1).then_with(|| name_order(a.0, b.0))
}
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.next_if(char::is_ascii_digit) {
                        digits.push(c);
                    }
                    digits.trim_start_matches('0').to_owned()
                };
                let (x, y) = (number(&mut a), number(&mut b));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(&y));
                if order.is_ne() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = fold(x).cmp(&fold(y));
                if order.is_ne() {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The file or folder `relative` (from the project's folder, parts split by `/` or `\`) of the
/// project at `root`, in the canonical `\\?\` form every direct call to Windows takes. Each part
/// passes [`check_part`], so the path names one file however it is opened (`Pista.` would be
/// another folder than `Pista` in this form and the same one without it); `..`, drives,
/// absolute paths and links that lead out of the folder are refused.
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf, PersistenceError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || path.has_root()
        || relative.contains(':')
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(PersistenceError::InvalidAssetPath(relative.to_owned()));
    }
    let parts: Vec<&str> = parts(relative).collect();
    for part in &parts {
        check_part(part).map_err(|reason| PersistenceError::InvalidPath {
            path: relative.to_owned(),
            reason,
        })?;
    }
    let canonical_root = fs::canonicalize(root).map_err(|source| PersistenceError::Io {
        context: "Pasta do projeto indisponível",
        source,
    })?;
    let mut result = canonical_root.clone();
    result.extend(&parts);
    // A link or junction on the way may lead out of the folder.
    let mut ancestor = result.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or(PersistenceError::NoExistingFolder)?;
    }
    if !fs::canonicalize(ancestor)?.starts_with(&canonical_root) {
        return Err(PersistenceError::AssetOutsideProject(relative.to_owned()));
    }
    Ok(result)
}
fn parts(relative: &str) -> impl Iterator<Item = &str> {
    relative
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
}

/// `relative` written with `/` and the case its folders and file have on the disk (`Pista/pulo.wav`
/// for `PISTA/PULO.WAV` there), as OXY keeps paths before it writes, attaches or moves; the
/// parts that do not exist stay as given.
pub fn disk_case(root: &Path, relative: &str) -> Result<String, PersistenceError> {
    let parts: Vec<&str> = parts(relative).collect();
    let mut existing = resolve(root, relative)?;
    let mut found = parts.len();
    while found > 0 && !existing.exists() {
        existing.pop();
        found -= 1;
    }
    let canonical_root = fs::canonicalize(root)?;
    let real = fs::canonicalize(&existing)?;
    let on_disk: Vec<String> = real
        .strip_prefix(&canonical_root)
        .map(|inside| {
            inside
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    // A link inside the folder leads somewhere else: its own name is the one kept.
    if on_disk.len() != found {
        return Ok(parts.join("/"));
    }
    Ok(on_disk
        .iter()
        .map(String::as_str)
        .chain(parts[found..].iter().copied())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Whether the project's folder `root` and `relative` in it make a path past
/// [`LONG_PATH_CHARS`] characters, worth a warning.
pub fn too_long(root: &Path, relative: &str) -> bool {
    let root = root.to_string_lossy();
    let root = root.strip_prefix(r"\\?\").unwrap_or(&root);
    root.chars().count() + 1 + relative.chars().count() > LONG_PATH_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::new_id;

    #[test]
    fn typed_names_follow_what_windows_keeps() {
        for (typed, kept) in [
            ("  Música  ", "Música"),
            ("Pulo 2", "Pulo 2"),
            ("Clique · menu", "Clique · menu"),
            ("COM10", "COM10"),
            ("Console", "Console"),
            ("Conexão", "Conexão"),
            ("nulo", "nulo"),
            ("a.b", "a.b"),
        ] {
            assert_eq!(check_name(typed).as_deref(), Ok(kept), "{typed:?}");
        }
        for (typed, reason) in [
            ("", "vazio"),
            ("   ", "vazio"),
            ("Pista/Pulo", "/, que o Windows não aceita"),
            ("Pista\\Pulo", "\\, que o Windows não aceita"),
            ("Fase: 1", ":, que o Windows não aceita"),
            ("Quem?", "?, que o Windows não aceita"),
            ("<a>", "<, que o Windows não aceita"),
            ("a*b", "*, que o Windows não aceita"),
            ("a|b", "|, que o Windows não aceita"),
            ("\"a\"", "\", que o Windows não aceita"),
            ("sino\u{7}", "caractere de controle"),
            ("Pista.", "termina em ponto"),
            ("Pista...", "termina em ponto"),
            ("CON", "reserva para dispositivos"),
            ("con", "reserva para dispositivos"),
            ("nul.png", "reserva para dispositivos"),
            ("CON .txt", "reserva para dispositivos"),
            ("COM0", "reserva para dispositivos"),
            ("com9", "reserva para dispositivos"),
            ("LPT0", "reserva para dispositivos"),
            ("lpt¹", "reserva para dispositivos"),
            ("COM³.wav", "reserva para dispositivos"),
            ("CONIN$", "reserva para dispositivos"),
            ("conout$.txt", "reserva para dispositivos"),
            ("AUX", "reserva para dispositivos"),
            ("Prn", "reserva para dispositivos"),
            (".solucoes", "começa com ponto"),
            ("Mu\u{301}sica", "acento separado"),
            (&"a".repeat(81), "passa de 80"),
        ] {
            let error = check_name(typed).unwrap_err();
            assert!(error.contains(reason), "{typed:?}: {error}");
        }
        assert!(check_name(&"á".repeat(80)).is_ok(), "characters, not bytes");
        // Trimmed when typed; a part read from the disk is refused for it.
        assert!(
            check_part("Pista ")
                .unwrap_err()
                .contains("termina em espaço")
        );
        assert!(check_part(" Pista").is_ok());
        assert!(check_part(".oxy").is_ok());
    }

    #[test]
    fn device_names_are_known_in_every_form() {
        for name in [
            "CON",
            "PRN",
            "AUX",
            "NUL",
            "COM0",
            "COM1",
            "COM9",
            "LPT0",
            "LPT5",
            "LPT9",
            "COM¹",
            "COM²",
            "COM³",
            "LPT¹",
            "LPT²",
            "LPT³",
            "CONIN$",
            "CONOUT$",
            "con",
            "Nul",
            "nul.png",
            "NUL.tar.gz",
            "CON .wav",
            "com1 ",
        ] {
            assert!(reserved(name), "{name}");
        }
        for name in [
            "CONSOLE", "COM10", "LPT", "COM", "NULO", "CONIN", "COMA", "LPT⁴", "xCON", "AUX1", "",
            ".CON",
        ] {
            assert!(!reserved(name), "{name}");
        }
    }

    #[test]
    fn new_paths_keep_inside_the_project_and_its_limits() {
        assert_eq!(check_path("Pista/Pulo.wav"), Ok(()));
        assert_eq!(check_path("Pulo.wav"), Ok(()));
        for (path, reason) in [
            (".oxy/lixeira/x.wav", "começa com ponto"),
            ("Pista/.rascunho.luau", "começa com ponto"),
            ("Pista//x.wav", "parte vazia"),
            ("/x.wav", "parte vazia"),
            ("Pista/", "parte vazia"),
            ("Pista\\x.wav", "\\, que o Windows não aceita"),
            ("Pista./x.wav", "termina em ponto"),
            ("Pista /x.wav", "termina em espaço"),
            ("Pista/CON.wav", "reserva para dispositivos"),
            ("C:/x.wav", ":, que o Windows não aceita"),
        ] {
            let error = check_path(path).unwrap_err();
            assert!(error.contains(reason), "{path}: {error}");
        }
        let longest = format!("{}/{}.wav", "a".repeat(80), "b".repeat(95));
        assert_eq!(longest.len(), MAX_PATH_BYTES);
        assert_eq!(check_path(&longest), Ok(()));
        // Bytes, not characters: Luau cuts the chunk's name by bytes.
        let accented = format!("{}/{}.wav", "á".repeat(41), "b".repeat(95));
        assert!(accented.chars().count() < MAX_PATH_BYTES);
        assert!(check_path(&accented).unwrap_err().contains("longo demais"));
        assert!(!too_long(Path::new(r"C:\Projetos\Corrida"), &longest));
        assert!(too_long(
            Path::new(
                r"\\?\C:\Users\Gabriel\Documents\OXY Projetos\Uma pasta de projeto com nome comprido"
            ),
            &longest
        ));
    }

    #[test]
    fn names_made_by_oxy_are_adjusted_to_the_rules() {
        for (text, made) in [
            ("Pulo", "Pulo"),
            ("Clique: menu", "Clique menu"),
            ("  .Pulo  ", "Pulo"),
            ("...", "Som"),
            ("CON", "Som CON"),
            ("Ponto. ", "Ponto"),
            ("Mu\u{301}sica", "Música"),
            ("Ac\u{327}a\u{303}o", "Ação"),
            ("Pin\u{303}a colada", "Piña colada"),
            ("a\u{301}\u{301}", "á"),
            ("\u{301}Eco", "Eco"),
            ("x\u{301}", "x"),
            ("Fase\t1\n2", "Fase 1 2"),
            ("?.Pulo", "Pulo"),
            ("\u{1}.Eco", "Eco"),
            ("<>.CON", "Som CON"),
            (". . .x", "x"),
            ("?.", "Som"),
        ] {
            assert_eq!(fitted_name(text, "Som"), made, "{text:?}");
            assert!(check_name(&fitted_name(text, "Som")).is_ok(), "{text:?}");
        }
        assert_eq!(fitted_name(&"ç".repeat(100), "Som").chars().count(), 80);
        assert_eq!(fit("COM0", 64, "Jogo"), "Jogo COM0");
        assert_eq!(fit("conin$", 64, "Jogo"), "Jogo conin$");
        assert_eq!(
            fit(".Corrida", 64, "Jogo"),
            ".Corrida",
            "a game's folder may"
        );
    }

    #[test]
    fn names_compare_as_ntfs_does() {
        // As measured on NTFS (Windows 11): which name a create-only refuses after another.
        for (a, b) in [
            ("Pulo.WAV", "pulo.wav"),
            ("Ação", "AÇÃO"),
            ("σ", "Σ"),
            ("ǆ", "Ǆ"),
            ("ÿ", "Ÿ"),
            ("i", "I"),
        ] {
            assert!(same_name(a, b), "{a} {b}");
        }
        for (a, b) in [
            ("ς", "σ"),
            ("ς", "Σ"),
            ("ß", "SS"),
            ("ǅ", "Ǆ"),
            ("ı", "I"),
            ("ſ", "S"),
            ("µ", "Μ"),
            ("\u{212A}", "K"),
            ("Música", "Mu\u{301}sica"),
        ] {
            assert!(!same_name(a, b), "{a} {b}");
        }
        assert_eq!(fold('σ'), 'Σ');
        assert_eq!(fold('Σ'), 'Σ');
        assert_eq!(fold('ς'), 'ς');
    }

    #[test]
    fn the_tree_lists_folders_first_in_natural_order_with_a_total_tie_break() {
        let mut entries = vec![
            ("Pulo 10", false),
            ("pulo 2", false),
            ("Zeta", false),
            ("Árvore", false),
            ("_x", false),
            ("Pulo 2", false),
            ("Pista", true),
            ("a", false),
            ("Pulo 02", false),
            ("Menu", true),
            ("Pulo", false),
        ];
        entries.sort_by(|a, b| tree_order(*a, *b));
        let names: Vec<&str> = entries.iter().map(|e| e.0).collect();
        assert_eq!(
            names,
            [
                "Menu", "Pista", "a", "Pulo", "Pulo 02", "Pulo 2", "pulo 2", "Pulo 10", "Zeta",
                "_x", "Árvore"
            ]
        );
        assert_eq!(name_order("Pulo 2", "pulo 2"), Ordering::Less);
        assert_eq!(name_order("pulo 2", "pulo 2"), Ordering::Equal);
        assert_eq!(name_order("Fase 9", "Fase 10"), Ordering::Less);
        assert_eq!(
            name_order("Fase 99999999999999999999", "Fase 100000000000000000000"),
            Ordering::Less,
            "numbers past u64 too"
        );
        assert_eq!(name_order("a1b", "a1"), Ordering::Greater);
    }

    #[test]
    fn the_resolver_names_one_file_per_path() {
        let root = std::env::temp_dir().join(format!("oxy-names-{}", new_id()));
        fs::create_dir_all(root.join("Pista")).unwrap();
        let canonical = fs::canonicalize(&root).unwrap();
        assert_eq!(
            resolve(&root, "Pista/Pulo.wav").unwrap(),
            canonical.join("Pista").join("Pulo.wav")
        );
        assert_eq!(
            resolve(&root, r"Pista\Pulo.wav").unwrap(),
            canonical.join("Pista").join("Pulo.wav")
        );
        assert_eq!(
            resolve(&root, "./Pista//Pulo.wav").unwrap(),
            canonical.join("Pista").join("Pulo.wav")
        );
        assert_eq!(
            resolve(&root, ".oxy/agente/scripts/porta.luau.bak").unwrap(),
            canonical.join(".oxy/agente/scripts/porta.luau.bak")
        );
        if cfg!(windows) {
            assert!(
                resolve(&root, "Pulo.wav")
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(r"\\?\")
            );
        }
        for path in [
            "",
            "../fora.png",
            "Pista/../../fora.png",
            "/fora.png",
            "C:/x.png",
        ] {
            assert!(
                matches!(
                    resolve(&root, path),
                    Err(PersistenceError::InvalidAssetPath(_))
                ),
                "{path:?}"
            );
        }
        for (path, reason) in [
            ("Pista./Pulo.wav", "termina em ponto"),
            ("Pista /Pulo.wav", "termina em espaço"),
            ("Pista/Pulo.wav.", "termina em ponto"),
            ("assets/CON.png", "reserva"),
            ("assets/nul.png", "reserva"),
            ("COM0.wav", "reserva"),
            ("CONIN$.wav", "reserva"),
            ("Pista/a?b.wav", "?, que o Windows"),
        ] {
            let error = resolve(&root, path).unwrap_err();
            assert!(
                matches!(&error, PersistenceError::InvalidPath { reason: r, .. } if r.contains(reason)),
                "{path}: {error}"
            );
            assert!(error.to_string().starts_with("Caminho inválido"), "{error}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn paths_take_the_case_the_disk_has() {
        let root = std::env::temp_dir().join(format!("oxy-names-{}", new_id()));
        fs::create_dir_all(root.join("Pista")).unwrap();
        crate::fs_ops::create_new_file(&root.join("Pista").join("pulo.wav"), b"a").unwrap();
        assert_eq!(
            disk_case(&root, "PISTA/PULO.WAV").unwrap(),
            "Pista/pulo.wav"
        );
        assert_eq!(
            disk_case(&root, r"pista\novo.wav").unwrap(),
            "Pista/novo.wav"
        );
        assert_eq!(disk_case(&root, "Outra/x.wav").unwrap(), "Outra/x.wav");
        fs::remove_dir_all(root).unwrap();
    }
}
