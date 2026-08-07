#!/usr/bin/env python3
"""Can each queued ARM actually answer its own question? Exit 0 (1 if a queued arm cannot).

TWICE IN ONE DAY an arm was bought with fleet time it could not repay:

  * `kind_prompt` (F111) — its readout was CIRCULAR. `dispatch_audit` computed
    `mismatched = ... if not kind_prompt_on else 0`, so with the lever ON the count was HARDCODED to
    zero. "kind_mismatch_pct falls toward zero" would have succeeded BY CONSTRUCTION, on the very run
    bought to test it.
  * `retarget_off` (F117/F118) — its MECHANISM never fires. The redraft ladder runs only when
    `plan_confidence < ask_floor`, and plan_confidence is 88 in 8 of 13 archived runs against a floor
    of 85. So the arm switched off something already absent: a null experiment dressed as a comparison.

Both would have been caught by asking, BEFORE the run, two questions the baseline can answer:

    1. does the arm's MECHANISM fire on the baseline at all?   (else there is nothing to change)
    2. can the INSTRUMENT see the change?                      (else the readout is unearned)

That is what this script does. It is deliberately CONSERVATIVE: an arm whose precondition cannot be
decided from the baseline's own events is reported UNKNOWN, never OK. A green here is not proof the
arm will produce a result — it is proof the arm is not already doomed, which is the cheap half.

Usage:
    python3 armcheck.py [<baseline-run-dir>]      # defaults to the newest completed baseline
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
# The solo-window figure is occupancy.py's. Re-deriving it here would repeat the mistake review.py
# made twice (F107/F112) — a completion-sum misses a task superseded by a split.
import occupancy  # noqa: E402

RUNS = HERE.parent / "runs" / "nodeloop"

# For each arm: what must be TRUE of a baseline run for the arm to be able to change anything, and how
# the change would be READ. `probe` returns (verdict, why) given the baseline's event list.
#
# The distinction that matters: PRECONDITION is about the engine (can this fire?), READOUT is about the
# instrument (could we tell?). An arm needs BOTH, and F111 failed only the second.


def _count(ev, name):
    return sum(1 for e in ev if e.get("event") == name)


def _plan(ev):
    return next((e for e in ev if e.get("event") == "plan_loaded"), None)


def arm_kind_prompt(ev):
    n = _count(ev, "task_dispatched")
    if not n:
        return "UNKNOWN", "no dispatches in the baseline"
    # PRECONDITION: dispatches of a kind other than implementer exist (else no rules to re-target).
    # READOUT: `rules_delivered` must be in the log, or the mismatch metric is unmeasurable when the
    # lever is on — the exact circularity F111 found.
    if _count(ev, "rules_delivered") == 0:
        return "BLOCKED", ("no `rules_delivered` events — with the lever ON the delivered rule-set is "
                           "unprovable and the readout is circular (F111). Needs the post-F111 engine. "
                           "NOTE: F124 gave this arm a SECOND, un-gameable readout that does not wait "
                           "on the boundary — hard-test retry rate from task_dispatched.attempt "
                           "(baseline 60%, n=30, vs 12% for hard non-test). Nothing in the lever's own "
                           "accounting can touch it, which is exactly what killed the first readout.")
    # PRECONDITION, second half: the lever only re-targets rules for kinds that ACTUALLY DISPATCH. If
    # this baseline had no test-author dispatch there is no mismatch to fix and the arm is inert here.
    test_disp = sum(1 for e in ev if e.get("event") == "task_dispatched"
                    and str(e.get("task_id", "")).startswith("test"))
    if test_disp == 0:
        return "UNSUITABLE", ("rules_delivered present, but this baseline dispatched NO test-authoring "
                              "task — the kind carrying the entire measured retry burden (F124) never "
                              "ran, so the arm has nothing to improve against it")
    return "OK", (f"{n} dispatches, rules_delivered present, and {test_disp} test-author dispatch(es) "
                  f"— the kind F124 measured at 60% retry is present to be improved")


def arm_retarget_off(ev):
    """STOCHASTIC precondition — and getting this wrong once is why this docstring exists.

    F119 reported BLOCKED from a single baseline (`plan_confidence 100 >= ask_floor 85`) and I wrote
    it up as "the ladder never runs". Then a live 1-node unit came in at **confidence 36** with the
    redraft firing twice. Surveying all 14 archived runs: confidence ranges 36-100 at EVERY node count
    (1 node: 100/100/36; 3 nodes: 54-100), and `conf < floor` holds in **4 of 14 ≈ 29%**.

    So the precondition is neither absent nor reliable — it is a COIN FLIP, and a one-baseline check
    reported a distribution as a constant. That is the difference between "this arm cannot work" and
    "this arm needs a baseline that satisfies it", which are different actions.
    """
    pl = _plan(ev)
    if not pl:
        return "UNKNOWN", "no plan_loaded"
    conf, floor = pl.get("plan_confidence"), pl.get("ask_floor")
    if conf is None or floor is None:
        return "UNKNOWN", "plan_loaded lacks confidence/floor"
    if conf >= floor:
        return "UNSUITABLE", (f"plan_confidence {conf} >= ask_floor {floor} on THIS baseline, so the "
                              f"ladder does not run here. Measured across the archive the "
                              f"precondition holds ~29% of runs (conf 36-100 at every node count), so "
                              f"the arm is not dead — it needs a LOW-CONFIDENCE baseline to pair with")
    return "OK", f"plan_confidence {conf} < ask_floor {floor}: the ladder fires"


def arm_doc_prefetch(ev):
    rc = next((e for e in ev if e.get("event") == "research_completed"), None)
    if not rc:
        return "UNKNOWN", "no research_completed"
    g = rc.get("grounded")
    if not g:
        return "BLOCKED", ("grounded == 0, so doc_facts would be EMPTY and the verbatim channel carries "
                           "nothing — inert by construction (F84)")
    return "OK", f"grounded={g}: the verbatim channel has content to carry"


def arm_spec_repair(ev):
    rounds = _count(ev, "complete_verify")
    if rounds == 0:
        return "BLOCKED", "no repair round ran on the baseline — nothing to race"
    return "OK", f"{rounds} repair round(s): the race has work"


def arm_complete_parallel(ev):
    # Fans by FAILING FILE, so it needs a round whose findings name >1 distinct file.
    best = 0
    for e in ev:
        if e.get("event") == "complete_verify":
            best = max(best, e.get("findings") or 0)
    if best <= 1:
        return "BLOCKED", (f"max findings in any round = {best}; the fan is per-file so it can never "
                           f"exceed one shard (F73)")
    return "OK", f"a round had {best} findings: the fan has >1 item"


def arm_sink_review(ev, run=None):
    """Idle-fill during the sink needs idle capacity DURING the sink — which is exactly what
    occupancy.py's solo window measures, so ask it rather than guessing."""
    if _count(ev, "sink_review"):
        return "OK", "sink_review already fires"
    if not any(e.get("event") == "task_dispatched" and e.get("task_id") == "integrate-verify"
               for e in ev):
        return "UNKNOWN", "no integrate-verify dispatch to idle-fill around"
    if run is None:
        return "UNKNOWN", "no run dir to hand occupancy.py"
    try:
        occ = occupancy.analyse(run)
    except Exception as exc:
        return "UNKNOWN", f"occupancy unavailable ({exc})"
    solo = occ.get("solo_by_task") or {}
    sink_solo = solo.get("integrate-verify", 0.0)
    if sink_solo <= 0:
        return "BLOCKED", ("the sink never ran ALONE, so there was no idle capacity to fill — "
                           "idle-fill has no window")
    return "OK", (f"the sink held a node alone for {sink_solo:.0f}s with the other nodes idle — "
                  f"that is the window idle-fill exists for")


