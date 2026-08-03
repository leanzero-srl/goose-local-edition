# The queued engine fixes, written out so the freeze lift costs no thinking time

Every one of these was diagnosed under the F154 freeze and is blocked only by it. They are ordered
cheapest-first, and each carries the exact site, the exact change, and the check that decides whether
it worked. **Nothing here has been applied.** Apply in order, `cargo build -p goose-cli`, cross the
boundary once, then run the registered checks on the first fresh run.

⚠ ONE BOUNDARY FOR ALL OF THEM. Each crossing invalidates cross-build comparison (F154 froze the
campaign for exactly that reason), so these ship as a single batch, not one per tick.

---

## 1. F163 — `secs_since_any_activity` (~5 lines, `swarm.rs` + `judge.rs`)

**Why:** the deterministic stall trips key on LEVELS (`over_read_tool_calls = 16`, and
`spiral_thinking_chars` which resolves to 0 = OFF), so a worker that stops after one tool call and
857 characters is invisible to all of them. The LLM review that would catch it is skipped 60/69 times
on `no_idle_device` (F162). F160's flat-delta predicate was REFUTED by its own falsifier (F163) —
`thinking_chars` and `tool_calls` both freeze while the model streams a tool payload, i.e. while it
writes the file.

**The datum already exists** (F172): the digest is rewritten on stream activity, so its mtime IS the
last-activity time — and the judge already opens that exact file and throws the metadata away.

**Site** — `crates/goose-cli/src/commands/swarm.rs` ~16528, currently:

```rust
let digest = std::fs::read_to_string(
    cwd.join(".swarm").join("activity").join(format!("{}.json", req.task_id)),
).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
```

**Change** — bind the path once, take the mtime from the same inode:

```rust
let digest_path = cwd.join(".swarm").join("activity").join(format!("{}.json", req.task_id));
// The digest is rewritten on stream activity (coalesced ~2.5/s at :11269), so its mtime is the
// last time this worker emitted ANYTHING — thinking, text, or tool-argument bytes. That is the one
// signal that does NOT go blind while a tool payload streams, which is what refuted F160.
let secs_since_any_activity = std::fs::metadata(&digest_path)
    .ok()
    .and_then(|m| m.modified().ok())
    .and_then(|t| t.elapsed().ok())
    .map(|d| d.as_secs());
let digest = std::fs::read_to_string(&digest_path)
    .ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
```

`judge.rs`: add `pub secs_since_any_activity: Option<u64>,` next to the existing
`pub secs_since_last_write: Option<u64>,` (a sibling with the same shape — one is about OWNED-FILE
writes, this one is about ANY emission).

**Threshold: DERIVE, never pick.** The engine already has `idle_secs`, and swarm.rs:11547 already
declares a stall at `idle_secs` with no token/tool activity. Reuse that value; do not introduce a
second literal (Prime Directive 4).

**REGISTERED CHECK:** on one run, `judge_observed` carries a non-null `secs_since_any_activity`, and
it stays SMALL for a worker that is streaming a large write (the `test-meridian` case that refuted
F160: flat counters, 129 s, then a successful write).

---

## 2. F175b — emit `sink_review` even when `prewarmed == 0` (~1 line)

**Why:** at `swarm.rs` ~24448 the drain emits the event inside `if !prewarmed.is_empty()`. So
"the producer never ran" and "it ran and found nothing" are the SAME observation — DEAD and INERT
indistinguishable at the emission site, which is the exact confusion F171 exists to prevent. I nearly
published "sink_review is DEAD" because of it and was saved only by `lms ps` (F175).

**Change:** move the `sink.write_value(... "event": "sink_review" ...)` OUT of the emptiness guard;
keep the fan-out/re-verification inside it. `prewarmed: 0, survivors: 0` is a real, useful reading.

**REGISTERED CHECK:** a run with the lever ON and no findings emits `sink_review{prewarmed: 0}`
rather than nothing.

---

## 3. F182c — a "None found" report is not a finding (~1 line)

