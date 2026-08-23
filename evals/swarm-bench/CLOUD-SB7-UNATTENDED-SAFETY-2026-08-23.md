# Cloud SB7 unattended-safety closure

Date: 2026-08-23

## Scope and operator gate

This ledger covers only the frozen five-model SB7 cloud campaign and its
orchestrator. It does not change SB7, its scorer, public `sb-7.0`, the two local
fleet documents, or local-model execution. No provider call, hermetic score, or
website write is part of the preparation and recovery transitions.

The operator gate recorded on 2026-08-23 is absolute: neither a cloud run nor a
local run may start without fresh explicit permission. A green launch review
therefore produces a sealed, paused campaign and exact launch command; it does
not release a provider or local-model process.

## Failure model closed by this change

The adversarial review identified independent crash windows that could combine
into duplicate spend or a falsely successful unattended run:

- generation-2 accounting could confuse permanently carried ambiguous requests
  with requests created by the current episode;
- a one-time monitor check could not prove continued supervision;
- direct manager, supervisor, score, and publication entry points could bypass
  an `ATTENTION` transition;
- detached children could execute before their identities were durably written;
- a foreground smoke caller could disappear without leaving an owner;
- provider `Popen` failure could consume the only remaining episode ordinal;
- `ATTENTION` had no monitor-first autonomous resume;
- exact assistant-text recurrence was incorrectly allowed to become a
  deterministic runtime stop decision;
- deleting the progress ledger could discard that history and restart the
  recurrence classifier from a clean baseline;
- the publisher concealed a raw provisional `sb-7.0-rc` verdict as stable
  `sb-7.0` and removed its calibration disclosure;
- recovery evidence was not bound to clean committed source and allowed weak
  artifact identity.

The controller now treats the exact five source request IDs as an immutable
generation-2 baseline and refuses a missing baseline or any additional current
reservation. The manager is bound to a renewable monitor lease; every provider
admission and scorer/publication write boundary checks it. Monitor failure moves
the campaign to `ATTENTION` and stops recorded runtime groups. Direct entry
points fail closed outside the permitted campaign state.

Detached runtime children wait behind a unique atomic receipt containing the
child PID, parent PID, process identity, and token. If the parent dies before
the receipt exists, the child exits without executing the target command. The
provider episode counter is reclaimed only when process creation itself failed
and no provider process could have run.

Smoke has its own durable manager. It can re-adopt a dead smoke manager and,
after all five proofs pass, hands ownership directly to a detached monitor.
`ATTENTION` resume restores monitor ownership before manager ownership.

Assistant-text recurrence is now observation only. The meter still persists
its measured windows, sentences, fingerprints, and earlier matching sequence,
but it has no fail-stop field and no return channel into `ATTENTION`. Content,
duration, token volume, and silence therefore have no deterministic stop
authority. Goose's existing judge/nudge path remains the semantic supervisor.
Only closed instrument or orchestrator defects can stop the cloud runtime.

## Evidence-based progress supervision

There is deliberately no elapsed-duration, token-volume, or silence kill cap.
Each monitor tick persists a closed-schema, hash-chained observation for every
entrant. The observation binds campaign and smoke identity, process and provider
request generation, lifecycle counts, exact log and telemetry evidence, raw
tree evidence, and assistant-only semantic recurrence. Provider silence and
local-process silence are visible classifications but never automatic failure.
Tool-only output advances artifact evidence without being misclassified as
assistant recurrence. Repeated sentences followed by novel productive output
remain ordinary measured progress.

The ledger now has an independent random identity and identity-file hash
committed into monitor state with the last complete per-entrant head set. On
monitor restart and again at each append boundary, the complete ledger replays
against that commitment. Deleting a ledger or committed entrant record,
replacing the ledger identity bytes, missing sequences, foreign records, schema
drift, hash-chain tamper, invalid orphan tails, and evidence paths outside the
entrant unit all fail closed as instrument defects.

Publication now preserves the hermetic identity exactly:
`scorer_version`, the complete calibration string, and derived provisional
status. A dry-run publisher must emit one machine-readable identity receipt
that matches those values byte-for-value. The live publisher process cannot
start unless that receipt and its log hash remain sealed. The currently pinned
website publisher emits no such receipt and maps `sb-7.0-rc` to `sb-7.0`, so it
is deliberately refused before any live write. Remote and rendered verification
also require the exact RC/calibration/provisional identity; a future website
schema and publisher must represent all three before publication can resume.

## Verification and launch boundary

The combined warning-as-error suite currently passes 128 tests, including real
detached-process exercises for monitor death, receipt loss, measured growth and
silence, recurrence neutrality, ledger deletion/tamper refusal,
crash-before-head recovery, exact RC publication identity refusal, and
smoke-manager relaunch. The clean commit still requires a separate adversarial GO verdict and
freshly generated recovery evidence whose `fix_source_commit` equals that clean
HEAD.

Only after those checks and the operator's fresh permission may the one-time
recovery transition be materialized and `smoke` invoked. `smoke` owns the
monitor handoff; `monitor-start` and `start` are not separate operator steps.
Hermetic scoring stays serial. Publication remains blocked until the public
schema and pinned publisher preserve the exact hermetic scorer, calibration,
and provisional identity.
