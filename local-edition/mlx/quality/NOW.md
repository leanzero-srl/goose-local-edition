# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-28 04:04 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.65 on both Macs; split up "OK" (after a restart — harness Q-244 fixed). E2E #3n stopped for the install.
- LIVE TESTER agent is the only GPU user: J3 (strategy swap across windows), J4 (two-chat thrash probe), Q-231 wait
  prove, Q-232 read_image crop prove. Restores the split + clean.sh after. E2E #3o waits for it (one GPU user).
- Q-223 live prove on 3.0.65: HALF. goosed exits by itself 9–11 s after quit (stdin close works); the app's quit does
  not wait (Q-241); its teardown could not reach the Studio (Q-242); a leaked vitest fixture orphan (Q-243).
- On main after 3.0.65 (→ 3.0.66): Q-232 split tool-arg types, L10 wake broadcast, CI fix (/loop in builtin list).

## CI
- 035970561 RED (slash_command test lacked /loop, from L3) → fixed 7ebd44ac7. Later runs in progress; check next tick.

## Agents (worktrees)
- L2b on_prompt door · L4 start dialog + queue · Q-233/234/235 (split named 500, pipeline pin convergence, relay-test
  wait) · Q-241/242/243 quit path · live tester (no worktree, drives the app).
- Queued behind the L2b merge: Q-239 (kind header — touches on_prompt). Queued behind a free slot: Q-236, Q-237,
  Q-238, Q-240.

## Next actions (in order)
1. Live tester report → file its rows, dispatch fixes → start E2E #3o on 3.0.65 (brief 2026-09-25-1 jira migration).
2. Merge each agent as it lands (read log, gate, wincheck for Rust, push) → build 3.0.66 once L2b + quit fixes land.
3. Live critic walk (mlx-ux-critic) of Nodes, Strategies, loop rail after J3/J4.
4. L2c + L9 (loop harness + ≥5-tick loop E2E) after L2b/L4.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB — now 36 GB), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Release builds use a private TMPDIR.
- Merged worktrees: `git worktree remove -f -f` (never rm under .claude/). Crate-wide lib tests before a push.
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
