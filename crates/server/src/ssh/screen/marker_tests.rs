#![allow(clippy::unwrap_used, clippy::expect_used)]
//! FIX-M35-1 (QA M3.5 M1, D39): a staff session of a board that has telnet on shows the
//! `LOW-SECURITY` marker at the first cell of the last row on **every** screen the menu can reach,
//! drawn in the reserved style, and that style appears nowhere else. The walk reads the bytes the
//! terminal gets (not the widgets), so it sees what a member sees, over both transports.

use std::sync::Arc;

use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::{LayoutId, MemberFlow, TerminalSettings};
use russh_bbs_door::sim::MemoryDoorStore;

use super::{Content, Key, KeyEffects, PtySize, RenderCap, Screen};

/// One cell of the terminal the bytes were replayed onto.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    bold: bool,
    reverse: bool,
}

const BLANK: Cell = Cell {
    ch: ' ',
    bold: false,
    reverse: false,
};

/// A terminal that understands what ratatui's crossterm backend and the telnet footer send:
/// absolute moves, forward moves, erase, and the bold and reverse attributes. Everything else
/// (colours, modes) is skipped.
struct Grid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    row: usize,
    col: usize,
    bold: bool,
    reverse: bool,
}

/// Terminal columns taken by `c`: two for the wide ranges the games and the emoji use.
fn width_of(c: char) -> usize {
    match u32::from(c) {
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![BLANK; cols * rows],
            row: 0,
            col: 0,
            bold: false,
            reverse: false,
        }
    }

    fn at(&self, row: usize, col: usize) -> Cell {
        self.cells[row * self.cols + col]
    }

    fn put(&mut self, c: char) {
        if self.row < self.rows && self.col < self.cols {
            self.cells[self.row * self.cols + self.col] = Cell {
                ch: c,
                bold: self.bold,
                reverse: self.reverse,
            };
        }
        self.col += width_of(c);
    }

    fn sgr(&mut self, params: &str) {
        let numbers: Vec<u32> = params.split(';').map(|p| p.parse().unwrap_or(0)).collect();
        let mut at = 0;
        while at < numbers.len() {
            match numbers[at] {
                0 => {
                    self.bold = false;
                    self.reverse = false;
                }
                1 => self.bold = true,
                7 => self.reverse = true,
                22 => self.bold = false,
                27 => self.reverse = false,
                // An extended colour: the numbers after it are not attributes.
                38 | 48 => {
                    at += match numbers.get(at + 1) {
                        Some(5) => 2,
                        Some(2) => 4,
                        _ => 0,
                    };
                }
                _ => {}
            }
            at += 1;
        }
    }

    fn erase(&mut self, params: &str, kind: char) {
        let mode = params.parse::<u32>().unwrap_or(0);
        let range = match (kind, mode) {
            ('J', 2 | 3) => 0..self.cells.len(),
            ('J', 0) => (self.row * self.cols + self.col).min(self.cells.len())..self.cells.len(),
            ('K', 2) => self.row * self.cols..(self.row + 1) * self.cols,
            ('K', 0) => {
                (self.row * self.cols + self.col).min(self.cells.len())
                    ..((self.row + 1) * self.cols).min(self.cells.len())
            }
            _ => 0..0,
        };
        for cell in &mut self.cells[range] {
            *cell = BLANK;
        }
    }

    /// Replay `bytes` onto the grid.
    fn feed(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' if chars.peek() == Some(&'[') => {
                    chars.next();
                    let mut params = String::new();
                    for next in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            self.control(next, &params);
                            break;
                        }
                        params.push(next);
                    }
                }
                '\u{1b}' => {
                    chars.next();
                }
                '\r' => self.col = 0,
                c if c.is_control() => {}
                c => self.put(c),
            }
        }
    }

    fn control(&mut self, kind: char, params: &str) {
        let first = |default: usize| {
            params
                .split(';')
                .next()
                .and_then(|p| p.parse::<usize>().ok())
                .unwrap_or(default)
        };
        match kind {
            'H' | 'f' => {
                let mut parts = params.split(';');
                let row = parts.next().and_then(|p| p.parse::<usize>().ok());
                let col = parts.next().and_then(|p| p.parse::<usize>().ok());
                self.row = row.unwrap_or(1).saturating_sub(1);
                self.col = col.unwrap_or(1).saturating_sub(1);
            }
            'C' => self.col += first(1),
            'D' => self.col = self.col.saturating_sub(first(1)),
            'A' => self.row = self.row.saturating_sub(first(1)),
            'B' => self.row += first(1),
            'G' => self.col = first(1).saturating_sub(1),
            'J' | 'K' => self.erase(params, kind),
            'm' => self.sgr(params),
            _ => {}
        }
    }

    /// The text of a row, trimmed.
    fn row_text(&self, row: usize) -> String {
        (0..self.cols)
            .map(|col| self.at(row, col).ch)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// Every cell in the reserved marker style (bold and reverse together), as (row, col).
    fn reserved(&self) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        for row in 0..self.rows {
            for col in 0..self.cols {
                let cell = self.at(row, col);
                if cell.bold && cell.reverse && cell.ch != ' ' {
                    found.push((row, col));
                }
            }
        }
        found
    }

    fn dump(&self) -> String {
        (0..self.rows)
            .map(|row| self.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A member session of `role`, on the board's low-security mode if asked, over telnet if `telnet`.
fn session(
    ui: &Arc<crate::ui::Ui>,
    role: (&Slug, u8),
    settings: TerminalSettings,
    size: PtySize,
    telnet: bool,
    low: bool,
) -> Screen {
    let flow = MemberFlow::new(
        &sanitize_username("alice").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(1)),
            role.0,
            role.1,
            &Via::Account,
            Permission::ALL.iter().copied(),
        ),
        None,
        settings,
    );
    let mut screen = Screen::with_footer(
        size,
        RenderCap::default(),
        Content::Member(Box::new(flow)),
        Arc::clone(ui),
        telnet.then_some(SecurityMarker::Unencrypted),
    )
    .unwrap();
    screen.set_low_security(low);
    screen.set_accessible(true);
    screen
}

