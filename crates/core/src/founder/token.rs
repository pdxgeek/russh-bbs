//! The setup token and its hash.
//!
//! The token is [`TOKEN_BYTES`] random bytes (160 bits) shown as 32 base32 characters in groups
//! of four. Input is normalized (case, dashes and spaces ignored) before hashing so a person
//! typing it from a terminal is not defeated by formatting.
//!
//! **Why SHA-256 and not a slow hash.** A slow hash (argon2, bcrypt) protects low-entropy
//! secrets. This secret has 160 random bits, so brute force is impossible with any hash and a
//! slow one would only add cost to every guess against the server. The hash exists so the
//! token is never held in plaintext by anything that outlives the console print. A fixed
//! domain-separation prefix keeps the hash from being reused for anything else (D41).

use std::fmt;

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

/// Random bytes in a token: 160 bits, above the 128-bit floor.
pub const TOKEN_BYTES: usize = 20;

/// Most characters of typed input that are looked at. A token is 32 characters plus seven
/// dashes; anything longer cannot match and is not hashed in full.
pub const TOKEN_TEXT_MAX_CHARS: usize = 64;

/// RFC 4648 base32 alphabet (no padding).
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Domain separation for the hash.
const DOMAIN: &[u8] = b"russh-bbs founder setup token v1\0";

/// A freshly generated setup token. Shown once on the console and never stored.
pub struct SetupToken {
    text: String,
}

impl SetupToken {
    /// Build the token from random bytes. The caller supplies OS CSPRNG bytes; keeping the
    /// randomness outside makes the rest of this module deterministic to test.
    #[must_use]
    pub fn from_bytes(bytes: [u8; TOKEN_BYTES]) -> Self {
        let mut chars = String::with_capacity(32);
        let mut buffer: u32 = 0;
        let mut bits = 0_u32;
        for byte in bytes {
            buffer = (buffer << 8) | u32::from(byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                let index = usize::try_from((buffer >> bits) & 0x1f).unwrap_or(0);
                chars.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
            }
            buffer &= (1 << bits) - 1;
        }
        let mut text = String::with_capacity(chars.len() + chars.len() / 4);
        for (i, c) in chars.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                text.push('-');
            }
            text.push(c);
        }
        Self { text }
    }

    /// The token as shown to the sysop. This is the only way to read it; call it only to
    /// print the one console announcement.
    #[must_use]
    pub fn reveal(&self) -> &str {
        &self.text
    }

    /// The hash that a typed token is compared against.
    #[must_use]
    pub fn hash(&self) -> TokenHash {
        TokenHash::of(&self.text)
    }
}

impl fmt::Debug for SetupToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SetupToken(<redacted>)")
    }
}

/// The SHA-256 of a normalized token.
#[derive(Clone, Copy)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    /// Hash typed or generated token text after normalizing it: ASCII letters upper-cased,
    /// everything that is not a letter or digit dropped, at most [`TOKEN_TEXT_MAX_CHARS`]
    /// input characters considered.
    #[must_use]
    pub fn of(text: &str) -> Self {
        let normalized: String = text
            .chars()
            .take(TOKEN_TEXT_MAX_CHARS)
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        hasher.update(normalized.as_bytes());
        Self(hasher.finalize().into())
    }

    /// Constant-time equality, returned as a [`subtle::Choice`] so a caller can combine it
    /// with other conditions without branching on secret data.
    #[must_use]
    pub fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.0.ct_eq(&other.0)
    }

    /// A hash no typed text maps to: [`Self::of`] keeps only ASCII letters and digits, so it
    /// can never hash the NUL byte this one is built from.
    #[must_use]
    pub(super) fn unmatchable() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        hasher.update(b"\0unmatchable");
        Self(hasher.finalize().into())
    }
}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenHash(..)")
    }
}
