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
  dispatch-time brief floor (landed 2026-08-30: `thin_brief_missing` at the dispatch seam — a worker
  description below the named-fact floor — chars + at least one owned file + one concrete objective
  fact — emits a `thin_brief{task, chars, missing}` WARNING event, never a stop, and tick.py prints a
  `thin briefs:` line); SW-6's supervision_reply gate (provider failures can no longer be laundered
  into verdicts as error-closer text; failed-look events at every site; nothing applied on Err).

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

## 6. THE ONE-DOOR GATE — every task enters the DAG through the same repairs

Ordered by Mihai 2026-08-30, minutes after the r4 kill: *"note this down in our agentic mechanism,
or even better add it to our gates - make it a practice."*

What happened (r4, killed at BUILD+7m, archive
`swarm-3node-r4-KILLED-replan-r0-spliced-5-tasks-past-repair-shadow-reintroduced-sink-owned-README-build-7m`):

- `finalize_plan_before_dag` did its job perfectly — pin → skeleton → repairs — and the loaded plan
  was clean. Then the DYNAMIC REPLANNER, summoned at BUILD+4m with ZERO completed tasks, spliced five
  new tasks straight into the live DAG through `splice_specs`, which validates ids/deps/cycles and
  NOTHING about ownership. One of the five re-created the exact module/package import shadow
  (`app/notifierd.py` beside the skeleton's `app/notifierd/`) the repair had fixed four minutes
  earlier — with a 500-char brief (`thin_brief` fired; GEN-5 measured it live).
- Separately, synthesis gave the pinned sink `README.md` and nothing stripped it: `scheduler.rs`
  relaxes a dependent through an upstream failure ONLY when it owns no files, so a file-owning join
  is cascaded-Failed by any build failure — the app-never-binds-a-port class.

The rule: **a repair that guards one door is a guarantee that holds until the first other door
opens.** Every path that ADDS tasks to the DAG — synthesis, the flat fallback, review patches, the
skeleton prepend, the replanner splice, any future path — goes through the same ownership rules, and
the join's file-lessness is enforced structurally (a repair rule), never assumed from the planner's
good behavior.

The mechanisms:
- `repair_sink_files` (swarm.rs, in `repair_plan_flags`' chain): the pinned sink's files move to the
  first file-owning task; test `plan_repair_strips_the_sinks_files_to_a_real_owner`.
- `repair_module_package_collisions` REWRITES a shadowed module into its package (`<pkg>/impl.py`)
  instead of dropping it — the drop gutted a service task to owning nothing, rule (c) removed it,
  its brief died, and the replanner re-added the shadow; test
  `plan_repair_module_merge_never_creates_a_second_owner`.
- `repair_replan_specs` (scheduler.rs): the same rules against the LIVE dag's ownership, applied to
  every replan batch before `splice_specs`; its actions ride the `Replanned` event (loud, MILD —
  repaired, never refused silently); test `a_replan_batch_is_repaired_against_live_ownership` pins
  the r4 shape verbatim.
- The replanner is summoned only after at least one task has COMPLETED — its own prompt says the
  value is "hardening on the COMPLETED work"; with nothing completed it invents tasks from the goal.
  Progress-based: bounds when the ENGINE asks, never any model call.

Refusing test: `development_gates.rs` asserts the scheduler's splice site reaches `splice_specs` only
through `repair_replan_specs`, and that `repair_sink_files` stays in the repair chain.

## 7. THE READ-THE-WORDS GATE — the words decide; shapes only corroborate

Ordered by Mihai 2026-08-30, after catching the same miss twice in ten minutes on the r4-relaunch
looping reviewer. First: *"why don't you read the last 2000-4000 characters to understand what the
thinking sequence actually is?!"* — the diagnosis had been delivered as a shingle-duplication ratio.
Then, when the very next action was grepping detector thresholds instead of staying with the text:
*"JESUS FUCK ok listen I have asked for 9 weeks for you to read the WORDS not the fucking shape or
whatever motherfucking hell... You need to read the WORDS on both what it forms and what it thinks
to come up with ACTUAL improvements. Stop wasting my money please."*

What the words held that the shape did not: `tail -c 4000` of the lane's think.log showed a verbatim
ten-item checklist cycling — dependencies, files, difficulties, slices, models, descriptions,
integration, coverage, MUST-FIX, summaries — every item "This is good", the cycle closing with "Now
let me check if there are any other issues:", and the exit (`final_output`) never taken. From READING
that, the improvements are immediate and specific: (1) the reviewer needs an EXIT RAMP in its prompt
— an exhausted checklist means CALL final_output, re-checking a clean list is the failure mode;
(2) the judge must be shown the WORDS across looks (current tail + a verbatim span from a prior look)
so "same text as last look" is visible — a rolling 2k window reads each pass as coherent checking,
which is why looks 2-6 said OK over a 78%-duplicate stream; (3) the drift-hold must not eat the one
correct direction when recurrence is measured. The ratio (78%) corroborated the loop; it proposed
NONE of these. The words proposed all three.

The rule, both sides:
- OPERATOR: before any claim of looping, drift, judge efficacy, or output quality — read the tail
  WORDS: `tail -c 4000 <task>.think.log` and `tail -c 4000 <task>.log`. A note/OBSERVATIONS entry
  that claims a loop without QUOTING the looping words is invalid on sight. Stats come second, as
  corroboration, never as the diagnosis.
- ENGINE: supervision reads text, not only counters. The judge's prompt carries verbatim spans
  (current tail AND a span from an earlier look or 20-40k back), and its verdicts quote what they
  acted on. A detector may SUMMON; only a reader may judge.

Refusing test: `development_gates.rs` asserts both agentic docs and the campaign skill carry the
read-the-words step.

## 8. THE TRACE GATE — run the change mentally against the measured case, and write the trace down

Ordered by Mihai 2026-08-30: *"do you have a gate installed when making these engine changes to run
the changes mentally and see if they would make a difference or possibly? Have you considered
actually reading the code and not just skimming it?... install please gates around this to be more
specific at all times, to be more exact. This is why I pay a fortune for you."*

The receipt, same hour: three judge/reviewer fixes shipped for the r4b loop. Fix 3 — "a DRIFTING
verdict on a recurring stream delivers without a second look" — could not have fired ANYWHERE in
r4b's actual sequence: the one DRIFTING came at look 1 (07:15:12) when the meter held ~4.5k chars,
under its 8,000-char span floor, so `recurring()` was false; looks 2-6 said OK, so the drift gate
never ran again. The commit called it one of "the three fixes" with no such admission. It is a NET
for a sequence r4b never produced — legitimate to ship, dishonest to ship UNLABELED.

The rule, two halves:

- **THE TRACE.** Every engine change that claims to fix a measured behavior carries its trace in the
  commit message: walk the MOTIVATING RUN's real events and values through the NEW code path —
  which branch fires at which event, with the actual numbers (spans, verdicts, timestamps) — and end
  with one line: `TRACE VERDICT: would have changed the outcome — YES at <event/value>` or
  `NO, because <reason>; ships as a NET for <the sequence it does cover>`. A change with a NO trace
  may ship as a net; it may never be presented as the fix.
- **THE READING.** Before the edit: read the surrounding functions WHOLE, and follow the changed
  value to every consumer (grep, then read each hit — the drill-deeper rule, made refusing for
  engine work). The commit names what was read. Skimming five lines around the edit is how a comment
  three lines down contradicts the change.

Refusing enforcement: `development_gates.rs` pins this section and the AGENTS.md short form; the
campaign/knob-turning skills carry the trace template; in review, a fix-commit without a trace block
is returned on sight — the same standing as a new seconds-literal under gate 5.

## HOW GATES 7 AND 8 ACTUALLY DECIDE — the reader is the gate, the test is only a tripwire

**SCOPE (Mihai, same conversation): gates 7 and 8 are about HOW CLAUDE OPERATES ON GOOSE — the
development and analysis practice — not about the benchmark and not about what the engine does.
They bind every diagnosis, every fix, every review Claude performs in this repo, whether the
subject is engine code, UI, instruments, or docs. Engine behaviors that came out of applying them
(the judge's compare instruction, the reviewer's exit ramp) are ordinary work items, not the gate.**

Mihai, 2026-08-30, on the first enforcement I wrote: *"the gates you install... I assume they're not
pure deterministic garbage right? They're asking the AI agent to also assess. The real intent of the
gates are to have the AI think extra hard... and then sift through all of that at microscopic level
instead of not reading the actual information and just looking at shapes... you're once again
massaging it into your own stupidity. Rethink please that gate."*

He was right about the massage: gate 7 says "shapes never decide" and its first enforcement was a
`contains()` — a shape check. Corrected architecture, binding for gates 7 and 8 and every gate that
judges QUALITY of thought rather than presence of a string:

1. **The deterministic layer is a TRIPWIRE, never the gate.** `development_gates.rs`'s doc-presence
   asserts exist so a compaction cannot silently delete the practice. Passing them proves NOTHING
   about compliance. A tripwire may summon; it may never decide — the same law the engine's own
   supervision lives by (a detector summons the judge; only a reader judges).
2. **The gate is an AI assessment with an inspectable artifact.** At the gated moment the operator
   produces the artifact: gate 7 — the quoted spans and what the model is actually doing and why,
   quotes BEFORE any statistic, the improvement derived from the quotes; gate 8 — the motivating
   run's values walked branch-by-branch through the new code to a verdict. No artifact, no claim.
3. **The yay/nay comes from an INDEPENDENT READER of the primary material.** For anything that gates
   a kill, ships as a fix, or overturns a measurement: a separate agent is fed the RAW inputs (the
   think.log, the run.jsonl, the code at HEAD) — never the operator's summary — and concurs or
   refutes. The r4b tracer workflow (one adversarial tracer per fix, reconstructing the meter's
   state per look from the archived data) is the reference form. A fix whose independent trace says
   NO ships only as a labeled net; a kill whose independent read refutes the quoted loop was wrong.
4. **Microscopic means the primary source.** The reader reads the words/values themselves — never a
   ratio about them, never the summary of the person being checked. An assessment that cites only
   aggregates fails the gate by construction.

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
| "add it to our gates to avoid in the future - make it a practice" | one door into the DAG; the join owns nothing structurally | 6 ONE-DOOR |
| "read the WORDS not the fucking shape... stop wasting my money" | words first, quoted; shapes corroborate only | 7 READ-WORDS |
| "run the changes mentally... be more exact. This is why I pay a fortune" | every fix-commit carries its would-it-have-fired trace | 8 TRACE |
| "not pure deterministic garbage right?... Rethink please that gate" | tests are tripwires; an independent AI reading the primary data decides | 7+8 |
| "so let's gates that stop this madness from ever unfolding" | this file and its refusing tests | all |

The refusing tests live in `crates/goose-swarm/tests/development_gates.rs`. A doc regression (this file
or the AGENTS.md GATES section going missing) fails the build the same way `now_doc_recipe.rs` does.
