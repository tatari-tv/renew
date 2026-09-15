# Implementation Notes: valet default, MCP key ownership, update reporting

Design doc: `slack-cli docs/design/2026-09-14-valet-default-mcp-key-update-reporting.md`
(this repo implements the `renew` work stream, W3).

## Phase 1 (W3): A failed check is an error

### Design decisions

- API base injected at BOTH layers (`src/github.rs:latest_release` takes `api_base: &str`,
  and `Renew` carries `api_base: String` defaulted to `github::GITHUB_API_BASE` in
  `src/renew.rs:Renew::new`): all three `latest_release` callers are `Renew` methods, so a
  parameter alone leaves a `Renew`-level or `UpdateCmd`-level test nothing to set.
  `GITHUB_API_BASE` went from private `const` to `pub(crate)` to serve as that default.
- `fallback_to_cache` takes a `Fallback` reason instead of nothing
  (`src/renew.rs:Fallback`, `src/renew.rs:Renew::fallback_to_cache`): the four callers that
  shared one silent-success path are fixed in the function rather than in one arm, while
  the one case that is legitimately not a failure stays distinguishable.
- Lock contention is classified by the lock error, not by the caller's intent:
  `Err(std::fs::TryLockError::WouldBlock)` means a peer IS refreshing, so the cache may
  still report; `TryLockError::Error(io)` and an unopenable lock file mean no check
  happened, so they are failures.
- A stale cache informs, never votes: `Error::CheckFailed { stale: Option<StaleCache> }`
  (`src/error.rs`) carries the cached value and its `checked-at` into the failure message,
  and the call still errors (exit 2).
- The new variant names the cause it wraps (`src/error.rs:check_failed_message`), so a
  private-repo 404 renders as `the check did not succeed and there was no usable cache: no
  release found for <repo>` instead of the bare `no release found for <repo>`, which would
  tell the user the repo has no releases when the truth is their token cannot see it.
- `src/fixture.rs` (new, `#[cfg(test)]`) holds `CannedApi` (a loopback `TcpListener` on a
  thread answering one canned status), `offline_renew`, `seed_cache` and
  `hold_refresh_lock`, shared by the `renew`, `cmd` and `github` test modules. No
  dev-dependency was added: a closed port cannot produce the 404 the new variant exists
  for, and `wiremock` / `httpmock` / `mockito` are exactly what this phase must not grow.

### Deviations

- `refresh_and_compare(force: bool)` lost its parameter. Its only use was the
  `Err(_) if !force` lock arm, and once contention is classified by `TryLockError` the
  flag decided nothing; `deny(unused_variables)` would have rejected it, and a parameter
  that no longer decides anything is a name that does not tell the truth. Behavior
  consequence, stated plainly: `--refresh` while a peer holds the lock now reports from
  cache, or errors when there is nothing cached, instead of returning `Ok(None)`.
- `Renew` gained a `pub(crate) api_base` field but NO `with_api_base` builder. Every
  caller that sets it is in-crate (`src/fixture.rs`); a `pub(crate)` builder used only
  under `#[cfg(test)]` trips `deny(dead_code)` in the non-test build, and a `pub` one adds
  public API this phase does not need. Same effect, correct seam.
- `StaleCache` is a new PUBLIC type re-exported from `src/lib.rs`, required because
  `Error::CheckFailed` carries it and `Error` is public. `Update` and
  `check_latest_refresh` signatures are unchanged, as specified.
- Two tests beyond the two named in the doc were tightened, because the injected API base
  made them deterministic and leaving them would have kept the same "asserts nothing real"
  shape in place. `test_run_install_already_current_without_force_exits_0` accepted
  `[0, 2]` and depended on live GitHub; it now seeds a fresh cache entry naming the
  current version and asserts exactly 0.
  `test_notify_if_outdated_does_not_panic_on_network_error` returned at the TTY gate under
  `cargo test` and therefore exercised nothing; it is now `test_notify_if_outdated_swallows_a_failed_check`, which sets
  `RENEW_FORCE_NOTIFY` under the file's existing `ENV_LOCK` and proves the passive notice
  still swallows the NEW error, which is the claim the design doc makes about consumer
  blast radius.

### Tradeoffs

- One `Error::CheckFailed` variant with `Option<StaleCache>` vs two variants
  (failed-with-cache, failed-without): one variant keeps `matches!` simple for consumers
  and matches the doc's "a new `Error` variant", at the cost of a `check_failed_message`
  helper, because `thiserror` cannot express two renderings inline.
