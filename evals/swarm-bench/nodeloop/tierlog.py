#!/usr/bin/env python3
"""Preserve every cell's per-tier score breakdown, append-only.

F555 decomposed the composite score for the first time and found that tier B — behavioural
correctness, the heaviest weight at 0.30 — is the weakest tier in almost every cell, while tier A
(does it exist and run) sits 0.4 higher. F556 then read the check bodies and confirmed both tiers are
graded on comparable proportional scales, so the gap is real and not an artefact of harshness.

THAT FINDING RESTS ON FIVE CELLS WHEN THE CORPUS HOLDS FORTY-TWO REAL RUNS, and the reason is a
storage shape rather than anything about the engine. `nodeloop-result.json` holds ONE row per cell
NAME and is overwritten every time that name is re-run. `baseline-n3-r0` has been five different
runs; the file remembers the fifth. That is the same history-loss that silently destroyed the
campaign's best result in F538, and here it costs the tier decomposition 88% of its sample.

The fix is the shape loop.log already uses: APPEND-ONLY. Each completed unit gets one line, keyed by
the cell name AND its finish timestamp, so re-running a cell adds a row instead of replacing one.

WHY THIS IS A SEPARATE SCRIPT AND NOT A PATCH TO sweep.py: the sweep is RUNNING, and a running
interpreter does not see source edits — a patch would take effect only after a restart, which would
also throw away the in-flight cell. This is tick-driven instead: it scans for result rows it has not
recorded and appends them. At a 5-minute cadence against ~1.9-hour cells, no row can be overwritten
between passes. Patching sweep.py remains the durable answer whenever the sweep is next restarted.
"""
import json
import sys
from pathlib import Path

RUNS = Path("/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop")
LOG = RUNS / "tiers.jsonl"
EVENTLOGS = RUNS / "eventlogs"


def archive_eventlog(cell_dir: Path, cell: str, finished_at: str, dest=None) -> str | None:
    """Copy the run's event log somewhere the next same-named run cannot delete it.

    RESCUING FIELDS DOES NOT SCALE, AND THE FIELD-BY-FIELD VERSION ABOVE ONLY LOOKS SUFFICIENT UNTIL
    YOU COUNT THE READERS. EIGHTEEN instruments in this directory open `run.jsonl` — armcheck,
    bonusclass, curve, dispatch_audit, failures, goal, goalstate, occupancy, phases,
    prefix, reaudit, review, selftest, shardshare, suffixcost, sweep, tierlog, verdicts. Every one of
    them is silently answering its question over NINE cells while `loop.log` records 171 completed
    runs, because `run.jsonl` is overwritten per cell NAME. Each new question I think of would need
    its own rescue field, retrofitted after the evidence it needs is already gone.

    So archive the WHOLE log instead. Measured: 0.16 MB mean, 0.26 MB max, so all 171 runs cost about
    27 MB — nothing against a corpus that already holds full app trees. Every existing instrument can
    then be re-pointed at the archive and re-run over real history rather than over the last nine
    survivors.

    NEVER OVERWRITE. The filename carries the finish timestamp precisely so a re-run of a cell adds a
    file instead of replacing one; if the destination exists it is kept, because this whole mechanism
    exists to undo an overwrite and must not reintroduce one. Returns the archived filename, or None
    when the source log is already gone — absent, not zero, for the same reason `plan_signal` does.
    """
    src = cell_dir / "run.jsonl"
    if not src.is_file():
        return None
    (dest or EVENTLOGS).mkdir(parents=True, exist_ok=True)
    name = f"{cell}@{str(finished_at).replace(':', '-')}.jsonl"
    out = (dest or EVENTLOGS) / name
    if out.exists():
        return name
    try:
        out.write_bytes(src.read_bytes())
    except OSError:
        return None
    return name


