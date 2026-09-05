//! The mechanical engine boundary for the swarm's model-hosting runtime.
//!
//! `SwarmEngine` is the seam a second local engine (an MLX sidecar) plugs into NEXT TO LM Studio.
//! Step A moved the existing LM Studio free functions here verbatim from swarm.rs, fronted by one
//! trait object. Step B (multi-engine generalization) added `EngineKind`, the `Engines` registry,
//! and the per-engine partition of the proven-negative pool semantics. Step C registers a real
//! `SidecarEngine` (goose-sidecar's supervised Rapid-MLX process, dispatched through the
//! declarative `omlx` provider) — constructed ONLY when the config declares `mlx_engine` settings
//! AND a pool device is tagged for it, so an untagged pool stays byte-identical.

use anyhow::{anyhow, bail, Context, Result};
use goose_sidecar::engine::{EngineSettings, MlxEngineManager};
use goose_swarm::{DeviceCfg, DispatchRequest, EventSink};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::process::Command as ProcCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use console::style;

use super::swarm::{SamplingParams, SwarmConfig, SwarmDevice};

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
    /// Resident-model state straight from the engine's native catalog endpoint. `Err` = the
    /// probe could not answer (unreachable, refused, unparseable); `Ok(empty)` = it answered
    /// that nothing is loaded.
    fn catalog_probe(&self) -> Result<Vec<LmsProcess>>;
    /// The model ids the endpoint will actually SERVE. `None` means the probe itself failed —
    /// NOT "no models" — and callers must never gate on it (see `endpoint_model_ids` below).
    fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>>;
    /// The named probe absences recorded since the last drain (`lm-probe-unauthorized` class):
    /// facts about why a probe could not answer, for the run to write to run.jsonl. An engine
    /// that records none drains empty.
    fn take_probe_absences(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }
    /// Currently-loaded instance count for a model across the fleet.
    fn loaded_instance_count(&self, model_id: &str) -> usize;
    /// JIT warm-up: ensure up to `instances` copies are loaded, never more than already present.
    /// `Err` = the ENGINE ITSELF says the model cannot be mounted this run (the sidecar's mount
    /// failed, its model dir is unconfigured, or the engine call could not be driven) — a proven
    /// negative on the device's own engine, which `prewarm_pool` returns for the caller to name
    /// and exclude. LM Studio's `lms load` failing is NOT one (LM Link may hold the model on
    /// another node; `loaded_instance_count` is fleet-wide) — it is a named absence
    /// (`lms-load-failed`) and `Ok`.
    fn ensure_loaded(&self, model_id: &str, instances: u32) -> Result<()>;
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

impl EngineKind {
    /// The kind's config/event spelling (the serde name): "lmstudio" / "mlx-sidecar".
    pub(super) fn name(self) -> &'static str {
        match self {
            EngineKind::LmStudio => "lmstudio",
            EngineKind::MlxSidecar => "mlx-sidecar",
        }
    }
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

    /// Drain every registered engine's named probe absences — the run writes them to run.jsonl
    /// at its two probe seams (the pre-sink pool build, then `live_fleet_slots` per phase).
    pub(super) fn take_probe_absences(&self) -> Vec<serde_json::Value> {
        let mut out = self.lmstudio.take_probe_absences();
        for e in self.sidecars.values() {
            out.extend(e.take_probe_absences());
        }
        out
    }

    /// The engine that hosts THIS device's model — `None` for a device naming an engine kind
    /// nobody registered (tagged mlx-sidecar without `mlx_engine` config). The pre-step-C arm
    /// that routed such a device to LM Studio is gone: `lms load <sidecar-alias>` can never mount
    /// a sidecar, and `merge_sidecar_devices` keeps such a device out of the pool with a named
    /// event, so a None here is a caller that let one through (the allow_model_load bootstrap
    /// copies the configured list wholesale) and must say so.
    pub fn engine_for_device(&self, d: &SwarmDevice) -> Option<Arc<dyn SwarmEngine>> {
        self.for_kind(device_engine_kind(d))
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
            drop_unservable_devices(part, served.get(&kind).and_then(|o| o.as_ref()));
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
        all_resident_unservable(part, served.get(kind).and_then(|o| o.as_ref()))
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
///
/// One None is NOT unproven: a planner carried by a device whose engine KIND nobody registered
/// (tagged mlx-sidecar, config key `mlx_engine` missing/unparseable) will never mount — there is
/// no engine to warm it and no provider to dispatch it. Kept as "unproven" it stayed the pinned
/// planner, `engine_models` omitted it, `provider_for` fell to the shared lmstudio provider and
/// every planning call failed as text: the planner "planned from nothing". That arm falls back
/// to the first pool device whose engine IS registered, naming the missing engine.
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
    let Some(engine) = engines.for_kind(planner_kind) else {
        let alt = fleet_pool
            .iter()
            .find(|d| engines.engine_for_device(d).is_some())?
            .model_id
            .clone();
        return Some((
            format!(
                "{} (no engine registered — config key \"mlx_engine\" absent or unparseable)",
                planner_kind.name()
            ),
            alt,
        ));
    };
    let set = served.get(&planner_kind)?.as_ref()?;
    if set.contains(planner_model) {
        return None;
    }
    let alt = fleet_pool.first()?.model_id.clone();
    Some((engine.http_host(), alt))
}

/// One entry per SLOT the fleet can actually run, not one per device.
///
/// `fanout_over_fleet` sizes its permits from the list it is given, so a caller that collapses each
/// device to a single `model_id` silently caps every planning-phase fan at the DEVICE count. On this
/// fleet that is 3 where EXECUTE runs 6: `pick_device` admits a task while `d.in_flight < d.weight`
/// (baked default 2), so the plan phase was the only phase forbidden from using half the machine.
///
/// MEASURED from `detail_completed` spans in baseline-n3-r0: the detail fan spends its time at
/// concurrency {1: 34.3s, 2: 95.7s, 3: 112.4s} with a makespan of 244s, and never sustains 4 — the
/// same ceiling in every 3-node cell. On the 1-node arm it is worse: `swarm-1node-r0` detailed 17
/// items strictly serially, 1743.1s of a 5842.9s run, on a device whose weight is 2.
///
/// This is the same node-vs-slot substitution `00563c6ea` fixed for the planner's width prompt, which
/// the fan-outs were not fixed with.
///
/// ⚠ TAKES `DeviceCfg`, NOT `SwarmDevice`. Both carry a `weight` field and they are DIFFERENT
/// TYPES: `SwarmDevice` (swarm.rs) is the config-file shape, `DeviceCfg` (scheduler.rs) is what the
/// resolved runtime pool is built into and what every fan-out site actually holds. Writing this
/// against the config type compiled fine in isolation and failed at all five call sites.
///
/// ⚠ THE SKELETON DRAFT VOTE IS DELIBERATELY NOT AFFECTED, and must stay that way. `draft_models`
/// (see the dedup at the best-of-N site) folds this list through `HashSet::insert`, so duplicates
/// collapse back to distinct models and the number of drafts is unchanged. That dedup exists because
/// duplicate draft slots were MEASURED dying — 6 requested, exactly 3 survived, the duplicates
/// returning 158B/54B/162B — and its comment says plainly "Dedup is the fix; a length cap can never
/// be." Widening the vote is a separate experiment that needs the fleet, not a ride-along on a
/// concurrency change.
///
/// The LIVE-fleet variant every fan actually calls is `swarm_engine::live_fleet_slots` (per-engine
/// residency, this snapshot as its fallback).
pub(super) fn fleet_slot_models(devices: &[DeviceCfg]) -> Vec<String> {
    devices
        .iter()
        .flat_map(|d| std::iter::repeat_n(d.model_id.clone(), (d.weight as usize).max(1)))
        .collect()
}

/// The fan's slot list, checked against the LIVE fleet instead of the boot snapshot — PER ENGINE.
///
/// `fleet_slot_models(devices)` is computed once from the pool resolved at run start, and every fanned
/// phase — coverage, research, review, the test angles, the fix wave — used that snapshot. Only BUILD saw
/// a change, through the scheduler's `DeviceAdmission`. So a machine that died after the pool was
/// resolved kept being handed work for the rest of the run: each fan gave it a slot, the call failed, and
/// the slot was wasted while the surviving nodes queued.
///
/// CLOUD DEVICES ARE NEVER RESIDENCY-CHECKED. A cloud model does not appear in `lms ps` and never will,
/// so treating absence as death would delete the entire cloud half of a mixed fleet. That is not a
/// hypothetical: it is the version of this function I wrote first and reverted, and `DeviceCfg.is_cloud`
/// exists precisely so the question can be asked only where it has an answer.
///
/// EACH DEVICE IS JUDGED ONLY BY ITS OWN ENGINE'S PROBE, and a partition whose probe cannot answer
/// keeps its snapshot entries. The single-union shape this replaces (LM Studio residents ∪ sidecar
/// catalog, then "empty ⇒ snapshot") stopped falling back the moment a sidecar served anything: an
/// LM Studio probe hiccup (`lms ps` empty and the curl --max-time 6 fallback timing out — an
/// `Ok(empty)`, never an `Err`, which is why the old `let Ok(..) else` guard was unreachable) left
/// the union holding only the sidecar alias, and every LM Studio device vanished from every fan
/// for that call. `DeviceCfg` carries no engine tag (a goose-swarm type), so the dispatcher's
/// `engine_models` names the devices on a registered non-default engine; absent = LM Studio by
/// definition. Order is the original slot order; a result that would leave the fan with no slots
/// at all still falls back to the whole snapshot — a fan on a stale-but-working list is a small
/// inefficiency, a fan on an empty one is a dead phase.
pub(super) fn live_fleet_slots(
    devices: &[DeviceCfg],
    engines: &Engines,
    engine_models: &HashMap<String, EngineKind>,
    sink: &dyn EventSink,
) -> Vec<String> {
    let snapshot = fleet_slot_models(devices);
    let kind_of = |d: &DeviceCfg| {
        engine_models
            .get(&d.model_id)
            .copied()
            .unwrap_or(EngineKind::LmStudio)
    };
    // One residency probe per engine KIND in the pool. `None` = that engine proved nothing: its
    // probe errored, answered nothing, or no engine is registered for the kind (an unregistered
    // kind never reaches engine_models, so that arm is the honest spelling of "nobody to ask").
    // Neither failure shape is folded quietly: an `Err` is the probe saying WHY it could not
    // answer (curl missing, the server refusing or unreachable) and reaches run.jsonl as
    // `fleet-probe-failed{engine, error}`; an answered-EMPTY catalog is a proven negative the
    // arithmetic still treats as unproven (a fan on the snapshot beats a fan on nothing) and is
    // named `fleet-residency-empty{engine}`. Both once per fan, since each fan probes each kind
    // once. The slot arithmetic is unchanged: both kinds keep their snapshot entries.
    let mut proven: HashMap<EngineKind, Option<HashSet<String>>> = HashMap::new();
    for d in devices.iter().filter(|d| !d.is_cloud) {
        let kind = kind_of(d);
        proven.entry(kind).or_insert_with(|| {
            match engines.for_kind(kind).map(|e| e.resident_processes()) {
                Some(Ok(procs)) if !procs.is_empty() => {
                    Some(procs.into_iter().map(|p| p.identifier).collect())
                }
                Some(Ok(_)) => {
                    sink.write_value(serde_json::json!({
                        "event": "fleet-residency-empty",
                        "engine": kind.name(),
                    }));
                    None
                }
                Some(Err(e)) => {
                    sink.write_value(serde_json::json!({
                        "event": "fleet-probe-failed",
                        "engine": kind.name(),
                        "error": format!("{e:#}"),
                    }));
                    None
                }
                None => None,
            }
        });
    }
    // The probes' own named absences (`lm-probe-unauthorized`, first seen HERE when `lms ps`
    // answered nothing and the HTTP fallback was refused) ride to run.jsonl the same way.
    for ev in engines.take_probe_absences() {
        sink.write_value(ev);
    }
    let live: Vec<String> = devices
        .iter()
        .filter(|d| {
            d.is_cloud
                || match proven.get(&kind_of(d)) {
                    Some(Some(resident)) => resident.contains(&d.model_id),
                    _ => true,
                }
        })
        .flat_map(|d| std::iter::repeat_n(d.model_id.clone(), (d.weight as usize).max(1)))
        .collect();
    if live.is_empty() {
        // The fan runs on the stale snapshot rather than on nothing — said, not silent.
        sink.write_value(serde_json::json!({
            "event": "fleet-slots-snapshot-fallback",
            "reason": if devices.is_empty() {
                "the pool has no devices"
            } else {
                "every device's model is absent from its engine's answered catalog"
            },
            "snapshot_len": snapshot.len(),
        }));
        snapshot
    } else {
        live
    }
}

