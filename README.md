# renew

Library for self-update: check, install, and revert versioned GitHub release binaries.

A drop-in for Rust CLIs that ship via GitHub Releases. Wraps the
check-latest / download / verify / atomic-replace / backup / revert flow
behind a small typed API, a passive `notify_if_outdated` helper for
interactive use, and a reusable `clap::Args` subcommand.

**Integrating renew into a fleet CLI?** Follow the step-by-step
[integration guide](docs/integration-guide.md) - the canonical recipe
(`build.rs`, `Cargo.toml`, `cli.rs`, `main.rs` wiring) so every CLI integrates
identically. `persona-cli` is the reference implementation.

## Quickstart

Add the dep, pinned to a tag:

```toml
[dependencies]
renew = { git = "https://github.com/tatari-tv/renew", tag = "v0.1.0" }
```

Set `[package].repository` in your `Cargo.toml` (the macro reads it):

```toml
[package]
name = "ccu"
version = "0.4.3"
repository = "https://github.com/tatari-tv/claude-cost-usage"
```

Wire it into `main.rs`:

```rust
use clap::Parser;
use renew::{renew, UpdateCmd};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    Today,
    Update(UpdateCmd),
}

fn main() {
    env_logger::init();

    match Cli::parse().cmd {
        Cmd::Today => {
            // Passive notice on stderr if a newer version is out (TTY-gated).
            // Degrade to a debug log on a construction error; never abort.
            match renew!() {
                Ok(r) => r.notify_if_outdated(),
                Err(e) => log::debug!("renew unavailable: {e}"),
            }
            /* ... */
        }
        // The `update` subcommand maps a construction error to renew's exit 2.
        Cmd::Update(cmd) => match renew!() {
            Ok(r) => std::process::exit(cmd.run(&r)),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        },
    }
}
```

That's the full integration. `cargo run -- update check` / `update install --yes` /
`update revert --yes` work out of the box.

> **If your package name isn't your binary name**, override it: `renew!(bin = "mybin")`.
> The no-arg `renew!()` uses `CARGO_PKG_NAME` as the asset name it downloads
> (`<name>-vX.Y.Z-<platform>.tar.gz`), so a crate named `marquee-cli` that ships a
> `marquee` binary must use `renew!(bin = "marquee")` - otherwise it looks for
> `marquee-cli-v*` assets and fails at runtime (it still compiles and passes CI).
> Use the same `bin =` override at every call site.

## Current version source

`renew!()` resolves the running binary's version from **`BUILD_VERSION`**, the
value the fleet's [`build-version`](https://github.com/tatari-tv/build-version-rs)
crate emits from a Consumer's `build.rs`, when it is set and non-empty, falling back
to `CARGO_PKG_VERSION` otherwise. `BUILD_VERSION` is anchored on the Consumer's own
Cargo.toml version tag: `vX.Y.Z` on the tag, `vX.Y.Z-N-g<sha>` N commits past it, and
the bare `vX.Y.Z` before the tag exists. This matches the version every fleet Consumer
prints from `--version`, so the "current" version renew compares is the one the binary
actually reports.

`Renew::new` normalizes the string before comparing: it strips a leading `v` and a
trailing `-<count>-g<sha>[-dirty]` describe suffix, leaving the base semver
(`v1.2.1-3-gabc123` → `1.2.1`), while preserving a genuine prerelease (`1.2.1-rc.1`).
A bare SHA is a construction error (see the note below on handling it); an empty or
whitespace `BUILD_VERSION` is dropped by `renew!()`, which then falls back to
`CARGO_PKG_VERSION`.

For this to work the Consumer's `build.rs` calls `build_version::emit()`:

```toml
# Cargo.toml
[build-dependencies]
build-version = { git = "https://github.com/tatari-tv/build-version-rs", tag = "v0.1.0", version = "0.1.0" }
```

```rust
// build.rs
fn main() {
    build_version::emit();
}
```

A Consumer without a `build.rs` has no `BUILD_VERSION` at all, so `renew!()` reads its
`CARGO_PKG_VERSION`; that is the release version on a tagged build.

### Handling a construction error

`renew!()` returns `Result` and parses the current version eagerly, so an unparseable
version (bare SHA, malformed describe) is an `Err` **before** `notify_if_outdated` runs.
Do not `?`-propagate it on the passive-notice path — that would abort every command.
Degrade instead, and reserve the loud exit for the `update` subcommand:

```rust
// Passive notice: degrade to a debug log, never abort other commands.
match renew::renew!() {
    Ok(r) => r.notify_if_outdated(),
    Err(e) => log::debug!("renew unavailable: {e}"),
}
```

The passive notice is TTY-gated (silent in a pipe/redirect). Set `RENEW_FORCE_NOTIFY=1`
to bypass the TTY gate and print it regardless (testing/debugging the notify path); it
only bypasses the gate, so an up-to-date binary still prints nothing.

## Explicit form

When you need to override defaults (TTL, install path, token, timeout):

```rust
use renew::Renew;
use std::time::Duration;

let r = Renew::new("tatari-tv/claude-cost-usage", "ccu", "0.4.3")?
    .with_cache_ttl(Duration::from_secs(3600))
    .with_install_path("/usr/local/bin/ccu".into())
    .with_token(std::env::var("GH_TOKEN").ok());

match r.check_latest()? {
    Some(update) => println!("new: {}", update.latest),
    None => println!("up to date"),
}
```

