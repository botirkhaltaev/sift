/// A trigram encoded as a compact `u32` in big-endian order.
///
/// Numeric ordering matches bytewise ordering of the original `[u8; 3]`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Trigram(u32);

impl Trigram {
    /// Encode three bytes into a `Trigram`.
    #[inline]
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 3]) -> Self {
        Self(u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]))
    }

    /// Decode back to `[u8; 3]`.
    #[inline]
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 3] {
        let b = self.0.to_be_bytes();
        [b[1], b[2], b[3]]
    }

    /// Iterate over all overlapping 3-byte windows of `bytes`.
    pub const fn windows(bytes: &[u8]) -> TrigramWindows<'_> {
        TrigramWindows { bytes, offset: 0 }
    }

    /// Construct from a pre-encoded 24-bit u32 key (high 8 bits must be zero).
    #[inline]
    #[must_use]
    pub(crate) const fn from_u24(key: u32) -> Self {
        Self(key)
    }

    /// 24-bit integer key used for dedup bitsets.
    #[inline]
    #[must_use]
    pub const fn as_u24(self) -> u32 {
        self.0
    }
}

/// Iterator over overlapping 3-byte trigram windows.
pub struct TrigramWindows<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Iterator for TrigramWindows<'_> {
    type Item = Trigram;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset + 3 > self.bytes.len() {
            return None;
        }
        let tri = Trigram::from_bytes([
            self.bytes[self.offset],
            self.bytes[self.offset + 1],
            self.bytes[self.offset + 2],
        ]);
        self.offset += 1;
        Some(tri)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.bytes.len().saturating_sub(self.offset + 2);
        (remaining, Some(remaining))
    }
}

impl std::iter::FusedIterator for TrigramWindows<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigram_roundtrips_bytes() {
        for tri in [*b"abc", *b"\x00\x00\x00", *b"\xff\xff\xff"] {
            let key = Trigram::from_bytes(tri);
            assert_eq!(key.to_bytes(), tri);
        }
    }

    #[test]
    fn trigram_ordering_matches_bytes() {
        let k1 = Trigram::from_bytes(*b"abc");
        let k2 = Trigram::from_bytes(*b"abd");
        let k3 = Trigram::from_bytes(*b"abc");
        assert!(k1 < k2);
        assert_eq!(k1, k3);
    }

    #[test]
    fn windows_returns_overlapping_trigrams() {
        let tris: Vec<[u8; 3]> = Trigram::windows(b"abcd").map(Trigram::to_bytes).collect();
        assert_eq!(tris, vec![*b"abc", *b"bcd"]);
    }

    #[test]
    fn windows_short_input_is_empty() {
        assert!(Trigram::windows(b"").next().is_none());
        assert!(Trigram::windows(b"ab").next().is_none());
    }

    #[test]
    fn windows_exactly_three_bytes_yields_one() {
        let tris: Vec<[u8; 3]> = Trigram::windows(b"abc").map(Trigram::to_bytes).collect();
        assert_eq!(tris, vec![*b"abc"]);
    }
}
