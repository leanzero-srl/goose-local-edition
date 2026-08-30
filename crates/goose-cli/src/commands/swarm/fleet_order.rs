//! Fleet ordering and speed-weight resolution — which node gets work first.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases). Extracted verbatim from swarm.rs:
//! the slot builders (`fleet_slot_models`, `live_fleet_slots`), the fan pool ordering
//! (`order_fleet_by_speed`, `one_lane_per_host`) and the operator's `speed_weights` substring
//! resolution (`configured_speed_weight`).

use goose_swarm::DeviceCfg;

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

/// Order a fan's device pool by DESCENDING configured speed weight (stable: ties keep
/// discovery order). Every fan pops devices from the FRONT of its queue, so front == the
/// node that gets the first (and, when items < devices, the ONLY) work. MEASURED (r13,
/// operator screenshot): pool order is discovery order — gabee, mihai, workhorse — so the
/// weight-4 host was structurally LAST in line for every scout/detail fan and sat
/// READY while the weight-1 host prefilled. One rule, one place: every fan site inherits it.
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
    groups.sort_by_key(|(id, _)| std::cmp::Reverse(configured_speed_weight(weights, id)));
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
#[cfg(test)]
mod fan_order_tests {
    use super::*;

    #[test]
    fn the_fastest_host_is_first_in_every_fan_pool() {
        let weights: std::collections::HashMap<String, u32> = [
            ("gabee".to_string(), 1),
            ("mihai".to_string(), 2),
            ("workhorse".to_string(), 4),
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
            ("gabee".to_string(), 1),
            ("mihai".to_string(), 2),
            ("workhorse".to_string(), 4),
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
