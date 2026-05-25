# AGENTS.md -- search/

Regex execution: pattern compilation, file scanning, output formatting, and parallelism.

## Key Types

- `SearchQuery`: compiled regex + options + cached matcher/searcher. Reuse across queries.
- `SearchOptions`: flags, case mode, max results, context lines.
- `SearchMode`: output mode enum (standard, count, files-with-matches, etc.).
- `Match`: single search hit (file, line number, matched text).

## Conventions

- Filtering is done by `CandidateFilter`, applied in the `grep` orchestration layer.
- Parallel scanning uses Rayon with `par_iter`/`map_init`.
- Results are always sorted by `(file, line, text)` for determinism.
- `grep_regex`/`grep_searcher` are the underlying line-scanning engines.
- `search/` receives already-prepared `Vec<Candidate>`: no candidate resolution logic.

## Search Flow (orchestrated by `grep::run`)

```text
grep::run(query, GrepRequest{ indexes, filter, output, separators, collect_stats })
  -> QuerySpec from query.spec()
  -> candidates from Indexes::candidates(spec, coverage) or walk::collect_candidates
  -> candidate.matches(filter) via par_iter
  -> SearchExecution { candidates, output, separators, collect_stats }
  -> query.search(SearchExecution)
  -> scan with regex engine
  -> emit output
```

## Do NOT

- Be aware of index internals. Callers provide candidates.
- Break deterministic output ordering.
- Bypass the matcher/searcher cache in `SearchQuery`.
- Import from `crate::index::trigram`. Use `SearchIndex` trait only.
