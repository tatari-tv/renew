#![allow(clippy::unwrap_used)]

use crate::version::parse_current;
use semver::Version;

const REPO: &str = "https://github.com/tatari-tv/renew";

/// All five `renew!()` arms route the current-version argument through the one shared
/// source-selection (`__renew_current_version!`), so they must resolve to the same
/// normalized version — the base semver of this crate's own `BUILD_VERSION`.
#[test]
fn test_macro_all_arms_select_shared_source() {
    let no_arg = crate::renew!().unwrap();
    let bin_only = crate::renew!(bin = "renew").unwrap();
    let repo_only = crate::renew!(repo = REPO).unwrap();
    let repo_bin = crate::renew!(repo = REPO, bin = "renew").unwrap();
    let bin_repo = crate::renew!(bin = "renew", repo = REPO).unwrap();

    let expected = parse_current(env!("BUILD_VERSION")).unwrap();
    for r in [&no_arg, &bin_only, &repo_only, &repo_bin, &bin_repo] {
        assert_eq!(r.current, expected);
    }
}

/// The shared selection returns `BUILD_VERSION` verbatim when the build script set it
/// (non-empty), rather than the raw `CARGO_PKG_VERSION` — this is the whole point of
/// the change. `build-version` always emits a `v` prefix (`vX.Y.Z[-N-g<sha>]`) and
/// `CARGO_PKG_VERSION` never carries one, so the two are textually distinct and a
/// macro that fell through to `CARGO_PKG_VERSION` fails the equality below.
#[test]
fn test_macro_source_returns_build_version_when_present() {
    let selected = crate::__renew_current_version!();
    assert_eq!(selected, env!("BUILD_VERSION"));
    assert!(
        selected.starts_with('v'),
        "build-version emits a v-prefixed tag, got {selected:?}"
    );
    assert_ne!(selected, env!("CARGO_PKG_VERSION"));
}

/// `BUILD_VERSION` is anchored on the Cargo.toml version's own tag (`v$CARGO_PKG_VERSION`,
/// never an older one), so stripping its `v` prefix and describe suffix must land exactly
/// on `CARGO_PKG_VERSION`. The unanchored `git describe --tags` this replaced could name
/// an older tag and fail this.
#[test]
fn test_build_version_normalizes_to_cargo_pkg_version() {
    let normalized = parse_current(env!("BUILD_VERSION")).unwrap();
    assert_eq!(normalized, Version::parse(env!("CARGO_PKG_VERSION")).unwrap());
}
