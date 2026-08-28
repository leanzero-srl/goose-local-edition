# SWARM AGENDA — the live source of truth

Read this at EVERY tick. Do not re-derive it from context; context compacts and the agenda gets lost.
That already happened once and cost a whole night: the run was killed and nothing continued.

## The goal

Make the swarm BUILD BETTER SOFTWARE on local models, then beat the published `brun-fleet-qwen38-brainwaves-sb70`
(0.0273) on leanzero.net. Numbers follow from the product, not the other way round.

## THE HARD QUESTION, measured 2026-08-28 — one qwen3.8-27b beat our whole fleet of the SAME MODEL

Mihai: *"if the cloud 27b managed, ours must manage much better... otherwise all of our mechanisms are
proven invalid... what have we done that is wrong and how can one single qwen3.8 27b do so much better?"*

**THE NUMBERS, and they are not close:**

| | wall clock | app files | code written |
|---|---|---|---|
| our 3-node swarm | **136 min** | **0** | **0 bytes** |
| cloud qwen3.8-27b, ONE agent, no planning | 106 min | 9 incl. the whole frontend | 163,962 B |
| cloud glm-5.3-flash, ONE agent | 72.5 min | 14 | 167,555 B — **PUBLISHED 41.59%** |

Our planning phase alone is **1.9x the entire winning run**. Phase split: open 55.6 · ask 1.3 ·
research 25.9 · synthesis 7.4 · review 46.2.

**THE SINGLE MOST DAMNING NUMBER: we wrote 140,680 characters of SPECIFICATION before one line of code.**
That is 86% of the character count of the winner's entire finished codebase. We produced a shadow of the
program in English instead of the program.

**WHY THE SINGLE AGENT WINS — the mechanism, not the excuse.** It writes `ledgerd.py`, then writes
`notifierd.py` HAVING SEEN `ledgerd.py`. Coherence is free because there is one context. Our parallelism
destroys that coherence, so we spend the whole budget rebuilding it IN ADVANCE, in prose — and prose can
never be as good as looking at the code. We pay an enormous upfront cost to approximate something the
serial agent gets for nothing.

**WHAT IS ACTUALLY OVER-ENGINEERED — ranked, with the evidence:**
1. **Everything waits for everything.** No task starts building until all 21 briefs, the DAG and every
   REVIEW round are done. A slice whose brief is ready could build while others are still researched.
   This is the same fix Mihai already approved for coverage, applied to BUILD. Biggest single win.
2. **Over-decomposition: 21 slices for a 9-14 file product.** Every slice costs an OPEN entry, a coverage
   row, a ~6.7k-char brief, a DAG task, a contract stub, a BUILD lane and a REVIEW pass — seven phases
   times 21 units, for something that is 9 files. Decompose at roughly FILE granularity.
3. **Brief size: 6,443 chars MEDIAN.** A specification longer than the file it describes will be
   contradicted by the code. Interface + edge cases, not prose.
4. **REVIEW round 2 cost ~20 min for 2 findings.** Round 1 gave 5.
5. **Coverage ran 6 rounds; the later ones found 3 components of 79.** Already moved off the critical
   path; the rounds themselves are still the cheapest thing to cap.
6. **The judge's net contribution THIS RUN IS NEGATIVE.** 141 looks, 13 nudges, every one a re-stream
   that discarded a call's work. Two fixes landed (burst-gap rhythm, steer-lands-mid-generation) but the
   honest accounting is that supervision has destroyed more than it saved so far.

**THE THING WE CANNOT YET CLAIM.** The swarm's whole thesis is that parallel BUILD beats serial build.
**We have never reached BUILD.** So no mechanism here is proven either way — not the decomposition, not
the contracts, not the fan. The falsifier the plan already names is node occupancy during BUILD, and we
have never measured it. Until a run reaches BUILD, "our mechanisms are invalid" is unfalsified, not false.

## MEASURE THIS RUN'S END QUALITY BEFORE JUDGING THE SLICE CHANGE — Mihai, 2026-08-28

*"but for sure measure it after this run ends and we see the end quality. ok?"* — yes, and this run is a
CLEAN BASELINE: every change made today is committed but NOT BUILT, so none of it can touch it.

**THE BASELINE TO BEAT (run `swarm-3node-r0`, started 15:03:55Z):** 21 slices · 21 briefs ·
140,680 chars of spec · open 55.6m / research 25.9m / synthesis 7.4m / review 46.2m · REVIEW converged
5 findings -> 2. Score and file count TBD when it finishes.

**WHAT THE NEXT RUN MUST BE COMPARED ON, not argued about:**
- `plan_synthesized.tasks` vs `.distinct_files` vs `.tasks_sharing_a_file` — the over-decomposition
  ratio. This run's plan had FIVE slices for one file (`viz.js`) and four for endpoints of one service,
  against the NINE files a single engineer produced from the same request. At a ratio of 1.0 every task
  owns its own file and the fan is real; well under 1.0 the extra slices are pure overhead, because two
  tasks holding one file cannot build at once.
- `research_completed.brief_chars` — median was 6,443 against the 1,497 our own ledger measured as
  sufficient for 88.7%.
- Time to the FIRST app file. This run: never, in 136 minutes.
- `judge_quiet_within_rhythm` count — how often the burst-gap fix suppressed a false stall.
- Nudge delivery split. This run: 15 nudges, 15 re-streams, 0 steers.

**DO NOT tune any of these further until that comparison exists.** Two prompt changes are already in
(slice file-disjointness, brief concision) and they are the things being measured; stacking more blind
changes destroys the comparison. REVIEW IS EXPLICITLY NOT ON THE LIST — Mihai: *"review is good, 2 finding
in 20 min is excellent"*, and he is right; it produced a structural `add` in round 2. I had wrongly listed
it as over-engineered.

## THE JUDGE — five fixes landed today, ALL UNMEASURED. Stop and measure before touching it again.

Mihai: *"the judge seems to be a pretty piss poor implementation, you need to improve heavily, make it
smarter, make it bring more value not weight."* Fair on the evidence he had. What was actually wrong, and
what is now fixed but NOT YET IN ANY BINARY:
1. `6ecfe77cb` the ETA token became the worker's whole direction — `next: "ETA=5m"` — because the stripper
   only looked at the FIRST "ETA" in a line, so `metadata`/`details`/`theta` shielded the real one.
2. `f3cfbdbbd` tail similarity armed the looping streak on a PRODUCING call, because a coverage TABLE is
   repetitive by construction. Verdict `looping` at produced=4,006 with recurrence measured at 1.8%.
3. `2cdfaaa00` these models stream in ~2000/4000-char BURSTS with quiet gaps; a look landing in a gap read
   a healthy call as dead. Silence now only counts past the longest gap that same call already recovered
   from — self-calibrating, no seconds constant.
4. Steer lands MID-GENERATION and keeps the partial, so a nudge stops being destructive.
5. `can_steer` no longer needs a prior tool call, which also repairs the DRIFTING path honestly: drifting
   acts uncorroborated on the stated grounds that it costs "one in-session message" — true for a steer,
   false for the re-stream it always fell through to. Caught re-streaming producing calls TWICE
   (produced=4,001 and 4,004).

**WHAT IS NOT WRONG, corrected after I nearly "fixed" it:** the look CADENCE. 152 looks looked wasteful
against 17 nudges, but a cost backoff already exists (60s for the first 6 looks, then 300s), so that is
~14 looks per lane across 10+ lanes over 2h20m. Bounded and reasonable. Second time today I have started
to fix a mechanism that was working — see the nudge-justification entry.

**DO NOT ADD MORE JUDGE MECHANISM UNTIL THESE FIVE ARE MEASURED.** The next run must report:
`judge_quiet_within_rhythm` count, the nudge delivery split (this run: 17 nudges, 17 re-streams, 0 steers),
and whether any nudge still fires on a call producing >=2000.

## OVER-DECOMPOSITION DEFORMS THE PRODUCT — proven by REVIEW's own three rounds, 2026-08-28

The strongest evidence of the session, and it came from the run itself rather than from argument.

**ROUND 1 (new=5)** found exactly the over-decomposition the slice fix targets:
  `web/viz.js` owned by **FOUR** tasks (viz-picking-camera, viz-labels-brush, viz-streaming-instrumentation,
  viz-webgl-rendering) — stripped from three. Plus `app/notifierd.py`, `app/drafts.py`, `web/app.js` and
  `DECISIONS.md` each owned by two.
**ROUND 2 (new=2)** found the consequence: tasks left owning no files at all.
**ROUND 3 (new=7)** is REVIEW REPAIRING ITS OWN DAMAGE, and the method is the problem:
  viz-labels-brush -> `web/viz_labels.js` · viz-streaming-instrumentation -> `web/viz_streaming.js` ·
  viz-webgl-rendering -> `web/viz_webgl.js` · frontend-drafts-panel -> `web/drafts.js` ·
  draft-api-endpoints -> `app/drafts_api.py`.

**IT IS SPLITTING `viz.js` INTO THREE INVENTED FILES TO JUSTIFY THE SLICE COUNT.** REVIEW emits STRUCTURAL
PATCHES ONLY — by design it may not merge slices or edit a description — so when a task is orphaned its
only available lever is to invent a file for it. The decomposition therefore propagates into the product:
we will build `viz_labels.js`, `viz_streaming.js` and `viz_webgl.js` because OPEN made five slices for one
file. **The plan deforms the product to fit itself.**

**THIS IS NOT A REVIEW DEFECT.** REVIEW did good work three times: it found the collisions, found the
orphans, and repaired them with the only tool it has. Mihai is right that REVIEW is earning its keep. The
defect is upstream, at OPEN, and the fix is already committed: a slice must own files no other slice owns,
so the merge happens where merging is legal.

**ALSO SETTLED: the de-dupe is NOT broken.** `repeated=0` on all three rounds looked like the prefix-match
failing, which is what the previous run died of. Reading the texts shows the rounds are a genuine CASCADE —
round 1's fix caused round 2's finding caused round 3's fix — not three rephrasings of one complaint.

**ROUND 4 CLOSES THE ARC — REVIEW CAUGHT ITS OWN DEFORMATION:** *"The request explicitly requires exactly
4 frontend files (index.html, styles.css, app.js, viz.js), but the plan creates 9 by splitting app.js into
drafts.js and…"*. So: round 1 stripped the shared files, round 3 invented files to un-orphan the tasks,
round 4 noticed the invented files violate the request. REVIEW is CORRECT at every step and is now
fighting itself.

