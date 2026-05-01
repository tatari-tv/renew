#![allow(clippy::unwrap_used)]

use super::*;
use chrono::Utc;
use std::time::Duration;
use tempfile::TempDir;

fn make_entry(version: &str, age_secs: i64) -> CacheEntry {
    CacheEntry {
        latest_version: version.to_string(),
        checked_at: Utc::now() - chrono::Duration::seconds(age_secs),
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
