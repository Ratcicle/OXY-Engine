//! The colours of the code tab: a small reading of Luau, line by line, that tells keywords,
//! texts, comments and numbers apart. It only colours; what is wrong is the engine's compiler to
//! say. Each piece ends where a character starts, so slicing the line by them never splits
//! one.
use std::ops::Range;

/// What a piece of a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Plain,
    Keyword,
    Text,
    Comment,
    Number,
}

/// A line's pieces in order: the bytes of the line each covers and what it is.
pub(crate) type Pieces = Vec<(Range<usize>, Piece)>;

const KEYWORDS: [&str; 22] = [
    "and", "break", "continue", "do", "else", "elseif", "end", "false", "for", "function", "if",
    "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// A comment or text in long brackets that goes on past its line, with its level (how many `=`
/// stand between the brackets).
#[derive(Clone, Copy)]
enum Open {
    Comment(usize),
    Text(usize),
}

/// Each line of `code` (as [`str::lines`] splits it) as its pieces.
pub(crate) fn pieces(code: &str) -> Vec<Pieces> {
    let mut open = None;
    code.lines()
        .map(|line| line_pieces(line, &mut open))
        .collect()
}

fn line_pieces(line: &str, open: &mut Option<Open>) -> Pieces {
    let bytes = line.as_bytes();
    let mut pieces = Pieces::new();
    let mut push = |range: Range<usize>, piece: Piece| match pieces.last_mut() {
        Some((last, kind)) if *kind == piece && last.end == range.start => last.end = range.end,
        _ => pieces.push((range, piece)),
    };
    let mut at = 0;
    if let Some(long) = open.take() {
        let (level, piece) = match long {
            Open::Comment(level) => (level, Piece::Comment),
            Open::Text(level) => (level, Piece::Text),
        };
        match close(bytes, 0, level) {
            Some(end) => {
                push(0..end, piece);
                at = end;
            }
            None => {
                push(0..bytes.len(), piece);
                *open = Some(long);
                return pieces;
            }
        }
    }
    while at < bytes.len() {
        let start = at;
        let byte = bytes[at];
        if bytes[at..].starts_with(b"--") {
            match long_open(bytes, at + 2) {
                Some(level) => match close(bytes, at + 2 + level + 2, level) {
                    Some(end) => at = end,
                    None => {
                        at = bytes.len();
                        *open = Some(Open::Comment(level));
                    }
                },
                None => at = bytes.len(),
            }
            push(start..at, Piece::Comment);
        } else if matches!(byte, b'"' | b'\'' | b'`') {
            at = quoted_end(bytes, at);
            push(start..at, Piece::Text);
        } else if let Some(level) = long_open(bytes, at) {
            match close(bytes, at + level + 2, level) {
                Some(end) => at = end,
                None => {
                    at = bytes.len();
                    *open = Some(Open::Text(level));
                }
            }
            push(start..at, Piece::Text);
        } else if starts_number(bytes, at) {
            at = number_end(bytes, at);
            push(start..at, Piece::Number);
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            at += bytes[at..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                .count();
            let word = &line[start..at];
            let piece = if KEYWORDS.contains(&word) {
                Piece::Keyword
            } else {
                Piece::Plain
            };
            push(start..at, piece);
        } else {
            // Operators, spaces and anything else, up to where a piece of another kind may start.
            at += 1;
            while at < bytes.len() && !starts_piece(bytes, at) {
                at += 1;
            }
            push(start..at, Piece::Plain);
        }
    }
    pieces
}

/// Whether something other than plain code may start at `at`.
fn starts_piece(bytes: &[u8], at: usize) -> bool {
    let byte = bytes[at];
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'_' | b'"' | b'\'' | b'`' | b'[')
        || bytes[at..].starts_with(b"--")
        || starts_number(bytes, at)
}

/// Whether a number starts at `at`: a digit, or a point before one that does not follow
/// another point (`..1` joins texts).
fn starts_number(bytes: &[u8], at: usize) -> bool {
    bytes[at].is_ascii_digit()
        || (bytes[at] == b'.'
            && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)
            && (at == 0 || bytes[at - 1] != b'.'))
}

/// Where the number that starts at `at` ends: hexadecimal and binary, or decimal with a point
/// and an exponent; `_` separates digits.
fn number_end(bytes: &[u8], at: usize) -> usize {
    let mut end = at;
    let prefixed = [b"0x", b"0X", b"0b", b"0B"]
        .iter()
        .any(|p| bytes[at..].starts_with(*p));
    if prefixed {
        end += 2;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        return end;
    }
    while end < bytes.len() {
        match bytes[end] {
            b'e' | b'E' => {
                end += 1;
                if matches!(bytes.get(end), Some(b'+' | b'-')) {
                    end += 1;
                }
            }
            b'0'..=b'9' | b'_' | b'.' => end += 1,
            // What follows a number without a space is part of it, wrong as it may be.
            byte if byte.is_ascii_alphabetic() => end += 1,
            _ => break,
        }
    }
    end
}

