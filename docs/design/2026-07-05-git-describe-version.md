# Design Document: renew reads the installed binary's real version (GIT_DESCRIBE)

**Author:** Scott Idler
**Date:** 2026-07-05
**Status:** Implemented
**Shipped in:** renew v0.2.0, persona-cli v1.3.0
**Review Passes Completed:** 5/5

## Summary

`renew`'s version-check compares against the wrong baseline: the `renew!()` macro
feeds `CARGO_PKG_VERSION` as the "currently running" version, but every consumer
in the fleet sets its `--version` from `GIT_DESCRIBE` (a `build.rs`-emitted
`git describe --tags`). This design makes `renew` version-aware of `GIT_DESCRIBE`
with a clean fallback to `CARGO_PKG_VERSION`, adds describe-tolerant version
parsing so a `vX.Y.Z` tag (and a `vX.Y.Z-N-gSHA` dev build) parse correctly, and
proves the whole path end-to-end by piloting self-update on `persona-cli`.

## Problem Statement

### Background

- `renew` (`tatari-tv/renew`, currently `v0.1.2`) is a library for self-updating
  Rust CLIs that ship via GitHub Releases. Two halves: `notify_if_outdated()`
  (a passive "you're behind" notice printed on every interactive invocation) and
  `UpdateCmd` (a `clap::Args` `update check|install|revert` subcommand).
- Its three original consumers (`ccu`, `cr`, `claude-permit`) were folded into the
  `clyde` monorepo and deprecated. `renew` has **zero live consumers today**.
- The fleet still has tools that already publish GitHub Release binaries matching
  `renew`'s producer contract exactly: `persona-cli`, `sdv`, `pagerduty-cli`, `gx`
  (all four use the identical `linux-amd64 / linux-arm64 / macos-x86_64 / macos-arm64`
  suffix scheme `renew` hardcodes). Verified 2026-07-05.

### Problem

The version `renew` treats as "current" does not match the version the consumer
actually reports:

- `renew!()` expands to `Renew::new(env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))`
  (`src/lib.rs:22-29`).
- Consumers set `--version = env!("GIT_DESCRIBE")` (confirmed: `persona-cli/src/cli.rs:28`),
  where `GIT_DESCRIBE = git describe --tags --always` (`persona-cli/build.rs:4`).
- `Renew::new` parses "current" with `Version::parse(current)` **directly**
  (`src/renew.rs:63`), not through the `v`-stripping `parse_tag` (`src/version.rs:22`).

Consequences if `GIT_DESCRIBE` were passed to today's `Renew::new` unchanged:

| GIT_DESCRIBE value | `Version::parse` result | Effect |
|--------------------|-------------------------|--------|
| `v1.2.1` (clean release build — the shipped case) | **error** (semver rejects `v`) | check disabled / hard error |
| `v1.2.1-3-gabc123` (dev build) | **error** on `v`; even stripped, `1.2.1-3-gabc123` sorts as a *prerelease of 1.2.1* (below it) | false "1.2.1 available" nag |
| `1.2.1` (git absent, `CARGO_PKG_VERSION` fallback) | ok | correct |

So the macro cannot simply be pointed at `GIT_DESCRIBE`; the parsing has to change
with it. Fixing the macro alone would make correct-today consumers worse.

### Goals

- `renew!()` (all forms) resolves "current" from `GIT_DESCRIBE` when the consumer's
  build script set it, falling back to `CARGO_PKG_VERSION` when it did not.
- `renew` parses a `git describe` string correctly: strips a `v` prefix, strips a
  `-N-gSHA[-dirty]` describe suffix down to the base semver, and leaves a genuine
  semver prerelease (`1.2.1-rc.1`) intact.
- Prove it end-to-end: `persona-cli` adopts `renew` via the no-arg `renew!()` and
  its `update`/notify surface works against real published releases.

### Non-Goals

