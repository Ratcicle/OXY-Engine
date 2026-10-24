//! The `require` of the scripts as the tree's commands move them: a script moved or renamed, or
//! in a folder moved, renamed or deleted, still finds its modules, and the scripts that require
//! it still find it. Each `require` written with a whole text that no longer finds its module is
//! written `@project/…`, the module's path from the project's folder: in the scripts that
//! require what moved, and in the moved scripts themselves, whose `./` and `../` count from
//! another folder now. Each script rewritten is a file operation of the command's step
//! ([`FileOp::Write`]), journaled and undone with it. What no rewrite can follow is told with its
//! file and line: a path the script builds, `require` used as a value (`pcall(require, …)`), a
//! script that also declares a `require` of its own (its calls may not be the engine's), a
//! module sent to the trash, a script that cannot be read.
use super::TreeView;
use crate::{
    edit_history::FileOp,
    journal::{join, under},
    project_tree::FileKind,
    script::{
        self, ScriptFile,
        lexer::{self, RequireKind},
    },
};
use std::ops::Range;

/// What a `require` built from pieces gets when scripts move.
const BUILT: &str = "require com caminho montado não é atualizado quando a OXY move arquivos; \
    confira se ele ainda acha o módulo";
/// What a `require` used as a value, not called, gets when scripts move.
const PASSED: &str = "require usado como valor (como em pcall(require, …) ou local r = require) \
    não é atualizado quando a OXY move arquivos; confira se ele ainda acha o módulo";

/// Where a module a script required before the command is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Moved {
    /// At this path (the same one, when it stayed).
    To(String),
    /// In the project's trash.
    Trashed,
    /// It was no script of the tree: the `require` did not find it before either.
    Unknown,
}

/// What moving a script does to its text (see [`rewrite`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rewrite {
    /// The text with the `require` that no longer find their modules written again; none when
    /// none changed.
    pub text: Option<String>,
    /// What could not be followed, by line.
    pub told: Vec<(u32, String)>,
}

/// What the script `from`, now at `to` (the same path when it stayed), whose text is `text`,
/// becomes when the modules it required are where `moved` says: each `require` with a whole text
/// that no longer finds its module written `@project/…` in the quotes it had; a `require` that
/// did not find its module before stays as it is. Every `require` built from pieces or used as a
/// value is told, and so is each one to write again in a script that declares a `require` of its
/// own, which stays as it is.
pub(crate) fn rewrite(text: &str, from: &str, to: &str, moved: impl Fn(&str) -> Moved) -> Rewrite {
    let found = lexer::requires(text);
    let declared = found
        .iter()
        .any(|require| require.kind == RequireKind::Declared);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut told = Vec::new();
    for require in &found {
        let (path, span) = match &require.kind {
            RequireKind::Literal { path, span } => (path, span),
            RequireKind::Built => {
                told.push((require.line, BUILT.to_owned()));
                continue;
            }
            RequireKind::Passed => {
                told.push((require.line, PASSED.to_owned()));
                continue;
            }
            RequireKind::Declared => continue,
        };
        let Ok(target) = script::require_path(from, path) else {
            continue;
        };
        let module = match moved(&target) {
            Moved::To(module) => module,
            Moved::Trashed => {
                told.push((
                    require.line,
                    format!("require(\"{path}\") pede {target}, que foi para a lixeira do projeto"),
                ));
                continue;
            }
            Moved::Unknown => continue,
        };
        if script::require_path(to, path).is_ok_and(|found| found == module) {
            continue;
        }
        let stem = module.strip_suffix(".luau").unwrap_or(&module);
        let written = format!("@{}/{stem}", script::PROJECT_ALIAS);
        if declared {
            told.push((
                require.line,
                format!(
                    "require(\"{path}\") não foi atualizado, porque o script também declara um \
                     require seu (nome local, função ou parâmetro) e esta chamada pode não ser a \
                     da OXY; se for, escreva require(\"{written}\")"
                ),
            ));
            continue;
        }
        edits.push((
            span.clone(),
            lexer::quoted_like(&text[span.clone()], &written),
        ));
    }
    let text = (!edits.is_empty()).then(|| {
        let mut out = text.to_owned();
        for (span, written) in edits.iter().rev() {
            out.replace_range(span.clone(), written);
        }
        out
    });
    Rewrite { text, told }
}

