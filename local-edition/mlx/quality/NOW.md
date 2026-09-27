# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 21:28 (date) · heartbeat cron 90b0083a + runwatch.sh per run

## Live
- Installed: 3.0.61 (both Macs, verified). BUILDING 3.0.62 from a654dfa85 (Q-185 background labels, Q-187/188,
  Q-189/190 diffs + Changes rail, Q-191/192 LeanZero pill + report form, r1 Q-219) · watcher bgooafd7y. Install
  via `touch <run>/STOP` → driver exits at the boundary → install.sh → split-start → #3l.
- E2E #3k RUNNING on 3.0.61 (fresh r1: visible-only messages, STOP file) · runwatch bey93kmww. #3j's turn-3
  'notice' was #3i's hidden chat (Q-219) — not a split crash (rank1 alive since 20:49:37).
- leanzero.net: Q-209/210 pushed (ce043ba, red-teamed FIX-THEN-PUSH → fixed) · live probe bi5f4xi9l (413 wording).

## CI
- Green through dc701189c; merges since gated locally (tsc, vitest 3,243, eslint, i18n, cargo check, wincheck).

## Agents
- LANE D: design REVISION after the review (13 defects: one-way-at-a-time across Macs, cross-process holders,
  per-reply batching, Tier A can't reach split/remote, …) · S1 nav/IA surgeon (told: no main.ts edit).
- Q-215..218 engine glance (drop composer line + in-app float, card in flow, hide/restore, lead with the chat's
  request) · Q-211/212 audience · Q-220 fetch 404 wording · Q-221 skills path check.
- Merged locally, gate bdhcg38ai: Q-197/198/213/214 (9abeb5c1c).

## Next actions (in order)
1. 3.0.61 DONE (glance + Q-183) → install at #3i turn-3 boundary (watcher b3ugzqvvu) → split-start 27B → #3i continues (new session = #3j if needed).
2. Live critic round on 3.0.61: live states, needs-you card, engine glance on all three surfaces.
3. Merge Q-184 (Rust: wincheck.sh before push) → next build.
4. Flash pipeline E2E on lz-pipeline-qwen4.14 (Q-178/179/181 proof) + load run.

## Standing rules for every tick
- CI status, agent audit (processes, disk ≥ 30 GB, pushes), clean.sh orphan scan on both Macs.
- A DONE build is installed + split-start smoked in the same tick. Release builds use a private TMPDIR.
- Merged worktrees: `git worktree remove --force` (never rm under .claude/ — it prompts the owner).
- Before pushing a Rust merge: harness/wincheck.sh. One GPU user at a time. Nothing waits for the owner.
