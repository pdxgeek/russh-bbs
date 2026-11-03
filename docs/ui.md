# Terminal UI: menus, layouts, themes, icons

Status: **Partly implemented.** The menu tree, classic hotkey layout, universal keys and string table exist (M2-A, see "Implemented / Verified"); lightbar, compact, themes and the other layouts are M3.

## Four separate layers

| Layer | Controls | Example |
|---|---|---|
| Menu tree | What exists and where it leads | Main, Forums, Files, Mail, Doors |
| Layout | How a menu is arranged | Hotkey list, lightbar, two-pane, tabs, dashboard, command line, compact |
| Theme | How it looks | Colors, borders, art |
| Icon set | Which glyphs are drawn | Nerd Font, plain Unicode, ASCII |

The menu tree is defined once. Any layout renders it and any theme styles it. Adding a layout must not touch menu logic.

## Layouts

Ship first: **classic hotkey**, **lightbar**, **compact**. Add **two-pane** (menu plus live preview), **tabs/ribbon**, **dashboard** (widgets), and **command line** after the widget set exists.

- Layouts are **declarative TOML**: regions (header, menu, preview, status, footer) and widgets. No scripting, validated schema, size limits.
- Widgets are Rust code in the server crate (menu list, lightbar, breadcrumb, clock, new-counts, art block, ticker).
- Each layout declares a minimum terminal size and a fallback. Too small falls back automatically.
- Members choose layout and theme independently. The sysop sets the default and may restrict the list. A picker previews against the real menu tree.
- Hot reload with validation; a bad file is rejected with a clear error and the default stays.
- Universal keys (help, back, quit) are identical in every layout so switching never strands someone.

## Themes and strings

Colors, borders, art, login and logoff screens, and **every user-facing string** are themeable. Packs bundle layout, theme, and art; keep each pack's license and attribution files with it.

## Icons

- A single **icon registry** in core maps logical names (`board`, `restricted`, `unread`, `feed`, `mail`, `invite`) to a glyph for each of three tiers: **Nerd Font**, **plain Unicode**, **ASCII/CP437**. UI code never contains a raw codepoint.
- The font is a client-side setting the server cannot detect, so the first login shows a **glyph test screen** ("do these look like icons?") and stores the answer per user. `$TERM` and locale are weak defaults only.
- Many Nerd Font glyphs have ambiguous width. Keep a width table in core and test alignment in more than one terminal.
- Web UIs over the GraphQL API receive icon **names**, not codepoints.

## Security chrome (fixed)

The server-readable marker, the (future) E2EE lock, signature badges, and the key-changed warning are system chrome:
- Layouts may place them but cannot hide, restyle misleadingly, or replace them.
- Every icon tier has a text fallback.
- Every marker is drawn with a **reserved style** (bold + reverse video) that content cannot emit, because the sanitizer strips all escape sequences. The renderer never applies that style to sanitized content.
- Unicode-tier markers are drawn inside the reserved delimiters `⟦ ⟧`. The sanitizer replaces those exact codepoints and the four marker glyphs in all content; Nerd Font glyphs are private-use and replaced likewise. **Lookalikes are not blocked** (`〚☉〛`, `◉`, fullwidth brackets), so on a terminal without styling the Unicode tier also depends on the chrome-only position (D45).
- **ASCII-tier markers cannot be made unforgeable by text** (content can type `E2EE`). Draw them only in a chrome-only position (status line or fixed gutter). On a no-color terminal that position is the only cue, so a screen that cannot guarantee one must say "security markers unavailable" instead of drawing them. See D39.

## Message editor

The full-screen editor the forum, mail and poll screens call to compose text (M3-B, D48, D80). It is `russh-bbs-server::ui::editor`, built on `tui-textarea-2`; drafts, bounds and quoting are `russh-bbs-core::drafts`. Status: implemented and tested; **no feature screen calls it yet** (the first caller is M3-E).

### Calling it

The editor is a plain state machine. The host session feeds it keys, draws it every frame and does the store work its effects ask for, as the other screens do.

