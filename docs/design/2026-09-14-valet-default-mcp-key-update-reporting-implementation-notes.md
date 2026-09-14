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
