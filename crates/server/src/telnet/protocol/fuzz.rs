//! H-1: fuzz-style property tests for the telnet decoder over arbitrary byte streams cut at
//! arbitrary packet boundaries, plus the targeted hostile cases. Stable `proptest`, fast enough for
//! `cargo test`; the long run is `#[ignore]`d (`cargo test -p russh-bbs-server telnet::protocol::fuzz
//! -- --ignored`).
#![allow(clippy::expect_used, clippy::assert_is_empty, clippy::panic)]

use std::time::Instant;

use proptest::prelude::*;

use super::{
    DO, DONT, Decoded, Decoder, IAC, MAX_SUBNEGOTIATION, NUL, OPT_NAWS, OPT_TTYPE, SB, SE, WILL,
    WONT, encode,
};
use crate::ssh::terminal::{Key, KeyParser};
use crate::telnet::connection::clamp_window;

/// The most reply bytes a decoder may ever produce: one `DONT` and one `WONT` per option, and one
/// terminal-type request.
const MAX_REPLY: usize = 2 * 256 * 3 + 6;

/// Bytes weighted towards the ones the state machine cares about, so streams reach deep states.
fn hostile_byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        3 => Just(IAC),
        2 => prop::sample::select(vec![DO, DONT, WILL, WONT, SB, SE, 241, 244, 247, 248]),
        2 => prop::sample::select(vec![OPT_NAWS, OPT_TTYPE, 0, 1, 3]),
        1 => Just(b'\r'),
        1 => Just(b'\n'),
        1 => Just(0x1b),
        3 => any::<u8>(),
    ]
}

fn stream() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(hostile_byte(), 0..600)
}

/// Cut points as fractions, applied to the stream (so any length works).
fn cuts() -> impl Strategy<Value = Vec<usize>> {
    prop::collection::vec(any::<usize>(), 0..24)
}

fn split<'a>(wire: &'a [u8], cuts: &[usize]) -> Vec<&'a [u8]> {
    let mut points: Vec<usize> = cuts.iter().map(|c| c % (wire.len() + 1)).collect();
    points.sort_unstable();
    let mut pieces = Vec::new();
    let mut last = 0;
    for point in points {
        pieces.push(&wire[last..point]);
        last = point;
    }
    pieces.push(&wire[last..]);
    pieces
}

/// Feed `pieces` into one decoder with one accumulating output.
fn run_accumulating(pieces: &[&[u8]]) -> (Decoder, Decoded) {
    let mut decoder = Decoder::new();
    let mut out = Decoded::default();
    for piece in pieces {
        decoder.feed(piece, &mut out);
    }
    (decoder, out)
}

/// Feed `pieces` with a fresh output per piece, merged the way the connection merges them: data
/// and replies appended, the latest window kept, the first terminal type kept.
fn run_per_packet(pieces: &[&[u8]]) -> Decoded {
    let mut decoder = Decoder::new();
    let mut merged = Decoded::default();
    for piece in pieces {
        let mut out = Decoded::default();
        decoder.feed(piece, &mut out);
        merged.data.extend(out.data);
        merged.reply.extend(out.reply);
        if out.window.is_some() {
            merged.window = out.window;
        }
        if merged.terminal.is_none() {
            merged.terminal = out.terminal;
        }
    }
    merged
}

/// A reply is a sequence of whole protocol frames and nothing else: never a byte of client data.
fn assert_only_frames(reply: &[u8]) {
    let mut rest = reply;
    while !rest.is_empty() {
        match rest {
            [IAC, DONT | WONT, _option, tail @ ..] => rest = tail,
            [IAC, SB, OPT_TTYPE, 1, IAC, SE, tail @ ..] => rest = tail,
            other => panic!("reply is not a protocol frame: {other:?}"),
        }
    }
}

