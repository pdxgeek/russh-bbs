//! The board's identity and what members may choose (UI-2): the `[board]` configuration turned
//! into checked values the screens draw with.
//!
//! **Chrome, not content.** The name, tagline, location and "since" text are the sysop's own
//! words and are drawn in the header, the identity panel and the login banner. They still go
//! through the shared sanitizer (anything it would change is refused, not silently altered) and
//! the same honesty check as every themed string, so a board cannot be named to claim privacy or
//! to carry a reserved marker glyph. They are held as [`BoardText`], which only
//! [`Board::from_config`] can make.
//!
//! **Limits, not just defaults.** `styles` and `layouts` are what members may pick from. A list
//! of exactly one entry locks that choice for everyone, whatever they stored earlier.

use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::{LayoutId, ThemeName, layout_from_stored};

use super::depth::Tint;
use super::strings::StringError;
use super::theme::skin::parse_tint;
use crate::config::BoardConfig;

/// Longest board name, in characters.
pub const NAME_MAX: usize = 40;
/// Longest tagline or location, in characters.
pub const LINE_MAX: usize = 80;
/// Longest "since" text, in characters.
pub const SINCE_MAX: usize = 16;
/// Most styles or layouts a list may name.
pub const LIST_MAX: usize = 32;
/// Longest logoff quote, in characters.
pub const QUOTE_MAX: usize = 120;
/// Most logoff quotes.
pub const QUOTES_MAX: usize = 32;

/// The board's own words, checked once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BoardText(String);

impl BoardText {
    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether there is nothing to show.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Why a `[board]` value was refused: the key and what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{key}: {message}")]
pub struct BoardError {
    /// The dotted key.
    pub key: &'static str,
    /// What is wrong.
    pub message: String,
}

fn refuse(key: &'static str, message: impl Into<String>) -> BoardError {
    BoardError {
        key,
        message: message.into(),
    }
}

/// The board's identity and limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    name: BoardText,
    tagline: BoardText,
    location: BoardText,
    since: BoardText,
    accent: Option<Tint>,
    style: Option<ThemeName>,
    layout: Option<LayoutId>,
    styles: Vec<ThemeName>,
    layouts: Vec<LayoutId>,
    motion: bool,
    ticker: Vec<BoardText>,
    ticker_feeds: bool,
    art: bool,
    quotes: Vec<BoardText>,
}

/// The name a board has until its sysop sets one: the same word the screens always showed.
pub const DEFAULT_NAME: &str = "russh-bbs";

impl Default for Board {
    fn default() -> Self {
        Self::from_config(&BoardConfig::default()).unwrap_or_else(|_| Self {
            name: BoardText(DEFAULT_NAME.to_owned()),
            tagline: BoardText::default(),
            location: BoardText::default(),
            since: BoardText::default(),
            accent: None,
            style: None,
            layout: None,
            styles: Vec::new(),
            layouts: Vec::new(),
            motion: true,
            ticker: Vec::new(),
            ticker_feeds: false,
            art: true,
            quotes: Vec::new(),
        })
    }
}

impl Board {
    /// The board with every default: the same words the screens always had.
    #[must_use]
    pub fn standard() -> &'static Self {
        static STANDARD: std::sync::OnceLock<Board> = std::sync::OnceLock::new();
        STANDARD.get_or_init(Self::default)
    }

    /// Check and keep the `[board]` section. The same function the config validator calls, so
    /// a value that boots is a value that was checked.
    pub fn from_config(config: &BoardConfig) -> Result<Self, BoardError> {
        let name = text("board.name", &config.name, NAME_MAX, false)?;
        let tagline = text("board.tagline", &config.tagline, LINE_MAX, true)?;
        let location = text("board.location", &config.location, LINE_MAX, true)?;
        let since = text("board.since", &config.since, SINCE_MAX, true)?;
        let accent = config
            .accent
            .as_deref()
            .map(|text| {
                let value = text.trim().parse::<i64>().map_or_else(
                    |_| toml::Value::String(text.trim().to_owned()),
                    toml::Value::Integer,
                );
                parse_tint(&value).map_err(|message| refuse("board.accent", message))
            })
            .transpose()?;
        let style = config
            .style
            .as_deref()
            .map(|text| style_name("board.style", text))
            .transpose()?;
        let layout = config
            .layout
            .as_deref()
            .map(|text| layout_name("board.layout", text))
            .transpose()?;
        let styles = list("board.styles", &config.styles, style_name)?;
        let layouts = list("board.layouts", &config.layouts, layout_name)?;
        // A default the members are not allowed to pick would be an instant contradiction.
        if style.is_some_and(|style| !styles.is_empty() && !styles.contains(&style)) {
            return Err(refuse("board.style", "is not in board.styles"));
        }
        if layout.is_some_and(|layout| !layouts.is_empty() && !layouts.contains(&layout)) {
            return Err(refuse("board.layout", "is not in board.layouts"));
        }
        let ticker = ticker_lines(&config.ticker)?;
        let quotes = quotes(&config.quotes)?;
        Ok(Self {
            name,
            tagline,
            location,
            since,
            accent,
            style,
            layout,
            styles,
            layouts,
            motion: config.motion,
            ticker,
            ticker_feeds: config.ticker_feeds,
            art: config.art,
            quotes,
        })
    }

    /// The board's name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The tagline (may be empty).
    #[must_use]
    pub fn tagline(&self) -> &BoardText {
        &self.tagline
    }

    /// The location line (may be empty).
    #[must_use]
    pub fn location(&self) -> &BoardText {
        &self.location
    }

    /// The "since" text (may be empty).
    #[must_use]
    pub fn since(&self) -> &BoardText {
        &self.since
    }

    /// The accent colour that replaces the style's, if the sysop set one.
    #[must_use]
    pub const fn accent(&self) -> Option<Tint> {
        self.accent
    }

    /// The style new members start with, if the sysop set one.
    #[must_use]
    pub const fn style(&self) -> Option<ThemeName> {
        self.style
    }

    /// The layout new members start with, if the sysop set one.
    #[must_use]
    pub const fn layout(&self) -> Option<LayoutId> {
        self.layout
    }

    /// The styles members may pick from; empty means every loaded style.
    #[must_use]
    pub fn styles(&self) -> &[ThemeName] {
        &self.styles
    }

    /// The layouts members may pick from; empty means every layout.
    #[must_use]
    pub fn layouts(&self) -> &[LayoutId] {
        &self.layouts
    }

    /// The sysop's ticker lines (UI-9), each checked like the tagline.
    #[must_use]
    pub fn ticker(&self) -> &[BoardText] {
        &self.ticker
    }

    /// Whether the ticker also shows feed headlines.
    #[must_use]
    pub const fn ticker_feeds(&self) -> bool {
        self.ticker_feeds
    }

    /// Whether the board lets members have motion at all.
    #[must_use]
    pub const fn motion(&self) -> bool {
        self.motion
    }

    /// Whether the banner and header art is drawn (`board.art`); off leaves large letters only.
    #[must_use]
    pub const fn art(&self) -> bool {
        self.art
    }

    /// The logoff quotes the sysop wrote (may be empty).
    #[must_use]
    pub fn quotes(&self) -> &[BoardText] {
        &self.quotes
    }

    /// The quote for `seed` (any number: the clock, a counter), or `None` when there are none.
    #[must_use]
    pub fn quote_for(&self, seed: u64) -> Option<&BoardText> {
        let count = u64::try_from(self.quotes.len()).ok().filter(|n| *n > 0)?;
        self.quotes.get(usize::try_from(seed % count).ok()?)
    }

    /// The name as the string table's `ui.title`, when it differs from the built-in word.
    #[must_use]
    pub fn title_override(&self) -> Option<String> {
        (self.name.as_str() != DEFAULT_NAME).then(|| self.name.0.clone())
    }
}

