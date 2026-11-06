//! Hostile-input generators for the loader fuzz tests (H-4). Test code only.
//!
//! Every loader that reads a sysop- or file-supplied document gets the same four kinds of
//! input: a valid document with a few random mutations, random bytes, structured garbage (deep
//! nesting, huge strings, many keys), and valid documents with reserved characters spliced in.
//! The properties each loader checks live beside the loader's tests.

use proptest::prelude::*;
use russh_bbs_core::sanitize::RESERVED_CHROME;

/// Documents longer than this are cut: the loaders' own byte caps are far below it, and the
/// tests must stay fast in a debug build.
const MAX_DOC: usize = 160 * 1024;

/// Fragments that have broken parsers or sanitizers before.
const SNIPPETS: &[&str] = &[
    "\"\"\"",
    "'''",
    "\u{1b}[31m",
    "\u{1b}]0;title\u{7}",
    "\u{9b}2J",
    "\u{0}",
    "\u{7f}",
    "\u{200b}",
    "\u{202e}",
    "\u{feff}",
    "\u{e0a0}",
    "\u{f8ff}",
    "\u{2063}",
    "\\u001b",
    "\\u0000",
    "\\x41",
    "\\",
    "\"",
    "'",
    "[[",
    "]]",
    "[a.b.c.d.e]",
    "[[a.b]]",
    " = ",
    "= [[[[[[[[",
    "{ a = { b = { c = 1 } } }",
    "nan",
    "inf",
    "-inf",
    "-1",
    "0",
    "1e999",
    "-1e999",
    "99999999999999999999999999",
    "9223372036854775807",
    "-9223372036854775808",
    "0xFFFFFFFF",
    "true",
    "[]",
    "{}",
    "\n\n\n",
    "\r\n",
    "# comment",
    "\u{3010}\u{3011}",
    "\u{301a}\u{301b}",
    "\u{25c9}",
];

/// Values to put where a valid document has a different type.
const WRONG_TYPES: &[&str] = &[
    "[]",
    "{}",
    "true",
    "1.5",
    "nan",
    "inf",
    "-inf",
    "-1",
    "0",
    "18446744073709551616",
    "9223372036854775807",
    "1e309",
    "\"\"",
    "[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]",
    "{ a = { b = { c = [1, 2, { d = 3 }] } } }",
    "'\u{1b}[31m'",
    "\"\\u001b[2J\"",
    "[1, \"two\", 3.0, true]",
    "1979-05-27T07:32:00Z",
];

/// One random edit, as `(kind, a, b)` numbers resolved against the document being edited.
type Op = (u8, u32, u32);

fn pick(n: u32, len: usize) -> usize {
    if len == 0 { 0 } else { n as usize % len }
}

/// The char boundary at char index `at`.
fn byte_at(text: &str, at: usize) -> usize {
    text.char_indices().nth(at).map_or(text.len(), |(i, _)| i)
}

fn apply(text: &mut String, (kind, a, b): Op) {
    let chars = text.chars().count();
    match kind % 10 {
        // Cut the document short.
        0 => text.truncate(byte_at(text, pick(a, chars + 1))),
        // Delete a span.
        1 => {
            let from = pick(a, chars + 1);
            let to = (from + pick(b, 64)).min(chars);
            let (f, t) = (byte_at(text, from), byte_at(text, to));
            text.replace_range(f..t, "");
        }
        // Splice in a hostile fragment, or a reserved marker glyph.
        2 => {
            let at = byte_at(text, pick(a, chars + 1));
            let all = SNIPPETS.len() + RESERVED_CHROME.len();
            let n = pick(b, all);
            if let Some(s) = SNIPPETS.get(n) {
                text.insert_str(at, s);
            } else if let Some(c) = RESERVED_CHROME.get(n - SNIPPETS.len()) {
                text.insert(at, *c);
            }
        }
        // Duplicate a line (duplicate keys, duplicate tables).
        3 => {
            let lines: Vec<&str> = text.lines().collect();
            if let Some(line) = lines.get(pick(a, lines.len())) {
                let line = (*line).to_owned();
                text.push('\n');
                text.push_str(&line);
            }
        }
        // Repeat a span many times.
        4 => {
            let from = pick(a, chars + 1);
            let to = (from + 1 + pick(b, 200)).min(chars);
            let span: String = text.chars().skip(from).take(to - from).collect();
            let times = 2 + pick(a ^ b, 400);
            if !span.is_empty() && span.len() * times < MAX_DOC {
                let at = byte_at(text, to);
                text.insert_str(at, &span.repeat(times));
            }
        }
        // Put a value of the wrong type after a random `=`.
        5 => {
            let marks: Vec<usize> = text.match_indices("= ").map(|(i, _)| i + 2).collect();
            if let Some(&at) = marks.get(pick(a, marks.len())) {
                let end = text[at..].find('\n').map_or(text.len(), |n| at + n);
                text.replace_range(at..end, WRONG_TYPES[pick(b, WRONG_TYPES.len())]);
            }
        }
        // Swap two lines.
        6 => {
            let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
            if lines.len() > 1 {
                let (i, j) = (pick(a, lines.len()), pick(b, lines.len()));
                lines.swap(i, j);
                *text = lines.join("\n");
            }
        }
        // Replace one character with any character at all.
        7 => {
            let at = byte_at(text, pick(a, chars));
            if at < text.len() {
                let c = char::from_u32(b % 0x11_0000).unwrap_or('\u{fffd}');
                let old = text[at..].chars().next().map_or(0, char::len_utf8);
                text.replace_range(at..at + old, &c.to_string());
            }
        }
        // An unknown field or table.
        8 => {
            let at = byte_at(text, pick(a, chars + 1));
            let extra = if b % 2 == 0 {
                "\nunknown_field = 1\n"
            } else {
                "\n[unknown]\nx = 1\n"
            };
            text.insert_str(at, extra);
        }
        // A huge string where text goes.
        _ => {
            let marks: Vec<usize> = text.match_indices("= \"").map(|(i, _)| i + 3).collect();
            if let Some(&at) = marks.get(pick(a, marks.len())) {
                let n = 1 + pick(b, 70_000);
                text.insert_str(at, &"x".repeat(n));
            }
        }
    }
    if text.len() > MAX_DOC {
        let mut cut = MAX_DOC;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
}

/// `valid` with one to five random edits.
pub fn mutated(valid: &'static str) -> impl Strategy<Value = String> {
    prop::collection::vec((any::<u8>(), any::<u32>(), any::<u32>()), 1..6).prop_map(move |ops| {
        let mut text = valid.to_owned();
        for op in ops {
            apply(&mut text, op);
        }
        text
    })
}

/// Random bytes, read as text the way a loader reads a file it has already checked is UTF-8
/// (lossy, so every byte value is exercised).
pub fn random_text() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 0..4096)
        .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Documents shaped to hit recursion, allocation and quadratic paths, whatever the schema.
