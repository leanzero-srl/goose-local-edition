# THE JUDGE DESK (r7 candidate) — designed, refuted, PARKED pending r6's qualifying measurement

Status 2026-08-30: SOUND_WITH_AMENDMENTS. **Does not ship in r6.** The refuter's decisive
objection: REFUSED.md G-4's revive-if is a MEASUREMENT taken BEFORE revival — "steers still
failing on planner/reasoning calls DESPITE the re-stream" — and the restream escalation + tail
carry (63ebe140b) only just shipped. **r6 IS the qualifying measurement.** If r6 shows the ladder
(steer → restream-with-tail) still failing on reasoning calls while findings sit undelivered, the
desk builds with every amendment below; if the ladder holds, this file stays parked.

The ten amendments are MANDATORY on build — the silence hole (a quiet socket is invisible to a
bytes-appended desk; keep an interval SUMMON, never a verdict), the cost arithmetic (count
summons not nudges), the BUILD-slot claim race, the char-vs-byte replay cuts, the racy async
delivery-defect measurement (receiver re-checks pending before telling), transition event naming
(judge_source must reach every consumer or the desk gets its own event names), the gate-5 wording
strike (the seconds constants may summon file reads only), receiver-arm placement outside the
omni gate, and the end-state doc/gate moves (two-write-site invariant + abandon rule texts move in
the same commit or the tripwires refuse).

---

## THE DESIGN (as returned by the design fan, verbatim)

# THE JUDGE OUTSIDE THE PHASES — the desk/receiver split

VERBATIM ASK (SWARM-AGENDA.md:2685): "Workers follow phases, judges on the other hand should live outside of this and run constant checks not just observations... steer without wasting." This design is the G-4 revival (REFUSED.md:59-63) with the revive-if measurement built in: judge_source A/B makes "steers still ignored on reasoning calls while findings sit undelivered" directly measurable.

## THE ONE SENTENCE
Split the judge into a READER that lives outside every phase — a standing background desk that tails the durable artifacts and runs deterministic detectors for free, summoning a model only when a detector fires — and a thin in-loop RECEIVER that keeps only what is structurally loop-bound: steer preconditions, the re-stream, the abandon rule, the engine-end, and the fifteen-local resets. Judgment moves out; delivery stays.

## 1. THE DESK (new module: crates/goose-cli/src/commands/swarm/outofphase.rs; swarm.rs adds `pub mod outofphase;` — swarm.rs and swarm/ coexist under the 2018 edition, satisfying the split law)

One background task per run, spawned/aborted on the HeartbeatGuard pattern (swarm.rs:38784 — tokio::spawn loop, AtomicBool stop, abort on Drop). It holds a `LaneWatch` per active lane:
- byte offsets into `<task>.think.log`, `<task>.log`, `<task>.calls.jsonl` (incremental seek-reads, O(appended bytes))
- a REPLAYED RecurrenceMeter. Recon C proves exact replayability: recur.push (swarm.rs:17577-17579) and think.log receive the same chunks at the same site; the 47-char carry makes the shingle sequence chunk-boundary-independent; DefaultHasher is deterministic. Replay must (a) strip attempt-marker lines (:15404-15420 — written to think.log, never fed to recur), (b) cut at re-stream boundaries joined from run.jsonl nudge/restream events using their thinking_chars offsets (recur.reset() at :19032 has no in-band marker), (c) tolerate ≤400ms flush lag (DIGEST_IO_CADENCE :3418).
- look-count + the existing backoff idiom (first-look floor, 6-look cost backoff — the constants at :3373/:3375/:3411/:3569 reused as PROGRESS thresholds, not clocks)

