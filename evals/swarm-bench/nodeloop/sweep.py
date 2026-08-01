#!/usr/bin/env python3
"""The unattended loop: measure the swarm's DISPATCH QUALITY arm by arm, forever.

Why this exists rather than another node-count sweep. Two probes against the fleet as it stands
(3 hosts, all serving one identifier) established that LM Studio exposes exactly ONE addressable
worker: a host-qualified instance name is rejected with HTTP 400, and three concurrent calls on
the shared identifier were all served by a single host while the other two never left idle. The
engine's own `run_started.pool` agrees — every run on disk built a 1-device pool. Node count is
therefore not a variable on this fleet, and pretending otherwise measures nothing.

What IS a variable, and what the three runs on disk already show, is how specific an instruction
each worker receives:

    run              detail fell back to the architect one-liner   build score
    swarm-1node-r0   2 of 14                                       44.2%
    swarm-2node-r0   1 of 16                                       86.7%
    swarm-3node-r0   0 of 14                                       90.0%

So the loop's unit is an ARM (one dispatch-quality lever) measured over replicates, and its
primary readout is a deterministic mechanism count from dispatch_audit.py, not a score delta —
because a 46-point replicate spread makes any 1-vs-1 score comparison uninterpretable.

Operating rules this file implements, each of which has already cost a real overnight run:
  - a result not on disk did not happen: every unit persists result.json the moment it finishes
  - resumable: a unit with a complete result is skipped, so a killed loop resumes where it stopped
  - one bad unit never kills the sweep, and SystemExit is NOT an Exception
  - a provider/fleet blip is retried with backoff, never recorded as a score of zero
  - a flat timeout measures the timeout: timed_out is recorded and checked before interpreting
  - children are killed by process GROUP, or an orphan contends for the fleet unnoticed
  - it ends only on the STOP sentinel, never on a counter — when the backlog drains it raises the
    replicate target and keeps going, which is exactly what a 46-point spread calls for
"""
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
import traceback
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent / "bench"
sys.path.insert(0, str(BENCH))   # run_build, score_build, vendor_service
sys.path.insert(0, str(HERE))    # nodeloop's own instrument — must precede BENCH on the path

import dispatch_audit  # noqa: E402

OUT = HERE.parent / "runs" / "nodeloop"
STOP = HERE / "STOP"
QUEUE = HERE / "QUEUE"
PORT_BASE = 8930
TIMEOUT = 16200          # 4.5h. Measured swarm walls are 1.9-2.5h; a cap that truncates the work
                         # measures the cap, not the entrant.
ENTRANT = "swarm-1node"  # the pool is 1 device regardless; this names it honestly.
MIN_REPS = 3             # n=1 is uninterpretable against the measured spread.
TRANSIENT = ("500", "502", "503", "529", "overloaded", "rate limit", "throttl",
             "connection reset", "stream decode", "temporarily", "unreachable")
MAX_ATTEMPTS = 3
BACKOFF = (60, 240)

# Each arm sets exactly ONE thing against baseline, so any delta is attributable to it.
# `gate` is the prediction written down BEFORE the run, where it can fail.
ARMS = [
    {
        "name": "baseline",
        "env": {},
        "gate": "establishes the replicate spread and the detail-fallback rate. Re-measured "
                "rather than assumed: a stale baseline turns fleet drift into a false win.",
    },
    {
        "name": "kind_prompt",
        "env": {"GOOSE_SWARM_KIND_PROMPT": "1"},
        "gate": "72-80% of dispatches currently receive rules written for another job, and 3-5 "
                "per run own a test_*.py while being told never to read test files. Gating rules "
                "by task kind should cut kind_mismatch_pct toward zero. A prior adversarial pass "
                "refuted the naive version and put the score recovery in single digits, so the "
                "mechanism count is the readout here, not the build score.",
    },
    {
        "name": "scoped_contracts",
        "env": {"GOOSE_SWARM_SCOPED_CONTRACTS": "1"},
        "gate": "every worker currently receives the FULL frozen-contract bundle rather than its "
                "DAG neighborhood, so irrelevant interface text grows with the plan's width. "
                "scope_contract_bundle (coherence.rs:303) is written and unused.",
    },
    {
        "name": "doc_prefetch",
        "env": {"GOOSE_SWARM_DOC_PREFETCH": "1"},
        "gate": "doc_facts is the only un-paraphrased scout->worker channel. Tier C is graded on "
                "vendor-doc compliance and is the tier that collapsed (14.3%) in the run whose "
                "meridian module got a 95-char brief.",
    },
]


def now() -> str:
    return datetime.now().strftime("%H:%M:%S")


def log(msg: str) -> None:
    print(msg, flush=True)


def unit_dir(arm: str, rep: int) -> Path:
    return OUT / f"{arm}-r{rep}"


