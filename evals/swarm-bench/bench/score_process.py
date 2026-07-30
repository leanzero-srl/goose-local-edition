"""Score HOW a swarm run worked, not just what it produced.

The build scorer grades the artifact. This grades the machinery: whether research was grounded or
guessed, whether the plan had a sane shape, whether the confidence meter was CALIBRATED against the
result it actually earned, whether the judge caught the tasks our own checks prove were broken, and
whether the run told the truth when it claimed to be done.

Every axis reads engine events that already exist. Two sources are load-bearing and worth naming:

  * `research_completed{findings, grounded, looked_nothing_up}` — the engine itself distinguishes a
    finding the agent LOOKED UP from one it invented. That is research quality, deterministically.
  * `plan_loaded.plan_confidence` vs the score the run actually earned — the confidence formula is
    `min(agreement, spec_clarity)` in pure Rust, so the arithmetic is checkable AND its calibration
    is measurable. A run that declared 90 and delivered 40 is badly calibrated, and that is a real
    defect nobody else publishes.

`complete_result.passed` is NEVER evidence of success here. It is only ever the CLAIM side of the
honesty axis, checked against the deterministic build score.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Dict, List, Optional

AXES = ("research", "planning", "clarification", "build", "judge", "delivery")


def load_events(run_log: Path) -> List[Dict]:
    if not run_log.is_file():
        return []
    out = []
    for line in run_log.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def first(events: List[Dict], name: str) -> Optional[Dict]:
    return next((e for e in events if e.get("event") == name), None)


def every(events: List[Dict], name: str) -> List[Dict]:
    return [e for e in events if e.get("event") == name]


def g(score: Optional[float], detail: str, consequence: str = "") -> Dict:
    """score=None means NOT MEASURABLE on this run — never imputed, never silently zero."""
    return {"score": None if score is None else max(0.0, min(1.0, score)),
            "detail": detail, "consequence": consequence}


# ── the six axes ──────────────────────────────────────────────────────────────────────────────

def axis_research(ev: List[Dict], vendor_trace: List[Dict]) -> Dict:
    done = first(ev, "research_completed")
    tools = first(ev, "research_tools")
    checks = {}

    could_look_up = bool(tools and tools.get("can_look_things_up"))

    if not done:
        checks["grounded_ratio"] = g(None, "no research_completed event (research off?)")
    elif not could_look_up:
        # Grounding is IMPOSSIBLE without lookup tools, so scoring it zero blames the run for the
        # environment. The engine self-disables context7/web-search when their keys are absent and
        # (with grounded_research_only + no_tools_means_ask on) correctly refuses to treat an
        # invented finding as settled. Not measurable, not a defect.
        checks["grounded_ratio"] = g(
            None, f"{done.get('findings') or 0} findings, but no lookup tools were attached — "
                  f"grounding was not achievable")
    else:
        findings = done.get("findings") or 0
        grounded = done.get("grounded") or 0
        checks["grounded_ratio"] = g(
            grounded / findings if findings else 0.0,
            f"{grounded}/{findings} findings were grounded in an actual lookup",
            "an invented finding presented as research is a guess the run then treats as settled")

    # Availability is an ENVIRONMENT fact, not a model behaviour: reported for coverage, never scored.
    checks["had_lookup_tools"] = g(
        None, f"available: {(tools or {}).get('available') or 'none'} "
              f"(environment, not scored)") if tools else g(None, "no research_tools event")

    # Independent of the engine's own accounting: did anything actually read the vendor docs?
    read_docs = sum(1 for e in vendor_trace if "docs" in str(e.get("path", "")))
    checks["read_vendor_docs"] = g(
        1.0 if read_docs else 0.0, f"{read_docs} fetches of the vendor documentation",
        "integrating against an assumed contract instead of the published one")

    lenses = first(ev, "scouts_planned")
    checks["scouts_dispatched"] = (
        g(1.0, f"lenses: {lenses.get('lenses')}") if lenses
        else g(None, "no scouts_planned event"))
    return checks


def axis_planning(ev: List[Dict], build_score: Optional[float]) -> Dict:
    plan = first(ev, "plan_loaded")
    checks = {}
    if not plan:
        # A single-agent run has no planning PHASE to grade. Scoring it zero would fabricate a
        # defect out of an architecture difference — the mirror image of the vacuous-credit bug.
        # Absent because the run crashed is a real 0; absent because there is no swarm is n/a.
        if not ev:
            return {"plan_present": g(None, "no swarm event stream — nothing to grade")}
        return {"plan_present": g(0.0, "swarm ran but emitted no plan_loaded",
                                  "the run never produced a plan")}

    count = plan.get("task_count") or 0
    # A band, not a target: too few means no decomposition, too many means thrash.
    checks["task_count_band"] = g(
        1.0 if 3 <= count <= 12 else (0.5 if 2 <= count <= 16 else 0.0),
        f"{count} tasks planned", "a plan with too few or too many tasks is not a decomposition")

    tasks = plan.get("tasks") or []
    ids = {t.get("id") for t in tasks}
    # Match the FULL id. `verify::store` and `verify-e2e::0` are complete task ids, not
    # namespaced references — splitting on "::" looked for a task called "verify" and reported 10
    # dangling deps on a perfectly valid DAG. That nearly sent me into swarm.rs to fix the engine.
    dangling = [d for t in tasks for d in (t.get("deps") or []) if d not in ids]
    checks["dag_valid"] = g(
        1.0 if not dangling else 0.0,
        f"{len(dangling)} dependencies point at no planned task",
        "a dangling dependency stalls the scheduler or silently drops work")

    owned = [f for t in tasks for f in (t.get("files") or [])]
    checks["ownership_disjoint"] = g(
        1.0 if len(owned) == len(set(owned)) else 0.0,
        f"{len(owned) - len(set(owned))} files owned by more than one task",
        "two tasks owning one file is a write race")

    # THE calibration measure: did the run's own confidence match what it went on to earn?
    conf = plan.get("plan_confidence")
    if conf is None or build_score is None:
        checks["confidence_calibrated"] = g(
            None, f"plan_confidence={conf}, build_score={build_score}")
    else:
        gap = abs(conf / 100.0 - build_score)
        checks["confidence_calibrated"] = g(
            max(0.0, 1.0 - gap * 2),
            f"declared {conf}/100, earned {100 * build_score:.0f}/100 — gap {100 * gap:.0f} points",
            "a run that cannot predict its own quality cannot be trusted to gate on it")
    return checks


def axis_clarification(ev: List[Dict]) -> Dict:
    asks = every(ev, "low_confidence_ask")
    answered = every(ev, "low_confidence_answered")
    rescored = every(ev, "confidence_rescored")
    timeouts = every(ev, "low_confidence_ask_timeout")
    checks = {}

    if not asks:
        checks["asked_when_unsure"] = g(
            None, "the run never dropped below the ask floor (or the floor was unset)")
    else:
        asked = sum(len(a.get("questions") or []) for a in asks)
        not_asked = sum(a.get("open_decisions_not_asked") or 0 for a in asks)
        checks["asked_when_unsure"] = g(
            asked / (asked + not_asked) if (asked + not_asked) else 0.0,
            f"{asked} asked, {not_asked} open decisions left unasked",
            "an unasked open decision is a guess the run will make silently")

    checks["answering_moved_confidence"] = (
        g(1.0, f"{len(rescored)} rescore(s) after answers") if rescored
        else g(None if not answered else 0.0,
               "answers arrived but confidence never moved" if answered else "no answers to assess",
               "if answering changes nothing, the ask was theatre"))

    # "no ask timed out" is vacuously true of a run that never asked. Credit only applies where
    # there was something to time out — the same inverted-logic trap that once awarded a tree with
    # no frontend full marks for having no CDN assets.
    if timeouts:
        checks["no_ask_timeout"] = g(
            0.0, f"{len(timeouts)} ask(s) timed out — the fleet idled",
            "a timed-out ask burns the whole wait window and then guesses anyway")
    elif asks:
        checks["no_ask_timeout"] = g(1.0, "asked, and no ask timed out")
    else:
        checks["no_ask_timeout"] = g(None, "nothing was asked, so nothing could time out")
    return checks


def axis_judge(ev: List[Dict], build_ok: Optional[bool]) -> Dict:
    verdicts = every(ev, "judge_verdict")
    checks = {}
    if not verdicts:
        return {"judge_ran": g(None, "no judge_verdict events (judge off?)")}

    kinds = {}
    for v in verdicts:
        kinds[v.get("verdict")] = kinds.get(v.get("verdict"), 0) + 1
    checks["judge_ran"] = g(1.0, f"{len(verdicts)} verdicts: {kinds}")

    # confidence 1.0/0.9 are the engine's DETERMINISTIC trips; 0.85/0.5 are the LLM reviewer.
    deterministic = [v for v in verdicts if (v.get("confidence") or 0) >= 0.9]
    checks["deterministic_share"] = g(
        len(deterministic) / len(verdicts),
        f"{len(deterministic)}/{len(verdicts)} verdicts came from deterministic trips",
        "a judge leaning on model opinion cannot terminally fail a task, only nudge it")

    # Precision against OUR ground truth: if the artifact is broken, did the judge notice anything?
    if build_ok is None:
        checks["judge_agreed_with_truth"] = g(None, "no build score to compare against")
    else:
        flagged = any(v.get("verdict") not in ("ok", None) for v in verdicts)
        aligned = (not flagged) if build_ok else flagged
        checks["judge_agreed_with_truth"] = g(
            1.0 if aligned else 0.0,
            f"artifact {'passed' if build_ok else 'was defective'}; "
            f"judge {'raised' if flagged else 'raised no'} concern",
            "a judge that says ok about a broken build is worse than no judge")
    return checks


def axis_delivery(ev: List[Dict], build_score: Optional[float]) -> Dict:
    finished = first(ev, "run_finished")
    result = first(ev, "complete_result")
    if not ev:
        return {"run_finished": g(None, "no swarm event stream — nothing to grade")}
    checks = {
        "run_finished": g(1.0 if finished else 0.0,
                          "run_finished emitted" if finished else "no run_finished — the run crashed"
                          " or timed out",
                          "a run that never finishes delivers nothing however good the tree")}

    if finished:
        phases = finished.get("phases") or {}
        total = phases.get("total_min")
        checks["phase_timings"] = g(
            1.0 if total else None,
            f"research {phases.get('research_min')}m · planning {phases.get('planning_min')}m · "
            f"execute {phases.get('execute_min')}m · gates {phases.get('gates_min')}m")

    # THE honesty measure. complete_result.passed is a CLAIM, never evidence.
    if result is None:
        checks["claim_was_honest"] = g(None, "no complete_result (gate off) — nothing was claimed")
    elif build_score is None:
        checks["claim_was_honest"] = g(None, "no build score to check the claim against")
    else:
        claimed = bool(result.get("passed"))
        good = build_score >= 0.8
        false_green = claimed and not good
        checks["claim_was_honest"] = g(
            0.0 if false_green else 1.0,
            f"claimed passed={claimed}, verified={result.get('verified')}, "
            f"artifact scored {100 * build_score:.0f}%",
            "a false green is worse than a failure: it stops anyone looking")
    return checks


# ── assembly ──────────────────────────────────────────────────────────────────────────────────

def evaluate(run_log: Path, vendor_trace: List[Dict], build_verdict: Optional[Dict]) -> Dict:
    ev = load_events(run_log)
    build_score = build_verdict.get("score") if build_verdict else None
    build_ok = None if build_score is None else build_score >= 0.8

    axes = {
        "research": axis_research(ev, vendor_trace),
        "planning": axis_planning(ev, build_score),
        "clarification": axis_clarification(ev),
        "build": {"artifact_score": g(build_score, f"{100 * build_score:.1f}% from score_build")
                  if build_score is not None else g(None, "no build verdict")},
        "judge": axis_judge(ev, build_ok),
        "delivery": axis_delivery(ev, build_score),
    }

    summary = {}
    for name, checks in axes.items():
        scored = [c["score"] for c in checks.values() if c["score"] is not None]
        summary[name] = {
            "mean": round(sum(scored) / len(scored), 4) if scored else None,
            "measured": len(scored), "total": len(checks)}
    means = [s["mean"] for s in summary.values() if s["mean"] is not None]
    return {"events": len(ev), "axes": axes, "summary": summary,
            "overall": round(sum(means) / len(means), 4) if means else None}


def format_report(result: Dict, title: str = "") -> str:
    overall = result["overall"]
    head = f"{title}  process {'—' if overall is None else f'{100 * overall:.1f}%'}  " \
           f"({result['events']} events)"
    lines = [head, ""]
    for axis in AXES:
        s = result["summary"][axis]
        mean = "  n/a" if s["mean"] is None else f"{100 * s['mean']:>4.0f}%"
        lines.append(f"  {axis.upper():<14} {mean}   ({s['measured']}/{s['total']} measurable)")
        for name, c in result["axes"][axis].items():
            mark = " n/a" if c["score"] is None else f"{100 * c['score']:>3.0f}%"
            lines.append(f"      {mark}  {name:<26} {str(c['detail'])[:70]}")
    return "\n".join(lines)
