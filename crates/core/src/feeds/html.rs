//! HTML reduced to plain text with the links listed (never rendered as markup).
//!
//! This is not an HTML parser and does not try to be one: it drops every tag, skips `script`,
//! `style` and comments, turns block tags into line breaks, decodes a short list of entities and
//! all numeric ones, and collects `http` and `https` `href`s so the reader can see where a link
//! went. The result is untrusted text: the caller still runs it through the shared sanitizer.

/// Plain text and the links found in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlainText {
    /// The text, with `[n]` after each link's words.
    pub text: String,
    /// The `http` and `https` link targets, `[1]` first.
    pub links: Vec<String>,
}

/// Most characters of input looked at; the rest is ignored.
const MAX_INPUT_CHARS: usize = 200_000;
/// Longest link target kept, in characters.
const MAX_LINK_CHARS: usize = 300;

/// Reduce `html` to text, listing at most `max_links` links.
#[must_use]
pub fn to_plain_text(html: &str, max_links: usize) -> PlainText {
    let chars: Vec<char> = html.chars().take(MAX_INPUT_CHARS).collect();
    let mut out = PlainText::default();
    let mut i = 0;
    // The link currently open, as its number once it was given one.
    let mut open_link: Option<usize> = None;
    while i < chars.len() {
        let c = chars[i];
        if c == '&' {
            let (decoded, used) = entity(&chars[i..]);
            out.text.push_str(&decoded);
            i += used;
            continue;
        }
        if c != '<' {
            out.text.push(c);
            i += 1;
            continue;
        }
        let next = chars.get(i + 1).copied();
        if next == Some('!') && chars[i..].starts_with(&['<', '!', '-', '-']) {
            i = skip_past(&chars, i + 4, &['-', '-', '>']);
            continue;
        }
        let is_tag =
            next.is_some_and(|n| n.is_ascii_alphabetic() || n == '/' || n == '!' || n == '?');
        if !is_tag {
            out.text.push('<');
            i += 1;
            continue;
        }
        let Some(end) = find_tag_end(&chars, i + 1) else {
            // An unterminated tag: drop the rest rather than guess.
            break;
        };
        let inner: String = chars[i + 1..end].iter().collect();
        i = end + 1;
        let closing = inner.starts_with('/');
        let name: String = inner
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match name.as_str() {
            "script" | "style" if !closing => {
                i = skip_past_tag(&chars, i, &name);
            }
            "br" | "p" | "div" | "tr" | "table" | "ul" | "ol" | "blockquote" | "pre" | "h1"
            | "h2" | "h3" | "h4" | "h5" | "h6" | "hr" | "section" | "article" => {
                newline(&mut out.text);
            }
            "li" if !closing => {
                newline(&mut out.text);
                out.text.push_str("- ");
            }
            "a" if closing => {
                if let Some(n) = open_link.take() {
                    out.text.push_str(&format!(" [{n}]"));
                }
            }
            "a" => {
                open_link = None;
                if let Some(href) = attribute(&inner, "href") {
                    let href = href.trim();
                    let lower = href.to_ascii_lowercase();
                    if (lower.starts_with("http://") || lower.starts_with("https://"))
                        && out.links.len() < max_links
                    {
                        out.links.push(href.chars().take(MAX_LINK_CHARS).collect());
                        open_link = Some(out.links.len());
                    }
                }
            }
            _ => {}
        }
    }
    out.text = tidy(&out.text);
    out
}

fn newline(text: &mut String) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
}

/// Index just past the first `needle` at or after `from`, or the end.
fn skip_past(chars: &[char], from: usize, needle: &[char]) -> usize {
    let mut i = from;
    while i < chars.len() {
        if chars[i..].starts_with(needle) {
            return i + needle.len();
        }
        i += 1;
    }
    chars.len()
}

/// Index just past `</name>` (any case), or the end.
fn skip_past_tag(chars: &[char], from: usize, name: &str) -> usize {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == '<' && chars[i + 1] == '/' {
            let word: String = chars[i + 2..]
                .iter()
                .take(name.len())
                .collect::<String>()
                .to_ascii_lowercase();
            if word == name {
                return find_tag_end(chars, i + 2).map_or(chars.len(), |e| e + 1);
            }
        }
        i += 1;
    }
    chars.len()
}

/// The index of the `>` that ends a tag whose text starts at `from`, honouring quotes.
fn find_tag_end(chars: &[char], from: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, &c) in chars.get(from..)?.iter().enumerate() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(from + offset),
            _ => {}
        }
        // A tag that runs on this long is not a tag.
        if offset > 4_000 {
            return None;
        }
    }
    None
}

/// The value of attribute `name` in a tag's inner text.
fn attribute(inner: &str, name: &str) -> Option<String> {
    let lower = inner.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let start = from + found;
        from = start + name.len();
        let before_ok = start == 0
            || lower[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        if !before_ok {
            continue;
        }
        let rest = inner[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let value = match rest.chars().next()? {
            q @ ('"' | '\'') => rest[1..].split(q).next()?.to_owned(),
            _ => rest
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()?
                .to_owned(),
        };
        return Some(decode_entities(&value));
    }
    None
}

fn decode_entities(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let (decoded, used) = entity(&chars[i..]);
            out.push_str(&decoded);
            i += used;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Decode the entity at the start of `chars` (which begins with `&`): the text and how many
/// characters it used. Anything unknown is left as written.
fn entity(chars: &[char]) -> (String, usize) {
    let end = chars.iter().take(12).position(|&c| c == ';');
    let Some(end) = end else {
        return ("&".to_owned(), 1);
    };
    let name: String = chars[1..end].iter().collect();
    let used = end + 1;
    let decoded = match name.as_str() {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "hellip" => Some('\u{2026}'),
        "mdash" => Some('\u{2014}'),
        "ndash" => Some('\u{2013}'),
        "lsquo" => Some('\u{2018}'),
        "rsquo" => Some('\u{2019}'),
        "ldquo" => Some('\u{201C}'),
        "rdquo" => Some('\u{201D}'),
        "copy" => Some('\u{00A9}'),
        "reg" => Some('\u{00AE}'),
        "trade" => Some('\u{2122}'),
        n => n.strip_prefix('#').map(|digits| {
            let code = digits.strip_prefix(['x', 'X']).map_or_else(
                || digits.parse::<u32>().ok(),
                |h| u32::from_str_radix(h, 16).ok(),
            );
            code.and_then(char::from_u32).unwrap_or('\u{FFFD}')
        }),
    };
    match decoded {
        Some(c) => (c.to_string(), used),
        None => ("&".to_owned(), 1),
    }
}

/// Collapse runs of spaces, trim line ends and keep at most one blank line in a row.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_owned()
}
