#!/usr/bin/env python3
"""The unattended loop: does the swarm get BETTER with more nodes, and are its instructions specific?

Node count became measurable on 2026-08-01 when the fleet was given three distinct LM Studio
identifiers. Before that all three hosts served one identifier, LM Studio exposed exactly one
addressable worker, and every run labelled 1node/2node/3node built the same 1-device pool — so the
project's "more nodes make it worse" table compared a configuration with itself. Proven at the time
by a concurrency probe: three simultaneous calls were all served by one host while the other two
never left idle. Re-proven after the re-identification: three concurrent calls, one per identifier,
put ALL THREE instances into `generating` at once.

So this loop measures two things that were previously unmeasurable and are the whole of goal one:

  NODES        does build quality and fleet occupancy actually improve at 2 and 3 nodes
  DISPATCH     is each node given a SPECIFIC instruction, or a generic one

The second is not a side question. Three runs of an identical 1-node config scored 44.2 / 86.7 /
90.0% — a 46-point spread — and the spread tracked exactly how many workers got the architect's
one-liner instead of a detailed spec (2, 1 and 0 respectively). Any node-count effect must clear
that spread to mean anything, which is why every cell is replicated and why the mechanism counts
from dispatch_audit.py matter more than the score.

Operating rules below each cost a real overnight run at some point:
  - a result not on disk did not happen: every unit persists its result the moment it finishes
  - resumable: a completed unit is skipped, so a killed loop resumes where it stopped
  - one bad unit never kills the sweep, and SystemExit is NOT an Exception
  - a fleet blip is retried with backoff, never recorded as a score of zero
  - a flat timeout measures the timeout: timed_out is recorded and checked before interpreting
  - children die by process GROUP, or an orphan contends for the fleet unnoticed
  - a unit whose ACTUAL pool differs from the one it asked for is VOID, never averaged in
  - it ends only on the STOP sentinel, never on a counter
"""
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import threading
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
TIMEOUT = 16200          # 4.5h. A cap that truncates the work measures the cap, not the entrant.
MIN_REPS = 3             # n=1 is uninterpretable against a measured 46-point spread.
TRANSIENT = ("500", "502", "503", "529", "overloaded", "rate limit", "throttl",
             "connection reset", "stream decode", "temporarily", "unreachable")
MAX_ATTEMPTS = 3
BACKOFF = (60, 240)

WATCHDOG_POLL_SECS = 60
HEARTBEAT_STALE_SECS = 600   # the engine writes it every 5s; 10 min dead is wedged, not busy
MIN_FREE_GB = 15

# Each arm varies exactly ONE thing against baseline, and carries a prediction written down BEFORE
# the run, where it can fail.
ARMS = [
    {
        "name": "baseline",
        "env": {},
        "gate": "establishes the replicate spread, the detail-fallback rate, and the node curve. "
                "Re-measured rather than assumed: a stale baseline turns fleet drift into a false win.",
    },
    {
        "name": "kind_prompt",
        "env": {"GOOSE_SWARM_KIND_PROMPT": "1"},
        "gate": "72-80% of dispatches receive rules written for another job, and 3-5 per run own a "
                "test_*.py while being told never to read test files. Gating rules by task kind "
                "should drive kind_mismatch_pct toward zero. A prior adversarial pass refuted the "
                "naive version and put score recovery in single digits, so the MECHANISM count is "
                "the readout, not the build score.",
    },
    {
        "name": "scoped_contracts",
        "env": {"GOOSE_SWARM_SCOPED_CONTRACTS": "1"},
        "gate": "every worker receives the FULL frozen-contract bundle rather than its DAG "
                "neighborhood, so irrelevant interface text grows with the plan's width — the one "
                "instruction defect that gets WORSE as nodes are added. scope_contract_bundle "
                "(coherence.rs:303) is written and unused.",
    },
    {
        "name": "doc_prefetch",
        "env": {"GOOSE_SWARM_DOC_PREFETCH": "1"},
        "gate": "doc_facts is the only un-paraphrased scout->worker channel. Tier C is graded on "
                "vendor-doc compliance and collapsed to 14.3% in the run whose meridian module got "
                "a 95-char brief.",
    },
]

# Goal one is the node curve, so the node levels come first and every pass covers all three. An
# early stop then still leaves a balanced design rather than three reps of one node count.
NODE_LEVELS = (3, 1, 2)


