# Integrating `renew` into a CLI

How to give any Tatari Rust CLI self-update (`<bin> update …`) and a passive
"you're behind" notice, wired **identically** across the fleet. `persona-cli` is
the reference implementation; copy these steps verbatim.

## What you get

- **Passive notice** — a one-line "a newer version is out" message on stderr,
  TTY-gated (never prints into a pipe/redirect) and cached (24h TTL), printed on
  every interactive invocation of any command.
- **`<bin> update` subcommand** — `update check | install | revert`, with exit
  codes `0` = current, `1` = update available (for `check`), `2` = error.

## Prerequisites (must already be true)

1. **The CLI ships GitHub Release binaries** in renew's asset scheme:
   `<bin>-vX.Y.Z-<suffix>.tar.gz` for `linux-amd64`, `linux-arm64`,
   `macos-x86_64`, `macos-arm64` (each with a `.sha256` sidecar). Verify with
   `gh release view -R tatari-tv/<repo> --json assets`.
2. **The repo is reachable with the ambient GitHub token** (`GH_TOKEN` /
   `GITHUB_TOKEN`). Private repos work because the fleet already carries a token
   in CI and locally.

If a tool is `cargo install`-only (no release workflow), add the release workflow
first — that is a separate task, not part of this integration.

## Step 1 — canonical `build.rs`

Every consumer emits its running version as `BUILD_VERSION` from `build.rs` through the
fleet's [`build-version`](https://github.com/tatari-tv/build-version-rs) crate. Use
**this exact file** (it is the fleet standard; do not hand-roll a `git describe`):

```rust
fn main() {
    build_version::emit();
}
```

`build_version::emit()` runs `git describe --tags --match "v$CARGO_PKG_VERSION"
--abbrev=7`, anchored on the crate's own Cargo.toml version: `BUILD_VERSION` is `vX.Y.Z`
on the tag, `vX.Y.Z-N-g<sha>` N commits past it, and the bare `vX.Y.Z` before the tag
exists. It can never name an older tag. It also emits `GIT_REVISION`, `GIT_SHA` and
`GIT_BRANCH`, resolves the real git dir (plain repo, workspace member, worktree) for
`cargo:rerun-if-changed`, and fails the build on a shallow checkout (and, in CI, on a
checkout with no git repository or an untracked Cargo.toml), each with a message naming
the fix. A missing tag is not a failure: it emits the bare `vX.Y.Z`. No env var
overrides the version.

Set `--version` from it (clap): `#[command(version = env!("BUILD_VERSION"))]`, and in
`Cargo.toml` `[package]` set `build = "build.rs"`.

## Step 2 — `Cargo.toml`

```toml
[package]
# ... existing fields ...
build = "build.rs"
repository = "https://github.com/tatari-tv/<repo>"   # REQUIRED for the no-arg renew!()

[dependencies]
renew = { git = "https://github.com/tatari-tv/renew", tag = "vX.Y.Z" }

[build-dependencies]
build-version = { git = "https://github.com/tatari-tv/build-version-rs", tag = "v0.1.0", version = "0.1.0" }
```

`repository` is mandatory: the no-arg `renew!()` reads `CARGO_PKG_REPOSITORY` to know
which GitHub repo to check. `<repo>` is the repo the **releases** are published on -
for a workspace/monorepo that is the repo root (e.g. `tatari-tv/marquee`), NOT a
per-crate name, since the GitHub Release and its tarballs live at the repo level. Add
renew with `cargo add renew --git … --tag vX.Y.Z` (during development against an
unmerged renew, a `path`/`branch` dep; repin to the tag before release).

> **Package name vs binary name.** The no-arg `renew!()` also uses `CARGO_PKG_NAME`
> as the binary/asset name it looks for (`<name>-vX.Y.Z-<platform>.tar.gz`). If your
> crate name differs from the shipped binary - e.g. package `marquee-cli` but binary
> and assets `marquee` - the no-arg macro will look for `marquee-cli-v*` assets and
> fail at runtime (it still compiles and passes CI). Override it with
> `renew!(bin = "marquee")` at the call sites in Step 4. Most CLIs name the package
> after the binary and never hit this; check `[[bin]]` in your `Cargo.toml` if unsure.

## Step 3 — `src/cli.rs`

Add one variant to your top-level subcommand enum:

```rust
#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    // ... existing commands ...
    /// Check for, install, or revert a newer released version of <bin>
    Update(renew::UpdateCmd),
}
```

`renew::UpdateCmd` is `#[derive(clap::Args, Debug)]`, so it drops straight into a
`Subcommand` enum and brings its own `check|install|revert` sub-subcommands.

## Step 4 — `src/main.rs` wiring

Two insertions, immediately after logging is set up and **before any
app-specific setup** (config load, auth, DB, etc.). This is the one place the code
around the pattern differs per CLI: the *pattern* is identical, but "your setup" is
whatever your tool builds before dispatching.

