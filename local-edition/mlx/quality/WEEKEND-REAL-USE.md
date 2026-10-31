# WEEKEND REAL-USE campaign (2026-10-31 →)

Owner 2026-10-31: "test these local models until we fix all of the egregious problems ... start using goose outside
of the benchmarks ... for now engine fixes". Director: campaign director agent (workhorse session). Handoff thread:
~/Projects/machine-sync/docs/COORDINATOR-HANDOFF.md. Problem ids: WR-<n>, assigned here only (Q-522 was taken by the split prefix-keep fix e8b721c1ab; fix branches wk/q522..q525 = WR-1..WR-4).

## Plan
1. SHIP 3.0.137 from main (+ mimo-edit-arg-names / mimo-distributed if the merge gate is green; refreeze bench manifests).
2. REAL-USE rounds (goose-task-author style briefs, app over CDP, words read): Gabee small models now; workmule
   Qwen3.8-Flash single and the MiMo split once the Gauntlet frees the Studios.
3. Fix egregious problems at the root, ranked by how badly they block real use; test + mutation proof + independent check.

## Decisions
- 2026-10-31 10:50 mimo-distributed merge: trial `git merge-tree main mimo-edit-arg-names` = ONE conflict
  (crates/goose-sidecar/src/engine.rs); the branch already merged main 3.0.136 (2e1304d70a) and its dev build
  eca39ae893 has run the 5 h MiMo split Gauntlet 8985a86d (compaction graded B+, vision in use). Decision: merge
  into main IF the merge gate (cargo tests of touched crates + desktop typecheck/tests + unsigned package + launch)
  is green; otherwise ship 3.0.137 without it and record why.
- 11:20 MERGED: gate run2 green on the laptop (fmt, clippy 6 crates, sidecar/providers/goose developer/swarm gates, typecheck, vitest 568, unsigned package); main = 053c715e25 incl. 3.0.137 bump + notes. Notarized build waits for MiMo scoring.
- 11:40 owner (via coordinator): 3.0.137 must hold EVERY fix of the two days incl. mimo-distributed (re-merge its later commits); ancestry checklist in ~/goose-builds/release-3.0.137.NOTES.md. After install on both Studios: one E2E real-use test with everything on (single + split) before the benchmark re-runs.
- 11:06 MiMo 8985a86d scored; release agent waits for the Studio holds, then builds on the workhorse. Workmule Flash round runs AFTER the workmule auto-update check (tests the new release).

## Problems (one line each: id · found · evidence · fixed sha · re-test)
| id | rank | problem | found (round, evidence) | fixed sha | re-test |
|----|------|---------|-------------------------|-----------|---------|
| WR-1 | 1 BLOCKER | after Run it + Save as node, every chat errors "swarm chat: no `swarm` block in config.yaml"; New chats start on stays Any node (Auto) with an empty pool | Gabee smoke RU-2026-10-31-0, 20261031_1 t0-t2 (WEEKEND-GABEE-FINDINGS F1) | | |
| WR-2 | 2 MAJOR | Run it never persists mlx_engine.model_id: false "Restart required" right after first mount, and its Remount would load a different 31 GB model | Gabee setup 10:52-10:54 (F2) | | |
| WR-3 | 3 MAJOR | fresh chat prefills goose's ~22k-token system prompt + tools cold: 35-40 s before the first word on a 9B | Gabee 0 t0 calls.tsv input 22157 cache_read 0 (F7) | | |
| WR-4 | 4 MAJOR | "goose check" flags a TRUE claim ("All **19 tests passed**" vs tool "19 passed") | 20261031_2 t2 (F8) turn_assessment.rs:948 | | |
| WR-5 | 5 MINOR | 5.6 GB load took ~110 s (~50 MB/s) on Gabee | F3 | EXPLAINED: Gabee models are symlinks into ~/.goose/models-ssd = NFS from the workhorse over Tailscale (by design, machine-sync); not an engine bug | - |
| WR-6 | 6 MINOR | harness r1 grades error-notice turns `done` (false green) | RU-2026-10-31-0 (F6) | 6a405c47fb (turnend.mjs: notice elements end a turn ended=error + STOP; unit+DOM tests, mutation fails 2) | live check pending |
| WR-7 | 7 MINOR | Mesh card shows raw 409 JSON {"error":"update_required"}; Nodes card "Measuring your Macs" 18+ min idle; Mellum row has no fit badge | F4, F5 | | |
| WR-8 | harness | r1 cannot attach files (types text only, r1.mjs:455); playwright import hard-codes /Users/mihaiperdum | briefs agent 17f6fd984e | 6a405c47fb (setInputFiles per file + chip wait, ATTACH_FAIL stops; pw.mjs resolves playwright; 57 tests, mutations fail) | live check pending |

## Rounds
| round | host · config · model · build | brief | result |
|-------|-------------------------------|-------|--------|
| RU-2026-10-31-0/0b | gabee · single · Qwen3.5-9B-MLX-4bit · 3.0.136 | smoke 3 turns | after Q-522 workaround: right tools, recovered from PEP 668, 19 tests pass; 57/57/107 s per turn |

## Handoff (written at ~300k context)
