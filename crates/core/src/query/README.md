# query/

Query description and candidate planning. Turns user patterns into an index-agnostic query specification that any index kind can consume.

## Design

The query layer is deliberately independent of any index implementation. `QuerySpec` describes what the user is searching for (patterns + flags); each index kind decides how to use that specification to narrow candidates. The `QueryPlanner` orchestrates candidate resolution by consulting the `Indexes` registry and falling back to a filesystem walk when no index can narrow.

This separation means adding new index types requires no changes to the query layer. As the planner evolves to support richer query planning (choosing among index types, estimating costs, composing strategies), it will remain the single coordination point between the search pipeline and the index registry.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Module declarations and public re-exports |
| [`spec.rs`](spec.rs) | `QuerySpec`: neutral query description (patterns + flags) |
| [`planner.rs`](planner.rs) | `QueryPlanner`: candidate resolution via indexes or walk fallback |

## API

```rust
use sift_core::{QuerySpec, QueryFlags, QueryPlanner, CandidateRequirement};

// Describe a query
let spec = QuerySpec {
    patterns: &["pattern".to_string()],
    flags: QueryFlags::empty(),
};

// Plan candidate resolution
let planner = QueryPlanner::new(spec);
let candidates = planner.candidates(
    &indexes, requirement, &filter, store_meta, walk_unindexed, || base(),
)?;
```

`QuerySpec` is index-agnostic. Each index implementation decides how to interpret the spec for candidate narrowing.