**CORRECTION, 18:12Z — IT DID SETTLE, AND THAT IS WORSE.** I claimed the constraints had no fixed point
and REVIEW would oscillate forever. Wrong: rounds ran 5 -> 2 -> 7 -> 3 -> 1 and then SETTLED into
CONTRACTS. What actually happens is that the reviewer recognises it cannot fix the problem — it may not
merge slices — so round 5 raises the violation prefixed `STILL:` ("the request's performance budget
explicitly names exactly 4 frontend files … but the plan …") and then asks for no further change. The
stop rule fires and the run proceeds **with a plan that knowingly violates the request**. So the failure
is not non-termination, it is SETTLING ON A KNOWN-BAD PLAN, which is quieter and worse. The cycle
detector still earns its place — it bounds the oscillating case — but the real fix remains OPEN's
file-disjointness rule, and the falsifier is unchanged.

**THE ORIGINAL (over-stated) ARGUMENT, kept because the counting is still right:** Counted from the run:
**10 frontend/viz slices** — frontend-html-structure, frontend-css-styling, frontend-table-interactions,
frontend-drafts-panel, viz-picking-camera, viz-labels-brush, viz-streaming-instrumentation,
viz-layout-transforms, viz-webgl-rendering, viz-records-endpoint — against the **4 files the request
permits**. With disjoint ownership required, NO VALID ASSIGNMENT EXISTS. REVIEW must either orphan tasks
(round 2's finding) or invent files (round 4's finding); there is no third option and no fixed point.
Rounds so far: 5 -> 2 -> 7 -> 3, and the stop rule is a round with NO new finding, which cannot occur.

**THIS IS THE COMPLETE DIAGNOSIS OF THE 136-MINUTE PLANNING PHASE.** Not slowness, not a weak reviewer:
OPEN produced a decomposition finer than the product is allowed to be, and every phase downstream pays
for it forever. The fix is committed at the only place it is legal — OPEN merges slices that would share
a file, keeping both concerns named in the merged objective.

**WATCH ON THE NEXT RUN:** `plan_synthesized.tasks_sharing_a_file` must be 0. If it is, this cascade cannot
occur, round 1's findings should concern COVERAGE rather than ownership, and REVIEW should reach its
no-new-finding round instead of oscillating.

## THE NEXT BUILD — what it must contain, and what it must prove

Everything below is committed on `local-edition` and NONE of it is in the running binary (installed
18:02 local). The run in flight is therefore a clean baseline for all of it.

**THE TWO THAT MATTER MOST, both from the same root cause:**
- OPEN merges slices that would share a file. Prevents the unsatisfiable plan — 10 frontend/viz slices
  against 4 permitted files — that has held REVIEW for 80+ minutes with no fixed point.
- REVIEW ends on a repeated plan STATE. An exact cycle proof, not a round cap; it would have caught this
  run at round 3 instead of hour two.

**THE JUDGE, five fixes:** ETA no longer becomes the direction · tail similarity cannot arm the streak on
a producing call · silence is measured against the call's own recovered burst gap · a steer lands
MID-GENERATION and keeps the partial · steer is the default delivery again, which also restores the
DRIFTING path's stated justification.

**THE REST:** brief concision · `plan_synthesized` before REVIEW with `distinct_files` /
`tasks_sharing_a_file` · append-only reasoning transcript (kills the 24k tail clip) · the REVIEW file
manifest · fleet node cards + full-stream modal · event log separates action from observation · fleet
row needs corroboration before reading "working" · Alibaba Token Plan contract.

**WHAT THE NEXT RUN MUST PROVE, in order of how damning failure would be:**
1. `plan_synthesized.tasks_sharing_a_file` == 0. If not, OPEN's rule did not take and everything
   downstream repeats.
2. It REACHES BUILD. No run has. Until one does, the swarm's central thesis — that parallel build beats
   serial build — is untested, and one cloud instance of the same model has 36 files to our 0.
3. Time to the FIRST app file. Baseline: never, in 136 minutes.
4. `judge_quiet_within_rhythm` > 0 and nudges delivered as `steer` rather than `restream`.

Run `python3 ~/goose-builds/loop-state/compare_runs.py` — it scores all of this against the baseline.

## FREEZE THE WEBSITE REPO UNTIL THE CLOUD QUEUE DRAINS — 2026-08-28, learned the expensive way

**DO NOT COMMIT ANYTHING TO `~/Projects/LeanZero-website` WHILE ANY CLOUD CAMPAIGN IS LIVE.**

`cloud_sb7.py init` FREEZES the publisher at a commit and verifies it at publish time. Any commit to that
repo — even one touching nothing the run uses — invalidates every campaign in flight.

MEASURED: **qwen3.8-27b ran 151 minutes, built 60 app files and SCORED 0.2006 (inner 0.7662)** — then
died at the last step with `publication dry-run-validation failed: pinned publisher cannot be verified:
publisher website commit changed after freeze`. The cause was me registering the six new entrants in that
repo while the campaign was running. The result is preserved on disk and is NOT published.

**All seven entrants are already registered, so no further commits are needed.** The chain has six
campaigns left; touching that repo now loses each of them the same way, at the very end, after the money
and hours are spent.

**qwen3.8-27b RESULT, unpublished:** score **0.2006**, inner 0.7662, 60 app files incl. the full frontend,
151 min, $3.93, 56 admitted requests. For scale that is **7.3x the local published target of 0.0273** and
about half of glm-5.3-flash's 0.4159. Publish deliberately once the queue is drained and the publisher is
stable — do not retry blind, `resume` may re-run the episode.

## FIRST RUN TO REACH BUILD — 2026-08-28 18:18:33Z (21:18 EEST). Findings for the next one.

**IT GOT THERE.** open 55.6 · ask 1.3 · research 25.9 · synthesis 7.4 · review 94 · contracts 10 ·
**BUILD**. 23 tasks, 32 files, **0 files with more than one owner**, `integrate-verify` present owning
nothing, median description 6,312 chars. REVIEW's ownership work landed completely. First app files any
local run has ever written: `README.md`, `DECISIONS.md`, `app/db.py`.
**Time to first file: 4h15m.** glm shipped 14 files and PUBLISHED 41.59% in 72 minutes total.

**FINDING 1 — A TASK REVIEW ADDS GETS A SENTENCE; A TASK OPEN PRODUCES GETS A SPECIFICATION.**
`frontend-notifications-feed` shipped to BUILD with a **182-character** description against a 6,312
median. It is the task REVIEW invented in round 2 because nothing owned the notifications feed — so it
never had a slice, never had an owner, and never went through RESEARCH. The whole DETAIL-fan deletion
rests on "a worker's entire instruction is its slice owner's brief"; a REVIEW-added task has no owner, so
it gets whatever sentence the reviewer typed. **FIX: route REVIEW's `add` through the same late-research
path coverage now uses (`coverage_late_slices`), so an added task is researched before BUILD like any
other.** The machinery already exists.

**FINDING 2 — THE DEFORMATION SURVIVED INTO THE BUILD.** The plan builds NINE frontend files
(`index.html`, `styles.css`, `app.js`, `viz.js` + the invented `notifications.js`, `drafts.js`,
`viz_labels.js`, `viz_streaming.js`, `viz_webgl.js`) against a request that names FOUR and caps them at
150KB. REVIEW flagged it `STILL:` and settled anyway. Already fixed upstream at OPEN (file-disjointness);
this run is the proof of what it costs when it is not.

**FINDING 3 — REVIEW COST 94 MINUTES, and Mihai is right that it earns its keep.** It found every
ownership collision and fixed all of them. The cost is not REVIEW's fault: it was cleaning up an
over-decomposition it was never allowed to fix at the root.

**WHAT TO MEASURE FROM BUILD, now that we are finally here — the thesis has never been tested:**
node occupancy across the 3 nodes, whether the fan actually parallelises, and whether the 9-file frontend
deformation reaches the product.

## THE FALSIFIER PASSED — parallel BUILD is real, measured 2026-08-28 18:18:33Z

The plan names node occupancy during BUILD as the falsifier for deleting the 8 deterministic rewrites,
and it has never been measurable because no run reached BUILD. It did.

**SIX TASKS DISPATCHED IN THE SAME SECOND, TWO PER NODE, ALL THREE GENERATING:**
    18:18:33  service-boot-architecture   -> workhorse
    18:18:33  notifierd-consumer          -> mihai
    18:18:33  viz-picking-camera          -> gabee
    18:18:33  documentation-decisions     -> workhorse
    18:18:33  frontend-css-styling        -> mihai
    18:18:33  frontend-drafts-panel       -> gabee
Then `frontend-html-structure` at 18:23:36 and `frontend-notifications-feed` at 18:24:25 as nodes freed.
At 6.6 min: 8 dispatched, 2 completed, 6 in flight, 12 files on disk, **0 idle nodes**.

**So the fan works.** The DAG parallelises, work-stealing refills a freed node, and the zero-collision
file ownership REVIEW produced is what makes it legal for six tasks to write at once. Everything upstream
of BUILD is what costs us — 4h15m to the first file — not BUILD itself.

## THE LOG SWEEP FOUND WHAT EVERY DASHBOARD MISSED — BUILD IS UNSUPERVISED (2026-08-28)

Mihai asked whether I had checked the logs for something essential. I had not swept them systematically.
Doing it — counting EVERY event type and looking at the ones I had never opened — found seven blind spots
and one that matters enormously:

**44 `judge_skipped`, ALL in BUILD, all `reason: no_idle_device`.** Against 32 successful looks in the
same phase: **58% of supervision during BUILD is silently dropped.** The judge needs a device with a free
parallel slot; each node is `PARALLEL=2`; the fan dispatches 2 tasks per node and saturates them. Every
cap was deleted from this engine on purpose, so the judge is THE ONLY STOPPER — and it is missing exactly
where a spiral costs most. Invisible in `judge_look` counts (which stay healthy) and in the fleet strip
(which correctly shows 3 busy nodes). See skill §4k for the mechanism and the trade-offs.

**THE OTHER SIX BLIND SPOTS, now checked and benign:** `judge_observed`(66)/`judge_verdict`(65) — the
deterministic path, working. `force_write_decision`(11) — `armed:false, lever_on:false` on every task, a
deleted lever still emitting an inert record. `pitfalls_delivered`(11)/`rules_delivered`(11) — 3.3-4.2k
chars reaching every worker, working. `entry_files_required`(1) — `app/__main__.py`, pre-dag, correct.
`fan_last_outstanding`(1) — the contract fan's straggler, expected.

**THE LESSON, and it is the reason this sweep must be routine:** every number I was watching all day
looked fine. Blind spots do not announce themselves — they are the events nobody wrote a reader for.
**Count every event type at least once per campaign and open the ones you have never opened.**

## THE CHAIN'S LAUNCH GATE WORKED — and its failure mode was stopping instead of skipping (2026-08-28)

`18:51:13Z HOLDING hy4-preview: projected $11.27 > remaining $9.97.` Exactly right: it refused to START a
run it could not finish, which since caps were removed is the only way to avoid a run dying at 90% for
want of credit. **But it then EXITED**, stranding FOUR affordable models behind one expensive one —
ling-3.0-flash ($0.28), laguna-s-2.1 ($1.20), longcat-2.0 ($4.12) and both Seeds, none attempted.
One model being unaffordable says nothing about the next. Now it SKIPS and continues, leaves the model in
the queue for a later run, and reports what it skipped when the queue drains.

**BOARD SO FAR — and the ordering is worth staring at:**
| model | score | time | files | frontend files |
|---|---|---|---|---|
| **deepseek-v4-flash-vision-exp** | **67.53%** | **33 min** | 13 | 4 (exactly as the spec names) |
| glm-5.3-flash | 41.59% | 72 min | 14 | — |
| qwen3.8-27b | 20.06% | 151 min | 60 | — |
| our 3-node swarm (target) | 2.73% | — | — | 9 |

The winner is the FASTEST and the SMALLEST. Thirteen files, four frontend files, 33 minutes, one agent,
no planning phase — against our 23 tasks, nine frontend files and 3h15m before BUILD started. Every
argument for thinning the engine is in that table.

## THE LAUNCH GATE BLOCKED WORK ON AN INVENTED NUMBER — 2026-08-28

`18:51:13Z HOLDING hy4-preview: projected $11.27 > remaining $9.97.` **Both halves were wrong.**

1. **The balance was invented.** `/api/v1/key` reports `limit: null` on a pay-as-you-go account, so I fell
   back to a HARDCODED `$15`. The real balance was **$40.22**. Every rule in this file forbids inventing a
   number and I built a gate on one. **`/api/v1/credits` returns `total_credits` and `total_usage`; their
   difference is what the account page shows.** Ask the provider — never assume the balance.
2. **A hold stopped the whole queue.** It raised `SystemExit`, stranding FOUR affordable models behind one
   expensive one — ling-3.0-flash ($0.28), laguna-s-2.1 ($1.20), longcat-2.0 ($4.12), both Seeds. One
   model being unaffordable says nothing about the next. It now SKIPS, continues, and reports what it
   skipped.

**hy4-preview was skipped by the old chain and needs one more pass after the queue drains** — the running
chain read the model list at start, so editing it now does not reach it.

## BOARD, 2026-08-28 — the winner is the fastest AND the smallest

| model | score | time | files | frontend files |
|---|---|---|---|---|
| **deepseek-v4-flash-vision-exp** | **67.53%** | **33 min** | 13 | 4 — exactly what the spec names |
| glm-5.3-flash | 41.59% | 72 min | 14 | — |
| qwen3.8-27b | 20.06% | 151 min | 60 | — |
| our 3-node swarm (published target) | 2.73% | — | — | 9 |

One agent, no planning phase, 33 minutes, thirteen files — against our 23 tasks, nine frontend files and
3h15m before BUILD even started. Every argument for thinning the engine is in that table.

## §4k CONFIRMED IN THE WORST WAY, AND FIXED — BUILD ran 45% blind (2026-08-28 19:04Z)

The unsupervised-BUILD finding stopped being a trade-off and became measured damage.

**NINE OF TWENTY BUILD TASKS WERE NEVER SUPERVISED ONCE:** event-ledger-outbox, frontend-css-styling,
frontend-html-structure, **ledgerd-api-layer**, notification-materialization, notifierd-consumer,
stream-sse-endpoint, viz-records-endpoint, viz-records-validation-tests. `judge_skipped` reached **84**,
every one `no_idle_device`.

**AND ONE OF THEM STALLED WHILE UNWATCHED.** `ledgerd-api-layer` — owner of `app/api.py`, `app/routes.py`,
`app/serve.py`, i.e. the app's entire HTTP surface — was dispatched 18:55:54 and sat EIGHT MINUTES with
ZERO tool calls, its last words *"Let me write these"*. Zero judge looks, ever. Found only because the
`STALE, NOT DONE` lane column added an hour earlier fired on it.

**THE FIX (scheduler.rs `supervision_device`):** supervision prefers a free slot and now NEVER returns
nothing while a device is enabled. `weight` is the ENGINE's dispatch budget, not the provider's — LM
Studio serves `PARALLEL=2` per node and queues beyond it, so a judge call on a full node is a QUEUED SMALL
REQUEST, not a dropped check. **A late look beats no look.** Build dispatch still never oversubscribes;
this path is judge-only. 72+6+42 swarm tests green.

**WHY IT HID FOR A WHOLE DAY:** `judge_look` counts stayed healthy (58 in open, 114 in review) because
planning phases leave slots free by accident. The fleet strip correctly showed three busy nodes. Nothing
was wrong on any dashboard. It was visible ONLY by counting `judge_skipped` by phase — an event nobody had
ever read.

## [FIXED 2026-08-29] REVIEW REPORTED 4 UNOWNED FEATURES AND PATCHED NOTHING — `68d89b965`

`review_findings` round 1, run `swarm-3node-r0`: **`new: 4, patch_touches: 0`**.

    SSE streaming endpoint (GET /api/stream) with batch numbering — not explicitly owned by any task
    vs7dbg debug API (8 methods on window.vs7dbg)                — not explicitly owned by any task
    Screen-space labels with collision culling and occlusion     — not explicitly owned
    Linked brush with table<->instance sync and dimming          — not explicitly owned

The reviewer named four things nothing owns and **proposed an owner for none of them**. The run went to
CONTRACTS and BUILD with all four unowned, which means they are absent from the finished program and no
later phase can notice — the builders build the list, the reviewer reviews the list, and the missing part
is never mentioned again. **This is precisely the failure that left the last published local run at 0.0273
with `GET /` 404ing**, and it is why the coverage table exists.

Worth naming: the machinery WORKED. The coverage fan found the gaps, REVIEW reported them, the event
recorded them. The only broken link was that a finding with no patch changes nothing, and the engine
accepted that silently.

**THE FIX:** when a round returns fresh findings and an EMPTY patch, ask once — naming its own findings
back to it — and demand a patch only: `add` a task that owns each, or `replace` one whose files should own
it. A problem that turns out to be already owned is left out, but then it was not a finding. Emits
`review_patch_demanded {round, findings, patch_touches}` so the next run says whether the demand worked.

**THIS RUN KEEPS THE DEFECT** — the binary is the old one. Expect 4 features missing from its app, and read
the score with that in mind rather than as a verdict on the decomposition.

## `judge_skipped` IS NOT ONE THING — READ ITS `reason`, 2026-08-29 02:25 EEST

BUILD showed `judge_skipped: 1` after a tick at 0, and the instinct from the 45%-unsupervised incident was
that supervision had regressed. It had not. The single event reads:

    {"event":"judge_skipped","task_id":"frontend-app-js","reason":"unchanged_since_last_review"}

That is a DEDUPLICATED look — the call had not changed since the previous review, so re-judging it would
cost a look and tell the judge nothing new. **Correct behaviour, and the opposite of a supervision gap.**

**So "judge_skipped must be 0" was the wrong falsifier.** The right one is `judge_skipped` grouped BY
`reason`: `unchanged_since_last_review` is healthy at any count; anything else is a call that went
unwatched and must be opened. Recorded because the blunt version would have sent the next session chasing a
regression that is a working optimisation.

BUILD at this point: **9 dispatched, 7 of 11 complete, 0 failed, 0 retries**, and the tree holds
`app/{db,ledger,ledgerd,notifierd,approval,outbox,webhooks}.py`, `app/sync/{engine,upsert}.py`,
`web/{index.html,styles.css,viz.js}` — plus a `graded-sb7-db/` with real ledger and notifier databases,
meaning the app it built has BOOTED AND RUN.

## THE SHARED FILE, IDENTIFIED — and the local run BUILT ITS FRONTEND, 2026-08-29 02:05 EEST

`tasks_sharing_a_file: 1` resolved by hand (the `shared_files` fix will name it automatically next run):
**`viz-scene-rendering` and `viz-camera-picking-interaction` both own `web/viz.js`.**

The scheduler serialised them correctly. The first wrote the whole file — **20,945 bytes containing camera
(37 refs), pick (29), brush (7) and label (7)** — and the second then ran with nothing left to do and
completed having made **ZERO tool calls**. Benign THIS time, and only because the first task implemented
both halves; had it written only its own, the camera and picking work would have been silently absent or
overwritten. Fixed by REVIEW question 6b (`b2f...`), which names the pair and demands one owner.

**THE HEADLINE, AND IT IS THE POINT OF THE WHOLE CAMPAIGN:** the run has **`web/viz.js` (20,945 B),
`web/index.html` (4,050 B) and `web/styles.css` (5,052 B) on disk** — plus `app/db.py`, `app/ledgerd.py`,
`app/notifierd.py`, `app/ledger.py`, `app/sync/engine.py`, `app/sync/upsert.py`. The last published local
run scored 0.0273 with **no web frontend at all** and `GET /` 404ing, roughly 0.56 of the scoring weight
unreachable. The previous run's `viz-picking-camera` made zero tool calls for 76 minutes and never wrote
that file.

BUILD at 23 min: **8 dispatched, 5 completed, 0 retries, 0 failures**, 19 files, fleet 3/3 busy,
`judge_skipped` 0, 4 re-streams prevented, every nudge a steer.

## THREE OF FOUR CLOUD ENTRANTS DIED TO HARNESS RULES, NOT TO MODEL FAILURE — 2026-08-29

| entrant | admitted / terminal | spent | what it built | what killed it |
|---|---|---|---|---|
| seed-2-1-turbo | — | $12.29 | 152 KB, flat | **the model** — 509 shell calls, exploring not building |
| seed-2.0-code | 13 / 12 | $0.22 | nothing | terminal-finish-reason guard |
| laguna-s-2.1 | 17 / 16 | $0.09 | 4 files | finish-reason guard + ambiguous request |
| **longcat-2.0** | **102 / 101** | **$2.41** | **20 files, app booting** | **ONE ambiguous request out of 102** |

**longcat is the expensive one.** `failure: 1 provider request(s) retain full budget reserves; admission or
terminal usage is ambiguous and the episode is never retried`. It had already produced a COMPLETE app —
13 Python modules (`ledgerd`, `notifierd`, `sse`, `events`, `database`, `vendor`, `sync`…), `web/index.html`,
`web/styles.css`, `web/viz.js`, `web/app.js`, plus README and DECISIONS.md — and was booting it
(`python3 -m app --db-dir data --ledger-port 8080`, reading `logs/boot.log`, clearing ports). 2,360 seconds
and $2.41 discarded over a bookkeeping ambiguity in **one request out of 102**.

**THE RECOVERY, and it is a durable procedure:** the tree survives the campaign's verdict, so score it
directly with the campaign's OWN recorded seed —

    SEED=$(python3 -c "import json;print(json.load(open('<root>/entrants/<id>/state.json'))['fixture_seed'])")
    python3 score_sb7.py --tree <root>/entrants/<id>/tree --seed $SEED --json-out out.json

Serial, hermetic, at the advertised port, replaying the recorded seed rather than drawing a fresh one — so
the number is comparable to the published board. **An INCOMPLETE campaign is not an unscoreable tree.**

**THE HARNESS QUESTION THIS RAISES** (recorded, not acted on): one unresolved request out of 102 voiding a
finished build is a rule with the wrong blast radius. It exists so an ambiguous spend cannot be
under-counted, which is right — but it should void the ACCOUNTING, not the RESULT.

## TWO CLOUD MODELS KILLED BY ONE HARNESS RULE — the terminal-finish-reason guard, 2026-08-29

`seed-2.0-code` (12 of 13 requests) and `laguna-s-2.1` (16 of 17, $0.09, 908k tokens) both died with:

    Usage data error: Provider stream ended before a recognized terminal tool-call finish reason.

That is `goose-provider-types/src/formats/openai.rs` refusing to yield a tool call whose stream ended
without a `finish_reason` — pinned by two tests (`deepseek_partial_tool_usage_at_eof_is_not_terminal`,
`terminal_safe_partial_tool_at_eof_is_never_yielded`). **THE RULE IS CORRECT**: accepting a truncated tool
call is worse than failing. But it is now rejecting a meaningful fraction of OpenRouter models, and it costs
nothing to say so — laguna spent $0.09 and produced 4 files before dying, seed $0.22.

**NOT A CAP AND NOT TO BE RELAXED.** Recorded so the next session does not read two independent "model is
broken" verdicts and re-derive the shared cause. If a third model dies this way the question becomes whether
the retry path (`terminal_safe_retries_enabled`) should retry a stream that ended cleanly but without the
marker — a provider-format question, not a swarm question.

**AND THE OPPOSITE RESULT ON THE SAME TICK:** `longcat-2.0` reached **18 source files / 154 KB with its app
RUNNING** — `ledger.db` at 4.1 MB plus two 2.0 MB SQLite WAL files, written by its own booted service. That
is the deepseek shape, and it is what the local fleet has never produced.

## THE OVER-DECOMPOSITION FALSIFIER: MOSTLY PASSED — measured 2026-08-29 01:22 EEST

`plan_synthesized` on run `swarm-3node-r0`:

    tasks 11  (was 21 before the fix)      distinct_files 16
    tasks_sharing_a_file 1                 module_package_collisions []
    files_per_task  [2,2,2,2,1,2,1,1,1,3,0]
    deps_per_task   [0,1,1,2,0,0,0,1,2,5,10]
    description_chars [5200,5191,4744,7518,5452,6029,5958,4681,8418,6441,3650]
    ids: ledgerd-core · vendor-sync · webhook-event-ledger · approval-workflow-outbox ·
         notifierd-service · frontend-structure-style · viz-scene-rendering ·
         viz-camera-picking-interaction · frontend-app-js · boot-wrapper-docs · integrate-verify

**WHAT PASSED, and each is a named checkpoint:** the join is called `integrate-verify` and **owns 0 files**
(the [R-M1b] rule — a file-owning join is cascaded Failed by any build failure, which is the "app never
binds a port" class); it depends on all **10** producers; **no task shipped a one-line description** — the
minimum is 3,650 characters; **zero module/package collisions**; and **four tasks own the frontend/viz**
(`frontend-structure-style`, `viz-scene-rendering`, `viz-camera-picking-interaction`, `frontend-app-js`) —
the allocation whose absence made the last published local run score 0.0273 with `GET /` 404ing.

**THE ONE DEFECT: `tasks_sharing_a_file: 1`.** The target is 0. Not fatal — the scheduler serialises two
tasks that own the same file — but it violates "A SLICE MUST OWN FILES NO OTHER SLICE OWNS".

**AND THE INSTRUMENT COULD NOT NAME IT.** The event counted the collision and carried no file names, so the
single defect in an otherwise excellent plan was visible and un-actionable. Fixed in `be655e662`:
`shared_files: [{file, tasks:[ids]}]`. Next run says WHICH file, and the OPEN prompt can then be aimed at
the actual pattern instead of at the rule in general.

**ALSO VISIBLE THIS PHASE:** REVIEW lanes are named from the request's own headings —
`review-6-notifierd-the-ide`, `review-screen-space-labels`, `review-build-app-meridian` — the
`cut_request_into_sections` fix working in the display, where the old char-balanced cut produced two lanes
both reading "Answers to slice questions".

## THE BURST-GAP FIX (AB) SAVED THIS RUN'S RESEARCH PHASE — measured 2026-08-29 01:18 EEST

`open-coverage-1` went **329 seconds silent** at 21:18:34 after enumerating 145,000 characters. Under the
OLD rule that is a five-and-a-half minute stall and the judge re-streams — **discarding the entire coverage
table** it had spent 50 minutes building. `judge_quiet_within_rhythm` fired instead:
`quiet=329s <= known_gap=338s`. The lane had ALREADY recovered from a 338s gap earlier in its own life, so
the silence was inside its measured rhythm.

**It then emitted.** `tools=1`, 145,514 chars, done — and the run advanced to SYNTHESIS with two nodes that
had been idle waiting on it.

Nine characters of margin between the silence and the high-water mark. A literal seconds constant anywhere
near that number would have been wrong in both directions; the mark is per-call and self-calibrating, which
is the only reason it fit.

## [DONE 2026-08-29] AND THE SAME BLINDNESS FOR A STRUCTURED REPLY — `46cc5ff4f`

`owned_block` covers build tasks. A PLANNING lane owns nothing, so it stayed empty and the judge again read
"enormous reasoning, no actions" as a call thinking hard.

MEASURED LIVE, this run, while the previous fix was being written: `open-coverage-1` reached **144,935
characters with ZERO tool calls across FIVE nudges** — the last two literally *"call final_output NOW"* —
while **two of three nodes sat idle** waiting on it. Every verdict was DRIFTING and never LOOPING, and that
was CORRECT: it produced ~4,000 FRESH characters between every look. It was not looping. It was enumerating
forever into a channel that is not the deliverable, and no detector in the engine could say so.

THE FIX: when a call owes a structured reply (`response.is_some()`) and `call_records` is empty, the judge
is told that, plus the consequence it cannot otherwise know — **if the call ends without that tool call,
everything it worked out is discarded and the phase gets nothing.** A partial table that exists beats a
complete one still being composed. `wants_structured_reply` is captured before `apply_recipe_components`
moves the `Response`.

NOTE THE PAIR: a file never written and a structured reply never made are ONE failure with two deliverable
types. Both were invisible for the same reason — the judge is handed an `activity_key` and nothing about
what the call OWES.

## [DONE 2026-08-29] THE LIVENESS RULE ACCEPTED REASONING AS PROGRESS — a build task's progress is a FILE

MEASURED 2026-08-28, BUILD at 76 min, 20 of 21 tasks done and `viz-picking-camera` unable to finish.
It owns `web/viz.js` — the 3D field, roughly **0.56 of the sb-7 scoring weight** — and after 76 minutes has
made **ZERO tool calls** and written nothing. 28 judge looks, 13 nudges, all re-streams, verdicts
looping -> looping -> RESTART -> looping.

**WHY IT CANNOT END.** §8.1's rule is "a RESTART is permitted only while the previous attempt produced
something — a tool call, a file byte, or NEW REASONING. Two consecutive attempts that produce nothing end
the task as Failed." This call produces ~460 characters of new reasoning after every restart, so it
satisfies the rule forever. `thinking_total` climbed 8,340 -> 8,800 while `thinking_chars` reset each time.
0 retries, 0 failures recorded. **A task that owns files can reason indefinitely and never build.**

**THE FIX SHAPE:** for a task that OWNS FILES, "produced something" must mean a tool call or a file byte —
reasoning alone is not progress, because reasoning is not the deliverable. Its missing files then become
REPAIR work, which is the designed outcome. Needs `owned_files` plumbed to the nudge site, which the
judge loop does not currently have.

**IMPLEMENTED 2026-08-29, both halves, `0eca49bbb` + `b318c21ff`.**
(1) `GooseAgentDispatcher.owned_files_by_task: Mutex<HashMap<String, Vec<String>>>`, published in
`TaskDispatcher::run` — `DispatchRequest` already carries `task_id` AND `owned_files`, so no plan-load
plumbing and no signature churn across the 14 `run_agent_timed_at` call sites was needed. The judge prompt
now lists every owned path with its REAL state on disk (`EXISTS, N bytes` / `EXISTS BUT EMPTY` /
`DOES NOT EXIST`) and states that bytes are the progress, not characters. Empty for planner-side calls, so
those are byte-identical to before.
(2) The judge's NEXT instruction now says ASK FOR THE SMALLEST ACTION THAT LEAVES A TRACE, never the whole
deliverable — the file, the stub, the imports, one function; for a structured reply, emit what it has NOW
and refine after. Escalating by being more specific about the whole artefact is what made the viz spiral
worse, because the whole artefact is exactly what the call could not finish.
These two are one fix: (2) says "if it owns a file, name the file" and until (1) the judge had no way to
know whether it did.
Clippy 104 pre-existing errors in the crate, NONE in the edited ranges — verified line-by-line against the
diff hunks, not asserted.

**AND THE PROBABLE CAUSE OF THE SPIRAL, which is cheap to address now:** every nudge asked for the WHOLE
file — *"write web/viz.js with the complete implementation (WebGL context, orbit camera, picking…)"*. That
is the same failure the cloud qwen3.8-27b showed for 40 minutes: composing an entire large file inside the
reasoning channel and never emitting it. Asking for a MINIMAL first write breaks the spiral, because a
file that exists can be extended.

**ALSO CLEARED THIS PASS (both previously unread):** `pre_review` — a per-task review after each build
task, 12 fired, 2 with findings, working. `tail_review` — dimension reviews (`interface`, `wiring`),
2 fired, no findings, working.

## THE FIXES ARE WORKING — first measurements from run 2 (2026-08-28 20:24Z)

| | run 1 (baseline) | run 2 | |
|---|---|---|---|
| OPEN | **55.6 min** | **9.2 min** | coverage moved off the critical path |
| OPEN -> RESEARCH | 57 min | **11.2 min** | |
| coverage | blocked OPEN, 6 rounds | runs BESIDE research | as designed |
| slice names | 21, many per file | semantic, fewer | ledgerd-core, notifierd-service, webhook-event-ledger, vendor-sync, approval-workflow-ou…, frontend-structure-s… |

`open@20:05:59 -> ask@20:15:11 -> research@20:17:11`. The single largest phase cost in the engine fell by
**46 minutes**, and coverage lanes now run alongside slice research in the same tick. Still to confirm at
SYNTHESIS: `plan_synthesized.tasks_sharing_a_file` must be **0**.

**AND I BROKE MY OWN READER WITH TONIGHT'S CHANGE.** The engine now writes `<task>.log` and
`<task>.think.log` beside each `<task>.json`; `tick.py` iterated the whole activity directory, so every
lane appeared TWICE — once parsed, once as "(being written this instant)" — because `Path.stem` strips only
the last suffix and `json.loads` fails on a transcript. **When you add files next to a file another tool
reads, check that tool.** Fixed: the lane listing reads `.json` only.

## THE CALLS-RISING-NOTHING-LANDING DETECTOR CAUGHT A REAL SPIRAL (seed-2-1-turbo, 2026-08-28 20:34Z)

Built two ticks earlier because a FILE COUNT cannot tell an editing model from a stuck one. First live
firing, and it was right.

**seed-2-1-turbo: Δcalls+132, ΔKB+0.** Breakdown of its 852 calls: **509 shell**, 110 tree, 49 read_file,
31 read_image, 53 write, 78 todo. The last six calls were `ls web/ && ls app/`, `curl /v3/docs | head`,
`tree app`, `curl /v3/docs | wc -l`, `curl /v3/docs | tail -200` — **it re-read the vendor docs endpoint
three times in six calls.** Its own todo still reads `[ ] Build project structure and module layout` after
95 minutes. Spend **$8.54 -> $12.17**, 75% over the $6.97 projection, with bytes flat at 152KB since 23:14.

**IT IS EXPLORING, NOT BUILDING** — the opposite failure to qwen3.8-27b, which composed the app inside its
reasoning channel and made no calls at all. Same symptom in the tick (nothing landing), opposite cause.
That is precisely why the tick now shows calls AND bytes AND thinking: any one of them alone reads the
wrong story.

**NOT STOPPED.** A detector flag is a reason to LOOK, never to act — the rule earned on 27b, which unfroze
by itself twenty minutes after I recommended killing it. Reported to Mihai with the evidence; his call.

## Standing constraints — absolute, never negotiate them

- **NO SPEND CAP MAY EVER BIND ON A CLOUD RUN.** Mihai, 2026-08-28: *"don't put caps on models or runs
  please cause otherwise they might get blocked because of that!!!"* The harness REFUSES the request that
  would cross `spend_policy.total_cap` / `provider_caps`, which kills the episode mid-build — a cap is a
  way to throw away a nearly finished run, and the harness even ships a `budget-blocked` recovery path
  because it has happened. The schema requires a positive number, so set it to **500.0** — far above
  anything reachable. The REAL limit is the account: the OpenRouter balance or Token Plan credits, which
  fail cleanly at the provider instead of inside the ledger.
  MEASURED: 27b burned **$1.99/hour and RISING** (input tokens grow with context), so an $8 cap bound at
  ~4h. Caught at 63 min and relaunched uncapped for $2.08 rather than lose the run at hour four. This is
  the same doctrine as [uncapped runs, judge decides] — no wall-clock, no volume, and no spend threshold
  may terminate work.

- **NO deterministic caps, timers, thresholds or gates.** A clock may SUMMON the judge or SUGGEST to a
  worker; it may NEVER CUT one. No new literal seconds constant as a mechanism, ever.
- **Never reconfigure the fleet** on my own initiative. `lms ps` + `pgrep` before ANY run.
- **Desktop app only** for local runs. No headless.
- **Kill on divergence** — never let a diverged run finish. Then make a DIFFERENT fix, never the same one.
- **Tick every 10 minutes, without exception**, for as long as anything is running.
- Temperature: leave it unset so the user's 0.7 applies. Never pin 0.2.
- UI: no left accent rail, no faded/washed-out colours, no native `<select>`/`alert`/`confirm`/`prompt`.
  LeanZero palette. Solid saturated accents.
- Commit EVERY change as it lands.

## TICK HYGIENE — do not let the tick lie to you

- **STOPPING A RUN IS A SCRIPT, NOT A pkill: `~/goose-builds/loop-state/stop_local_run.sh`.** It cancels in
  the app, kills all THREE goose command lines plus the harness, and refuses to exit 0 until `lms ps` shows
  zero GENERATING. MEASURED 2026-08-28: a hand-written `pkill -f 'Goose.app/Contents/MacOS/Goose'` matched
  the Electron binary only; `Resources/bin/goose swarm run` survived, reparented to launchd, and drove all
  three nodes for ~25 minutes after the window closed. `launch.sh` already had the right patterns — I
  retyped a shorter version instead of calling it. If a procedure exists, CALL IT.
- **CLOUD TICK SHOWS DELTAS, because absolutes hide a freeze.** "10 tool calls" read identically on two
  ticks eight minutes apart while thinking grew 4,000 chunks — the difference between a model working and
  a model talking to itself, invisible in the absolute. `tick.py` keeps `tick-state.json` beside it and
  prints `Δcalls / Δthink / Δfiles`, flagging **ACTIONS FROZEN, thinking still growing**. Proven live:
  Δcalls+0 Δthink+275 across 45s on qwen3.8-27b r2.
- **PAIR EVERY NUDGE WITH THE `produced_since_last_look` THAT JUSTIFIED IT — the direction text alone
  reads the same whether the nudge was right or wrong.** MEASURED 2026-08-28: four of seven directions
  looked like the task merely restated ("Restart the call to complete the structural patch analysis"),
  which reads as a judge defect and as evidence that the "must add information" fix had failed. The
  pairing showed **every one of the seven fired at produced=0..9** — genuinely dead streams, where
  restating the job is the CORRECT thing to say. I was one step from "fixing" a mechanism that was
  working. The tick now prints verdict + produced beside each nudge and flags one that hit a call
  producing >=2000.
- **`compare_runs.py` SCORES A RUN AGAINST THE 2026-08-28 BASELINE** — decomposition ratio, brief chars,
  per-phase time, time-to-first-app-file, and the judge's delivery split, each with a +/-% against the
  baseline. Hand-deriving these is how a comparison becomes an argument.
  **AN APP FILE MUST LIVE IN A SUBDIRECTORY.** Everything the engine writes lands at the run ROOT --
  `run.jsonl`, `heartbeat`, `engine-console.log`, and `patch.json`, which is REVIEW's own output. The
  first cut counted `patch.json` and reported "first app file at 96.1 min" for a run that had built
  NOTHING. The instrument was about to flatter us; the products the winners built were all `app/…` and
  `web/…`.
- **THE TICK NOW READS THE CLOUD RUNS TOO, and the number that matters is WHETHER ANYTHING LANDS.**
  MEASURED 2026-08-28: qwen3.8-27b sat 32 min with 4.7 MB of log and ZERO app files, composing the
  application inside its reasoning channel (`"thinking": " = document.getElementById('"`). Not stuck — it
  had made 10 tool calls — but 2,051 thinking chunks PER tool call where the same model's previous episode
  averaged 369 and finished with 53 calls. Neither `campaign=RUNNING` nor the spend showed any of it.
  CALIBRATION, because my first threshold flagged a winner: the RATIO ALONE proves nothing —
  **glm-5.3-flash PUBLISHED at 41.59% while running 1,223 think/call.** What separates them is whether
  work LANDS: glm turned 112 calls into 14 files; the 27b episode had 10 calls and an empty tree. The tick
  flags a RUNNING entrant with substantial reasoning and an EMPTY TREE. It only warns; it terminates
  nothing.
- **`lms ps` HAS THREE STATES, NOT TWO — `PROCESSINGPROMPT` IS BUSY.** MEASURED 2026-08-28: the tick
  printed "0 generating / 0 idle / 3 nodes" while ALL THREE nodes were prompt-processing at a phase
  boundary — a fully busy fleet reading as a dead one, which is a kill-checkpoint input. Counting only
  GENERATING and IDLE is the same neighbouring-question mistake as the rest of this section.
- **THE FLEET LINE MUST REPORT THE TOTAL, NOT JUST THE SPLIT.** MEASURED 2026-08-28: the tick printed
  "1 generating / 1 idle" for a fleet that had THREE nodes — one row was transiently absent from `lms ps`
  and nothing in the line said so, so a two-node fleet read as normal. A node missing from `lms ps` is
  exactly the departed-node case `live_fleet_slots` exists to handle, and the tick has to be able to see
  it. `tick.py` now prints `N generating / N idle / N nodes` and flags a total under 3.
- **NEVER READ A BUILD'S SUCCESS FROM A PIPELINE, AND NEVER FROM AN UNFINISHED BACKGROUND TASK.**
  MEASURED 2026-08-28, and it cost a BROKEN COMMIT on `local-edition`: I ran
  `cargo build ... | grep -E '^error' -A 8 | head -14; echo "exit=$?"`. `$?` there is HEAD's exit code,
  never cargo's — and when the same command ran in the background I read its output file BEFORE the
  compile had written anything, saw no errors, and committed. `goose-cli` had FOUR errors
  (`serde_json::json!` cannot take a block containing `let` in a value position; it reports it as
  "comparison operators cannot be chained" pointing at an unrelated turbofish).
  THE RULE: redirect to a file, capture cargo's OWN `$?` on the line immediately after, and print an
  explicit `BUILD OK` only on `-eq 0`. Absence of matched error lines is NOT success — it is also what an
  empty file looks like. This is the third instance today of a check answering a neighbouring question,
  and the first one that reached a commit.
- **A STALE LANE THAT IS `phase=done` IS FINISHED, NOT DYING — the tick now says which.** The digest
  stamps `phase: "done"` the instant a call ends, so a lane 20 minutes old with that stamp is a completed
  member of a fan whose straggler is still running: two idle nodes and one working is what a fan LOOKS
  like, and it is explicitly not a kill. MEASURED 2026-08-28: review-2 (974s) and review-3 (1239s) both
  read as alarming ancient lanes; both were `done`, with review-1 fresh at 14s. I hand-checked the stamp
  twice before putting it in the tick.
- **COMPARE TIMESTAMPS BY THE SECOND — `ts` CARRIES MICROSECONDS.** The BUILD falsifier in
  `compare_runs.py` grouped dispatches by exact `ts` equality, matched exactly ONE event, and printed
  **"SERIAL -- the DAG is over-constrained"** for the run whose six tasks had demonstrably dispatched in
  the same second across three nodes. A falsifier that reports the OPPOSITE of the truth is worse than no
  falsifier: it would have sent the next version chasing a parallelism bug that does not exist. Slice to
  `[:19]` before comparing.
- **`__pycache__` IS A BUILD ARTEFACT — exclude it from every file count.** It makes deltas lie in BOTH
  directions: seed-2-1-turbo read **+25 files then -50** across two ticks purely from pyc churn, and the
  same inflation once reported "first app file at 96.1 min" for a run that had built nothing. Fixed in
  `compare_runs.py` earlier the same day and left unfixed in `tick.py` — fixing the instance, not the
  class, again.
- **ARCHIVE MARKERS ARE A VOCABULARY — when you add one, update every reader.** `tick.py` skipped
  `-KILLED-` and `-DEBRIS-` but not `-ENDED-`, which I introduced the same evening, so it reported
  `phase=build (96m)` for a run that had been stopped four minutes earlier. The three markers now live in
  one tuple. Same shape as the `__pycache__` count: fixing the instance and leaving the class.
- **COUNT BYTES, NOT JUST FILES — an EDITING model looks identical to a stuck one.** seed-2-1-turbo added
  **331 tool calls across two ticks with Δfiles+0**, which is either steady refinement of existing files or
  a spiral, and a file COUNT cannot distinguish them. The cloud line now carries `files/NNKB` and `ΔKB`,
  and flags **CALLS RISING, NOTHING LANDING** when calls climb past 20 in a tick with zero byte movement.
  Same family as the `__pycache__` inflation: the number was measuring the wrong thing rather than
  measuring it wrongly.
- **THE TICK IS A SCRIPT: `python3 ~/goose-builds/loop-state/tick.py`.** Do not retype the reader. Three
  times today a hand-written check answered a NEIGHBOURING question and read as healthy: `pgrep -f
  run_build.py` matched the shell running the tick; a build-progress grep for cargo/rustc/electron/node
  counted VS Code helpers and MCP servers, so a finished build read as running for 11 minutes; and the
  judge pairing keyed on `activity_key`, which these events DO NOT CARRY — they carry **`task_id`** — so
  every probe fell into one '?' bucket and the hung-probe checkpoint was never actually evaluated at all.
  The script also skips any dir whose name contains `-KILLED-` or `-DEBRIS-`, because "newest dir" briefly
  pointed at orphan debris.
- **NEVER COUNT PROCESSES BY NAME PATTERN — anchor on the PID you started or the log you are writing.**
  MEASURED 2026-08-28: a build-progress check of `ps aux | grep -cE '[c]argo|[r]ustc|[e]lectron|[n]ode.*build'`
  counted 4 and I reported the build still running. All four were VS Code helpers and MCP servers; the
  build had finished 11 minutes earlier. Mihai asked why I was watching VS Code. Same shape as the
  `pgrep -f run_build.py` trap two entries up, and the third time this class has cost something today.
  A background job returns a PID — poll `kill -0 <pid>`, or the mtime of the log it writes. Both are things
  the observer cannot accidentally contain.
- **Never "stop the fleet" by unloading.** `lms load` has no host flag and gabee (Mac.lan) has no
  passwordless SSH, so an unload there cannot be undone from this laptop. IDLE is the correct resting
  state; stop whatever is DRIVING the fleet instead.

- **`pgrep -f run_build.py` MATCHES THE SHELL RUNNING THE TICK.** The tick command contains that string,
  so pgrep finds its own zsh and reports `run_build RUNNING` when the harness is dead. Observed 12:44Z
  with a 3/3 IDLE fleet and the run dir already archived. Use `pgrep -f 'Python.*run_build\.py'` — anchor
  on the INTERPRETER, so only a real harness process matches.
- Same shape as every other defect this week: a check that answers a neighbouring question. Anchor every
  process check on something the observer itself cannot contain.

## Kill checkpoints — DELIBERATELY NARROW. Slowness is NOT a kill.

Rewritten 2026-08-28 after counting the kills. Nine runs died; where they died:

    open x2 · research x2 · synthesis x1 · review x4 · integrate x1 · rate x1

Two of those got a long way — `undercovered-0738` reached RATE, meaning it went through BUILD, INTEGRATE
and TEST into REPAIR and produced a scored app (0.0023). `nudge-loop-2257` reached INTEGRATE. So the
pipeline DOES run end to end. It was killed for PLAN QUALITY, not for hanging — and plan quality is the
thing the coverage rewrite has now fixed.

One kill was simply WRONG: `review-3h-silent-1036` was healthy, and I killed it on a local-time-minus-UTC
subtraction that invented a three-hour gap.

So the posture changes. A kill costs hours and cannot be undone; being slow costs only time and often ends
in a finished run. KILL ONLY ON A PROVEN WEDGE OR A PROVEN DEFECT:

| kill when | proof required — all of it, not one snapshot |
|---|---|
| the engine is wedged | NO new event in `run.jsonl` AND no `.swarm/activity/*.json` mtime movement, sampled >=3 times over >=90s, AND `lms ps` shows the fleet idle |
| a lane died after a re-stream | its digest has `thinking_chars: null` or frozen for >5 min AND its last judge verdict was drifting/looping AND no other lane is still feeding the fan |
| a correction is a full re-emission | `plan_patched` absent where the plan changed |
| the join is misnamed or owns files | `plan_loaded.tasks[]` |
| a task dispatches with a one-line description | `plan_loaded.tasks[].description` length |
| anything is stopped by a clock | any `agent stalled` text |
| a judge look genuinely hung | `judge_look_dispatched` with NEITHER `judge_look` NOR `judge_look_abandoned`, paired PER TASK_ID |

**NOT kill conditions, and each of these has caused a wrong kill:** a phase taking a long time; nodes idle
while a fanned straggler finishes (that is what a fan looks like); a judge probe outstanding (it is raced
against the stream now, and abandoned looks are normal); a plan that looks under-covered mid-run — coverage
runs several rounds and the gaps arrive in the later ones.

**Before any kill, state the UTC clock explicitly.** `date -u`, and compare UTC to UTC.

## DONE (committed on `local-edition`)

- [x] `010f43fe1` clarify proxy race — answers were computed, logged as success, thrown away
- [x] `845b35713` deleted the three 420s stopwatch verdicts + tail-review cap (were parked behind `&& false`)
- [x] `ad0b1d0ea` Synthesize row stops reading as live when Review opens (was a permanent display lie)
- [x] coverage fans across the fleet (`4e5cec44d`)
- [x] GLM-5.3-Flash cloud campaign launched and publishing (see below)

## TODO — in order. Check items off IN THIS FILE as they land.

- [x] **A. REVIEW wedge — FIXED.** Root cause: the judge's own model call was `.await`ed serially inside
      the stream loop it supervises, and `judge_look` is emitted AFTER it, so a hung supervisor froze the
      worker and logged nothing. The probe now races the stream; deferred events drain in order; a call
      that finishes while the judge is out abandons the look, not the result. Detector shipped:
      `judge_look_dispatched` before the call — a dispatched with no matching `judge_look` = hung judge.
- [x] **B. Virtual nodes — DONE.** Node-first unified list (Node A/B/C, provider chip per node, union of local+cloud — 02c79ac77), per-node Smartest/supervisor (67b431faf), '+ Add node' picker, engine fields carried across pool rebuild (d4fdbeeb2). Earlier note: Engine: SwarmDevice gains speed_weight+supervision, threaded into DeviceCfg and carried across pool rebuild (d4fdbeeb2). UI: per-node Smartest/supervisor control shipped. Cloud-node-per-provider already existed (CloudPane). REMAINING: unified Node A/B/C list with `+`, provider picker per node in one place. Original ask: — a Node is a slot that picks a PROVIDER + MODEL.
      Settings > Swarm > Nodes. Node A picks a provider; LM Studio populates from loaded models; cloud
      providers must be configured first to appear. `+` adds Node B, C… Each node independently chooses.
      Per-node role hints: which is FASTEST, which is SMARTEST. Engine must consume the role hints.
- [x] **C. DONE — all three sub-items were ALREADY BUILT; I verified rather than assumed, and nearly
      shipped a duplicate.** (a) known-active-bugs panel: exists at SwarmRunPanel.tsx:2668, labelled
      "the run passed — these are what it passed WITH". I added a SECOND one in error red; the existing
      smoke test caught the duplicate ("Found multiple elements with the text: Known active bugs") and I
      reverted it (d264c278c reverted). (b) phase chips read the engine `phase` event via `foldRunPhase`,
      with a test named "the ribbon reads the engine, never a label" — no regex-on-label path remains.
      (c) LeanZero palette: ZONE_HUES and FORMATION_RAMP already carry #1d4ed8 / #0891b2 / #7c3aed /
      #d97706 / #dc2626 / #db2777. Was: — LeanZero palette pass on the swarm panel; known-active-bugs panel; phase
      chips read the engine `phase` event.
- [x] **P. OpenRouter cloud entrants — WORKING, no rebuild needed.** goose ships an `openrouter` provider
      and the SEALED grok46-era binary (`af7bf73c…`, 2026-08-25) already speaks it — proven by RUNNING it,
      not by `strings` (which found nothing and was the wrong probe). So a model too new for the sealed
      binary routes through OpenRouter instead of forcing a new binary, and every board row stays
      comparable. Key `OPENROUTER_API_KEY` in the harness secrets file (0600, no git repo), $15 balance.
      Coordinator taught the provider by mirroring the `xai_api` branch in four places; full procedure and
      bindings in the `goose-swarm-campaign` skill §4e.
- [x] **AA. DONE — the ETA token was still becoming the worker's DIRECTION, and my first fix missed it.**
      Caught live at 16:41/16:42Z: two nudges delivered as re-streams whose `next` read, in full,
      `ETA=5m` and `ETA=10m`. A direction that says only how long something will take is not a direction.
      ROOT CAUSE, second order: the stripper I added this morning used `find("ETA")` — the FIRST match —
      so any earlier word containing those three letters (metadata, details, theta, beta, retain) failed
      the `:`/`=` guard, the line was kept whole, and the real token at the end survived as a free segment.
      Reproduced deterministically before fixing. Now scans EVERY occurrence, byte-compared against the
      ORIGINAL string rather than an uppercased copy (`to_uppercase` can change length, so an index from
      it cannot safely slice the original — a latent unsoundness in the old code). Four shielding words
      pinned by a test.
      LESSON WORTH KEEPING: a fix shipped is not a fix proven. This one had a comment explaining the
      failure it prevented, and still failed within hours on a case the comment did not consider.
- [x] **AB. DONE AND MEASURED LIVE 2026-08-28 23:44 EEST — THE JUDGE WAS RE-STREAMING CALLS THAT ARE MERELY BETWEEN BURSTS — measured 2026-08-28, and it
      is costing this run its REVIEW phase.** These models do not stream evenly: they emit in ~2000/4000
      character BURSTS with quiet gaps between. The judge looks during a gap, sees
      `produced_since_last_look` of 1..9, calls LOOPING, and re-streams — discarding everything the call
      had built.
      THE EVIDENCE (review lanes, `thinking_total`):
        review-3: 12013 -> produced 4004 -> 12018 -> produced 5 (STALL, re-streamed) -> 16021 -> produced
                  4003. It was NOT dead; it resumed and produced another 4,000 immediately after.
        Resets paid for those false stalls: review-3 **27,297 -> 2,004**, review-2 **13,291 -> 2,002**,
        review-1 **8,640 -> 2,006**. Ten nudges, every one a re-stream, REVIEW 30 min and round 2 not
        landed.
      NOT COVERED BY `f3cfbdbbd` (production veto): that only stops tail-similarity arming the streak
      while a call IS producing. Here the call genuinely reads as not-producing at that instant, because
      the look landed in the gap.
      THE FIX SHAPE, and it must not be a constant: the burst gap is a property OF THE CALL, so measure
      it. Treat produced≈0 as a stall only when the silence exceeds that call's own longest observed gap
      between bursts. Self-calibrating, no literal seconds anywhere, and a genuinely dead socket still
      trips it because its silence grows without bound.
      Confirms the earlier note "OPEN LANES STALL NEAR ROUND NUMBERS ... looks like a provider-side buffer
      or chunk edge" — it is, and the round numbers (4001/4002/4003/4009) are the giveaway.
      **PROOF, from the live run's own log** (`swarm-3node-r0/run.jsonl`, 52 looks):
        `judge_nudge` delivery = **{'steer': 5}** — five nudges, FIVE steers, **zero re-streams**.
        Before the fix the same shape measured **12 nudges, 12 re-streams, 0 steers**.
        `judge_quiet_within_rhythm` fired **twice**, and each firing is a re-stream that did not happen:
          20:28:33 open-coverage-1            quiet=93s <= known_gap=338s
          20:32:31 slice-frontend-structure-s quiet=90s <= known_gap=144s
        Both silences would have read as LOOPING under the old rule. The gap high-water mark is per-call
        and self-calibrating, so there is no literal seconds constant anywhere in it, and a genuinely dead
        socket still trips because its silence grows without bound while the mark stops rising.
        `judge_skipped` = **0** (BUILD previously ran 45% unsupervised).
      **DETECTOR SHIPPED IN THE SAME PASS:** `tick.py` now prints `RE-STREAMS PREVENTED (burst-gap): N`
      with the last four firings, and prints `0  <-- calibration never engaged, check it` when a run has
      nudges but no firings. A fix with no counter is a fix that silently regresses.
      **TRAP:** the event key is `task_id`, not `task`. My ad-hoc reader used `task` and printed `?` for
      every lane — the instrument lied, not the engine. Read the emission site before believing a null.

- [x] **AC. DONE — THE QUEUED MESSAGE NOW LANDS MID-GENERATION, and steer is the default delivery again.**
      Mihai, twice: *"the queued message is a must"*, and *"is the judge not offering partial incremental
      nudges? instead is it just restarting? this should not be the case."* He was exactly right.
      MEASURED: **12 nudges, 12 re-streams, 0 steers**, because **128 of 134 looks saw
      `actions_since_last_look = 0`** — planning calls (OPEN/coverage/REVIEW) are pure reasoning, never
      reach a turn boundary, so `can_steer` was always false. Re-stream DROPS THE SOCKET and discards the
      partial: review-3 **27,297 -> 2,004**, review-2 13,291 -> 2,002, review-1 8,640 -> 2,006.
      TWO HALVES, both landed:
      (1) `Agent::steer` now notifies the in-flight stream, which stops at its next chunk boundary and
      KEEPS the partial (the cancelled path already falls through normal persistence). Guarded on
      `saw_tool_request_in_turn` so it can never orphan a tool request — and a tool call IS a boundary, so
      waiting there costs nothing.
      (2) The swarm's `can_steer` drops `acted_since_last_look` and keeps only `pending.is_empty()`.
      THIS ALSO REPAIRS THE DRIFTING PATH honestly rather than gating it: DRIFTING acts on the FIRST look
      with no corroboration, justified in the code as costing "one in-session message rather than a dead
      worker" — true for a steer, FALSE for a re-stream. Caught live re-streaming a call producing 4,001
      chars. Making steer the default delivery makes that justification true again.
      NOTE: two core tests assert the OLD behaviour by name and must be REWRITTEN, not silenced —
      `test_steer_does_not_interrupt_in_flight_generation`,
      `test_steer_never_lands_on_a_nonterminating_generation`. This supersedes item Y, which I had
      declined on scope; Mihai made the call.
- [ ] **Q. qwen3.8-flash — DIRECT TOKEN PLAN, manifest PREFLIGHT-PASSED, BLOCKED ON A CLOUD BINARY THAT
      MUST BE BUILT FROM `codex/salvage-benchmarks`, NOT `local-edition`.**
      MEASURED 2026-08-28: the fresh `local-edition` release binary is REJECTED by the harness —
      "goose binary lacks required cloud safety capabilities: GOOSE_BENCH_BUDGET_CONFIG,
      GOOSE_BENCH_BUDGET_LEDGER, GOOSE_BENCH_EXPECTED_PROVIDER, GOOSE_PROVIDER_LIFECYCLE_FILE,
      GOOSE_TOOL_SANDBOX_ROOT, ..." (11 in total). Those rails are the budget guard, the provider
      lifecycle ledger and the tool sandbox, and they exist ONLY on `codex/salvage-benchmarks`
      (`benchmark_budget.rs`, `provider_lifecycle.rs`, `developer/sandbox.rs`, `developer/shell.rs`).
      So a cloud binary is `codex/salvage-benchmarks` PLUS the Alibaba Token Plan change from `93cdb1d14`
      — verified to apply: that branch has the same `sanitize_request_for_compat` compat table and the same
      `alibaba.json`. The Token Plan contract itself is in NO branch here; the sealed binary was built from
      source that is not in this repo, which is why it had to be written from scratch.
      **THE BUILD IS DEFERRED WHILE A LOCAL RUN IS LIVE** — every machine that could compile it is also a
      fleet node (mihai = local, workhorse = Mac Studio, gabee = Mac.lan), and a compile starved the fleet
      badly enough once to take judge probe latency from 16s to 117s. Degrading the primary run to build a
      cloud binary is the wrong trade. Build it the moment the local run ends.
      **STAGED AND READY TO COMPILE (2026-08-28 15:2xZ):** worktree `~/goose-builds/cloudbin-wt` on
      `codex/salvage-benchmarks` with `93cdb1d14` cherry-picked. Two conflicts, both branch drift, both
      resolved: that branch's `sanitize_request_for_compat` takes an extra `&ModelConfig`, and its tests
      go through a `sanitize(&provider, payload)` helper. `alibaba.json` applied clean. No conflict
      markers, braces balance. **NOT COMPILED — that is the verification step and it waits for the fleet.**
      When the local run ends: `cd ~/goose-builds/cloudbin-wt && cargo build --release && cargo test -p
      goose-providers --lib sanitize`, then preflight/init/smoke/start the ali-stage campaign against
      `~/goose-builds/cloudbin-wt/target/release/goose`.
      Everything else is READY: manifest at `~/goose-builds/ali-stage`, vendor port 9142, $60 shadow cap,
      flash on the authenticated Token Plan roster, `sk-sp-` credential reading from the keychain, harness
      identity gate widened to `{qwen3.8-max, qwen3.8-flash}`, website registry on the bare id, and
      **preflight passes against the sealed binary**. OpenRouter is a dead end for this model and always will be: ONE endpoint (Alibaba),
      served from OpenRouter's SHARED key pool (`is_byok: false`,
      `limit_source: upstream_provider_shared_pool`). It 429'd inside the 3-turn smoke, then the episode
      died at 123s / 5 requests / 3 files. Archived `-KILLED-openrouter-shared-pool-429`, $0.0198 spent.
      THE REAL FIX IS THE DIRECT ROUTE, and it is now unblocked: `93cdb1d14` teaches the alibaba provider
      the Token Plan contract (`max_completion_tokens`, no `max_tokens`, `enable_thinking`,
      `preserve_thinking`) with tests pinning all four; `qwen3.8-flash` IS on the authenticated Token Plan
      roster (12 models) and the `sk-sp-` credential reads from the keychain. Staged at
      `~/goose-builds/ali-stage`, vendor port 9142, cap $60 shadow guard. The harness pinned the Alibaba
      identity to the literal `qwen3.8-max`; widened to `{qwen3.8-max, qwen3.8-flash}` with every other
      clause (roster id, exact endpoint, 1M context, 131072 out, thinking max) untouched. Website registry
      is back to the bare `qwen3.8-flash` for the direct route. **Preflight PASSES.** Launch once the
      release build lands. Old note follows:
      SUPERSEDED — **Q-old. via OpenRouter — BLOCKED ON CAPACITY, not on us.** It has exactly ONE endpoint
      (Alibaba) so no fallback is possible, and it is served from OpenRouter's SHARED key pool
      (`is_byok: false`, `limit_source: upstream_provider_shared_pool`). It 429'd inside the 3-turn
      contract smoke, then the real episode died at 123s / 5 requests / 3 files with
      `Rate limit exceeded: Provider returned error`. Archived
      `-KILLED-openrouter-shared-pool-429`; $0.0198 spent. `terminal_safe_retry_limit: 0` makes one
      upstream 429 terminal, so retrying into the same saturated pool is not a fix.
      UNBLOCK: add an Alibaba key as BYOK at https://openrouter.ai/settings/integrations — that gives
      dedicated rate limits AND keeps OpenRouter's request shape, so it also sidesteps the Token Plan
      contract problem that blocked the direct Alibaba route.
- **VINDICATED 16:24Z — 27b UNFROZE ON ITS OWN.** Δcalls+3, Δfiles+1, an 8,385-byte `app/common.py`,
  20 minutes after I recommended stopping it. A very long single generation on a 27B is a normal prelude
  to a large first write, not a hang. THE FLAGS ARE A REASON TO LOOK, NEVER A REASON TO ACT.
- **STANDING DECISION 2026-08-28 ~16:20Z — 27b r2 RUNS TO COMPLETION. Do not stop it.** Mihai: *"let it
  continue please"*, after being shown the evidence that it is 42 min in with an empty tree, 10 tool calls
  frozen >10 min, and the application being composed inside its reasoning channel. The tick will keep
  printing REASONING BUT NOTHING LANDING and ACTIONS FROZEN — **those flags are now EXPECTED on this
  entrant and are NOT grounds to act.** A later tick must not stop it on its own initiative; report only.
  It is uncapped and costing ~$0.19, so time is the only thing at stake.
- [x] **R. DONE — qwen3.8-27b via OpenRouter PUBLISHED at 20.06%.** Was LIVE from 14:3xZ**, `cloud-sb7-orqwen27b-20260828-r1`, cap $8,
      projected ~$6. 11 endpoints / 8 healthy, so it fails over cleanly — the reason it runs where flash
      cannot. Their context limits differ (65,500 → 1,000,000), so the manifest declares the FLOOR of the
      healthy set (262144 / 65536); declaring 1M would overflow a route to a small provider.
      TRAP HIT AND FIXED: the contract smoke compared the final marker with strict equality, and 27b
      reliably prefixes `\n\n`. It emitted the marker byte-for-byte otherwise, so the comparison now
      ignores surrounding whitespace only. Smoke is a pre-flight gate and feeds no published score.
      First root archived `-KILLED-smoke-marker-whitespace`.
- [x] **S. DONE — deepseek-v4-flash-vision-exp PUBLISHED at 67.53%, the board leader.** 33 min, 13 files, exactly 4 frontend files — the allocation the local fleet has never managed. Score follows DECOMPOSITION, not model size.
      Mihai toggled the OpenRouter privacy settings and DeepSeek-the-provider came back into the roster.
      VERIFIED BY A REAL COMPLETION, not by the roster: `content='VISION_OK'`, reasoning present,
      finish=stop, provider=DeepSeek. (A first 16-token probe returned `content: None` because reasoning
      consumed the budget — a 200 with empty content is not a working model, so it was re-tested at 600.)
      Staged at `~/goose-builds/ds-stage`, vendor port 9143, cap $4, registered on the website as
      `deepseek-v4-flash-vision-exp` -> `brun-baseline-deepseek-v4-flash-vision-exp-sb70`.
      **RISK, same shape as flash: ONE endpoint (DeepSeek first-party), so no failover.** If it hits an
      upstream rate limit mid-episode, `terminal_safe_retry_limit: 0` makes it terminal. Fallback the user
      already authorised is `deepseek/deepseek-v4-flash` (17 providers).
      **LAUNCHES ITSELF: `~/goose-builds/loop-state/cloud_queue.sh` is watching r2 and will init+smoke+
      start `cloud-sb7-dsvision-20260828-r0` the moment 27b reaches a terminal status** (log:
      `~/goose-builds/loop-state/cloud_queue.log`). It waits for the watched manager PID to exit first, so
      two campaigns never share a provider lane. Manual equivalent if the watcher is gone:
      `cd ~/goose-builds/ds-stage/evals/swarm-bench/bench && python3 cloud_sb7.py
      init --root ~/goose-builds/cloud-sb7-dsvision-20260828-r0 --binary <sealed glm binary>
      --publisher-repo ~/Projects/LeanZero-website --publish-live` then `smoke`.
      SUPERSEDED — **S-old. was BLOCKED BY AN ACCOUNT SETTING.**
      One endpoint, and the account rejects it: "No endpoints available matching your guardrail
      restrictions and data policy." A per-request `provider.data_collection: "allow"` does NOT override
      it. Needs a toggle at https://openrouter.ai/settings/privacy. `deepseek/deepseek-v4-flash` works
      today and is the fallback he authorised ("or a version that works for this").
- [x] **T. DONE — a node no longer renders as WORKING on an engine claim alone.** Mihai saw gabee showing
      "Review 1 · working" with a live nudge quoted under it while `lms ps` reported all three nodes IDLE.
      Cause: `deriveFleet` populated `workingByDevice` from `laneSources` where `status === 'running'`
      with NO corroboration of any kind, so a lane the engine opened and never closed — re-streamed 13
      times, its stream gone — stayed working for as long as the panel was open. The digest path beside it
      already had freshness guards; the lane path had none. Now the claim is demoted only when BOTH
      independent signals disagree: LM Studio is reporting fleet state at all AND does not list the node,
      AND the digest is stale past the open-call window. No timer was added — each signal alone has
      wrongly demoted a working node before (mtime mid-shell-call; `busyNodes` is empty for a cloud
      device, which never appears in `lms ps`). 5 tests pin every branch including both keep-it cases.
- [x] **U. DONE — the event log now shows at a glance what ACTED and what only WATCHED.** Mihai: *"the
      event log is not clear right off the bat with just observations or nudges? It's way too thick right
      now and it's not clear what is what?"* Three separate causes, all fixed:
      (1) **Every action was a verbatim duplicate of the observation above it.** The engine emits
      `judge_look` then `judge_nudge` in the same breath carrying identical `established`/`next`, and the
      panel rendered both — so half the wall was literal repetition and the 3 rows that changed the run
      were lost among the 15 that did not. A nudge matching the look before it now REPLACES it.
      (2) **Nothing was ever actually collapsed.** The row used `wrap ? 'break-words' : 'truncate'`, and
      the event log passes `wrap`, so every "collapsed" sub grew to full height — the chevron was there
      but had nothing to reveal. Collapsed subs now clamp to 2 lines.
      (3) **Both used the same icon and register.** Observation is now a recessive `Eye` on the muted
      formation hue; an action is a solid saturated `Gavel` in the action colour with a 600-weight label.
      No left rail, no faded tint. 4 tests pin the fold, including both must-NOT-fold cases (different
      direction, different task).
- [x] **V. DONE — FLEET is delineated node cards, and a click opens the node's FULL stream in a modal.**
      Mihai: *"it's sort of useless what it shows now… just throwing in some floating text… here it usually
      gets truncated… I would want to see the thinking tags correctly exposed and then what it shoots
      out"*, then a screenshot of a generation cut mid-word at `39. three-role authentication (maker/check`.
      (1) Each node is now its own bordered card instead of text floating under a name.
      (2) Clicking a node (or Enter/Space — it is a real button with an aria-label) opens `NodeInspector`,
      an inset-8 modal, Escape to close, backdrop to dismiss.
      (3) THINKING and OUTPUT are SEPARATE panes, because they answer different questions and are separate
      in the protocol: thinking is the reasoning channel, output is the tool calls and text it actually
      emitted. A node reasoning hard while emitting nothing is the exact state that has cost whole runs and
      it is invisible once the two are concatenated. Each pane carries its own count (chars / tool calls).
      (4) Both panes reuse `NodeExpandBox`, so each follows the newest text like a terminal while a
      scroll-up to read stays put. The clipped inline box it replaced is deleted, not left dangling.
      **STILL OPEN — the 24,000-char TAIL clip.** `build_full_reasoning` (swarm.rs:13926) keeps only the
      last 24k chars, which is why the screenshot starts at item 25 rather than item 1. The clip exists
      because the digest is REWRITTEN on a hot timer, so it cannot simply grow. The right fix is an
      append-only per-task transcript the modal reads instead of the digest — not a bigger number.
- [x] **W. DONE — Mihai chose (C): COVERAGE MOVED OFF THE CRITICAL PATH.** It now runs CONCURRENTLY with
      ASK and RESEARCH instead of ahead of them. The fleet researches the slices the opener already found
      while coverage keeps reading for the ones it missed; the task is joined just before SYNTHESIS (which
      cannot wire a slice with no brief) and any late slice is researched then, at the SAME depth with the
      SAME tools — a component found late is still one the request named, and a thin brief would give back
      exactly what coverage was built to win. Only the WAIT moved; none of the work was dropped. Still a
      patch loop ending the first time it adds nothing, still no round ceiling. New event
      `coverage_late_slices` so the next run can be measured rather than assumed. Was:
      *"how did we end up from my idea of opener with one model doing something to an opener that lasts
      over 40 min? … at some point we were having everything up to integrate in probably an hour max."*
      THE HONEST ANSWER, from the commits:
      - The DESIGNED opener (plan §3) is ONE call plus at most one rebalance patch. That is still what
        `open` and `open-resplit` are.
      - `59d999d2d` added COVERAGE after a run scored **0.0023**: the opener read a 54,146-char spec naming
        ~11 components and produced nine slices for "a table with pagination and a filter". Webhooks,
        notifierd, the outbox, the event ledger, the approval workflow and the 3D field were never planned.
        **Seven hours of repair could not reach it — repair fixes what was BUILT, never what was never
        PLANNED.** Most scorer checks came back UNAVAILABLE, not failed.
      - `4e5cec44d` fanned coverage across 3 nodes because one 27B call could not hold 54KB. Reading depth
        rose (1 gap -> 5); named components stayed 2/11.
      - `4411fff13` made the component->owner TABLE the output with a quoted proof of ownership. THIS is
        the one that worked: 139 components, 0 unowned, 12 slices naming the request's own components.
      SO EACH STEP ANSWERED A MEASURED FAILURE — but I optimised for the CORRECTNESS of the decomposition
      and never re-measured what it did to the PHASE BUDGET. That is the real mistake and it is mine.
      VALUE, measured on the live run: 79 components, and round 2 closed the 3 gaps round 1 left. So the
      extra rounds buy ~3 components out of 79.
      COST: OPEN at 50+ min. A large share of it was the judge re-streaming producing coverage calls —
      4 in one phase, each discarding a table already built. `f3cfbdbbd` fixes that and is NOT yet measured.
      OPTIONS PUT TO HIM: (A) keep, and measure OPEN again with the judge fix in; (B) one coverage round
      only; (C) run coverage CONCURRENTLY with RESEARCH so it leaves the critical path — research starts
      on known slices immediately, late-found slices research a few minutes behind.
- [x] **X-2026-08-29. CLOUD QUEUE — four models dropped, and TWO of the four were MY bugs.**
      `seed-2-1-turbo` exploring not building (509 shell calls, $12.29) · `seed-2.0-code` stream ends with
      no terminal finish reason so the harness refuses a possibly-truncated tool call — a safety rule
      working correctly, $0.22 · `ling-3.0-flash` rate-limited, and when it ran its final text was not the
      exact contract token · `longcat-2.0` **NOT a model failure**: OpenRouter advertises
      `max_completion_tokens=262144`, the serving provider 400s on it, 131072 proven fine by direct probe.
      Pinned in `cloud-queue-models.json` and requeued.
      TWO CHAIN BUGS FOUND AND FIXED (`loop-state` b22db28): `root exists` was read as "model is done", so
      ling was dropped from the queue on every pass with $0 spent and nothing logged; and `wait_terminal`
      polls campaign status, which never leaves INITIALIZED when smoke fails — the chain was about to hang
      on ling with longcat and laguna blocked behind it forever.
      REMAINING: longcat-2.0 (pinned), laguna-s-2.1. Budget $28.76.

- [ ] **X. CLOUD QUEUE — 7 campaigns chained, unattended. `~/goose-builds/loop-state/cloud_chain.py`,
      pid in `cloud_chain.pid`, log `cloud_chain.log`.** Order: deepseek-vision → hy4-preview →
      seed-2-1-turbo → seed-2.0-code → ling-3.0-flash → longcat-2.0 → laguna-s-2.1. All seven are
      REGISTERED on the website (22 entrants, model-id regex passes) and each manifest is generated from
      `cloud-queue-models.json` with caps at 500 so nothing can bind.
      **THE ONE GATE IS ON LAUNCH, NEVER ON A RUNNING EPISODE:** a campaign is not started unless the
      remaining balance covers its projection. Since caps were removed, running out of credit mid-episode
      is now how a run dies at 90%, so the money is checked BEFORE the work starts, where refusing costs
      nothing. Unreadable balance is NOT treated as zero.
      PROJECTIONS from glm's measured 12.85M in / 0.22M out (which reproduced $1.02 exactly, so these are
      FLOORS): hy4 $11.22 · seed-2-1-turbo $6.97 · seed-2.0-code $7.08 · longcat $4.12 · laguna $1.20 ·
      ling $0.27 — **~$31 for the six**, plus deepseek ~$3. Mihai is topping up OpenRouter.
      **RISK: 5 of the 6 have exactly ONE endpoint** (only ling-3.0-flash has 2), which is the qwen-flash
      shape — no failover, and one upstream 429 is terminal at zero retries. Expect at least one re-run.
      All six support tool calling and carry 262k-1M context, so none is disqualified on capability.
