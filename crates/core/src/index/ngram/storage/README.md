# index/ngram/storage/

On-disk binary format for N-gram index tables. All access is zero-copy via memory-mapped files.

## Modules

| File | Description |
|------|-------------|
| [`mod.rs`](mod.rs) | Module re-exports |
| [`format.rs`](format.rs) | Magic bytes (`SIFTFIL1`, `SIFTLEX2`, `SIFTPST1`, `SIFTGRM1`) |
| [`lexicon.rs`](lexicon.rs) | `LexiconEntry<G>`, `Lexicon<G>`: sorted gram to postings slice descriptor |
| [`postings.rs`](postings.rs) | `Postings`: contiguous `u32` LE file-ID payloads |
| [`grams.rs`](grams.rs) | `GramSet<G>`, `GramSets<G>`: per-file gram sets for incremental updates |

## Format Overview

Each table file starts with an 8-byte magic header:

| File | Magic | Contents |
|------|-------|----------|
| `files.bin` | `SIFTFIL1` | Offset table + length-prefixed UTF-8 paths with fingerprints (mtime, size) |
| `lexicon.bin` | `SIFTLEX2` | Width-aware sorted gram entries with postings offsets |
| `postings.bin` | `SIFTPST1` | Flat array of `u32` file IDs referenced by lexicon |
| `grams.bin` | `SIFTGRM1` | Per-file sorted unique gram sets for incremental rebuild |

All integers are little-endian. Lexicon entries are sorted by gram ordinal for binary search. Width-bearing files store the gram width in the header and reject mismatched specializations at open time.
