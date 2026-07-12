# Full exploratory (UI-dispatched) — 2026-07-12

Fresh complex apps, one per archetype, each with a TIGHT explicitly-named command contract (to measure the
spec-contract-drift failure the last assessment found). Dispatched FROM THE UI (typed into the app chat →
the app spawns the swarm run → visible live in the verbose run panel). Fixed binary (captures reasoning +
tool output + judge verdicts). Sequential on the 3-node fleet. Each verified by RUNNING it + its tests.

| app | archetype | lang | status | LOC | contract fidelity | verdict |
|-----|-----------|------|--------|-----|-------------------|---------|
| inventory | data app | Python/SQLite | done | 808 | **spec-EXACT** | STRONG PASS |
| csvql | algorithmic engine | Python | done | 936 | CLI crashes | **FAIL** |
| kvstore | systems tool | Rust | done | 253 (stub) | incomplete | **FAIL** |

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

### inventory — STRONG PASS (spec-exact CLI)
808 LOC, 10 modules (db/models/cli + commands/{product,supplier,movement,reporting,io}). 22/22 pytest pass.
Golden spec-contract check (the point of this run): the EXACT documented commands + flags all work —
`product add --sku --name --category --price --qty` → "added W-01"; duplicate sku rejected (exit 1);
`receive`/`ship`/`adjust`/`movements`; `ship` exceeding qty rejected (exit 1); qty math correct (100+50-30=120);
`report value` = 1198.80 (120×9.99); global `--db` before the subcommand. NO CLI drift — a clear improvement
over the last overnight assessment (tracker/sheet invented their own CLI). LIKELY CAUSE: this spec listed the
exact command names/flags with "match these EXACTLY"; being explicit in the spec lifts contract fidelity.
Minor: movements shows "ship +30" (sign cosmetic; type column already says ship).

### csvql — FAIL (broken CLI slipped through; the deep archetype + a verification gap)
936 LOC, clean architecture (tokenizer/parser/ast/evaluator/cli). But the CLI CRASHES on every query:
`cli.py:50` does `row.values()` while the evaluator returns rows as LISTS → AttributeError on all queries.
Classic cross-module CONTRACT DRIFT (cli-worker and evaluator-worker disagreed on the row type). The 3
"passing" unit tests only exercise the evaluator internals, so they never hit the broken CLI glue.

WHY IT SLIPPED THROUGH — two root causes (read from the .swarm logs):
1. **No end-to-end verification ran.** GOOSE_SWARM_SMOKE / GOOSE_SWARM_COMPLETE gate on an explicit env var
   or "assured" mode; a UI-dispatched build (swarm PROVIDER) sets neither, so smoke+complete were OFF and
   nothing ran `python3 -m csvql`. My CLI overnight runs set GOOSE_SWARM_SMOKE=1, so they DID catch such
   crashes — UI builds silently got LESS verification. **FIX: the swarm provider now enables the smoke
   end-to-end gate for UI builds.**
2. **The scheduler got stuck.** The `parser` worker stalled 420s (weak model on a hard module) → retry →
   `scheduler_stuck {remaining:1}`; integrate-verify never completed. The evaluator worker also
   over_read→looped→salvaged. The parser+evaluator are the deep archetype's hard core (last time `sheet`
   failed here too). Deeper issue; noted, not fixed this pass.

VERDICT: FAIL. The value is the finding: UI builds must run the end-to-end gate (fix shipping now), and the
deep algorithmic archetype still strains the fleet on the parser/evaluator.

### kvstore — FAIL (incomplete stub; workers looped on the hard Rust modules)
Only 253 LOC / 3 files (log/index/main) vs the ~1000 target — missing store/cli/commands entirely. The run
report: 4 of 6 subtasks FAILED (cli-and-commands, store-module, integration-tests, integrate-verify); only
index-module + shared-types-and-log completed. main.rs is a stub (no command handling). The `store-module`
worker repeatedly hit judge `over_reading` ("produced no file yet, STOP deliberating") — it looped without
writing. High-variance: last time a Rust systems tool (vcs) STRONG-passed; this harder log-structured spec
(compaction, index replay) stalled the fleet.
Note: the new smoke gate RAN but was vacuous for Rust — py_files=0, entry_ok=True without building/running
the cargo binary. So the smoke fix helps Python builds but not Rust (follow-up below).

---
## FINAL CRUNCH — 1 STRONG PASS, 2 FAIL, and 4 concrete fixes

| app | archetype | verdict |
|-----|-----------|---------|
| inventory | data (Python/SQLite) | STRONG PASS — spec-exact CLI, 22/22 tests |
| csvql | algorithmic (Python) | FAIL — broken CLI (cross-module contract drift), no end-to-end gate |
| kvstore | systems (Rust) | FAIL — incomplete stub, workers looped on the hard modules |

### Observations → fixes
1. **UI-dispatched builds skipped end-to-end verification** (smoke/complete gates need an env var / assured
   mode the app never set) → a crash-on-every-command CLI shipped as "done". **FIXED**: the swarm provider
   now enables GOOSE_SWARM_SMOKE for UI builds (commit in this session). [biggest fix]
2. **Builds ran in $HOME**, so workers wandered into sibling projects (integrate-verify tested ~/wc2 not the
   app). **FIXED** for this run via `--dir` isolation; PRODUCT BACKLOG: builds should not default to $HOME.
3. **Run panel had 5 nested scrollbars** → **FIXED**: panel flows, chat scrolls as one (user feedback).
4. **The fleet is archetype-dependent + high-variance**: data/CRUD (inventory) passes cleanly and hits the
   spec contract EXACTLY when the spec names commands explicitly; the deep algorithmic (csvql parser/
   evaluator) and a hard systems spec (kvstore log-store) both stalled — workers over_read/loop/stall (420s
   timeout) on the hard modules, and retry/salvage isn't enough. The judge DETECTS it (over_reading/looping
   verdicts) but can't rescue it. This is the standing ceiling.

### Follow-ups (backlog, not done this pass — bigger/uncertain)
- Extend the smoke gate to Rust (cargo build + run the binary); it's Python-centric today (kvstore slipped).
- The parser/evaluator + log-store stalls: the corrective loop (retry, salvage) doesn't recover a looping
  worker on hard modules. Candidate: a harder-model escalation or a stricter "write-first" nudge earlier.
- Spec explicitness lifts contract fidelity (inventory proof) — worth encoding into the planner's subtask
  specs (derive exact command signatures from the brief).