/// The board's UI with the doors it ships, built once per test.
fn board_ui() -> Arc<crate::ui::Ui> {
    Arc::new(
        crate::ui::Ui::standard()
            .unwrap()
            .with_doors(crate::doors::Doors::builtin().unwrap()),
    )
}

/// The doors the board ships, in list order.
fn door_count() -> usize {
    crate::doors::Doors::builtin().unwrap().all().len()
}

/// Run the door request the keys asked for against an in-memory store and give back the answer.
fn answer_door(screen: &mut Screen, effects: KeyEffects, store: &MemoryDoorStore) {
    let Some(request) = effects.door_request else {
        return;
    };
    let access = match &screen.content {
        Content::Doors(session) => session.flow.access_snapshot().clone(),
        _ => return,
    };
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(20_000).start()));
    let response = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(crate::doors::run(store, &access, &clock, request));
    screen.deliver_door(response);
}

/// Press `keys` one at a time, answer any door request, repaint the whole screen and replay the
/// bytes onto `grid`. Returns the screen as text.
fn press(
    screen: &mut Screen,
    grid: &mut Grid,
    size: PtySize,
    keys: &[Key],
    store: &MemoryDoorStore,
) -> String {
    for key in keys {
        let effects = screen.apply_keys(&[*key]);
        answer_door(screen, effects, store);
    }
    // A repaint of the same window draws every cell again.
    screen.set_size(size);
    grid.feed(&screen.render().unwrap().unwrap_or_default());
    grid.dump()
}

/// The widest the marker's glyph and label are drawn; the caption after it is plain text.
const MARKER_WIDTH: usize = 20;

/// What a frame must show: the marker in the first cell of `marker_row`, in the reserved style,
/// and the reserved style on no other row but the transport's `footer_row`.
fn check(grid: &Grid, marker_row: usize, footer_row: Option<usize>, expect_marker: bool, at: &str) {
    let reserved = grid.reserved();
    let outside: Vec<_> = reserved
        .iter()
        .filter(|(row, _)| Some(*row) != footer_row && !(expect_marker && *row == marker_row))
        .collect();
    assert!(
        outside.is_empty(),
        "{at}: the reserved style outside the status row: {outside:?}\n{}",
        grid.dump()
    );
    let in_marker_row = reserved.iter().any(|(row, _)| *row == marker_row);
    assert_eq!(
        in_marker_row,
        expect_marker,
        "{at}: marker row {marker_row}\n{}",
        grid.dump()
    );
    if expect_marker {
        let first = grid.at(marker_row, 0);
        assert!(
            first.bold && first.reverse && first.ch != ' ',
            "{at}: no marker in the first cell of row {marker_row}\n{}",
            grid.dump()
        );
        assert!(
            reserved
                .iter()
                .filter(|(row, _)| *row == marker_row)
                .all(|(_, col)| *col < MARKER_WIDTH),
            "{at}: more than the marker (glyph and label) is in the reserved style\n{}",
            grid.dump()
        );
    }
    if let Some(row) = footer_row {
        let first = grid.at(row, 0);
        assert!(
            grid.row_text(row).contains("read this session") && first.bold && first.reverse,
            "{at}: the telnet footer is gone\n{}",
            grid.dump()
        );
    }
}

/// The screens the menu reaches without the store, by the key that opens them and the keys that
/// leave them.
const MENU_SCREENS: [(&str, &[Key]); 8] = [
    ("menu", &[Key::Char('x')]),
    ("settings", &[Key::Char('s')]),
    ("members", &[Key::Escape, Key::Char('u')]),
    ("community", &[Key::Escape, Key::Char('y')]),
    ("invites", &[Key::Escape, Key::Char('i')]),
    ("sysop tools", &[Key::Escape, Key::Char('o')]),
    ("door list", &[Key::Escape, Key::Char('d')]),
    ("menu again", &[Key::Escape]),
];

