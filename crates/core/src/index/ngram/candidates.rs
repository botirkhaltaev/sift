use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::gram::{Gram, GramWindows};
use super::index::Index;
use super::storage::postings::Postings;

impl Index {
    pub(crate) fn candidate_file_ids(&self, arms: &[Vec<u8>], case_insensitive: bool) -> Vec<u32> {
        if arms.is_empty() {
            return Vec::new();
        }
        if arms.len() == 1 {
            return self
                .posting_ids_for_literal(&arms[0], case_insensitive)
                .unwrap_or_default();
        }
        let mut id_lists: Vec<Vec<u32>> = Vec::with_capacity(arms.len());
        for arm in arms {
            if let Some(ids) = self.posting_ids_for_literal(arm, case_insensitive) {
                id_lists.push(ids);
            }
        }
        Self::merge_sorted_runs(id_lists)
    }

    fn posting_ids_for_literal(&self, lit: &[u8], case_insensitive: bool) -> Option<Vec<u32>> {
        let width = self.width.get();
        if lit.len() < width {
            return None;
        }
        if case_insensitive {
            return self.posting_ids_for_ascii_casei_literal(lit);
        }
        let grams: Vec<Gram> = GramWindows::new(lit, self.width).collect();
        if grams.is_empty() {
            return None;
        }
        let mut slices: Vec<&[u8]> = Vec::with_capacity(grams.len());
        for gram in &grams {
            let s = self.posting_bytes_slice(*gram);
            if s.is_empty() {
                return None;
            }
            slices.push(s);
        }
        let ids = Self::intersect_sorted_slices(&slices);
        if ids.is_empty() { None } else { Some(ids) }
    }

    /// Narrow with ASCII case folding: for each gram window, union postings of all
    /// ASCII letter case variants, then intersect across windows.
    fn posting_ids_for_ascii_casei_literal(&self, lit: &[u8]) -> Option<Vec<u32>> {
        debug_assert!(lit.is_ascii());
        let width = self.width.get();
        let mut window = vec![0u8; width];
        let mut cur: Option<Vec<u32>> = None;

        for offset in 0..=lit.len() - width {
            window.copy_from_slice(&lit[offset..offset + width]);
            let mut variant_lists = Vec::new();
            for variant in AsciiCaseVariants::new(&mut window) {
                let slice = self.posting_bytes_slice(variant);
                if !slice.is_empty() {
                    variant_lists
                        .push(Postings::decode_sorted(slice).expect("postings validated at open"));
                }
            }
            let unioned = Self::merge_sorted_runs(variant_lists);
            if unioned.is_empty() {
                return None;
            }
            cur = Some(match cur {
                None => unioned,
                Some(prev) => intersect_sorted_ids(&prev, &unioned),
            });
            if cur.as_ref().is_some_and(Vec::is_empty) {
                return None;
            }
        }
        cur.filter(|ids| !ids.is_empty())
    }

    fn posting_bytes_slice(&self, gram: Gram) -> &[u8] {
        let Some(entry) = self.storage.lexicon.get(gram) else {
            return &[];
        };
        let start = usize::try_from(entry.offset).unwrap_or(usize::MAX);
        let payload_len = self.storage.postings.payload_len();
        let end = self
            .storage
            .lexicon
            .posting_byte_end(entry.offset, payload_len);
        self.storage
            .postings
            .slice(start, end.saturating_sub(start))
    }

    pub(crate) fn intersect_sorted_slices(slices: &[&[u8]]) -> Vec<u32> {
        if slices.is_empty() {
            return Vec::new();
        }
        if slices.len() == 1 {
            return Postings::decode_sorted(slices[0]).expect("postings validated at open");
        }
        if slices.len() == 2 {
            let (first, second) = if slices[0].len() <= slices[1].len() {
                (slices[0], slices[1])
            } else {
                (slices[1], slices[0])
            };
            if first == second {
                return Postings::decode_sorted(first).expect("postings validated at open");
            }
            let ids = Postings::decode_sorted(first).expect("postings validated at open");
            return Postings::intersect_sorted(&ids, second).expect("postings validated at open");
        }
        let mut ordered: Vec<&[u8]> = slices.to_vec();
        ordered.sort_unstable_by_key(|slice| slice.len());
        if ordered[1..].iter().all(|slice| *slice == ordered[0]) {
            return Postings::decode_sorted(ordered[0]).expect("postings validated at open");
        }
        let mut cur = Postings::decode_sorted(ordered[0]).expect("postings validated at open");
        for s in &ordered[1..] {
            cur = Postings::intersect_sorted(&cur, s).expect("postings validated at open");
            if cur.is_empty() {
                break;
            }
        }
        cur
    }

