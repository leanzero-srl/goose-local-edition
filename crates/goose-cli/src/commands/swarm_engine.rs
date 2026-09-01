//! The mechanical engine boundary for the swarm's model-hosting runtime.
//!
//! `SwarmEngine` is the seam a second local engine (an MLX sidecar) plugs into NEXT TO LM Studio.
//! Step A moved the existing LM Studio free functions here verbatim from swarm.rs, fronted by one
//! trait object. Step B (multi-engine generalization) added `EngineKind`, the `Engines` registry,
//! and the per-engine partition of the proven-negative pool semantics. Step C registers a real
//! `SidecarEngine` (goose-sidecar's supervised Rapid-MLX process, dispatched through the
//! declarative `omlx` provider) — constructed ONLY when the config declares `mlx_engine` settings
//! AND a pool device is tagged for it, so an untagged pool stays byte-identical.

use anyhow::{bail, Result};
use goose_sidecar::engine::{EngineSettings, MlxEngineManager};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::process::Command as ProcCommand;
use std::sync::Arc;

use console::style;

use super::swarm::{gen_entry_id, SwarmConfig, SwarmDevice};

/// One resident/served model row as an engine's probe reports it — the exchange type every
/// catalog probe returns and the pool builder consumes.
#[derive(Clone, Debug, PartialEq)]
pub struct LmsProcess {
    pub(super) identifier: String,
    pub(super) status: String,
    pub(super) device: Option<String>,
    /// LM Studio's PARALLEL column — how many requests this model instance serves at once. The swarm
    /// uses it as the device weight so dispatch concurrency tracks the user's LM Studio concurrency.
    pub(super) parallel: Option<u32>,
    /// Loaded context length as the engine's catalog reports it (LM Studio `/api/v0/models`
    /// loaded_context_length; rapid-mlx `/v1/models` context_window). None when the source has
    /// no such figure (`lms ps` has no column for it) — absent, never invented.
    pub(super) loaded_context_length: Option<u64>,
}

/// Parse `lms ps` output (a plain whitespace-aligned table). Splits data rows on runs of >=2 spaces
/// (so "29.53 GB" stays one field) and reads DEVICE by its header column index. Errs if no header.
pub(super) fn parse_lms_ps(raw: &str) -> Result<Vec<LmsProcess>> {
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let gap = regex::Regex::new(r"\s{2,}").unwrap();
    let clean = ansi.replace_all(raw, "");
    let lines: Vec<&str> = clean.lines().collect();
    let header = lines
        .iter()
        .position(|l| l.contains("IDENTIFIER") && l.contains("DEVICE"))
        .ok_or_else(|| anyhow::anyhow!("lms ps: header (IDENTIFIER/DEVICE) not found"))?;
    let cols: Vec<&str> = lines[header].split_whitespace().collect();
    let device_idx = cols.iter().position(|c| *c == "DEVICE").unwrap_or(6);
    let status_idx = cols.iter().position(|c| *c == "STATUS").unwrap_or(2);
    let parallel_idx = cols.iter().position(|c| *c == "PARALLEL");
    let mut out = Vec::new();
    for line in &lines[header + 1..] {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<String> = gap
            .split(line.trim())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if f.is_empty() {
            continue;
        }
        out.push(LmsProcess {
            identifier: f[0].clone(),
            status: f.get(status_idx).cloned().unwrap_or_default(),
            device: f.get(device_idx).cloned().filter(|s| !s.is_empty()),
            parallel: parallel_idx
                .and_then(|i| f.get(i))
                .and_then(|s| s.parse::<u32>().ok()),
            loaded_context_length: None,
        });
    }
    Ok(out)
}

/// The engine-specific surface of the run pipeline: host resolution, residency/servability
/// probes, and JIT warm-up. Everything engine-neutral (pool building, dispatch, judging) stays
/// in swarm.rs.
pub trait SwarmEngine: Send + Sync {
    fn provider_name(&self) -> &'static str;
    /// The engine's HTTP host (base URL) for catalog probes.
    fn http_host(&self) -> String;
    /// Resident-model state straight from the engine's native catalog endpoint.
    fn catalog_probe(&self) -> Vec<LmsProcess>;
    /// The model ids the endpoint will actually SERVE. `None` means the probe itself failed —
    /// NOT "no models" — and callers must never gate on it (see `endpoint_model_ids` below).
    fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>>;
    /// Currently-loaded instance count for a model across the fleet.
    fn loaded_instance_count(&self, model_id: &str) -> usize;
    /// JIT warm-up: ensure up to `instances` copies are loaded, never more than already present.
    fn ensure_loaded(&self, model_id: &str, instances: u32);
    /// Resident-model state through the engine's own probe chain (for LM Studio: `lms ps`
    /// primary — richest, carries DEVICE + PARALLEL — with the native HTTP catalog as fallback).
    fn resident_processes(&self) -> Result<Vec<LmsProcess>>;
    /// Human-readable fleet probe printed to the console (`swarm pool probe`).
    fn probe_report(&self);
}

/// Which LOCAL engine hosts a device's model. Both values are local engines — cloud devices are a
/// separate axis (`SwarmDevice.provider`) and never consult this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineKind {
    #[serde(rename = "lmstudio")]
    LmStudio,
    #[serde(rename = "mlx-sidecar")]
    MlxSidecar,
}

/// A device's engine KIND. `None` MEANS LM Studio by definition (the serde default that keeps
/// every existing config byte-identical), so this `unwrap_or` is definitional, not a fallback.
pub(super) fn device_engine_kind(d: &SwarmDevice) -> EngineKind {
    d.engine.unwrap_or(EngineKind::LmStudio)
}

/// The runtime's engine registry: the global LM Studio engine (fronting the whole LM Link pool)
/// plus zero-or-more named sidecar engines (`engines_for_run` registers the MLX sidecar when the
/// config + pool demand it). Constructed once per run and threaded through the same path the
/// single step-A engine object took (run_swarm -> DispatcherRecipe -> GooseAgentDispatcher).
pub struct Engines {
    lmstudio: Arc<dyn SwarmEngine>,
    sidecars: BTreeMap<String, Arc<dyn SwarmEngine>>,
}

impl Engines {
    pub fn new() -> Self {
        Self {
            lmstudio: default_engine(),
            sidecars: BTreeMap::new(),
        }
    }

    pub fn register_sidecar(&mut self, name: &str, engine: Arc<dyn SwarmEngine>) {
        self.sidecars.insert(name.to_string(), engine);
    }

    /// Test seam only: a registry whose LM Studio slot is a recording double, so the pre-warm
    /// routing can be asserted without touching `lms` or the network.
    #[cfg(test)]
    pub(super) fn with_lmstudio_for_tests(lmstudio: Arc<dyn SwarmEngine>) -> Self {
        Self {
            lmstudio,
            sidecars: BTreeMap::new(),
        }
    }

    /// The registered engine for a KIND — `None` only for a sidecar kind nobody registered.
    pub fn for_kind(&self, kind: EngineKind) -> Option<Arc<dyn SwarmEngine>> {
        match kind {
            EngineKind::LmStudio => Some(self.lmstudio.clone()),
            EngineKind::MlxSidecar => self.sidecars.values().next().cloned(),
        }
    }

