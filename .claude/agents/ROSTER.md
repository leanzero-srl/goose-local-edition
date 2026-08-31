# The roster's own ledger — how these agents improve, and how new ones get minted

Ordered by Mihai 2026-08-30: *"have a mechanism please for constantly improving these little parts
plus adding new ones in other areas of the project."* This file is that mechanism's memory. The
charters are living documents with the same law as the skills: a stale charter is worse than none,
because it reads authoritative.

## The improvement loop — runs on EVERY delegation, in the same turn as the synthesis

When a roster agent returns, the orchestrator grades the return in one line before using it, on
four questions, and AMENDS THE CHARTER IN THE SAME TURN when any answer is yes:

1. **Charter gap** — did the agent need a rule/fact its charter lacked (it asked, guessed, or the
   brief had to carry standing rules)? → move the rule INTO the charter, with the receipt.
2. **Charter unclear** — did it violate a rule its charter carried? → sharpen the rule with the
   quoted miss (the rebuke-table pattern: his words / the incident ≤80 chars, then the rule).
3. **Charter bloat** — did it receive charter material irrelevant to its whole surface (not just
   this task)? → trim. Charters stay under ~900 words of load-bearing text.
4. **Brief leak** — did the ORCHESTRATOR's brief carry context the agent didn't use? → note the
   over-briefing here so the next brief is leaner.

A miss that cost a commit, a kill, or a rebuke gets its receipt QUOTED in the charter — rules
without their reasons get cleaned up by later readers.

## Minting new agents — the trigger is repetition, not ambition

- The SAME kind of work briefed inline twice with no matching charter → the THIRD time mints an
  agent (the skills prime directive applied to agents). Mint = charter + this ledger's row + a
  CLAUDE.md roster-table line, one commit.
- An agent whose grading shows two DISTINCT rule-sets fighting for space → split it.
- An agent not delegated to in a long stretch is NOT deleted for that — it is checked for
  staleness against its sources instead.

## Staleness audit — the gate-auditor turned on the roster itself

Each charter footer names its AUTHORITATIVE SOURCES. When a source changes materially (a gate
moves, an invariant is added, a mechanism is deleted), the touching commit re-checks the charters
naming it — the `words-reader`/`gate-auditor` can be pointed at a charter + its sources with the
question "does this charter describe the current system?" Periodically (each campaign phase
boundary), run one gate-auditor pass over the whole roster.

## Candidate agents not yet minted (from the measured fracture lines) — mint on trigger, not now