- [ ] **Y. DECIDED: NOT IMPLEMENTING. Diagnosis stands; the change does not belong to me.**
      This is not a confidence problem — I am confident in both the diagnosis and the fix shape. It is a
      SCOPE problem. `Agent::steer` is `crates/goose/src/agents/agent.rs`, so changing when a queued
      message lands changes behaviour for EVERY goose session, not the swarm. The current behaviour is
      PINNED BY TWO TESTS that assert it deliberately
      (`test_steer_does_not_interrupt_in_flight_generation`,
      `test_steer_never_lands_on_a_nonterminating_generation`), so implementing it means deleting tests
      that exist to say "this is intended" — that is a product decision about how goose behaves for all
      users, and Mihai should make it, not me inside a swarm campaign. The swarm judge already has the
      two-mode delivery and is unaffected. Original finding follows.
      ORIGINAL: **QUEUED MESSAGES CANNOT INTERRUPT A GENERATION — Mihai's concern, CONFIRMED IN CODE, and it is
      a CORE AGENT issue, not a swarm one.** *"I am not sure how well queued messages work in goose right
      now but it's a must for them to work well as an agent."* He is right.
      THE MECHANISM: `Agent::steer` (agent.rs:506) ONLY enqueues onto `pending_steers`. There is ONE
      production drain (agent.rs:1955) and it is gated on `can_drain_pending_steers`, which is set at
      exactly one place — **agent.rs:2616, AFTER the provider stream for the round-trip has closed.** So a
      queued message lands at the next TURN BOUNDARY and nowhere else. There is no cancel path on the user
      side at all: `steer()` pushes and returns.
      THE BEHAVIOUR IS PINNED AS INTENDED by two tests whose names state the failure outright:
      `test_steer_does_not_interrupt_in_flight_generation` and
      **`test_steer_never_lands_on_a_nonterminating_generation`**.
      WHY IT MATTERS BEYOND THEORY: qwen3.8-27b spent 40+ minutes inside ONE generation (25,000 thinking
      chunks, one outstanding request). A user message typed into that session would have sat unread for
      the whole 40 minutes. On a local 27B this is the normal case, not an edge case.
      THE JUDGE ALREADY SOLVED THIS and its solution is the proposal: two-mode delivery — steer when the
      call has ACTED since the last look (a turn boundary is coming, so it costs nothing), cancel-and-reply
      when it has not (drop the socket; the partial is persisted through the normal path). The swarm judge
      does exactly this today and it works.
      CONFIDENCE: HIGH on the diagnosis (read from code, corroborated by the test names). MEDIUM on a safe
      fix, and the blast radius is the honest reason to pause — this is `crates/goose/src/agents/agent.rs`,
      every goose session, not just swarm. Cancelling mid-tool-call also aborts in-flight tool futures;
      `fix_messages` repairs the pairing but whether every provider accepts the repaired history is not
      determinable from code. NOT implemented unilaterally — Mihai's call.
