#![allow(clippy::unwrap_used)]

use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn test_verify_sha256_passes_for_valid_fixture() {
    let tarball = fixture_path("ccu-v0.5.0-linux-amd64.tar.gz");
    let sidecar = fixture_path("ccu-v0.5.0-linux-amd64.tar.gz.sha256");
    assert!(verify_sha256(&tarball, &sidecar).is_ok());
}

#[test]
fn test_verify_sha256_fails_for_wrong_hash() {
    let tmp = TempDir::new().unwrap();
    let tarball = fixture_path("ccu-v0.5.0-linux-amd64.tar.gz");
    // Write a sidecar with a wrong hash (64 zeros)
    let bad_sidecar = tmp.path().join("bad.sha256");
    std::fs::write(
        &bad_sidecar,
        "0000000000000000000000000000000000000000000000000000000000000000  ccu.tar.gz\n",
    )
    .unwrap();
    let err = verify_sha256(&tarball, &bad_sidecar).unwrap_err();
    assert!(matches!(err, Error::ChecksumMismatch { .. }));
}

#[test]
fn test_verify_sha256_fails_for_short_sidecar() {
    let tmp = TempDir::new().unwrap();
    let tarball = fixture_path("ccu-v0.5.0-linux-amd64.tar.gz");
    let bad_sidecar = tmp.path().join("short.sha256");
    std::fs::write(&bad_sidecar, "abc123\n").unwrap();
    let err = verify_sha256(&tarball, &bad_sidecar).unwrap_err();
    assert!(matches!(err, Error::ChecksumMismatch { .. }));
}

#[test]
fn test_extract_single_extracts_one_file() {
    let tmp = TempDir::new().unwrap();
    let tarball = fixture_path("ccu-v0.5.0-linux-amd64.tar.gz");
    let dest = extract_single(&tarball, tmp.path(), "ccu").unwrap();
    assert!(dest.exists());
    assert_eq!(dest.file_name().unwrap(), "ccu");
}

#[test]
fn test_extract_single_rejects_multi_file_tarball() {
    let tmp = TempDir::new().unwrap();
    let tarball = fixture_path("multi-file.tar.gz");
    let err = extract_single(&tarball, tmp.path(), "ccu").unwrap_err();
    assert!(matches!(err, Error::TarballShape { count } if count > 1));
}

#[test]
fn test_backup_dir_for_is_deterministic() {
    let install = PathBuf::from("/home/user/.cargo/bin/ccu");
    let data = PathBuf::from("/home/user/.local/share/ccu");
    let dir1 = backup_dir_for(&install, &data);
    let dir2 = backup_dir_for(&install, &data);
    assert_eq!(dir1, dir2);
}

#[test]
fn test_backup_dir_for_differs_by_install_path() {
    let data = PathBuf::from("/home/user/.local/share/ccu");
    let dir1 = backup_dir_for(&PathBuf::from("/home/user/.cargo/bin/ccu"), &data);
    let dir2 = backup_dir_for(&PathBuf::from("/tmp/ccu"), &data);
    assert_ne!(dir1, dir2);
}
