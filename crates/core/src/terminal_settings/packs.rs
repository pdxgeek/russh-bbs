//! The sysop's Theme packs screen (UI-7): the styles installed, the pack files waiting in the
//! packs directory, and the keys to export one and import another.
//!
//! The files and the validation belong to the server (it owns the theme and art loaders); this
//! is the state the member flow keeps, so the keys, the `themes.manage` re-check and the return
//! to Sysop tools work like every other page. The flow turns a key into a [`PackRequest`]; the
//! server carries it out, audits it and answers with [`PackScreen::listed`].

use super::layout::ThemeName;
use crate::sanitize::SingleLine;

/// Longest message line, in characters.
const MESSAGE_MAX: usize = 200;

/// What the server is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackRequest {
    /// List the installed styles and the pack files.
    List,
    /// Write the installed style `name` to `packs/<name>.pack.toml`.
    Export(ThemeName),
    /// Validate and install the pack file called `file` from `packs/`.
    Import(String),
}

/// An installed style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRow {
    /// Its name.
    pub name: ThemeName,
    /// What it is called on screen.
    pub title: String,
    /// Whether it has a file to export (the built-in default has none).
    pub exportable: bool,
}

/// A pack file in the packs directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// The file name.
    pub file: String,
    /// The style it carries, or why it is refused.
    pub verdict: Result<ThemeName, String>,
}

/// What the server found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackListing {
    /// Installed styles.
    pub styles: Vec<StyleRow>,
    /// Pack files.
    pub files: Vec<FileRow>,
    /// The board has no packs directory (it was started without one).
    pub unavailable: bool,
}

/// A key on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKey {
    /// Up a row.
    Up,
    /// Down a row.
    Down,
    /// Export the style under the cursor.
    Export,
    /// Import the pack file under the cursor.
    Import,
    /// Look again.
    Rescan,
    /// Leave.
    Back,
}

/// What a key asks of the flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackStep {
    /// Redraw.
    Stay,
    /// Ask the server.
    Ask(PackRequest),
    /// Leave the screen.
    Leave,
}

/// The screen's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackScreen {
    listing: Option<PackListing>,
    row: usize,
    message: Option<String>,
}

impl PackScreen {
    /// A screen waiting for its first listing.
    #[must_use]
    pub const fn open() -> Self {
        Self {
            listing: None,
            row: 0,
            message: None,
        }
    }

    /// The listing, once the server has answered.
    #[must_use]
    pub const fn listing(&self) -> Option<&PackListing> {
        self.listing.as_ref()
    }

    /// The row under the cursor: styles first, then files.
    #[must_use]
    pub const fn row(&self) -> usize {
        self.row
    }

    /// The last outcome, as one sanitized line.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The server's answer: the listing, and a line saying what happened. The line may quote a
    /// file's reason, so it is sanitized here.
    pub fn listed(&mut self, listing: PackListing, message: Option<&str>) {
        let rows = listing.styles.len() + listing.files.len();
        self.row = self.row.min(rows.saturating_sub(1));
        self.listing = Some(listing);
        self.message =
            message.map(|text| SingleLine::new(MESSAGE_MAX).sanitize(text).into_string());
    }

    /// Apply `key`.
    pub fn press(&mut self, key: PackKey) -> PackStep {
        let Some(listing) = &self.listing else {
            return match key {
                PackKey::Back => PackStep::Leave,
                _ => PackStep::Stay,
            };
        };
        let (styles, files) = (listing.styles.len(), listing.files.len());
        match key {
            PackKey::Up => self.row = self.row.saturating_sub(1),
            PackKey::Down => {
                if self.row + 1 < styles + files {
                    self.row += 1;
                }
            }
            PackKey::Rescan => return PackStep::Ask(PackRequest::List),
            PackKey::Export => {
                if let Some(style) = listing.styles.get(self.row)
                    && style.exportable
                {
                    return PackStep::Ask(PackRequest::Export(style.name));
                }
            }
            PackKey::Import => {
                if let Some(file) = self
                    .row
                    .checked_sub(styles)
                    .and_then(|i| listing.files.get(i))
                    && file.verdict.is_ok()
                {
                    return PackStep::Ask(PackRequest::Import(file.file.clone()));
                }
            }
            PackKey::Back => return PackStep::Leave,
        }
        PackStep::Stay
    }
}