/// The request-body extras for a LOCAL model call, spelled in the serving ENGINE's own names.
///
/// The sampling knobs went on the wire under LM Studio's names whatever engine served the model.
/// rapid-mlx (0.13.1, models.py:1588) reads `repetition_penalty`, has no `repeat_penalty`, and
/// its request model is Pydantic `extra=ignore` — so a sidecar device's repeat penalty was
/// dropped silently with a 200, and `lm_extra_body` (an LM Studio body by definition) rode along
/// with it. Per engine: LM Studio devices get the previous block byte-for-byte; a sidecar device
/// gets the rapid-mlx key and no LM Studio body. The openai-format prefill/force-tool keys are
/// goose's own request-param protocol (consumed by the provider, not the server) and apply to
/// both.
pub(super) fn local_request_params(
    kind: EngineKind,
    sampling: &SamplingParams,
    lm_extra_body: Option<serde_json::Map<String, serde_json::Value>>,
    force_tool_until_act: Option<&str>,
    prefill_assistant: Option<&str>,
) -> HashMap<String, serde_json::Value> {
    let mut extra = HashMap::new();
    if kind == EngineKind::LmStudio {
        if let Some(body) = lm_extra_body {
            for (k, v) in body {
                extra.insert(k, v);
            }
        }
    }
    if let Some(v) = sampling.top_p {
        extra.insert("top_p".to_string(), serde_json::json!(v));
    }
    if let Some(v) = sampling.top_k {
        extra.insert("top_k".to_string(), serde_json::json!(v));
    }
    if let Some(v) = sampling.min_p {
        extra.insert("min_p".to_string(), serde_json::json!(v));
    }
    if let Some(v) = sampling.repeat_penalty {
        let key = match kind {
            EngineKind::LmStudio => "repeat_penalty",
            EngineKind::MlxSidecar => "repetition_penalty",
        };
        extra.insert(key.to_string(), serde_json::json!(v));
    }
    if let Some(tool) = force_tool_until_act.filter(|t| !t.is_empty()) {
        extra.insert(
            goose_provider_types::formats::openai::FORCE_TOOL_UNTIL_ACT_KEY.to_string(),
            serde_json::json!(tool),
        );
    }
    if let Some(text) = prefill_assistant.filter(|t| !t.is_empty()) {
        extra.insert(
            goose_provider_types::formats::openai::PREFILL_ASSISTANT_KEY.to_string(),
            serde_json::json!(text),
        );
    }
    extra
}

/// The LM Studio engine. Construct through `default_engine` so every call site shares the seam
/// a second engine slots into. Carries the run's named probe absences: an HTTP probe that LM
/// Studio refuses for want of a token (`lm-probe-unauthorized`) is said ONCE per engine object —
/// one per run, one per `swarm pool` command — and drained by the run into run.jsonl.
#[derive(Default)]
pub struct LmStudioEngine {
    unauthorized_said: AtomicBool,
    /// Models whose `lms load` failure was already named this run (`lms-load-failed` is said
    /// once per model per engine object).
    lms_load_failed_said: Mutex<HashSet<String>>,
    absences: Mutex<Vec<serde_json::Value>>,
}

pub fn default_engine() -> Arc<dyn SwarmEngine> {
    Arc::new(LmStudioEngine::default())
}

impl SwarmEngine for LmStudioEngine {
    fn provider_name(&self) -> &'static str {
        "lmstudio"
    }
    fn http_host(&self) -> String {
        lms_http_host()
    }
    fn catalog_probe(&self) -> Result<Vec<LmsProcess>> {
        self.probe_lms_http_at(&lms_http_host(), lm_api_token().as_deref())
    }
    fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
        self.endpoint_model_ids_at(&lms_http_host(), lm_api_token().as_deref())
    }
    fn loaded_instance_count(&self, model_id: &str) -> usize {
        loaded_instance_count(model_id)
    }
    fn ensure_loaded(&self, model_id: &str, instances: u32) -> Result<()> {
        self.ensure_loaded_lms(model_id, instances);
        Ok(())
    }
    fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
        self.probe_lms_processes()
    }
    fn probe_report(&self) {
        self.probe_fleet()
    }
    fn take_probe_absences(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.absences.lock().unwrap())
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

/// The key the CHAT path authenticates LM Studio with: the declarative `lmstudio` provider
/// (goose-providers `declarative/definitions/lmstudio.json`, `api_key_env`, `requires_auth:
/// false`) resolves it through `ConfigKeyResolver` → `Config::get_secret`, i.e. the environment
/// first, the goose secret store second. The probes read the SAME key by the SAME resolver, so
/// what they send is exactly what the dispatcher's provider sends.
pub(super) const LM_API_TOKEN_KEY: &str = "LMSTUDIO_API_KEY";

/// The LM Studio API token as the chat path would resolve it, or `None` when none is configured
/// (an unauthenticated server needs none; an authenticated one answers 401, which the probes
/// NAME — see `LmStudioEngine::note_unauthorized`). A store read that fails for a reason other
/// than absence is said on stderr, never folded into a quiet None.
fn lm_api_token() -> Option<String> {
    match goose::config::Config::global().get_secret::<String>(LM_API_TOKEN_KEY) {
        Ok(k) if !k.trim().is_empty() => Some(k),
        Ok(_) | Err(goose::config::ConfigError::NotFound(_)) => None,
        Err(e) => {
            eprintln!(
                "lm-token-unreadable: {LM_API_TOKEN_KEY} could not be read from the goose secret \
                 store ({e}) — probing LM Studio without a bearer"
            );
            None
        }
    }
}

/// The probe's curl argv: silent; a transport max-time (a dead endpoint is transport, never
/// model work); the HTTP status on its own last line so a refusal is classified by STATUS and
/// not by guessing at the body; and the bearer header iff a token exists — the same
/// `Authorization: Bearer` the chat path sends.
fn probe_curl_args(url: &str, token: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = ["-s", "--max-time", "6", "-w", "\n%{http_code}"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(t) = token {
        args.push("-H".to_string());
        args.push(format!("Authorization: Bearer {t}"));
    }
    args.push(url.to_string());
    args
}

/// One HTTP catalog probe's outcome, classified by the status curl reported.
#[derive(Debug)]
enum LmProbe {
    /// 2xx with a JSON body.
    Answered(serde_json::Value),
    /// 401/403: the server wants a token the probe did not carry, or rejected the one it did.
    Unauthorized,
    /// curl could not run, the server was unreachable, a non-2xx other than a refusal, or a
    /// body that was not JSON — the probe cannot answer.
    Failed(String),
}

fn classify_probe_output(stdout: &[u8]) -> LmProbe {
    let text = String::from_utf8_lossy(stdout);
    let Some((body, code)) = text.rsplit_once('\n') else {
        return LmProbe::Failed("curl wrote no status line".to_string());
    };
    let status: u16 = match code.trim().parse() {
        Ok(s) => s,
        Err(_) => return LmProbe::Failed(format!("unparseable HTTP status '{}'", code.trim())),
    };
    match status {
        0 => LmProbe::Failed("unreachable (no HTTP response)".to_string()),
        401 | 403 => LmProbe::Unauthorized,
        200..=299 => match serde_json::from_str::<serde_json::Value>(body) {
            Ok(v) => LmProbe::Answered(v),
            Err(e) => LmProbe::Failed(format!("HTTP {status} but the body was not JSON: {e}")),
        },
        other => LmProbe::Failed(format!("HTTP {other}")),
    }
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

/// The CANONICAL node name for a model served by `kind` — what run.jsonl's `node` field carries
/// and what every fleet surface groups by. LM Studio names are `device_from_lms_id` byte for
/// byte (`workhorse-qwen3.8-27b` → `workhorse`). A NON-LM-Studio engine carries its kind in the
/// name (`workhorse-qwen3.5-9b-4bit-mlx` on the sidecar → `workhorse-mlx`): the sidecar and LM
/// Studio on the same host are two feeds, and collapsing both to `workhorse` made the desktop's
/// fleet corroboration and FLEET rows conflate them (Q3). An LM Studio canonical name never
/// contains a dash (it is the segment before the first one), so the suffixed form can never
/// collide with one.
pub(super) fn canonical_node_name(kind: EngineKind, model_id: &str) -> Option<String> {
    let node = device_from_lms_id(model_id)?;
    Some(match kind {
        EngineKind::LmStudio => node,
        EngineKind::MlxSidecar => format!("{node}-mlx"),
    })
}

/// The engine KIND serving `model_id`: the first pool device carrying it (the resolved pool
/// first, the configured list second — the pushed planner device is in neither by id, but its
/// model is). No device carries it = LM Studio by definition (`None` = LmStudio), not a fallback.
pub(super) fn engine_kind_of_model(
    enabled: &[SwarmDevice],
    configured: &[SwarmDevice],
    model_id: &str,
) -> EngineKind {
    enabled
        .iter()
        .chain(configured.iter())
        .find(|d| d.model_id == model_id)
        .map(device_engine_kind)
        .unwrap_or(EngineKind::LmStudio)
}

impl LmStudioEngine {
    /// One HTTP catalog probe of `url` on `host`, carrying `token` as the chat path would. A
    /// refusal (401/403) is recorded through `note_unauthorized`; the caller still sees only an
    /// unproven outcome — a refused probe is never a proven negative.
    fn lm_probe(&self, url: &str, host: &str, token: Option<&str>) -> LmProbe {
        let probe = match ProcCommand::new("curl")
            .args(probe_curl_args(url, token))
            .output()
        {
            Ok(out) => classify_probe_output(&out.stdout),
            Err(e) => LmProbe::Failed(format!("curl could not run: {e}")),
        };
        if matches!(probe, LmProbe::Unauthorized) {
            self.note_unauthorized(host, token.is_some());
        }
        probe
    }

    /// The named absence for a refused probe, said ONCE per engine object: the yellow stderr line
    /// and an `lm-probe-unauthorized{host, token_key, token_present}` event for run.jsonl. Every
    /// later refusal in the same run is the same fact and stays quiet.
    fn note_unauthorized(&self, host: &str, token_present: bool) {
        if self.unauthorized_said.swap(true, Ordering::SeqCst) {
            return;
        }
        let why = if token_present {
            format!("the {LM_API_TOKEN_KEY} it carried was rejected")
        } else {
            format!("no {LM_API_TOKEN_KEY} is set in the environment or the goose secret store")
        };
        eprintln!(
            "{}",
            style(format!(
                "lm-probe-unauthorized: {host} wants an API token ({why}) — its residency and \
                 servability probes cannot answer this run, so every LM Studio device stays \
                 UNPROVEN (never dropped, never proven servable); set {LM_API_TOKEN_KEY} to the \
                 token LM Studio's server settings show"
            ))
            .yellow()
            .bold()
        );
        self.absences.lock().unwrap().push(serde_json::json!({
            "event": "lm-probe-unauthorized",
            "host": host,
            "token_key": LM_API_TOKEN_KEY,
            "token_present": token_present,
        }));
    }

    /// Discover loaded models straight from the LM Studio HTTP server (native /api/v0/models) —
    /// the fallback for when the `lms` CLI is missing/unreachable (a Finder-launched desktop app
    /// has no lms on PATH). The HTTP server MUST be up for the swarm to call the models at all,
    /// so it is the robust source. Uses `curl` (a system binary present on the minimal GUI PATH)
    /// to avoid a blocking HTTP call inside the async runtime. `Ok` carries the loaded,
    /// non-embedding models as LmsProcess entries (device derived from the id prefix) — empty
    /// when the server answered that nothing is loaded; `Err` when it could not answer (refused,
    /// unreachable, not JSON, no `data` list).
    fn probe_lms_http_at(&self, host: &str, token: Option<&str>) -> Result<Vec<LmsProcess>> {
        let url = format!("{}/api/v0/models", host.trim_end_matches('/'));
        let json = match self.lm_probe(&url, host, token) {
            LmProbe::Answered(v) => v,
            LmProbe::Unauthorized => {
                bail!("{url}: HTTP 401 — LM Studio wants an API token ({LM_API_TOKEN_KEY})")
            }
            LmProbe::Failed(why) => bail!("{url}: {why}"),
        };
        let arr = json
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("{url}: answered without a `data` list"))?;
        Ok(arr
            .iter()
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
            .collect())
    }

    /// The model ids the ENDPOINT will actually serve — i.e. the only ids a worker can dispatch to.
    ///
    /// `None` means the probe itself failed (endpoint down, token refused, curl missing,
    /// unparseable body). That is NOT the same as "no models", and the caller must never gate on
    /// it: an instrument reporting zero has been wrong seven times in this project, and gating a
    /// whole run off a failed probe would be the eighth. A refusal is additionally NAMED (once)
    /// through `note_unauthorized` — measured 2026-09-01 on the 3-node fleet: LM Studio answered
    /// `401 invalid_api_key` to a bare probe, so `served[LmStudio]` was None on every run and
    /// every servability consumer sat permanently "unproven" with no event saying why.
    ///
    /// WHY THIS IS NOT `lms ps`: `lms ps` lists what is RESIDENT; `/v1/models` lists what is
    /// SERVABLE, and they disagree in exactly the case that costs a run. MEASURED 2026-07-17:
    /// `lms ps` showed `workhorse-qwopus3.6-27b-coder-mlx` IDLE and loaded, while POSTing to it
    /// returned `400 Invalid model identifier` — the Mac Studio had dropped off the LAN and LM
    /// Link had withdrawn the alias, but the resident list still carried it. The pool is built
    /// from `lms ps`, so the swarm cheerfully dispatched a third of its tasks into an instant 400.
    fn endpoint_model_ids_at(&self, host: &str, token: Option<&str>) -> Option<HashSet<String>> {
        let url = format!("{}/v1/models", host.trim_end_matches('/'));
        let LmProbe::Answered(json) = self.lm_probe(&url, host, token) else {
            return None;
        };
        let arr = json.get("data")?.as_array()?;
        let ids: HashSet<String> = arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string))
            .collect();
        if ids.is_empty() {
            return None;
        }
        Some(ids)
    }

    fn probe_lms_processes(&self) -> Result<Vec<LmsProcess>> {
        // Primary: the `lms` CLI (richest — carries DEVICE + PARALLEL). Resolve its real path
        // since a Finder-launched app has no lms on PATH.
        let mut lms_answered_empty = false;
        if let Ok(out) = ProcCommand::new(resolve_lms()).arg("ps").output() {
            if out.status.success() {
                if let Ok(procs) = parse_lms_ps(&String::from_utf8_lossy(&out.stdout)) {
                    if !procs.is_empty() {
                        return Ok(procs);
                    }
                    lms_answered_empty = true;
                }
            }
        }
        // Fallback: the LM Studio HTTP server (no lms CLI needed), with the chat path's token.
        match self.probe_lms_http_at(&lms_http_host(), lm_api_token().as_deref()) {
            Ok(procs) => Ok(procs),
            // `lms ps` itself ANSWERED — an empty table is a measured empty, and the HTTP call
            // was only corroboration (a refusal was named above). Its failure does not overturn
            // the CLI's answer. Only when lms was missing or failing too is the probe an Err.
            Err(_) if lms_answered_empty => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    fn probe_fleet(&self) {
        println!("\n{}", style("lms ps:").bold());
        match ProcCommand::new(resolve_lms()).arg("ps").output() {
            Ok(out) => print!("{}", String::from_utf8_lossy(&out.stdout)),
            Err(e) => println!("  (lms ps failed: {e})"),
        }
        println!("{}", style("endpoint model ids:").bold());
        let host = lms_http_host();
        let token = lm_api_token();
        let url = format!("{}/v1/models", host.trim_end_matches('/'));
        match self.lm_probe(&url, &host, token.as_deref()) {
            LmProbe::Answered(v) => {
                for id in v
                    .get("data")
                    .and_then(|d| d.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                {
                    println!("  {id}");
                }
            }
            LmProbe::Unauthorized => println!(
                "  (HTTP 401 — {host} wants an API token; {})",
                if token.is_some() {
                    format!("the {LM_API_TOKEN_KEY} set here was rejected")
                } else {
                    format!("set {LM_API_TOKEN_KEY}")
                }
            ),
            LmProbe::Failed(why) => println!("  (probe failed: {why})"),
        }
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

/// `exit N: <first 200 chars of stderr>` for a subprocess that ran and failed — the fact a caller
/// names instead of discarding the `Output`.
fn subprocess_failure(out: &std::process::Output) -> String {
    let code = out
        .status
        .code()
        .map_or_else(|| "killed by a signal".to_string(), |c| format!("exit {c}"));
    let stderr: String = String::from_utf8_lossy(&out.stderr)
        .trim()
        .chars()
        .take(200)
        .collect();
    if stderr.is_empty() {
        code
    } else {
        format!("{code}: {stderr}")
    }
}

impl LmStudioEngine {
    /// Ensure up to `instances` copies of a model are loaded — and NEVER more than already
    /// present, so repeated runs / pre-warms don't stack duplicate instances (the cause of "3
    /// instances on one box"). Default `instances` is 1, so goose never spins up extras unless the
    /// user raises it. A `lms load` that cannot run or exits non-zero used to be discarded; it is
    /// now a named absence (`lms-load-failed{model, error}`, once per model) — the loop itself is
    /// unchanged, every wanted copy is still attempted.
    fn ensure_loaded_lms(&self, model_id: &str, instances: u32) {
        let want = instances.max(1) as usize;
        let have = loaded_instance_count(model_id);
        for _ in have..want {
            let failure = match ProcCommand::new(resolve_lms())
                .args(["load", model_id, "-y", "--ttl", "3600"])
                .output()
            {
                Ok(out) if out.status.success() => None,
                Ok(out) => Some(format!("lms load {}", subprocess_failure(&out))),
                Err(e) => Some(format!("lms could not run: {e}")),
            };
            if let Some(error) = failure {
                self.note_lms_load_failed(model_id, &error);
            }
        }
    }

    /// The named absence for a failed warm-up, said ONCE per model per engine object: a yellow
    /// stderr line and an `lms-load-failed{model, error}` event drained into run.jsonl with the
    /// other probe absences. Repeats for the same model (the re-warm on a transient) stay quiet.
    fn note_lms_load_failed(&self, model_id: &str, error: &str) {
        if !self
            .lms_load_failed_said
            .lock()
            .unwrap()
            .insert(model_id.to_string())
        {
            return;
        }
        eprintln!(
            "{}",
            style(format!(
                "lms-load-failed: `lms load {model_id}` failed ({error}) — the model is not \
                 warmed by goose this run; its devices dispatch against whatever LM Studio holds"
            ))
            .yellow()
            .bold()
        );
        self.absences.lock().unwrap().push(serde_json::json!({
            "event": "lms-load-failed",
            "model": model_id,
            "error": error,
        }));
    }
}

// ---------------------------------------------------------------------------------------------
// Step C: the MLX sidecar engine — goose-sidecar's supervised Rapid-MLX process behind the trait
// ---------------------------------------------------------------------------------------------

/// Why a sidecar engine call could not be driven from the calling thread — typed, so a caller
/// reports it by name instead of the panic `block_in_place` raises inside a `current_thread`
/// runtime (acp/provider.rs builds one; a sync trait call from there used to abort the process).
#[derive(Debug)]
pub enum EngineCallError {
    /// The caller sits inside a `current_thread` tokio runtime, where `block_in_place` panics
    /// and `Handle::block_on` from the runtime's own thread cannot make progress.
    CurrentThreadRuntime,
    /// No runtime is running and a throwaway one could not be built.
    RuntimeBuild(std::io::Error),
}

impl std::fmt::Display for EngineCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentThreadRuntime => write!(
                f,
                "called from inside a current_thread tokio runtime — a sidecar engine call needs \
                 a multi-thread runtime (block_in_place) or no runtime at all"
            ),
            Self::RuntimeBuild(e) => write!(f, "could not build a tokio runtime: {e}"),
        }
    }
}

impl std::error::Error for EngineCallError {}

/// Drive an engine-manager future from the SYNC trait surface. Inside a multi-thread runtime
/// (the run pipeline / dispatcher — goose-cli's runtime is multi-thread) `block_in_place` keeps
/// the worker thread legal; outside one (unit tests, sync menu paths) a throwaway runtime drives
/// it; inside a `current_thread` runtime the answer is a typed error, never a panic.
fn block_on_engine<F: std::future::Future>(fut: F) -> Result<F::Output, EngineCallError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            if matches!(
                handle.runtime_flavor(),
                tokio::runtime::RuntimeFlavor::CurrentThread
            ) {
                return Err(EngineCallError::CurrentThreadRuntime);
            }
            Ok(tokio::task::block_in_place(|| handle.block_on(fut)))
        }
        Err(_) => Ok(tokio::runtime::Runtime::new()
            .map_err(EngineCallError::RuntimeBuild)?
            .block_on(fut)),
    }
}

