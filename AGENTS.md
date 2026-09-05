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

**Phases (r6, research fan v2 81cd50d38):** `OPEN → [ASK handshake, only when the opener leaves open decisions] → RESEARCH FAN (the opener's per-slice questions dispatched one-per-host to the fleet, uncapped, read-only-quarantined; answers snowball into the ledger and the briefs; a miss is a loud research_unanswered that flows to REPAIR — never a block) → SYNTHESIS (planning over ANSWERED material) → the deterministic plan repairs (`finalize_plan_before_dag`) → THE SPLIT (2c S1, `swarm/shards.rs`: spec sections per owned file; FAT = above mean+σ of the plan's section-claiming tasks AND ≥ 2× their median — mean+σ alone flags the maximum of almost any plan (r6c minus web-viz: ledgerd-core 2.0/file vs 1.78), the median floor is what makes it "twice the typical task"; a fat task — r6c web-viz, 7 sections → 1 file → 519 min — gets ONE split patch from synthesis: N SHARD tasks working in `.swarm/shards/<module>/<shard>/` on pieces + a structured README, the module task as MERGER owning the final file, the interface declared as plan text; declining is loud `split_declined`) → BUILD → INTEGRATE → REPAIR`. REVIEW's LLM round (VA-014), the dynamic replanner (VA-015) and LEARN/persona (VA-016) are DELETED as of 2026-09-01; RESEARCH, coverage, the resplit, the ASK proxy and CONTRACTS are DELETED — workers read real dependency sources (dep_block) and the ledger block instead of briefs and frozen stubs. **(The engine on main IS the r6h golden 393a99351, sb-7 0.4616 — the post-golden research-fan-v3/VA-089..118 mechanisms were removed 2026-09-05; an engine change lands only with a measured run ≥ 0.4616.)**

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
2. **`"integrate-verify"` is an exact-equality string test (two live literals — `scheduler.rs` and `patch.rs`'s `SINK_ID` — and every other use goes through `SINK_ID`)**, and the join must own
   NO files — the scheduler's owns-nothing arm relaxes a dependent through an upstream failure only if it owns
   nothing, so a file-owning join is cascaded-Failed and the app never binds a port. Since `ee0cbfe73`
   the plan is REPAIRED by code before the DAG exists (`finalize_plan_before_dag`: pin sink → repair →
   entry files); a NON-sink task that owns nothing is REPORTED there (`tasks_owning_nothing`,
   `plan_repaired.before/after`) and removed by the repair — never refused. Mihai, 2026-08-29: "avoid
   making it overly deterministic and gated, be very mild" — code measures and nudges, it does not abort.
3. **A correction is a PATCH, never a re-emission.** Re-emitting whole plans is what burned 3h40m
   without starting a build. Since VA-014 (2026-09-01) the deterministic path is
   `finalize_plan_before_dag`'s repairs, which patch the loaded plan in place and ride
   `plan_repaired.before/after`; the ONE model-assisted correction is THE SPLIT (2c S1,
   `shards::split_fat_tasks`): a measured fat task gets one `PlanPatch` (add shards, widen the
   module's deps) built by CODE from synthesis's declaration, emitted as `plan_patched{source: split}`,
   and the patched plan walks the door again (`plan_repaired{source: split}`). No path re-plans; the
   LLM review round that once emitted `plan_patched` is deleted.
4. **The judge NUDGES; it does not kill.** A steer interrupts the stream at a chunk boundary and keeps the
   partial — but MEASURED on r1 a reasoning-only looping call ignored six of them; the RESTART verdict is
   what must reach such a call (re-stream), and that wiring is still open. Since VA-013 (2026-09-01) a
   BUILD/REPAIR lane is looked at on EVIDENCE only (`ladder::judge_summon_trigger`: repeat, degenerate
   answer, the recurrence meter, a forming-channel stall) — r6c spent 925 look-minutes of cadence and
   growth looks on build lanes for two compliances and zero kills; since e444953af (VA-056, 2026-09-01) EVERY lane kind is looked at on evidence only — recurrence, a forming-frame stall, or a judge NEXT the lane never acted on; r6e measured 31 cadence looks / 0 steers on planner lanes and 19 research looks all on nodes generating a sibling; the cadence/first-look/growth triggers and OMNI_JUDGE_*_SECS are deleted.
5. **Every app-under-test spawn goes through `spawn_grouped` / `kill_app_tree` (process groups).** A bare
   `tokio::process::Command` with piped stdio and `kill()` reaches ONE pid; the wrapper's `Popen`
   grandchildren keep the pipe write-ends and a reader awaiting EOF parks forever — r0 hung 20 minutes
   after its verdict, and 41 leaked servers were found holding ports. Proof without a run:
   `goose swarm gate <archived tree> --spec evals/swarm-bench/spec-build-sb7.md` must return and leave
   `tick.py` at `orphans: 0`.
6. **The LLM REVIEW round is DELETED (VA-014, 2026-09-01); no planning phase loops on an LLM's own
   novelty, and none is re-added without the measurement gate 9 demands.** History: r1's REVIEW surfaced
   8 → 4 → 9 new findings across three rounds, 51 minutes and 209k reasoning chars, because a 27B
   reviewer always finds another "not explicitly owned" concern, so it was cut to ONE round
   (`review_once`). Then three runs measured the one round at ZERO effective patches: r5 (52.6 wall-min,
   4 lanes) added `brush-contract` with a 658-char brief and the brush ReferenceError shipped anyway;
   r6c (28.1 wall-min) added `decisions-doc` with a 387-char brief that nothing depended on while its
   findings claimed "both now depend on it"; r6b/r6d one finding, zero patches — ~140–206 node-minutes
   per run to rediscover flags the engine had already computed. The plan's structural defects are
   repaired DETERMINISTICALLY in `finalize_plan_before_dag` (`repair_plan_flags`: owning-nothing,
   shared files, module/package shadows, the join's files, unowned advertised entries) — that is what
   stays, and `plan_slices_to_dag`'s seam test pins open → synthesis → plan_repaired with no review
   event between. A terminator that waits for an LLM to find nothing is not a terminator.

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
run path, prove the empty MEANS empty (honest-empty exemplar: the scheduler's fingerprint hashes `"ABSENT"`
distinctly). WHY: the nine-week template lived inside an empty-ledger fallback, and the GEN-6 sweep
found 10 of these hiding real failures — one turned a pillars serialize failure into a green gate.
HOW IT REFUSES: `development_gates.rs` ratchets the run-path `unwrap_or_default()` count — it may only
decrease. THE HAPPY-PATH CRITERION (2026-08-30): a fallback is legitimate only on an arm whose
primary path has MEASURED happy traffic; a fallback on a 0-happy-path arm is the implementation
impersonating one — delete it and let the failure be loud. Prove reachability as-configured
(the `proxy_yes` audit). NO HARD CODING is a prime directive: no magic values or baked-in
names/paths/counts where a derivation exists.

**2. THE SPECIFICITY GATE — no generic or template task text ever reaches a model.** "Integrate every
module and VERIFY", "DO EVERYTHING" and their class are banned — the ban is nine weeks old and the
phrase still shipped on 2026-08-30. Every dispatched description is assembled from THIS run's facts
(spec surface, ledger, fs_delta), and every output is a HANDOFF: exact files, symbols, the concrete
next step — vagueness is what a small model copes with by overthinking (measured: 11+ min of reasoning
on a trivial-but-vague probe task). HOW IT REFUSES: structurally — the template functions are `#[cfg(test)]`-only and
`sink_semantic_description` returns a `SinkBrief` with no template arm, so the phrase cannot reach a dispatch (the unit test
`the_banned_integrate_template_cannot_reach_a_dispatch` pins it), plus the banned-phrase count-ratchet in `development_gates.rs`;
GEN-5's brief floor emits a `thin_brief` WARNING event (2026-09-05 audit: there is no runtime `swarm.rs:~4900` checkpoint).

**3. THE BENCHMARK-LAUNCH GATE — a benchmark run starts ONLY from the app's Benchmark view.**
`pkill` stray Goose apps, `open -n /Applications/Goose.app --args --remote-debugging-port=9897`, then
`bench_dispatch.mjs` over CDP. NEVER headless, NEVER by typing the spec into a chat, NEVER a hand-rolled
vendor/harness (run_build.py already serves the vendor, builds fixtures, substitutes placeholders and
scores). WHY: every headless run of 2026-08-28 was void — twice, the second time via a self-written
harness. HOW IT REFUSES: campaign skill §4a is the procedure and `development_gates.rs` asserts the
skill still carries it; `first_tick_r1.sh` proves a run is real (run_build `--sb7`, vendor 200, orphans 0).

**4. THE REAPING GATE — kill PIDs, never killpg.** r2 died at INTEGRATE minute 139 because a killpg
aimed at two orphaned app servers took the engine with them — bare-spawn orphans share the engine's
process group. Reap surgically per-pid; tree kills belong to `kill_app_tree` only — plus goose-sidecar's
PROOF-GATED `sigkill_owned_group` (`getpgid(pid) == pid` and `pid != getpgrp()` proven, then killpg on
a group the sidecar itself created; detail in development-gates.md §4). HOW IT REFUSES:
tick.py and launch.sh reap per-pid by construction; any `killpg`/`kill -- -PGID` in an operator command
is wrong on sight.

**5. THE NO-TIME-INPUT GATE — no seconds value may decide model work.** II-7 made this structural:
read windows, idle budgets and seconds-verdicts are DELETED, only connect timeouts (transport) remain,
and terminators are look-counts and progress, not clocks. In review, any new literal-seconds constant
that can bound a model call is rejected on sight. WHY: the 600s read cut was manufacturing retries
(r2 drop 1), and the 420s stopwatch was the real harm behind r8's measurement. HOW IT REFUSES: the
NO CAPS invariant above, and the structure itself. HONEST NOTE (2026-09-05 audit): the golden engine still carries six literal-seconds constants
(`REPEAT_BREAK_MIN_SECS` 60 — decides a lane kill, `HANG_CONFIRM_SECS` 200, `POST_PROBE_SECS` 20, `FIX_PROGRESS_SAMPLE_SECS` 60,
`SCAN_TIMEOUT_SECS` 60, `JUDGE_WAKE` 30 s); they were IN the measured 0.4616 run, so changing them is an engine change behind the
measured-run gate, and the live-const ratchet does not yet count `Duration` or fn-body defaults (VA-164).

**6. THE ONE-DOOR GATE — every task enters the DAG through the same repairs, and the join owns
nothing STRUCTURALLY.** r4 (2026-08-30) was killed at BUILD+7m: the dynamic replanner spliced five
tasks straight into the live DAG past `finalize_plan_before_dag` — one re-created the exact
module/package import shadow (`app/notifierd.py` vs the skeleton's `app/notifierd/`) the plan repair
had fixed four minutes earlier, with a 500-char brief; and the pinned sink shipped owning `README.md`
(the cascaded-Failed, app-never-binds-a-port class). A repair that guards only ONE door holds until
the first other door opens. HOW IT REFUSES: `repair_sink_files` strips the join's files to a real
owner inside `finalize_plan_before_dag`; the DYNAMIC REPLANNER — the door r4 came through — is
DELETED (VA-015, 2026-09-01, gate 9: r6c's `replan-r0` ran 208 unsupervised minutes for two bonus
tasks nothing imported, r5's held two READY tasks 19 minutes at B+80; `repair_replan_specs` and the
`Replanned` event went with it), and `development_gates.rs` refuses its return and enumerates the one
splice site left (the merger's gap door, `splice_merge_gaps`, whose refusals are its repair; the idle-model judge's `apply_split` door is deleted in 2c S6). Mihai,
2026-08-30: "note this down in our agentic mechanism, or even better add it to our gates - make it a
practice."

**7. THE READ-THE-WORDS GATE (an OPERATING gate: how Claude works on goose, not what goose does) —
no loop, quality or efficacy judgment from shapes alone; the WORDS decide.** Mihai, 2026-08-30, after catching it twice in ten minutes: *"I have asked for 9 weeks for
you to read the WORDS not the fucking shape... You need to read the WORDS on both what it forms and
what it thinks to come up with ACTUAL improvements."* The r4-relaunch loop was diagnosable in one
`tail -c 4000` — a verbatim ten-item checklist cycling "This is good… now let me check if there are
any other issues" with the exit (`final_output`) never taken — and the improvement list (an exit
ramp in the reviewer prompt; hand the judge the words across looks, not a ratio about them) falls
straight out of READING it. The shingle ratio only corroborates; it names no fix. HOW IT REFUSES:
every loop/efficacy diagnosis starts with the tail WORDS of `<task>.think.log` AND `<task>.log`,
QUOTED — a claim without the quotes is invalid on sight; for a kill or a shipped fix, an INDEPENDENT
reader of the PRIMARY material (the raw logs, the archived events, the code — never the claimant's
summary) concurs or the claim falls. The deterministic doc-tests are amnesia TRIPWIRES: they may
summon, they never decide — see "HOW GATES 7 AND 8 ACTUALLY DECIDE" in development-gates.md.

**8. THE TRACE GATE (an OPERATING gate, like 7) — a change to goose ships WITH its trace, or ships
labeled a net.** Mihai,
2026-08-30: *"do you have a gate installed when making these engine changes to run the changes
mentally and see if they would make a difference?... Have you considered actually reading the code
and not just skimming it?... install gates around this to be more specific at all times, to be more
exact."* The receipt: the "recurrence corroborates drift" fix shipped minutes earlier could NOT have
fired anywhere in r4b's actual look sequence — DRIFTING came at look 1 with the meter under its 8k
span floor, and every later look said OK — and the commit never said so, because nobody walked the
motivating run's real values through the new branch. THE RULE: a change that claims to fix a
measured behavior carries, in its commit message, the trace — the motivating run's actual events
and numbers walked through the new path, branch by branch, ending "would have changed the outcome:
YES at <event, value> / NO, because <reason>". A NO may still ship, labeled A NET, never as the fix.
And the reading half: before editing, read the surrounding functions WHOLE and follow the value to
every consumer — a commit that cannot name what was read around the edit is skimming. HOW IT
REFUSES: `development_gates.rs` pins this gate's presence here and in development-gates.md; the
knob-turning/campaign skills carry the trace template; and in review, a fix-commit with no trace
block is sent back on sight.

**9. THE VALUE GATE (engine design AND operating) — a step exists only while its measured delivery is
CONSUMED downstream; a step that costs hours and delivers little is DELETED, never capped.** Mihai,
2026-09-01, after r6d's research fan ran 165 minutes at 59% spec-lookups under four vigil ticks that said
`continue`: *"Why would a phase that takes 4 hours and doesn't bring value continue? This is the
question."* and *"we don't want steps that consume time and not a lot of value. Get that straight."*
Every phase and sub-step is a PURCHASE — node-minutes for information the next step consumes — and it is
graded on BOTH sides: the vigil grades the CURRENT phase every tick (tick-surgeon step 2b: cost and
projection from tick.py's `PHASE VALUE` row, delivery by class from READING the units, verdict
`earning`/`NOT EARNING`; NOT EARNING files an ACTION in `VIGIL-ACTIONS.md` — the queue surgeons are
dispatched from — and recommends `cut` at the FIRST tick the numbers exist), and every finished run is
audited step by step (cost, delivery, who consumed it) so a step that fails on two runs is deleted in
the next engine change. The fix for a wasteful step is its MECHANISM — the prompt that asks for
questions with no lookup/decision split, the fan that dispatches duplicates, the brief that injects what
the spec already says — never a cap, clock or count (gates 1 and 5 refuse those). Receipts: the research
fan (r6c 126m, r6d 4h projected, 16 of 27 questions need not have run); fix waves r5 144m / r6c 215m with
zero score value both runs. HOW IT REFUSES: `development_gates.rs` pins this gate, the tick-surgeon's
PHASE VALUE step, `VIGIL-ACTIONS.md` and tick.py's cost row; in review, a new phase or step lands only
with the measurement that says what it buys and which step consumes it.

**10. THE NO-ABSOLUTES GATE — a number lives in the engine only as a RATIO or a MEASUREMENT.** Mihai,
2026-09-02: *"we need to avoid hard coded bits because this is an agent and that makes it useless
outside of the scope of what we are doing now — the benchmark is the cause not the goal."* A typed
absolute sized for this model / language / API (24,000 chars, 200 s, `impl.py`, `?cursor=1`) is a
defect; a fraction of the probed window, a multiple of the app's own median, a share of the lane's own
output, an algorithm constant or a named policy ratio with its receipt may stay. HOW IT REFUSES:
`development_gates.rs` ratchets the count of live numeric `const` literals outside `cfg(test)` (28 on
2026-09-02; 27 on the golden engine main ships — VA-140/141's receipts landed only on the archived r6k line) — it may only decrease; a new one needs `// ratio:` or `// measured:` on its line. VA-126.

**11. THE KNOWN-FIX GATE — a fix whose design is known starts NOW, in a worktree; only cargo waits for
a run.** Mihai, 2026-09-02, after VA-126 was parked "until after r6j": *"are you not doing anything
about the hard coded bits I asked about?"* An edit in a worktree touches nothing the running bundle
executes; the sole in-run constraint is no cargo on the machine whose node the run holds. Action rows
are `OPEN` / `CLAIMED` / `QUEUED behind: <slot>` / `SCHEDULED waits on: <measurement>` / `LANDED` /
`DROPPED` — "after the run" is never a status. HOW IT REFUSES: `development_gates.rs` fails on a
SCHEDULED row without `waits on:` or a QUEUED row without `behind:`.

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
