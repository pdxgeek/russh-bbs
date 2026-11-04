# UI refresh and board customization (plan)

Status: **Approved by the owner, 2026-11-04** ("ratatui can make that UI look way better... there needs to be ways to customize the BBS so it looks cool"). Slices UI-1 to UI-5 are in [coordination.md](coordination.md). Nothing here is built yet.

## Goals

1. **Look like a modern terminal app, feel like a BBS.** Use what ratatui offers:
   - rich layouts: split panes, tabs, tables with headers, scrollbars, popups and modals;
   - live widgets: gauges, sparklines, sparkline-style activity bars, a clock;
   - colour: 16, 256 and truecolor, with graceful fallback.

   Keep the classic BBS soul: ANSI-style art headers, hotkeys, a status bar.
2. **Let every board look like its own.** A sysop can give the board a name, art, colours, borders, layout and wording without touching code. Members can pick from the board's themes.
3. **Keep what must not change:**
   - security markers (`UNENCRYPTED`, `LOW-SECURITY` and future ones) keep their reserved style and chrome-only position (D39);
   - every screen works at 80x24 and in the ASCII tier (D18);
   - all non-chrome text is sanitized;
   - accessibility mode stays a plain, readable path (M5).

## What changes on screen

- **Frame:** a top **header bar** (the board name or logo, the area you are in, and a clock) and a bottom **status bar** (hotkeys for this screen, unread counts, who's online). The current status row becomes part of the status bar, and security markers keep their reserved cell at its left.
- **Main menu:** a two-pane home with the menu on the left and a **dashboard** on the right:
  - last callers;
  - new posts since your last call;
  - unread mail;
  - who's in chat;
  - today's one-liner;
  - an activity sparkline for the board.

  At 80x24 the dashboard collapses into a single line.
- **Lists:** forum boards, threads, members, mail and search results become **tables**: sortable columns, a highlighted row, a scrollbar, and paging with the FIX-M3-1 keys.
- **Reader:** a message view with a header block (author, date, board), quote blocks in a distinct style, and a scrollbar. Long threads can be split into tabs.
- **Chat:** a three-pane layout. The room list and the who's-online list sit beside the main conversation, and the input line sits at the bottom. Nicks are coloured by hash, never using the reserved style.
- **Popups:** confirmations, errors and help appear in centred modal boxes instead of inline lines.
- **Motion, used sparingly:**
  - a typewriter reveal for the login banner;
  - a short transition between menus;
  - a blinking cursor in inputs.

  Members can turn motion off, and it is always off in accessibility mode.
- **Doors:** the framework's `Canvas` gains the same widgets, so the game UI slices (S3, E3, N3, L3, H3) use them.

## Customization (sysop and member)

### Theme packs v2 (extends D82)

A theme becomes a directory (or one TOML file) the sysop drops into `themes/`. It can set:

- **Palette:** named colours for about 20 roles (header, status, menu, selection, borders, links, quotes, timestamps, nick colours, success, warning, error and so on). Each colour is given as truecolor, a 256-colour index or a 16-colour name, with automatic fallback down the tiers.
- **Borders:** plain, rounded, double, thick, ASCII, or none, per element (frame, panes, popups).
- **Layout preset:** classic, lightbar, compact (as now), plus **dashboard** (the new two-pane home) and **wide** (for 120 columns and up).
- **Strings:** the string table, as now, with the reserved keys still refused.
- **Art:** see below.
- **Motion:** on or off by default.

Theme files remain untrusted input: size caps, a schema, refusal with a logged reason, and they never stop boot (D82). A theme cannot give anything the reserved marker style (bold plus reverse in the marker cell), and a test checks every shipped theme and a hostile one.

### Board art

- **Placement:** a sysop can place art in fixed slots: the login banner, the main menu header, the logoff screen, each top-level area's header, and the door menu.
- **Accepted formats:**
  - **`.ans` files (CP437 ANSI art)**, the classic BBS format. They are parsed by our own small parser into styled cells, never passed through to the terminal raw. Only SGR colour and attributes and cursor-forward are understood. Everything else (cursor movement beyond the art box, OSC, DCS, clears, title changes) is dropped. CP437 is mapped to Unicode, with an ASCII fallback for the ASCII tier.
  - **A simple text-plus-colour format of our own (`.art.toml`)**, for sysops who would rather not draw in an ANSI editor.
- **Safety:** art is a sysop asset, not member content, but it is still treated as untrusted at load:
  - size caps (for example 80x25 for a header, 132x50 for a full screen);
  - no reserved style and no marker glyphs (they become U+FFFD, as for content);
  - art never sits in the marker cell.
- **Tiers and fallback:** each slot falls back to the board name in large letters (a built-in figlet-style font) when no art is set, or when the terminal is too small or in the ASCII tier without a CP437-safe version.

### Board identity

A `[board]` config section sets:

- the name, tagline and a short location line, shown in the header and the login banner;
- the default theme and layout for new members;
- the menu order and which areas are shown;
- custom labels for the areas ("Message Bases" or "Forums", for example);
- the board's "since" date for the header;
- the logoff quote list.

### Member choices

Members pick from the board's themes and layouts (as now), plus three new settings: motion on or off, colour depth (auto, 16, 256 or truecolor), and borders (theme default or ASCII).

### A sysop preview screen

Under Sysop tools, a **theme preview** renders each installed theme against sample screens (menu, list, reader, chat, popup), at the current size and tier. Load errors appear there with their reasons, so a sysop can check art and colours without logging in as a member.

## Constraints and tests

- **Renders everywhere:** every screen has render tests at 80x24, 100x30 and 132x43, in each tier and layout, and with every shipped theme.
- **Markers are protected:** a hostile theme and hostile art cannot restyle, hide or imitate a security marker (the existing marker tests, extended).
- **Art parser hardening:** fuzz and property tests for the `.ans` parser, plus tests that unsupported sequences are dropped.
- **Cost stays bounded:**
  - motion cannot raise redraws above the D49 cap, and motion off means no timer-driven redraws;
  - the truecolor fallback is tested against a terminal that advertises only 16 colours.
- **The plain path keeps working:** accessibility mode (when M5 builds it) bypasses art, motion and multi-pane layouts.

## Slices

1. **UI-1: frame and widgets.** Header and status bars, popups, tables with scrollbars, and the colour-depth fallback. Shared widgets live in `crates/server/src/ui`. Done when the main menu, the forum lists and the members list use them at every size and tier.
2. **UI-2: theme packs v2 and board identity.** Palette roles, borders, layout presets (including dashboard and wide), the `[board]` config, and member settings (motion, colour depth, borders). Ships three new themes that look distinct (for example a phosphor green terminal, a neon synth night, and a calm paper light theme).
3. **UI-3: board art.** The `.ans` parser, the `.art.toml` format, art slots, the large-letter fallback, and the sysop theme preview. Ships one original art set per new theme.
4. **UI-4: screens.** The dashboard home, the reader, three-pane chat, mail and the community screens moved onto the new widgets, plus motion.
5. **UI-5: door canvas.** The framework `Canvas` gains the widgets, so the game UI slices can use them.

The game UI slices (S3, E3, N3, L3 and H3) wait for UI-5. The engine and content slices do not.
