# AGENTS.md -- search/

## Responsibility

Regex execution: pattern compilation, file scanning, output formatting, and parallelism. Receives already-resolved candidates and knows nothing about which index types produced them.

## Key Types

- `SearchQuery`: compiled regex + options + cached matcher/searcher. Reuse across queries.
- `SearchOptions`: flags, case mode, max results, context lines.
- `SearchMode`: output mode enum (standard, count, files-with-matches, etc.).
- `Match`: single search hit (file, line number, matched text).

## Design

The search layer is the final pipeline stage. By the time code reaches here, candidates have been narrowed by the index registry (trigram, or any future index kind) and filtered by the candidate filter. This strict separation means the search layer works identically regardless of which index types produced the candidates.

## Conventions

- Filtering is done by `CandidateFilter`, applied in the `grep` orchestration layer.
- Parallel scanning uses Rayon with `par_iter`/`map_init`.
- Results are always sorted by `(file, line, text)` for determinism.
- `grep_regex`/`grep_searcher` are the underlying line-scanning engines.
- `search/` receives already-prepared `Vec<Candidate>`: no candidate resolution logic.

## Search Flow (orchestrated by `grep::run`)

```text
grep::run(query, GrepRequest{ indexes, filter, output, separators, collect })
  -> QuerySpec from `SearchQuery::build_query_spec()` (internal)
  -> candidates from Indexes::candidates(spec, coverage) or walk::collect_candidates
  -> candidate.matches(filter) via par_iter
  -> SearchExecution { candidates, output, separators, collect }
  -> query.search(SearchExecution)
  -> scan with regex engine
  -> emit output
```

## Do NOT

- Be aware of index internals. Callers provide candidates.
- Break deterministic output ordering.
- Bypass the matcher/searcher cache in `SearchQuery`.
- Import from `crate::index::trigram` or any concrete index module. Use `Index` enum only.
