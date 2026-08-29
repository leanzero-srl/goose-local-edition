# EXPERIMENTS LEDGER — what was tried, what it cost, and why it is not coming back

**Read this before proposing a change to the swarm engine.** Most of what looks like an obvious
improvement here has been tried. Several ideas in this file were tried *twice*, because the first
attempt's failure lived only in a conversation that was compacted away.

Each entry states the idea, what it actually did when measured, and the rule that replaced it. An entry
is only removed if new evidence overturns it — and then the overturning evidence goes in its place.

Companion files: `RUN-LEDGER.md` (per-run numbers), `NOW.md` (the current thread), `SWARM-AGENDA.md`
(open work), and the `goose-swarm-campaign` skill (durable procedure).

---

## DEAD — do not revive without new measurement

### Caps, timers and deterministic gates on model work
**Tried:** wall-clock timeouts, turn ceilings, retry counts, volume limits — ~35 of them.
**Measured:** V25 died to a file named `…active-semantic-openers-cut-by-900s-idle-watchdog`. The sink
cap cut `integrate-verify` at exactly 1800s, after 23 shell calls and 2 edits, and the run logged it
`status=done` — a truncated call and a finished one written identically into the row every verdict is
read from. `worker_timeout` replaced a worker carrying a stale hint from its own kill.
**Rule now:** every terminator is progress-based or lives in the transport. `effective_idle_budget()`
returns uncapped for any input and has a test that says so. A configured number may never bound a call.

### The multi-draft plan vote (best-of-N, backbone round, redraft ladder)
**Tried:** N nodes each draft a whole plan; a Rust scorer picks a winner; extra full-redraft rounds.
**Measured:** the backbone round was discarded 28 of 29 times. The ladder measured 84→84→70→70 and
shipped 52 — each round made the plan *worse*. Codex's variant re-emitted six whole plans in 3h40m and
never started building, with the planner compacting at 53,902 bytes.
**Rule now:** one plan, corrected by targeted PATCHES (`plan_patched`), never re-emitted — in ONE review
round (see "Looping REVIEW until a round surfaces no new finding", below).

### De-duplicating review findings on the sentence
**Tried:** cross-round de-dup on `trim().to_lowercase().take(120)`.
**Measured:** one defect reported three ways — "viz-interaction and viz-rendering-engine share the same
file (web/viz.js)", "Two tasks write to the same file (web/viz.js)", "viz.js written by two tasks" — all
counted NEW. A later round prefixed everything `STILL: ` and produced 9 findings with `repeated: 0` on an
untouched plan. The stop rule is "a round with no new finding", so a rephrasing reviewer defeated it by
construction.
**Rule now:** `review_dedupe_key` keys on (kind, identifiers) with basename normalisation. Verified live
2026-08-29: `r1:new=4 → r2:new=0`, stopped correctly — and then r1 (next entry) showed that a correct key
does not make the loop converge. The key survives to collapse one defect raised by two lanes of the ONE
round.

### Looping REVIEW until a round surfaces no new finding (2026-08-29, r1)
**Tried:** REVIEW as a loop of review rounds. Each round's findings were de-duped against every earlier
round's by `review_dedupe_key`; the loop ended on the first round with no NEW finding, with a plan-state
cycle detector (`review_oscillating`) and a same-rejection detector (`review_patch_stuck`, `RejectMemo`)
as terminators for when that never came. No round cap, by design.
**Measured:** r1 (run dir `swarm-3node-r0` at the time, archived as
`swarm-3node-r1-KILLED-review-diverged-8-4-9-new-findings-4-rounds-51min-vs-r0-12min`): round 1 new=8 → `plan_patched` replace 3 remove 1; round 2
new=4 → replace 1; round 3 new=9 repeated=2 → replace 3; round 4 started. 51 minutes and 209,110
reasoning characters in REVIEW, against r0's whole REVIEW of 12 minutes in two rounds. Round 1 caught the
one real structural defect — `viz-engine` owned no files — and SYNTHESIS had ALREADY flagged it,
deterministically, in `plan_synthesized.tasks_owning_nothing=['viz-engine']` before the round began; the
reviewer spent ~25,000 characters rediscovering it. Rounds 2 and 3 found dependency tweaks and "X is not
explicitly owned" cross-cutting concerns (error envelope, webhook URL registration).
**Why it is not coming back:** an LLM reviewer's novelty never converges. Asked "what is missing?", it
always finds something, and de-duping harder (the sentence key, then the (kind, identifiers) key) only
changes WHICH rephrasing counts as new — it cannot make the stop arrive. The real signal was the
deterministic flags the engine already computes, and the loop paid a model to recompute them slowly and
then kept paying.
**Rule now:** REVIEW is ONE round (`review_once`) — phase structure, like OPEN and SYNTHESIS being one call
each, not a cap; the one call stays uncapped. SYNTHESIS's measured flags — `tasks_owning_nothing`,
`shared_files`, `module_package_collisions` — go into that round's prompt as a MUST-FIX block
(`review_must_fix_block`, omitted when the plan is clean), so the round is aimed at the structural defects
instead of rediscovering them. Findings and patch are applied exactly as before (`review_findings`,
`plan_patched` with `round: 1`; the one patch demand for findings-without-a-patch stays, as a follow-up
inside the round). `review_oscillating`, `review_patch_stuck` and `RejectMemo` went with the loop.

