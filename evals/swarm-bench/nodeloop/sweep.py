#!/usr/bin/env python3
"""The unattended loop: does the swarm get BETTER with more nodes, and are its instructions specific?

Node count became measurable on 2026-08-01 when the fleet was given three distinct LM Studio
identifiers. Before that all three hosts served one identifier, LM Studio exposed exactly one
addressable worker, and every run labelled 1node/2node/3node built the same 1-device pool — so the
project's "more nodes make it worse" table compared a configuration with itself. Proven at the time
by a concurrency probe: three simultaneous calls were all served by one host while the other two
never left idle. Re-proven after the re-identification: three concurrent calls, one per identifier,
put ALL THREE instances into `generating` at once.

So this loop measures two things that were previously unmeasurable and are the whole of goal one:

  NODES        does build quality and fleet occupancy actually improve at 2 and 3 nodes
  DISPATCH     is each node given a SPECIFIC instruction, or a generic one

The second is not a side question. Three runs of an identical 1-node config scored 44.2 / 86.7 /
90.0% — a 46-point spread — and the spread tracked exactly how many workers got the architect's
one-liner instead of a detailed spec (2, 1 and 0 respectively). Any node-count effect must clear
that spread to mean anything, which is why every cell is replicated and why the mechanism counts
from dispatch_audit.py matter more than the score.

Operating rules below each cost a real overnight run at some point:
  - a result not on disk did not happen: every unit persists its result the moment it finishes
  - resumable: a completed unit is skipped, so a killed loop resumes where it stopped
  - one bad unit never kills the sweep, and SystemExit is NOT an Exception
  - a fleet blip is retried with backoff, never recorded as a score of zero
  - a flat timeout measures the timeout: timed_out is recorded and checked before interpreting
  - children die by process GROUP, or an orphan contends for the fleet unnoticed
  - a unit whose ACTUAL pool differs from the one it asked for is VOID, never averaged in
  - it ends only on the STOP sentinel, never on a counter
"""
from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
import traceback
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent / "bench"
sys.path.insert(0, str(BENCH))   # run_build, score_build, vendor_service
sys.path.insert(0, str(HERE))    # nodeloop's own instrument — must precede BENCH on the path

import dispatch_audit  # noqa: E402
import prefix  # noqa: E402

OUT = HERE.parent / "runs" / "nodeloop"
STOP = HERE / "STOP"
QUEUE = HERE / "QUEUE"
PORT_BASE = 8930
TIMEOUT = 16200          # 4.5h. A cap that truncates the work measures the cap, not the entrant.
MIN_REPS = 3             # n=1 is uninterpretable against a measured 46-point spread.
TRANSIENT = ("500", "502", "503", "529", "overloaded", "rate limit", "throttl",
             "connection reset", "stream decode", "temporarily", "unreachable")
MAX_ATTEMPTS = 3
BACKOFF = (60, 240)

WATCHDOG_POLL_SECS = 60
HEARTBEAT_STALE_SECS = 600   # the engine writes it every 5s; 10 min dead is wedged, not busy
MIN_FREE_GB = 15

