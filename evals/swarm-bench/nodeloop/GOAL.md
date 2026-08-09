# THE GOAL — FINAL NUMBERS (set 2026-08-09, quality-first, per added node)

**Supersedes the speed-first framing below.** Mihai: *quality is the most important thing, but avoid
way too slow swarms — so quality first with some speed too*, and *calculate it per node added*.

## ⚠️ FIRST: "50% higher quality" cannot mean the score

The score is bounded at 1.0 and one node already sits at **0.733**. A 50% increase is **1.10 —
arithmetically impossible.** So the 50% is stated where it IS achievable and where it means what
Mihai actually wants: **HALVE THE DEFECTS.** "50% better" = *the app gets half as much wrong*, which
is both the standard reading on a bounded scale and genuinely ambitious.

## The per-node targets

Two different mechanisms scale two different ways, and the shape is read off the engine, not guessed:

- **QUALITY scales with SPARE nodes, ~linearly.** ⚠️ **CORRECTED BY MEASUREMENT (F662).** The first
  version of this claimed one node runs *zero* idle review. **That is FALSE — it runs pre-review in
  100% of runs.** What is true is the VOLUME: **pre-review runs 1.3×/run at one node and 10.6×/run at
  three — 8×.** So the per-node quality shape stands, but it rests on measured 8× scaling of
  pre-review, not on a zero that does not exist. **The judge does NOT scale at all** (85.0 → 82.8 per
  run), so it contributes nothing to the per-node quality story. ⇒ **25% of the defect gap per spare
  node**, carried by pre-review alone.
- **SPEED scales by Amdahl, sublinearly.** Measured parallel fraction p = 0.79 (EXECUTE is 77.2 of a
  98.1 min one-node run). Ceilings: 2 nodes 1.65x (39%), 3 nodes 2.11x (53%). Real DAGs reach a
  fraction of that, and the first added node delivers ~74% of the total.

| | 1 node (baseline) | **2 nodes** | **3 nodes** |
|---|---|---|---|
| spare nodes | 0 | 1 | 2 |
| **QUALITY — defect gap** | 0.267 | **−25% ⇒ 0.200** | **−50% ⇒ 0.134** |
| **⇒ score** | 0.733 | **≥ 0.800** | **≥ 0.866** |
| **Tier B (behaviour)** | 0.538 | **≥ 0.653** | **≥ 0.769** |
| **excellent-run rate (≥0.90)** | 36.4% | **≥ 45%** | **≥ 54.5%** |
| **SPEED vs 1 node** | — | **≥ 10% faster** | **≥ 15% faster** |
| | | (≥ 9.8 min) | (≥ 14.7 min) |

## Priority — quality is the GATE, speed is a FLOOR

1. **GATE (must pass):** the quality row. A 3-node swarm that is fast and no better is a FAILURE.
2. **FLOOR (must not breach):** **three nodes must NEVER be slower than one.** Today it is +10.0 min
   SLOWER at 1.04 SE — *the floor is currently breached*, which is why the ladder fix is the first
   thing to land.
3. **TARGET:** the 15% / 10% speed numbers. Missing these while passing the gate is a partial win,
   not a failure.

## Where the quality actually is — TIER B, and it is half of everything lost

| tier | what it checks | weight | 1-node | 3-node | score lost (1-node) |
|---|---|---|---|---|---|
| **B** | **does the app DO what the spec says** — sync completeness, resync idempotency, pagination, row shape, totals, chronological order, summary accuracy, UTC bounds, input validation, UI states/currency/offline | 0.30 | **0.538** | **0.572** | **0.1385** |
| C | robustness | 0.25 | 0.792 | 0.686 | 0.0520 |
| D | quality-of-build | 0.20 | 0.771 | 0.757 | 0.0458 |
| A | it exists and runs | 0.25 | 0.876 | 0.933 | 0.0311 |

