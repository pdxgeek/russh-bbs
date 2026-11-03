//! The storage trait for the community features: persistence and the writes that must be
//! atomic. Who may call it, and what a result means to a person, is in [`super::service`]; the
//! trait takes no permission because only the service functions (which `require` one) are meant
//! to be called by a front end.

use std::future::Future;

use crate::store::{AccountId, StoreError, UnixSeconds};

use super::types::{EntryId, LineKind, LineText, NewPoll, PollId, ResultsVisibility};

/// A line as the store holds it. The service sanitizes `author` and `text` again when it reads
/// them, so a row edited by hand cannot reach a screen unchecked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLine {
    /// The row id.
    pub id: EntryId,
    /// The author's handle.
    pub author: String,
    /// The text.
    pub text: String,
    /// When it was added.
    pub at: UnixSeconds,
}

/// What [`CommunityStore::add_line`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddLine {
    /// Stored. Lines past the list's size were deleted in the same transaction.
    Added(EntryId),
    /// The member already added the hourly limit; nothing was written. The time is when their
    /// oldest counted line leaves the hour.
    RateLimited {
        /// When the member may add again.
        retry_at: UnixSeconds,
    },
}

/// What [`CommunityStore::remove_line`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Removed, and the audit row written in the same transaction.
    Removed,
    /// No such entry (or it is a different kind of list).
    NotFound,
}

/// A member in last callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCaller {
    /// The handle.
    pub handle: String,
    /// When their latest session began.
    pub at: UnixSeconds,
}

/// A row of the poll list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPollRow {
    /// The poll.
    pub id: PollId,
    /// The question.
    pub question: String,
    /// Whether it is closed.
    pub closed: bool,
    /// Votes cast so far.
    pub votes: u64,
}

/// A poll as the store holds it, with the viewer's own vote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPoll {
    /// The poll.
    pub id: PollId,
    /// The question.
    pub question: String,
    /// The description, empty when none.
    pub description: String,
    /// Who made it.
    pub creator: AccountId,
    /// The creator's handle.
    pub creator_handle: String,
    /// When counts show (the stored name).
    pub visibility: String,
    /// When it was closed, if it was.
    pub closed_at: Option<UnixSeconds>,
    /// When it was made.
    pub created_at: UnixSeconds,
    /// The options in voting order.
    pub options: Vec<String>,
    /// The option (zero-based) the viewer voted for.
    pub my_vote: Option<usize>,
}

/// The bounds a poll creation is checked against, inside the transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollRules {
    /// Polls kept in all.
    pub max_polls: usize,
    /// Polls one member may make in a rolling day.
    pub per_day: usize,
}

/// What [`CommunityStore::create_poll`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatePoll {
    /// Stored.
    Created(PollId),
    /// The member made the day's limit already.
    RateLimited {
        /// When the member may make another.
        retry_at: UnixSeconds,
    },
    /// The board holds as many polls as it keeps; staff must remove one.
    BoardFull,
}

/// What [`CommunityStore::vote`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteOutcome {
    /// The vote is counted.
    Voted,
    /// The member voted on this poll already; nothing changed. The store's unique key decides.
    AlreadyVoted,
    /// The poll is closed.
    Closed,
    /// No such poll, or no such option on it.
    NotFound,
}

/// What [`CommunityStore::close_poll`] and [`CommunityStore::remove_poll`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// Done (and audited when staff acted on someone else's poll).
    Done,
    /// The poll was closed already.
    AlreadyClosed,
    /// The actor is neither the creator nor staff.
    NotAllowed,
    /// No such poll.
    NotFound,
}

