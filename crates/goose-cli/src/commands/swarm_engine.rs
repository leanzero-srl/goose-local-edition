//! The mechanical engine boundary for the swarm's model-hosting runtime.
//!
//! `SwarmEngine` is the seam a second local engine (an MLX sidecar) plugs into NEXT TO LM Studio.
//! Step A (this file) is pure mechanics: the existing LM Studio free functions moved here
//! verbatim from swarm.rs, fronted by one trait object constructed unconditionally as
//! `LmStudioEngine`. Every body is byte-identical to its former swarm.rs self.

use std::process::Command as ProcCommand;
use std::sync::Arc;

use console::style;

use super::swarm::{device_from_lms_id, LmsProcess};

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
    /// Human-readable fleet probe printed to the console (`swarm pool probe`).
    fn probe_report(&self);
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
    fn probe_report(&self) {
        probe_fleet()
    }
}

// `resolve_lms` and `probe_lms_http` are pub(super) ONLY for swarm.rs's `probe_lms_processes`
// (the lms-ps-primary/HTTP-fallback composite that lives beside `parse_lms_ps` and its tests).
// Pulling that composite behind the boundary is step B; nothing else outside this file may
// reach past the trait.

/// Resolve the `lms` CLI binary. A Finder-launched desktop app does NOT inherit the shell PATH, so a bare
/// `lms` is not found — the GUI swarm bailed with "no models loaded" despite a loaded fleet. Check an
/// explicit override, then LM Studio's default install locations, else fall back to PATH.
pub(super) fn resolve_lms() -> String {
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

/// Discover loaded models straight from the LM Studio HTTP server (native /api/v0/models) — the fallback
/// for when the `lms` CLI is missing/unreachable (a Finder-launched desktop app has no lms on PATH). The
/// HTTP server MUST be up for the swarm to call the models at all, so it is the robust source. Uses `curl`
/// (a system binary present on the minimal GUI PATH) to avoid a blocking HTTP call inside the async
/// runtime. Returns loaded, non-embedding models as LmsProcess entries (device derived from the id prefix).
pub(super) fn probe_lms_http() -> Vec<LmsProcess> {
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
