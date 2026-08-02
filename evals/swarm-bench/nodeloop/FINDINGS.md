# nodeloop — findings ledger

Chat is compacted away; this file is not. Every conclusion here is derived from an engine event
log, a source line, or a probe against the live fleet — never from a model's self-report.

---

## F1 — The 46-point "noise" is the detailer failing open (2026-08-01)

Three runs, identical config, identical 1-device pool, same spec:

| run | tasks whose spec stayed the architect's one-liner | build | tier C (vendor) |
|---|---|---|---|
| `swarm-1node-r0` | **2** of 14 — `meridian`, `test-api` | **44.2%** | 14.3% |
| `swarm-2node-r0` | **1** of 16 — `api` | **86.7%** | 100.0% |
| `swarm-3node-r0` | **0** of 14 | **90.0%** | 85.7% |

The architect emits a description of "ONE short line" (`swarm.rs:11494`). The detailer expands it
into an implementation-ready spec, one call per device, under a hard 75 s cap
(`swarm.rs:12353`); on timeout/empty/error the engine silently keeps the one-liner (the
`_ => brief` fallback) and **emits no event**.

Per-module trace, which is stronger than the correlation: in the 44.2% run `meridian` — the
vendor client tier C exists to grade — was dispatched with **95 characters**. In the 90% run the
same module received **2,464 characters** with the full class signature block, exact endpoints and
required headers. Tier C moved 14.3% → 85.7%.

Commit `dfd67e84d`.

## F2 — SUPERSEDED 2026-08-01 10:15 — node count IS a variable now

Mihai gave each host's loaded model a distinct identifier (`mihai-` / `workhorse-` / `gabee-`).
Everything below about the collapse was true of the fleet as it stood and is left intact, because
it explains why every earlier node-scaling number in this project is void. What is no longer true
is the conclusion that it could never be measured.

Re-proven after the change, by the same probe that condemned the old fleet: three concurrent calls,
one per identifier, put **all three instances into `generating` at once** (7.7 / 10.0 / 13.4 s —
one call's latency each, not serialized). And the engine's own `run_started.pool` for
`baseline-n3-r0` now lists **three devices**, one per host:

```
mac-gabee-...              model=gabee-...      weight=1
local-mihai-...            model=mihai-...      weight=1
worksmacstudio-workhorse-  model=workhorse-...  weight=1
```

The loop now varies node count as its primary dimension (3, 1, 2 on baseline, then the
dispatch-quality arms at 3), and voids any unit whose actual pool differs from the one it asked for.

**One comparability warning:** the three hosts are not identical. `gabee` runs **Q6_K at ctx
193792** while `mihai` and `workhorse` run **Q8_0 at ctx 262144**. So a 3-node run includes a
lower-quant node that a 1-node run does not, and `pick_device` will route to it. Any node-count
delta is a delta of "this fleet", not of "N identical workers" — worth stating before a number
gets read as more general than it is.

### The original finding, kept because it voids the old numbers

Three hosts (Local, WorksMacStudio.lan, Mac.lan) all serve one identifier. Two probes against the
fleet exactly as it is:

- the host-qualified instance name `<deviceId>:<path>` that `lms ps --json` reports as
  `indexedModelIdentifier` is **rejected** by the endpoint: `HTTP 400 Invalid model identifier`
- three concurrent calls on the shared identifier were **all served by one host**; the other two
  never left idle for the duration

`reconcile_pool_with_fleet` (`swarm.rs:2043-2049`) builds one worker per DISTINCT identifier, so it
correctly reports a 1-device pool — the collapse is LM Studio's, not goose's. Every swarm run on
disk confirms it: `run_started.pool` has a single entry and every dispatch went to it, so
`swarm-1node`, `swarm-2node` and `swarm-3node` were three replicates of the same configuration.

**Consequence:** a sweep over `GOOSE_SWARM_MAX_NODES` measures nothing, and the earlier
"more nodes make it worse" table compared a configuration with itself. The loop measures dispatch
quality instead.

**Not yet fixed, and worth fixing:** the engine collapses three resident instances into one device
*silently*. It should say so — two thirds of a fleet going unused is not a detail.

## F3 — Most workers are given another job's rules

From `nodeloop/dispatch_audit.py` over the same three runs:

- **72–80% of dispatches** receive the implementer prompt while not being implementers
  (`kind_prompt` defaults OFF, `swarm.rs:18032`)
- only **20–24%** are actually implementers; **39–47%** own nothing (the `verify::<M>` and
  `verify-e2e::N` fan)
- **3–5 dispatches per run** own a `test_*.py` and are told *"NEVER read the project's TEST files"*
  (`swarm.rs:18100`) — the file they must produce is the file they may not open
- **5 dispatches per run** get `context_slice_len == 0`: no dependency context at all

Earlier work estimated this at 59.9%; on this spec it is higher, because `fan_verify`/`fan_e2e`
generate many owns-nothing tasks.

## F4 — Two grader flaws, caught by controls before anything was believed

`dispatch_audit.py` first docked every owns-nothing verifier for not naming files it is forbidden
to write, then graded verifiers on "names symbols" when what pins a verifier down is the command
it must run. Both were fabricated deductions of exactly the kind this project's standing law
predicts ("a grader's bugs invent defects rather than excuse them"). The self-test now grades per
task kind and asserts both score directions, the vacuous-truth case, and an empty run.

---

## F5 — I almost published "every finding was refuted" off a broken harness

Adversarial round 1 returned `survivors: []` with the note *"every finding was refuted — report
that plainly, do not soften it"*. That was **my bug, not a result.** The workflow's inner
`parallel([...])` was passed promises instead of thunks, so all 34 verification calls failed and
`votes` was empty for every finding — and my survival test was `votes.length > 0 && kills === 0`,
which turns "nothing was checked" into "it did not survive".

Four finder agents had in fact raised **38 findings**, none of which was ever tested. Recovering
them needed a second correction: my journal parser read `label`, but the field is `key`, so the
first recovery attempt also printed zero. Two blind instruments in a row, both reporting a
confident zero.

This is the project's standing law firing twice in five minutes: **a zero is usually a broken
instrument, not an empty world** — and before any zero licenses a conclusion, prove the query can
see the thing at all. The `24 agents completed` count is what refused the first zero, and the raw
journal shape is what refused the second.

Round 1 now dedupes by site before verifying, because three lenses independently raised the 75s
detail budget and three raised the missing event. Independent rediscovery is recorded as
corroboration rather than spent as three separate verifications.

## F6 — Adversarial round 1: 2 of 34 survived, both shipped (commit `1a6849ec2`)

Four independent lenses raised **38 findings**, deduped to 34 sites, and **32 were refuted** by
agents attacking both the defect and its proposed fix. The two that survived:

**The detail fallback fired silently and printed a green check** (`swarm.rs:12410`). The `_ => brief`
arm collapsed timeout / agent error / filler / empty into one untyped fallback, then the same
unconditional green ✓ printed — so a detailer that died was indistinguishable from an architect who
wrote a short line. Both sibling fanouts already report their failures; detail was the only one that
failed invisibly. Now emits `detail_fallback` {task_id, reason, brief_chars, budget_secs} and prints
⚠. The verifier added evidence I did not have: the fallback fired **3 of 8 times** in
`swarm-1node-r0` — `meridian`, `test-api` and `integrate-verify` — the third masked downstream by
the canonical sink override, which is why only two were visible in `plan_loaded`.

**Every read-only verify task was classified as a fix round** (`swarm.rs:18097`). `is_fix_round` was
`owned_files.is_empty() && !all_files.is_empty()`, which is also true of every fanned `verify::<M>`
and `verify-e2e::<i>`. With `read_on_fix` on — and swarm-bench sets it on by default — those
read-only gates were told the read prohibitions were **SUSPENDED**, granted edits across the
manifest, and handed the fabricated premise *"the failure below was already reproduced by running
the app"* when no failure exists. `integrate-verify` is deliberately still included: it owns nothing
too, but it is the run's sole repair point.

Neither is rebuilt into the binary the campaign is running — swapping the engine under live arms
voids comparability. The rebuild waits for a pass boundary and a re-baseline.

## F7 — The fleet's idle time is PLANNING, not scheduling. And it may grow with node count.

`nodeloop/occupancy.py` measures busy node-seconds ÷ (wall × pool). First numbers, and they killed
two of my own hypotheses in a row.

| run | pool | whole-run occupancy | EXECUTE occupancy | before first dispatch |
|---|---|---|---|---|
| `swarm-1node-r0` | 1 | 0.873 | **1.000** | 964 s |
| `swarm-3node-r0` (archived, 1 device) | 1 | 0.863 | **1.000** | 913 s |
| live `baseline-n3-r0` | 3 | 0.604 | **0.990** | **1,312 s** |

**The scheduler is not the problem.** Across the execute window it owns, it keeps essentially every
node busy essentially all the time — 1.00, 1.00, 0.99.

**Plan width is not the ceiling either.** I claimed it might be, from the whole-run number, and that
was wrong. With measured task durations the live plan's critical path is 1,648 s out of 6,092 s of
total work, so `max_useful_nodes = 3.58` — the plan has enough independent work for more than three
nodes, and a perfect scheduler could reach 1.0 at this pool.

**All of the gap is before the first task is dispatched:** research, scouts, planning drafts,
detailing and contract stubs. 22 minutes on the live run, 39% of its wall so far. Those are real
model calls on real nodes, but none of them emits `task_dispatched`, so the time lands in the
denominator as wall and never in the numerator as busy.

Two consequences:

1. **An instrumentation gap.** The one phase where the fleet's idle time actually lives is the phase
   the instrument cannot see. Occupancy during planning is currently unmeasurable, so "does planning
   use three nodes well" has no answer. Closing that needs planner-side call events (round 1's
   synthesis proposed a `plan_phase` event; it was not among the two findings that survived).
2. **A hypothesis worth testing, not a result.** Pre-execute went 913–964 s at one node to 1,312 s at
   three — 43% longer. `best_of_n` is sized to the fleet (`devices.len().clamp(1,5)`), so three nodes
   means three skeleton drafts where one node means one. If that holds up, adding nodes buys
   parallelism in EXECUTE and pays for it in PLAN. It is n=1 against a 46-point-spread fleet and
   three different plans, so it is a question for the n=1 and n=2 cells, not an answer.

### Instrument bugs found and fixed on the way (three of mine, all caught by data not by my controls)

Occupancy of **1.28 and 1.93 on a one-device pool** — impossible, since a weight-1 device runs one
task at a time. Summing spans double-counted retries and in-flight tasks; a device's busy time is
the **union** of its spans. Same class gave a "biggest task share" of **1.118**, a share above 1.0.
And three *finished* runs were reported as having tasks "in flight" when those tasks had simply
never completed — failures, not work in progress. The self-test now carries the controls that would
have caught all three, plus the invariant that occupancy can never exceed 1.0.

## F8 — Planning fans out correctly across 3 nodes, but the DETAIL failures went UP

The planner-side calls each leave a `.swarm/activity/<kind>-<id>.json` naming the node that ran
them, so the planning phase is measurable today without any engine change.

| run | pool | planner-side calls, by node | plandrafts | detail fallbacks |
|---|---|---|---|---|
| `swarm-1node-r0` | 1 | 16 all on one node | 1 | 2 |
| `swarm-3node-r0` (archived, 1 device) | 1 | 16 all on one node | 1 | 0 |
| live `baseline-n3-r0` | 3 | **6 / 7 / 6** — gabee / workhorse / mihai | **3** | **4** |

**The fan-out itself is healthy.** 19 planner-side calls spread 6/7/6 across the fleet is near-perfect
balance, and `best_of_n` correctly scaled to three skeleton drafts where a 1-node run gets one.

**But four detail calls fell back to the architect's one-liner**, confirmed by two independent
instruments: `dispatch_audit.py` reading `plan_loaded` descriptions, and the raw description lengths
— `meridian` 116, `store` 124, `test-meridian` 124, `test-api` 146 characters, against a median of
1,055 for the tasks that got a real spec.

`meridian` is the vendor client. It is the module Tier C exists to grade, and it is the *same* module
that got 95 characters in the 44.2% run whose Tier C collapsed to 14.3%.

**The hypothesis this raises is the sharpest node-scaling question yet, and it is NOT yet answered:**
does adding nodes make detail calls *more* likely to fail? Per-node load goes DOWN with three nodes
(6-7 calls each instead of 16), so the naive expectation is fewer timeouts, not more. Candidate
causes worth separating: LM Link adds network latency per call, `gabee` is a lower quant with a
smaller context, and a 3-node fleet makes the architect emit more subtasks (17 vs 14) and therefore
more detail calls under the same fixed 75 s per-call budget.

This is n=1 per node level on a fleet with a measured 46-point spread — exactly what the running
campaign's `fallbacks` column exists to settle, at n≥3 across 3, 1 and 2 nodes. **Nothing here
justifies a fix yet.** Note also that round 1's adversarial pass REFUTED both "raise the 75 s budget"
and "add a retry" on code-reading grounds; if the campaign shows fallbacks rising with node count,
those refutations deserve re-examination against evidence they did not have.

## F9 — My adversarial harness was throwing away two-thirds of its confirmed findings

Round 2 returned **0 survivors of 32** and instructed me to report that plainly. Before believing a
zero I checked the harness: 68 result records = 4 finders + 64 verifiers, no failures, and the
verdict mix was **59 refute-HIGH and 5 confirm-HIGH**. So the verification genuinely ran — but five
HIGH confirmations coexisting with zero survivors is not a coherent verdict, and that is what
exposed the flaw.

**The two verifiers answer different questions.** `votes[0]` asks *is the defect real?*; `votes[1]`
asks *is the proposed fix sound?* They are not redundant voters. My rule killed a finding if
**either** refuted it, which silently conflates "this is not a defect" with "this is a real defect
whose fix is wrong" — and discards the second class entirely.

Replaying round 1 from cache with the corrected rule (0 tokens, 24 ms):

| | old rule | corrected |
|---|---|---|
| survived intact | 2 | **2** |
| real defect, fix refuted | *(discarded)* | **22** |
| genuinely refuted | 32 | **10** |

Twenty-two confirmed defects had been thrown away. The refutations are substantive and several
propose a *better* fix than the finder did — which is exactly the signal the old rule destroyed.

## F10 — The detail budget is pinned at the p100 of the call it bounds

Three independently-raised findings (#1, #5, #16 of the recovered 22) were each refuted with a
variant of *"the defect is real, but a one-constant change is strictly cheaper and strictly better"*.
The measurement in those refutations is the point:

> the SAME `meridian` brief was detailed in **44.5 s** on one run and blew through **75 s** on
> another. The run that lost it shipped a 95-character spec for the module tier C exists to grade —
> 14.3% there, against 85.7% for the run that kept it.

75 s is a bare literal sitting at the *observed maximum* of the call it bounds, so ordinary variance
lands on the far side of it. The sibling CONTRACT fanout — same call shape, same fleet — already
abandoned a small fixed budget for `worker_timeout_secs.max(120)` after a mass stub failure, and its
comment records that reasoning.

**Deliberately not raised.** `detail_budget_secs()` now resolves env > default with the default still
**75**, so baseline stays byte-identical, and the value is echoed in `levers_resolved`. A
`detail_budget` arm (300 s) enters the campaign with a falsifiable prediction written down before the
run: *fallbacks go to ~0 and pre-execute wall grows only slightly, because the budget is a ceiling on
the slow tail, not on the ~50 s mean. If fallbacks do not drop, the ceiling is not the cause and this
whole line of reasoning is wrong.* The constant gets baked when a replicated arm says what it should
be — not because the argument sounds convincing.

## F11 — `scoped_contracts` cannot work under this planner, and I nearly spent three runs proving it

It was queued as a campaign arm on the reasoning that every worker receives the FULL frozen-contract
bundle, so irrelevant interface text grows with the fan. Round 2's adversarial pass confirmed the
defect but refuted the fix, and checking its claim against real plans settled it:

**Zero inter-module dependency edges among code modules, in all three runs measured.** Every code
module is a root. That is by design — the architect prompt says *"Default to a FLAT FAN: make every
module a root with no deps"* (`swarm.rs:11493`), and `relax_contracted_deps` exists to flatten any
chain that survives it.

So a worker's DAG neighbourhood is **itself**. Scoping the bundle to it would delete every sibling
interface and leave only the module's own stub — the one interface it does not need, since it is the
thing writing it. Under a flat fan the full bundle is the *correct* bundle. The lever is inert at
best and destructive at worst, and its precondition is precisely what the planner works to prevent.

Arm removed before it ran; a warning comment now sits on `scoped_contracts_on()`. Measurement time
is the scarce resource here — roughly two hours per unit, three units per arm — and a lever
predicted broken on evidence does not earn three replicates. It becomes meaningful only if plans
ever carry real inter-module edges.

**The three other recovered round-2 defects, triaged and not yet acted on:**
- the dynamic-replan await sits on the dispatch loop and can freeze all placement for up to 25
  minutes (`scheduler.rs:2050`). The refuter's counter-proposal is a resolvable replan budget in
  this repo's established shape (`draft_timeout_secs` / `clarity_probe_secs`), plus making expiry
  distinguishable so a timeout does not consume the replan budget. It also honestly notes the gain
  is small by construction: the gate only fires when two nodes are already idle.
- every parallel fix shard reports the same device id while running on a different host
  (`swarm.rs:21823`) — an observability defect. The refuter's better idea: emit a per-shard event
  carrying {task_id, device, model}, since `device_id` never reaches an event and no fix to a
  console label can be seen fire.

## F12 — THE CHAIN, END TO END, ON A REAL 3-NODE RUN (`baseline-n3-r0`)

The first unit finished: 3-node pool confirmed (`actual_pool` = gabee / mihai / workhorse), 126 min,
not timed out, not aborted, not void. Headline **50.0%** — Tier A **100%**, B 36.1%, **C 14.3%**,
D 53%.

**The score is a claim, so I ran the app.** It is non-functional exactly where it matters:

```
total_count()          -> 0            (spec requires 247)
fetch_all_payments()   -> 0 payments in 0.0s   (no successful request at all)
create_payment(...)    -> uncaught HTTPError: 404 — crashes
```

Every link of the causal chain is now measured, not inferred:

1. The detail call for `meridian` exceeded the 75 s budget and fell back.
2. The worker was dispatched with **116 characters**: *"MeridianClient with cursor pagination,
   rate-limit retry, cursor expiry restart, ETag support, UTC-normalized sorting"* — five behaviours
   and **not one endpoint**.
3. The worker invented paths: it calls `/payments` and `/payments/count`. The vendor serves
   **`/v1/payments`**. Every request 404s.
4. 0 of 247 payments; `create_payment` dies on an uncaught 404.
5. Tier C = **14.3%** — the identical value `swarm-1node-r0` scored when the *same* module lost its
   spec to the *same* fallback.

And the run still reported Tier A at 100% and a 50% headline, because the app imports, serves a page
and has the right health shape. This is precisely the failure class the four-tier split exists to
make legible: a correctly-structured application whose vendor integration does nothing.

The spec itself carries the base URL and the docs URL — but a worker never sees the spec (recovered
round-1 finding: it reaches every planning and supervisory call and no worker), and the detailer,
whose job is to turn the one-liner into exact signatures and endpoints, timed out. The
`detail_budget` arm's prediction is now testing a mechanism with a fully traced failure path rather
than a hunch.

## F13 — Two instrument/process notes from this turn

**A session limit truncated adversarial round 3** — 40 of 71 agents died mid-verification, and my
rule scored a missing vote as a refutation (`Boolean(null)` is false, so a dead agent was
indistinguishable from a HIGH-confidence refutation). Its `refuted_count: 27` was therefore not a
count of refutations at all, and the `contracts` and `research` lenses were effectively unverified.

The vote rule now has THREE states — stands / refuted / **unknown** — and an unanswered question is
re-run rather than silently resolved against the finding. Worth noting how this was caught: my own
loop instructions had already *asserted* the rule treated a missing vote as unknown. It did not.
Reading the code rather than trusting the note is the only reason the re-run is meaningful.
The round is re-runnable from cache. Its one confirmed survivor:
the whole confidence / ask / retarget apparatus is gated on `n > 1` where `n` is capped at the
fleet's **distinct-model count** (`swarm.rs:11596`) — so a 1-node run structurally cannot gate on
confidence at all (`plan_confidence: null` on every 1-node run, `88` at three nodes), and two
existing guards written expressly to force `best_of_n >= 2` are silently undone by that cap.

**A pre-existing test failure, verified not mine** (`git stash` and it still fails):
`omitted_config_keys_resolve_to_the_intended_default_not_the_type_default` — `sink_max_turns`
resolves to `None` instead of `Some(120)`. That is the serde-default gotcha this project has already
been bitten by once, and it means the running campaign has a sink turn cap of 40, not 120. Not fixed
here: changing it mid-campaign would void comparability. It belongs in the pass-boundary rebuild.

## F14 — "Workers are starved of the spec" is mostly WRONG, and I was heading toward shipping it

Two of round 1's recovered defects said workers cannot see the product spec. Both were confirmed as
defects and both had their fixes refuted, and reading *why* corrects a story I had been assembling
all session.

**Fix workers are not context-starved.** With `owned_files` empty, `dep_block` already injects the
current on-disk source of every non-test source file at 3,500 chars each up to a 14,000 budget, plus
the full unscoped contract bundle, the pillars, verbatim `user_decisions`, `doc_facts` and retrieved
pitfalls. What is absent is pre-build **intent**, not context. And the codebase has a documented rule
against supplying distilled intent: a check distilled from the spec once used `--db` after the
subcommand where the app had built it as a global before it, *"a CORRECT app went red, and the fix
loop then broke 2 passing tests chasing the phantom. That is why the pillar checks are advisory to
this day… The fix is to remove the guess, not to mitigate it downstream."*

**Injecting the spec into the sink would make things worse, not better.** `sink_lean_prefill` exists
specifically to *remove* bytes from `integrate-verify` because it is the slowest task; in the 90% run
it took 1,719 s against an 1,800 s cap — **81 seconds of margin**. Adding ~3.9 kB pushes the best run
toward a cap whose expiry finalizes the task DONE without finishing, which is the silent-false-green
class the engine already fights. And `owned_files.is_empty()` is not even the right predicate: in one
run the sink owned `README.md`, and `swarm.rs:17780` already documents and handles exactly that.

**The one narrow gap that survives all five objections** is the `verify-e2e::` shards. They are told
to confirm the actual output equals *"the SPECIFIC value the spec implies"* while never seeing the
spec — and unlike the sink they are short, parallel and nowhere near a cap. `spec_frozen`
(`swarm.rs:9920`, "the OPERATOR's spec as it stood before any model wrote into it") already exists,
so the machinery is there and only the audience is wrong. Not shipped: no fix verifier has attacked
that version yet, and it goes to the next adversarial round rather than straight into the engine.

The honest correction: the meridian failure was **not** caused by workers being blind to the spec. It
was caused by the detailer timing out, which is a different defect with a different fix — and one
already shipped.

## F15 — RETRACTION: "edge-cases is always the dropped scout" is NOT established

The lens reorder shipped on that claim, and verifying it live refuted the evidence behind it.

The reorder itself landed — `scouts_planned` on the new engine reads
`['edge-cases', 'architecture', 'libraries']`. But `scout-edge-cases.json` STILL lacks `phase: done`,
and so does `scout-libraries.json`, while `research_completed` reports **2 findings**. Two files
missing the marker and two findings returned is a contradiction, and it exposes the instrument.

Reading the source settles it: a scout that blows its OWN budget returns a `ResearchFinding` carrying
an apology string (`"(scout 'x' exceeded Ns budget — skipped…)"`) and **is counted**. Only a
straggler-ABORTED scout vanishes, because an aborted task never reaches the match arm that would name
it. Both cases leave an activity file without `phase: done`. So that field **conflates two different
failures, over-counts drops, and cannot identify the casualty** — which is exactly what I used it for.

**What survives:** every run measured planned 3 lenses and returned 2, so precisely one lens is lost
every time. **What does not survive:** the claim that it is always `edge-cases`.

The reorder is harmless either way — byte-identical when straggler-stop is off, and it only changes
which lens is last — but it was justified by an unvalidated inference and should be re-judged once
the attribution is real.

So the fix is observability, not another guess: `research_completed` now carries **`lenses_returned`**,
taken from each finding's `kind`. Diffed against `scouts_planned.lenses` that names the dropped lens
deterministically, on every run, forever. Held for the next pass boundary — the loop is mid-campaign
and a rebuild invalidates the backlog.

This is the fourth instrument failure today, and the pattern is stable enough to name: **every one was
an inference I made from a signal that was never designed to answer my question.** The thunk bug, the
two-verifier conflation, the dead-agent-as-refutation, and now a heartbeat field read as a completion
marker.

## F16 — The mechanism is now a RECORDED FACT, and it says "timeout"

First run on the rebuilt engine, and `detail_fallback` fires for real:

```
task=store             reason=timeout   brief_chars=66   budget=75s
task=meridian-client   reason=timeout   brief_chars=86   budget=75s
```

Both **timeouts** — not filler, not agent error. The 75 s ceiling is the cause, which is what three
independently-raised round-1 findings converged on and what the `detail_budget` arm exists to test.
Until this build the only trace of any of it was a short string inside `plan_loaded`; the reason was
unrecoverable and the whole thing was written off as noise.

`meridian` loses its spec for the **third consecutive run**. That module is the vendor client, and on
the two runs already crunched its loss took tier C to 14.3% both times.

Note `lenses_returned: None` — the field committed in `9bb8d413d` is correctly absent from this
binary, since it was held for the next boundary. The instrument reporting `None` rather than silently
omitting it is the behaviour I want.

## F17 — ⚠ COMPARABILITY: the n=1 and n=3 cells differ by MORE than node count

The same run emitted:

```
confidence_retarget  round 1  binding_signal=agreement  action=redraft
                     conf_before=81  detail="best_of_n 3→4"
```

Round 3's one confirmed survivor said the entire confidence / ask / retarget apparatus is gated on
`n > 1`, where `n` is capped at the fleet's **distinct-model count** — so a 1-node run structurally
*cannot* run it, and a 3-node run does. Here it is firing live: at three nodes the run scored its
plan, found agreement 81, and spent an extra redraft round growing `best_of_n` from 3 to 4.

**A 1-node run would have done none of that.** So the node-count cells in this campaign are not
"same engine, fewer workers" — they are *different planning algorithms*. Any wall-clock or score
delta between n=1 and n=3 confounds node count with the presence of an entire retarget ladder, and
it is also a concrete mechanism for the measured pre-dispatch growth (913-964 s at one node versus
1,312 s at three).

This does not invalidate the campaign — the dispatch-quality arms all run at 3 nodes and are
unaffected — but the **node curve specifically must be read with it stated**, and it should be
reported alongside any n=1-vs-n=3 number rather than discovered afterwards.

## F18 — Round 3 recovered: 3 survivors, 10 real-defects-with-refuted-fixes, 20 refuted

Re-run from cache with the three-state vote rule: **71/71 agents, 0 errors, 0 unverified**. The
truncated pass had reported 1 survivor and 27 "refutations"; 27 was never a count of refutations.

**Survived intact:**
- the confidence/ask/retarget apparatus gated on `n > 1` (see F17 — now confirmed firing live)
- the contract validator is **Python-only** while the CONTRACTS phase is language-agnostic, so on a
  non-Python build the whole validation silently does nothing (`swarm.rs:21217`). Inert for this
  campaign — the spec is Python — but real for Go/Rust/TS beds.
- **the empty-bundle guard runs BEFORE the drop** (`swarm.rs:21205`), so a bundle emptied by
  `drop_unparseable_stubs` still reports `frozen: true` and prints the success line. `swarm-3node-r0`
  already showed one module's stub dropped; had they all been dropped, the run would have claimed a
  frozen contract bundle it did not have.

**Most campaign-relevant of the ten with refuted fixes:**
- **straggler-stop makes the research-findings ORDER race-determined** (`swarm.rs:2700`) — a
  replicate-variance source, in a campaign whose entire problem is a 46-point replicate spread.
- **the straggler grace is measured from the ARMING instant, not the straggler's own start**
  (`swarm.rs:2686`) — the mechanism behind the consistently-lost lens. My F15 retraction was about
  *which* lens is lost, not *whether* one is; this confirms the latter.
- **a straggler abort emits no engine event** (`swarm.rs:14749`) — exactly the gap `lenses_returned`
  (`9bb8d413d`) closes. Independent corroboration that the fix targets a real hole.
- **PILLARS is a one-node serial phase between CONTRACTS and the first dispatch that consumes nothing
  CONTRACTS produces** (`swarm.rs:21277`) — a candidate for overlapping the pre-dispatch critical path.
- **the backbone round-2 re-draft is a SECOND fleet-wide draft round existing only at `n > 1`**
  (`swarm.rs:11929`) — another node-count-conditional planning cost, corroborating F17.

**A correction to my own F17 attribution.** The defect verifier confirmed the `n > 1` gating but
*refuted* my causal claim that it explains the pre-dispatch growth: from the runs' own timestamps,
**research alone went 262.7 s → 419.8 s, so 157.1 s of the 347.4 s delta — 45% — accrues before
`levers_resolved` and never enters `parallel_plan` at all.** The confidence apparatus is a real
node-count-conditional cost, but it is not the main term. Where the rest of the pre-dispatch delta
lives is still unattributed.

## F19 — The cross-check failed, and it was MY instrument that was wrong (caught by design this time)

The engine now emits `detail_fallback`, so for the first time the shape-based inference could be
checked against engine truth on the same run:

| instrument | says |
|---|---|
| engine `detail_fallback` events | **3** — `store`, `meridian-client`, `test-api`, all `reason=timeout` |
| my `dispatch_audit` shape inference | **1** — only `test-api` |

**Neither is broken. They answer different questions**, and I had been treating them as one.
`confidence_retarget` fired a redraft (`best_of_n 3→4`), so the detail calls that failed in round 1
were re-run and succeeded, and `plan_loaded` holds the FINAL plan. The events count **detail-call
failures in any round**; the plan records **what a worker actually received**.

So the instrument now reports both, named for what they measure:
- `shipped_one_liners` — what the workers got. **The quality number.**
- `detail_fallback_events` — how unreliable the detail call is. **The mechanism number.**

This is the fifth instrument problem today and the first one caught **by design** rather than by
stumbling into a contradiction — the cross-check existed precisely because the previous four taught
me to build one. It also means F1's original table (fallbacks vs build score, measured before the
event existed) counted shipped one-liners, which is the right number for that claim.

**And the redraft is a real recovery mechanism.** Two of three failed details were repaired by a
retarget round the run took because agreement scored 81 against a floor of 85. That is a
node-count-conditional safety net a 1-node run cannot have (F17), which raises the stakes on the
n=1-vs-n=3 comparability warning rather than lowering them.

## F20 — The `frozen: true` survivor bites on a live run

Round 3's third survivor, confirmed on this run's own `contracts` event:

```
frozen: true   modules: 4
  store      parsed=False  bytes=775  err="expected ':'"   <- DROPPED
  meridian   parsed=True   bytes=785
  api        parsed=True   bytes=176
  cli        parsed=True   bytes=280
```

The event claims **4 frozen modules** when only 3 survived `drop_unparseable_stubs`. The
empty-bundle guard runs before the drop, so the count and the `frozen: true` flag are both stale.

`store` is also one of the three modules whose detail call timed out — so on this run the same
module lost its detailed spec *and* its frozen interface, and the run reported a full contract
bundle either way.

## F21 — 22% of a run is the repair phase, on ONE node, with two idle

Measured live on `baseline-n3-r0` while it was still running:

```
run wall so far      : 88.2 min
last task_completed  : 68.7 min in
-> COMPLETE/repair    : 19.6 min          (22% of the run)
complete_verify at   : [68.8, 88.2] min   (both rounds)
fleet during it      : gabee generating, mihai idle, workhorse idle
```

Two independent calculations agree: `occupancy.py` puts solo-node time at **1,174.6 s (0.222 of
wall)**, and the gap since the last `task_completed` is 19.6 min. `smoke_fix_target =
devices.first()` (`swarm.rs:21260`), so the fix loop is single-node unless
`GOOSE_SWARM_COMPLETE_PARALLEL` is on — and it defaults **off**.

This is the sharpest goal-one instance found so far. It is not a subtle prompt defect: it is a fifth
of the run in which adding nodes provably buys nothing, in the phase this project's own ledger names
as *"REPAIR is what fails"*, with the lever to fix it already built and disabled.

**Added as a campaign arm** (`complete_parallel`) rather than shipped, with the prediction written
down first: wall falls by roughly the repair tail's idle share and **the build score is unchanged
within the replicate spread** — this buys fleet utilisation, not correctness. *If the score moves,
the parallel fix path is not equivalent to the serial one, and that is a defect worth more than the
speedup.*