### Personas and roleplay for workers
**Tried:** "You are a WORKER on a local AI swarm", supervisor/subordinate framing.
**Measured:** role-as-identity is null on this model class (Zheng et al., 162 personas × 2,410 questions
on Qwen2.5-Instruct; none beat the no-persona control). A LOW-STATUS role — exactly the "worker who obeys
the supervisor" register — measurably COSTS: 51.6 / 45.3 against 53.5 for no role.
**Rule now:** ownership and duty lines, not identity. `kind_prompt` SUBTRACTS rules; it never adds a
persona. Instruction density is the mechanism that pays.

### Killing a spiralling call
**Tried:** the judge ends an unproductive call; re-stream on drift.
**Measured:** every one of 13 nudges in one run was a re-stream that discarded the call's work — one
review lane fell from 27,297 characters to 2,004. The judge's net contribution that run was NEGATIVE.
**Rule now:** the judge NUDGES. Steer lands at a turn boundary and costs nothing; cancel keeps the
partial. `may_terminate` is false at 12 of 14 call sites — only the coverage fan and the review fan can
absorb a lost lane.

### Suppressing DRIFTING on any producing call
**Tried:** hold DRIFTING whenever the call is producing, because 33 of 34 such nudges changed nothing and
cost 66 minutes of worker time. The measurement is real and the hold was right.
**Measured:** "producing" counts reasoning characters, so a call that reasons and never acts is producing
by definition — which is the pathology DRIFTING exists to name. Live 2026-08-29: `open-coverage-1` reached
68,393 reasoning characters with ZERO tool calls, was diagnosed DRIFTING, and was held. Five DRIFTING
verdicts across the run produced one nudge.
**Rule now:** drift corroborates like LOOPING — held once, delivered on a second DRIFTING with still no
action taken. Acting resets it.

### Checking the neighbour of the thing you mean
**Tried, three times in one day, all in instruments:** run-directory NAMES for liveness; the installed
BUNDLE for whether the running app is current; the outer cell WRAPPER for whether a control is clickable.
**Measured:** a run dead for hours reported as live with an ETA; a two-hour-old zombie app serving CDP
while the check said "current with HEAD", so every UI verdict for a morning was about old code; and an
instrument reporting "clicking a node cell opened nothing" about an inspector that opens fine.
**Rule now:** assert on the property itself. Liveness reads `.swarm/heartbeat` and `pgrep`; the install
check compares each process's start time against the bundle mtime; the click tick clicks the
`role="button"`.

---

