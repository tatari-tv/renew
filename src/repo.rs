use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoSlug {
    owner: String,
    repo: String,
}

impl RepoSlug {
    pub(crate) fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Err(Error::InvalidRepo(
                "empty repo identifier; set `repository = \"https://github.com/owner/repo\"` in Cargo.toml or use `renew!(repo = \"...\")`".to_string(),
            ));
        }

        let slug = normalize_to_slug(input)?;

        let (owner, repo) = slug
            .split_once('/')
            .ok_or_else(|| Error::InvalidRepo(format!("expected owner/repo, got: {input}")))?;

        if owner.is_empty() || repo.is_empty() {
            return Err(Error::InvalidRepo(format!(
                "owner and repo must be non-empty, got: {input}"
            )));
        }

        if repo.contains('/') {
            return Err(Error::InvalidRepo(format!(
                "expected exactly one '/' separator, got: {input}"
            )));
        }

        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    pub(crate) fn as_path(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

fn normalize_to_slug(input: &str) -> Result<String> {
    // SSH form: git@github.com:owner/repo.git
    if let Some(rest) = input.strip_prefix("git@") {
        let rest = rest
            .split_once(':')
            .map(|(_, path)| path)
            .or_else(|| rest.split_once('/').map(|(_, path)| path))
            .ok_or_else(|| Error::InvalidRepo(format!("malformed SSH URL: {input}")))?;
        return Ok(strip_dot_git(rest).to_string());
    }

    // SSH URL form: ssh://git@github.com/owner/repo.git
    if let Some(rest) = input
        .strip_prefix("ssh://git@")
        .or_else(|| input.strip_prefix("ssh://"))
    {
        let path = rest
            .split_once('/')
            .map(|x| x.1)
            .ok_or_else(|| Error::InvalidRepo(format!("malformed SSH URL: {input}")))?;
        return Ok(strip_dot_git(path).to_string());
    }

    // HTTPS/HTTP form: https://github.com/owner/repo
    if let Some(rest) = input
        .strip_prefix("https://github.com/")
        .or_else(|| input.strip_prefix("http://github.com/"))
    {
        let rest = rest.trim_end_matches('/');
        return Ok(strip_dot_git(rest).to_string());
    }

    // Bare slug: owner/repo
    Ok(strip_dot_git(input).to_string())
}

fn strip_dot_git(s: &str) -> &str {
    s.trim_end_matches('/').strip_suffix(".git").unwrap_or(s)
}

#[cfg(test)]
mod tests;
