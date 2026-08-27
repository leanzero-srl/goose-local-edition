#!/usr/bin/env python3
"""Grade the INSTRUCTIONS the swarm hands its nodes, from a run's own event log.

Every existing instrument grades the ARTIFACT. None of them grades what a worker was actually
told, so "the swarm throws generic messages at the nodes" has never been a number. This is that
number, and it is derived only from deterministic fields in `run.jsonl` — never a model opinion.

Four families of check:

  KIND MISMATCH   Each dispatch is classified from its owned files into the job it really has
                  (implementer / test-author / entrypoint / owns-nothing). The shared worker
                  prompt is written for the implementer, so every other kind receives rules
                  authored for a different job. The sharpest case is a test-author, which owns a
                  `test_*.py` and is told "NEVER read the project's TEST files" (swarm.rs:18100)
                  — the file it must produce is the file it may not open. `kind_prompt` in
                  `levers_resolved` says whether the engine was gating rules by kind at the time.

  SPECIFICITY    Per planned task, does its description actually pin the work down: does it name
                  the exact files the dispatch owns, concrete symbols to implement, and a check?
                  A description that names none of these is a generic message regardless of length.

  BLIND SPOTS    A dispatch whose task_id has no `plan_loaded` description was created at runtime
                  (a judge split, a fix worker, a wire-fix). Its instruction is NOT in the log at
                  all, so its quality cannot be measured. That is itself a finding: the 43-char
                  split child documented at scheduler.rs:35 is invisible to every instrument.

  CONTEXT        `context_slice_len == 0` means the worker was given no dependency context.

Also reports fleet collapse: distinct devices that received work vs the pool the engine built.

Usage:
    python3 dispatch_audit.py <run-dir-or-run.jsonl> [...]      human table
    python3 dispatch_audit.py --json <run-dir-or-run.jsonl>     machine-readable
    python3 dispatch_audit.py --self-test                       controls, both directions
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

# da-3: the lever-ON mismatch rate is RETRACTED — it was read off `tailored`, which reports the
# test-author branch alone, so every read-only-shard counted as misinstructed while receiving
# rules written for it. Lever-ON now reads UNMEASURED until a run carries `rules_sections`.
# da-2: kind-mismatch is MEASURED from the engine's `rules_delivered` event instead of
# reading UNMEASURED whenever kind_prompt is ON. A stale row does NOT need its unit re-run —
# `reaudit.py` recomputes the audit from the stored run.jsonl and rewrites the row in place.
AUDIT_VERSION = "da-4"

# A dispatch's KIND is decided by the files it owns, because that is what the engine itself keys
# its behaviour on (owns_nothing, test relaxation, entry wiring). Order matters: a task owning
# both a test and a source file is a test-author, since the test is what it is graded on.
TEST_FILE = re.compile(r"(^|/)(test_[^/]+|[^/]+_test)\.(py|ts|js|go|rs)$")
ENTRY_FILE = re.compile(r"(^|/)(__main__|main|cli|index|app)\.(py|ts|js|go|rs)$")

KINDS = ("implementer", "test-author", "entrypoint", "owns-nothing")


def classify(owned_files: list[str]) -> str:
    if not owned_files:
        return "owns-nothing"
    if any(TEST_FILE.search(f) for f in owned_files):
        return "test-author"
    if any(ENTRY_FILE.search(f) for f in owned_files):
        return "entrypoint"
    return "implementer"


def read_events(path: pathlib.Path) -> list[dict]:
    p = pathlib.Path(path)
    if p.is_dir():
        cands = sorted(p.glob("run.jsonl")) or sorted(p.glob(".swarm/run-swarm-*.jsonl"))
        if not cands:
            raise FileNotFoundError(f"no run.jsonl under {p}")
        p = cands[-1]
    events = []
    for line in p.read_text(errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return events


# --- specificity -------------------------------------------------------------------------------
# Deterministic string checks only. Each one names a concrete way a worker can be under-instructed.

SYMBOL = re.compile(r"\b(def |class |func |fn |interface |type )\w+|\w+\s*\([^)]*\)\s*(->|:)")
CHECK_WORD = re.compile(
    r"\b(must|should) (return|raise|print|exit|handle|reject|accept)\b|"
    r"\btest[s]? (must|should)\b|\bverif|\bassert|\bexpect", re.I)
# What a verifier needs instead of symbols: the concrete invocation it is supposed to run.
COMMAND = re.compile(
    r"\b(python3?\s+-m|go run|go test|cargo run|cargo test|npm run|npx|node|pytest|curl)\b|"
    r"`[^`]*\b(run|test|serve|build|sync|mine)\b[^`]*`", re.I)


# Named, not inline, because selftest.py's cross-instrument ceiling derives from the SAME number:
# a slice whose spec is at least this long cannot possibly ship as a one-liner. Two copies of the
# threshold would let the two instruments disagree about what a one-liner is.
ONE_LINER_MAX_CHARS = 400


def is_skeleton_brief(desc: str) -> bool:
    """Did this worker get a one-liner instead of a module specification?

    THE MECHANISM MOVED BUT THE FAILURE DID NOT. It used to be the detail fan: a best-effort call
    under a hard cap that, on timeout, silently kept the architect's one-liner. That fan is gone. In
    the linear engine each slice OWNER writes the spec and `splice_briefs` writes it into its task's
    `description` VERBATIM — so a one-line description now means either the owner returned nothing
    (RESEARCH substitutes a bare `PURPOSE:` line and the run continues) or the splice matched no
    slice for that task. Both land a worker with the thinnest instruction in the engine, and neither
    emits an event, which is precisely why this is measured off the shipped plan rather than off a
    counter.

    A surviving one-liner is recognisable by shape, not by length alone: a single unstructured line
    with no code block, no bullets and no line breaks.
    """
    d = (desc or "").strip()
    if not d:
        return False
    return "\n" not in d and "`" not in d and len(d) < ONE_LINER_MAX_CHARS


def specificity(desc: str, owned: list[str]) -> dict:
    d = desc or ""
    names_files = bool(owned) and any(f in d or f.split("/")[-1] in d for f in owned)
    return {
        "chars": len(d),
        "names_owned_files": names_files,
        "names_symbols": bool(SYMBOL.search(d)),
        "names_command": bool(COMMAND.search(d)),
        "states_a_check": bool(CHECK_WORD.search(d)),
    }


def specificity_score(s: dict, owns_files: bool = True) -> float:
    """0..1, graded on the criteria that can APPLY to the task.

    An author is pinned down by: the files it owns, the symbols it must produce, and the check its
    work must satisfy. A verifier owns and writes nothing, so files and symbols are the wrong
    questions — it is pinned down by the command it must run and the outcome it must confirm.

    Grading both kinds on the author's criteria would dock every verifier for obeying its own
    spec, and this project's standing law is that a grader's bugs invent defects rather than
    excuse them. Length is deliberately not a criterion: a long vague brief scores zero.
    """
    if owns_files:
        applicable = (s["names_owned_files"], s["names_symbols"], s["states_a_check"])
    else:
        applicable = (s["names_command"], s["states_a_check"])
    return round(sum(applicable) / len(applicable), 3)


def audit(path) -> dict:
    events = read_events(path)
    plan: dict[str, str] = {}
    dispatches: list[dict] = []
    pool: list[dict] = []
    levers: dict = {}
    run_id = None

    for e in events:
        ev = e.get("event")
        if ev == "run_started":
            pool = e.get("pool") or []
            run_id = e.get("run_id")
        elif ev == "levers_resolved":
            levers = e.get("levers") or {}
        elif ev == "plan_loaded":
            for t in e.get("tasks") or []:
                plan[t.get("id")] = t.get("description") or ""
        elif ev == "task_dispatched":
            dispatches.append(e)

    kind_prompt_on = bool(levers.get("kind_prompt"))
    scoped_contracts_on = bool(levers.get("scoped_contracts"))
    split_on = bool(levers.get("split"))

    per_dispatch = []
    for d in dispatches:
        tid = d.get("task_id")
        owned = d.get("owned_files") or []
        kind = classify(owned)
        desc = plan.get(tid)
        measurable = desc is not None
        spec = specificity(desc or "", owned) if measurable else None
        per_dispatch.append({
            "task_id": tid,
            "device": d.get("device"),
            "attempt": d.get("attempt"),
            "kind": kind,
            "owned_files": owned,
            "context_slice_len": d.get("context_slice_len"),
            "instruction_measurable": measurable,
            "specificity": spec,
            "specificity_score": specificity_score(spec, bool(owned)) if spec else None,
        })

    n = len(per_dispatch)
    by_kind = {k: sum(1 for p in per_dispatch if p["kind"] == k) for k in KINDS}
    # The implementer prompt is the one the engine sends when kind_prompt is off, so every other
    # kind is receiving rules written for another job.
    # DEFINITIONALLY CIRCULAR, and it would have fabricated a win. This read
    #     mismatched = n - by_kind["implementer"] if not kind_prompt_on else 0
    # so with the lever ON the count was HARDCODED to zero. The kind_prompt arm's stated readout is
    # "does kind_mismatch_pct fall toward zero", and it would have fallen to exactly zero BY
    # CONSTRUCTION — a guaranteed success that measures nothing, on the very run bought to test it.
    #
    # The honest position: with the lever OFF this is a sound INFERENCE (the engine sends the
    # implementer prompt to everyone, so every non-implementer kind is mismatched). With it ON the
    # engine sends kind-specific rules and this file has no evidence of WHICH rules any dispatch
    # actually received — system prompts are not reliably persisted, so only a deterministic engine
    # event can prove it. `rules_kind` (engine-side) is that event; until a run carries it, the
    # honest answer here is UNMEASURED, never zero.
    # THE EVENT EXISTS AND THIS FILE WAS WAITING FOR A NAME THAT WAS NEVER USED.
    #
    # The comment above asks for a `rules_kind` engine event. The engine emits **`rules_delivered`**
    # — and has been emitting it all along, per dispatch, carrying `task_id`, `kind`, `kind_prompt`
    # and `tailored`. MEASURED: 24 / 22 / 24 / 14 events across four archived runs, in BOTH lever
    # states (kind_prompt True on the think_off runs, False on sink_review and swarm-1node), which is
    # the both-directions control this metric needs. So the headline dispatch-quality number read
    # UNMEASURED for every run on the shipped build purely because the reader looked for the wrong key.
    #
    # WHAT COUNTS AS MISMATCHED, taken from the engine's own fields rather than from an assumption:
    # a dispatch is mismatched when its kind is not `implementer` and the rules it got were not
    # `tailored` to it. With the lever OFF nothing is tailored, so this reduces exactly to the old
    # inference — which is the positive control: sink_review reads 75.0% under the new rule against
    # the 79.2% the inference produced, the small gap being dispatches with no plan entry.
    #
    # ⚠ AND IT DOES NOT FALL TO ZERO WHEN THE LEVER IS ON. `tailored` is `kind_prompt_on &&
    # is_test_author`, so `read-only-shard` and `owns-nothing` still receive the implementer-shaped
    # generic rules. That is the number the campaign actually wants and could not see.
    # 🔴 RETRACTED, ONE TICK AFTER I PUBLISHED IT. Reading mismatch off `tailored` gave 40.9 / 42.3 /
    # 41.7% for the three lever-ON runs. **That number was wrong and it is withdrawn.** `tailored` is
    # `kind_prompt_on && is_test_author` — the TEST-AUTHOR BRANCH ONLY — while the worker prompt is
    # assembled from at least three independently-branching sections, and `owned_part` has a dedicated
    # `read_only_shard && kind_prompt_on` variant whose own comment says "this kind sees FEWER rules,
    # never more". So every read-only-shard dispatch was counted as misinstructed while it was in fact
    # receiving a rule set written specifically for it. L141: a boolean summary can be narrower than
    # the behaviour it summarises.
    #
    # The engine now emits `rules_sections` {owned_part, reading_rules, stopping_rules} naming the
    # variant each section took. Until a run carries it the honest answer for a lever-ON run is
    # UNMEASURED — never a rate derived from one bit of a three-bit question.
    extra: dict = {}
    rules_events = [e for e in events if e.get("event") == "rules_delivered"]
    sectioned = [e for e in rules_events if e.get("rules_sections")]
    if sectioned:
        # da-4 (F851). da-3's any-generic predicate was SATURATED: every non-implementer kind always
        # carried at least one generic-labelled section (test-authors because the owned_part LABEL
        # never branched on is_test_author while the delivered text did; read-only shards because no
        # reading/stopping variant existed), so the headline arithmetically equalled the plan's kind
        # mix (70-81% on every row, verified to the decimal on 10 rows) and could never move. Third
        # incarnation of the same trap (da-1 hardcoded 0, da-2 over-counted off one bit).
        # Two honest numbers instead of one saturated one:
        #   kind_mismatch_pct     — a tailored variant EXISTS for (kind, section) and a generic one
        #                           was delivered anyway. Can reach zero; measures the lever.
        #   untailored_sections_pct — non-implementer sections for which the engine possesses NO
        #                           variant (an engine coverage gap, not a lever failure).
        # The (test-author, owned_part)=='generic' reading on builds before F851 is the LABEL bug,
        # not the prompt (delivery branches on the same predicate as the kind field), so it is
        # counted as a label artifact, never a mismatch.
        GENERIC = {"generic", "kind-generic", "off-generic"}
        TAILORED_EXISTS = {
            ("test-author", "owned_part"), ("test-author", "reading_rules"),
            ("test-author", "stopping_rules"),
            ("read-only-shard", "owned_part"), ("read-only-shard", "reading_rules"),
            ("read-only-shard", "stopping_rules"),
            ("owns-nothing", "owned_part"),
        }
        mismatched = 0
        label_artifact_sections = 0
        untailored_sections = 0
        noimpl_sections = 0
        for e in sectioned:
            kind = e.get("kind")
            if kind == "implementer":
                continue
            hit = False
            for name, v in (e.get("rules_sections") or {}).items():
                noimpl_sections += 1
                if v not in GENERIC:
                    continue
                if kind == "test-author" and name == "owned_part":
                    label_artifact_sections += 1
                elif (kind, name) in TAILORED_EXISTS:
                    hit = True
                else:
                    untailored_sections += 1
            if hit:
                mismatched += 1
        mismatch_n = len(sectioned)
        extra["untailored_sections_pct"] = (
            round(100 * untailored_sections / noimpl_sections, 1) if noimpl_sections else None)
        extra["label_artifact_sections"] = label_artifact_sections
        basis = ("MEASURED (da-4): mismatched = a tailored variant exists for the kind+section and a "
                 "generic one was delivered. Sections with no engine variant are untailored_sections_pct "
                 "(coverage gap, not lever failure); pre-F851 builds mislabel the test-author owned_part "
                 "as generic (label_artifact_sections) and read-only reading/stopping variants only exist "
                 "from F851 on, so older builds read mismatched there by vocabulary, not by regression")
    elif rules_events and kind_prompt_on:
        mismatched, mismatch_n = None, len(rules_events)
        basis = ("UNMEASURED: rules_delivered carries only `tailored`, which reports the test-author "
                 "branch alone; a rate off that bit counted every read-only-shard as misinstructed "
                 "when it receives rules written for it. Needs rules_sections.")
    elif rules_events:
        mismatched, mismatch_n = sum(1 for e in rules_events if e.get("kind") != "implementer"), len(rules_events)
        basis = ("inferred UPPER BOUND: kind_prompt OFF, so reading/stopping rules are the generic set "
                 "for every kind. Called an upper bound because owns-nothing still receives the sink's "
                 "own owned_part paragraph, which is not gated on the lever.")
    elif kind_prompt_on:
        mismatched, mismatch_n = None, n
        basis = ("UNMEASURED: kind_prompt is ON and this run predates the rules_delivered event, so "
                 "the delivered rule-set is not in the log")
    else:
        mismatched, mismatch_n = n - by_kind["implementer"], n
        basis = ("inferred: kind_prompt OFF, so every non-implementer kind receives the implementer "
                 "prompt")

    # A SECOND OPINION ON THE SAME QUESTION, EXPOSED RATHER THAN RECONCILED. `classify(owned_files)`
    # and the engine's own `kind` answer the same question from different sides; if they disagree one
    # of them is wrong and the reader must be told which runs are affected, not handed a silent pick.
    event_kinds = {k: sum(1 for e in rules_events if e.get("kind") == k)
                   for k in {e.get("kind") for e in rules_events}} if rules_events else {}
    kind_disagreement = (sorted(
        (k, by_kind.get(k, 0), event_kinds.get(k, 0))
        for k in set(by_kind) | set(event_kinds)
        if by_kind.get(k, 0) != event_kinds.get(k, 0)) if rules_events else [])

    # The named contradiction: owns a test file AND is told never to read test files.
    contradiction = sum(
        1 for p in per_dispatch
        if p["kind"] == "test-author" and not kind_prompt_on)

    unmeasurable = [p["task_id"] for p in per_dispatch if not p["instruction_measurable"]]
    zero_ctx = sum(1 for p in per_dispatch if (p["context_slice_len"] or 0) == 0)

    # ONLY THE QUALITY NUMBER SURVIVES, AND THAT IS THE RIGHT ONE TO KEEP. There used to be a second,
    # `detail_fallback` events, counting DETAIL-CALL FAILURES in any planning round — plus a third,
    # the GHOST split, for a failure on a task belonging to a draft that was later thrown away. The
    # detail fan and the redraft ladder are both gone, so the mechanism counter has no event to read
    # and no draft can be discarded: every one of those keys could only have printed 0 or [] forever,
    # which reads as "the detail call never failed" rather than "there is no detail call". Removed.
    #
    # `shipped_one_liners` is measured off the final `plan_loaded` — WHAT A WORKER ACTUALLY RECEIVED —
    # and that is unchanged by the rewrite. It is also the number that predicted a bad build: across
    # the two runs whose scores differed most, the client module got 1497 chars containing the
    # vendor's /v1 prefix and scored 88.7%, or 122 chars without it and scored 42.7%.
    briefed = sorted({tid for tid, desc in plan.items() if is_skeleton_brief(desc)})

    scored = [p["specificity_score"] for p in per_dispatch if p["specificity_score"] is not None]
    lens = sorted(p["specificity"]["chars"] for p in per_dispatch if p["specificity"])

    devices_used = sorted({p["device"] for p in per_dispatch if p.get("device")})

    return {
        "audit_version": AUDIT_VERSION,
        "run_id": run_id,
        "path": str(path),
        "dispatches": n,
        "pool_size": len(pool),
        "devices_that_did_work": devices_used,
        "levers": {
            "kind_prompt": kind_prompt_on,
            "scoped_contracts": scoped_contracts_on,
            "split": split_on,
        },
        "kind_counts": by_kind,
        "kind_mismatch_count": mismatched,
        "kind_mismatch_pct": (round(100.0 * mismatched / mismatch_n, 1)
                              if (mismatch_n and mismatched is not None) else None),
        # Says WHERE the number came from, so a reader never mistakes an inference for a measurement.
        "kind_mismatch_basis": basis,
        **extra,
        "kind_counts_from_events": event_kinds,
        "kind_source_disagreement": kind_disagreement,
        "test_author_contradiction_count": contradiction,
        "instruction_unmeasurable_count": len(unmeasurable),
        "instruction_unmeasurable_tasks": sorted(set(unmeasurable)),
        "shipped_one_liners": len(briefed),
        "shipped_one_liner_tasks": briefed,
        # Kept under the old key so existing result rows stay readable; it IS the shipped count.
        "detail_fallback_count": len(briefed),
        "detail_fallback_tasks": briefed,
        "planned_tasks": len(plan),
        "zero_context_slice_count": zero_ctx,
        "specificity_mean": round(sum(scored) / len(scored), 3) if scored else None,
        "specificity_min": min(scored) if scored else None,
        "instruction_chars_min": lens[0] if lens else None,
        "instruction_chars_median": lens[len(lens) // 2] if lens else None,
        "instruction_chars_max": lens[-1] if lens else None,
        "per_dispatch": per_dispatch,
    }


def render(a: dict) -> str:
    out = []
    out.append(f"=== {a['path']}  ({a['audit_version']})")
    out.append(f"  pool built by engine : {a['pool_size']} device(s)")
    out.append(f"  devices that worked  : {', '.join(a['devices_that_did_work']) or '(none)'}")
    lv = a["levers"]
    out.append(f"  levers               : kind_prompt={lv['kind_prompt']} "
               f"scoped_contracts={lv['scoped_contracts']} split={lv['split']}")
    out.append(f"  dispatches           : {a['dispatches']}")
    kc = a["kind_counts"]
    tot = a["dispatches"] or 1
    out.append("  kind mix             : " + "  ".join(
        f"{k}={kc[k]} ({100.0 * kc[k] / tot:.1f}%)" for k in KINDS))
    if a["kind_mismatch_count"] is None:
        out.append(f"  KIND MISMATCH        : UNMEASURED — {a['kind_mismatch_basis']}")
    else:
        out.append(f"  KIND MISMATCH        : {a['kind_mismatch_count']} "
                   f"({a['kind_mismatch_pct']}%) got rules written for another job")
    out.append(f"  test-author told 'never read TEST files' : "
               f"{a['test_author_contradiction_count']}")
    out.append(f"  SHIPPED one-liners (what workers got): "
               f"{a['shipped_one_liners']} of {a['planned_tasks']} tasks {a['shipped_one_liner_tasks']}")
    out.append(f"  instruction NOT in log (split/fix children): "
               f"{a['instruction_unmeasurable_count']} {a['instruction_unmeasurable_tasks']}")
    out.append(f"  zero dependency context : {a['zero_context_slice_count']} of {a['dispatches']}")
    out.append(f"  specificity  mean={a['specificity_mean']} min={a['specificity_min']}"
               f"   chars min={a['instruction_chars_min']} "
               f"med={a['instruction_chars_median']} max={a['instruction_chars_max']}")
    worst = sorted((p for p in a["per_dispatch"] if p["specificity_score"] is not None),
                   key=lambda p: (p["specificity_score"], p["specificity"]["chars"]))[:4]
    if worst:
        out.append("  weakest instructions:")
        for p in worst:
            s = p["specificity"]
            out.append(f"    {p['task_id']:<18} score={p['specificity_score']} "
                       f"chars={s['chars']:<5} files={s['names_owned_files']} "
                       f"symbols={s['names_symbols']} check={s['states_a_check']}")
    return "\n".join(out)


def self_test() -> int:
    """Controls in BOTH directions. A grader that cannot fail is not a grader."""
    fails = []

    def check(name, got, want):
        if got != want:
            fails.append(f"{name}: got {got!r}, want {want!r}")

    check("classify implementer", classify(["vendorsync/store.py"]), "implementer")
    check("classify test-author", classify(["tests/test_store.py"]), "test-author")
    check("classify test-author mixed", classify(["a.py", "test_a.py"]), "test-author")
    check("classify entrypoint", classify(["vendorsync/__main__.py"]), "entrypoint")
    check("classify owns-nothing", classify([]), "owns-nothing")

    # Specificity must be LOW on the real 43-char split child and HIGH on a real detailed spec.
    thin = specificity("(split of data-model-persistence) note-store", ["store.py"])
    check("thin split child scores 0", specificity_score(thin), 0.0)

    rich = specificity(
        "Owns exactly `vendorsync/store.py`. Implement `class Store` with "
        "`def upsert_many(self, payments: list[dict]) -> int`. Tests must assert a re-sync "
        "inserts 0 new rows.",
        ["vendorsync/store.py"])
    check("rich spec scores 1", specificity_score(rich), 1.0)

    # Vacuous truth: an empty description must score ZERO, never full marks.
    check("empty desc scores 0", specificity_score(specificity("", ["a.py"])), 0.0)

    # A long but vague description must NOT score well just for being long.
    vague = specificity("Build the thing well. " * 80, ["store.py"])
    check("long vague scores 0", specificity_score(vague), 0.0)

    # An owns-nothing verifier is graded only on the checks that can apply to it, and a fully
    # specified one must be able to reach 1.0 — otherwise the whole class carries a fake deduction.
    ver = specificity(
        "Run `python3 -m vendorsync` and assert the summary total equals EUR 1,234.56.", [])
    check("owns-nothing verifier can reach 1.0", specificity_score(ver, owns_files=False), 1.0)
    # A verifier told only to "check it works" names no command and must score low, or the
    # criterion is decorative.
    lazy = specificity("Verify the module behaves correctly.", [])
    check("verifier with no command scores 0.5", specificity_score(lazy, owns_files=False), 0.5)
    check("verifier with neither scores 0",
          specificity_score(specificity("Look at the module.", []), owns_files=False), 0.0)

    # The surviving-brief detector must fire on the REAL 95-char meridian brief that shipped a
    # 44% run, and must NOT fire on the 2.4k detailed spec that shipped a 90% run.
    check("brief detector fires on the real 95-char meridian brief",
          is_skeleton_brief("Meridian API client: cursor pagination, ETag caching, "
                            "429/410 retry, create_payment idempotency"), True)
    check("brief detector ignores a real detailed spec",
          is_skeleton_brief("### Subtask: MeridianClient\n\n**Owned file:** `meridian.py`"), False)
    check("brief detector ignores an empty description", is_skeleton_brief(""), False)
    check("brief detector ignores a long one-liner with code",
          is_skeleton_brief("Implement `class Store` in store.py"), False)

    # The positive path of names_owned_files must actually fire, or the check is dead weight.
    named = specificity("Owns exactly: `vendorsync/store.py`.", ["vendorsync/store.py"])
    check("names_owned_files fires on a real match", named["names_owned_files"], True)
    basename = specificity("write store.py fully", ["vendorsync/store.py"])
    check("names_owned_files fires on a basename", basename["names_owned_files"], True)
    check("names_owned_files stays False when absent",
          specificity("build the store layer", ["vendorsync/store.py"])["names_owned_files"], False)

    # An empty run must produce zeros, not crash and not full marks.
    empty = {"event": "run_finished"}
    import tempfile
    with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False) as fh:
        fh.write(json.dumps(empty) + "\n")
        tmp = fh.name
    a = audit(tmp)
    check("empty run: 0 dispatches", a["dispatches"], 0)
    check("empty run: no specificity", a["specificity_mean"], None)

    if fails:
        print("SELF-TEST FAILED:")
        for f in fails:
            print("  -", f)
        return 1
    print(f"self-test OK ({AUDIT_VERSION}) — classification, both score directions, "
          "vacuous-truth and empty-input controls all pass")
    return 0


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    if "--self-test" in argv:
        return self_test()
    if not args:
        print(__doc__)
        return 2
    as_json = "--json" in argv
    results = [audit(a) for a in args]
    if as_json:
        print(json.dumps(results if len(results) > 1 else results[0], indent=1))
    else:
        for r in results:
            print(render(r))
            print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
