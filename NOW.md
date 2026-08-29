# NOW — what we are researching and doing, this week

**Read this FIRST, at every tick, before `SWARM-AGENDA.md`.** The agenda is 2,400 lines of campaign
history and it is the record; this file is the *current thread* and it is short on purpose. Compaction
has repeatedly destroyed the answer to "what were we in the middle of", and that is the only question
this file exists to answer.

Rule: when the current thread changes, THIS FILE CHANGES IN THE SAME COMMIT. A stale NOW.md is worse
than none.

---

## ⚠️ FIRST THING AFTER A COMPACTION — RE-MINE, ON A BUDGET

A compaction summary keeps the *shape* of the work and loses the *thread*. Resuming from one has already
sent me onto the wrong task while the real thread sat untouched.

**But recovery must be CHEAP.** Mihai: *"we can't afford for eg. after a compaction to overconsume and
get to 50% right off the bat but we should be holding persistent 200k context all the time so the
sessions are alive and sharp."* A recovery that costs half the window is not a recovery, it is the next
compaction. **Budget: ~10k tokens. Never more.**

The rule that makes it cheap: **anything that requires reading a LOT is delegated, and only the
conclusion comes back.** A subagent burns its own context, not mine. I never grep 46MB of transcript
into my own window.

```bash
# ~2k tokens, and it is usually enough on its own
cat ~/Projects/goose/NOW.md
grep -n '^- \[ \]' ~/Projects/goose/SWARM-AGENDA.md | cut -c1-160   # open items only, never the file
sed -n '1,60p' ~/.agents/skills/goose-swarm-campaign/SKILL.md          # the header, not all 707 lines
tail -5 ~/Projects/goose/TICK-NOTES.md                                 # newest findings
git -C ~/Projects/goose log --oneline -12
```

Then, ONLY if the thread is still unclear, spend one Explore subagent on the raw transcript:

> Read `~/.claude/projects/-Users-mihaiperdum-Projects-goose/<session>.jsonl` (tens of MB, one JSON per
> line — grep it, never read it whole). Return **under 400 words**: the user's last 5 instructions
> verbatim, any mandate phrased as always/never/must-be-respected, and what work was mid-flight.

**State the current thread in one line before continuing.** If it does not match what the summary
implied, the summary was wrong and the sources win.

### Staying sharp during normal work, not just after a compaction

- **Delegate reading, keep conclusions.** Any question answered by sweeping many files goes to a
  subagent or a workflow. The finding lands in my context; the file dumps do not.
- **Read ranges, not files.** `sed -n '3200,3300p'` over `cat`. Never re-read a file I just edited.
- **The durable files ARE the memory** — `NOW.md`, `SWARM-AGENDA.md`, `TICK-NOTES.md`, the skill. Write
  a finding down and it costs nothing to carry; hold it in context and it costs on every single turn.
- **Keep NOW.md short.** If it grows past ~120 lines the detail belongs in the agenda, not here.

---

## The goal, unchanged

**PART II LANDED (`3a09bf427` + amendments `55cd200d6`, DESIGN-STABILITY-FIRST.md:300-560): the stateful
harness — capture → `.swarm/ledger` → read-before-act message; a semantic dispatch-time sink description
built from the run's real artefacts (worked example from r2); don't-repeat as INPUT; the time inventory
with per-mechanism replacements (the 600 s read cut and the 420 s stopwatch both provably decided model
work WRONG in r2 → progress/lms-ps replacements); SAID provenance chips; the forming line (LM Studio arg
streaming honestly UNMEASURED — measure before building); Claude Code patterns mirrored with doc cites.
II.7 = a 15-row ordered r3 list in TWO TRACKS: desktop row 10 (SAID) may start now; row 11 (forming) after
its LM Studio measurement; ALL engine rows only after run_finished + the r2 score, re-verdicted per row.
Nine refuter amendments applied (min_age_secs was itself a wall clock; the K-counter needed the lms-ps
guard and r2-derived K; the early gate run kept inside sink formation; the 1800 s extension expiry returns
a measurement, never an error).**

