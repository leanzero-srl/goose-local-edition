//! The controller's menu-bar status, mirrored on every peer of a distributed run (owner
//! 2026-10-31: "when distributed compute happens I want to see the same message in the status bar
//! for all macs involved. Right now the master shows but the so called slave doesn't show a
//! thing").
//!
//! A peer's goose app reads only its own goosed. A rank started over ssh is a `python -c` child
//! of the peer's sshd that the peer's goosed never hears of, and a Link-hosted rank's record
//! carries no word of what the controller is doing (a benchmark's calls, scoring). So the
//! controller tells each peer, over the channel it already holds to it:
//!
//! 1. The controller's Electron main writes the title it sets (and the benchmark's menu lines) to
//!    `~/.goose/distributed/tray/controller.json`, rewriting it every few seconds while it holds.
//! 2. This module's relay, alive for the run's whole supervision (loading, serving, restarts),
//!    builds one [`PeerTrayRecord`] per peer rank from the run's status plus that title while it is
//!    fresh, and sends it as [`super::node_op::NodeOp::TrayStatus`] — on a change, and as a heartbeat
//!    every third of the staleness window. The run's end sends one last record (stopped / failed).
//! 3. The peer writes it atomically to `~/.goose/distributed/tray/rank-status.json`; the peer's goose
//!    app (if one runs — none is fine) polls that file and shows the same title with its role added,
//!    "… · rank 1/2 (for <controller>)". A record older than `staleAfterS`, or whose rank pid is gone,
//!    reads "rank stopped" for a short hold and then clears: never a frozen message.
//!
//! Tunables: `GOOSE_DISTRIBUTED_PEER_TRAY=off` turns the relay off;
//! `GOOSE_DISTRIBUTED_PEER_TRAY_STALE_SECS` (default 30, clamped 6..=3600) is the window after which a
//! peer stops believing a record.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::{debug, warn};

use super::exec::{sh_quote, NodeExec};
use super::node_op::NodeOp;

/// Under `$HOME` on every Mac: the controller's title (written by its app) and a peer's record
/// (written by the controller's relay). The desktop app reads the same paths (`peerTray.ts`).
pub const TRAY_DIR: &str = ".goose/distributed/tray";
pub const CONTROLLER_FILE: &str = "controller.json";
pub const RANK_FILE: &str = "rank-status.json";
pub const RECORD_VERSION: u32 = 1;
pub const DEFAULT_STALE_SECS: u64 = 30;
const STALE_RANGE: (u64, u64) = (6, 3600);
const TEXT_MAX: usize = 200;
const LINES_MAX: usize = 12;
/// The whole record as the peer accepts it (a Link peer refuses a larger one).
pub const RECORD_MAX_BYTES: usize = 8 * 1024;
const TICK: Duration = Duration::from_secs(2);
const PUSH_TIMEOUT: Duration = Duration::from_secs(10);

/// What a peer's tray shows for its rank. Wire shape (camelCase) is the desktop app's contract
/// (`ui/desktop/src/peerTray.ts`, fixture `peerTray.fixture.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerTrayRecord {
    pub v: u32,
    /// The controller Mac's node name (rank 0's).
    pub controller: String,
    pub rank: usize,
    pub size: usize,
    pub model_id: Option<String>,
    /// The run's state (`RunState::as_str`).
    pub state: String,
    /// The rank's load phase while it starts (`loading` / `warming` / `ready`).
    #[serde(default)]
    pub load_phase: Option<String>,
    #[serde(default)]
    pub loaded_bytes: Option<u64>,
    #[serde(default)]
    pub planned_bytes: Option<u64>,
    /// The controller's own menu-bar title, while its app keeps it fresh.
    #[serde(default)]
    pub title: Option<String>,
    /// The controller's menu lines under that title (a benchmark's era, phase, calls).
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// The rank's pid ON THE PEER: the peer's app treats a record whose rank is gone as stopped.
    #[serde(default)]
    pub rank_pid: Option<u32>,
    pub updated_ms: u64,
    pub stale_after_s: u64,
}

impl PeerTrayRecord {
    /// The record as a peer accepts it: bounded, so a Link requester cannot write an unbounded file.
    pub fn check(&self) -> Result<()> {
        let bytes = serde_json::to_vec(self)?.len();
        ensure!(
            bytes <= RECORD_MAX_BYTES,
            "the tray status record is {bytes} bytes; a peer accepts at most {RECORD_MAX_BYTES}"
        );
        Ok(())
    }

