use super::{Key, MAX_COLS, MAX_ROWS, PtySize, parse_keys};

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
    // ESC [ A is "up"; a naive scan would see 'A', and ESC O q must not read as 'q'.
    assert_eq!(parse_keys(b"\x1b[A"), vec![Key::Other]);
    assert_eq!(parse_keys(b"\x1bOq"), vec![Key::Other]);
    assert_eq!(parse_keys(b"\x1b[1;5q"), vec![Key::Other]);
}

#[test]
fn a_truncated_escape_sequence_is_dropped_without_panicking() {
    assert_eq!(parse_keys(b"\x1b"), vec![Key::Other]);
    assert_eq!(parse_keys(b"\x1b[12"), vec![Key::Other]);
}

#[test]
fn keys_after_an_escape_sequence_are_still_seen() {
    assert_eq!(parse_keys(b"\x1b[Aq"), vec![Key::Other, Key::Char('q')]);
}

#[test]
fn arbitrary_bytes_never_panic() {
    for first in 0..=255_u8 {
        let _ = parse_keys(&[first, 0x1b, first, b'[', first]);
    }
}
