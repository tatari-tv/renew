#![allow(clippy::unwrap_used)]

use super::*;
use chrono::Utc;
use std::time::Duration;
use tempfile::TempDir;

fn make_entry(version: &str, age_secs: i64) -> CacheEntry {
    let checked_at = Utc::now() - chrono::Duration::seconds(age_secs);
    CacheEntry {
        latest_version: version.to_string(),
        checked_at,
        published_at: Some(checked_at),
        release_url: Some(format!("https://github.com/tatari-tv/ccu/releases/tag/v{version}")),
    }
}

fn make_legacy_entry(version: &str, age_secs: i64) -> CacheEntry {
    CacheEntry {
        latest_version: version.to_string(),
        checked_at: Utc::now() - chrono::Duration::seconds(age_secs),
        published_at: None,
        release_url: None,
    }
}

#[test]
fn test_is_fresh_within_ttl() {
    let entry = make_entry("0.5.0", 3600);
    assert!(entry.is_fresh(Duration::from_secs(24 * 60 * 60)));
}

#[test]
fn test_is_stale_past_ttl() {
    let entry = make_entry("0.5.0", 25 * 60 * 60);
    assert!(!entry.is_fresh(Duration::from_secs(24 * 60 * 60)));
}

#[test]
fn test_save_and_load_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let entry = make_entry("0.5.0", 0);
    save(tmp.path(), &entry).unwrap();

    let loaded = load(tmp.path()).unwrap();
    assert_eq!(loaded.latest_version, "0.5.0");
}

#[test]
fn test_load_returns_none_when_absent() {
    let tmp = TempDir::new().unwrap();
    assert!(load(tmp.path()).is_none());
}

#[test]
fn test_load_returns_none_on_corrupt_file() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join(CACHE_FILE), b"not: valid: yaml: {{{{").unwrap();
    assert!(load(tmp.path()).is_none());
}

#[test]
fn test_save_is_atomic_temp_renamed() {
    let tmp = TempDir::new().unwrap();
    let entry = make_entry("0.5.0", 0);
    save(tmp.path(), &entry).unwrap();

    // Temp file must not remain
    assert!(!tmp.path().join(format!("{CACHE_FILE}.tmp")).exists());
    assert!(tmp.path().join(CACHE_FILE).exists());
}

#[test]
fn test_lock_path_is_in_cache_dir() {
    let tmp = TempDir::new().unwrap();
    let lp = lock_path(tmp.path());
    assert_eq!(lp.parent().unwrap(), tmp.path());
    assert_eq!(lp.file_name().unwrap(), LOCK_FILE);
}

#[test]
fn test_cache_entry_serializes_kebab_case() {
    let entry = make_entry("0.5.0", 0);
    let yaml = serde_yaml::to_string(&entry).unwrap();
    assert!(yaml.contains("latest-version"));
    assert!(yaml.contains("checked-at"));
    assert!(!yaml.contains("latest_version"));
}

/// A legacy two-key file - `latest-version` and `checked-at` only, no `published-at` or
/// `release-url` - is the shape of every cache file in the fleet today. It must still
/// deserialize (an old binary must be able to read a new one's file and vice versa), with
/// the two new fields landing as `None` rather than erroring the parse.
#[test]
fn test_legacy_two_key_entry_deserializes_with_none_new_fields() {
    let tmp = TempDir::new().unwrap();
    let yaml = "latest-version: 0.5.0\nchecked-at: 2026-01-01T00:00:00Z\n";
    std::fs::write(tmp.path().join(CACHE_FILE), yaml).unwrap();

    let loaded = load(tmp.path()).unwrap();

    assert_eq!(loaded.latest_version, "0.5.0");
    assert!(loaded.published_at.is_none());
    assert!(loaded.release_url.is_none());
}

/// No `skip_serializing_if`, by design: a `None` field must serialize as an explicit
/// `null` rather than being omitted, so the on-disk shape does not change without the
/// type changing (see the design doc's Data Model section).
#[test]
fn test_legacy_entry_serializes_new_fields_as_explicit_null() {
    let entry = make_legacy_entry("0.5.0", 0);
    let yaml = serde_yaml::to_string(&entry).unwrap();
    assert!(yaml.contains("published-at: null"), "got: {yaml}");
    assert!(yaml.contains("release-url: null"), "got: {yaml}");
}

#[test]
fn test_entry_with_published_at_and_release_url_roundtrips() {
    let tmp = TempDir::new().unwrap();
    let entry = make_entry("0.6.0", 0);
    save(tmp.path(), &entry).unwrap();

    let loaded = load(tmp.path()).unwrap();

    assert_eq!(loaded.published_at, entry.published_at);
    assert_eq!(loaded.release_url, entry.release_url);
}
