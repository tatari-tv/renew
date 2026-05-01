use crate::error::{Error, Result};
use crate::platform;
use crate::repo::RepoSlug;
use crate::version::{InstalledVersion, Update, parse_tag};
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
            "check_latest: repo={} bin={} current={} ttl={}s timeout={}s cache_dir={:?}",
            self.repo.as_path(),
            self.bin,
            self.current,
            self.cache_ttl.as_secs(),
            self.network_timeout.as_secs(),
            self.cache_dir
        );
        let token = self.resolve_token();
        log::debug!("check_latest: auth={}", token.is_some());
        // Phase 3: implement cache + GitHub client
        Err(Error::NoRelease {
            repo: self.repo.as_path(),
        })
    }

    /// Force a network call, bypassing the cache.
    pub fn check_latest_refresh(&self) -> Result<Option<Update>> {
        log::debug!("check_latest_refresh: {}", self.repo.as_path());
        self.check_latest()
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
        log::debug!("install_latest: download_timeout={}s", self.download_timeout.as_secs());
        let platform = platform::current_platform()?;
        log::debug!("install_latest: platform={}", platform);
        // Phase 4: implement install pipeline
        Err(Error::NoRelease {
            repo: self.repo.as_path(),
        })
    }

    /// Install a specific version.
    pub fn install_version(&self, version: &Version) -> Result<InstalledVersion> {
        let tag = format!("v{version}");
        let parsed = parse_tag(&tag)?;
        log::debug!("install_version: {}", parsed);
        let platform = platform::current_platform()?;
        log::debug!("install_version: platform={}", platform);
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
