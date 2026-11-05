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
    // ESC [ A is "up" and so on; a naive scan would see 'A', and ESC O q must not read as 'q'.
    assert_eq!(parse_keys(b"\x1b[A"), vec![Key::Up]);
    assert_eq!(parse_keys(b"\x1b[B"), vec![Key::Down]);
    assert_eq!(parse_keys(b"\x1b[C"), vec![Key::Right]);
    assert_eq!(parse_keys(b"\x1b[D"), vec![Key::Left]);
    // A modified arrow (Ctrl-Up) is consumed whole, as one key; anes reports it unmodified.
    assert_eq!(parse_keys(b"\x1b[1;5A"), vec![Key::Up]);
    assert_eq!(parse_keys(b"\x1b[1;5D"), vec![Key::Left]);
    assert!(!parse_keys(b"\x1bOq").iter().any(|k| k.is_quit()));
    // A modified key is consumed whole; its final `q` is not a quit.
    assert!(!parse_keys(b"\x1b[1;5q").iter().any(|k| k.is_quit()));
}

#[test]
fn ss3_cursor_keys_are_the_same_keys() {
    assert_eq!(parse_keys(b"\x1bOA"), vec![Key::Up]);
    assert_eq!(parse_keys(b"\x1bOB"), vec![Key::Down]);
    assert_eq!(parse_keys(b"\x1bOC"), vec![Key::Right]);
    assert_eq!(parse_keys(b"\x1bOD"), vec![Key::Left]);
    assert_eq!(parse_keys(b"\x1bOH"), vec![Key::Home]);
    assert_eq!(parse_keys(b"\x1bOF"), vec![Key::End]);
    // F1 stays a swallowed key; an unknown SS3 final is swallowed and never typed.
    assert_eq!(parse_keys(b"\x1bOP"), vec![Key::Other]);
    assert_eq!(parse_keys(b"\x1bOzq"), vec![Key::Char('q')]);
    assert_eq!(parse_keys(b"\x1bOAq"), vec![Key::Up, Key::Char('q')]);
}

#[test]
fn home_end_delete_and_paging_in_every_common_form() {
    for (bytes, key) in [
        (&b"\x1b[H"[..], Key::Home),
        (b"\x1b[1~", Key::Home),
        (b"\x1b[7~", Key::Home),
        (b"\x1b[F", Key::End),
        (b"\x1b[4~", Key::End),
        (b"\x1b[8~", Key::End),
        (b"\x1b[3~", Key::Delete),
        (b"\x1b[5~", Key::PageUp),
        (b"\x1b[6~", Key::PageDown),
    ] {
        assert_eq!(parse_keys(bytes), vec![key], "{bytes:?}");
    }
    // Insert and function keys are still swallowed whole.
    assert_eq!(parse_keys(b"\x1b[2~"), vec![Key::Other]);
    assert_eq!(parse_keys(b"\x1b[15~"), vec![Key::Other]);
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
    b"\x1bOA\x1bOD\x1bOH\x1bOFq",
    b"\x1bOz",
    b"\x1b[C\x1b[D\x1b[H\x1b[F\x1b[1~\x1b[4~\x1b[7~\x1b[8~\x1b[5~\x1b[6~",
    b"\x1b[3~",
    b"\x1b[Aq\x1b[Br",
    b"a\x1b[Ab\r",
    "\u{e9}x".as_bytes(),
    b"\x1bx",
    b"\x1b\x1b[A",
    b"\x1b[200~hi\x1b[201~q",
    b"\x1b[200~a\x1b[Ab\rc\x1b[201~z",
    b"\x1b[200~one\r\ntwo\nthree\r\r\nx\x1b[201~\r",
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
    // Ctrl-C, Tab and the Esc byte inside a paste are dropped; what is left of an escape
    // sequence is its printable letters, as text. A line break is a PasteBreak, never Enter.
    let keys = parse_keys(b"\x1b[200~ab\r\x03\x1b[A\tq\x1b[201~");
    assert_eq!(
        keys,
        vec![
            Key::Char('a'),
            Key::Char('b'),
            Key::PasteBreak,
            Key::Char('['),
            Key::Char('A'),
            Key::Char('q')
        ],
        "only printable ASCII and line breaks survive, and the markers are not keys"
    );
    assert!(!keys.iter().any(|k| matches!(k, Key::Enter | Key::Ctrl(_))));
}

