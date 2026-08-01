#!/usr/bin/env python3
"""Where the pre-dispatch prefix goes. Exit 0 always — this reports, it never grades.

WHY. `occupancy.py` measures the EXECUTE window, because that is the only window that emits task
events. Everything before the first dispatch — scouts, the confidence ladder, the skeleton drafts,
the detail fan, contracts — is invisible to it, and it is a quarter of the run. Round 5's proposals
for shrinking that quarter were all refuted, and the reason is visible in hindsight: they were
designed against a number nobody had measured. The prefix was treated as "serial planning" when most
of it is fleet work that simply emits no task event.

MEASURED across the three baseline units on disk, and the shape is stable:

    research    264 / 379 / 420 s
    planning    1292 / 1457 / 892 s     <- 57-83% of the prefix
    prefix      1556 / 1836 / 1312 s

The unit that retargeted spent 582 s on a planning round, discarded it, and spent 710 s on the
replacement. So this also reports what `retarget_discarded` says was thrown away, and how much of it
came back unchanged in the plan that actually shipped — the number that decides whether a reuse path
is worth building. Runs from before that event existed report it as unavailable, never as zero: an
absent event is a blind instrument, not an empty world.

Usage:
    python3 prefix.py <unit-dir> [<unit-dir> ...]
    python3 prefix.py --self-test
"""
from __future__ import annotations

import datetime as dt
import json
import pathlib
import sys

PREFIX_VERSION = "px-1"


def _ts(v):
    try:
        return dt.datetime.fromisoformat(str(v).replace("Z", "+00:00")).timestamp()
    except (ValueError, TypeError):
        return None


def read_events(unit: pathlib.Path) -> list[dict]:
    f = unit / "run.jsonl"
    if not f.is_file():
        return []
    out = []
    for line in f.read_text(errors="replace").splitlines():
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def analyse_events(events: list[dict]) -> dict:
    """Pure, so the controls can drive it with a hand-built stream."""
    t0 = None
    first_dispatch = None
    research_done = None
    rounds: list[dict] = []      # one per confidence_retarget that redrafted
    discarded: list[dict] = []   # one per retarget_discarded
    fallbacks: list[dict] = []
    plan_tasks: list[dict] = []
    contracts_at = None

    for e in events:
        ev = e.get("event")
        t = _ts(e.get("ts"))
        if ev == "run_started" and t0 is None:
            t0 = t
        elif ev == "research_completed" and research_done is None:
            research_done = t
        elif ev == "confidence_retarget" and e.get("action") == "redraft":
            rounds.append({"round": e.get("round"), "at": t, "conf_before": e.get("conf_before")})
        elif ev == "retarget_discarded":
            discarded.append({"round": e.get("round"), "tasks": e.get("tasks") or []})
        elif ev == "detail_fallback":
            fallbacks.append({"task_id": e.get("task_id"), "reason": e.get("reason"),
                              "brief_chars": e.get("brief_chars"), "at": t})
        elif ev == "contracts" and contracts_at is None:
            contracts_at = t
        elif ev == "plan_loaded" and not plan_tasks:
            plan_tasks = [t_ for t_ in (e.get("tasks") or []) if isinstance(t_, dict)]
        elif ev == "task_dispatched" and first_dispatch is None:
            first_dispatch = t

    # An absent run_started or an absent dispatch means the window does not exist. Report nothing
    # rather than a zero — `all([])` is True and a prefix of 0s would read as "instant planning".
    if t0 is None or first_dispatch is None:
        return {"prefix_version": PREFIX_VERSION, "measurable": False,
                "reason": "no run_started or no task_dispatched — the prefix window does not exist"}

    prefix = first_dispatch - t0
    research = (research_done - t0) if research_done else None
    planning = prefix - research if research is not None else None

    # Per-round planning cost. A round ENDS at its redraft event; the last round ends at first dispatch.
    bounds = [research_done or t0] + [r["at"] for r in rounds] + [first_dispatch]
    round_secs = [b - a for a, b in zip(bounds, bounds[1:])]

    # THE decision number: of the tasks a redraft discarded, how many come back in the shipped plan?
    #
    # BY ID UNDER-COUNTS, and it is not a hypothetical. MEASURED: a run whose redraft discarded
    # `meridian-client` shipped the same module as `meridian` — same job, same file, renamed. An
    # id-keyed reuse path would have thrown that spec away and re-derived it. So survival is measured
    # BOTH ways and the file-based number is the one a reuse path should be designed against: a
    # module is the files it owns, not the name a draft happened to give it.
    reuse = None
    if discarded and plan_tasks:
        shipped_ids = {t.get("id") for t in plan_tasks}
        shipped_files = {f for t in plan_tasks for f in (t.get("owned_files") or [])}
        rows = []
        for d in discarded:
            tasks = [t_ for t_ in d["tasks"] if isinstance(t_, dict)]
            by_id = [t_ for t_ in tasks if t_.get("id") in shipped_ids]
            by_file = [t_ for t_ in tasks
                       if set(t_.get("owned_files") or []) & shipped_files]
            rows.append({
                "round": d["round"],
                "discarded": len(tasks),
                "survived_by_id": len(by_id),
                "survived_by_owned_files": len(by_file),
                "survivor_desc_chars": sum(t_.get("desc_chars") or 0 for t_ in by_file),
                "renamed": sorted({t_.get("id") for t_ in by_file}
                                  - {t_.get("id") for t_ in by_id}),
            })
        reuse = rows

    return {
        "prefix_version": PREFIX_VERSION,
        "measurable": True,
        "prefix_secs": round(prefix, 1),
        "research_secs": round(research, 1) if research is not None else None,
        "planning_secs": round(planning, 1) if planning is not None else None,
        "planning_share_of_prefix": round(planning / prefix, 3) if planning and prefix else None,
        "redraft_rounds": len(rounds),
        "round_secs": [round(s, 1) for s in round_secs],
        "detail_fallbacks": fallbacks,
        # A fallback on a task the shipped plan does not contain cost fleet time but harmed no
        # worker: it belonged to a draft that was thrown away. Counting the two together overstates
        # the damage, which is exactly the mechanism-vs-quality confusion dispatch_audit exists to
        # avoid. MEASURED: `meridian-client` in one run, a ghost of the discarded round.
        "ghost_fallbacks": [f["task_id"] for f in fallbacks
                            if plan_tasks and f["task_id"] not in {t_.get("id") for t_ in plan_tasks}],
        "contracts_to_dispatch_secs": (round(first_dispatch - contracts_at, 1)
                                       if contracts_at else None),
        "plan_task_count": len(plan_tasks),
        # None means the engine did not emit retarget_discarded (pre-boundary build). NOT zero.
        "reuse": reuse,
        "reuse_available": bool(discarded),
    }


