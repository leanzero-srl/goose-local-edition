//! Q-138: a stdio extension child dies with what owns it, and a startup reaper stops orphans that
//! provably run goose's own bundled-MCP entries — per pid, never anything else.
//!
//! The fake MCPs here are the worst case measured on 2026-09-26: they ignore stdin EOF and SIGTERM,
//! and one launches a grandchild into its process group (the way a web-search MCP launches a browser).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use goose::agents::extension::{Envs, ExtensionConfig};
use goose::agents::extension_manager::ExtensionManager;
use goose::agents::stdio_children;
use goose_sidecar::{GRACE_TICK, GRACE_TICKS};
use serial_test::serial;

fn alive(pid: u32) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    let stat = String::from_utf8_lossy(&out.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

fn parent_of(pid: u32) -> Option<u32> {
    let out = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Poll through two grace windows — one for the TERM, one for the escalation to land.
async fn all_gone(pids: &[u32]) -> bool {
    for _ in 0..(2 * GRACE_TICKS) {
        if pids.iter().all(|pid| !alive(*pid)) {
            return true;
        }
        tokio::time::sleep(GRACE_TICK).await;
    }
    pids.iter().all(|pid| !alive(*pid))
}

const DEAF_MCP: &str = r#"
import json, os, signal, subprocess, sys
signal.signal(signal.SIGTERM, signal.SIG_IGN)
grandchild = subprocess.Popen(["sleep", "600"])
with open(sys.argv[1], "w") as f:
    f.write(f"{os.getpid()} {grandchild.pid}")
for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:
        continue
    if msg["method"] == "initialize":
        result = {"protocolVersion": msg["params"]["protocolVersion"], "capabilities": {"tools": {}},
                  "serverInfo": {"name": "q138-deaf", "version": "0"}}
    elif msg["method"] == "tools/list":
        result = {"tools": []}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
while True:
    signal.pause()
"#;

fn read_pids(path: &Path) -> (u32, u32) {
    let text = std::fs::read_to_string(path).expect("fake MCP wrote its pids");
    let mut parts = text.split_whitespace().map(|p| p.parse::<u32>().unwrap());
    (parts.next().unwrap(), parts.next().unwrap())
}

#[tokio::test]
#[serial]
async fn dropping_the_session_extension_manager_kills_its_stdio_child_and_its_group() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("deaf_mcp.py");
    let pids_file = dir.path().join("pids");
    std::fs::write(&script, DEAF_MCP).unwrap();

    let manager = Arc::new(ExtensionManager::new_without_provider(
        dir.path().to_path_buf(),
    ));
    manager
        .add_extension(
            ExtensionConfig::Stdio {
                name: "q138-deaf".into(),
                description: String::new(),
                cmd: "python3".into(),
                args: vec![
                    script.display().to_string(),
                    pids_file.display().to_string(),
                ],
                envs: Envs::default(),
                env_keys: Vec::new(),
                timeout: Some(60),
                cwd: None,
                bundled: None,
                available_tools: Vec::new(),
            },
            Some(dir.path().to_path_buf()),
            None,
            None,
        )
        .await
        .expect("the fake MCP connects");

    let (mcp, grandchild) = read_pids(&pids_file);
    assert!(alive(mcp) && alive(grandchild));
    assert!(
        stdio_children::registered_pids().contains(&mcp),
        "the stdio child was not registered"
    );

    drop(manager);

    assert!(
        all_gone(&[mcp, grandchild]).await,
        "a TERM-deaf stdio MCP (pid {mcp}) or its group's grandchild (pid {grandchild}) outlived its session"
    );
    assert!(!stdio_children::registered_pids().contains(&mcp));
}

#[tokio::test]
#[serial]
async fn goosed_teardown_stops_every_registered_child_and_names_the_deaf_ones() {
    let mut deaf = tokio::process::Command::new("sh");
    deaf.args(["-c", "trap '' TERM; echo deaf; while :; do sleep 0.2; done"]);
    goose::subprocess::configure_subprocess(&mut deaf);
    let mut deaf = deaf
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // Wait until the trap is armed; a TERM before it would kill the shell and prove nothing.
    let mut ready = String::new();
    tokio::io::AsyncBufReadExt::read_line(
        &mut tokio::io::BufReader::new(deaf.stdout.take().unwrap()),
        &mut ready,
    )
    .await
    .unwrap();
    assert_eq!(ready.trim(), "deaf");
    let mut polite = tokio::process::Command::new("sleep");
    polite.arg("600");
    goose::subprocess::configure_subprocess(&mut polite);
    let polite = polite.stdin(Stdio::null()).spawn().unwrap();
    let (deaf_pid, polite_pid) = (deaf.id().unwrap(), polite.id().unwrap());

    let _deaf_guard = stdio_children::register(deaf_pid, "q138-deaf").unwrap();
    let _polite_guard = stdio_children::register(polite_pid, "q138-polite").unwrap();

    let outcome = stdio_children::teardown_all().await;

    assert!(
        outcome.contains("2 stdio extension child(ren)"),
        "{outcome}"
    );
    assert!(outcome.contains("1 exited on SIGTERM"), "{outcome}");
    assert!(
        outcome.contains(&format!(
            "SIGKILLed after ignoring SIGTERM: q138-deaf (pid {deaf_pid}, group survivors [{deaf_pid}])"
        )),
        "{outcome}"
    );
    assert!(all_gone(&[deaf_pid, polite_pid]).await);
    assert!(stdio_children::teardown_all()
        .await
        .contains("no stdio extension children"));
}

/// Start `/bin/sh <script>` detached so its parent exits at once and it is reparented to PID 1.
fn spawn_orphan(script: &Path) -> u32 {
    let out = Command::new("sh")
        .args([
            "-c",
            "/bin/sh \"$0\" </dev/null >/dev/null 2>&1 & echo $!",
            &script.display().to_string(),
        ])
        .output()
        .unwrap();
    let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    for _ in 0..GRACE_TICKS {
        if parent_of(pid) == Some(1) {
            return pid;
        }
        std::thread::sleep(GRACE_TICK);
    }
    panic!("pid {pid} was never reparented to PID 1");
}

fn write_script(root: &Path, rel: &str, body: &str) -> PathBuf {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

#[tokio::test]
#[serial]
async fn the_startup_reaper_stops_only_provable_goose_orphans_per_pid() {
    let dir = tempfile::tempdir().unwrap();
    // A unique id so the scan can match nothing real on this machine.
    let id = format!("leanzero-q138-test-{}", std::process::id());
    let entry = format!("bundled-mcps/{id}/dist/index.js");
    let scripts = vec![PathBuf::from(&entry)];

    let spin = "while :; do sleep 0.2; done\n";
    let deaf = format!("trap '' TERM; {spin}");
    let polite_orphan = spawn_orphan(&write_script(
        dir.path(),
        &format!("install-a/{entry}"),
        spin,
    ));
    let deaf_orphan = spawn_orphan(&write_script(
        dir.path(),
        &format!("install-b/{entry}"),
        &deaf,
    ));
    // Controls: an orphan that is not a bundled entry, and a bundled entry whose parent is alive.
    let foreign_orphan = spawn_orphan(&write_script(
        dir.path(),
        &format!("elsewhere/{id}/dist/index.js"),
        spin,
    ));
    let mut adopted = Command::new("/bin/sh")
        .arg(write_script(
            dir.path(),
            &format!("install-c/{entry}"),
            spin,
        ))
        .stdin(Stdio::null())
        .spawn()
        .unwrap();

    let mut reaped = stdio_children::reap_orphans_running(&scripts).await;
    reaped.sort_by_key(|r| r.pid);
    let mut expected = vec![(polite_orphan, "SIGTERM"), (deaf_orphan, "SIGKILL")];
    expected.sort();
    assert_eq!(
        reaped.iter().map(|r| (r.pid, r.signal)).collect::<Vec<_>>(),
        expected
    );
    assert!(all_gone(&[polite_orphan, deaf_orphan]).await);

    let foreign_alive = alive(foreign_orphan);
    let adopted_alive = alive(adopted.id());
    let _ = Command::new("kill")
        .arg(foreign_orphan.to_string())
        .status();
    let _ = adopted.kill();
    let _ = adopted.wait();
    assert!(
        foreign_alive,
        "an orphan outside the bundled entries was touched"
    );
    assert!(
        adopted_alive,
        "a bundled MCP with a live parent was touched"
    );
}