    /// The goose provider name that dispatches to this KIND's engine ("lmstudio" / "omlx").
    pub(super) fn provider_name_for_kind(&self, kind: EngineKind) -> Option<&'static str> {
        self.for_kind(kind).map(|e| e.provider_name())
    }

    /// The LM Studio engine directly — for the paths that are LM-Studio-specific by construction
    /// (planner pre-warm and the `lms ps` pool build; a sidecar planner is unresolved behavior).
    pub fn lmstudio(&self) -> Arc<dyn SwarmEngine> {
        self.lmstudio.clone()
    }

    /// The engine that hosts THIS device's model. A device naming an engine kind with no
    /// registered engine (tagged mlx-sidecar without `mlx_engine` config) routes to LM Studio
    /// with a LOUD named absence-event — never silently.
    pub fn engine_for_device(&self, d: &SwarmDevice) -> Arc<dyn SwarmEngine> {
        match device_engine_kind(d) {
            EngineKind::LmStudio => self.lmstudio.clone(),
            EngineKind::MlxSidecar => match self.sidecars.values().next() {
                Some(e) => e.clone(),
                None => {
                    eprintln!(
                        "engine-absent: device '{}' names engine 'mlx-sidecar' but no sidecar \
                         engine is registered — operating via lmstudio until step C registers it",
                        d.id
                    );
                    self.lmstudio.clone()
                }
            },
        }
    }

    /// The servable-ids probe for one engine KIND. An engine kind with NO registered engine
    /// returns `None` — "the probe cannot answer", never a proven negative — with a loud named
    /// absence-event, so a device tagged for a missing engine can neither be condemned by nor
    /// counted against another engine's catalog (the proven-negative-on-the-same-object rule).
    pub fn servable_ids_for_kind(&self, kind: EngineKind) -> Option<HashSet<String>> {
        match kind {
            EngineKind::LmStudio => self.lmstudio.servable_model_ids(),
            EngineKind::MlxSidecar => {
                if self.sidecars.is_empty() {
                    eprintln!(
                        "engine-absent: the pool names engine 'mlx-sidecar' but no sidecar engine \
                         is registered — its servable probe reports failed (None), never a proven \
                         negative"
                    );
                    return None;
                }
                let mut ids = HashSet::new();
                let mut any = false;
                for e in self.sidecars.values() {
                    if let Some(s) = e.servable_model_ids() {
                        any = true;
                        ids.extend(s);
                    }
                }
                if any {
                    Some(ids)
                } else {
                    None
                }
            }
        }
    }
}

impl Default for Engines {
    fn default() -> Self {
        Self::new()
    }
}

/// One servable-ids probe per engine KIND present in the pool — each kind probed once, through
/// its own engine. A pool with no devices of a kind never probes that kind (and an empty pool
/// probes nothing), matching the old single-engine shape where the probe's only consumers are
/// pool-guarded.
pub(super) fn served_by_engine(
    engines: &Engines,
    pool: &[SwarmDevice],
) -> HashMap<EngineKind, Option<HashSet<String>>> {
    let mut out: HashMap<EngineKind, Option<HashSet<String>>> = HashMap::new();
    for d in pool {
        let kind = device_engine_kind(d);
        out.entry(kind)
            .or_insert_with(|| engines.servable_ids_for_kind(kind));
    }
    out
}

/// Per-ENGINE application of the proven-negative drop kernel (`drop_unservable_devices`,
/// unchanged in swarm.rs with its tests). Each device is judged ONLY against ITS OWN engine's
/// servable catalog, so a dead/unreachable sidecar can never condemn LM Studio devices (or vice
/// versa), and NEVER-EMPTIES-POOL holds independently per engine. With every device on LM Studio
/// — today's only reality — this is one partition and byte-identical to the kernel call it
/// replaced. Original pool order is preserved (each kernel output is an order-preserving
/// subsequence of its partition, re-interleaved against the original slot order).
pub(super) fn drop_unservable_devices_per_engine(
    devices: Vec<SwarmDevice>,
    served: &HashMap<EngineKind, Option<HashSet<String>>>,
) -> (Vec<SwarmDevice>, Vec<(String, String)>) {
    let slots: Vec<(EngineKind, String)> = devices
        .iter()
        .map(|d| (device_engine_kind(d), d.id.clone()))
        .collect();
    let mut parts: Vec<(EngineKind, Vec<SwarmDevice>)> = Vec::new();
    for d in devices {
        let kind = device_engine_kind(&d);
        match parts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, v)) => v.push(d),
            None => parts.push((kind, vec![d])),
        }
    }
    let mut dropped_all: Vec<(String, String)> = Vec::new();
    let mut kept: Vec<(EngineKind, VecDeque<SwarmDevice>)> = Vec::new();
    for (kind, part) in parts {
        let (keep, dropped) =
            super::swarm::drop_unservable_devices(part, served.get(&kind).and_then(|o| o.as_ref()));
        dropped_all.extend(dropped);
        kept.push((kind, keep.into()));
    }
    let mut keep_ordered: Vec<SwarmDevice> = Vec::new();
    for (kind, id) in slots {
        if let Some((_, dq)) = kept.iter_mut().find(|(k, _)| *k == kind) {
            if dq.front().is_some_and(|d| d.id == id) {
                if let Some(d) = dq.pop_front() {
                    keep_ordered.push(d);
                }
            }
        }
    }
    (keep_ordered, dropped_all)
}

/// Per-ENGINE #128 no-start guard: refuse only when EVERY engine's partition is PROVEN
/// all-unservable by its own probe (`all_resident_unservable`, unchanged in swarm.rs with its
/// tests). One healthy — or merely unproven — engine keeps the run alive; a failed probe on one
/// engine can never join a refusal it did not prove. All-LM-Studio pools are one partition and
/// byte-identical to the kernel call this replaced.
pub(super) fn all_resident_unservable_per_engine(
    pool: &[SwarmDevice],
    served: &HashMap<EngineKind, Option<HashSet<String>>>,
) -> bool {
    if pool.is_empty() {
        return false;
    }
    let mut parts: Vec<(EngineKind, Vec<SwarmDevice>)> = Vec::new();
    for d in pool {
        let kind = device_engine_kind(d);
        match parts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, v)) => v.push(d.clone()),
            None => parts.push((kind, vec![d.clone()])),
        }
    }
    parts.iter().all(|(kind, part)| {
        super::swarm::all_resident_unservable(part, served.get(kind).and_then(|o| o.as_ref()))
    })
}

/// The planner's servability fallback, decided by the engine of the POOL DEVICE that carries
/// the planner model. Before this the check consulted `served[LmStudio]` no matter where the
/// planner lived, so on a mixed pool (three LM Studio devices + a sidecar planner) the LM Studio
/// catalog — which by construction never lists a sidecar alias — "proved" the planner
/// unservable and silently moved it to `fleet_pool[0]`. A planner carried by no pool device is
/// LM Studio by definition (the historical shape, unchanged). Returns `Some((host, alt))` only on
/// a negative PROVEN by the planner's own engine — its probe answered with a non-empty set that
/// lacks the planner — and only when the pool has a first device to fall back to. An unmounted
/// sidecar probes to None (unproven) and is left alone: the pre-warm mounts it.
pub(super) fn planner_fallback(
    engines: &Engines,
    fleet_pool: &[SwarmDevice],
    served: &HashMap<EngineKind, Option<HashSet<String>>>,
    planner_model: &str,
) -> Option<(String, String)> {
    let planner_kind = fleet_pool
        .iter()
        .find(|d| d.model_id == planner_model)
        .map(device_engine_kind)
        .unwrap_or(EngineKind::LmStudio);
    let set = served.get(&planner_kind)?.as_ref()?;
    if set.contains(planner_model) {
        return None;
    }
    let alt = fleet_pool.first()?.model_id.clone();
    // A proven set for a kind implies its engine is registered (an unregistered kind probes to
    // None above); the label on the unreachable arm names the kind rather than inventing a host.
    let host = engines
        .for_kind(planner_kind)
        .map(|e| e.http_host())
        .unwrap_or_else(|| format!("{planner_kind:?} (no engine registered)"));
    Some((host, alt))
}

