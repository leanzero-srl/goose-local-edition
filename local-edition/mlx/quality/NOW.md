# MLX quality loop — NOW (rewritten every tick, ≤ 60 lines; history → FINDINGS-LEDGER.md / E2E-RUNS.md)

Updated: 2026-09-27 12:10 (real clock) · heartbeat cron 90b0083a (silent 00:35→07:50!) + runwatch.sh per run

## Live
- Installed on both Macs: 3.0.57 (Q-161 XML guard + repeat stop, Q-164 last-token reuse, Q-162, Q-103 single-engine
  SRPF lz.10, the 18 reconciled open rows, Q-163). NOT yet built: Q-165 (memory guard counts only the engine).
- E2E #3f STOPPED at turn 0 (~6 min): Q-161 REOPENED — the model leaves a write call via `!\n</parameter>\n</function>`
  loops the ported guard does not constrain. Seen IN WORDS by the new forming disclosure (Q-151 live) and livecheck
  green all turn (Q-147 live). runwatch now also fires on a repeating live tail.
- Q-161 agent owns the 27B split for probes (no other GPU user). Pipeline E2E #5b waits behind it.
- BUILDING 3.0.58 (Q-165) from 6b1f9f22e · watcher barnucg1n.
- LOAD 26e DONE: 19 canaries, median 1.98 s, max 5.6 s (26d: 297–353 s) → Q-145 + Q-160 PROVEN LIVE; the one 503
  was Q-165.

## Agents
- Q-161 (reopened): walk the live text through the guard's rules, mirror lz.7 exactly, add an engine-side stop on a
  verbatim repeating span outside any call.

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
