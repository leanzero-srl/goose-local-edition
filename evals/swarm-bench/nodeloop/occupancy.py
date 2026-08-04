#!/usr/bin/env python3
"""How much of the fleet did a run actually use, in TIME? Exit 0. Reads only a run's own event log.

Dispatch COUNTS are the wrong unit and have misled this project before: three tasks of 40 minutes
and three of 90 seconds is "balanced 3/3" and a nearly-idle fleet. What matters is node-seconds.

    occupancy = (busy node-seconds) / (wall-clock x pool size)

1.0 means every node worked for the whole run; 1/N means one node did everything while N-1 idled.
That single number is the direct test of goal one — the swarm must get better, not merely bigger,
as nodes are added — and it is derived from deterministic timestamps the engine emits, never from a
model's self-report.

Also reported, because occupancy alone hides the two known shapes of waste:
  - the biggest single task's share of node-busy time (integrate-verify has been measured at 36-47%,
    which is a serialization no amount of extra nodes can help)
  - the tail: wall-clock during which only one node was working
  - the idle-node mechanisms (judge / pre-review / speculation / replan), which are the swarm's
    "smarter with more nodes" half — they only ever run on a node that would otherwise be idle, so
    a fleet that never idles never gets them, and a 1-node fleet cannot get them at all

Usage:
    python3 occupancy.py <run-dir-or-run.jsonl> [...]
    python3 occupancy.py --json <run-dir>
    python3 occupancy.py --self-test
"""
from __future__ import annotations

import json
import pathlib
import sys
from datetime import datetime

# occ-3: adds the CONCURRENT-TASKS histogram, computed in the same exact interval sweep that
# already produced solo time — off the CORRECTED spans, because a hand-rolled version read 9
# concurrent tasks on a 6-slot fleet (F266). Above pool_size x 2 is impossible and is flagged.
# occ-2: the prefix is no longer one lump — it reports its phases, draft rounds, plan_confidence
# and the redraft cost, because that is where the arms differ in KIND and not merely in speed.
OCCUPANCY_VERSION = "occ-3"

# THE ENGINE'S OWN EVENT NAMES. Four of the eight keys here used to be names the engine has never
# emitted — `prereview` for `pre_review`, `speculation`/`speculative_promoted` for `speculated`,
# `replan`/`dynamic_replan` for `replanned`. So the line rendered as "the 'smarter with more nodes'
# half", the one that measures goal one, reported {'judge': 161} on a run that fired `pre_review`
# SEVEN times, every run, and nobody could see it because a missing key looks exactly like a
# mechanism that did not fire.
#
# Same disease as prefix.py reading `owned_files` where plan_loaded emits `files`. That one was
# caught by an adversarial round; this one was found by applying its lesson — when a defect is fixed,
# go and look for the same shape everywhere else. `real_shape_control()` below now asserts against an
# actual run so a renamed event fails the instrument instead of quietly zeroing it.
HERE = pathlib.Path(__file__).resolve().parent

IDLE_NODE_EVENTS = {
    # Each of these only ever runs on a node that would otherwise sit idle, so their counts are the
    # measurable form of "the swarm got smarter because it had spare capacity".
    "judge_verdict": "judge",
    "pre_review": "pre_review",
    "speculated": "speculation",
    "replanned": "replan",
    "task_split": "split",
    "sink_review": "sink_review",
}