fn assert_bounded(decoder: &Decoder, out: &Decoded, input: usize) {
    assert!(
        decoder.sb.len() <= MAX_SUBNEGOTIATION,
        "sb {}",
        decoder.sb.len()
    );
    assert!(
        decoder.sb.capacity() <= 4 * MAX_SUBNEGOTIATION,
        "sb capacity"
    );
    assert!(out.data.len() <= input, "data cannot outgrow its input");
    assert!(out.reply.len() <= MAX_REPLY, "reply {}", out.reply.len());
    if let Some(name) = &out.terminal {
        assert!(!name.is_empty() && name.len() <= 40, "terminal {name:?}");
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+')),
            "terminal {name:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Nothing panics, memory and replies stay bounded, and a reply is only whole protocol
    /// frames, whatever the stream and however it is cut.
    #[test]
    fn arbitrary_streams_are_bounded_and_answer_only_in_frames(wire in stream(), cuts in cuts()) {
        let pieces = split(&wire, &cuts);
        let (decoder, out) = run_accumulating(&pieces);
        assert_bounded(&decoder, &out, wire.len());
        assert_only_frames(&out.reply);
    }

    /// The output is the same however the input is split: one read, byte by byte, or at
    /// arbitrary points, with the output accumulated or merged per packet.
    #[test]
    fn output_does_not_depend_on_packet_boundaries(wire in stream(), cuts in cuts()) {
        let (_, whole) = run_accumulating(&[&wire]);
        let bytes: Vec<&[u8]> = wire.chunks(1).collect();
        let (_, single) = run_accumulating(&bytes);
        let (_, cut) = run_accumulating(&split(&wire, &cuts));
        let merged = run_per_packet(&split(&wire, &cuts));
        for other in [&single, &cut, &merged] {
            prop_assert_eq!(&whole.data, &other.data);
            prop_assert_eq!(&whole.reply, &other.reply);
            prop_assert_eq!(whole.window, other.window);
            prop_assert_eq!(&whole.terminal, &other.terminal);
        }
    }

    /// Whatever the client sends, the keys that come out of decoder plus key parser are typed
    /// printable ASCII or named keys: no control byte or escape sequence becomes text that a
    /// prompt could echo (the login prompt echoes `Key::Char` and nothing else).
    #[test]
    fn typed_text_is_printable_ascii_however_the_input_is_cut(wire in stream(), cuts in cuts()) {
        let mut decoder = Decoder::new();
        let mut parser = KeyParser::new();
        let mut keys = Vec::new();
        for piece in split(&wire, &cuts) {
            let mut out = Decoded::default();
            decoder.feed(piece, &mut out);
            keys.extend(parser.feed(&out.data, Instant::now()));
        }
        keys.extend(parser.expire());
        for key in keys {
            if let Key::Char(c) = key {
                prop_assert!((' '..='~').contains(&c), "typed {c:?}");
            }
        }
    }

    /// A client that answers every reply with the opposite request cannot start a storm: the
    /// decoder never answers a refusal, so the conversation ends after the first round.
    #[test]
    fn a_client_that_answers_every_reply_cannot_start_a_loop(options in prop::collection::vec(any::<u8>(), 1..64)) {
        let mut decoder = Decoder::new();
        let mut wire: Vec<u8> = options
            .iter()
            .flat_map(|&o| [IAC, WILL, o, IAC, DO, o])
            .collect();
        let mut total = 0;
        for round in 0..50 {
            let mut out = Decoded::default();
            decoder.feed(&wire, &mut out);
            total += out.reply.len();
            prop_assert!(total <= MAX_REPLY, "round {round}: {total} reply bytes");
            // The client's answer to every frame it was sent is the same request again, plus
            // the mirrored one.
            wire = out
                .reply
                .chunks(3)
                .filter(|frame| frame.len() == 3)
                .flat_map(|frame| [IAC, WILL, frame[2], IAC, DO, frame[2], IAC, DONT, frame[2], IAC, WONT, frame[2]])
                .collect();
        }
        prop_assert!(wire.is_empty(), "the exchange did not end");
    }

    /// What we encode for the wire decodes to itself (apart from the line-end rules), and never
    /// reads back as a command, a reply or a window size.
    #[test]
    fn encoded_output_never_reads_back_as_protocol(screen in prop::collection::vec(any::<u8>(), 0..400)) {
        let out = {
            let mut out = Decoded::default();
            Decoder::new().feed(&encode(&screen), &mut out);
            out
        };
        prop_assert!(out.reply.is_empty());
        prop_assert!(out.window.is_none() && out.terminal.is_none());
        let plain: Vec<u8> = screen.iter().copied().filter(|b| *b != NUL && *b != b'\n').collect();
        let mut again = Decoded::default();
        Decoder::new().feed(&encode(&plain), &mut again);
        prop_assert_eq!(again.data, plain);
    }

    /// Every reported size lands inside the window the screens can draw.
    #[test]
    fn every_window_size_is_clamped_into_range(cols in any::<u16>(), rows in any::<u16>()) {
        let size = clamp_window(cols, rows);
        prop_assert!((40..=500).contains(&size.cols()), "cols {}", size.cols());
        prop_assert!((10..=200).contains(&size.rows()), "rows {}", size.rows());
    }
}

/// The long run: many more streams, longer, for a one-off check. Not part of `cargo test`.
#[test]
#[ignore = "long fuzz run; use --ignored"]
fn long_run() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(2_000));
    let strategy = (prop::collection::vec(hostile_byte(), 0..4000), cuts());
    runner
        .run(&strategy, |(wire, cuts)| {
            let pieces = split(&wire, &cuts);
            let (decoder, out) = run_accumulating(&pieces);
            assert_bounded(&decoder, &out, wire.len());
            assert_only_frames(&out.reply);
            let (_, whole) = run_accumulating(&[&wire]);
            prop_assert_eq!(whole.data, out.data);
            prop_assert_eq!(whole.reply, out.reply);
            Ok(())
        })
        .expect("long fuzz run");
}

