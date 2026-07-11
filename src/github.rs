use crate::error::{Error, Result};
use serde::Deserialize;
use std::io;
use std::path::Path;
use std::time::Duration;
use ureq::config::RedirectAuthHeaders;

const GITHUB_API_BASE: &str = "https://api.github.com";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const MAX_API_BODY_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(crate) struct ReleaseInfo {
    pub(crate) tag_name: String,
    pub(crate) html_url: String,
    pub(crate) published_at: String,
    pub(crate) assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReleaseAsset {
    pub(crate) name: String,
    pub(crate) browser_download_url: String,
}

impl ReleaseInfo {
    pub(crate) fn find_asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

fn api_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .redirect_auth_headers(RedirectAuthHeaders::SameHost)
        .timeout_per_call(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

fn download_agent(timeout: Duration) -> ureq::Agent {
    // RedirectAuthHeaders::Never: GitHub asset redirects go to S3.
    // We must not forward the GitHub token to S3's presigned URLs.
    ureq::Agent::config_builder()
        .redirect_auth_headers(RedirectAuthHeaders::Never)
        .timeout_per_call(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

pub(crate) fn latest_release(repo_path: &str, token: Option<&str>, timeout: Duration) -> Result<ReleaseInfo> {
    log::debug!("latest_release: repo={} auth={}", repo_path, token.is_some());

    let agent = api_agent(timeout);
    let url = format!("{GITHUB_API_BASE}/repos/{repo_path}/releases/latest");

    let resp = make_api_get(&agent, &url, token);

    let mut response = map_api_error(resp, repo_path)?;

    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_API_BODY_BYTES)
        .read_to_string()
        .map_err(Error::Network)?;

    let info: ReleaseInfo = serde_json::from_str(&text)?;
    log::debug!("latest_release: tag={} assets={}", info.tag_name, info.assets.len());
    Ok(info)
}

pub(crate) fn download_asset(url: &str, token: Option<&str>, dest: &Path, timeout: Duration) -> Result<()> {
    log::debug!("download_asset: url={} auth={}", url, token.is_some());

    // Use the download agent (RedirectAuthHeaders::Never) to ensure the GitHub
    // token is never forwarded to the S3 presigned redirect URL.
    // The initial request to github.com carries the token; on the 302 redirect,
    // the agent issues the follow-up to S3 without any auth header.
    let agent = download_agent(timeout);

    let mut req = agent.get(url).header("Accept", "application/octet-stream");

    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }

    let resp = req.call();

    match resp {
        Ok(mut r) => {
            let mut file = std::fs::File::create(dest).map_err(Error::Io)?;
            let mut reader = r.body_mut().as_reader();
            io::copy(&mut reader, &mut file).map_err(Error::Io)?;
            Ok(())
        }
        Err(ureq::Error::StatusCode(code)) => handle_download_error(code, url),
        Err(e) => Err(Error::Network(e)),
    }
}

fn make_api_get(
    agent: &ureq::Agent,
    url: &str,
    token: Option<&str>,
) -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let mut req = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }

    req.call()
}

fn map_api_error(
    resp: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    repo_path: &str,
) -> Result<ureq::http::Response<ureq::Body>> {
    match resp {
        Ok(r) => Ok(r),
        Err(ureq::Error::StatusCode(429)) => {
            log::warn!("GitHub rate limit exceeded");
            Err(Error::RateLimited { retry_after: None })
        }
        Err(ureq::Error::StatusCode(404)) => Err(Error::NoRelease {
            repo: repo_path.to_string(),
        }),
        Err(ureq::Error::StatusCode(code)) if code >= 500 => {
            log::warn!("GitHub 5xx: {code}");
            Err(Error::Network(ureq::Error::StatusCode(code)))
        }
        Err(e) => Err(Error::Network(e)),
    }
}

fn handle_download_error(code: u16, url: &str) -> Result<()> {
    match code {
        429 => {
            log::warn!("rate limited downloading {url}");
            Err(Error::RateLimited { retry_after: None })
        }
        _ => Err(Error::Network(ureq::Error::StatusCode(code))),
    }
}

#[cfg(test)]
mod tests;
