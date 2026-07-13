# Cycle-1 Fix Plan (implement in task #58, after all 15 apps explored)

Ordered by CONFIDENCE + value (never by effort, per the rules). Each ships gated (cargo fmt + build +
clippy -D warnings + cargo test) and committed separately. All are default-safe (no behavior change on the
cloud/upstream path where the flags are off / the language paths don't apply).

## FIX 1 — Scheduler salvage relaxes dependents (BACKLOG #7). Confidence: HIGH.
File: crates/goose-swarm/src/scheduler.rs
Problem: the SUCCESS path (lines ~579-590) decrements each dependent's `indegree_remaining` and promotes it
to Ready at 0; the SALVAGE branch (~line 1159) sets state=Done but SKIPS that loop → dependents orphaned →
scheduler_stuck (expense: cli-entry never dispatched → no `python -m spend`).
Change:
  1. Extract the 579-590 loop into `fn relax_dependents(&mut self, tid: &str)`:
       let dependents = self.dag.dependents.get(tid).cloned().unwrap_or_default();
       for d in dependents {
           let nd = self.dag.tasks.get_mut(&d).unwrap();
           if nd.indegree_remaining > 0 { nd.indegree_remaining -= 1; }
           if nd.indegree_remaining == 0 && nd.state == TaskState::Pending {
               nd.state = TaskState::Ready;
               let fan_out = nd.fan_out;
               self.ready.push(Ranked { fan_out, id: d });
           }
       }
  2. Call it from the success path (replace the inline loop) AND from the salvage branch right after
     `self.dag.tasks.get_mut(tid).unwrap().state = state;` when `salvage` is true (state==Done).
Test (crates/goose-swarm, scheduler_mock): task B deps=[A]; A dispatched, judged Looping with an owned file
written on disk (so salvage fires); assert A ends Done AND B becomes Ready (indegree 0) AND the scheduler
dispatches B — NO scheduler_stuck. Mirror the existing salvage tests in scheduler.rs / judge.rs.

## FIX 2 — Language-aware done/smoke gate; Rust no longer a free pass (BACKLOG #8, reinforces #3). Conf: HIGH.
File: crates/goose-cli/src/commands/swarm.rs (the smoke/complete phase inserted after scheduler.run, between
run_finished at ~:4002-4004 in the older map; find the fn that emits the `smoke` event).
Problem: gate is Python-only. kvstore (Rust, empty `fn main(){}`, 4/6 tasks failed incl integrate-verify)
reported `{py_files:0, entry_ok:true, tests:{pass}, findings:[]}` and shipped.
Change:
  - If Cargo.toml present: run `cargo build` (compile gate) + `cargo test` (if a tests/ target was planned,
    assert not "0 tests"); run the built binary `--help` AND one spec subcommand, assert exit 0 AND non-empty
    stdout. Flag an empty/near-empty `fn main()` (heuristic: main.rs body <= a couple tokens) as a finding.
  - REAL-COMMAND probe for Python too (fixes #3/bookclub): don't rely on `python -m pkg --help` (Click/argparse
    short-circuit --help before the ctx.obj bug); run one real spec subcommand round-trip against a temp db and
    assert exit 0. `--help` exit 0 is necessary, not sufficient.
  - ASSERT OUTPUT, NOT JUST EXIT CODE (from the tmpl capstone): `tmpl render` EXITS 0 while producing EMPTY
    output (parser/renderer shape drift). An exit-0-only gate passes a fully-broken app. The gate must run a
    spec example with a KNOWN expected output (the specs literally embed golden cases: calc `2+3*4==14`,
    jsonq `$.items[?(@.price>10)].id`, tmpl `Hello {{name}}` → non-empty) and assert the output is non-empty /
    matches. Golden spec-contract checks derived from the spec's own examples are the ground truth the model's
    tests keep dodging (bookclub CLI, timesheet entry, jsonq slice+chain, tmpl render).
  - HARD-BLOCK semantics: if integrate-verify or the CLI/entry task is in report.failed, the run is NOT
    shippable — surface loudly (and, under GOOSE_SWARM_COMPLETE, fire one corrective re-dispatch). Advisory
    smoke detects but doesn't prevent; the empty-main app proves detection alone is not enough.
Test: a fixture run report with integrate-verify failed → gate marks unshippable; a Rust dir with empty main
→ finding emitted.

## FIX 3 — CONTRACTS: shared-types the workers must import (BACKLOG #4). Confidence: MED-HIGH.
Dominant failure across archetypes (bookclub ctx.obj None; csvql rows dict-vs-list AttributeError). A planner
pre-EXECUTE step emits signature+docstring-only stubs for each module's public surface (incl. the CLI ctx
contract and the evaluator row type), injected into every worker prompt (layout_block). Keep stubs body-less.
Verify at done-gate that a real command runs (FIX 2 covers the enforcement half). GOOSE_SWARM_CONTRACTS flag.