### Scoring under hermit's node, or without `--seed` (2026-08-29, r0)
**Tried:** scoring r0's tree with whatever `node` was on PATH (hermit's v24) and without passing the seed
the run was fed. Twice.
**Measured:** 0.0832 — a number that looked comparable and was not. hermit's node has no playwright, so
the browser probe crashed on every J/V/P/T/E check: 30 of 99 checks came back PROBE-UNAVAILABLE, the means
quietly excluded them, and the frontend r0 built was never graded. Both scores also drew a FRESH seed at
port 8899 while the run had been fed `687ff58bfa6b707d` at 8850, so every fixture-derived expectation was
for a different dataset. A second `'str' object has no attribute 'get'` crash (`d_peer_absence`) was the
same class as one already fixed at one site. Rescored correctly: **0.0568**, with a second critical the
blind run could not see.
**Why it is not coming back — the scorer REFUSES instead of guessing** (`evals/swarm-bench/bench/score_sb7.py`):
`--seed` is REQUIRED (16 hex, from the vendor trace header; `--fresh-seed` only on purpose) — exit 2
without it; `_probe_preflight()` runs the probe script with `--preflight` under `GOOSE_SWARM_RENDER_NODE`
and refuses when playwright cannot load (`--allow-blind-probe` is the only way past, and the verdict says
so); `_port_holder(port)` names the process holding the vendor port before bind instead of dying with a
traceback (shared with `run_build.py`); `_error_obj(body)` is the ONE predicate for the error envelope,
so a str-vs-dict body cannot crash a check site again. `loop-state/compare_vs_cloud.py` reads the
verdict's `score` field rather than recomputing inner × crit (it had overstated 0.0568 as 6.44%).
**Rule now:** score serially, hermetically, at the advertised port, with the run's seed and a node that
passes preflight — anything else is a number about the scorer, not the run.

## THE TARGET'S SCORECARD — read before designing anything (2026-08-29)

The 20.06%/22.07% run is not a run that wrote better code. Its verdict.json says:

    inner            0.7662     the app itself was GOOD
    crit multiplier  0.288      three unsuppressed criticals
    FINAL            22.07%

It lost **72% of its score to three criticals**, not to code quality:

| critical | factor | what it was |
|---|---|---|
| `b_buckets_dst` | 0.8 | wrong money — mis-bucketed days |
| `j_workflow_journey` | 0.6 | dead primary flow — approval cannot complete through the UI |
| `r_workflow_durability` | 0.6 | data loss — submitted/approved state reverting after SIGKILL |

**This reframes the whole target.** Byte counts and wall clock were implying "write more, write faster".
The scorecard says the lever is "stop tripping criticals", and criticals MULTIPLY rather than add — a
better app with more unsuppressed criticals scores LOWER, which has already happened here once (2.6× the
inner scoring 0.017 against 0.0273).

**Our engine already aims at all three, verified 2026-08-29 rather than assumed:**

- `b_buckets_dst` → the REVIEW `domain-conventions` dimension, which names calendar/cron/timezone
  explicitly, plus `correctness` for a wrong constant, unit or sign.
- `j_workflow_journey` → the REVIEW `wiring` dimension: "a spec deliverable is BUILT but never
  imported/wired into the program's entry point, so the advertised behaviour is unreachable at runtime."
- `r_workflow_durability` → an in-run check at `swarm.rs:20653`: it respawns the app on the SAME db with
  the SAME argv and recounts rows. Its own comment records this as "the `h_durability` class the fleet
  shipped blind — now an in-run finding."

So the machinery is pointed at the right targets. Whether it FIRES is what r0's score will say.

## OPEN QUESTION — raised by evidence, not yet answered

### Transport drops are excluded from exhaustion. Correct premise, possibly wrong diagnosis.

**The rule:** a `stream decode error (mid-stream body drop)` does not count toward `real_failures`, so a
task hitting them retries rather than exhausting. **The reasoning is sound and was paid for**: counting
them let a flaky node DELETE finished-quality modules from a build — r1 lost three tasks, two of which
never recovered.

**What r0 shows:** `app-js` drew that exact error three times, at 11:30, 11:46 and 11:58, on **three
different nodes** (gabee → mihai → gabee → workhorse), burning 45 minutes of BUILD while its 4,798-byte
`web/app.js` sat finished on disk. A fault that follows the task across every node in the fleet is not a
node fault.

`app-js` is the longest generation in the run — the whole page behaviour: pagination, filtering, custom
dropdowns, optimistic updates with rollback, polling with degraded states, a drafts panel and role token
management. The plausible reading is that the length is the cause and the socket is the symptom, in which
case the engine is retrying a generation that will drop again for the same reason, forever, having
classified it as somebody else's problem.

