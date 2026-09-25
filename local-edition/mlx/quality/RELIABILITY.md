# Reliability matrix — Goose MLX quality loop

Each row's PASS criterion is written before it runs; a run's verdict goes in FINDINGS-LEDGER.md as a
reliability row. Evidence: `~/goose-builds/quality/<R-id>-<date>/`. Method: the skill
`goose-mlx-quality-loop`.

| id | scenario | pass criterion | how it runs | last run | verdict |
|---|---|---|---|---|---|
| R1 | Split long soak through goose: an agentic workflow on the tensor split for hours | no rank death, no hang (soak.py rule), wired memory flat after warm-up, no panic in `log show` on either Mac | goose session driven by the harness with tool use + growing context; mem sampler both Macs | — | — |
| R2 | Remote single under heavy load over Link: concurrent turns + a model copy over Link | zero relay 5xx besides admission 503; tailscaled RSS flat; p95 first-token stable | relay load driver + Link replicate at the same time | — | — |
| R3 | Link disconnect/reconnect, idle and mid-chat, each side | chat resumes without a click within a measured bound, or the UI names the break and the one action; never a silent hang | Link off/on in the UI; peer app relaunch; tailscaled pid killed | — | — |
| R4 | Relaunch/restore races: relaunch during mount, during split start, both Macs relaunching together | ends serving; no false failure line | harness relaunch loop | partial: 3.0.31 race found and fixed (Q-10) | — |
| R5 | Switch races: Run on another way while one mounts; Run twice; Stop during provisioning; two windows | exactly one engine serves; memory matches; no orphan processes on either Mac | harness click sequences + `pgrep` census both Macs | — | — |
| R6 | Sleep/wake of the Studio mid-route | as R3 | `pmset` on the workhorse | — | — |