/// Follows, in the scripts of `view`, what the file operations planned since the `start`th
/// moved or sent to the trash: each script whose `require` no longer find their modules is
/// rewritten (an operation of its own, after them, and its text in the view), and what cannot
/// be followed is answered, one line each, as `Pista/a.luau:3: …`. Nothing when no script moved
/// nor went to the trash.
pub(super) fn follow(view: &mut TreeView, start: usize) -> Vec<String> {
    let mut moves: Vec<(String, String)> = Vec::new();
    let mut trashed: Vec<String> = Vec::new();
    for op in &view.files[start..] {
        match op {
            FileOp::Move { from, to } => moves.push((from.clone(), to.clone())),
            FileOp::Trash { from, .. } => trashed.push(from.clone()),
            _ => {}
        }
    }
    // Where a path before the operations is after them, and the other way round.
    let forth = |path: &str| {
        let mut path = path.to_owned();
        for (from, to) in &moves {
            if let Some(rest) = under(&path, from) {
                path = join(to, rest);
            }
        }
        path
    };
    let back = |path: &str| {
        let mut path = path.to_owned();
        for (from, to) in moves.iter().rev() {
            if let Some(rest) = under(&path, to) {
                path = join(from, rest);
            }
        }
        path
    };
    let scripts = &view.scripts;
    let exists =
        |path: &str| scripts.files.contains_key(path) || scripts.unreadable.contains_key(path);
    let trashed: Vec<String> = trashed
        .iter()
        .filter(|path| FileKind::of(path) == FileKind::Script)
        .map(|path| back(path))
        .collect();
    let moved_any = scripts
        .files
        .keys()
        .chain(scripts.unreadable.keys())
        .any(|path| back(path) != *path);
    if !moved_any && trashed.is_empty() {
        return Vec::new();
    }
    let moved = |target: &str| {
        let now = forth(target);
        if exists(&now) && back(&now) == target {
            Moved::To(now)
        } else if trashed.iter().any(|gone| gone == target) {
            Moved::Trashed
        } else {
            Moved::Unknown
        }
    };
    let mut told = Vec::new();
    let mut writes: Vec<(String, String, String)> = Vec::new();
    for (path, file) in &scripts.files {
        let done = rewrite(&file.text, &back(path), path, moved);
        told.extend(
            done.told
                .into_iter()
                .map(|(line, why)| format!("{path}:{line}: {why}")),
        );
        match done.text {
            Some(text) if text.len() as u64 > script::MAX_BYTES => told.push(format!(
                "{path}: os require dele não foram atualizados, porque ele passaria de 1 MB"
            )),
            Some(text) => writes.push((path.clone(), file.text.clone(), text)),
            None => {}
        }
    }
    for (path, why) in &scripts.unreadable {
        told.push(format!("{path} {why}; a OXY não conferiu os require dele"));
    }
    for (path, before, after) in writes {
        view.scripts
            .files
            .insert(path.clone(), ScriptFile::new(after.clone()));
        view.plan(FileOp::Write {
            path,
            before: before.into(),
            after: after.into(),
        });
    }
    // Once a batch: a later command that moves scripts again finds the same.
    told.retain(|line| view.told.insert(line.clone()));
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Where each module of `moves` (before → after) is; the others of `known` stayed.
    fn place<'a>(
        known: &'a [&'a str],
        moves: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Moved + 'a {
        move |target: &str| {
            let moved: BTreeMap<&str, &str> = moves.iter().copied().collect();
            match moved.get(target) {
                Some(to) => Moved::To((*to).to_owned()),
                None if known.contains(&target) => Moved::To(target.to_owned()),
                None => Moved::Unknown,
            }
        }
    }

    #[test]
    fn a_module_moved_is_found_by_every_script_that_required_it() {
        let moves = [("Pista/dados.luau", "Comum/dados.luau")];
        let moved = place(&[], &moves);
        for (from, text, expected) in [
            (
                "Pista/a.luau",
                "local d = require(\"./dados\")\n",
                "local d = require(\"@project/Comum/dados\")\n",
            ),
            (
                "Menu/b.luau",
                "local d = require '../Pista/dados' -- dados\n",
                "local d = require '@project/Comum/dados' -- dados\n",
            ),
            (
                "c.luau",
                "return require([[@Project/Pista/dados]]).x",
                "return require(\"@project/Comum/dados\").x",
            ),
        ] {
            let done = rewrite(text, from, from, &moved);
            assert_eq!(done.text.as_deref(), Some(expected), "{from}");
            assert!(done.told.is_empty(), "{from}: {:?}", done.told);
        }
    }

    #[test]
    fn a_script_moved_keeps_finding_its_modules_and_what_still_finds_them_stays() {
        let known = ["Comum/sons.luau", "Pista/dados.luau"];
        let text = "local a = require('../Comum/sons')\n\
                    local b = require('@project/Pista/dados')\n\
                    local c = require('./dados')\n\
                    local d = require('./naoexiste')\n";
        // Up to the root: `../` would leave the project, `./dados` names another folder.
        let done = rewrite(text, "Pista/a.luau", "a.luau", place(&known, &[]));
        assert_eq!(
            done.text.as_deref(),
            Some(
                "local a = require('@project/Comum/sons')\n\
                 local b = require('@project/Pista/dados')\n\
                 local c = require('@project/Pista/dados')\n\
                 local d = require('./naoexiste')\n"
            )
        );
        // A folder moved whole, its modules with it: nothing to change.
        let moves = [
            ("Pista/dados.luau", "Fases/Pista/dados.luau"),
            ("Comum/sons.luau", "Comum/sons.luau"),
        ];
        let done = rewrite(
            "return require('./dados')",
            "Pista/a.luau",
            "Fases/Pista/a.luau",
            place(&known, &moves),
        );
        assert_eq!(done, Rewrite::default());
    }

    #[test]
    fn what_cannot_be_followed_is_told_by_line() {
        let moves = [("Pista/dados.luau", "Comum/dados.luau")];
        let built = "local nome = 'dados'\nreturn require('./' .. nome)\n";
        let done = rewrite(built, "Pista/a.luau", "Pista/a.luau", place(&[], &moves));
        assert_eq!(done.text, None);
        assert_eq!(done.told, [(2, BUILT.to_owned())]);
        // Used as a value, its calls cannot be followed; the calls of require itself are.
        let passed = "local ok, d = pcall(require, './dados')\nreturn d";
        let done = rewrite(passed, "Pista/a.luau", "Pista/a.luau", place(&[], &moves));
        assert_eq!(done.text, None);
        assert_eq!(done.told, [(1, PASSED.to_owned())]);
        let kept = "local r = require\nreturn require('./dados')\n";
        let done = rewrite(kept, "Pista/a.luau", "Pista/a.luau", place(&[], &moves));
        assert_eq!(
            done.text.as_deref(),
            Some("local r = require\nreturn require('@project/Comum/dados')\n")
        );
        assert_eq!(done.told, [(1, PASSED.to_owned())]);
        // A script that declares a require of its own keeps its calls as they are.
        let declared = "local function usar(require)\n    return require('./dados')\nend\n";
        let done = rewrite(declared, "Pista/a.luau", "Pista/a.luau", place(&[], &moves));
        assert_eq!(done.text, None);
        assert_eq!(done.told.len(), 1);
        assert_eq!(done.told[0].0, 2);
        assert!(
            done.told[0]
                .1
                .contains("se for, escreva require(\"@project/Comum/dados\")"),
            "{:?}",
            done.told
        );
        // A module sent to the trash.
        let done = rewrite(
            "return require('./dados')",
            "Pista/a.luau",
            "Pista/a.luau",
            |_: &str| Moved::Trashed,
        );
        assert_eq!(
            done.told,
            [(
                1,
                "require(\"./dados\") pede Pista/dados.luau, que foi para a lixeira do projeto"
                    .to_owned()
            )]
        );
    }
}
