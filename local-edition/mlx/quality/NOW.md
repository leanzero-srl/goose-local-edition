# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 00:10 · heartbeat: session cron 90b0083a (:04/:14/…/:54, expires 2026-10-03)

## Live
- Installed on both Macs: 3.0.53 (+ Q-139, Q-143/144/145; pipeline fork 419306f70 on both). Flash pipeline smoke: up in 57 s.
- LOAD 26d (Q-145): canaries 1–2 at 3.3 / 1.5 s (was 347 s) — then canaries 3–4 at 353 / 297 s: the aging
  bound counts FIFO wait, so in steady state nothing can jump. Q-145 REOPENED, fork agent on the aging rule → 26e.
- CI green; the one red (developer_client_uses_working_dir_for_shell_tool) passed on re-run — a flake, watched.
- lz-ppm session adopted the live method: first live round found 5 defects (B-119..B-125); its trap is in quality-hunt-loop.
  (The relaunch restored the previous Flash split first; split-start now refuses a split serving another model.)
- E2E #3d ENDED at turn 0 (recorded): the "silent" 40-min answer was 54 IDENTICAL ledger_append calls — Q-159: the
  split samples GREEDY (no --temp; goose sends none; generation_config says temp 1.0/top_k 20/top_p 0.95). Every
  split E2E so far was greedy → re-run #3d only on a build with Q-159 + Q-146's instrument (3.0.54).
- The Engine-tab move was the owner looking at Providers (Q-147 screenshot). r1.mjs must re-open its own session when
  the view is not its chat, and share the app with the owner.
- Building 3.0.53 from main 4e3d8c228: Q-139 (MCPs out of the cwd), Q-144+Q-145 (pipeline pin lz-pipeline-qwen4.8:
  lz.7/lz.8 ported, shortest-remaining-prefill-first). Log ~/goose-builds/release-3.0.53.log · watcher bafdqwu34.
- NEW TOOLING (Q-147 lesson): harness/livecheck.mjs cross-surface live-state probe (proven on the live defect),
  r1.mjs runs it every minute (live.jsonl), critic = whole app + LIVE walk + state-propagation matrix, tick step 3b.
- Flash load 26c (3.0.51, fixed load.py): decode healthy (gap 0.7 s); prefills FIFO one at a time → canary 347 s (Q-145).
- Disk 22:30: cleaned 34 → 156 GB free (debug deps > 6 h, incremental > 2 h, ui/desktop/out, uv cache).
  Not touched (owner's): ~/.goose/models 128 GB, forge-live-harness/evidence 37 GB, ~/.codex 19 GB.

## Agents (worktree, NO GPU)
- Q-146 instrument MERGED 56ae7ca19 (/v1/status `stream`: parser_state, withheld chars, text tail; GOOSE_RANK_WITHHELD) → ships in 3.0.54.
- Method sent to the CogniRunner (cognirunner-86) and lz-ppm (projects-bd) sessions at the owner's ask.
- LIVE critic round 1 DONE: 14 rows → Q-148..Q-158 + Q-147 reach; Q-146 reframed (tool deltas reach goose; ~80% of
  tokens do not; one answer holds 41 tool calls). Test node mihai-flash-mlx removed from config (backup .bak-2026-09-26-2330).
- Engine-surfaces agent: Q-148 (Run/tray Stop cut a live answer, no confirm), Q-149, Q-150, Q-154..Q-157.
- MERGED since 3.0.53: Q-146 instrument · Q-159 split sampling (tensor + pipeline were greedy; fork .9) · Q-159 goose
  guard (a copied call runs once; ledger refuses duplicates) · NEEDS-YOU + Q-147 session states (ask_user tool, pinned
  card, Active now group, Running · 27m / Needs you / Failed pills, top-bar counts, Engine card + tray open the session).
- 3.0.54 FAILED at codesign ("bundle format unrecognized"); stale out/ cleared → building 3.0.55 from 3003d38e5
  (everything above) · watcher b8pxlp9bh → install → 27B tensor → E2E #3d rerun + livecheck + live critic round 2.
- Engine-surfaces agent: Q-148..Q-150, Q-154..Q-157 · Q-145 aging-rule fork agent (on .9 → .10).
- Chat-surfaces agent: Q-151 (live progress line), Q-152, Q-153, Q-158.
- Q-146 instrument: parser state + withheld chars + text tail in /v1/status, withholding-path table.
- NEEDS-YOU + Q-147 RUNNING state (owner asks 22:3x / 22:5x) in ONE agent (same sidebar files): ask_user tool + persistent card above composer + sidebar marker + app-wide count;
  never in benchmark/swarm sessions.

## Next actions (in order)
1. Read the silent call's words (watcher) → decide Q-146's fix. Then re-run E2E #3d. Prove: calls ≥90% cached after turn 1 (Q-142), tool call streams "writing a tool call
   to shell" (Q-141), load_tools keeps the cache (Q-107); turn times vs #3c (110/690/1380/516/924 s) and #4 Studio
   (170/81/1333 s). diskio.py on goosed (Q-115).
2. 3.0.53 DONE → install → Flash pipeline split-start → load.py rerun (Q-145 canary TTFT) + E2E #5b; build 3.0.54
   (Q-146 instrument) right after → E2E #3d rerun on 27B tensor with /v1/status stream visible → then Flash pipeline + load.py rerun (canary TTFT).
3. Critic pass (mlx-ux-critic) on 3.0.52 after #3d; include the needs-you surface once merged.
4. Agents return → read log → merge → gate (clippy, tests, wincheck.sh) → push → CI.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes, ~/.config/goose), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Never install without the smoke.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
