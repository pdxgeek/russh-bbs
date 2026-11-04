//! Fixed words for the door list. They are chrome, not content, and not themeable yet (the menu
//! label itself is in the string table); a door's own text comes from the door and is sanitized
//! on its way to the screen.

pub(super) const HEADING: &str = "Doors";
pub(super) const PICK: &str = "Pick a door: Up and Down, then Enter, or its number. Esc goes back.";
pub(super) const NONE: &str = "No doors are installed on this board.";
pub(super) const OPENING: &str = "Opening the door...";
pub(super) const LEAVE_HINT: &str = "Ctrl-C logs off from anywhere.";

pub(super) const BARRED: &str = "You are not allowed to play this door.";
pub(super) const DENIED: &str = "You do not have access to doors.";
pub(super) const TOO_NEW: &str =
    "Your saved game was written by a newer version of this door. It was left untouched.";
pub(super) const FAILED: &str = "The door could not be opened right now. Try again later.";
pub(super) const CONFLICT: &str =
    "Your save changed in another session, so this door was closed to keep it safe. Open it again.";
pub(super) const TOO_LARGE: &str =
    "The door tried to save more than it is allowed to and was closed. Nothing was lost.";
pub(super) const SAVE_FAILED: &str =
    "The door could not save and was closed. What you did since the last save may be lost.";
