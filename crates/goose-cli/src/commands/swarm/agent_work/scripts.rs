//! Running the desk's own scripts — guards, polls, the one post command, the close-out. A
//! script runs in the agent directory with the env file sourced, its whole stdout/stderr
//! captured, its exit code trusted over any prose. No timeout: a cap on a script is a cap on the
//! work (gate 5); a hung script is visible in the desk's phase clock and the operator ends it.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRun {
    pub command: String,
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub secs: f64,
}

/// `KEY=VALUE` lines (an optional `export ` prefix, quotes stripped, `#` comments skipped) — the
/// same shape the desks source with `set -a; source …; set +a`.
pub fn load_env_file(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(parse_env(&text))
}

pub fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let v = v.trim();
        let unquoted = ['"', '\'']
            .iter()
            .find_map(|q| v.strip_prefix(*q).and_then(|s| s.strip_suffix(*q)))
            .unwrap_or(v);
        out.push((k.to_string(), unquoted.to_string()));
    }
    out
}

pub async fn run_script(dir: &Path, command: &str, env: &[(String, String)]) -> ScriptRun {
    let started = std::time::Instant::now();
    let mut cmd = tokio::process::Command::new("/bin/bash");
    cmd.arg("-lc").arg(command).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.kill_on_drop(true);
    match cmd.output().await {
        Ok(out) => ScriptRun {
            command: command.to_string(),
            exit: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            secs: started.elapsed().as_secs_f64(),
        },
        Err(e) => ScriptRun {
            command: command.to_string(),
            exit: None,
            stdout: String::new(),
            stderr: format!("could not start: {e}"),
            secs: started.elapsed().as_secs_f64(),
        },
    }
}

pub async fn run_guards(
    dir: &Path,
    commands: &[String],
    env: &[(String, String)],
    mut record: impl FnMut(&ScriptRun, Option<&str>),
) -> Option<String> {
    for command in commands {
        let run = run_script(dir, command, env).await;
        let hold = match run.exit {
            Some(0) => None,
            Some(3) => {
                let why = run.stdout.lines().next().unwrap_or("").trim();
                Some(if why.is_empty() {
                    format!("guard `{command}` said hold (exit 3)")
                } else {
                    why.to_string()
                })
            }
            other => Some(format!(
                "guard `{command}` failed (exit {}) — {}",
                other
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "none".into()),
                super::store::tail_chars(&format!("{}{}", run.stdout, run.stderr), 300).trim()
            )),
        };
        record(&run, hold.as_deref());
        if hold.is_some() {
            return hold;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn guards_stop_on_hold_error_or_spawn_failure() {
        let dir = tempfile::tempdir().unwrap();
        for (command, reason) in [
            ("echo waiting-for-approval; exit 3", "waiting-for-approval"),
            ("echo permission-denied >&2; exit 1", "failed (exit 1)"),
            ("exit 2", "failed (exit 2)"),
        ] {
            let mut records = Vec::new();
            let hold = run_guards(
                dir.path(),
                &[command.into(), "touch should-not-run".into()],
                &[],
                |run, _| records.push(run.clone()),
            )
            .await
            .unwrap();
            assert!(hold.contains(reason), "{hold}");
            assert_eq!(records.len(), 1);
            assert!(!dir.path().join("should-not-run").exists());
        }
        let hold = run_guards(
            &dir.path().join("missing"),
            &["true".into()],
            &[],
            |_, _| {},
        )
        .await
        .unwrap();
        assert!(hold.contains("failed (exit none)"));
        assert!(hold.contains("could not start"));
        let hold = run_guards(
            dir.path(),
            &["true".into(), "touch success".into()],
            &[],
            |_, _| {},
        )
        .await;
        assert!(hold.is_none());
        assert!(dir.path().join("success").exists());
    }

    #[test]
    fn env_lines_parse_like_source() {
        let v = parse_env("# c\nexport A=1\nB=\"two words\"\nC='x'\nbad line\n_D=\n");
        assert_eq!(
            v,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two words".into()),
                ("C".into(), "x".into()),
                ("_D".into(), "".into())
            ]
        );
    }

    #[tokio::test]
    async fn a_script_reports_exit_and_streams() {
        let d = tempfile::tempdir().unwrap();
        let r = run_script(
            d.path(),
            "echo out; echo err >&2; exit 3",
            &[("X".into(), "1".into())],
        )
        .await;
        assert_eq!(r.exit, Some(3));
        assert_eq!(r.stdout.trim(), "out");
        assert_eq!(r.stderr.trim(), "err");
    }
}