def parse_ts(v):
    if not v:
        return None
    try:
        return datetime.fromisoformat(str(v).replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def read_events(path) -> list[dict]:
    p = pathlib.Path(path)
    if p.is_dir():
        cands = sorted(p.glob("run.jsonl")) or sorted(p.glob(".swarm/run-swarm-*.jsonl"))
        if not cands:
            raise FileNotFoundError(f"no run log under {p}")
        p = cands[-1]
    out = []
    for line in p.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def analyse(path) -> dict:
    events = read_events(path)
    if not events:
        return {"occupancy_version": OCCUPANCY_VERSION, "path": str(path),
                "pool_size": None, "occupancy": None,
                "note": "empty log — nothing measured, which is not the same as zero occupancy"}

    pool, t0, t_end = [], None, None
    disp: dict[str, list] = {}
    done: dict[str, list] = {}
    split_at: dict[str, float] = {}
    idle_jobs: dict[str, int] = {}

    for e in events:
        ts = parse_ts(e.get("ts"))
        if ts is not None:
            t_end = ts if t_end is None else max(t_end, ts)
        ev = e.get("event")
        if ev == "run_started":
            pool = e.get("pool") or []
            t0 = ts if ts is not None else t0
        elif ev == "task_dispatched":
            disp.setdefault(e.get("task_id"), []).append((ts, e.get("device")))
        elif ev == "task_split":
            # A SPLIT PARENT IS ABORTED, NOT STILL RUNNING. It never completes under its own id — the
            # scheduler replaces it with its children — so the "no completion => credit to t_end" rule
            # below invents everything between the split and the end of the run.
            #
            # MEASURED, and it invalidated a published headline: `api-web` was dispatched at +13.3m
            # and split at +24.1m, a REAL span of 651s. It was credited 5940s — 9.1x too much — which
            # made it 49.5% of all node-busy when the truth is 9.7%, and that inflated figure was the
            # sole basis for "MAX USEFUL NODES = 1.92, the plan is the ceiling".
            #
            # The phantom_tail detector could not catch it: that guard fires on a task which HAS a
            # completion and still owns a span to t_end. A split parent has NO completion, so it slips
            # through the exact check written to stop this.
            if ts is not None:
                split_at[e.get("task_id")] = ts
        elif ev == "task_completed":
            done.setdefault(e.get("task_id"), []).append(ts)
        if ev in IDLE_NODE_EVENTS:
            idle_jobs[IDLE_NODE_EVENTS[ev]] = idle_jobs.get(IDLE_NODE_EVENTS[ev], 0) + 1

    if t0 is None:
        t0 = min((parse_ts(e.get("ts")) for e in events if parse_ts(e.get("ts"))), default=None)
    n = len(pool) or None
    wall = (t_end - t0) if (t0 is not None and t_end is not None) else None

    # Pair each dispatch with the next completion of the SAME task. A dispatch with no completion is
    # still running (or died) and is credited only up to the last event we can see — never beyond.
    per_device_spans: dict[str, list[tuple[float, float]]] = {}
    per_task_spans: dict[str, list[tuple[float, float]]] = {}
    attempt_spans: list[dict] = []
    spans: list[tuple[float, float]] = []
    for task_id, ds in disp.items():
        cs = sorted(t for t in done.get(task_id, []) if t is not None)
        starts = [s for s, _ in sorted(ds, key=lambda x: (x[0] or 0)) if s is not None]
        for i, (start, device) in enumerate(sorted(ds, key=lambda x: (x[0] or 0))):
            if start is None:
                continue
            # A RETRY re-dispatches a task before the first attempt ever completes, so pairing
            # dispatch[i] with completion[i] leaves the extra dispatch unmatched — and crediting an
            # unmatched dispatch to the END OF THE RUN invents time that was never spent. MEASURED:
            # test-meridian was dispatched twice and completed once, and that single retry was
            # credited 83 minutes of phantom busy time on a 122-minute run, inflating busy from 94 to
            # 156 min, occupancy from 0.26 to 0.43, and the critical path from 31 to 89 min — which
            # produced a published max_useful_nodes of 1.75 when the real figure is about 3.
            #
            # An attempt ends at whichever comes first: its own completion, or the NEXT dispatch of
            # the same task (which is what supersedes it). Only a task still outstanding at the last
            # event we can see is credited to t_end, and on a finished run that is genuinely a task
            # that never completed.
            nxt = starts[i + 1] if i + 1 < len(starts) else None
            comp = next((c for c in cs if c >= start), None)
            # A SPLIT ends this attempt as surely as a completion does.
            spl = split_at.get(task_id)
            if spl is not None and spl < start:
                spl = None
            cands = [x for x in (comp, nxt, spl) if x is not None]
            end = min(cands) if cands else t_end
            if end is None or end < start:
                continue
            per_device_spans.setdefault(device, []).append((start, end))
            per_task_spans.setdefault(task_id, []).append((start, end))
            # Exported verbatim as `_spans` so a caller asking a NEW question about the same
            # timeline (per-phase occupancy, for instance) reuses this pairing instead of rebuilding
            # it. Rebuilding it is not hypothetical: an ad-hoc re-derivation of exactly these spans
            # reported a 1484s solo window where the real figure was 55.9s, because it paired
            # dispatch[i] with completion[i] across a retry.
            attempt_spans.append({"task": task_id, "device": device, "start": start, "end": end})
            spans.append((start, end))

    # A device's busy time is the UNION of its spans, not their sum. Summing was wrong and the first
    # real data caught it: two archived 1-device runs scored 1.28 and 1.93 occupancy, which is
    # impossible — a weight-1 device runs one task at a time, so it cannot be busy for more
    # wall-clock than the run lasted. The inflation comes from retries (two dispatches, one
    # completion) and from in-flight tasks, both of which produce overlapping spans that a sum
    # double-counts. The union is also the honest definition of the thing we want to know: what
    # fraction of the run was this node working.
    def union_secs(iv: list[tuple[float, float]]) -> float:
        total, cur_s, cur_e = 0.0, None, None
        for s, e in sorted(iv):
            if cur_e is None or s > cur_e:
                if cur_e is not None:
                    total += cur_e - cur_s
                cur_s, cur_e = s, e
            else:
                cur_e = max(cur_e, e)
        if cur_e is not None:
            total += cur_e - cur_s
        return total

    # THE PHANTOM-TAIL DETECTOR. A span that ends at the last observed event is only legitimate for
    # a task still outstanding there. If a task HAS a completion and still owns a span running to the
    # end of the run, the pairing invented time — which is exactly how one retry was credited 83
    # minutes on a 122-minute run and inverted a published conclusion. Phantom time on a 3-node pool
    # hides under the wall*N ceiling, so no aggregate invariant can see it; it has to be caught here,
    # at the span.
    phantom_tail = sorted({
        t for t, iv in per_task_spans.items()
        if done.get(t) and any(abs((e or 0) - (t_end or 0)) < 1e-6 for _, e in iv)
    })
    per_device = {d: union_secs(iv) for d, iv in per_device_spans.items()}
    # Union per TASK too, for the same reason: a retried task has overlapping spans, and summing
    # them produced a "share of node-busy" of 1.118 — a share above 1.0, which is nonsense and is
    # the tell that two different measures were being divided by each other.
    per_task = {t: union_secs(iv) for t, iv in per_task_spans.items()}
    busy = sum(per_device.values())
    occupancy = (busy / (wall * n)) if (wall and n) else None

    # Whole-run occupancy UNDERSTATES the scheduler, and the gap is not small. Research, scouts,
    # planning drafts, detailing and contract stubs are real model calls on real nodes, but none of
    # them emits task_dispatched — so that time lands in the denominator as wall and never in the
    # numerator as busy. Judging the scheduler on that number would send someone to fix a scheduler
    # that is behaving correctly during a phase it does not control. EXECUTE occupancy measures the
    # window the scheduler actually owns: first dispatch to last completion.
    exec_start = min((s for s, _ in spans), default=None)
    exec_end = max((e for _, e in spans), default=None)
    exec_wall = (exec_end - exec_start) if (exec_start is not None and exec_end is not None) else None
    exec_occupancy = (busy / (exec_wall * n)) if (exec_wall and n) else None
    pre_exec_secs = (exec_start - t0) if (exec_start is not None and t0 is not None) else None

    # Wall-clock during which at most one node was busy — the serial tail, which more nodes cannot
    # shorten. Computed by sweeping the span endpoints rather than sampling.
    # WHICH TASK owns the serial tail, not just how long it is. Added because the question "is the
    # dynamic-replan precondition ever met" needs to know whether the lone runner is the SINK (replan
    # is deliberately suppressed there) or an ordinary task (it is not) — and because answering it in
    # a throwaway script re-implemented the span pairing and got it wrong in exactly the way the
    # comment above warns about: naive dispatch->completion pairing drops a retried attempt's busy
    # time and inflated solo from 55.9s to 1484.0s. The derived number lives here so nobody has to
    # rebuild the spans to get it.
    solo_secs = None
    solo_by_task: dict[str, float] = {}
    # HOW MANY TASKS WERE RUNNING AT ONCE, in seconds at each level. The same exact interval sweep that
    # already computes solo time answers it for free — and it must be computed HERE, off the corrected
    # spans, because a hand-rolled version got it catastrophically wrong: pairing dispatch[i] with
    # completion[i] and crediting unmatched retries to end-of-run reported NINE concurrent tasks on a
    # SIX-SLOT fleet (F266). The retry/split/phantom-tail corrections above are the whole difference.
    #
    # The unit is CONCURRENT TASKS, not busy nodes: with PARALLEL 2 a 3-node fleet holds 6 tasks, so a
    # reading of 4 is not "more nodes than exist". Anything above pool_size x 2 IS impossible and
    # indicts this sweep.
    concurrency: dict[int, float] = {}
    if spans and wall:
        marks = sorted({t for s in spans for t in s})
        solo = 0.0
        for a, b in zip(marks, marks[1:]):
            mid = (a + b) / 2
            live = [tid for tid, ss in per_task_spans.items()
                    if any(s <= mid < e for s, e in ss)]
            concurrency[len(live)] = concurrency.get(len(live), 0.0) + (b - a)
            if len(live) == 1:
                solo += b - a
                solo_by_task[live[0]] = solo_by_task.get(live[0], 0.0) + (b - a)
        solo_secs = solo

    biggest = max(per_task.items(), key=lambda kv: kv[1]) if per_task else None

    # THE PLAN CEILING. Low occupancy has two completely different causes and they call for opposite
    # fixes: either the scheduler is leaving nodes idle it could have used, or the PLAN has no more
    # independent work to give it. Only the DAG can tell them apart.
    #
    # With measured per-task durations, the shortest wall-clock any scheduler could achieve on N
    # workers is bounded below by max(critical path, total work / N) — the first term is the longest
    # chain of dependent tasks, which no amount of hardware shortens. So:
    #
    #     max_useful_nodes = total_work / critical_path
    #
    # is the node count beyond which this plan CANNOT go faster, whatever the fleet. If that number
    # is below the pool size, the answer to "does the swarm get better with more nodes" is decided by
    # the planner, not the scheduler, and the work belongs in the architect prompt.
    plan_deps: dict[str, list[str]] = {}
    for e in events:
        if e.get("event") == "plan_loaded":
            for t in e.get("tasks") or []:
                plan_deps[t.get("id")] = list(t.get("depends_on") or t.get("deps") or [])
    for tid, ds in disp.items():
        plan_deps.setdefault(tid, [])

    def longest_path() -> float:
        memo: dict[str, float] = {}
        visiting: set[str] = set()

        def walk(t: str) -> float:
            if t in memo:
                return memo[t]
            if t in visiting:      # a cycle cannot happen in a validated DAG, but never hang on one
                return 0.0
            visiting.add(t)
            best = max((walk(d) for d in plan_deps.get(t, []) if d in plan_deps), default=0.0)
            visiting.discard(t)
            memo[t] = best + per_task.get(t, 0.0)
            return memo[t]

        return max((walk(t) for t in plan_deps), default=0.0)

    total_work = sum(per_task.values())
    critical = longest_path()
    max_useful = (total_work / critical) if critical > 0 else None
    ceiling_wall = max(critical, total_work / n) if (n and critical > 0) else None
    ceiling_occ = (total_work / (ceiling_wall * n)) if (ceiling_wall and n) else None

    # A task is unfinished when its LAST dispatch has no completion at or after it. Computed once here
    # so the count and the ID list can never disagree.
    unfinished = sorted(
        t for t, ds in disp.items()
        if (lambda last: last is not None and not any(
            c is not None and c >= last for c in done.get(t, [])))(
            max((s for s, _ in ds if s is not None), default=None)))
    prefix = prefix_breakdown(events, t0)
    return {
        "occupancy_version": OCCUPANCY_VERSION,
        "path": str(path),
        "pool_size": n,
        "prefix_phases": prefix["phases"],
        "prefix_draft_rounds": prefix["draft_rounds"],
        "prefix_redraft_secs": prefix["redraft_secs"],
        "prefix_plan_confidence": prefix["plan_confidence"],
        "devices_that_worked": len(per_device),
        "wall_secs": round(wall, 1) if wall else None,
        "busy_node_secs": round(busy, 1),
        "occupancy": round(occupancy, 4) if occupancy is not None else None,
        "per_device_secs": {k: round(v, 1) for k, v in sorted(per_device.items())},
        "device_share": {k: round(v / busy, 3) for k, v in sorted(per_device.items())} if busy else {},
        "biggest_task": biggest[0] if biggest else None,
        "biggest_task_share_of_busy": round(biggest[1] / busy, 3) if biggest and busy else None,
        "concurrency_secs": {k: round(v, 1) for k, v in sorted(concurrency.items())},
        "concurrency_share": ({k: round(v / sum(concurrency.values()), 3)
                               for k, v in sorted(concurrency.items())} if concurrency else {}),
        "solo_node_secs": round(solo_secs, 1) if solo_secs is not None else None,
        "solo_by_task": {k: round(v, 1) for k, v in
                         sorted(solo_by_task.items(), key=lambda kv: -kv[1])},
        "solo_share_of_wall": round(solo_secs / wall, 3) if (solo_secs is not None and wall) else None,
        "execute_wall_secs": round(exec_wall, 1) if exec_wall else None,
        "execute_occupancy": round(exec_occupancy, 4) if exec_occupancy is not None else None,
        "pre_execute_secs": round(pre_exec_secs, 1) if pre_exec_secs is not None else None,
        "total_task_secs": round(total_work, 1),
        "critical_path_secs": round(critical, 1),
        "max_useful_nodes": round(max_useful, 2) if max_useful else None,
        "ceiling_occupancy_at_pool": round(ceiling_occ, 4) if ceiling_occ is not None else None,
        "phantom_tail_tasks": phantom_tail,
        # Parents whose span was correctly ENDED AT THEIR SPLIT rather than run to t_end. Reported so
        # a reader can see the correction was applied — a silent fix to a number this project has
        # already published would be worse than the bug.
        "split_superseded_tasks": sorted(split_at),
        "idle_node_jobs": idle_jobs,
        "_spans": attempt_spans,
        "_t0": t0,
        "_t_end": t_end,
        # A dispatch with no completion means "still running" only while the run is still going. On
        # a FINISHED run the same shape means the task never completed at all — a failure, not work
        # in progress — and calling it in-flight made three archived, finished runs look live.
        "finished": any(e.get("event") == "run_finished" for e in events),
        # A task is unfinished when its LAST dispatch has no completion at or after it. Counting
        # "more dispatches than completions" calls every RETRY unfinished — 2 dispatches, 1
        # completion — even though the retry completed. That stale counter survived the pairing fix
        # and was caught by the harness self-test on its first real run, which is the whole point of
        # having one.
        "unfinished_tasks": len(unfinished),
        # The IDs, not only the count. The self-test needs to check the invariant ON THESE TASKS; when
        # it re-derived its own cruder rule instead it voided a clean 112-minute run because two
        # UNRELATED tasks had been retried, while the genuinely-unfinished one was a third task
        # dispatched once and never completed. One definition, exported, or the two drift.
        "unfinished_task_ids": unfinished,
    }


PREFIX_MILESTONES = (
    "research_completed", "skeleton_drafts", "confidence_retarget", "retarget_discarded",
    "low_confidence_ask", "low_confidence_ask_timeout", "contracts", "plan_loaded",
)


def prefix_breakdown(events: list[dict], t0: float | None) -> dict:
    """Where the PRE-DISPATCH time actually goes, phase by phase.

    The prefix is 71% of a 3-node run's wall (F261) and this file used to report it as one lump —
    "2218.7s before the first dispatch" — so the phases had to be re-derived by hand every time, which
    is how a fourth ad-hoc reader gets written (L2).

    It is also where the arms differ in KIND, not just in speed (F262): plan confidence is agreement
    across independent drafts, so a 1-node run computes `null`, cannot breach the ask floor, and never
    redrafts — while a 3-node run resolves a real number, fails the floor, and pays for a second full
    planning pass. `draft_rounds` and `redraft_secs` are what make that visible without reading a log.

    `detail_completed` fires once per task and would drown the list, so it is collapsed into one
    `detail x N` span. Everything is measured from `run_started`; a run with no first dispatch yet
    reports what it has.
    """
    if t0 is None:
        return {"phases": [], "draft_rounds": 0, "redraft_secs": None, "plan_confidence": None}
    first_disp = None
    marks: list[tuple[float, str]] = []
    detail_at: list[float] = []
    draft_rounds = 0
    first_retarget = None
    plan_conf = None
    plan_loaded_at = None
    for e in events:
        ts = parse_ts(e.get("ts"))
        ev = e.get("event")
        if ts is None:
            continue
        if ev == "task_dispatched" and first_disp is None:
            first_disp = ts
        if first_disp is not None:
            continue
        if ev == "detail_completed":
            detail_at.append(ts)
            continue
        if ev in PREFIX_MILESTONES:
            if ev == "skeleton_drafts":
                draft_rounds += 1
            if ev == "confidence_retarget" and first_retarget is None:
                first_retarget = ts
            if ev == "plan_loaded":
                plan_loaded_at = ts
                plan_conf = e.get("plan_confidence")
            marks.append((ts, ev))
    if detail_at:
        marks.append((max(detail_at), f"detail x{len(detail_at)}"))
    marks.sort()

    phases, prev = [], t0
    for ts, label in marks:
        phases.append({"phase": label, "at_secs": round(ts - t0, 1), "dur_secs": round(ts - prev, 1)})
        prev = ts
    if first_disp is not None:
        phases.append({"phase": "first dispatch", "at_secs": round(first_disp - t0, 1),
                       "dur_secs": round(first_disp - prev, 1)})
    # The cost of NOT being confident: everything from the first retarget to a shipped plan. A 1-node
    # run cannot enter this window at all, so `None` here is a structural fact, not a missing reading.
    redraft = (round(plan_loaded_at - first_retarget, 1)
               if (first_retarget is not None and plan_loaded_at is not None) else None)
    return {"phases": phases, "draft_rounds": draft_rounds,
            "redraft_secs": redraft, "plan_confidence": plan_conf}


def render(a: dict) -> str:
    if a.get("occupancy") is None and a.get("pool_size") is None:
        return f"=== {a['path']}\n  {a.get('note', 'nothing measurable')}"
    out = [f"=== {a['path']}  ({a['occupancy_version']})"]
    out.append(f"  pool {a['pool_size']} device(s); {a['devices_that_worked']} did work")
    out.append(f"  wall {a['wall_secs']}s   busy {a['busy_node_secs']} node-secs   "
               f"OCCUPANCY {a['occupancy']}"
               + (f"  (perfect = 1.0, one-node-only = {round(1 / a['pool_size'], 3)})"
                  if a["pool_size"] else ""))
    out.append(f"  EXECUTE window {a['execute_wall_secs']}s (scheduler-owned)   "
               f"EXECUTE OCCUPANCY {a['execute_occupancy']}   "
               f"— {a['pre_execute_secs']}s before the first dispatch is research/plan/contracts, "
               f"real node work that emits no task event")
    if a.get("concurrency_secs"):
        tot = sum(a["concurrency_secs"].values())
        cap = (a["pool_size"] or 0) * 2      # PARALLEL 2 => slots; above this is IMPOSSIBLE
        out.append(f"  CONCURRENT TASKS over {tot / 60:.0f} min of dispatch window "
                   f"(fleet holds {cap} at PARALLEL 2)")
        for k, v in a["concurrency_secs"].items():
            flag = "   <-- IMPOSSIBLE, indicts this sweep" if cap and k > cap else ""
            out.append(f"    {k:>2d} task(s): {a['concurrency_share'].get(k, 0):6.1%}  "
                       f"({v / 60:6.1f} min){flag}")
    if a.get("prefix_phases"):
        out.append(f"  PREFIX breakdown  (draft rounds {a.get('prefix_draft_rounds')}, "
                   f"plan_confidence {a.get('prefix_plan_confidence')}, "
                   # A 1-node run CANNOT enter the redraft window — its plan_confidence is null and the
                   # floor is unbreachable — so "n/a (cannot redraft)" is the honest reading, not "0s".
                   + (f"redraft cost {a['prefix_redraft_secs']}s)"
                      if a.get("prefix_redraft_secs") is not None
                      else "redraft cost n/a — this run never entered a redraft)"))
        for p in a["prefix_phases"]:
            out.append(f"    +{p['at_secs']:>7.0f}s  {p['dur_secs']:>6.0f}s  {p['phase']}")
    for d, s in a["per_device_secs"].items():
        out.append(f"    {d:<46} {s:>8.0f}s  {a['device_share'].get(d, 0):.1%}")
    out.append(f"  biggest task: {a['biggest_task']} = {a['biggest_task_share_of_busy']} of node-busy")
    if (a["pool_size"] or 0) < 2:
        out.append("  serial-tail: not meaningful at pool size 1 — every busy moment is solo there")
    else:
        out.append(f"  only ONE node working for {a['solo_node_secs']}s "
                   f"({a['solo_share_of_wall']} of wall) — more nodes cannot shorten that")
        for tid, secs in list((a.get("solo_by_task") or {}).items())[:4]:
            note = "  <- the SINK; dynamic replan is deliberately suppressed here" \
                if tid == "integrate-verify" else ""
            out.append(f"      {secs:>8.1f}s  {tid}{note}")
    mu, ceil = a.get("max_useful_nodes"), a.get("ceiling_occupancy_at_pool")
    if mu and not a.get("finished"):
        # The critical path only GROWS as a run proceeds, so this ratio falls monotonically and a
        # mid-run reading is not a verdict about the plan. Measured on one live run it read 3.58,
        # then 2.93, then 2.19 within an hour — the same plan, three different "answers".
        out.append(f"  plan ceiling: NOT YET MEANINGFUL — the run is unfinished, and the critical "
                   f"path only grows (currently would read {mu})")
    elif mu:
        verdict = ("the PLAN is the ceiling — more nodes cannot help this run"
                   if mu < (a["pool_size"] or 1)
                   else "the plan could use more nodes than the fleet has")
        out.append(f"  plan ceiling: critical path {a['critical_path_secs']}s of "
                   f"{a['total_task_secs']}s total work")
        out.append(f"    MAX USEFUL NODES = {mu}   (pool is {a['pool_size']}) — {verdict}")
        out.append(f"    best occupancy any scheduler could reach on this plan at this pool: {ceil}"
                   f"   (actual {a['occupancy']})")
    out.append(f"  idle-node jobs (the 'smarter with more nodes' half): {a['idle_node_jobs'] or 'none'}")
    if a["unfinished_tasks"]:
        if a["finished"]:
            out.append(f"  NOTE: {a['unfinished_tasks']} task(s) were dispatched and NEVER completed "
                       f"on a finished run — those are failures, not work in progress")
        else:
            out.append(f"  NOTE: run still going, {a['unfinished_tasks']} task(s) in flight — "
                       f"occupancy is a lower bound")
    return "\n".join(out)


def real_shape_control() -> list[str]:
    """Assert IDLE_NODE_EVENTS uses names the ENGINE actually emits, against a run on disk.

    Every other control in this file is driven by a stream this file writes, so all of them passed
    while four of the eight keys named events that do not exist. A control that shares the
    instrument's assumption cannot test it.

    `pre_review` is the anchor: it fired 7 times in every measured unit, so if the census cannot see
    it, the map is wrong. The mechanisms that legitimately never fire (`replanned`, `sink_review`)
    cannot be asserted positively and are deliberately not — that would be a control demanding a
    defect stay present.
    """
    import collections

    fails: list[str] = []
    # A FINISHED run, never merely the newest file. An in-flight run has not reached the phases these
    # mechanisms live in, so its absent events are a clock, not a defect — and picking newest-wins is
    # the same trap that once made a panel re-render a four-hour-old run. Measured: this control fired
    # against a unit five minutes old, which had legitimately not pre-reviewed anything yet.
    counts: collections.Counter = collections.Counter()
    chosen = None
    for path in sorted(HERE.parent.glob("runs/nodeloop*/*/run.jsonl"), reverse=True):
        c: collections.Counter = collections.Counter()
        for line in path.read_text(errors="replace").splitlines():
            try:
                c[json.loads(line).get("event")] += 1
            except json.JSONDecodeError:
                continue
        if c.get("run_finished"):
            counts, chosen = c, path
            break
    if chosen is None:
        return fails
    if counts.get("pre_review", 0) and "pre_review" not in IDLE_NODE_EVENTS:
        fails.append(f"{chosen.parent.name}: the run emits pre_review {counts['pre_review']}x but "
                     f"IDLE_NODE_EVENTS has no such key — the census is silently zeroing a mechanism")
    unknown = [k for k in IDLE_NODE_EVENTS
               if k not in counts and k not in ("replanned", "sink_review", "speculated",
                                                "task_split")]
    if unknown:
        fails.append(f"IDLE_NODE_EVENTS keys never seen in {chosen.parent.name} and not on the "
                     f"known-never-fires list: {unknown} — check them against the engine's emitted "
                     f"event names before trusting a zero")
    return fails


def self_test() -> int:
    """Controls in BOTH directions, plus the vacuous-truth case an empty log would otherwise pass."""
    import tempfile
    fails = real_shape_control()

    def check(name, got, want, tol=0.02):
        ok = (got == want) if not isinstance(want, float) else (
            got is not None and abs(got - want) <= tol)
        if not ok:
            fails.append(f"{name}: got {got!r}, want {want!r}")

    def write(events) -> str:
        fh = tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False)
        for e in events:
            fh.write(json.dumps(e) + "\n")
        fh.close()
        return fh.name

    def ts(sec):
        return datetime.utcfromtimestamp(1_700_000_000 + sec).isoformat() + "+00:00"

    def pool(n):
        return [{"id": f"d{i}", "model_id": f"m{i}", "weight": 1} for i in range(n)]

    # PERFECT: 3 nodes each busy the entire 100s wall -> occupancy 1.0
    ev = [{"event": "run_started", "pool": pool(3), "ts": ts(0)}]
    for i in range(3):
        ev.append({"event": "task_dispatched", "task_id": f"t{i}", "device": f"d{i}", "ts": ts(0)})
    for i in range(3):
        ev.append({"event": "task_completed", "task_id": f"t{i}", "device": f"d{i}", "ts": ts(100)})
    check("3 nodes fully busy -> 1.0", analyse(write(ev))["occupancy"], 1.0)

    # WORST: 3-node pool, one node does everything -> occupancy 1/3
    ev = [{"event": "run_started", "pool": pool(3), "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "t0", "device": "d0", "ts": ts(0)},
          {"event": "task_completed", "task_id": "t0", "device": "d0", "ts": ts(100)}]
    a = analyse(write(ev))
    check("1 of 3 nodes working -> 0.333", a["occupancy"], 1 / 3)
    check("solo tail is the whole wall", a["solo_share_of_wall"], 1.0)
    check("only one device worked", a["devices_that_worked"], 1)

    # A 1-node run cannot be penalised for having one node: full use is still 1.0.
    ev = [{"event": "run_started", "pool": pool(1), "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "t0", "device": "d0", "ts": ts(0)},
          {"event": "task_completed", "task_id": "t0", "device": "d0", "ts": ts(50)}]
    check("1-node fully busy -> 1.0", analyse(write(ev))["occupancy"], 1.0)

    # VACUOUS TRUTH: an empty log must measure NOTHING, never a perfect score. all([]) is True and
    # this is exactly where that bites.
    a = analyse(write([]))
    check("empty log -> occupancy None", a["occupancy"], None)
    check("empty log -> pool None", a["pool_size"], None)

    # A run with a pool but no dispatches is real ZERO occupancy, not "unmeasurable".
    a = analyse(write([{"event": "run_started", "pool": pool(3), "ts": ts(0)},
                       {"event": "run_finished", "ts": ts(100)}]))
    check("pool but no work -> 0.0", a["occupancy"], 0.0)

    # Determinism: two passes over identical input must agree exactly.
    p = write(ev)
    check("deterministic", analyse(p)["occupancy"], analyse(p)["occupancy"])

    # THE CONTROL THAT WAS MISSING, and that real data caught before it did: a RETRY produces two
    # dispatches and one completion, and an IN-FLIGHT task produces a dispatch with none. Summing
    # those spans made two archived 1-device runs score 1.28 and 1.93 occupancy — impossible, since
    # a weight-1 device runs one task at a time. Occupancy must NEVER exceed 1.0.
    ev = [{"event": "run_started", "pool": pool(1), "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "t0", "device": "d0", "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "t0", "device": "d0", "ts": ts(10)},
          {"event": "task_completed", "task_id": "t0", "device": "d0", "ts": ts(100)}]
    a = analyse(write(ev))
    check("a retry cannot exceed the wall", a["occupancy"], 1.0)

    ev = [{"event": "run_started", "pool": pool(1), "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "a", "device": "d0", "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "b", "device": "d0", "ts": ts(20)},
          {"event": "task_completed", "task_id": "a", "device": "d0", "ts": ts(60)},
          {"event": "run_finished", "ts": ts(100)}]
    a = analyse(write(ev))
    check("in-flight overlap cannot exceed the wall", a["occupancy"], 1.0)
    # That case DOES carry run_finished, so the honest reading is "a finished run with one task
    # that never completed" — a failure, not work in progress. The distinction matters because
    # calling it in-flight made three archived, finished runs look live.
    check("never-completed task is counted", a["unfinished_tasks"], 1)
    check("a run with run_finished is finished", a["finished"], True)

    # A genuinely unfinished run: same shape, no run_finished.
    live = analyse(write([{"event": "run_started", "pool": pool(1), "ts": ts(0)},
                          {"event": "task_dispatched", "task_id": "a", "device": "d0", "ts": ts(0)},
                          {"event": "judge_verdict", "task_id": "a", "ts": ts(60)}]))
    check("a live run is not marked finished", live["finished"], False)
    check("a live run reports its in-flight task", live["unfinished_tasks"], 1)

    # And the invariant itself, stated once so any future change has to keep it.
    for name, evs in (("retry", ev),):
        got = analyse(write(evs))["occupancy"]
        if got is not None and got > 1.0 + 1e-9:
            fails.append(f"{name}: occupancy {got} exceeds 1.0, which is physically impossible")

    # The biggest-task share must actually find the hog, or the serialization metric is decorative.
    ev = [{"event": "run_started", "pool": pool(3), "ts": ts(0)},
          {"event": "task_dispatched", "task_id": "small", "device": "d0", "ts": ts(0)},
          {"event": "task_completed", "task_id": "small", "device": "d0", "ts": ts(10)},
          {"event": "task_dispatched", "task_id": "sink", "device": "d1", "ts": ts(0)},
          {"event": "task_completed", "task_id": "sink", "device": "d1", "ts": ts(90)}]
    a = analyse(write(ev))
    check("finds the hog", a["biggest_task"], "sink")
    check("hog share", a["biggest_task_share_of_busy"], 0.9)

    # PLAN CEILING controls, both directions. These decide whether low occupancy is the scheduler's
    # fault or the planner's, so getting them backwards would send the work to the wrong place.
    # Fully PARALLEL: 3 equal independent tasks -> critical path = one task -> 3 nodes are all useful.
    ev = [{"event": "run_started", "pool": pool(3), "ts": ts(0)},
          {"event": "plan_loaded", "tasks": [{"id": f"t{i}", "depends_on": []} for i in range(3)],
           "ts": ts(0)}]
    for i in range(3):
        ev.append({"event": "task_dispatched", "task_id": f"t{i}", "device": f"d{i}", "ts": ts(0)})
        ev.append({"event": "task_completed", "task_id": f"t{i}", "device": f"d{i}", "ts": ts(30)})
    a = analyse(write(ev))
    check("parallel plan: 3 nodes useful", a["max_useful_nodes"], 3.0)
    check("parallel plan: ceiling is perfect", a["ceiling_occupancy_at_pool"], 1.0)

    # Fully SERIAL: a->b->c, each 30s. Critical path IS the total work, so only ONE node is ever
    # useful and the best any scheduler can do on a 3-pool is 1/3.
    ev = [{"event": "run_started", "pool": pool(3), "ts": ts(0)},
          {"event": "plan_loaded", "tasks": [{"id": "a", "depends_on": []},
                                             {"id": "b", "depends_on": ["a"]},
                                             {"id": "c", "depends_on": ["b"]}], "ts": ts(0)}]
    for i, t in enumerate(["a", "b", "c"]):
        ev.append({"event": "task_dispatched", "task_id": t, "device": "d0", "ts": ts(i * 30)})
        ev.append({"event": "task_completed", "task_id": t, "device": "d0", "ts": ts(i * 30 + 30)})
    a = analyse(write(ev))
    check("serial plan: only 1 node useful", a["max_useful_nodes"], 1.0)
    check("serial plan: ceiling is 1/3 on a 3-pool", a["ceiling_occupancy_at_pool"], 1 / 3)

    if fails:
        print("SELF-TEST FAILED:")
        for f in fails:
            print("  -", f)
        return 1
    print(f"self-test OK ({OCCUPANCY_VERSION}) — perfect/worst/1-node controls, vacuous-truth, "
          "real-zero, determinism and hog detection all pass")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    args = [a for a in argv[1:] if not a.startswith("--")]
    if not args:
        print(__doc__)
        return 2
    results = [analyse(a) for a in args]
    if "--json" in argv:
        print(json.dumps(results if len(results) > 1 else results[0], indent=1))
    else:
        for r in results:
            print(render(r))
            print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
