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
- 11:31 3.0.137 re-gate green on the workhorse (all 19 listed fix commits ancestors; manifests sb7.2 118/118, forge2 156/156; engine pin lz.20 = newest; split fork lz-pipeline-qwen4.16; rank fixes ship via include_str!). Notarized build pid 85910.
- 11:45 MiMo 0.48 diagnosis (WHAT-WENT-WRONG.md): concrete our-side cause = SCORER defect b_money_rendered (graded vs pack.payments[:50] only; 12/12 correct cells scored 0/12 because the table showed newest-first) -> branch wk/scorer-money. Other causes are the model's (backend never reaches `sent`; 1.7 s/frame render under SwiftShader). Plan: smoke real-use on 3.0.137 (Flash single, MiMo split) while 3.0.138 = scorer fix + WR-1..4 (after independent checks) is cut; MiMo x2 + local Flash Gauntlet/Forge launch on 3.0.138 so they are scored by the fixed scorer.
- 12:15 coordinator: stop the 3a4e2c3fa0 build, rebuild from main >= dff3d4aba9 (scorer fix). Independent check of the scorer fix CONFIRMED a lenient fallback (cells without a pack id graded vs all 12,288 payments: "$7.83" for $78.30 passes) -> fixing before the rebuild; release head = main + WR-1/2/3/4 once their independent checks pass.
- 11:06 MiMo 8985a86d scored; release agent waits for the Studio holds, then builds on the workhorse. Workmule Flash round runs AFTER the workmule auto-update check (tests the new release).

## Problems (one line each: id · found · evidence · fixed sha · re-test)
| id | rank | problem | found (round, evidence) | fixed sha | re-test |
|----|------|---------|-------------------------|-----------|---------|
| WR-1 | 1 BLOCKER | after Run it + Save as node, every chat errors "swarm chat: no `swarm` block in config.yaml"; New chats start on stays Any node (Auto) with an empty pool | Gabee RU-2026-10-31-0, 20261031_1 t0-t2 (F1) | wk/q522-setup 1679022af5 (saved node becomes the new-chat start when Auto has nothing; refusal in UI words + Choose a node button); independent check running | |
| WR-2 | 2 MAJOR | false "Restart required" right after first mount; its Remount would load a different 31 GB model | Gabee setup (F2) | wk/q522-setup 1679022af5 (engine.rs:3004 compares the RUNNING model); independent check running | |
| WR-3 | 3 MAJOR | fresh chat first token 35-40 s on a 9B | Gabee 0 t0 (F7) | prompt already byte-stable within a project; wk/q524-prefix ff2abc9e7c moves per-project skills/agents last (cross-project prefix 8k->16.8k). REAL cause in the ENGINE: hybrid-model resume only at stored checkpoints (memory_cache.py ~2351, max 4 every 2048 tokens; shared=21860 resumed_at=16384, 11.6 s re-prefill); no warm-up call. Engine fix OPEN | |
| WR-4 | 4 MAJOR | "goose check" flags a TRUE claim ("All **19 tests passed**" vs tool "19 passed") | 20261031_2 t2 (F8) turn_assessment.rs:948 | wk/q525-check c134b620fe (claim_check::report_the_same_run; 44 tests, mutation fails 4); independent check running | |
| WR-5 | 5 MINOR | 5.6 GB load took ~110 s (~50 MB/s) on Gabee | F3 | EXPLAINED: Gabee models are symlinks into ~/.goose/models-ssd = NFS from the workhorse over Tailscale (by design, machine-sync); not an engine bug | - |
| WR-6 | 6 MINOR | harness r1 grades error-notice turns `done` (false green) | RU-2026-10-31-0 (F6) | 6a405c47fb (turnend.mjs: notice elements end a turn ended=error + STOP; unit+DOM tests, mutation fails 2) | live check pending |
| WR-7 | 7 MINOR | Mesh card shows raw 409 JSON {"error":"update_required"}; Nodes card "Measuring your Macs" 18+ min idle; Mellum row has no fit badge | F4, F5 | | |
| WR-9 | 1 (bench) | scorer b_money_rendered grades correct money as wrong when the table is newest-first | WHAT-WENT-WRONG.md s1 (8985a86d 0/12 vs 12/12 recomputed) | wk/scorer-money (running) | |
| WR-10 | 1 BLOCKER | model ran `git reset --hard 1192c90` unasked and wiped 4 commits ("The commits got lost when I did `git reset --hard`"); no confirm for destructive shell | Gabee RU-2026-10-31-1 qwen35-9b (F17; also F9/F15/F28 unrequested edits/commits) | wk/f17-destructive (running) | |
| WR-11 | 2 MAJOR | gemma 4 E4B served with vision, but read_image returns "reads text only — the image … was not sent" | Gabee RU-2026-10-31-3 gemma4 (F22) | wk/f22-vision (running) | |
| WR-12 | MAJOR (model) | "remember for all my code work" never proposed as a memory; a proposed memory was a wrong lesson | F21 | | |
| WR-13 | MAJOR (model) | false "can't" claims ("I do not have a `git` tool" while shell exists; "cannot find style.css") ; turns end on "I will…"; send_note used to "reply to Aoife" | F27 F31 F23 F33 | | |
| WR-14 | MINOR | models on NFS shown as "On disk"; gemma load 7 min 16 s; after Stop the list says "No models in the models folder yet" until Models tab opens | F26 | | |
| WR-8 | harness | r1 cannot attach files (types text only, r1.mjs:455); playwright import hard-codes /Users/mihaiperdum | briefs agent 17f6fd984e | 6a405c47fb (setInputFiles per file + chip wait, ATTACH_FAIL stops; pw.mjs resolves playwright; 57 tests, mutations fail) | live check pending |

## Rounds
| round | host · config · model · build | brief | result |
|-------|-------------------------------|-------|--------|
| RU-2026-10-31-1 | gabee · single · Qwen3.5-9B · 3.0.136 | brief 1 code+debug | ran to end; no hang/crash/compaction; WR-10 BLOCKER, unrequested edits, false claims |
| RU-2026-10-31-3 | gabee · single · gemma-4-e4b-8bit · 3.0.136 | brief 3 image+audio | audio understood (both voice notes); image blocked by WR-11 |
| RU-2026-10-31-0/0b | gabee · single · Qwen3.5-9B-MLX-4bit · 3.0.136 | smoke 3 turns | after Q-522 workaround: right tools, recovered from PEP 668, 19 tests pass; 57/57/107 s per turn |

## Handoff (written at ~300k context)
