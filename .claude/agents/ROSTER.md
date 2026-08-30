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
