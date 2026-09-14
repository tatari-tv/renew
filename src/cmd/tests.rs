#![allow(clippy::unwrap_used)]

use super::*;
use crate::fixture::{CannedApi, offline_renew, seed_cache, seed_cache_with, seed_legacy_cache};
use chrono::{TimeZone, Utc};
use clap::Parser;
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
            cmd: Some(UpdateSub::Check),
            refresh,
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

/// When already current and force=false, exit 0 without prompting or installing. Install
/// with no explicit version always refreshes now (Phase 3), so a fresh, current-matching
/// TAG from the network - not a cached entry - is what makes this a no-op: the cache is
/// never consulted for the no-version install path.
#[test]
fn test_run_install_already_current_without_force_exits_0() {
    let body = r#"{
        "tag_name": "v0.4.3",
        "html_url": "https://github.com/tatari-tv/ccu/releases/tag/v0.4.3",
        "published_at": "2024-01-01T00:00:00Z",
        "assets": []
    }"#;
    let api = CannedApi::json(200, "OK", body);
    let tmp = tempfile::tempdir().unwrap();
    let renew = offline_renew(&api, tmp.path());
    let cmd = UpdateCmd {
        cmd: Some(UpdateSub::Install {
            version: None,
            force: false,
            yes: true,
            install_path: None,
        }),
        refresh: false,
    };
    assert_eq!(cmd.run(&renew), 0);
}

/// Install with no explicit version and a STALE cache entry: the old behavior only fed
/// the equality gate and prompt text from cache, but resolved the target from a fresh
/// network call anyway. Now the whole check is fresh - a stale cache naming a version
/// that is no longer latest must not survive into the equality gate or the prompt.
#[test]
fn test_run_install_no_version_always_refreshes_past_a_stale_cache() {
    let body = r#"{
        "tag_name": "v0.4.3",
        "html_url": "https://github.com/tatari-tv/ccu/releases/tag/v0.4.3",
        "published_at": "2024-01-01T00:00:00Z",
        "assets": []
    }"#;
    let api = CannedApi::json(200, "OK", body);
    let tmp = tempfile::tempdir().unwrap();
    // A cache entry naming a DIFFERENT, higher version, still within TTL. If install
    // consulted the cache it would target 9.9.9 rather than the network's 0.4.3.
    seed_cache(tmp.path(), "9.9.9", Duration::from_secs(0));
    let renew = offline_renew(&api, tmp.path());
    let cmd = UpdateCmd {
        cmd: Some(UpdateSub::Install {
            version: None,
            force: false,
            yes: true,
            install_path: None,
        }),
        refresh: false,
    };
    assert_eq!(
        cmd.run(&renew),
        0,
        "must resolve target from the refreshing check, not the stale cache"
    );
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
        refresh: false,
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

    let bare = UpdateCmd {
        cmd: None,
        refresh: false,
    }
    .run(&renew);
    let explicit = UpdateCmd {
        cmd: Some(UpdateSub::Check),
        refresh: false,
    }
    .run(&renew);

    assert_eq!(bare, explicit);
    assert_eq!(bare, 2, "a failed check exits 2 in either spelling");
}

/// renew ships no binary, so a consumer's top-level command is stood in with a minimal
/// `Parser` wrapping `UpdateCmd` as its `update` subcommand - the same shape any real
/// consumer CLI uses. This drives real clap argv parsing rather than a hand-built struct
/// literal, which matters here specifically: a hand-built `UpdateCmd { refresh: true,
/// .. }` cannot prove the `global = true` declaration actually makes `--refresh` legal
/// AFTER the `check` token, only that the field exists.
#[derive(clap::Parser)]
struct TestCli {
    #[command(subcommand)]
    top: TestTop,
}

#[derive(clap::Subcommand)]
enum TestTop {
    Update(UpdateCmd),
}

fn parse_update(args: &[&str]) -> UpdateCmd {
    let argv = std::iter::once("bin").chain(args.iter().copied());
    let TestTop::Update(cmd) = TestCli::try_parse_from(argv).unwrap().top;
    cmd
}

/// `<bin> update --refresh` and `<bin> update check --refresh` must BOTH parse and BOTH
/// take the refreshing path, against the INJECTED fixture API, never live GitHub. Proof
/// that `--refresh` reached the network path (not merely that it parsed): the cache
/// carries a fresh, TTL-live entry naming a HIGHER version, so without `--refresh` the
/// check reports it straight from cache (exit 1, no network touched); `--refresh` must
/// force past that cache to the injected API, which is wired to fail here, flipping the
/// exit code to 2. A parse-only bug (flag accepted but never read) would still exit 1.
#[test]
fn test_refresh_flag_parses_and_forces_the_network_path_at_both_scopes() {
    let api = CannedApi::status(404, "Not Found");
    let tmp = tempfile::tempdir().unwrap();
    seed_cache(tmp.path(), "9.9.9", Duration::from_secs(0));
    let renew = offline_renew(&api, tmp.path());

    let no_refresh = parse_update(&["update"]);
    assert_eq!(
        no_refresh.run(&renew),
        1,
        "no --refresh: the fresh cache hit must report the update without touching the network"
    );

    let cases: [&[&str]; 2] = [&["update", "--refresh"], &["update", "check", "--refresh"]];
    for args in cases {
        let cmd = parse_update(args);
        assert_eq!(
            cmd.run(&renew),
            2,
            "{args:?}: --refresh must force past the cache to the (failing) injected API"
        );
    }
}