/// The one engine today. Construct through `default_engine` so every call site shares the seam
/// a second engine will slot into.
pub struct LmStudioEngine;

pub fn default_engine() -> Arc<dyn SwarmEngine> {
    Arc::new(LmStudioEngine)
}

impl SwarmEngine for LmStudioEngine {
    fn provider_name(&self) -> &'static str {
        "lmstudio"
    }
    fn http_host(&self) -> String {
        lms_http_host()
    }
    fn catalog_probe(&self) -> Vec<LmsProcess> {
        probe_lms_http()
    }
    fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
        endpoint_model_ids()
    }
    fn loaded_instance_count(&self, model_id: &str) -> usize {
        loaded_instance_count(model_id)
    }
    fn ensure_loaded(&self, model_id: &str, instances: u32) {
        ensure_loaded(model_id, instances)
    }
    fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
        probe_lms_processes()
    }
    fn probe_report(&self) {
        probe_fleet()
    }
}

// Everything below is engine-internal: swarm.rs reaches this module only through the trait, the
// registry and `default_engine`. `parse_lms_ps` stays in swarm.rs beside its tests (pub(super)).

/// Resolve the `lms` CLI binary. A Finder-launched desktop app does NOT inherit the shell PATH, so a bare
/// `lms` is not found — the GUI swarm bailed with "no models loaded" despite a loaded fleet. Check an
/// explicit override, then LM Studio's default install locations, else fall back to PATH.
fn resolve_lms() -> String {
    if let Ok(p) = std::env::var("SWARM_LMS_PATH") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        for rel in [".lmstudio/bin/lms", ".cache/lm-studio/bin/lms"] {
            let cand = std::path::Path::new(&home).join(rel);
            if cand.is_file() {
                return cand.to_string_lossy().into_owned();
            }
        }
    }
    "lms".to_string()
}

/// The LM Studio HTTP host for the fallback probe (LMSTUDIO_HOST, else the default local server).
fn lms_http_host() -> String {
    std::env::var("LMSTUDIO_HOST")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:1234".to_string())
}

/// The no-start guard (#128) is ON unless explicitly killed. It only ever ADDS a refusal on a PROVEN negative,
/// so the safe direction is on; the kill-switch exists so a misfiring probe can be worked around without a
/// rebuild. Env only (not a persisted config field) on purpose: a safety guard should not be silently disabled
/// by a stale serialized config, and a bare-bool `#[serde(default)]` would deserialize to false = off.
pub(super) fn require_servable() -> bool {
    match std::env::var("GOOSE_SWARM_REQUIRE_SERVABLE") {
        Ok(v) => !matches!(v.trim(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

/// Node/device name from an LM Link model id: the prefix before the first '-' (mihai-, workhorse-, gabee-).
pub(super) fn device_from_lms_id(id: &str) -> Option<String> {
    // NODE-FIRST: the fleet's per-host aliases put the node at the START of the id, and since the
    // qwen3.8 roll-over they carry the publisher inside them (`mihai-qwen/qwen3.8-27b`) — stripping
    // the namespace first collapsed all three nodes to "qwen3.8". Only when the first segment has
    // no dash at all (`qwen/qwen3.8-27b`, a shared alias) fall back to the post-slash segment.
    let first = id.split('/').next().unwrap_or(id);
    let seg = if first.contains('-') {
        first
    } else {
        id.rsplit('/').next().unwrap_or(id)
    };
    seg.split_once('-').map(|(prefix, _)| prefix.to_string())
}

/// Discover loaded models straight from the LM Studio HTTP server (native /api/v0/models) — the fallback
/// for when the `lms` CLI is missing/unreachable (a Finder-launched desktop app has no lms on PATH). The
/// HTTP server MUST be up for the swarm to call the models at all, so it is the robust source. Uses `curl`
/// (a system binary present on the minimal GUI PATH) to avoid a blocking HTTP call inside the async
/// runtime. Returns loaded, non-embedding models as LmsProcess entries (device derived from the id prefix).
fn probe_lms_http() -> Vec<LmsProcess> {
    let url = format!("{}/api/v0/models", lms_http_host().trim_end_matches('/'));
    let Ok(out) = ProcCommand::new("curl")
        .args(["-s", "--max-time", "6", &url])
        .output()
    else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return Vec::new();
    };
    let Some(arr) = json.get("data").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter(|m| {
            m.get("state").and_then(|v| v.as_str()) == Some("loaded")
                && m.get("type").and_then(|v| v.as_str()) != Some("embeddings")
        })
        .filter_map(|m| {
            let id = m
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                return None;
            }
            Some(LmsProcess {
                device: device_from_lms_id(&id),
                identifier: id,
                status: "loaded".to_string(),
                parallel: None,
                loaded_context_length: m.get("loaded_context_length").and_then(|v| v.as_u64()),
            })
        })
        .collect()
}

/// The model ids the ENDPOINT will actually serve — i.e. the only ids a worker can dispatch to.
///
/// `None` means the probe itself failed (endpoint down, curl missing, unparseable body). That is NOT the
/// same as "no models", and the caller must never gate on it: an instrument reporting zero has been wrong
/// seven times in this project, and gating a whole run off a failed probe would be the eighth.
///
/// WHY THIS IS NOT `lms ps`: `lms ps` lists what is RESIDENT; `/v1/models` lists what is SERVABLE, and they
/// disagree in exactly the case that costs a run. MEASURED 2026-07-17: `lms ps` showed
/// `workhorse-qwopus3.6-27b-coder-mlx` IDLE and loaded, while POSTing to it returned
/// `400 Invalid model identifier` — the Mac Studio had dropped off the LAN and LM Link had withdrawn the
/// alias, but the resident list still carried it. The pool is built from `lms ps`, so the swarm cheerfully
/// dispatched a third of its tasks into an instant 400.
fn endpoint_model_ids() -> Option<std::collections::HashSet<String>> {
    let url = format!("{}/v1/models", lms_http_host().trim_end_matches('/'));
    let out = ProcCommand::new("curl")
        .args(["-s", "--max-time", "6", &url])
        .output()
        .ok()?;
    let json = serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()?;
    let arr = json.get("data")?.as_array()?;
    let ids: std::collections::HashSet<String> = arr
        .iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string))
        .collect();
    if ids.is_empty() {
        return None;
    }
    Some(ids)
}

fn probe_fleet() {
    println!("\n{}", style("lms ps:").bold());
    match ProcCommand::new(resolve_lms()).arg("ps").output() {
        Ok(out) => print!("{}", String::from_utf8_lossy(&out.stdout)),
        Err(e) => println!("  (lms ps failed: {e})"),
    }
    println!("{}", style("endpoint model ids:").bold());
    let models_url = format!("{}/v1/models", lms_http_host().trim_end_matches('/'));
    match ProcCommand::new("curl")
        .args(["-s", "--max-time", "6", &models_url])
        .output()
    {
        Ok(out) => {
            let body = String::from_utf8_lossy(&out.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                    for m in arr {
                        if let Some(id) = m.get("id").and_then(|i| i.as_str()) {
                            println!("  {id}");
                        }
                    }
                }
            }
        }
        Err(e) => println!("  (curl failed: {e})"),
    }
}