- Lock contention with nothing cached wraps a synthesized
  `io::Error(WouldBlock, "another process is refreshing the update cache")` vs a second
  variant or an optional source: it keeps the source chain non-optional and the message
  truthful, at the cost of an error value that is constructed rather than caught.
- `CannedApi` polls a non-blocking listener every 5 ms and stops on `Drop` vs a detached
  thread parked in `accept`: a few lines over the doc's "about fifteen", and no leaked
  thread per test.

### Open questions

- `seed_cache` (`src/fixture.rs`) builds a `CacheEntry` literal. W3 Phase 2 adds
  `published-at` / `release-url` and must extend it, including the "a legacy two-key entry
  is a cache MISS" case, which has no coverage yet because Phase 1 does not touch the
  schema.
- With `force` gone from `refresh_and_compare`, the refresh / no-refresh distinction lives
  entirely in `check_latest` vs `check_latest_refresh`. W3 Phase 3 makes `--refresh`
  global; confirm that is the intended seam before wiring the flag.

## Phase 2 (W3): Stop fabricating the report

### Design decisions

- `cache::CacheEntry` gains `published_at: Option<DateTime<Utc>>` and
  `release_url: Option<String>` (`src/cache.rs`), kebab-cased on disk to `published-at` /
  `release-url` as the doc specifies. No `#[serde(default)]` was needed: serde's derive
  already treats a missing key as `None` for an `Option` field, which is what makes a
  legacy two-key file deserialize without error.
- `Renew::compare_cached` now takes the whole `&cache::CacheEntry` and returns
  `Option<Result<Option<Update>>>` (`src/renew.rs`): the outer `None` means "this entry
  cannot back a report" (either field missing), which both callers treat exactly like "no
  cache" rather than rendering partial data. This is the single seam that makes the schema
  bump self-healing: a legacy entry read at a fresh-hit TTL falls through to
  `refresh_and_compare`, which writes a full entry on success.
