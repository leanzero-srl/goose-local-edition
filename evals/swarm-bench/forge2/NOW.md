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

- Forge 1.0 queue entries: stopped/blocked (owner 19:2x). Gauntlet continues (Mistral Large 4 running 19:30).
- Owner 2026-10-09 21:2x: "freeze forge 1.0 with a note, 2.0 replaces it" -> wf forge10-freeze-note running (map site+app,
  note drafted + attacked on facts and reading, `note` field on benchmarkState + render on board/run pages on site branch
  `forge10-freeze-note`, two diff reviews); then I do the Sanity write (frozen:true + note; familyCurrent stays until the
  2.0 flip), push, revalidate, verify live.
- Owner 21:2x on Mistral: "it's a petty that mistral can't be benchmarked". Route found: goose's built-in `mistral` provider
  (MISTRAL_API_KEY, in the benchmark's CLOUD_PROVIDER_LABELS) talks to Mistral's own API, bypassing OpenRouter's 120 s idle
  cut. Needs a Mistral key from the owner; probe a big tool call on it before any paid run.
