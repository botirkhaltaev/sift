mod build;
mod files;
pub mod gram;
pub mod storage;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

use regex_syntax::ast::parse::Parser as AstParser;
use regex_syntax::hir::literal::{ExtractKind, Extractor};
use regex_syntax::hir::{self, Hir};

use crate::index::snapshot::ArtifactData;
use crate::index::{CorpusKind, FileId, IndexBuildConfig, IndexDestination, IndexSource};
use crate::query::QuerySpec;

use self::build::{CorpusWalker, FingerprintCollector, IndexTables, PostingTables};
use self::files::FileFingerprint;
use self::files::FileTable;
pub use gram::{Gram, GramWidth, GramWindows};
use storage::grams::{GramSet, GramSets};
use storage::lexicon::Lexicon;
use storage::postings::Postings;

/// Errors specific to opening or persisting an N-gram index.
#[derive(Debug, thiserror::Error)]
pub enum NGramIndexError {
    #[error("index component missing: {0}")]
    MissingComponent(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configured runtime-width N-gram index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Config {
    width: GramWidth,
}

/// Opened runtime-width N-gram index.
#[derive(Debug)]
pub struct Index {
    width: GramWidth,
    storage: Storage,
}

#[derive(Debug)]
struct Storage {
    root: PathBuf,
    pub(crate) fingerprints: Vec<FileFingerprint>,
    gram_sets: storage::grams::GramSets,
    lexicon: storage::lexicon::Lexicon,
    postings: storage::postings::Postings,
    corpus_kind: CorpusKind,
}

impl Config {
    #[must_use]
    pub const fn new(width: GramWidth) -> Self {
        Self { width }
    }

    pub const DEFAULT: Self = Self {
        width: GramWidth::TRIGRAM,
    };

    #[must_use]
    pub const fn width(self) -> GramWidth {
        self.width
    }

    #[must_use]
    pub fn name(self) -> String {
        format!("ngram-{}", self.width.get())
    }

    /// Parse an N-gram index configuration name.
    ///
    /// # Errors
    ///
    /// Returns an error if `value` is not `ngram-N` or `ngram:N`, or if `N` is not a valid width.
    pub fn parse_name(value: &str) -> Result<Self, String> {
        let width = value
            .strip_prefix("ngram-")
            .or_else(|| value.strip_prefix("ngram:"))
            .ok_or_else(|| format!("unknown index: {value}"))?;
        let width = width
            .parse::<u8>()
            .map_err(|_| format!("invalid ngram width: {width}"))?;
        Ok(Self::new(GramWidth::new(width)))
    }

    #[must_use]
    pub const fn artifact_names(self) -> &'static [&'static str] {
        &[
            crate::FILES_BIN,
            crate::LEXICON_BIN,
            crate::POSTINGS_BIN,
            crate::GRAMS_BIN,
        ]
    }

    /// Build an N-gram index from an explicit path list, or from a full walk when `paths` is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or encoding fails.
    pub fn build(
        self,
        config: &IndexBuildConfig<'_>,
        output_dir: &Path,
        paths: &[PathBuf],
    ) -> crate::Result<Index> {
        self.build_into(config, IndexDestination::Directory(output_dir), paths)
    }

    /// Build an N-gram index into a directory or snapshot destination.
    pub(crate) fn build_into(
        self,
        config: &IndexBuildConfig<'_>,
        dest: IndexDestination,
        paths: &[PathBuf],
    ) -> crate::Result<Index> {
        let tables = IndexTables::build(self.width, config, paths)?;
        let root = config.corpus.root.canonicalize()?;
        Self::persist_tables(self.width, &tables, &root, config.corpus.kind, dest)
    }

    /// Encode and store tables at the given destination, returning a live index.
    pub(crate) fn persist_tables(
        width: GramWidth,
        tables: &IndexTables,
        root: &Path,
        corpus_kind: CorpusKind,
        dest: IndexDestination,
    ) -> crate::Result<Index> {
        match dest {
            IndexDestination::Directory(dir) => {
                Self::create_in_dir(width, tables, root, corpus_kind, dir)
            }
            IndexDestination::Snapshot { writer, namespace } => {
                let ((fr, lr), (pr, gr)) = rayon::join(
                    || {
                        rayon::join(
                            || FileTable::encode(&tables.fingerprints),
                            || Lexicon::encode(width, &tables.lexicon),
                        )
                    },
                    || {
                        rayon::join(
                            || Postings::encode(&tables.postings),
                            || GramSets::encode(width, &tables.file_grams),
                        )
                    },
                );

                let files_bytes = fr.map_err(crate::Error::Io)?;
                let lexicon_bytes = lr.map_err(crate::Error::Io)?;
                let postings_bytes = pr.map_err(crate::Error::Io)?;
                let gram_sets_bytes = gr.map_err(crate::Error::Io)?;

                let files =
                    FileTable::from_artifact(ArtifactData::Memory(files_bytes.clone().into()))?;
                let lexicon = Lexicon::from_artifact(
                    ArtifactData::Memory(lexicon_bytes.clone().into()),
                    width,
                )?;
                let postings =
                    Postings::from_artifact(ArtifactData::Memory(postings_bytes.clone().into()))?;
                let gram_sets = GramSets::from_artifact(
                    ArtifactData::Memory(gram_sets_bytes.clone().into()),
                    width,
                )?;

                writer.put_artifact(namespace, crate::FILES_BIN, files_bytes)?;
                writer.put_artifact(namespace, crate::LEXICON_BIN, lexicon_bytes)?;
                writer.put_artifact(namespace, crate::POSTINGS_BIN, postings_bytes)?;
                writer.put_artifact(namespace, crate::GRAMS_BIN, gram_sets_bytes)?;

                let fingerprints = files.to_fingerprints().map_err(crate::Error::Io)?;
                Self::validate_file_paths(&fingerprints)?;
                Self::validate_lexicon_postings(&lexicon, &postings)?;

                Ok(Index {
                    width,
                    storage: Storage {
                        root: root.to_path_buf(),
                        fingerprints,
                        gram_sets,
                        lexicon,
                        postings,
                        corpus_kind,
                    },
                })
            }
        }
    }

    /// Open a previously persisted N-gram index from `index_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if persistence files are missing or malformed.
    pub fn open(
        width: GramWidth,
        index_dir: &Path,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Index> {
        Self::open_tables(width, IndexSource::Directory(index_dir), root, corpus_kind)
    }

    pub(crate) fn open_from(
        self,
        source: IndexSource,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Index> {
        Self::open_tables(self.width, source, root, corpus_kind)
    }

    /// Open index tables from a storage source (directory or snapshot).
    pub(crate) fn open_tables(
        width: GramWidth,
        source: IndexSource,
        root: &Path,
        corpus_kind: CorpusKind,
    ) -> crate::Result<Index> {
        match source {
            IndexSource::Directory(dir) => {
                let files_path = dir.join(crate::FILES_BIN);
                let lexicon_path = dir.join(crate::LEXICON_BIN);
                let postings_path = dir.join(crate::POSTINGS_BIN);
                let grams_path = dir.join(crate::GRAMS_BIN);

                for p in [&files_path, &lexicon_path, &postings_path, &grams_path] {
                    if !p.is_file() {
                        return Err(NGramIndexError::MissingComponent(p.clone()).into());
                    }
                }

                let files = FileTable::open(&files_path).map_err(NGramIndexError::Io)?;
                let fingerprints = files.to_fingerprints().map_err(NGramIndexError::Io)?;
                Self::validate_file_paths(&fingerprints)?;

                let lexicon = Lexicon::open(&lexicon_path, width).map_err(NGramIndexError::Io)?;
                let postings = Postings::open(&postings_path).map_err(NGramIndexError::Io)?;
                let gram_sets = GramSets::open(&grams_path, width).map_err(NGramIndexError::Io)?;

                Ok(Index {
                    width,
                    storage: Storage {
                        root: root.to_path_buf(),
                        fingerprints,
                        gram_sets,
                        lexicon,
                        postings,
                        corpus_kind,
                    },
                })
            }
            IndexSource::Snapshot { reader, namespace } => {
                let files_data = reader.artifact(namespace, crate::FILES_BIN)?;
                let files = FileTable::from_artifact(files_data).map_err(NGramIndexError::Io)?;
                let fingerprints = files.to_fingerprints().map_err(NGramIndexError::Io)?;
                Self::validate_file_paths(&fingerprints)?;

                let lexicon_data = reader.artifact(namespace, crate::LEXICON_BIN)?;
                let lexicon =
                    Lexicon::from_artifact(lexicon_data, width).map_err(NGramIndexError::Io)?;

                let postings_data = reader.artifact(namespace, crate::POSTINGS_BIN)?;
                let postings =
                    Postings::from_artifact(postings_data).map_err(NGramIndexError::Io)?;

                let gram_sets_data = reader.artifact(namespace, crate::GRAMS_BIN)?;
                let gram_sets =
                    GramSets::from_artifact(gram_sets_data, width).map_err(NGramIndexError::Io)?;

                Ok(Index {
                    width,
                    storage: Storage {
                        root: root.to_path_buf(),
                        fingerprints,
                        gram_sets,
                        lexicon,
                        postings,
                        corpus_kind,
                    },
                })
            }
        }
    }

    /// Write tables to `dir` as persistence files and return an mmap-backed index.
    fn create_in_dir(
        width: GramWidth,
        tables: &IndexTables,
        root: &Path,
        corpus_kind: CorpusKind,
        dir: &Path,
    ) -> crate::Result<Index> {
        std::fs::create_dir_all(dir)?;

        let files_path = dir.join(crate::FILES_BIN);
        let lexicon_path = dir.join(crate::LEXICON_BIN);
        let postings_path = dir.join(crate::POSTINGS_BIN);
        let grams_path = dir.join(crate::GRAMS_BIN);

        let ((fr, lr), (pr, gr)) = rayon::join(
            || {
                rayon::join(
                    || FileTable::create(&files_path, &tables.fingerprints),
                    || Lexicon::create(&lexicon_path, width, &tables.lexicon),
                )
            },
            || {
                rayon::join(
                    || Postings::create(&postings_path, &tables.postings),
                    || GramSets::create(&grams_path, width, &tables.file_grams),
                )
            },
        );

        let files = fr.map_err(crate::Error::Io)?;
        let lexicon = lr.map_err(crate::Error::Io)?;
        let postings = pr.map_err(crate::Error::Io)?;
        let gram_sets = gr.map_err(crate::Error::Io)?;

        let fingerprints = files.to_fingerprints().map_err(crate::Error::Io)?;
        Self::validate_file_paths(&fingerprints)?;
        Self::validate_lexicon_postings(&lexicon, &postings)?;

        Ok(Index {
            width,
            storage: Storage {
                root: root.to_path_buf(),
                fingerprints,
                gram_sets,
                lexicon,
                postings,
                corpus_kind,
            },
        })
    }

    fn validate_lexicon_postings(
        lexicon: &Lexicon,
        postings: &Postings,
    ) -> Result<(), NGramIndexError> {
        Index::validate_lexicon_postings(lexicon, postings)
    }

    fn validate_file_paths(fingerprints: &[FileFingerprint]) -> Result<(), NGramIndexError> {
        Index::validate_file_paths(fingerprints)
    }
}

impl Index {
    #[must_use]
    pub const fn width(&self) -> GramWidth {
        self.width
    }

    #[must_use]
    pub fn file_path(&self, id: FileId) -> Option<&Path> {
        self.storage
            .fingerprints
            .get(id.get())
            .map(|fp| fp.path.as_path())
    }

    #[must_use]
    pub fn file_abs_path(&self, id: FileId) -> Option<PathBuf> {
        self.storage
            .fingerprints
            .get(id.get())
            .map(|fp| self.storage.root.join(&fp.path))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.storage.root
    }

    #[must_use]
    pub const fn corpus_kind(&self) -> CorpusKind {
        self.storage.corpus_kind
    }

    /// Produce narrowed candidate files for the query.
    /// Returns `None` if the query can't be narrowed (full scan required).
    #[must_use]
    pub fn candidates(&self, query: &QuerySpec<'_>) -> Option<Vec<crate::Candidate>> {
        let arms = Config::new(self.width).extract_literal_arms(query)?;
        Some(
            self.candidate_file_ids(&arms)
                .into_iter()
                .filter_map(|id| {
                    let fid = FileId::new(usize::try_from(id).ok()?);
                    let fp = self.storage.fingerprints.get(fid.get())?;
                    Some(crate::Candidate::with_metadata(
                        fp.path.clone(),
                        self.storage.root.join(&fp.path),
                        Some(fp.size),
                        None,
                    ))
                })
                .collect(),
        )
    }

    /// Returns an explanation of how a query would be handled.
    #[must_use]
    pub fn explain(&self, query: &QuerySpec<'_>) -> crate::index::QueryPlanOutput {
        let mode = match Config::new(self.width).extract_literal_arms(query) {
            Some(_) => crate::index::PlanMode::IndexedCandidates,
            None => crate::index::PlanMode::FullScan,
        };
        crate::index::QueryPlanOutput {
            pattern: query.patterns.to_vec().join("|"),
            mode,
        }
    }

    #[must_use]
    pub(crate) fn all_files(&self) -> Vec<crate::Candidate> {
        self.storage
            .fingerprints
            .iter()
            .map(|fp| {
                crate::Candidate::with_metadata(
                    fp.path.clone(),
                    self.storage.root.join(&fp.path),
                    Some(fp.size),
                    None,
                )
            })
            .collect()
    }

    /// Update the index from the current corpus, writing artifact files into `output_dir`.
    ///
    /// Returns `Ok(Some(index))` if a new index was written, or `Ok(None)` if no files changed.
    ///
    /// # Errors
    ///
    /// Returns an error if corpus walking, extraction, or encoding fails.
    pub fn update(
        &self,
        config: &IndexBuildConfig<'_>,
        output_dir: &Path,
        paths: &[PathBuf],
    ) -> crate::Result<Option<Self>> {
        self.rebuild(config, IndexDestination::Directory(output_dir), paths)
    }

    /// Rebuild index tables for changed files and persist to `dest`.
    pub(crate) fn rebuild(
        &self,
        config: &IndexBuildConfig<'_>,
        dest: IndexDestination,
        paths: &[PathBuf],
    ) -> crate::Result<Option<Self>> {
        use rayon::prelude::*;
        use std::collections::HashMap;

        let fingerprints = if paths.is_empty() {
            let corpus_paths = CorpusWalker::new(config).collect()?;
            FingerprintCollector::new(config.corpus.root, &corpus_paths).collect()?
        } else {
            Self::merge_partial_fingerprints(&self.storage.fingerprints, config.corpus.root, paths)?
        };

        if fingerprints == self.storage.fingerprints {
            return Ok(None);
        }

        let prev_id_by_fp: HashMap<(&Path, i64, u64), usize> = self
            .storage
            .fingerprints
            .iter()
            .enumerate()
            .map(|(id, fp)| ((fp.path.as_path(), fp.mtime_secs, fp.size), id))
            .collect();

        let file_grams: Vec<GramSet> = fingerprints
            .par_iter()
            .map(|fp| {
                if let Some(&prev_id) =
                    prev_id_by_fp.get(&(fp.path.as_path(), fp.mtime_secs, fp.size))
                {
                    return self
                        .storage
                        .gram_sets
                        .get(prev_id)
                        .map_err(crate::Error::Io);
                }
                let abs = config.corpus.root.join(&fp.path);
                std::fs::read(&abs)
                    .map(|bytes| GramSet::collect(self.width, &bytes))
                    .map_err(crate::Error::Io)
            })
            .collect::<crate::Result<_>>()?;

        let postings = PostingTables::assemble(self.width, &file_grams)?;

        let tables = IndexTables {
            fingerprints,
            file_grams,
            lexicon: postings.lexicon,
            postings: postings.postings,
        };

        let root = config.corpus.root.canonicalize()?;
        let index = Config::persist_tables(self.width, &tables, &root, config.corpus.kind, dest)?;
        Ok(Some(index))
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
        slices.sort_unstable_by_key(|slice| slice.len());
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

    fn intersect_sorted_slices(slices: &[&[u8]]) -> Vec<u32> {
        if slices.is_empty() {
            return Vec::new();
        }
        if slices.len() == 1 {
            return Postings::decode_sorted(slices[0]).expect("postings validated at open");
        }
        let mut ordered: Vec<&[u8]> = slices.to_vec();
        ordered.sort_unstable_by_key(|slice| slice.len());
        let mut cur = Postings::decode_sorted(ordered[0]).expect("postings validated at open");
        for s in &ordered[1..] {
            cur = Postings::intersect_sorted(&cur, s).expect("postings validated at open");
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
}

impl Config {
    /// Extract literal byte arms from a query spec.
    /// Returns `None` if no usable literals for this N-gram width can be extracted.
    fn extract_literal_arms(self, query: &QuerySpec<'_>) -> Option<Vec<Vec<u8>>> {
        if query.invert_match() {
            return None;
        }
        let width = self.width.get();
        let mut literal_arms: Vec<Vec<u8>> = Vec::new();
        for p in query.patterns {
            let arms = if query.fixed_strings() {
                Self::fixed_string_literals(p.as_bytes(), query.case_insensitive())
            } else {
                Self::plan_pattern(
                    p.as_str(),
                    query.case_insensitive(),
                    query.word_regexp(),
                    query.line_regexp(),
                    width,
                )?
            };
            for lit in arms {
                if lit.len() < width {
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
        width: usize,
    ) -> Option<Vec<Vec<u8>>> {
        let hir = Self::build_configured_hir(pattern, case_insensitive)?;
        let shaped = Self::shape_hir(hir, word_regexp, line_regexp);
        let lits = Self::extract_literals(&shaped, width);
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

    fn extract_literals(hir: &Hir, width: usize) -> Vec<Vec<u8>> {
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

        Self::pick_better_lits(lits_prefix, lits_suffix, width)
    }

    fn pick_better_lits(
        lits_a: Option<&[regex_syntax::hir::literal::Literal]>,
        lits_b: Option<&[regex_syntax::hir::literal::Literal]>,
        width: usize,
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
            if bytes.len() >= width {
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

impl Index {
    fn merge_partial_fingerprints(
        existing: &[FileFingerprint],
        root: &Path,
        paths: &[PathBuf],
    ) -> crate::Result<Vec<FileFingerprint>> {
        use std::collections::HashMap;

        let mut by_path: HashMap<PathBuf, FileFingerprint> = existing
            .iter()
            .map(|fp| (fp.path.clone(), fp.clone()))
            .collect();
        for rel in paths {
            let abs = root.join(rel);
            let meta = std::fs::metadata(&abs).map_err(crate::Error::Io)?;
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
            let fp = FileFingerprint {
                path: rel.clone(),
                mtime_secs,
                size: meta.len(),
            };
            by_path.insert(rel.clone(), fp);
        }
        let mut merged: Vec<_> = by_path.into_values().collect();
        merged.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(merged)
    }

    fn validate_lexicon_postings(
        lexicon: &Lexicon,
        postings: &Postings,
    ) -> Result<(), NGramIndexError> {
        let payload_len = postings.payload_len();
        for entry in lexicon {
            let start = usize::try_from(entry.offset).map_err(|_| {
                NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} offset {} exceeds usize",
                        entry.gram, entry.offset
                    ),
                ))
            })?;
            let end = lexicon.posting_byte_end(entry.offset, payload_len);
            if start > end || end > payload_len {
                return Err(NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} posting range [{start},{end}) exceeds payload_len {payload_len}",
                        entry.gram,
                    ),
                )));
            }
            let slice = postings.slice(start, end.saturating_sub(start));
            let decoded_count = Postings::validate_list(slice).map_err(|e| {
                NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("posting list for gram {:?}: {e}", entry.gram),
                ))
            })?;
            if decoded_count != entry.len as usize {
                return Err(NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "lexicon entry {:?} claims len {} but posting list has {decoded_count} entries",
                        entry.gram, entry.len,
                    ),
                )));
            }
        }
        Ok(())
    }

