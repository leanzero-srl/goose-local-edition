#!/usr/bin/env python3
"""THE STANDING SCOREBOARD — two pillars, never one.

Mihai, 19:05: "make sure both quality and speed are the pillars to look out for and gradually
improve."

That is the whole contract of this file. QUALITY is the build score; SPEED is wall-clock. Every row
prints both, so a gain in one that was bought with the other cannot be reported as progress. Today
that mattered: three nodes score +0.18 and take 20% longer, and either number alone tells a story
the other contradicts.

SPLIT BY build_sha, ALWAYS. Cell directories are reused, so a cell name identifies a slot and never a
generation. Pooling across commits once produced a "the wall gap has closed to parity" headline that
was an artefact of one cell built from a DIRTY tree (F479). Rows here are grouped by commit and the
cross-commit mean is never computed.

WITHIN-ARM SPREAD IS PRINTED BESIDE EVERY DELTA. A gap smaller than the spread of the arm it came
from is a direction, not an effect, and this campaign has already published three such gaps as
findings before retracting them.
"""
import json
import os
import subprocess
import sys
import re
from statistics import mean

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import shardshare  # noqa: E402  (path is set above)


ARCHIVE = os.path.join(RUNS, "_archive")
LOG_ARCHIVE = os.path.join(ARCHIVE, "logs")


def archive_log(cell: str) -> None:
    """SNAPSHOT THE RUN LOG, NOT JUST THE SCORE.

    Archiving the result was half a fix. A cell's capacity numbers are computed from its `run.jsonl`,
    and the sweep TRUNCATES that file when it re-runs the cell — so an archived result kept pointing at
    whatever log happened to be in the directory later. MEASURED: this scoreboard printed dev-busy
    0.6893 / slots 0.5083 for BOTH the f9b6d2bd6 and a3fdfce02 rows of `baseline-n3-r0`, two runs 29
    minutes apart in wall-clock, because `capacity_of` took a cell NAME and read the live directory.
    F487 recorded f9b6d2bd6's occupancy as 0.7971; the board silently replaced it with the newer run's
    number and nothing objected.

    Keyed by the LOG's own mtime so re-runs accumulate rather than overwrite, and only a log carrying
    `run_finished` is kept — a partial log would archive a cell mid-flight and freeze half a run as if
    it were the whole one.
    """
    src = os.path.join(RUNS, cell, "run.jsonl")
    if not os.path.exists(src):
        return
    mt = int(os.path.getmtime(src))
    dest = os.path.join(LOG_ARCHIVE, f"{cell}-{mt}.jsonl")
    try:
        body = open(src, errors="replace").read()
    except OSError:
        return
    if '"run_finished"' not in body:
        return
    os.makedirs(LOG_ARCHIVE, exist_ok=True)
    if not os.path.exists(dest):
        with open(dest, "w") as fh:
            fh.write(body)
    # Deliberately OUTSIDE the "log already snapshotted" guard. The first version returned early when
    # the log copy existed, so on every cell already archived — which was all of them — the activity
    # digests were silently skipped and the recovery control read 0%. Two artefacts of one run get two
    # independent existence checks, or adding the second one never takes effect for the runs that
    # already have the first.
    archive_activity(cell, mt)


def archive_activity(cell: str, log_mtime: int) -> None:
    """THE ONLY SURVIVING RECORD OF THE TASKS THE EVENT LOG CANNOT SEE.

    A judge-terminated `task_completed` carries an empty `tool_calls` and a null `session_id`
    (F499/L305) — 5% to 30% of a cell's tasks, and systematically the hard ones. Their work is not
    gone, it is in `.swarm/activity/<task_id>.json`, which the dispatcher refreshes as the worker
    streams and which therefore survives an abort. MEASURED on baseline-n3-r0: the event log reports
    ZERO tool calls for `integrate-verify`; its digest holds 9, with the command text AND the output
    of each. `test-meridian-edge` — the 80-minute task that is that cell's whole critical path —
    shows 49,335 thinking characters against the join's 4,963, and three consecutive `write` calls to
    the same owned file.

    The digest is RICHER than the event record, which carries only name/is_mcp/ok. It is also per
    TASK rather than per attempt and is overwritten by each new attempt, so it describes the LAST
    attempt only — worth knowing before treating it as a full history.

    And the sweep wipes the cell directory when it reuses the slot, so none of this outlives the next
    unit unless something copies it. That is the whole reason this function exists: L293 said a bench
    that reuses a directory overwrites its own history, and the run log was only the half of that
    history I had already noticed.
    """
    src = os.path.join(RUNS, cell, ".swarm", "activity")
    if not os.path.isdir(src):
        return
    dest = os.path.join(LOG_ARCHIVE, f"{cell}-{log_mtime}-activity")
    if os.path.isdir(dest):
        return
    os.makedirs(dest, exist_ok=True)
    for f in os.listdir(src):
        if not f.endswith(".json"):
            continue
        try:
            with open(os.path.join(src, f), errors="replace") as fh:
                body = fh.read()
            with open(os.path.join(dest, f), "w") as fh:
                fh.write(body)
        except OSError:
            continue  # one unreadable digest must not cost the other forty-five


