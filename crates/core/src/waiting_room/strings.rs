//! Every fixed string the waiting room shows. They are UI chrome, so they bypass the
//! sanitizer, and they live here (not in rendering code) so a theme or translation can replace
//! them later (`docs/ui.md`).
//!
//! None of these names a member, a board or a statistic, and none differs between a member's
//! key and a stranger's: the waiting room reveals nothing (threat model, "Waiting room").

/// Shown on the first line of every page.
pub const TITLE: &str = "russh-bbs";
/// The home page's heading.
pub const HOME_HEADING: &str = "Waiting room";
/// The rules page's heading.
pub const RULES_HEADING: &str = "House rules";
/// Heading of the invite-request form.
pub const REQUEST_INVITE_HEADING: &str = "Request an invite";
/// Heading of the invite-redeem form.
pub const REDEEM_INVITE_HEADING: &str = "Redeem an invite";
/// Heading of the application form.
pub const APPLY_HEADING: &str = "Apply for membership";
/// Heading of the create-account form.
pub const CREATE_ACCOUNT_HEADING: &str = "Create an account";

/// Menu labels.
pub const MENU_RULES: &str = "Read the rules";
/// See [`MENU_RULES`].
pub const MENU_REQUEST_INVITE: &str = "Request an invite";
/// See [`MENU_RULES`].
pub const MENU_REDEEM_INVITE: &str = "Redeem an invite code";
/// See [`MENU_RULES`].
pub const MENU_APPLY: &str = "Apply for membership";
/// See [`MENU_RULES`].
pub const MENU_CREATE_ACCOUNT: &str = "Create an account";
/// The disconnect entry, always last.
pub const MENU_QUIT: &str = "Disconnect";

/// Footer hints.
pub const FOOTER_HOME: &str = "Press a key from the list.";
/// See [`FOOTER_HOME`].
pub const FOOTER_PAGE: &str = "Esc: back";
/// See [`FOOTER_HOME`].
pub const FOOTER_FORM: &str = "Enter: next or send   Esc: back   Ctrl-C: disconnect";

/// Honest privacy line on the rules page. Fixed chrome: sysops cannot edit it away, because
/// the product must not imply privacy it does not have (D3).
pub const PRIVACY_NOTE: &str = "Privacy: in this version the server can read everything except \
PGP-encrypted mail and files you encrypt yourself.";

/// Banner used when the sysop has not set one.
pub const DEFAULT_BANNER: &str = "A quiet place.";
/// Rules used when the sysop has not set any.
pub const DEFAULT_RULES: &str = "Be decent to the people here.\n\
No spam, scams or harassment.\n\
Do not share what other members say outside the board.";

/// Field labels.
pub const LABEL_HANDLE: &str = "Handle you would like";
/// See [`LABEL_HANDLE`].
pub const LABEL_ABOUT: &str = "A line about yourself";
/// See [`LABEL_HANDLE`].
pub const LABEL_CODE: &str = "Invite code";

/// The one answer to the request-an-invite form (M2-E). It is the same whether the request was
/// kept or dropped (a rate limit, a full queue, a repeat, a recent refusal), so it tells a
/// stranger nothing about the board. It says what is true either way: who can read the text if
/// it is kept, and that nobody is notified.
pub const REQUEST_INVITE_ANSWER: &str = "Thank you. If your request is kept, the staff and trusted members here can read your handle, your message and this key's fingerprint, and the staff decide when they can. Nobody is notified: connect again later with this same key to find out.";

/// The one answer to the application form (M2-G). Like [`REQUEST_INVITE_ANSWER`] it is shown
/// before the store is asked and is the same whether the application was kept or dropped (a
/// rate limit, a full queue, a repeat, a recent refusal, a key that already has an account).
/// It says what is true either way: who can read the text if it is kept, that the account
/// stays inactive until the staff decide, and that nobody is notified.
pub const APPLY_ANSWER: &str = "Thank you. If your application is kept, the staff and trusted members here can read your handle, your message and this key's fingerprint, and the staff decide when they can. This key stays outside the board until they say yes. Nobody is notified: connect again later with this same key to find out.";

/// The one answer to a create-account attempt that did not work (M2-G). It is the same for a
/// taken handle, a key that already has an account, a rate limit, a day whose cap is used up
/// and a store that failed, so it is not an oracle for any of them. A day that is full shows
/// [`OPEN_CAPPED_NOTICE`] to the next connection.
pub const CREATE_ACCOUNT_FAILED: &str = "That account could not be created. Try a different handle, or disconnect and connect again later.";

/// Shown on the home and application pages of an `open` board on a day when its cap on new
/// accounts is used up (M2-G, D2). It is the one thing a stranger can read about the cap, and
/// it says what happens instead.
pub const OPEN_CAPPED_NOTICE: &str = "Today's limit on new accounts has been reached. Until the day ends (UTC), people apply instead and the staff decide.";

/// The one answer to a redemption that did not work. It is the same for a code that does not
/// exist, one that was used or has expired, a handle that is taken, a key that already has an
/// account and a store that failed, so it is not an oracle for any of them (D67).
pub const REDEEM_FAILED: &str =
    "That invite could not be used. Check the code, or try a different handle.";

/// Validation messages. They depend only on what was typed, never on who is a member, so they
/// cannot tell a member from a stranger.
pub const ERR_EMPTY: &str = "Please fill in every field.";
/// See [`ERR_EMPTY`].
pub const ERR_HANDLE_TOO_SHORT: &str = "A handle needs at least 2 characters.";
/// See [`ERR_EMPTY`].
pub const ERR_HANDLE_TOO_LONG: &str = "A handle can have at most 24 characters.";
/// See [`ERR_EMPTY`].
pub const ERR_HANDLE_CHARS: &str =
    "A handle may use letters, digits, _ and -, and must start with a letter or digit.";
/// See [`ERR_EMPTY`]. The reserved list is fixed and public, so this says nothing about members.
pub const ERR_HANDLE_RESERVED: &str = "That handle is reserved. Please choose another.";

/// Shown when a connection has used up its form submissions.
pub const ERR_TOO_MANY: &str = "Too many attempts on this connection. Goodbye.";
