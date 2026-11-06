//! Hardening property tests for the key parser (H-2): arbitrary bytes cut at arbitrary places.
//!
//! The promises, each a test here: no panic and bounded state; the same keys however the bytes
//! are split; nothing from an invalid or oversized sequence becomes a key; a bracketed paste is
//! only text; a forged cursor-position reply is an acknowledgement and never a key; the parser is
//! never left wedged once its timeout has run.

use std::time::Instant;

use proptest::prelude::*;

use super::{Key, KeyParser, MAX_MOUSE_QUEUE, PASTE_END, PASTE_START};
use crate::ui::input::mouse::MAX_COORD;

/// Everything one run of a parser produced, so two runs can be compared whole.
#[derive(Debug, PartialEq)]
struct Seen {
    keys: Vec<Key>,
    mouse: usize,
    acks: u32,
    dropped: usize,
}

/// Feed `bytes` in pieces of the given lengths (the rest in a last piece), then let the parser
/// time out, as a session does when input stops.
fn run(bytes: &[u8], cuts: &[usize]) -> Seen {
    let mut parser = KeyParser::new();
    let now = Instant::now();
    let mut keys = Vec::new();
    let mut rest = bytes;
    for &cut in cuts {
        let at = cut.min(rest.len());
        let (head, tail) = rest.split_at(at);
        keys.extend(parser.feed(head, now));
        rest = tail;
    }
    keys.extend(parser.feed(rest, now));
    keys.extend(parser.expire());
    Seen {
        keys,
        mouse: parser.take_mouse().len(),
        acks: parser.take_acks(),
        dropped: parser.take_dropped(),
    }
}

fn soup() -> impl Strategy<Value = Vec<u8>> {
    // Weighted towards the bytes that matter: escapes, brackets, digits, separators, the
    // finals of reports and paste markers, controls, and UTF-8 lead and continuation bytes.
    proptest::collection::vec(
        prop_oneof![
            4 => Just(0x1b_u8),
            3 => Just(b'['),
            2 => Just(b'O'),
            2 => Just(b'<'),
            4 => b'0'..=b'9',
            2 => Just(b';'),
            2 => prop_oneof![Just(b'M'), Just(b'm'), Just(b'R'), Just(b'~')],
            2 => prop_oneof![Just(0x03_u8), Just(0x04), Just(b'\r'), Just(b'\n'), Just(b'q')],
            3 => any::<u8>(),
        ],
        0..300,
    )
}

fn cuts() -> impl Strategy<Value = Vec<usize>> {
    proptest::collection::vec(0_usize..40, 0..12)
}

proptest! {
    /// No panic, and every buffer stays inside its bound, at every packet boundary.
    #[test]
    fn arbitrary_bytes_cannot_panic_or_grow_state(bytes in soup(), cuts in cuts()) {
        let mut parser = KeyParser::new();
        let now = Instant::now();
        let mut rest = &bytes[..];
        let mut produced = 0_usize;
        for cut in cuts.iter().copied().chain([usize::MAX]) {
            let (head, tail) = rest.split_at(cut.min(rest.len()));
            produced += parser.feed(head, now).len();
            rest = tail;
            prop_assert!(parser.held.len() <= 32, "held {}", parser.held.len());
            prop_assert!(parser.mouse.len() <= MAX_MOUSE_QUEUE);
        }
        produced += parser.expire().len();
        // Each byte makes at most one key; a held run settles into a handful at most.
        prop_assert!(produced <= bytes.len() + 1, "{produced} keys from {} bytes", bytes.len());
        prop_assert!(parser.held.is_empty());
    }

    /// The same bytes mean the same keys, mouse reports, acknowledgements and drop count
    /// however they are cut into packets.
    #[test]
    fn the_split_never_changes_the_meaning(bytes in soup(), cuts in cuts()) {
        prop_assert_eq!(run(&bytes, &cuts), run(&bytes, &[]));
    }

    /// After the timeout has run the parser is not stuck in any state: what is typed next is
    /// read as typed, whatever came before.
    #[test]
    fn the_timeout_always_leaves_a_working_parser(bytes in soup()) {
        let mut parser = KeyParser::new();
        let now = Instant::now();
        parser.feed(&bytes, now);
        parser.expire();
        let keys = parser.feed(b"ab\x03", now);
        prop_assert_eq!(keys, vec![Key::Char('a'), Key::Char('b'), Key::Ctrl('c')]);
    }

    /// Every mouse report that is accepted is inside the coordinate bound.
    #[test]
    fn mouse_reports_stay_inside_their_bounds(bytes in soup()) {
        let mut parser = KeyParser::new();
        parser.feed(&bytes, Instant::now());
        for event in parser.take_mouse() {
            prop_assert!(u32::from(event.col) < MAX_COORD);
            prop_assert!(u32::from(event.row) < MAX_COORD);
        }
    }

    /// A bracketed paste is text: whatever the payload (it cannot hold its own end marker),
    /// only printable characters and line breaks come out, never Enter, a control key, Escape,
    /// an arrow or anything that logs off. The keys after the end marker are read as usual.
    #[test]
    fn a_paste_is_only_text(
        payload in proptest::collection::vec(any::<u8>(), 0..400),
        cuts in cuts(),
    ) {
        prop_assume!(!payload.windows(PASTE_END.len()).any(|w| w == PASTE_END));
        let mut bytes = PASTE_START.to_vec();
        bytes.extend(&payload);
        bytes.extend(PASTE_END);
        bytes.extend(b"\x03");
        let seen = run(&bytes, &cuts);
        let (last, pasted) = seen.keys.split_last().expect("the Ctrl-C after the paste");
        prop_assert_eq!(*last, Key::Ctrl('c'));
        for key in pasted {
            prop_assert!(
                matches!(key, Key::Char(' '..='~') | Key::PasteBreak),
                "{key:?} came out of a paste"
            );
        }
        prop_assert_eq!(seen.mouse, 0);
        prop_assert_eq!(seen.acks, 0);
    }

    /// A forged cursor-position reply (any row from 2, any column) is an acknowledgement
    /// and no key: put between the keys of a text it changes none of them. Cut anywhere.
    #[test]
    fn a_forged_cursor_reply_changes_no_key(
        row in 2_u32..99_999,
        col in 0_u32..99_999,
        at in 0_usize..8,
        cuts in cuts(),
    ) {
        let text = "ab q\x03\x1b[A\x1b[B\r";
        let clean = run(text.as_bytes(), &[]);
        // Cut points that fall between whole keys.
        let boundaries = [0, 1, 2, 3, 4, 5, 8, 11, 12];
        let at = boundaries[at % boundaries.len()];
        let mut bytes = text.as_bytes()[..at].to_vec();
        bytes.extend(format!("\x1b[{row};{col}R").bytes());
        bytes.extend(&text.as_bytes()[at..]);
        let forged = run(&bytes, &cuts);
        prop_assert_eq!(&forged.keys, &clean.keys);
        prop_assert_eq!(forged.acks, 1);
    }

    /// Acknowledgements are counted from whole replies only: never more than the `R`
    /// bytes there were.
    #[test]
    fn replies_are_never_invented(bytes in soup()) {
        let seen = run(&bytes, &[]);
        let finals = bytes.iter().filter(|b| **b == b'R').count();
        prop_assert!(seen.acks as usize <= finals);
    }
}
