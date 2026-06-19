use std::cmp::Reverse;
use std::collections::BinaryHeap;

use regex_syntax::ast::parse::Parser as AstParser;
use regex_syntax::hir::literal::{ExtractKind, Extractor};
use regex_syntax::hir::{self, Hir};

use crate::index::{FileId, PlanMode, QueryPlanOutput};
use crate::query::QuerySpec;

use super::TrigramIndex;
use super::key::Trigram;
use super::storage;

impl TrigramIndex {
    /// Produce narrowed candidate files for the query.
    /// Returns `None` if the query can't be narrowed (full scan required).
    #[must_use]
    pub fn candidates(&self, query: &QuerySpec<'_>) -> Option<Vec<crate::Candidate>> {
        let arms = Self::extract_literal_arms(query)?;
        Some(
            self.candidate_file_ids(&arms)
                .into_iter()
                .filter_map(|id| {
                    let fid = FileId::new(usize::try_from(id).ok()?);
                    let fp = self.fingerprints.get(fid.get())?;
                    Some(crate::Candidate::new(
                        fp.path.clone(),
                        self.root.join(&fp.path),
                    ))
                })
                .collect(),
        )
    }

    /// Returns an explanation of how a query would be handled.
    #[must_use]
    pub fn explain(&self, query: &QuerySpec<'_>) -> QueryPlanOutput {
        let mode = match Self::extract_literal_arms(query) {
            Some(_) => PlanMode::IndexedCandidates,
            None => PlanMode::FullScan,
        };
        QueryPlanOutput {
            pattern: query.patterns.to_vec().join("|"),
            mode,
        }
    }

    #[must_use]
    pub(crate) fn all_files(&self) -> Vec<crate::Candidate> {
        self.fingerprints
            .iter()
            .map(|fp| crate::Candidate::new(fp.path.clone(), self.root.join(&fp.path)))
            .collect()
    }

    fn candidate_file_ids(&self, arms: &[Vec<u8>]) -> Vec<u32> {
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
        if lit.len() < 3 {
            return None;
        }
        let trigrams: Vec<Trigram> = Trigram::windows(lit).collect();
        if trigrams.is_empty() {
            return None;
        }
        let mut slices: Vec<&[u8]> = Vec::with_capacity(trigrams.len());
        for tri in &trigrams {
            let s = self.posting_bytes_slice(*tri);
            if s.is_empty() {
                return None;
            }
            slices.push(s);
        }
        slices.sort_unstable_by_key(|slice| slice.len());
        let ids = Self::intersect_sorted_slices(&slices);
        if ids.is_empty() { None } else { Some(ids) }
    }

    fn posting_bytes_slice(&self, tri: Trigram) -> &[u8] {
        let Some(entry) = self.lexicon.get(tri.to_bytes()) else {
            return &[];
        };
        let start = usize::try_from(entry.offset).unwrap_or(usize::MAX);
        let payload_len = self.postings.payload_len();
        let end = self.lexicon.posting_byte_end(entry.offset, payload_len);
        self.postings.slice(start, end.saturating_sub(start))
    }

    fn intersect_sorted_slices(slices: &[&[u8]]) -> Vec<u32> {
        if slices.is_empty() {
            return Vec::new();
        }
        if slices.len() == 1 {
            return storage::postings::Postings::decode_sorted(slices[0])
                .expect("postings validated at open");
        }
        let mut ordered: Vec<&[u8]> = slices.to_vec();
        ordered.sort_unstable_by_key(|slice| slice.len());
        let mut cur = storage::postings::Postings::decode_sorted(ordered[0])
            .expect("postings validated at open");
        for s in &ordered[1..] {
            cur = storage::postings::Postings::intersect_sorted(&cur, s)
                .expect("postings validated at open");
            if cur.is_empty() {
                break;
            }
        }
        cur
    }

