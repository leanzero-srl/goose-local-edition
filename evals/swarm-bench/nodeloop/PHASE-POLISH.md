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

## Batch F859 strikes (2026-08-16 ~23:20)
- ~~dead_attempt_node_secs (partial)~~ — F857a no_first_write->split ships (the 1037s web-assets
  class); the LLM-latency drift kills (444-983s) remain open — deterministic first-write
  contract probe is the queued design (lowest-confidence hunt item, needs its own evidence).
- ~~repair_phase: flat rounds + serial join + accept-bypass~~ — progress-based rounds, join
  skip/composite, accept exclusion, scaled caps at all sites (F859).
- ~~prologue: pillars bubble~~ — spawned concurrent with contracts. Remaining prologue floor
  ~20 min = research 7 + skeleton 7 + detail 6-7 (saturated-fleet phases; detail+contracts
  merge (~3 min) is the recorded next step; full pipelining NO-GO with reasons in F859).
- OPEN: e2e twin straggler tail (re-measure on the 2-shard cut before touching); no-claimant
  wave cap (bounded to one wave by progress-rounds; conservative design recorded).

## swarm-20260816-202317154  wall 110 min  (audited 2026-08-17T01:13:33)
- repair_phase: 43.4 min (best ever 43.4)
- dead_attempt_node_secs: 25.3 min (best ever 25.3)
- research: 6.8 min (best ever 6.8)
- dag occupancy 2.38 of pool; 12.2 min at concurrency <=1

## swarm-20260816-221334699  wall 90 min  (audited 2026-08-17T02:46:59)
- dead_attempt_node_secs: 19.4 min (best ever 19.4)
- repair_phase: 16.0 min (best ever 16.0)
- skeleton_convergence: 8.0 min (best ever 5.0)
- REGRESSION skeleton_convergence: 8.0 min vs best 5.0 — find the cause before the next batch ships
- dag occupancy 1.89 of pool; 23.1 min at concurrency <=1

## swarm-20260817-003133940  wall 142 min  (audited 2026-08-17T05:54:38)
- dead_attempt_node_secs: 52.3 min (best ever 19.4)
- repair_phase: 42.9 min (best ever 16.0)
- research: 5.9 min (best ever 5.9)
- REGRESSION dag_window: 83.4 min vs best 47.4 — find the cause before the next batch ships
- REGRESSION dead_attempt_node_secs: 52.3 min vs best 19.4 — find the cause before the next batch ships
- REGRESSION repair_phase: 42.9 min vs best 16.0 — find the cause before the next batch ships
- REGRESSION wall_total: 142.3 min vs best 89.7 — find the cause before the next batch ships
- dag occupancy 1.72 of pool; 49.5 min at concurrency <=1

## swarm-20260817-025439301  wall 104 min  (audited 2026-08-17T07:39:31)
- dead_attempt_node_secs: 39.7 min (best ever 19.4)
- repair_phase: 20.4 min (best ever 16.0)
- research: 6.5 min (best ever 5.9)
- REGRESSION skeleton_convergence: 5.9 min vs best 3.3 — find the cause before the next batch ships
- REGRESSION detail_fan: 5.3 min vs best 3.9 — find the cause before the next batch ships
- REGRESSION dead_attempt_node_secs: 39.7 min vs best 19.4 — find the cause before the next batch ships
- dag occupancy 2.58 of pool; 16.7 min at concurrency <=1

## swarm-20260817-045132704  wall 90 min  (audited 2026-08-17T09:22:38)
- repair_phase: 20.4 min (best ever 16.0)
- dead_attempt_node_secs: 15.9 min (best ever 15.9)
- research: 7.7 min (best ever 5.9)
- REGRESSION skeleton_convergence: 5.5 min vs best 3.3 — find the cause before the next batch ships
- REGRESSION detail_fan: 5.3 min vs best 3.9 — find the cause before the next batch ships
- dag occupancy 3.74 of pool; 4.1 min at concurrency <=1

## swarm-20260817-081600135  wall 112 min  (audited 2026-08-17T13:09:16)
- repair_phase: 39.5 min (best ever 16.0)
- dead_attempt_node_secs: 18.3 min (best ever 15.9)
- research: 6.3 min (best ever 5.9)
- REGRESSION skeleton_convergence: 4.7 min vs best 3.3 — find the cause before the next batch ships
- REGRESSION repair_phase: 39.5 min vs best 16.0 — find the cause before the next batch ships
- dag occupancy 2.34 of pool; 13.1 min at concurrency <=1

## swarm-20260817-100917796  wall 74 min  (audited 2026-08-17T14:24:49)
- dead_attempt_node_secs: 39.2 min (best ever 15.9)
- repair_phase: 8.2 min (best ever 8.2)
- research: 7.2 min (best ever 5.9)
- REGRESSION skeleton_convergence: 6.0 min vs best 3.3 — find the cause before the next batch ships
- REGRESSION dead_attempt_node_secs: 39.2 min vs best 15.9 — find the cause before the next batch ships
- dag occupancy 2.69 of pool; 9.8 min at concurrency <=1
