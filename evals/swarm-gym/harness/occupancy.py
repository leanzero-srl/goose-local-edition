"""Deterministic TIME-based fleet occupancy from a run's own JSONL events — no brain, no key.

Why this exists alongside `monitor.py`: that module measures idleness by DISPATCH COUNT share, which
cannot see the dominant stall shape in this system. A node holding ONE 78-minute task and a node holding
ten 2-minute tasks look equally busy by count, and the single serial sink — the biggest idle source
measured — is exactly one dispatch. Counting tasks says "balanced"; counting TIME says the fleet was idle.

MEASURED over the ~/goose-builds corpus (31 runs with complete phase data, 3-node fleet):

    occupancy      median 22.1%, mean 24.4%, max 47.2%
    integrate-verify   36% of ALL node-busy time — one serial task on one node
    longest task   median 13.9 min, max 78.9 min

Occupancy is `sum(task elapsed_ms) / (execute wall-clock * pool size)`. The numerator comes from
`task_completed.elapsed_ms` (the engine's own per-attempt measurement, not a timestamp subtraction, so a
retried task contributes each attempt honestly); the denominator's wall window runs from the first
`task_dispatched` to the last `task_completed`, so PLANNING is excluded and this measures the execute
phase only — the phase task-parallelism can actually affect.

There is no "node idle" event to read, and adding one would change the engine; this reconstructs it from
events that already exist, so it works on every archived run.
"""

from __future__ import annotations

import glob
import json
import os
from datetime import datetime
from typing import Dict, List, Optional

from .contracts import Finding

# A run that leaves most of the fleet idle is the thing this harness exists to catch.
LOW_OCCUPANCY = 0.35
# One task owning this much of all busy time is a serialization bottleneck, not a busy fleet.
DOMINANT_TASK_FRAC = 0.30


def _ts(value: Optional[str]) -> Optional[float]:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def latest_run_log(build_dir: str) -> Optional[str]:
    """The NEWEST run log in a build dir. One log = one run; a dir gets a second log whenever a dead run
    is relaunched, and merging them yields a Frankenstein run whose wall window spans both. The filename
    carries a sortable timestamp, so the last name is the newest."""
    logs = sorted(glob.glob(os.path.join(build_dir, ".swarm", "run-swarm-*.jsonl")))
    return logs[-1] if logs else None


def occupancy(log_path: str) -> Dict[str, object]:
    pool: List[str] = []
    busy_ms = 0
    per_task: Dict[str, int] = {}
    first_dispatch: Optional[float] = None
    last_complete: Optional[float] = None

    with open(log_path, errors="replace") as fh:
        for line in fh:
            try:
                e = json.loads(line)
            except ValueError:
                continue
            kind = e.get("event")
            when = _ts(e.get("ts"))
            if kind == "run_started":
                pool = [str(d) for d in (e.get("pool") or [])]
            elif kind == "task_dispatched" and when and first_dispatch is None:
                first_dispatch = when
            elif kind == "task_completed":
                ms = int(e.get("elapsed_ms") or 0)
                busy_ms += ms
                per_task[str(e.get("task_id"))] = per_task.get(str(e.get("task_id")), 0) + ms
                if when:
                    last_complete = when

    n = len(pool)
    wall_s = (last_complete - first_dispatch) if (first_dispatch and last_complete) else 0.0
    capacity_s = wall_s * n
    busy_s = busy_ms / 1000.0
    occ = (busy_s / capacity_s) if capacity_s > 0 else 0.0
    hottest, hottest_ms = ("", 0)
    if per_task:
        hottest, hottest_ms = max(per_task.items(), key=lambda kv: kv[1])
    return {
        "log": os.path.basename(log_path),
        "n_devices": n,
        "execute_wall_min": round(wall_s / 60.0, 1),
        "node_busy_min": round(busy_s / 60.0, 1),
        "occupancy": round(occ, 3),
        "hottest_task": hottest,
        "hottest_task_min": round(hottest_ms / 60000.0, 1),
        "hottest_task_frac": round(hottest_ms / busy_ms, 3) if busy_ms else 0.0,
        "measurable": bool(n and wall_s > 0 and busy_ms > 0),
    }


def findings_for(occ: Dict[str, object]) -> List[Finding]:
    """Findings an outsider can re-derive from the same log. Silent when the run is not measurable — an
    absent measurement is never reported as a healthy one."""
    if not occ.get("measurable"):
        return []
    out: List[Finding] = []
    pct = float(occ["occupancy"]) * 100
    if float(occ["occupancy"]) < LOW_OCCUPANCY:
        out.append(
            Finding(
                id="occupancy-low",
                dimension="cluster",
                severity="high",
                text=(
                    f"the fleet was busy only {pct:.0f}% of the execute phase "
                    f"({occ['node_busy_min']} node-min of {occ['n_devices']} x "
                    f"{occ['execute_wall_min']} min available)"
                ),
                evidence=str(occ),
                fix_hint=(
                    "check the hottest task first — one long serial task idles every other node, and "
                    "task-count balance will look fine while that happens"
                ),
            )
        )
    if float(occ["hottest_task_frac"]) > DOMINANT_TASK_FRAC:
        out.append(
            Finding(
                id="occupancy-serial-task",
                dimension="cluster",
                severity="high",
                text=(
                    f"`{occ['hottest_task']}` alone is "
                    f"{float(occ['hottest_task_frac']) * 100:.0f}% of all node-busy time "
                    f"({occ['hottest_task_min']} min on ONE node)"
                ),
                evidence=str(occ),
                fix_hint=(
                    "if this is integrate-verify, GOOSE_SWARM_FAN_VERIFY splits the shardable half into "
                    "per-module verify tasks and leaves a thin end-to-end join"
                ),
            )
        )
    return out
