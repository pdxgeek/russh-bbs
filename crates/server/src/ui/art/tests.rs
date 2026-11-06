#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::type_complexity,
    clippy::too_many_arguments
)]

//! Board art (UI-3): the parser's hardening and goldens, the `.art.toml` format, the large-letter
//! font, every bundled piece, and art drawn on real screens at both sizes, in mono and in the
//! ASCII tier, with hostile files.

use std::fmt::Write as _;

use proptest::prelude::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::sanitize::{RESERVED_CHROME, sanitize_username};
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::{LayoutId, MemberInput, TerminalSettings, ThemeName};

use super::draw::ArtCtx;
use super::*;
use crate::settings::Look;
use crate::ui::classic::MenuScreen;
use crate::ui::depth::ColorDepth;
use crate::ui::layout::{self, render_menu};
use crate::ui::layout_tests::{hostile_body, page, ui};
use crate::ui::{Board, Ui};

const STYLES: [&str; 6] = ["phosphor", "neon", "paper", "amber", "deepspace", "ansi"];

fn name(style: &str) -> ThemeName {
    ThemeName::new(style).unwrap()
}

fn rows_of(grid: &Grid, ascii: bool) -> Vec<String> {
    (0..grid.height())
        .map(|y| grid.row_text(y, ascii).trim_end().to_owned())
        .collect()
}

fn parse_ok(bytes: &[u8]) -> Grid {
    parse_ans(bytes).unwrap().grid
}

// ---- code page 437 ----

#[test]
fn every_cp437_character_is_one_column_and_safe() {
    for byte in 0..=255_u8 {
        let c = super::cp437::char_of(byte);
        if byte == 0 {
            continue;
        }
        assert_eq!(
            ratatui::text::Span::raw(c.to_string()).width(),
            1,
            "byte {byte:#04x} ({c:?})"
        );
        assert!(
            !RESERVED_CHROME.contains(&c),
            "{byte:#04x} is a marker glyph"
        );
        assert!(
            !('\u{e000}'..='\u{f8ff}').contains(&c),
            "{byte:#04x} is private-use"
        );
        let plain = super::cp437::ascii_of(c);
        assert!(
            plain.is_ascii_graphic() || plain == ' ',
            "{c:?} has no ASCII stand-in"
        );
    }
}

#[test]
fn the_ascii_stand_in_is_always_printable_ascii() {
    for code in 0..0x3000_u32 {
        if let Some(c) = char::from_u32(code) {
            let plain = super::cp437::ascii_of(c);
            assert!(
                plain.is_ascii_graphic() || plain == ' ',
                "{c:?} -> {plain:?}"
            );
        }
    }
}

// ---- the .ans parser: goldens ----

#[test]
fn plain_text_and_line_breaks_make_rows() {
    let grid = parse_ok(b"Hello\r\nworld");
    assert_eq!(rows_of(&grid, false), ["Hello", "world"]);
}

#[test]
fn sgr_colours_land_in_cells_in_the_vga_palette() {
    // ESC[31m is ANSI red = VGA 4; ESC[1;32m is bright green = 10; ESC[44m is blue = 1.
    let grid = parse_ok(b"\x1b[31mR\x1b[1;32mG\x1b[0;44mB\x1b[0mN");
    let cells: Vec<_> = (0..4).map(|x| grid.get(x, 0)).collect();
    assert_eq!(
        (cells[0].ch, cells[0].fg, cells[0].bg),
        ('R', Some(4), None)
    );
    assert_eq!((cells[1].ch, cells[1].fg), ('G', Some(10)));
    assert_eq!(
        (cells[2].ch, cells[2].fg, cells[2].bg),
        ('B', None, Some(1))
    );
    assert_eq!((cells[3].ch, cells[3].fg, cells[3].bg), ('N', None, None));
}

#[test]
fn bold_is_the_bright_colour_and_reverse_swaps_the_colours() {
    let grid = parse_ok(b"\x1b[1;34mA\x1b[0;33;42;7mB");
    assert_eq!(grid.get(0, 0).fg, Some(9));
    let b = grid.get(1, 0);
    // Reverse: foreground and background trade places; there is no reverse attribute.
    assert_eq!((b.fg, b.bg), (Some(2), Some(6)));
}

#[test]
fn bright_foregrounds_and_backgrounds_from_the_aixterm_codes() {
    let grid = parse_ok(b"\x1b[91;102mX");
    assert_eq!((grid.get(0, 0).fg, grid.get(0, 0).bg), (Some(12), Some(10)));
}

#[test]
fn blink_is_ignored_unless_the_file_asks_for_ice_colours() {
    let plain = parse_ok(b"\x1b[5;41mX");
    assert_eq!(plain.get(0, 0).bg, Some(4));
    let mut bytes = b"\x1b[5;41mX".to_vec();
    bytes.push(0x1a);
    bytes.extend(sauce(80, true, b"iCE"));
    let ice = parse_ok(&bytes);
    assert_eq!(ice.get(0, 0).bg, Some(12));
}

#[test]
fn cp437_bytes_become_unicode_and_ascii() {
    let grid = parse_ok(&[0xDB, 0xB0, 0xB1, 0xB2, 0xC9, 0xCD, 0xBB, 0xE9]);
    assert_eq!(rows_of(&grid, false), ["█░▒▓╔═╗Θ"]);
    assert_eq!(rows_of(&grid, true), ["#.:#+=+O"]);
}

#[test]
fn cursor_forward_and_position_move_inside_the_grid() {
    let grid = parse_ok(b"A\x1b[3CB\x1b[2;1HC\x1b[1;10HD");
    assert_eq!(rows_of(&grid, false), ["A   B    D", "C"]);
}

#[test]
fn cursor_save_and_restore_work() {
    let grid = parse_ok(b"AB\x1b[s\r\nCD\x1b[uX");
    assert_eq!(rows_of(&grid, false), ["ABX", "CD"]);
}

#[test]
fn text_wraps_at_the_width() {
    let long = "x".repeat(100);
    let grid = parse_ok(long.as_bytes());
    assert_eq!(grid.width(), 80);
    assert_eq!(grid.height(), 2);
}

#[test]
fn a_ctrl_z_ends_the_picture() {
    let grid = parse_ok(b"AB\x1aCD");
    assert_eq!(rows_of(&grid, false), ["AB"]);
}

// ---- the .ans parser: what is dropped ----

