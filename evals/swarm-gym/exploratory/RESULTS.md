# Full exploratory (UI-dispatched) — 2026-07-12

Fresh complex apps, one per archetype, each with a TIGHT explicitly-named command contract (to measure the
spec-contract-drift failure the last assessment found). Dispatched FROM THE UI (typed into the app chat →
the app spawns the swarm run → visible live in the verbose run panel). Fixed binary (captures reasoning +
tool output + judge verdicts). Sequential on the 3-node fleet. Each verified by RUNNING it + its tests.

| app | archetype | lang | status | LOC | contract fidelity | verdict |
|-----|-----------|------|--------|-----|-------------------|---------|
| inventory | data app | Python/SQLite | building… | — | — | — |
| csvql | algorithmic engine | Python | queued | — | — | — |
| kvstore | systems tool | Rust | queued | — | — | — |

## Observations

### Tool-call failures investigated (from reading the live inventory build's .swarm logs)
1. **integrate-verify verified the WRONG project (`~/wc2`)** — it ran `ls ~/wc2/` and `cd ~/wc2 && pytest`
   (a leftover demo app), not the inventory app. ROOT CAUSE: the build's working directory is **`$HOME`**, so
   the whole home tree (`~/wc2`, `~/inv`, `~/Games`, …) counts as "inside the working directory." The worker
   prompt already forbids `cd`/siblings, but `~/wc2` is a *child* of the working dir (home), not a sibling, so
   the model legitimately wandered there. This is the real issue, and it is a working-directory problem, not a
   prompt problem.
   - FIX (this run): removed the exact leftover artifacts (`~/wc2`), and the remaining builds run with
     `--dir ~/goose-builds/<app>` so each has an isolated, empty working directory (no confusable siblings).
   - PRODUCT BACKLOG: a UI-dispatched swarm BUILD defaults its working dir to `$HOME`, which is wrong — it
     dumps `~/inv`, `~/csvql`, … into the user's home AND lets workers wander the whole home tree. The app
     should build in a dedicated project dir, not home. (Bigger change; logged, not done here.)
2. **supplier-commands ran a non-idempotent inline test** ("supplier 'Acme' already exists") — the weak model
   wrote its own verification script without a fresh temp DB; it recovered. Benign model-noise.
3. **reporting-io checked a file before writing it** (`wc reporting.py` → not found). Transient; recovered.

Net: only #1 is a real defect. It is a working-directory hygiene issue, fixed for the remaining builds.

## Fixes shipped this session
- **Run panel: removed 5 nested scrollbars** — the verbose panel now flows at natural height and the chat
  window scrolls as one (user feedback: "too many scroll bars"). Commit d04c44466.
- **Working-dir isolation** for the exploratory builds (`--dir ~/goose-builds/<app>`).
- (Backlog) UI-dispatched builds should not default to `$HOME`.