The `Renew::new` constructor accepts bare slugs (`tatari-tv/repo`), HTTPS URLs,
and SSH URLs (`git@github.com:owner/repo.git`).

## `tracing` consumers

`renew` emits `log` records. Consumers using `tracing` can bridge with `tracing-log`:

```toml
[dependencies]
tracing-log = "0.2"
```

```rust
tracing_log::LogTracer::init().expect("log bridge");
```

All `log::debug!` / `log::info!` calls inside `renew` then flow through your
`tracing` subscriber.

## Producer-side release contract

For `renew` to install your binary, your release pipeline must publish:

| File    | Format                                       | Notes                                              |
|---------|----------------------------------------------|----------------------------------------------------|
| Tag     | `v<semver>`                                  | annotated, on `main`                               |
| Tarball | `<bin>-<tag>-<platform>.tar.gz`              | exactly one regular file inside                    |
| Sidecar | `<bin>-<tag>-<platform>.tar.gz.sha256`       | first 64 chars are the lowercase hex digest        |

`<platform>` is one of: `linux-amd64`, `linux-arm64`, `macos-x86_64`, `macos-arm64`.

`<bin>` is the binary/asset name renew looks for: `CARGO_PKG_NAME` with the no-arg
`renew!()`, or the value you pass to `renew!(bin = "…")` / `Renew::new(repo, bin, …)`.
It must match the asset base name your release pipeline actually publishes - if the
crate name and binary name differ, pass `bin` explicitly (see the Quickstart note).

The tarball must contain exactly one regular file (the binary).
Multi-file tarballs are rejected. The sidecar may use any of the common
formats: `<hex>`, `<hex>  <name>`, or `<hex> *<name>`.

A reference release workflow lives in `.github/workflows/release.yml`.

## Platform support

| OS      | Arch     | Asset platform                                       |
|---------|----------|------------------------------------------------------|
| Linux   | x86_64   | `linux-amd64`                                        |
| Linux   | aarch64  | `linux-arm64`                                        |
| macOS   | x86_64   | `macos-x86_64`                                       |
| macOS   | aarch64  | `macos-arm64`                                        |
| Linux   | musl     | rejected (`AssetMissing`); install from cargo source |
| Windows | any      | not supported                                        |

musl builds are rejected at runtime: glibc binaries from a release tarball
will not run under musl, and silently replacing the binary would crash the
next invocation with a dynamic-linker error.

## Authentication

Anonymous works for public repos. For private repos, set `GH_TOKEN` or
`GITHUB_TOKEN` in the environment, or pass an explicit token via
`with_token`. Order: `GH_TOKEN` first, then `GITHUB_TOKEN`.

The token is used for the GitHub API request and the initial download
request. On the 302 redirect from `github.com` to S3, the auth header
is stripped (`RedirectAuthHeaders::Never`). The token is never forwarded
to a presigned third-party URL.

## File locations

| Purpose                      | Path                                                                         |
|------------------------------|------------------------------------------------------------------------------|
| Cache (latest-version check) | `dirs::cache_dir()/<bin>/check.yml`                                          |
| Lock (refresh serialization) | `dirs::cache_dir()/<bin>/check.lock`                                         |
| Backup directory             | `dirs::data_local_dir()/<bin>/<sha256(canonical install_path)[:12]>/backup/` |

Backup directories are keyed by install path, so a binary installed at
`~/.cargo/bin/ccu` and an experimental copy at `/tmp/ccu` keep independent
backups; reverting the stable install can never restore the experimental one.

## Exit codes (`UpdateCmd`)

| Subcommand       | 0                 | 1                | 2     |
|------------------|-------------------|------------------|-------|
| `update check`   | up to date        | update available | error |
| `update install` | installed / abort | -                | error |
| `update revert`  | reverted / abort  | -                | error |

User-typed `N` (or anything that isn't `y`/`yes`) at a confirmation prompt
exits cleanly with code 0; the prompt defaults to `N` on empty input.

If stdin is not a TTY and `--yes` was not passed, the prompt errors with
`PromptRequiredButStdinNotTty` and exits 2 - statusline-style invocations
that pipe stderr to `/dev/null` will never block on input.

## Rolling out to a new consumer

1. Add `repository = "https://github.com/owner/repo"` to `Cargo.toml` (the repo the
   releases are published on - the repo root for a monorepo).
2. Add `renew = { git = "https://github.com/tatari-tv/renew", tag = "v0.1.0" }`.
3. In `main.rs`, near the top, match-and-degrade on the passive notice - do **not**
   `?`-propagate it (a construction error would abort every command; see
   [Handling a construction error](#handling-a-construction-error)):
   ```rust
   match renew::renew!() {
       Ok(r) => r.notify_if_outdated(),
       Err(e) => log::debug!("renew unavailable: {e}"),
   }
   ```
4. Add `Update(UpdateCmd)` to your CLI enum and dispatch to `cmd.run(&r)`.
5. If your package name isn't your binary name, use `renew!(bin = "<binary>")` at
   every call site (see the Quickstart note).
6. Cut a release that includes the changes.

For the full copy-paste recipe (canonical `build.rs`, exact `main.rs` wiring, and a
verification script) follow the [integration guide](docs/integration-guide.md).

## License

MIT - see `LICENSE`.
