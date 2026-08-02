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

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
# The duration-weighted node ceiling is occupancy.py's, NOT re-derived here. A local re-derivation
# weighting by task_completed.elapsed_ms gave a different (wrong) answer, because a task superseded by
# a split never completes and vanished from the sum.
import occupancy  # noqa: E402

REVIEW_VERSION = "rev-1"
HERE = pathlib.Path(__file__).resolve().parent
RUNS = HERE.parent / "runs" / "nodeloop"
GOAL = HERE / "GOAL.md"
PREDICTIONS = HERE / "PREDICTIONS"
ROOT = HERE.parents[2]


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
    L += settleable_on_this_build(ev)
    return L


def settleable_on_this_build(ev: list[dict]) -> list[str]:
    """Which registered predictions can this run's BINARY possibly settle?

    A prediction is only testable by a binary that contains the fix it is about. The 1-node unit
    running while this was written carries none of `sink_capped`, `rules_delivered`, "WHAT THE
    SUPERVISOR ALREADY FOUND", "was ALREADY BOUND before", `straggler_aborted` or "SAME KIND" — all
    committed after that binary was built and all still held for the boundary. Its sink emitting no
    `sink_capped` would therefore be an UNCONTROLLED ZERO, not a falsification of F115, and the same
    standing rule I apply to arms applies here.

    The live trap is F116: it predicts the next sink takes materially fewer than 25 calls, turn
    count is readable from any session trace, and reading THIS sink's count as the test would be the
    natural mistake. The mechanism expected to do the shortening is not in this binary.

    Attribution rule: the run used the release binary only if that binary was built BEFORE the run
    started. A binary newer than the run has been rebuilt since, so it says nothing about what the
    run executed and this reports UNKNOWN rather than guessing.
    """
    preds = PREDICTIONS
    if not preds.is_file():
        return []
    rs = next((e for e in ev if e.get("event") == "run_started"), None)
    started = _ts((rs or {}).get("ts"))
    started = started.timestamp() if started else None
    binary = ROOT / "target" / "release" / "goose"
    if started is None or not binary.is_file():
        return ["  (cannot attribute a binary to this run — prediction gate skipped)"]
    if binary.stat().st_mtime > started:
        return [f"  (the release binary is NEWER than this run — it was rebuilt since, so which "
                f"fixes this run carried cannot be read off it)"]
    try:
        import subprocess
        blob = subprocess.run(["strings", str(binary)], capture_output=True, text=True,
                              timeout=180).stdout
    except Exception as exc:
        return [f"  (prediction gate unavailable: {exc})"]

    can, cannot = [], []
    for line in preds.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("|", 2)
        if len(parts) != 3:
            continue
        pid, markers, _claim = parts
        need = [m for m in markers.split(",") if m.strip() and m.strip() != "-"]
        missing = [m for m in need if m not in blob]
        (cannot if missing else can).append((pid, missing))
    out = []
    if cannot:
        out.append(f"  ⚠ NOT SETTLEABLE on this build ({len(cannot)}): "
                   f"{', '.join(p for p, _ in cannot)} — their fix is not in the running binary, so "
                   f"a zero here is UNCONTROLLED and must leave the prediction OPEN, never failed")
    if can:
        out.append(f"  settleable on this build: {', '.join(p for p, _ in can)}")
    return out


