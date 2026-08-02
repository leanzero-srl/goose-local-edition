#!/usr/bin/env python3
"""THE TICK REVIEW — four levels, in order, every tick. Exit 0.

Mihai, 2026-08-02: *"before finalizing a tick you start a review of what was created last in logs
versus the plan and versus your goal and then finally versus the overarching goal ... you own the
supervision which I am not convinced you do."*

He was right. Ticks had become event-driven — react to whatever broke (disk, an intruder, a crash) —
and the plan itself was being QUERIED (count the /v1s, count the tasks) rather than STUDIED. The first
time it was actually read end to end it showed that the planner had put `vendorsync/api.py` and
`vendorsync/web/index.html` in ONE task, which is the chokepoint the judge later had to split. Nothing
in the tick routine would ever have surfaced that, because nothing compared the PLAN to the GOAL.

So this instrument walks the four levels deliberately, and each one is allowed to say NOTHING IS KNOWN
YET rather than manufacture a reading:

  1. LOGS   what the run has actually emitted, and what it is doing right now
  2. PLAN   what the run committed to — width, depth, chokepoints, instruction size per task
  3. GOAL   the CURRENT goal in GOAL.md and which of its registered predictions this run can settle
  4. ABOVE  the overarching goal: can this plan use more than one node at all

Usage:
    python3 review.py                 # newest run under runs/nodeloop
    python3 review.py <run-dir>
    python3 review.py --self-test
"""
from __future__ import annotations

import collections
import datetime
import json
import pathlib
import re
import sys

REVIEW_VERSION = "rev-1"
HERE = pathlib.Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"
GOAL = HERE / "GOAL.md"


def newest_run() -> pathlib.Path | None:
    cands = sorted(RUNS.glob("*/*.jsonl"), key=lambda p: -p.stat().st_mtime)
    return cands[0].parent if cands else None


def load(run_dir: pathlib.Path) -> list[dict]:
    out = []
    for f in sorted(run_dir.glob("*.jsonl")):
        for line in f.read_text(errors="replace").splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return out


def _ts(v):
    try:
        return datetime.datetime.fromisoformat(str(v))
    except Exception:
        return None


def level1_logs(ev: list[dict]) -> list[str]:
    c = collections.Counter(e.get("event") for e in ev)
    t0, tn = _ts(ev[0].get("ts")), _ts(ev[-1].get("ts"))
    mins = (tn - t0).total_seconds() / 60 if t0 and tn else 0
    disp = {e["task_id"] for e in ev if e.get("event") == "task_dispatched"}
    done = {e["task_id"] for e in ev if e.get("event") == "task_completed"}
    L = [f"elapsed {mins:.1f} min, {len(ev)} events, last = {ev[-1].get('event')}",
         f"dispatched {len(disp)} / completed {len(done)} / in flight {len(disp - done)}"]
    if disp - done:
        L.append(f"in flight: {sorted(disp - done)}")
    fired = [k for k in ("task_split", "speculated", "replanned", "pre_review",
                         "complete_fix_dispatched", "spec_repair_wave", "sink_review") if c[k]]
    L.append(f"idle-node mechanisms that FIRED: {fired or 'none'}")
    skips = collections.Counter(e.get("reason") for e in ev if e.get("event") == "judge_skipped")
    if c["judge_verdict"] or skips:
        hints = sum(1 for e in ev if e.get("event") == "judge_verdict" and (e.get("hint") or "").strip())
        L.append(f"judge: {c['judge_verdict']} ran, {sum(skips.values())} skipped {dict(skips)}, "
                 f"{hints} real interventions")
    return L


