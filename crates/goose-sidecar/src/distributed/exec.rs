//! Running a script on a node: `/bin/sh -c` on this Mac, `ssh <alias> <script>` on a peer (where
//! the peer's login shell — zsh on macOS — runs it, so every script here is sh/zsh-neutral and
//! calls tools by absolute path).

use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::Command;

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
}

pub trait NodeExec: Send + Sync {
    /// Run `script` on `host` (`None` = this Mac) and collect its output.
    fn run<'a>(
        &'a self,
        host: Option<&'a str>,
        script: &'a str,
    ) -> BoxFuture<'a, Result<ExecOutput>>;
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
}
