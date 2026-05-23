# grep/

Grep-style search execution built on the public grep crates (`grep_matcher`, `grep_regex`, `grep_searcher`, `grep_printer`).

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Module declarations and public re-exports |
| [`types.rs`](types.rs) | `CompiledSearch`, `SearchOptions`, output config, result types |
| [`filter.rs`](filter.rs) | Path, glob, ignore, hidden, type filtering; `CandidateInfo` |
| [`matcher.rs`](matcher.rs) | Regex matcher/searcher construction and cache |
| [`execute.rs`](execute.rs) | Orchestration: `run_index`, `run_walk`, `collect_*` |
| [`candidate.rs`](candidate.rs) | (pending split) Candidate planning and preparation |
| [`output.rs`](output.rs) | (pending split) Output formatting: color, prefixes, JSON, summary |
| [`scan.rs`](scan.rs) | (pending split) Per-file scanning and sinks |
| [`stats.rs`](stats.rs) | (pending split) Stats counters and aggregation |
| [`walk.rs`](walk.rs) | (pending split) Walk-mode candidate discovery |

## API

```rust
use sift_core::{CompiledSearch, SearchOptions, SearchFilter, SearchOutput};

let search = CompiledSearch::new(&patterns, SearchOptions::default())?;
search.run_index(&index, &filter, output, &separators)?;
```

`CompiledSearch` compiles the regex once; repeated calls reuse the compiled matcher and searcher cache.
