---
paths:
  - "ui/desktop/src/utils/swarmIncrementalRead.ts"
  - "ui/desktop/src/utils/swarmWatch.ts"
  - "ui/desktop/src/main.ts"
---

# The panel's data path — incremental reads and the push channel

`run.jsonl`, `<task>.log` and `<task>.think.log` are APPEND-ONLY. Reading them from byte 0 twice a second
was ~99% waste and is why the panel felt heavy.

## Cache identity is inode + birthtime, NEVER path + size

A run log REPLACED at the same path by a LONGER one grows, which is indistinguishable from an append if
size is all you compare. Proven: `OLDRUN-AAAA` rewritten to `NEWRUN-BBBBBBBBBBBBBBBB` came back as
`OLDRUN-AAAABBBBBBBBBBBB`, and `readEvents` returned the previous run's events wholesale. The bench
harness replaces `run.jsonl` run over run, so this is a live path, not a hypothetical.

## `readEvents` must stay serialised per path

Two overlapping 500ms polls raced the same offset, parsed the overlap twice, and **permanently lost** a
`task_dispatched` when the overlap landed on a partial line. The `inFlight` map chains them. The renderer
polls with `setInterval` and never awaits the previous tick, and the hook is mounted at four sites, so
the overlap is unconditional rather than an edge case.

## The push watches the EVENT LOG ONLY — never `activity/`

The engine rewrites `activity/<task>.json` ~2.5×/sec PER LANE. Watching it produced ~10 deltas/sec
against the 2/sec poll it was meant to improve — five times the work — and digests are rewritten in
place, so the incremental reader cannot make those reads cheap. Push on the append-only log; leave the
digests to the poll.

## The watch registry is keyed by SUBSCRIPTION, not by window

One renderer mounts several `useSwarmRun` hooks on different working dirs. Keying on `webContents.id`
alone made them overwrite each other's target. Key is `${sender.id}::${workingDir}`.

An errored watcher must be REMOVED from the map, not merely closed — `arm()` skips any directory already
present, so a closed-but-present handle is never re-created.