def analyse(unit: pathlib.Path) -> dict:
    r = analyse_events(read_events(unit))
    r["unit"] = unit.name
    return r


def render(r: dict) -> str:
    if not r.get("measurable"):
        return f"=== PREFIX {r.get('unit','?')}  NOT MEASURABLE — {r.get('reason')}"
    out = [f"=== PREFIX {r['unit']}  ({r['prefix_version']})  {r['prefix_secs']:.0f}s before the "
           f"first dispatch"]
    out.append(f"    research {r['research_secs']:.0f}s   planning {r['planning_secs']:.0f}s "
               f"({r['planning_share_of_prefix']:.0%} of the prefix)   "
               f"redraft rounds {r['redraft_rounds']}  per-round {r['round_secs']}")
    if r["detail_fallbacks"]:
        ghosts = set(r.get("ghost_fallbacks") or [])
        who = ", ".join(f"{f['task_id']}({f['brief_chars']}ch)"
                        + (" [GHOST — never shipped]" if f["task_id"] in ghosts else "")
                        for f in r["detail_fallbacks"])
        out.append(f"    detail fell back to the architect's one-liner for: {who}")
    if r["reuse"]:
        for row in r["reuse"]:
            out.append(f"    round {row['round']}: discarded {row['discarded']} task(s); "
                       f"{row['survived_by_owned_files']} came back by OWNED FILES "
                       f"({row['survived_by_id']} by id), carrying {row['survivor_desc_chars']} "
                       f"chars of detail that were re-derived"
                       + (f" — renamed: {row['renamed']}" if row["renamed"] else ""))
    elif not r["reuse_available"]:
        out.append("    retarget reuse: UNAVAILABLE on this engine build (retarget_discarded not "
                   "emitted) — not zero, unmeasured")
    return "\n".join(out)


