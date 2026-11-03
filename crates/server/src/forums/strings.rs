//! The forum screens' words. Fixed chrome text, not member text. (A later pass can move these
//! into the string table, as the editor's are waiting to be.)

pub(super) const TITLE_BOARDS: &str = "Forums";
pub(super) const TITLE_SEARCH: &str = "Search";
pub(super) const TITLE_NEW_BOARD: &str = "New board";
pub(super) const TITLE_ALLOW: &str = "Let a member in";
pub(super) const TITLE_SUBJECT: &str = "New thread";

pub(super) const NO_BOARDS: &str = "There are no boards you can open yet.";
pub(super) const NO_THREADS: &str = "No threads yet. Write the first.";
pub(super) const NO_RESULTS: &str = "Nothing found.";
pub(super) const NOT_SEARCHED: &str = "Type at least one word to search for.";
pub(super) const GONE_MEMBER: &str = "(gone)";

pub(super) const HELP_BOARDS: &str = "Up/Down + Enter open   1-9 jump   s search   Esc back";
pub(super) const HELP_BOARDS_MANAGE: &str =
    "Up/Down + Enter open   1-9 jump   s search   n new board   Esc back";
pub(super) const HELP_THREADS: &str =
    "Up/Down + Enter read   1-9 jump   w write   n/b next/back page   s search   Esc back";
pub(super) const HELP_THREADS_MANAGE: &str = "Up/Down + Enter read   1-9 jump   w write   n/b page   s search   a let a member in   Esc back";
pub(super) const HELP_RESULTS: &str =
    "Up/Down + Enter read   1-9 jump   n/b next/back page   Esc back";
pub(super) const HELP_INPUT: &str = "Enter to continue   Esc to cancel";
pub(super) const HELP_READER_NOTE: &str = "Space next page   b page back   n/p next/previous post   r reply   c reply with quote   Esc back";
pub(super) const HELP_READER_MODERATE: &str =
    "Space next page   b back   n/p post   r reply   c quote   x remove   Esc back";
pub(super) const CONFIRM_REMOVE: &str =
    "Remove this post for everyone? y to remove, any other key to keep it.";
pub(super) const CONFIRM_REMOVE_FIRST: &str = "This is the opening post: removing it removes the whole thread. y to remove, any other key to keep it.";

pub(super) const PROMPT_SEARCH: &str = "Words to search for (every word must match):";
pub(super) const PROMPT_SUBJECT: &str = "Subject of the new thread:";
pub(super) const PROMPT_BOARD_NAME: &str = "Name of the new board:";
pub(super) const PROMPT_BOARD_DESCRIPTION: &str = "What is it for? (optional)";
pub(super) const PROMPT_BOARD_RESTRICTED: &str =
    "Only members you let in can see it? y = restricted, n = open to every member";
pub(super) const PROMPT_ALLOW: &str = "Handle of the member to let into this board:";

pub(super) const OK_POSTED: &str = "Posted.";
pub(super) const OK_REMOVED: &str = "Removed.";
pub(super) const OK_REMOVED_THREAD: &str = "Removed, with its thread.";
pub(super) const OK_DRAFT: &str = "Draft saved.";
pub(super) const OK_BOARD: &str = "Board created.";
pub(super) const OK_ALLOWED: &str = "They can now use this board.";
pub(super) const KEPT_DRAFT: &str = "Your text was kept as a draft; write again to continue it.";
pub(super) const FAILED: &str = "That did not work. Try again in a moment.";
pub(super) const GONE: &str = "That is not available.";
pub(super) const DENIED: &str = "You may not do that.";
pub(super) const NOT_SIGNED_IN: &str = "You are not signed in.";
pub(super) const NAME_TAKEN: &str = "A board with that name already exists.";
pub(super) const UNKNOWN_MEMBER: &str = "No active member has that handle.";
pub(super) const ROLLING_HOUR: &str = "Posts are counted over a rolling hour.";