def plan_signal(run_log: Path) -> dict:
    """The ladder + round-1 convergence verdict, rescued before the next run overwrites it.

    WHY THIS LIVES HERE. `run.jsonl` is overwritten per cell NAME exactly like the result file was.
    MEASURED: 171 [done] rows in loop.log, 118 result dirs, and only NINE surviving run.jsonl. So
    every event-derived question is capped at nine cells forever, and the one that matters right now
    — does the confidence ladder BUY the quality it costs 35 minutes for — came back `n=1 vs 5, SE
    UNMEASURABLE`. The engine parked a real change on that question with the words "measure first",
    and the archive cannot answer it at any sample size because the evidence is deleted, not absent.
    tierlog already runs on a 5-minute tick against ~1.9-hour cells, so it sees each `run.jsonl`
    while it still exists. This makes the sample grow from here instead of staying at nine.

    ROUND ONE, NOT THE LAST ROUND. A laddering cell emits one `plan_convergence` per draft round, and
    it is the FIRST that decides whether the ladder fires at all. Reading the last one is a different
    question and quietly answers it: on baseline-n3-r0 round 1 is conf 69 / best2 88 / penalty 19 —
    the numbers that bought the ladder — while round 3 is 68 / 81 / 13. Taking the last would have
    understated the pool penalty by six points and pointed at a decision that was never made.

    ABSENT IS `None`, NEVER 0. A cell whose `run.jsonl` is already gone must be distinguishable from
    a cell that genuinely never laddered, or the overwrite silently manufactures evidence for the
    cheaper answer — L340 in the data model rather than in a sentence.

    THE LADDER COLUMNS STAY, on a build that has no ladder, because this function's whole job is
    reading ARCHIVED logs and half the archive predates the linear engine. `backfill` gates its write
    on re-deriving `ladder` from those logs and matching what the row already carries; dropping the
    counter would fail that gate on every historical row and refuse every repair. What is added
    instead is `engine`, so a zero can never be mistaken for a mechanism that stopped firing, plus
    the linear engine's own equivalents: REVIEW rounds, plan patches, and the opener's cut.
    """
    out = {"ladder": None, "retarget_discarded": None, "draft_rounds": None, "conv1": None,
           "engine": None, "review_rounds": None, "plan_patches": None, "slices": None}
    if not run_log.is_file():
        return out
    lad = disc = rounds = 0
    reviews = patches = 0
    conv1 = None
    slices = None
    linear = False
    try:
        text = run_log.read_text(errors="replace")
    except OSError:
        return out
    for line in text.splitlines():
        if '"confidence_retarget"' in line:
            lad += 1
        elif '"retarget_discarded"' in line:
            disc += 1
        elif '"skeleton_drafts"' in line:
            rounds += 1
        elif '"review_findings"' in line:
            reviews += 1
        elif '"plan_patched"' in line:
            patches += 1
        elif '"event": "phase"' in line or '"event":"phase"' in line:
            linear = True
        elif '"slices_opened"' in line and slices is None:
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            slices = {k: e.get(k) for k in ("count", "weights", "secs")}
        elif '"plan_convergence"' in line and conv1 is None:
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            conv1 = {k: e.get(k) for k in
                     ("drafts", "agreement_conf", "agreement_best2", "pool_penalty",
                      "struct_conv", "struct_stop", "enforced", "would_skip_ladder")}
    return {"ladder": lad, "retarget_discarded": disc, "draft_rounds": rounds, "conv1": conv1,
            # WHICH ENGINE WROTE THIS LOG, decided from the log itself. Without it the ladder columns
            # go quietly to 0 on every linear-engine run and a later analysis pooling old rows with
            # new ones reads a pile of honest zeros as "the ladder stopped firing" — the pooling
            # error this campaign keeps paying for, one field away from being unavoidable. A `phase`
            # event exists only on the linear engine, so it is the marker.
            "engine": "linear" if linear else "ladder",
            "review_rounds": reviews, "plan_patches": patches, "slices": slices}


def existing_keys(path=LOG) -> set:
    """(cell, finished_at) pairs already recorded. The timestamp is half the key on purpose: without
    it a re-run of the same cell would look like a duplicate and be dropped, which is precisely the
    overwrite this file exists to undo."""
    keys = set()
    if not path.exists():
        return keys
    for line in path.read_text(errors="replace").splitlines():
        if not line.strip():
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        keys.add((r.get("cell"), r.get("finished_at")))
    return keys


