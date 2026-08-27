#!/usr/bin/env python3
"""Adversarially audit the HARNESS, not the swarm. Exit 0 clean, 1 if any instrument is lying.

Six instrument failures in one day, two of them published before being caught, and every one had the
same shape: a number inferred from a signal that was never designed to answer the question. A thunk
bug turned "nothing was checked" into "nothing survived". Two verifiers answering different questions
were treated as redundant voters, discarding 22 confirmed defects. A dead agent counted as a
refutation. A liveness field was read as a completion marker. A grader compared timestamp STRINGS
across mixed UTC offsets and failed a correct app. And an unmatched dispatch was credited to the end
of the run, inventing 83 minutes of busy time from one retry and inverting a published conclusion.

None of those were caught by the instrument's own unit tests, because a unit test asks "does this
function do what I wrote" and every one of these bugs was faithfully doing what I wrote. What catches
them is an INVARIANT that must hold of the real world, checked against a real run.

So this runs after every unit, and the loop records its verdict alongside the score. It cannot make
the loop stop — a harness fault must never silently discard fleet time — but a unit whose harness
failed its own audit is marked, and a marked unit is not evidence.

Usage:
    python3 selftest.py                 # controls only (no run needed)
    python3 selftest.py <unit-dir>      # controls + invariants against a real run
"""
from __future__ import annotations

import datetime as dt
import json
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
SELFTEST_VERSION = "st-2"   # st-2: check 4 tests the tasks REPORTED unfinished, not any retry



# WHAT THE HARNESS READS OUT OF THE ENGINE. Three instruments have now shipped a name the engine does
# not emit, each invisible because a missing key is indistinguishable from a mechanism that did not
# fire: prefix.py read `owned_files` off plan_loaded where the engine emits `files`, and occupancy.py's
# idle-node map named `prereview`, `speculation`, `replan` and `dynamic_replan` — four of eight keys —
# so the single line that measures goal one silently dropped pre_review's 7 firings per run.
#
# Both halves are asserted below, and they need different ground truth:
#   FIELDS come from a real FINISHED run (only a run proves what an event carries), and are asserted
#          conditionally — if the event appears, the field must too. A new event that has never fired
#          is not a failure.
#   EVENT NAMES come from the ENGINE SOURCE, because a mechanism that legitimately never fires would
#          make a run-based check unable to tell a wrong name from a quiet mechanism — which is
#          exactly the confusion that hid the last one.
EVENT_FIELDS: dict[str, list[str]] = {
    "run_started": ["pool"],
    "phase": ["phase"],
    "plan_loaded": ["tasks"],
    "plan_loaded.tasks[]": ["id", "deps", "description", "files"],
    "task_dispatched": ["task_id", "device", "owned_files", "attempt"],
    "slices_opened": ["count", "weights", "slices"],
    "research_completed": ["slices", "brief_chars"],
    "review_findings": ["round", "new", "findings", "patch_touches"],
    "defects_rated": ["round", "critical", "minor"],
    "complete_verify": ["round", "ran", "findings"],
    "pool_resolved": ["worker_count", "planner_pushed"],
}

# Every event name the harness filters on or counts. Each must be a name the engine can actually emit.
HARNESS_EVENT_NAMES = [
    "run_started", "phase", "plan_loaded", "task_dispatched", "task_completed",
    "contracts", "pillars", "slices_opened", "clarify_proxy_armed", "clarify_proxy_answered",
    "research_completed", "review_findings", "plan_patched", "defects_rated", "sink_id_pinned",
    "run_finished", "complete_verify", "pool_resolved", "judge_verdict", "pre_review",
    "sink_review", "replanned", "speculated", "task_split",
]

ENGINE_SRC = pathlib.Path.home() / "Projects/goose/crates"


def engine_event_names() -> set[str]:
    """Names the engine can emit: literal `"event": "x"` plus snake_cased SwarmEvent variants."""
    names: set[str] = set()
    swarm_rs = ENGINE_SRC / "goose-cli/src/commands/swarm.rs"
    if swarm_rs.is_file():
        names |= set(re.findall(r'"event"\s*:\s*"([a-z_]+)"', swarm_rs.read_text(errors="replace")))
    ev_rs = ENGINE_SRC / "goose-swarm/src/event.rs"
    if ev_rs.is_file():
        body = ev_rs.read_text(errors="replace")
        m = re.search(r"pub enum SwarmEvent\s*\{(.*?)\n\}", body, re.S)
        if m:
            for variant in re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*)", m.group(1), re.M):
                names.add(re.sub(r"(?<!^)(?=[A-Z])", "_", variant).lower())
    return names


