---
paths:
  - "crates/goose-cli/src/commands/swarm.rs"
---

# swarm.rs — the engine. 42,000 lines, and four ways to break it silently.

Read `EXPERIMENTS-LEDGER.md` before proposing a change here. Most obvious improvements have been tried.

## NO CAPS. This is Mihai's hardest rule and it has been broken by accident twice.

No wall clock, turn ceiling, retry count or volume limit may bound model work. Local models are slow;
that is expected, not a fault. Every terminator must be progress-based or live in the transport.

- `effective_idle_budget()` returns uncapped for ANY input and has a test that says so
  (`no_configured_timeout_can_ever_bound_a_call`). `worker_timeout_secs: 420` and
  `planner_timeout_secs: 900` are still in `config.yaml` and are DEAD — they arrive as `idle_secs` and
  are ignored. Do not "restore" them.
- The sink cap was deleted once and came back unguarded, because `uncapped()` was removed in the same
  purge. It cut `integrate-verify` at exactly 1800s and the run logged `status=done` — a truncated call
  and a finished one written identically into the row every verdict is read from.

## AUDIT FOR DEAD PHASES, NOT ONLY FOR CAPS

`let proxy_yes = !benchmark() && (round == 0 || last_round_promoted);` was false for EVERY round under
`GOOSE_SWARM_BENCHMARK=1`, so the repair-continue ask could only be answered no and **REPAIR had never run
in any measured run** — r0 ended with 29 criticals and `complete_fix_dispatched: 0`. Every local
benchmark number this project published was a pre-repair score.

It survived because its own comment three lines below described the opposite: *"round 0 buys round 1
because proxy_yes is true at round 0"*. A comment stating the intent is what stops the next reader
checking the expression.

**So when auditing this file, ask "is this reachable?" as well as "is this capped?".** A no-caps sweep
passes happily over a phase that never executes. The check that finds these: for any flag or mode the
benchmark sets, grep for it in boolean expressions and ask what the expression evaluates to WITH the flag
on — which is the only configuration we ever measure.

## `"integrate-verify"` is an exact-equality string test in five live places

`patch.rs:254`, eight sites here, 34 in `scheduler.rs`, 16 in `useSwarmRun.ts`, plus the bench detectors.
Renaming it, or letting a model name the join, breaks replan suppression with no compiler error.
**The sink must own NO files** — `scheduler.rs:2603` relaxes a dependent through an upstream failure only
`if n.spec.owned_files.is_empty()`, so a file-owning join is cascaded-Failed by any build failure and the
wire-and-boot step never runs. That is the "app never binds a port" class.

## A `-fn` line in a diff is not evidence of deletion

Four "deleted" planner functions turned out to be MOVED under `#[cfg(test)]`. Check the current tree, not
the diff.

## NEVER run a brace-matching script over this file

A regex/brace matcher written to remove one function deleted **34,827 lines** in one pass. It was
recovered only because the previous step was committed. Edit by explicit line range, verify with
`cargo build` before writing anything else, and commit before attempting the next removal.

## Before committing

```bash
source bin/activate-hermit
cargo fmt && cargo clippy --all-targets -- -D warnings   # workspace, not -p: a scoped clippy reports pass while the gate is red
cargo test -p goose-cli 2>&1 | grep -E '^test result:' | awk '{s+=$4} END {print s}'   # SUM: tail -3 shows one binary and lies
```