# Each arm varies exactly ONE thing against baseline, and carries a prediction written down BEFORE
# the run, where it can fail.
ARMS = [
    {
        "name": "baseline",
        "env": {},
        "gate": "establishes the replicate spread, the detail-fallback rate, and the node curve. "
                "Re-measured rather than assumed: a stale baseline turns fleet drift into a false win.",
    },
    # scoped_contracts was REMOVED from this queue before it ever ran, and that removal was WRONG —
    # not in its measurement, which was correct, but in the population it generalised from (F159).
    #
    # The old note said: the architect is told "Default to a FLAT FAN: make every module a root with
    # no deps" (swarm.rs:11493), so a worker's DAG neighborhood is just itself, so scoping the bundle
    # would delete every sibling interface and leave only the module's own stub. All true — OF
    # IMPLEMENTERS. Measured on the live plan, per kind:
    #
    #     implementer   5/5 have an EMPTY neighborhood  -> `!req.neighborhood.is_empty()` fails, INERT
    #     test-author   0/3 empty (each depends on the ONE module it tests)          -> LIVE
    #     verify/sink   0/9 empty                                                    -> LIVE
    #
    # The flat fan gives tests and verifiers a neighborhood BY CONSTRUCTION — a test must depend on
    # the thing it tests. So the lever is inert for 5 of 17 tasks and live for 12, and the 12 include
    # every test-author: the exact population F156/F159 measure at 22,511-char prompts (2.3x the
    # implementer), 3.3x the dry reasoning, and 3 of 3 of this run's judge interventions.
    #
    # What it would actually cut: the `test_meridian.py` author currently receives `## API of` blocks
    # for ALL FIVE modules — 265 lines of implementation body against 35 signature lines, including
    # six private methods (`_get_with_429`, `_handle_429`, `_send_json`, `_headers`, `_existing_ids`,
    # `_do_request_with_429_retry`). Its neighborhood is `[meridian]`.
    {
        "name": "scoped_contracts",
        "reps": 3,
        "env": {"GOOSE_SWARM_SCOPED_CONTRACTS": "1"},
        "gate": "READOUT IS ON TEST-AUTHORS ONLY — implementers are 5/5 empty-neighborhood and the "
                "lever cannot touch them, so pooling the kinds would dilute a real effect into "
                "nothing (that pooling error is F156, and re-making it here is what this arm exists "
                "to avoid). Report, for test-author dispatches only: (1) system-prompt chars, vs the "
                "22,511 baseline; (2) `## API of` blocks delivered, vs 5; (3) max dry reasoning "
                "before the first owned write, vs a 3,402 median / 24,032 max; (4) judge "
                "interventions on test-authors, vs 3. A cut in (1) and (2) with NO improvement in "
                "(3) refutes the mechanism hypothesis and is the result that matters most — it would "
                "mean prompt volume is not what delays the first write, and the next suspect is the "
                "'DON'T OVER-READ / there is nothing further to look up' instruction sitting next to "
                "12k of readable code. VOID the arm if any test-author reports an empty neighborhood.",
    },
    # think_off — the prefill arm. The ONLY queued lever with BEHAVIOURAL evidence rather than a
    # mechanism argument: measured on this fleet at real prompt size, a 22,187-char worker prompt took
    # 47.3 s and produced 974 characters of prose and NO tool call by default, against 3.1 s with the
    # tool call as the FIRST token once the thinking block was pre-closed (F216).
    #
    # Everything else in this queue is a hypothesis about what the model SHOULD do. This one was
    # watched doing it.
    {
        "name": "think_off",
        "reps": 3,
        # BOTH LEVERS, ONE ARM — a deliberate departure from F204's "test them apart".
        #
        # F204 is right when the question is ATTRIBUTION. It is the wrong rule when the question is
        # whether the metric moves AT ALL, and after 16 stalled ticks that is the question. The two
        # levers attack the two failure modes actually observed in test-authors, and they are
        # different modes, not two guesses at one:
        #   THINK_OFF      -> the worker that GENERATES but only reasons (F216: 47.3s/no tool call
        #                     -> 3.1s/tool call first). Proven to reach the wire (F220).
        #   DEP_SIGNATURES -> the worker that never generates at all because LM Studio is still
        #                     PROCESSING a 22,511-char prompt (F223: thinking_chars None for 390s
        #                     means the digest is still the seed). 10,097 of those chars — 50.7% —
        #                     are `## API of` dependency BODIES, 3 of 4 truncated mid-token, one
        #                     failing ast.parse outright (F196). That is a broken artifact, and a
        #                     broken artifact is a BUG, not a tuning preference.
        #
        # If the row moves, a follow-up arm with one lever separates them. If it does not move with
        # BOTH on, neither is the cause and the search goes elsewhere — which is worth more than a
        # clean null on either alone.
        "env": {"GOOSE_SWARM_THINK_OFF": "1", "GOOSE_SWARM_DEP_SIGNATURES": "1"},
        "gate": "STREAMING IS THE FALSIFIER AND IT DECIDES THE WHOLE ROUTE. The 47.3s->3.1s evidence "
                "was measured NON-streaming; goose ALWAYS streams. FIRST, before any score: in "
                "`llm_request.*.jsonl` the LAST element of `messages` must be role:'assistant' "
                "carrying `<think>\n\n</think>\n\n`, ON EVERY REQUEST of a worker's loop and not "
                "merely the first — `format_messages_with_options` may merge, drop or reorder a "
                "synthetic trailing assistant, and turn>=2 after a `tool` message is untested. "
                "SECOND: the swarm's own thinking accumulator must read 0 for those dispatches. "
                "MESSAGE PRESENT BUT THINKING STILL NON-ZERO ⇒ the server is not taking the "
                "continuation path under streaming ⇒ THE ROUTE IS DEAD, and that is the most "
                "valuable outcome because it closes the lead rather than leaving it half-tested. "
                "ONLY IF BOTH PASS does the test-author row mean anything: `python3 goalstate.py` "
                "reports the p-value; n=5 clean is p=0.157 and NINE clean completions are needed to "
                "clear p<0.05. VOID the arm if any worker's request lacks the trailing assistant.",
    },
    # dep_signatures — the arm the LIVE scoped_contracts run argued for, which is not the same arm.
    #
    # Measured on `scoped_contracts-n3-r0` while it ran (F196): the lever IS armed and IS working, and
    # it cut the test-author prompt from 22,511 to 20,552 chars — 8.7%, one `## API of` block of five.
    # That is the whole effect available to it, because a test-author's DAG neighborhood is genuinely
    # most of the app: it imports what it tests AND that module's collaborators. Scoping the COUNT of
    # blocks was the wrong axis. The blocks that remain are 10,097 chars = **50.7% of the entire
    # prompt**, and each one is the dependency's FULL SOURCE — 6 private methods against 5 public ones,
    # under a header that says "## API of" and "do NOT `cat` it".
    #
    # swarm.rs:19628 already has the right mechanism and it is switched off: `dep_signatures_on()`
    # swaps the pasted body for `goose_swarm::extract_signatures` (coherence.rs:34 — "Function/method
    # BODIES are removed; type, const and var declarations are kept as-is"), with a fallback to the
    # full body when extraction finds nothing, so ON can never inject an empty API. It targets the
    # 50.7%, not the 8.7%.
    {
        "name": "dep_signatures",
        "reps": 3,
        "env": {"GOOSE_SWARM_DEP_SIGNATURES": "1"},
        "gate": "ARM-ARMED CHECK FIRST, and per F194 read BOTH addresses: `levers_resolved."
                "dep_signatures` (swarm.rs:22140 emits it) AND `run_started.gates`, requiring a "
                "control run to differ on whichever is non-null. READOUT ON TEST-AUTHORS ONLY. "
                "Registered BEFORE the run: (1) `## API of` bytes, vs the measured 10,097 = 50.7% of "
                "prompt; (2) private methods pasted, vs 6 against 5 public; (3) blocks whose fenced "
                "body FAILS `ast.parse`, vs 1 of 4 today (meridian.py is cut at `def _up`); (4) max "
                "dry reasoning before the first owned write, vs 3,402 median / 24,032 max; (5) the "
                "test-author row of `failures.py`, vs 42 completed / 13 failed = 31%. "
                "THE FALSIFIER: if (1) and (2) fall sharply and (4)+(5) do NOT move, prompt volume is "
                "not the cause of the test-author failure rate and this whole thread — F156, F159, "
                "F164, F196 — has been chasing a correlate. That is the outcome worth the run. "
                "VOID if extraction falls back to the full body on every dependency (an empty "
                "`extract_signatures` return means the language was not recognised, not that the lever "
                "worked).",
    },
    {
        # Both LOSING 3-node units split their single most-detailed task; the 0.8708 winner never
        # split at all. On a split, `child_description()` (scheduler.rs:70-99) returns the literal
        # `"(split of <parent>) <child-id>"` — about 35 characters replacing a ~3833-char detailed
        # spec — unless this lever is on. It is env-ONLY: there is no config field, so the desktop
        # path can never correct it, while `split` itself IS config-reachable and defaults to on.
        #
        # ⚠ THE ARM IS INVALID IF NOTHING SPLITS. The mechanism is stochastic (the judge only splits
        # when a device is free and the task crosses a 300s threshold), and an arm that measures a
        # mechanism which never fired has already been recorded once as "ARM INVALID — splits=0".
        # ASSERT `task_split > 0` in the treatment runs before reading any score from this cell.
        "name": "split_inherit_spec",
        "env": {"GOOSE_SWARM_SPLIT_INHERIT_SPEC": "1"},
        "gate": "a split child's whole task statement is ~35 chars — the parent's detailed spec is "
                "discarded at the moment of use. Both 3-node losers split their most-detailed task; "
                "the 1-node winner never split. Readout: task_split > 0 (else the arm is VOID) and "
                "the score against the baseline's own replicate spread.",
    },
    {
        # The SEPARATE hypothesis. "Splitting is harmful" and "splitting with an amputated spec is
        # harmful" are different claims and only the second is fixed by split_inherit_spec. Running
        # just the lever would leave the first untested and let a null be read as "splitting is fine".
        "name": "split_off",
        "env": {"GOOSE_SWARM_SPLIT": "0"},
        "gate": "the control for split_inherit_spec. If disabling splitting entirely beats both the "
                "baseline AND the inherit-spec arm, the defect is the split decision itself and not "
                "the spec it throws away.",
    },
    {
        # A phantom generator until F134: the reviewer was handed chars().take(2400) with no marker,
        # and reported api.py's handlers "not defined" when every one of them sits past char 2561 of
        # a 5731-char file. That finding was persisted and injected into the SINK as a fix ORDER.
        # Fixed by routing through review_file_excerpt. This arm now asks the REMAINING question:
        # with the phantom gone, is pre-review worth its slot at all? 2 of 4 archived findings were
        # genuine catches, so a null here means KEEP it, not delete it.
        "name": "prereview_off",
        "env": {"GOOSE_SWARM_PREREVIEW": "0"},
        "gate": "prereview findings rank-ordered PERFECTLY with score across the three archived "
                "units (0 findings -> 0.8708, 1 -> 0.7186, 3 -> 0.6720) and the path is gated on "
                "spare capacity, so it fires MORE with more nodes. Measures the damage that remains "
                "after the F134 truncation fix.",
    },
    {
        # THE RESEARCH SYNTHESIS ASKED FOR THIS ON *EVERY* ARM AS A "FREE INSTRUMENT". IT IS NOT ONE.
        #
        # Read the enforcement site (swarm.rs:19137-19165): just before the sink reads the tree, the
        # fence RESTORES every owned file a non-owner clobbered back to the owner's bytes, via
        # `write_frozen_bytes`. That is a TREATMENT — it changes the tree the sink verifies.
        #
        # Its own comment says "OFF (or no snapshots) => byte-identical", and that is exactly what
        # makes the "free instrument" argument circular: it is free ONLY IF violations are zero, which
        # is the very thing the probe is meant to measure. Same shape as F111's circular readout.
        # Putting it on all four score arms would confound every one of them in precisely the case
        # where it had something to say.
        #
        # So it runs as its OWN n=1 mechanism cell, like every other mechanism readout here. A
        # contaminated tree is fine in a cell whose output is an event count, not a score.
        "name": "owned_file_fence",
        "env": {"GOOSE_SWARM_OWNED_FILE_FENCE": "1"},
        "gate": "does cross-worker clobbering happen AT ALL? 76 archived runs contain ZERO "
                "owned_file_violation events — because the detector has never been switched on, so "
                "that zero is uncontrolled and means nothing. The scheduler already prevents the "
                "common case (held_files/files_conflict make two tasks owning one file "
                "un-coschedulable), leaving only out-of-scope writes. Readout: the violation count. "
                "ZERO across a 3-node run CLOSES 'more nodes -> more interference' as a hypothesis; "
                "non-zero makes it the first thing to fix.",
    },
    {
        # THE ENGINE TELLS ITS ARCHITECT TO MAKE THE PLAN NARROW, AND IT OBEYS PERFECTLY.
        #
        # `converge` is ON by default (swarm.rs:1033, part of the golden bake). With it on, the
        # architect receives BOTH of these:
        #   homo_hint    "Commit to the SIMPLEST CANONICAL decomposition: the FEWEST cohesive modules
        #                 that fully cover the spec ... Do NOT over-split; do NOT invent extra modules."
        #   count_clause "decompose into the FEWEST cohesive module subtasks ... target is usually
        #                 {worker_count} to 2x {worker_count}"
        # With it OFF, the homogeneous branch says the OPPOSITE — "Split AGGRESSIVELY into many fine
        # independent subtasks — do NOT fear interface divergence" — and the count target becomes
        # 2x-3x worker_count.
        #
        # MEASURED across 19 archived plans, module counts are 3,3,3,4,4,4,4,4,4,4,5,5,5,5,5,5,5,6 —
        # EVERY ONE inside [worker_count, 2x worker_count]. The instruction is not aspirational; it is
        # obeyed exactly. Median 4 modules against a fleet of SIX concurrent slots (3 nodes x
        # PARALLEL 2), and modules are the level-0 roots — the only tasks runnable at t=0.
        #
        # Its own comment says why it exists: to make independently-drafted plans CONVERGE so
        # `plan_agreement` scores higher. That is an INTERNAL metric, and narrowing the plan to raise
        # it is the engine trading the thing being measured for the measurement.
        #
        # HONEST RISK, stated before the run: converge is described as "the proven agreement raiser",
        # and lower agreement drives the redraft ladder, which is ~40 min of prefix when it fires. So
        # this arm can lose on wall-clock even if it wins on width. BOTH must be read.
        "name": "converge_off",
        "env": {"GOOSE_SWARM_CONVERGE": "0"},
        "gate": "the default config instructs the architect to emit the FEWEST modules, and 19 of 19 "
                "archived plans obeyed (3-6 modules, median 4, against 6 concurrent slots). This is "
                "the most direct engine-side limit on how much parallelism a plan can express. "
                "Readouts: module count per plan, max antichain width, execute occupancy, AND the "
                "prefix — if agreement falls the redraft ladder costs back what width gains.",
    },
    {
        "name": "kind_prompt",
        "env": {"GOOSE_SWARM_KIND_PROMPT": "1"},
        "gate": "72-80% of dispatches receive rules written for another job, and 3-5 per run own a "
                "test_*.py while being told never to read test files. Gating rules by task kind "
                "should drive kind_mismatch_pct toward zero. A prior adversarial pass refuted the "
                "naive version and put score recovery in single digits, so the MECHANISM count is "
                "the readout, not the build score.",
    },
    {
        "name": "doc_prefetch",
        "env": {"GOOSE_SWARM_DOC_PREFETCH": "1"},
        "gate": "the ONLY verbatim research->worker channel. Inert by construction until F78 - its "
                "grounded filter could never match on a bench with no MCP tools. Readout: doc_facts "
                "non-empty, and the external literals research reported reaching a worker unchanged.",
    },
    {
        "name": "spec_repair",
        "env": {"GOOSE_SWARM_SPEC_REPAIR": "1"},
        "gate": "the ONE mechanism found that puts three nodes on a one-finding round. The tail is "
                "13-26% of every run and has NEVER gone green — `passed` false in 13 of 13 archived "
                "rounds, with findings RISING in 3 of them because the default fix path writes into "
                "the real tree with nothing verifying the edit. This races one attempt per node in "
                "its own shadow and promotes only a twin whose re-verify STRICTLY beats the round's "
                "baseline. TWO readouts, and they are independent: (1) mechanism — does "
                "spec_repair_wave fire with twins>1 and does complete_fix_dispatched finally give "
                "the tail an occupancy number at all; (2) safety — `winner_findings` must never "
                "exceed `baseline_findings`, which is the property the unit test asserts and this "
                "is the live check of it. A round where NOTHING is promoted is a PASS for the "
                "safety readout, not a failure of the mechanism.",
    },
    # detail_budget is BLOCKED and STALE — armcheck.py flags it, and reading it shows why it is worse
    # than merely inert. It sets the budget to 300s. F49 already made the budget DERIVE from
    # worker_timeout_secs, which resolves to 420s on this fleet, and the baseline's slowest detail call
    # is 161s — 38% of the ceiling. So the arm would LOWER a ceiling nothing is near, i.e. it could only
    # ever make things worse, and its gate text still argues against a 75s literal that no longer
    # exists. Left in the list with reps 0 so the reasoning survives; requeue only if a detail call is
    # measured near its budget.
    {
        "name": "detail_budget",
        "reps": 0,
        "env": {"GOOSE_SWARM_DETAIL_BUDGET_SECS": "300"},
        "gate": "the 75s detail budget is a bare literal pinned at the OBSERVED MAXIMUM of the call "
                "it bounds, so normal variance lands on the far side of it: the SAME meridian brief "
                "was detailed in 44.5s on one run and blew through 75s on another, and the run that "
                "lost it shipped a 95-char spec for the module tier C grades (14.3% vs 85.7%). The "
                "sibling contract fanout already abandoned a small fixed budget for "
                "worker_timeout_secs.max(120) after a mass stub failure. PREDICTION: "
                "detail_fallback_count goes to ~0 and pre-execute wall grows only slightly, because "
                "the budget is a ceiling on the slow tail, not the mean (~50s). If fallbacks do NOT "
                "drop, the cause is not the ceiling and this whole line of reasoning is wrong.",
    },
    {
        "name": "complete_parallel",
        "env": {"GOOSE_SWARM_COMPLETE_PARALLEL": "1"},
        "gate": "MEASURED live on baseline-n3-r0: the COMPLETE/repair phase ran 20 of 88 minutes — "
                "22% of the run — on ONE node, with two of three idle for all of it "
                "(smoke_fix_target = devices.first(), swarm.rs:21260). Two independent calculations "
                "agree: occupancy.py put solo-node time at 1174.6s and the gap from the last "
                "task_completed was 19.6 min. This is the phase the project's own ledger fingers as "
                "'REPAIR is what fails', and the lever to fan it across the fleet already exists and "
                "defaults OFF. PREDICTION: wall time falls by roughly the repair tail's idle share "
                "and the build score is UNCHANGED within the replicate spread — this buys fleet "
                "utilisation, not correctness. If the score MOVES, the parallel fix path is not "
                "equivalent to the serial one and that is a defect worth more than the speedup.",
    },
    {
        "name": "e2e_oracle",
        "env": {"GOOSE_SWARM_E2E_ORACLE": "1"},
        "gate": "fan_e2e does not currently partition: e2e_shard_spec tells each shard to number the "
                "advertised commands 'in the order the spec gives them' and never gives it the spec, "
                "so each derives the list from the README the build itself wrote. MEASURED on one "
                "run: three shards derived lists of length 1, 1 and 3, and the one that enumerated "
                "an empty slice reported clean. This arm hands every shard the SAME engine-extracted "
                "table from spec_frozen. PREDICTION: tier C and the e2e-derived checks rise, because "
                "the shards start checking the operator's endpoints rather than the build's own "
                "documentation — and crucially the shards' reports should stop citing the README. If "
                "tier C does NOT move but the reports stop citing the README, the oracle landed and "
                "the app was already right; if the reports still cite the README, the injection is "
                "not reaching them and the arm has failed regardless of the score.",
    },
    # doc_prefetch was here and is PULLED, not deprioritised: it forwards only findings where
    # `grounded == is_mcp && ok`, and research_tools reports available: [] on every run this machine
    # has ever produced, because the research extensions are context7 and web-search and neither key
    # exists. With no grounded finding the block is empty and the worker prompt is byte-identical to
    # baseline, so the arm cannot fire. An arm that cannot fire is not evidence, and running it would
    # spend hours of fleet time to produce an INERT result. doc_fetch replaces it with a fetch that
    # needs no extension and no key.
    {
        "name": "retarget_off",
        "env": {"GOOSE_SWARM_RETARGET": "0"},
        "gate": "THE most expensive mechanism in the engine, judged on its INTENT rather than its own "
                "metric. It exists to make the build functional and predictable; what it optimises is "
                "cross-draft AGREEMENT, which is literally whether the plan drafts emitted task counts "
                "within 1 of each other (spread 1 = 88, spread 0 = 100; spec_clarity scores 100 in "
                "every run and never binds). Four of five shipped plans scored exactly 88 — including "
                "one that never redrafted — and the three with build scores went 88.7 / 50.0 / 42.7 at "
                "identical confidence. MEASURED LIVE (F50): three rounds, 68->80->81, best_of_n "
                "3->4->5->6, NEVER reached the 85 floor, 48.4 MINUTES before any dispatch, and 27,806 "
                "chars of model-authored spec discarded — while detail_fallback fired 13x and the "
                "module owning the vendor contract lost its spec four times. "
                "PREDICTION: build score UNCHANGED within the replicate spread, wall-clock down "
                "10-20%, and the pre-dispatch prefix roughly halved. If the score DROPS below the "
                "spread, the redraft buys something real that this reading missed and it stays — that "
                "is the outcome that would make this arm worth more than a speedup.",
    },
    {
        "name": "sink_review",
        "env": {"GOOSE_SWARM_SINK_REVIEW": "1"},
        "gate": "the SINK owns 100% of the solo window in 2 of 3 measured runs — 543-1045s with two "
                "nodes idle while integrate-verify runs alone — and this is the only mechanism built "
                "to fill it. It has never run once: the scheduler's producer defaulted OFF while the "
                "drain and levers_resolved both defaulted ON, so every run REPORTED it enabled and its "
                "queue was never filled. Both halves now read one resolver, default OFF, so this arm "
                "is the first time the mechanism executes at all. "
                "PREDICTION: `sink_review` fires with prewarmed > 0, and solo_by_task['integrate-"
                "verify'] falls because the idle nodes are doing read-only dimension reviews instead "
                "of nothing. The findings are ADVISORY and re-verified fail-closed against the final "
                "tree, so the build score should NOT move outside the replicate spread — if it moves "
                "DOWN, the re-verification is not fail-closed and that is worth more than the "
                "utilisation. If prewarmed is 0 with the lever on, the producer still cannot see its "
                "precondition and the fix is incomplete.",
    },
    {
        "name": "doc_fetch",
        "env": {"GOOSE_SWARM_DOC_FETCH": "1"},
        "gate": "THE measured coin flip. Three baseline units, identical config: the one whose "
                "plan_loaded carried the vendor's /v1 prefix scored 88.7% and the two that did not "
                "scored 50.0% and 42.7% with every vendor call returning 404. The prefix appears six "
                "times in the document the spec points at and zero times in the spec, and no scout "
                "has ever had a tool to open it. This arm has the ENGINE fetch that document and "
                "splice it verbatim into the planner's channel and every worker's. "
                "PREDICTION: doc_fetched{ok:true} fires, /v1 appears in plan_loaded, and crunch.py's "
                "fetch_all_payments returns 247 rather than raising 404. The mechanism claim is "
                "settled by the first two regardless of the score — if the paths are still wrong "
                "with a 200-status fetch on record, the splice is not reaching the decomposition and "
                "the arm has failed no matter what the number does.",
    },
]

