use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CACHE_FILE: &str = "check.yml";
const LOCK_FILE: &str = "refresh.lock";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CacheEntry {
    pub(crate) latest_version: String,
    pub(crate) checked_at: DateTime<Utc>,
}

impl CacheEntry {
    pub(crate) fn is_fresh(&self, ttl: Duration) -> bool {
        let age = Utc::now()
            .signed_duration_since(self.checked_at)
            .to_std()
            .unwrap_or(Duration::MAX);
        age < ttl
    }
}

/// Load cache entry from `<cache_dir>/check.yml`. Returns None if absent or unparseable.
pub(crate) fn load(cache_dir: &Path) -> Option<CacheEntry> {
    let path = cache_dir.join(CACHE_FILE);
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_yaml::from_str(&content) {
        Ok(entry) => Some(entry),
        Err(e) => {
            log::warn!("cache parse error at {:?}: {e}; treating as cache miss", path);
            None
        }
    }
}

/// Atomically write cache entry to `<cache_dir>/check.yml`.
pub(crate) fn save(cache_dir: &Path, entry: &CacheEntry) -> Result<()> {
    std::fs::create_dir_all(cache_dir).map_err(Error::Io)?;
    let yaml = serde_yaml::to_string(entry)?;
    let tmp = cache_dir.join(format!("{CACHE_FILE}.tmp"));
    std::fs::write(&tmp, &yaml).map_err(Error::Io)?;
    std::fs::rename(&tmp, cache_dir.join(CACHE_FILE)).map_err(Error::Io)?;
    log::debug!("cache: saved latest={} at={}", entry.latest_version, entry.checked_at);
    Ok(())
}

/// Returns the path to the lock file used to serialize GitHub refreshes.
pub(crate) fn lock_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(LOCK_FILE)
}

#[cfg(test)]
mod tests;
