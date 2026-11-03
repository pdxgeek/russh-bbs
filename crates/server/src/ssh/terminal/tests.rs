use std::time::{Duration, Instant};

use super::{
    ESC_TIMEOUT, Key, KeyParser, MAX_COLS, MAX_ROWS, PASTE_IDLE, PtySize, RenderCap, parse_keys,
};

#[test]
fn zero_means_default_size() {
    let size = PtySize::from_request(0, 0);
    assert_eq!((size.cols(), size.rows()), (80, 24));
}

#[test]
fn normal_size_is_kept() {
    let size = PtySize::from_request(132, 50);
    assert_eq!((size.cols(), size.rows()), (132, 50));
}

#[test]
fn hostile_sizes_are_clamped() {
    for (cols, rows) in [(u32::MAX, u32::MAX), (65_535, 65_535), (65_536, 70_000)] {
        let size = PtySize::from_request(cols, rows);
        assert_eq!(
            (size.cols(), size.rows()),
            (MAX_COLS, MAX_ROWS),
            "{cols}x{rows}"
        );
    }
}

#[test]
fn render_cap_defaults_to_the_whole_clamped_window() {
    let cap = RenderCap::default();
    assert_eq!((cap.cols(), cap.rows()), (MAX_COLS, MAX_ROWS));
    assert_eq!(cap.cells(), 100_000);
}

#[test]
fn render_cap_is_kept_between_the_smallest_screen_and_the_clamp() {
    assert_eq!(RenderCap::new(1, 1), RenderCap::new(80, 24));
    assert_eq!(RenderCap::new(u16::MAX, u16::MAX), RenderCap::default());
    let cap = RenderCap::new(300, 100);
    assert_eq!((cap.cols(), cap.rows()), (300, 100));
    // Two buffers of cells; the bill the sysop multiplies by the connection cap.
    assert_eq!(
        cap.bytes_per_session(),
        2 * 30_000 * std::mem::size_of::<ratatui::buffer::Cell>()
    );
}

#[test]
fn plain_q_quits_in_either_case() {
    assert!(parse_keys(b"q").iter().any(|k| k.is_quit()));
    assert!(parse_keys(b"Q").iter().any(|k| k.is_quit()));
}

#[test]
fn ctrl_c_and_ctrl_d_quit() {
    assert_eq!(parse_keys(&[0x03]), vec![Key::Ctrl('c')]);
    assert!(parse_keys(&[0x03]).iter().any(|k| k.is_quit()));
    assert!(parse_keys(&[0x04]).iter().any(|k| k.is_quit()));
}

#[test]
fn other_letters_do_not_quit() {
    assert!(!parse_keys(b"hello world\r").iter().any(|k| k.is_quit()));
}

#[test]
fn arrow_keys_are_not_read_as_letters() {
    // ESC [ A is "up" and ESC [ B "down" (the lightbar cursor keys); a naive scan would see
    // 'A', and ESC O q must not read as 'q'. Left and right stay ignored.
    assert_eq!(parse_keys(b"\x1b[A"), vec![Key::Up]);
    assert_eq!(parse_keys(b"\x1b[B"), vec![Key::Down]);
    assert_eq!(parse_keys(b"\x1b[C"), vec![Key::Other]);
    assert_eq!(parse_keys(b"\x1b[D"), vec![Key::Other]);
    // A modified arrow (Ctrl-Up) is consumed whole, as one key; anes reports it as Up.
    assert_eq!(parse_keys(b"\x1b[1;5A"), vec![Key::Up]);
    assert!(!parse_keys(b"\x1bOq").iter().any(|k| k.is_quit()));
    // A modified key is consumed whole; its final `q` is not a quit.
    assert!(!parse_keys(b"\x1b[1;5q").iter().any(|k| k.is_quit()));
}

#[test]
fn a_lone_escape_is_the_escape_key_and_a_truncated_sequence_is_dropped() {
    assert_eq!(parse_keys(b"\x1b"), vec![Key::Escape]);
    // `ESC [ 1 2` could still become a paste marker prefix, so it is held and then dropped.
    assert_eq!(parse_keys(b"\x1b[2"), vec![Key::Other]);
    assert_eq!(
        parse_keys(b"\x1b[12"),
        Vec::<Key>::new(),
        "settled as a CSI stub"
    );
}

#[test]
fn keys_after_an_escape_sequence_are_still_seen() {
    assert_eq!(parse_keys(b"\x1b[Aq"), vec![Key::Up, Key::Char('q')]);
}

#[test]
fn arbitrary_bytes_never_panic() {
    for first in 0..=255_u8 {
        let _ = parse_keys(&[first, 0x1b, first, b'[', first]);
    }
}

#[test]
fn enter_backspace_and_escape_are_distinct_keys() {
    assert_eq!(parse_keys(b"\r"), vec![Key::Enter]);
    assert_eq!(parse_keys(b"\n"), vec![Key::Enter]);
    assert_eq!(parse_keys(&[0x7f]), vec![Key::Backspace]);
    assert_eq!(parse_keys(&[0x08]), vec![Key::Backspace]);
    assert_eq!(
        parse_keys(b"ab\x1b"),
        vec![Key::Char('a'), Key::Char('b'), Key::Escape]
    );
}

