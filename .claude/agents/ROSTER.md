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
| studio-remake surgeon | LeanZero Studio remakes of non-swarm desktop surfaces (shell/nav, session chrome, hub, benchmark/settings, MLX view) | TRIPPED 8× on 2026-09-02 (design system, stage-1 polish, R1–R6) — deliberately NOT minted on the grading pass: panel-surgeon's Learned 2026-09-02 block absorbs the Studio facts; mint if the next campaign briefs a remake inline again |
| ban/leftovers sweeper | DESIGN.md ban sweeps + primitive promotion after a remake wave | TRIPPED 5× on 2026-09-02 (chrome leftovers, leftovers-2/-3, tiny ban fix, lz-extensions) — NOT minted; same absorption |
| diff-reviewer (lens review) | branch-review lenses (worker, Rust security, UI, engine) that feed the refuter | TRIPPED 4× on 2026-09-02 — NOT minted; each lens is one brief with the refuter as its gate (kill ratio on the day: 4 refuted, 3 partial, 9 fixes corrected of ~34) |
| non-run fix-tracer | gate-8 traces over desktop/link/crate commits with no archived run (live config + the refuter's sequence as the motivating case) | TRIPPED 5× on 2026-09-02 — NOT minted; fix-tracer's Learned 2026-09-02 block states the live-config motivating case instead |

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

## Grading pass 2026-09-02 — the branch-review + fix-wave + Studio-remake campaign (one pass over 50 returns)

Graded in one sitting from the campaign's delegation log (the session scratchpad's review-findings-index.md),
after the functional work landed (consumption policy). Every amendment below was verified against the log or
the tree before it went into a charter; nothing speculative was folded. Grades: gap / unclear / bloat / leak —
`—` = no. Bloat was observed in NONE of the 49 graded returns. Two facts from the orchestrator's brief could
not be verified in any primary source and were NOT folded: "vitest could not resolve node_modules in a
worktree" and the OTP sender's local part (the env holds a leanzero.atlascrafted.com address; only the
domain is written into link-backend). Row 50 (the works-prover live pass) was still running — ungraded.

