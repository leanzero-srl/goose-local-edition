# Judge batch — VA-052 + VA-056 + VA-058 (swarm-surgeon; dispatch ONLY after r6e ends — cargo starves the local node mid-run)

Surface: `crates/goose-cli/src/commands/swarm/supervision.rs` (parse_judge_reply ~825), `swarm/ladder.rs` (judge_summon_trigger), the judge nudge/restream bookkeeping in `swarm.rs` (grep `judge_restream`, `nudge:`/"first nudge", `judge_look_dispatched` trigger "cadence"/"growth"). Read the surrounding functions WHOLE first (gate 8 reading half) and name what was read in the commit.

## VA-052 — parser
- `VERDICT=OK|CONFIDENCE=HIGH|ESTABLISHED=…|NEXT=…` (LABEL=value) is recorded `drifting` (no keyword → Drifting default). Accept LABEL=value and LABEL/value and the two-line `VERDICT|OK|HIGH|…\nNEXT|…` form.
- The OK arm returns `JudgeOutcome::ok()` and DROPS ESTABLISHED/NEXT — 14 of 14 OK events on r6e research carry established='' next=''. Keep both on OK.
- `VERDICT=RESTART` must never read as drifting.
- Test: the r6e look-1 text (run.jsonl seq 11 of swarm-20260901-141137451, archived) parses OK with next non-empty; a RESTART text parses RESTART.

## VA-056 — cadence looks on planner-side (output-tool) lanes
Measured r6e: OPEN 12 looks / 0 nudges (34 node-min); RESEARCH 19 looks by 18:25 → 46 looks by 18:56, 3 nudges, 0 acted on, ~151 node-min, EVERY research look ran on a node generating a sibling lane (inflight=1). VA-013 made BUILD/REPAIR lanes evidence-only; output-tool lanes kept every trigger. Verdict under gate 9: NOT EARNING on 58 looks.
- Make output-tool lanes evidence-only too: recurrence meter, forming stall (a `final_output` frame at args 0 beyond the lane's own first-emit history), or a repeated judge NEXT — "emit now" across two looks IS the evidence; deliver it as the nudge instead of a third look.
- First-look DRIFTING on a FRESH lane keyed on "0 tool calls in N chars" (api-boot look 1 at 9 min: "Call the output tool NOW" while the lane's own tail already said "Write it as a handoff"; frame formed 83 s later) → key on a prior look's ESTABLISHED covering every [qN], never on tool-call count.
- Do NOT add a clock, count cap or seconds constant (gates 1/5). Delete the cadence trigger's mechanism for these lanes; do not gate it.

## VA-058 — steer acknowledged, never acted; restream re-buys the derivation
Measured r6e viz3d: 18:38:20 steer ("Your next message must be a tool call: invoke the output tool and emit your [qN] table now") → lane wrote "The supervisor note says … my next message must be a tool call … Let me construct the answer carefully" then 10k chars, no call; 18:45:38 restream ("steer ignored: no write progress") abandoned 22,621 chars, lane RE-DERIVED the shader plan and tie cases ("settled g=150 → 82.5! … banker's 82, half-away 83"); 18:48:14 post-restream look recorded `nudge: 1, reason: "first nudge"` — the COUNTER RESET with the restreamed call; emitted on its own at 18:58:05 (1895 s).
- Counter reset on restream is a plain defect: nudge history belongs to the TASK attempt, not the stream.
- The restream must carry the abandoned reasoning's SETTLED content forward (it carried 238+2000 chars of 22.6k) — or not restream a lane that is composing (its own tail said "let me write the final output").
- REFUTER FIRST on the "harness-formed prefilled output-tool draft from the judge's ESTABLISHED" idea: the draft must be the lane's OWN established words (never judge-invented content — fallback gate); if it cannot be, drop the idea and keep the two fixes above.
- Prompt: compose the [qN] table INSIDE the tool arguments, not in reasoning first.

## Gate 8 trace (mandatory in the commit message)
Walk r6e's judge-research-viz3d-engine sequence (looks at 18:38:20 / 18:41:50 held / 18:42:58 held / 18:45:38 restream / 18:48:14 "first nudge" / 4× restream_held to 18:56) and OPEN's 12 + research's 46 looks through the new branches; end with TRACE VERDICT YES at <event> / NO, ships as a NET.

## Proof
`cargo fmt`; `cargo test -p goose-swarm --test development_gates` (ratchets: SWARM_RS_LINE_BASELINE, UNWRAP_OR_DEFAULT_BASELINE — adjust only downward); `cargo test -p goose-cli swarm --no-fail-fast` (doc-tests included); `cargo clippy --all-targets -- -D warnings`. Commit --only the touched files with the trailers. Then works-prover on the parser (feed the archived r6e judge texts).

## VA-061 — steer text shaped by lane kind (added tick 5, 19:26)
Measured r6e split-viz3d-engine: judge look 2 (19:16:09) steered "emit that vs7dbg API table plus the cross-shard dependency list as your structured reply NOW — submit this partial version and refine after"; look 4 NEXT "ONLY the vs7dbg API table header plus rows 24–31 … no shared-state section". A one-shot structured-output lane (split, synthesis, opener, research) CANNOT refine after — its single final_output IS the deliverable, and a partial interface starves the merger. The judge's NEXT for an output-tool lane must be "emit the COMPLETE <schema> now" and never "partial/refine after"; the prompt carries the lane kind. Trace: the split lane's 20→30 min of composing after that steer.