**Why:** survivor #9 of 10 read `[domain-conventions] None found — code correctly handles: (1) money
as integer minor units…`. A clean report was counted as a finding AND passed by the fail-closed
accuracy re-gate — which is correct of the re-gate (the statement is true) and wrong of the counter.
`survivors` can therefore never be zero even when the reviewers find nothing.

**Change:** drop a dimension's output at the producer when it reports nothing, before it enters the
prewarmed queue.

**REGISTERED CHECK:** no survivor string contains a "None found" / "no issues" preamble; and the
survivor count for a clean tree can reach 0.

---

## 4. F182b — feed the survivors into the repair tail, and deduplicate before counting

**Why (the strongest single argument in the campaign):** `sink_review-n3-r0` scored 0.7326 while
carrying 4-5 real, precisely-located defects **the swarm itself had already found and re-verified**
(F182: `api.py:109`, `meridian.py:141`, `meridian.py:167`, `api.py:18` — all confirmed to the line),
then discarded because the mechanism is advisory. The idle nodes did the hard part of debugging and
the run shipped the bugs anyway. This also inverts F179's cost: the sink paid 257 s/call to share its
node with reviewers whose output went nowhere.

**Change:** two parts, and the dedup must come first or the repair tail gets the same currency bug
three times (#2/#4/#7 were one issue; #1/#3 and #6/#10 also overlapped — 10 survivors ≈ 4-5 defects).

**REGISTERED CHECK:** distinct survivor count < raw count on a run that produced duplicates; and a
`complete_fix_dispatched` (or equivalent repair event) references a survivor.

⚠ RISK, stated: this converts an advisory mechanism into one that MUTATES the tree late in the run.
The re-verification is fail-closed, but the arm's own gate warns that if the build score moves DOWN
the re-gate is not fail-closed enough. **Ship 1-3 first, measure, then this.**

---

## 5. F180b — the sink cap is not a wall-clock ceiling

**Why:** `sink_capped` DID fire (F180, F115 settled), but at **+59.9 min against an 1800s cap**, and
via the **event-gap** site (11538) rather than the deadline site (11506). The deadline is only
CHECKED when the stream produces, so on a contended node emitting a token every few minutes it never
runs. 1800s let 3,594s through. Separately, `task_completed.elapsed_ms` records the CAP (30.0 min)
rather than the real duration (59.9) — across 14 sinks, 6 recorded < real, worst understatement 29.9
min, which is what corrupted F152's statistics.

**Change:** a real timer (`tokio::select!` on the deadline) so the ceiling holds regardless of stream
cadence; and record the true elapsed on a capped completion.

**REGISTERED CHECK:** a capped sink's `elapsed_ms` matches its dispatch→completion timestamps to
within a few seconds, and no sink exceeds `sink_cap_secs` by more than one call.

---

## 6. F179b — keep idle-fill off the critical path

**Why:** PARALLEL is 2 per node, so `idle_dimension_review` lands on the SAME device serving the
sink, and the sink is the LAST task — everything waits on it. Measured: baseline sink 63 s/call
against sink_review's 257 s/call, fleet median 83. The sink WAITS; it does not work harder.

**Change:** exclude the device currently serving `integrate-verify` from `pick_sink_review`'s
candidate list.

⚠ n=1 per arm. **The settling measurement is baseline r1/r2's sink s/call from dispatch→completion
timestamps**, inside the retention window (F176: retention is under 3 h at this fleet's volume).
Do not ship this before that number exists.

---

## 7. F157 — the TOOLS block's test-author bullets reach every implementer

**Why (F183-confirmed on two runs):** the block is **4,450 chars, BYTE-IDENTICAL on all 11
file-owning prompts of r1**, 35-48% of each. Two of its longest bullets are written for a test
author: the exemption *"any test file YOU OWN is your deliverable"* (an implementer owns none) and
*"the MOMENT your file's tests pass … do NOT re-run pytest more than ~2 times"* (its tests are a
SIBLING's deliverable and often do not exist yet).

**Change:** gate both bullets on `is_test_author`, which F139/F146 already use. Also fix the 13-char
stray indent on the `NEVER run cd` bullet.

**REGISTERED CHECK (no new literal — Lesson 34):** both bullets appear in test-author prompts and
NOT in implementer prompts, on the SAME run.

---

## 8. F158 — every non-Python owner gets the Python conventions

**Why (F183-confirmed on two runs):** 4 non-Python owners on r1, **all 4 carrying the CONVENTIONS
block**, including the pure `index.html` owner. On r0 the `web` task fired 4 conventions — as many as
`store.py` — receiving banker's rounding, `-7//2 == -4`, SQL BETWEEN, pagination offsets, 1,857 chars
of frozen Python signatures and *"run `python3 -m pytest`"*: ~48% of its prompt about a language it
is not writing. r0 measured `kind_mismatch` at **84.0%**.

