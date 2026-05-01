#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn test_error_display_invalid_repo() {
    let e = Error::InvalidRepo("bad/input/here".to_string());
    assert!(e.to_string().contains("invalid repo identifier"));
}

#[test]
fn test_error_display_no_backup() {
    let e = Error::NoBackup;
    assert_eq!(e.to_string(), "no backup available to revert to");
}

#[test]
fn test_error_display_prompt_required_not_tty() {
    let e = Error::PromptRequiredButStdinNotTty;
    assert!(e.to_string().contains("--yes"));
}

#[test]
fn test_error_display_asset_missing() {
    let e = Error::AssetMissing {
        os: "freebsd".to_string(),
        arch: "x86_64".to_string(),
    };
    assert!(e.to_string().contains("freebsd"));
    assert!(e.to_string().contains("x86_64"));
}

#[test]
fn test_error_display_checksum_mismatch() {
    let e = Error::ChecksumMismatch {
        expected: "abc123".to_string(),
        actual: "def456".to_string(),
    };
    assert!(e.to_string().contains("abc123"));
    assert!(e.to_string().contains("def456"));
}

#[test]
fn test_result_alias_works() {
    let ok: Result<i32> = Ok(42);
    assert!(ok.is_ok());
}
