"""Cluster verification from the event log + RunResult: device distribution, starved devices,
tool/MCP counts, retries — no model involved."""

from __future__ import annotations

from typing import List, Tuple

from ..contracts import DimensionVerdict, Evidence, Finding


def assess(ev: Evidence) -> Tuple[DimensionVerdict, List[Finding]]:
    pd = ev.run.per_device
    pool: List[str] = []
    for e in ev.events:
        if e.get("event") == "run_started":
            pool = [d.get("id") for d in e.get("pool", [])]

    findings: List[Finding] = []
    dispatched = {d: int(pd.get(d, {}).get("dispatched", 0)) for d in pool}
    starved = [d for d in pool if dispatched.get(d, 0) == 0]
    total_retries = sum(int(v.get("retries", 0)) for v in pd.values())
    total_mcp = sum(int(v.get("mcp_calls", 0)) for v in pd.values())

    status = "pass"
    if starved and len(pool) > 1:
        status = "warn"
        findings.append(
            Finding(
                id="cluster-starved",
                dimension="cluster",
                severity="medium",
                text=f"enabled devices received no work: {starved}",
                fix_hint="check `lms ps` placement, raise their weight, or vary the planner's model routing",
            )
        )
    if total_retries > 0:
        findings.append(
            Finding(
                id="cluster-retries",
                dimension="cluster",
                severity="low",
                text=f"{total_retries} transient retries during the run",
                fix_hint="pre-warm models and/or raise max_attempts",
            )
        )

    detail = {
        "dispatched": dispatched,
        "starved": starved,
        "retries": total_retries,
        "mcp_calls": total_mcp,
    }
    return DimensionVerdict(status=status, source="cluster", detail=detail), findings
