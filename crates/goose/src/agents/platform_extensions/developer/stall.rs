//! Why a timed-out shell command was still running — derived from its processes at the moment of
//! the timeout, never guessed and never looked up in a list of folders.
//!
//! Q-406 (2026-09-28): `find ~ -maxdepth 3 … | head` entered ~/Library/Mobile Documents, sandboxd
//! raised a privacy (TCC) request for the app that never got an answer and showed no dialog, and
//! find sat in `open()` until the 300 s timeout. The model saw "(no output) Command timed out" and
//! nothing it could act on. The evidence that names the cause exists at timeout time: the stuck
//! process's stack (`sample` — its leaf frame is the open call), its working directory (find
//! walks by chdir, so the entry it is opening lives there), and tccd's own log line attributing a
//! request to that pid. Each is read here; a fact that cannot be read is said, not filled in.

#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::time::Instant;

/// One sentence for the model: the stuck process, what it was doing and what to do about it, or a
/// plain statement that the cause could not be determined and why.
pub(super) async fn diagnose(members: &std::io::Result<Vec<i32>>, started: Instant) -> String {
    #[cfg(target_os = "macos")]
    {
        diagnose_macos(members, started).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (members, started);
        "Why it had not finished could not be determined: reading a process's current system \
         call needs macOS's `sample`, which this platform does not have."
            .to_string()
    }
}

#[cfg(target_os = "macos")]
async fn diagnose_macos(members: &std::io::Result<Vec<i32>>, started: Instant) -> String {
    const UNDETERMINED: &str = "Why it had not finished could not be determined";
    let members = match members {
        Ok(m) if m.is_empty() => {
            return format!("{UNDETERMINED}: none of its processes was left to inspect.")
        }
        Ok(m) => m,
        Err(e) => return format!("{UNDETERMINED}: listing its processes failed ({e})."),
    };
    let rows = match process_rows(members).await {
        Ok(rows) => rows,
        Err(e) => return format!("{UNDETERMINED}: reading its process table failed ({e})."),
    };
    // A shell waiting on its pipeline is never the stuck party; the leaves are where work happens.
    let leaves: Vec<&ProcessRow> = rows
        .iter()
        .filter(|r| !r.stat.starts_with('Z') && !rows.iter().any(|o| o.ppid == r.pid))
        .collect();
    let samples = futures::future::join_all(leaves.iter().map(|r| leaf_frame_of(r.pid))).await;

    let mut blocked = Vec::new();
    let mut elsewhere = Vec::new();
    let mut unsampled = Vec::new();
    for (row, sample) in leaves.iter().zip(samples) {
        match sample {
            Ok(frame) if is_open_call(&frame) => blocked.push(*row),
            Ok(frame) => elsewhere.push(format!("{} in {frame}", row.name)),
            Err(e) => unsampled.push((*row, e)),
        }
    }

    // A pending privacy request naming a pid is direct evidence on its own, so the log is read
    // for a leaf that could not be sampled as well as for one sampled in open().
    let privacy_log = if blocked.is_empty() && unsampled.is_empty() {
        None
    } else {
        Some(tcc_requests_since(started).await)
    };
    let named_by_privacy = |pid: i32| match &privacy_log {
        Some(Ok(log)) => tcc_request_naming(log, pid),
        _ => None,
    };

    let mut causes: Vec<String> = blocked
        .iter()
        .map(|row| {
            let who = format!("`{}` (pid {})", row.name, row.pid);
            let dir = dir_of(row.pid);
            if let Some(app) = named_by_privacy(row.pid) {
                return privacy_block(&who, &dir, &app);
            }
            match &privacy_log {
                Some(Ok(log)) if log.contains("AUTHREQ_ATTRIBUTION") => format!(
                    "{who} was blocked in open() on an entry of {dir}, and no macOS privacy \
                     request names it — whatever it was opening there never answered; narrow \
                     the command or skip that path."
                ),
                Some(Err(e)) => format!(
                    "{who} was blocked in open() on an entry of {dir}; whether macOS privacy \
                     protection held it could not be checked ({e})."
                ),
                _ => format!(
                    "{who} was blocked in open() on an entry of {dir}; whether macOS privacy \
                     protection held it could not be confirmed — the privacy log shows no \
                     request at all during the command."
                ),
            }
        })
        .collect();
    let mut unreadable = Vec::new();
    for (row, error) in &unsampled {
        match named_by_privacy(row.pid) {
            Some(app) => causes.push(privacy_block(
                &format!("`{}` (pid {})", row.name, row.pid),
                &dir_of(row.pid),
                &app,
            )),
            None => unreadable.push(format!("{}: {error}", row.name)),
        }
    }
    if !causes.is_empty() {
        return causes.join(" ");
    }

    let mut why = Vec::new();
    if leaves.is_empty() {
        why.push("every process in it had already exited".to_string());
    }
    if !elsewhere.is_empty() {
        why.push(format!(
            "no process in it was blocked opening a file ({})",
            elsewhere.join(", ")
        ));
    }
    if !unreadable.is_empty() {
        let privacy = match &privacy_log {
            Some(Err(e)) => format!("; the privacy log could not be read ({e})"),
            _ => String::new(),
        };
        why.push(format!(
            "sampling failed ({}){privacy}",
            unreadable.join("; ")
        ));
    }
    format!("{UNDETERMINED}: {}.", why.join("; "))
}