```text
let request = EditorRequest::new(title)             // title: Sanitized, one line
    .context(ContextKey::parse("forum:reply:12")?)  // optional; enables drafts
    .initial(quote(Some(&who), &post, QuoteLimits::default())) // optional, Sanitized
    .limits(BodyLimits::new(4_000, 100));           // optional; narrower than the board-wide cap only
let mut editor = Editor::open_with_store(&store, account, request).await?; // resumes a saved draft
match editor.input(key) {                           // key: ssh::terminal::Key
    EditorEffect::Stay => {}
    EditorEffect::Done(Outcome::Submitted(body)) => { /* write it, THEN discard_draft */ }
    EditorEffect::Done(Outcome::Cancelled) => {}    // nothing sent, nothing stored changed
    EditorEffect::SaveDraft(save) => {              // the member chose "save draft"
        let saved = save.persist(&store, account, now).await;
        editor.draft_saved(saved);                  // Done(Outcome::SavedDraft), or back to editing
    }
    EditorEffect::LogOff(save) => { /* persist `save` if Some, then end the session */ }
}
editor.render(frame, &EditorView { ui, settings, markers });
```

- `Editor::open(request, saved_draft)` is the same without the store; it errors (`BodyError`) if the starting text or draft does not fit the limits, rather than cut a member's text.
- `Submitted(Body)` carries the sanitized, in-bounds text. The draft stays stored until the caller calls `drafts::discard_draft(store, account, editor.context())` after its own write succeeded.
- A saved draft for the context replaces the starting text (a quote) and the screen says so. Saving blank text removes the draft.
- Quoting: `drafts::quote(attribution, text, QuoteLimits)` takes and returns `Sanitized`. Each line gets `> `; lines already two deep are dropped; the result ends with a blank line to reply on. A quote mark is ordinary text anyone can type: it says nothing about authorship and is never security chrome. The editor draws quote lines dim, never in the reserved marker style.
- Limits: at most 16,000 characters and 400 lines whatever a caller asks (`BodyLimits::MAX`); at most 20 drafts per member (`MAX_DRAFTS_PER_MEMBER`). An edit that would pass a limit is undone and a `Problem:` line says so. Text is sanitized by the shared multi-line sanitizer on save and read; over-long text is refused, not cut.
- The editor says nothing about who can read a message or a draft (D78), shows no lock, and draws no security marker of its own: the host passes the markers for the reserved status row.

### Keys the editor captures

The key parser delivers no arrow keys, Home, End, Delete, PageUp/PageDown, non-ASCII characters or pasted line breaks yet, so the editor uses Ctrl chords (Tab is Ctrl-I). Ctrl-G lists them on screen.

| Key | Does |
|---|---|
| Printable ASCII, Enter, Tab, Backspace | type, new line, spaces to the next tab stop of 4, delete before the cursor |
| Ctrl-X | send |
| Esc | leave. With no changes: leaves at once (`Cancelled`). With changes: asks `[s] save draft` (only with a context key), `[d] discard changes`, `[c] keep editing`. Any other key keeps editing |
| Ctrl-C, Ctrl-D | log off (the universal key, D54). The host first stores the draft if the text changed and there is a context key |
| Ctrl-G | key list; any key returns without typing |
| `?` | a question mark (help is Ctrl-G) |
| Ctrl-B, Ctrl-F, Ctrl-P, Ctrl-N, Ctrl-A, Ctrl-E | left, right, up, down (by screen row), line start, line end |
| Ctrl-T, Ctrl-V | page up, page down |
| Ctrl-W, Ctrl-K, Ctrl-Y | delete word before; delete to end of line (kept); put it back |
| Ctrl-Z, Ctrl-R | undo, redo |
| every other key | ignored |

The pause setting (a pager) does not apply: the editor scrolls with the cursor. A window change keeps the text and the cursor; a window under 26 columns or 9 rows says so in words and keeps the text.

## Accessibility

A compact, text-only layout with no icons, no art, and screen-reader-friendly output is selectable from any screen and is the fallback whenever the terminal cannot be probed.