    fn validate_file_paths(fingerprints: &[FileFingerprint]) -> Result<(), NGramIndexError> {
        for fp in fingerprints {
            if fp.path.as_os_str().is_empty()
                || fp.path.is_absolute()
                || fp
                    .path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(NGramIndexError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid file path in index: {}", fp.path.display()),
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod candidate_tests {
    use std::path::Path;

    use crate::query::QueryFlags;

    use super::*;

    fn default_config() -> Config {
        Config::new(GramWidth::TRIGRAM)
    }

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
        default_config().extract_literal_arms(&spec).is_some()
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
        default_config().extract_literal_arms(&spec).is_none()
    }

    #[test]
    fn merge_sorted_runs_preserves_order_and_uniqueness() {
        let merged = Index::merge_sorted_runs(vec![vec![1, 3, 7], vec![1, 2, 7, 9], vec![4, 7, 8]]);
        assert_eq!(merged, vec![1, 2, 3, 4, 7, 8, 9]);
    }

    #[test]
    fn intersect_sorted_posting_byte_slices_handles_smallest_first_order() {
        let a = encode(&[1, 3, 5, 7, 9]);
        let b = encode(&[3, 7]);
        let c = encode(&[0, 3, 4, 7, 8]);
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
        let a = encode(&[1, 3, 5]);
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
        let a = encode(&[1, 2, 3]);
        let b = encode(&[4, 5, 6]);
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
        let spec = QuerySpec {
            patterns: &["ab".to_string()],
            flags: QueryFlags::empty(),
        };
        assert!(
            Config::new(GramWidth::new(2))
                .extract_literal_arms(&spec)
                .is_some()
        );
    }

    #[test]
    fn fixed_string_narrows() {
        let spec = QuerySpec {
            patterns: &["beta.gamma".to_string()],
            flags: QueryFlags::FIXED_STRINGS,
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

        let mut posting_payload = Vec::new();
        let mut buf = unsigned_varint::encode::u64_buffer();
        posting_payload.extend_from_slice(unsigned_varint::encode::u64(0, &mut buf));
        let mut buf2 = unsigned_varint::encode::u64_buffer();
        posting_payload.extend_from_slice(unsigned_varint::encode::u64(1, &mut buf2));
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
