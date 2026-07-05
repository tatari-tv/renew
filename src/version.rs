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

/// Parse the running binary's version from a `GIT_DESCRIBE`-or-`CARGO_PKG_VERSION`
/// string. Accepts: `1.2.1`, `v1.2.1`, `v1.2.1-3-gabc123`, `v1.2.1-3-gabc123-dirty`,
/// and a genuine prerelease `1.2.1-rc.1` (left intact). Errors on a bare SHA or an
/// empty/whitespace string.
pub(crate) fn parse_current(input: &str) -> Result<Version> {
    let s = input.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let core = strip_describe_suffix(s);
    Version::parse(core).map_err(|e| Error::InvalidTag {
        tag: input.to_string(),
        source: e,
    })
}

/// Remove a `git describe` suffix `-<count>-g<sha>[-dirty]`, leaving the base tag.
///
/// Scans from the **right** (the describe suffix is always the final one), so a base
/// tag that itself carries a prerelease (`1.2.1-rc.1-3-gabc`) keeps `1.2.1-rc.1`, and a
/// tag literally embedding the pattern is not over-truncated. A plain semver prerelease
/// (no trailing `-<digits>-g<hex>`) is returned unchanged. No `regex` dep: a ~20-char
/// right-to-left scan does not justify one.
fn strip_describe_suffix(s: &str) -> &str {
    // Optional trailing `-dirty` marker sits outside the `-<N>-g<sha>` core.
    let without_dirty = s.strip_suffix("-dirty").unwrap_or(s);
    // Rightmost segment must be `g<hex...>` (the abbreviated object name).
    let Some((prefix, last)) = without_dirty.rsplit_once('-') else {
        return s;
    };
    let Some(hex) = last.strip_prefix('g') else {
        return s;
    };
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return s;
    }
    // The segment before it must be the commit count `<digits>`.
    let Some((base, count)) = prefix.rsplit_once('-') else {
        return s;
    };
    if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) {
        return s;
    }
    base
}

#[cfg(test)]
mod tests;
