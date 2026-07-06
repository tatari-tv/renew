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
  doc's "clyde is cargo-install-only" line is stale. **Resolved: pilot is persona** (its
  code lives here as `tatari-tv/persona-cli`); clyde is a natural fast-follow adopter.

## Phase 2: persona-cli pilot integration

Lives in `tatari-tv/persona-cli` (a separate repo). Committed there, not here.

### Design decisions
- **Canonical `build.rs` (fleet standard), not just persona's finding-#2 fix.** Owner
  directed that all fleet CLIs integrate identically. Investigation found `build.rs`
  had drifted three ways (persona: no `status.success()` guard; renew: guarded, no
  `--dirty`, 2 rerun lines; clyde: guarded, `--dirty`, 1 rerun line). Replaced persona's
  with a canonical file that (a) guards `status.success()`, (b) uses `--tags --always
  --dirty`, (c) resolves the real git dir via `git rev-parse --git-dir` /
  `--git-common-dir` so `rerun-if-changed` is correct for regular repos, workspace
  members, AND worktrees (all three prior variants hardcode `.git/HEAD`, which never
  resolves for a workspace member or worktree - clyde's rebuild-on-tag trigger is dead
  today), and (d) honors an explicit `GIT_DESCRIBE` env override (CI pinning + the
  doc's `GIT_DESCRIBE=v1.0.0` test method). Verified end-to-end in throwaway regular +
  workspace git repos and against clyde's real worktree.
- **`Update` intercepted via `if let` before `Config::load`/`OktaAuth`** (`src/main.rs`),
  both arms diverging via `std::process::exit`; the main dispatch `match` carries
  `Commands::Update(_) => unreachable!(...)` to stay exhaustive. This is the documented
  early-intercept (update needs neither Persona config nor Okta).
- **renew added as a local `path` dep** (`renew = { path = "../renew" }`), per the
  rollout: integrate against unmerged renew, prove e2e, THEN tag `renew v0.2.0` and
  repin to the tag. Not yet repinned.

### Deviations
- None from the doc's Phase 2 intent; the canonical `build.rs` is a superset of the
  doc's "fix `status.success()`" (owner-directed scope expansion for fleet uniformity).

### Tradeoffs
- Canonical `build.rs` copied verbatim vs. a shared `renew-build` helper crate.
  Chose copy-verbatim-with-a-documented-source-of-truth (the integration guide): a
  ~25-line stable file does not justify a git `[build-dependencies]` on every fleet CLI.
  Revisit only if drift recurs despite the guide.

### Open questions
- None.

### Success criteria — verified (against the real `v1.2.1` release)
- `otto ci` green in persona-cli.
- `persona update check` on the `v1.2.1` build → `persona 1.2.1-dirty (latest)`, exit 0.
- `GIT_DESCRIBE=v1.0.0 cargo build` then `persona update check` → `1.0.0 → 1.2.1
  available (released 2026-06-15)`, exit 1.
- `update check` reached its decision with no Okta interaction (early intercept).
- `persona --version` (`v1.2.1-dirty`) and renew's normalized `1.2.1` refer to the same
  release.

## Phase 3: shakedown + regression

### Design decisions
- Redirected `XDG_DATA_HOME` to a writable temp dir when exercising the binary
  in-sandbox (persona logs under `~/.local/share`, which the harness sandbox does not
  make writable; unrelated to the feature).

### Deviations / Tradeoffs / Open questions
- None.

### Success criteria — verified
- Passive notice is TTY-gated: on an artificially-old (`GIT_DESCRIBE=v1.0.0`) build, a
  real pty (`script -qefc`) shows `persona: new version 1.2.1 available (currently
  1.0.0)  → persona update install` on stderr; the same command with stderr redirected
  to a file produces zero notice lines.

## Fleet integration guide

Added `docs/integration-guide.md` (this repo): the canonical, copy-verbatim recipe
(`build.rs`, `Cargo.toml`, `cli.rs`, `main.rs` wiring, verification) so every fleet CLI
integrates identically, with persona as the reference implementation. Linked from
`README.md`. This is the artifact that makes "all CLIs integrate exactly the same"
enforceable by convention.
