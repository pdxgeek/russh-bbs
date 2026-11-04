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
pub(super) const TITLE_FEED: &str = "Add a feed";
pub(super) const TITLE_FEED_REMOVE: &str = "Remove the feed";
pub(super) const PROMPT_FEED_URL: &str = "Address of the RSS or Atom feed (https://...):";
pub(super) const PROMPT_FEED_NAME: &str = "Name of the board that will show it:";
pub(super) const PROMPT_FEED_HTTP: &str =
    "Allow plain http for this feed? y = http is allowed, n = https only";
pub(super) const PROMPT_FEED_REMOVE: &str =
    "Remove this feed and its board with every item on it? y to remove, n to keep it";
pub(super) const OK_FEED_ADDED: &str =
    "Feed added. It is read on the next scheduled run; items appear on the new board.";
pub(super) const OK_FEED_REMOVED: &str = "Feed removed.";
pub(super) const OK_PUBLIC: &str =
    "This board can now be read as RSS by anyone who has its address.";
pub(super) const OK_NOT_PUBLIC: &str = "This board is no longer served as RSS.";
pub(super) const READ_ONLY: &str = "This board shows a feed: it is read only.";
pub(super) const FEED_URL_BAD: &str = "That address cannot be used.";
pub(super) const FEED_TAKEN: &str = "That address is already subscribed.";
pub(super) const FEED_LIMIT: &str = "The limit of feeds is reached.";
pub(super) const FEED_RESTRICTED: &str = "A restricted board cannot be public.";
pub(super) const FEED_BOARD_TAG: &str = "feed";
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
