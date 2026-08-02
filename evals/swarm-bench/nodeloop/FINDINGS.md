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
