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

/// The RAM a raised limit must leave to macOS and every process outside the GPU, unless the
/// owner sets `GOOSE_WIRED_LIMIT_MIN_RESERVE_GIB`. Measured 2026-10-30/31 under a distributed
/// Gauntlet: the MLX engine's own memory outside Metal (Python, token buffers) held ~12 GB
/// anonymous and the kernel/system another ~10 GB, so 122,000 MB on a 128 GiB Mac (8.9 GiB left)
/// starves them; 16 GiB covers the engine's share with room for the kernel's growth.
pub const DEFAULT_MIN_RESERVE_GIB: u64 = 16;
/// The config key of the reserve above.
pub const MIN_RESERVE_CONFIG_KEY: &str = "GOOSE_WIRED_LIMIT_MIN_RESERVE_GIB";
static MIN_RESERVE_GIB: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(DEFAULT_MIN_RESERVE_GIB);

/// The reserve in force: the owner's `GOOSE_WIRED_LIMIT_MIN_RESERVE_GIB` once goose has read
/// its config ([`set_min_reserve_gib`]), else [`DEFAULT_MIN_RESERVE_GIB`]. The distributed
/// preflight reads it here (this crate has no config of its own).
pub fn min_reserve_gib() -> u64 {
    MIN_RESERVE_GIB.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_min_reserve_gib(gib: u64) {
    MIN_RESERVE_GIB.store(gib, std::sync::atomic::Ordering::Relaxed);
}

/// How long the administrator prompt waits for someone at the Mac before goose gives up (and
/// kills the prompt): a peer's prompt with nobody at it must not hold the request forever.
pub const ADMIN_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);
/// Suggestions are rounded down to this step, so the number reads like one a person types.
const SUGGEST_STEP_MB: u64 = 1000;

pub fn physical_mb(physical_bytes: u64) -> u64 {
    physical_bytes / MIB
}

/// What macOS and every process outside the GPU keep under limit `v`: `None` for 0 (the macOS
/// default, which macOS sizes itself).
pub fn keep_mb(v: u64, physical_bytes: u64) -> Option<u64> {
    (v != 0).then(|| physical_mb(physical_bytes).saturating_sub(v))
}

/// The highest limit that still leaves `reserve_gib` to macOS.
pub fn max_mb(physical_bytes: u64, reserve_gib: u64) -> u64 {
    physical_mb(physical_bytes).saturating_sub(reserve_gib * 1024)
}

/// `v` when it is 0, or within 8192..=physical MB and leaves at least `reserve_gib` to macOS;
/// the only gate between a number and a root shell.
pub fn validate_mb(v: u64, physical_bytes: u64, reserve_gib: u64) -> Result<u64> {
    let physical_mb = physical_mb(physical_bytes);
    ensure!(
        v == 0 || (MIN_NONZERO_MB..=physical_mb).contains(&v),
        "wired limit {v} MB: use 0 (the macOS default) or {MIN_NONZERO_MB}..{physical_mb} MB on this Mac"
    );
    if let Some(keep) = keep_mb(v, physical_bytes) {
        ensure!(
            keep >= reserve_gib * 1024,
            "wired limit {v} MB leaves macOS and other apps {:.1} GiB, under the {reserve_gib} GiB \
             minimum reserve (the engine also needs RAM outside the GPU); the highest allowed on \
             this Mac is {} MB",
            keep as f64 / 1024.0,
            max_mb(physical_bytes, reserve_gib)
        );
    }
    Ok(v)
}

/// The limit goose suggests: what the Mac uses outside the GPU right now (`others_bytes`) plus
/// the reserve stays with macOS, the rest may be wired; rounded down to 1,000 MB and never above
/// [`max_mb`]. `None` when that leaves less than [`MIN_NONZERO_MB`].
pub fn suggested_mb(physical_bytes: u64, others_bytes: u64, reserve_gib: u64) -> Option<u64> {
    let others_mb = others_bytes.div_ceil(MIB);
    let room = physical_mb(physical_bytes)
        .saturating_sub(others_mb)
        .saturating_sub(reserve_gib * 1024)
        .min(max_mb(physical_bytes, reserve_gib));
    let rounded = room / SUGGEST_STEP_MB * SUGGEST_STEP_MB;
    (rounded >= MIN_NONZERO_MB).then_some(rounded)
}

/// Metal's working set under limit `v`: macOS reports exactly `v` MiB (2026-10-30: 112,000 MB
/// read back as a 117.44 GB working set).
pub fn ceiling_bytes(v: u64) -> u64 {
    v * MIB
}