def level2_plan(ev: list[dict]) -> tuple[list[str], dict]:
    """STUDY the plan, not just count it. Width, depth, chokepoints, instruction size."""
    pl = next((e for e in ev if e.get("event") == "plan_loaded"), None)
    if not pl:
        return ["NO PLAN YET — the run has not committed to a decomposition, so there is nothing to "
                "review at this level. This is not a finding."], {}
    tasks = pl.get("tasks") or []
    deps = {t["id"]: list(t.get("depends_on") or t.get("deps") or []) for t in tasks}
    files = {t["id"]: list(t.get("files") or t.get("owned_files") or []) for t in tasks}
    desc = {t["id"]: len(t.get("description") or "") for t in tasks}

    def depth(tid, seen=None):
        seen = seen or set()
        if tid in seen:
            return 0
        seen = seen | {tid}
        return 1 + max((depth(d, seen) for d in deps.get(tid, []) if d in deps), default=-1)

    levels = collections.defaultdict(list)
    for t in deps:
        levels[depth(t)].append(t)
    width = max((len(v) for v in levels.values()), default=0)
    roots = [t for t, d in deps.items() if not d]

    L = [f"{len(tasks)} tasks, confidence {pl.get('plan_confidence')}, ask_floor {pl.get('ask_floor')}",
         f"critical-path DEPTH {max(levels) + 1 if levels else 0}, max ANTICHAIN WIDTH {width} "
         f"({len(roots)} roots)"]
    for d in sorted(levels):
        L.append(f"  level {d}: {len(levels[d]):>2} — {', '.join(sorted(levels[d])[:6])}"
                 + (" ..." if len(levels[d]) > 6 else ""))

    # CHOKEPOINTS — the shape that costs the most and is invisible to a task count.
    multi = {t: f for t, f in files.items() if len(f) > 1}
    if multi:
        L.append("MULTI-FILE producing tasks (a split-candidate the PLANNER created):")
        for t, f in sorted(multi.items(), key=lambda kv: -len(kv[1])):
            L.append(f"  {t}: {len(f)} files — {', '.join(f)}   [desc {desc.get(t,0)} chars]")
    funnels = [t for t, d in deps.items() if len(d) >= 3]
    if funnels:
        L.append(f"FUNNELS (>=3 deps, they serialise the tail): {funnels}")
    if desc:
        v = sorted(desc.values())
        L.append(f"instruction size per task: min {v[0]} / median {v[len(v)//2]} / max {v[-1]} chars")
        thin = [t for t, n in desc.items() if n < 300]
        if thin:
            L.append(f"  THIN instructions (<300 chars, the degraded-brief signature): {thin}")
    return L, {"width": width, "depth": max(levels) + 1 if levels else 0,
               "tasks": len(tasks), "roots": len(roots)}


def level3_goal(ev: list[dict]) -> list[str]:
    """What does the CURRENT goal ask, and can this run settle any of it?"""
    if not GOAL.is_file():
        return ["GOAL.md missing — the tick has no goal to serve, which is itself the finding"]
    txt = GOAL.read_text()
    cur = re.search(r"## CURRENT GOAL[^\n]*\n", txt)
    L = [cur.group(0).strip() if cur else "no CURRENT GOAL heading found"]
    # Registered predictions are table rows; an EMPTY outcome cell is still open.
    open_p, done_p = [], []
    for row in re.findall(r"^\|\s*(P\d+)\s*\|([^|]*)\|([^|]*)\|([^|]*)\|", txt, re.M):
        pid, claim, _where, outcome = (x.strip() for x in row)
        (done_p if outcome else open_p).append((pid, claim[:58]))
    L.append(f"predictions: {len(done_p)} settled, {len(open_p)} OPEN")
    for pid, claim in open_p:
        L.append(f"  OPEN {pid}: {claim}")
    return L


def level4_overarching(ev: list[dict], plan: dict) -> list[str]:
    """The only question that outranks everything: can this run use more than one node?"""
    pool = next((e for e in ev if e.get("event") == "pool_resolved"), None)
    n = (pool or {}).get("worker_count")
    L = ["OVERARCHING: make the swarm worth it — beat ONE node on time and on what ships."]
    if not plan:
        L.append("  no plan yet, so the plan's node ceiling is unknown — NOT zero, unknown")
        return L
    w, n = plan["width"], n or 0
    L.append(f"  pool {n} nodes; the PLAN's widest parallel level is {w}")
    if n and w < n:
        L.append(f"  ** the PLAN is the ceiling, not the fleet: {w} < {n}. More nodes cannot help "
                 f"this run, and the work belongs in the architect prompt, not the scheduler.")
    elif n:
        L.append(f"  the plan can saturate {n} nodes ({w} >= {n}) — so occupancy below 1.0 is a "
                 f"SCHEDULER or a duration-skew question, not a planning one")
    return L