/// The community features' rows.
pub trait CommunityStore: Send + Sync {
    /// Add a line. Atomically: count the member's lines of this kind in the last hour and
    /// refuse at `per_hour`; insert; delete the oldest lines past `keep`.
    ///
    /// Fails with [`StoreError::AccountNotFound`] for an id with no account.
    fn add_line(
        &self,
        account: AccountId,
        kind: LineKind,
        text: &LineText,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<AddLine, StoreError>> + Send;

    /// The newest lines of `kind`, newest first, at most `limit`.
    fn recent_lines(
        &self,
        kind: LineKind,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<StoredLine>, StoreError>> + Send;

    /// Delete one line. The audit row `community.entry_removed` (actor `actor`, target the
    /// author) is written in the same transaction.
    fn remove_line(
        &self,
        actor: AccountId,
        kind: LineKind,
        id: EntryId,
    ) -> impl Future<Output = Result<RemoveOutcome, StoreError>> + Send;

    /// Record that `account` began a session. One row per member, updated in place.
    fn record_sign_in(
        &self,
        account: AccountId,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The most recent sign-ins of **active** accounts, newest first, at most `limit`.
    fn last_callers(
        &self,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<StoredCaller>, StoreError>> + Send;

    /// Store a poll. Atomically: refuse at `rules.per_day` for the creator in the last day and
    /// at `rules.max_polls` in all.
    fn create_poll(
        &self,
        creator: AccountId,
        poll: &NewPoll,
        rules: PollRules,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<CreatePoll, StoreError>> + Send;

    /// A page of polls, newest first, with the total.
    fn list_polls(
        &self,
        offset: usize,
        limit: usize,
    ) -> impl Future<Output = Result<(Vec<StoredPollRow>, usize), StoreError>> + Send;

    /// One poll with `viewer`'s own vote.
    fn poll(
        &self,
        id: PollId,
        viewer: AccountId,
    ) -> impl Future<Output = Result<Option<StoredPoll>, StoreError>> + Send;

    /// Votes per option (index = option), zero for an option nobody chose.
    fn tally(&self, id: PollId) -> impl Future<Output = Result<Vec<u64>, StoreError>> + Send;

    /// Count a vote for option `option` (zero-based). One vote per member per poll is a unique
    /// key in the table, so two votes at once cannot both count.
    fn vote(
        &self,
        id: PollId,
        account: AccountId,
        option: usize,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<VoteOutcome, StoreError>> + Send;

    /// Close a poll. Allowed for its creator, and for anyone when `staff` is true; a staff
    /// close of someone else's poll writes `community.poll_closed` in the same transaction.
    fn close_poll(
        &self,
        actor: AccountId,
        id: PollId,
        staff: bool,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<CloseOutcome, StoreError>> + Send;

    /// Delete a poll and its votes (staff only; the caller checked). Writes
    /// `community.poll_removed` in the same transaction.
    fn remove_poll(
        &self,
        actor: AccountId,
        id: PollId,
    ) -> impl Future<Output = Result<CloseOutcome, StoreError>> + Send;
}

/// Convenience for stores: the visibility a stored name means, or [`StoreError::Corrupt`].
///
/// # Errors
/// [`StoreError::Corrupt`] for a name the code could not have written.
pub fn visibility_of(name: &str) -> Result<ResultsVisibility, StoreError> {
    ResultsVisibility::parse(name)
        .ok_or_else(|| StoreError::Corrupt("a stored poll has an unknown results setting".into()))
}

/// The board with no members (tests, and a front end started without a database): nothing to
/// read, and every write names no account.
impl CommunityStore for crate::access::NoMembers {
    fn add_line(
        &self,
        _account: AccountId,
        _kind: LineKind,
        _text: &LineText,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<AddLine, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn recent_lines(
        &self,
        _kind: LineKind,
        _limit: usize,
    ) -> impl Future<Output = Result<Vec<StoredLine>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn remove_line(
        &self,
        _actor: AccountId,
        _kind: LineKind,
        _id: EntryId,
    ) -> impl Future<Output = Result<RemoveOutcome, StoreError>> + Send {
        std::future::ready(Ok(RemoveOutcome::NotFound))
    }

    fn record_sign_in(
        &self,
        _account: AccountId,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn last_callers(
        &self,
        _limit: usize,
    ) -> impl Future<Output = Result<Vec<StoredCaller>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn create_poll(
        &self,
        _creator: AccountId,
        _poll: &NewPoll,
        _rules: PollRules,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<CreatePoll, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn list_polls(
        &self,
        _offset: usize,
        _limit: usize,
    ) -> impl Future<Output = Result<(Vec<StoredPollRow>, usize), StoreError>> + Send {
        std::future::ready(Ok((Vec::new(), 0)))
    }

    fn poll(
        &self,
        _id: PollId,
        _viewer: AccountId,
    ) -> impl Future<Output = Result<Option<StoredPoll>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn tally(&self, _id: PollId) -> impl Future<Output = Result<Vec<u64>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn vote(
        &self,
        _id: PollId,
        _account: AccountId,
        _option: usize,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<VoteOutcome, StoreError>> + Send {
        std::future::ready(Ok(VoteOutcome::NotFound))
    }

    fn close_poll(
        &self,
        _actor: AccountId,
        _id: PollId,
        _staff: bool,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<CloseOutcome, StoreError>> + Send {
        std::future::ready(Ok(CloseOutcome::NotFound))
    }

    fn remove_poll(
        &self,
        _actor: AccountId,
        _id: PollId,
    ) -> impl Future<Output = Result<CloseOutcome, StoreError>> + Send {
        std::future::ready(Ok(CloseOutcome::NotFound))
    }
}
