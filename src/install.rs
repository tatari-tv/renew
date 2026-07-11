use crate::backup;
use crate::error::{Error, Result};
use crate::github::{self, ReleaseInfo};
use crate::platform;
use crate::version::{InstalledVersion, parse_tag};
use semver::Version;
use sha2::Digest;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Run the full install pipeline, returning the new version state.
///
/// Pipeline order:
/// 1. resolve_install_path  2. preflight  3. use provided info
/// 4. pick asset from list  5. download tarball + sidecar
/// 6. verify sha256         7. extract    8. chmod
/// 9. capture_backup        10. replace_in_place
/// 11. cleanup              → return InstalledVersion
pub(crate) fn run(
    repo: &str,
    bin: &str,
    current: &Version,
    info: &ReleaseInfo,
    install_path: &Path,
    cache_dir: &Path,
    data_dir: &Path,
    token: Option<&str>,
    download_timeout: Duration,
) -> Result<InstalledVersion> {
    let platform = platform::current_platform()?;
    let tag = &info.tag_name;
    let target = parse_tag(tag)?;

    let asset_name = format!("{bin}-{tag}-{platform}.tar.gz");
    let asset = info.find_asset(&asset_name).ok_or(Error::AssetMissing {
        os: platform.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    })?;

    let sha_name = format!("{asset_name}.sha256");
    let sha_asset = info.find_asset(&sha_name).ok_or(Error::AssetMissing {
        os: format!("{platform} (sha256 sidecar)"),
        arch: std::env::consts::ARCH.to_string(),
    })?;

    let download_dir = cache_dir.join("download");
    std::fs::create_dir_all(&download_dir).map_err(Error::Io)?;

    let tarball_dest = download_dir.join(&asset_name);
    let sha_dest = download_dir.join(&sha_name);

    log::debug!("install: downloading tarball {}", asset.browser_download_url);
    github::download_asset(&asset.browser_download_url, token, &tarball_dest, download_timeout)?;

    log::debug!("install: downloading sidecar {}", sha_asset.browser_download_url);
    github::download_asset(&sha_asset.browser_download_url, token, &sha_dest, download_timeout)?;

    verify_sha256(&tarball_dest, &sha_dest)?;
    log::debug!("install: sha256 verified");

    let staged = extract_single(&tarball_dest, &download_dir, bin)?;
    chmod_executable(&staged)?;

    let backup_dir = backup_dir_for(install_path, data_dir);
    if install_path.exists() {
        backup::capture(&backup_dir, install_path, current)?;
        log::debug!("install: backup captured at {:?}", backup_dir);
    }

    replace_in_place(&staged, install_path)?;
    log::debug!("install: replaced {:?}", install_path);

    // Cleanup download artifacts.
    let _ = std::fs::remove_file(&tarball_dest);
    let _ = std::fs::remove_file(&sha_dest);
    let _ = std::fs::remove_dir(&download_dir);

    log::info!("install: {} {} -> {} at {:?}", repo, current, target, install_path);

    Ok(InstalledVersion {
        from: current.clone(),
        to: target,
        path: install_path.to_path_buf(),
    })
}

/// Verify a tarball against its sha256 sidecar.
/// Sidecar format: `<64 hex chars>[  | *]<name>[\n]`
pub(crate) fn verify_sha256(tarball: &Path, sidecar: &Path) -> Result<()> {
    let sidecar_text = std::fs::read_to_string(sidecar).map_err(Error::Io)?;
    let expected = sidecar_text
        .trim()
        .get(..64)
        .ok_or_else(|| Error::ChecksumMismatch {
            expected: sidecar_text.trim().to_string(),
            actual: "(sidecar too short)".to_string(),
        })?
        .to_string();

    let actual = sha256_hex(tarball)?;

    if actual != expected {
        return Err(Error::ChecksumMismatch { expected, actual });
    }
    Ok(())
}

fn sha256_hex(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(Error::Io)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = hasher.finalize();
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}

/// Extract exactly one regular file from a tar.gz archive into `dest_dir/<bin>`.
pub(crate) fn extract_single(tarball: &Path, dest_dir: &Path, bin: &str) -> Result<PathBuf> {
    let file = std::fs::File::open(tarball).map_err(Error::Io)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let mut count = 0usize;
    let dest = dest_dir.join(bin);

    for entry in archive.entries().map_err(Error::Io)? {
        let mut entry = entry.map_err(Error::Io)?;
        if entry.header().entry_type().is_file() {
            count += 1;
            if count > 1 {
                return Err(Error::TarballShape { count });
            }
            entry.unpack(&dest).map_err(Error::Io)?;
        }
    }

    if count == 0 {
        return Err(Error::TarballShape { count: 0 });
    }

    Ok(dest)
}

fn chmod_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(Error::Io)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(Error::Io)
}

/// Atomically replace `install_path` with `staged`.
/// Uses `self_replace` when replacing the running binary; `rename` otherwise.
fn replace_in_place(staged: &Path, install_path: &Path) -> Result<()> {
    let is_current = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .zip(install_path.canonicalize().ok())
        .map(|(cur, dest)| cur == dest)
        .unwrap_or(false);

    if is_current {
        self_replace::self_replace(staged).map_err(|e| Error::InstallPath {
            path: install_path.to_path_buf(),
            source: e,
        })
    } else {
        atomic_rename(staged, install_path)
    }
}

fn atomic_rename(src: &Path, dest: &Path) -> Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            std::fs::copy(src, dest).map_err(Error::Io)?;
            let _ = std::fs::remove_file(src);
            Ok(())
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Compute the per-install backup directory.
pub(crate) fn backup_dir_for(install_path: &Path, data_dir: &Path) -> PathBuf {
    let canonical = install_path
        .canonicalize()
        .unwrap_or_else(|_| install_path.to_path_buf());
    let hash = sha2::Sha256::digest(canonical.display().to_string().as_bytes());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    data_dir.join(&hex[..12]).join("backup")
}

#[cfg(test)]
mod tests;