def q1_does_the_plan_make_sense(ev, plan) -> tuple[str, list[str]]:
    """Mihai's FIRST question, answered as a VERDICT with reasons rather than a metric dump.

    His framing, verbatim: "DOES THE PLAN MAKE SENSE? THEN: IS THE PLAN BEING FOLLOWED?" The order is
    load-bearing — a faithfully-executed bad plan is still a bad run, so checking execution first
    would flatter it.
    """
    pl = next((e for e in ev if e.get("event") == "plan_loaded"), None)
    if not pl:
        return "UNKNOWN", ["no plan committed yet"]
    tasks = pl.get("tasks") or []
    files = {t["id"]: list(t.get("files") or t.get("owned_files") or []) for t in tasks}
    desc = {t["id"]: len(t.get("description") or "") for t in tasks}
    deps = {t["id"]: list(t.get("depends_on") or t.get("deps") or []) for t in tasks}
    pool = next((e for e in ev if e.get("event") == "pool_resolved"), None)
    n = (pool or {}).get("worker_count") or 0

    bad, warn = [], []
    # A PRODUCING task owning several files is a chokepoint the PLANNER made. MEASURED: `api-web`
    # owned api.py AND web/index.html, stalled 11 minutes with the judge saying "ok" six times, and
    # had to be split into a backend and a frontend child. The split worked — it was repairing a
    # planning error that should never have reached the fleet.
    for t, f in sorted(((t, f) for t, f in files.items() if len(f) > 1), key=lambda kv: -len(kv[1])):
        line = (f"{t} owns {len(f)} files ({', '.join(f)}) with a {desc.get(t, 0)}-char brief — "
                f"one task, several concerns; this is the shape that gets split mid-run")
        (bad if len(f) > 2 or desc.get(t, 0) > 3000 else warn).append(line)
    thin = [t for t, k in desc.items() if k < 300]
    if thin:
        bad.append(f"THIN instructions (<300 chars) on {thin} — the degraded-brief signature")
    if plan and n and plan.get("width", 0) < n:
        bad.append(f"plan width {plan['width']} < pool {n}: more nodes CANNOT help this run, and the "
                   f"fix belongs in the architect prompt rather than the scheduler")
    if not [t for t in files if files[t]]:
        bad.append("no task owns a file — nothing will be produced")
    fun = [t for t, d in deps.items() if len(d) >= 3]
    if fun:
        warn.append(f"{len(fun)} funnel(s) with >=3 deps ({fun[:4]}) — they serialise the tail")

    verdict = "NO — fix the planner" if bad else ("YES, with reservations" if warn else "YES")
    return verdict, [f"BAD  {b}" for b in bad] + [f"warn {w}" for w in warn] or ["no structural objection"]


def q2_is_the_plan_being_followed(ev, plan) -> tuple[str, list[str]]:
    """Mihai's SECOND question: DRIFT. What was planned versus what is actually happening."""
    pl = next((e for e in ev if e.get("event") == "plan_loaded"), None)
    if not pl:
        return "UNKNOWN", ["no plan to follow yet"]
    planned = {t["id"] for t in (pl.get("tasks") or [])}
    disp = {e["task_id"] for e in ev if e.get("event") == "task_dispatched"}
    done = {e["task_id"] for e in ev if e.get("event") == "task_completed"}
    kids = {c for e in ev if e.get("event") == "task_split" for c in (e.get("children") or [])}
    added = {c for e in ev if e.get("event") == "replanned" for c in (e.get("added") or [])}
    superseded = {e.get("task_id") for e in ev if e.get("event") == "task_split"}

    L, drift = [], []
    unplanned = disp - (planned | kids | added)
    if unplanned:
        drift.append(f"DISPATCHED BUT NEVER PLANNED: {sorted(unplanned)} — not from the plan, a split "
                     f"or a replan; something is inventing work")
    retries = collections.Counter(e["task_id"] for e in ev if e.get("event") == "task_dispatched")
    hot = {t: k for t, k in retries.items() if k >= 3}
    if hot:
        drift.append(f"RE-DISPATCHED >=3x: {hot} — the plan IS being followed but a task is not "
                     f"converging, and that is where a run dies")
    pending = sorted(planned - disp - superseded)
    if pending:
        L.append(f"planned, not yet dispatched: {pending}")
    if superseded:
        L.append(f"superseded by a split (correctly never completes under its own id): {sorted(superseded)}")
    L.append(f"plan {len(planned)} | +split {len(kids)} | +replan {len(added)} | "
             f"dispatched {len(disp)} | done {len(done)}")
    return ("NO — drifting" if drift else "YES"), [f"DRIFT {d}" for d in drift] + L