#[test]
fn every_other_sequence_is_dropped_never_passed_through() {
    let hostile: &[&[u8]] = &[
        b"\x1b[2J",               // clear screen
        b"\x1b[2K\x1b[K",         // erase line
        b"\x1b]0;owned\x07",      // OSC title (BEL)
        b"\x1b]52;c;aGk=\x1b\\",  // OSC clipboard (ST)
        b"\x1bP1$r0m\x1b\\",      // DCS
        b"\x1b_Gf=24;AAAA\x1b\\", // APC (kitty graphics)
        b"\x1b[?1049h",           // private mode: alternate screen
        b"\x1b[?25l",             // hide cursor
        b"\x1b[6n",               // device status report
        b"\x1b[1;1;1;1;1t",       // window manipulation
        b"\x1b(0",                // charset designation
        b"\x1bc",                 // full reset
        b"\x1b[38;2;255;0;0m",    // truecolor (skipped whole)
        b"\x1b[38;5;196m",
        b"\x1b[999999999999999999999m", // absurd number
    ];
    for sequence in hostile {
        let mut bytes = b"A".to_vec();
        bytes.extend_from_slice(sequence);
        bytes.push(b'B');
        let grid = parse_ok(&bytes);
        assert_eq!(rows_of(&grid, false), ["AB"], "{sequence:?}");
        for cell in grid.cells() {
            assert!(!cell.ch.is_control(), "{sequence:?}");
        }
    }
}

#[test]
fn an_unterminated_string_sequence_cannot_swallow_the_picture_forever() {
    // No terminator: the sequence is abandoned at its cap and the rest is read normally.
    let mut bytes = b"\x1b]0;".to_vec();
    bytes.extend(std::iter::repeat_n(b'x', 5_000));
    bytes.extend_from_slice(b"\r\nVISIBLE");
    let grid = parse_ok(&bytes);
    assert!(
        rows_of(&grid, false)
            .iter()
            .any(|row| row.contains("VISIBLE"))
    );
}

#[test]
fn a_truncated_escape_at_the_end_is_dropped() {
    for tail in [&b"\x1b"[..], b"\x1b[", b"\x1b[31", b"\x1b]", b"\x1b[3;"] {
        let mut bytes = b"OK".to_vec();
        bytes.extend_from_slice(tail);
        assert_eq!(rows_of(&parse_ok(&bytes), false), ["OK"]);
    }
}

#[test]
fn control_bytes_are_dropped() {
    let grid = parse_ok(b"A\x00\x01\x07\x0c\x7fB");
    assert_eq!(rows_of(&grid, false), ["AB"]);
}

// ---- the .ans parser: caps ----

#[test]
fn input_over_the_byte_cap_is_refused() {
    let big = vec![b'x'; super::ans::MAX_BYTES + 1];
    assert!(matches!(parse_ans(&big), Err(ArtError::TooManyBytes(_, _))));
}

#[test]
fn too_many_rows_are_refused_and_empty_art_is_refused() {
    let tall = "x\r\n".repeat(60);
    assert_eq!(parse_ans(tall.as_bytes()), Err(ArtError::TooTall));
    assert_eq!(parse_ans(b""), Err(ArtError::Empty));
    assert_eq!(parse_ans(b"   \r\n  "), Err(ArtError::Empty));
    assert_eq!(parse_ans(b"\x1b[2J"), Err(ArtError::Empty));
}

#[test]
fn cursor_movement_is_clamped_to_the_box() {
    let grid = parse_ok(b"\x1b[9999;9999HX\x1b[9999AY\x1b[9999DZ");
    assert!(grid.height() <= MAX_ROWS);
    assert!(grid.width() <= 80);
    // Moving far down is clamped to the last row the cap allows, not refused.
    assert_eq!(grid.height(), MAX_ROWS);
}

#[test]
fn a_sauce_width_beyond_the_cap_is_refused() {
    let mut bytes = b"hi".to_vec();
    bytes.push(0x1a);
    bytes.extend(sauce(500, false, b""));
    assert_eq!(parse_ans(&bytes), Err(ArtError::TooWide));
}

// ---- SAUCE ----

/// A SAUCE record for `ANSi` data: `columns` wide, with the iCE flag if asked.
fn sauce(columns: u16, ice: bool, title: &[u8]) -> Vec<u8> {
    let mut record = vec![b' '; 128];
    record[..7].copy_from_slice(b"SAUCE00");
    record[7..7 + title.len()].copy_from_slice(title);
    record[42..48].copy_from_slice(b"Author");
    record[62..67].copy_from_slice(b"Group");
    record[82..90].copy_from_slice(b"20261104");
    record[90..94].copy_from_slice(&[0; 4]);
    record[94] = 1; // character data
    record[95] = 1; // ANSi
    record[96..98].copy_from_slice(&columns.to_le_bytes());
    record[98..100].copy_from_slice(&25_u16.to_le_bytes());
    record[100..104].copy_from_slice(&[0; 4]);
    record[104] = 0; // comments
    record[105] = u8::from(ice);
    record
}

#[test]
fn a_sauce_record_is_read_and_stripped() {
    let mut bytes = b"AB\r\nCD".to_vec();
    bytes.push(0x1a);
    bytes.extend(sauce(40, false, b"My Title"));
    let parsed = parse_ans(&bytes).unwrap();
    assert_eq!(rows_of(&parsed.grid, false), ["AB", "CD"]);
    let record = parsed.sauce.unwrap();
    assert_eq!(record.title, "My Title");
    assert_eq!(record.author, "Author");
    assert_eq!(record.group, "Group");
}

#[test]
fn a_sauce_comment_block_is_stripped_too() {
    let mut bytes = b"OK".to_vec();
    bytes.push(0x1a);
    bytes.extend_from_slice(b"COMNT");
    bytes.extend(std::iter::repeat_n(b'c', 64));
    let mut record = sauce(80, false, b"t");
    record[104] = 1;
    bytes.extend(record);
    let parsed = parse_ans(&bytes).unwrap();
    assert_eq!(rows_of(&parsed.grid, false), ["OK"]);
}

#[test]
fn sauce_text_is_sanitized() {
    let mut record = sauce(80, false, b"");
    record[7..15].copy_from_slice(b"\x1b[31mhi\x07");
    let mut bytes = b"x".to_vec();
    bytes.push(0x1a);
    bytes.extend(record);
    let title = parse_ans(&bytes).unwrap().sauce.unwrap().title;
    assert!(!title.contains('\u{1b}'));
    assert!(title.chars().all(|c| !c.is_control()));
}

// ---- the .ans parser: property tests ----