```rust
setup_logging(…)?;   // or your equivalent

// `update check|install|revert` needs none of this CLI's own setup (config, auth,
// …), so intercept it here, before that setup runs. A renew!() construction failure
// (unparseable version, missing repo metadata) maps to renew's documented exit 2.
if let Commands::Update(cmd) = &args.command {
    match renew::renew!() {
        Ok(r) => std::process::exit(cmd.run(&r)),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

// Passive "a newer version is out" notice (TTY-gated, cached) for every other
// command. Degrade to a debug log on a construction failure - NEVER abort the
// command the user actually ran.
match renew::renew!() {
    Ok(r) => r.notify_if_outdated(),
    Err(e) => log::debug!("renew unavailable: {e}"),
}

// ... now your config load / auth / dispatch ...
```

Because `Update` is intercepted above (and both arms diverge via
`std::process::exit`), your main dispatch `match` never handles it — add an
explicit unreachable arm so the match stays exhaustive and the intent is documented:

```rust
// Handled by the early intercept above, which exits the process.
Commands::Update(_) => unreachable!("Update is intercepted before <your setup>"),
```

### Why the wiring looks like this (do not "simplify" it)

- **Never `renew!()?`.** `renew!()` returns `Result` and parses the current version
  eagerly, so an unparseable version is an `Err` *before* `notify_if_outdated` can
  run. `renew!()?.notify_if_outdated()` would abort every command on that `Err`.
  The passive path must match-and-drop; the `update` path maps `Err` to exit 2.
- **Intercept `update` before your setup.** `update` needs no config/auth/DB, and a
  user who is (e.g.) logged out must still be able to run `update install`.
- **Override `bin` when the package name isn't the binary name.** Use
  `renew!(bin = "<binary>")` at BOTH call sites (the intercept and the passive notice)
  when `CARGO_PKG_NAME` differs from the shipped binary/asset name - see the callout in
  Step 2. `marquee-cli` shipping the `marquee` binary is the canonical case:
  `renew!(bin = "marquee")`. Leave it as the no-arg `renew!()` when they match (the
  common case; `persona-cli`'s package IS `persona`, so it uses the no-arg form).

## Step 5 — verify (before you ship)

Run against the real published release. Redirect logs to a writable dir if your
tool logs under `~/.local/share` (`XDG_DATA_HOME=$(mktemp -d)`).

```bash
# current build == latest release -> exit 0
<bin> update check ; echo $?            # "<bin> X.Y.Z (latest)" ; 0

# force an older running version -> exit 1: build from an older tag's checkout
# (build-version has no env override; BUILD_VERSION comes from the checked-out tag)
git checkout vOLD && cargo build && git checkout -
<bin> update check ; echo $?            # "OLD → X.Y.Z available …" ; 1

# update check reaches a decision with NO auth present (proves the early intercept)

# raw --version and the version renew reports refer to the same release
<bin> --version                         # e.g. vX.Y.Z (or vX.Y.Z-N-gSHA-dirty on a dev build)

# passive notice is TTY-gated: present on a terminal, absent when redirected
# (on the older-tag build from above)
script -qefc "<bin> <some-cmd>" /tmp/tty.out ; grep 'new version' /tmp/tty.out   # present
<bin> <some-cmd> 2>/tmp/redir.err ; grep -c 'new version' /tmp/redir.err          # 0
```

## Reference

- **Version normalization.** renew's `Renew::new` runs the `BUILD_VERSION` string
  through `parse_current`: strips a leading `v` and a trailing `-N-gSHA[-dirty]`
  describe suffix down to the base semver (`v1.2.1-3-gabc-dirty` → `1.2.1`), while
  preserving a genuine prerelease (`1.2.1-rc.1`). You do not normalize anything
  yourself; hand renew the raw `BUILD_VERSION`.
- **`BUILD_VERSION` fallback.** The `renew!()` macro reads `option_env!("BUILD_VERSION")`
  at the consumer's call site, drops an empty/whitespace value, and falls back to
  `CARGO_PKG_VERSION`; a consumer with no `build.rs` therefore reports its Cargo.toml
  version, which on a tagged release build is the same thing.
- **Exit codes** (`update`): `0` current / installed, `1` update available (`check`
  only), `2` error.
- **Forcing the passive notice.** The passive notice is TTY-gated (silent in a pipe/
  redirect). Set `RENEW_FORCE_NOTIFY=1` (fleet-wide, honored by every renew consumer) to
  bypass the TTY gate and print it regardless - for testing/debugging the notify path.
  It only bypasses the gate: an up-to-date binary still prints nothing, and a check
  error still degrades silently. To see a check result on demand instead, use
  `<bin> update check --refresh`.
- **Overriding defaults** (TTL, install path, token, timeout) — use the explicit
  `Renew::new(repo, bin, env!("BUILD_VERSION"))` builder instead of `renew!()`. The
  no-arg macro is the fleet default; only reach for the builder when you must.
</content>