**Cause:** `relevant_pitfalls` (swarm.rs:9507) keyword-matches `description + owned_files`. The
description carries every trigger; the filename can only ever ADD. **A fact that can only broaden is
not a scoping fact.**

**Change:** derive the deliverable's language from the owned file extensions (`TargetLang` already
exists and F146 gates on it) and skip a convention whose language no owned file is written in.

**REGISTERED CHECK:** `KNOWN-CORRECT CONVENTIONS` ABSENT for a worker whose owned files are all
non-`.py`, still PRESENT for `store`/`api`.

---

## 9. F165 — give the judge an ACCEPT

**Why:** `test-meridian` was recorded a TERMINAL FAILURE while its file was on disk with 8 test
functions, 12 assertions, **all passing** — and contributed 8 of the 72 tests the crunched app passes
(F169). The engine's own final hint said so before killing it: *"Nothing is reported failing, so
`tests/test_meridian.py` is most likely already done and you are polishing."* The action attached to
that verdict is `failed`. **The judge has no ACCEPT; its only lever is kill, and the third kill is
terminal.**

**Change:** on a `looping` verdict where the owned files exist and nothing is reported failing,
FINISH the task rather than spending its last attempt.

**REGISTERED CHECK:** `failures.py`'s test-author row moves. That row — not a pooled score — is the
campaign's improvement metric (F164: implementers 0/63 failed, test-authors 13/42 = 31%).

---

## ANCHOR VERIFICATION — all ten sites confirmed present in source (2026-08-03 04:1x)

Checked before the freeze lifts, so the batch applies without a hunt. Every string these patches key
on still exists in the tree:

    OK  F163   `let digest = std::fs::read_to_string(`            swarm.rs
    OK  F163   `pub secs_since_last_write: Option<u64>,`          judge.rs   (the sibling field)
    OK  F175b  `if !prewarmed.is_empty() {`                       swarm.rs   (the guard to move out of)
    OK  F175b  `"event": "sink_review",`                          swarm.rs
    OK  F180b  `"event": "sink_capped",`  x2 — BOTH sites         swarm.rs   (11506 deadline, 11538 gap)
    OK  F158   `fn relevant_pitfalls(task_text: &str)`            swarm.rs
    OK  F157   `TOOLS & ENVIRONMENT`                              swarm.rs
    OK  F157   `any test file YOU OWN is your deliverable`        swarm.rs   (bullet 1 to gate)
    OK  F157   `STOP WHEN GREEN`                                  swarm.rs   (bullet 2 to gate)
    OK  F165   the `looping` verdict                              judge.rs

The `sink_capped` count being exactly 2 matters: F180 established that the one which fired was the
**event-gap** site, not the deadline site, so F180b must fix the deadline path and both sites must
still be accounted for after the change.

⚠ These anchors were verified against the CURRENT tree while the engine is FROZEN. If any further
`crates/**` edit lands before the batch is applied, re-run this check — a moved anchor is a silent
mis-apply, and there are nine patches to get wrong.

---

## 10. F191b — `is_still_producing` must require ACTION, not reasoning (~2 lines, `judge.rs`)

**Why:** the finalize-spin trip (judge.rs:434) exists precisely for a worker that wrote its file and
then stopped touching it. On r2, `test-api` attempt 0 met FOUR of its five conditions — file written
at 408 s, not the sink, elapsed 1878 s, untouched far beyond 420 s — and spent **595 s making ZERO
tool calls while `thinking_chars` climbed 2,897 → 22,627**. It was never killed, and the task took
three dispatches.