**Tier B is 52% of all score lost at one node and 47% at three.** Half the app's specified behaviours
are wrong in the average run. **This is the goal's real target — everything else is rounding.** It is
also exactly Mihai's end goal (working apps), so the metric and the intent finally agree.

**Two real per-arm differences already visible, both worth chasing:** three nodes is **better on A**
(0.933 vs 0.876 — it ships something that runs more reliably) and **worse on C** (0.686 vs 0.792 —
robustness regresses). The C regression is a live defect, not noise to average away.

## What this costs to prove — and quality is CHEAPER than speed

Pooled score sd = 0.1839. At 80% power, alpha 0.05:

| claim | effect | **n/arm** |
|---|---|---|
| **3-node quality (gap −50%)** | 0.134 | **30** |
| 3-node speed (15%) | 14.7 min | 44 |
| 2-node quality (gap −25%) | 0.067 | 119 |

**The 3-node quality gate is the cheapest headline claim available (~30/arm, ~4 fleet-days).** The
2-node row needs 119/arm and is NOT affordable as a measured claim — it is stated as a *design
target* and will be judged on the mechanism (does a 2-node run fire idle-node review at all), not on
a powered score comparison. Saying otherwise would be promising a number the fleet cannot deliver.

## ⚠️ TWO MECHANISMS THAT ARE STRICTLY THREE-NODE-ONLY (F662) — one taxes speed, one taxes quality

Measured on 9 one-node and 15 three-node logs, positive control passing on every one:

| event | 1-node | 3-node | what it is |
|---|---|---|---|
| **`replanned`** | **0%** | **60%** | the author's documented speed penalty — made an already-finished run **55% longer** with work nobody asked for |
| **`task_split`** | **0%** | **53%** | a split child's whole task statement is **43 characters** (`scheduler.rs:35`) |

**These are the only two mechanisms in the engine that one node can never trigger.** Both make three
nodes worse, and both fire in the MAJORITY of three-node runs. Half of every three-node run hands an
extra node a near-empty instruction; 60% inject work the planner never asked for.

**The judge is pure overhead in both arms.** ~85 verdicts/run with ~80 skipped — a **94% skip rate at
one node**, 68% at three. Invoked ~165×/run, does nothing in most of them. This is the looping Mihai
named, it is measured, and it is not a node-count question.

## The named levers (Mihai's own list)

*"Judge stops looping, planning is done better"* — both already have evidence behind them:
- **Planning** — the confidence ladder costs **~25 min/round** on four independent estimates and buys
  +0.029 at 0.31 SE. Fix committed (`a9f43543d`), **not yet in the running binary.**
- **Judge / idle-node work** — F662 is measuring whether it fires at all before anything is claimed.
- **Dynamic replan** — the author's own comment records it making an already-finished 3-node run
  **55% longer** with work nobody asked for, gated on `idle_capacity()>=2` that one node never has.

---

# THE GOAL — read this FIRST on every wake, before anything else

**Overarching:** make the swarm actually WORTH IT — beat one node on time and on the quality of what
ships. Everything below is a sub-goal of that and nothing else belongs here.

## 🎯 THE MEASURABLE TARGET — set 2026-08-09 after Mihai asked for percentages

The overarching line above is a slogan until it names a number and a sample size. It now does.

| pillar | target | how it is judged |
|---|---|---|
| **SPEED** | 3-node **≥15% faster** wall-clock than 1-node | difference of means, same spec, both arms |
| **QUALITY** | 3-node **non-inferior within 15%** | lower bound of the difference stays above −15% |

**Both read off the SAME runs: ~44 runs per arm, about 6 fleet-days.** Not twelve — one set of cells
answers both questions.

### Why 15%, and not 5%

Measured on the deduped corpus (1-node n=11, 3-node n=15; pooled sd wall **24.3 min**, score
**0.185**):

| to detect | runs per arm | fleet-days |
|---|---|---|
| 5% | ~389 | ~55 |
| 10% | ~98 | ~14 |
| **15%** | **~44** | **~6** |
| 20% | ~25 | ~4 |