def self_test() -> int:
    """Both directions, plus the vacuous-truth trap."""
    fails = []

    # An EMPTY stream must be NOT MEASURABLE, never a 0-second prefix.
    empty = analyse_events([])
    if empty.get("measurable"):
        fails.append("an empty event stream reported a measurable prefix")

    # A stream with a dispatch but no run_started is equally unmeasurable.
    if analyse_events([{"event": "task_dispatched", "ts": "2026-01-01T00:00:00+00:00"}]).get("measurable"):
        fails.append("a stream with no run_started reported a measurable prefix")

    def at(s):
        return dt.datetime.fromtimestamp(s, dt.timezone.utc).isoformat()

    # KNOWN VALUES: 100s research, a redraft at 400s, first dispatch at 1000s.
    stream = [
        {"event": "run_started", "ts": at(0)},
        {"event": "research_completed", "ts": at(100)},
        {"event": "confidence_retarget", "action": "redraft", "round": 1, "ts": at(400)},
        {"event": "retarget_discarded", "round": 1, "ts": at(400), "tasks": [
            {"id": "api", "desc_chars": 900, "owned_files": ["a.py"]},
            {"id": "web-old", "desc_chars": 700, "owned_files": ["w.py"]},
            {"id": "gone", "desc_chars": 500, "owned_files": ["z.py"]}]},
        {"event": "detail_fallback", "task_id": "api", "reason": "timeout",
         "brief_chars": 83, "ts": at(380)},
        {"event": "contracts", "ts": at(980)},
        {"event": "plan_loaded", "ts": at(1000), "tasks": [
            {"id": "api", "owned_files": ["a.py"]}, {"id": "web", "owned_files": ["w.py"]}]},
        {"event": "task_dispatched", "ts": at(1000)},
    ]
    r = analyse_events(stream)
    if r["prefix_secs"] != 1000.0:
        fails.append(f"prefix {r['prefix_secs']} != 1000")
    if r["research_secs"] != 100.0:
        fails.append(f"research {r['research_secs']} != 100")
    if r["planning_secs"] != 900.0:
        fails.append(f"planning {r['planning_secs']} != 900")
    if r["round_secs"] != [300.0, 600.0]:
        fails.append(f"round_secs {r['round_secs']} != [300, 600]")
    # `api` survives by id and carried 900 chars; `gone` does not survive and must NOT be counted.
    row = (r["reuse"] or [{}])[0]
    # `api` survives under both keys. `web-old` -> `web` survives ONLY by owned files, and it is the
    # case an id-keyed reuse path would silently miss. `gone` survives under neither.
    if row.get("survived_by_id") != 1:
        fails.append(f"survived_by_id {row.get('survived_by_id')} != 1: {row}")
    if row.get("survived_by_owned_files") != 2:
        fails.append(f"survived_by_owned_files {row.get('survived_by_owned_files')} != 2: {row}")
    if row.get("renamed") != ["web-old"]:
        fails.append(f"the rename was not detected: {row}")
    if row.get("survivor_desc_chars") != 1600:
        fails.append(f"survivor chars {row.get('survivor_desc_chars')} != 1600: {row}")
    if r.get("ghost_fallbacks") != []:
        fails.append(f"api shipped, so it is not a ghost: {r.get('ghost_fallbacks')}")
    ghost = analyse_events(stream + [{"event": "detail_fallback", "task_id": "web-old",
                                      "reason": "timeout", "brief_chars": 86, "ts": at(390)}])
    if ghost.get("ghost_fallbacks") != ["web-old"]:
        fails.append(f"a fallback on a task that never shipped must be a ghost: {ghost.get('ghost_fallbacks')}")

    # NEGATIVE DIRECTION: the same stream WITHOUT retarget_discarded must report unavailable, and
    # must NOT report a reuse rate of zero — that is the difference between blind and empty.
    no_ev = analyse_events([e for e in stream if e["event"] != "retarget_discarded"])
    if no_ev["reuse"] is not None or no_ev["reuse_available"]:
        fails.append("a build that never emitted retarget_discarded reported a reuse figure")

    if fails:
        print(f"prefix.py SELF-TEST FAILED ({PREFIX_VERSION}):")
        for f in fails:
            print("  -", f)
        return 1
    print(f"prefix.py self-test OK ({PREFIX_VERSION})")
    return 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    args = [a for a in argv[1:] if not a.startswith("--")]
    if not args:
        print(__doc__)
        return 2
    for a in args:
        u = pathlib.Path(a).resolve()
        if not u.is_dir():
            print(f"no such unit dir: {u}")
            continue
        r = analyse(u)
        print(json.dumps(r, indent=1) if "--json" in argv else render(r))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
