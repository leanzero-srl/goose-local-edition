# Pillar-polish loop — assessments + improvements (on merged v1.41.0)

## polish-1 (csvkit-lite, 4 modules 6 commands, all flags + JUDGE) — assessed 2026-07-04
DELIVERABLE: EXCELLENT — exit 0, 36 pytest pass, every command CORRECT (stats age mean=30/min=25/max=35/count=3; head/select/filter/sort/to-json all right). COMPLETE green at round 0 (built correctly first pass on the merged binary — pillars survive the upstream merge).
REASONING: SOUND — decomposition clean (io-module shared base; transforms/stats/formatting depend on io; cli depends on the 3; integrate-verify last). owned_files DISJOINT + sensible. Contracts + parallel-skeleton planning fired.
BEHAVIOR findings:
1. JUDGE re-judges the long sink (integrate-verify) 5-6x, ALL "ok" conf 1.0 (seq 35/37/38/39/42, ~60s apart = JUDGE_REJUDGE_COOLDOWN_SECS=60). The re-judges add nothing (task already trusted). With SINK_REVIEW on, those idle nodes could do REVIEW (finds defects) instead of re-confirming -> a judge x idle-fill MISALLOCATION. Candidate fix: after k consecutive "ok" verdicts on a task, back off / stop re-judging it (free the node for sink-review); the idle-based worker_timeout stays the hard-stall backstop, so catch-ability is preserved. TO VERIFY + adversarially review before shipping.
2. gates_min=13.7 on a CLEAN app (review_fanout findings=0) — the REVIEW stack runs full cost even when COMPLETE is green round-0 and nothing is found. Possible: skip/short-circuit some review when COMPLETE is green + app is small. LOWER confidence (the review is the safety net; cutting it risks quality). HOLD unless a safe form found.
Phase buckets: research 1.9 / planning 15.7 / execute 16.5 / gates 13.7 / total 47.7 min.
Per-device dispatch: mac-gabee 29 (judge-inflated), worksmacstudio 12, local-mihai 9, mihai 6, workhorse 2 — real WORKER balance is fine; the 29 is mostly judge verdicts on one node.

## Fix #1 SHIPPED (eb7161e86 + test-fix): JUDGE — skip re-judging the owns-nothing sink
scheduler.rs pick_judge_target now skips RE-judging a task with no owned files (keeps its first judge); the re-judge cooldown became a JudgeConfig knob (rejudge_cooldown_secs default 60). Adversarially cleared (workflow w6nllng8r, both skeptics HIGH conf): the BLANKET k-ok backoff was REJECTED (unsound — disarms the judge behavioral gates on file-owning workers; worker_timeout is idle-based so it can't catch ACTIVE looping). The scoped fix loses nothing for the sink (its deterministic gates are already disarmed, its verdict always non-actionable "ok") and returns ~1 idle node to sink-review. Test: judge_skips_rejudging_owns_nothing_sink (deterministic sink<=1). Lesson: timing-based judge-firing-count assertions flake under concurrent test load — assert the DETERMINISTIC cap, not a >=N count.

## polish-2 (calcpipe, deep tokenizer->parser->evaluator chain) — assessed 2026-07-04
DELIVERABLE: EXCELLENT — 54 pytest pass; calcpipe CORRECT (2+3*4=14 precedence, (2+3)*4=20 parens, sqrt(16)=4.0 functions, -5+3=-2 unary). PILLARS HEALTHY:
- COMPLETE fixed a REAL parser.py defect (round0 findings=1 fail -> round1 findings=0 pass -> green). Correct behavior.
- DYNAMIC REPLAN fired (round0 added test-tokenizer+test-functions to fill idle nodes on the serial chain). Working.
- SINK idle-fill fired: prewarmed=3, found 3, ALL 3 REFUTED by adversarial verify -> 0 survivors (correct fail-closed, no false fixes).
- Node utilization on the deep serial chain: 74% >=2 nodes, 25% single-node (largely inherent chokepoint cost, mitigated by replan). owned_files disjoint, types = shared base.
FINDINGS:
1. planning_min=22.6 (26% of an 85.6min run) — the biggest overhead lever. Investigate: skeleton (serial 27B) vs parallel detailing time. Planning quality is critical -> tune carefully, don't cut blindly.
2. complete_result reports remaining_findings=1 but the final complete_verify round had findings=0 (passed=true). Reporting DISCREPANCY (cosmetic but confusing to an operator reading the report). Candidate small COMPLETE-pillar fix — verify against code.
3. Per-device: mac-gabee 54 vs mihai-mlx 1 — preferred-model routing + judge/reviews concentrate on one node (known behavior).

## polish-3 AMENDMENT (add dedup to kept green csvkit-lite) — assessed 2026-07-04
DELIVERABLE: EXCELLENT — exit 0, 49 pytest pass (36 preserved + ~13 new dedup), regression_ok=1. NEW dedup correct (removes dup rows keep-first; --cols dedups on a subset). OLD commands preserved (stats correctly rejects non-numeric — not a regression). 8 existing files preserved + dedup added. JUDGE+CONTRACTS+COMPLETE handled MODIFICATION cleanly (extended, did not rewrite).
FIX #1 VALIDATED LIVE (A/B): post-fix binary judged the sink (integrate-verify) EXACTLY 1 time (vs polish-2 pre-fix binary's many re-judges); total judge_verdict 4 vs polish-2's 55. The scoped skip works on a real run -> valuable + positive, breaks nothing. "Validate by enabling" bar MET.

## polish-4 MINIMAL-SPEC ('a command-line unit converter') — assessed 2026-07-04
KEY: from 1 line + NO language, the swarm chose RUST (src/units.rs/convert.rs/cli.rs/main.rs + Cargo.toml + tests/cli_tests.rs) — coherent 4-module plan, disjoint owned_files, research/planning fleshed out a sensible design. Conversions mostly CORRECT (100m->328.08ft, 100C->212F, 1km->1000m).
PILLAR GAP (real, valuable): the verification pillars are PYTHON-ONLY. For this Rust app: complete_verify ran=false passed=true (SMOKE/COMPLETE self-disabled -> the app's 2 FAILING cargo integration tests were NEVER caught/fixed); review ran=false modules=0 (REVIEW self-disabled). So a non-Python app self-disables the whole verify+correct path and ships UNVERIFIED as trivially "green". run exit=1 (a task failed) but complete_result passed=true. Only the GOALS golden-checks ran (cargo run) — advisory, didn't block; some may be stale-round.
FIX OPTIONS (to crunch): (a) language-aware SMOKE — detect Cargo.toml -> cargo build + cargo test as the Rust smoke oracle (COMPLETE then fixes compile/test failures); (b) HONEST-unverified — when smoke can't run (non-Python), do NOT report passed=true trivially, surface 'unverified language'; (c) steer the planner to Python when the spec omits a language. fix#1 held (sink_rejudges=1).
