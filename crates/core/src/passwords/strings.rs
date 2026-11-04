//! Fixed text of the password screen (chrome). Kept out of the state machine so it can be
//! themed later. Nothing here calls anything "private": the server checks the password and
//! stores a salted hash, and says so.

/// Settings menu entry.
pub const MENU_PASSWORD: &str = "Password";
/// Page heading.
pub const HEADING: &str = "Password";
/// Status: a password is set.
pub const STATUS_SET: &str = "A password is set for this account.";
/// Status: none set.
pub const STATUS_NOT_SET: &str = "No password is set. You sign in with your SSH key only.";
/// What a password does, and does not.
pub const NOTE_USE: &str = "A password signs you in over SSH only if the sysop has turned password login on. The server checks it and keeps only a salted hash, never the password.";
/// Menu: set.
pub const MENU_SET: &str = "Set or change password";
/// Menu: clear.
pub const MENU_CLEAR: &str = "Clear password";
/// Menu: back.
pub const MENU_BACK: &str = "Back";
/// Prompt: current.
pub const PROMPT_CURRENT: &str = "Current password";
/// Prompt: new.
pub const PROMPT_NEW: &str = "New password";
/// Prompt: repeat.
pub const PROMPT_REPEAT: &str = "Repeat new password";
/// Hint under the new-password prompts.
pub const HINT_RULES: &str = "At least 12 characters. Nothing you type is shown.";
/// Hint under the current-password prompt.
pub const HINT_CURRENT: &str = "Nothing you type is shown.";
/// Confirmation to clear (key session).
pub const CONFIRM_CLEAR: &str = "Clear the password? You will sign in with your SSH key only.";
/// Footer: menu.
pub const FOOTER_MENU: &str = "Esc: back";
/// Footer: entry.
pub const FOOTER_ENTRY: &str = "Enter: next   Backspace: delete   Esc: cancel";
/// Footer: confirmation.
pub const FOOTER_CONFIRM: &str = "y: clear it   n or Esc: keep it";
/// Footer: waiting for the server.
pub const FOOTER_WORKING: &str = "Working...";
/// Notice: set.
pub const DONE_SET: &str = "Password set.";
/// Notice: cleared.
pub const DONE_CLEARED: &str = "Password cleared.";
/// Notice: the two entries differ.
pub const ERR_MISMATCH: &str = "The two entries did not match. Nothing was changed.";
/// Notice: too short.
pub const ERR_SHORT: &str = "Too short: use at least 12 characters.";
/// Notice: too long.
pub const ERR_LONG: &str = "Too long: use at most 128 characters.";
/// Notice: control character.
pub const ERR_CONTROL: &str = "That holds a control character. Use plain characters.";
/// Notice: wrong current password.
pub const ERR_CURRENT: &str = "That is not the current password. Nothing was changed.";
/// Notice: the password is the account's only way in (D94).
pub const ERR_LAST_CREDENTIAL: &str =
    "This account has no SSH key, so the password is the only way in. It was not cleared.";
/// Notice: not allowed.
pub const ERR_DENIED: &str = "You may not change the password.";
/// Notice: failure.
pub const ERR_FAILED: &str = "That did not work. Nothing was changed.";
