# Swarm work pause checkpoint — 2026-08-23

## Active resume update — 2026-08-23, post-review

Mihai has returned and authorized both local and cloud launches once their technical gates are
actually green. The launch gate is still closed. No benchmark, scorer, publisher, or website write
has started. The orphaned read-only `tail` left from the old Qwen run was stopped; no model run was
affected.

Current integration HEAD is `106926a28` on `codex/swarm-provider-boundary`. The cross-process
provider lease landed at `3cec04ff3`; lower-layer LM Studio/OpenAI HTTP integration is in progress.
Its required authority shape is now explicit: the engine mints the provider request ID, returns a
non-Clone live request object, exposes only a borrowed HTTP scope, and consumes the live object for
terminal/abandon while returning it on retryable settlement failure. A receipt is evidence, never a
replacement for possession of that live request object.

Canonical-planning correction `4c7d1716f` is pushed. It runs semantic adjudication after every
nonempty valid first round (including one-node), binds immutable raw spec plus explicit user
decisions, retains raw inter-draft agreement beside typed adjudication state, requires one primary
owner plus typed applies/verifies coverage for every binding requirement, removes task-name
exemptions, restricts model IDs to the resolved roster, and checks path/ownership/DAG/integration
closure before acceptance. Its focused tests and isolated warnings-as-errors clippy pass; a fresh
independent adversarial review is active. Legacy/incremental round two remains fallback only when
adjudication fails or reports a material conflict.

The semantic-nudge evidence slice passed its focused tests but failed adversarial review on four P0s
and remains uncommitted. Its repair must use opaque engine-minted task/acceptance and provider-start
capabilities, bind the exact source request into snapshot production, make review completion opaque
and bind the canonical parsed reply to the full reviewer lifecycle, and atomically consume an
eligibility against engine-held current lifecycle/newest-trace/replay state. Required negative
replays include forged acceptance, one snapshot bound to two requests, CONTINUE rewritten as NUDGE,
cross-admission terminal splice, source terminal after review, newer trace after review, and duplicate
redemption. Plain equality against a caller-returned boundary is not freshness.

Cloud harness commits `6dddf93e5` and `a91c9187b` correct and adversarially harden the publication
contract while preserving the raw
hermetic identity. The dry-run and live publication chain now seals the raw scorer/calibration/
provisional evidence separately from the authorized public mapping (`scorerVersion=sb-7.0`, with no
public calibration or provisional fields), and remote/rendered verification rejects case-insensitive
RC/calibration/provisional leakage, duplicate dry-run identity lines, and unrelated JSON-LD. All 203
warnings-as-errors offline tests and the actual website publisher's five unit tests pass. The fresh
adversarial review passed; the isolated scorer-only Chromium/macOS sandbox smoke is active as the
final cloud runtime gate. The website itself stays untouched until a real score is ready.

Local integration commit `fc36a1afba9d46aa483c7fbed352838df778eb0b` seals the exact OpenAI HTTP
protocol and final transport immediately before POST, reserves the engine-minted provider request
against the global physical authority, and consumes terminal ownership without fabricating release
on drop or ambiguous failure. Owned gates pass: 90 provider tests, six HTTP-boundary adversarial
replays, 20 authority replays, ten lifecycle tests, formatting, and warnings-as-errors clippy. The
full swarm suite advanced through 194 tests and exposed one remaining semantic panic-path assertion,
which belongs to the active semantic one-shot authority correction.

Next integration order: complete and independently review canonical planning; complete and
independently review semantic one-shot redemption; port causal repair only after those authorities
exist; clear the cloud publication review and run the real scorer-only sandbox smoke; then run
consolidated gates and launch the authorized local/cloud campaigns under their monitors.

## Resume update — 2026-08-23

Mihai returned and explicitly authorized local and cloud launches once the offline implementation gates are genuinely ready. No model benchmark, hermetic scorer, publisher, or website process has started yet. Authorization is open; the technical launch gate remains closed until the blockers below pass.

Local integration is clean and pushed at `eddb04385` on `codex/swarm-provider-boundary`. Commit `5e4a561bb` proves provider-protocol terminal authority, bare-EOF quarantine, provider-owned start-failure provenance, exact model/transport admission, the fail-latched lifecycle journal, and engine-memory semantic activity authority keyed by an engine-minted admission publisher. `.swarm/activity` is only a best-effort atomic UI mirror: disk spoofing and mirror failure cannot become judge evidence, while malformed or mismatched authoritative writes fail closed. Commit `eddb04385` adds run/phase namespaces, monotonic phase epochs, exact source authority in semantic publishers, and an explicit scheduler execution role so a Build retry remains Build and Repair may begin at attempt zero. This does not certify launch readiness.