- The stale-fallback caller (`fallback_to_cache`'s `LockHeld` arm) applies the same
  `compare_cached` and the same "unusable = treat as absent" rule, so a legacy entry there
  lands on the existing `Error::CheckFailed { stale: None, .. }` arm rather than a new one.
- The network path's parse of GitHub's `published_at` string is now a named failure
  (`Error::InvalidPublishedAt { value, source }`, `src/error.rs`) instead of a silent
  default. It returns before the `CacheEntry` is built, so bad data is never cached either
  - a later, working check is not corrupted by what a broken one wrote.
- `render_check` (`src/cmd.rs`) is `run_check`'s match arms pulled out into a pure
  `(String, i32)` function. `run_check` itself is unchanged in behavior (same `println!`,
  same exit codes); the split exists purely to give a test something to assert against.
- `src/fixture.rs`: `seed_cache` now writes a full, usable entry (adds `published_at` /
  `release_url` so every EXISTING cache-hit test keeps reporting); `seed_cache_with` adds
  explicit control over `checked_at` / `published_at` for a test asserting a SPECIFIC
  date; `seed_legacy_cache` writes raw two-key YAML (not through `CacheEntry`, so the new
  keys are genuinely ABSENT rather than present-and-null) to cover the fleet's actual
  on-disk shape; `CannedApi::json` serves a 200 with a body so the network path can be
  driven past deserialization into an unparseable `published_at`, `CannedApi::status`
  becoming a thin wrapper over the same `respond`.

### Deviations

- Same effect, correct seam: the doc doesn't specify a return type for `compare_cached`
  beyond "uses the cached values"; making it `Option<Result<Option<Update>>>` (rather than
  panicking, defaulting, or erroring outright on a missing field) is what lets BOTH callers
  apply the "unusable = absent" rule in one place instead of duplicating a field-presence
  check at each call site.
- The network-path fix is reported as a named `Error::InvalidPublishedAt`, not specified in
  the doc's Data Model / API Design sections (those cover only `CheckFailed` from Phase 1).
  An unparseable timestamp cannot be turned into a valid `Update`/`CacheEntry` without
  inventing a date, so honesty forces a new failure variant; it deliberately does NOT route
  through `fallback_to_cache`/`CheckFailed`, mirroring the existing (pre-Phase-2) handling
  of a bad `tag_name` a few lines above it, which also returns `Err(e)` directly rather than
  offering stale-cache context.
- Criterion 2 ("a test feeds a cache entry with a known published-at through `run_check`
  and asserts the rendered line carries THAT date") is tested via the extracted
  `render_check` pure function rather than by scraping process stdout: `cargo test`'s
  default output capture intercepts `println!` before it reaches a real file descriptor
  (it is a thread-local override inside libtest, not fd redirection), so there is nothing
  at the OS level to capture without `--nocapture`. The test still drives the real
  `check_latest` -> `compare_cached` -> `render_check` path end to end; only the final
  `println!` is swapped for asserting the string `render_check` already computed.
- Criterion 1's grep is satisfied by two adjustments to comment WORDING in `src/renew.rs`
  (not code): explanatory comments that named the literal tokens `UNIX_EPOCH` and
  `unwrap_or(Utc::now())` for context were reworded to describe the same fabrication
  without quoting it verbatim, since the doc's criterion is a literal zero-line grep
  against the whole file, comments included.

### Tradeoffs

- `seed_legacy_cache` writes YAML by hand instead of constructing a `CacheEntry` with
  `#[serde(skip_serializing)]` fields or a second struct: it costs a hardcoded string, but
  proves the ACTUAL on-disk shape (keys absent) rather than a shape that happens to
  round-trip the same way (keys present as `null`), which is the distinction the design
  doc's Data Model section calls out as load-bearing.
- `CannedApi::json` vs a second fixture type: folding it into the existing `CannedApi` via
  a shared `respond` keeps one loopback-listener implementation instead of two, at the cost
  of `status` no longer being the only constructor.

### Open questions

- None.

## Phase 3 (W3): Refresh escape and install freshness

### Design decisions

- `--refresh` is now one field, `#[arg(long, global = true)] refresh: bool` on
  `UpdateCmd` itself (`src/cmd.rs`), not on `UpdateSub::Check` or `UpdateSub::Install`.
  `UpdateSub::Check` is now a unit variant; `UpdateSub::Install` lost its `refresh` field.
  `run_inner` reads `self.refresh` for the `None | Check` arm; `Install` no longer reads
  it at all (see Deviations).
- Confirmed the seam Phase 1 handed forward, against the code as committed: refresh vs
  no-refresh lives entirely in `Renew::check_latest` vs `Renew::check_latest_refresh`
  (`src/renew.rs`), and `refresh_and_compare` takes no `force`/`refresh` parameter. Wiring
  the global flag to `self.refresh` choosing between those two methods in `run_check`
  needed no change to `src/renew.rs`.
- Install with no explicit version now unconditionally calls `renew.check_latest_refresh()`
  (`src/cmd.rs:run_install`), never `check_latest()`. This is the fix for "the confirm
  prompt can name a stale version and `--force` can be demanded spuriously": the cache is
  no longer consulted at all on that path, so a stale-but-TTL-fresh entry cannot leak into
  either the equality gate or the prompt text.
- Criterion 1 ("both parse and both take the refreshing path... never against live
  GitHub") is asserted through REAL clap argv parsing, not a hand-built `UpdateCmd { .. }`
  literal: since renew ships no binary, `src/cmd/tests.rs` declares a test-only
  `#[derive(clap::Parser)] TestCli { #[command(subcommand)] top: TestTop }` /
  `enum TestTop { Update(UpdateCmd) }`, the same nesting shape any real consumer CLI uses,
  and drives it via `TestCli::try_parse_from(["bin", "update", ...])`. A struct literal
  would only prove the field exists, not that `global = true` actually makes `--refresh`
  legal after the `check` token.
- That same test proves the flag reaches the network path, not merely that it parses: it
  seeds a fresh, TTL-live cache entry naming a HIGHER version against a `CannedApi` wired
  to 404. Without `--refresh` the cache hit reports the update (exit 1, no network
  touched); with `--refresh`, at both `update --refresh` and `update check --refresh`,
  the flag must force past the cache to the failing injected API (exit 2). A
  parse-accepted-but-never-read bug would still exit 1, so the exit-code flip is the
  actual assertion, not the successful parse alone.
- Criterion 2 ("a test asserts the no-version install path calls the refreshing check") is
  `test_run_install_no_version_always_refreshes_past_a_stale_cache`: it seeds a cache
  entry naming a version the injected API does NOT return, so the test only passes 0 if
  the target was resolved from the (refreshing) network call rather than from the stale
  cache entry.

### Deviations

- `run_install`'s `refresh: bool` parameter is deleted outright rather than kept and
  ignored: once the no-version arm always calls `check_latest_refresh()`, the parameter
  decided nothing and `deny(unused_variables)` / `deny(dead_code)` would have rejected a
  dead binding. Same effect as "install always refreshes," correct seam: the flag simply
  never reaches `run_install` at all now, rather than reaching it and being discarded.
- `--refresh` remains syntactically legal on `install` and `revert` (clap's
  `global = true` propagates to every subcommand of `UpdateCmd`, not just `check`), even
  though `run_install`/`run_revert` never read it. The design doc's own phrasing
  ("usable at either level") describes container-vs-`check`, not a restriction to
  `check` alone; narrowing acceptance to specific subcommands would need a custom clap
  validator this phase does not add, and a no-op flag being accepted is not the defect
  this doc is about (unlike the two flags this phase deletes, it cannot make Install
  demand `--force` spuriously or misreport a version, since Install now always refreshes
  regardless of whether the caller also typed `--refresh`).
- `test_run_install_already_current_without_force_exits_0` (Phase 1) is rewritten from a
  seeded-cache assertion to a `CannedApi::json` 200 response naming the current version:
  since install with no version no longer consults the cache at all, the old fixture no
  longer exercises the path the test's name claims.

### Tradeoffs

- A test-local `TestCli`/`TestTop` wrapper vs asserting only on `UpdateCmd` field values
  after a hand-built literal: the wrapper costs ~15 lines and a `clap::Parser` import
  local to the test module, in exchange for actually exercising clap's argv parser (the
  thing criterion 1 says "parses") rather than assuming the derive attributes behave as
  named.
- Proving "takes the refreshing path" via an exit-code flip (cache-hit 1 vs
  forced-network-failure 2) vs instrumenting `Renew` with a call counter: the exit-code
  approach needed no test-only observability added to `Renew`/`Fallback`, at the cost of
  the test's intent depending on a comment to explain why 1-vs-2 is the load-bearing
  signal rather than the flag simply being present.

### Open questions

- None.

## Audit fold-in: M2 (lock-held arm)

### Design decisions

- The lock-held arm now splits on WHAT the cache says rather than merely on whether it
  said anything - `src/renew.rs:fallback_to_cache` - because the two cached verdicts are
  not equally true once a check did not happen. A cached `Ok(Some(update))` stays
  reportable: "a newer release exists" does not stop being true because a peer holds
  `refresh.lock`. A cached `Ok(None)` is the stale cache voting that the caller is
  current, which is exactly the D3 defect, and it now takes the same `CheckFailed` path
  as an empty cache (exit 2) instead of rendering `<bin> <version> (latest)` at exit 0.
- The `CheckFailed` the lock-held arm raises now carries `stale: cached.map(..)` rather
  than a hardcoded `stale: None` - `src/renew.rs:fallback_to_cache` - so the failure
  message names what the cache knew and when ("from cache, unverified: last seen X
  (checked at ...)"), matching the `Fallback::Failed` arm. With no cache file at all the
  map yields `None`, so `test_lock_held_by_peer_with_empty_cache_is_an_error`'s
  `stale: None` assertion is unaffected.
- The regression test lives in `src/cmd/tests.rs`, not beside its two siblings in
  `src/renew/tests.rs`, because the claim under test is "does not report `(latest)` at
  exit 0" and both the rendering (`render_check`) and the exit code (`UpdateCmd::run`)
  are private to `cmd`. It asserts all three: the `Err(CheckFailed)`, the exit code 2,
  and that the code is not the one `render_check(&renew, &None)` produces.
- The `fallback_to_cache` doc comment at `src/renew.rs:306` kept its "never `Ok(None)`"
  sentence (now true on all four arms) and gained the distinction that makes it true:
  the only success it can return is `Ok(Some(update))` under contention.

### Deviations

- None. The W3 Phase 1 bullet that lock contention is not in itself a failure is intact:
  contention still reports from cache whenever the cache has something it is entitled to
  say. What narrowed is the set of things it is entitled to say.

### Tradeoffs

- Matching on `Some(Ok(Some(_))) / Some(Err(_)) / Some(Ok(None)) | None` vs keeping the
  flat `Some(result) => result` and filtering afterward: the nested match is wordier but
  puts the "informs vs votes" distinction in the control flow where a reader hits it,
  rather than in a comment above a passthrough.
- `Some(Err(e)) => Err(e)` passes an unparseable cached tag through as `InvalidTag`
  rather than wrapping it in `CheckFailed` like its neighbors. It already exits 2 and
  already names its own cause, so wrapping it would only add a layer; the cost is that
  the three failure exits from this arm do not all render with the same prefix.

### Open questions

- None.
