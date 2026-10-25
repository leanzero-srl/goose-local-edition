//! "Free up memory" reclaims goose does outside its own engine, each one measured by the caller:
//!
//! - **Ollama**: `GET /api/ps` lists the loaded models (`size` = bytes held); `POST /api/generate
//!   {"model": …, "keep_alive": 0}` unloads one now (docs.ollama.com FAQ, "How do I keep a model
//!   loaded in memory or make it unload immediately?", read 2026-10-25).
//! - **LM Studio**: `GET /api/v1/models` lists every model with `loaded_instances[].id`;
//!   `POST /api/v1/models/unload {"instance_id": …}` unloads one (lmstudio.ai/docs/developer/rest
//!   list + unload, read 2026-10-25). When that server is off or wants a token goose has not got,
//!   the `lms` CLI's `lms unload --all` (lmstudio.ai/docs/cli/unload) talks to the app directly.
//! - **A left-over goose server**: the same proof the desktop's launch reap uses
//!   (ui/desktop/src/utils/orphanGoosedReap.ts `selectOrphanedGoosed`), re-proven before each
//!   signal; SIGTERM only, reported (never forced) if it outlives the grace.

use crate::fit::gb;
use crate::memfree::ProcRow;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// The two model servers goose can ask to unload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiServerKind {
    Ollama,
    LmStudio,
}

/// Which server an `aiServer` group is, from its census id/label/bundle id.
pub fn ai_server_kind(group_id: &str, bundle_id: Option<&str>) -> Option<AiServerKind> {
    let id = group_id.to_ascii_lowercase();
    if bundle_id == Some("ai.elementlabs.lmstudio")
        || id.contains("lm studio")
        || id.ends_with("/lms")
        || id == "ai:lms"
    {
        return Some(AiServerKind::LmStudio);
    }
    if bundle_id == Some("com.electron.ollama") || id.contains("ollama") {
        return Some(AiServerKind::Ollama);
    }
    None
}

/// One loaded model: the name the unload call takes, and the bytes the server says it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedModel {
    pub name: String,
    pub bytes: u64,
}

pub fn ollama_base() -> String {
    match std::env::var("OLLAMA_HOST") {
        Ok(h) if !h.trim().is_empty() => {
            let h = h.trim().trim_end_matches('/');
            let h = if h.contains("://") {
                h.to_string()
            } else {
                format!("http://{h}")
            };
            // `OLLAMA_HOST=0.0.0.0` means "listen everywhere"; ask it on loopback.
            h.replace("0.0.0.0", "127.0.0.1")
        }
        _ => "http://127.0.0.1:11434".to_string(),
    }
}

pub fn lmstudio_base() -> String {
    "http://127.0.0.1:1234".to_string()
}

