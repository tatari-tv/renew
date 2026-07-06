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

#[test]
fn test_notify_if_outdated_does_not_panic_on_network_error() {
    // check_latest will fail (no real GitHub access, bad repo slug triggers early error)
    // notify_if_outdated must swallow the error silently
    let r = make_renew();
    // This should not panic regardless of network state
    r.notify_if_outdated();
}

#[test]
fn test_check_latest_returns_error_without_network() {
    // With a zero TTL and no network, check_latest should return an error or Ok(None).
    // We just verify it doesn't panic.
    let r = make_renew().with_cache_ttl(Duration::from_secs(0));
    let result = r.check_latest();
    // Either Ok (stale-cache fallback) or Err (network failure) - both are valid
    let _ = result;
}

// Serialize env-var-touching tests to prevent parallel races (see rust conventions).
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_force_notify_env_truthiness() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var(FORCE_NOTIFY_ENV).ok();

    for (val, expected) in [
        ("1", true),
        ("true", true),
        ("TRUE", true),
        ("yes", true),
        ("on", true),
        ("0", false),
        ("false", false),
        ("False", false),
        ("", false),
        ("   ", false),
    ] {
        unsafe { std::env::set_var(FORCE_NOTIFY_ENV, val) };
        assert_eq!(force_notify(), expected, "RENEW_FORCE_NOTIFY={val:?}");
    }
    unsafe { std::env::remove_var(FORCE_NOTIFY_ENV) };
    assert!(!force_notify(), "unset -> false");

    match prior {
        Some(v) => unsafe { std::env::set_var(FORCE_NOTIFY_ENV, v) },
        None => unsafe { std::env::remove_var(FORCE_NOTIFY_ENV) },
    }
    drop(guard); // hold the lock for the whole test; explicit drop keeps the binding used
}
