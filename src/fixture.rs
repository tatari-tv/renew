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
                            "HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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

/// Seed `<cache_dir>/check.yml` as if a check had recorded `latest` `age` ago.
pub(crate) fn seed_cache(cache_dir: &Path, latest: &str, age: Duration) {
    let entry = CacheEntry {
        latest_version: latest.to_string(),
        checked_at: Utc::now() - chrono::Duration::from_std(age).expect("representable age"),
    };
    cache::save(cache_dir, &entry).expect("seed cache entry");
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
