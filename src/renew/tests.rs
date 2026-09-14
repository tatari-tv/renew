#![allow(clippy::unwrap_used)]

use super::*;
use crate::fixture::{CannedApi, hold_refresh_lock, offline_renew, seed_cache};

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

/// The passive notice must keep swallowing a failed check, because it rides on commands
/// the user actually ran. `RENEW_FORCE_NOTIFY` bypasses the TTY gate so the swallow is
/// exercised rather than skipped under `cargo test`.
#[test]
fn test_notify_if_outdated_swallows_a_failed_check() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var(FORCE_NOTIFY_ENV).ok();
    unsafe { std::env::set_var(FORCE_NOTIFY_ENV, "1") };

    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    let r = offline_renew(&api, tmp.path()).with_cache_ttl(Duration::from_secs(0));
    assert!(r.check_latest().is_err(), "precondition: the check must fail");

    r.notify_if_outdated();

    match prior {
        Some(v) => unsafe { std::env::set_var(FORCE_NOTIFY_ENV, v) },
        None => unsafe { std::env::remove_var(FORCE_NOTIFY_ENV) },
    }
    drop(guard);
}

/// The check failed and there was nothing cached, so there is nothing to report. The old
/// body bound the result and asserted nothing, which let `Ok(None)` - "you are up to
/// date", from a check that never happened - through for the life of the crate.
#[test]
fn test_check_latest_returns_error_without_network() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    let r = offline_renew(&api, tmp.path()).with_cache_ttl(Duration::from_secs(0));

    let result = r.check_latest();

    assert!(
        matches!(&result, Err(Error::CheckFailed { stale: None, .. })),
        "a failed check must not report success: {result:?}"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.starts_with("the check did not succeed and there was no usable cache"),
        "message must name what happened: {msg}"
    );
    assert!(
        msg.contains("no release found for tatari-tv/ccu"),
        "message must name the cause it wraps: {msg}"
    );
}

/// A private-repo 404 is the reason the variant exists: on its own it renders as "no
/// release found for <repo>", which tells the user the repo has no releases when the
/// truth is that their token cannot see it.
#[test]
fn test_failed_check_does_not_render_as_a_bare_no_release() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    let r = offline_renew(&api, tmp.path()).with_cache_ttl(Duration::from_secs(0));

    let msg = r.check_latest().unwrap_err().to_string();

    assert!(
        !msg.starts_with("no release found"),
        "a failed check must not masquerade as an empty repo: {msg}"
    );
}

/// A stale cache can inform, never vote: the value and its `checked-at` ride along as
/// context and the call still fails, because the check did not happen.
#[test]
fn test_failed_check_with_cache_still_errors_and_carries_the_cache_as_context() {
    let api = CannedApi::status(500, "Internal Server Error");
    let tmp = tempfile::tempdir().unwrap();
    seed_cache(tmp.path(), "9.9.9", Duration::from_secs(48 * 60 * 60));
    let r = offline_renew(&api, tmp.path());

    let result = r.check_latest();

    assert!(
        matches!(&result, Err(Error::CheckFailed { stale: Some(_), .. })),
        "a cached value is context, not a verdict: {result:?}"
    );
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("9.9.9"), "cached value must print as context: {msg}");
    assert!(msg.contains("checked at"), "cached checked-at must print: {msg}");
}

/// The network path's own fabrication: an unparseable release timestamp must fail
/// honestly rather than silently become "today". `src/renew.rs` used to do exactly that
/// via `info.published_at.parse().unwrap_or(Utc::now())`.
#[test]
fn test_network_path_does_not_fabricate_an_unparseable_release_date() {
    let body = r#"{
        "tag_name": "v9.9.9",
        "html_url": "https://github.com/tatari-tv/ccu/releases/tag/v9.9.9",
        "published_at": "not-a-date",
        "assets": []
    }"#;
    let api = CannedApi::json(200, "OK", body);
    let tmp = tempfile::tempdir().unwrap();
    let r = offline_renew(&api, tmp.path()).with_cache_ttl(Duration::from_secs(0));

    let result = r.check_latest();

    assert!(
        matches!(&result, Err(Error::InvalidPublishedAt { .. })),
        "an unparseable release date must be a named error, not Utc::now(): {result:?}"
    );
    assert!(
        cache::load(tmp.path()).is_none(),
        "bad data must not be cached, so a later working check is not corrupted"
    );
}

/// A peer holding the refresh lock means a check IS happening, so the cache still
/// reports. This is the one fallthrough that is not a failure.
#[test]
fn test_lock_held_by_peer_still_reports_from_cache() {
    let api = CannedApi::status(500, "Internal Server Error");
    let tmp = tempfile::tempdir().unwrap();
    seed_cache(tmp.path(), "9.9.9", Duration::from_secs(0));
    let lock = hold_refresh_lock(tmp.path());
    let r = offline_renew(&api, tmp.path()).with_cache_ttl(Duration::from_secs(0));

    let update = r
        .check_latest()
        .unwrap()
        .expect("cache should report while a peer refreshes");

    assert_eq!(update.latest.to_string(), "9.9.9");
    drop(lock);
}

/// Same fallthrough with nothing cached: there is no report to make, so it is an error
/// rather than a silent "up to date".
#[test]
fn test_lock_held_by_peer_with_empty_cache_is_an_error() {
    let api = CannedApi::status(500, "Internal Server Error");
    let tmp = tempfile::tempdir().unwrap();
    let lock = hold_refresh_lock(tmp.path());
    let r = offline_renew(&api, tmp.path()).with_cache_ttl(Duration::from_secs(0));

    let result = r.check_latest();

    assert!(
        matches!(&result, Err(Error::CheckFailed { stale: None, .. })),
        "nothing to report is an error, not success: {result:?}"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("refreshing the update cache"),
        "message must name the cause: {msg}"
    );
    drop(lock);
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