# Goal one is the node curve, so the node levels come first and every pass covers all three. An
# early stop then still leaves a balanced design rather than three reps of one node count.
NODE_LEVELS = (3, 1, 2)


# EVERY CELL ANSWERS A QUESTION, and the READOUT decides how many replicates it needs.
#
# Mihai, after a night that produced findings and no measurements: "don't just run this loop for the
# sake of it — each run should have a purpose." A twelfth baseline replicate answers nothing that the
# third did not.
#
# The rule that makes the queue short: a MECHANISM readout is a fact about the code and is valid at
# n=1 — "did `/v1` reach plan_loaded", "did detail_fallback go to zero", "did the fan fire". Only a
# SCORE comparison needs n>=3, because only a score has to clear the 46-point replicate spread. Most
# of what is worth knowing right now is mechanism, so most cells are n=1.
#
# `reps` is the replicate count for THIS cell; `asks` is the question, printed in the log so an
# operator reading it knows why the fleet is spending two hours.
QUESTIONS: list[dict] = [
    {"arm": "baseline", "nodes": 3, "reps": 3,
     "asks": "the replicate spread on this engine (every score comparison is measured against it), "
             "AND whether the F49 detail-budget fix drove detail_fallback to zero — that second half "
             "is a mechanism readout and is answered by the FIRST unit, not the third."},
    {"arm": "scoped_contracts", "nodes": 3, "reps": 3,
     "asks": "F164: test-authors are 31% of completions failed against ZERO for implementers, and "
             "93% of every failure this campaign has recorded. This is the ONLY queued arm aimed at "
             "that population. A `test_meridian.py` author currently receives `## API of` blocks for "
             "ALL FIVE modules — 265 lines of implementation BODY against 35 of signature, six "
             "private methods — when its declared dependency is `meridian` alone. READOUT ON "
             "TEST-AUTHORS ONLY (pooling the kinds is F156 a fourth time): system-prompt chars vs "
             "22,511; `## API of` blocks vs 5; max dry reasoning before first write vs a 3,402 "
             "median / 24,032 max; judge interventions vs 3. A cut in size with NO improvement in "
             "dry reasoning REFUTES the mechanism hypothesis and is the most valuable outcome. VOID "
             "if any test-author reports an empty neighborhood."},
    {"arm": "think_off", "nodes": 3, "reps": 3,
     "asks": "whether pre-closing the thinking block makes a test-author ACT. This is the only arm "
             "backed by a measurement of the model's behaviour rather than an argument about its "
             "inputs: 47.3s/974 chars/no tool call by default vs 3.1s with the tool call as the first "
             "token (F216). The falsifier is STREAMING, not the score — goose always streams and the "
             "evidence was non-streaming. If the trailing assistant message is present on every "
             "request and thinking is STILL non-zero, the route is dead and the lead closes."},
    {"arm": "dep_signatures", "nodes": 3, "reps": 3,
     "asks": "the same population as scoped_contracts, on the axis that actually holds the bytes. "
             "Measured live on the scoped run (F196): scoping the block COUNT cut 8.7% (one block of "
             "five) because a test-author's neighborhood is most of the app by nature. The four "
             "surviving blocks are 10,097 chars = 50.7% OF THE WHOLE PROMPT, and each is the "
             "dependency's full SOURCE — 6 private methods against 5 public, one of them truncated "
             "mid-`def` and fenced as if complete, under a header reading 'API of' and 'do NOT `cat` "
             "it'. swarm.rs:19628 already swaps that body for `extract_signatures` and is OFF. "
             "READOUT ON TEST-AUTHORS ONLY. FALSIFIER: if `## API of` bytes and private-method count "
             "fall sharply while dry reasoning and the failures.py test-author row do NOT move, then "
             "prompt volume is a correlate and not the cause, and F156/F159/F164/F196 have been "
             "chasing the wrong quantity. That is the result worth the fleet time."},
    {"arm": "sink_review", "nodes": 3, "reps": 1,
     "asks": "does the sink idle-fill run at all, now that both halves read one resolver (F44)? The "
             "SINK owns ~100% of the solo window (543-1045s with two nodes idle). MECHANISM: "
             "`sink_review{prewarmed>0}`. If prewarmed is 0 with the lever on, the producer still "
             "cannot see its precondition and the fix is incomplete."},
    {"arm": "split_inherit_spec", "nodes": 3, "reps": 3,
     "asks": "the ONLY mechanism whose firing pattern matches the scoreline on all three archived "
             "units: both 3-node losers split their single most-detailed task and the 1-node winner "
             "(0.8708) never split. On a split the child's ENTIRE task statement becomes "
             "'(split of <parent>) <child-id>' — ~35 chars replacing a ~3833-char spec the run paid "
             "40% of its wall-clock to produce. GUARD: if task_split == 0 in the treatment units the "
             "arm measured NOTHING and must be voided, not read as a null."},
    {"arm": "split_off", "nodes": 3, "reps": 3,
     "asks": "the control that keeps split_inherit_spec honest. 'Splitting is harmful' and 'splitting "
             "with an amputated spec is harmful' are DIFFERENT hypotheses; only the second is fixed "
             "by the lever. If this beats both baseline and the lever, the split DECISION is the "
             "defect and the spec inheritance is a distraction."},
    {"arm": "prereview_off", "nodes": 3, "reps": 3,
     "asks": "prereview findings rank-ordered perfectly with score across the three archived units "
             "(0 -> 0.8708, 1 -> 0.7186, 3 -> 0.6720), and the path is gated on spare capacity so it "
             "fires MORE with more nodes. F134 fixed the phantom generator (a 2400-char head-truncate "
             "with no marker, which made the reviewer declare working handlers 'not defined' and sent "
             "that to the sink as a fix order). This asks what damage REMAINS. A null means KEEP "
             "pre-review — 2 of 4 archived findings were genuine catches."},
    {"arm": "converge_off", "nodes": 3, "reps": 3,
     "asks": "THE most direct engine-side limit on plan width, and it is ON by default. The architect "
             "is told to emit 'the FEWEST cohesive modules ... do NOT over-split', with a target of "
             "worker_count to 2x worker_count — and 19 of 19 archived plans landed inside that band "
             "(3-6 modules, median 4) against SIX concurrent slots. Modules are the level-0 roots, so "
             "at t=0 the plan can occupy only that many. The instruction exists to raise cross-draft "
             "`plan_agreement`, an INTERNAL metric. With the lever off the architect is told to split "
             "AGGRESSIVELY and target 2x-3x worker_count instead. RISK, registered first: lower "
             "agreement drives the redraft ladder (~40 min of prefix when it fires), so this arm can "
             "lose on wall-clock while winning on width — read module count, antichain width, "
             "occupancy AND prefix together, not the score alone."},
    {"arm": "retarget_off", "nodes": 3, "reps": 3,
     "asks": "THE highest-value question, after F53. The redraft is the most expensive mechanism in "
             "the engine and it optimises draft-count parity. MEASURED on the last unit: prefix 3014s "
             "with 90% of it planning across FOUR redraft rounds, ~15,800 chars of model-authored "
             "spec re-derived, occupancy 0.30 — and the build came out at 83.4% with crunch 7/7 "
             "ANYWAY. PREDICTION: score unchanged within the replicate spread, prefix roughly halved, "
             "occupancy up. If the score DROPS below the spread the redraft buys something real and "
             "it stays — that is the outcome that would make this arm worth more than a speedup."},
    # THE THREE ARMS THE FRESH-EYES SWEEP PUT AHEAD OF EVERYTHING (F134), each at reps 3 because a
    # SCORE comparison is what they need and a score has to clear the 46-point replicate spread.
    # They run right after baseline so the spread and the arms come from one binary and one session.
    {"arm": "owned_file_fence", "nodes": 3, "reps": 1,
     "asks": "the ONLY defect class that MECHANICALLY must scale with concurrency — does a non-owner "
             "ever clobber an owned file? 76 archived runs show zero owned_file_violation events, but "
             "the detector has never been on, so that zero is uncontrolled and proves nothing (the "
             "standing rule). Runs as its own cell rather than on every arm because the fence "
             "RESTORES the clobbered bytes before the sink reads the tree — a treatment, not a probe, "
             "and 'free' only in the case where it has nothing to report. ZERO violations here CLOSES "
             "more-nodes-more-interference; non-zero promotes it to first place."},
    {"arm": "spec_repair", "nodes": 3, "reps": 1,
     "asks": "the tail is 13-26% of every run, has an occupancy number of NONE (it emits no dispatch "
             "at all), and has never once gone green — 13 of 13 archived rounds ended with findings "
             "outstanding and 3 of them ended with MORE than they started, because the default fix "
             "writes into the real tree unverified. This arm answers two things at once and they "
             "are separable: does racing one attempt per node fire (`spec_repair_wave.twins`), and "
             "does the verified-winner rule hold live (`winner_findings` < `baseline_findings`, "
             "always). It also produces the tail's FIRST occupancy number via "
             "complete_fix_dispatched, which is why it outranks the remaining n=1 readouts: they "
             "measure mechanisms, this one measures a region of the run nobody has ever measured."},
    {"arm": "doc_prefetch", "nodes": 3, "reps": 1,
     "asks": "UNBLOCKED BY F78, and it could not have been asked before. doc_prefetch builds its "
             "payload from findings.filter(|f| f.grounded), and `grounded` was `is_mcp && ok` on a "
             "bench with NO MCP tools attached - so the filter matched nothing on every run ever and "
             "the lever was INERT BY CONSTRUCTION, not merely off. Measuring it before tonight would "
             "have recorded silence as a negative result. First run on the new engine reports "
             "grounded=2, so the precondition is now reachable. The question is therefore not 'does "
             "the verbatim channel help' but 'does it CARRY ANYTHING': MECHANISM readout is doc_facts "
             "non-empty and /v1 present verbatim in a worker dispatch. This is the fourth mechanism "
             "found with a precondition that never held (after task_split, sink_review, "
             "complete_parallel), which makes 'prove the precondition can occur' the first question "
             "to ask of any lever."},
    {"arm": "doc_fetch", "nodes": 3, "reps": 1,
     "asks": "DEMOTED by F53. It was cell 2 on the argument that losing `/v1` breaks the build; the "
             "83.4% unit lost it entirely and crunch still passed 7/7, because workers have shell and "
             "re-derive the path themselves (F54). Still worth one unit — a verbatim document is the "
             "densest instruction available and removes a coin flip — but it is no longer urgent. "
             "MECHANISM readout: `doc_fetched{ok:true}` and `/v1` count in plan_loaded."},
    {"arm": "complete_parallel", "nodes": 3, "reps": 1,
     "asks": "now that F41 taught the finding-extractor to read backticked paths and dotted modules, "
             "does the repair fan actually fire? MECHANISM: count `complete_fix_wave` / per-file fix "
             "shards. Before F41 five of six real finding shapes resolved to nothing, so the fan had "
             "nothing to fan and this arm would have measured silence."},
    {"arm": "kind_prompt", "nodes": 3, "reps": 1,
     "asks": "does gating worker rules by task kind drive kind_mismatch_pct toward zero? 72-83% of "
             "dispatches currently get rules written for another job. MECHANISM readout."},
    {"arm": "e2e_oracle", "nodes": 3, "reps": 1,
     "asks": "do the e2e shards stop deriving their checklist from the build's own README and start "
             "from the engine-extracted surface? MECHANISM: the shard reports must stop citing the "
             "README. Can fail independently of the score."},
]

