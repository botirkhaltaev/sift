# index/

Generic index traits and concrete index implementations.

## Modules

| Module | Description |
|--------|-------------|
| [`mod.rs`](mod.rs) | `Index` trait, `CandidateSource<P>` trait, `FileId`, `CorpusKind`, `IndexMeta` |
| [`trigram/`](trigram/) | Trigram index: build, load, search, and persistence |

## API

```rust
use sift_core::{Index, CandidateSource, FileId, TrigramIndex, TrigramIndexBuilder};

// Build
let index = TrigramIndexBuilder::new(&corpus_root).with_dir(&index_dir).build()?;

// Open
let index = TrigramIndex::open(&index_dir)?;

// Use via trait
let count: usize = index.file_count();
let path = index.file_path(FileId::new(0));
```

## Future Index Kinds

```text
index/
  trigram/     — current trigram-based index
  symbol/      — future symbol table index
  suffix/      — future suffix array index
```

Each kind implements `Index` and its own `CandidateSource<SpecificPlan>`.