CADENCE IS PROGRESS-BASED. The desk's own wake is the JUDGE_WAKE idiom (:19132-19155: "A CLOCK MAY SUMMON THE JUDGE. IT MAY NEVER CUT THE CALL") — a clock may summon a FILE READ; only bytes-appended-since-last-look crossing the existing floors (OMNI_JUDGE_MIN_CHARS, OMNI_JUDGE_GROWTH_CHARS) makes a lane look-eligible; nothing time-based ever decides a verdict or cuts anything. No new seconds literal decides model work (gate 5 clean).

DETECTORS (all free, every poll): recurrence replay (recurring()), repeat-run from calls.jsonl consecutive-identical rows (degraded per recon B.14 — 2,000-char tail vs 4,000-char clip; acceptable, it only SUMMONS), degenerate answer from .log tail, growth-without-acting (think.log chars vs calls.jsonl rows), verify_owned_files/verify_tree_imports on the tree (the zero-node-second quartet, call sites :18121/:24580), unowned_writes against a now-persisted attempt baseline (see §3).

MODEL CALL — ONLY WHEN SUMMONED. This is how the 46.3% ceiling (SWARM-AGENDA.md:592-601: 211 looks, 222 of 480 node-min, 5.5 calls per intervention) is beaten: the interval-driven majority of those 211 looks becomes free file reads; a model probe fires only on a detector summon, and r0/r1 data (149 looks → 2 nudges; 122 → 6) says summons are a small fraction of looks. When it fires, the node comes from the scheduler's existing idle-claim discipline — THE POOL MECHANISM WE RIDE, quoted: `least_loaded_free_device()` (scheduler.rs:2079, `enabled && in_flight < cfg.weight`, supervision-first) via `pick_judge_target` (scheduler.rs:2121), claim `idle_jobs += 1; devices[i].in_flight += 1`, released by IdleSlotGuard (scheduler.rs:~886-948, Drop-based, F893 no-notify-for-judge anti-spin), guarded by the A-3 ready-work yield: `if !s.ready.is_empty() && !s.has_free_supervision_device() { None }` — "real work outranks every idle-fill claim". BUILD always outranks the desk; at saturation the desk stays deterministic-only or oversubscribes as a queued request exactly as the scheduler judge already may (scheduler.rs:2110 rationale comment). The probe itself is the existing keyless `run_agent(&planner_model, ..., None)` shape (:16867, :16929-16932 — no lane, no observer, not itself judged), with prompt assembly extracted into the module and fed from files per recon what-judge-reads §A-E (every input durable or reconstructible; per-look "since" deltas from the desk's own prior judge_look event).

## 2. THE RECEIVER (thin, in-loop — what recon (2) proved loop-bound stays)

A per-lane mailbox: `Arc<Mutex<Option<DeskVerdict>>>` registered in a run-level `HashMap<activity_key, JudgeMailbox>` when run_agent_in_inner starts a keyed lane, deregistered by a Drop guard. This is the missing wiring recon (4) names — the Agent is a local at swarm.rs:17121, registered nowhere; we register the MAILBOX, not the agent, so the desk never touches stream state. The worker loop's trigger site (:17768-17776) gains one arm: mailbox non-empty. Everything downstream is the EXISTING verdict-intake code, unchanged in doctrine: streak corroboration (:18594-18604), drift-hold (:18690-18711), nudge_delivery with LIVE pending.is_empty() (:3487, :18752 — orphan-safety never leaves the loop), steer_superseding (:18928), the restream arm with its fifteen-local reset (:18950-19037), judge_out_of_moves/engine-end (:18871-18912). The abandon rule survives structurally: a verdict deposited after the stream ends is never consumed — the loop has returned; the desk sees the terminal digest (phase "done", :19205) and emits judge_look_abandoned itself. Each DeskVerdict carries the stream epoch it judged (attempt + restream count + thinking_chars watermark, all in run.jsonl); the receiver drops an epoch-mismatched verdict as abandoned rather than steering a fresh stream about a dead one. MILD throughout: the desk never uses apply_judge_outcome's abort channel (scheduler.rs:2577), never kills, never refuses — it measures, summons, and deposits; the receiver nudges exactly as today.