- [ ] **Z. DECIDED: NOT IMPLEMENTING the `live_fleet_slots` node RE-ADD.** A node absent at boot that
      returns mid-run stays idle through RESEARCH/REVIEW/TEST/FIX. Real, but: it needs the CONFIGURED
      device list, and `live_fleet_slots(devices)` is fed a function PARAMETER — the full config is not in
      scope at either call site (swarm.rs:27161, :38351), so the fix is threading a second list through
      the planning signature and its caller. Against that: the scenario needs a node to be down at boot
      AND to come back mid-run, and the probe-failure fallback would then have to distinguish "config" from
      "boot pool" or a failed `lms` probe would dispatch to a permanently dead node — WORSE than the bug.
      Departure is already handled (`is_cloud`-exempt residency filter). Revisit only if a node actually
      rejoins mid-run and is measured sitting idle.
- [ ] **D. Dead-code sweep — DEFERRED UNTIL NO LOCAL RUN IS LIVE.** `cargo clippy` starves the fleet:
      with the sweep running, judge probe latency went 16s -> 18s -> 27s -> **117s**, clippy-driver at
      55%+30% CPU. I was degrading my own run to tidy code. Ordering also matters and cost one aborted
      pass: delete FUNCTIONS to fixpoint, THEN structs/enums, THEN consts last — a const deleted while a
      dead function still references it breaks the build. Script: scratchpad/sweep3.sh (builds after each
      pass, `git checkout` on failure, so a broken compile can never read as "no warnings left").
      Original: — 79 dead-code warnings exist at HEAD in `swarm.rs` (old planning path:
      SCOUT_LENSES, plan_agreement, consensus_backbone, fan_verify_split, …). The clippy gate has been
      passing on STALE per-crate cache. Delete bottom-up: methods/fns first to fixpoint, then structs.
      A broken compile must never read as "no warnings".
