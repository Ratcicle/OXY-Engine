//! A small lexer of Luau: names, numbers, texts (in quotes, in long brackets and between
//! backticks, whose `{}` parts are read as code), comments and symbols, each with its place in
//! the source and its line. It tells exactly where a script names the modules it requires
//! ([`requires`]), which moving a script rewrites, and what the script viewer colors.
use std::ops::Range;

/// What a [`Token`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Name,
    Number,
    /// A text written whole: in quotes, in long brackets (`[[…]]`, `[==[…]==]`) or between
    /// backticks without `{}`.
    Text,
    /// A piece of a text between backticks that has `{}`: from its start or a `}` to the next
    /// `{` or its end. The code between the braces is tokens of its own.
    Interpolated,
    Comment,
    Symbol,
    /// A text or a comment that never ends, or a character Luau does not read.
    Broken,
}

/// A piece of a Luau source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Where it is in the source, in bytes.
    pub span: Range<usize>,
    /// The line it starts at, from 1.
    pub line: u32,
}

/// The tokens of `source`, in order; the spaces and line breaks between them are left out.
pub fn tokens(source: &str) -> Vec<Token> {
    let mut lexer = Lexer {
        source: source.as_bytes(),
        at: 0,
        line: 1,
        braces: Vec::new(),
        tokens: Vec::new(),
    };
    lexer.run();
    lexer.tokens
}

/// The symbols of more than one character, the longest first.
const SYMBOLS: [&str; 17] = [
    "...", "..=", "//=", "..", "::", "==", "~=", "<=", ">=", "->", "//", "+=", "-=", "*=", "/=",
    "%=", "^=",
];

