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

Every consumer emits its running version as `GIT_DESCRIBE` from `build.rs`. Use
**this exact file** (it is the fleet standard — do not hand-roll a variant):

```rust
// Canonical fleet build.rs: emits GIT_DESCRIBE for the crate's `--version`.
//
// Resolves the real git directory via `git rev-parse` instead of assuming a `.git/`
// sits next to Cargo.toml, so `cargo:rerun-if-changed` is correct for:
//   - a regular single-crate repo (`.git/` at the crate root),
//   - a workspace member (`.git/` at the workspace root, above the crate), and
//   - a git worktree, including bare-container worktrees (`.git` is a gitdir file).
// Falls back to CARGO_PKG_VERSION when git is unavailable (e.g. a source tarball).
use std::process::Command;

/// Run `git` with the given args; return trimmed stdout, or None if git is absent,
/// the command failed, or the output was empty.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn main() {
    // Precedence: an explicit GIT_DESCRIBE env (CI pinning, or forcing a version in
    // tests) wins; else `git describe`; else the Cargo.toml version.
    println!("cargo:rerun-if-env-changed=GIT_DESCRIBE");
    let describe = std::env::var("GIT_DESCRIBE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| git(&["describe", "--tags", "--always", "--dirty"]))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=GIT_DESCRIBE={describe}");

    // Rebuild when HEAD moves or tags change. HEAD lives in the (per-worktree) git
    // dir; refs and packed-refs live in the common dir. cargo resolves a relative
    // rerun path against this crate's manifest dir (= the build script's CWD), the
    // same base `git rev-parse` prints relative paths against - so both a plain
    // `.git` and an absolute worktree gitdir resolve correctly.
    if let Some(git_dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
    }
    if let Some(common_dir) = git(&["rev-parse", "--git-common-dir"]) {
        println!("cargo:rerun-if-changed={common_dir}/refs");
        println!("cargo:rerun-if-changed={common_dir}/packed-refs");
    }
}
```

Why not just `.git/HEAD`? A hardcoded `.git/HEAD` rerun path only resolves when the
crate sits at the repo root. In a **workspace member** the `.git/` is above the
crate, and in a **worktree** `.git` is a gitdir file pointing elsewhere - so the
rebuild-on-tag trigger silently never fires. Resolving via `git rev-parse` is
correct in all three layouts (verified against a regular repo, a workspace member,
and a bare-container worktree).

Set `--version` from it (clap): `#[command(version = env!("GIT_DESCRIBE"))]`, and in
`Cargo.toml` `[package]` set `build = "build.rs"`.

## Step 2 — `Cargo.toml`

```toml
[package]
# ... existing fields ...
build = "build.rs"
repository = "https://github.com/tatari-tv/<repo>"   # REQUIRED for the no-arg renew!()

[dependencies]
renew = { git = "https://github.com/tatari-tv/renew", tag = "vX.Y.Z" }
```

`repository` is mandatory: the no-arg `renew!()` reads `CARGO_PKG_REPOSITORY` to know
which GitHub repo to check. Add renew with `cargo add renew --git … --tag vX.Y.Z`
(during development against an unmerged renew, a `path`/`branch` dep; repin to the
tag before release).

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

## Step 5 — verify (before you ship)

Run against the real published release. Redirect logs to a writable dir if your
tool logs under `~/.local/share` (`XDG_DATA_HOME=$(mktemp -d)`).

```bash
# current build == latest release -> exit 0
<bin> update check ; echo $?            # "<bin> X.Y.Z (latest)" ; 0

# force an older running version -> exit 1 (uses the build.rs env override)
GIT_DESCRIBE=v0.0.1 cargo build
<bin> update check ; echo $?            # "0.0.1 → X.Y.Z available …" ; 1

# update check reaches a decision with NO auth present (proves the early intercept)

# raw --version and the version renew reports refer to the same release
<bin> --version                         # e.g. vX.Y.Z (or vX.Y.Z-N-gSHA-dirty on a dev build)

# passive notice is TTY-gated: present on a terminal, absent when redirected
GIT_DESCRIBE=v0.0.1 cargo build
script -qefc "<bin> <some-cmd>" /tmp/tty.out ; grep 'new version' /tmp/tty.out   # present
<bin> <some-cmd> 2>/tmp/redir.err ; grep -c 'new version' /tmp/redir.err          # 0
```

## Reference

- **Version normalization.** renew's `Renew::new` runs the `GIT_DESCRIBE` string
  through `parse_current`: strips a leading `v` and a trailing `-N-gSHA[-dirty]`
  describe suffix down to the base semver (`v1.2.1-3-gabc-dirty` → `1.2.1`), while
  preserving a genuine prerelease (`1.2.1-rc.1`). You do not normalize anything
  yourself; hand renew the raw `GIT_DESCRIBE`.
- **`GIT_DESCRIBE` empty guard.** The `renew!()` macro also drops an empty/whitespace
  `GIT_DESCRIBE` and falls back to `CARGO_PKG_VERSION`, defense-in-depth with the
  `build.rs` `status.success()` guard.
- **Exit codes** (`update`): `0` current / installed, `1` update available (`check`
  only), `2` error.
- **Overriding defaults** (TTL, install path, token, timeout) — use the explicit
  `Renew::new(repo, bin, env!("GIT_DESCRIBE"))` builder instead of `renew!()`. The
  no-arg macro is the fleet default; only reach for the builder when you must.
</content>