/// Every combination of tier, layout, size and transport.
fn combinations() -> Vec<(Tier, LayoutId, PtySize, bool)> {
    let mut all = Vec::new();
    for tier in [Tier::Ascii, Tier::Unicode, Tier::NerdFont] {
        for layout in LayoutId::ALL {
            for (cols, rows) in [(80, 24), (132, 43)] {
                for telnet in [false, true] {
                    all.push((tier, layout, PtySize::from_request(cols, rows), telnet));
                }
            }
        }
    }
    all
}

fn settings_of(tier: Tier, layout: LayoutId) -> TerminalSettings {
    TerminalSettings::default()
        .with_tier(tier)
        .with_layout(layout)
        .with_motion(false)
}

/// The rows of the grid and the two rows that matter in it: the status row the screen draws and
/// the transport's footer under it, if any.
fn rows_of(size: PtySize, telnet: bool) -> (usize, usize, Option<usize>) {
    let rows = usize::from(size.rows());
    if telnet {
        (rows, rows - 2, Some(rows - 1))
    } else {
        (rows, rows - 1, None)
    }
}

/// The marker on every menu-reachable screen, the door list and a running door of each of the
/// six games, in every tier and layout at both sizes, over ssh and telnet.
#[test]
fn a_staff_session_shows_the_marker_on_every_screen_and_the_style_nowhere_else() {
    let store = MemoryDoorStore::new();
    let ui = board_ui();
    let doors = door_count();
    assert_eq!(doors, 6, "five games and the coin toss");
    for (tier, layout, size, telnet) in combinations() {
        let (rows, marker_row, footer_row) = rows_of(size, telnet);
        let mut screen = session(
            &ui,
            (&Slug::SYSOP, 100),
            settings_of(tier, layout),
            size,
            telnet,
            true,
        );
        let mut grid = Grid::new(usize::from(size.cols()), rows);
        grid.feed(&screen.enter().unwrap());
        for (name, keys) in MENU_SCREENS {
            press(&mut screen, &mut grid, size, keys, &store);
            let at = format!("{tier:?} {layout:?} {size:?} telnet={telnet} {name}");
            check(&grid, marker_row, footer_row, true, &at);
        }
        // A running door of each game, from the menu: `d`, its number.
        for door in 1..=doors {
            let mut screen = session(
                &ui,
                (&Slug::SYSOP, 100),
                settings_of(tier, layout),
                size,
                telnet,
                true,
            );
            let mut grid = Grid::new(usize::from(size.cols()), rows);
            grid.feed(&screen.enter().unwrap());
            let digit = char::from_digit(u32::try_from(door).unwrap(), 10).unwrap();
            let shown = press(
                &mut screen,
                &mut grid,
                size,
                &[Key::Char('x'), Key::Char('d'), Key::Char(digit)],
                &store,
            );
            let at = format!("{tier:?} {layout:?} {size:?} telnet={telnet} door {door}");
            assert!(
                matches!(&screen.content, Content::Doors(session) if session.screen.is_playing()),
                "{at}: the door did not open\n{shown}"
            );
            check(&grid, marker_row, footer_row, true, &at);
            // Keys inside the door redraw it; the marker holds through them.
            press(
                &mut screen,
                &mut grid,
                size,
                &[Key::Down, Key::Enter, Key::Char('1')],
                &store,
            );
            check(
                &grid,
                marker_row,
                footer_row,
                true,
                &format!("{at} after keys"),
            );
        }
    }
}

/// A plain member, and a staff member outside the mode, never see the marker; telnet keeps its
/// footer; the style is nowhere else.
#[test]
fn a_member_or_a_staff_session_outside_the_mode_shows_no_marker() {
    let store = MemoryDoorStore::new();
    let ui = board_ui();
    for (role, low) in [
        ((&Slug::MEMBER, 10), true),
        ((&Slug::MEMBER, 10), false),
        ((&Slug::SYSOP, 100), false),
    ] {
        for (tier, layout, size, telnet) in combinations() {
            let (rows, marker_row, footer_row) = rows_of(size, telnet);
            let mut screen = session(&ui, role, settings_of(tier, layout), size, telnet, low);
            let mut grid = Grid::new(usize::from(size.cols()), rows);
            grid.feed(&screen.enter().unwrap());
            for (name, keys) in MENU_SCREENS
                .iter()
                .chain(&[("door", &[Key::Char('d'), Key::Char('2')][..])])
            {
                press(&mut screen, &mut grid, size, keys, &store);
                let at = format!(
                    "{role:?} low={low} {tier:?} {layout:?} {size:?} telnet={telnet} {name}"
                );
                check(&grid, marker_row, footer_row, false, &at);
            }
        }
    }
}
