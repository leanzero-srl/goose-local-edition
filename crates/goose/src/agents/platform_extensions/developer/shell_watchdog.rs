//! Q-407: a goose serve that dies without its teardown still ends its shell commands.
//!
//! WHY: Q-406 gave every shell command a process group of its own, so the desktop's signal to
//! goosed's group (gooseServe.ts `killGroupOrProcess`) no longer reaches them. goosed stops them
//! itself on SIGTERM, SIGHUP or stdin EOF (the teardown step "shell commands"), but a goosed that
//! dies WITHOUT that teardown — a crash, or the desktop's SIGKILL fallback — left them running: a
//! `find` blocked by macOS privacy protection outlived goosed exactly as it did before Q-406.
//!
//! THE MECHANISM: goose serve spawns `goose shell-watchdog` as it starts, in a process group of its
//! own (so the desktop's group kill of goosed never reaches it), with its stdin the read end of a
//! pipe only goosed holds the write end of. goosed tells it, one line each, every group its teardown
//! would stop — an in-flight command's leader stamped with the leader's start time, a command that
//! ended leaving processes behind (a server started with `&`, Q-406) stamped member by member — and
//! when a group stops being goosed's to stop. The write end is close-on-exec, so no child inherits
//! it: however goosed ends, the kernel closes it and the watchdog reads EOF. No heartbeat and no
//! clock decides that goosed is gone; EOF is the kernel saying so.
//!
//! On EOF the watchdog ends what is still recorded, each group only under proof, through the same
//! code as the teardown (`process_groups::terminate_command_groups`): an in-flight group only while
//! its leader is still the process that was stamped (a pid — and so a group id — the kernel handed
//! to another process fails the stamp), then the sidecar's group proof on the leader; an ended group
//! only while a stamped member is still that process and still in the group, then per pid.
//!
//! Chosen over, measured 2026-09-28:
//! - a registry FILE the desktop reads after goosed dies: it covers only a goosed the desktop
//!   started, and Node has no getpgid, so the ownership proof would get a second home in TypeScript
//!   over `ps`;
//! - a watcher INSIDE each command's group reading the pipe: a model command with a bare `wait`
//!   waits on the watcher (measured: a 0.2 s job still waiting after 3 s), and every group would
//!   outlive its command;
//! - the orphan-goosed reap (Q-223) and clean.sh's orphan classes: they prove a process by its
//!   command line, and a shell command has no shape to prove.

#[cfg(unix)]
use super::process_groups::{self, Lingering};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::{Mutex, OnceLock};

/// The name of the hidden `goose` subcommand goose serve spawns as its watchdog.
pub const SUBCOMMAND: &str = "shell-watchdog";

#[cfg_attr(not(unix), allow(dead_code))]
struct Watchdog {
    stdin: std::process::ChildStdin,
    child: std::process::Child,
}

fn watchdog() -> &'static Mutex<Option<Watchdog>> {
    static WATCHDOG: OnceLock<Mutex<Option<Watchdog>>> = OnceLock::new();
    WATCHDOG.get_or_init(|| Mutex::new(None))
}

/// Start the watchdog from `command` (its program and arguments; stdio and process group are set
/// here) and tell it about every group already recorded. Returns its pid. Call it before anything
/// long-lived is spawned: the write end is marked close-on-exec just after the pipe is made, and a
/// process forked in between would hold it open.
#[cfg(unix)]
pub fn arm(mut command: std::process::Command) -> std::io::Result<u32> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .process_group(0)
        .spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("the watchdog was spawned without a stdin pipe"))?;
    let pid = child.id();
    *watchdog().lock().unwrap_or_else(|e| e.into_inner()) = Some(Watchdog { stdin, child });
    for leader in process_groups::live_leaders() {
        note_live(leader);
    }
    process_groups::with_lingering(|ended| ended.iter().for_each(note_ended));
    Ok(pid)
}