def render(run_dir: pathlib.Path) -> str:
    ev = load(run_dir)
    if not ev:
        return f"{run_dir.name}: no events yet — nothing to review, which is not a finding"
    out = [f"=== TICK REVIEW ({REVIEW_VERSION})  {run_dir.name} ==="]
    for title, lines in (("1. LOGS — what actually happened", level1_logs(ev)),):
        out += [f"\n{title}"] + [f"   {x}" for x in lines]
    plan_lines, plan = level2_plan(ev)
    out += ["\n2. PLAN — what the run committed to"] + [f"   {x}" for x in plan_lines]
    out += ["\n3. GOAL — the current mini-goal and its open predictions"] + \
           [f"   {x}" for x in level3_goal(ev)]
    out += ["\n4. ABOVE — the overarching goal"] + [f"   {x}" for x in level4_overarching(ev, plan)]

    # THE TWO QUESTIONS, in Mihai's order, answered with a verdict. Everything above is EVIDENCE for
    # these; these are the tick's actual output. Order matters: a faithfully-executed bad plan is
    # still a bad run, so checking execution first would flatter it.
    v1, r1 = q1_does_the_plan_make_sense(ev, plan)
    v2, r2 = q2_is_the_plan_being_followed(ev, plan)
    out += [f"\n>>> Q1  DOES THE PLAN MAKE SENSE?   {v1}"] + [f"      {x}" for x in r1]
    out += [f"\n>>> Q2  IS THE PLAN BEING FOLLOWED? {v2}"] + [f"      {x}" for x in r2]

    # ACT, never merely observe. A tick ending without a decision is the idling Mihai called out.
    if v1.startswith("NO"):
        act = ("INTERVENE — the PLAN is wrong. Executing it faithfully cannot rescue the run, so the "
               "fix belongs in the architect prompt. Let this unit finish ONLY if it still settles an "
               "open prediction, then change the planner.")
    elif v2.startswith("NO"):
        act = "INTERVENE — plan sound, run DRIFTING. Chase the drift named above."
    elif v1 == "UNKNOWN":
        act = "CONTINUE — no plan yet, nothing decidable. Do OTHER work; do not wait on it."
    else:
        act = "CONTINUE — plan sound, execution faithful. Work the next OPEN prediction."
    out += [f"\n>>> VERDICT: {act}"]
    return "\n".join(out)


def self_test() -> int:
    """A review that cannot say 'I do not know yet' will invent a reading, so that is what is tested."""
    import tempfile
    fails = []
    d = pathlib.Path(tempfile.mkdtemp())

    def write(events):
        p = d / "run.jsonl"
        p.write_text("\n".join(json.dumps(e) for e in events))
        return d

    # A run with NO plan must say so plainly at every level, and must NOT report width 0 as a ceiling.
    ev = [{"event": "run_started", "ts": "2026-08-02T10:00:00+00:00"},
          {"event": "pool_resolved", "worker_count": 3, "ts": "2026-08-02T10:00:01+00:00"}]
    txt = render(write(ev))
    if "NO PLAN YET" not in txt:
        fails.append("a planless run must say so, not render an empty plan table")
    if "unknown — NOT zero" not in txt:
        fails.append("a planless run must not report its node ceiling as zero")

    # A plan NARROWER than the pool must name the PLAN as the ceiling — the goal-one verdict.
    ev2 = ev + [{"event": "plan_loaded", "ts": "2026-08-02T10:05:00+00:00", "plan_confidence": 90,
                 "tasks": [{"id": "a", "depends_on": [], "files": ["a.py"], "description": "x" * 900},
                           {"id": "b", "depends_on": ["a"], "files": ["b.py"], "description": "y" * 900}]}]
    txt2 = render(write(ev2))
    if "the PLAN is the ceiling" not in txt2:
        fails.append("a plan narrower than the pool must be named as the ceiling")

    # A multi-file producing task is the chokepoint shape that a task COUNT cannot see.
    ev3 = ev + [{"event": "plan_loaded", "ts": "2026-08-02T10:05:00+00:00", "plan_confidence": 90,
                 "tasks": [{"id": "fat", "depends_on": [], "files": ["api.py", "web/index.html"],
                            "description": "z" * 3800},
                           {"id": "m2", "depends_on": [], "files": ["m2.py"], "description": "z" * 900},
                           {"id": "m3", "depends_on": [], "files": ["m3.py"], "description": "z" * 900}]}]
    txt3 = render(write(ev3))
    if "MULTI-FILE producing tasks" not in txt3 or "fat" not in txt3:
        fails.append("a multi-file producing task must be surfaced as a planner-made chokepoint")

    for f in fails:
        print(f"FAIL {f}")
    if fails:
        return 1
    print(f"self-test OK ({REVIEW_VERSION}) — planless runs say so, a narrow plan is named as the "
          f"ceiling, and a planner-made chokepoint is surfaced")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    args = [a for a in argv if not a.startswith("--")]
    run = pathlib.Path(args[0]) if args else newest_run()
    if not run or not run.is_dir():
        print("no run to review")
        return 0
    print(render(run))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
