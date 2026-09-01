//! Fleet ordering and speed-weight resolution — which node gets work first.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). Extracted verbatim from swarm.rs:
//! the slot builders (`fleet_slot_models`, `live_fleet_slots`), the fan pool ordering
//! (`order_fleet_by_speed`, `one_lane_per_host`) and the operator's `speed_weights` substring
//! resolution (`configured_speed_weight`).

use console::style;
use goose_swarm::DeviceCfg;

use super::{gen_entry_id, load_config, probe_lms_processes, EventSink, LmsProcess, SwarmDevice};
use std::sync::{Arc, Mutex};

/// A device id to its configured `speed_weight` — substring match against the `speed_weights` map
/// (e.g. `{"worksmacstudio":3,"local":2,"gabee":1}`); default 1 = equal share.
///
/// A FREE FUNCTION because two places need it and they were not both using it. The dispatcher read
/// the map to ROUTE work; the `GOOSE_SWARM_MAX_NODES` cap, which decides WHICH devices survive at
/// all, sorted alphabetically and never consulted it. So an operator could rank their fleet
/// correctly and still have the cap hand them the slowest machine.
pub(super) fn configured_speed_weight(
    weights: &std::collections::HashMap<String, u32>,
    id: &str,
) -> u32 {
    weights
        .iter()
        .find(|(pat, _)| id.contains(pat.as_str()))
        .map(|(_, w)| (*w).max(1))
        .unwrap_or(1)
}

/// The routing weight for every model the CONFIG can field, resolved ONCE against the ids the
/// operator actually wrote — never re-derived from raw slot strings at a fan.
///
/// Same precedence chain the run-start pool builder uses per device: the node's own explicit
/// `speed_weight` wins, else the `speed_weights` substring map matched against the DEVICE id
/// (the id the map's fragments were written for), else 1. The planner model gets the same
/// resolution the planner-also-works push gives it (matched against the model id, since the
/// pushed device's id is the literal "planner").
///
/// Keyed by MODEL id because that is what the fan pools carry (`fleet_slot_models` /
/// `live_fleet_slots` flat_map `d.model_id`); on a model_id shared by two configured devices
/// the first wins, matching the pool builder's first-host-wins dedupe.
pub(super) fn config_speed_weights(
    cfg: &super::SwarmConfig,
) -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    for d in cfg.devices.iter().filter(|d| d.enabled) {
        let w = d
            .speed_weight
            .unwrap_or_else(|| configured_speed_weight(&cfg.speed_weights, &d.id));
        map.entry(d.model_id.clone()).or_insert(w);
    }
    map.entry(cfg.planner_model.clone())
        .or_insert_with(|| configured_speed_weight(&cfg.speed_weights, &cfg.planner_model));
    map
}

/// The RUN's resolved pool weights, published once at pool resolve. Config resolution alone is
/// not enough: r5's pool was reconciled from `lms ps` ("auto-use what's loaded") while every
/// configured device sat disabled on a stale model generation, so the run's real model ids
/// existed in no config entry and a config-only map would have resolved the whole wave to the
/// same all-tie 1 the substring mismatch did. The published map IS the run-start resolution the
/// scheduler routes by (`DeviceCfg.speed_weight`), generated ids and explicit overrides
/// included.
static PUBLISHED_FLEET_WEIGHTS: std::sync::OnceLock<
    std::sync::RwLock<std::collections::HashMap<String, u32>>,
> = std::sync::OnceLock::new();

/// model_id → resolved routing weight for a RESOLVED pool. `DeviceCfg.speed_weight` already
/// carries the full precedence (explicit override, else the substring map vs the device id,
/// else 1) — this only re-keys it by the model ids the fan pools carry. First host wins on a
/// shared model_id, matching the pool builder's dedupe.
pub(super) fn fleet_weights_of(devices: &[DeviceCfg]) -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    for d in devices {
        map.entry(d.model_id.clone()).or_insert(d.speed_weight);
    }
    map
}

/// Publish the run's pool weights so every later fan orders by the SAME resolution the
/// scheduler routes by. Called once per run, at the `pool_resolved` site, after the planner
/// push — the last point the full `Vec<DeviceCfg>` exists before it moves into the scheduler.
pub(super) fn publish_fleet_speed_weights(devices: &[DeviceCfg]) {
    let map = fleet_weights_of(devices);
    let lock = PUBLISHED_FLEET_WEIGHTS.get_or_init(Default::default);
    match lock.write() {
        Ok(mut g) => *g = map,
        Err(poisoned) => *poisoned.into_inner() = map,
    }
}

/// What a fan orders by: the config resolution as the floor, overlaid by the run's published
/// pool — the run's own resolution wins wherever both name a model, because it is the one the
/// ROUTING already honors (and the only one that exists for a reconciled pool).
pub(super) fn resolved_fleet_speed_weights(
    cfg: &super::SwarmConfig,
) -> std::collections::HashMap<String, u32> {
    let mut map = config_speed_weights(cfg);
    if let Some(lock) = PUBLISHED_FLEET_WEIGHTS.get() {
        let published = match lock.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        for (k, v) in published.iter() {
            map.insert(k.clone(), *v);
        }
    }
    map
}

/// The `speed_weights` map keys that matched NOTHING in the run's resolved pool — neither a
/// device id (what the fragments are written for) nor a model id (what the planner push and the
/// telemetry keys use). Emitted in `pool_resolved` so a key/id mismatch is VISIBLE instead of
/// silently resolving every miss to 1: r5's `{local, worksmacstudio}` matched no model id at the
/// fan, and nothing anywhere said so.
pub(super) fn unmatched_speed_weight_patterns(
    weights: &std::collections::HashMap<String, u32>,
    devices: &[DeviceCfg],
) -> Vec<String> {
    let mut out: Vec<String> = weights
        .keys()
        .filter(|pat| {
            !devices
                .iter()
                .any(|d| d.id.contains(pat.as_str()) || d.model_id.contains(pat.as_str()))
        })
        .cloned()
        .collect();
    out.sort();
    out
}

/// The measured rate for a device: the telemetry node key must equal the device/model id or be
/// a '-'-bounded prefix of it (the fleet's node identity is the model-id prefix, "gabee-…").
/// The boundary requirement is load-bearing: a bare starts_with would let a one-letter key
/// claim every device. Pure/testable.
pub(super) fn measured_rate_for(
    rates: &std::collections::HashMap<String, f64>,
    dev_id: &str,
    model_id: &str,
) -> Option<f64> {
    rates.iter().find_map(|(k, r)| {
        let matches = |s: &str| {
            s == k
                || s.strip_prefix(k.as_str())
                    .is_some_and(|rest| rest.starts_with('-'))
        };
        (matches(model_id) || matches(dev_id)).then_some(*r)
    })
}

