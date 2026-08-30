# Development gates — the rules that refuse

Ordered by Mihai 2026-08-30 07:40: *"add some gates to our development in goose … after compacting the
context these urges might return to you … so let's gates that stop this madness from ever unfolding."*
The short form lives in AGENTS.md (`## GATES`), which loads every session unconditionally; THIS file
carries the detail, the suspect catalogue and the receipts. Enforced mechanically by
`crates/goose-swarm/tests/development_gates.rs` (`cargo test -p goose-swarm --test development_gates`).

These are not style preferences. Every gate below is the residue of a rebuke, a destroyed run, or a
nine-week defect. Do not relitigate them; if one must move, it moves with Mihai's word and the test's
baseline in the same commit.

## 1. THE FALLBACK GATE — a missing input never silently substitutes content

The rule (Mihai 2026-08-30 07:30, folded into every GEN row): **no silent substitution — where the input
is missing, build from facts that exist AND emit a named event for the absence (`ledger_empty_at_sink`
class) that tick.py prints; never a template, never a quiet default.**

- A fallback the owner ordered killed STAYS dead. The named failure shape: he says "kill all fallbacks
  so we see how it fails", the root cause gets fixed, and the fallback quietly comes BACK. That is the
  thing he called "Miserable." Root-causing does not revive a killed fallback; only his word does.
- Before writing any `unwrap_or_default()`, `Err(_) => <empty>`, or `.ok()`-and-continue in the run
  path: prove the empty MEANS empty. If a read/parse/call can fail for a reason the operator needs to
  see, the arm emits an event first (then degrades loudly), or it does not exist.
- The enforcement points that already exist: GEN-1's arming predicate (the sink description arms on
  SPEC SURFACE non-empty, not ledger non-empty — the facts are computed unconditionally); GEN-5's
  dispatch-time brief floor (a worker description below a named-fact floor — chars + at least one owned
  file + one concrete objective fact — emits a `plan_flag` WARNING event, never a stop, and the tick
  prints it); SW-6's supervision_reply gate (provider failures can no longer be laundered into verdicts
  as error-closer text; failed-look events at every site; nothing applied on Err).

### The suspect catalogue — the ten evidence-hiders (GEN-6 sweep, 2026-08-30 07:29)

Ranked in the sweep; fixes queued as GEN-6a. Each hides a real failure behind fabricated-empty content:

1. pillars empty-string write → a serialize failure becomes a GREEN gate (worst of the ten)
2. unparseable config → `levers_resolved` lies about what the run is actually running
3. corrupt ledger mini → silently truncated roll-up (a dependent reads partial history as whole)
4. replanner transport error reads as "planner declined" (a network fault becomes a decision)
5. malformed package.json = "no package.json" (a broken file becomes an absent file)
6. smoke_typescript malformed package.json → must be inconclusive-with-reason, not clean
7. parse_ast_review unparseable stdout → must be ran:false + reason, not zero findings
8. transcript writers best-effort-silent → one `transcript_write_failed` per activity key
9. Replanned lacks failed-vs-stopped (scheduler + CLI replan Err arms untagged)
10. pillars distill unparseable → must carry a reason, not an empty distillation

### The honest-empty counter-examples (what a LEGITIMATE empty looks like)

- `crates/goose-swarm/src/scheduler.rs:237` — `Err(_) => "ABSENT".hash(&mut h)`: an unreadable file
  hashes DISTINCTLY from an empty one, so absence changes the fingerprint instead of impersonating
  emptiness. This is the exemplar.
- A rules block that is an instructional CONSTANT branched on a measured predicate (the prompt surface
  is clean this way — measured 08-30 07:19).
- A fallback that STATES the measured absence in its output ("no ledger rows existed at dispatch") is
  honest; one that fills the hole with content is not.

Refusing test: the run-path `unwrap_or_default()` count-ratchet in `development_gates.rs` — the count
may only DECREASE; a legitimate new one requires proving the empty means empty in a comment and
adjusting the baseline in the same commit.

## 2. THE SPECIFICITY GATE — no generic/template task text ever reaches a model

Banned for nine weeks and it still shipped: "Integrate every module and VERIFY…", "DO EVERYTHING",
"(the owner named none — infer them)", "(task X completed)", the judge's "you already have the spec…"
overclaim. His words, 2026-08-29: *"I will ask for the millionth time, the gazillionth time … let's
remove generic tasks, let's remove everything generic and superficial."* And 2026-08-28: *"never build
generic shit … the only way that this will work is if slices are neatly done not generic."*

- Every dispatched description is assembled from THIS run's facts: spec_boot_line, spec_get_endpoints,
  spec_documented_keys, extract_signatures, the ledger, fs_delta. If the facts are missing, see gate 1 —
  emit the absence-event; a template is never the answer.
- Every model OUTPUT is a handoff: exact files, exact symbols, the concrete next step. Mihai 08-30
  07:30: *"overthinking is the model COPING with vagueness"* — the II-11 probe reasoned 11+ minutes on a
  trivial-but-vague task. The models themselves must be instructed to hand SPECIFIC next steps to the
  next model.
- The judge asserts only what was actually delivered (rules_delivered/pitfalls_delivered/dep_block),
  never context that may not exist (GEN-4).

Refusing tests: the `swarm.rs` dispatch checkpoint (`!desc.contains("Integrate every module and
VERIFY")`, ~line 4900) and the banned-phrase count-ratchet in `development_gates.rs` (drops to zero when
GEN-1 lands, then tightens).

## 3. THE BENCHMARK-LAUNCH GATE — runs start from the Benchmark view, never headless

From the campaign skill §4a (title verbatim): **"START RUNS FROM THE BENCHMARK VIEW. NEVER BY TYPING THE
SPEC INTO A CHAT."** — *"This is the single most expensive mistake of 2026-08-28 and it was made twice."*

