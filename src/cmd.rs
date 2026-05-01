use crate::Renew;
use crate::backup;
use crate::error::Error;
use crate::install;
use crate::version::parse_tag;
use semver::Version;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct UpdateCmd {
    #[command(subcommand)]
    cmd: Option<UpdateSub>,
}

#[derive(clap::Subcommand, Debug)]
enum UpdateSub {
    /// Check whether an update is available (exit 0 = current, 1 = update available, 2 = error).
    Check {
        /// Bypass the cache and force a network call.
        #[arg(long)]
        refresh: bool,
    },
    /// Install a specific version (defaults to latest).
    Install {
        /// Specific version to install (e.g. 0.5.0). Defaults to latest.
        version: Option<String>,
        /// Reinstall even if already on this version.
        #[arg(long)]
        force: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Bypass the cache when resolving latest.
        #[arg(long)]
        refresh: bool,
        /// Override the install path (default: current executable).
        #[arg(long)]
        install_path: Option<PathBuf>,
    },
    /// Revert to the previously installed version.
    Revert {
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Override the install path (default: current executable).
        #[arg(long)]
        install_path: Option<PathBuf>,
    },
}

impl UpdateCmd {
    /// Run the subcommand. Returns a process exit code.
    ///
    /// Exit codes:
    ///   0 - success / already current
    ///   1 - update available (check subcommand only)
    ///   2 - error (rendered to stderr)
    pub fn run(&self, renew: &Renew) -> i32 {
        match self.run_inner(renew) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {e}");
                2
            }
        }
    }

    fn run_inner(&self, renew: &Renew) -> crate::Result<i32> {
        match &self.cmd {
            None | Some(UpdateSub::Check { refresh: false }) => run_check(renew, false),
            Some(UpdateSub::Check { refresh: true }) => run_check(renew, true),
            Some(UpdateSub::Install {
                version,
                force,
                yes,
                refresh,
                install_path,
            }) => run_install(
                renew,
                version.as_deref(),
                *force,
                *yes,
                *refresh,
                install_path.clone(),
            ),
            Some(UpdateSub::Revert { yes, install_path }) => {
                run_revert(renew, *yes, install_path.clone())
            }
        }
    }
}

fn run_check(renew: &Renew, refresh: bool) -> crate::Result<i32> {
    let update = if refresh {
        renew.check_latest_refresh()?
    } else {
        renew.check_latest()?
    };

    match update {
        None => {
            println!("{} {} (latest)", renew.bin, renew.current);
            Ok(0)
        }
        Some(u) => {
            println!(
                "{} {} \u{2192} {} available (released {})",
                renew.bin,
                u.current,
                u.latest,
                u.published_at.format("%Y-%m-%d")
            );
            Ok(1)
        }
    }
}

fn run_install(
    renew: &Renew,
    version: Option<&str>,
    force: bool,
    yes: bool,
    refresh: bool,
    install_path: Option<PathBuf>,
) -> crate::Result<i32> {
    let renew = match install_path {
        Some(p) => std::borrow::Cow::Owned(renew.clone().with_install_path(p)),
        None => std::borrow::Cow::Borrowed(renew),
    };
    let renew = renew.as_ref();

    let target: Version = match version {
        Some(v) => parse_tag(v)?,
        None => {
            let update = if refresh {
                renew.check_latest_refresh()?
            } else {
                renew.check_latest()?
            };
            match update {
                Some(u) => u.latest,
                None if force => renew.current.clone(),
                None => {
                    println!(
                        "{}: already on {} (latest). Use --force to reinstall.",
                        renew.bin, renew.current
                    );
                    return Ok(0);
                }
            }
        }
    };

    if target == renew.current && !force {
        println!(
            "{}: already on {} (latest). Use --force to reinstall.",
            renew.bin, renew.current
        );
        return Ok(0);
    }

    let install_path_display = renew
        .install_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "(unknown)".to_string())
        });

    if !confirm(
        &format!(
            "{}: install {} over {} (replacing {})? [y/N] ",
            renew.bin, target, renew.current, install_path_display
        ),
        yes,
    )? {
        return Ok(0);
    }

    let result = if version.is_some() {
        renew.install_version(&target)?
    } else {
        renew.install_latest()?
    };

    println!(
        "{}: installed {} (was {})",
        renew.bin, result.to, result.from
    );
    Ok(0)
}

fn run_revert(renew: &Renew, yes: bool, install_path: Option<PathBuf>) -> crate::Result<i32> {
    let renew = match install_path {
        Some(p) => std::borrow::Cow::Owned(renew.clone().with_install_path(p)),
        None => std::borrow::Cow::Borrowed(renew),
    };
    let renew = renew.as_ref();

    let resolved = renew.resolve_install_path()?;
    let backup_dir = install::backup_dir_for(&resolved, &renew.data_dir);

    if !backup::exists(&backup_dir) {
        return Err(Error::NoBackup);
    }

    // Peek the backup version so the prompt is informative.
    let backup_version = backup::peek(&backup_dir)
        .map(|m| m.version)
        .unwrap_or_else(|| "(unknown)".to_string());

    let install_path_display = resolved.display().to_string();

    if !confirm(
        &format!(
            "{}: revert {} \u{2192} {} (restoring {})? [y/N] ",
            renew.bin, renew.current, backup_version, install_path_display
        ),
        yes,
    )? {
        return Ok(0);
    }

    let result = renew.revert()?;
    println!(
        "{}: reverted to {} (was {}). No further previous version available.",
        renew.bin, result.from, result.to
    );
    Ok(0)
}

/// Prompt the user for confirmation. Returns Ok(true) to proceed, Ok(false) for a clean
/// user-initiated cancel. Errors only if stdin is not a TTY and --yes was not passed.
fn confirm(prompt: &str, yes: bool) -> crate::Result<bool> {
    if yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Err(Error::PromptRequiredButStdinNotTty);
    }
    eprint!("{prompt}");
    io::stderr().flush().map_err(Error::Io)?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(Error::Io)?;
    let trimmed = input.trim().to_lowercase();
    Ok(trimmed == "y" || trimmed == "yes")
}

#[cfg(test)]
mod tests;
