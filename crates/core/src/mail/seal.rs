//! Sealing a subject or a body with XChaCha20-Poly1305 (D78, D90).
//!
//! A sealed value is `nonce (24 bytes) || ciphertext || tag (16 bytes)`. The nonce is random per
//! sealed value (192 bits, so a random nonce is safe without a counter). The **associated data**
//! binds the value to the row it was written for: a fixed label, which field it is (subject or
//! body), the message id, the recipient's account id and the key id. Move the sealed bytes to
//! another message, another mailbox, or from subject to body, and opening fails the
//! authentication check; nothing is guessed or repaired.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::TryRng as _;
use rand::rngs::SysRng;
use zeroize::Zeroizing;

use super::ids::MessageId;
use super::key::{KeyId, MailKeys};
use crate::store::AccountId;

const NONCE_BYTES: usize = 24;
const TAG_BYTES: usize = 16;
const LABEL: &[u8] = b"russh-bbs mail v1";

/// Which part of a message a sealed value is. Part of the associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The subject.
    Subject,
    /// The body.
    Body,
}

impl Field {
    const fn tag(self) -> u8 {
        match self {
            Self::Subject => 1,
            Self::Body => 2,
        }
    }
}

/// Why a value was not sealed or opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SealError {
    /// No key with the row's key id is held.
    #[error("the key that sealed this message is not available")]
    UnknownKey,
    /// The value is too short, fails authentication (damaged, moved or sealed under another
    /// key) or does not hold text.
    #[error("this message cannot be opened")]
    Unreadable,
    /// The random source failed.
    #[error("no random source")]
    Random,
}

fn aad(field: Field, id: MessageId, recipient: AccountId, key: KeyId) -> Vec<u8> {
    let mut out = Vec::with_capacity(LABEL.len() + 1 + 8 + 8 + 4);
    out.extend_from_slice(LABEL);
    out.push(field.tag());
    out.extend_from_slice(&id.as_raw().to_be_bytes());
    out.extend_from_slice(&recipient.as_raw().to_be_bytes());
    out.extend_from_slice(&key.as_raw().to_be_bytes());
    out
}

/// Seal `text` for the message `id` in `recipient`'s mailbox with the active key. Returns the
/// key id that was used and the sealed bytes.
///
/// # Errors
/// [`SealError::Random`] or [`SealError::UnknownKey`] (the active key is missing: a bug).
pub fn seal(
    keys: &MailKeys,
    field: Field,
    id: MessageId,
    recipient: AccountId,
    text: &str,
) -> Result<(KeyId, Vec<u8>), SealError> {
    let key_id = keys.active();
    let key = keys.get(key_id).ok_or(SealError::UnknownKey)?;
    let cipher = XChaCha20Poly1305::new(key.bytes().into());
    let mut nonce = [0_u8; NONCE_BYTES];
    SysRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| SealError::Random)?;
    let sealed = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: text.as_bytes(),
                aad: &aad(field, id, recipient, key_id),
            },
        )
        .map_err(|_| SealError::Unreadable)?;
    let mut out = Vec::with_capacity(NONCE_BYTES + sealed.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok((key_id, out))
}

/// Open a sealed value. The decrypted bytes are wiped once they have been copied into the
/// returned text.
///
/// # Errors
/// [`SealError::UnknownKey`] when no key with `key_id` is held; [`SealError::Unreadable`] for
/// anything else that goes wrong (the cause is deliberately not distinguished).
pub fn open(
    keys: &MailKeys,
    key_id: KeyId,
    field: Field,
    id: MessageId,
    recipient: AccountId,
    sealed: &[u8],
) -> Result<String, SealError> {
    let key = keys.get(key_id).ok_or(SealError::UnknownKey)?;
    if sealed.len() < NONCE_BYTES + TAG_BYTES {
        return Err(SealError::Unreadable);
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_BYTES);
    let cipher = XChaCha20Poly1305::new(key.bytes().into());
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad(field, id, recipient, key_id),
                },
            )
            .map_err(|_| SealError::Unreadable)?,
    );
    String::from_utf8(plain.to_vec()).map_err(|_| SealError::Unreadable)
}
