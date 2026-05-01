//! Real-network smoke tests against a public release of `ccu`
//! (`tatari-tv/claude-cost-usage`). All tests are `#[ignore]` so the
//! default `cargo test` run skips them. Run them explicitly with:
//!
//!     cargo test --test network -- --ignored --nocapture
//!
//! These tests hit real GitHub and S3 endpoints. They may be flaky if
//! the network is down, GitHub is rate limiting, or the public ccu
//! release is yanked. They are intentionally not part of CI.

#![allow(clippy::unwrap_used)]

use renew::Renew;
use semver::Version;
use std::time::Duration;

const REPO: &str = "tatari-tv/claude-cost-usage";
const BIN: &str = "ccu";

fn make_renew(current: &str) -> Renew {
    Renew::new(REPO, BIN, current)
        .unwrap()
        .with_cache_ttl(Duration::from_secs(0))
        .with_network_timeout(Duration::from_secs(15))
}

#[test]
#[ignore]
fn check_latest_against_real_ccu() {
    // Pretend we are on a very old version so an update is virtually guaranteed.
    let r = make_renew("0.0.1");
    let update = r
        .check_latest()
        .expect("check_latest against tatari-tv/claude-cost-usage")
        .expect("expected an update from 0.0.1; got None");

    assert!(
        update.latest > Version::parse("0.4.0").unwrap(),
        "latest ccu should be at least v0.4.0, got {}",
        update.latest
    );
    assert!(
        update.tag.starts_with('v'),
        "tag should be v-prefixed, got {}",
        update.tag
    );
}

/// Full install pipeline against a real ccu release: download, sha verify,
/// extract, chmod, backup capture (of a seeded fake "current"), atomic
/// replace, then revert. Exercises every step that unit-test fixtures cannot.
#[test]
#[ignore]
fn install_latest_then_revert_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let install_path = tmp.path().join("ccu");
    let data_dir = tmp.path().join("data");
    let cache_dir = tmp.path().join("cache");

    // Seed a fake "current" binary so the install pipeline captures it as backup.
    let fake_marker = b"fake-old-ccu-marker\n";
    std::fs::write(&install_path, fake_marker).unwrap();

    // Pretend we are on 0.0.1 so install_latest will resolve a real upgrade.
    let r = make_renew("0.0.1")
        .with_install_path(install_path.clone())
        .with_data_dir(data_dir.clone())
        .with_cache_dir(cache_dir);

    let result = r
        .install_latest()
        .unwrap_or_else(|e| panic!("install_latest: {e}"));

    assert!(
        install_path.exists(),
        "install_path should exist after install"
    );

    let installed = std::fs::read(&install_path).unwrap();
    assert_ne!(
        installed, fake_marker,
        "fake current should have been replaced"
    );
    assert!(
        installed.len() > fake_marker.len(),
        "real ccu binary should be larger than fake"
    );

    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&install_path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o755,
        "installed binary should be 0o755, got {mode:o}"
    );

    assert_eq!(result.from, Version::parse("0.0.1").unwrap());
    assert!(result.to > Version::parse("0.4.0").unwrap());

    // Backup of the fake "current" should now exist.
    assert!(r.has_backup(), "backup should exist after install");

    // Revert: restore the fake bytes, consume the backup.
    let reverted = r.revert().unwrap();
    assert_eq!(reverted.from, Version::parse("0.0.1").unwrap());

    let after_revert = std::fs::read(&install_path).unwrap();
    assert_eq!(
        after_revert, fake_marker,
        "revert should restore the fake bytes"
    );

    assert!(!r.has_backup(), "backup should be consumed after revert");
}
