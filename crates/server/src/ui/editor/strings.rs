//! Every fixed string of the editor, in one place, so a theme or translation can replace them
//! later (the M3-A string table takes these over; the editor owns its strings until then so
//! this slice does not edit the shared table). All of it is UI chrome, none of it is content.
//! A `{name}` is filled by `terminal_settings::strings::fill`.

/// Shown when the window is too small to draw the editor. The text is kept.
pub(super) const TOO_SMALL: &str = "Window too small (needs {cols}x{rows}); your text is kept.";

/// Notice: a saved draft was loaded.
pub(super) const RESUMED: &str = "Resumed your saved draft.";
/// Notice: a saved draft was loaded and the starting text (a quote, for example) was not used.
pub(super) const RESUMED_INSTEAD: &str =
    "Resumed your saved draft instead of the starting text. Leave and discard to start over.";
/// Problem: the character limit stopped an edit.
pub(super) const AT_LIMIT_CHARS: &str = "Limit reached: at most {max} characters.";
/// Problem: the line limit stopped an edit.
pub(super) const AT_LIMIT_LINES: &str = "Limit reached: at most {max} lines.";
/// Problem: send with nothing in the editor.
pub(super) const NOTHING_TO_SEND: &str = "There is nothing to send yet.";
/// Problem: the text could not be sent as it is (cannot happen while the limits hold).
pub(super) const CANNOT_SEND: &str = "The text cannot be sent as it is: {why}.";
/// Problem: saving the draft failed for a reason the member can act on.
pub(super) const DRAFT_REFUSED: &str = "The draft was not saved: {why}.";
/// Problem: saving the draft failed.
pub(super) const DRAFT_FAILED: &str = "The draft could not be saved. Try again.";
/// Counter line: lines and characters used.
pub(super) const COUNTS: &str = "Lines {lines}/{max_lines}   Characters {chars}/{max_chars}";
/// The keys always on screen.
pub(super) const HINT: &str = "Ctrl-X send   Esc leave   Ctrl-G keys";
/// The leave prompt with a context (a draft can be saved).
pub(super) const LEAVE_WITH_DRAFT: &str =
    "Leave the editor?   [s] save draft   [d] discard changes   [c] keep editing";
/// The leave prompt without a context.
pub(super) const LEAVE_WITHOUT_DRAFT: &str =
    "Leave the editor?   [d] discard changes   [c] keep editing";

/// Heading of the key list.
pub(super) const HELP_TITLE: &str = "Editor keys";
/// What the key list is about.
pub(super) const HELP_INTRO: &str = "Any other printable key types a character.";
/// The key list: key, what it does.
pub(super) const HELP_KEYS: &[(&str, &str)] = &[
    ("Ctrl-X", "send the message"),
    (
        "Esc",
        "leave: asks to save a draft or discard changes first",
    ),
    (
        "Ctrl-C, Ctrl-D",
        "log off (a draft is saved first if this editor has one)",
    ),
    ("Ctrl-G", "show this list (? types a question mark)"),
    ("Enter", "new line"),
    ("Tab", "insert spaces"),
    ("Backspace", "delete the character before the cursor"),
    ("Ctrl-B, Ctrl-F", "cursor left, right"),
    ("Ctrl-P, Ctrl-N", "cursor up, down"),
    ("Ctrl-A, Ctrl-E", "start, end of the line"),
    ("Ctrl-T, Ctrl-V", "page up, page down"),
    ("Ctrl-W", "delete the word before the cursor"),
    (
        "Ctrl-K",
        "delete to the end of the line (keeps it for Ctrl-Y)",
    ),
    ("Ctrl-Y", "put back what Ctrl-K deleted"),
    ("Ctrl-Z", "undo"),
    ("Ctrl-R", "redo"),
    (
        "Arrow keys, Home, End, Delete, PageUp, PageDown",
        "not read by the terminal parser yet",
    ),
    ("Pasted text", "arrives without its line breaks for now"),
];
/// Footer of the key list.
pub(super) const HELP_FOOTER: &str = "Press any key to go back to your text.";
