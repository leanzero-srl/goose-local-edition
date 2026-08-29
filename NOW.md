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
— `review_findings round:1` then `plan_loaded`, no round 2; (2) ✅ BUILD REACHED 22:14 (CONTRACTS 6 min, plan_loaded=1, plan_patched=1) — and the run COMPLETES:
`complete_result` → `run_finished` → heartbeat `EXITED:`, orphans 0; (3) REPAIR round 0 runs
(`fix_criticals yes`, `complete_fix_dispatched` with `app/sync.py` and `app/ledgerd.py` shards) and
round 1 answers no; (4) the score: does `sync_completeness` close and `GET /` serve? (5) the durable
logs keep pace with the digests under judge looks (`tick_ui` "EXCEEDS the durable log" must stay quiet).

**r2's OWN fixture seed is `5cd47b42e2a7c3e0`** (header of `runs/build/trace-swarm-3node-r0.jsonl`, mtime
21:10 — the harness overwrote r0's trace because the run dir name is reused; r0 was `687ff58bfa6b707d`).
Score r2 with `--seed 5cd47b42e2a7c3e0`, port 8850, nvm node, orphans 0. The ledger row now carries
`fixture_seed` so the next overwrite cannot lose it.

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

**QUEUED for r3 (desktop, Mihai 22:00): the app must not answer stray shortcuts** — Cmd+Shift+N opened a
second window on the Benchmark view mid-run. Enumerate every combo (menu accelerators, globalShortcut,
renderer keydown, Electron default roles), remove the spawn/reload/close ones, guard the rest while a
benchmark is live; verify over CDP in the rebuilt app after r2. Agenda item stamped.

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
