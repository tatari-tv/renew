#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn test_parse_tag_with_v_prefix() {
    let v = parse_tag("v1.2.3").unwrap();
    assert_eq!(v, Version::new(1, 2, 3));
}

#[test]
fn test_parse_tag_without_v_prefix() {
    let v = parse_tag("1.2.3").unwrap();
    assert_eq!(v, Version::new(1, 2, 3));
}

#[test]
fn test_parse_tag_with_prerelease() {
    let v = parse_tag("v1.0.0-alpha.1").unwrap();
    assert_eq!(v.major, 1);
    assert!(!v.pre.is_empty());
}

#[test]
fn test_parse_tag_rejects_non_semver() {
    let err = parse_tag("v1.2.x").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("v1.2.x"));
}

#[test]
fn test_parse_tag_rejects_missing_patch() {
    let err = parse_tag("1.2").unwrap_err();
    assert!(err.to_string().contains("1.2"));
}

#[test]
fn test_parse_tag_rejects_empty() {
    assert!(parse_tag("").is_err());
}

#[test]
fn test_version_comparison() {
    let older = parse_tag("v0.4.3").unwrap();
    let newer = parse_tag("v0.5.0").unwrap();
    assert!(newer > older);
}

// --- parse_current: the GIT_DESCRIBE-tolerant matrix ---

#[test]
fn test_parse_current_plain_semver() {
    assert_eq!(parse_current("1.2.1").unwrap(), Version::new(1, 2, 1));
}

#[test]
fn test_parse_current_v_prefix() {
    assert_eq!(parse_current("v1.2.1").unwrap(), Version::new(1, 2, 1));
}

#[test]
fn test_parse_current_describe_suffix() {
    // git describe dev build: strip `-<count>-g<sha>` down to the base tag.
    assert_eq!(parse_current("v1.2.1-3-gabc123").unwrap(), Version::new(1, 2, 1));
}

#[test]
fn test_parse_current_describe_suffix_dirty() {
    assert_eq!(parse_current("v1.2.1-3-gabc123-dirty").unwrap(), Version::new(1, 2, 1));
}

#[test]
fn test_parse_current_keeps_genuine_prerelease() {
    let v = parse_current("1.2.1-rc.1").unwrap();
    assert_eq!((v.major, v.minor, v.patch), (1, 2, 1));
    assert!(!v.pre.is_empty(), "genuine prerelease must be preserved");
    assert_eq!(v.pre.as_str(), "rc.1");
}

#[test]
fn test_parse_current_prerelease_with_describe_suffix() {
    // Rightmost scan: strip only the trailing describe suffix, keep the prerelease.
    let v = parse_current("1.2.1-rc.1-3-gabc").unwrap();
    assert_eq!((v.major, v.minor, v.patch), (1, 2, 1));
    assert_eq!(v.pre.as_str(), "rc.1");
}

#[test]
fn test_parse_current_rejects_bare_sha() {
    assert!(parse_current("abc123f").is_err());
}

#[test]
fn test_parse_current_rejects_empty() {
    assert!(parse_current("").is_err());
}

#[test]
fn test_parse_current_rejects_whitespace() {
    assert!(parse_current("   ").is_err());
}

#[test]
fn test_parse_current_error_names_original_input() {
    let err = parse_current("v1.2.x-3-gabc").unwrap_err();
    // The error carries the caller's original string, not the internally-stripped core.
    assert!(err.to_string().contains("v1.2.x-3-gabc"));
}
