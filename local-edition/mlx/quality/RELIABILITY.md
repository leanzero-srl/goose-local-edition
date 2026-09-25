# Reliability matrix — Goose MLX quality loop

Each row's PASS criterion is written before it runs; a run's verdict goes in FINDINGS-LEDGER.md as a
reliability row. Evidence: `~/goose-builds/quality/<R-id>-<date>/`. Method: skill `goose-mlx-quality-loop`.
Failure modes and sources: `RESEARCH-failure-modes.md` (ids D=MLX/exo, L=LM Link, T=Tailscale, S=single server).

## Detectors (every row, both Macs, sampled by the harness)
- **census** `~/goose-builds/quality/harness/census.sh <label>` — engine/rank processes, model ports, free %, wired.
- **canary** — a 1-token completion through the SAME path chat uses; judged instead of any health endpoint
  (the invisible wedge: /v1/models 200 while generation is dead — S5, S8, D34, D36, T6).
- **correctness ladder** — needle prompts at ~400 / 2.1k / 2.7k / 4.5k / 7.3k / 15k / 30k tokens, temperature 0,
  answer checked (D33: tensor-parallel over JACCL returned garbage above ~2k prompt tokens while decode looked fine).
- **panic/crash census** — new `/Library/Logs/DiagnosticReports/*.panic|*.ips`; `log show --last 1h --predicate
  'eventMessage CONTAINS "panic"'`; match `IOGPUMemory.cpp:550`, `IOGPUGroupMemory`, `watchdog timeout`, `dlil_if_ref`,
  `tbt_post_recv`, `ibv_reg_mr`, `Fence::wait`.
- **rank spin** — a rank at ≥95% CPU with the token counter flat for 3 samples → `sample <pid> 5` (D: survivors spin
  forever when a peer dies; mlx#4530 open).
- **footprint** — `footprint <pid>` for the engine and tailscaled (RSS lies — S4); wired from `vm_stat`.
- **path** — `tailscale status --json` per peer: direct (CurAddr) vs DERP (Relay); health warnings.

| id | scenario | trigger | pass criterion | last run | verdict |
|---|---|---|---|---|---|
| R1 | Split long soak through goose | correctness ladder before + hourly; an agentic goose session climbing 2k→50k+ tokens with tool calls until compaction fires; a 30-min idle then one turn; two sessions at once | every ladder answer right; no rank spin; no rank exit (read from the engine, not the launcher — D27); wired flat after the first hour; no panic/crash files | — | — |
| R2 | Remote single under heavy load over Link (run 1 2026-09-25: PASS on stability — see Baselines) | 2+ concurrent streams each with a UNIQUE 12–16k prompt (prefix-cache eviction churn — D3's panic recipe, 102–108 s on stock) ≥20 min; ~20% of streams aborted mid-body; a model copy over Link at the same time; one half-close run | no panic; engine running/admission counters back to 0 after clients stop (ghost slots — S8); relay 5xx only admission 503; tailscaled footprint flat; TTFT/inter-token p95 per level; path stays direct | — | — |
| R3 | Link disconnect/reconnect, idle and mid-stream, each side (cue check: harness/recovery.mjs, 1 sample/s) | tailscaled pid killed; Link off/on in the UI; Wi-Fi off/on; UDP 41641 blocked (DERP) then restored; headscale restarted; the peer app quit; a turn after 10+ min idle | break → a named error at the client within a measured bound (never a silent hang); restore → first good token with no click; the UI never lists a dead peer as available; the orphaned request releases its slot | — | — |
| R4 | Relaunch/restore races | relaunch during mount, during split start, both Macs together — 20×; SIGKILL one rank mid-generation then relaunch | ends serving (canary), no false failure line; wired back to the pre-mount baseline after teardown (D12/D13: 90+ GB left wired); restored split uses Thunderbolt; tok/s within 10% | partial — Q-10 (3.0.31) fixed | — |
| R5 | Switch races | Run on B while A mounts; Run twice; Stop during provisioning; two windows; single↔split on different models 20× | exactly one engine and no orphan rank on either Mac after every step; wired/footprint match the mounted model; no jetsam; no "busy" once idle | 2026-09-25 run 1 (3 steps) | PASS on census; Q-28, Q-29 found; still to run: Run twice, Stop during provisioning, two windows, 20× cycles |
| R6 | Sleep/wake | `pmset sleepnow` on the Studio idle and mid-stream, wake; MacBook lid mid-stream; the same during a split | in-flight stream ends with a named error within a bound; wake → first good token timed; path back to direct; prefill tok/s after wake vs baseline (S13: launchd-spawned servers ~100× slower prefill) | — | — |

## Baselines
- 2026-09-25 11:4x — correctness ladder on the 27B SINGLE engine on Work's Mac Studio, through the Link relay (3.0.35):
  7/7 correct at 545 / 2,801 / 3,593 / 5,965 / 9,544 / 19,729 / 39,343 prompt tokens; 39k read in 134 s (~290 tok/s).
  Evidence ~/goose-builds/quality/R1-baseline-single/. This is the reference the split must match in R1.
- 2026-09-25 16:2x — R3 cue check on 3.0.38 (recovery.mjs): Link kill → amber named "reconnecting" at +4.7 s, clear at +11.2 s,
  dropped-turn notice with Retry at +14.4 s; peer app relaunch → amber at +4.5 s, "Loading … on Work's Mac Studio" at +12.2 s,
  ready at +23 s, notice with Retry at +23 s. 3.0.36: 10 s blank, glued jargon error. Evidence ~/goose-builds/quality/recovery-*-3.0.38/.
- 2026-09-25 23:24–23:50 — R2 run 1 on 3.0.41 (Studio single over Link, 3 workers × unique 13k-token prompts, 20% aborted, canary every 60 s, 25 min):
  29 load requests (25 complete, 4 aborted mid-body), 0 errors, 0 relay 5xx, 8/8 canaries 200, 0 panic/ips files, engine alive throughout,
  Studio footprint 51–66 GB, free 23–38%. LATENCY: a 1-token canary took up to 161 s — head-of-line behind the 13k prefills (Q-103).
  Not yet covered: a model copy over Link during the load, a half-close run, ≥ 2 h duration. Evidence ~/goose-builds/quality/R2-2026-09-25/.
