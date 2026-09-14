#![allow(clippy::unwrap_used)]

use super::*;
use crate::fixture::{CannedApi, offline_renew, seed_cache};
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
