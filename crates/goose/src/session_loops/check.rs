//! The loop's check command (design §4.6): a shell command run in the session's working dir after
//! a tick whose verdict is progress or done.
//!
//! - It ENDS AT ITS EXIT STATUS, never at pipe EOF. Its stdout and stderr go straight into the log
//!   file (`<data_dir>/loops/<loopId>/check-<n>.log`) — there is no pipe at all — so a grandchild
//!   the check leaves running (a dev server a test script started) cannot hold an EOF reader open
//!   (swarm invariant 5's shape: r0 parked 20 minutes on one). `child.wait()` is the end; the log
//!   holds everything the check wrote before it exited.
//! - It HAS NO TIMEOUT (gate 5). The only stop is the user's [Stop check].
//! - It leads its own process group (`configure_subprocess`), and Stop check kills it through the
//!   ONE sanctioned proof-gated group kill, `goose_sidecar::sigkill_owned_group` (gate 4); when the
//!   proof fails the pid alone is signalled, and that is logged.
//! - A check that cannot run (the working dir is gone, no shell, the log cannot be opened) is
//!   `CouldNotRun` with its error — never a failed check (the D4 shape refused).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::subprocess::configure_subprocess;

pub struct CheckSpec {
    pub command: String,
    pub working_dir: PathBuf,
    pub log_path: PathBuf,
    /// The PATH the check runs with; `None` = the one goose inherited.
    pub path_env: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckEnd {
    /// The check exited; `code` is absent when a signal ended it.
    Exited {
        code: Option<i32>,
    },
    CouldNotRun {
        error: String,
    },
    /// The user stopped it.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub end: CheckEnd,
    /// Everything the check wrote, as text; `Err` names why the log could not be read back.
    pub output: Result<String, String>,
}

pub const STOPPED_BY_YOU: &str = "stopped by you";

fn shell(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.arg("/C").raw_arg(command);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    }
}

fn open_log(path: &Path) -> Result<std::fs::File, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            format!(
                "the check's log folder {} could not be made: {e}",
                dir.display()
            )
        })?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| {
            format!(
                "the check's log {} could not be opened: {e}",
                path.display()
            )
        })
}

/// The SIGKILL of Stop check: the check's own process group when the proof holds, else its pid.
fn stop(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        if goose_sidecar::sigkill_owned_group(pid) {
            return;
        }
        tracing::warn!(
            pid,
            "stop check: the check is not the live leader of its own process group; signalling the pid alone"
        );
    }
    if let Err(error) = child.start_kill() {
        tracing::warn!(%error, "stop check: the check could not be signalled");
    }
}

/// Run the check to its exit, or until `stop` is cancelled.
pub async fn run(spec: &CheckSpec, stop_check: CancellationToken) -> CheckOutcome {
    let could_not_run = |error: String| CheckOutcome {
        end: CheckEnd::CouldNotRun { error },
        output: Ok(String::new()),
    };
    let log = match open_log(&spec.log_path) {
        Ok(log) => log,
        Err(error) => return could_not_run(error),
    };
    let err_log = match log.try_clone() {
        Ok(log) => log,
        Err(e) => return could_not_run(format!("the check's log could not be shared: {e}")),
    };
    let mut command = shell(&spec.command);
    command
        .current_dir(&spec.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err_log));
    if let Some(path) = &spec.path_env {
        command.env("PATH", path);
    }
    configure_subprocess(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return could_not_run(format!(
                "`{}` could not start in {}: {e}",
                spec.command,
                spec.working_dir.display()
            ))
        }
    };
    let end = tokio::select! {
        status = child.wait() => match status {
            Ok(status) => CheckEnd::Exited { code: status.code() },
            Err(e) => CheckEnd::CouldNotRun { error: format!("goose lost the check's exit status: {e}") },
        },
        _ = stop_check.cancelled() => {
            stop(&mut child);
            if let Err(e) = child.wait().await {
                tracing::warn!(error = %e, "stop check: the stopped check's exit status was lost");
            }
            CheckEnd::Stopped
        }
    };
    let output = tokio::fs::read(&spec.log_path)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| {
            format!(
                "goose could not read the check's output back from {}: {e}",
                spec.log_path.display()
            )
        });
    CheckOutcome { end, output }
}