/// The COMPLETE-phase fix target: the operator's resolved routing weight is PRIMARY; the run's
/// own median decode rate breaks ties only WITHIN equal weights. Returns the chosen
/// (device_id, model_id) and the honest basis string for `fix_target_selected`.
///
/// The median-first version this replaces trusted a confounded measurement over the operator —
/// r5 measured: rates {gabee: 12.654, mihai: 12.566, workhorse: 8.208} because workhorse's
/// samples were the 93-minute sink call at huge context (decode collapses as KV grows) while
/// gabee idled through BUILD and sampled light calls. The per-node median measures the WORKLOAD
/// each node got, not the node, so it overrode the correct weight-3 target (workhorse) with the
/// weight-1 gabee for every serialized repair dispatch.
///
/// Callers prove every candidate measured before consulting rates (mixed comparisons lie); a
/// missing rate here can therefore only be a tie that resolves to the later candidate, same as
/// `max_by`'s last-maximum rule for exact-equal rates.
pub(super) fn rank_fix_target(
    candidates: &[(String, String, u32)],
    rates: &std::collections::HashMap<String, f64>,
) -> Option<((String, String), &'static str)> {
    let best = candidates.iter().max_by(|a, b| {
        a.2.cmp(&b.2).then_with(|| {
            let ra = measured_rate_for(rates, &a.0, &a.1).unwrap_or(0.0);
            let rb = measured_rate_for(rates, &b.0, &b.1).unwrap_or(0.0);
            ra.total_cmp(&rb)
        })
    })?;
    let top_weight_shared = candidates.iter().filter(|c| c.2 == best.2).count() > 1;
    Some((
        (best.0.clone(), best.1.clone()),
        if top_weight_shared {
            "speed_weight+measured_tiebreak"
        } else {
            "speed_weight"
        },
    ))
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
pub(super) fn fleet_slot_models(devices: &[DeviceCfg]) -> Vec<String> {
    devices
        .iter()
        .flat_map(|d| std::iter::repeat_n(d.model_id.clone(), (d.weight as usize).max(1)))
        .collect()
}

/// The fan's slot list, checked against the LIVE fleet instead of the boot snapshot.
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
/// FALLS BACK TO THE SNAPSHOT ON ANY DOUBT — a failed probe, a probe that returns nothing, or a result
/// that would leave the fan with no slots at all. A fan running on a stale-but-working list is a small
/// inefficiency; a fan running on an empty one is a dead phase, and the probe is the newer and less
/// trustworthy of the two inputs.
pub(super) fn live_fleet_slots(devices: &[DeviceCfg]) -> Vec<String> {
    let snapshot = fleet_slot_models(devices);
    let Ok(procs) = super::probe_lms_processes() else {
        return snapshot;
    };
    let resident: std::collections::HashSet<String> =
        procs.iter().map(|p| p.identifier.clone()).collect();
    if resident.is_empty() {
        return snapshot;
    }
    let live: Vec<String> = devices
        .iter()
        .filter(|d| d.is_cloud || resident.contains(&d.model_id))
        .flat_map(|d| std::iter::repeat_n(d.model_id.clone(), (d.weight as usize).max(1)))
        .collect();
    if live.is_empty() {
        snapshot
    } else {
        live
    }
}

/// One lane per HOST for the prologue fans (scout/detail) — their own docstrings
/// promise "one per device", but the pool arrives SLOT-expanded, so two calls stack on one
/// host and degrade each other (F623: detail time is queue time, monotonic in concurrency).
/// MEASURED, r16 vs r17: the same detail fan cleared 14/14 on 4 slots, then dropped three
/// details at ~490s the moment the third host's lanes doubled the fan width — the grace
/// modeled prefill, not 2-deep decode contention. Dedupe by identity, first occurrence
/// wins; the fan's own speed-ordering then puts the fastest host first.
pub(super) fn one_lane_per_host(models: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .filter(|m| seen.insert(m.clone()))
        .collect()
}

/// Order a fan's device pool by DESCENDING resolved speed weight (stable: ties keep
/// discovery order). Every fan pops devices from the FRONT of its queue, so front == the
/// node that gets the first (and, when items < devices, the ONLY) work. MEASURED (r13,
/// operator screenshot): pool order is discovery order — gabee, mihai, workhorse — so the
/// weight-4 host was structurally LAST in line for every scout/detail fan and sat
/// READY while the weight-1 host prefilled. One rule, one place: every fan site inherits it.
///
/// `weights` is the RESOLVED model_id → weight map from `resolved_fleet_speed_weights`, looked
/// up EXACTLY — never the raw `speed_weights` substring map. The slot strings here are MODEL
/// ids, and matching the free-form map against them is the r5 defect (swarm-20260830-083847650):
/// the operator's `{local: 2, gabee: 1, worksmacstudio: 3}` is keyed by DEVICE-id fragments, so
/// against `workhorse-qwen3.8-27b`/`mihai-qwen3.8-27b`/`gabee-qwen3.8-27b` only "gabee" matched,
/// the misses all defaulted to 1, the all-tie sort kept config order (gabee first), and round
/// 0's two-shard fix wave rode gabee+mihai while the weight-3 workhorse idled.
pub(super) fn order_fleet_by_speed(
    devices: Vec<String>,
    weights: &std::collections::HashMap<String, u32>,
) -> Vec<String> {
    // The pool arrives SLOT-EXPANDED (a weight-2 device appears twice), so a plain sort
    // groups a host's lanes together and a 3-item fan stacks two calls on the fast host
    // while a whole node idles — the r14 screenshot, the mirror image of r13's. Two
    // concurrent generations on one Apple host degrade each other (F623), so the pool is
    // emitted as a speed-ordered ROUND-ROBIN: every host's first lane (fastest first),
    // then every host's second lane, and so on — a small fan touches every host once,
    // fastest first; a big fan still fills every slot.
    let mut groups: Vec<(String, usize)> = Vec::new();
    for d in devices {
        if let Some(g) = groups.iter_mut().find(|(id, _)| *id == d) {
            g.1 += 1;
        } else {
            groups.push((d, 1));
        }
    }
    // 1 is the documented precedence floor for a model the resolver never saw (a device outside
    // the config, e.g. a pool reconciled from `lms ps` with a stale config) — the same "default
    // 1 = equal share" the substring resolution always had, never a silent swallow of a resolver
    // failure: unmatched map keys are reported by `unmatched_speed_weight_patterns` in
    // `pool_resolved`.
    groups.sort_by_key(|(id, _)| std::cmp::Reverse(weights.get(id).copied().unwrap_or(1)));
    let max_lanes = groups.iter().map(|(_, n)| *n).max().unwrap_or(0);
    let mut out = Vec::new();
    for lane in 0..max_lanes {
        for (id, n) in &groups {
            if lane < *n {
                out.push(id.clone());
            }
        }
    }
    out
}

/// "Auto-use what's loaded": build the worker pool from the models currently resident on the fleet
/// (`lms ps`) so the swarm runs on what's actually loaded, not (possibly stale) configured model_ids.
/// Returns (pool, planner_model). An empty pool means the fleet has nothing loaded (caller bootstraps
/// or bails). Weights: explicit device override, else speed_weight, else LM Studio PARALLEL, else 1.
pub(super) fn reconcile_pool_with_fleet(
    cfg: &super::SwarmConfig,
) -> (Vec<SwarmDevice>, Option<String>) {
    let procs = match probe_lms_processes() {
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
        })
        .collect();
    // Planner: keep the configured planner if it is resident; else pick the best resident model for
    // the hardest job (the architect skeleton). The quality bar and the rank are the NAMED functions
    // below (`planner_grade`, `planner_rank`) so the mid-run aux router applies the SAME bar as this
    // pick, never a copy.
    let planner = if resident.iter().any(|p| p.identifier == cfg.planner_model) {
        Some(cfg.planner_model.clone())
    } else {
        resident
            .iter()
            .filter(|p| planner_grade(&p.identifier))
            .max_by_key(|p| {
                planner_rank(
                    &cfg.speed_weights,
                    p.device.as_deref(),
                    &super::gen_entry_id(cfg, p.device.as_deref(), &p.identifier),
                    &p.identifier,
                )
            })
            .or_else(|| {
                resident.iter().max_by_key(|p| {
                    planner_rank(
                        &cfg.speed_weights,
                        p.device.as_deref(),
                        &super::gen_entry_id(cfg, p.device.as_deref(), &p.identifier),
                        &p.identifier,
                    )
                })
            })
            .map(|p| p.identifier.clone())
    };
    (pool, planner)
}