#[cfg(target_os = "macos")]
fn privacy_block(who: &str, dir: &str, app: &str) -> String {
    format!(
        "{who} is blocked by macOS privacy protection while opening an entry of {dir} — {app} \
         has no access there; narrow the search or prune that folder."
    )
}

#[cfg(target_os = "macos")]
fn dir_of(pid: i32) -> String {
    match cwd_of(pid) {
        Ok(dir) => dir.display().to_string(),
        Err(e) => format!("its working directory (unreadable: {e})"),
    }
}

#[cfg(target_os = "macos")]
struct ProcessRow {
    pid: i32,
    ppid: i32,
    stat: String,
    name: String,
}

#[cfg(target_os = "macos")]
async fn process_rows(pids: &[i32]) -> Result<Vec<ProcessRow>, String> {
    let list = pids
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let out = tokio::process::Command::new("ps")
        .args(["-o", "pid=,ppid=,stat=,comm=", "-p", &list])
        .output()
        .await
        .map_err(|e| format!("ps: {e}"))?;
    // ps exits 1 when some listed pid is already gone; the rows it printed are still true.
    let rows: Vec<ProcessRow> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let stat = parts.next()?.to_string();
            let comm = parts.collect::<Vec<_>>().join(" ");
            let name = std::path::Path::new(&comm)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(comm);
            Some(ProcessRow {
                pid,
                ppid,
                stat,
                name,
            })
        })
        .collect();
    if rows.is_empty() {
        return Err(format!(
            "ps listed none of {list}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(rows)
}

/// The frame a process sits in, from `sample`'s "Sort by top of stack" section. One second is
/// `sample`'s smallest duration; `-file /dev/stdout` keeps it from leaving a report in /tmp.
#[cfg(target_os = "macos")]
async fn leaf_frame_of(pid: i32) -> Result<String, String> {
    let out = tokio::process::Command::new("sample")
        .args([&pid.to_string(), "1", "-mayDie", "-file", "/dev/stdout"])
        .output()
        .await
        .map_err(|e| format!("sample: {e}"))?;
    let report = String::from_utf8_lossy(&out.stdout);
    leaf_frame(&report).ok_or_else(|| {
        let err = String::from_utf8_lossy(&out.stderr);
        let reason = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no stack in its report");
        format!("sample: {}", reason.trim())
    })
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn leaf_frame(report: &str) -> Option<String> {
    let mut lines = report.lines();
    lines.find(|l| l.starts_with("Sort by top of stack"))?;
    let first = lines.find(|l| !l.trim().is_empty())?;
    let symbol = first.trim().split("  (in ").next()?.trim();
    (!symbol.is_empty()).then(|| symbol.to_string())
}

#[cfg(any(target_os = "macos", test))]
/// `__open`, `__open_nocancel`, `open$NOCANCEL`, `__openat` … — the kernel entry of open(2)/openat(2).
pub(super) fn is_open_call(frame: &str) -> bool {
    let name = frame.trim_start_matches('_');
    let name = name.split('$').next().unwrap_or(name);
    let name = name.strip_suffix("_nocancel").unwrap_or(name);
    matches!(name, "open" | "openat")
}

#[cfg(target_os = "macos")]
fn cwd_of(pid: i32) -> std::io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if read != size {
        return Err(std::io::Error::last_os_error());
    }
    let path = &info.pvi_cdir.vip_path;
    let bytes = unsafe {
        std::slice::from_raw_parts(path.as_ptr() as *const u8, std::mem::size_of_val(path))
    };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end])))
}

