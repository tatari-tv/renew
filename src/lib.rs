#![deny(clippy::unwrap_used)]
#![deny(dead_code)]
#![deny(unused_variables)]

mod cache;
mod error;
mod github;
mod platform;
mod renew;
mod repo;
mod version;

pub use error::{Error, Result};
pub use renew::Renew;
pub use version::{InstalledVersion, Update};

#[macro_export]
macro_rules! renew {
    () => {
        $crate::Renew::new(
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        )
    };
    (bin = $bin:expr) => {
        $crate::Renew::new(env!("CARGO_PKG_REPOSITORY"), $bin, env!("CARGO_PKG_VERSION"))
    };
    (repo = $repo:expr) => {
        $crate::Renew::new($repo, env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    };
    (repo = $repo:expr, bin = $bin:expr) => {
        $crate::Renew::new($repo, $bin, env!("CARGO_PKG_VERSION"))
    };
    (bin = $bin:expr, repo = $repo:expr) => {
        $crate::Renew::new($repo, $bin, env!("CARGO_PKG_VERSION"))
    };
}