**At today's sample nothing under ~28% is provable on either pillar** — minimum detectable effect is
27.6% on wall and 28.1% on score. That is why every headline in this campaign has sat under one
standard error: the corpus cannot resolve anything smaller. **15% is the cheapest target that is both
reachable and falsifiable.** A 5% goal costs 55 fleet-days to prove and would be unfalsifiable in
practice.

### Why quality is NON-INFERIORITY and not "better by X%"

**Three measured mechanisms should make 3 nodes faster. Zero should make it better.** Six
quality-adjacent measures and three separate corpora have failed to separate the arms, and the
cleanest contrast available — equal-n, same-build — is **+0.0008**, a dead tie. Setting "better by
10%" would set a goal with no mechanism to reach it and no budget to measure it. Non-inferiority is
the honest form: **the swarm must not BUY its speed with quality.**

### Is 15% speed reachable? The decomposition, not a hope

- ladder fix (`a9f43543d`): 3-node planning 24.5 → ~14.5 min ⇒ **~7.5%**
- research is fleet-blind — ~7 min per run buying nothing; parallelising it ⇒ **~3%**
- execute is **already 7.7 min faster** at 3 nodes and is banked

That is **~10.5% from known causes**. The remaining ~5% has to come from **plan width, and that is
the real ceiling**: F601 measured the delivered plan widening only **1.11x for 3.00x of hardware**.
Until the architect emits a wider DAG, no scheduling change gets near 2x — let alone 3x.

### Two channels, two very different prices (F661)

**The mechanism check is powered today; the outcome check is not.** Measured on the same rows:

| channel | difference | SE | SE units | verdict |
|---|---|---|---|---|
| WALL | +4.1 min | 9.6 | 0.38 | hopeless at this n — drowned by execute variance |
| **PLANNING** | **+12.8 min** | **3.5** | **3.23** | **already decisive** |

So F654's P2 — 3-node planning falling below 20 min — should read after roughly **3-5 post-fix
three-node runs**, not forty-four. The **15% wall target still needs the full ~44 per arm.**

⚠️ **The fast channel must never stand in for the slow one.** Planning falling shows the ladder
stopped firing — a mechanism verdict, valid at small n. It does **not** show the run got 15% faster.
That is the outcome claim, it is what this goal says, and it costs six fleet-days whatever the
mechanism does.

**No cheaper measurement exists.** The 24% CV was checked for pathology: **zero of 26 rows carry
`timed_out` or `aborted`**, and both arms run smoothly from ~67 to ~150 min with no isolated outlier
a flag would catch. Trimming the tail until 15% becomes detectable would be fitting the instrument to
the answer, so the price stands.

### The failure gate

If, after the ladder fix has run its ~44 cells per arm, speed is not ≥15% and the decomposition above
is exhausted, **the answer is plan width or nothing** — and that becomes the whole campaign rather
than one queue item.

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

## PENDING SUPERVISOR RESTART — do it at the r0/r1 boundary, not before (2026-08-02 22:5x)

`loop.sh check` will report sweep.py as newer than the running supervisor (pid 78290). That is TRUE
and the gate is right to say so: `scoped_contracts` (F159, reps=3) is in the file and NOT in the live
queue.

DO NOT restart mid-unit. Baseline r0 has been running since 22:11 and is the unit the F154 freeze
exists to obtain; restarting the supervisor now throws it away and the freeze gets no closer to
lifting. Restart in the gap AFTER r0 writes its result.json and BEFORE r1 launches — units already
complete are skipped on resume, so nothing is lost at that boundary.

Until then the [ACT] line is EXPECTED, not a new problem. Do not suppress the gate to quiet it: a
warning that is true and deliberately deferred is the gate working.

## The restart is PREFERRED, not required — and why (2026-08-02 23:1x)

