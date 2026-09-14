//! Test-only fixtures: a canned GitHub API and a seeded cache.
//!
//! A closed port cannot produce a 404, and a 404 is exactly the case the failed-check
//! error exists for (a private repo the caller's token cannot see). A loopback listener
//! on a thread covers it in a few lines and adds no dev-dependency.

use crate::Renew;
use crate::cache::{self, CacheEntry};
use chrono::Utc;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A loopback HTTP server that answers every request with one canned status.
pub(crate) struct CannedApi {
    base: String,
    stop: Arc<AtomicBool>,
}

impl CannedApi {
    pub(crate) fn status(code: u16, reason: &'static str) -> Self {
        Self::respond(code, reason, String::new())
    }

    /// Like [`Self::status`], but with a JSON body - the shape needed to drive the
    /// network path past a 2xx into `ReleaseInfo` deserialization, e.g. a release whose
    /// `published_at` this build cannot parse.
    pub(crate) fn json(code: u16, reason: &'static str, body: impl Into<String>) -> Self {
        Self::respond(code, reason, body.into())
    }

    fn respond(code: u16, reason: &'static str, body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!("http://{}", listener.local_addr().expect("local addr"));
        listener.set_nonblocking(true).expect("nonblocking listener");

        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut sock, _)) => {
                        // Drain the request line/headers so the client sees a clean
                        // response rather than a reset peer.
                        let mut buf = [0u8; 1024];
                        let _ = sock.read(&mut buf);
                        let _ = write!(
                            sock,
                            "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });

        Self { base, stop }
    }

    pub(crate) fn base(&self) -> &str {
        &self.base
    }
}

impl Drop for CannedApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A `Renew` whose GitHub calls land on `api` and whose cache is `cache_dir`, so nothing
/// depends on the machine's network or on `~/.cache`.
pub(crate) fn offline_renew(api: &CannedApi, cache_dir: &Path) -> Renew {
    let mut renew = Renew::new("tatari-tv/ccu", "ccu", "0.4.3")
        .expect("valid slug and version")
        .with_cache_dir(cache_dir.to_path_buf())
        .with_token(None);
    renew.api_base = api.base().to_string();
    renew
}

/// Seed `<cache_dir>/check.yml` as if a check had recorded `latest` `age` ago, with a
/// release date and URL so the entry is usable for a report (the shape a healthy,
/// post-schema-bump cache is in). `seed_legacy_cache` covers the field-less shape that
/// every cache file in the fleet actually carries today.
pub(crate) fn seed_cache(cache_dir: &Path, latest: &str, age: Duration) {
    let checked_at = Utc::now() - chrono::Duration::from_std(age).expect("representable age");
    seed_cache_with(cache_dir, latest, checked_at, checked_at, &default_release_url(latest));
}

/// Seed a cache entry with an explicit `checked_at` and `published_at`, for a test that
/// asserts a SPECIFIC rendered date rather than merely "some date `age` ago".
pub(crate) fn seed_cache_with(
    cache_dir: &Path,
    latest: &str,
    checked_at: chrono::DateTime<Utc>,
    published_at: chrono::DateTime<Utc>,
    release_url: &str,
) {
    let entry = CacheEntry {
        latest_version: latest.to_string(),
        checked_at,
        published_at: Some(published_at),
        release_url: Some(release_url.to_string()),
    };
    cache::save(cache_dir, &entry).expect("seed cache entry");
}

fn default_release_url(latest: &str) -> String {
    format!("https://github.com/tatari-tv/ccu/releases/tag/v{latest}")
}

/// Seed a PRE-schema-bump cache entry: `latest-version` and `checked-at` only, written
/// as raw YAML rather than through `CacheEntry` so it genuinely lacks the two new keys
/// rather than merely setting them to `None` (which would still round-trip as `null`,
/// not "absent"). Every cache file in the fleet is this shape today, and it must read as
/// a cache MISS rather than feed a report with data it doesn't have.
pub(crate) fn seed_legacy_cache(cache_dir: &Path, latest: &str, age: Duration) {
    std::fs::create_dir_all(cache_dir).expect("cache dir");
    let checked_at = Utc::now() - chrono::Duration::from_std(age).expect("representable age");
    let yaml = format!("latest-version: {latest}\nchecked-at: {}\n", checked_at.to_rfc3339());
    std::fs::write(cache_dir.join(cache::CACHE_FILE), yaml).expect("write legacy cache entry");
}

/// Hold `<cache_dir>/refresh.lock` the way a peer mid-refresh would.
pub(crate) fn hold_refresh_lock(cache_dir: &Path) -> std::fs::File {
    std::fs::create_dir_all(cache_dir).expect("cache dir");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cache::lock_path(cache_dir))
        .expect("open lock file");
    file.try_lock().expect("lock is free in this test");
    file
}
