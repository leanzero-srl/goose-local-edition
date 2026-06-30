# Atomic-writes design (research workflow wdep83bjx, 6 agents)

All five findings check out against the code. Key verifications: the one-big-write rule is verbatim at `swarm.rs:5247-5249`; over_read is deterministically gated on `!input.any_owned_written` at `judge.rs:279` and `:297`; the scorer plateaus at `wc` (`indep_score = independent.min(wc)*10` at `:2040`, `size_score = if n>=wc {5}` at `:2090`); and critically the completion guard at `swarm.rs:5336-5340` only flags **missing or empty** files — a skeleton-only file with `pass` bodies is non-empty and slips through. Here is the design doc.

---

# Design: Make goose-swarm Write Code Atomically

## 1. PROBLEM

On task `commands-cli` a worker spent ~5k tokens reading (`cat`/Read across the project) before emitting a single 2.6KB `cli.py` in one `write`, and the judge flagged `over_reading`. This is not a bug — it is the **tuned, mandated** behavior: the base worker prompt (`swarm.rs:5247-5249`) hard-rules *"Write each file COMPLETE in ONE `write`… Do NOT write a rough draft then refine it with a chain of small `edit`s — plan the whole file first, then write it once. Every extra round-trip costs ~30-60s on a local model."* That "plan the whole file first" is exactly what produces read-everything-then-dump. It hurts three ways: (1) the deterministic over_read trip (`judge.rs:279-296`) fires whenever `!any_owned_written && tool_calls >= 16` after 90s, so a front-loading worker that's *about to* produce gets a false-positive kill + re-dispatch (each re-dispatch is minutes on the slow fleet); (2) a bad import / unregistered command surfaces only after a full 5k-token rewrite instead of one cheap check; (3) the base prompt actively **contradicts the judge's own remediation hint** ("write the SIMPLEST version first… then refine", `judge.rs:288-293`/`301-306`), so the worker gets whipsawed.

## 2. THE THREE DIRECTIONS

### (A) Atomic writes WITHIN a worker — skeleton-first then fill

**What it is.** For a wiring/entry file (`cli.py`, `__main__.py`, `src/main.rs`, `src/index.ts|cli.ts`, `main.go` — the names already encoded in `entry_clause` at `swarm.rs:4903-4923`), the worker's *first* `write` emits a complete compiling skeleton (every import + every command/handler signature with a placeholder body: `pass`/`todo!()`/`throw new Error('todo')`), runs ONE cheap parse/import check (`lang.entry_run_example()` at `swarm.rs:4940-4953`), confirms every command is registered, THEN fills bodies with focused `edit`s.

**Exact code change.**
1. In the single-owned-file branch `format!` at `swarm.rs:5115-5130` (commands-cli owns one file → lands here), after the existing "WRITE FIRST" sentence (~5119), inject a STRUCTURE-FIRST block **only when** the owned file matches an entry name. Use `lang.entry_run_example()` for the check command.
2. **Resolve the contradiction** at `swarm.rs:5247-5249`: this cannot be appended — it must be *edited*, or a weak local model gets "one write" and "skeleton then edits" simultaneously. Soften to: *"one write per logical unit; for a multi-command entry/wiring file, write the import+signature skeleton first, confirm it imports, then fill bodies. For an ordinary single-responsibility module, still write it complete in one write."* This makes the base prompt agree with the judge hint (`judge.rs:288-293`).
3. **Fix the correctness hazard this introduces:** the completion guard at `swarm.rs:5336-5340` accepts any non-empty file, so a worker killed after the skeleton write (bodies still `pass`) would be **wrongly accepted as done**. Add a placeholder-token scan (`pass`/`todo!()`/`throw new Error('todo')`) to that guard for entry files, or treat skeleton-only as `ContentRetry` like the missing-file path at `5353`.
4. Gate behind a new env flag `GOOSE_SWARM_SKELETON_FIRST`, mirroring `GOOSE_SWARM_CONTRACTS` read at `swarm.rs:5204-5206`, so the default path stays byte-identical and you can A/B on the slow fleet.

**Confidence.** *Mechanism: HIGH.* over_read is deterministically gated on `!any_owned_written` at `judge.rs:279` and `:297`; one early non-empty write **permanently disarms both paths** for that attempt (proven by `behavioral_over_read_quiet_while_writing`, judge.rs:~397). The kill-risk reduction is structurally guaranteed, not hopeful. *Behavioral outcome: MEDIUM* — whether the qwopus-class local models reliably emit a clean compiling skeleton (vs. a half-skeleton that wastes a turn) is genuinely uncertain on weak models, which is exactly why the env-gate + A/B matters. **I am least sure about the placeholder-detection in step 3**: distinguishing a legitimate `pass` (empty `except`, abstract stub) from an unfilled body is heuristic and could false-positive — flag this for careful review.

