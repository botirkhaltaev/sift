# grep/

Grep-style search execution built on the public grep crates (`grep_matcher`, `grep_regex`, `grep_searcher`, `grep_printer`).

## Modules

| Directory | Description |
|-----------|-------------|
| [`options/`](options/) | `SearchOptions`, `SearchMatchFlags`, `CaseMode`, `BinaryMode` |
| [`search/`](search/) | `CompiledSearch`, `Match` |
| [`compile/`](compile/) | `PatternCompiler` — composable regex builder |
| [`matcher/`](matcher/) | Regex matcher/searcher construction and cache |
| [`filter/`](filter/) | `SearchFilter`, `CandidateInfo`, config/ignore/type_filter |
| [`output/`](output/) | `SearchOutput`, style/mode/format/passthru |
| [`execution/`](execution/) | `run_indexes`, `run_walk`, workers, sinks, stats |
| [`mod.rs`](mod.rs) | Module declarations, `SearchError` aggregate, public re-exports |

## API

```rust
use sift_core::{CompiledSearch, SearchOptions, SearchFilter, SearchOutput};

let search = CompiledSearch::new(&patterns, SearchOptions::default())?;
search.run_indexes(&indexes, SearchExecution { filter, output, separators, stats: None })?;
```

`CompiledSearch` compiles the regex once; repeated calls reuse the compiled matcher and searcher cache.
