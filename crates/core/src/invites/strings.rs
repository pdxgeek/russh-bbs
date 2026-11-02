//! Every fixed string of the invite screens. They are UI chrome, so they bypass the sanitizer,
//! and they live here (not in rendering code) so a theme or translation can replace them later.
//!
//! Honesty rules (AGENTS.md): nothing here calls anything "private" or implies the server cannot
//! read it. The invite code is shown once because only a one-way hash of it is kept, and the
//! text says exactly that and no more.

/// Heading of the create-invite screen.
pub const CREATE_HEADING: &str = "Invite someone";
/// Introduces the code.
pub const CREATED_INTRO: &str = "Give this code to the person you are inviting. It works once.";
/// The code line.
pub const CODE_LINE: &str = "Invite code: {code}";
/// When the code stops working.
pub const EXPIRES: &str = "It expires {when}.";
/// Why the code cannot be shown again.
pub const SHOWN_ONCE: &str = "This is the only time it is shown. The board keeps only a one-way hash of it, so if you lose it, make another.";
/// What redeeming does to the inviter.
pub const BOUND: &str =
    "Whoever uses it joins as your invitee, and you are recorded as their inviter.";
/// Invites left in the window.
pub const REMAINING: &str = "Invites you can still make this month: {n}.";
/// The sysop has no limit.
pub const UNLIMITED: &str = "Your invites have no monthly limit.";
/// Shown after the code; closes the screen.
pub const FOOTER_CODE: &str = "Any key: done (the code is not shown again)";

/// Notice after the quota is used up.
pub const NOTICE_QUOTA: &str = "No invite was made.";
/// Body line for the quota.
pub const QUOTA_REACHED: &str =
    "You have made all {limit} of your invites for this month. The count restarts {when}.";
/// Notice when the session may not do it (a role changed since the menu was drawn).
pub const NOTICE_DENIED: &str = "You are not allowed to do that.";
/// Notice when the store failed. Nothing was changed.
pub const NOTICE_FAILED: &str = "That did not work and nothing was changed. Try again later.";

/// Heading of the list of the member's own invites.
pub const LIST_HEADING: &str = "Your invites";
/// Nothing to list.
pub const LIST_EMPTY: &str = "You have not made any invites yet.";
/// One invite.
pub const LIST_LINE: &str = "#{id}  made {made}  {status}";
/// Status of an unused invite.
pub const STATUS_OPEN: &str = "open until {when}";
/// Status of a used invite.
pub const STATUS_USED_BY: &str = "used by {handle}";
/// Status of a used invite whose account no longer exists.
pub const STATUS_USED: &str = "used";
/// Status of an unused invite past its expiry.
pub const STATUS_EXPIRED: &str = "expired {when}";
/// Reminder under the list.
pub const LIST_NOTE: &str = "Codes are shown only when you make them.";
/// Footer of the list.
pub const FOOTER_LIST: &str = "Any key: back";

/// Heading of the invite tree.
pub const TREE_HEADING: &str = "Invite tree";
/// Count line.
pub const TREE_COUNT: &str = "Who invited whom: {n} people.";
/// Nobody to show.
pub const TREE_EMPTY: &str = "There is nobody to show yet.";
/// Page position, when there is more than one page.
pub const TREE_PAGE: &str = "Page {page} of {pages}";
/// Footer of a tree with one page.
pub const FOOTER_TREE: &str = "Esc: back";
/// Footer of a tree with more than one page.
pub const FOOTER_TREE_PAGED: &str = "n: next page   p: previous page   Esc: back";