/// tccd's authorization-request lines over the command's whole lifetime: `--last` counts back from
/// the query's own start, so the window is the command's age measured NOW, rounded up (a request
/// the command raised cannot be older than the command).
#[cfg(target_os = "macos")]
async fn tcc_requests_since(started: Instant) -> Result<String, String> {
    let window = format!("{}s", started.elapsed().as_secs() + 1);
    let out = tokio::process::Command::new("log")
        .args([
            "show",
            "--last",
            &window,
            "--style",
            "compact",
            "--predicate",
            "process == \"tccd\" AND composedMessage CONTAINS \"AUTHREQ_\"",
        ])
        .output()
        .await
        .map_err(|e| format!("log show: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "log show: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(any(target_os = "macos", test))]
/// The app a privacy request naming `pid` as the ACCESSING process was attributed to: tccd logs
/// `AUTHREQ_ATTRIBUTION: msgID=…, attribution={responsible={… responsible_path=<app binary>, …},
/// accessing={TCCDProcess: identifier=…, pid=<pid>, …}`. The app name is the `.app` bundle in the
/// responsible path, else its identifier.
///
/// Answered or not does not matter, and the incident is why: the one request naming find (msgID
/// 45720.597, SystemPolicyAllFiles, preflight) was answered — denied — 31 ms later, and what then
/// held find in open() for the rest of the turn was sandboxd's follow-up
/// `TCCAccessRequestIndirectWithOptions` for the File Provider domain, which tccd never attributes
/// to a pid. A request naming the pid, with the pid still in open(), is the privacy machinery at work.
pub(super) fn tcc_request_naming(log: &str, pid: i32) -> Option<String> {
    let needle = format!("pid={pid},");
    log.lines()
        .filter(|l| l.contains("AUTHREQ_ATTRIBUTION"))
        .filter_map(|line| {
            let accessing = line.split("accessing={TCCDProcess:").nth(1)?;
            let accessing = accessing.split('}').next()?;
            if !accessing.contains(&needle) {
                return None;
            }
            let responsible = line.split("responsible={TCCDProcess:").nth(1)?;
            let app = field(responsible, "responsible_path=")
                .and_then(|path| {
                    path.split('/')
                        .find_map(|c| c.strip_suffix(".app").map(str::to_string))
                })
                .or_else(|| field(responsible, "identifier="))?;
            Some(app)
        })
        .next()
}

#[cfg(any(target_os = "macos", test))]
fn field(text: &str, key: &str) -> Option<String> {
    let rest = text.split(key).nth(1)?;
    let value = rest.split([',', '}']).next()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verbatim from the Q-406 incident (unified log, 2026-09-28 17:13:25, /tmp/q394b).
    const INCIDENT: &str = "2026-09-28 17:13:25.363 Df tccd[13424:574c0a3] [com.apple.TCC:access] AUTHREQ_ATTRIBUTION: msgID=45720.597, attribution={responsible={TCCDProcess: identifier=net.leanzero.goose-swarm, pid=93148, auid=501, euid=501, responsible_path=/Applications/Goose Swarm.app/Contents/MacOS/Goose Swarm, binary_path=/Applications/Goose Swarm.app/Contents/MacOS/Goose Swarm}, accessing={TCCDProcess: identifier=com.apple.find, pid=75277, auid=501, euid=501, binary_path=/usr/bin/find}, requesting={TCCDProcess: identifier=com.apple.sandboxd, pid=45720, auid=0, euid=0, binary_path=/usr/libexec/sandboxd}, },";

    #[test]
    fn a_request_naming_the_pid_names_the_app_from_its_bundle() {
        assert_eq!(
            tcc_request_naming(INCIDENT, 75277).as_deref(),
            Some("Goose Swarm")
        );
        // The responsible app's own pid and the requesting daemon's are not the accessing process.
        assert_eq!(tcc_request_naming(INCIDENT, 93148), None);
        assert_eq!(tcc_request_naming(INCIDENT, 45720), None);
        assert_eq!(tcc_request_naming(INCIDENT, 7527), None);
    }

    /// The incident's own sequence: the request naming find was answered (denied) at .394, and
    /// find stayed in open() behind sandboxd's indirect File Provider request, which names no pid.
    /// Dropping answered requests would have told the model "no privacy request names it".
    #[test]
    fn an_answered_request_still_names_the_privacy_block() {
        let log = format!(
            "{INCIDENT}\n2026-09-28 17:13:25.394 Df tccd[13424:574c0a3] [com.apple.TCC:access] AUTHREQ_RESULT: msgID=45720.597, authValue=0, authReason=5, authVersion=1, desired_auth=0, error=(null),"
        );
        assert_eq!(
            tcc_request_naming(&log, 75277).as_deref(),
            Some("Goose Swarm")
        );
    }

    #[test]
    fn the_leaf_frame_is_read_from_the_top_of_stack_section() {
        // Shape of `sample`'s report for the incident's find (/tmp/q394b/probe/sample_find.txt).
        let report = "Call graph:\n    895 Thread_96026561\n      895 open$NOCANCEL  (in libsystem_kernel.dylib) + 64\n\nTotal number in stack (recursive counted multiple, when >=5):\n\nSort by top of stack, same collapsed (when >= 5):\n        __open_nocancel  (in libsystem_kernel.dylib)        895\n\nBinary Images:\n";
        assert_eq!(leaf_frame(report).as_deref(), Some("__open_nocancel"));
        assert_eq!(leaf_frame("no report"), None);
    }

    #[test]
    fn only_the_open_family_counts_as_blocked_opening() {
        for frame in [
            "__open",
            "__open_nocancel",
            "open$NOCANCEL",
            "__openat_nocancel",
        ] {
            assert!(is_open_call(frame), "{frame}");
        }
        for frame in [
            "__read_nocancel",
            "__semwait_signal",
            "__wait4",
            "__opendir",
        ] {
            assert!(!is_open_call(frame), "{frame}");
        }
    }
}