/// One line to the watchdog. A write that fails means the watchdog is gone (it was killed): that
/// is said once, loudly, with its exit status, and nothing is sent again — the commands still end
/// by the teardown on every exit that runs it.
fn send(line: &str) {
    let mut slot = watchdog().lock().unwrap_or_else(|e| e.into_inner());
    let Some(watchdog) = slot.as_mut() else {
        return;
    };
    if let Err(error) = writeln!(watchdog.stdin, "{line}") {
        let status = watchdog.child.try_wait();
        tracing::error!(
            event = "shell_watchdog_gone",
            %error,
            ?status,
            "the shell-command watchdog stopped reading; a crash of this goose would now leave its running shell commands behind"
        );
        *slot = None;
    }
}

fn armed() -> bool {
    watchdog()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

/// An own-group command started; its leader is stamped now, while this process still holds it
/// unreaped (its pid cannot belong to anyone else yet). A leader with no readable start time is
/// not recorded — nothing could later prove it was the same process.
pub(super) fn note_live(leader: i32) {
    #[cfg(unix)]
    if armed() {
        if let Some(stamp) = process_groups::start_time(leader) {
            send(&format!("live {leader} {stamp}"));
        }
    }
    #[cfg(not(unix))]
    let _ = leader;
}

#[cfg(unix)]
pub(super) fn note_ended(ended: &Lingering) {
    if !armed() {
        return;
    }
    let stamps: Vec<String> = ended
        .stamps()
        .iter()
        .map(|(pid, stamp)| format!("{pid}={stamp}"))
        .collect();
    send(&format!("ended {} {}", ended.leader(), stamps.join(" ")));
}

pub(super) fn note_gone(leader: i32) {
    if armed() {
        send(&format!("gone {leader}"));
    }
}

#[derive(Debug, PartialEq)]
#[cfg_attr(not(unix), allow(dead_code))]
enum Record {
    Live(String),
    Ended(Vec<(i32, String)>),
}

/// What goosed recorded, as of the end of its pipe.
#[derive(Debug, Default)]
pub struct Records {
    groups: BTreeMap<i32, Record>,
    unreadable: Vec<String>,
    /// Why the pipe stopped before EOF. Only EOF says goosed is gone; a read error says nothing
    /// about goosed, so nothing is ended on one.
    read_error: Option<String>,
}

fn parse_stamp(field: &str) -> Option<(i32, String)> {
    let (pid, stamp) = field.split_once('=')?;
    Some((pid.parse().ok()?, stamp.to_string()))
}

/// Read goosed's lines until EOF — the end of goosed. A line that does not parse is kept and
/// reported, never skipped silently.
pub fn read_records(mut reader: impl BufRead) -> Records {
    let mut records = Records::default();
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                records.read_error = Some(error.to_string());
                break;
            }
        }
        let line = String::from_utf8_lossy(&bytes).trim_end().to_string();
        let fields: Vec<&str> = line.split_whitespace().collect();
        let leader = fields.get(1).and_then(|l| l.parse::<i32>().ok());
        match (fields.first().copied(), leader) {
            (Some("live"), Some(leader)) if fields.len() == 3 => {
                records
                    .groups
                    .insert(leader, Record::Live(fields[2].to_string()));
            }
            (Some("ended"), Some(leader)) if fields.len() > 2 => {
                let stamps: Option<Vec<_>> = fields[2..].iter().map(|f| parse_stamp(f)).collect();
                match stamps {
                    Some(stamps) => {
                        records.groups.insert(leader, Record::Ended(stamps));
                    }
                    None => records.unreadable.push(line),
                }
            }
            (Some("gone"), Some(leader)) if fields.len() == 2 => {
                records.groups.remove(&leader);
            }
            _ => records.unreadable.push(line),
        }
    }
    records
}

