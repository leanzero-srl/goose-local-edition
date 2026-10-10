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
| 7 | PILOT (paid) — ONE run of GPT-6.1 Sol through the desktop app on 2.0 = THE ACCEPTANCE TEST. Owner 2026-10-09 20:1x: "make sure you make a test with whichever is cheaper, gpt-6.1 sol or opus 5.5 as they're both real top dogs and see if they get challenged. if that happens we're good". Sol is the cheaper: half Opus on every token class ($2/$10/$0.10 cache-read vs $4/$20/$0.20 per M, OpenRouter 2026-10-09) and Forge 1.0 Sol = $0.72 / 13 min vs Opus 26 min. "Challenged" = Sol loses real points on stated engineering requirements (not on harness faults or unstated rules); if Sol cruises, harden 2.0 before anyone else runs it | — |
| 8 | RERUN every model that ran Forge 1.0, on one goose release | — |

## Line state

- 2026-10-10 13:34 RELIABILITY v2, PER GROUP (branch `forge2/final` = forge2/severity + the hardening gate; not merged,
  not pushed). The per-row rule below was proven unfair by an independent review and reproduced on Sonnet's verdict:
  0.7473 as graded, 0.7078 with `u_widget_numbers` FIXED, 0.7747 with it made worse; count-once exemptions let an
  absent or idle surface beat a working one with failures; a fired critical paid twice; weight-0 `k_v2_surfaces`
  multiplied. Now (SPEC §4): reliability = max(0.25, Π over the 18 groups — every tier but E — of (1 − 0.10 × the
  shortfall of the group's worst eligible row)); eligible = scored, weighted, critical not fired; vacuous and absent
  rows count their 0; no ROOT_BLOCKS folding (it stays for the criticals and forge2_controls); cross-group effects of
  one cause are not folded. Verdict block `{multiplier, k, floor, floored, defects [{tier, check, score, factor}],
  folded {tier: [check]}, priced_as_critical}`. Recomposed (old per-row -> per group): Sol hardened 0.8247 -> 0.8248,
  Sonnet hardened 0.7473 -> 0.7450, Sol lean 0.7564 -> 0.6990 (its two vacuous R rows now count), Haiku 0.0845 ->
  0.0998 (off the floor, 14 groups); the six g3 reference verdicts keep reliability 1. Monotonicity sweep (every
  non-E row over 0/.25/.5/.75/.9/1): 0 violating moves on all ten verdicts, the per-row rule 10/14/11/5 on the
  pilots. Fixtures for the desktop and the site: scratchpad `sev2-out/recomposed-<name>.json` (the earlier `sev-out/`
  ones carry the retired block). Also: deploy-readiness poor-practice findings are no longer priced in
  k_manifest_semantics (M3 "every consumer queue is pushed to" is not in the contract; M9/P3 are, and stay priced by
  k_dashboard_widget / l_scopes) — no kept verdict moves (M3 passes on all ten).
- 2026-10-10 12:17 RELIABILITY (branch `forge2/severity`, off `forge2/prerun` 9ac3ed765; not merged, not pushed). Owner's
  acceptance rule: Sonnet 5.5 near 0.9 (0.9628 with 13 failed guarantee rows) is not discriminating. SPEC §4 now:
  `final = earned × critical multiplier × reliability`, reliability = max(0.25, Π (1 − 0.10 × counted shortfall)) over
  the scored rows outside E below 1; unavailable and vacuous rows never count; a row counts only the shortfall beyond
  its failed ROOT_BLOCKS root's; one missing surface counts once. Kept verdicts recomposed from their stored rows
  (`score_forge2.py --recompose <verdict.json>`, no probe): Sol 0.9618 -> 0.7564, Sonnet-hardened 0.9628 -> 0.7473,
  Haiku 0.3378 -> 0.0845 (the floor), the reference's kept verdict 0.9907 -> 0.9907 (reliability 1). Sol and Haiku were
  graded by the pre-hardening scorer and probe: their rows are not the hardened scorer's. A run scored by an app
  bundle that predates this branch composes by the old rule: recompose its verdict. Open (panel work, not done here):
  the desktop's ScoringDetail prints the recorded composition inputs (inner, excellence, critical multiplier) and no
  reliability, so a 0.96 inner beside a 0.75 final reads unexplained until it shows the verdict's `reliability`
  block. `rawScore` carries reliability, so `score` is still the capped `rawScore` (the site's side of a publish was
  not read here).
- 2026-10-10 01:2x INTEGRATED on branch `forge2/integrate` (11 package branches merged, every integration note applied
  or listed in the integrator's report). Golden v2 on scoring seed 0123456789abcdef: 0.6649 -> 0.9573 (no critical; v2
  0.7229 of 0.75), scoring wall ~436 s per seed alone. Untouched starter: 0.3716 under the old 0.599 band -> band
  lowered to 0.30 (SPEC §4/§5 conflict) -> 0.2694 through `run_build --forge2` (e2e: 5/6 PASS; the 6th compared a
  contract I edited mid-run). Open: golden 'added +<points>' after a field switch (contract wording), v1 economy
  optimums without 2.0 work, R3's 900 s stimulus blocked by the panel read on some seeds, 2.0 mutants, 3-seed gate,
  calibration, sb7.x/forge manifests to refreeze after P11's shared bench edits.
- Forge 1.0 queue entries: stopped/blocked (owner 19:2x). Gauntlet continues (Mistral Large 4 running 19:30).
- DONE 2026-10-09 23:55: Forge 1.0 FROZEN with the reviewed 654-char note (site af5162c on master; `register-forge.mjs freeze --live`: bstate-forge-1-0 frozen:true + note, still forge familyCurrent until the 2.0 flip; 32/32 pages render it; POST 409). Installed apps show "no single available Forge benchmark" for Forge until a 2.0 app ships. Rewrite the note in the past tense at the 2.0 flip (`register-forge.mjs note`).
- (history) Owner 2026-10-09 21:2x: "freeze forge 1.0 with a note, 2.0 replaces it" -> wf forge10-freeze-note running (map site+app,
  note drafted + attacked on facts and reading, `note` field on benchmarkState + render on board/run pages on site branch
  `forge10-freeze-note`, two diff reviews); then I do the Sanity write (frozen:true + note; familyCurrent stays until the
  2.0 flip), push, revalidate, verify live.
- Owner 21:2x on Mistral: "it's a petty that mistral can't be benchmarked". Route found: goose's built-in `mistral` provider
  (MISTRAL_API_KEY, in the benchmark's CLOUD_PROVIDER_LABELS) talks to Mistral's own API, bypassing OpenRouter's 120 s idle
  cut. Needs a Mistral key from the owner; probe a big tool call on it before any paid run.