struct Lexer<'a> {
    source: &'a [u8],
    at: usize,
    line: u32,
    /// For each text between backticks whose `{}` part is being read, the braces open in it.
    braces: Vec<usize>,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.source.get(self.at + ahead).copied()
    }

    /// Moves on to `end`, counting the line breaks passed.
    fn advance(&mut self, end: usize) {
        let end = end.min(self.source.len());
        self.line += self.source[self.at..end]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count() as u32;
        self.at = end;
    }

    fn push(&mut self, kind: TokenKind, start: usize, line: u32) {
        self.tokens.push(Token {
            kind,
            span: start..self.at,
            line,
        });
    }

    fn run(&mut self) {
        while let Some(byte) = self.peek(0) {
            let (start, line) = (self.at, self.line);
            let kind = match byte {
                b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => {
                    self.advance(self.at + 1);
                    continue;
                }
                b'-' if self.peek(1) == Some(b'-') => {
                    self.advance(self.at + 2);
                    match self.long_bracket() {
                        Some(level) => self.long(level, TokenKind::Comment),
                        None => {
                            let end = self.source[self.at..]
                                .iter()
                                .position(|&byte| byte == b'\n' || byte == b'\r')
                                .map_or(self.source.len(), |at| self.at + at);
                            self.advance(end);
                            TokenKind::Comment
                        }
                    }
                }
                b'"' | b'\'' => self.quoted(byte),
                b'[' if self.long_bracket().is_some() => {
                    let level = self.long_bracket().unwrap_or_default();
                    self.long(level, TokenKind::Text)
                }
                b'`' => self.backtick(true),
                b'}' if self.braces.last() == Some(&0) => {
                    self.braces.pop();
                    self.backtick(false)
                }
                b'0'..=b'9' => self.number(),
                b'.' if self.peek(1).is_some_and(|next| next.is_ascii_digit()) => self.number(),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    let end = self.source[self.at..]
                        .iter()
                        .position(|&byte| !(byte.is_ascii_alphanumeric() || byte == b'_'))
                        .map_or(self.source.len(), |at| self.at + at);
                    self.advance(end);
                    TokenKind::Name
                }
                _ if byte.is_ascii_punctuation() => {
                    let rest = &self.source[self.at..];
                    let length = SYMBOLS
                        .iter()
                        .find(|symbol| rest.starts_with(symbol.as_bytes()))
                        .map_or(1, |symbol| symbol.len());
                    match (byte, self.braces.last_mut()) {
                        (b'{', Some(open)) => *open += 1,
                        (b'}', Some(open)) => *open = open.saturating_sub(1),
                        _ => {}
                    }
                    self.advance(self.at + length);
                    TokenKind::Symbol
                }
                _ => {
                    // A whole character, never part of one.
                    let length = std::str::from_utf8(&self.source[self.at..])
                        .ok()
                        .and_then(|rest| rest.chars().next())
                        .map_or(1, char::len_utf8);
                    self.advance(self.at + length);
                    TokenKind::Broken
                }
            };
            self.push(kind, start, line);
        }
    }

    /// The level of the long bracket opening at the current place (`[[` is 0, `[==[` is 2).
    fn long_bracket(&self) -> Option<usize> {
        if self.peek(0) != Some(b'[') {
            return None;
        }
        let level = self.source[self.at + 1..]
            .iter()
            .take_while(|&&byte| byte == b'=')
            .count();
        (self.peek(level + 1) == Some(b'[')).then_some(level)
    }

    /// Reads a long text or comment of `level` to its closing bracket: `kind`, or broken when it
    /// never closes.
    fn long(&mut self, level: usize, kind: TokenKind) -> TokenKind {
        self.advance(self.at + level + 2);
        let mut closing = vec![b']'];
        closing.extend(std::iter::repeat_n(b'=', level));
        closing.push(b']');
        match self.source[self.at..]
            .windows(closing.len())
            .position(|window| window == closing.as_slice())
        {
            Some(at) => {
                self.advance(self.at + at + closing.len());
                kind
            }
            None => {
                self.advance(self.source.len());
                TokenKind::Broken
            }
        }
    }

    /// Passes the escape that starts at the current `\`: the character after it, a line break
    /// written as one (`\r\n`), or `\z` with the spaces and line breaks after it.
    fn escape(&mut self) {
        match self.peek(1) {
            Some(b'\r') if self.peek(2) == Some(b'\n') => self.advance(self.at + 3),
            Some(b'z') => {
                self.advance(self.at + 2);
                while self.peek(0).is_some_and(|byte| byte.is_ascii_whitespace()) {
                    self.advance(self.at + 1);
                }
            }
            Some(_) => self.advance(self.at + 2),
            None => self.advance(self.at + 1),
        }
    }

    /// Reads a text in `quote`s; one that a line break or the end of the source cuts is broken,
    /// ending before the break.
    fn quoted(&mut self, quote: u8) -> TokenKind {
        self.advance(self.at + 1);
        while let Some(byte) = self.peek(0) {
            match byte {
                b'\\' => self.escape(),
                b'\r' | b'\n' => return TokenKind::Broken,
                _ if byte == quote => {
                    self.advance(self.at + 1);
                    return TokenKind::Text;
                }
                _ => self.advance(self.at + 1),
            }
        }
        TokenKind::Broken
    }

    /// Reads a text between backticks from its start (`opens`) or from the `}` that ends one of
    /// its `{}` parts, to its end or its next `{`, after which its code is read as any other.
    fn backtick(&mut self, opens: bool) -> TokenKind {
        self.advance(self.at + 1);
        while let Some(byte) = self.peek(0) {
            match byte {
                b'\\' => self.escape(),
                b'\r' | b'\n' => return TokenKind::Broken,
                b'`' => {
                    self.advance(self.at + 1);
                    return if opens {
                        TokenKind::Text
                    } else {
                        TokenKind::Interpolated
                    };
                }
                b'{' => {
                    self.advance(self.at + 1);
                    self.braces.push(0);
                    return TokenKind::Interpolated;
                }
                _ => self.advance(self.at + 1),
            }
        }
        TokenKind::Broken
    }

    /// Reads a number as Luau does, a malformed one whole: digits, letters, `_` and `.`, and the
    /// sign of a decimal exponent.
    fn number(&mut self) -> TokenKind {
        let hex = self.peek(0) == Some(b'0') && matches!(self.peek(1), Some(b'x' | b'X'));
        while let Some(byte) = self.peek(0) {
            let sign = matches!(byte, b'+' | b'-')
                && !hex
                && matches!(self.source[self.at - 1], b'e' | b'E');
            if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || sign {
                self.advance(self.at + 1);
            } else {
                break;
            }
        }
        TokenKind::Number
    }
}

/// What a [`TokenKind::Text`] token of `source` says, its escapes read as Luau reads them; none
/// for another kind of token, or an escape Luau refuses.
pub fn text(source: &str, token: &Token) -> Option<String> {
    if token.kind != TokenKind::Text {
        return None;
    }
    let raw = source.get(token.span.clone())?;
    match raw.as_bytes().first()? {
        b'"' | b'\'' | b'`' => unescape(raw.get(1..raw.len() - 1)?),
        b'[' => {
            let level = raw[1..].bytes().take_while(|&byte| byte == b'=').count();
            let inner = raw.get(level + 2..raw.len() - level - 2)?;
            // A line break right after the opening bracket is not part of the text.
            let inner = ["\r\n", "\n", "\r"]
                .iter()
                .find_map(|eol| inner.strip_prefix(eol))
                .unwrap_or(inner);
            Some(inner.to_owned())
        }
        _ => None,
    }
}

