use std::sync::OnceLock;
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW_FLAG: u32 = 0x08000000;

pub trait SubprocessExt {
    fn set_no_window(&mut self) -> &mut Self;
}

impl SubprocessExt for Command {
    fn set_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            self.creation_flags(CREATE_NO_WINDOW_FLAG);
        }
        self
    }
}

impl SubprocessExt for std::process::Command {
    fn set_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW_FLAG);
        }
        self
    }
}

/// Resolve the user's full PATH by running a login shell.
///
/// When goosed is launched from a desktop app (e.g. Electron), it may inherit
/// a minimal PATH like `/usr/bin:/bin`. This function spawns a login shell to
/// source the user's profile and recover the full PATH.
///
/// Ported from `crates/goose/src/agents/platform_extensions/developer/shell.rs`
/// where it was introduced in #5774 for the developer extension. This makes the
/// same fix available to all MCP extensions in goose-mcp.
#[cfg(not(windows))]
fn resolve_login_shell_path() -> Option<String> {
    use process_wrap::std::{CommandWrap, ProcessSession};
    use std::path::PathBuf;
    use std::process::Stdio;

    // Prefer the user's configured shell so we source the right profile files.
    // Fall back to /bin/bash (common default) then sh as last resort.
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| PathBuf::from(s).is_file())
        .unwrap_or_else(|| {
            if PathBuf::from("/bin/bash").is_file() {
                "/bin/bash".to_string()
            } else {
                "sh".to_string()
            }
        });

    let mut cmd = CommandWrap::from(std::process::Command::new(&shell));
    cmd.command_mut()
        .args(["-l", "-i", "-c", "echo $PATH"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let (Some(dir), Ok(inherited)) = (tool_shim_dir(), std::env::var("PATH")) {
        cmd.command_mut()
            .env("PATH", path_without(&inherited, &dir));
    }

    // Spawn in a new session so that interactive shell job-control setup
    // cannot steal the terminal foreground from the parent goose process.
    cmd.wrap(ProcessSession);

    let child = cmd.spawn().ok()?;
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    // Take the last non-empty line — interactive shells may emit
    // extra output from profile scripts before our echo.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .filter(|path| !path.is_empty())
}

/// goose's bundled runtime shims (node, npx, uvx, jbang), which the desktop puts first on goose
/// serve's PATH so MCP servers find a runtime. A script the user asks for must run the user's own
/// runtime, so the shims are withheld from the login probe and merged last — the same rule as the
/// developer shell tool (`crates/goose/src/agents/platform_extensions/developer/shell.rs`,
/// `TOOL_SHIM_DIR_ENV`), which this crate cannot import (Q-102).
#[cfg(not(windows))]
const TOOL_SHIM_DIR_ENV: &str = "GOOSE_TOOL_SHIM_DIR";

#[cfg(not(windows))]
fn tool_shim_dir() -> Option<String> {
    std::env::var(TOOL_SHIM_DIR_ENV)
        .ok()
        .filter(|dir| !dir.is_empty())
}

#[cfg(not(windows))]
fn path_without(path: &str, dir: &str) -> String {
    let dir = dir.trim_end_matches('/');
    path.split(':')
        .filter(|entry| entry.trim_end_matches('/') != dir)
        .collect::<Vec<_>>()
        .join(":")
}

/// Returns the user's full login shell PATH, resolved once and cached.
///
/// Call this before spawning subprocesses to ensure they inherit the user's
/// full PATH rather than the restricted one from the desktop app launcher.
#[cfg(not(windows))]
pub fn user_login_path() -> Option<&'static str> {
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED.get_or_init(resolve_login_shell_path).as_deref()
}

/// Merge the login shell PATH with the current process PATH.
///
/// Prepends login shell entries so user tools are found first, while
/// preserving any runtime PATH additions (e.g. from direnv, nix, or
/// auto-install helpers like ensure_peekaboo).
#[cfg(not(windows))]
pub fn merged_path() -> Option<String> {
    let login = user_login_path()?;
    let current = std::env::var("PATH").unwrap_or_default();
    Some(merge_login_path(
        login,
        &current,
        tool_shim_dir().as_deref(),
    ))
}

#[cfg(not(windows))]
fn merge_login_path(login: &str, current: &str, shim_dir: Option<&str>) -> String {
    // Deduplicate: login shell entries first, then any current entries not already present.
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<&str> = login
        .split(':')
        .chain(current.split(':'))
        .filter(|entry| !entry.is_empty() && seen.insert(*entry))
        .collect();
    if let Some(dir) = shim_dir {
        let dir = dir.trim_end_matches('/');
        let before = merged.len();
        merged.retain(|entry| entry.trim_end_matches('/') != dir);
        if merged.len() != before {
            merged.push(dir);
        }
    }
    merged.join(":")
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn goose_tool_shims_merge_behind_the_users_path() {
        let shims = "/Applications/Goose Swarm.app/Contents/Resources/bin";
        assert_eq!(
            merge_login_path(
                "/Users/u/.nvm/versions/node/v22/bin:/usr/bin",
                &format!("{shims}:/usr/bin:/bin"),
                Some(shims)
            ),
            format!("/Users/u/.nvm/versions/node/v22/bin:/usr/bin:/bin:{shims}")
        );
        assert_eq!(
            merge_login_path("/usr/bin", &format!("{shims}:/bin"), None),
            format!("/usr/bin:{shims}:/bin")
        );
        assert_eq!(
            merge_login_path("/usr/bin", "/bin", Some(shims)),
            "/usr/bin:/bin"
        );
        assert_eq!(
            path_without(&format!("{shims}/:/usr/bin"), shims),
            "/usr/bin"
        );
    }
}