- [x] **E. Coverage — DONE (4411fff13).** Enumerate-then-prove: the component->owner table IS the output, an owner must quote the slice's own words, `coverage_enumerated` logs the table. Old text: Fanned coverage got 10 -> 13 slices but named components stayed
      2/11. Next fix: each shard must ENUMERATE its portion's components first, then match against the
      slice list — two steps in one call. Generic slice names (`api-backend`) absorb everything.
- [x] **F. DONE 12:11.** Run `swarm-3node-r0` live on the FRESH engine (build_sha 79e0f6d41).
      TRAP HIT AND CLEARED: the first launch ran `/Applications/Goose.app` from **Jul 28** — a month-old
      engine, proven by three missing strings with a positive control. Always verify the INSTALLED binary,
      not the built one. `just make-ui` writes to ui/desktop/out; /Applications must be replaced separately.
- [x] **M. DONE.** `benchmark` now emitted in `levers_resolved`. Was: It is read from config.yaml (SwarmConfig.benchmark) but is
      absent from the `levers_resolved` event, so the log cannot show whether a run was unattended. Add it.
      Functional proof meanwhile: `clarify_proxy_armed {mode:"immediate", wait_secs:0}` at ASK.
- [x] **G. DONE.** `review_plan_fanned` — one portion per lane against the whole plan, patches union with per-id dedupe, lanes<=1 falls through to the old path. Needs the next build to take effect. Was: It reads the 54KB spec + the whole plan in ONE call — the same
      volume ceiling that made single-call coverage useless. Same fix shape as `cover_slices_fanned`.
