# r6e — what changed, what was shut down, and why (review copy for Mihai)

Written 2026-09-01 as the changes land; rendered as a review page when the r6e binary is ready. Every row
names its measurement (primary archives r5 / r6c / r6d), its commit, and the first-tick row in tick.py that
proves or refutes it. STATUS: `LANDED <sha>` · `IN FLIGHT <batch>` · `MEASURE ON r6e`.

**What r6e is measured against.** r5: 0.3609 hermetic, 701 min (build 325, fix 144), no research phase,
6k briefs. r6c: 0.1420, 1,101 min (research fan 126, build 608, fix 215), 21k briefs, frontend dead
(`sort=date_desc` → 400 → zero rows). r6d: killed at research 165m (38-question fan, 59% spec-lookups).

## 1. SHUT DOWN — steps deleted because their measured delivery was consumed by nothing (gate 9)

| step | measurement | status |
|---|---|---|
| Research fan as 1 lane per question | r6c 126m; r6d 38 planned ≈ 4h; 13 of 27 questions answerable at a cited `request.md` line, 3 dups, D1 decided 3× | LANDED — C1 `3068ccf90` spec-answerable questions become cited SPEC FACTS in the brief (no lane); C2 `fab90e91b` decisions once + dedup vs landed minis; C3 `8d6a4eb7c` ONE lane per slice (r6d: 6 lanes instead of 38; ≈80 wall-min for what took 165) |
| `max_research_questions` lever | recorded in `levers_resolved`, consumed nowhere — a bound that bounded nothing | LANDED `64738a6ee` (retired_levers echo) |
| REVIEW (LLM round + 3 section lanes) | 0 effective patches in 3 runs: r5 added brush-contract (658-char brief) and the brush ReferenceError shipped anyway; r6c added decisions-doc (387-char brief) NOTHING depended on; 28–52 wall-min, ~140–206 node-min | LANDED `2447d145c` (+ `9eaea07b3` deleted the completion-time pre-review, off in every measured run) — deterministic plan repairs stay |
| Replanner ("bonus" tasks) | r6c: replan-r0 ran 208m UNSUPERVISED, its files imported by 0 modules, 248 node-min; r5 68 node-min, tests unscored; r5's dispatch also held 20m while it ran | IN FLIGHT 2a D2 (splice path + tests deleted; gate 6 test becomes "no splice site") |
| LEARN / persona | `persona.rs` filters an event (`judge_verdict`) no run ever emits → lessons structurally 0; stack key "stack-angular" for a Python+vanilla-JS app; r6d never loaded it | IN FLIGHT 2a D3 |
| Judge looks on BUILD lanes by cadence/growth | judge = 44% (r5) / 42% (r6c) of ALL node-minutes (940 / 1,906), 0 kills, build-lane compliance 2/9, all 182 r6c looks on the node running both hard tasks; output-tool lanes complied 7/7 (kept) | IN FLIGHT 2a D4 — BUILD/REPAIR lanes summon the judge only on measured recurrence or a forming-channel stall; `judge_look` gains node/secs/forming_bytes |
| Blanket skeleton barrier | every task waited 56–64 min for the skeleton with 2 nodes idle; frontend tasks import no entry module | LANDED `d4f63be0a` — dep kept only for tasks owning a file in a package the skeleton creates (r5 trace: INTEGRATE ≈56m earlier; r6c: its critical path is the fat tasks → §3) |

## 2. CHANGED — mechanisms, each with its receipt