def result_path(arm: str, rep: int) -> Path:
    return unit_dir(arm, rep) / "nodeloop-result.json"


def complete(arm: str, rep: int) -> bool:
    p = result_path(arm, rep)
    if not p.is_file():
        return False
    try:
        r = json.loads(p.read_text())
    except Exception:
        return False
    return r.get("audit_version") == dispatch_audit.AUDIT_VERSION


def looks_transient(tail: str) -> bool:
    low = (tail or "").lower()
    return any(t in low for t in TRANSIENT)


def run_unit(arm: dict, rep: int, port: int) -> dict:
    """One episode: build, grade the artifact, then grade the INSTRUCTIONS it was given."""
    import run_build  # imported late so a syntax error there cannot stop the loop from starting

    prev = dict(os.environ)
    try:
        for k, v in arm["env"].items():
            os.environ[k] = v
        verdict = run_build.run(ENTRANT, rep, OUT, TIMEOUT, port)
    finally:
        os.environ.clear()
        os.environ.update(prev)

    # run_build names its outputs after the ENTRANT, so every arm's rep0 would overwrite the last
    # one's tree AND its vendor trace. Re-home both under the arm, or the evidence for an arm is
    # silently the next arm's.
    src = OUT / f"{ENTRANT}-r{rep}"
    dst = unit_dir(arm["name"], rep)
    if src.exists():
        if dst.exists():
            shutil.rmtree(dst)
        src.rename(dst)
    trace_src = OUT / f"trace-{ENTRANT}-r{rep}.jsonl"
    if trace_src.exists():
        trace_src.replace(dst / "vendor-trace.jsonl")

    audit = {}
    run_log = dst / "run.jsonl"
    if run_log.is_file():
        try:
            audit = dispatch_audit.audit(run_log)
            audit.pop("per_dispatch", None)   # kept in run.jsonl; the summary is what we compare
        except Exception as exc:  # noqa: BLE001 - a broken instrument must be visible, not fatal
            audit = {"audit_error": f"{type(exc).__name__}: {exc}"}

    return {
        "arm": arm["name"],
        "rep": rep,
        "env": arm["env"],
        "gate": arm["gate"],
        "finished_at": datetime.now().isoformat(timespec="seconds"),
        "score": verdict.get("score"),
        "tiers": verdict.get("tiers"),
        "timed_out": (verdict.get("agent") or {}).get("timed_out"),
        "wall_secs": (verdict.get("agent") or {}).get("secs"),
        "actual_pool": verdict.get("actual_pool"),
        "actual_nodes": verdict.get("actual_nodes"),
        "scorer_version": verdict.get("scorer_version"),
        "audit_version": audit.get("audit_version") or dispatch_audit.AUDIT_VERSION,
        "audit": audit,
    }


def kill_strays() -> None:
    """An orphaned engine contends for the shared fleet and poisons the next unit silently."""
    try:
        out = subprocess.run(["pgrep", "-f", "goose swarm run"],
                             capture_output=True, text=True, timeout=20)
        for pid in [p for p in out.stdout.split() if p.strip().isdigit()]:
            try:
                os.killpg(os.getpgid(int(pid)), signal.SIGKILL)
                log(f"[warn] killed stray engine pgroup for pid {pid}")
            except (ProcessLookupError, PermissionError):
                pass
    except Exception:
        pass


def backlog(target_reps: int) -> list[tuple[dict, int]]:
    units = []
    for rep in range(target_reps):
        for arm in arms_now():
            if not complete(arm["name"], rep):
                units.append((arm, rep))
    return units


def arms_now() -> list[dict]:
    """ARMS plus anything appended to the QUEUE file, so arms can be added without a restart.

    A running interpreter does not see source edits, so a new arm added to ARMS in this file would
    never reach a loop that is already up. The QUEUE is re-read every pass.
    """
    arms = list(ARMS)
    if QUEUE.is_file():
        for raw in QUEUE.read_text().splitlines():
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            name, _, rest = line.partition(" ")
            env = {}
            for tok in rest.split():
                if "=" in tok:
                    k, _, v = tok.partition("=")
                    env[k] = v
            if name and name not in {a["name"] for a in arms}:
                arms.append({"name": name, "env": env, "gate": "(from QUEUE)"})
    return arms


