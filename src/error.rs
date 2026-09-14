use chrono::{DateTime, Utc};
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid repo identifier: {0}")]
    InvalidRepo(String),

    #[error("malformed release tag {tag}: {source}")]
    InvalidTag {
        tag: String,
        #[source]
        source: semver::Error,
    },

    #[error("no release found for {repo}")]
    NoRelease { repo: String },

    /// GitHub returned a release whose `published_at` this build cannot parse. Reported
    /// rather than silently defaulted to "now": a report that does not know the release
    /// date must say so, not invent one (see the design doc's D3).
    #[error("release date {value:?} could not be parsed: {source}")]
    InvalidPublishedAt {
        value: String,
        #[source]
        source: chrono::ParseError,
    },

    /// An update check that did not succeed, whether or not the cache held something.
    ///
    /// It exists because reusing the inner error lies: a private-repo 404 arrives here as
    /// [`Error::NoRelease`], which on its own reads "the repo has no releases" when the
    /// truth is that the caller's token cannot see it. The cause is named, never renamed.
    #[error("{}", check_failed_message(.source, .stale))]
    CheckFailed {
        /// What the cache knew, when it knew anything: context for the report, never the
        /// verdict. A check that did not happen cannot say "up to date".
        stale: Option<StaleCache>,
        source: Box<Error>,
    },

    #[error("rate limited by GitHub; retry after {retry_after:?}")]
    RateLimited { retry_after: Option<Duration> },

    #[error("no release asset for platform {os}-{arch}")]
    AssetMissing { os: String, arch: String },

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("tarball must contain exactly one file; found {count}")]
    TarballShape { count: usize },

    #[error("cannot write to install path {path}: {source}")]
    InstallPath {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("no backup available to revert to")]
    NoBackup,

    #[error("a confirmation prompt was required but stdin is not a TTY; use --yes to bypass")]
    PromptRequiredButStdinNotTty,

    #[error("network error: {0}")]
    Network(#[from] ureq::Error),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// The cache entry a failed check found, rendered as context alongside the failure.
#[derive(Debug, Clone)]
pub struct StaleCache {
    pub latest_version: String,
    pub checked_at: DateTime<Utc>,
}

impl fmt::Display for StaleCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "last seen {} (checked at {})",
            self.latest_version,
            self.checked_at.to_rfc3339()
        )
    }
}

fn check_failed_message(source: &Error, stale: &Option<StaleCache>) -> String {
    match stale {
        None => format!("the check did not succeed and there was no usable cache: {source}"),
        Some(stale) => format!("the check did not succeed: {source}; from cache, unverified: {stale}"),
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
