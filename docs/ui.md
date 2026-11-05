# Terminal UI: menus, layouts, themes, icons

A UI refresh and board customization plan (owner approved, 2026-11-04) is in [ui-refresh.md](ui-refresh.md).

Status: **Partly implemented.** UI-4 (the screens: reader, three-pane chat, mail, community, sortable list tables, the split layout's chat and mail panes, the blinking cursor; [below](#screens-ui-4-d127)) moves the long-text and live screens onto the frame. UI-2 (style packs, seven layouts, board identity, member choices; [below](#styles-layouts-and-board-identity-ui-2-d117)) is built on UI-1. UI-1 (the frame, tables, popups and colour depth, [below](#frame-tables-popups-and-colour-depth-ui-1-d109)) is the foundation the rest of M3.5 builds on. The forum screens (M3-E, below) are the first long-text screens. The menu tree, universal keys and string table (M2-A) and the **classic, lightbar and compact layouts with theme packs and live switching** (M3-A, D82) exist; two-pane, tabs, dashboard and command line, declarative layout files and art blocks do not.

## Four separate layers

| Layer | Controls | Example |
|---|---|---|
| Menu tree | What exists and where it leads | Main, Forums, Files, Mail, Doors |

**Doors (M4-D0, D98).** The main menu's `d` (shown with `doors.run`) opens the door list; Enter or the door's number opens it; Esc goes back. A door is drawn inside the same box and reserved status row as every other screen, so on telnet the `UNENCRYPTED` footer row is outside what the door is given. **A running door's box and colour (FIX-UI-2a, D121).** The door is composited at the member's effective colour depth (`Ui::look(..).depth()`: the client's, lowered by the member's cap), so truecolor and 256 colours reach clients that have them and 16, mono and the ASCII tier degrade as before; the per-cell cleaning is unchanged. The box follows the layout: the Focus layout gives a door the whole body (no margin, no width limit); every other layout gives it up to 132 columns (`DOOR_MAX_WIDTH`), centred, instead of the reading box's 76 (74 inside). The border is the only chrome inside the box's rectangle, and any status row (the marker cell) is split off before the box is sized. The door list and the opening screen keep the reading box. The door list's own sentences are fixed English for now (not in the theme string table); the menu label is. Ctrl-C and Ctrl-D log off from inside a door.
| Layout | How a menu is arranged | Hotkey list, lightbar, two-pane, tabs, dashboard, command line, compact |
| Theme | How it looks | Colors, borders, art |
| Icon set | Which glyphs are drawn | Nerd Font, plain Unicode, ASCII |

The menu tree is defined once. Any layout renders it and any theme styles it. Adding a layout must not touch menu logic.

## Layouts

Ship first: **classic hotkey**, **lightbar**, **compact**. Add **two-pane** (menu plus live preview), **tabs/ribbon**, **dashboard** (widgets), and **command line** after the widget set exists.

- Layouts are **declarative TOML** (planned): regions (header, menu, preview, status, footer) and widgets. No scripting, validated schema, size limits. **Not built:** the three shipped layouts are Rust code (D82); a layout file format waits for a second layout author.
- Widgets are Rust code in the server crate (menu list, lightbar, breadcrumb, clock, new-counts, art block, ticker).
- Each layout declares a minimum terminal size and a fallback. Too small falls back automatically (built, M3-A: lightbar 44x12 to classic, classic 24x9 to compact).
- Members choose layout and theme independently (built, M3-A: keys `l` and `t` on the settings screen, applied at once). The sysop sets the default and may restrict the list, and a picker previews against the real menu tree (**not built**: the default is classic and every loaded theme is offered).
- Validation; a bad file is rejected with a logged warning and the default stays (built for themes, M3-A). **Hot reload is not built**: themes load at boot.
- Universal keys (help, back, quit) are identical in every layout so switching never strands someone.

## Themes and strings

Colors and **every user-facing string except the security-status captions** are themeable (M3-A). Borders follow the icon tier (the ASCII tier draws `+ - |`). Art, login and logoff art, and packs that bundle layout, theme and art are **not built**; keep each pack's license and attribution files with it when they are.

A **theme** is one TOML file: `name`, an optional `[colors]` table (roles `heading`, `key`, `border`, `info`, `error`, `selected`; values from the sixteen named terminal colors) and string overrides in the same nested form as `ui/strings.toml`. A theme inherits every string it does not set. Three ship embedded (`amber`, `green`, `contrast`); the sysop adds files to `themes/` beside the host key. See the loader rules in D82 and the Implemented section below.

## Icons

- A single **icon registry** in core maps logical names (`board`, `restricted`, `unread`, `feed`, `mail`, `invite`) to a glyph for each of three tiers: **Nerd Font**, **plain Unicode**, **ASCII/CP437**. UI code never contains a raw codepoint.
- The font is a client-side setting the server cannot detect, so the first login shows a **glyph test screen** ("do these look like icons?") and stores the answer per user. `$TERM` and locale are weak defaults only.
- Many Nerd Font glyphs have ambiguous width. Keep a width table in core and test alignment in more than one terminal.
- Web UIs over the GraphQL API receive icon **names**, not codepoints.

## Security chrome (fixed)

The transport markers (`UNENCRYPTED`, `LOW-SECURITY`), the (future) E2EE lock, signature badges, and the key-changed warning are system chrome:
- Layouts may place them but cannot hide, restyle misleadingly, or replace them.
- Every icon tier has a text fallback.
- Every marker is drawn with a **reserved style** (bold + reverse video) that content cannot emit, because the sanitizer strips all escape sequences. The renderer never applies that style to sanitized content.
- Unicode-tier markers are drawn inside the reserved delimiters `⟦ ⟧`. The sanitizer replaces those exact codepoints and the four marker glyphs in all content; Nerd Font glyphs are private-use and replaced likewise. **Lookalikes are not blocked** (`〚☉〛`, `◉`, fullwidth brackets), so on a terminal without styling the Unicode tier also depends on the chrome-only position (D45).
- **ASCII-tier markers cannot be made unforgeable by text** (content can type `E2EE`). Draw them only in a chrome-only position (status line or fixed gutter). On a no-color terminal that position is the only cue, so a screen that cannot guarantee one must say "security markers unavailable" instead of drawing them. See D39.

## Message editor

The full-screen editor the forum, mail and poll screens call to compose text (M3-B, D48, D80). It is `russh-bbs-server::ui::editor`, built on `tui-textarea-2`; drafts, bounds and quoting are `russh-bbs-core::drafts`. Status: implemented and tested; the first caller is the poll form (M3-J), which opens it for a poll's description **without a draft context**. The wiring is in `ssh/screen.rs`: `Screen.editor` takes every key while it is open and is drawn instead of the content; it sits in a `Mutex` only because the widget's caches are `Cell`s and the session future must be `Send` (it is reached with `get_mut`, so nothing waits). The forum (M3-E) and mail (M3-I) screens each hold their own editor inside their own state (`Content::Forums`, `Content::Mail`) and do not use this slot; later callers can reuse it: open it with `Screen::open_editor`-style code and hand the finished `Body` to their own flow.

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

The key parser delivers the arrows (CSI and SS3), Home, End, Delete, PageUp and PageDown (FIX-M3-1, D88), and a pasted line break as `Key::PasteBreak`, which becomes a new line only here. Non-ASCII characters are not delivered yet. The Ctrl chords below work on every terminal too (Tab is Ctrl-I). Ctrl-G lists the keys on screen.

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
| Arrow keys, Home, End, Delete, PageUp, PageDown | move; line start and end; delete the character under the cursor; scroll a screen |
| Bracketed paste | printable text is typed and each line break becomes a new line (editor only; D88) |
| Ctrl-W, Ctrl-K, Ctrl-Y | delete word before; delete to end of line (kept); put it back |
| Ctrl-Z, Ctrl-R | undo, redo |
| every other key | ignored |

The pause setting (a pager) does not apply: the editor scrolls with the cursor. A window change keeps the text and the cursor; a window under 26 columns or 9 rows says so in words and keeps the text.

## Chat screen (M3-F)

The chat screen ([chat.md](chat.md)) is an overlay on the member's session, drawn in the classic content box (`render_content`) so the member's theme, icon tier and the reserved status row apply in every layout. The room page is a heading, the newest transcript rows (wrapped to the box), a hint or notice line and the input line, always exactly the box's height (`chat::box_height` mirrors `settings::boxed`'s margins). Other people's text is cleaned again when drawn, appears only in the content rectangle, never in the marker style and never as a label; a whisper is marked by the words "whispers" / "you whisper to", not only by color. A new line redraws the screen without a key press: see "The push path" in chat.md.

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
- **Security chrome in the type system.** `SecurityMarker` is a separate type: layout and theme APIs that accept `ContentIcon` cannot be given one, and `Rendered::is_security_chrome()` is true for them in every tier, so a renderer must not hide, replace or restyle them. `Rendered` exposes `to_terminal()` (styled), `plain()` and `chrome_style()`; there is no bare printable string for a marker. Their ASCII tier is the honest label (`SERVER-READABLE` (registered, not drawn: D93), `E2EE`, `SIGNED`, `KEY-CHANGED`), distinguished only by style and position. The `Restricted` icon (access restriction, renamed from `Lock`) is a different glyph from `E2eeLock` in every tier and is not a privacy claim. **Not yet implemented:** a renderer that applies the style and a chrome-only position (cross-slice request to the M2 screen framework).
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
- **Not implemented:** no screen has text long enough to page, so pause still changes no screen today (the pager is built and tested, not wired; the message reader in M3 is the first user); the tier changes only the border and the glyph test today, because there are no other icons on screen; layout and theme selection (M3-A built it; the settings screen's strings moved into the table too); a color check screen; per-terminal (rather than per-account) overrides. The blind recovery for a size override, `s`, `w`, Enter, `h`, Enter, still works and is tested.
- **Verified:** unit tests (rules, glyph answers, fallback, the whole flow, the string filler, the UTC formatter), render tests on a ratatui test backend (no color cells with color off, ASCII border for the ASCII tier, registry glyphs on screen, tiny and huge windows), store tests through the core traits, a real russh client (claim, first-login glyph test, stored answer, reconnect without it, Esc fallback, settings persistence, resize storm, an unknown key still sees only the waiting room), and OpenSSH under `pyte` and `tmux`.
- **Not verified:** other terminal emulators; a Nerd Font terminal; a screen reader.

**Screen framework** (`russh-bbs-core::menu` and `russh-bbs-server::ui`), M2-A, D54.

- **Menu tree** (`core::menu`): static data, defined once (`main_tree()`): Main (Forums opens the forum screens, M3-E; Mail opens the mail screens, M3-I (`server::mail`, drawn like the forum screens through `ui::classic::render_content`, the reader through `ui::pager`, composing through the editor); Files is an honest placeholder; Members is a submenu; Terminal settings; Sysop tools for a session holding `members.approve`; Log off) and a Sysop submenu (M2-E: its one item, *Invite requests*, is now the sysop queue screen, and *Members* gained *Vouch for a request* for sessions holding `members.vouch`; both are `Action`s drawn by `server::sysop::queue` through `ui::classic::render_content`, with the requester's text in the content box only). Validated (duplicate or non-alphanumeric hotkeys, dangling targets, empty, duplicate or unreachable menus). Which items a session sees comes from one hook, `core::access::PermissionCheck`; the visible items are also the only pickable ones.
- **Classic hotkey layout** (`ui::classic`): a bordered box, `[k] Label` lines, a notice line that carries the word `Notice` or `Problem`, a footer of the universal keys, and help drawn by the same layout. Items for areas that do not exist say `(not built yet)` and picking one says so again.
- **Universal keys:** `?` help, Esc back (nothing at the top), Ctrl-C and Ctrl-D log off. They work in every menu, including submenus (tested). Items cannot take `?`. Help and Back are menu-only; the login, icon check and number entry take typed characters, and the settings screen is not yet a menu.
- **String table** (`ui/strings.toml`, loader and validation in `ui::strings`): every string of the menu screens, by dotted key. Boot fails, naming the key, if one is missing, unknown, empty, not text, over 240 characters, or contains control, invisible, private-use or reserved marker characters. A sysop override file is not wired (it needs a config key).
- **Security chrome:** the last row of the screen is reserved. The layout removes it from the area before laying out content, clears it, and draws markers there with the reserved style (`Rendered::chrome_style()`, bold + reverse); if a marker does not fit the width it says "security markers unavailable"; on a one-row terminal no marker is drawn. **D93:** the `SERVER-READABLE` row is retired and member screens over ssh draw no marker (amended by D109: the last row is now the status bar with the screen's keys, and its left marker cell stays empty when no marker applies) and what the server can read is explained in the docs. The row is reserved and drawn, in the reserved style, only where a marker applies: `LOW-SECURITY` on staff screens while telnet is on. Telnet's `UNENCRYPTED` is the transport footer outside every layout. The `ServerReadable` variant stays in the registry, undrawn. If several markers do not fit with their captions, the row shows the markers alone (a marker is never clipped). Test: hostile content (2,000-character lines, 200 lines, text and lookalikes of markers) at five sizes in all three tiers leaves the status row identical to an empty screen and puts the reserved style on no other cell.
- **Not implemented:** the other layouts, declared widest layout per screen (FIX-M1-4 request), a pager wired to a screen (the pager itself exists, D65), the waiting-room strings in the table (the settings, glyph, login and logoff strings moved in M3-A), `tui-input` and `tui-big-text` (evaluated, not needed yet).
- **Verified:** unit tests in core and server, a real russh client, and OpenSSH under `pyte` at 80x24 and 200x60 (claim, icon check, main menu, settings, Esc back, help, placeholder notice, sysop submenu, Back item, log off). **Not verified:** other terminal emulators; a Nerd Font or screen-reader session.

**Message editor and drafts** (`russh-bbs-server::ui::editor`, `russh-bbs-core::drafts`), M3-B, D80 (and D48).

- **Implemented:** see [Message editor](#message-editor). A word-wrapped `tui-textarea-2` editor in a full-width box (title, line and character counters on the bottom border, a `Notice:`/`Problem:` line, a key line), the reserved status row beneath through `chrome::render_status`, its own strings in `ui::editor::strings`. Drafts: migration `0012_drafts` on SQLite and Postgres, `DraftStore` on both, the 20-per-member cap checked and written in one transaction.
- **Security chrome:** the editor draws no marker itself. Tests (all three icon tiers, color on and off, sizes from 1x1 to 200x60, hostile text with lookalikes and marker words) prove the status row is identical to an empty editor's and that no cell above it carries bold + reverse; the cursor is reverse + underline and quote lines are dim.
- **Not implemented:** non-ASCII typing (the key parser, cross-slice request in [coordination.md](coordination.md)); draft expiry; a menu entry or any feature screen; the editor's strings in the string table (M3-A).
- **Verified:** core unit tests; the shared store suite for drafts on SQLite, Postgres 16 and TLS-required Postgres 16 (cap under concurrency, per-member and per-context isolation, sanitizing in the stored row, `CHECK`s, cascade on account delete, a broken store errors); 32 editor tests (keys, limits including a full 16,000-character message, leave and draft flow against a real SQLite store, drawing, wide characters, resize, scrolling, hostile text); and **real OpenSSH 9.6 under `pyte` (36 checks)** against the test-only example `crates/server/examples/editor_demo.rs` (`cargo run -p russh-bbs-server --example editor_demo`; loopback only, accepts every key, not part of the binary or the image): type and send, reply with a quote, save a draft with Esc and `s`, resume it on reconnect, log off with Ctrl-C and resume, a send that discards the draft, character and line limits with their `Problem:` line, resize from 80x24 to 40x12, 20x5 and 120x40 with the text and cursor kept, 60 lines scrolled with Ctrl-T, an arrow key not mistaken for Esc, the key list, and the marker row's reserved style appearing only on the last row.
- **Not verified:** other terminals, a screen reader, a Nerd Font session, a feature screen (none exists), the editor on a slow link (the example repaints the whole screen per key; the real screen sends diffs).

**Layouts, themes and live switching** (`russh-bbs-server::ui::{layout, lightbar, compact, theme}`, `core::menu` cursor, `core::terminal_settings::{layout, strings}`), M3-A, D82.

- **Implemented:**
  - **Layouts** over the same `core::menu` page: *classic* (the M2-A box, default), *lightbar* (the same box with a `>` pointer row moved by Up and Down and picked with Enter; hotkeys still work; the cursor stays on the last item used, so Esc from a screen returns to it) and *compact* (no box, one line per item, no spacer lines, words only). `ui::layout::render_menu` is one exhaustive `match`; `effective` picks the fallback for a small terminal (lightbar to classic below 44x12, classic to compact below 24x9). Content screens (members, queue, invites, settings) use the same box, borderless under compact.
  - **Live switch:** the settings screen has `l` (next layout) and `t` (next theme), saved with the other settings under `account.edit_settings` and drawn on the next frame. The choice is two columns of `terminal_settings` (migration `0014`) and is there on the next login.
  - **Themes:** `ui::theme` (palette, string overrides, `ThemeSet`, loader). Shipped: `default`, `amber`, `green` (also rewords the main menu title and the key hint), `contrast`. The sysop's `themes/*.toml` beside the host key is read at boot, at most 32 files of at most 32 KiB, refused with a logged warning when invalid (see D82 for every check), never fatal.
  - **Strings:** the login, glyph test, settings and logoff screens' text is in `ui/strings.toml` under `[member]`; `core::terminal_settings::strings` is keys plus `TextSource`.
  - **Keys:** `ssh::terminal::Key` has `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `Delete`, `PageUp`, `PageDown` (CSI and SS3 forms, FIX-M3-1) and `PasteBreak`. The lightbar uses Up and Down only; the other keys do nothing outside the editor.
- **Security chrome:** unchanged and independent of layout and theme. The status row is split off first and drawn by `chrome::render_status` with the reserved style; the theme palette, string overrides and layout are not inputs to it, and its two captions are reserved string keys. The lightbar cursor uses a pointer, bold and underline, never reverse video.
- **Not implemented:** two-pane, tabs/ribbon, dashboard and command-line layouts; declarative layout files; art blocks, login and logoff art, packs; a sysop-set default layout or theme and a restricted list; a picker with a preview; hot reload of themes; RGB or indexed theme colors; a contrast check on theme colors; per-layout widest-width declarations (FIX-M1-4 request).
- **Verified:** unit tests (core: stored layout and theme text, theme names, cycling, the settings keys and their permission gate, cursor rules; server: every layout shows every item of the same menu, submenu and help in every layout, fallback table, every layout x shipped theme x tier x color x four sizes with hostile content keeps the marker in the reserved style in the status row and nowhere else, theme schema and every refusal, directory loader with good, bad, oversized, endless, duplicate and too many files, the settings screen fits 80x24 with its notice), the store suite on SQLite, on a plain Postgres 16 and on a TLS-required Postgres 16, a real russh client (live switch to lightbar, arrow keys and Enter, compact, theme steps, the choice remembered on reconnect, cursor keys inert in classic) and OpenSSH under `pyte` (the same walk at 80x24 and a resize storm to 40x10 and 30x8, the sysop's `plum` theme, bad theme files in the directory and the boot log). **Not verified:** other terminals or a screen reader; a Nerd Font terminal; the look of the shipped colors on a real terminal with a light background.

## Frame, tables, popups and colour depth (UI-1, D109)

Status: **Implemented and tested; walked over real OpenSSH 9.6 and a raw telnet client** (see Verified). Code: `crates/server/src/ui/{frame,table,popup,depth}.rs`. These are the reusable pieces UI-2 (styles and layouts), UI-5 (door canvas) and UI-10 (widgets) call; each has one entry point.

```text
 russh-bbs | Forums                                              UTC 14:05   <- header bar
 +- General --------------------------------------------------------------+
 |  #  Subject                    From         Posts  Last post            |
 |> 1. Welcome to the board       ann              3  2026-11-04 09:12 UTC #  <- table, pointer,
 |  2. Gardening                  bob              1  2026-11-03 18:40 UTC #     scrollbar
 +- Page 1 - 1-9 of 40 ---------------------------------------------------+
 [marker cell]  Enter read  1-9 jump  w write  n/b page  s search  Esc back   <- status bar
```

### The API

| Piece | Entry point | Notes |
|---|---|---|
| Frame | `FrameSpec::new(strings, look, tier).markers(..).place(..).clock(..).hints(..).draw(frame) -> Rect` | Draws the header bar and status bar and returns the **body**. `parts(area)` gives the split without drawing. `ChromeText::table(strings, key)` / `ChromeText::fixed("Esc")` are the only things a bar accepts. |
| Table | `table::render(frame, area, look, &TableSpec) -> TableView { top, shown }` | `Column::new(title, Width::{Fixed, Min, Fill})`, `.right()`, `.from_width(n)`; `TableRow::new(cells).with_detail(cell)`; `Cell::content(&str)` (sanitized when drawn), `Cell::plain`, `Cell::chrome`; `.styled(CellStyle)`. `selected: None` is a hotkey-driven list. |
| Popup | `popup::render(frame, body, look, &Popup { kind, title, lines, keys })` | `PopupLine::content` sanitizes; the popup is clamped inside `body`, so it cannot cover a bar. |
| Colour depth | `ColorDepth::detect(term, colorterm)`, `ColorDepth::effective(detected, color_on, cap)`, `Look::at_depth(depth)`, `Tint::resolve(depth)` | `Look::fg/bg(tint)` return nothing in mono. |

### Rules

- **Security markers.** The marker cell is the status bar's left cell. It is drawn by `chrome::status_cell` with the reserved style (bold + reverse) and no colour, in a rectangle no body widget is given. Nothing else in the frame, a table or a popup uses reverse video; the selected row is a pointer plus bold, underline and the theme's selection colour. If a marker does not fit, the cell says "security markers unavailable".
- **Sizes.** The status bar needs 2 rows. The header needs 12 rows and 40 columns and is not drawn in the compact layout or on the settings family of screens (those need their 22 rows at 80x24). Hints that do not fit are dropped whole, from the end.
- **Colour depth** comes from the client: over ssh `TERM` (the `pty-req`) and `COLORTERM` (an `env` request; the only variable accepted), over telnet the terminal type (RFC 1091). Truecolor draws 24-bit shades, 256 draws palette cells, 16 draws named colours, mono draws weight and underline only. The member's colour on/off setting lowers it (off is mono); a stored 16/256/truecolor cap is UI-2's column and `effective` already takes it. Named colours go out as `38;5;n` (crossterm's form); a terminal that reads only SGR 30-37 is not specially handled.
- **The clock** is drawn with the screen, not by a timer (D49).
- **Words, not only colour.** Popups carry `Notice`/`Problem`/`Confirm` in the title; the pointer and the header row read in mono and in the ASCII tier (plain `+ - |`, `#` scrollbar, `...` ellipsis).

### Converted screens

The main menu in the classic, lightbar and compact layouts (the menu is a ratatui `List` with a highlight and a scrollbar; help is two panes when the box is 64 columns wide); the forum board list, thread list and search results (tables; `1`-`9` jumps and paging use the table's window); forum errors and the remove confirmation (popups); the members list (a table of the page's members); the login, glyph test, settings, password and key screens (status bar only); the invite, community and queue screens (header and status bar). **Not converted in UI-1:** the forum reader, mail, chat, door and editor screens. The reader, mail, chat and community screens moved onto the frame in UI-4 ([below](#screens-ui-4-d127)); the door screens are UI-5's ratatui buffers and the editor keeps its own full-screen view.

### Verified

- **Unit tests** (`ui::{depth,frame,table,popup}`, `forums`, `members`, `ssh::screen`, `telnet::protocol`): the split at every size, hints dropped whole, the marker cell identical with hostile content and colourless at 80x24, 100x30 and 132x43 in all three tiers, every depth and every shipped theme; reverse video only on markers; popups clamped; table window, scrollbar, dropped columns, two-line rows, hostile cells, tiny areas; the forum lists with hostile names, subjects and snippets in every tier, layout, theme, depth and size; depth detection and fallback (a screen fed `xterm-256color` + `truecolor`, `xterm-256color`, `xterm`, `vt100` and colour off emits 24-bit, palette, named, and no colour); `LOW-SECURITY` on every screen of a staff session and on none of a member's; the telnet terminal-type option.
- **Real OpenSSH 9.6 against the real binary** (scratch board, founder claim, telnet on), read through `pyte` (cell attributes) and `tmux capture-pane`: the main menu, help, forum board list, thread list, search results, an error popup (duplicate board name), the members list and the sysop menu at **80x24, 100x30 and 132x43**; the bars' colours at four client depths (`TERM=xterm-256color` with `COLORTERM=truecolor` sent by `ssh -o SetEnv`: 24-bit; `xterm-256color` alone: palette cells; `xterm`: named colours; `vt100`: no colour) with the reserved style only on the marker cell; `LOW-SECURITY` in the marker cell on the first-run, menu, forum, members and sysop screens while telnet was on; a resize storm in `tmux` (40x10, 30x8, 20x5, 132x43, 44x12, 80x24) that kept the session, the table and the bars.
- **A raw-socket telnet client** that answers NAWS and the terminal-type request: it signs in with a password, the status bar is painted in 256-colour palette cells (from `TERM=xterm-256color`), the transport's `UNENCRYPTED` row stays below the status bar, and the forum lists, search and members list draw and resize (80x24 to 100x30).
- **Not verified:** other terminal emulators, a Nerd Font session, a screen reader, a light background, telnet at 132x43, a terminal that reads only SGR 30-37. `pyte` does not implement the alternate screen, so its first telnet frames show the login text under the menu; real terminals switch screens.

## Styles, layouts and board identity (UI-2, D117)

Status: **Implemented and tested; walked over real OpenSSH 9.6** (see Verified). Code: `crates/server/src/ui/{theme,backdrop,board,panes,layout}` and `settings::Look`; member settings in `crates/core/src/terminal_settings` (migration `0024_ui_settings`); the `[board]` section in [config.md](config.md#board-ui-2). Screenshots of every look are in [ui-screenshots/](ui-screenshots/README.md).

Style, layout and widgets are **independent choices**: any style works with any layout. A *style* is how it looks, a *layout* is how the panes are arranged.

### Palette roles

A style gives colours to 24 roles (`ui::theme::ROLES`). Each is a `#rrggbb` truecolor, a 0-255 index, one of the sixteen names, `"reset"` (the terminal's own) or `{ rgb = "#rrggbb", named = "lightred" }` to pick the sixteen-colour fallback (a plain hex colour gets the nearest of the sixteen). Every colour goes down through the depth fallback: truecolor, 256 cells, sixteen names, mono (weights and underline only).

| Group | Roles |
|---|---|
| Backgrounds | `surface` (the screen), `panel` (inside boxes), `popup`, `stripe` (every other table row), `head` (table header), `bar` (header and status bar) |
| Text | `text`, `muted`, `bar_text`, `title`, `heading`, `key` (hotkeys), `link`, `quote`, `timestamp` |
| Accents | `accent` (the board name, pointers; the board's `accent` replaces it), `accent2` (the end of a gradient), `border` |
| Selection | `selected` (text) and `select_bg` (the cursor row) |
| Notices | `info`, `success`, `warning`, `error` |

A file may set only the six original roles (`heading`, `key`, `border`, `info`, `error`, `selected`, the D82 form): the rest are derived, so older theme files keep their look. **There is no role for a security marker and no way to name one.** A style whose text cannot be read against the backgrounds it sets itself (WCAG contrast below 4.5 for text and bar text, 3 for headings, keys, notices and the accent, 2.5 for muted text, 2 for borders) is refused with the roles and the ratio named.

### Skin: how a style draws

`[style.*]` tables in the theme file choose from a fixed set of drawings. A skin has no way to say "bold", "reverse" or any attribute.

| Setting | Values |
|---|---|
| `[style.borders]` `frame`, `pane`, `popup` | `plain`, `rounded`, `double`, `thick`, `ascii`, `blocks`, `none` (the ASCII tier, or a member's "borders: ASCII", always draws `+ - |`; `blocks` is described [below](#the-blocks-border-fix-ui-7a)) |
| `[style.title]` `treatment`, `align` | `plain`, `caps`, `spaced`, `bracket` (`[ Title ]`), `rule` (`╡ Title ╞`), `angle` (`« Title »`); `left`, `center` |
| `[style.header]` `decoration`, `gradient` | `none`, `blocks` (`░▒▓ name ▓▒░`), `chevrons`, `diamonds`; a gradient board name from `accent` to `accent2` where the depth is 256 or truecolor (sixteen colours would band, so those draw the flat accent) |
| `[style.backdrop]` `kind` | `none`, `scanlines` (every other row a shade darker, over the finished screen), `stars`, `grid` (drawn behind the panels, which clear their own area) |
| `[style.motion]` `default` | a ceiling: `false` means this style never animates (the member's switch can only turn motion off further) |

Every decoration has an ASCII form, and backdrops are dropped on a mono terminal and in the ASCII tier.

### The six styles

| Style | Feel | Colours | Skin |
|---|---|---|---|
| `phosphor` | A green CRT | near-black glass, one phosphor hue, bright-white hotkeys | plain frame, thick popups, `[ bracket ]` titles, scanlines |
| `neon` | Synthwave | violet glass, magenta and cyan | rounded frame, double popups, `« angle »` titles, `» chevrons «` and a magenta-to-cyan gradient name, a faint grid |
| `paper` | Calm and light | ink on warm paper, no motion | rounded boxes, plain titles, no texture |
| `amber` | Amber terminal | warm amber on black | thick borders, CAPITAL titles |
| `deepspace` | Cold blues | deep blue glass, gold hotkeys | rounded panes, double popups, centred `S P A C E D` titles, `◆ name ◆`, a starfield, a blue-to-violet gradient name |
| `ansi` | A 1994 board | sixteen named colours only (identical at every depth), blue bars, cyan double borders | double borders everywhere, `╡ rule ╡` titles, `░▒▓ name ▓▒░` |

`default`, `green` and `contrast` (colour-only themes, D82) stay. The board ships them all; a sysop adds more as `*.toml` files in `themes/` beside the host key (same loader and limits as D82: 32 files, 32 KiB each, schema-checked, refused with a logged reason, never fatal). Art for each style is in [Board art](#board-art-ui-3-d123).

### The seven layouts

The **menu screen** is arranged in panes; every other screen draws in the body of the frame with the style's borders and colours (moving them into the panes is UI-4). Below a layout's minimum size it falls back, and **says so** in a dim line ("Showing Dashboard: this terminal is too small for Wide.").

| Layout | Arrangement | Needs | Falls back to |
|---|---|---|---|
| `classic` | A hotkey menu in a centred box | 24x9 | compact |
| `lightbar` | The same box with a cursor (Up, Down, Enter) | 44x12 | classic |
| `compact` | One column, no box, no header | none | |
| `dashboard` | Menu on the left, a grid of widget cells on the right (2x3 at 80x24, 3x4 at 132x43, fewer rows when short) | 72x20 | lightbar |
| `wide` | Navigation, a content pane (where you are, the signed-in line, the row under the cursor) and a right rail of widget cells | 110x22 | dashboard |
| `focus` | The menu as a roomy centred column with no box; `Tab` opens a widgets pop-over | 40x10 | lightbar |
| `split` | The menu beside a second pane with a divider | 72x16 | lightbar |

**Cells** are the places the member's widgets fill (UI-10, [Widgets](#widgets-ui-10); UI-2 had named slots with placeholders). A cell no widget fills is blank; **a pane never shows invented numbers.** The geometry is a pure function of the body rectangle (`panes::plan`), tested for overlap and containment from 24x9 to 500x198.

**Not built:** dragging the split divider (UI-9; the grip is drawn), the other screens inside the panes (UI-4), animated motion (the setting is stored and read as `Look::motion()`; UI-4 and UI-8 will use it).

### Board identity and limits (`[board]`)

The board's name replaces the string table's title in every style (header bar, box titles, member screens), and its tagline, location and "since" fill the identity widget. The text is the sysop's own, but it goes through the shared sanitizer (refused, not altered, if it would change) and the honesty check: **a board cannot be named to claim it is private, secure or encrypted.** `board.accent` replaces the style's accent. `board.style` and `board.layout` are what members who never chose get. `board.styles` and `board.layouts` limit what members may pick; **one entry locks the choice for everyone**, whatever they stored. `board.motion = false` turns motion off board-wide. All keys: [config.md](config.md#board-ui-2).

### Member choices

On the settings screen, saved per member and applied on the next frame: **layout** (`l`), **style** (`t`), **motion** (`m`), **colour depth ceiling** (`d`: auto, 16, 256, truecolor; it can only lower what the client advertises) and **borders** (`r`: the style's own, ASCII or blocks). A member who never picked a style or layout follows the board's default (the screen says "board default"); a stored choice the board no longer allows draws the board default. Stored in `terminal_settings` on both backends (migration `0024`, and `0027` for the `blocks` border value; layout and style are nullable, null meaning "follow the board").

### Rules that did not move

- **Markers.** The marker cell is the status bar's left cell, drawn by `chrome::status_cell` with the reserved style and no colour. The backdrop is painted above the status row, the status bar clears its own row, and the scanline pass skips the last row, so no style paints into the marker cell. Reverse video is on the marker and nowhere else, in every style, layout, depth and tier.
- **Content.** Member text is sanitized by the shared sanitizer wherever it is drawn. A style's title treatment only changes the case or spacing of chrome text (string table or fixed words) and of already-sanitized table titles.
- **ASCII tier and mono.** Every style and layout draws only ASCII in the ASCII tier and no colour in mono; tests check every cell.

### Verified

- **Unit and render tests** (`ui::{styles_tests,snapshots,resolve_tests,panes,board,theme}`, `config`): six styles that differ in palette and in at least two drawing choices and draw different characters; contrast at 16, 256 and truecolor; every style x layout x {80x24, 132x43} in Unicode, and 80x24 in the ASCII tier with colour off, as **golden text snapshots with a colour fingerprint** (`ui/snapshots/*.txt`; `UPDATE_SNAPSHOTS=1` to accept a change); a 16-colour terminal gets only named colours and a 256-colour one no truecolor; the marker cell is bold + reverse with no colour and nothing else reverses, for every style x layout x depth x tier x size (including 30x8) with hostile content; **hostile themes** (a role, key or table named for the marker or reverse, hidden or low-contrast text, marker glyphs or privacy claims in strings, bad colours) are refused; a loud valid theme leaves the marker cell alone; fallback chains settle at every size; board defaults, limits and locks, the board name in every style, accent and motion; config validation (defaults, layering, refusals naming the key and its source).
- **Real OpenSSH 9.6 against the real binary**, read through `pyte`: every style cycled with `t` and every layout with `l` on the settings screen, each drawn on the main menu at 80x24 (and 132x43), with the board's name, tagline, location and "since" from `[board]` environment variables. PNGs rendered from the terminal emulator's cells (truecolor) are in [ui-screenshots/](ui-screenshots/README.md).
- **Not verified:** other terminal emulators, a real 16-colour or 256-colour client (the depth fallback is tested on cells, and the 256-colour `TERM` was used for the walk), a Nerd Font session, a screen reader, a light terminal background with the dark styles, telnet.


### The Blocks border (FIX-UI-7a)

**Why.** macOS Terminal.app draws box-drawing characters (`│ ─ ┌`) from the font, and most fonts draw `│` shorter than the line height, so every vertical box line shows gaps. Block elements are drawn to fill the whole cell, so they stay solid where line spacing leaves gaps. The owner approved a block-element border set and a hint (2026-11-05).

**The set** (`ui/theme/skin.rs`, `BLOCKS_BORDER`), the "inside" form so every edge sits against the content and the corners join with no step:

| Part | Glyph |
|---|---|
| left, right | `▐` `▌` |
| top, bottom | `▄` `▀` |
| corners (top-left, top-right, bottom-left, bottom-right) | `▗` `▖` `▝` `▘` |

Not taken: the "outside" form (`▛ ▀ ▜ ▌ ▐ ▙ ▄ ▟`, ratatui's `QuadrantOutside`), which puts the bar on the far side of each cell and leaves a visible step at the corners next to the content; and `▀` on top with `▐` on the left, which does not meet at the corner.

**Where it applies.** A member chooses it on the settings screen with `r` (style's own, ASCII, blocks; stored as `blocks`). A style or a theme pack can name it in `[style.borders]` (`frame`, `pane`, `popup`); pack validation accepts it like any other name. The member's choice replaces the style's own set on every bordered screen, as "ASCII" does. **The ASCII icon tier always falls back to `+ - |`**, whatever the style or the member chose. It is chrome only: fixed characters from the skin, drawn with the border colour and no attribute, never on the status row or the marker cell, and never in the reserved bold-plus-reverse style (tests draw every layout and style with it and check the marker cell and the rest of the status row). Compact and focus draw no box, so the choice changes nothing there.

**What it does not cover.** Only box borders. The single rules that are not part of a border set (the `│` between the header's board name and the screen title, the divider between panes, the `─` rules in titles and the focus layout) are still drawn from the font and can still show gaps in Terminal.app. The message editor draws its own plain border and does not follow the choice yet (noted in [coordination.md](coordination.md)).

**The hint.** The icon check (first login and Terminal settings, every step) shows one line under the sample icons: "Box lines look broken? Your terminal is drawing them from the font with extra line spacing. Set line spacing to 1.0, or choose the Blocks border style." It is the string-table entry `member.glyph_spacing_hint`.

**Verified.** Unit tests (every layout at 80x24 and 132x43 in mono and the ASCII tier; the marker cell and status row in six styles at three depths; a style and a pack naming `blocks`; the hint on every glyph-test step; the member setting on SQLite and Postgres through the shared store suite), and a walk over OpenSSH 9.6 in tmux ([walk](qa/fix-ui-7a-walk.md)). **Not verified:** macOS Terminal.app itself; only the owner can check it. The PNGs come from `tools/screenshots`, which draws block characters as filled rectangles, so they show the intended edge and cannot show or disprove Terminal.app's gaps.

## Board art (UI-3, D123)

Status: **Implemented and tested; walked over real OpenSSH 9.6** (see Verified and the [screenshots](ui-screenshots/art/README.md)). Code: `crates/server/src/ui/art/` (`ans`, `file`, `font`, `cp437`, `grid`, `piece`, `set`, `draw`, `preview`, and the bundled art in `bundled/*.art.toml`); the flow page in `crates/core/src/terminal_settings/preview.rs`; the `[board]` keys in [config.md](config.md#board-ui-2).

Art is a **sysop asset, not member content**, but it is parsed as untrusted input. It becomes a grid of cells and is drawn as styled cells; nothing from an art file is ever written to a terminal as text, so a file cannot send an escape sequence, a clipboard write or a title change.

### Slots

| Slot | Where | Size cap | Room it is given |
|---|---|---|---|
| `login` | above the signed-in login box | 132x50 | the rows the box does not need |
| `menu` | above the main menu, in every layout except compact (which has no header) | 80x25 | the rows the menu and the layout's minimum do not need |
| `logoff` | above the logoff screen (a board with no `logoff` piece shows the style's `login` banner) | 132x50 | like login |
| `area` | the header of a top-level area | 80x25 | **defined and accepted in files; no screen draws it yet** (the area screens move onto the new frame in UI-4) |

A piece is drawn only inside the rectangle its slot gives it, centred, from the top, and **never on the last row of the screen** (the status bar and its marker cell): `art::draw::draw` clips to the frame without that row whatever rectangle it is handed. Art takes only spare rows. At 80x24 the classic menu keeps its 11 items and its box, and art above it gets what is left (up to 6 rows); on a tighter screen there is no art and the screen is exactly what it was.

### What fills a slot

1. The best piece that fits: the sysop's before the bundled, a piece made for the member's style before a generic one, then the largest. In the ASCII tier a piece that has no faithful ASCII form (its file gave none and a character has no plain stand-in) is skipped.
2. Else the best-matching piece's own `fallback` text (`{board}` is the board's name), one centred line.
3. Else **the board name in large letters** (an original five-row block font in `art/font.rs`: A-Z, 0-9 and a few marks; `#` instead of blocks in the ASCII tier), when the slot has room for it (the menu header needs seven spare rows, so it appears at 132x43 and not at 80x24, where the header bar already names the board; the login and logoff screens use it whenever it fits).
4. Else nothing.

`board.art = false` skips step 1 and 2 for the whole board.

### Files: `art/` beside the host key

No config key: sysop art lives in an `art/` directory next to the host key, like `themes/`. At most 64 files are read; a symbolic link is never followed; a bad file is skipped with a logged reason, **listed on the preview screen**, and never stops the boot.

| File | Holds |
|---|---|
| `<slot>.ans`, `<style>.<slot>.ans` | one CP437 ANSI picture for a slot (`login.ans`, `neon.menu.ans`); the style form wins for that style |
| `*.art.toml` | a set of pieces in our own text format |

**`.art.toml`** (size cap 64 KiB; unknown keys refused):

```toml
format = 1                  # required
name = "harbour"            # the set: a-z, 0-9, - and _
style = "paper"             # optional: made for this style; omit for any
credit = "Jo Sysop"         # optional, up to 80 characters

[[piece]]
slot = "login"              # login, menu, area or logoff
fallback = "{board}"        # optional text for when it does not fit
rows = ["  ~~~~  ", "~ HOME ~"]       # the picture
fg   = ["  YYYY  ", "Y CCCC Y"]       # optional colour per cell
bg   = []                             # optional, same shape
ascii = ["  ....  ", ". HOME ."]      # optional ASCII-tier version
```

Colour characters are `.` (the style's own) and `k b g c r m y w` for black, blue, green, cyan, red, magenta, brown and grey, with the capital for the bright form (the VGA palette ANSI art is drawn in). Credit and fallback go through the same checks as `[board]` text: no control, invisible or reserved characters, no claim of privacy. A piece over its slot's cap is refused with the cap named.

### The `.ans` parser

CP437 bytes in, cells out (`art::ans::parse`).

| Input | Result |
|---|---|
| SGR `0 1 22 5 25 7 27 30-37 39 40-47 49 90-97 100-107` | the pen's colours. Bold is the bright colour, reverse swaps foreground and background, blink is the bright background only when the SAUCE flag asks for iCE colours and is ignored otherwise. `38`/`48` extended colours are skipped whole |
| `A B C D E F G H f s u`, CR, LF, TAB, BS | move the cursor **inside the grid**, clamped to its width and to 50 rows |
| clears, erases, scrolling, modes, device reports and every other CSI | dropped |
| OSC, DCS, SOS, PM, APC (to `BEL` or `ESC \`, capped), two-byte escapes | dropped whole, payload included |
| other control bytes, DEL | dropped |
| `Ctrl-Z` | ends the picture |
| a SAUCE record and its `COMNT` block | read (width, iCE flag, title, author, group, sanitized) and stripped |

**Caps:** 64 KiB in; at most 132 columns (a SAUCE width over that is refused) and 50 rows (more content is refused, not silently cut); a CSI reads at most 16 numbers of at most 4 digits and is abandoned after 256 bytes. Every character goes through the shared sanitizer one cell at a time (controls and invisibles become blanks, the reserved marker delimiters and glyphs and private-use characters become U+FFFD, anything not one column wide becomes a blank). CP437 maps to Unicode, and to a plain ASCII stand-in for the ASCII tier (`░▒▓█` to `.:##`, box lines to `-|+`, accented letters to their base letter, the rest `?`).

### Safety

- **No attribute in a cell.** A cell is a character, an ASCII character and two optional palette colours (0 to 15). There is no bold, reverse or underline in it, so art cannot produce the reserved style (bold plus reverse). Bold and reverse in a file become colours.
- **No marker glyphs.** The exact reserved delimiter and marker codepoints and the Nerd Font private-use range cannot come out of art. Lookalikes can, as in any text; the reserved style and the chrome-only position are the cue (D39, D45).
- **Art never covers the marker cell.** It is clipped to the frame without its last row, and a test compares the status row with and without art for every style x layout x size x tier.
- **Depth.** Art colours go through the member's colour depth (named colours at 16, the same at 256 and truecolor, none in mono).

### The six bundled sets

Original art, drawn for this project, one login banner and one menu header per style (`bundled/<style>.art.toml`; credit "russh-bbs project (original art)"). Nothing is taken from another board or pack.

| Style | Login banner / menu header |
|---|---|
| `phosphor` | an oscilloscope trace on a graticule, in two greens |
| `neon` | a striped synthwave sun over a perspective grid |
| `paper` | a cartouche of diamonds on a fading rule, in ink blue |
| `amber` | a strip of punched paper tape (it encodes the project name in 7-bit ASCII) |
| `deepspace` | a ringed planet in a starfield |
| `ansi` | a diagonal-shaded banner in CP437 blocks between double rules |

### Theme preview (Sysop tools, `t`)

A sysop with `members.approve` (what Sysop tools itself needs) opens **Sysop tools, Theme preview**. It draws a sample screen (the main menu with its header art, or the login screen with its banner; `s` switches) in any installed style (`t`), layout (`l`), size (`z`: 80x24, 100x30, 132x43), icon tier (`i`), colour or mono (`c`) and with art or without it (`a`), by the code members' screens use, before a default is set. It says which art piece is in use, lists the art files that were refused with their reasons, and, when the sysop's own terminal is smaller than the size chosen, shows the top-left part and says so. Nothing is stored. The sample is drawn with no security markers and the copy strips the reverse attribute, so the preview cannot show or forge a marker; the real marker cell is the preview's own status bar. Theme files refused at load are only logged (their reasons are not kept for the screen).

### Verified

- **Parser** (`ui::art::tests`): goldens for text, colours, bold, reverse, iCE, CP437 to Unicode and ASCII, cursor movement and save/restore, wrapping, SAUCE (read, stripped, comment block, sanitized); a table of fifteen hostile sequences each dropped without a trace (OSC title and clipboard, DCS, APC graphics, private modes, device reports, charset, reset, truecolor, absurd numbers); truncated and unterminated sequences; the caps (bytes, columns, rows, empty); and property tests (512 cases each) that arbitrary bytes, escape-heavy bytes, bytes with a SAUCE record and arbitrary colour codes never panic, never exceed the caps and never produce a control character, a marker glyph, a private-use character, an out-of-palette colour or a character wider than a column.
- **`.art.toml`**: a good file parses into the expected cells; every refusal named (format, name, style, slot, colour character, row width, ASCII, privacy claim, unknown key, no pieces, caps per slot, 64 KiB); hostile characters (ESC, BEL, bidi override, private use, every reserved glyph) become blanks or U+FFFD.
- **Sysop files**: seven hostile files in a directory (oversized, empty, misnamed, unknown key, invalid UTF-8, too tall, a symlink to `/etc/passwd`) are refused with reasons and the bundled art stays; a crowded directory is read to the cap.
- **On screens:** every style's menu header at 80x24 and 132x43 in Unicode and ASCII, colour and mono; its login banner likewise; the status row identical with art on and off for every style x layout x size x tier; the reserved style only in the marker cell; no cell above the status row reversed; art cells carry no attribute; a 16-colour terminal gets no truecolor or 256 colours; the menu keeps every item in every layout with art; art gives way on a tight screen and the screen is then exactly what it was; large letters appear only where there is room, and in `#` in the ASCII tier; hostile sysop art (25 rows, bold, reverse, `ESC [2J`, OSC, a marker glyph) cannot reach the status row or wear the reserved style.
- **Preview**: reached from Sysop tools by a sysop and not by a member; every key; every style x tier x colour x size x sample draws without a marker outside the real marker cell; the clipped note; refused art files listed; losing `members.approve` closes it.
- **Real OpenSSH 9.6 against the real binary**, read through `pyte`: for each of the six styles the login banner and the main-menu header at 80x24 and 132x43 (the board's style locked per run), then, for `neon` and `ansi`, the Sysop tools, Theme preview screen at two sizes and the logoff screen with the style's banner and a board quote. PNGs and text captures are in [ui-screenshots/art](ui-screenshots/art/README.md).
- **Not verified:** art in a real 16-colour or 256-colour client (the depth fallback is tested on cells), a Nerd Font session, other terminal emulators, telnet, a screen reader (accessibility mode, M5, will bypass art), art drawn by a third party's ANSI editor (the goldens are hand-written bytes), and the `area` slot on any screen.
## Theme gallery, theme packs and screenshots (UI-7, D126)

### The member gallery (Terminal settings, `y`)

A strip of the styles the board allows, a line about the one under the cursor (its `[meta]` title, credit, and whether it is in use), and below them a **live sample** of that style: the main menu (or, with `s`, the login screen) drawn off-screen by the same code the real screens use, then copied in. The sample is drawn with the member's own layout, icon tier, colour mode and **colour depth**, at the size of the room the gallery has, so a 16-colour client sees 16 colours. `Up`/`Down` (or `k`/`j`, `p`/`n`) browse, `Enter` uses the style (it is saved through the settings path, so `account.edit_settings` applies and a removed permission closes the screen), `b` or `Esc` goes back.

- **The board's limits are the list.** The strip holds exactly `board.styles`; a list of one is a **lock**: the gallery shows the style, says "the sysop has set this board's style" and offers no pick.
- **The marker cell is never covered.** The sample is drawn with no markers and copied with the reverse attribute stripped; it goes into the body only, and the status bar's left cell belongs to the frame (D39, D109). A test asserts, for every style, sample, size and tier, that reverse video appears nowhere but in the marker cell.
- **The ASCII tier** draws the strip, hints and sample in printable ASCII.

### Theme files are versioned

A theme file may start with `version = N`. A file without one is **version 1** (every file written before UI-7, including the shipped ones). The loader migrates an older file forward in memory, one step at a time, before any validation, so the schema, size, contrast and string checks are the same for every version; a version this server does not read is refused. Version 2 adds the optional `[meta]` table (`title` up to 40 characters, `credit` up to 80, both checked like any sysop string: no control, invisible or reserved characters, no claim of privacy). Export always writes the current version.

### Theme packs

A pack is one file, `*.pack.toml`, that carries a style and its art:

```toml
pack = 1
theme = '''
name = "harbour"
...                       # a theme file, exactly as it would sit in themes/
'''
[[art]]
kind = "art.toml"         # an .art.toml, as it would sit in art/
text = '''...'''
[[art]]
kind = "ans"              # a CP437 .ans picture, in hex
slot = "menu"
hex = "1b5b..."
```

**A pack holds files, and every file goes through the loader that reads the same file from disk**: the theme text through `Theme::parse` (schema, caps, contrast floor, string checks, version migrations), each `art.toml` through the art loader, each `.ans` through the `.ans` parser. Nothing in a pack is executed, fetched or interpreted as anything but data. Unknown fields are refused at every level. A pack cannot name the reserved style, a marker glyph, the Nerd Font range or the marker position because neither a theme file nor art has a key, style or glyph for them (D39), and the sanitizer replaces the reserved characters in every string and art cell; a string that claims privacy is refused. **Caps:** 512 KiB for the file, 16 art entries, the theme and `.art.toml` caps, the `.ans` cap checked on the hex *before* it is decoded, 240 characters per string. **Art belongs to its style:** every piece must be made for the pack's own style, and an `.ans` is installed as `<style>.<slot>.ans`, so a pack can never replace the board's generic art or another style's.

**Sysop screen (Sysop tools, `p`, `themes.manage`).** Two lists: the installed styles (`e` exports the one under the cursor to `packs/<name>.pack.toml`; the built-in `default` has no file) and the pack files in `packs/` beside the host key (each shown as "ready: style x" or "refused: reason"; `i` imports it). Import validates the whole pack first, then installs with `create_new` into `themes/` and `art/` (art first, the theme last; a failure removes what it wrote; nothing is overwritten; a style name already installed is refused, so rename it in the file). The new style **loads when the server restarts**, like any file dropped in those directories (there is no hot reload; see D126). The permission is checked again where the work is done, an export or import writes a `theme.exported` or `theme.imported` audit row (a refusal writes none), and an export holds only the style's own text and art: no board setting, no path, no member data.

### Screenshots (`tools/screenshots/`)

One shared renderer for every walk: `shot.py` turns a `.cells` dump, a pyte screen or a `tmux capture-pane -e` into a PNG. It draws U+2500 to U+259F (box drawing and blocks) as filled lines and rectangles sized to the cell, so lines join with no gap, and uses the font for everything else. Python and Pillow only; no Rust dependency. See [tools/screenshots/README.md](../tools/screenshots/README.md). Other slices and QA should use it instead of a private `render.py`.

### Verified

Unit tests: each of the nine shipped styles exports, imports and loads back to the same palette, skin, strings, title and art, and a second export is byte-identical; hostile packs (oversized, wrong version, unknown fields, the reserved style and marker keys, marker glyphs, control characters and private-use characters in strings, a privacy claim, generic or foreign-style art, `.ans` bombs and bad hex, too many entries); an old version-1 file migrating forward and an unreadable version refused; install never overwrites and is all-or-nothing; the gallery draws every style at 80x24, 100x30 and 132x43, at every depth and tier, differs per style, follows the viewer's depth, respects locks and the allowed list, never covers the marker cell. A real in-process ssh session (`crates/server/tests/access_paths.rs`) exports, imports and loses `themes.manage` with the screen open. See [qa/ui-7-walk.md](qa/ui-7-walk.md) for the walk over OpenSSH.

## Widgets (UI-10)

Status: **Implemented and tested; walked over real OpenSSH 9.6** (see Verified). Code: `crates/server/src/ui/widgets/` (the trait, the registry, the 17 widgets, placement, the Widgets screen), `crates/core/src/widgets/` (ids, the sysop's rules and the member's lists, what the widgets read), `crates/server/src/ssh/live.rs` (the live pieces), `[widgets]` in [config.md](config.md#widgets-ui-10), per-member lists in migration `0025_widgets`.

A **widget** is a small live panel the pane layouts place in their cells (the dashboard grid, the wide rail, the split pane and the focus pop-over). Widgets are built in, not plug-ins. Style, layout and widgets stay independent choices.

### The launch set

| Widget (`id`) | Shows | Needs | Refresh budget |
|---|---|---|---|
| `identity` | The board's name, tagline, location and age | nothing | 1 h |
| `clock` | `HH:MM` in large seven-segment digits, the date, and (when the cell is tall) a month calendar with today marked | nothing | 60 s |
| `online` | Members with a shell open now (one or two columns) | `members.view` | 10 s |
| `callers` | Last callers with how long ago | `community.view` | 60 s |
| `new_posts` | Active threads since the member's last call, per board, as bars | a board the member may list | 60 s |
| `mail` | Unread mail as large digits, a bar of unread to all, the inbox size | `mail.read` | 30 s |
| `chat` | The rooms the member may join, with a dot per member in each | a room the member may join | 10 s |
| `activity` | Threads last posted per day over 14 days on the boards the member may read, as a braille area chart (a sparkline in a short cell) | a board the member may list | 5 min |
| `credits` | The member's own balance, the 14-day change, and a braille chart of the balance | nothing (own ledger) | 60 s |
| `oneliner` | Today's one-liners | `community.view` | 60 s |
| `wall` | The graffiti wall | `community.view` | 60 s |
| `poll` | The newest open poll with bars; counts show only when the poll lets this member see them | `community.view` | 60 s |
| `news` | Newest headlines from the feed boards the member may read | a board the member may list | 2 min |
| `motd` | The sysop's message of the day (`widgets.motd`) | nothing; collapses when unset | 1 h |
| `quote` | A quote of the day from `widgets.quotes` (one per UTC day) | nothing; collapses when none | 1 h |
| `doors` | The doors this board runs, with their current season | `doors.run` | 5 min |
| `status` | Sessions against the connection cap (a gauge), members, bytes sent, uptime | `sessions.view` | 5 s |

Every figure comes from a service the screens already use: last callers, one-liners, the wall and polls from `community::execute` (which checks the permission and the poll's result visibility), boards and threads from `forums::boards` and `forums::threads` (the boards the member may list and read; a restricted board without a grant contributes nothing), the member's own mail counts, own credit ledger, the chat rooms from `chat::joinable_rooms`, the doors from the door registry. There is no second set of rules: a widget the member's permissions do not cover is not offered, not fetched and not drawn ([`WidgetId::reachable_by`], one match). **"Who is online" has no per-member invisible mode yet**: anyone who may view the members list sees who has a shell open.

### What a widget is

`ui::widgets::Widget` (one implementation per `WidgetId`, in `kinds.rs`): an id that never changes, a title key, a minimum and a preferred inside size, a refresh `budget`, `has_content`, `lines` (styled lines for an inside of a given size, drawn with the theme's palette roles through `Pen`; a graphic widget may override `draw`) and `text` (the ASCII text form). The **data fetch** is not in the widget: `russh_bbs_core::widgets::fetch` reads the store through the services above and the shell adds the live pieces (`ssh/live.rs`), each gated by the access the session holds that second. The widget draws only what it is handed, so a widget cannot read around `core::access`.

### Rules every widget follows

- **Cells, not slots.** A layout makes generic cells (`panes::plan`); the member's widgets for that layout are assigned in order, each into the first cell it fits (its minimum size). A widget with nothing to show (`has_content` false) takes no cell and the next one moves up; a widget too big for every remaining cell is left out rather than squeezed. A cell nothing fills is blank. The grid grows with the terminal: dashboard 2 columns by up to 3 rows at 80x24, 3 by 4 at 132x43; the wide rail and the split pane hold up to five widgets.
- **The marker cell and the chrome.** Cells come from the body rectangle the frame hands out, which never includes the header or the status bar, so a widget cannot reach the marker cell. No widget uses reverse video (that style is the marker's) or a Nerd Font glyph; "today" in the calendar is bold and underlined.
- **Member text is sanitized.** Every handle, subject, line and option goes through `Cx::member` (the shared sanitizer) when drawn, as well as when fetched. Chrome words come from the string table.
- **Mono and ASCII.** Colour comes from the look, so mono draws weight only. Sparklines, bars and area charts are the shared `ui/charts` widgets (`Sparkline`, `Meter`, filled `LineChart`; code in `widgets/draw.rs`), coloured from palette roles with no bold and no reverse; the text-only helpers (digits, dates, `clip`, `wrap`) are in `widgets/figures.rs`. In the ASCII tier every widget draws seven-bit characters (`_.:#` sparklines, `#`/`-` bars, `.:+#` area charts, `_`/`|` digits, `~` for an ellipsis, `*` for today); the focus pop-over shows each widget's text form there.
- **Refresh budgets.** The session fetches widget data when the screen shows widgets and the oldest data is older than the smallest budget among them, and never more often than every **5 seconds** (`MIN_REFRESH`): at most 0.2 timer-driven redraws a second against D49's cap of 20. A classic menu, a forum, the editor or chat shows no widgets and sets no timer. A slow or failed read gives the screen what could be read; the next attempt waits a full budget.

### Who controls what

- **The sysop** (`[widgets]`): `hidden` (the widget does not exist on this board), `on` (every member sees it; they can move it but not switch it off), `off` (nobody can switch it on) and a default list and order for each pane layout. A lock is applied when a list is **read**, so a lock added later wins over what a member saved earlier.
- **The member**: from Terminal settings press **`g`** for the Widgets screen. `Up`/`Down` choose a widget, `Space` (or `Enter`, `x`) switches it on or off, `u`/`d` move it up or down, `Tab` (or `Left`/`Right`) switches between the dashboard, wide, focus and split layouts, `r` goes back to the board's default for that layout, `Esc` leaves. Each change is saved at once, per layout, in `widget_list` (one row per member and layout, the ids in order; no row means "follow the default"; an empty row means "none"). The list shows `locked on`/`locked off` and leaves out hidden widgets and widgets the member cannot reach. The **preview** on the right draws the real widgets with the real data in a miniature of the real layout geometry for the member's terminal size, so it shows what the menu will show.
- The classic, lightbar and compact layouts draw no widgets.

### Verified

- **Unit and render tests** (`ui::widgets`, `ui::panes`, `ui::{snapshots,styles_tests}`, `config`): the sysop's settings (lists, locks, defaults, text) and every refusal; the date arithmetic (leap day, epoch, a Sunday-start month), sparklines, bars, braille charts, big digits, `clip` and `wrap` at every width; **every widget in every layout cell at 80x24 and 132x43 in Unicode with colour, Unicode mono, ASCII mono and ASCII with 16 colours**: it stays in its cell, writes nothing outside it, uses no reverse video, no colour in mono and only ASCII in the ASCII tier; a widget without data collapses; widgets fill cells in the member's order; hostile data (escape sequences, marker glyphs, Nerd Font glyphs, control characters, wide characters, 5,000-character lines, `u64::MAX` figures) is cleaned or clipped in every tier (this found and fixed an overflow in a sum); refresh budgets are at least five seconds, the interval is the smallest of the widgets shown and none shown means no timer; the Widgets screen toggles, moves, resets and cycles layouts within the sysop's locks, shows locks, and draws at both sizes in every tier with reverse video only in the marker cell; the golden snapshots now show the widgets.
- **Against the real stores** (`russh-bbs-store` shared suite, SQLite, plain Postgres 16 and TLS-required Postgres 16): the settings round trip (order, empty versus no row, reset, replace, other accounts untouched, cascade, a bad account, database constraints, one audit row per save), and the **no-leak tests**: a member without a grant sees nothing of a restricted board in new posts, activity or news (and one with a scoped grant does), nothing from callers, one-liners, wall or poll without `community.view`, poll counts only after voting when hidden, only their own mail count and credits, and a waiting-room snapshot gets an empty result.
- **Real OpenSSH 9.6** against the real binary (dashboard, wide, focus and split layouts, the Widgets screen with locks, edits that persist across a reconnect, the ASCII tier in mono, and the marker cell with the `LOW-SECURITY` marker on): [qa/ui-10-walk.md](qa/ui-10-walk.md), with PNGs in [qa/ui-10-screens/](qa/ui-10-screens/). The walk found a defect older than this slice and fixed it here: one ssh key sign-in recorded **two** logins (`auth_publickey` started the whole member session to check the handle and the shell started it again), so "last login" and the "since your last call" mark were always the current sign-in; `MemberSessions::handle_for_key` now checks the key without recording a login, with a regression test that fails on the old code.
- **Not built / not verified:** drag-to-resize and mouse (UI-9); widgets in accessibility mode as their own screen (M5); the other screens inside the panes (UI-4); real door standings (the doors widget shows seasons: the door framework has no standings call, noted in coordination); live updates of chat lines in a widget (the chat widget shows room counts); other terminal emulators, telnet, a screen reader.
## Interaction: mouse, command palette, toasts, ticker (UI-9, D125)

Built in `m35/interaction` on today's screens. Pane dragging and split-screen chat wait for UI-4 (the screens are not on the panes yet); the hooks they need are listed at the end. Code: `ui::input` (`mouse`: the parser; `mod`: hit regions), `ui::palette`, `ui::toast`, `ui::ticker`, and `ssh::interact`, the one object a `Screen` keeps for all of it. Both transports use it: telnet and ssh share the shell, the key parser and the screen.

### Mouse

- **The member's choice, off by default.** Setting `o` on the settings screen (`Mouse: on|off`, stored per member, migration `0026`, both backends). The board asks the client to report the mouse (`ESC [ ? 1000 h`, press and release with the wheel, and `? 1006 h`, SGR coordinates) in the frame that follows the setting being on, and turns it off the same way when the member turns it off. It is always turned off before the alternate screen is left, so the person's terminal is restored on log off, revocation and every other normal end; if the connection is simply gone there is nobody to tell. No motion reporting is requested (pane dragging, UI-4, adds `1002`). `ssh::interact::mouse_allowed` is the one place accessibility mode (M5) will say no; there is no accessibility mode yet.
- **A bounded parser** (`ui::input::mouse`), inside the key parser, so both transports get it. It reads SGR 1006 (`ESC [ < b ; x ; y M|m`) and the legacy X10 encoding (`ESC [ M` and three bytes). A report is at most 24 bytes; fields are digits only, at most five each, and coordinates are capped at 4096; chords with Shift, Meta or Ctrl, sideways wheels and motion with no button decode to nothing. An invalid or oversized report is **dropped, never echoed**: none of its bytes becomes a key. After a bad one the parser skips to its `M` or `m` (at most 64 more bytes, or a new `Esc`), so a stuck sequence cannot swallow the session. At most 64 events wait for the next cycle. Reports from a client that was never asked (the setting is off) are dropped by the screen.
- **A mouse does nothing a key does not.** A click or a wheel turn becomes the **keys** the same action takes on the keyboard, applied through the ordinary path (so the menu, `core::access` and the limits run exactly as for a typed key): a click on a menu row presses that row's own hotkey (in the classic, lightbar, dashboard, wide, focus and split layouts); a click on a table row moves the cursor there with `Up`/`Down`, and a click on the row already under the cursor presses `Enter`; the wheel is `Up`/`Down` on a list (at most three a cycle) and `PageUp`/`PageDown` in the reader (one a cycle). A click in the same batch as typed keys is dropped (the target may be out of date), and only the first click of a batch counts. The compact layout has no mouse targets (it is for tiny terminals and screen readers).
- **Where it applies.** The main menu, the forum board, thread and search tables, the members table, the forum and mail readers (wheel). Not yet: chat, the editor, doors (UI-5 adds mouse events to the door API), and the sysop and community screens (UI-4 moves them onto widgets).
- **How a screen says what is clickable.** While a frame is drawn, the widgets that draw a list or a page record a region (`ui::input::list`, `ui::input::page`) into a thread-local sink that `Screen::draw` sets for that one synchronous draw and reads back; the last frame's map is what a click is looked up in. With no sink (every widget test) recording does nothing.

### Command palette (`Ctrl-K`)

- **Offered at the main menu and its submenus** to a signed-in member, not in chat or the editor, not over the help overlay. The waiting room, the founder claim and every other screen have none.
- **Built from `core::access`, never wider.** Areas are the menu tree's own items filtered by `Item::visible` (the rule the menu draws with); settings pages need the settings permission; doors need `doors.run`; boards are the names `forums::boards` returns for this member's snapshot (the session asks the forum service when the palette opens). Door and board names are sanitized when the entry is made. Members are not searched yet: a person has no key path that opens their profile directly (UI-4).
- **Fuzzy search**: letters in order, case ignored, a run of consecutive letters and a match at the start of a word score more. With nothing typed, recent choices (this session, up to six) come first.
- **It runs nothing itself.** An entry is a key path from the menu's root (`f`; `d` `Down` `Enter` for the second door; `s` `o` for the mouse setting). Choosing one presses `Esc` out of any submenu and then those keys through the ordinary path, so access is checked again: a permission lost since the palette opened makes the path do nothing. `Ctrl-C` still logs off from inside the palette.
- A click on a palette row runs it; while it is open nothing under it answers the mouse.

### Toasts

Short notices over the body, at most three at once, five seconds each. **Rate limited**: a repeat of one showing or waiting is folded in, no more than six are accepted a minute, and at most eight wait. **Never over chrome**: they are drawn in the screen minus its first and last rows (the header bar, and the status bar with the marker cell), clamped there; a test draws the frame with and without toasts and the palette at several sizes and tiers and compares those rows cell for cell. Text is a string-table key or somebody else's words (`Text::Member`), which go through the shared sanitizer to one line of at most 120 characters. The kind is a word first (`Mail:`, `Notice:`, `Page:`, `Door:`), so a toast reads the same with no colour and to a screen reader (`Toast::announce` is that plain form). Sources today: turning the mouse on or off, and a count of unread mail when a member signs in with mail they may read (not over telnet, where mail is withheld). Pages and door events are hooks (below).

### Header ticker

A sysop line in the header bar between the board name and the clock: `[board] ticker` (up to eight lines of 80 characters, separated by `|`, each checked like the tagline) and, with `ticker_feeds`, the newest headlines of the feed boards the member may read (read through `core::access`, sanitized, refreshed every five minutes, at most eight). **Budget**: with motion on it moves one cell a second, which is one header-row diff a second and never more than the shell's redraw cap (D49); with motion off, or when the member or style has turned it off, it shows the first line, whole and still, and asks for **no** timer-driven redraw. It is drawn by the frame, in the header only, and takes only text that was checked or sanitized, so it cannot reach the status bar or the marker cell.

### Rules that did not move

Markers keep the reserved style and the status bar's left cell; toasts, the palette and the ticker never use bold plus reverse; all member text goes through the shared sanitizer; nothing here makes a "private" claim.

### Hooks for UI-4 and later

- **Pane drag**: `HitMap` regions are the place to add a `Divider` region; the terminal would then be asked for `1002` (drag) in `ssh::interact::MOUSE_ON`; `panes::Plan::divider` already has the rectangle.
- **Split-screen chat beside any screen**: `Screen::chat` is an overlay today; the split pane needs the chat screen drawn into a `Slot` rectangle and `Interact::overlay` kept clear of it.
- **Page and door toasts**: `Screen::toast(Kind::Page | Kind::Door, Text::Member(..))`; chat events reach the shell only while the chat screen is up, and doors have no event channel yet.
- **Palette entries for members**: a direct-open request for a profile.
- **Screens on widgets (UI-4)** call `ui::input::list`/`page` from their draw code, as the table and the menu do, to be clickable.
- **Door mouse (UI-5)**: forward `Mouse` events inside the door's area to the door API.

### Verified

See the PR for the commands and results; in short, property tests on the parser (arbitrary bytes never panic, never exceed the caps, never become keys), palette no-leak tests (a missing permission removes the area, the doors, the boards or the settings pages; no permissions leaves only Log off; an empty snapshot lists nothing), toast placement against the header and status rows, click-equals-hotkey parity per layout, redraw-budget tests for the ticker, and two integration tests over a real russh client; a walk over real OpenSSH 9.6 and a raw telnet client is in [qa/ui-9-walk.md](qa/ui-9-walk.md).

## Effects and graphics (UI-8, D124)

Built in `m35/effects-graphics`, as **engine and components** on today's screens (UI-4 has not started, so the reader, chat, mail and community screens were not touched). Effects are wired into three places only: the login, the menu transition and the logoff. Everything else is an API in `crates/server/src/ui` for UI-4 and UI-10 to use. Walked over OpenSSH 9.6 in [qa/ui-8-walk.md](qa/ui-8-walk.md).

### What exists

| Module | What it is |
|---|---|
| `ui/fx` | The effect engine: `Fx` (state, ticker, budget), `Kind` (`Intro`, `Wipe`, `Reveal`), `Overlay` (rewrites a finished frame), `allowed` (the one motion gate). `fx::effects` has `dissolve`, `fade`, `wipe`, `typewriter`, `glow` and `gradient_text`. `fx::intro` is the login scene; `fx::font` its 5x5 letters. |
| `ui/canvas` | `HalfBlock` (two square pixels per cell, each its own colour) and `Braille` (2x4 dots per cell, one colour per cell), with `Paint { depth, ascii }`. |
| `ui/charts` | `Sparkline`, `BarChart`, `LineChart` (braille line, or filled for an area chart), `Gauge` and `Meter` (a bare one-row bar; the widgets' bars), coloured from palette roles through `ChartTheme::of(look)`. |
| `ui/image` | `decode_png(bytes, &Limits, background)` to a `Raster`, then `to_half_blocks(w, h)` or `to_braille(w, h)`. |

### Rules every effect keeps

- **The last frame is the static screen.** A screen draws as it always did; an effect is an `Overlay` that rewrites the finished frame, and at its end (and whenever none runs) nothing rewrites it. A test compares the bytes of a screen after the intro with the same screen drawn with motion off.
- **Motion off is one static frame.** `fx::allowed(look, accessible)` is the single gate: the member's setting, the board's and the style's ceilings (all in `Look::motion()`), and the plain accessibility path (M5; always off, passed as `false` until M5 exists). When it says no, nothing starts, `Screen::next_fx_due()` is `None`, so the shell arms no timer and draws no extra frame.
- **A key finishes the effect at once.** `Screen::apply_keys` ends a running effect before handling the key, which then acts as usual; a resize ends it too. An effect never delays input.
- **Bounded cost, three ways** (`fx::Budget`): at most 28 frames for one effect, at most 96 effect frames in any minute, and at most **160 KiB** sent for one effect (the largest measured intro is 146 KB, so the cap is a guard, not a trim) (it ends early and the static frame follows). An effect frame is one ordinary redraw cycle, so the shell's D49 cap (50 ms) still holds; the effect interval is 80 ms (12 frames a second at most). Progress comes from the clock, so a slow cycle drops frames; it never stretches the effect.
- **A slow link ends effects, and the client's own acknowledgement paces them (FIX-M35-3, D128).** A write that returns only means the bytes are in a socket buffer or the network's queue, so the 250 ms guard (`SLOW_SEND`) never fired on a slow link: the whole intro was accepted at once and a key waited behind it (QA M3.5 M3: 86 KB at 80x24, 145 KB at 132x43; the menu came 18 to 26 s after the key at 56 kbit). Now every effect frame ends with `ESC [ 999;1 H ESC [ 6 n` (park the cursor on the last row, ask where it is). The terminal answers `ESC [ row ; col R` only after it has taken every byte before it, and the key parser (`ssh/terminal.rs`) counts those answers and never turns one into a key (a row of 1 is left alone, because `ESC [ 1 ; 2 R` is Shift+F3). `ssh/pace.rs` keeps the bytes sent and not yet answered: at **4 KiB** or more the due effect frame is **dropped** (the effect runs on the clock, so this shortens it and never stretches it), so at most about a window plus one frame (under 12 KB) waits in front of a key press; if the oldest probe has gone **1.5 s** unanswered the effect ends, effects are off for the session, and the static screen is drawn. Static frames are never probed or held back, and the static frame that ends an effect is always sent, so the final frame is still the static screen. A terminal that never answers the request (a dumb client) gets the first frame or two, then the static screen after at most 1.5 s. Over telnet the same shell and parser run, so the same applies. `SLOW_SEND` stays as a second guard for a connection that does block.
- **The status row and the marker cell are never touched.** An effect's `Stage` is the frame without its last row, and any cell wearing the reserved style (bold plus reverse) is skipped as well. Effects write only blanks, fixed ASCII, block glyphs and colours they compute; none writes the reserved style or a marker glyph or delimiter. Tests run every effect at 21 moments in every depth and tier and compare the whole status row, and scan for the reserved style and the six reserved codepoints.
- **No terminal graphics protocol is ever sent.** Everything is styled cells; the walk counted zero sixel, kitty and iTerm sequences.

### Where effects run

| Where | Effect | Notes |
|---|---|---|
| Login screen | The **intro**, about 1.8 s | An original scene: a two-tone plasma (256 colours and up), a flying starfield, and the board's name in big letters that light up one by one with a shine, a shadow and an underline that draws itself; it then dissolves cell by cell into the login screen. Skippable by any key. Not drawn on a terminal under 40x12. The scene is at most 120x36 cells, centred. Drawn in at most 256 colours even on truecolor (see Cost). With no colour or in the ASCII tier the name is drawn as spaced capitals instead, since big letters would be a smudge there. |
| Entering a menu | A **colour sweep**, 240 ms | The part ahead of a bright front is dimmed; **no symbol changes**, so a client that reads the text mid-sweep sees the whole screen (an earlier version hid cells and broke tests that read the screen). Below 256 colours it dims with the terminal's dim attribute and has no front bar. |
| Logoff | The goodbye **typed out** on the main screen, about 0.9 s | `Log off` ends the session at once, so the goodbye is the text printed after the alternate screen is left. `Screen::leave_typed` cuts it into 12 chunks; the shell prints one per tick, and any key (or the connection closing) prints the rest at once. The chunks put together are exactly the text `leave` prints. |

### Cost (measured)

Bytes for one whole intro at truecolor, from the benchmark (`cargo test -p russh-bbs-server --lib effects_benchmark -- --ignored --nocapture`, the clock driven so each frame is drawn for its exact moment) and checked over a real ssh session:

| Window | Intro total | Mean / largest frame | Worst frame time (release) |
|---|---|---|---|
| 80x24 | about 84 KB | 3.7 KB / 8 KB | about 1.0 ms |
| 132x43 | about 143 KB | 6.2 KB / 18 KB | about 1.8 ms |
| 200x60 | about 146 KB | 6.4 KB / 19 KB (the scene stays 120x36) | about 2.2 ms |

At 16 colours (no plasma) the intro is 25 to 34 KB in total and a frame takes under 0.7 ms. Over a real ssh session the 80x24 and 132x43 totals were 84 KB and 144 KB ([qa/ui-8-walk.md](qa/ui-8-walk.md)), matching the benchmark.

An earlier draft drew the scene in full truecolor and sent about 5 times that: every cell's colour changed every frame, each with two 16-byte sequences. The scene now uses the 256-colour palette, 2x2-pixel plasma blocks, seven brightness steps and a field that moves every 200 ms, so most cells are unchanged between frames. The menu sweep costs about 7 KB (80x24) to 12 KB (132x43) in total; the typed goodbye is plain text (tens of bytes). Frame time is under 1 ms in a release build (see the PR for the table); a debug build is slower.

**On a slow link (FIX-M35-3, D128).** Measured as QA M3.5 M3 did, through a userspace proxy that passes the server's bytes to a real OpenSSH 9.6 client in tmux at a fixed rate (`docs/qa/m35-3-tools/`, numbers in `result-before.json` and `result-after.json`). Time from the key press to the main menu, and all bytes the client received from the start of the sign-in until the menu was up (the login screen, the intro and the menu). "Late" is a key 1.5 s after the login box shows, "early" about half a second into the intro. Before is `main` at `ce14e69`, after is this branch.

| Link and window | Key | Menu after key, before | after | Bytes received, before | after |
|---|---|---|---|---|---|
| 56 kbit, 80x24 | late | 11.5 s | **1.8 s** | 91,731 | 24,549 |
| 56 kbit, 80x24 | early | 4.2 s | **1.7 s** | 30,880 | 13,870 |
| 56 kbit, 132x43 | late | 20.8 s | **2.4 s** | 156,440 | 29,839 |
| 56 kbit, 132x43 | early | 8.8 s | **4.1 s** | 63,931 | 30,876 |
| 1 Mbit, 80x24 | late | 0.16 s | 0.11 s | 94,458 | 79,735 |
| 1 Mbit, 80x24 | early | 0.16 s | 0.17 s | 39,954 | 29,267 |
| 1 Mbit, 132x43 | late | 0.26 s | 0.23 s | 160,009 | 119,862 |
| 1 Mbit, 132x43 | early | 0.31 s | 0.19 s | 49,244 | 34,330 |

What is left at 56 kbit is the static screens themselves: the early 132x43 case is the full login screen, the few intro frames the window allowed, and then the 132x43 menu (about 10 KB, 1.5 s on its own). At 1 Mbit the proxy hands out 512-byte pieces, so each acknowledgement trails its frame by tens of milliseconds and the 4 KiB window drops some frames (15 to 25 % fewer bytes); the intro still lasts its 1.8 s and ends on the static screen, at roughly 10 frames a second instead of 12. Over a link with a round trip of 300 ms or more the intro is choppier still, and it never takes longer. The test with a throttled writer is `tests/slow_link.rs`: a client that answers at once gets the whole intro (over 20 KB in the first three seconds), a client that never answers receives under 24 KB in four seconds, one that answers 5 s late gives up the same way, and a key reaches the menu within 1.5 s with under 16 KB sent after it. Limits of the measurement: a userspace proxy, not `tc`, and the proxy's receive buffer is large, which is exactly why a write-latency guard cannot see this link.

### Canvases, charts and images

- **Canvases** draw at every depth and in the ASCII tier. Truecolor, 256 and 16 colours draw `▀ ▄ █` or braille with the colour quantised to the depth; mono draws shades (`░▒▓█`) for half-blocks and the braille glyph with no colour; the ASCII tier draws a brightness ramp (` .:-=+*#%@`) or a dot-density ramp (` .:+#`) and never a block or braille glyph (D18).
- **Charts** take their colours from palette roles (accent for lines and bars, muted for tracks, success, warning and error for a gauge's tone) and have no way to ask for bold or reverse. A braille dot is drawn in the RGB the accent role stands for. **Labels are content**: every label goes through the shared sanitizer and is clipped to its column (a test feeds escape sequences and the reserved glyphs).
- **Image to cells** is for sysop-uploaded files only (a logo, a member avatar once a sysop enables avatars). Nothing here opens a URL, so there is no SSRF surface. The decoder is the `png` crate (pure Rust, image-rs, MIT or Apache-2.0); **only PNG** is accepted (a sysop converts a JPEG once). `Limits::default()`: 256 KiB of file, 1024 pixels a side, 1,000,000 pixels, 250 ms to decode, and a decoder memory cap derived from the pixel cap. The IHDR is read **by hand before the decoder runs**, so a header that claims a huge picture is refused without any pixel buffer; the cap is checked again on what the decoder parsed. Truncated, corrupt, interlaced-with-no-data and "bomb" files are errors, never panics. `decode_png` is CPU work bounded by the caps; call it on a blocking thread.
- **Not built, and why.** The widget system (UI-10) draws its sparklines, bars and area charts with `ui/charts` (the private `widgets/gfx.rs` is gone); `BarChart` and `Gauge` have no caller yet, and the dashboard (UI-4) will use them. The widgets' area chart is now a connected filled line rather than stepped columns, and a zero value sits on the baseline. No sysop screen uploads a logo yet, so `ui/image` has no caller; its tests are the only exercise. The `Wipe` and `Reveal` effects on other screens, and `glow` on the lightbar's selected row, are API only.

### Verified

Unit tests (`ui/fx`, `ui/canvas`, `ui/charts`, `ui/image`, `ssh/screen/fx_tests.rs`) cover the gate, timing, the three budgets, motion off, the final frame, the interrupting key, the marker cell and reserved style, every tier and depth, goldens for the canvases and charts, hostile board names and labels, and hostile images. A real OpenSSH 9.6 walk captured the intro, an interrupting key, the menu sweep, the typed goodbye and a motion-off session ([qa/ui-8-walk.md](qa/ui-8-walk.md)). **Not verified:** terminals other than xterm-like ones (the walk used `pyte`), a real slow link (the slow-send guard is unit-tested through `slow_link` but no throttled connection was run), telnet, and a screen reader.

## Screens (UI-4, D127)

Status: **Implemented and tested; walked over real OpenSSH 9.6** ([walk](qa/ui-4-walk.md), [screenshots](qa/ui-4-screens/)). Code: `crates/server/src/ui/screens/` (`reader`, `list`, `nick`, `motion`, `side`, and the test-only `matrix`), `chat/render.rs`, `forums/draw.rs`, `mail/draw.rs`, `members/{mod,panes}.rs`; `ui::panes` (the split plan). Not walked: telnet, other terminals, a screen reader, Postgres for the new data (it adds none).

Every converted screen is drawn in the frame ([UI-1](#frame-tables-popups-and-colour-depth-ui-1-d109)): the header bar, the status bar with the marker cell, a body, and no screen draws its own bars. Business rules stay in core and `core::access`; these modules only draw what a service hands them, sanitize content again with the shared sanitizer, and never use the reserved marker style.

### What changed on each screen

| Screen | What it is now | Keys (unchanged unless named) |
|---|---|---|
| **Forum lists** (boards, threads, search results) | The UI-1 tables, now under the board's **`area` art** (UI-3's slot, in the rows beyond 14), with a sort mark in the header and `o sort` in the status bar | `o` cycles the sort: each column ascending, then descending, then the next column. A paged list sorts **the page on screen**; the cursor, digits and Enter follow the order drawn |
| **Reader** (forums and mail) | A **tab strip** (a tab per post of the thread, a window of them around the current one with `<`/`>` when there are more, and `n of N`; mail shows its breadcrumb), a two-row **header block** (subject, then `From <handle>` coloured by hash and the date), a bordered text box titled with the board, **quote blocks** (a line starting `>` is drawn with a bar per level, italic in the quote colour, and keeps its bar on every wrapped row), a **scrollbar** and the pager's prompt. The keys moved from the text to the status bar's hints; a removal confirmation is a popup | `Space`/`b` page, `n`/`p` post, `r` reply, `c` quote, `x` remove (forums); `r`, `c`, `d`, `x` (mail) |
| **Chat** | **Three panes**: the rooms the member may join on the left (the current one marked), the conversation in the middle with the input in its own box under it, and **who is here** on the right (a dot and the handle, coloured by hash). The room list is a table beside a Commands pane; naming a room, closing one and a refusal are popups. Below 70 columns the rooms pane goes, below 56 the who pane goes, and the compact layout shows the conversation alone. A scrollbar appears when the transcript is longer than the pane | `/w`, `/who`, `/leave`, `/help`, Enter, Esc as before |
| **Mail inbox** | A table (who from, subject, sent time in UTC) with the unread asterisk, a count footer, `o sort`, the paging keys and a popup for the delete confirmation and for problems | `c` write, `d` delete, `n`/`b` pages, `1`-`9` and Enter |
| **Members list** | A table with its own key column, a **highlight you move with the arrows, Page Up/Down, Home and End**, and `o` to sort (by handle or name). Core still numbers the rows and opens one on its digit, so Enter is turned into the highlighted row's digit and the sorted list opens the member it shows | `/`, `n`/`p`, `c`, digits as before |
| **Community** | **Last callers** and **polls** are tables; **one-liners and the wall** are a pane of entries with each handle coloured by hash, drawn through the string table's own entry template; an **open poll** draws its counts as bars (only when core says this member may see counts, and it says in words what it hides). Everything else (a profile, the profile and poll forms) is its lines in a pane | as before |
| **Home** (menu) | Unchanged in the pane layouts (UI-10's widgets). A dashboard layout the window is too small for **collapses into one line** under the menu: the fallback sentence, then `3 online · 2 unread · 7 new`, only the figures the member may see | as before |
| **Split layout, second pane** | A real **chat pane** (each room the member may join, how many are in it and who) above a real **mail pane** (unread of total and the newest messages: who from, subject, how long ago). A pane the member cannot reach is not drawn and the other takes its room; below 16 rows, or with neither, the pane is the widget stack as before | as before |

### Nick colours

`ui::screens::nick`: a handle gets one of eight colours from a hash of its lower-cased name (the theme's own `accent`, `accent2`, `info`, `success`, `warning`, `link`, `key` and `quote` roles, so a nick is as readable as the style's other text). It is a foreground colour only: **no modifier is ever set, reverse video least of all**, and mono gets no colour. Colour is a hint, never the way to tell who said something: the handle is always in the text.

### Motion

The wipe between menus and the other effects are UI-8's engine ([below](#effects-and-graphics-ui-8-d124)). UI-4 adds the **blinking cursor**: an input line's cursor (`_`) goes dark for half a second and lit for half a second (`ui::screens::motion`, `Look::cursor`). It is a character in the line, not a style. It runs only when motion is allowed (`fx::allowed`: the member wants it, the board and the style allow it, and the session is not in accessibility mode); off, the cursor is a steady `_` and **there is no timer at all**. The shell wakes for it at most twice a second, and only while an input is on screen, far under the D49 cap of 20 redraws a second (a test asserts it).

### The geometry that did not move

The reader's page size (`ui::pager::text_rows`/`text_width`) and its drawing both come from `ui::screens::reader`, so a page of text is never cut. The list keys are `ui::screens::list::pick` (shared by forums, mail and the members list): the FIX-M3-1 keys. The split plan is a pure function of the body (`panes::plan`), tested not to overlap or leave the body. **Hooks for pane drag and split chat (UI-9's remaining work):** `Plan::divider` is the grip to drag, `Plan::chat` and `Plan::mail` are the two panes' rectangles, and `side::chat_pane`/`mail_pane` draw into any `Rect`, so "chat beside any screen" calls them for another screen's second pane.

### Verified

- **Render matrix** (`ui::screens::matrix`, golden files in `ui/screens/snapshots/`): fifteen screens (home, forum boards, threads, results and reader, mail inbox and reader, chat room and room list, members list and profile, last callers, wall, polls, one poll) in **all seven layouts at 80x24, 100x30 and 132x43, in Unicode mono and the ASCII tier**; a render equal to the classic one is written `= same as classic`, so the files show which layouts really change a screen. For every cell of every render: the status bar's marker cell is the reserved style (bold plus reverse) with no colour, nothing else has reverse video, mono draws no colour, and the ASCII tier is seven-bit. The home golden of the **split layout changed on purpose** (`ui/snapshots/split.txt`: the second pane is now the chat and mail panes); no other existing golden changed.
- **No-leak** (`ui::screens::tests::panes`): the split panes and the collapsed dashboard line draw only what the widget data holds, so a member without mail or chat sees neither pane nor figure; hostile handles, subjects and room names (escape sequences, a lookalike of a marker, a private-use glyph, a right-to-left override) never reach a cell and never carry reverse video. The data itself is gated by `WidgetId::reachable_by`, the store suites UI-10 added, and `core::mail::inbox` (which checks `mail.read` itself).
- **Motion** (`ui::screens::tests`, `ssh::screen::tests::frame`): the blink is lit and dark on the half seconds, only while an input is on screen; motion off and accessibility mode (with motion on) give no timer and a steady cursor; the blink is under the redraw cap.
- **Unit tests** for the reader geometry and tab window, the nick hash (stable, case-blind, never reversed), the sort cycle (each column both ways, stable, a permutation of the rows), the members list highlight (Enter opens the highlighted row by its digit, a sorted list still opens the right member), the three-pane chat at every width, and the roster following joins and leaves in core.
- **Integration suites** (`tests/{access_paths,chat,forums,mail,members}.rs`) pass with their expected text updated only where a screen deliberately changed: lists are tables (`1 user00` for `1. user00`), the reader shows `2 of 2` and a quote as a bar instead of `Post 2 of 2 in General` and `> `, the inbox is a table, the chat hints moved to the status bar, a problem on the chat list is a popup, and the one-liner and wall lines are several coloured runs.
- **Real OpenSSH 9.6 against the real binary**: see [qa/ui-4-walk.md](qa/ui-4-walk.md).
- **Not built:** tabs that split a thread's *text* (a long post pages with the pager; the tabs are the thread's posts), a chat transcript scrollback (core keeps 200 lines and has no scroll keys), sorting the whole of a paged list (it sorts the page on screen), a live chat pane while the menu has the keys (UI-9 puts chat beside any screen; the split pane lists rooms and who is in them, not lines), pane dragging and chat beside any screen (the rest of UI-9; UI-9's mouse already clicks table rows, which turn into the same keys), accessibility mode (M5).

## Forum screens (M3-E, D83)

`server::forums` draws four screens in the framed content box with the reserved status row: the **board list** (`Up`/`Down` and `Enter`, `1`-`9` to open, `s` search, `n` new board for `forum.manage`), the **thread list** (`w` write, `n`/`b` next/previous page, `s` search, `a` let a member into a restricted board for `forum.manage`), the **reader** and **search results**. The reader shows one post at a time and pages it with the pager (`Space`/`Enter` next page, `b` page back; with pause off the first screen is shown and the screen says how many lines are not); `n`/`p` move between posts, `r` replies, `c` replies quoting the post, `x` (then `y`) removes it for `forum.moderate`, `Esc` or `q` goes back. Posting opens the M3-B editor with the draft for that board or thread; a new thread asks for its subject first. Every key is described on its screen in words. Ctrl-C and Ctrl-D log off from every forum screen, from the editor after storing its changed draft. Member text (subjects, handles, bodies, snippets) is drawn only inside the content box in plain style; a test draws hostile names, subjects and 3,000-character lines at three sizes and asserts that nothing outside the status row carries the marker style. No forum screen shows a privacy notice or a lock (D78).