def cells() -> list[dict]:
    """(nodes, arm) pairs in priority order: the node curve first, then the dispatch-quality arms.

    The quality arms run at 3 nodes because that is the configuration whose behaviour we want to
    ship, and because scoped_contracts is predicted to matter MORE the wider the fleet.
    """
    base = ARMS[0]
    out = [{"nodes": n, "arm": base} for n in NODE_LEVELS]
    out += [{"nodes": max(NODE_LEVELS), "arm": a} for a in arms_now()[1:]]
    return out


def now() -> str:
    return datetime.now().strftime("%H:%M:%S")


def log(msg: str) -> None:
    print(msg, flush=True)


def unit_name(arm: str, nodes: int, rep: int) -> str:
    return f"{arm}-n{nodes}-r{rep}"


def unit_dir(arm: str, nodes: int, rep: int) -> Path:
    return OUT / unit_name(arm, nodes, rep)


def result_path(arm: str, nodes: int, rep: int) -> Path:
    return unit_dir(arm, nodes, rep) / "nodeloop-result.json"


def complete(arm: str, nodes: int, rep: int) -> bool:
    p = result_path(arm, nodes, rep)
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


def engine_pids() -> list[int]:
    try:
        r = subprocess.run(["pgrep", "-f", "goose swarm run"],
                           capture_output=True, text=True, timeout=15)
        return [int(p) for p in r.stdout.split() if p.strip().isdigit()]
    except Exception:
        return []


class Watchdog(threading.Thread):
    """Cut a DOOMED unit loose instead of waiting out its 4.5h cap.

    A wedged run does not fail — it sits there holding fleet capacity until the timeout, and a cap
    that truncates work measures the cap rather than the swarm. The LOOP is never stopped by this;
    only the unit is. Every trip condition is a fact about the process or the filesystem, never an
    inference from how long something is taking, because a slow unit is not a doomed one.
    """

    def __init__(self, label: str) -> None:
        super().__init__(daemon=True)
        self._stop = threading.Event()
        self.label = label
        self.reason: str | None = None

    def doomed(self) -> str | None:
        pids = engine_pids()
        if len(pids) > 1:
            return (f"{len(pids)} engines running at once {pids} — an orphan is contending for the "
                    f"fleet and will skew this unit and every later one")
        if pids:
            beats = sorted(OUT.glob("*/heartbeat"), key=lambda p: p.stat().st_mtime, reverse=True)
            if beats:
                age = time.time() - beats[0].stat().st_mtime
                if age > HEARTBEAT_STALE_SECS:
                    return (f"heartbeat {int(age)}s stale under a live engine — the run is wedged "
                            f"and will hold the fleet until its cap")
        free_gb = shutil.disk_usage(os.path.expanduser("~")).free / 1e9
        if free_gb < MIN_FREE_GB:
            return f"only {free_gb:.0f} GB free — a run that fills the disk corrupts its own tree"
        return None

    def abort(self, reason: str) -> None:
        log(f"[abort] {now()} {self.label}: {reason}")
        for pid in engine_pids():
            try:
                os.killpg(os.getpgid(pid), signal.SIGKILL)
                log(f"[abort] killed engine pgroup for pid {pid}")
            except (ProcessLookupError, PermissionError) as exc:
                log(f"[abort] could not kill {pid}: {exc}")

    def run(self) -> None:
        while not self._stop.wait(WATCHDOG_POLL_SECS):
            try:
                reason = self.doomed()
            except Exception:
                continue
            if reason:
                self.reason = reason
                self.abort(reason)
                return

    def done(self) -> None:
        self._stop.set()


def kill_strays() -> None:
    for pid in engine_pids():
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
            log(f"[warn] killed stray engine pgroup for pid {pid}")
        except (ProcessLookupError, PermissionError):
            pass


