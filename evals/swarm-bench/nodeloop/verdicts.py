#!/usr/bin/env python3
"""Evaluate every mechanically-checkable registered prediction against a cell, automatically.

Mihai, 22:20: "don't go over bad behaviors, taking note and ignoring." F503/L309 answered that for
two pathologies by shipping detectors. This answers the wider version of the same criticism: the
PREDICTIONS register has thirteen open entries and every one of them was being checked BY HAND, which
is why several sat open for hours after the evidence to settle them had already landed on disk.

THREE OUTCOMES, KEPT STRICTLY APART, because collapsing them is how this campaign has published
findings it later retracted:

    PASS   — the predicate was testable on this cell and held
    FAIL   — it was testable and did not hold
    INERT  — the PRECONDITION never occurred, so the cell says NOTHING about it

INERT IS NOT A PASS. A tool that prints two colours will eventually be read as if the third did not
exist, so INERT is spelled out with the reason its precondition was missing.

Each check names the finding it belongs to and states its own falsifier inline, so a reader can see
what would have made it fail without going back to the register.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import occupancy as occ  # noqa: E402
import shardshare  # noqa: E402

RUNS = "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop"
LOG_ARCHIVE = os.path.join(RUNS, "_archive", "logs")

PASS, FAIL, INERT = "PASS", "FAIL", "INERT"


def f491_hint_was_worth_keeping(ev, a) -> tuple[str, str]:
    """A cap-exhausted `observed` problem verdict carrying a real hint — the thing F491 stopped
    discarding. FALSIFIER: none exists, which makes the fix unexercised on this cell."""
    kills = {}
    hits = []
    for e in ev:
        if e.get("event") != "judge_verdict":
            continue
        t = str(e.get("task_id"))
        if e.get("action") == "re_dispatch":
            kills[t] = kills.get(t, 0) + 1
        elif (e.get("action") == "observed" and kills.get(t, 0) >= 2
              and (e.get("hint") or "").strip() and e.get("verdict") not in ("ok", "accept")):
            hits.append(f"{t}:{e.get('verdict')}")
    if not hits:
        return INERT, "no cap-exhausted observed verdict carried a hint — the fix had nothing to keep"
    return PASS, f"{len(hits)} hint(s) the old engine would have discarded: {', '.join(hits[:3])}"


F492_FIX = "5ed189bcf"  # the commit that populates elapsed_ms on the judge paths


def carries(fix_sha: str, cell_sha: str) -> bool:
    """Does the binary this cell ran actually CONTAIN the fix being checked?

    Without this every pre-fix cell shows a permanent red FAIL, and a check that is always red stops
    being read — the same failure as one that is always green. A cell that predates a fix cannot
    falsify it; the honest verdict is INERT, and the reason is provenance rather than a missing
    precondition.
    """
    import subprocess
    if not cell_sha or cell_sha == "?":
        return False
    cell_sha = cell_sha.split("-")[0]  # a dirty build is still that commit's tree
    try:
        return subprocess.run(["git", "-C", "/Users/mihaiperdum/Projects/goose", "merge-base",
                               "--is-ancestor", fix_sha, cell_sha],
                              capture_output=True, timeout=15).returncode == 0
    except Exception:
        return False


def f492_judge_attempts_report_time(ev, a) -> tuple[str, str]:
    """A judge-terminated completion must report real elapsed_ms. FALSIFIER: any of them reads 0."""
    sha = shardshare.build_sha(ev)
    if not carries(F492_FIX, sha):
        return INERT, f"this cell ran {sha}, which predates the fix {F492_FIX} — it cannot falsify it"
    jt = [e for e in ev if e.get("event") == "task_completed" and not e.get("session_id")
          and not (e.get("tool_calls") or [])]
    if not jt:
        return INERT, "no judge-terminated completion in this cell"
    zero = [str(e.get("task_id")) for e in jt if (e.get("elapsed_ms") or 0) == 0]
    if zero:
        return FAIL, f"{len(zero)}/{len(jt)} report elapsed_ms 0: {', '.join(zero[:4])}"
    return PASS, f"all {len(jt)} judge-terminated completions report real time"


def f499_unmeasured_is_the_judge_set(ev, a) -> tuple[str, str]:
    """`unmeasured_tasks` must be exactly the judge-ended set — the four-signal agreement that
    justified the detector. FALSIFIER: the two sets differ, meaning the detector drifted."""
    um = set(a.get("unmeasured_tasks") or [])
    judged = {str(e.get("task_id")) for e in ev if e.get("event") == "judge_verdict"
              and e.get("action") in ("accepted", "failed")}
    if not um and not judged:
        return INERT, "no judge-terminated task — nothing to agree about"
    if um != judged:
        return FAIL, f"unmeasured={sorted(um)} but judge-ended={sorted(judged)}"
    return PASS, f"{len(um)} task(s), both signals agree"


def activity_dir_for(log_path: str) -> str | None:
    """Where this log's per-task digests live, for an ARCHIVED snapshot or a LIVE cell.

    Digests sit in two different places depending on which the caller passed, and this check knew
    only the archived one. Pointing it at a live cell therefore reported "NO archived digests at
    all" — a FAIL describing the dispatcher as broken when the real difference was the shape of the
    path I typed. MEASURED: it fired on baseline-n3-r0 while that cell's own `.swarm/activity` held
    43 digests and its archived sibling held a full copy.

    A check that fails on its INPUT SHAPE rather than on the property it tests is a false-finding
    generator, which is the whole of L332. Returning None here lets the caller say "I could not
    look" instead of "I looked and it was empty" — the distinction the PASS/FAIL/INERT split exists
    to preserve.
    """
    archived = log_path[:-6] + "-activity"          # _archive/logs/<cell>-<epoch>-activity
    if os.path.isdir(archived):
        return archived
    live = os.path.join(os.path.dirname(log_path), ".swarm", "activity")
    return live if os.path.isdir(live) else None


def f500_every_invisible_task_recovered(ev, a, log_path) -> tuple[str, str]:
    """Every task the event log cannot see must have a digest. FALSIFIER: one has none — which would
    mean the dispatcher never wrote it, a worse defect than the one this recovered."""
    um = a.get("unmeasured_tasks") or []
    if not um:
        return INERT, "no invisible tasks in this cell"
    act = activity_dir_for(log_path)
    if act is None:
        return INERT, (f"{len(um)} invisible task(s) but no digest directory exists for this log — "
                       "an un-archived live cell reads identically to a broken dispatcher here, so "
                       "this says nothing either way")
    missing = [t for t in um if not os.path.exists(os.path.join(act, f"{t}.json"))]
    if missing:
        return FAIL, f"{len(missing)}/{len(um)} have no digest in {act}: {', '.join(missing[:4])}"
    return PASS, f"{len(um)}/{len(um)} recovered from {os.path.basename(act)}"


def f501_rewrite_loop_is_not_a_pattern(ev, a) -> tuple[str, str]:
    """At most ONE task per cell may reach 3+ repeated writes. FALSIFIER: two or more — which would
    make it a pattern and would justify the lever F501 declined to build on n=1."""
    p = a.get("pathologies") or {}
    if not p.get("digests"):
        return INERT, "no archived digests — the pathology scan cannot run"
    bad = [r for r in p.get("rewrite_loops", []) if r["repeats"] >= 3]
    if len(bad) >= 2:
        return FAIL, ("PATTERN, not an outlier — the lever is now justified: "
                      + ", ".join(f"{r['task']}({r['repeats']})" for r in bad))
    return PASS, (f"{len(bad)} task(s) at 3+ repeats" +
                  (f" ({bad[0]['task']})" if bad else "") + " — still an outlier, not a pattern")


def f470_owns_nothing_sink_can_be_accepted(ev, a) -> tuple[str, str]:
    """The owns-nothing Accept branch: a join that has acted and gone quiet should be ACCEPTED rather
    than left to a timer. FALSIFIER: the sink owns nothing, was judged, and the only verdicts it ever
    drew were non-acting — meaning the branch is still unreachable in practice.

    F486 is why the first gate is the sink's own `files`: the planner hands it a deliberable roughly a
    third of the time, and in those cells every owns-nothing fix is INERT BY CONSTRUCTION. Reporting
    that as a pass would be reporting a fix that could not have run.
    """
    pl = [e for e in ev if e.get("event") == "plan_loaded"]
    if not pl:
        return INERT, "no plan_loaded"
    sink = next((t for t in (pl[-1].get("tasks") or []) if t.get("id") == "integrate-verify"), None)
    if sink is None:
        return INERT, "no integrate-verify in this plan"
    owned = sink.get("files") or []
    if owned:
        return INERT, f"the planner gave the sink {owned} — every owns-nothing branch is disarmed (F486)"
    verdicts_ = [e for e in ev if e.get("event") == "judge_verdict"
                 and e.get("task_id") == "integrate-verify"]
    if not verdicts_:
        return INERT, "the sink owns nothing but was never judged"
    if any(v.get("action") == "accepted" for v in verdicts_):
        return PASS, f"owns-nothing sink ACCEPTED after {len(verdicts_)} verdict(s)"
    seen = ", ".join(sorted({str(v.get("verdict")) for v in verdicts_}))
    return INERT, (f"sink owns nothing and drew {len(verdicts_)} verdict(s) ({seen}) but never went "
                   f"quiet long enough to reach the branch")


def f474_a_dropped_body_costs_less_the_second_time(ev, a) -> tuple[str, str]:
    """After a mid-stream body drop the surviving attempt should be FASTER, because the hint tells it
    the earlier work is still on disk. FALSIFIER: the post-drop attempt takes longer than the one it
    replaced."""
    drops = [e for e in ev if e.get("event") == "task_retry"
             and "mid-stream body drop" in str(e.get("error", ""))]
    if not drops:
        return INERT, "no mid-stream body drop in this cell"
    spans: dict[str, list] = {}
    for s in sorted(a.get("_spans") or [], key=lambda s: s["start"]):
        spans.setdefault(s["task"], []).append(s["end"] - s["start"])
    out = []
    for d in drops:
        t = str(d.get("task_id"))
        ds = spans.get(t) or []
        if len(ds) < 2:
            continue
        out.append((t, ds[-2] / 60, ds[-1] / 60))
    if not out:
        return INERT, "a drop occurred but the replaced attempt has no measurable span"
    worse = [o for o in out if o[2] > o[1]]
    detail = "; ".join(f"{t}: {b:.1f}m → {c:.1f}m" for t, b, c in out)
    return (FAIL, f"the post-drop attempt was SLOWER — {detail}") if worse else (PASS, detail)


F511_FIX = "a29a4399e"  # the worker-prompt rule forbidding a blocking server inside a test


def f511_no_test_author_stalled(ev, a) -> tuple[str, str]:
    """No test-authoring task may draw an "agent stalled" retry.

    That message is what the 420s no-progress watchdog emits, and F502 caught its cause live: a test
    called the app's blocking server entry point, pytest hung before printing a line, the worker
    produced no tokens, and the attempt was discarded. Measured three times across two cells, costing
    two thrown-away attempts and ~36 minutes on one of them.

    FALSIFIER: one test-author task with a stall retry. Checked retrospectively from the log rather
    than from a live process scan, so it works on any archived cell — reap.py's HUNG MID-RUN section
    only ever sees the machine as it is right now.

    INERT is effectively unavailable here, which is unusual and is the point: every cell this
    campaign has run contains test-authoring tasks, so an absence of stalls on a cell that HAS them
    is a genuine pass rather than a precondition that never occurred.
    """
    sha = shardshare.build_sha(ev)
    if not carries(F511_FIX, sha):
        return INERT, f"this cell ran {sha}, which predates the rule {F511_FIX} — it cannot falsify it"
    pl = [e for e in ev if e.get("event") == "plan_loaded"]
    tasks = {str(t.get("id")): (t.get("files") or []) for t in (pl[-1].get("tasks") or [])} if pl else {}

    def authors_tests(tid: str) -> bool:
        return tid.startswith("test-") or tid.startswith("test::") or any(
            "test" in str(f).rsplit("/", 1)[-1] for f in tasks.get(tid, []))

    if not any(authors_tests(t) for t in tasks):
        return INERT, "no test-authoring task in this plan — the rule had no addressee"
    stalled = [str(e.get("task_id")) for e in ev
               if e.get("event") == "task_retry" and "stalled" in str(e.get("error", ""))
               and authors_tests(str(e.get("task_id")))]
    if stalled:
        return FAIL, f"test-author task(s) still stalled: {', '.join(sorted(set(stalled)))}"
    n = sum(1 for t in tasks if authors_tests(t))
    return PASS, f"{n} test-authoring task(s), none stalled"


F517_FIX = "d91fd8b96"          # the commit that bakes spec_repair ON
F517_SERIAL_WORST_SECS = 1410   # 23.5 min — the worst post-execute suffix the SERIAL path ever produced


def f517_raced_repair_fires_and_never_regresses(ev, a) -> tuple[str, str]:
    """`spec_repair` must fire on a multi-node repair round, and must never make the app worse.

    Three things are checked, and the SECOND is the one that matters most:

      1. FIRES. On a fleet with more than one model that actually entered repair, a
         `spec_repair_wave` must appear. Its absence would mean the baked default is not reaching the
         code path — the same class of defect as a lever that is on but dead.
      2. NEVER REGRESSES. `pick_repair_winner` promotes only when a twin's re-verified finding count
         is STRICTLY below the count that opened the round, so `winner_findings < baseline_findings`
         must hold on EVERY promoted wave. This is the whole safety argument for racing N writers at
         one tree; if it ever fails, racing is unsafe and the lever must come straight back off.
      3. NOT SLOWER. Where it fires, the post-execute suffix must not exceed 1410s — the worst the
         serial path ever produced (F516: 23.5 min on baseline-n1-r2).

    INERT is genuine here in two ways that must not be confused with a pass: a 1-node fleet cannot
    race (the guard requires len() > 1, deliberately), and a cell that never entered repair had
    nothing to race. Both are reported with their reason.
    """
    sha = shardshare.build_sha(ev)
    if not carries(F517_FIX, sha):
        return INERT, f"this cell ran {sha}, which predates the bake {F517_FIX} — it cannot falsify it"
    waves = [e for e in ev if e.get("event") == "spec_repair_wave"]
    entered_repair = any(e.get("event") == "complete_fix_dispatched" for e in ev)
    pool = a.get("pool_size") or 0
    if not waves:
        if pool < 2:
            return INERT, f"{pool}-node fleet — racing requires more than one model by design"
        if not entered_repair:
            return INERT, "this cell never entered repair, so there was nothing to race"
        return FAIL, "a multi-node cell entered repair and NO spec_repair_wave fired — the baked lever is dead"
    bad = [w for w in waves if w.get("promoted")
           and not (isinstance(w.get("winner_findings"), int)
                    and isinstance(w.get("baseline_findings"), int)
                    and w["winner_findings"] < w["baseline_findings"])]
    if bad:
        return FAIL, ("PROMOTION WAS NOT STRICTLY BETTER — racing is unsafe, take the lever back off: "
                      + "; ".join(f"round {w.get('round')}: {w.get('baseline_findings')} -> "
                                 f"{w.get('winner_findings')}" for w in bad))
    post = a.get("post_execute_secs")
    if post is not None and post > F517_SERIAL_WORST_SECS:
        return FAIL, (f"raced repair fired but the suffix is {post / 60:.1f} min, worse than the "
                      f"serial path's {F517_SERIAL_WORST_SECS / 60:.1f} min worst case")
    promoted = sum(1 for w in waves if w.get("promoted"))
    return PASS, (f"{len(waves)} wave(s), {promoted} promoted, every promotion strictly better, "
                  f"suffix {(post or 0) / 60:.1f} min")


F537_FIX = "46f0c84ca"        # the commit that gates dynamic-replan on the fraction of plan remaining
F537_MIN_FRACTION = 0.25      # the gate: at least a quarter of the mandatory plan must still be open


def f537_replan_did_not_inject_into_a_finishing_dag(ev, a) -> tuple[str, str]:
    """No replan may inject bonus work once the mandatory plan is nearly done.

    F537 IS OTHERWISE INVISIBLE, and that is the whole reason this check exists. The engine emits an
    event when a replan HAPPENS and nothing at all when the gate REFUSES one, so a working fix looks
    exactly like a quiet run. Three times today I read an absence I could not observe as an absence
    that did not exist (F530 on the suffix, F531 on `complete_fix_completed`, F534 on the digests),
    and shipping a fourth silent mechanism without a detector would be inviting the same mistake a
    fourth time. The freeze forbids adding an engine event, so the property is reconstructed from
    what IS emitted: `plan_loaded` gives the mandatory task set, `task_completed` says which of them
    were done at any instant, and `replanned` carries the moment of injection.

    MEASURED HARMS THIS MUST NOW REFUSE: n3-r2 injected at 3-of-21 mandatory left (14%) and grew an
    18.3-minute bonus tail; n3-r3 injected at 2-of-18 (11%) and grew 26.8 minutes onto a run whose
    mandatory work had ALREADY finished.

    A FIRING REPLAN IS NOT A FAILURE. F537 blocks only LATE injection; n3-r0's injection came at
    44 minutes with most of the DAG still open and would still be allowed. Reporting every
    `replanned` as a regression would make this check red on correct behaviour, which is how a check
    stops being read.

    Bonus ids are excluded from both sides of the fraction. The DAG GROWS with every injection, so
    counting them makes a run look busier the more unplanned work it has taken on — the same
    denominator error the gate itself had to avoid.
    """
    sha = shardshare.build_sha(ev)
    if not carries(F537_FIX, sha):
        return INERT, f"this cell ran {sha}, which predates the fix {F537_FIX} — it cannot falsify it"
    replans = [e for e in ev if e.get("event") == "replanned"]
    if not replans:
        return INERT, ("no replan fired in this cell — the gate was never asked, which says nothing "
                       "either way about whether it would have refused")
    plan = next((e for e in ev if e.get("event") == "plan_loaded"), None)
    if not plan:
        return INERT, "no plan_loaded — the mandatory task set is unknown"
    planned = {t.get("id") for t in (plan.get("tasks") or []) if t.get("id")}
    if not planned:
        return INERT, "plan_loaded carried no tasks"

    def t(e):
        return occ.parse_ts(e.get("ts"))

    bonus = {x for e in replans for x in (e.get("added") or [])}
    mandatory = planned - bonus
    late = []
    for r in replans:
        at = t(r)
        done = {e.get("task_id") for e in ev
                if e.get("event") == "task_completed" and (t(e) or 0) <= (at or 0)}
        left = len(mandatory - done)
        frac = left / len(mandatory) if mandatory else 1.0
        if frac < F537_MIN_FRACTION:
            late.append(f"round {r.get('round')}: {left}/{len(mandatory)} mandatory left ({frac:.0%})")
    if late:
        return FAIL, ("replan injected into a finishing DAG — the gate did not hold: " + "; ".join(late))
    fracs = []
    for r in replans:
        at = t(r)
        done = {e.get("task_id") for e in ev
                if e.get("event") == "task_completed" and (t(e) or 0) <= (at or 0)}
        fracs.append(len(mandatory - done) / len(mandatory))
    return PASS, (f"{len(replans)} replan(s), all at or above the {F537_MIN_FRACTION:.0%} bar "
                  f"(lowest {min(fracs):.0%} of the mandatory plan still open)")


F532_FIX = "34359b8b7"   # the commit that enforces the repair-budget invariant on the RESOLVED value
F532_FIX_CAP_SECS = 1200  # one fix attempt's own cap; a round costs up to this


def f532_repair_budget_was_not_left_on_the_table(ev, a) -> tuple[str, str]:
    """A red app must never finish COMPLETE holding enough budget for another repair round.

    This is the defect F532 fixes, stated as a property of the run rather than of the config.
    baseline-n3-r0 scored the campaign's best 0.9033 and still shipped RED at 1 finding: the LIVE
    config carried the pre-raise `complete_cap_secs: 1200`, one round consumed it, and the loop broke
    at `cap_deadline` before the second round. Nothing in the suite could see it — the invariant test
    asserts on `default_complete_cap_secs()` and config merges OVER the default.

    THE PRE-FIX DEFECT IS EXHAUSTION, NOT SLACK, and an earlier version of this docstring said the
    opposite ("1216s of a nominal 3000s budget"). That run's budget WAS 1200 and it spent 1216 — seven
    seconds over, fully consumed. Nothing was left on any table. Across the corpus the signature is a
    cell spending 1201-1411s against its 1200s cap and ending RED WITH EXACTLY ONE FINDING, in roughly
    a third of runs reaching COMPLETE.

    The check below is nonetheless about SLACK, and correctly so — it runs only on POST-fix cells,
    where the lift has already granted a budget large enough for another round. There, unspent budget
    beside a red app is a real failure: the engine was given the room and did not use it. The pre-fix
    world could not produce that state at all, which is exactly why the provenance gate matters here.

    The budget is read from `complete_cap_lifted.effective_secs`, which is the only place a run states
    the repair budget it actually used. When that event is absent the budget is genuinely unknown from
    the log alone, and the honest verdict is INERT with that as the reason — an unknown budget must
    never be guessed at, because guessing high manufactures a FAIL and guessing low manufactures a
    PASS. F533 predicts the event fires on every cell on this machine, so a persistent INERT here is
    itself the interesting reading.
    """
    sha = shardshare.build_sha(ev)
    if not carries(F532_FIX, sha):
        return INERT, f"this cell ran {sha}, which predates the fix {F532_FIX} — it cannot falsify it"
    result = next((e for e in ev if e.get("event") == "complete_result"), None)
    verifies = [e for e in ev if e.get("event") == "complete_verify"]
    if result is None or not verifies:
        return INERT, "this cell never reached the COMPLETE loop"

    def t(e):
        return occ.parse_ts(e.get("ts"))

    elapsed = (t(result) or 0) - (t(verifies[0]) or 0)
    lifted = next((e for e in ev if e.get("event") == "complete_cap_lifted"), None)
    if lifted is None:
        return INERT, ("no complete_cap_lifted — the run never states its repair budget, so unspent "
                       "time cannot be computed without guessing at it")
    budget = lifted.get("effective_secs")
    if not isinstance(budget, (int, float)):
        return INERT, "complete_cap_lifted carried no effective_secs"
    unspent = budget - elapsed
    red = not result.get("passed") and (result.get("remaining_findings") or 0) > 0
    where = (f"used {elapsed:.0f}s of {budget:.0f}s "
             f"(requested {lifted.get('requested_secs')}s), {unspent:.0f}s unspent")
    if red and unspent >= F532_FIX_CAP_SECS:
        return FAIL, (f"shipped RED with {result.get('remaining_findings')} finding(s) and {where} — "
                      f"enough for another {F532_FIX_CAP_SECS}s round it never ran")
    if red:
        return PASS, f"red at {result.get('remaining_findings')} finding(s) but the budget was spent: {where}"
    return PASS, f"finished green; {where}"


def f497_plan_is_still_the_ceiling(ev, a) -> tuple[str, str]:
    """maxuse above 4.0 is F497's registered bar for "the DAG got wider". Reported for EVERY cell
    because it is the campaign's headline number, not only when it moves."""
    mu, slots = a.get("max_useful_nodes"), a.get("slot_count")
    if mu is None or not slots:
        return INERT, "no critical path — cannot compute the plan ceiling"
    who = "PLAN binds" if mu < slots else "FLEET binds"
    over = " — ABOVE the 4.0 bar (F497 threshold met)" if mu > 4.0 else ""
    return PASS, f"max_useful_nodes {mu} vs {slots} slots ⇒ {who}{over}"


