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

**The unfalsified claim:** the swarm's whole thesis is that parallel BUILD beats serial build, and
**we have never reached BUILD**. Until a run does, no mechanism here is proven either way.

---

## WHAT WE ARE DOING RIGHT NOW

**One sentence: real-time streaming in the desktop panel — the panel has never actually streamed, it
polls, and that is the user's longest-standing complaint. All four steps below are now CODE COMPLETE in
the working tree; what is left is committing them and WATCHING THE PANEL STREAM IN THE RUNNING APP,
because compiling and passing tests is not evidence that a UI change works.**

Everything below is under a hard rule the user set: **no run starts until each fix is proven in
ISOLATION; only then one holistic run.** Runs cost 3-4 hours and were being spent to discover things a
2-minute test could have shown.

### The realtime thread — `DESIGN-REALTIME-UI.md` — **COMPLETE**

All four steps landed and are verified in the RUNNING app, not just in tests.

| step | what it is | state |
|---|---|---|
| **S1** | read the run log and transcripts from a byte offset | **done** — `utils/swarmIncrementalRead.ts`; identity is inode+birthtime (a same-path replacement spliced the old file's head onto the new one's tail); reads are serialised per path with `inFlight` (overlapping 500ms polls raced the offset and *permanently lost* a `task_dispatched`); LRU-bounded at 64 paths |
| **S3** | `foldEvents` folds only what was appended | **done** — `foldEventsIncremental`, keyed on `(runId, generation)` issued by main, not a content fingerprint |
| **S4** | say why a lane is quiet | **done** — and simplified: the engine no longer buffers during a probe, so counters keep moving and `queued_chunks` was deleted on both sides |
| **S2** | `fs.watch` push instead of waiting for the poll | **done** — `utils/swarmWatch.ts` → `main.ts` `sender.send('swarm:delta')` → `preload` `onSwarmDelta` → renderer; the 500ms poll stays as the safety net |

**S2 watches the EVENT LOG ONLY, never `activity/`.** The engine rewrites `activity/<task>.json` ~2.5×/s
per lane; watching it produced ~10 deltas/s against a 2/s poll, and digests are rewritten in place so the
incremental reader cannot make those reads cheap. Push on the append-only log; poll the digests.

Verified live over CDP against a growing run: events 3→4, thinking bytes 25→50, `fullThinking` holding
**both** chunks rather than rolling, `generation` correctly stable across an append.

### The other live threads

- **The judge as a NUDGER, not a terminator.** The user: *"it looks more harming than anything else…
  I was hoping the judge is not only a terminator but rather a NUDGER of good quality."* **Run 4,
  measured 2026-08-29 09:05 EEST: 211 looks dispatched, 38 nudges, 222 node-min judging = 46% of the
  fleet watching rather than working — and delivery was STEER, zero re-streams.** The destructive
  re-stream is FIXED, not current: `let can_steer = pending.is_empty();` (`swarm.rs:18032`) makes steer
  the default and re-streams only while a tool request is in flight. The "141 looks, 13 nudges, every
  one a re-stream" reading is **2026-08-28, the BEFORE picture** — quoting it as current states the
  engine's behaviour exactly backwards. What is still negative is OVER-steering: of 34 nudges with a
  follow-up look, 33 produced no action, costing **66 minutes of WORKER time**. Fuller ledger, and the
  copy to trust: `SWARM-AGENDA.md` :568 and :599. Snapshot counts of the same run differ by the minute
  they were taken (211/38 at 09:05, 214/40 later), so cite the timestamp; never reconcile them. Open: the
  judge running **outside the phase machinery**, checking files and plans **as they are created**, using
  idle nodes — *"workers follow phases, judges should live outside of this and run constant checks."*
- **The complacency audit.** A workflow found ~20 confirmed places where a fix was applied at one call
  site when several existed. Most are **not yet applied**.
- **Agenda item AD** — REVIEW's no-new-finding stop is defeated by a reviewer that merely rephrases;
  de-dup must key on the structural CLAIM, not the sentence.
- **Agenda item D** — 41 never-used functions measured; deletion deferred while workflows hold line
  numbers in `swarm.rs`.

---

## r1 IS LIVE — launched 2026-08-29 18:43:52 from the Benchmark view, engine `d748a7d3e`