def arm_detail_budget(ev):
    # Raising the detail ceiling only matters if a detail call is being CUT by it.
    secs = [e.get("secs") for e in ev if e.get("event") == "detail_completed" and e.get("secs")]
    budget = next((e.get("budget_secs") for e in ev if e.get("event") == "detail_completed"), None)
    if not secs:
        return "UNKNOWN", "no detail_completed events"
    if budget and max(secs) < budget * 0.8:
        return "BLOCKED", (f"slowest detail {max(secs):.0f}s vs budget {budget}s — nothing is near the "
                           f"ceiling, so raising it changes nothing")
    return "OK", f"slowest detail {max(secs):.0f}s against budget {budget}s"


def arm_e2e_oracle(ev):
    if _count(ev, "task_dispatched") == 0:
        return "UNKNOWN", "no dispatches"
    shards = sum(1 for e in ev if e.get("event") == "task_dispatched"
                 and str(e.get("task_id", "")).startswith("verify-e2e::"))
    if shards == 0:
        return "BLOCKED", "no verify-e2e:: shards ran — the oracle has nothing to re-source"
    return "OK", f"{shards} e2e shard(s) ran"


def arm_spec_sized_plan(ev):
    """Can the plan this baseline emitted actually SHRINK?

    The arm's registered threshold is <=5 module subtasks (F457/F458). If the baseline ALREADY emits
    <=5, a spec-sized clause has nothing to remove and the cell would spend a fleet unit reproducing
    the baseline — the classic INERT result that says nothing and reads like a null.

    Counted from `plan_loaded.tasks[]` with the scaffolding excluded, because `verify::*`,
    `verify-e2e::*`, `test-*` and `integrate-verify` are added by the engine AFTER the architect
    answers the count clause, so they are not what the clause controls and folding them in would
    inflate every reading. This is the same nesting the desc_sha probe got wrong (L264): the tasks
    live INSIDE plan_loaded, not beside it.
    """
    pl = [e for e in ev if e.get("event") == "plan_loaded"]
    if not pl:
        return "UNKNOWN", "no plan_loaded"
    tasks = pl[-1].get("tasks") or []
    if not tasks:
        return "UNKNOWN", "plan_loaded carries no tasks[] — cannot count modules"
    mods = [str(t.get("id", "")) for t in tasks]
    mods = [m for m in mods
            if not m.startswith(("verify::", "verify-e2e::", "test-")) and m != "integrate-verify"]
    if len(mods) <= 5:
        return "BLOCKED", (f"baseline already emits {len(mods)} module subtasks (<=5), so the "
                           f"spec-sized clause has nothing to remove and the cell would reproduce "
                           f"the baseline")
    return "OK", f"baseline emits {len(mods)} modules (>5): the clause has room to bind"


