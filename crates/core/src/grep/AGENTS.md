# grep/

Grep-style search execution: pattern compilation, file scanning, filtering, output formatting, and parallelism.

## Key Types

- `CompiledSearch` — compiled regex + options + cached matcher/searcher. Reuse across queries.
- `SearchOptions` — flags, case mode, max results, context lines.
- `SearchFilter` — search-time filtering (globs, hidden files, ignore rules, path scoping).
- `SearchMode` — output mode enum (standard, count, files-with-matches, etc.).
- `Match` — single search hit (file, line number, matched text).

## Conventions

- All filtering happens at search time, not index time.
- Parallel scanning uses Rayon with a threshold check (`parallel_candidate_threshold`).
- Results are always sorted by `(file, line, text)` for determinism.
- `grep_regex`/`grep_searcher` are the underlying line-scanning engines.

## Do NOT

- Apply filtering logic at index build time.
- Break deterministic output ordering.
- Bypass the matcher/searcher cache in `CompiledSearch`.
- Import from `crate::index::trigram` — use `crate::index::Index` and `crate::index::CandidateSource` traits only.
