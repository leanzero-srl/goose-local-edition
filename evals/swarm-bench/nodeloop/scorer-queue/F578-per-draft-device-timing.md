QUEUED ENGINE INSTRUMENT — specified, deliberately NOT coded while the binary is frozen.

WHAT IS MISSING. `skeleton_drafts` emits `requested`, `returned`, `dead`, `straggler_aborted`,
`secs` (the ROUND total) and `chars` (per candidate). It does NOT emit which DEVICE produced which
draft, nor how long each draft took.

WHY THAT SPECIFIC GAP MATTERS. The draft round is the only place in the whole engine where every
device is handed an IDENTICAL prompt at the SAME MOMENT. It is therefore the system's only matched
device-speed comparison — everywhere else the scheduler routes by preferred model and availability,
so each device receives a different task mix and any per-device latency is confounded by what it was
asked to do.

EARNED, NOT SPECULATED. I tried to answer "does the fan wait for a slow node?" from
`task_completed.elapsed_ms` and the attempt refuted itself. Pooled medians across every real
archived cell: mac-gabee 149.9s (n=22), worksmacstudio 177.9s (n=76), local-mihai 274.6s (n=26) —
gabee FASTEST, the opposite of the 25.88-vs-32.08 tok/s ordering quoted in a source comment. And the
ranking is unstable run to run:

    baseline-n3-r0    mac 256s  <  local 331s  <  worksmacstudio 412s      spread 1.61x
    baseline-n3-r2    worksmacstudio 184s  <  local 217s  <  mac 235s      spread 1.28x
    baseline-n3-r3    worksmacstudio 64s   <  mac 111s    <  local 337s    spread 5.29x

A device that is fastest in one run and slowest in the next is not being measured for speed. That
instrument is broken for this question and cannot be fixed by more cells.

THE CHANGE. Carry `(device_id, secs)` alongside each draft out of `draft_round` /
`collect_drafts_with_straggler_stop`, and add to the `skeleton_drafts` event:

    "drafts": [{"device": "...", "secs": 123, "chars": 6192, "valid": true}, ...]

⚠️ NOT WRITTEN AS CODE, ON PURPOSE. This is a signature change through
`collect_drafts_with_straggler_stop` (JoinSet payload `Option<String>` -> a tuple) and its unit
tests, and the binary is frozen so it cannot be compiled. F560 and F567 were queued as commits
because they are additive JSON fields that cannot break a build; this one can. Committing code I
cannot compile into a queue that must build cleanly is how the queue gets poisoned — so it is
specified here and implemented at the rebuild.

WHAT IT SETTLES IN ONE LINE ONCE IT LANDS: whether the 3-node draft round (236-270s) costs more than
the 1-node round (163s) because one device lags, or because three concurrent drafts simply draw a
worse maximum from the same latency distribution. Those imply completely different fixes — routing
away from a slow node, versus lowering N or arming the straggler grace sooner.
