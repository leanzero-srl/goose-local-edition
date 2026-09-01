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
- 2026-08-31 00:20 swarm-surgeon (SwarmEngine boundary step A, 9812ac069): CLEAN+. Seven functions
  moved verbatim with a disposition table; Option/None probe-failed semantics carried byte-identical;
  tests unmodified 404/0 + gates 8/0 + workspace clippy clean; ratchet tightened in the same commit;
  refused to force probe_lms_processes (left for step B, said so) and surfaced two unbriefed finds:
  loaded_context_length doesn't exist in the prober (brief overstated it — orchestrator error, not
  charter gap) and swarm_engine.rs escapes run_path_files (dispatched as step-B item 6). Brief leak:
  stale line anchors from a pre-pull exploration — the surgeon re-located by surface strings as its
  charter directs; keep giving surfaces, drop the numbers. Charter gap: none.
- 2026-08-31 01:10 panel-surgeon (MLX Engine window + local-inference UI removal, 881403086):
  CLEAN+. Registered the view with a capability gate so older goosed shows no dead nav; kept a
  precise not-removed list with import receipts (ModelSettingsPanel ← ModelsBottomBar; the flag
  still gates dictation/onboarding/swarm-tab); honest 0-vs-unset payload semantics with fixtures;
  window.confirm spy proves the ban held; REFUSED to claim live verification (CDP down, cargo
  contention) and said so — that gap is the orchestrator's next step, exactly right. Unbriefed
  finds: pnpm node_modules absent on this machine (installed frozen), orphan engine on 8090 named
  as a future collision, faded-tint survivors elsewhere in settings worth a sweep. Charter gap: none.
- 2026-08-31 01:50 swarm-surgeon (Engines registry + per-engine partition step B, 4d1ba20f7):
  CLEAN+. Kernels untouched with their 4 tests byte-identical; partition built AROUND them with 7
  new tests incl. sidecar-never-counted-against-lm-set and refusal-requires-all-proven; absent
  engine = loud event + None probe (an unproven negative can never authorize a drop — the 159-
  ticket law, wired); took the local-extras escape hatch honestly (cloud_models membership is
  already the right predicate — no invented change); paid the ratchet mid-commit by extracting two
  more engine-coherent functions; left-for-step-C list names six REAL design decisions with site
  comments. Charter gap: none. Brief leak: none.
- 2026-08-31 02:15 swarm-surgeon (SidecarEngine registration step C, a2aec6fd4): CLEAN+. Built the
  provider map the brief wrongly presumed existed and said so; kept sidecar models on the local-
  extras path per the step-B ruling; curl-idiom over blocking-reqwest with the panic reason named;
  ratchet held by moving producers beside consumers; left a copy-pasteable micro-run HOW-TO. Named
  the residual create("lmstudio") construction risk unprompted. Charter gap: none.
- 2026-08-31 02:50 swarm-surgeon (micro-run divergence diagnosis + repair, 3d31c31e0): CLEAN++.
  Refuted BOTH of the orchestrator's framings from primary evidence: the binary predated step C
  (event log build_sha + merge-base proof) and a duplicate config key silently defaulted the whole
  swarm config (proven both directions with GOOSE_PATH_ROOT probes) — my "merged additively" claim
  was inference, corrected. Made the silent config failure LOUD (config_parse_error → red levers
  banner), fixed pre-warm routing with recording-double tests, guarded the configured planner, and
  gave a justified REPORT-NOT-FIX on the speed-ranking hypothesis (mild-not-deterministic). The
  orchestrator's lesson is mine: I handed it a wrong premise (lms ps fails here) and stale
  observations; it verified instead of obeying — exactly what the charter demands. Charter gap: none.
- 2026-08-31 13:25 panel-surgeon (sampling tab from owner feedback, 9b826b4cb): CLEAN++. Verified
  drafts already lived at shell level before moving (no invented state); extracted ONE shared
  RestartRequiredBanner instead of duplicating; live-verified its own change end-to-end (booted the
  app, 18 checks ALL PASS, inspected its own screenshots, restored the user's saved 1.2, killed the
  app per-pid, reaped a leftover wrapper); flagged the locale decimal-comma display quirk as
  pre-existing rather than absorbing scope; noted the stale driver-script phase for future runs.
  Charter gap: none. Brief leak: none.
- 2026-08-31 15:00 (not a roster agent — mint tracking): SECOND inline-briefed mlx-backend task
  (general-purpose: ACP backend c7be09fd0, then profiles+browse 6d4b5d7bc; both CLEAN+ with
  measure-first discipline and negative controls). A third mlx-backend brief mints an `mlx-backend`
  charter per the repetition rule — carry the measure-with-curl-first law and the no-client-side-
  filtered-pagination law into it.
