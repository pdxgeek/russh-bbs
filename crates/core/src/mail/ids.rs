//! Ids for mail. A message id is chosen by core (random), not by the database, because it is
//! part of what the message is sealed against (see [`super::seal`]): it has to exist before the
//! row does.

use std::fmt;

use rand::TryRng as _;
use rand::rngs::SysRng;

use super::MailError;

/// A message in one mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(i64);

impl MessageId {
    /// Wrap a stored id.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }

    /// A fresh random id: 62 random bits, never zero, so it fits a signed 64-bit column on both
    /// backends and is not guessable from another message's id.
    ///
    /// # Errors
    /// [`MailError::Random`] when the operating system's random source fails.
    pub fn random() -> Result<Self, MailError> {
        loop {
            let mut bytes = [0_u8; 8];
            SysRng
                .try_fill_bytes(&mut bytes)
                .map_err(|_| MailError::Random)?;
            let raw = i64::try_from(u64::from_be_bytes(bytes) >> 2).unwrap_or(0);
            if raw != 0 {
                return Ok(Self(raw));
            }
        }
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A report a recipient sent to staff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReportId(i64);

impl ReportId {
    /// Wrap a stored id.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for ReportId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