def arm_e2e_oracle_off(ev):
    """ABLATION of a now-BAKED lever, so the precondition inverts.

    Turning the oracle OFF can only show something if the oracle was ON in the baseline. A baseline
    that already ran without it reproduces itself and answers nothing — and after F455 baked it ON,
    "was it on" is a real question about which binary the cell ran, not a formality.
    """
    # DISTINCT shards, not dispatches: a retried shard dispatches twice and would be double-counted,
    # which read "5 shards" on a cell that has four. The verdict was right and the sentence was not,
    # and a wrong sentence in a gate is what a later claim gets built on.
    shards = len({str(e.get("task_id", "")) for e in ev
                  if e.get("event") == "task_dispatched"
                  and str(e.get("task_id", "")).startswith("verify-e2e::")})
    if shards == 0:
        return "BLOCKED", "no verify-e2e:: shards ran — nothing to un-source"
    lv = [e for e in ev if e.get("event") == "levers_resolved"]
    if not lv:
        return "UNKNOWN", "no levers_resolved — cannot tell whether the oracle was on to ablate"
    levers = lv[-1].get("levers")
    if not isinstance(levers, dict):
        return "UNKNOWN", "levers_resolved carries no nested `levers` dict (L264)"
    if "e2e_oracle" not in levers:
        return "UNKNOWN", "this binary predates the e2e_oracle lever — absent is not False (L264)"
    if not levers["e2e_oracle"]:
        return "BLOCKED", (f"the oracle was already OFF in this baseline ({shards} shards), so the "
                           f"ablation reproduces it — this baseline predates e620bf0b6")
    return "OK", f"oracle ON with {shards} shard(s): the ablation has something to remove"