END STATE (post-transition): the in-loop probe race is DELETED — no select! against run_agent (:18312-18402), no judging label, and digest Site B (:18337-18390) collapses into Site A, retiring the two-write-site invariant by construction (one site is trivially correct where two had to be kept token-identical). The deferred_events freeze the ask names is already gone at HEAD (b0dd68eac); this removes the mechanism that made it possible.

## 3. THE THREE DURABILITY GAPS (recon F), closed cheaply
- fs_before (the one input LOST outright): persist the attempt-start tree file list to `.swarm/activity/<key>.fsbase.jsonl` at the snapshot site (:17390-17393). One small write per attempt.
- wants_structured_reply: one field added to build_worker_digest (:15766).
- in-flight pending rows: the digest's inflight array at ≤400ms staleness is sufficient for the READER (prompt color only); the DELIVERY-side pending check stays live in-loop, so nothing safety-bearing rides stale data.
Fallback-gate compliance: every desk file-read failure emits a named event (`judge_desk_read_failed{lane, artifact, error}`) that tick.py prints — never a quiet skip, no new unwrap_or_default in the run path (ratchet holds).

## 4. EVENTS AND INSTRUMENTS
judge_look_dispatched / judge_look / judge_look_failed / judge_look_abandoned / judge_nudge / judge_restream keep their names and shapes; every judge event gains `judge_source: "inloop"|"outofphase"` for the transition. tick.py and all instruments survive unchanged; the A/B per source is one jq filter.