**Risk to the slow fleet.** Trades wall-clock for kill-safety. Skeleton + N fills + 1 import check = roughly +2–4 min per entry file (the prompt itself quantifies ~30-60s/round-trip at `5248`), and these edits are **serial on one node** — the fleet cannot fan them out. Net win ONLY if over_read/loop kills were costing more than the added turns. Must be "skeleton early + a FEW meaningful fills," never many tiny edits.

**Blast radius.** `swarm.rs` only: the entry-file branch (`5115-5130`), the one-big-write rule (`5247-5249`, touched, not appended), the completion guard (`5326-5358`). Env-gated → default byte-identical. No `goose-swarm/**` change required (judge gates already do the right thing). Scoping by filename is brittle for `TargetLang::Other` and non-standard entry names — accept that gap.

### (B) Fleet-aware finer slicing — atomic units capped at fleet width

**What it is.** Push atomicity DOWN into the *subtask spec* at plan time, while keeping subtask **count** capped at fleet width. The detailer (`swarm.rs:3285-3301`) already enumerates each subtask's owned files; have it additionally prescribe a per-file **build order / incremental write checklist** so the worker writes file-by-file within ONE dispatch — atomic writes with **zero extra fleet waves**.

**Exact code change.** Extend the detailer system prompt at `swarm.rs:3285-3291` and the `files_line` block at `3292-3301`: *"List the owned files in build order; the worker creates each file with its own `write` and stops reading once it has the signatures it needs — do NOT batch all files into one write or re-read siblings already summarized here."* Mirror one line into the MODULAR ARCHITECTURE rule at `swarm.rs:2999-3005`. **Do NOT** lower the `2x to 3x {worker_count}` multiplier at `swarm.rs:2991-2995`.