/// The MLX sidecar: one supervised Rapid-MLX process serving one mounted model, OpenAI-compat on
/// 127.0.0.1:{port}, dispatched through the declarative `omlx` provider (OMLX_HOST). Every probe
/// reads the LIVE `/v1/models` catalog — facts only, nothing fabricated.
pub struct SidecarEngine {
    manager: Arc<MlxEngineManager>,
    base_url: String,
    /// The catalog probe's failure, said ONCE per engine object on the paths that must read it
    /// as "cannot answer" (see `note_probe_failure`).
    probe_failed_said: AtomicBool,
    absences: Mutex<Vec<serde_json::Value>>,
}

/// One `curl -sS --max-time 6 <url>` run against the sidecar's `/v1/models`, classified into the
/// fact it is: `Err` names WHY the catalog could not answer — curl exited non-zero (exit 7 is a
/// refused connection = the engine is not listening; exit 28 a timeout), an empty body, or a body
/// that is not JSON (a 503/404 HTML page) — each with the first 200 chars of what curl said. A
/// body that PARSES is `Ok` whatever it holds: `{"data":[]}` or a JSON error object is the engine
/// answering, a proven negative for the callers to read, never an error.
fn classify_v1_models_output(url: &str, out: &std::process::Output) -> Result<serde_json::Value> {
    if !out.status.success() {
        let meaning = match out.status.code() {
            Some(7) => " (connection refused — nothing is listening; the engine is down)",
            Some(28) => " (timed out — the engine did not answer)",
            _ => "",
        };
        bail!("{url}: curl {}{meaning}", subprocess_failure(out));
    }
    if out.stdout.iter().all(u8::is_ascii_whitespace) {
        bail!("{url}: curl exited 0 with an empty body");
    }
    serde_json::from_slice::<serde_json::Value>(&out.stdout).map_err(|e| {
        let head: String = String::from_utf8_lossy(&out.stdout)
            .trim()
            .chars()
            .take(200)
            .collect();
        anyhow!("{url}: body was not JSON ({e}): {head}")
    })
}

impl SidecarEngine {
    pub fn new(manager: Arc<MlxEngineManager>) -> Self {
        let base_url = format!("http://127.0.0.1:{}", manager.settings().port);
        Self {
            manager,
            base_url,
            probe_failed_said: AtomicBool::new(false),
            absences: Mutex::new(Vec::new()),
        }
    }

    /// GET {base_url}/v1/models via curl — the same subprocess idiom as the LM Studio probes
    /// (a blocking HTTP client inside the async runtime is the trap both avoid). `Err` carries
    /// the named reason the catalog could not answer (`classify_v1_models_output`).
    fn v1_models(&self) -> Result<serde_json::Value> {
        let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
        let out = ProcCommand::new("curl")
            .args(["-sS", "--max-time", "6", &url])
            .output()
            .with_context(|| format!("spawning curl for {url}"))?;
        classify_v1_models_output(&url, &out)
    }

    /// (served id, context_window) per catalog entry. `Ok(empty)` = the engine ANSWERED and serves
    /// nothing (or its answer carries no `data` list) — a proven negative; `Err` = it could not
    /// answer, with the reason.
    fn served_entries(&self) -> Result<Vec<(String, Option<u64>)>> {
        let json = self.v1_models()?;
        let Some(arr) = json.get("data").and_then(|v| v.as_array()) else {
            return Ok(Vec::new());
        };
        Ok(arr
            .iter()
            .filter_map(|m| {
                let id = m.get("id").and_then(|v| v.as_str())?.to_string();
                Some((id, m.get("context_window").and_then(|v| v.as_u64())))
            })
            .collect())
    }

    /// `served_entries` for the callers whose contract has no Err arm (servability, the instance
    /// count, the pre-warm fast path): a failure reads as `None` = "cannot answer" — exactly the
    /// unproven outcome those callers produced when the failure was folded into an empty list —
    /// and the reason is said once (`note_probe_failure`) instead of vanishing.
    fn served_entries_or_note(&self) -> Option<Vec<(String, Option<u64>)>> {
        match self.served_entries() {
            Ok(entries) => Some(entries),
            Err(e) => {
                self.note_probe_failure(&e);
                None
            }
        }
    }

    /// The named absence for a catalog probe that could not answer, said ONCE per engine object:
    /// the yellow stderr line and a `sidecar-probe-failed{host, error}` event for run.jsonl (drained
    /// through `take_probe_absences` at the pool build and at every fan). `catalog_probe` does NOT
    /// route through here — it propagates the Err, and `live_fleet_slots` names each failed probe
    /// as `fleet-probe-failed{engine, error}`.
    fn note_probe_failure(&self, e: &anyhow::Error) {
        if self.probe_failed_said.swap(true, Ordering::SeqCst) {
            return;
        }
        eprintln!(
            "{}",
            style(format!(
                "sidecar-probe-failed: the mlx-sidecar catalog at {} could not answer ({e:#}) — \
                 every mlx-sidecar device stays UNPROVEN (never dropped, never proven servable) \
                 until it does",
                self.base_url
            ))
            .yellow()
            .bold()
        );
        self.absences.lock().unwrap().push(serde_json::json!({
            "event": "sidecar-probe-failed",
            "host": self.base_url,
            "error": format!("{e:#}"),
        }));
    }
}

impl SwarmEngine for SidecarEngine {
    fn provider_name(&self) -> &'static str {
        "omlx"
    }
    fn http_host(&self) -> String {
        self.base_url.clone()
    }
    fn catalog_probe(&self) -> Result<Vec<LmsProcess>> {
        // `Err` = the catalog could not answer, with the reason — `live_fleet_slots` writes it as
        // `fleet-probe-failed{engine: "mlx-sidecar", error}`; `Ok(empty)` = it answered nothing.
        Ok(self
            .served_entries()?
            .into_iter()
            .map(|(id, context_window)| LmsProcess {
                // The sidecar's own feed, named apart from LM Studio's on the same host.
                device: canonical_node_name(EngineKind::MlxSidecar, &id),
                identifier: id,
                // A serving rapid-mlx holds its model resident — served IS loaded here.
                status: "loaded".to_string(),
                // rapid-mlx does not report a PARALLEL figure; absent, never invented.
                parallel: None,
                loaded_context_length: context_window,
            })
            .collect())
    }
    fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
        // Identical None semantics to LM Studio's probe: empty/unreachable is "cannot answer",
        // never "no models" — the per-engine guard treats it as unproven. The unreachable case
        // now says why, once (`note_probe_failure`).
        let ids: HashSet<String> = self
            .served_entries_or_note()?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        if ids.is_empty() {
            return None;
        }
        Some(ids)
    }
    fn loaded_instance_count(&self, model_id: &str) -> usize {
        // One supervised process serves one mounted model: 1 iff the live catalog serves it; a
        // catalog that cannot answer counts 0, as before, and is named once.
        usize::from(
            self.served_entries_or_note()
                .is_some_and(|entries| entries.iter().any(|(id, _)| id == model_id)),
        )
    }
    fn ensure_loaded(&self, model_id: &str, _instances: u32) -> Result<()> {
        // Fast path: the live catalog already serves it — possibly mounted by ANOTHER process's
        // manager (the desktop window); mounting again would fight over the port.
        if self.loaded_instance_count(model_id) > 0 {
            return Ok(());
        }
        // `instances` is accepted-and-ignored: the supervisor owns one process serving one model.
        // TTL likewise — the supervisor owns the engine's lifetime, and rapid-mlx has its own
        // --resident-model-idle-ttl if that lever is ever wanted.
        let mut settings = self.manager.settings();
        let Some(hf_dir) = settings.model_id.clone() else {
            let why = format!(
                "engine-config-absent: mlx_engine.model_id is not set — cannot mount \
                 '{model_id}' (set the HF model directory id under config key \"mlx_engine\")"
            );
            eprintln!("{why}");
            bail!(why);
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
        // The stderr lines are unchanged; the SAME fact now also returns to the caller, so the
        // pre-warm seam can write it to run.jsonl and exclude the device (it was stderr-only, and
        // with loading ON nothing else excludes a sidecar device — `exclude_unmountable_sidecar_
        // devices` stands down — so a failed mount left the device pinned and every call to it
        // hit the refused port).
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                eprintln!("engine-mount-failed: {e:#}");
                Err(anyhow!("engine-mount-failed: {e:#}"))
            }
            Err(e) => {
                eprintln!("engine-call-unavailable: cannot mount '{model_id}' — {e}");
                Err(anyhow!(
                    "engine-call-unavailable: cannot mount '{model_id}' — {e}"
                ))
            }
        }
    }
    fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
        self.catalog_probe()
    }
    fn probe_report(&self) {
        println!("{}", style("mlx-sidecar:").bold());
        match block_on_engine(self.manager.status()) {
            Ok(status) => {
                println!("  state: {}", status.state);
                if let Some(m) = &status.model_id {
                    println!("  mounted: {m}");
                }
                if let Some(e) = &status.last_error {
                    println!("  last error: {e}");
                }
            }
            Err(e) => println!("  state: (unavailable — {e})"),
        }
        println!("  port: {}", self.manager.settings().port);
        match self.served_entries() {
            Ok(entries) => {
                for (id, ctx) in entries {
                    match ctx {
                        Some(c) => println!("  serves: {id} (context {c})"),
                        None => println!("  serves: {id}"),
                    }
                }
            }
            Err(e) => println!("  serves: (catalog probe failed: {e:#})"),
        }
    }
    fn take_probe_absences(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut *self.absences.lock().unwrap())
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