The fifth condition blocked it:

```rust
fn is_still_producing(input: &JudgeInput) -> bool {
    match (input.prev_thinking_chars, input.worker_thinking_chars) {
        (Some(prev), Some(now)) => now > prev,
        _ => false,
    }
}
```

**A reasoning spiral is monotonically growing reasoning by definition, so `is_still_producing` is
permanently true for it and the trip can never fire on the pathology it was written to catch.** I
added this guard in F144 ("GREW = no kill; FLAT = kill") and it was right for its case — F163 later
proved a flat-kill would have killed healthy workers streaming a tool payload.

**Change:** require growth in something that represents ACTION. Thinking growth with a frozen
`tool_calls` and an untouched file is the spiral signature, not progress:

```rust
// Thinking-only growth is NOT progress — it is the spiral signature (F191). A worker streaming a
// tool payload has FLAT thinking but WILL advance tool_calls when the call lands, so keying on
// action preserves F163's protection while closing the immunity this granted every spiral.
fn is_still_producing(input: &JudgeInput) -> bool {
    match (input.prev_tool_calls, input.worker_tool_calls) {
        (Some(prev), Some(now)) => now > prev,
        _ => false,
    }
}
```

This needs `prev_tool_calls` alongside the existing `prev_thinking_chars` on `JudgeInput`, mirrored
in the dispatcher's per-task `judge_prev_*` map exactly as `prev_thinking_chars` already is.

⚠ **INTERACTION WITH F163 — CHECK THIS EXPLICITLY.** F163's refutation case (`test-meridian` flat at
1,209 chars / 0 calls for 129 s, then a successful write) must STILL survive: it had flat thinking
AND flat tool_calls, so `is_still_producing` returns false either way — but the finalize-spin trip
also requires `any_owned_written`, which was FALSE there. It is protected by a different condition.
**Verify that before shipping**, because getting it wrong re-introduces exactly the false kill F163
was registered to prevent.

**REGISTERED CHECK:** on one run, a worker with a written file, `secs_since_last_write > 420`,
growing `thinking_chars` and FLAT `tool_calls` receives a `Looping` verdict rather than running to
its attempt cap. Falsifier: a healthy worker killed while mid-write — which would mean action-growth
is also the wrong proxy and the trip needs the digest mtime from F163 instead.

---

## ANCHOR VERIFICATION #2 — F191b (added after the first check, now confirmed) + a COST CORRECTION

`is_still_producing` confirmed present with exactly the body the patch quotes:

    fn is_still_producing(input: &JudgeInput) -> bool {
        match (input.prev_thinking_chars, input.worker_thinking_chars) {
            (Some(prev), Some(now)) => now > prev,
            _ => false,
        }
    }

Fields the replacement needs, all present:
    judge.rs:81   `pub worker_tool_calls: Option<u32>,`      <- what the new version reads
    judge.rs:99   `pub prev_thinking_chars: Option<u64>,`    <- the sibling to mirror

**⚠ COST CORRECTED: "~2 lines in judge.rs" was WRONG.** The `prev_*` value is not carried on
`JudgeInput` alone — the dispatcher maintains it in a per-task map with THREE sites that must all be
mirrored for `prev_tool_calls`:

    swarm.rs:10867  `judge_prev_thinking: Mutex<HashMap<String, u64>>,`   declaration
    swarm.rs:10955  `judge_prev_thinking: Mutex::new(HashMap::new()),`    construction
    swarm.rs:16558  `let mut g = self.judge_prev_thinking.lock()...`      read-then-write per observation

So F191b is **~6 lines across 2 files with 3 mirror sites**, not 2 lines in one. Re-costing after
reading the surface is the same discipline that took F163 from "a digest-writer change with a large
blast radius" down to three lines — it moves in both directions, and the estimate written before the
search is a guess either way.