    pub(crate) fn merge_sorted_runs(lists: Vec<Vec<u32>>) -> Vec<u32> {
        if lists.is_empty() {
            return Vec::new();
        }
        if lists.len() == 1 {
            return lists.into_iter().next().unwrap_or_default();
        }

        let total: usize = lists.iter().map(Vec::len).sum();
        let mut heap: BinaryHeap<Reverse<(u32, usize)>> = BinaryHeap::with_capacity(lists.len());
        let mut positions = vec![0usize; lists.len()];

        for (list_idx, list) in lists.iter().enumerate() {
            if let Some(&first) = list.first() {
                heap.push(Reverse((first, list_idx)));
            }
        }

        let mut out = Vec::with_capacity(total);
        let mut last = None;
        while let Some(Reverse((value, list_idx))) = heap.pop() {
            if last != Some(value) {
                out.push(value);
                last = Some(value);
            }

            positions[list_idx] += 1;
            if let Some(&next) = lists[list_idx].get(positions[list_idx]) {
                heap.push(Reverse((next, list_idx)));
            }
        }
        out
    }
}

fn intersect_sorted_ids(left: &[u32], right: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(left.len().min(right.len()));
    let mut i = 0;
    let mut j = 0;
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(left[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// Yields every ASCII letter case variant of a gram window as [`Gram`] values.
///
/// Non-letters are left unchanged. Alphabetic bytes are normalized to lowercase
/// before enumeration so bit patterns cover the full upper/lower product.
struct AsciiCaseVariants<'a> {
    bytes: &'a mut [u8],
    alpha_idx: Vec<usize>,
    state: u32,
    total: u32,
}

impl<'a> AsciiCaseVariants<'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        let alpha_idx: Vec<usize> = bytes
            .iter()
            .enumerate()
            .filter_map(|(i, &b)| b.is_ascii_alphabetic().then_some(i))
            .collect();
        let n = alpha_idx.len();
        let total = 1u32 << n;
        for &i in &alpha_idx {
            bytes[i] = bytes[i].to_ascii_lowercase();
        }
        Self {
            bytes,
            alpha_idx,
            state: 0,
            total,
        }
    }
}

impl Iterator for AsciiCaseVariants<'_> {
    type Item = Gram;

    fn next(&mut self) -> Option<Self::Item> {
        if self.state >= self.total {
            return None;
        }
        for (bit, &idx) in self.alpha_idx.iter().enumerate() {
            if (self.state >> bit) & 1 == 1 {
                self.bytes[idx] = self.bytes[idx].to_ascii_uppercase();
            } else {
                self.bytes[idx] = self.bytes[idx].to_ascii_lowercase();
            }
        }
        let gram = Gram::from_window(self.bytes);
        self.state += 1;
        Some(gram)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_case_variants_cover_letter_product() {
        let mut bytes = *b"Ab_";
        let grams: Vec<_> = AsciiCaseVariants::new(&mut bytes).collect();
        assert_eq!(grams.len(), 4);
        let ords: Vec<_> = grams.into_iter().map(Gram::ordinal).collect();
        assert!(ords.contains(&Gram::from_window(b"ab_").ordinal()));
        assert!(ords.contains(&Gram::from_window(b"Ab_").ordinal()));
        assert!(ords.contains(&Gram::from_window(b"aB_").ordinal()));
        assert!(ords.contains(&Gram::from_window(b"AB_").ordinal()));
    }

    #[test]
    fn ascii_case_variants_non_alpha_single() {
        let mut bytes = *b"12_";
        let grams: Vec<_> = AsciiCaseVariants::new(&mut bytes).collect();
        assert_eq!(grams, vec![Gram::from_window(b"12_")]);
    }
}
