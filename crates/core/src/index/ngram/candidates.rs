use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::gram::{Gram, GramWindows};
use super::index::Index;
use super::storage::postings::Postings;

impl Index {
    pub(crate) fn candidate_file_ids(&self, arms: &[Vec<u8>]) -> Vec<u32> {
        if arms.is_empty() {
            return Vec::new();
        }
        if arms.len() == 1 {
            return self.posting_ids_for_literal(&arms[0]).unwrap_or_default();
        }
        let mut id_lists: Vec<Vec<u32>> = Vec::with_capacity(arms.len());
        for arm in arms {
            if let Some(ids) = self.posting_ids_for_literal(arm) {
                id_lists.push(ids);
            }
        }
        Self::merge_sorted_runs(id_lists)
    }

    fn posting_ids_for_literal(&self, lit: &[u8]) -> Option<Vec<u32>> {
        let width = self.width.get();
        if lit.len() < width {
            return None;
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