def arm_spiral_thinking(ev):
    """#134's early spiral trip: BUILT (judge.rs:359), default-OFF (`spiral_thinking_chars: 0`).

    It fires at `min_age_secs` (90s) instead of the 420s floor when a worker owns files, has written
    none, has made ZERO tool calls, and has emitted more than a cap of THINKING. That last term is
    the reason this arm could never be checked: `worker_thinking_chars` appeared on no event, so the
    precondition was unobservable and the lever sat off and unmeasured (F128).

    `judge_observed` is that fix. It emits the RAW inputs on every judge invocation, so the question
    "would this lever ever have fired, and at what cap?" is answerable from an archived run — before
    a unit is spent, which is the whole point of this file.
    """
    obs = [e for e in ev if e.get("event") == "judge_observed"]
    if not obs:
        return "BLOCKED", ("no `judge_observed` events — worker_thinking_chars is emitted nowhere, so "
                           "the trip's precondition cannot be checked and the arm is F111 all over "
                           "again (F128). Needs the post-F128 engine.")
    # PRECONDITION, exactly the trip's own terms minus the cap, so no threshold is hardcoded here.
    cands = [e for e in obs if e.get("owns_files") and not e.get("any_owned_written")
             and e.get("tool_calls") == 0]
    if not cands:
        return "UNSUITABLE", (f"{len(obs)} judge observations, but none has owns_files AND nothing "
                              f"written AND tool_calls == 0 — the shape the trip exists for never "
                              f"occurred on this baseline, so switching it on changes nothing here")
    thinking = [e.get("thinking_chars") for e in cands if isinstance(e.get("thinking_chars"), int)]
    if not thinking:
        return "BLOCKED", (f"{len(cands)} zero-action observations but thinking_chars is null on all "
                           f"of them — the digest predates the key, so the cap term is undecidable")
    thinking.sort()
    return "OK", (f"{len(cands)} zero-action observation(s); thinking_chars median {thinking[len(thinking)//2]}, "
                  f"max {thinking[-1]} — pick the cap BELOW the max or the trip cannot fire")


def arm_doc_fetch(ev):
    """The engine's own rule (spec_doc_urls): an http(s) URL with a PATH — a bare origin is the app's
    base URL, not a document. Mirrored here deliberately and narrowly; if the two ever disagree the
    arm is what suffers, so this states the rule rather than approximating it."""
    rs = next((e for e in ev if e.get("event") == "run_started"), None)
    spec = (rs or {}).get("prompt") or ""
    if not spec:
        return "UNKNOWN", "no run_started prompt to read"
    urls = [u for u in re.findall(r"https?://[^\s`'\"<>()\[\]{},;]+", spec)
            if u.split("://", 1)[-1].count("/") >= 1]
    if not urls:
        return "BLOCKED", ("the spec names no fetchable document (an http(s) URL WITH a path) — "
                           "doc_fetch has nothing to fetch")
    return "OK", f"the spec names {len(urls)} fetchable doc URL(s), e.g. {urls[0]}"


