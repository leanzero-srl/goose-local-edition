# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-29 23:3x (date) · runwatch.sh per run · vigil-shot.mjs every tick (READ the PNG)

## ⏸ PAUSED by the owner, 2026-09-29 23:3x
"let's pause the ticks for now and let workhorse and this laptop rest a bit please" — "take note of where you left off".
Nothing of the loop runs. Do NOT restart any run, gate, build, agent or cron until he says resume.
- Cron 90b0083a (UNATTENDED TICK) DELETED — recreate it only on his resume.
- Killed per pid: r1 #3z (33130) + sampler (33131); /tmp/rel082.sh (28950), `just release-notarized 3.0.82` (39611),
  `cargo build --release` (40308) and its orphaned rustc (98468). Agent a3549e2f2c822e525 (Q-521 redo) and
  runwatch task b6rhjtnuz stopped.
- Engine /v1/status: idle, 0 running. 3.0.81 installed on BOTH Macs, split still loaded (idle).
- Not ours, left alone: workhorse scripts/sag/scan_attachments.py ×4, laptop tools/make_fake_logs.py, other sessions' node.

## Where it stands
- 3.0.82 = main 7b90a1789: Q-509 (Link Busy during schedules), Q-518 (skills cache), Q-519 (a stop keeps its partial
  words), Q-520 (shell tool names survivors; chat close reaps them). Gates g082/b/c green. Release build KILLED at the
  pause — rebuild from scratch (reset package.json, detach ~/Projects/goose-rel to origin/main, /tmp/rel082.sh).
- E2E #3z (RU-2026-09-29-3z-split-tensor-coffee, session 20260929_22) STOPPED at turn 4/41, idle at stop (no Stop
  button showing). Turns 0–3: 418 s, 83 s, 5,558 s (56 tools; the model debugged its own generator's infinite loop;
  two orphans reaped → Q-520), 1,897 s; context ~121k. NOT yet recorded in E2E-RUNS.md.
- Q-521 (starvation flakes: crates/goose/tests/acp_ws/mod.rs — SETTLE 20 s in next_frame, eventually() deadline
  polls) OPEN: its first worktree was deleted by my cleanup, the redo was killed by the pause. Re-dispatch with that.

## On resume, in order
1. Recreate the cron; check CI, disk (≥ 30 GB), `git worktree list`.
2. Rebuild 3.0.82, install on BOTH Macs (`REPO=~/Projects/goose-rel zsh install.sh 3.0.82`), smoke the split.
3. Re-dispatch Q-521 (worktree agent).
4. Live proofs: Q-519 (stop a multi-tool turn both ways), Q-520 (`( … &)` command, then close the chat), Q-504/507
   (a scheduled run), Q-495, Q-511/512 (UI remount), Q-514 (side call vs one chat's prefix), Q-516, Q-509.
5. Record #3z in E2E-RUNS; restart the E2E for: the Steer note at turn 26 (Q-488), chat search 25/27/35, memory
   recall after compaction 33/39, Q-513/506 model behaviour, Q-517 turn starts.

## Queued / scheduled
- Q-398 and Q-425 QUEUED behind: fork access.
- Q-424 SCHEDULED waits on: a Studio ladder with the engine free.
- Q-270, Q-373, Q-205 SCHEDULED waits on: their named measurements.
- Q-452 waits on: a second red. Q-506/513 parked (model). Q-510 refuted (keys — owner: leave as is).

## Standing rules for every tick
- Check CI, agents, disk ≥ 30 GB. Take a vigil screenshot and READ it. The next free id is Q-522.
- Merge via scratch plus ledger_resolve, then gate, ff, push.
- Nothing an E2E raises stays pending. Never navigate the main window while r1 runs.
- If the owner uses another chat mid-run, Q-498 thrash follows. Read the Engine glance first.
- Kill pids, never killpg. Training ONLY on the owner's word.