def report(log_path: str, label: str) -> int:
    ev = [json.loads(l) for l in open(log_path) if l.strip()]
    a = occ.analyse(log_path)
    print(f"=== {label}   build_sha {shardshare.build_sha(ev)} ===")
    checks = [
        ("F491 observed hint kept", f491_hint_was_worth_keeping(ev, a)),
        ("F492 judge attempts timed", f492_judge_attempts_report_time(ev, a)),
        ("F499 unmeasured == judged", f499_unmeasured_is_the_judge_set(ev, a)),
        ("F500 invisible recovered", f500_every_invisible_task_recovered(ev, a, log_path)),
        ("F501 rewrite not a pattern", f501_rewrite_loop_is_not_a_pattern(ev, a)),
        ("F470 owns-nothing accepted", f470_owns_nothing_sink_can_be_accepted(ev, a)),
        ("F474 post-drop is faster", f474_a_dropped_body_costs_less_the_second_time(ev, a)),
        ("F511 no test-author stall", f511_no_test_author_stalled(ev, a)),
        ("F517 raced repair safe", f517_raced_repair_fires_and_never_regresses(ev, a)),
        ("F532 repair budget spent", f532_repair_budget_was_not_left_on_the_table(ev, a)),
        ("F537 replan not late", f537_replan_did_not_inject_into_a_finishing_dag(ev, a)),
        ("F497 plan is the ceiling", f497_plan_is_still_the_ceiling(ev, a)),
    ]
    worst = 0
    for name, (verdict, why) in checks:
        print(f"  {verdict:<5} {name:<28} {why}")
        if verdict == FAIL:
            worst = 1
    return worst


def main() -> int:
    args = sys.argv[1:]
    if args:
        paths = [(p, os.path.basename(p)) for p in args]
    else:
        paths = [(os.path.join(LOG_ARCHIVE, f), f.rpartition("-")[0])
                 for f in sorted(os.listdir(LOG_ARCHIVE)) if f.endswith(".jsonl")]
    worst = 0
    for p, label in paths:
        worst = max(worst, report(p, label))
        print()
    return worst


if __name__ == "__main__":
    sys.exit(main())
