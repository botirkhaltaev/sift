# AGENTS.md -- sift-cli

## Responsibility

Thin CLI binary over `sift-core`. Parses flags with clap, maps them to `SearchOptions`/`SearchMatchFlags`, and dispatches to core.

## Structure

- **`src/lib.rs`**: `main_entry`; re-exports `grep::*` and `index::daemon` for tests/benches.
- **`src/cli.rs`**: `Cli` (clap Parser), `Commands` (`Update`, `Index`), `IndexCommands` (`Build`, `Update`).
- **`src/update.rs`**: `sift update` (install script via curl).
- **`src/index/`**: `command.rs` (`index build` / `index update`), `daemon.rs` (background refresh).
- **`src/grep/`**: search path — `search`, `pattern`, `filter`, `output`, `paths`, `ignore`, `engine`.
- **`src/main.rs`**: thin binary entrypoint calling `sift_grep::main()`.
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

- Duplicate regex or search logic from `sift-core`.
- Add heavy dependencies. This crate should stay thin.
- Change flag semantics without updating integration tests.
- Use `#[allow(...)]` or `#[expect(...)]`. Fix the root cause instead. If a helper is only used on certain platforms, gate its import with `#[cfg(...)]`.
