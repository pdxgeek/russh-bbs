//! The `[widgets]` configuration turned into checked values (UI-10): the sysop's limits and
//! defaults ([`Rules`]), the message of the day and the quotes.
//!
//! The sysop's text goes through the same checks as the board's own words (`ui::board`): the
//! shared sanitizer must leave it unchanged, and it may not claim the board is private, secure
//! or encrypted. It is held as [`SysopText`], which only this module can make.

use russh_bbs_core::terminal_settings::LayoutId;
use russh_bbs_core::widgets::{MAX_PER_LAYOUT, Rules, WidgetId};

use crate::config::WidgetsConfig;
use crate::ui::strings::{StringError, check_board_text};

/// Longest message of the day, in characters.
pub const MOTD_MAX: usize = 240;
/// Longest quote, in characters.
pub const QUOTE_MAX: usize = 200;
/// Most quotes.
pub const QUOTES_MAX: usize = 64;

/// Why a `[widgets]` value was refused: the key and what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{key}: {message}")]
pub struct WidgetsError {
    /// The dotted key.
    pub key: &'static str,
    /// What is wrong.
    pub message: String,
}

fn refuse(key: &'static str, message: impl Into<String>) -> WidgetsError {
    WidgetsError {
        key,
        message: message.into(),
    }
}

/// The sysop's own words, checked once (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SysopText(String);

impl SysopText {
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

/// The checked `[widgets]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WidgetSettings {
    rules: Rules,
    motd: SysopText,
    quotes: Vec<SysopText>,
}

impl WidgetSettings {
    /// Check and keep the section. The same function the config validator calls.
    ///
    /// # Errors
    /// [`WidgetsError`] naming the key: an unknown widget id, a widget in two of the hidden, on
    /// and off lists, a default list that is too long, or text that fails the board-text checks.
    pub fn from_config(config: &WidgetsConfig) -> Result<Self, WidgetsError> {
        let hidden = ids("widgets.hidden", &config.hidden)?;
        let on = ids("widgets.on", &config.on)?;
        let off = ids("widgets.off", &config.off)?;
        let mut defaults = Vec::new();
        for (key, layout, raw) in [
            ("widgets.dashboard", LayoutId::Dashboard, &config.dashboard),
            ("widgets.wide", LayoutId::Wide, &config.wide),
            ("widgets.focus", LayoutId::Focus, &config.focus),
            ("widgets.split", LayoutId::Split, &config.split),
        ] {
            let list = ids(key, raw)?;
            if !list.is_empty() {
                if list.len() > MAX_PER_LAYOUT {
                    return Err(refuse(
                        key,
                        format!("names more than {MAX_PER_LAYOUT} widgets"),
                    ));
                }
                defaults.push((layout, list));
            }
        }
        let rules = Rules::new(hidden, on, off, defaults).map_err(|error| {
            // The conflict names the widget; point at the key that listed it second.
            refuse("widgets.on", error.to_string())
        })?;
        let motd = text("widgets.motd", &config.motd, MOTD_MAX)?;
        let mut quotes = Vec::new();
        for part in config.quotes.split('|').map(str::trim).filter(|p| !p.is_empty()) {
            if quotes.len() >= QUOTES_MAX {
                return Err(refuse(
                    "widgets.quotes",
                    format!("has more than {QUOTES_MAX} quotes"),
                ));
            }
            quotes.push(text("widgets.quotes", part, QUOTE_MAX)?);
        }
        Ok(Self {
            rules,
            motd,
            quotes,
        })
    }

    /// The sysop's hide, lock and default settings.
    #[must_use]
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// The message of the day; empty when the sysop set none.
    #[must_use]
    pub fn motd(&self) -> &SysopText {
        &self.motd
    }

    /// The quote for the day number `day` (days since the epoch), if the sysop listed any.
    #[must_use]
    pub fn quote(&self, day: i64) -> Option<&SysopText> {
        if self.quotes.is_empty() {
            return None;
        }
        let at = usize::try_from(day.rem_euclid(i64::try_from(self.quotes.len()).unwrap_or(1)))
            .unwrap_or(0);
        self.quotes.get(at)
    }

    /// Whether any quote is set.
    #[must_use]
    pub fn has_quotes(&self) -> bool {
        !self.quotes.is_empty()
    }
}

fn ids(key: &'static str, raw: &str) -> Result<Vec<WidgetId>, WidgetsError> {
    let mut out: Vec<WidgetId> = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let id = WidgetId::parse(part)
            .ok_or_else(|| refuse(key, format!("{part:?} is not a widget")))?;
        if out.contains(&id) {
            return Err(refuse(key, format!("names {part:?} twice")));
        }
        out.push(id);
        if out.len() > WidgetId::ALL.len() {
            return Err(refuse(key, "names too many widgets"));
        }
    }
    Ok(out)
}

fn text(key: &'static str, raw: &str, max: usize) -> Result<SysopText, WidgetsError> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(SysopText::default());
    }
    if value.chars().count() > max {
        return Err(refuse(key, format!("is longer than {max} characters")));
    }
    if russh_bbs_core::sanitize::SingleLine::new(max + 1)
        .sanitize(value)
        .as_str()
        != value
    {
        return Err(refuse(
            key,
            "contains control, invisible or reserved characters",
        ));
    }
    match check_board_text(key, value) {
        Ok(()) => Ok(SysopText(value.to_owned())),
        Err(StringError::Claim(_)) => Err(refuse(
            key,
            "must not say the board is private, secure or encrypted: the server can read everything except PGP-encrypted mail and files members encrypt themselves",
        )),
        Err(other) => Err(refuse(key, other.to_string())),
    }
}
