//! Locate the `tailscaled` and `tailscale` binaries this crate drives.
//!
//! Order: env override (`LEANZERO_TAILSCALED` / `LEANZERO_TAILSCALE_CLI`), then every
//! PATH directory, then the known install locations. An env override that points at
//! nothing is a hard error — it never falls through to a different binary. Absence is a
//! loud typed error listing every place that was searched; embedding the binaries in an
//! app bundle is a packaging concern outside this crate, which only consumes paths.

use std::env;
use std::path::{Path, PathBuf};

use thiserror::Error;

pub const TAILSCALED_ENV: &str = "LEANZERO_TAILSCALED";
pub const TAILSCALE_CLI_ENV: &str = "LEANZERO_TAILSCALE_CLI";

const TAILSCALED_KNOWN: &[&str] = &[
    "/opt/homebrew/bin/tailscaled",
    "/usr/local/bin/tailscaled",
    "/usr/sbin/tailscaled",
    "/usr/bin/tailscaled",
];

const TAILSCALE_CLI_KNOWN: &[&str] = &[
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
    "/usr/bin/tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
];

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error(
        "${var} is set to '{}' but no executable exists there — refusing to fall through \
         to a different binary; fix or unset ${var}",
        path.display()
    )]
    EnvOverrideMissing { var: &'static str, path: PathBuf },
    #[error(
        "could not find '{name}': ${var} is unset, no executable '{name}' in any PATH \
         directory [{path_dirs}], and none of the known locations exist [{known}]. \
         Install tailscale (e.g. `brew install tailscale`) or point ${var} at the binary"
    )]
    NotFound {
        name: &'static str,
        var: &'static str,
        path_dirs: String,
        known: String,
    },
}

pub fn find_tailscaled() -> Result<PathBuf, DiscoveryError> {
    discover(
        "tailscaled",
        TAILSCALED_ENV,
        env::var_os(TAILSCALED_ENV).map(PathBuf::from),
        &path_dirs(),
        TAILSCALED_KNOWN,
    )
}

pub fn find_tailscale_cli() -> Result<PathBuf, DiscoveryError> {
    discover(
        "tailscale",
        TAILSCALE_CLI_ENV,
        env::var_os(TAILSCALE_CLI_ENV).map(PathBuf::from),
        &path_dirs(),
        TAILSCALE_CLI_KNOWN,
    )
}

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// The pure search order behind [`find_tailscaled`] / [`find_tailscale_cli`], exposed
/// so callers (and tests) can drive it with an explicit environment.
pub fn discover(
    name: &'static str,
    var: &'static str,
    env_override: Option<PathBuf>,
    path_dirs: &[PathBuf],
    known: &[&str],
) -> Result<PathBuf, DiscoveryError> {
    if let Some(path) = env_override {
        return if is_executable(&path) {
            Ok(path)
        } else {
            Err(DiscoveryError::EnvOverrideMissing { var, path })
        };
    }
    for dir in path_dirs {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    for candidate in known {
        let candidate = Path::new(candidate);
        if is_executable(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }
    Err(DiscoveryError::NotFound {
        name,
        var,
        path_dirs: path_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        known: known.join(", "),
    })
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && std::fs::metadata(path)
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
