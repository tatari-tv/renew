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

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