## FIX 4 — speed_weights feed pool DISPATCH weight (BACKLOG #6, standing weights ask). Confidence: MED-HIGH.
File: swarm.rs reconcile_pool_with_fleet (~:1167-1183). When no explicit cfg.devices[].weight override, fall
back to the speed_weight matched by pattern against (host + identifier) — same haystack planner_rank uses at
:1201-1207 — before defaulting to 1. Explicit override still wins. Then worksmacstudio→w3, gabee/local→w2.
Test: speed_weights {worksmacstudio:3} + a workhorse node with no device override → pool weight 3; an explicit
device weight still wins over speed_weights.

## FIX 5 — Pool = intersection of configured+live; drop unloaded; dedup JIT ":N" (BACKLOG #1 + #2). Conf: MED.
swarm.rs run-start: DROP config devices whose model_id is not currently loaded (respect allow_model_load=false,
never cold-load); filter loaded ids whose suffix is a bare `:N` JIT duplicate when the aliased sibling is
present; map live ids back to a configured device by model_id/host, not raw prefix. Removes 400-loop noise
when a node is down and the phantom "qwopus3.6" JIT node.

## FIX 6 — Durable Playwright/extension node resolution (BACKLOG #5). Confidence: MED.
ui/desktop/src/main.ts extension env build: when an extension cmd is `npx`/`node`, resolve/prepend a node
that satisfies the version floor ahead of /usr/local/bin, so a stale system node (19.8.1) can't break npx
extensions. Immediate config workaround (cmd → /opt/homebrew/bin/npx) already applied + verified.

## Sequence
1 (scheduler) → 2 (gate) first: they convert the most unrunnable-but-good apps to runnable and stop shipping
no-ops. Then 3 (contracts) for the drift class. Then 4 (weights), 5 (pool), 6 (extensions). Rebuild the DMG
after the goose-side fixes land (milestone task #59), then cycle 2 validates on the fixed binary.

## STATUS (cycle-1 fix phase)
DONE + gated + pushed:
- #7 scheduler salvage relaxes dependents — relax_dependents helper, regression test (fails without fix). HIGH.
- #11 SPLIT enabled in provider (GOOSE_SWARM_SPLIT=1 + SPLIT_SECS=300) — fleet starvation. HIGH.
- #8 smoke_rust flags empty-output entry stub (kvstore empty main) — unit-tested. HIGH.
- #6 speed_weights shape DISPATCH weight (pool_dispatch_weight + speed_weight_for) — unit-tested. MED-HIGH.
- #4 CONTRACTS + COMPLETE enabled in provider (the built-but-off assured gates): freeze module interfaces
  (drift) + verify-by-running/fix-until-green language-aware gate (kvstore/wal/taskq would be caught+fixed).
  Folds in #3/#8's "blocking+corrective". REVIEW left off (highest cost/lowest confidence). MED-HIGH.

DISCOVERY that reframed the plan: CONTRACTS and COMPLETE (and REVIEW/SMOKE) are a pre-built ASSURED bundle
gated by swarm_gate(..., in_assured_bundle=true); the UI provider only ever set SMOKE. So the biggest cycle-1
failures were largely "quality gates were off," not "the model can't build." Cycle 2 validates them ON.

DEFERRED (robustness/cleanup, not correctness-critical for cycle-2 validation; follow-on batch):
- #1/#2 pool robustness (drop unloaded devices, dedup JIT ":N") — only bites when a node drops mid-session.
- #5 durable extension node-PATH injection — the config WRAPPER fix is applied+verified; durable is core.
- #9 worker tool-call waste (python->python3 etc.) — touches the shared shell tool, out of clean surface.
- #12 goosed reaps child swarm runs on shutdown — leak amplified by relaunch-heavy testing; cleaned by hand.
