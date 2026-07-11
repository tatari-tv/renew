#![deny(clippy::unwrap_used)]
#![deny(dead_code)]
#![deny(unused_variables)]

mod backup;
mod cache;
mod cmd;
mod error;
mod github;
mod install;
mod platform;
mod renew;
mod repo;
mod version;

pub use cmd::UpdateCmd;
pub use error::{Error, Result};
pub use renew::Renew;
pub use version::{InstalledVersion, Update};

/// Internal: the single source-selection expression shared by every `renew!()` arm.
///
/// Expands at the **consumer's** call site (macro expansion happens during the
/// consumer crate's compilation), so `option_env!`/`env!` read the consumer build
/// script's `GIT_DESCRIBE` and the consumer's `CARGO_PKG_VERSION` — never renew's own.
/// The `.filter` guards a consumer build script that emits an empty/whitespace
/// `GIT_DESCRIBE` on git failure, falling back to `CARGO_PKG_VERSION`.
///
/// Not part of the public API; call `renew!()` instead.
#[macro_export]
#[doc(hidden)]
macro_rules! __renew_current_version {
    () => {
        option_env!("GIT_DESCRIBE")
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(env!("CARGO_PKG_VERSION"))
    };
}

/// Construct a [`Renew`] from crate metadata, returning `Result<Renew, Error>`.
///
/// The no-arg form reads `CARGO_PKG_REPOSITORY` (the repo to check for releases)
/// and `CARGO_PKG_NAME` (the binary/asset name to download) plus the current
/// version from `GIT_DESCRIBE`/`CARGO_PKG_VERSION`:
///
/// ```ignore
/// renew!()                                  // repo + bin from Cargo metadata
/// renew!(bin = "marquee")                   // override bin: package marquee-cli, binary marquee
/// renew!(repo = "tatari-tv/marquee")        // override repo
/// renew!(bin = "marquee", repo = "…")       // override both (either order)
/// ```
///
/// Override `bin` whenever the crate name differs from the shipped binary/asset
/// name - otherwise renew looks for `<CARGO_PKG_NAME>-vX.Y.Z-<platform>.tar.gz`,
/// which won't exist (it compiles and passes CI, then fails at runtime). Reach for
/// [`Renew::new`] directly when you also need to tune the TTL, install path, token,
/// or timeout.
#[macro_export]
macro_rules! renew {
    () => {
        $crate::Renew::new(
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_NAME"),
            $crate::__renew_current_version!(),
        )
    };
    (bin = $bin:expr) => {
        $crate::Renew::new(env!("CARGO_PKG_REPOSITORY"), $bin, $crate::__renew_current_version!())
    };
    (repo = $repo:expr) => {
        $crate::Renew::new($repo, env!("CARGO_PKG_NAME"), $crate::__renew_current_version!())
    };
    (repo = $repo:expr, bin = $bin:expr) => {
        $crate::Renew::new($repo, $bin, $crate::__renew_current_version!())
    };
    (bin = $bin:expr, repo = $repo:expr) => {
        $crate::Renew::new($repo, $bin, $crate::__renew_current_version!())
    };
}

#[cfg(test)]
mod tests;
