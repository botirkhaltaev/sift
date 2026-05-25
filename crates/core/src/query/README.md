# query/

Query description and candidate planning. Owns the logic that turns user patterns into an index-agnostic query specification.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Module declarations and public re-exports |
| [`spec.rs`](spec.rs) | `QuerySpec`: neutral query description (patterns + flags) |

## API

```rust
use sift_core::{QuerySpec, QueryFlags};

let spec = QuerySpec {
    patterns: &["beta".to_string()],
    flags: QueryFlags::empty(),
};
```

`QuerySpec` describes the query in index-agnostic terms. Each `SearchIndex` implementation decides how to use the spec to narrow candidates.