def level4_overarching(ev: list[dict], plan: dict, run_dir: pathlib.Path) -> list[str]:
    """The only question that outranks everything: can this run use more than one node?"""
    pool = next((e for e in ev if e.get("event") == "pool_resolved"), None)
    n = (pool or {}).get("worker_count")
    L = ["OVERARCHING: make the swarm worth it — beat ONE node on time and on what ships."]
    if not plan:
        L.append("  no plan yet, so the plan's node ceiling is unknown — NOT zero, unknown")
        return L
    w, n = plan["width"], n or 0
    L.append(f"  pool {n} nodes; the PLAN's widest parallel level is {w} tasks")

    # WIDTH IS ONLY AN UPPER BOUND, and reading it as the answer was wrong. This function once
    # reported "the plan can saturate 3 nodes (width 8 >= 3)" for a run whose real ceiling was 1.92
    # nodes, because ONE task was 49.5% of all node-busy time and the critical path dominated however
    # many tasks were nominally parallel. Eight tasks that can start together are not eight tasks'
    # worth of work.
    #
    # THE CEILING IS COMPUTED BY occupancy.py AND IS NOT RE-DERIVED HERE. The first attempt at this
    # weighted by `task_completed.elapsed_ms` and got a DIFFERENT answer — `integrate-verify` at 31%
    # instead of `api-web` at 49.5% — because a task SUPERSEDED BY A SPLIT never completes, carries no
    # elapsed_ms, and vanished from the sum. occupancy.py pairs dispatch->completion spans and unions
    # them per task, which is exactly the case a naive completion-sum gets wrong. Re-implementing an
    # instrument is a standing prohibition in this project and this is why.
    try:
        occ = occupancy.analyse(run_dir)
    except Exception as exc:
        L.append(f"  occupancy unavailable ({exc}) — width {w} is an UPPER BOUND only")
        return L
    mun = occ.get("max_useful_nodes")
    big, share = occ.get("biggest_task"), occ.get("biggest_task_share_of_busy")
    if mun is None:
        L.append(f"  too little has run to weight by duration; width {w} is an UPPER BOUND only — "
                 f"eight tasks that can start together are not eight tasks' worth of work")
        if n and w < n:
            L.append(f"  ** structurally the PLAN is already the ceiling: width {w} < pool {n}.")
        return L
    L.append(f"  duration-weighted MAX USEFUL NODES = {mun} (pool {n}); "
             f"biggest task `{big}` = {100*(share or 0):.0f}% of all node-busy")
    if n and mun < n:
        L.append(f"  ** THE PLAN IS THE CEILING, NOT THE FLEET: {mun} < {n}. More nodes CANNOT help "
                 f"this run. The work belongs in the architect prompt, not the scheduler.")
        if share and share > 0.30:
            L.append(f"  ** and the reason is one task: `{big}` at {100*share:.0f}% of the work. "
                     f"No fan-out helps that — SPLIT IT IN THE PLAN.")
    elif n:
        L.append(f"  the plan can genuinely use {n} nodes — occupancy below 1.0 here is a SCHEDULER "
                 f"question, not a planning one")
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
    # MIXED KINDS — DEMOTED from BAD to a note by F124, and the demotion is the point.
    #
    # This rule went through two wrong versions before the measurement. v1 flagged any multi-file
    # producing task, which contradicted the architect prompt's own instruction ("a subtask may and
    # SHOULD own SEVERAL small files, ONE concern each"). v2 narrowed it to tasks mixing KINDS —
    # api.py + web/index.html, __main__.py + README.md — on the story that one brief covering two
    # concerns must produce a worse dispatch.
    #
    # MEASURED across all 17 archived plans (262 producing tasks) before acting on it:
    #     mixed-kind tasks retry 18.2% (n=22)   pure-kind tasks retry 15.2% (n=217)   delta +3.0pp
    # Three findings-worth of consequence, and it fires on 88% of plans (15/17). A verdict that says
    # "NO — fix the planner" on 88% of runs over a 3-point non-effect carries no information; it is
    # the P4 pattern (a measure that cannot separate two opposite situations) in my own supervisor.
    #
    # It stays visible because the SHAPE is real and perfectly reproducible — only three collisions
    # ever occur (code+docs 10, asset+code 9, asset+code+docs 3), always on `cli`/`main`/`entry` or
    # `api`/`api-web`. If a future change gives it a measurable cost, the counter is already here.
    # What it must NOT do is drive the verdict. See F124 for what does.
    def kind(path: str) -> str:
        p = path.lower()
        for suf, k in ((".md", "docs"), (".rst", "docs"), (".txt", "docs"),
                       (".html", "asset"), (".css", "asset"), (".js", "asset"),
                       (".json", "config"), (".yaml", "config"), (".yml", "config"),
                       (".toml", "config"), (".cfg", "config"), (".ini", "config")):
            if p.endswith(suf):
                return k
        return "code"

    # THE PREDICTOR THAT ACTUALLY SEPARATES (F124), measured over 239 dispatched tasks with a plan
    # entry. It is an INTERACTION, which is why neither factor alone was ever visible:
    #     hard AND test-authoring   60.0% retry (n= 30)   <-- the whole retry burden lives here
    #     hard, not a test task     12.1% retry (n= 91)
    #     test task, not hard       12.5% retry (n= 16)
    #     neither                    5.9% retry (n=102)
    # Test tasks retried worse than their run's other tasks in 5 of the 6 runs that had any. A retry
    # is ~83s x the task's turns of pure waste, so this is the single largest planner-visible cost to
    # the overarching goal, and `kind_prompt` is the lever aimed straight at it (G3).
    hard_tests = [t["id"] for t in tasks
                  if t.get("difficulty") == "hard" and str(t.get("id", "")).startswith("test")]
    if hard_tests:
        bad.append(f"{len(hard_tests)} HARD TEST task(s) {hard_tests} — measured 60% retry (n=30) "
                   f"against 12% for hard non-test work; this is the run's predictable waste region")
    for t, f in sorted(files.items(), key=lambda kv: -len(kv[1])):
        if len(f) < 2:
            continue
        kinds = {kind(x) for x in f}
        if len(kinds) > 1:
            warn.append(f"{t} mixes kinds {sorted(kinds)} across {len(f)} files ({', '.join(f)}) — the "
                        f"shape is real but MEASURED at +3.0pp retry (F124), so it is noted, not charged")
        elif desc.get(t, 0) > 3000:
            warn.append(f"{t} owns {len(f)} same-kind files but carries a {desc.get(t,0)}-char brief — "
                        f"large even for a legitimate multi-file task")
    # The brief-length ladder is real but ONLY inside test tasks: 0-1200 33.3%, 1200-1800 34.8%,
    # 1800+ 64.3%. Across producing tasks the same buckets go 0.0% / 22.2% / 10.7% — no ladder at all.
    # So length is not a general defect and must not be flagged as one.
    fat_tests = [t["id"] for t in tasks
                 if str(t.get("id", "")).startswith("test") and len(t.get("description") or "") >= 1800]
    if fat_tests:
        warn.append(f"test task(s) {fat_tests} carry a >=1800-char brief — that bucket retries 64% "
                    f"(n=14) against 33% for shorter test briefs")
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
    # A task that burned 3 attempts and then FINISHED is history, not drift. Reporting it as live
    # drift is the same defect Q1 carried (F124): a predicate that cannot separate two opposite
    # situations — "burning attempts right now" and "burned them and converged" — and it made the
    # verdict say INTERVENE about `api` six minutes after `api` completed. The cost is not zero
    # either way (F124: a retry is ~83s x turns of waste) so the finished case is still REPORTED,
    # just not as a reason to intervene.
    retries = collections.Counter(e["task_id"] for e in ev if e.get("event") == "task_dispatched")
    hot = {t: k for t, k in retries.items() if k >= 3}
    stuck = {t: k for t, k in hot.items() if t not in done}
    settled = {t: k for t, k in hot.items() if t in done}
    if stuck:
        drift.append(f"RE-DISPATCHED >=3x AND STILL IN FLIGHT: {stuck} — not converging, and that is "
                     f"where a run dies")
    if settled:
        L.append(f"burned >=3 attempts but COMPLETED: {settled} — cost paid (~83s x turns each), "
                 f"not a live problem")
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
    out += ["\n4. ABOVE — the overarching goal"] + [f"   {x}" for x in level4_overarching(ev, plan, run_dir)]

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
    # With no completions the DURATION-weighted ceiling cannot exist, so the honest output is the
    # STRUCTURAL bound plus an explicit statement that width is only an upper bound. Asserting the
    # duration wording here would demand a number nothing could have computed — the vacuous-truth
    # trap, pointed at my own control.
    if "UPPER BOUND only" not in txt2:
        fails.append("with no completions the review must say width is only an upper bound")
    if "PLAN is already the ceiling" not in txt2:
        fails.append("a plan structurally narrower than the pool must still be named as the ceiling")

    # And the duration-weighted branch must fire once completions exist, naming occupancy's figure
    # rather than a locally-computed one.
    ev2b = ev2 + [{"event": "task_dispatched", "task_id": "a", "device": "d0",
                   "ts": "2026-08-02T10:05:01+00:00"},
                  {"event": "task_completed", "task_id": "a", "device": "d0", "elapsed_ms": 60000,
                   "ts": "2026-08-02T10:06:01+00:00"},
                  {"event": "task_dispatched", "task_id": "b", "device": "d1",
                   "ts": "2026-08-02T10:06:01+00:00"},
                  {"event": "task_completed", "task_id": "b", "device": "d1", "elapsed_ms": 60000,
                   "ts": "2026-08-02T10:07:01+00:00"}]
    txt2c = render(write(ev2b))
    if "MAX USEFUL NODES" not in txt2c:
        fails.append("with completions the review must report the duration-weighted ceiling")

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
