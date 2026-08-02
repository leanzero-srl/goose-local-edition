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
