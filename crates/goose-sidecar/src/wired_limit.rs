//! The macOS GPU wired limit (`iogpu.wired_limit_mb`): how much of physical RAM Metal may wire,
//! which bounds every engine's working set. 0 = the macOS default. Raising it needs root, so goose
//! only ever asks macOS to run one validated `sysctl` through its own administrator prompt (the
//! person types their password; nothing is silent), and a reboot resets it unless the person also
//! installs the LaunchDaemon below.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};

pub const SYSCTL_KEY: &str = "iogpu.wired_limit_mb";
pub const SYSCTL_BIN: &str = "/usr/sbin/sysctl";
pub const DAEMON_LABEL: &str = "net.leanzero.iogpu-wired-limit";
pub const DAEMON_PATH: &str = "/Library/LaunchDaemons/net.leanzero.iogpu-wired-limit.plist";
/// The lowest non-zero limit goose applies: below it macOS and the engine would fight over a
/// few GB and the Mac stalls (owner spec, 2026-10-30: "0 or 8192..physical MB").
pub const MIN_NONZERO_MB: u64 = 8192; // measured: owner's valid range for iogpu.wired_limit_mb
const MIB: u64 = 1024 * 1024;

/// `v` when it is 0 or within 8192..=physical MB; the only gate between a number and a root shell.
pub fn validate_mb(v: u64, physical_bytes: u64) -> Result<u64> {
    let physical_mb = physical_bytes / MIB;
    ensure!(
        v == 0 || (MIN_NONZERO_MB..=physical_mb).contains(&v),
        "wired limit {v} MB: use 0 (the macOS default) or {MIN_NONZERO_MB}..{physical_mb} MB on this Mac"
    );
    Ok(v)
}

/// The one command that sets the limit, for a value [`validate_mb`] passed.
pub fn sysctl_command(v: u64) -> String {
    format!("{SYSCTL_BIN} {SYSCTL_KEY}={v}")
}

/// `osascript` asking macOS to run `shell` as root behind its own password prompt. `shell` is
/// built here from validated integers and fixed paths; a quote or backslash in it is refused so
/// it cannot leave the AppleScript string.
pub fn admin_osascript_argv(shell: &str) -> Result<Vec<String>> {
    ensure!(
        !shell.contains(['"', '\\', '\n']),
        "refusing to hand macOS an admin command with a quote, backslash or newline: {shell}"
    );
    Ok(vec![
        "/usr/bin/osascript".to_string(),
        "-e".to_string(),
        format!("do shell script \"{shell}\" with administrator privileges"),
    ])
}

/// The LaunchDaemon that sets `v` again at every boot.
pub fn daemon_plist(v: u64) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{DAEMON_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{SYSCTL_BIN}</string>
		<string>{SYSCTL_KEY}={v}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
</dict>
</plist>
"#
    )
}

fn safe_path(path: &Path) -> Result<String> {
    let text = path.to_string_lossy().into_owned();
    ensure!(
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)),
        "the staged daemon path {text} holds characters goose will not put in an admin command"
    );
    Ok(text)
}

/// Root's half of installing the daemon: copy the plist goose staged at `staged`, own it as
/// root, set the limit now, and (re)load the daemon.
pub fn install_daemon_shell(v: u64, staged: &Path) -> Result<String> {
    let staged = safe_path(staged)?;
    Ok(format!(
        "/bin/cp '{staged}' '{DAEMON_PATH}' && /usr/sbin/chown root:wheel '{DAEMON_PATH}' && \
         /bin/chmod 644 '{DAEMON_PATH}' && {} && \
         (/bin/launchctl bootout system/{DAEMON_LABEL} 2>/dev/null; \
         /bin/launchctl enable system/{DAEMON_LABEL}; \
         /bin/launchctl bootstrap system '{DAEMON_PATH}')",
        sysctl_command(v)
    ))
}

/// Root's half of removing the daemon. The live limit stays until reboot or a new value.
pub fn remove_daemon_shell() -> String {
    format!("/bin/launchctl bootout system/{DAEMON_LABEL} 2>/dev/null; /bin/rm -f '{DAEMON_PATH}'")
}

/// Where goose stages the daemon plist before root copies it.
pub fn staged_plist_path() -> PathBuf {
    std::env::temp_dir().join(format!("goose-{DAEMON_LABEL}-{}.plist", std::process::id()))
}