**What r1 carries that r0 did not:** REPAIR round 0 under benchmark (`a1324c68e`); the exit-hang fix —
process-group kill + drain on group liveness at six spawn sites (`44b2ad6cd`), proven by two unit tests
and by the `swarm gate` replay on r0's tree (old binary leaked 2 servers, new leaks 0); the contract
block that tells workers a stub is a signature and licenses a targeted read of the real file
(`133bf3bec`); no phantom endpoints in the deterministic gate (`0d5ac740d`: r0's gate findings on its
own tree went 10 → 4 and the REAL `GET /` 404 finally appears); first-source-path attribution so
"Frontend not served (in `app/ledgerd.py`, `web/index.html`)" shards to ledgerd (`d748a7d3e`).

**Claims r1 settles, in order of the score they gate:**
1. Does the run COMPLETE — `complete_result` then `run_finished`, heartbeat `EXITED:`, orphans 0?
2. Does REPAIR run — `fix_criticals.answer == "yes"` at round 0, `phase: fix`, `complete_fix_dispatched`
   with shards including `app/sync.py` and `app/ledgerd.py`; round 1 answers "no" and the loop ends?
3. Does the wave close `sync_completeness` (the `items`-vs-`data` key) and serve `GET /`? Those two
   criticals are the whole distance between 0.0568 and the target.
4. Do BUILD workers READ their dependencies now (tool calls > 0 on dependents like boot-wrapper)?
5. Judge: drift corroborates (`judge_drift_held.drift_streak`), REVIEW stops on a no-new-finding round.

**First-tick checks:** `~/goose-builds/loop-state/first_tick_r1.sh d748a7d3e`. **Scoring after:**
seed from `runs/build/trace-swarm-3node-r0.jsonl` header, port 8850, `GOOSE_SWARM_RENDER_NODE` = nvm v22,
orphans 0 first, then `compare_vs_cloud.py`.

## WHERE THINGS STAND — r0 scored, the hang is root-caused, r1 is being prepared (2026-08-29 ~17:55)

**r0 = 0.0568 hermetic** (inner 0.1789 × crit_mult 0.36; seed `687ff58bfa6b707d`, vendor port 8850,
playwright node). 28% of the 20.06% target, 2.1× the published 0.0273. Two criticals fired and between
them they zero almost the whole scorecard:

1. **`GET /` → 404.** The frontend EXISTS (`web/index.html`, `app.js` 25KB, `viz.js` 13KB) and ledgerd
   does not serve it, so every J/V/P/T/E check is 0 and `j_loads_data` + `j_workflow_journey` trip.
2. **`sync.py` reads `body.get("items")`; the vendor sends `"data"`.** 0/12288 payments, so B/C/X/R are
   vacuous and `sync_completeness` trips.

Both are one-line-class fixes in `app/ledgerd.py` and `app/sync.py`. r1's REPAIR round 0 (now reachable
under benchmark, `a1324c68e`) is aimed at exactly these. Verdicts live in the run dir:
`verdict-hermetic-seed687ff58b-port8850-0.0568.json`. An earlier 0.0832 was BLIND (hermit node has no
playwright → 30/99 checks unavailable) and on a FRESH seed — retracted, kept under a name that says so.

**The scorer gained three refuse-gates today** (`_error_obj`, `--preflight`, `--seed` required, plus a
held-port probe shared with run_build), so none of those wrong-number mechanisms can recur silently.

### THE TWO BUGS r0 FOUND — both root-caused, one committed, one landing

1. **`swarm.rs:37015` — REPAIR never ran in any benchmark.** FIXED `a1324c68e`.
2. **The exit hang — ROOT-CAUSED, corroborated by three adversarial refuters and one independent
   agent.** `boot_invocation` (`swarm.rs:~18934`) spawns `python3 -m app` with piped stdout/stderr,
   polls 4s, `kill()`s the direct child, then awaits the pipe tails to EOF. The wrapper's `Popen`
   grandchildren (`ledgerd`, `notifierd`) inherit the pipes and survive the single-pid SIGKILL, so EOF
   never comes: 0% CPU, heartbeat alive, fleet idle. **Proof:** two orphans with cwd in the run dir,
   PPID 1, started at the exact second of `fix_criticals`. `main.rs:36` joins a big-stack thread for the
   whole run, so `_pthread_join` on the main thread was never a finding. **The leak is systemic:** 41
   orphaned app servers were alive on this machine, 25 from r0 alone — every boot probe / smoke that
   killed a wrapper leaked its grandchildren, and `boot_probe` refuses to conclude on a pre-bound port.
   **FIXED `44b2ad6cd`**: every app spawn leads its own process group; `kill_app_tree` SIGKILLs the
   group; the pipe drain releases on GROUP liveness, not EOF. Applied at six spawn sites. Two regression
   tests (`boot_invocation_returns_when_a_grandchild_holds_the_pipe`, `..._escaped_the_group`);
   609 lib tests, clippy clean. Isolation proof in flight: `goose swarm verify` over r0's tree hangs on
   the old binary and must return on the new one.

