# grep/

Grep-style search execution built on the public grep crates (`grep_matcher`, `grep_regex`, `grep_searcher`, `grep_printer`).

## Modules

| Directory | Description |
|-----------|-------------|
| [`options/`](options/) | `SearchOptions`, `SearchMatchFlags`, `CaseMode`, `BinaryMode` |
| [`pattern/`](pattern/) | `PatternCompiler` — composable regex builder |
| [`query/`](query/) | `SearchQuery`, `Match` |
| [`request/`](request/) | `SearchRequest`, `WalkOptions`, `LinkTraversal` |
| [`filter/`](filter/) | `SearchFilter`, `CandidateInfo`, config/ignore/type_filter |
| [`output/`](output/) | `SearchOutput`, style/mode/format/passthru |
| [`candidates/`](candidates/) | Candidate resolution for indexed and walk paths |
| [`scan/`](scan/) | Text / summary / JSON scanning workers |
| [`emit/`](emit/) | Output formatting, result chunks, and stats helpers |
| [`mod.rs`](mod.rs) | Module declarations, `SearchError` aggregate, public re-exports |

## API

```rust
use sift_core::{SearchQuery, SearchRequest, SearchOptions, SearchFilter, SearchOutput};

let search = SearchQuery::new(&patterns, SearchOptions::default())?;
search.run(SearchRequest { indexes: &indexes, filter: &filter, output, separators: &separators, collect_stats: false })?;
```

`SearchQuery` compiles the regex once; repeated calls reuse the compiled matcher and searcher cache.