/// The installed daemon's limit: `None` when no daemon is installed.
pub fn installed_daemon_mb() -> Result<Option<u64>> {
    let text = match std::fs::read_to_string(DAEMON_PATH) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(DAEMON_PATH),
    };
    Ok(Some(daemon_value(&text).with_context(|| {
        format!("{DAEMON_PATH} sets no {SYSCTL_KEY}=<MB>")
    })?))
}

fn daemon_value(plist: &str) -> Option<u64> {
    let start = plist.find(&format!("{SYSCTL_KEY}="))? + SYSCTL_KEY.len() + 1;
    let digits: String = plist[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// `sysctl -n iogpu.wired_limit_mb` (0 = macOS default).
#[cfg(target_os = "macos")]
pub fn read_wired_limit_mb() -> Result<u64> {
    let out = std::process::Command::new(SYSCTL_BIN)
        .args(["-n", SYSCTL_KEY])
        .output()
        .with_context(|| format!("running {SYSCTL_BIN} -n {SYSCTL_KEY}"))?;
    ensure!(
        out.status.success(),
        "{SYSCTL_BIN} -n {SYSCTL_KEY} failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .parse()
        .with_context(|| format!("{SYSCTL_KEY} answered {text:?}"))
}

#[cfg(not(target_os = "macos"))]
pub fn read_wired_limit_mb() -> Result<u64> {
    bail!("{SYSCTL_KEY} exists only on macOS")
}

/// Runs `shell` as root behind the macOS password prompt. A cancelled prompt is its own error.
pub async fn run_as_admin(shell: &str) -> Result<()> {
    let argv = admin_osascript_argv(shell)?;
    let out = tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .await
        .with_context(|| format!("running {}", argv[0]))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("-128") {
        bail!("the administrator prompt was cancelled; nothing changed");
    }
    bail!("macOS refused the change: {}", stderr.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * MIB;

    #[test]
    fn only_zero_or_8192_to_physical_mb_pass() {
        let physical = 128 * GIB;
        assert_eq!(validate_mb(0, physical).unwrap(), 0);
        assert_eq!(validate_mb(8192, physical).unwrap(), 8192);
        assert_eq!(validate_mb(122_000, physical).unwrap(), 122_000);
        assert_eq!(validate_mb(131_072, physical).unwrap(), 131_072);
        for bad in [1, 8191, 131_073, u64::MAX] {
            let e = validate_mb(bad, physical).unwrap_err().to_string();
            assert!(e.contains("8192..131072"), "{e}");
        }
    }

    #[test]
    fn the_admin_command_is_exactly_one_sysctl() {
        let argv = admin_osascript_argv(&sysctl_command(122_000)).unwrap();
        assert_eq!(
            argv,
            vec![
                "/usr/bin/osascript",
                "-e",
                "do shell script \"/usr/sbin/sysctl iogpu.wired_limit_mb=122000\" with administrator privileges",
            ]
        );
        assert!(admin_osascript_argv("echo \"x\"; rm -rf /").is_err());
        assert!(admin_osascript_argv("a\\\" & do shell script \"b").is_err());
    }

    #[test]
    fn the_daemon_sets_the_value_at_load_and_installs_through_one_prompt() {
        let plist = daemon_plist(122_000);
        assert!(plist.contains("<string>iogpu.wired_limit_mb=122000</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(plist.contains(DAEMON_LABEL));
        assert_eq!(daemon_value(&plist), Some(122_000));

        let shell = install_daemon_shell(122_000, Path::new("/tmp/goose-x.plist")).unwrap();
        assert!(shell.starts_with("/bin/cp '/tmp/goose-x.plist' '/Library/LaunchDaemons/net.leanzero.iogpu-wired-limit.plist'"));
        assert!(shell.contains("/usr/sbin/sysctl iogpu.wired_limit_mb=122000"));
        assert!(shell.contains("/bin/launchctl bootstrap system"));
        assert!(admin_osascript_argv(&shell).is_ok());
        assert!(install_daemon_shell(1, Path::new("/tmp/a b'; rm -rf ~")).is_err());

        let remove = remove_daemon_shell();
        assert!(remove.contains("bootout system/net.leanzero.iogpu-wired-limit"));
        assert!(remove.contains("/bin/rm -f '/Library/LaunchDaemons/"));
        assert!(admin_osascript_argv(&remove).is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_mac_answers_its_wired_limit() {
        read_wired_limit_mb().unwrap();
    }
}
