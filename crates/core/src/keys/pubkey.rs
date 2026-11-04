//! Reading a pasted public key (FIX-M3-11, D21, D97): the `ssh-ed25519 AAAA... comment` line
//! that `ssh-keygen` writes to a `.pub` file, and nothing else.
//!
//! Only Ed25519 is accepted for a pasted key, so the parser is small enough to write and test
//! by hand: the blob is base64 of `string "ssh-ed25519"`, `string key(32 bytes)` and nothing
//! after. The fingerprint is the OpenSSH one, `SHA256:` plus the unpadded base64 of the SHA-256
//! of the blob, and a test compares it with the value `ssh-keygen -l` prints. What was pasted is
//! never echoed back, logged or stored: only the fingerprint survives.

use sha2::{Digest as _, Sha256};

use crate::access::Fingerprint;

const KEY_TYPE: &str = "ssh-ed25519";
const KEY_BYTES: usize = 32;
/// Longest paste that is looked at. A real line is about 100 characters; a comment can be long.
pub const PASTE_MAX_CHARS: usize = 600;

/// Why a paste is not an Ed25519 public key. The text never names the paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PubKeyError {
    /// Not one line of `ssh-ed25519 <base64> [comment]` holding a well-formed key.
    #[error("not an Ed25519 public key")]
    NotEd25519,
}

/// Parse a pasted public key and return its fingerprint.
///
/// # Errors
/// [`PubKeyError::NotEd25519`] for anything but one well-formed Ed25519 public key line,
/// including private keys, other key types, options before the type, and several lines.
pub fn parse_ed25519(text: &str) -> Result<Fingerprint, PubKeyError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > PASTE_MAX_CHARS || text.contains(['\n', '\r']) {
        return Err(PubKeyError::NotEd25519);
    }
    let mut parts = text.split_whitespace();
    let (Some(kind), Some(body)) = (parts.next(), parts.next()) else {
        return Err(PubKeyError::NotEd25519);
    };
    if kind != KEY_TYPE {
        return Err(PubKeyError::NotEd25519);
    }
    let blob = decode_base64(body).ok_or(PubKeyError::NotEd25519)?;
    check_blob(&blob)?;
    let digest = Sha256::digest(&blob);
    let fingerprint = format!("SHA256:{}", encode_base64_unpadded(&digest));
    Fingerprint::parse(&fingerprint).map_err(|_| PubKeyError::NotEd25519)
}

/// The blob is exactly `string "ssh-ed25519"` then `string key`, 32 bytes, with nothing after.
fn check_blob(blob: &[u8]) -> Result<(), PubKeyError> {
    let (name, rest) = read_string(blob).ok_or(PubKeyError::NotEd25519)?;
    let (key, rest) = read_string(rest).ok_or(PubKeyError::NotEd25519)?;
    if name == KEY_TYPE.as_bytes() && key.len() == KEY_BYTES && rest.is_empty() {
        Ok(())
    } else {
        Err(PubKeyError::NotEd25519)
    }
}

/// An SSH `string`: a big-endian `u32` length, then that many bytes.
fn read_string(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let (len, rest) = bytes.split_first_chunk::<4>()?;
    let len = usize::try_from(u32::from_be_bytes(*len)).ok()?;
    (rest.len() >= len).then(|| rest.split_at(len))
}

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Strict base64 (padded, standard alphabet). `None` for anything else.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunks = bytes.len() / 4;
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == chunks;
        let mut value: u32 = 0;
        let mut pad = 0;
        for &byte in chunk {
            value <<= 6;
            if byte == b'=' {
                if !last {
                    return None;
                }
                pad += 1;
            } else if pad > 0 {
                // Data after padding.
                return None;
            } else {
                let sextet = STANDARD.iter().position(|&c| c == byte)?;
                value |= u32::try_from(sextet).ok()?;
            }
        }
        if pad > 2 {
            return None;
        }
        let triple = value.to_be_bytes();
        out.extend_from_slice(triple.get(1..4 - pad)?);
    }
    Some(out)
}

fn encode_base64_unpadded(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut buffer = [0_u8; 3];
        buffer
            .get_mut(..chunk.len())
            .map(|dst| dst.copy_from_slice(chunk));
        let value = (u32::from(buffer[0]) << 16) | (u32::from(buffer[1]) << 8) | u32::from(buffer[2]);
        let symbols = chunk.len() + 1;
        for position in 0..symbols {
            let shift = 18 - 6 * u32::try_from(position).unwrap_or(0);
            let index = usize::try_from((value >> shift) & 0x3f).unwrap_or(0);
            out.push(char::from(STANDARD.get(index).copied().unwrap_or(b'A')));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{PubKeyError, decode_base64, encode_base64_unpadded, parse_ed25519};

    /// The all-zero Ed25519 key: `ssh-keygen -l -f` prints the fingerprint below for this line
    /// (computed with OpenSSH 9.6 `ssh-keygen -lf` when the test was written).
    const ZERO_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ZERO_KEY_FINGERPRINT: &str = "SHA256:kmYcvdi2GkPeWxB6XLjrZB8JHsy2Hm8luHMFp9GMvqk";

    #[test]
    fn a_public_key_line_gives_the_openssh_fingerprint() {
        let found = parse_ed25519(ZERO_KEY).unwrap();
        assert_eq!(found.as_str(), ZERO_KEY_FINGERPRINT);
        let with_comment = parse_ed25519(&format!("  {ZERO_KEY} me@laptop \n")).unwrap();
        assert_eq!(with_comment, found);
    }

    #[test]
    fn anything_else_is_refused() {
        let bad = [
            "",
            "   ",
            "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC",
            "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY=",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "ssh-ed25519",
            "ssh-ed25519 not-base64!",
            // The right type name in the line but a blob of another type.
            "ssh-ed25519 AAAAB3NzaC1yc2EAAAADAQABAAABAQC=",
            // One byte too few and too many in the key.
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAHwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "options ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ];
        for text in bad {
            assert_eq!(parse_ed25519(text), Err(PubKeyError::NotEd25519), "{text:?}");
        }
        // Two lines are two keys: refused rather than guessed at.
        assert!(parse_ed25519(&format!("{ZERO_KEY}\n{ZERO_KEY}")).is_err());
        // Trailing bytes after the key.
        let trailing = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
        assert!(parse_ed25519(trailing).is_err());
        assert!(parse_ed25519(&"a".repeat(10_000)).is_err());
    }

    #[test]
    fn base64_round_trips_and_is_strict() {
        for len in 0..40_u8 {
            let data: Vec<u8> = (0..len).map(|i| i.wrapping_mul(37).wrapping_add(11)).collect();
            let unpadded = encode_base64_unpadded(&data);
            let padded = format!("{unpadded}{}", "=".repeat((4 - unpadded.len() % 4) % 4));
            if data.is_empty() {
                assert_eq!(decode_base64(&padded), None);
            } else {
                assert_eq!(decode_base64(&padded).unwrap(), data);
            }
        }
        assert_eq!(decode_base64("AA=A"), None);
        assert_eq!(decode_base64("A==="), None);
        assert_eq!(decode_base64("AAA"), None);
        assert_eq!(decode_base64("AA\nA"), None);
    }
}
