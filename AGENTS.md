# AGENTS Instructions

goose is an AI agent framework in Rust with CLI and Electron desktop interfaces.

## Setup
```bash
source bin/activate-hermit
cargo build
```

## Commands

### Build
```bash
cargo build                   # debug
cargo build --release         # release  
just release-binary           # release binary
```

### Test
```bash
cargo test                   # all tests
cargo test -p goose          # specific crate
cargo test --package goose --test mcp_integration_test
just record-mcp-tests        # record MCP
```

### Lint/Format
```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

### UI
```bash
just run-ui                  # start desktop
cd ui/desktop && pnpm run typecheck
cd ui/desktop && pnpm test   # test UI
```

## Structure
```
crates/
├── goose              # core logic
├── goose-acp-macros   # ACP proc macros
├── goose-cli          # CLI entry
├── goose-mcp          # MCP extensions
├── goose-test         # test utilities
└── goose-test-support # test helpers

ui/desktop/            # Electron app
```

## Development Loop
```bash
# 1. source bin/activate-hermit
# 2. Make changes
# 3. cargo fmt
```

### Run these only if the user has asked you to build/test your changes:
<!-- EXCEPTION, so the two instructions do not contradict: for SWARM work
     (crates/goose-cli/src/commands/swarm.rs, crates/goose-swarm/*, ui/desktop/src/components/swarm/*)
     the gates in .claude/rules/swarm-engine.md are MANDATORY before every commit, asked for or not --
     a scoped `clippy -p` reports pass while the workspace gate is red, and `cargo test | tail -3` shows
     one binary of five. -->
```
# 1. cargo build
# 2. cargo test -p <crate>
# 3. cargo clippy --all-targets -- -D warnings
```

## Rules

- Test: Prefer tests/ folder, e.g. crates/goose/tests/
- Test: When adding features, update goose-self-test.yaml, rebuild, then run `goose run --recipe goose-self-test.yaml` to validate
- Error: Use anyhow::Result
- Provider: Implement Provider trait see providers/base.rs
- MCP: Extensions in crates/goose-mcp/
- UI Desktop: Use ACP SDK types or local `src/types/*` types. Do not import generated OpenAPI types/client code from `ui/desktop/src/api`

## Code Quality

- Comments: Write self-documenting code - prefer clear names over comments
- Comments: Never add comments that restate what code does
- Comments: Only comment for complex algorithms, non-obvious business logic, or "why" not "what"
- Simplicity: Don't make things optional that don't need to be - the compiler will enforce
- Simplicity: Booleans should default to false, not be optional
- Errors: Don't add error context that doesn't add useful information (e.g., `.context("Failed to X")` when error already says it failed)
- Simplicity: Avoid overly defensive code - trust Rust's type system
- Logging: Clean up existing logs, don't add more unless for errors or security events

## Ink / Terminal UI (ui/text)

- Ink renders React to a fixed character grid — not a browser. Content that exceeds a Box's dimensions is NOT clipped; it visually overflows into neighboring cells and breaks the layout.

- Ink-Text: Never use `wrap="wrap"` inside a fixed-height Box — wrapped text can exceed the Box height and bleed into adjacent components. Use `wrap="truncate"` and pre-truncate the string to fit the available character budget (lines × width).
  
- Ink-Layout: When changing card/cell dimensions, always recalculate how much content fits. Account for borders (2 chars), padding, margins, and sibling elements when computing the
remaining space for dynamic text.
  
- Ink-Overflow: Ink has no `overflow: hidden`. The only way to prevent overflow is to ensure content never exceeds the container size — truncate text, limit list items, or cap height.
  
- Ink-FlexGrow: Avoid `flexGrow={1}` on text containers inside fixed-height cards — the text will try to fill available space but Ink won't clip it if it exceeds the boundary.
  
- Ink-HeightBudget: When computing how many rows/items fit vertically, count EVERY line used by headers, footers, margins, borders, and scroll indicators. Under-reserving vertical space (e.g., `height - 8` when chrome actually uses 16 lines) causes Ink to squeeze out margins between items, making borders collapse. Always audit the actual line count.
  
- Ink-TrailingMargin: Don't apply `marginBottom` to the last item in a list — it wastes a line and can push content out of the container. Use conditional margins or container `gap`.

## The swarm engine — read before changing it

The swarm builds software by fanning work across 3 local LM Studio nodes. It is the subject of most work
in this repo and it has a specific set of invariants that produce no compiler error when broken.

**Phases (r3, P1-4/P1-5):** `OPEN → [ASK handshake, only when the opener leaves open decisions] → SYNTHESIS → REVIEW (one round) → BUILD → INTEGRATE → REPAIR`. RESEARCH, coverage, the resplit, the ASK proxy and CONTRACTS are DELETED — workers read real dependency sources (dep_block) and the ledger block instead of briefs and frozen stubs.

**These six are repeated here ON PURPOSE.** The detail lives in `.claude/rules/swarm-engine.md`, but
path-scoped rules arm only on the **Read** tool — `cat`, `sed`, `grep`, Grep and Glob do NOT trigger
them (measured against Claude Code 2.1.247). Since the fast way to open a 42,165-line file is
`grep -n` then `sed -n 'A,Bp'`, an agent can work in `swarm.rs` for a whole session and never load that
rules file. AGENTS.md loads at session start unconditionally, so the invariants that must never be
broken live HERE, and the rules files carry the detail for whoever does hit them.

**The six that break silently:**

1. **NO CAPS.** No wall clock, turn ceiling, retry count or volume limit may bound model work — local
   models are slow and that is expected. Terminators must be progress-based or live in the transport.
   Since II-7 (e1e32cdda) the guard is STRUCTURAL: `run_agent`/`run_agent_in` carry no time parameter
   at all, so re-arming a cap means re-adding a parameter through every signature — never flipping a
   number. (`effective_idle_budget()` and its test are deleted; do not reintroduce either.)
2. **`"integrate-verify"` is an exact-equality string test in five live places**, and the join must own
   NO files — `scheduler.rs:2603` relaxes a dependent through an upstream failure only if it owns
   nothing, so a file-owning join is cascaded-Failed and the app never binds a port. Since `ee0cbfe73`
   the plan is REPAIRED by code before the DAG exists (`finalize_plan_before_dag`: pin sink → repair →
   entry files); a NON-sink task that owns nothing is REPORTED there (`tasks_owning_nothing`,
   `plan_repaired.before/after`) and removed by the repair — never refused. Mihai, 2026-08-29: "avoid
   making it overly deterministic and gated, be very mild" — code measures and nudges, it does not abort.
3. **A correction is a PATCH (`plan_patched`), never a re-emission.** Re-emitting whole plans is what
   burned 3h40m without starting a build.
4. **The judge NUDGES; it does not kill.** A steer interrupts the stream at a chunk boundary and keeps the
   partial — but MEASURED on r1 a reasoning-only looping call ignored six of them; the RESTART verdict is
   what must reach such a call (re-stream), and that wiring is still open.
5. **Every app-under-test spawn goes through `spawn_grouped` / `kill_app_tree` (process groups).** A bare
   `tokio::process::Command` with piped stdio and `kill()` reaches ONE pid; the wrapper's `Popen`
   grandchildren keep the pipe write-ends and a reader awaiting EOF parks forever — r0 hung 20 minutes
   after its verdict, and 41 leaked servers were found holding ports. Proof without a run:
   `goose swarm gate <archived tree> --spec evals/swarm-bench/spec-build-sb7.md` must return and leave
   `tick.py` at `orphans: 0`.
6. **REVIEW is ONE round (`review_once`); no planning phase loops on an LLM's own novelty.** r1's REVIEW
   surfaced 8 → 4 → 9 new findings across three rounds, 51 minutes and 209k reasoning chars, because a
   27B reviewer always finds another "not explicitly owned" concern. The measured plan flags
   (`tasks_owning_nothing`, `module_package_collisions`, `shared_files`) are injected as MUST-FIX instead.
   A terminator that waits for an LLM to find nothing is not a terminator.

**Every worker call has FIVE lane-building paths in the UI and one shared join, `digestStreamFields()`.**
Never hand-copy a digest field onto a lane; the join diverged twice that way and the failure is invisible
because the other four paths look correct.

**Rolling vs durable:** the activity digest is a small window the engine REWRITES IN PLACE;
`<task>.log` and `<task>.think.log` are append-only and complete — the worker loop has TWO digest write
sites (the main loop and the judge-probe branch) and both must append the transcripts; the probe branch
did not until `c3b211582`, and the biggest lanes' logs froze under looks. Any surface a person reads must
prefer the durable log, and the panel's live line follows whichever channel advanced last.

`.claude/rules/*.md` carry the detail per area and load automatically when you touch a matching file.
`EXPERIMENTS-LEDGER.md` records what was already tried and what it measured — **read it before proposing
an engine change**, because several ideas here have been tried twice.

## GATES — the rules that refuse (paid for, do not relitigate)

Each of these was bought with a destroyed run or a nine-week defect. After a compaction the urge to
break them returns; the gate is what refuses, not your memory. Full detail, the suspect catalogue and
what each one cost: `.claude/rules/development-gates.md`. Enforced by
`cargo test -p goose-swarm --test development_gates`.

**1. THE FALLBACK GATE — a missing input never silently substitutes content.** Facts, or a loud NAMED
absence-event (`ledger_empty_at_sink` class) that tick.py prints — never a template, never a quiet
default. A fallback the owner ordered killed STAYS dead without his word; root-causing it does not
revive it. Before writing any `unwrap_or_default()` / `Err(_) => empty` / `.ok()`-and-continue in the
run path, prove the empty MEANS empty (honest-empty exemplar: `scheduler.rs:237` hashes `"ABSENT"`
distinctly). WHY: the nine-week template lived inside an empty-ledger fallback, and the GEN-6 sweep
found 10 of these hiding real failures — one turned a pillars serialize failure into a green gate.
HOW IT REFUSES: `development_gates.rs` ratchets the run-path `unwrap_or_default()` count — it may only
decrease.

**2. THE SPECIFICITY GATE — no generic or template task text ever reaches a model.** "Integrate every
module and VERIFY", "DO EVERYTHING" and their class are banned — the ban is nine weeks old and the
phrase still shipped on 2026-08-30. Every dispatched description is assembled from THIS run's facts
(spec surface, ledger, fs_delta), and every output is a HANDOFF: exact files, symbols, the concrete
next step — vagueness is what a small model copes with by overthinking (measured: 11+ min of reasoning
on a trivial-but-vague probe task). HOW IT REFUSES: the `swarm.rs:~4900` dispatch checkpoint plus the
banned-phrase count-ratchet in `development_gates.rs`; GEN-5's brief floor emits a `plan_flag` WARNING.

**3. THE BENCHMARK-LAUNCH GATE — a benchmark run starts ONLY from the app's Benchmark view.**
`pkill` stray Goose apps, `open -n /Applications/Goose.app --args --remote-debugging-port=9897`, then
`bench_dispatch.mjs` over CDP. NEVER headless, NEVER by typing the spec into a chat, NEVER a hand-rolled
vendor/harness (run_build.py already serves the vendor, builds fixtures, substitutes placeholders and
scores). WHY: every headless run of 2026-08-28 was void — twice, the second time via a self-written
harness. HOW IT REFUSES: campaign skill §4a is the procedure and `development_gates.rs` asserts the
skill still carries it; `first_tick_r1.sh` proves a run is real (run_build `--sb7`, vendor 200, orphans 0).

**4. THE REAPING GATE — kill PIDs, never killpg.** r2 died at INTEGRATE minute 139 because a killpg
aimed at two orphaned app servers took the engine with them — bare-spawn orphans share the engine's
process group. Reap surgically per-pid; tree kills belong to `kill_app_tree` only. HOW IT REFUSES:
tick.py and launch.sh reap per-pid by construction; any `killpg`/`kill -- -PGID` in an operator command
is wrong on sight.

**5. THE NO-TIME-INPUT GATE — no seconds value may decide model work.** II-7 made this structural:
read windows, idle budgets and seconds-verdicts are DELETED, only connect timeouts (transport) remain,
and terminators are look-counts and progress, not clocks. In review, any new literal-seconds constant
that can bound a model call is rejected on sight. WHY: the 600s read cut was manufacturing retries
(r2 drop 1), and the 420s stopwatch was the real harm behind r8's measurement. HOW IT REFUSES: the
NO CAPS invariant above, and the structure itself — there is no knob left to set.

**6. THE ONE-DOOR GATE — every task enters the DAG through the same repairs, and the join owns
nothing STRUCTURALLY.** r4 (2026-08-30) was killed at BUILD+7m: the dynamic replanner spliced five
tasks straight into the live DAG past `finalize_plan_before_dag` — one re-created the exact
module/package import shadow (`app/notifierd.py` vs the skeleton's `app/notifierd/`) the plan repair
had fixed four minutes earlier, with a 500-char brief; and the pinned sink shipped owning `README.md`
(the cascaded-Failed, app-never-binds-a-port class). A repair that guards only ONE door holds until
the first other door opens. HOW IT REFUSES: `repair_replan_specs` (scheduler.rs) applies the same
ownership rules to every replan batch BEFORE `splice_specs` and its actions ride the `Replanned`
event; `repair_sink_files` strips the join's files to a real owner; the replanner is summoned only
once something has COMPLETED (its own value theory is "harden the completed work"); and
`development_gates.rs` refuses a splice site that reaches the DAG around the repair. Mihai,
2026-08-30: "note this down in our agentic mechanism, or even better add it to our gates - make it a
practice."

**7. THE READ-THE-WORDS GATE — no loop, quality or efficacy judgment from shapes alone; the WORDS
decide.** Mihai, 2026-08-30, after catching it twice in ten minutes: *"I have asked for 9 weeks for
you to read the WORDS not the fucking shape... You need to read the WORDS on both what it forms and
what it thinks to come up with ACTUAL improvements."* The r4-relaunch loop was diagnosable in one
`tail -c 4000` — a verbatim ten-item checklist cycling "This is good… now let me check if there are
any other issues" with the exit (`final_output`) never taken — and the improvement list (an exit
ramp in the reviewer prompt; hand the judge the words across looks, not a ratio about them) falls
straight out of READING it. The shingle ratio only corroborates; it names no fix. HOW IT REFUSES:
operator side — every loop/efficacy diagnosis starts with the tail WORDS of `<task>.think.log` AND
`<task>.log` (the skill's checkpoint procedure carries the exact commands), and an OBSERVATIONS/note
entry claiming a loop without QUOTING the looping words is invalid on sight; engine side — the judge
is handed verbatim spans (current tail + a span from a prior look), never only counters, so it can
SEE "same text as last look"; `development_gates.rs` asserts both docs and the skill carry this.

## Never

- Never: Recreate `ui/desktop/src/api` or add `@hey-api/openapi-ts` to `ui/desktop`
- Cargo.toml: For human-authored dependency changes, use `cargo add` instead of manually editing dependency entries unless there is a specific reason not to.
- Cargo.toml: Automated dependency bump PRs are exempt; when manual edits are necessary, keep `Cargo.lock` consistent.
- Never: Skip cargo fmt
- Never: Merge without running clippy
- Never: Comment self-evident operations (`// Initialize`, `// Return result`), getters/setters, constructors, or standard Rust idioms

## Entry Points
- CLI: crates/goose-cli/src/main.rs
- UI: ui/desktop/src/main.ts
- Agent: crates/goose/src/agents/agent.rs