| candidate | would own | minting trigger |
|---|---|---|
| run-forensics | post-run/post-kill archaeology: the full story from run.jsonl + activity, per phase, with citations | third inline forensics brief |
| launch-officer | the launch/kill/relaunch procedure (today the orchestrator's — deliberately, it carries the checkpoints) | only if the orchestrator's context pressure demands it |
| campaign-scribe | NOW.md/RUN-LEDGER/TICK-NOTES coherence after big days | third inline doc-sweep brief |
| feature-dev surgeon | non-swarm goose features (goose-feature-dev skill's surface) | first real feature task outside the swarm |
| publish-officer | leanzero.net board publishing (guarded exact-id publisher) | next publish task |
| cloud-campaign runner | the cloud-sb7 admission machinery | when that campaign resumes |

## The works-prover fan (standing practice, Mihai 2026-08-30)

After ANY batch of landed implementation: fan read-only works-provers, ONE PER CLAIM, each brief
SHARP — the claim, the anchors, what proof counts. Never one dummy agent told "verify the batch";
that produces an agent that "just does something to finish." Verdicts WORKS / APPEARS-TO-WORK /
CANNOT-PROVE, quotes mandatory; APPEARS-TO-WORK findings are implemented-or-scheduled on arrival
like any finding. THE CHECKING LAW (Mihai 2026-08-30 14:00): a handoff is a CLAIM, not a
delivery — the orchestrator checks every delivered build against the brief's goals before it
counts as done; big deliveries (the research fan) get their own prover pass against their
amendments. "This is not work to be done superficially." The doctrine: fallbacks only on many-happy-path arms; reachability proven
as-configured; no hard coding. First run: 2026-08-30 over the day's five landed changes.

## The module split — SUPERSEDED by the INCREMENTAL-SPLIT LAW (Mihai chose piece-by-piece over bulk); the law lives in swarm-surgeon.md + swarm-engine.md + the line-count ratchet

swarm.rs at 42k lines is accretion, not design (Mihai 15:25: "why did one of you models decide to
push everything in one file?"). The measured cost TODAY: one-agent-per-file serializes ALL engine
work (#6 blocked #7 blocked forming, serially). The split: mod research / forming / supervision /
plan / repair, inline tests out to tests/ — PURE MECHANICAL, compiler-gated (cargo test identical
before/after), never a brace script (the 34,827-line deletion is why), rules files + roster
re-anchored in the same commit. Payoff: parallel surgeons per module. Window: mid-run, binary
sealed, tree free.

## Surgeon #8 candidate (refute first, then brief): code-written skeleton — the fan opens at minute zero

r5 measured: planning 137m before BUILD, then the whole DAG serialized behind the skeleton ROOT
(8 dependents) while a heavy-reasoner authored a full routes table for what was designed as a
stub-write. REFUSED.md:56-57's revive condition ("OPEN+SYNTHESIS dominate >25% of wall") is MET
by r5's own numbers. The sharper form than the refused early-dispatch: the repair already NAMES
the skeleton files — code writes the package layout (empty __init__.py, minimal delegating
__main__ shells) deterministically at finalize; the skeleton task stops being the root; three
tasks start in parallel immediately. MUST SETTLE via refuter: the current skeleton brief asks for
a real HTTP entry shell — split "layout by code" from "entry content owned by boot-contract";
scaffolding is not the dead contract-stub form (interface stubs replacing real sources) — argue
it on mechanism. Mihai 14:35: "isn't build supposed to fan out?"

## Live queue (groomed 2026-08-30 17:55 — done items moved to the grading log)

DONE 18:2X — surgeon #11 landed all four (aa725c54a..321cdc13f); queue empty pre-build.

R6 LAUNCH CHECKLIST (at launch, not before): research chip/lanes live over CDP on the first fan;
the integrate-verify DISPATCH description is measured-tree, not the placeholder; supervision lanes
render against a real r6 log.

NEXT ENGINE BATCH — EARLY-SKELETON SHAPE C (refuter-designed, precedent-anchored; swarm.rs half
waits for #12, scheduler half dispatchable): side lane in a SHADOW WORKSPACE (speculative-twin
idiom, dispatch.rs), promote-on-match at plan load; engine-side completion at the promote seam
(match + verify_owned_files clean + no skeleton_only file -> Done via the normal seam +
relax_dependents, own loud event + completion flavor per the salvaged lesson); stuck-bail sibling
guard (!skeleton_lane_in_flight, notify on termination); the lane's device booked in the device
table at the planning->build handoff. Match compares FILE SETS only. Full anchors in refuter #4's
handoff + the grading entry.

R7 / GATED (deliberately parked, each with its qualifying condition):
- the judge desk (DESIGN-JUDGE-DESK.md) — r6 measures whether the steer→restream ladder still fails;
- early-skeleton dispatch — needs the scheduler pre-completed seam first (4 blockers in #9's handoff);
- ~~transport inactivity cut~~ REFUTED BROKEN (refuter #3): the II-7-deleted mechanism itself;
  false positives measured both directions. The honest shape (lms-ps ground-truth silent-stream
  detection) rides the shadow desk as desk_silent_stream_suspected;
- apply_split break-test coverage — before any split latch flips.

NEXT-BATCH COSMETICS: judge-lane SAID chips say 'attempt' where looks are meant; fan lane groups
sort q10 before q2; partition_delegated_decisions + delegated_decisions_ok appear live-dead;
research_request_block's armed-no-claimed-sections arm wording.

## Grading log (newest first — one line per delegation; move closed items to the per-agent notes)
- 2026-08-30 swarm-surgeon (cluster A, 6abd934c3..8a68a9c59): CLEAN PASS — six items with honest
  per-item traces including two labeled NETs (item 4's look-1 case was already covered and it said
  so; judge amendment (d)); the web_refs scan proven EMPIRICALLY against the real r5 viz.js (one
  true finding, zero false positives on 89KB); the collision handled with a hygiene commit that
  made HEAD buildable and banked both agents' extractions explicitly; item 2 landed with ZERO
  swarm.rs lines via #[path] module hookup. Ratchet 45,772. Post-interleave HEAD verified by
  orchestrator: gates 8/8, log coherent. DISPOSED: steer_superseding key mismatch (narrow) +
  GL-semantics checks -> smalls queue; cross_task rendering -> panel queue; spec_set_exceeded
  tick row -> done by orchestrator next edit.
- 2026-08-30 swarm-surgeon (shadow desk, 4218ddaea + loop-state f1517e7): CLEAN PASS — honest
  three-part trace (YES on the growth crossing, 7 min ahead of the in-loop steer; NO on recurrence
  with the reason stated — the deterministic layer cannot see semantic loops; NO on silence,
  marked not solved), the addendum resolved at the RIGHT layer (no per-poll subprocess in the
  engine; desk_silent + tick's existing lms-ps record join), char-exact replay tests across split
  UTF-8, zero judge_* event pollution. ORCHESTRATOR FAULT logged against MYSELF: I dispatched TWO
  agents into swarm.rs simultaneously (desk wiring + cluster A) — the staleness guards absorbed
  it, but the law is now absolute: ONE swarm.rs-touching agent at a time, even when briefs are
  module-heavy; the wiring lines always meet in the root. Post-#12, verify HEAD once (git log +
  full gates) given the interleaved commits.
- 2026-08-30 refuter #4 (early-skeleton Shape B): EXEMPLARY KILL-AND-REBUILD — proved the ACCEPT
  lever dead code at HEAD three ways (no deterministic producer since the F165-era tail deletion;
  parse_judge_reply has no ACCEPT keyword and hardcodes deterministic:false; revival collides with
  a pinned gate-5 test), proved the hold as specified bails the run at BUILD+0 (all four stuck
  predicates true), landed the one-door attack (pre-load tree writes = a plan-vs-disk shadow
  channel no repair compares), measured r5's zero file-set drift — then DESIGNED Shape C with
  every mechanism on a named in-tree precedent (speculative-twin shadow workspace + promote-on-
  match; engine-side completion at the promote seam per the watchdog-salvage precedent; the
  *_in_flight stuck-guard idiom; device booking). Also caught the orchestrator's misrouted desk
  addendum. Shape C QUEUED as the next engine batch (swarm.rs half after #12; scheduler half
  parallel) — lands in r6 if it beats r5's end, else first r6.5 item.
- 2026-08-30 refuter #3 (transport cut): EXEMPLARY KILL — proved the candidate byte-for-byte the
  II-7-deleted mechanism (reqwest read_timeout IS chunk-resetting; the frame "my shape differs"
  was false), falsified the premise BOTH directions from the repo's own measurements (581s
  byte-silent LIVE call at PARALLEL:2 — any threshold cuts queued work; SSE keep-alives reset
  forever — the real hang never gets cut), invoked gate 1's killed-stays-dead correctly, and
  surfaced II-7's own named alternative (lms-ps ground truth, K zero-production looks) which is
  now folded into the shadow desk as a detector. Candidate DEAD; the queue entry closes as
  refuted-with-receipts.
- 2026-08-30 swarm-surgeon #8 (judge context + attribution + mirror, d93d7ca77..e6c620749):
  CLEAN PASS, with the day's best honesty move — found ITEM 1 ALREADY CLOSED by ef23d728e and
  said so instead of re-implementing or claiming it; honored the brief's module direction
  retroactively by making that exact cluster the split law's FIRST module (−274 lines, ratchet
  tightened 47,150→46,936), and extended run_path_files to scan swarm/*.rs so the split cannot
  dodge the fallback/specificity ratchets. Extended the attribution fix to all four look
  lifecycle events (brief named two) with the reasoning declared. Agenda 2486 checked off by
  orchestrator. No charter gap.
- 2026-08-30 design fan #3 + refuter (judge desk): CLEAN — the fan produced the desk/receiver
  split with real mechanism (replayed RecurrenceMeter proven chunk-boundary-independent; the
  IdleSlotGuard ride; epoch-matched verdicts), and the refuter's 10 objections included one FATAL
  (the silence hole — a bytes-appended desk cannot see the quiet-socket class) and the decisive
  sequencing catch (G-4's revive-if is a measurement BEFORE revival — r6 qualifies it). PARKED as
  DESIGN-JUDGE-DESK.md with all amendments mandatory-on-build. The refuter also caught the design
  citing seconds constants as "progress thresholds" — gate-5 discipline holding under pressure.
- 2026-08-30 refuter #2 (decisions-into-fan): EXEMPLARY — dug the actual deletion reason out of
  commit 0409beef5 (redundancy+race, not answer quality), found the in-code quarantine pin the
  proposal would have silently breached (:27561 "open_decisions never enter the fan" — must be
  rewritten with the r5 receipt in the same commit), corrected the fold mechanism (per-slice row
  keying means decisions need their OWN partition + provenance header + a splice point that does
  not exist), scoped the repair backstop to settled-only, corrected the numbers (5 decisions;
  width 4 not 5-6; wall labeled estimate), and undersold-strengthened the case (D-questions are
  convention-by-design). ADOPTED with all amendments into surgeon #9.
- 2026-08-30 bench-scorer #2 (stale-narration sweep, 4645ff056 + loop-state 6decf25): CLEAN PASS —
  era-split every stale comment instead of deleting history (pre-cfcd32908 runs keep scoring
  truthfully, post-fix 1.0 documented as expected-not-flattery), REPLAY-verified mechanism_screen
  old-vs-new on the archived bed (one intended diff line, verdicts byte-identical), proved the
  arm_config set-equality with ast, and correctly REFUSED to touch the tombstoned legacy script
  carrying another session's uncommitted work. Flagged six stale July escalation markers —
  removed by orchestrator (dead campaign era). No charter gap.
- 2026-08-30 refuter #1 (skeleton-by-code): EXEMPLARY — refuted the orchestrator's own candidate
  in both arms on MECHANISM (dep_block hands the skeleton's real source as "build against THIS";
  code shells = CONTRACTS' frozen-behaviour surface with less information; the demote arm trips
  the judge's ACCEPT lever so the walking skeleton never gets written), corrected the inflated
  75m attribution against already-landed fixes, caught the brief drifting harsher than its own
  source note, and delivered a BETTER fix pre-licensed by REFUSED.md's own revive clause (early
  skeleton dispatch concurrent with REVIEW). ADOPTED as surgeon #9's headline. This is why
  refute-first exists.
- 2026-08-30 fallback-hunter #1 (breakdown field): CLEAN PASS — proved the field structurally
  unemittable (both producers PlanConf::default(); breakdown_json None-gates), classified every
  consumer honestly (one GUILTY instrument printing FIRED-with-None — fixed same turn; one
  self-flagged retirement candidate — tombstoned; UI null-tolerant but carrying archive-only
  renderers), verified the killed literal-0 fallback stayed dead, and named the right fix
  (delete + retire, never feed). Engine deletion + UI labeling queued.
- 2026-08-30 panel-surgeon #4 (r6 event coverage, a7c473a76/8475b196b/f983ec159): CLEAN PASS —
  read every emit site before writing a case (caught the real field shapes incl. lane_panicked's
  honestly-empty model), derived the Research chip from measured events after PROVING no phase
  event exists (phase_banner is stderr-only), found and fixed the key whitelist that would have
  filtered the fan off the board entirely (PLANNING_FAN_PREFIXES — the silent lane-hiding class),
  and retired the lever by the file's own precedent. Five unbriefed observations — DISPOSED:
  engine-side ask_max_q field removal + brief-header cosmetic -> surgeon #9 queue; hasActivity
  filter missing fullTranscript (latent lane-hiding) -> surgeon/panel small queue; research-lanes
  grouping under the chip -> panel #5 nice-to-have; r6 first-fan live CDP check -> the r6 launch
  checklist. No charter gap.
- 2026-08-30 swarm-surgeon #7 (10-item hardening batch, f2d75bebc..760fd8b32): CLEAN PASS — nine
  commits each gated before the next; the panic-cascade fix went STRUCTURAL (Vec<Result> join +
  DeviceReturn Drop guard + poison-recovery, all 5 fanout callers folded, the cascade pinned by
  an executing test) and shipped as an honestly-labeled NET; the ask truncation was KILLED not
  parameterized (dead lever doc'd for the desktop round-trip); the needle test builds its Err
  from the shared const (the pin, not a copy); item-7 decision argued (emission rides the guard
  that already owns every-exit-path). Traces: 1 NO/net, 2 YES at r5's real values, rest labeled
  nets. Five unbriefed observations — DISPOSED: score_process r5 commentary -> bench-scorer next
  pass; inert ask_max_q desktop lever -> panel #4 queue; discarded smoke-fix dispatcher error +
  odd sink_max_turns opener -> fallback-hunter/works-prover next fan. RESEARCH FAN NOW DELIVERED
  (both APPEARS properties closed). No charter gap.
- 2026-08-30 swarm-surgeon #6 (research fan v2, 81cd50d38): PROVISIONAL CLEAN, delivery pending
  the prover pass (the checking law) — all 8 amendments accounted for with anchors; hit the
  fallback ratchet with 2 new unwrap_or_defaults and removed them STRUCTURALLY rather than
  bumping the baseline (the gate refusing exactly as designed); honest trace YES at r5's real
  slices_opened (25 questions over 3 one-per-host lanes vs the unamended 6 stacked lanes pasting
  54k each); quarantine scope stated honestly (tool-menu, not sandbox). Five unbriefed
  observations incl. fanout_over_fleet silently dropping a panicked lane handle (gate-1 class,
  shared with the review fan) — QUEUED for surgeon #7. Handed back the orchestrator items
  (tick rows, phase map, NOW risk) — all done same turn.
- 2026-08-30 bench-scorer #1 (unflatter asked_when_unsure, 2cf79d6d9 + loop-state e557940):
  CLEAN PASS — chose the primary by MEASURING the three candidates on r5 (open.json rolls,
  calls.jsonl truncates at 200, open.log carries the array whole), verified 0.6-vs-1.0 through
  the new code on the real run, replayed the archived bed before/after per charter, and made ONE
  judged deviation ARGUED correctly (archived non-zero totals stay measurable — a dead call site
  can only emit 0, so non-zero is evidence; zero licenses nothing) — flagged in code, commit,
  and test rather than smuggled. Closed a note-and-ignore trap it found (dead open_decisions
  variable now surfaced). QUEUED: its finding that plan_loaded.plan_confidence_breakdown may
  also be dead post-rewire (conf_trail.py:35, lever_check.py:93,105 read it) — works-prover
  material for the next fan. No charter gap.
- 2026-08-30 panel-surgeon #3 (forming UI + follow-ups, 66a302bfb/871dc729e): CLEAN PASS — the
  join needed NO code change and the surgeon said so instead of touching it (the whole-object
  pass-through IS the law working); forming outranks both channels in laneLiveLine with the rank
  pinned by test; the replay test made hermetic without clocks (stalest-heartbeat-first ordering,
  snapshot-once); the torn-read catch KEPT and relabeled for old binaries' archives rather than
  deleted. Four unbriefed observations; QUEUED for a future panel brief: the !hasWork stacked
  empty pane, structured-call full view. Its "parent moved under me" narrative misread cf2d573d8
  (my ROSTER-only commit) — harmless, noted. Typecheck contradiction with the IDE LSP settled by
  running the gate myself: clean; the LSP view is stale.
- 2026-08-30 swarm-surgeon #5 (forming capture provider+engine, 0fd574002): CLEAN PASS — verified
  the refuter's amendment in the code ("verbatim would not have compiled" — the get_mut binding),
  chose flush option (a) only AFTER reading both digest sites whole and proving option (b) added
  no coverage during true silence, stated the residual staleness honestly (a held fragment during
  total silence — the case where nothing new is forming), made the dirty flag earn a real read
  (held_unflushed in the failure event), held the unwrap_or_default ratchet, and enumerated all
  five stale UI comments for the panel brief instead of touching held files. Two unbriefed finds:
  probe-inside-scope forming leakage (pre-existing, commented) and fix-shard forming sidecars
  landing in the shadow dir the desktop never reads (QUEUED for a future engine brief). No
  charter gap.
- 2026-08-30 panel-surgeon #2 (folded durable inspector, 030ea1cb9): CLEAN PASS — nothing clears
  on phase end (history via deriveNodeHistory beside deriveFleet, ended chips, full-transcript
  on-demand IPC with engine-codec key encoding + containment check), the honesty pins are TESTS
  (38,780 stream chars vs 128,270 durable bytes pinned; finish-under-open-inspector persistence
  pinned), memory strategy argued not defaulted (release-on-collapse over LRU: page cache already
  holds the file), and the drift-guard fixture deliberately unextended with the reason stated.
  Seven unbriefed observations. QUEUED for panel #3: jump-to-start affordance (Mihai's ask was
  the cut BEGINNING), symmetric Work-pane show-all, replay-test live-run race fix, laneless-call
  interrupted-state gap. tick_ui clickability rule revisit queued for next-build install. No
  charter gap.
- 2026-08-30 swarm-surgeon #4 (restream seed carries formed tail, 63ebe140b): CLEAN PASS — made
  the seed a PURE FUNCTION so a real test seam exists instead of theater; matched the file's
  established tail idiom (2,000-char tail_chars, the judge's own look scale) rather than inventing
  a truncation; proved the empty-tail case REACHABLE (pending-tool-request/RESTART restreams with
  no thinking) so the honest-absence branch is earned, not decorative; followed the change to
  every describer comment (the "does not go looking" narrative was now false and changed with the
  code); honestly bounded trace (YES at 09:24:12, but only the last 2,000 chars — older rows still
  lost, structural buffer bound stated). Three unbriefed observations: tick.py printer (fixed by
  orchestrator same turn), panel not surfacing the field (folded into next panel brief), closing
  line could be sharper (correctly left minimal). No charter gap.
- 2026-08-30 panel-surgeon #1 (stale-cell render path): CLEAN PASS — refused the instrument's
  diagnosis and proved the chain link by link over live CDP (digest fresh, fiber memoizedProps
  fresh, DOM frozen), found the real mechanism (useSmoothText rAF fires 0x while the window is
  hidden), fixed BOTH sides with tests (goose 2696b926d, loop-state b27818f), closed it with a
  natural experiment (window visible -> lane advanced), and correctly ruled the WASTE duplicates
  by-design (two surfaces, two sources). Four unbriefed observations reported-not-fixed. CHARTER
  STALENESS fixed this turn: the "2 skipped realfs push tests" expectation does not match plain
  `pnpm test` (they live under the integration config) — charter line amended.
- 2026-08-30 swarm-surgeon #3 (splice_briefs unclaimed-brief files, 7b5171998): CLEAN PASS —
  ordering proof given unprompted (splice runs before review/finalize, so the repair chain sees the
  appended files and repair_shared_files backstops residual overlap), read-whole list includes the
  fallback-path twin's doc comment, honest NO trace (structural net — every prior unclaimed append
  died at owns-nothing), both test halves pinned (declared-files survive; declaration-free ghost
  still removed). Two unbriefed observations reported-not-fixed per charter: RUN-LEDGER churn
  (known, auto-snapshot) and a stale splice_briefs comment narrating the deleted orphan-research
  block. SCHEDULED: fold the comment fix into the next swarm-surgeon brief. No charter gap.
- 2026-08-30 words-reader #1 (r5 opener, live): CLEAN PASS — contract followed exactly (quotes with
  offsets, shapes after words, improvements derived from quotes, falsifier named); refuted the
  orchestrator's implied stall and found two prompt defects + a missing bound instead. No charter
  gap, no brief leak. Its falsifier ("if the next ~5k chars re-open the S4 fork, that becomes real
  cycling and the judge's empty next an undelivered steer") adopted as the live watch.
- 2026-08-30 swarm-surgeon #1 (opener-text fixes, 14831a321): CLEAN PASS — honest TRACE VERDICT (NO
  for the live lane, fires next run), consumer follow-through verified unprompted (SYNTHESIS gets
  objectives whole), two unbriefed finds reported-not-fixed per charter (SliceBrief.files always
  empty + four sibling take(400) truncation sites). No charter gap; the
  report-don't-fix-unbriefed rule earned its keep on its first outing.
- 2026-08-30 swarm-surgeon #2 (files-from-objectives + truncation siblings, 5316de72f/176c4513f):
  CLEAN PASS — full reader list with per-reader verdicts, honest leave-alone reasons (events and
  test strings excluded from sentence-ending), honest NO-trace on finding 2 (ships as a net), one
  new unbriefed find (splice_briefs' hardcoded files:[] feeds the owns-nothing removal) reported
  not fixed. No charter gap. Pattern holding: each surgeon pass mines the next brief.

- 2026-08-30 ui-truth batch (general-purpose, pre-roster): two spec deviations argued correctly by
  the agent (F8 event-carry vs digest-join, F22 not-independently-shippable) → panel-surgeon's
  charter already carries the digest-join contract; ADDED to its charter: nothing (the agent's
  argument was the charter working). Brief leak: none observed.
- 2026-08-30 gate-8 tracers (workflow, pre-roster): traces validated their own replay against
  logged values to 4 decimals — that discipline is now IN fix-tracer's charter ("a replay that
  matches logs to 3+ decimals is a measurement; one you cannot validate is an estimate").
- 2026-08-30 21:05 bench-scorer (probe sources[]): CLEAN+. Corrected the ORCHESTRATOR's brief — I named
  ui/desktop/src/swarm-bench (the gitignored forge mirror) as the file; the tracked source is
  evals/swarm-bench/bench. Agent committed the source, synced the mirror byte-identically, proved the
  shipped code via the file's own --selfcheck battery plus a mutated negative control when a bare
  import proved impossible (main() at module top). Charter gap: none — the hermetic law held. Lesson
  is MINE: verify tracked-vs-mirror before naming a path in a brief (git ls-files the path first).
- 2026-08-30 21:25 swarm-surgeon (attribution fix + extraction, 7d1ae2f0e): CLEAN+. Both fixes landed,
  swarm.rs 45,772→45,529 with the ratchet tightened in-commit; honest split trace (YES for the clean()
  cut at seq 415-416, NO-labeled-NET for the sources suffix on r5 itself); shaped src_suffix as a match
  specifically to avoid a new unwrap_or_default tripping the fallback ratchet — the gates are steering
  construction, not just review. Bonus: named two more possessive-template emitters (GET ~21976,
  acquired-not-persisted ~22577) already covered by the shared clean(). Charter gap: none. Independent
  gate-8 tracer dispatched on the commit (the self-trace never suffices for a shipped fix).
- 2026-08-30 21:45 fix-tracer (independent trace of 7d1ae2f0e): EXEMPLARY. Validated its replay against
  the logged outcome 8/8 BEFORE tracing (charter discipline holding), then measured every literal's
  boundary/raw counts on the live tree read-only, confirmed both verdicts, and caught what the surgeon's
  self-trace missed: the F4 three-way boundary tie decided by RAW counts, a doc comment one rank from
  winning a shard, a gate-7 misquote (1 vs 4 errors), and the five-of-six-findings-still-can't-promote
  residual. This is why the self-trace never suffices. Charter gap: none. Brief leak: none — it was
  handed the claim and the primary anchors only.
- 2026-08-30 22:00 swarm-surgeon (transcript mirror, fce592811): CLEAN+. Mirror-aware wrappers in
  transcripts.rs with single-consumption state contracts (buffer clears iff primary succeeded, watermark
  advances once), swarm.rs 45,529→45,455, five loud kinds through note_transcript_write_failure, 4 new
  tests incl. state-consumption and unwritable-mirror shapes. The unwrap_or_default ratchet CAUGHT its
  first test helper and it correctly chose a commented unwrap — absent must panic, not impersonate
  empty. Unbriefed find: primary .calls.jsonl still best-effort-silent (GEN-6a class) — folded into the
  next surgeon brief same-turn per implement-don't-backlog. Charter gap: none.
- 2026-08-30 22:15 fix-tracer (independent trace of fce592811): CLEAN+. Byte-exact replay of the
  attempt marker against the rescued log, adversarial sweep classifying all ten fs-write sites,
  non-fix-lane arming checked, and the redispatch-starts-blind residual — the read half of the
  mirror the write-half fix could not see. Two factual slips in the surgeon's self-trace caught
  (tool-row count, omitted drifting verdict). The pattern holds: every self-trace so far has been
  honest in verdict and wrong in at least one detail — the independent read is earning its cost.
- 2026-08-30 22:40 swarm-surgeon (attribution residuals + calls loudness, 6585f0845): CLEAN+. All four
  landed, swarm.rs 45,455→45,453, and THE TRACE GATE WORKED IN CONSTRUCTION: walking r5's values caught
  that a pure comment-strip would flip F1 off app/sync.py (its hits are a comment + docstring) and
  regress the run's ONE real promotion — the shipped 4-tuple ordering key is trace-bought, not
  designed. Also closed a latent concurrent-claim race (two .js groups, one .html) by resolving
  ownership sequentially before the fan, and refuted my brief's round-1 premise honestly (httpapi.py
  is F3's own group; runner-up claim correctly refused until F3 lands). Charter gap: none.
- 2026-08-30 23:00 fix-tracer (independent trace of 6585f0845): CLEAN+. Mechanical old-vs-new replay
  validated against the run's dispatch events; checked the tree-state question the surgeon skipped
  (best-tree vs live — sync.py's docstring hit moved lines but the profile held); downgraded fix 1's
  YES to PARTIAL with the honest firing condition; found the omitted runner_ups[sync]=httpapi and the
  F4-literal single-point-of-rescue. Pattern holds (4/4): self-traces honest in verdict, incomplete in
  at least one detail. Residuals deliberately NOT implemented — r6's task_owns events are the
  measurement that decides which of them is real.
- 2026-08-30 23:20 swarm-surgeon (redispatch reader + verbatim literal, 0dc8c297f): CLEAN+. Fix 1's
  premises verified in the live tree (16 mirrored rows, all attempt:0) before coding; fix 2 shipped as
  an honest NO-net with a per-finding winner table proving zero r5 change; took the mid-flight
  addendum (stale promote comment) without scope creep; minted briefs.rs for the softened multi-file
  note instead of growing swarm.rs. Named the same-class sibling (build_task_ledger_row) instead of
  silently skipping it — that sibling is now dispatched per drill-deeper. Charter gap: none.
- 2026-08-30 23:55 swarm-surgeon (reader-class closer + winner class-split + addendum, fc3e907a7):
  CLEAN+. Complete reader census with per-reader disposition (converted / already-fixed / comment /
  test-write / desk-correct), swarm.rs 45,448→45,265 via moving build_task_ledger_row beside its
  reader, all three fixes labeled honest NETs with real-value walks (16 mirror rows classified
  {boot:2, import:1, other:7}), took the mid-flight stub-note addendum with tests. Measured README
  greps 0 TODAY because the __main__ shard rewrote it at 19:56 — checked the live state rather than
  assuming the morning's. Charter gap: none.
- 2026-08-31 00:15 fix-tracer (independent trace of fc3e907a7): EXEMPLARY. Re-derived the ledger-row
  counts from the raw 16 rows through the commit's own classify_command; re-ran the census repo-wide
  including ui/desktop and evals; proved fix-2's NO at the ACTUAL attribution event via the best-tree
  snapshot (stronger than the surgeon's today's-tree claim); and found the residual that ENGAGED in
  the motivating run — skeleton_note's missing repairing disarm, delivered twice to the 0-edit shard.
  5/5 pattern: every self-trace honest in verdict, incomplete in at least one detail. The independent
  read remains the cheapest defect-finder we have.
- 2026-08-31 00:35 swarm-surgeon (pitfall lessons + skeleton disarm, a3ffe49d1 + c3c5ec42d): CLEAN+.
  Lessons are pure class-knowledge (zero r5 names — "measured:" clauses describe the shape, not the
  instance) and the trigger craft is the standout: "render"/".js"/"api" REJECTED with measured
  collision reasons, prospective YES proven on r5's real task texts (viz-field at "webgl",
  ledgerd/integrate/fix-shard at "endpoint"). Took the mid-flight skeleton addendum as a clean second
  commit gated on the shared repairing predicate; checked cli_contract_note and correctly left it
  armed. Mechanism verification: the finder-tracer's walk stands as the independent read; I verified
  the implementation hunks directly. swarm.rs 45,265→44,901 across the pair. Trap recorded: the
  DOMAIN_PITFALLS trailing-backslash fusion (tripwired by pitfall_items_match_triggers). Charter gap: none.
- 2026-08-31 00:55 panel-surgeon (verdict surface + endgame visibility, 33deb8d39): CLEAN+. Split
  HEAD-already-had from added per defect; proved r5's invisible tail was the stale binary (HEAD's
  reflect lane already streams) rather than inventing an engine key; verdict state rides the one
  reducer (digest-join law held); fixed the KnownActiveBugs caption lying when the verdict is
  retracted; corrected my brief's "10 tasks" to the real 11. Unbriefed find worth doing: a
  drift-guard test pinning the panel's supervisionLaneKind mirror to the engine's fixtures — the
  doc said EXACTLY while five kinds behind; queue with the priorities surgeon batch.
- 2026-08-31 01:20 panel-surgeon (Benchmark publish flow, e564bc236): CLEAN+. Read the FAILURE'S WORDS
  from the live app over CDP instead of theorizing — the 400's verbatim tier message named the real
  blocker (server allowlist predates sb-7.0-rc's T/X/R/E tiers; MAX_CHECKS 90 vs 91) — and correctly
  REFUSED the client-side filter that would silently drop scorer evidence (fallback gate). Also
  caught the success copy lying ("for review" after posts went live-immediately). All four asks
  landed with 10 new tests. Server-side fix dispatched same-turn. Charter gap: none.
- 2026-08-31 01:35 fix-tracer (node-weights trace): EXEMPLARY. Replayed the OLD path against the
  archived dispatches until they matched categorically before trusting the NEW-path walk; measured
  the confound itself (sink ended the SAME SECOND the re-rank sampled; gabee idle 2h47m); refused to
  state a saved-minutes number the run never measured, labeling the ~30m an estimate. 6/6: every
  self-trace corrected in at least one detail (here: smoke_fix_target had zero consumers fire in r5).
- 2026-08-31 00:30 general-purpose (site API tiers, 07cc27b): CLEAN+. Worktree off origin/master to
  avoid the concurrent seeder's checkout; derived the rejection message from the tier array so set
  and error cannot drift; proved its new tests refuse the UNFIXED route (4/5 fail before, 5/5 after);
  live before/after probes verbatim with a no-write 503-sentinel test design. The route was the last
  seven-letter holdout — every other layer already spoke eleven. Pattern for the roster: refusal-
  proven tests (run them against the defect first) is worth adopting in surgeon charters.
- 2026-08-31 00:50 swarm-surgeon (priorities, c47674fd4): CLEAN+. The exact non-hardcoded shape Mihai
  ordered: 17 provenance sources authored at the push sites, severity a pure function of source
  (untagged = loud "unsourced", last), MILD-only ordering (texts byte-identical, parallel arrays),
  passed-partition membership untouched with the pinned test proving it, and an EMPIRICAL replay
  (swarm repair on the r2 archive returned max_severity critical, zero orphans). swarm.rs 44,511 →
  43,982. Queued smalls from its observations: stale known_active_bugs after a late green round;
  PYTHONDONTWRITEBYTECODE missing in handle_repair (r2's archive already carries a __pycache__).
- 2026-08-31 00:45 fix-tracer (priorities trace): EXEMPLARY. Deterministic replay to the note-text
  byte level; diffed the partition needles one by one; found the unnamed behavioral widening
  (ownership pairings now claim in severity order); downgraded the headline to PARTIAL per the
  gate's own template while crediting the body's honesty. The severest->fastest pairing is a
  scheduling probability, not a structural gate — recorded for r6 reading.
- 2026-08-31 01:00 bench-scorer (closure config r6, d5bd48d46): CLEAN+. Authored the config with real
  hashes and stated-null absences, probed one layer past the first refusal to prove unfixability
  (missing launch-time receipts, not just the port pin), refused the identity-falsifying "fix", and
  left the live run untouched (verified after). The refusal IS the deliverable — it surfaced the
  closure-vs-Benchmark-view doctrine conflict as an owner decision instead of a silent workaround.
- 2026-08-31 01:15 swarm-surgeon (three smalls, c6e511ba1): CLEAN. All three landed with the split law
  paid (elide_middle out; 43,982→43,933); honest NET label on fix 1 and an honest deviation (the
  inline latch is untestable without an unordered extraction — said so instead of gold-plating).
  Left a fused doc-comment verbatim rather than editorialize — right call. Did not push (charter
  ambiguity?) — pushed by the orchestrator; consider adding push-after-commit to the charter.
- 2026-08-31 02:00 swarm-surgeon (ladder reset + hold, 1fe842a8e): CLEAN+. Enumerated the seam state
  reset-vs-kept with reasons (the tracer's verification target, by design), routed producing calls
  through a HOLD that keeps their chars, made ignored structurally unreachable on a fresh attempt,
  and kept r5's stuck-shape coverage. Side-find of the day: r6a's judge once emitted an
  assistant-limit refusal as a DIRECTION — a new r7 detector class. swarm.rs 43,933→43,715.
  Unbriefed gaps it named honestly: tick.py (fixed same-turn) + panel don't know judge_restream_held.
- 2026-08-31 02:20 fix-tracer (ladder trace): THE STRONGEST OF THE CHAIN — first outright refutation
  of a guardrail claim, from the archive's own numbers (r5's save ran at 0.0159 vs the 0.25 trigger;
  no recurrence measurement exists anywhere in r5's record). Replay validated against the desk rows
  to 3-4 decimals; enumerated all ~38 ladder locals; named the eternal-hold liveness answer honestly
  and the reader-rung remedy in gate-7 terms (words, not a counter). 8/8 pattern holds.
- 2026-08-31 09:05 swarm-surgeon (fan fold, ecc7f168d): EXEMPLARY — REFUTED THE BRIEF from the archive
  before building (0 of 48 raises ever dispatched; the growth curve was a queue drain), then shipped
  the part that was real (raises fold into briefs — previously unread), refused to mint a one-value
  generation field as hardcoding, and named the actual fan cost driver (933s/question mean, two
  65-min outliers). The orchestrator's brief committed the shape-not-words error the gates exist for.
- 2026-08-31 09:30 MINTED tick-surgeon (Mihai: "let's have a proper tick-surgeon then that is invested
  into this!") after the vigil degraded to tick.py|grep + commit and a shape-read produced a phantom
  snowball loop. Charter carries the three tick rules inline (words + generations, delivered content,
  quoted improvement notes), a fixed-shape report, its own delta memory (.vigil/last_tick.json), the
  kill-checkpoint field table with proof, and the doctrine (gate 7, microscopic, works-not-appears,
  purpose-over-prose, kill-pids, no-caps). Read-only toward the run; recommends, never kills. The cron
  protocol now delegates the reading to it every tick. First return graded below.
  GOTCHA (registry): a newly minted agent file is NOT callable by subagent_type until the session
  restarts — the roster loads at session start (works-prover hit this too). Bridge: general-purpose
  agent instructed to read + execute the charter file. The tick cron carries that fallback.
- 2026-08-31 11:40 tick-surgeon FIRST RETURN: EXEMPLARY — the vigil as ordered. Read every active
  lane's words with quotes and a class each; read 5 delivered minis' CONTENT and caught two answers
  WRONG against the spec (Health shape; webhook counters) plus a same-slice contradiction — a defect
  invisible to every counter — and pinned the mechanism to research.rs:342-372 (claimed sections
  only, no prior minis). Answered the orchestrator's question with evidence (no 65-min shape; q0 was
  1210s with re-derivation the judge steered correctly). Honest self-grade with five named holes.
  Charter gap: none. Bloat: the 40-line cap held. Brief leak: none — it was handed only the run dir
  and the question. Fix dispatched same-tick (research grounding + intra-fan snowball).
- 2026-08-31 11:55 tick-surgeon #2: EXEMPLARY. Real deltas from its state file; spot-checked two new
  minis LINE-BY-LINE against the spec (notifierd-q0 grounded on every cited line; api-q1 body right,
  both raised items FALSE with the spec lines that refute them); caught a contradiction FORMING in a
  live lane's words (source:"send" vs L210-213/core-q0) before it landed; verified the mechanism in
  research_request_block and the lane's 11 shell calls (grepping sibling think.logs for spec text).
  Named four holes honestly (no research_planned event; no per-look node-time; three looks pending).
  Two specifics fed to the in-flight grounding surgeon same-tick. Charter gap: none.
- 2026-08-31 12:10 tick-surgeon #3: EXEMPLARY. Answered both questions with the words (api-q2 STILL
  shipping "send" while it READ core-q0's CHECK vocabulary — 6 calls into the ledger — the sharpest
  form of the grounding defect: the material was in front of it and the prompt's "pick a convention"
  overrode it); the raised audit measured 6 FALSE / 4 GENUINE with the spec line refuting each false
  one. Steer-obeyed-in-stream observed (api-q3 tightening on the nudge's exact list). Honest holes.
  Charter gap: none.
- 2026-08-31 12:10 swarm-surgeon (research grounding, b7516297b): CLEAN+. Both addenda folded; the
  system sentence directly targets the "not the lane's to pick" defect surgeon #3 measured; events
  landed (planned/context/phase/not-persisted, all derived); honest same-instant NET label; orientation
  cluster extracted to pay the wiring (43,684→43,444). tick.py taught the new events same-turn.
- 2026-08-31 12:20 refuter (judge turn-cap): EXEMPLARY. Confirmed claim 1 with the full path (probe
  max_turns=1 -> SessionConfig -> agent.rs:2067; deliberate Q&A contract, all work lanes structurally
  uncapped) and REFUTED claim 2 as stated (no turns exist after the verdict at max_turns=1) while
  finding the worse truth underneath: r6a's seq-58 DRIFTING was MANUFACTURED by parse_judge_reply's
  fallback from the cap filler alone, and live next fields carry the filler as a trailing line. Also
  found the schedjudge path already owns the detector the omni path lacks — the fix became a mirror,
  not an invention. The corrected fix (filler = failed look; strip trailing MAX_TURNS_MESSAGE via the
  shared-const pattern) is dispatched. NO-CAPS ruling parked for Mihai: whether a 1-turn supervision
  Q&A reply counts as "model work" — the repo's own comment calls a turn count "a volume cap wearing
  a different hat", but every measured harm was the filler leak, not the cap ending healthy looks.
- 2026-08-31 12:25 tick-surgeon #4: EXEMPLARY. Both WATCH items closed with quotes (api-q2 SHIPPED the
  poisoned source:'send' — a builder following it verbatim throws IntegrityError against core-q0's
  CHECK; false-raised now 7/11; judge look3 abandoned correctly, its pre-abandon think crediting the
  lane's shift "from deliberation to composition"). Corrected its OWN tick-3 prediction honestly
  (api-q4 needed a 4th nudge, not free emission). Caught the snowball WORKING in notifierd-q2 (adopted
  q0's raised ntf_<seq> convention) — evidence B's mechanism pays. New design finding parked: a judge
  steer DICTATED technical content ("vendor send follows as background step") and the lane was right
  to ignore it — steers command emission, never author design. Charter gap: none.
- 2026-08-31 12:35 tick-surgeon #5: EXEMPLARY. Closed its own tick-4 hole (ntf-q1 verified verbatim
  vs L362-364); SNOWBALL CONFIRMED with the words ("use the sibling-slice convention already recorded
  for this store" — cross-host, via the ledger block, before timing could explain it); caught a FIFTH
  contradiction (q1's in-memory notifications counter vs q3's SELECT COUNT(*), L360-361 grounding q3);
  false-raised STOPPED growing (7/18 total — the new 7 all genuine); two nudges obeyed with
  timestamps. New engine-defect candidate: ok-verdict judge_look events blank established/next while
  the judge's formed words carried substance — the archive loses clean-look words. Charter gap: none.
- 2026-08-31 12:45 tick-surgeon #6: CLEAN+ and ON BUDGET (62.7k tokens / 12 calls — at the new target
  before the tightened charter even reached it; the state file is doing its job). Closed the tick-5
  hole (all 5 stale judge lanes abandoned cleanly, none leaked); best find: web-viz-q0 MINING run.jsonl
  to recover spec sections its brief omitted ("the full layout math IS embedded in run.jsonl") —
  ~20k chars burned re-mining; b7516297b's spec-path+index fix is exactly the remedy, r6d measures it.
  Its ASK-FOLD waited=5s suspicion: answered from doctrine — the 5s bounds waiting on a HUMAN under
  benchmark mode (plan §5's exempt class), not model work; no code read needed. Charter gap: none.
