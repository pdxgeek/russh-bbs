//! A tiny binary writer and reader for saves and the shared world.
//!
//! The framework allows `serde` and `toml` only, and `toml` here only parses, so the formats
//! are written by hand: fixed-width little-endian integers, length-prefixed text and lists. A
//! [`Reader`] never panics and never allocates more than the bytes it was given can hold, so a
//! corrupt blob decodes to `None` rather than an error path a game could mishandle.

/// Builds a byte blob.
#[derive(Debug, Default)]
pub struct Writer {
    out: Vec<u8>,
}

impl Writer {
    /// An empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One byte.
    pub fn u8(&mut self, value: u8) {
        self.out.push(value);
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self, value: u16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self, value: u32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// A little-endian `i32`.
    pub fn i32(&mut self, value: i32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// A little-endian `i64`.
    pub fn i64(&mut self, value: i64) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    /// A list length (at most 65 535; longer lists are cut by the caller's own caps).
    pub fn len(&mut self, len: usize) {
        self.u16(u16::try_from(len).unwrap_or(u16::MAX));
    }

    /// Text, as a one-byte length (up to 255 bytes) and the bytes.
    pub fn text(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(255);
        self.u8(u8::try_from(len).unwrap_or(255));
        self.out
            .extend_from_slice(bytes.get(..len).unwrap_or_default());
    }

    /// The finished blob.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// Reads a byte blob written by [`Writer`].
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Start at the beginning of `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    /// Bytes not yet read.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// One byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    /// A little-endian `i32`.
    pub fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    /// A little-endian `i64`.
    pub fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// A list length, refused if more than `max` or if the bytes left could not hold that many
    /// items of at least `min_item` bytes each (so a corrupt length cannot ask for a huge list).
    pub fn len(&mut self, max: usize, min_item: usize) -> Option<usize> {
        let len = usize::from(self.u16()?);
        (len <= max && len.checked_mul(min_item)? <= self.remaining()).then_some(len)
    }

    /// Text written by [`Writer::text`].
    pub fn text(&mut self) -> Option<String> {
        let len = usize::from(self.u8()?);
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    /// True when every byte was read.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.pos == self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_short_input_is_none() {
        let mut w = Writer::new();
        w.u8(7);
        w.u16(513);
        w.u32(70_000);
        w.i32(-5);
        w.i64(-1 << 40);
        w.text("Wren ✦");
        w.len(3);
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8(), Some(7));
        assert_eq!(r.u16(), Some(513));
        assert_eq!(r.u32(), Some(70_000));
        assert_eq!(r.i32(), Some(-5));
        assert_eq!(r.i64(), Some(-1 << 40));
        assert_eq!(r.text().as_deref(), Some("Wren ✦"));
        assert_eq!(r.len(10, 0), Some(3));
        assert!(r.done());
        assert_eq!(r.u8(), None);
        assert_eq!(Reader::new(&bytes[..3]).u32(), None);
    }

    #[test]
    fn a_huge_length_is_refused_before_anything_is_allocated() {
        let bytes = [0xFF, 0xFF, 1, 2, 3];
        assert_eq!(Reader::new(&bytes).len(usize::MAX, 1), None);
        assert_eq!(Reader::new(&[5, 0, 1]).len(10, 1), None);
        assert_eq!(Reader::new(&[2, 0, 1, 1]).len(10, 1), Some(2));
    }
}
