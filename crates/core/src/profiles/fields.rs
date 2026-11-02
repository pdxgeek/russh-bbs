//! The fixed set of profile fields, their length caps, and the one place they are sanitized.
//!
//! A profile is five short text fields and nothing else: no avatar, no custom fields, no
//! links. Every field is single-line text that has passed the shared sanitizer ([`Profile`] can
//! only be built from raw text through [`Profile::from_raw`] or [`Profile::from_stored`]), and
//! it is sanitized again when it is drawn, so a row edited in the database cannot reach a
//! terminal unclean (AGENTS.md: sanitize on write and on render).

use crate::sanitize::{Sanitized, SingleLine};

/// Most characters in a display name.
pub const DISPLAY_NAME_MAX: usize = 40;
/// Most characters in pronouns.
pub const PRONOUNS_MAX: usize = 24;
/// Most characters in a location.
pub const LOCATION_MAX: usize = 40;
/// Most characters in the "about" text.
pub const ABOUT_MAX: usize = 280;
/// Most characters in a website. It is plain text: never fetched, never a link (SSRF rule).
pub const WEBSITE_MAX: usize = 100;

/// One of the five profile fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// A name shown beside the handle.
    DisplayName,
    /// Pronouns, free text.
    Pronouns,
    /// Where the member says they are, free text.
    Location,
    /// A short text about the member.
    About,
    /// A website, shown as plain text.
    Website,
}

impl Field {
    /// Every field, in the order screens show and edit them.
    pub const ALL: [Self; 5] = [
        Self::DisplayName,
        Self::Pronouns,
        Self::Location,
        Self::About,
        Self::Website,
    ];

    /// The most characters the field keeps.
    #[must_use]
    pub const fn max_chars(self) -> usize {
        match self {
            Self::DisplayName => DISPLAY_NAME_MAX,
            Self::Pronouns => PRONOUNS_MAX,
            Self::Location => LOCATION_MAX,
            Self::About => ABOUT_MAX,
            Self::Website => WEBSITE_MAX,
        }
    }

    /// A stable ASCII name, for an audit note.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DisplayName => "display_name",
            Self::Pronouns => "pronouns",
            Self::Location => "location",
            Self::About => "about",
            Self::Website => "website",
        }
    }

    /// Sanitize `raw` for this field: single line, capped, and trimmed.
    #[must_use]
    pub fn sanitize(self, raw: &str) -> Sanitized {
        let context = SingleLine::new(self.max_chars());
        // The second pass drops spaces that control characters or spaces left at the ends; it
        // is idempotent, so running the sanitizer twice changes nothing else.
        context.sanitize(context.sanitize(raw).as_str().trim())
    }

    /// The string-table key of the field's label.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::DisplayName => super::keys::LABEL_DISPLAY_NAME,
            Self::Pronouns => super::keys::LABEL_PRONOUNS,
            Self::Location => super::keys::LABEL_LOCATION,
            Self::About => super::keys::LABEL_ABOUT,
            Self::Website => super::keys::LABEL_WEBSITE,
        }
    }
}

/// Text as typed or read from a store, before sanitizing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawProfile {
    /// Display name.
    pub display_name: String,
    /// Pronouns.
    pub pronouns: String,
    /// Location.
    pub location: String,
    /// About text.
    pub about: String,
    /// Website.
    pub website: String,
}

impl RawProfile {
    /// The raw text of one field.
    #[must_use]
    pub fn get(&self, field: Field) -> &str {
        match field {
            Field::DisplayName => &self.display_name,
            Field::Pronouns => &self.pronouns,
            Field::Location => &self.location,
            Field::About => &self.about,
            Field::Website => &self.website,
        }
    }

    /// Replace one field.
    pub fn set(&mut self, field: Field, value: String) {
        match field {
            Field::DisplayName => self.display_name = value,
            Field::Pronouns => self.pronouns = value,
            Field::Location => self.location = value,
            Field::About => self.about = value,
            Field::Website => self.website = value,
        }
    }
}

/// A profile whose every field has passed the sanitizer and its cap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    display_name: Sanitized,
    pronouns: Sanitized,
    location: Sanitized,
    about: Sanitized,
    website: Sanitized,
}

impl Profile {
    /// Sanitize typed text into a profile (the write path).
    #[must_use]
    pub fn from_raw(raw: &RawProfile) -> Self {
        Self {
            display_name: Field::DisplayName.sanitize(&raw.display_name),
            pronouns: Field::Pronouns.sanitize(&raw.pronouns),
            location: Field::Location.sanitize(&raw.location),
            about: Field::About.sanitize(&raw.about),
            website: Field::Website.sanitize(&raw.website),
        }
    }

    /// Sanitize text read from a store (the render path). Same rules as [`Profile::from_raw`]:
    /// a store is never trusted to hold clean text.
    #[must_use]
    pub fn from_stored(raw: &RawProfile) -> Self {
        Self::from_raw(raw)
    }

    /// The sanitized text of one field.
    #[must_use]
    pub fn get(&self, field: Field) -> &str {
        match field {
            Field::DisplayName => self.display_name.as_str(),
            Field::Pronouns => self.pronouns.as_str(),
            Field::Location => self.location.as_str(),
            Field::About => self.about.as_str(),
            Field::Website => self.website.as_str(),
        }
    }

    /// The profile as editable text, for a form to start from.
    #[must_use]
    pub fn to_raw(&self) -> RawProfile {
        RawProfile {
            display_name: self.display_name.as_str().to_owned(),
            pronouns: self.pronouns.as_str().to_owned(),
            location: self.location.as_str().to_owned(),
            about: self.about.as_str().to_owned(),
            website: self.website.as_str().to_owned(),
        }
    }

    /// The fields whose text differs from `other`, in screen order. Names only, never values,
    /// so an audit row can say what changed without copying content.
    #[must_use]
    pub fn changed_from(&self, other: &Self) -> Vec<Field> {
        Field::ALL
            .into_iter()
            .filter(|field| self.get(*field) != other.get(*field))
            .collect()
    }
}
