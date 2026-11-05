//! The screen framework's drawing side (M2-A): the **classic hotkey layout** over the menu
//! tree in `russh_bbs_core::menu`, the reserved **status row** for security markers, and the
//! themeable **string table**.
//!
//! The four layers of `docs/ui.md` map to code like this: the menu tree is core data, the layout
//! is [`classic`], the strings (and, later, themes) are [`strings`], and the icon tier comes
//! from the member's settings. Adding a layout later means a new module beside [`classic`]; the
//! tree does not change.
//!
//! [`Ui`] is built once at boot. It checks that the shipped menu tree is valid and that the
//! string table holds exactly the keys the tree and the layout use, so a bad table stops the
//! server with a message naming the key instead of drawing blanks.

pub mod art;
pub(crate) mod backdrop;
pub mod board;
pub(crate) mod chrome;
pub(crate) mod classic;
pub(crate) mod compact;
pub mod depth;
pub mod editor;
pub(crate) mod frame;
pub(crate) mod gallery;
pub(crate) mod keys;
pub(crate) mod layout;
pub(crate) mod lightbar;
pub(crate) mod pager;
pub(crate) mod panes;
pub(crate) mod popup;
pub mod strings;
pub(crate) mod table;
pub mod theme;
pub mod widgets;

use std::path::Path;
use std::sync::Arc;

use russh_bbs_core::menu::{MenuError, main_tree};
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings, ThemeName};

pub use board::Board;

pub use strings::{StringError, StringTable};
pub use theme::{Palette, Theme, ThemeError, ThemeSet};

/// Why the UI could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UiError {
    /// The menu tree is invalid.
    #[error("menu tree: {0}")]
    Menu(#[from] MenuError),
    /// The string table is invalid.
    #[error("string table: {0}")]
    Strings(#[from] StringError),
    /// A shipped theme is invalid (a build bug; sysop themes never produce this, they are
    /// skipped with a warning).
    #[error("shipped theme: {0}")]
    Theme(#[from] ThemeError),
    /// Bundled art is invalid (a build bug).
    #[error("bundled art: {0}")]
    Art(String),
}

/// What every screen on the framework draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    themes: ThemeSet,
    board: Board,
    /// The styles members may pick from (the board's list, or every loaded style).
    styles: Vec<ThemeName>,
    /// The layouts members may pick from.
    layouts: Vec<LayoutId>,
    mail: MailKeysHandle,
    doors: crate::doors::Doors,
    /// The banner and header art (UI-3): bundled, plus the sysop's `art/` directory.
    art: art::ArtSet,
    widgets: widgets::WidgetSettings,
    /// Where theme packs are exported to, imported from and installed (UI-7).
    packs: Option<PackDirs>,
}

/// The directories theme packs use, all beside the host key like `themes/` and `art/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackDirs {
    /// `packs/`: exported packs are written here and imports are read from here.
    pub packs: std::path::PathBuf,
    /// `themes/`: an imported style's theme file is installed here.
    pub themes: std::path::PathBuf,
    /// `art/`: an imported style's art is installed here.
    pub art: std::path::PathBuf,
}

/// The keys mail is sealed with (M3-I), carried to the sessions beside the screens' words because
/// every transport already hands the one shared [`Ui`] to each session. Not presentation: it is
/// compared by identity and prints nothing of the keys.
#[derive(Clone, Default)]
struct MailKeysHandle(Option<Arc<russh_bbs_core::mail::MailKeys>>);

impl PartialEq for MailKeysHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

impl Eq for MailKeysHandle {}

impl std::fmt::Debug for MailKeysHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "MailKeys(held)"
        } else {
            "MailKeys(none)"
        })
    }
}

impl Ui {
    /// The shipped table, tree and themes. Fails only if the build itself is inconsistent,
    /// which a unit test also catches.
    pub fn standard() -> Result<Self, UiError> {
        Self::from_table(strings::DEFAULT_TOML)
    }

    /// The same screens with the board's mail keys, so sessions can open and seal mail. A board
    /// built without them says honestly that mail is not available.
    #[must_use]
    pub fn with_mail_keys(mut self, keys: Arc<russh_bbs_core::mail::MailKeys>) -> Self {
        self.mail = MailKeysHandle(Some(keys));
        self
    }

    /// The mail keys, if the board has them.
    #[must_use]
    pub fn mail_keys(&self) -> Option<Arc<russh_bbs_core::mail::MailKeys>> {
        self.mail.0.clone()
    }

