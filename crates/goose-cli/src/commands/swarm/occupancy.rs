//! Honest fleet DISPATCH-OCCUPANCY (§1-#10 / #104): the fraction of available node-time a node
//! actually HELD a task, from the scheduler's per-device `busy_ms` — never a CPU sample.
//! Observability only: `swarm.occupancy` in config, env wins; OFF leaves `run_finished.phases`
//! byte-identical. Moved out of swarm.rs under the incremental-split law (VA-139's memory wiring
//! paid for by this extraction).

use super::{load_config, swarm_gate_cfg};

/// Emit the honest fleet dispatch-occupancy alongside run_finished. Observability only. `swarm.occupancy` in
/// config; env wins. OFF => run_finished is byte-identical.
fn occupancy_on() -> bool {
    swarm_gate_cfg("GOOSE_SWARM_OCCUPANCY", load_config().occupancy)
}

/// Honest fleet DISPATCH-OCCUPANCY: the fraction (0-100%) of available node-time a node actually HELD a task,
/// computed from the scheduler's per-device task-holding time (`busy_ms`), NOT from CPU/generation sampling.
///
/// This distinction is the whole point (§1-#10): a `PARALLEL:1` local model blocked on I/O reads 0% CPU while
/// it is BUSY holding a dispatched task, so a CPU-sampled "12% util / 85% idle" figure overstates idleness and
/// any headroom claim resting on it. Dispatch-occupancy cannot lie that way — a node either holds a task or it
/// does not.
///
/// `busy_node_ms` = Σ busy_ms over devices; `wall_ms` = the wall-clock window; `node_count` = fleet size. A
/// device runs tasks sequentially so its busy_ms ≤ wall_ms, hence the sum ≤ wall_ms·node_count and the ratio
/// is a true fraction (clamped only against float rounding). Pure — unit-testable without a run.
fn dispatch_occupancy_pct(busy_node_ms: u64, wall_ms: f64, node_count: usize) -> f64 {
    if node_count == 0 || wall_ms <= 0.0 {
        return 0.0;
    }
    let cap = wall_ms * node_count as f64;
    ((busy_node_ms as f64 / cap) * 100.0).clamp(0.0, 100.0)
}

/// The `run_finished.phases` annotation (swarm.rs, beside the write). OFF => `phases_value` is
/// untouched and run_finished is byte-identical. ON => add the fraction of node-time a node actually
/// HELD a task (`busy_node_ms`, the scheduler's per-device busy_ms summed), NOT a CPU sample — the
/// instrument the "12% util" figure needs before any headroom claim (B1) may be trusted. `_run`
/// divides the SAME execute-phase busy time by the WHOLE-run wall (the analog of the historical
/// whole-run util); `_execute` divides by the execute window alone.
pub(super) fn annotate_phases(
    phases_value: &mut serde_json::Value,
    busy_node_ms: u64,
    execute_m: f64,
    total_m: f64,
    fleet_size: usize,
) {
    if !occupancy_on() {
        return;
    }
    let occ_execute = dispatch_occupancy_pct(busy_node_ms, execute_m * 60_000.0, fleet_size);
    let occ_run = dispatch_occupancy_pct(busy_node_ms, total_m * 60_000.0, fleet_size);
    let busy_node_min = (busy_node_ms as f64 / 60_000.0 * 10.0).round() / 10.0;
    let round1 = |x: f64| (x * 10.0).round() / 10.0;
    if let Some(obj) = phases_value.as_object_mut() {
        obj.insert("fleet_nodes".into(), serde_json::json!(fleet_size));
        obj.insert("busy_node_min".into(), serde_json::json!(busy_node_min));
        obj.insert(
            "dispatch_occupancy_execute_pct".into(),
            serde_json::json!(round1(occ_execute)),
        );
        obj.insert(
            "dispatch_occupancy_run_pct".into(),
            serde_json::json!(round1(occ_run)),
        );
    }
    eprintln!(
        "dispatch-occupancy (node HELD a task, not CPU): execute {:.1}% · whole-run {:.1}% across {} node(s) ({:.1} node-min busy)",
        occ_execute, occ_run, fleet_size, busy_node_min
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_occupancy_is_honest_fraction() {
        // 3 nodes, 10-min window = 30 node-min available. 15 node-min held => 50%.
        assert_eq!(
            dispatch_occupancy_pct(15 * 60_000, 10.0 * 60_000.0, 3),
            50.0
        );
        // Fully idle fleet.
        assert_eq!(dispatch_occupancy_pct(0, 10.0 * 60_000.0, 3), 0.0);
        // Degenerate denominators never divide-by-zero or emit garbage.
        assert_eq!(dispatch_occupancy_pct(999, 0.0, 3), 0.0);
        assert_eq!(dispatch_occupancy_pct(999, 10.0, 0), 0.0);
        // Rounding-slop above 100 is clamped, never > 100%.
        assert_eq!(
            dispatch_occupancy_pct(31 * 60_000, 10.0 * 60_000.0, 3),
            100.0
        );
    }
}