**RESOLVED THE SAME RUN, against my suspicion — and the existing rule was right.** `app-js` completed on
its FOURTH attempt at 12:08, and the run went straight to INTEGRATE with 9 of 10 tasks done and code
bytes jumping 80,966 → 108,682. So the drops were genuinely transient after all: the retry-forever
behaviour bought a finished task where exhausting would have shipped a partial file and degraded the
capstone.

Worth keeping precisely because I was about to design against it. The shape "same error, three different
nodes" LOOKED like proof the task was the variable, and it was not — three nodes on one LAN share enough
that a fault can follow work around without being caused by it. The lesson is not about transport: it is
that a suspicious pattern with a plausible mechanism is still not evidence, and a run in flight can
answer a question faster than a redesign can.

The cost is real and stays on the record: **45 minutes of a 3-node fleet on one task's transport drops**,
which is roughly the whole of BUILD. If a future run shows the same task failing this way and NOT
recovering, the candidate signal is the same task drawing the same transport error on N distinct devices
— it needs no clock and no cap. Until then there is nothing to fix.

## ALIVE BUT UNPROVEN — measured once, not yet twice

- **Slice-level decomposition (OPEN → RESEARCH → SYNTHESIS).** r0 produced 10 tasks over 16 files with
  zero collisions and chain depth 3 — the best plan this project has made. One run.
- **The tree warden** (`sweep_tree_defects`). Built, tested, has not yet fired on a real hollow
  dependency because nothing had reached BUILD until r0.
- **S1/S3/S2 realtime path.** Verified end-to-end in the running app; not yet watched through a full run.
- **The coverage loop (`coverage_gap` → second pass) — MEASURED on r1, 2026-08-29.** It EARNED its place: the gap pass
  ADDED `frontend-serving`, `reversals-fetch` and `sse-stream-endpoint` (12 → 15 slices; `coverage_gap` at 16:15:43Z,
  `coverage_late_slices researched_after_the_first_wave=true`), so r0's `GET /` 404 — the 0.56-weight loss — had an OWNER
  before BUILD, and the re-enumeration after the add found 0 unowned. What it COST: the second pass ran under the same
  lane key, SERIALLY, on ONE node while two idled (`open-coverage-1` restarted 41k → 13k chars at 16:18:51Z), stretching
  RESEARCH to 32 min against a 19 min median (r0: 39 min); that lane reached 41,239 reasoning chars with 0 tool calls and
  was judged OK on every look. Not a kill and not a cap: the waste to attack is that coverage is bounded by a judge that
  reads reasoning as production rather than by PROGRESS (a table row landed), and that a second pass has no reason to be
  serial. One run.

---

## FIXED — the engine hung after the repair verdict; root-caused and closed (2026-08-29, r0 → `44b2ad6cd`)

**Symptom.** After `complete: STOPPING at round 0 …` the process sat 20+ minutes at **0.0% CPU**,
`run.jsonl` frozen at 589 events, fleet idle, `run_build.py` waiting on it. The tree and the verdict were
already on disk — only the exit was stuck.

**Root cause — proven, not guessed.** `boot_invocation` (the post-verdict epilogue, via `boot_probe`)
spawns `python3 -m app` with piped stdout/stderr, polls, `kill()`s the ONE direct pid, then awaits the
pipe tails to EOF. The wrapper's `Popen` grandchildren (`ledgerd`, `notifierd`) inherit the write-ends and
survive a single-pid SIGKILL, so EOF never comes and `run_swarm` parks forever. Proof: pids 11519/11520,
cwd in the r0 run dir, PPID 1, started at 16:16:22 local — the exact second `fix_criticals` was logged.
Corroborated by three adversarial refuters and one independent agent.

**Corrections to the first write-up of this entry.** `_pthread_join` on the main thread was NEVER a
finding: `main.rs:36` joins a big-stack thread for the whole run, so the main thread sits in
`_pthread_join` from start to finish, hang or no hang. "The heartbeat was empty so Drop never ran" read a
file that does not exist — the heartbeat lives at the tree root beside `run.jsonl`, not `.swarm/heartbeat`,
and it live-ticked for 20 minutes with no `EXITED:` sentinel, i.e. the tokio runtime was alive throughout.
This was not a runtime-shutdown deadlock, and `swarm.rs` has no `spawn_blocking`/`thread::spawn`/`.join()`
to walk.

