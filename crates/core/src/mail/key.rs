//! The mail key and the key file's byte format (D78, D90).
//!
//! The key is 32 random bytes held in memory in a [`Zeroizing`] buffer (wiped on drop) and
//! never printed: `Debug` shows nothing of it. The file format is a magic line, the key's id and
//! the key, and a short checksum so a truncated or damaged file is told apart from a good one.
//! The file itself (where it lives, its 0600 mode, first-start generation and the refusal to
//! make a new key when mail exists) is the server's `mail::key_file`; this module is only bytes.
//!
//! **Rotation (not built; how it would work).** Every sealed row carries the id of the key that
//! sealed it, and [`MailKeys`] holds a set of keys with one *active* (used to seal new mail).
//! A rotation adds a key with a higher id to the file and makes it active; old keys stay in the
//! set so old rows still open. A re-seal job (the M3-C scheduler) then reads each row sealed
//! under an old id and writes it again under the active one, and once
//! [`MailStore::key_ids_in_use`](super::MailStore::key_ids_in_use) no longer lists an old id its
//! key can be removed from the file. Boot refuses to start while any stored id has no key.

use std::collections::BTreeMap;
use std::fmt;

use rand::TryRng as _;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::MailError;

/// Bytes in a key (XChaCha20-Poly1305).
pub const KEY_BYTES: usize = 32;

const MAGIC: &[u8; 10] = b"RBBSMAIL1\n";
const CHECK_BYTES: usize = 8;
/// Length of a key file's contents.
pub const FILE_BYTES: usize = MAGIC.len() + 4 + KEY_BYTES + CHECK_BYTES;

/// Which key sealed a row. Stored beside the row so a key can be replaced later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(u32);

impl KeyId {
    /// The id of the first key a fresh install makes.
    pub const FIRST: Self = Self(1);

    /// Wrap a stored id.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The id as stored.
    #[must_use]
    pub const fn as_raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One 32-byte key. No `Clone`, no `Display`, and a `Debug` that says nothing of the bytes.
pub struct MailKey(Zeroizing<[u8; KEY_BYTES]>);

impl MailKey {
    /// A new random key from the operating system.
    ///
    /// # Errors
    /// [`MailError::Random`] when the random source fails.
    pub fn generate() -> Result<Self, MailError> {
        let mut bytes = Zeroizing::new([0_u8; KEY_BYTES]);
        SysRng
            .try_fill_bytes(bytes.as_mut_slice())
            .map_err(|_| MailError::Random)?;
        Ok(Self(bytes))
    }

    /// A key from known bytes (tests, and the file decoder).
    #[must_use]
    pub fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(super) fn bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    /// The key file's contents for this key under `id`. The buffer is wiped on drop.
    #[must_use]
    pub fn to_file_bytes(&self, id: KeyId) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(FILE_BYTES));
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&id.0.to_be_bytes());
        out.extend_from_slice(self.0.as_slice());
        let check = checksum(&out);
        out.extend_from_slice(&check);
        out
    }

    /// Read a key file's contents.
    ///
    /// # Errors
    /// [`MailError::KeyFile`] for the wrong length, the wrong magic, a zero id or a checksum
    /// that does not match. The message never contains key bytes.
    pub fn from_file_bytes(bytes: &[u8]) -> Result<(KeyId, Self), MailError> {
        let corrupt = |why: &'static str| MailError::KeyFile(why);
        if bytes.len() != FILE_BYTES {
            return Err(corrupt("the key file has the wrong size"));
        }
        let (body, check) = bytes.split_at(FILE_BYTES - CHECK_BYTES);
        if !body.starts_with(MAGIC) {
            return Err(corrupt("the key file is not a mail key file"));
        }
        if !bool::from(checksum(body).as_slice().ct_eq(check)) {
            return Err(corrupt("the key file's checksum does not match"));
        }
        let rest = &body[MAGIC.len()..];
        let (id, key) = rest.split_at(4);
        let id = u32::from_be_bytes(id.try_into().map_err(|_| corrupt("bad id"))?);
        if id == 0 {
            return Err(corrupt("the key file names key 0"));
        }
        let key: [u8; KEY_BYTES] = key.try_into().map_err(|_| corrupt("bad key"))?;
        Ok((KeyId(id), Self::from_bytes(key)))
    }
}

impl fmt::Debug for MailKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailKey(..)")
    }
}

fn checksum(data: &[u8]) -> [u8; CHECK_BYTES] {
    let digest = Sha256::digest(data);
    let mut out = [0_u8; CHECK_BYTES];
    out.copy_from_slice(&digest[..CHECK_BYTES]);
    out
}

/// The keys the board can open mail with, and which one seals new mail.
pub struct MailKeys {
    active: KeyId,
    keys: BTreeMap<KeyId, MailKey>,
}

impl MailKeys {
    /// A set of one key, which is the active one.
    #[must_use]
    pub fn single(id: KeyId, key: MailKey) -> Self {
        Self {
            active: id,
            keys: BTreeMap::from([(id, key)]),
        }
    }

    /// Keep `key` under `id` so rows sealed with it still open. It is not made active.
    #[must_use]
    pub fn with_retired(mut self, id: KeyId, key: MailKey) -> Self {
        self.keys.entry(id).or_insert(key);
        self
    }

    /// The id used to seal new mail.
    #[must_use]
    pub const fn active(&self) -> KeyId {
        self.active
    }

    /// Whether a key with this id is held.
    #[must_use]
    pub fn holds(&self, id: KeyId) -> bool {
        self.keys.contains_key(&id)
    }

    pub(super) fn get(&self, id: KeyId) -> Option<&MailKey> {
        self.keys.get(&id)
    }
}

impl fmt::Debug for MailKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MailKeys {{ active: {}, held: {} }}",
            self.active,
            self.keys.len()
        )
    }
}