fn check_grid(grid: &Grid) {
    assert!(grid.width() <= MAX_COLS, "width {}", grid.width());
    assert!(grid.height() <= MAX_ROWS, "height {}", grid.height());
    for cell in grid.cells() {
        for c in [cell.ch, cell.ascii] {
            assert!(!c.is_control(), "control {c:?}");
            assert!(!RESERVED_CHROME.contains(&c), "marker glyph {c:?}");
            assert!(!('\u{e000}'..='\u{f8ff}').contains(&c), "private use {c:?}");
            assert_ne!(c, '\u{1b}');
        }
        assert!(cell.ascii.is_ascii_graphic() || cell.ascii == ' ');
        assert!(cell.fg.is_none_or(|i| i < 16));
        assert!(cell.bg.is_none_or(|i| i < 16));
    }
}

/// Bytes weighted toward the ones that start and shape sequences.
fn art_like() -> impl Strategy<Value = Vec<u8>> {
    let interesting = prop::sample::select(
        b"\x1b[];0123456789ABCDEFGHJKfhlmnsu?<>=!\"#$%&'()*+,-./:\\^_`P\r\n\t\x08\x07\x1a ~"
            .to_vec(),
    );
    prop::collection::vec(prop_oneof![3 => interesting, 1 => any::<u8>()], 0..2048)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic_and_never_exceed_the_caps(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(parsed) = parse_ans(&bytes) {
            check_grid(&parsed.grid);
        }
    }

    #[test]
    fn escape_heavy_bytes_never_emit_a_control_sequence_or_a_style(bytes in art_like()) {
        if let Ok(parsed) = parse_ans(&bytes) {
            check_grid(&parsed.grid);
            // The grid text carries no escape, and no cell has a field for an attribute.
            for y in 0..parsed.grid.height() {
                prop_assert!(!parsed.grid.row_text(y, false).contains(char::from(0x1b_u8)));
            }
        }
    }

    #[test]
    fn a_picture_with_a_sauce_record_still_respects_the_caps(
        body in prop::collection::vec(any::<u8>(), 0..512),
        columns in any::<u16>(),
        ice in any::<bool>(),
    ) {
        let mut bytes = body;
        bytes.push(0x1a);
        bytes.extend(sauce(columns, ice, b"t"));
        if let Ok(parsed) = parse_ans(&bytes) {
            check_grid(&parsed.grid);
        }
    }

    #[test]
    fn colour_choices_stay_inside_the_palette(codes in prop::collection::vec(0_u16..200, 0..40)) {
        let mut bytes = Vec::new();
        for code in codes {
            bytes.extend(format!("\x1b[{code}mx").into_bytes());
        }
        if let Ok(parsed) = parse_ans(&bytes) {
            check_grid(&parsed.grid);
        }
    }
}

