# index/trigram/storage/

On-disk binary format for the trigram index tables. All access is zero-copy via memory-mapped files.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Module re-exports |
| [`format.rs`](format.rs) | Magic bytes (`SIFTFIL2`, `SIFTLEX1`, `SIFTPST1`) and little-endian write helpers |
| [`lexicon.rs`](lexicon.rs) | `LexiconEntry`, `MappedLexicon`: sorted trigram to postings slice descriptor |
| [`postings.rs`](postings.rs) | `MappedPostings`: contiguous `u32` LE file-ID payloads |
| [`mmap.rs`](mmap.rs) | `open_mmap`: minimal memory-map wrapper (contains the only `unsafe` in the crate) |

## Format Overview

Each table file starts with an 8-byte magic header:

| File | Magic | Contents |
|------|-------|----------|
| `files.bin` | `SIFTFIL2` | Offset table + length-prefixed UTF-8 paths |
| `lexicon.bin` | `SIFTLEX1` | Sorted trigram entries with postings offsets |
| `postings.bin` | `SIFTPST1` | Flat array of `u32` file IDs referenced by lexicon |

All integers are little-endian. Lexicon entries are sorted by trigram bytes for binary search.