- [x] **H. DONE (612cbd4cb).** Seen twice: the nudge was literally "Check the
      slice list against the request section by section" — the job restated, costing a restream. The judge
      must add information or return OK.
- [x] **I. DONE.** The rater can answer `DUPLICATE <n>`; the engine drops those from the wave. Model decides — no similarity score, no threshold. Backward-only, and `forced` engine checks still win. Was: (`merge_duplicate_findings`) — designed, never shipped.
- [x] **J. DONE.** `DeviceCfg.is_cloud` (54a06a882) then `live_fleet_slots` — fans drop devices LM Studio no longer serves, and never residency-check a cloud device. Falls back to the boot snapshot on any doubt. KNOWN LIMIT: drops the departed, does not re-add a node absent at boot (needs cfg, not the pool). Was: Confirmed real: `worker_models`
      is `fleet_slot_models(devices)` computed ONCE at swarm.rs:26951 (planning) and again at :38053
      (repair), both from the boot pool. Only BUILD sees a rejoin, via the scheduler's `DeviceAdmission`.
      So a machine that returns mid-run sits idle through RESEARCH/REVIEW/TEST/FIX however long they take.
      THE OBVIOUS FIX IS WRONG: filtering the slot list by LM Studio residency drops every CLOUD device,
      because a cloud model never appears in `lms ps` — and cloud nodes are now a supported fleet.
      `DeviceCfg` has no provider field to tell them apart (SwarmDevice does; it is not threaded through).
      PREREQUISITE: put the provider on DeviceCfg, then residency-filter LOCAL devices only. Confidence in
      the diagnosis HIGH, in a safe fix without that prerequisite LOW. Was: RESEARCH/TEST/FIX hold pre-BUILD fleet snapshots, so a
      node that comes back mid-run (gabee) sits idle for those phases.
