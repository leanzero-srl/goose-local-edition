# THE GOAL — read this FIRST on every wake, before anything else

**Overarching:** make the swarm actually WORTH IT — beat one node on time and on the quality of what
ships. Everything below is a sub-goal of that and nothing else belongs here.

**The through-line Mihai keeps returning to:** the crux across all phases is EXACT AND PRECISE
INSTRUCTIONS. Every fix should be checkable against that.

---

## Rules for the tick (these exist because I broke them)

1. **Read this file, then `FINDINGS.md`'s last entry, then `git log --oneline -8`.** Never recall.
   Context gets compacted; the repo does not.
2. **A tick either CONTINUES the current goal or CLOSES it and opens the next.** It never drifts.
   Write the outcome here before the tick ends.
3. **A fleet unit must be answering a named question written in CURRENT GOAL below.** If it is not,
   kill it (supervisor BEFORE engine — `./loop.sh boundary`), batch the held fixes, cross, restart.
   Keeping a fleet warm for its own sake is waste and Mihai has said so repeatedly.
4. **Never end a turn without doing work.** Reporting is not the work.
5. Every finding lands in `FINDINGS.md` with a number, and every change is committed as it is made.

---

## CURRENT GOAL — G1: do the ten shipped fixes actually change what a run produces?

**Opened 2026-08-02 09:35 local. engine_build 1785652162-235742240.**

The binary just crossed a boundary carrying ten commits. Every one was justified from evidence, and
**not one has been observed working end to end.** "I fixed that" is a hypothesis until a deterministic
event says otherwise. One baseline@3n unit tests all of these at once, which is why it runs first.

### Registered predictions — write the outcome next to each

| # | Prediction | Where to read it | Outcome |
|---|---|---|---|
| P1 | `log_message` appears in NO review finding | `complete_verify.finding_texts` | |
| P2 | `spec_contract` STOPS saying "CHECKED NOTHING" and probes real endpoints | `spec_contract.verified > 0` | |
| P3 | `grounded > 0` — a scout's curl now counts as a lookup | `research_completed.grounded` | **CONFIRMED** grounded=2, looked_nothing_up=0 (was 0 / 2) |
| P4 | `/v1` reaches task descriptions verbatim | `plan_loaded.tasks[].description` | |
| P5 | the repair tail has an occupancy number for the first time | `complete_fix_dispatched` present | |
| P6 | a pytest finding ends in a traceback, not a list of collected tests | `complete_verify.finding_texts` | |
| P7 | replan can fire a SECOND time after an empty answer | `replanned` count > 1 | |

| P9 | prompt chars drop ~49% once F87 ships | `llm_request.*.jsonl` | **CONFIRMED, exceeded: -68% total, -94% system, 0 of 15 carry hints** |

**P2 carries a standing caution:** `spec_contract` is two-for-two on phantoms. If it produces a
FINDING, verify against `crunch.py` before believing it.

### Done this session (do not redo)

F74 phases.py — 37-46% of a run had no occupancy number; the tail emits zero dispatches.
F75 the repair tail has NEVER gone green: `passed` false 13/13, findings ROSE in 3.
F76 judge starved of idle slots (no_idle_device 80-94%) but doing REAL work — 4 of 32 caught defects.
F77 scouts DO report `/v1` verbatim; the loss is downstream.
F78 grounding was `is_mcp && ok` and this bench has NO MCP tools => the verbatim channel was dead.
F79 two `--help` parsers disagreeing; a single-subcommand app advertised nothing.
F80 90-log adversarial sweep: caught my own regression; refuted its `fan_verify` claim; tail fan
    width is 1.19 nodes, so the ATTEMPT is the only axis that decomposes.
F81 replan disabled for a whole run by ONE empty answer, and missing from the 15s tick list.
F82 the `log_message` phantom source, confirmed with controls in both directions. Phantom 45% -> 5%.
F83 `finding_texts` truncation kept "what was checked" and dropped "what went wrong".

---

## NEXT GOALS, in order — each becomes CURRENT when the one above closes

**G2 — arm `spec_repair` (sweep arm #3).** The tail is 13-26% of every run, has never gone green, and
racing one verified attempt per node is the only mechanism found that uses three nodes on a
one-finding round. Two independent readouts: MECHANISM (`spec_repair_wave.twins > 1`) and SAFETY
(`winner_findings` NEVER above `baseline_findings`). A round that promotes NOTHING passes the safety
readout — it is not a failure.

**G3 — arm `kind_prompt`.** 69.7% of dispatches get rules written for another job. The lever is built
and OFF. Readout: `kind_mismatch_pct` from `dispatch_audit.py` falls toward zero.

**G4 — the node curve at 1 / 2 / 3.** This is goal one stated directly and it needs n>=3 per cell,
because an identical config has been measured with a 46-point spread.

**G5 — bring the repair tail under the judge.** Mihai's design intent was that idle nodes take the
judge role so hard-coded timings become unnecessary. The fix worker is NOT a scheduler task, emits no
`task_dispatched`, and is invisible to `pick_judge_target` — so its only bound is a 1200s wall-clock
literal, observed live burning 20 minutes with three idle nodes. Also: that 1200 exists TWICE, once as
`fix_cap_secs()` (configurable, clamped) and once as a bare `from_secs(1200)` at swarm.rs:23596 — the
two-versions-of-one-rule defect again.

**G6 — stop the swarm leaking app servers.** A worker starts the built app to exercise it and never
stops it; an orphan held port 8931 for 82 minutes after its run was killed, failed the next unit with
`Errno 48`, and is the confirmed cause of the pytest-collect mystery (F88). Two fixes, and they are
not alternatives: REAP the process (the worker that starts a server owns stopping it, and the harness
should sweep survivors between units), and give `interpret_pytest_collect` the `Inconclusive` variant
its sibling `interpret_pytest_run` already has — now justified by evidence rather than by a hunch.

---

## DISK POLICY — decided 2026-08-02, do not re-derive

`MIN_FREE_GB = 15` is a hard abort in the sweep's watchdog, so disk is a real unattended risk. The
consumer is `target/debug`, which repeated `cargo check`/`test` grows without bound (measured 64 GB).

**Delete `target/debug` only, and it is safe at any time** — the sweep executes
`target/release/goose`, and debug is regenerable.

**NEVER run `goose-clean --go` while a run is live.** It removes every `target/`, which INCLUDES the
release binary the engine is currently executing. The skill's own guard already refuses while
`goose swarm` is running; do not override it with FORCE.

**Do not run the full clean at a boundary either**, tempting as the idle window is. The boundary
rebuild is incremental (~3.5 min); after a full clean it is a cold release build, and that is fleet
time spent reclaiming space that was not scarce.

---

## CLOSED — the pytest-collect mystery had a cause, and it was not the one I was leaning toward

RESOLVED by F88. A tree failed `pytest --collect-only` at 08:58 and passed at 09:20 unmodified because
an ORPHANED APP SERVER a swarm worker had left running held the port a test needed. An environment
collision, F67's class. Holding out for evidence was correct: the tempting fix (add `Inconclusive` to
`interpret_pytest_collect`) is now justified, but it treats the symptom — the defect is the leak, and
that is G6.