def log_for(cell: str, result_mtime: float):
    """The snapshot this ROW was actually measured from.

    A run log is finished before its result is scored, so the right snapshot is the newest one whose
    mtime is at or before the result's. Returning None is a real answer — it means the log was
    destroyed before anything captured it, and the honest response is to print nothing rather than the
    next run's numbers.
    """
    if not os.path.isdir(LOG_ARCHIVE):
        return None
    best = None
    for f in os.listdir(LOG_ARCHIVE):
        if not f.endswith(".jsonl"):
            continue
        name, _, stamp = f[:-6].rpartition("-")
        if name != cell or not stamp.isdigit():
            continue
        mt = int(stamp)
        if mt <= result_mtime + 1 and (best is None or mt > best[0]):
            best = (mt, os.path.join(LOG_ARCHIVE, f))
    return best[1] if best else None


def archive(cell: str, sha: str, result: dict, mtime: float) -> None:
    """SNAPSHOT EVERY RESULT, BECAUSE THE BENCH OVERWRITES ITS OWN HISTORY.

    The sweep re-runs a cell IN THE SAME DIRECTORY and replaces `nodeloop-result.json`. Measured: the
    0.9178 / 180-min cell that anchored a whole morning's comparison vanished mid-session, taking its
    commit from n=2 vs 2 down to n=1 vs 2 — the evidence for a claim deleted while the claim stood.

    Keyed by build_sha AND mtime, so a re-run on the same commit ACCUMULATES as a replicate rather
    than overwriting its predecessor. Replicates are the scarcest thing in this campaign: every
    outcome comparison so far has died on a within-arm spread measured at n=2.

    Archiving happens here rather than in the sweep on purpose — the sweep is a running interpreter
    and would not see the edit (L265). Reading the scoreboard is what preserves the data.
    """
    d = os.path.join(ARCHIVE, sha)
    os.makedirs(d, exist_ok=True)
    path = os.path.join(d, f"{cell}-{int(mtime)}.json")
    if not os.path.exists(path):
        with open(path, "w") as fh:
            json.dump(result, fh)


def archived() -> list[dict]:
    out = []
    if not os.path.isdir(ARCHIVE):
        return out
    for sha in sorted(os.listdir(ARCHIVE)):
        for f in sorted(os.listdir(os.path.join(ARCHIVE, sha))):
            try:
                r = json.load(open(os.path.join(ARCHIVE, sha, f)))
            except Exception:
                continue
            pool = r.get("actual_pool")
            if not pool or r.get("score") is None:
                continue
            cell, _, stamp = f[:-5].rpartition("-")
            out.append({"cell": cell, "sha": sha, "nodes": len(pool),
                        "quality": float(r["score"]),
                        "speed_min": (r.get("wall_secs") or 0) / 60.0,
                        "mtime": float(stamp) if stamp.isdigit() else 0.0,
                        "archived": True})
    return out


def cells() -> list[dict]:
    out = []
    for d in sorted(os.listdir(RUNS)):
        res = os.path.join(RUNS, d, "nodeloop-result.json")
        if not os.path.exists(res):
            continue
        try:
            r = json.load(open(res))
        except Exception:
            continue
        pool = r.get("actual_pool")
        if not pool or r.get("score") is None:
            continue  # never scored, or scored with no fleet — not a data point
        try:
            ev = shardshare.load(d)
            sha = shardshare.build_sha(ev)
        except SystemExit:
            sha = "?"
        mt = os.path.getmtime(res)
        archive(d, sha, r, mt)
        archive_log(d)
        out.append({
            "cell": d, "sha": sha, "nodes": len(pool),
            "quality": float(r["score"]), "speed_min": (r.get("wall_secs") or 0) / 60.0,
            "mtime": mt, "archived": False,
        })
    # Merge the archive in, de-duplicated on (sha, cell, mtime) so a live result and its own snapshot
    # never double-count into a mean.
    seen = {(r["sha"], r["cell"], int(r["mtime"])) for r in out}
    for a in archived():
        if (a["sha"], a["cell"], int(a["mtime"])) not in seen:
            out.append(a)
    return out


