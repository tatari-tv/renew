# renew

Library for self-update: check, install, and revert versioned GitHub release binaries.

A drop-in for Rust CLIs that ship via GitHub Releases. Wraps the
check-latest / download / verify / atomic-replace / backup / revert flow
behind a small typed API, a passive `notify_if_outdated` helper for
interactive use, and a reusable `clap::Args` subcommand.

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

## Current version source

`renew!()` resolves the running binary's version from **`GIT_DESCRIBE`** — the
`git describe --tags` string a Consumer's `build.rs` emits — when it is set and
non-empty, falling back to `CARGO_PKG_VERSION` otherwise. This matches the version
every fleet Consumer already prints from `--version`, so the "current" version renew
compares is the one the binary actually reports.

`Renew::new` normalizes the describe string before comparing: it strips a leading `v`
and a trailing `-<count>-g<sha>[-dirty]` describe suffix, leaving the base semver
(`v1.2.1-3-gabc123` → `1.2.1`), while preserving a genuine prerelease (`1.2.1-rc.1`).
A bare SHA (untagged build) or an empty/whitespace `GIT_DESCRIBE` is a construction
error — see the note below on handling it.

For this to work the Consumer's `build.rs` must emit `GIT_DESCRIBE`, and should guard
it behind `output.status.success()` so a git failure yields the `CARGO_PKG_VERSION`
fallback rather than an empty string:

```rust
// build.rs
let describe = std::process::Command::new("git")
    .args(["describe", "--tags", "--always"])
    .output()
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
println!("cargo:rustc-env=GIT_DESCRIBE={describe}");
```

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

1. Add `repository = "https://github.com/owner/repo"` to `Cargo.toml`.
2. Add `renew = { git = "https://github.com/tatari-tv/renew", tag = "v0.1.0" }`.
3. In `main.rs`: `renew::renew!()?.notify_if_outdated();` near the top.
4. Add `Update(UpdateCmd)` to your CLI enum and dispatch to `cmd.run(&r)`.
5. Cut a release that includes the changes.

## License

MIT - see `LICENSE`.
