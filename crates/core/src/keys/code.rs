//! The one-time codes of FIX-M3-11 (D97): the device-link code and the staff recovery code.
//!
//! Like an invite code (D67) a code is random, shown once, and **only its hash is stored**.
//! The hash is SHA-256 over a domain prefix that names the kind and the normalized text, so a
//! link code can never be redeemed as a recovery code or as an invite, and the store finds a
//! code by an indexed equality on the hash (nothing compares a secret byte by byte).
//!
//! The codes are shorter than an invite code because they are typed on a new machine in a few
//! minutes and are cheap to refuse: a link code lives 10 minutes and carries 50 random bits, a
//! recovery code lives 24 hours and carries 80. Redemption attempts are rate limited per network
//! and per key in the waiting room, so a guess costs far more than the code is worth.
//!
//! Randomness is passed in, so this module is deterministic to test.

use std::fmt;

use sha2::{Digest as _, Sha256};

/// Crockford base32 (no padding): digits and letters without `I`, `L`, `O`, `U`.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Most typed characters that are looked at.
pub const CODE_TEXT_MAX_CHARS: usize = 32;

/// Characters in a device-link code: 50 random bits.
pub const LINK_CHARS: usize = 10;
/// Characters in a staff recovery code: 80 random bits.
pub const RECOVERY_CHARS: usize = 16;

/// Which kind of code. The kind is part of the hash and of the stored row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodeKind {
    /// Made by a signed-in member to bind a new machine's key (10 minutes).
    Link,
    /// Made by staff for a member who cannot sign in (24 hours); also sets a new password.
    Recovery,
}

impl CodeKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Recovery => "recovery",
        }
    }

    /// Characters in a freshly made code.
    #[must_use]
    pub const fn chars(self) -> usize {
        match self {
            Self::Link => LINK_CHARS,
            Self::Recovery => RECOVERY_CHARS,
        }
    }

    /// How long the code lives, in seconds.
    #[must_use]
    pub const fn ttl_secs(self) -> i64 {
        match self {
            Self::Link => LINK_TTL_SECS,
            Self::Recovery => RECOVERY_TTL_SECS,
        }
    }

    const fn domain(self) -> &'static [u8] {
        match self {
            Self::Link => b"russh-bbs device link code v1\0",
            Self::Recovery => b"russh-bbs recovery code v1\0",
        }
    }
}

/// A device-link code lives 10 minutes (D97).
pub const LINK_TTL_SECS: i64 = 10 * 60;
/// A staff recovery code lives 24 hours (D97).
pub const RECOVERY_TTL_SECS: i64 = 24 * 60 * 60;

/// A freshly generated code. Shown once; never stored, logged or audited.
pub struct LoginCode {
    kind: CodeKind,
    text: String,
}

impl LoginCode {
    /// Build a code from random bytes (the caller supplies OS CSPRNG bytes). One byte makes one
    /// character; 256 is a multiple of 32, so every character is uniform. Extra bytes are
    /// ignored, and a short slice is padded with the first alphabet character (callers pass
    /// [`CodeKind::chars`] bytes).
    #[must_use]
    pub fn from_bytes(kind: CodeKind, bytes: &[u8]) -> Self {
        let mut text = String::with_capacity(kind.chars() + kind.chars() / 5);
        for index in 0..kind.chars() {
            if index > 0 && index % 5 == 0 {
                text.push('-');
            }
            let pick = bytes.get(index).copied().unwrap_or(0) & 0x1f;
            text.push(char::from(
                ALPHABET.get(usize::from(pick)).copied().unwrap_or(b'0'),
            ));
        }
        Self { kind, text }
    }

    /// The kind of code.
    #[must_use]
    pub const fn kind(&self) -> CodeKind {
        self.kind
    }

    /// The code as shown to the person. Call it only to draw the one screen that shows it.
    #[must_use]
    pub fn reveal(&self) -> &str {
        &self.text
    }