def capacity_of(log_path):
    """DEVICE-BUSY occupancy and SLOT utilisation — two different questions, both worth asking.

    Device-busy asks "was the machine doing anything"; slot utilisation asks "were its slots full". A
    1-node fleet reads 1.0000 device-busy while filling only ~0.86-0.95 of its slots, so the perfect
    number hides real headroom (F480).

    Slot utilisation was WITHDRAWN for several hours because the concurrency histogram reported three
    concurrent tasks on a two-slot fleet. It is restored only because that artefact was found and
    fixed — a judge kill ends an attempt, and nothing was closing the span there (F490). The
    `impossible_concurrency` guard is checked HERE on every read, so if it ever fires again the number
    is suppressed rather than printed: an impossible value must never reach a scoreboard twice.

    Takes a LOG PATH, never a cell name. Cell directories are reused and their logs truncated, so a
    name resolves to whichever run happens to be there now — which is how two rows 29 minutes apart
    came to print the same occupancy. `None` means the log for that row no longer exists, and that
    prints as SUPPRESSED.
    """
    if log_path is None:
        return None, None
    try:
        import occupancy as occ  # same directory, already on sys.path
        a = occ.analyse(log_path)
    except Exception:
        return None, None
    if a.get("impossible_concurrency"):
        return a.get("execute_occupancy"), None  # slot number suppressed, guard fired
    cs = a.get("concurrency_secs") or {}
    slots = a.get("slot_count")
    tot = sum(cs.values())
    util = (sum(int(k) * v for k, v in cs.items()) / (tot * slots)) if (tot and slots) else None
    return a.get("execute_occupancy"), util


def main() -> int:
    rows = cells()
    if not rows:
        print("no scored cells yet")
        return 2
    by_sha: dict[str, list[dict]] = {}
    for r in rows:
        by_sha.setdefault(r["sha"], []).append(r)

    print("=" * 78)
    print("GOAL: does MORE THAN ONE NODE buy both QUALITY and SPEED?")
    print("  QUALITY = build score (higher better)   SPEED = wall minutes (LOWER better)")
    print("=" * 78)

    # Newest commit last, so 'gradually improve' reads top-to-bottom.
    for sha in sorted(by_sha, key=lambda s: max(r["mtime"] for r in by_sha[s])):
        grp = by_sha[sha]
        print(f"\n--- build_sha {sha} ---")
        for r in sorted(grp, key=lambda x: (x["nodes"], x["cell"])):
            log = log_for(r["cell"], r["mtime"])
            occ, util = capacity_of(log)
            why = "LOG GONE" if log is None else "guard fired"
            print(f"  {r['cell']:<18} {r['nodes']}-node   quality {r['quality']:.4f}   "
                  f"speed {r['speed_min']:6.0f} min   "
                  f"dev-busy {occ if occ is not None else 'SUPPRESSED':<11} "
                  f"slots {f'{util:.4f}' if util is not None else f'SUPPRESSED ({why})'}")
        n1 = [r for r in grp if r["nodes"] == 1]
        nm = [r for r in grp if r["nodes"] > 1]
        if not (n1 and nm):
            print("  (no paired comparison in this commit — a lone arm proves nothing)")
            continue
        dq = mean(r["quality"] for r in nm) - mean(r["quality"] for r in n1)
        ds = mean(r["speed_min"] for r in nm) / mean(r["speed_min"] for r in n1)
        # AT n=1 THE WITHIN-ARM SPREAD IS UNMEASURABLE, NOT ZERO.
        #
        # `max - min` over a single cell is 0.0000, and printing that beside a verdict reads as "this
        # arm barely varies" when the truth is "nobody measured whether it varies". The guard below
        # then never fires, because no gap is <= 0 — so the ONE case that most needs the caveat is
        # exactly the case that silently loses it. That is L305 again: an absent measurement must not
        # be spelled the same way as a measured zero.
        #
        # The fallback bar is the spread this campaign has ACTUALLY observed for the same arm across
        # every other commit. It is enormous — the 3-node quality band alone runs 0.4548 to 0.9343 —
        # and a 1-vs-1 gap smaller than that is a direction whatever the verdict line says. Using
        # measured history rather than a chosen constant keeps the bar out of my hands.
        spread = (max(r["quality"] for r in nm) - min(r["quality"] for r in nm)
                  if len(nm) > 1 else None)
        same_arm = [r for r in rows if (r["nodes"] > 1) == (nm[0]["nodes"] > 1)]
        historical = (max(r["quality"] for r in same_arm) - min(r["quality"] for r in same_arm)
                      if len(same_arm) > 1 else None)
        shown = f"{spread:.4f}" if spread is not None else "UNMEASURABLE at n=1"
        print(f"  => QUALITY {dq:+.4f}   SPEED {ds:.2f}x   "
              f"(multi-node within-arm spread {shown}, n={len(nm)} vs {len(n1)})")
        # BOTH PILLARS OR IT IS NOT PROGRESS.
        verdict = ("BOTH PILLARS" if dq > 0 and ds < 1.0 else
                   "QUALITY ONLY — bought with time" if dq > 0 else
                   "SPEED ONLY — bought with quality" if ds < 1.0 else
                   "NEITHER")
        print(f"     VERDICT: {verdict}")
        bar, label = ((spread, "multi-node spread") if spread is not None
                      else (historical, "SAME-ARM spread measured across every other commit"))
        if bar is not None and abs(dq) <= bar:
            print(f"     ⚠ the quality gap ({dq:+.4f}) is INSIDE the {label} "
                  f"({bar:.4f}) — a DIRECTION, not a proven effect")
        # THE SAME TREATMENT FOR SPEED, because the directive names TWO pillars and a caveat on only
        # one of them produces exactly the half-checked headline it exists to prevent. Compared as a
        # RATIO against the same arm's own fastest-to-slowest ratio, since a gain in minutes and a
        # spread in minutes are not comparable across cells of different sizes.
        if len(same_arm) > 1:
            sp = [r["speed_min"] for r in same_arm if r["speed_min"]]
            if sp and min(sp) > 0:
                hist_ratio = max(sp) / min(sp)
                observed = max(ds, 1 / ds) if ds else 1.0
                if observed <= hist_ratio:
                    print(f"     ⚠ the speed gap ({ds:.2f}x) is INSIDE the same arm's own "
                          f"fastest-to-slowest ratio ({hist_ratio:.2f}x) — a DIRECTION, not a proven effect")
        if spread is None:
            print(f"     ⚠ n=1 in the multi-node arm: this commit measured NO within-arm variance. "
                  f"A byte-identical config has scored 44.2 / 86.7 / 90.0 on this fleet.")

    print_prediction_checks(rows)
    return 0


