# query/

Query description and candidate planning. Owns the logic that turns user patterns into an index-agnostic candidate plan.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Module declarations and public re-exports |
| [`spec.rs`](spec.rs) | `QuerySpec` — neutral query description |
| [`planner.rs`](planner.rs) | `QueryPlanner` — produces `CandidatePlan` |
| [`candidate_plan.rs`](candidate_plan.rs) | `CandidatePlan`, `TrigramCandidatePlan`, `Arm` |
| [`trigram.rs`](trigram.rs) | Raw trigram extraction utilities |

## API

```rust
use sift_core::{QuerySpec, QueryPlanner, CandidatePlan};

let spec = QuerySpec {
    patterns: &["beta".to_string()],
    fixed_strings: false,
    case_insensitive: false,
    word_regexp: false,
    line_regexp: false,
    invert_match: false,
};

let plan = QueryPlanner::plan(&spec);
match plan {
    CandidatePlan::FullScan => { /* scan all files */ }
    CandidatePlan::Trigram(p) => { /* narrow by trigram arms */ }
}
```
