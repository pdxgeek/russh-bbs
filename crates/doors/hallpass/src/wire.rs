//! A tiny binary codec for saves and the world.
//!
//! The framework gives a door bytes and a version, and the door's dependencies are limited to
//! `serde` and `toml` (the sandbox audit), neither of which can write a compact binary form here.
//! So the formats are written out by hand: little-endian integers, `u16` lengths, strings as UTF-8.
//! Every read is bounds-checked and returns `None` for bytes that are not a value, so a damaged
//! blob is refused rather than guessed at.

/// Appends values to a buffer.
#[derive(Debug, Default)]
pub struct Writer(Vec<u8>);

impl Writer {
    /// A new, empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes written.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.0
    }

    /// Append raw bytes.
    pub fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
}

/// Reads values from a slice.
#[derive(Debug)]
pub struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    /// A reader over `bytes`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    /// How many bytes are left.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.0.len()
    }

    /// Take `n` bytes.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
}

/// A value with a stable binary form.
pub trait Wire: Sized {
    /// Write `self`.
    fn put(&self, w: &mut Writer);
    /// Read a value, or `None` if the bytes are not one.
    fn get(r: &mut Reader<'_>) -> Option<Self>;
}

macro_rules! wire_int {
    ($($t:ty),*) => {$(
        impl Wire for $t {
            fn put(&self, w: &mut Writer) {
                w.raw(&self.to_le_bytes());
            }
            fn get(r: &mut Reader<'_>) -> Option<Self> {
                Some(<$t>::from_le_bytes(r.take(std::mem::size_of::<$t>())?.try_into().ok()?))
            }
        }
    )*};
}
wire_int!(u8, u16, u32, u64, i8, i16, i32, i64);

impl Wire for bool {
    fn put(&self, w: &mut Writer) {
        u8::from(*self).put(w);
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        match u8::get(r)? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

impl Wire for String {
    fn put(&self, w: &mut Writer) {
        let bytes = self.as_bytes();
        let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
        len.put(w);
        w.raw(bytes.get(..usize::from(len)).unwrap_or_default());
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        let len = usize::from(u16::get(r)?);
        std::str::from_utf8(r.take(len)?).ok().map(str::to_owned)
    }
}

impl<T: Wire> Wire for Vec<T> {
    fn put(&self, w: &mut Writer) {
        let len = u16::try_from(self.len()).unwrap_or(u16::MAX);
        len.put(w);
        for item in self.iter().take(usize::from(len)) {
            item.put(w);
        }
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        let len = usize::from(u16::get(r)?);
        // Every element takes at least one byte, so a length beyond what is left is damage.
        if len > r.remaining() {
            return None;
        }
        (0..len).map(|_| T::get(r)).collect()
    }
}

impl<T: Wire> Wire for Option<T> {
    fn put(&self, w: &mut Writer) {
        match self {
            None => 0u8.put(w),
            Some(value) => {
                1u8.put(w);
                value.put(w);
            }
        }
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        match u8::get(r)? {
            0 => Some(None),
            1 => Some(Some(T::get(r)?)),
            _ => None,
        }
    }
}

impl<A: Wire, B: Wire> Wire for (A, B) {
    fn put(&self, w: &mut Writer) {
        self.0.put(w);
        self.1.put(w);
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        let a = A::get(r)?;
        let b = B::get(r)?;
        Some((a, b))
    }
}

impl<const N: usize> Wire for [u64; N] {
    fn put(&self, w: &mut Writer) {
        for item in self {
            item.put(w);
        }
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        let mut out = [0u64; N];
        for slot in &mut out {
            *slot = u64::get(r)?;
        }
        Some(out)
    }
}

/// Implement [`Wire`] for a struct by listing its fields in their stored order.
macro_rules! wire_struct {
    ($t:ty { $($f:ident),* $(,)? }) => {
        impl $crate::wire::Wire for $t {
            fn put(&self, w: &mut $crate::wire::Writer) {
                $($crate::wire::Wire::put(&self.$f, w);)*
            }
            fn get(r: &mut $crate::wire::Reader<'_>) -> Option<Self> {
                Some(Self { $($f: $crate::wire::Wire::get(r)?),* })
            }
        }
    };
}
pub(crate) use wire_struct;

/// Encode one value.
#[must_use]
pub fn encode<T: Wire>(value: &T) -> Vec<u8> {
    let mut w = Writer::new();
    value.put(&mut w);
    w.finish()
}

/// Decode one value that fills `bytes` exactly.
#[must_use]
pub fn decode<T: Wire>(bytes: &[u8]) -> Option<T> {
    let mut r = Reader::new(bytes);
    let value = T::get(&mut r)?;
    (r.remaining() == 0).then_some(value)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn values_round_trip_and_damage_is_refused() {
        let value: (Vec<(String, i16)>, Option<u32>) =
            (vec![("ab".into(), -3), ("é".into(), 7)], Some(9));
        let bytes = encode(&value);
        assert_eq!(
            decode::<(Vec<(String, i16)>, Option<u32>)>(&bytes),
            Some(value)
        );
        assert_eq!(
            decode::<(Vec<(String, i16)>, Option<u32>)>(&bytes[..bytes.len() - 1]),
            None
        );
        let mut longer = bytes;
        longer.push(0);
        assert_eq!(decode::<(Vec<(String, i16)>, Option<u32>)>(&longer), None);
        // A huge claimed length is refused without allocating.
        assert_eq!(decode::<Vec<u8>>(&[0xff, 0xff, 1]), None);
        assert_eq!(decode::<bool>(&[2]), None);
        assert_eq!(decode::<String>(&[2, 0, 0xff, 0xfe]), None);
    }
}