pub fn garbage() -> impl Strategy<Value = String> {
    prop_oneof![
        // Deep arrays.
        (1_usize..20_000).prop_map(|n| format!("a = {}{}\n", "[".repeat(n), "]".repeat(n))),
        // Deep inline tables.
        (1_usize..8_000).prop_map(|n| format!("a = {}1{}\n", "{ a = ".repeat(n), " }".repeat(n))),
        // Unclosed nesting.
        (1_usize..20_000).prop_map(|n| format!("a = {}\n", "[".repeat(n))),
        // A long dotted key and a long table header chain.
        (1_usize..6_000).prop_map(|n| format!("{} = 1\n", vec!["a"; n].join("."))),
        (1_usize..6_000).prop_map(|n| format!("[{}]\nx = 1\n", vec!["a"; n].join("."))),
        // Many array-of-tables entries.
        (1_usize..4_000).prop_map(|n| "[[piece]]\n".repeat(n)),
        (1_usize..4_000).prop_map(|n| "[[menu]]\n".repeat(n)),
        // Many keys, and one key repeated.
        (1_usize..8_000).prop_map(|n| (0..n).fold(String::new(), |mut s, i| {
            s.push_str(&format!("k{i} = {i}\n"));
            s
        })),
        (1_usize..8_000).prop_map(|n| "version = 1\n".repeat(n)),
        // A huge string, and a huge multi-line string.
        (1_usize..150_000).prop_map(|n| format!("name = \"{}\"\n", "x".repeat(n))),
        (1_usize..150_000)
            .prop_map(|n| format!("text = \"\"\"\n{}\"\"\"\n", "line\n".repeat(n / 5 + 1))),
        // A very long single line, and many short lines.
        (1_usize..150_000).prop_map(|n| format!("# {}\n", "y".repeat(n))),
        (1_usize..60_000).prop_map(|n| "\n".repeat(n)),
        // Numbers at and beyond the edges.
        prop::sample::select(WRONG_TYPES.to_vec())
            .prop_map(|v| format!("version = {v}\nformat = {v}\npack = {v}\n")),
        // Random bytes.
        random_text(),
    ]
}

/// Whether `c` is something no loaded, displayable string may contain.
pub fn forbidden_in_content(c: char) -> bool {
    c.is_control()
        || RESERVED_CHROME.contains(&c)
        || ('\u{e000}'..='\u{f8ff}').contains(&c)
        || ('\u{f0000}'..='\u{ffffd}').contains(&c)
        || ('\u{100000}'..='\u{10fffd}').contains(&c)
}

/// What a refusal says is logged and shown: one short line, no control characters.
pub fn reason_is_loggable(reason: &str) -> Result<(), String> {
    if reason.chars().count() > 600 {
        return Err(format!(
            "reason is {} characters: {:.400}",
            reason.chars().count(),
            reason
        ));
    }
    if reason.chars().any(char::is_control) {
        return Err(format!("reason has a control character: {reason:?}"));
    }
    Ok(())
}