/// End every recorded group that is provably still the one recorded, and say what happened.
#[cfg(unix)]
pub async fn end_recorded(records: Records) -> String {
    if let Some(error) = records.read_error {
        return format!(
            "the pipe from goose failed before its end ({error}): goose may still be running, so none of its {} recorded shell command group(s) was touched",
            records.groups.len()
        );
    }
    let mut live = Vec::new();
    let mut not_the_same = Vec::new();
    let mut ended = Vec::new();
    for (leader, record) in records.groups {
        match record {
            Record::Live(stamp) => {
                if process_groups::start_time(leader).as_deref() == Some(stamp.as_str()) {
                    live.push(leader);
                } else {
                    not_the_same.push(leader);
                }
            }
            Record::Ended(stamps) => ended.push(Lingering::new(leader, stamps)),
        }
    }
    let outcome = process_groups::terminate_command_groups(live, ended).await;
    let mut report = format!(
        "goose is gone: {outcome}; left alone, their leader no longer the process recorded: {not_the_same:?}"
    );
    if !records.unreadable.is_empty() {
        report.push_str(&format!("; unreadable lines: {:?}", records.unreadable));
    }
    report
}

/// `goose shell-watchdog`: read goosed's records from stdin until goosed is gone, then end what is
/// still recorded. Returns the one-line outcome.
pub async fn run_on_stdin() -> String {
    let records = tokio::task::spawn_blocking(|| read_records(std::io::stdin().lock())).await;
    match records {
        #[cfg(unix)]
        Ok(records) => end_recorded(records).await,
        #[cfg(not(unix))]
        Ok(records) => format!(
            "no process groups on this platform: {} recorded group(s) left as they are (pipe error: {:?})",
            records.groups.len(),
            records.read_error
        ),
        Err(error) => format!("the watchdog's reader failed ({error}); nothing was ended"),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn own_group(args: &[&str]) -> std::process::Child {
        std::process::Command::new(args[0])
            .args(&args[1..])
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn")
    }

    fn members(group: i32) -> Vec<i32> {
        let out = std::process::Command::new("pgrep")
            .args(["-g", &group.to_string()])
            .output()
            .expect("pgrep");
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect()
    }

    fn wait_for<F: FnMut() -> bool>(mut cond: F) -> bool {
        for _ in 0..150 {
            if cond() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    fn alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// What a test started, killed per pid when the test ends — pass or fail — and each stray
    /// only while it is still the process that was started.
    #[derive(Default)]
    struct Started {
        children: Vec<std::process::Child>,
        strays: Vec<(i32, String)>,
    }

    impl Drop for Started {
        fn drop(&mut self) {
            for (pid, stamp) in &self.strays {
                if process_groups::start_time(*pid).as_deref() == Some(stamp.as_str()) {
                    unsafe { libc::kill(*pid, libc::SIGKILL) };
                }
            }
            for child in &mut self.children {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// A command whose leader exits at once, leaving one backgrounded member in its group — the
    /// `&` server shape. Returns (leader, the member).
    fn ended_with_a_member(marker: &str) -> (i32, i32) {
        let mut leader = own_group(&["sh", "-c", &format!("( {marker} & )")]);
        let group = leader.id() as i32;
        let _ = leader.wait();
        assert!(
            wait_for(|| members(group).len() == 1),
            "{:?}",
            members(group)
        );
        (group, members(group)[0])
    }

    /// The recovery path on its own: goosed's records up to EOF, then the end. The genuine groups
    /// go; a pid now held by ANOTHER process (the shape a reused group id takes: the recorded
    /// leader or member died and the kernel handed its pid to someone else, so the stamp is the
    /// old process's) is left alone, as is a group goosed said was no longer its to stop.
    #[tokio::test]
    async fn the_recovery_ends_recorded_groups_and_nothing_that_reused_their_ids() {
        let stamp = |pid| process_groups::start_time(pid).expect("a stamp");
        let mut started = Started::default();
        let mut in_flight = own_group(&["sh", "-c", "sleep 407001 | cat"]);
        let in_flight_group = in_flight.id() as i32;
        assert!(wait_for(|| members(in_flight_group).len() >= 3));
        for pid in members(in_flight_group) {
            started.strays.push((pid, stamp(pid)));
        }
        let (ended_group, ended_member) = ended_with_a_member("sleep 407002");
        started.strays.push((ended_member, stamp(ended_member)));

        let reused_leader = own_group(&["sleep", "407003"]);
        let reused_leader_pid = reused_leader.id() as i32;
        started.children.push(reused_leader);
        let (reused_ended_group, reused_member) = ended_with_a_member("sleep 407004");
        started.strays.push((reused_member, stamp(reused_member)));
        let released = own_group(&["sleep", "407005"]);
        let released_pid = released.id() as i32;
        started.children.push(released);

        let earlier = "1.000000";
        let lines = format!(
            "live {in_flight_group} {}\n\
             ended {ended_group} {ended_member}={}\n\
             live {reused_leader_pid} {earlier}\n\
             ended {reused_ended_group} {reused_member}={earlier}\n\
             live {released_pid} {}\n\
             gone {released_pid}\n",
            stamp(in_flight_group),
            stamp(ended_member),
            stamp(released_pid),
        );
        let outcome = end_recorded(read_records(std::io::Cursor::new(lines))).await;

        // The leader is this test's child: reaped here, or its zombie stays in the group.
        assert!(
            wait_for(|| matches!(in_flight.try_wait(), Ok(Some(_)))),
            "the in-flight leader survived: {outcome}"
        );
        assert!(
            wait_for(|| members(in_flight_group).is_empty()),
            "the in-flight group survived: {outcome}"
        );
        assert!(
            wait_for(|| !alive(ended_member)),
            "the ended group's member survived: {outcome}"
        );
        assert!(
            alive(reused_leader_pid),
            "a process holding a recorded pid, started later, was signalled: {outcome}"
        );
        assert!(alive(reused_member), "{outcome}");
        assert!(alive(released_pid), "{outcome}");
        assert!(
            outcome.contains(&format!("[{reused_leader_pid}]")),
            "{outcome}"
        );
    }

    /// Only EOF says goosed is gone. A pipe that fails some other way says nothing about goosed,
    /// so a recorded group — here provably the recorded one — is left running.
    #[tokio::test]
    async fn a_pipe_error_before_eof_ends_nothing() {
        struct FailsAfterOneLine(std::io::Cursor<Vec<u8>>);
        impl std::io::Read for FailsAfterOneLine {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                unreachable!("read through BufRead")
            }
        }
        impl BufRead for FailsAfterOneLine {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                if self.0.position() as usize == self.0.get_ref().len() {
                    return Err(std::io::Error::from_raw_os_error(libc::EIO));
                }
                self.0.fill_buf()
            }
            fn consume(&mut self, n: usize) {
                self.0.consume(n)
            }
        }
        let mut started = Started::default();
        started.children.push(own_group(&["sleep", "407006"]));
        let leader = started.children[0].id() as i32;
        let line = format!(
            "live {leader} {}\n",
            process_groups::start_time(leader).unwrap()
        );
        let records = read_records(FailsAfterOneLine(std::io::Cursor::new(line.into_bytes())));
        let outcome = end_recorded(records).await;
        assert!(outcome.contains("may still be running"), "{outcome}");
        assert!(alive(leader), "{outcome}");
    }

    #[test]
    fn a_line_that_does_not_parse_is_reported_not_skipped() {
        let records = read_records(std::io::Cursor::new(
            "live 12 3.4\nlive twelve 3.4\nended 13 14\nsomething else\n",
        ));
        assert_eq!(records.groups.len(), 1);
        assert_eq!(
            records.unreadable,
            vec!["live twelve 3.4", "ended 13 14", "something else"]
        );
    }
}
