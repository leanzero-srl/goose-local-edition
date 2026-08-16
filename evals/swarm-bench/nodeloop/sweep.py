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

import fcntl
import itertools
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

# THE REGIME FILE (sb-5 product tier). The scoring regime must survive a WATCHDOG restart,
# which runs with a clean environment — an env-only switch would silently revert the campaign
# to sb-4 on the first auto-restart (the F817 night proved watchdog restarts are real).
# KEY=VALUE lines, applied at IMPORT so every consumer of this module (the supervisor,
# health.py, run_build/score_build imported in-process) sees one consistent regime. Committed
# to git; main() logs it at startup. Flipping the file IS the boundary switch.
if (HERE / "REGIME.env").is_file():
    for _line in (HERE / "REGIME.env").read_text().splitlines():
        _line = _line.strip()
        if _line and not _line.startswith("#") and "=" in _line:
            _k, _, _v = _line.partition("=")
            os.environ[_k.strip()] = _v.strip()
TIMEOUT = 16200          # 4.5h. A cap that truncates the work measures the cap, not the entrant.
# n=1 is uninterpretable against a measured 46-point spread — and n=3 turns out to be barely better.
#
# COMPUTED BEFORE THE FIRST PAIR EXISTS, so this is not a threshold moved to fit a result. The node
# curve is a MATCHED-PAIR design (same spec, alternating n3/n1), so its natural test is the one-sided
# sign test, whose smallest attainable p is 0.5**n:
#
#     n=3  perfect separation -> p = 0.125   <- CANNOT REACH 0.05 EVEN IF FLAWLESS
#     n=4  perfect separation -> p = 0.0625  <- still misses
#     n=5  perfect separation -> p = 0.031   <- clears, AND tolerates nothing less
#
# Read unpaired instead (exact permutation) and n=3 reaches exactly 0.05 on perfect separation and
# 0.20 the moment ONE replicate crosses. On a fleet whose identical-config replicates have scored
# 44.2 / 86.7 / 90.0 and whose real unit walls run 6376-8729 s, **one crossing is the expected case,
# not the exception.** So n=3 spends ~12 hours of fleet time to reach a number that was never able to
# clear the bar. n=5 costs ~20 hours and can.
#
# BLAST RADIUS: this raises every SCORE cell, not just the curve. Mechanism cells (`reps == 1`) are
# capped at 1 in `backlog()` and are untouched.
MIN_REPS = 5
# The node curve's own replicate target (F327). Scoped to baseline n3/n1 only — see `backlog`.
CURVE_REPS = 8
TRANSIENT = ("500", "502", "503", "529", "overloaded", "rate limit", "throttl",
             "connection reset", "stream decode", "temporarily", "unreachable",
             # ⚠ THE FLEET BEING DOWN IS THE MOST COMMON BLIP AND IT WAS THE ONE OMISSION (F666).
             # This module's own docstring promises "a fleet blip is retried with backoff, never
             # recorded as a score of zero" — and then listed only HTTP faults. When LM Studio is
             # empty the engine refuses correctly and exits 1 in 0.1s, `looks_transient` said no,
             # and the unit was written as a real 0.0. Because each failure costs a tenth of a
             # second, THE SWEEP CONSUMED 104 QUEUED UNITS IN MINUTES — a transient outage did not
             # pause the backlog, it ANNIHILATED it, and `is_done` then skipped every one forever.
             "no models are loaded", "lms ps` is empty", "model loading is off",
             "no models loaded", "fleet is empty")
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
        # ⚠️ PREFILL REMOVED (F226). With it on, "You finished WITHOUT writing your owned file(s)"
        # hit test-authors 9 times against 3 without, on identical dispatch counts, and four
        # test-authors failed outright at 26 minutes where the prior run had zero at 99. That error
        # is a turn that COMPLETED without acting — exactly what a pre-completed assistant turn
        # produces. The remaining two levers change prompt CONTENT and cannot make a turn arrive
        # already finished.
        "env": {"GOOSE_SWARM_DEP_SIGNATURES": "1", "GOOSE_SWARM_KIND_PROMPT": "1"},
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
        "name": "probe_post",
        "env": {"GOOSE_SWARM_PROBE_ADVERTISED_POST": "1"},
        "gate": "F738/F740. The contract gate issues ONLY bare GETs, so every requirement living "
                "behind an advertised POST has never been checked by the engine on any run. That is "
                "not abstract: `vendor_conditional` and `resync_conditional_ratio` are rank 1 and "
                "rank 2 of all remaining weighted loss on the n=8 current-binary corpus (0.03982 of "
                "0.24411 = 16.3% — NOT the 44% I first reported off a 4-cell slice, which the "
                "adversarial pass refuted). Both are the spec's own sentence: a second sync must be "
                "CHEAP and must not duplicate rows. MECHANISM readout, n=1 and deterministic: the "
                "run log must carry a spec-contract POST probe with probed_post >= 1, which is zero "
                "in every cell to date — if that is absent the arm examined NOTHING and must be "
                "reported as such, never as a pass. QUALITY readout: whether the fix loop, finally "
                "shown the finding, repairs it — vendor_conditional and resync_conditional_ratio "
                "moving together on a cell that ALSO scores vendor_all_pages 1.00. A pass on a cell "
                "with vendor_all_pages 0.00 is the one-page-client loophole and settles nothing. "
                "⚠️ THE RISK IS A FALSE FINDING: this is the first WRITE the gate has ever issued, "
                "and `repeated_post_verdict` already had to be corrected once for passing the exact "
                "defect it exists to catch. If a cell reports NOT idempotent while its rows are "
                "provably fine, the verdict is wrong and the arm reverts on that alone.",
    },
    {
        "name": "scout_doc_urls",
        "env": {"GOOSE_SWARM_SCOUT_DOC_URLS": "1"},
        "gate": "C7. The OTHER half of the doc wire, and the cheap half. `doc_fetch` splices the "
                "document in from the ORCHESTRATOR; this one stops the engine LYING to its scouts. "
                "One boolean drove both the tool hint and the clause asserting the scout 'cannot "
                "look anything up on this run' - but a scout with no MCP extension still has a "
                "SHELL, and 59 of 77 archived scouts (77%) fetched a spec-named URL while under "
                "that instruction. The engine tells them a falsehood and then counts the result as "
                "grounded. MECHANISM readout, n=1: the literal `The spec names these documents` must "
                "appear in a scout's system prompt (it is in the binary but has NEVER executed, "
                "since the gate is default OFF). QUALITY readout: per-lens doc-only vendor tokens "
                "(next_cursor, Retry-After, ETag, Idempotency-Key, 429) in finding_texts, against a "
                "measured per-run spread of 4-9. HARD GUARD: prefix.research_secs (mean 7.49, sd "
                "2.51) must NOT rise - by this phase's own tier-C correlation (r=-0.650, t=-2.90) a "
                "longer research phase is a loss regardless of grounding, and a rise triggers revert "
                "on that readout alone. Do NOT settle this on `grounded`: the instruction guarantees "
                "it, so that readout is circular.",
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
        # WAS an ON arm. e2e_oracle is BAKED ON as of e620bf0b6, so setting it to "1" would measure
        # nothing against a baseline that already has it. Flipped to the OFF direction: this is now
        # the ABLATION that gives F455 its counterfactual on the SAME binary, which is the only way
        # to attribute the change rather than compare across a rebuild.
        "name": "e2e_oracle_off",
        "env": {"GOOSE_SWARM_E2E_ORACLE": "0"},
        "gate": "ABLATION of a now-baked lever. fan_e2e does not partition without it: e2e_shard_spec tells each shard to number the "
                "advertised commands 'in the order the spec gives them' and never gives it the spec, "
                "so each derives the list from the README the build itself wrote. MEASURED on one "
                "run: three shards derived lists of length 1, 1 and 3, and the one that enumerated "
                "an empty slice reported clean. This arm hands every shard the SAME engine-extracted "
                "table from spec_frozen. PREDICTION: tier C and the e2e-derived checks rise, because "
                "the shards start checking the operator's endpoints rather than the build's own "
                "documentation — and crucially the shards' reports should stop citing the README. If "
                "tier C does NOT move but the reports stop citing the README, the oracle landed and "
                "the app was already right; if the reports still cite the README, the injection is "
                "not reaching them and the arm has failed regardless of the score. AS AN ABLATION THE SIGNS "
                "INVERT: this cell should REPRODUCE the 90%-in-one-shard imbalance (baseline-n3-r0 "
                "ran 1/1/28/1 tool calls across four shards) while the baked baseline should not. "
                "PREDICTION, ONE THRESHOLD: this cell's busiest e2e shard holds 60% OR MORE of the "
                "e2e tool calls. Falsified below 60% — which would mean the oracle was never the "
                "cause and F455 credited the wrong mechanism.",
    },
    {
        "name": "spec_sized_plan",
        # F853: the engine default flipped ON — env=1 is now IDENTICAL to baseline, so this arm is
        # inert as written; the informative direction inverted to env=0 (measures the OLD fleet-
        # scaled ask as the treatment). Any queued/archived spec_sized_plan rows from before F853
        # measured the then-treatment against the then-baseline and stay interpretable via build id.
        "env": {"GOOSE_SWARM_SPEC_SIZED_PLAN": "0"},
        "gate": "BOTH branches of skeleton_count_clause size the plan to the FLEET, not the JOB: the "
                "target is derived from worker_count, which is SLOTS. The same spec is therefore "
                "asked for 'usually 2 to 4' modules on one node and 'usually 6 to 12' on three. "
                "MEASURED across four baseline cells: the 1-node ask sits BELOW what the spec needs "
                "and the model overrides it (5 and 5 modules); the 3-node ask sits ABOVE and the "
                "model drifts up into it (7 and 6), so the scaling only ever binds in the "
                "INFLATIONARY direction — +30% modules and +64% tree bytes for +0.0492 score, which "
                "is one seventh of the replicate spread. That extra code is what the serial join "
                "must then swallow, and the join grew from 20-32% to 36% of the run. This arm "
                "replaces the clause with one that takes NO fleet argument at all. PREDICTION, ONE "
                "THRESHOLD: a 3-node cell under this clause emits AT MOST 5 module subtasks, "
                "matching what the same spec yields on one node. FALSIFIED AT 6 OR MORE. Note the "
                "causal claim is only a CORRELATION today and the model already refuses the ask's "
                "upper half, so a null result here is a real possibility and would say the plan "
                "inflates for some other reason.",
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
        "name": "diverse_plan",
        "env": {"GOOSE_SWARM_DIVERSE_PLAN": "1"},
        "gate": "F438: the redraft ladder is the 3-node TAX. Every confidence_retarget in the "
                "archive carries binding_signal 'agreement', and it cost 786 / 821 / 1657s on the "
                "3-node cells against ZERO on the 1-node cell — 40-57% of the 3-node planning "
                "prefix, which is roughly the whole gain the detail fan just bought (148s per "
                "detail on 1 node vs 13-74s on 3). The cause is structural: 3 nodes draft 3 "
                "skeletons where 1 node drafts 2, and `plan_agreement` is max-min spread plus MEAN "
                "PAIRWISE JACCARD — both of which, per `best_subset_agreement`'s own doc, 'only "
                "worsen (or hold) as the pool grows'. So the bigger fleet is scored lower for its "
                "own diversity and pays a ladder the smaller fleet never pays. ENFORCE swaps in "
                "`structural_convergence`, which ignores count spread. "
                "PREDICTION: the ladder is SKIPPED, the planning prefix drops by roughly the "
                "ladder cost, and the build score does NOT move outside the replicate spread. "
                "⚠️ THE OUTCOME THAT MATTERS MORE IS THE OTHER ONE: the three cells that "
                "retargeted scored 0.9343 / 0.7147 / 0.8157 against 0.6030 / 0.6695 for the two "
                "that did not, so THE TAX MAY BE BUYING THE QUALITY. If the score falls outside "
                "the spread, the ladder is load-bearing and the correct fix is to make agreement "
                "POOL-SIZE-INVARIANT rather than to skip the redraft — a different change "
                "entirely, and worth far more than the wall-clock. "
                "GATED: armcheck refuses this arm unless the baseline's own "
                "`plan_convergence.would_skip_ladder` is true, so it can never be the null "
                "experiment `retarget_off` was twice.",
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
                "⚠️ F389 (2026-08-05): THE /v1 HALF OF THIS GATE IS OVERTAKEN — DO NOT SETTLE THIS "
                "ARM ON IT. On the current archive /v1 appears in plan_loaded in ALL FIVE cells "
                "(6/14/12/14/6) and in ALL FIVE built clients, and it IS in the spec once, inside the "
                "docs URL. So '/v1 appears in plan_loaded' would read TRUE with the lever doing "
                "nothing — an INERT arm scored as FIRED, the exact false positive mechanism_screen "
                "exists to prevent. The discriminator MOVED: only r3 ever reaches 247 payments and it "
                "is the only cell scoring 1.00 on BOTH vendor_cursor_paging and vendor_all_pages, so "
                "what the fleet still cannot read out of that unopenable document is the CURSOR/"
                "PAGINATION protocol, not the path prefix. "
                "PREDICTION (revised): doc_fetched{ok:true} fires, and the settling signal is "
                "vendor_cursor_paging + vendor_all_pages both reaching 1.00 on a cell that is not r3, "
                "with crunch.py's fetch_all_payments returning 247 rather than a short page. The "
                "mechanism claim is settled by the fetch event plus the paging pair regardless of the "
                "score — if the paging is still wrong "
                "with a 200-status fetch on record, the splice is not reaching the decomposition and "
                "the arm has failed no matter what the number does.",
    },
    {
        "name": "aux_slim",
        "env": {"GOOSE_TOOL_PAIR_SUMMARIZATION": "false"},
        "gate": "K3. Tool-pair summarization defaults ON and burns up to 10 SERIALIZED 27B calls at "
                "the end of a long worker's turn, on the worker's own node, in no budget — minutes "
                "of wall per long worker that appear nowhere. This arm turns it off and relies on "
                "compaction (which now keeps the last 3 turns verbatim, K4). WALL readout: unit wall "
                "vs the current-binary baseline mean. QUALITY falsifier: stable-24 below spread "
                "reverts — summaries may be load-bearing for the sink's long context even with K4.",
    },
    {
        "name": "amend_feature",
        "env": {
            "BENCH_SEED_TREE": "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/build/opus-5-r0",
            "BENCH_AMEND": "1",
            "BENCH_AMEND_SPEC": "/Users/mihaiperdum/Projects/goose/evals/swarm-bench/spec-amend.md",
        },
        "gate": "BENCH3 (BENCH3-AMEND.md, Mihai 2026-08-13): add a feature to the EXISTING known-good "
                "app instead of building from scratch — the brownfield axis nothing has tested. PARKED "
                "(reps 0) until sweep seed_tree support + the feature/regression scorer halves land; "
                "the design doc carries the full spec. REGRESSION half: the amended tree must hold the "
                "known-good's sb-4 level minus spread. FEATURE half: by-currency endpoint + CSV export "
                "checks (folded into the BENCH2 ranks 3-10 scorer change).",
        "asks": "can the swarm add a multi-file feature to an existing app without regressing it?",
    },
    {
        "name": "judge_nudge",
        "env": {"GOOSE_SWARM_JUDGE_NUDGE": "1"},
        "gate": "F790-1 (Mihai's direction: the judge should nudge with direction, not just kill). "
                "At 2 corroborated LOOPING looks the omni-judge now REDIRECTS the call in-session "
                "with its own hint — context preserved, attempt not burned — bounded at 2, then the "
                "abort backstop. MECHANISM, n=1: judge_nudge event fires AND the nudged call "
                "subsequently makes a tool call (read the activity digest after the nudge "
                "timestamp); a run with no loop-class call examined nothing and says so. SAFETY: "
                "nudged tasks complete at least as often as the abort era's re-dispatches. "
                "FALSIFIER: stable-24 below spread, or nudged calls that ignore both notes and eat "
                "2x wall before the backstop, kills the lever.",
        "asks": "does an in-session directed nudge convert looping calls into productive ones "
                "cheaper than kill+redispatch?",
    },
    {
        "name": "fix_sched",
        "env": {"GOOSE_SWARM_FIX_SCHED": "1", "GOOSE_SWARM_PROBE_ADVERTISED_POST": "1"},
        "gate": "F781/#16 c7 LIVE VERIFY (FIX-SCHED-DESIGN.json, adversarially verified). The fix "
                "round runs as a REAL scheduler over fix::r{N}::{file} DAG tasks on a fresh "
                "dispatcher — judge (probing SHADOWS) + tail-review supervise repair itself. "
                "MECHANISM, n=1: complete_fix_dispatched{path:'sched'} on a multi-file round, AND "
                "judge/tail events referencing fix::r ids, AND no BrokenCode storm at ~90s on a "
                "syntax-error fix (c5's probe-root proof). SAFETY: the real tree changes ONLY via "
                "promoted strictly-better shards (complete_fix_completed{path:'sched',promoted:true} "
                "with verified<baseline); round findings never rise after a promote. FALSIFIER: "
                "stable-24 below the replicate spread, or wall above the fan arm's on the same "
                "binary, kills the lever before any default-ON talk.",
        "asks": "does scheduler-run repair match the fan's quality at no worse wall, with "
                "supervision catching what the fan cannot?",
    },
    {
        "name": "sink_shard",
        "env": {"GOOSE_SWARM_SINK_SHARD": "1", "GOOSE_SWARM_PROBE_ADVERTISED_POST": "1"},
        "gate": "S1 increment 1 (S1-DESIGN.md). Two full race waves died at the per-fix cap with "
                "findings unchanged — monolithic twins re-derive whole-app context. This arm prefers "
                "the per-file fan when a round's findings partition into 2+ file groups; probe_post "
                "rides along so multi-finding rounds exist to shard. MECHANISM, n=1: "
                "complete_fix_wave{shards>=2} on a multi-finding round (absent => the arm examined "
                "NOTHING). SAFETY: round findings never rise after a promote. FALSIFIER: stable-24 "
                "score below the baseline spread reverts the arm on that alone. "
                "Increments 2+3 (in the 23:46 2026-08-11 binary) extend the readout: i2 — every "
                "shard now emits complete_fix_dispatched/completed{shard, verified_findings, "
                "promoted} and promotes ONLY a verified strictly-better tree (agent_ok true with "
                "promoted false is the mechanism catching an unverified claim — count those); "
                "i3 — file-less findings dispatch as complete-fix::cross-file with "
                "shard:'(cross-file)', baseline re-measured post-wave; a round with unassigned "
                "findings and NO cross-file event means the real-tree gate could not run (the "
                "designed skip, not a bug). SAFETY unchanged and now enforced per shard by "
                "shard_beats_baseline (pinned by test).",
    },
    {
        "name": "fill_fan",
        "env": {"GOOSE_SWARM_FILL_FAN": "1"},
        "gate": "S3 i3 (S3-DESIGN.md FINAL SHAPE): eligible hard modules (one .py file, "
                "detailer subsplit>=2) expand at plan parse into skeleton::<M> (deterministic "
                "contract-skeleton write) -> fill::<M>::<slot> xN (shadow fillers, one slot "
                "each, fence-enforced) -> join::<M> (deterministic splice; refusals keep "
                "skeleton bodies; complete gate judges). Downstream deps re-wired to the join. "
                "Read skeleton_written / join_spliced / the refusal list.",
    },
    {
        "name": "testgen",
        "env": {"GOOSE_SWARM_TESTGEN": "1"},
        "gate": "S7 (S7-DESIGN.md): idle slots generate contract-derived pytest files — the one "
                "idle job with ZERO merge surface, replacing the never-fired speculative-twin rung "
                "(Speculated: 0 in 75+ logs). Tests come from the FROZEN CONTRACTS + goal, never "
                "the code (a code-derived test cements the code's own bug); landing requires "
                "pytest --collect-only to pass from the tree root, else the file is removed. "
                "MECHANISM, n=1: testgen{landed} with tests/generated/test_gen_*.py in the unit "
                "tree; testgen{reason} rows are the honest misses (no contracts, no fence, "
                "collection failure) — count both. CAP: 3/run, claimed like every idle job "
                "(never the last free slot). QUALITY: do the landed tests FAIL anything the "
                "suite passed — a generated test that fails a green build is either the "
                "cross-execution currency working or an undocumented-value assertion; read the "
                "failure before crediting it. FALSIFIER: stable-24 below the baseline spread "
                "reverts the arm; landed=0 across the unit means the mechanism examined nothing.",
    },
    {
        "name": "doc_examples",
        "env": {"GOOSE_SWARM_DOC_FETCH": "1", "GOOSE_SWARM_DOC_EXAMPLES": "1"},
        "gate": "THE THIRD DELIVERY MECHANISM, and the parked doc_fetch entry asked for exactly this "
                "— 'do not simply flip reps; that is a different lever and deserves its own arm'. "
                "Two have failed: doc_fetch broadcast 4769 bytes into EVERY worker prompt and scored "
                "0.369 with server_runs 1.00 -> 0.00, and scout_doc_urls left both its target checks "
                "perfect while the sync family went 0.00. The document is not the problem and never "
                "was; it is the ONLY place the vendor's response shape is written, because "
                "spec-build.md documents the APP's api and not the vendor's. "
                "This arm keeps the fetch and shrinks the payload: only the fenced example blocks, "
                "MEASURED at 5 blocks / 750 of 4769 bytes = 16%, carrying `{\"data\": [...]}`, "
                "`next_cursor` and `total` verbatim — which is precisely what the failing cells get "
                "wrong. "
                "MECHANISM readout, n=1 and deterministic: `doc_fetched{ok:true}` with `bytes` around "
                "750 instead of 4769. If bytes does not shrink, the gate did not reach the fetch and "
                "the arm examined NOTHING — report it as such, never as a pass. "
                "⚠️ THE SHRINK IS NOT THE POINT, the sync is: a cell that delivers 750 bytes and "
                "still scores sync_completeness 0 has refuted the bet that reading beats guessing, "
                "and no amount of correct plumbing rescues it.",
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
    # FLIPPED 0 -> 3 at the 2026-08-11 17:49 rebuild: `probe.py --verify` confirmed every new
    # literal flipped absent -> present (probed_post + the Q2 detector family + PREREVIEW_DIMS +
    # straggler_deferred), which is exactly the condition the ⏸ note above this line demanded.
    {"arm": "aux_slim", "nodes": 3, "reps": 1,
     "asks": "whether cutting the invisible end-of-turn 27B summarization calls (up to 10 serialized "
             "per long worker) buys measurable wall at no stable-24 cost, now that K4's keep-tail "
             "carries the recent turns verbatim through compaction."},
    {"arm": "fill_fan", "nodes": 3, "reps": 1,
     "asks": "whether a hard module with a contract-anchored subsplit builds FASTER as "
             "skeleton->fills->join without losing quality — MECHANISM n=1: skeleton_written + "
             "join_spliced{spliced>=1} on an expanded module (absent => the fan examined "
             "NOTHING); WALL: the expanded module's dispatch-to-join span vs the 1522s corpus "
             "p90 for hard modules; SAFETY: join_spliced.refused stays low and every refusal "
             "names its slot (a fence refusal is the mechanism working, a majority refused "
             "means the slot discipline is not reaching the fillers); FALSIFIER: stable-24 "
             "below the baseline spread reverts the arm on that alone"},
    {"arm": "testgen", "nodes": 3, "reps": 1,
     "asks": "whether idle slots convert into contract-derived tests that pytest can collect — "
             "read testgen{landed} vs testgen{reason} (honest misses), generated files in the "
             "unit tree, and the stable-24 guard"},
    {"arm": "judge_nudge", "nodes": 3, "reps": 1,
     "asks": "whether the judge's in-session nudge (redirect with its own hint, context kept) "
             "beats the abort it replaces — read judge_nudge events first (absent on a run with "
             "no loop-class call = examined nothing), then post-nudge tool activity."},
    {"arm": "fix_sched", "nodes": 3, "reps": 1,
     "asks": "whether repair-as-a-real-scheduler-run (fix::r{N} DAG tasks on a fresh dispatcher, "
             "judge probing SHADOWS, tail-review live) matches the fan's quality at no worse wall "
             "— read complete_fix_dispatched{path:'sched'} FIRST (absent on a multi-file round = "
             "the lever examined nothing), then no BrokenCode storm at ~90s, then promoted shards "
             "strictly-better only; stable-24 below spread or wall above the fan arm kills it."},
    {"arm": "sink_shard", "nodes": 3, "reps": 1,
     "asks": "whether sharding a multi-file repair round by the existing file partition lands the "
             "substance two monolithic race waves could not — read complete_fix_wave{shards>=2} "
             "first; a run whose rounds never partition examined nothing and says so."},
    {"arm": "doc_examples", "nodes": 3, "reps": 3,
     "asks": "whether SHRINKING the payload rescues the document that two delivery mechanisms have "
             "failed to deliver. Established today and the reason this arm exists: spec-build.md "
             "documents the APP's own api and NOT the vendor's response shape, so the fetched "
             "document is the ONLY source of truth for it — and half the corpus (7 of 14 cells) "
             "scores sync_completeness 0. doc_fetch put all 4769 bytes in every worker prompt and "
             "collapsed to 0.369; scout_doc_urls cached itself into emptiness. The fenced blocks are "
             "750 bytes (16%) and contain the exact fact the failing cells get wrong. "
             "MECHANISM, n=1: doc_fetched{ok:true} with bytes ~750, not ~4769 — if it does not "
             "shrink the arm examined NOTHING. SCORE, needs the replicates: the sync family moving "
             "on a cell that is not already passing it. "
             "⚠️ PRE-REGISTERED FALSIFIER: a cell that delivers 750 bytes and still scores "
             "sync_completeness 0 refutes the bet that reading beats guessing — and the wrong-key "
             "census says 5 of 7 failing cells already read the key correctly, so the document can "
             "only ever address part of the class."},
    # ⛔ PARKED AT reps 0 BY F736 — THE PRE-REGISTERED FALSIFIER FIRED. Kept in the queue with its
    # reasoning intact rather than deleted, because a removed arm looks like one nobody thought of.
    #
    # doc_fetch-n3-r0 scored 0.369 against a same-binary baseline of 0.829 — 2.30 SD below the
    # pre-batch 3-node mean — and took 156 min against 95. EVERY tier fell (A 0.833->0.333,
    # B 0.861->0.250, C 0.857->0.429, D 0.740->0.518) and SEVEN checks unrelated to sync regressed,
    # including `server_runs` 1.00 -> 0.00: the app does not start. That is exactly the dilution the
    # falsifier named in advance — 4789 bytes in EVERY worker prompt against a measured 27B
    # compliance curve of 0.588 at 10 rules falling to 0.094 at 40.
    #
    # The DOCUMENT is not the problem; the DELIVERY is. scout_doc_urls, which tells scouts to fetch
    # it instead of injecting it everywhere, scored 0.8843 — inside the baseline band. Re-running
    # doc_fetch at reps 3 would spend ~7 fleet-hours re-confirming a result that is already
    # unambiguous, and the engine_build reset is the only reason it re-entered the queue at all.
    #
    # TO REVIVE IT, do not simply flip reps: gate `doc_facts` BY TASK KIND first (C10's own
    # pre-registered follow-up — vendor-touching implementers yes, test-authors no). That is a
    # different lever and deserves its own arm.
    {"arm": "doc_fetch", "nodes": 3, "reps": 0,
     "asks": "🔼 RE-PROMOTED BY F390 — the F53 demotion was right about the PREFIX and wrong about the "
             "DOCUMENT. F53 refuted 'losing `/v1` breaks the build' (the 83.4% unit lost it and crunch "
             "still passed 7/7, because workers have shell and re-derive a PATH), and F389 confirms the "
             "prefix half is dead: /v1 now appears in plan_loaded in ALL FIVE cells and in every built "
             "client. But the demotion generalised one refuted claim to the whole arm. The document is "
             "4769 bytes (MEASURED by serving vendor_service and fetching /v1/docs — well under "
             "doc_fetch's 24000-byte cap, so NOTHING is truncated) and it carries `cursor` x11, "
             "`next_cursor` x3, ETag/If-None-Match and Retry-After. A worker re-derives a PATH by trying "
             "it; it does not re-derive a CURSOR PAGINATION PROTOCOL by trial and error, and the archive "
             "agrees — only 1 of 5 cells ever retrieves all 247 payments. "
             "WHY THIS IS NOW THE TOP ARM: the sync-dependent family (payment_row_shape, total_field, "
             "chronological_order, summary_accuracy, summary_bounds_utc, concurrent_sync_safe, "
             "local_pagination, resync_idempotent — SEVEN of eight in tier B, the heaviest tier) is the "
             "single largest block of score on the board. If every 3-node cell synced like r3 the arm "
             "mean goes 0.6390 -> 0.7717 and the gap +0.0593 -> +0.1920, which the PRE-REGISTERED 5 "
             "pairs can settle (F385: as measured it needs 51). Nothing else examined today is within an "
             "order of magnitude of that. "
             "⚠️ THAT COUNTERFACTUAL SIZES THE PRIZE, NOT THIS LEVER'S EFFECT — r3 synced WITHOUT "
             "doc_fetch, by guessing the protocol. This arm is the bet that reading beats guessing; it "
             "is not evidence that it does. "
             "⚠️ THE REAL RISK IS DILUTION, not failure: 4769 bytes (~1.2k tokens) lands in EVERY "
             "worker's prompt, and measured 27B compliance falls 0.588@10 rules -> 0.094@40. A drop in "
             "checks UNRELATED to sync is the signal that the document is crowding out the task. "
             "MECHANISM readout (revised — the old one was `/v1` count in plan_loaded, which F389 showed "
             "now reads TRUE with the lever doing nothing): `doc_fetched{ok:true, status:200}` PLUS "
             "vendor_cursor_paging and vendor_all_pages both reaching 1.00 on a cell that is not r3. "
             "reps=3 because the mechanism settles at n=1 but the score cannot (F382)."},
    {"arm": "baseline", "nodes": 3, "reps": 3,
     "asks": "the replicate spread on this engine (every score comparison is measured against it), "
             "AND whether the F49 detail-budget fix drove detail_fallback to zero — that second half "
             "is a mechanism readout and is answered by the FIRST unit, not the third."},
    # PROMOTED BY F436 from position 10 of 15 — roughly two days of fleet time away — to cell 1 of 19,
    # which is the FIRST cell after the current baseline. `cells()` splices the node curve in after
    # QUESTIONS[:2], so sitting at QUESTIONS index 1 puts this arm BEFORE the curve, not after it, and
    # that is deliberate: it costs the curve exactly one cell, and F430 showed the curve's score
    # comparison cannot settle anything at n=1 per side anyway while this arm's readout is a
    # deterministic mechanism event that is valid at n=1. Verified by running cells() after the edit
    # rather than by reading the slice — an ordering asserted in a comment is how arms go missing here.
    {"arm": "sink_review", "nodes": 3, "reps": 1,
     "asks": "GOAL ONE, THE BOTTLENECK ARM. F436 measured the sink second-by-second across four "
             "3-node cells: integrate-verify holds 53.1 / 60.6 / 30.7 / 15.6 percent of the whole "
             "dispatch window, and in the BEST CELL EVER RECORDED (0.9343) one hundred percent of "
             "that 56.2 minutes sits at 2 or fewer of 6 slots. The existing pre-review fill uses only "
             "8.6% and 18.9% of the sink's idle node-time in the two cells where it is measurable — "
             "its work-list is one review per task, so it EXHAUSTS long before the sink does. This is "
             "the only mechanism built to fill the rest, and gates.sink_review is False in all 6 "
             "archived cells with the `sink_review` event never once emitted (proven with a positive "
             "control: sibling gates in the same block are present and True). "
             "MECHANISM READOUT, valid at n=1: `sink_review{prewarmed>0}`. If prewarmed is 0 with the "
             "lever on, the producer still cannot see its precondition and the fix is incomplete. "
             "OCCUPANCY READOUT, the number that matters: the share of the dispatch window spent at "
             "<=2 concurrent must FALL against the 3-node baseline band of 27.6-53.9%. "
             "FALSIFIER: the event fires, prewarmed > 0, and the <=2 share does NOT move — then the "
             "idle-fill runs but cannot keep the slots busy, and the ceiling is the work-list, not "
             "the lever. The findings are ADVISORY and re-verified fail-closed, so the build score "
             "should NOT move outside the replicate spread; if it moves DOWN, the re-verification is "
             "not fail-closed and THAT is worth more than the utilisation."},
    # F436 and F438 are the two halves of one answer — the sink and the prefix are most of a run and
    # neither scales with the fleet — so this sits as close to sink_review as the ordering allows.
    # VERIFIED BY RUNNING cells(), not by reading the slice: this is QUESTIONS[2], and cells() splices
    # the node curve in after QUESTIONS[:2], so it lands at cell 4 with the curve BETWEEN it and
    # sink_review, not directly behind it. Left there deliberately — the curve's own reps are what
    # F430 showed the score comparison lacks — but the comment says where it actually goes, because an
    # ordering asserted in a comment is how arms go missing in this file.
    # armcheck GATES this on the baseline's own `plan_convergence.would_skip_ladder`, so if the
    # counterfactual says ENFORCE would change nothing the arm is refused BEFORE it spends a unit.
    {"arm": "probe_post", "nodes": 3, "reps": 1,
     "asks": "whether the engine can SEE the requirement that is rank 1 and rank 2 of all "
             "remaining weighted loss. The contract gate issues only bare GETs, so the spec's "
             "own sentence — a second sync must be cheap and must not duplicate rows — has "
             "never been checked on any run, and the repair loop cannot fix what nothing "
             "reports. MECHANISM, n=1: the run log must carry probed_post >= 1; it is zero in "
             "every cell to date, so absence means the arm examined NOTHING and is reported as "
             "such rather than as a pass. The apps already send If-None-Match (measured 0 of "
             "13, 0 of 16, 0 of 13 answered 304, every mismatch shifted exactly ONE PAGE) — "
             "they replay a single ETag on the next request, so the finding now names the real "
             "repair: key each page ETag by path plus offset plus limit. FALSIFIER: a NOT-idempotent finding on a cell whose rows are provably correct means the verdict is wrong and "
             "the arm reverts on that alone."},
    {"arm": "scout_doc_urls", "nodes": 3, "reps": 1,
     "asks": "whether the engine can stop telling 77% of its scouts a falsehood, and whether "
             "that changes what they bring back. MECHANISM, n=1 and deterministic: the literal "
             "\"The spec names these documents\" must appear in a scout system prompt — it is in "
             "the binary (probe verified the flip) but has NEVER executed, because the gate is "
             "default OFF. QUALITY: per-lens doc-only vendor tokens in finding_texts against the "
             "measured 4-9 per-run spread. GUARD: research_secs must not rise; a rise reverts the "
             "arm on that readout alone. Paired with doc_fetch, which runs immediately before it — "
             "doc_fetch hands the document to WORKERS from the orchestrator, this hands it to "
             "SCOUTS via their own shell, and the two are separable because they touch different "
             "phases. NOT settled on `grounded`: the instruction guarantees it, so it is circular."},
    {"arm": "diverse_plan", "nodes": 3, "reps": 2,
     "asks": "GOAL ONE, THE PREFIX HALF. Does removing the pool-size penalty on plan agreement give "
             "the 3-node fleet back the 786-1657s redraft ladder it pays and the 1-node fleet does "
             "not? MECHANISM, valid at n=1: `confidence_retarget` count falls to zero and "
             "`plan_convergence{enforced: true}` shows struct_conv replacing agreement_conf. "
             "WALL-CLOCK: the pre-dispatch prefix should fall from the 3-node mean of 2025s toward "
             "the ~1250s the two no-ladder cells actually took. "
             "FALSIFIER, and the more valuable outcome: if the build score drops below the "
             "replicate spread, the ladder is LOAD-BEARING — the redraft is buying the quality, not "
             "wasting the fleet — and the correct fix becomes making agreement pool-size-invariant "
             "rather than skipping the ladder. reps=2 because the mechanism settles at n=1 but the "
             "score direction is the thing that decides which fix is right."},
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
    # F708: these two arms carried reps>0 but were named in NO question, so `cells()` could never
    # schedule them and the sweep warned about it on EVERY pass, unread, for a long time. Both were
    # locked out — and both test defects the 2026-08-09 phase audit independently REDISCOVERED from
    # scratch, which is the expensive part: the experiment that would have answered the question was
    # already written and could not run.
    {"arm": "e2e_oracle_off", "nodes": 3, "reps": 1,
     "asks": "ABLATION of the baked oracle, and it now has a corroborating measurement it did not "
             "have when it was written: 27% of e2e shards and integrate-verify in 17 of 25 runs "
             "completed with status=done and ZERO shell tool calls. This arm's own gate explains "
             "HOW — a shard that derives an empty command slice from the build's README reports "
             "CLEAN without running anything. MECHANISM readout on the new verify_coverage.exec_rate, "
             "not on the score."},
    {"arm": "spec_sized_plan", "nodes": 3, "reps": 1,
     "asks": "does sizing the plan to the JOB instead of the FLEET break the depth-4 ceiling? The "
             "audit measured DAG depth EXACTLY 4 in 14 of 14 three-node runs (zero variance) while "
             "EXECUTE is critical-path bound in 13 of 14 — and skeleton_count_clause derives its "
             "target from worker_count, which is SLOTS. Same worker_count-is-SLOTS confusion as the "
             "e2e fan's clamp(worker_count,2,4). MECHANISM: plan_loaded depth and root width."},
    {"arm": "amend_feature", "nodes": 3, "reps": 1,
     "asks": "BENCH3 brownfield: can the swarm add a multi-file feature to the existing known-good "
             "app without regressing it? UNPARKED (seed_tree + scorer halves + spec landed); deliberately LAST in the queue — it must never jump the decisive set "
             "(BENCH3-AMEND.md); reps 0 keeps it visible in every banner without burning fleet time."},
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
    # AN ABANDONED OR NODE-SHORT UNIT IS NOT A COMPLETE UNIT.
    #
    # MEASURED (F227): all three `think_off` reps sat on disk marked complete, carrying scores of
    # 0.0357 / 0.0918 / 0.0357 — with `abandoned: True` and `actual_nodes: 2`. They were runs I
    # killed mid-flight during the fleet outage and the restart cycles, and every one of them wrote
    # a scored result. Because this function looked only at instrument and engine versions, the arm
    # was skipped forever and a fabricated 3.5% would have stood as `think_off`'s answer.
    #
    # `abandoned` is the supervisor's own verdict that the unit was not worth finishing; `aborted`
    # is an explicit kill. Neither produces a measurement. And a unit whose ENGINE-RESOLVED pool is
    # smaller than the pool it asked for is a different experiment wearing the right name — the
    # campaign has had that rule since the beginning ("mismatch ⇒ row marked VOID") and it was
    # never enforced HERE, which is the only place that decides whether to re-run.
    if r.get("abandoned") or r.get("aborted"):
        return False
    # ⚠ THE THIRD INSTANCE OF THIS EXACT CLASS (after F227's abandoned and F665's None-pool): the
    # F784 void-on-STOP fix mints rows with void=True, score=None, FULL pool and CURRENT engine —
    # and nothing here knew. Measured 2026-08-14: fill_fan-n3-r0, killed by Mihai's fleet stop and
    # correctly voided, counted COMPLETE and the queue skipped straight to testgen — the voided
    # single would have been skipped forever, its mechanism question silently unanswered. A VOID
    # ROW IS NOT A MEASUREMENT.
    if r.get("void"):
        return False
    want, got = r.get("nodes"), r.get("actual_nodes")
    # ⚠ THE SAME `None`-EXEMPTION AS THE VOID GUARD, IN THE ONE PLACE THAT DECIDES RE-RUNS (F665).
    # `isinstance(None, int)` is False, so a unit that NEVER REPORTED A POOL sailed past the
    # node-shortfall check and counted as complete — skipped forever, its fabricated 0.0 standing as
    # that arm's answer. That is the identical "missing is treated as fine" defect the void guard
    # had, in a second location, and it is why re-running the lever arms could never have healed the
    # corpus on its own. A run with no pool is not a short run; it is not a run.
    if got is None:
        return False
    if isinstance(want, int) and isinstance(got, int) and got < want:
        return False
    if r.get("harness_ok") is False:
        return False
    # THE REGIME GATE (sb-5 product tier, Mihai 2026-08-15). A row scored by a different grader
    # answers a different question — the row's own scorer_version is compared to the version the
    # CURRENT regime produces (REGIME.env → BENCH_PRODUCT → sb-5). Same class as the engine_build
    # check below: after the regime flips, every old-regime row re-runs rather than standing as a
    # silent answer measured by the wrong instrument.
    # THE SCORER'S OWN CONSTANT, never a parallel literal: the first sb-5.1 bump left a
    # hardcoded "sb-5" here for eight minutes — two versions of one rule, the exact
    # drift class this file documents everywhere else. Imported late so a scorer syntax
    # error cannot stop the loop from starting.
    import score_build
    expected_scorer = score_build.SCORER_VERSION
    if r.get("scorer_version") != expected_scorer:
        return False
    # ROLLING MODE (Mihai, Sunday 2026-08-16 evening: one-run-at-a-time improvement — each
    # unit may run on a newer binary than the last). BENCH_ROLLING=1 accepts a completed row
    # from ANY binary (scorer + audit versions still enforced; each row records its build for
    # the ledger). Without the flag, the strict same-binary rule stands — that mode is how a
    # formal single-binary curve gets re-run at stabilization.
    if os.environ.get("BENCH_ROLLING"):
        return r.get("audit_version") == dispatch_audit.AUDIT_VERSION
    return (r.get("audit_version") == dispatch_audit.AUDIT_VERSION
            and r.get("engine_build") == engine_build())


def looks_transient(tail: str) -> bool:
    low = (tail or "").lower()
    return any(t in low for t in TRANSIENT)


LMS = os.path.expanduser("~/.lmstudio/bin/lms")


def fleet_loaded() -> int:
    """How many models `lms ps` reports. READ-ONLY — this never loads, unloads or re-aliases.

    Returns -1 when the probe itself failed, which is NOT the same as an empty fleet and must never
    be treated as one.
    """
    try:
        out = subprocess.run([LMS, "ps"], capture_output=True, text=True, timeout=60)
    except Exception:
        return -1
    if out.returncode != 0:
        return -1
    # ⚠ PARSE THE REAL OUTPUT, NOT THE ONE I ASSUMED. My first version counted lines containing
    # "Identifier:" and returned ZERO against a healthy three-node fleet with all three GENERATING —
    # `lms ps` prints a TABLE whose header is `IDENTIFIER  MODEL  STATUS ...`, with no colons
    # anywhere. wait_for_fleet would then have blocked the sweep for its full two-hour ceiling on
    # every single unit, on a working fleet: a "fix" strictly worse than the bug it replaced. Caught
    # only by running the probe against the actual command instead of trusting the parser.
    rows = 0
    for ln in out.stdout.splitlines():
        s = ln.strip()
        if not s or s.startswith("IDENTIFIER"):
            continue
        if len(s.split()) >= 4:
            rows += 1
    return rows


def wait_for_fleet(ceiling_secs: int = 7200) -> bool:
    """Block until the fleet has models again. Waiting beats consuming the backlog (F666).

    Retry-with-backoff alone still marches through every queued unit during a long outage — 104
    units at ~5 min of backoff each is most of a day spent manufacturing failures. Unattended, the
    right response to a shared resource being briefly gone is to WAIT, not to spend the night
    proving it is gone once per unit.

    A probe failure (-1) is treated as "unknown, keep waiting", never as "empty" — the negative that
    would authorise marching on has to be a POSITIVE observation of an empty fleet, not a broken
    probe. Returns False if the ceiling is reached, so the caller can still make progress.
    """
    if fleet_loaded() > 0:
        return True
    waited = 0
    log(f"[fleet] {now()} FLEET IS DOWN — holding the sweep rather than burning the backlog "
        f"(ceiling {ceiling_secs // 60} min). Nothing is being reconfigured; this only watches.")
    while waited < ceiling_secs:
        time.sleep(60)
        waited += 60
        # STOP MUST BE HONOURED HERE TOO, AND IT WAS NOT.
        #
        # MEASURED 2026-08-10: the fleet went down at 08:25, STOP was armed at 08:30, and the sweep
        # ignored it — this loop only ever checks the fleet, so `./loop.sh stop` could not take
        # effect until the fleet returned or the 120-minute ceiling expired. An operator who stops
        # the loop during an outage is asking to stop NOW; making them wait out a ceiling for a
        # resource that is already gone is the opposite of what the request means. It also blocks
        # the one thing an outage is genuinely good for: rebuilding while no cell can possibly run.
        if STOP.exists():
            log(f"[fleet] {now()} STOP observed during the fleet wait — exiting cleanly after "
                f"{waited // 60} min. No cell is running, so nothing is interrupted.")
            raise SystemExit(0)
        n = fleet_loaded()
        if n > 0:
            log(f"[fleet] {now()} fleet is back ({n} model(s) loaded) after {waited // 60} min — resuming")
            return True
        if waited % 600 == 0:
            log(f"[fleet] {now()} still down after {waited // 60} min (probe={n})")
    log(f"[fleet] {now()} ⚠ fleet still down at the {ceiling_secs // 60} min ceiling — proceeding so "
        f"the loop cannot wedge forever; expect the next unit to fail and be retried.")
    return False


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


# A unit is a full swarm build; measured walls on this fleet are 1.9-2.5h. Sixty seconds is
# three orders of magnitude below the floor, so it separates "never started" from "fast"
# without any risk of excluding a real run (F349).
MIN_REAL_UNIT_SECS = 60


def is_real_unit(r: dict) -> bool:
    """Did this unit actually MEASURE something, i.e. may its wall-clock describe a normal unit?

    A VOID unit is a refusal, not a run. The pool-mismatch gate turns one round in ~60 seconds and
    writes a result file, so before this predicate existed FIVE 60-second refusals sat in a
    nine-unit "finished" population and dragged the median to 60.3s against a real median of 7237s
    — a 114x understatement that made every downstream "too long" judgement meaningless.

    The rule lives HERE, once, because it had already been written twice with different filters:
    `median_unit_secs` excluded timed_out/aborted and let voids through, and the ETA's `durations`
    excluded nothing at all. Two copies of one rule that disagree is the shape of defect this
    campaign keeps paying for.

    ⚠ A UNIT THAT NEVER STARTED PASSES EVERY FLAG ABOVE. It is not void (nothing refused it), not
    aborted, not timed out — it simply returned in 0.2s with score 0.0 because the fleet had no
    models loaded. MEASURED 2026-08-05: 113 such rows entered the corpus in twenty minutes, and
    `curve.py` — the instrument that decides goal one — paired them up and published
    "score: 4/8 pairs favour 3 nodes, p = 0.6367" off SEVEN pairs that were fabricated from
    no-log zeros. The one real pair said something quite different.

    `harness_ok` is the scorer's own verdict and it was already False on every one of those rows;
    nothing read it. The wall floor is deliberately a SECOND, independent condition rather than a
    tidier single test: a gate that shares its only input with the thing it guards goes silent the
    moment that input is missing, and `harness_ok` is absent entirely from rows written before it
    existed (`.get()` returns None, which is not False).
    """
    if r.get("harness_ok") is False:
        return False
    if (r.get("wall_secs") or 0) < MIN_REAL_UNIT_SECS:
        return False
    return bool(r.get("wall_secs")) and not r.get("timed_out") \
        and not r.get("aborted") and not r.get("void")


def median_unit_secs() -> float | None:
    """Median wall of units that actually finished, so "too long" is measured, not guessed."""
    walls = [r["wall_secs"] for r in read_results() if is_real_unit(r)]
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
        # ⚠⚠ MEASURE SILENCE, NOT DURATION. A LONG PREFIX IS NOT A STUCK ONE.
        #
        # This rule keyed on total elapsed and would have killed a perfectly healthy run. The redraft
        # ladder is a DESIGNED branch: `confidence_retarget` -> `retarget_discarded` -> re-plan, and
        # F303 measured redrafting prefixes of 1730.9 / 2218.7 / 2839.0 / 2882.7 s against no-redraft
        # prefixes of 1091-1330 s. `baseline-n3-r3` tripped the 2400 s rung at conf 0.50 with two
        # discards — and a THIRD discard costs another ~700-1000 s, which puts the prefix past 3600 s
        # and onto a rung of 0.85, ABOVE the 0.8 abandon line. The rule's own comment says it is
        # "weighted below the kill line on its own"; the 3600 s rung is not, and it would have
        # abandoned a healthy cell and VOIDED ITS PAIR — the most expensive possible false positive on
        # goal one, because a dropped pair is worse than a lost one (F327).
        #
        # The engine states its own progress. `skeleton_drafts`, `confidence_retarget`,
        # `retarget_discarded` and `plan_loaded` are deterministic events, and one arriving two
        # minutes ago proves planning is advancing whatever the total elapsed says. So the question is
        # not "how long has this run taken" but "how long since the engine last did anything".
        PLANNING_EVENTS = {"skeleton_drafts", "confidence_retarget", "retarget_discarded",
                           "plan_loaded", "research_findings", "scout_done", "pool_resolved"}
        last = None
        for e in events:
            if e.get("event") in PLANNING_EVENTS:
                try:
                    t = datetime.fromisoformat(str(e.get("ts")).replace("Z", "+00:00")).timestamp()
                except (ValueError, TypeError):
                    continue
                last = t if last is None else max(last, t)
        quiet = (time.time() - last) if last else elapsed
        if quiet > 3600:
            conf = max(conf, 0.85)
            reasons.append(f"{quiet / 60:.0f} min since the last planning event and ZERO dispatches — "
                           f"planning has genuinely stopped (total elapsed {elapsed / 60:.0f} min)")
        elif quiet > 2400:
            conf = max(conf, 0.5)
            reasons.append(f"{quiet / 60:.0f} min since the last planning event, no dispatch yet "
                           f"(total elapsed {elapsed / 60:.0f} min)")

    # 4. FAR BEYOND THE MEASURED NORM. Uses the median of units that actually finished, so it adapts
    #    rather than encoding a guess. Alone it is under the line — slow is not doomed — but it
    #    compounds with anything else.
    # 2.5x was chosen when the median read 60s. Against the REAL median (7237s) it lands at 18092s,
    # which is beyond the 16200s unit cap — the rule could never fire, and a rule that cannot fire is
    # dead code wearing the costume of a safeguard. 1.8x fires at 13027s: 1.49x the slowest real unit
    # ever observed (8729s), and still inside the cap.
    # FALSIFIER: if a unit that later yields a VALID (non-void, scored) result ever trips this, 1.8 is
    # too tight and goes back up. Recorded before the first unit runs under it.
    med = median_unit_secs()
    if med and elapsed > 1.8 * med:
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


def _etime_secs(et: str) -> int:
    """ps ETIME -> seconds. Formats: MM:SS, HH:MM:SS, D-HH:MM:SS."""
    days, _, rest = et.rpartition("-")
    parts = [int(p) for p in rest.split(":")]
    while len(parts) < 3:
        parts.insert(0, 0)
    return (int(days) if days else 0) * 86400 + parts[0] * 3600 + parts[1] * 60 + parts[2]


def reap_run_orphans(min_age_secs: int = 10800, protect_root: int | None = None,
                     orphan_age_secs: int = 600, dry_run: bool = False) -> list[int]:
    """Kill orphaned worker-spawned processes by their WORKING DIRECTORY, not by port range.

    `reap_stray_listeners` below was calibrated on ONE observed leak — a worker that ran
    `python3 -m vendorsync --db … --port 8931 &` — so it sweeps PORT_BASE..PORT_BASE+40. MEASURED
    2026-08-06: 68 orphaned processes had accumulated over FOUR DAYS, and **not one of them was in
    that range**. The apps under test pick their own ports; the live leaks held 18082, 19000, 18080,
    18095-18101, 18765, 18123, 19001, 19004-19006, 8000, 8081, 8099, 9000, 9090, 9999, 38081. The
    reaper had been running between every unit and finding nothing, correctly, forever.

    Widening the port range is NOT the fix: 8000/9000/9999 are exactly where a person's own dev
    server lives, and this runs unattended on Mihai's machine. So the discriminator is the CWD —
    every one of these was spawned by a worker inside a run directory, and nothing of his is in
    there. The sweep itself sits at evals/swarm-bench (not under runs/), so it can never match.

    AGE IS THE SECOND GUARD, and it is load-bearing rather than belt-and-braces: run directories are
    REUSED across sweeps. Four processes with cwd `runs/nodeloop/swarm-3node-r1` were 2d20h old while
    that cell had been running for 80 minutes — same path, different run. Age separates them where
    the path cannot.

    Kills the process GROUP: the leak is a `bash -c "… &"` whose python child holds the socket, and
    killing only the pid the CWD scan matched leaves the listener alive.

    ⚠️ THE THREE-HOUR FLOOR IS TOO SLOW FOR THE COMMONEST LEAK, which is why `orphan_age_secs` exists.
    MEASURED 2026-08-07, DURING the cell it was corrupting: eleven orphans in three groups had burned
    FIFTY CPU-MINUTES, and the two doing the burning were pytest runs spinning at 39% and 63% of a
    core — 55 and 48 minutes old, both comfortably UNDER the floor, both invisible to this function.
    Their start times match the run's two `agent stalled — no progress for 420s` retries to the
    minute: the worker ran pytest, pytest never returned, the watchdog restarted the attempt, and
    nobody killed the pytest. So the engine manufactures one of these every 420 seconds of stall,
    and the reaper was set to notice three hours later.

    PPID 1 IS THE DISCRIMINATOR THE AGE GUARD WAS STANDING IN FOR. A live worker's child has the
    engine as its parent; reparenting to init happens only when the parent is already dead. So a
    ppid-1 process under a run directory has nobody waiting on it BY DEFINITION, not by inference
    from its age, and it gets the short floor. Everything else keeps the three-hour one — that guard
    was written for a different case (a stale process sharing a REUSED path with a live cell) and is
    untouched here.

    The ancestry walk is unaffected: a process that descends from this sweep never has ppid 1, so the
    engine and its shells cannot reach the short path however they are grouped.
    """
    root = str((HERE.parent / "runs").resolve())
    try:
        out = subprocess.run(["lsof", "-d", "cwd", "-Fpn"],
                             capture_output=True, text=True, timeout=60).stdout
        ps = subprocess.run(["ps", "-eo", "pid=,pgid=,etime=,ppid="],
                            capture_output=True, text=True, timeout=20).stdout
    except Exception:
        return []
    cwds: dict[int, str] = {}
    pid = None
    for line in out.splitlines():
        if line.startswith("p"):
            pid = int(line[1:]) if line[1:].isdigit() else None
        elif line.startswith("n") and pid is not None:
            cwds.setdefault(pid, line[1:])
    # (pgid, age_secs, ppid) — ppid is what makes the ancestry walk possible.
    meta: dict[int, tuple[int, int, int]] = {}
    for line in ps.splitlines():
        f = line.split()
        if len(f) >= 4 and f[0].isdigit():
            try:
                meta[int(f[0])] = (int(f[1]), _etime_secs(f[2]), int(f[3]))
            except ValueError:
                continue
    # PARAMETERISED SO THE GUARD CAN BE CONTROLLED. Defaults to this process; a test passes the
    # live sweep's pid, because a safety guard that can only be exercised by BEING the sweep is a
    # guard nobody has ever checked. Verifying it required exactly this: from a throwaway process
    # the engine does not descend from ME, so the control read as a failure until the root was
    # made explicit.
    me = protect_root if protect_root is not None else os.getpid()

    def descends_from_me(p: int) -> bool:
        """Is `p` anywhere in THIS sweep's process tree?

        A process-GROUP guard is not enough, and getting that wrong would be the worst bug in this
        file. `goose swarm run` is spawned in its own group (measured: sweep pid 23655 is pgid 23655,
        its live engine 87167 is pgid 87167) and the engine's CWD *is* a run directory — so a
        group-only check leaves the running engine matching every condition here, with nothing but
        the age guard between it and SIGKILL. Cells take ~2.5h against a 3h floor. That is not a
        margin, it is a countdown.

        Walking ppid to the root protects the engine, its shells and their children by construction,
        however they are grouped.
        """
        seen = set()
        while p and p not in seen:
            if p == me:
                return True
            seen.add(p)
            p = meta.get(p, (0, 0, 0))[2]
        return False

    groups: dict[int, list[int]] = {}
    for p, c in cwds.items():
        if not c.startswith(root) or p == me:
            continue
        pgid, age, ppid = meta.get(p, (None, 0, 0))
        if pgid is None or descends_from_me(p):
            continue
        if age < (orphan_age_secs if ppid == 1 else min_age_secs):
            continue
        groups.setdefault(pgid, []).append(p)
    killed = []
    for pgid, pids in groups.items():
        if dry_run:
            killed.extend(pids)
            continue
        try:
            os.killpg(pgid, signal.SIGKILL)
            killed.extend(pids)
        except (ProcessLookupError, PermissionError):
            continue
    return killed


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


NEVER_RAN = "no run log"


def unit_is_void(actual_nodes, nodes: int, harness_ok, harness_detail: str = "") -> bool:
    """Is this unit's number evidence? Pure, so it can be tested without a fleet.

    Extracted because the inline version carried an `actual_nodes is not None` exemption that voided
    a MISMATCH while passing MISSING. `None != nodes` is True, which is the whole point: a run that
    never reported a pool is the most broken outcome available and must never outrank one that
    reported the wrong number.

    ⚠ THE `harness_ok is False` CLAUSE I ADDED THIS MORNING WAS DESTROYING REAL EVIDENCE (F698).
    I verified it as safe on the LIVE corpus — "0 non-void rows carry harness_ok False" — and that
    check was true and useless, because the live corpus is a SURVIVORSHIP SNAPSHOT (F694/F695).
    Across all 31 run trees on disk the boolean covers TWO DIFFERENT FAILURES:
      654 rows fail st-2, detail "invariant pass CRASHED: no run log" — the fleet-outage phantoms,
          0.1 s, no pool, nothing ran. These are correctly void.
        3 rows fail st-1 (dispatch/completion pairing) — REAL 112-124 min runs with pool 3/3 and
          scores 0.7186 / 0.672 / 0.819, one of them `retarget_off-n3-r0`, a REAL LEVER RUN.
          Voiding these deletes the scarcest data the campaign has.
    So the void keys on the st-2 SIGNATURE, never on the boolean. A harness self-test that failed for
    any OTHER reason marks a unit as suspect — which `harness_ok` already records — and must not
    silently erase a run that actually executed. 654 against 3 is not a close call.
    """
    if actual_nodes != nodes:
        return True
    return harness_ok is False and NEVER_RAN in (harness_detail or "")


def test_unit_is_void() -> None:
    """The F664 regression. 104 of 133 rows scored 0.0 and read as real because of one clause."""
    assert unit_is_void(None, 3, True, ""), "THE F664 BUG: a run that never reported a pool must be VOID"
    assert unit_is_void(None, 1, True, ""), "same at one node"
    assert unit_is_void(1, 3, True, ""), "a pool mismatch is void (this half always worked)"
    # F698: the boolean alone destroyed REAL runs. Void only on the st-2 "never ran" signature.
    assert unit_is_void(3, 3, False, "invariant pass CRASHED: no run log under /x"), \
        "a run that NEVER RAN (st-2) must be VOID"
    assert not unit_is_void(3, 3, False, "HARNESS SELF-TEST FAILED (st-1, controls + invariants)"), \
        "THE F698 BUG: a REAL 112-min run that failed st-1 must NOT be voided"
    assert not unit_is_void(3, 3, False, ""), "a bare False with no signature must not void a real run"
    assert not unit_is_void(3, 3, True, ""), "a matching pool with a passing self-test is REAL"
    assert not unit_is_void(1, 1, True, ""), "and at one node"
    # harness_ok is None means the audit never reported, which is not a refusal.
    assert not unit_is_void(3, 3, None, ""), "an unreported audit must not void a good run"
    print("test_unit_is_void: PASS — st-2 voids, st-1 does NOT (F698)")


def run_unit(arm: dict, nodes: int, rep: int, port: int) -> dict:
    """One episode: build, grade the artifact, then grade the INSTRUCTIONS it was given."""
    import run_build  # imported late so a syntax error there cannot stop the loop from starting

    entrant = f"swarm-{nodes}node"   # run_build reads the N and sets GOOSE_SWARM_MAX_NODES
    # STAMP THE BINARY AT DISPATCH, NOT AT RESULT-WRITE.
    #
    # `engine_build()` stats the binary, and the result dict used to call it AFTER the run returned —
    # so rebuilding while a cell was in flight stamped that finished cell with the NEW binary's
    # identity although it had executed entirely on the OLD one. A confident, wrong attribution, which
    # is the precise failure this field was added to prevent: its own docstring records 34 hours of
    # backlog queued against a binary predating the levers the arms set, reported as "no effect".
    #
    # The campaign rule "never rebuild mid-cell" was carrying this on discipline alone. Now the code
    # carries it: a mid-cell rebuild can still mix binaries, but it can no longer LIE about which one
    # ran, and the mismatch shows up as a stale `engine_build` that `is_done()` re-runs.
    engine_build_at_dispatch = engine_build()
    # F811: a SAME-BINARY boundary-STOP void is resumable — hand its dir to run_build so the
    # engine reloads the plan and re-runs against the warm tree instead of from scratch. Marked
    # via resumed_from in the result so wall aggregates can exclude it (the prologue skip makes
    # its wall incomparable); the SCORE stays valid — the app is judged as built. Different
    # binary => no resume (a plan from another engine is a different experiment).
    resume_from = ""
    _prev_dir = unit_dir(arm["name"], nodes, rep)
    _prev_res = _prev_dir / "nodeloop-result.json"
    if _prev_res.is_file():
        try:
            _r = json.loads(_prev_res.read_text())
            if (_r.get("void") and "boundary STOP" in str(_r.get("void_reason", ""))
                    and _r.get("engine_build") == engine_build_at_dispatch):
                resume_from = str(_prev_dir)
        except Exception:
            pass
    prev = dict(os.environ)
    # F812 (Mihai: "check the node count used in LM Studio too"): GROUND-TRUTH the pool against
    # the independent 30s fleet sampler — goose's actual_nodes is the ENGINE'S OWN claim, and the
    # node curve deserves an oracle goose cannot influence. Window by FILE OFFSET, not
    # timestamps (the tsv carries time-of-day only).
    _fleet_tsv = OUT / "fleet-samples.tsv"
    _fleet_off = _fleet_tsv.stat().st_size if _fleet_tsv.is_file() else 0
    dog = Watchdog(unit_name(arm["name"], nodes, rep), OUT / f"{entrant}-r{rep}", arm, nodes)
    dog.start()
    try:
        for k, v in arm["env"].items():
            os.environ[k] = v
        if resume_from:
            os.environ["BENCH_RESUME_FROM"] = resume_from
            log(f"[resume] {now()} {unit_name(arm['name'], nodes, rep)} resumes from its "
                f"boundary-STOP void (same binary) — plan reloads, tasks re-run warm")
        verdict = run_build.run(entrant, rep, OUT, TIMEOUT, port)
        if resume_from:
            verdict["resumed_from"] = resume_from
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
        orphans = reap_run_orphans()
        if orphans:
            log(f"[reap] {now()} killed {len(orphans)} orphaned worker-spawned process(es) rooted "
                f"in run directories: {orphans} — these hold app ports OUTSIDE the bench range "
                f"(18000-19100, 8000-9999), which is why the port-range reaper never saw them")

    # run_build names its outputs after the ENTRANT, so two arms at the same node count and rep
    # would overwrite each other's tree AND vendor trace. Re-home both under the unit.
    src = OUT / f"{entrant}-r{rep}"
    dst = unit_dir(arm["name"], nodes, rep)
    if src.exists():
        if dst.exists():
            # ⚠ THIS WAS `shutil.rmtree(dst)` AND IT WAS DESTROYING THE CORPUS (F694/F695).
            #
            # A cell directory is REUSED every time its unit re-runs — after a rebuild, after a
            # void, after a fleet outage. Deleting the old one did not merely drop a row: it
            # ERASED A COMPLETED, SCORED RUN AND ITS EVENT LOG. loop.log records 19 cells with
            # more than one `[done]` line; `baseline-n3-r0` alone has SEVENTEEN, of which only the
            # last survives on disk. Worse, the fleet-outage phantoms re-ran real cells and their
            # 0.0 rows deleted genuine results — `sink_review-n3-r0` went 0.7326 then 0.0, and
            # two real `think_off` runs (0.4428, 0.9143) were erased the same way.
            #
            # That is why the result corpus read as "every lever arm is phantom" and why
            # "the instruments do not undercount" was wrong: BOTH SIDES OF THAT COMPARISON WERE
            # READING A SURVIVORSHIP SNAPSHOT. 25 of 53 real runs on disk are missing from it.
            #
            # Archiving instead of deleting is the whole fix. Disk is cheap; a scored run on a
            # binary that no longer exists cannot be re-made at any price.
            keep = dst.parent / f"_superseded/{dst.name}@{engine_build_at_dispatch}"
            keep.parent.mkdir(parents=True, exist_ok=True)
            if keep.exists():
                keep = keep.with_name(f"{keep.name}-{int(time.time())}")
            shutil.move(str(dst), str(keep))
            log(f"[archive] {now()} {dst.name} already existed — moved to {keep} rather than "
                f"deleted; a re-run must never erase the run it replaces")
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

    # F812: which identifiers did LM STUDIO see active during this unit's window?
    lms_ids: set = set()
    try:
        if _fleet_tsv.is_file():
            with open(_fleet_tsv) as fh:
                fh.seek(_fleet_off)
                for line in fh:
                    if "IDENT=STATUS" not in line:
                        continue
                    for tok in line.split("IDENT=STATUS", 1)[1].split():
                        name, _, st = tok.partition("=")
                        if st in ("GENERATING", "PROCESSINGPROMPT"):
                            lms_ids.add(name)
        verdict["lms_observed_ids"] = sorted(lms_ids)
        verdict["lms_observed_nodes"] = len(lms_ids)
        verdict["lms_node_mismatch"] = len(lms_ids) > nodes
        if len(lms_ids) > nodes:
            log(f"[LMS-MISMATCH] {unit_name(arm['name'], nodes, rep)} intended {nodes} node(s) "
                f"but LM Studio saw {len(lms_ids)} active: {sorted(lms_ids)} — either goose "
                f"dispatched beyond its pool or something else used the fleet; this row is "
                f"FLAGGED and the curve reporter must exclude it")
    except Exception as exc:  # noqa: BLE001 — a broken oracle must be visible, not fatal
        verdict["lms_observed_nodes"] = None
        verdict["lms_observe_error"] = f"{type(exc).__name__}: {exc}"
    actual = verdict.get("actual_nodes")
    # The label is an intention; run_started.pool is the fact. A mismatch has silently voided a
    # whole campaign before, so it voids the unit here rather than being averaged in.
    #
    # ⚠ THE `is not None` EXEMPTION COST 104 OF 133 ROWS (F664). It guarded a MISMATCH and let
    # MISSING through: a run that never reported a pool at all — the most broken outcome there is —
    # got `actual = None`, failed `actual is not None`, and was recorded as a VALID result. Its score
    # is 0.0 because the scorer graded an empty directory, so 78% of the corpus read as genuine
    # zeroes. EVERY lever arm was 100% phantom and the whole lever campaign measured nothing.
    # `None != nodes` is already True, so dropping the exemption is the entire fix.
    #
    # The harness clause closes the second half. selftest.py CAUGHT all 104 at the time
    # (`harness_ok: false`, "invariant pass CRASHED: no run log") and nothing was ever wired from its
    # verdict to this flag — the instrument was right and unheard. Verified safe before landing: of
    # 118 rows, 0 would be voided by the harness clause alone and 0 surviving rows carry
    # `harness_ok is False`, so this voids nothing that is currently counted as evidence.
    harness_failed = harness["ok"] is False
    void = unit_is_void(actual, nodes, harness["ok"], harness.get("detail", ""))

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
        # F811/F812: the resume mark + the LM Studio node oracle — set on `verdict` in run_unit
        # and LOST here until this line existed (the result dict cherry-picks; measured: the
        # first resumed unit persisted neither field).
        "resumed_from": verdict.get("resumed_from"),
        "lms_observed_ids": verdict.get("lms_observed_ids"),
        "lms_observed_nodes": verdict.get("lms_observed_nodes"),
        "lms_node_mismatch": verdict.get("lms_node_mismatch"),
        # The engine's own exit and stderr tail. Kept because the retry loop can only see an
        # EXCEPTION, and a fleet-down is not one — the engine refuses cleanly, run_unit returns a
        # perfectly-formed verdict scoring 0.0, and the loop breaks having "succeeded" (F666).
        "engine_exit": (verdict.get("agent") or {}).get("exit"),
        "engine_tail": (verdict.get("agent") or {}).get("tail"),
        "actual_pool": verdict.get("actual_pool"),
        "actual_nodes": actual,
        "void": void,
        "void_reason": (None if not void else "; ".join(filter(None, [
            (f"NEVER REPORTED A POOL (actual_nodes is None) — the run produced no run log, so its "
             f"score grades an empty tree and is a MISSING measurement, not a bad build"
             if actual is None else
             f"asked for {nodes} nodes, engine built {actual}"),
            ("harness self-test FAILED — this unit's own instruments did not pass their controls, "
             "so its numbers are not evidence" if harness_failed else None),
        ]))),
        "scorer_version": verdict.get("scorer_version"),
        "engine_build": engine_build_at_dispatch,
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


def curve_first(units: list[tuple], full: int) -> list[tuple]:
    """Put the node-curve units at the head, ordered so a MATCHED PAIR closes as early as possible.

    A PURE FUNCTION OF WHAT IS STILL INCOMPLETE, and that is the whole point. `main()` recomputes
    `backlog()` on every iteration and always takes `todo[0]`, so any ordering that depends on the
    list being consumed in sequence is silently defeated: the previous `zip_longest(n3, n1)` put
    `n1-r0` at index 1, and after each n3 unit finished the recomputed backlog zipped a fresh n3 rep
    into index 0 and pushed `n1-r0` back to index 1. MEASURED: four consecutive n3 cells ran while
    the log printed "NEXT: baseline-n1-r0" every single time.

    Sorting by (rep, then 3-nodes-before-1-node) gives the same sequence whether the list is consumed
    once or recomputed at every step, because it is a function of the remaining set alone.
    """
    curve = [u for u in units if u[0]["name"] == "baseline" and u[1] in (full, 1)]
    if not curve:
        return units
    picked = {id(u) for u in curve}
    curve.sort(key=lambda u: (u[2], -u[1]))
    return curve + [u for u in units if id(u) not in picked]


def curve_order_self_test(full: int = 3, reps: int = 5) -> None:
    """The property that matters is STABILITY UNDER RECOMPUTATION — so simulate exactly that.

    Controls both ways (L96/L123): the new rule must close a pair on unit 2, and the OLD zip_longest
    rule must FAIL the same simulation. A test the previous implementation also passes proves nothing.
    """
    arm = {"name": "baseline"}
    other = ({"name": "kind_prompt"}, full, 0)

    def sim(order_fn) -> list[str]:
        done: set[tuple[int, int]] = set()
        seq = []
        for _ in range(reps * 2):
            remaining = [(arm, n, r) for r in range(reps) for n in (full, 1)
                         if (n, r) not in done] + [other]
            todo = order_fn(remaining, full)
            if not todo:
                break
            a, n, r = todo[0]
            if a["name"] != "baseline":
                break
            done.add((n, r))
            seq.append(f"n{n}-r{r}")
        return seq

    def old_rule(units, full_):
        n3 = [u for u in units if u[0]["name"] == "baseline" and u[1] == full_]
        n1 = [u for u in units if u[0]["name"] == "baseline" and u[1] == 1]
        if not (n3 and n1):
            return units
        picked = {id(u) for u in n3} | {id(u) for u in n1}
        paired = [u for pair in itertools.zip_longest(n3, n1) for u in pair if u is not None]
        return paired + [u for u in units if id(u) not in picked]

    def first_pair_at(seq: list[str]) -> int:
        seen = set()
        for i, u in enumerate(seq, 1):
            seen.add(u)
            rep = u.split("-")[1]
            if f"n{full}-{rep}" in seen and f"n1-{rep}" in seen:
                return i
        return 10 ** 6

    new_seq, old_seq = sim(curve_first), sim(old_rule)
    assert first_pair_at(new_seq) == 2, f"a pair must close on unit 2, got {new_seq}"
    assert first_pair_at(old_seq) > 2, (
        f"the OLD rule must FAIL this simulation or the test proves nothing — got {old_seq}")
    assert len(set(new_seq)) == len(new_seq), f"no unit may be scheduled twice: {new_seq}"
    assert len(new_seq) == reps * 2, f"every curve unit must still run: {new_seq}"
    assert curve_first([other], full) == [other], "a backlog with no curve units must pass through"
    assert curve_first([], full) == [], "an empty backlog must stay empty, never invent work"
    print(f"curve_order self-test OK — new closes a pair at unit {first_pair_at(new_seq)}, "
          f"old at {first_pair_at(old_seq)}; sequence {new_seq}")


def backlog(target_reps: int) -> list[tuple[dict, int, int]]:
    """Units still owed, ordered so the cheapest decisive question comes first.

    A cell's OWN `reps` bounds it, not a global n. A mechanism readout — did `/v1` reach the plan,
    did the fan fire, did detail_fallback go to zero — is a fact about the code and is settled by one
    unit; only a SCORE comparison has to clear the 46-point replicate spread. `target_reps` still
    RAISES a cell above its own floor when the backlog drains, so a long night deepens the score
    cells instead of inventing new ones.
    """
    units = []
    for rep in range(max(target_reps, CURVE_REPS)):
        for c in cells():
            # A MECHANISM cell (reps == 1) is DONE after one unit — "did `/v1` reach the plan" does
            # not get truer with a second run, and re-running it is the "loop for the sake of it"
            # this design exists to stop. A SCORE cell grows with target_reps instead, so a long
            # night deepens the comparisons that have to clear the replicate spread rather than
            # inventing new questions nobody asked.
            # THE CURVE GETS ITS OWN REPLICATE TARGET, and only the curve.
            #
            # F327: the sign test's bar is a SAWTOOTH in n. Below 8 pairs a single crossing kills the
            # result outright (6-of-7 = 0.0625 > 0.05), so 6 and 7 pairs are a HARDER test than 5 —
            # more fleet time for a worse question. 8 is the first n that absorbs one loss, dropping
            # the required score gap from 0.148 to 0.110 against a replicate spread that is 31% of the
            # mean. Registered BLIND, with zero n1 cells on disk, so it cannot have been chosen after
            # seeing which way the pairs fell.
            #
            # Raising the GLOBAL `target_reps` to 8 would drag every score arm to 8 reps as well —
            # `cap` reads `max(c.reps, target_reps)` — which is a much larger blast radius than the
            # decision justifies. So the target is scoped to the two arms the curve actually compares.
            is_curve = c["arm"]["name"] == "baseline" and c["nodes"] in (NODE_LEVELS[0], 1)
            floor = CURVE_REPS if is_curve else target_reps
            # F838: the n1 arm runs OUTSIDE this sweep (parallel_n1.py, three pinned units at a
            # time) — with the flag set, this sweep never spends the whole fleet's window on a
            # one-node unit. curve.py reads the parallel rows as the n1 source.
            if (os.environ.get("BENCH_SKIP_N1")
                    and c["arm"]["name"] == "baseline" and c["nodes"] == 1):
                continue
            # ⛔ reps == 0 MEANS PARKED, AND IT DID NOT.
            #
            # MEASURED 2026-08-10: I parked `doc_fetch` at reps 0 after its falsifier fired, verified
            # by running backlog() — and it came back with FIVE units, still at position 0. The cap
            # expression read `1 if reps == 1 else max(reps, floor)`, so zero fell into the else and
            # became `max(0, target_reps)` = target_reps. Setting reps to 0 did not park the arm, it
            # gave it the FULL score budget: the exact opposite of what the number plainly means, and
            # of what the `detail_budget` comment in ARMS already claims it does.
            #
            # This is why the check is `cap <= 0 -> continue` rather than a guard at the call site:
            # every future reader will write reps 0 expecting parked, and they should be right.
            declared = c.get("reps", 1)
            if declared == 0:
                continue
            cap = 1 if declared == 1 else max(declared, floor)
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
    # ORDER THE TWO VERIFIED-DEFECT FLIPS FIRST. Both fix something MEASURED to be wrong rather than
    # testing a hypothesis, and both target the population that is 93% of all failures:
    #   think_off + dep_signatures : the prompt a test-author receives is half truncated dependency
    #                                source (3 of 4 blocks cut mid-token, one failing ast.parse), and
    #                                the model is handed an OPEN <think> tag it must write its way out of.
    #   kind_prompt                : the tailored test-author rule blocks EXIST in the engine and are
    #                                UNREACHABLE, so test-authors receive the IMPLEMENTER rules
    #                                "NEVER read the project's OTHER TEST files" and "STOP WHEN GREEN"
    #                                — instructions for a job that SATISFIES tests handed to one that
    #                                AUTHORS them (F216, and commit d15ed448e measured the same class).
    # Everything after these is a question; these two are defects with a switch already written.
    # INTERLEAVE THE NODE CURVE WITH ITS OWN DENOMINATOR — goal ONE is the session-resolving question
    # and it was queued behind seventeen mechanism arms.
    #
    # THE MEASUREMENT THAT FORCED THIS. On the 10 tasks completed by BOTH `swarm-1node-r0` and
    # `think_off-n3-r0`, the SAME task took a median 0.75x as long with three nodes — not merely more
    # tasks at once, but each task 25% faster, because one node at PARALLEL 2 runs two workers against
    # one GPU while three nodes give each worker most of a machine. Plan size (16 vs 17 tasks), scout
    # lenses (3) and findings (2) were identical, so this is not the 3-node run doing less work. That
    # is the first quantitative signal for goal one in the whole campaign — and it is CONFOUNDED (two
    # different arms, n=1 each, and the 1-node unit was killed at 81 min so its task set is
    # selection-biased). A confounded signal on the session's own question earns a clean experiment,
    # not a conclusion.
    #
    # Ordering matters more than it looks. Three n=3 replicates followed by three n=1 replicates yields
    # NOTHING comparable until unit six, roughly twelve hours in, and a fleet outage anywhere in that
    # window loses the lot. Interleaved, a MATCHED PAIR exists after every two units and each further
    # pair tightens the interval. The same units run; the answer just stops being all-or-nothing.
    #
    # n=2 stays where it was: it shapes the curve, it does not answer "does 3 beat 1".
    # THE INTERLEAVE MUST SURVIVE RECOMPUTATION, AND `zip_longest` DID NOT.
    #
    # `main()` recomputes `backlog()` every iteration and always takes `todo[0]`. Zipping the two
    # LISTS produced [n3_next, n1_r0, n3_..., ...], so `n1_r0` sat at index 1 — and after the n3 unit
    # finished, the recomputed backlog zipped a fresh n3 rep into index 0 and put n1_r0 back at index
    # 1. MEASURED: four consecutive n3 cells ran while the log printed "NEXT: baseline-n1-r0" every
    # single time. The 1-node arm was starved by a scheduler that never advanced past its own head,
    # and the comment above promising "a MATCHED PAIR exists after every two units" was describing an
    # intent the code defeated.
    #
    # Ordering by (rep, then 3-nodes-before-1-node) is a PURE FUNCTION OF WHAT IS STILL INCOMPLETE, so
    # it gives the same sequence whether the list is consumed once or recomputed at every step. Once
    # n3-r0 is done, n1-r0 is the lowest key remaining and becomes the head — the pair closes on the
    # very next unit instead of after the whole n3 arm.
    units = curve_first(units, full)

    # GOAL ONE GOES FIRST, NOW THAT THE MINI-GOAL IT WAS QUEUED BEHIND HAS RESOLVED.
    #
    # `kind_prompt` and `think_off` were hoisted here because they were the only arms pointed at the
    # test-author failure row. That row is CLOSED — F252: 11 completions, 0 failures, p = 0.017 — and
    # both levers are now DEFAULT ON and verified on the wire, so those arms would re-measure a
    # question already answered while the session-resolving one waited four more hours behind them.
    #
    # The node curve is Mihai's actual goal: does a 3-node run beat a 1-node run on wall-clock AND on
    # shipped quality. It is also the only thing that supplies the contemporaneous control F252 is
    # missing — that result is a before/after across builds, and the n=3 baseline cells are what turn
    # it into a comparison rather than a coincidence.
    curve = [u for u in units if u[0]["name"] == "baseline" and u[1] in (full, 1)]
    if curve:
        picked = {id(u) for u in curve}
        units = curve + [u for u in units if id(u) not in picked]

    # THE DOC WIRE RUNS BEFORE THE CURVE — ONE UNIT EACH, MECHANISM ONLY.
    #
    # This is the same argument the `think_off` hoist above makes, applied to a bigger question, and
    # it is here because the queue had buried the arm its own gate text calls the top arm: "the
    # single largest block of score on the board… nothing else examined today is within an order of
    # magnitude of that." It sat at position 16.
    #
    # THE CURVE IS NOT THE RIGHT THING TO WAIT FOR HERE. `CURVE_REPS = 8` means 16 curve units, and
    # 14 are still owed — roughly 24-30 fleet-hours before any treatment runs at all. The curve
    # answers "does 3 beat 1", which genuinely needs 8 matched pairs. It is NOT the denominator these
    # two arms need, because their PRIMARY readouts are deterministic mechanism events that no
    # denominator improves:
    #   doc_fetch       `doc_fetched{ok:true,status:200,bytes:4789}` — zero in all 54 archived logs,
    #                   so any non-zero is unambiguous; plus vendor_cursor_paging and vendor_all_pages
    #                   reaching 1.00 on a cell that is not r3.
    #   scout_doc_urls  the literal "The spec names these documents" reaching a scout prompt — present
    #                   in the binary, never executed, because the gate is default OFF.
    #
    # Both wires have NEVER carried a byte: `doc_facts` is empty in every archived run and there are
    # zero `doc_fetched` events across 54 logs. "Does the wire connect" cannot be answered by more
    # baselines, and if it does not connect, every score comparison built on top of it is moot.
    #
    # SCOPED TO ONE UNIT EACH, DELIBERATELY. Only rep 0 is hoisted; doc_fetch's remaining score
    # replicates stay behind the curve where they belong, because THOSE do need the denominator. So
    # this costs the curve exactly two cells and buys the answer to the campaign's largest open
    # question about 24 hours earlier. Self-limiting: once both rep-0 units complete this partition
    # is empty and the ordering is exactly what it was before.
    # `probe_post` joins this partition for the same reason and on the same terms. Its primary
    # readout is `probed_post >= 1` in the run log — zero in every cell ever recorded — which is a
    # deterministic fact about whether the gate can SEE the requirement at all. No number of baseline
    # replicates makes that question easier, and if the gate cannot see it, every score comparison
    # built on top of the conditional-request family is moot.
    wire = [
        u
        for u in units
        if u[0]["name"] in ("doc_fetch", "scout_doc_urls", "probe_post") and u[2] == 0
    ]
    if wire:
        picked = {id(u) for u in wire}
        units = wire + [u for u in units if id(u) not in picked]
    # F794: NEVER-OBSERVED MECHANISMS FIRST — ahead even of the wire partition.
    #
    # MEASURED starvation: every rebuild invalidates completeness (by design — a row must come
    # from the current engine), and the wire hoist then re-runs probe_post/scout_doc_urls rep-0
    # FIRST after every boundary. Under an active build session (7 boundaries in one night) the
    # queue never advanced past them: probe_post accumulated 12 rows while judge_nudge/fix_sched/
    # testgen/fill_fan/aux_slim — mechanisms that have NEVER BEEN OBSERVED AT ALL — sat queued and
    # unreached the whole time. The wire partition's own rationale ("cheapest decisive experiment
    # first") ranks a never-observed mechanism ABOVE a re-verification: probe_post's mechanism has
    # 12 historical observations, judge_nudge has zero. Rep 0 only, self-limiting exactly like its
    # siblings: once each lands one current-engine unit this partition is empty.
    never_seen = [
        u
        for u in units
        if u[0]["name"] in ("judge_nudge", "fix_sched", "testgen", "fill_fan", "aux_slim")
        and u[2] == 0
    ]
    if never_seen:
        picked = {id(u) for u in never_seen}
        units = never_seen + [u for u in units if id(u) not in picked]
    # CURVE_FIRST HAS THE FINAL WORD (F828). At the sb-5 regime flip every row in the corpus
    # went incomplete at once (scorer-version gate), which made the never_seen partition above
    # swallow the whole inventory and put a mechanism single at the head of a queue whose plan
    # of record says the 16-run curve leads. Re-applying the curve hoist AFTER every other
    # partition preserves each hoist's intent among the non-curve units while making the
    # decisive question structurally un-jumpable.
    units = curve_first(units, full)
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
    if (HERE / "REGIME.env").is_file():
        log("[regime] " + "; ".join(
            l for l in (HERE / "REGIME.env").read_text().splitlines()
            if l.strip() and not l.strip().startswith("#")))
    # SINGLE-INSTANCE LOCK (F817). Two supervisors over one results dir is not a race, it is a
    # shredder: each treats the other's engine as a stray orphan and kills it every ~10 s, every
    # kill scores a dead or seed tree as a real [done] row, the phantom completions drain the
    # backlog, and the never-end-on-a-counter rule then re-queues the whole campaign at rep+1.
    # A second instance must REFUSE, not duel. The fd is held for the process lifetime.
    global _LOCK_FH
    _LOCK_FH = open(OUT / "sweep.lock", "a+")
    try:
        fcntl.flock(_LOCK_FH, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        _LOCK_FH.seek(0)
        sys.stderr.write(
            f"REFUSING to start: another sweep supervisor already holds {OUT / 'sweep.lock'} "
            f"(pid file says {_LOCK_FH.read().strip() or 'unknown'}). "
            f"There is never a reason to run two.\n")
        return 2
    _LOCK_FH.seek(0)
    _LOCK_FH.truncate()
    _LOCK_FH.write(str(os.getpid()))
    _LOCK_FH.flush()
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
        # Same reason as the dispatch-time stamp in run_unit: a crashed unit is recorded with
        # `failed: True`, and `is_done()` treats that as COMPLETE. So if a rebuild landed between
        # the attempt and the record, the failure would carry the NEW binary's id and be skipped
        # forever against a binary it never ran on.
        build_at_attempt = engine_build()
        result = None
        tail = ""
        for attempt in range(MAX_ATTEMPTS):
            try:
                result = run_unit(arm, nodes, rep, port)
                port += 1
                # A FLEET-DOWN IS NOT AN EXCEPTION, WHICH IS WHY THE RETRY BELOW NEVER FIRED (F666).
                # The engine refuses cleanly ("No models are loaded on the fleet"), exits 1 in 0.1s,
                # and run_unit returns a well-formed verdict whose score is 0.0 because the scorer
                # graded an empty tree. So this loop `break`s having apparently succeeded, and a
                # transient outage is written to disk as that arm's answer. At a tenth of a second
                # per unit the whole backlog is consumed in minutes: 104 of 133 rows, every lever
                # arm, all of it fabricated. A unit that never reported a pool is NOT a result.
                if result.get("actual_nodes") is None and looks_transient(result.get("engine_tail")):
                    tail = f"engine refused (exit {result.get('engine_exit')}): {result.get('engine_tail')}"
                    result = None
                    log(f"[fail] {now()} {label} attempt {attempt}: FLEET UNAVAILABLE — {tail[:300]}")
                    if attempt < MAX_ATTEMPTS - 1:
                        wait_for_fleet()
                        continue
                    break
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
                      "engine_build": build_at_attempt, "audit": {}}

        unit_secs = time.time() - started
        # F784: a unit whose engine was killed by a boundary STOP is a KILL ARTIFACT, not a datum.
        # The wrapper used to score the half-born tree (0.0225/4min, 0.0394/3min made the ledger,
        # void=False) and any naive mean read them as catastrophic n3 scores. If the STOP sentinel
        # exists when the unit ends, the run was cut by the operator boundary — void it. The score
        # is preserved under kill_artifact_score for forensics, never under score.
        if (HERE / "STOP").exists() and result.get("score") is not None:
            result["kill_artifact_score"] = result.pop("score")
            result["score"] = None
            result["void"] = True
            result["void_reason"] = "boundary STOP killed the engine mid-unit (F784)"
        # F830 — THE FOURTH KILL-ARTIFACT INSTANCE, from the one direction F784 couldn't see: an
        # engine killed by something that leaves NO STOP file (measured 2026-08-15: two -9s,
        # most plausibly macOS memory pressure during a concurrent harness burst) scored its
        # half-built tree as a REAL row — 0.454 and 0.045 landed void=False in the curve. The
        # engine's own exit code was in the row the whole time. A killed engine is never a
        # measurement, whoever sent the signal.
        _exit = result.get("engine_exit")
        if (isinstance(_exit, int) and _exit < 0 and not result.get("void")
                and result.get("score") is not None):
            result["kill_artifact_score"] = result.pop("score")
            result["score"] = None
            result["void"] = True
            result["void_reason"] = (f"engine killed (exit {_exit}) — kill artifact, not a "
                                     f"measurement (F830)")
        if is_real_unit({**result, "wall_secs": result.get("wall_secs") or unit_secs}):
            durations.append(unit_secs)
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
            f"({round(unit_secs / 60)} min)")

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
    # NO ARGUMENTS, EVER (F817). This file IS the supervisor — running it starts a sweep. There
    # is no read-only subcommand here; `sweep.py backlog` once launched a second supervisor that
    # duelled the live one for 24 minutes. A wrong invocation must refuse, not run.
    if len(sys.argv) > 1:
        sys.stderr.write(
            f"sweep.py takes NO arguments (got {sys.argv[1:]}) — running it STARTS a sweep "
            f"supervisor. For a read-only view use `loop.sh check` or read "
            f"runs/nodeloop/loop.log.\n")
        sys.exit(2)
    sys.exit(main())