- **Fix `install_version` for non-latest tags.** Pre-existing defect: `install_version`
  fetches only `releases/latest` and errors if the requested tag != latest
  (`src/renew.rs:302,330`), so `update install <older>` cannot reach a non-latest
  release. Parked (Addendum A) — independent of the version baseline; revisit when a
  consumer needs pinned-version rollback.
- **Fix the cache-hit `published_at` cosmetic.** On a cache hit, `compare_cached`
  synthesizes `published_at = UNIX_EPOCH` (`src/renew.rs:166`), so `update check`
  prints "released 1970-01-01" on the cached path. Parked (Addendum A).
- **Add a release workflow to `cargo install`-only tools** (`marquee`, `drata-cli`,
  `slack-cli`, `git-tools`, `clyde`). Out of scope until each is chosen for adoption.
- **Broaden platform support** (musl, Windows). Unchanged.
- **Roll out to `sdv`/`pd`/`gx`.** Symmetric to the persona pilot; excluded here to
  keep one concrete case (they follow after the pilot proves the path).

## Proposed Solution

### Overview

Three changes in `renew`, then a `persona-cli` pilot:

1. **Macro → source selection** (`src/lib.rs`): every `renew!()` form resolves the
   current-version string via
   `option_env!("GIT_DESCRIBE").filter(|s| !s.trim().is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))`.
2. **Describe-tolerant parsing** (`src/version.rs`): a new `parse_current()` that
   normalizes a `git describe` string to a base `semver::Version`; `Renew::new`
   uses it instead of raw `Version::parse`.
3. **Tests** (`src/version/tests.rs`): the full matrix of describe forms.
4. **Pilot** (`persona-cli`): add `[package].repository` + fix `build.rs`, depend on
   `renew`, wire passive notify (match-and-drop, not `?`) + an early-intercepted
   `Update(UpdateCmd)`. See Construction-failure policy for why notify must not
   `?`-propagate.

### Architecture

```
consumer crate (persona)                 renew crate
------------------------                 -----------
build.rs: cargo:rustc-env=GIT_DESCRIBE   lib.rs      renew!() -> option_env!("GIT_DESCRIBE")
   |  (compile-time env of THIS crate)              .unwrap_or(env!("CARGO_PKG_VERSION"))
   v                                                        |
main.rs: renew!()  --expands here-->  Renew::new(repo, bin, current_str)
                                                            |
                                                 version::parse_current(current_str)
                                                   strip 'v' -> strip -N-gSHA[-dirty] -> semver
```

`option_env!` is expanded at the macro's **call site**, during compilation of the
**consumer** crate, so it reads the consumer build script's `GIT_DESCRIBE`. Unlike
`env!`, `option_env!` yields `None` (not a compile error) when the var is absent,
which is what makes the `CARGO_PKG_VERSION` fallback work for build-script-less
consumers. This cross-crate resolution is the one environmental assumption the
design rests on and is proven zero-code in Phase 0.

### Data Model

No persisted schema changes. `check.yml` cache and `Update`/`InstalledVersion`
types are unchanged. The only behavioral change is the string handed to
`Renew::new` and how it is parsed into `semver::Version`.

### API Design

**`src/version.rs`** — new function (replaces the raw parse at the seam):

```rust
/// Parse the running binary's version from a `GIT_DESCRIBE`-or-`CARGO_PKG_VERSION`
/// string. Accepts: `1.2.1`, `v1.2.1`, `v1.2.1-3-gabc123`, `v1.2.1-3-gabc123-dirty`,
/// and a genuine prerelease `1.2.1-rc.1` (left intact).
pub(crate) fn parse_current(input: &str) -> Result<Version> {
    let s = input.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let core = strip_describe_suffix(s);
    Version::parse(core).map_err(|e| Error::InvalidTag { tag: input.to_string(), source: e })
}

/// Remove a `git describe` suffix `-<count>-g<sha>[-dirty]`, leaving the base tag.
/// Scans from the **right** (the describe suffix is always the final one), so a base
/// tag that itself carries a prerelease (`1.2.1-rc.1-3-gabc`) keeps `1.2.1-rc.1`, and
/// a tag literally embedding the pattern is not over-truncated. A plain semver
/// prerelease (no trailing `-<digits>-g<hex>`) is returned unchanged. No `regex` dep:
/// a ~20-char right-to-left scan does not justify one.
fn strip_describe_suffix(s: &str) -> &str { /* rightmost -\d+-g[0-9a-f]+ scan */ }
```