## 5. R7 STAGING — what ships now vs later
R7 FIRST CUT (shadow desk, zero behavior change to workers):
1. `commands/swarm/outofphase.rs` — desk task, LaneWatch, incremental tails, RecurrenceMeter replay, deterministic detectors. NO model calls in r7.
2. The three durability closures (fsbase persist, digest field, named read-fail events).
3. `judge_source` on all existing judge events (inloop) and on desk events (outofphase).
4. The desk emits SHADOW judge_look events (deterministic verdict + its replayed recur_rate/span) and delivers ONE thing for real, via the mailbox: DELIVERY-DEFECT steers — the verdict-independent, facts-only class the engine already trusts (:18251-18296, "A DEFECT IS A FACT, AND IT SHOULD NOT WAIT FOR AN OPINION") — now firing between looks instead of only at look time. This is the smallest true "constant checks, steer without wasting" delivery.
5. The r7 measurement the shadow buys for free: per-look comparison of desk-replayed recur_rate/span vs the in-loop judge_look's recorded values (:18509-18510) — replay fidelity proven or refuted on a real run before any verdict rides it; plus desk node-cost (should be ~0) vs the 46.3% baseline.
LATER (r8+, only after the r7 fidelity/cost numbers):
6. Model summons on the desk via pick_judge_target claim + prompt extraction; full DeskVerdicts through the mailbox; in-loop probe demoted to receiver-only; probe race + Site B deleted (net-negative LOC in swarm.rs).
7. RESTART-reaches-reasoning-calls (AGENTS.md invariant 4's open wiring) — the receiver consuming a mailbox RESTART verdict IS that wiring; measure on the r3-class failure shape.
8. Unify with the scheduler's per-task judge (pick_judge_target consumer) so there is ONE judge desk, not two judges with two node rules.
Gate-8 note for ship time: the r8 cut-over commit must carry the trace — r1's six-ignored-steers lane walked through the desk path (which detector summons at which byte offset, which verdict the mailbox carries, what the receiver does) — or ship labeled a net.

## LOC ESTIMATE
R7 first cut: ~950-1,200 LOC total. Breakdown: commands/swarm/outofphase.rs ~600-750 (desk loop + LaneWatch offsets ~150, RecurrenceMeter replay incl. marker-strip and restream-cut ~120, deterministic detectors reusing verify_* ~120, shadow judge_look + defect-steer mailbox deposit ~120, events/read-fail plumbing ~90); swarm.rs touchpoints ~150 (mailbox registry + register/deregister Drop guard ~60, receiver mailbox arm in the trigger ~30, fsbase persist at :17390 ~20, digest wants_structured_reply ~5, judge_source on existing events ~35); RecurrenceMeter + verify fns made pub(crate) and re-exported ~20; tests ~180 (replay fidelity against a recorded think.log + restream-boundary cut, mailbox epoch-drop, ready-work-yield respected). R8 cut-over: ~+450-600 in the module (prompt extraction, pick_judge_target claim path, full-verdict receiver) MINUS ~300-400 deleted from swarm.rs (probe race, Site B, judging label) — swarm.rs shrinks, honoring the split law's direction.

## DESIGNER-ADMITTED RISKS
- LOWER CONFIDENCE — RecurrenceMeter replay fidelity: the marker-strip plus restream-cut (boundary joined from run.jsonl thinking_chars offsets, since think.log has no in-band reset marker at :19032) is the subtlest piece; a mis-cut replays old-stream bytes into the new meter and MANUFACTURES recurrence — the exact manufactured-symptom class the fifteen-local reset (:19012-19031, seq 89-91 receipt) exists to prevent. Mitigated by design: r7 is shadow-only and compares replayed rate/span against the in-loop look events on a real run before any verdict rides the replay.
- Stale-verdict delivery: the desk judges an epoch the lane has since left (restream, new attempt, call ended). Mitigation is the epoch stamp + receiver drop-as-abandoned; if the epoch fields are wrong the failure is a steer about a dead stream — MILD (a wasted note, never a kill) but it is the class to test explicitly.
- Double-judging during transition: in 'both' mode the in-loop probe and the desk could each nudge the same lane; the receiver owns the single streak/nudge state so corroboration dedupes, but r7 avoids the question entirely by keeping the desk shadow + defect-steers-only (defects_told already dedupes per defect).
- Mailbox registry lifetime: a leaked entry after a lane panic would let the desk deposit forever; Drop-guard deregistration on the IdleSlotGuard pattern, and the desk drops any lane whose digest goes terminal.
- Desk read cost at scale: per-wake incremental reads are O(appended bytes), but verify_owned_files/tree walks are per-poll fs work; bound by running tree-level checks only on fs mtime change, and measure desk CPU in r7 shadow (it must be ~0 node-min against the 46.3% ceiling).
- Gate exposure: gate 1 — every desk read/parse failure needs its named event (no quiet skips, unwrap_or_default ratchet may not rise); gate 5 — the desk wake clock must remain summon-only (reads), never verdict-deciding; gate 8 — the r8 cut-over needs its trace against r1's ignored-steers lane or ships labeled a net.
- Scope note (blast radius, not priority): r7 touches swarm.rs at five small seams plus one new module; the r8 step is the large one — it deletes the probe race and Site B, the code path two hard-won invariants (abandon rule, two-write-site transcripts) currently live in, so it must not ship without the r7 shadow numbers.


## THE REFUTATION (verdict: SOUND_WITH_AMENDMENTS) — every objection verbatim

### Objection 1
SILENCE HOLE (fatal in the r8 end state unless amended): the desk's look-eligibility is 'bytes-appended-since-last-look crossing OMNI_JUDGE_MIN_CHARS/GROWTH_CHARS' — a silent lane appends nothing and is NEVER summoned, and every listed detector (recurrence, repeat-run, degenerate, growth-without-acting, verify quartet) requires output. Today the in-loop interval look (omni_next_look disjunct at swarm.rs:17771-17776, woken by JUDGE_WAKE :19164) is the ONLY thing that catches a quiet-but-open socket — the exact r0 receipt at :19133-19147 (research call silent, 9 lanes idle 26-40 min) and NOW.md's named A2 risk. r8 deletes that look and replaces it with nothing. AMENDMENT: the desk needs a staleness detector in the self-calibrating quiet-vs-longest-recovered-gap idiom (omni_quiet_secs/omni_longest_gap_secs, :18485-18560 — per-lane, no literal seconds) that SUMMONS on quiet exceeding the lane's own recovered high-water; without it the design revives a paid-for death.

### Objection 2
COST ARITHMETIC DOES NOT CLOSE against the 46.3% ceiling: the design's evidence (149 looks -> 2 nudges; 122 -> 6) counts NUDGES, but the desk's model cost is SUMMONS, and the trigger at swarm.rs:17766-17772 shows trip-triggered looks (recur.recurring() — LATCHED once tripped, span/rate are cumulative; grew_without_acting every 4,000 chars on streams the code itself rates at 2-4k chars/min, :3716 comment; repeat_evidence; degenerate_answer) bypass the interval and map 1:1 onto desk summons at median 49s each. The looks the desk converts to free reads are the INTERVAL ones — already diluted to 300s after 6 looks by the live cost backoff (:17863-17868), i.e. the already-cheap fraction. judge_look events carry NO trigger-reason field (verified :18485-18516), so the 'interval-driven majority of 211 looks' premise cannot be verified from any run's data. AMENDMENT: add trigger_reason to the in-loop judge_look in r7 and take the decomposition on a real run BEFORE any r8 claim that the ceiling is beaten.

### Objection 3
BUILD-NEVER-LOSES-A-SLOT IS NOT PROVABLE as stated: (a) claim-instant race — the A-3 yield (scheduler.rs:4041/4151/4185) guards only at claim time; a task readied one second after the desk claims the last free slot waits behind an UNCAPPED probe (median 49s, max 221s, no clock may cut it); (b) at saturation supervision_device() (scheduler.rs:2113-2125) oversubscribes, which preserves the engine's slot accounting but not throughput — F623 (scheduler.rs:2075-2078) measured concurrent generations degrading each other on one Apple host, so BUILD loses generation speed, not slots; (c) the 1-node fleet is an unresolved either/or in the design's own text: deterministic-only means ZERO model supervision for the whole of BUILD on a single node (a regression vs today's unclaimed in-loop probe, which still runs as an LM Studio queued request), and oversubscribing means degrading the only build node; (d) pick_judge_target (scheduler.rs:2121+) is not a node-picker the desk can ride — it selects ITS OWN target (longest-running Claimed, min_age_secs, marks judge_running 'at most one at a time'); the desk must use supervision_device()+IdleSlotGuard directly and must decide whether the single-judge invariant binds desk summons (if yes, summons serialize fleet-wide; if no, a new contention rule is a new door). AMENDMENT: specify the exact claim path and the saturation/1-node policy; drop the word 'prove'.

### Objection 4
G-4 REVIVAL INVERTS THE REFUSED CONTRACT: REFUSED.md:59-63's revive-if is a MEASUREMENT — 'r3's judge-ON arm shows steering still failing on planner/reasoning calls DESPITE the re-stream... while gate-on-completion findings sit undelivered' — taken BEFORE revival. The design substitutes 'judge_source A/B makes it measurable' (measurement built into the revival) and cites r1 ignored-steer data that PREdates the restream escalation now at HEAD (nudge_delivery :3497 returns restream on Restart; the restream arm :18963-19040 is live). Its own item 7 (RESTART-reaches-reasoning-calls) is sequenced AFTER the desk ships, i.e. the interim fix the refusal named is finished last. Note also AGENTS.md invariant 4 still calls that wiring 'open' while the code shows Restart->restream wired — one of the two is stale and I cannot determine which from the repo alone (cannot-verify objection). AMENDMENT: reconcile AGENTS.md invariant 4 with HEAD, then take the revive-if measurement on the current engine (one judge-ON run, count judge_restream events with no behaviour change on reasoning lanes) before any r8 commitment; r7's shadow-only cut may proceed as it changes no worker behaviour except defect steers.

### Objection 5
REPLAY GAPS BEYOND THE ADMITTED ONES: (a) judge_restream carries abandoned_thinking_chars — a CHAR count per stream — while the cut needs a BYTE offset into a UTF-8 file interleaved with attempt markers (attempt_marker_line :15407); reconstructible only by cumulative char-walking the file, so say so; (b) producer-side write losses skew offsets SILENTLY: append_thinking_transcript failures only emit note_transcript_write_failure (chars counted in thinking_chars but absent from the file), and append_calls_jsonl (:15526-15561) returns silently on open/write error — the design's judge_desk_read_failed covers desk READS only, not missing producer writes, and a mis-cut 'MANUFACTURES recurrence' by the design's own admission; (c) 'Recon C proves exact replayability' is true only for counts/order — RecurrenceMeter's mid/older spans (note_text :3672-3684, per-chunk truncation at >1600 chars) are chunk-boundary-DEPENDENT, so the judge prompt's out-of-tail compare span cannot be reproduced exactly (the desk must substitute its own prior-poll spans); (d) the judge prompt's `ran` rows carry result tails richer than calls.jsonl's 2,000-char result_tail — the degradation is admitted for the repeat DETECTOR but silently also degrades the summoned judge's PROMPT. AMENDMENT: on any transcript_write_failed for a lane the desk invalidates that attempt's replayed meter with a named event (judge_desk_replay_invalid); the r7 shadow comparison mismatch also needs its own named event, not a quiet tolerance (fallback gate).

### Objection 6
R7'S ONE REAL DELIVERY IS THE RACY ONE: in-loop, delivery defects are measured AND told inside `if !owned_defects.is_empty() && pending.is_empty()` (:18253) — measurement is near-synchronous with a quiescent tool state. The desk measures asynchronously against a digest whose pending array is <=400ms stale, so it can py_compile a file mid-write and deposit a FALSE 'MEASUREMENT of the files you own, taken from disk just now' — and defects_told marks it told once, so no retraction ever comes. AMENDMENT: the receiver re-runs verify_owned_files on the named files at delivery time (where pending.is_empty() is already checked live) and drops defects that no longer reproduce; also specify how the desk task gets 'static access to self.defects_told/owned_files_by_task (they must become Arc-cloned into the spawned task).

### Objection 7
TRANSITION EVENT CORRUPTION: emitting shadow desk events under the SAME judge_look name breaks every existing reconciliation — the agenda's cost table pairs judge_look_dispatched/returned/abandoned (the desk emits looks with no dispatched), tick.py counts, and RUN-LEDGER comparability across r0-r6 — unless every consumer is taught the judge_source filter, so 'tick.py and all instruments survive unchanged' is false as claimed; a deterministic verdict inside a judge_look also puts fabricated-verdict rows where model verdicts live. AMENDMENT: r7 desk events get distinct names (desk_look/desk_summon/desk_deposit) and the names unify only at the r8 cut-over commit, in the same commit that re-teaches the instruments.

### Objection 8
GATE-5 WORDING DEFECT: ':3373/:3375... reused as PROGRESS thresholds, not clocks' is wrong — those lines are OMNI_JUDGE_FIRST_LOOK_SECS (45) and OMNI_JUDGE_INTERVAL_SECS (60), literal seconds; they cannot be progress thresholds, and importing them into desk eligibility would be a new seconds-decides-work path. AMENDMENT: strike them; desk eligibility uses only OMNI_JUDGE_MIN_CHARS/OMNI_JUDGE_GROWTH_CHARS plus the amended staleness rhythm, and the desk's poll clock is documented as file-read-summon-only under the :19133 law. Minor anchor drift for the record: HeartbeatGuard is at swarm.rs:38487, not :38784 (pattern exists as claimed); the split-law claim is CONFIRMED — swarm/judge_context.rs already coexists with swarm.rs at HEAD.

### Objection 9
RECEIVER WIRING UNDERSPECIFIED (walked, mostly sound, two holes): the mailbox arm must sit OUTSIDE the omni_judge_on gate and outside the thinking_total floor at :17766-17772, or desk deliveries on a judge-off run or a low-char lane are never consumed; and delivery latency is bounded by JUDGE_WAKE (30s) on a silent lane — acceptable but must be stated. The kept intake code's baselines (produced/actions since 'last look', producing-veto, within_known_rhythm) are loop-locals whose meaning changes when 'look' becomes 'desk deposit' — a LOOPING verdict formed at watermark W consumed after 6k more live chars is corroborated against LIVE state about a PAST claim; the epoch stamp drops only restream/attempt mismatches, not plain growth. Consequence is MILD (wasted steer) per the design's own admission, but the r7 shadow comparison should also log deposit-to-consumption char drift so r8 can bound it.

### Objection 10
END-STATE BLAST RADIUS IS PINNED BY GATES: deleting the probe race and Site B removes the code path the AGENTS.md two-write-site invariant and the abandon rule are written against, and development_gates.rs pins doc text; the r8 commit must move those invariant texts in the same commit or the tripwires refuse — the design admits the blast radius but does not name the doc/gate edits. Also the gate-8 trace obligation the design assigns itself (r1's six-ignored-steers lane walked through the desk path) cannot produce a YES verdict for the steer-ignored class as currently argued: those steers were DELIVERED and ignored by the model (:18746-18760 measured receipt), which the desk does not change — only the mailbox-RESTART restream (item 7) changes that outcome, and it is deliberately sequenced last. The r8 cut-over as staged would therefore ship labeled a NET for the steer-ignored class, not the fix — the design should say so now rather than discover it at commit time.


## Qualifying data from r6d (2026-09-01, ticks 1–2)
- **The judge is the least reliable reader in the loop on research content.** judge-research-q2 flipped across
  four looks on one question (look 1 "yes, note bumps version" → look 3 "bumps payment.version (same int)" →
  look 4 "note write does NOT touch payment.version" — WRONG per request.md:189-192); only the look-2 steer
  was delivered and it happened to be spec-compatible; the lane's mini was CORRECT because it read the spec,
  not the judge. The hold ladder is what kept the wrong look 4 undelivered. Design consequence: a desk that
  DICTATES content is worse than none; a desk that points at spec lines and sibling minis is the only safe
  shape (E6 lands the prompt half; the desk should structurally forbid content assertions on research lanes).
- **Supervision spend with no verdict**: q0's 7 looks = 5 abandoned (call finished while the judge read it),
  1 failed (turn-cap filler — the judge read the WORKER's "1/1 used" line as its own and ended in a tool call),
  1 skipped. tick.py now prints JUDGE LOOK OUTCOMES. The desk should read the durable transcript position
  (b8f915841) and skip a look on a lane whose call ended since the trigger.
- **Placement**: with weight 1 and three busy hosts, least-loaded ties put every q0 look on q0's own node
  (gabee, inflight 1). The desk/aux picker needs "avoid the supervised lane's own model on ties" (E8).
- **Late snowball**: a mini that lands 33 s after a same-topic sibling dispatches is invisible to it (q5 vs q2);
  the desk holds the sibling minis at look time and can deliver them as the steer's ESTABLISHED (E7).
- **Desk axiom (r6d, refuter 2026-09-01): the judge holds NO tools.** The omni-judge probe ran with the full
  developer toolset (write/edit/shell/tree — `read_only: false` for byte-parity) and 29 of 62 completed looks
  ended in a shell call (`echo "no-op: verdict only"`, `true`, …); one look was filler-only and FAILED. A reader
  that can act will act; a desk lane is text-in/verdict-out, and the `<turn-budget>` line moim.rs injects into
  its own message (max_turns=1 → "1/1 used") must be labeled as its own or not injected on judge lanes.