- [x] **K. DONE.** Judge's `eta_mins` now surfaces per running lane (latest wins). The run-level band stays arithmetic and is honest about being an extrapolation. Was: — the judge estimates remaining time; surface it in the panel.
- [x] **N. DONE.** `cut_request_into_portions` cuts by character count; deliberately NOT named 'weight' (four other meanings exist). Was: Measured: part 1 got 72
      components, part 3 got 9; one lane ran 25+ min while two sat idle. Cut on section headings, or
      rebalance by character weight — the same "no slice more than ~2x another" rule OPEN applies to slices.
- [x] **L. DONE (as far as is honest).** Rows now say "built — the app has not been run yet; verified end-to-end after Repair". NO earlier trigger exists: the `smoke` gate is superseded by GOOSE_SWARM_COMPLETE and has never fired on any run; the panel also drops its findings list so it could not tell pass from fail. Promotion stays complete_result.passed && verified — engine truth, not a model claim. The board has never flipped green because no run has finished, not because the rule is wrong. Was: There must be a real transition to verified when the engine
      has evidence; today no event ever promotes it.

## Evidence worth acting on

- **WHY: `qwen3.8-flash` IS TWO DAYS OLD (created 2026-08-26) AND THE SEALED BINARY IS FROM 2026-08-25.**
  The engine cannot know a model that did not exist when it was compiled, so it falls back to the generic
  OpenAI shape. Live Token Plan roster, 12 models, newest first: qwen3.8-flash (08-26),
  qwen-audio-3.0-realtime-plus (08-05), qwen3.8-max (08-03), deepseek-v4-flash-0731 (07-31),
  qwen-audio-3.0-tts-plus (07-28), glm-5.2 (06-16), qwen3.7-plus (06-07), qwen3.7-max (05-21),
  wan2.7-image-pro, wan2.7-image, qwen3.6-flash, deepseek-v4-pro (05-19).
  **`qwen3.8-27b` IS NOT ON THIS ROSTER AT ALL** — there is no 27B under the Token Plan. And `qwen-flash`,
  which alibaba.json does list, is not served either: that definition is stale against the live plan.