    /// The hash that is stored and that a typed code is looked up by.
    #[must_use]
    pub fn hash(&self) -> LoginCodeHash {
        LoginCodeHash::of(self.kind, &self.text)
    }
}

impl fmt::Debug for LoginCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoginCode(<redacted>)")
    }
}

/// The SHA-256 of a normalized code of one kind.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LoginCodeHash([u8; 32]);

impl LoginCodeHash {
    /// Hash typed or generated text after normalizing it: ASCII letters upper-cased, `O` read
    /// as `0` and `I`/`L` as `1`, everything that is not a letter or digit dropped, at most
    /// [`CODE_TEXT_MAX_CHARS`] input characters considered.
    #[must_use]
    pub fn of(kind: CodeKind, text: &str) -> Self {
        let normalized: String = text
            .chars()
            .take(CODE_TEXT_MAX_CHARS)
            .filter(char::is_ascii_alphanumeric)
            .map(|c| match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            })
            .collect();
        let mut hasher = Sha256::new();
        hasher.update(kind.domain());
        hasher.update(normalized.as_bytes());
        Self(hasher.finalize().into())
    }

    /// The hash as 64 lowercase hex characters, the form the database stores.
    #[must_use]
    pub fn to_hex(&self) -> String {
        use std::fmt::Write as _;
        self.0.iter().fold(String::with_capacity(64), |mut out, b| {
            // Writing to a String cannot fail.
            let _ = write!(out, "{b:02x}");
            out
        })
    }
}

impl fmt::Debug for LoginCodeHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoginCodeHash(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeKind, LoginCode, LoginCodeHash};

    #[test]
    fn a_link_code_is_ten_characters_in_two_groups() {
        let code = LoginCode::from_bytes(CodeKind::Link, &[0xAB; 10]);
        assert_eq!(code.reveal().len(), 11);
        assert_eq!(code.reveal().split('-').count(), 2);
        let long = LoginCode::from_bytes(CodeKind::Recovery, &[0x31; 16]);
        assert_eq!(long.reveal().split('-').count(), 4);
        assert_eq!(long.reveal().replace('-', "").len(), 16);
    }

    #[test]
    fn typed_input_is_forgiving_about_case_spacing_and_lookalikes() {
        let code = LoginCode::from_bytes(CodeKind::Link, &[7, 9, 11, 13, 17, 19, 23, 29, 31, 5]);
        let typed = code.reveal().to_lowercase().replace('-', " ");
        assert_eq!(LoginCodeHash::of(CodeKind::Link, &typed), code.hash());
        assert_eq!(
            LoginCodeHash::of(CodeKind::Link, "0O"),
            LoginCodeHash::of(CodeKind::Link, "00")
        );
    }

    #[test]
    fn a_code_of_one_kind_is_not_the_other_kinds() {
        let bytes = [3_u8; 16];
        let link = LoginCode::from_bytes(CodeKind::Link, &bytes);
        assert_ne!(
            link.hash(),
            LoginCodeHash::of(CodeKind::Recovery, link.reveal())
        );
    }

    #[test]
    fn different_and_truncated_codes_do_not_match() {
        let a = LoginCode::from_bytes(CodeKind::Link, &[1; 10]);
        let b = LoginCode::from_bytes(CodeKind::Link, &[2; 10]);
        assert_ne!(a.hash(), b.hash());
        let cut = &a.reveal()[..a.reveal().len() - 1];
        assert_ne!(LoginCodeHash::of(CodeKind::Link, cut), a.hash());
        assert_ne!(LoginCodeHash::of(CodeKind::Link, ""), a.hash());
    }

    #[test]
    fn debug_never_shows_the_code() {
        let code = LoginCode::from_bytes(CodeKind::Link, &[9; 10]);
        assert!(!format!("{code:?}").contains(code.reveal()));
        assert!(!format!("{:?}", code.hash()).contains(&code.hash().to_hex()));
    }
}
