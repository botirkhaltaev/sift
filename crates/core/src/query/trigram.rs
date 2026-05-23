use std::collections::HashSet;

#[must_use]
#[cfg(test)]
fn extract_trigrams(text: &str) -> Vec<[u8; 3]> {
    extract_trigrams_from_bytes(text.as_bytes())
}

#[must_use]
pub fn extract_trigrams_from_bytes(b: &[u8]) -> Vec<[u8; 3]> {
    if b.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(b.len() - 2);
    for i in 0..=b.len() - 3 {
        out.push([b[i], b[i + 1], b[i + 2]]);
    }
    out
}

#[must_use]
pub fn extract_unique_trigrams_from_bytes(b: &[u8]) -> HashSet<[u8; 3]> {
    let mut out = HashSet::new();
    if b.len() < 3 {
        return out;
    }
    out.reserve(b.len().min(1 << 16));
    for i in 0..=b.len() - 3 {
        out.insert([b[i], b[i + 1], b[i + 2]]);
    }
    out
}

#[must_use]
#[cfg(test)]
pub fn extract_trigrams_utf8_lossy(bytes: &[u8]) -> Vec<[u8; 3]> {
    std::str::from_utf8(bytes).map_or_else(
        |_| extract_trigrams(String::from_utf8_lossy(bytes).as_ref()),
        extract_trigrams,
    )
}

#[must_use]
pub fn extract_unique_trigrams_utf8_lossy(bytes: &[u8]) -> HashSet<[u8; 3]> {
    std::str::from_utf8(bytes).map_or_else(
        |_| extract_unique_trigrams_from_bytes(String::from_utf8_lossy(bytes).as_ref().as_bytes()),
        |text| extract_unique_trigrams_from_bytes(text.as_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_lossy(bytes: &[u8]) -> Vec<[u8; 3]> {
        extract_trigrams(String::from_utf8_lossy(bytes).as_ref())
    }

    #[test]
    fn utf8_lossy_matches_reference_valid_ascii() {
        let b = b"hello world";
        assert_eq!(extract_trigrams_utf8_lossy(b), reference_lossy(b));
    }

    #[test]
    fn utf8_lossy_matches_reference_multibyte() {
        let b = "café résumé 日本語".as_bytes();
        assert_eq!(extract_trigrams_utf8_lossy(b), reference_lossy(b));
    }

    #[test]
    fn utf8_lossy_matches_reference_invalid() {
        for b in [
            &[0xff, 0xfe, 0xfd][..],
            b"ok\xff\xfe trail",
            &[0x80][..],
            b"a\xe0\x80\x80b",
        ] {
            assert_eq!(
                extract_trigrams_utf8_lossy(b),
                reference_lossy(b),
                "bytes={b:?}"
            );
        }
    }

    #[test]
    fn utf8_lossy_matches_reference_mixed() {
        let b: Vec<u8> = (0_u8..=255)
            .cycle()
            .take(512)
            .chain(std::iter::once(0xff))
            .collect();
        assert_eq!(extract_trigrams_utf8_lossy(&b), reference_lossy(&b));
    }

    #[test]
    fn short_string_empty() {
        assert!(extract_trigrams("").is_empty());
        assert!(extract_trigrams("ab").is_empty());
    }

    #[test]
    fn ascii_three_chars_one_trigram() {
        assert_eq!(extract_trigrams("abc"), vec![*b"abc"]);
    }

    #[test]
    fn overlapping_windows() {
        assert_eq!(extract_trigrams("abcd"), vec![*b"abc", *b"bcd"]);
    }

    #[test]
    fn extract_trigrams_from_bytes_exactly_three() {
        assert_eq!(extract_trigrams_from_bytes(b"abc"), vec![*b"abc"]);
    }

    #[test]
    fn extract_trigrams_from_bytes_overlapping() {
        assert_eq!(extract_trigrams_from_bytes(b"abcd"), vec![*b"abc", *b"bcd"]);
    }

    #[test]
    fn extract_trigrams_from_bytes_short_returns_empty() {
        assert!(extract_trigrams_from_bytes(b"").is_empty());
        assert!(extract_trigrams_from_bytes(b"ab").is_empty());
    }

    #[test]
    fn extract_unique_trigrams_deduplicates() {
        let tris = extract_unique_trigrams_from_bytes(b"ababa");
        assert_eq!(tris.len(), 2);
        assert!(tris.contains(b"aba"));
        assert!(tris.contains(b"bab"));
    }

    #[test]
    fn extract_unique_trigrams_from_bytes_short_returns_empty() {
        assert!(extract_unique_trigrams_from_bytes(b"").is_empty());
        assert!(extract_unique_trigrams_from_bytes(b"ab").is_empty());
    }

    #[test]
    fn extract_unique_trigrams_utf8_lossy_matches_reference_invalid() {
        let b = &[0xff, 0xfe, 0xfd][..];
        let unique_lossy = extract_unique_trigrams_utf8_lossy(b);
        let reference: HashSet<[u8; 3]> =
            extract_unique_trigrams_from_bytes(String::from_utf8_lossy(b).as_ref().as_bytes());
        assert_eq!(unique_lossy, reference);
    }
}