    /// The same record with its clock removed: two of these differ only when what a peer shows does.
    fn content(&self) -> PeerTrayRecord {
        PeerTrayRecord {
            updated_ms: 0,
            ..self.clone()
        }
    }
}

/// The script a peer runs to keep `record`: written beside the file, then renamed over it, so the
/// app never reads half a record.
pub fn write_script(record: &PeerTrayRecord) -> Result<String> {
    record.check()?;
    let json = serde_json::to_string(record)?;
    Ok(format!(
        "d=\"$HOME/{TRAY_DIR}\" && /bin/mkdir -p \"$d\" && /usr/bin/printf '%s\\n' {} > \"$d/.{RANK_FILE}.$$\" && /bin/mv -f \"$d/.{RANK_FILE}.$$\" \"$d/{RANK_FILE}\"",
        sh_quote(&json)
    ))
}

/// The controller app's title file, as `controller.json` holds it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerTray {
    pub title: String,
    #[serde(default)]
    pub lines: Vec<String>,
    pub updated_ms: u64,
}

pub fn controller_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(TRAY_DIR).join(CONTROLLER_FILE))
}

/// The controller's title while its app keeps it fresh (`updatedMs` within `stale_ms` of now);
/// a missing, unreadable, empty or stale file is no title.
pub fn read_controller(path: &Path, now_ms: u64, stale_ms: u64) -> Option<ControllerTray> {
    let bytes = std::fs::read(path).ok()?;
    let tray: ControllerTray = serde_json::from_slice(&bytes).ok()?;
    let fresh = now_ms.saturating_sub(tray.updated_ms) <= stale_ms;
    (fresh && !tray.title.trim().is_empty()).then_some(tray)
}

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= TEXT_MAX {
        return text.to_string();
    }
    let mut out: String = text.chars().take(TEXT_MAX - 1).collect();
    out.push('…');
    out
}

/// One node of the run, as the relay reads it from the supervisor's status.
#[derive(Debug, Clone, PartialEq)]
pub struct RelayNode {
    pub name: String,
    pub rank: usize,
    /// `None` = this Mac (rank 0); a peer's ssh alias or `link:` host otherwise.
    pub host: Option<String>,
    pub pid: Option<u32>,
    pub load_phase: Option<String>,
    pub loaded_bytes: Option<u64>,
    pub planned_bytes: Option<u64>,
}

/// The run as the relay reads it, under the supervisor's lock.
#[derive(Debug, Clone, PartialEq)]
pub struct RelayView {
    pub state: String,
    pub model_id: Option<String>,
    pub error: Option<String>,
    pub nodes: Vec<RelayNode>,
}