#[test]
fn a_pasted_line_break_is_one_paste_break_whether_cr_lf_or_crlf() {
    let breaks = |bytes: &[u8]| {
        parse_keys(bytes)
            .into_iter()
            .filter(|k| *k == Key::PasteBreak)
            .count()
    };
    assert_eq!(breaks(b"\x1b[200~a\rb\x1b[201~"), 1);
    assert_eq!(breaks(b"\x1b[200~a\nb\x1b[201~"), 1);
    assert_eq!(breaks(b"\x1b[200~a\r\nb\x1b[201~"), 1);
    assert_eq!(
        breaks(b"\x1b[200~a\n\nb\x1b[201~"),
        2,
        "a blank line is kept"
    );
    assert_eq!(breaks(b"\x1b[200~a\r\rb\x1b[201~"), 2);
    assert_eq!(breaks(b"\x1b[200~a\n\r\nb\x1b[201~"), 2);
    // Typed Enter and LF outside a paste stay Enter.
    assert_eq!(
        parse_keys(b"a\r\n"),
        vec![Key::Char('a'), Key::Enter, Key::Enter]
    );
}

#[test]
fn a_crlf_cut_between_cr_and_lf_is_still_one_break() {
    let whole = parse_keys(b"\x1b[200~a\r\nb\x1b[201~");
    assert_eq!(feed_in_pieces(&[b"\x1b[200~a\r", b"\nb\x1b[201~"]), whole);
    assert_eq!(whole, vec![Key::Char('a'), Key::PasteBreak, Key::Char('b')]);
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

// ---- UI-9: mouse reports are read, bounded, and never become keys ----

mod mouse {
    #![allow(clippy::assert_is_empty)]

    use std::time::Instant;

    use super::{Key, KeyParser};
    use crate::ui::input::mouse::{Button, Kind};

    #[test]
    fn an_sgr_report_is_a_mouse_event_and_no_key() {
        let mut parser = KeyParser::new();
        let keys = parser.feed(b"\x1b[<0;12;7M", Instant::now());
        assert!(keys.is_empty(), "{keys:?}");
        let events = parser.take_mouse();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Down(Button::Left));
        assert_eq!((events[0].col, events[0].row), (11, 6));
        assert!(parser.take_mouse().is_empty(), "taken once");
    }

    #[test]
    fn a_report_split_across_packets_is_one_event_and_keys_around_it_survive() {
        let mut parser = KeyParser::new();
        let mut keys = parser.feed(b"a\x1b[<6", Instant::now());
        keys.extend(parser.feed(b"4;3;4Mb", Instant::now()));
        assert_eq!(keys, vec![Key::Char('a'), Key::Char('b')]);
        assert_eq!(parser.take_mouse().len(), 1);
    }

    #[test]
    fn an_invalid_or_oversized_report_is_dropped_and_never_echoed_as_keys() {
        let mut parser = KeyParser::new();
        // Invalid fields, then an oversized run of digits, each followed by a real key.
        let mut bytes = b"\x1b[<x;1;1Mq".to_vec();
        bytes.extend_from_slice(b"\x1b[<");
        // Longer than a report may be, but short enough that the skip reaches its `M`.
        bytes.extend(std::iter::repeat_n(b'7', 40));
        bytes.extend_from_slice(b"Mz");
        let keys = parser.feed(&bytes, Instant::now());
        assert!(parser.take_mouse().is_empty());
        // Only the real keys after each dropped report survive; no digit, `;` or `<` leaked.
        assert!(
            keys.iter().all(|k| matches!(k, Key::Char('q' | 'z'))),
            "{keys:?}"
        );
        assert!(
            keys.contains(&Key::Char('q')) && keys.contains(&Key::Char('z')),
            "{keys:?}"
        );
    }

    #[test]
    fn a_stuck_discard_ends_so_it_cannot_swallow_the_session() {
        let mut parser = KeyParser::new();
        // An oversized report with no final byte, then more digits, then text.
        let mut bytes = b"\x1b[<".to_vec();
        bytes.extend(std::iter::repeat_n(b'1', 100));
        bytes.extend_from_slice(b"hello");
        let keys = parser.feed(&bytes, Instant::now());
        // The report and the 64 bytes after it are skipped; then typing resumes, text intact.
        let text: String = keys
            .iter()
            .filter_map(|k| if let Key::Char(c) = k { Some(*c) } else { None })
            .collect();
        assert!(text.ends_with("hello"), "{text:?}");
        assert!(text.len() < 100, "the skipped bytes are gone: {text:?}");
        let tail = parser.feed(b"xyz", Instant::now());
        assert_eq!(tail, vec![Key::Char('x'), Key::Char('y'), Key::Char('z')]);
    }

    fn chars(keys: &[Key]) -> String {
        keys.iter()
            .map(|k| match k {
                Key::Char(c) => *c,
                _ => '?',
            })
            .collect()
    }

    /// QA's N2 repro: `ESC [ < 0 ; 1 ;`, 400 nines, `M` opened Mail.
    #[test]
    fn an_oversized_report_leaks_no_tail_as_keys() {
        let mut parser = KeyParser::new();
        let mut bytes = b"\x1b[<0;1;".to_vec();
        bytes.extend(std::iter::repeat_n(b'9', 400));
        bytes.push(b'M');
        bytes.extend_from_slice(b"mq");
        let keys = parser.feed(&bytes, Instant::now());
        assert_eq!(chars(&keys), "mq", "{keys:?}");
        assert!(parser.take_mouse().is_empty());
    }

    #[test]
    fn an_oversized_report_split_across_packets_leaks_nothing() {
        let mut parser = KeyParser::new();
        let mut bytes = b"\x1b[<0;1;".to_vec();
        bytes.extend(std::iter::repeat_n(b'9', 400));
        bytes.extend_from_slice(b"Mm");
        let mut keys = Vec::new();
        for chunk in bytes.chunks(7) {
            keys.extend(parser.feed(chunk, Instant::now()));
        }
        assert_eq!(chars(&keys), "m", "{keys:?}");
    }

    #[test]
    fn a_run_past_the_resync_bound_ends_and_typing_resumes() {
        let mut parser = KeyParser::new();
        let mut bytes = b"\x1b[<".to_vec();
        bytes.extend(std::iter::repeat_n(
            b'9',
            super::super::mouse::MAX_RESYNC + 10,
        ));
        let _ = parser.feed(&bytes, Instant::now());
        let keys = parser.feed(b"ab", Instant::now());
        assert_eq!(chars(&keys), "ab");
    }

    #[test]
    fn the_event_queue_is_bounded() {
        let mut parser = KeyParser::new();
        let mut bytes = Vec::new();
        for _ in 0..1000 {
            bytes.extend_from_slice(b"\x1b[<0;1;1M");
        }
        parser.feed(&bytes, Instant::now());
        assert!(parser.take_mouse().len() <= super::super::MAX_MOUSE_QUEUE);
    }

    #[test]
    fn arrows_pastes_and_escape_still_work_beside_the_mouse_parser() {
        let mut parser = KeyParser::new();
        let keys = parser.feed(
            b"\x1b[A\x1b[B\x1b[200~hi\x1b[201~\x1b[<0;1;1M\x1b[C",
            Instant::now(),
        );
        assert_eq!(
            keys,
            vec![
                Key::Up,
                Key::Down,
                Key::Char('h'),
                Key::Char('i'),
                Key::Right
            ]
        );
        let lone = parser.feed(b"\x1b", Instant::now());
        assert!(lone.is_empty());
        assert_eq!(parser.expire(), vec![Key::Escape]);
    }

    mod props {
        use proptest::prelude::*;

        use super::{Instant, KeyParser};

        proptest! {
            /// Arbitrary bytes, cut anywhere, never panic; the queue never exceeds its cap; and
            /// no printable ASCII that sits inside a mouse report comes out as a key.
            #[test]
            fn arbitrary_bytes_never_panic_or_exceed_caps(
                chunks in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..40), 0..12),
            ) {
                let mut parser = KeyParser::new();
                for chunk in &chunks {
                    let _ = parser.feed(chunk, Instant::now());
                    prop_assert!(parser.mouse.len() <= super::super::super::MAX_MOUSE_QUEUE);
                    prop_assert!(parser.held.len() <= 32);
                }
                let _ = parser.expire();
            }

            /// No byte of an over-cap report reaches the key stream, however it is cut into
            /// packets, and the keys typed after it still work.
            #[test]
            fn an_over_cap_report_never_reaches_the_key_stream(
                body in proptest::collection::vec(
                    prop_oneof![Just(b'9'), Just(b'0'), Just(b';'), Just(b'-'), Just(b'<')],
                    65..3000,
                ),
                end in prop_oneof![Just(b'M'), Just(b'm')],
                cut in 1_usize..200,
                tail in "[a-z ]{0,12}",
            ) {
                let mut bytes = b"\x1b[<0;1;".to_vec();
                bytes.extend(&body);
                bytes.push(end);
                bytes.extend(tail.bytes());
                let mut parser = KeyParser::new();
                let mut keys = Vec::new();
                for chunk in bytes.chunks(cut) {
                    keys.extend(parser.feed(chunk, Instant::now()));
                }
                let text: String = keys
                    .iter()
                    .map(|k| match k { super::super::super::Key::Char(c) => *c, _ => '?' })
                    .collect();
                prop_assert_eq!(text, tail);
            }

            #[test]
            fn well_formed_reports_never_leak_keys(
                code in 0_u32..3, x in 1_u32..500, y in 1_u32..200, n in 1_usize..20,
            ) {
                let mut parser = KeyParser::new();
                let report = format!("\x1b[<{code};{x};{y}M");
                let bytes = report.repeat(n);
                let keys = parser.feed(bytes.as_bytes(), Instant::now());
                prop_assert!(keys.is_empty());
                prop_assert_eq!(parser.take_mouse().len(), n);
            }
        }
    }
}