The namespace slice passed all 78 Goose Swarm unit tests, all integration suites (including 22 broker, 20 physical-control, 12 semantic-control, and the scheduler/corpus replays), 26 relevant Goose CLI lifecycle/semantic tests, formatting, and all-target `-D warnings` clippy for `goose-swarm` and `goose-cli`. New replays prove identical task IDs in separate run or phase lineages do not collide, a newer epoch supersedes an older queued authority without laundering it, and scheduler roles are never inferred from attempt numbers.

Remaining local P0 gates:

- Replace the project-local lifecycle journal with an engine-owned cross-process global provider lease. It must serialize all runs and working directories by exact physical host/model-instance/transport/capacity identity, survive crash, reject symlinks/tampering, and require explicit reset/reconciliation for unresolved starts.
- Create one run-scoped `PhysicalRuntime` for main, COMPLETE, repair, persona, overview, and custom helper calls. It owns one control and semantic plane, supports a generic admitted `submit_operation`, never drains between DAGs, and settles exactly once after submissions close.
- Repair must use a distinct `SemanticRepairAcceptanceReceipt`; ordinary semantic observations remain evidence-only. Salvage receipts bind an explicit workspace/shadow authority and never ambient cwd.
- Rewrite repair promotion as prepare -> admitted semantic acceptance -> crash-atomic commit. The authoritative PREPARED/COMMITTING/COMMITTED WAL, rollback bytes, and interprocess writer lock live outside the project/tool/ruler namespace. Kill-after-every-boundary replays must yield exactly the parent or child tree, never a mixture.
- Freeze an immutable ruler contract and run it on disposable clones with isolated caches/output, scrubbed secrets, and controlled network. The candidate tree is hashed before ruling; ruler side effects (including `npm install`, generated files, caches, or `.swarm` tampering) cannot become promoted bytes. The landed tree must equal the pre-ruler candidate hash and pass the same ruler in a fresh clone.

Cloud hardening continues under adversarial review. Current launch blockers include immutable lifecycle history across retries, scorer-runtime identity bound into the smoke/qualification lineage, entrant/scorer isolation with a parent-only verdict channel, an exact post-cleanup SCORED evidence seal, resume/monitor launch serialization, nonempty process identities, and stop/publication ownership checks. Cloud runs remain stopped until the offline suite and reviewer both clear these boundaries.

The older pause snapshot below is historical context. Where it conflicts with this resume update, this section is authoritative.

## Invariants that must survive resume

- The public website remains stable `sb-7.0`, not an RC label.
- The two local-fleet document IDs remain under SB7: `brun-fleet-qwen38-sb70` and `brun-fleet-qwen-sb70`.
- Do not touch the website before a real hermetic score exists and launch/publish permission is explicit.
- Never restore deterministic model kills or fixed output/time caps as a substitute for semantic supervision.
- API credentials supplied in chat are not written here or committed anywhere.

## Worktrees and durable commits

- Local integration: `/Users/mihaiperdum/Projects/goose-engine-integration`, branch `codex/swarm-provider-boundary`. Pre-checkpoint HEAD was `1fd5c37e2`.
- Cloud integration: `/Users/mihaiperdum/Projects/goose-cloud-harness`, branch `codex/cloud-sb7-harness`, clean at `985b6ff40`.
- Cloud runtime hardening: `/Users/mihaiperdum/Projects/goose-cloud-runtime-integrity`, branch `codex/cloud-runtime-integrity`. Durable commits are `402a51da9`, `f45449306`, `a879421eb`, `774a8ffa6`, and pause checkpoint `49a994ac1`. The worktree is clean and pushed; the interrupted final full-suite rerun has not certified the checkpoint.
- Repair causal promotion: `/Users/mihaiperdum/Projects/goose-repair-causal-promotion`, branch `codex/swarm-repair-causal-promotion`, clean at `7609e71a3`; cherry-pick `a2533d341` then `7609e71a3` only after the local provider-boundary slice is complete.
- Website correction is already committed separately as `694927b`; do not alter it during this implementation pause.

## Local integration state