/// Count currently-loaded instances of a model across the fleet (`lms ps`).
fn loaded_instance_count(model_id: &str) -> usize {
    match ProcCommand::new(resolve_lms()).arg("ps").output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains(model_id))
            .count(),
        Err(_) => 0,
    }
}

/// Ensure up to `instances` copies of a model are loaded — and NEVER more than already present, so
/// repeated runs / pre-warms don't stack duplicate instances (the cause of "3 instances on one box").
/// Default `instances` is 1, so goose never spins up extras unless the user raises it.
fn ensure_loaded(model_id: &str, instances: u32) {
    let want = instances.max(1) as usize;
    let have = loaded_instance_count(model_id);
    for _ in have..want {
        let _ = ProcCommand::new(resolve_lms())
            .args(["load", model_id, "-y", "--ttl", "3600"])
            .output();
    }
}

fn probe_lms_processes() -> Result<Vec<LmsProcess>> {
    // Primary: the `lms` CLI (richest — carries DEVICE + PARALLEL). Resolve its real path since a
    // Finder-launched app has no lms on PATH.
    if let Ok(out) = ProcCommand::new(resolve_lms()).arg("ps").output() {
        if out.status.success() {
            if let Ok(procs) = parse_lms_ps(&String::from_utf8_lossy(&out.stdout)) {
                if !procs.is_empty() {
                    return Ok(procs);
                }
            }
        }
    }
    // Fallback: the LM Studio HTTP server (no lms CLI needed). Empty if the server is unreachable too.
    Ok(probe_lms_http())
}

// ---------------------------------------------------------------------------------------------
// Step C: the MLX sidecar engine — goose-sidecar's supervised Rapid-MLX process behind the trait
// ---------------------------------------------------------------------------------------------

/// Drive an engine-manager future from the SYNC trait surface. Inside the runtime (the run
/// pipeline / dispatcher — goose-cli's runtime is multi-thread) `block_in_place` keeps the worker
/// thread legal; outside one (unit tests, sync menu paths) a throwaway runtime drives it.
fn block_on_engine<F: std::future::Future>(fut: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("fresh tokio runtime for a sidecar engine call")
            .block_on(fut),
    }
}

/// The MLX sidecar: one supervised Rapid-MLX process serving one mounted model, OpenAI-compat on
/// 127.0.0.1:{port}, dispatched through the declarative `omlx` provider (OMLX_HOST). Every probe
/// reads the LIVE `/v1/models` catalog — facts only, nothing fabricated.
pub struct SidecarEngine {
    manager: Arc<MlxEngineManager>,
    base_url: String,
}

impl SidecarEngine {
    pub fn new(manager: Arc<MlxEngineManager>) -> Self {
        let base_url = format!("http://127.0.0.1:{}", manager.settings().port);
        Self { manager, base_url }
    }

    /// GET {base_url}/v1/models via curl — the same subprocess idiom as the LM Studio probes
    /// (a blocking HTTP client inside the async runtime is the trap both avoid).
    fn v1_models(&self) -> Option<serde_json::Value> {
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        let out = ProcCommand::new("curl")
            .args(["-s", "--max-time", "6", &url])
            .output()
            .ok()?;
        serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()
    }

    /// (served id, context_window) per catalog entry; empty when the engine is down/unreachable.
    fn served_entries(&self) -> Vec<(String, Option<u64>)> {
        let Some(json) = self.v1_models() else {
            return Vec::new();
        };
        let Some(arr) = json.get("data").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|m| {
                let id = m.get("id").and_then(|v| v.as_str())?.to_string();
                Some((id, m.get("context_window").and_then(|v| v.as_u64())))
            })
            .collect()
    }
}

impl SwarmEngine for SidecarEngine {
    fn provider_name(&self) -> &'static str {
        "omlx"
    }
    fn http_host(&self) -> String {
        self.base_url.clone()
    }
    fn catalog_probe(&self) -> Vec<LmsProcess> {
        self.served_entries()
            .into_iter()
            .map(|(id, context_window)| LmsProcess {
                device: device_from_lms_id(&id),
                identifier: id,
                // A serving rapid-mlx holds its model resident — served IS loaded here.
                status: "loaded".to_string(),
                // rapid-mlx does not report a PARALLEL figure; absent, never invented.
                parallel: None,
                loaded_context_length: context_window,
            })
            .collect()
    }
    fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
        // Identical None semantics to LM Studio's probe: empty/unreachable is "cannot answer",
        // never "no models" — the per-engine guard treats it as unproven.
        let ids: HashSet<String> = self
            .served_entries()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        if ids.is_empty() {
            return None;
        }
        Some(ids)
    }
    fn loaded_instance_count(&self, model_id: &str) -> usize {
        // One supervised process serves one mounted model: 1 iff the live catalog serves it.
        usize::from(self.served_entries().iter().any(|(id, _)| id == model_id))
    }
    fn ensure_loaded(&self, model_id: &str, _instances: u32) {
        // Fast path: the live catalog already serves it — possibly mounted by ANOTHER process's
        // manager (the desktop window); mounting again would fight over the port.
        if self.loaded_instance_count(model_id) > 0 {
            return;
        }
        // `instances` is accepted-and-ignored: the supervisor owns one process serving one model.
        // TTL likewise — the supervisor owns the engine's lifetime, and rapid-mlx has its own
        // --resident-model-idle-ttl if that lever is ever wanted.
        let mut settings = self.manager.settings();
        let Some(hf_dir) = settings.model_id.clone() else {
            eprintln!(
                "engine-config-absent: mlx_engine.model_id is not set — cannot mount \
                 '{model_id}' (set the HF model directory id under config key \"mlx_engine\")"
            );
            return;
        };
        // The swarm-facing alias: the server advertises the requested pool model_id, so the
        // fleet's node-prefix identity convention needs zero goose changes.
        settings.served_model_name = Some(model_id.to_string());
        self.manager.set_settings(settings);
        let result = block_on_engine(async {
            self.manager.mount(&hf_dir).await?;
            // Poll the manager's own state machine to a terminal state. No wall ceiling here:
            // the supervisor's startup handling terminates the Mounting state itself (Running or
            // Failed), and the interval below is a lifecycle poll, not a bound on model work.
            loop {
                let status = self.manager.status().await;
                match status.state.as_str() {
                    "running" => return Ok(()),
                    "failed" => bail!(
                        "mlx engine mount failed for '{hf_dir}': {}",
                        status
                            .last_error
                            .as_deref()
                            .unwrap_or("(no error recorded)")
                    ),
                    "stopped" => bail!("mount of '{hf_dir}' was superseded (engine stopped)"),
                    _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                }
            }
        });
        if let Err(e) = result {
            eprintln!("engine-mount-failed: {e:#}");
        }
    }
    fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
        Ok(self.catalog_probe())
    }
    fn probe_report(&self) {
        let status = block_on_engine(self.manager.status());
        println!("{}", style("mlx-sidecar:").bold());
        println!("  state: {}", status.state);
        if let Some(m) = &status.model_id {
            println!("  mounted: {m}");
        }
        if let Some(e) = &status.last_error {
            println!("  last error: {e}");
        }
        println!("  port: {}", self.manager.settings().port);
        for (id, ctx) in self.served_entries() {
            match ctx {
                Some(c) => println!("  serves: {id} (context {c})"),
                None => println!("  serves: {id}"),
            }
        }
    }
}