# THE NODE CURVE IS GOAL ONE AND IT RUNS THIRD, not last.
#
# It was last on the reasoning that the mechanism arms change the engine the curve measures. That is
# true of exactly ONE of them: `retarget_off`. The prefix is ~25% of a run, 88-90% of it planning, and
# it is NODE-INDEPENDENT overhead — halving it moves occupancy materially and would expire any curve
# measured before it. The other five arms are n=1 MECHANISM readouts (does the fan fire, does the
# report carry /v1, does kind_mismatch fall) and cannot move a score curve.
#
# So the curve waits for `retarget_off` and nothing else. Deferring it behind five readouts that
# cannot affect it was deferring the only question the project exists to answer — every node-scaling
# number on disk is still 1 node at 44.2% against a 3-node range of 42.7-88.7%, which straddles it.
NODE_CURVE = [{"arm": "baseline", "nodes": n, "reps": 3} for n in (1, 2)]


def cells() -> list[dict]:
    """One entry per (arm, nodes) cell, in the order the questions are worth asking."""
    by_name = {a["name"]: a for a in arms_now()}
    out = []
    curve = [{**c, "asks": "GOAL ONE — the node curve: does build quality and fleet occupancy "
                           "actually improve at this node count? Every measurement on disk so far is "
                           "1 node at 44.2% against a 3-node range of 42.7-88.7% that straddles it, "
                           "so nothing is known yet."}
             for c in NODE_CURVE]
    # baseline, retarget_off, THEN the curve, then the mechanism readouts.
    ordered = QUESTIONS[:2] + curve + QUESTIONS[2:]
    for q in ordered:
        arm = by_name.get(q["arm"])
        if arm is None:
            continue  # an arm named here but not defined yet is skipped, never silently substituted
        out.append({"nodes": q["nodes"], "arm": arm, "reps": q["reps"], "asks": q["asks"]})

    # THE MIRROR OF THE GUARD ABOVE, AND ITS ABSENCE ALREADY COST ME AN ARM (F170).
    #
    # The line above protects QUESTIONS -> ARMS: a question naming an undefined arm is skipped
    # loudly rather than substituted. Nothing protected ARMS -> QUESTIONS, and that direction is
    # WORSE, because it fails completely silently: `scoped_contracts` was defined in ARMS with
    # reps=3, was deliberately moved to index 1 as the campaign's top priority after F164, and would
    # NEVER HAVE RUN — cells() builds from QUESTIONS, so an arm absent there is simply invisible.
    # Nothing in the log would have said so; the arm would just never appear, forever.
    #
    # An arm with reps=0 is deliberately parked (detail_budget) and is not a defect.
    scheduled = {q["arm"] for q in ordered}
    orphans = [a["name"] for a in by_name.values()
               if a["name"] not in scheduled and a.get("reps", 1) > 0]
    if orphans:
        log(f"[SWEEP] ⚠ {len(orphans)} arm(s) defined in ARMS with reps>0 but named in NO question, "
            f"so they can never be scheduled: {', '.join(sorted(orphans))}. Add them to QUESTIONS "
            f"or set reps=0 to park them deliberately.")
    return out


