//! What stops a script, as the person reads it: the file and line it points at and the message
//! in Portuguese. The most common Luau messages are translated at the boundary and the rest
//! pass as they come; no message carries the address of a Luau value.
use crate::runtime::ScriptLocation;
use std::fmt;

/// How far an error reaches (decision Q4 of 0.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptErrorKind {
    /// A refusal at load time or an error in a call: only that load or that call stops.
    Error,
    /// A limit of work or memory went past: the script stops on that object.
    Limit,
    /// The engine failed in a function scripts call; only that call stops.
    Engine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// The script file the error points at, as `Pista/porta.luau`.
    pub path: String,
    /// Its line, from 1; 0 when only the file is known.
    pub line: u32,
    /// One line in Portuguese, without the file and the line.
    pub message: String,
    pub kind: ScriptErrorKind,
}

impl ScriptError {
    pub(crate) fn new(path: &str, line: u32, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            line,
            message: message.into(),
            kind: ScriptErrorKind::Error,
        }
    }
    pub(crate) fn with_kind(mut self, kind: ScriptErrorKind) -> Self {
        self.kind = kind;
        self
    }
    /// The line the Console and agents open; none when only the file is known.
    pub fn location(&self) -> Option<ScriptLocation> {
        (self.line > 0).then(|| ScriptLocation {
            path: self.path.clone(),
            line: self.line,
        })
    }
    /// Whether the error stops the script on the object that ran it.
    pub fn stops_script(&self) -> bool {
        self.kind == ScriptErrorKind::Limit
    }
}

impl fmt::Display for ScriptError {
    /// `scripts/porta.luau:12: message`, or `scripts/porta.luau: message` without a line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "{}:{}: {}", self.path, self.line, self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// An error that reached the engine from Luau, placed at the script line it names (or that its
/// traceback names), else at `path:line`.
pub(crate) fn describe(error: &mlua::Error, path: &str, line: u32) -> ScriptError {
    let (place, message) = message_of(error);
    let (path, line) = place.unwrap_or_else(|| (path.to_owned(), line));
    ScriptError {
        path,
        line,
        message,
        kind: ScriptErrorKind::Error,
    }
}

type Place = Option<(String, u32)>;

fn message_of(error: &mlua::Error) -> (Place, String) {
    match error {
        mlua::Error::CallbackError { cause, traceback } => {
            let (place, message) = message_of(cause);
            (place.or_else(|| first_place(traceback)), message)
        }
        mlua::Error::WithContext { cause, .. } => message_of(cause),
        mlua::Error::RuntimeError(text) => from_text(text),
        mlua::Error::SyntaxError { message, .. } => from_text(message),
        mlua::Error::MemoryError(_) => (None, "faltou memória para os scripts".into()),
        mlua::Error::BadArgument { to, pos, cause, .. } => {
            let (place, reason) = message_of(cause);
            let function = to.as_deref().unwrap_or("?");
            (
                place,
                format!("argumento #{pos} de '{function}' inválido ({reason})"),
            )
        }
        mlua::Error::FromLuaConversionError { from, to, .. } => (
            None,
            format!("esperava {}, recebeu {}", type_word(to), type_word(from)),
        ),
        other => from_text(&other.to_string()),
    }
}

/// A Luau error text: its first line, without the location it starts with (kept apart) and
/// translated; the location may also come from a traceback below it.
fn from_text(text: &str) -> (Place, String) {
    let first = text.lines().next().unwrap_or_default();
    let (place, rest) = match split_place(first) {
        Some((path, line, rest)) => (Some((path, line)), rest),
        None => (None, first),
    };
    let place = place.or_else(|| first_place(text));
    (place, translate(&scrub(rest)))
}

/// `Pista/x.luau:12: rest` → the file, the line and the rest.
fn split_place(text: &str) -> Option<(String, u32, &str)> {
    let (path, line, after) = place_at(text)?;
    let rest = after.strip_prefix(':')?;
    Some((path, line, rest.strip_prefix(' ').unwrap_or(rest)))
}

/// A script file and line at the start of `text` (`Pista/Fase ação/x.luau:12`), with what
/// follows: a script's path, which never holds `:`, up to the first one.
fn place_at(text: &str) -> Option<(String, u32, &str)> {
    let end = text.find(':')?;
    let path = &text[..end];
    let stem = path.strip_suffix(".luau")?;
    if stem.is_empty() || stem.ends_with('/') || path.starts_with(['[', '=', ' ']) {
        return None;
    }
    let after = &text[end + 1..];
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    let line = after[..digits].parse().ok().filter(|&line| line > 0)?;
    Some((text[..end].to_owned(), line, &after[digits..]))
}

/// The first script line a traceback names (`Pista/x.luau:3 function f`).
fn first_place(text: &str) -> Place {
    text.lines()
        .filter_map(|line| place_at(line.trim()))
        .map(|(path, line, _)| (path, line))
        .next()
}

/// Luau writes values that have no `__tostring` as `table: 0x0000…`: the address goes, the kind
/// stays, in Portuguese when it is one of Luau's.
pub(crate) fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(": 0x") {
        let digits = rest[at + 4..]
            .bytes()
            .take_while(u8::is_ascii_hexdigit)
            .count();
        let before = &rest[..at];
        let word = before
            .bytes()
            .rev()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if digits == 0 || word == 0 {
            out.push_str(&rest[..at + 4]);
            rest = &rest[at + 4..];
            continue;
        }
        let (head, kind) = before.split_at(before.len() - word);
        out.push_str(head);
        out.push_str(match kind {
            "table" => "tabela",
            "function" => "função",
            "thread" => "corrotina",
            other => other,
        });
        rest = &rest[at + 4 + digits..];
    }
    out.push_str(rest);
    out
}

