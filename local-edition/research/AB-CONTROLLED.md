# Controlled A/B — qwen vs qwopus on the SAME 5 apps (removes the disjoint-app confound)

Both run on the FROZEN binary `/tmp/goose-ab-fe6c1ce7b` (predates M2/M3/M6/M7), 3-node fleet, identical spec
pulled from the original qwen run's `.swarm` jsonl. Reviewed QUALITATIVELY: read source, traced correctness,
RAN the real feature on a golden input, judged whether tests assert real values. Scores 1–10.

## App 1 — chaos-fern (Barnsley fern via chaos game / IFS)  ← FIRST DATA POINT

### qwopus
- **Ran it** (`python3 -m chaos_fern fern --width 50 --height 32 --iter 50000`): renders a **correct Barnsley
  fern** — central stem, self-similar fronds curving out, tapering crown. Golden visual check PASS.
- **Default params correct**: `chaos_fern/ifs.py builtin_fern()` = the canonical coefficients exactly:
  stem (0,0,0,0.16,0,0,.01), leaflet (.85,.04,-.04,.85,0,1.6,.85), left (.2,-.26,.23,.22,0,1.6,.07),
  right (-.15,.28,.26,.24,0,.44,.07). Weights normalized in `__post_init__`.
- **One clean implementation**: a single `ifs.py` (frozen dataclasses AffineMap/IFSMap/IFSSystem + builtins +
  file parser). 6 small modules (ifs/chaos_game/renderer/cli/__main__/__init__). Proper package.
- **Tests**: 85 pass (0.07s) and assert GOLDEN values — weight normalization to 1/3·2/3, exact coefficients,
  empty-maps raises. Not just "runs".
- **Scores**: correctness **9**, test_depth **8**, quality **9**, spec **9**.

### qwen (from loop42 + re-read now)
- **Default path BROKEN by corrupted params**: `cli.py` explicitly `importlib`-loads `builtins.py`, whose fern
  is garbled — map order swapped and the large left frond uses translation `f=0.44` instead of `1.6`, plus
  coefficients (0.28,-0.26,-0.24,0.24) vs the correct (0.2,-0.26,0.23,0.22). The rendered "fern" is malformed.
- **DUPLICATE implementation that drifted**: a SEPARATE `ifs_core.py._make_fern()` has the *correct* params —
  but it is UNUSED; the wired default path uses the corrupted `builtins.py`. Classic lesson #1 + #2 together.
- **Scores**: correctness **4**, test_depth **6**, quality **5**, spec **6**.

### Head-to-head verdict (chaos-fern)
**qwopus decisively better on the SAME app.** The earlier "qwopus modestly better but confounded by disjoint
apps" worry does NOT hold here: given the identical spec, qwopus produced a correct, single, verified fern;
qwen shipped two diverging implementations and wired the broken one. The gap is real, not an app artifact.
This is exactly the failure mode the swarm improvements target: M2 (no duplicate impls) + M7-distill
(integrate-verify must check the DEFAULT path output is correct) would both have caught qwen's bug.

Perf: qwopus chaos-fern ran ~1h wall-clock (done 7 subtasks; slow tail on cli-entry-point/test tasks — the
exact too-big-task case M3 task-splitting now addresses). qwen was comparable-or-faster but wrong.

## App 2 — antic-turmite — A/B RUNNING (qwopus), review pending
## App 3 — logfunnel — pending
## App 4 — fsdrift — pending
## App 5 — byte-oracle — pending

## Running tally
- chaos-fern: **qwopus 9/8/9/9 vs qwen 4/6/5/6 → qwopus WINS (correctness gap decisive).**

## App 2 — antic-turmite (Langton's ant + programmable-turmite amendment)
### qwopus
- RUNS (`python3 -m antic_turmite run --width 75 --height 50 --frames 11500`): produces a CORRECT Langton's
  ant — the chaotic core + the emergent diagonal HIGHWAY clearly form. Golden visual check PASS.
- 10-module package (ant/grid/highway/rendering/heatmap/cli/__main__ + turmite_ant/turmite_rules for the
  AMENDMENT) — clean, modular, AND it implemented the programmable-turmite amendment on top of the base.
- highway.py has a REAL detection algorithm (scans periods max->min, confirms N consecutive cycles by
  constant displacement + matching direction) — not a hardcoded 104.