/// What a person at the Mac types instead, when no goose there can ask for them.
pub fn fallback_command(v: u64) -> String {
    format!("sudo {}", sysctl_command(v))
}

/// The one command that sets the limit, for a value [`validate_mb`] passed.
pub fn sysctl_command(v: u64) -> String {
    format!("{SYSCTL_BIN} {SYSCTL_KEY}={v}")
}

/// Root's half of "Disable": the macOS default again, and the boot setting removed when one is
/// installed, behind one prompt.
pub fn disable_shell(daemon_installed: bool) -> String {
    let reset = sysctl_command(0);
    if daemon_installed {
        format!("{reset} && {}", remove_daemon_shell())
    } else {
        reset
    }
}

/// The words macOS shows in its administrator prompt: what goose asks for and, for a request
/// another Mac sent, which Mac sent it. Only `[A-Za-z0-9 ._-]` of the name is kept.
pub fn prompt_text(action: &str, requested_by: Option<&str>) -> String {
    let who = requested_by
        .map(|name| {
            name.chars()
                .filter(|c| c.is_ascii_alphanumeric() || " ._-".contains(*c))
                .take(64)
                .collect::<String>()
        })
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    match who {
        Some(name) => format!("goose on {name} asks to {action} on this Mac."),
        None => format!("goose wants to {action}."),
    }
}