/// A Luau type name in Portuguese.
fn type_word(name: &str) -> String {
    match name {
        "boolean" => "booleano",
        "number" => "número",
        "string" => "texto",
        "table" => "tabela",
        "function" => "função",
        "thread" => "corrotina",
        "vector" => "vetor",
        other => other,
    }
    .to_owned()
}

/// "um valor nil", "uma tabela", "um número"…
fn a_value(name: &str) -> String {
    match name {
        "nil" => "um valor nil".into(),
        "table" | "function" | "thread" => format!("uma {}", type_word(name)),
        other => format!("um {}", type_word(other)),
    }
}

/// What follows an error of walking or copying an object, its attributes or its memory.
const OBJECT_HINT: &str = "; objetos, self.attr e self.state não se percorrem nem se copiam: \
                           guarde uma lista num campo de self.state (veja “Memória do script” no \
                           Guia)";

/// The Luau runtime messages scripts meet most, in Portuguese; others pass as they come.
fn translate(text: &str) -> String {
    let quoted = |text: &str| {
        text.strip_prefix('\'')
            .and_then(|t| t.strip_suffix('\''))
            .map(str::to_owned)
    };
    if let Some(rest) = text.strip_prefix("attempt to index ")
        && let Some((what, key)) = rest.split_once(" with ")
    {
        return match quoted(key) {
            Some(name) => format!("tentou ler '{name}' de {}", a_value(what)),
            None => format!(
                "tentou ler um índice do tipo {} de {}",
                type_word(key),
                a_value(what)
            ),
        };
    }
    if let Some(rest) = text.strip_prefix("attempt to call missing method ")
        && let Some((method, what)) = rest.split_once(" of ")
    {
        return format!(
            "tentou chamar o método {method}, que {} não tem",
            a_value(what)
        );
    }
    if let Some(rest) = text.strip_prefix("attempt to call a ")
        && let Some(what) = rest.strip_suffix(" value")
    {
        return format!("tentou chamar {}", a_value(what));
    }
    if let Some(rest) = text.strip_prefix("attempt to iterate over a ")
        && let Some(what) = rest.strip_suffix(" value")
    {
        let hint = if what == "Object" { OBJECT_HINT } else { "" };
        return format!("tentou percorrer {} num for{hint}", a_value(what));
    }
    if let Some(rest) = text.strip_prefix("attempt to perform arithmetic (")
        && let Some((operation, values)) = rest.split_once(") on ")
    {
        return match values.split_once(" and ") {
            Some((a, b)) => format!(
                "tentou fazer uma conta ({operation}) com {} e {}",
                a_value(a),
                a_value(b)
            ),
            None => format!(
                "tentou fazer uma conta ({operation}) com {}",
                a_value(values)
            ),
        };
    }
    if let Some(rest) = text.strip_prefix("attempt to compare ") {
        let parts: Vec<&str> = rest.split(' ').collect();
        if let [a, operator, b] = parts.as_slice() {
            return format!("tentou comparar {} {operator} {}", a_value(a), a_value(b));
        }
    }
    if let Some(rest) = text.strip_prefix("attempt to concatenate ")
        && let Some((a, b)) = rest.split_once(" with ")
    {
        return format!("tentou juntar {} com {} (..)", a_value(a), a_value(b));
    }
    if let Some(rest) = text.strip_prefix("invalid argument #")
        && let Some((number, rest)) = rest.split_once(" to ")
        && let Some((function, reason)) = rest.split_once(" (")
    {
        let reason = reason.strip_suffix(')').unwrap_or(reason);
        return match reason.split_once(" expected, got ") {
            Some((expected, got)) => format!(
                "argumento #{number} de {function} inválido: esperava {}, recebeu {}{}",
                type_word(expected),
                type_word(got),
                if got == "Object" { OBJECT_HINT } else { "" }
            ),
            None => format!("argumento #{number} de {function} inválido ({reason})"),
        };
    }
    if let Some(rest) = text.strip_prefix("missing argument #")
        && let Some((number, rest)) = rest.split_once(" to ")
        && let Some((function, expected)) = rest.split_once(" (")
    {
        let expected = expected.strip_suffix(" expected)").unwrap_or(expected);
        return format!(
            "falta o argumento #{number} de {function} ({})",
            type_word(expected)
        );
    }
    match text {
        "attempt to modify a readonly table" => {
            "tentou alterar uma tabela travada (o módulo e o que ele alcança são só leitura)".into()
        }
        "stack overflow" | "C stack overflow" => {
            "a pilha de chamadas estourou (uma função que chama a si mesma sem parar?)".into()
        }
        "table index is nil" => "tentou usar nil como índice de tabela".into(),
        "table index is NaN" => "tentou usar NaN como índice de tabela".into(),
        "cannot resume dead coroutine" => "a corrotina já terminou".into(),
        other => other.to_owned(),
    }
}

