# rg Compatibility Matrix

This tracks `sift` against `rg` with **no backward compatibility**: when a sift flag
conflicts with ripgrep, `rg` wins. The current target is full parity for
**non-engine** behavior; engine-specific features such as `-P` / PCRE2 remain
explicitly deferred.

Use the local `ripgrep/` clone as the source of truth when updating a row, and
back each implemented row with `sift`-only golden tests. Do not spawn `rg` at runtime.

| Area | `rg` reference | `sift` status | Notes / next test |
|---|---|---|---|
| `-L` / `--follow` | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/misc.rs` (`symlink_follow`) | Implemented | Traversal matches `rg`; add path-output tests with absolute/relative scopes. |
| `--files-without-match` | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/misc.rs` (`files_without_match`) | Implemented | Long flag only, no `-L`; output matches `rg` for relative and absolute scopes. |
| Default path printing | `ripgrep/crates/printer/src/standard.rs`, `ripgrep/tests/misc.rs` | Implemented | Respects user scopes: absolute scopes produce absolute paths, relative scopes produce relative paths. Unified through `PathDisplay` in core. |
| `--heading` / `--no-heading` | `ripgrep/crates/printer/src/standard.rs`, `ripgrep/tests/misc.rs` (`with_heading`) | Implemented | See `integration_output.rs`. |
| Ignore / git defaults | `ripgrep/crates/ignore`, `ripgrep/crates/core/flags/defs.rs` | Partial | Hidden and glob basics exist; no full `--no-ignore*` family yet. |
| Context output (`-A/-B/-C`) | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/misc.rs` (`after_context`) | Implemented | See `integration_context.rs`. |
| Color / separators / null | `ripgrep/crates/printer/src/standard.rs`, `ripgrep/crates/printer/src/summary.rs` | Implemented | `--color`, `-0` / `--null`; see `integration_null_color.rs`. |
| `--stats` | `ripgrep/crates/printer/stats.rs` (approx.) | Partial | Match tally, files contained matches, files searched, bytes printed, bytes searched (metadata sum), elapsed on stderr; no "matched lines" / split timing like rg; parity vs rg totals not guaranteed. See `integration_stats.rs`. |
| Encoding / multiline | `ripgrep/crates/core/flags/defs.rs`, `ripgrep/tests/json.rs`, `ripgrep/tests/multiline.rs` | Missing | In scope, but after basic output parity lands. |
| `--json` | `ripgrep/tests/json.rs`, `grep-printer` JSON | Implemented | JSON Lines (`begin` / `match` / `context` / `end` / `summary`); `--json` implies stderr stats like `rg`. See `integration_json.rs`. |
| `--vimgrep`, `--debug` | `ripgrep/tests/misc.rs` (`vimgrep`) | Missing | In scope for non-engine parity. |
| `-P` / PCRE2 | `ripgrep/crates/pcre2`, `ripgrep/crates/core/flags/defs.rs` | Deferred | Explicitly out of scope for the current parity phase. |

## Implementation Notes

- Path display is resolved in CLI based on user-provided scopes only.
- Core receives resolved `PathDisplay` through `SearchLineStyle`.
- All output paths (standard, heading, summary) use `display_path_for_candidate()`.
- No runtime `rg` dependency in tests; use `ripgrep/` clone as manual reference only.
