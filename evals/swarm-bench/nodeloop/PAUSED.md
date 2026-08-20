# LOOP PAUSED — Mihai's order, 2026-08-20 ~14:10

No new runs, no new engine changes, no rebuilds, no publishes until Mihai resumes.

Still physically running (pre-dated the pause, deliberately left to finish):
- sb-7 SWARM run (runs/sb7-fleet/swarm-3node-r0, port 8900, started ~13:50, ~3h). On
  completion it just writes its auto verdict and exits — nothing auto-publishes.
  Monitor STOPPED; on resume, hermetic re-score at port 8900, then decide.

State at pause: r19 hermetic 0.170 (board holds at r1 0.1837, F909); sb-7 cloud board
published; batch fully live (F905-F909); next engine target = frontend surface (J/V/T).

## Overnight exception (Mihai, ~15:20: "I will be asleep so just post")
The sb-7 swarm r1 path is PRE-APPROVED end-to-end: run -> hermetic re-score (port 8901)
-> publish the fleet entry to the site (with telemetry rates). If r1 duds at a 30-min
check: diagnose + ONE relaunch. Everything else stays paused.
