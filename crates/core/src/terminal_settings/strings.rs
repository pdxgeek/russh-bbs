//! The string-table keys of the login, logoff, glyph test and settings screens (M3-A moved the
//! text itself into the server's string table, `ui/strings.toml`, section `[member]`, so a theme
//! can replace it; the constants here are the keys the flow names). A flow screen carries keys
//! and the renderer resolves them; text with placeholders is filled by the flow through a
//! [`TextSource`]. [`TITLE`] stays text: it is the product name every screen shares.
//!
//! Templates use `{name}` placeholders filled by [`fill`]. Substituted values are user-derived
//! (the handle) and must already be sanitized by the caller.
//!
//! Nothing here claims privacy: the server can read everything except PGP-encrypted mail and
//! files members encrypt themselves, and no screen of this slice shows a lock or the word
//! "private" (D3).

/// Window title, first line of every screen.
pub const TITLE: &str = "russh-bbs";

/// Login screen heading.
pub const LOGIN_HEADING: &str = "member.login_heading";
/// `{handle}` is the sanitized account handle.
pub const SIGNED_IN_AS: &str = "member.signed_in_as";
/// `{when}` is a UTC time.
pub const LAST_LOGIN: &str = "member.last_login";
/// Shown when no earlier login is recorded.
pub const FIRST_LOGIN: &str = "member.first_login";
/// `{icons}` is the tier name, `{color}` and `{pause}` are on/off.
pub const TERMINAL_LINE: &str = "member.terminal_line";
/// The terminal line while no tier has been chosen.
pub const TERMINAL_LINE_UNSET: &str = "member.terminal_line_unset";
/// Login footer.
pub const LOGIN_FOOTER: &str = "member.login_footer";

/// Glyph test heading.
pub const GLYPH_HEADING: &str = "member.glyph_heading";
/// Explains why the question is asked.
pub const GLYPH_WHY: &str = "member.glyph_why";
/// `{style}` is the tier's name.
pub const GLYPH_STYLE: &str = "member.glyph_style";
/// Names the icons in the sample row, in order.
pub const GLYPH_LEGEND: &str = "member.glyph_legend";
/// The question.
pub const GLYPH_QUESTION: &str = "member.glyph_question";
/// Footer on the first run.
pub const GLYPH_FOOTER_FIRST: &str = "member.glyph_footer_first";
/// Footer on a re-run.
pub const GLYPH_FOOTER_AGAIN: &str = "member.glyph_footer_again";

/// Settings heading.
pub const SETTINGS_HEADING: &str = "member.settings_heading";
/// `{value}` is a number or "automatic".
pub const SETTING_WIDTH: &str = "member.setting_width";
/// `{value}` is a number or "automatic".
pub const SETTING_HEIGHT: &str = "member.setting_height";
/// `{value}` is on/off.
pub const SETTING_COLOR: &str = "member.setting_color";
/// `{value}` is on/off.
pub const SETTING_PAUSE: &str = "member.setting_pause";
/// Added to the width line when the stored value is wider than the terminal. `{terminal}` is
/// the terminal's width. The value is kept; it applies again in a wider window.
pub const CLAMPED_WIDTH: &str = "member.clamped_width";
/// Like [`CLAMPED_WIDTH`] for the height; `{terminal}` is the terminal's height.
pub const CLAMPED_HEIGHT: &str = "member.clamped_height";
/// `{value}` is the tier name, or [`NOT_CHOSEN`].
pub const SETTING_ICONS: &str = "member.setting_icons";
/// Value text for "follow the terminal".
pub const AUTOMATIC: &str = "member.automatic";
/// Value text for no tier chosen.
pub const NOT_CHOSEN: &str = "member.not_chosen";
/// On.
pub const ON: &str = "member.on";
/// Off.
pub const OFF: &str = "member.off";
/// Settings menu labels.
pub const MENU_WIDTH: &str = "member.menu_width";
/// See [`MENU_WIDTH`].
pub const MENU_HEIGHT: &str = "member.menu_height";
/// See [`MENU_WIDTH`].
pub const MENU_COLOR: &str = "member.menu_color";
/// See [`MENU_WIDTH`].
pub const MENU_PAUSE: &str = "member.menu_pause";
/// See [`MENU_WIDTH`].
pub const MENU_ICONS: &str = "member.menu_icons";
/// See [`MENU_WIDTH`].
pub const MENU_BACK: &str = "member.menu_back";
/// Settings footer.
pub const SETTINGS_FOOTER: &str = "member.settings_footer";
/// Label of the number field.
pub const FIELD_WIDTH: &str = "member.field_width";
/// Label of the number field.
pub const FIELD_HEIGHT: &str = "member.field_height";
/// Footer while typing a number.
pub const EDIT_FOOTER: &str = "member.edit_footer";

