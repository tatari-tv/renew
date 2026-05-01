# Renew

A library crate for self-updating Rust CLI binaries that ship via GitHub releases.

## Language

**Renew**:
The library itself; the crate that consumers depend on.

**Consumer**:
A CLI binary that depends on Renew to manage its own version checks and self-updates. Today's set: `ccu`, `cr`, `claude-permit`.
_Avoid_: client, dependent, host

**Release**:
A GitHub release on the Consumer's repo, tagged `vX.Y.Z` (SemVer with `v` prefix), with platform binaries attached as assets named `<bin>-<tag>-<os>-<arch>.tar.gz` plus a `.sha256` sidecar.
_Avoid_: version, tag, build

**Update**:
A Release whose version is strictly greater than the Consumer's currently-running version.
_Avoid_: upgrade, new version

**Check**:
Looking up the latest Release on GitHub and comparing it against the running version. May be served from cache; never fails noisily.
_Avoid_: poll, fetch

**Install** (verb):
Downloading the new binary asset, verifying its SHA, and replacing the file at `current_exe()` with the new binary. Runs from inside the Consumer process; the running process keeps its in-memory copy.
_Avoid_: upgrade, swap, deploy

**Notify**:
Printing a one-line "new version available" message to stderr. Caused by a positive Check; never causes Install on its own.

## Relationships

- A **Consumer** depends on **Renew** as a git+tag dependency (matches the existing `claude-pricing` pattern).
- A **Consumer** has many **Releases** over time on its GitHub repo.
- A **Check** compares the Consumer's current version against the latest **Release** and may produce an **Update**.
- An **Update** is the input to **Install**; **Notify** is the user-facing side effect of a positive Check.

## API layering

Layers stack; each built on the one below:

- **Core struct** — `Renew::new(repo, bin, current_version)` plus a builder for optional knobs (cache TTL, cache dir). Holds Consumer identity once.
- **(a) Core methods** — `Renew::check_latest`, `Renew::install_latest`. Sync, fallible, return structured data.
- **(b) Notify helper** — `Renew::notify_if_outdated`. One-line drop-in for `main()`. Infallible (swallows network errors); prints to stderr.
- **(c) Clap subcommand** — `UpdateCmd` implementing `clap::Args`. Drop-in so Consumers get `<bin> update` for free.
- **Macro** — `renew::renew!()` expands to `Renew::new(env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))`. Override args (`repo = "..."`, `bin = "..."`) when defaults don't fit. Sugar only.

Power users skip the helpers and call core methods directly.

## Consumer identity table

Confirmed by reading each Consumer's `Cargo.toml`:

| Consumer | package name | binary name | repo | macro form |
|----------|--------------|-------------|------|------------|
| ccu | `ccu` | `ccu` | `tatari-tv/claude-cost-usage` | `renew!()` |
| cr | `claude-report` | `cr` | `tatari-tv/claude-report` | `renew!(bin = "cr")` |
| claude-permit | `claude-permit` | `claude-permit` | `tatari-tv/claude-permit` | `renew!()` |

Rollout precondition: every Consumer must add `repository = "https://github.com/tatari-tv/<repo>"` to its `[package]` table so the macro's no-arg form works.

`Renew::new` accepts either a full URL (`https://github.com/owner/repo`) or a bare slug (`owner/repo`); the constructor normalizes.

## Release artifact contract

Confirmed by downloading current tarballs from all three Consumers:

- Asset names: `<bin>-v<X.Y.Z>-<os>-<arch>.tar.gz` (and `.sha256` sidecar)
- Platforms shipped today: `linux-amd64`, `linux-arm64`, `macos-arm64`, `macos-x86_64`. No Windows.
- Tarball contents: exactly one file named `<bin>`, mode 755, ELF/Mach-O binary directly. No nesting, no man pages, no extras.
- Sidecar format: standard `sha256sum` output — `<64 hex chars><two spaces><filename>\n`. Verified against actual content for all three.

If a Consumer ever adds extras to its tarball, the contract changes and `renew` needs a deliberate version bump. The library will fail loudly if it sees more than one regular file in the tarball, not silently ignore.

## Install pipeline

```
1. Detect platform           → "linux-amd64" / "linux-arm64" / "macos-arm64" / "macos-x86_64"
2. Build asset URLs          → tarball + .sha256 sidecar
3. Download tarball + sidecar → <cache_dir>/<bin>/download/
4. Verify SHA256             → ChecksumMismatch on fail
5. Extract single binary     → <cache_dir>/<bin>/download/<bin>
6. chmod 755 extracted file
7. Save current binary as backup → <data_local>/<bin>/backup/binary + meta.yml
8. Atomic-replace current_exe() via self_replace
9. Return InstalledVersion { from, to, path }
```

Steps 7 and 8 must succeed together — backup is captured immediately before the swap so revert always undoes the last install. If step 8 fails after step 7, the backup is left in place (no harm — it now matches the unchanged current binary).

## Filesystem layout

| Purpose | Location | Survives cache cleanup? |
|---------|----------|-------------------------|
| Version-check cache | `dirs::cache_dir()/<bin>/check.yml` | No — regenerable |
| Download staging | `dirs::cache_dir()/<bin>/download/` | No — regenerable |
| Revert backup | `dirs::data_local_dir()/<bin>/backup/{binary,meta.yml}` | Yes — recovery state |

`check.yml` shape:

```yaml
latest-version: 0.5.0
checked-at: 2026-05-01T08:00:00Z
```

No de-dup state. The Notify cadence ("print every interactive invocation when outdated") doesn't need it.

## Notify cadence

`Renew::notify_if_outdated`:

