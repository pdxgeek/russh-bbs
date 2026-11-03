//! Reading an untrusted RSS or Atom document into [`NewItem`]s.
//!
//! The document is hostile. Before the parsing crate sees it, one flat pass with the XML
//! tokenizer refuses what it should never be asked to handle: a document type declaration (the
//! door to entity-expansion bombs, so there is no way to define an entity at all), nesting deeper
//! than [`MAX_DEPTH`] (the parsing crate recurses), and more than [`MAX_EVENTS`] tokens. The
//! response size is capped by the fetcher before this runs. Every field then passes through the
//! shared sanitizer; HTML in a body is reduced to text with its links listed ([`super::html`]).

use feed_rs::model::{Entry, Feed};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::drafts::Body;
use crate::forums::body_limits;
use crate::sanitize::{MultiLine, Sanitized, SingleLine};
use crate::store::UnixSeconds;

use super::html::to_plain_text;
use super::item::{NewItem, digest};
use super::url::FeedUrl;

/// Deepest element nesting accepted.
pub const MAX_DEPTH: usize = 48;
/// Most XML tokens accepted in one document.
pub const MAX_EVENTS: usize = 400_000;
/// Most items read from one document (the newest are kept).
pub const MAX_ITEMS: usize = 100;
/// Longest text of an item kept, in characters, before the link list is added.
pub const ITEM_TEXT_CHARS: usize = 4_000;
/// Most lines of an item's text kept.
pub const ITEM_TEXT_LINES: usize = 120;
/// Most links listed under an item.
pub const MAX_LINKS: usize = 8;
/// Longest subject, in characters.
const SUBJECT_CHARS: usize = 120;
/// Longest author name, in characters.
const AUTHOR_CHARS: usize = 64;
/// Longest link shown, in characters.
const LINK_CHARS: usize = 300;

/// Why a document was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Not well-formed XML, or not a feed.
    #[error("not a feed")]
    NotAFeed,
    /// It declares a document type, which no feed needs and an attacker uses.
    #[error("the document declares a document type")]
    DocType,
    /// Nested too deeply or with too many tokens.
    #[error("the document is too complex")]
    TooComplex,
}

/// What was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The feed's own title, cleaned (may be empty).
    pub title: Sanitized,
    /// The items, oldest first, at most [`MAX_ITEMS`] (the newest).
    pub items: Vec<NewItem>,
}

/// The flat pass: refuse what the parsing crate must never see.
fn prescan(bytes: &[u8]) -> Result<(), ParseError> {
    let mut reader = Reader::from_reader(bytes);
    let mut depth = 0_usize;
    let mut events = 0_usize;
    loop {
        events += 1;
        if events > MAX_EVENTS {
            return Err(ParseError::TooComplex);
        }
        match reader.read_event() {
            Ok(Event::DocType(_)) => return Err(ParseError::DocType),
            Ok(Event::Start(_)) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(ParseError::TooComplex);
                }
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Eof) => return Ok(()),
            Ok(_) => {}
            Err(_) => return Err(ParseError::NotAFeed),
        }
    }
}

/// Parse `bytes` (an RSS or Atom document). `now` caps item dates: an item never claims to be
/// from the future.
///
/// # Errors
/// [`ParseError`] for a document that is hostile, malformed or not a feed.
pub fn parse(bytes: &[u8], now: UnixSeconds) -> Result<Parsed, ParseError> {
    prescan(bytes)?;
    let feed: Feed = feed_rs::parser::Builder::new()
        .sanitize_content(false)
        .build()
        .parse(bytes)
        .map_err(|_| ParseError::NotAFeed)?;
    let title = SingleLine::new(SUBJECT_CHARS).sanitize(
        feed.title
            .as_ref()
            .map_or("", |t| t.content.as_str())
            .trim(),
    );
    let mut items: Vec<NewItem> = feed
        .entries
        .iter()
        .take(MAX_ITEMS * 2)
        .filter_map(|entry| item(entry, now))
        .collect();
    // Oldest first, so thread ids follow time; keep the newest MAX_ITEMS.
    items.sort_by_key(|i| i.at);
    if items.len() > MAX_ITEMS {
        items.drain(..items.len() - MAX_ITEMS);
    }
    Ok(Parsed { title, items })
}

fn single(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text.trim()).into_string()
}

/// A link target as plain text, or `None` when it is not an `http`/`https` URL.
fn link_text(raw: &str) -> Option<String> {
    let url = FeedUrl::parse(raw, true).ok()?;
    let text = single(url.as_str(), LINK_CHARS);
    (!text.is_empty()).then_some(text)
}

fn item(entry: &Entry, now: UnixSeconds) -> Option<NewItem> {
    let link = entry
        .links
        .iter()
        .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
        .or_else(|| entry.links.first())
        .and_then(|l| link_text(&l.href));
    let raw_title = entry.title.as_ref().map_or("", |t| t.content.as_str());
    // A title is often HTML-ish too; reduce it the same way before it is cleaned.
    let title = single(&to_plain_text(raw_title, 0).text, SUBJECT_CHARS);
    let raw_body = entry
        .content
        .as_ref()
        .and_then(|c| c.body.as_deref())
        .or_else(|| entry.summary.as_ref().map(|s| s.content.as_str()))
        .unwrap_or("");
    let plain = to_plain_text(raw_body, MAX_LINKS);
    let text = MultiLine::new(ITEM_TEXT_CHARS).sanitize(&plain.text);
    let text = cut_lines(text.as_str(), ITEM_TEXT_LINES);
    let author = entry
        .authors
        .first()
        .map(|a| single(a.name.as_deref().unwrap_or(""), AUTHOR_CHARS))
        .filter(|a| !a.is_empty());

    let subject = if !title.is_empty() {
        title
    } else if let Some(first) = text.lines().find(|l| !l.trim().is_empty()) {
        single(first, SUBJECT_CHARS)
    } else {
        link.clone().unwrap_or_default()
    };
    if subject.is_empty() {
        return None;
    }

    let mut composed = String::new();
    if let Some(author) = &author {
        composed.push_str("By ");
        composed.push_str(author);
        composed.push_str("\n\n");
    }
    composed.push_str(&text);
    let mut refs: Vec<String> = Vec::new();
    if let Some(link) = &link {
        refs.push(format!("Link: {link}"));
    }
    for (n, target) in plain.links.iter().enumerate() {
        if let Some(target) = link_text(target) {
            refs.push(format!("[{}] {target}", n + 1));
        }
    }
    if !refs.is_empty() {
        if !text.trim().is_empty() {
            composed.push_str("\n\n");
        }
        composed.push_str(&refs.join("\n"));
    }
    let body = Body::prepare(body_limits(), &composed).ok()?;
    if body.is_blank() {
        return None;
    }

    let key_source = if entry.id.trim().is_empty() {
        link.as_deref().unwrap_or(&subject)
    } else {
        entry.id.as_str()
    };
    let key_source = single(key_source, 600);
    let at = entry
        .published
        .or(entry.updated)
        .map_or(now.as_raw(), |t| t.timestamp())
        .clamp(0, now.as_raw());
    Some(NewItem {
        key_hash: digest(&[&key_source]),
        content_hash: digest(&[&subject, link.as_deref().unwrap_or(""), body.as_str()]),
        subject: SingleLine::new(SUBJECT_CHARS).sanitize(&subject),
        body,
        at: UnixSeconds::from_raw(at),
    })
}

/// The first `max` lines of `text`.
fn cut_lines(text: &str, max: usize) -> String {
    text.lines().take(max).collect::<Vec<_>>().join("\n")
}