/// `osascript` asking macOS to run `shell` as root behind its own password prompt, showing
/// `prompt`. Both are built here from validated integers, fixed paths and filtered names; a
/// quote, backslash or newline in either is refused so it cannot leave the AppleScript string.
pub fn admin_osascript_argv(shell: &str, prompt: &str) -> Result<Vec<String>> {
    for text in [shell, prompt] {
        ensure!(
            !text.contains(['"', '\\', '\n']),
            "refusing to hand macOS an admin command with a quote, backslash or newline: {text}"
        );
    }
    Ok(vec![
        "/usr/bin/osascript".to_string(),
        "-e".to_string(),
        format!("do shell script \"{shell}\" with prompt \"{prompt}\" with administrator privileges"),
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

/// Runs `shell` as root behind the macOS password prompt. A cancelled prompt is its own error;
/// a prompt nobody answers within [`ADMIN_PROMPT_TIMEOUT`] is closed and reported as such.
pub async fn run_as_admin(shell: &str, prompt: &str) -> Result<()> {
    let argv = admin_osascript_argv(shell, prompt)?;
    let child = tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(ADMIN_PROMPT_TIMEOUT, child).await {
        Ok(out) => out.with_context(|| format!("running {}", argv[0]))?,
        Err(_) => bail!(
            "nobody approved the administrator prompt on this Mac within {} minutes; nothing \
             changed",
            ADMIN_PROMPT_TIMEOUT.as_secs() / 60
        ),
    };
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

    const R: u64 = DEFAULT_MIN_RESERVE_GIB;

    #[test]
    fn only_zero_or_8192_to_physical_mb_pass_and_the_reserve_stays_with_macos() {
        let physical = 128 * GIB;
        assert_eq!(validate_mb(0, physical, R).unwrap(), 0);
        assert_eq!(validate_mb(8192, physical, R).unwrap(), 8192);
        assert_eq!(validate_mb(114_688, physical, R).unwrap(), 114_688);
        for bad in [1, 8191, 131_073, u64::MAX] {
            let e = validate_mb(bad, physical, R).unwrap_err().to_string();
            assert!(e.contains("8192..131072"), "{e}");
        }
        // The owner's first number: 122,000 MB leaves 8.9 GiB, under the 16 GiB reserve.
        let e = validate_mb(122_000, physical, R).unwrap_err().to_string();
        assert!(e.contains("8.9 GiB"), "{e}");
        assert!(e.contains("114688 MB"), "{e}");
        assert!(validate_mb(114_689, physical, R).is_err());
        assert_eq!(validate_mb(122_000, physical, 8).unwrap(), 122_000);
    }

    #[test]
    fn the_reserve_math_on_64_96_and_128_gb_macs() {
        // (physical GiB, others measured GB, keep for 120,000 MB, max, suggestion)
        let table: [(u64, u64, Option<u64>, u64, Option<u64>); 3] = [
            (64, 8, None, 49_152, Some(41_000)),
            (96, 10, None, 81_920, Some(72_000)),
            (128, 10, Some(11_072), 114_688, Some(105_000)),
        ];
        for (gib, others_gb, keep_120k, max, suggestion) in table {
            let physical = gib * GIB;
            assert_eq!(physical_mb(physical), gib * 1024);
            assert_eq!(max_mb(physical, R), max, "{gib} GiB max");
            let keep = keep_mb(120_000, physical);
            if gib * 1024 >= 120_000 {
                assert_eq!(keep, keep_120k, "{gib} GiB keep");
            }
            assert_eq!(keep_mb(0, physical), None);
            let others = others_gb * 1_000_000_000;
            assert_eq!(suggested_mb(physical, others, R), suggestion, "{gib} GiB");
            let s = suggestion.unwrap();
            assert!(validate_mb(s, physical, R).is_ok());
            assert!(keep_mb(s, physical).unwrap() * MIB >= others + R * GIB);
            assert!(validate_mb(max, physical, R).is_ok());
            assert!(validate_mb(max + 1, physical, R).is_err());
        }
        // Nothing to suggest when others + reserve leave less than 8,192 MB.
        assert_eq!(suggested_mb(16 * GIB, 4 * GIB, R), None);
        assert_eq!(ceiling_bytes(112_000), 117_440_512_000);
    }

    #[test]
    fn enable_is_exactly_one_sysctl_behind_a_prompt_that_names_the_requester() {
        let prompt = prompt_text("set the GPU wired limit to 110000 MB", Some("Mihai's \"MBP\"\\"));
        assert_eq!(
            prompt,
            "goose on Mihais MBP asks to set the GPU wired limit to 110000 MB on this Mac."
        );
        let argv = admin_osascript_argv(&sysctl_command(110_000), &prompt).unwrap();
        assert_eq!(
            argv,
            vec![
                "/usr/bin/osascript",
                "-e",
                "do shell script \"/usr/sbin/sysctl iogpu.wired_limit_mb=110000\" with prompt \"goose on Mihais MBP asks to set the GPU wired limit to 110000 MB on this Mac.\" with administrator privileges",
            ]
        );
        assert_eq!(prompt_text("x", None), "goose wants to x.");
        assert_eq!(prompt_text("x", Some("\"\"")), "goose wants to x.");
        assert!(admin_osascript_argv("echo \"x\"; rm -rf /", "p").is_err());
        assert!(admin_osascript_argv("a\\\" & do shell script \"b", "p").is_err());
        assert!(admin_osascript_argv("ls", "p\" & do shell script \"rm").is_err());
        assert_eq!(fallback_command(110_000), "sudo /usr/sbin/sysctl iogpu.wired_limit_mb=110000");
    }

    #[test]
    fn disable_restores_zero_and_removes_the_boot_setting_only_when_installed() {
        assert_eq!(disable_shell(false), "/usr/sbin/sysctl iogpu.wired_limit_mb=0");
        let both = disable_shell(true);
        assert!(both.starts_with("/usr/sbin/sysctl iogpu.wired_limit_mb=0 && /bin/launchctl bootout system/net.leanzero.iogpu-wired-limit"));
        assert!(both.ends_with("/bin/rm -f '/Library/LaunchDaemons/net.leanzero.iogpu-wired-limit.plist'"));
        assert!(admin_osascript_argv(&both, "p").is_ok());
    }

    #[test]
    fn the_daemon_sets_the_value_at_load_and_installs_through_one_prompt() {
        let plist = daemon_plist(110_000);
        assert!(plist.contains("<string>iogpu.wired_limit_mb=110000</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(plist.contains(DAEMON_LABEL));
        assert_eq!(daemon_value(&plist), Some(110_000));

        let shell = install_daemon_shell(110_000, Path::new("/tmp/goose-x.plist")).unwrap();
        assert!(shell.starts_with("/bin/cp '/tmp/goose-x.plist' '/Library/LaunchDaemons/net.leanzero.iogpu-wired-limit.plist'"));
        assert!(shell.contains("/usr/sbin/sysctl iogpu.wired_limit_mb=110000"));
        assert!(shell.contains("/bin/launchctl bootstrap system"));
        assert!(admin_osascript_argv(&shell, "p").is_ok());
        assert!(install_daemon_shell(1, Path::new("/tmp/a b'; rm -rf ~")).is_err());

        let remove = remove_daemon_shell();
        assert!(remove.contains("bootout system/net.leanzero.iogpu-wired-limit"));
        assert!(remove.contains("/bin/rm -f '/Library/LaunchDaemons/"));
        assert!(admin_osascript_argv(&remove, "p").is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_mac_answers_its_wired_limit() {
        read_wired_limit_mb().unwrap();
    }
}