// ---- art on real screens ----

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Draw the main menu with art on, in `chosen` at `size`, with the markers in the status row.
fn shot(
    ui: &Ui,
    style: &str,
    chosen: LayoutId,
    size: (u16, u16),
    tier: Tier,
    color: bool,
    depth: ColorDepth,
    set: Option<&ArtSet>,
    art_on: bool,
) -> Buffer {
    let theme = ui.theme(name(style));
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(color)
        .with_layout(chosen);
    let area = Rect::new(0, 0, size.0, size.1);
    let layout = layout::effective(chosen, area);
    let look = Look::for_theme(&settings, layout, theme).at_depth(depth);
    let page = page(&[]);
    let board = Board::standard();
    let ctx = ArtCtx {
        set: set.unwrap_or_else(|| ui.art()),
        style: name(style),
        board: board.name(),
        enabled: art_on,
    };
    let body = hostile_body();
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            render_menu(
                frame,
                layout,
                &MenuScreen {
                    strings: theme.strings(),
                    page: &page,
                    body: &body[..1],
                    notice: None,
                    look,
                    tier,
                    markers: &[SecurityMarker::Unencrypted, SecurityMarker::LowSecurity],
                    clock: Some("UTC 12:00"),
                    mail_over_ssh_only: false,
                    board,
                    wanted: chosen,
                    popover: false,
                    art: Some(&ctx),
                    widgets: crate::ui::widgets::WidgetView::none(),
                },
            );
            crate::ui::backdrop::finish(frame, look);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

// ---- the .art.toml format ----

const GOOD: &str = r#"
format = 1
name = "harbour"
style = "paper"
credit = "Jo Sysop"

[[piece]]
slot = "login"
fallback = "Welcome to {board}"
rows = ["ab", "cd"]
fg = ["Rg", ".."]
bg = ["..", ".b"]
ascii = ["AB", "CD"]
"#;

#[test]
fn a_good_art_toml_file_parses_into_cells() {
    let file = parse_art_toml(GOOD, true).unwrap();
    assert_eq!(file.name, "harbour");
    let piece = &file.pieces[0];
    assert_eq!(piece.slot(), Slot::Login);
    assert_eq!(piece.style(), Some(name("paper")));
    assert_eq!(piece.credit(), "Jo Sysop");
    assert_eq!(piece.fallback(), Some("Welcome to {board}"));
    assert!(piece.is_sysop());
    let grid = piece.grid();
    assert_eq!(rows_of(grid, false), ["ab", "cd"]);
    assert_eq!(rows_of(grid, true), ["AB", "CD"]);
    assert_eq!(grid.get(0, 0).fg, Some(12)); // R is bright red
    assert_eq!(grid.get(1, 0).fg, Some(2)); // g is green
    assert_eq!(grid.get(1, 1).bg, Some(1)); // b is blue
    assert!(piece.ascii_safe());
}

fn refused(text: &str) -> String {
    parse_art_toml(text, true).unwrap_err().to_string()
}

#[test]
fn art_toml_files_are_validated() {
    let piece = "[[piece]]\nslot = \"login\"\nrows = [\"x\"]\n";
    let head = "format = 1\nname = \"a\"\n";
    assert!(refused("format = 2\nname = \"a\"\n").contains("format 2"));
    assert!(refused(&format!("{head}\n")).contains("no [[piece]]"));
    assert!(refused(&format!("{head}bogus = 1\n{piece}")).contains("bogus"));
    assert!(refused(&format!("format = 1\nname = \"Bad Name\"\n{piece}")).contains("name"));
    assert!(refused(&format!("{head}style = \"BAD\"\n{piece}")).contains("style"));
    assert!(refused(&format!("{head}{}", piece.replace("login", "footer"))).contains("slot"));
    assert!(
        refused(&format!("{head}{piece}fg = [\"z\"]\n")).contains("fg row 1"),
        "colour characters are checked"
    );
    assert!(refused(&format!("{head}{piece}fg = [\"..\"]\n")).contains("wider"));
    assert!(refused(&format!("{head}{piece}ascii = [\"é\"]\n")).contains("ASCII"));
    assert!(
        refused(&format!(
            "{head}credit = \"a fully private board\"\n{piece}"
        ))
        .contains("privacy")
    );
    assert!(refused("not toml at all [[[").contains("not valid"));
}

#[test]
fn art_over_its_slots_cap_is_refused() {
    let wide = "x".repeat(81);
    let text =
        format!("format = 1\nname = \"a\"\n[[piece]]\nslot = \"menu\"\nrows = [\"{wide}\"]\n");
    assert!(refused(&text).contains("wider than the 80-column cap for the menu slot"));
    let rows = vec!["\"x\""; 26].join(", ");
    let text = format!("format = 1\nname = \"a\"\n[[piece]]\nslot = \"menu\"\nrows = [{rows}]\n");
    assert!(refused(&text).contains("taller than the 25-row cap for the menu slot"));
    // The same height is fine for a full-screen slot.
    let text = format!("format = 1\nname = \"a\"\n[[piece]]\nslot = \"login\"\nrows = [{rows}]\n");
    assert!(parse_art_toml(&text, true).is_ok());
    let big = format!("format = 1\nname = \"a\"\n# {}\n", "x".repeat(70_000));
    assert!(matches!(
        parse_art_toml(&big, true),
        Err(ArtError::TooManyBytes(_, _))
    ));
}

#[test]
fn hostile_characters_in_art_toml_are_neutralised_not_passed() {
    // An ESC, a bell, a bidi override, a private-use glyph and every reserved marker glyph.
    let mut row = String::from("\\u001b[31m\\u0007\\u202e\\ue000");
    for c in RESERVED_CHROME {
        write!(row, "\\u{:04x}", u32::from(*c)).unwrap();
    }
    let text =
        format!("format = 1\nname = \"a\"\n[[piece]]\nslot = \"login\"\nrows = [\"{row}\"]\n");
    let file = parse_art_toml(&text, true).unwrap();
    check_grid(file.pieces[0].grid());
    let drawn = rows_of(file.pieces[0].grid(), false).join("");
    for c in RESERVED_CHROME {
        assert!(!drawn.contains(*c), "{c:?}");
    }
    assert!(
        drawn.contains('\u{fffd}'),
        "markers and private use become U+FFFD"
    );
}

#[test]
fn a_fallback_that_claims_privacy_or_hides_a_marker_is_refused() {
    let head = "format = 1\nname = \"a\"\n[[piece]]\nslot = \"login\"\nrows = [\"x\"]\n";
    assert!(refused(&format!("{head}fallback = \"Secure chat\"\n")).contains("privacy"));
    assert!(refused(&format!("{head}fallback = \"\\u27e6x\\u27e7\"\n")).contains("fallback"));
}

#[test]
fn trailing_blank_rows_do_not_claim_screen_space() {
    let text =
        "format = 1\nname = \"a\"\n[[piece]]\nslot = \"menu\"\nrows = [\"hi\", \"  \", \"\"]\n";
    let file = parse_art_toml(text, false).unwrap();
    assert_eq!(file.pieces[0].grid().height(), 1);
}

// ---- the large-letter font ----

#[test]
fn large_letters_draw_the_name_in_blocks_and_in_ascii() {
    let grid = big_letters("Hi!", '\u{2588}', None).unwrap();
    assert_eq!(grid.height(), LETTER_HEIGHT);
    assert_eq!(grid.width(), big_letters_width("Hi!"));
    let unicode = rows_of(&grid, false);
    assert_eq!(unicode[0].trim_end(), "█   █ █████   █");
    let ascii = rows_of(&grid, true);
    assert!(
        ascii
            .iter()
            .all(|r| r.chars().all(|c| c == '#' || c == ' '))
    );
    assert!(big_letters("", '#', None).is_none());
}

#[test]
fn every_letter_digit_and_a_stranger_has_a_glyph() {
    for c in ('A'..='Z')
        .chain('0'..='9')
        .chain(" .,-_'!?:&/+#".chars())
        .chain(['é', '日'])
    {
        let grid = big_letters(&c.to_string(), '#', None).unwrap();
        check_grid(&grid);
        if c != ' ' {
            assert!(grid.cells().any(|cell| cell.ch == '#'), "{c:?} is blank");
        }
    }
}

// ---- the bundled art ----

#[test]
fn each_style_ships_a_login_banner_and_a_menu_header_made_for_it() {
    let set = ArtSet::bundled().unwrap();
    for style in STYLES {
        for slot in [Slot::Login, Slot::Menu] {
            let found: Vec<_> = set
                .pieces()
                .iter()
                .filter(|p| p.slot() == slot && p.style() == Some(name(style)))
                .collect();
            assert_eq!(found.len(), 1, "{style} {}", slot.name());
            let piece = found[0];
            assert!(!piece.is_sysop());
            assert!(piece.credit().contains("original"), "{style} credit");
            check_grid(piece.grid());
            let (cols, rows) = slot.cap();
            assert!(piece.grid().width() <= cols && piece.grid().height() <= rows);
            // A header must leave the 80x24 menu its room: at most 5 rows, 76 columns.
            if slot == Slot::Menu {
                assert!(piece.grid().height() <= 5 && piece.grid().width() <= 76);
            } else {
                assert!(piece.grid().width() <= 76 && piece.grid().height() <= 10);
            }
            assert!(
                piece.ascii_safe(),
                "{style} {} has a faithful ASCII form",
                slot.name()
            );
        }
    }
}

#[test]
fn the_six_styles_have_different_art() {
    let set = ArtSet::bundled().unwrap();
    let mut seen = std::collections::HashSet::new();
    for piece in set.pieces() {
        assert!(
            seen.insert(rows_of(piece.grid(), false)),
            "{} duplicates",
            piece.set()
        );
    }
    // Login, menu, area and logoff for each of the six styles.
    assert_eq!(seen.len(), 24);
}

// ---- choosing ----

#[test]
fn the_largest_piece_that_fits_wins_and_a_sysops_beats_a_bundled_one() {
    let mut set = ArtSet::bundled().unwrap();
    let neon = name("neon");
    let login = set.pick(Slot::Login, neon, 80, 20).unwrap();
    assert_eq!(login.set(), "neon");
    assert!(
        set.pick(Slot::Login, neon, 80, 2).is_none(),
        "nothing fits two rows"
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("login.ans"), b"\x1b[35mmine").unwrap();
    assert_eq!(set.add_dir(dir.path()), 1);
    let picked = set.pick(Slot::Login, neon, 80, 20).unwrap();
    assert!(picked.is_sysop());
    assert_eq!(rows_of(picked.grid(), false), ["mine"]);
    // A file made for one style wins over a generic sysop file for that style only.
    std::fs::write(dir.path().join("neon.login.ans"), b"neon only").unwrap();
    set.add_dir(dir.path());
    assert_eq!(
        rows_of(set.pick(Slot::Login, neon, 80, 20).unwrap().grid(), false),
        ["neon only"]
    );
    assert_eq!(
        rows_of(
            set.pick(Slot::Login, name("paper"), 80, 20).unwrap().grid(),
            false
        ),
        ["mine"]
    );
}

// ---- sysop files, hostile ----

#[test]
fn hostile_art_files_are_refused_with_reasons_and_never_stop_the_boot() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let put = |file: &str, bytes: &[u8]| std::fs::write(dir.path().join(file), bytes).unwrap();
    put("login.ans", &vec![b'x'; 70_000]);
    put("menu.ans", b"\x1b[2J");
    put("nonsense.ans", b"hello");
    put("a.art.toml", b"format = 1\nbogus = true\n");
    put("b.art.toml", &[0xff, 0xfe, 0x00]);
    put("area.ans", "x\r\n".repeat(60).as_bytes());
    put("notes.txt", b"ignored");
    symlink("/etc/passwd", dir.path().join("logoff.ans")).unwrap();
    let mut set = ArtSet::bundled().unwrap();
    let before = set.pieces().len();
    assert_eq!(set.add_dir(dir.path()), 0);
    assert_eq!(set.pieces().len(), before);
    let files: Vec<_> = set.problems().iter().map(|p| p.file.as_str()).collect();
    for expected in [
        "login.ans",
        "menu.ans",
        "nonsense.ans",
        "a.art.toml",
        "b.art.toml",
        "area.ans",
        "logoff.ans",
    ] {
        assert!(
            files.contains(&expected),
            "{expected} was not reported: {files:?}"
        );
    }
    assert!(!files.contains(&"notes.txt"));
    assert!(set.problems().iter().all(|p| !p.reason.is_empty()));
    // A directory that does not exist adds nothing and is not a problem.
    assert_eq!(set.add_dir(&dir.path().join("missing")), 0);
}

