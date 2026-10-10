# Forge 2.0 — campaign working list (the thread; update it in the same commit as the work)

## The owner's mandate (verbatim, 2026-10-09; do not paraphrase it away)

- 19:1x "the idea that bother me is that all models are going at 0.9... what is the point then? I need more complex
  and challenging benchmark on forge ...."
- 19:2x "stop the current models from doing forge 1.0 and start designing forge 2.0, a bigger more complex app, more
  challenging forge modules, more content, more judicious checking across the board as I need it to check all models
  thoroughly. Do this and prove to yourself that a model like opus 5.5 could be challenged or gpt-6.1 sol"
- 19:4x (ultracode, max effort) "please try your best to design the toughest possible forge benchmark 2.0 especially
  since I don't want to waste money on 2.1 and future for now at least ,so let's have it build something that combines
  the latest modules existing with forge and make it a really big and tough app for even top models to struggle with
  it, invite some complex backend resolvers that invite both security concepts which matter in forge, invite
  stability and robustness in forge which again matter, speed in booting up for the custom UI, usage of UI kit 2 for
  the administrative panel, because I dream it should have a front facing, an admin panel, ensure it involves a ton of
  calls and it should know also to dose itself for tier 1 usage, look into forge tiers which are super important when
  building apps. So the app should incorporate DIFFICULT concepts, security, complex backend resolvers, pretty UI using
  for front users custom UI and then UI kit 2 for admin panel, robustness in how it use burst calls to keep it within
  tier 1, good forge LLM usage. GO!"

- 22:5x "ok great and then implement the new forge 2.0 and then please start the new benchmarks on all of the
  previous models" -> after the red-team: BUILD (all work packages) -> GATE -> release + site per-era support ->
  Sol pilot (the 20:1x acceptance test) -> flip -> rerun EVERY model that ran Forge 1.0 (31 entries; Mistral excluded:
  no OpenRouter host streams tool args) plus Haiku 5.5 / Step 5 preview, Forge-only queue entries.