fn lmstudio_token() -> Option<String> {
    std::env::var("LM_API_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// `GET /api/ps` → loaded models.
pub fn parse_ollama_ps(body: &Value) -> Result<Vec<LoadedModel>> {
    let models = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Ollama's /api/ps has no models list"))?;
    Ok(models
        .iter()
        .filter_map(|m| {
            let name = m
                .get("model")
                .or_else(|| m.get("name"))
                .and_then(Value::as_str)?;
            Some(LoadedModel {
                name: name.to_string(),
                bytes: m.get("size").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect())
}

/// `GET /api/v1/models` → one entry per loaded instance (a model loaded twice holds it twice).
pub fn parse_lmstudio_models(body: &Value) -> Result<Vec<LoadedModel>> {
    let models = body
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("LM Studio's /api/v1/models has no models list"))?;
    let mut out = vec![];
    for m in models {
        let bytes = m.get("size_bytes").and_then(Value::as_u64).unwrap_or(0);
        for inst in m
            .get("loaded_instances")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = inst.get("id").and_then(Value::as_str) {
                out.push(LoadedModel {
                    name: id.to_string(),
                    bytes,
                });
            }
        }
    }
    Ok(out)
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("http client")
}

async fn get_json(url: &str, token: Option<&str>) -> Result<Value> {
    let mut req = client()?.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("GET {url} answered {status}");
    }
    let text = resp.text().await?;
    serde_json::from_str(&text).with_context(|| format!("GET {url} body"))
}

async fn post_json(url: &str, body: Value, token: Option<&str>) -> Result<()> {
    let mut req = client()?
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string());
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.with_context(|| format!("POST {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!("POST {url} answered {status}: {}", text.trim());
    }
    Ok(())
}

pub async fn loaded_models(kind: AiServerKind, base: &str) -> Result<Vec<LoadedModel>> {
    match kind {
        AiServerKind::Ollama => parse_ollama_ps(&get_json(&format!("{base}/api/ps"), None).await?),
        AiServerKind::LmStudio => parse_lmstudio_models(
            &get_json(
                &format!("{base}/api/v1/models"),
                lmstudio_token().as_deref(),
            )
            .await?,
        ),
    }
}

pub async fn unload_model(kind: AiServerKind, base: &str, name: &str) -> Result<()> {
    match kind {
        AiServerKind::Ollama => {
            post_json(
                &format!("{base}/api/generate"),
                serde_json::json!({ "model": name, "keep_alive": 0 }),
                None,
            )
            .await
        }
        AiServerKind::LmStudio => {
            post_json(
                &format!("{base}/api/v1/models/unload"),
                serde_json::json!({ "instance_id": name }),
                lmstudio_token().as_deref(),
            )
            .await
        }
    }
}

/// The `lms` CLI: LM Studio installs it at `~/.lmstudio/bin/lms`; else whatever PATH has.
pub fn lms_cli() -> Option<PathBuf> {
    let home = dirs::home_dir().map(|h| h.join(".lmstudio/bin/lms"));
    if let Some(p) = home.filter(|p| p.is_file()) {
        return Some(p);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("lms"))
            .find(|p| p.is_file())
    })
}

/// `lms unload --all`, bounded.
pub async fn lms_unload_all(lms: &std::path::Path) -> Result<String> {
    let run = tokio::process::Command::new(lms)
        .args(["unload", "--all"])
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(30), run)
        .await
        .map_err(|_| anyhow!("`lms unload --all` did not finish in 30 s"))?
        .context("run lms")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        bail!(
            "`lms unload --all` failed ({}): {}",
            out.status,
            text.trim()
        );
    }
    Ok(text.trim().to_string())
}

// ---------------------------------------------------------------------------------------------
// Left-over goose servers
// ---------------------------------------------------------------------------------------------

/// The desktop's own `goose serve` shape (orphanGoosedReap.ts `isDesktopServe`).
pub fn is_desktop_serve(args: &str) -> bool {
    let serve = args.contains(" serve ") || args.ends_with(" serve");
    serve && args.contains(" --platform desktop") && args.contains(" --host 127.0.0.1")
}

/// THE PROOF, pure: ppid 1 (its app is gone), this user, not us, THIS install's goose binary
/// (`self_exe`, the running goose's own path), running the desktop's `serve` shape. Anything else
/// is left alone. Returns (pid, started_at).
pub fn select_orphaned_goosed(
    rows: &[ProcRow],
    cmds: &HashMap<u32, String>,
    self_exe: &str,
    uid: u32,
    self_pid: u32,
) -> Vec<(u32, u64)> {
    rows.iter()
        .filter(|r| r.ppid == 1 && r.uid == uid && r.pid != self_pid && r.pid > 1)
        .filter(|r| !self_exe.is_empty() && r.path == self_exe)
        .filter(|r| cmds.get(&r.pid).is_some_and(|c| is_desktop_serve(c)))
        .map(|r| (r.pid, r.started_at))
        .collect()
}

/// This process's executable, resolved the way `proc_pidpath` reports it.
pub fn self_exe() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(&p).ok().or(Some(p)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Command lines for the pids given (sysinfo; argv joined by spaces).
#[cfg(unix)]
pub fn cmdlines(pids: &[u32]) -> HashMap<u32, String> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    if pids.is_empty() {
        return HashMap::new();
    }
    let wanted: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&wanted),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    pids.iter()
        .filter_map(|&pid| {
            let p = sys.process(Pid::from_u32(pid))?;
            let cmd: Vec<String> = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            (!cmd.is_empty()).then(|| (pid, cmd.join(" ")))
        })
        .collect()
}

#[cfg(not(unix))]
pub fn cmdlines(_pids: &[u32]) -> HashMap<u32, String> {
    HashMap::new()
}