mod replies {
    use std::time::Instant;

    use super::super::{Key, KeyParser};

    #[test]
    fn a_cursor_report_is_an_acknowledgement_and_no_key() {
        let mut parser = KeyParser::new();
        let keys = parser.feed(b"\x1b[24;1R", Instant::now());
        assert!(keys.is_empty(), "{keys:?}");
        assert_eq!(parser.take_acks(), 1);
        assert_eq!(parser.take_acks(), 0, "taken once");
    }

    #[test]
    fn a_report_split_at_every_byte_is_one_acknowledgement_and_keys_around_it_survive() {
        let stream = b"a\x1b[40;120Rb\x1b[A";
        for cut in 0..=stream.len() {
            let mut parser = KeyParser::new();
            let (head, tail) = stream.split_at(cut);
            let mut keys = parser.feed(head, Instant::now());
            keys.extend(parser.feed(tail, Instant::now()));
            assert_eq!(
                keys,
                vec![Key::Char('a'), Key::Char('b'), Key::Up],
                "cut at {cut}"
            );
            assert_eq!(parser.take_acks(), 1, "cut at {cut}");
        }
    }

    #[test]
    fn modified_f3_and_other_csi_keys_are_still_keys() {
        // `ESC [ 1 ; 2 R` is Shift+F3: row 1 is never taken as a reply.
        let mut parser = KeyParser::new();
        let keys = parser.feed(b"\x1b[1;2R", Instant::now());
        assert_eq!(parser.take_acks(), 0);
        assert!(!keys.is_empty(), "still a key: {keys:?}");
        // Arrows with modifiers, Home/End/PageUp/Delete in their numeric forms.
        for bytes in [&b"\x1b[1;5A"[..], b"\x1b[3~", b"\x1b[5~", b"\x1b[1~"] {
            let mut parser = KeyParser::new();
            let keys = parser.feed(bytes, Instant::now());
            assert_eq!(keys.len(), 1, "{bytes:?}: {keys:?}");
            assert_eq!(parser.take_acks(), 0);
        }
    }

