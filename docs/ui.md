# Terminal UI: menus, layouts, themes, icons

Status: **Planned.**

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
- Unicode-tier markers are drawn inside the reserved delimiters `⟦ ⟧`. The sanitizer replaces those exact codepoints and the four marker glyphs in all content; Nerd Font glyphs are private-use and replaced likewise. **Lookalikes are not blocked** (`〚☉〛`, `◉`, fullwidth brackets), so on a terminal without styling the Unicode tier also depends on the chrome-only position (D44).
- **ASCII-tier markers cannot be made unforgeable by text** (content can type `E2EE`). Draw them only in a chrome-only position (status line or fixed gutter). On a no-color terminal that position is the only cue, so a screen that cannot guarantee one must say "security markers unavailable" instead of drawing them. See D39.

## Accessibility

A compact, text-only layout with no icons, no art, and screen-reader-friendly output is selectable from any screen and is the fallback whenever the terminal cannot be probed.

## Implemented / Verified

**Waiting room** (`russh-bbs-core::waiting_room` rules and view model, `russh-bbs-server::waiting_room` drawing), M1-C.

- **Implemented:** the page set per registration mode, menu keys, form input (Enter next/send, Backspace, Esc back, Ctrl-C leave), and the drawing: a bordered box with a heading, the banner, menu or form, a `Notice:` / `Problem:` line (words, not only colour) and a footer hint. Every fixed string is in `waiting_room::strings`, not in drawing code, so a theme or translation can replace them later. Banner and rules come from settings and are sanitized.
- **Security cues:** the rules page carries a fixed privacy line ("the server can read everything except PGP-encrypted mail and files you encrypt yourself") that settings cannot edit; no lock or privacy glyph is shown anywhere in the waiting room.
- **Not implemented:** theme and layout selection for these screens (strings are themeable by design, the layer is M3); scrolling of long rules or banner text (clipped to the window); non-ASCII typing; the real request, redeem, apply and create-account flows (M2).
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
  - **Login screen** (signed in as the sanitized handle, last login in UTC or "first login", the stored tier, color and pause), a **placeholder home** (`s` settings, `l` log off), the **settings screen** (`w`/`h` size with range-checked numbers and empty = automatic, `c` color, `p` pause, `i` icon check, Esc back) and the **logoff screen**, which is also printed on the main screen after the alternate screen is left.
  - Every fixed string is in `terminal_settings::strings`; nothing shows a lock or the word "private".
- **Security chrome:** no security marker is drawn on these screens. When one is needed it goes through `Rendered::to_terminal()` / `chrome_style()` in a chrome-only position (D39), not through the content styles in `settings::render`.
- **Not implemented:** pause changes no output (nothing pages yet); the tier changes only the border and the glyph test today, because there are no other icons on screen; layout and theme selection, and the real menu tree in place of the placeholder home (M2/M3); a color check screen; per-terminal (rather than per-account) overrides. A stored size override that does not fit the current terminal garbles the layout until it is cleared (`s`, `w`, Enter, `h`, Enter, which works blind).
- **Verified:** unit tests (rules, glyph answers, fallback, the whole flow, the string filler, the UTC formatter), render tests on a ratatui test backend (no color cells with color off, ASCII border for the ASCII tier, registry glyphs on screen, tiny and huge windows), store tests through the core traits, a real russh client (claim, first-login glyph test, stored answer, reconnect without it, Esc fallback, settings persistence, resize storm, an unknown key still sees only the waiting room), and OpenSSH under `pyte` and `tmux`.
- **Not verified:** other terminal emulators; a Nerd Font terminal; a screen reader.
