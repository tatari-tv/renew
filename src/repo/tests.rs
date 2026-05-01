#![allow(clippy::unwrap_used)]

use super::*;

fn parse(s: &str) -> RepoSlug {
    RepoSlug::parse(s).unwrap()
}

fn canonical() -> RepoSlug {
    RepoSlug {
        owner: "tatari-tv".to_string(),
        repo: "ccu".to_string(),
    }
}

#[test]
fn test_bare_slug() {
    assert_eq!(parse("tatari-tv/ccu"), canonical());
}

#[test]
fn test_https_url() {
    assert_eq!(parse("https://github.com/tatari-tv/ccu"), canonical());
}

#[test]
fn test_http_url() {
    assert_eq!(parse("http://github.com/tatari-tv/ccu"), canonical());
}

#[test]
fn test_https_url_with_git_suffix() {
    assert_eq!(parse("https://github.com/tatari-tv/ccu.git"), canonical());
}

#[test]
fn test_https_url_with_trailing_slash() {
    assert_eq!(parse("https://github.com/tatari-tv/ccu/"), canonical());
}

#[test]
fn test_bare_slug_with_git_suffix() {
    assert_eq!(parse("tatari-tv/ccu.git"), canonical());
}

#[test]
fn test_ssh_colon_form() {
    assert_eq!(parse("git@github.com:tatari-tv/ccu.git"), canonical());
}

#[test]
fn test_ssh_url_form() {
    assert_eq!(parse("ssh://git@github.com/tatari-tv/ccu.git"), canonical());
}

#[test]
fn test_as_path() {
    assert_eq!(canonical().as_path(), "tatari-tv/ccu");
}

#[test]
fn test_empty_input_errors() {
    let err = RepoSlug::parse("").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("empty repo identifier"));
    assert!(msg.contains("repository"));
}

#[test]
fn test_whitespace_only_errors() {
    let err = RepoSlug::parse("   ").unwrap_err();
    assert!(err.to_string().contains("empty repo identifier"));
}

#[test]
fn test_no_slash_errors() {
    let err = RepoSlug::parse("justarepo").unwrap_err();
    assert!(err.to_string().contains("owner/repo"));
}

#[test]
fn test_too_many_slashes_errors() {
    let err = RepoSlug::parse("owner/repo/extra").unwrap_err();
    assert!(err.to_string().contains("exactly one"));
}

#[test]
fn test_empty_owner_errors() {
    let err = RepoSlug::parse("/repo").unwrap_err();
    assert!(err.to_string().contains("non-empty"));
}

#[test]
fn test_empty_repo_errors() {
    let err = RepoSlug::parse("owner/").unwrap_err();
    assert!(err.to_string().contains("non-empty"));
}
