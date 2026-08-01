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