This does not change F191b's priority: it is still the fix for a defect that cost three dispatches of
one task on r2, and the cost is still small. It changes what "applied correctly" means — miss the
read-then-write at :16558 and `prev_tool_calls` is permanently `None`, `is_still_producing` returns
`false` for everything, and the finalize-spin trip fires on healthy workers. **That failure mode is
silent and is exactly the false kill F163 exists to prevent, so verify all three sites landed.**

## 11. The `## API of` paste is truncated mid-identifier and fenced as complete

**Site** `crates/goose-cli/src/commands/swarm.rs:19638`

```rust
let capped: String = api_source.chars().take(dep_budget.min(3500)).collect();
```

then :19641 wraps it as ``` "```\n{capped}\n```" ``` unconditionally.

**Measured (F196)** three of four blocks on a live test-author prompt end mid-token —
`meridian.py` at `    def _up`, `api.py` at `        self._se` — and the pasted body **fails
`ast.parse`**. No block carries a truncation notice. The same prompt forbids the worker to `cat`
the real file, so the missing remainder is unrecoverable by any permitted action.

**Change** cut on a line boundary and say so, instead of cutting mid-token and lying by omission:

```rust
let budget = dep_budget.min(3500);
let full: String = api_source.chars().take(budget).collect();
let capped = if full.chars().count() < api_source.chars().count() {
    let head = full.rsplit_once('\n').map(|(h, _)| h).unwrap_or(&full);
    format!("{head}\n# … TRUNCATED — this is a PARTIAL view of {f}; the rest is not shown")
} else {
    full
};
```

**Why it is not merely cosmetic** a body that stops mid-`def` inside a closed fence is
indistinguishable from a complete file. The worker cannot know it is missing anything, cannot open
the file to find out, and the most likely response to an unreadable dependency is to reason rather
than write — which is exactly the F191 spiral signature.

**Registered check** re-run F196's extraction on the next post-crossing run: every `## API of`
block's fenced body must either `ast.parse` clean or carry the `TRUNCATED` marker. Today: 1 of 4
fails to parse, 0 of 4 carry a marker.

**Interaction with the `dep_signatures` arm** independent. Extraction shrinks most bodies below the
cap, but a large declaration surface still hits `min(3500)` and would be cut the same way. Both are
needed; neither depends on the other.

## 12. The over-read hint asserts the worker has APIs the engine truncated

**Site** `crates/goose-swarm/src/judge.rs:355` (hint text) and its input struct.

**Measured (F196 + F197)** all 11 `over_reading` verdicts ever recorded are on test-authors
(0 of 85 implementer verdicts), and test-authors are the only kind receiving `## API of` blocks.
Three of four of those blocks end mid-token; one fails `ast.parse`. The hint tells that worker
*"you already have the spec, the file layout, and the injected dependency APIs"* — the engine
computed the truncation at `swarm.rs:19638` and then denies it.

**Change** carry the fact the engine already has. Add `truncated_deps: Vec<String>` to `JudgeInput`,
populated from the same `capped` computation, and branch the hint:

```rust
let hint = if input.truncated_deps.is_empty() {
    "…you already have the spec, the file layout, and the injected dependency APIs. WRITE …"
} else {
    &format!("You have written nothing yet. Note: {} was injected TRUNCATED, so reading it \
              directly is legitimate — do that ONCE, then WRITE your owned file(s).",
             input.truncated_deps.join(", "))
};
```

**Why it is not cosmetic** the worker is being told its unusable input is sufficient. Under
PRIME DIRECTIVE 2 a hint that contradicts what the engine knows is the generic failure in its
purest form — the engine holds the fact and emits a canned sentence instead.

**Registered check** on the next post-crossing run, every `over_reading` verdict on a task whose
dependency paste was truncated must carry the truncated-dep hint. Today: 11 of 11 carry the generic
text.

**⚠ ORDERING** patch #11 (line-boundary cut + marker) reduces how often #12 triggers but does not
replace it — a dependency larger than the cap is still truncated, correctly marked, and the worker
still needs permission to read it.

