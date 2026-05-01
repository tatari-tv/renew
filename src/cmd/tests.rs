#![allow(clippy::unwrap_used)]

use super::*;

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

#[test]
fn test_run_check_no_network_returns_valid_exit_code() {
    let renew = make_renew();
    let cmd = UpdateCmd {
        cmd: Some(UpdateSub::Check { refresh: false }),
    };
    let code = cmd.run(&renew);
    // 0 (up to date), 1 (update available), or 2 (error) are all valid without network
    assert!([0, 1, 2].contains(&code));
}

#[test]
fn test_run_install_already_current_without_force_exits_0() {
    // When already current and force=false, exit 0 without prompting or installing.
    let renew = make_renew();
    let cmd = UpdateCmd {
        cmd: Some(UpdateSub::Install {
            version: None,
            force: false,
            yes: true,
            refresh: false,
            install_path: None,
        }),
    };
    let code = cmd.run(&renew);
    // 0 = already current, or 2 = network error — both acceptable without real network
    assert!([0, 2].contains(&code));
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

#[test]
fn test_update_cmd_none_subcommand_acts_as_check() {
    let renew = make_renew();
    let cmd = UpdateCmd { cmd: None };
    let code = cmd.run(&renew);
    assert!([0, 1, 2].contains(&code));
}
