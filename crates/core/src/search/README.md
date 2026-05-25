# search/

Regex execution: pattern compilation, file scanning, output formatting, and parallelism.

## Modules

| Directory | Description |
|-----------|-------------|
| [`options/`](options/) | `SearchOptions`, `SearchMatchFlags`, `CaseMode`, `BinaryMode` |
| [`pattern/`](pattern/) | `PatternCompiler` — composable regex builder |
| [`query/`](query/) | `SearchQuery`, `Match` |
| [`request/`](request/) | `SearchExecution`, `WalkOptions`, `LinkTraversal` |
| [`filter/`](filter/) | `CandidateFilter`, `CandidateFilterConfig`, ignore/type_filter |
| [`output/`](output/) | `SearchOutput`, style/mode/format/passthru, `CandidateCoverage` |
| [`candidates/`](candidates/) | Candidate resolution for indexed and walk paths |
| [`scan/`](scan/) | Text / summary / JSON scanning workers |
| [`emit/`](emit/) | Output formatting, result chunks, and stats helpers |
| [`mod.rs`](mod.rs) | Module declarations, `SearchError` aggregate, public re-exports |

## API

```rust
use sift_core::{SearchQuery, SearchOptions, CandidateFilter};

let search = SearchQuery::new(&patterns, SearchOptions::default())?;
search.run(SearchExecution { candidates: &candidates, output, separators, collect_stats: false })?;
```

`SearchQuery` compiles the regex once; repeated calls reuse the compiled matcher and searcher cache.
