# WALL-TIME DESIGN — from the r5/r6 attribution sweep (2026-08-19, wf_80119517-aa2)

Mihai's constraint: finish sooner WITHOUT a hollow repair phase ("I don't want it to last
2 hours but not fix a thing"). No time hardcodes (standing order). Everything below is
engine-truth from run.jsonl; the design cuts wall by CONVERSION, never by capping.

## Measured attribution (r5 = 177.4 min; r6 live on the f83fe8131 batch)

| sink | r5 | r6 (partial) |
|---|---|---|
| prologue | 38.5 (17.1 silent gaps, 14.0 detail, 4.0 contracts) | 30.7 |
| DAG window | 96.6 | open; 100% at 3+ in-flight so far |
| retried-attempt wall | 58.3 (test-api 30.0, web-viz 19.4) | **104.9 already** (test-api/meridian/web, web-viz/css) |
| tail | 42.4, 71% single-task | not reached |
| serial fix chain | 41.7, promoted NOTHING | tbd (first live strategy-switch test) |

Repair economics, all six fleet runs: 313.9 repair min cleared 8 findings net (~39 min/
finding); 55% of repair time (7 rounds, 172.8 min) cleared zero. Race promoted 2/3 rounds,
sched 1/7. Verify toll ~0.63 min median — NOT the dead weight. When evidence existed the
repair was fast (r0: app.js 11->6 verified in 238 s; F878 chain 12->6->3).

Token physics (live capture, r6 traffic): decode gabee 13.2 / workhorse 12.1 / mihai-local
7.9 tok/s; small prompts prefill ~1 s. One 3k-token generation = 4-7 min. Turn count and
output length ARE the wall. Stall class survived temp 0.2 -> task shape, not sampling.

## The batch (rank = measured minutes; confidence stated per Mihai's rule)

1. WARM RETRY (highest yield; HIGH confidence on mechanism, MEDIUM on size).
   A retry must be a DIFFERENT attempt: (a) in-session reset carrying partial work + the
   stall reason (core RetryConfig, K2 — swarm passes retry_config: None today);
   (b) forced tool-choice on the "clean reasoning, zero tool calls" stall (13/15 measured,
   Q11); (c) after two stalled attempts SPLIT the task (split buys +0.036 quality measured;
   stall-after-write currently never splits — only no-first-write does, F859).
   Recovers ~30-40 min/run and ships FEWER failed tasks into the fix phase.
2. PARALLEL FIX LANE (HIGH confidence). fix_sched's file tasks are disjoint by
   construction; dispatch them concurrently, #join stays the only serial step. r5 ran the
   chain one-at-a-time while 5 slots idled (71% lone-task tail). More repair mechanisms
   per wall-minute — repair gets STRONGER, not shorter-circuited. ~10-20 min.
3. SERIAL PHASES ON THE FASTEST NODE (MEDIUM-HIGH). Local node 7.9 tok/s vs gabee 13.2:
   the same serial chain is ~40% cheaper on the fast node. Needs the adopted per-call
   telemetry (task #40) for per-run measurement; until then a static rank from lms stats.
   ~8-12 min on r5's shape.
4. PIPELINED PROLOGUE (MEDIUM — dispatch-ordering bugs hide here; S5 in the campaign
   plan). Release a module when ITS contract+detail land. ~8-12 min.

Net honest target: ~110-125 min on r5's workload with a STRONGER repair phase. No cap,
timeout, or round count is lowered anywhere.

Discipline note: the shipped-history sweep found ~1 in 3 prior speed mechanisms inert as
first shipped (OnceCell cap, gate-bypassed scans, the F904 five). This batch takes the
same path as F904: adversarial review + live event proof before any number is believed.
Implementation after r6 completes (frozen binary during runs) + telemetry adoption first.