    /// The same screens with the doors this board runs (M4-D0). Without them the Doors menu item
    /// opens an empty list.
    #[must_use]
    pub fn with_doors(mut self, doors: crate::doors::Doors) -> Self {
        self.doors = doors;
        self
    }

    /// The same screens with the board's widget settings (UI-10): which widgets exist, their
    /// defaults and locks, the message of the day and the quotes.
    #[must_use]
    pub fn with_widgets(mut self, widgets: widgets::WidgetSettings) -> Self {
        self.widgets = widgets;
        self
    }

    /// The board's widget settings.
    #[must_use]
    pub fn widgets(&self) -> &widgets::WidgetSettings {
        &self.widgets
    }

    /// The doors this board runs.
    #[must_use]
    pub fn doors(&self) -> &crate::doors::Doors {
        &self.doors
    }

    /// Like [`Ui::standard`], with the sysop's themes from `dir` added. A bad theme file never
    /// fails this: it is skipped with a logged warning (see [`theme`]).
    pub fn standard_with_themes(dir: &Path) -> Result<Self, UiError> {
        let mut ui = Self::standard()?;
        let base = ui.strings().clone();
        ui.themes.add_dir(dir, &base);
        Ok(ui)
    }

    /// Build from a table in TOML (the loader a theme or translation uses), validated against
    /// the menu tree and the layout's own keys.
    pub fn from_table(toml: &str) -> Result<Self, UiError> {
        let tree = main_tree();
        tree.validate()?;
        let required: Vec<&str> = keys::FRAMEWORK
            .iter()
            .copied()
            .chain(keys::LAYOUT_KEYS.iter().copied())
            .chain(keys::PREVIEW_KEYS.iter().copied())
            .chain(keys::GALLERY_KEYS.iter().copied())
            .chain(tree.text_keys())
            .chain(widgets::TEXT_KEYS.iter().copied())
            .chain(widgets::screen::TEXT_KEYS.iter().copied())
            .chain(russh_bbs_core::profiles::TEXT_KEYS.iter().copied())
            .chain(russh_bbs_core::community::TEXT_KEYS.iter().copied())
            .chain(russh_bbs_core::queue::keys::TEXT_KEYS.iter().copied())
            .chain(
                russh_bbs_core::terminal_settings::strings::TEXT_KEYS
                    .iter()
                    .copied(),
            )
            .collect();
        let strings = StringTable::parse(toml, &required)?;
        Ok(Self {
            styles: Vec::new(),
            layouts: LayoutId::ALL.to_vec(),
            board: Board::default(),
            themes: ThemeSet::shipped(&strings)?,
            mail: MailKeysHandle::default(),
            doors: crate::doors::Doors::none(),
            art: art::ArtSet::bundled().map_err(|error| UiError::Art(error.to_string()))?,
            widgets: widgets::WidgetSettings::default(),
            packs: None,
        })
    }

    /// The same screens with the directories theme packs use (UI-7). Without them the sysop's
    /// Theme packs screen says packs are not available.
    #[must_use]
    pub fn with_pack_dirs(mut self, dirs: PackDirs) -> Self {
        self.packs = Some(dirs);
        self
    }

    /// The pack directories, if the board has them.
    #[must_use]
    pub fn pack_dirs(&self) -> Option<&PackDirs> {
        self.packs.as_ref()
    }

    /// The loaded themes.
    #[must_use]
    pub fn theme_set(&self) -> &ThemeSet {
        &self.themes
    }

    /// The same screens with the sysop's art files from `dir` added (UI-3). A bad file is
    /// skipped with a logged reason and never fails this.
    #[must_use]
    pub fn with_art_dir(mut self, dir: &Path) -> Self {
        self.art.add_dir(dir);
        self
    }

    /// The art the board has.
    #[must_use]
    pub fn art(&self) -> &art::ArtSet {
        &self.art
    }

