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
/// Notice when Mail is chosen over telnet, where it is withheld (D90).
pub(crate) const NOTICE_MAIL_SSH_ONLY: &str = "ui.notice.mail_ssh_only";
/// The one D78 (7) explanation about mail, on the help screen.
pub(crate) const HELP_MAIL: &str = "ui.help.mail";
/// Help footer.
pub(crate) const HELP_FOOTER: &str = "ui.help.footer";
/// Footer hint on a lightbar menu.
pub(crate) const FOOTER_LIGHTBAR: &str = "ui.footer.lightbar";
/// Help line explaining the lightbar's cursor keys.
pub(crate) const HELP_LIGHTBAR: &str = "ui.help.lightbar";
/// Marker drawn before the row under the lightbar cursor (a word-free cue that survives a
/// mono terminal).
pub(crate) const POINTER: &str = "ui.pointer";
/// Status line text next to the server-readable marker.
pub(crate) const STATUS_SERVER_READABLE: &str = "ui.status.server_readable";
/// Status line text when markers cannot be drawn.
pub(crate) const STATUS_UNAVAILABLE: &str = "ui.status.unavailable";

/// Pager prompt, pause on, more pages follow (`{page}`, `{pages}`).
pub(crate) const PAGER_MORE: &str = "ui.pager.more";
/// Pager prompt, pause on, last page (`{page}`, `{pages}`).
pub(crate) const PAGER_END: &str = "ui.pager.end";
/// Pager prompt, pause off, text longer than the screen (`{hidden}`).
pub(crate) const PAGER_CLIPPED: &str = "ui.pager.clipped";

/// Every key above; the table must hold these plus the menu tree's keys.
pub(crate) const FRAMEWORK: [&str; 22] = [
    NOTICE_MAIL_SSH_ONLY,
    HELP_MAIL,
    FOOTER_LIGHTBAR,
    HELP_LIGHTBAR,
    POINTER,
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
    PAGER_MORE,
    PAGER_END,
    PAGER_CLIPPED,
];