/// The text the inside of a quoted text `raw` says.
fn unescape(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    let hex = |byte: u8| char::from(byte).to_digit(16);
    while let Some(&byte) = bytes.get(at) {
        at += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let escaped = *bytes.get(at)?;
        at += 1;
        match escaped {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' | b'\n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'\\' | b'"' | b'\'' | b'`' | b'{' => out.push(escaped),
            b'\r' => {
                out.push(b'\n');
                if bytes.get(at) == Some(&b'\n') {
                    at += 1;
                }
            }
            b'z' => {
                while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                    at += 1;
                }
            }
            b'x' => {
                let high = hex(*bytes.get(at)?)?;
                let low = hex(*bytes.get(at + 1)?)?;
                out.push((high * 16 + low) as u8);
                at += 2;
            }
            b'u' => {
                if bytes.get(at) != Some(&b'{') {
                    return None;
                }
                let end = at + bytes[at..].iter().position(|&byte| byte == b'}')?;
                let code = u32::from_str_radix(raw.get(at + 1..end)?, 16).ok()?;
                let mut buffer = [0; 4];
                out.extend(char::from_u32(code)?.encode_utf8(&mut buffer).bytes());
                at = end + 1;
            }
            b'0'..=b'9' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    match bytes.get(at) {
                        Some(digit @ b'0'..=b'9') => {
                            value = value * 10 + u32::from(digit - b'0');
                            at += 1;
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(value).ok()?);
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

/// How a script names a module it requires (see [`requires`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequireKind {
    /// With a text written whole: the path it says, and where the text is in the source, quotes
    /// included, which a rewrite replaces.
    Literal { path: String, span: Range<usize> },
    /// With a path the script builds (joined texts, a name, a text with `{}`), which no rewrite
    /// can follow.
    Built,
    /// `require` declared anew (a local, a function, a parameter or a loop's name) or given a
    /// value, so a call of it may not be the engine's.
    Declared,
    /// `require` used as a value, not called: passed on (`pcall(require, "./dados")`), kept in
    /// another name (`local r = require`) or returned, whose calls no rewrite can follow.
    Passed,
}

/// A place where a script names `require`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Require {
    pub kind: RequireKind,
    /// The line of the name `require`.
    pub line: u32,
}

/// Every place where `source` names the global `require`, in order: called with a text written
/// whole (`require("./dados")`, `require "./dados"`, `require [[./dados]]`), called with a path
/// it builds, declared anew or used as a value. A field or method called `require` of another
/// table is none, and so is a key of a table written `{ require = … }`; comments and texts are
/// no code.
pub fn requires(source: &str) -> Vec<Require> {
    let tokens: Vec<Token> = tokens(source)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Comment)
        .collect();
    let says = |at: usize, kind: TokenKind, what: &str| {
        tokens
            .get(at)
            .is_some_and(|token| token.kind == kind && &source[token.span.clone()] == what)
    };
    // The bracket the token at `at` is in, none at the top.
    let enclosing = |at: usize| {
        let mut depth = 0usize;
        for before in (0..at).rev() {
            if tokens[before].kind != TokenKind::Symbol {
                continue;
            }
            match &source[tokens[before].span.clone()] {
                ")" | "]" | "}" => depth += 1,
                "(" | "[" | "{" if depth == 0 => return Some(before),
                "(" | "[" | "{" => depth -= 1,
                _ => {}
            }
        }
        None
    };
    // Whether the `(` at `open` starts the parameters of a function: `function(`,
    // `function nome(`, `function M.a:b(`.
    let parameters = |open: usize| {
        let mut at = open;
        loop {
            let Some(before) = at.checked_sub(1) else {
                return false;
            };
            if says(before, TokenKind::Name, "function") {
                return true;
            }
            if tokens[before].kind != TokenKind::Name {
                return false;
            }
            match before.checked_sub(1) {
                Some(dot)
                    if says(dot, TokenKind::Symbol, ".") || says(dot, TokenKind::Symbol, ":") =>
                {
                    at = dot;
                }
                Some(previous) => return says(previous, TokenKind::Name, "function"),
                None => return false,
            }
        }
    };
    let literal = |token: &Token| match text(source, token) {
        Some(path) => RequireKind::Literal {
            path,
            span: token.span.clone(),
        },
        None => RequireKind::Built,
    };
    let mut found = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if !says(at, TokenKind::Name, "require") {
            continue;
        }
        let before = at.checked_sub(1);
        if before.is_some_and(|before| {
            says(before, TokenKind::Symbol, ".") || says(before, TokenKind::Symbol, ":")
        }) {
            continue;
        }
        let assigned = says(at + 1, TokenKind::Symbol, "=");
        let open = enclosing(at);
        // A key of a table or of a table's type: `{ require = 1 }`, `{ a = 1, require: T }`.
        if open.is_some_and(|open| says(open, TokenKind::Symbol, "{"))
            && (assigned || says(at + 1, TokenKind::Symbol, ":"))
        {
            continue;
        }
        let declared = assigned
            || before.is_some_and(|before| {
                ["local", "function", "for"]
                    .iter()
                    .any(|word| says(before, TokenKind::Name, word))
            })
            || open.is_some_and(|open| says(open, TokenKind::Symbol, "(") && parameters(open));
        let next = tokens.get(at + 1).map(|token| token.kind);
        let kind = if declared {
            RequireKind::Declared
        } else if next == Some(TokenKind::Text) {
            literal(&tokens[at + 1])
        } else if next == Some(TokenKind::Interpolated) || says(at + 1, TokenKind::Symbol, "{") {
            RequireKind::Built
        } else if says(at + 1, TokenKind::Symbol, "(") {
            match tokens.get(at + 2) {
                Some(argument)
                    if argument.kind == TokenKind::Text && says(at + 3, TokenKind::Symbol, ")") =>
                {
                    literal(argument)
                }
                _ => RequireKind::Built,
            }
        } else {
            RequireKind::Passed
        };
        found.push(Require {
            kind,
            line: token.line,
        });
    }
    found
}