/// The proven orphans in a census (blocking: reads command lines).
pub fn orphaned_goosed_now(rows: &[ProcRow], self_pid: u32, uid: u32) -> Vec<(u32, u64)> {
    let exe = self_exe();
    let candidates: Vec<u32> = rows
        .iter()
        .filter(|r| r.ppid == 1 && r.uid == uid && r.path == exe)
        .map(|r| r.pid)
        .collect();
    select_orphaned_goosed(rows, &cmdlines(&candidates), &exe, uid, self_pid)
}

/// How long a left-over server's own teardown may take before goose reports it still running (its mesh daemon and engine
/// stop on SIGTERM; the desktop's quit grace is of the same order).
pub const ORPHAN_GRACE: Duration = Duration::from_secs(10);

/// Is `(pid, started_at)` still the proven orphan (same process, still ppid 1, still this
/// install's desktop serve)?
#[cfg(unix)]
fn still_orphan(pid: u32, started_at: u64, exe: &str, uid: u32) -> bool {
    let rows: Vec<ProcRow> = crate::memfree::census()
        .into_iter()
        .filter(|r| r.pid == pid)
        .collect();
    let ok = rows.first().is_some_and(|r| r.started_at == started_at);
    ok && !select_orphaned_goosed(&rows, &cmdlines(&[pid]), exe, uid, std::process::id()).is_empty()
}

/// Stop one proven left-over goose server: re-proven, SIGTERM, wait the grace; still running is
/// reported, never forced.
#[cfg(unix)]
pub async fn reap_orphaned_goosed(pid: u32, started_at: u64) -> Result<String> {
    let exe = self_exe();
    let uid = crate::memfree::self_uid();
    let proven = {
        let exe = exe.clone();
        move || still_orphan(pid, started_at, &exe, uid)
    };
    if !tokio::task::spawn_blocking(proven).await? {
        bail!("pid {pid} is no longer the left-over goose server it was");
    }
    // SAFETY: plain signal to a pid proven to be the orphan a moment ago.
    if unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } != 0 {
        bail!("SIGTERM to pid {pid}: {}", std::io::Error::last_os_error());
    }
    let deadline = tokio::time::Instant::now() + ORPHAN_GRACE;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if !crate::memfree::alive(pid, started_at) {
            return Ok(format!("stopped left-over goose server pid {pid}"));
        }
    }
    bail!(
        "left-over goose server pid {pid} is still running {} s after SIGTERM; goose does not \
         force it",
        ORPHAN_GRACE.as_secs()
    )
}

/// Stop ANOTHER MLX engine for "Free up memory": re-proven against a fresh engine census (still an
/// MLX engine, not on `own_port`, same start time), then SIGTERM only — never SIGKILL, never a
/// group. Reports, without forcing, an engine still running after the grace window.
/// (`machine::stop_other_engine`, the mlxEngine ACP's explicit stop, keeps its own behaviour.)
#[cfg(unix)]
pub async fn terminate_engine(pid: u32, started_at: u64, own_port: u16) -> Result<String> {
    let engines =
        tokio::task::spawn_blocking(move || crate::machine::other_engines(Some(own_port))).await?;
    let Some(engine) = engines.into_iter().find(|e| e.pid == pid) else {
        bail!("pid {pid} is not another MLX engine on this Mac: nothing was signalled");
    };
    if engine.started_at != started_at {
        bail!("pid {pid} is no longer the engine you were shown: nothing was signalled");
    }
    // SAFETY: plain signal to one pid proven a moment ago to be that engine.
    if unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } != 0 {
        bail!("SIGTERM to pid {pid}: {}", std::io::Error::last_os_error());
    }
    let signalled = std::time::Instant::now();
    while signalled.elapsed() < crate::grace_window() {
        if crate::machine::prove(pid, started_at) != crate::machine::Liveness::Alive {
            return Ok(format!(
                "stopped {} (SIGTERM); its {} are the Mac's again",
                engine.describe(),
                gb(engine.resident_bytes)
            ));
        }
        tokio::time::sleep(crate::GRACE_TICK).await;
    }
    bail!(
        "{} is still running {} s after SIGTERM; goose does not force it",
        engine.describe(),
        crate::grace_window().as_secs()
    )
}

#[cfg(not(unix))]
pub async fn terminate_engine(pid: u32, _started_at: u64, _own_port: u16) -> Result<String> {
    bail!("pid {pid}: stopping a process is unix-only")
}

