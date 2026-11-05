//! The dock on a whole `Screen` (UI-W): chat beside each main screen in every layout, at
//! 80x24 and 132x43, in mono and in the ASCII tier; the divider by keys and by mouse; the
//! marker and the status row; the redraw budget. The terminal output is read back through a
//! small grid reader so the tests can see the marker cell's attributes.
#![allow(clippy::unwrap_used)]

use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::chat::{ChatResponse, Entered, Event, Line, Room, RoomId, clean_line};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_core::terminal_settings::{LayoutId, MemberFlow, TerminalSettings};

use super::{Key, PtySize, RenderCap, Screen};
use crate::ui::Ui;
use crate::ui::dock;
use crate::ui::input::mouse::{Button, Kind, Mouse};

/// One cell as the grid reader saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The screen as the terminal would hold it after `bytes`: cursor moves, SGR bold and reverse,
/// clears and text. Colours and every other sequence are skipped.
struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl Grid {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![BLANK; width * height],
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut chars = text.chars().peekable();
        let (mut row, mut col) = (0_usize, 0_usize);
        let (mut bold, mut reverse) = (false, false);
        while let Some(ch) = chars.next() {
            if ch != '\u{1b}' {
                if ch >= ' ' && row < self.height && col < self.width {
                    self.cells[row * self.width + col] = Cell { ch, bold, reverse };
                }
                col += 1;
                continue;
            }
            if chars.next_if_eq(&'[').is_none() {
                continue;
            }
            let mut params = String::new();
            let mut last = ' ';
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    last = next;
                    break;
                }
                params.push(next);
            }
            if params.starts_with('?') {
                continue;
            }
            let numbers: Vec<usize> = params
                .split(';')
                .map(|part| part.parse().unwrap_or(0))
                .collect();
            match last {
                'H' => {
                    row = numbers.first().copied().unwrap_or(1).max(1) - 1;
                    col = numbers.get(1).copied().unwrap_or(1).max(1) - 1;
                }
                'C' => col += numbers.first().copied().unwrap_or(1).max(1),
                'J' if numbers.first() == Some(&2) => self.cells.fill(BLANK),
                'm' => {
                    let mut codes = numbers.iter().copied();
                    while let Some(code) = codes.next() {
                        match code {
                            0 => (bold, reverse) = (false, false),
                            1 => bold = true,
                            7 => reverse = true,
                            22 => bold = false,
                            27 => reverse = false,
                            38 | 48 => match codes.next() {
                                Some(5) => {
                                    codes.next();
                                }
                                Some(2) => {
                                    codes.nth(2);
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn at(&self, col: usize, row: usize) -> Cell {
        self.cells[row * self.width + col]
    }

    fn row(&self, row: usize) -> String {
        (0..self.width).map(|col| self.at(col, row).ch).collect()
    }

    /// Columns `from..to` of `row`, as characters (the text has wide glyphs, so not bytes).
    fn cols(&self, row: usize, from: usize, to: usize) -> String {
        (from..to.min(self.width))
            .map(|col| self.at(col, row).ch)
            .collect()
    }

    fn text(&self) -> String {
        (0..self.height)
            .map(|row| self.row(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn member_with(permissions: &[Permission], settings: TerminalSettings, size: PtySize) -> Screen {
    let flow = MemberFlow::new(
        &sanitize_username("alice").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(1)),
            &Slug::SYSOP,
            100,
            &Via::Account,
            permissions.iter().copied(),
        ),
        None,
        settings,
    );
    let mut screen = Screen::new(
        size,
        RenderCap::default(),
        super::Content::Member(Box::new(flow)),
        std::sync::Arc::new(Ui::standard().unwrap()),
    )
    .unwrap();
    screen.set_client_terminal(Some("xterm-256color"), Some("truecolor"));
    // A staff session in low-security mode: the status bar's marker cell has `LOW-SECURITY`,
    // so the tests can see that nothing covers it.
    screen.set_low_security(true);
    screen
}

fn settings(layout: LayoutId, tier: Tier, color: bool) -> TerminalSettings {
    TerminalSettings::default()
        .with_layout(layout)
        .with_tier(tier)
        .with_color(color)
        .with_motion(false)
}

/// A signed-in member at the main menu.
fn at_menu(settings: TerminalSettings, size: (u32, u32)) -> Screen {
    let mut screen = member_with(
        &Permission::ALL,
        settings,
        PtySize::from_request(size.0, size.1),
    );
    screen.enter().unwrap();
    screen.apply_keys(&[Key::Char('x')]);
    screen.render().unwrap();
    screen
}

/// Repaint everything and read it back.
fn grid(screen: &mut Screen) -> Grid {
    let size = screen.pty;
    screen.set_size(size);
    let mut grid = Grid::new(
        usize::from(screen.viewport.width),
        usize::from(screen.viewport.height),
    );
    grid.feed(&screen.render().unwrap().unwrap_or_default());
    grid
}

fn room(n: i64, name: &str) -> Room {
    Room {
        id: RoomId::from_raw(n),
        name: Slug::parse(name).unwrap(),
        created_at: UnixSeconds::from_raw(0),
    }
}

/// Answer the dock's room-list request, as the shell would.
fn rooms(screen: &mut Screen) {
    screen.chat_deliver(ChatResponse::Rooms {
        rooms: vec![room(1, "lobby"), room(2, "retro")],
        notice: None,
    });
}

/// The column of the divider, from what the last frame recorded for the mouse; the grid must
/// show the rule there on every body row, with the grip on the middle one.
fn divider_col(screen: &Screen, grid: &Grid) -> Option<usize> {
    let hit = screen.interact.hits().dock().copied()?;
    let col = usize::from(hit.divider.x);
    let rule = ['\u{2502}', '|'];
    let grip = ['\u{2503}', '#'];
    let top = usize::from(hit.divider.y);
    let rows = usize::from(hit.divider.height);
    // A toast may be over the last rows (it overlays the body from the bottom, three rows at
    // most); the rows above it must be the rule.
    for row in top..(top + rows).saturating_sub(3) {
        let ch = grid.at(col, row).ch;
        let wanted: &[char] = if row == top + rows / 2 { &grip } else { &rule };
        assert!(
            wanted.contains(&ch),
            "divider column {col}, row {row}: {ch:?}\n{}",
            grid.text()
        );
    }
    // The bars are outside the divider's rows.
    // (A layout with no header bar starts at the first row.)
    assert!(
        top <= 1,
        "the divider starts under the header, if there is one"
    );
    assert_eq!(
        top + rows,
        grid.height - 1,
        "and stops above the status row"
    );
    Some(col)
}

/// The status row's marker cell is the reserved style (bold plus reverse), drawn in its
/// delimiters (`⟦⊖⟧` in the Unicode tier, `LOW-SECURITY` in ASCII), whatever happens; the reserved style is nowhere else on the screen.
fn assert_marker(grid: &Grid, what: &str) {
    let last = grid.height - 1;
    let cell = (0..grid.width)
        .take_while(|col| {
            let c = grid.at(*col, last);
            c.bold && c.reverse
        })
        .count();
    let word: String = (0..cell).map(|col| grid.at(col, last).ch).collect();
    assert!(
        cell > 0 && (word.contains('\u{27e6}') || word.contains("LOW-SECURITY")),
        "{what}: the marker cell is {word:?} in {:?}",
        grid.row(last)
    );
    for col in cell..grid.width {
        let c = grid.at(col, last);
        assert!(
            !(c.bold && c.reverse),
            "{what}: reserved style at column {col} of the status row"
        );
    }
    for row in 0..last {
        for col in 0..grid.width {
            let c = grid.at(col, row);
            assert!(
                !(c.bold && c.reverse),
                "{what}: reserved style at ({col},{row}): {c:?}"
            );
        }
    }
}

const SIZES: [(u32, u32); 2] = [(80, 24), (132, 43)];
const LAYOUTS: [LayoutId; 7] = [
    LayoutId::Classic,
    LayoutId::Lightbar,
    LayoutId::Compact,
    LayoutId::Dashboard,
    LayoutId::Wide,
    LayoutId::Focus,
    LayoutId::Split,
];
/// Unicode mono (no colour) and the ASCII tier.
const TIERS: [(Tier, bool); 2] = [(Tier::Unicode, false), (Tier::Ascii, false)];

#[test]
fn ctrl_t_opens_chat_beside_the_menu_in_every_layout_size_and_tier() {
    for size in SIZES {
        for layout in LAYOUTS {
            for (tier, color) in TIERS {
                let what = format!("{layout:?} {tier:?} {size:?}");
                let mut screen = at_menu(settings(layout, tier, color), size);
                let before = grid(&mut screen);
                let effects = screen.apply_keys(&[Key::Ctrl('t')]);
                assert!(effects.chat_open, "{what}: the session must join the hub");
                assert!(
                    effects.chat_request.is_some(),
                    "{what}: the room list is asked for"
                );
                rooms(&mut screen);
                let after = grid(&mut screen);
                assert_marker(&after, &what);
                // The bars keep their whole width: the status row is the same text and the
                // marker cell is the same glyph.
                let last = after.height - 1;
                assert_eq!(after.row(last), before.row(last), "{what}: status row");
                assert_eq!(after.at(0, last), before.at(0, last), "{what}: marker cell");
                // The divider sits where the geometry says, and the pane names a room.
                let wanted = dock::default_width(u16::try_from(size.0).unwrap());
                let col = divider_col(&screen, &after)
                    .unwrap_or_else(|| panic!("{what}: no divider\n{}", after.text()));
                assert_eq!(col, usize::from(after.width as u16 - wanted - 1), "{what}");
                let pane: String = (1..after.height - 1)
                    .map(|row| after.cols(row, col + 1, after.width))
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(pane.contains("lobby"), "{what}: pane\n{pane}");
                // Nothing of the pane leaked left of the divider.
                let left: String = (1..after.height - 1)
                    .map(|row| after.cols(row, 0, col))
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(!left.contains("retro"), "{what}: left side\n{left}");
                if tier == Tier::Ascii {
                    assert!(after.text().is_ascii(), "{what}: ASCII tier is seven-bit");
                }
            }
        }
    }
}

#[test]
fn the_split_layout_gives_its_second_column_to_the_dock() {
    let mut screen = at_menu(settings(LayoutId::Split, Tier::Unicode, false), (132, 43));
    assert!(screen.wants_side_panes(), "split draws its own side panes");
    assert_eq!(screen.menu_layout(), LayoutId::Split);
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    assert!(
        !screen.wants_side_panes(),
        "the dock is the second column now"
    );
    assert_eq!(screen.menu_layout(), LayoutId::Lightbar);
    let frame = grid(&mut screen);
    assert_marker(&frame, "split with the dock");
}

#[test]
fn chat_sits_beside_forums_mail_and_other_menus_and_keys_follow_the_focus() {
    for size in SIZES {
        for (tier, color) in TIERS {
            for hotkey in ['f', 'm', 'u', 'y'] {
                let what = format!("{hotkey} {tier:?} {size:?}");
                let mut screen = at_menu(settings(LayoutId::Lightbar, tier, color), size);
                screen.apply_keys(&[Key::Ctrl('t')]);
                rooms(&mut screen);
                // The pane has the keys: `f` is typed into chat, not a menu hotkey.
                let effects = screen.apply_keys(&[Key::Char(hotkey)]);
                assert!(
                    effects.forum_request.is_none() && effects.mail_request.is_none(),
                    "{what}: a key for the pane reached the screen"
                );
                // Ctrl-W gives the keys back to the screen; now the hotkey works.
                screen.apply_keys(&[Key::Ctrl('w')]);
                screen.apply_keys(&[Key::Char(hotkey)]);
                assert!(
                    screen.chat.is_some() && screen.dock.open,
                    "{what}: chat stays open"
                );
                let frame = grid(&mut screen);
                assert_marker(&frame, &what);
                let col = divider_col(&screen, &frame)
                    .unwrap_or_else(|| panic!("{what}: no divider\n{}", frame.text()));
                let pane: String = (1..frame.height - 1)
                    .map(|row| frame.cols(row, col + 1, frame.width))
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(pane.contains("lobby"), "{what}: pane\n{}", frame.text());
            }
        }
    }
}

#[test]
fn ctrl_w_moves_the_keys_and_ctrl_t_closes_and_leaves_the_hub() {
    let mut screen = at_menu(settings(LayoutId::Lightbar, Tier::Unicode, false), (80, 24));
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    assert!(screen.dock_has_keys());
    screen.apply_keys(&[Key::Ctrl('w')]);
    assert!(!screen.dock_has_keys() && screen.dock_active());
    screen.apply_keys(&[Key::Ctrl('w')]);
    assert!(screen.dock_has_keys());
    let effects = screen.apply_keys(&[Key::Ctrl('t')]);
    assert!(effects.chat_exit, "closing leaves the hub");
    assert!(screen.chat.is_none() && !screen.dock.open);
    // Opened and closed in one batch: opening has to be answered first, so the second key
    // waits for the shell and is applied afterwards, as for every other request.
    let effects = screen.apply_keys(&[Key::Ctrl('t'), Key::Ctrl('t')]);
    assert!(effects.chat_open && effects.chat_request.is_some());
    assert_eq!(effects.held_back, vec![Key::Ctrl('t')]);
    let later = screen.apply_keys(&[Key::Ctrl('t')]);
    assert!(later.chat_exit);
    assert!(screen.chat.is_none());
}

#[test]
fn escape_in_the_room_list_closes_the_pane_and_leaves_the_hub() {
    let mut screen = at_menu(settings(LayoutId::Lightbar, Tier::Unicode, false), (80, 24));
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    let effects = screen.apply_keys(&[Key::Escape]);
    assert!(effects.chat_exit);
    assert!(screen.chat.is_none() && !screen.dock.open);
}

#[test]
fn a_member_without_the_chat_permission_gets_no_dock() {
    let permissions: Vec<Permission> = Permission::ALL
        .iter()
        .copied()
        .filter(|permission| *permission != Permission::ChatJoin)
        .collect();
    let mut screen = member_with(
        &permissions,
        settings(LayoutId::Lightbar, Tier::Unicode, false),
        PtySize::from_request(80, 24),
    );
    screen.enter().unwrap();
    screen.apply_keys(&[Key::Char('x')]);
    let effects = screen.apply_keys(&[Key::Ctrl('t')]);
    assert!(!effects.chat_open && screen.chat.is_none() && !screen.dock.open);
}

#[test]
fn a_terminal_too_narrow_for_both_sides_says_so_and_opens_nothing() {
    let mut screen = at_menu(settings(LayoutId::Lightbar, Tier::Unicode, false), (60, 24));
    let effects = screen.apply_keys(&[Key::Ctrl('t')]);
    assert!(!effects.chat_open && screen.chat.is_none());
    let frame = grid(&mut screen);
    assert!(frame.text().contains("too narrow"), "{}", frame.text());
}

#[test]
fn leaving_for_a_screen_the_frame_does_not_carve_closes_the_dock() {
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    screen.apply_keys(&[Key::Ctrl('w')]);
    // `s` opens the terminal settings, which have no pane.
    let effects = screen.apply_keys(&[Key::Char('s')]);
    assert!(
        effects.chat_exit,
        "a joined chat must not be left behind a hidden pane"
    );
    assert!(screen.chat.is_none() && !screen.dock.open);
}

#[test]
fn the_resize_keys_move_the_divider_inside_the_bounds_and_never_cover_the_marker() {
    for size in SIZES {
        let total = u16::try_from(size.0).unwrap();
        let (least, most) = dock::bounds(total).unwrap();
        let mut screen = at_menu(settings(LayoutId::Lightbar, Tier::Unicode, false), size);
        screen.apply_keys(&[Key::Ctrl('t')]);
        rooms(&mut screen);
        screen.apply_keys(&[Key::Ctrl('w')]);
        let start = grid(&mut screen);
        let from = divider_col(&screen, &start).unwrap();
        screen.apply_keys(&[Key::Char('<'), Key::Char('<'), Key::Char('<')]);
        let moved = grid(&mut screen);
        assert_eq!(divider_col(&screen, &moved), Some(from - 3), "{size:?}");
        assert_marker(&moved, "after <<<");
        // Far past either end: held at the bounds, both sides keep their minimum.
        screen.apply_keys(&vec![Key::Char('<'); 300]);
        let wide = grid(&mut screen);
        assert_eq!(
            divider_col(&screen, &wide),
            Some(usize::from(total - most - 1))
        );
        assert_marker(&wide, "pane at its widest");
        screen.apply_keys(&vec![Key::Char('>'); 300]);
        let narrow = grid(&mut screen);
        assert_eq!(
            divider_col(&screen, &narrow),
            Some(usize::from(total - least - 1))
        );
        assert_marker(&narrow, "pane at its narrowest");
        screen.apply_keys(&[Key::Char('{')]);
        let big = grid(&mut screen);
        assert_eq!(
            divider_col(&screen, &big),
            Some(usize::from(total - least - dock::BIG_STEP - 1))
        );
    }
}

#[test]
fn the_resize_keys_are_text_in_the_pane_and_in_a_text_field() {
    let mut screen = at_menu(settings(LayoutId::Lightbar, Tier::Unicode, false), (80, 24));
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    // The pane has the keys: they are for chat's own input and change nothing here.
    screen.apply_keys(&[Key::Char('<'), Key::Char('>')]);
    assert_eq!(screen.dock.width, None);
}

fn drag(kind: Kind, col: u16, row: u16) -> Mouse {
    Mouse { kind, col, row }
}

#[test]
fn dragging_the_divider_resizes_with_the_mouse_inside_the_same_bounds() {
    for size in SIZES {
        let total = u16::try_from(size.0).unwrap();
        let (least, most) = dock::bounds(total).unwrap();
        let mut screen = at_menu(
            settings(LayoutId::Lightbar, Tier::Unicode, false).with_mouse(true),
            size,
        );
        screen.apply_keys(&[Key::Ctrl('t')]);
        rooms(&mut screen);
        let start = grid(&mut screen);
        let from = u16::try_from(divider_col(&screen, &start).unwrap()).unwrap();
        let row = 8;
        // Press on the divider, drag left by ten, release.
        let keys = screen.mouse_keys(
            &[
                drag(Kind::Down(Button::Left), from, row),
                drag(Kind::Drag(Button::Left), from - 5, row),
                drag(Kind::Drag(Button::Left), from - 10, row),
                drag(Kind::Up(Button::Left), from - 10, row),
            ],
            false,
        );
        assert!(keys.is_empty(), "a drag stands for no key");
        let moved = grid(&mut screen);
        // Ten columns left, but never past the bounds (the pane may not leave the screen less
        // than its minimum).
        let wanted = usize::from(from - 10).max(usize::from(total - most - 1));
        assert_eq!(divider_col(&screen, &moved), Some(wanted), "{size:?}");
        assert_marker(&moved, "after a drag");
        // Dragged off both edges: the bounds hold.
        screen.mouse_keys(
            &[
                drag(Kind::Down(Button::Left), from - 10, row),
                drag(Kind::Drag(Button::Left), 0, row),
                drag(Kind::Up(Button::Left), 0, row),
            ],
            false,
        );
        let wide = grid(&mut screen);
        assert_eq!(
            divider_col(&screen, &wide),
            Some(usize::from(total - most - 1))
        );
        let col = u16::try_from(divider_col(&screen, &wide).unwrap()).unwrap();
        screen.mouse_keys(
            &[
                drag(Kind::Down(Button::Left), col, row),
                drag(Kind::Drag(Button::Left), total + 40, row),
                drag(Kind::Up(Button::Left), total + 40, row),
            ],
            false,
        );
        let narrow = grid(&mut screen);
        assert_eq!(
            divider_col(&screen, &narrow),
            Some(usize::from(total - least - 1))
        );
        assert_marker(&narrow, "after dragging past the right edge");
    }
}

#[test]
fn motion_that_did_not_start_on_the_divider_moves_nothing() {
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false).with_mouse(true),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    let frame = grid(&mut screen);
    let before = divider_col(&screen, &frame).unwrap();
    screen.mouse_keys(
        &[
            drag(Kind::Down(Button::Left), 3, 8),
            drag(Kind::Drag(Button::Left), 10, 8),
            drag(Kind::Up(Button::Left), 10, 8),
        ],
        false,
    );
    let frame = grid(&mut screen);
    assert_eq!(divider_col(&screen, &frame), Some(before));
    // And with the mouse off, nothing drags at all.
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false).with_mouse(false),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    let frame = grid(&mut screen);
    let before = divider_col(&screen, &frame).unwrap();
    let from = u16::try_from(before).unwrap();
    screen.mouse_keys(
        &[
            drag(Kind::Down(Button::Left), from, 8),
            drag(Kind::Drag(Button::Left), from - 8, 8),
        ],
        false,
    );
    let frame = grid(&mut screen);
    assert_eq!(divider_col(&screen, &frame), Some(before));
}

#[test]
fn a_click_on_the_side_without_the_keys_moves_them() {
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false).with_mouse(true),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    grid(&mut screen);
    assert!(screen.dock_has_keys());
    // A click in the menu half: the keys go back to the screen (and the click still lands).
    let keys = screen.mouse_keys(&[drag(Kind::Down(Button::Left), 4, 8)], false);
    assert_eq!(keys.first(), Some(&Key::Ctrl('w')));
    screen.apply_keys(&keys);
    assert!(!screen.dock_has_keys());
    grid(&mut screen);
    // A click in the pane: the keys go there.
    let keys = screen.mouse_keys(&[drag(Kind::Down(Button::Left), 95, 8)], false);
    assert_eq!(keys, vec![Key::Ctrl('w')]);
}

fn said(text: &str) -> Event {
    Event::Line(Line {
        from: sanitize_username("bob").unwrap(),
        text: clean_line(text).unwrap(),
    })
}

#[test]
fn the_pane_is_live_and_a_flood_costs_one_redraw() {
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    // Join the first room, as the shell would answer the Enter.
    screen.apply_keys(&[Key::Enter]);
    screen.chat_deliver(ChatResponse::Joined {
        room: room(1, "lobby"),
        entered: Entered {
            roster: vec![sanitize_username("alice").unwrap()],
            history: Vec::new(),
        },
    });
    screen.render().unwrap();
    assert!(screen.render().unwrap().is_none(), "nothing is waiting");
    // A thousand lines arrive between two cycles: one frame, and it shows the newest.
    let events: Vec<Event> = (0..1000)
        .map(|n| said(&format!("line number {n}")))
        .collect();
    screen.chat_receive(events);
    let frame = screen.render().unwrap().expect("a frame for the news");
    let mut grid = Grid::new(100, 30);
    grid.feed(&frame);
    // Only the changed cells are sent, so the newest line is there and the budget is one frame.
    assert!(grid.text().contains("line number 999"), "{}", grid.text());
    assert!(
        frame.len() < 8 * 1024,
        "one small frame: {} bytes",
        frame.len()
    );
    assert!(
        screen.render().unwrap().is_none(),
        "and nothing more is owed"
    );
}

#[test]
fn hostile_chat_text_in_the_pane_never_carries_the_reserved_style() {
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    screen.apply_keys(&[Key::Enter]);
    screen.chat_deliver(ChatResponse::Joined {
        room: room(1, "lobby"),
        entered: Entered {
            roster: vec![sanitize_username("alice").unwrap()],
            history: Vec::new(),
        },
    });
    screen.chat_receive(vec![said(
        "\u{1b}[1;7m\u{27e6}\u{2299}\u{27e7} fake marker \u{1b}[0m",
    )]);
    let frame = grid(&mut screen);
    assert_marker(&frame, "hostile line");
    assert!(!frame.text().contains('\u{1b}'));
    assert!(!frame.text().contains('\u{2299}'), "{}", frame.text());
}

#[test]
fn a_full_screen_chat_from_the_menu_still_works_and_replaces_the_dock() {
    let mut screen = at_menu(
        settings(LayoutId::Lightbar, Tier::Unicode, false),
        (100, 30),
    );
    screen.apply_keys(&[Key::Ctrl('t')]);
    rooms(&mut screen);
    screen.apply_keys(&[Key::Ctrl('w')]);
    let effects = screen.apply_keys(&[Key::Char('c')]);
    assert!(effects.chat_open);
    assert!(screen.chat_fullscreen() && !screen.dock.open);
}