/// One pool device whose engine REFUSED to mount its model during the pre-warm — the proven
/// negative `prewarm_pool` hands back for the caller to name in run.jsonl and exclude.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MountFailure {
    pub device_id: String,
    pub model_id: String,
    pub engine: EngineKind,
    pub error: String,
}

/// JIT pre-warm for the resolved pool: each model warms through ITS OWN engine. The planner
/// warms through the engine of the POOL DEVICE that carries it — the LM-pinned planner arm this
/// replaces fired a doomed `lms load <sidecar-alias>` and could never mount the sidecar. A
/// planner carried by no pool device keeps the historical LM Studio warm-up byte-identically
/// (a sidecar planner outside the pool has no device to name its engine — left unresolved).
/// Returns every device whose engine refused the mount (`SwarmEngine::ensure_loaded` Err), one
/// entry per device — the planner warm and the device loop touch the same device, and a device
/// that fails once has failed. Empty on every happy path.
pub(super) fn prewarm_pool(
    engines: &Engines,
    enabled: &[SwarmDevice],
    planner_model: &str,
) -> Vec<MountFailure> {
    let mut failures: Vec<MountFailure> = Vec::new();
    let mut note = |d: &SwarmDevice, outcome: Result<()>| {
        if let Err(e) = outcome {
            if !failures.iter().any(|f| f.device_id == d.id) {
                failures.push(MountFailure {
                    device_id: d.id.clone(),
                    model_id: d.model_id.clone(),
                    engine: device_engine_kind(d),
                    error: format!("{e:#}"),
                });
            }
        }
    };
    if !enabled
        .iter()
        .any(|d| d.is_cloud() && d.model_id == planner_model)
    {
        match enabled
            .iter()
            .find(|d| !d.is_cloud() && d.model_id == planner_model)
        {
            Some(d) => match engines.engine_for_device(d) {
                Some(e) => note(d, e.ensure_loaded(planner_model, 1)),
                None => eprintln!(
                    "engine-absent: planner '{planner_model}' is carried by device '{}' on engine \
                     '{:?}' but no such engine is registered — not warmed",
                    d.id,
                    device_engine_kind(d)
                ),
            },
            // LM Studio's warm-up never errs (its `lms load` failure is a named absence); the
            // discarded Ok keeps this arm byte-identical.
            None => {
                let _ = engines.lmstudio().ensure_loaded(planner_model, 1);
            }
        }
    }
    for d in enabled.iter().filter(|d| !d.is_cloud()) {
        match engines.engine_for_device(d) {
            Some(e) => note(d, e.ensure_loaded(&d.model_id, d.instances)),
            None => eprintln!(
                "engine-absent: device '{}' names engine '{:?}' but no such engine is registered \
                 — not warmed",
                d.id,
                device_engine_kind(d)
            ),
        }
    }
    failures
}

/// A device whose mount FAILED during the pre-warm leaves the pool by name, through the same
/// shape as the S-M6 exclusion (`sidecar-device-excluded{id, reason}`), preceded by the failure
/// itself (`engine-mount-failed{device, model_id, engine, error}`). Mild: only the named device
/// goes; the caller decides what an emptied pool means. Returns the run.jsonl events in order.
pub(super) fn exclude_mount_failed_devices(
    pool: &mut Vec<SwarmDevice>,
    failures: &[MountFailure],
) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    for f in failures {
        eprintln!(
            "{}",
            style(format!(
                "sidecar-device-excluded: '{}' — its engine ({}) refused to mount '{}' during \
                 the pre-warm ({}); nothing will serve it this run",
                f.device_id,
                f.engine.name(),
                f.model_id,
                f.error
            ))
            .yellow()
            .bold()
        );
        events.push(serde_json::json!({
            "event": "engine-mount-failed",
            "device": f.device_id,
            "model_id": f.model_id,
            "engine": f.engine.name(),
            "error": f.error,
        }));
        events.push(serde_json::json!({
            "event": "sidecar-device-excluded",
            "id": f.device_id,
            "reason": format!("mount-failed: {}", f.error),
        }));
        pool.retain(|d| d.id != f.device_id);
    }
    events
}

/// The pre-warm SEAM of run_swarm (loading ON): warm the pool, then settle what the engines said.
/// A device whose engine REFUSED the mount is a proven negative on its own engine — with loading
/// ON `exclude_unmountable_sidecar_devices` stands down (the pre-warm IS the mount path), so
/// until this a failed mount was a stderr line and the device stayed: on a one-device sidecar
/// pool it stayed pinned as planner and every planning call hit the refused port. Now the failure
/// is named in run.jsonl (`exclude_mount_failed_devices`), the device leaves by name, a planner it
/// carried moves to the first remaining device (`planner-fallback{from, to, reason}`), and an
/// EMPTIED pool is a named refusal — never a dispatch into a dead port. Mild: only the failed
/// device leaves; no failures → the pool returns unchanged, byte for byte.
pub(super) fn settle_prewarm(
    engines: &Engines,
    enabled: Vec<SwarmDevice>,
    planner_model: &mut String,
    sink: &dyn EventSink,
) -> Result<Vec<SwarmDevice>> {
    let failures = prewarm_pool(engines, &enabled, planner_model);
    let mut pool = enabled;
    for ev in exclude_mount_failed_devices(&mut pool, &failures) {
        sink.write_value(ev);
    }
    if failures.is_empty() {
        return Ok(pool);
    }
    if pool.is_empty() {
        let named: Vec<String> = failures
            .iter()
            .map(|f| format!("{} ({})", f.device_id, f.error))
            .collect();
        bail!(
            "Every device in the pool refused to mount during the pre-warm — [{}] — so NONE of the \
             pool is servable; dispatching now would fail every call into a refused port. Start the \
             engine (or fix its mount), then re-run.",
            named.join("; ")
        );
    }
    if let Some(f) = failures.iter().find(|f| f.model_id == *planner_model) {
        let alt = pool[0].model_id.clone();
        eprintln!(
            "{}",
            style(format!(
                "planner '{planner_model}' is NOT servable by {} — its device '{}' refused to \
                 mount; falling back to '{alt}'",
                f.engine.name(),
                f.device_id
            ))
            .yellow()
            .bold()
        );
        sink.write_value(serde_json::json!({
            "event": "planner-fallback",
            "from": planner_model.clone(),
            "to": alt,
            "reason": format!("mount-failed: {}", f.error),
        }));
        *planner_model = alt;
    }
    Ok(pool)
}

/// The dispatcher's best-effort re-warm before a transient re-dispatch, routed through the
/// model's OWN engine — `lms load` on an mlx-sidecar model would warm the wrong runtime. Absent
/// from `engine_models` = the default LM Studio engine, definitionally. An engine that refuses
/// the mount is `engine-mount-failed{…, task_id, attempt, site}` in run.jsonl — recorded, never
/// acted on here (the scheduler alone decides retry/fail). The `None` arm is a NET by
/// construction: `engine_models` only names kinds with a registered engine and the LM Studio slot
/// is always filled — the same absence `prewarm_pool` names.
pub(super) fn rewarm_on_transient(
    engines: &Engines,
    engine_models: &HashMap<String, EngineKind>,
    events: &dyn EventSink,
    req: &DispatchRequest,
) {
    let kind = engine_models
        .get(&req.model_id)
        .copied()
        .unwrap_or(EngineKind::LmStudio);
    match engines.for_kind(kind) {
        Some(engine) => {
            if let Err(e) = engine.ensure_loaded(&req.model_id, 1) {
                events.write_value(serde_json::json!({
                    "event": "engine-mount-failed",
                    "device": req.device_id,
                    "model_id": req.model_id,
                    "engine": kind.name(),
                    "error": format!("{e:#}"),
                    "task_id": req.task_id,
                    "attempt": req.attempt,
                    "site": "re-warm on transient",
                }));
            }
        }
        None => {
            eprintln!(
                "engine-absent: model '{}' names engine '{:?}' but no such engine is registered \
                 — not re-warmed",
                req.model_id, kind
            );
            events.write_value(serde_json::json!({
                "event": "engine-absent",
                "model_id": req.model_id,
                "engine": kind.name(),
                "task_id": req.task_id,
                "attempt": req.attempt,
                "site": "re-warm on transient",
            }));
        }
    }
}

fn short_model(identifier: &str) -> String {
    identifier
        .rsplit('/')
        .next()
        .unwrap_or(identifier)
        .to_lowercase()
        .chars()
        .take(28)
        .collect()
}

