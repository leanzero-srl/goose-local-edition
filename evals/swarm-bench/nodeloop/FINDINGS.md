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