**The leak was systemic, not just the exit.** 41 orphaned app servers (PPID 1) were alive on the machine,
~25 from r0 alone, spawned all through INTEGRATE/TEST/RATE: every boot probe or smoke that killed a wrapper
leaked its grandchildren holding ports and RAM, and `boot_probe` refuses to conclude on a pre-bound port,
so one probe's leak poisoned the next.

**Fix `44b2ad6cd`.** Every app spawn leads its own process group (`process_group(0)`); `kill_app_tree`
SIGKILLs the group; the pipe drain is released on GROUP liveness (`kill(-pgid, 0)`) rather than EOF — a
drain-after-kill on an already-dead non-model process, not a model cap. Applied at all six spawn sites
(`boot_invocation`, `run_spec_contract`'s server and its restart-durability reboot, `run_repro_once`, the
pytest `--collect-only` in `land_generated_tests`). Regression tests
`boot_invocation_returns_when_a_grandchild_holds_the_pipe` (hung under a 20 s alarm on the old code, exit
142) and `boot_invocation_returns_when_the_grandchild_escaped_the_group`, both ~4 s.

**Isolation proof, no run needed.** `goose swarm gate <r0 clone> --spec evals/swarm-bench/spec-build-sb7.md`
— NOT `swarm verify`, which never boots the app and prints "clean" in 3 s — on the OLD 16:52 binary
returned its findings but LEAKED 2 app servers in under a minute; on the NEW binary (`d748a7d3e`) it
returned in 2.6 s with 4 real findings and 0 leaked servers. r1 then ran 98 minutes with 0 orphans through
the run and after the kill; r2's gate replay: 4 findings, 0 leaks. `tick.py` counts orphaned app servers
every tick, so a regression is visible at the next tick rather than the next hang.

## REPAIR HAS NEVER RUN IN A BENCHMARK — found 2026-08-29 on r0

```rust
let proxy_yes = !benchmark() && (round == 0 || last_round_promoted);   // swarm.rs:37015
```

Under `GOOSE_SWARM_BENCHMARK=1` — which is how every benchmark run is launched — `benchmark()` is true,
so `!benchmark()` is false, so **`proxy_yes` is false unconditionally**. The repair-continue ask can only
ever be answered NO. r0's console says it plainly:

    ✗ 29 critical defect(s) remain, 2 minor
    complete: STOPPING at round 0 with 29 critical(s) open — proxy said no

`complete_fix_dispatched: 0`. **Not one fix was attempted.** TEST found 12 defects, RATE expanded them to
29 criticals and 2 minors, and the run shipped every one of them untouched.

**The code believes the opposite.** Its own comment three lines below reads *"round 0 buys round 1
because proxy_yes is true at round 0"* — describing a branch that cannot be reached in the only mode we
ever measure in. The intent is visible and correct; the expression contradicts it.

**Why it looks defensible:** the guard exists because "under `benchmark` a model asked 'want another
round?' answers yes forever, and the only exit is Ctrl-C". That is a real hazard and the fix for it is
`last_round_promoted` — grant a round while the tree is still changing, refuse once it stops. The
`!benchmark()` term is belt-and-braces on top of a rule that already terminates, and it disables the
whole phase to prevent a loop the other half of the same expression already prevents.

**Consequence for every number this project has published.** Each local benchmark result is a
pre-repair score: the app as TEST first found it, with no fix wave, no re-test, no verdict loop. The
REPAIR design — the fan by file, the promote-only-if-better guard, `fix_converged` — has never executed
under measurement. That is not a small correction to the numbers; it is a phase of the engine that has
been dark every time we looked.

**The fix is one term**, and it must keep the anti-loop property: `round == 0 || last_round_promoted`
already refuses the moment a round stops changing the tree, which is exactly the terminator the design
asks for. Removing `!benchmark()` restores the phase without restoring the infinite-yes.

