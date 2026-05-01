use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use semver::Version;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Update {
    pub current: Version,
    pub latest: Version,
    pub tag: String,
    pub release_url: String,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct InstalledVersion {
    pub from: Version,
    pub to: Version,
    pub path: PathBuf,
}

pub(crate) fn parse_tag(tag: &str) -> Result<Version> {
    let stripped = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(stripped).map_err(|e| Error::InvalidTag {
        tag: tag.to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests;
