"""Deterministic PLAN-SHAPE acceptance checks read from a run's own events — no brain, no key.

These exist because a lever can be ON in `levers_resolved` and still not have done its job. The measured
case: `fan_verify` split the sink into per-module `verify::<M>` tasks, but a later plan mutation re-wrote
integrate-verify's description back to the full monolithic spec, so every fanned run paid for N extra
tasks AND still ran the whole suite serially in the sink. Both the lever echo and the task list looked
correct; only the sink's SPEC TEXT showed the split had been undone.

So: check the lever's structural consequences, not the lever's own report of itself.
"""

from __future__ import annotations

import json
from typing import Dict, List, Optional

from .contracts import Finding

SINK = "integrate-verify"
VERIFY_PREFIX = "verify::"
# The thin join OPENS with this; the monolithic spec opens with "Integrate every module and VERIFY...".
#
# This must be a PREFIX test, never a substring test. The sink canonicalizer appends the pre-existing
# description under "Also run these concrete plan-enumerated checks:", so a monolithic sink that swallowed
# a thin spec CONTAINS this marker several thousand characters in while still opening with — and running —
# the full monolith. VERIFIED against the archived corpus: verify-6's sink is 6651 chars beginning
# "Integrate every module and VERIFY ... run the test suite (python3 -m pytest), then", with
# "INTEGRATION JOIN" at index 3221, right after "Also run these" at 3173. A `contains` check called that
# run thin. It was not.
THIN_MARKER = "INTEGRATION JOIN"


def plan_shape(log_path: str) -> Dict[str, object]:
    fan_verify: Optional[bool] = None
    tasks: List[dict] = []
    with open(log_path, errors="replace") as fh:
        for line in fh:
            try:
                e = json.loads(line)
            except ValueError:
                continue
            if e.get("event") == "levers_resolved":
                levers = e.get("levers") or {}
                if "fan_verify" in levers:
                    fan_verify = bool(levers["fan_verify"])
            elif e.get("event") == "plan_loaded":
                tasks = list(e.get("tasks") or [])

    by_id = {str(t.get("id")): t for t in tasks if t.get("id")}
    verify_ids = sorted(i for i in by_id if i.startswith(VERIFY_PREFIX))
    sink = by_id.get(SINK)
    sink_deps = set()
    if sink:
        sink_deps = {str(d) for d in (sink.get("deps") or sink.get("depends_on") or [])}
    return {
        "fan_verify": fan_verify,
        "n_tasks": len(tasks),
        "verify_tasks": verify_ids,
        "sink_present": sink is not None,
        "sink_gates_every_verify": bool(verify_ids) and set(verify_ids).issubset(sink_deps),
        "sink_is_thin": bool(
            sink and str(sink.get("description") or "").lstrip().startswith(THIN_MARKER)
        ),
        "verify_tasks_own_nothing": all(
            not (by_id[i].get("files") or []) for i in verify_ids
        ),
        # A verify:: task must depend on its MODULE and nothing else. An edge onto a test subtask deadlocks
        # the oracle: MEASURED h1-treat-1, `test-wal` failed and fail_descendants cascaded Failed through
        # `verify::wal` into `integrate-verify`, so the end-to-end gate never ran and the corrupt-store probe
        # — the only check that would have caught the shipped defect — never happened.
        "verify_tasks_depend_only_on_their_module": all(
            [d for d in (by_id[i].get("deps") or by_id[i].get("depends_on") or [])]
            == [i[len(VERIFY_PREFIX):]]
            for i in verify_ids
        ),
        "measurable": bool(tasks),
    }


def findings_for(shape: Dict[str, object]) -> List[Finding]:
    """Silent when the run never loaded a plan — an absent check is never reported as a passing one."""
    if not shape.get("measurable"):
        return []
    out: List[Finding] = []
    ev = str(shape)

    if shape["fan_verify"] and not shape["verify_tasks"]:
        out.append(Finding(
            id="fanverify-no-split", dimension="cluster", severity="high",
            text="fan_verify resolved ON but the plan carries no verify:: tasks — the split did not apply",
            evidence=ev,
            fix_hint="fan_verify_split no-ops with no sink, no file-owning module, or an already-fanned plan",
        ))
    if shape["fan_verify"] is False and shape["verify_tasks"]:
        out.append(Finding(
            id="fanverify-unexpected-split", dimension="cluster", severity="high",
            text="verify:: tasks exist while fan_verify resolved OFF — the OFF path is not byte-identical",
            evidence=ev,
        ))
    if shape["verify_tasks"]:
        if not shape["sink_present"]:
            out.append(Finding(
                id="fanverify-no-join", dimension="correctness", severity="high",
                text="the plan was fanned but has NO integrate-verify join — nothing runs the app end-to-end",
                evidence=ev,
            ))
        else:
            if not shape["sink_gates_every_verify"]:
                out.append(Finding(
                    id="fanverify-join-ungated", dimension="correctness", severity="high",
                    text="integrate-verify does not depend on every verify:: task, so the join can run "
                         "before a module has been verified",
                    evidence=ev,
                ))
            if not shape["sink_is_thin"]:
                out.append(Finding(
                    id="fanverify-join-not-thin", dimension="cluster", severity="high",
                    text="the plan was fanned but integrate-verify still carries the MONOLITHIC spec — the "
                         "run pays for the split and keeps the serial sink",
                    evidence=ev,
                    fix_hint="a later plan mutation is overwriting the thin join spec after fan_verify_split",
                ))
        if not shape["verify_tasks_depend_only_on_their_module"]:
            out.append(Finding(
                id="fanverify-verify-waits-on-a-test", dimension="correctness", severity="high",
                text="a verify:: task depends on something other than its own module — a failing test now "
                     "cascades Failed into integrate-verify and the end-to-end gate never runs",
                evidence=ev,
                fix_hint="verify::<M> must depend on [M] only; strip_integrate_verify_test_deps exists to "
                         "keep a failing unit test from blocking the sink, and this routes around it",
            ))
        if not shape["verify_tasks_own_nothing"]:
            out.append(Finding(
                id="fanverify-verify-owns-files", dimension="correctness", severity="high",
                text="a verify:: task declares owned files — it is a read-only gate and must write nothing",
                evidence=ev,
            ))
    return out