**FIXED `a1324c68e` (16:44).** The `!benchmark()` term is gone; `round == 0 || last_round_promoted` is the whole guard. r1 launched
with `levers_resolved.levers.benchmark=true` and REPAIR round 0 reachable; r2's claim (3) is the first measurement of the phase
under a benchmark.

## THE GUTTING — measured on r0, 2026-08-29. This is the mechanism, not a metaphor.

Mihai: *"check if the model isn't gutted by something. Clearly the cloud model shows us that it's very
possible to do much better."* It is not gutted by a cap, by sampling, by context or by the thinking
prefill — all four were checked and cleared:

- sampling is entirely `null` in config: temperature, top_p, top_k, min_p, repeat_penalty. The model uses
  its own defaults.
- the `<think>` pre-close SUPPRESSES deliberation but is correctly scoped: off by default, and otherwise
  only on a RETRY of a task whose owned file is still missing. The first attempt keeps full deliberation.
- no cap bounds a call (`effective_idle_budget` is tested to return uncapped for any input).
- context is 262,144 on two nodes and 135,936 on gabee — an asymmetry worth fixing, but not binding on a
  ~22k-character prompt.

**What IS gutting it is that BUILD workers do not LOOK at anything.** Measured, per lane, from the
digests' own `calls` records:

| worker | tool calls | reads |
|---|---|---|
| `boot-wrapper` — depends on ledgerd AND notifierd | **0** | **0** |
| `styles-css` | 0 | 0 |
| `notifierd-service` | 4 | **0** |
| `viz-core` | 3 | 1 |
| `app-js` | 3 | 1 |
| `ledgerd-service` | 8 | 2 |
| `integrate-verify` | 60 | 23 |

`boot-wrapper` wrote the code that wires both services together **without reading either service**.
`notifierd-service` never read the ledger it has to integrate with. Only the sink behaves like an agent.

**This is exactly the difference the agenda names and it is now quantified.** The cloud single agent
writes `notifierd.py` HAVING SEEN `ledgerd.py` — coherence is free because there is one context. Our
worker writes it having seen a 4,789-character ENGLISH DESCRIPTION of it. Prose cannot be as good as
looking at the code, and we spend 266,614 reasoning characters manufacturing the prose.

**THE PRECISE SHAPE, found by reading the prompt rather than guessing at it.** The workers are not
careless; they are doing exactly what they are told. CONTRACTS freezes each module's interface into a
signature stub, and the worker prompt hands a dependent those stubs under the banner *"build against
these EXACTLY"*. There IS a read-the-real-file instruction — at `swarm.rs:25603` — but it fires ONLY in
the failure case, when a stub could not be parsed: *"Its real interface exists only in its source file.
If you must call this module, read that ONE file directly before writing the call."*

So in the normal case a dependent receives its dependency's **SIGNATURE** and never its **BEHAVIOUR** —
what it actually does with bad input, what shape the data really has, which error it raises, what it
writes to disk. And by BUILD time the real file EXISTS: the DAG will not dispatch a dependent until its
dependencies are Done. The engine is withholding a file that is sitting on disk, finished, three
directories away.

That is defensible for parallel work in general — a stub lets you build before the dependency lands —
but it is exactly wrong for a task whose dependencies have ALREADY completed, which is every task with a
`depends_on` in a DAG the scheduler is honouring.

**AND r0's OWN DEFECTS PROVE IT.** TEST found 12 defects. Five of them are the signature-without-
behaviour failure, verbatim:

1. `app/sync.py` reads `body.get("items", [])` **but the vendor API returns `"data"`** — sync never loads
   a single payment.
2. Health endpoint returns the wrong shape.
3. Summary endpoint returns `{"data": []}` instead of `{count, last_sync, oldest, newest, by_currency,
   reversals}`.
4. Buckets endpoint returns the wrong shape.
6. Sync parses **`amount` instead of `amount_minor`** — so nothing persists.

A worker that had READ the vendor's actual responses, or the ledger's real code, could not have written
`items` for `data` or `amount` for `amount_minor`. These are not reasoning failures; the model never had
the information. It had a signature and 4,789 characters of English about a signature.

