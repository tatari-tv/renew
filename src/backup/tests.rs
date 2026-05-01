#![allow(clippy::unwrap_used)]

use super::*;
use semver::Version;
use tempfile::TempDir;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn write_fake_binary(path: &Path) {
    std::fs::write(path, b"#!/bin/sh\necho fake\n").unwrap();
    chmod_755(path).unwrap();
}

#[test]
fn test_capture_creates_binary_and_meta() {
    let tmp = TempDir::new().unwrap();
    let binary = tmp.path().join("mybin");
    write_fake_binary(&binary);
    let backup_dir = tmp.path().join("backup");

    capture(&backup_dir, &binary, &v("0.4.3")).unwrap();

    assert!(backup_dir.join("binary").exists());
    assert!(backup_dir.join("meta.yml").exists());
    assert!(!backup_dir.join("binary.new").exists());
    assert!(!backup_dir.join("meta.yml.new").exists());
}

#[test]
fn test_capture_meta_contains_correct_version() {
    let tmp = TempDir::new().unwrap();
    let binary = tmp.path().join("mybin");
    write_fake_binary(&binary);
    let backup_dir = tmp.path().join("backup");

    capture(&backup_dir, &binary, &v("0.4.3")).unwrap();

    let text = std::fs::read_to_string(backup_dir.join("meta.yml")).unwrap();
    let meta: BackupMeta = serde_yaml::from_str(&text).unwrap();
    assert_eq!(meta.version, "0.4.3");
}

#[test]
fn test_exists_returns_true_after_capture() {
    let tmp = TempDir::new().unwrap();
    let binary = tmp.path().join("mybin");
    write_fake_binary(&binary);
    let backup_dir = tmp.path().join("backup");

    capture(&backup_dir, &binary, &v("0.4.3")).unwrap();
    assert!(exists(&backup_dir));
}

#[test]
fn test_exists_returns_false_without_backup() {
    let tmp = TempDir::new().unwrap();
    assert!(!exists(tmp.path()));
}

#[test]
fn test_restore_replaces_install_path() {
    let tmp = TempDir::new().unwrap();
    let original = tmp.path().join("original");
    write_fake_binary(&original);

    let backup_dir = tmp.path().join("backup");
    capture(&backup_dir, &original, &v("0.4.3")).unwrap();

    // Simulate a new version at install_path
    let install = tmp.path().join("installed");
    std::fs::write(&install, b"new version").unwrap();

    let meta = restore(&backup_dir, &install).unwrap();
    assert_eq!(meta.version, "0.4.3");

    // Backup dir should be gone
    assert!(!backup_dir.exists());
    // Install path should now have the old binary content
    let content = std::fs::read(&install).unwrap();
    assert_eq!(content, b"#!/bin/sh\necho fake\n");
}

#[test]
fn test_restore_returns_no_backup_when_absent() {
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("installed");
    std::fs::write(&install, b"current").unwrap();
    let backup_dir = tmp.path().join("nonexistent-backup");
    let err = restore(&backup_dir, &install).unwrap_err();
    assert!(matches!(err, Error::NoBackup));
}

#[test]
fn test_peek_returns_meta_without_deleting_backup() {
    let tmp = TempDir::new().unwrap();
    let binary = tmp.path().join("mybin");
    write_fake_binary(&binary);
    let backup_dir = tmp.path().join("backup");

    capture(&backup_dir, &binary, &v("0.4.3")).unwrap();
    let meta = peek(&backup_dir).unwrap();
    assert_eq!(meta.version, "0.4.3");

    // Backup must still exist after peek
    assert!(exists(&backup_dir));
}

#[test]
fn test_peek_returns_none_when_absent() {
    let tmp = TempDir::new().unwrap();
    assert!(peek(tmp.path()).is_none());
}

#[test]
fn test_meta_serializes_kebab_case() {
    let meta = BackupMeta {
        version: "0.4.3".to_string(),
        saved_at: chrono::Utc::now(),
        original_path: std::path::PathBuf::from("/home/user/.cargo/bin/ccu"),
    };
    let yaml = serde_yaml::to_string(&meta).unwrap();
    assert!(yaml.contains("saved-at"));
    assert!(yaml.contains("original-path"));
}