    #[test]
    fn malformed_or_oversized_reports_are_never_acknowledgements() {
        for bytes in [
            &b"\x1b[R"[..],
            b"\x1b[;R",
            b"\x1b[24R",
            b"\x1b[24;1;3R",
            b"\x1b[99999999;1R",
            b"\x1b[1234567890123;1R",
        ] {
            let mut parser = KeyParser::new();
            parser.feed(bytes, Instant::now());
            assert_eq!(parser.take_acks(), 0, "{bytes:?}");
        }
    }

    #[test]
    fn a_truncated_report_times_out_and_does_not_swallow_the_session() {
        let mut parser = KeyParser::new();
        assert_eq!(parser.feed(b"\x1b[24;", Instant::now()), Vec::<Key>::new());
        assert!(parser.deadline().is_some(), "held bytes have a deadline");
        assert_eq!(
            parser.expire(),
            Vec::<Key>::new(),
            "dropped, as a CSI stub is"
        );
        assert_eq!(parser.feed(b"q", Instant::now()), vec![Key::Char('q')]);
    }

    #[test]
    fn an_endless_stream_of_replies_never_grows_anything() {
        let mut parser = KeyParser::new();
        for _ in 0..10_000 {
            parser.feed(b"\x1b[24;80R", Instant::now());
        }
        assert_eq!(parser.take_acks(), 10_000);
    }
}
