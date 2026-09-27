# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 16:24 (date) · heartbeat cron 90b0083a (silent 00:35→07:50!) + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.59 — VERIFIED running (install.sh now proves a post-swap pid; Q-180: the 13:08 install
  left 3.0.58 running an hour and voided E2E #3g).
- E2E #3h RUNNING (27B tensor, jira brief, 3.0.59 with the Q-161 row-processor fix really live): turns 0–4 done in
  250 / 314 / 208 / 1827 / 419 s, NO runaway so far. runwatch bbcribpnh (its LIVE counter + serving-title parse fixed).
- BUILDING 3.0.60 (3rd try) from ada1bbc39 with a PRIVATE TMPDIR (the 2 earlier failures + 3.0.54 = an agent's
  electron-packager wiping the shared $TMPDIR/electron-packager) · watcher b3rd227rg. Adds Q-182 (the split's cache
  evicted the reusable prefix under a label request's padded batch → periodic 70–98k full re-reads). Install at a
  #3h turn boundary, then #3h continues on it or #3i starts.
- #5b (Flash pipeline) found Q-178 (no arg streaming, no live words) and Q-179 (0 cache past ~53k) — both merged.

## Agents
- Engine PiP (owner idea 2026-09-27 15:4x): research + design + build an in-app compact live card in the sidebar's
  empty space / movable mini-card, and an optional system-level floating window (always on top, never steals focus).

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
