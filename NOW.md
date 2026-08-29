# NOW — what we are researching and doing, this week

**Read this FIRST, at every tick, before `SWARM-AGENDA.md`.** The agenda is 2,400 lines of campaign
history and it is the record; this file is the *current thread* and it is short on purpose. Compaction
has repeatedly destroyed the answer to "what were we in the middle of", and that is the only question
this file exists to answer.

Rule: when the current thread changes, THIS FILE CHANGES IN THE SAME COMMIT. A stale NOW.md is worse
than none.

---

## ⚠️ FIRST THING AFTER A COMPACTION — RE-MINE, DO NOT RESUME FROM THE SUMMARY

A compaction summary keeps the *shape* of the work and loses the *thread*. Resuming from one has already
sent me onto the wrong task while the real thread sat untouched. So, before the first tool call that
changes anything:

```bash
# 1. the current thread, then the record, then the durable memory
cat ~/Projects/goose/NOW.md
grep -n '^- \[ \]' ~/Projects/goose/SWARM-AGENDA.md          # what is genuinely open
sed -n '1,80p' ~/.agents/skills/goose-swarm-campaign/SKILL.md

# 2. re-mine the RAW transcript for what the summary dropped -- grep it, never read it whole
T=~/.claude/projects/-Users-mihaiperdum-Projects-goose/<session>.jsonl
grep -o '"type":"user".\{0,4000\}' "$T" | tail -40                 # the user's own recent words
grep -oiE '.{0,300}(always|never|must be respected|not joking|stop starting|isolation).{0,300}' "$T" | tail -60
```

3. **State the current thread in one line before continuing.** If it does not match what the summary
   implied, the summary was wrong and the sources win.
4. On a large or contested recovery, fan a workflow across transcript + agenda + skill + code + git in
   parallel and verify every recovered claim against the repo. Do not read serially and hope.

---

## The goal, unchanged

Make the swarm **build better software** on 3 local LM Studio nodes, then beat the published local
result `brun-fleet-qwen38-brainwaves-sb70` (**0.0273**) on leanzero.net. The cloud board leader is a
single-agent deepseek run at **67.53%**. Numbers follow from the product, not the other way round.

**The unfalsified claim:** the swarm's whole thesis is that parallel BUILD beats serial build, and
**we have never reached BUILD**. Until a run does, no mechanism here is proven either way.

---

## WHAT WE ARE DOING RIGHT NOW

**One sentence: implementing real-time streaming in the desktop panel (S1→S3→S2), because the panel has
never actually streamed — it polls — and that is the user's longest-standing complaint.**

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
| **S1** | read `run.jsonl` and both transcripts from a byte offset, return only the delta | **in progress** — `ui/desktop/src/utils/swarmIncrementalRead.ts` written, NOT yet wired into `main.ts`, NOT yet tested |
| **S4** | say WHY a lane is quiet (`judging` + `queued_chunks` → "supervisor reading · N chunks queued") | **done**, 14 references in `useSwarmRun.ts` |
| **S3** | `foldEvents` takes accumulated state + new events instead of rebuilding from the full array | **not started** — prerequisite for S2 |
| **S2** | `fs.watch` on `.swarm/` + `.swarm/activity/`, debounced ~100ms, `webContents.send('swarm:delta')`; the 500ms poll stays as a SAFETY NET only | **not started** — last, because it is the only piece that can DROP an update |

### The other live threads

- **The judge as a NUDGER, not a terminator.** The user: *"it looks more harming than anything else…
  I was hoping the judge is not only a terminator but rather a NUDGER of good quality."* Measured on the
  last full run: **141 looks, 13 nudges, every one a re-stream that discarded the call's work — net
  contribution NEGATIVE.** Two fixes landed (burst-gap rhythm; steer-lands-mid-generation). Open: the
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

```bash
goose swarm verify <tree> --owns <files>     # engine checks against ~30 archived trees + positive control
cd ui/desktop && pnpm test                   # UI pure functions (inspectorThinkingText/inspectorOutputText)
cargo test -p goose-swarm && cargo test -p goose-cli swarm
```

## HOW TO LAUNCH (when a run is authorised)

Through the desktop **Benchmark view**, never a chat. `launch.sh` SIGKILLs every stray Goose — a
graceful kill makes the app rewrite `config.yaml` and drop the whole `swarm:` block — refuses to launch
over a live run using engine truth rather than the DOM, writes the levers into `config.yaml`
(`open -n` gives the app no environment, so config not env), and snapshots that config into the run dir.