/// Inputs whose key meaning must not depend on where the network cuts them.
const STREAMS: &[&[u8]] = &[
    b"abc",
    b"\x1b[A",
    b"\x1b[1;5A",
    b"\x1bOP",
    b"\x1b[3~",
    b"\x1b[Aq\x1b[Br",
    b"a\x1b[Ab\r",
    "\u{e9}x".as_bytes(),
    b"\x1bx",
    b"\x1b\x1b[A",
    b"\x1b[200~hi\x1b[201~q",
    b"\x1b[200~a\x1b[Ab\rc\x1b[201~z",
    b"\x03\x04\x7f",
];

fn feed_in_pieces(pieces: &[&[u8]]) -> Vec<Key> {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    let mut keys = Vec::new();
    for piece in pieces {
        keys.extend(parser.feed(piece, now));
    }
    keys.extend(parser.expire());
    keys
}

#[test]
fn a_stream_means_the_same_split_at_every_byte_boundary() {
    for stream in STREAMS {
        let whole = parse_keys(stream);
        for cut in 0..=stream.len() {
            let (a, b) = stream.split_at(cut);
            assert_eq!(feed_in_pieces(&[a, b]), whole, "{stream:?} cut at {cut}");
        }
    }
}

#[test]
fn a_stream_means_the_same_split_into_three_pieces_and_into_single_bytes() {
    for stream in STREAMS {
        let whole = parse_keys(stream);
        for first in 0..=stream.len() {
            for second in first..=stream.len() {
                let pieces = [&stream[..first], &stream[first..second], &stream[second..]];
                assert_eq!(
                    feed_in_pieces(&pieces),
                    whole,
                    "{stream:?} cuts {first},{second}"
                );
            }
        }
        let singles: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(feed_in_pieces(&singles), whole, "{stream:?} byte by byte");
    }
}

#[test]
fn the_escape_split_from_its_sequence_is_an_arrow_not_escape_and_garbage() {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    assert_eq!(parser.feed(b"\x1b", now), Vec::<Key>::new(), "ESC is held");
    assert!(parser.deadline().is_some());
    // The old stateless parser read this as Escape followed by the letters `[A`.
    assert_eq!(parser.feed(b"[A", now), vec![Key::Up]);
    assert!(parser.deadline().is_none());
}

#[test]
fn a_held_escape_becomes_escape_at_its_deadline() {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    assert_eq!(parser.feed(b"\x1b", now), Vec::<Key>::new());
    assert_eq!(parser.deadline(), Some(now + ESC_TIMEOUT));
    assert_eq!(parser.expire(), vec![Key::Escape]);
    assert!(parser.deadline().is_none());
    // After the timeout the next bytes start fresh: `[A` is two letters, as typed.
    assert_eq!(
        parser.feed(b"[A", now + Duration::from_secs(1)),
        vec![Key::Char('['), Key::Char('A')]
    );
}

#[test]
fn bracketed_paste_is_text_not_commands() {
    // Newline, Ctrl-C, Tab and the Esc byte inside a paste are dropped; what is left of an
    // escape sequence is its printable letters, as text.
    let keys = parse_keys(b"\x1b[200~ab\r\x03\x1b[A\tq\x1b[201~");
    assert_eq!(
        keys,
        vec![
            Key::Char('a'),
            Key::Char('b'),
            Key::Char('['),
            Key::Char('A'),
            Key::Char('q')
        ],
        "only printable ASCII survives, and the markers are not keys"
    );
    assert!(!keys.iter().any(|k| matches!(k, Key::Enter | Key::Ctrl(_))));
}

#[test]
fn keys_after_a_paste_work_again() {
    assert_eq!(
        parse_keys(b"\x1b[200~x\x1b[201~\r"),
        vec![Key::Char('x'), Key::Enter]
    );
}

#[test]
fn a_paste_whose_end_marker_never_arrives_times_out() {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    parser.feed(b"\x1b[200~abc", now);
    assert_eq!(parser.deadline(), Some(now + PASTE_IDLE));
    assert_eq!(parser.expire(), Vec::<Key>::new());
    assert_eq!(parser.feed(b"q", now), vec![Key::Char('q')]);
    assert_eq!(parser.feed(b"\x03", now), vec![Key::Ctrl('c')]);
}

#[test]
fn an_endless_paste_ends_by_itself() {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    parser.feed(b"\x1b[200~", now);
    let junk = vec![b'x'; 20 * 1024];
    parser.feed(&junk, now);
    assert_eq!(
        parser.feed(b"\x03", now),
        vec![Key::Ctrl('c')],
        "paste mode left"
    );
}

#[test]
fn control_characters_are_keys_outside_a_paste() {
    assert_eq!(parse_keys(b"\x03"), vec![Key::Ctrl('c')]);
    assert_eq!(parse_keys(b"\x04"), vec![Key::Ctrl('d')]);
    assert_eq!(parse_keys(b"\t"), vec![Key::Ctrl('i')]);
}

#[test]
fn hostile_streams_never_panic_or_grow_without_bound() {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    for round in 0..64_u8 {
        let chunk: Vec<u8> = (0..=255_u8).map(|b| b.wrapping_mul(round | 1)).collect();
        let _ = parser.feed(&chunk, now);
        let _ = parser.expire();
    }
    // A long run of ESC bytes keeps at most one held.
    let escapes = vec![0x1b_u8; 100_000];
    let keys = parser.feed(&escapes, now);
    assert!(keys.len() <= escapes.len());
    assert!(parser.expire().len() <= 1);
}
