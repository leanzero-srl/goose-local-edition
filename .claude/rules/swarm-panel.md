---
paths:
  - "ui/desktop/src/components/swarm/**"
---

# The swarm panel — where "it doesn't show what's happening" keeps coming from

Mihai's longest-running complaint. Every recurrence has had a mechanism; none was a rendering nicety.

## THERE ARE FIVE LANE-BUILDING PATHS, and the one that wins is the BUILD worker lane

The digest→lane join was copy-pasted into five places and diverged twice. It now lives in ONE function,
`digestStreamFields()`, with all 17 fields. **Never hand-copy a digest field onto a lane.** A path that
spreads this cannot be half-wired; a path that does its own join will be, and the failure is invisible
because the other four paths look fine.

## ROLLING vs DURABLE — the distinction behind "the output rolls"

The digest keeps a small window the engine **rewrites in place**; `<task>.log` and `<task>.think.log` are
append-only and complete. Any surface a person reads must prefer the durable log. Three separate surfaces
were fixed for this at three different times, and the third was found by an instrument, not by reading.

## A ONE-LINE ROW MUST BE HANDED A LINE, NOT A BLOCK

`tailOf(x, 2400)` returns a BLOCK; a single-line row renders its BEGINNING, so the cell shows text from
2,400 characters ago and only moves when the whole block rolls. Use `lastSubstantiveLine`. This was fixed
on the transcript branch and left broken on the thinking branch — and thinking is the branch that
matters, because OPEN and RESEARCH are pure reasoning and every lane in them falls through to it.

## Nodes run PARALLEL: 2 — a node has TWO live lanes

`workingByDevice` is keyed by device and holds one. The rest are in `alsoRunningByDevice` and must be
rendered. Dropping them hid the two LARGEST lanes of a run entirely (68,393 and 45,712 reasoning chars,
no cell at all).

## Say what is TRUE about a count

`thinkingChars` is a per-stream counter that RESETS on a re-stream; it is not a size. Only
`thinkingBytes` is. Captioning a rolling 2,400-char window as "2,400 chars" tells the reader they are
seeing everything.

## Verify in the RUNNING app, never only in vitest

Two committed UI changes were dead on arrival. `strings app.asar` cannot settle it either — the bundler
minifies function names. Use `~/goose-builds/loop-state/tick_ui_click.mjs` (drives the real controls) and
`live_s1_check.mjs` (drives the real IPC). Check the app is not a zombie first: `tick.py` compares each
process's start time against the bundle mtime.
