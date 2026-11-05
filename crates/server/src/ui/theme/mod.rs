//! Theme packs: a palette and string overrides on top of the default string table, chosen per
//! member and switched live (M3-A, D82).
//!
//! **What a style can change (UI-2).** About twenty colour roles ([`Palette`]), how it draws
//! ([`Skin`]: border sets, title treatment, header decoration, backdrop, gradients) and the
//! wording of any string in the table except the reserved ones (the status-line captions). A
//! file may still set only the six original roles; the rest are derived. **What it cannot.**
//! The security markers: they are drawn by [`super::chrome`] with the reserved style, in the row
//! the layout takes out of the screen first, and nothing in this module is an input to that
//! drawing. A theme also cannot make a marker's *caption* say something else, because the
//! caption keys are reserved and refused in a theme file, nor put the words "private", "secure"
//! or "encrypted" or a lock glyph in any string (best effort, not a proof: the rule is that the
//! server can read everything except PGP mail and files members encrypt themselves).
//!
//! **Theme files are untrusted input.** The sysop's directory is read with a size cap per file
//! and a cap on how many files are read, parsed as TOML, and checked against a fixed schema:
//! unknown keys, unknown colors, unknown string keys, reserved keys, unsafe or oversized values
//! and a name that does not match the file are all refusals. A refused theme is skipped with a
//! logged warning and the default stays; nothing here can stop the server from booting.
//!
//! Shipped themes are embedded and must be valid; the unit tests load every one.

use std::fs;
use std::io::Read;
use std::path::Path;

use russh_bbs_core::terminal_settings::{DEFAULT_THEME, ThemeName};
use tracing::warn;

use super::depth::Tint;
use super::strings::{StringError, StringTable};

pub mod pack;
mod palette;
pub(crate) mod skin;
mod version;

pub use palette::{Base, Palette, ROLES, check_contrast, contrast_ratio};
pub use skin::{Backdrop, BorderSet, Element, HeaderDeco, Skin, TitleAlign, TitleTreatment};

/// Longest theme file read, in bytes.
pub const MAX_THEME_BYTES: usize = 32 * 1024;

/// Most files read from a theme directory; the rest are ignored with a warning.
pub const MAX_THEME_FILES: usize = 32;

/// A loaded theme: its name, palette and the full string table with its overrides applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    name: ThemeName,
    palette: Palette,
    skin: Skin,
    strings: StringTable,
    /// The display title and credit from `[meta]` (UI-7). The title is the name when unset.
    title: String,
    credit: String,
    /// The file as authored and the version it was written in, so an export can write it back
    /// (migrated forward) without re-deriving it from the resolved table. `None` for the
    /// built-in theme, which has no file.
    source: Option<Source>,
}

/// A theme file as read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    text: String,
    version: u32,
}

impl Theme {
    /// The built-in default theme over `strings`.
    #[must_use]
    pub fn builtin(strings: StringTable) -> Self {
        Self {
            name: ThemeName::default_theme(),
            palette: Palette::default(),
            skin: Skin::default(),
            strings,
            title: String::new(),
            credit: String::new(),
            source: None,
        }
    }

    /// What to call the style on a screen: its `[meta]` title, or its name.
    #[must_use]
    pub fn title(&self) -> &str {
        if self.title.is_empty() {
            self.name.as_str()
        } else {
            &self.title
        }
    }

    /// Who made it (`[meta] credit`), or empty.
    #[must_use]
    pub fn credit(&self) -> &str {
        &self.credit
    }

    /// The theme file as an export writes it, in the current version: the authored text, with
    /// the version line a version-1 file lacks put in front. `None` for the built-in theme,
    /// which is code and has no file to export.
    #[must_use]
    pub fn export_text(&self) -> Option<String> {
        let source = self.source.as_ref()?;
        Some(if source.version >= version::CURRENT {
            source.text.clone()
        } else {
            format!("version = {}\n{}", version::CURRENT, source.text)
        })
    }

    /// The theme's name.
    #[must_use]
    pub const fn name(&self) -> ThemeName {
        self.name
    }

    /// The theme's colors.
    #[must_use]
    pub const fn palette(&self) -> Palette {
        self.palette
    }

    /// How the theme draws (borders, titles, decoration).
    #[must_use]
    pub const fn skin(&self) -> Skin {
        self.skin
    }

    /// The theme's strings: the default table with this theme's overrides.
    #[must_use]
    pub const fn strings(&self) -> &StringTable {
        &self.strings
    }

