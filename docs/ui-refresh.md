# UI refresh and board customization (plan)

Status: **Approved by the owner, 2026-11-04** as its own milestone, **M3.5: Look and feel** ([milestones.md](milestones.md)). The owner asked for it because "ratatui can make that UI look way better" and "there needs to be ways to customize the BBS so it looks cool". Slices UI-1 to UI-7 and the M3.5 QA gate are in [coordination.md](coordination.md). Nothing here is built yet.

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

## North star: a next-gen BBS (owner, 2026-11-04)

"Let's really flex what Rust and ratatui can do. Think of it this way: we're a next-gen BBS." The board should make people say "that's a terminal?". Concretely:

- **Graphics in the terminal, safely.** Braille and half-block canvases for real graphics:
  - an animated login intro (original demoscene-style effects: plasma, starfield, fire, scrolling greetings);
  - charts and sparklines on the dashboard;
  - maps in the doors;
  - sysop logos and member avatars converted to half-block cells.

  Everything is drawn as ordinary styled cells, so no terminal graphics protocol (sixel, kitty, iTerm) is ever sent. That keeps the sanitizer's guarantees and works over plain ssh and telnet. Graphics protocols stay out unless the owner decides otherwise later.
- **Effects:** truecolor gradients, shaded panels, fades and wipes between screens, glow on the selected item, and a typewriter reveal. An effect engine runs at a capped frame rate (well under the D49 redraw cap; tuned per session, and off when the member turns motion off or the terminal is slow). An existing ratatui effects crate may be used if it passes D47 and `cargo deny`.
- **Interaction like a desktop app:**
  - **mouse support** (click menus and list rows, scroll with the wheel, drag pane dividers), parsed by our own bounded input parser;
  - a **command palette** (`Ctrl-K`): fuzzy-find any area, board, thread, member, room or door and jump there;
  - **toasts** for new mail, whispers and mentions, which never cover the marker cell;
  - **split-screen multitasking**: keep chat open in a side pane while you read the forums.
- **Live everywhere:** a who's-online list, the chat sidebar, a news ticker in the header, unread counters and door events update in real time, without a key press.
- **Responsive:** layouts reflow from 80x24 to ultrawide. A 200-column terminal gets a three-column dashboard; 80x24 gets a tight classic view. Same code, no separate skins.
- **Fast:** diffed rendering, so only changed cells are sent. Effects are budgeted per session, so hundreds of members can be online with animation on, and a benchmark in CI-local checks frame cost and bytes sent per frame.
- **Still a BBS:** hotkeys, ANSI art, the sysop's voice. Everything stays usable from a keyboard alone, and accessibility mode stays plain.

## Several styles, several layouts, widgets you switch on and off (owner, 2026-11-04)

Not one cool look: **many, all selectable.** Style, layout and widgets are three independent choices, so any style works with any layout and any set of widgets.

### Styles (theme packs)

The board ships **at least six distinct styles**, each a complete theme pack (palette, borders, art, motion defaults). The names below are working titles, and every piece of art is original:

| Style | Feel |
|---|---|
| **Phosphor** | A green CRT: scanline shading, a soft glow, monospace art |
| **Neon Night** | Synthwave: magenta and cyan gradients, a chrome logo, fades |
| **Paper** | Calm and light: an ink-on-paper palette, thin rounded borders, no motion |
| **Amber Terminal** | Warm amber on black, thick borders, a typewriter reveal |
| **Deep Space** | Starfield backgrounds, cold blues, braille nebula art |
| **Classic ANSI** | 16-colour, CP437 block art and double borders, like a 1994 board done perfectly |

The existing default, amber, green and contrast themes stay. Sysops can add more styles, import and export them (UI-7), and set the default; members pick one in the theme gallery, live.

### Layouts

**At least five layouts**, each a real arrangement of panes, not a recolour:

| Layout | Arrangement |
|---|---|
| **Classic** | A hotkey menu in a centred box, as now |
| **Lightbar** | A highlighted menu you move with arrows, as now |
| **Compact** | Dense single-column, for small terminals, as now |
| **Dashboard** | Menu on the left, a widget grid on the right |
| **Wide** | Three columns for 120+ columns: navigation, content, widgets |
| **Focus** | Content full-screen with a slim header, and widgets in a pop-over (`Tab`) |
| **Split** | Any screen beside a second pane (chat, mail or a widget), with a draggable divider |

Every layout reflows to the terminal's size and still works at 80x24. Below a layout's minimum size it falls back to a smaller layout and says so.

### Widgets

A **widget** is a small live panel the layouts place in their widget areas (the dashboard grid, the wide right column, the focus pop-over and the split pane). Widgets are built in, in core and server code. They are not plug-ins, and each one reads only what its viewer may reach through `core::access`.