- **QWEN3.8-FLASH IS BLOCKED ON THE ENGINE, not on credentials or the guard.** Preflight passes:
  the roster PROVES `qwen3.8-flash` exists under the Token Plan, the keychain credential reads without a
  prompt, ports are free, the website entrants are registered. The SMOKE fails:
  `Qwen request 0 violates Chat contract: max_completion_tokens, enable_thinking, preserve_thinking,
  unsupported:max_tokens`. The Token Plan needs `max_completion_tokens` equal to the declared cap,
  `enable_thinking: true`, `preserve_thinking`, and NO `max_tokens`.
  The sealed cloud binary CONTAINS all three field names and the literal `qwen3.8-max`, so it speaks that
  contract — for that ONE model. `crates/.../definitions/alibaba.json` lists qwen3.7-max, qwen3.6-*,
  qwen3-max, qwen-plus, qwen-turbo, qwen-flash and NO qwen3.8-* at all, and **no ref in this repo declares
  `qwen3.8-max` anywhere under crates/** (verified with a positive control). The source that taught the
  engine that model is not in any branch here — it exists only inside the sealed binary.
  SO: teaching flash/27b means writing the token-plan contract into the alibaba provider and building a
  NEW cloud binary, which also raises a comparability question for the board (other entrants ran the
  grok46-sealed binary). Nothing was spent: $0.0020 of a $60 guard, and the campaign is stopped.

- **THE CLARIFY-PROXY RACE FIX IS CONFIRMED IN PRODUCTION (13:23:40-13:25:20Z).**
  ask 13:23:40 -> proxy armed + question asked 13:23:40 -> answered 13:25:19 -> low_confidence_answered
  13:25:20 -> research 13:25:20. The run WAITED 99 SECONDS for the proxy and NO
  `low_confidence_ask_timeout` was emitted, so the answers reached the plan.
  Compare the defect: reader gave up at 5s, proxy answered 42s LATER, plan built from guesses while the
  log said a node had answered. The exit condition is the proxy TASK now, not a clock, and it holds.

- **BEST OPEN YET (13:23:40Z, 1982s).** 139 components enumerated across parts of 25/57/57 with **0
  unowned in every part**, and 12 slices that are the request's own components: ledgerd-core, vendor-sync,
  event-ledger-outbox, webhook-handling, approval-workflow, notifierd-service, ledgerd-api,
  frontend-structure-style, frontend-behavior, decisions-readme, frontend-3d-rendering,
  frontend-3d-interaction. No coverage_gap was needed — OPEN got it right first time.
  - The BALANCED SPLITTER is confirmed: 25/57/57 against the previous run's 72/9.
  - Judge health: 2 nudges, 3 abandoned probes for the whole phase, against 213 abandoned on ONE lane
    two runs ago.
  - Cost: OPEN 33 min against 20 min, for 139 enumerated components against 49. Worth it.

- **LENS 3, measured this run — one fix works, one did not.** Abandoned judge probes: 213 on one lane
  before, 1 worst-lane / 2 total now — the storm fix WORKS. But coverage lanes were still judged `looping`
  at produced_since_last_look 4000 and 4001, because the recurrence sentence asserted "a healthy advancing
  call measures under 5%" — a prose threshold applied to a TABLE, which measures high by construction. A
  model choosing between a deterministic detector with a stated healthy range and a paragraph of
  reassurance believes the detector. Fixed by removing the verdict from the measurement and pointing the
  judge at whether the repeats ADVANCE.

- **RUN KILLED 12:40Z — `-KILLED-review-cannot-converge-1240` — and the reason is a fixed defect not yet
  in the binary.** REVIEW rounds 1 and 2 each reported the SAME defects in different words and both logged
  `repeated: 0`, because the cross-round de-dupe is a 120-char lowercase PREFIX comparison. The stop rule
  is "a round with no NEW finding", so a reviewer that rephrases can never satisfy it. Round 3 ran 19
  minutes. Verified with a positive control that none of the four completion-critical fixes were in the
  running binary: ALREADY REPORTED, DUPLICATE <n>, the fix worker's real done-condition, and the
  advertised surface. Letting it run would have re-proved the old REPAIR does not finish — which nine
  runs already established.
- **WHAT THAT RUN PROVED BEFORE IT DIED, and it is a lot:** 13 correct slices naming the request's own
  components; coverage 49 components / 0 unowned; all 13 briefs substantial (3,530-23,059 chars);
  SYNTHESIS produced a DAG; the fanned REVIEW ran on 3 nodes; judge probe accounting fully balanced. Every
  phase before REPAIR now works. The remaining unknown is BUILD onward.

- **RESEARCH BRIEFS ARE SUBSTANTIAL (11:58:09Z, 1564s).** brief_chars = [5073, 5808, 6995, 5971, 15292,
  8110, 5838, 7123, 5512, 11256, 3530, 23059, 7442] — all 13, min 3,530. The historical failure was a
  122-char brief scoring 42.7% where a 1,497-char spec scored 88.7%. The DETAIL-fan deletion is vindicated.
- **REPAIR'S FOUR DEFECTS, all pre-existing, all fixed in source (0a5d4e8c1 + the spec fix):** the fix
  worker's done-condition was already true before it started (verify_recipe cannot see TEST-fan, DOM, CSS,
  HTTP or spec-contract findings); criticals were computed then went OUT OF SCOPE before the wave; the
  benchmark proxy bought rounds forever; and the fix worker never saw the request. None needed a cap.

- **THE DECOMPOSITION PROBLEM IS SOLVED (11:29:59Z, first verified sb-7 run).** 13 slices in 1181s:
  boot-contract, vendor-sync, **event-ledger-outbox**, **webhook-handling**, **approval-workflow**,
  ledgerd-api, **notifierd-service**, frontend-html, frontend-css, frontend-app-js, **decisions-docs**,
  **viz-rendering-engine**, **viz-interaction**. Those are the REQUEST'S components, not program layers.
  Every previous run scored 2 of 11 named components; this is 9+. Coverage enumerated 49 across three
  parts with 0 unowned. This is what the enumerate-and-prove rewrite was for.
- **MY OWN DEFECT, found the same tick: 213 abandoned judge looks on one lane.** open-coverage-2
  dispatched 218 and abandoned 213, looks 2-214 over 838s. Once a stream ends mid-probe the loop drains
  its deferred events, and the judge trigger — which sits at the TOP of that loop — fired again on each
  one, dispatching a fresh probe against an ended stream. Correctness was fine (abandoning is the right
  behaviour) but it is 213 started-and-dropped model calls, scaling with the deferred backlog rather than
  with anything real. Fixed by teaching the trigger that an ended call is not worth looking at.

- **THE COVERAGE-OBJECTIVE FIX WORKS (measured 11:19-11:24Z).** BEFORE it, `looping` fired on coverage
  lanes at `produced_since_last_look=4003` — calls producing healthily, judged as looping because the
  table's rows repeat. AFTER it, looping fires only at `produced=1,2,3` — a genuine stall — and the
  re-stream RECOVERS the lane (coverage-1: think 2002 stalled -> restream -> 4002 produced -> 8005). The
  judge also carries `established` forward, so the replacement stream resumes rather than restarting
  blind. The judge is now right when it acts, which is the whole point.
- **OPEN LANES STALL NEAR ROUND NUMBERS — worth watching, not yet explained.** Stalls observed at
  think=2001, 2002, 4400, 4403; recoveries then run to exactly 4002/4003/8005. Repeated stalls landing on
  ~2k/~4k boundaries look like a provider-side buffer or chunk edge rather than model behaviour. If this
  recurs, splice the stream and compare against LM Studio's own chunking before blaming the engine.

- **VERIFIED sb-7 RUN LIVE from 11:10Z** — the first correct local run of the day. prompt 53,634 chars,
  "# Build `app` — Meridian Payments Console", all of ledgerd/notifierd/viz.js/DECISIONS.md/outbox/12,288
  present, BENCH_SPEC=spec-build-sb7.md, vendor 200, secrets store=file, benchmark=true.
  `bench_dispatch.mjs` now asserts these markers itself and exits 6 rather than reporting success.

- **THE BENCHMARK VIEW COULD NEVER RUN sb-7 (fixed 80ccfe548).** `benchmark-run` set `BENCH_SPEC` with no
  sb-7 branch, and `build_prompt` reads BENCH_SPEC BEFORE the regime — so selecting sb-7 ran the sb-5
  spec. The 10:44Z run received 6,278 chars of "# Build `vendorsync`" instead of 54,146 chars of Meridian,
  and looked healthy: 9 balanced slices, coverage 0 unowned, clean verdicts. ALWAYS verify the tier from
  `run_started.prompt` (54,146 chars, contains ledgerd/notifierd/viz.js/outbox/12,288), never from the flag.
  `GOOSE_SWARM_RENDER_PROBE` had the identical gap and would have graded Meridian by VendorSync's rules.

- **THE FIRST VALID LOCAL RUN of 2026-08-28 started 10:44Z** through the Benchmark view. All three
  conditions hold and were checked from the run's own events, not the UI: `run_build.py --sb7` running,
  its own vendor answering /v3/docs 200, `secrets_source {store:"file"}`, `levers_resolved
  {benchmark:true}`. Everything before this was a chat with a placeholder spec and no vendor.

- **THE CHAIN THAT KILLED RUN swarm-3node-r0 (archived -KILLED-restream-killed-coverage-lanes-1008).**
  Four links, and each is worth keeping straight:
  1. The new coverage deliverable IS a table — dozens of near-identical rows. The recurrence detector
     reads structural repetition, so it trips on exactly the output the task was redesigned to produce.
  2. The judge returns `drifting` (or `looping`). DRIFTING acts on the FIRST look with no corroboration —
     deliberately, because tail recurrence cannot corroborate "working on the wrong thing".
  3. Delivery is a re-stream, and it could not have been anything else: a coverage lane makes ZERO tool
     calls, so `actions_since_last_look` is 0, so `can_steer` is false. THIS IS A CONSEQUENCE OF MY OWN
     can_steer FIX and it is still the right rule — a pure-reasoning call has no turn boundary for a steer
     to land on until it finishes. For such calls, re-stream is the ONLY delivery.
  4. The re-stream dropped the socket and the new stream produced NOTHING. `open-coverage-1` sat with
     `thinking_chars: null` and a frozen digest for 9.6 minutes; two of three lanes idle, OPEN at 56 min.
  Fix landed for link 1 (fcef6947f: the judge is told repetition is the deliverable here). Links 2-4 are
  untouched and are the next thing to watch: if a healthy coverage lane still gets nudged, the objective
  fix was not enough.
- [x] **O. DONE (4e70ad192).** The judge's readiness floor now reads `thinking_total` (survives the re-stream reset) instead of `thinking_chars`, so a re-streamed lane that produces nothing can still be looked at. Detector `post_restream_silence` ships with it. Was: The lane dies silently — no event, no
      retry, and the fan waits for it forever. Either detect a re-stream that yields nothing and fall
      back to the pre-re-stream partial, or do not re-stream a call that is producing steadily.

- **THE JUDGE RE-STREAMS COVERAGE SHARDS ON A FALSE LOOP READ (fixed in source, needs next build).** The
  coverage table is repetitive by construction, the recurrence detector trips on it, and the re-stream
  delivery discards the partial table and restarts from the top. Three times in one run = 30-minute
  coverage rounds. Fixed by telling the judge what repetition means for that call.
- **Dispatch/return imbalance to re-check next run:** 2,140 `judge_look_dispatched` vs 63 `judge_look` on
  the 0828 run. Partly abandonment (call finished first) but the running binary predates
  `judge_look_abandoned`, so it cannot be attributed yet. CHECK THIS on the next run — if the gap is not
  fully explained by abandonments, there is a second defect.

- **COVERAGE IS ENUMERATING PROPERLY NOW (2026-08-28 run).** part 1/3 -> **72 components, 1 unowned**;
  part 3/3 -> 9 components, 0 unowned. Previous runs found 1-5 gaps IN TOTAL. The table is in the request's
  own vocabulary (`vs7dbg.pickPixel`, `#viz-labels`, `GET /api/stream`, `DECISIONS.md`) and owners are
  specific slices (`viz-interaction`, `viz-rendering`, `decisions-doc`) rather than layers.
- **BUT THE SPLIT IS BADLY UNBALANCED:** 72 components in part 1 against 9 in part 3, because the fan cuts
  by PARAGRAPH COUNT, not by content weight. One lane grinds while two idle. Same defect shape as the
  slice-balance rule OPEN already applies to itself. -> new item N.

- **THE WEDGE FIX IS CONFIRMED LIVE (2026-08-28 09:16-09:18Z).** `open` judge look 4 was dispatched at
  09:16:30 and never returned; the phase advanced to `open-resplit` at 09:18:01 regardless. The OPEN call
  finished while its probe was still out and the "abandon the look, not the result" path fired. Under the
  old serial `.await` that probe would have blocked the loop and OPEN could not have completed. Measured
  probe latencies this run: 16s, 18s, 27s, and one >117s — every one of those was previously time the
  supervised worker spent frozen.
- **The re-stream baseline reset is also confirmed:** `open-resplit` look 1 reported
  `produced_since_last_look: 2002` against `thinking_chars: 2002`. Before the fix this reported 0, which
  the rate block hands to the judge as "a DEAD STREAM ... Say LOOPING" — the symptom the engine invented.

- **GLM-5.3-flash (cloud) is producing what the local swarm never has**: `DECISIONS.md`, `notifierd.log`,
  `ledgerd.pid`, `notifier.db`, 95 files. The local coverage failure (named components stuck at 2/11) is
  therefore a DECOMPOSITION defect, not a model-capability ceiling. Compare its plan against ours.

## HOW TO LAUNCH — THROUGH THE BENCHMARK VIEW, NOT A CHAT

    ~/goose-builds/loop-state/stop_local_run.sh 9897    # MUST exit 0 — it gates on `lms ps`
    open -n /Applications/Goose.app --args --remote-debugging-port=9897
    node ~/goose-builds/loop-state/bench_dispatch.mjs 9897 sb-7 3

THE FIRST LINE USED TO READ `pkill -9 -f 'Goose.app/Contents/MacOS/Goose'` AND THAT IS THE BUG. It matches
the Electron binary only, so `Resources/bin/goose swarm run` survives, reparents to launchd, and keeps the
whole fleet GENERATING for a run whose window is gone — measured 2026-08-28, ~25 minutes across three nodes,
and Mihai had to point it out. Never launch over that: a second run against a fleet already serving a
zombie makes both numbers meaningless. `stop_local_run.sh` kills all three goose command lines plus the
harness and refuses to exit 0 while any node is still generating, so a failed stop cannot be mistaken for a
clean one.

`benchmark-run` spawns run_build.py --sb7 --timeout 0 with GOOSE_SWARM_BENCHMARK=1; run_build serves the
vendor, builds the fixtures, substitutes the spec placeholders and SCORES at the end. Verify with
`pgrep -fl run_build.py` (must carry --sb7) and `curl 127.0.0.1:8850/v3/docs` (200). Do NOT start a vendor
yourself — run_build owns the port. A benchmark run must NOT appear as a "# Build `app`" chat in the
sidebar; if it does, it is a chat, not a benchmark.

## SUPERSEDED — the chat path, kept only as a warning

`launch.sh` alone is WRONG and produced a full day of void runs: it types the raw spec into the desktop,
so the prompt keeps its literal {BASE_URL} / {DOCS_URL} / {API_KEY} and there is no vendor to sync from.

    cd evals/swarm-bench/bench
    nohup python3 sb7_local_vendor.py --port 8850 \
        --out /tmp/sb7-prompt.md --trace /tmp/sb7-trace.jsonl > /tmp/sb7-vendor.log 2>&1 &
    # it prints {port, seed, prompt, docs_status} — KEEP THE SEED, scoring needs the same one
    ~/goose-builds/loop-state/launch.sh swarm-3node-r0 9897 /tmp/sb7-prompt.md benchmark=true

CHECK BEFORE TRUSTING ANY RUN: the dispatched prompt must contain ZERO `{[A-Z_]+}` placeholders and the
vendor must answer /v3/docs with 200. Both are asserted by the script, and both must be re-checked from
`run_started.prompt` in run.jsonl. The vendor must stay up for the WHOLE run.

## Commands

```bash
# build + install the desktop app (NEVER run headless)
source bin/activate-hermit && cargo fmt && cargo build --release && just make-ui

# launch a local run
~/goose-builds/loop-state/launch.sh <name> 9897 <spec> benchmark=true

# GLM-5.3-flash cloud run
cd /private/tmp/goose-cloud-launch-glm53flash
python3 evals/swarm-bench/bench/cloud_sb7.py status --root ~/goose-builds/cloud-sb7-glm53flash-20260828-r0
# publishes to brun-baseline-glm-5-3-flash-sb70 ; key ZHIPU_API_KEY in
# ~/.agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env
```
