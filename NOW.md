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

Make the swarm **build better software** on 3 local LM Studio nodes, then beat the published local
result `brun-fleet-qwen38-brainwaves-sb70` (**0.0273**) on leanzero.net. The cloud board leader is a
single-agent deepseek run at **67.53%**. Numbers follow from the product, not the other way round.

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

### The realtime thread — `DESIGN-REALTIME-UI.md`

Measured, not assumed: the renderer polls `readSwarmRun()` every 500ms; main **re-reads the whole run
directory on every call**; push channels from main to renderer for run data: **zero**. Per poll, twice
a second, for a 9-lane run: 9 activity JSONs re-parsed, 68KB `run.jsonl` re-parsed from byte 0, up to
600KB of transcript tails re-read. All three of those files are **append-only**.

| step | what it is | state |
|---|---|---|
| **S1** | read `run.jsonl` and both transcripts from a byte offset, return only the delta | **done** — `swarmIncrementalRead.ts` wired into `main.ts:23` (`eventsGeneration`/`readEvents`/`readTail`), covered by `swarmIncrementalRead.test.ts` + `.replay.test.ts` |
| **S4** | say WHY a lane is quiet (`judging` + `queued_chunks` → "supervisor reading · N chunks queued") | **done**, 14 references in `useSwarmRun.ts` |
| **S3** | `foldEvents` takes accumulated state + new events instead of rebuilding from the full array | **done** — `foldEventsIncremental` (`useSwarmRun.ts:2352`), called at :3463, covered by `foldIncremental.test.ts` |
| **S2** | `fs.watch` on `.swarm/` + `.swarm/activity/`, debounced ~100ms, `webContents.send('swarm:delta')`; the 500ms poll stays as a SAFETY NET only | **code complete, UNCOMMITTED and NOT yet seen working in the app** — `SwarmWatchRegistry` (`main.ts:3204`, sends at :3259), `onSwarmDelta` (`preload.ts:360`), consumed at `useSwarmRun.ts:3627` with the poll kept at :3626; tests `swarmWatch.test.ts` |

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