def arm_diverse_plan(ev):
    """The one arm whose precondition the ENGINE now answers directly, so nothing has to be inferred.

    F438: the redraft ladder is the 3-node tax. Every `confidence_retarget` in the archive carries
    `binding_signal: "agreement"`, and it cost 786/821/1657s on the 3-node cells against ZERO on the
    1-node cell — because 3 nodes draft 3 skeletons where 1 node drafts 2, and `plan_agreement` is
    max-min spread plus mean pairwise Jaccard, both of which (per `best_subset_agreement`'s own doc)
    "only worsen (or hold) as the pool grows". `diverse_plan` ENFORCE replaces that score with
    `structural_convergence`, which ignores count spread.

    `plan_convergence.would_skip_ladder` is the engine's OWN counterfactual, computed from the same
    predicate the enforce branch uses (`diverse_plan_would_skip`), and emitted whether or not the
    lever is on. So this probe reads a fact instead of estimating one — the failure mode that made
    `retarget_off` a null experiment twice.

    UNKNOWN, never OK, on a baseline older than the event: the conservative rule this file states up
    front. A pre-086981caf run simply cannot answer, and "absent" must never read as "false".
    """
    pcs = [e for e in ev if e.get("event") == "plan_convergence"]
    if not pcs:
        return "UNKNOWN", ("no plan_convergence event — baseline predates 086981caf, so the "
                           "counterfactual is UNRECORDED, not false")
    pc = pcs[0]
    ladder = _count(ev, "confidence_retarget")
    if not ladder:
        return "BLOCKED", (f"agreement {pc.get('agreement_conf')} cleared the floor on the first "
                           f"draft — no ladder ran, so there is nothing for ENFORCE to skip")
    if not pc.get("would_skip_ladder"):
        return "BLOCKED", (f"struct_conv {pc.get('struct_conv')} does not clear struct_stop "
                           f"{pc.get('struct_stop')} while beating agreement "
                           f"{pc.get('agreement_conf')} — ENFORCE would change NOTHING on this "
                           f"baseline, so the arm is inert by construction")
    return "OK", (f"ladder ran {ladder}x and would_skip_ladder is true (struct_conv "
                  f"{pc.get('struct_conv')} vs agreement {pc.get('agreement_conf')}) — ENFORCE has "
                  f"something real to remove, and prefix wall-clock reads the change")


ARMS = {
    "diverse_plan": arm_diverse_plan,
    "kind_prompt": arm_kind_prompt,
    "doc_prefetch": arm_doc_prefetch,
    "spec_repair": arm_spec_repair,
    "detail_budget": arm_detail_budget,
    "complete_parallel": arm_complete_parallel,
    "e2e_oracle": arm_e2e_oracle,
    "e2e_oracle_off": arm_e2e_oracle_off,
    "spec_sized_plan": arm_spec_sized_plan,
    "retarget_off": arm_retarget_off,
    "sink_review": arm_sink_review,
    "doc_fetch": arm_doc_fetch,
    "spiral_thinking": arm_spiral_thinking,
}


def queued_reps() -> dict:
    """How many units the sweep will actually SPEND on each arm.

    Without this the summary line claimed every BLOCKED arm "would spend a fleet unit and answer
    nothing" — and `detail_budget` is parked at `reps: 0` in sweep.py, so it will spend nothing at
    all. Charging a parked arm is the same defect this script exists to catch, one level up: a
    verdict that cannot separate "blocked and queued" from "blocked and already parked", which are
    different actions (fix it now vs leave it alone).

    Parsed rather than imported: sweep.py is the LIVE supervisor's module and importing it here
    would run its top level while it is mid-run.
    """
    src = HERE / "sweep.py"
    if not src.is_file():
        return {}
    text = src.read_text(errors="replace")
    out: dict[str, int] = {}
    for m in re.finditer(r'"arm"\s*:\s*"([a-z_]+)"\s*,\s*"nodes"\s*:\s*\d+\s*,\s*"reps"\s*:\s*(\d+)', text):
        out[m.group(1)] = max(out.get(m.group(1), 0), int(m.group(2)))
    for m in re.finditer(r'"name"\s*:\s*"([a-z_]+)"\s*,\s*\n\s*"reps"\s*:\s*(\d+)', text):
        out.setdefault(m.group(1), int(m.group(2)))
    return out