And defect 5 is the whole 0.0273 failure again: **`GET /` returns `{"error": "Not found"}`** — the
frontend files EXIST and are self-consistent (index.html references styles.css/app.js/viz.js and all
three are on disk) but nothing serves them. That is `j_workflow_journey`, the critical that cost the
cloud run 40% of its score, and roughly 0.56 of the scoring weight is unreachable without a served page.

The remaining six are missing input validation — a different class, and the one place where more prose
in the brief might genuinely have helped.

**The candidate change:** when a dependent's dependencies are Done, tell it the real files are on disk
and to read them before writing. It buys the single agent's coherence for one tool call rather than for
4,789 characters of brief. It is an instruction change, not an architecture change, and it is the
highest-value candidate r0 produced. It needs a design and a run.

## THE STANDING NUMBERS

**ONLY THE SCORE COMPARES US TO THE CLOUD.** Not wall clock — the cloud entrant runs on far faster
hardware, so minutes measure the machine, not the method. Not bytes — more code is not better code, and
a single agent has no OPEN/RESEARCH/SYNTHESIS/REVIEW/CONTRACTS to spend budget on, so any
bytes-at-time comparison silently indicts phases the other run does not possess. What survives a
hardware difference is WORK (characters reasoned, tool calls, tasks completed, retries) and OUTCOME
(the score). Phase timings are for diagnosing OUR OWN waste against OUR OWN phases; they are never a
cross-run number.

**The number to beat is 20.06%, not 0.0273.** Those answer different questions and confusing them lets a
bad run look like progress. `0.0273` is the local row currently PUBLISHED on leanzero.net — it is what a
new result would replace, and it is a floor, not a target. **20.06% is `qwen3.8-27b` — the SAME MODEL this
fleet runs — scored as ONE cloud agent with no planning, no decomposition, no judge and no fan.**

That is the honest falsifier for the entire swarm thesis. Three nodes of a model must beat one node of
that model, or the decomposition, the contracts, the supervision and the fan are costing more than they
return. Everything in this repo exists to clear 20.06%; clearing 0.0273 only means the run finished.

| | value | what it is |
|---|---|---|
| **THE TARGET** | **20.06%** | `qwen3.8-27b` via OpenRouter, ONE agent, no planning — the same model as the fleet |
| local published row | 0.0273 | `brun-fleet-qwen38-brainwaves-sb70` — the floor a new result replaces |
| **r0, 2026-08-29 — the comparable score** | **0.0568** | hermetic: inner 0.1789 × crit_mult 0.36, seed `687ff58bfa6b707d` (the run's own), vendor port 8850, playwright node. 28% of the target, 2.1× the published row. TWO criticals gate it: `sync_completeness` 0/12288 (`sync.py` reads `items`; the vendor sends `data`) and `j_workflow_journey` (`GET /` 404 — the frontend exists and is unserved). `verdict-hermetic-seed687ff58b-port8850-0.0568.json` in the run dir |
| ~~r0 first score~~ | ~~0.0832~~ | **RETRACTED** — BLIND (hermit node, no playwright: 30/99 checks PROBE-UNAVAILABLE, the frontend never graded) AND on a FRESH seed at port 8899. Kept as `verdict-BLIND-fresh-seed-NOT-COMPARABLE-0.0832.json`; never cite it |
| cloud board leader | 67.53% | deepseek-v4-flash-vision-exp, single agent — a different, stronger model |
| a single qwen3.8-27b, measured | 106 min, 9 files, 163,962 B incl. the whole frontend | beat the 3-node fleet on wall clock AND product |
| glm-5.3-flash, single agent | 41.59% | 72.5 min, 14 files |
| spec written before any code, last full run | 140,680 chars | 86% of the winner's finished codebase |
| brief size that scored 88.7% | ~1,500 chars | vs 6,443 median then, 4,789 on r0 |

**Why the single agent wins, mechanically:** it writes `ledgerd.py`, then writes `notifierd.py` HAVING
SEEN `ledgerd.py`. Coherence is free because there is one context. Parallelism destroys that coherence,
so the fleet spends its whole budget rebuilding it IN ADVANCE, in prose — and prose can never be as good
as looking at the code. r0 spent 258,566 characters of reasoning to write 74,963 bytes of program.