**`src/renew.rs:63`** — `Renew::new` swaps
`Version::parse(current.as_ref())` for `version::parse_current(current.as_ref())`.

**`src/lib.rs`** — all five `renew!()` arms replace the version argument
`env!("CARGO_PKG_VERSION")` with
`option_env!("GIT_DESCRIBE").filter(|s| !s.trim().is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))`
(one shared internal expansion so the five arms stay identical — siblings behave
identically). The `.filter` guards the empty/whitespace `GIT_DESCRIBE` a consumer
build script emits on git failure when it does not check `output.status.success()`
(persona's does not — brought to fleet parity in Phase 2).

**Construction-failure policy (unparseable input).** `Renew::new` parses the current
version eagerly and returns `Err` on unparseable input (bare SHA from an
untagged/tarball build; a malformed describe). `Renew::new` runs **before**
`notify_if_outdated`, so a naive `renew!()?.notify_if_outdated()` would abort the
whole program on that `Err` — defeating the "notice degrades silently" intent
(finding #1, verified). The two consumer paths therefore handle construction failure
differently:
- **Passive notify:** never `?`-propagate.
  `match renew::renew!() { Ok(r) => r.notify_if_outdated(), Err(e) => log::debug!("renew unavailable: {e}") }`.
  A bad version or repo-metadata error disables the notice and logs at debug; every
  other command runs normally.
- **`update` subcommand:** map a construction `Err` to renew's documented exit 2
  (`eprintln!("error: {e}")` then `exit(2)`), matching `UpdateCmd::run`'s contract
  (`src/cmd.rs:52`).

This is fail-loud-fail-closed done right: no fabricated `0.0.0` that would nag every
dev build, and no startup abort that takes down unrelated commands. `Renew::new`
keeps returning `Err` (the loud signal stays available); the *consumer wiring*
decides degrade-vs-abort per path.

### Implementation Plan

Cross-repo. Phases 0-1 land in `tatari-tv/renew`; the `renew v0.2.0` tag is the
hand-off; Phases 2-3 land in `tatari-tv/persona-cli`. See Rollout Plan for ship order.

#### Phase 0: Prove the GIT_DESCRIBE contract on a shipped artifact
**Model:** sonnet
- Download the latest `persona-cli` release tarball and run the extracted binary's
  `--version`; confirm it prints a clean `vX.Y.Z` (not a dirty describe or bare SHA).
- Confirm `option_env!("GIT_DESCRIBE")` resolves the *consumer's* build-script value:
  build a throwaway 2-crate example (a lib exporting a `option_env!("GIT_DESCRIBE")`
  macro + a bin whose `build.rs` sets it) and print the result. Zero production code.
- **Success criteria:**
  - the shipped `persona v1.2.1` binary prints `v1.2.1` from `--version`
  - the throwaway consumer prints its own `GIT_DESCRIBE`, and prints the
    `CARGO_PKG_VERSION` fallback when the build script is removed

#### Phase 1: renew version normalization + macro
**Model:** opus
- Add `version::parse_current` + `strip_describe_suffix` (rightmost scan) (`src/version.rs`).
- Point `Renew::new` at `parse_current` (`src/renew.rs:63`).
- Update all five `renew!()` arms to source
  `option_env!("GIT_DESCRIBE").filter(|s| !s.trim().is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))`
  via one shared internal expansion (`src/lib.rs:21-46`).
- Add the test matrix (`src/version/tests.rs`) and a macro-expansion test covering all
  five arms.
- Update `README.md` (the `tag = "v0.1.0"` pin example at lines 16, 175) and `CONTEXT.md`
  to document the `GIT_DESCRIBE`-first current-version semantics and the empty-string
  guard — living docs track shipped reality.
- **Success criteria:**
  - `otto ci` green in `renew`
  - `parse_current` tests assert: `v1.2.1 == 1.2.1`; `v1.2.1-3-gabc123 == 1.2.1`;
    `v1.2.1-3-gabc123-dirty == 1.2.1`; `1.2.1-rc.1` keeps a non-empty `.pre`;
    `1.2.1-rc.1-3-gabc == 1.2.1-rc.1` (rightmost scan, the pathological case named in
    the API comment); `abc123f`, `""`, and `"   "` all `.is_err()`
  - a macro test proves each of the five arms expands with consumer-side source
    selection (not renew's own env)
  - one negative test breaks intentionally (assert the wrong expected value) to prove
    the test bites, then is reverted

#### Phase 2: persona-cli pilot integration
**Model:** sonnet
- Add `repository = "https://github.com/tatari-tv/persona-cli"` to `[package]`
  (`persona-cli/Cargo.toml`) — required for the no-arg `renew!()`.
- Fix `persona-cli/build.rs:4-8` to check `output.status.success()` before using
  stdout (fleet parity: renew/sdv/pagerduty-cli already do; persona emits
  `GIT_DESCRIBE=""` on git failure without it). Defense-in-depth with the macro
  `.filter` from Phase 1.
- Depend on `renew` via an **unmerged branch/path dep** for integration (NOT a tag
  yet — see Rollout: the tag is burned if this phase finds a defect, and Scott's git
  rules forbid moving a tag).
- Wire the thin shell (`persona-cli/src/main.rs`):
  - **Passive notify**, degrade-not-abort:
    `match renew::renew!() { Ok(r) => r.notify_if_outdated(), Err(e) => log::debug!("renew unavailable: {e}") }`
    placed after `setup_logging` (so renew's `log` records flow) but such that a
    failure never aborts the command.
  - **`update` subcommand**: add `Update(UpdateCmd)` to `Commands` (`src/cli.rs`) and
    **intercept it early in `run()` — right after `setup_logging`, before
    `Config::load`/`OktaAuth`** (`src/main.rs:76-94`): `update check|install|revert`
    need neither Persona API config nor Okta auth. Map a `renew!()` construction
    `Err` to exit 2, else `std::process::exit(cmd.run(&r))`.
- **Success criteria:**
  - `otto ci` green in `persona-cli`
  - `persona update check` on a binary built at `v1.2.1` exits 0 ("latest"); on a
    binary built with `GIT_DESCRIBE` forced older (env override at build:
    `GIT_DESCRIBE=v1.0.0 cargo build`) exits 1
  - `persona update check` runs to a decision **without** valid Okta creds present
    (proves the early intercept bypasses auth)
  - `persona --version` (raw `GIT_DESCRIBE`) and the normalized version renew reports
    in `update check` output refer to the same release (renew prints normalized
    semver, so assert the semver core matches, not string-equality)

#### Phase 3: shakedown + regression
**Model:** sonnet
- `/cli-shakedown` the `persona update` surface (check/install/revert help + check).
  Use `update check --refresh` (or accept the parked 1970 cache-date on the cached
  path per Addendum A.2) so the cosmetic cache-date is not mistaken for a regression.
- Assert the notify path is TTY-gated with **two separate runs** (a single run cannot
  have stderr both a TTY and redirected):
  - interactive run (stderr is a TTY), artificially-old build → a "new version" line
    appears on stderr
  - redirected run `persona whois scott 2>/tmp/e`, same build → `/tmp/e` is empty
- **Success criteria:**
  - the interactive run prints the notice to the TTY; the redirected run leaves
    `/tmp/e` empty
  - shakedown field guide records the tested `update` invocations

## Acceptance Criteria

- [ ] `version::parse_current` returns `1.2.1` for `v1.2.1`, `v1.2.1-3-gabc123`,
      `v1.2.1-3-gabc123-dirty`; keeps the prerelease for `1.2.1-rc.1` and
      `1.2.1-rc.1-3-gabc` (rightmost scan); errors on a bare SHA, `""`, and `"   "`.
      (renew unit tests)
- [ ] All five `renew!()` arms resolve current from the consumer's `GIT_DESCRIBE`
      when present and non-empty, and fall back to `CARGO_PKG_VERSION` when absent or
      empty. (Phase 0 spike + Phase 1 macro test + persona)
- [ ] A construction failure degrades per path: passive `renew!()` failure disables
      the notice with a `log::debug!` and does NOT abort other commands; `update`
      construction failure exits 2. (persona)
- [ ] `persona update check` reaches a decision without Okta creds, exits 1 when the
      running build is older than the latest published release and 0 when equal, and
      the notice is printed on an interactive stderr but absent when stderr is
      redirected. (persona, against the real `v1.2.1` release; two TTY runs)
- [ ] `otto ci` green in both `renew` and `persona-cli`.

## Resolved Decisions

- **2026-07-05 (author):** Pilot uses the no-arg `renew!()` + a new `[package].repository`
  field, **not** the explicit `Renew::new("tatari-tv/persona-cli", "persona", env!("GIT_DESCRIBE"))`
  escape hatch. Rationale: the pilot must exercise the exact macro path being shipped
  for the fleet; the explicit form would bypass the very thing under test.
- **2026-07-05 (author):** The two pre-existing renew defects (`install_version`
  non-latest; cache-hit `published_at`) are parked, not folded in. Rationale: scope
  discipline — neither is caused by or blocks the version-baseline fix. Addendum A.
  Both reviewers concurred they are correctly parked.
- **2026-07-05 (author):** Normalization lives in `renew::version::parse_current`
  (single source of truth), never smeared into the macro. The macro only selects the
  source string.

### Review-panel consensus (2026-07-05, Architect + Staff Engineer)

- **Construction-failure wiring (finding #1, verified defect — both models):**
  `renew!()?` propagates a constructor `Err` before `notify_if_outdated` can run, so
  an unparseable version would abort every persona command. Folded in: passive notify
  matches-and-drops with a `log::debug!`; `update` maps construction `Err` to exit 2.
  `Renew::new` still returns `Err` (loud signal preserved). See Construction-failure
  policy.
- **Empty `GIT_DESCRIBE` (finding #2, verified — both models):** persona's `build.rs`
  emits `GIT_DESCRIBE=""` on git failure (no `status.success()` check). Folded in:
  macro guards with `.filter(|s| !s.trim().is_empty())` AND persona's build.rs is
  fixed for fleet parity; `""`/`"   "` added to the test matrix.
- **`strip_describe_suffix` scan direction (finding #5, divergence resolved):** adopt
  a **rightmost** scan (the describe suffix is always the final one) — identical cost,
  strictly more correct than leftmost. **Pushback accepted, no `regex` dep**: a
  ~20-char scan does not justify a dependency (Architect proposed `regex`; rejected on
  owner taste + the doc's no-new-crates constraint; panel concurred).
- **Ship order (finding #4):** integrate persona against an unmerged renew branch/path
  dep and go e2e-green BEFORE tagging `renew v0.2.0`, because Scott's git rules make
  tags immutable and a Phase-2 defect would otherwise burn the tag. Folded into Rollout.
- **`update` dispatch bypass (finding #6):** intercept `Update` early in persona's
  `run()`, before `Config::load`/`OktaAuth`, since the update surface needs no Persona
  API/auth. Folded into Phase 2.
- **Living docs (finding #7) + AC/Phase wording (findings #3, #8):** README/CONTEXT
  updated in Phase 1; test matrix and TTY/`--version` assertions tightened. Folded in.
- **Confirmed sound, kept as guardrail:** the `option_env!` cross-crate assumption
  (both models verified it independently against Rust macro-expansion semantics).
  Phase 0 remains as a cheap guardrail, not a live risk.

## Alternatives Considered

### Alternative 1: Fix only the macro (`option_env!` GIT_DESCRIBE), leave parsing alone
- **Description:** Point `renew!()` at `GIT_DESCRIBE`; no `parse_current`.
- **Pros:** One-line change.
- **Cons:** `Renew::new` still calls raw `Version::parse`, which rejects the `v`
  prefix and mis-orders `-N-gSHA` as a prerelease. Would break the correct-today
  `CARGO_PKG_VERSION` consumers and every shipped `vX.Y.Z` binary.
- **Why not chosen:** Makes the bug worse; the macro and the parser must move together.

### Alternative 2: Normalize inside the macro
- **Description:** Do the `v`-strip / describe-strip in `renew!()` expansion.
- **Pros:** Keeps `Renew::new` untouched.
- **Cons:** Logic duplicated per macro arm and invisible to the explicit
  `Renew::new` path; two code paths to keep in sync; untestable without expanding
  the macro.
- **Why not chosen:** Violates single-source-of-truth; parsing belongs in `version.rs`.

### Alternative 3: Require every consumer to pass the version explicitly
- **Description:** Drop the no-arg `renew!()`; consumers call
  `Renew::new(repo, bin, env!("GIT_DESCRIBE"))`.
- **Pros:** No `option_env!` subtlety.
- **Cons:** Boilerplate at every call site; defeats the drop-in macro; the user's
  stated ask was specifically to make the macro read the right value.
- **Why not chosen:** Contradicts the requirement; the macro is the product.

## Technical Considerations

### Dependencies

- No new crates. `option_env!`/`env!` are macro built-ins; `semver` already present.
- `persona-cli` gains a git dependency on `renew`: an unmerged branch/path dep during
  the pilot, repinned to `tag = "v0.2.0"` once the tag is cut (see Rollout).

### Performance

Irrelevant. One extra string scan at startup (`strip_describe_suffix` over a
~20-char string), inside the already-cached `notify_if_outdated` path.

### Security

- No change to the token/redirect model (`GH_TOKEN`/`GITHUB_TOKEN` auto-read,
  `Bearer` on API + download, stripped on the S3 redirect — `src/github.rs:44-49`).
- persona is a private repo; the fleet already carries a token in CI and locally, so
  `notify_if_outdated` against a private release works via the existing channel.

### Testing Strategy

- Unit: the `parse_current` matrix in `src/version/tests.rs` (positive + negative +
  a deliberately-failing assertion proven to fail then reverted).
- Integration: Phase 0 throwaway proves `option_env!` cross-crate resolution.
- End-to-end: persona `update check`/notify against the real `v1.2.1` release.

### Rollout Plan

Ship order is designed so the immutable `renew v0.2.0` tag is created only after the
pilot proves the change end-to-end (tags are never moved per Scott's git rules, so a
tag must never front-run an unverified integration):

1. Land Phase 1 in `renew` `main` (ungated — classic protection 404, rulesets empty
   as of 2026-07-05). Do **not** tag yet.
2. Integrate Phases 2-3 in `persona-cli` against an **unmerged** `renew` (a branch git
   dep `{ git = "...", branch = "..." }` or a local `path` dep) and drive the pilot to
   e2e-green (all Phase 2-3 success criteria).
3. Only then tag `renew v0.2.0` on `main` via `bump -m` (minor: current-version source
   semantics change for consumers), then `git push origin main && git push origin v0.2.0`.
4. Repin persona's `renew` dep to `tag = "v0.2.0"` on the persona branch. persona
   `main` is gated → `bump --no-tag` on the branch, PR, merge, then `bump --tag-only`
   post-merge.
5. Cut a persona release so a binary carrying the self-update wiring exists in the
   wild; verify `persona update check` against it (done = a shipped binary self-reports
   correctly, not localhost).

### Blast radius

Two repos: `tatari-tv/renew` (the change) and `tatari-tv/persona-cli` (the pilot
consumer). `sdv`, `pd`, `gx` are unaffected until separately adopted. No shared
crate or deployed service is touched.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Construction `Err` aborts every command via `renew!()?` | Med | High | Verified finding #1: passive notify matches-and-drops (no `?`); `update` maps `Err` to exit 2. Construction-failure policy + Phase 2 wiring. |
| Empty/whitespace `GIT_DESCRIBE` defeats the fallback | Med | High | Verified finding #2: macro `.filter(non-empty)` + persona `build.rs` `status.success()` fix; `""`/`"   "` in the test matrix |
| `option_env!` reads renew's env, not the consumer's | Low | High | Confirmed sound by both reviewers; Phase 0 spike kept as a guardrail before any production change |
| Shipped `GIT_DESCRIBE` is dirty/`-N-gSHA`, not a clean tag | Low | Med | `parse_current` strips the describe suffix; Phase 0 confirms the real artifact prints clean `vX.Y.Z` |
| `strip_describe_suffix` mangles a prerelease tag | Low | Med | Rightmost `-\d+-g<hex>` scan; prerelease (`-rc.1`) has no trailing such pattern; `1.2.1-rc.1-3-gabc` covered by test matrix |
| Tagging `renew v0.2.0` before pilot e2e burns an immutable tag | Med | Med | Rollout: integrate persona against unmerged renew branch/path dep, e2e-green, THEN tag |
| Consumer lacks `[package].repository` → `renew!()` fails to compile | Med | Low | Phase 2 adds it; loud compile error, not silent |
| persona `main` gate blocks the tag | Low | Low | Documented gated flow (`bump --no-tag` → merge → `bump --tag-only`) |

## Open Questions

- None. Review panel run (Architect + Staff Engineer, 2026-07-05); every finding is
  dispositioned in Resolved Decisions → Review-panel consensus (all folded in; one
  `regex` suggestion pushed back with rationale and the panel concurred). No
  unresolved pushbacks, nothing escalated. Ready to build.

## Addendum A: Parked pre-existing renew defects

Captured so the review does not re-litigate them as novel and so they are not lost:

1. **`install_version` cannot reach a non-latest tag.** It calls
   `github::latest_release` and returns `NoRelease` when the requested tag != latest
   (`src/renew.rs:302,330`); it never uses `GET /releases/tags/{tag}`. `CONTEXT.md:208`
   advertises "arbitrary rollback," which the code contradicts. Revisit when a
   consumer needs pinned-version install/rollback (backup-based `revert` is
   unaffected and works).
2. **Cache-hit `published_at` is `UNIX_EPOCH`.** `compare_cached` synthesizes the
   date (`src/renew.rs:166`), so `update check` on a cache hit prints
   "released 1970-01-01" (`src/cmd.rs:112`). Cosmetic; two-line fix (carry the real
   date in the cache entry or omit the date on the cached path).

## References

- renew: `src/lib.rs:21-46` (macro), `src/renew.rs:56-88` (`Renew::new`),
  `src/version.rs:22-28` (`parse_tag`), `src/github.rs:44-49` (redirect auth)
- persona-cli: `src/cli.rs:28` (`version = env!("GIT_DESCRIBE")`),
  `build.rs:4` (`git describe --tags --always`), `src/main.rs:55-67` (thin shell),
  `.github/workflows/release.yaml` (producer asset contract)
- renew `README.md` (producer contract, exit codes), `CONTEXT.md` (design language)
- Rule of Five: `~/repos/scottidler/obsidian/notes/jeffrey-emanuel-rule-of-five-agentic-llm.md`
</content>
</invoke>