/// Step-C registry construction — the ONE construction site's body. LM Studio always; the MLX
/// sidecar iff the config declares `mlx_engine` settings AND a pool device is tagged for it.
/// Neither condition -> the step-B registry, byte-identical.
pub(super) fn engines_for_run(devices: &[SwarmDevice]) -> Engines {
    let mut engines = Engines::new();
    if !devices
        .iter()
        .any(|d| d.enabled && d.engine == Some(EngineKind::MlxSidecar))
    {
        return engines;
    }
    match goose::config::Config::global().get_param::<EngineSettings>("mlx_engine") {
        Ok(settings) => {
            let manager = Arc::new(MlxEngineManager::new());
            manager.set_settings(settings);
            let sidecar = SidecarEngine::new(manager);
            // The LMSTUDIO_HOST idiom from step A: exported before the dispatcher constructs any
            // provider, so the declarative `omlx` provider resolves to THIS sidecar.
            std::env::set_var("OMLX_HOST", sidecar.http_host());
            eprintln!(
                "  · mlx-sidecar engine registered at {} (provider omlx)",
                sidecar.http_host()
            );
            engines.register_sidecar("mlx-sidecar", Arc::new(sidecar));
        }
        Err(e) => eprintln!(
            "engine-config-absent: a pool device is tagged engine=mlx-sidecar but config key \
             \"mlx_engine\" did not load ({e}) — no sidecar engine registered; its devices' \
             probes report failed (None) and dispatching to them will error"
        ),
    }
    engines
}

/// JIT pre-warm for the resolved pool: each model warms through ITS OWN engine. The planner
/// warms through the engine of the POOL DEVICE that carries it — the LM-pinned planner arm this
/// replaces fired a doomed `lms load <sidecar-alias>` and could never mount the sidecar. A
/// planner carried by no pool device keeps the historical LM Studio warm-up byte-identically
/// (a sidecar planner outside the pool has no device to name its engine — left unresolved).
pub(super) fn prewarm_pool(engines: &Engines, enabled: &[SwarmDevice], planner_model: &str) {
    if !enabled
        .iter()
        .any(|d| d.is_cloud() && d.model_id == planner_model)
    {
        match enabled
            .iter()
            .find(|d| !d.is_cloud() && d.model_id == planner_model)
        {
            Some(d) => engines.engine_for_device(d).ensure_loaded(planner_model, 1),
            None => engines.lmstudio().ensure_loaded(planner_model, 1),
        }
    }
    for d in enabled.iter().filter(|d| !d.is_cloud()) {
        engines
            .engine_for_device(d)
            .ensure_loaded(&d.model_id, d.instances);
    }
}

/// "Auto-use what's loaded": build the worker pool from the models currently resident on the
/// LM Studio fleet (`lms ps` through the engine's own probe chain) so the swarm runs on what's
/// actually loaded, not (possibly stale) configured model_ids. LM-Studio-only BY CONSTRUCTION:
/// sidecar devices are config-declared, not discovered — `merge_sidecar_devices` adds them.
/// Returns (pool, planner_model). An empty pool means the fleet has nothing loaded (caller
/// bootstraps or bails). Weights: explicit device override, else speed_weight, else LM Studio
/// PARALLEL, else 1.
pub(super) fn reconcile_pool_with_fleet(
    cfg: &SwarmConfig,
    engines: &Engines,
) -> (Vec<SwarmDevice>, Option<String>) {
    let procs = match engines.lmstudio().resident_processes() {
        Ok(p) => p,
        Err(_) => return (Vec::new(), None),
    };
    // One worker per DISTINCT loaded identifier (LM Link routes by identifier); first host wins.
    let mut seen = std::collections::HashSet::new();
    let mut resident: Vec<&LmsProcess> = Vec::new();
    for p in &procs {
        if !p.identifier.is_empty() && seen.insert(p.identifier.clone()) {
            resident.push(p);
        }
    }
    if resident.is_empty() {
        return (Vec::new(), None);
    }
    let pool: Vec<SwarmDevice> = resident
        .iter()
        .map(|p| SwarmDevice {
            id: gen_entry_id(cfg, p.device.as_deref(), &p.identifier),
            model_id: p.identifier.clone(),
            // Weight = an explicit configured override for this model_id (USER WINS — never clamped: a
            // weight above the probed PARALLEL is a legit throughput tactic since agent tasks are bursty, so
            // an extra slot overlaps the idle LM Studio window between an agent's LLM calls), else the
            // configured speed_weight for this host/model (so "a slower machine does less work" actually
            // shapes DISPATCH, not just planner pick — backlog #6), else LM Studio's PARALLEL for this
            // instance, else 1. K6: if an override EXCEEDS the probed PARALLEL we WARN but keep the value.
            weight: {
                let user_w = cfg
                    .devices
                    .iter()
                    .find(|d| d.model_id == p.identifier)
                    .map(|d| d.weight);
                if let (Some(w), Some(par)) = (user_w, p.parallel) {
                    if w > par {
                        eprintln!(
                            "  {} pool weight {w} for {} exceeds LM Studio PARALLEL {par} — kept (oversubscribing can overlap the idle gaps between an agent's LLM calls; if requests just queue with no gain, lower it)",
                            style("⚠").yellow(),
                            p.identifier,
                        );
                    }
                }
                // Concurrency = the node's real capacity: an explicit device override, else LM Studio's
                // PARALLEL, else 1. NOT the speed_weight — LM Studio serves one request per model at a time,
                // so weight > PARALLEL just QUEUES requests on that node and STARVES an idle one (observed:
                // workhorse w3 got 3 tasks, 2 queued, while gabee sat READY). "Faster host does MORE work" is
                // handled separately by pick_device's speed_weight-weighted ROUTING (DeviceCfg.speed_weight),
                // which spreads proportionally more tasks to the fast node OVER TIME via work-stealing (it
                // finishes first and grabs the next ready task) — the correct lever, without oversubscribing.
                user_w.or(p.parallel).unwrap_or(1).max(1)
            },
            enabled: true,
            instances: 1,
            host: p.device.clone(),
            provider: None,
            // Discovery rebuilds the local pool from `lms ps`, so anything the user set per node has to be
            // carried across or it is silently lost every run. Matched on model_id, which is what LM Link
            // actually routes by.
            speed_weight: cfg
                .devices
                .iter()
                .find(|d| d.model_id == p.identifier)
                .and_then(|d| d.speed_weight),
            supervision: cfg
                .devices
                .iter()
                .find(|d| d.model_id == p.identifier)
                .and_then(|d| d.supervision),
            // Discovered from the LM Studio fleet, so LM Studio by definition (None = LmStudio).
            engine: None,
        })
        .collect();
    // Planner: keep the configured planner if it is resident; else pick the best resident model for the
    // hardest job (the architect skeleton). QUALITY outranks speed here: a low-quant model (q5/q4/q3/q2)
    // fails the structured skeleton, so prefer a NOT-low-quant model FIRST, then the fastest host
    // (highest speed_weight). speed_weight keys match device+identifier (some identifiers omit the host).
    let planner_rank = |p: &&LmsProcess| -> (u8, u32) {
        let ident = p.identifier.to_lowercase();
        let quant_ok = u8::from(
            !(ident.contains("q2_")
                || ident.contains("q3_")
                || ident.contains("q4_")
                || ident.contains("q5")),
        );
        let hay = format!("{} {}", p.device.as_deref().unwrap_or(""), ident);
        let speed = cfg
            .speed_weights
            .iter()
            .find(|(pat, _)| hay.contains(pat.as_str()))
            .map(|(_, w)| *w)
            .unwrap_or(1);
        (quant_ok, speed)
    };
    let planner = if resident.iter().any(|p| p.identifier == cfg.planner_model) {
        Some(cfg.planner_model.clone())
    } else {
        resident
            .iter()
            .filter(|p| {
                let n = p.identifier.to_lowercase();
                n.contains("27b") || n.contains("dense") || n.contains("coder")
            })
            .max_by_key(|p| planner_rank(p))
            .or_else(|| resident.iter().max_by_key(|p| planner_rank(p)))
            .map(|p| p.identifier.clone())
    };
    (pool, planner)
}

