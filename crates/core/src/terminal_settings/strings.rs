//! Every fixed string of the login, logoff, glyph test and settings screens. They are UI
//! chrome, so they bypass the sanitizer, and they live here (not in rendering code) so a theme
//! or translation can replace them later (`docs/ui.md`, "Themes and strings").
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
pub const LOGIN_HEADING: &str = "Signed in";
/// `{handle}` is the sanitized account handle.
pub const SIGNED_IN_AS: &str = "Signed in as {handle}.";
/// `{when}` is a UTC time.
pub const LAST_LOGIN: &str = "Last login: {when}";
/// Shown when no earlier login is recorded.
pub const FIRST_LOGIN: &str = "This is your first login.";
/// `{icons}` is the tier name, `{color}` and `{pause}` are on/off.
pub const TERMINAL_LINE: &str = "Terminal: {icons} icons, color {color}, pause {pause}";
/// The terminal line while no tier has been chosen.
pub const TERMINAL_LINE_UNSET: &str =
    "Terminal: icon style not chosen yet (plain ASCII, no color until you do)";
/// Login footer.
pub const LOGIN_FOOTER: &str = "Any key: continue   Ctrl-C: log off";

/// Glyph test heading.
pub const GLYPH_HEADING: &str = "Icon check";
/// Explains why the question is asked.
pub const GLYPH_WHY: &str =
    "The server cannot see your terminal font, so it asks. This shows one icon style.";
/// `{style}` is the tier's name.
pub const GLYPH_STYLE: &str = "Style: {style}";
/// Names the icons in the sample row, in order.
pub const GLYPH_LEGEND: &str = "Board, restricted, unread, feed, mail, invite";
/// The question.
pub const GLYPH_QUESTION: &str = "Do these look like icons?";
/// Footer on the first run.
pub const GLYPH_FOOTER_FIRST: &str =
    "y: yes   n: no   Esc: skip (plain ASCII, no color)   Ctrl-C: log off";
/// Footer on a re-run.
pub const GLYPH_FOOTER_AGAIN: &str = "y: yes   n: no   Esc: cancel   Ctrl-C: log off";

/// Settings heading.
pub const SETTINGS_HEADING: &str = "Terminal settings";
/// `{value}` is a number or "automatic".
pub const SETTING_WIDTH: &str = "Width: {value}";
/// `{value}` is a number or "automatic".
pub const SETTING_HEIGHT: &str = "Height: {value}";
/// `{value}` is on/off.
pub const SETTING_COLOR: &str = "Color: {value}";
/// `{value}` is on/off.
pub const SETTING_PAUSE: &str =
    "Pause at screen end: {value} (applies to long text; no screen pages yet)";
/// Added to the width line when the stored value is wider than the terminal. `{terminal}` is
/// the terminal's width. The value is kept; it applies again in a wider window.
pub const CLAMPED_WIDTH: &str = " (your terminal is {terminal} wide, so {terminal} is used)";
/// Like [`CLAMPED_WIDTH`] for the height; `{terminal}` is the terminal's height.
pub const CLAMPED_HEIGHT: &str = " (your terminal is {terminal} tall, so {terminal} is used)";
/// `{value}` is the tier name, or [`NOT_CHOSEN`].
pub const SETTING_ICONS: &str = "Icons: {value}";
/// Value text for "follow the terminal".
pub const AUTOMATIC: &str = "automatic";
/// Value text for no tier chosen.
pub const NOT_CHOSEN: &str = "not chosen";
/// On.
pub const ON: &str = "on";
/// Off.
pub const OFF: &str = "off";
/// Settings menu labels.
pub const MENU_WIDTH: &str = "Set width";
/// See [`MENU_WIDTH`].
pub const MENU_HEIGHT: &str = "Set height";
/// See [`MENU_WIDTH`].
pub const MENU_COLOR: &str = "Toggle color";
/// See [`MENU_WIDTH`].
pub const MENU_PAUSE: &str = "Toggle pause";
/// See [`MENU_WIDTH`].
pub const MENU_ICONS: &str = "Run the icon check again";
/// See [`MENU_WIDTH`].
pub const MENU_BACK: &str = "Back";
/// Settings footer.
pub const SETTINGS_FOOTER: &str = "Esc: back   Ctrl-C: log off";
/// Label of the number field.
pub const FIELD_WIDTH: &str = "Width, 20 to 500 (empty = automatic)";
/// Label of the number field.
pub const FIELD_HEIGHT: &str = "Height, 6 to 200 (empty = automatic)";
/// Footer while typing a number.
pub const EDIT_FOOTER: &str = "Enter: save   Esc: cancel";

/// Notice: width out of range. Keep in step with `MIN_WIDTH`/`MAX_WIDTH` (a test checks).
pub const ERR_WIDTH: &str = "Width must be from 20 to 500.";
/// Notice: height out of range. Keep in step with `MIN_HEIGHT`/`MAX_HEIGHT` (a test checks).
pub const ERR_HEIGHT: &str = "Height must be from 6 to 200.";
/// Notice: saved.
pub const SAVED: &str = "Saved.";

/// Shown when the account no longer holds `account.edit_settings`: the change applies on the
/// next action, so the page the person was on closes instead of pretending to save.
pub const NOT_PERMITTED: &str = "You are not allowed to change these settings.";
/// Notice: tier chosen.
pub const CHOSE_NERD: &str = "Icons set to Nerd Font.";
/// Notice: tier chosen.
pub const CHOSE_UNICODE: &str = "Icons set to Unicode.";
/// Notice: fallback chosen.
pub const CHOSE_ASCII: &str =
    "Using plain ASCII icons and no color. Change this any time under Terminal settings.";

/// Tier display names.
pub const TIER_NERD: &str = "Nerd Font";
/// See [`TIER_NERD`].
pub const TIER_UNICODE: &str = "Unicode";
/// See [`TIER_NERD`].
pub const TIER_ASCII: &str = "ASCII";

/// Logoff heading.
pub const LOGOFF_HEADING: &str = "Logged off";
/// `{handle}` is the sanitized account handle.
pub const GOODBYE: &str = "Goodbye, {handle}.";
/// Logoff body.
pub const SETTINGS_SAVED_NOTE: &str = "Your terminal settings are saved.";

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
