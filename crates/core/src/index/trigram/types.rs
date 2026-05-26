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

    /// Extract unique trigrams from raw `bytes`.
    ///
    /// Returns a sorted, deduplicated `Vec<Trigram>`.
    #[must_use]
    pub fn unique_from_bytes(bytes: &[u8]) -> Vec<Self> {
        if bytes.len() < 3 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(bytes.len() - 2);
        for i in 0..=bytes.len() - 3 {
            out.push(Self::from_bytes([bytes[i], bytes[i + 1], bytes[i + 2]]));
        }
        out.sort_unstable();
        out.dedup();
        out
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

    #[test]
    fn unique_from_bytes_sorts_and_deduplicates() {
        let tris = Trigram::unique_from_bytes(b"ababa");
        assert_eq!(tris.len(), 2);
        assert!(tris.contains(&Trigram::from_bytes(*b"aba")));
        assert!(tris.contains(&Trigram::from_bytes(*b"bab")));
    }

    #[test]
    fn unique_from_bytes_short_returns_empty() {
        assert!(Trigram::unique_from_bytes(b"").is_empty());
        assert!(Trigram::unique_from_bytes(b"ab").is_empty());
    }

    #[test]
    fn unique_from_bytes_matches_raw_windows_valid_ascii() {
        let b = b"hello world";
        let unique: Vec<[u8; 3]> = Trigram::unique_from_bytes(b)
            .into_iter()
            .map(Trigram::to_bytes)
            .collect();
        let mut ref_set: Vec<[u8; 3]> = Trigram::windows(b).map(Trigram::to_bytes).collect();
        ref_set.sort_unstable();
        ref_set.dedup();
        assert_eq!(unique, ref_set);
    }

    #[test]
    fn unique_from_bytes_matches_raw_windows_multibyte() {
        let b = "café résumé 日本語".as_bytes();
        let unique: Vec<[u8; 3]> = Trigram::unique_from_bytes(b)
            .into_iter()
            .map(Trigram::to_bytes)
            .collect();
        let mut ref_set: Vec<[u8; 3]> = Trigram::windows(b).map(Trigram::to_bytes).collect();
        ref_set.sort_unstable();
        ref_set.dedup();
        assert_eq!(unique, ref_set);
    }

    #[test]
    fn unique_from_bytes_uses_raw_windows_for_invalid_utf8() {
        let b: Vec<u8> = [b"ok", &[0xff, 0xfe][..], b" trail"].concat();
        let unique: Vec<[u8; 3]> = Trigram::unique_from_bytes(&b)
            .into_iter()
            .map(Trigram::to_bytes)
            .collect();
        let mut ref_set: Vec<[u8; 3]> = Trigram::windows(&b).map(Trigram::to_bytes).collect();
        ref_set.sort_unstable();
        ref_set.dedup();
        assert_eq!(unique, ref_set);
    }

    #[test]
    fn unique_from_bytes_does_not_allocate_lossy_replacement_trigrams() {
        let b = &[0xff, 0xfe, 0xfd];
        let unique = Trigram::unique_from_bytes(b);
        assert_eq!(unique.len(), 1);
        assert_eq!(unique[0].to_bytes(), *b);
    }
}