#[cfg(not(unix))]
pub async fn reap_orphaned_goosed(pid: u32, _started_at: u64) -> Result<String> {
    bail!("pid {pid}: stopping a process is unix-only")
}

/// The port in a server base URL (`http://127.0.0.1:1234` → 1234); `None` without one.
pub fn base_port(base: &str) -> Option<u16> {
    let host = base.split_once("://").map_or(base, |(_, h)| h);
    let host = host.split('/').next().unwrap_or("");
    host.rsplit_once(':')?.1.parse().ok()
}

/// `lsof -F pc` output → (pid, command), one per process, in first-seen order. Other field lines
/// (`f…`, the per-file fields lsof always adds) are skipped.
pub fn parse_lsof_fpc(out: &str) -> Vec<(u32, String)> {
    let mut res: Vec<(u32, String)> = vec![];
    let mut current: Option<u32> = None;
    for line in out.lines() {
        let mut chars = line.chars();
        let Some(tag) = chars.next() else { continue };
        let rest = chars.as_str().trim();
        match tag {
            'p' => current = rest.parse().ok(),
            'c' => {
                if let Some(pid) = current {
                    if !res.iter().any(|(p, _)| *p == pid) {
                        res.push((pid, rest.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    res
}

/// Who holds an ESTABLISHED TCP connection to `port` on this Mac, minus the server's own pids
/// (`lsof -nP -iTCP:<port> -sTCP:ESTABLISHED -Fpc`, bounded to 5 s). Read-only. Without root lsof
/// sees only this user's sockets, so a client of another user is not listed.
pub fn port_clients(port: u16, exclude: &[u32]) -> Vec<(u32, String)> {
    let child = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fpc"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return vec![];
    };
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < std::time::Duration::from_secs(5) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                // Our own lsof child only.
                let _ = child.kill();
                let _ = child.wait();
                return vec![];
            }
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        let _ = stdout.read_to_string(&mut out);
    }
    parse_lsof_fpc(&out)
        .into_iter()
        .filter(|(pid, _)| !exclude.contains(pid))
        .collect()
}

fn proc_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The "used by" lines a confirm shows for a server group: its TCP clients, the process that
/// started it (when that is not launchd) and the launchd job that owns it.
pub fn used_by_lines(
    group: &crate::memfree::Group,
    rows: &[ProcRow],
    clients: &[(u32, String)],
) -> Vec<String> {
    let own: Vec<u32> = group.pids.iter().map(|(p, _)| *p).collect();
    let mut out: Vec<String> = clients
        .iter()
        .filter(|(pid, _)| !own.contains(pid))
        .map(|(pid, name)| format!("{name} (pid {pid}) is connected to it"))
        .collect();
    let parent = group.pids.iter().find_map(|(pid, _)| {
        let row = rows.iter().find(|r| r.pid == *pid)?;
        (row.ppid > 1 && !own.contains(&row.ppid)).then_some(row.ppid)
    });
    if let Some(ppid) = parent {
        let name = rows
            .iter()
            .find(|r| r.pid == ppid)
            .map(|r| proc_name(&r.path).to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "an unreadable process".into());
        out.push(format!("started by {name} (pid {ppid})"));
    }
    if let Some(label) = &group.launchd_label {
        out.push(format!("launchd job {label}"));
    }
    out
}

/// The pids of an app that may get a SIGTERM after it ignored "quit" (nuclear clean only), from a
/// FRESH census: still a quittable user app, not protected, not on the never-touch list, and only
/// its processes that run as this user with the start time the census shows. `Ok(empty)` when the
/// app is gone; `Err(why)` when it may not be signalled at all.
pub fn app_term_targets(
    group: Option<&crate::memfree::Group>,
    rows: &[ProcRow],
    self_uid: u32,
    self_pid: u32,
    never_touch: &[String],
) -> std::result::Result<Vec<(u32, u64)>, String> {
    use crate::memfree::Category;
    let Some(g) = group else {
        return Ok(vec![]);
    };
    if g.category != Category::UserApp {
        return Err(format!("{} is not a user app", g.label));
    }
    if let Some(why) = &g.protected_why {
        return Err(format!("{} is protected: {why}", g.label));
    }
    if let Some(hit) = crate::memfree_protect::group_never_touch(g, never_touch) {
        return Err(format!("{} is on the never-touch list ({hit})", g.label));
    }
    if !g.quittable {
        return Err(format!(
            "{}: {}",
            g.label,
            g.why
                .clone()
                .unwrap_or_else(|| "goose may not quit it".into())
        ));
    }
    Ok(g.pids
        .iter()
        .filter(|(pid, started)| {
            *pid > 1
                && *pid != self_pid
                && rows
                    .iter()
                    .any(|r| r.pid == *pid && r.started_at == *started && r.uid == self_uid)
        })
        .copied()
        .collect())
}

/// SIGTERM each (pid, start) still alive, then wait the grace window. Never SIGKILL, never a
/// process group. Returns how many are still running afterwards.
#[cfg(unix)]
pub async fn terminate_pids(targets: &[(u32, u64)]) -> usize {
    for &(pid, started) in targets {
        if crate::memfree::alive(pid, started) {
            // SAFETY: plain signal to one pid whose start time was re-proven a moment ago.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
    }
    let signalled = std::time::Instant::now();
    loop {
        let left = targets
            .iter()
            .filter(|(p, s)| crate::memfree::alive(*p, *s))
            .count();
        if left == 0 || signalled.elapsed() >= crate::grace_window() {
            return left;
        }
        tokio::time::sleep(crate::GRACE_TICK).await;
    }
}

#[cfg(not(unix))]
pub async fn terminate_pids(targets: &[(u32, u64)]) -> usize {
    targets.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn row(pid: u32, ppid: u32, uid: u32, path: &str) -> ProcRow {
        ProcRow {
            pid,
            ppid,
            uid,
            started_at: 1000 + pid as u64,
            path: path.into(),
            footprint: 1,
            pgid: pid,
            has_tty: false,
        }
    }

    #[test]
    fn server_kind_from_group() {
        assert_eq!(
            ai_server_kind("ai:/Applications/LM Studio.app", None),
            Some(AiServerKind::LmStudio)
        );
        assert_eq!(
            ai_server_kind("ai:x", Some("ai.elementlabs.lmstudio")),
            Some(AiServerKind::LmStudio)
        );
        assert_eq!(ai_server_kind("ai:lms", None), Some(AiServerKind::LmStudio));
        assert_eq!(
            ai_server_kind("ai:/Applications/Ollama.app", None),
            Some(AiServerKind::Ollama)
        );
        assert_eq!(
            ai_server_kind("ai:ollama", None),
            Some(AiServerKind::Ollama)
        );
        assert_eq!(ai_server_kind("ai:llama-server", None), None);
    }

    #[test]
    fn parses_both_servers_lists() {
        let ps = serde_json::json!({"models": [
            {"name": "llama3.2:latest", "model": "llama3.2:latest", "size": 3_000_000_000u64, "size_vram": 3_000_000_000u64},
            {"name": "qwen3:8b", "size": 6_000_000_000u64}
        ]});
        assert_eq!(
            parse_ollama_ps(&ps).unwrap(),
            vec![
                LoadedModel {
                    name: "llama3.2:latest".into(),
                    bytes: 3_000_000_000
                },
                LoadedModel {
                    name: "qwen3:8b".into(),
                    bytes: 6_000_000_000
                },
            ]
        );
        assert!(parse_ollama_ps(&serde_json::json!({})).is_err());
        let lm = serde_json::json!({"models": [
            {"key": "openai/gpt-oss-20b", "size_bytes": 12_000_000_000u64,
             "loaded_instances": [{"id": "openai/gpt-oss-20b", "config": {}}, {"id": "openai/gpt-oss-20b:2", "config": {}}]},
            {"key": "idle", "size_bytes": 5u64, "loaded_instances": []}
        ]});
        let got = parse_lmstudio_models(&lm).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].name, "openai/gpt-oss-20b:2");
        assert!(got.iter().all(|m| m.bytes == 12_000_000_000));
    }

    #[test]
    fn desktop_serve_shape() {
        let exe = "/Applications/Goose.app/Contents/Resources/bin/goose";
        assert!(is_desktop_serve(&format!(
            "{exe} serve --tls --platform desktop --host 127.0.0.1 --port 54154"
        )));
        assert!(!is_desktop_serve(&format!("{exe} serve --host 127.0.0.1")));
        assert!(!is_desktop_serve(&format!(
            "{exe} session --platform desktop --host 127.0.0.1"
        )));
    }

    #[test]
    fn orphan_proof_table() {
        let exe = "/Applications/Goose.app/Contents/Resources/bin/goose";
        let other = "/Users/x/Projects/goose/target/debug/goose";
        let desk =
            |p: &str| format!("{p} serve --tls --platform desktop --host 127.0.0.1 --port 1");
        let rows = vec![
            row(10, 1, 501, exe),   // proven
            row(11, 77, 501, exe),  // its app is alive
            row(12, 1, 502, exe),   // another user
            row(13, 1, 501, other), // another install: named, never stopped
            row(14, 1, 501, exe),   // not the desktop shape
            row(500, 1, 501, exe),  // ourselves
        ];
        let cmds: HashMap<u32, String> = [
            (10, desk(exe)),
            (11, desk(exe)),
            (12, desk(exe)),
            (13, desk(other)),
            (14, format!("{exe} run --text hi")),
            (500, desk(exe)),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            select_orphaned_goosed(&rows, &cmds, exe, 501, 500),
            vec![(10, 1010)]
        );
        // No command line read = no proof.
        assert!(select_orphaned_goosed(&rows, &HashMap::new(), exe, 501, 500).is_empty());
        assert!(select_orphaned_goosed(&rows, &cmds, "", 501, 500).is_empty());
    }

    /// A one-shot HTTP server: answers `body` to the first request and hands back what it got.
    async fn serve_once(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let reply = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let mut got = String::new();
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                got.push_str(&String::from_utf8_lossy(&buf[..n]));
                if let Some(head_end) = got.find("\r\n\r\n") {
                    let len = got[..head_end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if got.len() >= head_end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            sock.write_all(reply.as_bytes()).await.unwrap();
            got
        });
        (base, handle)
    }

    #[tokio::test]
    async fn ollama_unload_sends_keep_alive_zero() {
        let (base, got) = serve_once("200 OK", r#"{"done":true,"done_reason":"unload"}"#).await;
        unload_model(AiServerKind::Ollama, &base, "qwen3:8b")
            .await
            .unwrap();
        let req = got.await.unwrap();
        assert!(req.starts_with("POST /api/generate "));
        let body: Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"model": "qwen3:8b", "keep_alive": 0})
        );
    }

    #[tokio::test]
    async fn lmstudio_unload_posts_instance_id_and_reports_refusal() {
        let (base, got) = serve_once("200 OK", r#"{"instance_id":"m1"}"#).await;
        unload_model(AiServerKind::LmStudio, &base, "m1")
            .await
            .unwrap();
        let req = got.await.unwrap();
        assert!(req.starts_with("POST /api/v1/models/unload "));
        let body: Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body, serde_json::json!({"instance_id": "m1"}));

        let (base, _) = serve_once("401 Unauthorized", r#"{"error":"token"}"#).await;
        let err = unload_model(AiServerKind::LmStudio, &base, "m1")
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("401"));
    }

    #[tokio::test]
    async fn loaded_models_reads_ollama_ps() {
        let (base, got) = serve_once("200 OK", r#"{"models":[{"model":"a:1","size":42}]}"#).await;
        let models = loaded_models(AiServerKind::Ollama, &base).await.unwrap();
        assert_eq!(
            models,
            vec![LoadedModel {
                name: "a:1".into(),
                bytes: 42
            }]
        );
        assert!(got.await.unwrap().starts_with("GET /api/ps "));
    }

    #[tokio::test]
    async fn server_off_is_an_error_not_an_empty_list() {
        // Bind then drop: nothing listens on that port.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let base = format!("http://127.0.0.1:{port}");
        assert!(loaded_models(AiServerKind::Ollama, &base).await.is_err());
    }

    fn group(
        id: &str,
        cat: crate::memfree::Category,
        pids: &[(u32, u64)],
    ) -> crate::memfree::Group {
        crate::memfree::Group {
            id: id.into(),
            label: id.trim_start_matches("app:").into(),
            category: cat,
            bytes: 1,
            pids: pids.to_vec(),
            bundle_id: id.strip_prefix("app:").map(str::to_string),
            quittable: cat == crate::memfree::Category::UserApp,
            reclaimable_by_goose: false,
            why: None,
            protected_why: None,
            used_by: vec![],
            launchd_label: None,
            nuclear_default: false,
        }
    }

    #[test]
    fn memfree_base_port_reads_the_url() {
        assert_eq!(base_port("http://127.0.0.1:1234"), Some(1234));
        assert_eq!(base_port("http://127.0.0.1:11434/"), Some(11434));
        assert_eq!(base_port("localhost:8093/v1"), Some(8093));
        assert_eq!(base_port("http://example.com"), None);
    }

    #[test]
    fn memfree_lsof_fpc_parses_clients_once_per_pid() {
        let out =
            "p97100\ncrapid-mlx\nf12\nf13\np9375\ncPython\nf7\np9375\ncPython\np14200\ncnode\n";
        assert_eq!(
            parse_lsof_fpc(out),
            vec![
                (97100, "rapid-mlx".to_string()),
                (9375, "Python".to_string()),
                (14200, "node".to_string())
            ]
        );
        assert!(parse_lsof_fpc("").is_empty());
        assert!(parse_lsof_fpc("cname-without-pid\nfé\n").is_empty());
    }

    #[test]
    fn memfree_used_by_names_clients_parent_and_launchd_job() {
        use crate::memfree::Category;
        let rows = vec![
            row(97100, 36740, 501, "/opt/rapid-mlx/bin/python3"),
            row(
                36740,
                1,
                501,
                "/Users/w/Projects/machine-sync/bin/heavy-run",
            ),
            row(9375, 1, 501, "/usr/bin/python3"),
        ];
        let mut g = group("mlx:97100", Category::GooseEngine, &[(97100, 98100)]);
        g.launchd_label = Some("net.leanzero.tts".into());
        let clients = vec![(97100, "python3".to_string()), (9375, "Python".to_string())];
        assert_eq!(
            used_by_lines(&g, &rows, &clients),
            vec![
                "Python (pid 9375) is connected to it".to_string(),
                "started by heavy-run (pid 36740)".to_string(),
                "launchd job net.leanzero.tts".to_string(),
            ]
        );
        // ppid 1 (launchd) is not a "started by" line.
        let lone = group("mlx:9375", Category::GooseEngine, &[(9375, 10375)]);
        assert!(used_by_lines(&lone, &rows, &[]).is_empty());
    }

    #[test]
    fn memfree_app_term_targets_only_own_unprotected_unlisted_apps() {
        use crate::memfree::Category;
        let rows = vec![
            row(
                700,
                1,
                501,
                "/Applications/TextEdit.app/Contents/MacOS/TextEdit",
            ),
            row(
                701,
                700,
                0,
                "/Applications/TextEdit.app/Contents/MacOS/helper",
            ),
            row(
                800,
                1,
                501,
                "/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams",
            ),
        ];
        let nt = crate::memfree_protect::never_touch_list(&[]);
        let te = group(
            "app:com.apple.TextEdit",
            Category::UserApp,
            &[(700, 1700), (701, 1701), (999, 1)],
        );
        // Own uid only, live start time only.
        assert_eq!(
            app_term_targets(Some(&te), &rows, 501, 1, &nt),
            Ok(vec![(700, 1700)])
        );
        // Gone: nothing to signal.
        assert_eq!(app_term_targets(None, &rows, 501, 1, &nt), Ok(vec![]));
        // Never-touch, protected, system and not-quittable are refused outright.
        let teams = group(
            "app:com.microsoft.teams2",
            Category::UserApp,
            &[(800, 1800)],
        );
        assert!(app_term_targets(Some(&teams), &rows, 501, 1, &nt).is_err());
        let mut prot = te.clone();
        prot.protected_why = Some("has a terminal".into());
        assert!(app_term_targets(Some(&prot), &rows, 501, 1, &nt).is_err());
        let sys = group("sys:WindowServer", Category::System, &[(700, 1700)]);
        assert!(app_term_targets(Some(&sys), &rows, 501, 1, &nt).is_err());
        let mut nq = te.clone();
        nq.quittable = false;
        assert!(app_term_targets(Some(&nq), &rows, 501, 1, &nt).is_err());
        // The caller's own pid is never a target.
        assert_eq!(
            app_term_targets(Some(&te), &rows, 501, 700, &nt),
            Ok(vec![])
        );
    }
}
