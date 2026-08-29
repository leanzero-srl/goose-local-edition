---
paths:
  - "crates/goose-cli/src/commands/swarm.rs"
---

# swarm.rs — the engine. 42,000 lines, and six ways to break it silently.

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

## Every app-under-test spawn is a PROCESS GROUP (`44b2ad6cd`)

`own_process_group` → `spawn_grouped` → `kill_app_tree` / `kill_app_tree_and_drain` (grep the names). The
old shape — `tokio::process::Command` with piped stdio, `kill()`, then await the pipe readers to EOF —
parked `run_swarm` forever when the wrapper's `Popen` grandchildren kept the write-ends (r0, 20 min at 0%
CPU after its verdict; 41 leaked `ledgerd`/`notifierd` found). The drain now releases on GROUP liveness,
not EOF; that bound is on a process we already killed, never on a model. Applied at `boot_invocation`,
`run_spec_contract` and its kill sites, the restart reboot, `run_repro_once`, `land_generated_tests`.
Isolation proof: `./target/release/goose swarm gate <archived tree> --spec evals/swarm-bench/spec-build-sb7.md`
returns in seconds and `tick.py` prints `orphans: 0`. **`swarm verify` does NOT boot the app** — it only
checks imports/owned files — so it can prove neither a hang nor a phantom endpoint.

## REVIEW is one round (`5173eab67`)

`review_once`; the loop, `review_oscillating`, `review_patch_stuck`, `RejectMemo` are deleted. The
measured flags from `decomposition_of` are rendered by `review_must_fix_block` into the prompt. r1's
three rounds (8 → 4 → 9 new, 51 min, 209k chars) are the reason; `review_dedupe_key` now de-dupes lane
rephrasings WITHIN the round so `review_findings.new` counts distinct findings.

## The deterministic gate probes ledgerd's OWN table (`0d5ac740d`)

`spec_advertised_surface`: a path cell's path is the first backticked token (row 115 `\`/\` + \`web/*\``
used to yield "/`"); rows under a heading naming another service (`### 6. \`notifierd\``) are dropped;
`spec_get_endpoints` ignores prose once a table exists (the vendor's `GET /v3/reversals` in line 86 was
being probed on the app). Test `the_real_sb7_spec_yields_only_ledgerds_own_table_endpoints` reads the
real spec. Five of r0's 29 "criticals" were these phantoms.

## Two digest write sites, both append the transcripts (`c3b211582`)

The main loop (~`:17104`) and the judge-probe branch (~`:16416`) each write the digest on a 400 ms
coalesce; both must call `append_reasoning_transcript` and `append_thinking_transcript`. The probe branch
did not, so a lane under back-to-back looks had its `.think.log` frozen 155 s behind its digest.

## Findings attribute to the FIRST authored source path (`d748a7d3e`)

`extract_file_from_finding`: backticked/authored paths first-wins ("Frontend not served (in
`app/ledgerd.py`, `web/index.html`)" → ledgerd); pytest tracebacks keep last-wins (the failing frame is
last). Tests `an_authored_finding_shards_to_the_first_file_in_its_attribution_list`,
`a_traceback_still_shards_to_its_last_owned_frame`.

## Before committing

```bash
source bin/activate-hermit
cargo fmt && cargo clippy --all-targets -- -D warnings   # workspace, not -p: a scoped clippy reports pass while the gate is red
cargo test -p goose-cli 2>&1 | grep -E '^test result:' | awk '{s+=$4} END {print s}'   # SUM: tail -3 shows one binary and lies
```