def run_unit(arm: dict, nodes: int, rep: int, port: int) -> dict:
    """One episode: build, grade the artifact, then grade the INSTRUCTIONS it was given."""
    import run_build  # imported late so a syntax error there cannot stop the loop from starting

    entrant = f"swarm-{nodes}node"   # run_build reads the N and sets GOOSE_SWARM_MAX_NODES
    prev = dict(os.environ)
    dog = Watchdog(unit_name(arm["name"], nodes, rep))
    dog.start()
    try:
        for k, v in arm["env"].items():
            os.environ[k] = v
        verdict = run_build.run(entrant, rep, OUT, TIMEOUT, port)
    finally:
        dog.done()
        os.environ.clear()
        os.environ.update(prev)

    # run_build names its outputs after the ENTRANT, so two arms at the same node count and rep
    # would overwrite each other's tree AND vendor trace. Re-home both under the unit.
    src = OUT / f"{entrant}-r{rep}"
    dst = unit_dir(arm["name"], nodes, rep)
    if src.exists():
        if dst.exists():
            shutil.rmtree(dst)
        src.rename(dst)
    trace_src = OUT / f"trace-{entrant}-r{rep}.jsonl"
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

    actual = verdict.get("actual_nodes")
    # The label is an intention; run_started.pool is the fact. A mismatch has silently voided a
    # whole campaign before, so it voids the unit here rather than being averaged in.
    void = actual is not None and actual != nodes

    return {
        "arm": arm["name"],
        "nodes": nodes,
        "rep": rep,
        "env": arm["env"],
        "gate": arm["gate"],
        "finished_at": datetime.now().isoformat(timespec="seconds"),
        "score": verdict.get("score"),
        "tiers": verdict.get("tiers"),
        "aborted": dog.reason is not None,
        "abort_reason": dog.reason,
        "timed_out": (verdict.get("agent") or {}).get("timed_out"),
        "wall_secs": (verdict.get("agent") or {}).get("secs"),
        "actual_pool": verdict.get("actual_pool"),
        "actual_nodes": actual,
        "void": void,
        "void_reason": (f"asked for {nodes} nodes, engine built {actual}" if void else None),
        "scorer_version": verdict.get("scorer_version"),
        "audit_version": audit.get("audit_version") or dispatch_audit.AUDIT_VERSION,
        "audit": audit,
    }


