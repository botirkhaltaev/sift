use std::marker::PhantomData;

/// Validated N-gram width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GramWidth(u8);

impl GramWidth {
    pub const TRIGRAM: Self = Self(3);

    /// Creates a gram width in the range supported by packed gram storage.
    ///
    /// # Panics
    ///
    /// Panics when `width` is outside `1..=8`.
    #[must_use]
    pub const fn new(width: u8) -> Self {
        assert!(width >= 1 && width <= 8, "gram width must be 1..=8");
        Self(width)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0 as usize
    }

    #[must_use]
    pub fn as_u32(self) -> u32 {
        u32::from(self.0)
    }
}

/// A sortable packed N-gram key.
pub trait GramKey:
    Copy
    + Ord
    + Eq
    + std::fmt::Debug
    + Send
    + Sync
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
{
    const WIDTH: GramWidth;

    fn from_window(bytes: &[u8]) -> Self;

    /// Decodes a gram key from its sortable ordinal representation.
    ///
    /// # Errors
    ///
    /// Returns an error when the ordinal is outside the representable range for
    /// the gram width.
    fn from_ordinal(value: u64) -> std::io::Result<Self>;

    fn ordinal(self) -> u64;

    fn write_bytes(self, out: &mut Vec<u8>);

    /// Reads a gram key from its fixed-width byte representation.
    ///
    /// # Errors
    ///
    /// Returns an error when `bytes` does not match the gram width or encodes an
    /// invalid key for the implementation.
    fn read_bytes(bytes: &[u8]) -> std::io::Result<Self>;
}

/// Packed N-gram key for widths that fit in a `u64`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct PackedGram<const N: usize>(u64);

impl<const N: usize> PackedGram<N> {
    const fn max_value() -> Option<u64> {
        if N == 0 || N > 8 {
            None
        } else if N == 8 {
            Some(u64::MAX)
        } else {
            Some((1u64 << (N * 8)) - 1)
        }
    }
}

impl<const N: usize> GramKey for PackedGram<N> {
    const WIDTH: GramWidth = match N {
        1 => GramWidth::new(1),
        2 => GramWidth::new(2),
        3 => GramWidth::new(3),
        4 => GramWidth::new(4),
        5 => GramWidth::new(5),
        6 => GramWidth::new(6),
        7 => GramWidth::new(7),
        8 => GramWidth::new(8),
        _ => panic!("packed gram width must be 1..=8"),
    };

    fn from_window(bytes: &[u8]) -> Self {
        debug_assert_eq!(bytes.len(), N);
        let mut value = 0u64;
        for &byte in bytes {
            value = (value << 8) | u64::from(byte);
        }
        Self(value)
    }

    fn from_ordinal(value: u64) -> std::io::Result<Self> {
        let Some(max) = Self::max_value() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "packed gram width must be 1..=8",
            ));
        };
        if value > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "packed gram value exceeds width",
            ));
        }
        Ok(Self(value))
    }

    fn ordinal(self) -> u64 {
        self.0
    }

    fn write_bytes(self, out: &mut Vec<u8>) {
        let bytes = self.0.to_be_bytes();
        out.extend_from_slice(&bytes[8 - N..]);
    }

    fn read_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        if bytes.len() != N {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gram key has wrong byte width",
            ));
        }
        Ok(Self::from_window(bytes))
    }
}

/// A trigram encoded as a compact `u32`-sized value in big-endian order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Trigram(u32);

impl Trigram {
    #[inline]
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 3]) -> Self {
        Self(u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]))
    }

    #[inline]
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 3] {
        let b = self.0.to_be_bytes();
        [b[1], b[2], b[3]]
    }

    #[inline]
    #[must_use]
    pub(crate) const fn from_u24(key: u32) -> Self {
        debug_assert!(key <= 0x00FF_FFFF);
        Self(key)
    }

    #[inline]
    #[must_use]
    pub const fn as_u24(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn windows(bytes: &[u8]) -> GramWindows<'_, Self> {
        GramWindows::new(bytes)
    }
}

impl GramKey for Trigram {
    const WIDTH: GramWidth = GramWidth::TRIGRAM;

    fn from_window(bytes: &[u8]) -> Self {
        debug_assert_eq!(bytes.len(), 3);
        Self::from_bytes([bytes[0], bytes[1], bytes[2]])
    }

    fn from_ordinal(value: u64) -> std::io::Result<Self> {
        let value = u32::try_from(value).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "trigram value exceeds u32")
        })?;
        if value > 0x00FF_FFFF {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trigram value exceeds 24-bit range",
            ));
        }
        Ok(Self::from_u24(value))
    }

    fn ordinal(self) -> u64 {
        u64::from(self.as_u24())
    }

    fn write_bytes(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_bytes());
    }

    fn read_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let bytes: [u8; 3] = bytes.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "trigram key must be 3 bytes",
            )
        })?;
        Ok(Self::from_bytes(bytes))
    }
}

/// Iterator over overlapping fixed-width gram windows.
pub struct GramWindows<'a, G: GramKey> {
    bytes: &'a [u8],
    offset: usize,
    gram_type: PhantomData<fn() -> G>,
}

impl<'a, G: GramKey> GramWindows<'a, G> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            gram_type: PhantomData,
        }
    }
}

impl<G: GramKey> Iterator for GramWindows<'_, G> {
    type Item = G;

    fn next(&mut self) -> Option<Self::Item> {
        let width = G::WIDTH.get();
        if self.offset + width > self.bytes.len() {
            return None;
        }
        let gram = G::from_window(&self.bytes[self.offset..self.offset + width]);
        self.offset += 1;
        Some(gram)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .bytes
            .len()
            .saturating_sub(self.offset + G::WIDTH.get().saturating_sub(1));
        (remaining, Some(remaining))
    }
}

impl<G: GramKey> std::iter::FusedIterator for GramWindows<'_, G> {}

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
    fn windows_returns_overlapping_trigrams() {
        let tris: Vec<[u8; 3]> = Trigram::windows(b"abcd").map(Trigram::to_bytes).collect();
        assert_eq!(tris, vec![*b"abc", *b"bcd"]);
    }

    #[test]
    fn packed_gram_windows_support_other_widths() {
        assert_eq!(
            GramWindows::<PackedGram<4>>::new(b"abcde")
                .map(PackedGram::ordinal)
                .count(),
            2
        );
    }

    #[test]
    #[should_panic(expected = "gram width must be 1..=8")]
    fn gram_width_rejects_zero() {
        let _ = GramWidth::new(0);
    }
}
