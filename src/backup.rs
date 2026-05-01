use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct BackupMeta {
    pub(crate) version: String,
    pub(crate) saved_at: DateTime<Utc>,
    pub(crate) original_path: PathBuf,
}

/// Atomically capture the current binary into `backup_dir` before overwriting.
/// Writes binary first, meta last — a torn write leaves the old backup intact.
pub(crate) fn capture(backup_dir: &Path, current_binary: &Path, current_version: &Version) -> Result<()> {
    std::fs::create_dir_all(backup_dir).map_err(Error::Io)?;

    // Write binary atomically: copy to .new, rename to final.
    let binary_new = backup_dir.join("binary.new");
    let binary = backup_dir.join("binary");
    std::fs::copy(current_binary, &binary_new).map_err(|e| Error::InstallPath {
        path: binary_new.clone(),
        source: e,
    })?;
    std::fs::rename(&binary_new, &binary).map_err(Error::Io)?;
    chmod_755(&binary)?;

    // Write meta atomically: serialize to .new, rename to final.
    let meta = BackupMeta {
        version: current_version.to_string(),
        saved_at: Utc::now(),
        original_path: current_binary.to_path_buf(),
    };
    let yaml = serde_yaml::to_string(&meta)?;
    let meta_new = backup_dir.join("meta.yml.new");
    let meta_path = backup_dir.join("meta.yml");
    std::fs::write(&meta_new, &yaml).map_err(Error::Io)?;
    std::fs::rename(&meta_new, &meta_path).map_err(Error::Io)?;

    log::debug!("backup: captured v{} at {:?}", current_version, backup_dir);
    Ok(())
}

/// Restore the backup binary to `install_path`. Deletes the backup directory afterward.
pub(crate) fn restore(backup_dir: &Path, install_path: &Path) -> Result<BackupMeta> {
    let meta_path = backup_dir.join("meta.yml");
    let binary_path = backup_dir.join("binary");

    if !meta_path.exists() || !binary_path.exists() {
        return Err(Error::NoBackup);
    }

    let meta_text = std::fs::read_to_string(&meta_path).map_err(Error::Io)?;
    let meta: BackupMeta = serde_yaml::from_str(&meta_text)?;

    // Atomic restore: copy to sibling .revert.new, rename into place.
    let staged = {
        let mut p = install_path.to_path_buf();
        let name = p
            .file_name()
            .map(|n| format!("{}.revert.new", n.to_string_lossy()))
            .unwrap_or_else(|| "binary.revert.new".to_string());
        p.set_file_name(name);
        p
    };

    std::fs::copy(&binary_path, &staged).map_err(|e| Error::InstallPath {
        path: staged.clone(),
        source: e,
    })?;
    chmod_755(&staged)?;

    if staged.as_os_str() != install_path.as_os_str() {
        atomic_replace(&staged, install_path)?;
    }

    // Delete backup directory.
    std::fs::remove_dir_all(backup_dir).map_err(Error::Io)?;

    log::debug!("backup: restored v{} to {:?}", meta.version, install_path);
    Ok(meta)
}

/// Check whether a backup exists at `backup_dir`.
pub(crate) fn exists(backup_dir: &Path) -> bool {
    backup_dir.join("meta.yml").exists() && backup_dir.join("binary").exists()
}

/// Read backup metadata without consuming (deleting) the backup.
pub(crate) fn peek(backup_dir: &Path) -> Option<BackupMeta> {
    let meta_text = std::fs::read_to_string(backup_dir.join("meta.yml")).ok()?;
    serde_yaml::from_str(&meta_text).ok()
}

fn atomic_replace(src: &Path, dest: &Path) -> Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        // EXDEV: cross-filesystem rename not supported; fall back to copy+remove.
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            std::fs::copy(src, dest).map_err(Error::Io)?;
            std::fs::remove_file(src).map_err(Error::Io)?;
            Ok(())
        }
        Err(e) => Err(Error::Io(e)),
    }
}

fn chmod_755(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(Error::Io)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(Error::Io)
}

#[cfg(test)]
mod tests;