**⚠ RE-SCOPED AND DEPRIORITISED (F200).** The false sentence lives at `judge.rs:355` (needs 16 tool
calls — never reached; measured max was 2) and `judge.rs:381` (the #134 spiral trip, OFF at
`spiral_thinking_chars: 0`). **Neither branch fires at the shipping config**, so this changes nothing
today; the branch that DOES fire (`judge.rs:418`) already composes an accurate hint from counts via
`no_file_hint`. Keep the patch — it goes live the moment either branch is armed — but it ranks below
#11 and #13. The note below is superseded on provenance and kept for the audit trail:

**~~PRECONDITION ANSWERED — PROMOTED, NOT GATED (F198)~~.** The gate asked whether the verdict is
ever consumed. It is: **all 11 `over_reading` verdicts carry `action = re_dispatch`** at confidence
0.90. F197's "0 kill events" was my own query error — the scheduler stamps the action as a FIELD on
`judge_verdict` (`scheduler.rs:1414`), it does not emit a separate kill event. So this hint is not
decoration: **it is the message a killed worker is restarted with**, and it tells that worker the
truncated paste is sufficient. Of the 11 tasks hit, none finished in one attempt (8 took 3, 3 took
2) and 2 never completed.

## 13. `JudgeVerdict` drops the provenance bit that decides whether it is a fact or a guess

**Site** `crates/goose-swarm/src/scheduler.rs:1421` and `:1441` (both `SwarmEvent::JudgeVerdict`
constructions), event definition in `event.rs`.

**Measured (F199)** all 11 `over_reading` verdicts fired at 0-2 tool calls against a threshold of
16, so `deterministic_verdict` cannot have produced them — they are LLM judge opinions steering
re-dispatches at confidence 0.90. But that is a DEDUCTION from the guard's arithmetic, not a
reading: the emitted event's fields are `action, confidence, device, event, hint, run_id, seq,
task_id, ts, verdict` and **provenance is not among them**. `JudgeOutcome.deterministic` exists
(`judge.rs:126`, "True only for a verdict produced by `deterministic_verdict` — a real engine
fact"), is used for the terminal-fail gate, and is then discarded.

**Change** add `deterministic: bool` to the `JudgeVerdict` event and stamp `outcome.deterministic`
at both emit sites.

**Why it matters beyond tidiness** the campaign's standing rule is that only a deterministic engine
event may confer or retract a verdict. That rule is currently unauditable from the run log: every
analysis has to re-derive provenance from thresholds, which is exactly how I got F197 wrong. One
bool makes "was this an engine fact or a weak model's guess" a lookup instead of an argument.

**Registered check** on the next post-crossing run, every `judge_verdict` carries `deterministic`,
and every `over_reading` with `tool_calls < over_read_tool_calls` reports `deterministic: false`.
Today: the field is absent on all 302 verdicts.

## 14. The verdict label says `over_reading` for a worker that read nothing

**Site** `crates/goose-swarm/src/judge.rs:418-427` (the 420s deadline branch) and `Verdict` in the
same file.

**Measured (F199 + F200)** all 11 trips fired with `tool_calls` 0-2 (median 0) at elapsed 420-485s,
floor exactly 420. The branch computes `let read_nothing = input.worker_tool_calls == Some(0);` and
uses it to compose an accurate hint — then stamps `Verdict::OverReading` regardless. The run log
therefore records "over_reading" about workers that ran no command at all.

**Change** add a `Verdict::NoFirstWrite` (serialising as `no_first_write`) and select on the flag the
branch already computes:

```rust
verdict: if read_nothing { Verdict::NoFirstWrite } else { Verdict::OverReading },
```

**Why it matters** this label is the primary key of every downstream analysis. It sent me down a
false causal chain three separate times (F197's trap story, F198's retraction, F199's provenance
deduction) before I checked the tool-call column. A run log that misdescribes its own trips costs
every future reader the same three ticks. The engine already has the bit; it just does not use it —
the same shape as F196's unused `extract_signatures`.

**Registered check** on the next post-crossing run, no `judge_verdict` carries `over_reading` with
`tool_calls == 0`. Today: 9 of 11 do.
