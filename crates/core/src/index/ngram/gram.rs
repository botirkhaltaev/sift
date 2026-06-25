/// Validated N-gram width.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
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

/// Packed runtime-width N-gram key.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Gram(u64);

impl Gram {
    #[must_use]
    pub fn from_window(bytes: &[u8]) -> Self {
        debug_assert!(!bytes.is_empty() && bytes.len() <= 8);
        let mut value = 0u64;
        for &byte in bytes {
            value = (value << 8) | u64::from(byte);
        }
        Self(value)
    }

    /// Decodes a gram key from its sortable ordinal representation.
    ///
    /// # Errors
    ///
    /// Returns an error when the ordinal is outside the representable range for
    /// the gram width.
    pub fn from_ordinal(width: GramWidth, value: u64) -> std::io::Result<Self> {
        if value > max_value(width) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gram value exceeds width",
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn ordinal(self) -> u64 {
        self.0
    }

    pub fn write_bytes(self, width: GramWidth, out: &mut Vec<u8>) {
        let bytes = self.0.to_be_bytes();
        out.extend_from_slice(&bytes[8 - width.get()..]);
    }

    /// Reads a gram key from its fixed-width byte representation.
    ///
    /// # Errors
    ///
    /// Returns an error when `bytes` does not match the expected gram width.
    pub fn read_bytes(width: GramWidth, bytes: &[u8]) -> std::io::Result<Self> {
        if bytes.len() != width.get() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "gram key has wrong byte width",
            ));
        }
        Ok(Self::from_window(bytes))
    }
}

/// Iterator over overlapping fixed-width gram windows.
pub struct GramWindows<'a> {
    bytes: &'a [u8],
    width: GramWidth,
    offset: usize,
}

impl<'a> GramWindows<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8], width: GramWidth) -> Self {
        Self {
            bytes,
            width,
            offset: 0,
        }
    }
}

impl Iterator for GramWindows<'_> {
    type Item = Gram;

    fn next(&mut self) -> Option<Self::Item> {
        let width = self.width.get();
        if self.offset + width > self.bytes.len() {
            return None;
        }
        let gram = Gram::from_window(&self.bytes[self.offset..self.offset + width]);
        self.offset += 1;
        Some(gram)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .bytes
            .len()
            .saturating_sub(self.offset + self.width.get().saturating_sub(1));
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for GramWindows<'_> {}

const fn max_value(width: GramWidth) -> u64 {
    match width.get() {
        8 => u64::MAX,
        n => (1u64 << (n * 8)) - 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gram_width_rejects_zero() {
        let result = std::panic::catch_unwind(|| GramWidth::new(0));
        assert!(result.is_err());
    }

    #[test]
    fn gram_windows_use_runtime_width() {
        let grams: Vec<_> = GramWindows::new(b"abcde", GramWidth::new(4)).collect();
        assert_eq!(grams.len(), 2);
        assert_eq!(grams[0].ordinal(), Gram::from_window(b"abcd").ordinal());
        assert_eq!(grams[1].ordinal(), Gram::from_window(b"bcde").ordinal());
    }

    #[test]
    fn gram_roundtrip() {
        let width = GramWidth::new(3);
        let gram = Gram::from_window(b"abc");
        let mut bytes = Vec::new();
        gram.write_bytes(width, &mut bytes);
        assert_eq!(bytes, b"abc");
        assert_eq!(Gram::read_bytes(width, &bytes).unwrap(), gram);
        assert_eq!(Gram::from_ordinal(width, gram.ordinal()).unwrap(), gram);
    }
}
