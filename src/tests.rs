#![allow(clippy::unwrap_used)]

use crate::version::parse_current;

const REPO: &str = "https://github.com/tatari-tv/renew";

/// All five `renew!()` arms route the current-version argument through the one shared
/// source-selection (`__renew_current_version!`), so they must resolve to the same
/// normalized version — the base semver of this crate's own `GIT_DESCRIBE`.
#[test]
fn test_macro_all_arms_select_shared_source() {
    let no_arg = crate::renew!().unwrap();
    let bin_only = crate::renew!(bin = "renew").unwrap();
    let repo_only = crate::renew!(repo = REPO).unwrap();
    let repo_bin = crate::renew!(repo = REPO, bin = "renew").unwrap();
    let bin_repo = crate::renew!(bin = "renew", repo = REPO).unwrap();

    let expected = parse_current(env!("GIT_DESCRIBE")).unwrap();
    for r in [&no_arg, &bin_only, &repo_only, &repo_bin, &bin_repo] {
        assert_eq!(r.current, expected);
    }
}

/// The shared selection returns `GIT_DESCRIBE` verbatim when the build script set it
/// (non-empty), rather than the raw `CARGO_PKG_VERSION` — this is the whole point of
/// the change. Under a normal git build `GIT_DESCRIBE` carries a `v` prefix and/or a
/// describe suffix, so it is textually distinct from `CARGO_PKG_VERSION`.
#[test]
fn test_macro_source_returns_git_describe_when_present() {
    let selected = crate::__renew_current_version!();
    assert_eq!(selected, env!("GIT_DESCRIBE"));
    assert!(!selected.trim().is_empty());
}