    fn merge_sorted_runs(lists: Vec<Vec<u32>>) -> Vec<u32> {
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

    // -----------------------------------------------------------------------
    // Literal extraction (absorbs TrigramPlanner)
    // -----------------------------------------------------------------------

    /// Extract literal byte arms from a query spec.
    /// Returns `None` if no usable literals >= 3 bytes can be extracted.
    fn extract_literal_arms(spec: &QuerySpec<'_>) -> Option<Vec<Vec<u8>>> {
        if spec.invert_match() {
            return None;
        }
        let mut literal_arms: Vec<Vec<u8>> = Vec::new();
        for p in spec.patterns {
            let arms = if spec.fixed_strings() {
                Self::fixed_string_literals(p.as_bytes(), spec.case_insensitive())
            } else {
                Self::plan_pattern(
                    p.as_str(),
                    spec.case_insensitive(),
                    spec.word_regexp(),
                    spec.line_regexp(),
                )?
            };
            for lit in arms {
                if lit.len() < 3 {
                    return None;
                }
                literal_arms.push(lit);
            }
        }
        if literal_arms.is_empty() {
            return None;
        }
        Some(literal_arms)
    }

    fn plan_pattern(
        pattern: &str,
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> Option<Vec<Vec<u8>>> {
        let hir = Self::build_configured_hir(pattern, case_insensitive)?;
        let shaped = Self::shape_hir(hir, word_regexp, line_regexp);
        let lits = Self::extract_literals(&shaped);
        if lits.is_empty() { None } else { Some(lits) }
    }

    fn build_configured_hir(pattern: &str, case_insensitive: bool) -> Option<Hir> {
        let ast = AstParser::new().parse(pattern).ok()?;
        let mut builder = regex_syntax::hir::translate::TranslatorBuilder::new();
        builder.unicode(true);
        if case_insensitive {
            builder.case_insensitive(true);
        }
        let mut translator = builder.build();
        let hir = translator.translate(pattern, &ast).ok()?;
        Some(hir)
    }

    fn shape_hir(hir: Hir, word_regexp: bool, line_regexp: bool) -> Hir {
        if line_regexp {
            Self::wrap_line(hir)
        } else if word_regexp {
            Self::wrap_word(hir)
        } else {
            hir
        }
    }

    fn wrap_word(hir: Hir) -> Hir {
        Hir::concat(vec![
            Hir::look(hir::Look::WordStartHalfUnicode),
            hir,
            Hir::look(hir::Look::WordEndHalfUnicode),
        ])
    }

    fn wrap_line(hir: Hir) -> Hir {
        Hir::concat(vec![
            Hir::look(hir::Look::StartLF),
            hir,
            Hir::look(hir::Look::EndLF),
        ])
    }

    fn extract_literals(hir: &Hir) -> Vec<Vec<u8>> {
        let extractor_prefix = Extractor::new();
        let extractor_suffix = {
            let mut e = Extractor::new();
            e.kind(ExtractKind::Suffix);
            e
        };

        let seq_prefix = extractor_prefix.extract(hir);
        let seq_suffix = extractor_suffix.extract(hir);

        let lits_prefix = seq_prefix.literals();
        let lits_suffix = seq_suffix.literals();

        Self::pick_better_lits(lits_prefix, lits_suffix)
    }

    fn pick_better_lits(
        lits_a: Option<&[regex_syntax::hir::literal::Literal]>,
        lits_b: Option<&[regex_syntax::hir::literal::Literal]>,
    ) -> Vec<Vec<u8>> {
        fn total_bytes(lits: Option<&[regex_syntax::hir::literal::Literal]>) -> usize {
            lits.map_or(0, |l| l.iter().map(|lit| lit.as_bytes().len()).sum())
        }

        let a_count = lits_a.map_or(0, <[regex_syntax::hir::literal::Literal]>::len);
        let b_count = lits_b.map_or(0, <[regex_syntax::hir::literal::Literal]>::len);
        let a_has = a_count > 0;
        let b_has = b_count > 0;

        let lits = match (a_has, b_has) {
            (true, false) => lits_a,
            (false, true) => lits_b,
            (false, false) => return Vec::new(),
            (true, true) => {
                let a_total = total_bytes(lits_a);
                let b_total = total_bytes(lits_b);
                if a_total >= b_total { lits_a } else { lits_b }
            }
        };

        let lits = match lits {
            Some(l) if !l.is_empty() => l,
            _ => return Vec::new(),
        };

        let mut out = Vec::new();
        for lit in lits {
            let bytes = lit.as_bytes();
            if bytes.len() >= 3 {
                out.push(bytes.to_vec());
            }
        }
        out
    }

    fn fixed_string_literals(lit: &[u8], case_insensitive: bool) -> Vec<Vec<u8>> {
        if case_insensitive {
            vec![lit.to_ascii_lowercase()]
        } else {
            vec![lit.to_vec()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::QueryFlags;
    use std::path::Path;

    fn encode(ids: &[u32]) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut prev = 0u64;
        for (i, &value) in ids.iter().enumerate() {
            let raw = if i == 0 {
                u64::from(value)
            } else {
                u64::from(value) - prev
            };
            let mut varint_buf = unsigned_varint::encode::u64_buffer();
            let encoded = unsigned_varint::encode::u64(raw, &mut varint_buf);
            buf.extend_from_slice(encoded);
            prev = u64::from(value);
        }
        buf
    }

    fn narrow(
        patterns: &[String],
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> bool {
        let mut flags = QueryFlags::empty();
        if case_insensitive {
            flags |= QueryFlags::CASE_INSENSITIVE;
        }
        if word_regexp {
            flags |= QueryFlags::WORD_REGEXP;
        }
        if line_regexp {
            flags |= QueryFlags::LINE_REGEXP;
        }
        let spec = QuerySpec { patterns, flags };
        TrigramIndex::extract_literal_arms(&spec).is_some()
    }

    fn full_scan(
        patterns: &[String],
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> bool {
        let mut flags = QueryFlags::empty();
        if case_insensitive {
            flags |= QueryFlags::CASE_INSENSITIVE;
        }
        if word_regexp {
            flags |= QueryFlags::WORD_REGEXP;
        }
        if line_regexp {
            flags |= QueryFlags::LINE_REGEXP;
        }
        let spec = QuerySpec { patterns, flags };
        TrigramIndex::extract_literal_arms(&spec).is_none()
    }

    #[test]
    fn merge_sorted_runs_preserves_order_and_uniqueness() {
        let merged =
            TrigramIndex::merge_sorted_runs(vec![vec![1, 3, 7], vec![1, 2, 7, 9], vec![4, 7, 8]]);
        assert_eq!(merged, vec![1, 2, 3, 4, 7, 8, 9]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_handles_smallest_first_order() {
        let a = encode(&[1, 3, 5, 7, 9]);
        let b = encode(&[3, 7]);
        let c = encode(&[0, 3, 4, 7, 8]);
        let slices = vec![a.as_slice(), b.as_slice(), c.as_slice()];
        let ids = TrigramIndex::intersect_sorted_slices(&slices);
        assert_eq!(ids, vec![3, 7]);
    }

    #[test]
    fn merge_sorted_runs_empty_input_returns_empty() {
        let merged = TrigramIndex::merge_sorted_runs(vec![]);
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_sorted_runs_single_list_returns_as_is() {
        let merged = TrigramIndex::merge_sorted_runs(vec![vec![1, 2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn merge_sorted_runs_with_empty_lists_mixed_in() {
        let merged = TrigramIndex::merge_sorted_runs(vec![vec![1, 3], vec![], vec![2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_empty_input_returns_empty() {
        let ids = TrigramIndex::intersect_sorted_slices(&[]);
        assert!(ids.is_empty());
    }

    #[test]
    fn intersect_sorted_slices_single_returns_decoded_ids() {
        let a = encode(&[1, 3, 5]);
        let ids = TrigramIndex::intersect_sorted_slices(&[a.as_slice()]);
        assert_eq!(ids, vec![1, 3, 5]);
    }

    #[test]
    #[should_panic(expected = "postings validated at open")]
    fn intersect_sorted_slices_invalid_varint_panics() {
        let a = &[0xff];
        TrigramIndex::intersect_sorted_slices(&[a]);
    }

    #[test]
    fn intersect_sorted_slices_no_overlap_returns_empty() {
        let a = encode(&[1, 2, 3]);
        let b = encode(&[4, 5, 6]);
        let ids = TrigramIndex::intersect_sorted_slices(&[a.as_slice(), b.as_slice()]);
        assert!(ids.is_empty());
    }

    #[test]
    fn literal_narrows() {
        assert!(narrow(&["beta".to_string()], false, false, false));
    }

    #[test]
    fn dot_star_full_scan() {
        assert!(full_scan(&[".*".to_string()], false, false, false));
    }

    #[test]
    fn alternation_narrows() {
        assert!(narrow(&[r"foo|bar".to_string()], false, false, false));
    }

    #[test]
    fn word_literal_narrows() {
        assert!(narrow(&["beta".to_string()], false, true, false));
    }

    #[test]
    fn line_regexp_narrows() {
        assert!(narrow(&["beta".to_string()], false, false, true));
    }

    #[test]
    fn case_insensitive_narrows() {
        assert!(narrow(&["beta".to_string()], true, false, false));
    }

    #[test]
    fn required_literal_inside_regex_narrows() {
        assert!(narrow(&["[A-Z]+_RESUME".to_string()], false, false, false));
    }

    #[test]
    fn unicode_class_full_scan() {
        assert!(full_scan(&[r"\p{Greek}".to_string()], false, false, false));
    }

    #[test]
    fn no_literal_full_scan() {
        assert!(full_scan(
            &[r"\w{5}\s+\w{5}".to_string()],
            false,
            false,
            false
        ));
    }

    #[test]
    fn short_literal_full_scan() {
        assert!(full_scan(&["ab".to_string()], false, false, false));
    }

    #[test]
    fn fixed_string_narrows() {
        let spec = QuerySpec {
            patterns: &["beta.gamma".to_string()],
            flags: QueryFlags::FIXED_STRINGS,
        };
        assert!(TrigramIndex::extract_literal_arms(&spec).is_some());
    }

    #[test]
    fn open_tables_accepts_count_mismatch() {
        use crate::index::trigram::storage::format::{
            FILES_MAGIC, LEXICON_MAGIC, POSTINGS_MAGIC, TRIGRAMS_MAGIC,
        };
        use tempfile::TempDir;
        let tmp = TempDir::new().expect("create temp dir");
        let dir = tmp.path().join("index");
        std::fs::create_dir(&dir).expect("create index dir");

        let mut files = FILES_MAGIC.to_vec();
        files.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(dir.join("files.bin"), &files).expect("write files");

        let mut lex = LEXICON_MAGIC.to_vec();
        lex.extend_from_slice(&1u32.to_le_bytes());
        lex.extend_from_slice(b"abc");
        lex.extend_from_slice(&0u64.to_le_bytes());
        lex.extend_from_slice(&3u32.to_le_bytes());
        std::fs::write(dir.join("lexicon.bin"), &lex).expect("write lexicon");

        let mut posting_payload = Vec::new();
        let mut buf = unsigned_varint::encode::u64_buffer();
        posting_payload.extend_from_slice(unsigned_varint::encode::u64(0, &mut buf));
        let mut buf2 = unsigned_varint::encode::u64_buffer();
        posting_payload.extend_from_slice(unsigned_varint::encode::u64(1, &mut buf2));
        let mut pb = POSTINGS_MAGIC.to_vec();
        pb.extend_from_slice(&u32::try_from(posting_payload.len()).unwrap().to_le_bytes());
        pb.extend_from_slice(&posting_payload);
        std::fs::write(dir.join("postings.bin"), &pb).expect("write postings");

        let mut tri = TRIGRAMS_MAGIC.to_vec();
        tri.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(dir.join("trigrams.bin"), &tri).expect("write trigrams");

        // Posting count mismatches are caught at build time.
        // The open path skips content-level validation for speed.
        let result = TrigramIndex::open(
            &dir,
            Path::new("/root"),
            crate::index::CorpusKind::Directory,
        );
        assert!(result.is_ok());
    }
}
