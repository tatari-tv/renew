// Simple pattern for git describe -> version
use std::process::Command;

fn main() {
    // NO `--always`: on a shallow/tagless checkout (exactly how rust-ci.yml checks
    // out) `--always` emits a bare short SHA, which `version::parse_current` rejects
    // as non-semver and panics the version tests. Without it, `git describe --tags`
    // exits non-zero when no tag is reachable, routing into the CARGO_PKG_VERSION
    // fallback below so GIT_DESCRIBE is always a valid semver string.
    let git_describe = Command::new("git")
        .args(["describe", "--tags"])
        .output()
        .and_then(|output| {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                Err(std::io::Error::other("git describe failed"))
            }
        })
        .unwrap_or_else(|_| {
            // Fallback to Cargo.toml version when git describe fails
            env!("CARGO_PKG_VERSION").to_string()
        });

    println!("cargo:rustc-env=GIT_DESCRIBE={}", git_describe);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");
}