**MIHAI'S STEER, 2026-08-29 23:32 EEST — decomposed into seven asks (his words in TICK-NOTES; memory
`stateless-models-harness-forms-the-message`):**
1. **Wait for r2's score before deciding anything about r3.** If it fails or scores badly "we still need to figure
   stuff out" — but NEVER redesign from scratch: keep r2's good parts, add what the research noted, throw away
   what r2 showed was waste. Maintain the ledgers and skills so no mistake is made twice.
2. **Inspector WORK pane: a tool call must be visible as it FORMS** — "a line that is loading and generating" —
   not only once done. (`156a95957`+`26612c1a3` show the call the instant its request is complete, with a
   spinner; the FORMING part — while the model still generates the arguments — needs the provider stream's
   argument deltas. Research lens R4.)
3. **SAID pane has no state:** it kept showing attempt 0's "Network error: Stream decode error" while attempt 1
   ran — nothing says current vs. superseded, which attempt, when. Needs provenance + an error state.
4. **No time-related mechanism anywhere** — "local models suck". Round/progress-related may stay with r2
   evidence. This puts the 1800 s provider read window (`afa644ddd`) on the table too: a time cap on silence.
5. **The integrate task is "hilariously generic" — "INTEGRATE EVERY MODULE"**: it must be a SEMANTIC, specific
   task built from this run's artefacts (which modules exist, entry points, what already ran and passed).
   "The models are poised to think more the less specific something is."
6. **BUILD runs tests, then INTEGRATE runs tests again.** The good deterministic thing: CAPTURE tool calls and
   outcomes per task into mini-ledgers, and let CODE form the next task's message from them ("don't run the
   tests again — they ran 22 times, here is what failed"). Read-before-act is the only acceptable gate — the
   ledger is injected so the model cannot start without it. Models are stateless; the harness forms the
   message for the next turn. Read Claude Code's own mechanisms (memory files, notes-as-you-go, hooks,
   compaction) and mirror what transfers.
7. **"Decompose, research, synthesize, bring to fruition"** — a research workflow is running now (five lenses +
   synthesis + two refuters); its output lands as PART II of DESIGN-STABILITY-FIRST.md. Engine code waits for
   r2's score; the two desktop items (2, 3) may go first once the research says how.