def repair_plan_signal(log=LOG, eventlogs=None, archives=None) -> dict:
    """Recover `ladder`/`conv1` for rows whose `run.jsonl` was gone WHEN THEY WERE RECORDED.

    `backfill` protects the LOG; it does not repair the ROW. A row harvested before its event log
    was archived keeps `ladder: None` for ever, and F645 then read those Nones as "did not ladder"
    and published a collinearity that did not exist. The data was on disk the whole time.

    THE CONTROL IS NOT OPTIONAL AND IT GATES THE WRITE. Rows that ALREADY carry a ladder count are
    re-derived from their archived log first; if a single one disagrees, NOTHING is written and the
    disagreement is returned. A repair that silently rewrites history on an unvalidated derivation is
    worse than the gap it closes. Measured when this was added: 5 of 5 agreed exactly (1,0,2,0,1).

    Absent stays absent. A row with no archived log keeps `None`, because L340 in the data model is
    the whole point of the field.
    """
    dirs = [eventlogs if eventlogs is not None else EVENTLOGS]
    if archives is None:
        archives = [RUNS / "_archive" / "logs"]
    dirs += list(archives)

    def _find(cell, fin) -> Path | None:
        exact = f"{cell}@{str(fin).replace(':', '-')}.jsonl"
        for d in dirs:
            p = Path(d) / exact
            if p.is_file():
                return p
        want = _epoch(fin)
        best, gap = None, None
        for d in dirs:
            if not Path(d).is_dir():
                continue
            for p in Path(d).glob("*.jsonl"):
                if not p.name.startswith(f"{cell}@") and not p.name.startswith(f"{cell}-"):
                    continue
                got = _log_finish_epoch(p)
                if got is None or want is None:
                    continue
                delta = abs(got - want)
                if delta <= 240 and (gap is None or delta < gap):
                    best, gap = p, delta
        return best

    if not Path(log).exists():
        return {"checked": 0, "agreed": 0, "repaired": 0, "disagreements": []}
    rows, changed, agreed, disagree = [], 0, 0, []
    for line in Path(log).read_text(errors="replace").splitlines():
        if not line.strip():
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        rows.append(r)
    for r in rows:
        src = _find(r.get("cell"), r.get("finished_at"))
        if src is None:
            continue
        got = plan_signal(src)
        if r.get("ladder") is not None:
            if got["ladder"] == r["ladder"]:
                agreed += 1
            else:
                disagree.append({"cell": r.get("cell"), "recorded": r["ladder"], "derived": got["ladder"]})
    if disagree:
        return {"checked": len(rows), "agreed": agreed, "repaired": 0, "disagreements": disagree}
    for r in rows:
        if r.get("ladder") is not None:
            continue
        src = _find(r.get("cell"), r.get("finished_at"))
        if src is None:
            continue
        r.update(plan_signal(src))
        r["ladder_source"] = "archive"
        changed += 1
    if changed:
        Path(log).write_text("\n".join(json.dumps(r) for r in rows) + "\n")
    return {"checked": len(rows), "agreed": agreed, "repaired": changed, "disagreements": []}


def _epoch(s):
    import datetime as _dt
    try:
        return _dt.datetime.fromisoformat(str(s).replace("Z", "+00:00")).timestamp()
    except (ValueError, TypeError):
        return None


def _log_finish_epoch(p: Path):
    """`run_finished.ts` is UTC-with-offset; tier rows carry LOCAL time. Parse through the offset —
    subtracting a constant reintroduces the three-hour error that made an earlier join match 1 of 43."""
    import datetime as _dt
    try:
        for line in p.read_text(errors="replace").splitlines():
            if '"run_finished"' not in line:
                continue
            ts = json.loads(line).get("ts")
            if ts:
                return _dt.datetime.fromisoformat(ts).timestamp()
    except (OSError, ValueError, json.JSONDecodeError):
        return None
    try:
        return p.stat().st_mtime
    except OSError:
        return None


