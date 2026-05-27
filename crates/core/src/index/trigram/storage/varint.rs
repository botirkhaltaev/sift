//! Unsigned varint encoding for sorted delta-compressed integer lists.

use std::cmp::Ordering;

/// Integer values stored as 7-bit continuation varints in sorted delta lists.
pub trait Varint: Copy + Ord {
    /// Encode one varint to `out`.
    fn encode(self, out: &mut Vec<u8>);

    /// Decode one varint from `bytes` at `pos`. Returns `(value, next_pos)`.
    fn decode(bytes: &[u8], pos: usize) -> Option<(Self, usize)>;

    /// Identity for delta lists.
    fn zero() -> Self;

    /// `self - prev` for delta encoding (same semantics as sorted `u32` subtraction today).
    fn delta_from(self, prev: Self) -> Self;

    /// `prev + delta` for delta decoding.
    fn apply_delta(prev: Self, delta: Self) -> Self;
}

impl Varint for u32 {
    fn encode(self, out: &mut Vec<u8>) {
        encode_wire(out, u64::from(self));
    }

    fn decode(bytes: &[u8], pos: usize) -> Option<(Self, usize)> {
        let (value, next) = decode_wire(bytes, pos)?;
        Self::try_from(value).ok().map(|v| (v, next))
    }

    fn zero() -> Self {
        0
    }

    fn delta_from(self, prev: Self) -> Self {
        self - prev
    }

    fn apply_delta(prev: Self, delta: Self) -> Self {
        prev + delta
    }
}

impl Varint for u64 {
    fn encode(self, out: &mut Vec<u8>) {
        encode_wire(out, self);
    }

    fn decode(bytes: &[u8], pos: usize) -> Option<(Self, usize)> {
        decode_wire(bytes, pos)
    }

    fn zero() -> Self {
        0
    }

    fn delta_from(self, prev: Self) -> Self {
        self - prev
    }

    fn apply_delta(prev: Self, delta: Self) -> Self {
        prev + delta
    }
}

fn encode_wire(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(u8::try_from(value & 0x7F).expect("varint byte") | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("varint terminal byte"));
}

fn decode_wire(bytes: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while pos < bytes.len() {
        let byte = bytes[pos];
        pos += 1;
        value |= (u64::from(byte & 0x7f)) << shift;
        if byte & 0x80 == 0 {
            return Some((value, pos));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

/// Encode a sorted list using delta varints.
pub fn encode_sorted_deltas<T: Varint>(out: &mut Vec<u8>, values: &[T]) {
    let mut prev = T::zero();
    for (i, &value) in values.iter().enumerate() {
        if i == 0 {
            value.encode(out);
        } else {
            value.delta_from(prev).encode(out);
        }
        prev = value;
    }
}

/// Decode a delta-varint sorted list from `bytes`.
#[must_use]
pub fn decode_sorted_deltas<T: Varint>(bytes: &[u8]) -> Vec<T> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut prev = T::zero();
    while pos < bytes.len() {
        let Some((delta, next)) = T::decode(bytes, pos) else {
            return Vec::new();
        };
        pos = next;
        let value = if out.is_empty() {
            delta
        } else {
            T::apply_delta(prev, delta)
        };
        out.push(value);
        prev = value;
    }
    out
}

/// Iterator over a delta-varint encoded sorted list.
pub struct SortedDeltaIter<'a, T: Varint> {
    bytes: &'a [u8],
    pos: usize,
    prev: T,
    started: bool,
}

impl<'a, T: Varint> SortedDeltaIter<'a, T> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            prev: T::zero(),
            started: false,
        }
    }
}

impl<T: Varint> Iterator for SortedDeltaIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let (delta, next) = T::decode(self.bytes, self.pos)?;
        self.pos = next;
        let value = if self.started {
            T::apply_delta(self.prev, delta)
        } else {
            self.started = true;
            delta
        };
        self.prev = value;
        Some(value)
    }
}

/// Intersect a sorted slice with a delta-varint encoded sorted list.
#[must_use]
pub fn intersect_sorted<T: Varint>(ids: &[T], encoded: &[u8]) -> Vec<T> {
    let mut iter = SortedDeltaIter::<T>::new(encoded);
    let mut i = 0usize;
    let mut next = iter.next();
    let mut out = Vec::with_capacity(ids.len().min(encoded.len()));
    while i < ids.len() && next.is_some() {
        let encoded_id = next.expect("next id");
        match ids[i].cmp(&encoded_id) {
            Ordering::Less => i += 1,
            Ordering::Greater => next = iter.next(),
            Ordering::Equal => {
                out.push(ids[i]);
                i += 1;
                next = iter.next();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_varint_roundtrip() {
        for value in [0, 1, 127, 128, 16_383, 1_048_576, u32::MAX] {
            let mut buf = Vec::new();
            value.encode(&mut buf);
            let (decoded, pos) = u32::decode(&buf, 0).expect("decode");
            assert_eq!(decoded, value);
            assert_eq!(pos, buf.len());
        }
    }

    #[test]
    fn u64_varint_roundtrip() {
        for value in [0u64, 1, u64::MAX] {
            let mut buf = Vec::new();
            value.encode(&mut buf);
            let (decoded, pos) = u64::decode(&buf, 0).expect("decode");
            assert_eq!(decoded, value);
            assert_eq!(pos, buf.len());
        }
    }

    #[test]
    fn encode_decode_sorted_deltas_u32() {
        let ids = vec![0, 1, 5, 100, 10_000];
        let mut buf = Vec::new();
        encode_sorted_deltas(&mut buf, &ids);
        assert_eq!(decode_sorted_deltas::<u32>(&buf), ids);
    }

    #[test]
    fn sorted_delta_iter_matches_decode() {
        let ids: Vec<u32> = (0..100).map(|i| i * 3).collect();
        let mut buf = Vec::new();
        encode_sorted_deltas(&mut buf, &ids);
        let collected: Vec<u32> = SortedDeltaIter::<u32>::new(&buf).collect();
        assert_eq!(collected, ids);
    }

    #[test]
    fn intersect_sorted_u32() {
        let left = vec![1u32, 3, 5, 7];
        let mut encoded = Vec::new();
        encode_sorted_deltas(&mut encoded, &[2u32, 3, 6, 7]);
        assert_eq!(intersect_sorted(&left, &encoded), vec![3, 7]);
    }
}
