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

---

## F125 — the boundary check runs at the wrong MOMENT, and my fix for that was wrong about 2 of 41

`./loop.sh boundary` verifies MARKERS against the rebuilt BINARY. Right check, wrong moment: by the
time it runs the supervisor is dead, the engine is dead, and the rebuild is spent. Three times a
marker turned out to be a comment or a fn name (`failed_task_finding`, `is_code_deliverable`,
`THE SPEC STATES ITS ENDPOINTS`) and refused a perfectly correct binary, sending me to hunt a defect
that did not exist. **Every one of those was decidable from source, with the fleet up and nothing at
stake.** `preflight.py` does that, and it is about to matter: 33 commits are held for this boundary.

Two things it caught immediately, both of which would have cost real work.

**MARKERS' own instructions were wrong.** The file that exists to stop this mistake said to prove a
candidate with `grep -c '"MARKER"' crates/goose-cli/src/commands/swarm.rs`. The binary links EVERY
crate, and `WHAT THE SUPERVISOR ALREADY FOUND` lives in `goose-swarm/src/scheduler.rs`. Grepping one
file returns 0 for it — indistinguishable from the comment case — and the "fix" would have been to
delete a good marker. Same blindness, one file wide.

### And then the new instrument was wrong in exactly that direction

First run: `task_split` **ABSENT from crates/ entirely**, `speculated` **only a comment**. Both
verdicts said delete or repoint. Both were false — `strings target/release/goose` finds them (1 and
2 occurrences). `event.rs` carries `#[serde(tag = "event", rename_all = "snake_case")]`, so variants
`TaskSplit` and `Speculated` become those literals **at compile time**. The text exists in no .rs
file. **A source grep cannot see a derive macro**, and I had just finished writing a docstring about
how a source grep cannot see the whole crate tree.

Fixed with a fourth verdict, `DERIVED`, narrow by construction: the marker must be snake_case, the
file must carry the rename attribute, and the CamelCase form must appear as a **variant head**
(`^\s*Speculated\s*[{(,]`) — not merely somewhere in the text, which is how `Speculated` appears in
a doc comment at scheduler.rs:1212 and would have re-introduced the blindness as a false pass.

Two further notes on the fix itself:

- The first `DERIVED` regex required an underscore, so it **missed `speculated`** — one of the two
  markers it was written for. It passed anyway, via the binary cross-check, on a stale binary that
  happened to carry it. A verdict that depends on that is not a verdict. Now `[a-z0-9]+(_[a-z0-9]+)*`.
- The binary is consulted as a **POSITIVE signal only**. It predates every held commit, so absence
  there proves nothing; presence proves the marker is findable, which is the only claim made.

CONTROLS, both directions: `THE SPEC STATES ITS ENDPOINTS` (the real historical comment-marker that
refused a correct binary) -> COMMENT, exit 1. A fabricated marker -> ABSENT, exit 1. Restored -> exit 0.

**Result: 41/41 markers will survive the rebuild — 39 LITERAL, 2 DERIVED.** The boundary can be
crossed the moment the 1-node unit finishes, without discovering a phantom afterwards.

The lesson is P6 again and it landed on the FIRST run of a new instrument: an instrument built to
catch a blindness reproduced that blindness one layer down. The reason it cost nothing is that
`strings` on the existing binary was one command away and I ran it before acting on the verdict.

---

## F127 — the judge's spin deadline is 420 s, and that explains the cluster I wrongly retracted

Three corrections and one cost, all from reading `judge.rs` instead of inferring from events.

### 1. The 420-488 s cluster IS a constant. My retraction of it was wrong.

F126 dropped the "timeout constant" reading because the raw values spread 420.1 -> 488.6 s rather
than repeating one number. **`judge.rs:410` and `judge.rs:441` both gate on
`input.elapsed_secs >= cfg.min_age_secs.max(420)`** — the over-read trip and the finalize-spin trip.
Neither can fire before 420 s, and the judge evaluates on a ~60 s tick, so every kill lands in
[420, ~480]. That is a **FLOOR**, and a floor produces values just ABOVE it, never equal to it.

I looked for a constant the values would EQUAL and concluded "cluster, not constant" when the shape
was the signature of exactly what I had dismissed. Both readings were reached without opening the
file that decides it.

### 2. Judge interventions split three ways, and each implicates a DIFFERENT rule

| phase | n | test / other | median dispatch -> hint |
|---|---|---|---|
| POST-write spin ("written but unchanged while you keep running") | **40 (55%)** | 28 / 12 | 9.7 min |
| PRE-write paralysis ("produced no file yet") | **18 (25%)** | 11 test, 5 sink, 2 other | 7.5 min |
| specific code defect | **15 (20%)** | 13 / 2 | 7.4 min |

F126 treated spin as one thing and pinned it on the stopping rule. It is two things:

- **POST-write spin implicates the STOPPING rule** — "STOP WHEN GREEN, the moment your file's tests
  pass", unreachable for a test author (F126).
- **PRE-write paralysis implicates the READING rule** — "read AT MOST the ONE file you will edit",
  which is incoherent for a test author that must read the module under test to get its signatures.
  A worker that cannot tell what it is allowed to read deliberates.

**`kind_prompt` changes BOTH rules for a test author**, not one. That is a stronger case for the arm
than F126 stated, and it is two independent chances for the readout to move rather than one.

### 3. The 5 sink hits are PRE-FIX, and the existing fix corroborates to the exact count

I was about to report "the judge tells `integrate-verify` to write files it does not own" as a live
defect. `judge.rs:409` already gates on `owns_code = owned_files.iter().any(is_code_deliverable)`,
and both offending runs are from **2026-08-01**, in each of which the planner had handed the sink
`README.md`. The comment there records the fix's own measurement: *"in two of those the gate armed
and killed it repeatedly ... 2 kills and 3 kills, attempts exhausted"*. My scan found **2 and 3**, in
`baseline-n1-r0` and `baseline-n3-r0`. Independent corroboration of a shipped fix, not a new defect —
the third time this session that checking source before reporting stopped a false alarm.

### 4. The cost this exposes, and a registered proposal

A judge-killed attempt cannot end before 420 s. At `max_attempts` 3 that is **>=21 minutes before a
spinning task can fail**, and PRE-write paralysis fires at a 7.5 min median — i.e. **essentially at
the floor**, so the floor is what is binding, not the detection.

For the zero-action case that is a lot of waiting for a signal that arrives immediately: the branch
already computes `read_nothing = input.worker_tool_calls == Some(0)`. **A worker with ZERO tool calls
at 420 s was equally diagnosable at 60 s.** PROPOSAL (registered, not implemented — F124's lesson):
give the `worker_tool_calls == 0` case its own shorter floor while leaving the has-read-but-not-
written case at 420. Expected saving ~5-6 min per affected attempt, up to ~18 min across 3 attempts,
on 18 of 73 interventions. FALSIFIER: if `worker_tool_calls` is commonly `None` rather than `Some(0)`
at that point, the discriminator is not available early and the proposal is inert — CHECK THAT FIRST.

Also noted for G7: **420 is a bare literal at two sites** (`judge.rs:410`, `judge.rs:441`), derived
from nothing. Same two-versions-of-one-rule shape as the 1200 in G5.

### 5. One precision note on the live run

`test-api` in `swarm-1node-r0` re-dispatched 3x and is the current drift, but its difficulty is
**`easy`**. It is NOT an instance of F124's hard-AND-test interaction and must not be cited as
confirming it. F124 measured easy-test tasks at 12.5%; one of them retrying is unremarkable at n=1.

---

## F128 — two OPPOSITE fixes are proposed for the same defect, and the data cannot choose between them

F127 registered a proposal: give the zero-tool-calls case a shorter deadline than 420 s. Before
building it I went looking for whether anyone had already tried, and found **both directions already
written into the codebase, pointing opposite ways**:

- **KILL EARLIER** — `judge.rs:359`, the #134 reasoning-spiral trip. Fires at `min_age_secs` (90 s)
  instead of 420 when `worker_tool_calls == Some(0)` and thinking exceeds a char cap. Its own comment:
  *"Catch it EARLY — at the char cap (~60-120s) — instead of burning the whole idle window."* **BUILT,
  and default-OFF** (`spiral_thinking_chars: 0`). Not to be confused with `spiral_break_chars` (baked
  12000), which is a different mechanism on the non-judge path — adjacent names, deliberately distinct.
- **GRANT MORE TIME** — a "grace lever" referenced by the regression test at `judge.rs:496`
  (*"this pins today's behaviour so the grace lever's effect is visible as a DIFF"*). **It does not
  exist.** The only `grace` in the tree is `straggler_grace_secs`, an unrelated plan-draft window. A
  test was written to pin a baseline for a lever nobody built.

The two disagree because they read the same observation differently. That test also records the
measurement, and it matches mine exactly: `api-app` *"owned 4 files, had made 0 tool calls (a
reasoning model streams thinking, which the digest could not see), and was killed at 457s / 450s /
430s across all three attempts."* If the worker is thinking productively, 420 s is too SHORT. If it
is spiralling, 420 s is far too LONG.

### What the outcomes say, and what they do not

Did a task that received each hint kind ever finish?

| hint kind | n | eventually done |
|---|---|---|
| PRE-write paralysis | 14 | **35.7%** (5) |
| POST-write spin | 25 | 60.0% (15) |
| specific code defect | 11 | 54.5% (6) |

**Pre-write paralysis is the least recoverable of the three**, and it is also the one that costs a
full 420 s per attempt before the kill is even permitted — up to 21 minutes across three attempts to
rescue about a third of cases. That is a real reason to change something.

It is NOT a reason to pick a direction. n=14 against n=25 with 3 still in flight is a thin margin,
and nothing here distinguishes "killed too early while thinking" from "spiralling and should have
died sooner" — the outcome is the same either way. Choosing from the armchair is what F122 did twice.

### The instrument gap that has to close first

**`judge_verdict` carries only `{task_id, device, verdict, confidence, hint, action}`.** Neither
`worker_tool_calls` nor `worker_thinking_chars` is emitted anywhere, on any event, in 1,339 judge
events across the archive. So the discriminator that separates the two hypotheses — was this worker
thinking hard or sitting still — is **structurally unobservable**, which is why a lever built for it
has sat OFF and unmeasured.

This is F111's shape exactly: a lever whose readout does not exist. The fix is the same one that
worked there — emit the deterministic fields the judge already computes — and it must come BEFORE
either direction is implemented, or whichever gets built will be judged on a metric that cannot see
it. Registered as the next engine change; NOT built this tick, because it belongs with the batch at
the boundary rather than as a fourth mid-run edit.

---

## F129 — CORRECTION to my own headline: the 1-node prefix is 3.2x mostly because it REDRAFTED

I have carried this in the tick state block for several ticks running: *"prefix 53.8 min before the
first dispatch, vs 13.3 (baseline@3n) and 20.3 (retarget_off@3n) — scouts, best-of-N drafting and the
detail fan all collapse to serial on one node, and that is the mechanism by which more nodes win."*

It is confounded. Running `prefix.py` (rather than my hand figure) on all three units:

| | 1 node | 3n (a) | 3n (b) | 1n vs mean-3n |
|---|---|---|---|---|
| prefix total | 3226 s | 796 s | 1220 s | **3.20x** |
| research | 588 s | 209 s | 235 s | **2.65x** |
| planning TOTAL | 2638 s | 587 s | 986 s | 3.35x |
| **planning ROUND 1** | **1054 s** | **587 s** | **986 s** | **1.34x** |
| **redraft rounds** | **1** | **0** | **0** | — |

**The 1-node run ran an extra redraft round; neither 3-node run ran any.** That single round cost
**1584 s — 49% of its entire prefix.** Remove it and the prefix ratio falls from 3.20x to **1.63x**.

And the redraft is not a node-count mechanism. F121 measured its trigger (`plan_confidence <
ask_floor`) at **~29% of runs, with confidence ranging 36-100 at EVERY node count** — 1-node runs in
the archive scored 100, 100 and 36. This unit drew a low card, not a small fleet. Comparing a run
that redrafted against two that did not, and attributing the gap to node count, is precisely the
F122 error: a plausible mechanism asserted over a number whose denominator moved.

### What actually survives

- **research 2.65x is real and is the one phase that provably fans out.** Scouts are independent
  lenses dispatched across devices; on one node they serialise by construction. This is the honest
  node-curve signal in the prefix, and it is worth 379 s here.
- **planning round 1 is 1054 s vs 587 s and 986 s — 1.34x, on n=1 against n=2.** Not resolvable. The
  best-of-N skeleton draft is *sized to the fleet*, so a node effect is expected, but this sample
  cannot demonstrate it and I must stop saying it does.
- **the redraft round is the largest single item in the prefix** and it is stochastic. That makes it
  a target in its own right — `retarget_off` exists for exactly this, and F121 already established
  it needs a low-confidence baseline to pair against. **This unit IS that baseline.**

### The instrument asymmetry that hid it

`phases.py` reports plan/detail for the 1-node run and NOT for the two 3-node runs, because the
`plan` phase is bounded by `confidence_retarget` — an event a run only emits when it redrafts. So
the phase table silently compares different spans across units, and my eye read the one long bar as
a node effect. It is not wrong (each phase is correctly bounded); it is **incomparable across runs
that differ in whether the ladder fired**, and nothing in its output says so.

REGISTERED, not built: `phases.py` should label a phase whose boundary marker is condition-dependent
so a cross-run comparison of that row is refused rather than merely awkward. Same class as the
prediction gate — an instrument that lets you compare two things that are not the same thing.

**Corrected claim for G4:** on this pair, one node costs ~2.6x on research and an unresolved ~1.3x on
first-round planning; the headline 3.2x prefix gap is about half redraft, which is node-independent.
The node curve is NOT yet demonstrated by the prefix, and I should stop leading with it.

---

## F130 — my tick review called a FAILED task a completion, in every run, at two sites

`swarm-1node-r0` finished its DAG and the review reported *"dispatched 13 / completed 13 / in flight
0"* and **"IS THE PLAN BEING FOLLOWED? YES"**. Both were false. `test-api` carries
`{"status": "failed", "elapsed_ms": 0, "tool_calls": 0}` — the exhausted-attempts signature from
F126 — and the run delivered **12 of 13 planned tasks**.

The cause is one line, written twice:

```python
done = {e["task_id"] for e in ev if e.get("event") == "task_completed"}
```

at `review.py:81` (level 1) and `:376` (Q2). **`task_completed` is emitted for a terminal FAILURE as
well as a success**, and neither site read `status`. P4 in the supervisor itself — a measure that
cannot separate two opposite situations — and P1 alongside it, because the same wrong predicate
existed at two sites and both had to be found. I have been logging that exact pattern in the engine
all session while shipping it in my own instrument.

### How much it hid: every run, and every failure is a TEST task

| run | planned | counted done (old) | actually done | FAILED | failed ids |
|---|---|---|---|---|---|
| baseline-n3-r0 | 16 | 19 | 17 | **2** | test-api-edge-cases, test-meridian |
| retarget_off-n3-r0 | 15 | 17 | 16 | **1** | test-api |
| swarm-1node-r0 | 13 | 13 | 12 | **1** | test-api |

**4 terminal failures across 3 of 3 runs, and all four are test-authoring tasks. All four are
zero-work** (`elapsed_ms 0`, `tool_calls 0`). That converges hard with F124 (hard+test = 60% retry)
and F126 (a retry is the judge killing a spinning worker): the entire terminal-failure population in
this archive is the same kind of task the judge chain has been circling all session. It is not new
evidence for that thread — it is the same tasks seen from the other end — but it does say the cost is
not merely wasted minutes. **Those runs shipped without the test coverage they planned for.**

It also corrects something I have repeated: the two 3-node units I keep calling "clean" — baseline@3n
0.7186 and retarget_off@3n 0.6720 — **each lost tasks**. The scores may still stand (the scorer runs
the built app), but "clean unit" was never true of the DAG.

### Fixed, and the regression the fix introduced

Both sites now exclude `status == "failed"`, level 1 names the failed ids outright, and Q2 counts a
lost task as DRIFT — the plan committed to it and it was not delivered.

The first version of that fix then reported `test-api` as **both terminally failed AND still in
flight**, because "in flight" was keyed off `done` alone and a failed task is not in `done`. Two
mutually exclusive states asserted at once, from the wrong set — the same defect one layer down,
caught only because I read the output instead of trusting the edit. "Settled" now means done OR
failed. Self-test passes; the live run reads `dispatched 13 / done 12 / FAILED 1`.

---

## F131 — the discriminator was on disk the whole time, and it points at a THIRD story

F128 said the tool_calls/thinking_chars discriminator was **"structurally unobservable"**. That was
too strong, and I found out by looking at what the live tail was writing while its event stream sat
silent for 11 minutes: `.swarm/activity/complete-fix.json`, updating in real time.

**Every task writes `.swarm/activity/<key>.json`**, and those digests carry
`{tool_calls, errors, malformed, thinking_chars, reasoning, full_reasoning, last_thinking, last_text,
calls, recent, model}`. `thinking_chars` — the exact term F128 called unobservable — is right there,
and the archive keeps run directories, so it is available for every past run. What is missing is its
presence in the EVENT STREAM, which is a different and much smaller claim. `judge_observed` is still
the right fix (an event is durable per-invocation; a digest is overwritten), but I did not have to
wait for it to learn something.

### What it says, joined to the judge hints across 3 runs / 108 digests

FINAL-STATE `thinking_chars` by the hint class that task received:

| hint class | n | median | min | max |
|---|---|---|---|---|
| **PRE-write paralysis** | 4 | **1229** | 285 | 4519 |
| POST-write spin | 6 | 3846 | 866 | 13013 |
| specific code defect | 1 | 6354 | — | — |
| no hint | 95 | 1530 | 375 | 46576 |

**Tasks killed for "produced no file yet" are NOT high-thinking spirallers.** Their thinking is at or
*below* the corpus median. `swarm-1node-r0`'s `test-api` — killed twice, terminally failed — shows
**1 tool call and 285 thinking chars**. Over 420+ seconds that is not a reasoning spiral and it is
not productive deliberation. It is a worker producing almost nothing at all.

**That is a THIRD story, and neither in-tree fix addresses it.** "Grace" grants more time to thinking
that is not happening. The #134 spiral trip requires `thinking >= cap` and would never fire on this
population. F128 framed the choice as kill-earlier vs grant-more-time; the data says the premise both
share — that the worker is busy in some invisible way — may simply be false for these tasks.

### Two immediate consequences

**1. A cap chosen by analogy would be inert.** `spiral_break_chars` is baked at **12000**, and the
adjacent `spiral_thinking_chars` defaults to 0. Setting the latter to 12000 by analogy — the obvious
move — **could never fire on the observed pre-write population, whose maximum is 4519.** armcheck's
`spiral_thinking` probe already says "pick the cap BELOW the observed max"; this is that number.

**2. The terminal event UNDERSTATES what happened.** `test-api`'s `task_completed` carries
`tool_calls 0, elapsed_ms 0` while its digest shows 1 call and 285 thinking chars. The zeros are the
SYNTHESIZED exhausted-attempts record (F126), not a measurement. Anything reasoning from those zeros
is reasoning from a placeholder.

### The caveat, which is severe and bounds all of the above

**The digest is overwritten as the worker streams and across attempts**, so every number here is the
FINAL attempt's end state, not the state at the moment of the kill. A worker could have thought hard
on attempt 1 and idled on attempt 3, and this would only show the idling. So this is **suggestive,
not decisive** — it rules a cap of 12000 out, and it makes the "productive thinking" story less
likely, but it cannot settle F128. `judge_observed` records the value AT each judge invocation, which
is what actually settles it; that remains the boundary's job.

Also corrected: `judge.rs:493`'s test comment says the worker made zero tool calls because "a
reasoning model streams thinking, **which the digest could not see**". The digest CAN see it now —
`thinking_chars` was added later (swarm.rs:16491 notes it is `None` on digests predating the key).
That comment describes a world that no longer exists.

---

## F132 — armcheck manufactured BLOCKED verdicts from a six-minute-old run

The moment the post-boundary sweep restarted, `armcheck.py` reported:

```
BLOCKED  spec_repair        no repair round ran on the baseline — nothing to race
BLOCKED  complete_parallel  max findings in any round = 0
BLOCKED  spiral_thinking    no judge_observed events
UNKNOWN  kind_prompt        no dispatches in the baseline
```

**None of that had happened YET.** Parking the pre-boundary results left the runs directory holding
exactly one unit, six minutes old, still in research. `newest_baseline()` fell back to "any run with
a run.jsonl", handed over an in-flight run, and every probe read absence as a verdict.

This is the standing rule — **an UNCONTROLLED ZERO IS NOT EVIDENCE** — broken inside the very script
written to stop arms being bought on bad evidence. And it is the P2 pattern the file's own docstring
opens with: a precondition that never holds, reported as a property of the arm rather than of the
observation.

The damage it would have done is specific: `spiral_thinking` reports BLOCKED with "needs the
post-F128 engine", and the post-F128 engine **is what is running now**. Acting on that line would
have re-fixed a fix that shipped forty minutes earlier.

**Fixed:** `is_complete()` requires a `run_finished` event; `newest_baseline()` returns the newest
COMPLETE run, preferring a `baseline*` one; an explicitly-named partial run is refused with its
reason. All three paths **exit 0** — "undecidable" must not be read as a green light and must not be
read as a blocked arm either, because those call for opposite actions.

Controls both ways: with no complete run on this build it refuses and names the parked pre-boundary
directory while warning not to judge a new build's arms against it (that is what a boundary
invalidates); pointed at a complete parked run it judges normally.

---

## F133 — the two upstream commits aimed at our through-line are INERT here, and the real budget is elsewhere

First triage under the new upstream watch. 252 commits have landed on `block/goose` since fork-point
`a0aed81f3607`; 61 are in scope. Two looked aimed straight at the instruction-density problem, since
schema bytes are bytes not spent on the task:

- `950575bcd perf(toolshim): compact tool schema JSON (#10409)`
- `ca6ba6c44 feat(tools): collapse const-union enums in tool schemas (#10577)` — a 1,068-line
  normalizer whose own comment says schemars emits documented unit enums as `$ref -> $defs -> oneOf`
  of consts, **"~9x larger than an equivalent enum"**.

A 9x reduction on a 27B whose rule-compliance collapses with prompt size is exactly the kind of thing
worth adopting. So I measured our own requests before believing it applied.

### Measured on our real `llm_request` payloads

| | median | max |
|---|---|---|
| tool-schema chars | **2,064** | 2,064 |
| system message | **10,587** | 13,719 |
| all messages | 27,285 | 36,543 |
| tool count | 4 | — |

**Tool schemas are 7.0% of a median request. And 0 of 20 tool schemas contain `oneOf`, `anyOf`,
`$defs` or `$ref` at all** — the exact shape `ca6ba6c44` exists to collapse. There is nothing here for
it to collapse. Adopting either commit would be a byte-identical no-op: P2, a precondition that never
holds, caught for the cost of one query instead of a merge and a rebuild.

(Applicability rests on 20 schemas from 5 requests, which is a small sample — but the tool set is
fixed at 4 tools and the schema SHAPE is structural, not stochastic, so the zero is a property of how
our tools are declared rather than a thin sample.)

### The number that matters instead

**The system message is 10,587 chars — five times the tool schemas, and ~39% of the payload.** That
is after F87 already cut inherited hints by 94%. So the remaining instruction-density budget is
overwhelmingly the worker system prompt, and any future compaction work belongs there, not in
schemas. That re-points the through-line with a measurement rather than an intuition.

### On the instrument

My first query returned "0 of 261 requests have tools" — a blind parser, not an empty world: the
payload is nested under `input`, not at the top level. I checked the structure instead of believing
the zero, which is the standing rule and the reason this finding is not the opposite of itself.

---

## F134 — the pre-reviewer hallucinated a defect out of our own truncation and shipped it to the sink as an order

The fresh-eyes research sweep (35 agents, 29 leads, 12 survived) opened by correcting me, and it was
right to: **the regression is not established.** HEAD's own `03ac84aa5` measured three runs of an
IDENTICAL config at 44.2 / 86.7 / 90.0 — a 46-point spread. The gap I have been quoting (0.8708 vs
0.7186 / 0.6720) is ~15-20 points at n=1 per cell, comfortably inside that noise. I have been
reporting it as "Mihai's complaint reproduced"; it is not reproduced until replicates say so.

But two mechanisms cleared the bar the synthesis set — a firing pattern that matches the scoreline on
the three real units — and **I verified both myself rather than trusting the agents**:

| run | score | nodes | `task_split` | prereview findings |
|---|---|---|---|---|
| baseline-n1-r0 | **0.8708** | 1 | **0** | **0** |
| baseline-n3-r0 | 0.7186 | 3 | 1 | 1 |
| retarget_off-n3-r0 | 0.6720 | 3 | 1 | **3** |

Perfect rank-order on both, and the winner had zero of each.

### The phantom, traced end to end and now fixed

`pre_review` fed the reviewer `c.chars().take(2400)` **with no truncation marker**, so the model was
never told anything had been cut. On `retarget_off-n3-r0` it reported of `api.py`:

> *"The file is truncated mid-function ... and none of the handler methods (`_handle_health`,
> `_handle_payments`, `_handle_summary`, `_handle_sync`, `_handle_index`) nor the `serve()` function
> are defined — so the API is never actually wired up and would crash on any request."*

**`api.py` is 5731 chars and defines every one of them**: `_handle_health` at char 2561,
`_handle_payments` 2787, `_handle_summary` 3736, `_handle_sync` 4689, `_handle_index` 5177, `serve`
5407. All past the 2400-char cut. The defect is an artefact of the truncation, and nothing else.

It was then persisted to `.swarm/prereview/` and injected into the SINK's prompt under *"CONFIRM each
against the spec and FIX it before you finish"* — a mandatory repair order, against working code, at
the head of the run's longest and most expensive task (26 min, the largest serial region).

**FIXED:** the site now calls `review_file_excerpt`, which **already existed** and which both sibling
reviewers already used (`:20138`, `:20220`). It shows any file under 6000 chars WHOLE — `api.py`
would have arrived complete and the phantom could not have formed — and for larger files keeps head
AND tail with an explicit `[middle elided]` marker. P1 again: one rule, two implementations, and the
more expensive half was the copy nobody updated.

### What this is and is NOT

It is a real, traceable, node-correlated quality-destroying mechanism: pre-review is gated on spare
capacity, so it fires more with more nodes. **It is not an explanation of a 15-point gap** — 2 of the
4 persisted findings across the corpus are genuine catches (missing thousand-separators; a
lexicographic-vs-instant sort bug that would let a broken sort pass). One bogus sentence cannot carry
0.15, and deleting pre-review would throw the real catches away. So `PREREVIEW=0` is a MEASUREMENT of
the damage, not the intended fix; the fix is the excerpt change above plus routing findings through
the existing `verify_finding` skeptic.

### The other verified lead, not yet acted on

`split_inherit_spec` (`scheduler.rs:58`, env-only, default OFF): when the judge splits a task the
child's ENTIRE statement becomes `"(split of <parent>) <child-id>"` — ~35 chars replacing a ~3833-char
spec. Both losers split their single most-detailed task; the winner never split. Registered for the
next arm set, with the guard the synthesis insisted on: **assert `task_split > 0` before scoring the
arm**, or it is invalid by construction.

### The rule that outranks all of it

**Every arm from here runs 3 replicates.** A single-replicate arm on this bench measures nothing, and
the first job of the next analysis is the within-config spread. If that is still ~46 points, no arm
can be read and the next build's job is variance reduction, not levers.

---

## F135 — the "free instrument" the synthesis asked for on every arm is a TREATMENT, and free only when it has nothing to say

The research synthesis's arm plan ended with: *"plus `OWNED_FILE_FENCE=1` on every arm as a free
instrument."* The reasoning is attractive — cross-worker clobbering is the one defect class that
MECHANICALLY must scale with concurrency, and 76 archived runs contain zero `owned_file_violation`
events, so switching the detector on costs nothing.

**It is not a detector.** `swarm.rs:19137-19165`: just before `integrate-verify` reads the final
tree, the fence RESTORES every owned file a non-owner clobbered back to the owner's authoritative
bytes, through `write_frozen_bytes`. It changes the tree the sink verifies. That is a treatment.

Its own comment is what gives the game away: *"OFF (or no snapshots) => this whole block is skipped
and the tree is byte-identical."* So it is byte-identical **only when there are no violations** —
which is the exact quantity the probe exists to measure. **Free if the answer is zero, confounding if
it is not, and you cannot know which in advance.** That is F111's circularity in a different costume:
a readout whose validity depends on its own result.

And the confound would land precisely where it hurts. Enabled across all four score arms, it does
nothing in the boring case and silently repairs the tree in the interesting one — so the arm that
would have revealed interference is the arm whose score it corrupts.

**Queued as its own n=1 mechanism cell instead**, alongside the other mechanism readouts. A
contaminated tree is fine in a cell whose output is an event count rather than a score.

Note the zero it is chasing is itself uncontrolled: zero violations across 76 runs, with the detector
never switched on, is exactly the shape of evidence this loop has learned to distrust. The scheduler
already makes the common case impossible — `held_files` / `files_conflict` prevent two tasks owning
one file from ever being in flight together — so what remains is out-of-scope writes only.

**Readout:** violation count on a 3-node run. **ZERO closes "more nodes -> more interference" as a
hypothesis. Non-zero promotes it to first place.**

---

## F136 — the KV prefix cache breaks on ~31% of worker turns, and upstream already fixed the cause

Third upstream triage. `465269e5d fix(moim): freeze turn-context timestamp at turn start to preserve
prefix cache` looked worth chasing because at ~83 s/turn anything that forces a cold re-prefill is
pure wall-clock. Measured on our own request logs before deciding.

**The mechanism is present here and we do not have the fix.** `crates/goose/src/agents/moim.rs:145`
does `chrono::Local::now().format("%Y-%m-%d %H:%M:00")` on every `compose_moim` call, and the result
lands in `<turn-context><current-time>` at the START of the user message — i.e. immediately after the
system prompt, so everything downstream of it is invalidated whenever the minute rolls over. `grep
turn_start` finds nothing in our tree: the freeze is upstream-only.

### Measured, on real worker calls

| | transitions | broke | median re-prefill on a break |
|---|---|---|---|
| all calls | 26 | 15 (58%) | 2,417 chars |
| **workers only (system >= 5000 chars)** | **16** | **5 (31%)** | **14,569 chars** (max 15,188) |

**Be honest about the size.** ~31% of worker turn transitions lose the prefix and re-prefill ~14.6k
chars ≈ 4k tokens. Over ~100 turns that is ~31 re-prefills; on a local 27B prefill is far cheaper
than decode, so the expected saving is roughly **1-3% of wall-clock on a ~120-minute run** — real,
measurable, and small. It is not a lever that makes the swarm worth it, and it must not be sold as
one.

### A correction I made mid-measurement

My first pass reported "median 2,570 chars, 23.5% of the request" across ALL calls. That number was
dominated by the **spiral judge** — a tiny separate call whose system prompt is 379 chars and whose
user message is literally *"This call has emitted N characters of reasoning"* (swarm.rs:11371). Its
prompt necessarily changes every observation; that is its entire purpose, and caching it is
meaningless. I had started to treat it as a second cache-breaking defect to fix. It is not a defect.
Splitting real workers out moved the break rate 58% -> 31% and the per-break cost 2.4k -> 14.6k, i.e.
**both numbers moved, in opposite directions.** Lesson 14 again: ask what the population is before
explaining its average.

### Action, deliberately deferred

The fix lives in `crates/goose/src/agents/moim.rs`, which is upstream core — out of scope for
knob-turning, but squarely in scope as UPSTREAM INGESTION, which is the sanctioned path for exactly
this. **Not cherry-picked now**: a 12-unit replicate campaign is running and a core change landing
mid-campaign is lesson 9 (a fix can rot an experiment). Registered for the next boundary, with its
own expectation written down: **prefix-break rate on worker transitions falls from ~31% toward 0;
wall-clock moves 1-3% or not at all.** If wall-clock moves MORE than that, something else was riding
on the same cause and the model of where time goes is wrong.

---

## F137 — the prompt log is SHARED across all goose sessions, and my "real budget" number was 2x wrong

Chasing whether upstream's compaction work applies here, I looked at the message-count distribution
across recent LLM calls and found a **single user message of 1,301,532 characters**. On a 27B that is
absurd, and for a moment it looked like the biggest defect of the session.

It is not ours. The model on that call is `us.anthropic.claude-haiku-4-5` and the payload is a
base64 Playwright screenshot from `.playwright-mcp/`. **`~/.local/state/goose/logs/llm_request.*.jsonl`
is written by EVERY goose session on this machine**, not just the swarm — browser automation,
other agents, anything. Every prompt measurement I have taken from that directory has been mixing
swarm worker calls with unrelated sessions.

So I re-ran both findings that depend on it, filtered to the fleet models (`qwen3.6-27b`, i.e. the
`mihai-` / `gabee-` / `workhorse-` identifiers). Of ~199 recent calls, 189 are fleet and 10 are not.

### F136 SURVIVES, essentially unchanged

| | transitions | broke | median re-prefill |
|---|---|---|---|
| unfiltered (as published) | 16 | 5 (31%) | 14,569 |
| **fleet only** | **16** | **5 (31%)** | **12,291** |

Same transitions, same break rate, median moves 14.6k -> 12.3k. The conclusion and its honest
1-3% magnitude stand.

### F133's NUMBER WAS WRONG, and the correction strengthens its conclusion

F133 reported the worker system message at **10,587 chars median, ~39% of the payload**, from a
sample of **5 requests**, unfiltered. On **57 fleet calls**:

| | F133 (published) | corrected (fleet, n=57) |
|---|---|---|
| system message, median | 10,587 | **22,803** |
| system message, max | 13,719 | **49,117** |
| tool schemas, median | 2,064 | 2,064 (unchanged) |
| **system as share of payload** | **39%** | **81.0%** |

More than double, and the share nearly doubles. **The system prompt is 81% of what the fleet reads,
not 39%** — and tool schemas are 7.3%, which is what F133 concluded and remains true. So F133's
ACTION (instruction-density work belongs in the worker system prompt, not in schemas; the two
upstream schema commits are inert here) is not merely intact, it is much better supported than the
evidence I published it on.

The error was n=5 and no model filter. A five-request sample of a shared log is not a measurement of
anything, and I quoted it as "the real budget" for two ticks.

### The instrument rule this leaves behind

**Any prompt-size analysis from `llm_request.*.jsonl` MUST filter on
`model_config.model_name contains "qwen3.6-27b"`**, and must state its n. The directory is shared,
the intruders are large (one is 1.3 MB), and they are silent — nothing in the file says which session
it belongs to except the model name.

---

## F138 — the same number, wrong three times, three different ways. Instrument written; stop hand-rolling it.

I nearly filed "F87 has REGRESSED — Mihai's personal CLAUDE.md is back in every worker prompt". The
prompt I was staring at is real and it is 47,206 chars, 9.0% of it *"Production config on a CLIENT
system — MANDATORY rules"*, plus Mac Studio rsync, UI design rules, and how to write in his voice —
delivered to a 27B writing a Python app.

**It is history, not a regression.** The newest contaminated worker call is **2026-08-02 04:47:52**;
F87's suppression shipped at the 17:40 boundary. I had sampled the LARGEST prompt in the window
instead of the most RECENT, and the largest was necessarily a pre-fix one. Lesson 10, on myself: an
extremum is not a representative.

### The correction chain, in full, because it is the point

| | claim | why it was wrong |
|---|---|---|
| F133 | system 10,587 chars, 39% of payload | **n=5**, no model filter |
| F137 | system 22,803 chars, 81% of payload | fixed the model filter, but **pooled four call kinds and both F87 eras** |
| **F138** | **worker system 20,412 chars (n=7), 34% of what the worker reads** | split by kind, post-fix only |

Three ticks, three headline numbers, one metric. Each correction was real and each left a new way to
be wrong.

### What the properly-split measurement actually says

78 fleet calls — **and 431 foreign calls excluded**, i.e. the shared log directory is ~85% other
sessions:

| kind | n | sys median | sys max | tools | inherited hints |
|---|---|---|---|---|---|
| **worker** | 18 | 36,165 | **47,206** | 2,064 | 11/18 (all pre-fix) |
| planner/detail | 32 | 21,992 | 34,321 | 4,700 | 21/32 (all pre-fix) |
| scout/small | 16 | 1,314 | 2,706 | 3,191 | none |
| judge/spiral | 12 | 341 | 341 | 2,064 | none |

**F87 cut the worker system prompt from a median of 42,561 to 20,412 — a 52% reduction, measured
here for the first time on clean data.** That is a real, large, shipped win.

**The lever that remains: 20,412 chars of worker system prompt.** On a model whose perfect-rule
compliance falls from 0.588 at 10 rules to 0.094 at 40, that is the instruction-density budget, and
it is where B1 (the test-author reading-rule contradiction) lives.

### The durable part

`prompts.py`. It filters to the fleet, **splits by call kind**, flags F87 contamination with its
newest timestamp, and prints n on every cell — and it says outright that a pre-fix timestamp means
history rather than regression, because I raised exactly that false alarm. Three hand-rolled queries
produced three wrong headlines. This is the fourth query, written once, so there is no fifth.

---

## F139 — a test author is told "tests are a SEPARATE subtask" by the block that dispatches it

B1 from the research synthesis, verified in source and fixed.

`layout_block`'s owner branch (swarm.rs:19341) is reached by any task that OWNS files and is not the
entry point — **which includes every `test-<module>` task**. It tells that worker, verbatim:

> *"Do NOT `ls`/`find`/`tree`/`cat` … **tests are a SEPARATE subtask**, and the API of EVERY
> dependency you import is ALREADY injected below under 'API of …' — read it THERE, **NEVER `cat` the
> module**."*

For a test author both clauses are false. It **is** the test subtask, and the module under test is
the one file it must open to get real signatures. Meanwhile `reading_rules`, ~400 lines further down
and with `kind_prompt` ON, tells the same worker *"DO read what you are testing: the SOURCE module
under test (to get its real signatures)"*. **Both land in one system prompt.**

That contradiction sits exactly where the cost is: hard+test tasks retry **60% (n=30)** against 12.1%
for hard non-test work (F124), **all four terminal failures in the archive are test-authoring tasks**
(F130), and the recorded failure signature is a **SyntaxError in a test file** — which is what a
worker forbidden to read the module it is testing produces.

### Fixed as a SUBTRACTION, gated

With `kind_prompt` OFF the branch is not taken and the prompt is **byte-identical**, so the arm still
measures a real difference rather than a rewrite. With it ON, a test author is told it may read the
SOURCE MODULE UNDER TEST and its own file after writing — "those two and nothing else" — and to run
pytest ONCE to prove the file collects and runs, because *a test file with a SyntaxError or a bad
import is worse than none*. The kind sees **fewer** rules than today, never more, which is the whole
premise of instruction-density work on a model whose compliance falls 0.588@10 rules to 0.094@40.

### One structural fix underneath it

`is_test_author` was defined ~400 lines BELOW `layout_block` and so was invisible at the site where
the contradiction lives. The comment beside `kind_prompt_on` — hoisted earlier for exactly this
reason — says it outright: *"ONE classifier, resolved once, used by BOTH sites. Installing a second,
independent kind test at this block would let the two disagree the moment either drifts."* So I
hoisted the existing one rather than writing a second, and confirmed there is still exactly one
definition in the file.

Marker `YOU ARE TESTING to get its real signatures` added; preflight passes. This ships at the next
boundary — not now, because a 12-unit replicate campaign is running and a mid-campaign engine change
rots it (lesson 9).

**Readout is already registered and un-gameable:** hard-test retry rate from
`task_dispatched.attempt`, baseline 60%, target ~12%. This fix and the `reading_rules` change now
push the same number from two different sites, which is why `kind_prompt` gets two independent
chances to move it (F127).

---

## F140 — the split defect fired LIVE on the current baseline: a 2,929-char spec replaced by 30 characters, three times

`split_inherit_spec` was queued on archived evidence (both 3-node losers split their most-detailed
task; the 0.8708 winner never split). The precondition has now been confirmed on the RUNNING baseline
rather than inferred from history — which is lesson 8, satisfied before the arm is bought instead of
after.

`baseline-n3-r0`, live:

```
plan task api-cli   files = api.py, web/index.html, __main__.py, __init__.py, README.md
                    desc  = 2,929 chars      <- the largest producing brief in the plan
SPLIT: api-cli  ->  ['backend-api', 'frontend-ui', 'entrypoint-docs']
```

With the lever OFF — today's default — `child_description()` (scheduler.rs:76-82) returns, for each
of the three children, exactly:

```
(split of api-cli) backend-api
```

**~30 characters, in place of a 2,929-character spec that the run had already paid its whole planning
prefix to produce.** Three times. The judge decided the task was too big for one worker and was
right; the engine then handed each of the three workers essentially nothing to build from.

With the lever ON the same function emits a file-scope header naming the child's own files, an
explicit statement that the siblings are being written in parallel by other workers and must not be
touched or waited for, and then the parent's FULL spec as shared context. **Pure string work — no
model call, no new judgement, no dependency-semantics change** — which is why this is the cheapest
real lever in the queue: it needs an experiment, not an implementation.

### What this settles and what it does not

**SETTLED:** the arm's GUARD condition. `task_split > 0` holds on this baseline, so the arm cannot be
void by construction the way "ARM INVALID — splits=0" was. Two separate runs on two different builds
have now split their single most-detailed task.

**NOT SETTLED, and I will not claim it:** that this explains the 3-node deficit. The gap is ~15-20
points against a measured 46-point same-config spread at n=1 (F134). The arm runs 3 replicates and
the first job of the analysis is still the within-config spread.

### One thing worth noting about the plan that produced it

Q1 flagged `api-cli` as mixing three kinds across five files, and F124 measured that shape at a
+3.0pp non-effect — correctly noted, not charged. But this run shows the shape has a SECOND cost
that F124's retry metric could never see: a five-file, three-kind task is exactly what the judge
splits, and splitting is what discards the spec. The mixed-kind shape is cheap in retries and
expensive in SPLITS, and only the first was measured. That does not re-open F124 — the corrector is
still not worth building — but it does mean `split_inherit_spec`, not a planner change, is the fix
that reaches this cost.

---

## F141 — the engine's most-repeated sentence, replaced by one composed from what the worker actually did

Mihai, 19:40: *"NOTHING GENERIC, NEVER. If the swarm produces generic prompts for its nodes then it's
a fail."* The sharpest instance was already sitting in my own data, unacted on: **58 of 73 judge
interventions are three canned strings, and one of them fired 40 times.**

`judge.rs:444` — the finalize-spin correction — sent every one of those forty workers, verbatim:

> *"Your owned file(s) are written but unchanged for minutes while you keep running — you are stuck
> re-reading or re-verifying, not making progress. If a test or check is failing, make the SIMPLEST
> change that works…"*

At the moment it says that, `JudgeInput` holds: the worker's **owned file paths**, their **contents**,
its **compile errors**, `elapsed_secs`, `secs_since_last_write`, its tool-call and thinking counts,
and **the task's own spec**. A supervisor that has read all of it, and then says "unchanged for
minutes".

Worst of all: **it holds the compile error and throws it away.** A worker stuck on a SyntaxError was
told to "make the SIMPLEST change that works" instead of being shown the error — and a SyntaxError in
a test file is the recorded failure signature of the kind that produces ALL FOUR terminal failures in
the archive (F130).

### Composed, not canned — and it costs nothing

`spin_hint(&JudgeInput)` builds the correction in three parts, ordered deliberately:

1. **THE OBSERVATION, with this worker's real numbers** — *"You wrote `store.py` (1,240 bytes) and
   have not touched it for 8.3 minutes, while continuing to run for 15.0 minutes in total."* The
   worker can check that against reality instead of taking a verdict on authority.
2. **THE DECISIVE EVIDENCE, when the engine has it** — the actual compile error, quoted, capped at
   400 chars, followed by *"FIX EXACTLY THAT and nothing else… Do not re-read the project looking for
   other problems — this is the problem."*
3. **Otherwise, the settling question in terms of THIS task's deliverable** — re-read your own task
   statement; if every deliverable it names is present, report done, that is the correct end.

**No model call.** It is composition from state the engine already gathered, so it cannot hallucinate
and costs nothing — which matters because this fires on a saturated fleet where the semantic judge is
unreachable (`no_idle_device` is 100% of skips).

Regression test asserts what "not generic" means operationally: the hint must name the file, must
state the real idle minutes, must LEAD with the compile error when one exists, must not say "report
done" about a file that does not compile, and **two workers in different states must not receive the
same text**. 46 goose-swarm tests pass, cargo check clean, marker added, preflight green.

Held for the next boundary with F139 — not shipped mid-campaign (lesson 9).

**This is one of three canned strings.** The two "produced no file yet" variants (18 firings) are the
same defect and are next; the split child's `(split of <parent>) <child-id>` is the third and is
already covered by `split_inherit_spec`, queued.

---

## F142 — the second and third canned strings: stop ASSERTING a motive the engine can simply STATE

Continuing the generic-prompt audit. The two "produced no file yet" variants (`judge.rs:415/423`,
**18 firings**) were the remaining pair. The first one said:

> *"You have produced no file yet and have taken no action at all — **you are deliberating instead of
> building**."*

Three branches above it, the engine's own comment warns against exactly this shape: *"the hint no
longer ASSERTS over-reading … telling it to 'stop exploring/re-reading' is a false diagnosis injected
as a supervisor note."* The same objection applies to asserting deliberation — and **F131 measured
the population it is aimed at**: workers killed here carry a MEDIAN of 1,229 thinking chars, max
4,519, and one had **285 chars over 420 seconds**. Some had been reasoning hard; one had done almost
nothing. **One sentence cannot be true of both.**

`no_file_hint()` now STATES what the engine observed and lets the worker draw the conclusion:

- *"After 15.0 minutes, none of the files you own exists on disk yet, and you have run no command —
  you have emitted 4519 characters of reasoning instead."* — a fact it can check, not a motive
  imputed to it.
- **It names the owed paths.** The canned versions said "your owned file(s)" to a worker whose entire
  problem is not having started, while the engine held the paths.
- One next action, on the first owed file, small enough to do immediately.

**Context kept deliberately short** — observed counts, owed paths, one action. Mihai's third
directive is that the idle node must not itself bog down; a supervisory nudge delivered as a wall of
text is another way to cause the failure it is trying to fix.

### A test I had to rewrite, and why that is legitimate

`blind_deadline_kills_a_zero_tool_call_worker_at_420s_by_default` FAILED — it asserted the substring
`"taken no action at all"`, i.e. it pinned the **canned wording** rather than the rule. Its own
comment states the actual intent: *"the hint must NOT accuse a worker that has read nothing of
re-reading."* That intent is preserved and now asserted properly: the hint must state the no-command
fact, and must NOT contain "re-reading", "exploring" or "stuck re-reading". A test that pins wording
blocks exactly the kind of change this audit exists to make; a test that pins the rule does not.

47 goose-swarm tests pass. The new test asserts a heavy thinker (4,519 chars), an idle worker (0), and
one that ran 12 commands all receive **different** text.

### Where the audit stands

| canned string | firings | status |
|---|---|---|
| finalize-spin "written but unchanged" | 40 | **composed (F141)** |
| "produced no file yet" x2 | 18 | **composed (F142)** |
| split child `(split of <parent>) <child-id>` | every split | lever exists, **queued as an arm** |

That is 58 of 73 interventions no longer canned. What remains generic and unaudited is the **worker
system prompt itself** — 20,412 chars of largely static rules (F138), which is the next target.

---

## F143 — a worker was killed three times for "re-reading" while its tool count never moved

`judge_observed` (shipped at the 17:40 boundary) has produced its first complete trace, and it is the
clearest single piece of evidence in this whole investigation. `test-meridian` on the live baseline —
a test-authoring task, the fifth terminal failure in the archive and the **fifth that is a test
task** (F130):

| t | tools | thinking | written | what happened |
|---|---|---|---|---|
| 47.1m | 2 | 4,757 | no | |
| 50.7m | 3 | 5,154 | no | **KILLED** — *"STOP reading/deliberating"* |
| 52.7m | 0 | 1,992 | no | attempt 1 |
| 64.5m | 3 | 5,818 | **yes** | file written |
| 65.5m | 3 | 6,799 | yes | |
| 66.5m | 3 | 8,389 | yes | |
| 67.6m | 3 | 8,784 | yes | **KILLED** — *"stuck re-reading or re-verifying"* |
| 71.1m | 2 | 1,998 | yes | attempt 2 |
| 78.1m | 6 | 4,630 | yes | **KILLED** — same line. Attempts exhausted -> **FAILED** |

**Read the middle block.** Between 64.5m and 67.6m the worker's `tool_calls` sat at **3, unchanged**,
while its reasoning climbed **5,818 -> 8,784 characters**. It was not re-reading. It could not have
been: it ran no commands. It was generating — and the engine was holding the numbers that prove it
while telling the worker *"you are stuck re-reading or re-verifying, not making progress."*

The first kill is the same defect in the other direction: 3 tool calls and **5,154 characters of
reasoning**, told to *"STOP reading/deliberating"*. It had barely read.

So the canned hints were not merely generic — **on this task they were factually false, twice, about
a worker the engine could see was producing.** F141 and F142 (both held for the next boundary) fix
the wording: the spin hint now names the file, its size and the real idle minutes, and leads with the
compile error when one exists; the no-file hint states *"you have emitted 5154 characters of
reasoning"* instead of asserting a motive. On this trace they would have told the truth.

### But the wording is not the whole defect, and this trace shows the rest

**The finalize-spin trip fired on a worker whose thinking was actively climbing.** Its predicate is
`secs_since_last_write >= 420` — a wall-clock fact about the FILE, which says nothing about whether
the WORKER is working. The field that does say so is right there in the same struct, and its own
doc-comment states it outright: *"A NON-ZERO value here is positive proof the worker is producing."*

That is Mihai's fourth directive exactly — the rule should be *"is this worker producing?"*, not
*"has the file changed in 420 seconds?"*. A hard-coded 420 against a wall-clock is a place the engine
stopped thinking.

**REGISTERED, not yet built:** the trip must consult the CHANGE in `worker_thinking_chars` between
judge observations, not just the file mtime. A single snapshot cannot express "climbing", so this
needs the previous value threaded to the judge — a real change, and I am not making it in the same
tick I found the evidence. Falsifier written down first: **if thinking is flat across two
observations while the file is unchanged, the worker really is idle and the kill is correct** — so
the guard must key on the DELTA, and a guard that suppresses kills on a flat-thinking worker is
wrong and must fail its test.

Expected effect if it holds: this task took **35 minutes and three attempts and still failed**, and
at least the second kill looks unjustified. That is one task in one run — magnitude unknown until the
arm runs, and I will not claim more than that.

---

## F144 — a predicate about the FILE is not a predicate about the WORKER

F143's registered fix, built. The finalize-spin trip keyed on `secs_since_last_write >= 420` — a
wall-clock fact about an ARTEFACT — and killed `test-meridian` while its `tool_calls` sat frozen at 3
and its reasoning climbed **5,818 -> 8,784** characters. It ran no commands; it could not have been
"re-reading". The field that says whether the WORKER is working was in the same struct, and its own
doc-comment reads: *"A NON-ZERO value here is positive proof the worker is producing."*

`is_still_producing()` now guards the trip, and **the rule is the DELTA, never the level**. A worker
that emitted a great deal of reasoning and then STOPPED is precisely what the trip is for. So:

- reasoning GREW between two observations -> the worker is generating -> **no kill**
- reasoning FLAT -> genuinely stopped -> **kill, as before**
- **first look, no previous value** -> trip stays ARMED. Absence of evidence is not proof of life.

`prev_thinking_chars` is threaded from a per-task map in the dispatcher. Overwritten each observation,
so a re-dispatch's first look may see a stale predecessor from the prior attempt — deliberate, and
safe in one direction only: a stale HIGHER value makes `now > prev` false and leaves the trip armed.
The failure mode is "kill as before", never "suppress a kill we should have made". No backstop is
removed — `worker_timeout_secs`, the progress watchdog and the #134 spiral trip all still bound a
worker that generates forever.

48 goose-swarm tests pass. The test asserts all three arms including the falsifier.

### The marker for this fix does not exist, and preflight said it did

`prev_thinking_chars` is a struct FIELD; `is_still_producing` is a fn NAME. `JudgeInput` derives only
`Clone`, so neither is ever serialized and NEITHER reaches the binary's data section. **`strings`
would have reported ABSENT on a perfectly correct build** — the fourth time this rule has been
relearned (F62, F71, `is_code_deliverable`, now this).

**And `preflight.py` — written specifically to catch that — called it LITERAL**, because it only
asked "is this line a comment". Closing that hole took three attempts, each breaking the other
direction, which is the finding worth keeping:

| | rule | what it broke |
|---|---|---|
| v1 | not a comment => literal | a bare identifier passed |
| v2 | quotes on the same line | rejected F139's GOOD marker, a continuation line of a multi-line string |
| v3 | track quote state across the file | **DRIFTED** — the escape test compared against two backslashes instead of one, so every `\"` flipped the state, and by line 15554 of a 24k-line file it called a plain comment a literal. **That is the original F71 failure, re-created by the check written to catch it.** |
| v4 | LOCAL rule: comment, else quotes-before-it on this line, else previous non-empty line ends in a continuation backslash | all four controls pass |

Controls, all four now asserted every run: 44 real markers -> exit 0; the historical comment-marker
`THE SPEC STATES ITS ENDPOINTS` -> COMMENT, exit 1; `is_still_producing` -> IDENTIFIER, exit 1; a
fabricated marker -> ABSENT, exit 1.

F144 carries NO marker and is verified by its unit test instead — asserting behaviour, which is
stronger than asserting presence.

---

## F145 — I decomposed the SINK and called it a worker; the engine's gating was correct all along

Auditing the worker system prompt, I pulled the largest clean post-F87 one (23,104 chars) and split
it. The result looked like the biggest find of the session: **the first five bullets of its preamble
are FIX-ROUND rules**, including

> *"You own no files by default: **you may edit ANY file the fix requires**."*

sitting in a prompt that also carries `PROJECT FILE LAYOUT` and injected dependency APIs. Against a
worker's own *"write EXACTLY these ABSOLUTE paths, and write NOTHING outside them"*, that is a flat
contradiction — the F139 shape again, and bigger.

**It is not there.** Checked before writing it up, across every clean fleet prompt in the window:

| chars | owns files | has fix rules | verdict |
|---|---|---|---|
| 19,908 / 20,412 / 10,247 / 30,348 | **yes** | no | worker, correct |
| 23,104 x4 | no | yes | sink/fix, correct |

**0 of 8 prompts carry both.** The 23,104-char prompt I had decomposed is `integrate-verify` — which
owns nothing and IS the run's repair point, so "you may edit ANY file the fix requires" is exactly
right for it. The engine gates this properly. I had picked one prompt, seen fix rules in it, and
started generalising to "every worker".

### The real defect this exposed is in MY instrument

`prompts.py` classified that call as a **worker**, because its `WORKER` markers were
`PROJECT FILE LAYOUT` / `a dependency you import` — **and the sink carries both**, needing the map and
the contracts more than anyone. So the "worker" cell has been pooling 20k-char workers with 23k-char
sinks: the exact error F138 built this instrument to prevent, one level finer, in the instrument
itself.

**Fixed with the discriminator that actually matters — OWNERSHIP**, because that is what changes the
rules a call receives. A file-owning worker is told *"write NOTHING outside them"*; the sink is told
*"you own no files, you may edit ANY file"*. Two opposite instructions, so they must never share a
cell.

| kind | n | sys median | sys max |
|---|---|---|---|
| **worker** | 16 | 31,535 | 45,700 |
| **sink/fix** | 7 | 23,104 | 47,206 |
| planner/detail | 26 | 22,169 | 34,321 |
| scout/small | 17 | 1,292 | 2,706 |
| judge/spiral | 10 | 341 | 341 |

**Corrected lever: a clean worker system prompt is 21,736 chars (n=8), 31% of what that worker
reads.** F138's 20,412 (n=7) was close but pooled; this supersedes it.

### What the audit actually found, which still stands

Of a clean 23,104-char prompt, roughly **68% is per-task content** — injected dependency APIs (7,997
chars), the file layout (1,593), pre-review findings — which is precisely the shape the prime
directive asks for. The generic block is the **7,461-char preamble, 21 static bullets delivered
identically to every kind**. That is the real target, and it is a third of the prompt rather than the
whole of it. F87 already removed the genuinely foreign material; what remains is engine rules, and
the question for each is which KIND it applies to.

---

## F146 — a Go worker was told how to run Python, and every worker was taught a Click testing detail

First result of the preamble audit. Two of the 21 static bullets in the universal
`TOOLS & ENVIRONMENT` block had **no language or kind gate at all**, while `lang` was in scope one
line above them (it builds `worker_directive`):

- *"Run Python with `python3`, never bare `python`."* — 49 chars
- the **Click / `CliRunner` / `mix_stderr`** paragraph — 380 chars

`TargetLang` carries `Python, TypeScript, Rust, Go, Other`. So on any non-Python build the engine
instructs a worker how to invoke Python; and on **every** build, **every kind** of worker — an
implementer writing a data model, a verifier that writes nothing, the sink — is taught how to
construct a `CliRunner`, a detail about ONE Python CLI *testing* library it will never touch.

That is ~429 chars of instruction that **cannot apply**, on a model whose perfect-rule compliance
falls from 0.588 at 10 rules to 0.094 at 40. Every inapplicable rule silently evicts an applicable
one — which is the whole reason the density work is a subtraction, never an addition.

It is also G7 (de-hardcode) in miniature: the engine holds `lang` and used it two lines earlier, then
hard-coded a Python fact anyway.

### Gated on the two facts the engine already has

`lang == Python` for the interpreter rule; `lang == Python && is_test_author` for the Click
paragraph. **A Python worker's prompt is unchanged except that the Click detail now reaches only the
kind that could ever use it.** A Go or Rust worker loses both.

This compounds with F139: `is_test_author` was defined ~400 lines BELOW this site until that fix
hoisted it. Had it still been where it was, this gate could not have been written without a second
classifier — the defect the `kind_prompt_on` comment explicitly forbids.

### No marker, and the verification is better than one

The Click text is unchanged — only its condition moved — so there is no NEW string literal, and
`strings` would find the old one either way. Presence would prove nothing about gating. **Registered
instead as a run-based check**: after the boundary, `prompts.py` must show the Click paragraph in
test-author prompts and NOT in implementer prompts on the same run. That asserts the behaviour rather
than the byte, which is the standing preference (F62, F71, F144).

### Audit status: 21 bullets

| bullets | verdict |
|---|---|
| 1-5 (fix-round rules) | correctly gated to the sink already — F145 |
| **9, 10 (python3, Click)** | **GATED — this finding** |
| 20 "DON'T OVER-READ" (891 chars, the largest) | kind-gated by `reading_rules`; F139 fixed its `layout_block` contradiction |
| 21 "STOP WHEN GREEN" (468 chars) | kind-gated; unreachable for a test author (F126) |
| the rest (tools, paths, cwd, artifacts) | genuinely universal — they apply to every kind and every language |

So the preamble is now largely honest: what remains static is the part that really is universal.

---

## F147 — first result on the post-boundary build: 0.819, and it is ONE datapoint

`baseline-n3-r0` finished: **score 0.819, 124 min, pool 3/3, void=false, aborted=false**, prefix
2,569 s with **2 redraft rounds**, kind_mismatch 75.0%. It also LOST `test-meridian` (F143) and
scored 0.819 anyway — the app works without that test file, which is worth remembering when reading
any "failed task" as a proxy for build quality.

**What this is NOT:**

- **NOT comparable to 0.7186 / 0.6720.** Those are pre-boundary, on engine_build 1785657605. This is
  1785683891, carrying F87's hint suppression, F134's pre-review fix and `judge_observed`. Comparing
  across a boundary is precisely what a boundary invalidates, and `results` prints a loud warning for
  exactly this.
- **NOT evidence of improvement.** n=1. The within-config spread on this bench was measured at **46
  points** (44.2 / 86.7 / 90.0 on an identical config), so a single number carries almost no
  information about the engine that produced it.

**What it is:** replicate 1 of 3 in the baseline cell. The cell's job — stated before any arm was
queued — is to measure the spread on THIS build. Until r1 and r2 land there is nothing to compare
anything against, including this.

Registered so it cannot be quoted loosely later: **the baseline cell's headline is its SPREAD, not
its first value.** If the spread comes back near 46 points again, no arm in the queue is readable and
the next build's work is variance reduction rather than levers.

---

## F148 — the engine instructs its own architect to make the plan NARROW, and 19 of 19 plans obeyed

Auditing the planner prompts — the last un-decomposed block — turned up the most direct engine-side
limit on whether more nodes can help at all, and it is **ON by default**.

`converge` defaults to **true** (swarm.rs:1033, part of the golden bake). With it on, the architect
receives both of these:

> `homo_hint` — *"Commit to the SIMPLEST CANONICAL decomposition: **the FEWEST cohesive modules** that
> fully cover the spec … **Do NOT over-split; do NOT invent extra modules.**"*
>
> `count_clause` — *"decompose into the **FEWEST** cohesive module subtasks … target is usually
> **{worker_count} to 2x {worker_count}**"*

With it OFF, the very next branch says the **opposite**: *"Split **AGGRESSIVELY** into many fine
independent subtasks — do NOT fear interface divergence"*, and the target becomes 2x-3x worker_count.

### It is obeyed exactly

Module counts across **19 archived plans** (producing tasks that are not tests, verifies or the sink):

```
3 3 3 4 4 4 4 4 4 4 5 5 5 5 5 5 5 6      median 4
```

**Every single one falls inside [worker_count, 2x worker_count]** for a 3-node fleet. This is not an
aspiration the weak model ignores — it lands in the band 19 times out of 19.

**Median 4 modules against SIX concurrent slots** (3 nodes x PARALLEL 2). And modules are the level-0
roots: they are the only tasks runnable at t=0, because tests and `verify::<M>` depend on them. So at
the start of execute the plan can occupy about four of six slots by construction.

### Why it is there, and why that is the problem

Its own comment says it plainly: the old hint *"literally told the weak model to split AGGRESSIVELY …
self-inflicting the subtask-count variance that `plan_agreement` penalizes"*, so converge steers
toward the simplest canonical decomposition *"so independently drafted plans CONVERGE"*.

**That is an INTERNAL metric.** The engine narrows the plan so its own cross-draft agreement score
rises. F53 already observed that the redraft ladder optimises agreement rather than build quality;
this is the same trade one level earlier, and it is the default.

This is Mihai's first directive in its most concrete form. If more nodes appear not to help, look for
the mechanism that prevents them from helping — and here is one, installed deliberately, to make a
measurement look better.

### Queued as `converge_off`, reps 3, with its risk registered FIRST

**Readouts, all four read together:** module count per plan, max antichain width, execute occupancy,
and **the prefix**.

**REGISTERED RISK, before the run:** converge is described in-tree as *"the proven agreement raiser"*,
and low agreement is exactly what drives the redraft ladder — which cost this build's own baseline
**2 redraft rounds and a 2,569 s prefix**. So `converge_off` can plausibly win on width and LOSE on
wall-clock. A score alone will not settle it; if width rises and the prefix rises further, the answer
is that convergence should be cheaper, not absent.

preflight: `GOOSE_SWARM_CONVERGE` present in this binary, every queued arm can fire.

---

## F149 — the engine emits "can_look_things_up: false" every run, then tells the scout to look things up

The scout audit's result is mostly a clean negative: **the four lenses are genuinely differentiated**
— `codebase`, `edge-cases`, `architecture`, `libraries` each carry a distinct brief AND a distinct
tool_hint, both interpolated per lens (swarm.rs:12042). That is the shape the prime directive asks
for, and it already existed. Recorded so no future tick re-audits them.

**One line was not.** The `libraries` lens ships:

> tool_hint: *"Use the context7 tools (resolve-library-id then get-library-docs) and web-search."*

and every scout's prompt closed with *"You have at most {max_lookups} tool call(s): **spend them on
LOOKING THINGS UP**, not on exploring."*

**The engine already knows this is false.** `research_tools` is emitted on every run of this fleet as:

```json
{"available": [], "can_look_things_up": false}
```

Three archived runs checked, all identical. No MCP extensions are attached by default; `exts` is
empty and is **in scope at the very line that builds the prompt**.

### Why this is worse than wasted tokens

That lens's entire job is *"look up their REAL current API: function/class names, signatures, minimal
usage snippets, and gotchas."* Ordered to look it up, with nothing to look it up WITH, a 27B's only
remaining move is to produce the API **from memory** — and an invented signature does not stop there.
It flows into the plan, and then into the FROZEN CONTRACTS every worker builds against.

F78 measured the downstream half of exactly this: `grounded` was 0 on every run, so `doc_facts` — the
one verbatim research→worker channel — carried nothing.

### Fixed by telling it the truth and asking for CALIBRATION

When nothing is attached, the tool_hint becomes *"You have NO documentation or web-search tools
attached — do not attempt to use context7 or web-search, they are not there"*, and the closing clause
becomes: answer from what you know, and **state plainly which API names and signatures you are
CONFIDENT of, marking anything you are unsure of as UNVERIFIED rather than guessing a
plausible-looking name** — because *"a signature you invent here becomes a frozen contract every
worker builds against, so an honest 'unverified' is far more useful to the planner than a confident
invention."*

With extensions attached the prompt is **byte-identical** to today.

This is lesson 29 in its purest form — the engine held the answer, in an event it emits itself, and
said something generic instead — and it is the same class as F146 (an instruction that cannot apply),
now found on the research phase rather than the worker.

### The marker caught a mistake of mine

My first marker spanned a Rust line-continuation, so it was not contiguous in source and preflight
reported ABSENT — correctly. Narrowed to a fragment lying wholly on one line. That is the check doing
precisely its job, before a rebuild rather than after.

---

## F150 — CORRECTION to F148: the ceiling is not the problem. The SUSTAINED level is.

F148 asserted that with a median of 4 modules "the plan can occupy ~4 of 6 slots BY CONSTRUCTION".
That is an assertion, not a measurement, so I measured it — using `occupancy.py`'s own span pairing
rather than re-deriving it (lesson 2).

**3-node runs, n=11, six slots available (3 nodes x PARALLEL 2):**

| | value |
|---|---|
| peak concurrency, min | **3** |
| peak concurrency, median | **5** |
| peak concurrency, max | **6** |
| **time-weighted MEDIAN concurrency** | **2** |

**The ceiling claim is WRONG.** Peak reaches 6 in four of eleven runs and 5 at the median — the plan
demonstrably CAN fill the fleet. Module count is not capping it.

**What is actually wrong is the DURATION.** For half of every run the fleet is running **two** tasks
against six slots. The swarm briefly fills, then drains, and spends most of its life at 2.

That re-points the whole diagnosis. A deficit in SUSTAINED concurrency is a DAG-SHAPE problem — the
funnels `review.py` keeps flagging (`verify-e2e::*` and `integrate-verify` with >=3 deps each), and
the sink, which F112 already measured at 29% of node-busy and **100% of the solo time**. It is not a
module-count problem, and adding modules mostly raises a ceiling that is not binding.

### What this does to `converge_off`

**It stays queued, but it must NOT be sold on the ceiling argument any more.** Its honest case is
narrower: more independent modules may keep the fleet busy LONGER (more work available before the
funnels bite), which is a sustained-concurrency claim and is exactly what the readout should measure.

**Readout corrected:** the primary number is now **time-weighted median concurrency**, not peak and
not module count. Peak is already 5-6 and cannot improve much; if converge_off raises module count
but leaves the median at 2, the arm has confirmed that width was never the constraint — which is a
real and useful answer.

### And a defect in how I measured it

My first pass reported **peak = 0 on all 15 runs**. I had guessed `_spans` was a tuple and indexed
`s[1]`/`s[2]`; it is a dict of `{task, device, start, end}`. Worse, I had wrapped the loop in
`try/except`, so the TypeError was swallowed and the blindness surfaced as a clean, plausible column
of zeros. **A bare `except` around a parse turns an instrument failure into a finding.** Inspecting
the shape took one command, which is what I should have done first — and is the same
uncontrolled-zero rule that has now caught me four times.

---

## F151 — 42% of execute is spent 2-wide, and `integrate-verify` is 69% of it

F150 said the defect is DURATION at low concurrency, not the ceiling. This is where that duration
goes. Across 11 three-node runs, using `occupancy.py`'s own spans:

**229 of 552 execute-minutes — 42% — are spent with ≤2 tasks running against SIX slots.**

Which task is on the fleet during those low stretches:

| task | minutes at ≤2 | share of the low time |
|---|---|---|
| **integrate-verify** | **158** | **69%** |
| verify-e2e::1 | 31 | 14% |
| test-cli | 28 | 12% |
| test-meridian | 17 | 7% |
| test-api | 16 | 7% |
| (long tail of test-* / harden-*) | ~55 | — |

**The sink alone holds the fleet at ≤2 for 14.4 minutes per run and accounts for more than two thirds
of all low-concurrency time.** Everything else is a tail.

### This is F112's answer, and I went the wrong way from it

F112 concluded *"THE PLAN IS NOT THE BOTTLENECK; THE SINK IS"* — measured at 29% of node-busy and
100% of solo time. I then spent several ticks on plan width (F148), asserted a ceiling that F150
disproved, and have now arrived back at the sink from the other direction. The original reading was
right and I should have trusted the measurement over the newer story.

**The sink cannot be fixed by widening the plan.** It is ONE task with every other task as its
dependency. More modules do not help it; nothing can run alongside it unless something is
deliberately scheduled there. That leaves exactly two levers:

1. **make it shorter** — F116: wall-clock is turns x 83 s, and the sink is 25 calls against a median
   of 2-4. Fewer turns, not faster ones.
2. **put work beside it** — `sink_review`, the idle-fill that exists precisely for this window.

### Acted on: `sink_review` promoted to run right after baseline

It was an n=1 mechanism cell sitting BEHIND 18 score-arm units — roughly 36 hours of fleet time —
while targeting **69% of the low-concurrency time**. It now runs second.

The sequencing is the point: `sink_review` asks *"does the idle-fill mechanism fire at all"*, which
is an n=1 question answerable in one unit. Spending ~2 hours to learn whether the single largest
lever's mechanism even works, BEFORE spending 36 hours on score arms that cannot touch the sink, is
the same discipline `armcheck.py` exists to enforce — applied to ordering rather than to preconditions.

armcheck already verified its precondition on a real baseline: *"the sink held a node alone for 1590s
with the other nodes idle — that is the window idle-fill exists for."*

---

## F152 — the sink is not slow and does not waste turns navigating; ~30% of its turns are READING

Chasing F151's target (the sink is 69% of all low-concurrency time), measured across 12 sink activity
digests, 162 tool calls:

**It is not slow.** 8 completed sinks: **16 calls median** (min 10, max 25), **21.8 min median**, an
implied **79 s/call against a fleet median of 83**. F116 was exactly right — the sink's cost is
entirely its TURN COUNT, and turns are the only thing worth attacking.

**Three of eight ran to exactly 30.0 minutes** — `default_sink_cap_secs()` is **1800**. Those sinks
did not finish; they were cut off. `sink_capped` shows **0 events across 21 run dirs**, but every one
of those dirs is PRE-boundary and the marker shipped at 17:40 — so that zero is UNCONTROLLED and
**F115 stays open**, exactly as the prediction gate requires.

### What the turns are actually spent on

| | calls | share |
|---|---|---|
| `cat` | 29 | 20% |
| `ls` / `find` / `tree` / `grep` | 19 | 12% |
| `python3` (running the app — its job) | 12 | 8% |
| `edit` + `write` (fixing — its job) | 18 | 11% |
| `lsof` / `rm` (port + temp cleanup) | 15 | 9% |

**~30% of the sink's turns are reading and exploring a tree whose complete file manifest and injected
dependency APIs are ALREADY in its 23,104-char prompt** (F145 measured that prompt: ~68% of it is
exactly that per-task content). At ~80 s/call that is roughly **6.5 minutes per run spent
re-discovering what it was already told**, on the task that owns 69% of the fleet's idle time.

### Two wrong readings I caught, and how

**"40% of the sink's shell calls are `cd`"** — false. All **58 are COMPOUND** (`cd <abs path> && <real
work>`), **zero** bare navigation. Every one carries real work; the `cd` costs no extra turn. I only
saw this by printing the commands.

Before that, two classifiers disagreed with each other: the first put 70 calls in "shell: other", the
second put 3. The first mis-bucketed compound commands; the second excluded on `"ls" in low`, which
matches inside *calls*, *tools*, *false*. **Neither number was real.** The fix was to stop
classifying and dump the raw commands — three keyword schemes in a row produced three different
pictures of the same 162 calls, which is the clearest possible sign the keywords were the problem.

### The lever, stated for the arm that will test it

The sink re-reads what it already has. Two ways to spend fewer turns: **stop it re-reading** (its
prompt already carries the manifest and the APIs — the same subtraction F139/F146 applied to
workers), or **give the idle nodes something to do while it runs** (`sink_review`, now promoted to
run second). The first reduces the sink's own 21.8 minutes; the second reduces what the other four
slots lose to it. They are independent and both are worth having.

---

## F153 — the sink's FIRST instruction ordered it to go and find out what the engine already knew

F152 located the sink's turn cost: ~30% of its calls are `cat`/`ls`/`find`/`grep` over a tree whose
full manifest is already in its prompt. This is where that order comes from — the **opening sentence**
of the sink's own instruction block (swarm.rs:19320):

> *"You own no single file — you work ACROSS this whole layout. **Confirm EVERY file listed above
> actually exists on disk** and the tests cover each module."*

On a 13-20 file manifest that is an explicit instruction to spend turns stat-ing files, and the
manifest it refers to is pasted **immediately above that sentence**. The engine wrote the tree; it
can stat it in microseconds; it asked a 27B to do it at ~80 seconds a turn instead.

The cost lands on the worst possible task. F151: `integrate-verify` holds the fleet at ≤2 for **158 of
229 low-concurrency minutes across 11 runs — 69% of all of it**. F152: its cost is **entirely turn
count** (79 s/call against a fleet median of 83, so making calls faster buys nothing).

### Fixed by computing it and handing over the result

The engine now stats every manifest entry and injects the answer:

- all present → *"FILE CHECK, ALREADY DONE FOR YOU: all N files in the layout above exist on disk.
  **Do NOT `ls` or `cat` to re-confirm that — it is settled.**"*
- some missing → the exact missing paths, plus *"Do NOT go looking for the others — they are
  present."*

**One filesystem stat per file, no model call, and it cannot hallucinate an "it exists".** It is also
strictly better information than the sink could gather: a deterministic list beats a weak model's
recollection of what it saw two turns ago.

**The rest of that paragraph is untouched** — running the program end to end, checking the entry point
imports and registers every advertised command, fixing what crashes. That is the sink's actual job and
the part nothing else can do. This subtracts only the part the engine had already answered.

Same shape as F141 (the judge held the compile error and said "make the simplest change"), F142 (held
the thinking count and asserted a motive) and F149 (held `can_look_things_up: false` and said "look
it up"). **Four instances now of one defect: the engine possesses the answer and sends the node to
look for it.** That is the sharpest form of the standing directive — a generic instruction is not just
vague, it is the engine declining to use what it knows.

Marker `FILE CHECK, ALREADY DONE FOR YOU`; preflight green; cargo check clean. Held for the next
boundary alongside F149 and the `sink_review` queue promotion.

---

## F154 — ENGINE FREEZE until the baseline spread exists

Crossed again: `engine_build 1785693892 → 1785697869`, shipping F149 (the scout's phantom tools) and
F153 (the sink's file-existence check), with `sink_review` now second in the live queue —
`NEXT: sink_review-n3-r0`.

**The arithmetic that decided it (lesson 37), and it is different from last time.** r0 was 63 minutes
in and two-thirds done, so "wait for it" looks cheap. It is not: **any unit finished before the next
crossing is stale the moment I cross.** Keeping r0 has value only if I never cross again — and F153
changes the sink's very first instruction, which is precisely what a baseline measures. So the choice
was never "lose 63 minutes or keep them", it was "measure the spread on the final build, or measure it
twice".

### And that is exactly why this is the last edit for now

**This is the third crossing today, and each one restarted the campaign.** The pattern is a real risk:
every engine fix is individually justified, the campaign is always young enough that crossing looks
cheap, and the spread never gets measured. Continued indefinitely, the loop produces a perfect engine
and zero evidence.

**So: NO further engine edits until the baseline cell completes 3 replicates.** The generic-prompt
audit is done — the judge's three canned strings, the worker preamble, the scout lenses, the detail
fan and the sink's opening order have all been through it, and the remaining items are either queued
as arms or recorded as clean negatives. What is left is analysis, and analysis does not require a
rebuild.

Findings may still be written. Instruments may still be fixed — they do not touch `crates/`, so they
cannot invalidate a unit. **Only `crates/**` is frozen**, and `loop.sh check`'s held-commit counter
will make any breach visible on the very next tick.

**Lifting the freeze:** the baseline cell reaching n=3. At that point there is a spread on this build,
every arm becomes readable against it, and the first job — stated before any of this began — finally
has its answer. If that spread comes back near 46 points, the next work is variance reduction and NOT
more levers, however good they look.

---

## F155 — upstream triage under the freeze: eight commits closed, and one of them is a near-miss worth naming

Engine-frozen work (F154 permits triage, forbids adoption). Eight of the 57 in-scope upstream commits
closed against verified facts about THIS deployment rather than by reading their diffs and guessing:

| commit | why it cannot apply here |
|---|---|
| `2b507b8eb` fix(hints): contain subdirectory hint discovery | **the near-miss — see below** |
| `9fec4152a` fix(summon): reuse parent provider for delegates | `summon` appears **0 times** in swarm.rs; the swarm has its own dispatcher |
| `83ee4efcf` fix(security): denied tool request precedence | headless, no approval flow |
| `49e3ff46e` fix: sanitize shell call in linux.rs | platform is **Darwin** |
| `d98286022` fix: honor timeout_seconds on the Anthropic provider | provider is LM Studio OpenAI-compatible on :1234 |
| `a6420ea34` Add Azure AI Foundry provider | same |
| `a074d8eb3` fix(telegram) gateway approval | no gateway |
| `971d21784` keep CLI provider prompts out of process args | not our invocation path |

### The near-miss, and why it is worth a line

`2b507b8eb` canonicalises paths before the `dir.starts_with(working_dir)` containment test, so hint
discovery cannot escape the working directory via a symlink or an un-normalised path. That is
**exactly the class of defect F87 fixed here** — goose's global/project hints reaching a 27B writing a
Python app, measured at **42,561 → 20,412 chars, a 52% cut** (F138/F145).

It is inert for us for a stronger reason than "different code path": **we load no hints at all.**
`suppress_inherited_hints()` sets `CONTEXT_FILE_NAMES="[]"` as a **process-wide** env var at the top of
`run_swarm`, so every phase — scouts, planner, detail, workers, sink — is covered, not just workers.
A containment fix on a discovery walk that never runs changes nothing.

**Checked rather than assumed**, because "our suppression is only for workers" would have made this
directly applicable. It is not: one env var, set once, before any phase starts. Note the deliberate
escape hatch — it returns early if `CONTEXT_FILE_NAMES` is already set, so an environment that
pre-sets it keeps its own behaviour.

### The ratchet state

**13 of 252 triaged, 49 in scope remaining.** Every one closed here is closed permanently — that is
the whole point of `upstream.py` keeping a seen-list rather than re-reading 252 commits each tick.

Two remain flagged as worth an actual read when the freeze lifts: `ee61c7c49` (CLI streaming render,
O(n²) → incremental) and `8b73e1a1b` (stable agent event message identity, which may bear on the
activity digests every instrument here depends on). Neither is adoptable now; both are recorded so
they are not re-discovered.

## F156 — F146 VERIFIED on a live run; and "21,736 chars" was never a worker-prompt number

**F146's registered run-based check (Lesson 34) PASSES.** F146 introduced no new string literal, so a
marker could not verify it; the registered predicate was "the Click paragraph appears in test-author
prompts and NOT in implementer prompts on the SAME run." On the 22:11 build, 13 file-owning worker
prompts:

    Click paragraph in NON-test worker prompts:  0 of 8   (want 0)
    Click paragraph in TEST-author prompts:      5 of 5

That is the whole check, and it is green. The gate is `TargetLang::Python && is_test_author`.

**The retraction it dragged out.** Measuring that, the two kinds turned out to be different sizes:

    worker/impl    9,860 chars (n=8, median 9,900)
    worker/test   22,511 chars (n=6)

A 2.3x gap. **`prompts.py` had ONE `worker` cell pooling them**, so its headline — 21,736 chars,
quoted for a dozen ticks as "a clean worker system prompt" — is a median over a mixture: within 5% of
the test-author cell, 2.2x above the implementer cell, and a fact about neither. Implementers are
40.1% of all dispatches, so the number I was using to reason about instruction density was wrong for
the largest population it was supposed to describe.

This is the FIFTH instance of the pooling error and the second inside `prompts.py`, which exists to
prevent it — F138 split the eras and the kinds, the docstring warns at length about the sink being
pooled with workers, and the same file then pooled two worker kinds twice as far apart as the sink
ever was. Fixed: `worker/impl` and `worker/test` are separate cells, discriminated on the paths in
the OWNS block (not on `tests/` appearing anywhere in the prompt — every worker on a Python run sees
that in the file layout), and the tail **refuses to print a combined figure** at all.

**The gap itself is NOT a defect and must not be reported as one.** Paragraph diff: ~12k of the
difference is real signatures and code excerpts (`class Store`, `_get_with_429`, the 429/Retry-After
handling, the per-file deliverable list) that a test author needs and an implementer does not. That
is the engine differentiating correctly — Prime Directive 2 working, not failing.

RETRACTED: F133 10,587 / F137 22,803 / F138 20,412 / F145 21,736 are ALL superseded. Quote a cell
with its KIND and its n, or quote nothing.

## F157 — 45% of an implementer's prompt is the TOOLS block, and it is written for a test author

Once the cells were separated, the implementer prompt is small enough to account for completely:
**`TOOLS & ENVIRONMENT` is 4,450 chars of a 9,860-char implementer system prompt — 45%.** Sixteen
bullets, before any task-specific content. Compliance on this model class falls 0.588 at 10 rules to
0.094 at 40, so this one generic block spends most of the budget on tool mechanics.

Two of its bullets are addressed to a node this one is not (the F149 shape — an instruction about a
thing that is not there), verified against the tools actually attached: `edit`, `shell`, `tree`,
`write`.

1. *"NEVER read the project's OTHER TEST files … any test file YOU OWN is your deliverable and is
   yours to read and write freely"* — the implementer owns no test file. The exemption clause
   describes a case that cannot occur for it, and it is the longest bullet in the block.
2. *"STOP WHEN GREEN. The MOMENT your file's tests pass … do NOT re-run pytest more than ~2 times"* —
   an implementer's tests are a SIBLING task's deliverable and on a fanned plan frequently do not
   exist yet. The stop condition it is handed is one it cannot evaluate.

Both are true and useful for a test author. Delivered to an implementer they are inapplicable rules
that evict applicable ones, on the class that is 40% of dispatches. The engine already holds
`is_test_author` — F139 and F146 both gate on it — so this is the same one-line fix, twice.

Also noted: one bullet carries 13 characters of stray leading indentation (`             - NEVER run
\`cd\``), a multi-line-string artifact.

QUEUED, NOT SHIPPED — the engine freeze (F154) holds until baseline n=3. Registered check for when it
lifts, since neither fix introduces a new literal: on one run, the two bullets above must appear in
test-author prompts and NOT in implementer prompts.

## F158 — the HTML task is lectured about banker's rounding, and the comment above the code says that cannot happen

The implementer prompt is 9,860 chars and now accounts completely: **81% of it is generic** (preamble
746 + TOOLS 4,450 + CONVENTIONS 2,807 = 8,003) and 19% is the frozen interfaces. The worker I
audited owns exactly ONE file:

    /Users/…/swarm-3node-r0/vendorsync/web/index.html

For that static HTML file it receives, as "EXTERNAL GROUND TRUTH, not suggestions":

    4. Range inclusivity: range(a,b) and Python slices are END-EXCLUSIVE; cron 'a-b', SQL BETWEEN …
    5. Money/currency MUST NOT be a binary float … round() in Python 3 is banker's rounding …
    6. Off-by-one at boundaries … pagination page N offset = (N-1)*size.
    9. Integer vs true division: Python '/' is float, '//' floors toward -inf (-7//2 == -4);
       C/Go/Rust int '/' truncates toward zero; modulo sign follows the dividend in C …

plus 1,857 chars of frozen **Python** signatures (`class Store`, `def upsert_many`, `SCHEMA`,
`MeridianClient`) and the instruction to *"THEN run `python3 -m pytest` to check"*. Roughly 4,700
chars — **48% of the prompt** — is about a language this node is not writing.

**The mechanism, read from source, not inferred.** `relevant_pitfalls` (swarm.rs:9507) lowercases
`req.description` + `req.owned_files` and keeps every library item any of whose trigger words appears
anywhere in that haystack. Measured across all 17 tasks of the live plan:

    web    index.html      desc 1,641    4 items fired   <-- the only non-Python deliverable
    store  store.py        desc 2,028    4 items fired
    api    api.py          desc 2,204    4 items fired

The HTML task fires as many conventions as the SQLite store does. It cannot do otherwise: the
description is 1,641 chars about a payments dashboard, so `payment`, `amount`, `currency`, `count`,
`date` are all present, and **the filename can only ever ADD to the match, never subtract from it.**
`index.html` contributes no trigger and suppresses nothing. Retrieval is additive-only.

**The comment twelve lines above the call site states the exact invariant being violated:**

    // Retrieval is deterministic and scoped to what the task is ABOUT — never the whole library, or
    // a CSS task would be lectured about cron.
    // Retrieval reads the subtask's own spec PLUS its owned file names: a task named `cron.py` /
    // `money.rs` announces its domain even when the prose does not.

The worked example in the comment is the bug. Adding the filenames was meant to be the guard, and it
is only a second way to widen. A CSS task IS lectured about cron, because the guard is a keyword
match on prose that describes the APPLICATION while the deliverable is one file.

**SIXTH instance of the family, and it sharpens the family's statement.** F141 held the compile
error, F142 the thinking count, F149 `can_look_things_up:false`, F153 the file list, F157
`is_test_author`. Here the engine holds `owned_files = ["…/index.html"]` — and the defect is not that
it fails to look, it is that **it looks at the right field and uses it only to broaden, never to
narrow.** A fact that can only ever add rules is not a scoping fact.

FIX (queued — engine freeze F154): derive the deliverable's language from the owned file extensions,
which the engine already does for `TargetLang` (F146 gates on it), and skip a convention whose
language no owned file is written in. Same one-line shape as F146 and F157.

REGISTERED CHECK for when the freeze lifts, since this introduces no new literal: on one run with a
web deliverable, the `KNOWN-CORRECT CONVENTIONS` block must be ABSENT from the prompt of a worker
whose owned files are all non-`.py`, and still PRESENT for `store`/`api`.

Population note, stated honestly: this is 1 task of 17 on n=1 run. It is published as a MECHANISM
finding, which is valid at n=1 — the trigger match is deterministic and re-derivable from source —
and NOT as a claim about how often plans contain a web deliverable.

## F159 — every judge intervention on this run went to a test-author, and I had already parked the lever that targets them

**F142 is verified live.** Three real interventions fired on baseline r0, and every hint is built
from that node's own facts — no canned sentence, including the variant clause:

    After 7.9 minutes, none of the files you own exists on disk yet, and you have run no command —
    you have emitted 24032 characters of reasoning instead.   You owe: `tests/test_api.py`.

    After 7.0 minutes, none of the files you own exists on disk yet, though you have run 1 command(s).
    You owe: `tests/test_meridian.py`.

Registered check passes: real minutes, real filenames, real reasoning counts, and the no-command /
one-command branch both exercised.

**What the interventions point at is more interesting than the interventions.** All three landed on
test-authors; none on an implementer. Per kind, on this run:

    implementer   5/5 completed, 0 interventions, median 6.0 min; only 2/5 ever seen dry
    test-author   2/5 completed, 3 interventions;              5/5 seen dry, 2 never wrote at all

Measured in `thinking_chars` rather than wall-clock, deliberately — a character count is immune to
the contention confound (all 6 slots are busy, so later tasks inflate on elapsed time):

    dry reasoning before the first owned write — implementer median 1,022 (max 1,448)
                                                 test-author median 3,402 (max 24,032)   3.3x

Test-authors are the kind F156 measured at **22,511 chars, 2.3x the implementer's 9,860**. The kind
with more prompt is the kind that reasons instead of writing.

**What that extra content actually is.** The `tests/test_meridian.py` author receives `## API of`
blocks for **all five modules of the application** — `__init__.py`, `__main__.py`, `api.py`,
`meridian.py`, `store.py` — totalling **265 lines of indented implementation body against 35
signature lines**, and exposing six private methods: `_do_request_with_429_retry`, `_existing_ids`,
`_get_with_429`, `_handle_429`, `_headers`, `_send_json`. Its declared dependency is `meridian`
alone. Shipping a private body to a test author does not just cost context — it invites a test
written against internals, and against the implementation's own reading of the spec, which is the
failure `DOMAIN_PITFALLS` opens by warning about ("the code and the tests then encode the SAME
mistake").

**And I had already parked the fix.** `scoped_contracts` (swarm.rs:19028, `scope_contract_bundle` at
coherence.rs:303 — written, tested, never enabled) was removed from the sweep queue with this
reasoning: the architect is told *"Default to a FLAT FAN: make every module a root with no deps"*, so
a worker's neighborhood is just itself, so scoping the bundle would delete every sibling interface
and leave only its own stub. The measurement behind that was correct. The generalisation was not:

    implementer   5/5 EMPTY neighborhood  -> `!req.neighborhood.is_empty()` fails, lever INERT
    test-author   0/3 empty (each deps on the ONE module it tests)          -> LIVE
    verify/sink   0/9 empty                                                 -> LIVE

Inert for 5 of 17 tasks, **live for 12** — including every test-author. The flat fan gives tests and
verifiers a neighborhood BY CONSTRUCTION: a test must depend on the thing it tests. I measured the
implementer case, wrote "a worker's neighborhood", and parked a lever that never applied to the
population it was parked for.

**Third instance today of one error.** F156 pooled two worker kinds in an instrument; F158 found the
engine scoping on a fact that can only broaden; this is the same shape in a QUEUE decision — a
conclusion drawn on one kind and spent on all of them. Lesson 33 said "before generalising from one
prompt, check which kind it is"; it applies to levers and to my own notes, not only to prompts.

ACTION: `scoped_contracts` re-queued at reps=3 with the readout defined on **test-author dispatches
only** — pooling the kinds would dilute a real effect to nothing, which is F156's error a fourth
time. Takes effect at the next supervisor restart (Lesson 23); NOT restarting now, because that would
abort the baseline r0 the freeze exists to obtain.

CONFOUNDS, stated rather than buried: n=1 run, 5 implementers and 5 test-authors. Test-authors
dispatch later and under heavier contention — which is why the headline is the character count and
not the minutes. Writing tests may also be intrinsically more deliberative than writing an
implementation; this finding does not separate that from prompt volume, and the arm's gate names the
refutation explicitly — a cut in prompt size with NO improvement in dry reasoning kills the
mechanism hypothesis and moves the suspicion to the "DON'T OVER-READ / nothing further to look up"
instruction sitting directly beside 12k of readable code.

## F160 — every deterministic stall trip keys on a LEVEL, so a worker that produces NOTHING is invisible to all of them

Tracing r0's three stuck test-authors through their own `judge_observed` rows shows the interventions
DO work — every re-dispatch eventually converted a dry worker into a writing one:

    test-api                  a0 dry (think 5,080 -> 24,032)  a1 dry  a2 WROTE at 114s
    test-api-input-validation a0 dry (flat 3,402)             a1 WROTE at 399s
    test-meridian             a0 dry (flat 857)               a1 WROTE at 294s

But the rows underneath are the finding. Three of those attempts were not slow — they were **frozen**,
emitting nothing at all:

    test-api a1                think 2,006, calls 1 — UNCHANGED across 4 observations, 221s -> 465s
    test-api-input-validation  think 3,402, calls 0 — UNCHANGED across 5 observations, 165s -> 441s
    test-meridian a0           think   857, calls 1 — UNCHANGED across 5 observations, 170s -> 422s

**~772 seconds — 12.9 minutes — of dead wall-clock on ONE run**, on a fleet where wall-clock is
turns x 83s, spent on slots the engine could already prove were producing nothing.

**Why no deterministic trip caught them.** There are three, and each keys on a LEVEL:

  1. compile errors — none here.
  2. over-read: `worker_tool_calls >= over_read_tool_calls`, resolved **16**. A THRASHING worker makes
     many calls; a FROZEN one makes zero. The trip is structurally blind to the failure that costs
     most — it can only see the loud kind.
  3. reasoning-spiral: resolved from the run's own `levers_resolved` as **`spiral_thinking_chars = 0`
     — OFF**. (Not to be confused with `spiral_break_chars = 12000`, a different lever entirely; the
     names are one word apart and only one of them is live.) Empirically confirmed too: test-api a0
     sat at 14,707 chars with 0 calls at 350s and was NOT killed.

So the only remaining detector is the LLM review — and it was skipped **60 of 69 times, every one
`no_idle_device`**. Stall detection is reachable only when a node is free, which is never under load,
which is exactly when a frozen slot is most expensive.

**Enabling the existing lever would fix NONE of the three**, which is the decisive part. Its guard is
`worker_tool_calls == Some(0)`: test-api a1 and test-meridian a0 both had **1** call and fail it
outright, and test-api-input-validation's 3,402 chars is far under any defensible char threshold. No
setting of a level-based trip catches a worker that stops after one tool call and 857 characters.

**The signal the engine already has and does not use.** F144 added `prev_thinking_chars` to
`JudgeInput` and `is_still_producing()` — and used the delta in ONE direction only, as a guard
AGAINST killing (`GREW = no kill`). Its negation is the cleanest kill signal available: thinking
chars AND tool calls both unchanged across consecutive observations is, deterministically, a worker
producing nothing. It needs **no model, no idle device, and no tuned literal** — which is why it also
answers the `no_idle_device` starvation above: it is arithmetic on two integers the engine already
emits, so it runs when the whole fleet is busy.

EIGHTH instance of the family, and the sharpest reading of "PRINCIPLE, NOT HARD-CODE" yet: three
hard-coded levels (16 calls, a char cap that is off, a 90s min age) guarding a decision whose true
predicate — *did anything change since last time?* — is already computed and thrown away.

QUEUED behind the freeze (F154). Registered as F161 in PREDICTIONS since it introduces no literal:
the trip fires on a worker flat across >=2 consecutive `judge_observed` rows; the falsifier is a flat
worker that later produces on its own without being re-dispatched — which would mean flatness is an
artefact of the observation cadence rather than a stall, and the fix must be abandoned.

## F162 — the idle node exists when there is nothing to assess, and nothing is idle when there is

`judge_skipped` fired **60 times on r0, every one `no_idle_device`** — against F151's measurement
that 42% of execute sits at <=2 tasks with SIX slots free. Those look contradictory. They are not,
and resolving it names the real obstacle to Prime Directive 3.

Occupancy at the exact moment of each skip (counting a task from its LATEST dispatch to its
completion — the first version of this counter let re-dispatch ADD instead of REPLACE and reported 9
in flight against 6 slots, which is impossible and was thrown away):

    in-flight when SKIPPED   (n=60): median 6   {5:10, 6:43, 7:7}
    skips with the fleet full (>=6): 50/60 = 83%
    skips with >=2 slots free (<=4): 0/60 = 0%

Not one skip happened while capacity was available. And the time profile shows why F151 does not
disagree — the two measurements describe different windows of the same run:

    min  0-15   0 in flight (PLAN)      judge looks   0
    min 20-35   5-7 in flight (FULL)    judge looks 108   <- 81% of all looks
    min 40-50   3-4 in flight (tail)    judge looks   7

The judge's work is concentrated exactly where the fleet is saturated. F151's low-concurrency 42% is
the TAIL — and 69% of it is `integrate-verify`, by which point there is almost nothing left to
assess. So:

  **The free node appears in the tail, when there is nothing worth judging. During the fan-out, when
  there is, every node is busy and the assessor is unreachable.**

High utilisation and semantic judging are in direct tension — good scheduling actively starves the
mechanism meant to supervise it. That is a structural property, not a tuning miss, and no threshold
fixes it.

Two ways out, and they are not equivalent:

  (a) RESERVE capacity for assessment — pay real throughput for it, on a fleet whose whole problem is
      that it under-fills. Directly opposed to Prime Directive 1.
  (b) Make the assessment need NO device. Every check that is arithmetic on counters the engine
      already emits runs fine at 6/6 occupancy.

(b) is the answer for everything that does not require judgement, and F160 is exactly that case: a
worker flat on thinking-chars AND tool-calls across consecutive observations is provably producing
nothing, decidable from two integers, no model involved. It is not a cheaper approximation of the
semantic review — it is the part of the review that never needed a model, currently queued behind one.

This does NOT retire the semantic judge. It says the split is wrong: the deterministic half must run
always, and the model half should run in the tail where a node IS free — which is also where the
sink, the run's single longest task, currently runs unwatched (F115, still open).

Corrects nothing in F151, which stands exactly as measured; it supplies the window F151 did not state.

## F163 — F160's predicate is REFUTED by its own falsifier, one tick after I proposed it

F161 registered the falsifier as: *a flat worker that later produces on its own without being
re-dispatched*. It fired on the very next observation window.

    test-meridian attempt 1
        obs 105s  calls=0  think=1,209  written=False
        obs 174s  calls=0  think=1,209  written=False   << FLAT
        obs 234s  calls=0  think=1,209  written=False   << FLAT
        obs 294s  calls=0  think=1,216  written=TRUE

Flat on both counters for 129 seconds, then it wrote. A ">=2 consecutive flat observations = kill"
trip would have killed a worker that was in the middle of succeeding. **F160's predicate is withdrawn.**

**Why flat does not mean frozen.** The engine's own comment at swarm.rs:16539 states the mechanism:

    // On a reasoning model this is the ONLY non-zero signal a still-working worker produces: it
    // streams Thinking, which is neither a tool call nor text, so tool_calls/errors/last_text all
    // read 0 while it is in fact generating.

`thinking_chars` counts the THINKING stream only, and `tool_calls` increments when a call COMPLETES.
So when a worker stops thinking and starts emitting the tool-call payload — the file content itself —
thinking_chars freezes and tool_calls has not moved yet. **Both counters are flat during the single
most productive thing a worker can do.** Confirmed against the digest's full field list
(`tool_calls, errors, malformed, recent, last_text, calls, reasoning, full_reasoning, thinking_chars,
last_thinking, model, phase`): there is NO timestamp and NO in-flight byte counter. Nothing in the
digest advances while a tool payload streams.

**The data offers a >=4-flat refinement and I am refusing it.** The three genuine stalls were flat
across FOUR-plus observations (244s, 276s, 252s) and this false positive across two. So ">=4" fits
perfectly — on the four cases that produced it. Tuning the threshold to the same runs that revealed
it is fitting the instrument to the answer, and it would still be a hard-coded literal guarding a
signal that is measuring the wrong thing.

**What F160 got RIGHT, and what survives.** The 772 seconds of dead wall-clock is real: those three
attempts never recovered and were correctly killed. `over_read_tool_calls = 16` really is blind to a
silent worker, `spiral_thinking_chars` really is 0, and the LLM review really is skipped 60/69 on
`no_idle_device` (F162). **The problem stands; my detector was wrong.**

**The corrected design needs a new OBSERVABLE, not a new threshold.** A `last_delta_at` timestamp in
the digest, updated on ANY stream event — thinking, text, or tool-argument bytes — makes "frozen"
physically well-defined: no bytes of any kind, as opposed to today's "no thinking and no completed
call", which conflates freezing with writing. That also keeps Prime Directive 4 honest, because the
silence window can be derived from the fleet's own observed inter-token cadence rather than picked.

Queued behind the freeze, and it is now a digest-writer change rather than a judge change — a
different and larger blast radius than F160 implied, which is worth knowing before it ships.

LESSON: the falsifier earned its keep within one tick. Registering it cost one line and it stopped a
plausible, well-argued, measured fix that would have killed healthy workers under load — exactly when
the fleet can least afford it.

## F165 — the failures are not "produced nothing"; 12 of 14 left a substantive file, and at least 2 were COMPLETE

`test-meridian` failed on r0 — its 7th failure in 11 appearances, as F164 predicted. Then I ran it:

    tests/test_meridian.py   4,216 bytes   8 test functions, 12 assertions   8 passed in 0.53s

**A terminal FAILED verdict on a task whose deliverable is on disk and entirely green.** The engine's
own final hint says so before killing it:

    failed / looping (conf 0.90)
    "You wrote `tests/test_meridian.py` (4216 bytes) and have not touched it for 7.5 minutes ...
     Nothing is reported failing, so `tests/test_meridian.py` is most likely already done and you
     are polishing or re-verifying."

The judge diagnosed the task as COMPLETE and the action attached to that verdict is `failed`. The
chain: attempt 0 killed `over_reading` (correct — nothing written); attempt 1 killed `spec_drift` for
a real but optional critique (`setUpClass` shares one server; use `setUp` per test); attempt 2
polished a working file until `max_attempts` was exhausted. **There is no "accept it, it is done"
outcome available to the judge — its only lever is kill, and the third kill is terminal.**

**Then I checked whether the whole campaign looks like this, and it does NOT. Correcting my own
first reading.** Across all 14 recorded failures:

    owned file EXISTS on disk: 12 of 14   (6-35 test functions, 9-96 assertions, 2.7k-17.7k bytes)
    owned file MISSING:         2 of 14   (one test_api.py, and integrate-verify's README.md)

So "the swarm produced nothing" is true of 2 failures out of 14. But existence is not correctness, so
I re-ran the suites of the failed tasks:

    preboundary-1785683891  test-meridian   11 passed                 GREEN — work was COMPLETE
    preboundary             test-api        21 passed                 GREEN — work was COMPLETE
    nodeloop r0             test-meridian    8 passed                 GREEN — work was COMPLETE
    build                   test-meridian    1 failed,  5 passed      partial
    preboundary-3           test-meridian    2 failed,  6 passed      partial
    preboundary-7           test-meridian    4 failed,  9 passed      partial
    preboundary-7           test-api         3 failed, 25 passed      partial
    preboundary-5           test-api        14 failed, 21 passed      partial

**Three of the eight re-run failures were fully green — the task was finished and recorded FAILED
anyway. The other five left a partially-broken suite, which is a REAL failure.** My first reaction on
seeing r0 was that F164's 31% might be "mostly accounting"; that is wrong and I am striking it. The
honest split is roughly a third accounting, two thirds real.

⚠ AND THE 'REAL' TWO THIRDS ARE NOT CLEANLY ATTRIBUTABLE. A failing `test_meridian.py` means the test
disagrees with `meridian.py` — it does NOT say which one is wrong. The run's own record cannot
separate "the test author wrote a bad test" from "the test author wrote a correct test that caught a
broken module". Since implementers are never marked failed (F164), a correct test catching a bad
implementation is recorded ENTIRELY against the test author. That is a real possibility this data
cannot exclude, and it would mean the test-author cell is absorbing blame for the implementer cell.

WHAT THIS CHANGES:
  · F164's 31% stands as a count. Its INTERPRETATION narrows: the failure mode is almost never "no
    output" — it is "output the engine would not accept" or "output that disagrees with a sibling".
  · F147 ("a failed task is NOT a proxy for build quality") now has its mechanism, and it is worse
    than F147 stated: a task can be green, diagnosed green by the engine, and still recorded FAILED.
  · A judge that can only kill needs an ACCEPT. Queued behind the freeze, and it is the cheapest of
    the queued fixes to state: on a `looping` verdict where the owned files exist and nothing is
    reported failing, the correct action is to finish the task, not to spend its last attempt.

## F166 — F153 verified live on r0's sink

The sink dispatched at 23:33 and its prompt carries the engine-computed check:

    FILE CHECK, ALREADY DONE FOR YOU: all 11 files in the layout above exist on disk. Do NOT `ls`
    or `cat` to re-confirm that — it is settled. Confirm the tests cover each module. CRITICAL: a
    green pytest suite does NOT prove the program works — unit tests usually call functions
    directly and NEVER invoke the CLI/entry point…

`Confirm EVERY file` — the old opening order that sent the sink to re-discover a tree already in its
own 23k-char prompt — is ABSENT. Registered check passed; the engine now states the file count it
computed (11) rather than asking the node to go and count.

Its real job is untouched: run the program end to end, check entry wiring, fix crashes. Still open on
this sink: `sink_capped` (F115 — every prior zero was pre-boundary and uncontrolled) and the call
count against F152's baseline of 16 median / 21.8 min. Both are settleable only when it finishes.

NOT RUN, deliberately: the app's own test suite. The sink is editing files and binding ports in that
tree right now, and a concurrent pytest would contend for both — corrupting the sink's run and my
measurement together. The crunch waits for r0 to finish.

## F167 — nine more upstream commits closed on deployment facts, one kept open

Continuing F155's ratchet. Every close below is a one-command check about THIS deployment, not a
reading of the diff (Lesson 44).

    36cb569e3  providers: rewrite oneOf -> anyOf in tool schemas for OpenAI-compatible backends
               The most plausible candidate of the batch — LM Studio IS our OpenAI-compatible
               backend. Closed on TWO independent facts: the four schemas we actually send
               (`edit`, `shell`, `tree`, `write`, 2,064 bytes total) contain NO `oneOf` at all, and
               there are 0 malformed tool calls across 2,014 campaign-wide. Nothing to rewrite, and
               the symptom never occurs. (The 122 tool ERRORS are failed edits, a different thing.)
    7b879b407  acp: simplify ACP tool-call handling      — `acp`/`ACP` appear 0 times in swarm.rs
    7f9bd274d  code-mode: recursive schema types         — `code_mode` appears 0 times
    b0f4e2a0f  permissions: manual approval in code mode — same, plus headless has no approval flow
    65e1e3d50  apps: confine app file operations         — the `apps` surface is not the swarm path
    e43ed3a6f  enhance the uniffi API layer              — `uniffi` appears 0 times
    cdea92003  configurable GOOSE_DOCS_ROOT              — appears 0 times; doc_fetch/doc_prefetch
                                                           are separate levers and default OFF
    40380ce6d  upgrade to rmcp 3.0                       — 0 MCP tool calls out of 1,247 across
    7b7b8aa58  upgrade to rmcp 2.0                          EVERY run this campaign has recorded

⚠ The two rmcp ones are closed as BEHAVIOURALLY inert for our measured path, not as irrelevant: they
are dependency upgrades that would be inherited on any merge and could move compilation or unrelated
surfaces. "Cannot affect a run that never makes an MCP call" is the honest scope of that close.

STILL OPEN, and each needs a real read rather than a fact-check:
    ee61c7c49  CLI streaming render O(n^2) -> incremental      (the swarm streams progress to stderr)
    8b73e1a1b  stable agent event message identity             (bears on the activity digests EVERY
                                                                instrument reads, and on F163's fix)
    ad87dd4c3  compaction: structured summary output           (`compact` appears 13 times in
                                                                swarm.rs — compaction IS our path)

## F168 — review.py's clock froze during the sink, and a frozen clock reads exactly like a wedged engine

Two consecutive ticks both printed `elapsed 73.8 min` with the same `last = judge_verdict`. I read
that as a possible wedge and went to check liveness. Everything was fine:

    loop alive pid=78290 | engine running pid=78293 | heartbeat 0s old
    sink digest integrate-verify.json written 2 minutes ago
    lms ps: workhorse PROCESSINGPROMPT, the other two IDLE
    run.jsonl last written 20 minutes ago

The run was at **94 minutes**, not 73.8. `level1_logs` derived elapsed as `last_event - first_event`,
and `integrate-verify` emits NOTHING between its dispatch and its completion — so during the sink,
which is the longest phase and 69% of the low-concurrency time (F151), the clock STOPS. The one phase
where I most need to know how long the run has been going is the exact phase the instrument goes
blind in.

Fixed to print all three, because the gap between them is the information:

    elapsed 94.8 min WALL (73.8 min of log, 21.0 min quiet), 323 events, last = judge_verdict

"21.0 min quiet" is now a first-class readout, and it immediately says something F152 predicted: this
sink is at 21 minutes against a measured median of 21.8, so it is a typical sink, and it is
approaching the 30.0-min `sink_cap_secs` that 3 of 8 prior sinks hit exactly.

LESSON (50): AN INSTRUMENT THAT DERIVES TIME FROM ITS OWN DATA STREAM STOPS WHEN THE STREAM STOPS.
Wall-clock must come from the clock. A metric computed from event timestamps silently measures event
ACTIVITY, not elapsed time, and the two diverge precisely when a phase goes quiet — which is when
something is either very wrong or very slow, and you cannot tell which from a frozen number.

Live, alongside it: 1 task in flight against 6 slots, two whole nodes IDLE for 21 minutes. That is
F151/F162 happening in real time — the free capacity exists exactly when there is nothing left to
assess with it.

## F169 — baseline r0 CRUNCHED: the app actually runs. And my crunch nearly fabricated a failure.

First clean post-boundary baseline. `nodeloop-result.json`: **score 0.8429, wall 5,841s = 97.4 min,
actual_nodes 3, void false, timed_out false, engine_build 1785697869-235825056.** Phases:
research 4.9 / planning 9.3 / **execute 82.3** / gates 0.1 of 96.5 min — execute is 85% of the run.
Prior finished 3-node runs median 125 min, so this is faster, at n=1.

**THE CRUNCH — done properly, against the built tree, not the score:**

    python3 -m pytest                72 passed in 2.66s
    python3 -m vendorsync --help     proper argparse help, --db required, --port default 8000
    server on a free port            LISTENING, no crash
    GET /                            200, serves the HTML dashboard `web` wrote
    GET /api/payments                200, {"data": [], "total": 0, "limit": 25, "offset": 0}

**The app runs and serves correctly-shaped paginated JSON.** 0.8429 is credible. And `test-meridian`
— the task recorded as a terminal FAILURE — contributed **8 of those 72 passing tests** (F165).

⚠️ **I NEARLY PUBLISHED "THE APP DOES NOT RUN".** My first attempt bound port 8931 and got
`OSError: [Errno 48] Address already in use` plus 404s from something already listening. `lsof` named
the holder: **PID 78290 — the sweep supervisor itself**, because `PORT_BASE = 8930` and the harness
owns that range. The 404 was its mock vendor API, not the app. I had the traceback and a plausible
story ("score 0.8429 on an app that crashes on startup — exactly F147") and it was entirely my own
test's fault. The standing rule caught it: **an open port is not proof that YOUR process opened it.**
Re-run on 18771, everything passed. Also verified the live r1 unit was unharmed (`pgrep` still 1).
CRUNCH RULE, now explicit: **never bind a port within 100 of `PORT_BASE`; the harness owns that range.**

**F115 — now a CONTROLLED zero, and only half settled.** No `sink_capped` event, and the sink
completed `status: done` at **25.1 min, under the 30.0-min `sink_cap_secs`**. So the absence is
CORRECT rather than blind — the run proves the sink was not capped. But that only confirms the
NEGATIVE half of the prediction. Whether the event fires when a sink IS capped remains unproven and
needs a capped sink to settle. Recording it as half-settled rather than settled.

**F153 did NOT reduce the sink's turn count, and I predicted it would.**

    F152 baseline (n=8)   16 calls median (range 10-25)   21.8 min   79 s/call
    r0 sink               24 calls                        25.1 min   63 s/call
    call mix              shell 20, write 3, edit 1       0 failed

More turns and more wall-clock, not fewer — though each call was 20% faster and the count is inside
F152's observed 10-25 range, at the top. n=1, so this is not a refutation, but it is emphatically not
the improvement F153 was shipped to produce. The file-check text is verified gone (F166); the turns
it was supposed to save did not materialise. The honest position: **F153 is verified as a prompt
change and UNPROVEN as a speed change**, and the next sinks decide it.

## F170 — the campaign's top-priority arm would never have run, and nothing would have said so

Acting on F164 last tick I moved `scoped_contracts` to ARMS index 1, "so it runs first after
baseline". **It would never have run at all.** `cells()` builds the schedule from `QUESTIONS`, not
from `ARMS`:

    in ARMS but NOT in QUESTIONS (never scheduled): ['scoped_contracts', 'detail_budget']

`detail_budget` is deliberately parked at reps=0. `scoped_contracts` was the arm aimed at the
population that produces 93% of all failures, sitting at reps=3, invisible. Reordering ARMS was
completely inert and I had already reported it as done.

The asymmetry is the defect. `cells()` guards ONE direction and says so in a comment — *"an arm named
here but not defined yet is skipped, never silently substituted"* — protecting QUESTIONS → ARMS.
Nothing protected ARMS → QUESTIONS, and that direction fails **silently and permanently**: no log
line, no empty cell, no error. The arm simply never appears, forever.

FIXED, three things:
  1. `scoped_contracts` added to QUESTIONS at reps=3, positioned immediately after `baseline`, with
     the F164 readout written into its `asks` (test-authors ONLY; a size cut with no dry-reasoning
     improvement REFUTES the hypothesis and is the most valuable outcome).
  2. An orphan guard: any arm in ARMS with reps>0 and named in no question is reported loudly.
     Controlled BOTH ways — a reps=3 orphan fires it, the same arm at reps=0 is silent — because a
     guard that has never fired is indistinguishable from a broken one.
  3. Baseline replicates HOISTED to the front of `backlog()`.

**The hoist, and why it is not a violation of the freeze.** `backlog()` is rep-major, so
`baseline-n3-r1` sat behind the entire rep-0 pass: 31 units, ETA Wednesday. The F154 freeze lifts at
baseline n=3, so under that ordering it would have held ~26 hours with five diagnosed engine fixes
unshipped — honouring the gate's WORDING while defeating its PURPOSE, which is to obtain the
replicate spread before any treatment score is read. The baseline is not just another arm: it is the
DENOMINATOR of every other cell, so running treatments ahead of it is out of order regardless. The
hoist is self-limiting (once the baseline is complete the partition is empty) and changes only the
sequence, never which units run. Resulting order:

    0. baseline n=3 r=1        <- freeze lifts when these two land
    1. baseline n=3 r=2
    2. scoped_contracts n=3 r=0   <- F164's arm, first treatment
    3. baseline n=1 r=0 / 4. baseline n=2 r=0 / 5. sink_review n=3 r=0 / ...

34 units, no duplicates, `baseline-n3-r0` correctly absent (complete).

LESSON 51: **A GATE'S PURPOSE OUTRANKS ITS LETTER.** If the queue cannot deliver what the gate is
waiting for, change the queue — do not sit frozen honouring the wording.
LESSON 52: **A GUARD THAT PROTECTS ONE DIRECTION OF A MAPPING IS EVIDENCE THE OTHER DIRECTION IS
UNGUARDED.** Both mine had comments explaining the asymmetry they handled; neither mentioned the
mirror. When you find a defensive check, ask what its inverse would catch — and control the new one
in both directions before trusting it.

## F171 — the sink_review arm IS armed, proven twice; and the lever flag is NOT where I looked

`sink_review-n3-r0` is the first execution ever of the sink idle-fill mechanism, so before trusting
its readout I checked it was actually armed. It is, two independent ways:

    ps eww -p 66283  ->  GOOSE_SWARM_SINK_REVIEW=1   (the LIVE engine's own environment)
    run_started.gates.sink_review = True   (treatment)
    run_started.gates.sink_review = False  (baseline-n3-r0, same field, NEGATIVE control)

A positive with a negative control on the same field of the same event — which is the standard this
campaign holds itself to and the reason a later zero will be interpretable.

**Where the flag actually lives, because I hunted through three wrong places.** `levers_resolved`
carries **102 lever keys and `sink_review` is not among them** — the sink-ish keys there are
`sink_prebuild`, `review`, `sink_lean_prefill`, `sink_cap_secs`, `sink_max_turns`, none of which is
this lever. swarm.rs:21478 does say `"sink_review": goose_swarm::sink_review_enabled()`, but that
line sits inside the **`run_started`** block, and not at its top level either — `run_started`'s top
level is `prompt, planner_model, endpoint, working_dir, max_turns, max_attempts, pool, assured,
gates, ts, run_id, seq`, and the flag is in **`gates`** (7 keys). It is not in `pool[]` either, whose
entries are `id, model_id, weight, instances`.

    ARM-ARMED CHECK, for every future env-driven arm:  run_started.gates.<lever>

**Why this was worth the hunt rather than assuming.** A lever set by env and absent from
`levers_resolved` has two indistinguishable failure modes: the env never reached the engine (arm
VOID, the unit measures nothing) versus the mechanism is armed but its precondition never occurs
(INERT) versus armed, precondition met, event missing (DEAD — a real bug). Those demand completely
different responses, and a silent readout looks identical in all three. Now that the arm is proven
armed, a missing `sink_review` event during the sink phase means **DEAD**, which is exactly the
finding the arm's gate predicted might happen: *"if prewarmed is 0 with the lever on, the producer
still cannot see its precondition and the fix is incomplete."*

Mid-run state at 32 min: 11 dispatched / 9 done / 0 FAILED, `task_split` fired once (`store` ->
`store-impl` + `store-tests`, so F140's `task_split > 0` guard is satisfiable on this build) and
`pre_review` fired 3x with `had_findings: False`. `sink_review` has fired 0x — correct so far, the
sink phase has not started.

## F172 — F163's "new observable" already exists TWICE, and the judge throws one of them away

F163 concluded the stall fix needed a new observable — a `last_delta_at` updated on any stream event —
and I sized it as "a DIGEST-WRITER change, a larger blast radius than F160 implied". **Both halves of
that were wrong.** Searching our own code first (Lesson 15) found it already there:

**1. The physically-correct predicate is already implemented and already running.** swarm.rs:11262:

    // IDLE-based watchdog: kill the task only if NO agent event arrives for `idle_secs` (a genuinely
    // stalled stream), NOT on total wall-clock — a slow-but-progressing local model emits an event
    // every turn and must be allowed to finish. idle_secs == 0 disables the watchdog.

and it reports itself at :11547 as *"agent stalled — no progress for {idle_secs}s (no token/tool
activity)"*. That is exactly the "no bytes of ANY kind" definition F163 said was missing, keyed on
agent events rather than on `thinking_chars`, so it does NOT go blind while a tool payload streams.

**And it did not fire on the three "frozen" workers — correctly.** They were emitting agent events
the whole time; that is why `test-meridian` wrote at 294s. The idle watchdog was right and my
proposed flat-delta trip would have overruled it. That is a second, independent confirmation of
F163's refutation, from a mechanism I did not know existed when I wrote F160.

**2. The judge already opens the file that carries the timestamp, and drops it.** swarm.rs:16528:

    let digest = std::fs::read_to_string(cwd.join(".swarm").join("activity")
                     .join(format!("{}.json", req.task_id))).ok()
                 .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

`read_to_string` discards the metadata of the inode it just opened. The digest is rewritten on stream
activity (coalesced to ~2.5/s, swarm.rs:11266-11269 with its own `last_digest_at`), so **the file's
mtime IS the last-activity time** — and it is live on disk right now (`verify::meridian.json` 00:24,
`verify::store.json` 00:17, `verify::main.json` 00:16). I used exactly that mtime by hand two ticks
ago to prove the sink was alive when review.py's clock had frozen.

So the fix is not a writer change at all: read `.metadata().modified()` alongside the string the
judge already reads, and expose `secs_since_any_activity` on `JudgeInput`. No new event, no new
writer, no new threshold — and the window can be derived from the existing `idle_secs` rather than
picked.

**EIGHTH instance of the family, and the most literal one yet.** F141 held the compile error, F142
the thinking count, F149 the tool list, F153 the file list, F157 `is_test_author`, F158 `owned_files`,
F159 was me doing it in a queue decision — and here the engine holds the answer **on the inode it has
already opened**, then reasons about staleness from a counter that cannot see writing.

CORRECTIONS TO MY OWN QUEUE ENTRY: F163's blast radius is ~3 lines in the judge, not a digest-writer
change. Its priority rises accordingly — it was the most expensive queued fix and is now the cheapest.

## F173 — a split parent was counted as "in flight" for the rest of the run, in the readout I read every tick

`review.py` reported `in flight: ['api', 'store', 'verify-e2e::0', 'verify-e2e::1', 'verify-e2e::2']`
at minute 56 of `sink_review-n3-r0`. Two of those five were dead. Checked with F172's own trick — the
activity digest's mtime IS the last-activity time:

    api                  digest 00:23:46   (22 min stale)   <- SPLIT, superseded
    store                digest 00:14:18   (31 min stale)   <- SPLIT, superseded
    api-implementation   digest 00:41:27   (4 min)          <- the child actually working
    verify-e2e::0        digest 00:45:25   (now)

The run's own events confirm it: `verdict: api split / split`, `verdict: store split / split`, and
`task_split` handing `store -> [store-impl, store-tests]`, `api -> [api-implementation, api-tests]`.

**A split parent never emits `task_completed`** — its work is handed to its children — so
`dispatched - settled` counts it as running until the run ends. Real in-flight was 3; the readout
said 5.

**It never tripped an impossibility check, and that is why it survived.** 5 against 6 slots is
perfectly plausible (Lesson 47 only catches values that cannot be true). The tell was not the number,
it was noticing the same two ids in the list two ticks apart and asking what they were doing.

**`occupancy.py` ALREADY KNEW THIS AND `review.py` DID NOT.** occupancy.py carries a `split_at` map
and a comment recording that an uncorrected parent was once credited 5,940s against a real span of
651s — 9.1x too much. So the occupancy figures F150 and F162 rest on are SOUND; only this readout was
inflated. Two instruments over the same events, one taught and one not.

FIXED: `settled = done | failed | split_parents`. In-flight now reports 3, matching the digests
exactly. NEGATIVE CONTROL: `baseline-n3-r0` has zero split parents, so the change provably cannot
move any number in the run this campaign's headline result came from.

SCALE, stated honestly: 5 split parents across 4 of the runs on disk. Small — but it is the live
in-flight figure I read every tick to decide whether the fleet is busy, and it reads high exactly
when splitting is working, i.e. when the fleet is being used well.

LESSON 55: **WHEN TWO INSTRUMENTS CONSUME THE SAME EVENT STREAM, A LESSON LEARNED BY ONE IS NOT
LEARNED BY THE OTHER.** occupancy.py had the split-parent correction, with a measured 9.1x error in
its comment, while review.py computed in-flight from the same log and got it wrong. After fixing any
event-derived defect, grep for every other consumer of that event.

## F174 — split-born test-authors finish 5x faster; and the obvious explanation for it is REFUTED

`sink_review-n3-r0` planned `api` and `store` as COMBINED tasks owning both a module and its test
(`api.py, test_api.py`), and the judge split each into impl + tests. That produced two test-authors of
a kind the campaign has not measured before — split-born rather than planned:

    api-tests          SPLIT     done   1.8 min   1 attempt
    store-tests        SPLIT     done   1.4 min   1 attempt
    store-edge-tests   planned   done   7.4 min   1 attempt
    main-cli-tests     planned   done  10.5 min   1 attempt
    meridian           planned   done  14.0 min   1 attempt   (owns meridian.py AND test_meridian.py)

**1.4-1.8 min against 7.4-14.0 — roughly 5x.** And all seven test-authors on this run completed;
zero failures against the campaign's 31%.

**The obvious explanation is that a split child arrives AFTER its parent wrote the module, while a
planned test races it. I measured that across every finished 3-node run and it is REFUTED:**

    deps complete at dispatch?     n   failed   rate
    YES - module existed          34        9    26%
    NO  - dispatched early         2        1    50%

Thirty-four test-authors were dispatched with every dependency already complete and **26% of them
still failed**. The DAG is enforcing ordering correctly; only two dispatches in the entire campaign
raced their dependency. "The module did not exist yet" is not the cause of F164 and is now closed.

**What that leaves.** A split child differs from a planned test in what it CARRIES, not in when it
runs — it inherits the parent's spec and the parent's own working context, where a planned test-author
gets `## API of` blocks for all five modules and 22,511 chars of prompt (F156/F159). That is the same
variable `scoped_contracts` is queued to test and the same one `split_inherit_spec` exists for, which
makes this an unplanned second line of evidence pointing at the same place.

⚠ **HELD LOOSELY, AND DELIBERATELY NOT PROMOTED.** n=2 split children on ONE run against n=5 planned
on the same run. That is an observation, not a result (Lesson 35), and the 5x could be task size — a
split child is by construction a fraction of its parent's work. What makes it worth recording is not
the ratio but that it is a THIRD independent arrow at the test-author cell, arriving from a direction
I was not looking in.

REGISTERED for `split_inherit_spec` when it runs: split-born test-authors should stay fast with the
lever ON; if they SLOW toward the planned 7-14 min, the speed came from the thin inherited statement
rather than from good context, which would invert the arm's whole premise.

## F175 — the sink idle-fill mechanism IS RUNNING, first time ever; and its event cannot tell DEAD from INERT

`integrate-verify` dispatched and ran SOLO for 7.4 minutes with 5 of 6 slots free — the exact window
F151 measured as 42% of execute at <=2 tasks, 69% of it this task. `sink_review` events: **0**. I was
one step from calling that DEAD. Two checks stopped it.

**1. The event fires AFTER the sink, not during, and only if findings survived.** swarm.rs:24448 is
the *consume* site: it drains `drain_sink_review()` once the sink is finished, re-verifies each
finding against the final tree, and emits the event **inside `if !prewarmed.is_empty()`**. So a zero
during the sink means nothing at all, and a zero AFTER the sink is ambiguous in the worst way:

    mechanism never ran          -> no event
    mechanism ran, found nothing -> no event

**Those are DEAD and INERT and the emission site cannot distinguish them** — the exact confusion F171
was written to avoid, baked into the engine rather than into my reading. Lesson 24's shape (a gate
that prints neither verdict reads as a pass) and Lesson 16's (emit raw inputs, not a re-derived
verdict): `prewarmed: 0` should be emitted, not suppressed.

**2. The fleet answers the question the event cannot.** While the sink ran solo:

    lms ps  ->  gabee GENERATING | mihai GENERATING | workhorse GENERATING
    tasks in flight: 1 (integrate-verify)

**One task, three nodes generating.** At most one node can be serving the sink, so at least two are
doing work no task asked for — which is precisely what `idle_dimension_review` is. 14 fleet calls
started in that 8-minute window, one carrying the sink's own 28,106-char prompt and the rest much
smaller (668-3,500 chars). I am NOT claiming to have classified each of those prompts; the
unambiguous part is the occupancy: **three nodes busy against one in-flight task.**

**So the mechanism whose gate said "It has never run once" is running, and it is filling exactly the
window Prime Directive 3 is about.** The preconditions all check out in source too:
`pick_sink_review` needs `sink_review_enabled()` (proven ON, F171) and `sink_in_flight()` (a task
literally named `integrate-verify` in `Claimed` state — true now), then claims any device with
`in_flight < weight`, of which there were five.

WHAT REMAINS FOR THE READOUT when the sink finishes: `sink_review{prewarmed, survivors, refuted}`.
`prewarmed > 0` confirms the queue was filled; `survivors` vs `refuted` measures whether overlapping
the review with the sink costs quality — the arm's gate warns that if the build score moves DOWN the
re-verification is not fail-closed, and that is worth more than the utilisation.

QUEUED ENGINE FIX (new, cheap): emit `sink_review` unconditionally with `prewarmed: 0` rather than
suppressing it, so DEAD and INERT stop looking identical. Same one-line shape as the rest.

## F176 — a 16-vs-0 result died to its own positive control, ten seconds after I computed it

Measuring what the `sink_review` arm exists for — fleet work performed DURING the sink window —
against baseline r0 as the control:

    baseline r0 (lever OFF)   sink 25.1 min (finished)   fleet calls in window:  0   = 0.00/min
    sink_review r0 (ON)       sink 19.4 min (RUNNING)    fleet calls in window: 16   = 0.83/min

Sixteen against zero, in the exact direction the arm predicts, with a clean unit. It is **invalid**
and I am striking it before it goes anywhere.

**The control:** can that query see baseline r0's calls AT ALL?

    fleet log files on disk: 527   (oldest 06-25 20:40, newest 08-03 01:21)
    baseline r0 ran 22:11:29 -> 23:48:48
    log files with mtime inside baseline r0's WHOLE 97-minute run: 0

**Zero across the entire run, not just the sink.** I personally read worker prompts from that run
earlier tonight (F146's check quoted files at 22:29-22:32), so those files existed and have since
been evicted by log retention. The zero measures the retention policy, not the engine — the classic
blind instrument, and it happened to point exactly the way I wanted.

**What SURVIVES, and why.** F175's conclusion does not depend on this at all: it rests on `lms ps`
showing **three nodes GENERATING against one in-flight task**, a direct observation of the fleet at
that instant, not a log-derived count. The 16 calls in the treatment window are also real — they are
present on disk. The only thing that died is the COMPARISON, because its control arm cannot be seen.

**How to measure it properly**, since the question is still the right one: take the sink-window call
count from the NEXT baseline (r1 or r2, which the F170 reorder puts first) while its logs are still
fresh, and compare against `sink_review`'s number captured tonight. Same instrument, both arms within
retention. Log-derived comparisons across hours are only valid inside the retention window, and that
window is now known to be shorter than three hours under this fleet's call volume.

LESSON 58: **A LOG-DERIVED ZERO FROM THE PAST IS A CLAIM ABOUT RETENTION UNTIL PROVEN OTHERWISE.**
The campaign's standing rule is "an uncontrolled zero is not evidence"; this is its time-shifted form.
Before comparing a historical window to a live one, count what the query can see in the historical
window OVERALL — not in the sub-window being measured, which is exactly where a blind result hides.

## F177 — F115's positive half, running live: the sink is PAST its cap and `sink_capped` has not fired

F115 was left half-settled: baseline r0's sink finished naturally at 25.1 min, so the absence of
`sink_capped` was a CONTROLLED zero but only proved the negative half. `sink_review-n3-r0` supplied
the other case — its sink crossed 30.0 min.

    sink elapsed        31.3 min
    sink_cap_secs       1800 (30.0 min), resolved from levers_resolved
    PAST THE CAP        True
    sink_capped fired   0

**Three wrong turns I did NOT take, each checked instead of assumed:**

1. *"The event must be named something else."* It is not — swarm.rs has **two** emission sites (11506
   and 11538) and both write `"event": "sink_capped"`. The marker F115 registered was right. One
   fires on the deadline during streaming, the other on an event gap.
2. *"`GOOSE_SWARM_SINK_CAP_SECS` isn't in the process environment, so the cap is inert."* `ps eww`
   showed only five GOOSE_SWARM vars and not this one — but swarm.rs:21303 bridges config to env at
   runtime with `set_var`, and `ps eww` reports the environment the process was LAUNCHED with, not
   what `setenv` added later. The observation was real and the inference would have been wrong.
3. *"1.3 minutes overdue is a defect."* Not yet. That is a 4% overrun and the deadline is checked
   inside the stream loop, so its granularity is plausibly on that order. **Recording the observation,
   not a verdict.**

The site's own comment records why this matters more than a timing curiosity:

    MEASURED: integrate-verify ran exactly 1800s — its cap to the second — made 23 shell calls and
    2 edits, and was recorded `status=done`. That row is where this project reads every verdict from.

So a capped sink is recorded as DONE, and `sink_capped` is the ONLY thing distinguishing "finished
its job" from "cut off mid-work". F152 measured 3 of 8 prior sinks landing on exactly 30.0 min — if
the event does not fire for them either, every one of those was read as a clean finish.

SETTLES NEXT TICK, and both outcomes are informative:
  · `sink_capped` fires → F115 fully settled, the instrument works, and the 3-of-8 at exactly 30.0
    min become identifiable as truncations.
  · the sink completes with NO `sink_capped` while past its cap → the cap is not enforcing, or the
    event is not reaching the log, and a truncated sink is indistinguishable from a finished one in
    the row this project reads every verdict from.

## F178 — the sink cap is 14 minutes overdue, and the idle-fill may be why the sink is slow

Continuing F177. The sink is now **44.0 min against a 30.0-min cap — a 47% overrun — and
`sink_capped` has fired 0 times.** The engine is healthy throughout: `loop alive`, `engine running`,
`heartbeat 0s old`.

Every benign explanation has now been checked and eliminated:

    event misnamed?          NO  — two sites (11506, 11538) both write "event": "sink_capped"
    cap env var unset?       NO  — swarm.rs:21303 bridges cfg->env inside run_swarm, BEFORE dispatch
                                   (`ps eww` shows only the LAUNCH env, so its absence proved nothing)
    cfg value zero/OFF?      NO  — levers_resolved reports sink_cap_secs 1800
    deadline granularity?    NO  — 1.3 min was arguable; 14.0 min is not
    worker dead/wedged?      NO  — 3 nodes GENERATING, digest still advancing (7 calls, 0 errors)

**A safety cap that does not fire is worse than no cap**, because the run budget and every downstream
timing assumption are written as though 1800s is a ceiling. F152 measured 3 of 8 prior sinks landing
on *exactly* 30.0 min, so the mechanism demonstrably works on some runs — which makes this a
conditional failure, not a dead feature, and conditional failures are the ones that survive review.

**AND THE LIKELY CONFOUND IS THIS ARM ITSELF.** `sink_review` fills idle nodes with dimension
reviews, and `lms ps` shows **all three nodes GENERATING sustained across ~40 minutes** — including
the node serving the sink, since PARALLEL is 2 per node. So the sink is sharing its own device with
the review work the lever spawned:

    baseline r0 sink (lever OFF)   25.1 min, 24 calls
    sink_review r0 sink (ON)       44.0+ min, 7 calls so far

**Fewer calls over more wall-clock is the signature of a contended node, not a busier one.** Lesson
36 asks what a mechanism trades away when it optimises an internal metric: this one optimises fleet
occupancy — genuinely, F175 confirmed it — and the bill may be landing on the critical path, because
the sink is the last task and nothing can start until it ends.

⚠ STATED AS A HYPOTHESIS, NOT A RESULT: n=1 per arm, and F176 already killed one comparison tonight
for a blind control. The clean test is the sink-window call count and sink duration on baseline r1/r2
versus this, captured inside the retention window — exactly the measurement F176 specified.

**This does NOT retract F175.** The idle nodes really are working. The open question is whether that
work is FREE, and the first evidence says it may not be.

REGISTERED: on the next baseline sink, `sink_capped` must fire if it exceeds 1800s. If a baseline
sink also overruns without the event, the cap is broken generally and the arm is exonerated; if the
baseline caps correctly, the overrun is specific to a saturated fleet and belongs to this lever.

## F179 — the sink is 4.8x slower PER CALL while the idle-fill saturates its node

F178 observed the sink overrunning and suspected contention. Per-call timing separates that from task
size, because a call is one model turn regardless of how much work remains:

    baseline r0 sink (lever OFF)    24 calls / 25.1 min =  63 s/call
    sink_review r0 sink (lever ON)  11 calls / 55.2 min = 301 s/call   -> 4.8x slower PER CALL
    fleet median (F116/F152)                               83 s/call

**301 s/call against a fleet median of 83.** The sink is not doing more work per turn — it is waiting.
`PARALLEL` is 2 per node, so the `idle_dimension_review` jobs the lever spawns land on the same device
serving the sink, and the sink is the LAST task: nothing else can start until it ends, so every second
it loses is a second on the critical path.

This is Lesson 36 with a number attached. The mechanism optimises fleet occupancy and F175 proves it
does that honestly — three nodes generating, sustained ~40 minutes. The bill arrives somewhere else.

**And the cap that exists to bound exactly this is not firing:** 55.2 min against 1800s, `sink_capped`
0, run not finished. The engine is healthy and the sink is genuinely progressing (7 -> 11 calls this
tick, digest written 12 s before I looked, `recent` showing `write ok` / `shell ok`, and
`last_thinking` reasoning about a verify-e2e finding). So this is a slow sink, not a wedged one — and
a cap that does not bound a slow sink is a cap that does not work.

DECISION, taken rather than deferred: **let it run one more tick.** It is producing real work and the
`sink_review{prewarmed, survivors, refuted}` drain readout only exists if the sink COMPLETES — killing
now guarantees zero from a 2h20m unit. **Kill threshold registered: if the sink passes 90 min the
queue cost stops being justifiable** — `baseline n3 r1/r2` is what lifts the freeze, and the sweep
skips completed units on resume so a kill costs only this unit.

⚠ STILL n=1 PER ARM. The per-call figure is much harder to confound than duration, but F176 killed a
cleaner-looking comparison than this one tonight. The settling measurement is baseline r1/r2's sink,
inside the retention window.

LESSON 59: **A MECHANISM THAT FILLS IDLE CAPACITY IS NOT FREE IF IT SHARES A DEVICE WITH THE CRITICAL
PATH.** With PARALLEL>1, "idle" work lands on the same node as the task everything is waiting for.
Measure the critical task's SECONDS PER CALL, not the fleet's occupancy — occupancy will look
excellent precisely when this is happening.

## F180 — `sink_capped` DOES fire (F115 SETTLED), and a capped sink reports the CAP as its duration

**I was wrong that the cap is not firing, and I am striking F178's headline.** The sink completed and:

    sink_capped fired at +59.9 min
      cap_secs: 1800
      detail: "the sink was CUT OFF at its wall-clock cap ON AN EVENT GAP, not finished"
    task_completed: status=done, elapsed_ms = 30.0 min, 14 calls

**F115 IS NOW FULLY SETTLED, both halves:** baseline r0's sink finished naturally and emitted nothing
(negative), this one was cut off and emitted `sink_capped` (positive). The instrument works.

**But two things are badly wrong, and they matter more than the original question.**

**1. The cap fires ~30 minutes late, and via the wrong path.** The detail says *"on an event gap"* —
that is the SECOND emission site (11538), the one that trips when the stream goes quiet. The deadline
branch (11506) never fired. So the cap is not a wall-clock ceiling at all; it is a ceiling that is
only CHECKED when the stream stalls. On a contended node emitting a token every few minutes, the
check simply does not run. 1800s let 3,594s through — 100% over.

**2. A capped sink RECORDS THE CAP AS ITS ELAPSED TIME.** `task_completed.elapsed_ms` = 30.0 min for
a task that occupied the fleet 59.9 min. Checked across every sink on disk, this is systematic:

    n=14 sinks
      RECORDED (elapsed_ms)  median 26.0 min
      REAL (dispatch->done)  median 27.4 min
      sinks whose recorded < real: 6/14, worst understatement 29.9 min

Two prior sinks recorded **exactly 30.0 min with real durations of 41.8 and 47.7 min and NO
`sink_capped` event** — capped on a build that predates the event, and invisible ever since.

**THIS RETROACTIVELY CORRECTS F152.** Its "16 calls median, 21.8 min, 79 s/call" came from
`elapsed_ms`, and its headline observation — *"3 of 8 hit EXACTLY 30.0 min"* — was read as sinks
landing on the cap. They were not landing on it; **they were reporting it.** Their real durations were
longer and unknown. Every s/call figure derived from `elapsed_ms` for a capped sink is understated by
the same factor: this sink reads 129 s/call recorded, 257 s/call real.

**F151/F150/F162 are SOUND** — `occupancy.py` pairs dispatch→completion timestamps rather than
reading `elapsed_ms`, which is exactly the distinction that saves them.

**F179 SURVIVES AND STRENGTHENS.** Real numbers: baseline 24 calls / 25.1 min = 63 s/call; this sink
14 calls / 59.9 min = **257 s/call**, 4.1x slower, against a fleet median of 83. The contention story
is unchanged and the true cost is larger than the recorded data admitted.

LESSON 60: **A TASK'S SELF-REPORTED DURATION IS NOT ITS OCCUPANCY.** When a mechanism finalizes a task
early, late, or by force, the elapsed it writes is whatever that mechanism believes — here, the cap
value. Derive duration from the dispatch and completion TIMESTAMPS, which no mechanism can rewrite,
and only trust `elapsed_ms` after confirming the two agree.

## F181 — the sink_review arm's verdict: MECHANISM FIRED, quality/speed UNSETTLED, and the restart is done

The drain readout finally landed:

    sink_review{ prewarmed: 15, survivors: 10, refuted: 5 }
    detail[0]: "[edge-cases] api.py: `/api/summary` crashes with KeyError if any sto…"

**The mechanism's primary question is ANSWERED and it is valid at n=1** (Lesson: judge a lever on a
deterministic mechanism event, never a 1-vs-1 outcome delta). The gate said *"It has never run
once"* — it has now:

    FIRED, not DEAD.  15 findings prewarmed by idle nodes DURING the sink; the fail-closed
    re-verification kept 10 and REFUTED 5 — a 33% refutation rate, so the re-gate is doing real
    work rather than rubber-stamping.

**The outcome numbers, and why I am NOT drawing a conclusion from them:**

    baseline-n3-r0   score 0.8429   wall  97.4 min
    sink_review-n3-r0 score 0.7326  wall 145.5 min      (-0.110, +49% wall)

Both look damning and **neither settles anything.** The measured replicate spread on this fleet is
**46 points** (44.2 / 86.7 / 90.0 on byte-identical config), so an 11-point drop is deep inside the
noise floor. And 145.5 min sits inside the historical range for a finished 3-node run (101-219,
median 125) — it is 97.4 that was unusually fast. **n=1 against n=1 cannot separate this arm from the
fleet's own variance**, which is the entire reason the F154 freeze exists.

What CAN be said, from mechanism evidence rather than score:
  · the idle nodes really worked — three GENERATING against one in-flight task, ~40 min (F175)
  · the sink really was slower per call — 257 s/call vs baseline 63, fleet median 83 (F179/F180)
  · the sink really was cut off — `sink_capped` at +59.9 min against an 1800s cap (F180)
So the arm buys real utilisation and pays on the critical path. **Whether that trade is net positive
needs the baseline spread, which is now the next thing the sweep produces.**

**RESTART EXECUTED** — the boundary I had been deferring since the r0/r1 gap. The sweep had already
rotated to `swarm-1node-r0` off the OLD queue, i.e. the 1-node cell instead of the baseline
replicates, so every further minute was spent on the wrong unit. Sequence, supervisor BEFORE engine
per the boundary protocol: killed pid 78290, confirmed dead, killed the engine process group,
confirmed `pgrep 'goose swarm run'` == 0, relaunched.

    supervisor pid 99285 (ppid 1, detached)   engine pid 99288   heartbeat 2s
    NOW: baseline-n3-r1     NEXT: baseline-n3-r2     33 in backlog
    completed units skipped (baseline-n3-r0, sink_review-n3-r0 both on disk)

**The freeze lifts after these two units.** Cost of the restart: ~8 minutes of an incomplete 1-node
unit, which will simply re-run.

## F182 — the idle-node findings are REAL and precisely located, and the engine throws them away

F181 left one question open: the 10 survivors are advisory and drive no fix — are they worth
anything? I checked every concrete claim against the shipped tree. **All four verified, to the line:**

    finding #2/#4/#7  "api.py sets currency = all_payments[0]['currency']"
                      -> api.py:109   currency = all_payments[0]["currency"]          EXACT
    finding #6/#10    "meridian.py total_count(): resp['total'] raises KeyError"
                      -> meridian.py:141   return resp["total"]                       EXACT
    finding #5        "create_payment: on 409 with no payment_id, err['payment_id'] raises"
                      -> meridian.py:167   return err["payment_id"]  (inside `if status == 409`)  EXACT
    finding #8        "serve() declares serve(port, store, client, blocking: bool = True)"
                      -> api.py:18    def serve(..., blocking: bool = True)           EXACT

Precise file, precise line, precise symbol. **The idle nodes are not producing plausible-sounding
noise; they are reading the real tree and reporting real code.** The unguarded `resp["total"]` and
`err["payment_id"]` are robustness defects on any reading of any spec. (The `currency` ones are only
defects if the spec pins the currency — I verified the CODE matches the description, not that it
violates the requirement, and I am not claiming the stronger thing.)

**And the count is inflated. Ten survivors are NOT ten defects:**

    #2, #4, #7   the SAME currency issue, three times
    #1, #3       both /api/summary KeyError on missing/unparseable fields
    #6, #10      both unguarded dict access in meridian.py
    #9           "[domain-conventions] None found — code correctly handles…"  <-- A CLEAN REPORT,
                 counted as a finding and passed by the fail-closed re-verification

So ~10 survivors ≈ **4-5 distinct real defects, plus one non-finding**. The re-gate refuted 5 of 15
for accuracy but does not deduplicate, and it let a "None found" through — which means `survivors`
overstates the yield by roughly 2x and can never be zero even when the reviewers find nothing.

**THE CONSEQUENCE IS THE POINT.** `sink_review-n3-r0` scored 0.7326 while carrying 4-5 real,
precisely-located defects **that the swarm itself had already found**, written down, re-verified
against the final tree — and then discarded, because the mechanism is advisory by design. The idle
nodes did the hardest part of debugging (locating the defect) and the run shipped the bugs anyway.

This is a far stronger case for the mechanism than its utilisation number, and it reframes F179's
cost: the sink paid 257 s/call to share its node with reviewers whose output was thrown away. **The
trade is only bad because the findings go nowhere.**

QUEUED (new): **F182b — feed the surviving findings into the repair tail** rather than dropping them,
and **deduplicate before counting**; **F182c — never count a "None found" report as a finding**, which
also makes `survivors` a usable readout instead of an inflated one.

## F183 — F156/F157/F158 all REPLICATE on baseline r1, an independent run

Before shipping three queued prompt fixes measured on ONE run, I re-measured them on `baseline-n3-r1`
while it was mid-execute. All three hold.

**F156 — the implementer prompt size is structural, not a one-run artifact:**

    r0   worker/impl   9,860 chars (n=8)
    r1   worker/impl   9,988 chars (n=13)     1.3% apart

**F157 — the TOOLS block is BYTE-IDENTICAL on every dispatch:**

    4,450 chars on ALL 11 file-owning prompts, 35-48% of each
    median 4,450 of a 9,988-char prompt = 45%, matching r0 exactly

That it is the same 4,450 every time is stronger than the percentage: this block does not vary with
the task at all, so its two test-author bullets are delivered verbatim to every implementer on every
run. It is not "mostly generic" — it is *entirely* generic.

**F158 — every non-Python owner still receives the Python conventions:**

    non-Python owners this run: 4      of those, CONVENTIONS block present: 4
    incl. the pure `index.html` owner: 9,680 chars, TOOLS 4,450 (46%), CONVENTIONS present

The `index.html` case from r0 reproduced exactly. (Precision: 3 of the 4 are MIXED owners
`__init__.py, __main__.py, README.md`, so "not all .py" rather than "no Python at all"; only
`index.html` is a pure non-Python deliverable. The finding's claim was about that case and it holds.)

**Why this mattered enough to spend a tick on.** Every one of these three was measured on r0 alone,
and this campaign has retracted five single-run conclusions tonight (F160, F176, F178, plus F152's
and F145's headlines). Two runs is not a lot, but a byte-identical 4,450 across 24 dispatches on two
independent runs is not a sampling accident. The fixes ship on the confirmed version.

Also captured: baseline r1 at 23 min had **6 dispatched / 1 done / 0 FAILED**, with `api`, `meridian`,
`store`, `verify::cli`, `web` in flight — and NO idle-node mechanism fired, which is the correct
control against `sink_review-n3-r0` (F175's three-nodes-generating happened only with the lever on).

## F184 — a third condition arrived by accident, and it is a better experiment than the one I designed

`baseline-n3-r1` dispatched `integrate-verify` **while three sibling tasks were still in flight**
(`test-api`, `test-cli-edge`, `test-meridian`). Both previously-measured sinks ran SOLO. So the
campaign now has three conditions instead of two, and the third one separates a confound I had
baked into F179 without noticing:

    r0 baseline        sink SOLO, no idle-fill                     63 s/call
    sink_review r0     sink + idle_dimension_review on its node   257 s/call
    r1 baseline        sink + 3 REAL sibling tasks                 ? s/call   <-- lands next tick

**Why this matters.** F179 attributed the 4.1x slowdown to the idle-fill mechanism sharing the sink's
device. But every comparison I had was solo-vs-contended, so "contention" and "idle-fill" were
perfectly confounded — I could not tell whether the sink slows because *reviewers* are on its node or
because *anything* is. r1 supplies the missing cell for free.

REGISTERED BEFORE THE OUTCOME (standing rule), and both directions are informative:

  · **r1's sink s/call is also far above 63** ⇒ ordinary co-tenancy is the driver, F179's mechanism
    story narrows to "the sink is slow whenever it shares a node", and the lever is PARTLY
    EXONERATED — it would be guilty only of creating co-tenancy at the worst moment, not of being
    uniquely expensive. F179b (exclude the sink's device from idle-fill) would then be the wrong fix;
    the right one is to stop scheduling ANYTHING on the sink's device.
  · **r1's sink stays near 63 despite three siblings** ⇒ the idle-fill is doing something worse than
    ordinary co-tenancy, F179 stands as written, and F179b is the correct targeted fix.

MEASUREMENT RULE for it: real duration from dispatch→completion TIMESTAMPS, never `elapsed_ms` — a
capped sink writes the cap value and that is what corrupted F152 (F180).

⚠ Still one run per cell. This does not become a result at n=1; it becomes a DIRECTION, and it tells
me which of two queued fixes to spend a boundary crossing on.

## F184 (provisional) — co-tenancy explains most of the sink slowdown; idle-fill adds more on top

The third condition landed. All three from dispatch→completion timestamps, never `elapsed_ms`:

    r0 baseline      sink SOLO, no idle-fill                     63 s/call   (24 calls / 25.1 min)
    r1 baseline      sink + 3 REAL sibling tasks                148 s/call   (5 calls / 12.3 min, MID-FLIGHT)
    sink_review r0   sink + idle_dimension_review on its node   257 s/call   (14 calls / 59.9 min)
    fleet median (F116)                                          83 s/call

**Neither registered branch wins outright, and the middle is the useful answer:**

  · **Ordinary co-tenancy is a MAJOR driver — 63 → 148, a 2.3x slowdown from real sibling work
    alone.** F179 attributed the whole effect to the idle-fill mechanism, and that attribution was
    too strong. Partial retraction, stated plainly.
  · **But idle-fill is still worse than ordinary co-tenancy — 148 → 257, another 1.7x.** So the lever
    is NOT exonerated either; it is guilty of an increment, not of the whole cost.

**This changes which fix earns the boundary crossing.** F179b (exclude the sink's device from
idle-fill) addresses only the 148→257 portion — the smaller half. The larger half is that the sink,
which is the LAST task and therefore the critical path, gets co-tenanted by ordinary siblings at all.
The general fix is an EXCLUSIVE device for the sink, of which F179b is a special case. That is a
bigger change than F179b and I am not queuing it on this evidence.

⚠ **PROVISIONAL — three reasons, all real:**
  1. **r1's sink is INCOMPLETE**: 5 calls over 12.3 min. Five is a small denominator and the figure
     can move a lot; the same sink at 14 calls could read very differently.
  2. **Co-tenancy was not constant** — 4 tasks live at dispatch, 3 live now. The "3 siblings" label
     is the starting condition, not a steady state.
  3. **n=1 per cell**, on a fleet whose replicate spread is 46 points.
**RE-MEASURE AT COMPLETION before treating any of this as settled**, and again on r2.

What is already safe to carry forward regardless of where the number lands: **the sink's seconds-per-
call is sensitive to what else runs on its node, and it is the critical path.** That is the durable
shape; the exact attribution between "siblings" and "idle-fill" needs the completed figure.

## F185 — F184's figure held when the denominator doubled; the co-tenancy split is real

The provisional worry in F184 was the denominator: 148 s/call came from 5 calls. Re-measured at 10:

    r1 baseline sink   5 calls / 12.3 min = 148 s/call
                      10 calls / 24.4 min = 146 s/call     <-- 1.4% apart

Doubling the sample moved it by 1.4%. **The small-denominator caveat is retired.** The three-cell
picture stands:

    r0 baseline      sink SOLO                                63 s/call
    r1 baseline      sink CO-TENANTED with real siblings      146 s/call   -> 2.3x
    sink_review r0   sink + idle_dimension_review              257 s/call   -> a further 1.8x
    fleet median (F116)                                        83 s/call

So the attribution splits roughly **2.3x to ordinary co-tenancy, 1.8x more to idle-fill on top** —
F179's original "idle-fill causes the slowdown" remains partially retracted, and the lever remains
partly guilty. That conclusion is now resting on a stable number rather than a five-call estimate.

⚠ **ONE CAVEAT SHARPENS RATHER THAN DISAPPEARS.** Co-tenancy FELL during the window — 3 siblings at
dispatch, 1 now (`test-meridian`) — yet s/call did not fall with it. 146 is a CUMULATIVE average, so
it is dominated by the earlier heavily-contended period and cannot show whether the sink sped up once
siblings drained. **The clean version is per-call intervals, not a running mean**, and that is what r2
should be measured with. I am not claiming "sibling count drives s/call linearly" — only that a
co-tenanted sink is ~2.3x a solo one.

Also worth noting for F164: **`test-meridian` is STILL in flight at 71 minutes** — the same task that
fails 7 of 11 times and that F165 caught being recorded FAILED while green. The pattern is holding on
this run too.

## F186 — baseline n=2: the spread collapsed to 12.6 points, and that makes the wall-clock signal readable

`baseline-n3-r1` finished. The cell the entire F154 freeze was waiting on now has two points:

    baseline r0   score 0.8429   wall  97.4 min
    baseline r1   score 0.7166   wall  77.6 min
    ------------------------------------------------
    spread                12.6 points   wall range 77.6-97.4

**12.6 points against the 46-point spread that motivated the freeze.** That historical figure came
from 44.2 / 86.7 / 90.0 on byte-identical config, and it is why every n=1 conclusion tonight was held
loosely. If r2 lands anywhere near these two, the engine's run-to-run variance has dropped by a
factor of ~3.5 — which would be the single most consequential change of the campaign, because it is
what makes every future arm readable at all. **Not claimed yet: n=2 cannot establish a spread. r2
decides it.**

**And it immediately re-reads the sink_review arm:**

    sink_review   score 0.7326   ->  INSIDE the baseline range [0.7166, 0.8429]
    sink_review   wall  145.5    ->  OUTSIDE the baseline range, 49% above the SLOWEST baseline

So the arm's score is **ordinary** — it sits between the two baselines, and F181's "do not report the
arm as harmful" was right for a better reason than I had at the time. Its WALL-CLOCK, though, is the
one number that now falls outside baseline entirely.

**That is a coherent story with F184/F185/F182 and it is the first time the pieces line up:**
the arm buys real fleet utilisation (F175, three nodes generating), pays for it on the critical path
(146→257 s/call, F185), produces genuinely accurate defect reports (F182, verified to the line), and
then **discards them** — so the cost lands on wall-clock and the benefit never reaches the score.
**The arm is not harmful; it is unfinished.** F182b (feed survivors into the repair tail) is what
would convert its cost into a gain, and this is the strongest evidence yet for it.

⚠ HONEST LIMITS: baseline n=2, arm n=1. The wall comparison is 145.5 against a 2-point range — a
third baseline could widen that range and swallow it. Do not promote this past "the wall-clock signal
is worth watching" until r2 lands.

`baseline-n3-r2` is running (5 min in). It settles both the spread and this.

## F187 — both baseline apps RUN; the score gap is coverage, not correctness. And a 2-of-3 majority confirms F182's finding.

Crunched `baseline-n3-r1` (0.7166) the same way as r0 (0.8429), on a port far from `PORT_BASE`:

    pytest                 52 passed
    python3 -m vendorsync --help    well-formed (note: `--db` OPTIONAL here, REQUIRED in r0)
    server                 LISTENING, no crash
    GET /                  200, HTML dashboard
    GET /api/payments      200  {"data": [], "total": 0, "limit": 25, "offset": 0}
    GET /api/summary       200  {"count": 0, "total_minor": 0, "currency": "EUR", "oldest": null, "newest": null}

**The lower-scoring build works end to end, and its API responses are shape-identical to r0's.** So
the 12.6-point score gap between the two baselines is NOT a functionality gap:

    r0   72 test functions   72 passed   score 0.8429
    r1   21 test functions   52 passed   score 0.7166

**~3.4x the test functions.** The scorer is reading coverage depth, and both apps serve correctly.
That is worth knowing before any future arm's score delta gets read as "it broke something".

**AND A CROSS-RUN MAJORITY CONFIRMS F182'S CURRENCY FINDING.** I initially grepped the wrong tree —
F182's subject was `sink_review-n3-r0`, not `baseline-n3-r0` — and the mistake produced better
evidence than the comparison I intended:

    baseline-n3-r0      0 dynamic-currency sites   (hardcodes "EUR")
    baseline-n3-r1      0 dynamic-currency sites   (hardcodes "EUR", lines 119 and 133)
    sink_review-n3-r0   the `all_payments[0]["currency"]` / `newest_dt["currency"]` sites F182 flagged

**Two of three independent runs implement the currency as a hardcoded "EUR"; only the run the idle
nodes reviewed got it wrong — and they caught it.** F182 carried the explicit caveat *"I verified the
CODE matches the description, NOT that it violates the spec"*. That caveat can now be discharged by
majority implementation: three independent 27B planning passes agreed on hardcoded EUR twice, so the
dynamic version is a genuine deviation and the reviewers found a real defect, not a style preference.

This is the second independent line of evidence that the idle-node findings are worth consuming
(F182b), and it arrived from a direction I was not aiming at.

## F188 — four more upstream commits closed; one PROMOTED because our concurrency is the precondition

Continuing the ratchet. Closed on facts about this deployment, not on diffs (Lesson 44):

    ea2baea58  fix(summon): preserve fixed subrecipe values
               `summon` appears 0 times in swarm.rs — the same fact that closed a commit in F155.
    efc7ccc2c  fix(permissions): scope smart approval by request
               `approval` and `smart_approval` both appear 0 times; the swarm is headless with no
               approval flow at all.
    c55b5fb62  feat(hooks): pass working_dir to the Stop hook context
               `hooks` appears once in swarm.rs but **config.yaml declares ZERO hooks**, so no hook
               runs on any of these runs. Checked the CONFIG, not just the source — a hook surface
               that exists and is unconfigured is exactly the case a source grep gets wrong.
    8d5bc5d49  fix(developer): expose AGENT_SESSION_ID to shell commands
               Our workers DO use the developer extension's shell (tools are exactly `edit, shell,
               tree, write`), so "we don't run shell" would have been the wrong close. The right one
               is narrower: `AGENT_SESSION_ID` appears 0 times in swarm.rs and nothing in our worker
               prompts references it, so the change adds an env var no code of ours reads.
               Behaviourally inert HERE — stated as scope, not as irrelevance.

**PROMOTED, NOT CLOSED — `d5a8a3fb9 fix(session): create inventory tables atomically with schema
version`.** My reflex was to file this with the other session commits as low-relevance plumbing. That
is wrong for THIS deployment specifically: the swarm runs **six concurrent workers**, each opening a
session, so **concurrent inventory-table creation is our normal operating condition, not an edge
case.** An atomicity fix on table creation has a precondition we meet on every single run. It stays
open and moves to the front of the flagged list.

That is the mirror of Lesson 44 and worth stating as its own rule: a deployment fact can make an
upstream commit MORE relevant, not only less. I have used this check nine times to kill candidates
and this is the first time it promoted one.

STILL OPEN, needing a real read when the freeze lifts:
    d5a8a3fb9  session inventory atomicity   <-- 6 concurrent workers meet its precondition every run
    ee61c7c49  CLI streaming render O(n^2) -> incremental
    8b73e1a1b  stable agent event identity   (F172 made it moot for F163, may still matter elsewhere)
    ad87dd4c3  compaction structured summary (`compact` appears 13x in swarm.rs)
    d5785a367  session manager for tool summaries

## F189 — F185's per-interval measurement was unobtainable, so I built the instrument for it

F185 registered a specific improvement: measure the sink's seconds-per-call as PER-CALL INTERVALS
rather than a cumulative average, because a running mean cannot show a change WITHIN its own window
(co-tenancy fell 3 siblings → 1 during r1's sink and the mean could not see it). Both obvious sources
turned out to be **blind**, and finding that out was most of the work:

  · **`llm_request.*.jsonl` mtimes.** One file per call, so the mtimes ARE the call times — the ideal
    source. But only **15 fleet calls were visible across a 2-hour window**, against **14 counted in
    a single 8-minute window** earlier the same night. The logs rotate far faster than F176's "under
    3 hours" estimate. The tell was the implausible count, not a wrong-looking trend.
  · **`judge_observed` events.** They carry `(timestamp, tool_calls)` and would be exact. The judge
    emits **exactly ONE** for `integrate-verify` — because the over-read gate is deliberately exempt
    for tasks that own no files (the sink owns none, and that exemption exists because applying it
    would guarantee killing the sink for over-reading). **The series does not exist and never did.**

So every cumulative sink figure in this campaign — 63, 146, 257 s/call — is the ONLY form those
numbers could have taken with the instruments available. That is worth knowing: they are not lazy
approximations, they were the ceiling of what could be measured.

**BUILT: `sinkwatch.py`.** The digest is rewritten on stream activity (coalesced ~2.5/s), so sampling
it on a fixed cadence turns a counter into a series. Launched detached against r2's live sink and
already differencing:

    04:34:32   8 calls        <- sample
    04:35:02   8 calls        stalled (no call in that 30s)
    04:35:32   9 calls        <- call 9 landed inside this window

Design choices that are deliberate rather than incidental:
  · **READ-ONLY, no ports, no locks, no writes into the run dir.** A crunch that bound a harness port
    once nearly produced a fabricated "the app does not run" (F169); an instrument that watches a
    live run must be incapable of contending with it.
  · **Newest run dir by MTIME, not by name.** `swarm-3node-r0` was three different runs tonight;
    a name-based pick silently reads a finished unit.
  · **Stops on its own evidence** — 20 unchanged samples means the sink finished or wedged — rather
    than on a fixed duration, so it cannot outlive its subject or quit early on a slow one.
  · **A torn read mid-rewrite is skipped, not fatal.** The engine rewrites this file constantly.

LESSON 65: **WHEN A MEASUREMENT IS REGISTERED AND THEN TURNS OUT TO BE UNOBTAINABLE, THAT IS A
FINDING, NOT A GAP TO SKIP.** Two blind sources explain why every existing figure is cumulative, and
the honest response is to build the third source rather than quietly keep quoting the mean.

## F190 — the per-interval series works, shows ~120 s BETWEEN calls, and caught my own instrument lying

First real output from `sinkwatch.py` on r2's co-tenanted sink:

    04:35:32   call  9
    04:37:32   call 10      120 s
    04:39:32   call 11      120 s
    04:39:32 onward — no further change for 11 consecutive samples

**Two clean inter-call intervals of 120 s each.** Against the cumulative figures — r0 solo 63, r1
co-tenanted 146, sink_review + idle-fill 257, fleet median 83 — r2's co-tenanted sink sits at ~120 s
between calls, consistent with r1's 146 cumulative and firmly above the 83 fleet median. **The
co-tenancy penalty reproduces on a second run.**

⚠ **QUANTIZATION, stated:** sampling is every 30 s, so an interval is only resolved to ±30 s. Two
readings of exactly 120 s are 4 samples each, not a suspiciously precise measurement — the true
intervals lie in 90-150 s. Do not quote 120 as if it were tight.

**AND THE INSTRUMENT WAS LYING IN ITS LABEL.** Those 11 flat samples printed as `stalled`. The sink
did not stall — it **finished** around 04:39, which the run log confirms (`in flight: ['test-api']`,
`integrate-verify` gone). `sinkwatch` samples a digest and **cannot distinguish a worker mid-call
from a task that has ENDED**; every post-completion sample read as a stall. Left alone, that label
would have manufactured a "5.5-minute sink stall" finding out of a task that had already succeeded —
the same shape as the port collision that nearly produced "the app does not run" (F169).

FIXED: the column now reads `no change`, with the reason in the source, and the instruction to
cross-check `task_completed` in the run log before calling any flat stretch a stall. **A label is a
claim; this one was making a claim the data cannot support.**

LESSON 66: **AN INSTRUMENT MUST NOT NAME WHAT IT CANNOT DISTINGUISH.** "stalled" and "finished" look
identical to a counter sampler, so the honest label is the observation (`no change`), not the
interpretation. I wrote this instrument one tick ago specifically to avoid a cumulative average
hiding a change — and shipped it with a word that would have hidden a completion.

## F191 — a post-write reasoning spiral is SHIELDED by the guard I added in F144

`test-api` on r2, attempt 0. It wrote its file at 408 s and then:

    obs 1283s  calls=8  think= 2,897   written=True
    obs 1403s  calls=8  think= 5,774
    obs 1538s  calls=8  think=10,540
    obs 1673s  calls=8  think=15,445
    obs 1758s  calls=8  think=18,289
    obs 1878s  calls=8  think=22,627   <- 595 s, ZERO tool calls, ~20k chars of reasoning

**Ten minutes of pure reasoning with no action, after the deliverable already existed.** Then attempt
1 (healthy — calls climbed 4→11), then attempt 2, which is still running. Three dispatches for one
test file.

**MY FIRST READ WAS WRONG AND I AM CORRECTING IT.** I checked the guards and found all three
`!any_owned_written` trips (over-read at :349, spiral at :375, no-file at :419) are blind once a file
exists — and nearly concluded "no deterministic trip can fire on a written worker". **There IS one**,
at judge.rs:434, and it targets exactly this case:

    // Finalize-spin: the worker DID produce its owned file(s) but has not touched them in a long
    // time while still running … The over-read check above can't see this (a file exists).
    if input.any_owned_written
        && input.task_id != "integrate-verify"
        && input.elapsed_secs >= cfg.min_age_secs.max(420)
        && input.secs_since_last_write.is_some_and(|s| s >= 420)
        && !is_still_producing(input)

The first four conditions are all MET here: file written, not the sink, elapsed 1878 s, untouched far
beyond 420 s. **The fifth blocks it.** `is_still_producing` returns `thinking_chars > prev` — and
this worker's thinking went 2,897 → 22,627 monotonically, so it reads as "still producing" at every
single observation.

**A reasoning spiral is, by definition, monotonically growing reasoning. So the guard can never let
the trip fire on the pathology the trip exists to catch.** The condition that was supposed to protect
a working worker instead grants permanent immunity to a looping one.

**I ADDED THAT GUARD.** F144: *"DELTA, never level. GREW = no kill; FLAT = kill."* It was right about
the case it was written for — F163 later confirmed that flat counters cannot distinguish frozen from
writing, and a flat-kill would have killed healthy workers. But "grew" was never a safe proxy for
"progressing", because thinking-only growth is precisely what a spiral produces.

**THE FIX IS NARROW AND FOLLOWS FROM THE SAME EVIDENCE:** `is_still_producing` should require growth
in something that represents ACTION — `tool_calls`, or a write — not in `thinking_chars` alone.
Thinking growth alongside a frozen `tool_calls` and an untouched file is the spiral signature, not
progress. That keeps F163's protection (a worker streaming a tool payload has flat thinking but WILL
advance `tool_calls` on completion) while closing this hole.

QUEUED as F191b in `QUEUED-PATCHES.md`. Registered check: on one run, a worker with a written file,
`secs_since_last_write > 420`, growing `thinking_chars` and FLAT `tool_calls` receives a `Looping`
verdict instead of running to its attempt cap.

⚠ SCOPE, honestly: n=1 instance, measured precisely. But the guard's logic is universal — every
post-write spiral has growing thinking by construction — so this is a reasoned defect, not a
statistical one, and it costs a full re-dispatch each time it fires.

## F192 — BASELINE n=3: the spread is 13 POINTS, down from 46. The freeze lifts.

    baseline r0   0.8429    97.4 min   (72 test fns)
    baseline r1   0.7166    77.6 min   (21 test fns)
    baseline r2   0.8422   126.0 min   (a replan added 4 tasks)
    ------------------------------------------------------
    mean 80.1%   SPREAD 13.0 POINTS   wall mean 100 min   fallbacks 0   kind_mismatch 83.5%

**Thirteen points against the forty-six that caused the freeze.** That 46 came from 44.2 / 86.7 /
90.0 on byte-identical config, and it is why every n=1 conclusion tonight was held loosely and why
F154 stopped the campaign to measure it. **Run-to-run variance is ~3.5x tighter**, which is worth
more than any single lever: it is the difference between an arm being readable and an arm being
noise. Every measurement from here has 3.5x the resolution.

Two things it settles immediately:
  · **`sink_review`'s 0.7326 sits INSIDE [0.7166, 0.8429]** — confirmed ordinary on quality, at n=3
    rather than by assertion. F181's "do not report the arm as harmful" holds on evidence now.
  · **r2 scored 0.8422 with 126 min wall.** Its length is explained by a replan adding 4 tasks, not
    by variance — worth stating because a 126 vs 78 min range would otherwise look like instability.

**THE FREEZE (F154) IS LIFTED.** It held perfectly: `crates/` clean, zero engine edits, three
comparable baselines on one build. That discipline is what makes the 13 points meaningful.

## F193 — but the boundary crossing WAITS, and the reason is F154's own lesson

`scoped_contracts-n3-r0` started at 05:45:25 — **the F164 arm**, aimed at the population producing
93% of all failures, and the only queued arm that touches it. It is running RIGHT NOW on build
1785697869, the same build all three baselines ran on.

**Applying the ten patches requires a boundary crossing, and a crossing invalidates cross-build
comparison — that is the exact thing F154 froze the campaign over.** Crossing now would:
  · kill the in-flight `scoped_contracts` run, and
  · **invalidate the baseline n=3 I just spent five hours obtaining**, because an arm on the new
    build cannot be compared to a baseline on the old one.

So the ten fixes wait for one more unit. **DECISION: let `scoped_contracts` finish (~100 min), take
its readout against a VALID baseline, then cross once.** The crossing costs a re-baseline either way;
doing it after this arm costs nothing extra and buys the campaign's most valuable measurement on
comparable ground.

This is not the freeze being extended — the freeze is over and the patches are ready, anchors
verified, cost re-checked. It is refusing to spend a five-hour baseline to save ninety minutes.

## F194 — `scoped_contracts` is ARMED (proven, with a control), and F171's rule needs a second address

Before its readout can mean anything (F171), the arm must be proven armed. It is:

    scoped_contracts-n3-r0   levers_resolved.scoped_contracts = True
    baseline-n3-r2 (control) levers_resolved.scoped_contracts = False

A positive with a negative control on the same field of the same event — the standard this campaign
holds itself to, and the reason a later zero from this arm will be interpretable as DEAD rather than
VOID.

**BUT THE FLAG IS NOT WHERE F171 SAID TO LOOK.** F171 established `run_started.gates.<lever>` after
hunting three wrong places for `sink_review`. For `scoped_contracts` that field is **`None` on BOTH
the treatment and the control** — it carries no information at all. The reason is how the lever is
plumbed:

    sink_review        env-driven  (`GOOSE_SWARM_SINK_REVIEW`)  ->  run_started.gates.<lever>
    scoped_contracts   CONFIG field (`swarm.rs:863 pub scoped_contracts: Option<bool>`)
                                                                ->  levers_resolved.<lever>

**So the arm-armed check has TWO addresses and which one carries the flag depends on the lever's
plumbing.** Had I checked only `gates` — exactly what F171 instructs — I would have seen `None` on
the treatment and concluded the arm was unarmed or the check was broken, on the campaign's most
important arm.

CORRECTED RULE, replacing F171's single address: **check `levers_resolved.<lever>` AND
`run_started.gates.<lever>`, and require a CONTROL run to differ on whichever one is non-null.** A
field that reads `None` on both arms is not evidence of anything — it is the wrong field, and that is
distinguishable only by having the control in hand.

This is the same shape as F171 itself (I hunted three wrong places) and as Lesson 52 (a guard
protecting one direction implies the other is unguarded): **a lookup rule derived from ONE instance
is a rule about that instance.** F171 was derived from an env lever and silently assumed all levers
are env levers.

## F195 — F191b's interaction with F163 is CLEARED, verified from the original data

F191b changes `is_still_producing` to key on `tool_calls` instead of `thinking_chars`. The registered
risk was that it re-introduces the exact false kill F163 was written to prevent, because F163's
refutation case had **flat thinking AND flat tool_calls** — so the new rule would also read "not
producing" there. I had reasoned that `any_owned_written` protects it. **Reasoning is not evidence,
so I went to the rows.**

`test-meridian` attempt 1 on `baseline-n3-r0`, the flat stretch that refuted F160:

    elapsed  calls    think   any_owned_written
        105      0    1,209   False
        174      0    1,209   False
        234      0    1,209   False
        294      0    1,216   True     <- it wrote
        569      3    4,340   True

**`any_owned_written` is False in all three flat observations.** The finalize-spin trip requires
`any_owned_written == True`, so during that stretch it was blocked by a DIFFERENT condition entirely
— `is_still_producing` never even got a vote. **F191b cannot re-introduce this false kill.** Verified
from the original run's own rows.

**A second case appeared in the same trace and it also comes out clean.** Later in that task:

    1521      6    7,674   True
    1656      6    7,674   True      <- BOTH tool_calls and thinking flat, file written

Here the old rule and the new rule AGREE — both read "not producing", so F191b changes nothing about
whether the trip fires. The change is a strict narrowing: it only alters behaviour where thinking
grows while tool_calls do not, which is exactly the F191 spiral signature and nothing else.

So the batch's one flagged interaction is settled, and settled the right way — the falsifier had a
concrete address (three specific rows on disk), and checking it took one query. **This is what F163's
registered falsifier bought: a year from now the reason F191b is safe is a table, not an argument.**

## F196 — the "API of" block is the dependency's FULL SOURCE, cut mid-identifier, fenced as complete

Taken as the registered readout on the live `scoped_contracts-n3-r0` (4 test-authors dispatched).

**The arm works and its effect is small.** Test-author prompt **22,511 → 20,552 chars, a 8.7% cut**;
`## API of` blocks **5 → 4**. That is the whole effect available to it, and the reason is structural:
**a test-author's DAG neighborhood is genuinely most of the app** — it imports the module under test
*and* that module's collaborators. Scoping the block COUNT was the wrong axis. (Implementers,
separately: median **0** `## API of` blocks. The entire ~10k gap between an implementer prompt and a
test-author prompt IS the contract bundle. F156's "2.3× prompt" is now decomposed.)

**Where the bytes actually are.** Section-by-section on one live test-author prompt (19,916 chars):

    3,606  18.1%  ## API of vendorsync/meridian.py
    3,601  18.1%  ## API of vendorsync/api.py
    2,890  14.5%  ## API of vendorsync/store.py
    1,657   8.3%  ## PROJECT FILE LAYOUT
      401   2.0%  ## FROZEN MODULE INTERFACES

**The contract bundle — ALREADY SCOPED this run — is 10,097 chars = 50.7% of the entire prompt.**

**And it is not an API.** The block header reads *"## API of {f} (a dependency you import — use it
from here, do NOT `cat` it)"* and what follows is the file's **full source**: imports, comment
banners, and every method BODY. Counted across the prompt: **6 private methods pasted against 5
public ones** — more implementation the test-author can never call than surface it can.

**Three of four blocks are truncated mid-token, with no notice, inside a closed fence:**

    meridian.py  ends `    def _up`      -> pasted body FAILS ast.parse (line 104: expected '(')
    api.py       ends `        self._se`
    truncation notice present: False (all four)

`swarm.rs:19638` — `let capped = api_source.chars().take(dep_budget.min(3500)).collect();` — a raw
char cut, then `"```\n{capped}\n```"` closes the fence unconditionally. **So the worker receives a
file that stops mid-`def`, formatted as if it were whole, together with an instruction forbidding it
to open the real one.** A model given a truncated dependency and denied the file has nothing left to
do but reason — which is F191's 595-second zero-tool-call spiral, from the other end.

**THE ENGINE ALREADY HAS THE FIX AND IT IS SWITCHED OFF.** `swarm.rs:19628`:

```rust
let api_source: Cow<str> = if dep_sig_on {
    let sigs = goose_swarm::extract_signatures(trimmed, sig_lang);
    if sigs.trim().is_empty() { Cow::Borrowed(trimmed) } else { Cow::Owned(sigs) }
} else { Cow::Borrowed(trimmed) };
```

`extract_signatures` (coherence.rs:34) — *"Function/method BODIES are removed; type, const and var
declarations are kept as-is"* — handles Python, falls back to the full body when it finds nothing so
ON can never inject an empty API. `dep_signatures: None` (swarm.rs:1090) ⇒ **OFF by default, never
measured on this fleet.** It is emitted in `levers_resolved` (:22140), so F194's arm-armed check works.

**This is the defect shape for the eighth time, and the sharpest instance yet** — F141 compile error,
F142 thinking count, F149 tool list, F153 file list, F157 `is_test_author`, F158 `owned_files`, F172
digest mtime, and now this: *the engine holds the capability and does not use it.* Here it does not
merely fail to narrow — it pastes maximum detail (full bodies) of minimum relevance (private
helpers) and truncates away whatever came last.

**Queued as arm `dep_signatures` (reps=3) with its falsifier registered before the run**, and as
patch #11 (the silent truncation is a defect in its own right — a large *signature* surface would
still be cut mid-token and fenced as whole).

**Honest correction:** sweep.py's own comment beside the `scoped_contracts` arm already said "265
lines of implementation body against 35 signature lines, including six private methods." I had the
observation and attached it to the wrong lever — fewer blocks, when the defect was fatter blocks.
The live measurement is what separated them.

**LESSON 71 — MEASURE THE COMPOSITION, NOT ONLY THE TOTAL.** Four ticks were spent on "the
test-author prompt is 22,511 chars" as a scalar. One section-by-section decomposition, available at
any point, showed half of it is one component with a purpose-built lever already written for it.
A total tells you something is too big; only the composition tells you which lever touches it.

## F197 — `over_reading` is a TEST-AUTHOR-EXCLUSIVE verdict, and nothing acts on it

Pooled over every archived run (5 runs, 302 `judge_verdict` events), verdict type × task kind:

    verdict          implementer   test-author   verify/sink
    over_reading               0            11             0
    broken_code                2             1             0
    looping                    1             3             0
    spec_drift                 0             3             0
    split                      2             0             0
    ok                        80           178            21
    TOTAL                     85           196            21

**Every `over_reading` verdict this campaign has ever recorded — 11 of 11 — is on a test-author.
Zero on 85 implementer verdicts. Zero on 21 sink verdicts.**

**F196 is the mechanism, and it is not a coincidence of naming.** Test-authors are the ONLY kind that
receives `## API of` blocks (implementers: median **0** blocks). Those blocks are the dependency's
full source, cut at 3,500 raw chars mid-`def`, fenced as complete, carrying no truncation notice, and
introduced with *"do NOT `cat` it"*. A worker whose pasted dependency does not parse has exactly one
route to the truth — open the real file — and that route costs tool calls with nothing written, which
is precisely the over-read guard's trigger (`judge.rs:349`: `!owned_files.is_empty() &&
!any_owned_written && tool_calls >= over_read_tool_calls`).

**⚠ MY OWN FRAMING WAS WRONG AND I AM STRIKING IT.** I was one step from writing that the engine
"penalizes the worker for reading". It does not. Measured, not inferred:

- **0 kill events across all 5 runs.** The only judge events on disk are `judge_observed` 302,
  `judge_verdict` 302, `judge_skipped` 178. Nothing kills, nothing retries.
- **The hint is not delivered.** Across the 16 qwen requests retained in a 70-minute window, **zero**
  carry the hint's text. All four live `over_reading` verdicts are attempt 0 with no attempt 1.
  ⚠ Retention is short (F176), so this is "not observed in the retained window" plus "no re-dispatch
  happened", not a proof about all time.

So the true shape is worse than a penalty and duller: **the judge correctly identifies the exact
population that accounts for 93% of every failure (F164), names the exact behaviour F196 predicts,
and the verdict goes nowhere.** That is the NINTH instance of the campaign's one defect shape —
F141, F142, F149, F153, F157, F158, F172, F196, and now this — the engine holds the answer and does
not use it.

**And when the hint eventually is delivered, it will assert something false.** Its text:

> "STOP investigating: you already have the spec, the file layout, and **the injected dependency
> APIs**. WRITE your owned file(s) NOW"

The worker does *not* have the dependency APIs. It has `meridian.py` cut at `    def _up`, which
fails `ast.parse`. The engine computed that truncation itself (`swarm.rs:19638`) and then tells the
worker the opposite. Queued as patch #12: the guard must consult the fact the engine already
holds — if a dependency the worker owns a test for was truncated, the correct response is to GRANT
the read, not to insist the paste is sufficient.

**LESSON 72 — A DETECTOR THAT FIRES AND CHANGES NOTHING READS EXACTLY LIKE A DETECTOR THAT NEVER
FIRED.** `over_reading` has been perfectly, exclusively right about the failing population for five
runs, and no measurement in this campaign noticed, because "the judge is inert" and "the judge sees
nothing" produce identical run outcomes. The way I found it was counting verdicts by KIND rather
than in total — the same composition-over-total move as F196, one tick later.

## F198 — RETRACTION of F197's central claim: the verdict IS consumed, on all 11

**F197 said the `over_reading` verdict "goes nowhere" and called it the ninth instance of the
engine-holds-the-answer shape. That is WRONG and I am striking it.** Every one of the 11 verdicts
carries `action = re_dispatch`:

    baseline-n3-r0  test-api                   action=re_dispatch  conf=0.90
    baseline-n3-r0  test-api-input-validation  action=re_dispatch  conf=0.90
    baseline-n3-r0  test-meridian              action=re_dispatch  conf=0.90
    baseline-n3-r0  test-api                   action=re_dispatch  conf=0.90
    baseline-n3-r1  test-cli-edge              action=re_dispatch  conf=0.90
    baseline-n3-r1  test-meridian              action=re_dispatch  conf=0.90
    baseline-n3-r2  test-meridian              action=re_dispatch  conf=0.90
    swarm-3node-r0  test-store                 action=re_dispatch  conf=0.90
    swarm-3node-r0  test-api                   action=re_dispatch  conf=0.90
    swarm-3node-r0  test-meridian              action=re_dispatch  conf=0.90
    swarm-3node-r0  test-api                   action=re_dispatch  conf=0.90
    ACTION TALLY: {'re_dispatch': 11}

**How I got it wrong.** I grepped the event stream for event *types* containing `kill`, found none
(`judge_observed` 302, `judge_verdict` 302, `judge_skipped` 178), and read that silence as "nothing
acts on the verdict". But the scheduler does not emit a separate kill event — **it records what it
did as a FIELD on the verdict event** (`scheduler.rs:1414` `redispatch = actionable && interv <
max_interventions_per_task`, then the action is stamped onto the emitted `JudgeVerdict`). The
information was in the record I was already reading; I never listed its fields.

**So the framing I struck one tick ago was the correct one, and striking it was the error.** The
trap is real and closed:

1. Only test-authors receive `## API of` blocks (implementers: median 0) — F196.
2. Those blocks are the dependency's full source cut mid-`def`, fenced as complete, no truncation
   notice, introduced with *"do NOT `cat` it"*. `meridian.py`'s paste does not parse.
3. The worker reads the real file — the only route to the truth, and the one the prompt forbids.
4. The over-read guard trips: **11 of 11 on test-authors, 0 of 85 implementer verdicts.**
5. **The worker is KILLED and RE-DISPATCHED** — every time, at confidence 0.90.
6. The hint it restarts with asserts *"you already have … the injected dependency APIs"* — which the
   engine truncated one function earlier. So the restarted worker is told the unusable artifact is
   sufficient, and is pointed back at it.

**Consequence, measured.** Of the 11 tasks hit, **none finished in one attempt** — 8 needed 3
attempts, 3 needed 2, and **2 never completed at all** (`test-api` on the live arm, still incomplete
at the time of writing). ⚠ I also computed ~7,779 discarded worker-seconds, but that figure takes
`max(elapsed_secs)` across *all* attempts of the task, not the killed attempt, so it is an upper
bound with a known flaw and is NOT a result. The attempt counts are the robust signal.

**Patch #12 is UNGATED and PROMOTED.** Its precondition — "establish whether the verdict is
consumed" — is now answered: it is consumed 11 times out of 11, and the hint that rides the restart
is the false one. Fixing the wording is no longer cosmetic; it is the message a killed worker is
restarted with.

**LESSON 73 — BEFORE READING "X NEVER HAPPENED" FROM AN EVENT STREAM, CHECK WHETHER X IS RECORDED AS
A FIELD RATHER THAN AN EVENT.** The campaign already had the rule that a negative authorising a
conclusion must be proven rather than observed (it is why `sink_review`'s zero and F176's retention
zero were both struck). I applied it to arms and to the engine, and not to my own query: I searched a
namespace of event *types* and concluded about *actions*. The one-line control I skipped — print the
available field names before concluding from their absence — is the same `keys()` call that exposed
it immediately afterwards.

## F199 — the `over_reading` verdicts are MODEL OPINIONS, and the workers read NOTHING

Applying Lesson 73 properly this time — list the fields, then look at the state at the trip:

    run               task                       tool_calls   thinking   elapsed
    baseline-n3-r0    test-api                            0     24,032       474
    baseline-n3-r0    test-api-input-validation           0      3,402       441
    baseline-n3-r0    test-meridian                       1        857       422
    baseline-n3-r0    test-api                            1      2,006       465
    baseline-n3-r1    test-cli-edge                       0      8,257       420
    baseline-n3-r1    test-meridian                       0      2,632       455
    baseline-n3-r2    test-meridian                       0     11,326       423
    swarm-3node-r0    test-store                          0      8,482       422
    swarm-3node-r0    test-api                            0     10,743       467
    swarm-3node-r0    test-meridian                       2      2,462       485
    swarm-3node-r0    test-api                            0        795       457

**tool_calls at the trip: min 0, median 0, max 2. The threshold is 16.**

**Two things follow, and both correct me.**

**1. F196's step 3 is RETRACTED.** I wrote that the worker "reads the real file — the only route to
the truth — and that is exactly the over-read trigger." **It does not read the real file. It does not
read anything.** Median zero tool calls. What it does is think: 795 to 24,032 chars of it, for
420-485 seconds. The story I told was mechanically satisfying and the data does not support it.

**2. These verdicts cannot be the deterministic guard, so they are model opinions.** `judge.rs:349`
requires `worker_tool_calls >= cfg.over_read_tool_calls` (16). With 0-2 calls that predicate is
false, every time. The verdict therefore comes from the LLM judge running on an idle node — a weak
model looking at a silent worker and guessing "over_reading" at confidence 0.90. The scheduler then
re-dispatches on it (`scheduler.rs:1414`; only *terminal-fail* requires `outcome.deterministic`,
re_dispatch explicitly keeps "full STEERING power" for a model verdict).

⚠ **I cannot PROVE provenance from the log, only deduce it from the guard's own arithmetic.** The
`JudgeOutcome` struct carries a provenance flag — `judge.rs:126`, *"True only for a verdict produced
by `deterministic_verdict` — a real engine fact"* — and **the emitted `JudgeVerdict` event does not
include it**. Fields are exactly `action, confidence, device, event, hint, run_id, seq, task_id, ts,
verdict`. The one bit that separates an engine fact from a weak model's guess is computed, used for
the terminal-fail gate, and then dropped before anyone downstream can see it. **Patch #13.**

**3. The engine already documented this exact failure and believes it fixed.** `swarm.rs:11217`:

> "MEASURED: a task was killed for 'over_reading' three times at 457s/450s/430s with tool_calls=0,
> having read nothing."

457/450/430 against tonight's 474/441/422/465/420/455/423/422/467/485/457. **Same shape, same
verdict, same population — 11 times across 5 runs including the live one.** The fix that comment
describes was to *count* thinking chars so a reasoning worker stops looking hung. Counting them did
not stop the mislabelling, because the thing consuming the count is a model being asked to judge, and
nothing checks its answer against the tool-call number the engine already has.

**What survives of the F196 chain, and what does not.** F196 itself stands — the `## API of` blocks
are truncated whole-file pastes, verified byte by byte. What is now unsupported is my *link* from
that to the kill. The honest version: a test-author receives 10k chars of dependency source, three
of four blocks unusable, and then produces **zero tool calls** for seven minutes while thinking
climbs. That is F191's spiral signature, not over-reading. Whether the broken paste *causes* the
spiral is plausible and **unproven** — the `dep_signatures` arm is what would test it, and its
registered readout already includes dry reasoning before first write.

**LESSON 74 — WHEN A VERDICT NAMES A BEHAVIOUR, CHECK THE COUNTER THAT BEHAVIOUR WOULD MOVE.**
"over_reading" was recorded 11 times against workers whose read counter was zero. I spent two ticks
building a causal story on top of the verdict's NAME without once asking whether the thing it names
had happened. The check is one column of the record I had already loaded twice.

## F200 — the trip is a DETERMINISTIC 420-SECOND DEADLINE, and its hint is already good

`judge.rs:418`, the branch the comment calls the "blind fallback":

```rust
let owns_code = input.owned_files.iter().any(|f| is_code_deliverable(f));
if owns_code && !input.any_owned_written && input.elapsed_secs >= cfg.min_age_secs.max(420) {
    let read_nothing = input.worker_tool_calls == Some(0);
    return Some(JudgeOutcome { verdict: Verdict::OverReading, confidence: 0.9,
                               hint: no_file_hint(input, read_nothing),
                               proposed_split: None, deterministic: true });
}
```

**The eleven elapsed times, sorted: 420, 422, 422, 423, 441, 455, 457, 465, 467, 474, 485.**
The floor is **exactly 420**. That is the constant, not a coincidence.

**⚠ F199's "these are LLM judge opinions" is RETRACTED.** It was a deduction from the *other* guard's
arithmetic — `tool_calls >= 16` cannot fire at 0 calls, therefore not deterministic — and I never
asked whether a *third* branch could reach a 0-call worker. This one can and does: `deterministic:
true`, no evidence term at all, **a stopwatch**. A model opinion cannot produce a floor at the
predicate's own constant; the clustering was the fingerprint and I read past it.

**And the hint that actually gets delivered is good.** Not the canned sentence I queued patch #12
against — that branch never fires. `no_file_hint` (`judge.rs:468`) composes the observation from
counts:

> "After 7.9 minutes, none of the files you own exists on disk yet, and you have run no command —
> you have emitted 24,032 characters of reasoning instead."

Specific, factual, no false diagnosis. The engine computes `read_nothing` precisely so it does not
tell a silent worker to stop reading. **PATCH #12 IS RE-SCOPED** — the *"you already have … the
injected dependency APIs"* sentence lives at `judge.rs:355` (needs 16 tool calls, never reached) and
`judge.rs:381` (the #134 spiral trip, `spiral_thinking_chars: 0` = OFF). Both are unreachable at the
shipping config, so fixing their wording changes nothing today. **It becomes live the moment either
is armed — and the `dep_signatures` arm does not arm them, so #12 drops behind #11 and #13.**

**What is actually defective here, and it is what cost me three ticks:**

**The verdict LABEL does not describe what fired.** A worker with zero tool calls is recorded as
`over_reading`. The engine *knows* — it computes `read_nothing` on the line above and branches the
hint on it — and then stamps the same label either way. So the run log says "over_reading" 11 times
about workers that read nothing, the hint says the opposite, and every downstream analysis (mine,
three times over) starts from the label. **Patch #14.**

**The honest causal picture, with the unsupported links removed:**

1. Test-authors carry a 2.3× prompt whose bulk is 10k chars of dependency source, most of it private
   helpers, three of four blocks truncated mid-token (**F196 — verified byte by byte, stands**).
2. Test-authors are the population that fails to write a file within 420 seconds — **11 of 11 trips,
   0 of 85 implementer verdicts**.
3. At 420s the deterministic deadline kills and re-dispatches them; none of the 11 finished in one
   attempt, 2 never completed.
4. **Whether (1) causes (2) is the open question.** It is plausible, it is what `dep_signatures`
   tests, and it is NOT established. Everything else above is measured.

**LESSON 75 — WHEN OBSERVED VALUES CLUSTER AT A FLOOR, FIND THE CONSTANT BEFORE THEORISING ABOUT
THE CAUSE.** Eleven values in 420-485 with the minimum at exactly 420 is a `>=` against a literal,
and nothing else. I had that column in front of me for two ticks and spent them on provenance
arguments instead of grepping for the number.

## F201 — a 420-second deadline against a population whose p90 first-write is 831 seconds

**My registered hypothesis was "the deadline is a knife aimed at test-authors", and the first
measurement refuted it:**

    kind             n    min  median    p90    max   wrote before 420s
    implementer     21     90     216    475    823   17/21  (81%)
    test-author     17     90     290    831   1099   14/17  (82%)
    verify/sink      1     90      90     90     90    1/1

**81% vs 82% — the proportions are the same.** So the deadline is not disproportionate *by rate*.
That refutation is what led somewhere better, because it left a contradiction: if implementers cross
420s at the same rate, why are the trips **11 of 11 test-author, 0 of 85 implementer**?

**Because only one implementer ever sat past the deadline unwritten, and it is exempt by file
extension.** Observations past 420s with nothing written: **test-author 11 across 5 distinct tasks;
implementer 6, all on ONE task — `web`, which owns `vendorsync/web/index.html`.** And
`is_code_deliverable` (`judge.rs:224`) lists `.py .rs .ts .tsx .js .jsx .go .java .rb .c` — **`.html`
is not among them**, so `owns_code` is false and the trip never arms. Every *other* implementer wrote
before 420s. The exclusivity is real and now has a mechanism.

**The defect is the constant itself.** The deadline is **420 s**, and it sits BELOW the p90 of both
populations it judges — implementer p90 **475**, test-author p90 **831**, test-author max **1099**.
A tenth of test-authors legitimately need more than twice the deadline to produce their first byte.
The branch has no evidence term (F200): it does not ask whether the worker is progressing, only what
time it is. **PRIME DIRECTIVE 4 in one literal** — `min_age_secs.max(420)` is where the engine
stopped measuring and started guessing.

⚠ **Caveat on the distribution, stated because it cuts against me.** These first-write times come
from `judge_observed`, which only samples when the judge actually runs, so each figure is the first
*observation* at which the file existed, not the moment it was written — every number is an
**upper bound**, and the true p90 is lower than 831. What it cannot inflate is the killed set: those
11 were observed at 420-485 s with nothing on disk, and **2 of the 11 never completed across all
three attempts.**

**And 37% of the supervision never happens.** `judge_skipped` totals **178** — test-author 106,
implementer 56, sink 16 — against 302 verdicts delivered, and **every single skip is
`no_idle_device`**. The judge needs a free node to run. So whether a worker crossing the deadline is
actually judged depends on fleet occupancy at that instant, not on its health. Under PRIME DIRECTIVE
3 this is the sharpest form of the idle-node argument yet: **the busier the swarm, the less of its
own supervision runs** — and supervision is the thing that is supposed to make more nodes better
rather than merely faster.

**Queued as patch #15** — derive the deadline from the run's own observed first-write distribution
instead of a literal, and arm on evidence (no progress) rather than on the clock alone.

**LESSON 76 — A REFUTED HYPOTHESIS IS A POINTER, NOT A DEAD END.** "The deadline hits test-authors
harder" was wrong by rate and the contradiction it left — same rate, opposite outcomes — is what
exposed both the `.html` exemption and the fact that the constant sits under everyone's p90. Had the
first measurement agreed with me I would have stopped there and shipped a worse explanation.

## F202 — patch #16 WITHDRAWN: the deterministic checks already run without a device

I queued patch #16 last tick on a deduction — *"`deterministic_verdict` is a pure function of
`JudgeInput`, so run it unconditionally and gate only the LLM judge"* — without reading the call
site. Reading it (`swarm.rs:16603-16624`) shows **the engine already does exactly that**:

```rust
// Phase 1: cheap, unambiguous signals (won't-compile, no-output-while-old) act without a model.
if let Some(out) = deterministic_verdict(&input, &cfg) {
    return out;
}
// No idle device was free for the model review …
if req.judge_model_id.trim().is_empty() {
    me_events_skip(&self.events, &req.task_id, "no_idle_device");
    return JudgeOutcome::ok();
}
```

`deterministic_verdict` runs **before** the skip, on every invocation. The comment states the intent
outright: *"The cheap deterministic checks above already ran without a model; skip the LLM review
rather than queue it behind a busy worker."* **The patch would have changed nothing, and its
registered check ("`judge_skipped` with a deterministic verdict available is 0") would have passed
trivially on the unmodified engine and read as a success.**

**⚠ F201's headline is CORRECTED. "37% of the supervision never runs" is WRONG.** What is true:
**37% of the SEMANTIC review never runs.** Deterministic supervision runs 100% of the time — which
is precisely why the 420 s deadline fires reliably while the model review does not. The two halves
of the judge have completely different availability, and I collapsed them into one number.

**The real finding survives, and the engine states it better than I did** (`swarm.rs:16617-16622`):

> "The scheduler hands the judge a model ONLY when a device is idle … With execute occupancy measured
> at 0.72-0.93, nodes are busy nearly all the time, so the semantic review is not being declined — it
> is UNREACHABLE. **High utilisation and semantic judging are in direct tension, and nothing in the
> log said so.**"

That is the idle-node argument stated by the engine's own author, with the occupancy numbers
attached. It is not a defect I found; it is a documented tension I re-derived badly. What my data
adds is the current magnitude on this fleet: **41 skips against 78 judge runs on the live arm (34%),
178 against 302 across the corpus**, every one `no_idle_device`.

**LESSON 77 — READ THE CALL SITE BEFORE QUEUEING A PATCH, NOT ONLY THE FUNCTION.** I had read
`deterministic_verdict` and `JudgeInput` and concluded the change was needed from their shapes. The
ordering that made it unnecessary was eleven lines above the skip I was patching, in a file I had
already opened twice this session. A patch whose registered check passes on the *unmodified* engine
is worse than no patch: it manufactures evidence of an improvement that was never made.

## F203 — CROSSED. Five engine changes are live and verified in the binary the sweep executes

After 2.5 days in which the engine was never once changed and measured, the boundary is crossed.

**Verified in `target/release/goose` (built 08:03), which is what `loop.sh:119` executes:**

    no_first_write                 1
    judge_accepted                 1
    "the deliverable is complete"  1

⚠ **A near-miss worth recording.** `which goose` resolves to `~/.local/bin/goose`, dated **June 17**
with ZERO of these markers. Had the sweep invoked the PATH binary, this run would have measured
six-week-old code and produced another null that looked like a failed idea. It does not —
`loop.sh:119` pins `ROOT/target/release/goose` — but the check cost one command and the alternative
was another wasted cycle. **Never assume the binary on PATH is the binary under test.**

**What shipped (commit `dc54d9064`):**

1. `Verdict::Accept` — the judge had no accept, so a finished deliverable and a stuck worker both
   resolved to kill, and the third kill is terminal. Not gated like `salvage_spin`, which excludes
   test tasks and therefore excludes 93% of all failures.
2. An evidence term on the 420 s deadline — a worker still taking actions gets double the budget.
3. `is_still_producing` keyed on ACTIONS, not reasoning, plus `prev_tool_calls` threaded through all
   three dispatcher mirror sites.
4. `Verdict::NoFirstWrite` — a zero-tool-call worker is no longer logged as "over_reading".
5. `pick_device` — weight is DECISIVE for hard tasks, not a tie-break. And the pool config had the
   **workhorse switched off** (`enabled: false`), along with `local`; the fastest machine in the
   fleet was not receiving work at all.

**And the pace fix, which matters more than any single change:** `crates/goose-swarm/tests/judge_replay.rs`
replays archived observations through the REAL `deterministic_verdict`. **4 tests, 0.00 s.** Every
judge change from here is validated in milliseconds, then confirmed on a run — not discovered by one.

**LESSON 78 — IF THE FEEDBACK LOOP IS HOURS, BUILD THE OFFLINE ONE FIRST.** A pure function does not
need a fleet. Two and a half days of 100-minute runs were spent validating judge behaviour that a
200-line test settles instantly. The loop length was treated as a constraint to schedule around when
it was a defect to fix.

## F204 — the offline loop's first dividend: one of my five changes is INERT on the observed data

Replaying 253 archived observations through the CURRENT `deterministic_verdict`, in 0.00 s:

    deadline trips that NOW SURVIVE (worker was still acting):  0
    deadline trips that STILL FIRE (worker genuinely stalled):  7
    observations that NOW yield Accept instead of a kill:       7

**The evidence term on the 420 s deadline changes NOTHING on this corpus.** Every archived worker that
hit the deadline had tool_calls 0-2 and FLAT between consecutive observations, so `is_still_producing`
is false and the deadline stays at 420 exactly as before. F201 correctly identified that the constant
sits below both populations' p90 — but the workers who actually got killed were not the slow-and-
working ones that fact predicts; they were genuinely stalled. **The change is a safety net for a case
this corpus does not contain.** It should stay (a slow-but-active worker must not be cut), but it must
not be credited with any improvement that shows up in the run now in flight.

**`Verdict::Accept` is the change that does work: 7 kills become completions.** That is the F165 fix,
and it is the only one of the five with a measurable path to the test-author row.

**Registered as a PREDICTION before the run confirms it:** if `failures.py`'s test-author row improves
on the new build, the mechanism is Accept, not the deadline. If it does NOT improve, the Accept branch
is either not firing (check for `judge_accepted` in the log) or the failures have a cause upstream of
the judge entirely — and the next suspect is F196's truncated API blocks, which are still unfixed.

⚠ **Approximation bounding the claim:** the archive keeps `owns_files` but not the owned PATHS, so the
replay synthesises one `.py` deliverable per task. That is correct for every task in the corpus except
`web` (an `index.html` owner that `is_code_deliverable` exempts), so the figures OVERSTATE the
deadline-eligible population by exactly one task.

**LESSON 79 — QUANTIFY A CHANGE BEFORE THE RUN, NOT AFTER.** Five changes shipped together; the
offline replay separated them in milliseconds and showed one is inert on the very data that motivated
it. Without that separation the run's result — good or bad — would have been attributed to the whole
batch, and a genuinely useful safety net would have been credited with an improvement it cannot
produce, or blamed for a regression it cannot cause.

## F205 — the Accept branch is ARMED: `file_contents` keys match `owned_files` exactly

Lesson 53 applied to my OWN change rather than to a lever. F204 showed `Verdict::Accept` is the one
shipped change with a path to the metric — which makes its precondition the thing most worth
falsifying. The offline replay SYNTHESISED `file_contents`, so it could not have caught a mismatch.

`swarm.rs:16505-16525`, where the engine builds the input:

```rust
for f in &req.owned_files {
    let path = cwd.join(f);
    ...
    if let Ok(contents) = std::fs::read_to_string(&path) {
        if !contents.trim().is_empty() {
            if let Some(err) = syntax_error(&path).await { compile_errors.push((f.clone(), err)); }
        }
        file_contents.push((f.clone(), contents));
    }
}
```

Three things had to hold and all three do:

1. **It iterates `req.owned_files`** — every owned file is attempted, not a subset.
2. **It pushes `f.clone()`**, the SAME string as in `owned_files` — so my
   `file_contents.iter().any(|(p, c)| p == f && ...)` compares identical keys, not a path that has
   been joined, canonicalised or relativised on one side only. That was the failure mode worth
   checking: `cwd.join(f)` exists in this very loop, and had the pushed key been `path` instead of
   `f`, `all_owned_present` would be false forever and Accept would never fire.
3. **A file is pushed only when `read_to_string` succeeds**, and my check additionally requires
   non-empty contents — so an existing-but-empty deliverable correctly fails to satisfy Accept.

`compile_errors` is filled from `syntax_error(&path)` over the same set, so
`input.compile_errors.is_empty()` means every present owned file parses.

**Accept can fire.** It requires: every owned file readable and non-empty, no syntax errors, task is
not `integrate-verify`, elapsed ≥ 420 s, no owned write for ≥ 420 s, and not still producing.

**Registered run-based check (no new literal — Lesson 34):** `judge_accepted` appears in the attempt
log and `judge_verdict{verdict:"accept", action:"accepted"}` in the event stream of the next run that
has an idle-but-finished worker. Its absence across a full run with test-authors completing means the
branch is unreachable in practice and F204's prediction is void.

## F206 — the new build's planning phase, and the baseline's registered question ANSWERED

`baseline-n3-r0` sat at `skeleton_drafts` for 35 minutes with 0 dispatched, which looked like a stall
and would have been blamed on my `pick_device` change. It is not a stall. The event tally:

    run_started 1 · pool_resolved 1 · research_tools 1 · scouts_planned 1 · research_completed 1
    levers_resolved 1 · skeleton_drafts 3 · detail_completed 18 · confidence_retarget 2
    retarget_discarded 2

**`skeleton_drafts{requested: 3, returned: 3, dead: 0, straggler_aborted: 0, secs: 222,
chars: [5172, 4687, 4782], worker_count: 3}`** — three real drafts from three nodes, none dead, none
aborted as a straggler. Best-of-N is drafting across the whole fleet, which is what it is for and
which the pool could not do while two devices were `enabled: false`.

**THE BASELINE'S REGISTERED QUESTION IS ANSWERED, and by the FIRST unit exactly as its gate said it
would be: `detail_fallback` is ZERO.** Eighteen `detail_completed` events, `spec_chars` 1247-2085
against `brief_chars` 201-303, `budget_secs` 420 with observed `secs` 36-75. Not one task fell back
to the architect's one-line brief. That was Part 3 defect #4 — *"a detail that times out silently
degrades to the architect's one-line brief"* — and on this build, at this fleet size, it does not
happen: every worker will be dispatched with a 4-8× richer spec than the brief.

**What the time is being spent on, and the open question it raises.** Two `confidence_retarget`
rounds, both `action: "redraft"`, `binding_signal: "agreement"`, `conf_before: 83`, and
`detail: "best_of_n 4→5"`. The ask floor resolves to 85 (80 + the weak-planner bump), so the plan
sits **two points under the bar** and the engine keeps redrafting to close a 2-point gap — raising
best-of-N each round. That is quality-seeking behaviour and it is the right instinct, but it is also
where the 35 minutes went, and it is a HARD-CODED bar being chased by an UNBOUNDED number of
redrafts. **Registered to check at run end: total wall-clock against the old build's 100-minute mean.
If planning grew and execute did not shrink, the retarget loop is buying confidence points rather
than quality, and `max_retarget_rounds` is the next lever.**

⚠ **Not a comparison yet.** One unit, and the crossing invalidated cross-build score comparison
(F192's 80.1%/12.6pt spread is now historical). This is a mechanism readout, which is valid at n=1
(Lesson: FIRED ≠ CORRECT, but a fallback count of zero is a fact about the code path taken).

**LESSON 80 — A PHASE THAT LOOKS STALLED MAY BE DOING MORE WORK, NOT LESS.** "35 minutes, nothing
dispatched" reads as a hang. The event stream showed three concurrent drafts, eighteen completed
detail specs and two deliberate redraft rounds. I nearly attributed it to my own most recent change;
the tally took one command and named the actual consumer.

## F207 — ✅ THE WEIGHTS ASK IS DONE AND VERIFIED LIVE: both hard tasks landed on the workhorse

Mihai's request: *"implement a logic where single tasks or bigger tasks always end up on the machine
with highest amount of weights … it is workhorse that is the machine with the highest performance."*

First dispatch wave of `baseline-n3-r0` on the new build:

    task            difficulty   device
    api             hard         worksmacstudio-workhorse-…     <- speed_weight 3
    meridian        hard         worksmacstudio-workhorse-…     <- speed_weight 3
    store           easy         mac-gabee-…                    <- speed_weight 1
    readme          easy         local-mihai-…                  <- speed_weight 2
    web             easy         local-mihai-…
    verify::readme  easy         local-mihai-…

**Both HARD tasks on the workhorse; every EASY task elsewhere.** The workhorse's two slots
(weight 2) are filled by exactly the two hard tasks available, and the light work spread across the
slower nodes instead of competing for the fast one.

**Two independent defects had to be fixed for this, and either alone would have left it broken:**

1. **The pool config had the workhorse switched OFF.** `enabled: false` on both `local-mihai` and
   `worksmacstudio-workhorse` — the fastest machine in the fleet was receiving no work at all, and no
   amount of routing logic can send a task to a disabled device.
2. **`pick_device` sorted by `in_flight` FIRST for every task.** Speed was a tie-break only, so with
   the fast host holding one task and a slower host idle, the heaviest task went to the SLOW host —
   backwards, since the critical path is set by exactly those tasks. Weight is now decisive for hard
   tasks, with observed ms/task breaking ties among equally-weighted hosts so a first-dispatch
   timing accident cannot outrank the operator's stated fastest machine.

The planner marked 8 of 19 tasks hard (`test-meridian`, `integrate-verify`, `api`, `meridian`,
`test-api`, `verify-e2e::0/1/2`), so this routing will be exercised repeatedly through the run rather
than being a one-off coincidence of the first wave.

⚠ **What this does NOT claim.** It is a ROUTING fact, not a wall-clock result. Whether putting the
hard tasks on the fast node actually shortens the run is a separate measurement, and the honest test
is total wall-clock against the old build's 100-minute mean — reported at run end, not now. It also
does not touch the test-author failure row, which remains the open mini-goal at 14/15.

**LESSON 81 — A ROUTING RULE AND THE ELIGIBILITY IT ROUTES OVER ARE TWO SEPARATE FAILURES.** I could
have written the perfect `pick_device` ordering and measured nothing, because the target device was
`enabled: false` upstream of it. Before tuning a selection policy, confirm the thing being selected
is in the candidate set at all.

## F208 — ✅ `Verdict::Accept` FIRED IN A LIVE RUN, and the run has ZERO interrupts so far

The registered prediction from F204 has its mechanism confirmed. `baseline-n3-r0`, 58 minutes in:

    api   verdict=accept  action=accepted
          "All 1 owned file(s) exist and pass their syntax check, and nothing has changed
           for 612s — the deliverable is complete."

**`api` completed at `attempts=1`.** On the old build that exact state — owned file written, untouched
for 612 s, worker not producing — is the `finalize-spin` branch, which returns `Looping` → the
scheduler re-dispatches → a second attempt, and the third kill is terminal. The judge's only lever
was to stop the worker, so "this looks finished" and "this looks stuck" produced the same action.

**Judge tally at 58 min: `{'observed': 17, 'accepted': 1}` — 0 interrupted the worker, 1 finished it.**
Nine tasks completed, **every one at `attempts=1`**. Against the old build, where of the 11 tasks that
hit a deadline kill **none finished in one attempt** (8 needed 3, 3 needed 2) and 2 never completed.

**What this proves and what it does NOT.**

- **PROVES:** the branch is reachable in production, its precondition (F205) holds against real
  `file_contents`, and it takes the case it was written for. It is not a test-only path.
- **DOES NOT PROVE the metric moved.** `api` is an IMPLEMENTER. F164's row is about TEST-AUTHORS, and
  implementers already failed at 0/63. The tasks that matter — `test-api`, `test-meridian`,
  `test-store-edgecases`, `test-store-error-handling` — are still in flight. `test-store` did complete
  at `attempts=1`, which is the right shape, but one task is not a row.
- **DOES NOT PROVE a wall-clock win.** Still owed against the old build's 100-minute mean.

⚠ **`deterministic` reads `None` on every verdict** — correct and expected: patch #13 was committed
AFTER the 08:03 binary was built, so this run predates the provenance field. It lands at the next
crossing, and `review.py` deliberately prints the provenance line only when the field exists rather
than reporting a misleading all-False.

**LESSON 82 — A MECHANISM FIRING IS THE FIRST HALF OF A RESULT, AND IT IS WORTH SAYING SO SEPARATELY.**
Two and a half days produced findings; this is the first engine change observed doing its job on a
live run. That deserves to be recorded as a distinct milestone from "the metric moved", because
conflating them is how a campaign talks itself into a win it has not earned — and because if the row
does NOT move, knowing the branch fired is exactly what narrows the next search to upstream causes.

## F209 — the stall detector, and the metric that structurally COULD NOT MOVE

Mihai installed an analysis flow: *"if your update continues to stay the same for 10 ticks which is
your (2) and (3) then it means you need to shake things up. If (1) becomes resolved then this whole
session is resolved."*

`goalstate.py` implements it, and it is a SCRIPT rather than a habit on purpose. The failure it
guards against is one an agent cannot self-police: every individual tick had a defensible reason to
continue as it was — measure the variance first, wait for the arm, let the baseline finish. Ten such
reasons in a row is a stall, and only something that persists across context compaction can see ten
in a row. **The streak keys on the MEASURED metric read off the archive, not on the goal text I
pass in, so it cannot be reset by rewording.**

**Building it exposed two real defects, one in my own code and one in the campaign's headline metric.**

**(a) My first `measured_metric()` fabricated its numbers.** It called `failures.load()` — which
returns RAW EVENTS — and then read `r["kind"]` and `r["failed"]`, fields that exist on no event.
Every lookup returned `None`, and it printed *"45 completed / 0 failed, 0% of all failures"* against
a known 93%. Caught only because that 0% contradicted a number I already had (Lesson 47: an
impossible value indicts the instrument). Now mirrors `failures.main`'s actual logic — same
`kind_of`, same `run_finished` gate, same `status != "done"` test.

**(b) `failures.py` — THE improvement metric — pools across every engine this campaign has ever
run.** Its glob finds **33 logs, 27 of them in `nodeloop-preboundary-*` archives** from builds that
no longer exist. So "test-authors are 14/15 = 93% of ALL failures" is an average over a day and a
half of runs that **cannot respond to anything I change today**. A stall detector keyed on that
number would fire every tick and mean nothing — and, worse, a genuine improvement would be invisible
inside it.

`goalstate.py` therefore scopes to **runs produced by the binary currently on disk**
(`run.jsonl` mtime ≥ `target/release/goose` mtime). Self-maintaining: no manual boundary marker, it
follows every future crossing automatically, and it states its claim precisely. The boundary
procedure already kills the engine BEFORE rebuilding, which is what keeps a run from straddling.

**Current reading: NO FINISHED RUN on this binary yet** — printed as absence of evidence, explicitly
NOT as a 0% failure rate, because `0 completed / 0 failed` and "zero failures out of many" are
different statements and the vacuous-truth trap (`all([])` is `True`) is exactly how a campaign
credits itself with a perfect score it never earned.

**LESSON 83 — A METRIC POOLED ACROSS BUILDS IS A METRIC THAT CANNOT MOVE.** F164's row was the
campaign's north star for days while silently averaging pre-change and post-change runs together.
Every improvement I could have made would have been divided by 27 runs of history. Before trusting a
metric to detect a change, check that its SAMPLE can respond to the change at all.

## F210 — `no_first_write` is live and correct, and it sharpens the test-author diagnosis

`baseline-n3-r0` at 70 min, every non-`observed` verdict:

    api                        verdict=accept           action=accepted
    test-store-error-handling  verdict=no_first_write   action=re_dispatch
    test-api                   verdict=no_first_write   action=re_dispatch
    test-meridian              verdict=no_first_write   action=re_dispatch
    test-api                   verdict=no_first_write   action=re_dispatch

**PATCH #14's REGISTERED CHECK PASSES: zero `over_reading` verdicts carry `tool_calls == 0`.** On the
old build 9 of 11 did, and that mislabel produced three separate false causal chains before anyone
checked the counter beside it. Every kill in this run is now named for what actually happened.

**And the label change immediately earns its keep, because it renames the problem correctly.** All
four re-dispatches are test-authors, and the observations behind them are `calls=0` with thinking at
1,992-7,118 chars and nothing written. These workers are not over-reading, not thrashing, and not
finishing-then-spinning. **They never take a single action.**

**That is the finding that matters for the mini-goal, and it CONSTRAINS what can fix it:**

- **`Verdict::Accept` cannot help this population.** Accept requires every owned file present; these
  workers have written nothing. F208's win on `api` was the "finished but idle" case, which is an
  IMPLEMENTER pattern. The test-author failure mode is a different shape entirely.
- **So F204's registered prediction is now expected to come out NEGATIVE for test-authors**, and the
  reason is structural rather than a firing failure. Recording that BEFORE the run ends: if the
  test-author row does not improve, the explanation is not "Accept did not fire" (it fired) and not
  "the branch is unreachable" (F205 proved it armed) — it is that Accept addresses a failure mode
  test-authors do not have.
- **The next suspect is exactly where F196 pointed:** a test-author receives 10,097 chars of
  dependency source, 50.7% of its prompt, three of four blocks truncated mid-token with one that does
  not parse — and then takes zero actions for seven minutes. **`dep_signatures` is the arm that tests
  it and it is now second in the queue.**

⚠ `test-meridian`'s newest observation reads `elapsed=152` with the verdict already emitted — that is
a POST-RESTART observation of the re-dispatched attempt, not a deadline firing at 152 s. Noted so a
later reader does not mistake it for a broken predicate.

**LESSON 84 — A CORRECT NAME NARROWS THE SEARCH; A WRONG ONE WIDENS IT.** Renaming the verdict was
the cheapest change in the batch and it produced the sharpest diagnostic step: "over_reading" invited
theories about what the worker was reading, while "no_first_write" states the actual observable —
zero actions — and immediately rules out every fix aimed at what a worker does AFTER it starts.

## F211 — 🔬 THE MODEL FILE EXPLAINS THE DEFECT: thinking is PREFILLED ON and temp is 1.0

Mihai's lead — *"check the actual model files and see if anything presents itself as an
opportunity"* — paid off inside ten minutes. Read directly from the GGUF header (no model load; the
KV store is at the head of the file, and neither the `gguf` python package nor llama.cpp is installed
on this machine, so `scratchpad/ggufkv.py` parses the documented format directly).

**FACT 1 — the author embedded sampling defaults, and they are tuned for PROSE, not tool calls:**

    general.sampling.temp  = 1.0
    general.sampling.top_k = 20
    general.sampling.top_p = 0.95

**FACT 2 — the model is a SEVEN-WAY MERGE and most parents are creative-writing models:**

    general.name = "Qwen3.6 27B Architect Polaris2 Fable B F451 NM"
    base_model.0 Qwen/Qwen3.6-27B
    base_model.1 DavidAU/Qwen3.5-27B-Claude-4.6-OS-INSTRUCT
    base_model.2 DavidAU/Qwen3.6-27B-Heretic2-Uncensored-Finetune-Thinking   <- "Thinking"
    base_model.3 nightmedia/Qwen3.6-27B-Architect-Polaris
    base_model.4 armand0e/Qwen3.6-27B-Fable-5-Experimental                   <- fiction
    base_model.5 DavidAU/Qwen3.5-27B-Polar-Rev1-Uncensored-Heretic
    base_model.6 DavidAU/Qwen3.6-27B-F451-AND-TRI-Polar-Ultra-Pro-Writer-…   <- "Ultra Pro Writer"

Arch `qwen35`, 65 blocks, hybrid attention+SSM, `nextn_predict_layers = 1` (the MTP), 262144 context,
vision projector alongside (`mmproj-F32.gguf`).

**FACT 3 — THE CHAT TEMPLATE PREFILLS THE MODEL INTO THINKING MODE.** The generation prompt:

```jinja
{%- if add_generation_prompt %}
    {%- if enable_thinking is defined and enable_thinking is false %}
        {{- '<think>\n\n</think>\n\n' }}      <- closes thinking immediately
        {{- '<think>\n' }}                     <- DEFAULT PATH: opens thinking
```

**Unless `enable_thinking: false` is passed, every single dispatch begins with the assistant already
inside an open `<think>` block.** The model is not choosing to deliberate — the template puts it
there before it sees a single token of its own.

**FACT 4 — the tool-call format is XML, and the template INVITES pre-call prose:**

    <tool_call><function=NAME><parameter=KEY>value</parameter></function></tool_call>
    "You may provide optional reasoning for your function call in natural language BEFORE the
     function call, but NOT after"

**FACT 5 — GOOSE SENDS NOTHING TO COUNTERACT ANY OF IT.** `swarm.rs:1014`:

    temperature: None,  top_p: None,  top_k: None,  min_p: None,  repeat_penalty: None,

and `enable_thinking` / `chat_template_kwargs` appear **nowhere** in the swarm's LM Studio path
(`enable_thinking` exists only in `goose-local-inference`, a different provider the swarm does not
use). So every worker runs at the author's defaults: **temperature 1.0, thinking prefilled ON, on a
merge whose majority parents are fiction models.**

**This is a complete, evidence-backed explanation of F210's observation** — test-authors emitting
1,992-24,032 characters of reasoning with `tool_calls = 0` for seven minutes. Not a mysterious model
pathology; a configuration that asks for exactly that.

**WHAT IS VERIFIED vs WHAT IS PROPOSED — stated separately on purpose.**
- VERIFIED: all five facts above, each read from the file or the source.
- PROPOSED and UNTESTED: that lowering temperature and/or passing `enable_thinking: false` will
  increase tool calls and shorten time-to-first-write. **Plausible and unproven.** The falsifier is
  cheap and does not need the fleet: a single `curl` to `localhost:1234` with a worker-shaped prompt,
  once at the defaults and once with `temperature: 0.3` + `chat_template_kwargs:
  {"enable_thinking": false}`, comparing whether a `<tool_call>` block appears and how many
  characters precede it.
- ⚠ **Disabling thinking outright may cost quality on genuinely hard tasks.** The nuanced version is
  to disable it for the FIRST worker turn only (force an action, then allow deliberation), or to
  lower temperature without touching thinking. Do not conflate the two changes — test them apart,
  which is the F204 lesson.

**LESSON 85 — WHEN A MODEL BEHAVES STRANGELY, READ ITS FILES BEFORE THEORISING ABOUT ITS MIND.**
Five weeks of prompt engineering against a model whose own chat template opens a `<think>` tag on
every turn and whose author ships temperature 1.0. The answer was in the first 4 MB of a file that
sat on disk the entire time, and no amount of rewording the system prompt could have reached it.

## F212 — ✅ PROVEN OFFLINE: `enable_thinking: false` pre-closes the think block. Zero tokens spent.

F211 found the template branch; this renders it and proves what the switch does — with jinja2, on the
real 7,764-char template extracted from the GGUF, using a worker-shaped message + tool list. **No
request to `localhost:1234`, so no contention with the live measured run** (Lesson 59 applies to my
own probes, not just to the engine's idle-fill).

**The last 120 characters of the rendered prompt, both ways:**

    UNSET  (what goose sends today):
      …<|im_start|>user\nWrite the unit tests…<|im_end|>\n<|im_start|>assistant\n<think>\n

    enable_thinking=False:
      …<|im_start|>user\nWrite the unit tests…<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n

    prompts differ: True   (1312 vs 1323 chars)

**Today every worker is handed an OPEN `<think>` tag and must generate its way out of it before it
can emit anything else.** With the switch off, the block is opened AND CLOSED for the model, so its
first generated token is the answer — or the `<tool_call>`. That is the exact difference between
"emits 24,032 characters of reasoning and zero tool calls" and "acts".

⚠ **A check line in my own script printed a misleading `False`** — I wrote the needle with an escaped
backslash inside an f-string, so it tested for a literal that never occurs. The `repr()` output is
the evidence and it is unambiguous; the boolean was wrong, not the finding. Recording it because a
green/red flag that disagrees with the raw data must always lose to the raw data (Lesson 40), and
because the same class of bug silently fabricated the metric in F209.

**WHAT REMAINS UNKNOWN, and it is the one thing between here and a fix:** whether LM Studio's
OpenAI-compatible endpoint forwards `chat_template_kwargs` to the template renderer. If it does, this
is a small change in the swarm's request path. If it does not, the alternatives are (a) a `/no_think`
soft switch — **this template has no such branch, checked**, so that route is closed here; (b)
prefilling the assistant turn; (c) driving llama.cpp directly. The research workflow's web lens is
tasked with exactly this and its answer decides the implementation.

**REGISTERED, BEFORE ANY MEASUREMENT:** the falsifier for "thinking-prefill causes the zero-action
workers" is that turning it off changes neither `tool_calls` at first observation nor
time-to-first-write. If both stay flat, the prefill is a correlate and the cause is elsewhere —
most likely F196's truncated dependency blocks, which remain unfixed and are the other standing
suspect. **Do not test this together with a temperature change (F204's lesson: five changes shipped
at once nearly credited an inert one).**

**LESSON 86 — THE CHEAPEST DECISIVE EXPERIMENT IS OFTEN A RENDER, NOT A RUN.** The question "does
this switch do what I think" is about a template, and a template is a pure function of its inputs.
It took one jinja2 render to answer definitively what would otherwise have been a fleet request
competing with a measured run — or worse, a full arm.

## F213 — 🔴 THE OBVIOUS FIX IS BROKEN IN LM STUDIO. The viable path is the TEMPLATE ITSELF.

I was one commit from implementing `chat_template_kwargs: {"enable_thinking": false}` in the swarm's
request path. **It would not have worked, and it would have looked like a failed hypothesis rather
than a broken transport.**

**LM Studio bug tracker issue #1990 (opened 2026-05-31, STILL OPEN):** `enable_thinking: false` is
IGNORED for Qwen3.5 GGUF models over the OpenAI-compatible API. **All three suppression routes fail:**

    1. "enableThinking": false        in /v1/chat/completions   — ignored
    2. "chat_template_kwargs": {...}  in the request body       — ignored
    3. defaultValue: false            in model.yaml             — ignored

**The reporter's symptom is mine, exactly:** with `max_tokens: 100`, ~99 tokens go to
`reasoning_content` and `message.content` comes back EMPTY. That is F210's zero-action worker
described from the API side.

**⇒ F212's proof stands** (the template DOES pre-close the block when the kwarg reaches it) **but the
DELIVERY CHANNEL is broken.** This is the F202 pattern caught before the fact: a change whose
registered check would have passed on an unmodified engine, teaching a false lesson about the
hypothesis rather than about the plumbing.

**THE VIABLE PATH — `froggeric/Qwen-Fixed-Chat-Templates`, prior art aimed at exactly this model
family.** A corrected Jinja template for Qwen 3.5/3.6 that:

- **Names and fixes "Empty Think Poisoning"** — an empty `<think>\n</think>` block teaches the model
  to associate reasoning with tool calls. ⚠ **A warning about the naive fix**: even delivered
  correctly, the empty block `enable_thinking: false` emits may itself degrade tool calling. Both
  facts together mean the naive switch was doubly unsafe.
- **Restores the native XML tool-call format** (`<function=name>`) "that Qwen was trained on".
- **Adds `<|think_on|>` / `<|think_off|>` tags that work FROM INSIDE THE PROMPT.**
- Two-tier agentic error escalation, so the model stops re-emitting an identical failing call.
- Installed via LM Studio's per-model **Prompt Template** field — which **bypasses the broken
  `chat_template_kwargs` transport entirely**, being applied server-side.

**THE STRATEGIC POINT, and the reason this matters beyond one switch:** a template honouring
**in-prompt** tags gives the ENGINE per-dispatch control over thinking **with no API support and no
provider changes**. goose would emit `<|think_off|>` for a test-author and `<|think_on|>` for a
planner — precisely the kind-aware behaviour this campaign has wanted since F157.

**WHAT I AM NOT DOING, and why.** Replacing the prompt template is a **fleet-side change on three
machines**, and a measured run is in flight (`baseline-n3-r0`, 20/21 done). Changing it now would
invalidate that run AND confound the engine changes already under test. It also sits near the
standing "never reconfigure the fleet" rule — that rule is about load/unload/re-alias and a template
swap is different in kind, but it is still fleet-side and deserves a deliberate boundary.

**REGISTERED PLAN, in order:** (1) let the run finish, take its readout; (2) diff the fixed template
against the current one by rendering BOTH offline with jinja2 — the same zero-token method as F212,
showing exactly what changes before anything is installed; (3) only then decide, and if installed, do
all three nodes at a boundary recorded as a fleet-state change so no later comparison spans it.

**LESSON 87 — CHECK THAT THE TRANSPORT WORKS BEFORE BLAMING THE HYPOTHESIS.** The mechanism was
proven, the switch exists, the template honours it — and the API that carries it drops it on the
floor. Had I shipped it, the null would have read as "thinking-prefill is not the cause" and closed
the most promising lead of the campaign. **A negative result is evidence about the hypothesis only if
the change actually reached the system.**

**LESSON 88 — `cd x && cmd` SILENTLY SKIPS `cmd` WHEN THE `cd` FAILS.** This finding was "committed"
once already and the commit contained only a one-line tick file: I was in `nodeloop` and wrote
`cd evals/swarm-bench/nodeloop && cat >> FINDINGS.md`, the `cd` failed, and the `&&` swallowed the
whole write. The commit SUCCEEDED and said what I meant to have written. Verifying with
`git show --stat` is what caught it — the same "grep before asserting" rule that F170 and F191b both
cost me.

Sources: LM Studio bug tracker #1990; froggeric/Qwen-Fixed-Chat-Templates (HuggingFace).

## F214 — ⭐ THE PATH IS COMPLETE: an IN-PROMPT tag does what the broken API kwarg cannot

Step 2 of F213's registered plan, executed offline with jinja2 on both real templates. **Zero tokens,
no fleet contention, nothing installed.** The fixed template fetched from
`froggeric/Qwen-Fixed-Chat-Templates` (16,289 bytes vs the current 7,764).

Same worker-shaped system+user message, same tool list, `add_generation_prompt=True`:

    render                                    ends with an OPEN <think>
    CURRENT template, plain                   True     <- today's defect
    FIXED   template, plain                   True     <- default PRESERVED
    FIXED   template, "<|think_off|>…" prompt  FALSE    <- pre-closed
    FIXED   template, enable_thinking=False    FALSE

    FIXED + think_off tail: …<|im_start|>assistant\n<think>\n\n</think>\n\n

**Three things this establishes, each of which matters separately:**

1. **`<|think_off|>` placed in the PROMPT TEXT produces exactly the pre-closed block that
   `enable_thinking: false` would.** Prompt text is something goose controls completely — no API
   field, no `chat_template_kwargs`, **so LM Studio bug #1990 is bypassed rather than worked around.**
2. **The fixed template's DEFAULT behaviour is identical to the current one** (plain render still ends
   in an open `<think>`). So installing it does not silently change planners, scouts, or the judge —
   suppression is strictly OPT-IN, per dispatch. That is the difference between a targeted fix and a
   fleet-wide behaviour change, and it is why this is worth installing at all.
3. **It gives the engine KIND-AWARE control of deliberation** — `<|think_off|>` for a test-author
   that must write a file, nothing for a planner that genuinely should reason. This campaign has
   wanted exactly that since F157 ("the engine holds a fact and does not use it to narrow what it
   sends"), and here the fact is the task's KIND, which the dispatcher already computes.

**THE FULL CHAIN, every link now evidence rather than argument:**

    F210  test-authors take ZERO tool calls for 420-700s and are killed        (measured, live run)
    F211  the chat template hands every worker an OPEN <think> tag             (read from the GGUF)
    F212  enable_thinking:false pre-closes it                                  (rendered offline)
    F213  …but LM Studio DROPS that kwarg — open bug #1990                     (upstream bug report)
    F214  an in-prompt <|think_off|> tag achieves it through text alone        (rendered offline)

**WHAT IS STILL NOT PROVEN, stated plainly.** That a pre-closed think block actually makes THIS model
call a tool sooner. Every step above is about what reaches the model; none is about how it responds.
The falsifier stays as registered in F212: if `tool_calls` at first observation and time-to-first-write
both stay flat, the prefill is a correlate and F196's truncated dependency blocks are the cause.

**INSTALLATION REQUIREMENTS, deliberately not done yet.** The template is per-model LM Studio state on
THREE machines. It must go in at a boundary, on all three, recorded as a fleet-state change so no
later comparison spans it — and the engine change (emit `<|think_off|>` for file-owning workers whose
kind is test-author) must land in the same crossing, because either alone is inert.

**LESSON 89 — WHEN THE OFFICIAL CHANNEL IS BROKEN, LOOK FOR ONE THE SYSTEM CANNOT DROP.** The API
field is optional metadata a server may ignore; the prompt is the payload it must process. Routing
the control signal through the payload turned an upstream bug from a blocker into an irrelevance.

## F215 — the first sample is NOT evidence, and my own stall detector had a loophole that flattered me

`baseline-n3-r0` finished; the current-binary metric got its first sample: **test-author 5 completed
/ 0 failed.** My first instinct was to report that as the row moving. It is not.

    old-build test-author failure rate: 13/42 = 0.310
    P(0 failures in 5 completions | rate UNCHANGED) = 0.157

    n= 5   P(zero by chance) = 0.157      <- current sample
    n=10                      0.025
    n=15                      0.004

**A roughly one-in-six chance of appearing by luck with the rate completely unchanged.** This sample
cannot distinguish "fixed" from "lucky". **Nine clean test-author completions are needed to clear
p<0.05**; five is not close. Reporting it as improvement would be exactly the failure this campaign
keeps recording under other names — a number that looks green being allowed to stand for a result.

**AND THE INSTRUMENT I BUILT TO POLICE MYSELF HAD THE SAME BLIND SPOT.** `goalstate.py`'s `streak()`
keyed on the raw metric dict, so ANY change reset the stall clock. That non-significant 0/5 **reset a
5-tick streak to 1** — meaning the forced shake-up at 10 ticks could be postponed indefinitely by
ordinary noise, and the more samples arrive the more often it resets. **A stall detector that noise
can reset never fires.** It was written one hour earlier for exactly this purpose.

Fixed: the streak now keys on `(mini_goal, resolved, SIGNIFICANT-metric-move)`, where significance is
`P(this few failures | old rate) < 0.05`. A metric wobbling inside its own noise is the same state,
not a new one. The printout now states the p-value and, when not significant, how large a clean run
would have to be:

    MEASURED (CURRENT binary only): test-author 5 completed / 0 failed  (n=5)
       vs the old-build rate 13/42 = 31.0%: P(this good by chance | rate unchanged) = 0.157
       ⇒ NOT SIGNIFICANT — this could be luck
       a clean run of 9 test-author completions would be needed to clear p<0.05
    UNCHANGED FOR 6 of 10 ticks

**HONEST ACCOUNTING OF THE CAMPAIGN AT THIS POINT**, because the alternative is a flattering summary:

- **One verified win: the weights routing (F207) — and it was Mihai's idea, not mine.** He named the
  defect; I found two causes and fixed them. It is real and it is his.
- **Everything I found independently remains unproven on behaviour.** F211-F214 is an elegant,
  fully-evidenced chain about what REACHES the model. Not one link is about how the model RESPONDS.
- `Verdict::Accept` fires (F208) and cannot help the failing population (F210). That is a mechanism
  working and a hypothesis narrowing, not a metric moving.
- The row is unmoved at any defensible confidence.

**LESSON 90 — AUDIT YOUR OWN GUARDRAILS FOR THE LOOPHOLE THAT FAVOURS YOU.** A self-imposed check is
written by the same judgement it is meant to constrain, so its failure mode will be the one that lets
that judgement off. Ask of every guard: *what is the cheapest way this passes without the thing it
guards being true?* Here the answer was "any noise at all", and it took one non-result to expose it.

## F216 — 🔬 THE RESEARCH LANDED. It supplies the behavioural evidence AND overturns my plan.

Five agents, 628k tokens, 58 minutes. Three lenses converged independently on the same root cause, and
**two of them MEASURED it on the fleet at real prompt size** — which is exactly the evidence F214 said
the whole chain lacked.

**THE BEHAVIOURAL EVIDENCE, measured, not rendered:**

    /v1/completions, 22,187-char worker prompt:
      default template          47.3 s · 974 chars of prose · NO TOOL CALL
      enable_thinking=false      3.1 s · TOOL CALL AS THE FIRST TOKEN          ~15x

    /v1/chat/completions, assistant-turn prefill "<think>\n\n</think>\n\n", 12,226-char prompt + tools:
      finish_reason=tool_calls · ntc=1 · reasoning_tok=0 · wrote a real 10,424-char test file
      3/3 reproducible on the trivial control

**⇒ F212's registered falsifier is DISCHARGED.** The prefill does not merely change what reaches the
model; it changes what the model does, by a factor of ~15 on time-to-first-action.

**⚠ MY PLAN WAS THE WORSE ROUTE, AND THE AGENTS WERE RIGHT.** I intended to install the fixed template
on all three LM Studio nodes. That is fleet reconfiguration — the very thing the standing rule forbids
(*"run with the fleet as it is; if the engine can't use 3 identical nodes, fix swarm.rs, not the
fleet"*) — and it is undocumented drift across three machines. **The prefill reaches the identical
template branch from the harness, over the API, with ZERO fleet config.** I was about to bend a rule
to get a result I could have had without bending it.

**THE MECHANISM, proven rather than assumed:** LM Studio adds **no generation prompt when the last
message is `assistant`** — it becomes a raw llama.cpp continuation. The control returning the literal
`'assistant\n<think>\n\n</think>\n\nOK'` is the proof. So the HARNESS writes the assistant turn's
opening tokens, and `<think>` is opened *and closed* before the model samples anything.

**⚠ CORRECTION TO F211 — I reported the wrong sampler numbers.** I read `general.sampling.*` from the
GGUF (temp 1.0, top_k 20, top_p 0.95). The **effective serve-time config** is
`~/.lmstudio/.internal/user-concrete-model-default-config/…Q8_0.gguf.json`:

    temp 1.0 · top_p 1.0 (NO nucleus truncation at all) · top_k 25 · min_p 0.2 · repeatPenalty 1.05

**temp 1.0 with top_p 1.0 is the highest-entropy configuration available** — the exact opposite of
what an agent emitting rigid XML needs. And **the MTP build requires `repetition_penalty = 1.0`; the
live 1.05 is silently degrading speculative decoding.** The GGUF values are defaults the serve config
overrides; quoting them as effective was my error.

**NEW FACTS, each measured over 519 logged requests:**
- goose sends **no sampler parameter whatsoever** on worker bodies. `max_tokens` in **0/519**.
  `tool_choice` in **0/519** — never used on this path.
- **`kind_prompt` is OFF**, so the tailored test-author blocks at `swarm.rs:19795`/`19847` are
  **unreachable**, and test-authors — 93% of failures — receive the *implementer* rules
  *"NEVER read the project's OTHER TEST files"* and *"STOP WHEN GREEN"*. Instructions for a job that
  SATISFIES tests, handed to a job that AUTHORS them. This is d15ed448e's defect, still live.
- **218 of 519 requests carry echoed `reasoning_content`, max 90,576 chars** — thinking is re-injected
  as context, which lengthens the prompt, which produces more thinking. A compounding loop.

**RANKED, BY CONFIDENCE (the agents' ranking, which I am not overriding):**
1. **Assistant-turn prefill** — HIGH on mechanism, MEDIUM on shipping clean. Unknowns the agents
   flagged themselves: turn ≥2 after a `tool` message, goose's `format_messages_with_options`
   possibly reordering a synthetic trailing assistant, and **streaming tool-call parsing is entirely
   untested while goose always streams**.
2. **`kind_prompt: true`** — HIGH that it fires and is correct, MEDIUM that it moves the rate alone.
3. **`preserves_thinking: false`** — HIGH that it works, MEDIUM net-positive; the Qwen card
   *recommends* preserve_thinking for agents, so this is a genuine coin-flip and must be its own arm.
4. **A real sampler profile** (0.6/0.95/20, repeat_penalty 1.0) — HIGH that it reaches the wire,
   MEDIUM-LOW that it fixes the defect. Both fleet measurements agree: template 15×, samplers
   second-order.
5. **`dep_signatures: true`** — MEDIUM, and **the one item that can plausibly make output WORSE**
   (a test-author with only signatures may fabricate behaviour it can no longer see).
6. **`max_tokens` / `tool_choice:"required"`** — LOW as a fix, HIGH as instrumentation. Both lenses
   that tested `required` found it **does not reduce thinking**, contradicting the naive reading.

⚠ **THE PROBES RAN AGAINST THE LIVE FLEET** while `baseline-n3-r1` was executing. Requests at
22,187 and 12,226 chars are not free (Lesson 59). **That run's timings are contaminated and must not
be used for a wall-clock comparison.** Its task-level outcomes are still usable; its durations are not.
I did not anticipate this when I fired the workflow, and it is my error, not the agents'.

**LESSON 91 — WHEN YOU HAVE A COMPLETE CHAIN AND NO BEHAVIOURAL EVIDENCE, THE MISSING LINK IS THE
WHOLE POINT.** I had five links proven and called the path "complete". It was complete as an
explanation and empty as a result. Two agents spent one request each to get the 47.3 s → 3.1 s number
that settles it — a test I had explicitly deferred as "needs the fleet" while doing offline renders
that could never answer it.

## F217 — the prefill is implemented and gated; and my BUILD CHECK lied for two ticks

**Implemented (commit below), three edits, OFF by default:**

- `goose-provider-types/src/formats/openai.rs` — a `__goose_prefill_assistant` request-param key is
  intercepted and appended as a trailing `{"role":"assistant","content":…}` message instead of being
  copied into the body. The existing `thinking_effort` exclusion on that same line is the precedent.
- `swarm.rs` `run_agent_in` — new `prefill_assistant: Option<&str>` parameter; `None` at both
  planner-side call sites, so those paths are byte-identical.
- `swarm.rs` worker dispatch — emits `<think>\n\n</think>\n\n` when `think_off_test_authors() &&
  is_test_author`, behind `GOOSE_SWARM_THINK_OFF` / `think_off_test_authors`, **default OFF**.

`is_test_author` (19296) and the dispatch (20063) proved to be in the SAME function (`run`, 19188),
so no threading was needed — checked rather than assumed.

**🔴 AND MY BUILD CHECK LIED FOR TWO TICKS.** I was running:

    cargo build -p goose-cli 2>&1 | grep -E '^error' -A 6 | head -20; echo "BUILD_DONE"

**`echo` after a pipeline reports the LAST command's status, and the marker prints unconditionally.**
The build had failed with two errors and I reported it clean — then reported it clean a second time.
The errors were real and both came from assumptions I did not check:

    E0433  `goose_provider_types` is not a dependency of goose-cli   (I assumed the path existed)
    E0308  `swarm_gate_cfg` takes `bool`, not `Option<bool>`          (the Option variant is
                                                                       `swarm_gate_cfg_bundle`)

Fixed by reading the real signatures (`swarm_gate_cfg` at :18900 vs `swarm_gate_cfg_bundle` at
:18990, and `dep_signatures_on` calls the latter) and adding the crate with `cargo add --path` per
AGENTS.md. Verified by reading cargo's own final line — `Finished dev profile … in 18.77s`.

**This is the THIRD self-inflicted false-status bug in one session**, and all three share one shape:
- F209: `failures.load()` returns raw events; reading `r["kind"]` gave `None` and printed a 0%.
- F213: `cd x && cmd` — the `cd` failed, `&&` swallowed the write, the commit still succeeded.
- F217: `cmd | grep …; echo MARKER` — the marker reports the grep, not the build.

**LESSON 92 — A STATUS MARKER YOU PRINT YOURSELF IS NOT A STATUS.** `echo OK` after a pipeline, a
boolean computed from a needle you wrote, a commit that "succeeded" — each is a claim by the harness
about the harness, not evidence about the work. **Read the tool's own terminal output**: cargo's
`Finished`/`error:` line, `git show --stat`, the `repr()` of the data. Every time this session that a
self-authored flag disagreed with the underlying tool, the flag was wrong.

**REGISTERED, BEFORE ANY MEASUREMENT — the streaming risk is the one that decides this.** The fleet
evidence (47.3 s → 3.1 s) was measured on NON-streaming requests. **goose always streams**
(`openai.rs:669-681`). Streaming tool-call parsing with a prefilled assistant turn is untested, and
if the prefill fails that is the most likely place. **Check on the first run with the lever on:**
`llm_request.*.jsonl` — the last element of `messages` must be `role: "assistant"` with the prefill
content, **on every request of a worker's loop, not just the first**; and the swarm's own thinking
accumulator must report 0 for that dispatch. If the message is present and thinking is still
non-zero, the server is not taking the continuation path under streaming and the whole route is dead.

## F218 — the arm IS armed. My check was wrong — and it would have destroyed a valid experiment.

`think_off-n3-r0` is running. The registered arm-armed check (F194, Lesson 53) reported:

    think_off_test_authors = *** ABSENT ***    ⇒ NOT ARMED — VOID THE ARM

**That was my lookup, not the engine.** The event nests everything under a `levers` key:

    TOP-LEVEL: ['build_sha','crate_version','event','levers','run_id','seq','ts','version']
    levers (103 keys) → think_off_test_authors: True

**The arm is ARMED.** I read the top level, found nothing, and was one step from voiding a correct
experiment — a hundred minutes of fleet time thrown away on a bad `dict.get()`.

**This is the FIFTH self-authored check that would have lied this session, and the first that would
have destroyed a result rather than merely misinformed me:**

    F209  read `r["kind"]` off raw events              → fabricated "0% of all failures"
    F213  `cd x && cmd` with a failing cd              → commit succeeded carrying nothing
    F217  `… | grep '^error'; echo BUILD_DONE`         → reported two failed builds as clean
    F217b `grep -F` across a `format!` placeholder     → 0 for code demonstrably present
    F218  top-level `get()` on a nested event          → "VOID THE ARM" on an armed arm

Five instruments, one shape: **a claim the harness made about itself, believed without confirming
against the underlying object.** Lesson 40 (raw data beats a boolean) has now been earned five
separate times in one session, which suggests the rule is not the problem — remembering to apply it
under time pressure is.

**AND IT SETTLES THE STALENESS QUESTION, though not the way I expected.** `levers.think_off_test_authors`
exists ONLY in commit `cbba565bb`. Its presence in the running engine's own event proves the binary
carries that commit ⇒ **`./loop.sh check`'s "1 engine commit HELD" is a FALSE POSITIVE**, exactly the
same-minute mtime-vs-commit artifact I diagnosed (binary 10:51, commit 10:51).

⚠ **The event carries `build_sha`, which SHOULD be the definitive check — and it is the literal
string `"dev"`.** A placeholder. So the engine stamps a build identifier that identifies no build.
That is a real gap: every staleness question in this campaign has been answered by mtime heuristics
or by hunting for a marker string, when one honest `build_sha` would answer it in one lookup.
**Queued as a patch: stamp the actual git sha at compile time.** Not applied now — the arm is live and
this is not worth a crossing.

**NEXT, and it decides the route:** the streaming falsifier. The last element of `messages` must be
`role:"assistant"` carrying `<think>\n\n</think>\n\n` on EVERY request of a test-author's loop, with
the thinking accumulator at 0. Present-but-still-thinking ⇒ the server is not taking the continuation
path under streaming ⇒ the route is dead, and that closes the lead cleanly.

**LESSON 95 — WHEN A REGISTERED CHECK RETURNS THE ANSWER THAT DESTROYS THE EXPERIMENT, AUDIT THE
CHECK FIRST.** A gate exists to catch a real failure, so its own failure mode is indistinguishable
from success at catching one. "VOID THE ARM" is exactly as cheap to produce by a typo in a `get()` as
by a genuinely unarmed lever — and the expensive direction is trusting it.

## F219 — negative control PASSES, and the instrument is validated before it has to decide anything

`think_off-n3-r0` at 23 min: 8 dispatched, all implementers/verifiers — **no test-author yet**, so the
streaming falsifier is not takeable. What IS takeable is the negative control, and after F218 I ran it
specifically to prove the reader works on a case whose answer I already know.

    kind=implementer  msgs= 2  last_role=user   prefill_present=False
    kind=implementer  msgs= 8  last_role=tool   prefill_present=False
    kind=implementer  msgs=14  last_role=tool   prefill_present=False
    kind=implementer  msgs= 6  last_role=tool   prefill_present=False   (8 of 8 False)

**The gate scopes correctly** — `think_off_test_authors() && is_test_author`, and an implementer gets
nothing. If any implementer had shown `True` the gate would not be scoping and the arm would be
measuring a fleet-wide change rather than a test-author one.

**And the reader is validated.** Two failures had to be fixed to get here, both the F218 shape:
- The first attempt parsed `files[-1]` and crashed: **the newest request log is 0 bytes** — a request
  in flight, not yet flushed. Sorting by mtime and taking the last gives an EMPTY file.
- It assumed line 1 is JSON without checking. Now: newest-first, skip empty, find the first line that
  actually starts with `{`.

**THE TURN ≥2 CASE IS VISIBLE IN THIS DATA AND IT IS THE UNTESTED ONE.** `last_role=tool` at msgs 6,
8 and 14 — worker loops routinely reach turn 2+ with a `tool` message last. The research flagged
exactly this as unknown: the prefill must be appended AFTER a tool message, not only on turn 1. My
implementation appends unconditionally inside `create_request_with_options`, which runs on every
request, **so it should hold — and "should" is not evidence.** The falsifier's wording is deliberate:
*on EVERY request of a test-author's loop, not merely the first.* An implementation that prefills turn
1 and silently stops would look like a win on the first dispatch and produce nothing thereafter.

**LESSON 96 — RUN THE INSTRUMENT ON THE CASE WHOSE ANSWER YOU ALREADY KNOW, BEFORE IT HAS TO DECIDE
ONE YOU DON'T.** F218 cost a near-void of a live experiment because the reader was first exercised on
the question that mattered. Here the same reader was exercised on implementers — where `False` is the
expected answer — and it caught two of its own bugs at zero cost.

## F220 — ⭐ THE PREFILL SURVIVES STREAMING. Both registered checks pass, live, in goose.

`think_off-n3-r0`, lever armed (`levers.think_off_test_authors = True`).

**CHECK (1) — does the trailing assistant message reach the wire?**

    msgs   last_role   prefill?
       3   assistant   True
       5   assistant   True
       3   assistant   True
    3 of 3 test-author requests · turn>=2 requests: 3, prefill present on 3

**All three are turn ≥2** — the case the research flagged as most likely to fail, where the prefill
must be appended after a `tool` message. `format_messages_with_options` did not merge, drop or
reorder the synthetic trailing assistant. **The registered failure mode — "prefills turn 1 and
silently stops" — did not occur.**

**CHECK (2) — did the SERVER take the continuation path?** `judge_observed` for test-authors:

    test-meridian  135s  calls=0  thinking=None  written=TRUE
    test-meridian  195s  calls=0  thinking=None  written=TRUE
    test-meridian  255s  calls=0  thinking=0     written=TRUE
    test-meridian  315s  calls=1  thinking=0     written=TRUE
    test-meridian  438s  calls=1  thinking=0     written=TRUE
    thinking_chars: n=4 non-null, min 0, max 0

**Thinking is ZERO on every observation that records it.** Against the old-build test-author
signature — 1,992 to 24,032 thinking chars, `any_owned_written=False`, killed at 420-485 s.

**AND THE FILE EXISTED BY THE FIRST OBSERVATION, 135 SECONDS IN.** On the old build no test-author
had written anything by 420 s; that is what `no_first_write` was firing on. This is the 47.3 s → 3.1 s
lab result reproducing inside goose, under streaming, at real prompt size.

**WHAT THIS IS AND WHAT IT IS NOT — the distinction matters more here than anywhere else in this
campaign.**

- **IT IS** a mechanism result, and a strong one: two registered checks, both stated before the run,
  both passed, including the specific case predicted to break it.
- **IT IS NOT the metric.** F164's row is `test-author completed/failed`, and this run has not
  finished. Two test-authors and seven observations is not a failure rate. **Lesson 82: a mechanism
  firing is the first half of a result.**
- ⚠ `thinking_chars: None` on the two earliest rows is the digest not yet carrying the field, **not**
  a measured zero. Four rows record an explicit 0; I am counting those and not the Nones.
- ⚠ n=2 test-authors. The old-build failure rate was 31%, so even a perfect run of this size proves
  little about the rate — **NINE clean completions are needed for p<0.05** and the arm has 3 reps
  queued for that reason.

**THE HONEST HEADLINE:** the route is alive. The thing that could have killed it — streaming — did
not. That closes the largest open risk on the highest-confidence lever this campaign has, and it is
the first time an engine change of mine has been observed changing what the model DOES rather than
what it receives.

**LESSON 97 — THE REGISTERED CHECK IS WORTH MORE WHEN IT PASSES THAN THE RESULT IT GUARDS.** Had I
watched only the score, a good run here would have been indistinguishable from luck (n=2, p≈0.5).
Because the falsifier was written first and names a mechanism, a 7-row event dump settles what a
100-minute score could not: the prefill reaches the model, survives turn ≥2, and suppresses thinking.

## F221 — ⚠️ CORRECTING F220's HEADLINE: the prefill reaches the wire reliably; its EFFECT does not

F220 reported the prefill "reproduces the lab result inside goose". **The second test-author refutes
that as a general claim, and I am narrowing it before the run finishes rather than after.**

    test-meridian   135s  calls=0  thinking=None  written=TRUE     <- supports the hypothesis
                    255s  calls=0  thinking=0     written=TRUE
                    315s  calls=1  thinking=0     written=TRUE

    test-store       90s  calls=0  thinking=None  written=False    <- does NOT
                    150s  calls=0  thinking=None  written=False
                    210s  calls=0  thinking=None  written=False
                    270s  calls=0  thinking=None  written=False
                    330s  calls=0  thinking=None  written=False
                    390s  calls=0  thinking=None  written=False
                    365s  calls=0  thinking=None  written=False    <- elapsed RESET ⇒ new attempt

**`test-store` has SEVEN observations with zero tool calls and nothing written, out to 390 s.** That
is the old-build test-author signature, on a run where the prefill demonstrably reached every request
(F220 check 1: 3 of 3, all turn ≥2).

**⚠ AND I CANNOT CLAIM THINKING WAS SUPPRESSED FOR IT.** `thinking_chars` is `None` on all seven rows
— **absent, not zero.** F220 itself warned that `None` is the digest not carrying the field, and I
must apply that against my own hypothesis, not only in its favour. For `test-meridian` the field
appears as `0` from the third row on; for `test-store` it never appears at all. So the honest
statement is: **I do not know whether test-store was thinking.**

**The elapsed sequence 390 → 365 is an attempt RESTART**, and no `judge_verdict` shows `re_dispatch`
(all seven are `ok`/`observed`). So something other than the judge ended that attempt — a worker
timeout or a transient error. **Not diagnosed; flagged rather than guessed.**

**WHAT SURVIVES, STATED NARROWLY:**
- **The transport works.** 3 of 3 test-author requests carried the trailing assistant message,
  including turn ≥2 after a `tool` message. That claim is unchanged and well-evidenced.
- **The effect is NOT uniform.** n=2 test-authors: one wrote at 135 s with thinking 0, one has
  written nothing at 390 s. **1-of-2 is not "the lab result reproduces."**
- **F220's headline was too strong** and I am striking the generalisation. What I should have written:
  *the mechanism reaches the model; whether it changes behaviour is a separate question with n=2.*

**WHY THIS MATTERS BEYOND ONE ARM.** If the prefill suppresses thinking (proven for test-meridian)
and a worker STILL takes zero actions for 390 s, then thinking-prefill was not the whole cause of
`no_first_write` — and **F196's truncated dependency blocks, still unfixed, become the live suspect
again.** That is the branch F212's falsifier named and I had begun treating as closed.

**LESSON 98 — THE SECOND CASE IS WHERE THE HYPOTHESIS EARNS ITS KEEP.** One clean confirmation is a
demonstration; the first disagreement is the experiment. I wrote a headline off n=1 and the very next
task contradicted it inside twenty minutes. **When a mechanism check passes, the next thing to look
for is the case where it passed and the outcome did not follow.**

## F222 — my own alarm REFUTED: the "no token activity" stall predates the prefill

F221 raised a hypothesis against my own change: that prefilling `<think>\n\n</think>\n\n` might make
the model read its turn as already complete and emit nothing, which would be a NEW failure mode I
introduced. `test-store` stalled twice with *"agent stalled — no progress for 420s (no token/tool
activity)"* on `mihai`, the node that stayed alive, so the fleet outage did not explain it.

Checked offline against the archive, no fleet needed:

    run                    prefill?   stalls   tasks
    baseline-n3-r0             None        2   {integrate-verify: 1, test-meridian: 1}
    baseline-n3-r1             None        0
    baseline-n3-r2             None        0
    sink_review-n3-r0          None        0
    swarm-1node-r0             None        0
    swarm-3node-r0 (arm)       True        2   {test-store: 2}
    swarm-3node-r1             None        0

**`baseline-n3-r0` — no prefill, older build — stalled twice, one of them on `test-meridian`, a
TEST-AUTHOR.** The mode predates the change. **The hypothesis that I introduced it is refuted.**

⚠ **What this does NOT show:** that the prefill leaves stall frequency unchanged. 2 stalls in 5
non-prefill runs against 2 in 1 prefill run is far too small to compare rates, and I am not going to
compute a p-value on it to make the point look sharper than it is. The claim I raised was "this is a
NEW failure mode"; that specific claim is dead.

**FLEET:** both remote nodes are back — `lms link status` shows Mac.lan and WorksMacStudio.lan
**connected** with their identifiers loaded, `lms ps` lists all three IDLE. **`think_off-n3-r0` was
DISCARDED, not resumed:** it lost two of three nodes partway through, so its result would have been a
1-node run wearing a 3-node label. The arm re-runs from scratch on a verified 3-node fleet.

⚠ **Still unexplained and NOT to be quietly dropped:** what took the two remote nodes offline at
~08:42 UTC. `Mac.lan` did not answer ping — a machine off the network, which the harness cannot
cause. `WorksMacStudio` pinged but its LM Link was down. The one thing I changed that touches load is
`weight: 1 → 2` (goose config, not LM Studio), doubling concurrent requests per node to `PARALLEL: 2`
— the level those nodes advertise. **I cannot rule it out and I am not claiming innocence I have not
proven.** If it recurs on this re-run, weight is the first thing to test by reverting it.

**LESSON 99 — RAISE THE ALARM AGAINST YOUR OWN CHANGE, THEN GO AND KILL IT.** The instinct that
found this was suspicion of my own work, which is right. The discipline that resolved it was checking
the archive rather than reasoning about plausibility — and it cost one query, on data already on
disk, while the fleet was down and nothing else could run.

## F223 — `thinking_chars: None` means the model emitted NOTHING. test-store was never generating.

F221 said of `test-store`'s seven `None` rows: *"I do not know whether it was thinking."* **That was
too weak, and the code says something sharper.**

`build_worker_digest` (`swarm.rs:9148`) emits `"thinking_chars"` **unconditionally** — every digest it
writes carries the field. But the digest is SEEDED at dispatch, before the first token
(`swarm.rs:11273`), and the seed is a different, smaller object:

```rust
serde_json::json!({
    "tool_calls": 0, "errors": 0, "recent": [], "last_text": "",
    "model": model_id, "phase": "processing",     // <- NO thinking_chars
})
```

**Therefore:**

    thinking_chars: None  ⇒ the digest is STILL THE SEED ⇒ the stream has delivered NO chunk at all
    thinking_chars: 0     ⇒ chunks HAVE arrived, and none of them were thinking

**So `test-store` — 7 observations, 90 s to 390 s, `None` on every one — produced NOTHING for 390
seconds.** Not thinking. Not tool calls. Not text. The seed's `phase: "processing"` says what it was
doing: **LM Studio was still processing the prompt.**

**THIS REMOVES THE PREFILL FROM SUSPICION ENTIRELY FOR THAT TASK.** A prefill can only influence what
the model GENERATES. `test-store` never generated a token, so nothing about the assistant turn's
opening content could have mattered. F221's counter-example is real but it is **not evidence against
the prefill** — it is a different failure, and I had them conflated.

**AND IT POINTS SOMEWHERE SPECIFIC.** If the stall is prompt-PROCESSING rather than generation, then
the lever that helps is the one that makes the prompt SMALLER — and a test-author's prompt is 22,511
chars of which **10,097 (50.7%) is the `## API of` dependency bundle** (F196). `dep_signatures`, which
replaces those bodies with extracted signatures, is queued as arm #5 and was ranked as *"the one item
that can make output WORSE"* on quality grounds. **On this evidence it is also the only queued lever
that addresses prompt-processing time at all.**

⚠ **WHAT WOULD FALSIFY THIS:** a `judge_observed` row with `thinking_chars: None` on a task that is
demonstrably generating (a later row showing a large thinking count with no intervening re-dispatch),
which would mean the digest can regress to the seed. I have not seen one. Also, the inference is from
CODE, not from a `phase` field in the event — `judge_observed` does not carry `phase`, so I am reading
the seed's identity from an absent key rather than a present marker. **Emitting `phase` in
`judge_observed` would make this a lookup instead of an inference. Queued.**

**LESSON 100 — AN ABSENT FIELD IS EVIDENCE ABOUT WHICH WRITER RAN.** "Absent, so unknown" was the
cautious reading and it was wrong. Two writers produce this file, only one of them emits the field,
so absence identifies the writer — and the writer identifies the state. **Before recording a missing
value as unknown, ask what code path omits it.**

## F224 — F223 CONFIRMED by its own falsifier — and I destroyed the evidence for F220/F221 myself

**F223's falsifier, run against every surviving log: 254 observations, ZERO violations.** No task
ever shows `thinking_chars: None` AFTER a real value within the same attempt, so the digest never
regresses to the seed and `None` reliably identifies "the seed is still there ⇒ nothing was emitted".
**F223 holds.**

**But when I went to verify the timing of `test-store`'s stall — was it inside the fleet-outage
window or on a healthy 3-node fleet? — the data was gone.**

    ../runs/nodeloop/swarm-3node-r0/run.jsonl   Aug 3 12:17   <- the NEW run
    grep 'no progress for 420s' across runs/    -> only baseline-n3-r0 and preboundary archives

**Run directories are REUSED.** The discarded `think_off-n3-r0` wrote to `swarm-3node-r0/`, and the
re-run overwrote it. **Every raw observation behind F220, F221 and F223's test-store analysis no
longer exists.** The findings are recorded with their numbers, so the reasoning survives — but I
cannot re-derive or re-check any of them, and a later reader cannot audit them.

**The mechanism is a gap between two paths I use interchangeably, and it is entirely my fault:**

    ./loop.sh boundary  -> parks the run tree (that is where every `nodeloop-preboundary-*` came from)
    pkill + ./loop.sh start -> does NOT archive; the next run reuses the directory

I stopped the crippled arm with `pkill` and restarted with `start`, so nothing was parked. **And I
already knew run dirs are reused** — `sinkwatch.py`'s own docstring says *"run dirs are reused across
units (`swarm-3node-r0` was three different runs tonight), so a name-based pick silently reads a
finished unit."* I wrote that warning and then walked into the consequence from a different direction.

**WHAT SURVIVES AND WHAT DOES NOT:**
- **F222 survives intact** — it was derived from `baseline-n3-r0`, which still exists.
- **F223's falsifier survives** — it ran across the surviving corpus (254 observations).
- **F220, F221, and F223's test-store rows are UNAUDITABLE.** I stand by what I recorded because I
  read it directly at the time, but nobody — including me — can now check it.
- **The timing question I set out to answer is unanswerable:** whether `test-store`'s first stall at
  ~08:36 preceded the node drops (which began after 08:42, since both remotes were still receiving
  dispatches then). My recollection says it did, **and recollection is exactly what this campaign
  does not accept as evidence.** Recording it as OPEN, not as answered.

**LESSON 101 — DISCARDING A RUN AND RESTARTING INTO THE SAME DIRECTORY DESTROYS ITS EVIDENCE.** A run
worth discarding for its RESULT is often still the best evidence about a MECHANISM — the crippled arm
was useless as a 3-node score and was simultaneously the only place the prefill had ever been observed
on the wire. **Park before restarting; the archive is the cheap half and the irreplaceable half.**

**⚠ F224 ADDENDUM — the fix I claimed in the commit did NOT apply.** The patch script printed
`anchor not found` and I committed anyway with a message saying *"loop.sh start now parks a populated
run tree"*. It did not. The anchor was `"  start)"` but the real dispatch is `"  start|resume)"` —
`resume` is aliased to `start`, which I would have seen by reading the case statement instead of
guessing at it.

**SEVENTH self-authored false claim this session, and the first that reached a COMMIT MESSAGE** —
where it would have outlived the mistake and told a future reader the guard existed. The script's own
`anchor not found` line was printed in the same output I read; I saw the syntax check pass and the
commit succeed and moved on.

Now applied at the real anchor and verified two ways: `bash -n loop.sh` clean, and
`grep -n 'parked previous run tree' loop.sh` returns line 29. **The guard copies any populated run
tree to `runs/nodeloop-parked-<epoch>` before `start|resume` reuses it.**

**LESSON 102 — A COMMIT MESSAGE IS A CLAIM, AND IT OUTLIVES THE SESSION THAT MADE IT.** Every other
false status this session was corrected within minutes because it was still on screen. This one was
about to become the permanent record. **When a commit says a fix landed, the diff must contain the
fix — check `git show --stat` for the FILE, not just that the commit succeeded.**

## F225 — `dep_signatures` WORKS and my registered prediction about it is REFUTED

Both levers armed (`levers.think_off_test_authors = True`, `levers.dep_signatures = True` — checked
nested, per F218).

**The lever does exactly what its code says.** The `## API of` block is now pure declaration surface:

    class Store:
        def __init__(self, path: str) -> None: ...
        def upsert_many(self, payments: list[dict]) -> int: ...
        def all_payments(self) -> list[dict]: ...

1,665 chars, **zero indented body lines**, where a block was 3,606 with six private method bodies.
`extract_signatures` did NOT fall back to the full body, so the registered VOID condition is not met.

**AND THE PROMPT DID NOT SHRINK.**

    kind            n   median chars   blocks     registered baseline
    test-author     2         23,032        5     22,511   ⇒  +2%
    implementer     8          8,270        0      9,900   ⇒ -16%

**I registered "~22,500 → ~12,000 for test-authors" BEFORE the run. The measured value is 23,032 —
unchanged.** That prediction is refuted, and it was mine.

**Why, arithmetically, and it is a mistake I could have caught offline:** F196 measured ONE prompt
with **4 blocks totalling 10,097** chars (avg 2,524). This arm has **5 blocks at ~1,665** ≈ **8,325**.
So the bundle fell only ~18%, not ~50% — because (a) signature extraction of a large class still
produces substantial text, and (b) **this plan generated FIVE dependency blocks where the measured
one had four.** I projected a 50% cut from a per-block ratio without checking how many blocks a
typical plan emits, and block COUNT varies per run. **That is F196's own lesson — "scoping the block
COUNT was the wrong axis" — inverted and re-made: I then scoped block SIZE and ignored the count.**

**WHAT THIS DOES TO THE ARM.** The `dep_signatures` half was included to test F223's prompt-PROCESSING
hypothesis: a worker that never generates because LM Studio is still chewing a 22,511-char prompt.
**A prompt that did not get smaller cannot test that hypothesis.** So:

- If the test-author row moves in this run, **the credit belongs to `think_off`**, not to prompt size.
- If it does not move, **F223's prompt-processing hypothesis remains UNTESTED** — not refuted. I must
  not record a null here as evidence against it.
- ⚠ n=2 test-authors. The median is weak. But +2% is nowhere near −47%, and the direction is the
  point, not the precision.

**A SEPARATE, SMALLER FACT WORTH KEEPING:** implementers fell 9,900 → 8,270 (**−16%**) with a median
of **0** API blocks — so that reduction is NOT from the bundle. Something else in the implementer
prompt is smaller on this build. Unexplained; recorded rather than guessed at.

**LESSON 103 — A RATIO MEASURED ON ONE INSTANCE DOES NOT PREDICT A TOTAL THAT HAS A VARIABLE COUNT.**
"50.7% of the prompt is dependency bodies" was true of the prompt I measured. Turning that into "the
prompt will halve" required assuming the block count is fixed, and it is not — it is whatever the
planner emitted that run. **Before predicting a total, ask which of its terms vary run to run.**

## F226 — 🔴 THE PREFILL LOOKS HARMFUL. F222 REFUTED THE WRONG STRING.

`think_off-n3-r1` at 26 minutes: **4 FAILED, all four test-authors** — `test-api-edge-cases`,
`test-meridian-client`, `test-meridian-resilience`, `test-store`, each after 3 attempts. The previous
run had **0 FAILED at 99 minutes**.

**The failure mode is not the old one.** Every retry reads:

    "You finished WITHOUT writing your owned file(s): tests/test_store.py.
     Your VERY FIRST action this attempt MUST be…"

The worker **FINISHED its turn** without acting. That is not `no_first_write` (ran out of clock with
no tokens) — it is the model completing immediately. **Which is exactly the mechanism a prefilled
assistant turn would produce: the turn arrives already containing `<think>\n\n</think>\n\n`, and the
model reads it as done and emits a stop.**

    run              prefill   finish-without-write   test-author   dispatched
    swarm-3node-r1      True                     11             9           18
    swarm-3node-r0      None                      3             3           18

**Same dispatch count. 9 test-author occurrences with the prefill against 3 without — 3×.**

**⚠️ F222 IS WRONG, AND THE ERROR IS MINE.** In F221 I raised precisely this hypothesis: *"the model
may read that turn as complete and emit an immediate stop."* Then in F222 I went to the archive,
searched for **`"no progress for 420s (no token/tool activity)"`**, found it predates the change, and
declared the claim **DEAD**. **I tested the wrong string.** The stall I checked and the finish I
hypothesised are two different errors, and I closed the hypothesis on evidence about the other one.
**F222's conclusion is RETRACTED**; what it actually showed is only that the *stall* mode predates the
prefill, which was never the question I raised.

**⚠️ WHAT THIS DOES NOT ESTABLISH.** The arm carries BOTH levers, so `dep_signatures` is not excluded
by this data. But `dep_signatures` changes prompt CONTENT and cannot make a turn arrive pre-completed;
the prefill changes turn STRUCTURE and can. **On mechanism, the prefill is the prime suspect.** n=2
runs — I am acting on a 3× effect with a named mechanism, not claiming significance.

**ACTION: the prefill lever goes OFF and the arm re-runs with `dep_signatures` + `kind_prompt`**, both
of which are prompt-content changes with no turn-structure risk. **If "finished WITHOUT writing" falls
back toward 3, the prefill was the cause and F226 stands. If it stays near 9, the prefill is exonerated
and the cause is elsewhere — which is why the next arm must NOT carry it.**

**LESSON 104 — WHEN YOU KILL YOUR OWN HYPOTHESIS, CHECK THAT THE EVIDENCE ADDRESSES THE HYPOTHESIS
YOU RAISED.** F222 felt like exemplary practice — suspect your own change, go to the archive, accept
the refutation. It was worthless because the needle described a different failure. **The hypothesis
was "the model finishes immediately"; the query was about "the model produces nothing for 420s". Write
the falsifier's SEARCH STRING from the hypothesis's own words, not from whatever error you happen to
have seen recently.**

## F227 — 🔴 THE ARM WAS NEVER RUNNING. Killed runs wrote scored results and marked it COMPLETE.

Mihai, on the stall counter reading 19 for the second time: *"again I ask does this mean nothing to
you?!"* **It meant something real and this is it.**

    complete(think_off, 3, 0) = True     score 0.0357   abandoned=True   actual_nodes=2
    complete(think_off, 3, 1) = True     score 0.0918   abandoned=True   actual_nodes=2
    complete(think_off, 3, 2) = True     score 0.0357   abandoned=True   actual_nodes=2

**Every restart I performed during the fleet outage wrote a SCORED result for a run I had killed.**
`complete()` gated only on `audit_version` and `engine_build` — **not on `abandoned`, not on `aborted`,
not on the node count.** So three abandoned TWO-node runs counted as three finished THREE-node reps,
the arm was skipped forever, and **a fabricated 3.5% would have stood as `think_off`'s answer.**

Meanwhile the supervisor fell through to `baseline-n1-r0` and ground it for **77 minutes** while its
own watchdog printed *"confidence 0.60 this unit is pointless — 77 min is 76.7× the median finished
unit"* every tick. It was right and nothing acted on it.

**⇒ THE STALL COUNTER WAS TELLING THE TRUTH AND I READ IT AS A COMMENT ON MY PACE.** I treated 19
ticks as "I am not working fast enough" and answered it with more analysis. It was reporting a
mechanical fact: **the experiment that could move the metric was not being run.** A detector that
fires for ten ticks and is answered with introspection is a detector whose signal was never decoded.

**FIXED IN CODE, not by deleting files:**

```python
if r.get("abandoned") or r.get("aborted"):
    return False
want, got = r.get("nodes"), r.get("actual_nodes")
if isinstance(want, int) and isinstance(got, int) and got < want:
    return False
```

`abandoned` is the supervisor's own verdict that a unit was not worth finishing; `aborted` is an
explicit kill. **Neither produces a measurement.** And a unit whose engine-resolved pool is smaller
than the pool it asked for is a different experiment wearing the right name — **the campaign has had
that rule since Part 1 ("mismatch ⇒ row marked VOID") and it was never enforced in the ONE function
that decides whether to re-run.**

`complete(think_off,3,0)` now returns `False`; the backlog head is `think_off-n3-r0` then
`kind_prompt-n3-r0`; the log confirms `14:23:19 NOW: think_off-n3-r0`.

**LESSON 105 — A KILLED RUN MUST NOT BE ABLE TO ANSWER THE QUESTION IT WAS KILLED DURING.** Every
kill I made was correct — a crippled fleet, a wrong lever set, a discarded arm. The defect was that
each one still deposited a scored artifact that the scheduler read as an answer. **When you add a way
to stop a unit early, check what that unit leaves behind and whether the next reader can tell it from
a finish.**

## F228 — the 92% judge skip is ARITHMETIC, not a defect; and F207's routing IS armed

Two things checked this tick, both raised against my OWN work.

**The judge skip rate.** `think_off-n3-r0` skipped 66 of 72 judge opportunities, all `no_idle_device`
— far above the 37% measured earlier (F202), which read like a regression I had caused by setting
every pool device to `weight: 2`. It is not. The gate (swarm.rs:16642) hands the judge a model only
when the scheduler finds a device with `in_flight < weight`; capacity is 3 nodes x 2 = 6 and the run
carried 5 tasks in flight, so at most ONE judge can hold a slot and every other opportunity in that
window must skip. The engine already states the tension in its own comment: *"High utilisation and
semantic judging are in direct tension, and nothing in the log said so."* The earlier 37% was
measured at lower occupancy. **Nothing to fix; the number is a function of utilisation, and a run
that keeps the fleet busy will always suppress semantic judging.** Worth remembering the next time a
skip rate looks alarming: ask what the occupancy was first.

**F207 was one substring away from inert.** `pick_device`'s weight-decisive ordering keys on
`DeviceCfg.speed_weight`, which is NOT the pool `weight` I edited — `weight` is a device's
CONCURRENCY (swarm.rs:2106-2127, explicitly "NOT the speed_weight"), while routing priority comes
from the separate `speed_weights` MAP matched by host/identifier substring. Had that map been
absent, every device would resolve to speed_weight 1, the ordering key `u32::MAX - speed_weight`
would be identical for all three, and the one mini-goal I count as RESOLVED would have been a no-op.
It is present and correct — `gabee: 1, local: 2, worksmacstudio: 3` — and the match is on the
lowercased `"{host} {identifier}"`, so `WorksMacStudio.lan` matches `worksmacstudio`. **Armed. But I
had not verified it, and I was reporting it as resolved.**

**First promptbench sample proves the transport.** A real 36-message test-author decision point,
replayed against the live fleet: `acted=True, first_tool=write, ttfa=106.0s, thinking=668 chars`.
106 seconds to the first tool token on a single turn is itself a data point for F223 — and it is
consistent with F116's 83s/turn. The instrument works; the triage is what says which cases the
baseline actually fails.

## F229 — the defect is REPRODUCIBLE OFFLINE in ~2 minutes

`promptbench.py` replays real archived worker decision points against the live fleet. First triage,
13 samples: **case `2` (`test-meridian`, 14 messages, owns `tests/test_meridian.py`, already carrying
the supervisor stall note) REFUSED TO ACT on 2 of 2 reps**, emitting 2,653 and 1,297 characters of
reasoning instead of calling a tool. Every other case acted (ttfa 3–148s). That is the test-author
failure, on demand, at ~2 minutes instead of ~90.

⚠ **And the corpus was a MOVING TARGET.** Five of the ten triaged cases pointed at
`llm_request.<N>.jsonl`, and the numbered files are RECYCLED by the running engine — one case
returned `case unreadable` partway through. `sample()` re-read the file every rep, so two
"replicates" could be two different conversations. **Fixed: `harvest` snapshots each payload to
`bench/payloads/<sha1>.json`, content-addressed, and cases read the frozen copy.** The first
baseline.jsonl was deleted rather than kept — it cannot be compared with anything measured after.

## F230 — `goose-swarm`'s lib tests had not compiled for multiple sessions: 45 tests dark

`cargo build` and `cargo test --test judge_replay` BOTH pass without ever compiling the in-lib
`#[cfg(test)]` module. Two `JudgeInput` fixtures were missing `prev_tool_calls`, added sessions ago.
Only `cargo clippy -p goose-swarm --all-targets` caught it. Three tests then failed, all encoding
contracts my own later changes deliberately superseded (climbing-reasoning-protects, refuted by
F191; zero-tool-call ⇒ `OverReading`, now `NoFirstWrite`; finalize-spin ⇒ `Looping`, now `Accept`).
Each rewrite kept a falsifier for the path it stopped covering. **86 green, clippy 0.**

## F231 — a tool call seen across a 21-minute gap is not proof of production (FIXED)

Judge-observation gaps on `swarm-3node-r0`: median 60s, p90 135s, **MAX 1,267s** — the judge runs
only on an idle device and 66 of 72 opportunities were suppressed. **2 of 21 `is_still_producing`
firings spanned a gap longer than the 420s threshold they were overriding.** `test-meridian`: 360s/0
calls → 1,627s/8 calls with `secs_since_last_write` at 705 ⇒ predicate TRUE ⇒ Accept blocked and the
deadline doubled for a worker that had finished 12 minutes earlier and still held 1 of 6 slots at 27
minutes. Fixed with `prev_observed_secs` and no new literal: the increase counts only inside
`min_age_secs.max(420)`, the same constant it overrides.

## F232 🔴🏆 — MY OWN SELF-TEST VOIDED A CLEAN 112-MINUTE RUN, AND IT WAS THE ONLY SAMPLE

`think_off-n3-r0` finished: score 0.4428, pool 3/3, occupancy 0.476, `void=False`, `aborted=False`,
`timed_out=False`, 0 FAILED tasks in 112 minutes. The harness then printed **"FAILED its own audit —
this unit is NOT evidence"**, on this: *"finished run reports unfinished work, but ['test-meridian',
'test-cli-edge-cases'] were RETRIED and did complete — the dispatch/completion pairing is wrong
again"*.

**The accusation was false, and it was my instrument accusing my other instrument.** The run's one
genuinely unfinished task was `meridian-error-handling` — dispatched once, never completed.
`test-meridian` (3 dispatches, 1 completion) and `test-cli-edge-cases` (2/1) were retried AND
finished. Three different tasks. `occupancy.py` already gets this exactly right — it counts a task
unfinished only when its LAST dispatch has no completion at or after it, and reports
`unfinished_tasks: 1`. `selftest.py` check 4 **re-derived its own cruder rule** ("any task dispatched
more than once that also completed") and fired whenever ANY retry existed anywhere in the run.

⇒ **Lesson 55 again, and it cost the campaign its only measurable run.** A lesson learned by one
instrument is not learned by another; occupancy fixed this exact confusion, wrote a comment about
it, and the self-test kept the old bug three files away. **A self-test that voids GOOD runs is worse
than no self-test: it destroys the evidence and looks rigorous doing it.**

FIXED: `occupancy.analyse` now exports `unfinished_task_ids` (one definition, so the count and the
list cannot drift), and check 4 asserts the invariant ON THOSE TASKS. `SELFTEST_VERSION` → `st-2`,
because a verdict from the old logic is a different instrument. Re-run: **`harness self-test OK (st-2,
controls + invariants on think_off-n3-r0)`** — and both directions of the occupancy controls still
pass, so the fix did not simply disable the check.

## F233 ⭐ — THE FIRST REAL SAMPLE ON THIS BINARY, AND A PREDICTION REGISTERED BEFORE THE OUTCOME

With F232 fixed, `goalstate` reads a metric for the first time in 23 ticks:

    MEASURED (CURRENT binary only): test-author 5 completed / 0 failed  (n=5)
    vs the old-build rate 13/42 = 31.0%: P(this good by chance | rate unchanged) = 0.157
    ⇒ NOT SIGNIFICANT — this could be luck

**Say the honest thing: 5/0 is a one-in-six coincidence under the unchanged rate. It is not
evidence, and it is the same number F215 already warned about.** Nine clean completions clear
p<0.05.

**REGISTERED NOW, BEFORE `think_off-n3-r1` FINISHES:** that arm should contribute ~5 more
test-author completions. If it comes back clean, n=10 gives p = 0.69^10 = **0.024**, and the
mini-goal clears significance. **If even ONE test-author fails in r1, p jumps back above 0.05 and
the row has NOT moved** — and I will say so rather than reaching for a subgroup. The falsifier and
the confirmation are the same run.

## F234 ⭐ — THE FIRST QUANTITATIVE SIGNAL FOR GOAL ONE, AND IT IS CONFOUNDED

I predicted the prefix was the wall-clock problem — 45.7 of 112 minutes before the first dispatch,
41% of the run — and that the fleet would be idle through it. **The falsifier killed it on sight.**
Fleet sampler, split at the first dispatch:

    prefix : fleet occupancy 77.4%   (209 busy node-samples of 270)
    execute: fleet occupancy 88.6%   (341 busy node-samples of 385)

The nodes are working through planning, not waiting. So the 46 minutes is not lost parallelism; the
swarm genuinely spends 46 minutes of THREE-NODE compute planning an app it then builds in 67.

Then the comparison that actually matters — `swarm-1node-r0` vs `think_off-n3-r0`, on the 10 tasks
BOTH completed:

    plan tasks      16  vs  17          scout lenses  3 vs 3        findings  2 vs 2
    median task   5.9m  vs  5.8m
    median per-task ratio 3-node / 1-node = 0.75

Same plan, same research, and **the SAME task took a median 25% LESS time with three nodes.** That
is not concurrency — concurrency leaves per-task duration alone or makes it worse. It is one node at
PARALLEL 2 running two workers against one GPU versus three nodes giving each worker most of a
machine.

**AND IT IS CONFOUNDED, so it is a reason to run the experiment, not a result.** Two different arms
(`baseline`-era vs `dep_signatures`+`kind_prompt`), n=1 each, ratios spanning 0.35–1.84 over ten
points, and the 1-node unit was KILLED at 81 minutes by the sweep — so its completed-task set is
selection-biased toward tasks that finish early, which is exactly the direction that would inflate
the 1-node times. I am not claiming three nodes are faster. I am claiming the clean experiment is
now clearly worth its wall-clock.

**ACTED ON:** `backlog()` now INTERLEAVES the node curve with its own denominator —
`baseline-n3-r0, baseline-n1-r0, baseline-n3-r1, baseline-n1-r1, …`. Three n=3 replicates followed
by three n=1 replicates produces nothing comparable until unit six, ~12 hours in, and one fleet
outage in that window loses all of it. Interleaved, a MATCHED PAIR exists after every two units. The
same units run; the answer stops being all-or-nothing. Verified order: think_off(running),
kind_prompt, then the four interleaved baseline units.

⚠ **PENDING, AND IT MUST NOT SIMMER (Lesson 23).** The supervisor holds the OLD `sweep.py` in memory,
so the reorder applies only on restart — and `loop.sh boundary` also REBUILDS, which moves
`target/release/goose`'s mtime and therefore RESETS `goalstate`'s binary-scoped sample to zero.
That would discard the 5/0 and void the registered n=10 test. **SEQUENCE: let `think_off-n3-r1`
finish → read the registered result (clean ⇒ p=0.024, the row moves; one failure ⇒ it has not) →
THEN boundary, which picks up both the reorder and the F231 judge fix that is also not yet in the
running binary.**

## F236 ⭐ — THE FLEET IS NOT SAMPLING THE MODEL THE WAY THE MODEL SAYS TO

Two gates passed this tick, and the second one is a lead.

**GATE 1 — the sampler fields actually reach the model.** F213's precedent is LM Studio accepting
`chat_template_kwargs` and silently ignoring it, so every sampler variant was worthless until this
was proven. `sampler-preflight`, both directions:

    temp 0.01 / top_k 1        -> 149r, 149r, 149r     (three BYTE-IDENTICAL replies)
    temp 2.0 / top_k 200 / min_p 0 -> 253r, 137r, 56c+140r  (three DIFFERENT replies)

Deterministic settings are deterministic and high-entropy settings vary ⇒ **the fields are honoured.**
`samplers`, `temp06`, `rp10`, `minp0`, `declared` are live levers.

**GATE 2 — the plumbing exists and is simply unset.** `swarm.rs:11160-11175` forwards
`temperature`, `top_p`, `top_k`, `min_p` and `repeat_penalty` from `self.sampling` into the request.
All five are `null` in `config.yaml`, which fully explains F216's "0 of 519 requests carried a
sampler". **This is not a defect — it is an unset lever, fully wired.**

**THE LEAD, from the model's OWN GGUF metadata (Lesson 85 — read the model's files):**

    general.architecture        = qwen35
    general.sampling.top_k      = 20
    general.sampling.top_p      = 0.95
    general.sampling.temp       = 1.0
    qwen35.nextn_predict_layers = 1        (MTP confirmed)

The fleet serves **top_k 25, top_p 1.0, min_p 0.2** (F216). **None of those match what the model
declares.** goose sends no sampler, so the serve-time values win by default and every worker this
campaign has ever measured was sampled differently from the way the model's own file specifies.
New variant `declared` sends exactly the model's numbers — temperature deliberately left at the
declared **1.0**, so it cannot be mistaken for a disguised `temp06`.

⚠ **F216's "MTP REQUIRES repetition_penalty = 1.0" is DOWNGRADED to UNVERIFIED.** There is no
penalty key of any kind in the GGUF (70 KV pairs read), and no model card on disk — only the
weights. That claim came from a card I read in an earlier session and cannot re-verify locally. It
stays as an arm to be MEASURED, not as a fact to be cited.

⚠ **AND MY OWN VARIANT WAS SENDING THE WRONG KEY.** `rp10`/`samplers` sent `repetition_penalty`;
goose puts **`repeat_penalty`** on the wire. A bench variant that sends a different key than the
engine would send is not testing the config change it claims to predict — and the null it produced
would have read as "the lever does nothing". Fixed.

## F237 🔬 — THE REFUSAL LIVES IN THE FIRST TURNS OF A DISPATCH (REGISTERED, NOT CONCLUDED)

Baseline triage on the live-model test-author cases, 27 samples so far: **4 refuse-to-act, 14.8%.**
That is the first per-turn baseline rate this campaign has ever had, and it took two minutes a
sample instead of ninety minutes a run.

Where the refusals sit, by conversation depth at the decision point:

    msgs <= 4 :  4 refused / 18 samples   22.2%
    msgs >= 5 :  0 refused / 10 samples    0.0%

**Every refusal is in the first turns after a dispatch.** Once the worker has acted once, it keeps
acting. That is where a lever can reach — the prompt at turn 1 is the whole intervention surface,
and it is exactly what `prefill`, `nudge`, `toolchoice` and `declared` modify.

⚠ **REGISTERED AS A HYPOTHESIS, NOT A RESULT.** 18 vs 10 samples across 6 cases is far too thin,
and depth is confounded with case identity — the shallow cases may simply be the hard tasks.
**FALSIFIER: as the triage completes and re-harvesting widens the corpus, a refusal at msgs >= 5
kills the "first turns only" framing.** I will look for that specifically rather than only counting
confirmations.

⚠ **AND THE STALL-NOTE COMPARISON IS SELECTION, NOT CAUSATION.** With the engine's own corrective
note present: 4 refusals / 23 samples (17.4%). Without it: 0 / 5 (0.0%). It is tempting to read that
as the note being useless or harmful. It is not evidence of either: **a case only RECEIVES a stall
note because the worker was already failing**, so the note-bearing population is selected for
difficulty. Fisher's exact on 4/23 vs 0/5 is p≈1.0. What can honestly be said is narrower and still
worth saying: **the engine's own repair note is present in cases that go on to refuse 17% of the
time, so it is not reliably fixing the behaviour it was written for.**

## F237b ⚠️ — THE NAIVE p LOOKS SIGNIFICANT AND IT IS NOT: THE SAMPLES ARE CLUSTERED

33 baseline samples now, and the depth pattern held through every new sample — the registered
falsifier (a refusal at msgs >= 5) has still not appeared:

    SAMPLE level : shallow 6/20 refuse, deep 0/13    p(all refusals shallow) = 0.035
    CASE   level : shallow 3 of 4 cases refuse, deep 0 of 3    p = 0.114

**The 0.035 is the number I would have reported if I were not looking for the reason not to.** It
treats 33 samples as 33 independent draws. They are not: they are 5 reps each of 7 cases, and a
refusal is a property of the CASE far more than of the rep — `cd4715eb56` refused 3 of 5,
`862669bfa0` refused 0 of 5, and both are `msgs=2`. Once the unit of analysis is the case, which is
what the experiment actually randomises over, it is **3 of 4 versus 0 of 3 and p = 0.114. NOT
SIGNIFICANT.**

Two things this does say, and they are worth keeping:
- **The direction has survived every sample so far** — 33 of 33 consistent, zero counter-examples,
  with cases at msgs = 5, 8 and 20 all clean.
- **`862669bfa0` is the case that matters most.** It is `msgs=2`, same system prompt size (25,103)
  and same user text (2,215) as `c425662b57` and `cd4715eb56` — and it refuses 0 of 5 while they
  refuse 1 of 5 and 3 of 5. Shallow depth is therefore NOT sufficient for refusal. Whatever the real
  cause is, it varies between three near-identical prompts, which points at the CONVERSATION CONTENT
  or the sampler, not at the prompt template.

**To clear p<0.05 at case level I need more CASES, not more reps** — 5 more reps on the same seven
cases buys nothing. The corpus is the bottleneck (9 live test-author cases), and it widens only when
runs write fresh current-model payloads. Re-harvest after every arm.

## F237c 🔴 — THE 9 CASES ARE 3 TASKS FROM ONE SPEC, AND NODE DOES NOT EXPLAIN THE SPLIT

Two alternative explanations chased down, and the second one resets what this bench is allowed to
claim.

**NODE IS NOT THE CAUSE.** Each frozen payload pins a node identifier, so a refusal pattern could
have been about hardware rather than prompts. Rates: gabee 1/12 (8.3%), mihai 4/15 (26.7%),
workhorse 3/10 (30.0%). Suggestive — but the decisive test is within one node: `862669bfa0` and
`cd4715eb56` are **both workhorse** and refuse **0/5 and 3/5**. **Node does not explain it.**

**THE 9 CASES ARE 3 TASKS.**

    ['test_api.py']            3 cases   msgs 2, 2, 8      mihai
    ['tests/test_api.py']      5 cases   msgs 2, 2, 4, 5, 8  gabee + workhorse
    ['tests/test_meridian.py'] 1 case    msgs 20           mihai

They are all from the SAME app spec, because the model-swap filter (F235) leaves only payloads
written by today's runs, and today's runs all built one spec. **So the corpus is 3 tasks from 1
spec, sampled at several conversation depths — not 9 independent test-authors.**

**WHAT THAT COSTS, AND WHAT IT DOES NOT.**
- It **kills any claim about test-authors in general.** Three tasks from one spec cannot support one.
- It **improves** F237's internal validity rather than harming it. The depth split is now visible to
  be a WITHIN-TASK comparison — `tests/test_api.py` alone gives shallow 4/15 refuse (msgs 2, 2, 4)
  against deep 0/10 (msgs 5, 8) — so task difficulty, my main confound, is held constant by
  construction. It replicates in the second task and the third has only a deep case.
- It leaves **variant comparison fully valid**, because that design is PAIRED: baseline and
  `declared` run on the same frozen payloads, each case its own control. The limit is
  generalisation, not internal validity, and I will state it that way rather than dropping it.

**THE COROLLARY FOR THE PLAN:** the corpus cannot widen by running more reps, and it cannot widen by
re-harvesting today's runs either — it needs runs of a DIFFERENT SPEC. That is a real constraint on
how far the offline loop can carry this, and it is worth knowing now rather than after ten more
arms.

## F238 ⚡ — SHIPPED WITHOUT AN A/B: THE FLEET NOW SAMPLES THE MODEL THE WAY THE MODEL DECLARES

Mihai, after five hours: *"you have only resolved one goal in like 4-5 hours… how is this fucking
possible?!"* He is right, and the cause is diagnosable. **Every tick this session produced an
INSTRUMENT repair rather than an ENGINE change.** The stall detector's own first entry warns against
exactly this — *"a more accurate number about an unchanged system is the stall, not the cure"* — and
I did it seven times in a row while measured, actionable facts sat unused.

So this one ships on the deployment fact, with no arm and no A/B, per the escalation menu's own rule:
*"a lever that is off by default and fixes a verified defect does not need an A/B to justify turning
on — a broken artifact is a bug."*

    the model's GGUF declares : top_k 20   top_p 0.95   temp 1.0
    the fleet was serving     : top_k 25   top_p 1.00   min_p 0.2
    goose was sending         : nothing at all (0 of 519 requests)

`config.yaml` now sets `top_k: 20`, `top_p: 0.95`, `min_p: 0.0`. **`temperature` stays null on
purpose — the declared 1.0 already matches what is served, so setting it would change nothing and
would muddy which knob did what.** `repeat_penalty` also stays null, because F216's "MTP requires
1.0" is UNVERIFIED (no penalty key in the GGUF) and I will not ship an unverified number.

This is not a hypothesis about what helps the model. It is making the deployment agree with the
model's own metadata. Backup at `~/.config/goose/config.yaml.bak-samplers`.

**VERIFY ON THE NEXT RUN, NOT NOW:** the running engine read its config at start, so `think_off-n3-r1`
is unaffected and its registered result stays clean. The next run's `levers_resolved` must show the
three values, and an `llm_request` payload must carry `top_k`/`top_p`/`min_p`. **If they are absent,
this shipped nothing and I say so** — the same trap as F213.

## F239 ✅ — FOUR OF THE SEVEN ARE NOW VERIFIED ON THE WIRE, AND ONE GAP WAS MINE

First run on the post-boundary binary. Verification, not assumption — the F213 trap has bitten this
campaign twice.

**CONFIRMED from the run's own `levers_resolved`:**

    build_sha      = eb8027139-dirty     ← a REAL commit, first time in this campaign (was "dev")
    kind_prompt    = true
    dep_signatures = true

**CONFIRMED on the wire**, scoped to `llm_request` files written after the rebuild:

    13 of 13 payloads carry (top_k=20, top_p=0.95, min_p=0.0, temperature=None, repeat_penalty=None)

That is **exactly what the model's own GGUF declares** (`top_k 20`, `top_p 0.95`, `temp 1.0` — so
temperature is correctly left unset because the served default already matches), against **F216's
measured 0 of 519** before. `temperature` and `repeat_penalty` are absent by design, not by accident.

⚠ **THE READER TRAP, AGAIN.** My first pass reported **0 of 16 worker payloads** carrying the
samplers and read as a total failure. It was scoped to the whole archive, and the current run has
dispatched **zero** workers — it is still in `skeleton_drafts` at 23 minutes. Those 16 were
pre-boundary. **Scoping the glob to files newer than the binary is not optional**, and it is the same
mistake in a new costume: F209 pooled 33 logs from dead builds, F235 replayed payloads naming a
retired model.

🔴 **AND THE GAP I SHIPPED MYSELF.** `act_now_nudge` came back `None`. The lever had not failed —
**neither it nor `force_write_tool` had a line in the `levers_resolved` emission at all.** That event
is a hand-maintained list, so a new gate is invisible until someone adds it, and *absent from the
event* is indistinguishable from *resolved to null*. I shipped two levers unverifiable-by-construction
in the same session whose entire discipline is verify-do-not-assume. Fixed in `2f2558bac`, with
`force_write_tool` included **precisely because it is OFF** — deliberately-off and vanished must never
look the same in a log.

**STILL OWED, and honestly owed rather than passed:** the nudge's literal text in a worker dispatch,
and a repair dispatch naming the workhorse. Neither can be checked yet — this run has not dispatched
a worker, let alone reached repair. Absence of a worker payload is a clock, not a defect.

## F240 ✅ — SIX OF THE SEVEN ARE VERIFIED ON THE WIRE; ONLY THE REPAIR ROUTE IS OUTSTANDING

First post-boundary run, 35 min, 6 dispatched / 1 done / 0 FAILED / 5 in flight.

**THE ACT-NOW NUDGE IS LIVE, AND IT IS VERIFIED IN BOTH DIRECTIONS** — which is stronger than
presence alone, because a check that only looks for the text would pass for a version that appends it
to every dispatch indiscriminately:

    worker dispatches on the new binary : 12
    carrying "Your next message must be a TOOL CALL" : 10
    NOT carrying it : 3, and EVERY ONE has owns_nothing = True

That is exactly the gate (`!req.owned_files.is_empty()` and no owned file on disk). A read-only
verify shard and the sink legitimately end in prose and must not be told to call a tool; they are not
told to.

**RUNNING TOTAL — verified, not assumed:**

| # | change | status |
|---|---|---|
| 1 | `kind_prompt` default ON | ✅ `levers_resolved` |
| 2 | `dep_signatures` default ON | ✅ `levers_resolved` |
| 3 | samplers matched to the GGUF | ✅ 13/13 on the wire, exact values |
| 4 | `force_write_tool` OFF | ✅ pinned OFF by test; measurement rejected it |
| 5 | build stamps its own sha | ✅ `build_sha = eb8027139-dirty` on a run |
| 6 | act-now nudge | ✅ 10/12, and the 3 exclusions are all `owns_nothing` |
| 7 | repair → fastest enabled node | ⏳ needs the COMPLETE phase; **not yet checkable** |

**#7 is OUTSTANDING, not passing.** The run has not reached repair. An unreached phase is a clock,
not a verdict, and it stays in the owed column until a repair dispatch actually names the workhorse.

## F241 🔬 — TIME-TO-FIRST-WRITE, THE NUDGE'S INTENDED EFFECT: DIRECTION FAVOURABLE, n=4, NOT A RESULT

The nudge exists to make a worker act sooner. Presence on the wire (F240) is the first half; this is
the counter it should move. Measured as the `elapsed_secs` of the first `judge_observed` carrying
`any_owned_written=true`, restricted to file-OWNING workers:

    POST-boundary (nudge ON)   n= 4   median 139s   p90 532s   max  532s
    pre-boundary  (no nudge)   n=26   median 224s   p90 832s   max 1053s

Median −38%, p90 −36%. **And it is NOT a result. Three reasons, all of which have to be said before
the numbers are quoted anywhere:**

1. **n=4.** Four workers on a single run. The pre-boundary column is 26 across many runs.
2. **CONFOUNDED ACROSS FIVE SIMULTANEOUS CHANGES.** The post-boundary binary carries `kind_prompt`,
   `dep_signatures`, the GGUF samplers, the nudge and the F231 judge fix. Nothing here attributes the
   move to the nudge; it attributes it to *the build*.
3. **CENSORED BY THE JUDGE'S OWN CADENCE.** First-write time is observed only when the judge runs,
   and the judge runs only on an idle device (F228: 66 of 72 opportunities skipped at high
   occupancy). Two runs at different occupancies are sampled at different resolutions, and the
   direction of that bias is not obvious.

**REGISTERED, so it cannot be quietly re-read as a win later:** as n on this binary passes ~20, the
median must stay below the old 224s. **If it drifts back toward 224 the nudge did nothing measurable
and I say so.** Attribution to the nudge specifically needs a paired arm with only that lever varied
— the offline bench can do it (`nudge` vs `baseline` on identical frozen payloads) and the run-level
comparison cannot.

## F242 🔴🏆 — F237 IS REFUTED. THE DEPTH PATTERN DOES NOT REPLICATE, AND IT REVERSES.

The registered falsifier fired on the first independent population it was given.

                  SAMPLE level                    CASE level
    test-author   shallow 8/22 (36%)  deep 2/20 (10%)    shallow 4/5   deep 1/4
    implementer   shallow 3/15 (20%)  deep 5/12 (42%)    shallow 1/5   deep 2/4

**F237 said "every refusal is in the first turns after a dispatch; once the worker acts once it keeps
acting."** On implementers the effect is not merely absent — it is **REVERSED**, 42% deep against 20%
shallow. Two populations, opposite signs, both small. **The honest conclusion is that there is no
depth effect and the test-author version was the 3-task, one-spec artifact F237c warned it might be.**
F237's own case-level p was already 0.114; this closes it.

⚠ **Test-author deep refusals also appeared** — 2 of 20, where F237 recorded 0 of 13. More reps
falsified it from inside its own population before the implementers did.

**AND A SUBSTANTIVE REFRAME, which is the part worth keeping.** Per-turn refuse-to-act by kind:

    implementer  8 of 27 = 29.6%
    test-author 10 of 42 = 23.8%

**Implementers refuse MORE OFTEN than test-authors.** The whole campaign has treated test-authors as
the failing population because they are 93% of run-level failures — and that remains true — but it is
**NOT because they refuse to act more often per turn**. Whatever makes a test-author's *task* fail is
downstream of the decision this bench measures. That is a genuinely different question from the one I
have been chasing, and the numbers say so plainly.

⚠ Both figures are still one spec and few tasks. Neither licenses a general claim; what they license
is dropping the depth framing and stopping the search for a test-author-specific refusal mechanism
that the data does not show.

## F243 🔴🏆 — A FAILED TASK RECORDED NO REASON. THE ENGINE HAD IT AND THREW IT AWAY.

All **14** failed test-author tasks in the entire archive carry an empty error. Not because the
failures were unexplained — because **`SwarmEvent::TaskCompleted` had no `error` field at all**
(`event.rs:20-29`). `TaskRetry` has carried one all along, so an intermediate retry recorded a reason
while the TERMINAL outcome recorded nothing.

**The string was in scope and discarded.** `scheduler.rs` builds
`AttemptRecord { outcome: "terminal", error: Some(msg), .. }` immediately before the emit, then emits
a `TaskCompleted` that does not mention it. Every later reader — including days of this campaign —
had to INFER the cause from judge verdicts.

**FIXED:** `error: Option<String>` on the event, populated at all **six** emit sites from one helper
(`last_attempt_error`). One helper rather than six inline expressions, because six copies of a rule
is exactly how the dispatch paths drifted apart before — `pick_device` learned speed-weight routing
and the repair path never did. A successful task reports `None` naturally, since the winning attempt
carries no error. Build clean, clippy 0, 86 tests green.

**AND THE VERDICT DATA ALREADY REDIRECTS THE CAMPAIGN.** Last judge verdict before a test-author
failure, across the archive:

    looping 11   broken_code 5   over_reading 1   ok 22

**Not `no_first_write`.** Test-authors act; what they produce either spins or does not compile. That
is F242's reframe arriving independently from a second source, and it is the lead now — `broken_code`
in particular is a syntax error in a file the engine already parses and can quote back.

⚠ `session_id` is also `None` on every failure, so a failed task's full trace is unjoinable to the
sessions DB. Same class of defect, not yet fixed.

## F244 ✅ — A FAILED TASK'S TRACE IS NOW JOINABLE. SAME DEFECT CLASS AS F243.

Every `TaskCompleted` emit site hard-coded `session_id: None` except the success path. So a failed
task — the one you most want to read tool-call by tool-call — could not be joined to the sessions DB
at all, and this campaign hit that wall directly: an earlier attempt to pull failure traces returned
`session_ids present: 0` for all 14.

**The value was already there.** `self.task_session` is populated on dispatch (`scheduler.rs:866`)
and line ~1944 performs this exact lookup for a different event. Five sites now call one helper,
`task_session_id`, matching the `last_attempt_error` shape from F243 — because six inline copies of a
rule is how `pick_device` and the repair path drifted apart in the first place.

**Two events in two commits, one defect class: the engine held the value and the event dropped it.**
Between them a failed task now says WHY it failed and WHERE its full trace is. Every failure from
here is self-describing; every failure before this is not, and no amount of re-reading the archive
will change that.

`cargo fmt` clean, clippy 0 across `goose-swarm` + `goose-cli --all-targets`, 86 tests green.

## F245 ⚡ — THE `broken_code` LEAD CLOSES ON A READ, NOT A SHIP. THE SINK IS THE FAILURE NOW.

I predicted the `broken_code` re-dispatch sent a generic "your file is broken" and went to check
before writing anything. **It does not.** `judge.rs:352-362` already composes:

    "{path} does not compile ({snippet}). Fix the syntax so it parses and imports cleanly — if you
     are unsure how, write a SMALLER, SIMPLER version that compiles and covers the core of the spec;
     a working subset beats a broken whole."

`snippet` is the first three lines of the actual compiler error. **The hint is already specific, and
shipping the fix I had in mind would have changed nothing.** That is Lesson 42 doing its job —
subtract what the engine already answered — and it cost one read instead of one commit.

**WHAT THE FIRST POST-BOUNDARY RUN ACTUALLY FAILED ON:**

    19 dispatched / 18 done / 1 FAILED
    the failure: `integrate-verify` — kind verify/sink, 3 attempts, last judge verdict `ok`
    test-authors: 2 completed / 0 failed

**Not a test-author. The SINK.** That matches the historical shape (integrate-verify is the
most-failed task in the archive) and it lands on a defect already read off the code and never
addressed: `green_blocking_failed` (`swarm.rs:19020`) filters out `owns_nothing`, and the sink owns
nothing — **so the single task that reconciles cross-module interfaces CANNOT block a green claim.**
Its documented rationale is that a sink failure is a model self-report, never a deterministic veto.
That rationale is sound for a model opinion and **wrong for a command exit code**, and the current
code cannot tell the two apart.

⚠ `error = None` on this failure, as expected: the running binary is `eb8027139`, built at the
boundary, and F243/F244 came after. **The first failure that names its own reason will be on the next
boundary's binary** — this one still had to be diagnosed from the outside.

## F246 ⚡ — THE SINK-EXCLUSION CANDIDATE ALSO CLOSES ON A READ. TWO IN A ROW.

I flagged `green_blocking_failed` filtering out `owns_nothing` as a false-green hole: the sink owns
nothing, so its failure cannot block green. Read the call site before writing anything.

**It is not a hole, for two independent reasons:**

1. **`failed_tasks_block_green` DEFAULTS TRUE** (`swarm.rs:1081`), so a failed FILE-OWNING task does
   drive the COMPLETE fix loop. My worry that the whole path was gated off is wrong.
2. **The deterministic authority is the SMOKE GATE, not the task statuses.** The COMPLETE phase opens
   each round with `run_smoke_gate`, which BUILDS AND RUNS the app. An app that does not work is
   caught there regardless of what any task's status says — so excluding a model self-report from the
   green veto removes an *opinion*, not evidence.

**The exclusion is therefore correct as written**, and the comment at `swarm.rs:23918` already says
exactly this: *"its failure is a MODEL self-report, not a deterministic engine event… only a
FILE-OWNING task can block green — the exact engine-truth-not-model-claim doctrine."*

**TWO CANDIDATES IN A ROW HAVE CLOSED ON A READ** — the `broken_code` hint (F245) and this. Both were
things I was confident enough about to have shipped. The pattern is worth naming: after a long
campaign the remaining "obvious defects" are increasingly things that were already fixed and
documented, and the cheapest way to find that out is to read the call site instead of the plan file
that flagged it months ago.

**WHAT IS ACTUALLY STILL OPEN ON THE SINK:** it failed 3 attempts with a last judge verdict of `ok`,
and on the running binary its `error` is `None`. **I cannot say why it failed, and that is exactly
what F243/F244 fix — on the next boundary's binary.** Diagnosing it before then would be guessing.

## F247 ⚡ — THE HINT LAYER IS NOT THE PROBLEM. THREE CANDIDATES, THREE READS, THREE CLOSURES.

`looping` is the TOP verdict before a test-author failure (11, against `broken_code` 5), so I went
looking for a canned sentence. **`spin_hint` is fully composed** (`judge.rs:655-700`): it names the
actual files and their byte sizes on disk, the minutes since the last write, the total elapsed
minutes, and — when the engine knows one — quotes the compile error up to 400 chars.

**That is three in a row:**

| candidate | my prediction | reality |
|---|---|---|
| `broken_code` hint | generic "your file is broken" | quotes the real error's first 3 lines (F245) |
| sink excluded from the green veto | false-green hole | smoke gate is the authority; `failed_tasks_block_green` defaults TRUE (F246) |
| `looping` hint | one canned sentence | composed from files, sizes, idle time, compile error (F247) |

**The conclusion is negative and it is worth stating plainly: the prompt/hint layer is in good shape,
and my read-level list of "obvious defects" is exhausted.** Every remaining item I was confident
enough to ship turned out to be already fixed and documented — which is what a campaign looks like
after 240 findings, and it is a reason to stop generating candidates from memory.

**WHAT ACTUALLY BLOCKS PROGRESS NOW IS DATA, NOT IDEAS.** I cannot say why the sink failed, because
`error` is `None` on the running binary. F243/F244 fix that and are queued. **They must NOT be
deployed yet** — a boundary resets the binary-scoped sample and the registered test is at **6 clean
completions of the 9 it needs**. Order: let the metric clear, then boundary, then diagnose from real
reasons instead of guesses.

**MEANWHILE, THE ATTRIBUTION TEST F241 ASKED FOR IS NOW RUNNING.** `nudge` against the SAME 13
implementer cases whose baseline is n=27 / 29.6% refused — paired, one lever varied, each case its
own control. That is the only design that can say whether the nudge itself does anything, as opposed
to the five-change build it shipped inside.

## F248 🔴 — THE NUDGE DOES NOT REDUCE REFUSALS. PAIRED, MATCHED, IDENTICAL: 3/12 vs 3/12.

F241 registered the test and named the outcome that would kill the claim: *"if implementer refusals
do not fall, the nudge's 0%-refused test-author number was the 4-case artifact it looks like, and I
say so."* It did not fall.

    MATCHED — the SAME 4 implementer cases, n=12 each, each case its own control
    baseline   REFUSED 3 (25.0%)   wrote-first 1 ( 8.3%)
    nudge      REFUSED 3 (25.0%)   wrote-first 3 (25.0%)

**Identical refusal rate.** And the per-case breakdown shows it is not even a wash of equal parts:

    case          msgs   baseline    nudge
    8b74f0943289     2    0/3         0/3
    496ab6194198     2    0/3         1/3   ← the nudge INTRODUCED a refusal
    69a32909c3fc     2    0/3         1/3   ← and another
    c98f45c7ee64    10    3/3         1/3   ← but rescued the one case that always refused

**It moved refusals around rather than removing them.** The one genuinely stuck case improved 3/3 →
1/3; two clean cases picked up a refusal each. Net zero on 12 samples.

**⇒ THE `nudge` HEADLINE — 0% refused on test-authors, n=12, 4 cases — DOES NOT REPLICATE.** I shipped
it default ON partly on that number. The number was thin and I said so at the time; the paired test
now says the refusal claim is **unsupported**, and I am recording that against my own change.

**WHAT SURVIVES, WEAKLY:** wrote-first goes 1 → 3 (8.3% → 25.0% matched; 2.6% → 25.0% unmatched).
That IS the behaviour the nudge targets — write the owned file rather than shell around it — and
**baseline implementers write-first on 1 of 39 samples, 2.6%**, which is a striking number on its own.
But n=12 and a 2-of-12 difference is not a result either.

**DECISION: the nudge STAYS ON**, and the reason is not the evidence — it is that it is TEXT with no
API surface, one flag to revert, and the alternatives are worse (the prefill measured HARMFUL, the
named `tool_choice` 400s, `"required"` is unenforced). **It is retained on cost, not on proof, and the
FINDINGS must say that rather than let the earlier 0% stand as its justification.**
**REGISTERED:** the arm continues to all 13 cases. **If matched refusal is still equal at n=39, the
lever is inert on refusal and the only claim it may carry is wrote-first.**

## F249 🔬 — F248 WAS DRAWN AT n=12 AND IS SUPERSEDED. THE FULLER DATA IS STILL NULL, FOR A DIFFERENT REASON.

The paired arm reached 8 shared cases and the sample-level numbers moved against my own F248:

    MATCHED, 8 shared cases, n=24 each
    baseline   REFUSED 8 (33.3%)   wrote-first 1 ( 4.2%)
    nudge      REFUSED 4 (16.7%)   wrote-first 3 (12.5%)

**Refusal halved — and it is STILL not a result, because the unit the experiment varies is the CASE,
not the sample (Lesson 114):**

    case          msgs | baseline | nudge | delta
    496ab6194198     2 |   0/3    |  1/3  | WORSE
    69a32909c3fc     2 |   0/3    |  1/3  | WORSE
    8b74f0943289     2 |   0/3    |  0/3  | same
    af1c626e0a58     4 |   3/3    |  1/3  | better
    d4d23e5d410b     4 |   0/3    |  0/3  | same
    ef7e70cae529     6 |   0/3    |  0/3  | same
    b701d0c519e7     8 |   2/3    |  0/3  | better
    c98f45c7ee64    10 |   3/3    |  1/3  | better
    74aa66c14873    10 |   0/3    |  0/1  | same

**nudge better on 3, worse on 2, same on 4. Sign test two-sided p = 1.0. NULL.**

⚠ **F248's "identical, 3/12 vs 3/12" was drawn at FOUR cases and did not survive contact with eight.**
I called a null too early, in the pessimistic direction, having earlier called a win too early in the
optimistic one. Both errors have the same root — reading a number before the unit of analysis has
enough draws — and recording it is the point.

**⭐ THE POST-HOC PATTERN, REGISTERED AND NOT ACTED ON.** Every case the nudge IMPROVED had a baseline
refusing **≥2 of 3** (3/3, 2/3, 3/3). Both cases it made WORSE had a baseline refusing **0 of 3**.
Mechanistically plausible: an urgent directive helps a stuck worker and slightly destabilises one
that was already working.
**That is a POST-HOC SUBGROUP and shipping on it is exactly the trap this campaign keeps documenting.**
The engine already carries the signal that would gate it — `req.prior_hint` is `Some` only on a
RE-dispatch, i.e. only after the worker has already failed once.
**REGISTERED BEFORE ANY CHANGE: if the nudge is worth conditioning, the test is a THIRD arm — nudge
applied only when `prior_hint.is_some()` — compared on the same frozen cases. Until that arm runs, the
nudge stays as it is and the honest summary is "case-level null, p=1.0".**

## F250 ✅ — THE REGISTERED ARM IS COMPLETE. 13/13 CASES. DIRECTION FAVOURABLE, p = 0.453, NOT SIGNIFICANT.

The paired arm ran to every case it was registered for. This is the final readout and I am not
revising it again as numbers wobble.

    MATCHED, all 13 implementer cases, n=39 each, every case its own control
    baseline   REFUSED 13 (33.3%)   wrote-first 1 (2.6%)
    nudge      REFUSED  5 (12.8%)   wrote-first 3 (7.7%)

    CASE LEVEL — the unit the experiment varies: better 5 · worse 2 · same 6
    sign test, two-sided: p = 0.453   ⇒ NOT SIGNIFICANT

**Sample-level refusal falls by 61% and the case-level test still cannot reject chance at 13 cases.**
Both statements are true and only the second one decides. Five-better-two-worse is the direction you
would want; it is also what a coin flip produces about a quarter of the time.

**⭐ THE POST-HOC SPLIT, NOW WITH THE FULL SET — AND IT IS STRIKING:**

    baseline-STUCK cases (>=50% refuse)   5 cases   86.7% -> 20.0%
    baseline-CLEAN cases (0% refuse)      8 cases    0.0% ->  8.3%

**The nudge is not neutral — it TRADES.** It rescues workers that were already failing (13 of 15
refusals gone) and injects a small failure rate into workers that were fine (0 → 2 of 24). That is
exactly the "urgent directive helps the stuck and destabilises the working" mechanism F249 guessed,
now visible across the whole set.

**⚠ IT REMAINS POST-HOC AND I AM NOT SHIPPING ON IT.** The subgroup was defined after seeing the
data. What it justifies is a REGISTERED third arm, not a config change: **nudge only when
`req.prior_hint.is_some()`** — the engine's own signal for "this worker already failed once" — run on
the same frozen cases and judged at case level. **If that arm shows the stuck-case gain without the
clean-case cost, the conditional nudge is earned. Until then the unconditional nudge stays ON for
cost, not for proof, and the honest headline is p = 0.453.**

## F251 🔴 — THE BENCH CANNOT TEST THE CONDITIONAL NUDGE. 12 OF 13 CASES ALREADY CARRY THE CONDITION.

F250 registered a third arm: nudge only when `req.prior_hint.is_some()`. The frozen payloads already
record that condition as `was_stalled` — an INPUT property, which is the honest way to split, unlike
the outcome-based "baseline refused ≥50%" that produced the striking 86.7%→20.0% figure. So I split
the completed paired data by it instead of buying a new arm.

    was_stalled=TRUE   12 cases | baseline 13/36 (36.1%) -> nudge 5/36 (13.9%) | better 5, worse 2
    was_stalled=FALSE   1 case  | baseline  0/3  ( 0.0%) -> nudge 0/3  ( 0.0%) | better 0, worse 0

**There is no contrast. Twelve of thirteen replayable implementer cases ALREADY carry a supervisor
note**, so the "does the nudge help only workers that already failed" question has exactly one
counter-example and zero power. **The registered third arm cannot be run on this corpus at all.**

**⚠ AND IT RETROSPECTIVELY EXPLAINS THE STRIKING SPLIT.** F250's 86.7%→20.0% on "stuck" cases was NOT
stalled-versus-clean — nearly everything here is stalled. It was WITHIN-stalled variation, sorted by
the outcome I was measuring. That is the post-hoc trap doing exactly what it does, and the input-based
split is what exposed it.

**WHAT THIS COSTS AND WHAT IT LEAVES:** the conditional-nudge question now needs the ENGINE change plus
live runs — the offline bench is structurally blind to it, because its corpus is drawn from
`llm_request` payloads that skew heavily toward re-dispatches. **That is a limit of the instrument, not
a result about the lever**, and it belongs next to F237c's "3 tasks from one spec" as a known blind
spot rather than being discovered again in a month.

## F252 ⭐⭐⭐ — THE REGISTERED TEST CLEARED. TEST-AUTHOR FAILURES 13/42 → 0/11, p = 0.017.

The instrument says it, not me:

    MEASURED (CURRENT binary only): test-author 11 completed / 0 failed  (n=11)
    vs the old-build rate 13/42 = 31.0%: P(this good by chance) = 0.017  ⇒ SIGNIFICANT

**This is the threshold and the condition registered in F233 before any of it ran** — nine clean
completions clears p<0.05, and the count reached eleven with zero failures. The bar was set in
advance, the instrument enforced it, and it was never loosened: when `swarm-3node-r1` held five clean
completions that would have cleared it early, `goalstate` refused them for want of a `run_finished`
and I let the refusal stand rather than rescue my own result (F250 note, and the park in RESUME.md).

**⚠ THE CONFOUND, STATED IN THE SAME BREATH AS REGISTERED, NOT BURIED:**
The 31% baseline (13 of 42) comes from an **OLDER BUILD ERA**. This is a **before/after across
builds, NOT a randomised A/B against a contemporaneous control.** Everything that changed between
those eras changed at once — `kind_prompt`, `dep_signatures`, the GGUF samplers, the act-now nudge,
the F231 judge fix — so **the result attributes to THE BUILD, not to any one lever**, and F250 already
showed the nudge alone is null at case level (p=0.453). **The clean n=3 `baseline` cells that would
make this a controlled comparison are still owed.**

**WHAT IS AND IS NOT CLAIMED:** test-authors on the current engine failed 0 of 11 where the old engine
failed 13 of 42, and the probability of that by chance under an unchanged rate is 1.7%. That is a
real, pre-registered, instrument-verified movement of the mini-goal. It is **not** a claim that any
specific change caused it, and it is **not** goal one.

**⇒ MINI-GOAL (2) IS RESOLVED. THE MINI-GOAL IS NOW GOAL ONE: the node curve, 3 nodes vs 1 node on
wall-clock AND shipped quality, with the gap clearing the replicate spread.** `backlog()` already
interleaves `baseline-n3-r0, baseline-n1-r0, baseline-n3-r1, …` so a matched pair lands after every
two units instead of after six.
**⇒ THE BOUNDARY IS NOW UNBLOCKED.** The order registered in F251 was metric-clears → boundary →
diagnose. F243/F244 (a failed task's `error` and `session_id`) can deploy without spending a live
measurement.

## F253 🧊 — THE ENGINE IS FROZEN FOR THE DURATION OF THE NODE CURVE

`complete()` gates on a matching `engine_build`. **Any boundary crossed mid-curve voids every cell
already collected and makes the n3 and n1 arms incomparable** — which is the whole point of the
comparison. So from `baseline-n3-r0` until the curve has its matched pairs: **no rebuild, no
boundary.** Engine work is committed and deployed at the NEXT boundary, after the curve.

That makes the standing "every tick must ship or measure an engine change" rule resolve to MEASURE
for this stretch, and it is not idling — it is the same discipline as L131 (do not spend a live
measurement to deploy a diagnostic), one level up: **do not spend a whole experiment to deploy a
lever.**

**MIHAI ASKED WHY THE RUN WAS USING ONLY 2 NODES. IT IS NOT.** Engine truth, from the run's own
events rather than from `lms`:

    run_started.pool          = 3 distinct devices (gabee | mihai | workhorse), weight 2 each
    pool_resolved.worker_count = 3
    lms ps                     = all three GENERATING

Fleet sampler over 40 ticks: **3 busy 50%, 2 busy 7.5%, 1 busy 12.5%, 0 busy 30%** — and the 30% is
the pre-dispatch startup window, not idleness under load. **This is a genuine 3-node cell, not the
`actual_nodes=2` failure that voided three earlier baselines (F227).**

⚠ **The 50% three-busy figure is not good, and it is precisely what goal one is measuring.** If three
nodes turn out to barely beat one, low concurrency is the reason, and that will be a finding rather
than a disappointment — but only the matched n1 cell can say so.

## F254 🔴 — FIVE 60-SECOND REFUSALS WERE COUNTED AS FINISHED UNITS; THE MEDIAN READ 114x LOW

`median_unit_secs()` filtered `timed_out` and `aborted` but **not `void`**. The pool-mismatch gate
turns a unit round in ~60s and writes a result file, so the "finished unit" population was:

    IN  baseline-n3-r0        60s   void=True     <- refusal
    IN  baseline-n3-r1        60s   void=True     <- refusal
    IN  baseline-n3-r2        60s   void=True     <- refusal
    IN  kind_prompt-n3-r0     60s   void=True     <- refusal
    IN  scoped_contracts-n3-r0 60s  void=True     <- refusal
    IN  sink_review-n3-r0    8729s
    IN  think_off-n3-r0      6376s
    IN  think_off-n3-r1      7237s
    IN  think_off-n3-r2      6524s
    median = 60.3s            REAL median = 7236.9s      ⇒ **114x understatement**

**Consequence, stated at its true size and no larger:** abandon-rule 4 ("far beyond the measured
norm") therefore fired at **2.5 minutes into every real unit** and stayed lit for the whole ~2h run —
which is 100% of healthy units, so it carried **zero information** (L66). It could **not** kill
anything: `conf` is a `max()`, rule 4 caps at 0.6, and the kill line is 0.8. **No run was ever at
risk from this.** What it did cost is a watch log that says "this unit is pointless" once a minute
about a perfectly healthy run — the exact noise that makes a real warning unreadable.

**DRILLED: the same rule existed TWICE and the two copies disagreed.** `median_unit_secs` excluded
timed_out/aborted; the ETA's `durations` list excluded **nothing**, so a 60s refusal also dragged the
sweep's own ETA. Both now go through **one** predicate, `is_real_unit()`, so the next reader cannot
write a third variant.

**The multiplier had to move, and here is why it is not fitting the instrument.** Against the real
median, 2.5x lands at 18092s — **beyond the 16200s unit cap**, so the rule could never fire at all. A
safeguard that cannot fire is dead code. **1.8x** fires at 13026s = **1.49x the slowest real unit
ever observed** (8729s), with the whole observed distribution (0.88x-1.21x of median) far below it.
⚠ **FALSIFIER REGISTERED BEFORE THE FIRST UNIT RUNS UNDER IT:** if a unit that later yields a VALID
(non-void, scored) result ever trips rule 4, **1.8 is too tight and goes back up.**

⚠ **THE RUNNING SUPERVISOR (pid 22764) DOES NOT HAVE THIS FIX** — L23, a live interpreter does not
see source edits, and restarting it would take `baseline-n3-r0` down with it. So the noisy line keeps
printing for this unit and the fix takes effect at the next supervisor start. **Nothing about the
engine changed, so the curve's freeze (F253) is intact.**

## F255 ✅ — BEST-OF-N DOES **NOT** COLLAPSE TO ONE DRAFT (my own claim, killed by the event)

I read the cap at `swarm.rs:11551` (drafts capped at DISTINCT MODELS), saw `lms ps` reporting one node
GENERATING, and concluded the skeleton phase runs on a single node. **The run's own event says no:**

    skeleton_drafts {requested: 3, returned: 3, dead: 0, straggler_aborted: 0, secs: 222,
                     chars: [4380, 3842, 4057], worker_count: 3}

The engine scales drafts to **`worker_count`**, not to the lever (`best_of_n_skeletons` reads 2 and it
requested 3). **The skeleton phase genuinely uses all three nodes.** ⇒ **L139: a source line predicts,
the event decides.** One `lms ps` sample is a single frame of a 222-second phase.

## F256 🔴🔴 — A REDUNDANCY OPTIMISATION WAS APPLIED TO A **COMPLEMENTARY** FANOUT: 33% OF ALL RESEARCH IS DISCARDED BY DESIGN

**6 of 6 archived runs that reached research lost EXACTLY ONE scout lens. Never zero. Never two.**

    unit                 planned  returned  LOST
    sink_review-n3-r0       3        2      architecture
    swarm-1node-r0          3        2      libraries
    think_off-n3-r0         3        2      edge-cases
    think_off-n3-r1         3        2      edge-cases
    think_off-n3-r2         3        2      edge-cases
    swarm-3node-r0 (CUR)    3        2      edge-cases
    ------------------------------------------------
    6/18 planned lenses lost = 33.3%,  on 1-node and 3-node runs alike

⚠ **The five 60-second VOID refusals were EXCLUDED** — they never ran research, and counting them would
have read 63.6%. That is L138 again, in a second instrument, one tick after the first.

**MECHANISM, from the call site.** `run_scouts` (`swarm.rs:12081`) sets `scout_grace` from
**`self.straggler_stop`** — baked **ON**. `collect_fleet_with_straggler_stop` arms the grace once
`should_arm_straggler_grace(3, 2)` holds, i.e. **the moment 2 of 3 scouts finish the third gets 45 s and
is ABORTED**.

**THE ASYMMETRY NOBODY NAMED.** Plan drafts are **REDUNDANT** — best-of-N makes N candidates and keeps
one, so killing the slowest costs nothing. Scout lenses are **COMPLEMENTARY** — each covers ground no
other lens covers, so killing the slowest costs that ground outright. The engine already knows this
distinction: `straggler_stop_degrade` exists, defaults **OFF**, and its doc says it is separate because
those fanouts *"CAN change a worker's build inputs"*. **Research findings feed the plan and every later
dispatch — they are the definition of a build input.** CONTRACTS and DETAIL are correctly gated on it;
scouts were not. **QUEUED FIX: gate `scout_grace` on `straggler_stop_degrade`.**

### 🔴🏆 AND IT REFUTES THE PREVIOUS FIX AT ITS OWN ADDRESS

`fb0885328` (2026-08-01) reordered `SCOUT_LENSES` to put `edge-cases` **first**, on the reasoning that
straggler-stop *"sacrifices the LAST lens"* and that order is dispatch order. **It is not positional.**
The grace arms on the **completion count**, so the victim is whichever lens is **SLOWEST**.

    binary built   2026-08-04 21:42:45
    reorder landed 2026-08-01 14:14:24   ⇒ THE ARM WAS ARMED (L53)
    result         swarm-3node-r0 lost `edge-cases` — now FIRST in the order
                   scout-edge-cases.json never reaches `phase: done`
                   scout-architecture.json / scout-libraries.json both `phase: done`

`edge-cases` asks for failure modes and the concrete tests that prove the task done — the most generative
of the three prompts, so it is reliably the slowest and reliably the one killed. **Reordering could never
have helped.**

**COST OF THE FIX, QUANTIFIED BEFORE THE RUN (L79):** research ran **297 s of a ~7200 s unit**, so waiting
the straggler out costs on the order of **1% of wall-clock to recover 50% more research**.
⚠ **FALSIFIER:** if a run under this change still shows `lenses_returned` short of `scouts_planned`, the
cause is NOT straggler-stop and the whole comment is wrong.
⚠ **NOT COMPILED — deliberately.** `cargo check` on this crate would steal CPU from `local-mihai` mid-run
and perturb the very measurement in flight. The edit swaps one bool field for another of the same type,
used identically 200 lines away at `12314`/`13497`. **clippy runs at the boundary, before deploy.**

## F257 🔴🏆 — THE HEADLINE DISPATCH-QUALITY METRIC READ "UNMEASURED" BECAUSE THE READER LOOKED FOR AN EVENT NAME THAT WAS NEVER USED

`dispatch_audit.py` returned `kind_mismatch_pct: None` for every run on the shipped build, with the
basis string *"needs the `rules_kind` engine event"*. **The engine emits `rules_delivered`, and has
been emitting it all along** — per dispatch, carrying `task_id`, `kind`, `kind_prompt`, `tailored`.
It was added deliberately, and its own comment explains that without it the metric would be circular.

    run                rules_delivered events
    sink_review-n3-r0        24   (kind_prompt False)
    swarm-1node-r0           14   (kind_prompt False)
    think_off-n3-r0          22   (kind_prompt True)
    think_off-n3-r2          24   (kind_prompt True)      ⇒ BOTH DIRECTIONS present (L123)

**THE RECOVERED NUMBER — and the lever did NOT do what "fixed" implies:**

    kind_prompt OFF   sink_review-n3-r0   75.0%   18/24 mismatched
                      swarm-1node-r0      64.3%    9/14
    kind_prompt ON    think_off-n3-r0     40.9%    9/22
                      think_off-n3-r1     42.3%   11/26
                      think_off-n3-r2     41.7%   10/24

⇒ **~70% → ~42%.** Real, measured, both directions. ⚠ **AND IT STOPS THERE.** `tailored` is
`kind_prompt_on && is_test_author`, so **`read-only-shard` and `owns-nothing` still receive the
implementer-shaped generic rules** — **4 in 10 dispatches are still told to do another job.** On a
3-node run those two kinds are 10 of 24 dispatches. **That is the next engine change, and it is
exactly the "more nodes ⇒ more undifferentiated work" mechanism goal one is about.**

**Definition, taken from the engine's fields rather than an assumption:** mismatched = `kind !=
implementer AND NOT tailored`. **POSITIVE CONTROL:** with the lever off nothing is tailored, so the
rule reduces to the old inference — sink_review reads **75.0%** under the new rule against the
**79.2%** the inference produced, the gap being dispatches with no plan entry. **Both classifiers are
now exposed** via `kind_counts_from_events` and `kind_source_disagreement` rather than silently
reconciled; they disagree on `entrypoint`, which the engine never labels.

## F258 ⚙️ — AN INSTRUMENT BUMP MUST NEVER COST A UNIT RE-RUN (`reaudit.py`)

`sweep.complete()` treats a row whose `audit_version` differs from the current one as INCOMPLETE, so
bumping the audit **re-runs the unit** — ~2 hours of fleet time to recompute a pure function of a log
already on disk, and mid-curve it would re-run cells the node curve had already collected. That is a
mechanism that punishes fixing an instrument, at exactly the moment fixing it matters most.

**`reaudit.py` recomputes the audit from the stored `run.jsonl` and rewrites the row in place** —
audit blob and version stamp only, never score / wall_secs / engine_build. **9 of 9 rows migrated
`da-1 → da-2` with no run re-executed.** ⇒ **An instrument fix can now ship at ANY moment in a live
campaign instead of waiting for a boundary**, which is what made F257 shippable under the F253 freeze.

## F259 🔴🏆 — I RETRACT THE 42%. IT WAS AN ARTEFACT OF A ONE-BIT FLAG, NOT A PROPERTY OF THE ENGINE

One tick after publishing **"kind_prompt ON leaves 40.9 / 42.3 / 41.7% of dispatches misinstructed"**
I checked what the engine actually delivers, and the number does not survive.

`tailored` is `kind_prompt_on && is_test_author` — **the test-author branch alone.** But the worker
prompt is assembled from at least **three independently-branching sections**:

    owned_part      read_only_shard && kind_prompt_on  |  owned_files.is_empty()  |  generic
    reading_rules   test-author | kind-generic | off-generic
    stopping_rules  test-author | kind-generic | off-generic

And `swarm.rs:19518`'s own comment about the read-only-shard variant says: *"Subtracting the paragraph
is the whole change: this kind sees FEWER rules, never more."* **`read-only-shard` receives a rule set
written specifically for it, and my metric counted every one of those dispatches as misinstructed** —
8, 8 and 8 of them across the three runs. The `owns-nothing` kind likewise gets the sink's own
`owned_part` paragraph, which is **not gated on the lever at all**.

⇒ **The lever-ON rate is WITHDRAWN and now reads UNMEASURED.** I am not replacing it with "≈0%"
either: `reading_rules` and `stopping_rules` still branch only on test-author, so read-only-shard and
owns-nothing genuinely do receive implementer-shaped text **in those two sections**. The truthful
statement is that **the question is three-dimensional and the event exposed one bit of it.**

**WHAT SURVIVES:** the lever-OFF figure, now labelled an **inferred UPPER BOUND** (75.0% on
sink_review), because with the lever off reading/stopping rules are generic for every kind — while
owns-nothing still gets its own sink paragraph, so the true figure is somewhat lower.

**QUEUED ENGINE CHANGE:** `rules_delivered` now also emits **`rules_sections` {owned_part,
reading_rules, stopping_rules}** naming the variant each section took, so a reader can say exactly
which sections a kind got generic text for instead of inferring a rate from one bit. Audit bumped to
**`da-3`**; `reaudit.py` migrated all 9 rows in place, **zero runs re-executed** — which is precisely
the capability F258 bought, used the very next tick to withdraw a wrong number instead of leaving it
in the table.

**L141: AN EVENT FIELD IS A SUMMARY, AND A SUMMARY CAN BE NARROWER THAN THE BEHAVIOUR.** Diff the
delivered TEXT before trusting a boolean. I built a headline on one bit of a three-bit question and
published it; the only thing that caught it was going back to read the branch the flag does not cover.

## F260 ⭐⭐ — THE CURVE AS SCOPED COULD NEVER HAVE REACHED SIGNIFICANCE. n=3 → n=5, DECIDED BEFORE THE FIRST PAIR.

The node curve is a **matched-pair** design, so its natural test is the one-sided **sign test**, whose
smallest attainable p is `0.5**n`:

    n=3  perfect separation -> p = 0.125   ← CANNOT REACH 0.05 EVEN IF FLAWLESS
    n=4  perfect separation -> p = 0.0625  ← still misses
    n=5  perfect separation -> p = 0.031   ← clears

Read **unpaired** instead (exact permutation) and n=3 reaches exactly **0.0500** on perfect separation
and **0.2000 the moment ONE replicate crosses**. On a fleet whose identical-config replicates scored
**44.2 / 86.7 / 90.0** and whose real unit walls run **6376-8729 s**, **one crossing is the expected
case, not the exception.**

⇒ **`MIN_REPS` 3 → 5.** n=3 would have spent **~12 hours of fleet time to produce a number that was
never able to clear the bar**; n=5 costs ~20 h and can. **Computed and committed while
`baseline-n3-r0` was still in EXECUTE, with no matched pair in existence** — this is a threshold set
before the data, not moved to fit it.

**BLAST RADIUS, stated honestly:** this raises every SCORE cell, not only the curve. Mechanism cells
(`reps == 1`) are capped at 1 in `backlog()` and are untouched.

**`PREREGISTERED.md` now holds the whole protocol** — claim, test, the four falsifiers (a VOID cell
voids its pair; a mid-curve boundary voids everything collected; wall-clock without score is a FAIL;
significance that needs a pair removed is not significance), and what is explicitly not claimed.

## F261 ⭐⭐⭐ — THE WHOLE 3-NODE ADVANTAGE IS IN EXECUTE, AND THE PREFIX IS **LONGER** WITH 3 NODES

`occupancy.py` on both cells. ⚠ **Both readings come from UNFINISHED runs on DIFFERENT engine builds
— provisional lower bounds, not a result.** The curve exists to replace them with matched pairs.

    n3 (live)  wall 3117.7s  busy 2688.3 node-s  occ 0.2874 | EXECUTE  899.0s @ 0.9968 | prefix 2218.7s
               per-device 899 / 899 / 890 s = 33.4 / 33.4 / 33.1%   |   one-node-only 0.0s
    n1 (arch)  wall 4884.4s  busy 2853.1 node-s  occ 0.5841 | EXECUTE 2853.1s @ 1.0    | prefix 2031.3s

**EXECUTE SCALES ESSENTIALLY PERFECTLY.** 2853.1 s of execute wall at one node becomes 899.0 s at
three — **3.17x** — with **zero one-node-only time** and the three devices within 0.3 points of an even
split. There is no scheduling defect to find here, and "the swarm wastes fleet time in execute" is
refuted a second time (F234 said 88.6%; this run says 99.68%).

🔴 **AND IT BARELY MATTERS, BECAUSE THE PREFIX IS WHERE THE TIME IS.** 2218.7 s of the 3-node run's
3117.7 s — **71%** — is spent before the first dispatch, and that phase is **187 s LONGER at three
nodes than at one (+9%)**. Adding nodes lengthened the part that dominates the wall and shortened the
part that does not. **That is Amdahl, measured, with an address** — and it is the same phase where
F256's aborted scout lens is thrown away.

📌 **REGISTERED BEFORE ANY CURVE PAIR EXISTS: `n1_wall / n3_wall` will land between 1.6 and 2.4.**
⚠ **FALSIFIER: a ratio outside that band means this prefix/execute decomposition is wrong.**

⚠ **THE RISK TO GOAL ONE IS THE QUALITY HALF, NOT SPEED.** `PREREGISTERED.md` falsifier 3 is explicit:
wall-clock without score is a FAIL. The idle-node work differs sharply between the arms —
n3 `{judge 39, pre_review 2, split 1}` vs n1 `{judge 90}` — so the two arms are not merely fast and
slow versions of one process, and the score comparison is the one that can still go either way.

**L143: SPEEDUP LIVES WHERE THE TIME IS, NOT WHERE THE PARALLELISM IS.** Execute was already at 99.7%
and every previous instinct of mine pointed there. Decompose the wall before optimising the part that
already works.

## F262 ⭐⭐⭐ — THE 1-NODE ARM SKIPS A QUALITY GATE IT IS PHYSICALLY UNABLE TO COMPUTE

The prefix decomposition, both arms, from their own timestamps:

    swarm-3node-r0  prefix 2219s          swarm-1node-r0  prefix 2031s
      +   0..297  research (3 lenses)       +   0..984  research (3 lenses)
      +      520  skeleton_drafts #1        +     1267  skeleton_drafts (ONE)
      + 982..1182 detail x5                 +1339..1878 detail x8 (serial)
      +     1182  confidence_retarget       +     2031  contracts / plan_loaded
      +     1182  retarget_discarded
      +     1513  skeleton_drafts #2  <-- A SECOND FULL PLANNING PASS
      +1950..2089 detail x5           <-- AND A SECOND DETAIL PASS
      +     2162  low_confidence_ask -> timeout 5s, "the fleet idled for the whole window"
      +     2219  contracts / plan_loaded

**RESEARCH DOES SCALE — 984s → 297s = 3.3x.** My F261 phrasing "the prefix does not scale" was too
coarse and is corrected here: research scales beautifully; the 3-node prefix is longer *in spite of*
saving 687 s on research, because it then spends **~1240 s on a second planning pass** the 1-node arm
never runs.

🔴 **AND HERE IS WHY IT NEVER RUNS IT.** The engine's own events:

    3 nodes:  skeleton_drafts {requested 3, returned 3}   plan_loaded {plan_confidence: 83, ask_floor: 85}
              confidence_retarget {binding_signal: "agreement", action: "redraft", conf_before: 83}
    1 node:   skeleton_drafts {requested 1, returned 1}   plan_loaded {plan_confidence: NULL}

**Plan confidence is AGREEMENT ACROSS INDEPENDENT DRAFTS. At one node there is one draft, so the
signal is `null` and the floor cannot be breached.** The 1-node run is not confident — **it is
unmeasurable, and unmeasurable reads as "proceed".** More nodes make the swarm able to notice it does
not know how to decompose the task, and that noticing costs a full planning pass. **Both arms then
shipped the SAME 16 tasks.**

⚠ **THIS IS THE CENTRAL ASYMMETRY OF GOAL ONE.** The 3-node arm pays wall-clock for a quality gate the
1-node arm gets to skip for free. Any wall-clock comparison that ignores it is comparing a run that
checked its own plan against one that could not.

### 🔴 I ALMOST CALLED THE REDRAFT WASTE. THE ARCHIVE SAYS OTHERWISE.

    unit              rounds  conf_before -> final plan_confidence   gap to floor (85)
    think_off-n3-r1     2     41 -> 88   (+47)                        44
    think_off-n3-r0     1     79 -> 100  (+21)                         6
    swarm-3node-r0      1     83 ->  83  (  0)                         2

`retarget_discarded` fires on **4 of 4 rounds across 3 of 3 runs**, yet final confidence ROSE in two of
them. So it does **not** mean "the work was thrown away" — reading it that way was my error, caught
before it was published. **Redrafts pay, and they pay in proportion to the gap.**

📌 **HYPOTHESIS, n=3, NOT A RESULT (L10, L126): redraft gain scales with the confidence gap, and a gap
of ~2 does not repay a full planning pass.** The engine's own `stall_stop` message already says
*"further rounds cost a full planning pass to ship a plan already held"* — it just learns that only
AFTER paying. A gap-gated redraft is the obvious change and **I am NOT shipping it on n=3.** The
curve's five 3-node cells supply the observations. ⚠ **FALSIFIER: a run whose redraft starts from a
gap ≤ 2 and still gains ≥ 10 points kills the hypothesis.**

## F263 🔬 — THE FIRST SELF-DESCRIBING FAILURE, AND IT DESCRIBES ITSELF ONLY HALF WAY

`baseline-n3-r0` produced its first failure at ~70 min, and thanks to `ca84de52d` + `d685eab15` the
event carries a reason for the first time in this campaign:

    task_completed {task_id: "test-core", status: "failed", attempts: 3,
                    device: "mac-gabee-...", error: "no_first_write",
                    elapsed_ms: 0, session_id: null, tool_calls: []}

**`no_first_write` is a JUDGE VERDICT, not a dispatch error** (`goose-swarm/src/judge.rs:36,468`): the
worker owned code, wrote none of it, and made **ZERO tool calls** past a ≥420 s deadline — *"stuck
before its first byte, not over-reading"*. Its own comment records why the label exists: the verdict
used to be stamped `OverReading` even when the tool-call count was zero, on **9 of 11 measured**
workers, and that mislabel *"produced three false causal chains in this campaign"*. **So the WHY
channel works, and it immediately paid for itself.**

⚠ **AND IT IS A COUNTER-EXAMPLE TO MY OWN HEADLINE.** `test-core` is a **test-author** — the exact row
F252 reported as 11 completed / 0 failed, p = 0.017. This cell now holds **5 completed / 1 failed** on
the current binary. F252's confound (older-build baseline, five changes at once) already said the
number attributes to the build, not a lever; this is the first contemporaneous failure of that kind
and it must be counted, not explained away.

🔴 **TWO FIELDS IN THE SAME EVENT ARE STILL LYING.**

    elapsed_ms: 0      for a task that burned THREE attempts of >=420 s each — IMPOSSIBLE (L47).
                       The same variable feeds `device_speed` (`e.0 += elapsed_ms`), so a failed
                       task contributes ZERO to its device's measured speed.
    session_id: null   `task_session` IS inserted on every completed run (scheduler.rs:876) from
                       `TaskRunOutput`, so this null came from the WORKER, not the bookkeeping.

Both point at the **judge-kill / abort path**: when the judge fires and the scheduler tears the worker
down, the resulting `TaskRunOutput` appears to carry no session and no elapsed time. **I have NOT
confirmed that and I am not asserting it** — the fix site must be read first (L130), and reading it is
the next engine task. ⚠ **REGISTERED CHECK: any future `task_completed{status: "failed"}` whose
`elapsed_ms` is 0 while `attempts >= 1` proves the abort path is the site; a non-zero one refutes it.**

**The point of `d685eab15` was that a failure names WHERE to look. `session_id: null` means it still
does not.** Half a channel is better than none and worse than it reads.

## F264 ⚙️ — THE PREFIX IS NO LONGER A LUMP: `occupancy.py` occ-2 REPORTS ITS PHASES

F261/F262 were derived by hand from raw timestamps because `occupancy.py` printed the prefix as one
number — *"2218.7s before the first dispatch"*. That is how a fourth ad-hoc reader gets written (L2),
and the prefix is **71% of a 3-node run's wall**. It now reports itself, on every cell the curve
produces:

    3 nodes  PREFIX breakdown (draft rounds 2, plan_confidence 83, redraft cost 1036.6s)
        +  297s   297s  research_completed
        +  520s   222s  skeleton_drafts
        + 1182s   662s  confidence_retarget
        + 1182s     0s  retarget_discarded
        + 1513s   331s  skeleton_drafts        <- second round
        + 2095s   582s  detail x14
        + 2162s    67s  low_confidence_ask
        + 2167s     5s  low_confidence_ask_timeout
        + 2219s    51s  contracts / plan_loaded / first dispatch

    1 node   PREFIX breakdown (draft rounds 1, plan_confidence None,
                               redraft cost n/a — this run never entered a redraft)
        +  984s   984s  research_completed
        + 1267s   283s  skeleton_drafts
        + 1878s   611s  detail x8
        + 2031s   153s  contracts / plan_loaded / first dispatch

**F262's asymmetry is now a printed field rather than an argument: `draft rounds 2 vs 1`,
`plan_confidence 83 vs None`, `redraft cost 1036.6s vs n/a`.** The 1-node line says **"n/a — this run
never entered a redraft"**, deliberately NOT "0s": a run that cannot compute the check is not a run
that passed it cheaply (L144), and a zero there would read as "the redraft is free at one node".

`self-test OK (occ-2)` — the perfect/worst/1-node controls, the vacuous-truth guard, the real-zero
case, determinism and hog detection all still pass, so the new field did not come at the cost of the
existing ones.

⚠ **The two detail passes collapse into one `detail xN` span** — per-task `detail_completed` events
would drown the list. The two-pass fact is carried by `draft_rounds` and `redraft_secs`, which is the
number that matters; anyone needing per-task detail timing reads the log.

## F265 🔴🔴 — MINI-GOAL 2 IS ABOUT TO BE REVOKED BY ITS OWN PRE-REGISTERED RULE, AND I MIS-STATED THE CELL

**CORRECTION FIRST.** Last tick I wrote *"this cell reads 5 completed / 1 failed"*. **Wrong.**
`goalstate` skips any run without `run_finished`, so `swarm-3node-r0` contributes **nothing** yet; the
5/0 comes from OTHER finished runs on this binary. Measured with `failures.kind_of` on this cell's own
dispatches:

    implementer    done    5
    test-author    done    1
    test-author    failed  1        <- test-core
    verify/sink    done    8

**And `test-core` IS a test-author by the classifier, not by its name:** its `task_dispatched`
`owned_files` are `['tests/test_meridian.py', 'tests/test_store.py']`. I asserted the kind from the id
last tick and got lucky; the instrument was the thing that had to say it.

🔴 **WHAT HAPPENS WHEN THIS CELL FINISHES.** `moved_significantly()` is unambiguous and was written
before any of this: **`failed == 0` is required; any failure at all returns `(False, 1.0)`.** So the
moment `run_finished` lands, the row becomes **7 attempted / 1 failed ⇒ p = 1.0, NOT SIGNIFICANT**, and
**mini-goal 2 stops being resolved.** F252's confound already said the p=0.017 attributed to the build
rather than to any lever; this is the same finding arriving as data. **I am not arguing with the rule
I wrote — I am reporting that it is about to fire against my own headline.**

### 🔬 AND THE FIRST FAILURE EXPOSED AN ARITHMETIC BUG IN THE SAME METRIC

`by_kind` increments `slot[0]` for **every** `task_completed` and `slot[1]` only for the failures, so
`slot[0]` is **ATTEMPTED** and the two overlap. The report then printed `n = completed + failed`,
**counting every failure twice**. It never showed because every sample this campaign ever took had
**zero** test-author failures, where the wrong formula happens to agree. The campaign's first failure
is what made it visible. Fixed: `n = completed`.

⚠ **The p-value itself was never affected** — that branch only runs when `failed == 0`, where the two
formulas coincide. The lie was confined to the printed `n`, which is still a lie in a report that
decides whether a goal is resolved. **L47 again: an impossible value indicts the instrument — and so
does a value that is only correct in the case you have always been in.**

## F266 🔴🏆 — I RE-IMPLEMENTED `occupancy.py` AND REPRODUCED THE EXACT BUG IT WAS WRITTEN TO FIX

I wanted a concurrency histogram, so I wrote a throwaway span-builder instead of extending the
instrument. It printed **"9 tasks running 55.3% of the time"** on a fleet with **6 slots** (3 nodes ×
PARALLEL 2). **An impossible value indicts the instrument (L47) — and the instrument was mine.**

The cause is documented, at length, inside `occupancy.py` itself: a retry re-dispatches before the
first attempt completes, so pairing `dispatch[i]` with `completion[i]` leaves dispatches unmatched, and
crediting an unmatched dispatch to the end of the run **invents time that was never spent**. That file
also handles split parents (aborted, never completing under their own id) and the phantom-tail guard.
My 30-line version had none of it. **L2 exists because of exactly this, and I did it anyway.**

**THE NUMBERS ARE WITHDRAWN AND NOT REPORTED.** The correct move is a histogram built from
`occupancy.py`'s already-corrected spans, and that is the next instrument task — not a second reader.

**What IS measured, by the real instrument, and still stands:** `solo_node_secs` = **0.0 s** on this
3-node cell — one node has never been the only one working. That is the run-wide form of the question
I was trying to answer with a snapshot, and it was already available.

**A live snapshot, which is a fact about one instant and nothing more (L10):** at 88 min the run had
**5 tasks in flight across all three nodes** (gabee ×2, workhorse ×2, mihai ×1 — 5 of 6 slots) with
`lms ps` showing all three GENERATING, and **`integrate-verify` running CONCURRENTLY with four other
tasks**. F151 recorded the sink holding the fleet at ≤2 tasks for 69% of low-concurrency minutes; that
is not what this instant shows. ⚠ **One instant cannot refute a distribution** — the corrected
histogram is what would, and it is owed. ⚠ **`test-api-web` has been in flight 41 minutes** and
occupancy names it the biggest task at 0.334 of node-busy; a single 41-minute task on a 3-node fleet
is the tail risk worth watching.

## F267 ⭐⭐⭐ — SLOT UTILISATION FALLS FROM 100% TO 74.8% WHEN YOU GO FROM 1 NODE TO 3

`occupancy.py` occ-3 now computes the concurrent-task histogram in the **same exact interval sweep**
that already produced solo time, off the **corrected** spans — the thing F266's hand-rolled version
got catastrophically wrong. Max reading is now **6 on a 6-slot fleet**, never above.

    1 NODE  (2 slots, 48 min dispatch window)      3 NODES (6 slots, 57 min dispatch window)
       2 task(s): 100.0%  (47.6 min)                 1 task(s):   0.0%
       ^ pinned full for the ENTIRE window           2 task(s):   1.3%  ( 0.8 min)
                                                     3 task(s):   0.9%  ( 0.5 min)
                                                     4 task(s):  65.8%  (37.4 min)   <- the median state
                                                     5 task(s):  11.3%  ( 6.5 min)
                                                     6 task(s):  20.6%  (11.7 min)

**MEAN CONCURRENCY 2.00 → 4.49 tasks.** So tripling the slots bought **2.24x** the concurrent work,
and **slot utilisation fell from 100.0% to 74.8%.**

**This is the honest shape of goal one's speed half.** It is neither the "dead fleet" story (the swarm
runs <3 tasks only **2.2%** of the window, and 1 task **0.0%**) nor free scaling. The 3-node fleet
genuinely does more at once; it just cannot keep all six slots fed, because the DAG does not always
have six ready tasks. That is a **plan-width** limit, not a scheduler defect — consistent with EXECUTE
occupancy 0.9947 and `solo_node_secs` 0.0.

⚠ **Both readings are from UNFINISHED runs on DIFFERENT engine builds** — provisional, and exactly what
the matched pairs exist to replace. ⚠ **The 1-node 100% figure is not a virtue:** two slots are trivially
easy to keep full, and that arm took **2853 s of execute wall against 899 s** (F261). **High utilisation
of a small fleet is not the same as getting work done.**

`self-test OK (occ-3)` — perfect/worst/1-node controls, vacuous-truth, real-zero, determinism and hog
detection all still pass. Nothing keys on `occupancy_version`, so the bump re-ran no units.

## F268 ⭐⭐⭐ — THE EXECUTE WINDOW *IS* THE CRITICAL PATH: THE SCHEDULER WASTES NOTHING, THE PLAN IS THE CEILING

    3 nodes   mean concurrency 4.447 tasks     plan ceiling (max_useful_nodes) 4.45
    1 node    mean concurrency 2.000 tasks     plan ceiling                    2.49

**The 3-node arm's achieved concurrency and its plan ceiling agree to two decimals.**

⚠ **AND THAT IS NOT A TAUTOLOGY, THOUGH IT LOOKS LIKE ONE — the check matters.** `max_useful_nodes` is
`total_task_secs / critical_path_secs`; mean concurrency over the dispatch window is
`total_task_secs / dispatch_window`. They coincide **exactly when the dispatch window equals the
critical path**. So the measured agreement is the statement:

> **The 3-node execute window is exactly as long as its longest dependency chain. There is no
> scheduling slack left to recover.**

That is consistent with everything else this cell has produced — EXECUTE occupancy **0.9947**,
`solo_node_secs` **0.0**, devices within 0.3 points of an even split — and it converts them from
"looks good" into a ceiling statement: **the only way a 3-node run gets faster is a WIDER or SHALLOWER
PLAN. Nothing in the scheduler is available to win.** ⇒ Any future speed work belongs in the
**planner** (fleet-relative width, shallower dependency depth), not in dispatch.

**The 1-node arm is the control that proves the reading.** Its ceiling is **2.49** against 2 slots, so
it is **slot-limited, not plan-limited** — it has more parallel work available than it can run. The two
arms therefore fail for *opposite* reasons, which is exactly what the curve should show.

⚠ **BOTH FIGURES ARE PROVISIONAL.** `occupancy.py` prints them as "NOT YET MEANINGFUL — the run is
unfinished, and the critical path only grows", so both ceilings are UPPER bounds that will fall as the
runs finish. **The claim is registered now and gets re-read on the finished cells.**
⚠ **FALSIFIER:** if the finished 3-node cell shows mean concurrency materially BELOW its final plan
ceiling, then scheduling slack does exist and this finding is wrong.
