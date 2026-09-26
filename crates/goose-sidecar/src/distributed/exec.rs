//! Running a script on a node: `/bin/sh -c` on this Mac, `ssh <alias> <script>` on a peer (where
//! the peer's login shell — zsh on macOS — runs it, so every script here is sh/zsh-neutral and
//! calls tools by absolute path).

use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::node_op::NodeOp;
use super::provision::EnvSpec;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// ssh's own transport bounds. `ConnectTimeout` fails an unreachable peer instead of waiting on
/// TCP; `ServerAlive*` ends a session whose peer vanished (a rank's ssh session then exits, which
/// the supervisor reads as that rank's death). Transport, not model work.
pub(crate) const SSH_OPTIONS: [&str; 8] = [
    "-o",
    "BatchMode=yes",
    "-o",
    "ConnectTimeout=10",
    "-o",
    "ServerAliveInterval=5",
    "-o",
    "ServerAliveCountMax=3",
];

#[derive(Debug, Clone)]
pub struct ExecOutput {
    /// `None` when the process was killed by a signal.
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.status == Some(0)
    }

    /// ssh's own failure code: the script never ran (unreachable, auth refused).
    pub fn ssh_failed(&self) -> bool {
        self.status == Some(255)
    }

    /// What a `/bin/ps … -p <pid>` run PROVES about the pid: `Ok(Some(rows))` it runs,
    /// `Ok(None)` no process holds it, `Err` ps could not answer — and an unanswered ps is
    /// never read as either (a guard that signals on it fails OPEN). Measured on macOS 26.6:
    /// a pid with no process exits 1 with empty stdout AND empty stderr; a ps that failed
    /// (`ps: Invalid process id`, `process id too large`, a missing binary, ssh's 255) exits
    /// non-zero WITH stderr, or with a code other than 0/1.
    pub fn ps_answer(&self) -> Result<Option<&str>> {
        let rows = self.stdout.trim();
        match self.status {
            Some(0) if !rows.is_empty() => Ok(Some(rows)),
            Some(1) if rows.is_empty() && self.stderr.trim().is_empty() => Ok(None),
            status => anyhow::bail!(
                "ps could not answer (exit {status:?}, stdout {} bytes): {}",
                rows.len(),
                self.stderr.trim()
            ),
        }
    }
}

pub trait NodeExec: Send + Sync {
    /// Run `script` on `host` (`None` = this Mac) and collect its output.
    fn run<'a>(
        &'a self,
        host: Option<&'a str>,
        script: &'a str,
    ) -> BoxFuture<'a, Result<ExecOutput>>;

    /// Run one of goose's own node operations on `host`. Over ssh (and on this Mac) that is its
    /// script; a LeanZero Link peer receives the typed op and builds the script itself
    /// (`link_control::LinkRoutedExec`), so every call site that may reach a Link node uses this.
    fn run_op<'a>(
        &'a self,
        host: Option<&'a str>,
        op: &'a NodeOp,
    ) -> BoxFuture<'a, Result<ExecOutput>> {
        Box::pin(async move {
            let script = op.script()?;
            self.run(host, &script).await
        })
    }

    /// Build one of goose's managed envs on `host`, every output line to `on_line`; the script's
    /// exit code (`provision::provision_on`: a Link node builds it itself, ssh and this Mac run
    /// the script).
    fn provision<'a>(
        &'a self,
        host: Option<&'a str>,
        spec: &'a EnvSpec,
        on_line: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<Option<i32>>> {
        Box::pin(super::provision::provision_on(host, spec, on_line))
    }
}

/// The real transport.
pub struct SystemExec;

impl NodeExec for SystemExec {
    fn run<'a>(
        &'a self,
        host: Option<&'a str>,
        script: &'a str,
    ) -> BoxFuture<'a, Result<ExecOutput>> {
        Box::pin(async move {
            let mut cmd = match host {
                None => {
                    let mut cmd = Command::new("/bin/sh");
                    cmd.arg("-c")
                        .arg(script)
                        .env("PATH", crate::engine::sidecar_spawn_path());
                    cmd
                }
                Some(alias) => {
                    let mut cmd = Command::new("/usr/bin/ssh");
                    cmd.args(SSH_OPTIONS)
                        .args(["-o", "LogLevel=ERROR"])
                        .arg(alias)
                        .arg(script);
                    cmd
                }
            };
            let output = cmd
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output()
                .await
                .with_context(|| format!("running a script on {}", host.unwrap_or("this Mac")))?;
            Ok(ExecOutput {
                status: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        })
    }
}

/// Single-quote `value` for a POSIX shell (and zsh): `'` becomes `'\''`.
pub fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_spaces_and_single_quotes() {
        assert_eq!(sh_quote("EXO Thunderbolt 3"), "'EXO Thunderbolt 3'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[tokio::test]
    async fn a_quoted_value_reaches_the_script_verbatim() {
        let value = "a 'b' $HOME `c`";
        let out = SystemExec
            .run(None, &format!("printf '%s' {}", sh_quote(value)))
            .await
            .unwrap();
        assert!(out.success());
        assert_eq!(out.stdout, value);
    }

    /// The real `/bin/ps`, all three answers: a live pid, a pid no process holds, and a ps that
    /// could not answer — the last must never read as "no such process".
    #[tokio::test]
    async fn ps_proves_a_pid_runs_or_is_gone_and_a_failed_ps_proves_neither() {
        let ps = |pid: String| async move {
            SystemExec
                .run(None, &format!("/bin/ps -o command= -p {pid}"))
                .await
                .unwrap()
        };
        let live = ps(std::process::id().to_string()).await;
        assert!(live.ps_answer().unwrap().is_some(), "{live:?}");

        let mut exited = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let gone = exited.id();
        exited.wait().unwrap();
        let absent = ps(gone.to_string()).await;
        assert_eq!(absent.ps_answer().unwrap(), None, "{absent:?}");

        // macOS ps refuses a pid past its range by name ("process id too large"); Linux procps
        // answers the same pid as absent (exit 1, silent), which is a true proof of absence there.
        #[cfg(target_os = "macos")]
        {
            let refused = ps("999999999".into()).await;
            let err = refused.ps_answer().unwrap_err().to_string();
            assert!(err.contains("ps could not answer"), "{err}");
        }

        let missing = SystemExec
            .run(None, "/bin/ps-not-installed -o command= -p 1")
            .await
            .unwrap();
        assert!(missing.ps_answer().is_err(), "{missing:?}");
        let ssh_down = ExecOutput {
            status: Some(255),
            stdout: String::new(),
            stderr: "ssh: connect to host peer: Connection refused".into(),
        };
        assert!(ssh_down.ps_answer().is_err());
    }
}