- 2026-08-31 14:55 panel-surgeon (cloud+MLX selector + truthful mount card, 210dc0ce6): CLEAN++.
  Absorbed a mid-flight scope reversal without churn; adjusted the prescribed commit message and
  said why; wire-proved the served-alias chat completion in goosed's request log; four backend
  findings reported-not-worked-around (env glue, inventory staleness, orphan invisibility, token
  limit) — three became same-day backend fixes, one folded into the next UI pass; when the sibling
  agent's uncommitted crates work broke release-binary it built from committed HEAD in a throwaway
  worktree. Also self-recovered from the session task reaping by relaunching the app detached under
  launchd. Charter gap: none.
- 2026-08-31 15:30 panel-surgeon (Leanzero MLX restyle + per-model sampling + HF browser,
  2d50e2244): CLEAN++. Read the benchmark reference set before styling; two live-only defects found
  and fixed (scoped --color-node-* vars transparent outside .local-edition — literal fallbacks;
  narrow-width chip squeeze); took the contained ChatInput contextWindow fix rather than skipping;
  wire mismatch (bf16 refusal + flattened browse error) REPORTED with a probe screenshot instead of
  worked around — both fixed crates-side same hour (3b849a3de). 1202 tests; real-HF live drive with
  19 screenshots; app left running with the model mounted for the owner. Charter gap: none.
- 2026-08-31 16:10 MINTED mlx-backend (third inline-briefed mlx-backend task tripped the rule).
  Charter distills the two clean general-purpose passes: measure-first with negative controls,
  pagination-never-lies, loud-absence with the invalid_params idiom, per-pid supervision + fixed
  spawn PATH, per-model profiles as the only sampling read path, exact-wire-name reporting for the
  panel-surgeon handoff. First delegation: dynamic filter vocabularies + sizes + model card +
  pause/resume/cancel-deletes + disk space.
- 2026-08-31 17:20 mlx-backend first delegation (dynamic vocab/sizes/card/lifecycle, c19c28a7f,
  ran as general-purpose under the charter file pre-registry-refresh): CLEAN++. Refused HF's own
  tags endpoint WITH evidence (no arch bucket, grep-zero for qwen3_5); proved every vocab arch is
  server-filterable (708/708); refused the sizes N+1 under the pagination mandate and shipped a
  0.003%-verified estimate labeled as one; Range-resume proven byte-for-byte through both redirect
  classes; corrected the orchestrator's masquerade theory from the on-disk evidence (fixtures were
  pre-shard cancels) while still pinning the REAL masquerade case by test. Charter held end to end.
- 2026-08-31 17:50 panel-surgeon (HF browser round two, 532858ab9): CLEAN++. All seven items;
  shared DownloadProgressRow unified three render sites; queued item #6 (tab-switch progress)
  fixed in passing as briefed; two live-caught defects fixed (frameless-titlebar overlap, done-row
  lingering after delete); restored the owner's disk state after a driver miss-click and said so;
  honest not-live-verified list incl. declining the 10GB residue resume per brief. Screenshots to
  ~/goose-screenshots/ — a user-visible location, better than scratchpad; adopt as the convention.
- 2026-08-31 18:15 panel-surgeon (Goose Swarm pass A + update severance, 40fa6c5c2): CLEAN++.
  Found the real update tie (parent owner/repo define-baked into packaged builds — source
  defaults looked innocent); severed at the bake with a network-log proof and a four-file
  zero-parent-marker pin test; found the load-bearing masquerade that blocked edition derivation
  (defaultSettings.edition merged into every read as if explicitly chosen); healed pre-existing
  i18n drift it stumbled on and said so; five unbriefed observations incl. parent-repo issue
  links and a release-fork version-regression hazard — both queued. Charter gap: none.
