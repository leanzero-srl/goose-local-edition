# PHASE-POLISH — the standing phase-inefficiency queue

Mihai, 2026-08-16 22:20: "start something to CONTINUOUSLY polish the phases. I am sure there's
still plenty of inefficiency. I keep asking for it and you keep ignoring it." He was right: every
phase audit before this file was a one-off, run when he pushed, and the polish died with the turn.

MECHANISM: phase_audit.py runs on EVERY scored unit (wired into sweep.py's row assembly). It cuts
the run into phases from its own events, ratchets each phase against the campaign best
(PHASE-BEST.json, auto-updated), and appends the top wall segments + every regression here.
The operator loop's standing protocol: at every unit end, the TOP UNFIXED ITEM in this queue is
the default next-kaizen candidate — it can be out-argued by fresh evidence, never ignored.
Items get struck (~~strikethrough~~ + the finding number that fixed them) when a batch ships.

Seeded from the first two audited runs (r1 old binary / r0-redo F851 binary):
- dead_attempt_node_secs: 3223 / 3904 node-s (~60 node-min per run) — THE standing #1: judge
  kills land at 8-14 min after the evidence existed at minute 1 (F857a routing + earlier drift
  detection are the queued fixes)
- prologue_total: 2277s -> 1375s (diverse_plan skip bought ~16 min — F854 PROVEN by the ratchet's
  own numbers) — remaining 23 min is research 408 + skeleton 428 + detail 348 + contracts + slack:
  S5 pipelined prologue is the structural fix
- repair_phase: 1487 / 1919+s — progress-based rounds + fix-wave early-close are the queued fixes
- dag occupancy 2.2-2.4 of 6 slots