**Confidence.** *MEDIUM.* The clean insight (verified): the architect prompt ALREADY decouples file-atomicity from subtask count — `2999-3005` mandates a subtask own *several* small single-concern files, and the scorer gives **zero reward past `wc`** (`indep_score = independent.min(wc)*10` at `2040`; `size_score` neutral above `wc` at `2090`). So the lever is the detailer checklist, not the count. Uncertainty: whether a detailer-emitted build-order actually changes worker write cadence (the worker prompt's one-big-write rule at `5247` would still fight it unless A's edit ships too — **B depends on A's contradiction-fix to not be inert**).

**Risk to the slow fleet.** The *cheap* variant (detailer checklist, count unchanged) adds zero waves. The **trap variant** — lowering the multiplier toward 1x-1.5x — is actively counterproductive: it *coarsens* subtasks (more concerns each → MORE over-reading and bigger monolithic writes, the exact symptom), and a single coarse-subtask redo costs more wall-clock. Going the *other* way (1 concern per subtask) inflates BOTH detailer waves (`swarm.rs:3268`, `ceil(N/3)` × ≤75s) AND execute waves (scheduler weight) — the serialization the prompt warns against.

**Blast radius.** `swarm.rs` detailer prompt + `files_line` (`3285-3301`) and one line at `2999-3005`. If you also touch the multiplier (`2991-2995`) or scorer (`2090`), you change plan *shape* on every run AND must update the `score_skeleton_prefers_wider_flatter_plan` unit test (`swarm.rs:~1221-1250`), and you must also fix the solo-fallback `plan()` prompt (`swarm.rs:3338-3361`) which currently says the **opposite** ("MANY… AT LEAST {worker_count}, more is better", uncapped) or a planning failure flips the policy mid-run.

### (C) Multi-model slicing of one file — via frozen interfaces

**What it is.** Multiple models each implement a frozen-signature *subset* of one large file, then a deterministic assemble step stitches fragments into the real path. Contracts (`generate_contracts`, `swarm.rs:2884-2949`) remove signature drift, making this newly conceivable.

**Exact code change (Design A in finding 4).** Extend ownership from `Vec<String>` to `{path, symbols: Vec<String>}` at `dag.rs:18-28`, threaded through `DispatchRequest` (`dispatch.rs:~50`), `TaskDispatched` (`event.rs:~17`), `do_claim` (`scheduler.rs:380-437`), and `JudgeInput`. Route each function-group writer to its own fragment file in a speculative shadow (`make_shadow`/`spec_shadows`). Add an `assemble-<file>` subtask depending on every fragment writer that concatenates fragments under one header (import-union + dedup + module docstring + `if __name__=='__main__'`) and writes the real path.

**Confidence.** *LOW* for the literal ask. The merge — not the signatures — is the blocker: import-union/dedup/ordering/who-owns-the-header (the argparse/Click root object, top-level constants, `__main__`) is itself an AST/LLM task and a **fresh cross-fragment drift source**, reintroducing one level down the exact failure class contracts were built to kill, and hidden again behind per-fragment tests passing.

**Risk to the slow fleet.** High and likely **net-negative**. The `assemble` subtask is SERIALIZED after all fragment writers and adds an extra dispatch + minutes-long LLM round on the 3-node fleet — easily more wall-clock than the intra-file parallelism buys. If `files_conflict` (`scheduler.rs:279-285`) is relaxed for the shared path, two concurrent whole-file writers clobber under last-writer-wins (`copy_owned_files`, `swarm.rs:4744-4777`).

**Blast radius.** Large: `dag.rs` (`TaskSpec` + `apply_split` partition `1089-1219`), `dispatch.rs`, `event.rs`, `scheduler.rs` (`files_conflict`, `held_files`, `do_claim`), `judge.rs` (`is_split_candidate:187`), plus brand-new assemble + promotion-concat logic and worker-prompt changes. Reshapes `score_skeleton` overlap scoring (`2068-2075`) and M3 split semantics.

## 3. RECOMMENDED SEQUENCING (by confidence + correctness, not effort)

**First: (A) skeleton-first, env-gated.** Highest-confidence *mechanism* and the only direction that targets the actual observed symptom. The over_read trip is deterministically gated on `!any_owned_written` (`judge.rs:279`, `:297`) — an early skeleton write structurally disarms the false positive; this is provable from the code, not speculative. Ship it behind `GOOSE_SWARM_SKELETON_FIRST` so default behavior is unchanged and you A/B on the slow fleet. **Ship the completion-guard fix (`5336-5340`) in the same change** — without it, a worker killed mid-fill is accepted with `pass` bodies, a real regression. Brutally honest: my confidence the *weak local models comply cleanly* is only MEDIUM, and the placeholder-vs-legitimate-`pass` detection is the part I'm least sure I can make robust — verify it adversarially on real runs before un-gating.

**Second: (B) detailer build-order checklist, count capped — do the cheap variant ONLY.** Reinforces A at plan time with zero extra fleet waves, and the insight is well-grounded (the architect prompt already decouples file-atomicity from count; the scorer already plateaus at `wc`). It is second because it is partly *dependent on A* — until the one-big-write rule at `5247` is softened, a detailer checklist is inert. Explicitly **do not** lower the `2x-3x` multiplier.

**Third / probably never: (C).** Lowest confidence, highest blast radius, and likely net-negative wall-clock on a 3-node fleet. The *realizable* slice of C — function-groups as separate small files composed via imports — is **already** the existing modular rule (`swarm.rs:2999-3005`); if `commands-cli` produced a monolith anyway, the fix is sharper example-driven language in the architect prompt (`2988-3026`), i.e. a slice of B, **not** intra-file multi-model machinery. Recommend not building C's Design-A unless A+B demonstrably fail.

## 4. WHAT NOT TO DO

- **Do NOT lower the `2x-3x {worker_count}` multiplier** (`swarm.rs:2991-2995`) to chase atomicity. It coarsens subtasks → MORE over-reading and bigger monolithic writes — the exact symptom, backwards.
- **Do NOT implement "atomic" as one-concern-per-subtask.** It multiplies front-loaded detailer waves (`ceil(N/3)` × ≤75s at `3268`) + execute waves with zero parallelism past fleet width — the serialization the architect prompt explicitly guards against.
- **Do NOT append the skeleton rule next to `5247-5249`** — edit it. Weak models handle "write in one shot" + "skeleton then edits" simultaneously badly.
- **Do NOT change `score_skeleton` thresholds without also changing the prompt** — the scorer runs only on the best-of-N path (`n>1`); single-draft runs (`n==1`) ignore it, so a scorer-only change is silently inert.
- **Do NOT mis-implement atomic as "many small writes at the END."** The file still appears late (over_read still fires, gated on `!any_owned_written`) AND you pay the round-trip tax — worst of both.
- **Do NOT relax `files_conflict` (`scheduler.rs:279-285`) for a shared path** — concurrent whole-file writers clobber under last-writer-wins (`copy_owned_files`, `4744-4777`).
- **Do NOT forget the solo-fallback `plan()` prompt (`swarm.rs:3338-3361`)** if you touch granularity policy — it currently gives uncapped "more is better" guidance and will flip the policy on any parallel-planning failure.

**Verified anchors:** architect prompt `swarm.rs:2991-3005`; modular rule `2999-3005`; one-big-write `5247-5249`; entry branch `5115-5130`; over_read gates `judge.rs:279-309`; scorer `swarm.rs:2028-2092`; detailer `3285-3301`; entry names `4903-4923`; entry-run example `4940-4953`; env-flag pattern `5204-5206`; completion guard `5326-5358`.