The procedure, verbatim from the skill:

```bash
pkill -9 -f 'Goose.app/Contents/MacOS/Goose'          # never two apps
open -n /Applications/Goose.app --args --remote-debugging-port=9897
# wait for CDP, then:
node ~/goose-builds/loop-state/bench_dispatch.mjs 9897 sb-7 3
```

Why headless/chat is VOID, from the skill: typing the spec into the composer starts an ordinary CHAT
SESSION — no vendor service, no fixtures (no 12,288 payments), `{BASE_URL}`/`{DOCS_URL}`/`{API_KEY}` go
in literally, no scoring. *"Every local run that day was void."* The second mistake was subtler: on
being told it was wrong, a self-written vendor+substitution harness — still wrong, because run_build.py
already serves `vendor_service_v3`, builds fixtures, substitutes placeholders and scores.
**Before building a harness, check whether the product already has one.**

Verify a run is REAL (never take the UI's word): `pgrep -fl run_build.py` carries `--sb7`; the vendor
answers 200 on `127.0.0.1:8850/v3/docs`; or run `~/goose-builds/loop-state/first_tick_r1.sh <sha>` for
all of it. run_build.py owns the vendor — starting your own collides the port and kills the run. The
control arm is `bench_dispatch.mjs 9897 sb-7 1`, nothing edited, nothing reconfigured.

Refusing test: `development_gates.rs` asserts the skill file still carries §4a's never-headless rule and
`bench_dispatch.mjs`; `launch.sh` exits 6 with no lever/mode and 7 when the CDP dispatch never landed.

## 4. THE REAPING GATE — kill PIDs, never killpg

r2, 2026-08-30 01:31: a `killpg` aimed at two orphaned app servers (PPID 1, attempt-0 corpses) TOOK THE
ENGINE — the bare-spawn leak path never calls `setsid`, so the orphans' process group WAS the engine's.
The run died at INTEGRATE minute 139; the archive is named
`swarm-3node-r2-KILLED-by-operator-killpg-reap-INTEGRATE-139m` so the cost stays visible.

- Reap surgically, per-pid: `kill <pid>` after reading `ps -o pgid=,ppid=,args= -p <pid>`.
- Tree kills belong to `kill_app_tree` (which owns its own groups) — never to an operator killpg.
- launch.sh's per-pid reaper was always the correct model; tick.py carries the comment.

Recorded in NOW.md, memory `kill-pids-never-killpg`, campaign skill §7. Any `killpg` / `kill -- -PGID`
in an operator command is wrong on sight.

## 5. THE NO-TIME-INPUT GATE — no seconds value may decide model work

Mihai 2026-08-21 onward, restated at every violation: *"I thought we clearly said no more caps... STOP
THIS RUN AND REMOVE THAT MISERABLE CAP."* II-7 (2026-08-30, 7803faffd) made it STRUCTURAL: the provider
read/total window and both env overrides are deleted (connect-30 stays — a dead endpoint is transport);
the judge's seconds-verdicts became look-counts; `effective_idle_budget` and `UNCAPPED_SECS` are GONE,
not parked. Measured cost of the violations: the 600s read cut was manufacturing drop-1-class retries
(r2 drop 1 landed ON the arithmetic); the 420s stopwatch was the real harm behind r8's measurement.

In review this gate means: any NEW literal-seconds constant that can bound a model call — timeout,
budget, window, stopwatch, min-age on a verdict branch — is rejected on sight. Terminators are
progress-based (look-counts, action growth, byte production) or they live in the transport. There is no
knob left to set, and no knob comes back.

## What each gate cost — the rebukes, verbatim

His words (each ≤80 chars), the rule they produced, and the gate that now refuses it:

| His words | Rule produced | Gate |
|---|---|---|
| "usually these fallbacks suck … they only hide the real evidence" | facts or a loud named absence-event | 1 FALLBACK |
| "Then you root cause it and bring the fallback BACK...... Miserable." | a killed fallback stays dead | 1 FALLBACK |
| "I will ask for the millionth time, the gazillionth time … INTEGRATE EVERY MODULE" | nine-week template ban, now a ratchet | 2 SPECIFICITY |
| "never build generic shit … slices are neatly done not generic" | descriptions from THIS run's facts | 2 SPECIFICITY |
| "overthinking is the model COPING with vagueness" | outputs are handoffs: files, symbols, next step | 2 SPECIFICITY |
| "this is not going through the benchmark... it's just a stupid little prompt" | Benchmark view only; verify with run_build/vendor | 3 LAUNCH |
| "you have fucked up and haven't even realised" | never trust the launch; three checks, none the UI's | 3 LAUNCH |
| r2 killed by own killpg reap, owned 08-30 01:44 (self-inflicted, under autonomy) | kill pids, never killpg | 4 REAPING |
| "I thought we clearly said no more caps..." | no seconds value decides model work | 5 NO-TIME |
| "WHY THE FUCK DO WE STILL HAVE CAPS... REMOVE THAT MISERABLE CAP" | caps deleted structurally (II-7) | 5 NO-TIME |
| "FUCK YOU PLEASE DO ALL OF THESE IN PARALLEL … I HAVE TO CHASE FOR YOU" | batch independent calls; fan out (CLAUDE.md) | working style |
| "don't let them rot in a fucking backlog" | implement-don't-backlog (memory, prime) | working style |
| "so let's gates that stop this madness from ever unfolding" | this file and its refusing tests | all |

The refusing tests live in `crates/goose-swarm/tests/development_gates.rs`. A doc regression (this file
or the AGENTS.md GATES section going missing) fails the build the same way `now_doc_recipe.rs` does.
