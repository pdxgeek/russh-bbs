//! A tiny binary codec for saves and the world: little-endian integers and short strings. Hand
//! written so the format is exactly what the tests pin, and so a door needs no serialization
//! dependency beyond what the sandbox audit allows.

/// Appends fields to a byte vector.
#[derive(Debug, Default)]
pub struct Writer(Vec<u8>);

impl Writer {
    /// An empty writer.
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// The bytes written.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.0
    }

    /// One byte.
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    /// A bool as one byte.
    pub fn bool(&mut self, v: bool) {
        self.0.push(u8::from(v));
    }

    /// A little-endian `u16`.
    pub fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    /// A little-endian `u32`.
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    /// A little-endian `u64`.
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    /// A little-endian `i64`.
    pub fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    /// Raw bytes with no length (the reader must know the size).
    pub fn raw(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }

    /// A count that fits a byte (longer lists are cut by the caller's caps; this saturates).
    pub fn count(&mut self, n: usize) {
        self.u8(u8::try_from(n).unwrap_or(u8::MAX));
    }

    /// A string: one length byte (at most 255 bytes, cut at a character boundary) then UTF-8.
    pub fn str(&mut self, v: &str) {
        let mut end = v.len().min(255);
        while !v.is_char_boundary(end) {
            end -= 1;
        }
        self.u8(u8::try_from(end).unwrap_or(u8::MAX));
        self.0.extend_from_slice(&v.as_bytes()[..end]);
    }
}

/// Reads fields back; every read is `None` past the end or on bad data.
#[derive(Debug)]
pub struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    /// A reader over `bytes`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    /// Whether every byte was consumed.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.0.is_empty()
    }

    /// The next `n` bytes.
    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    /// One byte.
    pub fn u8(&mut self) -> Option<u8> {
        let [b] = self.array()?;
        Some(b)
    }

    /// A bool; any byte but 0 and 1 is bad data.
    pub fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// A `u16`.
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.array()?))
    }

    /// A `u32`.
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }

    /// A `u64`.
    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.array()?))
    }

    /// An `i64`.
    pub fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.array()?))
    }

    /// A string written by [`Writer::str`].
    pub fn str(&mut self) -> Option<String> {
        let n = usize::from(self.u8()?);
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn fields_round_trip_and_short_reads_fail() {
        let mut w = Writer::new();
        w.u8(7);
        w.bool(true);
        w.u16(513);
        w.u32(70_000);
        w.u64(1 << 40);
        w.i64(-5);
        w.str("Brindle ü");
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8(), Some(7));
        assert_eq!(r.bool(), Some(true));
        assert_eq!(r.u16(), Some(513));
        assert_eq!(r.u32(), Some(70_000));
        assert_eq!(r.u64(), Some(1 << 40));
        assert_eq!(r.i64(), Some(-5));
        assert_eq!(r.str().as_deref(), Some("Brindle ü"));
        assert!(r.done());
        assert_eq!(r.u8(), None);
        assert_eq!(Reader::new(&bytes[..3]).u32(), None);
        assert_eq!(Reader::new(&[2]).bool(), None);
    }

    #[test]
    fn a_long_string_is_cut_on_a_character_boundary() {
        let mut w = Writer::new();
        w.str(&"é".repeat(200));
        let bytes = w.finish();
        let s = Reader::new(&bytes).str().unwrap();
        assert_eq!(s.chars().count(), 127);
    }
}
