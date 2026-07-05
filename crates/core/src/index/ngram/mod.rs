mod build;
mod files;
pub mod gram;
pub mod storage;

mod candidates;
mod config;
mod index;
mod lifecycle;
mod literals;

pub use config::Config;
pub use gram::{Gram, GramWidth, GramWindows};
pub use index::{Index, NGramIndexError};

#[cfg(test)]
mod candidate_tests {
    use std::path::Path;

    use crate::candidates::{CandidateFlags, CandidateSpec};
    use crate::index::ngram::storage::postings::Postings;

    use super::*;

    fn default_config() -> Config {
        Config::new(GramWidth::TRIGRAM)
    }

    fn narrow(
        patterns: &[String],
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> bool {
        let mut flags = CandidateFlags::empty();
        if case_insensitive {
            flags |= CandidateFlags::CASE_INSENSITIVE;
        }
        if word_regexp {
            flags |= CandidateFlags::WORD_REGEXP;
        }
        if line_regexp {
            flags |= CandidateFlags::LINE_REGEXP;
        }
        let spec = CandidateSpec { patterns, flags };
        default_config().extract_literal_arms(&spec).is_some()
    }

    fn full_scan(
        patterns: &[String],
        case_insensitive: bool,
        word_regexp: bool,
        line_regexp: bool,
    ) -> bool {
        let mut flags = CandidateFlags::empty();
        if case_insensitive {
            flags |= CandidateFlags::CASE_INSENSITIVE;
        }
        if word_regexp {
            flags |= CandidateFlags::WORD_REGEXP;
        }
        if line_regexp {
            flags |= CandidateFlags::LINE_REGEXP;
        }
        let spec = CandidateSpec { patterns, flags };
        default_config().extract_literal_arms(&spec).is_none()
    }

    #[test]
    fn merge_sorted_runs_preserves_order_and_uniqueness() {
        let merged = Index::merge_sorted_runs(vec![vec![1, 3, 7], vec![1, 2, 7, 9], vec![4, 7, 8]]);
        assert_eq!(merged, vec![1, 2, 3, 4, 7, 8, 9]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_handles_smallest_first_order() {
        let a = Postings::encode_list(&[1, 3, 5, 7, 9]);
        let b = Postings::encode_list(&[3, 7]);
        let c = Postings::encode_list(&[0, 3, 4, 7, 8]);
        let slices = vec![a.as_slice(), b.as_slice(), c.as_slice()];
        let ids = Index::intersect_sorted_slices(&slices);
        assert_eq!(ids, vec![3, 7]);
    }

    #[test]
    fn merge_sorted_runs_empty_input_returns_empty() {
        let merged = Index::merge_sorted_runs(vec![]);
        assert!(merged.is_empty());
    }

    #[test]
    fn merge_sorted_runs_single_list_returns_as_is() {
        let merged = Index::merge_sorted_runs(vec![vec![1, 2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn merge_sorted_runs_with_empty_lists_mixed_in() {
        let merged = Index::merge_sorted_runs(vec![vec![1, 3], vec![], vec![2, 3]]);
        assert_eq!(merged, vec![1, 2, 3]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_empty_input_returns_empty() {
        let ids = Index::intersect_sorted_slices(&[]);
        assert!(ids.is_empty());
    }

    #[test]
    fn intersect_sorted_slices_single_returns_decoded_ids() {
        let a = Postings::encode_list(&[1, 3, 5]);
        let ids = Index::intersect_sorted_slices(&[a.as_slice()]);
        assert_eq!(ids, vec![1, 3, 5]);
    }

    #[test]
    #[should_panic(expected = "postings validated at open")]
    fn intersect_sorted_slices_invalid_varint_panics() {
        let a = &[0xff];
        Index::intersect_sorted_slices(&[a]);
    }

    #[test]
    fn intersect_sorted_slices_no_overlap_returns_empty() {
        let a = Postings::encode_list(&[1, 2, 3]);
        let b = Postings::encode_list(&[4, 5, 6]);
        let ids = Index::intersect_sorted_slices(&[a.as_slice(), b.as_slice()]);
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
    fn generic_width_uses_spec_width_for_literal_extraction() {
        let spec = CandidateSpec {
            patterns: &["ab".to_string()],
            flags: CandidateFlags::empty(),
        };
        assert!(
            Config::new(GramWidth::new(2))
                .extract_literal_arms(&spec)
                .is_some()
        );
    }

    #[test]
    fn fixed_string_narrows() {
        let spec = CandidateSpec {
            patterns: &["beta.gamma".to_string()],
            flags: CandidateFlags::FIXED_STRINGS,
        };
        assert!(default_config().extract_literal_arms(&spec).is_some());
    }

    #[test]
    fn open_tables_accepts_count_mismatch() {
        use crate::index::ngram::storage::format::{
            FILES_MAGIC, GRAMS_MAGIC, LEXICON_MAGIC, POSTINGS_MAGIC,
        };
        use tempfile::TempDir;

        let tmp = TempDir::new().expect("create temp dir");
        let dir = tmp.path().join("index");
        std::fs::create_dir(&dir).expect("create index dir");

        let mut files = FILES_MAGIC.to_vec();
        files.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(dir.join("files.bin"), &files).expect("write files");

        let mut lex = LEXICON_MAGIC.to_vec();
        lex.extend_from_slice(&3u32.to_le_bytes());
        lex.extend_from_slice(&1u32.to_le_bytes());
        lex.extend_from_slice(&0u32.to_le_bytes());
        lex.extend_from_slice(b"abc");
        lex.extend_from_slice(&0u64.to_le_bytes());
        lex.extend_from_slice(&3u32.to_le_bytes());
        std::fs::write(dir.join("lexicon.bin"), &lex).expect("write lexicon");

        let posting_payload = Postings::encode_list(&[0, 1]);
        let mut pb = POSTINGS_MAGIC.to_vec();
        pb.extend_from_slice(&u32::try_from(posting_payload.len()).unwrap().to_le_bytes());
        pb.extend_from_slice(&posting_payload);
        std::fs::write(dir.join("postings.bin"), &pb).expect("write postings");

        let mut grams = GRAMS_MAGIC.to_vec();
        grams.extend_from_slice(&3u32.to_le_bytes());
        grams.extend_from_slice(&0u32.to_le_bytes());
        grams.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(dir.join(crate::GRAMS_BIN), &grams).expect("write grams");

        // Posting count mismatches are caught at build time.
        // The open path skips content-level validation for speed.
        let result = Config::open(
            GramWidth::TRIGRAM,
            &dir,
            Path::new("/root"),
            crate::index::CorpusKind::Directory,
        );
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod persistence_tests {
    use std::path::PathBuf;

    use super::files::FileFingerprint;
    use super::*;

    #[test]
    fn validate_file_paths_accepts_normal_relative_paths() {
        let fps = vec![
            FileFingerprint {
                path: PathBuf::from("a.txt"),
                mtime_secs: 0,
                size: 0,
            },
            FileFingerprint {
                path: PathBuf::from("sub/b.txt"),
                mtime_secs: 0,
                size: 0,
            },
        ];
        let result = Index::validate_file_paths(&fps);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_file_paths_rejects_absolute_paths() {
        let abs = std::env::current_dir().unwrap().join("a.txt");
        let fps = vec![FileFingerprint {
            path: abs,
            mtime_secs: 0,
            size: 0,
        }];
        let result = Index::validate_file_paths(&fps);
        assert!(result.is_err());
    }

    #[test]
    fn validate_file_paths_rejects_empty_paths() {
        let fps = vec![FileFingerprint {
            path: PathBuf::from(""),
            mtime_secs: 0,
            size: 0,
        }];
        let result = Index::validate_file_paths(&fps);
        assert!(result.is_err());
    }

    #[test]
    fn validate_file_paths_rejects_parent_dir_paths() {
        let fps = vec![FileFingerprint {
            path: PathBuf::from("../escape.txt"),
            mtime_secs: 0,
            size: 0,
        }];
        let result = Index::validate_file_paths(&fps);
        assert!(result.is_err());
    }
}
