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
