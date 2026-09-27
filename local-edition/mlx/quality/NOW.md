# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 13:00 (date) · heartbeat cron 90b0083a (silent 00:35→07:50!) + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.58 (3.0.57 + Q-165 memory guard counts only the engine).
- E2E #5b RUNNING: Flash PIPELINE split, stdlib brief, runwatch bkbs1ri4h (the pipeline's /v1/status has no
  `stream.tail`, so only the size/notice/driver rules apply). Proves Q-143/144/145/159/160 in real chat use.
- 3.0.59 DONE — install QUEUED behind: E2E #5b turn 3 (install restarts the pipeline run). (Q-161 FIXED AT THE CAUSE: mlx_lm's GenerationBatch.filter left a finished row's empty processor
  list, so the guard ran 1 token on the joining tool request; each row now keeps its own processors + a text-cycle
  stop; + Q-177 validation is NOT in it). Then 27B tensor E2E #3g.
- Critic round 2 filed (Q-166..Q-176, 20 proven, 8 reopened); load 26e proved Q-145/Q-160.

## Agents
- Round-2 sidecar (Q-166/167/168/174) · desktop (Q-166/170/172/173/174/176/23/45) · core (Q-169/171/97/175).
- Q-177 split request validation (top_logprobs > 11 drops the connection).

## Next actions (in order)
1. Q-161 + Q-162 + chat merge land → 3.0.56 (with engine surfaces + Q-160) → install → split-start 27B → E2E #3f
   (jira brief) WITH runwatch.sh → prove Q-161 (no runaway), Q-162, Q-141/142/107 and livecheck green (Q-147).
2. Live critic round 2 during #3f (needs-you card, Active now, Q-148 confirm dialog seen in the app).
3. Flash pipeline load 26e (Q-145 aging + Q-160 slots): canaries every 60 s stay ~seconds in steady state.
4. Then E2E #5b Flash pipeline (stdlib brief).

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Never install without the smoke.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