/// The name in `candidates` closest to `name`, when it is close enough to be a typo.
pub(crate) fn closest<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).clamp(1, 3);
    candidates
        .into_iter()
        .map(|candidate| (distance(name, candidate), candidate))
        .filter(|&(d, _)| d <= limit)
        .min()
        .map(|(_, candidate)| candidate)
}

/// Edit distance with swaps of neighbours (optimal string alignment).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luau_messages_read_in_portuguese_without_addresses() {
        for (luau, ours) in [
            (
                "attempt to index nil with 'abrir'",
                "tentou ler 'abrir' de um valor nil",
            ),
            (
                "attempt to index number with number",
                "tentou ler um índice do tipo número de um número",
            ),
            ("attempt to call a nil value", "tentou chamar um valor nil"),
            (
                "attempt to call missing method 'mover' of table",
                "tentou chamar o método 'mover', que uma tabela não tem",
            ),
            (
                "attempt to perform arithmetic (add) on nil and number",
                "tentou fazer uma conta (add) com um valor nil e um número",
            ),
            (
                "attempt to compare nil < number",
                "tentou comparar um valor nil < um número",
            ),
            (
                "attempt to concatenate string with table",
                "tentou juntar um texto com uma tabela (..)",
            ),
            (
                "attempt to iterate over a table value",
                "tentou percorrer uma tabela num for",
            ),
            (
                "invalid argument #1 to 'floor' (number expected, got string)",
                "argumento #1 de 'floor' inválido: esperava número, recebeu texto",
            ),
            (
                "missing argument #2 to 'clamp' (number expected)",
                "falta o argumento #2 de 'clamp' (número)",
            ),
            (
                "attempt to modify a readonly table",
                "tentou alterar uma tabela travada (o módulo e o que ele alcança são só leitura)",
            ),
            ("algo novo do Luau", "algo novo do Luau"),
        ] {
            assert_eq!(translate(luau), ours);
        }
        // Walking or copying an object points to where lists live.
        assert_eq!(
            translate("attempt to iterate over a Object value"),
            format!("tentou percorrer um Object num for{OBJECT_HINT}")
        );
        assert_eq!(
            translate("invalid argument #1 to 'pairs' (table expected, got Object)"),
            format!(
                "argumento #1 de 'pairs' inválido: esperava tabela, recebeu Object{OBJECT_HINT}"
            )
        );
        assert_eq!(
            scrub("table: 0x000001f2a3b4c5d6 e function: 0xABC, thread: 0x1 · Foo: 0x2 · x: 0x"),
            "tabela e função, corrotina · Foo · x: 0x"
        );
    }

    #[test]
    fn errors_keep_the_line_they_name_or_the_traceback_names() {
        let error = mlua::Error::RuntimeError(
            "scripts/porta.luau:12: attempt to index nil with 'abrir'\nstack traceback:\n\
             scripts/porta.luau:12 function on_click"
                .into(),
        );
        let e = describe(&error, "scripts/outro.luau", 3);
        assert_eq!(
            e.to_string(),
            "scripts/porta.luau:12: tentou ler 'abrir' de um valor nil"
        );
        assert_eq!(
            e.location(),
            Some(ScriptLocation {
                path: "scripts/porta.luau".into(),
                line: 12
            })
        );
        let error = mlua::Error::RuntimeError(
            "table: 0x00000123\nstack traceback:\n[C] function error\nscripts/a/b.luau:7 function f"
                .into(),
        );
        assert_eq!(
            describe(&error, "scripts/x.luau", 1).to_string(),
            "scripts/a/b.luau:7: tabela"
        );
        let error = mlua::Error::RuntimeError("sem lugar".into());
        let e = describe(&error, "scripts/x.luau", 0);
        assert_eq!(
            (e.to_string(), e.location()),
            ("scripts/x.luau: sem lugar".into(), None)
        );
        // A script in any folder of the project, the root too, spaces and accents in its path.
        for path in [
            "Pista/Fase ação/porta.luau",
            "porta.luau",
            "Pista/Checkpoints/a.b.luau",
        ] {
            let error = mlua::Error::RuntimeError(format!(
                "{path}:12: attempt to call a nil value\nstack traceback:\n{path}:12 function f"
            ));
            let e = describe(&error, "scripts/x.luau", 3);
            assert_eq!(
                e.to_string(),
                format!("{path}:12: tentou chamar um valor nil")
            );
            let error = mlua::Error::RuntimeError(format!(
                "tabela\nstack traceback:\n[C] function error\n{path}:5 function f"
            ));
            assert_eq!(describe(&error, "x.luau", 1).location().unwrap().path, path);
        }
        // What only looks like a place stays in the message.
        for text in [
            "erro: porta.luau:3: x",
            "[string \"porta.luau\"]:3: x",
            ".luau:3: x",
            "Pista/porta.lua:3: x",
            "porta.luau: x",
        ] {
            let error = mlua::Error::RuntimeError(text.into());
            let e = describe(&error, "scripts/x.luau", 2);
            assert_eq!(e.location().unwrap().path, "scripts/x.luau", "{text}");
        }
        assert_eq!(closest("prnt", ["print", "pairs", "type"]), Some("print"));
        assert_eq!(
            closest("has_atribute", ["exists", "has_attribute"]),
            Some("has_attribute")
        );
        assert_eq!(closest("xyz", ["print"]), None);
    }
}