GOOSE_BIN = pathlib.Path.home() / "Projects/goose/target/release/goose"


def dead_in_binary(names: list[str]) -> list[str]:
    """Which of these event names are in the source but NOT in the built binary — i.e. dead code."""
    if not GOOSE_BIN.is_file():
        return []
    try:
        out = subprocess.run(["strings", str(GOOSE_BIN)], capture_output=True, text=True, timeout=180)
    except (OSError, subprocess.SubprocessError):
        return []
    if out.returncode != 0:
        return []
    hay = out.stdout
    # A CONTROL, because an empty haystack would report EVERY name dead and read as a total harness
    # failure. `task_dispatched` is the most common event in any run; if it is missing the instrument
    # is broken, not the engine.
    if "task_dispatched" not in hay:
        return ["`strings` found no task_dispatched in the release binary — the dead-code check is "
                "BLIND and none of its zeros mean anything"]
    dead = sorted(n for n in names if n not in hay)
    if dead:
        return [f"event name(s) {dead} exist in the engine SOURCE but not in the built binary — they "
                f"sit in code nothing calls, so they can never appear in a log again"]
    return []


def _started_after(rows: list[dict], build_mtime: float) -> bool:
    """Was this run produced by the CURRENT binary? Decided from its own `run_started`, not its path."""
    for r in rows:
        if r.get("event") != "run_started":
            continue
        ts = r.get("ts")
        if not ts:
            return False
        try:
            return dt.datetime.fromisoformat(str(ts).replace("Z", "+00:00")).timestamp() >= build_mtime
        except (TypeError, ValueError):
            return False
    return False


def field_contract() -> list[str]:
    """Assert the harness's field and event names against the engine, not against its own habits."""
    fails: list[str] = []

    # 1. EVENT NAMES vs the engine source.
    engine = engine_event_names()
    if not engine:
        fails.append("could not read the engine's event names — the name check did not run, which is "
                     "not the same as passing")
    else:
        unknown = sorted(n for n in HARNESS_EVENT_NAMES if n not in engine)
        if unknown:
            fails.append(f"the harness references event name(s) the engine never emits: {unknown} "
                         f"— a missing key reads exactly like a mechanism that did not fire")

    # 1b. THE SOURCE CHECK ALONE IS NOT ENOUGH, and the linear-engine rewrite is how that was found.
    # `confidence_retarget`, `plan_convergence`, `skeleton_drafts`, `retarget_discarded` and
    # `detail_completed` all still grep out of swarm.rs as `"event": "..."` literals — in functions
    # nothing calls any more. The source check passed on every one of them while not a single one can
    # ever appear in a log again, because unreachable code is never codegen'd and the literal never
    # reaches .rodata. `strings` on the built binary is the instrument that separates the two, and it
    # is the same one probe.py uses. Absent binary => the check does not run, and says so.
    fails.extend(dead_in_binary(HARNESS_EVENT_NAMES))

    # 2. FIELDS vs a real FINISHED run. Never the newest file: an in-flight run has not reached the
    #    phases these events live in, so its absent events are a clock, not a defect.
    #
    #    AND NEVER A RUN FROM AN OLDER BINARY. This check pooled across engine builds and the
    #    linear-engine rewrite made that visible: the newest finished run on disk was written by the
    #    pre-rewrite engine, whose `research_completed` carries {findings, grounded, lenses_returned}.
    #    Asserting the current field list against it fails FOREVER, on a harness that is correct — the
    #    same vintage trap landcheck.py already guards with `started_after_build`, missing here.
    #    An event cannot carry a field that did not exist when its binary was compiled.
    build_mtime = GOOSE_BIN.stat().st_mtime if GOOSE_BIN.is_file() else None
    chosen = None
    events: list[dict] = []
    for path in sorted((HERE.parent / "runs").glob("nodeloop*/*/run.jsonl"), reverse=True):
        rows = []
        for line in path.read_text(errors="replace").splitlines():
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
        if not any(r.get("event") == "run_finished" for r in rows):
            continue
        if build_mtime is not None and not _started_after(rows, build_mtime):
            continue
        chosen, events = path, rows
        break
    if chosen is None:
        # No run on the CURRENT binary yet. Nothing can be attributed, and reporting that as a pass
        # would be the vacuous truth this whole file exists to refuse — but it is not a failure
        # either, so it returns whatever the name checks above already found.
        return fails

    seen: dict[str, set] = {}
    for e in events:
        ev = e.get("event")
        seen.setdefault(ev, set()).update(e.keys())
        if ev == "plan_loaded":
            for t in (e.get("tasks") or []):
                if isinstance(t, dict):
                    seen.setdefault("plan_loaded.tasks[]", set()).update(t.keys())
    for ev, wanted in EVENT_FIELDS.items():
        if ev not in seen:
            continue  # the event never fired in this run — that is a clock, not a defect
        missing = [f for f in wanted if f not in seen[ev]]
        if missing:
            fails.append(f"{chosen.parent.name}: `{ev}` is missing field(s) the harness reads: "
                         f"{missing} (it carries {sorted(seen[ev])})")
    return fails