`arms_now()` (sweep.py:1016) re-reads a `QUEUE` file every pass, precisely so an arm can be added to
a loop that is already up — a running interpreter never sees a source edit (Lesson 23), and this is
the designed way around it. So `loop.sh check`'s advice ("restart it or the arm you just added will
never run") is INCOMPLETE: there is a no-restart path.

It is still the wrong path HERE. `arms_now()` does `arms.append(...)`, so a QUEUE-added arm lands at
the END of the order — behind baseline r3, sink_review, split_inherit_spec, split_off, prereview_off,
converge_off and retarget_off. After F164 that is exactly backwards: `scoped_contracts` is the only
queued arm aimed at the population that actually fails, and every arm ahead of it tunes one that does
not. It now sits at index 1, immediately after baseline, which only a restart delivers.

USE THE QUEUE FALLBACK IF the restart cannot happen for any reason — a late arm still beats no arm.

## THE IMPROVEMENT METRIC IS `python3 failures.py` — not a score (2026-08-02 23:2x)

Mihai asked "any improvement so far?" and the honest answer needed a metric that was not a pooled
score. F164 supplied it and `failures.py` now computes it:

    implementer   63 completed,  0 failed     0%
    test-author   42 completed, 13 failed    31%
    verify/sink   99 completed,  1 failed     1%

**Improvement means the test-author row moves.** A better pooled build score with that row unchanged
is the swarm getting luckier, not better — F147 is the precedent, where a run scored 0.819 while
silently LOSING a task. Run this before claiming any progress, and quote the cell with its n.

The instrument reproduces F164 exactly (its control), and in doing so CORRECTED it: classifying the
sink by task id BEFORE owned files moved `integrate-verify`'s single failure out of the implementer
column, so implementers are 0/63, not 1/65. "No implementer has ever failed" is exact, not rounded.

## RESTART DECISION REVISED — wait for sink_review, not for baseline (2026-08-03 00:0x)

I missed the r0/r1 gap; the sweep rotated at 23:48:49 while I was crunching. The running unit is
**`sink_review-n3-r0`**, not a baseline replicate. Do NOT kill it. Its gate:

    "the SINK owns 100% of the solo window in 2 of 3 measured runs — 543-1045s with two nodes idle
     while integrate-verify runs alone — and this is the only mechanism built to fill it. It has
     never run once: the scheduler's producer defaulted OFF while the drain and levers_resolved both
     defaulted ON, so every run REPORTED it enabled and its queue was never filled."

That is F162's problem and Prime Directive 3's mechanism, executing for the first time ever. It is
worth strictly more than a queue reorder. **Restart at the sink_review/next boundary instead**
(ETA ~01:26). Readout to take from it: `sink_review{prewarmed>0}` — if prewarmed is 0 with the lever
ON, the producer still cannot see its precondition and the fix is incomplete.

⚠ THE FREEZE CONDITION NEEDS RE-READING. `baseline` is at **n=1**, and the live NEXT is
`baseline-n1-r0` — the ONE-node cell, not a second 3-node replicate. `backlog()` is rep-major
(`for rep in range(target_reps): for c in cells()`), so `baseline-n3-r1` sits behind the entire rep-0
pass (31 units, ETA Wed 02:07). "Freeze lifts at baseline n=3" therefore means ~26 hours, not ~3.
Decide next tick whether to promote the two baseline replicates ahead of the rep-0 pass — that is a
sweep-ordering change (instrument, allowed) and it is the difference between the freeze lifting
tonight and lifting Wednesday.

## r0 HEADLINE NUMBERS (2026-08-03)
score 0.8429 | 97 min | pool 3/3 | void False | timed_out False | **fallbacks 0** (F49 detail-budget
fix confirmed: detail_fallback is ZERO) | **kind_mismatch 84.0%** | prefix 849.2s / plan 555.1s /
redraft 0. The kind-mismatch figure is the plan's Part-3 defect #2 measured on this build, and it is
WORSE than the 60% that finding was written from — F157 and F158 are aimed straight at it.