/// One record per peer rank (rank ≥ 1 with a host), paired with the host it goes to.
pub fn records(
    view: &RelayView,
    controller: Option<&ControllerTray>,
    now_ms: u64,
    stale_after_s: u64,
) -> Vec<(String, PeerTrayRecord)> {
    let controller_name = view
        .nodes
        .iter()
        .find(|n| n.rank == 0)
        .map(|n| n.name.clone())
        .unwrap_or_default();
    let size = view.nodes.len();
    view.nodes
        .iter()
        .filter(|n| n.rank > 0)
        .filter_map(|n| {
            let host = n.host.clone()?;
            Some((
                host,
                PeerTrayRecord {
                    v: RECORD_VERSION,
                    controller: clip(&controller_name),
                    rank: n.rank,
                    size,
                    model_id: view.model_id.as_deref().map(clip),
                    state: view.state.clone(),
                    load_phase: n.load_phase.clone(),
                    loaded_bytes: n.loaded_bytes,
                    planned_bytes: n.planned_bytes,
                    title: controller.map(|c| clip(&c.title)),
                    lines: controller
                        .map(|c| c.lines.iter().take(LINES_MAX).map(|l| clip(l)).collect())
                        .unwrap_or_default(),
                    error: view.error.as_deref().map(clip),
                    rank_pid: n.pid,
                    updated_ms: now_ms,
                    stale_after_s,
                },
            ))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    pub enabled: bool,
    pub stale_after_s: u64,
}

impl Tuning {
    pub fn from_env() -> Tuning {
        Tuning::parse(
            std::env::var("GOOSE_DISTRIBUTED_PEER_TRAY").ok().as_deref(),
            std::env::var("GOOSE_DISTRIBUTED_PEER_TRAY_STALE_SECS")
                .ok()
                .as_deref(),
        )
    }

    pub fn parse(switch: Option<&str>, stale: Option<&str>) -> Tuning {
        let enabled = !matches!(
            switch.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
            Some("off" | "0" | "false" | "no")
        );
        let stale_after_s = stale
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_STALE_SECS)
            .clamp(STALE_RANGE.0, STALE_RANGE.1);
        Tuning {
            enabled,
            stale_after_s,
        }
    }

    /// A peer hears from the relay at least three times per staleness window.
    pub fn heartbeat(self) -> Duration {
        Duration::from_secs((self.stale_after_s / 3).max(2))
    }
}

/// Dropping it (the supervision ending, however it ends) has the relay send its last record and stop.
pub struct RelayGuard {
    _done: Option<oneshot::Sender<()>>,
}

/// The relay for one run: `read` returns the run as it stands (called on every tick, under the
/// supervisor's lock — keep it cheap). Runs until the returned guard drops.
pub fn spawn_relay(
    read: impl Fn() -> RelayView + Send + Sync + 'static,
    exec: Arc<dyn NodeExec>,
    controller: Option<PathBuf>,
    tuning: Tuning,
) -> RelayGuard {
    if !tuning.enabled {
        return RelayGuard { _done: None };
    }
    let (tx, rx) = oneshot::channel();
    tokio::spawn(relay(read, exec, controller, tuning, rx));
    RelayGuard { _done: Some(tx) }
}

struct Sent {
    content: PeerTrayRecord,
    at: tokio::time::Instant,
}

pub async fn relay(
    read: impl Fn() -> RelayView + Send + Sync + 'static,
    exec: Arc<dyn NodeExec>,
    controller: Option<PathBuf>,
    tuning: Tuning,
    mut done: oneshot::Receiver<()>,
) {
    let stale_ms = tuning.stale_after_s * 1000;
    let mut sent: std::collections::HashMap<String, Sent> = Default::default();
    let mut failing: std::collections::HashSet<String> = Default::default();
    loop {
        let finished = tokio::select! {
            _ = &mut done => true,
            _ = tokio::time::sleep(TICK) => false,
        };
        let now = super::preflight::now_ms();
        let title = controller
            .as_deref()
            .and_then(|path| read_controller(path, now, stale_ms));
        let due: Vec<(String, PeerTrayRecord)> =
            records(&read(), title.as_ref(), now, tuning.stale_after_s)
                .into_iter()
                .filter(|(host, record)| {
                    finished
                        || sent.get(host).is_none_or(|last| {
                            last.content != record.content()
                                || last.at.elapsed() >= tuning.heartbeat()
                        })
                })
                .collect();
        let results = futures::future::join_all(due.iter().map(|(host, record)| {
            let exec = Arc::clone(&exec);
            async move {
                let op = NodeOp::TrayStatus {
                    record: record.clone(),
                };
                match tokio::time::timeout(PUSH_TIMEOUT, exec.run_op(Some(host), &op)).await {
                    Ok(Ok(out)) if out.success() => Ok(()),
                    Ok(Ok(out)) => Err(format!(
                        "exit {:?}: {}",
                        out.status,
                        out.stderr.trim().chars().take(200).collect::<String>()
                    )),
                    Ok(Err(e)) => Err(format!("{e:#}")),
                    Err(_) => Err(format!("no answer in {} s", PUSH_TIMEOUT.as_secs())),
                }
            }
        }))
        .await;
        for ((host, record), result) in due.into_iter().zip(results) {
            match result {
                Ok(()) => {
                    failing.remove(&host);
                    sent.insert(
                        host,
                        Sent {
                            content: record.content(),
                            at: tokio::time::Instant::now(),
                        },
                    );
                }
                Err(e) if failing.insert(host.clone()) => warn!(
                    host = %host,
                    "distributed: the peer's tray status could not be sent ({e}); its menu bar \
                     will say the rank went quiet"
                ),
                Err(e) => debug!(host = %host, "distributed: peer tray status still failing: {e}"),
            }
        }
        if finished {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::exec::{BoxFuture, ExecOutput};
    use std::sync::Mutex;

    fn node(rank: usize, host: Option<&str>, pid: Option<u32>) -> RelayNode {
        RelayNode {
            name: if rank == 0 {
                "Work's Mac Studio".into()
            } else {
                format!("node{rank}")
            },
            rank,
            host: host.map(str::to_string),
            pid,
            load_phase: None,
            loaded_bytes: None,
            planned_bytes: None,
        }
    }

    fn view(state: &str) -> RelayView {
        RelayView {
            state: state.into(),
            model_id: Some("mlx-community/MiMo-V2.6-Flash".into()),
            error: None,
            nodes: vec![
                node(0, None, Some(10)),
                node(1, Some("workmule"), Some(7452)),
            ],
        }
    }

    fn fixture_record() -> PeerTrayRecord {
        PeerTrayRecord {
            v: 1,
            controller: "Work's Mac Studio".into(),
            rank: 1,
            size: 2,
            model_id: Some("mlx-community/MiMo-V2.6-Flash".into()),
            state: "serving".into(),
            load_phase: Some("ready".into()),
            loaded_bytes: None,
            planned_bytes: None,
            title: Some("Benchmark · MiMo-V2.6-Flash · 86/150".into()),
            lines: vec![
                "Benchmark running: Gauntlet sb-7.2".into(),
                "Calls: 86 of 150".into(),
            ],
            error: None,
            rank_pid: Some(7452),
            updated_ms: 1_761_900_000_000,
            stale_after_s: 30,
        }
    }

    /// The desktop app's reader parses the same fixture (peerTray.test.ts): the wire shape is one
    /// contract on both sides.
    #[test]
    fn the_record_matches_the_desktop_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../ui/desktop/src/peerTray.fixture.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(fixture_record()).unwrap(), fixture);
    }

    #[test]
    fn records_go_to_peer_ranks_only_with_the_controller_title() {
        let title = ControllerTray {
            title: "Benchmark · MiMo-V2.6-Flash · 86/150".into(),
            lines: vec!["Calls: 86 of 150".into()],
            updated_ms: 5,
        };
        let out = records(&view("serving"), Some(&title), 1000, 30);
        assert_eq!(out.len(), 1);
        let (host, record) = &out[0];
        assert_eq!(host, "workmule");
        assert_eq!(record.rank, 1);
        assert_eq!(record.size, 2);
        assert_eq!(record.controller, "Work's Mac Studio");
        assert_eq!(
            record.title.as_deref(),
            Some("Benchmark · MiMo-V2.6-Flash · 86/150")
        );
        assert_eq!(record.lines, vec!["Calls: 86 of 150".to_string()]);
        assert_eq!(record.rank_pid, Some(7452));
        assert_eq!(record.updated_ms, 1000);
        // No title: the peer composes its own from the run's facts.
        assert_eq!(records(&view("starting"), None, 1, 30)[0].1.title, None);
    }

    #[test]
    fn the_controller_title_is_believed_only_while_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONTROLLER_FILE);
        assert_eq!(read_controller(&path, 0, 1000), None);
        std::fs::write(
            &path,
            r#"{"title":"Benchmark · x · 3/150","updatedMs":10000}"#,
        )
        .unwrap();
        assert!(read_controller(&path, 10_500, 1000).is_some());
        assert_eq!(read_controller(&path, 11_001, 1000), None, "stale");
        std::fs::write(&path, r#"{"title":"  ","updatedMs":10000}"#).unwrap();
        assert_eq!(read_controller(&path, 10_000, 1000), None, "empty title");
        std::fs::write(&path, "{half").unwrap();
        assert_eq!(read_controller(&path, 10_000, 1000), None, "unreadable");
    }

    #[test]
    fn tuning_reads_the_switch_and_clamps_the_window() {
        assert_eq!(
            Tuning::parse(None, None),
            Tuning {
                enabled: true,
                stale_after_s: 30
            }
        );
        assert!(!Tuning::parse(Some("OFF"), None).enabled);
        assert_eq!(Tuning::parse(None, Some("1")).stale_after_s, 6);
        assert_eq!(Tuning::parse(None, Some("99999")).stale_after_s, 3600);
        assert_eq!(Tuning::parse(None, Some("junk")).stale_after_s, 30);
        assert_eq!(
            Tuning::parse(None, Some("30")).heartbeat(),
            Duration::from_secs(10)
        );
        assert_eq!(
            Tuning::parse(None, Some("6")).heartbeat(),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn an_oversized_record_is_refused() {
        let mut record = fixture_record();
        record.lines = vec!["x".repeat(TEXT_MAX); 200];
        assert!(record.check().is_err());
        assert!(write_script(&record).is_err());
        // records() clips what it builds, so its own output always fits.
        let title = ControllerTray {
            title: "t".repeat(10_000),
            lines: vec!["l".repeat(10_000); 100],
            updated_ms: 0,
        };
        for (_, r) in records(&view("serving"), Some(&title), 0, 30) {
            r.check().unwrap();
            assert_eq!(r.lines.len(), LINES_MAX);
        }
    }

    /// Two fake hosts, each a HOME of its own: the op's real script runs under `/bin/sh` there, so
    /// the file each peer's app would read is the one this test reads back.
    struct FakeHosts {
        homes: std::collections::HashMap<String, PathBuf>,
        ops: Mutex<Vec<(String, String)>>,
    }

    impl NodeExec for FakeHosts {
        fn run<'a>(
            &'a self,
            host: Option<&'a str>,
            script: &'a str,
        ) -> BoxFuture<'a, Result<ExecOutput>> {
            Box::pin(async move {
                let host = host.expect("the relay never writes to this Mac");
                self.ops
                    .lock()
                    .unwrap()
                    .push((host.to_string(), script.to_string()));
                let home = self.homes.get(host).expect("a known fake host");
                let out = tokio::process::Command::new("/bin/sh")
                    .arg("-c")
                    .arg(script)
                    .env("HOME", home)
                    .output()
                    .await?;
                Ok(ExecOutput {
                    status: out.status.code(),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                })
            })
        }
    }

    fn read_rank_file(home: &Path) -> PeerTrayRecord {
        serde_json::from_slice(&std::fs::read(home.join(TRAY_DIR).join(RANK_FILE)).unwrap())
            .unwrap()
    }

    #[tokio::test]
    async fn the_relay_writes_every_peer_and_its_last_record_on_the_end() {
        let root = tempfile::tempdir().unwrap();
        let (a, b, controller_home) = (
            root.path().join("mule"),
            root.path().join("gabee"),
            root.path().join("controller"),
        );
        for dir in [&a, &b, &controller_home] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let exec = Arc::new(FakeHosts {
            homes: [
                ("mule".to_string(), a.clone()),
                ("gabee".to_string(), b.clone()),
            ]
            .into_iter()
            .collect(),
            ops: Mutex::new(Vec::new()),
        });
        let controller = controller_home.join(TRAY_DIR).join(CONTROLLER_FILE);
        std::fs::create_dir_all(controller.parent().unwrap()).unwrap();
        std::fs::write(
            &controller,
            serde_json::json!({
                "title": "Benchmark · MiMo-V2.6-Flash · 86/150",
                "lines": ["Calls: 86 of 150"],
                "updatedMs": crate::distributed::preflight::now_ms(),
            })
            .to_string(),
        )
        .unwrap();
        let state = Arc::new(Mutex::new("serving".to_string()));
        let read = {
            let state = Arc::clone(&state);
            move || RelayView {
                state: state.lock().unwrap().clone(),
                model_id: Some("MiMo-V2.6-Flash".into()),
                error: None,
                nodes: vec![
                    node(0, None, Some(1)),
                    node(1, Some("mule"), Some(2)),
                    node(2, Some("gabee"), Some(3)),
                ],
            }
        };
        let tuning = Tuning {
            enabled: true,
            stale_after_s: 30,
        };
        let guard = spawn_relay(read, exec.clone(), Some(controller), tuning);
        tokio::time::sleep(Duration::from_millis(2600)).await;
        for (home, rank) in [(&a, 1), (&b, 2)] {
            let record = read_rank_file(home);
            assert_eq!(record.rank, rank);
            assert_eq!(record.size, 3);
            assert_eq!(record.state, "serving");
            assert_eq!(
                record.title.as_deref(),
                Some("Benchmark · MiMo-V2.6-Flash · 86/150")
            );
            // No temp file is left beside the record.
            let names: Vec<_> = std::fs::read_dir(home.join(TRAY_DIR))
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(names, vec![std::ffi::OsString::from(RANK_FILE)]);
        }
        let pushes = exec.ops.lock().unwrap().len();
        assert_eq!(pushes, 2, "one push per peer, none to this Mac");
        // Unchanged within the heartbeat: nothing is resent.
        tokio::time::sleep(Duration::from_millis(2100)).await;
        assert_eq!(exec.ops.lock().unwrap().len(), 2);
        // The run ends: its last record reaches both peers at once.
        *state.lock().unwrap() = "stopped".into();
        drop(guard);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(read_rank_file(&a).state, "stopped");
        assert_eq!(read_rank_file(&b).state, "stopped");
        assert_eq!(exec.ops.lock().unwrap().len(), 4);
    }

    #[test]
    fn the_script_survives_quotes_in_the_title() {
        let mut record = fixture_record();
        record.controller = "Mihai's MacBook".into();
        let script = write_script(&record).unwrap();
        assert!(script.contains(r"Mihai'\''s MacBook"));
    }
}