## Implemented / Verified

**Waiting room** (`russh-bbs-core::waiting_room` rules and view model, `russh-bbs-server::waiting_room` drawing), M1-C.

- **Implemented:** the page set per registration mode, menu keys, form input (Enter next/send, Backspace, Esc back, Ctrl-C leave), and the drawing: a bordered box with a heading, the banner, menu or form, a `Notice:` / `Problem:` line (words, not only colour) and a footer hint. Every fixed string is in `waiting_room::strings`, not in drawing code, so a theme or translation can replace them later. Banner and rules come from settings and are sanitized.
- **Security cues:** the rules page carries a fixed privacy line ("the server can read everything except PGP-encrypted mail and files you encrypt yourself") that settings cannot edit; no lock or privacy glyph is shown anywhere in the waiting room.
- **Not implemented:** theme and layout selection for these screens (strings are themeable by design, the layer is M3); scrolling or paging of long rules or banner text (the text is capped instead, D64: banner 300 characters and 5 lines, rules 900 characters and 10 lines, a cut ends with an ellipsis, and the renderer keeps the menu and footer on screen however long a view is); non-ASCII typing; the real request, redeem, apply and create-account flows (M2).
- **Verified:** rendering of every page of every mode into a ratatui test backend; a real russh client and OpenSSH walking the screens. Not verified in other terminal emulators.

**Icon registry** (`russh-bbs-core::icons`), implemented and unit tested; the only front end that uses it is the M1-E glyph test (below).

- Logical names are closed enums: `ContentIcon` (`Board`, `Restricted`, `Unread`, `Feed`, `Mail`, `Invite`) and `SecurityMarker` (`ServerReadable`, `E2eeLock`, `Signature`, `KeyChanged`), joined by `Icon`. Each icon is one exhaustive `match` arm (no wildcard) giving Nerd Font, plain Unicode and ASCII glyphs plus a text label, so an icon missing a tier does not compile.
- Tests: every icon non-empty in all tiers; ASCII tier is printable ASCII; Unicode tier has no private-use characters; Nerd Font glyphs are exactly private-use and the sanitizer replaces every one; a property test proves no sanitized string reproduces the reserved style, delimiters, glyphs or Nerd Font marker glyphs, another pins that ASCII text is forgeable, and another that lookalike brackets and glyphs are not blocked; glyphs are unique per tier; no raw private-use character exists in any workspace source file (registry glyphs are `\u{..}` escapes).
- **Security chrome in the type system.** `SecurityMarker` is a separate type: layout and theme APIs that accept `ContentIcon` cannot be given one, and `Rendered::is_security_chrome()` is true for them in every tier, so a renderer must not hide, replace or restyle them. `Rendered` exposes `to_terminal()` (styled), `plain()` and `chrome_style()`; there is no bare printable string for a marker. Their ASCII tier is the honest label (`SERVER-READABLE`, `E2EE`, `SIGNED`, `KEY-CHANGED`), distinguished only by style and position. The `Restricted` icon (access restriction, renamed from `Lock`) is a different glyph from `E2eeLock` in every tier and is not a privacy claim. **Not yet implemented:** a renderer that applies the style and a chrome-only position (cross-slice request to the M2 screen framework).
- **Widths.** `width` holds an explicit 1-or-2 cell entry for every non-ASCII character used. Nerd Font glyphs reserve 2 cells and `render` appends one space (correct whether the terminal draws 1 or 2 cells); plain-Unicode glyphs are East Asian Width Neutral, 1 cell. A glyph with no entry renders as its ASCII tier. See D18.
- **Not verified:** alignment in real terminals (multiple emulators and fonts); Nerd Font codepoints against a specific patched font release. The glyph test screen (M1-E) sends the registry glyphs (checked by codepoint over OpenSSH), but no one has looked at them in a Nerd Font terminal.

**Terminal settings, glyph test, login and logoff** (`russh-bbs-core::terminal_settings` rules and screens, `russh-bbs-server::settings` drawing), M1-E, D42.