| change | receipt | status |
|---|---|---|
| Spec sections route to CONSUMERS + cross-cutting broadcast | r6c product-killer: `Endpoints` (the sort values) went only to ledgerd-api; web-console never saw `-created_at`, built 330m before api.py existed, sent `sort=date_desc` → 400 → zero rows. Splice is owner-only (`research.rs` `splice_claimed_sections`); no budget exists — r5's opener multi-claimed, r6c's partitioned | LANDED `645600966` — "calls into" (advertised routes, claimed parent's children) + cross-cutting broadcast; one helper for brief and research prompt |
| Decisions rendered per slice | one identical 5,582-char block in all 5 r6c briefs (22k duplicate); truncated answer pastes | LANDED `13cae3428` — only decisions naming the slice reach its brief; answers render whole, never a 1,500-char head |
| A brief may not name a file it does not own | r6c web-console: "Ship DECISIONS.md (owned by this slice)" vs `files=[web/*]`; the worker burned its 0% and 80% spans on the contradiction | IN FLIGHT 2a D5 (rewritten + loud `brief_names_unowned_file`) |
| Workers are told `.swarm/request.md` exists | r6c web-viz spent ~160 min extracting the 53k spec from run.jsonl with python | IN FLIGHT 2a D6 |
| Steer-cut turn is not a "call final_output NOW" turn | r6d q5 received the relay paired with "You MUST call the final_output tool NOW"; agent.rs arm order | LANDED `65df1cd55` (+ test proven failing on the old arm) |
| Late minis relayed to running lanes, drained during judge looks | E7 916c7414b; the drain at the loop top was unreachable during a look | LANDED in `8d6a4eb7c` |
| Repair ownership: every finding owned, criticals never unassigned | r5 and r6c: the product-killing criticals rode `unassigned`/`known_bugs` both rounds | LANDED `afae2eb1b` (replay: 0 unowned) |
| Probe artifacts are not findings | 85% (r6c) / 88% (r5) of repair node-minutes went to SIX false findings: bare `curl -X POST` receiving a correct 401/400 JSON envelope; HEAD replays 6/6 → "NOT a finding" | LANDED (GAP 3 arm, `run_spec_contract`) |
| Placement: heaviest-first onto distinct nodes; weight 1×3 | r6c stacked both hard tasks + 2 judge streams on workhorse (measured slowest, 8.28 tok/s); web-viz thought at 344 chars/min vs 1,290 alone | LANDED `623ae8eef` + config |
| Judge looks off the busy planner node | r6c 222/222 looks on workhorse | LANDED `8b03be2da` |
| Pre-fix tree snapshot at the REPAIR handover | `.swarm/best-tree` is overwritten every strictly-better verify → r6c's pre-fix tree is gone; "did the waves move the score" unmeasurable | IN FLIGHT 2a D7 (`.swarm/prefix-tree`, write-once; harness `score_run.sh --prefix` landed) |
| `spec_documented_keys` reads prose-documented keys too | keys documented as a fenced shape under a "shape below" label were invisible to the extractor (table cells only) | LANDED `857eb4ef2` |
| `planner_rank` matches device id | r6d aux order [workhorse, gabee, mihai] because mihai's model_id matched no pattern | IN FLIGHT 2a D9 |

## 3. THE BET — module shards + merger (Mihai's design, 2026-09-01)

r6c web-viz = ONE 39KB file = ONE session = 519 min; ledgerd-core 431 min; 65% of BUILD one node busy.
`split_fat_modules` had been test-only since b0dd68eac; the synthesis prompt punished dependencies into fat
tasks. IN FLIGHT 2c: fatness measured as spec sections per owned file → a PATCH request to split THAT task;
each shard works in its OWN temp folder producing PIECES (functions, sections) per its split plus a structured
README (provides / assumes / unfinished / checked-with); nobody writes the module's final file until the MERGER,
a JUDICIOUS model task with a numbered, specific brief built from a code-produced dossier (duplicates,
conflicting signatures, undefined references, unfinished items, the declared interface); it reconciles, dedupes,
fills small gaps and sends bigger ones to free nodes immediately; code checks afterwards (parse, conformance to
the interface SYNTHESIS declared as plan text, unexplained drops) and reports. Interfaces are declared, never
stubbed — CONTRACTS' measured harm was stub files, not declarations. (Mihai's design, 2026-09-01; my earlier "separate
final files" narrowing was wrong — his version avoids overwrites by construction and covers one-file modules.) Repair: one shard per finding, hunk merge of non-overlapping diffs,
no round barrier, `fix_claimed_without_edit` loud, NOT REAL must quote its replay. Expected to fail first;
tripwires: shard bytes ≪ its sections' weight + notes full of "unfinished"; `MERGE GAP` rows outpacing free nodes.

## 4. KEPT, and why

OPEN (slices consumed 1:1 by synthesis), SYNTHESIS (plan consumed 1:1), BUILD, the sink's boot fix (both
hermetic `server_runs=1.0` depend on it), VERIFY as instrument, REPAIR (Mihai: "really good idea" — it must
FIX, §2/§3), the skeleton (Mihai's walking-skeleton ask; barrier relaxed), the judge on output-tool lanes (7/7).

## 5. First-tick falsifiers (tick.py rows)

`retired_levers` lists `max_research_questions` · `research: … lanes N` == slices, `facts` > 0, `UNKINDED` 0 ·
no `phase: review`, no `replan_orientation`, no `persona_loaded` · `SKELETON DEP RELAXED` names the frontend
tasks · `SPLIT plan:` fattest ≤ ~2× median or a `plan flag fat_task` + `plan_patched` split · `SPLIT build:`
idle-STARVED 0 · `PHASE VALUE judge:` streams in flight ≪ 2.0, build looks only with a reason ·
`spec sections consumed` names Endpoints for the web slice · `SECTION CLAIMS UNMATCHED 0` · at REPAIR
`PREFIX TREE SNAPSHOT ok`, `CRITICAL UNASSIGNED` absent, `FIX CLAIMED WITHOUT EDIT` 0 · `MERGE GAP` dispatches
within the tick · pool weight 1×3, orphans 0, run_build `--sb7`, vendor 200.

## 6. Confidence, honestly

HIGH: the deletions (measured zero delivery over 2–3 runs; removing them cannot lower the score), the probe
rule, ownership, the fan cut, skeleton deps. MEDIUM: consumer routing (traced to change the r6c outcome, but a
model handed the values can still guess), the judge trigger (fewer looks = less contention; the loop-breaker
for real recurrence stays). LOWER: the shards + merger — interfaces must be right in the plan before anyone
builds; the first run is the measurement. Estimated r6e wall clock if the shards hold: ~5–6h (OPEN ~25m,
research ~35m, synthesis ~15m, BUILD 2–3h, merge+integrate ~1h, repair ~1–2h). An estimate, not a promise.