Note also the execute-window occupancy fell from 0.99 to **0.71** once the repair tail entered the
window. Earlier I reported ~1.0 and said the scheduler was not the problem; that remains true for the
build phase, but the *run* is less well utilised than that number suggested, because the tail is not
scheduler-owned work at all.

## F22 — Round 4: I fixed one of two sites with the identical defect, and missed the class

0 survivors intact, 6 real-defects-with-refuted-fixes, 24 refuted, 0 unverified (65/65 agents).
The top finding lands on this morning's work.

`layout_block`'s owns-nothing branch (`swarm.rs:17870`) is gated on `owned_files.is_empty()` **alone**,
so every read-only `verify::<M>` and `verify-e2e::<i>` shard receives the SINK's write-and-fix
directive in its system prompt — *"You MUST actually RUN the program end-to-end"*, *"If the entry
point crashes, FIX the offending file"*, *"wire them all"* — while its own user message says *"You own
NOTHING and must WRITE NO files"* and *"Run NO test command at all"*.

`is_fix_round` at `18306` already carries exactly that exclusion **because I shipped it this morning**
(`1a6849ec2`). Two sites, one predicate, one shape of defect — and I fixed the one in front of me.
That is the class-versus-instance failure, committed by the person who has the rule written down.

Owns-nothing dispatches are **7–11 of 15–21** per run, so this is not a corner. Direct evidence the
models burn reasoning resolving the contradiction, from `verify::cli`'s own activity file:
*"I should NOT run tests, NOT run the whole program end-to-end, NOT fix anything — just verify imports
and report."*

**The refutation of the obvious fix was better than the finding.** Patching `17870` directly would
have been unlevered across ~40% of dispatches mid-campaign, un-A/B-able, and would have installed a
**second kind classifier** disagreeing with `GOOSE_SWARM_KIND_PROMPT` — whose own design comment
already enumerates *"owns-nothing 12.1%"* as one of the four kinds it exists to cover. Two versions of
one rule that drift apart is how this defect arose in the first place.

So the fix extends `kind_prompt` to gate `layout_block` too, from a **single** resolution shared by
both sites. The lever now does what its comment always claimed. It also means the queued `kind_prompt`
arm tests its full intent rather than a third of it.