#[test]
fn problem_text_from_hostile_file_names_is_sanitized() {
    // H-3: the theme preview draws these. A name with an RTL override, a Nerd Font glyph
    // and the reserved marker delimiters must not reach the screen.
    let dir = tempfile::tempdir().unwrap();
    let name = "x\u{202E}\u{F07C}\u{27E6}\u{2299}\u{27E7}\u{200B}.art.toml";
    std::fs::write(dir.path().join(name), b"format = 1\nbogus = true\n").unwrap();
    let mut set = ArtSet::default();
    set.add_dir(dir.path());
    let problems = set.problems();
    assert_eq!(problems.len(), 1);
    for text in [&problems[0].file, &problems[0].reason] {
        assert!(
            !text.chars().any(|c| matches!(
                c,
                '\u{202E}' | '\u{F07C}' | '\u{27E6}' | '\u{27E7}' | '\u{2299}' | '\u{200B}'
            )),
            "{text:?}"
        );
    }
    assert!(problems[0].file.contains('\u{FFFD}'));
}

#[test]
fn only_the_first_files_of_a_crowded_directory_are_read() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..(MAX_FILES + 8) {
        std::fs::write(dir.path().join(format!("{i:03}.login.ans")), b"x").unwrap();
    }
    let mut set = ArtSet::default();
    set.add_dir(dir.path());
    // The numbered names are not styles ("000" is, in fact, a valid name), so they all load,
    // but never more than the cap.
    assert!(set.pieces().len() <= MAX_FILES);
    assert!(
        set.problems()
            .iter()
            .any(|p| p.reason.contains("more than"))
    );
}

// ---- art on real screens ----

fn marker_columns(tier: Tier, cols: u16) -> u16 {
    crate::ui::layout_tests::marker_cell_width(tier, cols)
}

/// Every cell wearing the reserved style, as `(x, y)`.
fn reserved_cells(buffer: &Buffer) -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)]
                .modifier
                .contains(Modifier::BOLD | Modifier::REVERSED)
            {
                found.push((x, y));
            }
        }
    }
    found
}

#[test]
fn each_styles_art_is_on_the_menu_at_both_sizes_in_mono_and_ascii() {
    let ui = ui();
    for style in STYLES {
        let piece = ui
            .art()
            .pick(Slot::Menu, name(style), 80, 24)
            .unwrap()
            .clone();
        for size in [(80, 24), (132, 43)] {
            for (tier, color, depth) in [
                (Tier::Unicode, true, ColorDepth::TrueColor),
                (Tier::Unicode, false, ColorDepth::Mono),
                (Tier::Ascii, true, ColorDepth::Ansi16),
                (Tier::Ascii, false, ColorDepth::Mono),
            ] {
                let buffer = shot(
                    &ui,
                    style,
                    LayoutId::Classic,
                    size,
                    tier,
                    color,
                    depth,
                    None,
                    true,
                );
                let ascii = tier == Tier::Ascii;
                let want = piece.grid().row_text(0, ascii).trim().to_owned();
                let screen = text(&buffer);
                assert!(
                    screen.contains(&want),
                    "{style} {size:?} {tier:?} lacks the first art row {want:?}\n{screen}"
                );
                if ascii {
                    assert!(
                        screen.is_ascii(),
                        "{style} {size:?} ascii tier draws non-ASCII"
                    );
                }
                if !color {
                    for cell in buffer.content() {
                        assert_eq!(cell.fg, ratatui::style::Color::Reset, "{style} mono colour");
                        assert_eq!(cell.bg, ratatui::style::Color::Reset, "{style} mono colour");
                    }
                }
            }
        }
    }
}

