use std::collections::HashSet;
use std::path::PathBuf;

use rayon::prelude::*;

use crate::corpus::Candidate;
use crate::corpus::CandidateCoverage;
use crate::corpus::CandidateOrder;
use crate::corpus::walk::FileWalk;
use crate::IndexCoverage;

use super::plan::{ResolutionPlan, ResolutionStrategy};
use super::planner::PlanContext;
use super::QuerySpec;

/// Per-run inputs for candidate resolution after planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionFallback {
    IndexHitsOnly,
    WalkOnStaleSnapshot,
}

impl ResolutionFallback {
    #[must_use]
    pub const fn walk_on_stale(self) -> bool {
        matches!(self, Self::WalkOnStaleSnapshot)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResolutionConfig {
    pub coverage: CandidateCoverage,
    pub fallback: ResolutionFallback,
    pub order: CandidateOrder,
}

struct CandidateSet {
    candidates: Vec<Candidate>,
}

impl CandidateSet {
    const fn new(candidates: Vec<Candidate>) -> Self {
        Self { candidates }
    }

    fn retain_matches(
        mut self,
        filter: &crate::corpus::filter::CandidateFilter,
    ) -> Self {
        self.candidates = self
            .candidates
            .into_par_iter()
            .filter(|candidate| candidate.matches(filter))
            .collect();
        self
    }

    fn order(mut self, order: CandidateOrder) -> crate::Result<Self> {
        order.order(&mut self.candidates)?;
        Ok(self)
    }

    fn into_vec(self) -> Vec<Candidate> {
        self.candidates
    }
}

impl ResolutionPlan {
    fn execute(
        self,
        spec: &QuerySpec<'_>,
        ctx: PlanContext<'_>,
        config: ResolutionConfig,
    ) -> crate::Result<Vec<Candidate>> {
        let raw = match self.strategy {
            ResolutionStrategy::WalkAll => FileWalk::from_filter(ctx.filter).collect()?,
            ResolutionStrategy::AllIndexed => ctx.indexes.complete_candidates(),
            ResolutionStrategy::UseIndex => {
                Self::resolve_index_hits(spec, ctx, config.fallback)?
            }
        };
        Ok(CandidateSet::new(raw)
            .retain_matches(ctx.filter)
            .order(config.order)?
            .into_vec())
    }

    fn resolve_index_hits(
        spec: &QuerySpec<'_>,
        ctx: PlanContext<'_>,
        fallback: ResolutionFallback,
    ) -> crate::Result<Vec<Candidate>> {
        let snapshot_hits = ctx.indexes.candidates(spec).unwrap_or_default();
        let Some(meta) = ctx.store_meta else {
            return Ok(snapshot_hits);
        };

        if !meta.covers_candidate_filter(ctx.filter) {
            return FileWalk::from_filter(ctx.filter).collect();
        }

        match meta.coverage {
            IndexCoverage::Complete => {
                if fallback.walk_on_stale() {
                    FileWalk::from_filter(ctx.filter).collect()
                } else {
                    Ok(snapshot_hits)
                }
            }
            IndexCoverage::Lazy => Self::merge_unindexed(ctx, snapshot_hits),
        }
    }

    fn merge_unindexed(
        ctx: PlanContext<'_>,
        mut snapshot_hits: Vec<Candidate>,
    ) -> crate::Result<Vec<Candidate>> {
        let indexed_paths = ctx.indexes.indexed_rel_paths();
        let walked = FileWalk::from_filter(ctx.filter).collect()?;
        let mut seen: HashSet<PathBuf> = snapshot_hits
            .iter()
            .map(|c| c.rel_path().to_path_buf())
            .collect();
        for candidate in walked {
            if indexed_paths.contains(candidate.rel_path()) {
                continue;
            }
            if seen.insert(candidate.rel_path().to_path_buf()) {
                snapshot_hits.push(candidate);
            }
        }
        Ok(snapshot_hits)
    }
}

impl super::QueryPlanner<'_> {
    /// Plan and resolve candidates for a query.
    ///
    /// # Errors
    ///
    /// Returns an error if filesystem walking or ordering fails.
    pub fn resolve(
        self,
        ctx: PlanContext<'_>,
        config: ResolutionConfig,
    ) -> crate::Result<Vec<Candidate>> {
        let spec = self.spec;
        let plan = self.plan(ctx, config.coverage, config.fallback.walk_on_stale());
        plan.execute(&spec, ctx, config)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use crate::corpus::filter::{
        CandidateFilter, CandidateFilterConfig, GlobConfig, VisibilityConfig,
    };
    use crate::corpus::{Candidate, CandidateCoverage, CandidateOrder};
    use crate::index::config::{IndexBuildConfig, IndexWalkConfig};
    use crate::index::{
        CorpusKind, CorpusMeta, FilterMeta, IndexConfig, IndexCoverage, Indexes, WalkMeta,
    };
    use crate::{IndexStore, StoreMeta};
    use crate::query::{PlanContext, QueryFlags, QueryPlanner, QuerySpec};

    use super::{ResolutionConfig, ResolutionFallback};

    fn build_indexes(root: &Path, sift_dir: &Path) -> Indexes {
        let root_buf = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let meta = StoreMeta::new(
            CorpusMeta {
                root: root_buf,
                kind: CorpusKind::Directory,
                include_paths: Vec::new(),
                exclude_paths: Vec::new(),
            },
            IndexCoverage::Complete,
            WalkMeta {
                follow_links: false,
                one_file_system: false,
                max_depth: None,
                max_filesize: None,
            },
            FilterMeta {
                visibility: VisibilityConfig::default(),
            },
            vec![IndexConfig::ngram(crate::GramWidth::TRIGRAM)],
        );
        let mut store = IndexStore::open_or_create(sift_dir, &meta).expect("open store");
        store
            .build(
                &[IndexConfig::ngram(crate::GramWidth::TRIGRAM)],
                &IndexBuildConfig {
                    corpus: crate::CorpusSpec {
                        root,
                        kind: CorpusKind::Directory,
                        follow_links: false,
                        include_paths: &[],
                        exclude_paths: &[],
                    },
                    walk: IndexWalkConfig::new(false),
                    visibility: VisibilityConfig::default(),
                },
                &[],
            )
            .expect("build");
        Indexes::open(sift_dir).expect("open indexes")
    }

    fn default_filter(root: &Path) -> CandidateFilter {
        CandidateFilter::new(
            &CandidateFilterConfig {
                scopes: vec![PathBuf::from("")],
                exclude_paths: Vec::new(),
                glob: GlobConfig::default(),
                visibility: VisibilityConfig::default(),
                follow_links: false,
                max_depth: None,
                max_filesize: None,
                type_filters: Vec::new(),
                one_file_system: false,
            },
            root,
        )
        .expect("filter")
    }

    fn default_meta(root: &Path) -> StoreMeta {
        StoreMeta::new(
            CorpusMeta {
                root: root.to_path_buf(),
                kind: CorpusKind::Directory,
                include_paths: Vec::new(),
                exclude_paths: Vec::new(),
            },
            IndexCoverage::Complete,
            WalkMeta {
                follow_links: false,
                one_file_system: false,
                max_depth: None,
                max_filesize: None,
            },
            FilterMeta {
                visibility: VisibilityConfig::default(),
            },
            vec![IndexConfig::ngram(crate::GramWidth::TRIGRAM)],
        )
    }

    fn make_parity_corpus(root: &Path) {
        fs::create_dir_all(root.join("a")).expect("create dir");
        fs::create_dir_all(root.join("b")).expect("create dir");
        fs::write(root.join("a/x.txt"), "alpha beta gamma\n").expect("write");
        fs::write(root.join("b/y.txt"), "delta epsilon\n").expect("write");
    }

    fn resolve(
        indexes: &Indexes,
        filter: &CandidateFilter,
        spec: QuerySpec<'_>,
        coverage: CandidateCoverage,
        store_meta: Option<&StoreMeta>,
        fallback: ResolutionFallback,
    ) -> Vec<Candidate> {
        let plan_ctx = PlanContext::new(indexes, filter, store_meta, true);
        QueryPlanner::new(spec)
            .resolve(
                plan_ctx,
                ResolutionConfig {
                    coverage,
                    fallback,
                    order: CandidateOrder::default(),
                },
            )
            .expect("resolve")
    }

    #[test]
    fn potential_matches_narrowable_uses_index() {
        let tmp = TempDir::new().expect("tempdir");
        let corpus = tmp.path().join("corpus");
        make_parity_corpus(&corpus);
        let sift_dir = tmp.path().join(".sift");
        let indexes = build_indexes(&corpus, &sift_dir);
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::empty(),
        };
        let filter = default_filter(&corpus);
        let result = resolve(
            &indexes,
            &filter,
            spec,
            CandidateCoverage::PotentialMatches,
            None,
            ResolutionFallback::WalkOnStaleSnapshot,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rel_path(), Path::new("a/x.txt"));
    }

    #[test]
    fn stale_complete_snapshot_falls_back_to_walk() {
        let tmp = TempDir::new().expect("tempdir");
        let corpus = tmp.path().join("corpus");
        make_parity_corpus(&corpus);
        let sift_dir = tmp.path().join(".sift");
        let indexes = build_indexes(&corpus, &sift_dir);
        let spec = QuerySpec {
            patterns: &["beta".to_string()],
            flags: QueryFlags::empty(),
        };
        let filter = default_filter(&corpus);
        let meta = default_meta(&corpus);
        let result = resolve(
            &indexes,
            &filter,
            spec,
            CandidateCoverage::PotentialMatches,
            Some(&meta),
            ResolutionFallback::WalkOnStaleSnapshot,
        );
        let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![PathBuf::from("a/x.txt"), PathBuf::from("b/y.txt")]
        );
    }

    #[test]
    fn stale_complete_coverage_walks_entire_corpus() {
        let tmp = TempDir::new().expect("tempdir");
        let corpus = tmp.path().join("corpus");
        make_parity_corpus(&corpus);
        let sift_dir = tmp.path().join(".sift");
        let indexes = build_indexes(&corpus, &sift_dir);
        let spec = QuerySpec {
            patterns: &["zzz".to_string()],
            flags: QueryFlags::empty(),
        };
        let filter = default_filter(&corpus);
        let meta = default_meta(&corpus);
        let result = resolve(
            &indexes,
            &filter,
            spec,
            CandidateCoverage::Complete,
            Some(&meta),
            ResolutionFallback::WalkOnStaleSnapshot,
        );
        let mut paths: Vec<_> = result.iter().map(|c| c.rel_path().to_path_buf()).collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![PathBuf::from("a/x.txt"), PathBuf::from("b/y.txt")]
        );
    }
}