- **Launch set:**
  - who's online;
  - last callers;
  - new posts since your last call;
  - unread mail;
  - the chat sidebar (a live room);
  - today's one-liner;
  - the graffiti wall;
  - the current poll;
  - the board activity sparkline;
  - a clock and calendar (with the event scheduler's next events);
  - a door standings ticker;
  - the news ticker (RSS feed headlines);
  - the sysop's message of the day;
  - your credits.
- **Who controls what:**
  - **the sysop** decides which widgets exist on the board, the default set and order for each layout, and can lock a widget on (for example the message of the day) or off;
  - **members** switch widgets on or off, reorder them and resize them within what the sysop allows, from a Widgets screen in settings or by dragging with the mouse (UI-9). Their choice is saved per layout.
- **Rules:**
  - a widget never covers the marker cell;
  - a widget that has nothing to show collapses;
  - each widget has a size class (small, medium, wide) and a refresh budget, so live widgets cannot blow the D49 redraw cap;
  - in accessibility mode the widgets become a plain list on their own screen;
  - in the ASCII tier every widget has a text form.
- **Settings storage:** widget settings are stored per member, in the store on both backends, with a forward migration.

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
- **Doors get all of ratatui** (owner, 2026-11-04). A door renders any ratatui widget into its own off-screen buffer, and the framework cleans every cell as it composites it onto the screen: the sanitizer runs on each symbol, the reserved marker style is removed, and everything is clipped to the door's area. Doors can use colours and every widget, but they can never forge a marker or draw outside their box. The game UI slices (S3, E3, N3, L3, H3) build on it. **Built (UI-5, D103):** off-screen buffer, per-cell cleaning, colour depth and ASCII-tier stand-ins; mouse (UI-9) and effects (UI-8) are not yet in the door API.

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

### Sysop menus and pages

- **`menus.toml`** in the data directory can reorder the built-in menu items, rename them, change their hotkeys, hide them, and group them into nested submenus. The menu tree and its actions stay defined in core (D54): a menu file can only rearrange what exists, and `core::access` still decides what each member can reach.
- **Pages:** sysop-written bulletins and info screens (house rules, history, links, a "who's who"), made of art plus text, and linked from any menu.
- Menu files are validated with a schema and size caps. A bad one is refused with a logged reason, and the default menu is used.

### Theme gallery, import and export

- **Gallery:** members open a theme gallery from settings. It previews each theme on a sample screen, and they pick one live.
- **Import and export:** sysops can export a theme pack (palette, borders, layout, strings and art in one file) and import one, for example from another board. Imported packs pass the same validation as theme files, and nothing in a pack executes.

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
2. **UI-2: theme packs v2, layouts and board identity.** Palette roles, borders, the seven layouts above, the `[board]` config, and member settings (motion, colour depth, borders). Ships the six styles above, as complete packs (their art lands in UI-3).
3. **UI-3: board art.** The `.ans` parser, the `.art.toml` format, art slots, the large-letter fallback, and the sysop theme preview. Ships one original art set per new theme.
4. **UI-10: widget system** (runs before UI-4). The widget trait and registry, the launch set of widgets, sysop defaults and locks, the member Widgets screen, per-member storage and refresh budgets.
5. **UI-4: screens.** The dashboard home, the reader, three-pane chat, mail and the community screens moved onto the new widgets, plus motion.
5. **UI-5: full ratatui for doors.** An off-screen buffer per door, composited with per-cell cleaning; any widget and any colour; mouse events and effects added as UI-8 and UI-9 land. The game UI slices use it.
6. **UI-6: sysop menus and pages.** A `menus.toml` reorders, renames, hides and nests the built-in menu items, and adds sysop pages (bulletins and info screens with art). It cannot add actions or bypass access checks.
7. **UI-7: theme gallery, import and export.** Members preview and pick a theme from a gallery; sysops import and export theme packs (one file, art included).
8. **UI-8: effects and graphics.**
   - the effect engine (fades, wipes, glow, gradients, typewriter) with a frame budget;
   - braille and half-block canvases;
   - the animated login intro (original effects);
   - image-to-cells conversion for sysop logos and member avatars (sysop-uploaded files only, size-capped, decoded by a maintained pure-Rust decoder);
   - dashboard charts.
9. **UI-9: interaction.**
   - mouse support (bounded parser; click, scroll and pane drag);
   - the command palette with fuzzy search over everything the member may reach (filtered through `core::access`, so it never leaks);
   - toasts;
   - split-screen multitasking (chat beside any screen);
   - the live header ticker.
10. **M3.5 QA gate.**

The game UI slices (S3, E3, N3, L3 and H3) wait for UI-5. The engine and content slices do not.
