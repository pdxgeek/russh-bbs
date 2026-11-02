//! The invite code and its hash (M2-D, D67).
//!
//! A code is [`CODE_BYTES`] random bytes (160 bits, above the 128-bit floor) from the OS CSPRNG,
//! written in Crockford base32 as eight groups of four characters, for example
//! `7QX9-M2KD-...`. Crockford's alphabet has no `I`, `L`, `O` or `U`, so a code read aloud or
//! copied by eye is hard to get wrong, and typed input is forgiving: case, dashes and spaces are
//! ignored and `O`, `I` and `L` read as `0`, `1` and `1`.
//!
//! **Only the hash is stored.** The plaintext exists in the inviter's session for the one screen
//! that shows it. The hash is SHA-256 over a fixed domain prefix and the normalized code. As with
//! the founder token (D41) a slow hash would protect nothing: the secret has 160 random bits, so
//! it cannot be searched, and a slow hash would only make every guess against the redeem form
//! cost the server more. The store finds a code by looking its hash up (an indexed equality), so
//! nothing compares a secret byte by byte; the comparison helper [`CodeHash::ct_eq`] is constant
//! time for the places that do hold two hashes.
//!
//! Randomness is passed in, like the founder token, so this module is deterministic to test.

use std::fmt;

use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

/// Random bytes in a code: 160 bits.
pub const CODE_BYTES: usize = 20;

/// Most characters of typed input that are looked at. A code is 32 characters plus seven
/// dashes; anything longer cannot match and is not hashed in full.
pub const CODE_TEXT_MAX_CHARS: usize = 64;

/// Crockford base32 (no padding): digits and letters without `I`, `L`, `O`, `U`.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Domain separation for the hash.
const DOMAIN: &[u8] = b"russh-bbs invite code v1\0";

/// A freshly generated invite code. Shown once to the inviter; never stored, logged or audited.
pub struct InviteCode {
    text: String,
}

impl InviteCode {
    /// Build the code from random bytes (the caller supplies OS CSPRNG bytes).
    #[must_use]
    pub fn from_bytes(bytes: [u8; CODE_BYTES]) -> Self {
        let mut chars = String::with_capacity(32);
        let mut buffer: u32 = 0;
        let mut bits = 0_u32;
        for byte in bytes {
            buffer = (buffer << 8) | u32::from(byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                let index = usize::try_from((buffer >> bits) & 0x1f).unwrap_or(0);
                chars.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'0')));
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

    /// The code as shown to the inviter. The only way to read it; call it only to draw the one
    /// screen that shows the code.
    #[must_use]
    pub fn reveal(&self) -> &str {
        &self.text
    }

    /// The hash that is stored and that a typed code is looked up by.
    #[must_use]
    pub fn hash(&self) -> CodeHash {
        CodeHash::of(&self.text)
    }
}

impl fmt::Debug for InviteCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InviteCode(<redacted>)")
    }
}

/// The SHA-256 of a normalized code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CodeHash([u8; 32]);

impl CodeHash {
    /// Hash typed or generated code text after normalizing it: ASCII letters upper-cased,
    /// `O` read as `0` and `I`/`L` as `1`, everything that is not a letter or digit dropped, at
    /// most [`CODE_TEXT_MAX_CHARS`] input characters considered.
    #[must_use]
    pub fn of(text: &str) -> Self {
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
        hasher.update(DOMAIN);
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

    /// Constant-time equality.
    #[must_use]
    pub fn ct_eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl fmt::Debug for CodeHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CodeHash(..)")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_32_crockford_characters_in_eight_groups() {
        let code = InviteCode::from_bytes([0xAB; CODE_BYTES]);
        let text = code.reveal();
        assert_eq!(text.len(), 32 + 7);
        let groups: Vec<&str> = text.split('-').collect();
        assert_eq!(groups.len(), 8);
        assert!(groups.iter().all(|g| g.len() == 4));
        assert!(
            text.chars()
                .filter(|c| *c != '-')
                .all(|c| ALPHABET.contains(&(c as u8)))
        );
    }

    #[test]
    fn the_encoding_carries_all_160_bits() {
        // Flipping any single bit of the input changes the code: nothing is dropped.
        let base = InviteCode::from_bytes([0; CODE_BYTES]).reveal().to_owned();
        for byte in 0..CODE_BYTES {
            for bit in 0..8 {
                let mut bytes = [0_u8; CODE_BYTES];
                bytes[byte] = 1 << bit;
                assert_ne!(InviteCode::from_bytes(bytes).reveal(), base, "{byte}:{bit}");
            }
        }
    }

    #[test]
    fn typed_input_is_forgiving_about_case_spacing_and_lookalikes() {
        let code = InviteCode::from_bytes([7; CODE_BYTES]);
        let typed = code.reveal().to_lowercase().replace('-', " ");
        assert!(CodeHash::of(&typed).ct_eq(&code.hash()));
        assert!(CodeHash::of(&format!("  {}  ", code.reveal())).ct_eq(&code.hash()));
        // O/I/L are not in the alphabet, so reading them as 0/1/1 never merges two codes.
        assert!(CodeHash::of("0O").ct_eq(&CodeHash::of("00")));
        assert!(CodeHash::of("1IL").ct_eq(&CodeHash::of("111")));
    }

    #[test]
    fn different_codes_and_truncated_codes_do_not_match() {
        let a = InviteCode::from_bytes([1; CODE_BYTES]);
        let b = InviteCode::from_bytes([2; CODE_BYTES]);
        assert!(!a.hash().ct_eq(&b.hash()));
        let cut = &a.reveal()[..a.reveal().len() - 1];
        assert!(!CodeHash::of(cut).ct_eq(&a.hash()));
        assert!(!CodeHash::of("").ct_eq(&a.hash()));
    }

    #[test]
    fn only_the_first_64_characters_are_hashed() {
        let code = InviteCode::from_bytes([3; CODE_BYTES]);
        let padded = format!("{}{}", code.reveal(), "x".repeat(1000));
        // 39 characters of code, then padding: the padding changes the hash (it is inside the
        // first 64), but a megabyte of it costs no more than 64 characters.
        assert!(!CodeHash::of(&padded).ct_eq(&code.hash()));
        let long = "A".repeat(1_000_000);
        assert!(CodeHash::of(&long).ct_eq(&CodeHash::of(&"A".repeat(CODE_TEXT_MAX_CHARS))));
    }

    #[test]
    fn the_hash_is_64_hex_characters_and_nothing_prints_the_secret() {
        let code = InviteCode::from_bytes([9; CODE_BYTES]);
        let hex = code.hash().to_hex();
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert!(!hex.contains(code.reveal()));
        let shown = format!("{code:?} {:?}", code.hash());
        assert!(!shown.contains(code.reveal()));
        assert!(!shown.contains(&hex));
    }
}
