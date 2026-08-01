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
    # scoped_contracts was queued here and REMOVED before it ran. Measured on three real plans:
    # ZERO inter-module dependency edges among code modules, because the architect is explicitly told
    # "Default to a FLAT FAN: make every module a root with no deps" (swarm.rs:11493). A worker's DAG
    # neighborhood is therefore just itself, so scoping the frozen-contract bundle to it would delete
    # every SIBLING interface and leave only the module's own stub — the one interface it does not
    # need, since it is the thing writing it. Under a flat fan the FULL bundle is the correct bundle,
    # so the lever is inert at best and destructive at worst, and its precondition is the very thing
    # the planner prompt works to prevent. Measurement time is the scarce resource; a lever predicted
    # broken on evidence does not earn three replicates.
    {
        "name": "detail_budget",
        "env": {"GOOSE_SWARM_DETAIL_BUDGET_SECS": "300"},
        "gate": "the 75s detail budget is a bare literal pinned at the OBSERVED MAXIMUM of the call "
                "it bounds, so normal variance lands on the far side of it: the SAME meridian brief "
                "was detailed in 44.5s on one run and blew through 75s on another, and the run that "
                "lost it shipped a 95-char spec for the module tier C grades (14.3% vs 85.7%). The "
                "sibling contract fanout already abandoned a small fixed budget for "
                "worker_timeout_secs.max(120) after a mass stub failure. PREDICTION: "
                "detail_fallback_count goes to ~0 and pre-execute wall grows only slightly, because "
                "the budget is a ceiling on the slow tail, not the mean (~50s). If fallbacks do NOT "
                "drop, the cause is not the ceiling and this whole line of reasoning is wrong.",
    },
    {
        "name": "complete_parallel",
        "env": {"GOOSE_SWARM_COMPLETE_PARALLEL": "1"},
        "gate": "MEASURED live on baseline-n3-r0: the COMPLETE/repair phase ran 20 of 88 minutes — "
                "22% of the run — on ONE node, with two of three idle for all of it "
                "(smoke_fix_target = devices.first(), swarm.rs:21260). Two independent calculations "
                "agree: occupancy.py put solo-node time at 1174.6s and the gap from the last "
                "task_completed was 19.6 min. This is the phase the project's own ledger fingers as "
                "'REPAIR is what fails', and the lever to fan it across the fleet already exists and "
                "defaults OFF. PREDICTION: wall time falls by roughly the repair tail's idle share "
                "and the build score is UNCHANGED within the replicate spread — this buys fleet "
                "utilisation, not correctness. If the score MOVES, the parallel fix path is not "
                "equivalent to the serial one and that is a defect worth more than the speedup.",
    },
    {
        "name": "e2e_oracle",
        "env": {"GOOSE_SWARM_E2E_ORACLE": "1"},
        "gate": "fan_e2e does not currently partition: e2e_shard_spec tells each shard to number the "
                "advertised commands 'in the order the spec gives them' and never gives it the spec, "
                "so each derives the list from the README the build itself wrote. MEASURED on one "
                "run: three shards derived lists of length 1, 1 and 3, and the one that enumerated "
                "an empty slice reported clean. This arm hands every shard the SAME engine-extracted "
                "table from spec_frozen. PREDICTION: tier C and the e2e-derived checks rise, because "
                "the shards start checking the operator's endpoints rather than the build's own "
                "documentation — and crucially the shards' reports should stop citing the README. If "
                "tier C does NOT move but the reports stop citing the README, the oracle landed and "
                "the app was already right; if the reports still cite the README, the injection is "
                "not reaching them and the arm has failed regardless of the score.",
    },
    # doc_prefetch was here and is PULLED, not deprioritised: it forwards only findings where
    # `grounded == is_mcp && ok`, and research_tools reports available: [] on every run this machine
    # has ever produced, because the research extensions are context7 and web-search and neither key
    # exists. With no grounded finding the block is empty and the worker prompt is byte-identical to
    # baseline, so the arm cannot fire. An arm that cannot fire is not evidence, and running it would
    # spend hours of fleet time to produce an INERT result. doc_fetch replaces it with a fetch that
    # needs no extension and no key.
    {
        "name": "doc_fetch",
        "env": {"GOOSE_SWARM_DOC_FETCH": "1"},
        "gate": "THE measured coin flip. Three baseline units, identical config: the one whose "
                "plan_loaded carried the vendor's /v1 prefix scored 88.7% and the two that did not "
                "scored 50.0% and 42.7% with every vendor call returning 404. The prefix appears six "
                "times in the document the spec points at and zero times in the spec, and no scout "
                "has ever had a tool to open it. This arm has the ENGINE fetch that document and "
                "splice it verbatim into the planner's channel and every worker's. "
                "PREDICTION: doc_fetched{ok:true} fires, /v1 appears in plan_loaded, and crunch.py's "
                "fetch_all_payments returns 247 rather than raising 404. The mechanism claim is "
                "settled by the first two regardless of the score — if the paths are still wrong "
                "with a 200-status fetch on record, the splice is not reaching the decomposition and "
                "the arm has failed no matter what the number does.",
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
    # A unit is only "done" if it was measured by the CURRENT instrument AND on the CURRENT engine.
    # Checking the instrument alone left a hole with teeth: after a rebuild, a stale unit still
    # counts as complete, gets skipped forever, and quietly contributes a row measured on a
    # different binary. That is the exact shape of the failure that once published a table showing
    # the cheaper model winning — every part of the loop did its job and the conclusion was wrong.
    return (r.get("audit_version") == dispatch_audit.AUDIT_VERSION
            and r.get("engine_build") == engine_build())


def looks_transient(tail: str) -> bool:
    low = (tail or "").lower()
    return any(t in low for t in TRANSIENT)


def engine_build() -> str:
    """Identify the ENGINE BINARY a unit ran on.

    Results already carry scorer_version and audit_version, but nothing identified the engine — and
    that gap cost a campaign: 34 hours of backlog were queued against a binary built before the
    levers the arms set even existed, so `detail_budget` would have set an env var the binary
    ignores and recorded a confident "no effect". mtime+size is enough to tell two builds apart and
    costs nothing; a content hash of a 235 MB binary would not be worth its own runtime.
    """
    try:
        import run_build
        st = run_build.GOOSE.stat()
        return f"{int(st.st_mtime)}-{st.st_size}"
    except Exception as exc:  # noqa: BLE001 - an unknown build must be visible, never silently absent
        return f"unknown:{type(exc).__name__}"


def engine_pids() -> list[int]:
    try:
        r = subprocess.run(["pgrep", "-f", "goose swarm run"],
                           capture_output=True, text=True, timeout=15)
        return [int(p) for p in r.stdout.split() if p.strip().isdigit()]
    except Exception:
        return []


def median_unit_secs() -> float | None:
    """Median wall of units that actually finished, so "too long" is measured, not guessed."""
    walls = []
    for f in OUT.glob("*/nodeloop-result.json"):
        try:
            r = json.loads(f.read_text())
        except Exception:
            continue
        if r.get("wall_secs") and not r.get("timed_out") and not r.get("aborted"):
            walls.append(r["wall_secs"])
    if not walls:
        return None
    walls.sort()
    return walls[len(walls) // 2]


def abandon_decision(unit: Path, arm: dict, nodes: int, elapsed: float) -> tuple[float, list[str]]:
    """How confident are we that this unit can NO LONGER inform goal one? 0..1 with reasons.

    The watchdog above kills what is BROKEN. This decides what is POINTLESS, which is a different and
    harder question, and the one that actually costs weeks: a unit that got the wrong pool runs its
    full ~2 hours and is only marked VOID afterwards. Nothing about that row was ever going to be
    evidence, and the fleet time was spent to learn something already known at minute one.

    Deliberately asymmetric. Killing a HEALTHY unit costs a full re-run and poisons the replicate
    count, so every predicate here must be something already DECIDED — a fact about this run that no
    amount of further work can change — not a prediction that it will go badly. A slow unit is not a
    doomed one, and a unit producing a BAD score is doing its job.
    """
    reasons: list[str] = []
    conf = 0.0
    log_path = unit / "run.jsonl"
    events = []
    if log_path.is_file():
        for line in log_path.read_text(errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except Exception:
                continue

    # 1. VOID BY CONSTRUCTION. run_started carries the pool the engine actually built. If it is not
    #    the pool this cell asked for, the row is excluded from every mean no matter how it ends —
    #    so finishing it buys nothing. This is certain, not probable, and it is knowable at minute 1.
    started = next((e for e in events if e.get("event") == "run_started"), None)
    if started is not None:
        actual = len(started.get("pool") or [])
        if actual and actual != nodes:
            conf = 1.0
            reasons.append(f"pool is {actual}, cell asked for {nodes} — VOID by construction, the row "
                           f"can never be evidence")

    # 2. THE ARM CANNOT FIRE. An arm sets env vars; if the running binary has no such lever, the arm
    #    is byte-identical to baseline and would be recorded as "no effect" — a fabricated null. This
    #    already happened: 34 hours were queued against a binary with no GOOSE_SWARM_DETAIL_BUDGET_SECS.
    for var in arm.get("env", {}):
        try:
            import run_build
            out = subprocess.run(["strings", str(run_build.GOOSE)],
                                 capture_output=True, text=True, timeout=120)
            if var not in out.stdout:
                conf = 1.0
                reasons.append(f"{var} is ABSENT from the engine binary — this arm cannot fire and "
                               f"would record a fabricated 'no effect'")
        except Exception:
            pass
        break   # one probe is enough; strings over 235MB is not free

    # 3. PLANNING STUCK. No task has been dispatched well past the point where every observed run had
    #    started dispatching. Not proof of doom, so it is weighted below the kill line on its own.
    if events and not any(e.get("event") == "task_dispatched" for e in events):
        if elapsed > 3600:
            conf = max(conf, 0.85)
            reasons.append(f"{elapsed / 60:.0f} min elapsed with ZERO dispatches — planning has not "
                           f"produced a single task (observed pre-dispatch is ~25-31 min)")
        elif elapsed > 2400:
            conf = max(conf, 0.5)
            reasons.append(f"{elapsed / 60:.0f} min with no dispatch yet (observed ~25-31 min)")

    # 4. FAR BEYOND THE MEASURED NORM. Uses the median of units that actually finished, so it adapts
    #    rather than encoding a guess. Alone it is under the line — slow is not doomed — but it
    #    compounds with anything else.
    med = median_unit_secs()
    if med and elapsed > 2.5 * med:
        conf = max(conf, 0.6)
        reasons.append(f"{elapsed / 60:.0f} min is {elapsed / med:.1f}x the median finished unit "
                       f"({med / 60:.0f} min)")

    return min(conf, 1.0), reasons


class Watchdog(threading.Thread):
    """Cut a DOOMED unit loose instead of waiting out its 4.5h cap.

    A wedged run does not fail — it sits there holding fleet capacity until the timeout, and a cap
    that truncates work measures the cap rather than the swarm. The LOOP is never stopped by this;
    only the unit is. Every trip condition is a fact about the process or the filesystem, never an
    inference from how long something is taking, because a slow unit is not a doomed one.
    """

    ABANDON_AT = 0.8   # kill only on something already DECIDED; a wrong kill costs a full re-run

    def __init__(self, label: str, unit: Path, arm: dict, nodes: int) -> None:
        super().__init__(daemon=True)
        self._stop = threading.Event()
        self.label = label
        self.unit = unit
        self.arm = arm
        self.nodes = nodes
        self.started_at = time.time()
        self.reason: str | None = None
        self.abandoned = False
        self.abandon_confidence = 0.0
        self.abandon_reasons: list[str] = []

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
                reason = None
            if reason:
                self.reason = reason
                self.abort(reason)
                return
            # BROKEN is not the only reason to stop. Judge whether this unit can still inform goal
            # one at all, and cut it loose the moment the answer is settled — waiting out a ~2h run
            # whose row is already void is the single largest avoidable waste in this campaign.
            try:
                conf, why = abandon_decision(self.unit, self.arm, self.nodes,
                                             time.time() - self.started_at)
            except Exception:
                continue
            if conf >= self.ABANDON_AT:
                self.abandoned = True
                self.abandon_confidence = conf
                self.abandon_reasons = why
                self.abort(f"ABANDONED at confidence {conf:.2f} — " + "; ".join(why))
                return
            if why:
                log(f"[watch] {self.label}: confidence {conf:.2f} this unit is pointless "
                    f"(kill at {self.ABANDON_AT}) — {'; '.join(why)}")

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
    dog = Watchdog(unit_name(arm["name"], nodes, rep), OUT / f"{entrant}-r{rep}", arm, nodes)
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

    # ADVERSARIAL AUDIT OF THE HARNESS, every unit, not just of the swarm. Six instrument failures in
    # one day and two published before being caught; a unit whose own instruments cannot pass their
    # controls and invariants is not evidence, and must be MARKED rather than quietly averaged in.
    # It never stops the loop — a harness fault must not silently discard fleet time.
    harness = {"ok": None, "detail": ""}
    try:
        r = subprocess.run([sys.executable, str(HERE / "selftest.py"), str(dst)],
                           capture_output=True, text=True, timeout=600)
        harness = {"ok": r.returncode == 0, "detail": (r.stdout + r.stderr).strip()[:2000]}
        if not harness["ok"]:
            log(f"[HARNESS] {unit_name(arm['name'], nodes, rep)} FAILED its own audit — this unit is "
                f"NOT evidence:\n{harness['detail']}")
    except Exception as exc:  # noqa: BLE001 - an audit that cannot run is an audit that failed
        harness = {"ok": False, "detail": f"{type(exc).__name__}: {exc}"}
        log(f"[HARNESS] audit could not run: {harness['detail']}")

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
        "abandoned": dog.abandoned,
        "abandon_confidence": dog.abandon_confidence,
        "abandon_reasons": dog.abandon_reasons,
        "timed_out": (verdict.get("agent") or {}).get("timed_out"),
        "wall_secs": (verdict.get("agent") or {}).get("secs"),
        "actual_pool": verdict.get("actual_pool"),
        "actual_nodes": actual,
        "void": void,
        "void_reason": (f"asked for {nodes} nodes, engine built {actual}" if void else None),
        "scorer_version": verdict.get("scorer_version"),
        "engine_build": engine_build(),
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
              and not r.get("abandoned") and not r.get("void")
              and r.get("harness_ok") is not False and r.get("score") is not None]
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
                      "audit_version": dispatch_audit.AUDIT_VERSION,
                      "engine_build": engine_build(), "audit": {}}

        durations.append(time.time() - started)
        result_path(arm["name"], nodes, rep).parent.mkdir(parents=True, exist_ok=True)
        result_path(arm["name"], nodes, rep).write_text(json.dumps(result, indent=2))

        a = result.get("audit") or {}
        if result.get("abandoned"):
            log(f"[abandon] {now()} {label} killed at confidence "
                f"{result.get('abandon_confidence'):.2f}: {'; '.join(result.get('abandon_reasons') or [])}")
        elif result.get("aborted"):
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