def harvest(runs=RUNS, log=LOG, eventlogs=None) -> list:
    """Append any result carrying a `tiers` block that is not already recorded."""
    seen = existing_keys(log)
    added = []
    for p in sorted(runs.glob("*/nodeloop-result.json")):
        try:
            j = json.loads(p.read_text())
        except (json.JSONDecodeError, OSError):
            continue
        if not j.get("tiers"):
            continue
        key = (p.parent.name, j.get("finished_at"))
        if key in seen:
            continue
        # `void` and `wall_secs` ride along so the reader can apply the same real-run filter the rest
        # of the campaign uses, rather than re-deriving it from a second file.
        row = {"cell": p.parent.name, "finished_at": j.get("finished_at"),
               "arm": j.get("arm"), "nodes": j.get("nodes"), "rep": j.get("rep"),
               "score": j.get("score"), "wall_secs": j.get("wall_secs"),
               "void": bool(j.get("void")), "engine_build": j.get("engine_build"),
               "scorer_version": j.get("scorer_version"),
               "tiers": {k: v.get("mean") for k, v in (j.get("tiers") or {}).items()},
               **plan_signal(p.parent / "run.jsonl"),
               "eventlog": archive_eventlog(p.parent, p.parent.name, j.get("finished_at"),
                                            eventlogs if eventlogs is not None else EVENTLOGS)}
        added.append(row)
        seen.add(key)
    if added:
        with open(log, "a") as f:
            for r in added:
                f.write(json.dumps(r) + "\n")
    return added


def backfill(runs=RUNS, log=LOG, eventlogs=None) -> list:
    """Archive the event logs of cells ALREADY recorded, before the next same-named run deletes them.

    THE FIX ABOVE WAS PROSPECTIVE ONLY AND THAT IS NOT GOOD ENOUGH. `archive_eventlog` runs when a
    row is first harvested, so the nine `run.jsonl` still on disk — every cell this campaign can
    currently reason about — are already past that point and stay unprotected. `loop.log` names
    `baseline-n1-r1` as the NEXT unit, and that cell scored 0.9650, the best result of the frozen era.
    Its event log would have been overwritten within the hour by a fix that had just been written to
    prevent exactly that.

    ATTRIBUTION IS THE ONLY SUBTLE PART, AND I GOT IT WRONG ON THE FIRST RUN OF THIS FUNCTION. A cell
    dir holds the LATEST run's `run.jsonl`, so it belongs to that cell's row with the greatest
    `finished_at`, never to an older one. Filing it under an earlier timestamp silently attaches one
    run's events to a different run's score, which is worse than losing it — a wrong row cannot be
    detected later, a missing one can.

    THE CASE I MISSED IS THE CELL THAT IS STILL RUNNING. `baseline-n3-r1` was mid-flight when this
    first executed: its `run.jsonl` was 11 KB and growing, and the newest RECORDED row for that name
    had finished hours earlier. So the backfill archived a partial, in-flight log under a completed
    run's timestamp — committing, within two minutes, the precise corruption the paragraph above
    warns against. The guard is mtime: if the log has been written to since the row it would be filed
    under finished, it belongs to a LATER run and must be skipped, not guessed at. A live cell is
    picked up normally by `harvest` once it writes its own result row.
    """
    import datetime as _dt

    def _fin_epoch(s):
        try:
            return _dt.datetime.fromisoformat(str(s).replace("Z", "+00:00")).timestamp()
        except (ValueError, TypeError):
            return None

    if not log.exists():
        return []
    latest: dict = {}
    for line in log.read_text(errors="replace").splitlines():
        if not line.strip():
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        c, f = r.get("cell"), r.get("finished_at")
        if c and f and (c not in latest or f > latest[c]):
            latest[c] = f
    done = []
    for cell, fin in sorted(latest.items()):
        d = runs / cell
        src = d / "run.jsonl"
        if not src.is_file():
            continue
        # STILL RUNNING, OR ALREADY REPLACED: the log has been touched since the row it would be
        # filed under finished, so it is a LATER run's log. Skip rather than misattribute.
        fe = _fin_epoch(fin)
        if fe is not None and src.stat().st_mtime > fe + 120:
            continue
        name = archive_eventlog(d, cell, fin, eventlogs if eventlogs is not None else EVENTLOGS)
        if name:
            done.append(name)
    return done


