//! What a request is, and the numbers that bound it.

use std::fmt;

use sha2::{Digest as _, Sha256};

use crate::access::Fingerprint;
use crate::passwords::Secret;
use crate::sanitize::{Sanitized, SingleLine, Username};
use crate::store::{AccountId, UnixSeconds};

/// Longest request message, in characters. The same cap as the waiting room's free-text field.
pub const MESSAGE_MAX_CHARS: usize = 280;

/// How long a request waits for a decision: 14 days. After that it is deleted, as if it had
/// never been made, and the person may ask again. Fixed; not a setting.
pub const PENDING_TTL_SECS: i64 = 14 * 86_400;

/// How long a denied key is turned away without being told: 24 hours. While it runs, a new
/// request from the key is dropped (with the same answer as any other), so a refusal cannot
/// be undone by pressing the key again. After it the cooldown row is deleted and nothing
/// remains. Fixed; not a setting.
pub const DENIAL_COOLDOWN_SECS: i64 = 86_400;

/// How many requests may wait at once unless the sysop sets `queue_max_pending`: 200. When the
/// queue is full new requests are dropped with the usual answer and a log line.
pub const DEFAULT_MAX_PENDING: u32 = 200;

/// The largest `queue_max_pending` that is honoured. A larger stored value is read as the
/// default, so a typo cannot turn the cap off.
pub const MAX_MAX_PENDING: u32 = 10_000;

/// What a queued request asks for. Both kinds share the queue's table, caps, rate limits,
/// expiry, screens and decisions (M2-G, D73); the kind is what the sysop reads on the detail
/// screen and nothing else treats them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RequestKind {
    /// A stranger asked for an invite in `invite` mode (M2-E).
    #[default]
    Invite,
    /// A stranger applied for membership in `apply` mode, or in `open` mode on a day the cap
    /// was used up (M2-G).
    Application,
}

impl RequestKind {
    /// The text stored in the `kind` column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invite => "invite",
            Self::Application => "application",
        }
    }

    /// Parse a stored value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "invite" => Some(Self::Invite),
            "application" => Some(Self::Application),
            _ => None,
        }
    }
}

/// Store-assigned id of a request. What the audit log and the screens name; never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(i64);

impl RequestId {
    /// Wrap a raw id. Call this only where a row is mapped (the store).
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for the store to bind into a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A request's message: one line, sanitized, at most [`MESSAGE_MAX_CHARS`] characters. The only
/// constructor sanitizes, so it is content that may be drawn but never chrome.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Message(Sanitized);

impl Message {
    /// Sanitize `raw` as one line and cap it.
    #[must_use]
    pub fn new(raw: &str) -> Self {
        Self(SingleLine::new(MESSAGE_MAX_CHARS).sanitize(raw))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A request's text is not logged.
        f.debug_tuple("Message").field(&self.0.len()).finish()
    }
}

/// What the waiting room's form hands over: a handle that passed `sanitize_username` and the
/// message as typed (the room already sanitized it; [`Message::new`] does it again).
#[derive(Clone, PartialEq, Eq)]
pub struct InviteAsk {
    /// The handle the person would like.
    pub handle: Username,
    /// The message, as the room holds it.
    pub message: String,
    /// Which form it came from.
    pub kind: RequestKind,
    /// The password the person chose, over telnet (D94); `None` for a key request. The front
    /// end hashes it before anything is stored.
    pub password: Option<Secret>,
}

impl fmt::Debug for InviteAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InviteAsk").finish_non_exhaustive()
    }
}

/// A request to record: everything stored about it, and nothing else.
#[derive(Clone, PartialEq, Eq)]
pub struct NewRequest {
    /// The handle asked for.
    pub handle: Username,
    /// The message.
    pub message: Message,
    /// The presenting key's public fingerprint: the key the account will be bound to.
    pub fingerprint: Fingerprint,
    /// Which form it came from.
    pub kind: RequestKind,
    /// For a person with no SSH key (telnet, D94): the argon2id PHC hash of the password they
    /// chose on the form. Approval binds the new account to it instead of to a key, and
    /// `fingerprint` is then the stand-in [`password_identity`]. `None` for a key request.
    pub password_hash: Option<String>,
}

impl fmt::Debug for NewRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewRequest")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

/// A member who has spoken for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voucher {
    /// Who.
    pub account: AccountId,
    /// Their handle.
    pub handle: Username,
}

/// A request waiting for a decision, as the staff screens list it. The fingerprint is
/// deliberately not here: no screen needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRequest {
    /// Its id.
    pub id: RequestId,
    /// The handle asked for.
    pub handle: Username,
    /// The message.
    pub message: Message,
    /// When it was made.
    pub created_at: UnixSeconds,
    /// When it is deleted if nobody decides.
    pub expires_at: UnixSeconds,
    /// Active members who have vouched, oldest vouch first.
    pub vouchers: Vec<Voucher>,
    /// An invite request or a membership application.
    pub kind: RequestKind,
}

/// The stand-in key identity of a request made without an SSH key (telnet, D94), so the queue's
/// one-request-per-identity rule, its rate limits and the denial cooldown work as they do for a
/// key. It is SHA-256 over a domain prefix and the handle's skeleton, written as a well-formed
/// `SHA256:` fingerprint, so a lookalike handle shares it. Nothing can be bound to it: no key
/// hashes to it, and the row that holds it also holds a password hash, which approval uses.
///
/// # Panics
/// Never: the text it builds is always a well-formed fingerprint (a test checks it).
#[must_use]
pub fn password_identity(handle: &Username) -> Fingerprint {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut hasher = Sha256::new();
    hasher.update(b"russh-bbs password request identity v1\0");
    hasher.update(handle.skeleton().as_bytes());
    let digest = hasher.finalize();
    // 32 bytes as unpadded base64: 43 characters, the shape of an OpenSSH SHA-256 fingerprint.
    let mut text = String::from("SHA256:");
    for chunk in digest.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let word = (b0 << 16) | (b1 << 8) | b2;
        let digits = match chunk.len() {
            1 => 2,
            2 => 3,
            _ => 4,
        };
        for index in 0..digits {
            let shift = 18 - 6 * index;
            let sextet = usize::try_from((word >> shift) & 0x3f).unwrap_or(0);
            text.push(char::from(ALPHABET.get(sextet).copied().unwrap_or(b'A')));
        }
    }
    Fingerprint::parse(&text).unwrap_or_else(|_| unreachable!("a 43-character base64 body"))
}

/// The stored form of a denied key: SHA-256 over a domain prefix and the fingerprint text, as
/// 64 lowercase hex characters. It lets the store recognise the key for
/// [`DENIAL_COOLDOWN_SECS`] without keeping the fingerprint, and it cannot be matched against
/// the audit log (which never holds a denied key). Both backends use this one function.
#[must_use]
pub fn cooldown_key(fingerprint: &Fingerprint) -> String {
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(b"russh-bbs request cooldown v1\0");
    hasher.update(fingerprint.as_str().as_bytes());
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            // Writing to a String cannot fail.
            let _ = write!(out, "{byte:02x}");
            out
        })
}