**r2 IS OVER — KILLED BY MY OWN REAP, 2026-08-30 01:44 EEST.** At 01:31 the leaked-server checkpoint fired (3 PPID-1
app servers from the sink's dead attempt 0 — the task_retry/body-drop path skips `kill_app_tree`). I reaped
them with `killpg`; the bare-spawn leak path never calls `setsid`, so their group WAS the engine's, and the
engine died with them at INTEGRATE minute 139 (heartbeat frozen 22:32:20Z, the reap minute). Owned: memory
`kill-pids-never-killpg`, skill §7 reaping rule, tick.py comment; launch.sh's per-pid reaper was always the
right tool. CONSEQUENCES: claims (2) completion/no-hang and (3) REPAIR never got settled by r2 — r3's first
run settles them on the new binary; NO re-run of the old binary (4 h for evidence about code r3 replaces).
run_build.py is auto-scoring the killed tree now; when it exits: hermetic score of the tree AS-IS labeled
KILLED, archive the dir truthfully (`…-KILLED-by-operator-killpg-reap-INTEGRATE-139m`), launch the
assessment workflow (its forensic lens gets my kill as a first-class event), then the build-everything
campaign. r2's evidence haul stands: one-round REVIEW ✅, BUILD 10/10 ✅, two body drops (~52 min), the B2
sink hold, the tail_review machine-gun (470+), the retry-path leak, gabee's dropout, the forming gap.

**FULL AUTONOMY (Mihai, 2026-08-30 00:21): "don't wait on me for anything - go at it unattended. I will ask
questions if need be."** No decision waits on him — including the deletions; the adversarial reviews are the
authority. Short reports continue every tick.

**THE r3 PLAN (Mihai, 2026-08-30 00:19 EEST, supersedes the 22:35 scope rule): EVERYTHING goes into r3.** No parts
staged across runs. The sequence: (1) r2 finishes; (2) assess r2 VERY THOROUGHLY (score + full review);
(3) build every candidate — Part I, Part II, Part III — into r3, batch by file, isolation-tested;
(4) every change passes adversarial review; **what does not survive the culling does not survive** and is
recorded in `REFUSED.md` (with why and what would revive it — check it before proposing anything new);
(5) rebuild, install, launch r3 only after the r2 review is done "so we know exactly what is needed".
The r2-evidence table below stays as INPUT to the reviews, not as a gate on what gets attempted.

**r3 KEEP/DROP CANDIDATES — judged against r2's score and events, not before (prepared 22:45):**
| candidate | the r2 finding it answers | class | verdict |
|---|---|---|---|
| `2b1e755ac` re-stream on ignored steers | r2 opener: 5 steers ignored, 22 min; r1 review lane: 6 | KEEP-class | after score |
| `afa644ddd` 1800 s provider read window | r2 open-coverage-2: 581 s silent slot, valid result | KEEP-class | after score |
| `ee0cbfe73`+`fae38abc3` PLAN-REPAIR, warning-only | r2 REVIEW fixed sharing/owning-nothing itself → the pass is a no-op safety net | KEEP-class, zero cost | after score |
| `ac9715d24` phase chips (Ask, Contracts) | Mihai 22:35 | his ask | keep |
| `156a95957` + panel RUNNING rows | Mihai 22:29 | his ask | keep |
| shortcut fix (workflow) | Mihai 22:00 | his ask | keep |
| judge sees the owned files (agenda :2486) | r2 camera-system: judge said "ok" while the lane coded at the tree root | ADD, mild (a measurement fed to the judge) | after score |
| `dynamic_replan: false` (config lever, `arm_config.py --set`, no plumbing) | r2 19:45Z: the replanner spliced `ledger-core-tests` + `vendor-sync-edge-tests` (296/327-char briefs) at the end of BUILD; the B2 claim gate then held INTEGRATE behind them while all 10 plan deps were done; `ledger-core-tests` attempt 0 went over_reading ×3 and died to a mid-stream body drop at 19:57Z (12 min), attempt 1 still running at 20:01Z | KEEP-class lever flip | after score — strong |
| `GOOSE_SWARM_PREREVIEW=0` (one env line in the Benchmark spawn; dies with any later deletion) | r2 20:00-20:11Z: 9 pre_review calls of 220-340 s on done tasks while the sink waited; `had_findings` sometimes true, no follow-up event ever | env line, not plumbing | after score |
| `qa: false` (config lever) — its testgen call | r2 20:02Z: one 352 s testgen call on mihai, `landed: None`, "no landable fenced test block" | KEEP-class lever flip | after score |
| the BODY-DROP class: root-cause + salvage (II.7 rows 1/8: calls.jsonl survives resets; prior_hint on retry) | r2 lost ~52 min to two 'mid-stream body drop' retries after long silent generations (ledger-core-tests 12 min; the SINK 40 min at 00:26:35) — both attempts' work erased by the digest reset | ADD — top of the assessment | assessment lens 3 |
| semantic integrate task + run ledger (asks 5-6; Part II) | r2 23:41: the sink re-ran pytest 9× in its first 35 calls after BUILD lanes ran it 20+×, and EDITED app/ledger_core.py (another task's file); its description is the template line | ADD — the owner's ask | Part II, then r3 |
| delete coverage lanes / RESEARCH | r2: RESEARCH 48 min, fleet 1/3 for 12+ min behind one coverage call, a slice coding instead of briefing | DELETE — Mihai's call | his word |
| delete CONTRACTS | r0 sync critical built against a stub; Mihai 22:13 "what the fuck" at the stub nudge | DELETE — Mihai's call | his word |
Anything not on this table does not go into r3.

**DESIGN STEER (Mihai, 2026-08-29 22:30, binding for every step from here): "let's avoid making it overly
too deterministic and gated, be very mild with this, we've done deterministic and plumbing a lot and it
didn't work because of how unpredictable these models are."** Code MEASURES and feeds the measurement to a
model call; it does not refuse, abort, cap or hard-limit model work. A deterministic pass may exist only as
an idempotent safety net that is a no-op when the model already did the job, and it never ends a run.
Terminators are lenient and progress-based (stop when nothing improves), never "exactly one round".
Supervision that redirects is the mild tool for unpredictability — cut its cost, do not replace it with
rules. Concretely: the plan-boundary REFUSAL that landed in `ee0cbfe73` becomes a warning; the design's
"exactly one repair wave" becomes "repair while it improves"; REVIEW (one round, worked in r2) stays.


Make the swarm **build better software** on 3 local LM Studio nodes.

**THE NUMBER TO BEAT IS 20.06%** — `qwen3.8-27b` scored as ONE cloud agent with no planning, no
decomposition, no judge and no fan. That is the SAME MODEL this fleet runs, so it is the honest falsifier
for the whole swarm thesis: three nodes of a model must beat one node of it, or none of the machinery is
earning its cost. The published local row is 0.0273 — that is the floor a new result replaces, not the
target, and clearing it only means the run finished.

**THE PRIORITY ORDER, set by Mihai 2026-08-29 after ~13 weeks: STABILITY > SPEED > QUALITY, lexicographic.**
The adoption case is fast + stable + some extra quality, in that order. Every stability failure measured
today lived in an LLM-in-the-loop layer that assumed convergence; the deterministic plumbing is what
worked. Design work lives in `DESIGN-STABILITY-FIRST.md` (panel output, 2026-08-29 evening). Prefer
deleting a layer to gating it; measure the fleet against ONE local node of the same engine ("all vs single").

**BUILD has been reached once (r0, 2026-08-29). REPAIR has never yet run to a verdict under benchmark.**

---

## WHAT WE ARE DOING RIGHT NOW

**One sentence: r2 is running under the vigil (both tick halves every 10 min, kill checkpoints armed);
`DESIGN-STABILITY-FIRST.md` (BP-1, the next evolution under STABILITY > SPEED > QUALITY) is WRITTEN and
REVIEWED. Step 1 (PLAN-REPAIR) LANDED `ee0cbfe73` — `repair_plan_flags` + `finalize_plan_before_dag`, 8 tests
on the real r0/r1 plans, a non-sink owns-nothing task is REFUSED at the plan boundary; step 14's fixtures
for `plan_repaired` are IN (tick.py, snapshot_run.py, the panel). Ships in the r3 binary.
The DELETIONS (REVIEW, CONTRACTS, judge, TEST/RATE) wait on Mihai's read — they are his call.**

**Design §8 arm-S trap — RESOLVED 21:35:** benchmark runs never read config.yaml's `devices:`; the pool
comes from `lms ps` capped by `GOOSE_SWARM_MAX_NODES` keeping the fastest by `speed_weights`
(swarm.rs:35016). Arm S = Benchmark view with nodes=1 → workhorse. No config edit, no fleet change.

Between runs, in this order: review the design under fresh eyes → implement what lands before r3 (batch by
file, isolation-tested) → the queued engine item below → rebuild → install → r3. The "all vs single"
control (the same engine on ONE local node, same seed and scorer) is part of that plan.

The desktop streaming thread that used to live here (S1 incremental reads, S2 freshest line, S3 log
folding) SHIPPED and was verified live on 2026-08-29; its fixes are in the table below.

## r2 IS LIVE — launched 2026-08-29 20:42:59 from the Benchmark view, engine `a80c1fa98`

**Different from r1:** REVIEW is ONE round with SYNTHESIS's measured flags injected as MUST-FIX
(`5173eab67`); the judge-probe branch flushes both durable transcripts (`c3b211582`); the desktop's live
line follows the channel that advanced last and the second-lane rows open the inspector (`2dd046553`,
`3ecdbed9d`). Everything r1 carried (process-group kill, phantom-free gate, first-source attribution,
contract block, REPAIR under benchmark) is in.

**Claims r2 settles, in order:** (1) ✅ SETTLED 22:08 — REVIEW ran ONE round, 7 min, 9 new → 10 touches, patch replace 4 / remove 6, sharing 0 / owning-nothing 0, then CONTRACTS. (Was:) REVIEW ends after ONE round with a patch that fixes the measured flags
— `review_findings round:1` then `plan_loaded`, no round 2; (2) ✅ BUILD REACHED 22:14 (CONTRACTS 6 min, plan_loaded=1, plan_patched=1), ✅ BUILD DONE 10/10, 0 failures, INTEGRATE started 23:22 (BUILD 68 min, ~37 of them the sink waiting on replanner test tasks) — and the run COMPLETES:
`complete_result` → `run_finished` → heartbeat `EXITED:`, orphans 0; (3) REPAIR round 0 runs
(`fix_criticals yes`, `complete_fix_dispatched` with `app/sync.py` and `app/ledgerd.py` shards) and
round 1 answers no; (4) the score: does `sync_completeness` close and `GET /` serve? (5) the durable
logs keep pace with the digests under judge looks (`tick_ui` "EXCEEDS the durable log" must stay quiet).

**r2's OWN fixture seed is `5cd47b42e2a7c3e0`** (header of `runs/build/trace-swarm-3node-r0.jsonl`, mtime
21:10 — the harness overwrote r0's trace because the run dir name is reused; r0 was `687ff58bfa6b707d`).
**THE ASSESSMENT IS PRE-ARMED:** when `run_finished` lands — (1) `score_run.sh "<run dir>"`; (2) launch the
workflow at `scratchpad/r2-assessment-workflow.js` (Workflow scriptPath, args {runDir, verdictJson,
wallMinutes}) — five lenses → the r3 BUILD QUEUE with build/refuse verdicts (the deletions decided there,
full autonomy) → two refuters; (3) refusals into REFUSED.md; (4) execute the queue batch-by-file with
adversarial review per change; (5) rebuild, install, launch r3, run NOW.md's live checks first tick.

Score r2 with ONE command once `run_finished` lands: `~/goose-builds/loop-state/score_run.sh "<run dir>"` —
it refuses while the run is live or a server leaks a port, waits for run_build.py, stops only the vendor,
clones the tree, scores under the playwright node at 8850 with the run's OWN seed (`5cd47b42e2a7c3e0`, from
the ledger row) and prints the cloud comparison. The ledger row carries `fixture_seed` so the next overwrite
cannot lose it.

**Kill checkpoints armed:** a second REVIEW round; a plan re-emission; a clock stop; WEDGED (heartbeat
fresh, no event ≥10m, fleet idle, 0% CPU); a leaked server count > 0.

## FIXED TODAY (2026-08-29) — all in the r2 binary `a80c1fa98`

| commit | what | proof |
|---|---|---|
| `a1324c68e` | REPAIR runs under benchmark (`proxy_yes`) | `benchmark_grants_exactly_one_repair_round` |
| `44b2ad6cd` | exit hang: process groups + drain on group liveness at six spawn sites | two tests; `swarm gate` replay: old binary leaked 2, new 0; r1 ran 98 min with 0 orphans |
| `133bf3bec` | contract block: a stub is a signature; read the real file | `the_contract_block_licenses_a_targeted_read_of_the_real_file` |
| `0d5ac740d` | no phantom endpoints in the gate (backtick cell, vendor prose, notifierd rows) | real-spec test; r0 tree 10 → 4 findings, real `GET /` appears |
| `d748a7d3e` | first source path wins in attribution | `an_authored_finding_shards_to_the_first_file…` |
| `c3b211582` | judge-probe branch appends both transcripts | measured lag 155 s → the branch flushes |
| `5173eab67` | REVIEW is one round, MUST-FIX flags injected | r1 8→4→9; three new tests |
| `3ecdbed9d` `2dd046553` | UI: named fields, reasons on disabled, also-row buttons, live line follows the freshest channel | 13 + 6 tests red→green; verified live 20:50 |
| `70486d959` `3dcf528bb` `af9dc6f3d` `b7ae6f4e1` `93280cc9c` | scorer/harness gates: string bodies, `--preflight`, `--seed` required, held-port probe, exit reaping | tests; r0 rescored 0.0568 comparable |

**r0 = 0.0568 comparable** (seed 687ff58bfa6b707d, port 8850, playwright node; two criticals: `GET /` 404
and `items`-vs-`data`). **r1** (18:43–20:23) killed at REVIEW round 4 — see RUN-LEDGER, EXPERIMENTS-LEDGER
and the campaign skill changelog for the full record.

**LANDED for r3 (desktop, Mihai 22:35): every engine phase is its own chip** — `ac9715d24`: the ribbon draws
10 steps in engine order (Open, Ask, Research, Synthesize, Review, Contracts, Build, Integrate, Repair, Done);
the engine's `ask`/`contracts` events no longer map onto Open/Build; contract-* lanes file under CONTRACTS;
r0 fixture test. Live check in the rebuilt app: Contracts chip active with the node chips under it.

**LANDED for r3 (desktop + engine, Mihai 22:29): tool calls show in the inspector WHILE running.** Engine
`156a95957` writes `inflight: [{id, tool, args, since}]` into the digest at the request moment; panel `26612c1a3`
renders RUNNING rows (amber pill, spinner, ticking seconds, args preview) above finished ones, dedupes by id when
the result lands, and the fleet cell's live line reads `running: write app/… (…)` during a call — all through
`digestStreamFields()`. Limit: arguments appear when the request is complete, not token by token (no partial
tool-call deltas in this branch's provider layer).

**LIVE CHECKS for the first tick of the rebuilt app (r3), over CDP, before trusting any of it:** (1) ribbon shows
10 chips Open · Ask · Research · Synthesize · Review · Contracts · Build · Integrate · Repair · Done, Contracts
active during CONTRACTS with the node chips under it; (2) a RUNNING row appears in the inspector the moment a
write/shell starts, caption "N tool calls · k ok · 1 running", no duplicate when it lands; (2b) a retried
lane shows the SAID chips (attempt N · live / superseded / error→retried) and "processing the prompt…", the
archived r0 run renders chipless, new runs' <task>.log files begin with an attempt-marker line; (3) on `#/benchmark` with
the run live, Cmd+N / Cmd+W / Cmd+Q / Cmd+T / Cmd+, each show the warning toast and do nothing, mouse clicks on the
same menu items still act; Cmd+R / Cmd+Shift+R / Cmd+Alt+I do nothing and the View menu shows no Reload items;
without a run, Cmd+N opens a sibling window on the focused directory and Cmd+W / Cmd+Q behave as before. Token-level argument streaming
is out of scope on this branch (no partial tool-call deltas in the provider layer).

**LANDED for r3 (desktop, Mihai 22:00): the app no longer answers stray shortcuts** — `959ab7ebb` (the
renderer Cmd+N keydown that matched Cmd+Shift+N is gone), `3ea9495d7` (Reload / Force Reload / DevTools off in
the packaged app), `82a6d1708` (with a run live: Cmd+N, Cmd+W, Cmd+Q, Cmd+T, Cmd+, from an accelerator are
refused with an in-app toast; mouse clicks still act). 995 vitest green. Live check after the rebuild —
nothing was verified on the running r2 app.

**NO THROWAWAY WORK (Mihai, 22:20: "if something gets deleted and redone what's the point of doing it to
begin with?").** Nothing gets built for a layer the design deletes — no config fields, no gates, no
levers rows for pre-review / tail idle-fill / idle-model judge / prereview dims. For r3 they are switched
off by four `=0` lines in the Benchmark view's spawn env (main.ts `benchmark-run`, beside
`GOOSE_SWARM_BENCHMARK`), which die with step 11; the tick proves the state by the absence of their events.
The step-10 config-plumbing agent was STOPPED before it changed a line. Rule for every step from here:
implement what the design KEEPS or ADDS; for what it DELETES, delete or leave — never plumb.

**QUEUED for r3 (desktop, Mihai 22:00): the app must not answer stray shortcuts** — Cmd+Shift+N opened a
second window on the Benchmark view mid-run. Enumerate every combo (menu accelerators, globalShortcut,
renderer keydown, Electron default roles), remove the spawn/reload/close ones, guard the rest while a
benchmark is live; verify over CDP in the rebuilt app after r2. Agenda item stamped.

**QUEUED for r3, behind the step-1 agent (same file, so not concurrent) — design step 10 measured 21:50:**
four supervision layers are ENV-ONLY and ON by default, so a packaged app cannot switch them off and
`levers_resolved` cannot prove their state: `GOOSE_SWARM_PREREVIEW` (swarm.rs:36724),
`GOOSE_SWARM_TAIL_REVIEW` (scheduler.rs:59 — the LIVE idle-fill; `sink_review` is the dead one),
`GOOSE_SWARM_JUDGE` (:36712, the idle-model judge, not the omni), `GOOSE_SWARM_PREREVIEW_DIMS` (:27190).
Each needs a `SwarmConfig` field + a `levers_resolved` row. The rest switch off in one command:
`arm_config.py --set omni_judge=false dynamic_replan=false incremental_replan=false goals=false sink_review=false supervision_pool=false retarget=false benchmark=true`.

**LANDED during r2, ships in r3 (`2b1e755ac`):** a nudge escalates to a seeded re-stream (conversation wiped in the same session, `judge_restream` event) when a prior steer produced no action or the judge says RESTART — `nudge_delivery()` is pure and tested. r2's opener needed 22 min and five ignored steers to finish; r1's review lane six. First `judge_restream` event in r3 is the live check. Also for r3: `afa644ddd` — the Benchmark view passes `GOOSE_PROVIDER_READ_TIMEOUT_SECS=1800` (a live PARALLEL:2 slot went 581 s silent on r2; the default 600 s would have cut it).

**Was queued (now done):** when the judge answers RESTART on a call with
`actions_since_last_look == 0` and the previous steer changed nothing (thinking grew, no action), deliver the
re-stream — a steer cannot be obeyed by a call that never reaches a turn boundary. r1 measured six steers
ignored on one looping review lane (`judge_out_of_moves` is the greppable state).


## THE INSTRUMENTS — all of them, current 2026-08-29

```bash
python3 ~/goose-builds/loop-state/tick.py            # backend: phase, ETA vs LOCAL clock, per-lane DELTAS,
                                                     # SPEC VOLUME + reasoning:answer ratio, the CLAIMS UNDER
                                                     # TEST, install drift AND process-vs-bundle zombie check
node ~/goose-builds/loop-state/tick_ui.mjs           # frontend: realtime, graphical issues, waste, UX
node ~/goose-builds/loop-state/tick_ui_click.mjs     # frontend: DRIVES the controls — opens a node, closes it
python3 ~/goose-builds/loop-state/snapshot_run.py    # → RUN-LEDGER.md (the tick runs this automatically)
python3 ~/goose-builds/loop-state/compare_vs_cloud.py <our-verdict.json>   # the THREE-COLUMN comparison
~/goose-builds/loop-state/note.sh <kind> "finding"   # → TICK-NOTES.md
node ~/goose-builds/loop-state/live_s1_check.mjs <dir>  # proves the panel's data path IN THE RUNNING APP
~/goose-builds/loop-state/review_diff.sh             # invariant check over a large multi-agent diff
~/goose-builds/loop-state/stop_local_run.sh 9897     # MUST exit 0 — gates on `lms ps`
```

**Scoring a tree directly** (needed whenever the engine hangs on exit):
```bash
python3 evals/swarm-bench/bench/score_sb7.py --tree <run-dir> --json-out verdict.json
python3 ~/goose-builds/loop-state/compare_vs_cloud.py verdict.json
```

**The target's scorecard, which reframes everything:** the 20.06% run had **inner 0.7662** — a good app —
and finished at 22.07% because **three criticals multiplied it by 0.288**. The lever is not "write more
code"; it is "stop tripping criticals". Our engine covers all three of its classes: `domain-conventions`
(dst/money), `wiring` (dead primary flow), and the in-run restart-durability check at `swarm.rs:20653`.

## THE HARD RULES (the user's, non-negotiable)

1. **Isolation first, always.** Every fix proven without a run — archived-tree replay, a pure-function
   test, a temp-dir test — and only then ONE holistic run. *"As it is you're just killing my machines
   for nothing."*
2. **The mandatory tick recipe** — every tick, no exceptions:
   - **Backend:** logs, progress, current phase, **ETA to completion vs the current time**, improvements
     identified and logged, **skill updated**.
   - **Frontend:** realtime streaming check, graphical issues, graphical waste, improper UX, improvements.
   - **At end of run:** implement all fixes → test each in isolation → start the run → verify holistically.
3. **Notes every tick, latest only.** Findings accumulate in `TICK-NOTES.md`; the tick prints only the
   newest three, so nothing has to be re-derived when asked "what have you found".
4. **NO CAPS on models or runs.** *"Otherwise they might get blocked because of that."*
5. **Never reconfigure the fleet** on my own initiative.
6. **The OpenRouter key** lives at `~/.agents/skills/goose-benchmark-iteration/secrets/cloud-providers.env`
   (mode 0600, in no git repo). Never in skill markdown, never in a commit.
7. **Short chat reports.** What I am doing now, what I am waiting on. Detail goes in commits and files.
8. **Commit every change as it lands**, so anything bad is revertible.

---

## THE INSTRUMENTS (run these, do not re-derive them)

```bash
python3 ~/goose-builds/loop-state/tick.py          # backend: phase, ETA, deltas, judge, fleet, cloud, notes
node    ~/goose-builds/loop-state/tick_ui.mjs      # frontend: CDP on 9897 — route, realtime deltas, defects, waste
~/goose-builds/loop-state/note.sh <kind> "finding" # append to TICK-NOTES.md
~/goose-builds/loop-state/kill_scoped.sh           # refuses any pattern that is not an absolute root path
```

`tick.py` liveness is **engine truth** — `.swarm/heartbeat` content plus a running process — not the
directory name. The name blacklist was one marker behind twice (`-ENDED-`, then `-STOPPED-`), and the
second time it reported a run dead for hours as live, with an ETA.

## THE TICK, both halves, every 10 minutes

```bash
python3 ~/goose-builds/loop-state/tick.py           # backend: phase, ETA vs the LOCAL clock, per-lane
                                                    # DELTAS, spec volume, the claims under test, install drift
node ~/goose-builds/loop-state/tick_ui.mjs          # frontend: realtime, graphical issues, waste, UX
node ~/goose-builds/loop-state/tick_ui_click.mjs    # frontend: DRIVES the controls — opens a node, closes it
~/goose-builds/loop-state/note.sh <kind> "finding"  # append to TICK-NOTES.md
python3 ~/goose-builds/loop-state/snapshot_run.py   # run into RUN-LEDGER.md (the tick does this automatically)
```

**At end of run: implement every fix, test each in ISOLATION, then start the run and verify holistically.**

**Kill checkpoints are deliberately narrow — slowness is NOT a kill.** A long phase, idle nodes while a
fanned straggler finishes, and an outstanding judge probe have each caused a WRONG kill. Kill only on a
proven wedge — no new event AND no digest mtime movement, sampled ≥3 times over ≥90s, AND `lms ps` idle
— or a named-field defect from the table in `SWARM-AGENDA.md`.

## HOW TO ISOLATION-TEST

Run from the repo root, and **never as a bare `goose`**: on this machine `which goose` is
`~/.local/bin/goose`, a June build that answers `error: unrecognized subcommand 'swarm'`. The repo
binary is the only one with the verifier, and it is only as current as the last release build.

```bash
cargo build --release -p goose-cli                           # the verifier is only as new as this
# NOT bare `goose`: `which goose` is ~/.local/bin/goose, version 1.38.0 from JUNE, with NO swarm
# subcommand at all. The documented command was unrunnable as written. Use the built binary by path.
./target/release/goose swarm verify <tree> --owns <files>    # ~30 archived trees + positive control
cd ui/desktop && pnpm test                   # UI pure functions (inspectorThinkingText/inspectorOutputText)
cargo test -p goose-swarm && cargo test -p goose-cli swarm
```

## HOW TO LAUNCH (when a run is authorised)

Through the desktop **Benchmark view**, never a chat. `launch.sh` SIGKILLs every stray Goose — a
graceful kill makes the app rewrite `config.yaml` and drop the whole `swarm:` block — refuses to launch
over a live run using engine truth rather than the DOM, writes the levers into `config.yaml`
(`open -n` gives the app no environment, so config not env), and snapshots that config into the run dir.
