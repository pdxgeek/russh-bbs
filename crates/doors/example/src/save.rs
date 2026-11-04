//! The save format, version 2 (little-endian):
//!
//! | Bytes | Field |
//! |---|---|
//! | 4 | coins |
//! | 1 | tosses left |
//! | 8 | day |
//! | 2 | streak |
//! | 2 | best streak |
//! | 1 | name length, then that many UTF-8 bytes |
//!
//! Version 1 had the first three fields only; [`migrate_v1`] appends the rest.

use crate::engine::{NAME_MAX, State};

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
}

/// Encode a game.
#[must_use]
pub fn encode(state: &State) -> Vec<u8> {
    let name = state.name.as_bytes();
    let name = name.get(..name.len().min(255)).unwrap_or_default();
    let mut out = Vec::with_capacity(18 + name.len());
    out.extend_from_slice(&state.coins.to_le_bytes());
    out.push(state.left);
    out.extend_from_slice(&state.day.to_le_bytes());
    out.extend_from_slice(&state.streak.to_le_bytes());
    out.extend_from_slice(&state.best.to_le_bytes());
    out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(name);
    out
}

/// Decode a version-2 save, or `None` if the bytes are not one (the door then starts fresh
/// rather than guess).
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<State> {
    let mut r = Reader(bytes);
    let coins = u32::from_le_bytes(r.array()?);
    let [left] = r.array()?;
    let day = i64::from_le_bytes(r.array()?);
    let streak = u16::from_le_bytes(r.array()?);
    let best = u16::from_le_bytes(r.array()?);
    let [len] = r.array()?;
    let name = std::str::from_utf8(r.take(usize::from(len))?).ok()?;
    if !r.0.is_empty() || name.chars().count() > NAME_MAX {
        return None;
    }
    Some(State {
        coins,
        left,
        day,
        streak,
        best,
        name: name.to_owned(),
    })
}

/// Bring a version-1 save to version 2: no streaks yet, no nickname.
#[must_use]
pub fn migrate_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() != 13 {
        return None;
    }
    let mut out = bytes.to_vec();
    out.extend_from_slice(&[0, 0, 0, 0, 0]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> State {
        State {
            coins: 12,
            left: 3,
            day: 20_123,
            streak: 2,
            best: 5,
            name: "Brindle".into(),
        }
    }

    #[test]
    fn a_save_round_trips() {
        assert_eq!(decode(&encode(&sample())), Some(sample()));
    }

    #[test]
    fn the_format_is_pinned() {
        let bytes = encode(&sample());
        assert_eq!(
            bytes,
            [12, 0, 0, 0, 3, 0x9b, 0x4e, 0, 0, 0, 0, 0, 0, 2, 0, 5, 0, 7, b'B', b'r', b'i', b'n', b'd', b'l', b'e']
        );
    }

    #[test]
    fn garbage_is_not_a_save() {
        let good = encode(&sample());
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&good[..good.len() - 1]), None);
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(decode(&trailing), None);
        let mut bad_utf8 = good;
        let last = bad_utf8.len() - 1;
        bad_utf8[last] = 0xff;
        assert_eq!(decode(&bad_utf8), None);
    }

    #[test]
    fn version_one_migrates_to_a_valid_version_two() {
        let mut v1 = Vec::new();
        v1.extend_from_slice(&9u32.to_le_bytes());
        v1.push(4);
        v1.extend_from_slice(&20_000i64.to_le_bytes());
        let v2 = migrate_v1(&v1).unwrap();
        let state = decode(&v2).unwrap();
        assert_eq!((state.coins, state.left, state.day), (9, 4, 20_000));
        assert_eq!((state.streak, state.best, state.name.as_str()), (0, 0, ""));
        assert_eq!(migrate_v1(&v1[..5]), None);
    }
}
