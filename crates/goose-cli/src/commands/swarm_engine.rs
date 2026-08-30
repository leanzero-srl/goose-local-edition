//! The mechanical engine boundary for the swarm's model-hosting runtime.
//!
//! `SwarmEngine` is the seam a second local engine (an MLX sidecar) plugs into NEXT TO LM Studio.
//! Step A moved the existing LM Studio free functions here verbatim from swarm.rs, fronted by one
//! trait object. Step B (multi-engine generalization) added `EngineKind`, the `Engines` registry,
//! and the per-engine partition of the proven-negative pool semantics — still with LM Studio as
//! the only constructed engine (the MLX `SidecarEngine` impl is step C). Every moved body is
//! byte-identical to its former swarm.rs self.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::process::Command as ProcCommand;
use std::sync::Arc;

use console::style;

use super::swarm::{parse_lms_ps, LmsProcess, SwarmDevice};

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
/// plus zero-or-more named sidecar engines — none today; step C registers the MLX sidecar here.
/// Constructed once per run and threaded through the same path the single step-A engine object
/// took (run_swarm -> DispatcherRecipe -> GooseAgentDispatcher).
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

    /// The LM Studio engine directly — for the paths that are LM-Studio-specific by construction
    /// today (planner pre-warm, dispatch re-warm, the `lms ps` pool build). Step C revisits each.
    pub fn lmstudio(&self) -> Arc<dyn SwarmEngine> {
        self.lmstudio.clone()
    }

    /// The engine that hosts THIS device's model. A device naming an engine kind with no
    /// registered engine routes to LM Studio with a LOUD named absence-event — never silently
    /// (no config writer can produce that tag until step C, but a hand-edited config could).
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