- GAPS (honest): (a) the `run` command does NOT report the detected highway/period — the detector is built
  but UNWIRED into the default path (lesson #5). (b) 2/42 tests FAIL (test_highway detection). (c) spec drift:
  `--steps` conflated with `--frames`, `--seed` dropped (defensible — Langton's ant is deterministic).
  CAVEAT: the run's 2 test tasks STALLED and I cut it pre-integrate-verify, so those failures/unwired report
  were never reconciled — a full run's integrate-verify might have fixed them.
- Scores: correctness 6, test_depth 7, quality 8, spec 5.
### qwen (loop43)
- NO runnable CLI / entry point (lesson #8) — the simulation could not be executed at all to verify.
- Scores: correctness 2, test_depth ~4, quality 4, spec 3.
### Head-to-head verdict (antic-turmite)
**qwopus WINS decisively on runnability + scope** (runs + correct core + amendment vs qwen's non-runnable),
but NOT flawless: the headline "detect + report the highway" is unwired and 2 tests fail. This is exactly the
case M3 (split the slow test tail so the run isn't cut), M5 (idle pre-review catches the unwired report +
failing tests before integrate-verify), and the M7 integrate-verify-correctness distillation all target.

## Running tally
- chaos-fern: qwopus 9/8/9/9 vs qwen 4/6/5/6 → qwopus WINS (decisive).
- antic-turmite: qwopus 6/7/8/5 vs qwen 2/4/4/3 → qwopus WINS (runnable+scope; real gaps from a cut run).

## App 3 — logfunnel (stage-pipeline DSL over log lines)
### qwopus
- INCOMPLETE / STALLED. Built 5 clean, partially inter-wired modules (lexer/parser/stages/renderer/models;
  renderer imports .models) BUT no entry point / dispatcher / CLI at all, and no tests completed — the
  `stages-renderer` task STALLED (zero writes for >6 min across multiple cycles; the judge re-dispatched it
  twice but could not unstick it), and the wiring/CLI subtasks never ran. I cut the run.
- So there is NO runnable pipeline — the core spec deliverable (run a `|`-separated stage pipeline over log
  lines) is absent. Scores: correctness n/a (not runnable), test_depth 0, quality 6 (clean modules), spec 2.
### qwen (loop44)
- Also INCOMPLETE: built components but NO dispatcher wiring them (lesson #5) + spec'd Rust, built Python
  (lesson #7). But its run TERMINATED. Scores: correctness 2, test_depth 3, quality 4, spec 2.
### Head-to-head verdict (logfunnel)
**~DRAW — both failed to deliver a runnable pipeline.** qwopus's modules are cleaner and more inter-wired,
but it STALLED and never produced an entry point (worse in that it didn't self-terminate); qwen terminated
but left the pipeline unwired. This is the HARDEST, most-decomposed app and the clearest evidence FOR the
features built tonight: M3 task-splitting would break the too-big stalled `stages-renderer` task, and the
judge's plain re-dispatch was NOT enough to recover it. An honest non-win for qwopus.

## Running tally
- chaos-fern: qwopus 9/8/9/9 vs qwen 4/6/5/6 → qwopus WINS (decisive).
- antic-turmite: qwopus 6/7/8/5 vs qwen 2/4/4/3 → qwopus WINS (runnable+scope; real gaps).
- logfunnel: qwopus ~n-a/0/6/2 vs qwen 2/3/4/2 → DRAW (both incomplete; qwopus stalled, qwen unwired).

## App 4 — fsdrift (filesystem snapshot / drift-detection CLI)
### qwopus
- CORE modules are SOPHISTICATED: diff() classifies added/removed/modified/TOUCHED(same hash,diff mtime)/
  MOVED(hash-match across sets)/dup-moved; walk_and_write IMPLEMENTS exclude_globs + follow_symlinks (the
  exact flags qwen left unimplemented). 45 unit tests pass.
- FATAL INTEGRATION BUG (found by RUNNING the real feature): snapshot.py writes mtime as an ISO-8601 STRING
  but diff.py parse_manifest does float(mtime_str) expecting a NUMBER -> diff() CRASHES (ValueError) on ANY
  real snapshot->diff. The 45 tests PASS because they test snapshot and diff IN ISOLATION with their own
  fixtures, NEVER the snapshot->diff pipeline. Textbook 'tests pass but the integrated feature is broken'.
- Also: cli-entrypoint + verify-module + integrate-verify all FAILED (terminal) -> NO runnable CLI, and
  integrate-verify (which would have run the pipeline) never executed to catch the mtime mismatch.
- Scores: correctness 3 (integrated feature crashes; no CLI), test_depth 4 (45 tests but miss the pipeline —
  smoke-tests lie), quality 6 (nice modules, broken cross-module contract), spec 3.
### qwen (loop45)
- Has a CLI but snapshot crashes with a TypeError (lesson #3); --exclude/--follow-symlinks unimplemented
  (lesson #7). Scores: correctness 3, test_depth 3, quality 4, spec 3.
### Head-to-head verdict (fsdrift)
**~DRAW — both crash on the real end-to-end feature.** qwopus's individual modules are clearly better
(richer diff, flags implemented, more tests) BUT a fatal snapshot<->diff format mismatch — HIDDEN by
isolation-only tests — breaks the actual tool, and it shipped no CLI. qwen runs but crashes on its primary
command. Neither delivers a working drift tool. The mismatch is exactly what an END-TO-END integrate-verify
+ M5 idle pre-review (run the pipeline, not just the unit tests) would catch.

## Running tally (4/5)
- chaos-fern: qwopus 9/8/9/9 vs qwen 4/6/5/6 -> qwopus WINS (decisive).
- antic-turmite: qwopus 6/7/8/5 vs qwen 2/4/4/3 -> qwopus WINS (runnable+scope, gaps).
- logfunnel: qwopus ~/0/6/2 vs qwen 2/3/4/2 -> DRAW (both incomplete; qwopus stalled).
- fsdrift: qwopus 3/4/6/3 vs qwen 3/3/4/3 -> DRAW (both crash end-to-end; qwopus nicer modules, fatal integ bug).

## App 5 — byte-oracle (content-based file-type sniffer)
### qwopus
- CLEAN WIN. RUNS (`python3 -m byte_oracle`): sniffs by CONTENT not extension — a PNG mislabeled .txt -> png,
  an ELF as .dat -> elf, a real .gz -> gzip, GIF89a -> gif (4/4 correct), with an expected-vs-actual
  extension-MISMATCH column. --recurse on a NESTED dir works (pdf found in nested/deep/doc.xyz) — exactly
  where qwen crashed (lesson #4). ZIP refinement distinguishes docx/apk/jar. 99 tests pass with GOLDEN
  assertions (refine_zip_type==docx/apk). Clean modules: detector/signatures/cli/__main__/zip_refinement.
- Scores: correctness 9, test_depth 9, quality 9, spec 9.
### qwen (loop46)
- Ran but --recurse CRASHED on nested dirs (lesson #4). Scores: correctness 4, test_depth 5, quality 5, spec 4.
### Verdict: qwopus WINS decisively (runnable, content-correct, nested recurse + ZIP refinement vs qwen crash).

# ============ FINAL CONTROLLED VERDICT (5/5 apps, SAME specs, frozen binary) ============
| app           | result | qwopus (corr/test/qual/spec) | qwen (corr/test/qual/spec) |
|---------------|--------|------------------------------|----------------------------|
| chaos-fern    | qwopus | 9/8/9/9                      | 4/6/5/6                    |
| antic-turmite | qwopus | 6/7/8/5                      | 2/4/4/3                    |
| logfunnel     | DRAW   | 2/0/6/2 (stalled, no CLI)    | 2/3/4/2 (no dispatcher)    |
| fsdrift       | DRAW   | 3/4/6/3 (pipeline crash)     | 3/3/4/3 (CLI crash)        |
| byte-oracle   | qwopus | 9/9/9/9                      | 4/5/5/4                    |
| **MEAN**      | 3W-2D-0L | **5.8 / 5.6 / 7.6 / 5.6**  | **3.0 / 4.2 / 4.4 / 3.6**  |

## Honest narrative (answering: is qwopus actually better than qwen?)
YES — controlled, SAME apps (disjoint-app confound REMOVED), qwopus is higher on EVERY dimension (notably
quality 7.6 vs 4.4 and correctness 5.8 vs 3.0), with 3 wins, 2 draws, 0 losses. BUT the win is concentrated
in CLEAN COHESIVE apps (chaos-fern, antic-turmite, byte-oracle) where qwopus is decisively better — runnable,
content-correct, advertised flags working. On the two BIG multi-module apps (logfunnel, fsdrift) it DRAWS:
qwopus writes nicer individual modules but loses to cross-module integration failures — a snapshot<->diff
contract mismatch hidden by isolation tests (fsdrift), a stalled too-big task + missing dispatcher
(logfunnel, where it actually STALLED and had to be cut). Speed: similar/slow on both (qwopus's long tails).
Stability: qwopus stalled once (logfunnel) where qwen terminated.

## What this means for the swarm (the draws validate tonight's work)
The two draws are NOT noise — they are precisely the failure classes the features shipped tonight target:
- M3 task-splitting -> the logfunnel stall (a too-big stages task plain re-dispatch couldn't recover).
- M5 idle pre-review + M7 integrate-verify-correctness -> the fsdrift snapshot<->diff mismatch + unwired
  entry points (run the END-TO-END pipeline, not isolation-only unit tests).
So qwopus is the better model AND the swarm's next gains are exactly in the multi-module integration regime,
which M3/M5/M6/M7 address. M4 will test whether enabling them actually converts a DRAW into a WIN.