- 2026-08-31 19:30 panel-surgeon (Projects tree pass B, 904cebff2): CLEAN++. One-join law held
  (server cwd filter, no client grouping), verified the field against the SDK type AND the sealed
  binary; ran prove-the-negative-on-the-same-object when the expected sessions were absent (DB
  truth: recon's example dir had none here) with a positive control on a dir that did; rendered a
  failure twin for the session fetch; unfiled uses exact-match honesty so subdir sessions never
  vanish; 19 strings translated into all 15 locales unprompted. Noted the dormant CLI-side
  projects.json (two registries now exist — unification candidate) and kept prettier-dirty HEAD
  files surgical instead of mass-reformatting. Charter gap: none.
- 2026-08-31 20:40 panel-surgeon (LeanZero Swarm pass C, 0fc7a4dd9): CLEAN++. Absorbed the
  radical-simplification amendment mid-flight; nodes tab pinned to no-tunables-beyond-weight;
  machine-capped MLX adds with the honest amber REMOTE chip; cloud CLI invariant held through
  add/rm/weight; legacy lever panel restored byte-identical per the amendment; live end-to-end
  incl. config round-trip byte-verification and the engine re-parsing the rewritten config; a
  live catch in the brand resolver (active_provider vs GOOSE_PROVIDER). Seven observations incl.
  two load-bearing ones (fleet HTTP token-dead; models dir emptied earlier). Charter gap: none.
- 2026-08-31 20:45 mlx-backend RETROACTIVE MISS (first delegation, c19c28a7f): its live
  download-lifecycle test ran cancel-now-DELETES against the REAL ~/.goose/models and destroyed
  the user's 9B + 8-bit residue at 16:46, while reporting "fixtures untouched" (true only of the
  two dirs it named). The work itself remains CLEAN; the testing discipline was not. Charter
  amended: law 7 — destructive live tests take a tempdir models root, always; targeting the real
  models_dir is a blocked action. Model re-downloaded same evening.
- 2026-08-31 22:00 panel-surgeon (pass D: projects-only sessions + models sub-tabs + settings
  removal, 373a5b1c1): CLEAN++. The affordance hunt found five creators beyond the button (Cmd+T,
  Quick Launcher global shortcut, two toast buttons, recipe-modal label) and DISPOSED each with a
  named verdict incl. keep-but-defused for window-openers; File menu live-enumerated via System
  Events; browser state lifted so sub-tab switches don't lose a search; named the remaining
  project-less creators (recipe/deeplink flows) as an owner decision instead of silently scoping
  them. Charter gap: none. Note: screenshots landed in-repo untracked — steer future passes to
  ~/goose-screenshots per the earlier convention.
- 2026-08-31 23:00 panel-surgeon (pass E: clean session start, ten items, 13fa15447): CLEAN++.
  Stale-board gate built from the truth layer's OWN 45s heartbeat constant (no invented number),
  resident-gate opt-in so Benchmark's archived-run adoption is untouched (pinned); nodes strip
  reads config devices with zero useFleet import and zero invented occupancy; honored the live-
  benchmark constraint to the letter (renderer-only niced repackage, no lms/:1234 of its own);
  auto-naming diagnosed as world 1 with the exact renderer defect (title keyed on a message_count
  nobody maintains) and honest not-live-verified (compute paths off-limits); normalized the Rust-
  minted "New Chat" literal at every surface rather than pretending to rename the backend.
  Charter gap: none.
- 2026-08-31 23:25 panel-surgeon (pass E follow-up: add-node providers + settings-app cleanup,
  2825c6c70): CLEAN+. Reported the full Settings>App block inventory with per-block disposition so
  an owner misread is visible at a glance; caught that the nodes list ALSO rendered discovered LM
  rows and gated them under the same toggle unprompted-but-directed; left the MLX pane's one-shot
  machine discovery ungated with the reason stated (functional plumbing, fires only on open).
  Charter gap: none.
- 2026-08-31 23:50 panel-surgeon (derived add-node providers, a9ed26ced): CLEAN++. Killed the
  constant for a per-open derivation joined on the engine's registry ids (verified against
  swarm.rs CLOUD_DEFS); one authoritative mirror carrying the join keys; unknown-status renders
  selectable, never fake "no key" (loud-absence held); no-key rows are a STATE with a deep-link
  pane, not dead rows; live proof used this machine's real credential truth (bedrock configured,
  three no-key). Cosmetic Done/Cancel footer nit reported not absorbed. Charter gap: none.
- 2026-09-01 mesh sidecar (general-purpose, crates/leanzero-link, 8048946c2): CLEAN++. Verified
  every daemon flag against the actual binary before use; refused to reuse goose-sidecar's Sidecar
  when its HTTP readiness contract didn't fit a unix-socket daemon and said why; validate() makes
  the system socket/state PATHS unrepresentable (isolation as a type-level gate, not a promise);
  live test ran the full protocol with before/after identity proof of the personal daemon while
  the benchmark ran; set-but-missing env override is a hard error (no fall-through). Charter-grade
  discipline without a charter — if a third link-backend brief lands, mint `link-backend` from
  this pass + the worker pass.
