# The panel does not stream. It polls. — design, 2026-08-29

Mihai, after ten weeks: *"why are the UI elements I keep asking for still not streaming in realtime?
Have you even researched how that can be done correctly?"*

No, I had not. Here is the research, then the design.

## What exists today, measured

    renderer   useSwarmRun polls window.electron.readSwarmRun() every 500ms
    main       'read-swarm-run' RE-READS THE WHOLE RUN DIRECTORY on every call
    engine     writes .swarm/activity/<task>.json, throttled to 400ms

    push channels from main to renderer for run data:  ZERO
    (`grep webContents.send | grep -iE 'swarm|bench|activity|lane'` returns nothing)

**Per poll, twice a second, for a 9-lane run:**

    9   activity JSON files re-read and re-parsed in full
    68 KB  run.jsonl re-read and re-parsed from byte 0
    up to 200KB + 400KB  transcript tails re-read PER LANE

`run.jsonl`, `<task>.log` and `<task>.think.log` are **APPEND-ONLY**. Re-reading them from the start
twice a second is pure waste, and it is also why the panel feels heavy rather than live.

## Why it will never feel realtime as built

1. **It is a pull, not a push.** Best case the UI is 500ms stale; there is no path for the engine to say
   "this changed now".
2. **The unit of transfer is a whole snapshot**, so the renderer re-folds every event on every tick.
3. **Any gap in engine writes reads as a stall**, because the panel only knows what the last snapshot said.
   The judge-probe freeze (`swarm.rs:17598`, events buffered raw during a probe) is exactly this: the file
   genuinely stops changing and the panel has no way to say why.

## The design

**S1. Append-only reads from a byte offset.** main keeps `{path -> {size, mtimeMs}}` per run. For
`run.jsonl` and both transcripts it reads ONLY the bytes past the last offset and returns them as a
delta. A truncated/rotated file (size < offset) resets to 0. This alone removes ~99% of the I/O.

**S2. Push, do not poll.** main `fs.watch`es `.swarm/` and `.swarm/activity/`, debounced ~100ms, and
sends `swarm:delta` over `webContents.send`. The renderer applies the delta to state it already holds.
The 500ms poll stays as a SAFETY NET only — `fs.watch` is unreliable on some filesystems and a missed
event must not freeze the panel forever.

**S3. The renderer folds incrementally.** `foldEvents` currently rebuilds from the full event array each
tick. It takes the accumulated state plus new events. This is required for S2 to be worth anything.

**S4. Say why a lane is quiet.** Already shipped: `judging` + `queued_chunks` on the digest, rendered as
"supervisor reading · N chunks queued". A frozen counter with `judging:false` is still a dead lane, and
that distinction must survive — `main.ts` folds the freshest activity mtime into the run mtime so a
killed run goes stale, so **no unconditional touch, ever.**

## Order, and why

    1. S1  incremental reads      pure win, no behaviour change, testable in isolation with a temp dir
    2. S4  quiet-reason           already done; verify in isolation
    3. S3  incremental fold       prerequisite for S2, testable as a pure function
    4. S2  fs.watch push          last, because it is the only piece that can DROP an update

## Testing rule — Mihai's, and it is now the rule

**Every fix is proven in ISOLATION before any run starts, and only then under a full run.**

    isolation for engine checks   `./target/release/goose swarm verify <tree>` against ~30 archived trees + positive control
                                  NEVER a bare `goose` -- that resolves to a June build with no `swarm` subcommand
    isolation for UI folding      pure-function tests (inspectorThinkingText / inspectorOutputText shape)
    isolation for main-process    a temp dir, files appended by the test, deltas asserted
    holistic                      ONE run, after all of the above pass

No run is started to find out whether something works.
