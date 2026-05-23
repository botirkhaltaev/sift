# Profiling Findings

Profiled on macOS (ARM64) using `sample` and `samply`. Benchmarks run with `--sample-size 10`, `--warm-up-time 2`, `--measurement-time 5`.

## Benchmark Timings (8k file monorepo corpus)

| Benchmark | Time | Notes |
|-----------|------|-------|
| `index_build/monorepo` | ~2.0s | Corpus materialization + index build + save |
| `index_build/many_tiny_files` (1k) | ~150ms | 1k small files |
| `index_open/large` | ~1.2ms | mmap-based reopen |
| `index_candidates/literal` | ~21µs | Trigram lookup only |
| `grep_indexed/literal` | ~1.8ms | Indexed search, 1 match file |
| `grep_indexed/alternation` | ~2.0ms | 4-term alternation |
| `grep_indexed/full_scan_fallback` | ~83ms | No trigrams, scans all 8k files |
| `grep_walk/literal` | ~21ms | Walk search, no index |

## Hot Spots by Phase

### Index Build (`index_build/monorepo`, ~2.0s)

**Top functions by sample count:**

| Function | Samples | % | Category |
|----------|---------|---|----------|
| `materialize_large_corpus` (fixture) | ~2400 | 51% | I/O - file creation |
| `HashMap::insert` (hashbrown) | 725 | 15% | Hash map insertion |
| `BuildHasher::hash_one` | 506 | 11% | SipHash hashing |
| `SipHasher::write` | 471 | 10% | Hash computation |
| `from_utf8` | 439 | 9% | UTF-8 validation |
| `TrigramIndexBuilder::build` | 167 | 4% | Index construction |
| `small_sort_network` | 96 | 2% | Sorting |
| `quicksort` | 69 | 1.5% | Sorting |
| `extract_unique_trigrams_from_bytes` | 49 | 1% | Trigram extraction |

**Breakdown:**
- **~51% fixture setup**: Creating 8k directories/files, writing content (open, write, mkdir, close syscalls)
- **~36% trigram extraction**: HashMap insertions, SipHash hashing, UTF-8 validation
- **~4% sorting**: small_sort_network + quicksort for posting list ordering
- **~9% I/O persistence**: `save_to_dir` → `std::fs::write` (mmap, write, close)

**Key insight**: The fixture materialization dominates. The actual index build (trigram extraction + sorting + persistence) is ~45% of total time.

### Grep Indexed Search (`grep_indexed/literal`, ~1.8ms)

**Note**: Profile dominated by fixture setup (index rebuild per iteration). The actual search is sub-millisecond.

**Top functions (includes fixture):**
- `HashMap::insert` / `hash_one` / `SipHasher::write` — trigram index rebuild
- `from_utf8` — UTF-8 validation during corpus materialization
- `__open` / `write` / `__unlinkat` — file I/O for fixture

**Actual search path** (from call graph analysis):
- `run_indexes` → `candidates` → trigram lookup (~21µs)
- `run_indexes` → `grep_searcher` → regex scan of candidate files
- For literal queries with few candidates, search is dominated by regex engine overhead

### Grep Full Scan Fallback (`grep_indexed/full_scan_fallback`, ~83ms)

**Top functions (includes fixture):**
- Same fixture dominance as indexed search
- Additional time in `grep_searcher` scanning all 8k files

**Actual search path**:
- `run_indexes` → full file enumeration (no trigram narrowing)
- Parallel scan via Rayon of all 8k files
- Regex matching on each file

### Grep Walk Search (`grep_walk/literal`, ~21ms)

**Top functions (includes fixture):**
- Same fixture dominance
- `__getdirentries64` — directory traversal
- `compare_components` — path comparison during walk

**Actual search path**:
- `ignore::WalkBuilder` directory traversal
- Filter application (gitignore, glob, hidden)
- Parallel scan via Rayon

## Optimization Opportunities

### High Impact

1. **Fixture reuse in benches**: The current benches rebuild the index for each iteration. For profiling the actual search path, fixtures should be built once outside `b.iter`. The bench code already does this correctly — the issue is that the sampling window catches the warmup/setup phase.

2. **Hash map performance (~25% of index build)**: `hashbrown::HashMap` insertions dominate trigram extraction. The SipHash hasher is called for every trigram in every file. Consider:
   - Using a faster hasher (e.g., `ahash`, `fxhash`) for the trigram frequency map
   - Pre-allocating HashMap capacity based on expected trigram count

3. **UTF-8 validation (~9% of index build)**: `from_utf8` is called for every file path. Since paths are known to be valid UTF-8 (from `ignore` crate), consider using `from_utf8_unchecked` with proper invariants.

### Medium Impact

4. **Sorting overhead (~3.5% of index build)**: `small_sort_network` + `quicksort` for posting list ordering. Consider:
   - Using radix sort for integer keys (FileId is usize)
   - Pre-sorting during extraction to avoid final sort

5. **Trigram extraction (~1% of index build)**: `extract_unique_trigrams_from_bytes` is called per file. For large files with many trigrams, this could be optimized with SIMD or a more efficient sliding window.

### Low Impact

6. **Index open (~1.2ms)**: Already fast (mmap-based). No obvious optimization needed.

7. **Candidate lookup (~21µs)**: Already fast. Dominated by HashMap lookups in the lexicon.

## Profiling Methodology

- **Tool**: macOS `sample` command (1ms sampling interval, 5s duration)
- **Timing**: Sampling started 3s after bench launch to catch measurement phase
- **Limitation**: Criterion's warmup phase completes before sampling starts, but fixture setup (corpus materialization + index build) happens inside `b.iter` for build benches and outside for search benches
- **Profiles saved**: `/tmp/sample_bench_build.txt`, `/tmp/sample_grep_literal.txt`, `/tmp/sample_grep_full_scan.txt`, `/tmp/sample_grep_walk.txt`

## Recommendations for Future Profiling

1. Use `samply` with `--unstable-presymbolicate` for better symbol resolution
2. Profile with `cargo bench --profile-time` for Criterion-integrated profiling
3. For search benches, verify fixtures are built outside `b.iter` (they are)
4. Consider `iai-callgrind` for instruction-count-based profiling (CI-friendly)
5. Profile CLI benches (`cargo bench -p sift-cli --bench cli`) for end-to-end hot paths
