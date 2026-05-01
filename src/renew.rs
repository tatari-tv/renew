use crate::cache;
use crate::error::{Error, Result};
use crate::github;
use crate::platform;
use crate::repo::RepoSlug;
use crate::version::{InstalledVersion, Update, parse_tag};
use chrono::Utc;
use semver::Version;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const DEFAULT_NETWORK_TIMEOUT_SECS: u64 = 5;
const DEFAULT_DOWNLOAD_TIMEOUT_SECS: u64 = 60;

#[derive(Debug)]
pub(crate) enum TokenSource {
    EnvAuto,
    Explicit(Option<String>),
}

#[derive(Debug)]
pub struct Renew {
    pub(crate) repo: RepoSlug,
    pub(crate) bin: String,
    pub(crate) current: Version,
    pub(crate) cache_ttl: Duration,
    pub(crate) cache_dir: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) install_path: Option<PathBuf>,
    pub(crate) token: TokenSource,
    pub(crate) network_timeout: Duration,
    pub(crate) download_timeout: Duration,
}

impl Renew {
    pub fn new(repo: impl AsRef<str>, bin: impl AsRef<str>, current: impl AsRef<str>) -> Result<Self> {
        let repo = RepoSlug::parse(repo.as_ref())?;
        let bin = bin.as_ref().to_string();
        let current = Version::parse(current.as_ref()).map_err(|e| Error::InvalidTag {
            tag: current.as_ref().to_string(),
            source: e,
        })?;

        let cache_dir = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".cache")).join(&bin);

        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from(".local/share"))
            .join(&bin);

        Ok(Self {
            repo,
            bin,
            current,
            cache_ttl: Duration::from_secs(DEFAULT_CACHE_TTL_SECS),
            cache_dir,
            data_dir,
            install_path: None,
            token: TokenSource::EnvAuto,
            network_timeout: Duration::from_secs(DEFAULT_NETWORK_TIMEOUT_SECS),
            download_timeout: Duration::from_secs(DEFAULT_DOWNLOAD_TIMEOUT_SECS),
        })
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    pub fn with_network_timeout(mut self, timeout: Duration) -> Self {
        self.network_timeout = timeout;
        self
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = TokenSource::Explicit(token);
        self
    }

    pub fn with_install_path(mut self, path: PathBuf) -> Self {
        self.install_path = Some(path);
        self
    }

    pub(crate) fn resolve_token(&self) -> Option<String> {
        match &self.token {
            TokenSource::EnvAuto => std::env::var("GH_TOKEN")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty())),
            TokenSource::Explicit(t) => t.clone(),
        }
    }

    /// Returns `Some(Update)` if a newer release exists, `None` if already current.
    /// Honors the cache; only hits GitHub if cache is stale or absent.
    pub fn check_latest(&self) -> Result<Option<Update>> {
        log::debug!(
            "check_latest: repo={} bin={} current={} ttl={}s",
            self.repo.as_path(),
            self.bin,
            self.current,
            self.cache_ttl.as_secs()
        );

        if let Some(cached) = cache::load(&self.cache_dir) {
            if cached.is_fresh(self.cache_ttl) {
                log::debug!("check_latest: cache hit, latest={}", cached.latest_version);
                return self.compare_cached(&cached.latest_version);
            }
            log::debug!("check_latest: cache stale, refreshing");
        }

        self.refresh_and_compare(false)
    }

    /// Force a network call, bypassing the cache. Still updates the cache afterward.
    pub fn check_latest_refresh(&self) -> Result<Option<Update>> {
        log::debug!("check_latest_refresh: {}", self.repo.as_path());
        self.refresh_and_compare(true)
    }

    fn compare_cached(&self, latest_str: &str) -> Result<Option<Update>> {
        let latest = parse_tag(latest_str)?;
        if latest > self.current {
            Ok(Some(Update {
                current: self.current.clone(),
                latest: latest.clone(),
                tag: format!("v{latest}"),
                release_url: String::new(),
                published_at: chrono::DateTime::<chrono::Utc>::from(std::time::UNIX_EPOCH),
            }))
        } else {
            Ok(None)
        }
    }

    fn refresh_and_compare(&self, force: bool) -> Result<Option<Update>> {
        let lock_path = cache::lock_path(&self.cache_dir);
        let _ = std::fs::create_dir_all(&self.cache_dir);

        // Acquire a non-blocking exclusive lock to serialize network calls.
        // If the lock is held (another process is refreshing), fall through to
        // whatever is in the cache (possibly stale) rather than blocking.
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path);

        let lock_file = match lock_file {
            Ok(f) => f,
            Err(e) => {
                log::warn!("cache: could not open lock file: {e}; falling through to cache");
                return self.fallback_to_cache();
            }
        };

        // try_lock is available on File in Rust 1.89+
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(_) if !force => {
                log::debug!("check_latest: lock held by another process, using existing cache");
                return self.fallback_to_cache();
            }
            Err(e) => {
                log::warn!("check_latest: could not acquire lock: {e}; using existing cache");
                return self.fallback_to_cache();
            }
        }

        let token = self.resolve_token();
        let result = github::latest_release(&self.repo.as_path(), token.as_deref(), self.network_timeout);

        let info = match result {
            Ok(info) => info,
            Err(e) => {
                log::warn!("check_latest: network error: {e}; falling back to cache");
                return self.fallback_to_cache().or(Err(e));
            }
        };

        let latest = match parse_tag(&info.tag_name) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("check_latest: bad tag {}: {e}", info.tag_name);
                return Err(e);
            }
        };

        let entry = cache::CacheEntry {
            latest_version: latest.to_string(),
            checked_at: Utc::now(),
        };
        if let Err(e) = cache::save(&self.cache_dir, &entry) {
            log::warn!("check_latest: could not save cache: {e}");
        }

        if latest > self.current {
            let published_at = info.published_at.parse().unwrap_or(Utc::now());
            Ok(Some(Update {
                current: self.current.clone(),
                latest,
                tag: info.tag_name,
                release_url: info.html_url,
                published_at,
            }))
        } else {
            Ok(None)
        }
    }

    fn fallback_to_cache(&self) -> Result<Option<Update>> {
        match cache::load(&self.cache_dir) {
            Some(cached) => self.compare_cached(&cached.latest_version),
            None => Ok(None),
        }
    }

    /// Verify the install path is writable without doing a full install.
    pub fn preflight(&self) -> Result<()> {
        let path = self.resolve_install_path()?;
        log::debug!("preflight: path={:?}", path);
        if path.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .truncate(false)
                .open(&path)
                .map(|_| ())
                .map_err(|e| Error::InstallPath { path, source: e })
        } else {
            let parent = path
                .parent()
                .ok_or_else(|| Error::InstallPath {
                    path: path.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "install path has no parent"),
                })?
                .to_path_buf();
            let sentinel = parent.join(".renew-preflight");
            std::fs::write(&sentinel, b"").map_err(|e| Error::InstallPath {
                path: path.clone(),
                source: e,
            })?;
            let _ = std::fs::remove_file(&sentinel);
            Ok(())
        }
    }

    /// Download, verify, extract, backup, and atomically replace the binary.
    pub fn install_latest(&self) -> Result<InstalledVersion> {
        log::debug!(
            "install_latest: repo={} download_timeout={}s",
            self.repo.as_path(),
            self.download_timeout.as_secs()
        );
        let platform = platform::current_platform()?;
        let info = github::latest_release(
            &self.repo.as_path(),
            self.resolve_token().as_deref(),
            self.network_timeout,
        )?;
        let asset_name = format!("{}-{}-{}.tar.gz", self.bin, info.tag_name, platform);
        let asset = info.find_asset(&asset_name).ok_or(Error::AssetMissing {
            os: platform.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        })?;
        log::debug!("install_latest: asset={}", asset.name);
        let dest = self.cache_dir.join("download").join(&asset.name);
        let sha_name = format!("{asset_name}.sha256");
        let sha_url = asset.browser_download_url.replace(&asset_name, &sha_name);
        log::debug!(
            "install_latest: downloading {} -> {:?}",
            asset.browser_download_url,
            dest
        );
        github::download_asset(
            &asset.browser_download_url,
            self.resolve_token().as_deref(),
            &dest,
            self.download_timeout,
        )?;
        log::debug!("install_latest: sha_url={}", sha_url);
        // Phase 4: full pipeline (verify, extract, backup, replace)
        Err(Error::NoRelease {
            repo: self.repo.as_path(),
        })
    }

    /// Install a specific version.
    pub fn install_version(&self, version: &Version) -> Result<InstalledVersion> {
        let tag = format!("v{version}");
        let parsed = parse_tag(&tag)?;
        let platform = platform::current_platform()?;
        log::debug!("install_version: {} platform={}", parsed, platform);
        // Phase 4: implement install pipeline
        Err(Error::NoRelease {
            repo: self.repo.as_path(),
        })
    }

    /// Restore the backup; consumes (deletes) it.
    pub fn revert(&self) -> Result<InstalledVersion> {
        log::debug!("revert: data_dir={:?}", self.data_dir);
        // Phase 4: implement revert
        Err(Error::NoBackup)
    }

    /// Whether a backup exists for the current install path.
    pub fn has_backup(&self) -> bool {
        self.backup_dir().map(|d| d.join("meta.yml").exists()).unwrap_or(false)
    }

    /// If a newer version is available and stderr is a TTY, print a notice.
    /// Swallows all errors.
    pub fn notify_if_outdated(&self) {
        if !std::io::stderr().is_terminal() {
            return;
        }
        match self.check_latest() {
            Ok(Some(update)) => {
                eprintln!(
                    "{}: new version {} available (currently {})  \u{2192} {} update install",
                    self.bin, update.latest, update.current, self.bin
                );
            }
            Ok(None) => {}
            Err(e) => {
                log::debug!("notify_if_outdated: suppressed error: {e}");
            }
        }
    }

    pub(crate) fn resolve_install_path(&self) -> Result<PathBuf> {
        match &self.install_path {
            Some(p) => Ok(p.clone()),
            None => std::env::current_exe().map_err(Error::Io),
        }
    }

    pub(crate) fn backup_dir(&self) -> Option<PathBuf> {
        use sha2::Digest;
        let path = self.resolve_install_path().ok()?;
        let canonical = path.canonicalize().unwrap_or(path);
        let hash = sha2::Sha256::digest(canonical.display().to_string().as_bytes());
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        Some(self.data_dir.join(&hex[..12]).join("backup"))
    }
}

#[cfg(test)]
mod tests;
