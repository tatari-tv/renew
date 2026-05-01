#![allow(clippy::unwrap_used)]

use super::*;

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64", not(target_env = "musl")))]
fn test_linux_amd64() {
    assert_eq!(current_platform().unwrap(), "linux-amd64");
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "aarch64", not(target_env = "musl")))]
fn test_linux_arm64() {
    assert_eq!(current_platform().unwrap(), "linux-arm64");
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn test_macos_x86_64() {
    assert_eq!(current_platform().unwrap(), "macos-x86_64");
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn test_macos_arm64() {
    assert_eq!(current_platform().unwrap(), "macos-arm64");
}

#[test]
#[cfg(target_env = "musl")]
fn test_musl_is_rejected() {
    let err = current_platform().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("linux-musl"));
}

#[test]
fn test_current_platform_returns_known_value() {
    // On all four supported platforms this should succeed; on unsupported it errors.
    // We don't assert exact value here — just that it compiles and runs.
    let _ = current_platform();
}