def now() -> str:
    return datetime.now().strftime("%H:%M:%S")


def log(msg: str) -> None:
    print(msg, flush=True)


def unit_name(arm: str, nodes: int, rep: int) -> str:
    return f"{arm}-n{nodes}-r{rep}"


def unit_dir(arm: str, nodes: int, rep: int) -> Path:
    return OUT / unit_name(arm, nodes, rep)


def result_path(arm: str, nodes: int, rep: int) -> Path:
    return unit_dir(arm, nodes, rep) / "nodeloop-result.json"


def complete(arm: str, nodes: int, rep: int) -> bool:
    p = result_path(arm, nodes, rep)
    if not p.is_file():
        return False
    try:
        r = json.loads(p.read_text())
    except Exception:
        return False
    # A unit is only "done" if it was measured by the CURRENT instrument AND on the CURRENT engine.
    # Checking the instrument alone left a hole with teeth: after a rebuild, a stale unit still
    # counts as complete, gets skipped forever, and quietly contributes a row measured on a
    # different binary. That is the exact shape of the failure that once published a table showing
    # the cheaper model winning — every part of the loop did its job and the conclusion was wrong.
    return (r.get("audit_version") == dispatch_audit.AUDIT_VERSION
            and r.get("engine_build") == engine_build())


def looks_transient(tail: str) -> bool:
    low = (tail or "").lower()
    return any(t in low for t in TRANSIENT)


def engine_build() -> str:
    """Identify the ENGINE BINARY a unit ran on.

    Results already carry scorer_version and audit_version, but nothing identified the engine — and
    that gap cost a campaign: 34 hours of backlog were queued against a binary built before the
    levers the arms set even existed, so `detail_budget` would have set an env var the binary
    ignores and recorded a confident "no effect". mtime+size is enough to tell two builds apart and
    costs nothing; a content hash of a 235 MB binary would not be worth its own runtime.
    """
    try:
        import run_build
        st = run_build.GOOSE.stat()
        return f"{int(st.st_mtime)}-{st.st_size}"
    except Exception as exc:  # noqa: BLE001 - an unknown build must be visible, never silently absent
        return f"unknown:{type(exc).__name__}"


def engine_pids() -> list[int]:
    try:
        r = subprocess.run(["pgrep", "-f", "goose swarm run"],
                           capture_output=True, text=True, timeout=15)
        return [int(p) for p in r.stdout.split() if p.strip().isdigit()]
    except Exception:
        return []


def _ppid(pid: int) -> int | None:
    try:
        r = subprocess.run(["ps", "-o", "ppid=", "-p", str(pid)],
                           capture_output=True, text=True, timeout=10)
        v = r.stdout.strip()
        return int(v) if v.isdigit() else None
    except Exception:
        return None


def intruder_engine_pids() -> list[int]:
    """Engines that are NOT this sweep's own child.

    The guard used to see `len(engine_pids()) > 1` and kill EVERY engine pgroup, its own included, then
    cut the running unit loose. MEASURED: that destroyed THREE consecutive units in forty minutes —
    baseline-n3-r0 at 24 min, baseline-n1-r0 at 19 min, and retarget_off which then failed instantly on
    the port the dying process still held. An intruder cost an hour of fleet time, and the innocent unit
    died with it every time.

    A sweep KNOWS which engine is its own: it is the one it spawned, so its ppid is this process. Kill
    the others and the contention is gone without the unit being sacrificed for it.
    """
    me = os.getpid()
    return [p for p in engine_pids() if _ppid(p) != me]