// ---- targeted cases ----

fn decode(bytes: &[u8]) -> (Decoder, Decoded) {
    run_accumulating(&[bytes])
}

#[test]
fn an_unterminated_subnegotiation_stays_bounded_and_ends_at_the_next_iac_se() {
    let mut wire = vec![IAC, SB, OPT_TTYPE, 0];
    wire.extend(std::iter::repeat_n(b'x', 1_000_000));
    let (mut decoder, mut out) = decode(&wire);
    assert!(decoder.sb.len() <= MAX_SUBNEGOTIATION);
    assert!(out.terminal.is_none() && out.window.is_none());
    assert!(out.data.is_empty(), "the swallowed bytes are not input");
    // Still inside the subnegotiation: ordinary text is swallowed, not typed.
    decoder.feed(b"hello", &mut out);
    assert!(out.data.is_empty());
    // The first IAC SE ends it (too long to count as an answer) and input flows again.
    decoder.feed(&[IAC, SE], &mut out);
    decoder.feed(b"ok", &mut out);
    assert_eq!(out.data, b"ok");
    assert!(out.terminal.is_none());
}

#[test]
fn iac_se_with_no_subnegotiation_before_it_is_dropped() {
    let (_, out) = decode(&[b'a', IAC, SE, b'b']);
    assert_eq!(out.data, b"ab");
    assert!(out.reply.is_empty() && out.window.is_none() && out.terminal.is_none());
    // After a completed subnegotiation a second SE is stray too.
    let (_, out) = decode(&[IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE, IAC, SE, b'z']);
    assert_eq!(out.window, Some((80, 24)));
    assert_eq!(out.data, b"z");
}

#[test]
fn naws_of_zero_by_zero_is_reported_raw_and_clamped_to_the_default() {
    let (_, out) = decode(&[IAC, SB, OPT_NAWS, 0, 0, 0, 0, IAC, SE]);
    assert_eq!(out.window, Some((0, 0)));
    let size = clamp_window(0, 0);
    assert_eq!((size.cols(), size.rows()), (80, 24));
}

#[test]
fn naws_of_65535_by_65535_survives_doubling_and_is_clamped_to_the_maximum() {
    // Every 0xFF is doubled on the wire.
    let wire = [
        IAC, SB, OPT_NAWS, IAC, IAC, IAC, IAC, IAC, IAC, IAC, IAC, IAC, SE,
    ];
    let (_, out) = decode(&wire);
    assert_eq!(out.window, Some((65535, 65535)));
    let size = clamp_window(65535, 65535);
    assert_eq!((size.cols(), size.rows()), (500, 200));
    // One-sided: a zero width with a real height keeps the height.
    let size = clamp_window(0, 50);
    assert_eq!((size.cols(), size.rows()), (80, 50));
}

#[test]
fn a_huge_or_hostile_terminal_type_is_ignored() {
    let reply = |name: &[u8]| {
        let mut wire = vec![IAC, SB, OPT_TTYPE, 0];
        wire.extend_from_slice(name);
        wire.extend_from_slice(&[IAC, SE]);
        decode(&wire).1.terminal
    };
    assert_eq!(reply(&vec![b'a'; 10_000_000 / 100]), None);
    assert_eq!(reply(b"xterm\x1b[2J"), None);
    assert_eq!(reply(b"xterm\r\nroot"), None);
    assert_eq!(reply(&[0x1b; 30]), None);
    assert_eq!(
        reply(&[IAC, IAC, b'x']),
        None,
        "an escaped 0xFF is not a name"
    );
    assert_eq!(reply("xtérm".as_bytes()), None);
    assert_eq!(reply(b"\x00"), None);
    assert_eq!(reply(b"vt100").as_deref(), Some("vt100"));
}

#[test]
fn a_will_and_wont_flood_gets_at_most_one_answer_per_option() {
    let mut wire = Vec::new();
    for round in 0..4_000_u32 {
        for option in 0..=255_u8 {
            let _ = round;
            wire.extend_from_slice(&[
                IAC, WILL, option, IAC, WONT, option, IAC, DO, option, IAC, DONT, option,
            ]);
        }
    }
    let (decoder, out) = decode(&wire);
    assert!(out.reply.len() <= MAX_REPLY, "{} bytes", out.reply.len());
    assert_only_frames(&out.reply);
    assert!(out.data.is_empty());
    assert!(decoder.sb.len() <= MAX_SUBNEGOTIATION);
    // A second flood on the same connection earns nothing at all.
    let mut again = Decoded::default();
    let mut decoder = decoder;
    decoder.feed(&wire[..100_000], &mut again);
    assert!(again.reply.is_empty(), "{:?}", again.reply);
}
