# THE GOAL — read this FIRST on every wake, before anything else

**Overarching:** make the swarm actually WORTH IT — beat one node on time and on the quality of what
ships. Everything below is a sub-goal of that and nothing else belongs here.

## ⭐ THE PRIME DIRECTIVES — filed by Mihai 2026-08-02 19:40. These OUTRANK every finding below.

**1. "IF IT APPEARS LIKE IT'S NOT WORTH IT THEN YOU ARE NOT DOING IT RIGHT."** More nodes CANNOT be
worse than one — that is redundancy in brain power. Harder to make work, yes; worse, never. A result
that says otherwise is a statement about MY implementation, not about the swarm. There is no
"the data says it doesn't help" ending. Keep researching until it works.

**2. NOTHING GENERIC. EVER. A generic prompt to a node IS the failure.** Not a rough edge — the
failure itself. Every instruction a node receives must be CONSTRUCTED ON THE FLY for that node, that
task, that moment, from principle and goal — never a canned string, never a static block, never a
rule written for a different job.
*Measured today, and this is the indictment: **58 of 73 judge interventions are THREE canned
strings.** The same sentence, forty times, to forty workers doing forty different jobs. And a split
child receives `(split of <parent>) <child-id>` — 30 characters, identical in shape every time.*

**3. THE IDLE NODE IS THE POINT.** When two workers are busy and a third finishes, that third node
should ASSESS the work in flight. That is where the redundancy pays. It needs a DYNAMIC prompt built
for what it is looking at, and ONLY the right amount of context — enough to judge, not so much that
the judge itself bogs down.

**4. THE SWARM'S OWN LOGIC MUST BE PRINCIPLE, NOT HARD-CODE.** Fan out, spread, divide and conquer —
expressed as goals the engine pursues, not as constants and canned text. Every hard-coded second,
threshold and fixed sentence is a place the engine stopped thinking.

**5. DON'T WAIT — ASSESS AND BUILD, EVERY TICK.** Between ticks: watch the fleet, form conclusions,
construct and implement. Waiting for a run to finish is not work. If there is nothing to measure,
there is always something to construct.

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

3b. **NEVER LET FIXES SIMMER.** Filed by Mihai 2026-08-02 after 51 engine commits sat unshipped for
   most of a day. Batching is a rule against crossing for ONE fix; it is NOT a licence to accumulate.
   **`./loop.sh check` now COUNTS held commits every tick** — at 8+ it says CROSS THE BOUNDARY, and
   that is not advisory. A killed unit costs ~2h of fleet time; an unshipped batch costs every
   experiment blocked on it AND every hour the engine runs without fixes that are already written.
   Crossing is cheap and pre-flighted (`preflight.py` decides from source and refuses without killing
   anything). **If a blocked arm or a registered prediction is waiting on the crossing, cross NOW.**

3c. **RUN CONTINUOUS OUTSIDE RESEARCH — do not only introspect.** Also filed by Mihai 2026-08-02:
   *"implement a continuous investigation and research from other agents how they're doing this
   better, like opencode, this fork's upper parent and see if they implemented something new since we
   forked ... investigate using agents giving them limited context and let them find out with fresh
   eyes what may be wrong."* Standing, every tick that has idle capacity:
     - **upstream** — `git fetch upstream && git log <merge-base>..upstream/main -- crates/`; adopt
       what improves the agent loop on weak local models.
     - **opencode / other agents** — how do they fan work out, keep instructions specific, handle a
       stalled worker?
     - **FRESH-EYES AGENTS** — hand a subagent the code and the MEASURED defects but NOT our findings
       history, and let it form its own view. Our own history is exactly what blinds us.
   Verify every lead adversarially before acting, and check FIRST whether it already exists in-tree —
   that has caught three "new" proposals already (lesson 15).
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

## G8 — CLOSED. The sink is not SLOW, it takes 25 TURNS, and wall-clock = turns x 83s (F116).

