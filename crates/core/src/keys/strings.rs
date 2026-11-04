//! Fixed text of the keys screens and the waiting-room entries (chrome). Nothing here calls
//! anything "private": the server checks a key by its fingerprint and says what it keeps.

/// Settings menu entry.
pub const MENU_KEYS: &str = "My keys";
/// Sysop menu label (the key that opens the staff screen is in the menu tree).
pub const HEADING: &str = "My keys";
/// What the page is about.
pub const NOTE_KEYS: &str = "A key signs you in without typing your password. The server keeps only each key's fingerprint.";
/// No keys.
pub const NO_KEYS: &str = "No key is saved. You sign in with your password.";
/// Menu: add.
pub const MENU_ADD: &str = "Add a key";
/// Menu: remove.
pub const MENU_REMOVE: &str = "Remove a key";
/// Menu: make a device-link code.
pub const MENU_LINK: &str = "Link a new machine";
/// Menu: back.
pub const MENU_BACK: &str = "Back";
/// Prompt for a pasted key.
pub const PROMPT_KEY: &str = "Public key";
/// Hint under the paste prompt.
pub const HINT_ADD: &str = "Paste the one line from your .pub file (ssh-ed25519 ...). Only Ed25519 keys are accepted.";
/// Hint when picking a key to remove.
pub const HINT_REMOVE: &str = "Press the number of the key to remove.";
/// Said when more keys exist than can be picked.
pub const MORE_KEYS: &str = "More keys are saved than can be picked here. Remove one to reach the rest.";
/// Footer: list.
pub const FOOTER_LIST: &str = "Esc: back";
/// Footer: paste entry.
pub const FOOTER_ENTRY: &str = "Enter: add   Backspace: delete   Esc: cancel";
/// Footer: pick.
pub const FOOTER_PICK: &str = "1-9: choose   Esc: cancel";
/// Footer: confirm.
pub const FOOTER_CONFIRM: &str = "y: remove it   n or Esc: keep it";
/// Footer: waiting.
pub const FOOTER_WORKING: &str = "Working...";
/// Footer: a code is showing.
pub const FOOTER_CODE: &str = "Enter: done (the code is not shown again)";
/// Added.
pub const DONE_ADDED: &str = "Key added. You will be reminded of this at your next sign-in.";
/// Removed.
pub const DONE_REMOVED: &str = "Key removed. You will be reminded of this at your next sign-in.";
/// Not a key.
pub const ERR_NOT_A_KEY: &str = "That is not an Ed25519 public key. Paste the single line from your .pub file.";
/// One message for a key that cannot be added, whoever has it.
pub const ERR_CANNOT_ADD: &str = "That key cannot be added.";
/// No such key.
pub const ERR_NO_SUCH_KEY: &str = "That key is not on your account.";
/// Last way in.
pub const ERR_LAST_WAY_IN: &str = "That is your only way in. Set a password or add another key first.";
/// Rate limit.
pub const ERR_RATE_LIMITED: &str = "You have made enough codes for now. Try again later.";
/// Not permitted.
pub const ERR_DENIED: &str = "That is not permitted for this account.";
/// Failed.
pub const ERR_FAILED: &str = "That did not work. Try again in a moment.";

/// Heading of the code page.
pub const CODE_HEADING: &str = "Link a new machine";
/// What to do with a link code.
pub const CODE_STEPS_LINK: [&str; 3] = [
    "On the new machine connect as new@ this board and choose \"Link this device\".",
    "Enter the code below there. It works once and expires in 10 minutes.",
    "Anyone who has the code can link their key, so tell it only to yourself.",
];
/// Label before the code.
pub const CODE_LABEL: &str = "Code:";
/// Telnet cannot bind a key.
pub const CODE_NOTE_TELNET: &str = "A code made here can be used from an ssh connection only; telnet has no key to bind.";

/// Heading of the staff page.
pub const RECOVERY_HEADING: &str = "Recovery code";
/// Menu label in Sysop tools.
pub const MENU_RECOVERY: &str = "Recovery code";
/// Staff instructions.
pub const RECOVERY_RULES: [&str; 3] = [
    "Confirm who is asking outside this board first (a call, a message you already trust).",
    "The member needs this code, once, within 24 hours, to bind a new key and set a new password.",
    "You can help only members below your own rank. The code is not shown again.",
];
/// Prompt for the member's handle.
pub const PROMPT_HANDLE: &str = "Member's handle";
/// Staff cannot help this member.
pub const ERR_NOT_YOURS: &str = "No code was made. Check the handle; you can help only active members below your rank.";
/// Staff footer: entry.
pub const FOOTER_RECOVERY: &str = "Enter: make a code   Esc: back";
/// After a recovery code.
pub const RECOVERY_DONE: &str = "Give the member this code. It works once and expires in 24 hours.";

// Waiting room.

/// Waiting-room menu: link this device.
pub const MENU_LINK_DEVICE: &str = "Link this device";
/// Waiting-room menu: use a recovery code.
pub const MENU_RECOVER: &str = "Recovery code";
/// Link page heading.
pub const LINK_DEVICE_HEADING: &str = "Link this device";
/// Recovery page heading.
pub const RECOVER_HEADING: &str = "Recovery code";
/// Field: the code.
pub const LABEL_CODE: &str = "Code";
/// One refusal for every cause.
pub const CODE_REFUSED: &str = "That code did not work. Check it, or ask for a new one.";
/// Recovery explanation.
pub const RECOVER_NOTE: &str = "Staff gave you a code. Enter it and choose a new password: this connection's key and the new password are saved to your account.";

// Sign-in notices.

/// Heading of the notices block at sign-in.
pub const NOTICE_HEADING: &str = "Changes to your keys since you were last here:";
/// A key was pasted. `{when}` is a UTC time and `{key}` the fingerprint.
pub const NOTICE_KEY_ADDED: &str = "A key was added on {when} ({key}).";
/// A key was removed.
pub const NOTICE_KEY_REMOVED: &str = "A key was removed on {when} ({key}).";
/// A key was linked with the password.
pub const NOTICE_KEY_LINKED: &str = "A key was added with your password on {when} ({key}).";
/// A key was linked with a code.
pub const NOTICE_DEVICE_LINKED: &str = "A key was added with a device-link code on {when} ({key}).";
/// A recovery code was used.
pub const NOTICE_RECOVERED: &str = "A recovery code was used on {when}: a key was added and your password was replaced ({key}).";
/// What to do if it was not you.
pub const NOTICE_IF_NOT_YOU: &str = "If this was not you, remove the key under My keys, change your password, and tell the sysop.";