def is_complete(run: pathlib.Path) -> bool:
    """A baseline is only usable once it has FINISHED. An in-flight run's event stream is empty of
    everything that has not happened YET, and every probe here reads absence as a verdict."""
    for f in run.glob("*.jsonl"):
        for line in f.read_text(errors="replace").splitlines():
            if '"run_finished"' in line:
                return True
    return False


def newest_baseline() -> pathlib.Path | None:
    """Newest COMPLETE run, preferring a `baseline*` one.

    MEASURED the moment the post-boundary sweep restarted: the runs dir held exactly one unit, six
    minutes old, with no dispatches. This function handed it over and every probe then reported a
    confident verdict from an empty stream — `spec_repair` BLOCKED ("no repair round ran"),
    `complete_parallel` BLOCKED ("max findings = 0"), `spiral_thinking` BLOCKED. None of those had
    happened YET. That is the standing rule — an UNCONTROLLED ZERO IS NOT EVIDENCE — broken inside
    the very script written to stop arms being bought on bad evidence.
    """
    named = [p.parent for p in RUNS.glob("baseline*/run.jsonl")]
    for pool in (named, [p.parent for p in RUNS.glob("*/run.jsonl")]):
        done = [p for p in pool if is_complete(p)]
        if done:
            return max(done, key=lambda p: p.stat().st_mtime)
    return None


def main(argv: list[str]) -> int:
    run = pathlib.Path(argv[0]) if argv else newest_baseline()
    if not run or not run.is_dir():
        parked = sorted(RUNS.parent.glob("nodeloop-preboundary-*"))
        print("NO COMPLETE BASELINE on this engine build — arm preconditions are UNDECIDABLE, not clear.")
        print("  Nothing is BLOCKED and nothing is OK; there is simply no evidence yet. Exit 0 so this")
        print("  cannot be read as a green light: wait for the first unit to reach run_finished.")
        if parked:
            print(f"  (pre-boundary runs are parked at {parked[-1].name}; do NOT judge a new build's")
            print("   arms against them — that is what a boundary invalidates.)")
        return 0
    if not is_complete(run):
        print(f"{run.name} has not FINISHED — refusing to judge arms against a partial event stream.")
        print("  Every probe here reads absence as a verdict, so an in-flight run manufactures BLOCKED.")
        return 0
    ev = []
    for f in sorted(run.glob("*.jsonl")):
        for line in f.read_text(errors="replace").splitlines():
            line = line.strip()
            if line:
                try:
                    ev.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    if not ev:
        print(f"{run.name}: no events")
        return 0
    reps = queued_reps()
    print(f"=== ARM CHECK against {run.name} ===")
    print("can each queued arm change anything, and could we tell?\n")
    blocked, parked = [], []
    for name, probe in ARMS.items():
        try:
            verdict, why = probe(ev, run) if name == "sink_review" else probe(ev)
        except Exception as exc:
            verdict, why = "UNKNOWN", f"probe raised: {exc}"
        # UNSUITABLE means "not against THIS baseline" — a pairing problem, not a dead arm. Counting
        # it as BLOCKED is what turned a 29%-likely precondition into "never runs".
        n = reps.get(name)
        if verdict == "BLOCKED":
            (parked if n == 0 else blocked).append(name)
        tag = "  [PARKED reps 0]" if n == 0 else ""
        print(f"  {verdict:<8} {name:<20} {why}{tag}")
    print()
    if parked:
        print(f"parked and harmless: {', '.join(parked)} — BLOCKED but reps 0, so the sweep will never "
              f"spend a unit on them. Nothing to do.")
    if blocked:
        print(f"{len(blocked)} arm(s) BLOCKED AND QUEUED ({', '.join(blocked)}) — each would spend a "
              f"fleet unit and answer nothing. Fix the precondition or the instrument first.")
        return 1
    print("no arm is provably doomed. That is the cheap half — it does not promise a result.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
