//! H-2: what a door can be sent. Whatever bytes the client sends, a door sees only printable
//! ASCII characters and the named keys; never a control character, an escape, a paste break
//! or a logoff key (those are the framework's).

use std::time::Instant;

use proptest::prelude::*;
use russh_bbs_door::DoorKey;

use super::door_key;
use crate::ssh::terminal::{Key, KeyParser};

#[test]
fn control_keys_and_unnamed_keys_never_reach_a_door() {
    for letter in 'a'..='z' {
        assert_eq!(door_key(Key::Ctrl(letter)), None, "Ctrl-{letter}");
    }
    for key in [
        Key::Home,
        Key::End,
        Key::Delete,
        Key::PageUp,
        Key::PageDown,
        Key::PasteBreak,
        Key::Other,
    ] {
        assert_eq!(door_key(key), None, "{key:?}");
    }
}

#[test]
fn the_named_keys_map_one_to_one() {
    assert_eq!(door_key(Key::Enter), Some(DoorKey::Enter));
    assert_eq!(door_key(Key::Escape), Some(DoorKey::Escape));
    assert_eq!(door_key(Key::Backspace), Some(DoorKey::Backspace));
    assert_eq!(door_key(Key::Up), Some(DoorKey::Up));
    assert_eq!(door_key(Key::Char('?')), Some(DoorKey::Char('?')));
}

proptest! {
    #[test]
    fn a_door_only_ever_sees_printable_ascii_and_named_keys(
        bytes in proptest::collection::vec(any::<u8>(), 0..400),
        cut in 0_usize..400,
    ) {
        let mut parser = KeyParser::new();
        let now = Instant::now();
        let (head, tail) = bytes.split_at(cut.min(bytes.len()));
        let mut keys = parser.feed(head, now);
        keys.extend(parser.feed(tail, now));
        keys.extend(parser.expire());
        for key in keys {
            if let Some(DoorKey::Char(c)) = door_key(key) {
                prop_assert!((' '..='~').contains(&c), "{c:?}");
            }
        }
    }
}