/// A pool entry's id for a discovered model: `<device>-<short model>`, de-duplicated against the
/// configured devices with a numeric suffix. No device label (empty means exactly that — the
/// probe row carried none) yields the bare short model.
pub(super) fn gen_entry_id(cfg: &SwarmConfig, device: Option<&str>, identifier: &str) -> String {
    let dev = device
        .map(|d| d.split('.').next().unwrap_or(d).to_lowercase())
        .unwrap_or_default();
    let base = if dev.is_empty() {
        short_model(identifier)
    } else {
        format!("{dev}-{}", short_model(identifier))
    };
    let mut id = base.clone();
    let mut n = 2;
    while cfg.devices.iter().any(|d| d.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// The `swarm pool import` half of the pool build: every loaded model the engine reports becomes a
/// pool entry (the run-time twin is `reconcile_pool_with_fleet`).
pub(super) struct ImportSummary {
    pub(super) added: Vec<SwarmDevice>,
    pub(super) skipped_existing: Vec<String>,
    /// (model_id, kept_device, dropped_device) — same identifier loaded on two hosts.
    pub(super) skipped_collision: Vec<(String, String, String)>,
}

/// Add loaded models as pool entries. Dedup by identifier first (the SAME identifier on two hosts
/// cannot be routed by LM Link → keep the first, flag the rest), then skip identifiers already pooled.
pub(super) fn import_processes(
    cfg: &mut SwarmConfig,
    procs: &[LmsProcess],
    default_weight: u32,
    enabled: bool,
) -> ImportSummary {
    let mut summary = ImportSummary {
        added: Vec::new(),
        skipped_existing: Vec::new(),
        skipped_collision: Vec::new(),
    };
    let mut kept: HashMap<String, String> = HashMap::new();
    for p in procs {
        let dev_label = p.device.clone().unwrap_or_else(|| "?".to_string());
        if let Some(prev) = kept.get(&p.identifier) {
            summary
                .skipped_collision
                .push((p.identifier.clone(), prev.clone(), dev_label));
            continue;
        }
        kept.insert(p.identifier.clone(), dev_label);
        if cfg.devices.iter().any(|d| d.model_id == p.identifier) {
            summary.skipped_existing.push(p.identifier.clone());
            continue;
        }
        let dev = SwarmDevice {
            id: gen_entry_id(cfg, p.device.as_deref(), &p.identifier),
            model_id: p.identifier.clone(),
            weight: default_weight.max(1),
            enabled,
            instances: 1,
            host: p.device.clone(),
            provider: None,
            speed_weight: None,
            supervision: None,
            engine: None,
        };
        cfg.devices.push(dev.clone());
        summary.added.push(dev);
    }
    summary
}

pub(super) fn print_import_summary(s: &ImportSummary) {
    for d in &s.added {
        println!(
            "  {} {:<14} {}{}",
            style("+ added").green().bold(),
            style(&d.id).bold(),
            style(&d.model_id).dim(),
            d.host
                .as_deref()
                .map(|h| format!("  @{h}"))
                .unwrap_or_default()
        );
    }
    for m in &s.skipped_existing {
        println!("  {} {} (already in pool)", style("· skip").dim(), m);
    }
    for (m, keep, drop) in &s.skipped_collision {
        println!(
            "  {} {} on {} — same model_id already taken by {} (LM Link can't distinguish)",
            style("! collision").red().bold(),
            m,
            drop,
            keep
        );
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
        Err(e) => {
            eprintln!(
                "{}",
                style(format!(
                    "fleet-probe-failed: the LM Studio residency probe could not answer ({e:#}) \
                     — no LM Studio device discovered this run; the pool falls to the configured \
                     devices (allow_model_load) or the cloud nodes"
                ))
                .yellow()
                .bold()
            );
            return (Vec::new(), None);
        }
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
///
/// A tagged device whose engine is NOT registered (config key `mlx_engine` missing or unparseable
/// — `engines_for_run` already said so) is a guaranteed-dead device: its probe answers None (never
/// dropped), its provider resolves to lmstudio, and every dispatch to it fails. It stays OUT of
/// the pool; its id is returned so the caller can write the named absence
/// (`sidecar-device-excluded`) to run.jsonl. Mild: the run proceeds on the devices that can work.
pub(super) fn merge_sidecar_devices(
    pool: &mut Vec<SwarmDevice>,
    configured: &[SwarmDevice],
    engines: &Engines,
) -> Vec<String> {
    let mut excluded = Vec::new();
    let registered = engines.for_kind(EngineKind::MlxSidecar).is_some();
    for d in configured
        .iter()
        .filter(|d| d.enabled && d.engine == Some(EngineKind::MlxSidecar))
    {
        if !registered {
            eprintln!(
                "engine-absent: device '{}' names engine 'mlx-sidecar' but no sidecar engine is \
                 registered — excluded from this run's pool (set config key \"mlx_engine\")",
                d.id
            );
            excluded.push(d.id.clone());
            continue;
        }
        if !pool.iter().any(|p| p.id == d.id) {
            eprintln!(
                "  · sidecar node: {} → {} via mlx-sidecar",
                d.id, d.model_id
            );
            pool.push(d.clone());
        }
    }
    excluded
}

/// One sidecar device that leaves the pool before dispatch, and the measured reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SidecarExclusion {
    /// The engine serves NOTHING (its probe answered None) and loading is off: nobody will mount
    /// it this run.
    Unmounted { id: String },
    /// The engine ANSWERED with a catalog that lacks this device's alias, and loading is off: a
    /// negative PROVEN by the device's own engine on the same object — the process is up,
    /// serving `serving`, and nothing will remount it as `wanted`.
    ServesOtherAlias {
        id: String,
        wanted: String,
        serving: Vec<String>,
    },
}

impl SidecarExclusion {
    fn id(&self) -> &str {
        match self {
            SidecarExclusion::Unmounted { id } | SidecarExclusion::ServesOtherAlias { id, .. } => {
                id
            }
        }
    }
}

/// The tying fact S-M7 lacked: a declared sidecar device that nobody can mount this run leaves
/// the pool here, BEFORE the planner-keep guard (a sidecar planner on an unmountable device
/// would otherwise survive as the pinned planner and every planning call would fail). Two
/// measured shapes, both only while model loading is OFF (the pre-warm is the only mount path
/// and allow_model_load gates it):
/// - the engine serves NOTHING (probe None) → `Unmounted`;
/// - the engine serves OTHER aliases (probe Some, lacking this device's model_id) →
///   `ServesOtherAlias` — the pool half of S-H3: `drop_unservable_devices` would KEEP such a
///   device when it is its partition's only member (the never-empties-the-pool rule reads an
///   all-unservable partition as a broken probe), so a wrong-alias sidecar stayed in the pool
///   with no event and every dispatch to it failed.
///
/// With loading ON the partition is untouched — the pre-warm mounts/remounts it. Mild: never a
/// refusal of the run; `sidecar_exclusion_events` turns each exclusion into its stderr line and
/// run.jsonl event.
pub(super) fn exclude_unmountable_sidecar_devices(
    pool: &mut Vec<SwarmDevice>,
    served: &HashMap<EngineKind, Option<HashSet<String>>>,
    allow_model_load: bool,
) -> Vec<SidecarExclusion> {
    if allow_model_load {
        return Vec::new();
    }
    let Some(partition) = served.get(&EngineKind::MlxSidecar) else {
        return Vec::new();
    };
    let mut gone = Vec::new();
    let mut keep = Vec::new();
    for d in pool.drain(..) {
        if device_engine_kind(&d) != EngineKind::MlxSidecar {
            keep.push(d);
            continue;
        }
        match partition {
            Some(set) if set.contains(&d.model_id) => keep.push(d),
            Some(set) if !set.is_empty() => {
                let mut serving: Vec<String> = set.iter().cloned().collect();
                serving.sort();
                gone.push(SidecarExclusion::ServesOtherAlias {
                    id: d.id,
                    wanted: d.model_id,
                    serving,
                });
            }
            _ => gone.push(SidecarExclusion::Unmounted { id: d.id }),
        }
    }
    *pool = keep;
    gone
}

/// The stderr line and the run.jsonl event for each sidecar exclusion: one grouped
/// `sidecar-unmounted-and-load-disabled{devices}` (byte-identical to the S-M7 shape) and one
/// `sidecar-device-serves-other-alias{id, serving, wanted}` per wrong-alias device. The caller
/// writes the events right after `run_started`.
pub(super) fn sidecar_exclusion_events(exclusions: &[SidecarExclusion]) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    let unmounted: Vec<String> = exclusions
        .iter()
        .filter(|x| matches!(x, SidecarExclusion::Unmounted { .. }))
        .map(|x| x.id().to_string())
        .collect();
    if !unmounted.is_empty() {
        eprintln!(
            "{}",
            style(format!(
                "sidecar-unmounted-and-load-disabled: [{}] — the mlx-sidecar serves nothing and \
                 allow_model_load is off, so nothing will mount them this run; mount the sidecar \
                 first or enable loading via `goose swarm pool`",
                unmounted.join(", ")
            ))
            .yellow()
            .bold()
        );
        events.push(serde_json::json!({
            "event": "sidecar-unmounted-and-load-disabled",
            "devices": unmounted,
        }));
    }
    for x in exclusions {
        let SidecarExclusion::ServesOtherAlias {
            id,
            wanted,
            serving,
        } = x
        else {
            continue;
        };
        eprintln!(
            "{}",
            style(format!(
                "sidecar-device-serves-other-alias: '{id}' wants '{wanted}' but the mlx-sidecar is \
                 serving [{}] and allow_model_load is off, so nothing will remount it this run — \
                 out of the pool; mount '{wanted}' or enable loading via `goose swarm pool`",
                serving.join(", ")
            ))
            .yellow()
            .bold()
        );
        events.push(serde_json::json!({
            "event": "sidecar-device-serves-other-alias",
            "id": id,
            "serving": serving,
            "wanted": wanted,
        }));
    }
    events
}

/// Drop devices the endpoint will not serve, so a dead node cannot silently eat a third of the run.
///
/// THE FAILURE THIS PREVENTS, measured end-to-end on a 71-minute run that produced nothing:
/// the `frontend` task was dispatched to a model the endpoint had withdrawn. Every attempt came back
/// `400 Invalid model identifier` in ~2s. But `run_agent_in` returns Ok for a provider error — the 400 lands
/// as the agent's *text* — so the dispatcher saw only "worker finished, owned files absent" and retried with
/// "You finished WITHOUT writing your owned file(s)". Three attempts, 6.8 seconds, zero tool calls, task
/// failed. `integrate-verify` depends on every task, so it never became ready, and the run ended with
/// passed=false having never built the app. The engine blamed the model for a network outage.
///
/// Returns the surviving devices and the dropped (id, model_id) pairs.
///
/// NEVER EMPTIES THE POOL. If every device would be dropped, the probe disagrees with `lms ps` about
/// literally everything, and the likeliest explanation is a broken probe — not a fleet that is 100% dead.
/// Keep the pool, report nothing dropped, and let the run proceed as it did before this function existed.
pub(super) fn drop_unservable_devices(
    devices: Vec<SwarmDevice>,
    served: Option<&std::collections::HashSet<String>>,
) -> (Vec<SwarmDevice>, Vec<(String, String)>) {
    let Some(served) = served else {
        return (devices, Vec::new());
    };
    let (keep, drop): (Vec<SwarmDevice>, Vec<SwarmDevice>) = devices
        .into_iter()
        .partition(|d| served.contains(&d.model_id));
    if keep.is_empty() {
        return (drop, Vec::new());
    }
    let dropped = drop.into_iter().map(|d| (d.id, d.model_id)).collect();
    (keep, dropped)
}

/// PROVEN-zero-servable: refuse ONLY when the `/v1/models` catalog demonstrably WORKS (it returned a non-empty
/// list — a positive control on the SAME endpoint) yet lists NONE of the resident pool's models. That is a
/// negative PROVEN on the same object, not an observed-empty: `endpoint_model_ids` collapses an empty/unreachable
/// probe to `None`, and `served == None` never refuses. So this can only fire when every resident alias has been
/// withdrawn (the LM Link node dropped off the LAN) — the exact case where proceeding dispatches a third (or all)
/// of the run into instant 400s and a dead run. It can NEVER authorize a bad dispatch — only stop one — so it is
/// structurally incapable of the false-"there are models" mistake.
pub(super) fn all_resident_unservable(
    pool: &[SwarmDevice],
    served: Option<&std::collections::HashSet<String>>,
) -> bool {
    match served {
        Some(s) if !s.is_empty() && !pool.is_empty() => {
            pool.iter().all(|d| !s.contains(&d.model_id))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Q3: the sidecar and LM Studio on ONE host are two feeds. LM Studio canonical names are
    /// `device_from_lms_id` byte for byte; a non-LM-Studio engine's name carries its kind, so
    /// `workhorse-qwen3.8-27b` (LM Studio) and `workhorse-qwen3.5-9b-4bit-mlx` (sidecar) no
    /// longer collapse to one `workhorse`.
    #[test]
    fn a_non_lmstudio_engines_canonical_node_name_carries_its_kind() {
        assert_eq!(
            canonical_node_name(EngineKind::LmStudio, "workhorse-qwen3.8-27b").as_deref(),
            Some("workhorse")
        );
        assert_eq!(
            canonical_node_name(EngineKind::LmStudio, "mihai-qwen/qwen3.8-27b").as_deref(),
            device_from_lms_id("mihai-qwen/qwen3.8-27b").as_deref(),
            "LM Studio names are byte-identical to device_from_lms_id"
        );
        assert_eq!(
            canonical_node_name(EngineKind::MlxSidecar, "workhorse-qwen3.5-9b-4bit-mlx").as_deref(),
            Some("workhorse-mlx")
        );
        assert_ne!(
            canonical_node_name(EngineKind::MlxSidecar, "workhorse-qwen3.5-9b-4bit-mlx"),
            canonical_node_name(EngineKind::LmStudio, "workhorse-qwen3.8-27b"),
            "two feeds on one host must not share a name"
        );
        assert_eq!(
            canonical_node_name(EngineKind::MlxSidecar, "solomodel"),
            None
        );
        // The kind behind a model id: the resolved pool first, the configured list second (the
        // pushed planner is in neither by id but its model is), LM Studio by definition otherwise.
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let configured = vec![dev("workhorse-mlx", alias, Some(EngineKind::MlxSidecar))];
        let enabled = vec![dev("mac-gabee", "gabee-qwen3.8-27b", None)];
        assert_eq!(
            engine_kind_of_model(&enabled, &configured, alias),
            EngineKind::MlxSidecar
        );
        assert_eq!(
            engine_kind_of_model(&enabled, &configured, "gabee-qwen3.8-27b"),
            EngineKind::LmStudio
        );
        assert_eq!(
            engine_kind_of_model(&enabled, &configured, "nowhere-planner"),
            EngineKind::LmStudio
        );
    }

    /// Golden rapid-mlx `/v1/models` body — the exact field shape the manager's own
    /// probe_model_info reads (data[].id / context_window / tool_call_parser).
    const GOLDEN_V1_MODELS: &str = r#"{"object":"list","data":[{"id":"workhorse-qwen3-coder-30b-mlx","object":"model","context_window":262144,"tool_call_parser":"qwen3_coder"}]}"#;

    /// One-thread HTTP stub serving a fixed body on every request (each probe curls separately).
    /// The detached accept loop lives for the test binary — harmless on an ephemeral port.
    fn serve_stub(body: &'static str) -> u16 {
        serve_stub_status("200 OK", body)
    }

    /// The same stub with a chosen status line — a 401 stub is how the LM Studio refusal is
    /// reproduced without a token-guarded server.
    fn serve_stub_status(status: &'static str, body: &'static str) -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        port
    }

    #[test]
    fn probe_curl_args_carry_the_bearer_only_when_a_token_exists() {
        let url = "http://h:1234/v1/models";
        let with = probe_curl_args(url, Some("tok-1"));
        let pos = with.iter().position(|a| a == "-H").expect("-H present");
        assert_eq!(with[pos + 1], "Authorization: Bearer tok-1");
        assert_eq!(with.last().map(String::as_str), Some(url));
        let without = probe_curl_args(url, None);
        assert!(
            !without
                .iter()
                .any(|a| a == "-H" || a.starts_with("Authorization")),
            "no token, no header: {without:?}"
        );
        assert!(
            without
                .windows(2)
                .any(|w| w[0] == "-w" && w[1] == "\n%{http_code}"),
            "the status rides on its own last line so a refusal is classified by status"
        );
    }

    #[test]
    fn a_probe_answer_is_classified_by_status_never_by_body_shape() {
        assert!(matches!(
            classify_probe_output(br#"{"error":{"code":"invalid_api_key"}}"#.iter().chain(b"\n401").copied().collect::<Vec<u8>>().as_slice()),
            LmProbe::Unauthorized
        ));
        assert!(matches!(
            classify_probe_output(b"\n000"),
            LmProbe::Failed(_)
        ));
        assert!(matches!(
            classify_probe_output(b"not json\n200"),
            LmProbe::Failed(_)
        ));
        assert!(matches!(
            classify_probe_output(b"{\"data\":[]}\n200"),
            LmProbe::Answered(_)
        ));
        assert!(matches!(
            classify_probe_output(b"{\"data\":[]}\n503"),
            LmProbe::Failed(_)
        ));
        assert!(matches!(classify_probe_output(b""), LmProbe::Failed(_)));
    }

    /// The 3-node fleet's shape on 2026-09-01: LM Studio answers `401 invalid_api_key` to a bare
    /// probe. The servable probe stays None (unproven — never a proven negative), the residency
    /// fallback errs with the reason, the absence is NAMED exactly once per engine object, and a
    /// catalog that answers to the bearer proves the set.
    #[test]
    fn a_401_from_lm_studio_is_a_named_absence_once_and_never_a_proven_negative() {
        const UNAUTHORIZED: &str = r#"{"error":{"type":"invalid_request","code":"invalid_api_key","message":"An LM Studio API token is required to make requests to this server"}}"#;
        let port = serve_stub_status("401 Unauthorized", UNAUTHORIZED);
        let host = format!("http://127.0.0.1:{port}");
        let engine = LmStudioEngine::default();
        assert_eq!(engine.endpoint_model_ids_at(&host, None), None);
        let err = engine
            .probe_lms_http_at(&host, None)
            .expect_err("a refused residency probe is an Err, not an empty fleet");
        assert!(err.to_string().contains("401"), "{err}");
        let absences = engine.take_probe_absences();
        assert_eq!(
            absences.len(),
            1,
            "two refusals, ONE named absence: {absences:?}"
        );
        assert_eq!(absences[0]["event"], "lm-probe-unauthorized");
        assert_eq!(absences[0]["host"], host);
        assert_eq!(absences[0]["token_key"], LM_API_TOKEN_KEY);
        assert_eq!(absences[0]["token_present"], false);
        assert!(engine.take_probe_absences().is_empty(), "drained");
        assert_eq!(
            engine.endpoint_model_ids_at(&host, Some("wrong-token")),
            None
        );
        assert!(
            engine.take_probe_absences().is_empty(),
            "said once per engine object, never per probe"
        );

        const FLEET: &str = r#"{"object":"list","data":[{"id":"gabee-qwen3.8-27b"},{"id":"mihai-qwen3.8-27b"},{"id":"workhorse-qwen3.8-27b"}]}"#;
        let port = serve_stub_status("200 OK", FLEET);
        let host = format!("http://127.0.0.1:{port}");
        let engine = LmStudioEngine::default();
        let served = engine
            .endpoint_model_ids_at(&host, Some("tok"))
            .expect("a catalog that answers proves the set");
        assert_eq!(served.len(), 3);
        assert!(served.contains("workhorse-qwen3.8-27b"));
        assert!(engine.take_probe_absences().is_empty());
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
        assert_eq!(
            procs[0].device.as_deref(),
            Some("workhorse-mlx"),
            "the sidecar's feed is named apart from LM Studio's on the same host"
        );
        assert_eq!(procs[0].parallel, None, "never invented");
        assert_eq!(procs[0].loaded_context_length, Some(262_144));
    }

    /// A dead sidecar answers None — the identical "cannot answer" semantics of the LM probe —
    /// and now SAYS so once: `sidecar-probe-failed{host, error}` through the probe-absence
    /// channel (the consumer is the run's drain into run.jsonl), while `catalog_probe` carries
    /// the same reason as an Err for `live_fleet_slots` to name per fan.
    #[test]
    fn sidecar_probe_failure_is_none_never_empty() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        let eng = sidecar_on(port);
        assert_eq!(eng.servable_model_ids(), None);
        assert_eq!(eng.loaded_instance_count("anything"), 0);
        let err = eng
            .resident_processes()
            .expect_err("a refused connection is an Err, never an empty catalog");
        assert!(
            format!("{err:#}").contains("exit 7"),
            "the curl exit is named: {err:#}"
        );
        let absences = eng.take_probe_absences();
        assert_eq!(
            absences.len(),
            1,
            "said once per engine object: {absences:?}"
        );
        assert_eq!(absences[0]["event"], "sidecar-probe-failed");
        assert_eq!(absences[0]["host"], format!("http://127.0.0.1:{port}"));
        assert!(
            absences[0]["error"]
                .as_str()
                .is_some_and(|e| e.contains("exit 7")),
            "the reason rides with the event: {absences:?}"
        );
        assert!(eng.take_probe_absences().is_empty(), "drained");
    }

    fn curl_output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// The pure classification behind `v1_models`: a refused connection names curl's exit, an
    /// answered `{"data":[]}` is Ok (a proven negative), a non-JSON body names itself, an empty
    /// body is named as such.
    #[test]
    fn v1_models_output_is_classified_by_name() {
        let url = "http://127.0.0.1:8090/v1/models";
        let refused = classify_v1_models_output(
            url,
            &curl_output(
                7,
                "",
                "curl: (7) Failed to connect to 127.0.0.1 port 8090 after 0 ms: Couldn't connect to server",
            ),
        )
        .expect_err("exit 7 is a failure");
        let text = format!("{refused:#}");
        assert!(text.contains("exit 7"), "{text}");
        assert!(text.contains("connection refused"), "{text}");
        assert!(
            text.contains("Failed to connect"),
            "stderr rides along: {text}"
        );

        let empty = classify_v1_models_output(url, &curl_output(0, r#"{"data":[]}"#, ""))
            .expect("an answered empty catalog is Ok");
        assert_eq!(empty["data"].as_array().map(Vec::len), Some(0));

        let garbage = classify_v1_models_output(
            url,
            &curl_output(0, "<html><body>503 Service Unavailable</body></html>", ""),
        )
        .expect_err("HTML is not a catalog");
        let text = format!("{garbage:#}");
        assert!(text.contains("not JSON"), "{text}");
        assert!(
            text.contains("503 Service Unavailable"),
            "the body head is quoted: {text}"
        );

        let blank = classify_v1_models_output(url, &curl_output(0, "  \n", ""))
            .expect_err("no body is not an answer");
        assert!(format!("{blank:#}").contains("empty body"));
    }

    /// ensure_loaded's fast path: already-served means NO mount attempt (the engine may belong
    /// to another process's manager — mounting again would fight over the port) and settings
    /// untouched.
    #[test]
    fn sidecar_ensure_loaded_fast_paths_when_already_served() {
        let port = serve_stub(GOLDEN_V1_MODELS);
        let eng = sidecar_on(port);
        eng.ensure_loaded("workhorse-qwen3-coder-30b-mlx", 1)
            .expect("already served is Ok");
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
        let err = eng
            .ensure_loaded("workhorse-qwen3-coder-30b-mlx", 1)
            .expect_err("an unconfigured model dir is a refused mount");
        assert!(
            format!("{err:#}").contains("engine-config-absent"),
            "the refusal is named for the caller: {err:#}"
        );
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
        /// What `resident_processes` answers — empty = the probe proved nothing.
        resident: Vec<&'static str>,
        /// Models whose `ensure_loaded` the double REFUSES (an engine that cannot mount them).
        refuse_mount: Vec<&'static str>,
        calls: std::sync::Mutex<Vec<(String, u32)>>,
    }
    impl RecordingEngine {
        fn new(name: &'static str) -> Arc<Self> {
            Self::with_host(name, "")
        }
        fn with_host(name: &'static str, host: &'static str) -> Arc<Self> {
            Self::serving(name, host, &[])
        }
        fn serving(name: &'static str, host: &'static str, resident: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                name,
                host,
                resident: resident.to_vec(),
                refuse_mount: Vec::new(),
                calls: std::sync::Mutex::new(Vec::new()),
            })
        }
        fn refusing(name: &'static str, refuse_mount: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                name,
                host: "",
                resident: Vec::new(),
                refuse_mount: refuse_mount.to_vec(),
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
        fn catalog_probe(&self) -> Result<Vec<LmsProcess>> {
            Ok(Vec::new())
        }
        fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
            None
        }
        fn loaded_instance_count(&self, _model_id: &str) -> usize {
            0
        }
        fn ensure_loaded(&self, model_id: &str, instances: u32) -> Result<()> {
            self.calls
                .lock()
                .expect("calls lock")
                .push((model_id.to_string(), instances));
            if self.refuse_mount.contains(&model_id) {
                bail!(
                    "engine-mount-failed: mlx engine mount failed for '{model_id}': uvx not found"
                );
            }
            Ok(())
        }
        fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
            Ok(self
                .resident
                .iter()
                .map(|id| LmsProcess {
                    identifier: id.to_string(),
                    status: "loaded".to_string(),
                    device: device_from_lms_id(id),
                    parallel: None,
                    loaded_context_length: None,
                })
                .collect())
        }
        fn probe_report(&self) {}
    }

    fn dev_cfg(id: &str, model: &str, weight: u32, is_cloud: bool) -> DeviceCfg {
        DeviceCfg {
            id: id.to_string(),
            model_id: model.to_string(),
            weight,
            enabled: true,
            speed_weight: 1,
            supervision: false,
            is_cloud,
        }
    }

    /// S-H2 pinned on the mixed pool: the LM Studio probe answers nothing (the `lms ps` +
    /// curl --max-time hiccup, an Ok(empty)) while the sidecar serves its alias. The old union
    /// held only the alias and every LM Studio device vanished from the fan; per engine, the LM
    /// partition is UNPROVEN and keeps its snapshot slots, the sidecar partition is filtered by
    /// its own catalog, and the original slot order survives.
    #[test]
    fn a_failed_lm_probe_keeps_the_lm_partition_while_the_sidecar_filters_its_own() {
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let mut engines = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        engines.register_sidecar(
            "mlx-sidecar",
            RecordingEngine::serving("omlx", "http://127.0.0.1:8899", &[alias]),
        );
        let devices = vec![
            dev_cfg("mac-gabee", "gabee-qwen3.8-27b", 2, false),
            dev_cfg("local-mihai", "mihai-qwen3.8-27b", 2, false),
            dev_cfg("works-workhorse", "workhorse-qwen3.8-27b", 2, false),
            dev_cfg("workhorse-mlx", alias, 1, false),
            dev_cfg("workhorse-mlx-stale", "workhorse-stale-alias-mlx", 1, false),
        ];
        let engine_models: HashMap<String, EngineKind> = [
            (alias.to_string(), EngineKind::MlxSidecar),
            (
                "workhorse-stale-alias-mlx".to_string(),
                EngineKind::MlxSidecar,
            ),
        ]
        .into_iter()
        .collect();
        let slots = live_fleet_slots(&devices, &engines, &engine_models, &goose_swarm::NullSink);
        assert_eq!(
            slots,
            vec![
                "gabee-qwen3.8-27b",
                "gabee-qwen3.8-27b",
                "mihai-qwen3.8-27b",
                "mihai-qwen3.8-27b",
                "workhorse-qwen3.8-27b",
                "workhorse-qwen3.8-27b",
                alias,
            ],
            "LM partition unproven → its 6 snapshot slots kept; the stale sidecar alias dropped by ITS probe"
        );
    }

    /// The mirror image and the all-LM shape: an answering LM Studio probe filters ITS devices
    /// (byte-identical to the single-engine behaviour), an unproven sidecar keeps its slot, a
    /// cloud device is never residency-checked, and a probe answering nothing on every engine
    /// is the whole snapshot.
    #[test]
    fn each_partition_is_judged_only_by_its_own_engines_probe() {
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let devices = vec![
            dev_cfg("mac-gabee", "gabee-qwen3.8-27b", 2, false),
            dev_cfg("local-mihai", "mihai-qwen3.8-27b", 1, false),
            dev_cfg("bedrock-1", "anthropic.claude", 1, true),
            dev_cfg("workhorse-mlx", alias, 1, false),
        ];
        let engine_models: HashMap<String, EngineKind> =
            [(alias.to_string(), EngineKind::MlxSidecar)]
                .into_iter()
                .collect();
        // LM Studio answers (mihai withdrawn); the sidecar probe answers nothing.
        let mut engines = Engines::with_lmstudio_for_tests(RecordingEngine::serving(
            "lmstudio",
            "",
            &["gabee-qwen3.8-27b"],
        ));
        engines.register_sidecar("mlx-sidecar", RecordingEngine::new("omlx"));
        assert_eq!(
            live_fleet_slots(&devices, &engines, &engine_models, &goose_swarm::NullSink),
            vec![
                "gabee-qwen3.8-27b",
                "gabee-qwen3.8-27b",
                "anthropic.claude",
                alias
            ],
            "LM filtered by its probe; cloud untouched; unproven sidecar keeps its slot"
        );
        // Every probe answers nothing → the whole snapshot, in order.
        let mut engines = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        engines.register_sidecar("mlx-sidecar", RecordingEngine::new("omlx"));
        assert_eq!(
            live_fleet_slots(&devices, &engines, &engine_models, &goose_swarm::NullSink),
            fleet_slot_models(&devices)
        );
        // All-LM pool (empty engine_models), LM answering: the single-engine filter, verbatim.
        let lm_only = &devices[..2];
        let engines = Engines::with_lmstudio_for_tests(RecordingEngine::serving(
            "lmstudio",
            "",
            &["mihai-qwen3.8-27b"],
        ));
        assert_eq!(
            live_fleet_slots(lm_only, &engines, &HashMap::new(), &goose_swarm::NullSink),
            vec!["mihai-qwen3.8-27b"]
        );
        // A proven probe that would strand the fan with zero slots still falls back to the snapshot.
        let engines = Engines::with_lmstudio_for_tests(RecordingEngine::serving(
            "lmstudio",
            "",
            &["something-else-entirely"],
        ));
        assert_eq!(
            live_fleet_slots(lm_only, &engines, &HashMap::new(), &goose_swarm::NullSink),
            fleet_slot_models(lm_only)
        );
    }

    /// The 2026-08-30 micro-run defect, pinned: a config-complete sidecar device (tagged, engine
    /// registered) MUST be pre-warmed through ITS OWN engine — and a planner carried by that
    /// device warms there too, never through a doomed `lms load` of the alias.
    /// An engine whose residency probe ERRS — the shape `probe_lms_processes` now produces when
    /// `lms` is missing/failing and the HTTP fallback is refused or unreachable.
    struct FailingProbeEngine;
    impl SwarmEngine for FailingProbeEngine {
        fn provider_name(&self) -> &'static str {
            "lmstudio"
        }
        fn http_host(&self) -> String {
            "http://lm.local:1234".to_string()
        }
        fn catalog_probe(&self) -> Result<Vec<LmsProcess>> {
            bail!("http://lm.local:1234/api/v0/models: HTTP 401 — LM Studio wants an API token")
        }
        fn servable_model_ids(&self) -> Option<std::collections::HashSet<String>> {
            None
        }
        fn loaded_instance_count(&self, _model_id: &str) -> usize {
            0
        }
        fn ensure_loaded(&self, _model_id: &str, _instances: u32) -> Result<()> {
            Ok(())
        }
        fn resident_processes(&self) -> Result<Vec<LmsProcess>> {
            self.catalog_probe()
        }
        fn probe_report(&self) {}
    }

    /// A sink that keeps every caller-side value it is handed, so a test can read run.jsonl.
    #[derive(Default)]
    struct RecordingSink(std::sync::Mutex<Vec<serde_json::Value>>);
    impl EventSink for RecordingSink {
        fn emit(&self, _event: &goose_swarm::SwarmEvent) {}
        fn write_value(&self, value: serde_json::Value) {
            self.0.lock().expect("sink lock").push(value);
        }
    }

    /// A residency probe that ERRS is a NAMED absence in run.jsonl (`fleet-probe-failed{engine,
    /// error}`), once per fan for that engine kind — never folded quietly into "unproven". The
    /// slot arithmetic is untouched: the errored kind keeps its snapshot slots, and a kind that
    /// answered still filters its own devices.
    #[test]
    fn a_failed_residency_probe_is_a_named_absence_in_the_run_log() {
        let mut engines = Engines::with_lmstudio_for_tests(Arc::new(FailingProbeEngine));
        engines.register_sidecar(
            "mlx-sidecar",
            RecordingEngine::serving("omlx", "http://127.0.0.1:8899", &["workhorse-mlx-9b"]),
        );
        let devices = vec![
            dev_cfg("mac-gabee", "gabee-qwen3.8-27b", 2, false),
            dev_cfg("works-workhorse", "workhorse-qwen3.8-27b", 2, false),
            dev_cfg("workhorse-mlx", "workhorse-mlx-9b", 1, false),
            dev_cfg("workhorse-mlx-stale", "workhorse-mlx-old", 1, false),
        ];
        let mut engine_models = HashMap::new();
        engine_models.insert("workhorse-mlx-9b".to_string(), EngineKind::MlxSidecar);
        engine_models.insert("workhorse-mlx-old".to_string(), EngineKind::MlxSidecar);
        let sink = RecordingSink::default();
        let slots = live_fleet_slots(&devices, &engines, &engine_models, &sink);
        assert_eq!(
            slots,
            vec![
                "gabee-qwen3.8-27b",
                "gabee-qwen3.8-27b",
                "workhorse-qwen3.8-27b",
                "workhorse-qwen3.8-27b",
                "workhorse-mlx-9b",
            ],
            "the errored LM kind keeps its snapshot; the sidecar filtered its own stale device"
        );
        let events = sink.0.lock().expect("sink lock").clone();
        assert_eq!(
            events.len(),
            1,
            "one probe per kind per fan → one event: {events:?}"
        );
        assert_eq!(events[0]["event"], "fleet-probe-failed");
        assert_eq!(events[0]["engine"], "lmstudio");
        assert!(
            events[0]["error"]
                .as_str()
                .is_some_and(|e| e.contains("401")),
            "the probe's own reason is carried: {events:?}"
        );
        // A kind that answered EMPTY keeps its snapshot slots (unproven arithmetic, unchanged) and
        // is named once per fan; a kind nobody registered stays a quiet None (nobody to ask).
        let quiet = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        let sink = RecordingSink::default();
        let slots = live_fleet_slots(&devices, &quiet, &engine_models, &sink);
        assert_eq!(
            slots,
            fleet_slot_models(&devices),
            "nothing proven → snapshot"
        );
        let events = sink.0.lock().expect("sink lock").clone();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "fleet-residency-empty");
        assert_eq!(events[0]["engine"], "lmstudio");
    }

    /// When every device is filtered by its own engine's answered catalog the fan falls back to
    /// the whole snapshot — as before — and says so: `fleet-slots-snapshot-fallback{reason,
    /// snapshot_len}`.
    #[test]
    fn the_snapshot_fallback_is_a_named_event() {
        let engines = Engines::with_lmstudio_for_tests(RecordingEngine::serving(
            "lmstudio",
            "",
            &["gabee-other-model"],
        ));
        let devices = vec![
            dev_cfg("mac-gabee", "gabee-qwen3.8-27b", 2, false),
            dev_cfg("works-workhorse", "workhorse-qwen3.8-27b", 1, false),
        ];
        let sink = RecordingSink::default();
        let slots = live_fleet_slots(&devices, &engines, &HashMap::new(), &sink);
        assert_eq!(slots, fleet_slot_models(&devices));
        let events = sink.0.lock().expect("sink lock").clone();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "fleet-slots-snapshot-fallback");
        assert_eq!(events[0]["snapshot_len"], 3);
        assert!(events[0]["reason"]
            .as_str()
            .is_some_and(|r| r.contains("absent from its engine")));
    }

    /// Item 7's shape: a sidecar device whose MOUNT FAILS during the pre-warm (engine down, uvx
    /// missing) is returned by `prewarm_pool` once — planner warm and device loop hit the same
    /// device — and `exclude_mount_failed_devices` names it (`engine-mount-failed`, then
    /// `sidecar-device-excluded{reason: "mount-failed: …"}`) and removes ONLY that device; a pool
    /// with no failures returns no events and is untouched (the happy path, byte-identical).
    #[test]
    fn a_failed_prewarm_mount_is_returned_named_and_excluded() {
        let lm = RecordingEngine::new("lmstudio");
        let sc = RecordingEngine::refusing("omlx", &["workhorse-alias-mlx"]);
        let mut engines = Engines::with_lmstudio_for_tests(lm);
        engines.register_sidecar("mlx-sidecar", sc);
        let mut pool = vec![
            dev(
                "workhorse-mlx",
                "workhorse-alias-mlx",
                Some(EngineKind::MlxSidecar),
            ),
            dev("mac-gabee", "gabee-qwen", None),
        ];
        let failures = prewarm_pool(&engines, &pool, "workhorse-alias-mlx");
        assert_eq!(failures.len(), 1, "one device, one failure: {failures:?}");
        assert_eq!(failures[0].device_id, "workhorse-mlx");
        assert_eq!(failures[0].engine, EngineKind::MlxSidecar);
        assert!(failures[0].error.contains("uvx not found"));
        let events = exclude_mount_failed_devices(&mut pool, &failures);
        assert_eq!(events.len(), 2, "{events:?}");
        assert_eq!(events[0]["event"], "engine-mount-failed");
        assert_eq!(events[0]["device"], "workhorse-mlx");
        assert_eq!(events[0]["engine"], "mlx-sidecar");
        assert_eq!(events[1]["event"], "sidecar-device-excluded");
        assert_eq!(events[1]["id"], "workhorse-mlx");
        assert!(events[1]["reason"]
            .as_str()
            .is_some_and(|r| r.starts_with("mount-failed: ")));
        assert_eq!(
            pool.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            vec!["mac-gabee"],
            "only the failed device leaves"
        );
        // The single-device shape of this machine: the pool empties — the caller refuses by name.
        let mut solo = vec![dev(
            "workhorse-mlx",
            "workhorse-alias-mlx",
            Some(EngineKind::MlxSidecar),
        )];
        let failures = prewarm_pool(&engines, &solo, "workhorse-alias-mlx");
        let _ = exclude_mount_failed_devices(&mut solo, &failures);
        assert!(solo.is_empty());
        // Happy path: nothing refused → nothing returned, pool untouched.
        let ok = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        let mut untouched = vec![dev("mac-gabee", "gabee-qwen", None)];
        let failures = prewarm_pool(&ok, &untouched, "gabee-qwen");
        assert!(failures.is_empty());
        assert!(exclude_mount_failed_devices(&mut untouched, &failures).is_empty());
        assert_eq!(untouched.len(), 1);
    }

    /// The seam itself: a mixed pool loses only the refused device and its planner moves
    /// (`planner-fallback` at the sink); a one-device pool whose mount fails is a named Err; a
    /// pool with no failures returns unchanged with an empty sink.
    #[test]
    fn settle_prewarm_excludes_moves_the_planner_and_refuses_an_emptied_pool() {
        let mut engines = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        engines.register_sidecar(
            "mlx-sidecar",
            RecordingEngine::refusing("omlx", &["workhorse-alias-mlx"]),
        );
        let mixed = vec![
            dev(
                "workhorse-mlx",
                "workhorse-alias-mlx",
                Some(EngineKind::MlxSidecar),
            ),
            dev("mac-gabee", "gabee-qwen", None),
        ];
        let mut planner = "workhorse-alias-mlx".to_string();
        let sink = RecordingSink::default();
        let pool = settle_prewarm(&engines, mixed.clone(), &mut planner, &sink)
            .expect("mixed pool survives");
        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].id, "mac-gabee");
        assert_eq!(
            planner, "gabee-qwen",
            "the planner leaves the refused device"
        );
        let events = sink.0.lock().expect("sink lock").clone();
        let names: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();
        assert_eq!(
            names,
            vec![
                "engine-mount-failed",
                "sidecar-device-excluded",
                "planner-fallback"
            ]
        );
        assert_eq!(events[2]["from"], "workhorse-alias-mlx");
        assert_eq!(events[2]["to"], "gabee-qwen");

        let mut planner = "workhorse-alias-mlx".to_string();
        let err = settle_prewarm(
            &engines,
            vec![mixed[0].clone()],
            &mut planner,
            &RecordingSink::default(),
        )
        .expect_err("an emptied pool is a named refusal");
        assert!(format!("{err:#}").contains("refused to mount"), "{err:#}");

        let mut planner = "gabee-qwen".to_string();
        let sink = RecordingSink::default();
        let pool = settle_prewarm(&engines, vec![mixed[1].clone()], &mut planner, &sink)
            .expect("happy path");
        assert_eq!(pool.len(), 1);
        assert_eq!(planner, "gabee-qwen");
        assert!(
            sink.0.lock().expect("sink lock").is_empty(),
            "no failures → no events"
        );
    }

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
        // UNREGISTERED engine (no `mlx_engine` config): the sidecar planner will never mount —
        // distinct from "not mounted yet". It falls back to the first device whose engine IS
        // registered, naming the missing engine; the sidecar device itself is never the alt.
        let unregistered = Engines::with_lmstudio_for_tests(RecordingEngine::with_host(
            "lmstudio",
            "http://lm.local:1234",
        ));
        let map = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, None),
        ]);
        let expect = Some((
            "mlx-sidecar (no engine registered — config key \"mlx_engine\" absent or unparseable)"
                .to_string(),
            "gabee-qwen3.8-27b".to_string(),
        ));
        assert_eq!(planner_fallback(&unregistered, &pool, &map, alias), expect);
        let sidecar_first: Vec<SwarmDevice> =
            [pool[3].clone(), pool[0].clone(), pool[1].clone()].to_vec();
        assert_eq!(
            planner_fallback(&unregistered, &sidecar_first, &map, alias),
            expect,
            "the alt skips the unregistered device even when it is first"
        );
        assert_eq!(
            planner_fallback(&unregistered, &pool[3..], &map, alias),
            None,
            "no registered device to fall back to: left alone, as before"
        );
        // Every other arm is byte-identical with the sidecar registered: the same map keeps the
        // planner (unproven, the pre-warm mounts it).
        assert_eq!(planner_fallback(&engines, &pool, &map, alias), None);
    }

    /// S-M5: the wire shape per engine. LM Studio keeps the previous block byte-for-byte
    /// (`repeat_penalty`, the configured `lm_extra_body` merged first); the sidecar gets rapid-mlx's
    /// `repetition_penalty` and never sees the LM Studio body. The goose-side prefill/force-tool
    /// keys reach both.
    #[test]
    fn local_request_params_spell_the_knobs_in_each_engines_own_names() {
        let sampling = SamplingParams {
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(40),
            min_p: Some(0.05),
            repeat_penalty: Some(1.1),
        };
        let body = || {
            let mut m = serde_json::Map::new();
            m.insert("ttl".to_string(), serde_json::json!(3600));
            Some(m)
        };
        let force = goose_provider_types::formats::openai::FORCE_TOOL_UNTIL_ACT_KEY;
        let prefill = goose_provider_types::formats::openai::PREFILL_ASSISTANT_KEY;

        let lm = local_request_params(
            EngineKind::LmStudio,
            &sampling,
            body(),
            Some("shell"),
            Some("<think>"),
        );
        let mut lm_keys: Vec<&str> = lm.keys().map(String::as_str).collect();
        lm_keys.sort_unstable();
        let mut want = vec![
            "ttl",
            "top_p",
            "top_k",
            "min_p",
            "repeat_penalty",
            force,
            prefill,
        ];
        want.sort_unstable();
        assert_eq!(lm_keys, want, "LM Studio: the previous block, verbatim");
        assert_eq!(lm["repeat_penalty"], serde_json::json!(1.1f32));
        assert_eq!(lm["ttl"], serde_json::json!(3600));

        let sc = local_request_params(
            EngineKind::MlxSidecar,
            &sampling,
            body(),
            Some("shell"),
            Some("<think>"),
        );
        let mut sc_keys: Vec<&str> = sc.keys().map(String::as_str).collect();
        sc_keys.sort_unstable();
        let mut want = vec![
            "top_p",
            "top_k",
            "min_p",
            "repetition_penalty",
            force,
            prefill,
        ];
        want.sort_unstable();
        assert_eq!(
            sc_keys, want,
            "sidecar: rapid-mlx's repetition_penalty, no repeat_penalty, no LM Studio body"
        );
        assert_eq!(sc["repetition_penalty"], serde_json::json!(1.1f32));
        assert!(!sc.contains_key("repeat_penalty"));
        assert!(!sc.contains_key("ttl"));

        // Nothing configured → nothing sent, on either engine; empty prefill/force strings are
        // not keys.
        for kind in [EngineKind::LmStudio, EngineKind::MlxSidecar] {
            assert!(local_request_params(
                kind,
                &SamplingParams::default(),
                None,
                Some(""),
                Some("")
            )
            .is_empty());
        }
    }

    /// THE 71-MINUTE RUN THIS EXISTS TO PREVENT (measured 2026-07-17, arm allon-mihai):
    /// `lms ps` reported workhorse-qwopus3.6-27b-coder-mlx IDLE and resident, so the pool included it. The
    /// Mac Studio had actually dropped off the LAN and LM Link had withdrawn the alias, so POSTing to it
    /// returned `400 Invalid model identifier`. The `frontend` task went there, every attempt 400'd in ~2s,
    /// and — because run_agent_in returns Ok for a provider error (the 400 arrives as the agent's TEXT) — the
    /// dispatcher saw "finished, owned files absent" and retried with "You finished WITHOUT writing your
    /// owned file(s)". 3 attempts, 6.8s, ZERO tool calls, task failed. integrate-verify depends on every
    /// task, so it never became ready. 71 minutes, no app, and the engine blamed the model for a dead node.
    #[test]
    fn a_resident_but_unservable_node_is_dropped_before_it_can_eat_a_third_of_the_run() {
        let pool = vec![
            dev("local-mihai", "mihai-qwopus3.6-27b-coder-mlx", None),
            dev("mac-gabee", "gabee-qwopus3.6-27b-coder-mlx", None),
            dev("works-workhorse", "workhorse-qwopus3.6-27b-coder-mlx", None),
        ];
        // Exactly what /v1/models returned while `lms ps` still listed all three.
        let served: std::collections::HashSet<String> = [
            "mihai-qwopus3.6-27b-coder-mlx",
            "gabee-qwopus3.6-27b-coder-mlx",
            "text-embedding-nomic-embed-text-v1.5",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let (keep, dropped) = drop_unservable_devices(pool, Some(&served));
        assert_eq!(keep.len(), 2, "the two servable nodes survive");
        assert!(!keep.iter().any(|d| d.model_id.starts_with("workhorse-")));
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].1, "workhorse-qwopus3.6-27b-coder-mlx");
    }

    /// A FAILED PROBE IS NOT AN EMPTY FLEET. Seven instruments in this project have reported a confident
    /// zero that was a bug in the instrument; gating a whole run off the eighth would be the same mistake
    /// with worse consequences.
    #[test]
    fn a_probe_that_cannot_answer_never_gates_anything() {
        let pool = vec![
            dev("local-mihai", "mihai-qwopus3.6-27b-coder-mlx", None),
            dev("mac-gabee", "gabee-qwopus3.6-27b-coder-mlx", None),
        ];
        let (keep, dropped) = drop_unservable_devices(pool.clone(), None);
        assert_eq!(keep.len(), pool.len(), "None => byte-identical passthrough");
        assert!(dropped.is_empty());
    }

    /// If EVERY device looks unservable, the probe disagrees with `lms ps` about literally everything. A
    /// fleet that is 100% dead is possible; a broken probe is likelier — and dropping everything turns a
    /// recoverable run into a guaranteed dead one. Keep the pool and let the run proceed as before.
    #[test]
    fn the_preflight_can_never_empty_the_pool() {
        let pool = vec![
            dev("local-mihai", "mihai-qwopus3.6-27b-coder-mlx", None),
            dev("mac-gabee", "gabee-qwopus3.6-27b-coder-mlx", None),
        ];
        let served: std::collections::HashSet<String> = ["something-else-entirely".to_string()]
            .into_iter()
            .collect();
        let (keep, dropped) = drop_unservable_devices(pool, Some(&served));
        assert_eq!(keep.len(), 2, "never strand the run with zero nodes");
        assert!(
            dropped.is_empty(),
            "and do not claim drops we did not act on"
        );
    }

    /// #128 no-start guard: it fires ONLY on a PROVEN negative (the endpoint served a non-empty catalog that
    /// excludes every resident) and NEVER on an observed-empty. The safety property — a broken/empty probe can
    /// never refuse — is the one that must not regress, so it is asserted directly here.
    #[test]
    fn no_start_guard_fires_only_on_proven_zero_servable() {
        let pool = vec![
            dev("local-mihai", "mihai-qwopus3.6-27b-coder-mlx", None),
            dev("mac-gabee", "gabee-qwopus3.6-27b-coder-mlx", None),
        ];
        // Endpoint WORKS (non-empty) but lists none of our models -> every alias withdrawn -> REFUSE.
        let disjoint: std::collections::HashSet<String> = [
            "some-other-model".to_string(),
            "text-embedding-x".to_string(),
        ]
        .into_iter()
        .collect();
        assert!(
            all_resident_unservable(&pool, Some(&disjoint)),
            "non-empty catalog disjoint from the pool is a proven zero -> refuse"
        );
        // At least one resident servable -> the pool is fine -> DO NOT refuse.
        let one_ok: std::collections::HashSet<String> =
            ["mihai-qwopus3.6-27b-coder-mlx".to_string()]
                .into_iter()
                .collect();
        assert!(
            !all_resident_unservable(&pool, Some(&one_ok)),
            "one servable resident means the run can proceed"
        );
        // Probe unreachable/empty (None) -> NEVER refuse (the seven-lying-instruments rule).
        assert!(
            !all_resident_unservable(&pool, None),
            "a probe that cannot answer must never gate the run"
        );
        // Empty catalog -> None-equivalent -> NEVER refuse.
        let empty: std::collections::HashSet<String> = std::collections::HashSet::new();
        assert!(!all_resident_unservable(&pool, Some(&empty)));
        // Empty pool -> nothing to prove unservable -> NEVER refuse (falls through to the zero-resident branch).
        assert!(!all_resident_unservable(&[], Some(&disjoint)));
    }

    /// S-M6: a tagged device whose engine nobody registered is a guaranteed-dead device (probe
    /// None → never dropped; provider → lmstudio; every dispatch fails). It stays out of the pool
    /// and its id comes back for the named event; with the engine registered the merge is the
    /// additive, id-deduped one it always was.
    #[test]
    fn a_sidecar_device_without_a_registered_engine_is_excluded_by_name() {
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let configured = vec![
            dev("mac-gabee", "gabee-qwen3.8-27b", None),
            dev("workhorse-mlx", alias, Some(EngineKind::MlxSidecar)),
        ];
        let mut pool = vec![dev("mac-gabee", "gabee-qwen3.8-27b", None)];
        let none = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        assert_eq!(
            merge_sidecar_devices(&mut pool, &configured, &none),
            vec!["workhorse-mlx".to_string()]
        );
        assert_eq!(pool.len(), 1, "the dead device never enters the pool");
        assert!(none.engine_for_device(&configured[1]).is_none());
        assert!(none.engine_for_device(&configured[0]).is_some());

        let mut engines = Engines::with_lmstudio_for_tests(RecordingEngine::new("lmstudio"));
        engines.register_sidecar("mlx-sidecar", RecordingEngine::new("omlx"));
        assert!(merge_sidecar_devices(&mut pool, &configured, &engines).is_empty());
        assert_eq!(pool.len(), 2);
        assert_eq!(pool[1].id, "workhorse-mlx");
        assert!(
            merge_sidecar_devices(&mut pool, &configured, &engines).is_empty(),
            "dedup by id"
        );
        assert_eq!(pool.len(), 2);
    }

    /// S-M7: a declared sidecar device whose engine serves nothing while loading is OFF has no
    /// mount path this run — it leaves the pool by name, the LM Studio partition untouched and in
    /// order. Loading ON, or a sidecar probe that answered, or no sidecar partition at all: nothing
    /// moves.
    #[test]
    fn an_unmounted_sidecar_under_load_off_leaves_the_pool_by_name() {
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let pool = vec![
            dev("mac-gabee", "gabee-qwen3.8-27b", None),
            dev("workhorse-mlx", alias, Some(EngineKind::MlxSidecar)),
            dev("local-mihai", "mihai-qwen3.8-27b", None),
        ];
        let lm_ids: &[&str] = &["gabee-qwen3.8-27b", "mihai-qwen3.8-27b"];
        let unmounted = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, None),
        ]);
        let mut p = pool.clone();
        assert_eq!(
            exclude_unmountable_sidecar_devices(&mut p, &unmounted, false),
            vec![SidecarExclusion::Unmounted {
                id: "workhorse-mlx".to_string()
            }]
        );
        assert_eq!(
            p.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            vec!["mac-gabee", "local-mihai"]
        );
        let mut p = pool.clone();
        assert!(
            exclude_unmountable_sidecar_devices(&mut p, &unmounted, true).is_empty(),
            "loading on: the pre-warm mounts it"
        );
        assert_eq!(p.len(), 3);
        let mounted = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (EngineKind::MlxSidecar, Some(&[alias])),
        ]);
        let mut p = pool.clone();
        assert!(exclude_unmountable_sidecar_devices(&mut p, &mounted, false).is_empty());
        assert_eq!(p.len(), 3);
        let lm_only = served(&[(EngineKind::LmStudio, Some(lm_ids))]);
        let mut p = pool.clone();
        assert!(exclude_unmountable_sidecar_devices(&mut p, &lm_only, false).is_empty());
        assert_eq!(p.len(), 3);
    }

    /// The pool half of S-H3: the sidecar is UP and serving alias X while the declared device
    /// wants Y, loading off. `drop_unservable_devices` keeps a lone unservable partition member
    /// (never-empties-the-pool), so the device is excluded HERE as a proven negative with its
    /// own event; loading on leaves it for the pre-warm's remount; a mounted alias stays.
    #[test]
    fn a_sidecar_serving_another_alias_under_load_off_leaves_the_pool_by_name() {
        let wanted = "workhorse-qwen3.5-9b-4bit-mlx";
        let pool = vec![
            dev("mac-gabee", "gabee-qwen3.8-27b", None),
            dev("workhorse-mlx", wanted, Some(EngineKind::MlxSidecar)),
            dev("local-mihai", "mihai-qwen3.8-27b", None),
        ];
        let lm_ids: &[&str] = &["gabee-qwen3.8-27b", "mihai-qwen3.8-27b"];
        let other = served(&[
            (EngineKind::LmStudio, Some(lm_ids)),
            (
                EngineKind::MlxSidecar,
                Some(&["workhorse-qwen3-coder-30b-mlx"]),
            ),
        ]);
        let mut p = pool.clone();
        let gone = exclude_unmountable_sidecar_devices(&mut p, &other, false);
        assert_eq!(
            gone,
            vec![SidecarExclusion::ServesOtherAlias {
                id: "workhorse-mlx".to_string(),
                wanted: wanted.to_string(),
                serving: vec!["workhorse-qwen3-coder-30b-mlx".to_string()],
            }]
        );
        assert_eq!(
            p.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            vec!["mac-gabee", "local-mihai"],
            "the LM Studio partition is untouched and in order"
        );
        // The proven negative would otherwise have SURVIVED the per-engine drop: a lone
        // unservable partition member is kept by the never-empties rule.
        let (kept, dropped) = drop_unservable_devices_per_engine(pool.clone(), &other);
        assert_eq!(kept.len(), 3);
        assert!(dropped.is_empty());
        let mut p = pool.clone();
        assert!(
            exclude_unmountable_sidecar_devices(&mut p, &other, true).is_empty(),
            "loading on: the pre-warm remounts it under the wanted alias"
        );
        assert_eq!(p.len(), 3);
        // The events: one grouped unmounted event (S-M7 shape) plus one per wrong-alias device.
        let events = sidecar_exclusion_events(&[
            SidecarExclusion::Unmounted {
                id: "mlx-a".to_string(),
            },
            gone[0].clone(),
            SidecarExclusion::Unmounted {
                id: "mlx-b".to_string(),
            },
        ]);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "sidecar-unmounted-and-load-disabled");
        assert_eq!(events[0]["devices"], serde_json::json!(["mlx-a", "mlx-b"]));
        assert_eq!(events[1]["event"], "sidecar-device-serves-other-alias");
        assert_eq!(events[1]["id"], "workhorse-mlx");
        assert_eq!(events[1]["wanted"], wanted);
        assert_eq!(
            events[1]["serving"],
            serde_json::json!(["workhorse-qwen3-coder-30b-mlx"])
        );
        assert!(sidecar_exclusion_events(&[]).is_empty());
    }

    /// The pre-warm never routes an unregistered engine's device to LM Studio (the deleted
    /// pre-step-C arm fired a doomed `lms load <alias>`): it is named on stderr and skipped, and
    /// the LM Studio device still warms.
    #[test]
    fn prewarm_skips_a_device_whose_engine_is_unregistered() {
        let lm = RecordingEngine::new("lmstudio");
        let engines = Engines::with_lmstudio_for_tests(lm.clone());
        let alias = "workhorse-qwen3.5-9b-4bit-mlx";
        let pool = vec![
            dev("workhorse-mlx", alias, Some(EngineKind::MlxSidecar)),
            dev("mac-gabee", "gabee-qwen", None),
        ];
        prewarm_pool(&engines, &pool, alias);
        assert_eq!(
            lm.calls(),
            vec![("gabee-qwen".to_string(), 1)],
            "no lms load of the sidecar alias, for the planner or the device"
        );
    }

    /// S-L9: the sync trait surface driven from each runtime shape. No runtime → a throwaway one;
    /// a multi-thread runtime → block_in_place; a current_thread runtime (acp/provider.rs builds
    /// one) → the typed error, where `block_in_place` used to panic the process.
    #[test]
    fn block_on_engine_answers_or_refuses_by_runtime_flavor_never_panics() {
        assert_eq!(
            block_on_engine(async { 7 }).expect("no runtime → throwaway"),
            7
        );
        let current = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("current_thread runtime");
        let refused = current.block_on(async { block_on_engine(async { 7 }) });
        assert!(
            matches!(refused, Err(EngineCallError::CurrentThreadRuntime)),
            "current_thread → typed error, got {refused:?}"
        );
        let multi = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .expect("multi_thread runtime");
        assert_eq!(
            multi
                .block_on(async { block_on_engine(async { 7 }) })
                .expect("multi_thread → block_in_place"),
            7
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
