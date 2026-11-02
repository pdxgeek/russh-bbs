//! The closed set of limit kinds and the value a limit takes.

use strum::{EnumString, IntoStaticStr, VariantArray};

use crate::store::StoreError;

/// What a limit counts (`docs/access.md`, "Resource limits"). A closed enum: a new kind is a
/// new variant, its row in `docs/access.md`, and a migration giving the built-in roles a value.
///
/// The credits earn rate is in the design but not here: there is no credit economy until the
/// doors milestone, and a limit nothing can spend would be dead configuration.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
pub enum LimitKind {
    /// Minutes of signed-in time per UTC day.
    #[strum(serialize = "minutes_per_day")]
    MinutesPerDay,
    /// Signed-in sessions started per UTC day.
    #[strum(serialize = "calls_per_day")]
    CallsPerDay,
    /// Signed-in sessions open at once (counted in this process).
    #[strum(serialize = "max_sessions")]
    MaxSessions,
    /// Posts per rolling hour.
    #[strum(serialize = "posts_per_hour")]
    PostsPerHour,
    /// Bytes uploaded per UTC day.
    #[strum(serialize = "upload_bytes_per_day")]
    UploadBytesPerDay,
    /// Bytes in one uploaded file.
    #[strum(serialize = "upload_bytes_per_file")]
    UploadBytesPerFile,
    /// Mail messages sent per UTC day.
    #[strum(serialize = "mail_per_day")]
    MailPerDay,
    /// Invite codes created per calendar month (UTC).
    #[strum(serialize = "invites_per_month")]
    InvitesPerMonth,
}

impl LimitKind {
    /// Every kind, in the order of `docs/access.md`.
    pub const ALL: &'static [Self] = <Self as VariantArray>::VARIANTS;

    /// The stored and documented name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// Parse a stored or documented name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }

    /// The unit of the number, for messages and the docs.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::MinutesPerDay => "minutes",
            Self::CallsPerDay => "calls",
            Self::MaxSessions => "sessions",
            Self::PostsPerHour => "posts",
            Self::UploadBytesPerDay | Self::UploadBytesPerFile => "bytes",
            Self::MailPerDay => "messages",
            Self::InvitesPerMonth => "invites",
        }
    }
}

/// How much of something is allowed.
///
/// The derived order puts every [`Limit::AtMost`] below [`Limit::Unlimited`], so "the most
/// generous" of several is simply the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Limit {
    /// At most this many. Zero means none at all.
    AtMost(u64),
    /// No limit.
    Unlimited,
}

impl Limit {
    /// Read a stored value: `NULL` is unlimited, a negative number is not one the code writes.
    pub fn from_stored(value: Option<i64>) -> Result<Self, StoreError> {
        match value {
            None => Ok(Self::Unlimited),
            Some(n) => u64::try_from(n)
                .map(Self::AtMost)
                .map_err(|_| StoreError::Corrupt("a limit is negative".to_owned())),
        }
    }

    /// The value to store, or `None` if it does not fit the database's integer.
    #[must_use]
    pub fn to_stored(self) -> Option<Option<i64>> {
        match self {
            Self::Unlimited => Some(None),
            Self::AtMost(n) => i64::try_from(n).ok().map(Some),
        }
    }

    /// The cap as a number, `None` when unlimited.
    #[must_use]
    pub const fn cap(self) -> Option<u64> {
        match self {
            Self::AtMost(n) => Some(n),
            Self::Unlimited => None,
        }
    }
}
