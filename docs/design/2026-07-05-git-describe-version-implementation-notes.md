# Implementation Notes: renew GIT_DESCRIBE version awareness

Running record of how the implementation diverges from or interprets
`2026-07-05-git-describe-version.md`. Append-only.

## Phase 0: Prove the GIT_DESCRIBE contract on a shipped artifact

Zero production code (a verification spike), so no commit for this phase.

### Design decisions
- None. Both proofs ran exactly as the phase specified.

### Deviations
- None.

### Tradeoffs
- Downloaded the real `persona v1.2.1` linux-amd64 release tarball and executed
  `--version` directly (host arch matched, exec succeeded) rather than relying on a
  `strings` scan. The live exec is strictly stronger evidence.

### Open questions
- None.

### Evidence
- `persona v1.2.1` binary from `tatari-tv/persona-cli` release `v1.2.1`:
  `--version` prints `persona v1.2.1` (clean tag, not a dirty describe or bare SHA).
- Throwaway 2-crate workspace (`renewlike` lib exporting a macro that expands to
  `option_env!("GIT_DESCRIBE").filter(|s| !s.trim().is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))`,
  `consumer` bin with a `build.rs` emitting `GIT_DESCRIBE=v9.9.9-3-gdeadbee`):
  - with the consumer build script → prints `v9.9.9-3-gdeadbee` (consumer's value, not
    renewlike's env). Cross-crate `option_env!` resolution at the macro call site confirmed.
  - build script removed → prints `7.7.7` (consumer's `CARGO_PKG_VERSION`). Fallback confirmed.

## Phase 1: renew version normalization + macro

### Design decisions
- `strip_describe_suffix` (`src/version.rs`) — rightmost scan implemented with two
  `rsplit_once('-')` steps plus a leading `-dirty` strip, no `regex` (per the doc's
  no-new-crates constraint). Segment validation: the last segment must be `g<hex>`
  (`is_ascii_hexdigit`), the one before it all-digits (`is_ascii_digit`). Anything else
  returns the input unchanged, so a genuine prerelease passes through.
- Shared macro expansion (`src/lib.rs`) — implemented as a `#[macro_export] #[doc(hidden)]`
  helper `__renew_current_version!`, invoked as `$crate::__renew_current_version!()` from
  all five `renew!()` arms. This keeps the source-selection expression in exactly one
  place while still expanding at the consumer's call site (`env!`/`option_env!` read the
  consumer's compile env, proven in Phase 0 for both the flat and nested macro shapes).
- Macro test lives at the crate root (`src/tests.rs` via `#[cfg(test)] mod tests;` in
  `lib.rs`) because it exercises the crate-level `renew!()` macro; the `parse_current`
  matrix stays in `src/version/tests.rs` next to the function.

### Deviations
- README quick-start `main()` example rewritten from `renew!().expect(...)` +
  unconditional `notify_if_outdated()` to the per-path construction-failure handling
  (passive notice degrades to `log::debug!`; `update` maps `Err` to exit 2). The doc's
  Phase 1 scope was "document GIT_DESCRIBE semantics + empty guard," but leaving the
  aborting `.expect` example directly contradicted the new Construction-failure section,
  so the example was corrected to teach the shipped-correct pattern (living docs).

### Tradeoffs
- In-crate macro test asserts `__renew_current_version!() == env!("GIT_DESCRIBE")` rather
  than `!= CARGO_PKG_VERSION`. Reason: in a git-less build renew's own `build.rs` falls
  back so `GIT_DESCRIBE == CARGO_PKG_VERSION`, which would make a `!=` assertion flaky.
  Equality-to-`GIT_DESCRIBE` is the always-true invariant that still proves the selection
  took the `option_env!` branch (non-empty). True cross-crate GIT_DESCRIBE≠CARGO proof is
  Phase 0's spike and will be re-proven live in the pilot (Phase 2).

### Open questions
- None.

### Success criteria — verified
- `otto ci` green in renew (exit 0; 90 tests pass, fmt/clippy/bloat/lint clean).
- `parse_current` matrix asserts all required cases: `v1.2.1`/`v1.2.1-3-gabc123`/
  `v1.2.1-3-gabc123-dirty` → `1.2.1`; `1.2.1-rc.1` keeps `.pre="rc.1"`;
  `1.2.1-rc.1-3-gabc` → `1.2.1-rc.1` (rightmost scan); `abc123f`, `""`, `"   "` all err;
  error message carries the original input, not the stripped core.
- Macro test proves all five arms construct and resolve the same normalized current
  version, sourced from `GIT_DESCRIBE`.
- Negative test bit: temporarily asserting `Version::new(9,9,9)` for `v1.2.1-3-gabc123`
  failed with `left: 1.2.1 / right: 9.9.9`, then reverted; suite green again.

## Pilot note (out of phase, recorded for Phases 2-3)
- Phases 0-1 (renew library change) are pilot-agnostic. The consumer pilot for
  Phases 2-3 is under discussion: the doc names `persona-cli`; `clyde` was raised as an
  alternative and independently qualifies (single `clyde` crate in a bare-container
  worktree repo — the `main/`+`open/` dirs are two branch worktrees, not two crates —
  with `build.rs`→`GIT_DESCRIBE`, `--version = env!("GIT_DESCRIBE")`, release binaries in
  renew's suffix scheme, and a `build.rs` that already checks `status.success()`). The
  doc's "clyde is cargo-install-only" line is stale. Final pilot choice pending.
