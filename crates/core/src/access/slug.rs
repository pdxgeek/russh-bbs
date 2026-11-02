//! Names of roles and groups: short ASCII slugs, so a decision reason reads `moderator via
//! staff` without a join and a name is never content that needs the sanitizer.

use std::borrow::Cow;
use std::fmt;

/// Longest slug, in characters. The same bound is a CHECK in the database.
pub const SLUG_MAX_CHARS: usize = 32;

/// A role or group name: 1 to 32 characters of `a-z`, `0-9` and `-`, not starting or ending
/// with `-`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(Cow<'static, str>);

/// Why a string is not a [`Slug`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSlug;

impl fmt::Display for InvalidSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a valid role or group name")
    }
}

impl std::error::Error for InvalidSlug {}

impl Slug {
    /// The built-in `waiting` role.
    pub const WAITING: Self = Self(Cow::Borrowed("waiting"));
    /// The built-in `member` role.
    pub const MEMBER: Self = Self(Cow::Borrowed("member"));
    /// The built-in `trusted` role.
    pub const TRUSTED: Self = Self(Cow::Borrowed("trusted"));
    /// The built-in `moderator` role.
    pub const MODERATOR: Self = Self(Cow::Borrowed("moderator"));
    /// The built-in `cosysop` role.
    pub const COSYSOP: Self = Self(Cow::Borrowed("cosysop"));
    /// The built-in `sysop` role.
    pub const SYSOP: Self = Self(Cow::Borrowed("sysop"));
    /// The built-in `everyone` group.
    pub const EVERYONE: Self = Self(Cow::Borrowed("everyone"));
    /// The built-in `staff` group.
    pub const STAFF: Self = Self(Cow::Borrowed("staff"));

    /// Validate a name. Call this at a boundary (config, a form, a stored row).
    pub fn parse(text: &str) -> Result<Self, InvalidSlug> {
        let well_formed = !text.is_empty()
            && text.len() <= SLUG_MAX_CHARS
            && !text.starts_with('-')
            && !text.ends_with('-')
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if well_formed {
            Ok(Self(Cow::Owned(text.to_owned())))
        } else {
            Err(InvalidSlug)
        }
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Slug").field(&self.0).finish()
    }
}
