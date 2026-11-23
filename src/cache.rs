use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use time::OffsetDateTime;

pub(crate) const CACHE_FILE: &str = "check.yml";
const LOCK_FILE: &str = "refresh.lock";

/// `published_at`/`release_url` are `Option` so a pre-schema-bump entry (every cache file
/// in the fleet today) deserializes without error - missing keys become `None` for an
/// `Option` field, and no `skip_serializing_if` keeps a fresh `None` serializing as an
/// explicit `null` rather than an omitted key. `None` on EITHER field means the entry
/// cannot back a report and must be treated as a cache miss, which makes the bump
/// self-healing: the first check after upgrading refreshes and writes both fields.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct CacheEntry {
    pub(crate) latest_version: String,
    #[serde(with = "crate::timefmt::serde_rfc3339")]
    pub(crate) checked_at: OffsetDateTime,
    #[serde(default, with = "crate::timefmt::serde_rfc3339_option")]
    pub(crate) published_at: Option<OffsetDateTime>,
    pub(crate) release_url: Option<String>,
}

impl CacheEntry {
    pub(crate) fn is_fresh(&self, ttl: Duration) -> bool {
        let age = Duration::try_from(OffsetDateTime::now_utc() - self.checked_at).unwrap_or(Duration::MAX);
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