/// The planner QUALITY bar (candidate filter): a model plausibly strong enough for planner-side
/// work — 27B-class, dense, or a coder build. Named (rather than inlined in the planner pick)
/// so the mid-run aux router below applies the SAME bar instead of a copy.
pub(super) fn planner_grade(identifier: &str) -> bool {
    let n = identifier.to_lowercase();
    n.contains("27b") || n.contains("dense") || n.contains("coder")
}

/// The planner pick's rank. QUALITY outranks speed: a low-quant model (q5/q4/q3/q2) fails the
/// structured skeleton, so prefer a NOT-low-quant model FIRST, then the fastest host (highest
/// configured speed_weight). speed_weight keys are matched against host + DEVICE ID + identifier
/// — the device id because that is what the operator's `speed_weights` fragments were written for
/// (`configured_speed_weight` matches them against `d.id`), and the pool that reaches the aux
/// router carries ids and often no host. VA-004 (r6d): `speed_weights {local: 2, gabee: 1,
/// worksmacstudio: 3}` against `mihai-qwen3.8-27b` with no host matched NOTHING — the fragment
/// `local` lives in the device id `local-mihai-qwen3.8-27b` — so mihai ranked 1, tied with gabee,
/// lost the id-order tiebreak, and the aux order was [workhorse, gabee, mihai] against the
/// operator's [3, 2, 1].
pub(super) fn planner_rank(
    speed_weights: &std::collections::HashMap<String, u32>,
    host: Option<&str>,
    device_id: &str,
    identifier: &str,
) -> (u8, u32) {
    let ident = identifier.to_lowercase();
    let quant_ok = u8::from(
        !(ident.contains("q2_")
            || ident.contains("q3_")
            || ident.contains("q4_")
            || ident.contains("q5")),
    );
    let hay = format!(
        "{} {} {}",
        host.unwrap_or(""),
        device_id.to_lowercase(),
        ident
    );
    let speed = speed_weights
        .iter()
        .find(|(pat, _)| hay.contains(pat.as_str()))
        .map(|(_, w)| *w)
        .unwrap_or(1);
    (quant_ok, speed)
}

/// The models a MID-RUN aux call (an omni-judge look) may run on: every
/// LOCAL pool device passing the planner's quality bar, best rank first — that order breaks
/// in-flight ties after the planner in `least_loaded_aux_model`. Quant is a FILTER here (not
/// only a rank tier) because an aux verdict from a low-quant build is exactly the misread the
/// quality bar exists to prevent. Cloud devices are excluded: aux looks are frequent, and the
/// frozen behavior this replaces was local-planner-only — on a cloud-only pool the candidate
/// list is empty and the planner keeps every aux call, byte-identical to before.
pub(super) fn aux_candidate_models(
    speed_weights: &std::collections::HashMap<String, u32>,
    pool: &[SwarmDevice],
) -> Vec<String> {
    let mut cands: Vec<&SwarmDevice> = pool
        .iter()
        .filter(|d| {
            !d.is_cloud()
                && planner_grade(&d.model_id)
                && planner_rank(speed_weights, d.host.as_deref(), &d.id, &d.model_id).0 == 1
        })
        .collect();
    cands.sort_by(|a, b| {
        planner_rank(speed_weights, b.host.as_deref(), &b.id, &b.model_id)
            .cmp(&planner_rank(
                speed_weights,
                a.host.as_deref(),
                &a.id,
                &a.model_id,
            ))
            .then_with(|| a.model_id.cmp(&b.model_id))
    });
    cands.into_iter().map(|d| d.model_id.clone()).collect()
}

/// The live pick for a mid-run aux call: the candidate with the fewest in-flight dispatcher
/// calls; the PLANNER wins ties (a candidate needs strictly fewer to displace it) EXCEPT when
/// the planner's node runs the lane this call supervises (`avoid`, walked last), so an idle
/// fleet resolves byte-identically to the frozen planner identity for every call that
/// supervises nothing. Returns the chosen model AND
/// its live count, for the routing event. A model with no map entry has zero calls in flight —
/// that empty means empty. A preference, never a refusal: whatever the counts say, SOME model is
/// returned and the call proceeds.
///
/// `avoid` is the model of the lane this call SUPERVISES: it is walked LAST, so among equally
/// idle models the look lands anywhere but on the worker it reads — never excluded, so a
/// single-node fleet still serves its own looks. MEASURED (r6d, research-ledger-core-q0): all
/// seven looks landed on gabee — q0's own node — at in-flight 1 while mihai also sat at 1;
/// `aux_candidate_models` had ordered gabee before mihai (equal rank, id order), so the
/// strictly-fewer walk kept the tie on the supervised node every time.
pub(super) fn least_loaded_aux_model(
    planner: &str,
    ranked_candidates: &[String],
    inflight: &std::collections::HashMap<String, u32>,
    avoid: Option<&str>,
) -> (String, u32) {
    let load = |m: &str| inflight.get(m).copied().unwrap_or(0);
    // Walk order IS tie-break order: planner, then rank — with the supervised lane's model
    // moved to the end (stable sort: everything else keeps its place).
    let mut order: Vec<&str> = std::iter::once(planner)
        .chain(ranked_candidates.iter().map(String::as_str))
        .collect();
    if let Some(a) = avoid {
        order.sort_by_key(|m| *m == a);
    }
    // `order` always holds the planner, so index 0 exists by construction.
    let mut best: (&str, u32) = (order[0], load(order[0]));
    for m in &order[1..] {
        let c = load(m);
        if c < best.1 {
            best = (m, c);
        }
    }
    (best.0.to_string(), best.1)
}

