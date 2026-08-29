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

## NEVER DECLARE A COMPONENT INSIDE ANOTHER COMPONENT

This cost SIX rounds of the same complaint. `Pane` was declared inside `NodeInspector`'s function body,
so every render produced a NEW component identity and React unmounted and remounted the whole subtree —
**at the 500ms poll**. Scroll position destroyed twice a second, follow state reset, `scrollTop =
scrollHeight` re-run each time. The user: *"you can't even scroll it, it jumps down."*

Five attempts went into the TEXT pipeline, where the bug could never be reached. If a pane will not hold
a scroll position, look for a component defined inside a render body BEFORE looking at what it renders.

## A HEADER MUST COUNT WHAT THE BODY SHOWS

The OUTPUT pane's header said "42 tool calls" while its body rendered six strings that were all literally
`"shell ok"`. Every one of those calls carried `summary` (the command) and `result` (its output) in the
digest, unrendered — reproduced across four archived runs, where lanes with 9-17 calls carry a `last_text`
of ONE character. This window has now lied about its own contents twice; `workCaption` exists so every
number is over the rows on screen and a slid window says "last N of M".

## `<task>.log` IS RAW STREAM DELTAS, NOT AUTHORED TEXT

It is `texts[already..].join("")`, so blank runs are a chunking artifact — measured at 86% blank lines
with runs of 13. `squeezeBlankRuns` collapses them and trims TRAILING blanks specifically, because follow
scrolls to the end and landing the viewport on 13 blank lines is the other half of "it does not show what
is being generated".

## Verify in the RUNNING app, never only in vitest

Two committed UI changes were dead on arrival. `strings app.asar` cannot settle it either — the bundler
minifies function names. Use `~/goose-builds/loop-state/tick_ui_click.mjs` (drives the real controls) and
`live_s1_check.mjs` (drives the real IPC). Check the app is not a zombie first: `tick.py` compares each
process's start time against the bundle mtime.