- **Implemented:**
  - Per-account settings: width and height overrides (default: follow the PTY), color, pause (default on), icon tier. No stored tier means the glyph test has not been answered.
  - **The safe default is the accessible layout:** ASCII icons, an ASCII `+-|` border and no color, until a tier is stored, and again if the glyph test is skipped (Esc on the first run) or answered "no" to every style. Color on or off is also always written in words (`Notice:` / `Problem:` prefixes), never color alone.
  - **Glyph test** on first login: shows the six content icons for one tier from the registry and asks "Do these look like icons?"; `y` keeps it, `n` goes to the next tier, Esc skips. It is re-runnable from the settings screen (`i`), where Esc cancels. `$TERM` and locale are not used.
  - **Login screen** (signed in as the sanitized handle, last login in UTC or "first login", the stored tier, color and pause), the **main menu** (below; it was a placeholder home in M1-E), the **settings screen** (`w`/`h` size with range-checked numbers and empty = automatic, `c` color, `p` pause, `i` icon check, Esc back) and the **logoff screen**, which is also printed on the main screen after the alternate screen is left.
  - Every fixed string is in `terminal_settings::strings`; nothing shows a lock or the word "private".
- **Security chrome:** no security marker is drawn on these screens. When one is needed it goes through `Rendered::to_terminal()` / `chrome_style()` in a chrome-only position (D39), not through the content styles in `settings::render`.
- **Pause and size (D64, D65):** `core::terminal_settings::Pager` pages long text by the pause setting and `ui::pager` draws it (pause on: "More (n of m)" pages, Space/Enter next, `b` back, Esc stop; pause off: the first screenful and a line saying how many lines are not shown). A width or height override larger than the real terminal is cut down to it, and the settings screen says so ("your terminal is N wide, so N is used"); the stored value is kept.
- **Not implemented:** no screen has text long enough to page, so pause still changes no screen today (the pager is built and tested, not wired; the message reader in M3 is the first user); the tier changes only the border and the glyph test today, because there are no other icons on screen; layout and theme selection, and the settings screen's strings in the string table (M2-A moved only the menu's); a color check screen; per-terminal (rather than per-account) overrides. The blind recovery for a size override, `s`, `w`, Enter, `h`, Enter, still works and is tested.
- **Verified:** unit tests (rules, glyph answers, fallback, the whole flow, the string filler, the UTC formatter), render tests on a ratatui test backend (no color cells with color off, ASCII border for the ASCII tier, registry glyphs on screen, tiny and huge windows), store tests through the core traits, a real russh client (claim, first-login glyph test, stored answer, reconnect without it, Esc fallback, settings persistence, resize storm, an unknown key still sees only the waiting room), and OpenSSH under `pyte` and `tmux`.
- **Not verified:** other terminal emulators; a Nerd Font terminal; a screen reader.

**Screen framework** (`russh-bbs-core::menu` and `russh-bbs-server::ui`), M2-A, D54.