/// The live load meter behind `least_loaded_aux_model`, held for the LIFE of one dispatcher
/// call: increment at `run_agent_in_inner`'s door, decrement on Drop — which covers Err returns
/// AND cancellation (the omni-judge probe future is dropped when the supervised stream ends
/// first; a plain decrement after the await would leak that count forever, and a leaked count
/// routes every later aux call away from a healthy node). Poisoned-lock arms still count for
/// the same reason. MEASURED need (r6c 18:38): worker + judge-ledgerd-core + judge-web-viz +
/// replan-r0 all streaming/queued on the planner's node while two hosts sat READY.
pub(super) struct InflightGuard<'a> {
    map: &'a std::sync::Mutex<std::collections::HashMap<String, u32>>,
    model: String,
}

impl<'a> InflightGuard<'a> {
    pub(super) fn enter(
        map: &'a std::sync::Mutex<std::collections::HashMap<String, u32>>,
        model: &str,
    ) -> Self {
        let mut g = match map.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        *g.entry(model.to_string()).or_insert(0) += 1;
        drop(g);
        Self {
            map,
            model: model.to_string(),
        }
    }
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        let mut g = match self.map.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(c) = g.get_mut(&self.model) {
            *c = c.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod fan_order_tests {
    use super::*;

    #[test]
    fn the_fastest_host_is_first_in_every_fan_pool() {
        // The RESOLVED map: exact model-id keys, as resolved_fleet_speed_weights builds it.
        let weights: std::collections::HashMap<String, u32> = [
            ("gabee-qwen3.6-27b".to_string(), 1),
            ("mihai-qwen3.6-27b".to_string(), 2),
            ("workhorse-qwen3.6-27b".to_string(), 4),
        ]
        .into_iter()
        .collect();
        let ordered = order_fleet_by_speed(
            vec![
                "gabee-qwen3.6-27b".to_string(),
                "mihai-qwen3.6-27b".to_string(),
                "workhorse-qwen3.6-27b".to_string(),
            ],
            &weights,
        );
        assert_eq!(
            ordered,
            vec![
                "workhorse-qwen3.6-27b",
                "mihai-qwen3.6-27b",
                "gabee-qwen3.6-27b"
            ],
            "a 1-item fan must hand its work to the weight-4 host, never the discovery-order front"
        );
    }

    #[test]
    fn a_slot_expanded_pool_round_robins_hosts_fastest_first() {
        // The r13/r14 pair, pinned: slot expansion duplicates hosts, and BOTH failure modes
        // (slow-host-first, and fast-host stacked while a whole node idles) are lane order.
        // A 3-item fan on this pool must touch all three hosts, fastest first.
        let weights: std::collections::HashMap<String, u32> = [
            ("gabee-q".to_string(), 1),
            ("mihai-q".to_string(), 2),
            ("workhorse-q".to_string(), 4),
        ]
        .into_iter()
        .collect();
        let ordered = order_fleet_by_speed(
            vec![
                "gabee-q".to_string(),
                "gabee-q".to_string(),
                "mihai-q".to_string(),
                "mihai-q".to_string(),
                "workhorse-q".to_string(),
                "workhorse-q".to_string(),
            ],
            &weights,
        );
        assert_eq!(
            ordered,
            vec![
                "workhorse-q",
                "mihai-q",
                "gabee-q",
                "workhorse-q",
                "mihai-q",
                "gabee-q"
            ],
            "every host once (fastest first) before any host twice"
        );
    }

    #[test]
    fn one_lane_per_host_dedupes_a_slot_expanded_pool() {
        let out = one_lane_per_host(vec![
            "a-q".to_string(),
            "a-q".to_string(),
            "b-q".to_string(),
            "b-q".to_string(),
        ]);
        assert_eq!(
            out,
            vec!["a-q", "b-q"],
            "prologue fans run ONE call per host"
        );
    }

    /// r5 (swarm-20260830-083847650) verbatim: the operator's substring map is keyed by
    /// DEVICE-id fragments while the wave pool carries MODEL ids. Resolution must happen against
    /// the device ids, ONCE, and the wave orders by the result — never by re-matching the raw
    /// map against slot strings, where only "gabee" matched, every miss defaulted to 1, and the
    /// all-tie sort kept config order (gabee first) while the weight-3 workhorse idled.
    #[test]
    fn the_wave_pool_orders_by_weights_resolved_against_device_ids() {
        use super::super::{SwarmConfig, SwarmDevice};
        let dev = |id: &str, model: &str| SwarmDevice {
            id: id.to_string(),
            model_id: model.to_string(),
            weight: 1,
            enabled: true,
            instances: 1,
            host: None,
            provider: None,
            speed_weight: None,
            supervision: None,
        };
        let cfg = SwarmConfig {
            // Config order lists gabee FIRST, exactly as r5's config.yaml did — the order the
            // all-tie sort used to preserve straight into the wave.
            devices: vec![
                dev("mac-gabee-lmstudio", "gabee-qwen3.8-27b"),
                dev("local-mihai-lmstudio", "mihai-qwen3.8-27b"),
                dev("worksmacstudio-workhorse-lmstudio", "workhorse-qwen3.8-27b"),
            ],
            speed_weights: [
                ("local".to_string(), 2),
                ("gabee".to_string(), 1),
                ("worksmacstudio".to_string(), 3),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let resolved = config_speed_weights(&cfg);
        assert_eq!(resolved["workhorse-qwen3.8-27b"], 3);
        assert_eq!(resolved["mihai-qwen3.8-27b"], 2);
        assert_eq!(resolved["gabee-qwen3.8-27b"], 1);
        let pool = order_fleet_by_speed(
            vec![
                "gabee-qwen3.8-27b".to_string(),
                "mihai-qwen3.8-27b".to_string(),
                "workhorse-qwen3.8-27b".to_string(),
            ],
            &resolved,
        );
        assert_eq!(
            pool,
            vec![
                "workhorse-qwen3.8-27b",
                "mihai-qwen3.8-27b",
                "gabee-qwen3.8-27b"
            ],
            "r5's round-0 wave (complete_fix_dispatched seq 415/416) popped gabee+mihai while \
             the weight-3 workhorse idled — the pool must lead with the operator's fastest host"
        );
    }

    /// r5's ACTUAL pool shape: every configured device DISABLED on a stale model generation,
    /// the run's pool reconciled from `lms ps` with generated ids the config never named — so
    /// config-only resolution sees nothing. The pool's own `DeviceCfg.speed_weight` (resolved
    /// at run start against the generated DEVICE ids, where the map's fragments do match) is
    /// what `publish_fleet_speed_weights` hands the fans, re-keyed by model id.
    #[test]
    fn a_reconciled_pool_hands_the_wave_its_run_start_resolution() {
        let dev = |id: &str, model: &str, sw: u32| DeviceCfg {
            id: id.to_string(),
            model_id: model.to_string(),
            weight: 2,
            enabled: true,
            speed_weight: sw,
            supervision: false,
            is_cloud: false,
        };
        // The r5 pool_resolved devices verbatim, with the run-start resolution the substring
        // map produced against their generated ids: gabee 1, local(mihai) 2, worksmacstudio 3.
        let devices = [
            dev("mac-gabee-qwen3.8-27b", "gabee-qwen3.8-27b", 1),
            dev("local-mihai-qwen3.8-27b", "mihai-qwen3.8-27b", 2),
            dev(
                "worksmacstudio-workhorse-qwen3.8-27b",
                "workhorse-qwen3.8-27b",
                3,
            ),
        ];
        let map = fleet_weights_of(&devices);
        let pool = order_fleet_by_speed(
            vec![
                "gabee-qwen3.8-27b".to_string(),
                "mihai-qwen3.8-27b".to_string(),
                "workhorse-qwen3.8-27b".to_string(),
            ],
            &map,
        );
        assert_eq!(
            pool,
            vec![
                "workhorse-qwen3.8-27b",
                "mihai-qwen3.8-27b",
                "gabee-qwen3.8-27b"
            ],
            "the wave must ride the run's own resolution even when the config names none of it"
        );
    }

    /// r5's fix_target_selected (seq 403) verbatim: every candidate measured, but workhorse's
    /// median came from the 93-minute sink call at huge context while gabee sampled light calls
    /// — the median measured the WORKLOAD, not the node, and overrode the weight-3 target with
    /// the weight-1 gabee. The operator's weight is primary; distinct weights leave nothing for
    /// the rate to decide.
    #[test]
    fn the_fix_target_keeps_the_operators_weight_over_a_confounded_median() {
        let rates: std::collections::HashMap<String, f64> = [
            ("gabee".to_string(), 12.654),
            ("mihai".to_string(), 12.566),
            ("workhorse".to_string(), 8.208),
        ]
        .into_iter()
        .collect();
        let candidates = vec![
            (
                "mac-gabee-lmstudio".to_string(),
                "gabee-qwen3.8-27b".to_string(),
                1u32,
            ),
            (
                "local-mihai-lmstudio".to_string(),
                "mihai-qwen3.8-27b".to_string(),
                2,
            ),
            (
                "worksmacstudio-workhorse-lmstudio".to_string(),
                "workhorse-qwen3.8-27b".to_string(),
                3,
            ),
        ];
        let ((id, model), basis) = rank_fix_target(&candidates, &rates).unwrap();
        assert_eq!(
            id, "worksmacstudio-workhorse-lmstudio",
            "the weight-3 host stays the repair target; gabee's 12.654 median never outranks it"
        );
        assert_eq!(model, "workhorse-qwen3.8-27b");
        assert_eq!(
            basis, "speed_weight",
            "distinct weights: the operator decided, and the basis says so honestly"
        );
    }

    /// Two equal-weight hosts: the run's own median is exactly the right tiebreak, and the
    /// basis admits the measurement decided.
    #[test]
    fn equal_weights_let_the_measured_median_break_the_tie() {
        let rates: std::collections::HashMap<String, f64> =
            [("gabee".to_string(), 12.654), ("mihai".to_string(), 12.566)]
                .into_iter()
                .collect();
        let candidates = vec![
            ("d-mihai".to_string(), "mihai-q".to_string(), 2u32),
            ("d-gabee".to_string(), "gabee-q".to_string(), 2),
        ];
        let ((id, _), basis) = rank_fix_target(&candidates, &rates).unwrap();
        assert_eq!(id, "d-gabee", "higher median wins WITHIN an equal weight");
        assert_eq!(basis, "speed_weight+measured_tiebreak");
    }

    #[test]
    fn rate_lookup_requires_a_dash_bounded_prefix() {
        let rates: std::collections::HashMap<String, f64> =
            [("gabee".to_string(), 13.2)].into_iter().collect();
        assert_eq!(
            measured_rate_for(&rates, "dev0", "gabee-qwen3.6-27b"),
            Some(13.2)
        );
        assert_eq!(
            measured_rate_for(&rates, "gabee", "other-model"),
            Some(13.2)
        );
        // "gabee" is NOT a '-'-bounded prefix of "gabeexl-…" — a loose starts_with would lie here.
        assert_eq!(measured_rate_for(&rates, "dev1", "gabeexl-qwen"), None);
        assert_eq!(measured_rate_for(&rates, "dev2", "mihai-qwen"), None);
    }
}

#[cfg(test)]
mod fleet_slot_tests {
    use super::*;

    /// The fan-outs hold `DeviceCfg` (the resolved runtime pool), not the config-file `SwarmDevice`
    /// shape. Two structs, both with a `weight` field, and the compiler is the only thing that
    /// tells them apart.
    fn cfg_w(id: &str, model: &str, weight: u32) -> DeviceCfg {
        DeviceCfg {
            id: id.to_string(),
            model_id: model.to_string(),
            weight,
            enabled: true,
            speed_weight: 1,
            supervision: false,
            is_cloud: false,
        }
    }

    #[test]
    fn fleet_slot_models_counts_slots_not_devices() {
        // The defect: three weight-2 devices are SIX slots, and every planning fan was sized at 3.
        let pool = vec![
            cfg_w("a", "m-a", 2),
            cfg_w("b", "m-b", 2),
            cfg_w("c", "m-c", 2),
        ];
        let slots = fleet_slot_models(&pool);
        assert_eq!(slots.len(), 6, "3 devices x weight 2 must yield 6 slots");
        assert_eq!(slots.iter().filter(|m| *m == "m-a").count(), 2);

        // THE OTHER DIRECTION, or this would pass on a helper that blindly doubles everything: a
        // weight-1 node must still get exactly one, which is the property the original docstring
        // promised and the only reason the cap existed at all.
        let mixed = vec![cfg_w("a", "m-a", 1), cfg_w("b", "m-b", 3)];
        let slots = fleet_slot_models(&mixed);
        assert_eq!(slots.len(), 4);
        assert_eq!(slots.iter().filter(|m| *m == "m-a").count(), 1);

        // A zero/absent weight must not erase a device from the fleet entirely.
        assert_eq!(fleet_slot_models(&[cfg_w("a", "m-a", 0)]).len(), 1);
        assert!(fleet_slot_models(&[]).is_empty());
    }

    #[test]
    fn slot_expansion_does_not_widen_the_skeleton_draft_vote() {
        // The draft path dedups through a HashSet before sizing the vote, so feeding it slots must
        // change NOTHING. Asserted because the dedup's own comment records duplicates being measured
        // dead — 6 requested, exactly 3 survived — and a future edit that drops the dedup would
        // otherwise silently turn a concurrency fix into a doubled, mostly-dead draft fan.
        let pool = vec![
            cfg_w("a", "m-a", 2),
            cfg_w("b", "m-b", 2),
            cfg_w("c", "m-c", 2),
        ];
        let distinct: Vec<String> = pool.iter().map(|d| d.model_id.clone()).collect();
        let slots = fleet_slot_models(&pool);
        let dedup = |models: &[String]| -> usize {
            let mut seen = std::collections::HashSet::new();
            std::iter::once("planner".to_string())
                .chain(models.iter().cloned())
                .filter(|m| seen.insert(m.clone()))
                .count()
        };
        assert_eq!(
            dedup(&slots),
            dedup(&distinct),
            "the vote width must be unchanged"
        );
        assert_eq!(dedup(&slots), 4, "planner + 3 distinct models");
    }

    /// A CAPPED POOL MUST KEEP THE FASTEST NODES, and the operator's ranking is the authority.
    ///
    /// MEASURED: `GOOSE_SWARM_MAX_NODES` sorted by `model_id` alone, so on Mihai's fleet — where
    /// identifiers order `gabee` < `mihai-…` < `qwen3.6-…` — every capped run took `gabee`, the node
    /// his own `speed_weights` ranks LOWEST (1 against local 2 and worksmacstudio 3) and which
    /// benchmarks at 25.88 tok/s against 32.08. Both 1-node cells on disk report `pool: ['gabee']`.
    ///
    /// That is worse than slow: MAX_NODES exists to measure the node curve, so the 1-node CONTROL was
    /// permanently handicapped while the 3-node arm used every machine.
    #[test]
    fn a_capped_pool_keeps_the_fastest_nodes_and_stays_deterministic() {
        let weights: std::collections::HashMap<String, u32> = [
            ("gabee".to_string(), 1),
            ("local".to_string(), 2),
            ("worksmacstudio".to_string(), 3),
        ]
        .into_iter()
        .collect();
        // The real fleet's device ids.
        let mut ids = vec!["mac-gabee", "local-mihai-qwen", "worksmacstudio-qwen"];
        let by_speed = |ids: &mut Vec<&str>| {
            ids.sort_by(|a, b| {
                configured_speed_weight(&weights, b)
                    .cmp(&configured_speed_weight(&weights, a))
                    .then_with(|| a.cmp(b))
            });
        };

        by_speed(&mut ids);
        assert_eq!(
            ids[0], "worksmacstudio-qwen",
            "capping to 1 node must keep the FASTEST host, not the alphabetically first — this is \
             the whole defect: every 1-node control ran on gabee"
        );
        assert_eq!(&ids[..2], &["worksmacstudio-qwen", "local-mihai-qwen"]);

        // DETERMINISM SURVIVES, which was the original sort's legitimate goal: a different input
        // order must produce the same cap, so N is reproducible across runs.
        let mut shuffled = vec!["worksmacstudio-qwen", "mac-gabee", "local-mihai-qwen"];
        by_speed(&mut shuffled);
        assert_eq!(
            shuffled, ids,
            "the cap must not depend on fleet enumeration order"
        );

        // AND AN UNRANKED FLEET FALLS BACK TO ALPHABETICAL, so an operator who never set
        // speed_weights gets exactly the old behaviour rather than an arbitrary one.
        let none: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut flat = vec!["c-node", "a-node", "b-node"];
        flat.sort_by(|a, b| {
            configured_speed_weight(&none, b)
                .cmp(&configured_speed_weight(&none, a))
                .then_with(|| a.cmp(b))
        });
        assert_eq!(flat, vec!["a-node", "b-node", "c-node"]);
    }

    fn dev(id: &str, model: &str, weight: u32, is_cloud: bool) -> DeviceCfg {
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

    /// FALLBACK-GATE visibility: a `speed_weights` key that matches nothing in the pool must be
    /// REPORTED, never silently resolved to 1 for everything it was meant to rank.
    #[test]
    fn a_map_key_matching_no_device_is_reported_unmatched() {
        let weights: std::collections::HashMap<String, u32> = [
            ("gabee".to_string(), 1),
            ("legacybox".to_string(), 5),
            ("workhorse".to_string(), 3),
        ]
        .into_iter()
        .collect();
        let devices = [
            dev("mac-gabee-lmstudio", "gabee-qwen3.8-27b", 1, false),
            // Matched via the MODEL id (the planner-push shape: device id is literally "planner").
            dev("planner", "workhorse-qwen3.8-27b", 1, false),
        ];
        assert_eq!(
            unmatched_speed_weight_patterns(&weights, &devices),
            vec!["legacybox".to_string()],
            "only the key that matched neither a device id nor a model id is unmatched"
        );
    }

    /// THE DEFECT THIS GUARDS. The first version of the residency refresh filtered every device by
    /// `lms ps`, and a cloud model never appears there — so a mixed fleet would have been emptied of its
    /// cloud half the moment any fan refreshed. `is_cloud` exists so the question is only asked where it
    /// has an answer.
    #[test]
    fn a_cloud_device_is_never_judged_by_lm_studio_residency() {
        let devices = [
            dev("local", "gabee-qwen3.8", 2, false),
            dev("zai", "glm-5.3-flash", 1, true),
        ];
        // Whatever the local probe says, the cloud slot must survive: build the live list by hand with
        // an EMPTY resident set, which is what a fleet with LM Studio down looks like.
        let resident: std::collections::HashSet<String> = std::collections::HashSet::new();
        let live: Vec<String> = devices
            .iter()
            .filter(|d| d.is_cloud || resident.contains(&d.model_id))
            .map(|d| d.model_id.clone())
            .collect();
        assert_eq!(
            live,
            vec!["glm-5.3-flash".to_string()],
            "the cloud node survives an empty LM Studio probe"
        );
    }

    /// Slots are per-device CAPACITY, so a weight-2 node contributes two. The snapshot builder and the
    /// live builder must agree on that or a refresh silently halves the fleet's concurrency.
    #[test]
    fn live_slots_preserve_weight_as_capacity() {
        let devices = vec![dev("a", "m-a", 2, false), dev("b", "m-b", 1, false)];
        assert_eq!(fleet_slot_models(&devices).len(), 3);
        let resident: std::collections::HashSet<String> =
            ["m-a".to_string(), "m-b".to_string()].into_iter().collect();
        let live: Vec<String> = devices
            .iter()
            .filter(|d| d.is_cloud || resident.contains(&d.model_id))
            .flat_map(|d| std::iter::repeat_n(d.model_id.clone(), (d.weight as usize).max(1)))
            .collect();
        assert_eq!(live.len(), 3, "weight is capacity, not one slot per device");
    }
}

#[cfg(test)]
mod aux_routing_tests {
    use super::*;

    fn sd(id: &str, model: &str, host: Option<&str>) -> SwarmDevice {
        SwarmDevice {
            id: id.to_string(),
            model_id: model.to_string(),
            weight: 1,
            enabled: true,
            instances: 1,
            host: host.map(String::from),
            provider: None,
            speed_weight: None,
            supervision: None,
        }
    }

    /// THE r6c 18:38 SHAPE: worker + judge-ledgerd-core + judge-web-viz + replan-r0 all on the
    /// planner's node (in-flight 3) while the other two quality hosts sat READY. The pick must
    /// leave the pile and take the idle host; among equally-idle candidates the better-ranked
    /// (faster) one wins because the candidate list arrives rank-ordered.
    #[test]
    fn a_loaded_planner_routes_aux_to_the_idle_quality_candidate() {
        let weights = std::collections::HashMap::from([
            ("workhorse".to_string(), 3u32),
            ("mihai".to_string(), 2u32),
            ("gabee".to_string(), 1u32),
        ]);
        let pool = [
            sd("gabee-qwen", "gabee-qwen3.8-27b", Some("gabee")),
            sd("workhorse-qwen", "workhorse-qwen3.8-27b", Some("workhorse")),
            sd("mihai-qwen", "mihai-qwen3.8-27b", Some("mihai")),
        ];
        let cands = aux_candidate_models(&weights, &pool);
        assert_eq!(
            cands,
            vec![
                "workhorse-qwen3.8-27b".to_string(),
                "mihai-qwen3.8-27b".to_string(),
                "gabee-qwen3.8-27b".to_string(),
            ],
            "candidates arrive best-rank-first so idle ties resolve to the faster host"
        );
        let inflight = std::collections::HashMap::from([
            ("workhorse-qwen3.8-27b".to_string(), 3u32),
            ("gabee-qwen3.8-27b".to_string(), 1u32),
        ]);
        let (model, seen) =
            least_loaded_aux_model("workhorse-qwen3.8-27b", &cands, &inflight, None);
        assert_eq!(model, "mihai-qwen3.8-27b", "the idle host serves the look");
        assert_eq!(seen, 0, "and the event can say what the pick saw");
    }

    /// VA-004, r6d's REAL pool (`pool_resolved` seq 1) and REAL `speed_weights` (config.yaml:
    /// local 2, gabee 1, worksmacstudio 3): the fragments name DEVICE ids, the pool carries no
    /// host, and mihai's model id contains none of them. Before: mihai ranked 1, tied with gabee,
    /// lost the id-order tiebreak — aux order [workhorse, gabee, mihai]. Now the device id is in
    /// the haystack and the operator's order holds.
    #[test]
    fn r6d_s_device_id_fragments_rank_the_aux_pool() {
        let weights = std::collections::HashMap::from([
            ("local".to_string(), 2u32),
            ("gabee".to_string(), 1u32),
            ("worksmacstudio".to_string(), 3u32),
        ]);
        let pool = [
            sd("mac-gabee-qwen3.8-27b", "gabee-qwen3.8-27b", None),
            sd("local-mihai-qwen3.8-27b", "mihai-qwen3.8-27b", None),
            sd(
                "worksmacstudio-workhorse-qwen3.8-27b",
                "workhorse-qwen3.8-27b",
                None,
            ),
        ];
        assert_eq!(
            planner_rank(
                &weights,
                None,
                "local-mihai-qwen3.8-27b",
                "mihai-qwen3.8-27b"
            ),
            (1, 2),
            "the `local` fragment is read off the device id"
        );
        assert_eq!(
            planner_rank(&weights, None, "", "mihai-qwen3.8-27b").1,
            1,
            "without the device id nothing matches — the r6d reading"
        );
        assert_eq!(
            aux_candidate_models(&weights, &pool),
            vec![
                "workhorse-qwen3.8-27b".to_string(),
                "mihai-qwen3.8-27b".to_string(),
                "gabee-qwen3.8-27b".to_string(),
            ],
            "the operator's 3 / 2 / 1"
        );
    }

    /// THE r6d SHAPE (research-ledger-core-q0, looks 6 and 7 at 04:42:30Z / 04:48:21Z): the
    /// counters read workhorse 2, gabee 1, mihai 1, the candidate list ran
    /// [workhorse, gabee, mihai] (gabee and mihai tie on rank, id order), and every look landed
    /// on gabee — the very node running the lane under review. Handing the pick the supervised
    /// lane's model walks it last: the equally idle stranger serves the look. Single-node fleet:
    /// the only model is the avoided one and it still serves — a preference, never a refusal.
    #[test]
    fn a_look_leaves_the_supervised_lanes_node_when_another_is_as_idle() {
        let cands = vec![
            "workhorse-qwen3.8-27b".to_string(),
            "gabee-qwen3.8-27b".to_string(),
            "mihai-qwen3.8-27b".to_string(),
        ];
        let r6d = std::collections::HashMap::from([
            ("workhorse-qwen3.8-27b".to_string(), 2u32),
            ("gabee-qwen3.8-27b".to_string(), 1u32),
            ("mihai-qwen3.8-27b".to_string(), 1u32),
        ]);
        let (without, _) = least_loaded_aux_model("workhorse-qwen3.8-27b", &cands, &r6d, None);
        assert_eq!(
            without, "gabee-qwen3.8-27b",
            "the measured pick: the tie stays on gabee"
        );
        let (model, seen) = least_loaded_aux_model(
            "workhorse-qwen3.8-27b",
            &cands,
            &r6d,
            Some("gabee-qwen3.8-27b"),
        );
        assert_eq!(
            model, "mihai-qwen3.8-27b",
            "the equally idle stranger serves q0's look"
        );
        assert_eq!(seen, 1);
        // Three candidates all idle, avoid = gabee: anything but gabee.
        let (model, _) = least_loaded_aux_model(
            "gabee-qwen3.8-27b",
            &cands,
            &Default::default(),
            Some("gabee-qwen3.8-27b"),
        );
        assert_ne!(
            model, "gabee-qwen3.8-27b",
            "even as planner, the supervised node is last"
        );
        // Strictly fewer still wins over the avoid rank: gabee at 0 against everyone at 1.
        let gabee_idle = std::collections::HashMap::from([
            ("workhorse-qwen3.8-27b".to_string(), 1u32),
            ("mihai-qwen3.8-27b".to_string(), 1u32),
        ]);
        let (model, _) = least_loaded_aux_model(
            "workhorse-qwen3.8-27b",
            &cands,
            &gabee_idle,
            Some("gabee-qwen3.8-27b"),
        );
        assert_eq!(model, "gabee-qwen3.8-27b", "a rank, not an exclusion");
        // Single candidate = the avoided one: it serves.
        let (model, _) = least_loaded_aux_model(
            "gabee-qwen3.8-27b",
            &["gabee-qwen3.8-27b".to_string()],
            &Default::default(),
            Some("gabee-qwen3.8-27b"),
        );
        assert_eq!(
            model, "gabee-qwen3.8-27b",
            "a single-node fleet still serves its looks"
        );
    }

    /// An idle fleet must resolve byte-identically to the frozen planner pick: the planner wins
    /// every tie, including the all-zeros tie, so the fix is a preference that only shows under
    /// measured load.
    #[test]
    fn an_idle_fleet_keeps_every_aux_call_on_the_planner() {
        let cands = vec![
            "workhorse-qwen3.8-27b".to_string(),
            "mihai-qwen3.8-27b".to_string(),
        ];
        let (model, seen) =
            least_loaded_aux_model("workhorse-qwen3.8-27b", &cands, &Default::default(), None);
        assert_eq!(model, "workhorse-qwen3.8-27b");
        assert_eq!(seen, 0);
        // Equal non-zero load is still a tie the planner keeps.
        let even = std::collections::HashMap::from([
            ("workhorse-qwen3.8-27b".to_string(), 1u32),
            ("mihai-qwen3.8-27b".to_string(), 1u32),
        ]);
        let (model, _) = least_loaded_aux_model("workhorse-qwen3.8-27b", &cands, &even, None);
        assert_eq!(
            model, "workhorse-qwen3.8-27b",
            "strictly fewer displaces, equal never does"
        );
    }

    /// The candidate list applies the planner's own quality bar: no low-quant build may serve a
    /// judge look however idle its node is, a small non-27b/dense/coder model is not a candidate,
    /// and cloud devices are excluded (the frozen behavior was local-planner-only). An empty
    /// candidate list leaves the planner serving every aux call.
    #[test]
    fn aux_candidates_apply_the_planner_quality_bar() {
        let weights = std::collections::HashMap::new();
        let mut cloud = sd("bedrock-big", "big-27b-cloud", None);
        cloud.provider = Some("bedrock".to_string());
        let pool = [
            sd("gabee-q4", "gabee-27b-q4_k_m", Some("gabee")),
            sd("mihai-tiny", "mihai-qwen-4b", Some("mihai")),
            cloud,
        ];
        assert!(
            aux_candidate_models(&weights, &pool).is_empty(),
            "low-quant, small and cloud devices are all outside the bar"
        );
        let (model, _) =
            least_loaded_aux_model("planner-27b", &[], &std::collections::HashMap::new(), None);
        assert_eq!(
            model, "planner-27b",
            "no candidates -> the planner keeps the call"
        );
    }

    /// The guard is the whole counting story: entry increments, drop decrements — including the
    /// drop that comes from a CANCELLED future, which is just a drop. A leaked count would route
    /// every later aux call away from a healthy node forever.
    #[test]
    fn the_inflight_guard_counts_entry_and_every_exit() {
        let map = std::sync::Mutex::new(std::collections::HashMap::new());
        {
            let _a = InflightGuard::enter(&map, "m1");
            let _b = InflightGuard::enter(&map, "m1");
            let _c = InflightGuard::enter(&map, "m2");
            assert_eq!(map.lock().unwrap().get("m1"), Some(&2));
            assert_eq!(map.lock().unwrap().get("m2"), Some(&1));
        }
        assert_eq!(map.lock().unwrap().get("m1"), Some(&0));
        assert_eq!(map.lock().unwrap().get("m2"), Some(&0));
    }
}

/// Run `items` across the fleet with at most ONE call in flight PER LIST ENTRY (work-stealing: each
/// item grabs the next free entry and returns it on completion). Callers pass `fleet_slot_models`, so
/// that bound is the per-device capacity the EXECUTE scheduler already honors — a weight-1 node still
/// never has a second request queued behind the first, and a weight-2 node gets the second slot it is
/// configured for. Results come back in item order.
/// Returns ONE element per item, in item order — `Ok(R)` from the lane's closure, or `Err(panic
/// message)` for a lane that panicked. The per-item slot is load-bearing twice over: the
/// sink-review consumer zips results against its input list (a silently dropped lane would shift
/// every later pairing), and the research consumer folds an `Err` into a terminal unanswered row.
///
/// A PANICKED LANE USED TO CONSUME THE WHOLE FAN, silently. The chain: the panic skipped the
/// device push_back, breaking the permits==pool invariant; a later lane's "a device is free
/// whenever a permit is held" expect then fired INSIDE the MutexGuard temporary, poisoning the
/// pool; every remaining lane's `lock().unwrap()` panicked in turn; and the join's
/// `if let Ok(r)` swallowed all of it. Three repairs, each necessary: the device rides a Drop
/// guard back to the pool (unwind runs Drop), both lock sites recover a poisoned mutex instead
/// of cascading (the queue holds Strings — no invariant can be torn mid-push), and the join
/// names every lost lane with a `lane_panicked` event instead of silence.
pub(super) async fn fanout_over_fleet<T, R, F, Fut>(
    context: &str,
    events: &dyn EventSink,
    devices: Vec<String>,
    items: Vec<T>,
    f: F,
) -> Vec<Result<R, String>>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T, String) -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = R> + Send + 'static,
{
    use std::collections::VecDeque;
    struct DeviceReturn {
        pool: Arc<Mutex<VecDeque<String>>>,
        dev: Option<String>,
    }
    impl Drop for DeviceReturn {
        fn drop(&mut self) {
            if let Some(dev) = self.dev.take() {
                // Never a second panic inside Drop (that aborts the process): recover a
                // poisoned lock and return the device anyway.
                match self.pool.lock() {
                    Ok(mut g) => g.push_back(dev),
                    Err(poisoned) => poisoned.into_inner().push_back(dev),
                }
            }
        }
    }
    let devices = if devices.is_empty() {
        vec![String::new()]
    } else {
        // RESOLVED weights, never the raw substring map: the pool entries are MODEL ids and the
        // map's keys are DEVICE-id fragments (r5: only "gabee" matched, the all-tie sort kept
        // config order, and the weight-3 host idled through the round-0 fix wave).
        order_fleet_by_speed(devices, &resolved_fleet_speed_weights(&load_config()))
    };
    // permits == pool size, so a permit holder is always guaranteed a free device to pop —
    // an invariant the DeviceReturn guard keeps true through panicking lanes.
    let permits = Arc::new(tokio::sync::Semaphore::new(devices.len()));
    let pool = Arc::new(Mutex::new(
        devices.into_iter().collect::<VecDeque<String>>(),
    ));
    let mut handles = Vec::with_capacity(items.len());
    for item in items {
        let permits = permits.clone();
        let pool = pool.clone();
        let f = f.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permits
                .acquire_owned()
                .await
                .expect("fleet semaphore never closed");
            let popped = match pool.lock() {
                Ok(mut g) => g.pop_front(),
                Err(poisoned) => poisoned.into_inner().pop_front(),
            };
            // The expect fires OUTSIDE the lock: even a broken invariant costs one lane a
            // panic (named at the join) without poisoning the pool for the rest.
            let dev = popped.expect("a device is free whenever a permit is held");
            let _return_guard = DeviceReturn {
                pool,
                dev: Some(dev.clone()),
            };
            f(item, dev).await
        }));
    }
    let mut results = Vec::with_capacity(handles.len());
    for h in handles {
        match h.await {
            Ok(r) => results.push(Ok(r)),
            Err(e) => {
                let error = match e.try_into_panic() {
                    Ok(payload) => payload
                        .downcast_ref::<&str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| payload.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "panicked with a non-string payload".to_string()),
                    Err(join_err) => join_err.to_string(),
                };
                events.write_value(serde_json::json!({
                    "event": "lane_panicked",
                    "context": context,
                    "error": error,
                }));
                eprintln!(
                    "  {} {context} lane panicked ({error}) — its slot is folded as a failure; \
                     the other lanes' results stand",
                    style("!").red().bold()
                );
                results.push(Err(error));
            }
        }
    }
    results
}
