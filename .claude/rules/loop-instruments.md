---
paths:
  - "NOW.md"
  - "SWARM-AGENDA.md"
  - "RUN-LEDGER.md"
  - "EXPERIMENTS-LEDGER.md"
  - "TICK-NOTES.md"
---

# The ledgers — and the mandatory tick

These four files exist because context compaction repeatedly destroyed hard-won knowledge. Keep them
current in the SAME commit as the work they describe; a stale ledger is worse than none, because it
reads authoritative.

| file | answers |
|---|---|
| `NOW.md` | what is the current thread — read FIRST, before the agenda |
| `RUN-LEDGER.md` | how each run actually went, in comparable numbers (written by `snapshot_run.py` every tick) |
| `EXPERIMENTS-LEDGER.md` | what was tried, what it measured, why it is not coming back |
| `SWARM-AGENDA.md` | what is still open |
| `TICK-NOTES.md` | every finding; the tick prints only the newest three |

## The tick, every 10 minutes, BOTH halves

```bash
python3 ~/goose-builds/loop-state/tick.py           # backend: phase, ETA vs local clock, per-lane DELTAS,
                                                    # spec volume, the claims under test, kill checkpoints
node ~/goose-builds/loop-state/tick_ui.mjs          # frontend: realtime, graphical issues, waste, UX
node ~/goose-builds/loop-state/tick_ui_click.mjs    # frontend: DRIVES the controls — opens a node, closes it
~/goose-builds/loop-state/note.sh <kind> "finding"  # append to TICK-NOTES.md
```

**At end of run: implement every fix, test each in ISOLATION, then start the run and verify holistically.**
No run is started to find out whether something works.

## Kill checkpoints are DELIBERATELY NARROW

Slowness is NOT a kill. A phase taking a long time, idle nodes while a fanned straggler finishes, and an
outstanding judge probe have each caused a WRONG kill. Kill only on a proven wedge — no new event AND no
digest mtime movement, sampled ≥3 times over ≥90s, AND `lms ps` idle — or a named-field defect from the
table in `SWARM-AGENDA.md`.
