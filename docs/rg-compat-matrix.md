# rg Compatibility Matrix

This tracks `sift` against `rg` with **no backward compatibility**: when a sift flag
conflicts with ripgrep, `rg` wins. The current target is full parity for
**non-engine** behavior; engine-specific features such as `-P` / PCRE2 remain
explicitly deferred.

Use the local `ripgrep/` clone as the source of truth when updating a row, and
back each implemented row with Rust tests that run `rg` and `sift` side by side.

| Area | `rg` reference | `sift` status | Notes / next test |
|---|---|---|---|
| `-L` / `--follow` | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/misc.rs` (`symlink_follow`) | Partial | Flag semantics now match `rg`; add rg-backed traversal/path-output parity tests. |
| `--files-without-match` | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/misc.rs` (`files_without_match`) | Partial | Long flag only, no `-L`; first rg-backed golden test should cover walk and index. |
| Default path printing | `ripgrep/crates/printer/src/standard.rs`, `ripgrep/tests/misc.rs` | Partial | Current `sift` still differs from `rg` in relative vs absolute output in some modes. |
| `--heading` / `--no-heading` | `ripgrep/crates/printer/src/standard.rs`, `ripgrep/tests/misc.rs` (`with_heading`) | Missing | Needs CLI flags plus printer behavior. |
| Ignore / git defaults | `ripgrep/crates/ignore`, `ripgrep/crates/core/flags/defs.rs` | Partial | Hidden and glob basics exist; no full `--no-ignore*` family yet. |
| Context output (`-A/-B/-C`) | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/misc.rs` (`after_context`) | Missing | Needs shared printer semantics and regression tests. |
| Color / separators / null | `ripgrep/crates/printer/src/standard.rs`, `ripgrep/crates/printer/src/summary.rs` | Missing | Implement after baseline path/heading parity. |
| Encoding / multiline | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/json.rs`, `ripgrep/tests/multiline.rs` | Missing | In scope, but after basic output parity lands. |
| `--json`, `--vimgrep`, `--stats`, `--debug` | `ripgrep/tests/json.rs`, `ripgrep/tests/misc.rs` (`vimgrep`) | Missing | In scope for non-engine parity. |
| `-P` / PCRE2 | `ripgrep/crates/pcre2`, `ripgrep/crates/core/flags/defs.rs` | Deferred | Explicitly out of scope for the current parity phase. |