/// Notice: width out of range. Keep in step with `MIN_WIDTH`/`MAX_WIDTH` (a test checks).
pub const ERR_WIDTH: &str = "member.err_width";
/// Notice: height out of range. Keep in step with `MIN_HEIGHT`/`MAX_HEIGHT` (a test checks).
pub const ERR_HEIGHT: &str = "member.err_height";
/// Notice: saved.
pub const SAVED: &str = "member.saved";

/// Shown when the account no longer holds `account.edit_settings`: the change applies on the
/// next action, so the page the person was on closes instead of pretending to save.
pub const NOT_PERMITTED: &str = "member.not_permitted";
/// Shown on the menu when a members screen closes because a permission it needed is gone
/// (M2-F): the change applies on the next action.
pub const MEMBERS_NOT_PERMITTED: &str = "member.members_not_permitted";
/// Notice: tier chosen.
pub const CHOSE_NERD: &str = "member.chose_nerd";
/// Notice: tier chosen.
pub const CHOSE_UNICODE: &str = "member.chose_unicode";
/// Notice: fallback chosen.
pub const CHOSE_ASCII: &str = "member.chose_ascii";

/// Tier display names.
pub const TIER_NERD: &str = "member.tier_nerd";
/// See [`TIER_NERD`].
pub const TIER_UNICODE: &str = "member.tier_unicode";
/// See [`TIER_NERD`].
pub const TIER_ASCII: &str = "member.tier_ascii";

/// Logoff heading.
pub const LOGOFF_HEADING: &str = "member.logoff_heading";
/// `{handle}` is the sanitized account handle.
pub const GOODBYE: &str = "member.goodbye";
/// Logoff body.
pub const SETTINGS_SAVED_NOTE: &str = "member.settings_saved_note";

/// Layout and theme on one line, to keep the settings screen within 80x24. `{layout}` is one
/// of the `LAYOUT_*` texts and `{theme}` is the theme's name.
pub const SETTING_LOOK: &str = "member.setting_look";
/// Settings menu label: switch to the next layout.
pub const MENU_LAYOUT: &str = "member.menu_layout";
/// Settings menu label: switch to the next theme.
pub const MENU_THEME: &str = "member.menu_theme";
/// Name of the classic hotkey layout.
pub const LAYOUT_CLASSIC: &str = "member.layout_classic";
/// Name of the lightbar layout.
pub const LAYOUT_LIGHTBAR: &str = "member.layout_lightbar";
/// Name of the compact layout.
pub const LAYOUT_COMPACT: &str = "member.layout_compact";

/// Every key above except [`TITLE`]; the string table must hold exactly these in `member.*`.
pub const TEXT_KEYS: [&str; 56] = [
    LOGIN_HEADING,
    SIGNED_IN_AS,
    LAST_LOGIN,
    FIRST_LOGIN,
    TERMINAL_LINE,
    TERMINAL_LINE_UNSET,
    LOGIN_FOOTER,
    GLYPH_HEADING,
    GLYPH_WHY,
    GLYPH_STYLE,
    GLYPH_LEGEND,
    GLYPH_QUESTION,
    GLYPH_FOOTER_FIRST,
    GLYPH_FOOTER_AGAIN,
    SETTINGS_HEADING,
    SETTING_WIDTH,
    SETTING_HEIGHT,
    SETTING_COLOR,
    SETTING_PAUSE,
    CLAMPED_WIDTH,
    CLAMPED_HEIGHT,
    SETTING_ICONS,
    AUTOMATIC,
    NOT_CHOSEN,
    ON,
    OFF,
    MENU_WIDTH,
    MENU_HEIGHT,
    MENU_COLOR,
    MENU_PAUSE,
    MENU_ICONS,
    MENU_BACK,
    SETTINGS_FOOTER,
    FIELD_WIDTH,
    FIELD_HEIGHT,
    EDIT_FOOTER,
    ERR_WIDTH,
    ERR_HEIGHT,
    SAVED,
    NOT_PERMITTED,
    MEMBERS_NOT_PERMITTED,
    CHOSE_NERD,
    CHOSE_UNICODE,
    CHOSE_ASCII,
    TIER_NERD,
    TIER_UNICODE,
    TIER_ASCII,
    LOGOFF_HEADING,
    GOODBYE,
    SETTINGS_SAVED_NOTE,
    SETTING_LOOK,
    MENU_LAYOUT,
    MENU_THEME,
    LAYOUT_CLASSIC,
    LAYOUT_LIGHTBAR,
    LAYOUT_COMPACT,
];

/// Where the flow reads text from: the server's string table, or a fixture in a test. Values
/// come from the sysop or the build, never from members.
pub trait TextSource {
    /// The text for `key`.
    fn text<'a>(&'a self, key: &'a str) -> &'a str;
}

/// Fill `{name}` placeholders in `template` in one pass. A value is never re-scanned, so text
/// that looks like a placeholder inside a value stays text. An unknown placeholder is kept.
#[must_use]
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let replaced = after.find('}').and_then(|close| {
            let name = &after[..close];
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value, close))
        });
        if let Some((value, close)) = replaced {
            out.push_str(value);
            rest = &after[close + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}