    /// Parse and validate one theme file over `base` (the default table).
    pub fn parse(text: &str, base: &StringTable) -> Result<Self, ThemeError> {
        if text.len() > MAX_THEME_BYTES {
            return Err(ThemeError::TooLarge(text.len()));
        }
        let mut root: toml::Table = text
            .parse()
            .map_err(|error: toml::de::Error| ThemeError::Syntax(error.message().to_owned()))?;
        let file_version = version::migrate(&mut root)?;
        let (title, credit) = version::meta(root.remove("meta"))?;
        let name = match root.remove("name") {
            Some(toml::Value::String(name)) => {
                ThemeName::new(&name).map_err(|_| ThemeError::Name(name))?
            }
            _ => return Err(ThemeError::MissingName),
        };
        if name.as_str() == DEFAULT_THEME {
            return Err(ThemeError::ReservedName);
        }
        let palette = match root.remove("colors") {
            Some(toml::Value::Table(colors)) => {
                let palette = palette_of(&colors)?;
                let explicit: Vec<&str> = colors.keys().map(String::as_str).collect();
                check_contrast(&palette, &explicit)?;
                palette
            }
            Some(_) => return Err(ThemeError::Colors("`colors` must be a table".to_owned())),
            None => Palette::default(),
        };
        let skin = match root.remove("style") {
            Some(toml::Value::Table(style)) => Skin::from_table(&style)?,
            Some(_) => return Err(ThemeError::Style("`style` must be a table".to_owned())),
            None => Skin::default(),
        };
        // Everything left is string overrides, in the same nested form as the table itself.
        let overrides = super::strings::flatten_table(&root)?;
        let strings = base.with_overrides(&overrides)?;
        Ok(Self {
            name,
            palette,
            skin,
            strings,
            title,
            credit,
            source: Some(Source {
                text: text.to_owned(),
                version: file_version,
            }),
        })
    }
}

/// Why a theme was refused. Every message names what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    /// The file is larger than [`MAX_THEME_BYTES`].
    #[error("theme file is {0} bytes; the limit is {MAX_THEME_BYTES}")]
    TooLarge(usize),
    /// Not valid TOML.
    #[error("theme file is not valid TOML: {0}")]
    Syntax(String),
    /// No `name = "..."`.
    #[error("theme file has no `name`")]
    MissingName,
    /// The name is not a valid theme name.
    #[error("theme name {0:?} is not valid (1 to 24 of a-z, 0-9, _ and -)")]
    Name(String),
    /// The name `default` belongs to the built-in theme.
    #[error("theme name `default` is reserved for the built-in theme")]
    ReservedName,
    /// The file name and the theme name differ.
    #[error("theme name `{name}` does not match the file name `{file}`")]
    NameMismatch {
        /// The `name` in the file.
        name: String,
        /// The file's stem.
        file: String,
    },
    /// Two files name the same theme.
    #[error("theme `{0}` is defined twice")]
    Duplicate(String),
    /// `version` is not a number this server reads.
    #[error("theme version {0} is not understood (this server reads 1 to {max})", max = version::CURRENT)]
    Version(String),
    /// A problem in `[meta]`.
    #[error("theme meta: {0}")]
    Meta(String),
    /// A problem in `[colors]`.
    #[error("theme colors: {0}")]
    Colors(String),
    /// A problem in `[style]`.
    #[error("theme style: {0}")]
    Style(String),
    /// A text colour cannot be read against its background.
    #[error(
        "theme colors: `{role}` on `{against}` has contrast {ratio}; at least {least} is needed to read it"
    )]
    Contrast {
        /// The text role.
        role: String,
        /// The background role.
        against: String,
        /// The ratio found.
        ratio: String,
        /// The ratio needed.
        least: String,
    },
    /// A problem in the strings.
    #[error(transparent)]
    Strings(#[from] StringError),
    /// The file could not be read.
    #[error("theme file could not be read: {0}")]
    Read(String),
}

/// Read `[colors]`: any of the [`ROLES`]. The six original roles set first, then the rest derive
/// from them, then every explicit role is applied, so a file that sets only `border` still gets
/// matching bars and selection.
fn palette_of(colors: &toml::Table) -> Result<Palette, ThemeError> {
    let mut explicit: Vec<(&str, Tint)> = Vec::new();
    for (role, value) in colors {
        if !ROLES.contains(&role.as_str()) {
            return Err(ThemeError::Colors(format!("`{role}` is not a color role")));
        }
        let tint = skin::parse_tint(value)
            .map_err(|message| ThemeError::Colors(format!("`{role}`: {message}")))?;
        explicit.push((role.as_str(), tint));
    }
    let pick = |role: &str, default: Tint| {
        explicit
            .iter()
            .find(|(name, _)| *name == role)
            .map_or(default, |(_, tint)| *tint)
    };
    let defaults = Base::DEFAULT;
    let mut palette = Palette::from_base(Base {
        heading: pick("heading", defaults.heading),
        key: pick("key", defaults.key),
        border: pick("border", defaults.border),
        info: pick("info", defaults.info),
        error: pick("error", defaults.error),
        selected: pick("selected", defaults.selected),
    });
    for (role, tint) in explicit {
        if let Some(slot) = palette.role_mut(role) {
            *slot = tint;
        }
    }
    Ok(palette)
}

