//! A feed item as the board keeps it: every field already cleaned, typed so that nothing
//! uncleaned can be stored. Built only by [`super::parse`].

use sha2::{Digest, Sha256};

use crate::drafts::Body;
use crate::sanitize::Sanitized;
use crate::store::UnixSeconds;

/// An item ready to become a thread: a subject and an opening post with no author account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    /// SHA-256 (hex) of the item's guid, or its link when it has none. The same value means the
    /// same item.
    pub key_hash: String,
    /// SHA-256 (hex) of the cleaned title, link and text. The same value means the same
    /// content even under a new guid.
    pub content_hash: String,
    /// The thread subject (single line, 1 to 120 characters).
    pub subject: Sanitized,
    /// The opening post.
    pub body: Body,
    /// When the item says it was published, never later than when it was read.
    pub at: UnixSeconds,
}

/// Lower-case hex of a SHA-256 over `parts`, each preceded by its length so that two different
/// splits of the same bytes cannot collide.
pub(super) fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    let out = hash.finalize();
    let mut hex = String::with_capacity(64);
    for byte in out {
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    hex
}
