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
6. **RUN `python3 review.py` BEFORE FINALISING ANY TICK.** It asks Mihai's two questions in his order
   — **DOES THE PLAN MAKE SENSE?** then **IS THE PLAN BEING FOLLOWED?** — against four levels: the
   logs, the plan, the current mini-goal, and the overarching goal. The order is load-bearing: a
   faithfully-executed bad plan is still a bad run, so checking execution first would flatter it.
   It ends in CONTINUE or INTERVENE. **A tick that ends without acting on that verdict is the idling
   Mihai called out.**
7. **Rinse and repeat, and the repeat ENDS when the mini-goal is achieved and a piece of the
   overarching goal is fulfilled.** Not when the run finishes, not when a number looks good.
5. Every finding lands in `FINDINGS.md` with a number, and every change is committed as it is made.

---

## G1 — CLOSED 2026-08-02. Verdict F107, **headline RETRACTED by F112**. 8 of 9 predictions settled.

⚠ **DO NOT REPEAT F107's HEADLINE.** "MAX USEFUL NODES = 1.92, the plan is the ceiling, api-web = 49.5%
of node-busy" was built on a phantom span: a task SUPERSEDED BY A SPLIT never completes, and
occupancy.py credited it to `t_end` — 9.1x its real 651s. Corrected: occupancy **0.4289**, biggest task
**integrate-verify at 29%**, solo time **1590s ALL of it the sink**, **MAX USEFUL NODES = 3.28** against
a pool of 3. **The plan is NOT the bottleneck. The SINK is.**

## CURRENT GOAL — G8: SHORTEN THE SINK. It is 29% of node-busy and 100% of the solo time.

`integrate-verify` held one node for **1590s — 23.6% of the whole run** — while the other two idled.
That is now the single largest serial region, and the plan around it is fine (`MAX USEFUL NODES` 3.28
> pool 3). `fan_verify` already shards per-module verification into `verify::<M>`, and those ran; what
remains is the single JOIN. Read what integrate-verify is actually INSTRUCTED to do before proposing
anything — F101's lesson is that a verdict can be right while its reason is wrong.

## NEXT — G2: arm `spec_repair` and measure the repair tail (blocked until the boundary ships F106)

**Opened 2026-08-02 09:35 local. engine_build 1785652162-235742240.**

The binary just crossed a boundary carrying ten commits. Every one was justified from evidence, and
**not one has been observed working end to end.** "I fixed that" is a hypothesis until a deterministic
event says otherwise. One baseline@3n unit tests all of these at once, which is why it runs first.

### Registered predictions — write the outcome next to each

| # | Prediction | Where to read it | Outcome |
|---|---|---|---|
| P1 | `log_message` appears in NO review finding | `complete_verify.finding_texts` | **CONFIRMED** — absent from all 3 findings |
| P2 | `spec_contract` STOPS saying "CHECKED NOTHING" and probes real endpoints | `spec_contract.verified > 0` | **MECHANICALLY YES, VERDICT WRONG** — it probed the VENDOR MOCK and produced 2 phantom 404s (F104) |
| P3 | `grounded > 0` — a scout's curl now counts as a lookup | `research_completed.grounded` | **CONFIRMED** grounded=2, looked_nothing_up=0 (was 0 / 2) |
| P4 | `/v1` reaches task descriptions verbatim | `plan_loaded.tasks[].description` | **CONFIRMED** 2 of 16 (`meridian`, `test-meridian`) — was ZERO |
| P5 | the repair tail has an occupancy number for the first time | `complete_fix_dispatched` present | **FALSIFIED — 0.** I put the events inside the `spec_repair` branch, which is default-OFF (F105) |
| P6 | a pytest finding ends in a traceback, not a list of collected tests | `complete_verify.finding_texts` | **CONFIRMED** — ends in FAILED test ids, with `[middle elided]` |
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

**G7 — DE-HARDCODE THE SWARM. Filed by Mihai 2026-08-02, and it outranks tuning.**

*"this agent will be used to produce script, software, apps etc"* — so logic that only works for one
stack is a ceiling on what the swarm can ever be, not a rough edge.

MEASURED in swarm.rs, literal occurrences:

| token | count | | token | count |
|---|---|---|---|---|
| `.py` | **366** | | `npm` | 25 |
| `pytest` | **103** | | `argparse` | 25 |
| `python3` | **74** | | `cargo` | 18 |
| `__main__.py` | **40** | | `go build` | 9 |

The engine is roughly **5-10x more Python-aware than anything else**, and `TargetLang` carries an
`Other` variant that in practice means "no gate knows how to verify this". Every deterministic check
this loop has been fixing — the smoke gate, the entry probe, the AST review, the collect/run
interpreters, the subcommand parser — is Python-shaped.

THE FIX IS ARCHITECTURAL, NOT A SED. One capability table per language, answering: how to BUILD, how
to TEST, how to find the ENTRY POINT, how to PROBE it, how to review it STATICALLY. Every gate then
reads the table instead of embedding `pytest`. Fragments already exist (`TargetLang`, `smoke_go`,
`verify_commands`) — the defect is that they are scattered, so adding a language means finding every
site rather than filling in a row.

Do NOT attempt 366 call sites in one pass. Take it a gate at a time, each with its own controls, and
never mid-run.

---

## DISK POLICY — decided 2026-08-02, do not re-derive

`MIN_FREE_GB = 15` is a hard abort in the sweep's watchdog, so disk is a real unattended risk. The
consumer is `target/debug`, which repeated `cargo check`/`test` grows without bound (measured 64 GB).

**FIRST check for a stale local snapshot.** `tmutil listlocalsnapshots /`. With one present, deleting
build cache FREES NOTHING — the blocks move into the snapshot and free space FALLS. Measured: one
snapshot held ~138 GB and reclaiming it took free space 27 -> 165 GB. The signature is `df` and `du`
disagreeing by tens of gigabytes while nothing is growing. Reclaim with
`tmutil thinlocalsnapshots / <bytes> 1` — the sanctioned API, gentlest urgency, not a named delete.

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
