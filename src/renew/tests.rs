#![allow(clippy::unwrap_used)]

use super::*;

fn make_renew() -> Renew {
    Renew::new("tatari-tv/ccu", "ccu", "0.4.3").unwrap()
}

#[test]
fn test_new_with_bare_slug() {
    let r = make_renew();
    assert_eq!(r.bin, "ccu");
    assert_eq!(r.current.to_string(), "0.4.3");
    assert_eq!(r.repo.as_path(), "tatari-tv/ccu");
}

#[test]
fn test_new_with_https_url() {
    let r = Renew::new("https://github.com/tatari-tv/ccu", "ccu", "0.1.0").unwrap();
    assert_eq!(r.repo.as_path(), "tatari-tv/ccu");
}

#[test]
fn test_new_rejects_bad_repo() {
    let err = Renew::new("not-valid", "bin", "0.1.0").unwrap_err();
    assert!(err.to_string().contains("owner/repo"));
}

#[test]
fn test_new_rejects_bad_version() {
    let err = Renew::new("tatari-tv/ccu", "ccu", "not-semver").unwrap_err();
    assert!(err.to_string().contains("not-semver"));
}

#[test]
fn test_default_cache_ttl_is_24h() {
    let r = make_renew();
    assert_eq!(r.cache_ttl.as_secs(), 24 * 60 * 60);
}

#[test]
fn test_with_cache_ttl() {
    let r = make_renew().with_cache_ttl(Duration::from_secs(3600));
    assert_eq!(r.cache_ttl.as_secs(), 3600);
}

#[test]
fn test_with_token_explicit() {
    let r = make_renew().with_token(Some("mytoken".to_string()));
    assert_eq!(r.resolve_token(), Some("mytoken".to_string()));
}

#[test]
fn test_with_token_none_explicit_returns_none() {
    // Explicit(None) means "no token regardless of env vars"
    let r = make_renew().with_token(None);
    assert_eq!(r.resolve_token(), None);
}

#[test]
fn test_with_install_path() {
    let path = PathBuf::from("/tmp/ccu");
    let r = make_renew().with_install_path(path.clone());
    assert_eq!(r.install_path, Some(path));
}

#[test]
fn test_default_install_path_is_none() {
    let r = make_renew();
    assert!(r.install_path.is_none());
}

#[test]
fn test_cache_dir_includes_bin_name() {
    let r = make_renew();
    assert!(r.cache_dir.ends_with("ccu"));
}

#[test]
fn test_data_dir_includes_bin_name() {
    let r = make_renew();
    assert!(r.data_dir.ends_with("ccu"));
}

#[test]
fn test_has_backup_false_without_backup_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let r = make_renew().with_install_path(tmp.path().join("ccu"));
    // No backup created yet
    assert!(!r.has_backup());
}