def arms_now() -> list[dict]:
    """ARMS plus anything appended to QUEUE, so arms can be added without restarting the loop.

    A running interpreter never sees a source edit, so a new arm added to ARMS in this file would
    not reach a loop that is already up. QUEUE is re-read every pass.
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


def backlog(target_reps: int) -> list[tuple[dict, int, int]]:
    units = []
    for rep in range(target_reps):
        for c in cells():
            if not complete(c["arm"]["name"], c["nodes"], rep):
                units.append((c["arm"], c["nodes"], rep))
    return units


def read_results() -> list[dict]:
    rs = []
    for f in sorted(OUT.glob("*/nodeloop-result.json")):
        try:
            rs.append(json.loads(f.read_text()))
        except Exception:
            continue
    return rs


def summarise() -> None:
    """Mechanism first, score second — a score alone cannot clear a 46-point spread."""
    rs = read_results()
    if not rs:
        return
    groups: dict[tuple[str, int], list[dict]] = {}
    for r in rs:
        groups.setdefault((r.get("arm"), r.get("nodes")), []).append(r)
    log("")
    log(f"{'arm':<18}{'nodes':>5}{'n':>3}  {'score mean':>10} {'spread':>8}  "
        f"{'fallbacks':>9} {'kind-mm%':>9} {'wall min':>9}  void")
    for (arm, nodes), g in sorted(groups.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)):
        ok = [r for r in g if not r.get("timed_out") and not r.get("aborted")
              and not r.get("void") and r.get("score") is not None]
        sc = [r["score"] for r in ok]
        fb = [r["audit"].get("detail_fallback_count") for r in ok
              if isinstance(r.get("audit"), dict)
              and r["audit"].get("detail_fallback_count") is not None]
        km = [r["audit"].get("kind_mismatch_pct") for r in ok
              if isinstance(r.get("audit"), dict)
              and r["audit"].get("kind_mismatch_pct") is not None]
        wl = [r["wall_secs"] for r in ok if r.get("wall_secs")]
        mean = f"{sum(sc) / len(sc):.1%}" if sc else "-"
        spread = f"{(max(sc) - min(sc)) * 100:.0f}pts" if len(sc) > 1 else "-"
        log(f"{arm:<18}{nodes if nodes is not None else '?':>5}{len(g):>3}  "
            f"{mean:>10} {spread:>8}  "
            f"{(sum(fb) / len(fb) if fb else 0):>9.1f} "
            f"{(sum(km) / len(km) if km else 0):>9.1f} "
            f"{(sum(wl) / len(wl) / 60 if wl else 0):>9.0f}  "
            f"{sum(1 for r in g if r.get('void'))}")
    log("")


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    log("=" * 78)
    log(f"nodeloop starting {datetime.now().isoformat(timespec='seconds')}  "
        f"pid={os.getpid()}  audit={dispatch_audit.AUDIT_VERSION}")
    log(f"node levels {NODE_LEVELS}, arms {[a['name'] for a in arms_now()]}, min reps {MIN_REPS}")
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
            # Never end on a counter. More replicates is the most useful thing this loop can do
            # next, because every verdict here is limited by n, not by ideas.
            target += 1
            log(f"[grow] {now()} backlog drained — raising replicate target to n={target}")
            summarise()
            continue

        arm, nodes, rep = todo[0]
        label = unit_name(arm["name"], nodes, rep)
        eta = ""
        if durations:
            avg = sum(durations) / len(durations)
            eta = (f"  (~{datetime.fromtimestamp(time.time() + avg).strftime('%H:%M')}, "
                   f"{len(todo)} left ~"
                   f"{datetime.fromtimestamp(time.time() + avg * len(todo)).strftime('%a %H:%M')})")
        nxt = unit_name(todo[1][0]["name"], todo[1][1], todo[1][2]) if len(todo) > 1 else "raise n"
        log("")
        log(f">>> {now()}  NOW: {label}   [{len(todo)} in backlog, n target {target}]{eta}")
        log(f"    NEXT: {nxt}")
        log(f"    gate: {arm['gate']}")

        kill_strays()
        started = time.time()
        result = None
        tail = ""
        for attempt in range(MAX_ATTEMPTS):
            try:
                result = run_unit(arm, nodes, rep, port)
                port += 1
                break
            except (Exception, SystemExit) as exc:   # SystemExit is NOT an Exception
                tail = f"{type(exc).__name__}: {exc}\n{traceback.format_exc()[-800:]}"
                log(f"[fail] {now()} {label} attempt {attempt}: {tail[:300]}")
                port += 1
                if attempt < MAX_ATTEMPTS - 1 and looks_transient(tail):
                    wait = BACKOFF[min(attempt, len(BACKOFF) - 1)]
                    log(f"[retry] transient — waiting {wait}s")
                    time.sleep(wait)
                    continue
                break

        if result is None:
            # Record the failure. A crashed unit that leaves nothing behind is a hole in the sample
            # indistinguishable from one that was never scheduled.
            d = unit_dir(arm["name"], nodes, rep)
            d.mkdir(parents=True, exist_ok=True)
            result = {"arm": arm["name"], "nodes": nodes, "rep": rep, "env": arm["env"],
                      "gate": arm["gate"],
                      "finished_at": datetime.now().isoformat(timespec="seconds"),
                      "score": None, "failed": True, "error": tail,
                      "audit_version": dispatch_audit.AUDIT_VERSION, "audit": {}}

        durations.append(time.time() - started)
        result_path(arm["name"], nodes, rep).parent.mkdir(parents=True, exist_ok=True)
        result_path(arm["name"], nodes, rep).write_text(json.dumps(result, indent=2))

        a = result.get("audit") or {}
        if result.get("aborted"):
            log(f"[abort] {now()} {label} CUT LOOSE — {result.get('abort_reason')}")
        log(f"[done] {now()} {label}  score="
            f"{result['score'] if result.get('score') is not None else 'FAILED'}  "
            f"pool={result.get('actual_nodes')}/{nodes}  void={result.get('void')}  "
            f"aborted={result.get('aborted')}  timed_out={result.get('timed_out')}  "
            f"fallbacks={a.get('detail_fallback_count')}  "
            f"kind_mismatch={a.get('kind_mismatch_pct')}%  "
            f"({round(durations[-1] / 60)} min)")

        # FEASIBILITY GATE. If the very first unit cannot get the pool it asked for, every later
        # unit is measuring the same thing under different labels — which is exactly how this
        # project's node-scaling table came to compare a configuration with itself. Stop and say so
        # rather than spend a night producing an answer to a question nobody asked.
        if len(read_results()) == 1 and result.get("void"):
            log(f"[STOP] {now()} FEASIBILITY GATE FAILED: {result.get('void_reason')}. "
                f"The engine is not building the pool the sweep asks for, so node-count cells are "
                f"not distinguishable. Stopping instead of producing an uninterpretable table.")
            STOP.write_text(f"feasibility gate: {result.get('void_reason')}\n")
            summarise()
            return 2

        summarise()


if __name__ == "__main__":
    sys.exit(main())
