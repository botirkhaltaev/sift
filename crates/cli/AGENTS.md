# AGENTS.md -- sift-grep (CLI crate)

## Responsibility

Thin CLI binary over `sift-core`. Parses flags with clap, maps them to `SearchOptions`/`SearchMatchFlags`, and dispatches to core.

## Architecture

Two-layer flag model:

1. **`*Decl` structs (clap)** — declare flags for help and parsing.
2. **Resolved domain types** — effective runtime values from raw argv (ripgrep last-wins ordering).

`Argv` is collected once in `main_entry`. `Cli` builds domain configs (`PatternConfig`, `FilterConfig`, `OutputConfig`, `GrepConfig`, `IndexRequest`) and an optional `Daemon` handle, then passes them to `Grep` / `Index`. Domain modules never import `Cli`. `Cli::dispatch` orchestrates only.

### Module pairing (decl → config → resolve/run)

| Module | Clap decls | Config / resolved type | Entry point |
|--------|------------|------------------------|-------------|
| `grep/argv.rs` | — | `Argv` | `Argv::from_env`, `Argv::new` |
| `grep/ignore.rs` | `Ignore*Decl`, … | `IgnoreResolution` | `IgnoreResolution::resolve` |
| `grep/pattern.rs` | `PatternArgs`, … | `PatternConfig`, `PatternArgv`, `ResolvedPatterns` | `ResolvedPatterns::resolve`, `PatternConfig::search_options` |
| `grep/output.rs` | `LineNumberDecl`, … | `OutputConfig`, `OutputArgv`, `SearchOutputCtx` | `OutputArgv::resolve`, `SearchOutputCtx::resolve`, `OutputConfig::separators` |
| `grep/filter.rs` | `FilterDecl`, … | `FilterConfig`, `TypeCatalog`, `SearchFilterCtx` | `FilterConfig::candidate_config`, `SearchFilterCtx::resolve` |
| `grep/paths.rs` | `PathArgs` | `CorpusScope` | `CorpusScope::resolve` |
| `grep/run.rs` | — | `GrepConfig`, `Grep`, `GrepOutcome` | `Grep::run` |
| `index/mod.rs` | — | `IndexRequest`, `IndexJob` | `IndexJob::resolve`, `IndexJob::run` |
| `index/daemon/mod.rs` | — | `Daemon`, `ServeConfig`, `DaemonError` | `Daemon::index`, `Daemon::ensure_running`, `Daemon::serve` |

## Structure

- **`src/lib.rs`**: `main_entry`; re-exports `grep::*` and `index::daemon` for tests/benches.
- **`src/cli.rs`**: `Cli` parser, config builders, `Cli::dispatch`.
- **`src/update.rs`**: `sift update` (install script via curl).
- **`src/index/`**: `IndexJob` / `IndexRequest` (build & update), `index/daemon/mod.rs` (IPC, spawn, serve loop).
- **`src/grep/`**: search domain — `argv`, `run`, `pattern`, `filter`, `output`, `paths`, `ignore`, `engine`.
- **`src/main.rs`**: thin binary entrypoint calling `sift_grep::main_entry()`.
- **`tests/common/mod.rs`**: shared test helpers: `TestProject`, assertion helpers, path normalization.
- **`tests/integration_*.rs`**: domain-focused integration tests spawning the real `sift` binary.

## Test Helpers (`tests/common/mod.rs`)

### TestProject
```rust
let p = TestProject::new("my-test");
p.write("a.txt", "content\n");
p.build_index();                   // index "." from project root
let out = p.index_output(["pattern"]);  // search with index
let out = p.walk_output(["pattern"]);   // search without index
p.assert_index_walk_same(["pattern"], "expected\n");
```

### Assertions
- `assert_success(out)`: exit 0 with rich failure message
- `assert_exit_code(out, n)`: specific exit code
- `assert_stdout_eq(out, expected)`: exact stdout match
- `assert_stdout_contains(out, substr)` / `assert_stdout_not_contains`
- `assert_stderr_empty(out)`
- `normalize_stdout(out)` / `normalize_stderr(out)`: cross-platform path/line-end normalization

### Path Helpers
- `rel_match(rel, rest)`: `"file.txt:content"`
- `abs_path(root, rel)`: canonicalized absolute path

## Behavior Notes

- Global options (e.g. `--sift-dir`) must appear **before** `index` subcommands.
- Search paths are resolved and must sit under the corpus root in the index metadata.
- Extend flags by threading new `SearchMatchFlags`/`SearchOptions` fields through to `SearchQuery::new` in core. Do not duplicate regex logic here.

## Testing

```bash
cargo test -p sift-grep
cargo build --release -p sift-grep
```

## Do NOT

- Add `resolve_*_from_args` free functions — use domain type `resolve` methods on `Argv`.
- Duplicate builders (`build_*_config` free fn + `Cli` method).
- Scatter argv scanning across `impl Cli` blocks in multiple files.
- Import `Cli` from domain modules — build configs in `cli.rs` and pass them in.
- Add `default_*()` helpers for test/bench fixtures — implement `Default` on the domain type instead; override fields with struct update (`Type { field: val, ..Default::default() }`).
- Create `helpers.rs`, `utils.rs`, or parallel `*_with_*` variants.
- Add one-line wrapper functions that only delegate to another call or re-wrap an error without adding domain logic — call the underlying API directly.
- Duplicate regex or search logic from `sift-core`.
- Add heavy dependencies. This crate should stay thin.
- Change flag semantics without updating integration tests.
- Use `#[allow(...)]` or `#[expect(...)]`. Fix the root cause instead. If a helper is only used on certain platforms, gate its import with `#[cfg(...)]`.