| # | agent | task (shas) | gap | unclear | bloat | leak | amendment |
|---|---|---|---|---|---|---|---|
| 1 | general-purpose (worker lens) | branch review W-H1..W-L12, probed | no charter | — | — | — | CANDIDATE diff-reviewer |
| 2 | general-purpose + fallback-hunter | Link Rust R-* / FH#1–13 | ratchet coverage of the surface | FH#7 GUILTY on an unconstructed sequence (refuted) | — | — | fallback-hunter Learned |
| 3 | general-purpose (UI lens) | U-H1..U-L10 | no charter | — | — | — | CANDIDATE diff-reviewer |
| 4 | works-prover | Link as-configured WP-1..7 → APPEARS | receipts list absent | its own prior "proven e2e" was a scripted chain (self-caught) | — | — | works-prover Learned |
| 5 | general-purpose (engine lens) | S-H1..S-L11 | no charter | — | — | — | CANDIDATE diff-reviewer |
| 6 | refuter | Link Rust fallbacks (6 C, 1 X, 2 new defects) | — | — | — | — | refuter Learned (construction finds the defect) |
| 7 | refuter | worker (7/7 C, live KV probe) | — | — | — | — | refuter Learned (probe the live deployment) |
| 8 | refuter | works-prover claims WP-2/WP-4 (2/2 C+corr) | — | — | — | — | — |
| 9 | refuter | desktop UI (5 C, 1 P, 1 X; 2 fixes wrong) | renderer CSP source unknown → its U-M3 correction was later refuted | — | — | — | refuter Learned (a corrected fix is a claim) |
| 10 | refuter | Link Rust security (7 C, 1 P, 2 X) | — | — | — | — | — |
| 11 | refuter | swarm engine (7/7 C, 2 P on "silent") | — | — | — | — | — |
| 12 | link-backend | worker wave 1 ffc2e38a0..94fa06376 | Funnel XFF semantics; nodeSecret contract | — | — | — | link-backend Learned |
| 13 | panel-surgeon | desktop logic F 03b291c84..6512c99e7 | renderer CSP is a static meta (U-M3 regressed) | — | — | — | panel-surgeon Learned |
| 14 | panel-surgeon (inline: design system) | Studio d64068fb6..77dd6f690 | cx-not-cn; font-lz-*; no pnpm add | — | — | — | Learned + CANDIDATE studio-remake |
| 15 | link-backend | W-L9 sweeper 5ab48a8c2 | — | — | — | — | — |
| 16 | panel-surgeon (inline: polish) | stage-1 575a141a3..cdcc6588c | just run-ui broken → package + CDP 9897 | — | — | — | Learned |
| 17 | swarm-surgeon | engine D a7ffb0516..7f74d3730 | worktree + own target; LM 401 premise | one-door: named the wrong door (reachability not walked) | — | — | swarm-surgeon Learned |
| 18 | mlx-backend | sidecar E fcab044c0/71189f66b/e09790ad0 | lsof clients; process tree; 4.91s gap | law 4 vs the proof-gated killpg; breaker miscount (4th death) | — | — | mlx-backend Learned |
| 19 | panel-surgeon (inline: remake R2) | session chrome 9ace453ee.. | px-0 vs px-2.5 | — | — | — | Learned |
| 20 | panel-surgeon (inline: remake R1) | shell/nav/projects 8ccdbd9b9.. | /usr/bin/grep -E for the ban grep | — | — | — | Learned |
| 21 | fix-tracer | sidecar + swarm (3 + 2 + one-door) | motivating case = live config, not a run | — | — | brief framed an archived run; tracer used the live fleet (right) | fix-tracer Learned |
| 22 | panel-surgeon (inline: remake R4) | benchmark + settings efafa4700.. | — | — | — | — | — |
| 23 | link-backend (seam) | goose link layer C1 7a745914f..94fd2ccdf | uppercase key = env override | — | — | — | Learned |
| 24 | panel-surgeon (inline: remake R3) | hub/link/nodes e610a589d.. | CDP driver must select role=radio/data-value | — | — | — | Learned |
| 25 | bench-scorer | tick.py rows for the new events | BRIEF ERROR (repo path; orchestrator's) — fixed 5a791375c | — | — | brief path wrong | confirmed reads right; Learned (5 unprinted events) |
| 26 | panel-surgeon (inline: remake R5) | MLX engine + models 3713e606c.. | — | — | — | — | — |
| 27 | panel-surgeon | Q2 confirm-on-close a80209a40..805d6a00d | — | — | — | — | — |
| 28 | panel-surgeon (inline: remake R6) | SwarmRunPanel d0acc8581..95ee1b8fb | — | — | — | — | — |
| 29 | link-backend | crate B (14 commits b7f1c3425..5938a2bf7) | LOCAL_PEERPID readiness; EINVAL = sun_path | — | — | — | Learned |
| 30 | panel-surgeon (inline: leftovers) | chrome leftovers c5743a61c..bb767a6e6 | size-N for square buttons | — | — | — | Learned + CANDIDATE sweeper |
| 31 | works-prover | worker → WORKS (7/7, live probes) | — | — | — | — | Learned (auth_verified never seen) |
| 32 | fix-tracer | desktop logic (4 C/P; U-M3 REGRESSION found) | — | — | — | — | Learned (composition trap; refuting a fix) |
| 33 | panel-surgeon (inline: leftovers-2) | 21614dfa8..f7ed93434 | — | — | — | stale finding (tone migration already done) | — |
| 34 | fix-tracer | link layer (6 C, 2 P; AgentBusyGuard race found) | — | — | — | — | Learned (409 unreachable at that commit) |
| 35 | fix-tracer | crate (7/7 C; errno measured on 1.98.5) | — | — | — | — | — |
| 36 | general-purpose | AgentBusyGuard race 70f7718c7 (goose-server) | no charter | — | — | — | — |
| 37 | panel-surgeon (inline: leftovers-3) | caaac7221..26832ab02 | — | — | — | — | — |
| 38 | panel-surgeon | U-M3/U-M8 correction 987889548..62d12d110 | probes belong in main | — | — | — | Learned |
| 39 | mlx-backend | Q1 busy signal 527eb821a..ed409244f | /v1/status; 503 admission cap | touched ui/desktop under a brief sanction — the NEVER lacks the seam clause | — | — | Learned |
| 40 | panel-surgeon | R7 timestamps + polish 8156093d7..1f36638f5 | font-extrabold compiles | — | — | — | Learned |
| 41 | link-backend | crate follow-ups 16730333a, 4a0e912a8 | — | — | — | — | Learned (look-count demotion) |
| 42 | link-backend (seam) | C2 goose-serve 59074c710..04abe8a9a | — | `cargo remove` root collateral committed partially | — | — | Learned (link + swarm) |
| 43 | panel-surgeon (inline: lz-ext) | 4d901286a..0787de9fc | — | — | — | — | — |
| 44 | fix-tracer | 5 newest commits (4 C, 1 P; freeBusy asymmetry) | — | — | — | — | — |
| 45 | general-purpose | i18n 9291aa9c2 | no charter | — | — | — | — |
| 46 | panel-surgeon | tiny ban fix b8072447d, 594cca9d2 | — | — | — | — | — |
| 47 | swarm-surgeon | wave-2 7d08bc4b3..d82f8e711 | LMSTUDIO_API_KEY set nowhere here; 503 cap scope | — | — | — | Learned |
| 48 | panel-surgeon | node-field follow-up cffe7bc1f..ba5d54a14 | main has no secret-store accessor | — | — | — | Learned |
| 49 | panel-surgeon | node identity e2e 3ab2197bf..03e88d064 | renderer secrets masked | — | — | — | Learned |
| 50 | works-prover | LIVE PASS (packaged app + OTP hand-off) | NOT GRADED — still running at this pass | | | | grade on return |

Totals: 23 charter gaps (all folded), 6 unclear (each sharpened with its receipt), 0 bloat, 3 brief leaks
(rows 21, 25, 33 — two orchestrator errors, one stale finding), 5 no-charter returns → the four candidates
above. Staleness seen, not fixed on this pass (existing text is kept by rule): panel-surgeon's gate line
about the realfs push tests is garbled from a 2026-08-30 edit; mlx-backend/link-backend trailers still say
"Claude Fable 5".

## Grading log (newest first — one line per delegation; move closed items to the per-agent notes)
- 2026-09-05 general-purpose ×3 rounds (swarm_router.rs, the Swarm provider's idle guard): CLEAN+ / one
  premise miss the live proof caught — round 1 built pick/sticky/queue/failover/named-error with 13 tests and
  found its own early-permit-release bug via the failover test; round 2 (own-context per route, pool context
  limit) read the trait callers before deciding; round 3 fixed probe_mlx reading THIS process's manager (the
  CLI got "engine is stopped" while the app served on :8090) by probing the engine over HTTP with wiremock
  tests. Brief leak: none. CHARTER GAP: no surgeon owns crates/goose/src/providers — a third inline brief
  there mints one (candidate: provider-surgeon: Provider trait seams, retry semantics, no seconds literals,
  the node-is-the-truth rule).
- 2026-09-05 panel-surgeon ×3 (fleet truth A–H, 1776 green; provider allow-list + migration, 1791 green;
  resumed-session migration, 1798 green): CLEAN++ ×3. Flagged the GOOSE_PROVIDER-vs-active_provider edition
  derivation (→ the orchestrator's edition-defaults-LOCAL fix) and that swarm-build was not yet in Rust.
  Charter gap: none. Bloat: none.
- 2026-09-05 swarm-surgeon (sidecar probe/mount loudness + settle_prewarm extraction, 36/36): CLEAN++ — paid
  the line ratchet with a whole-block extraction and traced the happy path byte-identical; caught by the
  proof chain on one clippy nit + the ratchet (both fixed on resume). Charter gap: none.
- 2026-09-05 fallback-hunter (11 findings on the sidecar/link surfaces): CLEAN — 1,2,3,7,8,9 confirmed and
  landed; 4/5/10/11 correctly judged design/pre-existing. Charter gap: none.
- 2026-09-05 refuter (3 desktop claims + 6 sweep): CLEAN++ — A/B confirmed-with-correction (worse on the
  MLX-only pool than claimed), C REFUTED (the join was clean), every sweep item file:line + fix direction.
- 2026-09-05 works-prover (MLX-node swarm path): APPEARS-TO-WORK with 7 findings; finding 2 (mount failure
  stderr-only, run proceeds into a dead port) became the settle_prewarm exclusion. Charter gap: none.
- 2026-09-05 Explore ×2 (provider surface map; idle-node map): CLEAN++ — facts only, file:line, exactly the
  tables the design needed. Nothing to amend.
- `swarm-surgeon` VA-149 + VA-148 (00:1x; 37 tool uses, 234k tokens, 19 min, cargo-free): on charter — wired both pure modules at every call site, deleted the old scan whole, verified r6h's render verdict identical from the archive (0 findings, +31 verified) and the dom scan's 2 → 1, moved the three delivered-at-dispatch events behind the turn-2 seam and made the judge read the rest as undelivered, baselines from replicas, residues named. Charter gap: none.
- `bench-scorer` VA-028 (00:0x; 17 tool uses, 97k tokens, 7 min): on charter — found the real mechanism (the state file rewritten per tick, so replays wiped the live baseline) instead of the filed guess (slot vs dir), three replay proofs incl. the live run, the same-class `.code_health` file fixed, byte-identical r6h replay. Charter gap: none.
- `panel-surgeon` VA-031 + VA-041 (00:0x; 34 tool uses, 161k tokens, 12 min, pnpm only): on charter — found that one of the three events is archive-only (deleted by VA-089) and rendered it for archives while saying so, fields verbatim with the emit line, the cover-before-dispatch ordering handled, a clean worktree, residues reported (→ VA-150). Charter gap: none.
- `swarm-surgeon` VA-033 (00:0x; 29 tool uses, 154k tokens, 11 min, cargo-free): on charter — the open signal chosen from the landing map's own lifecycle, no `lane_unopened` invented for the decisions lane, the residual window MEASURED (6–10 ms) and its swarm.rs line named, an honest NO trace. Charter gap: none.
- `swarm-surgeon` VA-146 (23:5x; 12 tool uses, 129k tokens, 8 min, cargo-free): on charter — one witness fn beside the existing three (no second delivery path), the flag armed at the steer's emit and cleared by the fact (an owned file), the hold's 55-line history moved to ladder.rs as doc under the split law, the trace both ways. Charter gap: none.
- `bench-scorer` VA-148 tick.py half (23:5x; 18 tool uses, 132k tokens, 7 min): on charter — fields from the staging emit sites, present-only proven on two replays, the minutes-to-first-write measurement printed for r6h AND r6j (the first real baseline for VA-144), and it found an unregistered live event (`settled_list_relisted`) that VA-135 missed. Charter gap: none.
- `swarm-surgeon` VA-114 + VA-088 (23:4x; 28 tool uses, 258k tokens, 23 min, cargo-free, three files): on charter — pure modules with the r6h classifications pinned by test (the false HIGH gone, the two right ones unchanged; the final tree renders clean), the render surface derived from the spec's identifiers, stopped at swarm.rs and named the wiring lines and the block to delete; reported where the sb-7 literals actually live. Charter gap: none.
- `swarm-surgeon` VA-142 + VA-144 + VA-136 residue (23:3x; 90 tool uses, 368k tokens, 32 min, cargo-free): on charter — found WHY the 28 rewrites were false (a phrase match), derived the hook name from the skeleton's own text before a fallback, chose the steer seam for turn 2 after reading that the core tool-result path is not swarm-side, three commits with trace lines, the residue named. Bloat: 368k tokens is the largest return of the day — three items across five files was one too many; keep briefs to two items when they touch swarm.rs.
- `bench-scorer` VA-147 (23:2x; 8 tool uses, 75k tokens, 4 min): on charter — fields verbatim, present-only proven on two replays, and it CORRECTED the tick-26 fact (13 of the 20 'starved' looks had no carry open: the trigger was silent) instead of printing the brief's number. Charter gap: none.
- `panel-surgeon` VA-145 (23:1x; 27 tool uses, 133k tokens, 8 min, pnpm only): on charter — no new carry field (the landings already rode the existing carry), the pinned list off the ONE join, a deliberate test-expectation update named, clean git status. Charter gap: none.
- `panel-surgeon` VA-143 (23:0x; 43 tool uses, 229k tokens, 15 min, pnpm only): on charter — the closer folded as a close not a miss, landings on both feeds through the ONE join, a clean git status (symlinks under gitignored paths), full suite run, residues named. Charter gap: the typecheck setup — two surgeons in a row improvised symlinks; write the sanctioned worktree typecheck recipe into panel-surgeon.md.
- `swarm-surgeon` VA-139 + VA-136 (23:0x; 42 tool uses, 264k tokens, 20 min, cargo-free): on charter — all five memory sites in one commit with a byte-identity test under benchmark, the retire ratio with a named receipt, the first-writer-wins rule READ and used for the source tag, the batch-first reproduction test, the split law (occupancy.rs), the repair_waves residue reported. Charter gap: none.
- `swarm-surgeon` VA-141 (23:1x; 8 tool uses, 78k tokens, 3 min, cargo-free): on charter — classified each constant against its actual derivation site before marking, verified every other use was under cfg(test), set the baseline from the counter replica, reported the doc residue. Charter gap: none.
- `panel-surgeon` VA-138 (23:0x; 65 tool uses, 316k tokens, 20 min, pnpm only): on charter — steps DERIVED from events (ask/split conditional), durations from event ts, one row per research lane, a probe of the real run.jsonl to prove the numbers, the full vitest suite run, residues reported not widened (→ VA-142). Cost: 316k tokens — the panel's phase machinery spans four files; the next panel brief should hand over the seam map from this return.
- `swarm-surgeon` VA-140 (22:5x; 11 tool uses, 97k tokens, 5 min, cargo-free): on charter — markers on the LINE the gate reads, a ratio derivation for the one shown span with the byte-identity stated, all three baselines set from replicas of the gates' own counters (and it caught that 27 had never been measured), the unmarked sibling receipts reported with a count. Charter gap: none.
- `swarm-surgeon` VA-137 (22:5x; 23 tool uses, 215k tokens, 18 min, cargo-free): on charter — the reach threaded through the meter's constructor and the shadow desk (both consumers), the bytes floor built as a sibling module under the split law with the poll-loop defence kept, the median from the lane's own counters; and it REPLICATED gate 10's counter and found the gate red on staging (six literals landed after the mint) — the kind of unbriefed check the charter asks for. Charter gap: none.
- `swarm-surgeon` VA-127 (22:4x; 17 tool uses, 208k tokens, 20 min, cargo-free, memory.rs only): on charter — read the VA-016 post-mortem first and built the opposite (emitted events only, each emitter line cited; key from the tree; consumer measured via `consumed`; benchmark → off), listed the wiring by symbol, warned that the mod line must land with its consumers. Charter gap: none.
- `scheduler-surgeon` VA-120 (22:3x; 28 tool uses, 156k tokens, 10 min, cargo-free): on charter — the event at BOTH DAG doors (one-door gate honoured), a false doc claim corrected in place, `task_id` kept consistent with every scheduler event, my trace premise corrected (shards are declared, not derived). Charter gap: none.
- `swarm-surgeon` VA-134 + python spawn (22:3x; 38 tool uses, 159k tokens, 9 min, cargo-free): on charter — the mismatch made loud without moving passed (least-impact), the ROOT CAUSE found and named (batch SmokeGate tag, no own source → VA-136) instead of patched over, the collect-only body moved into lang_arms under the split law, confirmed the consumer's rendering off-Python was garbage ('No module named pytest' read as an import failure). Charter gap: none.
- `fix-tracer` VA-126 class 2 (22:2x; 33 tool uses, 134k tokens, 7 min): the reference return — replayed the meter against 618 logged looks to Δ≤0.0001 before trusting itself, corrected a stale premise in my brief (GROWTH no longer summons a look since VA-056), REFUTED candidate (f) with the run's own numbers (a 7.5 s sync vs 16 ms GETs → a false critical) and named the siblings that would drift, distinguished 'verdict-identical' from 'byte-identical' where the desk fields change. Charter gap: none.
- ORCHESTRATOR self-grade 22:1x (Mihai: 'why aren't those started. it just by design, what design?'): three of five 'not started' rows had no real reason — VA-134 needed a READ of r6h's finding (a minute), the python spawn was a reported residue an hour old, VA-127 was parked on a design question I own. Dispatched all three plus the class-2 measurement in one turn. Rule added to the triage checklist: 'by design' is not a status; a row parked on MY decision is CLAIMED by me and decided in the same tick.
- `swarm-surgeon` r6k-staging wiring (21:4x; 32 tool uses, 167k tokens, 9 min, cargo-free): on charter — every site by symbol not stale line, the meter reset beside the recurrence reset, the block placed after the earlier-span block in the look prompt (gate 7's both-spans), a tests module extracted to keep the ratchet, and the two arms it deliberately did NOT feed to the meter named with the parity reason. Charter gap: none.
- `bench-scorer` VA-135 (21:4x; 38 tool uses, 134k tokens, 6 min): on charter — fields from the emit sites on five worktrees, present-only rows proven by two archive replays with zero row diffs, a synthetic stream to exercise every shape, the residue set updated. Charter gap: none.
- `swarm-surgeon` VA-132 (21:4x; 28 tool uses, 158k tokens, 9 min, cargo-free, research_tool.rs only): on charter — reused the shell-row writer, confirmed the desk derives 'acting' from the calls file so no counter was needed, two extra outcome names justified by real `land` results, avoided a new const under the gate-10 ratchet and said so. Charter gap: none.
- `swarm-surgeon` VA-060 / class 5 (21:3x; 28 tool uses, 207k tokens, 18 min, cargo-free): on charter — the language carried on the dispatcher once (no 44-signature thread), three arms routed with the Python path byte-identical, an honest 'said once per door pass, not per run' deviation named, the one unrouted python spawn reported with its line instead of silently widened, the split law honoured (repair_shared_files moved out, swarm.rs 33,508). Charter gap: none.
- `swarm-surgeon` VA-124 (21:3x; 34 tool uses, 257k tokens, 27 min, cargo-free, desk.rs + ladder.rs): on charter — territory-not-names as measured, the no-lookup-between guard that the words-reader's finding demanded, the judge shown both lists verbatim (gate 7), replayed the rules over the REAL logs and corrected my brief's expected fire point (pass 10, not 7) with the reason; stopped at swarm.rs and listed every wiring line. Cost: 257k — desk.rs is a second large file; the seam map from this return goes to the wiring surgeon.
- `swarm-surgeon` VA-128/130 (21:2x; 61 tool uses, 265k tokens, 28 min, cargo-free): on charter — the contract is the simpler one (`section_done` on the tool argument), the ONE matcher reused for the hand and the splice, four tests as briefed, STOPPED at the swarm.rs boundary and named the three lines exactly instead of touching another branch's file; named the two runtime unknowns it cannot test. Cost: 265k tokens — the research prompt assembly has many seams; the next research.rs surgeon gets the seam map from this return instead of rediscovering it.
- `swarm-surgeon` va-instruments VA-115/116/122/125 (21:1x; 25 tool uses, 167k tokens, 13 min, cargo-free, four commits): on charter — one commit per item with its trace line, the budget pair reported in the carry event, the steer-cut arm implemented at the only producer (agent.rs) and SAID so rather than faking it in swarm.rs, hand-checked the select! grammar it could not compile. Brief leak: my file list omitted agent.rs though the fix needed it — a brief for an event should name the PRODUCER, found by grep, not the consumer file.
- `words-reader` VA-124 measurement (21:0x; 30 tool uses, 139k tokens, 10 min): on charter — offsets, timestamps and QUOTES for eleven r6j lists, the r6h identical-sentence pair, r6i's near-verbatim replay with the seam byte; refuted the naive marker-count trigger from the words (N=2/3 fire while boundaries still move) and sized the recoverable minutes honestly (~15 of 46); flagged its own medium-confidence item (r6i replay vs transport re-append) with the evidence that would flip it. Charter gap: none — this is the reference form for a measurement brief.
- `panel-surgeon` VA-129 (21:2x; 47 tool uses, 129k tokens, 9 min, pnpm only): on charter — corrected the brief from the code (the list already rendered; the gap was two siblings) instead of building a duplicate, found and fixed a truth bug at the same line, seven tests including the r6h-verbatim fixture, reported the engine's partition/label disagreement instead of papering over it (→ VA-134). Charter gap: none. Note: it added an untracked ui/sdk/dist symlink for typecheck — the charter should name the sanctioned typecheck setup so worktrees stay clean.
- `swarm-surgeon` VA-126 class 1 (21:1x; 112 tool uses, 351k tokens, 31 min, cargo-free): on charter — one derivation resolved once per dispatcher, the fleet-MAX rule applied after the re-brief, honest split of derived vs left with a reason each, the sink's 7,000 literal found beyond the inventory, the split law honoured (user_notes.rs), byte-identity proven by test, brief-byte deviation named to the char. Brief leak: 351k tokens and 112 tool uses is the cost of 'read each use site WHOLE' across 44 sites — next time hand it the use-site list with line ranges and the classification pre-drafted, and let it verify rather than discover.
- `swarm-surgeon` VA-085 (21:0x; 17 tool uses, 139k tokens, 10 min, cargo-free, shards.rs only): on charter — one classing shared with `merge_holes::dispatch_gaps` instead of a second rule, byte-identical brief for r6h proven from the archive's dossier (the least-impact rule applied unprompted), two tests, residues handed to their owners. Bloat: +231 lines and 139k tokens for a list filter — the merger_brief function is long; the next shards.rs touch should extract the brief's section writers so a change like this is ~40 lines.
- `swarm-surgeon` VA-131 (20:5x; 14 tool uses, 125k tokens, 9 min, cargo-free, research.rs only): on charter — ONE admission rule for both channels as briefed, discovered that ResearchRow folds `raised_for` into `raised` and that ResearchLane carries no files (derived them from the objective) and said so, reason tagged in prior_from, four tests, TRACE YES with the run's real timestamps, residues listed for other owners instead of touching their files. Bloat: +421 lines for an admission rule — the rendering of reasons per row may be more than the brief asked; check at review.
- `scheduler-surgeon` VA-071 (20:5x; 29 tool uses, 138k tokens, 7 min, cargo-free): on charter — hoisted the merger lookup once, scoped `landed` to `merger_of.shards`, put the test in scheduler_mock.rs because the in-file module cannot build `State` (said why), honest TRACE NO for r6h (net), named every unverified compile risk. Bloat: 138k tokens for a ~25-line change — the brief asked it to read shards.rs WHOLE; next time point at the two structs by line instead.
- `panel-surgeon` VA-070+VA-094 (20:4x; 16 tool uses, 86k tokens, 6 min, pnpm only): on charter — read the switch WHOLE and chose the fallthrough marker over a `break` because the comment three lines down recorded why (drill-deeper), fixed a third no-undef of the same mechanism unbriefed and said so, ran typecheck/eslint/the full vitest suite and pasted the lines, reported the render_class_known_bugs gap instead of expanding scope (→ VA-129). Charter gap: none.
- ORCHESTRATOR self-grade 2026-09-02 20:3x (Mihai: 'you made a mistake that you need to account for in our agentic mechanisms'): VA-126 filed OPEN with 'after r6j finishes' while 19 SCHEDULED rows sat as 'next X touch' — the backlog he forbade, dressed in triage vocabulary. Mechanism landed the same turn: gate 10 (live-literal ratchet, 28) + gate 11 (SCHEDULED must say `waits on:`, QUEUED must say `behind:`, 'after the run' fails the build), the 19 rows relabelled and two dispatched. Charter change: the triage vocabulary in CLAUDE.md is the orchestrator's refusing checklist — every OPEN row leaves the turn as CLAIMED, QUEUED behind a named slot, or SCHEDULED behind a named measurement.
- `tick-surgeon` r6j ticks 6–8 (19:41 / 19:54 / 20:02; 6–8 tool uses, 56–82k tokens, 2.5–4 min each): on charter — read the minis' CONTENT (q0 alternatives, q1 framing, q3 pointer-events) and classed them itself before trusting `kind`; found the api/core hold-the-sort pattern from the words and filed VA-128 with the mechanism (per-section message forming), not a cap; named what it could not verify (cites vs request.md, lms ps). Gap: tick 8 recommended 'cut those two lanes' — there is no lane-kill door, only a run kill; the charter now says a lane verdict is `continue` or `kill <run>` with the reason, never a lane cut.
- `swarm-surgeon` r6j-wire follow-up, VA-119 (18:2x; 6 tool uses, 180k tokens, 15 min, cargo): on charter — the exact seam named in the brief (`ResearchLanding::close -> (next_q_index, landed)`, the closure seeds `out_rows` before the remainder), no re-persist/emit, ratchet held at 33,802, a test that pins the seam the model-bound closure calls, all five result lines pasted, TRACE YES with r6i's nine landed rows. Charter gap: none — but the DEFECT it fixed was the surgeon's own from the first pass (rows kept as a count), and the proof chain could not see it because no test asserted a tool-landed row in the fan's RETURN; the review did. Amend: a wiring brief must name the CONSUMER of every new value and ask for a test at the consumer, not at the producer (added to swarm-surgeon's brief checklist).
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
- 2026-09-01 auth worker (general-purpose, leanzero-link/worker, 410ecd910): CLEAN++. Measured the
  external APIs against live docs before coding and caught TWO real-world drifts the brief didn't
  know — Resend Audiences deprecated → Segments (used the current API, kept the env name), Tailscale
  OAuth secrets need a token exchange (dual-mode); audience-sync failure surfaced explicitly never
  blocks auth (loud-absence); 58 mock-fetch tests assert the EXACT documented request bodies;
  named the one unverifiable sub-claim (Resend 409-on-existing) and made the code robust to either.
  Second clean link-backend pass — a THIRD link-backend brief now mints `link-backend` (this + the
  mesh pass are the two).
- 2026-09-01 MINTED link-backend (third clean link-backend brief: mesh 8048946c2 + worker
  410ecd910 + control 923689793). Charter carries the isolation-is-a-type-gate law, measure-first
  with the Resend/Tailscale receipts, loud-absence states, per-pid supervision, and the
  companion-app stateless-contract discipline. First delegation under it: the Link manager.
- 2026-09-01 control service (general-purpose, crates/leanzero-link, 923689793): CLEAN++. Honestly
  modeled the userspace-networking bind (mesh IP isn't a kernel address → MeshBind::
  UserspaceForwarded, not a swallow); two REAL services peered over the real transport in-test
  (fold + delta + flip-offline); mirrored session_event_bus's replay/seq/eviction contract exactly;
  added ?scope=local as an echo-loop guard and said why; named the missing stable node-id as a
  ledger item. Charter gap: none (charter now exists).
- 2026-09-01 link-backend (manager, 8b055f2f1): CLEAN++. Introduced the Mesh/MeshFactory trait
  seam so the connect() state machine tests never spawn a daemon (benchmark-safe) while the real
  live path stays mesh.rs's guarded #[ignore]; audience_sync kept a String not an enum so a future
  worker value never fails an otherwise-good sign-in (loud-absence applied to success too); flagged
  the two integration must-dos (TLS feature, shared-not-per-node control token) AND named its own
  low-confidence literal (the guessed worker URL) as override-me. Every typed error carries the
  worker {error} body — no flattening. Charter gap: none.
- 2026-09-01 link-backend (goosed integration, b5c0ab725): CLEAN++. Delivered real node/session
  state + the leanzeroLink ACP surface; drew the honest line on delta mirroring (the per-session
  buses are in goose-server which depends on goose → can't pull from crates/goose; named the
  dependency-inversion a full P4 needs rather than faking deltas); rebuild-on-identity-change proven
  to only fire while disconnected so it never drops a live mesh; node_token derivation specified
  byte-for-byte for the iOS companion; discovery-fallback keeps health/verify working when Tailscale
  isn't installed while connect() fails loud. Charter gap: none.
- 2026-09-01 panel-surgeon (LeanZero Link tab, 0e7c87c1c): CLEAN++. Kept the two wire dialects
  separate (camelCase LinkState vs snake_case NodeState — the companion contract) instead of
  homogenizing; rendered the not-yet-deployed worker as the HONEST unreachable banner and refused
  to fake-advance; read RequestError.data for the real backend sentence past the SDK's generic
  "Invalid params"; flagged the one deferred truth-invalidation (a connected-view blip retains last
  state rather than flashing loggedOut) with its reasoning rather than hiding it. Capability-gated,
  18 cases. Charter gap: none.
- 2026-09-01 link-backend (P4 delta mirroring, 155480899): CLEAN++. Dependency-inverted the tap
  (goose-server owns it, injects into goose via set_delta_source) so control.rs/wire.rs stayed
  untouched; the ONE live-reply-path edit is provably additive+non-fallible (broadcast send after
  the unchanged bus.publish, no-receiver Err ignored, test-proven); RESERVED SessionDeltaKind::
  ToolCall rather than string-sniff a fake tool_call from an opaque payload (loud-absence over
  misclassification) and flagged it as the one brief-listed kind not produced, with the reason;
  carried origin seq across the seam instead of re-minting where session identity is lost. Charter
  gap: none.
- 2026-09-01 link-backend (cross-node remote execute, e15a4867c): CLEAN++. EXTRACTED spawn_reply_task
  from the reply route so remote-execute reuses the exact agent-spawn core (no loop reimplementation,
  both callers share it); receive-side idle guard is a real 409 (not dead); executor is an Option
  seam that 501s when unwired (loud, never fake-accept); allow_remote_execution default-true with a
  documented observe-only false; stated the security model (RCE among same-account devices) loudly;
  left the ACP method + dispatcher graft to the correct owners rather than half-wiring. Deviations
  each reasoned (executor as start-param not Debug-config-field). Charter gap: none.
- 2026-09-01 link-backend (leanzeroLink/remoteExecute ACP, 179c60288): CLEAN+. Thin well-typed
  wrapper in the exact idiom; every LinkError variant mapped (busy/disabled/unwired/unknown-peer →
  invalid_params with verbatim status text so the UI shows it as-is; transport/internal → internal);
  factored not_connected_to_mesh_err() so the connect-first test asserts the real literal not a
  tautology; chose isolation-safe pure tests over driving the process-wide LINK OnceLock + real
  identity file, and said why. Did not duplicate leanzeroLink/nodes. Charter gap: none.
- 2026-09-01 panel-surgeon (delegation UI + staleness gate, 5c53641c8): CLEAN++. Busy/Offline peers
  shown DISABLED with their state as the reason (honest, not hidden); errors verbatim via
  RequestError.data; staleness gate debounces only the ERROR case (3 fails → reconnecting strip, no
  flash to loggedOut) while auth transitions ride the success path; omitted a stream viewer/deep-link
  with reasons (P4 already mirrors; a remote session has no local route). CRITICAL catch: reported
  that NO goosed binary on the machine contains any leanzeroLink method — the bundle predates the
  feature; end-to-end needs a fresh release-binary. Charter gap: none.
- 2026-09-01 link-backend (remote model mgmt #21, c26940e94): CLEAN++. Optional-nodeId-on-all-16
  design (minimal UI change, no per-op branching) with the 16 handler bodies EXTRACTED into shared
  core_* fns so local and remote share one impl (no copy); peer errors preserve the local error
  CLASS + text verbatim (gate-BLOCK→invalid_params, node-failure→internal); one /v1/swarm/mlx/{op}
  route validated against MlxOp (unknown→404); documented the two honest asymmetries (no idle guard
  on management by design; remote status skips the local inventory-refresh to avoid a circular dep).
  Destructive ops warn-log on the executor. Charter gap: none.
- 2026-09-01 panel-surgeon (node picker for remote model mgmt, 926501c01): CLEAN++. One targetNodeId
  state threaded into all 16 calls via withNode() that OMITS the field when local (byte-identical);
  activeNodeRef guard drops a fetch that resolves after a device switch (truth layer — stale data
  never reads as the new node's); picker hidden unless connected+≥1 peer (local == today); remote
  errors verbatim; destructive ops name the device. Honest live-vs-unit split (remote path needs two
  connected nodes = a deployed worker). Charter gap: none.
- 2026-09-01 swarm-surgeon (#22 graft DESIGN, read-only, no edits): CLEAN++ (exemplary). Verified
  in-tree that the mesh mirrors EVENTS not FILES + the swarm's unit of completion is a LOCAL file
  (verify_owned_files) → proved framing (A) build-fan is a silent-break (missing file as success =
  FALLBACK shape), disqualified it with the mechanism; found the safe real scope (B1: mesh peers as
  read-only supervision devices for the input-embedding advisory calls review_dimension/
  verify_finding — no files cross); mapped every seam as a third instance of the EngineKind pattern
  with a byte-identical None-gated default + a real TRACE; per-invariant + per-gate preservation;
  and — the load-bearing honesty — flagged LOW confidence on REACHABILITY (goose swarm run is a
  standalone CLI with NO mesh handle; LinkManager is a private OnceLock needing connect()),
  recommending an S0 spike BEFORE any S1-S5 code. Read-only respected. This is the model of a
  design-first pass on the sacred engine.
- 2026-09-01 panel-surgeon (#13 nodes-tab weight = routing share, 6f3c8a282): CLEAN++. Wrote into
  the engine's EXACT precedence (device.speed_weight wins, speed_weights map fallback) per node kind,
  leaving concurrency (weight) untouched and honoring the CLI-owns-cloud-devices invariant (cloud
  share via the map, no CLI call); read mirrors the same precedence so a legacy map-only config shows
  the TRUE routed share (no lie); restored the live config to pristine after the write test. Flagged
  the AddNodeDialog label inconsistency (says share, sets concurrency). Charter gap: none.
- 2026-09-01 link-backend (Node self-host port, 31cf0d872): CLEAN++. Added the Node path WITHOUT
  touching the Workers entry/handlers/lib (reused handler tests pass as-is); fs-kv honors TTL
  (>= edge expired) + treats corrupt files as absent+logged (never throws) + base64url keys make
  path traversal UNREPRESENTABLE; verified node globals are strictly typed (deliberate-error probe);
  proved the adapter serves live (health JSON) + zero secrets in the tree via a pattern scan.
  Charter gap: none.
- 2026-09-01 link-backend (bake URL default + optional tag, 4e9d77713): CLEAN++. Baked the live
  self-host URL as DEFAULT_WORKER_BASE_URL (env still wins); made the tag three-state (unset→default,
  empty→untagged, set→value) with the tags field OMITTED when undefined + both bodies test-pinned;
  ran wrangler dry-run beyond the brief to confirm the Cloudflare front door still builds; corrected
  the now-false "always sends tags" doc comment (measure-first). Charter gap: none. NOTE: the mesh
  mint then failed on a SEPARATE bug the orchestrator caught live — Tailscale rejects '.'/'@' in key
  descriptions (c07bc0380 sanitizes the email); join-key now mints untagged end-to-end (verified).
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
- 2026-08-31 12:55 swarm-surgeon (filler door, 331a1a09c): EXEMPLARY. The brief named one seam; the
  per-arm audit found SEVEN and the two worst were unbriefed (prereview riding filler into the SINK's
  prompt; reflect shipping filler into SKILL.md read by FUTURE runs). One shared named door
  (supervised_reply_text in supervision.rs), schedjudge's vocabulary reused, MAX_TURNS_MESSAGE made
  pub via the JUDGE_ENDED_NEEDLE pattern, both traces YES on archived events. Reported HEAD's failing
  now_doc_recipe honestly instead of silently fixing out-of-scope — orchestrator trued the citation
  (test now reads ladder.rs; NOW.md names the symbol; 3/3 green). Charter gap: none.
- 2026-08-31 13:00 tick-surgeon #7: CLEAN+ at 60.4k/7 calls (budget holding with quality intact).
  Closed the core-q4 watch honestly (the nudge WORKED — emission 12m later after due diligence;
  obeyed-count now 3/4 verified); snowball adoption confirmed in a LANDED mini (web-console-q1
  carrying q0's data-brushed/data-state + exponents agreeing with viz-q0 cross-slice); false-raised
  flat a second tick (7/22). Second brief-elision instance named (q2's role-discovery agony) —
  strengthens the slicer-embeds-subsections improvement already queued. Deferred one spec check
  transparently (cached as a watch item, not silently skipped) — exactly the budget discipline
  intended. Charter gap: none.
- 2026-08-31 13:15 tick-surgeon #8: CLEAN+ at 57.7k/11 calls. Closed both watches honestly (console-q2
  EXITED the 0-tools class — 5 ledger reads after the steer, quarantine inferred from words with the
  inference LABELED and the argv-proof gap named for next tick; the deferred hex/table-empty pin
  closed with the lanes' "convention" claim vindicated). Nudge->action now 4-for-5 verified — the
  invariant-4 worry keeps narrowing to reasoning-only calls, run-scale evidence for the r7 desk
  question. Recorded its own 2-call rediscovery cost in state so it never re-pays it. Charter gap:
  none.
- 2026-08-31 13:25 tick-surgeon #9: CLEAN+ at 57.4k/9 calls. Closed its own argv gap (console-q2
  quarantine PROVEN from raw calls.jsonl: 5 reads, fs_delta empty — the probe POSTs are a design for
  the built UI, never executed); both new minis verified against cached pins incl. re-derived dim
  arithmetic; caught the run's next risk class in the words — viz-q2 froze an SSE draw convention at
  ZERO tool calls before reading clarify D1 or any sibling mini ("Version-only bumps: no draw
  (convention...)") — improvement filed with the reader-based fix shape. Charter gap: none.
- 2026-08-31 13:40 tick-surgeon #10: CLEAN+ at 62.7k/11. REFUTED its own prior suspicion with argv
  (viz-q2 DID read clarify + 5 sibling minis before emitting — freeze-before-read didn't happen;
  the improvement note stands only for the silent-convention half). Fan closed 26/26 with the three
  D-decisions closed by lanes that independently AGREE with earlier console verdicts on different
  grounds — cross-lane consistency, not copies. Skipped 3 stale judge logs under budget, said so.
  Named the next tension precisely: the judge's emit-the-DAG-NOW steer may land before the planner
  reaches the five contradictions. Charter gap: none.
- 2026-08-31 14:00 tick-surgeon #11: EXEMPLARY. The reconciliation verdict, with quotes: plan shape
  CLEAN (6 tasks/18 files/0 shared/sink files=[]); durable-counter WON, sync source correct, Health
  correct side — and it refused to claim the approval-path source verdict because the briefs are
  persisted NOWHERE (named the instrument gap with the fix shape; 13 calls, declared the overage on
  the top-priority thread). Review lanes independently converging on the same D2 fix is the
  fan-grounding paying at review. Armed a real checkpoint: plan_patched must go >0 before
  plan_loaded with review-5's patch pending. Charter gap: none. plan.json persistence dispatched.
- 2026-08-31 14:25 swarm-surgeon (plan sidecars, 389283e02): CLEAN+. Both stages persisted (plan.json
  at synthesis with the full 133k-char briefs — verified that IS the brief-carrying representation,
  briefs are not spliced later; plan-loaded.json only when bytes differ, resume arm honest); pure
  sidecar proven against all three .swarm readers with the module doc forbidding a load-bearing
  reader; honest NET label; paid the wiring by moving the resume cluster (43,423→43,208). The vigil
  can now audit briefs directly from r6d onward. Charter gap: none.
- 2026-08-31 14:45 tick-surgeon #12: EXEMPLARY. THE source-literal question CLOSED with the brief's
  verbatim line ("workflow actions use source `approval`" — api-q2's poison did NOT propagate; the
  fan's contradictions net resolved 4-correct 1-unpropagated). Checkpoint watch CLEARED with seq
  numbers (patched 1383 < repaired 1385 < loaded; 8 tasks, sink files=[]). Found the two next
  defects in the words: repair rewrote files[] but left brief PROSE stale (skeleton tripping on it
  live) and review-camera's patch vanished at the merge with no event — both dispatched. Skeleton at
  23m/8 attempts/0 files re-deriving under HELD restreams = the ladder-tracer's predicted shielded
  shape; one-tick escalation window set. Charter gap: none.
- 2026-08-31 15:10 swarm-surgeon (prose rewrite + merge drops, 6c4423e81): CLEAN+. Enumerated the
  whole repair chain (9 stages, one path-renamer); found the merge rule from the join loop
  (first-lane-wins by task id in section order) and traced r6c's exact drop to the HashSet::insert;
  honest split verdict (fix 2 YES at seq 1383; fix 1 a NET because the skeleton's confusion reads its
  OWN pre-repair brief — naming the REAL mechanism, prepend-before-repair, as unbriefed observation
  #1). That mechanism is now dispatched as its own fix. swarm.rs 43,208→43,085. Charter gap: none.
- 2026-08-31 15:25 tick-surgeon #13: EXEMPLARY. The escalation call made from evidence, not the
  window: two-span comparison proved DIFFERENT ground (routes table at -20k, probe-defect fixing at
  tail) — zero re-derivation; the directive rung WORKED (all 5 files, self-probe found 3 real
  defects, now fixing). Called out my instrument's false alarm (16 holds on an advancing lane —
  the ETERNAL-HOLD line can't tell advancing from re-deriving; fixed same-turn: produced-flat
  gating). Flagged __main__.py's delegation-vs-boots-BOTH question honestly as deferred-to-INTEGRATE.
  Charter gap: none.
- 2026-08-31 15:40 swarm-surgeon (skeleton brief post-repair, 044cafed6): EXEMPLARY. Chose regenerate
  over reorder by ANALYSIS — read all five repairs and proved reorder breaks three (skeleton must be
  first claimant; package-owner ties shift; sink-strip home vanishes); the seam sits inside the chain
  so both DAG arms walk it (one door held). Trace honest to the archive: SEEDED not wholly-caused,
  with the confusion's six span offsets enumerated. Paid the ratchet by moving the whole skeleton
  cluster out (swarm.rs 43,085→42,764) with boundary-asserted line-range deletion, no brace script.
  Named the one uncovered seam (replan renames after skeleton dispatch — usually moot, recorded).
  Charter gap: none.
- 2026-08-31 15:55 tick-surgeon #14: EXEMPLARY. The fan read with weight verification (workhorse
  carrying the two heaviest — 5fbc77af4 live-confirmed at last), WATCH-1 refuted from the words
  (ledgerd-core defers to the disk layout: "The project file layout is authoritative" — the stale
  prose did NOT bite), and the run's next finding named precisely: at fan-out+20m ALL FIVE builders
  hold zero files — 46k of formatMoney JS composed in web-console's HEAD, an engine disk-measured
  injection ignored — the compose-in-head class at fleet scale, with the skeleton's proven remedy
  (the exact-file directive rung) proposed as the fix shape. Charter gap: none.
- 2026-08-31 16:15 tick-surgeon #15: EXEMPLARY. First-bytes read with content (db.py's WAL/busy_timeout/
  CHECK constraints — the fan's research visibly IN the code); named the run's sharpest new shape:
  web-console emitting its owned files as FORMED TEXT (70.6k) — progress in the wrong channel, two
  exact-file steers absorbed, the ladder holding it as "advancing". Correctly recommended investigate-
  not-kill (the content is real; the channel is wrong). Also caught judge-web-viz's 46m look gap and
  the provider chat-ism leak (already fixed for r6d at 331a1a09c). File-progress ladder amendment
  dispatched. Charter gap: none.
- 2026-08-31 16:35 swarm-surgeon (wrong-channel ladder, 7728ea236): CLEAN+. Derived directive-pending
  from lane state after PROVING no rung marker exists at the omni seam (the brief's fallback,
  correctly taken); the tracker mirrors tool_calls_at_last_nudge's two mutation sites exactly so a
  fresh attempt structurally cannot trip it; trace YES at the first post-12:07 wanted nudge with the
  delivering-builder and r6a-opener negatives walked; wipe class closed twice over. Honest unbriefed
  finds: the seed carries think-tail not answer-tail for wrong-channel lanes (defensible either way,
  recorded); TWO disagreeing on-disk predicates pre-exist (is_file vs exists — drill-deeper class,
  queue for r6e). swarm.rs 42,764→42,747. Charter gap: none.
- 2026-08-31 16:55 tick-surgeon #16: EXEMPLARY. The standing question answered cleanly (web-console
  SELF-RECOVERED on nudge 3 — index.html+styles.css on disk with the frozen vocabulary verified;
  not steer-immune on this binary; the wrong-channel fix stands on r6d's own merit). Caught the
  sharpest engine defect yet: the judge's disk-measurement ordered a lane to fix a file ANOTHER lane
  owns — the one-owner invariant broken by the supervisor's own words; lane self-saved. Fix
  dispatched (ownership-filtered defect list). web-viz watch escalates: steer 2 + a run.jsonl
  detour brewing. Charter gap: none.
- 2026-08-31 17:15 swarm-surgeon (ownership-filtered defects, ad8212f20): CLEAN+. Found the real seam
  (verify_owned_files' HTML arm feeding both the look prompt AND delivery_defect_steer), derived
  owner+state from the ledger/dispatch tables (no hardcoding), proved r6c carried exactly one
  cross-owned line (the steer fired once), preserved every non-judge consumer byte-identical. Named
  three unbriefed siblings honestly: the completion event/watchdog/verify sweep still render the raw
  line (r6e queue); web_refs has the same hazard one level down; and web-console's look 4 is 3h out
  with NO verdict — the A2 silent class on a judge lane, handed to the tick-surgeon. Charter gap: none.
- 2026-08-31 17:45 tick-surgeon #17: EXEMPLARY. Both watches closed with primary data: the 3h silent
  look was already resolved (judge_look_abandoned 13:42 — my watch carried stale data; no orphan,
  slot released), and the web-viz investigate returned NOT-a-loop/NOT-defiance with the mechanism
  (a research mini answered with a POINTER into run.jsonl instead of extracted text — the lane is
  ledger-driven through a detour both steers predate). Delivered reads first-rate: app.js GUARDS the
  missing viz.js ("even if viz.js is missing, the page won't throw" — pitfall-lesson-19 behavior in
  the built code); sync.py carries the researched cursor semantics verbatim. New r6e item: judge ok
  with EMPTY next while the owned file is absent. Charter gap: none.
- 2026-08-31 18:15 tick-surgeon #18: CLEAN+ at 55.4k/9. Held the trip condition honestly — the
  scavenge is NOT complete (each quoted step settles new ground: the full records format, ordering,
  server-side day), so no restream call yet; but assembled the evidence pack for the HARD TRIP next
  tick and put desk_silent FIRST in the read order (a dead stream needs a different call than a slow
  one — judge-web-viz silent 4->8 polls since 14:14Z). impl.py read is the delivery standard: 1115B
  judged RIGHT-SIZED with the reason (init owns the HTTP frame; bind-before-workers so a down vendor
  can't delay the socket — a researched invariant IN the adapter). Charter gap: none.
- 2026-08-31 18:05 tick-surgeon #19: EXEMPLARY at 56.7k/10. The priority watch answered with the
  primary chain: judge-web-viz proved ALIVE (look6 17:40 broke the silence) and its OWN verdict
  ("DRIFTING|HIGH… write viz.js… ETA=20m") now concurs with our wrong-channel read — formed frozen
  1h44m while think advances real WebGL design, restream evidence pack fully assembled with fields.
  New find with quotes: judges deliberate ETA token placement ("Append '| ETA=8m'? That breaks the
  4-field format. Hmm.") — folded into the same-turn surgeon pass. Charter gap: none.
- 2026-08-31 18:20 swarm-surgeon (3 judge-seam smalls, 24fa10ba6): CLEAN. Traced all three (EDIT 1
  honest NO->NET; 2 and 3 YES), quoted the 19 blank-OK events and the ETA deliberation, read the
  parser before writing the instruction. BRIEF LEAK (mine): my brief gave a wrong evidence path
  (.swarm/run-*.jsonl — events live at the run ROOT as run.jsonl) and wrong verdict case ("OK" —
  recorded lowercase "ok"). Orchestrator briefs citing run.jsonl must state: run root, lowercase
  verdicts.
- 2026-08-31 18:25 tick-surgeon #20: EXEMPLARY at 57.3k/9. Closed the priority watch in both
  directions: refuted MY tick-19 "gold-plating" read with the primary quote (a real wire-format
  bug being fixed pre-completion) and turned the web-viz hold into a measured causation chain
  (steer 17:40 → formed thaw 17:55, first movement in 104m) with an explicit exhaustion rule for
  next tick. Also caught micro-recurrence inside a converging tail ("Let me quickly check around
  lines 440–450" 3×) — the microscopic lens working as ordered. Charter gap: none.
- 2026-08-31 18:40 swarm-surgeon (defect-consumer audiences, dfdf7bded): EXEMPLARY. The audience
  determinations ARE the deliverable and all three carry structural reasons — it refused two
  seductive rewrites by naming the mechanisms that would break (terminal-row-only tasks map would
  lie "pending" about a running owner; a one-branch rewrite breaks the contains() dedup) — MILD
  honored under temptation. Shrank swarm.rs by 54 lines via a visibility-only move. Surfaced the
  real remaining exposure (transcripts.rs raw lines republished into dispatch prompts) instead of
  half-fixing it. Charter gap: none.
- 2026-08-31 19:00 tick-surgeon #21: EXEMPLARY at 68.8k/9 — the best return yet. Overturned its own
  tick-20 causation claim with the primary record (judge_drift_held: no steer was EVER delivered;
  the thaw was self-recovery), reconstructed the full five-hold sequence with the arm-reset
  mechanism (read-only calls reset act), and pre-named the (iii) trigger in engine-mechanism terms
  (restream fires only on looping recur=0.0076; 7728ea236 not aboard; action-counting shields
  read-only lanes). This is gate 7+8 practiced in one report. Charter gap: none.
- 2026-08-31 19:30 tick-surgeon #22: EXEMPLARY at 69k/14. Closed the decision read with a finding
  nobody predicted (the interleaved-ok disarm — a THIRD arm-reset path, quoted against the
  held-detail's own promise), kept the lane honestly alive (+27k NEW ground, recur 0.004, no
  re-derivation) instead of pattern-matching to a kill, and produced the sibling burst precedent
  (165k think → 9-edit burst) that makes "converging" testable next tick. Substance reads on both
  replan deliverables with symbols and execution proof. Charter gap: none.
- 2026-08-31 20:00 tick-surgeon #23: EXEMPLARY at 58.6k/12. Ran the falsification test and
  discriminated correctly — frozen calls but the words MOVED from verifying to composing source
  (quoted the draft), so converging holds and the kill stays sheathed; caught the steering ladder
  re-arming (ok reset the streak, drifting re-armed) one look from the build's first delivery; and
  closed two watch items with file-level proof (outbox.py:86 wrapper, impl.py rewrite). Charter
  gap: none.
- 2026-08-31 20:15 fix-tracer (A+B independent trace): EXEMPLARY at 145k/41 — the reference form of
  gate 8's reader. Reconstructed in-flight-by-model from dispatches-minus-completions and caught the
  surgeon's own trace UNDERCOUNTING the pile (4 not 3 — the web-viz worker itself omitted), refuted
  the "mihai 0..1" hedge with the 15m53s ordering, walked both new paths look-by-look with the run's
  confidences (0.85 vs the 0.8 floor), and named the first UNCOVERED sequence for every residual.
  Charter gap: none.
- 2026-08-31 20:30 tick-surgeon #24: EXEMPLARY at 64k/12. Caught the run's first delivered steer AND
  its defeat in the same read, with the smoking-gun quote (the lane citing the one-write system rule
  to override the minimal-version ramp) — the single most actionable finding of the campaign's vigil;
  the engine fix was dispatchable from its words alone. Also kept ledgerd-core honest (self-caught
  pages=0 defect = advancing, not stalling) and correctly re-attributed this half-hour's workhorse
  pile to dep-shape rather than the routing defect. Charter gap: none.
- 2026-08-31 21:00 tick-surgeon #25: EXEMPLARY at 58.5k/8. Tracked the composition positionally
  (-20k span renderPick vs tail rAF loop = end-of-file, burst imminent) instead of just counting
  chars; caught the judge stop-boilerplate leak in look11's NEXT with the exact injected sentence
  (orchestrator verified it already covered at HEAD, 16243); and flagged the ledgerd-core cap-vs-
  completion race BEFORE it happens with the lane's own words ("About 13k tokens remaining").
  Charter gap: none.
- 2026-08-31 21:10 swarm-surgeon (trace follow-ups, aadbe6275): EXEMPLARY. Traced its own edit to a
  NO (the clause never rendered in r6c because no nudge was ever delivered) and shipped it labeled
  NET with the dependency stated — "the two fixes are load-bearing together" — instead of claiming
  the fix. Hoisted a binding 150 lines to avoid a stale manifest read. Shrank swarm.rs again.
  Charter gap: none.
- 2026-08-31 21:25 tick-surgeon #26: EXEMPLARY at 57.6k/7. Moved the falsifier one honest level
  (compose → audit → audit-the-audit) instead of freezing it, caught the delivery promise EVADED
  through the restream branch with the held-detail quoted, and staged the complete run-cannot-end
  record in its state file before it is needed. Numbers-with-teeth: recorded web-viz.log byte size
  (2433B) after noticing tick 25 never had. Charter gap: none.
- 2026-08-31 21:45 swarm-surgeon (promise evasion, f1d76cd17): EXEMPLARY — the walk-first brief
  honored exactly: proved the indefinite hold from the run's own seq numbers BEFORE editing, then
  the smallest rung that closes it (the streak as memory, no new counter), tests pinning ok-cannot-
  disarm and Restart precedence, and an honest boundary statement (the 1-2-read-calls shape is NOT
  covered and needs its own brief — correctly left to the r7 desk). Charter gap: none.
- 2026-08-31 22:00 tick-surgeon #27: EXEMPLARY at 63.6k/9 — the sharpest single read of the vigil.
  Refuted the judge's own "advancing" verdicts with three numbers (meter 141k→156k vs durable
  158,911 by 21:10 — backlog drain, not production), fired the staged record on its named fields,
  and separated the two lanes' fates cleanly (act on web-viz / do-not-touch ledgerd-core, with
  walk2 PROVEN). The meter-lag discovery is an engine defect nobody had seen in nine weeks of
  looking at holds. Charter gap: none.
- 2026-08-31 22:15 panel-surgeon (sessions backend, 802ce86d3): EXEMPLARY. The contract landed
  exactly as briefed plus honest refinements (runId null-until-reconciled instead of a fake id;
  launch never waits on the network; delete path-pinned so operator archives are untouchable BY
  CONSTRUCTION); proved its 1170/1170 green on a clean worktree to separate its work from the
  sibling's in-flight failures — the check-the-property discipline applied to test hygiene.
  Charter gap: none.
- 2026-08-31 22:30 site agent (benchmark state, 215eed1): EXEMPLARY. Refuted its own brief's alarm
  (MAX_CHECKS=90 was a stale local tree — rebase-first caught it), found the join defect that would
  have made the whole feature inert (sb-7.0-rc vs sb-7.0 era key), drilled the baseline cap against
  production data before choosing the policy (fleet rows always ride), and verified the deploy on
  the live URL before reporting. Charter gap: n/a (general-purpose).
- 2026-08-31 22:40 panel-surgeon (view redesign, 50a74f370): EXEMPLARY. Every design ban honored
  (solid chips, ConfirmationModal, no rails/tints/native), the named-absence rule applied where the
  baked fallback used to live, and the honest boundary stated first in its own report: in-app
  verification is UNVERIFIED until a rebuild — exactly the verify-in-the-running-app doctrine,
  self-applied. Charter gap: none.
- 2026-08-31 22:55 swarm-surgeon (meter lag, b8f915841): EXEMPLARY — refuted the brief's own
  hypothesis (no buffer; sampling skew between trigger and verdict, proven with both verdicts
  reading the same 156,267) and placed the fix at the site the mechanism demanded (verdict-site
  stat, where a trigger-site stat would NOT have fired). Found a second instance in the same lane
  unprompted. The best kind of surgeon return: the brief was wrong in the details and the fix is
  right because it said so. Charter gap: none.
- 2026-08-31 22:30 tick-surgeon #28: EXEMPLARY — the return that pays for the whole mechanism. It
  refuted ITS OWN tick-27 recommendation from the primary record (delivery 18:53Z vs verdict 19:00Z
  — the stale-snapshot save), withdrew it formally, and converted the counterfactual into the
  apply-time-re-check design finding our fresh fixes need. Also caught the judge steering on 100%
  false JS defects with the exact line quoted (regex flag misparse + hoisting). Charter gap: none.
- 2026-08-31 23:00 tick-surgeon #29: EXEMPLARY at 68.4k/13. Went beyond reading to OPERATOR
  VERIFICATION (ran node --check itself, checked the index.html script order) — closing the gap its
  own tick-28 caveat named; caught the lane diagnosing our checker bug in its own words; and
  flagged the ledgerd-api ownership smell BEFORE it becomes an edit. Charter gap: none.
- 2026-08-31 23:15 swarm-surgeon (apply-time guard + lexer, de6f0d9bf/5032c44a2): THE EXEMPLAR.
  Refused the brief's guard as theatre with the mechanism (no await between verdict and wipe),
  found the one channel that actually moves during a one-write delivery (forming args), excluded
  the channels a LOOP produces so the guard cannot disarm real restreams, refuted the brief's
  look-15 counterfactual, and replaced "hoisting" with the measured phantom-regex chain (line
  496→654 token jump), instrumented on the delivered file with refusing tests. Charter gap: none.
- 2026-08-31 23:30 tick-surgeon #30: EXEMPLARY at 66.6k/13. Caught the CAPITULATION pattern nobody
  had named (a lane disproving a steer, then appeasing it with dead code instead of completing) and
  the judge's refutation-amnesia behind it, with both quotes; gave the grace tick its precise exit
  condition (calls=19,705); and reported its own instrument's contradiction (since_ms vs mtime)
  instead of picking a side. Charter gap: none.
- 2026-08-31 23:58 tick-surgeon #31: EXEMPLARY at 57.4k/8. Closed the campaign's hardest lane with
  the verdict written honestly against its own prior suspicion ("a kill would have been WRONG"),
  discharged the stub watch via the lane's own re-verification, and armed the sharpest kind of
  next-watch: a named fallback in the APP-UNDER-TEST's planned dispatch, to be read in the landed
  text. Charter gap: none.
- 2026-09-01 00:30 tick-surgeon #32: EXEMPLARY at 66.2k/22. Discharged the empty-200 watch by
  reading the LANDED dispatch text and classifying each arm against the substitution class
  (honest-arm analysis, gate-1 vocabulary applied to the app's own code); caught the silent
  verification downgrade with the honest-handoff nuance intact; and read the sink's first 9 calls
  closely enough to confirm it found the two defects the skipped smoke would have caught.
  Charter gap: none.
- 2026-09-01 01:00 tick-surgeon #33: EXEMPLARY at 60.7k/13. The boot proof done right — the exact
  advertised command quoted, lsof on both ports, and the orphan discipline applied in reverse
  (verified the flagged server as the sink's OWN registered boot and said do-not-kill with the
  lineage). Read the battery's honest-empty SSE arm against the docs instead of calling it a
  failure. Charter gap: none.
- 2026-09-01 01:30 tick-surgeon #34: EXEMPLARY at 68.9k/18. Closed INTEGRATE with per-leg quotes,
  ran the full fix-stack audit against every named field in one pass (severities/sources/wave/
  PURPOSE/ownership), applied the degradation lens and correctly attributed the scary rows to
  BUILD-era consolidation with the empirical cover named, and flagged the phantom-vs-real
  contradiction WITH both sides quoted. The criticals-in-briefs watch is exactly the right next
  read. Charter gap: none.
- 2026-09-01 03:30 tick-surgeon #35 (ON SONNET, reduced budget): EXCELLENT — the farce test done
  properly on the cheaper model (calls-file edit-filter per shard + one conclusion quote each),
  the critical-fell-through finding nobody had predicted, and an honest budget-overrun flag (15
  calls vs 10) justified by the verification rule. The charter survives a model downgrade; note
  for capacity planning: sonnet suffices for structured farce/routing reads when briefed with
  known-facts + exact questions. Charter gap: none.
- 2026-09-01 04:20 tick-surgeon #36 (sonnet): EXCELLENT. Turned "the critical fell through" into the
  exact retiring mechanism (promoted shard retires its FILE while the finding survives), proved the
  absence with a both-rounds grep, caught the drafts.py lost-fix as the same class, and held the
  leak call one tick rather than guessing owners. Sonnet is holding the charter under budget.
  Charter gap: none.
- 2026-09-01 04:40 tick-surgeon #37: EXEMPLARY at 70.7k/16 — the tick that found the FIELD. It
  located findings_assigned[] in the per-shard ledger (invisible on the dispatch event, which is
  why two rounds of unowned criticals went undiagnosed), quoted the engine's own unassigned:2
  admission, and caught the verbatim re-dispatch of a finding already closed NOT REAL with
  evidence. Every one of the owner's two gates now has named-field proof. Charter gap: none.
- 2026-09-01 05:20 swarm-surgeon (repair brief, fabfe962f): EXEMPLARY. Produced the 17-block ordered
  diagnosis Mihai's "the tasks are the problem" asked for, fixed five collisions as ONE order with the
  amendment arm pinned byte-identical, traced two YES and two honest NO, and found the probe is a
  bare bodyless curl whose emitter drops the status/body it holds. Recovered from MY staging sweep
  cleanly. ORCHESTRATOR LESSON: `git commit --only <paths>` while any surgeon is in flight.
- 2026-09-01 05:05 fix-tracer (placement 623ae8eef): EXEMPLARY. Replayed the OLD key against the five
  logged devices (5/5 match) before walking the new one — the divergence point is measured, not
  argued; then dismantled the commit's wall-clock derivation honestly (a floor built from inflated
  walls) and replaced it with a band. Named the aux-door and long-pole residuals precisely.
  Charter gap: none.
- 2026-09-01 05:35 fix-tracer (brief fabfe962f): EXEMPLARY. Replayed all four lanes' calls files
  to the second, found a THIRD firing event the commit missed (webhooks' forced edit), and then
  the finding that matters: the handoff the new brief relies on is never persisted or consumed —
  a dead letter proven from parse_finding_verdicts' tail_chars(300) and the mid-word ledger
  detail. Corrected the commit's quote counts and order from primary data. Charter gap: none.
- 2026-09-01 05:45 bench-scorer (r6c hermetic): EXEMPLARY. Refused to score a run that had not
  finished (engine still in learn/reflect), waited for the harness to release 8850, scored
  hermetically with the run's own seed, quoted the render probe and the screenshots as the product
  check, read the `passed` emit site so the row cannot lie, and fixed two ledger-truth defects it
  found on the way (slot-header merge, artifact-inflated code counts) with replays proving the old
  numbers. Charter gap: none.
- 2026-09-01 05:50 swarm-surgeon (gate A follow-up, afae2eb1b): EXEMPLARY — the standard for a
  refuted-then-redone fix. Reused the tracer's replay as its harness and ran it on the REAL code
  against the archive; derived the render source from the served page's own <script src> list
  (no probe change, no name list); and BOOTED the archived app to replay the gate's bare curl,
  discovering four of eight findings were probe artifacts (JSON 401 envelopes) — turning "the
  lanes refuted the gate" from a suspicion into a measurement. Charter gap: none.
- 2026-09-01 06:20 fix-tracer (gate A re-trace, afae2eb1b): EXEMPLARY. Detached-worktree test run,
  re-execution of the committed functions on raw archive data in the real file order, AND a
  hermetic boot of the archived app with a positive control — three independent instruments
  agreeing. Corrected the surgeon's own count (six artifacts, not four), caught the handoff parser
  extracting a path from a NEGATION, and labeled the drafts.py machinery a net for this run
  honestly. Charter gap: none.
- 2026-09-01 06:40 swarm-surgeon (twelve smalls, three commits): EXEMPLARY. Twelve items, twelve
  one-line traces with honest NET labels, a compiler-proven lever audit table, and the stack-key
  root cause quoted from the spec's own physics prose. Paid every line with extractions (persona.rs,
  web_vocab_note move). Charter gap: none.
- 2026-09-01 07:00 general-purpose (r6d build+install): EXCELLENT with one lesson. Identity proven
  three ways, explicit stamping that pre-empted a `-dirty` sha the untracked .codex/ dir would have
  caused, atomic install without touching the running app, and two build-chain facts recorded in
  the skills. LESSON: it sat ~35 min in step 0 with no compile running until pinged — brief build
  agents to START the long pole first and do the small edits while it compiles.
- 2026-09-01 07:30 tick-surgeon (r6d tick 1): EXEMPLARY at 86k/13. Walked every falsifier to its
  field, caught the instrument's blind spot (AUX row vs judge placement) by reading
  judge_look_dispatched itself, found three new defects with quotes (backticked section matcher,
  empty-options decisions, an answer-dictating research judge) and set a WATCH that is a
  correctness test of a mini's content, not its presence. Charter gap: none.
- 2026-09-01 08:00 tick-surgeon (r6d tick 2): EXEMPLARY at 75.5k/6 — six calls. Closed the
  correctness watch by reading the mini AGAINST the spec lines (not against the judge), caught the
  judge flipping three ways and being wrong at look 4, and found the dispatch-time snowball gap by
  reading a NEW lane's opening words against a mini that landed 33 seconds later. Three engine
  defects with quotes from six calls. Charter gap: none.
- 2026-09-01 08:10 swarm-surgeon (r6e E1–E6, six commits): EXEMPLARY. Every item extracted into a
  named module (spec_surface.rs, opener.rs, lenient_json.rs), consumer walks quoted (E4's "an
  unmatched claim contributes nothing" is the fact that matters for the live run), honest NO/NET
  labels on E2/E3, and an explicit refusal to write a string-presence test for a prompt clause
  ("a tripwire pretending to gate"). Charter gap: none.
- 2026-09-01 08:25 refuter (E3/E4): EXEMPLARY. Answered the one question that decides the live run
  (NO — with the exact mechanism: heads frozen at 04:16:56Z, build brief formed by the same splice
  with no index/pointer), replayed all 53 real claims under both keys for the regression hunt, and
  caught a README overclaim by reading what the event actually carries. Charter gap: none.
- 2026-09-01 09:10 tick-surgeon (r6d tick 3, retry): EXEMPLARY at 79.8k/7. Judged the E7 proof
  honestly ("defect real, wrong-mini did not materialize" — a hedge, quoted) instead of forcing the
  predicted outcome; computed the fan's pace and the dispatch ORDER (viz last → the exposure test
  will likely be pre-empted); and found the turn-budget misread spreading to a third judge and
  heading toward a RESTART — E9's first harmful path, caught before it fired. Charter gap: none.
- 2026-09-01 09:20 refuter (E8/E9): EXEMPLARY. Forward-replayed all 81 judge dispatches (0 mismatches
  against the old policy) to correct the commit's own trace upward (7/7 not 3/7), traced "1/1 used" to
  moim.rs's turn-budget injection into the judge's OWN message, and then read the judge lanes'
  calls files to find what nobody had: 29 of 62 looks ended in a shell call because the judge holds
  the developer toolset — the structural cause behind a prompt net. Charter gap: none.
- 2026-09-01 09:25 tick-surgeon (r6d tick 4): EXEMPLARY at 75k/11. Closed watch A with the exact
  abandoned event (no RESTART — E9's harm not measured, said plainly), verified api-q2 against three
  spec lines to refute the eternal-hold worry a third time, and mined the judge calls files for two
  new mechanisms (shell no-op retries to attempt 7; the desk summoning judge lanes — 59/79 noise).
  Charter gap: none.

### 2026-09-01 — gate 9 measurements (grades)
- `refuter` as VALUE AUDITOR (r5/r6c/r6d step-by-step cost·delivery·consumer): excellent — every verdict quoted with L-numbers; found LEARN's dead `judge_verdict` filter and the 208m unsupervised replan. Second inline brief of this shape (first: the r6c occupancy). A THIRD mints `value-auditor` (charter: per-step table, ranked deletions, missing instruments). Brief leak: none.
- `refuter` as brief-composition measurer: excellent; REFUTED two of the orchestrator's claims (auth never attached; brief size → thinking). Keep the "measure, do not theorize" framing.
- `fix-tracer` on VA-008: excellent — replayed 28 sections through a Python port, refuted a sub-claim of the composition report (web-viz DID get linked brush), gave anchors + test. Charter gap: none.
- `Explore` agenda audit: good; 12 rows with shas. Charter fine for doc sweeps.
- `panel-surgeon` 24k clip (a8dd8974e): good — kept a measured load-bearing fallback (39 pre-08-29 archives without .log) instead of deleting blind; new drift-guard tests. PROCESS FAULT: ran a whole-tree `git stash`/`stash pop` with another surgeon in flight (popped clean, but it swept their files). Charter amended: no whole-tree git operations (stash, checkout ., reset --hard, clean) while any other agent is in flight — `git show HEAD:path` for HEAD comparisons. Unbriefed finds queued as VA-025/026.

- `bench-scorer` pre-fix snapshot (c7616b070 + loop-state 1051470/8b7b92d): excellent — REFUSED to invent a pre-fix tree when the archive had none (provenance-gated), measured the scorer's noise floor on identical bytes, found and fixed the orchestrator's tick.py crash (a filtered replay had hidden it), and found a live-run-killing fallthrough in score_run.sh. Charter gap: none. Brief leak: none. Orchestrator lesson recorded in memory (exit code, not grep).
- `swarm-surgeon` VA-023 skeleton deps (d4f63be0a): excellent — derived rule reusing `decomposition_of` (no second copy), rejected prose as evidence, honest split trace (YES r5 / NO r6c wall), stopped at the swarm.rs boundary and reported the exact hunk instead of crossing it. Charter fine. Seen-not-fixed items folded into 2a (D0, D8, replanner hold).
- `swarm-surgeon` THE FAN CUT (5 commits, 72 min, ~500k tokens): excellent — per-commit gate-8 traces with honest NO on C2 (shipped as a labeled net), test-count parity per file, extracted `research_plan.rs`, absorbed the E7 corrections mid-flight, ignored the red test it was told was not its own. Charter fine; brief leak: my status nudge was behind its commits — check `git log` before nudging. Its five seen-not-fixed items: tick.py (done by orchestrator), panel docs (VA-029), agent-core.md (fixed), resume drift (folded into the tick fix), TS type (left).
- `swarm-surgeon` batch 2b (645600966, 13cae3428, 857eb4ef2; 41 min): excellent — r6c's REAL 28-heading claim lists as the fixture, before/after tables per brief, honest NO traces on R2/R3 (nets), stopped at the file boundary and handed four exact hunks to 2a. Charter fine. Observation worth a rule: the file half of its vocabularies only fires when the opener names files in objectives (`slice_files_unnamed` ×5 on r6c) — the opener prompt already demands it; the prover should check r6e's `slices_opened`.
- `panel-surgeon` batch VA-025/026/029 (88b1352db, 72b70ebc7, 970f4b22f; 40 min, 126 tool uses): excellent — found the REAL drift (`malformed` read off the raw digest, absent from the join), red-on-HEAD rendered proofs for P2/P3, corrected my brief (`batch` is a COUNT, the lane key is `activity_key`), probed the real r6d archive. Charter fine; brief leak: I asserted an event field I had not read — verify field names from an event sample before briefing. Its unfixed observations: covered/kind events invisible (VA-031), realfs flake + lint pre-existing.
- `refuter` on batch 2b: excellent — verified the fixture IS r6c's real claim list (string-identical), reproduced the trace tables, found a doc-test regression the author's gate structurally could not see, and named the next routing gap (§7 never writes /api/drafts → VA-032). Charter gap closed: all surgeon charters now carry the `--no-fail-fast` + Doc-tests rule.
- `works-prover` on the fan cut: excellent — the exact charter case: C1/C2 APPEARS-TO-WORK because the happy path has 0 measured traffic (r6d's opener emitted 35 bare strings → all Unkinded under HEAD); found the cite-only dedup false-positive class and three silent mini-write sites (gate 1); replicated C2's numbers independently; corrected an off-by-one line cite. Fixes routed to 2a D10(5-7); instrument added (tick.py dispositions). Charter fine.
- `words-reader` on the opener prompt: excellent — gate 7 as intended: quoted the model announcing a file check it never made (both runs, 1 tool call each), named the exact prompt lines that license asking, wrote the 13 fact+cite pairs, and delivered a ≤25-line prompt with worked examples from THIS spec plus the schema change that makes the validator refuse a bare design. Also caught the judge residual (OPEN nudged "exactly one tool call — the output tool"). Charter fine.
- `refuter` on 2a's deletions: excellent — per-commit gates rebuilt from their own sources after catching cargo's cross-worktree rlib reuse (relative-path metadata hash collision in one target dir), found the repair shards' lost decisions channel behind a false commit sentence, the red desktop test no Rust gate runs, the ribbon's "Review — skipped", and ~20 doc/instrument residues. Charter gap → all surgeon briefs must include `pnpm test` when a commit touches config/lever plumbing, and a residue grep (`grep -rn <deleted-symbol>` across docs, tick.py, skills) is part of every deletion.
- `panel-surgeon` residue batch (bd5144bfc, 54514687d): excellent — mirrored the prior retirement decision (type keeps the field for round-trip, stated why), "retired is not skipped and not hidden" pinned by four tests, verified a false comment against the engine before fixing it, and flagged the benchmark spawn's env pins as a gate-1 suspect (VA-039). Charter fine.
- `swarm-surgeon` BATCH 2a (16 commits, 2h26m, 359 tool uses): excellent — every commit traced with an honest YES/NO/NET, absorbed eight D10 items and two refuter correction rounds mid-flight, honored the plan_store module law over my suggested source, ran the clean per-crate gate with the Doc-tests line, swarm.rs 39,583 → 37,068. Brief leak: my status nudge assumed unfinished work that was committed — check `git log` first (second time today).
- `refuter` on D11 (resumed after the outage; 3 tool uses to finish): excellent — verified the 0→5,584 trace against r6c's seven dispatches, found the second description tail the block cut misses, and the pre-existing sink-dependency hole (VA-040). Charter fine.
- `refuter` on 2a D4–D10(8) (resumed; finished in 2 tool uses): excellent — per-commit clean gates, an independent 182-look replay that corrected the commit's own number upward and exposed a join artifact, found the forming-stall reset gap (S8) and a D7 resume edge (VA-043), verified the opener HAS a shell. Charter fine.
- `swarm-surgeon` VA-032 (080c430cf, 38ebd2f0b; resumed after the outage): excellent — derived the resource vocabulary from the spec's own routes (no word list), per-brief before/after table, honest NO on R5 (net), stopped at the file boundary and reported the swarm.rs one-liner; found the `POST/GET` method-cell parser gap (VA-044). Charter fine.
- `swarm-surgeon` VA-044 (1e485c844, a7c37f6ba): excellent — found and fixed the ellipsis half of the parser gap the brief did not name (the row would have emitted `/api/drafts...`), enumerated every consumer newly seeing the row, honest NO/YES traces, rejected an over-extension on the trace, and reported 2c's clippy failure. Charter fine.
- `refuter` on 2c S1–S3: excellent — proved the mean+σ property algebraically (a unique max always flags for n≥3), reproduced a UTF-8 panic with rustc, caught the consumer-routing contamination of the split request, and read the fence code to say "prompt-only". One build at the tip as briefed (load). Charter fine.
- `refuter` on the routing commits (VA-032/044): excellent — ran the committed `briefs_from_slices` twice (fixture vs the archive's REAL objectives) via a throwaway cfg(test) diagnostic and found the fixture's paraphrases had silently become load-bearing; independent re-implementation of both path rules over every archived question; clean worktree hygiene. Charter lesson for briefs: name the RUN's real inputs (the brief's first paragraph is the objective), not the fixture's.
- `refuter` on 2c S4 (the merger): outstanding — built the three-shard r6c case and RAN the committed code to show the after-check greening a dangling call, a comment mention and a missing MERGE.md; caught the prompt-frame contradiction (four sites) the author's own commit had named as a class for shards; measured the gate-1 count drift (111→119) against the untouched baseline. This is gate 7/8 working as designed: the merger would have shipped looking right. Charter fine.
- `swarm-surgeon` VA-045 (884203d61): excellent — extracted the real objectives by script into include_str! fixtures, re-measured every rule assignment, kept the filter readings measured via an objective-less block, and corrected two false docstrings; ran clippy with `--tests` because the lib-only form does not lint cfg(test) code (a rule worth carrying). Charter add: `cargo clippy -p goose-cli --tests -- -D warnings` when a commit touches tests.
- `refuter` on 2c S5/S8: outstanding — a pure text walk of `run_wave` found the index-vs-retain corruption the author's own comment admitted and shipped anyway, plus the stale baseline and the hollow `promoted`; replayed `quotes_replay` on r6c's real NOT-REAL text; measured the gate-1 count rise. Gate 7/8 as designed: no test drove `run_wave` with two in-flight shards, so green tests proved nothing — the refuter did. Charter fine.
- `refuter` on 2c S12/S13: excellent — re-ran every measured case that had failed, then found the next layer (multi-line shorthand export, referenced drop not blocking, cargo-never-ran collapse, the regrade window with a timed scratch replay). Gates at tip 796/0, ratchet 117 == 117. Charter fine.
- `swarm-surgeon` BATCH 2c (S0–S14 minus S9-n/a; 2h55m + the outage; 377 tool uses; ~525k tokens): outstanding — implemented the owner's shards+merger design, absorbed five refuter correction rounds mid-flight (two of them refutations of its own commits) with honest per-commit traces, deleted the idle-model judge (1,171 lines) and the idle-fill reviews, swarm.rs 37,068 → 34,930, ratchets tightened to real counts, clean final gate with the Doc-tests line and the desktop tests. Brief leak: my S9 was already done by an earlier commit — check HEAD before sending a one-liner. Charter fine.
- `refuter` on 2c's final four: excellent — byte-compared the re-homed `skeleton_only`, walked every consumer of the deleted judge, verified both ratchets EQUAL their baselines, corrected the brief's own claim ("the 15 s wake is gone" — it is the Q&A poll), and gave a clear SAFE TO LAUNCH. Charter fine.
- `tick-surgeon` r6e tick 1 (17:22; 9 tool uses, 75k tokens): excellent — the fact arm read as WORDS (eight shell calls = the whole spec in order, quoted), found an engine defect from the words vs the event (judge said OK, engine recorded drifting — parser shape), the OK arm dropping NEXT, and a prompt contradiction the opener itself narrated. Under budget. Charter fine.

- `tick-surgeon` r6e tick 2 (17:53; 10 tool uses, 81k tokens): excellent — read the opener's DRAFT across three back-spans (50k/21k/tail) and proved "converging, not cycling" with offsets (the only final_output in 110k chars); verified five cites against request.md line by line; separated the parser mis-record (look 1) from a GENUINE wrong verdict (look 5) by quoting both; labelled every number it could not verify (emit 0 bytes → draft counts). Its WATCH (fan ≤13, not 34) was the right question and the emit answered it (dispatching 16). Charter fine; no amendment.
- `bench-scorer` VA-054 (18:0x; 10 tool uses, 64k tokens): excellent — corrected the brief's own numbers from the primary file (20 shell calls, not 21; command text lives in `summary`, and a line-level grep would over-count via `result_tail`), replayed three archives + a scratchpad copy to prove every branch (genuine 0 keeps the warning; absent prints absent), committed only on rc=0. Charter fine.
- `tick-surgeon` r6e tick 3 (18:25; 6 tool uses, 92k tokens): exemplary — read four minis and classed each with a quote (the external-NEGATIVE class — "no clock-skew window, I grepped the entire doc" — is a real delivery no count would show); caught tick.py's cost row lying by reconciling it against ONE lane's 959 s (VA-055) instead of quoting it; answered the WATCH with the judge's own NEXT texts across looks ("emit now" ×3 never delivered) and priced it (VA-056); flagged what it could not verify (a cite it failed to match, `lms ps` not run). Two actions filed in the right file. Charter fine.
- `bench-scorer` VA-055 (18:3x; 9 tool uses, 80k tokens): excellent — found the real join (research_answered has no activity_key; (slice,q_index) → research_dispatched; lane key (activity_key,batch)), recomputed the live cost independently (102.4 vs the row's 102), used r6d's one-question lanes as the regression oracle (per-lane == per-question), left 10 pre-dirty files alone with --only, and reported an adjacent lying row instead of silently fixing it (→ VA-057). Charter fine.
- `tick-surgeon` r6e tick 4 (18:56; 7 tool uses, 90k tokens): exemplary — settled the HOT question by reading the judge LOG against the event (every recorded drifting reads DRIFTING|HIGH → not a false steer) and proved "no cut frame" from forming_bytes/inflight/.log bytes; read the lane's words AFTER each steer and named the real class ("acknowledged in words, never acted on; the restream re-bought 22.6k chars"); quoted all three decisions with their line-cited reasons and separated a real raised item from filler. Charter fine; one gap — it inferred idle nodes from events instead of `lms ps` and said so.
- `tick-surgeon` r6e tick 5 (19:26; 8 tool uses, 94k tokens): exemplary — proved "not a loop" by locating where the signature table FIRST appears in the stream (byte 56k) before citing the recurrence ratio; proved the decisions reached the briefs by quoting the verdict AND its grounding text; caught tick.py's label contradicting the event field (VA-062) and a wrong-shaped judge steer for a one-shot lane (VA-061); named the pre-repair snapshot trap (`.swarm/plan.json` is PRE-repair — marks unverifiable until plan_loaded). Charter fine.
- `gate-auditor` repair-chain-vs-kinds (19:5x; 13 tool uses, 108k tokens): exemplary — a ranked table with the line quoted per rule and a verdict per KIND, separated "safe by explicit check" from "safe by luck", found two WRONG rules the kill had not exposed (a failed shard cascades the merger; the merger seam's guard is a comment) and named the reader-replaces-instrument case (shard pieces under `.swarm` are invisible to fs_delta and the judge). Four VA rows + a charter addition came straight out of it. Charter fine.
- `swarm-surgeon` VA-063 (20:0x; 32 tool uses, 111k tokens): exemplary — ran both new tests against HEAD's predicate FIRST and quoted the failure with the run's exact shape (`left: [] / right: ["m-a","m-b"]`), replaced an extension test with a positive definition derived from an existing constant (`tree::SNAPSHOT_EXCLUDES`, no new literal), left the sibling seam test untouched and explained why it never saw the defect, and reported an unbriefed observation (frontend-console owned DECISIONS.md beside code, so no real docs-only task existed on r6e). Stayed out of swarm.rs as briefed. Charter fine.
- `fix-tracer` 20d0520a7 (20:1x; 10 tool uses, 69k tokens): exemplary — found the PRIMARY source the event lacked (`.swarm/plan-loaded.json` carries shard_of/merger_of; the event's projection drops them), walked all 16 tasks with the predicate lines quoted, checked the other door and the unarmed walk, ran the tests independently, and named two honest residuals. Charter fine.
- `bench-scorer` VA-049/043 (20:1x; 43 tool uses, 142k tokens): excellent — proved each deleted row's event has zero emission sites (engine grep + archive), made the split rows print the r6e kill signature verbatim on the archive and `merger deps ok (8/8)` on a fixed run, built the VA-043 refusal from r6c's real timestamps with a scratch fixture, and reported four more dead rows instead of silently widening scope (→ VA-068). Over budget on tool uses (43) for two rows — acceptable given the replay proofs, but the charter should say: replay once per archive, not per row.
- `panel-surgeon` VA-048 + split surface (20:2x; 60 tool uses, 234k tokens, 17 min): excellent work, heavy return — every deleted pin carries its reader-count proof, the split events went from "rendered nowhere" to a patch row + dossier/gap/check rows + a planning-fan lane with a 17-case test, and it reported what it could not verify (no running app) with the exact things to look at on r6f. Three unbriefed observations filed (VA-069/070). Budget note for the charter: two tasks in one brief doubled the cost; brief one surface per dispatch.
- `scheduler-surgeon` VA-064 (20:3x; 29 tool uses, 136k tokens): exemplary — checked finding #0 first (TaskSpec already carried the markers), ran both mock tests RED against HEAD before the fix, kept the one-door test's single splice site, wrote an HONEST NO trace (r6e never dispatched a shard, so the branch had no edge to walk) and shipped as a labeled net, and reported three unbriefed observations including one its own change introduced (a stalled README-only shard degrades to Done). Charter fine.
- `refuter` 5f45e8ea0 (20:1x; 8 tool uses, 82k tokens): exemplary — confirmed all three claims with quoted lines, then found the harm the AUTHOR could not see: rule 1 × rule 2 compose into an infinite gap re-send (fresh gap ids per lap defeat the Done filter), and the completion-time silent hole (relax emits nothing; nothing reconciles readmes_missing at merger completion). Proved the two tests are not theater by naming the assertion that flips. Charter fine.
- `scheduler-surgeon` VA-072 (21:2x, resumed after the 429; 3 tool uses, 154k tokens): exemplary — a progress rule on the door's own record (the set of gaps it spliced), not a count; kept the distinction that a FAILED plan shard is still a real gap; RED-first; left another surgeon's dirty file alone. Charter fine.
- `refuter` e444953af (21:5x; 12 tool uses, 80k tokens): exemplary — enumerated the four remaining summon arms and walked the three scenarios to "never looked at", found the stale JUDGE_WAKE comment, confirmed the parser priority with adversarial strings, and gave a SAFE-TO-BUILD verdict with the hole named for documentation rather than blocking the run on it. One tool use over budget, justified. Charter fine.
- `tick-surgeon` r6f tick 1 (21:52; 5 tool uses, 54k tokens): excellent, right-sized — held the light budget, still read the WORDS (the opener designing slice weights against the fat threshold it had read: "exactly 2.0, 'about' gives slack"), correctly classed desk_summon as an evidence arm of the shadow reader rather than a VA-056 violation, and named what it could not verify. Charter fine.
- `tick-surgeon` r6f tick 2 (22:22; 6 tool uses, 54k tokens): right-sized — the VA-073 duty done exactly as chartered (two spans quoted, different activities named, "no forming frame" checked against both sources), and it backed the vendor-docs announcement with the actual curls. No amendment.
- `bench-scorer` VA-074 (22:4x; 7 tool uses, 68k tokens): excellent, right-sized — refused to change what was already correct (counted desk_look/desk_summon/desk_silent itself and matched the printed row, proved the judge zero is a measured zero by naming the three emit sites), added the OPEN PHASE VALUE row with the r6e receipt as a commented constant and no verdict word, deleted only the 0-emission-site row, and ran both replay gates unfiltered. Charter fine; the brief carried one stale count (silent-marks 11 vs 12) which it corrected from the primary file — brief facts should be stamped with their event seq so staleness is visible.
- `tick-surgeon` r6f tick 3 (22:54; 7 tool uses, 74k tokens): excellent evidence — proved the 0-byte frame was GENERATION from telemetry (33.8k tokens, 18.7 tok/s), read the collected call, named two waste mechanisms (fat fact fields → VA-075; a closing turn OPEN never consumes) and said `continue` with a 23:10 WATCH. The orchestrator killed at 22:55:58 on the same facts read as gate 9 (a second buffered emit, 2 nodes idle, no arm can end it) — a disagreement, recorded; the charter should say: a stream the engine requested AFTER a collected final_output on a planner lane is a finding NOW, not a WATCH.
- goals audit (general-purpose, 23:0x; 13 tool uses, 125k tokens): exemplary — read the 71-minute opener's calls and words and accounted for the time (5 min tools / 30 reasoning / 32 writing the plan twice, 14 min building a line map by hand), proposed only cap-free mechanisms and judged each against MILD/general, ranked threats by confidence not size. Third time this "audit the batch against Mihai's goals" work was briefed inline → mint a `goals-auditor` charter next session.
- `refuter` r6f delta (23:0x; 12 tool uses, 92k tokens): excellent — confirmed the worker loop untouched outside the judge block with the exact condition quoted, traced out-of-moves to its one `may_terminate` site, byte-compared the summon arms across engines. Charter fine.
- `works-prover` split + repair (23:0x; 14 tool uses, 83k tokens): exemplary — ran the 24 unit tests, then named the exact seam with ZERO evidence (swarm.rs 27493/27590/27596), found the README-only-Done silence and two silent fallbacks, and gave the vigil three first-failure fields. Charter fine.
- `fix-tracer` r6f kill mechanism (23:1x; 12 tool uses, 97k tokens): exemplary — refuted the orchestrator's regression hypothesis with a byte-identical core check, found the real mechanism in goose-core (proactive compaction after the collected final_output; token counts from telemetry on both runs), named the smallest fix and the invisibility (no SystemNotification arm). This is why a kill needs an independent tracer before the fix is written. Charter fine.
- `swarm-surgeon` S VA-080 (23:4x; 57 tool uses, 167k tokens, 22 min): excellent result, heavy — three commits each with a test and an honest NET trace, the ratchet tightened; but 57 tool uses for three small fallbacks says the proof chain was re-run per item on a 35k-line crate; the charter should say: fmt/test/clippy ONCE after the last item, with per-item commits made from the same green tree.
- `fix-tracer` 4c86436e9 (23:3x; 8 tool uses, 74k tokens): exemplary — walked both runs' token values through the guard, proved the collected-flag is set in-iteration from the tool's own result text, ran the steer tests, named the assertion that flips, and flagged the one inferred number (128k) as inferred. Charter fine.
- `swarm-surgeon` O VA-077/075/078 (23:4x; 52 tool uses, 189k tokens, 30 min): excellent substance — ranges tested to tile the spec and reach EOF, the fact refuser placed in the schema with the WHY, exemplars read back from the persisted file; it STATED the cost of its own VA-075 design ("a refused fact re-emits the whole plan") — that honesty is what the refuter is now weighing. Heavy for three prompt-level rows (the once-per-tree proof rule landed mid-flight; hold it to that next time).
- `refuter` opener trio (23:4x; 7 tool uses, 93k tokens): exemplary — replicated the section ranges against the real spec with line numbers (first/middle/nested/last-to-EOF), walked the schema refusal end to end to the "asking once more" re-ask, and MEASURED the blocker on r6e's verified facts (20/21 over 200 chars) instead of arguing shape; corrected fix named. Also noted macOS has no `timeout` (its first two test runs never executed — it said so). Charter fine.
- `swarm-surgeon` C VA-076/081/067/079/066/065 (23:5x; 92 tool uses, 292k tokens, 50 min): the biggest return of the day and a good one — six per-row commits each with a test, honest traces (one YES, five NETs labelled as such), and it paid for its additions by EXTRACTING code out of swarm.rs (34,905 → 34,564) instead of touching the ratchet upward. Heavy: the once-per-tree proof rule landed mid-flight; six rows in one brief was my choice, not its — next time two dispatches of three.
- `refuter` C's batch (00:0x; 9 tool uses, 86k tokens): exemplary — verified the macro arm placement against the surviving `_ => {}`, found the per-dispatch `shard_by_task` insert that covers gap shards, checked FILLED/SENT_OUT read the same constants the writer uses, named the one false-positive shape (a FILLED entry naming a symbol rather than the shard) as an event-only cost, counted moved test asserts. Charter fine.
- research: split (general-purpose, 22 tools, 123k) · repair/3D (general-purpose, 21 tools, 129k) · phase anatomy (bench-scorer, 10 tools, 113k) — all three exemplary: primary sources quoted (two fabricated fetch summaries discarded by the split researcher), archive ground truth with line numbers (the 3D field's single-line deaths; 70 min without a byte), the anatomy with method stated and non-derivables named. Fourth inline research brief of the day → mint `research-reader` (web + archive, sources with URLs, mechanisms ranked by confidence, no caps) and `phase-anatomist` (bench-scorer variant) charters next session.
- `swarm-surgeon` W2 shard verification (00:4x; 91 tool uses, 347k tokens, 38 min, worktree, no cargo): excellent — a new module with per-language globals as data, tri-state checks, GAPS wired into the merger brief, the free_hosts derivation with a compile-breaking handoff TODO, an extraction that took swarm.rs down 135 lines, an honest NET trace and named compile-risk spots for the merge. Heavy in tool uses without cargo to blame: the charter's once-per-tree rule needs a sibling — read a file region once, not per edit.
- `swarm-surgeon` W1 assembly (00:5x; 135 tool uses, 424k tokens, 42 min, worktree, no cargo): the substance is exactly the design — deterministic assembly with a glue-only merger, backed PROVIDES, WRITES, leak/predictable-gap instruments, a linear-partition sizing — each commit with READ-WHOLE and a trace, plus three residue findings incl. a ratchet coverage hole its own new directory opened. 135 tool uses is the most of the day: without cargo it should be far fewer; the charter gets "read a region once; batch edits per file". 
- `swarm-surgeon` W3 repair v2 (01:0x; 167 tool uses, 524k tokens, 43 min, worktree, no cargo): the deepest single change of the day — FindingCheck provenance, repro detection from the archives' real row shapes, per-check TreeGrade promotion, the VA-006 partition — with YES traces on both archived runs and one honest NO. One commit instead of four was argued correctly (shared hunks → non-compiling intermediates). 167 tool uses is the day's record; the reading-budget rule now in the charter is aimed at exactly this.
- `swarm-surgeon` W4 cite-only facts (01:2x; 25 tool uses, 200k tokens, 17 min, worktree off r6h-staging, no cargo): exemplary AND on budget (the reading-budget rule's first dispatch) — schema, rule, renderer with loud unrenderable arm, the old event deleted with its commit named, one deliberate deviation stated with its reason (verbatim lines, no sentence heuristic). Charter fine.
- refuters on r6h-staging (03:0x): lens 1 (split path, 13 tools, 130k) — EXEMPLARY: walked the definition rule on r6g's REAL pieces and found the blocker (state/constants not definitions → the brief orders duplicates); lens 2 (repair, 11 tools, 91k) — excellent, two labelled nets (0-happy-path promotion arm; key embeds the source label); lens 3 (one door/ratchets/opener, 12 tools, 83k) — excellent, found the double-section brief bloat and corrected the OPEN-duration expectation. The "test the real thing" rule paid three times in one hour.
- `swarm-surgeon` clippy pass (7 tools, 80k): exemplary, on budget. `swarm-surgeon` VA-096 (15 tools, 110k): exemplary, trace counted on the archive (77/80). `swarm-surgeon` VA-097/098 (69 tools, 199k): the right fix — one rule, real fixtures, five old tests re-pinned honestly — but 2× the budget; it said so.
- `bench-scorer` VA-099 (03:4x; 12 tool uses, 109k tokens): exemplary and on budget — every new event read at its emission site, one synthetic replay for shapes + the r6g archive + the live run, the sweep.py string rewritten from the code it describes, and an older history doc left alone deliberately. Charter fine.
- `swarm-surgeon` VA-089/100 (05:0x; 61 tool uses, 379k tokens, 32 min, worktree, no cargo): the substance is the decision — opener slices only, one lane per slice deriving its own questions, spec_fact_* deleted with research_plan.rs, swarm.rs shrunk 34 lines with the ratchet tightened — and it named its one deviation (VA-100's top-level gate would blind IIFE modules; fixed the real `=>` defect instead) with the fixture. Over budget again (48 self-counted vs 61 measured, brief said 45) — the charter's reading-budget rule holds the line at ~45; the self-count undercounting by 13 is new: the charter gets 'report the harness count, not your tally'.
- `refuter` VA-089/100 branch (05:2x; 16 tool uses, 132k tokens, 7 min after a 429 restart): EXEMPLARY — read-only it found the E0063 the no-cargo surgeon could not see (four fields in a five-field literal, twice), walked one answer through the whole consumer chain to the task description, counted the fallback baseline itself, classified five `rhs_is_function` cases with the rule quoted, and separated blocker from residue cleanly. This is what the post-worktree refute step is for; charter fine.
- `tick-surgeon` r6h BUILD tick 1 (05:25; 10 tool uses, 88k tokens, 6 min): on budget and on charter — three lanes quoted at the word level (the skeleton's third unfulfilled 'let me write', camera's coast math, data-stream's ds_-prefix collision awareness), checkpoints proven from named fields, PHASE VALUE graded PROVISIONAL with the receipts it lacked named, and the one thing it could not verify (the dispatch-time 59k brief is not stored) said plainly. Filed improvements, not actions, and said why (below the action bar until tick 2). Charter fine; the 'assembled brief not stored' gap is an instrument finding for tick.py/engine (VA candidate if it recurs).
- `tick-surgeon` r6h BUILD tick 2 (06:00; 9 tool uses, 77k tokens, 4 min): the tick the charter was written for — it measured what the 90k chars ARE (49% fenced code bodies, counted), quoted three spans per lane at the asked offsets, found the dep-source truncation from the lane's own words plus its recovery read, and turned both into ACTIONS with mechanism and fix shape, no caps. Two slips: it reported its note/action files under loop-state (they are in the repo — note.sh's paths), and it did not run `lms ps`. Charter: add 'name the files note.sh actually wrote (repo paths)'.
- `swarm-surgeon` VA-103 (06:2x; 17 tool uses, 135k tokens, 15 min, worktree, no cargo): exemplary — found the two literals AND their origin commits, measured the real byte offsets on the live tree (corrected the brief's '~4,200' to 5,128 and my `context_slice_len` misattribution to scheduler.rs:1246), extracted an 83-line loop into a tested module with r6h's case as a fixture, TRACE YES with the budget arithmetic, and named what it left (literals, tick.py row, a duplicate). On budget. Charter fine.
- `swarm-surgeon` VA-102 (06:3x; 25 tool uses, 193k tokens, 15 min, worktree, no cargo): good — read the prompt constants whole and quoted the exact sentence that ordered the README last, derived the first file from the plan's facts (least-claimed file / shortest signature) rather than prose, moved a body out of swarm.rs, and labelled its trace honestly as an inference about model behaviour. Named three residues it left unbriefed. Charter fine.
- `fix-tracer` VA-103 (06:2x; 25 tool uses, 127k tokens, 6 min): exemplary — found the real iteration order (sorted manifest, not plan order), replayed the OLD cut on the live file to the byte (3,481 at line 97, matching the commit), walked all five files with budget arithmetic, caught the `--db-dir` false-positive labelling and the unbounded cap-exempt carry, checked the sed range for off-by-one, named the event sink function, and — the part that matters — measured the OUTCOME delta honestly (~1.5 lane-minutes) rather than rubber-stamping the surgeon's YES. Charter fine.
- `refuter` VA-102 (06:4x; 22 tool uses, 151k tokens, 9 min): exemplary — rendered the new paragraph for r6h's real shard from plan-loaded.json and read it as the model would ('signatures exist ONLY in the interface below'), refuted the smallest-piece proxy with the two real picks (a getter vs the GL init) and named the derivation the code dropped (split_sized.weights), listed every ordering instruction in the order the model reads them and found the system-rule contradiction, and caught a false docstring in a test. Charter fine.
- `swarm-surgeon` VA-103 follow-up (06:5x; 3 tool uses, 174k tokens, 7 min): exactly the correction asked for, with the fixtures verified by python before being pinned and the delta stated honestly in the module doc. The 174k tokens on 3 tool uses is the resumed context, not waste.
- `tick-surgeon` r6h BUILD tick 3 (06:27; 10 tool uses, 94k tokens, 6 min): the retype PROVEN at line level (three passes with line numbers and the two diffs that separate them), the cross-slice duplicate found from the brief's ANSWERS block (0 hits for the webhooks answers) and the sibling brief's own claim, a new ACTION with mechanism and no re-filing, `lms ps` run, repo paths named. Charter fine. One lead it could not close (the vendor-docs excerpt 'sections 1-6 (truncated)') is handed to a fallback-hunter.
- `swarm-surgeon` VA-102 follow-up (06:4x; 9 tool uses, 229k tokens, 9 min): all five corrections landed as asked, the proxy deleted rather than patched, the derivation (cluster weights) carried through the struct instead of re-guessed, real fixture. Named what it eyeballed (struct-update moves). Charter fine.
- `fallback-hunter` VA-105 (06:4x; 15 tool uses, 94k tokens, 5 min): exemplary — found the site the tick-surgeon could not (the closure name hid the cut), its origin commit, measured the LIVE docs to the char and named the sections lost, read what the block header CLAIMS to the model, found the rule that steers workers away from recovery, checked the research lanes' actual fetches, and swept four more silent arms on the same surface with file:line and a GUILTY/ratchet-class verdict each. Two designs with the missing structured link named (ResearchRow.cite carries the question's cite, never the answer's docs section). Charter fine.
- `bench-scorer` SHARDS VERIFIED row (06:5x; 26 tool uses, 103k tokens, 7 min): on charter — additive only, replayed HEAD vs new on the live run, the r6g archive and a pre-shards control (byte-identical after normalising a pre-existing age string), the silence case printed loud ('done, NO verify object (verifier did not run)'), r6i event names pre-registered so the next engine does not trip UNKNOWN EVENTS. Committed `--only tick.py`. Charter fine.
- `swarm-surgeon` VA-104 (+ kind fix) (06:5x; 40 tool uses, 238k tokens, 21 min, worktree, no cargo): strong — the mechanism quoted, the fix placed after synthesis where the plan's files exist and at BOTH doors, no phrase list (whole-path match), the two unbuilt arms justified by a measurement on the real minis (0 hits), the block rendered verbatim for the motivating task, and the kind fix done without breaking the rollup's discriminator. At the top of the budget (40 of ~45). Charter fine.
- `swarm-surgeon` VA-105 (07:0x; 34 tool uses, 192k tokens, 18 min, worktree, no cargo): strong — extracted the whole probe cluster with its tests, made the docs page whole with the design call argued (no derived ceiling; no zero-traffic cut arm), cut bodies at a JSON object boundary with a marker that carries facts, named the failed fetch with the vendor's own body, rewrote the header to say what the block IS, decremented the fallback ratchet honestly, pinned the live page as fixture, and listed seven residues. Budget respected. Charter fine.
- `refuter` VA-104 (07:1x; 29 tool uses, 163k tokens, 10 min): exemplary — ported the matcher to Python and re-ran it over the REAL minis and plan, reproduced the seven routings and then showed three came from a deleted mechanism, found the ordering defect (routing before the path repairs) by reading the rename repair's scope, caught a fragment routed on a bare basename, checked every reader of the renamed `kind` field, and corrected two claims in the commit message with numbers. Charter fine.
- `refuter` VA-105 (07:1x; 27 tool uses, 135k tokens, 7 min): exemplary — resolved the visibility chain through a child-of-ancestor import, found the serde `preserve_order` dependency by reading cargo fingerprints, applied `excerpt_body` by hand to the two LIVE bodies with the numbers, walked the real 400 into the new event, and caught a baked benchmark phrase in the header (NO HARD CODING). Charter fine.
- `tick-surgeon` r6h BUILD tick 4 (07:02; 12 tool uses, 88k tokens, 7 min): on charter — the new lane read at the word level ('I can't know exactly' with the sibling README on disk), the merge mismatches predicted from the two READMEs' ASSUMES vs the pieces' actual names (vizGL/gl, uBrush/uBrushActive, ensureBrushFlag never called), the self-invented context pressure measured against telemetry, a NEW action with mechanism, no re-filing, real clock. Charter fine; its 'improvement' on measured usage is promoted to VA-107 by the orchestrator (an improvement that needs a surgeon is an action).
- `swarm-surgeon` VA-105 follow-up (07:1x; 4 tool uses, 174k tokens, 3 min): exactly the four corrections, the hardcoded phrase replaced by a derived fact (`json_row_array_key`), the tautology pinned to numbers verified with python. Resumed context, not waste.
- `swarm-surgeon` VA-104 follow-up (07:2x; 6 tool uses, 260k tokens, 8 min): all five corrections, the pre/post-repair path handled by hoisting the ONE constant the repair already uses instead of a second rule, and an honest note that the new floor would not have cut r6h's exact fragment (moot on the shipping engine). Resumed context, not waste.
- `panel-surgeon` VA-101 (07:2x; 28 tool uses, 183k tokens, 21 min, worktree, typecheck+tests run): on charter — every consumer line named with what it renders now (quoted), dead cases deleted not stubbed, the legacy shape kept explicitly labeled for archives, the gate run and its result quoted, two unbriefed residues named. Over the ~20 budget for a two-file change; the node_modules symlink trick belongs in the charter as the way to run the gate in a worktree.
- `swarm-surgeon` VA-106 (07:2x; 28 tool uses, 215k tokens, 18 min, worktree, no cargo): strong — the seam found and stated, the source of truth the ledger row + disk (not a new registry), the ASSUMES filtered to the receiver's cluster names, pending siblings named with status, the happens-before proven from the code path not the timestamps alone, ratchet PAID by moving VA-103's events into their module. Residues named. Charter fine.
- `swarm-surgeon` VA-108 (08:5x; 5 tool uses after resume, 256k tokens, 6 min): took the course correction (extend the existing emitter, one event name), an identifier rule that explains how a bare `gl` qualifies without a phrase list, a nearest rule with its arms named, the false mismatch killed at its source, the other false-duplicate half correctly deferred with the reason (pinned tests on the ONE definition rule). Charter fine.
- `swarm-surgeon` VA-109 (08:5x; 10 tool uses after resume, 208k tokens): exemplary — syntax-only token rule with its negatives named (hex colours at request.md:416), ancestor attribution for unclaimed sections, the block asserted verbatim on the real spec.
- `swarm-surgeon` VA-110 (08:5x; 8 tool uses after resume, 158k tokens): the best kind of return — REFUTED the brief's mechanism (the map was the plan's; the skip was the relative-import arm) and its expected verdict (the `..` import is genuinely wrong; python proved it), built the right thing, labelled pending a net.
- `general-purpose` VA-112 (09:0x; 17 tool uses, 119k tokens): on brief — fetched the live LM Studio shape and pinned it, kept the llama.cpp arm first, made the default arm loud, traced both r6h and r6f. A swarm-surgeon was not needed (providers crate); the general agent did fine with the gates spelled out in the brief.
- `refuter` VA-107 (09:0x; 51 tool uses, 138k tokens, 8 min after resume): exemplary — recomputed the arithmetic from the r6h BINARY's moim.rs, matched telemetry rows to the lane's calls by the second, found the four figures at their byte offsets and separated a look-alike ('0.55·20k = 11k' shader math), proved the window was the default via /v1/models + registry + config + the running app's env, and corrected two nits in the surgeon's trace. Over budget (51 tools) but every tool bought a number.
- `swarm-surgeon` VA-107 (08:3x; 49 tool uses, 217k tokens): changed the diagnosis under reading — the model was TOLD 11k by MOIM — and said so first; that reading produced VA-112. Over budget; the finding paid for it.
- `bench-scorer` VA-101 tick.py half (12:1x; 18 tool uses, 151k tokens, 12 min): on charter — fields quoted from the r6i emit sites, present-only rows, the r6h replay diffed to one added line (and it found r6h had emitted merge_dossier with no row at all), a synthetic stream exercising all 25 rows, `--only tick.py`. Charter fine.
- `swarm-surgeon` proof-chain fixes (12:3x; 19 tool uses, 142k tokens, 26 min, cargo): exemplary — for each of the five failures it said test-or-code with the reason (two were CODE: `path_tokens` trimmed before stripping `./`, `normalize_params` read the `>` of `=>` as a closer), fixed 39 clippy errors clippy had never reached in the unbuilt modules with written boundary proofs instead of blanket allows, deleted a dead field, and pasted the four result lines. Charter fine.
- `refuter` consolidated 1.41.108 review (12:5x; 33 tool uses, 150k tokens, 10 min): the review the batch needed — read the changes as they run TOGETHER (door sequence once per door, split door does not re-route, the two write-first surfaces never meet, one context-limit source), estimated the assembled brief for a real shard, named the worst latent risk with its trigger condition, spot-checked the string_slice proofs on non-ASCII, and confirmed the seam-test edit kept its pin. Charter fine.
- `tick-surgeon` r6i tick 1 (13:31; 10 tool uses, 111k tokens, 7 min): the roster working as designed — it REFUTED the orchestrator's restream verdict from the durable words (composition, not re-derivation) and traced the delivery to the meter predicate at swarm.rs:14547 / ladder.rs:478-499, quoting the judge's own 'This is advancing, not looping'; filed VA-117 with the mechanism; graded OPEN earning with the 16-min waste named; three lanes quoted; read the digest for VA-107/112. Charter fine. Orchestrator lesson: never grade a restream from a 2k window — read the durable span from the steer to the cut.
- `tick-surgeon` r6i tick 2 (14:30; 7 tool uses, 80k tokens, 4 min): the gate-9 read as chartered — cost and projection with numbers, six answers read and CLASSED against the spec lines they restate (2/2/2), the 113k chars located at three offsets with quotes, the mechanism traced to the prompt lines and filed as an action with a no-cap fix shape, an r6f-class watch item named. On budget. Charter fine.
- `scheduler-surgeon` VA-113a (15:5x; 34 tool uses, 147k tokens, 32 min, cargo): strong — one ordering rule derived from the DAG each pass, speed handled by the existing device ranking instead of a second mechanism, no TaskSpec break (weight on the Node, optional in the plan JSON with a derived default), the trace walked with the measured node rates (webhooks 202 min × 11.4/17.2), tests pinning the real DAG shape, and the one CLI line spelled out for the file it does not own. Charter fine.
- `swarm-surgeon` VA-118 (17:2x; 71 tool uses, 253k tokens, 114 min, cargo): the substance is right and the judgement better — it MEASURED the lexical threshold on the real answers, found no cut, and refused to fit one ('a reader-impersonating instrument'); shipped the structural rule; named the unwired tool and the ResearchRow compromise plainly. 71 tools and 114 min is far over budget — the shared cargo lock (two other surgeons) ate much of it; note for the charter: when three surgeons share a target dir, ask each to run cargo ONCE at the end.
- `bench-scorer` r6j tick rows (17:3x; 29 tool uses, 108k tokens, 6 min): on charter — fields from emit sites (including the UNCOMMITTED va117 worktree diff, noted as such with the field-name drift risk), present-only rows, the heaviest-first check as a printed flag, two archive replays diffed, synthetic stream. Charter fine.
- `swarm-surgeon` va117 (17:3x; 67 tool uses, 242k tokens, 124 min, cargo, three fixes): the honest NO on its own ordering trace is the return's value — it computed r6i's wall/idle under the new order with the measured lane minutes and reported that section count is not the driver. Delivery fix clean (reader decides; since-steer span). Over budget on time and tools; two hours of it was the shared cargo lock with two siblings — charter: ONE cargo pass per surgeon when the target dir is shared.
- `bench-scorer` r6j landed-rows pass (18:1x; 23 tool uses, 105k tokens, 6 min): on charter — fields at emit sites, re-verified the two rows it had read from an uncommitted diff (no drift), fixed a nested `if ctx:` that hid the UNANSWERED row, replay run twice so tick-state settled before diffing. Charter fine.
