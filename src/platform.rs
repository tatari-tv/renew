use crate::error::{Error, Result};
use std::env;

pub(crate) fn current_platform() -> Result<&'static str> {
    // Reject musl builds: the release pipeline produces glibc binaries only.
    // A musl consumer would download a glibc binary, pass SHA verification,
    // replace itself, and then crash with a cryptic dynamic-linker error.
    if cfg!(all(target_os = "linux", target_env = "musl")) {
        return Err(Error::AssetMissing {
            os: "linux-musl".to_string(),
            arch: env::consts::ARCH.to_string(),
        });
    }

    Ok(match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => "linux-amd64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "macos-x86_64",
        ("macos", "aarch64") => "macos-arm64",
        (os, arch) => {
            return Err(Error::AssetMissing {
                os: os.to_string(),
                arch: arch.to_string(),
            });
        }
    })
}

#[cfg(test)]
mod tests;
