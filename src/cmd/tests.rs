#![allow(clippy::unwrap_used)]

use super::*;
use crate::fixture::{CannedApi, offline_renew, seed_cache, seed_cache_with, seed_legacy_cache};
use chrono::{TimeZone, Utc};
use std::time::Duration;

fn make_renew() -> Renew {
    Renew::new("tatari-tv/ccu", "ccu", "0.4.3").unwrap()
}

#[test]
fn test_confirm_returns_true_when_yes_flag_set() {
    assert!(confirm("prompt", true).unwrap());
}

#[test]
fn test_confirm_errors_when_stdin_not_tty_and_no_yes_flag() {
    // In the test environment stdin is not a TTY, so without --yes it errors.
    let result = confirm("prompt", false);
    assert!(matches!(result, Err(Error::PromptRequiredButStdinNotTty)));
}

/// renew's documented contract (see [`UpdateCmd::run`]): 0 = current, 1 = update
/// available, 2 = error. A check that did not succeed is 2, with or without `--refresh`.
/// The old body accepted `[0, 1, 2]`, which is every code the function can return.
#[test]
fn test_run_check_exits_2_when_the_check_fails() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    let renew = offline_renew(&api, tmp.path());

    for refresh in [false, true] {
        let cmd = UpdateCmd {
            cmd: Some(UpdateSub::Check { refresh }),
        };
        assert_eq!(cmd.run(&renew), 2, "failed check with refresh={refresh} must exit 2");
    }
}

/// `<bin> update check --refresh` with an unusable credential and nothing cached: the
/// exact shape of a private repo the token cannot see. The scripting contract renew
/// publishes (`if ! <bin> update check; then <bin> update install --yes; fi`) depends on
/// the failure being visible, so the rendered message names the failure, not the repo.
#[test]
fn test_failed_check_message_names_the_failure() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    let renew = offline_renew(&api, tmp.path()).with_token(Some("ghp_unusable".to_string()));

    let err = run_check(&renew, true).unwrap_err();

    let msg = err.to_string();
    assert!(!msg.contains("ghp_unusable"), "the credential must never render: {msg}");
    assert!(
        msg.starts_with("the check did not succeed and there was no usable cache"),
        "message must name the failure: {msg}"
    );
    assert!(
        msg.contains("no release found for tatari-tv/ccu"),
        "cause must be named: {msg}"
    );
}

/// When already current and force=false, exit 0 without prompting or installing. A fresh
/// cache entry naming the current version is a check that DID happen, recently, so this
/// is the one path that still reports from cache alone.
#[test]
fn test_run_install_already_current_without_force_exits_0() {
    let api = CannedApi::status(500, "Internal Server Error");
    let tmp = tempfile::tempdir().unwrap();
    seed_cache(tmp.path(), "0.4.3", Duration::from_secs(0));
    let renew = offline_renew(&api, tmp.path());
    let cmd = UpdateCmd {
        cmd: Some(UpdateSub::Install {
            version: None,
            force: false,
            yes: true,
            refresh: false,
            install_path: None,
        }),
    };
    assert_eq!(cmd.run(&renew), 0);
}

#[test]
fn test_run_revert_no_backup_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    let renew = make_renew().with_install_path(tmp.path().join("ccu"));
    let cmd = UpdateCmd {
        cmd: Some(UpdateSub::Revert {
            yes: true,
            install_path: None,
        }),
    };
    // No backup exists, so revert should exit 2 with "no backup available"
    assert_eq!(cmd.run(&renew), 2);
}

/// A cache entry carries a KNOWN `published-at`; the rendered report line must carry
/// exactly that date, not `1970-01-01` (the `UNIX_EPOCH` this used to fabricate). Runs
/// through the real `check_latest` -> `compare_cached` -> `render_check` path.
#[test]
fn test_run_check_reports_the_cached_release_date_not_a_fabricated_one() {
    let api = CannedApi::status(500, "Internal Server Error"); // fresh cache hit, no network needed
    let tmp = tempfile::tempdir().unwrap();
    let published_at = Utc.with_ymd_and_hms(2024, 3, 14, 0, 0, 0).unwrap();
    seed_cache_with(
        tmp.path(),
        "9.9.9",
        Utc::now(),
        published_at,
        "https://github.com/tatari-tv/ccu/releases/tag/v9.9.9",
    );
    let renew = offline_renew(&api, tmp.path());

    let update = renew.check_latest().unwrap();
    let (line, code) = render_check(&renew, &update);

    assert_eq!(code, 1);
    assert!(line.contains("2024-03-14"), "line must carry the cached date: {line}");
    assert!(!line.contains("1970-01-01"), "must not fabricate the epoch: {line}");
}

/// Every cache file in the fleet today is this field-less shape. It must read as a
/// cache MISS (self-healing refresh), not feed a report a date it never recorded.
#[test]
fn test_legacy_two_key_cache_entry_is_a_cache_miss() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    seed_legacy_cache(tmp.path(), "9.9.9", Duration::from_secs(0));
    let renew = offline_renew(&api, tmp.path());

    // A legacy entry cannot back a report, so the check falls through to the network,
    // which fails here, so the check fails rather than silently reporting the legacy
    // entry as if it were current.
    let result = renew.check_latest();
    assert!(
        matches!(&result, Err(Error::CheckFailed { .. })),
        "a legacy entry must not be usable for a report: {result:?}"
    );
}

/// The bare `<bin> update` form is `check` with no refresh, so it must land on the same
/// exit code as the explicit subcommand rather than on any code at all.
#[test]
fn test_update_cmd_none_subcommand_acts_as_check() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    let renew = offline_renew(&api, tmp.path());

    let bare = UpdateCmd { cmd: None }.run(&renew);
    let explicit = UpdateCmd {
        cmd: Some(UpdateSub::Check { refresh: false }),
    }
    .run(&renew);

    assert_eq!(bare, explicit);
    assert_eq!(bare, 2, "a failed check exits 2 in either spelling");
}