- **Menu tree** (`core::menu`): static data, defined once (`main_tree()`): Main (Forums, Mail, Files, Members as honest placeholders; Terminal settings; Sysop tools for a session holding `members.approve`; Log off) and a Sysop submenu (M2-E: its one item, *Invite requests*, is now the sysop queue screen, and *Members* gained *Vouch for a request* for sessions holding `members.vouch`; both are `Action`s drawn by `server::sysop::queue` through `ui::classic::render_content`, with the requester's text in the content box only). Validated (duplicate or non-alphanumeric hotkeys, dangling targets, empty, duplicate or unreachable menus). Which items a session sees comes from one hook, `core::access::PermissionCheck`; the visible items are also the only pickable ones.
- **Classic hotkey layout** (`ui::classic`): a bordered box, `[k] Label` lines, a notice line that carries the word `Notice` or `Problem`, a footer of the universal keys, and help drawn by the same layout. Items for areas that do not exist say `(not built yet)` and picking one says so again.
- **Universal keys:** `?` help, Esc back (nothing at the top), Ctrl-C and Ctrl-D log off. They work in every menu, including submenus (tested). Items cannot take `?`. Help and Back are menu-only; the login, icon check and number entry take typed characters, and the settings screen is not yet a menu.
- **String table** (`ui/strings.toml`, loader and validation in `ui::strings`): every string of the menu screens, by dotted key. Boot fails, naming the key, if one is missing, unknown, empty, not text, over 240 characters, or contains control, invisible, private-use or reserved marker characters. A sysop override file is not wired (it needs a config key).
- **Security chrome:** the last row of the screen is reserved. The layout removes it from the area before laying out content, clears it, and draws markers there with the reserved style (`Rendered::chrome_style()`, bold + reverse); if a marker does not fit the width it says "security markers unavailable"; on a one-row terminal no marker is drawn. The menu screens show the server-readable marker with "The server can read what you do here." The other screens draw no markers. Test: hostile content (2,000-character lines, 200 lines, text and lookalikes of markers) at five sizes in all three tiers leaves the status row identical to an empty screen and puts the reserved style on no other cell.
- **Not implemented:** lightbar, compact and the other layouts, themes, declared widest layout per screen (FIX-M1-4 request), a pager wired to a screen (the pager itself exists, D65), the settings/glyph/login/waiting-room strings in the table, `tui-input` and `tui-big-text` (evaluated, not needed yet).
- **Verified:** unit tests in core and server, a real russh client, and OpenSSH under `pyte` at 80x24 and 200x60 (claim, icon check, main menu, settings, Esc back, help, placeholder notice, sysop submenu, Back item, log off). **Not verified:** other terminal emulators; a Nerd Font or screen-reader session.

**Message editor and drafts** (`russh-bbs-server::ui::editor`, `russh-bbs-core::drafts`), M3-B, D80 (and D48).

- **Implemented:** see [Message editor](#message-editor). A word-wrapped `tui-textarea-2` editor in a full-width box (title, line and character counters on the bottom border, a `Notice:`/`Problem:` line, a key line), the reserved status row beneath through `chrome::render_status`, its own strings in `ui::editor::strings`. Drafts: migration `0012_drafts` on SQLite and Postgres, `DraftStore` on both, the 20-per-member cap checked and written in one transaction.
- **Security chrome:** the editor draws no marker itself. Tests (all three icon tiers, color on and off, sizes from 1x1 to 200x60, hostile text with lookalikes and marker words) prove the status row is identical to an empty editor's and that no cell above it carries bold + reverse; the cursor is reverse + underline and quote lines are dim.
- **Not implemented:** arrow keys, Home/End/Delete, PageUp/PageDown, non-ASCII typing and pasted line breaks (the key parser, cross-slice request in [coordination.md](coordination.md)); draft expiry; a menu entry or any feature screen; the editor's strings in the string table (M3-A).
- **Verified:** core unit tests; the shared store suite for drafts on SQLite, Postgres 16 and TLS-required Postgres 16 (cap under concurrency, per-member and per-context isolation, sanitizing in the stored row, `CHECK`s, cascade on account delete, a broken store errors); 32 editor tests (keys, limits including a full 16,000-character message, leave and draft flow against a real SQLite store, drawing, wide characters, resize, scrolling, hostile text); and **real OpenSSH 9.6 under `pyte` (36 checks)** against the test-only example `crates/server/examples/editor_demo.rs` (`cargo run -p russh-bbs-server --example editor_demo`; loopback only, accepts every key, not part of the binary or the image): type and send, reply with a quote, save a draft with Esc and `s`, resume it on reconnect, log off with Ctrl-C and resume, a send that discards the draft, character and line limits with their `Problem:` line, resize from 80x24 to 40x12, 20x5 and 120x40 with the text and cursor kept, 60 lines scrolled with Ctrl-T, an arrow key not mistaken for Esc, the key list, and the marker row's reserved style appearing only on the last row.
- **Not verified:** other terminals, a screen reader, a Nerd Font session, a feature screen (none exists), the editor on a slow link (the example repaints the whole screen per key; the real screen sends diffs).