1. If cache is stale (older than `check_interval`, default 24h), refresh from GitHub. Short network timeout (~2s). Errors swallowed — keep stale cache.
2. If `latest > current` **and** `std::io::stderr().is_terminal()`: print one line to stderr.
3. No persistent "have we notified?" state — every interactive invocation prints if outdated.

Non-TTY stderr (statusline pipes, log redirection, CI) → silent. Cache refresh still happens so the next interactive invocation has fresh data.

Explicit subcommands (`<bin> update`, `<bin> update --check-only`) bypass `is_terminal` and always print — the user invoked them deliberately.

## Subcommand surface (the `(c)` drop-in)

Renew exports an `UpdateCmd` implementing `clap::Args`. Consumers wire it into their CLI enum. The shape:

```
<bin> update                 # default = install latest
<bin> update install         # explicit; install latest
<bin> update install 0.4.2   # install specific version
<bin> update install --force # reinstall even if already current
<bin> update check           # check only; exit 0 = current, 1 = update available, 2 = error
<bin> update revert          # restore the backup
```

### Interactive UX

- **Install / Revert**: prompt `Replace v1.2.3 with v1.5.0? [Y/n]` by default. `--yes` skips. If stdin is **not** a TTY and `--yes` was not passed → error out (no silent automation).
- **Check**: never prompts. Exit codes — `0` up-to-date, `1` update available, `2`+ error. Designed for `if ! ccu update check; then ccu update install --yes; fi`.
- **Install when already current**: no-op with `Already on v1.5.0 (latest). Use --force to reinstall.` `--force` runs the full pipeline anyway (re-download, verify, replace) for binary-corruption recovery. Same rule for `install <version>` when `version == current`.

## Release artifact contract (producer side)

Verified against `ccu`'s `release-and-publish.yml`. Every Consumer's release workflow MUST produce, on tag push `v*`:

| `std::env::consts::OS` / `ARCH` | Suffix in asset name | Cargo target triple |
|---------------------------------|----------------------|---------------------|
| `linux` / `x86_64` | `linux-amd64` | `x86_64-unknown-linux-gnu` |
| `linux` / `aarch64` | `linux-arm64` | `aarch64-unknown-linux-gnu` |
| `macos` / `x86_64` | `macos-x86_64` | `x86_64-apple-darwin` |
| `macos` / `aarch64` | `macos-arm64` | `aarch64-apple-darwin` |

Note the **asymmetric naming** — Linux uses `amd64` for x86_64 but macOS uses `x86_64`. Both use `arm64` for aarch64. This is what the workflow produces today; renew matches it exactly.

Per-asset shape: `<bin>-v<X.Y.Z>-<suffix>.tar.gz` containing exactly one file `<bin>` (mode 755), plus `<bin>-v<X.Y.Z>-<suffix>.tar.gz.sha256` in `sha256sum` format.

If a Consumer's release pipeline drifts from this contract (different naming, multi-file tarballs, missing platform), renew fails with `AssetMissing` or `ChecksumMismatch`. Renew does not silently substitute or fall back.

## Auth

Default: **auto-read `GH_TOKEN`, then `GITHUB_TOKEN`** from the environment. Falls back to anonymous if neither is set. Matches the convention used by `gh` CLI, GitHub Actions runners, and most other GitHub-touching tools.

Why auto-read by default:

- A user who has `GITHUB_TOKEN` set wants GitHub-touching tools to use it; that's the whole purpose of the env var.
- CI environments (GitHub Actions) always have `GITHUB_TOKEN` set — auto-read means renew works under load there without bumping into anonymous IP-shared rate limits.
- Failure modes are graceful: missing token → anonymous fallback; wrong/expired token → typed error, cache holds.
- Required for private-repo support; once future Consumers exist that aren't all public, those work with no API change.

Override surface:

```rust
.with_token(Some("ghp_..."))   // explicit token; overrides env
.with_token(None)              // explicit anonymous; disables env auto-read (tests, etc.)
```

When a token is set, all GitHub API requests and asset downloads send `Authorization: Bearer <token>`.

No CLI flag for the token — passing tokens on the command line leaks into shell history, `ps` output, and logs. Env or builder only.

## Logging

Renew uses the `log` crate facade only. No `env_logger`, no `tracing`.

| Consumer style | Wire-up | Result |
|----------------|---------|--------|
| `env_logger` (ccu, cr, claude-permit) | `env_logger::init()` (already done) | `log::*` from renew flows through |
| `tracing` (`pd`-style) | `tracing_log::LogTracer::init()?` then `tracing_subscriber::...` | `log::*` from renew is captured as tracing events |

Renew's level conventions: `debug!` for fetch/extract/replace steps; `info!` for "found update", "installed"; `warn!` for "rate limited", "stale cache used due to network failure". No `error!` from renew itself — errors are typed and returned; the consumer chooses when to log them.

## Install path override

Default: `current_exe()`. Overridable via the `install` and `revert` subcommands:

```
ccu update install --install-path /usr/local/bin/ccu
```

Renew does not attempt privilege escalation. If write to the path fails, error is `InstallPath { path, source }` carrying the original `io::Error` (typically `PermissionDenied`).

## Revert semantics

Renew keeps exactly one backup — the binary that was running before the most recent Install.

- `Renew::revert()` swaps the backup binary back into `current_exe()` and **deletes the backup**.
- After revert: `current = previous`, `previous = None`.
- To revert a second time, the Consumer must first Install something. There is no version stack.
- For arbitrary rollback (skip backup, install a specific older release), use `Renew::install_version("0.9.0")` — bypasses backup logic, just installs that tag.
