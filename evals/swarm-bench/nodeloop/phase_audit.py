"""PHASE AUDIT — the standing phase-polish instrument (Mihai, 2026-08-16 22:20: "start something
to CONTINUOUSLY polish the phases... I keep asking for it and you keep ignoring it").

Every audit so far was a one-off (F410, the wall anatomies, the deep hunts) run when the operator
was pushed. This makes the polish MECHANICAL: run on every unit, cut the run into phases from its
own events, compare each phase against the best this campaign has ever recorded (PHASE-BEST.json,
auto-ratcheted), and append every regression and every top wall segment to PHASE-POLISH.md — the
persistent queue the operator loop is bound to mine for the next kaizen. An inefficiency that is
not in the queue does not exist; one that is cannot be silently ignored twice.

Deterministic: reads run.jsonl only, no model calls. Non-fatal by design at the sweep call site.

Usage:  python3 phase_audit.py <run.jsonl> [--quiet]     (prints table, updates queue + bests)
        from phase_audit import audit_phases; audit_phases(path, update_state=False)
"""

from __future__ import annotations

import json
import sys
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
BEST = HERE / "PHASE-BEST.json"
QUEUE = HERE / "PHASE-POLISH.md"

AUDIT_PHASE_VERSION = "pa-1"

# A phase must be this much worse than the recorded best before it is called a regression: the
# replicate spread on wall segments is real, and a queue full of noise trains the reader to skip it.
REGRESSION_FACTOR = 1.35
# Judge verdicts that end/redirect an attempt — the node-time behind them is the dead-attempt cost.
KILL_VERDICTS = {"broken_code", "spec_drift", "no_first_write", "over_reading", "stalled", "looping"}