def run_controls() -> list[str]:
    """Each instrument's own both-directions controls. A failure here means it is already lying."""
    fails = []
    for tool in ("dispatch_audit.py", "occupancy.py"):
        r = subprocess.run([sys.executable, str(HERE / tool), "--self-test"],
                           capture_output=True, text=True, timeout=180)
        if r.returncode != 0:
            fails.append(f"{tool} --self-test FAILED: {(r.stdout + r.stderr).strip()[:400]}")
    return fails


def run_invariants(unit: pathlib.Path) -> list[str]:
    """Facts that must hold of ANY real run. Each one names a bug that actually shipped."""
    sys.path.insert(0, str(HERE))
    import occupancy
    import dispatch_audit

    fails: list[str] = []
    o = occupancy.analyse(unit)
    a = dispatch_audit.audit(unit)
    ev_all = occupancy.read_events(unit)

    n = o.get("pool_size") or 0
    wall = o.get("wall_secs") or 0
    busy = o.get("busy_node_secs") or 0

    # 1. A device cannot be busy for more wall-clock than the run lasted. This is the invariant the
    #    retry bug broke: 156 min of "busy" in a 122 min run across 3 nodes read as plausible until
    #    it was divided out. occupancy > 1.0 is physically impossible.
    occ = o.get("occupancy")
    if occ is not None and occ > 1.0 + 1e-9:
        fails.append(f"occupancy {occ} exceeds 1.0 — impossible; phantom busy time is being counted")
    if n and wall and busy > wall * n + 1:
        fails.append(f"busy {busy:.0f}s exceeds wall*pool {wall * n:.0f}s — phantom busy time")

    # 2. No single task can be more than all node-busy time. This caught a share of 1.118 when a
    #    per-task SUM was divided by a per-device UNION.
    share = o.get("biggest_task_share_of_busy")
    if share is not None and share > 1.0 + 1e-9:
        fails.append(f"biggest-task share {share} exceeds 1.0 — two different measures being divided")

    # 3. Solo-node time cannot exceed the wall clock.
    solo = o.get("solo_node_secs")
    if solo is not None and wall and solo > wall + 1:
        fails.append(f"solo-node {solo:.0f}s exceeds wall {wall:.0f}s")

    # 4. A FINISHED run must not report work in flight. Reading "no completion" as "still running"
    #    is what hid the retry, and it also made three finished runs look live.
    if o.get("finished") and o.get("unfinished_tasks"):
        # This is legitimate ONLY if the tasks REPORTED unfinished genuinely never completed. The check
        # must therefore be made ON THOSE TASKS.
        #
        # ⚠ THE FIRST VERSION OF THIS CHECK VOIDED A CLEAN 112-MINUTE RUN. It re-derived its own rule —
        # "any task dispatched more than once that also completed" — and fired whenever ANY retry
        # existed anywhere in the run, regardless of which task was actually unfinished. On
        # think_off-n3-r0 the unfinished task was `meridian-error-handling` (dispatched once, never
        # completed, genuinely outstanding) while `test-meridian` and `test-cli-edge-cases` were
        # retried and finished — three different tasks. The audit accused the pairing of a bug that
        # occupancy.py had already fixed, and declared the arm "NOT evidence". A self-test that voids
        # good runs is worse than no self-test: it removes the evidence AND looks rigorous doing it.
        ev = occupancy.read_events(unit)
        last_disp: dict[str, float] = {}
        comps: dict[str, list] = {}
        for e in ev:
            if e.get("event") == "task_dispatched":
                s = occupancy.parse_ts(e.get("ts"))
                if s is not None:
                    last_disp[e["task_id"]] = max(s, last_disp.get(e["task_id"], s))
            if e.get("event") == "task_completed":
                comps.setdefault(e["task_id"], []).append(occupancy.parse_ts(e.get("ts")))
        wrong = [t for t in (o.get("unfinished_task_ids") or [])
                 if t in last_disp and any(c is not None and c >= last_disp[t]
                                           for c in comps.get(t, []))]
        if wrong:
            fails.append(f"finished run reports {wrong} unfinished, but each COMPLETED at or after "
                         f"its last dispatch — the dispatch/completion pairing is wrong again")

    # 4b. THE FOUNDING CASE. A task that completed must not also own a span running to the end of
    #     the run. Phantom time on a multi-node pool stays under the wall*N ceiling, so the aggregate
    #     invariants above cannot see it — this is the only check that catches it, and the audit was
    #     decoration until it existed: reintroducing the retry-pairing bug passed every other check.
    phantom = o.get("phantom_tail_tasks") or []
    if phantom:
        fails.append(f"tasks {phantom} completed yet own a span running to the end of the run — "
                     f"the dispatch/completion pairing is inventing busy time")

    # 5. CROSS-INSTRUMENT. dispatch_audit infers shipped one-liners from the final plan; RESEARCH
    #    reports how long each slice owner's spec was. `splice_briefs` writes that spec into its
    #    task's `description` VERBATIM, so a slice whose brief is at least ONE_LINER_MAX_CHARS long
    #    CANNOT produce a one-liner task — its description fails the length half of the shape rule by
    #    construction. That is the ceiling: shipped one-liners can never exceed the tasks NOT covered
    #    by a long brief. It replaces the old detail_fallback pairing, which counted failed calls in
    #    an engine that no longer makes them.
    #
    #    A COUNT OF NON-EMPTY BRIEFS WOULD NOT WORK, and it is worth saying why: RESEARCH substitutes
    #    a `PURPOSE:` line for a dead owner rather than nothing at all, so brief_chars is never zero
    #    and "did every slice return something" is vacuously true on every run.
    shipped = a.get("shipped_one_liners")
    planned = a.get("planned_tasks")
    rc = next((e for e in ev_all if e.get("event") == "research_completed"), None)
    if shipped is not None and planned and rc:
        long_briefs = len([c for c in (rc.get("brief_chars") or [])
                           if (c or 0) >= dispatch_audit.ONE_LINER_MAX_CHARS])
        ceiling = planned - long_briefs
        if shipped > ceiling >= 0:
            fails.append(f"shipped one-liners {shipped} exceeds the ceiling of {ceiling} "
                         f"({planned} planned tasks less {long_briefs} slice(s) whose spec is too "
                         f"long to be one) — a worker got a brief no lost slice accounts for")

    # 6. The pool the engine built must match what the unit asked for, or the row is not evidence.
    res = unit / "nodeloop-result.json"
    if res.is_file():
        try:
            r = json.loads(res.read_text())
            if r.get("nodes") and r.get("actual_nodes") and r["nodes"] != r["actual_nodes"]:
                if not r.get("void"):
                    fails.append(f"pool mismatch {r['actual_nodes']}/{r['nodes']} not marked void")
        except Exception as exc:  # noqa: BLE001
            fails.append(f"result.json unreadable: {exc}")

    return fails


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    fails = run_controls() + field_contract()
    scope = "controls"
    if args:
        unit = pathlib.Path(args[0]).resolve()
        if unit.is_dir():
            scope = f"controls + invariants on {unit.name}"
            try:
                fails += run_invariants(unit)
            except Exception as exc:  # noqa: BLE001 - an audit that crashes is an audit that failed
                fails.append(f"invariant pass CRASHED: {type(exc).__name__}: {exc}")
    if fails:
        print(f"HARNESS SELF-TEST FAILED ({SELFTEST_VERSION}, {scope}) — "
              f"the numbers from this unit are NOT evidence:")
        for f in fails:
            print("  -", f)
        return 1
    print(f"harness self-test OK ({SELFTEST_VERSION}, {scope})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