### NEXT swarm.rs BATCH (after the hang commit; one agent, one file)

- **Phantom endpoints in the deterministic gate** — 5 of r0's 29 "criticals" no app change can close:
  table row ``| `GET` | `/` + `web/*` |`` yields `` /` ``; the prose regex scrapes the VENDOR's
  `GET /v3/reversals` (spec line 86); the `notifierd` table (spec §6, rows 350-353) is probed on
  ledgerd's port. RATE then folded the REAL `GET /` defect into a phantom duplicate.
- **First source path wins in `extract_file_from_finding`** — D5 "Frontend not served (in
  `app/ledgerd.py`, `web/index.html`)" shards to `web/index.html`; the fix lives in ledgerd.
- ~~`render_node` config key~~ and ~~build sha in `run_started`~~ — REFUTED by r0's own events: `spec_contract.render_gate = "ran (rows=0, console_errors=2)"` with screenshots, and `levers_resolved.build_sha` already exists. The desktop's `resolveBenchNode` ladder does the job.

### THE BIGGEST NON-BUG FINDING

**Workers get their dependency's SIGNATURE and never its BEHAVIOUR.** CONTRACTS freezes a stub; the
read-the-real-file instruction (`swarm.rs:25603`) fires only when a stub fails to PARSE. Measured:
`boot-wrapper` depends on two services and made **0 tool calls, 0 reads**. r0's own defects prove the
cost — `sync.py` reads `body.get("items")` where the vendor returns `"data"`, and parses `amount` where
it sends `amount_minor`. The real files were finished on disk when those workers ran.

## NEXT RUN (r1) — what is queued, and what it will settle

### FIXES COMMITTED BUT NOT IN THE RUNNING BINARY (installed build is 13:06)

| commit | fix | why it matters |
|---|---|---|
| `02c78cae3` | **drift corroborates instead of being suppressed forever** | r0: 5 DRIFTING verdicts → 1 nudge. `open-coverage-2` hit 21,749 reasoning chars with ZERO tool calls, was diagnosed DRIFTING, and held — "producing" counts reasoning, so a call that reasons and never acts is producing by definition |
| `8f883757b` | **the thinking path takes the freshest LINE** | I fixed the transcript branch and left this one. OPEN/RESEARCH are pure reasoning, so EVERY lane in them fell through to the broken branch — the tick caught it on `workhorse (slice-boot-wrapper)` |
| `6ba042ce3` | **the strip shows a node's second lane** | nodes are PARALLEL:2; `open-coverage-1` (68,393 chars) and `open-coverage-2` had **no cell at all** |
| `aa8e7d90d` | inspector single-column when Output is empty; `YOUR FLEET` badge stops overprinting the row label at low scores | both from Mihai's screenshots |
| `95f5b7d4e` | **panel defaults generated from Rust, not retyped** | `worker_max_turns` was 40 in the panel against 1,000,000 in Rust, under a test titled "the panel can never write a divergent value" |

### CLAIMS r1 MUST SETTLE

1. **Does the drift fix produce nudges?** r0: 5 drifting → 1 nudge. r1 should show drift delivered on a
   second look with no action taken. Read `judge_drift_held.drift_streak`.
2. **Does the strip show every live lane?** r0 hid the two largest lanes in the run.
3. **Does REVIEW still stop on a no-new-finding round?** r0: `r1:new=4 → r2:new=0`. Once is not twice.
4. **Does the warden ever fire?** Silent through all of r0 — correct only if no dependency completed
   hollow, which is unproven either way.
5. **Do we trip fewer criticals than the target's three?** This is the whole game: the target scored
   inner **0.7662** and finished at 22% because criticals MULTIPLY. Our engine covers all three of its
   classes (domain-conventions, wiring, restart-durability at `swarm.rs:20653`) — r1 says whether they fire.

### THE MEASURED WASTE TO ATTACK (from r0's ledger row)

- **research 39m ≈ build 49m.** Research costs as much as building.
- **266,614 reasoning chars for 110,095 bytes of program.** A 2.4:1 deliberation cost overall, but
  `review-screen-space-labels` alone ran **38:1** and `synthesis` **11:1**.
- **brief median 4,789** against the ~1,500 that measured 88.7%.
- **45 minutes lost to one task's transport drops** — recovered, but on the record.

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
