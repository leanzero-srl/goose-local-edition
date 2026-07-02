"""Deterministic idle-node monitor (EXPLORATORY mode): from per-device dispatch counts, flag
starved / idle / underused nodes and emit a scoped KnobDelta the tweaker can apply — no brain, no key.

Fed by the cluster verdict's `dispatched` map (device -> tasks dispatched; zeros included for
enabled-but-idle nodes, which verifier.cluster.assess fills from the run_started pool). This is the
"catch idle nodes and fix" piece: idle_scan -> findings_for / deterministic_fix -> tweaker.apply.
"""

from __future__ import annotations

from typing import Dict, List, Optional

from .contracts import Finding, KnobChange, KnobDelta

IDLE_FRAC = 0.05        # took under 5% of dispatched work -> "idle"
UNDERUSE_FRAC = 0.5     # took under half an even split -> "underused"
BUMP_WEIGHT = 3         # heuristic target for `swarm pool weight <dev> N` (blind set; see caveat)


def idle_scan(dispatched: Dict[str, int]) -> Dict[str, object]:
    devices = list(dispatched)
    total = sum(int(v) for v in dispatched.values())
    n = len(devices)
    fair = (total / n) if n else 0.0
    shares = {d: (int(dispatched[d]) / total if total else 0.0) for d in devices}
    starved = [d for d in devices if int(dispatched[d]) == 0]
    idle = [d for d in devices if 0 < shares[d] < IDLE_FRAC]
    underused = [
        d for d in devices
        if int(dispatched[d]) > 0 and fair > 0 and int(dispatched[d]) < UNDERUSE_FRAC * fair
    ]
    return {
        "total_dispatched": total, "n_devices": n, "fair_share": round(fair, 2),
        "dispatched": {d: int(dispatched[d]) for d in devices},
        "shares": {d: round(shares[d], 3) for d in devices},
        "starved": starved, "idle": idle, "underused": underused,
        "balanced": not (starved or idle or underused),
    }


def findings_for(scan: Dict[str, object]) -> List[Finding]:
    out: List[Finding] = []
    if int(scan.get("n_devices", 0)) <= 1:
        return out  # a single-node pool can't be "unbalanced"
    if scan.get("starved"):
        out.append(Finding(
            id="monitor-starved", dimension="cluster", severity="high",
            text=f"idle nodes got ZERO work: {scan['starved']}", evidence=str(scan["dispatched"]),
            fix_hint="re-enable + raise pool weight; check `lms ps` placement / planner routing"))
    if scan.get("underused"):
        out.append(Finding(
            id="monitor-underused", dimension="cluster", severity="medium",
            text=f"nodes under half fair share: {scan['underused']} (fair={scan['fair_share']})",
            evidence=str(scan["shares"]), fix_hint="raise pool weight on the underused nodes"))
    if scan.get("idle"):
        out.append(Finding(
            id="monitor-idle", dimension="cluster", severity="low",
            text=f"nodes nearly idle (<5% share): {scan['idle']}", evidence=str(scan["shares"])))
    return out


def deterministic_fix(scan: Dict[str, object], bump_to: int = BUMP_WEIGHT) -> Optional[KnobDelta]:
    """Scoped KnobDelta (re-enable + weight bump) for starved/underused nodes; None if nothing to do.
    Feeds straight into tweaker.apply — all knobs are pool.* (inside ALLOWED_PREFIXES).

    CAVEAT (honest): bump_to is a BLIND absolute set (we don't read current weights), so on an
    already-high-weight node it can be a no-op or a down-tune. A follow-up that reads current pool
    weights would make this correct rather than heuristic.
    """
    targets = list(dict.fromkeys(list(scan.get("starved") or []) + list(scan.get("underused") or [])))
    if not targets or int(scan.get("n_devices", 0)) <= 1:
        return None
    changes: List[KnobChange] = []
    for dev in scan.get("starved") or []:
        changes.append(KnobChange(knob=f"pool.{dev}.enabled", from_value=None, to_value=True))
    for dev in targets:
        changes.append(KnobChange(knob=f"pool.{dev}.weight", from_value=None, to_value=int(bump_to)))
    kd = KnobDelta(
        rationale=f"idle-node auto-tune: bump {targets} (deterministic monitor, no brain)",
        changes=changes)
    kd.scope_ok = True
    return kd


def aggregate(per_device_runs: List[Dict[str, Dict]]) -> Dict[str, int]:
    """Sum `dispatched` across a session's turns for the session-level idle view."""
    total: Dict[str, int] = {}
    for pd in per_device_runs or []:
        for dev, s in (pd or {}).items():
            total[dev] = total.get(dev, 0) + int((s or {}).get("dispatched", 0))
    return total