def summarise() -> None:
    """Mechanism first, score second — the score alone cannot clear a 46-point spread."""
    rows: dict[str, list[dict]] = {}
    for f in sorted(OUT.glob("*/nodeloop-result.json")):
        try:
            r = json.loads(f.read_text())
        except Exception:
            continue
        rows.setdefault(r["arm"], []).append(r)
    if not rows:
        return
    log("")
    log(f"{'arm':<18}{'n':>2}  {'score mean':>10} {'spread':>7}   "
        f"{'fallbacks':>9} {'kind-mismatch%':>15}  pool")
    for arm, rs in sorted(rows.items()):
        ok = [r for r in rs if not r.get("timed_out")]
        sc = [r["score"] for r in ok if r.get("score") is not None]
        fb = [r["audit"].get("detail_fallback_count") for r in ok
              if isinstance(r.get("audit"), dict)
              and r["audit"].get("detail_fallback_count") is not None]
        km = [r["audit"].get("kind_mismatch_pct") for r in ok
              if isinstance(r.get("audit"), dict)
              and r["audit"].get("kind_mismatch_pct") is not None]
        pools = {r.get("actual_nodes") for r in rs}
        mean = f"{sum(sc) / len(sc):.1%}" if sc else "-"
        spread = f"{(max(sc) - min(sc)) * 100:.0f}pts" if len(sc) > 1 else "-"
        fbs = f"{sum(fb) / len(fb):.1f}" if fb else "-"
        kms = f"{sum(km) / len(km):.1f}" if km else "-"
        log(f"{arm:<18}{len(rs):>2}  {mean:>10} {spread:>7}   {fbs:>9} {kms:>15}  {sorted(pools)}")
    log("")


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    log("=" * 78)
    log(f"nodeloop starting {datetime.now().isoformat(timespec='seconds')}  "
        f"pid={os.getpid()}  audit={dispatch_audit.AUDIT_VERSION}")
    log(f"stop with: touch {STOP}")
    log("=" * 78)

    target = MIN_REPS
    durations: list[float] = []
    port = PORT_BASE

    while True:
        if STOP.is_file():
            log(f"[stop] {now()} STOP sentinel present — exiting cleanly")
            summarise()
            return 0

        todo = backlog(target)
        if not todo:
            # Never end on a counter. More replicates is the single most useful thing this loop
            # can do next, because every verdict here is limited by n, not by ideas.
            target += 1
            log(f"[grow] {now()} backlog drained — raising replicate target to n={target}")
            summarise()
            continue

        arm, rep = todo[0]
        eta = ""
        if durations:
            avg = sum(durations) / len(durations)
            done_at = time.time() + avg
            eta = (f"  (~{datetime.fromtimestamp(done_at).strftime('%H:%M')}, "
                   f"{len(todo)} units left ~"
                   f"{datetime.fromtimestamp(time.time() + avg * len(todo)).strftime('%a %H:%M')})")
        nxt = f"{todo[1][0]['name']} rep{todo[1][1]}" if len(todo) > 1 else "raise n"
        log("")
        log(f">>> {now()}  NOW: {arm['name']} rep{rep}   [{len(todo)} in backlog, n target {target}]"
            f"{eta}")
        log(f"    NEXT: {nxt}")
        log(f"    gate: {arm['gate']}")

        kill_strays()
        started = time.time()
        result = None
        for attempt in range(MAX_ATTEMPTS):
            try:
                result = run_unit(arm, rep, port)
                port += 1
                tail = ""
                break
            except (Exception, SystemExit) as exc:   # SystemExit is NOT an Exception
                tail = f"{type(exc).__name__}: {exc}\n{traceback.format_exc()[-800:]}"
                log(f"[fail] {now()} {arm['name']} rep{rep} attempt {attempt}: {tail[:300]}")
                port += 1
                if attempt < MAX_ATTEMPTS - 1 and looks_transient(tail):
                    wait = BACKOFF[min(attempt, len(BACKOFF) - 1)]
                    log(f"[retry] transient — waiting {wait}s")
                    time.sleep(wait)
                    continue
                break

        if result is None:
            # Record the failure. A crashed unit that leaves nothing behind is a hole in the
            # sample indistinguishable from one that was never scheduled.
            d = unit_dir(arm["name"], rep)
            d.mkdir(parents=True, exist_ok=True)
            result = {"arm": arm["name"], "rep": rep, "env": arm["env"], "gate": arm["gate"],
                      "finished_at": datetime.now().isoformat(timespec="seconds"),
                      "score": None, "failed": True, "error": tail,
                      "audit_version": dispatch_audit.AUDIT_VERSION, "audit": {}}

        durations.append(time.time() - started)
        result_path(arm["name"], rep).parent.mkdir(parents=True, exist_ok=True)
        result_path(arm["name"], rep).write_text(json.dumps(result, indent=2))

        a = result.get("audit") or {}
        log(f"[done] {now()} {arm['name']} rep{rep}  score="
            f"{result['score'] if result.get('score') is not None else 'FAILED'}  "
            f"timed_out={result.get('timed_out')}  pool={result.get('actual_nodes')}  "
            f"fallbacks={a.get('detail_fallback_count')}  "
            f"kind_mismatch={a.get('kind_mismatch_pct')}%  "
            f"({round(durations[-1] / 60)} min)")
        summarise()


if __name__ == "__main__":
    sys.exit(main())
