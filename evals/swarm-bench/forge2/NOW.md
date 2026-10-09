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
| 1 | UNDERSTAND — 1.0 machinery reuse map, frontier behaviour on 1.0 (Haiku/Pareto transcripts + Sol/Opus rows), real-Forge fidelity tooling, desktop/site integration | running (wf forge2-understand) |
| 1b | RESEARCH — platform modules/changelog since 2026-07; mandate topics (tiers, UI Kit, boot speed, security, LLM, robustness), every design-critical claim re-verified | running (agent + wf forge2-research-mandate) |
| 2 | DESIGN — judge panel of independent full designs, scored on frontier difficulty, fairness, offline gradeability/fidelity, mandate coverage, build feasibility, cost; synthesis | next |
| 3 | RED-TEAM — multi-lens attack on the design (unstated requirements, emulator fidelity, gameability, everyone-scores-0, 1.0 defect classes, security-claim correctness), each finding verified, fixes | — |
| 4 | PRE-BUILD DIFFICULTY PROOF — fresh Opus-class agents plan the app from the contract alone; graded against the hidden check list; harden until frontier plans miss real things | — |
| 5 | BUILD — mock Jira/Confluence + rate tiers, emulator (concurrency, virtual-time limits, UI Kit host, webtrigger, admin), real lint in the kit, probe, scorer, golden, alt app, mutants | — |
| 6 | GATE — golden 1.0, alt ≥ 0.95, mutants exact, empty ≈ 0, severity selftest, calibration | — |
| 7 | PILOT (paid) — Opus 5.5 + GPT-6.1 Sol through the desktop app on 2.0: the proof | — |
| 8 | RERUN every model that ran Forge 1.0, on one goose release | — |

## Line state

- Forge 1.0 queue entries: stopped/blocked (owner 19:2x). Gauntlet continues (Mistral Large 4 running 19:30).
- Open owner question: fix + re-score the Forge 1.0 board (Sonnet/Omni need the laptop) or freeze 1.0 with a note.
