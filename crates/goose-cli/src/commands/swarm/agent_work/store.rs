//! The agent's runtime files under `<agent dir>/.swarm/agent/` — everything a desktop reader
//! or a later tick needs, on disk, rewritten whole or appended, never held in memory only.
//!
//! Borrowed from the build engine's ledger: every unit of delivered work is a MINI
//! (`ledger/<name>.json`), and `ledger.json` is rebuilt WHOLE from the minis on every write, so
//! writers are order-independent and idempotent and the roll-up can never drift from its parts.
//! `render_ledger_block` is the SNOWBALL — what the orchestrating node reads at the start of a
//! tick: the newest facts, the open asks, the staged and posted drafts, the last tick's summary.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const RUNTIME_SUBDIR: &str = ".swarm/agent";

#[derive(Debug, Clone)]
pub struct RuntimeDir {
    pub agent_dir: PathBuf,
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeskState {
    /// idle | waiting | holding | ticking | paused | stopped
    pub status: String,
    pub agent: String,
    pub title: String,
    pub pid: Option<u32>,
    pub run_id: String,
    pub tick: u64,
    pub phase: String,
    pub phase_started_at: Option<String>,
    pub next_tick_at: Option<String>,
    pub next_tick_reason: String,
    pub next_tick_local: String,
    pub cadence: String,
    pub timezone: String,
    pub window_open: bool,
    pub last_tick: Option<TickSummary>,
    pub lanes_planned: u32,
    pub lanes_done: u32,
    pub hold_reason: Option<String>,
    pub decisions_applied: u64,
    pub planner_model: String,
    pub devices: Vec<DeviceSlot>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceSlot {
    pub id: String,
    pub model_id: String,
    pub weight: u32,
    pub supervision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TickSummary {
    pub tick: u64,
    pub started_at: String,
    pub ended_at: String,
    /// done | held | failed
    pub outcome: String,
    pub summary: String,
    pub lanes: u32,
    pub staged: u32,
    pub posted: u32,
    pub asks: u32,
    pub lane_secs: f64,
    pub wall_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedRow {
    pub id: String,
    pub tick: u64,
    pub lane: String,
    pub surgeon: String,
    pub target: String,
    pub kind: String,
    pub body: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    /// staged | approved | declined | posted | failed | done
    pub status: String,
    pub staged_at: String,
    #[serde(default)]
    pub decided_at: Option<String>,
    #[serde(default)]
    pub decided_note: Option<String>,
    #[serde(default)]
    pub posted_tick: Option<u64>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub review: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRow {
    pub id: String,
    pub tick: u64,
    pub question: String,
    #[serde(default)]
    pub why: String,
    /// open | answered | dismissed
    pub status: String,
    pub raised_at: String,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub answered_at: Option<String>,
    /// The tick that read the answer (so the orchestrator sees it exactly once as NEW).
    #[serde(default)]
    pub consumed_tick: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    #[serde(default)]
    pub ts: String,
    pub id: String,
    /// approve | decline | done | reply | dismiss
    pub decision: String,
    #[serde(default)]
    pub text: String,
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

impl RuntimeDir {
    pub fn new(agent_dir: &Path) -> Self {
        Self {
            agent_dir: agent_dir.to_path_buf(),
            root: agent_dir.join(RUNTIME_SUBDIR),
        }
    }

    pub fn ensure(&self) -> Result<()> {
        for sub in ["", "ledger", "inbox", "inbox/consumed", "ticks", "drafts"] {
            std::fs::create_dir_all(self.root.join(sub))?;
        }
        // The desk commits its memory (ledger, ticks, drafts, asks, decisions) and not the churn
        // (activity digests, the event log, the heartbeat, flags, the state mirror).
        let ignore = self.agent_dir.join(".swarm").join(".gitignore");
        if !ignore.exists() {
            let _ = std::fs::write(
                &ignore,
                "activity/\nagent/run.jsonl\nagent/heartbeat\nagent/lock\nagent/engine.log\nagent/state.json\nagent/paused\nagent/tick-now\nagent/stop\nagent/*.tmp\nagent/inbox/\n",
            );
        }
        Ok(())
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }
    pub fn events_path(&self) -> PathBuf {
        self.root.join("run.jsonl")
    }
    pub fn heartbeat_path(&self) -> PathBuf {
        self.root.join("heartbeat")
    }
    pub fn lock_path(&self) -> PathBuf {
        self.root.join("lock")
    }
    pub fn flag(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    pub fn prepared_path(&self) -> PathBuf {
        self.root.join("prepared.json")
    }
    pub fn asks_path(&self) -> PathBuf {
        self.root.join("asks.json")
    }
    pub fn decisions_path(&self) -> PathBuf {
        self.root.join("decisions.jsonl")
    }
    pub fn tick_path(&self, n: u64) -> PathBuf {
        self.root.join("ticks").join(format!("{n}.json"))
    }
    pub fn draft_path(&self, id: &str) -> PathBuf {
        self.root.join("drafts").join(format!("{id}.md"))
    }

    pub fn touch_heartbeat(&self) {
        let _ = std::fs::write(self.heartbeat_path(), now_rfc3339());
    }

    pub fn write_state(&self, st: &mut DeskState) {
        st.updated_at = now_rfc3339();
        if let Ok(text) = serde_json::to_string_pretty(st) {
            write_atomic(&self.state_path(), text.as_bytes());
        }
    }

    /// `Ok(None)` = no state yet (a fresh desk); `Err` = a state file exists and cannot be read —
    /// the caller names that instead of silently starting from zero.
    pub fn read_state(&self) -> Result<Option<DeskState>, String> {
        read_json_file(&self.state_path())
    }

    // ---- flags the desktop sets by touching a file ----

    pub fn is_paused(&self) -> bool {
        self.flag("paused").exists()
    }
    pub fn stop_requested(&self) -> bool {
        self.flag("stop").exists()
    }
    /// Consumes the flag.
    pub fn take_tick_now(&self) -> bool {
        let f = self.flag("tick-now");
        if f.exists() {
            let _ = std::fs::remove_file(f);
            true
        } else {
            false
        }
    }
    pub fn clear_stop(&self) {
        let _ = std::fs::remove_file(self.flag("stop"));
    }

    // ---- the human's notes: <root>/inbox/*.json {text}, consumed once per tick ----

    /// The notes, plus the inbox directory's own read error when it has one (named, never an
    /// empty inbox impersonating a readable one).
    pub fn take_inbox(&self) -> (Vec<String>, Option<String>) {
        let dir = self.root.join("inbox");
        let mut names: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
                .collect(),
            Err(e) => return (Vec::new(), Some(format!("{}: {e}", dir.display()))),
        };
        names.sort();
        let mut out = Vec::new();
        for p in names {
            if let Ok(t) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
                        out.push(s.to_string());
                    }
                }
            }
            if let Some(name) = p.file_name() {
                let _ = std::fs::rename(&p, dir.join("consumed").join(name));
            }
        }
        (out, None)
    }

    // ---- staged drafts and asks: whole-file JSON arrays ----

    /// `Err` = the file exists and does not parse. A caller that would WRITE the file must stop
    /// on Err: rewriting from an empty read would erase every staged draft.
    pub fn prepared(&self) -> Result<Vec<PreparedRow>, String> {
        read_json_array(&self.prepared_path())
    }
    pub fn write_prepared(&self, rows: &[PreparedRow]) {
        write_json_array(&self.prepared_path(), rows);
    }
    pub fn asks(&self) -> Result<Vec<AskRow>, String> {
        read_json_array(&self.asks_path())
    }
    pub fn write_asks(&self, rows: &[AskRow]) {
        write_json_array(&self.asks_path(), rows);
    }

    /// Every decision the human wrote, in order (the desktop appends; the engine folds).
    pub fn decisions(&self) -> Vec<Decision> {
        let Ok(t) = std::fs::read_to_string(self.decisions_path()) else {
            return Vec::new();
        };
        t.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Apply the decisions past `applied` to the prepared rows and asks. Returns how many were
    /// applied and the rows touched (for the events).
    pub fn fold_decisions(&self, applied: u64) -> Result<(u64, Vec<Value>), String> {
        let all = self.decisions();
        let mut prepared = self.prepared()?;
        let mut asks = self.asks()?;
        let mut touched = Vec::new();
        let now = now_rfc3339();
        for d in all.iter().skip(applied as usize) {
            let mut hit = false;
            if let Some(row) = prepared.iter_mut().find(|r| r.id == d.id) {
                hit = true;
                match d.decision.as_str() {
                    "approve" => row.status = "approved".into(),
                    "decline" => row.status = "declined".into(),
                    "done" => row.status = "done".into(),
                    "reply" => {}
                    other => {
                        touched.push(json!({"id": d.id, "unknown_decision": other}));
                        continue;
                    }
                }
                row.decided_at = Some(now.clone());
                if !d.text.trim().is_empty() {
                    row.decided_note = Some(d.text.trim().to_string());
                }
                touched.push(json!({"draft": d.id, "status": row.status}));
            }
            if let Some(ask) = asks.iter_mut().find(|a| a.id == d.id) {
                hit = true;
                match d.decision.as_str() {
                    "reply" => {
                        ask.status = "answered".into();
                        ask.answer = Some(d.text.trim().to_string());
                        ask.answered_at = Some(now.clone());
                    }
                    "dismiss" | "decline" => {
                        ask.status = "dismissed".into();
                        ask.answered_at = Some(now.clone());
                    }
                    _ => {}
                }
                touched.push(json!({"ask": d.id, "status": ask.status}));
            }
            if !hit {
                touched.push(json!({"id": d.id, "unmatched": true}));
            }
        }
        self.write_prepared(&prepared);
        self.write_asks(&asks);
        Ok((all.len() as u64, touched))
    }

    // ---- the ledger: minis + the whole-rebuilt roll-up ----

    pub fn write_mini(&self, name: &str, row: &Value) -> Result<PathBuf> {
        let dir = self.root.join("ledger");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", sanitize_name(name)));
        let bytes = serde_json::to_string_pretty(row)?;
        if !std::fs::read_to_string(&path).is_ok_and(|old| old == bytes) {
            std::fs::write(&path, bytes)?;
        }
        self.rebuild_rollup();
        Ok(path)
    }

    /// `ledger.json`, rebuilt whole from every mini. Unreadable minis are NAMED in `dropped`.
    pub fn rebuild_rollup(&self) -> Value {
        let dir = self.root.join("ledger");
        let mut rows: Vec<Value> = Vec::new();
        let mut dropped: Vec<Value> = Vec::new();
        let mut names: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
            Err(e) => {
                dropped.push(json!({"file": dir.display().to_string(), "error": e.to_string()}));
                Vec::new()
            }
        };
        names.sort();
        for p in names {
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let fname = p.display().to_string();
            match std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
            {
                Ok(v) => rows.push(v),
                Err(e) => dropped.push(json!({"file": fname, "error": e})),
            }
        }
        let mut by_kind: std::collections::BTreeMap<String, Vec<Value>> = Default::default();
        for r in rows {
            let k = r
                .get("kind")
                .and_then(|k| k.as_str())
                .unwrap_or("other")
                .to_string();
            by_kind.entry(k).or_default().push(r);
        }
        for v in by_kind.values_mut() {
            v.sort_by_key(|r| {
                (
                    r.get("tick").and_then(|t| t.as_u64()).unwrap_or(0),
                    r.get("at")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string(),
                )
            });
        }
        let rollup = json!({
            "rebuilt_at": now_rfc3339(),
            "counts": by_kind.iter().map(|(k, v)| (k.clone(), json!(v.len()))).collect::<serde_json::Map<_, _>>(),
            "kinds": by_kind,
            "dropped": dropped,
        });
        if let Ok(text) = serde_json::to_string_pretty(&rollup) {
            write_atomic(&self.root.join("ledger.json"), text.as_bytes());
        }
        rollup
    }

    pub fn rollup(&self) -> Value {
        std::fs::read_to_string(self.root.join("ledger.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| self.rebuild_rollup())
    }

    /// THE SNOWBALL BLOCK the orchestrator reads at the start of every tick: the newest facts
    /// (from every tick so far), the open asks and their answers, staged / posted drafts, and the
    /// previous tick's summary. `newest` bounds each list by COUNT (the rest is on disk and the
    /// orchestrator is told how many were left out).
    pub fn render_ledger_block(&self, newest: usize) -> String {
        let rollup = self.rollup();
        let kinds = rollup.get("kinds").cloned().unwrap_or(json!({}));
        let mut out = String::new();
        let take = |k: &str| -> (Vec<Value>, usize) {
            // An absent kind is "no rows of that kind yet" — the roll-up lists every kind it read.
            let all: Vec<Value> = match kinds.get(k).and_then(|v| v.as_array()) {
                Some(rows) => rows.clone(),
                None => Vec::new(),
            };
            let total = all.len();
            let start = total.saturating_sub(newest);
            (all[start..].to_vec(), total)
        };

        let (ticks, total_ticks) = take("tick");
        if let Some(last) = ticks.last() {
            out.push_str(&format!(
                "PREVIOUS TICK (#{} of {} so far): {}\n",
                last.get("tick").and_then(|t| t.as_u64()).unwrap_or(0),
                total_ticks,
                last.get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or("(no summary)")
            ));
            if let Some(h) = last.get("handoff").and_then(|s| s.as_str()) {
                if !h.trim().is_empty() {
                    out.push_str(&format!("  handoff: {}\n", h.trim()));
                }
            }
        } else {
            out.push_str("PREVIOUS TICK: none — this is the first tick of this desk.\n");
        }

        let (facts, total_facts) = take("fact");
        out.push_str(&format!(
            "\nFACTS ON THE LEDGER (newest {} of {}):\n",
            facts.len(),
            total_facts
        ));
        if facts.is_empty() {
            out.push_str("  (none recorded yet)\n");
        }
        for f in &facts {
            out.push_str(&format!(
                "  t{} {}\n",
                f.get("tick").and_then(|t| t.as_u64()).unwrap_or(0),
                f.get("fact").and_then(|s| s.as_str()).unwrap_or("")
            ));
        }

        let asks = match self.asks() {
            Ok(a) => a,
            Err(e) => {
                out.push_str(&format!(
                    "\nASKS TO THE HUMAN: the asks file could not be read ({e})\n"
                ));
                Vec::new()
            }
        };
        let open: Vec<&AskRow> = asks.iter().filter(|a| a.status == "open").collect();
        let answered: Vec<&AskRow> = asks
            .iter()
            .filter(|a| a.status == "answered" && a.consumed_tick.is_none())
            .collect();
        out.push_str(&format!(
            "\nASKS TO THE HUMAN: {} open, {} newly answered\n",
            open.len(),
            answered.len()
        ));
        for a in &open {
            out.push_str(&format!("  OPEN {} (t{}): {}\n", a.id, a.tick, a.question));
        }
        for a in &answered {
            out.push_str(&format!(
                "  ANSWERED {} — Q: {} — A: {}\n",
                a.id,
                a.question,
                a.answer.as_deref().unwrap_or("")
            ));
        }

        let prepared = match self.prepared() {
            Ok(p) => p,
            Err(e) => {
                out.push_str(&format!(
                    "\nSTAGED DRAFTS: the prepared file could not be read ({e})\n"
                ));
                Vec::new()
            }
        };
        let by = |s: &str| prepared.iter().filter(|r| r.status == s).count();
        out.push_str(&format!(
            "\nSTAGED DRAFTS: {} staged (awaiting the next tick / approval), {} approved, {} posted, {} declined, {} failed\n",
            by("staged"),
            by("approved"),
            by("posted"),
            by("declined"),
            by("failed")
        ));
        for r in prepared
            .iter()
            .filter(|r| matches!(r.status.as_str(), "staged" | "approved" | "failed"))
        {
            out.push_str(&format!(
                "  {} [{}] {} → {} ({} chars){}\n",
                r.id,
                r.status,
                r.kind,
                r.target,
                r.body.chars().count(),
                r.result.as_deref().map_or(String::new(), |x| format!(
                    " result: {}",
                    tail_chars(x, 200)
                ))
            ));
        }
        let (posted, total_posted) = take("posted");
        if total_posted > 0 {
            out.push_str(&format!(
                "\nPOSTED (newest {} of {}):\n",
                posted.len(),
                total_posted
            ));
            for p in &posted {
                out.push_str(&format!(
                    "  t{} {} → {}\n",
                    p.get("tick").and_then(|t| t.as_u64()).unwrap_or(0),
                    p.get("id").and_then(|s| s.as_str()).unwrap_or(""),
                    p.get("target").and_then(|s| s.as_str()).unwrap_or("")
                ));
            }
        }
        if let Some(d) = rollup.get("dropped").and_then(|d| d.as_array()) {
            if !d.is_empty() {
                out.push_str(&format!(
                    "\n{} ledger rows could not be read and are NOT in this block: {}\n",
                    d.len(),
                    d.iter()
                        .filter_map(|x| x.get("file").and_then(|f| f.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        out
    }

    pub fn mark_asks_consumed(&self, tick: u64) {
        let Ok(mut asks) = self.asks() else {
            return;
        };
        for a in asks.iter_mut() {
            if a.status == "answered" && a.consumed_tick.is_none() {
                a.consumed_tick = Some(tick);
            }
        }
        self.write_asks(&asks);
    }

    pub fn write_tick_record(&self, n: u64, record: &Value) {
        if let Ok(text) = serde_json::to_string_pretty(record) {
            write_atomic(&self.tick_path(n), text.as_bytes());
        }
    }

    // ---- the operator-visible files in the agent dir ----

    pub fn append_line(&self, rel: &str, line: &str) -> Result<()> {
        use std::io::Write;
        let p = self.agent_dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|e| anyhow!("opening {}: {e}", p.display()))?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    /// `Ok(None)` = the file does not exist yet; `Err` = it exists and cannot be read.
    pub fn read_text(&self, rel: &str) -> Result<Option<String>, String> {
        let p = self.agent_dir.join(rel);
        match std::fs::read_to_string(&p) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", p.display())),
        }
    }

    pub fn write_text(&self, rel: &str, text: &str) -> Result<()> {
        let p = self.agent_dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(&p, text.as_bytes());
        Ok(())
    }

    /// pid in the lock file when a live process holds it.
    pub fn lock_holder(&self) -> Option<u32> {
        let pid: u32 = std::fs::read_to_string(self.lock_path())
            .ok()?
            .trim()
            .parse()
            .ok()?;
        if pid_alive(pid) {
            Some(pid)
        } else {
            None
        }
    }

    pub fn take_lock(&self) -> Result<()> {
        if let Some(pid) = self.lock_holder() {
            return Err(anyhow!(
                "another `goose swarm agent run` (pid {pid}) holds {}",
                self.lock_path().display()
            ));
        }
        std::fs::write(self.lock_path(), std::process::id().to_string())?;
        Ok(())
    }

    pub fn release_lock(&self) {
        if std::fs::read_to_string(self.lock_path())
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            == Some(std::process::id())
        {
            let _ = std::fs::remove_file(self.lock_path());
        }
    }
}

pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Missing = `Ok(None)` (the honest empty: nothing was ever written); any other failure is named.
fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

// Spelled as a match on purpose: the fallback gate ratchets the default-on-absence call in the run
// path (development_gates::run_path_silent_empty_fallbacks_only_shrink) and wants each honest empty named.
#[allow(clippy::manual_unwrap_or_default)]
fn read_json_array<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    // A file never written is an empty list — the one honest empty here; a broken file is the Err.
    Ok(match read_json_file::<Vec<T>>(path)? {
        Some(rows) => rows,
        None => Vec::new(),
    })
}

fn write_json_array<T: Serialize>(path: &Path, rows: &[T]) {
    if let Ok(text) = serde_json::to_string_pretty(rows) {
        write_atomic(path, text.as_bytes());
    }
}

pub fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn tail_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let skip = count - n;
    format!("…{}", s.chars().skip(skip).collect::<String>())
}

pub fn head_chars(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    format!("{}…", s.chars().take(n).collect::<String>())
}

pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> (tempfile::TempDir, RuntimeDir) {
        let d = tempfile::tempdir().unwrap();
        let r = RuntimeDir::new(d.path());
        r.ensure().unwrap();
        (d, r)
    }

    #[test]
    fn minis_roll_up_whole_and_a_bad_mini_is_named() {
        let (_d, r) = rt();
        r.write_mini("t1-fact-1", &json!({"kind":"fact","tick":1,"fact":"A"}))
            .unwrap();
        r.write_mini("t2-fact-1", &json!({"kind":"fact","tick":2,"fact":"B"}))
            .unwrap();
        std::fs::write(r.root.join("ledger").join("bad.json"), "{nope").unwrap();
        let roll = r.rebuild_rollup();
        assert_eq!(roll["counts"]["fact"], 2);
        assert!(roll["dropped"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("bad.json"));
        let block = r.render_ledger_block(5);
        assert!(block.contains("t1 A"));
        assert!(block.contains("t2 B"));
        assert!(block.contains("could not be read"));
    }

    #[test]
    fn decisions_fold_onto_drafts_and_asks_exactly_once() {
        let (_d, r) = rt();
        r.write_prepared(&[PreparedRow {
            id: "d1".into(),
            tick: 1,
            lane: "l".into(),
            surgeon: "s".into(),
            target: "ATL-1".into(),
            kind: "comment".into(),
            body: "hi".into(),
            evidence: vec![],
            status: "staged".into(),
            staged_at: now_rfc3339(),
            decided_at: None,
            decided_note: None,
            posted_tick: None,
            result: None,
            review: vec![],
        }]);
        r.write_asks(&[AskRow {
            id: "a1".into(),
            tick: 1,
            question: "which?".into(),
            why: "".into(),
            status: "open".into(),
            raised_at: now_rfc3339(),
            answer: None,
            answered_at: None,
            consumed_tick: None,
        }]);
        std::fs::write(
            r.decisions_path(),
            "{\"id\":\"d1\",\"decision\":\"approve\"}\n{\"id\":\"a1\",\"decision\":\"reply\",\"text\":\"the second\"}\n",
        )
        .unwrap();
        let (n, touched) = r.fold_decisions(0).unwrap();
        assert_eq!(n, 2);
        assert_eq!(touched.len(), 2);
        assert_eq!(r.prepared().unwrap()[0].status, "approved");
        assert_eq!(r.asks().unwrap()[0].answer.as_deref(), Some("the second"));
        let (n2, touched2) = r.fold_decisions(n).unwrap();
        assert_eq!(n2, 2);
        assert!(touched2.is_empty());
        let block = r.render_ledger_block(5);
        assert!(block.contains("ANSWERED a1"));
        r.mark_asks_consumed(2);
        assert!(!r.render_ledger_block(5).contains("ANSWERED a1"));
    }

    #[test]
    fn a_corrupt_prepared_file_is_named_and_never_rewritten_empty() {
        let (_d, r) = rt();
        std::fs::write(r.prepared_path(), "{not json").unwrap();
        let err = r.prepared().unwrap_err();
        assert!(err.contains("prepared.json"));
        assert!(r.fold_decisions(0).is_err());
        assert_eq!(
            std::fs::read_to_string(r.prepared_path()).unwrap(),
            "{not json"
        );
        assert!(r
            .render_ledger_block(5)
            .contains("prepared file could not be read"));
        assert!(r.read_state().unwrap().is_none());
    }

    #[test]
    fn inbox_notes_are_consumed_once() {
        let (_d, r) = rt();
        std::fs::write(
            r.root.join("inbox").join("1.json"),
            "{\"text\":\"hold ATL-9\"}",
        )
        .unwrap();
        assert_eq!(r.take_inbox().0, vec!["hold ATL-9".to_string()]);
        assert!(r.take_inbox().0.is_empty());
        assert!(r
            .root
            .join("inbox")
            .join("consumed")
            .join("1.json")
            .exists());
    }

    #[test]
    fn the_lock_names_a_live_holder_and_ignores_a_dead_one() {
        let (_d, r) = rt();
        std::fs::write(r.lock_path(), "999999").unwrap();
        assert!(r.lock_holder().is_none());
        r.take_lock().unwrap();
        assert_eq!(r.lock_holder(), Some(std::process::id()));
        assert!(r.take_lock().is_err());
        r.release_lock();
        assert!(!r.lock_path().exists());
    }
}
