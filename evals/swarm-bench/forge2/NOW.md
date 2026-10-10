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