/// Config-declared sidecar devices join the pool ADDITIVELY: reconcile discovers only the LM
/// Studio fleet (`lms ps` cannot see a rapid-mlx server), so a device tagged engine=mlx-sidecar
/// is declared, not discovered. Merged BEFORE the per-engine servability partition, which judges
/// it against the SIDECAR's own catalog (a not-yet-mounted engine probes to None and is never
/// condemned). Dedup by id, like the cloud merge.
pub(super) fn merge_sidecar_devices(pool: &mut Vec<SwarmDevice>, configured: &[SwarmDevice]) {
    for d in configured
        .iter()
        .filter(|d| d.enabled && d.engine == Some(EngineKind::MlxSidecar))
    {
        if !pool.iter().any(|p| p.id == d.id) {
            eprintln!(
                "  · sidecar node: {} → {} via mlx-sidecar",
                d.id, d.model_id
            );
            pool.push(d.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::swarm::drop_unservable_devices;

    fn dev(id: &str, model: &str, engine: Option<EngineKind>) -> SwarmDevice {
        SwarmDevice {
            id: id.to_string(),
            model_id: model.to_string(),
            weight: 1,
            enabled: true,
            instances: 1,
            host: None,
            provider: None,
            speed_weight: None,
            supervision: None,
            engine,
        }
    }

    fn served(
        entries: &[(EngineKind, Option<&[&str]>)],
    ) -> HashMap<EngineKind, Option<HashSet<String>>> {
        entries
            .iter()
            .map(|(k, ids)| {
                (
                    *k,
                    ids.map(|ids| ids.iter().map(|s| s.to_string()).collect()),
                )
            })
            .collect()
    }

    /// One engine's probe failed, the other answered: only the ANSWERED engine's devices can be
    /// dropped, and the failed engine's devices pass through untouched — a dead/unreachable
    /// sidecar can never condemn LM Studio devices, and vice versa.
    #[test]
    fn a_failed_probe_on_one_engine_never_condemns_the_other_engines_devices() {
        let pool = vec![
            dev("lm-a", "model-a", None),
            dev("lm-b", "model-b", Some(EngineKind::LmStudio)),
            dev("mlx-c", "model-c", Some(EngineKind::MlxSidecar)),
        ];
        // LM Studio answered (serves only model-a); the sidecar probe failed (None).
        let map = served(&[
            (EngineKind::LmStudio, Some(&["model-a"])),
            (EngineKind::MlxSidecar, None),
        ]);
        let (keep, dropped) = drop_unservable_devices_per_engine(pool.clone(), &map);
        let keep_ids: Vec<&str> = keep.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(
            keep_ids,
            vec!["lm-a", "mlx-c"],
            "lm-b dropped by ITS engine's proof; mlx-c untouched by the failed sidecar probe"
        );
        assert_eq!(dropped, vec![("lm-b".to_string(), "model-b".to_string())]);
        assert!(
            !all_resident_unservable_per_engine(&pool, &map),
            "one servable LM Studio resident keeps the run alive regardless of the sidecar probe"
        );
        // The mirror image: the sidecar answered, LM Studio's probe failed.
        let map = served(&[
            (EngineKind::LmStudio, None),
            (EngineKind::MlxSidecar, Some(&["model-c"])),
        ]);
        let (keep, dropped) = drop_unservable_devices_per_engine(pool.clone(), &map);
        assert_eq!(keep.len(), 3, "a failed LM Studio probe drops nothing");
        assert!(dropped.is_empty());
        assert!(!all_resident_unservable_per_engine(&pool, &map));
    }

    /// Both probes failed: byte-for-byte the None passthrough of the single-engine kernel, on
    /// every partition — nothing dropped, nothing refused (the seven-lying-instruments rule).
    #[test]
    fn both_probes_failed_drops_nothing_and_never_refuses() {
        let pool = vec![
            dev("lm-a", "model-a", None),
            dev("mlx-c", "model-c", Some(EngineKind::MlxSidecar)),
        ];
        let map = served(&[(EngineKind::LmStudio, None), (EngineKind::MlxSidecar, None)]);
        let (keep, dropped) = drop_unservable_devices_per_engine(pool.clone(), &map);
        assert_eq!(keep.len(), 2);
        assert!(dropped.is_empty());
        assert!(!all_resident_unservable_per_engine(&pool, &map));
    }

    /// A sidecar-tagged device is judged ONLY against the sidecar's catalog: absent from LM
    /// Studio's healthy servable set, it is neither dropped nor counted toward a refusal.
    #[test]
    fn a_sidecar_device_is_never_counted_against_the_lmstudio_servable_set() {
        let pool = vec![
            dev("lm-a", "model-a", None),
            dev("mlx-c", "model-c", Some(EngineKind::MlxSidecar)),
        ];
        // model-c is nowhere in LM Studio's (healthy, non-empty) catalog — its own engine serves it.
        let map = served(&[
            (EngineKind::LmStudio, Some(&["model-a"])),
            (EngineKind::MlxSidecar, Some(&["model-c"])),
        ]);
        let (keep, dropped) = drop_unservable_devices_per_engine(pool.clone(), &map);
        assert_eq!(keep.len(), 2, "each device servable by its OWN engine");
        assert!(dropped.is_empty());
        assert!(!all_resident_unservable_per_engine(&pool, &map));
    }

    /// The #128 refusal now requires EVERY engine's partition proven all-unservable by its own
    /// probe; one unproven (or healthy) engine vetoes the refusal.
    #[test]
    fn refusal_requires_every_engines_partition_proven_unservable() {
        let pool = vec![
            dev("lm-a", "model-a", None),
            dev("mlx-c", "model-c", Some(EngineKind::MlxSidecar)),
        ];
        // Both engines answered with non-empty catalogs excluding every resident -> proven -> refuse.
        let map = served(&[
            (EngineKind::LmStudio, Some(&["other-1"])),
            (EngineKind::MlxSidecar, Some(&["other-2"])),
        ]);
        assert!(all_resident_unservable_per_engine(&pool, &map));
        // The sidecar partition becomes unproven (probe failed) -> the refusal dies with it.
        let map = served(&[
            (EngineKind::LmStudio, Some(&["other-1"])),
            (EngineKind::MlxSidecar, None),
        ]);
        assert!(!all_resident_unservable_per_engine(&pool, &map));
        // Empty pool never refuses.
        assert!(!all_resident_unservable_per_engine(&[], &map));
    }

    /// Today's only reality — every device on LM Studio — is ONE partition, and the wrapper's
    /// result is exactly the kernel's on the same inputs (drop set, keep set, order).
    #[test]
    fn an_all_lmstudio_pool_is_byte_identical_to_the_kernel() {
        let pool = vec![
            dev("local-mihai", "mihai-qwopus3.6-27b-coder-mlx", None),
            dev("mac-gabee", "gabee-qwopus3.6-27b-coder-mlx", None),
            dev("works-workhorse", "workhorse-qwopus3.6-27b-coder-mlx", None),
        ];
        let ids: HashSet<String> = [
            "mihai-qwopus3.6-27b-coder-mlx",
            "gabee-qwopus3.6-27b-coder-mlx",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let map = served(&[(
            EngineKind::LmStudio,
            Some(&[
                "mihai-qwopus3.6-27b-coder-mlx",
                "gabee-qwopus3.6-27b-coder-mlx",
            ]),
        )]);
        let (keep_kernel, dropped_kernel) = drop_unservable_devices(pool.clone(), Some(&ids));
        let (keep_wrap, dropped_wrap) = drop_unservable_devices_per_engine(pool, &map);
        assert_eq!(
            keep_wrap.iter().map(|d| &d.id).collect::<Vec<_>>(),
            keep_kernel.iter().map(|d| &d.id).collect::<Vec<_>>()
        );
        assert_eq!(dropped_wrap, dropped_kernel);
    }

    /// An UNREGISTERED engine kind probes to None — a failed probe, never a proven negative —
    /// and a pool with no LM Studio devices never touches the LM Studio probe at all.
    #[test]
    fn an_unregistered_sidecar_kind_probes_to_none() {
        let engines = Engines::new();
        let pool = vec![dev("mlx-c", "model-c", Some(EngineKind::MlxSidecar))];
        let map = served_by_engine(&engines, &pool);
        assert_eq!(map.get(&EngineKind::MlxSidecar), Some(&None));
        assert!(
            !map.contains_key(&EngineKind::LmStudio),
            "no LM Studio device in the pool -> its probe is never run"
        );
    }

    #[test]
    fn device_from_lms_id_takes_node_prefix() {
        assert_eq!(
            device_from_lms_id("mihai-qwopus3.6-27b-coder-mlx").as_deref(),
            Some("mihai")
        );
        assert_eq!(
            device_from_lms_id("workhorse-qwopus3.6-27b-coder-mlx").as_deref(),
            Some("workhorse")
        );
        // NODE-FIRST rule: a per-host alias keeps its node even with a publisher inside
        assert_eq!(
            device_from_lms_id("mihai-qwen/qwen3.8-27b").as_deref(),
            Some("mihai")
        );
        // a shared publisher alias falls back to the post-slash segment's prefix
        assert_eq!(
            device_from_lms_id("qwen/qwen3.8-27b").as_deref(),
            Some("qwen3.8")
        );
        // no dash -> no derivable device
        assert_eq!(device_from_lms_id("solomodel").as_deref(), None);
    }

    /// Golden rapid-mlx `/v1/models` body — the exact field shape the manager's own
    /// probe_model_info reads (data[].id / context_window / tool_call_parser).
    const GOLDEN_V1_MODELS: &str = r#"{"object":"list","data":[{"id":"workhorse-qwen3-coder-30b-mlx","object":"model","context_window":262144,"tool_call_parser":"qwen3_coder"}]}"#;

    /// One-thread HTTP stub serving a fixed body on every request (each probe curls separately).
    /// The detached accept loop lives for the test binary — harmless on an ephemeral port.
    fn serve_stub(body: &'static str) -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        port
    }

    fn sidecar_on(port: u16) -> SidecarEngine {
        let manager = Arc::new(MlxEngineManager::new());
        let mut settings = manager.settings();
        settings.port = port;
        manager.set_settings(settings);
        SidecarEngine::new(manager)
    }

    /// Every SidecarEngine probe is a read of the LIVE catalog: ids, loaded state, context
    /// window, node-prefix device — and parallel stays None (rapid-mlx does not report one).
    #[test]
    fn sidecar_catalog_reads_v1_models_honestly() {
        let port = serve_stub(GOLDEN_V1_MODELS);
        let eng = sidecar_on(port);
        assert_eq!(eng.provider_name(), "omlx");
        assert_eq!(eng.http_host(), format!("http://127.0.0.1:{port}"));
        let ids = eng.servable_model_ids().expect("stub serves one model");
        assert!(ids.contains("workhorse-qwen3-coder-30b-mlx"));
        assert_eq!(
            eng.loaded_instance_count("workhorse-qwen3-coder-30b-mlx"),
            1
        );
        assert_eq!(eng.loaded_instance_count("something-else"), 0);
        let procs = eng.resident_processes().expect("catalog probe");
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].identifier, "workhorse-qwen3-coder-30b-mlx");
        assert_eq!(procs[0].status, "loaded");
        assert_eq!(procs[0].device.as_deref(), Some("workhorse"));
        assert_eq!(procs[0].parallel, None, "never invented");
        assert_eq!(procs[0].loaded_context_length, Some(262_144));
    }

    /// A dead sidecar answers None — the identical "cannot answer" semantics of the LM probe.
    #[test]
    fn sidecar_probe_failure_is_none_never_empty() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        let eng = sidecar_on(port);
        assert_eq!(eng.servable_model_ids(), None);
        assert_eq!(eng.loaded_instance_count("anything"), 0);
    }

    /// ensure_loaded's fast path: already-served means NO mount attempt (the engine may belong
    /// to another process's manager — mounting again would fight over the port) and settings
    /// untouched.
    #[test]
    fn sidecar_ensure_loaded_fast_paths_when_already_served() {
        let port = serve_stub(GOLDEN_V1_MODELS);
        let eng = sidecar_on(port);
        eng.ensure_loaded("workhorse-qwen3-coder-30b-mlx", 1);
        let status = tokio::runtime::Runtime::new()
            .expect("test runtime")
            .block_on(eng.manager.status());
        assert_eq!(
            status.state, "stopped",
            "already served -> no mount attempted"
        );
        assert_eq!(
            eng.manager.settings().served_model_name,
            None,
            "fast path leaves settings untouched"
        );
    }

    /// The loud-refusal case: mlx_engine.model_id unset and the model not served — ensure_loaded
    /// reports the named config absence and never attempts a mount.
    #[test]
    fn sidecar_ensure_loaded_refuses_loudly_without_a_configured_model_dir() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        let eng = sidecar_on(port);
        assert_eq!(eng.manager.settings().model_id, None, "precondition");
        eng.ensure_loaded("workhorse-qwen3-coder-30b-mlx", 1);
        let status = tokio::runtime::Runtime::new()
            .expect("test runtime")
            .block_on(eng.manager.status());
        assert_eq!(
            status.state, "stopped",
            "unset mlx_engine.model_id -> loud refusal, no mount"
        );
    }

    /// A SwarmEngine double that records every ensure_loaded call and answers every probe with
    /// honest emptiness — no lms, no network.
    struct RecordingEngine {
        name: &'static str,
        host: &'static str,
        calls: std::sync::Mutex<Vec<(String, u32)>>,
    }
    impl RecordingEngine {
        fn new(name: &'static str) -> Arc<Self> {
            Self::with_host(name, "")
        }
        fn with_host(name: &'static str, host: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                host,
                calls: std::sync::Mutex::new(Vec::new()),
            })
        }
        fn calls(&self) -> Vec<(String, u32)> {
            self.calls.lock().expect("calls lock").clone()
        }
    }
    impl SwarmEngine for RecordingEngine {
        fn provider_name(&self) -> &'static str {
            self.name
        }
        fn http_host(&self) -> String {
            self.host.to_string()
        }
        fn catalog_probe(&self) -> Vec<LmsProcess> {
            Vec::new()
        }
        fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
            None
        }
        fn loaded_instance_count(&self, _model_id: &str) -> usize {
            0
        }
        fn ensure_loaded(&self, model_id: &str, instances: u32) {
            self.calls
                .lock()
                .expect("calls lock")
                .push((model_id.to_string(), instances));
        }
        fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
            Ok(Vec::new())
        }
        fn probe_report(&self) {}
    }

    /// The 2026-08-30 micro-run defect, pinned: a config-complete sidecar device (tagged, engine
    /// registered) MUST be pre-warmed through ITS OWN engine — and a planner carried by that
    /// device warms there too, never through a doomed `lms load` of the alias.
    #[test]
    fn prewarm_mounts_a_sidecar_device_through_its_own_engine() {
        let lm = RecordingEngine::new("lmstudio");
        let sc = RecordingEngine::new("omlx");
        let mut engines = Engines::with_lmstudio_for_tests(lm.clone());
        engines.register_sidecar("mlx-sidecar", sc.clone());
        let pool = vec![
            dev(
                "workhorse-mlx",
                "workhorse-alias-mlx",
                Some(EngineKind::MlxSidecar),
            ),
            dev("mac-gabee", "gabee-qwen", None),
        ];
        prewarm_pool(&engines, &pool, "workhorse-alias-mlx");
        assert_eq!(
            sc.calls(),
            vec![
                ("workhorse-alias-mlx".to_string(), 1), // planner, via ITS device's engine
                ("workhorse-alias-mlx".to_string(), 1), // the device loop (fast-path at runtime)
            ],
            "the sidecar engine receives both warms for its model"
        );
        assert_eq!(
            lm.calls(),
            vec![("gabee-qwen".to_string(), 1)],
            "LM Studio warms only ITS device — no doomed lms load of the sidecar alias"
        );
    }

    /// The historical shape stays byte-identical: a planner carried by NO pool device keeps the
    /// LM Studio warm-up.
    #[test]
    fn prewarm_keeps_lmstudio_warmup_for_an_out_of_pool_planner() {
        let lm = RecordingEngine::new("lmstudio");
        let engines = Engines::with_lmstudio_for_tests(lm.clone());
        let pool = vec![dev("mac-gabee", "gabee-qwen", None)];
        prewarm_pool(&engines, &pool, "some-other-planner");
        assert_eq!(
            lm.calls(),
            vec![
                ("some-other-planner".to_string(), 1),
                ("gabee-qwen".to_string(), 1),
            ]
        );
    }

    /// S-H1 pinned on the mixed pool [gabee, mihai, workhorse-27b, workhorse-mlx]: the LM Studio
    /// catalog never lists a sidecar alias, so consulting it for a sidecar planner "proved" the
    /// planner unservable and moved it to fleet_pool[0]. The planner's OWN engine decides —
    /// mounted (its probe serves the alias) → kept; unmounted (probe None) → left for the
    /// pre-warm; withdrawn on its own engine → the fallback names THAT engine's host.
    #[test]
    fn planner_fallback_consults_the_engine_of_the_device_carrying_the_planner() {
        let mut engines = Engines::with_lmstudio_for_tests(RecordingEngine::with_host(
            "lmstudio",
            "http://lm.local:1234",
        ));
        engines.register_sidecar(
            "mlx-sidecar",
            RecordingEngine::with_host("omlx", "http://127.0.0.1:8899"),
        );
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let pool = vec![
            dev("mac-gabee", "gabee-qwen3.8-27b", None),
            dev("local-mihai", "mihai-qwen3.8-27b", None),
            dev("works-workhorse", "workhorse-qwen3.8-27b", None),
            dev("workhorse-mlx", alias, Some(EngineKind::MlxSidecar)),
        ];
        let lm_ids: &[&str] = &[
            "gabee-qwen3.8-27b",
            "mihai-qwen3.8-27b",
            "workhorse-qwen3.8-27b",
        ];
        // Mounted: the sidecar's own probe serves the alias -> the planner is KEPT (the old code
        // read served[LmStudio] here and moved it to gabee).
        let map = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, Some(&[alias])),
        ]);
        assert_eq!(planner_fallback(&engines, &pool, &map, alias), None);
        // Unmounted: the sidecar probe cannot answer -> unproven -> kept for the pre-warm.
        let map = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, None),
        ]);
        assert_eq!(planner_fallback(&engines, &pool, &map, alias), None);
        // Proven by ITS engine (a different alias is mounted): fall back, naming the sidecar host.
        let map = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, Some(&["workhorse-other-alias"])),
        ]);
        assert_eq!(
            planner_fallback(&engines, &pool, &map, alias),
            Some((
                "http://127.0.0.1:8899".to_string(),
                "gabee-qwen3.8-27b".to_string()
            ))
        );
        // All-LM pool, byte-identical to the LM-only check: a withdrawn planner falls back with
        // the LM host; a served one stays; a failed LM probe proves nothing.
        let lm_pool = &pool[..3];
        let map = served(&[(EngineKind::LmStudio, Some(lm_ids))]);
        assert_eq!(
            planner_fallback(&engines, lm_pool, &map, "mihai-withdrawn-27b"),
            Some((
                "http://lm.local:1234".to_string(),
                "gabee-qwen3.8-27b".to_string()
            ))
        );
        assert_eq!(
            planner_fallback(&engines, lm_pool, &map, "mihai-qwen3.8-27b"),
            None
        );
        let map = served(&[(EngineKind::LmStudio, None)]);
        assert_eq!(
            planner_fallback(&engines, lm_pool, &map, "mihai-withdrawn-27b"),
            None
        );
        // A planner carried by NO pool device is LM Studio by definition — the historical shape.
        let map = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, Some(&[alias])),
        ]);
        assert_eq!(
            planner_fallback(&engines, &pool, &map, "nowhere-planner"),
            Some((
                "http://lm.local:1234".to_string(),
                "gabee-qwen3.8-27b".to_string()
            ))
        );
    }

    /// The config contract of step B: a device yaml WITHOUT the engine key deserializes to None
    /// (= LM Studio) and serializes back WITHOUT the key — every existing config byte-identical.
    #[test]
    fn a_config_without_an_engine_key_roundtrips_byte_identically() {
        let yaml = "id: mac\nmodel_id: qwen/qwen3.6-35b-a3b\nweight: 2\nenabled: true\n";
        let d: SwarmDevice = serde_yaml::from_str(yaml).expect("existing configs still parse");
        assert_eq!(d.engine, None);
        assert_eq!(device_engine_kind(&d), EngineKind::LmStudio);
        let back = serde_yaml::to_string(&d).expect("serializes");
        assert!(
            !back.contains("engine"),
            "None must not serialize an engine key: {back}"
        );
        let e: SwarmDevice =
            serde_yaml::from_str(&format!("{yaml}engine: mlx-sidecar\n")).expect("tagged parses");
        assert_eq!(e.engine, Some(EngineKind::MlxSidecar));
    }
}