`integrate-verify` held one node for 1590s — 23.6% of the run — but at **72.0 s/call it is FASTER per
call than the run median of 82.9**. It is simply long: 25 calls against a median of 2-4. So sink work
is TURN-REDUCTION work, never latency work. Predictions on the next sink (fewer than 25 calls,
`sink_capped` on a cut sink, the supervisor's prior findings actually reaching it) settle at the
boundary.

## CURRENT GOAL — G4: THE NODE CURVE. This is Mihai's goal one, stated directly.

**`swarm-1node-r0` is in flight and is the first 1-node datapoint on this build.**

⚠ **F129 RETRACTED THE HEADLINE I CARRIED HERE FOR SEVERAL TICKS.** It said the 53.8-min prefix
against 13.3 and 20.3 showed "every fleet-parallel phase collapses to serial on one node". **The
1-node run ran a redraft round and neither 3-node run did** — that one round cost 1584 s, **49% of
its whole prefix**, and removing it takes the ratio from 3.20x to 1.63x. The redraft trigger is
`plan_confidence < ask_floor`, measured by F121 at ~29% of runs with confidence 36-100 at EVERY node
count. This unit drew a low card, not a small fleet.

What survives, and it is the honest node signal: **research 2.65x (588 s vs 209 / 235 s)** — scouts
are independent lenses dispatched across devices and serialise on one node by construction.
Like-for-like **planning round 1 is 1054 s vs 587 s and 986 s = 1.34x on n=1 against n=2**, which is
not resolvable. **The node curve is NOT yet demonstrated by the prefix; do not lead with it.**

The unit still stays — it is the only 1-node datapoint on this build, and its low confidence makes
it the pairing baseline `retarget_off` has been waiting for since F121.

Also true of this unit and worth reading together: plan_confidence 81 < ask_floor 85, so the redraft
ladder DID run here — this is the low-confidence baseline `retarget_off` has been waiting for (~29%
of runs, F121). Pair the arm against THIS unit, not against baseline-n3-r0.

Still owed under G4: the replicate spread on this build. Two units exist but they are DIFFERENT ARMS
(baseline@3n 0.7186, retarget_off@3n 0.6720) and their difference is not a spread (lesson 7).

## CLOSED QUESTION — do NOT re-derive (F124)

**Mixed-kind tasks are not worth fixing.** 22 of 262 archived tasks mix kinds; they retry 18.2% vs
15.2% pure = **+3.0pp**, while the rule fired on 88% of plans. A deterministic mixed-kind splitter was
designed and NOT built because the measurement killed its premise. The shape recurs perfectly
(`code+docs` 10, `asset+code` 9, `asset+code+docs` 3, always on `cli`/`main`/`entry` or `api`) —
reproducible is not costly.

**What carries the retry burden is an interaction:** hard AND test-authoring, **60.0% (n=30)**,
against hard-not-test 12.1%, test-not-hard 12.5%, neither 5.9%. Brief length compounds it ONLY inside
test tasks (33/35/64% by bucket; producing tasks show no ladder). This is `kind_prompt`'s real,
un-gameable readout — see G3 below.

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
and OFF. ⚠ The old readout (`kind_mismatch_pct` -> zero) is DEAD — F111 proved it circular, hardcoded
to zero with the lever ON, success by construction. **Use F124's instead: hard-test retry rate, from
`task_dispatched.attempt`, baseline 60% (n=30) against 12.1% for hard non-test work.** Nothing in the
lever's accounting touches it. REGISTERED: it falls 60% -> ~12%. FALSIFIER: it stays near 60%, and
the whole instruction-density story is wrong for this kind.

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

**MEASURED 2026-08-02 17:05, so no future tick re-derives it or panics:** a full `cargo check -p
goose-cli` (which rebuilds goose, goose-providers and goose-cli) took free space 204 -> 190 GB, and
`target/debug` then measured **11 GB**. That is a ONE-TIME cost to a steady state, not a per-check
tax — debug grows with distinct build configurations, not linearly per invocation (the historical
64 GB was many checks AND tests across many configs). Against a 15 GB abort threshold there is 175 GB
of headroom, so **no action; deleting it now would only buy a cold rebuild on the next check.**
Also checked, because F97 cost hours: the one local snapshot is
`com.apple.TimeMachine.2025-09-15-150504.local (dataless)` — **dataless**, so it holds no deleted
blocks and the "delete frees nothing" pathology does NOT currently apply.

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