/// The `|`-separated logoff quotes, each checked like any other board text.
fn quotes(raw: &str) -> Result<Vec<BoardText>, BoardError> {
    let mut out = Vec::new();
    for part in raw.split('|').map(str::trim).filter(|p| !p.is_empty()) {
        out.push(text("board.quotes", part, QUOTE_MAX, false)?);
        if out.len() > QUOTES_MAX {
            return Err(refuse(
                "board.quotes",
                format!("names more than {QUOTES_MAX} quotes"),
            ));
        }
    }
    Ok(out)
}

/// Check one piece of board text. `optional` allows it to be empty.
fn text(key: &'static str, raw: &str, max: usize, optional: bool) -> Result<BoardText, BoardError> {
    let value = raw.trim();
    if value.is_empty() {
        return if optional {
            Ok(BoardText::default())
        } else {
            Err(refuse(key, "must not be empty"))
        };
    }
    if value.chars().count() > max {
        return Err(refuse(key, format!("is longer than {max} characters")));
    }
    if SingleLine::new(max + 1).sanitize(value).as_str() != value {
        return Err(refuse(
            key,
            "contains control, invisible or reserved characters",
        ));
    }
    match super::strings::check_board_text(key, value) {
        Ok(()) => Ok(BoardText(value.to_owned())),
        Err(StringError::Claim(_)) => Err(refuse(
            key,
            "must not say the board is private, secure or encrypted: the server can read everything except PGP-encrypted mail and files members encrypt themselves",
        )),
        Err(other) => Err(refuse(key, other.to_string())),
    }
}

fn style_name(key: &'static str, text: &str) -> Result<ThemeName, BoardError> {
    ThemeName::new(text.trim()).map_err(|error| refuse(key, error.to_string()))
}

fn layout_name(key: &'static str, text: &str) -> Result<LayoutId, BoardError> {
    layout_from_stored(text.trim()).ok_or_else(|| {
        refuse(
            key,
            format!("{text:?} is not a layout (classic, lightbar, compact, dashboard, wide, focus, split)"),
        )
    })
}

/// A comma-separated list, each entry checked, no repeats.
/// Most ticker lines, and the longest one, in characters.
pub const TICKER_MAX_LINES: usize = 8;
/// See [`TICKER_MAX_LINES`].
pub const TICKER_LINE_MAX: usize = 80;

/// The `|`-separated ticker lines, each checked like the tagline (no control, invisible or
/// reserved characters, no claim of privacy).
fn ticker_lines(raw: &str) -> Result<Vec<BoardText>, BoardError> {
    let mut lines = Vec::new();
    for part in raw.split('|').map(str::trim).filter(|p| !p.is_empty()) {
        lines.push(text("board.ticker", part, TICKER_LINE_MAX, false)?);
        if lines.len() > TICKER_MAX_LINES {
            return Err(refuse(
                "board.ticker",
                format!("has more than {TICKER_MAX_LINES} lines"),
            ));
        }
    }
    Ok(lines)
}

fn list<T: PartialEq + Copy>(
    key: &'static str,
    raw: &str,
    parse: fn(&'static str, &str) -> Result<T, BoardError>,
) -> Result<Vec<T>, BoardError> {
    let mut out: Vec<T> = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let item = parse(key, part)?;
        if out.contains(&item) {
            return Err(refuse(key, format!("names {part:?} twice")));
        }
        out.push(item);
        if out.len() > LIST_MAX {
            return Err(refuse(key, format!("names more than {LIST_MAX} entries")));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