- 23:0x "decide on your own stuff ... I just want to wake up to have at least 2 models benchmarked or at least one on
  the new benchmark" · 23:1x "WTF? What do you mean days of work? ... find the more effective way" · "please make it a
  hard rule never to over engineer shit like this" (now a global CLAUDE.md HARD RULE + memory).
  -> THE PLAN IS `forge2/SPEC.md` (lean: Scope Ledger 2 = brownfield upgrade of the 1.0 app on 1.0's machinery).
  DESIGN-DRAFT.md + panel/ are reference only. Build wf forge2-build-night (11 packages, branches forge2/<pkg>,
  deadline 02:30) -> integrate + gate (golden 1.000, starter <= 0.30, one mutant per R-family, e2e) -> pilot via
  run_build --forge2: GPT-6.1 Sol, then Claude Haiku 5.5. Call budget 300 (FORGE20 only). Publishing after the app
  ships the forge2 tier.

## Mandate checklist (every item must be a graded, STATED requirement in the design)

1. latest Forge modules combined · 2. big, tough app (top models struggle; no 2.1 soon → maximal now) ·
3. complex backend resolvers · 4. Forge security concepts · 5. stability and robustness · 6. Custom UI boot speed ·
7. front-facing Custom UI (pretty) · 8. UI Kit admin panel · 9. a ton of API calls, dosed to stay within rate-limit
Tier 1, robust under bursts · 10. good Forge LLM usage · 11. judicious checking across the board (1.0 §17.8 lessons) ·
12. PROOF that Opus 5.5 / GPT-6.1 Sol are challenged.

## Inputs already in hand

- Forge 1.0: `evals/swarm-bench/forge/` (DESIGN.md §1-17; §17.8 = the 8 confirmed accuracy defects of 2026-10-09).
- Real-Forge receipt: two range attributes on a KVS index → refused by `forge lint` 14.1.0 and by `forge deploy`
  (throwaway app `lz-range-probe` on wolfaenpak). The kit's offline lint must equal the current CLI's.

## Phases (each one a workflow; read each result before starting the next)

| # | phase | status |
|---|---|---|
| 1 | UNDERSTAND — 1.0 machinery reuse map, frontier behaviour on 1.0 (Haiku/Pareto transcripts + Sol/Opus rows), real-Forge fidelity tooling, desktop/site integration | DONE 2026-10-09 20:05 — reports in session scratchpad forge2/understand/ (machinery, frontier-behaviour, real-forge-fidelity, integration); key: frontier models transcribed 1.0's trap list; emulator matched real Forge on only 13/24 KVS probe cases; real lint is half server-side |
| 1b | RESEARCH — platform modules/changelog since 2026-07; mandate topics (tiers, UI Kit, boot speed, security, LLM, robustness), every design-critical claim re-verified | DONE 2026-10-09 20:55 — `research/BRIEF.md` (292 claims re-verified: 285 confirmed, 4 refuted, 3 outdated) + topic notes + platform-2026-10-09.md + understand/ + forge1-review/ committed in `forge2/research/` |
| 2 | DESIGN — judge panel of independent full designs, scored on frontier difficulty, fairness, offline gradeability/fidelity, mandate coverage, build feasibility, cost; synthesis | running 20:58 (wf forge2-design-panel: 6 angles × 5 lenses → `forge2/panel/*.md`, `panel/SCORECARD.md`, `forge2/DESIGN-DRAFT.md`) |
| 3 | RED-TEAM — multi-lens attack on the design (unstated requirements, emulator fidelity, gameability, everyone-scores-0, 1.0 defect classes, security-claim correctness), each finding verified, fixes | — |
| 4 | PRE-BUILD DIFFICULTY PROOF — fresh Opus-class agents plan the app from the contract alone; graded against the hidden check list; harden until frontier plans miss real things | — |
| 5 | BUILD — mock Jira/Confluence + rate tiers, emulator (concurrency, virtual-time limits, UI Kit host, webtrigger, admin), real lint in the kit, probe, scorer, golden, alt app, mutants | — |
| 6 | GATE — golden 1.0, alt ≥ 0.95, mutants exact, empty ≈ 0, severity selftest, calibration | — |
| 7 | PILOT (paid) — ACCEPTANCE RULE REVISED by the owner 2026-10-10 ~08:0x: "if sol still lands in the 0.9 I guess it's fine because the gauntlet also shows it as being a top model so I think it's fine. I would though check a lower end model to see how it scores, like sonnet 5.5 for eg. if that goes into 0.9 as well then definitely not good. this is to keep in mind after the hardened version lands" -> after hardening: Sol rerun (0.9 acceptable) + Claude Sonnet 5.5 run; Sonnet near 0.9 = NOT discriminating -> harden further. (Original: ONE run of GPT-6.1 Sol through the desktop app on 2.0 = THE ACCEPTANCE TEST. Owner 2026-10-09 20:1x: "make sure you make a test with whichever is cheaper, gpt-6.1 sol or opus 5.5 as they're both real top dogs and see if they get challenged. if that happens we're good". Sol is the cheaper: half Opus on every token class ($2/$10/$0.10 cache-read vs $4/$20/$0.20 per M, OpenRouter 2026-10-09) and Forge 1.0 Sol = $0.72 / 13 min vs Opus 26 min. "Challenged" = Sol loses real points on stated engineering requirements (not on harness faults or unstated rules); if Sol cruises, harden 2.0 before anyone else runs it | — |
| 8 | RERUN every model that ran Forge 1.0, on one goose release | — |

## Line state

- Forge 1.0 queue entries: stopped/blocked (owner 19:2x). Gauntlet continues (Mistral Large 4 running 19:30).
- DONE 2026-10-09 23:55: Forge 1.0 FROZEN with the reviewed 654-char note (site af5162c on master; `register-forge.mjs freeze --live`: bstate-forge-1-0 frozen:true + note, still forge familyCurrent until the 2.0 flip; 32/32 pages render it; POST 409). Installed apps show "no single available Forge benchmark" for Forge until a 2.0 app ships. Rewrite the note in the past tense at the 2.0 flip (`register-forge.mjs note`).
- 15:45 OWNER: "and then please let's queue all models from gauntlet so we have a counterpart in forge for each."
  Checked the live board (Sanity, read-only): 34 Gauntlet 7.2 runs, all cloud baselines; 33 already had a forge-2.0
  queue entry; the one missing was openai/gpt-6-luna-pro (its Forge 1.0 entry was dropped 2026-10-04 under "one OpenAI
  variant per family") -> added on loop-state forge2/chain 6ec6a2a: 34 of 34, all BLOCKED until the Forge 2.0 release +
  register; order Sonnet 5.5, Sol, then the rest, Luna Pro last. Both agents resumed after the 429 (gate agent by
  SendMessage, copy review by resumeFromRunId wf_8e4008f2-e57 -> task wxmsozqcf).
- 15:40 SESSION LIMIT (429) KILLED THE FINAL-GATE AGENT at ~15:35; owner re-logged in 15:39 ("continue and retry").
  MAP: its queue runner SURVIVED as a plain shell process (scratchpad/fgate/run_jobs.sh jobs1.txt -> score.sh, serial,
  load-gated; log jobs1.txt.log): reference x5 on the final scorer ee4dd033d — g-0123 1.0000 and g-a0c4 1.0000 (forge-2.0,
  every row 1.0 incl. both E rows, reliability 1.0; ~450 s each), g-c4cd running, g-5eed and g-fedc queued (~15:57).
  wt-final clean at ee4dd033d. Its helpers: fgate/{ratios.py, rows.py, judge.py, rescore_kept.py, light_suites.sh},
  rsynced sources fgate/src-sol, src-sonnet. STILL TO RUN after jobs1: calibration check (ratios vs tops 2.58/2.0),
  --reference multi-seed, starter, Sol + Sonnet re-probed on their own seeds, alt app, 10 mutants + judge, rsync to
  /Volumes/AI-workhorse/runs/forge2-final-gate/. RESUME: SendMessage the same agent id (context intact); if that fails,
  a fresh bench-scorer agent with this map. The copy-review workflow wf_8e4008f2-e57 had its draft done and three
  lens agents in flight at the kill — resume with Workflow({scriptPath, resumeFromRunId}).
- 15:3x FAIRNESS FIXES LANDED on forge2/final: 8c87d4047 R9 ignores §9's CONDITIONAL_CHECK_FAILED (probe records kvsCode),
  27692e551 r_pagination excuses a hand-off finished by the same event's redelivery (refuter's mutants still graded),
  71f5613c3 oracle field-switch allowance (observed wall + delivery times), 2ad6f0913 r7 carry after an in-wall mark,
  d4121aaf7 emulated Jira 404 on a deleted-issue write, b850dc265 wording (event row: "baseline", not "optimum";
  RATE-MODEL.json states the scripted burst 429s — contract byte-identical), 2aec08d9e sources_for third route (a
  scheduled run or its continuation in flight between the change and its event's success; kept both old arms — the
  "started in between" form missed aef2's 1157004; medium: broad for apps with continuous continuations, ships as a
  net), eb6c6a4ae the 2.0 scorer takes both score locks, ee4dd033d manifest (140, clean; CALIB unchanged). Regrade
  (no probe): reference 1.0000 x9; Sol 0.8301 -> 0.8344 (0.8417 projected once a re-probe records kvsCode), Sonnet
  0.7486 -> 0.7530; monotone on 12 verdicts.
  FINAL GATE dispatched (one agent, serial): golden x5 on the final probe (recalibrate if the worst-of-5 ratio moved;
  calibrate() must keep reliability_k/floor), --reference, starter, Sol + Sonnet re-probed on their own seeds, alt app
  >= 0.95, 10 mutants judged. Outputs -> /Volumes/AI-workhorse/runs/forge2-final-gate/.
- 15:0x RELEASE PREP (no scoring involved): (a) released tiers PROVEN untouched by the shared harness edits (read-only
  agent: every hunk of bench_budget/isolated_tiers/run_build/release_manifest classified (a); run_build.invoke()
  differential byte-identical for SB71/SB72/FORGE10 on both branches; SB7.x and Forge 1.0 stay at 150 calls) -> refreeze
  sb7.2 (packaging refuses it today), sb7.1, forge, forge2 on main after the merges; procedure + tools in the skill and
  ~/goose-builds/loop-state/tools/forge2-release/. Found: main's release_manifest filter leaked forge2_* files into the
  payments payload unpinned (fixed on forge2/final). (b) DESKTOP: shared SDK rebuilt (`build-goose-sdk`, 6 s, no Rust;
  dist was Sep 5) -> forge2/desktop 93276c26b typecheck 667 -> 0, full suite 4847 pass / 2 env-skips, eslint clean.
  (c) SITE 9bb7b07 on forge2-era: tier means (and E, scoreInner, excellence fields) recomputed from the posted rows for
  both eras (all 41 real results match: 31 live forge-1.0 runs + 10 forge-2.0 verdicts); a post must carry EVERY check
  in the era's registry (dropping a failed row had lifted Sonnet 0.745 -> 0.8201); real bodies 201, 38/38 shifted-tier
  copies 422. (d) forge-2.0 scorer to take BOTH score locks (/tmp fixed + 1.0's $TMPDIR one) — sent to the fix agent.
- 14:5x wf forge2-harden COMPLETE (gate agent): hardened-gate 9cb27f51a = calibrated (CALIB 72acca75…, merged into
  forge2/final as 34550b5d…), golden 1.0000 forge-2.0 on the 3 scoring sites (--reference PASS), race mutant
  r3_check_then_act_dedupe 0.5652 judge PASS, starter 0.2706 (<= 0.30), 10 mutants build+lint. ITS OPEN ITEMS, triaged:
  (1) forge2/final's PROBE differs from the one calibrated (f866b7490 world events as agenda points, 4e20900e4 Rovo after
  the query lag, e45c44176 explain wait, admin pictures, kvsCode) -> the final gate re-probes the golden x5 on the final
  probe and recalibrates if a ratio passes its top [QUEUED behind: the fairness-fix agent]; (4) a change made just after
  the wall can be recorded first by the deferred hourly run and no row accepts it -> sent to the fairness-fix agent as
  item 7 (one general source rule); (5) golden corner (late delivery for a closed sprint after a switch) -> the oracle's
  new switch allowance (b) covers entrants; the golden stays as is unless a gate seed trips it; (6) economy tops loose
  (2.58/2.0) -> kept: reference-fitted is the fair bar, E never multiplies; (8) gate scripts committed in loop-state
  9016bb7. SITE v2 mirror 6d5f1b5 on forge2-era: per-group recompute equals the scorer field for field on all 10 real
  verdicts; the site now recomputes criticals() from posted rows (a poster can no longer hide a fired critical or move a
  row out of its group); refuses UNAVAILABLE rows; desktop bodies 201, four tampered copies 422; monotone. Pre-existing
  gap to close at the freeze re-sync: posted tier means are never recomputed from the rows (both Forge eras).
  forge2-era is 10 ahead / 3 behind origin (pre-rebase copy published by projsync; projsync never force-pushes) — the
  site lands via master, so the stale remote branch is harmless.
  FINAL SEQUENCE: fairness fixes -> final gate (golden x5 re-probe + calibrate if needed + --reference, Sol & Sonnet
  re-probed on their own seeds, alt app >= 0.95, 10 mutants + race + starter judged) -> freeze (manifest, CALIB) ->
  site re-sync at the frozen commit + tier-mean check + adversarial copy review -> desktop merge + SB7.x manifests +
  fresh SDK build + typecheck + notarized release + install -> register forge-2.0 (add) -> real publish test -> flip
  -> proxy FORGE_CURRENT + forge2/chain -> queue the 33 reruns (Sonnet first, Sol second).
- 14:3x FAIRNESS AUDIT DONE (wf_b8da61d3-ac3, 31 agents; journal = the evidence): of 16 items, FAIR (real model defects,
  stated + achievable): a_action_result (Sol serves the frozen row estimate; Sonnet falls back to it after a field
  switch), r5_panel_labels (both miss the Migration control), r3_never_killed, r9 boot (Sonnet ~245 KB vs 153,600),
  u_widget_live (Sonnet ignores the 5 s query staleness), u_widget_numbers/chart, t_trigger_handoff (medium),
  r7_writes_accepted (real Jira measured live on wolfaenpak: refuses the whole request, 404), t_reestimate_followed and
  the closed-sprint creep in b_comment_adf_as_user (I overrode the b_comment refuter: §12 "as at the close" + §3 + §10
  state it and the FIXED reference passes it on the 4 calibration seeds whose close falls inside the wall: 5eed, fedc,
  0123, aef2). UNFAIR (fixing on forge2/final now, one agent): k_runtime_risks (R9 charged §9's documented 400
  CONDITIONAL_CHECK_FAILED retries as limit errors — needs a re-probe, probe gains kvsCode), r_pagination (a sanctioned
  InvocationError hand-off finished by the redelivery charged as skipped pages), the FIELD SWITCH inside the quota wall
  (unknowable: no event/changelog; oracle allowance in one home beside the dropped-change one; reaches r4_field_switch,
  u_ledger_table, a_action_result, row_right), r7_values_fresh (the mark after an in-wall hourly run graded values
  nothing could refresh; Sonnet 0.842 -> 0.888), the E rows at the rc tops (fixed by the calibration: 2.58 / 2.0).
  Plus: emulated Jira answers the deleted-issue write like real Jira (404), honest E-row wording ("optimum" was not
  reachable), one public sentence that scripted burst 429s happen. Stray: an uninstalled scratch app
  "fair-probe-field-r7" left in the LeanZero SRL developer console (wolfaenpak test env; deletable only in the console).
- 14:2x forge2/final MERGES: admin-shot (8bf1ef8a1 + f5327f8db) and the CALIBRATED gate (8111d5c65 + 5a2fd113b; CALIB_SHA256
  re-pinned to the merged thresholds file 34550b5d…: calibration 7ed584279 unchanged + reliability_k/floor). Scorer now
  emits forge-2.0. Reference re-graded from kept observations (no probe): 1.0000 on all 9 seeds, reference_failures []
  (only the two E rows moved). Light suites all pass.
- 14:0x v2 LANDED on forge2/final: df9387c0e (rule, selftests, public text, SPEC §4, receipts, poor_practice) +
  de79a9841 (manifest refrozen, --check clean). Monotonicity sweep (every non-E row over {0,.25,.5,.75,.9,1}): 0
  violating moves on all ten kept verdicts (old rule: 10/14/11/5 on the four pilots); 16 mutated rules each fail the
  selftest. Recomposed (scratchpad/sev2-out): SOL-HARDENED 0.8248 (rel 0.8422, 9 groups) vs SONNET-HARDENED 0.7450
  (rel 0.7738, 8 groups); Sol-lean 0.6990 (its two vacuous R rows now count 0 — pre-S3 scorer artefact, lean task, never
  published); Haiku 0.0998 (rel 0.2954, not floored); reference g3 x6 reliability 1.0. Rule (iii) = every row the
  critical list carries (critical.rows, not `unsuppressed`). poor_practice: M3 ("every consumer queue is pushed to") was
  UNSTATED yet priced inside k_manifest_semantics -> _priced() now prices only would-fail findings (M9/P3 stay priced by
  their own stated rows); no kept verdict moves. Public sentence: "Failed tests also multiply the whole score, by
  group: each v2 requirement above is a group of tests and v1's behaviours make nine more. A group multiplies the
  score by 1 - 0.10 x the shortfall of its worst test ..." DESKTOP forge2/desktop 93276c26b: publish body passes the
  block through (reliability, reliabilityDefects [{tier,check,score,factor}], criticalMultiplier), refuses a 2.0 verdict
  without it; ScoringDetail one line per group; publish pictures widget L/D, sprint L/D, admin light; fixed a real bug
  (the saved snapshot published pictures in name order -> the contact sheet, never the widget); obsolete "Update Goose"
  banner deleted (launch gate refuses mismatches first; helper read the SB-only flag); 172/172 bench tests; typecheck
  blocked by the stale shared SDK build (667 errors, none in touched files) -> FRESH SDK BUILD BEFORE THE RELEASE.
  SITE: v2 mirror dispatched on forge2-era (from b4e3c06, which mirrors v1).
- 13:4x RELIABILITY RULE v1 REFUTED -> v2 (my decision; owner 13:3x asked "I assume that it is still all very fair
  correct?" — answered: not yet proven, fixing). The severity workflow's Attack returned FIX-THEN-SHIP: D1 the partial
  root fold INVERTS the gradient (I reproduced it with --recompose on Sonnet's verdict: as graded 0.7473; u_widget_numbers
  FIXED 0.875->1.0 gives 0.7078; made worse ->0.5 gives 0.7747); D2 a fired critical is charged twice (x0.6 and x0.9);
  D3 the weight-0 diagnostic k_v2_surfaces multiplies; D4 cross-family pairs double count; and INHERENT to any per-row
  "count once" exemption with K=0.10 against row weights 0.003-0.03: doing less beats doing more (dead widget 0.8838 vs
  live widget with three rows at 0 0.7220; absent web trigger 0.8424 vs present with two rows at 0 0.7841).
  v2 = WORST TEST PER GROUP: reliability = max(0.25, prod over the 18 tiers except E of (1 - 0.10 x (1 - worst eligible
  row))); not eligible: harness-unavailable rows, weight-0 diagnostic rows, a row whose critical fired (priced there);
  no ROOT_BLOCKS / vacuous / absent-surface logic in reliability(). Monotone by construction (raising a row never
  lowers the final) — pinned by a selftest + a sweep on the real verdicts. Hand estimate: Sol-hardened ~0.825,
  Sonnet-hardened ~0.745. Verdict block: reliability{multiplier,k,floor,floored,defects[{tier,check,score,factor}],
  folded{tier:[rows]},priced_as_critical[]}. Being implemented by an agent ON forge2/final (wt-final, 9866b588e = the
  merge of forge2/severity + forge2/hardened-gate with the ONE source rule; light suites pass, manifest pending);
  fixtures -> scratchpad/sev2-out/. The site mirror (forge2-era b4e3c06) and the desktop agent (told by message) must
  follow v2 — the site mirror is NOT yet re-done (dispatch after the scorer lands).
- 13:3x wf forge2-fairness-audit (wt5h1l6az / wf_b8da61d3-ac3): every row Sol-hardened and Sonnet-hardened lost points
  on (19 rows in 16 items), one reader each against the public text + the check + the observations + the reference
  and independent apps, then a refuter per row. THE THREE THAT SMELL (identical fraction in both models):
  r5_panel_labels 0.9 (both miss the SAME control, "Migration"), t_reestimate_followed 5/6 both, b_comment_adf_as_user
  10/12 both. Any confirmed unfair row is fixed on forge2/final BEFORE the freeze.
- 13:1x IN FLIGHT (four jobs, then the merge): (1) wf forge2-harden gate agent on forge2/hardened-gate — final golden
  on the calibration seeds, then CALIB_SHA256 + scorerVersion forge-2.0; its scorer change is ONE source rule
  `sources_for(c, change)` (a change made under the rate wall may name `reconcile` in every row that judges a source).
  (2) wf forge2-severity Attack (read-only refuter) on forge2/severity dab70b928; site mirror b4e3c06 on forge2-era.
  (3) agent on NEW branch forge2/admin-shot (worktree scratchpad/wt-admin-shot, from forge2/severity): pictures of the
  UI Kit admin panel in forge-shots/ (admin-panel-light/dark.png, admin-panel-saved-light.png), drawn from the kept
  ForgeDoc tree, CI secret never in a picture, no placeholder when the page did not render. WHY: owner 12:5x asked
  whether what is published still carries pictures + video of what was delivered — it does (34 shots + forge-ui.webm
  per run) except the admin panel, which is graded from its tree and had no image. (4) agent on forge2/desktop
  (worktree .claude/worktrees/wf_a728d84f-f50-1): publish body + ScoringDetail carry the reliability rule
  (verdict.reliability{multiplier,defects,folded,unexercised} -> post reliability/reliabilityDefects/criticalMultiplier;
  site route refuses a rawScore without the factor), admin shots in the publish set.
  MERGE PLAN (mine, when 1+2 return): forge2/severity + forge2/hardened-gate + forge2/admin-shot -> one branch; the ONE
  conflict is score_forge2.py t_event_rows: resolve by folding severity's `_redelivered_after_next_run` into the gate's
  `sources_for` (walled OR redelivered-past-the-next-run -> + reconcile, for every row). Then re-pin CALIB_SHA256 (the
  thresholds file gained reliability_k/floor), refreeze the manifest, `--reference` must give 1.000, re-score the four
  pilot trees + alt app + mutants on the final scorer, THEN site re-sync/review/register and the app build.
- 12:55 SOL's a_action_result 0/5 (both runs) = MODEL DEFECT (words-reader, high confidence; oracle reproduced from the
  seeds): Sol serves the frozen row attribute (`const points = c.estimate ?? 0` in src/views.js) where contract §1 says
  "a change's points are the issue's current value of the field that board used at the time of the change"; the
  starter it was handed had it right and its own SKILL.md still says "current estimate". Sonnet has a narrower form
  (falls back to the frozen estimate when the board's field switched: 3/5). NO check change, NO contract change.
- 12:2x SEVERITY RULE AS IMPLEMENTED (71cf9d6b4; differs from my brief in three reviewed places): a row under a failed
  root counts only its shortfall BEYOND the root's; vacuous/unexercised rows never count; the public sentence says "a
  test that fails only because another one did is not counted again". Recomposed from stored rows: Sol(lean) 0.7564,
  Sonnet(hardened) 0.7473, Haiku 0.0845 (floor), reference 0.9907 (reliability exactly 1.0; E rows uncalibrated).
  Sol(hardened) 0.9793 -> 0.8247 (`score_forge2.py --recompose <verdict.json>`, 13:1x). LIKE-FOR-LIKE ON THE HARDENED
  TASK: SOL 0.8247 vs SONNET 0.7473. Verdicts: scratchpad/sev-out/recomposed-*.json.
- 12:45 SOL ON THE HARDENED TASK (same task/scorer as Sonnet): 0.9793 provisional (seeds 0.9795/0.9814/0.9846), no
  critical, 66 of 300 calls, $1.35; 10 defect rows (a_action_result 0/5 in BOTH Sol runs — being diagnosed as model vs
  contract vs check; t_reestimate 5/6; b_comment 10/12; small ones). LIKE-FOR-LIKE under the severity rule (offline,
  K=0.10): SOL ~0.82 vs SONNET ~0.70 (K=0.12: 0.80 vs 0.65) -> the discrimination the owner asked for, pending the
  refuter's verdict on the rule, the calibrated re-scores and the a_action_result diagnosis. Pilot vigil cron ended.
  Runs: /Volumes/AI-workhorse/runs/forge2-pilot/{20261010-0357-sol, 20261010-0505-haiku, *-sonnet-hardened, *-sol-hardened}.
- 11:55 SONNET 5.5 ON THE HARDENED TASK: 0.9628 provisional (seeds 0.966/0.965/0.967), no critical, 39 of 300 calls,
  $2.22 — by the owner's rule NOT GOOD (Sonnet in the 0.9s). But it has 13 defect rows (u_widget_live 0 = the S2
  staleness trap; r9 boot ~245 KB of 153,600 on both surfaces; r7_values_fresh 0.84 (191/286 in the worst hour);
  r3 one resolver killed; a_action_result 3/5; t_reestimate 5/6 ...): the 99-row weighted mean is too lenient.
  Offline recompute with a per-defect multiplier: K=0.10 -> Sol(lean) 0.76, Sonnet(hardened) 0.70, golden 1.000.
- 12:0x DECISION (mine; owner: "decide on your own stuff"; his 1.0 rule "more aggressive without making it unfair"):
  final = earned x criticals x RELIABILITY, RELIABILITY = max(0.25, prod over defects (1 - 0.10 x (1 - s))), one root
  cause once, E tier excluded, stated in the prompt's Score section. wf forge2-severity (branch forge2/severity from
  forge2/prerun; site mirror on forge2-era; refuter). SOL RERUN on the hardened task launched 11:57 (sol-hardened 8855).
- 11:0x HARDENED TASK READY: branch forge2/prerun 9ac3ed765 (worktree scratchpad/wt-prerun) = forge2/hardened + pilot
  fixes + fairness review (43 alt-app notes: 39 fine, 1 must-fix contract sentence in §4 Live, 4 check/probe fixes) +
  a probe wall-clock race fixed (explain button counted the instant data painted; 4 of 25 golden scorings mis-scored).
  Golden 0.9907 (97/99 rows 1.0; economy rows uncalibrated). Site brief copy fixes on site branch forge2-era 110f600.
  The gate (branch forge2/hardened-gate) is still fixing golden edge cases before calibration.
- 11:08 SONNET 5.5 LAUNCHED on the hardened task (the owner's deciding check): forge2_pilot.sh anthropic/claude-sonnet-5.5
  sonnet-hardened 8854, FORGE2_REPO=wt-prerun, log ~/goose-builds/loop-state/forge2-pilot-sonnet.log. OpenRouter $25.68.
  Next: Sol rerun (sol-hardened 8855); results = calibrated re-scores once hardened-gate lands.
- 09:1x PARALLEL TRACK DONE: forge2/desktop 8985a45b3 (2.0 bundled era; 223 tests; fixes: 2.0 rows were dropped at
  publish, tray read the shared 150 budget; needs SB7.2 + forge2 manifest refreeze and a fresh SDK build to typecheck);
  site branch forge2-era (local, 5 commits; per-era snapshots/validation, era-scoped replace, register-forge2.mjs;
  125 tests + build; re-sync the 2.0 snapshot at the frozen commit; the 2.0 brief needs an adversarial review before
  going public); loop-state branch forge2/chain (era field + era guard, proxy FORGE_CURRENT=forge-2.0, per-era
  board_audit, 33 blocked forge-2.0 entries: Sonnet 5.5, Sol, then the field); forge2/pilot-fixes e09e2b6ba (non-UI-Kit
  admin charged: Haiku 0.3467; one fixed-path score lock in gather(); Haiku's 2.5 h was ONE hung ffprobe, not UI
  timeouts); forge2/alt (independent app from public text; ~30 unclear points in ALT-NOTES.md; React+bridge ~240 KB
  breaks the 150 KB boot budget -> surfaces rewritten without React).
- 08:2x owner: "Let's step it up and ensure everything that can be done safely in parallel is being done!" -> wf
  forge2-parallel-track beside wf forge2-harden: (1) desktop forge-2.0 era (branch forge2/desktop), (2) leanzero.net
  per-era forge support + register-forge2.mjs (site branch forge2-era, worktree /private/tmp/claude-501/lz-forge2-era,
  no push/no Sanity), (3) sb72_chain.py era field + catalog_proxy forge-2.0 familyCurrent flag + blocked forge-2.0 queue
  (Sonnet 5.5 first, Sol second), (4) pilot fixes on forge2/pilot-fixes (charge a non-UI-Kit admin page, bound scoring
  time on a dead app, one per-host score lock), (5) independent alt app bench/golden-forge2-alt (fairness proof).
  Landing order after both: merge harden + pilot-fixes + desktop -> gate + calibrate -> Sonnet 5.5 run (the deciding
  check) + Sol rerun -> site era + register 2.0 -> app release -> publish -> flip -> rerun the field.
- 07:5x HAIKU PROVISIONAL: 0.3378 (seeds 0.348/0.350/0.345), no critical; BUT 9 admin-dependent rows came back
  probe_unavailable (its jira:adminPage has no render: native) instead of charged — SCORER TODO (S4): an admin page
  that is not UI Kit is the app's observed defect -> charge those rows 0 (owner rule: "give a bad score", never refuse).
  Lean-version pilot summary: Sol 0.9618 ($2.90, 176 calls) vs Haiku 0.3378 ($0.49, 18 calls, quit itself).
- 07:5x HAIKU 5.5 (lean version): QUIT ITSELF after 18 of 300 calls ($0.49) with stubs — its words: "The core v2 work is
  still ahead, and I'm fairly deep into this session's budget" / "I can't finish all eight inside the remaining budget at
  the detail this takes"; admin page built in Custom UI, not UI Kit. Not a harness fault: goose showed "context: 4,342 of
  1,000,000 tokens used (0%)" and the prompt states the 300-call budget (same self-misjudgment as its 1.0 run). Its
  scoring took 2.5 h+ (a stub app makes every UI section wait out its timeout over 6 virtual hours x 3 seeds) — HARNESS
  TODO: bound scoring time for a non-functional app (abort a lane once its surface is proven absent).
- HARNESS TODO: run_build's in-process forge2 scoring does not take the per-host score lock the CLI takes.
- 05:0x SOL PROVISIONAL RESULT: 0.9618 (seeds 0.9607/0.9629/0.9657), no critical — NOT CHALLENGED (owner's acceptance
  test failed). v2 rows ~0.74 of 0.75: busiest background hour 339 of 1,680 points (optimum 262), boot ~40 KB of 150 KB,
  migration 86/86, all forged webhooks/admin attempts refused. Losses: UI details, Rovo action exactness, and the
  queue-delegation scorer artifact (r_as_app / r_completes_in_timeout vacuous — Pareto's 1.0 defect class, still in 2.0).
  Verdict: /Volumes/AI-workhorse/runs/forge2-pilot/20261010-0357-sol/openrouter-forge2-sol-r0/verdict.json.
- 05:1x HAIKU 5.5 launched on the same (lean) version as a second data point (~$1-2). Calibration run stopped (would go
  stale). wf forge2-harden: S1 concurrent deliveries under a deterministic reads-first schedule (check-then-act races),
  S2 eventually consistent KVS queries (5 virtual s), S3 lineage-based scheduled-run rows; golden adapted; race mutant;
  gate + calibration on branch forge2/hardened; then Sol reruns on the hardened version.
- 04:3x SOL FINISHED its build on its own: 176 of 300 calls, ~36 min, $2.90 billed (99 % cached). Its own stated
  limitation: "silent estimation-field switch times are inferred when first observed; the board API provides no
  historical switch timestamp." run_build is auto-scoring (pre-calibration scorer = provisional).
- 03:57 GATE PASSED (substance): golden 0.9907 / 0.9911 / 0.9908 on 3 seeds — every row 1.0 except the two economy rows
  awaiting calibration; starter 0.2697; deterministic. Contract red team: 24 defects, all fixed (19 contract, 5 graders).
  Final task+scorer: branch forge2/gate-fix e3f92d05a (worktree wf_1d0643c4-2d8-8 — FROZEN while pilots run from it).
- 03:57 SOL PILOT LAUNCHED: run_build --forge2, openai/gpt-6.1-sol, budget 300, out
  /Volumes/AI-workhorse/runs/forge2-pilot/20261010-0357-sol (log ~/goose-builds/loop-state/forge2-pilot-sol.log).
  Parallel: wf forge2-calibrate-mutants (branch forge2/calibrate: 5 golden seeds -> CALIB pinned -> starter + 9 mutants).
  The pilot's auto-score is provisional (pre-calibration scorer); the result is the CLI re-score with forge2/calibrate.
  Then Haiku 5.5 (forge2_pilot.sh anthropic/claude-haiku-5.5 haiku 8853). Vigil cron every 10 min.
- 01:25 INTEGRATED: branch forge2/integrate (11 branches + 16 integration commits, local worktree wf_2b463dc6-45f-1),
  every suite green; golden 0.9573 on one seed (main gap: the estimate-after-field-switch rule unstated), starter 0.2694
  (the "no v2 surfaces" band is 0.30, not 0.599 — SPEC §4/§5 contradicted; decided 0.30). Report: scratchpad
  forge2-int/INTEGRATION.md; scorer command integ/score.sh. Pilot launcher: ~/goose-builds/loop-state/forge2_pilot.sh.
- 01:30 wf forge2-fix-and-gate: fixers (estimate rule, checks economy classes, probe R3 window, site gaps), nine real
  mutants (forge2/mutants2), contract red team -> merge -> GATE (golden x3 seeds = 1.000, starter <= 0.30, each mutant
  loses exactly its rows). Then Sol pilot (forge2_pilot.sh openai/gpt-6.1-sol sol), then Haiku 5.5.
- 00:1x note CORRECTED live (771 chars): "top four by 0.01" + Pareto added as too low — its hourly job hands Jira reads to a
  queue worker, which 1.0's r_as_app / r_completes_in_timeout don't count (vacuous 0s; contract allows it). Found by the
  blog-correction red team and the frontier-behaviour report independently; verified on the local tree a221226b.
- 00:1x two blog corrections published (callout first in body, revision-guarded, read back): the Sonnet-vs-Opus post
  (defects B + A, the emulator's notices over Sonnet's buttons) and the Flash-vs-Max-Prime post (Max Prime/Muse too
  high; Omni Flash posted none, not two; Sonnet gains rather than drops; Flash's explain button covered). Text in
  scratchpad rt-blogcorr/fixes-final.txt.
- (history) Owner 2026-10-09 21:2x: "freeze forge 1.0 with a note, 2.0 replaces it" -> wf forge10-freeze-note running (map site+app,
  note drafted + attacked on facts and reading, `note` field on benchmarkState + render on board/run pages on site branch
  `forge10-freeze-note`, two diff reviews); then I do the Sanity write (frozen:true + note; familyCurrent stays until the
  2.0 flip), push, revalidate, verify live.
- Owner 21:2x on Mistral: "it's a petty that mistral can't be benchmarked". Route found: goose's built-in `mistral` provider
  (MISTRAL_API_KEY, in the benchmark's CLOUD_PROVIDER_LABELS) talks to Mistral's own API, bypassing OpenRouter's 120 s idle
  cut. Needs a Mistral key from the owner; probe a big tool call on it before any paid run.