#[test]
fn art_never_touches_the_marker_cell_or_the_status_row_and_never_wears_the_reserved_style() {
    let ui = ui();
    for style in STYLES {
        for layout_choice in LayoutId::ALL {
            for size in [(80, 24), (132, 43)] {
                for tier in [Tier::Unicode, Tier::Ascii] {
                    let off = shot(
                        &ui,
                        style,
                        layout_choice,
                        size,
                        tier,
                        true,
                        ColorDepth::TrueColor,
                        None,
                        false,
                    );
                    let on = shot(
                        &ui,
                        style,
                        layout_choice,
                        size,
                        tier,
                        true,
                        ColorDepth::TrueColor,
                        None,
                        true,
                    );
                    let last = size.1 - 1;
                    // The status row is identical with art on and off.
                    for x in 0..size.0 {
                        assert_eq!(
                            off[(x, last)],
                            on[(x, last)],
                            "{style} {layout_choice:?} {size:?} col {x}"
                        );
                    }
                    // Only the marker cell wears the reserved style, with or without art.
                    let marked = reserved_cells(&on);
                    assert_ne!(marked.len(), 0);
                    let width = marker_columns(tier, size.0);
                    for (x, y) in marked {
                        assert_eq!(
                            y, last,
                            "{style} {layout_choice:?}: reserved style off the status row"
                        );
                        assert!(
                            x < width,
                            "{style} {layout_choice:?}: reserved style past the marker cell"
                        );
                    }
                    // No cell anywhere is reversed except in that row.
                    for y in 0..last {
                        for x in 0..size.0 {
                            assert!(
                                !on[(x, y)].modifier.contains(Modifier::REVERSED),
                                "{style} {layout_choice:?} {size:?}: reverse at {x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn art_gives_way_to_a_tight_screen_and_the_menu_keeps_its_items() {
    let ui = ui();
    // At 80x14 the classic menu has no spare rows: no art, every item still there.
    for style in STYLES {
        let buffer = shot(
            &ui,
            style,
            LayoutId::Classic,
            (80, 14),
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            None,
            true,
        );
        let off = shot(
            &ui,
            style,
            LayoutId::Classic,
            (80, 14),
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            None,
            false,
        );
        assert_eq!(
            text(&buffer),
            text(&off),
            "{style}: art squeezed a tight menu"
        );
    }
    // Compact has no header, so it takes no header art.
    let compact = shot(
        &ui,
        "neon",
        LayoutId::Compact,
        (80, 24),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        None,
        true,
    );
    let compact_off = shot(
        &ui,
        "neon",
        LayoutId::Compact,
        (80, 24),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        None,
        false,
    );
    assert_eq!(text(&compact), text(&compact_off));
}

#[test]
fn every_item_of_the_menu_survives_art_in_every_layout() {
    let ui = ui();
    for style in STYLES {
        for layout_choice in LayoutId::ALL {
            let buffer = shot(
                &ui,
                style,
                layout_choice,
                (80, 24),
                Tier::Unicode,
                true,
                ColorDepth::TrueColor,
                None,
                true,
            );
            let screen = text(&buffer);
            for label in ["Forums", "Mail", "Doors", "Sysop tools", "Log off"] {
                assert!(
                    screen.contains(label),
                    "{style} {layout_choice:?} lost {label}\n{screen}"
                );
            }
        }
    }
}

#[test]
fn a_board_with_art_off_or_no_art_gets_large_letters_where_there_is_room() {
    let ui = ui();
    let empty = ArtSet::default();
    // Roomy: 132x43, the menu header is the board name in large letters.
    let buffer = shot(
        &ui,
        "neon",
        LayoutId::Classic,
        (132, 43),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        Some(&empty),
        true,
    );
    assert!(
        text(&buffer).contains("█"),
        "large letters on a roomy screen"
    );
    // 80x24: the menu needs its room, so no letters (the header bar already names the board).
    let buffer = shot(
        &ui,
        "neon",
        LayoutId::Classic,
        (80, 24),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        Some(&empty),
        true,
    );
    assert!(!text(&buffer).contains("█"));
    // The ASCII tier draws them with #.
    let buffer = shot(
        &ui,
        "neon",
        LayoutId::Classic,
        (132, 43),
        Tier::Ascii,
        true,
        ColorDepth::Ansi16,
        Some(&empty),
        true,
    );
    let screen = text(&buffer);
    assert!(screen.contains('#') && screen.is_ascii());
    // `board.art = false` ignores installed art and draws letters too.
    let off = shot(
        &ui,
        "neon",
        LayoutId::Classic,
        (132, 43),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        None,
        false,
    );
    assert!(text(&off).contains("█"));
}

#[test]
fn hostile_sysop_art_cannot_cover_the_status_row_or_the_marker() {
    let ui = ui();
    // A 25-row header that wants the whole screen, wearing every sequence it can.
    let mut hostile = Vec::new();
    for _ in 0..25 {
        hostile.extend_from_slice(
            b"\x1b[1;7;31m\xe2\x9c\x8e SERVER-READABLE \x1b[2J\x1b[5;5H\x1b]0;x\x07\r\n",
        );
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("menu.ans"), &hostile).unwrap();
    let mut set = ArtSet::bundled().unwrap();
    set.add_dir(dir.path());
    for size in [(80, 24), (132, 43), (80, 30)] {
        let on = shot(
            &ui,
            "neon",
            LayoutId::Classic,
            size,
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            Some(&set),
            true,
        );
        let off = shot(
            &ui,
            "neon",
            LayoutId::Classic,
            size,
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            Some(&ArtSet::bundled().unwrap()),
            true,
        );
        let last = size.1 - 1;
        for x in 0..size.0 {
            assert_eq!(
                on[(x, last)],
                off[(x, last)],
                "{size:?}: art reached the status row"
            );
        }
        for y in 0..last {
            for x in 0..size.0 {
                assert!(
                    !on[(x, y)].modifier.contains(Modifier::REVERSED),
                    "{size:?}"
                );
            }
        }
        assert!(!text(&on).contains('\u{1b}'));
    }
}

#[test]
fn art_cells_carry_colour_only() {
    let ui = ui();
    let buffer = shot(
        &ui,
        "ansi",
        LayoutId::Classic,
        (80, 24),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        None,
        true,
    );
    // Rows 1 to 3 hold the header art: no modifier on any of its cells.
    for y in 1..4 {
        for x in 0..80 {
            assert_eq!(buffer[(x, y)].modifier, Modifier::empty(), "{x},{y}");
        }
    }
}

#[test]
fn art_uses_only_the_depths_colours() {
    let ui = ui();
    let sixteen = shot(
        &ui,
        "ansi",
        LayoutId::Classic,
        (80, 24),
        Tier::Unicode,
        true,
        ColorDepth::Ansi16,
        None,
        true,
    );
    for cell in sixteen.content() {
        for color in [cell.fg, cell.bg] {
            assert!(
                !matches!(
                    color,
                    ratatui::style::Color::Rgb(..) | ratatui::style::Color::Indexed(_)
                ),
                "a 16-colour terminal got {color:?}"
            );
        }
    }
}

// ---- the login banner and the logoff screen ----

fn member_flow(settings: TerminalSettings) -> russh_bbs_core::terminal_settings::MemberFlow {
    russh_bbs_core::terminal_settings::MemberFlow::new(
        &sanitize_username("alice").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(1)),
            &Slug::MEMBER,
            10,
            &Via::Account,
            Permission::ALL.iter().copied(),
        ),
        None,
        settings,
    )
}

/// Draw a member screen the way a session does, at `size`, with art for `slot`.
fn member_shot(
    ui: &Ui,
    flow: &russh_bbs_core::terminal_settings::MemberFlow,
    size: (u16, u16),
    slot: Option<Slot>,
    depth: ColorDepth,
) -> Buffer {
    let settings = *flow.settings();
    let theme = ui.theme_for(&settings);
    let area = Rect::new(0, 0, size.0, size.1);
    let look = ui.look(&settings, area, depth);
    let strings = theme.strings();
    let mut view = flow.view(strings);
    if let (Some(Slot::Logoff), Some(quote)) = (slot, ui.board().quote_for(0)) {
        view.body.push(String::new());
        view.body.push(quote.as_str().to_owned());
    }
    let ctx = ui.art_ctx(ui.style_for(&settings));
    let hints = crate::ui::classic::content_hints(strings);
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            crate::settings::render_framed(
                frame,
                &view,
                &crate::settings::Framed {
                    strings,
                    look,
                    tier: settings.icon_tier(),
                    markers: &[SecurityMarker::Unencrypted, SecurityMarker::LowSecurity],
                    hints: &hints,
                    art: slot.map(|slot| (&ctx, slot)),
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn settings_for(style: &str, tier: Tier, color: bool) -> TerminalSettings {
    TerminalSettings::default()
        .with_tier(tier)
        .with_color(color)
        .with_theme(name(style))
}

#[test]
fn the_login_banner_of_every_style_shows_at_both_sizes_in_mono_and_ascii() {
    let ui = ui();
    for style in STYLES {
        let piece = ui
            .art()
            .pick(Slot::Login, name(style), 132, 43)
            .unwrap()
            .clone();
        for size in [(80, 24), (132, 43)] {
            for (tier, color) in [
                (Tier::Unicode, true),
                (Tier::Unicode, false),
                (Tier::Ascii, true),
                (Tier::Ascii, false),
            ] {
                let flow = member_flow(settings_for(style, tier, color));
                assert_eq!(flow.view(ui.strings()).heading, "member.login_heading");
                let buffer =
                    member_shot(&ui, &flow, size, Some(Slot::Login), ColorDepth::TrueColor);
                let ascii = tier == Tier::Ascii;
                let want = piece.grid().row_text(0, ascii).trim().to_owned();
                let screen = text(&buffer);
                assert!(
                    screen.contains(&want),
                    "{style} {size:?} {tier:?} color={color}\n{screen}"
                );
                assert!(
                    screen.contains("Signed in"),
                    "the login box survives\n{screen}"
                );
                if ascii {
                    assert!(screen.is_ascii());
                }
                // The status row is the same with and without art.
                let bare = member_shot(&ui, &flow, size, None, ColorDepth::TrueColor);
                for x in 0..size.0 {
                    assert_eq!(buffer[(x, size.1 - 1)], bare[(x, size.1 - 1)]);
                }
                assert!(
                    reserved_cells(&buffer)
                        .iter()
                        .all(|(_, y)| *y == size.1 - 1)
                );
            }
        }
    }
}

#[test]
fn the_logoff_screen_says_goodbye_under_art_and_a_quote_the_sysop_wrote() {
    use crate::config::BoardConfig;
    let board = Board::from_config(&BoardConfig {
        quotes: "Keep the line open.|See you in the morning.".into(),
        ..BoardConfig::default()
    })
    .unwrap();
    let ui = ui().with_board(board);
    let mut flow = member_flow(settings_for("amber", Tier::Unicode, true));
    // Log off from the home page: any key passes the login screen, then `l` ends the session.
    flow.input(MemberInput::Char('x'));
    flow.input(MemberInput::Char('l'));
    assert!(flow.is_logged_off());
    let buffer = member_shot(
        &ui,
        &flow,
        (80, 24),
        Some(Slot::Logoff),
        ColorDepth::TrueColor,
    );
    let screen = text(&buffer);
    assert!(screen.contains("Goodbye, alice."), "{screen}");
    assert!(screen.contains("Keep the line open."), "{screen}");
    // The style ships its own logoff picture (ART-1); it is what the screen draws.
    let logoff = ui.art().pick(Slot::Logoff, name("amber"), 80, 24).unwrap();
    assert!(
        screen.contains(logoff.grid().row_text(0, false).trim()),
        "{screen}"
    );
}

#[test]
fn a_tiny_screen_gets_no_banner_and_still_shows_the_login_box() {
    let ui = ui();
    let flow = member_flow(settings_for("neon", Tier::Unicode, true));
    let buffer = member_shot(
        &ui,
        &flow,
        (60, 14),
        Some(Slot::Login),
        ColorDepth::TrueColor,
    );
    let bare = member_shot(&ui, &flow, (60, 14), None, ColorDepth::TrueColor);
    assert_eq!(text(&buffer), text(&bare));
    assert!(text(&buffer).contains("Signed in"));
}

// ---- the sysop theme preview ----

fn sysop_flow(themes: &[&str]) -> russh_bbs_core::terminal_settings::MemberFlow {
    let mut flow = russh_bbs_core::terminal_settings::MemberFlow::new(
        &sanitize_username("alice").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(1)),
            &Slug::SYSOP,
            100,
            &Via::Account,
            Permission::ALL.iter().copied(),
        ),
        None,
        TerminalSettings::default()
            .with_tier(Tier::Unicode)
            .with_color(true),
    );
    flow.set_themes(themes.iter().map(|t| name(t)).collect());
    flow.input(MemberInput::Char('x'));
    flow
}

fn press(flow: &mut russh_bbs_core::terminal_settings::MemberFlow, keys: &str) {
    for c in keys.chars() {
        flow.input(MemberInput::Char(c));
    }
}

fn preview_shot(
    ui: &Ui,
    flow: &russh_bbs_core::terminal_settings::MemberFlow,
    size: (u16, u16),
    tier: Tier,
) -> Buffer {
    let preview = flow.preview().unwrap();
    let settings = TerminalSettings::default().with_tier(tier).with_color(true);
    let area = Rect::new(0, 0, size.0, size.1);
    let look = ui.look(&settings, area, ColorDepth::TrueColor);
    let markers = [SecurityMarker::Unencrypted, SecurityMarker::LowSecurity];
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            super::preview::render(
                frame,
                preview,
                &super::preview::PreviewCtx {
                    ui,
                    strings: ui.strings(),
                    look,
                    tier,
                    markers: &markers,
                    clock: Some("UTC 12:00"),
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn a_sysop_reaches_the_preview_from_sysop_tools_and_a_member_does_not() {
    let mut flow = sysop_flow(&STYLES);
    assert!(flow.preview().is_none());
    press(&mut flow, "ot");
    let preview = flow.preview().expect("the preview opened");
    assert_eq!(preview.size(), (80, 24));
    assert!(flow.menu_page().is_none());
    // Esc goes back to Sysop tools' parent: the main menu.
    flow.input(MemberInput::Escape);
    assert!(flow.preview().is_none());
    assert!(flow.menu_page().is_some());

    // A member without the permission never sees the item.
    let mut member = russh_bbs_core::terminal_settings::MemberFlow::new(
        &sanitize_username("bob").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(2)),
            &Slug::MEMBER,
            10,
            &Via::Account,
            Permission::ALL
                .iter()
                .copied()
                .filter(|p| *p != Permission::MembersApprove),
        ),
        None,
        TerminalSettings::default().with_tier(Tier::Unicode),
    );
    member.input(MemberInput::Char('x'));
    press(&mut member, "ot");
    assert!(member.preview().is_none());
}

#[test]
fn the_preview_keys_cycle_style_layout_size_tier_colour_sample_and_art() {
    let mut flow = sysop_flow(&STYLES);
    press(&mut flow, "ot");
    let start = flow.preview().unwrap().clone();
    press(&mut flow, "t");
    assert_ne!(flow.preview().unwrap().style(), start.style());
    // Cycling every style comes back to the first.
    press(&mut flow, "ttttt");
    assert_eq!(flow.preview().unwrap().style(), start.style());
    press(&mut flow, "l");
    assert_ne!(flow.preview().unwrap().layout(), start.layout());
    press(&mut flow, "z");
    assert_eq!(flow.preview().unwrap().size(), (100, 30));
    press(&mut flow, "zz");
    assert_eq!(flow.preview().unwrap().size(), (80, 24));
    press(&mut flow, "icsa");
    let p = flow.preview().unwrap();
    assert!(p.ascii() && p.mono() && !p.art());
    assert_eq!(
        p.sample(),
        russh_bbs_core::terminal_settings::PreviewSample::Login
    );
    // Nothing the preview does is saved: the member's own settings are untouched.
    assert_eq!(flow.settings().theme_choice(), None);
}

#[test]
fn the_preview_draws_every_style_at_every_size_with_art_in_mono_and_ascii() {
    let ui = ui();
    for style in STYLES {
        for tier_keys in ["", "i", "c", "ic"] {
            for size_presses in 0..3 {
                for sample_key in ["", "s"] {
                    let mut flow = sysop_flow(&STYLES);
                    press(&mut flow, "ot");
                    // Walk to the style, then set the rest.
                    while flow.preview().unwrap().style() != name(style) {
                        press(&mut flow, "t");
                    }
                    press(&mut flow, tier_keys);
                    press(&mut flow, sample_key);
                    for _ in 0..size_presses {
                        press(&mut flow, "z");
                    }
                    let p = flow.preview().unwrap().clone();
                    for real in [(80, 24), (132, 43)] {
                        let buffer = preview_shot(&ui, &flow, real, Tier::Unicode);
                        let screen = text(&buffer);
                        assert!(screen.contains("Theme preview"), "{screen}");
                        assert!(screen.contains(&format!("Style {style}")), "{screen}");
                        // The sample's first art row is on screen when it fits the real window.
                        let slot = if sample_key == "s" {
                            Slot::Login
                        } else {
                            Slot::Menu
                        };
                        if let Some(piece) =
                            ui.art().pick(slot, name(style), p.size().0, p.size().1)
                            && piece.grid().height() < 8
                            && real.0 >= p.size().0
                            && slot == Slot::Menu
                            && p.layout() == LayoutId::Classic
                            && p.size().1 >= 24
                        {
                            let ascii = p.ascii();
                            let want = piece.grid().row_text(0, ascii).trim().to_owned();
                            assert!(screen.contains(&want), "{style} {p:?} {real:?}\n{screen}");
                        }
                        // The real status row has the real marker; the sample has none, and
                        // nothing wears the reserved style outside that row's marker cell.
                        let last = real.1 - 1;
                        let width = marker_columns(Tier::Unicode, real.0);
                        for (x, y) in reserved_cells(&buffer) {
                            assert_eq!(y, last, "{style}: reserved style in the sample");
                            assert!(x < width);
                        }
                        for y in 0..last {
                            for x in 0..real.0 {
                                assert!(!buffer[(x, y)].modifier.contains(Modifier::REVERSED));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_preview_says_when_your_terminal_is_smaller_than_the_size_you_chose() {
    let ui = ui();
    let mut flow = sysop_flow(&STYLES);
    press(&mut flow, "otzz");
    let screen = text(&preview_shot(&ui, &flow, (80, 24), Tier::Unicode));
    assert!(screen.contains("top-left"), "{screen}");
    assert!(screen.contains("132x43"));
    let roomy = text(&preview_shot(&ui, &flow, (140, 50), Tier::Unicode));
    assert!(!roomy.contains("top-left"));
}

#[test]
fn the_preview_lists_art_files_that_were_refused_with_their_reasons() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("menu.ans"), b"\x1b[2J").unwrap();
    let ui = ui().with_art_dir(dir.path());
    let mut flow = sysop_flow(&STYLES);
    press(&mut flow, "ot");
    let screen = text(&preview_shot(&ui, &flow, (132, 43), Tier::Unicode));
    assert!(
        screen.contains("Art files refused (1): menu.ans"),
        "{screen}"
    );
    assert!(screen.contains("empty"), "{screen}");
}

#[test]
fn a_hostile_art_file_cannot_put_a_marker_in_the_preview() {
    let dir = tempfile::tempdir().unwrap();
    let mut hostile = Vec::new();
    for _ in 0..4 {
        hostile.extend_from_slice(b"\x1b[1;7;37m SERVER-READABLE \x1b[0m\r\n");
    }
    std::fs::write(dir.path().join("menu.ans"), &hostile).unwrap();
    let ui = ui().with_art_dir(dir.path());
    let mut flow = sysop_flow(&STYLES);
    press(&mut flow, "ot");
    let buffer = preview_shot(&ui, &flow, (80, 24), Tier::Unicode);
    assert!(
        text(&buffer).contains("SERVER-READABLE"),
        "the art is drawn as text"
    );
    let last = 23;
    for (_, y) in reserved_cells(&buffer) {
        assert_eq!(y, last);
    }
}

#[test]
fn losing_the_permission_closes_the_preview() {
    let mut flow = sysop_flow(&STYLES);
    press(&mut flow, "ot");
    assert!(flow.preview().is_some());
    flow.set_access(Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| *p != Permission::MembersApprove),
    ));
    press(&mut flow, "t");
    assert!(flow.preview().is_none());
}
