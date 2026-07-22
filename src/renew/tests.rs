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
fn test_preflight_ok_when_target_absent_but_parent_writable() {
    let tmp = tempfile::tempdir().unwrap();
    let r = make_renew().with_install_path(tmp.path().join("ccu"));
    assert!(r.preflight().is_ok());
}

#[test]
fn test_preflight_ok_when_target_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("ccu");
    std::fs::write(&target, b"old binary").unwrap();
    let r = make_renew().with_install_path(target);
    assert!(r.preflight().is_ok());
}

#[test]
fn test_preflight_errors_when_parent_missing() {
    let tmp = tempfile::tempdir().unwrap();
    // Parent directory does not exist -> the sentinel write fails -> preflight errors.
    let r = make_renew().with_install_path(tmp.path().join("no-such-dir").join("ccu"));
    let err = r.preflight().unwrap_err();
    assert!(
        matches!(err, Error::InstallPath { .. }),
        "expected InstallPath, got {err:?}"
    );
}

/// Regression for the Linux self-update `ETXTBSY` bug: preflight probed the target with
/// `OpenOptions::write(true).open(target)`, which fails with "text file busy" when the target
/// is the running executable - aborting every in-place self-update on Linux even though the
/// rename-based replace would succeed. Preflight must now probe the parent dir instead.
///
/// We copy a real system binary, execute it, wait until the kernel actually reports the file
/// as busy (write-open returns `ETXTBSY`), then assert preflight still succeeds. The wait makes
/// the precondition deterministic (no race on `exec`); on a platform that never returns
/// `ETXTBSY` the test skips rather than giving a false pass/fail.
#[cfg(unix)]
#[test]
fn test_preflight_ok_when_target_is_running_binary() {
    use std::os::unix::fs::PermissionsExt;

    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists());
    let Some(sleep) = sleep else {
        return; // no `sleep` available; nothing to exercise
    };

    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("running-bin");
    std::fs::copy(&sleep, &target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = std::process::Command::new(&target)
        .arg("30")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    // Wait until the target is genuinely busy-for-write (exec completed). Bounded so a
    // platform without ETXTBSY semantics skips instead of hanging or falsely failing.
    let mut busy = false;
    for _ in 0..200 {
        match std::fs::OpenOptions::new().write(true).open(&target) {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                busy = true;
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    }

    let result = if busy {
        Some(make_renew().with_install_path(target.clone()).preflight())
    } else {
        None
    };

    let _ = child.kill();
    let _ = child.wait();

    // If the precondition never held (platform without ETXTBSY semantics), skip the assert.
    if let Some(r) = result {
        assert!(
            r.is_ok(),
            "preflight must succeed for a running-binary target (rename-based replace works): {r:?}"
        );
    }
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