/// The themes a server offers: the default first, then the shipped ones, then the sysop's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSet {
    themes: Vec<Theme>,
}

impl ThemeSet {
    /// The default theme and the shipped packs over `base`. Fails only if a shipped file is
    /// invalid, which is a build bug the unit tests catch.
    pub fn shipped(base: &StringTable) -> Result<Self, ThemeError> {
        let mut themes = vec![Theme::builtin(base.clone())];
        for text in SHIPPED {
            let theme = Theme::parse(text, base)?;
            if themes.iter().any(|t| t.name == theme.name) {
                return Err(ThemeError::Duplicate(theme.name.to_string()));
            }
            themes.push(theme);
        }
        Ok(Self { themes })
    }

    /// Add the sysop's themes from `dir` (`*.toml`, not recursive). **Never fails**: a missing
    /// directory is fine, and an unreadable, oversized, invalid or duplicate file is skipped
    /// with a warning. Returns how many were added.
    pub fn add_dir(&mut self, dir: &Path, base: &StringTable) -> usize {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(error) => {
                warn!(dir = %dir.display(), %error, "theme directory unreadable; using shipped themes only");
                return 0;
            }
        };
        let mut added = 0;
        let mut seen = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            seen += 1;
            if seen > MAX_THEME_FILES {
                warn!(dir = %dir.display(), limit = MAX_THEME_FILES, "too many theme files; the rest are ignored");
                break;
            }
            match self.load_file(&path, base) {
                Ok(()) => added += 1,
                Err(error) => {
                    warn!(file = %path.display(), %error, "theme refused; the default stays");
                }
            }
        }
        added
    }

    fn load_file(&mut self, path: &Path, base: &StringTable) -> Result<(), ThemeError> {
        let text = read_bounded(path)?;
        let theme = Theme::parse(&text, base)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem != theme.name.as_str() {
            return Err(ThemeError::NameMismatch {
                name: theme.name.to_string(),
                file: stem.to_owned(),
            });
        }
        if self.themes.iter().any(|t| t.name == theme.name) {
            return Err(ThemeError::Duplicate(theme.name.to_string()));
        }
        self.themes.push(theme);
        Ok(())
    }

    /// The theme called `name`, or the default if none is loaded under it (a theme the sysop
    /// removed, or a stored name from before a restart).
    #[must_use]
    pub fn get(&self, name: ThemeName) -> &Theme {
        self.themes
            .iter()
            .find(|theme| theme.name == name)
            .unwrap_or_else(|| self.default_theme())
    }

    /// Apply `overrides` (the board's name) to every theme's table, after the theme's own, so
    /// the sysop's board name wins over a theme that also set it. A value the string table
    /// refuses leaves that theme as it was and is logged.
    pub fn apply_overrides(&mut self, overrides: &std::collections::BTreeMap<String, String>) {
        for theme in &mut self.themes {
            match theme.strings.with_overrides(overrides) {
                Ok(strings) => theme.strings = strings,
                Err(error) => {
                    warn!(theme = %theme.name, %error, "board text refused for this theme");
                }
            }
        }
    }

    /// Whether a theme called `name` is loaded.
    #[must_use]
    pub fn contains(&self, name: ThemeName) -> bool {
        self.themes.iter().any(|theme| theme.name == name)
    }

    /// The default theme.
    #[must_use]
    pub fn default_theme(&self) -> &Theme {
        // `shipped` always puts the built-in theme first and nothing removes it.
        &self.themes[0]
    }

    /// Every theme's name, the default first: the list "next theme" cycles through.
    #[must_use]
    pub fn names(&self) -> Vec<ThemeName> {
        self.themes.iter().map(Theme::name).collect()
    }
}

/// Read at most [`MAX_THEME_BYTES`] of `path`; a longer file is refused without reading the
/// rest, so a huge or endless file (a device, a symlink to one) cannot exhaust memory.
fn read_bounded(path: &Path) -> Result<String, ThemeError> {
    let file = fs::File::open(path).map_err(|error| ThemeError::Read(error.to_string()))?;
    let mut text = String::new();
    file.take(MAX_THEME_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| ThemeError::Read(error.to_string()))?;
    if text.len() > MAX_THEME_BYTES {
        return Err(ThemeError::TooLarge(text.len()));
    }
    Ok(text)
}

/// The shipped packs, embedded: the six styles, then the older colour-only themes.
const SHIPPED: [&str; 8] = [
    include_str!("../themes/phosphor.toml"),
    include_str!("../themes/neon.toml"),
    include_str!("../themes/paper.toml"),
    include_str!("../themes/amber.toml"),
    include_str!("../themes/deepspace.toml"),
    include_str!("../themes/ansi.toml"),
    include_str!("../themes/green.toml"),
    include_str!("../themes/contrast.toml"),
];
