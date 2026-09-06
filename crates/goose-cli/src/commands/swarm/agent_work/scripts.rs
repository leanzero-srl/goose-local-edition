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

#[cfg(test)]
mod tests {
    use super::*;

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