/// `path` as a Luau text in the quotes of `like`, the text it replaces: the same quote when
/// it was one, else double quotes.
pub fn quoted_like(like: &str, path: &str) -> String {
    let quote = match like.as_bytes().first() {
        Some(b'\'') => '\'',
        _ => '"',
    };
    let mut written = String::with_capacity(path.len() + 2);
    written.push(quote);
    for c in path.chars() {
        match c {
            '\\' => written.push_str("\\\\"),
            c if c == quote => {
                written.push('\\');
                written.push(c);
            }
            c => written.push(c),
        }
    }
    written.push(quote);
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tokens of `source` as (kind, text).
    fn read(source: &str) -> Vec<(TokenKind, &str)> {
        tokens(source)
            .into_iter()
            .map(|token| (token.kind, &source[token.span]))
            .collect()
    }

    #[test]
    fn comments_texts_and_symbols_are_read_whole() {
        use TokenKind::*;
        let source = "local a = 1 --[[ require('./x') ]] .. 'b' -- require(\"y\")\n\
                      x = [==[ ]] require('z') ]==] ... a.b:c(0x1F, 1.5e-3, .5)";
        assert_eq!(
            read(source),
            [
                (Name, "local"),
                (Name, "a"),
                (Symbol, "="),
                (Number, "1"),
                (Comment, "--[[ require('./x') ]]"),
                (Symbol, ".."),
                (Text, "'b'"),
                (Comment, "-- require(\"y\")"),
                (Name, "x"),
                (Symbol, "="),
                (Text, "[==[ ]] require('z') ]==]"),
                (Symbol, "..."),
                (Name, "a"),
                (Symbol, "."),
                (Name, "b"),
                (Symbol, ":"),
                (Name, "c"),
                (Symbol, "("),
                (Number, "0x1F"),
                (Symbol, ","),
                (Number, "1.5e-3"),
                (Symbol, ","),
                (Number, ".5"),
                (Symbol, ")"),
            ]
        );
        // Lines, for multiline comments and texts too.
        let lines: Vec<u32> = tokens("a\n--[[\n\n]] b [[\n]]\nc")
            .iter()
            .map(|token| token.line)
            .collect();
        assert_eq!(lines, [1, 2, 4, 4, 6]);
    }

    #[test]
    fn backticks_with_braces_are_pieces_around_their_code() {
        use TokenKind::*;
        let source = "`a{require(`b`) .. {1}}c{x}` `fim` `\\{não}`";
        assert_eq!(
            read(source),
            [
                (Interpolated, "`a{"),
                (Name, "require"),
                (Symbol, "("),
                (Text, "`b`"),
                (Symbol, ")"),
                (Symbol, ".."),
                (Symbol, "{"),
                (Number, "1"),
                (Symbol, "}"),
                (Interpolated, "}c{"),
                (Name, "x"),
                (Interpolated, "}`"),
                (Text, "`fim`"),
                (Text, "`\\{não}`"),
            ]
        );
    }

    #[test]
    fn texts_and_comments_that_never_end_are_broken_and_the_rest_reads_on() {
        use TokenKind::*;
        assert_eq!(
            read("'aberto\nb --[[ sem fim"),
            [(Broken, "'aberto"), (Name, "b"), (Broken, "--[[ sem fim")]
        );
        assert_eq!(read("ç = 1"), [(Broken, "ç"), (Symbol, "="), (Number, "1")]);
    }

    #[test]
    fn texts_say_what_luau_reads_in_them() {
        let say = |source: &str| text(source, &tokens(source)[0]);
        assert_eq!(
            say(r#""./Pista\\dados""#).as_deref(),
            Some(r"./Pista\dados")
        );
        assert_eq!(say(r"'d\'a'").as_deref(), Some("d'a"));
        assert_eq!(
            say(r#""\x41\65\u{e7}\z
                          b""#)
            .as_deref(),
            Some("AAçb")
        );
        assert_eq!(say("[[\n./dados]]").as_deref(), Some("./dados"));
        assert_eq!(say("[=[a]]b]=]").as_deref(), Some("a]]b"));
        assert_eq!(say("`./fase`").as_deref(), Some("./fase"));
        assert_eq!(say(r#""\q""#), None);
        assert_eq!(say(r#""\300""#), None);
        assert_eq!(say("nome"), None);
    }

    #[test]
    fn requires_are_found_in_code_only_whole_texts_apart() {
        let source = "\
local a = require(\"./a\")
local b = require './b' -- require('./comentario')
local c = require [[../c]]
local d = require(\"@project/\" .. nome)
local e = require(`./e{n}`)
local f = require(nome)
local g = outro.require('./g')
--[[ require('./h') ]]
local s = \"require('./i')\"
local t: typeof(require(`./t`)) = nil
local r = require
";
        let found: Vec<(u32, RequireKind)> = requires(source)
            .into_iter()
            .map(|found| (found.line, found.kind))
            .collect();
        let literal = |path: &str, line: usize, needle: &str| {
            let at = source
                .lines()
                .take(line - 1)
                .map(|line| line.len() + 1)
                .sum::<usize>()
                + source.lines().nth(line - 1).unwrap().find(needle).unwrap();
            RequireKind::Literal {
                path: path.into(),
                span: at..at + needle.len(),
            }
        };
        assert_eq!(
            found,
            [
                (1, literal("./a", 1, "\"./a\"")),
                (2, literal("./b", 2, "'./b'")),
                (3, literal("../c", 3, "[[../c]]")),
                (4, RequireKind::Built),
                (5, RequireKind::Built),
                (6, RequireKind::Built),
                (10, literal("./t", 10, "`./t`")),
                (11, RequireKind::Passed),
            ]
        );
        for declared in [
            "local require = 1",
            "local function require(x) end",
            "function require() end",
            "local a, require = 1, 2",
            "require = function() end",
            "for require = 1, 2 do end",
            "local function usar(a, require) end",
            "function M.carregar(require) end",
            "function M:carregar(nome: string, require: any) end",
            "local f = function(require) end",
        ] {
            assert_eq!(
                requires(declared)[0].kind,
                RequireKind::Declared,
                "{declared}"
            );
        }
        for passed in [
            "f(require)",
            "local ok, dados = pcall(require, './dados')",
            "local r = require",
            "return require",
            "local t = { require }",
            "local t = { carregar = require }",
            "obj:usar(require)",
            "(require)('./dados')",
        ] {
            assert_eq!(requires(passed)[0].kind, RequireKind::Passed, "{passed}");
        }
        // Keys of other tables.
        for key in [
            "t = { require = 1 }",
            "t = { a = 1, require = 2 }",
            "type T = { require: (string) -> any }",
            "type T = { a: number, require: (string) -> any }",
        ] {
            assert_eq!(requires(key), [], "{key}");
        }
    }

    #[test]
    fn a_path_is_written_in_the_quotes_it_replaces() {
        assert_eq!(quoted_like("'./a'", "@project/D'a/b"), r"'@project/D\'a/b'");
        assert_eq!(quoted_like("\"./a\"", "@project/D'a"), "\"@project/D'a\"");
        assert_eq!(quoted_like("[[./a]]", "@project/a]]b"), "\"@project/a]]b\"");
        assert_eq!(quoted_like("`./a`", "@project/a"), "\"@project/a\"");
    }
}
