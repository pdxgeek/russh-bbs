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
/// The pop-over's title.
pub(crate) const SLOT_POPOVER: &str = "ui.slot.popover";

/// The sysop theme preview's words (UI-3).
pub(crate) const PREVIEW_TITLE: &str = "ui.preview.title";
pub(crate) const PREVIEW_STATE: &str = "ui.preview.state";
pub(crate) const PREVIEW_CLIPPED: &str = "ui.preview.clipped";
pub(crate) const PREVIEW_ART: &str = "ui.preview.art";
pub(crate) const PREVIEW_ART_NONE: &str = "ui.preview.art_none";
pub(crate) const PREVIEW_PROBLEMS: &str = "ui.preview.problems";
pub(crate) const PREVIEW_TIER_UNICODE: &str = "ui.preview.tier_unicode";
pub(crate) const PREVIEW_TIER_ASCII: &str = "ui.preview.tier_ascii";
pub(crate) const PREVIEW_COLOUR: &str = "ui.preview.colour";
pub(crate) const PREVIEW_MONO: &str = "ui.preview.mono";
pub(crate) const PREVIEW_SAMPLE_MENU: &str = "ui.preview.sample_menu";
pub(crate) const PREVIEW_SAMPLE_LOGIN: &str = "ui.preview.sample_login";
pub(crate) const PREVIEW_ON: &str = "ui.preview.on";
pub(crate) const PREVIEW_OFF: &str = "ui.preview.off";
pub(crate) const PREVIEW_HINT_STYLE: &str = "ui.preview.hint_style";
pub(crate) const PREVIEW_HINT_LAYOUT: &str = "ui.preview.hint_layout";
pub(crate) const PREVIEW_HINT_SIZE: &str = "ui.preview.hint_size";
pub(crate) const PREVIEW_HINT_TIER: &str = "ui.preview.hint_tier";
pub(crate) const PREVIEW_HINT_COLOUR: &str = "ui.preview.hint_colour";
pub(crate) const PREVIEW_HINT_SAMPLE: &str = "ui.preview.hint_sample";
pub(crate) const PREVIEW_HINT_ART: &str = "ui.preview.hint_art";
pub(crate) const PREVIEW_HINT_BACK: &str = "ui.preview.hint_back";
pub(crate) const PREVIEW_SAMPLE_BODY: &str = "ui.preview.sample_body";

/// The member style gallery's and the sysop Theme packs screen's words (UI-7).
pub(crate) const GALLERY_TITLE: &str = "ui.gallery.title";
pub(crate) const GALLERY_HINT_MOVE: &str = "ui.gallery.hint_move";
pub(crate) const GALLERY_HINT_USE: &str = "ui.gallery.hint_use";
pub(crate) const GALLERY_HINT_SAMPLE: &str = "ui.gallery.hint_sample";
pub(crate) const GALLERY_HINT_BACK: &str = "ui.gallery.hint_back";
pub(crate) const GALLERY_IN_USE: &str = "ui.gallery.in_use";
pub(crate) const GALLERY_LOCKED: &str = "ui.gallery.locked";
pub(crate) const GALLERY_OFFER: &str = "ui.gallery.offer";
pub(crate) const PACKS_TITLE: &str = "ui.packs.title";
pub(crate) const PACKS_HINT_MOVE: &str = "ui.packs.hint_move";
pub(crate) const PACKS_HINT_EXPORT: &str = "ui.packs.hint_export";
pub(crate) const PACKS_HINT_IMPORT: &str = "ui.packs.hint_import";
pub(crate) const PACKS_HINT_RESCAN: &str = "ui.packs.hint_rescan";
pub(crate) const PACKS_HINT_BACK: &str = "ui.packs.hint_back";
pub(crate) const PACKS_STYLES: &str = "ui.packs.styles";
pub(crate) const PACKS_FILES: &str = "ui.packs.files";
pub(crate) const PACKS_NO_FILES: &str = "ui.packs.no_files";
pub(crate) const PACKS_READY: &str = "ui.packs.ready";
pub(crate) const PACKS_REFUSED: &str = "ui.packs.refused";
pub(crate) const PACKS_LOADING: &str = "ui.packs.loading";
pub(crate) const PACKS_UNAVAILABLE: &str = "ui.packs.unavailable";
pub(crate) const PACKS_BUILT_IN: &str = "ui.packs.built_in";

/// Every gallery and packs key, checked with [`FRAMEWORK`].
pub(crate) const GALLERY_KEYS: [&str; 22] = [
    GALLERY_TITLE,
    GALLERY_HINT_MOVE,
    GALLERY_HINT_USE,
    GALLERY_HINT_SAMPLE,
    GALLERY_HINT_BACK,
    GALLERY_IN_USE,
    GALLERY_LOCKED,
    GALLERY_OFFER,
    PACKS_TITLE,
    PACKS_HINT_MOVE,
    PACKS_HINT_EXPORT,
    PACKS_HINT_IMPORT,
    PACKS_HINT_RESCAN,
    PACKS_HINT_BACK,
    PACKS_STYLES,
    PACKS_FILES,
    PACKS_NO_FILES,
    PACKS_READY,
    PACKS_REFUSED,
    PACKS_LOADING,
    PACKS_UNAVAILABLE,
    PACKS_BUILT_IN,
];

/// Every preview key, checked with [`FRAMEWORK`].
pub(crate) const PREVIEW_KEYS: [&str; 23] = [
    PREVIEW_TITLE,
    PREVIEW_STATE,
    PREVIEW_CLIPPED,
    PREVIEW_ART,
    PREVIEW_ART_NONE,
    PREVIEW_PROBLEMS,
    PREVIEW_TIER_UNICODE,
    PREVIEW_TIER_ASCII,
    PREVIEW_COLOUR,
    PREVIEW_MONO,
    PREVIEW_SAMPLE_MENU,
    PREVIEW_SAMPLE_LOGIN,
    PREVIEW_ON,
    PREVIEW_OFF,
    PREVIEW_HINT_STYLE,
    PREVIEW_HINT_LAYOUT,
    PREVIEW_HINT_SIZE,
    PREVIEW_HINT_TIER,
    PREVIEW_HINT_COLOUR,
    PREVIEW_HINT_SAMPLE,
    PREVIEW_HINT_ART,
    PREVIEW_HINT_BACK,
    PREVIEW_SAMPLE_BODY,
];

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
pub(crate) const LAYOUT_KEYS: [&str; 10] = [
    HINT_WIDGETS,
    LAYOUT_FALLBACK,
    LAYOUT_NAMES[0],
    LAYOUT_NAMES[1],
    LAYOUT_NAMES[2],
    LAYOUT_NAMES[3],
    LAYOUT_NAMES[4],
    LAYOUT_NAMES[5],
    LAYOUT_NAMES[6],
    SLOT_POPOVER,
];