def median_unit_secs() -> float | None:
    """Median wall of units that actually finished, so "too long" is measured, not guessed."""
    walls = []
    for f in OUT.glob("*/nodeloop-result.json"):
        try:
            r = json.loads(f.read_text())
        except Exception:
            continue
        if r.get("wall_secs") and not r.get("timed_out") and not r.get("aborted"):
            walls.append(r["wall_secs"])
    if not walls:
        return None
    walls.sort()
    return walls[len(walls) // 2]


def abandon_decision(unit: Path, arm: dict, nodes: int, elapsed: float) -> tuple[float, list[str]]:
    """How confident are we that this unit can NO LONGER inform goal one? 0..1 with reasons.

    The watchdog above kills what is BROKEN. This decides what is POINTLESS, which is a different and
    harder question, and the one that actually costs weeks: a unit that got the wrong pool runs its
    full ~2 hours and is only marked VOID afterwards. Nothing about that row was ever going to be
    evidence, and the fleet time was spent to learn something already known at minute one.

    Deliberately asymmetric. Killing a HEALTHY unit costs a full re-run and poisons the replicate
    count, so every predicate here must be something already DECIDED — a fact about this run that no
    amount of further work can change — not a prediction that it will go badly. A slow unit is not a
    doomed one, and a unit producing a BAD score is doing its job.
    """
    reasons: list[str] = []
    conf = 0.0
    log_path = unit / "run.jsonl"
    events = []
    if log_path.is_file():
        for line in log_path.read_text(errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except Exception:
                continue

    # 1. VOID BY CONSTRUCTION. run_started carries the pool the engine actually built. If it is not
    #    the pool this cell asked for, the row is excluded from every mean no matter how it ends —
    #    so finishing it buys nothing. This is certain, not probable, and it is knowable at minute 1.
    started = next((e for e in events if e.get("event") == "run_started"), None)
    if started is not None:
        actual = len(started.get("pool") or [])
        if actual and actual != nodes:
            conf = 1.0
            reasons.append(f"pool is {actual}, cell asked for {nodes} — VOID by construction, the row "
                           f"can never be evidence")

    # 1b. THE POOL EVENT UNDER-REPORTS. `run_started.pool` is emitted before the engine may push the
    #     PLANNER on as an extra worker device, so it is not the worker count. MEASURED on the 1-node
    #     unit: pool of 1, dispatches to `mac-gabee-…` AND `planner`, peak of TWO devices working at
    #     once. A node-count cell that quietly runs an extra worker is the same defect that produced
    #     this project's original "more nodes make it worse" table, and the check above cannot see it
    #     because the pool really is 1.
    #
    #     Two independent readings, because the fix and the detector must not share an assumption:
    #     `pool_resolved` when the engine emits it, and otherwise the DISPATCH RECORD itself, which no
    #     engine build can misreport.
    resolved = next((e for e in events if e.get("event") == "pool_resolved"), None)
    if resolved is not None and resolved.get("worker_count") not in (None, nodes):
        conf = 1.0
        reasons.append(f"pool_resolved says {resolved.get('worker_count')} worker device(s), cell "
                       f"asked for {nodes} (planner_pushed={resolved.get('planner_pushed')}) — VOID")
    dispatched_devices = {e.get("device") for e in events
                          if e.get("event") == "task_dispatched" and e.get("device")}
    if len(dispatched_devices) > nodes:
        conf = 1.0
        reasons.append(f"{len(dispatched_devices)} distinct devices received dispatches "
                       f"{sorted(dispatched_devices)} but the cell asked for {nodes} — VOID; the run "
                       f"used more workers than the cell is labelled with")

    # 2. THE ARM CANNOT FIRE. An arm sets env vars; if the running binary has no such lever, the arm
    #    is byte-identical to baseline and would be recorded as "no effect" — a fabricated null. This
    #    already happened: 34 hours were queued against a binary with no GOOSE_SWARM_DETAIL_BUDGET_SECS.
    for var in arm.get("env", {}):
        try:
            import run_build
            out = subprocess.run(["strings", str(run_build.GOOSE)],
                                 capture_output=True, text=True, timeout=120)
            if var not in out.stdout:
                conf = 1.0
                reasons.append(f"{var} is ABSENT from the engine binary — this arm cannot fire and "
                               f"would record a fabricated 'no effect'")
        except Exception:
            pass
        break   # one probe is enough; strings over 235MB is not free

    # 3. PLANNING STUCK. No task has been dispatched well past the point where every observed run had
    #    started dispatching. Not proof of doom, so it is weighted below the kill line on its own.
    if events and not any(e.get("event") == "task_dispatched" for e in events):
        if elapsed > 3600:
            conf = max(conf, 0.85)
            reasons.append(f"{elapsed / 60:.0f} min elapsed with ZERO dispatches — planning has not "
                           f"produced a single task (observed pre-dispatch is ~25-31 min)")
        elif elapsed > 2400:
            conf = max(conf, 0.5)
            reasons.append(f"{elapsed / 60:.0f} min with no dispatch yet (observed ~25-31 min)")

    # 4. FAR BEYOND THE MEASURED NORM. Uses the median of units that actually finished, so it adapts
    #    rather than encoding a guess. Alone it is under the line — slow is not doomed — but it
    #    compounds with anything else.
    med = median_unit_secs()
    if med and elapsed > 2.5 * med:
        conf = max(conf, 0.6)
        reasons.append(f"{elapsed / 60:.0f} min is {elapsed / med:.1f}x the median finished unit "
                       f"({med / 60:.0f} min)")

    return min(conf, 1.0), reasons


class Watchdog(threading.Thread):
    """Cut a DOOMED unit loose instead of waiting out its 4.5h cap.

    A wedged run does not fail — it sits there holding fleet capacity until the timeout, and a cap
    that truncates work measures the cap rather than the swarm. The LOOP is never stopped by this;
    only the unit is. Every trip condition is a fact about the process or the filesystem, never an
    inference from how long something is taking, because a slow unit is not a doomed one.
    """

    ABANDON_AT = 0.8   # kill only on something already DECIDED; a wrong kill costs a full re-run

    def __init__(self, label: str, unit: Path, arm: dict, nodes: int) -> None:
        super().__init__(daemon=True)
        self._stop = threading.Event()
        self.label = label
        self.unit = unit
        self.arm = arm
        self.nodes = nodes
        self.started_at = time.time()
        self.reason: str | None = None
        self.abandoned = False
        self.abandon_confidence = 0.0
        self.abandon_reasons: list[str] = []
        self.contended = 0      # intruder engines evicted during this unit's life

    def evict_intruders(self) -> int:
        """Kill engines this sweep did not spawn. Returns how many were evicted.

        Eviction is ALWAYS right — an engine nobody scheduled is contending for a fleet this unit is
        being measured on. What is not right is taking the unit down with it, which is what the old
        all-or-nothing kill did three times in forty minutes.
        """
        evicted = 0
        for pid in intruder_engine_pids():
            try:
                os.killpg(os.getpgid(pid), signal.SIGKILL)
                evicted += 1
                log(f"[evict] {now()} {self.label}: killed INTRUDER engine pgroup {pid} — not this "
                    f"sweep's child; the unit continues")
            except Exception as exc:
                log(f"[evict] {now()} could not kill intruder {pid}: {exc}")
        if evicted:
            # The unit ran under contention for some unknown slice of its life, so its WALL-CLOCK is
            # tainted even though its build is not. Recorded rather than hidden: a timing number nobody
            # flagged is worse than one nobody has.
            self.contended += evicted
        return evicted

    def doomed(self) -> str | None:
        # EVICT FIRST, and only then ask whether this unit is beyond saving. An intruder is a reason to
        # remove the intruder, not a reason to lose the measurement.
        self.evict_intruders()
        pids = engine_pids()
        if len(pids) > 1:
            return (f"{len(pids)} engines running at once {pids} AFTER eviction — they are this "
                    f"sweep's own children, so something is spawning engines inside the unit; "
                    f"fleet and will skew this unit and every later one")
        if pids:
            beats = sorted(OUT.glob("*/heartbeat"), key=lambda p: p.stat().st_mtime, reverse=True)
            if beats:
                age = time.time() - beats[0].stat().st_mtime
                if age > HEARTBEAT_STALE_SECS:
                    return (f"heartbeat {int(age)}s stale under a live engine — the run is wedged "
                            f"and will hold the fleet until its cap")
        free_gb = shutil.disk_usage(os.path.expanduser("~")).free / 1e9
        if free_gb < MIN_FREE_GB:
            return f"only {free_gb:.0f} GB free — a run that fills the disk corrupts its own tree"
        return None

    def abort(self, reason: str) -> None:
        log(f"[abort] {now()} {self.label}: {reason}")
        for pid in engine_pids():
            try:
                os.killpg(os.getpgid(pid), signal.SIGKILL)
                log(f"[abort] killed engine pgroup for pid {pid}")
            except (ProcessLookupError, PermissionError) as exc:
                log(f"[abort] could not kill {pid}: {exc}")

    def run(self) -> None:
        while not self._stop.wait(WATCHDOG_POLL_SECS):
            try:
                reason = self.doomed()
            except Exception:
                reason = None
            if reason:
                self.reason = reason
                self.abort(reason)
                return
            # BROKEN is not the only reason to stop. Judge whether this unit can still inform goal
            # one at all, and cut it loose the moment the answer is settled — waiting out a ~2h run
            # whose row is already void is the single largest avoidable waste in this campaign.
            try:
                conf, why = abandon_decision(self.unit, self.arm, self.nodes,
                                             time.time() - self.started_at)
            except Exception:
                continue
            if conf >= self.ABANDON_AT:
                self.abandoned = True
                self.abandon_confidence = conf
                self.abandon_reasons = why
                self.abort(f"ABANDONED at confidence {conf:.2f} — " + "; ".join(why))
                return
            if why:
                log(f"[watch] {self.label}: confidence {conf:.2f} this unit is pointless "
                    f"(kill at {self.ABANDON_AT}) — {'; '.join(why)}")

    def done(self) -> None:
        self._stop.set()


def kill_strays() -> None:
    for pid in engine_pids():
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
            log(f"[warn] killed stray engine pgroup for pid {pid}")
        except (ProcessLookupError, PermissionError):
            pass


def reap_stray_listeners(port_lo: int, port_hi: int) -> list[int]:
    """Kill processes still LISTENING in the bench port range that this sweep did not spawn.

    THE LEAK IS MODEL-AUTHORED, so no engine prompt can be the primary defence. A worker decides on
    its own to exercise the app it just built, and does it like this (recovered verbatim from a real
    orphan):

        bash -c  rm -f vendorsync.db && python3 -m vendorsync --db vendorsync.db --port 8931 &
                 SERVER_PID=$! ...

    When that worker's run is killed — or simply ends without the model getting back to its own
    cleanup — the server survives with ppid 1 and holds the port forever. MEASURED: one such orphan
    held 8931 for EIGHTY-TWO MINUTES after its run was parked, failed the next unit outright with
    `OSError: [Errno 48] Address already in use`, and was the confirmed cause of a `pytest
    --collect-only` that failed at 08:58 and passed unmodified at 09:20 (F88) — a test importing the
    app while another process holds its port errors at COLLECT time.

    So a dead run poisons every later run, and a long sweep accumulates one dead port per leak. The
    harness owns the environment, so the harness reaps. Runs BETWEEN units, never during one.
    """
    me = os.getpid()
    try:
        out = subprocess.run(
            ["lsof", "-nP", f"-iTCP:{port_lo}-{port_hi}", "-sTCP:LISTEN"],
            capture_output=True, text=True, timeout=20).stdout
    except Exception:
        return []
    killed = []
    for line in out.splitlines()[1:]:
        parts = line.split()
        if len(parts) < 2 or not parts[1].isdigit():
            continue
        pid = int(parts[1])
        # NEVER our own process: the sweep runs the vendor service IN-PROCESS on a port inside this
        # very range, so an identity check is the only thing standing between this reaper and suicide.
        if pid == me:
            continue
        # AND NEVER ANY SWEEP. `pid == me` is not enough, and this is not hypothetical: running the
        # reaper's own control test from a throwaway process made the LIVE SWEEP look foreign — it was
        # holding 127.0.0.1:8933 — and killed it mid-unit with no STOP sentinel and no log line. The
        # identity guard was correct and the ASSUMPTION AROUND IT was wrong, because a guard written as
        # "not me" silently means "everything except whoever happens to be calling".
        #
        # So the rule is now POSITIVE rather than negative: only kill something that looks like a
        # leaked APP SERVER. A leaked server is `python -m <pkg>` / a bare app process; the sweep is
        # `python .../sweep.py` and the engine is `.../goose swarm run`. Anything not recognisable as
        # an app server is left alone, because the cost of a false kill here is a dead sweep and the
        # cost of a false spare is one held port that the next unit's port allocation steps over.
        try:
            cmd = subprocess.run(["ps", "-o", "command=", "-p", str(pid)],
                                 capture_output=True, text=True, timeout=10).stdout.strip()
        except Exception:
            continue
        if not cmd or "sweep.py" in cmd or "goose swarm run" in cmd or "loop.sh" in cmd:
            continue
        if " -m " not in cmd and "python" not in cmd.split("/")[-1][:16]:
            continue
        try:
            os.killpg(os.getpgid(pid), signal.SIGKILL)
        except Exception:
            try:
                os.kill(pid, signal.SIGKILL)
            except Exception:
                continue
        killed.append(pid)
    return killed


def run_unit(arm: dict, nodes: int, rep: int, port: int) -> dict:
    """One episode: build, grade the artifact, then grade the INSTRUCTIONS it was given."""
    import run_build  # imported late so a syntax error there cannot stop the loop from starting

    entrant = f"swarm-{nodes}node"   # run_build reads the N and sets GOOSE_SWARM_MAX_NODES
    prev = dict(os.environ)
    dog = Watchdog(unit_name(arm["name"], nodes, rep), OUT / f"{entrant}-r{rep}", arm, nodes)
    dog.start()
    try:
        for k, v in arm["env"].items():
            os.environ[k] = v
        verdict = run_build.run(entrant, rep, OUT, TIMEOUT, port)
    finally:
        dog.done()
        os.environ.clear()
        os.environ.update(prev)
        # Reap BEFORE the next unit binds its port. A leaked app server is not this unit's problem —
        # it is the NEXT one's, and it presents as a build defect rather than as an environment fault.
        strays = reap_stray_listeners(PORT_BASE, PORT_BASE + 40)
        if strays:
            log(f"[reap] {now()} killed {len(strays)} leaked app server(s) still holding a bench "
                f"port: {strays} — a worker started them and nothing stopped them")

    # run_build names its outputs after the ENTRANT, so two arms at the same node count and rep
    # would overwrite each other's tree AND vendor trace. Re-home both under the unit.
    src = OUT / f"{entrant}-r{rep}"
    dst = unit_dir(arm["name"], nodes, rep)
    if src.exists():
        if dst.exists():
            shutil.rmtree(dst)
        src.rename(dst)
    trace_src = OUT / f"trace-{entrant}-r{rep}.jsonl"
    if trace_src.exists():
        trace_src.replace(dst / "vendor-trace.jsonl")

    audit = {}
    run_log = dst / "run.jsonl"
    if run_log.is_file():
        try:
            audit = dispatch_audit.audit(run_log)
            audit.pop("per_dispatch", None)   # kept in run.jsonl; the summary is what we compare
        except Exception as exc:  # noqa: BLE001 - a broken instrument must be visible, not fatal
            audit = {"audit_error": f"{type(exc).__name__}: {exc}"}

    # The pre-dispatch window is 25% of the run and emits no task event, so occupancy.py is blind to
    # it. Measured on three units: planning is 68-83% of it, and a confidence redraft does the whole
    # thing twice. Run per unit rather than by hand, because an instrument nobody runs is a comment.
    pre = {}
    try:
        pre = prefix.analyse(dst)
    except Exception as exc:  # noqa: BLE001
        pre = {"prefix_error": f"{type(exc).__name__}: {exc}"}

    # ADVERSARIAL AUDIT OF THE HARNESS, every unit, not just of the swarm. Six instrument failures in
    # one day and two published before being caught; a unit whose own instruments cannot pass their
    # controls and invariants is not evidence, and must be MARKED rather than quietly averaged in.
    # It never stops the loop — a harness fault must not silently discard fleet time.
    harness = {"ok": None, "detail": ""}
    try:
        r = subprocess.run([sys.executable, str(HERE / "selftest.py"), str(dst)],
                           capture_output=True, text=True, timeout=600)
        harness = {"ok": r.returncode == 0, "detail": (r.stdout + r.stderr).strip()[:2000]}
        if not harness["ok"]:
            log(f"[HARNESS] {unit_name(arm['name'], nodes, rep)} FAILED its own audit — this unit is "
                f"NOT evidence:\n{harness['detail']}")
    except Exception as exc:  # noqa: BLE001 - an audit that cannot run is an audit that failed
        harness = {"ok": False, "detail": f"{type(exc).__name__}: {exc}"}
        log(f"[HARNESS] audit could not run: {harness['detail']}")

    actual = verdict.get("actual_nodes")
    # The label is an intention; run_started.pool is the fact. A mismatch has silently voided a
    # whole campaign before, so it voids the unit here rather than being averaged in.
    void = actual is not None and actual != nodes

    return {
        "arm": arm["name"],
        "nodes": nodes,
        "rep": rep,
        "env": arm["env"],
        "gate": arm["gate"],
        "finished_at": datetime.now().isoformat(timespec="seconds"),
        "score": verdict.get("score"),
        "tiers": verdict.get("tiers"),
        "aborted": dog.reason is not None,
        "abort_reason": dog.reason,
        # How many engines nobody scheduled were evicted while this unit ran. Non-zero means the
        # WALL-CLOCK is tainted (the fleet was shared for some unknown slice) even though the build is
        # not — so a timing comparison against this row must say so. A tainted number nobody flagged is
        # worse than a number nobody has.
        "contended": dog.contended,
        "abandoned": dog.abandoned,
        "abandon_confidence": dog.abandon_confidence,
        "abandon_reasons": dog.abandon_reasons,
        "timed_out": (verdict.get("agent") or {}).get("timed_out"),
        "wall_secs": (verdict.get("agent") or {}).get("secs"),
        "actual_pool": verdict.get("actual_pool"),
        "actual_nodes": actual,
        "void": void,
        "void_reason": (f"asked for {nodes} nodes, engine built {actual}" if void else None),
        "scorer_version": verdict.get("scorer_version"),
        "engine_build": engine_build(),
        "audit_version": audit.get("audit_version") or dispatch_audit.AUDIT_VERSION,
        "audit": audit,
        "prefix": pre,
        # summarise() excludes rows where this is False. It was computed above — up to 600s of
        # selftest.py per unit — and then never written, so the filter read `None is not False` on
        # every row and was vacuously true: a unit whose own instruments FAILED their controls was
        # averaged in exactly like a clean one. The guard existed, ran, and was thrown away at the
        # return statement.
        "harness_ok": harness["ok"],
        "harness_detail": harness["detail"],
    }


def arms_now() -> list[dict]:
    """ARMS plus anything appended to QUEUE, so arms can be added without restarting the loop.

    A running interpreter never sees a source edit, so a new arm added to ARMS in this file would
    not reach a loop that is already up. QUEUE is re-read every pass.
    """
    arms = list(ARMS)
    if QUEUE.is_file():
        for raw in QUEUE.read_text().splitlines():
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            name, _, rest = line.partition(" ")
            env = {}
            for tok in rest.split():
                if "=" in tok:
                    k, _, v = tok.partition("=")
                    env[k] = v
            if name and name not in {a["name"] for a in arms}:
                arms.append({"name": name, "env": env, "gate": "(from QUEUE)"})
    return arms


def backlog(target_reps: int) -> list[tuple[dict, int, int]]:
    """Units still owed, ordered so the cheapest decisive question comes first.

    A cell's OWN `reps` bounds it, not a global n. A mechanism readout — did `/v1` reach the plan,
    did the fan fire, did detail_fallback go to zero — is a fact about the code and is settled by one
    unit; only a SCORE comparison has to clear the 46-point replicate spread. `target_reps` still
    RAISES a cell above its own floor when the backlog drains, so a long night deepens the score
    cells instead of inventing new ones.
    """
    units = []
    for rep in range(target_reps):
        for c in cells():
            # A MECHANISM cell (reps == 1) is DONE after one unit — "did `/v1` reach the plan" does
            # not get truer with a second run, and re-running it is the "loop for the sake of it"
            # this design exists to stop. A SCORE cell grows with target_reps instead, so a long
            # night deepens the comparisons that have to clear the replicate spread rather than
            # inventing new questions nobody asked.
            cap = 1 if c.get("reps", 1) == 1 else max(c.get("reps", 1), target_reps)
            if rep >= cap:
                continue
            if not complete(c["arm"]["name"], c["nodes"], rep):
                units.append((c["arm"], c["nodes"], rep))

    # BASELINE REPLICATES ARE HOISTED TO THE FRONT until the baseline cell reaches MIN_REPS.
    #
    # The loop above is rep-MAJOR, so `baseline-n3-r1` sits behind the ENTIRE rep-0 pass — 31 units,
    # ETA Wednesday. That is fine for throughput and fatal for the thing the campaign is currently
    # blocked on: the F154 engine freeze lifts when the baseline has n=3, because a 46-point
    # replicate spread makes every treatment score uninterpretable until the spread itself is
    # measured. Under rep-major ordering the freeze would hold for ~26 hours while five diagnosed
    # engine fixes sat unshipped — honouring the gate's WORDING and defeating its PURPOSE.
    #
    # The baseline is not just another arm: it is the DENOMINATOR of every other cell. A treatment
    # score without it is a number with nothing to be compared against, so running treatments first
    # is strictly out of order. Hoisting costs nothing — the same units run, in a better sequence.
    #
    # Self-limiting by construction: once the baseline reaches MIN_REPS this partition is empty and
    # the ordering is exactly what it was before.
    # NODE_LEVELS[0] is the full fleet; a baseline measured at fewer nodes is a different question
    # (the node curve) and is NOT what the treatments are compared against.
    full = NODE_LEVELS[0]

    def is_base(u: tuple) -> bool:
        return u[0]["name"] == "baseline" and u[1] == full

    base = [u for u in units if is_base(u)]
    if base:
        units = base + [u for u in units if not is_base(u)]

    # THEN INTERLEAVE THE ONE ARM AIMED AT THE METRIC, so it runs SECOND rather than seventh.
    #
    # Hoisting all three baseline replicates to the front is right for the DENOMINATOR and wrong for
    # the CLOCK: it puts ~5 hours of replicates between the engine changes and the first treatment
    # that could move the test-author row. Measured cost of that ordering: `dep_signatures` sat at
    # position 7, about ten hours out.
    #
    # One baseline is enough to compare a treatment against — the second and third tighten the
    # interval, they do not create it. So: first baseline, then the arm, then the remaining
    # replicates. The same units still run and the interval still closes; the decisive comparison
    # just stops waiting behind work that only refines it.
    #
    # `dep_signatures` specifically, because it is the only queued arm with a measured mechanism
    # pointed at the failing population: the `## API of` bundle is 50.7% of a test-author's prompt and
    # 3 of 4 of its blocks are truncated mid-token (one of them does not parse). Everything else in
    # the queue is a question; this one is a defect with a switch already written for it.
    # MECHANISM BEFORE SCORE. `think_off` runs FIRST, ahead of the baseline replicate.
    #
    # The ordering above (baseline, then arm) is correct when the question is a SCORE — a treatment
    # needs a denominator. It is wrong when the question is whether the mechanism REACHES the model at
    # all, and that is what this arm is really asking: the 47.3s->3.1s evidence was measured
    # NON-streaming and goose always streams, so the first thing to learn is whether a synthetic
    # trailing assistant message survives `format_messages_with_options` and the streaming path.
    #
    # That is answerable from the arm's FIRST test-author dispatch — one `llm_request` line — not from
    # a finished run. Putting a 100-minute baseline in front of it buys nothing, because if the
    # mechanism does not reach the model the score is uninterpretable regardless of denominator. A
    # clean denominator already exists (n=5) and the remaining replicates refine an interval that a
    # dead mechanism would make moot.
    #
    # This is the stall detector's own advice taken literally: feedback latency is a variable under my
    # control, and the cheapest decisive experiment is not the one the queue happens to schedule first.
    arm = next((u for u in units if u[0]["name"] == "think_off" and u[1] == full), None)
    if arm is not None:
        units = [arm] + [u for u in units if u is not arm]
    return units


def read_results() -> list[dict]:
    rs = []
    for f in sorted(OUT.glob("*/nodeloop-result.json")):
        try:
            rs.append(json.loads(f.read_text()))
        except Exception:
            continue
    return rs


def summarise() -> None:
    """Mechanism first, score second — a score alone cannot clear a 46-point spread."""
    rs = read_results()
    if not rs:
        return
    groups: dict[tuple[str, int], list[dict]] = {}
    for r in rs:
        groups.setdefault((r.get("arm"), r.get("nodes")), []).append(r)
    log("")
    log(f"{'arm':<18}{'nodes':>5}{'n':>3}  {'score mean':>10} {'spread':>8}  "
        f"{'fallbacks':>9} {'kind-mm%':>9} {'wall min':>9}  void")
    for (arm, nodes), g in sorted(groups.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)):
        ok = [r for r in g if not r.get("timed_out") and not r.get("aborted")
              and not r.get("abandoned") and not r.get("void")
              and r.get("harness_ok") is not False and r.get("score") is not None]
        sc = [r["score"] for r in ok]
        fb = [r["audit"].get("detail_fallback_count") for r in ok
              if isinstance(r.get("audit"), dict)
              and r["audit"].get("detail_fallback_count") is not None]
        km = [r["audit"].get("kind_mismatch_pct") for r in ok
              if isinstance(r.get("audit"), dict)
              and r["audit"].get("kind_mismatch_pct") is not None]
        wl = [r["wall_secs"] for r in ok if r.get("wall_secs")]
        mean = f"{sum(sc) / len(sc):.1%}" if sc else "-"
        spread = f"{(max(sc) - min(sc)) * 100:.0f}pts" if len(sc) > 1 else "-"
        log(f"{arm:<18}{nodes if nodes is not None else '?':>5}{len(g):>3}  "
            f"{mean:>10} {spread:>8}  "
            f"{(sum(fb) / len(fb) if fb else 0):>9.1f} "
            f"{(sum(km) / len(km) if km else 0):>9.1f} "
            f"{(sum(wl) / len(wl) / 60 if wl else 0):>9.0f}  "
            f"{sum(1 for r in g if r.get('void'))}")
    log("")


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    log("=" * 78)
    log(f"nodeloop starting {datetime.now().isoformat(timespec='seconds')}  "
        f"pid={os.getpid()}  audit={dispatch_audit.AUDIT_VERSION}")
    qs = cells()
    log(f"{len(qs)} question(s) queued; each cell carries its own replicate target "
        f"(mechanism readouts n=1, score comparisons n=3):")
    for c in qs:
        log(f"    [{c['arm']['name']}@{c['nodes']}n x{c['reps']}] {c['asks'][:110]}")
    log(f"stop with: touch {STOP}")
    log("=" * 78)

    target = MIN_REPS
    durations: list[float] = []
    port = PORT_BASE

    while True:
        if STOP.is_file():
            log(f"[stop] {now()} STOP sentinel present — exiting cleanly")
            summarise()
            return 0

        todo = backlog(target)
        if not todo:
            # Never end on a counter. More replicates is the most useful thing this loop can do
            # next, because every verdict here is limited by n, not by ideas.
            target += 1
            log(f"[grow] {now()} backlog drained — raising replicate target to n={target}")
            summarise()
            continue

        arm, nodes, rep = todo[0]
        label = unit_name(arm["name"], nodes, rep)
        eta = ""
        if durations:
            avg = sum(durations) / len(durations)
            eta = (f"  (~{datetime.fromtimestamp(time.time() + avg).strftime('%H:%M')}, "
                   f"{len(todo)} left ~"
                   f"{datetime.fromtimestamp(time.time() + avg * len(todo)).strftime('%a %H:%M')})")
        nxt = unit_name(todo[1][0]["name"], todo[1][1], todo[1][2]) if len(todo) > 1 else "raise n"
        log("")
        log(f">>> {now()}  NOW: {label}   [{len(todo)} in backlog, n target {target}]{eta}")
        log(f"    NEXT: {nxt}")
        asks = next((c.get("asks") for c in cells()
                     if c["arm"]["name"] == arm["name"] and c["nodes"] == nodes), None)
        if asks:
            log(f"    ASKS: {asks}")
        log(f"    gate: {arm['gate']}")

        kill_strays()
        started = time.time()
        result = None
        tail = ""
        for attempt in range(MAX_ATTEMPTS):
            try:
                result = run_unit(arm, nodes, rep, port)
                port += 1
                break
            except (Exception, SystemExit) as exc:   # SystemExit is NOT an Exception
                tail = f"{type(exc).__name__}: {exc}\n{traceback.format_exc()[-800:]}"
                log(f"[fail] {now()} {label} attempt {attempt}: {tail[:300]}")
                port += 1
                if attempt < MAX_ATTEMPTS - 1 and looks_transient(tail):
                    wait = BACKOFF[min(attempt, len(BACKOFF) - 1)]
                    log(f"[retry] transient — waiting {wait}s")
                    time.sleep(wait)
                    continue
                break

        if result is None:
            # Record the failure. A crashed unit that leaves nothing behind is a hole in the sample
            # indistinguishable from one that was never scheduled.
            d = unit_dir(arm["name"], nodes, rep)
            d.mkdir(parents=True, exist_ok=True)
            result = {"arm": arm["name"], "nodes": nodes, "rep": rep, "env": arm["env"],
                      "gate": arm["gate"],
                      "finished_at": datetime.now().isoformat(timespec="seconds"),
                      "score": None, "failed": True, "error": tail,
                      "audit_version": dispatch_audit.AUDIT_VERSION,
                      "engine_build": engine_build(), "audit": {}}

        durations.append(time.time() - started)
        result_path(arm["name"], nodes, rep).parent.mkdir(parents=True, exist_ok=True)
        result_path(arm["name"], nodes, rep).write_text(json.dumps(result, indent=2))

        a = result.get("audit") or {}
        if result.get("abandoned"):
            log(f"[abandon] {now()} {label} killed at confidence "
                f"{result.get('abandon_confidence'):.2f}: {'; '.join(result.get('abandon_reasons') or [])}")
        elif result.get("aborted"):
            log(f"[abort] {now()} {label} CUT LOOSE — {result.get('abort_reason')}")
        log(f"[done] {now()} {label}  score="
            f"{result['score'] if result.get('score') is not None else 'FAILED'}  "
            f"pool={result.get('actual_nodes')}/{nodes}  void={result.get('void')}  "
            f"aborted={result.get('aborted')}  timed_out={result.get('timed_out')}  "
            f"fallbacks={a.get('detail_fallback_count')}"
            f"{'(+' + str(len(a.get('ghost_fallback_tasks') or [])) + ' ghost)' if a.get('ghost_fallback_tasks') else ''}"
            f"{' ON:' + ','.join(a.get('shipped_one_liner_tasks') or []) if a.get('shipped_one_liner_tasks') else ''}  "
            f"kind_mismatch={a.get('kind_mismatch_pct')}%  "
            f"prefix={(result.get('prefix') or {}).get('prefix_secs')}s"
            f"/plan{(result.get('prefix') or {}).get('planning_secs')}s"
            f"/redraft{(result.get('prefix') or {}).get('redraft_rounds')}  "
            f"({round(durations[-1] / 60)} min)")

        # FEASIBILITY GATE. If the very first unit cannot get the pool it asked for, every later
        # unit is measuring the same thing under different labels — which is exactly how this
        # project's node-scaling table came to compare a configuration with itself. Stop and say so
        # rather than spend a night producing an answer to a question nobody asked.
        if len(read_results()) == 1 and result.get("void"):
            log(f"[STOP] {now()} FEASIBILITY GATE FAILED: {result.get('void_reason')}. "
                f"The engine is not building the pool the sweep asks for, so node-count cells are "
                f"not distinguishable. Stopping instead of producing an uninterpretable table.")
            STOP.write_text(f"feasibility gate: {result.get('void_reason')}\n")
            summarise()
            return 2

        summarise()


if __name__ == "__main__":
    sys.exit(main())
