#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

use proptest::prelude::*;

use super::{Button, Class, Kind, MAX_COORD, MAX_REPORT, Mouse, classify};

fn sgr(code: u32, x: u32, y: u32, last: char) -> Vec<u8> {
    format!("\x1b[<{code};{x};{y}{last}").into_bytes()
}

fn done(bytes: &[u8]) -> Option<Mouse> {
    match classify(bytes) {
        Class::Done(event) => event,
        other => panic!("{bytes:?} is {other:?}"),
    }
}

#[test]
fn an_sgr_press_release_and_wheel_decode_to_zero_based_cells() {
    assert_eq!(
        done(&sgr(0, 5, 3, 'M')),
        Some(Mouse {
            kind: Kind::Down(Button::Left),
            col: 4,
            row: 2
        })
    );
    assert_eq!(
        done(&sgr(0, 5, 3, 'm')).map(|m| m.kind),
        Some(Kind::Up(Button::Left))
    );
    assert_eq!(
        done(&sgr(2, 1, 1, 'M')).map(|m| m.kind),
        Some(Kind::Down(Button::Right))
    );
    assert_eq!(
        done(&sgr(64, 9, 9, 'M')).map(|m| m.kind),
        Some(Kind::ScrollUp)
    );
    assert_eq!(
        done(&sgr(65, 9, 9, 'M')).map(|m| m.kind),
        Some(Kind::ScrollDown)
    );
    // Motion with a button held is a drag.
    assert_eq!(
        done(&sgr(32, 9, 9, 'M')).map(|m| m.kind),
        Some(Kind::Drag(Button::Left))
    );
}

#[test]
fn the_legacy_x10_encoding_is_read_and_a_release_decodes_to_nothing() {
    // Button 0 (left) at column 10, row 4: bytes offset by 32, coordinates one-based.
    let press = [0x1b, b'[', b'M', 32, 32 + 10, 32 + 4];
    assert_eq!(
        done(&press),
        Some(Mouse {
            kind: Kind::Down(Button::Left),
            col: 9,
            row: 3
        })
    );
    let wheel = [0x1b, b'[', b'M', 32 + 64, 33, 33];
    assert_eq!(done(&wheel).map(|m| m.kind), Some(Kind::ScrollUp));
    // Code 3 is "released": the legacy encoding does not say which button.
    let release = [0x1b, b'[', b'M', 32 + 3, 40, 40];
    assert_eq!(done(&release), None);
}

#[test]
fn a_partial_report_is_a_prefix_and_anything_else_is_not_mouse() {
    for part in [
        &b"\x1b[<"[..],
        b"\x1b[<0",
        b"\x1b[<0;1",
        b"\x1b[<0;1;",
        b"\x1b[M",
        b"\x1b[M ",
        b"\x1b[M  ",
    ] {
        assert_eq!(classify(part), Class::Prefix, "{part:?}");
    }
    for other in [&b"\x1b[A"[..], b"\x1b[200~", b"a", b"", b"\x1b", b"\x1bOA"] {
        assert_eq!(classify(other), Class::NotMouse, "{other:?}");
    }
}

#[test]
fn malformed_reports_are_invalid_not_decoded() {
    for bad in [
        &b"\x1b[<a;1;1M"[..],
        b"\x1b[<0;1M",
        b"\x1b[<0;1;1;1M",
        b"\x1b[<;;M",
        b"\x1b[<0;;1M",
        b"\x1b[<0;1;1x",
        b"\x1b[<-1;1;1M",
        b"\x1b[<123456;1;1M",
    ] {
        assert_eq!(classify(bad), Class::Invalid, "{bad:?}");
    }
}

#[test]
fn zero_oversized_and_chorded_reports_decode_to_nothing() {
    assert_eq!(done(&sgr(0, 0, 1, 'M')), None, "column zero");
    assert_eq!(done(&sgr(0, 1, 0, 'M')), None, "row zero");
    assert_eq!(done(&sgr(0, MAX_COORD + 1, 1, 'M')), None, "too far right");
    assert_eq!(done(&sgr(4, 1, 1, 'M')), None, "shift");
    assert_eq!(done(&sgr(8, 1, 1, 'M')), None, "meta");
    assert_eq!(done(&sgr(16, 1, 1, 'M')), None, "ctrl");
    assert_eq!(done(&sgr(66, 1, 1, 'M')), None, "sideways wheel");
    assert_eq!(done(&sgr(3, 1, 1, 'M')), None, "no button");
}

#[test]
fn a_report_longer_than_the_cap_is_invalid_even_while_it_still_looks_like_one() {
    let mut long = b"\x1b[<".to_vec();
    long.extend(std::iter::repeat_n(b'1', MAX_REPORT));
    assert_eq!(classify(&long), Class::Invalid);
    // The longest legitimate report fits.
    let longest = sgr(67, 4096, 4096, 'M');
    assert!(longest.len() <= MAX_REPORT);
    assert!(matches!(classify(&longest), Class::Done(_)));
}

proptest! {
    /// Arbitrary bytes never panic the classifier, whatever the prefix.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let _ = classify(&bytes);
        let mut with_prefix = b"\x1b[<".to_vec();
        with_prefix.extend(&bytes);
        let _ = classify(&with_prefix);
        let mut x10 = b"\x1b[M".to_vec();
        x10.extend(&bytes);
        let _ = classify(&x10);
    }

    /// A decoded event always lies inside the coordinate cap; nothing decodes from a run
    /// longer than the cap.
    #[test]
    fn decoded_events_stay_inside_the_caps(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let mut sgr_bytes = b"\x1b[<".to_vec();
        sgr_bytes.extend(&bytes);
        if let Class::Done(Some(event)) = classify(&sgr_bytes) {
            prop_assert!(sgr_bytes.len() <= MAX_REPORT);
            prop_assert!(u32::from(event.col) < MAX_COORD && u32::from(event.row) < MAX_COORD);
        }
        if sgr_bytes.len() > MAX_REPORT {
            prop_assert!(matches!(classify(&sgr_bytes), Class::Invalid));
        }
    }

    /// Every well-formed report decodes, and what decodes is what was sent.
    #[test]
    fn well_formed_reports_round_trip(code in 0_u32..3, x in 1_u32..=MAX_COORD, y in 1_u32..=MAX_COORD) {
        let event = done(&sgr(code, x, y, 'M')).unwrap();
        prop_assert_eq!(u32::from(event.col), x - 1);
        prop_assert_eq!(u32::from(event.row), y - 1);
    }
}