def print_prediction_checks(rows: list[dict]) -> None:
    """Run the registered prediction checks HERE, because this is the file the standing instruction
    names.

    Mihai's rule is "run goal.py every time a cell lands". A separate verdicts.py that I have to
    remember is a check that will be skipped on the night it matters — the same argument that put the
    hung-child scan into reap.py's default output rather than behind a flag (L309). Compact by
    design: one line per cell, with any FAIL expanded in full, so a green board stays readable and a
    red one cannot hide.
    """
    try:
        import verdicts
    except Exception as exc:  # noqa: BLE001 — a broken checker must be visible, never silently absent
        print(f"\n⚠ prediction checks unavailable: {exc}")
        return
    print("\n" + "=" * 78)
    print("PREDICTION CHECKS (INERT is not a pass; a cell predating a fix reads INERT, not FAIL)")
    print("=" * 78)
    seen = set()
    for r in sorted(rows, key=lambda r: r["mtime"]):
        log = log_for(r["cell"], r["mtime"])
        if not log or log in seen:
            continue
        seen.add(log)
        try:
            ev = [json.loads(l) for l in open(log) if l.strip()]
            a = verdicts.occ.analyse(log)
            checks = [
                ("F491 observed hint kept", verdicts.f491_hint_was_worth_keeping(ev, a)),
                ("F492 judge attempts timed", verdicts.f492_judge_attempts_report_time(ev, a)),
                ("F499 unmeasured == judged", verdicts.f499_unmeasured_is_the_judge_set(ev, a)),
                ("F500 invisible recovered", verdicts.f500_every_invisible_task_recovered(ev, a, log)),
                ("F501 rewrite not a pattern", verdicts.f501_rewrite_loop_is_not_a_pattern(ev, a)),
                ("F470 owns-nothing accepted", verdicts.f470_owns_nothing_sink_can_be_accepted(ev, a)),
                ("F474 post-drop is faster", verdicts.f474_a_dropped_body_costs_less_the_second_time(ev, a)),
                ("F497 plan is the ceiling", verdicts.f497_plan_is_still_the_ceiling(ev, a)),
            ]
        except Exception as exc:  # noqa: BLE001
            print(f"  {r['cell']:<18} ⚠ checks failed to run: {exc}")
            continue
        tally = {}
        for _, (v, _w) in checks:
            tally[v] = tally.get(v, 0) + 1
        summary = "  ".join(f"{k}×{n}" for k, n in sorted(tally.items()))
        print(f"  {r['cell']:<18} {r['sha']:<12} {summary}")
        for name, (v, why) in checks:
            if v == verdicts.FAIL:
                print(f"      🔴 {name}: {why}")


if __name__ == "__main__":
    sys.exit(main())