The current checkpoint is intentionally not build-ready. It preserves the complete provider-boundary and semantic-observation work plus an adversarial hardening pass that stopped midway. Earlier focused checks passed before the latest changes, but none of those earlier results certify the current tree.

Implemented or substantially implemented:

- same-run physical transport provenance carried through fleet lanes and admissions;
- positive endpoint model membership before a configured route may become verified physical evidence;
- task-local provider lifecycle accounting around physical main-build calls;
- single-attempt provider dispatch to avoid hiding internal retries;
- unresolved occupancy on dropped, aborted, pre-stream ambiguous, and mid-stream ambiguous provider calls;
- physical main-build semantic observations without giving observations action authority;
- legacy fix-round supervision kept separate from physical main-build substitutions;
- long-reach repetition evidence and observation-only semantic review wiring;
- SHA-256-only provider transport IDs enforced at physical fleet snapshot validation;
- model-aware OpenAI endpoint identity started so chat-completions and Responses API routes cannot share a false identity;
- typed semantic-review terminal-versus-unresolved lifecycle handling started.

The exact partial seam at pause:

- `Provider::transport_identity` now takes `model_name`; production and most fixtures were updated.
- `SingleAttemptFailureProvenance` was added to the provider trait and OpenAI received a provider-owned classifier.
- The OpenAI file still needs the new type imported.
- `swarm_provider_lifecycle.rs` and `swarm_semantic.rs` still call the old generic `provider_error_proves_terminal_response`; remove that helper and use each provider's `single_attempt_failure_provenance` instead.
- Test mock providers that deliberately prove a terminal response must opt into `TerminalResponse`; all other mocks should inherit the fail-closed `Unresolved` default.
- Re-run `rg -n "provider_error_proves_terminal_response|transport_identity\\("` after completing the seam.

## Adversarial findings still to close

1. Provider failure provenance must remain provider-owned. A shared `ProviderError` variant is not generic proof of a remote terminal response. Defaults must stay unresolved.
2. `ApiClient::transport_identity` must include the actual `default_query` in the hashed URL and return `None` when an opaque request-builder decorator could mutate the URL. Update its credential-safety test: a query changes the digest but never appears in serialized evidence.
3. Physical semantic supervision depends on `.swarm/activity` freshness. A startup write probe alone is insufficient because later ENOSPC/I/O errors are currently discarded. Do not simply propagate `?` from inside an active provider stream: that can drop an unproven remote decode and intentionally wedge its permit. Introduce a persistent, observable activity-sink health state so later write failures make semantic supervision explicitly degraded/unavailable without fabricating a provider terminal.
4. Keep physical-main supervision substitution scoped to main execute. Planning, research, and legacy repair/fix rounds retain their existing judge behavior until their own physical lifecycle is proved.
5. Repair promotion commits provide provisional/salvaged state and causal receipts, but no producer yet emits cited `SemanticRepairReview`; legacy scalar promotion and whole-tree fan/join/ship-best paths are not yet transactionally converted. Do not claim live parity.

## Resume sequence

1. Confirm the launch gate is still closed and inspect all four worktrees before editing.
2. Finish provider-owned failure provenance and model-aware endpoint identity, including query/decorator fail-closed tests.
3. Design and implement persistent activity-sink failure evidence without terminating an unresolved provider request.
4. Run `cargo fmt`, `git diff --check`, targeted provider/lifecycle/physical-broker/semantic tests, then `cargo check -p goose-providers -p goose-swarm -p goose-cli`.
5. Commit and push the completed local provider-boundary slice.
6. Cherry-pick `a2533d341` and `7609e71a3`, resolving `swarm.rs` by preserving both lifecycle and causal-repair changes; then run focused and consolidated gates.
7. Resume the cloud runtime worktree at `49a994ac1`, inspect the checkpoint diff, complete its interrupted full suite, and add any required correction commit; then cherry-pick the five runtime-hardening commits onto `codex/cloud-sb7-harness`.
8. Run final offline clippy/test gates and adversarial review. Do not start models.
9. Clean only exact regenerable target directories. The first cleanup already reclaimed about 131 GB; at pause the local integration target was about 16 GB and the data volume had about 114 GiB free.
10. Report readiness and wait for Mihai's explicit permission before launching either local or cloud benchmarks.

## Pause verification

All three worker/reviewer agents were interrupted. A process scan showed no Cargo build/test, cloud harness test, benchmark, scorer, publisher, or model process owned by these worktrees.