/// Where the text quoted by the byte at `at` ends, after its closing quote; the end of the
/// line when it does not close there.
fn quoted_end(bytes: &[u8], at: usize) -> usize {
    let quote = bytes[at];
    let mut end = at + 1;
    while end < bytes.len() {
        match bytes[end] {
            b'\\' => end += 2,
            byte if byte == quote => return end + 1,
            _ => end += 1,
        }
    }
    bytes.len()
}

/// The level of the long bracket that opens at `at` (`[[`, `[=[`, …).
fn long_open(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let level = bytes[at + 1..].iter().take_while(|b| **b == b'=').count();
    (bytes.get(at + 1 + level) == Some(&b'[')).then_some(level)
}

/// Where the long bracket of `level` closes, from `from` on, after it.
fn close(bytes: &[u8], from: usize, level: usize) -> Option<usize> {
    let mut at = from;
    while at < bytes.len() {
        if bytes[at] == b']'
            && bytes[at + 1..]
                .iter()
                .take(level)
                .filter(|b| **b == b'=')
                .count()
                == level
            && bytes.get(at + 1 + level) == Some(&b']')
        {
            return Some(at + level + 2);
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Piece::*;
    use super::*;

    /// The pieces of each line of `code` as the text they cover.
    fn read(code: &str) -> Vec<Vec<(&str, Piece)>> {
        code.lines()
            .zip(pieces(code))
            .map(|(line, pieces)| {
                pieces
                    .into_iter()
                    .map(|(range, piece)| (&line[range], piece))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn keywords_texts_comments_and_numbers_are_told_apart() {
        assert_eq!(
            read("local n = 3 -- três\nif endless then return \"já\" end"),
            [
                vec![
                    ("local", Keyword),
                    (" n = ", Plain),
                    ("3", Number),
                    (" ", Plain),
                    ("-- três", Comment),
                ],
                vec![
                    ("if", Keyword),
                    (" endless ", Plain),
                    ("then", Keyword),
                    (" ", Plain),
                    ("return", Keyword),
                    (" ", Plain),
                    ("\"já\"", Text),
                    (" ", Plain),
                    ("end", Keyword),
                ],
            ]
        );
        // Escapes stay inside the text, and one that does not close runs to the line's end.
        assert_eq!(
            read(r#"print('a\'b', `{x}`) x = "aberto"#),
            [vec![
                ("print(", Plain),
                (r"'a\'b'", Text),
                (", ", Plain),
                ("`{x}`", Text),
                (") x = ", Plain),
                ("\"aberto", Text),
            ]]
        );
    }

    #[test]
    fn numbers_take_their_whole_form_and_points_join_texts() {
        assert_eq!(
            read("x = 0xFF + 1_000 + 2.5e-3 + .5 .. 1"),
            [vec![
                ("x = ", Plain),
                ("0xFF", Number),
                (" + ", Plain),
                ("1_000", Number),
                (" + ", Plain),
                ("2.5e-3", Number),
                (" + ", Plain),
                (".5", Number),
                (" .. ", Plain),
                ("1", Number),
            ]]
        );
        assert_eq!(
            read("t..1 v2"),
            [vec![("t..", Plain), ("1", Number), (" v2", Plain)]]
        );
    }

    #[test]
    fn long_comments_and_texts_go_on_across_lines() {
        let code = "--[==[ início\n]] ainda ]==] local a = [[\nmeio\n]] .. b";
        assert_eq!(
            read(code),
            [
                vec![("--[==[ início", Comment)],
                vec![
                    ("]] ainda ]==]", Comment),
                    (" ", Plain),
                    ("local", Keyword),
                    (" a = ", Plain),
                    ("[[", Text)
                ],
                vec![("meio", Text)],
                vec![("]]", Text), (" .. b", Plain)],
            ]
        );
        // A bracket that is not long is code: indexing.
        assert_eq!(
            read("t[1] = t[ [[x]] ]"),
            [vec![
                ("t[", Plain),
                ("1", Number),
                ("] = t[ ", Plain),
                ("[[x]]", Text),
                (" ]", Plain),
            ]]
        );
    }

    #[test]
    fn pieces_cover_every_byte_and_never_split_a_character() {
        let code = "local ação = \"coração\" -- é\n\tvars.set(\"Vitórias\", 1) ÿ\n--[[ ç\nã ]] fim";
        for (line, pieces) in code.lines().zip(pieces(code)) {
            let mut end = 0;
            for (range, _) in &pieces {
                assert_eq!(range.start, end, "{line:?}");
                assert!(line.is_char_boundary(range.start) && line.is_char_boundary(range.end));
                end = range.end;
            }
            assert_eq!(end, line.len(), "{line:?}");
        }
    }
}
