# search/

Search execution engine — compiles patterns, scans files, applies filters, and formats output.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Re-exports for the search subsystem |
| [`types.rs`](types.rs) | `CompiledSearch`, `SearchOptions`, `SearchMatchFlags`, `SearchMode`, `Match`, output config types |
| [`execute.rs`](execute.rs) | `run_index`, `search_index`, `walk_file_paths` — parallel candidate scanning, output writing, stats |
| [`filter.rs`](filter.rs) | `SearchFilter`, `SearchFilterConfig` — glob, hidden-file, ignore-rule, and scope filtering |
| [`matcher.rs`](matcher.rs) | `build_matcher`, `build_searcher` — `grep_regex`/`grep_searcher` integration |

## Key Types

- **`CompiledSearch`** — compiled regex + options + cached matcher/searcher. Thread-safe for repeated queries.
- **`SearchOptions`** — flags (`-F`, `-w`, `-x`, `-v`, `-o`), case mode, max results, context lines.
- **`SearchFilter`** — applies glob include/exclude, hidden-file policy, `.gitignore` rules, and path scoping at search time (not index time).
- **`SearchMode`** — `Standard`, `Count`, `CountMatches`, `FilesWithMatches`, `FilesWithoutMatch`, `OnlyMatching`.
- **`Match`** — single hit: file path, line number, matched text.

## Parallelism

Candidate scanning parallelizes via Rayon when the candidate count exceeds `parallel_candidate_threshold()`. Results are merged sorted by `(file, line, text)` for deterministic output.