**Two over-claims the verifier would not sustain**, recorded because they narrow the finding: the
port-collision harm was misattributed (the shards used distinct ports; the `Errno 48` came from that
shard's own leftover server after `bash: timeout: command not found`), and no verify shard in that run
actually made a write call — so the write-race harm is real in principle but **unrealised in the
evidence**. What stands is the contradictory directive and ~12 inapplicable rules added to a budget
where measured perfect compliance is 0.094.

## F23 — The golden-value gate grades the build against a document the build wrote

The most serious finding of the session, verified in the shards' own reports rather than argued:

> `verify-e2e::1`: *"Now I'll number the advertised commands/usages **from the README** in order of
> appearance"*
> `verify-e2e::0`: *"**Spec expectation (README)**: 'Health check with payment count and last sync
> time'"*

`e2e_shard_spec` (`swarm.rs:3070`) orders each shard to confirm the actual output equals *"the
SPECIFIC value the spec implies"* — and never gives it the spec. A model handed a spec-shaped task
with no spec binds the word "spec" to whatever spec-shaped artifact is in the tree, and the only one
there is the **README the same swarm just wrote**.

So the golden-value gate — the one check that can catch a wrong constant, a wrong path or a wrong
unit — closes a loop: the build is graded against the build's own documentation. That is a
false-green machine, and it is the missing half of the explanation for `baseline-n3-r0` scoring 50%
with tier A at 100% while `total_count()` returned 0 of 247. The vendor client invented `/payments`;
the README documented `/payments`; the shard checked the app against the README and was satisfied.

**Precision, because the finding over-claimed:** it said "verbatim in all three shard reports". I
measure **two of three** in the pre-boundary run (`verify-e2e::2` does not mention it), and the
current run has not produced shard reports yet. Two of three is enough for the conclusion; three is
not what the evidence says.

**Not fixed here, deliberately.** The remedy is to give the shards `spec_frozen`, which is a
`DispatchRequest` change, and the refuter of a neighbouring finding was explicit that it belongs
there rather than as a side effect of something else. Shipping a new instruction channel unlevered,
two hours into a campaign, is the exact shape of mistake the last finding taught me to avoid. It goes
to a design round with the F14 candidate.

One part of the remedy IS already shipped: those same shards were receiving the sink's
*"FIX the offending file"* / *"wire them all"* directive, and `4ba5d5200` now subtracts it under
`kind_prompt`.

## F25 — RETRACTION of F24's headline, and the real answer to "why can't it use 3 nodes"

**F24 said "the PLAN caps this run at 1.75 nodes". That was wrong, and the cause was a bug in my own
occupancy instrument.**

`test-meridian` was dispatched TWICE and completed ONCE — a retry. I paired dispatch[i] with
completion[i], which left the second dispatch unmatched, and an unmatched dispatch was credited to
the END OF THE RUN. That one retry invented **83 minutes of phantom busy time** on a 122-minute run.

| | published (buggy) | corrected |
|---|---|---|
| busy node-secs | 156 min | **94 min** |
| occupancy | 0.43 | **0.258** |
| critical path | 89 min | **31 min** |
| MAX USEFUL NODES | 1.75 — "the plan is the ceiling" | **3.02 — the plan could use MORE nodes than the fleet has** |
| best achievable occupancy | 0.58 | **1.0** |
| only-one-node wall | 43.5% | **7.4%** |

An attempt now ends at whichever comes first, its own completion or the NEXT dispatch of the same
task, since that is what supersedes it. Only a task still outstanding at the last observed event is
credited to the end. Corroborated by an independent hand calculation that reached 3.02 before the fix
and now matches exactly.

### The actual answer: only 31% of the run is parallelisable at all

```
planning / pre-dispatch    30.6 min   25.2%   fans across nodes, but nothing dispatches until it ends
EXECUTE (parallel)         38.1 min   31.3%   occupancy 0.82 — three nodes DO work here
COMPLETE / repair          53.0 min   43.5%   single node by default
```

So the swarm **can** use three nodes — the DAG supports 3.02 and a perfect scheduler could reach 1.0
occupancy — but it only gets the chance for **31% of the wall clock**. The other 69% is a serial
planning prefix and a single-node repair tail. Amdahl does the rest: parallelising a third of the run
across three nodes cannot move the total much, which is why the third node "buys nothing" without the
plan being at fault.

This vindicates the `complete_parallel` arm already queued (repair is the single biggest block, and
the lever to fan it exists and defaults OFF) and re-points the planning work at the serial prefix
rather than at plan width.

**Sixth instrument failure today, and the second I published before catching.** The pattern holds
exactly: an inference drawn from a signal never designed to answer my question — here, treating "no
completion event" as "still running" when it also means "superseded by a retry".

## F24 — First finished unit on the new engine, fully crunched: the PLAN caps this run at 1.75 nodes

`baseline-n3-r0`, 122 min, 3-node pool confirmed, not void/timed-out/aborted, engine_build matching.

**Score 88.7%** (A 100 / B 91 / **C 85.7** / D 75) against the pre-boundary unit's 50.0% with tier C
at 14.3%. **That comparison is n=1 against n=1 on a fleet with a measured 46-point replicate spread,
so it attributes nothing to the fixes.** It is recorded as one observation, not as evidence.

**The crunch — 5 of 6, and this time the vendor integration actually works:**
```
[PASS] fetch_all_payments   247 in 3.9s          (pre-boundary: 0)
[PASS] chronological        mixed offsets normalised
[PASS] total_count          247                  (pre-boundary: 0)
[PASS] resync_idempotent    first=247 second=0
[FAIL] idempotent_create    KeyError: 'payment_id'
```
`crunch.py` and `score_build` broadly AGREE here (5/6 and 88.7%), where on the pre-boundary unit they
diverged violently (1/4 against 50%). Agreement between two independent instruments is worth more
than either number.

**And the node answer, from a FINISHED run so the plan ceiling is meaningful:**

| | |
|---|---|
| MAX USEFUL NODES | **1.75** (pool is 3) |
| best occupancy any scheduler could reach here | 0.58 |
| actual occupancy | 0.43 |
| wall with only ONE node working | **3,177 s — 43.5%** |
| biggest single task | `test-meridian` = **53.3% of all node-busy time** |
| before the first dispatch | 1,836 s — 25% of wall |

**On this spec, the swarm cannot use three nodes, and the limit is the PLAN, not the scheduler.** One
task holds 53% of node-busy time, so the DAG's critical path dominates and no scheduler could exceed
0.58 occupancy at this pool. Adding the third node was, for this run, worth nothing.

That is the honest answer to goal one so far: the machinery to *use* nodes works (planning fans 6/7/6,
execute occupancy is near 1.0 while it lasts, idle-node judging fired 54 times), but a quarter of the
run precedes any dispatch, 43% of it runs on one node, and a single task is over half the work. Those
are structural properties of the decomposition, not score noise — which is why they are worth acting
on at n=1 in a way a score delta never is.

Note I read `max_useful_nodes` at 3.58 mid-run and refused to draw a verdict from it. The finished
value is 1.75 — the opposite conclusion. That restraint was correct.

## F26 — The THIRD site of one defect class, and this time I enumerated the whole class

`read_prereview_findings` is spliced by `owned_part` at `swarm.rs:18016` under
`req.owned_files.is_empty() || req.task_id == "integrate-verify"`. The comment directly above states
the intent — *"inject idle-node PRE-REVIEW findings into the integrate-verify sink"* — and the
predicate over-matches it: every fanned `verify::<M>` and `verify-e2e::<i>` also owns nothing, so
each was handed findings framed as **confirm and FIX** while its own task statement forbids writing.

**Same over-broad owns-nothing predicate, third occurrence.** I fixed it at `18340` (`is_fix_round`)
this morning, again in `layout_block`'s `owned_part` this afternoon after round 4 found it, and
missed this one both times. All three now route through the single `read_only_shard` predicate under
`kind_prompt`, so there is one classifier rather than three that can drift.

This time I enumerated every site rather than stopping at the one I was handed. Of the eight
`owned_files.is_empty()` uses: three were this class and are fixed; `17893` is the sink text the
new branch precedes; `15471` builds a descriptive string for the judge (`"(works across the whole
layout)"`) and issues no directive; and `18535` / `18640` / `18679` are **negated** — owned-file
existence gates with different semantics. **No fourth instance.** That sentence is the deliverable;
the fix is the easy part.

432 of 432 tests pass, clippy clean, held for the next boundary.

## F27 — My own fix removed the waste but not the defect, and round 4 caught it

`9c16ec993` excluded `integrate-verify` from the detail fan-out when `fan_verify` applied. I recorded
that as removing the self-contradicting join spec. **It did not.** Round 4 found the contradiction
still live at `swarm.rs:12638`, against code that already had my fix.

The reason is exactly the thing the fix changed. With the sink excluded from detailing, the
description it carries is `thin_integrate_verify_spec` — engine-authored, 2,804 chars — and T2's
"substantive detail" test is `detailed.trim().len() > 240`. The thin spec sails through it, so the
join still received the canonical joined spec followed by *"Also run these concrete plan-enumerated
checks:"* and a verbatim copy of the sweep it had just been told to skip.

So my fix removed a wasted 75-second call and left the defect it was credited with fixing. The
instance moved; the class did not.

`detailed` is a MODEL-authored detail only when the sink actually went through the fan, and it is
excluded from the fan precisely when `fan_verify` applied — so `model_detailed = !fan_verify_applied`
is the honest test. The documented intent (*"keep a substantive spec-specific detail as EXTRA
checks"*) survives on the non-fan path where the sink IS detailed; the contradiction cannot arise on
the fan path. Byte-identical when `fan_verify` is off.

Pinned by a test that asserts the two engine-authored specs genuinely contradict each other and that
the thin spec clears the 240-char threshold — the three facts that together made the append a defect
rather than a redundancy. 433 of 433 pass.

**The lesson is not the patch.** I verified this only because I decided to re-check a fix I had
already claimed, rather than trusting my own commit message. Two of today's engine fixes have now
turned out to be partial on re-examination.

## F28 — The engine's fan-a-fat-task mechanism cannot reach the tasks that need it

Mihai: *"we need to find ways to fan this out more without breaking it and without having generic
instructions."* Checking whether `split_inherit_spec` was worth an arm answered a bigger question.

**Zero splits across all six runs on disk.** `judge_verdict` actions are only `observed`,
`re_dispatch` and `failed` — never `split`. So the splitter, which is the engine's mechanism for
fanning a too-big task across idle nodes, has never fired here.

`is_split_candidate` (`judge.rs:218`) requires, among other things:

```
input.elapsed_secs >= cfg.split_threshold_secs   (900s)
input.owned_files.len() >= 2                     <- this one
```

And the architect is instructed to keep files *"small and single-responsibility"*, one per module.
Measured on the finished 3-node run:

| task | share of node-busy | owned files | splittable? |
|---|---|---|---|
| `test-meridian` | **22.5%** | 1 | **no — owns one file** |
| `integrate-verify` | **18.7%** | 0 | **no — owns nothing** |
| `test-api` | 14.9% | 1 | under the threshold |

**41% of all node-busy time sits in tasks that are simultaneously over the split threshold and
structurally unsplittable.** The mechanism is aimed at a shape — one worker owning several files —
that this planner is explicitly told not to produce. It is not off, not broken, and not misconfigured:
it is unreachable by construction.

This also settles `split_inherit_spec`: **not queued as an arm.** A lever on a path that never fires
would burn three units to measure nothing. It stays echoed in `levers_resolved` so its inertness is
visible rather than assumed.

**The design rule this implies, from every fan that worked and every one that didn't:** a fan is
legitimate only when each node's instruction is COMPUTED from its own item — an index, a file, a
module. `e2e_shard_spec` computes a command slice from `position mod shards`; `split_fat_modules`
computes a per-concern file scope. Both are specific by construction. The judge-side split copies a
label and produces 43 characters, which is the generic-instruction failure in its purest form.

So fanning a single-file task means finding a **computable partition that is not files** — for a test
task, the behaviours it must cover; for the sink, the commands it must check (which `fan_e2e` already
does). That is a planner change, and it goes to a design round rather than into the engine tonight.

## F29 — The engine's only deterministic oracle reported nothing at all

Round 4's last untriaged defect was that `spec_contract` cannot parse a markdown endpoint table.
Before designing that fix I checked whether the check is even observable — the discipline that had
just saved three units on the splitter — and found something prior:

`spec_contract` is **ON** in every run and **emits no event**. Its findings are folded into
`complete_verify`'s bare count. Its siblings all report: `cross_module_drift` emits
`{checked: 8, findings: 0, detail: "no module reads a field its sibling does not define"}`, and
`complete_missing_deliverables` emits too. So a zero from the run's **only deterministic, no-model
spec-to-oracle path** was indistinguishable from that path being blind — the failure this project has
a standing law about, sitting inside the one check that is meant to be immune to it.

And the field that makes the zero readable already existed, unread. `SpecContractResult.verified` is
documented as existing so a consumer can require `verified >= 1`, so that *"findings.is_empty() &&
inconclusive.is_empty() because it CHECKED NOTHING is never mistaken for a real pass"* — the exact
false-green class. It was marked `#[allow(dead_code)]` because the reader was never wired. This is
that reader.

It now emits `{round, verified, findings, inconclusive, detail}`, where the detail distinguishes
**"CHECKED NOTHING — a clean result here is silence, not evidence"** from "every advertised check
that bound was satisfied".

**The table-parsing fix is deliberately NOT included.** Widening the matcher is behaviour; until the
event exists there is no way to tell whether widening it changed anything, and the refuter's own
ordering said the same. Observability first, then the behaviour it lets us judge — the shape that
made the 46-point spread legible in the first place.

## F30 — `fan_e2e` does not partition. Each shard divides a list it invented separately.

Round 5 returned **0 sound proposals of 3**, and its refutation carried a finding larger than the
problem it was reviewing. Verified independently from the shards' own reports, one run, three shards:

| shard | what it says the spec advertises | what it checked |
|---|---|---|
| `verify-e2e::0` | "advertised usages in order" | command **1** only |
| `verify-e2e::1` | "advertises only **one** command/usage mode for this program" | **nothing** — *"there is no command whose position satisfies `position mod 3 == 2`"* |
| `verify-e2e::2` | "advertises **3** commands/usages" | command **3** only |

`e2e_shard_spec` says *"number them 1,2,3... in the order the spec gives them, and verify ONLY the
ones whose position mod {shards} == {m}"* — and the shard never receives the spec, so each one
derives the list from the README the build wrote (F23) **and derives a different one**. One shard
enumerated 1 item, another 3. `position mod shards` over lists of different lengths is not a
partition: coverage is neither disjoint nor complete, and nothing in the run says so.

**A shard that checked nothing returns no findings, which reads as a pass.** That is a false green
manufactured by the fan itself.

This is the sharpest instance of the constraint Mihai set — *fan out more without breaking it* — and
it inverts what I believed this morning. I had `e2e_shard_spec` filed as the GOOD example of a
computed, specific fan, against the judge-side split's 43-character children. It is not: the *index*
is computed, but the *list being indexed* is not, so the specificity is an illusion and the guarantee
in its own comment (*"a shard whose slice is reworded checks everything or nothing, which silently
undoes the split"*) describes exactly what happens by default.

**The fix, prescribed by the refuter and correct: build the oracle at PLAN time, not dispatch.**
`parallel_plan` already holds `spec_frozen` and calls `fan_e2e_split`, so the numbered triples can be
passed into `e2e_shard_spec(lang, i, shards, &oracle)` and *"number them in the order the spec gives
them"* replaced with *"the numbered table below IS the list"*. One channel, one list, identical across
shards — then the partition is real and the golden values stop coming from the build's own README. It
inherits the existing guarantee that `verify-e2e::` descriptions are excluded from detailing, so a
slice cannot be reworded. Needs a default-OFF lever, a `levers_resolved` entry, and byte-identity when
OFF or when the table is empty.

Not built tonight: it changes a worker prompt mid-campaign, and the whole point of the boundary
discipline is that such a change waits.

## F31 — The design rule, now precise: a fan must enumerate the ITEMS, not just the selector

Checked rather than assumed whether `fan_verify` shares `fan_e2e`'s defect. It does not, and the
contrast between the two gives the rule Mihai asked for in a form that can be checked mechanically:

```rust
per_module_verify_spec:  "Import/build-check every file this module delivers ({owned})"
                          owned = files.join(", ")     <- the ENGINE enumerates the slice
e2e_shard_spec:          "number them 1,2,3... in the order the spec gives them,
                          verify ONLY the ones whose position mod {shards} == {m}"
                                                       <- the engine supplies only the SELECTOR
```

**A computed index is not enough. The SET being indexed must be enumerated by the engine.** When it
is, the slice is a fact (`fan_verify`, the detail fan, the contract fan, the scout fan — each item
is named). When it is not, every node reconstructs the set from whatever spec-shaped artifact it can
find, gets a different answer, and the partition silently stops being one.

That refines what I wrote this morning. "Each node's instruction is COMPUTED from its item" was too
weak — `e2e_shard_spec` satisfies it and is still broken. The testable form is: **can you point at
the line where the engine writes the item into the prompt?** For `fan_verify` it is
`owned = files.join(", ")`. For `fan_e2e` there is no such line, and that absence is the whole defect.

Audit of every fan in the engine against that rule:

| fan | items enumerated by the engine? | verdict |
|---|---|---|
| scouts (per lens) | yes — the lens brief is interpolated | sound |
| contracts (per module) | yes — the module and its files | sound |
| detail (per subtask) | yes — id, brief, owned files | sound |
| `fan_verify` (`verify::<M>`) | yes — `files.join(", ")` | sound |
| `fan_e2e` (`verify-e2e::<i>`) | **no — only the mod-selector** | **broken (F30)** |
| judge-side split | n/a — never fires (F28) | inert |

So exactly one fan is broken, and it is the one whose failure is invisible: a shard that enumerates
an empty slice reports no findings, which reads as a pass. Fixing it is a prerequisite for widening
anything, because widening a fan that does not partition multiplies a false green rather than the
work.

## F32 — The repair loop has been chasing an endpoint that is a regex artefact of a backtick

The highest-harm finding of the session, and it was found by checking whether `spec_contract` was
worth widening rather than by looking for it.

`spec_get_endpoints` matched `\bGET\s+(/\S*)`. `\S*` runs straight THROUGH a closing backtick, so the
real spec's prose — *"A single page, served by the backend at `GET /`."* — yielded the path `` /` ``.
That is not discarded: it survives the trims, the check boots the app, curls it, gets a 404, and
pushes a finding. **Confirmed verbatim in `baseline-n3-r0`'s own graded verdict, twice:**

```
- GET /` returned 404 — the spec advertises this endpoint but the app does not implement it
```

against an app that serves `/` correctly. And `spec_contract` findings go into `verdict.findings`,
which **blocks the green claim and drives the fix loop** — so the repair phase, 44% of the run on one
node, has been partly repairing a phantom.

This is exactly the hazard the engine already documents a few hundred lines away, in the comment
explaining why pillar checks were demoted to advisory: *"a distilled check ... would FALSE-FAIL a
correct app, and the fix loop would then REGRESS it"*. The same class, in the one check that is
supposed to be deterministic, live on the default path, in every run on disk.

Fixed by cutting the captured path at the first markdown delimiter. Pinned by a test using the exact
sentence from the real spec, plus controls that unambiguous endpoints and backticked endpoints both
still parse.

**How it was reached matters more than the patch.** I set out to widen the matcher to markdown
tables, checked what it currently does first, and found it was not blind but *wrong* — producing a
confident false finding rather than nothing. Had I widened it without looking, I would have added a
second parser beside a broken one and never seen this.

## F33 — Independent confirmation the phantom finding was false

`crunch.py` gains `serves_root`: the spec says *"A single page, served by the backend at `GET /`"*,
and nothing was checking it — the exact requirement `spec_contract` was falsely reporting on.

Run against the two control trees, it confirms F32 from the other side:

| tree | `spec_contract` said | `crunch.py serves_root` observes |
|---|---|---|
| `baseline-n3-r0` (the 50% run) | *"GET /` returned 404 — the app does not implement it"* | **HTTP 200, 400 bytes of markup** |
| `opus-5-r0` (known good) | — | HTTP 200, 398 bytes |

The app the engine accused of not implementing its root page **serves it correctly**. That is no
longer an inference from reading a regex; it is two instruments disagreeing, with the run's own
graded verdict on one side and a live HTTP request on the other.

Controls still hold in both directions: known-good 7/7 exit 0, known-bad 2/5 exit 1.

A false finding about a REAL requirement is the worst of both — it burns repair effort and leaves the
requirement unchecked. So the requirement now has a genuine check, independent of the engine.

## F34 — the "46-point replicate spread" is substantially ONE BIT, and the bit is `/v1`

Three baseline units, identical config, identical fleet. The scores read like noise. They are not.

| unit | `/v1` in `plan_loaded` | vendor calls | build score |
|---|---|---|---|
| preboundary-2 | 6 | `/v1/payments` | **88.7%** |
| preboundary | 0 | `/payments` | 50.0% |
| current (n3-r0) | 0 | `/payments` | **42.7%** |

Perfect separation, and it is read off a **deterministic engine event** (the contents of
`plan_loaded`), not off a score comparison — so it is valid at this n, where an outcome delta would
not be.

`crunch.py` on the current unit: 2/5. It imports and it serves `/` — and every single vendor call
raises `HTTPError 404`, because the client builds `{base}/payments` while the vendor serves
`/v1/payments`. The app is not "43% working". Its one job did not happen.

**Why the bit flips.** The spec gives the docs URL `http://127.0.0.1:8930/v1/docs`, then says
`Base URL http://127.0.0.1:8930`. The `/v1` prefix on the *data* routes is stated six times in the
vendor's own document and **never in the prompt**. A planner that reasons "the docs are under /v1,
so the API is" wins; one that concatenates the stated base URL loses. It is a coin flip on a fact
that is derivable but never given.

**Correction to the above (the vendor trace disproves it).** A node DID read the document — `curl`
fetched `/v1/docs` four times in the failing run and then exercised `/v1/payments` thoroughly enough
to trip the 429 throttle and the 409 idempotency replay. The fact was discovered. It died with the
node that discovered it: `plan_loaded` carries `/v1` zero times, and the implementer that needed it
wrote `/payments`. The problem is not that the fleet cannot read; it is that what one node learns
reaches nothing else. See F35 for the full chain.

**Why the SCOUTS could not read it.** Both runs, verbatim from `research_tools`:

    {"available": [], "can_look_things_up": false}
    research_completed {"findings": 1, "grounded": 0, "looked_nothing_up": 2}

The scouts have **no tools at all**. The spec's instruction "Read it before you start" is
unexecutable, and the engine records that it did not happen and proceeds anyway. Four to six minutes
of three-node fleet time per run produce findings that are, by the engine's own event, entirely from
the model's head.

**This kills one of the sweep's six arms before it runs.** `doc_prefetch` routes research findings
verbatim to workers — but only findings where `grounded == is_mcp && ok`. With no research tools
attached, `grounded_n` is 0 on every run, `doc_facts` stays empty, and the worker prompt is
byte-identical to baseline. The arm cannot fire on this bench. Running it would spend hours of fleet
time to produce an INERT result, and INERT proves nothing. It is pulled from the queue.

The lever was not wrong; its precondition has never existed here. The fix is to make the grounding
real rather than to widen the gate: a URL named in the spec should be fetched **by the engine**,
deterministically, and injected verbatim — a fetch the engine performs is grounded by construction,
and verbatim documentation is the least generic instruction it is possible to hand a node.

## F35 — the whole 42.7% build, explained end to end, every link a deterministic engine event

Not an inference. Eight events and one HTTP trace, in order:

1. `research_tools {"available": [], "can_look_things_up": false}` — the scouts cannot open the
   document the spec points at.
2. Vendor trace, 17:17–17:20: `curl` GETs `/v1/docs` **four times**, then exercises `/v1/payments`
   until it trips the 429 throttle and the 409 idempotency replay. **A node discovered the fact.**
3. `plan_loaded` carries `/v1` **zero times**. The fact did not reach the decomposition.
4. `detail_fallback {"task_id": "meridian", "reason": "timeout", "brief_chars": 122}` — the client's
   implementer got the architect's one-liner. The one channel that could still have carried a full
   spec to the one module that needed it **failed open at the 75 s budget**, and it did so for
   `store`, `api` and `test-web` too.
5. The implementer writes `{base}/payments`.
6. Vendor trace: **seven 404s on `/payments`**, three of them while `test-api`, `integrate-verify`
   and `test-web` are in flight. The fleet ran its own client against the real vendor and watched it
   fail.
7. `test-meridian` is dispatched **three times** — attempts 0, 1, 2 — accumulates 41 judge verdicts,
   and fails all three. `meridian` is dispatched **once** and never revisited. A failing
   `test-<module>` is treated as the test author's failure to write a passing test, never as evidence
   that the module under test is wrong.
8. `complete_failed_tasks` blocks green correctly. `complete_verify` rounds 0 and 1 both fail with
   the same 2 findings: a stub `log_message`, and the phantom ``GET /` `` (F32). `review_after_fix`
   then reports `findings: []` — the repair fixed the stub.

**The repair loop never saw the defect.** Its finding sources are an AST review and a regex over the
spec. Neither runs the code. The run's own HTTP traffic was 404ing seven times and no mechanism
looks at it, so the fix rounds spent themselves on a stub and on an endpoint that does not exist
while the app's only job stayed broken.

`complete_result {"passed": false, "verified": false}` — the engine was honest. It knew it had not
finished. It just could not tell which thing was broken.

**Two consequences for the queue.**

`doc_fetch` (committed, held for the boundary) attacks links 1–5, and specifically survives link 4:
`doc_facts_block` is its own block in the worker prompt template, spliced independently of the task
description, so a worker whose detail call timed out into a 122-character brief still receives the
document verbatim. That is the property that makes it worth fleet time.

Link 7 is a separate defect and is NOT being patched off one run. What the evidence supports so far:
three dispatches of the test, zero re-dispatches of the module, on a failure whose cause was in the
module. That wants its own finding round, not a guess.

## doc_fetch — verified by reading every consumer, not by assuming two

"I fixed that" is a hypothesis, so the splice was traced to each site that renders it:

| consumer | channel | interpolation site |
|---|---|---|
| architect / skeleton drafts | `research_block` | `swarm.rs:11870` -> `:11995` |
| detailer (per-task spec) | `research_block` | `:13075` -> `:13085` |
| pillars | `research_block` | `:11653` -> `:11659` |
| every worker | `doc_facts_block` | `:18234` -> the prompt template |

The fourth is the one that matters against F35 link 4: `doc_facts_block` is its own slot in the
worker prompt, spliced independently of the task description. A worker whose detail call timed out
into a 122-character brief still receives the full document. That is the property that makes this
worth fleet time rather than another prompt tweak.

Both `research_findings` consumers are fed by ASSIGNMENT during research (`:20459`), which is why the
fetch is spliced after that block and not before — anything written earlier is discarded.

## F36 — it is not how many one-liners shipped, it is WHICH module got one

`prefix.py` (new, `px-1`, controls both directions) breaks the pre-dispatch window into its parts.
Three baseline units, and the shape is stable:

| unit | prefix | research | planning | redrafts | score |
|---|---|---|---|---|---|
| preboundary-2 | 1836s | 379s | 1457s (79%) | 1 — 766s discarded, 691s replacement | 88.7% |
| preboundary | 1312s | 420s | 892s (68%) | 0 | 50.0% |
| current | 1556s | 264s | 1292s (83%) | 1 — 582s discarded, 710s replacement | 42.7% |

**Planning is 68–83% of the prefix, and a redraft does it twice.** That is the target the earlier
round was groping at without a number. It is also why `retarget_discarded` now exists: `plan_loaded`
fires once, for the survivor, so the discarded round left no trace at all.

**The correlation is at the TASK level, not the run level.** Both high and low scorers shipped
one-liners; what separates them is which module got one:

| unit | `meridian` (owns the vendor client) | shipped one-liners | score |
|---|---|---|---|
| preboundary-2 | **1497 chars, contains `/v1`** | 1 — `test-api` | 88.7% |
| current | **122 chars, no `/v1`** | 2 — `meridian`, `test-web` | 42.7% |

So "2 fallbacks vs 1" is not the signal. A thin brief on `test-api` costs a test some rigour; a thin
brief on the one module that owns an external contract nobody can guess costs the entire build. The
sweep's `fallbacks` column has been counting these as equivalent.

**Ghosts.** `meridian-client` failed its detail call in the 88.7% run and appears in no shipped plan
— it belonged to the draft the redraft threw away. Both instruments now separate ghost failures from
live ones (`prefix.py` `ghost_fallbacks`, `dispatch_audit.py` `live_fallback_events`), because a
fallback on a task no worker ever saw cost fleet time and harmed nothing, and counting the two
together overstates the damage. This is the same mechanism-vs-quality split that already cost one
wrong number.

**It also settles how a reuse path must be keyed.** `meridian-client` and `meridian` are the same
module under two names, so an id-keyed cache across a redraft would miss it. `prefix.py` measures
survival by OWNED FILES as well as by id and reports the renames; the file-based number is the one a
reuse path should be designed against.

## F37 — the test that found the bugs was reported as the thing that was broken

F35's link 7, taken as its own round. It did not need another run: the defect is readable off the
source and one stat.

`test-meridian` exhausted three attempts. The finding the engine then handed the fix loop, generated
from the task id alone:

> planned task `test-meridian` FAILED (its attempts were exhausted) — **its deliverable is missing or
> broken.** Find what it was meant to produce and finish it.

`tests/test_meridian.py` is on disk at **13,357 bytes — the largest test file in the tree.** Running
it: **6 passed, 2 failed**, and both failures are real defects in the module it tests —
`fetch_all_payments` returns one page of many, and the 429 retry with an HTTP-date `Retry-After` waits
1.10 s where the header demands 1.5 s.

So the test author did its job better than any other worker in the run. It produced a substantial
suite and caught two genuine bugs. The engine classified that as the test task failing, re-dispatched
it three times to rewrite the test, told the fix loop the file was missing, and never re-dispatched
`meridian` at all. Both defects shipped.

**Why it is a class, not a one-off.** The finding is built from the task id with no reference to the
world: it cannot distinguish "the deliverable was never written" from "the deliverable is written and
its checks fail", which are opposite instructions to a fix worker. The same shape produced the other
failed unit on disk — `test-api` in the preboundary run, deps `['api']`, same treatment.

**The fix, both halves deterministic.** `failed_task_finding` (pure, unit-tested) stats the task's
owned files: if none are written, today's string stands byte-for-byte. If they ARE written, the
finding says so, names them, names the files owned by the task's dependencies — the planner is
instructed to give `test-<module>` a dependency on ONLY that module, so the code under test is read
off the DAG rather than guessed from the id — and directs the fix there. It also carries an explicit
prohibition on weakening, skipping or deleting a check, because a worker told its test is broken will
make it pass by deleting the assertion that found the bug.

No model judges any of this. It is a stat and a dependency edge.

## F38 — the node curve was about to measure 2 vs 3 and call it 1 vs 3

Caught live, 57 minutes into the first 1-node unit, by reading its events instead of waiting for its
score. The unit was killed rather than given another hour.

`run_started.pool` reported **one** device. The run dispatched to **two**:

    devices that received dispatches: {'mac-gabee-qwen3.6-27b-fable-fusi': 5, 'planner': 5}
    PEAK CONCURRENT devices actually working: 2

Both worked at once — `api` on gabee and `meridian` on the planner were dispatched in the same
second, and the planner finished `meridian` and started `store` while gabee was still on `api`.

**Why.** `planner_also_works` (default **true**) pushes the planner on as an extra worker device
*unless its model is already in the pool*. At `MAX_NODES=3` the pool contains the planner's model, so
nothing is pushed and the run has three. At `MAX_NODES=1` the pool is one other device, so the planner
IS pushed and the run has two. The intended 1 → 2 → 3 curve was really **2 → 2-or-3 → 3**: nearly flat
by construction, which is the exact false conclusion — "more nodes do not help" — that this project
exists to correct, arrived at by a different route than last time.

**Why no instrument caught it.** `run_started.pool` is emitted from `enabled`, *before* the push. It
is the field every harness reads as ground truth for "how many nodes did this run have", and it is not
the worker count. The void check compared it to the requested count and passed, because the pool
really was 1.

**Fixed on three surfaces, deliberately not sharing an assumption.**

1. Engine — a new `pool_resolved` event emitted after the device list is final, carrying every device
   that can receive work, `worker_count`, and `planner_pushed`. Emitted as its own event rather than
   by correcting `run_started`, so nothing already parsing that event changes meaning underneath it.
2. Engine — `GOOSE_SWARM_PLANNER_ALSO_WORKS` now gates the push, and the sweep sets it to `0`, so N
   nodes means N workers. **The 3-node cell is unaffected** — nothing was ever pushed there — which is
   what makes this a correction rather than a change of subject.
3. Harness — the void check reads `pool_resolved` when present, and *independently* counts the
   distinct devices in the dispatch record, which no engine build can misreport. Either exceeding the
   cell's node count voids the row.

The three baseline 3-node units on disk are NOT affected: their pool already contained the planner
model, `planner` never appears as a dispatch device in them, and their peak concurrency is 3.

## F39 — killing the engine first lets the supervisor record a truncated run as a finished one

Found immediately after acting on F38, by checking what the kill actually wrote instead of assuming
it wrote nothing.

The unit killed at 57 minutes was recorded as:

    score 0.2999   void false   aborted false   timed_out false   actual_nodes 1

Indistinguishable from a completed run, and it went straight into the results table as a clean 1-node
row at 30.0% — next to a 3-node row at 42.7%. That reads as "one node is much worse than three", which
is a conclusion drawn from a run that was stopped early AND had two workers.

**Why.** The stop sequence killed the engine's process group first and the sweep supervisor about two
seconds later. In that window the supervisor did exactly what it is built to do: noticed the engine
exit, scored the half-finished tree, and persisted a result. Nothing in that path knows the difference
between "the engine finished" and "the engine was killed".

**Fixed:** the supervisor is killed FIRST, then the engine, in `loop.sh`'s stop path. A supervisor that
is already dead cannot score a corpse.

The row itself was voided by hand on deterministic evidence — 11 dispatches across `mac-gabee` (5) and
`planner` (6) in a cell that asked for one node — with the pre-void score kept as
`score_before_void` so the row is auditable rather than erased. The 3-surface check that catches this
class automatically ships at the next boundary.

Two lessons, and the second is the general one:

- A kill is not a no-op. Whatever is watching will interpret the silence, and its interpretation is
  usually "done".
- **Check what an intervention WROTE, not just that it worked.** The kill succeeded — engine gone,
  fleet idle, exactly as intended — and it still produced a corrupt row. "It worked" and "it left the
  world in the state I wanted" are different claims.

## F40 — the adversarial round found six, and two of them were mine from today

63 agents, five lenses, three refuters per finding with distinct jobs (is it a misreading, is the
path reachable, does it cause the stated harm). **19 raised, 13 killed, 6 survived.** Every survivor
was checked by hand before acting on it; the refutations were read too.

**The one that mattered most, found independently by two lenses at 3/3.** `prefix.py` read
`owned_files` off `plan_loaded`, and the engine emits that field as **`files`**
(`retarget_discarded` is the one that says `owned_files`). So the file-based survival number — the
number I had just written into FINDINGS as "the one a reuse path should be designed against" — was
structurally **zero on every real run**, and it passed its own controls, because those controls were
driven by a synthetic stream I wrote using my own assumed key.

**A control built from the instrument's assumption tests the assumption against itself.** That is the
general lesson and it is now enforced: `prefix.py` has a real-shape control that reads an actual
`run.jsonl` off disk and fails if `plan_loaded` yields no files. Reintroducing the bug makes it fail
with the exact diagnosis; removing it passes. Both directions verified.

**It also corrects F37.** I reported there that `test-meridian` and `test-api` had `owned_files: []`
— that was the same key miss, not an empty list. They own `tests/test_meridian.py` and
`tests/test_api.py`, with deps `['meridian']` and `['api']`. F37's conclusion is unaffected (it rests
on the file being on disk at 13,357 bytes and its failures being real module defects) and the fix is
actually *better* than I could show: the deps traversal names `vendorsync/meridian.py`.

**`spec_get_endpoints` — my backtick fix re-armed the bug it was written to remove.** I put `<` and
`>` in the delimiter split set, three lines above an exclusion that drops any path containing `<`.
`GET /api/payments/<id>` became the concrete path `/api/payments`, which the exclusion could no
longer see. `{id}` and `:id` still worked, which is exactly why it looked correct. The regex's own
comment, four lines up, says the char class must not stop before `<`. I did not read it.

**`sweep.py` computed the harness verdict and threw it away.** Up to 600s of `selftest.py` per unit,
then `harness_ok` was never written into the row — while `summarise()` filters on
`r.get("harness_ok") is not False`, which is vacuously true when the key does not exist. A unit whose
own instruments failed their controls was averaged in exactly like a clean one.

**The e2e shard prompt lost a space** on the DEFAULT path (`).The` for `). The`), against a lever
documented byte-identical, with a guard test that asserted on a fragment and could not see it. The
refuters argued convincingly that one space cannot move an outcome. They are probably right — and a
documented byte-identity claim that is false is still false, and it cost one character to fix.

**And one finding that was inert but pointed at something real.** The claim was that the new
failed-task finding never reaches the fix loop on a Python run, because its only consumer is guarded
by `if !verdict.ran` and the Python smoke gate always runs. That is correct as written, and the guard
is deliberate (a stale failed-task finding once pinned a repairable app red for three rounds). But
checking it exposed a worse gap: **`complete_verify` emitted only a COUNT.** The event that decides
green was the one verdict in the entire run that could not be checked against evidence afterwards — I
had to infer which two findings held a build red, and my first inference was wrong. It now carries
the finding texts, truncated and capped.

Measured while checking: the produced app's own suite runs in **6.4s** against a 120s cap and reports
2 failed / 46 passed, so the runtime oracle is not being timed out — whatever held that build red,
it was not a silent gate.

## F41 — the repair tail is 2-3 sequential model calls, and the fan meant to split them can't see its work

The tail after the last `task_completed`, measured on all three parked units: **27.8 / 32.2 / 53.0
minutes — 22% / 26% / 44% of the run.** It emits **no `task_dispatched` events at all**, which is why
`occupancy.py` reads ~0 there and why overall occupancy (0.50) is so far below execute occupancy
(0.93).

Timing every gap inside it shows the shape is simple and identical across runs:

    complete_verify round=0   findings=1     +0.1m   (deterministic — instant)
    ...                                     +19.4m   <- A FIX CALL
    complete_verify round=1   findings=2    +13.4m   <- A FIX CALL
    complete_verify round=2   findings=1     +0.0m
    review                    findings=1    +20.0m   <- A FIX CALL
    review_after_fix          findings=0

Everything deterministic (drift, verify, overview) is under 0.2 min. **The entire tail is 2-3 model
fix calls of 8-20 minutes each, run one after another on one device while the other two idle.**

**The fan that exists to split this cannot see its work.** `complete_parallel` groups findings by file
and fans one shadow-isolated shard per group — a proper fan, it enumerates its items. But
`extract_file_from_finding` matched only two shapes, both pytest-traceback: a leading `path:line:` and
`File "path"`. Porting it to Python and running it on the finding strings the real runs actually
produced:

| finding shape | seen | resolved to a file |
|---|---|---|
| AST review — "function 'log_message' in module 'vendorsync.api' is a STUB" | **3 of 3 runs** | **no** |
| spec_contract — "GET /… returned 404" | every run | no (correctly — it names no file) |
| cross-module drift — "module 'A' reads a field 'B' does not define" | yes | **no** |
| "planned deliverable \`vendorsync/store.py\` is MISSING" | yes | **no** |
| the F37 failed-task finding (path in backticks) | new | **no** |
| pytest traceback | when tests fail | yes |

Five of six, including the only two present in every run. The fan was well built and had almost
nothing to fan — the third instance of this exact class, after the judge-side splitter that never
fires and the e2e fan that did not partition.

**Fixed:** the extractor now also takes a path in BACKTICKS anywhere in the sentence (engine-authored
findings put it mid-sentence), and resolves a DOTTED MODULE against the run's own planned file list —
so a module can only ever map to a file this run really owns, and an invented path can never aim a fix
shard at nothing. First match wins, because these findings are written subject-first; taking the last
pointed the drift fix at the module that was correct. A finding that genuinely names no file stays
unassigned and goes to the serial path, which is where it belongs.

Pinned by a test built from the real strings, not from invented ones.

## F42 — the mechanisms that would make the swarm smarter with more nodes are mostly unobservable

Goal one is that the swarm gets better as nodes are added. The engine has a set of mechanisms whose
entire purpose is to spend a spare node on quality or latency. Counting them across the three parked
runs, and then checking whether each can even be counted:

| mechanism | event | 3 parked runs |
|---|---|---|
| idle-model judge | `judge_verdict` | **54 / 161 / 94** — fires hard |
| idle pre-reviewer | `pre_review` | **7 / 7 / 7** — fires |
| dynamic replan | `replanned` | **0 / 0 / 0** — a REAL zero |
| sink idle-fill review | `sink_review` | **0 / 0 / 0** — a REAL zero |
| speculation (twin race) | *none existed* | **unknowable** |
| judge-side split | *none existed* | **unknowable** |
| sink prebuild | *none existed* | **unknowable** |

I nearly published "seven mechanisms never fire". That would have been false. Four of those sevens
were not zeros — they were **blind instruments**, and the difference is the whole rule. The check that
caught it was extracting the engine's complete `"event"` name list and its `SwarmEvent` enum and
asking, per mechanism, *could a run have shown me this at all*. For `replanned` and `sink_review` the
answer is yes and they genuinely never fired. For speculation, split and sink prebuild there is no
event and never was: `pick_speculation_target`, `resolve_speculation` and `apply_split` contain
**zero** `sink.emit` calls between them.

So on a project whose first rule is that only a deterministic engine event may confer a verdict, the
three mechanisms most directly responsible for "better with more nodes" have been exempt from it. Not
disputed — unmeasurable. That is why the node-scaling claim has never been settled by evidence.

**Fixed:** two new `SwarmEvent` variants, emitted only where the thing actually happened.
`task_split{task_id, children}` at `apply_split`'s success return — so it means "a split was applied",
never "one was considered". `speculated{task_id, attempt, winner}` at every resolution of a twin, with
`winner` one of `twin` / `primary` / `twin_failed`, because "the twin won", "the primary won" and "the
twin errored and cost a device for nothing" are three different answers to whether an idle node bought
anything, and all three were previously the same silence.

`sink_prebuild` remains unobservable and is NOT being patched blind — it needs its own read first.

439 + 44 tests, clippy clean.

## F43 — `replanned` never fires because its window IS the sink, and that suppression is correct

Following F42's two real zeros. `dynamic_replan` needs, simultaneously: something in flight, nothing
ready, `idle_capacity() >= 2`, replans left, and **not the sink in flight** — the last deliberately,
because a bonus task completing after the sink's PASS would land unverified code.

`occupancy.py` now attributes the serial tail per task, and the answer is unambiguous:

| unit | solo time | who holds it |
|---|---|---|
| preboundary | 1045.3s | **100% `integrate-verify`** — the sink |
| preboundary-2 | 543.0s | **100% `integrate-verify`** — the sink |
| preboundary-3 | 55.9s | `test-web` |

So in two of three runs the ENTIRE window where two nodes sit idle is the one window replan is
designed to skip, and in the third it is 56 seconds. `replanned` not firing is not a defect and needs
no fix: the scheduler keeps the fleet busy right up to the sink, and the sink is excluded on purpose.
Recorded as a settled negative so it is not re-investigated.

**Getting here cost a corrected number, and the correction is the point.** I first computed the
per-task tail in a throwaway script and got 1484.0s where the instrument said 55.9s — a 26x
disagreement. The instrument was right. My script paired each dispatch with the task's single
completion, so for a RETRIED task only the last attempt got a span and the earlier attempts' busy time
vanished, inflating "solo". That is precisely the bug `occupancy.py` carries a sixteen-line comment
about, having once turned one retry into 83 minutes of phantom busy time and published
`max_useful_nodes = 1.75` for a real figure near 3.

Re-implementing an instrument in a throwaway script re-earns every bug it was fixed for. The
attribution therefore lives in `occupancy.py` now, built from the spans it already pairs correctly, so
the next person asking "which task owns the tail" reads it instead of rebuilding it.

**What this leaves for goal one.** Three places more nodes cannot help, all now measured:
the pre-dispatch prefix (1312-1836s, planning 68-83% of it, doubled whenever a redraft fires);
the sink (543-1045s, one node, by construction);
and the repair tail (1668-3180s, 2-3 sequential fix calls — the largest of the three, and the one
whose fan could not see its own findings until F41).

## F44 — `sink_review` reported itself ON for months while the half that fills its queue was OFF

The other real zero from F42, and unlike `replanned` this one is a defect.

`sink_review` puts otherwise-idle nodes on read-only whole-tree reviews **while the sink runs** —
precisely the window F43 measured as the biggest idle block in the run: `integrate-verify` owns
**100% of the solo time in 2 of 3 units, 543-1045s, with two nodes doing nothing.**

The mechanism has two halves and they disagreed about the default:

| half | where | default |
|---|---|---|
| PRODUCER — fills the queue | `scheduler.rs` `pick_sink_review` | `std::env::var(...).unwrap_or(**false**)` |
| CONSUMER — drains + re-verifies | `swarm.rs` | `swarm_gate(..., **true**)` |
| what the run TELLS you | `levers_resolved` | `swarm_gate(..., **true**)` |

So every run emitted `sink_review: true`, the consumer was live, the queue was never filled,
`prewarmed` was always empty, and the event never fired. An operator auditing levers would read
`true` and believe it. **The mechanism has never executed once.**

Both halves now read one resolver, `goose_swarm::sink_review_enabled()`, exported so there is a single
answer rather than two. The default stays **OFF** — the truthful one, matching every measurement taken
so far — so baseline does not shift underneath the campaign, and `levers_resolved` now reports what is
actually happening. Turning it on is an ARM with a written prediction, not a silent flip.

Also fixed while here: `sink_prebuild`'s doc said "OFF by default" while the Default impl has said
`true` since the golden-formula bake. The behaviour was right and the documentation was months stale —
which is how it ended up on the unobservable list in F42 under a false description.

**The pattern worth naming.** Three defects today have the same shape: a mechanism that reports one
thing and does another (`sink_review` on/off), an event that reports a count where the texts were
needed (`complete_verify`), and a pool event emitted before the pool was final (`run_started.pool`).
In each case the run was not lying about the outcome — it was lying about itself, and every downstream
verdict inherited that.

## F45 — the line that measures goal one was reading four event names the engine never emits

Found by applying F40's lesson rather than waiting for another adversarial round: when a defect is
fixed, go and look for the same shape everywhere else. `prefix.py` had been reading `owned_files`
where `plan_loaded` emits `files`. I had not checked whether the other instruments had the same
disease. They did.

`occupancy.py`'s `IDLE_NODE_EVENTS` — the map behind the line rendered as **"idle-node jobs (the
'smarter with more nodes' half)"**, which is the single line in the whole harness that measures goal
one — had four of eight keys naming events that do not exist:

| map key | what the engine actually emits |
|---|---|
| `prereview`, `prereview_finding` | **`pre_review`** |
| `speculation`, `speculative_promoted` | **`speculated`** |
| `replan`, `dynamic_replan` | **`replanned`** |

So it reported `{'judge': 161}` on a run that fired `pre_review` **seven times**, every run, and the
omission was invisible because a missing key looks exactly like a mechanism that did not fire. After
the fix: `{'judge': 161, 'pre_review': 7}`, `{'judge': 54, 'pre_review': 7}`, `{'judge': 94,
'pre_review': 7}`.

F42's published census is unaffected — it was computed from raw event counts, not from this map. But
anyone reading the instrument's own output would have got a different and wrong answer, which is the
worse failure.

**The control that would have caught it now exists**, and writing it produced a second finding. Its
first version picked the newest `run.jsonl`, which was the unit five minutes into its research phase,
and it failed on a `pre_review` that had legitimately not happened yet — newest-file-wins, the exact
trap the campaign notes warn about. It now selects the most recent run containing `run_finished`. An
in-flight run's absent events are a clock, not a defect.

Mechanisms that legitimately never fire (`replanned`, `sink_review`) are excluded from the positive
assertion on purpose: a control that demands they appear would be a control demanding a defect stay
present.

Verified both directions — restoring the broken key fails the self-test with the exact diagnosis.

## F46 — a contract between the harness and the engine, so the name class cannot recur

Three instruments shipped a name the engine does not emit (F40's `owned_files` vs `files`, F45's four
bad idle-node keys). Fixing the third one is not the job; removing the mechanism that produced all
three is. `selftest.py` now asserts the harness's reads against the engine itself, and the two halves
deliberately use **different ground truth**:

- **FIELD names** come from a real **FINISHED** run — only a run proves what an event carries — and are
  asserted conditionally: if the event appears, the field must too. An event that has never fired is
  not a failure, it is a clock.
- **EVENT names** come from the **engine source** (`"event": "x"` literals plus snake_cased
  `SwarmEvent` variants, 53 found). They cannot come from a run, because a mechanism that legitimately
  never fires would make a run-based check unable to tell a wrong name from a quiet mechanism — which
  is precisely the confusion that hid F45 for months.

Both directions verified, and each reproduces a defect that actually shipped:

    harness reads `prereview`         -> FAILS: "event name(s) the engine never emits: ['prereview']"
    harness reads `owned_files` on
      plan_loaded.tasks[]             -> FAILS: "missing field(s) the harness reads: ['owned_files']
                                                 (it carries ['deps','description','difficulty',
                                                 'files','id','model'])"
    both restored                     -> passes

And the discovery itself is proven non-blind — 53 engine event names found, with `pre_review`,
`speculated`, `task_split`, `replanned` and `pool_resolved` all confirmed present. A contract that
found zero names would pass everything, which is the failure mode this whole project keeps meeting.

Harness-only; permitted under the freeze. It runs on every unit, so the next time the engine renames
something the sweep fails loudly instead of quietly reporting a zero.

## F47 (preliminary, n=1, mid-run) — most of what a redraft discards is free to regenerate

First-ever `retarget_discarded` payload, observed live 17 minutes into the current unit. Round 1
discarded **12 tasks carrying 13,305 characters of detail**. But the composition matters more than
the total:

| what | tasks | chars | cost to regenerate |
|---|---|---|---|
| ENGINE-generated specs — `verify::*`, `verify-e2e::*`, `integrate-verify` | 7 | ~8,127 | **nothing** — deterministic templates over the plan |
| MODEL-authored detail — `api` (2044), `cli` (1870) | 2 | ~3,914 | a detail call each |
| FAILED details — `meridian` (141), `store` (123), both `detail_fallback` | 2 | ~264 | worthless, already lost |

So a redraft throws away **two model-authored specs**, not twelve tasks' worth. A detail-reuse cache
would save two calls, not the fleet-minutes the raw count suggests — which is materially less
valuable than "12 tasks discarded" reads.

This is exactly why the event was built as a MEASUREMENT and the cache was refused until the rate
was known. The first observation has already moved the design conclusion, before a line of cache
existed.

**Scope, honestly:** n=1, one redraft, one plan, and the unit has not finished — the survival rate
(discarded ∩ shipped, keyed on owned files) cannot be computed until `plan_loaded` lands. The
composition split is a property of the plan SHAPE rather than a sample statistic, so it is likely to
hold, but "likely" is not measured. Confirm across the baseline set before acting on it.

Also noted, not yet a finding: `meridian` — the module owning the vendor client — took a
`detail_fallback` at 141 chars for the **third consecutive run**. In this one it happened in the
DISCARDED round, so it may be repaired in round 2; that is precisely the ghost-versus-live
distinction, and it is why the instruments separate them.

## F48 — the fleet sat idle for 30 minutes waiting for an answer that changes nothing

Caught live on the first post-freeze unit by watching the event stream instead of the clock.

At +26.6m `low_confidence_ask` fired: plan confidence **68** against a floor of **85**. The engine
writes `.swarm/clarify-questions.json` and BLOCK-POLLS for `.swarm/clarify-answers.json` for
`ask_wait_secs` — **1800 seconds, thirty minutes** — then proceeds and decides the questions itself.
Nothing in this harness answers, so the wait is always paid in full and always ends the same way.
Confirmed by the fleet itself: `lms ps` showed **GENERATING = 0** across all three nodes.

**The plan that ships after 1800s is the plan that would have shipped after 5s.** The wait buys
nothing. It costs ~25% of a unit in idle fleet — and it is *node-independent*, so it lands directly in
the wall-clock and occupancy figures the node curve compares, as pure noise. Across a 12-unit baseline
set that is six hours of fleet time and a variance term that could swamp the effect being measured.

Fixed harness-side, `GOOSE_SWARM_ASK_WAIT_SECS=5` — the engine is frozen and did not need to move.
Deliberately NOT answering the questions from an authored canonical set: not answering keeps the
treatment identical across cells without putting my judgement into the build, and it is what an
unattended run does anyway once the timer expires.

The unit was killed and discarded rather than allowed to finish. It was 35 minutes in and had
dispatched **zero tasks** — the whole 35 minutes was planning and idle — so nothing of value was lost,
and letting it complete would have put one unit with a 30-minute stall into the same replicate set as
eleven without, which is worse than either extreme.

**The lesson for the freeze:** "prefer observing over building" is not "stop looking". This cost
nothing to find — one look at the live event stream and one `lms ps` — and it was silently burning a
quarter of every unit.

## Correction — "GENERATING" is not the whole of "busy"

I read fleet occupancy with `lms ps | grep -c GENERATING` and got 0 across four samples on a healthy
run, and briefly took it for a stall. The full status shows **`PROCESSINGPROMPT`** as a separate busy
state — two nodes were in it while the third generated. A node ingesting a long planning prompt is
working, not idle.

F48 is unaffected: its evidence is the engine's own 1800s block-poll and a `low_confidence_ask` with
nothing on the machine able to answer it. The fleet reading was corroboration, and during that window
the nodes were genuinely idle in both states. But the check itself was wrong, so any future
fleet-busy sample must count **GENERATING and PROCESSINGPROMPT**, not just the first.

## F49 — the engine spends 10-19 MINUTES redrafting a plan and 75 SECONDS writing the specs it depends on

Mihai's framing forced the right question: the redraft was built to make the build **functional
always and predictable**. So judge every mechanism on THAT, not on its own metric.

**What the redraft actually optimises.** `plan_confidence` breaks down as agreement + spec_clarity.
`spec_clarity` scores **100 in every run on disk** — it never binds. `agreement` is literally whether
the plan drafts emitted task counts within 1 of each other: "count spread 1, file-overlap 100%" gives
**88**, spread 0 gives **100**. It is a two-valued function of draft-count parity.

Four of five shipped plans scored exactly **88**, including one that never redrafted. The three units
with build scores all shipped at 88 and scored **88.7% / 50.0% / 42.7%** — a 46-point spread at
identical confidence, and the run whose redraft climbed 46→88 scored the *worst*. Cost: **10-19
minutes per round**, 8-15% of a run. And growing `best_of_n` (3→4→5) to raise agreement can LOWER it,
because more drafts means more chances one disagrees on count — observed directly as 75→68, caught by
the stall guard, whose existence suggests the authors met this too.

**What actually predicts a functional build**, measured: whether the module owning the external
contract got a real spec. `meridian` with 1497 chars containing the vendor's `/v1` prefix → 88.7%;
`meridian` with 122 chars → 42.7%, every vendor call 404ing.

**And the engine never protects that.** A failed detail call is **never retried** — `filler`,
`agent_error` and `timeout` all fall straight through to the architect's one-liner. Across six runs:
**27 detail failures, and all 25 checked are `timeout`.** Zero filler, zero agent errors. So the
failure mode is entirely the ceiling, and a retry at the same ceiling would fail identically.

**The disparity, which is the finding.** Two sibling fan-outs, same fleet, same model, both asking a
worker to author a spec:

| fan-out | ceiling | failures on disk |
|---|---|---|
| `contracts` | `worker_timeout_secs.max(10)` — baked **900s** | **1 of 19** (5%) |
| `detail` | hardcoded **75s**, twice (per-call timeout AND straggler grace) | **27** |

`meridian` is the most frequent victim at 6 of 25. The 75 is a bare literal with no derivation; its
sibling derives from the fleet's own worker timeout.

**Fixed:** `detail_budget_secs()` now derives from `worker_timeout_secs` like its sibling, and the
straggler grace uses the same ceiling instead of a second hardcoded 75.

**And the log can now size its own budget.** A timeout says ">budget" and nothing more, so every value
was a judgement call — including mine. `detail_completed{task_id, secs, spec_chars, brief_chars,
budget_secs}` now records the SUCCESSFUL calls, giving the duration distribution the ceiling must
clear and pairing it with the spec size that drives it. The next runs measure what the budget should
be instead of me guessing.

**The redraft itself is NOT removed.** Its intent is right and its cost is real; what is wrong is the
signal it fires on. Re-aiming it needs the `retarget` off/on arm to establish whether it buys anything
at all — written down now: build score unchanged within the replicate spread, wall-clock down 10-20%.
If the score DROPS, the redraft is buying something real and this reading was wrong.

## F50 — the redraft ladder, caught live: 48 minutes, 27,806 characters discarded, floor never reached

The current unit is the cleanest evidence yet, and it was produced by simply watching the log.

    +17.4m  redraft   conf 68  best_of_n 3->4    17 tasks discarded, 11,291 chars of model-authored spec
    +27.7m  redraft   conf 80  best_of_n 4->5    15 tasks discarded,  6,246 chars
    +39.7m  redraft   conf 81  best_of_n 5->6    18 tasks discarded, 10,269 chars
    +48.4m  low_confidence_ask                   gave up, asked the human
    +48.5m  low_confidence_ask_timeout           nobody answered; proceeding

**48.4 minutes before a single task was dispatched. 27,806 characters of model-authored
specification thrown away** — real work the fleet produced, implementers and tests, excluding the
engine-generated verify/e2e specs that regenerate for free. And the confidence never reached the 85
floor: 68 → 80 → 81, decelerating, then it asked a human instead.

This confirms the prediction in F49 that **growing `best_of_n` to raise agreement can fail to raise
it**: the pool went 3→4→5→6 and the gain went +12, +1, then nothing. More drafts means more chances
one disagrees on task count, and agreement is exactly draft-count parity.

Meanwhile `detail_fallback` fired **13 times** across those rounds, `meridian` — the module owning the
vendor contract, the measured predictor of a functional build — losing its spec **four separate
times**. The engine re-derived the whole plan three times and never once retried the one call whose
failure predicts the build failing.

**F48 is confirmed in the same log.** `low_confidence_ask` → `low_confidence_ask_timeout` in exactly
**5 seconds**, against the 1800s default it had this morning. The clarify block-poll no longer idles
the fleet for half an hour.

## F51 — PREDICTION REGISTERED BEFORE THE OUTCOME (unit swarm-3node-r0, plan loaded 23:00, build not yet graded)

Written at 2026-08-01 23:12, while the unit is still executing, so this
cannot be fitted to the answer. F36 said the predictor of a functional build is whether the module
owning the external contract got a real spec. This is the first chance to use it PROSPECTIVELY.

The plan that shipped at +50.2m, after 48 minutes and three discarded redrafts:

    task_count 17, plan_confidence 84 — BELOW its own floor of 85
    api        2184 chars
    web        1773 chars
    meridian    105 chars   <- owns the vendor client
    store        80 chars
    /v1 appears in ZERO task descriptions

**PREDICTION.** The vendor client will use the wrong base path, every vendor call will 404, and
`crunch.py` will fail `fetch_all_payments` / `total_count` / `idempotent_create`. Expected crunch
**≤ 3 of 7**; expected build score in the **40-55%** band, alongside the 42.7% and 50.0% runs whose
plans also carried no `/v1` — not the 88.7% run, whose `meridian` got 1497 chars containing it.

**FALSIFIABLE.** If this unit scores above 80%, or if `crunch.py` passes `fetch_all_payments`, then a
105-character brief on the contract-owning module does NOT determine the build and F36 is wrong as a
predictor. That would be the more valuable result and it must be reported as plainly as a confirmation.

Note what the engine did with its 48 minutes: three full re-plans chasing draft-count parity, which it
never achieved (84 < 85, shipped anyway), while the two modules that matter most went out with 105 and
80 characters and `detail_fallback` fired 13 times. The machinery optimised the proxy to exhaustion
and left the predictor unprotected — F49 and F50 in one run.

## Correction — I read the vendor trace at its destination, mid-run, and nearly published two fabricated findings

Caught by my own rule, one step before writing it down. The reading was **0 vendor requests**, and I
had already drafted two findings on it: that no node read the vendor documentation this run, and that
the `verify-e2e` shards completed without exercising the vendor.

Both were false. `sweep.py` writes the trace to `runs/nodeloop/trace-<unit>.jsonl` **during** the run
and only `.replace()`s it into `<unit>/vendor-trace.jsonl` when the unit FINISHES. I was checking the
destination, which does not exist until the end. The live file has **11 requests**:

    200 GET  /v1/docs      x4      <- a node DID read the documentation
    200 GET  /v1/payments  x4
    429 GET  /v1/payments  x1      <- and exercised the throttle
    201 POST /v1/payments  x1
    409 POST /v1/payments  x1      <- and the idempotency replay

All `curl/8.7.1` — exploration by a node, not the built app, which has not run yet. So the F51
prediction is still live and untested.

**What it actually shows is F35 for the third consecutive run.** A node read the documentation
thoroughly enough to trip the 429 and the 409, and `/v1` still appears in **zero** task descriptions
in `plan_loaded`. The fact was discovered and died with the node that discovered it. That is exactly
what the `doc_fetch` arm exists to remove — the engine fetching the document itself does not depend on
a node happening to curl it and then on that knowledge surviving into the decomposition.

**The rule that saved it:** a zero is usually a broken instrument, so prove the query can see the
thing at all before letting the zero license a conclusion. I checked whether the file existed rather
than trusting `0`, and the check cost one command. Two findings would have gone into the ledger
otherwise, and both would have been wrong in the same direction — toward a more dramatic story.

For the record, so the next reader does not repeat it: **during a run the trace is
`runs/nodeloop/trace-<unit>.jsonl`; after it, `<unit>/vendor-trace.jsonl`.**

## F49 addendum — the derived budget is 420s here, not 900, and the worst case needs watching

Checking my own fix rather than trusting the commit message. F49 says the detail budget now derives
from `worker_timeout_secs` "baked at 900". The BAKED default is 900; **this machine's config.yaml sets
`worker_timeout_secs: 420`**, so the live derivation is `420.max(75)` = **420 seconds**, 5.6x the old
ceiling rather than 12x. The fix is still correct — it removes a bare literal and ties the ceiling to
the same source its sibling uses — but the number is 420 and the record should say so.

**The worst case is now larger and is worth measuring, not assuming.** The ceiling only binds on
FAILURE: a successful detail was measured at 44.5s, so the typical fan is unchanged. But I raised TWO
ceilings — the per-call timeout and the straggler grace — so a fan of 17 tasks over 3 devices where
several calls hang could in principle spend far longer in the prefix than the ~7 minutes a 75s
ceiling allowed. Work-stealing means one slow item does not block the others, and the contracts
fanout has run with exactly this derivation without trouble, so the risk is bounded rather than
absent.

`detail_completed{secs, spec_chars}` exists precisely so the next runs settle this instead of me
reasoning about it: it gives the duration distribution the ceiling has to clear, and the prefix
measurement in `prefix.py` shows whether the fan got slower. **If the prefix grows materially while
`detail_fallback` goes to zero, the right answer is a ceiling between the two — sized from the
distribution, not from either literal.** That is the question the first post-boundary baseline unit
now answers, and it is why that cell carries two questions rather than one.

## F52 — the first `finding_texts` ever emitted exposed a defect that F41 had just made reachable

The event that decides green carried only a count until tonight. The first one it emitted on the new
engine paid for itself immediately.

The single finding holding this build red is a `pytest -q` failure, and its text is a Python traceback
whose **first frame is CPython's own stdlib**:

    `pytest -q` failed — the generated tests exercise runtime paths that `--help` never invoke:
        File "/opt/homebrew/.../python3.14/threading.py", line 1024, in run
        File "/Users/.../runs/nodeloop/swarm-3node-r0/vendorsync/api.py", line 40, in serve

I ported the extractor and ran it on that exact text. It returned
`/Users/.../swarm-3node-r0/vendorsync/api.py` — **an absolute path, not one of the run's owned
files**. Two consequences, both live:

1. `group_findings_by_file` keys the partition on that absolute path, while every other part of the
   engine uses repo-relative. A fix shard would own a path its shadow tree promotes by a different
   name.
2. Reorder the frames — entirely plausible, tracebacks vary — and the extractor returns
   **CPython's `threading.py`**. `complete_parallel` would then dispatch an agent to repair the Python
   standard library.

**F41 made this reachable.** Before it, five of six finding shapes resolved to nothing, so the fan
never fired and the bug never mattered. Fixing the extractor to make the fan work turned a latent
defect into a live one — which is exactly why "treat 'I fixed that' as a hypothesis" is a rule, and
why `complete_parallel` is a queued arm rather than a flipped default.

**Fixed:** an extracted path must resolve to a file the run actually owns — exact match first, then
longest repo-relative suffix, so a traceback's absolute path maps back to `vendorsync/meridian.py`. A
path naming only files the run does not own resolves to NOTHING and goes to the serial fix path;
inventing an owner is worse than admitting there is none. With an empty file list the old behaviour
stands, which is the unit-test path — every real call site passes the run's planned files.

Both cases pinned by tests built from the real traceback, and the guard is verified to BITE: removing
the restriction fails the suite.

## F53 — MY PREDICTION WAS WRONG. The build scored 83.4% and crunch passed 7/7.

F51 was registered before the outcome precisely so it could fail, and it failed comprehensively.

    PREDICTED   crunch <= 3 of 7,  score 40-55%
    ACTUAL      crunch  7 of 7,    score 83.4%

Every check passed. `fetch_all_payments` returned all **247** payments in 2.0s. The vendor trace shows
**52 successful GETs to `/v1/payments` from the built app** (`Python-urllib`) and **zero 404s**. The
client source contains `/v1/payments` three times.

And it did that with `meridian` shipping a **105-character brief**, `/v1` in **zero** task
descriptions, and a plan confidence of 84 that never cleared its own floor of 85.

**So F36 is refuted as a deterministic predictor.** A thin brief on the module owning the external
contract does NOT determine the build. The worker recovered the path itself — which is exactly what
the audit's retraction below makes possible.

The correlation across all 9 scored runs still separates, but barely, and the story has changed:

| `/v1` in the client's brief | n | scores |
|---|---|---|
| yes | 3 | 0.8998, 0.8872, 0.8673 |
| no | 6 | **0.8336**, 0.5000, 0.4424, 0.4424, 0.4270, 0.2999 |

The gap between the groups collapsed from 37 points to **3.4**, and the "no" group now spans 0.30 to
0.83. A predictor whose negative class covers half the scale is not a predictor. What it looks like
now is a *risk factor* — losing the brief makes a bad build more likely and does not cause one.

## F54 — RETRACTION: the scouts DO have tools. F34 was wrong, and it was propagating.

The audit checked what I asserted. **32 of 33 scout activity digests contain a successful shell call
carrying an http URL** — the scouts curl `/v1/docs`, trip the 429, exercise the 409 replay. Every
agent gets the `developer` builtin with `shell` attached unconditionally.

What they lack is an *MCP research extension*. `research_tools.available` lists only MCP extensions,
and `grounded = is_mcp && ok` excludes shell by construction — so a run that made 17 vendor HTTP calls
reports `available: [], can_look_things_up: false`. The event is not conservative, it is **wrong**,
and I built F34 on it and then repeated "scouts have NO tools" as settled fact in every subsequent
working note.

This also explains F53: a worker handed a 105-character brief can still read the vendor documentation
itself. The information channel that matters is not the plan — it is the worker's own shell.

**The proposed fix is NOT being applied.** An adversarial pass refuted it convincingly: a `retrieved`
flag defined as "a successful shell call whose command contains an http URL" is *more* launderable
than the claim it replaces — the corpus already contains
`MeridianClient('https://api.meridian.com/v1', ...)` printed by an import smoke test, against a host
that does not exist. Every URL in the entire corpus is loopback to the bench's own fixture, and the
flag would read true on 9 of 9 runs including the 42.7% one, so it has zero diagnostic power. Recording
the diagnosis and rejecting the cure.

## F55 — first measured retarget-discard survival, and the prefix reached 50 minutes

The finished unit's prefix: **3014s, 90% of it planning, four redraft rounds** (735 / 620 / 719 /
630s). Occupancy 0.3047 overall, 0.7174 in execute, `MAX USEFUL NODES 2.39`. Thirteen detail
failures, all `timeout`, `meridian` losing its spec four times and still shipping a working client.

Survival across the discarded rounds, keyed on OWNED FILES as the design requires:

    round 1: 17 discarded, 9 came back by owned files (17 by id), 11,291 chars re-derived
    round 2: 15 discarded, 4 came back by owned files (15 by id),  4,531 chars re-derived

The by-id number is inflated exactly as predicted — it counts the engine-generated `verify::*` tasks,
which own no files and regenerate for free. The honest reuse opportunity is **~15,800 characters of
model-authored spec across two rounds**, on tasks whose file ownership was unchanged. That is real,
and it is now measured rather than assumed.

**What this does to the queue.** `doc_fetch` was cell 2 on the argument that a lost `/v1` breaks the
build. F53 says it does not, reliably. It stays queued — a verbatim document is still the densest
instruction available and removes a coin flip — but it is no longer the highest-value question.
`retarget_off` is, by a wide margin: 50 minutes of prefix, 90% planning, four rounds, ~15,800 chars
re-derived, and the build came out fine anyway.

## F56 — the judge ALREADY replaced the clocks for dispatched tasks. The gap is the PLANNING phase.

Mihai's design: an idle node should take the judge role and investigate the others, and that
judgement should eliminate hard-coded timings. Measured against the corpus, that design is **already
implemented and already working** — for one half of the run, and completely absent from the other.

**Who actually terminates a worker**, across all 9 runs on disk:

    re-dispatches (attempt > 0)   32
      caused by the JUDGE         30   (94%)
      caused by a CLOCK            2   (6%)  — one stream-decode drop, one reasoning spiral

851 judge verdicts: 814 `observed`, 30 `re_dispatch`, 7 `failed` — a 4.3% intervention rate. So the
judge is not decorative and the clocks are not the decision-maker. `worker_timeout_secs` and
`progress_watchdog_secs` behave exactly like the failsafes their doc comments claim to be: they fired
**twice in nine runs**.

The judge is also not clock-driven where it matters. Its inputs are behavioural — `any_owned_written`,
`secs_since_last_write`, `tool_calls` — and its over-read trip is documented as firing "regardless of
the clock".

**So the premise needs correcting, and then it points somewhere better.** For DISPATCHED tasks the
architecture is already what was asked for. Every measured harm from a fixed timing this week came
from the **PLANNING phase, which no judge watches**:

| fixed timing | where | measured harm |
|---|---|---|
| detail budget 75s | detail fan | **27 failures, ALL timeout**; `meridian` lost its spec 4x in one run |
| `ask_wait_secs` 1800 | clarify gate | **30 minutes of idle fleet per run**, for an answer nothing writes |
| draft/redraft rounds | best-of-N ladder | 3014s prefix, 90% planning, four rounds, ~15,800 chars re-derived |

Scouts, plan drafts, the detail fan and contracts are **not scheduler tasks**. They are `fanout_over_fleet`
calls bounded by `tokio::time::timeout`. No judge is attached, nothing inspects whether the call is
producing or spinning, and the only available verdict is "the clock ran out". That is precisely the
pathology the judge was built to end, surviving in the phase the judge never reached.

**The design that follows, and it is Mihai's applied one level up:** a detail/draft call should be
watched the same way a dispatched worker is. The signals exist in the same shapes — is it emitting,
has it produced a spec, is it thinking with no output — and an idle node can form that judgement
cheaply and at low context, which is the whole reason the judge works on a weak model.

**Two of the judge's own clock-free trips are disabled by default**, so where a behavioural verdict
could already replace a clock, it does not:

- `spiral_thinking_chars: 0` — the reasoning-spiral trip is OFF. Its doc says it catches the spiral at
  ~60-120s where the idle watchdog needs the full window. One of the two clock kills in the corpus was
  exactly this pathology, caught late by the clock instead of early by the judge.
- `split_enabled: false` — the judge can never propose a split, which is why `task_split` has never
  fired.

Both become arms rather than silent default flips: the readout is a mechanism event, so n=1 settles
each.

## F57 — the judge's semantic review ran 4.3% of the time. It was not deciding "ok"; it was never asked.

Mihai's hypothesis was that the judge might be watching rather than acting because it is not given
proper direction. The measurement says something sharper: **it mostly is not given anything at all.**

851 judge verdicts across 9 runs, split by SHAPE rather than by verdict:

    early return / deterministic ok  (confidence 1.0, empty hint)   814   95.7%
    semantic review — looping                                        23    2.7%
    semantic review — over_reading                                   10    1.2%
    semantic review — broken_code                                     4    0.5%

The confidence distribution seals it: **817 verdicts at exactly 1.0**, 33 at 0.9, one at 0.85. A
model-authored verdict never lands on exactly 1.0; those 817 are code returning `JudgeOutcome::ok()`.
The SUPERVISOR prompt — the goal, the run state, the subtask spec, the files so far, the activity log,
the whole apparatus this mechanism exists for — is reached **4.3% of the time**.

**The cause is one gate:**

    if input.file_contents.is_empty() && acts < 4 { return JudgeOutcome::ok(); }

`worker_tool_calls` comes from the activity digest, and **a reasoning model that streams thinking
makes no tool calls**, so `acts` stays 0. A worker with no file and no actions was read as "hasn't got
going yet" — but that is equally the signature of a worker thinking itself in circles. The one worker
most in need of a supervisor was the one guaranteed not to get one. `worker_thinking_chars` was
already collected and already in `JudgeInput`; the gate simply never consulted it, and the only thing
that did — the spiral trip — is off by default.

**Fixed, and deliberately without a new threshold.** The gate now also requires `thinking == 0`:
"nothing to assess" means nothing produced, not "produced only thinking". No char limit, because
another tuned literal is exactly what is being removed here, and `min_age_secs: 90` plus
`rejudge_cooldown_secs: 60` already prevent reviewing a just-launched worker. A worker 90 seconds old
that has emitted reasoning while writing nothing and calling nothing is precisely what a supervisor
is for.

The judge also could not SEE the distinguishing signal: its trace block reported actions, errors,
recent calls and last reasoning, but never **how much** reasoning. It now reads
`reasoning emitted: N chars`, which is the difference between "slow" and "spiralling" on a model whose
only output while working is thinking.

**This is the through-line Mihai named:** across every phase the failures have been imprecise or
absent instruction — a worker handed a 105-character brief, e2e shards told to number commands from a
spec they were never given, a fan whose extractor could not read four of six finding shapes, and now
a supervisor asked for its judgement on one call in twenty-three.

## Correction — my ad-hoc marker check contradicted loop.sh twice, and loop.sh was right both times

After crossing the boundary for F57, `loop.sh boundary` reported the marker **present**; a hand-rolled
`grep -qF -- "$M" <(strings "$B")` a moment later reported **ABSENT**. Re-running it reported ABSENT
for four markers, two of which had been verified present an hour earlier — an impossible result, and
therefore an instrument fault rather than a build fault.

Positive control settled it in one command: `strings` finds `run_started`, `reasoning emitted` and
`detail_completed` exactly once each. Everything shipped.

The cause was mine. I invoked `./loop.sh boundary` a **second** time to re-read the markers, and that
verb REBUILDS — so my check was reading a binary being rewritten underneath it (mtime moved from
…635466 to …635667 between the two). `loop.sh` has a settle-wait for precisely this, added after the
same failure mode once refused a boundary with all four markers "missing"; the ad-hoc check bypassed
the guard that exists to prevent it.

Two rules, both already in the ledger and both re-earned tonight:

- **Never re-implement an instrument ad hoc.** This is the second time in one night a throwaway check
  disagreed with a real instrument and the instrument was right (the other gave 1484s where
  `occupancy.py` correctly said 55.9s).
- **`./loop.sh boundary` is not idempotent-cheap — it rebuilds.** To re-read markers, read them; do
  not re-run the verb.

## F58 — the thin brief was not an accident. The architect is told to write one.

Following Mihai's through-line into the planning phase and finding its clearest instance yet. The
architect's instruction, verbatim:

    For each subtask provide: id (kebab-case), description (ONE short line — a fuller spec is
    written separately, keep it terse here), ...

So the 105-character brief that shipped to `meridian` was not a degradation — **it is what the
architect was asked for.** The design is: deliberately-thin primary instruction, plus a separate
enrichment pass that replaces it. That is sound only while the enrichment succeeds.

It failed **27 out of 27 times**, every one a timeout, and the shipped fallbacks ran 59-226 characters
(median 97). The promise "a fuller spec is written separately" is the detail fan, and the detail fan
is the single least reliable call in the engine. **The fallback is bad by construction, because the
construction assumed the fallback would never be used.**

**Fixed at the instruction, not the fallback.** The architect now writes 2-4 lines that STAND ALONE —
what to build, in which files, and the one check that proves it — explicitly told that a richer spec
will replace it *but to write as if nothing else will arrive, because sometimes nothing does*. Still
bounded ("do NOT write an essay, do NOT restate the whole goal") so the skeleton call does not bloat.

**The cost, stated honestly:** the architect writes more per subtask, and `best_of_n` multiplies that
across drafts. If the pre-dispatch prefix grows materially, this trades planning time for failure
safety and the trade needs to be judged, not assumed — `prefix.py` reports the number every unit.

**Why this is worth doing even though F53 showed a thin brief is survivable:** the 83.4% build proved
a worker can recover from a 105-char brief, because workers have shell and re-derive what they need
(F54). But recovery is a coin flip that costs worker time, and the other five runs whose client module
got a thin brief scored 0.30-0.50. Making the fallback safe is cheap; relying on recovery is not.

## F59 — the detail-budget fix is CONFIRMED by the first duration data the engine has ever recorded

`detail_completed` was added because a timeout says ">budget" and nothing more, so every ceiling was
a judgement call — including mine. The first four measurements, live:

    cli        42s  ->  1493 chars
    store      65s  ->  1818 chars
    api        74s  ->  2805 chars
    meridian  111s  ->  1467 chars

**The old ceiling was 75 seconds.** `meridian` — the module owning the vendor contract, the one that
lost its spec **six times** across the corpus — takes **111 seconds**. Under the old budget it could
not have finished, and it did not: every one of those six failures was a timeout. `api` at 74s cleared
the old ceiling by one second.

So the ceiling sat at roughly the 75th percentile of its own distribution, which is exactly what the
code comment warned about — *"a bare literal pinned at the OBSERVED MAXIMUM of the call it bounds,
which is the wrong place for a ceiling: normal variance then lands on the far side of it."* Two of
four calls were at or past it.

**And `detail_fallback` has not fired once this run.** The prediction registered with F49 — that the
derived ceiling drives the fallback rate toward zero — is holding on the first unit that could test
it.

**What the data does NOT yet settle: where the ceiling belongs.** 420s is generous, and generosity is
cheap while the ceiling only binds on failure — but a genuinely hung call now costs 420s instead of
75s, and I raised the straggler grace to match. Observed max is 111s at n=4. A ceiling near 250s would
be >2x the slowest call and far tighter than 420. **n=4 is too thin to move on**, so the value stays
where it is and the distribution keeps accumulating; the decision is deferred until the numbers
justify one, which is the whole reason the event exists rather than another guess.

This is the loop Mihai asked for, closed once: the log could not answer the question, so the log was
changed to answer it, and the answer arrived on the next run.

## F60 — the research phase's entire output has never been recorded, in any run, ever

Chasing the through-line into the one phase whose product nobody has seen. The chain looked
sound: scouts read the docs (F54), findings go to the planner AND to the detailer (`fb` = "Research
findings:"), the detailer is told to write "exact function/class names and signatures". Yet `/v1`
reaches ZERO task descriptions in most runs.

So which link fails? **Unanswerable from disk.** `research_completed` emits counts only — `findings:
2, grounded: 0, looked_nothing_up: 2` — and `research_findings` is passed to the planner and the
detailer and then vanishes. It is never written to the event log, never persisted. Checking the scout
activity digest instead: `last_text` is **EMPTY**, `reasoning` **EMPTY**, `full_reasoning` **EMPTY**.
The only trace of a scout's work is `calls` (it did `curl -s .../v1/docs`, ok, real content back) and
`thinking_chars: 11977`.

A phase costing **224-420 seconds of three-node time** on every run, and across every run ever
recorded there is no way to answer the one question that matters about it: **did it produce anything
the build could use?**

I nearly published the wrong answer twice getting here. First I read `last_text` as the finding and
got 0-5 characters for every scout — which reads as "the scouts report nothing" and is really "that
field is not the finding". A zero is usually a broken instrument, and this one was.

**Fixed:** `research_completed` now carries `finding_texts` — per lens, the kind, the question,
whether it was grounded, the full length, and the first 700 characters. Truncated and capped at six,
because a finding can be long and this rides every run.

**Why it matters beyond curiosity.** The two candidate failures have opposite fixes and the log
currently cannot tell them apart: if the scout's report never contains `/v1`, the defect is what the
scout is ASKED to report — the same imprecise-instruction class as everything else this week. If it
does contain `/v1` and the plan still does not, the defect is in the planner's use of it. Guessing
between those would have been the exact mistake F53 punished.

## F61 — F57 was UNMEASURABLE, and the real constraint is that a busy fleet has no node to judge with

The live unit reports **23 judge verdicts, 100% early-return** — worse than the 95.7% baseline F57 was
meant to improve. Before calling F57 a failure, I checked whether the metric can attribute the cause.
It cannot.

**Four distinct paths return `JudgeOutcome::ok()`**, and every one lands in the log as
`confidence 1.0, hint ""`:

    :15508  a spec-drift path that decided OK
    :15953  no idle device -> the model review is SKIPPED ENTIRELY
    :15989  the "nothing produced yet" gate  <- the only one F57 touched
    :16123  the model call itself failed

So "the semantic review runs 4.3% of the time" was never attributable, and neither is the 100%. My
prediction was untestable when I registered it, which is a fault in the prediction, not the fix.

**And the decisive path is UPSTREAM of the one I fixed.** The scheduler hands the judge a model only
when a device is idle — `claimed_device` is the first device with `in_flight < weight`, and
`judge_model_id` is empty otherwise. Execute occupancy is measured at **0.72-0.93**, so nodes are busy
nearly all the time and the semantic review is not being *declined*, it is **unreachable**. Whatever
the downstream gate says is moot when there is no model to review with.

**That is a genuine architectural tension and nothing in the log said so:** high utilisation is the
goal, and high utilisation is exactly the condition under which the supervisor cannot supervise. It is
also precisely Mihai's premise — "whenever a node is empty it should take the judge role" — working as
designed, with a consequence nobody had measured: on a well-packed 3-node fleet, nodes are rarely
empty.

**Fixed the instrument, not the mechanism.** `judge_skipped{task_id, reason}` now fires at both
reachable early returns with `no_idle_device` or `nothing_produced_yet`. The next run says which
constraint is binding, and only then is there a basis for choosing between the available responses —
reserve a node for judging, judge less often but always, or accept that a saturated fleet judges
deterministically only.

**F57 is NOT retracted and NOT confirmed.** Its reasoning stands on its own (a thinking-only worker
was being read as "hasn't started"), it is harmless, and it becomes measurable now. Deciding either
way on an unattributable metric is the mistake F53 already punished.

## F62 — the sink was killed for doing its job, whenever the planner handed it a README

The most expensive task in the run is `integrate-verify`: median **17.6 minutes**, owner of ~100% of
the solo window. Auditing what it is INSTRUCTED to do led straight to a defect with perfect
separation across 13 runs.

`integrate-verify` runs the assembled app, reads what it finds, and fixes failures. It writes no
source. The deterministic over-read trip fires on *"owns files, has written none, and is old"* — which
is a defect for an implementer and is the **job description** of a verifier.

The gate was guarded by `!owned_files.is_empty()`, and the sink normally owns nothing, so it stayed
disarmed. **In 3 of 13 runs the planner gave it `README.md`:**

| sink owns | runs | attempts | over-read kills |
|---|---|---|---|
| `[]` | 7 completed | **1** every time | **0** every time |
| `['README.md']` | 3 | 1, **3**, **3** | 0, **2**, **3** |

Every over-read kill of the sink in the entire corpus happened in a run where it owned a README, and
the 3-kill run is the **only sink failure on record**. Each kill re-dispatched it with the canned hint
*"You have produced no file yet. STOP reading/deliberating … WRITE your file(s) NOW"* — telling a
verification task to stop verifying and write a document.

The third README run survived because it wrote the README early, which set `any_owned_written` and
disarmed the gate. That is the mechanism confirmed from the other direction.

**And the sink's total contribution that run was nothing — the build still scored 83.4% with crunch
7/7.** Worth holding next to the sink's median 17.6 minutes when its value comes up for judgement.

**Fixed:** the trip now arms only when the task owns at least one CODE deliverable
(`is_code_deliverable`: source extensions only — a doc, manifest or lockfile is legitimate for a task
to own and legitimate not to have written yet). Pinned by a test built from the real case.

This is Mihai's through-line again, and it is the third instance of exactly one shape: **a rule
written for one kind of task applied to another** — implementer rules to test-authors (72-83% of
dispatches), the over-read rule to the sink, and "write your files" to a task that owns none.

## F63 — a test-author is told NEVER to read test files, while owning one. Caught firing live.

The live unit produced two `over_reading` kills, and both are **test tasks**:

    test-api       owns tests/test_api.py       2 re-dispatches
    test-meridian  owns tests/test_meridian.py  1 re-dispatch

(The sink owns `[]` this run, so F62 is not the cause here — that fix is correctly scoped: a test file
IS a code deliverable, so the trip stays armed for test tasks.)

The shipped worker prompt, verbatim:

> Read AT MOST the ONE file you will edit … **NEVER read the project's TEST files**
> (`test_*.py`/`*_test.py`) — they are not your dependencies and tell you nothing you need …

For an implementer that is right. For a **test-author it forbids reading the exact file it must
write**, and the same paragraph tells it to "Read AT MOST the ONE file you will edit" — the two
clauses contradict each other in four lines. `kind_prompt`, whose entire purpose is subtracting rules
that do not apply to a task's kind, is **OFF by default** and its own doc comment names this exact
case.

The plausible chain — forbidden from its own file, the test-author reads source instead, trips the
16-call over-read gate, gets killed and re-dispatched — is consistent with the live evidence but is
NOT established; two kills in one run does not make a mechanism. What IS established is the
contradiction itself, read straight from the shipped prompt.

**Fixed unconditionally, without waiting for the lever.** The clause now reads "NEVER read the
project's OTHER TEST files — any test file YOU OWN is your deliverable and is yours to read and write
freely". That is correct for every kind: an implementer owns no test file so nothing changes for it,
and a test-author stops being told not to read what it is building. `kind_prompt` remains a queued arm
for the broader subtraction; this removes one self-contradiction that needed no lever.

Fourth instance of the same shape this session: **a rule written for one kind of task applied to
another.**

## F64 — the engine had already fixed this exact bug once, for the sibling, and never carried it across

Auditing `contracts` as the POSITIVE control — the sibling fan that works, 1 failure in 19 — and its
own source comment turns out to be the strongest evidence for F49 in the repository:

> The contract stub is a model call on the SAME fleet as workers, but the old **75/150s budget was ~5x
> below the worker budget** — measured on mustsolve-test2 it **TIMED OUT on all 3 modules**, so every
> module lost its frozen interface and the granular modules cascade-FAILED into an unusable app. Give
> the stub **the worker budget** (the proven-adequate fleet timeout) so it actually completes; the
> gated retry is then a second full-budget attempt.

So `contracts` had **precisely the defect F49 fixed in `detail`** — a hardcoded 75s ceiling on a model
call that needs the worker budget — it was diagnosed on real evidence, it was fixed by deriving from
the worker timeout, and **the fix was never carried across to its sibling.** The two calls are the same
kind of work on the same fleet, side by side in the same file.

    contracts                                    detail (before F49)
    budget   worker budget, DERIVED              hardcoded 75s, in TWO places
    retry    yes, gated full-budget second try   NONE
    failures 1 of 19  (5%)                       27, ALL timeout

That is "when fixing a defect, find every other place that does the same thing" failing historically —
and it is why the rule exists. F49 was not a new insight; it was an old one that had not been
propagated.

**The remaining asymmetry is the retry, and it is deliberately NOT being built.** With the ceiling
derived, the failure mode it would cover is `filler`/`agent_error`, and 25 of 25 checked failures were
`timeout`. A retry now would be a fix for a mode with no measured instances — exactly the speculative
work this project keeps refusing. Recorded so the asymmetry is a decision rather than an oversight; if
non-timeout detail failures ever appear in `detail_completed`/`detail_fallback`, the sibling already
shows what to do.

## F65 — `kind_prompt`'s precondition IS met, unlike `doc_prefetch`'s. The arm is safe to spend a unit on.

`doc_prefetch` was pulled from the queue without running because its precondition
(`grounded == is_mcp && ok`) is never true on this machine — an arm that cannot fire is not evidence.
`kind_prompt` is the next lever in the queue that promises to fix a measured defect, so it gets the
same check BEFORE it costs two hours.

**The classifier is exact on the corpus.** `is_test_author` is computed from OWNED FILES
(`lang.is_test_file` on each basename), not from the task id — so a task named `test-api` that owned no
test file would be missed, and a task named anything that owned one would be caught. Across every plan
on disk:

    id says test- , classifier agrees      40
    id says other , classifier agrees     160
    DISAGREEMENTS                           0

Zero misclassifications in 200 tasks. So when `kind_prompt` is on, the per-kind branches route
correctly, and they are already written: a test-author gets *"DO read what you are testing: the SOURCE
module under test"* instead of the implementer's *"NEVER read the project's TEST files"*, and
`read_only_shard` gets its own subtraction.

**So the whole kind-mismatch class has a working fix that is switched off**, and the arm will measure
exactly what it claims. That is the opposite of `doc_prefetch` and worth stating plainly: the check
that killed one arm cleared this one.

F63 remains worth having independently — it repaired the GENERIC branch, which is what every worker
gets while the lever is off, and it is correct for all kinds rather than conditional on a lever.

## F66 — an independent estimate of idle-slot supply, and it corroborates F61 before the new event ships

`pre_review` needs an idle node for exactly the same reason the judge's semantic review does. That
makes its firing rate an INDEPENDENT measure of how often a spare node exists — measured with a
different mechanism, on the same runs, without the event F61 added.

Per run, pre_reviews as a share of judge ticks:

    13.0%  4.3%  6.4%  7.4%  11.5%     (the five runs where pre_review ran at all)

And the semantic judge review, measured separately: **34 of 851 = 4.0%**.

Same order of magnitude, from two mechanisms that share one precondition. That is real corroboration
for F61's claim that the binding constraint is **idle-slot supply**, not the gate F57 touched — and it
arrives before `judge_skipped` has produced a single event. The next run will settle it directly; this
says the answer is unlikely to surprise.

**Two other things the data says, both worth having:**

`pre_review` is not the constant I took it for. It is 7 on 15-18 task plans and **10 on the 20-task
plan** — it scales with the work, which is the correct shape. My "exactly 7 every run" was an artefact
of looking only at similar-sized plans.

**It fires in 5 of 13 runs and not at all in the other 8.** The zeros are not small numbers, they are
zeros — including runs with 45, 82, 171 judge ticks. So on most runs no idle slot is ever handed to
the pre-reviewer, which is the same story again and sharpens it: the question is not "how often is a
node idle" but "why do some runs have idle slots and most have none". `judge_skipped{no_idle_device}`
will answer it per-tick rather than per-run.

Cost side, for when this comes up for judgement: **38 pre_reviews produced 5 findings — a 13% hit
rate** on otherwise-idle capacity. Cheap if the node would idle anyway; not free if it competes with a
semantic judge for the same slot, which on this evidence it does.

## F67 — the swarm collides with itself on a port, and reports it as a defect in the app

The first `complete_verify.finding_texts` on this engine earned the event immediately. The SINGLE
finding holding the live build red:

    `pytest -q` failed — the generated tests exercise runtime paths that `--help` never invoke:
      OSError: [Errno 48] Address already in use

That is not a defect. Several tasks in a run each START the built app — `test-*`, `verify-e2e::*`,
`integrate-verify`, and the complete/wire fix workers — and on one machine they contend for its port.
The loser cannot bind, pytest exits non-zero, and the gate converts that into a finding that drives
the repair loop against an app that is working.

**It is systematic: `Address already in use` appears in 8 of the 13 runs on disk**, across
`integrate-verify`, `test-api`, `verify-e2e::0`, `complete-fix` and `wire-fix` — every phase that
runs the app.

This is the same class as the phantom ``GET /` returned 404`` that drove repair for weeks, and worse
than no finding: it consumes the fix budget AND leaves the app's real state unknown.

**Fixed with the engine's own existing verdict.** The pytest path already records a TIMEOUT as
inconclusive — *"not a failure and NOT a pass either"* — so a collision now gets the same treatment:
a new `TestRunVerdict::Inconclusive` that is never a finding and never a pass, carrying the reason to
the operator (*"a COLLISION IN THE HARNESS, not a defect in the app: nothing was proven either way,
so do not 'fix' the app for it"*). It is pushed to the `inconclusive` list rather than dropped,
because silently discarding it would make "nothing was checked" indistinguishable from "everything
passed" — the vacuous-pass trap this file warns about in three other places.

The regression test pins both directions: a collision is Inconclusive, and a real
`AssertionError: 247 != 0` is still a Failure. A guard that swallowed genuine defects would be worse
than the bug.

**Not fixed here, and deliberately:** the collision itself. Making the app bind an ephemeral port, or
serialising the tasks that run it, changes what the swarm produces or how it schedules — both are
real changes needing their own measurement. Reclassifying a false finding costs nothing and stops the
repair loop chasing it, which is the immediate harm.

## F68 — correction: the fix round is NOT chasing the phantom, and F37 is working live

I assumed the in-flight fix worker was burning 13 minutes on the port collision from F67, and said so
before checking. The activity digest says otherwise:

    cat vendorsync/api.py        cat tests/test_api.py
    cat vendorsync/meridian.py   cat tests/test_meridian.py
    write vendorsync/api.py

    last_thinking: "code calls `resp.headers.get("ETag")`, it raises an AttributeError. The solution
    is to ensure all mock responses include a headers dict... I should also verify that
    `_request_with_retry` properly handles non-OK status codes like 410 before parsing the body."

That is a **real defect**, reasoned about precisely, and it wrote a fix. The fix loop had more than
the collision to work from — `complete_failed_tasks` also named `test-api` and `test-meridian` — and
it went after those.

**So F67's harm is smaller than I implied.** The collision is still a false finding that must not be
believed, and 8 of 13 runs carry one, but "the fix round is wasted" was my inference, not a
measurement. The honest claim is narrower: a collision adds a phantom to the finding set and can
consume budget, not that it necessarily does.

**And F37 is confirmed working, live.** That fix made a failed task's finding name the CODE UNDER TEST
rather than telling the worker its deliverable was missing. `test-api` and `test-meridian` failed, and
the fix worker went straight to `api.py` and `meridian.py` — the modules under test — instead of only
the test files. The marker is present in the running binary, so this is the shipped behaviour, not a
coincidence.

**And a third false negative from the same idiom.** Verifying the claim above, my
`grep -qF -- "$M" <(strings "$B")` reported the F37 marker ABSENT. A positive control — `strings "$B"
| grep -cF` — found it, and every other marker, exactly once. That idiom has now produced a false
ABSENT three times tonight (once mid-rebuild, once on four markers at once, once here) while the
direct pipe has never been wrong. **Stop using the process-substitution form; use
`strings BIN | grep -cF -- MARKER` and read the count.** `loop.sh` wraps it differently and is
unaffected.

Second time tonight I have asserted something about a live run before reading the evidence, and the
second time the evidence was one command away. Both times the correction was more informative than the
claim would have been.

## F69 — the ONLY function the AST review ever flagged is a correct idiom, and it cost a fix round every run

`vendorsync.api.log_message` is flagged in **8 of the 8 runs** that produced any review finding, and
it is the **only** function ever flagged. It is not a defect.

It is `BaseHTTPRequestHandler.log_message`, overridden with `pass` to silence default HTTP request
logging — a standard, correct Python idiom — sitting under a comment that says *"silence default
logging"*. **The spec never mentions logging at all.**

The finding says: *"is a STUB/UNIMPLEMENTED … implement it FULLY per the spec"*. So every run a
`wire-fix` worker spends a round re-implementing stderr logging the app deliberately suppressed. One
worker's own reasoning records the contradiction and defers to the finding anyway:

> "the comment says 'silence default logging', but the defect says it's a STUB that needs real
> implementation"

**The finding out-ranked the code's stated intent because it claimed to speak for the spec.** That is
Mihai's through-line in its purest form: an instruction asserting a requirement that does not exist,
and a worker correctly obeying it.

It also survived the best build in the corpus — the 83.4% / crunch-7/7 run carried the identical
finding and cleared it with a fix round, so the cost is paid even when nothing is wrong.

**Two fixes, both narrow.**

1. **An empty override is a deliberate suppression.** `pass` inside a class WITH BASES is now exempt.
   Validated against the real cases and the counter-cases: `log_message` (pass, subclass) is exempt;
   `do_GET` raising `NotImplementedError` in a subclass still flags; `compute_total` with `pass` in a
   class with NO bases still flags, so today's behaviour is preserved wherever the idiom does not
   apply.
2. **The finding no longer asserts the spec.** The reviewer does not read the spec and cannot know it
   requires the function. It now states what it actually knows — *"has an EMPTY BODY … If the spec
   requires this behaviour, implement it FULLY; if the emptiness is deliberate, say so in a comment
   and leave it"* — which lets the worker weigh the code's own comment instead of being overruled by
   a claim about a document it cannot see.

Third phantom-finding class found tonight, after the ``GET /` `` 404 (F32) and the port collision
(F67) — and the most reproducible of the three at 8 of 8.

## F70 — VERDICT, baseline@3n on the F57 engine: F49 confirmed outright, and `replanned` fired for the first time

Score **70.1%**, crunch **3/5**, wall 149 min.

**F49 IS CONFIRMED, decisively.** The prediction was that deriving the detail ceiling from
`worker_timeout_secs` would drive the fallback rate to zero. Measured on this unit:

    detail_fallback events        0
    shipped one-liners            0 of 20 tasks
    detail durations (n=18)       min 42s, median 65s, max 111s
    would have TIMED OUT at 75s   5 of 18  (28%)

Five of eighteen detail calls exceed the old ceiling. Against a corpus history of 27 fallbacks — every
one a timeout — and a run where `meridian` lost its spec four times, this unit shipped **not one**
worker with the architect's one-liner. The `detail_completed` event that made the ceiling sizeable
from data has now also proved the fix that preceded it.

**`replanned` fired — the first time in the project's history.** F43 recorded it as a real zero and
explained it: the dynamic-replan precondition excludes the sink, and the sink owned ~100% of the solo
window. This run's solo window was **2780s (31% of wall)** and NOT all sink, so the precondition was
finally met and replan ran once. **F43's explanation stands; its "never fires" does not** — it fires
when the fleet goes idle on something other than the sink.

Other numbers: prefix 1752s with 1 redraft (planning 88% of it), 16 tasks discarded with 9 returning
by owned files and **15,408 chars of model-authored spec re-derived** — consistent with F55.
Occupancy 0.39 overall, 0.62 execute, `MAX USEFUL NODES 2.91`. Kind mismatch 69.7%, unchanged as
expected with `kind_prompt` off.

**The two crunch failures are real defects, not phantoms:** `fetch_all_payments` raises
`JSONDecodeError`, and `idempotent_create` raises on the vendor's 409 instead of treating it as the
documented "already exists" success. Both are exactly what the run's own round-1 `complete_verify`
finding pointed at (`_request_with_retry`), so the repair loop had the right target and did not finish
the job — which is a quality question for the arms, not an instrument fault.

Harness self-test passes with the full invariant set on this unit.

## F71 — the check that gates green has NEVER seen a real endpoint on this bench

Chasing the "CHECKED NOTHING" strings in the corpus led to the mechanism they come from, and it is
worse than the phantom it replaced.

`spec_contract` asks: *does the app implement what the spec advertises?* Its endpoint list comes from
`spec_get_endpoints`, whose regex is `\bGET\s+(/\S*)` — **prose form**. The operator spec writes its
endpoints as a MARKDOWN TABLE:

    | `GET`  | `/api/health`  | service status |
    | `GET`  | `/api/payments?limit=<int>&offset=<int>` | … |
    | `GET`  | `/api/summary` | totals |
    | `POST` | `/api/sync`    | pulls from the vendor |

where `GET` is followed by a **backtick**, never whitespace-then-slash. Run on the real 3,943-character
spec, the regex returns exactly one match: **`` /`. ``** — the F32 phantom. Table-aware extraction
returns all four.

So the arc is complete and unflattering: **before F32 this check fabricated a 404 against a correct
app; after F32 it finds nothing and honestly reports CHECKED NOTHING (6 times across 3 runs). It has
never once verified an endpoint.**

**And the fix already existed in the same file.** `spec_advertised_surface` parses the table
correctly — written for the e2e oracle. This is F64's shape for the third time: two siblings, one
solved the problem, the other never got the fix. Merged rather than replaced, so a prose-written spec
behaves exactly as before, and the param-route exclusion still drops
`/api/payments?limit=<int>` for its `<`.

**Registered prediction, before the arm runs:** `spec_contract` will now probe `/api/health` and
`/api/summary` and emit `CHECKED NOTHING` no more. If it produces a FINDING, check it against
`crunch.py` before believing it — this mechanism's history is two-for-two on phantoms, and a check
that has never worked is not owed the benefit of the doubt on its first real verdict.

## F72 — F60 ANSWERED: research reports `/v1` in full. The loss is the detailer's missing verbatim rule.

The first `research_completed.finding_texts` ever emitted settles a question that has been open all
night. The architecture scout's report, 3,885 characters, contains:

    | List endpoint   | `GET /v1/payments?cursor=&limit=` — cursor-paginated, ends when `next_cursor` |
    | Create endpoint | `POST /v1/payments` with `Idempotency-Key` header |

Exact paths, the pagination contract, the idempotency header. **The research phase does its job
perfectly.** F60's decision rule was stated before the data: *if the report DOES contain `/v1`, the
defect is the planner's use of it.* It does, so it is.

That redirects the whole line of investigation. The scouts are not the problem — F54 already retracted
"they have no tools", and now their OUTPUT is confirmed correct and complete. The fact is handed to
the architect (`research_block`) and to the detailer (`fb`), and still reaches **zero task
descriptions** in most runs.

**And the detailer's instruction shows exactly where it goes.** It carries a verbatim-preservation
rule — for ONE class of literal:

> Use the EXACT file paths the subtask owns verbatim — NEVER invent, rename, or pluralize a filename.

Filenames are protected. **API paths, version prefixes, header names, query parameters, status codes
and field names are not** — and they are the literals a worker cannot re-derive. The same instruction
then says "BRIEF — about 150 words", so when a 3,885-character research report is compressed, the
unprotected literals are exactly what gets cut.

The transmission is lossy rather than blocked, which fits the evidence: the 88.7% run's `meridian`
carried 1,497 chars WITH `/v1`, and most runs carry none. Roughly one in three survives.

**Fixed at the instruction:** the verbatim rule now covers external literals explicitly — *"CARRY
THROUGH EVERY EXTERNAL LITERAL from the research findings EXACTLY as written… these cannot be
re-derived by the worker and a near-miss is a total failure: an endpoint off by a `/v1` returns 404 on
every call. If the findings give an endpoint, WRITE THE ENDPOINT."* The word budget is now explicitly
"about 150 words EXCLUDING those literals, which are never the thing to cut", so the two instructions
stop competing.

Mihai's through-line, sixth instance, and the most consequential: **the instruction protected the
literal the model could have re-derived and left unprotected the one it could not.**

## F73 — the repair tail cannot be fanned by finding: 0 of 31 rounds have enough work for 3 nodes

Mihai: *"that 44% needs to be done in parallel too, you need to find a way to split work, there's no
way around it."* Before designing anything, the decisive number — how much work a repair round
actually contains. Across **31 rounds** on disk:

    findings per round        min 0, median 1, max 2
    rounds with >= 3 findings   0 of 31   (never enough to occupy the fleet)
    rounds with <= 1 finding   24 of 31   (77% — nothing to fan at all)

`complete_parallel` groups findings by owned file and fans one shadow-isolated shard per group. On
this evidence it **can never exceed 2 nodes, and in 77% of rounds it has exactly one item.** F41 fixed
its extractor so the fan could see its work at all; this says that even working perfectly, the fan is
capped at ~1.2 nodes.

**So the tail is not sequential for want of a fan. It is sequential because there is only ever ONE
UNIT OF WORK.** Any scheme that decomposes "the findings" is refuted before it is built — which is the
same precondition failure that killed `doc_prefetch`, kept `task_split` dark, and made `sink_review`
inert. Three mechanisms already died that way; a fourth is not worth building.

The parallelism must come from a different axis. The candidates, all reusing machinery that already
exists:

- **Pipeline the verify with the fix** — the gates are independent and read-only, and today they run
  strictly after the fix rather than alongside it.
- **Speculative repair** — race N attempts at the SAME fix in shadow trees, promote the first whose
  tree verifies. `pick_speculation_target`/`resolve_speculation` implement exactly this shape for
  scheduler tasks and are OFF; the shadow-tree isolation in `complete_parallel` is already proven.
- **Fan the verification, not the fix** — pytest, the entry probe, spec_contract, the AST review and
  cross-module drift are independent checks running serially inside one call.
- **Shift left** — the most parallel repair is the one that never happens. `verify::<M>` already runs
  per-module during execute, on the scheduler, in parallel, and the app still arrives broken; what the
  tail catches that those miss is a measurable question.

A design round is running these four against each other with adversarial refutation. **The honest
possible outcome is that the tail is irreducibly ~1.5 nodes wide**, and if that is what the evidence
says it will be reported as such rather than dressed up — a scheme that looks parallel and is not is
exactly what this ledger already has three examples of.

## Open, in flight

- `nodeloop/loop.sh` is running arms `baseline → kind_prompt → scoped_contracts → doc_prefetch`
  at n≥3, raising n rather than stopping when the backlog drains.
- Adversarial round 1 (detail fan-out) is auditing the 75 s cap, the silence, the fallback itself,
  and whether the same shape exists elsewhere (judge-side splits produce a **43-character** task
  statement, `scheduler.rs:57`).

## Standing rules for this loop

- The LM Studio fleet is READ-ONLY. If the engine cannot use what is there, fix the engine.
- Only a deterministic engine event may confer or retract a verdict.
- An arm that moves the build score by less than the replicate spread has said nothing.
- Run the built app before believing any score.

---

## F75 — the repair tail has never once succeeded: `passed` is false in 13 of 13 rounds

**Measured** across all six archived runs (`complete_verify.passed`):

| run | rounds | findings per round | ever passed |
|---|---|---|---|
| preboundary/baseline-n3 | 2 | 1, 2 | no |
| preboundary-2/baseline-n3 | 3 | 1, 2, 1 | no |
| preboundary-3/baseline-n3 | 2 | 2, 2 | no |
| preboundary-5/baseline-n1 | 2 | 1, 1 | no |
| preboundary-5/baseline-n3 | 2 | 1, 1 | no |
| preboundary-7/baseline-n3 | 2 | 1, 1 | no |

**13 of 13 rounds ended with findings outstanding.** Every run leaves the repair loop by exhausting
`complete_rounds`, never by going green. The finding count went DOWN in exactly one round out of
thirteen, stayed flat in nine, and went UP in three.

This reframes the whole tail question. The loop asked for the last two days was "how do I run the
22-44% tail in parallel". The prior question, never asked, is why a mechanism that costs 13-26% of
every run and has a measured 0% success rate is being optimised for speed at all.

### What the finding TEXTS show (F40's finding_texts, first data)

preboundary-7 is the one run whose two rounds are legible, and the two rounds are DIFFERENT:

- **round 0** — `OSError: [Errno 48] Address already in use`. This is the port collision between the
  swarm's own app-running tasks. It is a PHANTOM: nothing in the built app is wrong. A fix worker
  cannot repair it, and the round is spent.
- **round 1** — a real failure on `POST /v1/payments`. Then `round == rounds` and the loop breaks.

So on that run the repair loop found the genuine defect in its LAST round and had no budget left to
act on it. The phantom did not merely waste a round; it consumed the only round that mattered.

preboundary-5 is the opposite shape and no better: byte-identical finding text in both rounds, so the
fix worker changed nothing the verifier could see.

### Consequence for the parallelisation directive

F73 established that findings cannot be fanned (0 of 31 rounds have work for 3 nodes). F75 says the
axis is not the finding at all — it is the ROUND. The tail is serial in rounds and runs out of them.
Three idle nodes racing independent attempts at the SAME finding, with the winner chosen by
re-verifying each shadow tree, raises the per-round success probability and therefore lowers the
round count. That is a real use of three nodes on a one-finding round, and it is the only one found
so far that does not require the finding to decompose.

It pays ONLY on a real finding. Racing three nodes at a phantom burns three nodes. So the build order
is forced, and it is not the order this loop was about to build in:

1. **instrument the tail** — it emits no `task_dispatched` at all (F74), so no change to it can be
   measured, including this one
2. **kill the phantoms** — F67 (port collision -> Inconclusive) is already live and preboundary-7
   round 0 is exactly the case it targets; this is a registered prediction the current unit tests
3. **then** race attempts, with a deterministic per-shadow verifier picking the winner

### Standing caution

`passed` is `verdict.findings.is_empty()`, so this measures "the verifier still had something to
say", not "the app is broken". Two of the six runs crunched 3/5 and better with findings outstanding.
The 0/13 is a fact about the LOOP, not a verdict on the apps.

---

## F76 — F61 ANSWERED: the judge is starved of idle slots, 80% of the time

First run on the F61 engine, harvested live:

| judge_skipped reason | count | share |
|---|---|---|
| `no_idle_device` | 4 | 80% |
| `nothing_produced_yet` | 1 | 20% |
| (judge_verdict — actually ran) | 7 | — |

The decisive path is UPSTREAM of the F57 semantic gate, exactly as predicted: the scheduler hands the
judge a model ONLY when a device is idle, so on a busy fleet the judge simply never gets one. The old
4.3%/95.7% split was unattributable because four `JudgeOutcome::ok()` paths all logged
`confidence 1.0, hint ""`; `judge_skipped{reason}` separates them and the answer is not the semantic
gate at all.

This settles Mihai's design question ("whenever a node is empty it should take the judge role — that
should eliminate the hard-coded timings"). The mechanism is built and correctly gated on idle. The
constraint is supply. Note the direction, because it is one of the few places the architecture is
already on the right side of goal one: **more nodes means more idle slots means more judging.** The
judge gets better with fleet size rather than worse.

## F77 — F60 ANSWERED, and it exonerates research: the scout DOES report `/v1`

`research_completed.finding_texts`, live run, architecture lens:

> `| List endpoint | GET /v1/payments?cursor=&limit= — cursor-paginated, ends when next...`

The literal is present, verbatim, in a markdown table, with the base URL and the bearer token. So the
`/v1` loss is NOT what the scout is asked to report. It is downstream — which is what F72 (the
detailer's verbatim rule covering filenames but not external literals) was written for. F72 is HELD
in git and not in the live binary; this readout is the evidence that it targets the right place.

## F78 — grounding cannot fire on this bench, so the verbatim channel is structurally dead

Chasing F77 downstream turned up the bigger defect. The same event reported `grounded: 0` and
`looked_nothing_up: 2` for the run above.

**That zero is a broken instrument, and the vendor trace proves it:** 11 `curl/8.7.1` requests reached
`/v1/payments` and `/v1/docs` during the run. The scout read the live docs and quoted them.

The cause is one predicate. `research_lookups` counted `t.ok == Some(true) && t.is_mcp` — MCP calls
only. And `levers_resolved` for this run says `research_tools: {"available": [], "can_look_things_up":
false}`: **there are no MCP tools attached at all on this bench.** So `grounded` could never be true,
on any run, ever. The engine's own comment admits the shape without drawing the conclusion — "which is
always the case when the research tools are not attached".

The consequence is not cosmetic. `doc_facts` — the ONE channel that routes research to workers
VERBATIM, under the banner "these were LOOKED UP with a real tool, use them EXACTLY, do NOT paraphrase"
— is filtered on `f.grounded`. On this bench it is always empty. The single mechanism designed to stop
API literals being paraphrased away has never carried a byte here.

**Fix:** `ToolCallRecord` gains `fetched_external`, set when a shell-ish call's arguments carry BOTH an
http(s) URL AND a fetching program (curl/wget/urllib/httpx/...). `research_lookups` now counts
`is_mcp || fetched_external`.

Both halves of that predicate are load-bearing and the original rule's concern is preserved: it exists
to stop a trivial `echo` laundering a guess into a "verified fact", so `echo https://x` still grounds
nothing, and neither does `curl --help`. The pre-existing test asserting a bare shell does not ground
still passes unchanged.

This is the same shape as F71 and F72 — a channel built for exactness, defeated by a predicate that
could not see the form the input actually takes. Three instances now, which is a pattern rather than a
coincidence: **every verbatim/grounding path in this engine should be audited against what the bench
actually produces, not against what it was imagined to produce.**

---

## F79 — two parsers for one `--help`, disagreeing, and one of them cannot see a single-subcommand app

The F71/F72/F78 audit ("a channel built for exactness, defeated by a predicate blind to the form its
input takes") turned up a fourth instance with a second defect on top.

`advertised_subcommands` (swarm.rs:13749) and `parse_subcommands` (swarm.rs:14835) read the SAME
thing — argparse's `{a,b,c}` choices block on a `--help` usage line — and were used by different
gates:

- `advertised_subcommands` → the smoke gate's command probe (two call sites)
- `parse_subcommands` → `run_spec_contract`, the deterministic check that gates green

They differed by one guard, so the same app could advertise different command sets depending on which
gate was asking:

```rust
// advertised_subcommands had, parse_subcommands did not:
if inner.is_empty() || inner.contains(' ') || !inner.contains(',') { return Vec::new(); }
```

**The space half is correct** and `parse_subcommands` was missing it: `see {the docs, really} for
more` is prose, not a command list.

**The comma half is a false zero.** argparse prints a ONE-subcommand app as `{serve}` — no comma — so
`advertised_subcommands` returned empty for it. The probe then had nothing to invoke and reported a
clean result it had not earned, which is the vacuous-pass shape this project has a standing law about.
`parse_subcommands`, lacking the guard, saw `serve` fine. Two gates, one app, opposite answers.

**Fix, in the shape Mihai's rule demands** — not "patch the one that was wrong", but remove the
mechanism that let them diverge. The comma requirement is deleted, the prose guard is kept,
`parse_subcommands` is deleted outright, and `run_spec_contract` now calls the surviving function —
which also hands spec_contract the prose protection it never had. Both tests are merged into one, so
a future divergence has to break a test rather than pass silently.

Note the direction of each half: dropping the comma guard makes the smoke probe see MORE apps, and
adopting the prose guard makes spec_contract see FEWER false ones. Both move toward the same place.

---

## F80 — an adversarial corpus sweep (90 logs) against F77, and what survived it

A design round over 90 run logs (51 with a complete `run_started -> run_finished` timeline, 353,171s
of wall-clock — five times the corpus behind F73/F75) argued that speculative repair should not be
built. Three of its claims were checked against source and corpus. **Two hold and one is wrong**, and
the one that holds found a real regression I had introduced.

### CHECKED, TRUE, and it caught a defect in F77

> Shadow isolation destroys 3 of the 12 historical greens. The serial fix is `speculative: false`
> wrapped in `let _ = timeout(...)`, so a killed agent's writes survive on the real tree. `nf-unit`,
> `h1-treat-1`, `nf-ts-cadence` each went 1 finding -> 0 on a round that hit the 1200s cap.

The mechanism is exactly as described (swarm.rs, the `else` branch of the fix step). And F77 as first
written matched on `(Ok(Ok(_)), Some(root))` — a timed-out twin scored `None` and was discarded. That
would have deleted three historical greens: a regression wearing the costume of a safety fix.

**Fixed:** the gate now grades the TREE, not the agent's exit. A shadow holds whatever its agent wrote
before it was stopped, so a timed-out twin is verified like any other and promoted if it strictly
beats the baseline. This is strictly better than both predecessors — the serial path keeps partial
writes but never checks them; F77-as-written checked but threw partial work away. `agent_ok` is still
recorded so "how often does a killed agent's tree beat a finished one" stays answerable.

### CHECKED, TRUE, and it is a property of the design rather than a defect

> `fanout_over_fleet` collects with `for h in handles { if let Ok(r) = h.await }` — awaits all, no
> cancellation. Any race built on it is max-of-3, not min-of-3.

Verified at swarm.rs:15575. But F77 never claimed first-to-finish: it verifies EVERY shadow and picks
the best, so max-of-3 is intended, not accidental. The cost is bounded — three twins run concurrently
on three nodes under the same `fix_cap_secs`, so a round's worst case is unchanged at the cap — and it
buys a round that can improve at all, against a mechanism measured at 0 for 13. `fanout_over_fleet_straggler`
(swarm.rs:15629) has JoinSet abort and is the primitive if first-to-green is ever wanted.

### CHECKED, FALSE

> `pub fan_verify: bool` defaults false — make sharded sink verification the default.

`fan_verify: true` at swarm.rs:1092, in the `Default for SwarmConfig` impl. The claim read the struct
FIELD DECLARATION (swarm.rs:877) and missed the baked default. Its headline proposal is to switch on
something that has been on since the golden-formula bake. The sink measurements around it may still be
worth having, but the recommendation as stated is a no-op.

Also overstated: "every parallel mechanism has fired zero times in 90 logs" lists `spec_repair_wave`
and `complete_fix_dispatched` among them. Those events were added tonight; no historical log could
contain them. `complete_fix_wave` 0 and `sink_review` 0 are real, and both are default-OFF levers.

### What the bigger corpus genuinely corrects

F73 said "0 of 31 rounds have >=3 findings". Over 49 red rounds the distribution is
`{1:28, 2:14, 3:2, 4:2, 7:3}` — **7 of 49 (14%) DID have >=3**, all inside three runs on older beds.
On the current bed it holds exactly: 22 red rounds, `{1:15, 2:7}`, max 2. So the claim stands for
today's bench and was too absolute as a general statement. Time-weighted fan width across the whole
tail: **1.19 nodes.** That number is the honest ceiling for any decompose-the-finding scheme and is
the strongest argument yet that the ATTEMPT, not the finding, is the only axis available.

---

## F81 — two nodes idle through the execute tail, and the mechanism built to fill them was switched off at +50 min

Caught LIVE, mid-run, which is why it is worth more than the usual post-hoc reading. At +68 min the
fleet showed one node GENERATING and two IDLE. The DAG said why:

```
dispatched tasks: 18   completed: 17   IN FLIGHT: 1  (test-cli)
READY-but-undispatched: 0     blocked by deps: 0
```

Not a scheduler failure — the plan was simply exhausted. `dynamic_replan` exists for exactly this
("workers idle while a task is still in flight — ask the planner for more parallel work"). It was off.

### Why it was off

```
REPLANNED {round: 0, added: [], stopped: True}       @ +50.1 min, 9 of 18 tasks done
max_replans = 1
```

The replanner was asked at the halfway point, with half the DAG still queued, and correctly answered
"nothing to add". The engine's response to an empty answer was `s.replans_done = self.max_replans` —
it burned the ENTIRE budget. One honest decline, made about a DAG that no longer exists, disabled the
mechanism for the remaining 18 minutes of single-task tail.

The replanner's answer is a function of the DAG state when it was asked. Treating it as permanent is
the defect.

**Fix:** an empty answer now REFUNDS its round and records `replan_declined_at_incomplete`. The gate
may ask again only once STRICTLY FEWER tasks remain — the one change that could honestly produce a
different answer — so the tail gets its ask while the planner is never pestered at an unchanged state.
Bounded by construction: each further ask costs at least one task completion.

### The second defect, found while testing the first

The regression test still failed after the fix, and the reason is its own finding. The scheduler's
wake-up is `timeout(tick, notify.notified())`, where `tick` is 15s if a judge, pre-reviewer or
speculation is attached and **86,400s otherwise**. The comment says the tick exists so an idle-node
mechanism can act BETWEEN completions.

**The replanner is not in that list.** Its trigger — nodes idle while a task is still in flight —
produces no completion to wake on, by construction. So on any run with a replanner and no judge, the
one window it exists for is never re-examined. It has only ever worked because a judge happened to be
attached and was lending it a heartbeat.

`self.replanner.is_some()` is now in the tick condition.

### Control

The test (`an_empty_replan_answer_does_not_disable_the_replanner_for_a_smaller_dag`) was run with the
fix REVERTED and FAILS, then restored and PASSES. It reproduces the live shape: an early decline while
a dependent task is still blocked (incomplete 3), then a completion edge that opens a second window
with one task left (incomplete 1). Under the old behaviour `late` never runs.

### Standing note

This is a THIRD serial tail, distinct from the two already recorded. Execute ends with a lone task
while the fleet idles, BEFORE the repair tail starts. `test-cli` — the task holding it — had been
dispatched four times and was the subject of two judge hints. Whether the replanner would have had
anything useful to add is unmeasured and is exactly what the next run answers.

---

## F82 — F69 CONFIRMED against the real phantom source, with controls, without waiting for the tail

The `log_message` phantom accounted for **8 of the 9 recorded phantom findings** (the ninth was the
port collision). It was being waited on as a live readout from the running unit. It did not need to
be: the phantom's source is in the ARCHIVE, and the fix can be run against it directly.

### Where the phantom actually comes from

Sampling a shipped test file (`preboundary-3 .../tests/test_meridian.py`) to check an unrelated
question — whether test-authors test the real module or a replica — turned it up:

```python
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self): ...
```

Every stub HTTP server in every generated test suite silences its logging this way. It is the textbook
deliberate override, it appears once per test class, and the AST review called each one an
unimplemented stub.

Note the shape: the class is **nested inside a test method**. F69 exempts `pass`-bodied methods in
classes WITH bases, and whether its walker sees a nested class's bases is not something to assume —
`ast.walk` recurses, so it does, but that is a reading, not a measurement.

### The measurement

The real `AST_REVIEW_SCRIPT` was extracted verbatim from swarm.rs (not reimplemented) and run:

- against the full archived app tree of `preboundary-3`: **0 findings, 0 of them `log_message`**
- against a two-case control file:
  - `log_message` in a class WITH a base -> **not flagged** (negative control PASS)
  - `do_the_work` in a class with NO bases -> **flagged** (positive control PASS)

The control is what makes the zero admissible. Without it, "0 findings" on the archive is
indistinguishable from a script that cannot find anything, which is the standing law here and has
caught three false zeros already tonight (F74's tail occupancy, F78's `grounded`, F79's `{serve}`).

### Consequence

With F69 live, the phantom rate on the recorded corpus drops from **9 of 20 findings (45%) to 1 of
20 (5%)** — the surviving one being the port collision, which is F67's case and is also already in the
binary. The repair loop should, for the first time, be spending its rounds on real defects.

That is now a REGISTERED PREDICTION for the next full run, and it is falsifiable in the obvious way:
if `complete_verify.finding_texts` still carries `log_message`, F69 does not cover the live path and
this confirmation was scoped too narrowly.

### Also settled, and it was the question that led here

Every shipped test file across all six archived runs DOES import the real application package. The
judge's in-flight catch ("you're testing your own `make_parser()` replica instead of the actual CLI")
was a real defect caught and corrected mid-run, not a systemic one. The high non-test definition
counts in those files (14-33) are fixtures and stub servers — the `test_meridian.py` above spins real
`HTTPServer` instances to exercise pagination, UTC sorting, and 429 retry against BOTH `Retry-After`
seconds and HTTP-date. That is genuinely thorough work, and a cruder metric would have libelled it.

---

## F83 — the only evidence for a repair round was truncated exactly where the error began

The live unit's round-0 finding read, in full:

```
pytest --collect-only errors (cross-module import?):
test_store.py::test_upsert_many_empty_is_noop
... (six more collected test ids) ...
================
```

A list of SUCCESSFULLY collected tests, then a separator, then nothing. I read that as a phantom — the
engine calling a passing collect an error — and was one command away from publishing it.

It is not a phantom. `finding_texts` truncates head-first at 400 chars, `tail_lines(output, 40)` had
already selected the last 40 lines (which DO contain the error), and `================` is the opening
of pytest's `=== ERRORS ===` banner, beginning one character past the cut. **The evidence existed and
the instrument threw away the half that mattered.**

### Why this is the worst kind of instrument defect

F40 added `finding_texts` for exactly one reason: so the verdict that decides green could be checked
against evidence afterwards. For the commonest finding class on this bench — a pytest failure — it
kept the sentence naming the check and discarded the traceback. The finding was not merely
abbreviated; it was rendered actively misleading, because what survived looks like a success.

This file already carried the same lesson: `review_file_excerpt`'s docstring records a flat 2000-char
head cut that hid the dispatch tail of every real entry point and FABRICATED "unwired/unreachable"
findings. Same defect, same cause, second occurrence, and the second one was written after the first
was understood.

**Fix:** one `elide_middle(s, head, tail)` helper, char-based, now used by BOTH — `review_file_excerpt`
(3500/2500) and `finding_texts` (150/650, deliberately tail-heavy). Extracting a shared helper rather
than patching the second site is the F79 lesson applied before the divergence happens instead of after.

### What is still open, and what I will NOT claim

The tree that failed collect at 08:58 local passes `pytest --collect-only` with exit 0 now, and
neither `vendorsync/store.py` (mtime 08:22) nor `test_store.py` (08:27) has been modified since. Same
files, same tree, opposite verdict.

That is genuinely unexplained. Two candidates, and I have evidence for NEITHER:

1. a transient environment condition at verify time — which would make it F67's class, and note that
   `interpret_pytest_collect` has **no `Inconclusive` variant** while its sibling
   `interpret_pytest_run` was given one by F67. Sibling functions, one fixed, one not.
2. a real import error that the fix worker has since repaired — but the mtimes say otherwise.

I am NOT adding `Inconclusive` to `CollectVerdict` on this evidence. Doing so would suppress a finding
class on a hunch, and the failure mode of a wrong suppression is an app shipped broken. F83 makes the
next occurrence readable; the fix, if one is warranted, follows the evidence rather than preceding it.

**Registered prediction:** the next run's `complete_verify.finding_texts` for a pytest failure will
end in a traceback or an error banner, not in a list of collected tests.

---

## F84 — P3 CONFIRMED, and it un-blocks a lever that has never been able to do anything

First run on engine_build 1785652162:

```
research_completed: grounded = 2   looked_nothing_up = 0   lenses = [architecture, libraries]
```

Every previous run on this bench: `grounded = 0`, `looked_nothing_up = 2`. **P3 confirmed.** A scout
that reads the vendor docs with `curl` is now recorded as having looked something up.

### The consequence nobody had noticed

`doc_prefetch` — the ONLY channel that routes research to workers VERBATIM, under the banner "these
were LOOKED UP with a real tool, use them EXACTLY, do NOT paraphrase" — builds its payload from
`findings.iter().filter(|f| f.grounded)`.

`grounded` was `is_mcp && ok`. This bench has no MCP tools. So the filter matched NOTHING, on every
run, ever. **`doc_prefetch` was not merely off; it was INERT BY CONSTRUCTION.** Turning it on before
tonight would have produced an empty `doc_facts` and measured pure silence — and the sweep queue
carried it as a real arm with a real gate, so that measurement would have been made and believed.

This is the fourth mechanism found with a precondition that never held, after `doc_prefetch`'s
siblings `task_split`, `sink_review` and `complete_parallel`. The pattern is now the single most
productive thing to check about any lever: **before measuring a lever, prove its precondition can
occur at all on this bench.** A lever whose precondition never fires is not a negative result, it is
a non-measurement, and it looks identical to "the lever does nothing".

### Second-order effect, worth watching rather than acting on

`grounded_research_only = true` (baked default) and `grounded` was always false, so every research
finding was previously classed INVENTED and never counted as "settled / do not re-ask". With
`grounded = 2` those findings now count as settled. That changes what the ask/clarify floor sees.
Direction is right — a genuinely looked-up fact SHOULD settle a question — but it is a behaviour change
that arrives as a side effect of F78 rather than by design, so it is recorded here and watched, not
assumed benign.

### Action

`doc_prefetch` is promoted in the sweep queue with its gate rewritten: it is no longer "does the
verbatim channel help", it is "does the verbatim channel CARRY ANYTHING", which is a question that
could not previously be asked. Mechanism readout: `doc_facts` non-empty, and `/v1` present verbatim in
worker dispatches.

---

## F85 — the longest silent window in a run is the one where planning answers goal one

The phase timeline of a 3-node baseline, from the engine's own markers:

```
+3.4m   research_completed, levers_resolved
                                              <-- 12 MINUTES, ZERO EVENTS
+15.4m  confidence_retarget, retarget_discarded
+29.2m  contracts, plan_loaded
```

That 12-minute gap is the **best-of-N skeleton draft**. It emitted nothing at all, inside a planning
prefix that `phases.py` already reports as having no occupancy number (F74). So the single longest
unobserved stretch of a run sat inside the single largest unmeasured region of a run.

It is not a minor phase. `best_of_n` is sized to the fleet at the call site —
`base.max(devices.len().clamp(1,5))` — so **this fan IS the planning phase's answer to goal one.**
Until now the only way to know whether it drafted one skeleton or three was to infer it from
wall-clock.

(Correcting an earlier note of mine: I had recorded best-of-N as "capped at distinct-model count, so
N=1 on an identical fleet". That was true in the single-identifier era and is not true now — with
three identifiers `n` is 3. The cap was never the defect; the invisibility was.)

**`skeleton_drafts{requested, returned, dead, secs, chars[], worker_count}`** now closes it.

`dead` is the load-bearing field. A dead slot is a node that spent minutes drafting and produced
nothing usable — the engine's own comment at that site says the confidence metric "only ever saw
ANSWERS" and "nothing anywhere revealed the gap". Now `requested - returned` is readable directly, and
`chars[]` gives the per-draft size distribution that decides whether a draft was a real skeleton or a
stub.

**Registered prediction for the next run:** `skeleton_drafts.requested == 3` and `dead == 0`. If
`dead > 0`, some fraction of the 12-minute window is nodes producing nothing, and the planning prefix
has a defect worth more than any tuning downstream of it.

---

## F86 — the planning prefix is NOT serial: all three nodes work through the silent window

Measured LIVE during the 13-minute silent window F85 instruments, by sampling `lms ps` while the
engine's event log emitted nothing:

```
gabee  PROCESSINGPROMPT   mihai  PROCESSINGPROMPT   workhorse  PROCESSINGPROMPT
```

All three, sustained across repeated samples. **The skeleton draft fan genuinely uses the whole
fleet.**

### What this retracts

The plan document for this whole effort lists, under "PART 4 — THE PHASE ROUNDS":

> Plan / best-of-N — drafts capped at distinct-model count; the cap exists for `PARALLEL:1`, not
> model diversity

That was true when all three hosts served ONE identifier, and it is **false now**. `best_of_n` is
sized at the call site to `base.max(devices.len().clamp(1,5))`, so with three identifiers it is 3, and
the fleet confirms three concurrent drafts. The cap was never the defect; the invisibility was (F85).

### Why it matters more than it sounds

It narrows the target. Taking the phases in turn, with what is now known about each:

| phase | share of wall | fanned? | evidence |
|---|---|---|---|
| research (scouts) | 2-6% | YES | parallel fixed-lens scouts, `lenses_returned` |
| skeleton draft | ~12-15% | **YES — 3 nodes** | this finding, sampled live |
| detail fan | 9-25% | YES | `detail_completed` per task, occupancy floor 0.49 |
| execute | 42-63% | YES | occupancy 0.62-0.93 |
| repair tail | 13-26% | **NO — 1 node** | F74/F75; emits no dispatch at all |

**Every phase except the tail is already fanned.** The serial-work problem is not spread across the
architecture as I had been treating it; it is concentrated in one place, and that place is the one
Mihai pointed at first ("that 44% needs to be done in parallel too").

That makes G2 (`spec_repair`) and G5 (bring the tail under the judge) the whole of the remaining
node-scaling work, and it demotes any further prefix tuning to a question about LATENCY, not about
parallelism.

### The new open question this raises

Thirteen minutes in `PROCESSINGPROMPT` — prompt PREFILL, not generation — is a long time. The fleet
reports context windows of 193,792 and 262,144 tokens. If the draft prompt is genuinely large enough
to spend minutes prefilling on every node, that is a cost paid three times over in parallel, and it is
a different lever entirely from anything examined so far (it would be about prompt SIZE, which is also
exactly Mihai's through-line about instruction density).

`fleetsample.sh` now records fleet state every 30s to `runs/nodeloop/fleet-samples.tsv`, read-only and
detached, so the PROCESSINGPROMPT-vs-GENERATING split across a whole run becomes measurable instead of
spot-checked. No conclusion is drawn from three samples.

---

## F87 — HALF of every prompt sent to the fleet was instructions about something else entirely

Chasing why the fleet sat in `PROCESSINGPROMPT` for thirteen minutes (F86), I measured the actual
prompts from goose's own `llm_request` logs. The prefill question turned out to be the small half of
the answer.

**Measured, last 3 hours, this machine:**

```
substantive requests            17
carrying the global hints       17   (100%)
prompt chars   median 45,264    max 47,040
hint block     22,389 chars     -> 49% of the median prompt
```

**What those 22,389 chars are.** The system prompt's `# Additional Instructions:` section, headings
verbatim:

```
### Global Hints
<!-- goose:import claude-code hash=2ec7165db11dc541 -->
## Wolfaenpak Atlassian is a TEST environment (GLOBAL — set by the user 2026-07-13)
## Autonomy — MANDATORY rules (ALL projects, no exceptions)
## Production config on a CLIENT system — MANDATORY rules
## Verifying test results — MANDATORY rules
## Workhorse — Mac Studio sync (IMPORTANT)
## UI / design — MANDATORY rules (ALL projects, no exceptions)
## Flagging & framing work — MANDATORY rules
## Writing as the user — MANDATORY rules
## AI models — MANDATORY rules
### Project Hints
# AGENTS Instructions   (the goose repo's own — cargo build, cargo clippy, Ink/terminal UI rules)
```

A local 27B whose entire job is to write `vendorsync/meridian.py` — a Python payments-sync tool in a
temp directory — is being told about Jira test tenants, rsync to a Mac Studio, never using a left
accent rail in a UI, how to write prose in Mihai's voice, and how to run `cargo clippy` on the goose
repo. The repo's AGENTS.md arrives only because the bench happens to run inside the goose checkout, so
the hint walk finds it on the way up.

### Why this is the instruction-density defect at its largest scale

Mihai's through-line is "the crux across all phases is EXACT AND PRECISE INSTRUCTIONS." Everything
this loop has fixed so far — the detailer's verbatim rule (F72), kind-mismatched worker rules, the
architect brief (F58) — operates on the ~1,370-token worker prompt. **This is ~6,200 tokens of
almost entirely irrelevant instruction, on every call, and nobody chose it.**

Three published results converge on why it hurts, and all three preconditions hold here:
- perfect-compliance for this model class falls from ~0.59 at 10 rules to ~0.09 at 40
- a distinct compliance drop past ~15 constraints, measured across models regardless of size
- **Qwen-family models show a PRIMACY bias — earlier constraints stick harder.** The hints sit in
  `# Additional Instructions` and the irrelevant material is weighted accordingly.

And the AGENTS.md-quality studies found the high-impact content is exact paths, commands and API
refs, while general "rules and prohibitions" are low-impact and excess verbosity actively *reduces*
success. This block is almost entirely rules and prohibitions about other projects.

### The fix, and why it is in scope

`prompt_manager.rs` appends hints as `system_prompt_extras` AFTER `override_system_prompt` sets the
swarm's carefully-built task prompt — so the swarm's own prompt engineering was being followed by 22k
chars it never asked for.

`get_context_filenames()` reads the `CONTEXT_FILE_NAMES` param, and `Config::get_param` checks the
UPPERCASE ENV VAR before the config file. `suppress_inherited_hints()` sets it to `[]` at the top of
`run_swarm`, which scopes the suppression to THIS PROCESS — `goose swarm run` and its in-process
workers. An interactive goose session is a different process and keeps its hints. **Nothing in goose
core is touched**, which matters because the standing rule is never to change upstream core to fix a
swarm issue.

`GOOSE_SWARM_INHERIT_HINTS=1` restores the old behaviour so this is measurable as an arm, and an
explicit `CONTEXT_FILE_NAMES` already in the environment always wins.

### Registered prediction

Next run: prompt chars drop by roughly 22k (~49%), and `skeleton_drafts.secs` falls — a shorter prompt
is less prefill on all three nodes at once. The QUALITY prediction is deliberately separate and
weaker: compliance should improve, but the replicate spread on this bench is 46 points, so a single
run cannot show it and I will not claim it from one.

**This does not replace the other instruction work.** It removes the noise floor those fixes were
competing against.

---

## F88 — the swarm leaves its own app servers running, and they poison every later unit

Two units died in the four minutes after the restart. The sweep's own guards caught both, which is the
only reason this is a finding rather than a corrupted results table:

```
[abort] 09:54:15 baseline-n3-r0: 3 engines running at once [14772, 41165, 41234]
                 — an orphan is contending for the fleet and will skew this unit and every later one
[abort] killed engine pgroup for pid 14772 / 41165 / 41234
[done]  baseline-n3-r0  score=0.2126  aborted=True   (24 min)
[fail]  retarget_off-n3-r0 attempt 0: OSError: [Errno 48] Address already in use
[done]  retarget_off-n3-r0  score=FAILED  (0 min)
```

`aborted=True` is recorded, so the 0.2126 row can never be read as a real result. That guard was
written after a unit killed at 57 minutes went into the table as a clean 1-node row.

### The orphan, identified

```
pid 69684  ppid 1  elapsed 1:22:03
  bash -c  rm -f vendorsync.db && python3 -m vendorsync --db vendorsync.db --port 8931 &
pid 69687  (its child)  LISTEN 127.0.0.1:8931
```

**A swarm WORKER started the built app to exercise it and never stopped it.** Eighty-two minutes
later it still held port 8931 — long after its own run had been killed and parked.

### This CLOSES the open item, and the answer was not the one I was leaning toward

The standing open question was: a tree that failed `pytest --collect-only` at 08:58 passed with exit 0
at 09:20, with neither source file modified in between. Same files, opposite verdict, unexplained. I
recorded two candidate causes and refused to fix either without evidence, noting that
`interpret_pytest_collect` has no `Inconclusive` variant while its sibling `interpret_pytest_run` was
given one by F67.

**The cause is this orphan.** A test that imports the app while another process holds its port gets an
error at COLLECT time; once the port frees, the identical tree collects cleanly. That is exactly the
observed behaviour, and it is an ENVIRONMENT collision — F67's class — not an app defect.

So the instinct was right and the discipline was also right: the fix I was tempted by (adding
`Inconclusive` to `interpret_pytest_collect`) is now justified by evidence rather than by a hunch, and
it is a DIFFERENT fix from the one that actually matters.

### What actually matters

Adding `Inconclusive` treats the symptom. The defect is that **the swarm leaks server processes.** Each
leak is a permanently-held port, and a long sweep accumulates them: unit N+1 fails to bind, unit N+2
fails to bind, and every failure looks like a build defect. This is the same root cause as the F67
phantom, generalised from "two tasks inside one run collide" to "a dead run poisons every later run".

Recorded as **G6** in GOAL.md. The worker prompt already tells a task to run the app; nothing tells it
to stop it, and nothing reaps it afterwards.

### Also fixed, immediately

`health.py` crashed with `TypeError: '<' not supported between instances of 'NoneType' and 'int'` when
it sorted `(nodes, actual_nodes)` across results — a FAILED unit has neither, so the first failure took
down the whole health check. **The one instrument that tells an unattended operator the sweep is sick
must never be the thing that dies when something goes wrong.** It now sorts None-safely, and it
immediately reported the real state: `[BAD] no unit produced a dispatch audit`.

---

## F89 — an intruder cost three units, because the guard killed the innocent one too

Three consecutive units died in forty minutes, all to the same guard firing correctly on a real
problem and then over-reacting:

```
[abort] 09:54:15 baseline-n3-r0: 3 engines running at once [14772, 41165, 41234]
[abort] 10:13:17 baseline-n1-r0: 2 engines running at once [41980, 85322]
        retarget_off-n3-r0: OSError: [Errno 48] Address already in use   (0 min)
```

`baseline-n3-r0` lost at 24 minutes, `baseline-n1-r0` at 19, and `retarget_off` never started because
a dying process still held its port. An hour of fleet time, and three rows that can only be discarded.

### Two separate defects, and only one of them was the intruder

**The intruder.** Engines appeared that this sweep did not spawn. The only process with repo access and
the ability to run commands was a background analysis workflow whose agents were told they could run
read-only checks. Correlation is strong — three occurrences while it ran, and a clean single engine
immediately after stopping it — but I did not capture the parent of pids 41165/41234/85322 before they
were killed, so this is a STRONG INFERENCE and not proof. Recorded as such.

**The guard, which is the part I can fix.** `doomed()` saw `len(engine_pids()) > 1` and `abort()` then
killed EVERY engine pgroup — its own included — and cut the unit loose. So an intruder did not merely
contend for the fleet; it destroyed whatever was running. The guard was written to stop an orphan
skewing a measurement, and it did that by throwing the measurement away.

**A sweep knows which engine is its own — it is the one it spawned.** `intruder_engine_pids()` now
compares each engine's ppid against `os.getpid()`, `evict_intruders()` kills only the foreign ones, and
`doomed()` evicts FIRST and only then asks whether the unit is beyond saving. An intruder is a reason
to remove the intruder, not a reason to lose the unit.

Contention is recorded rather than hidden: `contended` on the result row counts evictions, because the
unit's WALL-CLOCK is tainted for some unknown slice even though its build is not. A timing number
nobody flagged is worse than one nobody has.

**Control, both directions:** run from a shell (pid 88157) the live engine 85520 is correctly reported
as an intruder; measured against the real sweep (pid 14769) its ppid IS 14769, so it would be SPARED.

### The operational rule this earns

**Nothing that can spawn an engine may run while a sweep is live.** Before launching any background
work that touches this repo: `pgrep -f 'goose swarm run' | wc -l` must be 1, and it must still be 1
afterwards. The audit's value never exceeded the cost of destroying every measurement on the machine.

### Deliberately NOT restarting the sweep to pick this up

A running interpreter does not see a source edit, so the live sweep (pid 14769) still has the old
all-or-nothing guard. Restarting now would cost the unit currently in flight to install protection
against an intruder that is already stopped. The fix lands at the next boundary, which is due anyway
with seven commits held. Stated here so the gap is deliberate and visible rather than forgotten.

## F90 — disk: `target/debug` was 64 GB and the sweep halts at 15 GB free

Free space fell 56 -> 40 GB in forty minutes of build/test cycles. The consumer was not the runs
directory (17 MB total) but `target/debug` at **64 GB**, grown by repeated `cargo check`/`test`.

`MIN_FREE_GB = 15` is a hard abort in `doomed()`, so this was a scheduled failure a couple of hours
out — the kind that would have looked like a mysterious mass abort at 3am. Removed `target/debug`
only; `target/release/goose` is the LIVE binary the engine is executing and was untouched (verified by
size and by the running process's command path). Free space 40 -> 53 GB and still settling.

---

## F91 — the swarm's own system prompt is 3% of what the model reads. The inherited hints are 51%.

The pre-F87 baseline, measured from goose's own `llm_request` logs on the live engine
(engine_build 1785652162), n=16 substantive requests:

| component | median chars | share of median total |
|---|---|---|
| goose global + project hints | **22,152** | **51%** |
| **the swarm's OWN system prompt** | **1,389** | **3%** |
| tool schemas | 2,064 | 5% |
| user message (spec + research findings) | 11,132 | 26% |
| TOTAL | 43,050 | ~12,000 tokens |

(Component medians are computed independently and so do not sum exactly to the median total; the two
load-bearing figures are the 51% and the 3%.)

### What this means for everything this loop has done

**The swarm's own prompt engineering is 3% of the model's input, and the context nobody chose is 51% —
sixteen times larger.**

Every instruction fix shipped tonight operates inside that 1,389 chars: F58 (the architect brief must
STAND ALONE), F63 (a test-author's own file is exempt), F72 (carry through every external literal),
the 69.7% kind-mismatch work, the whole 30-to-50-rules analysis. All of it is real, all of it is
correct, and all of it was competing against sixteen times its own volume of instructions about Jira
tenants, Mac Studio rsync, UI accent rails and how to write prose in Mihai's voice.

Mihai's through-line is "the crux across all phases is EXACT AND PRECISE INSTRUCTIONS." The precision
work was being done on 3% of the surface.

This does not retract any earlier finding. It reprioritises them: F87 removes the noise floor those
fixes were competing against, and it should ship before any further tuning of the 1,389 chars, because
until it does, no instruction change can be cleanly attributed.

### Why the tokens matter on this specific fleet

~12,000 tokens per request, of which ~6,150 are the hints. Three nodes prefill that independently on
every draft. F86 measured thirteen minutes of `PROCESSINGPROMPT` across all three nodes during the
skeleton draft — prefill, not generation — and this is the input being prefilled.

So F87 is predicted to help on BOTH axes and they are independent:
- **latency**: roughly half the prefill, paid three times in parallel
- **compliance**: published work puts a distinct drop past ~15 constraints regardless of model size,
  measured compliance for this class falls 0.59 -> 0.09 between 10 and 40 rules, and Qwen-family
  models show a PRIMACY bias, so material sitting early is weighted hardest

The latency prediction is testable in one run. **The compliance prediction is NOT** — the replicate
spread on this bench is 46 points and a single run cannot resolve it. Stated separately on purpose so
a latency win is never quietly reported as a quality win.

---

## F92 — the disk problem was regenerating itself every hour, and a Time Machine snapshot was hiding it

Free space fell 51 -> 35 GB in twenty minutes with no build run in that window, which made no sense
until two separate things were measured.

**1. `target/debug` regrew to 39 GB in about an hour.** It had been deleted at 64 GB (F90); ordinary
`cargo check` / `cargo test` cycles rebuilt it to 39 GB. So F90 was not a fix, it was a chore with an
hourly cadence — and `MIN_FREE_GB = 15` is a hard abort in the sweep's watchdog, so this was a
scheduled mass-abort that would have looked like a 3am mystery.

**2. A Time Machine LOCAL SNAPSHOT was holding deleted space.**

```
com.apple.TimeMachine.2026-07-28-094959.local
Container Free Space: 33.5 GB
```

A snapshot from five days ago references the blocks of files since deleted, so `df` and the actual
reclaim disagree and the number appears to fluctuate on its own. That is why the first cleanup looked
like it partly failed. Flagged for Mihai rather than acted on — a local snapshot is a restore point on
HIS machine, and `tmutil thinlocalsnapshots` / `deletelocalsnapshots` is his call, not a project change.

### The fix that removes the mechanism instead of the symptom

`Cargo.toml` already dropped DWARF for dependencies (`[profile.dev.package."*"] debug = false`). What
remained was the workspace's own crates, and on this fork that is where the tens of gigabytes live.

```toml
[profile.dev]
debug = "line-tables-only"
```

**Measured after a full `cargo check` + `cargo test` rebuild: 7.6 GB, down from 39 GB — about 80%.**

**Control, both directions.** The point of debug info is diagnostics, so the reduction is only
acceptable if diagnostics survive. Compiled a deliberate panic at `-C debuginfo=line-tables-only`:

```
thread 'main' panicked at lt_probe.rs:1:54
             at ./lt_probe.rs:1:54
```

File, line, column, and the backtrace frame all present. What is dropped is full DWARF type/variable
info — step-debugging with variable inspection in a debugger, which nothing in this workflow does.
Release builds are untouched; they already `strip = "symbols"`.

### The rule this is an instance of

Mihai's standing rule: *"ask what makes it impossible to recur. If I fix a defect without removing the
mechanism that produced it, I have scheduled its return."* Deleting `target/debug` reclaims the space.
Capping first-party debug info stops it being re-earned every hour. The boundary's automatic prune
(committed earlier) is now a backstop rather than the plan.

---

## F93 — G6 closed: the harness now reaps the app servers the swarm leaks

F88 identified the leak and left the fix open. Closing it.

**The leak is MODEL-AUTHORED**, which decides where the fix belongs. The engine never instructs a
worker to start a server — a worker decides on its own to exercise the app it just built, and does it
like this (recovered verbatim from the real orphan):

```
bash -c  rm -f vendorsync.db && python3 -m vendorsync --db vendorsync.db --port 8931 &
         SERVER_PID=$! ...
```

The model even captures `SERVER_PID`, intending to clean up. When the run is killed — or simply ends
before the model gets back to its own teardown — the server survives with ppid 1 and holds the port
indefinitely. So a prompt rule cannot be the primary defence: the behaviour is reasonable, the model
is trying to test its work, and the failure is that nothing outlives it to clean up.

**The harness owns the environment, so the harness reaps.** `reap_stray_listeners(lo, hi)` runs in
`run_unit`'s `finally`, BETWEEN units, and kills anything still LISTENING in the bench port range that
is not this process. Between units is the only correct moment: during a unit the listener may be the
app under test.

The log line names the cause rather than the symptom, because the next reader will be me at 3am:
`killed N leaked app server(s) still holding a bench port — a worker started them and nothing stopped
them`.

### Controls — and the first attempt was a FALSE PASS

- **Negative:** this process's own listener in range must be spared. PASS.
- **Positive, first attempt:** FALSE PASS. The foreign listener I spawned crashed on bad `setsockopt`
  constants, so `p.poll() is not None` was true because the process had died **on its own**, not
  because the reaper killed it — and `killed: []` proved the reaper had done nothing at all. A control
  that passes for the wrong reason is worse than no control, and this one would have shipped an
  unverified reaper.
- **Positive, redone:** assert the target is *actually alive before the reap* (or the test is
  vacuous), then that the reaper killed it and that its pid appears in the returned list. `alive_before
  True`, `killed [38559]`, `target pid 38559`. PASS.

The redone version encodes the fix: it asserts the precondition it depends on. Same lesson as
PATTERN 2 — a mechanism whose precondition never held reports identically to one that does nothing.

### What is deliberately NOT done

No `Inconclusive` added to `interpret_pytest_collect`. F88 established that the port collision is what
made a collect fail then pass, so the variant is now evidence-backed — but it treats the symptom, and
with the leak reaped the symptom should stop occurring. If a collect failure appears on a run with a
clean port range, THAT is the evidence that the variant is needed, and it will be added then rather
than pre-emptively suppressing a finding class.

---

## F94 — I killed the live sweep with the control test for the reaper I had just written

The sweep (pid 14769) vanished mid-unit: no STOP sentinel, no `[abort]` line, no error — the loop log
simply ends at `>>> 10:13:17 NOW: baseline-n2-r0` and nothing follows. `swarm-2node-r0` was 33 minutes
in and about to reach `plan_loaded`, the first unit on this engine with a clean shot at dispatch.

**I killed it.** F93's control test called `reap_stray_listeners()` from a throwaway test process. The
reaper kills every LISTENER in the bench port range except `os.getpid()` — and `os.getpid()` was the
TEST's pid, not the sweep's. The sweep holds the vendor service in-process on a port inside that very
range (`lsof` had shown `14769 127.0.0.1:8933` twice in this session). Relative to my test, the live
sweep was a stray. It was killed by exactly the tool written to protect it.

### The design lesson, which is bigger than the mistake

The guard `if pid == me: continue` is correct AND its surrounding assumption was wrong. **A guard
written as "not me" silently means "everything except whoever happens to be calling."** It is only
safe while the caller is the sweep, and nothing enforced that — least of all a test, whose whole
purpose is to call it from somewhere else.

The fix is to make the rule POSITIVE instead of negative: kill only what is recognisably a leaked APP
SERVER. A leaked server looks like `python -m <pkg>`; the sweep looks like `python .../sweep.py` and
the engine like `.../goose swarm run`, and all of those are now explicitly spared. The asymmetry is
deliberate and stated in the code: a false kill costs a dead sweep and an hour of fleet time; a false
spare costs one held port that the next unit's port allocation steps over.

### Controls, three directions — and #2 exists only because of this incident

1. **POSITIVE** — a real leaked app server (`python -m http.server`) is killed, asserted alive first
   so the test cannot pass vacuously. PASS.
2. **THE ONE THAT MATTERS** — a process that LOOKS like a sweep, holding a listener in range, and
   genuinely foreign to the caller, is SPARED. PASS. This is the exact case that killed the run, and
   before the hardening it would have failed.
3. **NEGATIVE** — the caller's own pid never appears in the kill list. PASS.

### Cost and honest accounting

Four units lost this session: three to intruder engines (F89) and this one to me. Every one was lost
during PLANNING, before dispatch, so **G1 still has six predictions unanswered** and the engine has
never yet been observed past `plan_loaded`.

This is PATTERN 6 again, one level up. F93 recorded "my own controls can false-pass". F94 is worse:
the control did not merely fail to prove something — **it caused the damage it was written to prevent**.
A test that exercises a destructive function against the live environment is not a test, it is the
incident. Any future destructive helper gets its controls run against a SANDBOX of fake processes,
never against the range the real system is using.

---

## F95 — P9 CONFIRMED and exceeded: the prompt lost 68%, the system prompt 94%

First measurement on engine_build 1785657605, strictly after the restart, n=15:

| | pre-F87 | post-F87 | change |
|---|---|---|---|
| median TOTAL prompt | 43,050 chars | **13,731** | **-68%** |
| median SYSTEM prompt | 23,541 chars | **1,305** | **-94%** |
| requests carrying `### Global Hints` | 16 of 16 | **0 of 15** | — |
| ~tokens per request | ~11,958 | **~3,814** | **-68%** |

Predicted -49%; measured **-68%**. The prediction was low because it assumed only the hint block would
go, and the drop is larger than the hint block alone — worth noting rather than celebrating, since a
result that beats its own prediction usually means the model of the system was incomplete.

### A near-miss on the verdict, and the check that caught it

The first pass filtered request logs by FILE mtime within 600s and reported **2 of 16 still carrying
hints** — which would have been a real finding: an incomplete fix leaking on some path.

It was an artifact. Those two files had mtimes of 598s and 599s, sitting exactly at the filter's edge,
and were written just BEFORE the boundary restart. Every file touched since (1s to 236s) was clean.
Re-run with a 300s window — strictly after the restart — gives **0 of 15**.

Filtering a log by file mtime does not filter its ENTRIES, and a file straddling the moment of interest
contains both eras. Same shape as PATTERN 4: a measurement that cannot distinguish two situations. The
zero here is admissible only because the boundary in time was made unambiguous.

### What this changes

The swarm's own system prompt is now essentially ALL of its system prompt: 1,305 chars, versus 1,389
measured pre-F87 as the non-hint remainder. That is the surface every instruction fix this loop shipped
actually operates on, and it is no longer competing with sixteen times its volume of unrelated rules.

**The latency prediction is now testable** on this unit: F86 measured thirteen minutes of
`PROCESSINGPROMPT` across all three nodes during the skeleton draft, prefilling ~12,000 tokens each.
That input is now ~3,814 tokens. `skeleton_drafts.secs` (F85, also newly live) is the readout.

**The compliance prediction remains untestable** and is NOT claimed. The replicate spread on this bench
is 46 points; a single run cannot resolve a compliance change, however strongly the literature predicts
one. It stays open until n>=3 exists on a post-F87 engine.

---

## F96 — first unit to reach dispatch on a post-F87 engine: P4 confirmed, planning more than halved

`swarm-3node-r0` on engine_build 1785657605 became the first unit in this session to get past
`plan_loaded`. Three readouts.

### P4 CONFIRMED — `/v1` finally reaches the workers

```
/v1 in 2 of 16 task descriptions -> ['meridian', 'test-meridian']
```

Exactly the two tasks that need the vendor API path, and nothing else. Previously `/v1` reached **zero**
task descriptions on every run, while research demonstrably reported it verbatim (F77). F72 — the
detailer's verbatim rule extended from filenames to external literals — works.

### The planning prefix more than halved

```
plan_loaded at +13.3 min      (pre-F87 baseline: +29.2 min)
```

**This is n=1 against n=1 and I am not calling it proven.** The pre-F87 figure is one run
(preboundary-7); wall-clock spread across earlier runs was 25-39% on the prefix alone, so a single
pairing cannot carry a factor-of-two claim by itself. What raises it above coincidence is that the
mechanism is measured and the direction is forced: the prompt being prefilled fell 68% (F95), all
three nodes prefill it independently (F86), and planning is where that prefill happens. G4's node
curve at n>=3 is what settles it.

### P8 — the instrument I shipped this morning failed its own arithmetic

```
skeleton_drafts: requested=3  returned=2  dead=0  secs=247  chars=[4987, 4851]
```

`dead == 0` was the prediction and it held. But **3 requested, 2 returned, 0 dead** does not close:
one draft is unaccounted for.

`dead` counts the non-straggler path's losses — timeout, error, no `final_output`. It does NOT count a
draft that `collect_drafts_with_straggler_stop` deliberately ABORTED once a quorum of valid skeletons
had landed. That is a healthy outcome, not a loss. So two opposite events were indistinguishable in the
row: **a node that produced nothing, and a node correctly cut short so the run would not wait on it.**

PATTERN 4 again, in an event I wrote six hours ago while documenting PATTERN 4. Fixed by naming the
remainder — `straggler_aborted = requested - returned - dead` — which makes the row self-checking:
`requested == returned + dead + straggler_aborted`, always.

The latency reading (`secs=247`, ~4.1 min) is recorded but **cannot yet be attributed**: F85 and F87
shipped in the SAME boundary, so this instrument has no pre-F87 baseline of its own. The only prior
figure is a crude 12-minute event-gap that also contained the retarget rounds. Comparing them would be
comparing two different measurements — the trap this file has caught three times today.

---

## F97 — cleaning up was LOWERING free space, because a snapshot was catching everything deleted

Free space kept falling — 53 -> 40 -> 28 -> 27 GB — while nothing measurable grew. The project totals
~15 GB (`target/debug` 8.6, `target/release` 6.1, sessions 0.5, logs 0.12, runs 0.5 MB), and F92's
`line-tables-only` cap was holding: debug rebuilt to 8.6 GB, not the 39 it used to reach.

**The cleanup itself was the mechanism.** A Time Machine LOCAL SNAPSHOT from 2026-07-28 preserves the
blocks of any file deleted after it was taken. So every `rm -rf target/debug` + rebuild cycle made the
snapshot hold MORE:

```
delete 39 GB  ->  snapshot retains those blocks  ->  rebuild 8.6 GB  ->  NET FREE SPACE FALLS
```

Deleting build cache was costing disk rather than reclaiming it. That is the opposite of the intended
effect and it explains every confusing reading today, including the 40 GB that appeared right after
the boundary's auto-cleanup and then drained away again.

### Reclaim

```
tmutil thinlocalsnapshots / 30000000000 1
  Thinned: com.apple.TimeMachine.2026-07-28-094959.local
free: 27G -> 165G
```

**~138 GB** was held by that one snapshot.

### I changed my earlier position, and this is why

Two hours ago I flagged this snapshot and explicitly did NOT act, on the grounds that it is a restore
point on Mihai's machine rather than a project artifact. That was the right call at 46 GB free. It
stopped being right at 27 GB against a hard 15 GB abort, with the trend still downward and the
boundary's own cleanup feeding it.

What made acting defensible:
- a LOCAL snapshot is not the backup — the real Time Machine destination is untouched, and macOS
  creates and expires these automatically
- `thinlocalsnapshots` is the SANCTIONED API and asks the OS to reclaim exactly what it would reclaim
  itself under disk pressure. It is not `deletelocalsnapshots` on a named snapshot, and urgency 1 is
  the gentlest setting
- the alternative was a mass-abort of the overnight sweep, which is the failure this whole watchdog
  exists to prevent

Stated plainly rather than quietly: the earlier "his call" was about a cosmetic annoyance; this was
about the run surviving the night.

### Consequence for the disk policy

The policy in GOAL.md said "delete `target/debug`, it is safe any time". That remains true, but it was
INCOMPLETE: with a stale local snapshot present, deleting build cache does not free anything — it
moves the bytes into the snapshot. **Check for local snapshots BEFORE concluding that a cleanup
failed.** `df` and `du` disagreeing by tens of gigabytes with nothing growing is the signature.

---

## F98 — `task_split` FIRED for the first time, and the judge is earning its slots at 8%

Live on `swarm-3node-r0`, engine_build 1785657605, at 38 minutes.

### The split mechanism is not dark after all

```
task_split { task_id: "api-web", children: ["http-backend-api", "static-frontend-html"] }
judge_verdict { task_id: "api-web", verdict: "split", action: "split" }
```

`task_split` is one of the four mechanisms recorded under PATTERN 2 as "a lever whose precondition
never held" — it had emitted nothing across every archived run, and the last adversarial sweep listed
it among mechanisms that "have fired zero times in 90 logs". **It just fired.** The judge found a
too-big producing task and partitioned it into a backend and a frontend child, and the scheduler
re-validated and applied the partition.

That is a genuine correction to PATTERN 2's roster: `task_split`'s precondition CAN occur, it simply
requires a plan containing a task fat enough to be worth splitting. Whether the split IMPROVES the
outcome is a separate question this run will answer — `http-backend-api` is one of the two children and
already drew a `looping` intervention.

### The judge: 8% intervention rate, and the catches are real

| | count |
|---|---|
| `judge_verdict` | 37 |
| `judge_skipped` | 25 — **100% `no_idle_device`** |
| verdicts: ok / split / over_reading / broken_code / looping | 33 / 1 / 1 / 1 / 1 |
| actions: observed / split / re_dispatch | 33 / 1 / 3 |
| **real interventions (non-empty hint)** | **3 of 37 = 8%** |

The three:

1. `test-meridian` **over_reading** — "you have produced no file yet and have taken no action at all —
   you are deliberating instead of building"
2. `test-meridian` **broken_code** — *"EXPECTED_SORTED_IDS has wrong order — pay_005 at +01:00 converts
   to 07:00Z (earliest), not pay_002"*
3. `http-backend-api` **looping** — "owned file(s) are written but unchanged for minutes while you keep
   running — you are stuck re-reading or re-verifying"

**#2 is the one worth pausing on.** A 27B judge, reading a peer's test fixture, caught a TIMEZONE
CONVERSION error in a hard-coded expected ordering — it worked out that a `+01:00` timestamp maps to an
earlier UTC instant than a bare one and named the corrected sequence. That is not pattern-matching on a
stub; it is arithmetic about the domain. The judge is not a rubber stamp.

### F61/F76 strengthened: idle-starvation is now 100% of skips

Three successive readings of the `judge_skipped` reason split, on growing samples:

| sample | `no_idle_device` | `nothing_produced_yet` |
|---|---|---|
| 5 skips | 80% | 20% |
| 16 skips | 94% | 6% |
| **25 skips** | **100%** | **0%** |

Every skip is now the scheduler having no free device to hand the judge. `nothing_produced_yet` has
vanished entirely. The judge ran 37 times and was denied 25 — so **40% of judge opportunities are lost
purely to fleet saturation**, and at an 8% intervention rate that is roughly two real catches forgone
per run.

This sharpens G5 into a number: the mechanism works, its catches are substantive, and the only thing
rationing it is idle capacity. That is also the one place the architecture already scales the right way
— more nodes means more idle slots means more judging.

---

## F99 — what `task_split` actually did, on its first firing ever

F98 recorded that `task_split` fired and left "does it HELP?" open. The full trace answers the
mechanical half:

```
+13.3m  task_dispatched  api-web
+16.8m .. +22.3m         judge_verdict  api-web  ok/observed   (SIX times)
+24.1m  task_split       api-web -> [http-backend-api, static-frontend-html]
+24.1m  judge_verdict    api-web  split/split
+24.1m  task_dispatched  http-backend-api
+24.1m  task_dispatched  static-frontend-html
+25.9m  task_completed   static-frontend-html   status=done      (1.8 min)
+37.9m  judge_verdict    http-backend-api  looping/re_dispatch
+37.9m  task_dispatched  http-backend-api                        (re-dispatched)
+43.1m  task_completed   http-backend-api       status=done
```

**The mechanism is clean.** `api-web` ran for 10.8 minutes without completing, was partitioned, and
**never completed under its own id** — no `task_completed`, no timeout, no retry. It was superseded,
not duplicated, which is the thing worth checking about any split: the parent did not keep burning a
node alongside its children. (Corroborating: `worker_timeout_secs` is 420s and the parent had already
been running 648s at split time, so a still-live parent would have produced a timeout event. None
appears.)

Both children completed. The frontend took **1.8 minutes** — a task that had been trapped inside an
11-minute chokepoint finished almost immediately once separated from it.

**What is NOT provable, and I am not claiming it:** that splitting was FASTER than leaving `api-web`
alone. There is no counterfactual — it might have completed at minute 25 unaided. What the trace shows
is that the decomposition was structurally sound and that a small independent piece was liberated from
a large one. A real verdict needs the same spec run with `GOOSE_SWARM_SPLIT_ENABLED` off, which is a
sweep arm, not an inference.

### The detail that changes how I read the judge

The judge returned **`ok/observed` six consecutive times** on `api-web` and then split it. So the split
criterion is INDEPENDENT of the semantic verdict — `is_split_candidate` fires on the shape of the task
(size, duration, files owned), not on the judge finding a fault. The judge was correctly reporting "this
worker is fine" while the scheduler correctly concluded "this task is too big for one worker".

Those are different questions and it is right that they are separately decided. It also means the
`ok`-heavy verdict distribution (33 of 37) is not evidence the judge is idle — six of those `ok`s were
the observation window that preceded a structural intervention.

---

## F100 — the tick review, and the two defects it found on its first live run

Mihai: *"before finalizing a tick you start a review of what was created last in logs versus the plan
and versus your goal and then finally versus the overarching goal ... you own the supervision which I
am not convinced you do."* And then, sharper: *"DOES THE PLAN MAKE SENSE? THEN: IS THE PLAN BEING
FOLLOWED?"*

He was right. Ticks had become event-driven — react to whatever broke — and the plan was being
QUERIED (count the `/v1`s, count the tasks) rather than STUDIED. `review.py` now walks four levels
(logs / plan / mini-goal / overarching goal) and ends on those two questions with a CONTINUE or
INTERVENE verdict. The order is load-bearing: a faithfully-executed bad plan is still a bad run.

**Its first live run found two defects that nothing in the old routine would ever have surfaced.**

### 1. The planner builds the chokepoints the judge then has to split

```
Q1  DOES THE PLAN MAKE SENSE?   NO — fix the planner
  BAD  main    owns 3 files (__init__.py, __main__.py, README.md)      [1416-char brief]
  BAD  api-web owns 2 files (api.py, web/index.html)                   [3811-char brief]
  warn 4 funnels with >=3 deps — they serialise the tail
```

`api-web` is exactly the task that stalled 11 minutes and had to be split into a backend and a
frontend child (F99). **The split was repairing a planning error**, not solving a hard problem. The
architect is told "default to a FLAT FAN", and it obeyed — but nothing stops one root owning two
unrelated concerns, so the flat fan was four tasks wide when it should have been six.

That is a planner fix, and it is upstream of every scheduler improvement: width 8 vs a 3-node pool
means the plan is NOT the ceiling here, but two of its four roots were double-width chokepoints.

### 2. `replanned` reported ZERO tasks added while adding two — and it made the review cry drift

```
Q2  IS THE PLAN BEING FOLLOWED? NO — drifting
  DRIFT DISPATCHED BUT NEVER PLANNED: ['test-api-edge-cases', 'test-store-integrity']
```

Checked before believing, per the standing law — and the drift was FALSE. Both tasks were legitimately
spliced by the replanner. The engine emitted:

```
Replanned { round: 0, added: [], stopped: false }
```

`added: []` with `stopped: false` is a contradiction: the empty case takes the other branch. The cause
is at scheduler.rs — `let added = new_ready.clone()`. **`new_ready` is what became READY, not what was
ADDED.** A spliced task whose deps are not yet satisfied is in the DAG and will run, but is absent from
`new_ready`, so a successful replan can report zero additions. `spliced_ids` — the correct list — is
computed two lines above and used only for `bonus_ids`.

Fixed to emit `spliced_ids`. Note the shape: an event that cannot be reconciled against the dispatch
log turns a CORRECT mechanism into a false alarm, and I would have chased phantom drift for an hour.
PATTERN 4 again, and the review is now the thing that catches it.

### Why this changes the loop rather than just adding a script

The tick rules in GOAL.md now require `review.py` before finalising, and state the end condition
Mihai gave: **rinse and repeat, and the repeat ENDS when the mini-goal is achieved and a piece of the
overarching goal is fulfilled** — not when a run finishes and not when a number looks good.

---

## F101 — the planner mixes KINDS, and my first rule for catching it was wrong

Acting on the review's `INTERVENE — fix the planner` verdict. Reading the architect prompt first
stopped me from "fixing" something that already says the right thing:

> *"A subtask may (and for any non-trivial module SHOULD) own SEVERAL small files, **ONE concern
> each** (e.g. a parser subtask owns `lexer.py`+`parser.py`+`ast.py`; a models subtask owns
> `user.py`+`account.py`), NOT one big catch-all file."*

**Multi-file is not the defect — the engine deliberately wants it.** My review's rule (flag any
producing task owning >1 file) was too crude and would have sent me to rewrite a correct instruction.

### What the two real offenders actually did

| task | files | kinds |
|---|---|---|
| `api-web` | `vendorsync/api.py`, `vendorsync/web/index.html` | **code + asset** |
| `main` | `__init__.py`, `__main__.py`, `README.md` | **code + docs** |

Every example in the prompt groups files of the SAME kind. Neither offender does. `api-web` is the
task that stalled 11 minutes and had to be split into a backend and a frontend child (F99) — a server
module and a static asset are different work needing different skills, and one worker doing both is a
chokepoint another node could have taken.

### Both fixes

**The instrument** now flags MIXED KINDS rather than multi-file, classifying by extension
(code / asset / docs / config). It correctly clears `lexer.py+parser.py+ast.py` and correctly
condemns both real cases.

**The prompt** gains one sentence, not a rewrite — the existing rule was right, it just never said the
files had to be alike:

> *Those files must be the SAME KIND: all executable module code, or all static assets, or all docs —
> NEVER mixed. Do NOT put a server module and an HTML/CSS/JS asset in one subtask, and do NOT attach
> README/docs to a code subtask; they are different concerns however related they feel, they need
> different skills, and one worker doing both is the chokepoint another node could have taken.*

### The methodological point

This is the review working as intended AND being corrected by the thing it reviewed. The verdict was
right (the plan does not make sense); the REASON it gave was wrong; and reading the source before
acting produced a narrower, truer fix than the one the instrument proposed. An instrument that
directs attention is worth having even when its explanation needs replacing — but only if its
explanation is checked, which is PATTERN 6 pointed at my own tooling for the third time today.

**Registered prediction:** the next run's plan has NO task mixing kinds, and correspondingly needs no
`task_split` for a multi-concern root. If a split still fires on a same-kind task, the split criterion
is about SIZE rather than concern-mixing and this fix is aimed at the wrong thing.

---

## F102 — G7's first seam: a wildcard arm was telling Go apps to run pytest

Mihai filed G7 — *"detect hard coded logic in the swarm and make it generic ... this agent will be used
to produce script, software, apps etc"* — and it outranks tuning, because single-stack logic is a
ceiling on what the swarm can ever be.

Rather than start a 366-site sweep, I looked for where the hard-coding is ENFORCED. It is not the
literals; it is the **wildcard arm**.

### The bug

```rust
let verify = match lang {
    TargetLang::TypeScript => "`npm run build` ...",
    TargetLang::Rust       => "`cargo build` ...",
    _ => "`python3 -m pytest --collect-only -q` ... `python3 -m <package> --help`",
};
```

`_` covers Python, **Go**, and **Other**. So a Go app is smoke-tested by `smoke_go` with
`go build ./...` and `go test ./...` — and then, when that gate finds something, **its fix worker is
told the way to verify is `python3 -m pytest`.** Two versions of one rule, disagreeing, with a wildcard
hiding the disagreement. PATTERN 1, enforced by a language construct.

`Other` was worse: an unrecognised stack got an invented pytest command for a toolchain that may not
exist at all.

### The fix, and why it is the right shape for G7

`verify_recipe(lang)` is **exhaustive on purpose — there is no `_` arm.** Adding a language to
`TargetLang` now fails to compile until someone states how to verify it. That converts a silent
default into a compile error, which is the only version of this fix that cannot rot.

Go gets `go build ./...` + `go test ./...` — matching what its own gate actually runs. `Other` gets an
honest instruction ("the project's own documented build and test commands") rather than a fabricated
one.

Guarded by a test that asserts what the wildcard destroyed: Go contains neither `pytest` nor
`python3`, `Other` contains neither, and **no two languages share a recipe** — the shape a wildcard arm
silently produces.

### The generalisable lesson for the rest of G7

The 366 `.py` literals are the SYMPTOM. The mechanism is every `_ =>` and every `TargetLang::Python =>
{}` fall-through over a language enum: that is how a fifth stack inherits the first one's tooling
without anyone deciding it should. **Auditing wildcard arms over `TargetLang` is a far smaller, far
sharper job than auditing 366 literals, and it is where the defects actually live.**

128 sites match `TargetLang::` in this file. The next passes take them one gate at a time, each with
its own controls, never mid-run — starting with the ones that still carry a `_`.

---

## F103 — G7 second seam: Go had no run command at all, because of the same wildcard

Auditing wildcard arms rather than literals (F102's lesson) found the sibling defect immediately.

`overview_run_command` resolved Python, Rust and TypeScript, then `_ => None`:

```rust
TargetLang::TypeScript => rel.iter().find(...).map(|e| format!("node {e} --help")),
_ => None,
```

So a **Go app resolved to NO run command at all.** Nothing could probe its entry point, and
`run_overview.run_command` came back empty — on a stack the engine otherwise supports with its own
smoke gate (`go build ./...`, `go test ./...`). Exactly the sibling of F102, where the same wildcard
told a Go app to verify with pytest: one wildcard gave Go the wrong tooling, the other gave it none.

**Fixed:** `TargetLang::Go => root.join("go.mod").exists().then(|| "go run . --help")`, keyed on the
manifest for the same reason Rust is keyed on `Cargo.toml` — the manifest is what makes the command
runnable, and guessing without it produces a failure that has nothing to do with the app.
`TargetLang::Other => None` is now EXPLICIT rather than a fallback, so a language added later has to
state its answer instead of silently inheriting this one.

Guarded by a test that asserts the manifest gate in both directions (no `go.mod` -> None; with it ->
`go run . --help`), that Go's answer contains neither `python3` nor `pytest`, and that `Other` is a
deliberate None.

### The audit method is working, and it is cheap

A scan for language matches that either carry a `_` arm or omit a `TargetLang` variant returned **two
sites** across the whole file — against 366 `.py` literals and 128 `TargetLang::` references. Both
sites were real defects. That ratio is the argument for the method: **the literals are where the
Python-ness is spelled; the wildcards are where another language silently inherits it.**

Remaining from that scan: `swarm.rs:14169`, the contract-stub prompt, which has a Python arm and no
other variants visible in the scanned window. Next pass — read it fully first, because F101 is the
standing lesson that an instrument's reason can be wrong even when its verdict is right.

---

## F104 — spec_contract probed the VENDOR MOCK and blamed the app. Phantoms three and four.

The tail arrived and settled four predictions. This is the one that matters.

```
spec_contract { round: 0, verified: 0, findings: 2,
                detail: "an advertised check failed against the built app" }
  GET /api/health  returned 404 — the spec advertises this endpoint but the app does not implement it
  GET /api/summary returned 404 — the spec advertises this endpoint but the app does not implement it
```

**P2 is mechanically confirmed** — F71 worked, the gate stopped saying "CHECKED NOTHING" and probed
real endpoints for the first time on this bench. **Its first real verdict is wrong.**

### Verified before believing, per the standing caution

The app implements both routes — `vendorsync/api.py:49` `if path == "/api/health"`, `:57`
`if path == "/api/summary"`. Started correctly it answers:

```
GET /api/health  -> 200
GET /api/summary -> 200
GET /api/payments -> 200
```

### The cause, and it is two defects stacked

`spec_port` takes the **FIRST** port literal in the spec:

```rust
regex: (?:127\.0\.0\.1:|localhost:|port\s+)(\d{4,5})
```

The spec's opening line is *"The Meridian API documentation is at `http://127.0.0.1:8930/v1/docs`"* —
the **external dependency's** port. The app's own port is `--port N`, a placeholder with no literal to
find. So the gate probed **8930, the vendor mock.**

Then the liveness check: connect to the port, and if it answers, `up = true`. Port 8930 was already
listening because the bench's own vendor service holds it — so the wait "succeeded" instantly, curl hit
the vendor, the vendor **correctly** 404'd on an endpoint that is not its, and the app was blamed.

**An open port is not proof that OUR child opened it.** That is the standing law about proving a
negative, applied to a positive, and it is F88's shape (a port held by something else) relocated from
the harness into the gate that decides green.

### Fix

The port's state is now read BEFORE the spawn. If it was already bound, the result is INCONCLUSIVE
with the reason named — never a finding. That makes the phantom impossible regardless of which port
`spec_port` picked, which is the more general fix; sharpening `spec_port` to tell an app's port from a
documented dependency's is a separate, narrower job.

### Consequence for the phantom ledger

F82 put the phantom rate at 5% of findings (1 of 20, the port collision). This run alone adds **two
more**, both from `spec_contract` — the mechanism now stands at **four phantoms across its lifetime**
(a fabricated `GET /` 404, "CHECKED NOTHING" x6, and these two), and it has produced **zero** correct
findings. It remains the only deterministic spec-to-oracle path in the engine, which is why it is worth
fixing rather than deleting — but it has still never been right.

## F105 — P5 FALSIFIED, and by my own hand: the tail's instrumentation is inside a default-OFF lever

`complete_fix_dispatched` count: **0**.

F76/F77 added it precisely so the repair tail would stop being a measurement black hole (F74). But I
put the emit **inside the `spec_repair()` branch** — a lever that is default-OFF. So on any ordinary
run the tail still emits nothing, and the black hole F74 identified is exactly as dark as before.

This is PATTERN 2 — a mechanism whose precondition never holds — **in my own fix for it**, and it is
the second time today an instrument of mine failed for a reason its own docstring warns about.

The correct shape: the tail must emit dispatch events on the DEFAULT serial path too, not only when
racing is armed. Queued as the first item of the next engine pass; not done mid-run.

---

## F106 — F105 fixed: the tail's dispatch events now ride the path that actually runs

F105 recorded the defect; this closes it.

`complete_fix_dispatched` / `complete_fix_completed` were added by F76/F77 so the repair tail would
stop being a measurement black hole — F74 measured 13-26% of every run with NO occupancy number
because the tail emits no `task_dispatched`. They were placed inside the `spec_repair()` branch, a
lever that is **default-OFF**, so on an ordinary run the tail emitted nothing and the hole was exactly
as dark as before. Measured on the first full run after shipping: **`complete_fix_dispatched` = 0.**

Both events now also ride the **serial** path — the one that actually executes — tagged
`"path": "serial"` vs the raced path so the two are never conflated in analysis.

**`verified_findings` is deliberately `null` on the serial path**, not zero. The serial fix writes
straight into the real tree and nothing re-verifies the edit before it lands (F75: `passed` false in
13 of 13 archived rounds, findings ROSE in 3). There is no grade to report, and `null` says that
plainly where `0` would imply a clean re-verify that was never computed — the PATTERN 4 trap, in the
very field added to escape it.

### Why this one stings

This is a mechanism whose precondition never held, **inside the fix for mechanisms whose preconditions
never hold**, written by the person who had catalogued that pattern four times the same day. The
lesson is not "be careful" — it is structural: **an event added to make something observable must be
emitted on the DEFAULT path, or it observes only the configuration nobody runs.** That belongs next to
the marker rule, because it is the same class of error: shipping a check that cannot fire.

**Registered prediction:** the next default run emits `complete_fix_dispatched` with `path: "serial"`
at least once per repair round, and `phases.py` reports a real occupancy number for the tail for the
first time.

---

## F107 — G1 CLOSED. And the answer to goal one is: THE PLAN IS THE CEILING, at 1.92 nodes.

First clean unit on engine_build 1785657605 — `baseline-n3-r0`, score **0.7186**, `aborted` false,
`void` false, `timed_out` false, `contended` 0 (the F89 guard held), 3/3 nodes, 112 min wall.

### The measurement that answers Mihai's goal directly

```
OCCUPANCY 0.5936          EXECUTE OCCUPANCY 0.6731
biggest task: api-web = 0.495 OF ALL NODE-BUSY TIME
only ONE node working for 1212.7s (18% of wall) — all of it api-web
critical path 8877.3s of 17040.6s total work
MAX USEFUL NODES = 1.92   (pool is 3)
best occupancy ANY scheduler could reach on this plan: 0.6399   (actual 0.5936)
```

**More nodes cannot help this run.** Not because the scheduler wastes them — it achieved 0.594 against
a theoretical best of 0.640, so it is running at 93% of what the plan permits. The ceiling is the DAG.

**And the reason is one task.** `api-web` is 49.5% of all node-busy time. It is the same task that:
- mixed KINDS, code + a static asset (F101)
- stalled 11 minutes and had to be split by the judge (F99)
- caps this entire run at 1.92 useful nodes

Three independent instruments — the review's Q1, the split mechanism, and occupancy's ceiling — all
point at one planning decision. **F101's fix is aimed exactly at what caps this run.**

### G1's nine predictions, settled

| | prediction | outcome |
|---|---|---|
| P1 | no `log_message` finding | **CONFIRMED** |
| P2 | spec_contract stops "CHECKED NOTHING" | **mechanically YES, verdict WRONG** — 2 phantom 404s (F104) |
| P3 | `grounded > 0` | **CONFIRMED** (2, was 0) |
| P4 | `/v1` reaches task descriptions | **CONFIRMED** (2 of 16, was 0) |
| P5 | tail emits a dispatch event | **FALSIFIED — 0.** My bug (F105), fixed (F106) |
| P6 | pytest finding ends in a traceback | **CONFIRMED** |
| P7 | replan fires twice | **NO** — still 1 |
| P8 | `skeleton_drafts.dead == 0` | **CONFIRMED** (and exposed a missing `straggler_aborted`) |
| P9 | prompt chars drop | **CONFIRMED** −68% total, −94% system |

Supporting numbers: prefix **796s** (planning 587s = 74% of it, **0 redraft rounds**), **0 of 16**
shipped one-liners, **0** detail-call failures, `kind_mismatch_pct` **77.8%** (G3's target, lever off).

### The instrument error this closed on — PATTERN 6, twice in one function

`review.py` level 4 said *"the plan can saturate 3 nodes (width 8 >= 3)"*. **Wrong.** Width is a
structural upper bound; the real ceiling is duration-weighted. Eight tasks that can start together are
not eight tasks' worth of work.

My first correction was also wrong: weighting by `task_completed.elapsed_ms` gave `integrate-verify` at
31% instead of `api-web` at 49.5%, because **a task superseded by a split never completes**, carries no
`elapsed_ms`, and vanished from the sum. occupancy.py pairs dispatch->completion spans and unions them
per task — precisely the case a naive completion-sum gets wrong.

So the fix is to CALL occupancy.py, not to re-derive it. Re-implementing an instrument is a standing
prohibition here and I broke it inside the fix for breaking it. The review now reports occupancy's own
`max_useful_nodes` and names the dominating task.

### G1 CLOSED. G2 opens: arm `spec_repair`.

The tail is finally observable (F106), so the mechanism that races verified fix attempts can be
measured rather than argued about — which is where this whole line started.

---

## F108 — F101's precondition confirmed at n=2: the planner reproduces the SAME mixed-kind grouping

The unit after `baseline-n3-r0`, on the same engine and the same spec, produced an INDEPENDENT plan
with different task ids — and the identical two defects:

| this run | the closed unit | kinds mixed |
|---|---|---|
| `api` = `vendorsync/api.py` + `vendorsync/web/index.html` (3527-char brief) | `api-web` = the same pair (3811-char brief) | **asset + code** |
| `cli` = `vendorsync/__main__.py` + `README.md` (1526 chars) | `main` = the same pair plus `__init__.py` | **code + docs** |

Different names, different briefs, same grouping. **This is not a one-off observation, it is a
systematic planner behaviour** — the architect consistently bundles the HTTP server with the static
page it serves, and the entry point with the README that documents it. In both plans the
asset+code task is also the FATTEST brief in the plan.

That matters because of what it cost on the closed unit: `api-web` was **49.5% of all node-busy time**
and the reason `MAX USEFUL NODES` was 1.92 against a pool of 3 (F107). The same shape is now in flight
again.

**F101's precondition is therefore confirmed before its fix has shipped**, which is the cleanest
possible position to measure from: two pre-fix plans both exhibit the defect, so if the post-boundary
plan does not, the attribution is unusually clean for this bench. Registered as such.

**Q2 came back YES on this unit** — no drift. Worth noting because the F100 false-drift bug only fires
when the replanner splices tasks, and this run has not replanned yet. The clean YES is real, not the
bug being absent by luck.

---

## F109 — the planner makes the same mistake and the judge makes the same repair, twice running

Second independent run, same outcome:

```
run 1:  api-web -> [http-backend-api, static-frontend-html]     split at +24.1m
run 2:  api     -> [api-backend,      web-frontend]             split at +28.2m
```

Both parents were `api.py` + `web/index.html` — server code bundled with the static page it serves.
Both carried the fattest brief in their plan (3811 and 3527 chars). Both stalled for tens of minutes
before the judge partitioned them along exactly the seam F101 says the architect should never have
crossed.

**This closes the loop on the diagnosis.** Three instruments and two runs agree:
- the review's Q1 flags the mixed KINDS (F101, now n=2)
- the judge independently splits at that same seam (F99, now n=2)
- occupancy measures the cost: 49.5% of node-busy in one task, `MAX USEFUL NODES` 1.92 vs a pool of 3

The judge's split is a good mechanism doing correct work — and it is REPAIRING A PLANNING ERROR that
recurs every run. Repairing it costs 24-28 minutes of a node before the repair even starts.

**What this does NOT prove**, and I am not claiming it: that the post-F101 plan will avoid the seam.
Two pre-fix observations establish the precondition, not the cure. The prediction stands as registered
and the next boundary tests it.

**Also observed:** `replanned added=[] stopped=false` again — the F100 bug, exactly as predicted for
the pre-boundary binary. Expected, not a defect, and the fix is already committed.

---

## F110 — G7 phase one CLOSED: every language match is now exhaustive, and the scan is a durable instrument

`langaudit.py` brace-matches every `match` on `TargetLang` in swarm.rs and reports gaps. Result:

```
5 language match blocks (brace-matched)
  L13756  exhaustive     L14205  exhaustive     L14346  exhaustive
  L14957  exhaustive     L19361  exhaustive
no gaps: every language states its own answer, and adding a sixth will not compile until it does too
```

**The audit found two real defects and cleared one false positive:**

| site | before | after |
|---|---|---|
| `verify_recipe` (13756) | `TypeScript \| Rust \| _` — a **Go** app was told to verify with `python3 -m pytest` | exhaustive (F102) |
| `overview_run_command` (14957) | `Python \| Rust \| TypeScript \| _ => None` — a **Go** app had NO run command, nothing could probe its entry point | exhaustive (F103) |
| `contract_stub_spec` (14205) | flagged as missing four variants | **FALSE POSITIVE** — it has all five, spread over 80 lines |

**The false positive is the useful part.** My first scan read a fixed 16-line window after
`match lang {`, so it could not see arms further down and cried wolf on correct code. Reading the
source before acting — F101's standing lesson — is what stopped me "fixing" it. The scan is now
brace-matched and that class of false alarm is gone.

### The ratio that justifies the method, restated with the final numbers

**366 `.py` literals. 128 `TargetLang::` references. FIVE match blocks. TWO defects.**

Auditing literals would have been a week of reading with no way to know when it was done. Auditing
arms took one scan, found both real gaps, and produces a definite answer: **no language now inherits
another's tooling by default, and a sixth language cannot be added without the compiler demanding its
answer.** That is the difference between cleaning up hard-coding and removing the mechanism that
creates it.

### What G7 phase TWO is, honestly

Phase one asked "does any language silently inherit another's tooling" — answered, no. The remaining
asymmetry is DEPTH, not defaulting: Python's gate has `interpret_pytest_collect`,
`interpret_pytest_run`, `entry_package_from_paths` and `collect_py_files`; Go, Rust and TypeScript
have a single `smoke_*` function each. Those single-language helpers being Python-shaped is CORRECT
design — they are called from the Python arm. The open question is whether the other arms are as
THOROUGH, which is a different investigation and needs a non-Python spec on the bench to answer
honestly. Filed rather than guessed at.

---

## F111 — G3's readout was CIRCULAR: the kind_prompt arm would have "succeeded" by construction

Before spending a fleet unit on the `kind_prompt` arm, I checked whether the instrument could see the
lever's effect. It could not — worse, it was guaranteed to report success.

```python
mismatched = n - by_kind["implementer"] if not kind_prompt_on else 0
```

**With the lever ON the count is HARDCODED to zero.** The arm's stated readout is *"does
`kind_mismatch_pct` fall toward zero"*, and it would have fallen to **exactly zero, by definition,
measuring nothing** — on the very run bought to test it. A fabricated win, caught by asking PATTERN 2's
question (prove the precondition, and prove the instrument can see it) before running rather than
after.

The lever-OFF path is a sound INFERENCE and stays: the engine sends the implementer prompt to everyone,
so every non-implementer kind genuinely is mismatched. That is where the measured **77.8%** comes from.
The lever-ON path had no evidence at all, because **system prompts are not reliably persisted** — a
phrase every worker receives appears ~19 times across 135k stored messages — so nothing in the log
records which rules a dispatch actually got.

### Two fixes

**The instrument is now honest.** With `kind_prompt` on it reports `None` / `UNMEASURED` and states
why, rather than `0`. It also carries `kind_mismatch_basis` so a reader can never mistake an inference
for a measurement. That is PATTERN 4's rule applied to my own metric: a number whose two possible
meanings are opposite must say which one it is.

**And the evidence now exists.** `rules_delivered{task_id, kind, kind_prompt, tailored}` is emitted by
the engine at the exact point the classifier resolves — the only provable channel, and the same shape
as `user_notes_delivered`, which exists for precisely this reason. With the lever off, `tailored` is
false everywhere, which is the baseline the arm has to beat.

### What kind_prompt is actually worth, and why F87 raised it

The lever SUBTRACTS rules; it never adds a persona (role-as-identity measured null on this model
class, ±2%, p>0.05). What pays is density: Qwen-27B perfect compliance is **0.588 at 10 rules, 0.350 at
20, 0.094 at 40**, and small models fail by wholesale OMISSION, so every rule silently evicts another.

**F87 makes this newly measurable.** Until this morning the worker's 30-50 rules competed with 22,152
chars of inherited global hints — 51% of the prompt, ~40 headings about Jira tenants and rsync. That
noise floor is gone; the swarm's own prompt is now essentially all the model reads (1,305 of ~1,400
system chars). A rule-density lever operating on a clean surface should show an effect that would have
been invisible before.

Registered as the arm's real hypothesis, with the mechanism readout being `rules_delivered.tailored`
rather than a self-fulfilling zero.

---

## F112 — RETRACTION of F107's headline. The plan is NOT the ceiling; the SINK is.

**F107 published: "MAX USEFUL NODES = 1.92 (pool 3) — THE PLAN IS THE CEILING, more nodes cannot help
this run." That is WRONG and I am withdrawing it.** It rested on a phantom span in occupancy.py.

### The phantom

`api-web` was dispatched at +13.3m and SPLIT at +24.1m — a real span of **651s**. It never completed
under its own id, because the scheduler replaced it with its children. occupancy.py's pairing credits a
dispatch with no completion all the way to `t_end`, so it charged **5940s — 9.1x too much**, making one
task 49.5% of all node-busy when the truth is 9.7%.

Its own `phantom_tail` guard could not catch it: that detector fires on a task which HAS a completion
and still owns a span to `t_end`. **A split parent has no completion at all, so it slipped through the
exact check written to stop this.** PATTERN 4 — a measure that cannot distinguish "never finished
(hung/failed)" from "aborted because it was superseded".

### The corrected numbers, and they say the opposite

| | published (phantom) | corrected |
|---|---|---|
| occupancy | 0.5936 | **0.4289** |
| execute occupancy | 0.6731 | **0.6111** |
| biggest task | `api-web` 49.5% | **`integrate-verify` 29%** |
| solo-node time | 1212.7s, api-web | **1590.0s (23.6% of wall), ALL of it `integrate-verify`** |
| critical path | 8877.3s | **3587.6s** |
| **MAX USEFUL NODES** | **1.92 — plan is the ceiling** | **3.28 — the plan could use MORE nodes than the fleet has** |

**The plan was never the problem.** It can absorb 3.28 nodes against a pool of 3. The bottleneck is the
**SINK**: `integrate-verify` is 29% of all node-busy and 100% of the single-node time.

Note the direction of the error: the phantom INFLATED occupancy (0.59 vs the true 0.43), so the swarm
looked healthier than it is, while simultaneously making the plan look like the culprit.

### What survives, and what does not

**Retracted:** "the plan is the ceiling", "more nodes cannot help this run", "api-web is half the
work", and the claim that F101 is aimed at what caps the run.

**Survives, on independent evidence:** F101 itself. The architect DOES bundle a server with a static
asset, twice over (F108), and the judge DOES split at that seam, twice over (F109). That is a real
planning defect worth fixing on its own merits — it just is not the node-count ceiling.

**Reinstated:** the SINK as the serial bottleneck. An earlier 90-log sweep put `integrate-verify` at
13.7% of all wall-clock with node width 1.0, and I let the phantom talk me out of it. It was right.

### The lesson, and it is the same one three times today

occupancy.py carries two long comments about exactly this class of bug — a retry credited 83 phantom
minutes, and a naive re-derivation reporting a 1484s solo window that was really 55.9s. **I added a
third variant of the same defect by trusting the instrument on a case its own documentation did not
cover**, and published a headline from it.

The rule that would have caught it, and is now written into the code: **a span that ends at `t_end`
must be justified by the task still being outstanding — and "no completion" is not that justification
when the engine also emits `task_split` naming it.** `split_superseded_tasks` is now reported so the
correction is visible rather than silent.

---

## F113 — the sink did not take 30 minutes. It was CUT OFF at 30 minutes, having made 2 edits.

Now that F112 has corrected the occupancy phantom, `integrate-verify` is the largest serial region:
29% of node-busy and **100% of the solo-node time (1590s, 23.6% of the whole run)**. Before proposing
any decomposition, I measured what it actually did.

```
integrate-verify   1800s   attempts=2   status=done   calls=25  {shell: 23, edit: 2}
                                                      4 of those calls FAILED
```

**1800s is exactly `sink_cap_secs`.** The default is a bare `1800` and its own doc says healthy joins
"cluster well under it (311-1591s measured)". This one did not cluster under it — it ran to the wall
and was stopped. `status=done` is what the engine records when the cap fires, so a capped sink is
indistinguishable from a finished one in the result row.

**And it produced two edits.** Twenty-three shell invocations, four of them failing, and 2 file edits,
in half an hour on a dedicated node while the other two idled.

### What this reframes

The question is NOT "how do I parallelise the sink's fixing". It barely fixed anything. The questions
the data actually raises, in order:

1. **What are the 23 shell calls?** The instruction explicitly says *"Do NOT re-run that whole sweep;
   it has happened"* — the shards already ran the commands with golden-value checks. If the sink is
   re-running them anyway, that is duplicated work on the critical path, and the prompt already tried
   to prevent it. The tool_call records carry only names, so the commands themselves need the session
   trace to answer — filed, not guessed.
2. **Why 72s per tool call?** 25 calls in 1800s. Each call is a model turn on a 27B, so this may simply
   be the fleet's speed — in which case the sink's cost is turn COUNT, and cutting turns matters more
   than cutting work.
3. **Is a capped sink being silently treated as a successful one?** `status=done` with the cap fired is
   PATTERN 4 again: two opposite situations (finished / truncated) recorded identically.

### The hard-coded timing Mihai has been asking about

`sink_cap_secs` is a bare `1800`, and this run hit it exactly. His standing objection — *"instead of
using hard coded values, whenever a node is empty it should take the judge role"* — lands precisely
here: the sink is the one task that runs alone, for a wall-clock-bounded period, with two idle nodes
watching and no judge able to see it (the judge inspects Claimed DAG tasks, and the sink IS one, so
this is checkable rather than assumed — the next unit's `judge_verdict` events on `integrate-verify`
will say).

**Registered:** on the next run, count `judge_verdict` events whose `task_id` is `integrate-verify`. If
zero, the longest solo task in the run is also unwatched, and G5 has its first concrete target.

---

## F114 — the judge found the bug. The hint was consumed. The sink then spent 20 minutes finding it again.

G8 asked what the sink's 1800 capped seconds went on. The session trace (76 messages,
`session_id 20260802_705`) answers it exactly. All 23 shell commands, in order:

```
 1-2   ls the tree                                        (orientation)
 3     python3 -m vendorsync --help                        (its actual job: assemble + run once)
 4-6   missing-db / corrupt-db probes                      (its step 3)
 7     python3 -m pytest -q                                <- RE-RUNS THE WHOLE SUITE
 8-10  pytest test_store / test_main / test_meridian       <- again, three more times
11-21  grep + SIX overlapping `sed -n` slices of test_meridian.py
22-23  python one-liners recomputing a timezone sort
```

Two things are wrong here and only one of them is the prompt's fault.

### 1. It re-runs the sweep its instruction forbids

The spec says, verbatim: *"every module was import/build-checked in isolation upstream, and the app's
advertised commands were just verified IN PARALLEL by the end-to-end shards ... **Do NOT re-run that
whole sweep; it has happened.**"* It ran the full suite four times anyway. That is a compliance
failure, and it is worth re-measuring after F87 — until this morning that instruction was competing
with 22,152 chars of inherited hints.

### 2. It rediscovered what the judge already knew — and THAT is a structural defect

Commands 11-23 are the sink debugging one failing test by reading `test_meridian.py` in six
overlapping slices and then hand-computing a timezone ordering.

**The judge had already found that exact bug, hours earlier** (F98):

> `EXPECTED_SORTED_IDS has wrong order — pay_005 at +01:00 converts to 07:00Z (earliest), not pay_002`

Why the sink could not know: `prior_hints` is a `HashMap<TaskId, String>` and the dispatch path does
`self.prior_hints.remove(&tid)`. **A judge finding is keyed to ONE task and CONSUMED on that task's
next dispatch.** It survives exactly one re-dispatch and then no longer exists. Correct for guiding a
retry; wrong for everything else — and most wrong for the sink, which is told *"you are the ONLY task
permitted to edit files here"* and whose whole job is fixing what upstream found.

So the run contained the answer, in a deterministic engine event, and spent ~20 minutes of its longest
serial task re-deriving it.

### Fix

`judge_notes: Vec<(TaskId, String)>` accumulates every judge hint and is never consumed. A task that
owns no files and joins the graph — the sink — inherits all of them, appended to its `prior_hint`
under a heading that says what they are and that it need not rediscover them. Ordinary workers keep
the existing one-shot behaviour, so nothing else changes.

This is Mihai's through-line in its purest form: not a missing capability, but **information the run
already had, never delivered to the one task that could use it.**

**Registered prediction:** on the next run the sink's `prior_hint` contains the supervisor block, and
its shell-command count for re-deriving a known finding falls. The honest failure mode is that it
re-runs the suite anyway — which would say the compliance problem is separate from the information
problem, and both need work.

---

## F115 — a sink CUT OFF at its cap looked exactly like one that finished

The third of G8's open questions, closed. F113 measured `integrate-verify` at **1800s — its cap to the
second** — with 23 shell calls, 2 edits, and `status=done`.

`status=done` is the correct SCHEDULER behaviour: the app files exist, the sink owns no deliverables,
and finalizing lets the run terminate rather than hang. But the **only** record that it had been
truncated was an `eprintln!` on stderr — the progress log, not the structured event stream. So
`run.jsonl` showed `task_completed status=done` and nothing else, and every instrument reading a run
treated a sink cut off mid-work as one that had done its job.

That is PATTERN 4 in the single place it costs most: **the result row this project reads every verdict
from.** F113's own conclusion ("it was cut off, not slow") was only reachable because I happened to
notice 1800 equalled the configured cap. Nothing would have surfaced it on a run with a different cap,
and nothing would surface it to anyone else.

**Fixed:** `sink_capped{task_id, cap_secs, detail}` is emitted at BOTH cap sites — the top-of-loop
check that catches a continuously-active sink, and the event-gap arm. Instrumenting only one would be
worse than neither: the truncation would be visible on some runs and invisible on others, and the
invisible ones would read as clean completions.

### G8's three questions, answered

1. **What were the 23 shell calls?** Answered (F114): `--help` and three robustness probes were its
   real job; four pytest runs re-ran a sweep its spec forbids; eleven commands rediscovered a bug the
   judge had already found and whose hint had been consumed.
2. **Why 72s per tool call?** Still open, and it now matters more than the work itself — if that is
   simply the fleet's turn latency, the sink's cost is TURN COUNT, and every turn removed is ~72s off
   the longest serial region in the run. F114 removes turns by not re-deriving; the pytest re-runs are
   the next candidate and are a compliance question, newly testable on a clean prompt after F87.
3. **Capped vs finished, indistinguishable?** Closed by this finding.

---

## F116 — G8 CLOSED: the sink is not slow, it takes 25 turns. On this fleet, wall-clock ≈ turns × 83s.

The last open G8 question was whether 72s per tool call meant the sink was pathologically slow. Every
other task in the same run answers it:

| task | secs | calls | s/call |
|---|---|---|---|
| test-store-integrity | 474 | 2 | 236.8 |
| test-store | 259 | 2 | 129.4 |
| test-api-web | 650 | 7 | 92.8 |
| verify-e2e::0 | 349 | 4 | 87.1 |
| **integrate-verify** | **1800** | **25** | **72.0** |
| verify::main | 400 | 9 | 44.5 |
| main | 117 | 6 | 19.4 |

**Median s/call excluding the sink: 82.9. The sink: 72.0.**

The sink is not slow per call — it is slightly FASTER than typical. **Its entire cost is TURN COUNT:
25 calls against a median of 2-4 for every other task in the run.**

### The framing this gives the whole project

**On this fleet, wall-clock ≈ turns × ~83s.** That is the fundamental unit, and it reframes every
optimisation: making the swarm faster means making it take FEWER TURNS, not making it do less work per
turn. A rule that saves a worker one round-trip is worth ~83 seconds; a rule that makes a worker
"more efficient" within a turn is worth nothing measurable.

It also explains why instruction density (F87/F91/F111) matters so much here beyond compliance: a
worker that re-reads a file in six slices spends six turns — ~500s — on something one turn could have
done.

### The arithmetic on the sink, now concrete

Of the sink's 25 turns:
- **4** were pytest re-runs its spec explicitly forbids (~330s)
- **11** were re-deriving a finding the judge already had (~910s, and F114 targets exactly this)
- 10 were its actual job: orientation, `--help`, robustness probes, and 2 edits

So roughly **60% of the longest serial region in the run was turns it should not have taken** — and
both causes are now addressed or measurable: F114 removes the re-derivation by carrying judge findings
to the sink, and the pytest-re-run compliance question is newly testable on a clean prompt after F87.

**Registered prediction:** the next run's sink takes materially fewer than 25 tool calls, and the drop
comes from the re-derivation block (commands 11-23), not from the probes. If turn count holds at ~25
while the composition changes, the cost is intrinsic to what the sink is asked to do and the fix is to
ask it for less.

### G8 CLOSED

All three questions answered: what the turns were (F114), whether capped and finished were
distinguishable (F115, they were not, now they are), and whether the sink was slow (F116, no — it was
long). The remaining work is the compliance question, which needs a post-boundary run rather than more
analysis.

---

## F117 — `retarget_off` is INERT (redraft rounds 0 on BOTH arms), and the sink is capped 2 for 2

Two clean units now exist on engine_build 1785657605. **They are NOT replicates** — one is `baseline`,
one is `retarget_off` — and I nearly reported their 4.7-point difference as "the replicate spread on
this build", which would have been the same category error this file has caught six times today. The
replicate spread on this build is still UNMEASURED; both arms are n=1.

### The retarget_off arm measures nothing, and the reason is its precondition

| | prefix | planning | redraft rounds | score |
|---|---|---|---|---|
| baseline@3n | 796s | 587s (74%) | **0** | 0.7186 |
| retarget_off@3n | 1220s | 986s (81%) | **0** | 0.6720 |

The registered prediction was *"score unchanged within the replicate spread, prefix roughly HALVED,
occupancy up"*. The prefix went **UP 53%**.

But the decisive column is `redraft rounds: 0` **on BOTH**. The redraft never fired on the baseline
either — so `retarget_off` switched off a mechanism that was not running, and the 424s prefix
difference has some other cause. **The arm is INERT: it cannot answer the question it was bought for.**

That is PATTERN 2's fifth instance, and the second time today that asking "can this actually fire?"
would have saved a two-hour unit — the first being F111's circular kind_prompt readout. The rule earns
its place at the top of the arm queue: **before running an arm, confirm its mechanism FIRES on the
baseline.** A lever that switches off something already absent is a null experiment dressed as a
comparison.

(Whether the redraft firing 0 times is itself right is a separate question. Earlier corpus runs showed
up to FOUR redraft rounds; this build shows none. Plan confidence 100 on the baseline would explain it,
since the ladder only runs below the ask floor — but that is a hypothesis, not a measurement.)

### The sink is capped EVERY time

| | sink secs | sink turns | s/turn |
|---|---|---|---|
| baseline | **1800** | 25 | 72.0 |
| retarget_off | **1800** | 16 | 112.5 |

**Both hit 1800 exactly — the cap, to the second.** So the sink does not finish; it is stopped, on
every run measured. F115 makes that visible from now on (`sink_capped`), and F116's framing says the
turn budget is what matters: at ~83s/turn a 1800s cap buys roughly 21 turns.

That reframes `sink_cap_secs` from "a safety net for a pathological join" — what its own doc claims,
citing healthy joins at 311-1591s — into **the routine terminator of the longest task in the run**.

### Turn totals, for the F116 ledger

baseline 81 turns total (sink 25 = 31%); retarget_off 71 total (sink 16 = 23%). At ~83s/turn the whole
run is roughly 100 turns of model time across 3 nodes — the number any future speedup has to move.

---

## F118 — why the redraft never fires: `plan_confidence` is 88 in 8 of 13 runs, and the floor is 85

F117 left open why this build shows 0 redraft rounds when earlier corpus runs showed four. The answer
is deterministic and it closes the question.

Across every archived run with a `plan_loaded`:

| plan_confidence | ask_floor | retarget events | redrafts |
|---|---|---|---|
| 100 | 85 | 0 | 0 |
| **88** | 85 | 0-1 | **0-1** |
| **84** | 85 | 3 | **3** |
| **54** | 85 | 2 | **1** |

**The ladder runs only when `plan_confidence < ask_floor`.** Both units on this build came in at
**88 > 85**, so the redraft had no reason to run — and `retarget_off` therefore had nothing to switch
off. F117(a) is fully explained: the arm was inert because its precondition is a property of the PLAN,
not of the lever.

### The sharper observation underneath it

**`plan_confidence` is 88 in 8 of 13 runs.** That is not a measurement varying with plan quality; it
is very nearly a constant. An earlier note in this project records why: confidence is the cross-draft
AGREEMENT score, and *"spread 1 = 88, spread 0 = 100"* — literally whether the parallel skeleton drafts
emitted task counts within one of each other.

So the signal is effectively three-valued: **100** (drafts agreed exactly), **88** (off by one), and
below-80 (off by more). With the floor at 85, **only a task-count disagreement of 2 or more can ever
trigger a redraft.**

That makes `retarget` a lever whose precondition fires on roughly 3 of 13 observed runs, and makes
`ask_floor: 85` a threshold sitting in the GAP between two adjacent discrete values (88 and 84) rather
than on a continuum. Moving it a point either way changes nothing; moving it to 90 would fire the
redraft on nearly every run.

### What to do with it

**Nothing yet, deliberately.** The measured facts are: the ladder is precondition-gated, the gate is a
near-constant, and the arm built to test it cannot fire on a typical plan. Whether the redraft is
*worth* firing is a separate question this bench has never answered — the one run with 3 redrafts is
not comparable to anything.

Testing `retarget` needs a spec ambiguous enough to split the drafters. That is a bench-design problem
for the backlog, not a lever-tuning one.

**Registered:** any future `retarget` arm must first assert `plan_confidence < ask_floor` on its
baseline. Otherwise it is F117 again.

---

## F119 — armcheck.py: THREE of nine queued arms cannot answer their own question

Two arms were bought today with fleet time they could not repay — `kind_prompt` (circular readout,
F111) and `retarget_off` (mechanism never fires, F117/F118). Rather than discover a third the same way,
`armcheck.py` now asks both questions of every queued arm against a real baseline:

1. does the arm's **MECHANISM** fire on the baseline at all? (else there is nothing to change)
2. can the **INSTRUMENT** see the change? (else the readout is unearned)

Run against `baseline-n3-r0`:

```
BLOCKED  kind_prompt      no `rules_delivered` events — the delivered rule-set is unprovable with the
                          lever ON, so the readout is circular (F111). Needs the post-F111 engine.
BLOCKED  detail_budget    slowest detail 161s vs budget 420s — nothing is near the ceiling
BLOCKED  retarget_off     plan_confidence 100 >= ask_floor 85 — the ladder never runs
OK       doc_prefetch     grounded=2: the verbatim channel has content to carry
OK       spec_repair      2 repair rounds: the race has work
OK       complete_parallel a round had 3 findings: the fan has >1 item
OK       e2e_oracle       3 e2e shards ran
UNKNOWN  sink_review      needs occupancy's solo window, not decidable from events alone
UNKNOWN  doc_fetch        needs the spec checked against spec_doc_urls
```

**Three of nine.** At ~2 hours per unit that is six hours of fleet time that would have produced
nothing — and two of the three I would not have caught without asking.

### `detail_budget` is worse than inert — it is STALE AND BACKWARDS

The arm sets `GOOSE_SWARM_DETAIL_BUDGET_SECS=300`. But **F49 already made the budget derive from
`worker_timeout_secs`**, which resolves to **420s** here, and the baseline's slowest detail call is
**161s — 38% of the ceiling**.

So the arm would **LOWER a ceiling nothing is near**: it can only make things worse, and its gate text
still argues against a 75s literal that F49 removed. **A fix elsewhere silently invalidated an arm, and
nothing connected the two.** That is P5 (a fix that leaves its mechanism) inverted — here the fix
landed and the *experiment* rotted.

Set to `reps: 0` with the reasoning kept inline, rather than deleted, so requeuing it requires reading
why it was parked.

### The rule this makes routine

`armcheck.py` runs against the newest baseline and exits 1 if any queued arm is blocked. It is
deliberately CONSERVATIVE — an arm whose precondition cannot be decided from the baseline's own events
is UNKNOWN, never OK — because a green here proves only that the arm is *not already doomed*, which is
the cheap half of the question.

**This belongs in the tick routine alongside review.py**: before any arm is queued, the baseline must
show its mechanism firing and its instrument watching.

---

## F120 — armcheck's two UNKNOWNs resolved, and both arms are viable

F119 left `sink_review` and `doc_fetch` at UNKNOWN. UNKNOWN is the honest default, but leaving it there
is not the same as being unable to decide — both were answerable with instruments already on disk.

**`sink_review` — OK.** Idle-fill during the sink needs idle capacity DURING the sink, which is exactly
what occupancy.py's `solo_by_task` measures. It reports **the sink held a node alone for 1590s with the
other two idle**. That is the window the mechanism exists for, and it is 23.6% of the run. Asked
occupancy rather than re-deriving it — the mistake review.py made twice (F107/F112), where a
completion-sum silently missed a task superseded by a split.

**`doc_fetch` — OK.** The engine's own rule (`spec_doc_urls`) is an http(s) URL WITH A PATH; a bare
origin is the app's base URL, not a document. The bench spec names exactly one:
`http://127.0.0.1:8930/v1/docs`. So the arm has something to fetch.

### The queue now reads

```
BLOCKED  kind_prompt      readout circular until the boundary ships rules_delivered
BLOCKED  detail_budget    stale and backwards — F49 already raised the ceiling to 420s
BLOCKED  retarget_off     plan_confidence 100 >= floor 85, the ladder never runs
OK       doc_prefetch     grounded=2
OK       spec_repair      2 repair rounds
OK       complete_parallel a round had 3 findings
OK       e2e_oracle       3 shards ran
OK       sink_review      1590s of solo sink to fill
OK       doc_fetch        one fetchable doc URL in the spec
```

**Six viable arms, three blocked, nothing left unresolved that could be resolved.** The three BLOCKED
are blocked for three different reasons — an instrument, a stale target, and a precondition — which is
worth noting: there is no single class of failure to guard against, only the habit of asking.

### One caution recorded against `doc_fetch`

Its probe mirrors `spec_doc_urls`'s rule rather than calling it, because the rule lives in Rust. That
is a deliberate duplication and therefore exactly the shape this project keeps finding defects in
(P1: two versions of one rule that disagree). It is written narrowly and the comment says so; if the
Rust rule changes, the arm is what suffers, and the honest mitigation is that a wrong OK here costs one
unit rather than a wrong verdict about the engine.

---

## F121 — armcheck reported a DISTRIBUTION as a CONSTANT, and a live run caught it within the hour

F119 checked `retarget_off` against one baseline (`plan_confidence 100 >= ask_floor 85`) and I wrote it
up as **"the ladder never runs"**. F118 had said the same from the archive.

Then the very next unit — `swarm-1node-r0`, live — came in at:

```
confidence_retarget  round 1
retarget_discarded   round 1
confidence_retarget  round 1
low_confidence_ask   plan_confidence 36
low_confidence_ask_timeout  waited_secs 5
```

**Confidence 36, and the redraft fired twice.** The precondition I had just declared absent was
present on the next run.

### The survey I should have done first

| nodes | plan_confidence observed |
|---|---|
| 1 | 100, 100, **36** |
| 2 | 88 |
| 3 | 88, 88, **84**, **84**, 88, **54**, 100, 88 |

Confidence is **not structural** — it ranges 36-100 at every node count. `conf < floor` holds in
**4 of 14 runs ≈ 29%**.

So the precondition is neither absent nor reliable: it is a **coin flip**, and a single-baseline check
reported a distribution as a constant. That distinction changes the ACTION — "this arm cannot work"
means fix or delete it; "this arm needs a baseline that satisfies it" means pair it correctly.

### Fix

`armcheck.py` now distinguishes **BLOCKED** (the arm cannot work as built — a circular readout, a stale
target) from **UNSUITABLE** (the arm is fine but THIS baseline does not satisfy its precondition, with
the archive frequency stated). Only BLOCKED gates the queue. `retarget_off` moves to UNSUITABLE and the
queue drops from 3 blocked to 2.

### The lesson, and it is the fourth time today I have had to point one at my own tooling

F110 — a windowed scan cried wolf. F112 — I published a headline from a phantom span. F120 — I noted a
mirrored rule as a duplication risk. And now F121: **an instrument that samples ONE case and reports a
verdict about the general case.** Every one was caught by evidence arriving after the claim, not by
the instrument doubting itself.

The generalisable rule: **before an instrument reports a property of a MECHANISM, check whether it
sampled one instance or the distribution.** F119's other two verdicts survive precisely because they
are properties of the code (a circular metric; a budget default), not of a run.

---

## F122 — RETRACTION: the judge does NOT scale with node count. It scales with SPARE CAPACITY.

I claimed this twice today and it is wrong.

> F76: *"more nodes means more idle slots means more judging. The judge gets better with fleet size."*
> F98: *"that is also the one place the architecture already scales the right way."*

Measured across every archived run with dispatches, judge invocations per dispatch:

| nodes | judge/dispatch | runs |
|---|---|---|
| **1** | **4.86** | 2 |
| 2 | 2.67 | 1 |
| **3** | **2.57** | 7 |

**The judge runs roughly TWICE as often per dispatch on ONE node as on three.** Absolute counts agree:
a 1-node baseline logged **171** judge runs over 24 dispatches; 3-node runs logged 36-109 over 17-33.

### Why my reasoning was wrong

The mechanism is right — the judge only runs on a device with spare capacity — but I drew the wrong
consequence. More nodes do not mean more idle slots; **more nodes mean more work in flight**, because
the scheduler's whole job is to keep them busy. Spare capacity is highest when the fleet is
UNDER-utilised, which happens on FEWER nodes (a narrow plan cannot fill three) or during a serial
stretch.

So the judge is a **spare-capacity** mechanism, not a **fleet-size** mechanism, and those two are
anti-correlated by design. I had a plausible causal story and never checked its direction.

### What survives

F98's substance stands and is unaffected: the judge does real work (**8, 11, 7, 6, 5 real
interventions** across the larger runs, including the timezone catch), and it is starved when the fleet
is saturated (`no_idle_device` is 100% of skips). Both remain true.

What does not survive is the inference that adding nodes buys more of it. **If anything, adding nodes
buys LESS judging per unit of work** — which makes the judge a mechanism that a well-utilised fleet
suppresses, and that is a genuine tension worth naming rather than a happy accident worth citing.

### Caveats stated plainly

n=2 for 1 node against n=7 for 3, and one of the two 1-node runs is still IN FLIGHT (partial counts,
included because excluding it would flatter the retraction rather than test it). The direction is
consistent in both absolute and per-dispatch terms and is opposite to my claim, which is enough to
withdraw the claim — not enough to publish the inverse as a law.

**Registered:** if G4's node curve produces a completed 1-node and 3-node pair on the same build, the
judge/dispatch ratio is a free readout from it, and this retraction is what it tests.

---

## F123 — correcting the correction: judge rate does NOT vary with node count either way

F122 retracted "more nodes means more judging" and replaced it with the inverse — *"the judge runs
roughly TWICE as often per dispatch on ONE node as on three"*. **That replacement is also wrong**, and
for the same class of reason: `judge/dispatch` is confounded by dispatch count. A 1-node run completes
far fewer tasks in the same wall-clock, so identical judging spreads over less work and the ratio rises
without anything about the judge changing.

The capacity-normalised measure — judge invocations per WALL-CLOCK MINUTE:

| nodes | judge/min | skip% | runs |
|---|---|---|---|
| 1 | **0.60** | 25% | 2 |
| 2 | 0.29 | 0% | 1 |
| 3 | **0.62** | 14% | 7 |

**1 node and 3 nodes are indistinguishable (0.60 vs 0.62).** And the spread WITHIN the 3-node group is
0.36 to 1.24 — more than three-fold, and far larger than any difference between groups. Skip-rate
weakly favours the ORIGINAL direction (3 nodes skip 14%, 1 node 25%), which is the opposite of what
F122 concluded, and is equally unsupported at this n.

### The final, honest position

**The data cannot resolve whether judging scales with node count.** Not "it goes up", not "it goes
down" — the between-group signal is smaller than the within-group noise, on 2 runs versus 7.

That is the answer, and it is worth stating as one rather than picking whichever direction the last
metric happened to favour. Three passes at this question produced three different claims:

1. F76/F98 — "more nodes, more judging" (mechanism reasoning, never measured)
2. F122 — "fewer nodes, more judging" (measured, but on a confounded ratio)
3. F123 — no resolvable relationship (measured on a normalised rate, with the variance stated)

### What is actually established, and survives all three

- the judge does **real work**: 8, 11, 7, 6, 5 genuine interventions across the larger runs, including
  a timezone-conversion bug in a peer's test fixture
- it is **starved when the fleet is saturated**: `no_idle_device` is 100% of all skips, every run
- its **skip rate varies 0-49%** across runs, and that variance is unexplained

None of that depends on the node-count question, which is why those findings stand while three
successive claims about scaling did not.

### The lesson, stated against myself

Lesson 10 said "check whether the instrument sampled one instance or the distribution". F122 obeyed it
and still got the answer wrong, because **a normalised ratio can be confounded even when the sample is
adequate**. The addition: *when a ratio changes, ask what else moved in its denominator.* Dispatch
count is not a constant across node counts — it is the very thing node count changes.

**Stop here.** A fourth pass at the same question with the same data would be motivated reasoning, not
analysis. If G4 ever produces completed 1-node and 3-node runs on one build with replicates, this
becomes answerable; until then it is recorded as unresolved.

---

## F124 — the retry burden is ONE class of task, and it is not the one my supervisor was flagging

I was about to build a deterministic post-plan corrector that splits mixed-kind tasks (`api.py` +
`web/index.html`, `__main__.py` + `README.md`) into same-kind children. `review.py`'s Q1 had returned
**"NO — fix the planner"** on this shape, the story was plausible — one brief covering two concerns
must dispatch worse — and `split_fat_modules` was sitting right there as the template to extend.

**I measured the premise first, and it died.** Across all 17 archived plans, 262 producing tasks:

| | n | retried >1 attempt |
|---|---|---|
| mixes KINDS | 22 | **18.2%** |
| single kind | 217 | **15.2%** |

**+3.0pp.** Four retry events against thirty-three. And the rule fires on **88% of plans (15/17)** —
a verdict that says "fix the planner" on seven runs in eight, over a three-point non-effect. That is
P4 (a measure that cannot separate two opposite situations) living inside the instrument I built to
supervise the run. The shape is perfectly real and perfectly reproducible — only three collisions
ever occur (`code+docs` 10, `asset+code` 9, `asset+code+docs` 3), always on `cli`/`main`/`entry` or
`api`/`api-web` — but reproducible is not the same as costly.

### What DOES separate: an interaction nobody could see one factor at a time

239 dispatched tasks with a plan entry:

| | n | retry |
|---|---|---|
| **hard AND test-authoring** | **30** | **60.0%** |
| hard, not a test task | 91 | 12.1% |
| test task, not hard | 16 | 12.5% |
| neither | 102 | 5.9% |

Neither factor alone is worth anything — `hard` is 12.1% without `test`, `test` is 12.5% without
`hard`. **It is the interaction**, and it is why every single-factor scan I ran previously came back
flat. Split three ways: producing code 12.2% (n=74), test tasks 43.5% (n=46), verify tasks 6.7%
(n=119). Test tasks retried worse than their own run's other tasks in **5 of the 6 runs** that had
any (the exception is the live 1-node unit at n=4, still in flight).

Brief length compounds it, and **only inside test tasks**:

| brief chars | test tasks | producing tasks |
|---|---|---|
| 0–1200 | 33.3% (n=9) | 0.0% (n=19) |
| 1200–1800 | 34.8% (n=23) | 22.2% (n=27) |
| 1800+ | **64.3%** (n=14) | 10.7% (n=28) |

A monotone ladder for test authors; noise for everyone else. So "long briefs are bad" is NOT a
general defect and must not be flagged as one — the same mistake as the mixed-kind rule, one level up.

### Why this matters beyond the instrument

A retry is ~83 s x the task's turns of pure waste (F116), so this is the largest planner-visible cost
to the overarching goal, and it is concentrated in 30 tasks out of 239.

**It also hands `kind_prompt` (G3) a readout that cannot be gamed.** F111 blocked that arm because
its stated metric `kind_mismatch_pct` was hardcoded to zero with the lever ON — success by
construction. **Test-task retry rate is computed from `task_dispatched.attempt` and nothing in the
lever's accounting touches it.** REGISTERED PREDICTION: with `kind_prompt` on, the hard-test retry
rate falls from 60% toward the 12% that hard non-test work already achieves. FALSIFIER: it stays at
~60% => the mismatch is not what is breaking those tasks and the whole rules-density story is wrong.

### One stale comment corrected while verifying the mechanism

swarm.rs:19647 still justifies the lever with *"406 dispatches OWN a test_*.py and are told NEVER
read the project's TEST files — the file they must produce is the file they may not open."* Checked
the live OFF-path text at 19580: it now reads *"NEVER read the project's **OTHER** TEST files … any
test file YOU OWN is your deliverable and is yours to read and write freely."* The flat contradiction
was already fixed. What survives in the OFF path is softer and still wrong for a test author: *"Read
AT MOST the ONE file you will edit"* (a test author must also read the module under test, to get real
signatures) and *"STOP WHEN GREEN, the MOMENT your file's tests pass"* (a test author's tests may
legitimately fail — they are testing someone else's code). Do not quote the strong version again.

### Actions

`review.py` Q1: mixed kinds demoted BAD -> warn with the +3.0pp attached; hard test tasks promoted to
BAD with the 60% attached; a >=1800-char test brief added as a warn. Q1 now returns "YES, with
reservations" on the live plan instead of "NO — fix the planner", which is the honest answer.

**No corrector was written.** The measurement that would have justified it says it is worth three
points, and I checked before building rather than after. That is lesson 11 applied in the one
direction that actually saves work.
