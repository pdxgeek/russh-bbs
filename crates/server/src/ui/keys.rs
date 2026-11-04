//! The string-table keys the layout itself uses, as constants so rendering code names them
//! once. The menu tree's own keys (titles, labels, placeholder texts) live in the tree data.

/// Product name, the box title.
pub(crate) const TITLE: &str = "ui.title";
/// Status-bar hint labels (UI-1): the key names are fixed, the words are themeable.
pub(crate) const HINT_HELP: &str = "ui.hint.help";
pub(crate) const HINT_BACK: &str = "ui.hint.back";
pub(crate) const HINT_LOGOFF: &str = "ui.hint.logoff";
pub(crate) const HINT_MOVE: &str = "ui.hint.move";
pub(crate) const HINT_CHOOSE: &str = "ui.hint.choose";
/// Prefix of an informational notice.
pub(crate) const NOTICE_INFO: &str = "ui.notice.info";
/// Prefix of an error notice.
pub(crate) const NOTICE_ERROR: &str = "ui.notice.error";
/// Tag after an item whose area is not built yet.
pub(crate) const TAG_NOT_BUILT: &str = "ui.tag.not_built";
/// Help heading.
pub(crate) const HELP_TITLE: &str = "ui.help.title";
/// Help intro.
pub(crate) const HELP_INTRO: &str = "ui.help.intro";
/// Help line for `?`.
pub(crate) const HELP_KEY_HELP: &str = "ui.help.key_help";
/// Help line for Esc.
pub(crate) const HELP_KEY_BACK: &str = "ui.help.key_back";
/// Help line for Ctrl-C.
pub(crate) const HELP_KEY_QUIT: &str = "ui.help.key_quit";
/// Help line about choosing items.
pub(crate) const HELP_MENU: &str = "ui.help.menu";
/// Notice when Mail is chosen over telnet, where it is withheld (D90).
pub(crate) const NOTICE_MAIL_SSH_ONLY: &str = "ui.notice.mail_ssh_only";
/// The one D78 (7) explanation about mail, on the help screen.
/// Help footer.
pub(crate) const HELP_FOOTER: &str = "ui.help.footer";
/// Help line explaining the lightbar's cursor keys.
pub(crate) const HELP_LIGHTBAR: &str = "ui.help.lightbar";
/// Marker drawn before the row under the lightbar cursor (a word-free cue that survives a
/// mono terminal).
pub(crate) const POINTER: &str = "ui.pointer";
/// Status line text when markers cannot be drawn.
pub(crate) const STATUS_UNAVAILABLE: &str = "ui.status.unavailable";

/// Pager prompt, pause on, more pages follow (`{page}`, `{pages}`).
pub(crate) const PAGER_MORE: &str = "ui.pager.more";
/// Pager prompt, pause on, last page (`{page}`, `{pages}`).
pub(crate) const PAGER_END: &str = "ui.pager.end";
/// Pager prompt, pause off, text longer than the screen (`{hidden}`).
pub(crate) const PAGER_CLIPPED: &str = "ui.pager.clipped";

/// Status-bar hint for the widget pop-over of the focus layout.
pub(crate) const HINT_WIDGETS: &str = "ui.hint.widgets";
/// Note when a layout is replaced by a smaller one (`{shown}`, `{wanted}`).
pub(crate) const LAYOUT_FALLBACK: &str = "ui.layout.fallback";
/// A layout's name, by layout.
pub(crate) const LAYOUT_NAMES: [&str; 7] = [
    "ui.layout.classic",
    "ui.layout.lightbar",
    "ui.layout.compact",
    "ui.layout.dashboard",
    "ui.layout.wide",
    "ui.layout.focus",
    "ui.layout.split",
];
/// The panel title of a widget slot, by slot (the order of `panes::Slot`).
pub(crate) const SLOT_TITLES: [&str; 8] = [
    "ui.slot.identity",
    "ui.slot.clock",
    "ui.slot.who",
    "ui.slot.callers",
    "ui.slot.posts",
    "ui.slot.mail",
    "ui.slot.chat",
    "ui.slot.activity",
];
/// What an empty widget slot says.
pub(crate) const SLOT_EMPTY: &str = "ui.slot.empty";
/// The pop-over's title.
pub(crate) const SLOT_POPOVER: &str = "ui.slot.popover";

/// Every key above; the table must hold these plus the menu tree's keys.
pub(crate) const FRAMEWORK: [&str; 23] = [
    NOTICE_MAIL_SSH_ONLY,
    HINT_HELP,
    HINT_BACK,
    HINT_LOGOFF,
    HINT_MOVE,
    HINT_CHOOSE,
    HELP_LIGHTBAR,
    POINTER,
    TITLE,
    NOTICE_INFO,
    NOTICE_ERROR,
    TAG_NOT_BUILT,
    HELP_TITLE,
    HELP_INTRO,
    HELP_KEY_HELP,
    HELP_KEY_BACK,
    HELP_KEY_QUIT,
    HELP_MENU,
    HELP_FOOTER,
    STATUS_UNAVAILABLE,
    PAGER_MORE,
    PAGER_END,
    PAGER_CLIPPED,
];

/// The keys of the layouts and widget slots (UI-2), checked with [`FRAMEWORK`].
pub(crate) const LAYOUT_KEYS: [&str; 19] = [
    HINT_WIDGETS,
    LAYOUT_FALLBACK,
    LAYOUT_NAMES[0],
    LAYOUT_NAMES[1],
    LAYOUT_NAMES[2],
    LAYOUT_NAMES[3],
    LAYOUT_NAMES[4],
    LAYOUT_NAMES[5],
    LAYOUT_NAMES[6],
    SLOT_TITLES[0],
    SLOT_TITLES[1],
    SLOT_TITLES[2],
    SLOT_TITLES[3],
    SLOT_TITLES[4],
    SLOT_TITLES[5],
    SLOT_TITLES[6],
    SLOT_TITLES[7],
    SLOT_EMPTY,
    SLOT_POPOVER,
];