def _ts(ev):
    t = ev.get("ts")
    if not t:
        return None
    try:
        return datetime.fromisoformat(t.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def audit_phases(path, update_state: bool = True) -> dict:
    events = []
    for line in open(path, errors="replace"):
        try:
            ev = json.loads(line)
        except Exception:
            continue
        if isinstance(ev, dict) and ev.get("event"):
            events.append(ev)
    if not events:
        return {"audit_phase_version": AUDIT_PHASE_VERSION, "error": "no events"}

    def first(name):
        return next((e for e in events if e["event"] == name), None)

    def last(name):
        return next((e for e in reversed(events) if e["event"] == name), None)

    def all_of(name):
        return [e for e in events if e["event"] == name]

    t0 = _ts(first("run_started") or events[0])
    run_id = (first("run_started") or {}).get("run_id")
    finished = last("run_finished")
    t_end = _ts(finished) if finished else _ts(events[-1])

    # ---- PROLOGUE: run start -> first dispatch, segmented -----------------------------------
    research = last("research_completed")
    plan_loaded = last("plan_loaded")
    dispatches = all_of("task_dispatched")
    first_dispatch = _ts(dispatches[0]) if dispatches else None
    seg = {}
    if research and t0:
        seg["research"] = max(0.0, (_ts(research) or t0) - t0)
    # Skeleton + convergence: research end -> plan_convergence (covers redraft rounds too).
    convs = all_of("plan_convergence")
    if convs and research:
        seg["skeleton_convergence"] = max(0.0, (_ts(convs[-1]) or 0) - (_ts(research) or 0))
    details = all_of("detail_completed")
    if details:
        d_ts = [t for t in (_ts(d) for d in details) if t]
        d0 = min((t - d.get("secs", 0)) for t, d in zip(d_ts, details)) if d_ts else None
        if d0:
            seg["detail_fan"] = max(0.0, max(d_ts) - d0)
    if plan_loaded and t0 and first_dispatch:
        seg["prologue_total"] = first_dispatch - t0

    # ---- DAG WINDOW: intervals from task_completed (ts - elapsed_ms) ------------------------
    completes = all_of("task_completed")
    intervals = []
    for c in completes:
        te = _ts(c)
        el = (c.get("elapsed_ms") or 0) / 1000.0
        if te and el > 0:
            intervals.append((te - el, te, c.get("task_id")))
    cv = all_of("complete_verify")
    t_complete0 = _ts(cv[0]) if cv else None
    dag_end = t_complete0 or t_end
    dag_secs = (dag_end - first_dispatch) if (first_dispatch and dag_end) else None
    occupancy = None
    low_conc_secs = None
    if intervals and dag_secs and dag_secs > 0:
        marks = []
        for s, e, _ in intervals:
            marks.append((max(s, first_dispatch), 1))
            marks.append((min(e, dag_end), -1))
        marks.sort()
        busy_area = 0.0
        low = 0.0
        cur = 0
        prev = first_dispatch
        for t, d in marks:
            if t > prev:
                busy_area += cur * (t - prev)
                if cur <= 1:
                    low += t - prev
            cur += d
            prev = t
        if dag_end > prev:
            low += dag_end - prev
        occupancy = round(busy_area / dag_secs, 2)
        low_conc_secs = round(low, 0)
    if dag_secs:
        seg["dag_window"] = dag_secs

    # ---- DEAD ATTEMPTS: node-time behind kill verdicts --------------------------------------
    dead = 0.0
    kills = []
    last_obs = {}
    for ev in events:
        if ev["event"] == "judge_observed":
            last_obs[ev.get("task_id")] = ev.get("elapsed_secs") or 0
        elif ev["event"] == "judge_verdict" and ev.get("verdict") in KILL_VERDICTS:
            secs = last_obs.get(ev.get("task_id"), 0)
            dead += secs
            kills.append({"task": ev.get("task_id"), "verdict": ev.get("verdict"), "age_secs": secs})
    seg["dead_attempt_node_secs"] = round(dead, 0)

    # ---- REPAIR: first complete_verify -> end, with round trajectory ------------------------
    if t_complete0 and t_end:
        seg["repair_phase"] = t_end - t_complete0
    rounds = [{"round": e.get("round"), "findings": e.get("findings")} for e in cv]
    fix_secs = [e.get("secs") for e in all_of("complete_fix_completed") if e.get("secs")]
    fix_caps = sorted({e.get("fix_cap_secs") for e in all_of("complete_fix_dispatched")
                       if e.get("fix_cap_secs")})

    if t0 and t_end:
        seg["wall_total"] = t_end - t0

    out = {
        "audit_phase_version": AUDIT_PHASE_VERSION,
        "run_id": run_id,
        "finished": bool(finished),
        "phase_secs": {k: round(v, 0) for k, v in seg.items()},
        "dag_occupancy": occupancy,
        "dag_low_concurrency_secs": low_conc_secs,
        "kills": kills,
        "verify_rounds": rounds,
        "fix_secs": fix_secs,
        "fix_caps": fix_caps,
    }

    # ---- RATCHET + QUEUE (only for finished runs; a partial run's phases are not comparable) -
    if update_state and finished:
        best = {}
        if BEST.is_file():
            try:
                best = json.loads(BEST.read_text())
            except Exception:
                best = {}
        regressions = []
        for k, v in seg.items():
            if k in ("dead_attempt_node_secs",):
                ref = best.get(k, {}).get("best_secs")
                if ref is not None and v > max(ref * REGRESSION_FACTOR, ref + 300):
                    regressions.append((k, v, ref))
                if ref is None or v < ref:
                    best[k] = {"best_secs": v, "run_id": run_id}
                continue
            ref = best.get(k, {}).get("best_secs")
            if ref is not None and v > ref * REGRESSION_FACTOR:
                regressions.append((k, v, ref))
            if ref is None or v < ref:
                best[k] = {"best_secs": v, "run_id": run_id}
        BEST.write_text(json.dumps(best, indent=2))
        top = sorted(((k, v) for k, v in seg.items()
                      if k not in ("wall_total", "prologue_total", "dag_window")),
                     key=lambda kv: -kv[1])[:3]
        with open(QUEUE, "a") as q:
            q.write(f"\n## {run_id}  wall {seg.get('wall_total', 0)/60:.0f} min  "
                    f"(audited {datetime.now().isoformat(timespec='seconds')})\n")
            for k, v in top:
                b = best.get(k, {}).get("best_secs")
                q.write(f"- {k}: {v/60:.1f} min (best ever {b/60:.1f})\n")
            for k, v, ref in regressions:
                q.write(f"- REGRESSION {k}: {v/60:.1f} min vs best {ref/60:.1f} — "
                        f"find the cause before the next batch ships\n")
            if occupancy is not None:
                q.write(f"- dag occupancy {occupancy} of pool; "
                        f"{(low_conc_secs or 0)/60:.1f} min at concurrency <=1\n")
        out["regressions"] = [{"phase": k, "secs": v, "best": r} for k, v, r in regressions]

    return out


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not args:
        print("usage: phase_audit.py <run.jsonl> [--quiet]", file=sys.stderr)
        return 2
    res = audit_phases(args[0], update_state="--no-state" not in sys.argv)
    if "--quiet" not in sys.argv:
        print(json.dumps(res, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
