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

**Phases:** `OPEN → ASK → RESEARCH → SYNTHESIS → REVIEW → CONTRACTS → BUILD → INTEGRATE → REPAIR`.

**These four are repeated here ON PURPOSE.** The detail lives in `.claude/rules/swarm-engine.md`, but
path-scoped rules arm only on the **Read** tool — `cat`, `sed`, `grep`, Grep and Glob do NOT trigger
them (measured against Claude Code 2.1.247). Since the fast way to open a 42,165-line file is
`grep -n` then `sed -n 'A,Bp'`, an agent can work in `swarm.rs` for a whole session and never load that
rules file. AGENTS.md loads at session start unconditionally, so the invariants that must never be
broken live HERE, and the rules files carry the detail for whoever does hit them.

**The four that break silently:**

1. **NO CAPS.** No wall clock, turn ceiling, retry count or volume limit may bound model work — local
   models are slow and that is expected. Terminators must be progress-based or live in the transport.
   `effective_idle_budget()` returns uncapped for any input and is tested to.
2. **`"integrate-verify"` is an exact-equality string test in five live places**, and the join must own
   NO files — `scheduler.rs:2603` relaxes a dependent through an upstream failure only if it owns
   nothing, so a file-owning join is cascaded-Failed and the app never binds a port.
3. **A correction is a PATCH (`plan_patched`), never a re-emission.** Re-emitting whole plans is what
   burned 3h40m without starting a build.
4. **The judge NUDGES; it does not kill.** Steer lands at a turn boundary and costs nothing.

**Every worker call has FIVE lane-building paths in the UI and one shared join, `digestStreamFields()`.**
Never hand-copy a digest field onto a lane; the join diverged twice that way and the failure is invisible
because the other four paths look correct.

**Rolling vs durable:** the activity digest is a small window the engine REWRITES IN PLACE;
`<task>.log` and `<task>.think.log` are append-only and complete. Any surface a person reads must prefer
the durable log.

`.claude/rules/*.md` carry the detail per area and load automatically when you touch a matching file.
`EXPERIMENTS-LEDGER.md` records what was already tried and what it measured — **read it before proposing
an engine change**, because several ideas here have been tried twice.

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
