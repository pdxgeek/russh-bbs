//! A tiny binary codec for saves and the world: little-endian numbers, length-prefixed strings.
//! Hand-written so the format never changes under a dependency, and so a short or damaged blob is
//! `None`, never a panic.

/// Appends fields to a buffer.
#[derive(Default)]
pub(crate) struct Writer(pub(crate) Vec<u8>);

impl Writer {
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    /// A string of at most 255 bytes (longer is cut at a character boundary).
    pub(crate) fn str(&mut self, v: &str) {
        let mut end = v.len().min(255);
        while !v.is_char_boundary(end) {
            end -= 1;
        }
        self.u8(u8::try_from(end).unwrap_or(u8::MAX));
        self.0.extend_from_slice(&v.as_bytes()[..end]);
    }

    /// A count that fits a byte.
    pub(crate) fn count(&mut self, n: usize) {
        self.u8(u8::try_from(n).unwrap_or(u8::MAX));
    }
}

/// Reads fields from a buffer.
pub(crate) struct Reader<'a>(pub(crate) &'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        let [v] = self.array()?;
        Some(v)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.array()?))
    }

    pub(crate) fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.array()?))
    }

    pub(crate) fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    pub(crate) fn str(&mut self) -> Option<String> {
        let len = usize::from(self.u8()?);
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    pub(crate) fn count(&mut self) -> Option<usize> {
        self.u8().map(usize::from)
    }

    /// Whether every byte was read.
    pub(crate) fn done(&self) -> bool {
        self.0.is_empty()
    }
}
