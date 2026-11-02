//! The string-table keys the layout itself uses, as constants so rendering code names them
//! once. The menu tree's own keys (titles, labels, placeholder texts) live in the tree data.

/// Product name, the box title.
pub(crate) const TITLE: &str = "ui.title";
/// Footer hint on a menu.
pub(crate) const FOOTER_MENU: &str = "ui.footer.menu";
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
/// Help footer.
pub(crate) const HELP_FOOTER: &str = "ui.help.footer";
/// Status line text next to the server-readable marker.
pub(crate) const STATUS_SERVER_READABLE: &str = "ui.status.server_readable";
/// Status line text when markers cannot be drawn.
pub(crate) const STATUS_UNAVAILABLE: &str = "ui.status.unavailable";

/// Every key above; the table must hold these plus the menu tree's keys.
pub(crate) const FRAMEWORK: [&str; 14] = [
    TITLE,
    FOOTER_MENU,
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
    STATUS_SERVER_READABLE,
    STATUS_UNAVAILABLE,
];