    /// What screens ask for art with, for `style`.
    pub(crate) fn art_ctx(&self, style: ThemeName) -> art::draw::ArtCtx<'_> {
        art::draw::ArtCtx {
            set: &self.art,
            style,
            board: self.board.name(),
            enabled: self.board.art(),
        }
    }

    /// The same screens with the board's identity and limits (UI-2). The name replaces the
    /// string table's title in every style; a style or layout the board names that is not
    /// loaded is ignored with a warning, never an error (a missing theme file must not stop the
    /// boot); a list that ends up empty means "everything".
    #[must_use]
    pub fn with_board(mut self, board: Board) -> Self {
        if let Some(title) = board.title_override() {
            let overrides = std::collections::BTreeMap::from([(keys::TITLE.to_owned(), title)]);
            self.themes.apply_overrides(&overrides);
        }
        let known = |name: &ThemeName, what: &str| {
            let loaded = self.themes.contains(*name);
            if !loaded {
                tracing::warn!(style = %name, "{what} names a style that is not loaded; ignored");
            }
            loaded
        };
        self.styles = board
            .styles()
            .iter()
            .filter(|name| known(name, "board.styles"))
            .copied()
            .collect();
        self.layouts = if board.layouts().is_empty() {
            LayoutId::ALL.to_vec()
        } else {
            board.layouts().to_vec()
        };
        if let Some(style) = board.style().filter(|style| !known(style, "board.style")) {
            tracing::warn!(%style, "board.style is ignored");
        }
        self.board = board;
        self
    }

    /// The board's identity.
    #[must_use]
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// The styles a member may pick from, the default style first when it is allowed.
    #[must_use]
    pub fn allowed_styles(&self) -> Vec<ThemeName> {
        if self.styles.is_empty() {
            self.themes.names()
        } else {
            self.styles.clone()
        }
    }

    /// The layouts a member may pick from.
    #[must_use]
    pub fn allowed_layouts(&self) -> Vec<LayoutId> {
        self.layouts.clone()
    }

    /// The style `settings` draws with: the member's pick, else the board's default, kept inside
    /// what the board allows (a single allowed style is a lock and wins over what was stored).
    #[must_use]
    pub fn style_for(&self, settings: &TerminalSettings) -> ThemeName {
        let allowed = self.allowed_styles();
        if let [only] = allowed.as_slice() {
            return *only;
        }
        let board_default = self
            .board
            .style()
            .filter(|name| self.themes.contains(*name))
            .unwrap_or_else(ThemeName::default_theme);
        let wanted = settings.theme_choice().unwrap_or(board_default);
        if allowed.contains(&wanted) {
            wanted
        } else if allowed.contains(&board_default) {
            board_default
        } else {
            allowed.first().copied().unwrap_or_default()
        }
    }

    /// The layout `settings` chooses, before the terminal's size is considered.
    #[must_use]
    pub fn layout_for(&self, settings: &TerminalSettings) -> LayoutId {
        let allowed = &self.layouts;
        if let [only] = allowed.as_slice() {
            return *only;
        }
        let board_default = self.board.layout().unwrap_or_default();
        let wanted = settings.layout_choice().unwrap_or(board_default);
        if allowed.contains(&wanted) {
            wanted
        } else if allowed.contains(&board_default) {
            board_default
        } else {
            allowed.first().copied().unwrap_or_default()
        }
    }

    /// The layout a member who never chose follows.
    #[must_use]
    pub fn board_default_layout(&self) -> LayoutId {
        self.board.layout().unwrap_or_default()
    }

    /// The style a member who never chose follows (the board's, if it is loaded).
    #[must_use]
    pub fn board_default_style(&self) -> ThemeName {
        self.board
            .style()
            .filter(|name| self.themes.contains(*name))
            .unwrap_or_default()
    }

    /// The theme `settings` draws with.
    #[must_use]
    pub fn theme_for(&self, settings: &TerminalSettings) -> &Theme {
        self.themes.get(self.style_for(settings))
    }

    /// How to draw for `settings` in `area` at the depth the client can show: the style, the
    /// layout in force (a smaller one if the terminal is too small), the board's accent and the
    /// member's own limits.
    pub(crate) fn look(
        &self,
        settings: &TerminalSettings,
        area: ratatui::layout::Rect,
        depth: depth::ColorDepth,
    ) -> crate::settings::Look {
        let layout = layout::effective(self.layout_for(settings), area);
        crate::settings::Look::for_theme(settings, layout, self.theme_for(settings))
            .with_board(self.board.accent(), self.board.motion())
            .at_depth(depth)
    }

    /// The default theme's validated string table.
    #[must_use]
    pub fn strings(&self) -> &StringTable {
        self.themes.default_theme().strings()
    }

    /// The theme called `name`; the default if none is loaded under that name.
    #[must_use]
    pub fn theme(&self, name: ThemeName) -> &Theme {
        self.themes.get(name)
    }

    /// The names of the loaded themes, the default first.
    #[must_use]
    pub fn theme_names(&self) -> Vec<ThemeName> {
        self.themes.names()
    }
}

#[cfg(test)]
mod gallery_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod resolve_tests;
#[cfg(test)]
mod snapshots;
#[cfg(test)]
mod styles_tests;
#[cfg(test)]
mod tests;