def report(log=LOG) -> str:
    if not log.exists():
        return "no tiers.jsonl yet — run `tierlog.py` once to seed it"
    rows = [json.loads(l) for l in log.read_text().splitlines() if l.strip()]
    real = [r for r in rows if not r["void"] and (r.get("wall_secs") or 0) >= 1800]
    L = [f"TIER HISTORY  {len(rows)} row(s), {len(real)} real (non-void, >=30 min)"]
    if not real:
        return "\n".join(L)
    L.append(f"  {'arm':<12}{'nodes':>6}{'n':>4}{'score':>8}{'A run':>8}{'B behav':>9}"
             f"{'C vendor':>10}{'D craft':>9}")
    groups: dict = {}
    for r in real:
        groups.setdefault((r["arm"], r["nodes"]), []).append(r)
    for (arm, nodes), v in sorted(groups.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)):
        m = lambda k: sum((x["tiers"].get(k) or 0) for x in v) / len(v)  # noqa: E731
        s = sum(x["score"] for x in v) / len(v)
        L.append(f"  {arm:<12}{str(nodes):>6}{len(v):>4}{s:>8.3f}{m('A'):>8.3f}{m('B'):>9.3f}"
                 f"{m('C'):>10.3f}{m('D'):>9.3f}")
    return "\n".join(L)


def self_test() -> int:
    """The property that matters: a RE-RUN of the same cell must add a row, never replace one."""
    import tempfile
    fails = []
    d = Path(tempfile.mkdtemp())
    log = d / "t.jsonl"
    cell = d / "baseline-n3-r0"
    cell.mkdir()
    res = cell / "nodeloop-result.json"

    def write(score, when):
        res.write_text(json.dumps({"arm": "baseline", "nodes": 3, "rep": 0, "score": score,
                                   "wall_secs": 6000, "finished_at": when,
                                   "tiers": {"A": {"mean": 0.9}, "B": {"mean": 0.4},
                                             "C": {"mean": 0.5}, "D": {"mean": 0.6}}}))

    ev = d / "eventlogs"
    write(0.9033, "2026-08-08T10:55:28")
    if len(harvest(d, log, ev)) != 1:
        fails.append("first harvest did not record the row")
    if harvest(d, log, ev):
        fails.append("re-harvesting an unchanged result duplicated it")

    # THE WHOLE POINT: the same cell re-run must not overwrite its predecessor.
    write(0.0561, "2026-08-08T11:34:29")
    if len(harvest(d, log, ev)) != 1:
        fails.append("a RE-RUN of the same cell was not recorded — the overwrite survives")
    rows = [json.loads(l) for l in log.read_text().splitlines() if l.strip()]
    if len(rows) != 2 or {r["score"] for r in rows} != {0.9033, 0.0561}:
        fails.append(f"history not preserved: {[r.get('score') for r in rows]}")

    # A result with no tiers block must be skipped rather than recorded as zeros.
    res.write_text(json.dumps({"arm": "baseline", "nodes": 3, "finished_at": "x", "score": 0.5}))
    if harvest(d, log, ev):
        fails.append("a result with no tiers block was recorded anyway")

    # THE PLAN SIGNAL. Absent evidence must read as None, never as a confident zero — a cell whose
    # run.jsonl was already overwritten must not be recorded as a cell that did not ladder.
    rows = [json.loads(l) for l in log.read_text().splitlines() if l.strip()]
    if any(r.get("ladder") is not None for r in rows):
        fails.append("a missing run.jsonl was recorded as a real ladder count instead of None")
    if any(r.get("eventlog") is not None for r in rows):
        fails.append("a missing run.jsonl produced an eventlog name instead of None")

    # And with a run.jsonl present it must count the ladder and keep ROUND ONE's convergence.
    (cell / "run.jsonl").write_text("\n".join([
        json.dumps({"event": "skeleton_drafts", "requested": 3}),
        json.dumps({"event": "plan_convergence", "agreement_conf": 69, "agreement_best2": 88,
                    "pool_penalty": 19, "would_skip_ladder": True}),
        json.dumps({"event": "confidence_retarget"}),
        json.dumps({"event": "retarget_discarded"}),
        json.dumps({"event": "skeleton_drafts", "requested": 3}),
        json.dumps({"event": "plan_convergence", "agreement_conf": 68, "agreement_best2": 81,
                    "pool_penalty": 13, "would_skip_ladder": True}),
    ]))
    write(0.7, "2026-08-08T12:00:00")
    new = harvest(d, log, ev)
    if len(new) != 1:
        fails.append("the run with a plan signal was not recorded")
    else:
        r = new[0]
        if r["ladder"] != 1 or r["retarget_discarded"] != 1 or r["draft_rounds"] != 2:
            fails.append(f"ladder counts wrong: {r['ladder']}/{r['retarget_discarded']}/{r['draft_rounds']}")
        if (r["conv1"] or {}).get("pool_penalty") != 19:
            fails.append(f"kept the LAST plan_convergence, not round one: {r.get('conv1')}")
        if r.get("engine") != "ladder":
            fails.append(f"a ladder-era log must be marked engine=ladder, got {r.get('engine')}")

        # THE ARCHIVE: byte-identical, and a RE-RUN of the same cell must add a file, never replace.
        arch = ev / (r.get("eventlog") or "__missing__")
        if not arch.is_file():
            fails.append(f"event log was not archived: {r.get('eventlog')}")
        elif arch.read_bytes() != (cell / "run.jsonl").read_bytes():
            fails.append("archived event log does not match the source bytes")
        before = sorted(p.name for p in ev.glob("*.jsonl"))
        (cell / "run.jsonl").write_text(json.dumps({"event": "run_started"}))
        write(0.8, "2026-08-08T13:00:00")
        harvest(d, log, ev)
        after = sorted(p.name for p in ev.glob("*.jsonl"))
        if len(after) != len(before) + 1:
            fails.append(f"a re-run did not ADD an archive: {before} -> {after}")
        if arch.read_bytes() == (cell / "run.jsonl").read_bytes():
            fails.append("the earlier archive was overwritten — the mechanism reintroduced the bug it undoes")

    # AND THE OTHER VINTAGE. A linear-engine log has no ladder to count, and the honest zeros that
    # produces must be LABELLED — otherwise a later analysis pools them with ladder-era rows and reads
    # "the ladder stopped firing" off a mechanism that was deleted. Placed after the archive checks
    # rather than inside them: it rewrites `cell/run.jsonl`, and running it earlier would hand those
    # checks a different log than the one they harvested.
    (cell / "run.jsonl").write_text("\n".join([
        json.dumps({"event": "phase", "phase": "open"}),
        json.dumps({"event": "slices_opened", "count": 3, "weights": [3, 2, 2], "secs": 41}),
        json.dumps({"event": "phase", "phase": "research"}),
        json.dumps({"event": "review_findings", "round": 1, "new": 2, "patch_touches": 2}),
        json.dumps({"event": "plan_patched", "round": 1, "replace": 2, "add": 0, "remove": 0}),
        json.dumps({"event": "review_findings", "round": 2, "new": 0, "patch_touches": 0}),
    ]))
    write(0.8, "2026-08-08T14:00:00")
    lin = harvest(d, log, ev)
    if len(lin) != 1:
        fails.append("the linear-engine run was not recorded")
    else:
        lr = lin[0]
        if lr.get("engine") != "linear":
            fails.append(f"a `phase` event must mark the log as engine=linear, got {lr.get('engine')}")
        if lr.get("review_rounds") != 2 or lr.get("plan_patches") != 1:
            fails.append(f"review loop miscounted: {lr.get('review_rounds')}/{lr.get('plan_patches')}")
        if (lr.get("slices") or {}).get("weights") != [3, 2, 2]:
            fails.append(f"the opener's cut was not rescued: {lr.get('slices')}")
        if lr.get("ladder") != 0 or lr.get("conv1") is not None:
            fails.append("a linear log must report ladder 0 and no convergence, not None/garbage")

    for f in fails:
        print(f"  FAIL {f}")
    print(f"tierlog self-test: {'PASS' if not fails else str(len(fails)) + ' FAILURES'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(self_test())
    saved = backfill()
    if saved:
        print(f"backfilled {len(saved)} event log(s) that were one re-run from deletion: "
              + ", ".join(saved))
    new = harvest()
    if new:
        print(f"recorded {len(new)} new row(s): " +
              ", ".join(f"{r['cell']}@{r['finished_at']}" for r in new))
    print(report())
